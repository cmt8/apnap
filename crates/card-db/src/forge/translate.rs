//! Compile a Forge card script into a [`CardDef`] (decisions.md D-046).
//!
//! The rule is strict: a card compiles only if **every** line, parameter, filter property
//! and expression of its script is understood. Anything else is reported as a *blocker* (a
//! short feature name such as `api:Fight` or `param:Pump.Duration`), never dropped or
//! guessed. `harness forge-coverage` counts blockers over all cards to decide what to build
//! next.
//!
//! What each script feature means is documented in docs/forge/ (written from Forge's
//! sources); what it compiles to here must have a test in this crate or in the engine.

use super::parse::{Face, Record, Script};
use card_model::{
    AbilityDef, ActivationZone, Affected, AltCost, AltKind, AltKindName, Amount, AsEnters,
    BasicLandType, Body, Calc, CardDef, CardDest, CardType, CardZone, ChoiceOf, Cmp, Color,
    CombatRule, Condition, ContinuousEffect, CopyExcept, CopyWhat, Cost, CostChoice, CounterKind,
    DamageBy, DamagePer, DamageTo, DelayWindow, EachPlayerAction, Effect, Keyword, Layout,
    ManaCost, ManaProduce, ManaRestriction, ManaType, Measure, Mode, ObjFilter, ObjRef, OfChosen,
    PlayZone, PlayerRef, PlayerRule, ReplaceWith, Replaces, Selector, Signed, Supertype, Take,
    TargetFilter, TargetGroup, TargetRef, Timing, Trigger, TriggerCause, TriggerStep, Who,
};
use std::cell::RefCell;
use std::collections::BTreeSet;

/// Why a script doesn't compile: the features it needs that aren't supported yet.
pub type Blockers = BTreeSet<String>;

/// A compiled card, and the token scripts its effects create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    pub def: CardDef,
    pub tokens: BTreeSet<String>,
}

/// Parameters that never affect the rules: descriptions shown to players, prompts, and
/// hints for Forge's AI. (docs/forge/script-format.md, "keys a translator can ignore".)
const TEXT_ONLY: [&str; 21] = [
    // A hint for Forge's AI (respond to its own activation): never read by forge-game.
    "AIRespondsToOwnAbility",
    // How the cost reads in the ability's description (`SpellAbility.getDescription`):
    // the cost itself is `Cost$`.
    "CostDesc",
    "ValidTgtsDesc",
    "UnlessAI",
    "ChangeTypeDesc",
    "ChangeNumDesc",
    "DefinedDesc",
    "SelectPrompt",
    "OptionalPrompt",
    "SpellDescription",
    "TriggerDescription",
    "StackDescription",
    "Description",
    "ValidDescription",
    "TgtPrompt",
    "AILogic",
    "AINoRecursiveCheck",
    "IsCurse",
    "PrecostDesc",
    // Which targets Forge's AI prefers ("destroy target permanent", aimed at expensive
    // ones): never read by Forge's rules code (docs/forge/script-format.md, "AI only").
    "AITgts",
    "AITgtsStrict",
];

/// A record being read: remembers which parameters were looked at, so that any that
/// weren't can be reported.
struct Rd<'a> {
    rec: &'a Record,
    what: String,
    seen: RefCell<BTreeSet<&'a str>>,
}

impl<'a> Rd<'a> {
    fn new(rec: &'a Record, what: impl Into<String>) -> Rd<'a> {
        Rd {
            rec,
            what: what.into(),
            seen: RefCell::new(BTreeSet::new()),
        }
    }

    fn get(&self, key: &'a str) -> Option<&'a str> {
        self.seen.borrow_mut().insert(key);
        self.rec.get(key)
    }

    /// Report every parameter that wasn't read.
    fn finish(&self, blockers: &mut Raised) {
        let seen = self.seen.borrow();
        for (i, (k, _)) in self.rec.params.iter().enumerate() {
            let k = k.as_str();
            // Forge takes the last of two parameters with the same key; rather than rely
            // on that, a record that repeats a key isn't compiled.
            if self.rec.params[..i].iter().any(|(other, _)| other == k) {
                blockers.insert(format!("param:{}.{k} (twice)", self.what));
            }
            if seen.contains(k)
                || TEXT_ONLY.contains(&k)
                || matches!(k, "SP" | "AB" | "DB" | "ST" | "Mode" | "Event")
            {
                continue;
            }
            blockers.insert(format!("param:{}.{k}", self.what));
        }
    }
}

/// One alternative of a filter ("Zombie.Other+YouCtrl").
// Short-lived values while one filter is read: the size difference costs nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Alt {
    /// `Card.Self`
    This,
    Sel(Selector),
}

/// Where a move of "those cards" takes them (Forge's zone name), if `e` is one.
fn noted_moved(e: &Effect) -> Option<&'static str> {
    match e {
        Effect::PutInto {
            what: ObjRef::Noted,
            ..
        }
        | Effect::Exile {
            what: ObjRef::Noted,
        }
        | Effect::ReturnToHand {
            what: ObjRef::Noted,
        }
        | Effect::Destroy {
            what: ObjRef::Noted,
        } => noted_zone(e),
        _ => None,
    }
}

/// Forge's `AtEOT$` value (docs/forge/effects.md §0.6): the action (`Sacrifice` — by the
/// effect's controller — or `Exile`), and when: the next end step; `Your…` your next end
/// step; `…Combat` the next end of combat. Anything else is not read.
fn at_eot(what: &str) -> Option<(&'static str, TriggerStep, Who)> {
    let (whose, rest) = match what.strip_prefix("Your") {
        Some(rest) => (Who::You, rest),
        None => (Who::Any, what),
    };
    let (rest, step) = match rest.strip_suffix("Combat") {
        Some(rest) => (rest, TriggerStep::EndCombat),
        None => (rest, TriggerStep::EndStep),
    };
    let action = match rest {
        "Sacrifice" => "Sacrifice",
        "Exile" => "Exile",
        _ => return None,
    };
    Some((action, step, whose))
}

/// Is what the chain last noted one object — a single target moved, one token made?
fn notes_one(chain: &Chain) -> bool {
    let single = |i: &u8| {
        !chain
            .groups
            .iter()
            .any(|g| g.count > 1 && (g.first..g.first + g.count).contains(i))
    };
    let Some(inner) = chain.effects.iter().rev().find_map(|e| match e {
        Effect::Note(inner) => Some(inner),
        _ => None,
    }) else {
        return false;
    };
    match &**inner {
        Effect::Exile {
            what: ObjRef::Target(i),
        }
        | Effect::ReturnToHand {
            what: ObjRef::Target(i),
        }
        | Effect::Destroy {
            what: ObjRef::Target(i),
        }
        | Effect::PutInto {
            what: ObjRef::Target(i),
            ..
        } => single(i),
        Effect::CopyPermanent {
            what: CopyWhat::Obj(_),
            count: Amount::Fixed(1),
            ..
        } => true,
        _ => false,
    }
}

/// The targets and effects of one mode, as they are collected along a sub-ability chain.
#[derive(Default)]
struct Chain {
    /// Where the cards the chain has noted so far are (Forge's zone name), once it has
    /// noted any (D-069).
    noted_at: Option<&'static str>,
    targets: Vec<TargetFilter>,
    effects: Vec<Effect>,
    /// "…where X is [amount]": the script variable that `Amount::X` stands for.
    x: Option<(String, Amount)>,
    /// The instruction (its place in `effects`) that first uses that X: the amount is
    /// worked out as that instruction is applied (CR 608.2h), not before the ones ahead
    /// of it ("draw a card, then … equal to the number of cards in your hand").
    x_at: usize,
    /// Some link said `Defined$ Targeted` — in Forge, every target of the whole chain.
    refers_to_targets: std::cell::Cell<bool>,
    /// A link moved a target to another zone: the target is no longer that object
    /// (CR 400.7), and Forge's `Defined$ Targeted` (which follows the card) means
    /// something the engine's target slot doesn't.
    /// (Its place in `effects`, and the zone it went to.) A later reference to it is the
    /// card where that link put it — found because that link is noted (CR 400.7j).
    target_moved: Option<(usize, &'static str)>,
    /// A later link referred to the moved target: note the link that moved it.
    note_moved_target: std::cell::Cell<bool>,
    /// "Two target creatures", "up to two target creatures": slots that are one "target".
    groups: Vec<TargetGroup>,
    /// The group the record being compiled has just made: (first slot, slots).
    fresh_group: Option<(u8, u8)>,
    /// A "counter target spell" link remembered its target (`RememberTargets$`) or that
    /// spell's mana value (`RememberCounteredCMC$`): its target slot, and which.
    countered: Option<(u8, bool, bool)>,
}

/// The blockers raised while one card is compiled: each name once, and a count of every
/// raise — so "did this line raise anything?" (`len` before and after) holds even when a
/// later line raises a name an earlier one already did (two abilities with the same
/// unsupported parameter are both explained).
#[derive(Clone, Debug, Default)]
struct Raised {
    set: Blockers,
    raised: usize,
}

impl Raised {
    fn insert(&mut self, b: String) -> bool {
        self.raised += 1;
        self.set.insert(b)
    }

    /// How many raises so far (not how many names).
    fn len(&self) -> usize {
        self.raised
    }

    fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

struct Tx<'a> {
    face: &'a Face,
    blockers: Raised,
    used_svars: BTreeSet<String>,
    tokens: BTreeSet<String>,
    has_x_cost: bool,
    /// Reading a spell's own `Cost$` (its additional costs): "pay X life" is X announced.
    spell_cost: bool,
    /// A characteristic-defining ability defines the card's (power, toughness).
    defines_pt: (bool, bool),
    /// Compiling the body of a triggered ability (where "X" is not the X being paid).
    in_trigger: bool,
    /// The zone the activated ability being compiled works from ("this card" is then the
    /// card in that hand or graveyard).
    from_zone: ActivationZone,
    /// What the event of the trigger being compiled lets its ability refer to.
    trig: TrigCtx,
    /// The trigger being compiled is "you may" as a whole (`OptionalDecider$`).
    optional_trigger: bool,
    /// Compiling the spell ability of an instant or sorcery ("exile this spell").
    in_spell: bool,
    /// Additional costs of the spell, found on its `SP$` line.
    additional: Vec<AbilityDef>,
    /// The filter of the card's `K:Enchant:<filter>` line, if it is an Aura.
    enchant: Option<&'a str>,
    /// The card is an Equipment (its statics may speak of the "equipped creature").
    equipment: bool,
    /// The script variables that hold abilities, parsed.
    records: &'a [(String, Record)],
    known: &'a Known,
    /// Abilities the card gives to other objects (`CardDef::granted`), as compiled so far.
    granted: Vec<AbilityDef>,
    /// Compiling an ability that another object is given (how deep): "this" is whatever
    /// has it, a permanent on the battlefield.
    granting: u8,
    /// Compiling what an effect object carries: `EffectSource` is "this" (the source of
    /// the spell or ability that made it) and `IsRemembered` what it is about.
    in_effect: bool,
    /// The number of targets of the chain whose amount is being read (`None` outside an
    /// effect's amount): `Targeted$…` is about its one target.
    chain_targets: Option<usize>,
    /// An amount just read is about the chain's target (`Targeted$…`).
    read_target: bool,
    /// The trigger bodies (by the name of what they execute) in which "its controller"
    /// was taken for "you" because the event's object is the source itself: a second
    /// event added to such an ability must be about the source too.
    it_is_you: BTreeSet<String>,
    /// The body being compiled took the event's object for the source (see `it_is_you`).
    took_it_for_this: bool,
    /// The trigger bodies in which "that player" was taken for "you": a second event
    /// added to such an ability must happen to you too.
    player_is_you: BTreeSet<String>,
    /// The body being compiled took "that player" for "you".
    took_player_for_you: bool,
    /// The line just compiled means nothing for the rules (only hints for Forge's AI): no
    /// ability, and nothing blocked.
    nothing: bool,
    /// Compiling a chain whose "remembered" cards are its own (D-069): it remembers, and
    /// its last link forgets. Only there do `Remembered`, `IsRemembered`, `Remembered$…`
    /// mean "those cards".
    memory: bool,
    /// The zone of the objects the delayed ability being compiled remembers: its chain
    /// starts with them noted (taken by the chain's first mode).
    delayed_noted: Option<&'static str>,
    /// Compiling a delayed ability that remembers objects.
    in_delayed: bool,
}

/// The parameters with which a link of a chain remembers cards on the card (Forge's
/// `Remember…$`) that the compiler knows. Any other (`RememberRevealed$`, …) is not read
/// and blocks the card.
const REMEMBER: [&str; 9] = [
    // A countered spell's mana value, for a delayed trigger of the chain (CR 603.7c).
    "RememberCounteredCMC",
    "RememberChanged",
    "RememberTargets",
    "RememberLKI",
    "RememberDestroyed",
    "RememberSacrificed",
    "RememberTokens",
    "RememberMilled",
    "RememberDiscarded",
];

/// Where "those cards" an instruction noted are, by Forge's zone names.
fn noted_zone(e: &Effect) -> Option<&'static str> {
    let dest = |d: &CardDest| match d {
        CardDest::Hand => "Hand",
        CardDest::Battlefield { .. } => "Battlefield",
        CardDest::Graveyard => "Graveyard",
        CardDest::Exile => "Exile",
        CardDest::LibraryTop | CardDest::LibraryBottom | CardDest::Stay => "Library",
    };
    Some(match e {
        Effect::Exile { .. } => "Exile",
        Effect::ReturnToHand { .. } => "Hand",
        Effect::ReturnToBattlefield { .. } | Effect::CreateToken { .. } => "Battlefield",
        Effect::PutInto { to, .. } | Effect::ChooseCards { to, .. } | Effect::Dig { to, .. } => {
            dest(to)
        }
        Effect::Destroy { .. }
        | Effect::DestroyAll(_)
        | Effect::Sacrifice { .. }
        | Effect::SacrificeThis
        | Effect::EachPlayer(EachPlayerAction::SacrificeCreature(_))
        | Effect::EachPlayer(EachPlayerAction::Discard(_))
        | Effect::Discard { .. }
        | Effect::DiscardHand { .. }
        | Effect::Mill { .. } => "Graveyard",
        Effect::Together(v) => {
            let zones: Vec<_> = v.iter().map(noted_zone).collect();
            return zones
                .first()
                .copied()
                .flatten()
                .filter(|z| zones.iter().all(|x| *x == Some(*z)));
        }
        Effect::May(inner) => return noted_zone(inner),
        _ => return None,
    })
}

/// The kinds of ability a script gives to other objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Given {
    /// `AddAbility$` / `Abilities$`: an activated ability (`AB$ …`).
    Activated,
    /// `AddTrigger$` / `Triggers$`
    Triggered,
    /// `AddStaticAbility$` / `staticAbilities$`
    Static,
    /// `AddReplacementEffect$` / `Replacements$`
    Replacement,
}

/// A rule about one creature that Forge writes as a keyword and a card as a sentence:
/// "can't block", "prevent all damage that would be dealt to [it]". Not an ability of the
/// creature (CR 113.12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hidden {
    CantAttack,
    CantBlock,
    /// Prevent all [combat] damage that would be dealt to it, or (`by_it`) by it.
    Prevent {
        by_it: bool,
        combat: Option<bool>,
    },
}

/// The rules a keyword text stands for, if it is one of those (with or without Forge's
/// `HIDDEN` mark, which only keeps it off the card's keyword line).
fn hidden_rules(text: &str) -> Option<Vec<Hidden>> {
    let text = text.strip_prefix("HIDDEN ").unwrap_or(text).trim();
    let text = text.strip_suffix('.').unwrap_or(text);
    let prevent = |by_it, combat| Hidden::Prevent { by_it, combat };
    Some(match text {
        "CARDNAME can't block" => vec![Hidden::CantBlock],
        "CARDNAME can't attack" => vec![Hidden::CantAttack],
        "CARDNAME can't attack or block" => vec![Hidden::CantAttack, Hidden::CantBlock],
        "Prevent all damage that would be dealt to CARDNAME" => vec![prevent(false, None)],
        "Prevent all damage that would be dealt by CARDNAME" => vec![prevent(true, None)],
        "Prevent all combat damage that would be dealt to CARDNAME" => {
            vec![prevent(false, Some(true))]
        }
        "Prevent all combat damage that would be dealt by CARDNAME" => {
            vec![prevent(true, Some(true))]
        }
        "Prevent all damage that would be dealt to and dealt by CARDNAME" => {
            vec![prevent(false, None), prevent(true, None)]
        }
        "Prevent all combat damage that would be dealt to and dealt by CARDNAME" => {
            vec![prevent(false, Some(true)), prevent(true, Some(true))]
        }
        _ => return None,
    })
}

/// A hidden rule as an ability about `affected`. `None` if these can't be told apart from
/// other objects in a damage filter (several selectors at once).
fn hidden_ability(rule: Hidden, affected: &Affected) -> Option<AbilityDef> {
    let it = || match affected {
        Affected::This => Some(ObjFilter::This),
        Affected::Enchanted => Some(ObjFilter::Host),
        Affected::Remembered => Some(ObjFilter::Remembered),
        Affected::All(sel) => Some(ObjFilter::Matching(sel.clone())),
    };
    Some(match rule {
        Hidden::CantAttack => AbilityDef::CombatRule {
            affected: affected.clone(),
            rule: CombatRule::CantAttack,
        },
        Hidden::CantBlock => AbilityDef::CombatRule {
            affected: affected.clone(),
            rule: CombatRule::CantBlock,
        },
        Hidden::Prevent { by_it, combat } => AbilityDef::Replacement {
            replaces: Replaces::Damage {
                by: if by_it { Some(it()?) } else { None },
                to: if by_it {
                    DamageTo::Anything
                } else {
                    DamageTo::Permanent(it()?)
                },
                combat,
            },
            with: ReplaceWith::Prevent,
        },
    })
}

/// Script variables that only tell Forge's AI something, by the name a card that is given
/// them knows them under (`AddSVar$` of a variable written `SVar:<name>:<value>`).
const AI_HINTS: [&str; 12] = [
    "HasAttackEffect",
    "HasCombatEffect",
    "MustBeBlocked",
    "MustAttack",
    "EquipMe",
    "DontEquip",
    "EnchantMe",
    "SacMe",
    "DestroyWhenDamaged",
    "AITapDown",
    "EndOfTurnLeavePlay",
    "Targeting",
];

/// What a trigger's event gives its ability to refer to (Forge's `Triggered…` names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TrigCtx {
    /// Names of the event's object ("it"): `TriggeredCard`, `TriggeredAttacker`, … (each
    /// also with `LKICopy` appended).
    it: &'static [&'static str],
    /// Names that mean the ability's own source.
    this: &'static [&'static str],
    /// The names of "that player": the player the event happened to (also written as
    /// the controller or owner of a card that is always theirs, like a card they drew).
    players: &'static [&'static str],
    /// "It" is the ability's own source ("when this creature dies"): its controller is
    /// "you" (CR 603.3a — the controller of the source when it triggered).
    it_is_this: bool,
    /// "That player" is always the ability's controller ("at the beginning of your
    /// upkeep", "whenever you draw a card"): it is "you".
    player_is_you: bool,
    /// The expression for "that much".
    amount: Option<&'static str>,
    /// A permanent died: `TriggeredNewCardLKICopy` is the card it became in the graveyard.
    died: bool,
    /// … and that permanent was the ability's own source.
    died_self: bool,
}

/// What one of Forge's `Triggered…` names stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Named {
    It,
    This,
}

impl Default for TrigCtx {
    fn default() -> Self {
        TrigCtx {
            it: &["TriggeredCard"],
            this: &[],
            players: &[],
            it_is_this: false,
            player_is_you: false,
            amount: None,
            died: false,
            died_self: false,
        }
    }
}

impl TrigCtx {
    const NOTHING: TrigCtx = TrigCtx {
        it: &[],
        this: &[],
        players: &[],
        it_is_this: false,
        player_is_you: false,
        amount: None,
        died: false,
        died_self: false,
    };

    fn named(&self, name: &str) -> Option<Named> {
        let base = name.strip_suffix("LKICopy").unwrap_or(name);
        if self.it.contains(&base) {
            Some(Named::It)
        } else if self.this.contains(&base) {
            Some(Named::This)
        } else {
            None
        }
    }

    fn of(when: &Trigger) -> TrigCtx {
        let about_source = |f: &ObjFilter| matches!(f, ObjFilter::This | ObjFilter::Host);
        match when {
            Trigger::Damage { by, to, per, .. } => {
                let player = matches!(to, DamageTo::Player(_));
                let own = *by == Some(ObjFilter::This);
                TrigCtx {
                    // The permanent dealt damage; or the one source of damage dealt to a
                    // player.
                    it: match to {
                        DamageTo::Permanent(_) => &["TriggeredTarget"],
                        DamageTo::Player(_) if !own && *per != DamagePer::Recipient => {
                            &["TriggeredSource"]
                        }
                        _ => &[],
                    },
                    this: if own { &["TriggeredSource"] } else { &[] },
                    players: if player { &["TriggeredTarget"] } else { &[] },
                    player_is_you: matches!(to, DamageTo::Player(Who::You)),
                    amount: Some("TriggerCount$DamageAmount"),
                    ..TrigCtx::NOTHING
                }
            }
            Trigger::GainsLife(who) | Trigger::LosesLife(who) => TrigCtx {
                players: &["TriggeredPlayer"],
                player_is_you: *who == Who::You,
                amount: Some("TriggerCount$LifeAmount"),
                ..TrigCtx::NOTHING
            },
            // The card drawn is in its owner's hand; the card discarded is in its owner's
            // graveyard: its controller and owner are the player who drew or discarded it.
            Trigger::Draws { who, .. } | Trigger::Discards(who) => TrigCtx {
                players: &[
                    "TriggeredPlayer",
                    "TriggeredCardController",
                    "TriggeredCardOwner",
                ],
                player_is_you: *who == Who::You,
                ..TrigCtx::NOTHING
            },
            // Forge's `SpellCast`: `TriggeredActivator` is who cast it, and so is the
            // controller of the spell (`TriggeredCard`). The spell itself is no "it" here.
            Trigger::Cast { by, .. } => TrigCtx {
                players: &["TriggeredActivator", "TriggeredCardController"],
                player_is_you: *by == Who::You,
                ..TrigCtx::NOTHING
            },
            // Forge's `Phase`: `TriggeredPlayer` is the player whose step it is.
            Trigger::BeginningOf { whose, .. } => TrigCtx {
                players: &["TriggeredPlayer"],
                player_is_you: *whose == Who::You,
                ..TrigCtx::NOTHING
            },
            Trigger::PlayerAttacks { .. } => TrigCtx::NOTHING,
            // Forge's `Attacks`: `TriggeredDefendingPlayer` is the player it attacks (this
            // engine's creatures attack players only).
            Trigger::Attacks(f) | Trigger::AttacksAlone(f) => TrigCtx {
                it: &["TriggeredCard", "TriggeredAttacker"],
                it_is_this: *f == ObjFilter::This,
                players: &["TriggeredDefendingPlayer"],
                ..TrigCtx::NOTHING
            },
            Trigger::Blocks(_) => TrigCtx {
                it: &["TriggeredBlocker"],
                ..TrigCtx::NOTHING
            },
            // Forge's `Taps`: `TriggeredCard` is the permanent that became tapped.
            Trigger::BecomesTapped(f) => TrigCtx {
                it: &["TriggeredCard"],
                it_is_this: *f == ObjFilter::This,
                ..TrigCtx::NOTHING
            },
            // Forge's `AttackerBlocked` / `AttackerUnblocked`: `TriggeredDefendingPlayer` is
            // the player the attacker attacks.
            Trigger::BecomesBlocked(f) | Trigger::Unblocked(f) => TrigCtx {
                it: &["TriggeredAttacker"],
                it_is_this: *f == ObjFilter::This,
                players: &["TriggeredDefendingPlayer"],
                ..TrigCtx::NOTHING
            },
            // "It" is the other creature.
            Trigger::BlockedBy { attacker, blocker } => {
                if about_source(blocker) {
                    TrigCtx {
                        it: &["TriggeredAttacker"],
                        this: if *blocker == ObjFilter::This {
                            &["TriggeredBlocker"]
                        } else {
                            &[]
                        },
                        ..TrigCtx::NOTHING
                    }
                } else {
                    TrigCtx {
                        it: &["TriggeredBlocker"],
                        this: if *attacker == ObjFilter::This {
                            &["TriggeredAttacker"]
                        } else {
                            &[]
                        },
                        ..TrigCtx::NOTHING
                    }
                }
            }
            // Several events for one ability: only what all of them give alike.
            Trigger::Any(v) => {
                let mut all = v.iter().map(TrigCtx::of);
                match all.next() {
                    Some(first) if all.all(|c| c == first) => first,
                    _ => TrigCtx::NOTHING,
                }
            }
            Trigger::Dies(f) => TrigCtx {
                died: true,
                died_self: *f == ObjFilter::This,
                it_is_this: *f == ObjFilter::This,
                ..TrigCtx::default()
            },
            Trigger::Enters(f) | Trigger::Leaves(f) => TrigCtx {
                it_is_this: *f == ObjFilter::This,
                ..TrigCtx::default()
            },
            _ => TrigCtx::default(),
        }
    }

    fn is_player(&self, name: &str) -> bool {
        self.players.contains(&name)
    }
}

/// A selector for cards outside the battlefield: there a card has no controller, and the
/// selector's `controller` stands for its owner ("a creature card you own in a graveyard").
fn owned(mut s: Selector) -> Selector {
    if s.owner != Who::Any {
        s.controller = std::mem::take(&mut s.owner);
    }
    s
}

/// A cost names the card only where the ability works from: `{T}` and "sacrifice this" on
/// the battlefield, "discard this card" in the hand, "exile this card from your graveyard"
/// in the graveyard.
fn zone_cost_fits(cost: &Cost, from: ActivationZone) -> bool {
    let on_battlefield = cost.tap || cost.sacrifice_self;
    match from {
        ActivationZone::Battlefield => !cost.discard_self && !cost.exile_self,
        ActivationZone::Hand => !on_battlefield && !cost.exile_self,
        ActivationZone::Graveyard => !on_battlefield && !cost.discard_self,
    }
}

/// What the compiler needs to know about the whole card pool.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Known {
    /// Every subtype (creature type, land type, artifact type, …) that exists. A word in a
    /// filter is taken as a subtype only if it is one of these — otherwise it is some
    /// property the compiler doesn't know, and the card is blocked rather than misread.
    pub subtypes: BTreeSet<String>,
}

impl Known {
    /// The subtypes used on the type lines of `scripts` (card or token script texts).
    pub fn from_scripts<'s>(scripts: impl Iterator<Item = &'s str>) -> Known {
        let mut subtypes = BTreeSet::new();
        for text in scripts {
            for line in text.lines() {
                let Some(words) = line.strip_prefix("Types:") else {
                    continue;
                };
                for w in words.split_whitespace() {
                    let super_or_card = card_type(w).is_some()
                        || matches!(
                            w,
                            "Basic" | "Legendary" | "Snow" | "World" | "Ongoing" | "Tribal"
                        );
                    // Words Forge's filters use for players and for kinds of object are
                    // never read as subtypes, whatever a joke card's type line says
                    // ("Planeswalker — You"): "You have hexproof" is not about permanents.
                    let filter_word = matches!(
                        w,
                        "You" | "Opponent" | "Player" | "Card" | "Permanent" | "Spell" | "Emblem"
                    );
                    if !super_or_card && !filter_word {
                        subtypes.insert(w.to_string());
                    }
                }
            }
        }
        Known { subtypes }
    }
}

const CARD_TYPES: [(&str, CardType); 9] = [
    ("Artifact", CardType::Artifact),
    ("Battle", CardType::Battle),
    ("Creature", CardType::Creature),
    ("Enchantment", CardType::Enchantment),
    ("Instant", CardType::Instant),
    ("Kindred", CardType::Kindred),
    ("Land", CardType::Land),
    ("Planeswalker", CardType::Planeswalker),
    ("Sorcery", CardType::Sorcery),
];

/// The tokens of a cost: separated by spaces, except inside `<…>`
/// (`Sac<1/Creature.Other/another creature>` is one token).
fn cost_tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut depth) = (None, 0u32);
    for (i, c) in text.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                out.push(&text[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    out
}

/// CR 205.3i (2026-11-06): the land types.
const LAND_TYPES: [&str; 17] = [
    "Cave",
    "Desert",
    "Forest",
    "Gate",
    "Island",
    "Lair",
    "Locus",
    "Mine",
    "Mountain",
    "Plains",
    "Planet",
    "Power-Plant",
    "Sphere",
    "Swamp",
    "Tower",
    "Town",
    "Urza's",
];

/// A `Mode$ Continuous` line with nothing but whom it affects and hints for Forge's AI.
fn only_ai_hints(rec: &Record) -> bool {
    rec.params.iter().all(|(k, _)| {
        matches!(
            k.as_str(),
            "Mode" | "Affected" | "AffectedDefined" | "AddSVar" | "Description"
        )
    })
}

/// A word of a filter that names players, not cards (`You`, `Player.Opponent`).
fn player_word(head: &str) -> bool {
    matches!(head, "You" | "Player" | "Opponent")
}

fn card_type(word: &str) -> Option<CardType> {
    CARD_TYPES.iter().find(|(n, _)| *n == word).map(|(_, t)| *t)
}

fn color(word: &str) -> Option<Color> {
    Some(match word {
        "White" => Color::White,
        "Blue" => Color::Blue,
        "Black" => Color::Black,
        "Red" => Color::Red,
        "Green" => Color::Green,
        _ => return None,
    })
}

/// The keywords the engine implements, by their name in a `K:` line or a `KW$` parameter.
pub fn keyword(name: &str) -> Option<Keyword> {
    Some(match name {
        "Deathtouch" => Keyword::Deathtouch,
        "Double Strike" => Keyword::DoubleStrike,
        "First Strike" => Keyword::FirstStrike,
        "Flash" => Keyword::Flash,
        "Flying" => Keyword::Flying,
        "Haste" => Keyword::Haste,
        "Lifelink" => Keyword::Lifelink,
        "Menace" => Keyword::Menace,
        "Reach" => Keyword::Reach,
        "Trample" => Keyword::Trample,
        "Vigilance" => Keyword::Vigilance,
        "Defender" => Keyword::Defender,
        "Fear" => Keyword::Fear,
        "Hexproof" => Keyword::Hexproof,
        "Indestructible" => Keyword::Indestructible,
        "Intimidate" => Keyword::Intimidate,
        "Shroud" => Keyword::Shroud,
        "Shadow" => Keyword::Shadow,
        "Horsemanship" => Keyword::Horsemanship,
        "Landwalk:Plains" => Keyword::Landwalk(BasicLandType::Plains),
        "Landwalk:Island" => Keyword::Landwalk(BasicLandType::Island),
        "Landwalk:Swamp" => Keyword::Landwalk(BasicLandType::Swamp),
        "Landwalk:Mountain" => Keyword::Landwalk(BasicLandType::Mountain),
        "Landwalk:Forest" => Keyword::Landwalk(BasicLandType::Forest),
        _ => return None,
    })
}

/// `ManaCost:` → our cost text. `Ok(None)` for "no cost".
pub fn mana_cost(text: &str) -> Result<Option<ManaCost>, String> {
    if text == "no cost" {
        return Ok(None);
    }
    let mut out = String::new();
    for shard in text.split_whitespace() {
        let ok = shard.parse::<u32>().is_ok()
            || matches!(shard, "W" | "U" | "B" | "R" | "G" | "C" | "X");
        if !ok {
            return Err(format!("mana:{shard}"));
        }
        out.push_str(&format!("{{{shard}}}"));
    }
    ManaCost::parse(&out)
        .map(Some)
        .map_err(|_| format!("mana:{text}"))
}

impl<'a> Tx<'a> {
    fn block(&mut self, what: impl Into<String>) {
        self.blockers.insert(what.into());
    }

    /// A line that produced nothing must have said why. If no blocker was recorded since
    /// `before` (a count of blockers), the line would silently vanish from the card: that
    /// is a hole in the compiler, and the card is blocked for it.
    fn explained(&mut self, before: usize, what: &str) {
        if self.blockers.len() == before {
            self.block(format!("unexplained:{what}"));
        }
    }

    fn svar(&mut self, name: &str) -> Option<&'a str> {
        let v = self.face.svars.get(name)?;
        self.used_svars.insert(name.to_string());
        Some(v.as_str())
    }

    // ------------------------------------------------------------------ filters

    /// One alternative of a filter. `None` (with a blocker recorded) if not understood.
    fn alt(&mut self, text: &str) -> Option<Alt> {
        let mut parts = text.split('.');
        let head = parts.next().unwrap_or("").trim();
        let props: Vec<&str> = parts
            .flat_map(|p| p.split('+'))
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let mut s = Selector::default();
        match head {
            "Card" | "Permanent" => {}
            _ => match card_type(head) {
                Some(t) => s.types.push(t),
                None if self.known.subtypes.contains(head) => s.subtypes.push(head.to_string()),
                None => {
                    self.block(format!("filter:{head}"));
                    return None;
                }
            },
        }
        let mut this = false;
        let mut ok = true;
        for p in props {
            match p {
                "Self" => this = true,
                "Other" => s.other = true,
                "YouCtrl" => s.controller = Who::You,
                "OppCtrl" => s.controller = Who::Opponent,
                // "You don't control": every other player is an opponent in the games the
                // engine plays (no teams, CR 102.3), so the same set (D-072).
                "YouDontCtrl" => s.controller = Who::Opponent,
                // Whose card it is, whoever controls it (CR 108.3). For cards outside
                // the battlefield see `owned`.
                "YouOwn" => s.owner = Who::You,
                "OppOwn" => s.owner = Who::Opponent,
                // CR 716.2a: "that has an Adventure".
                "AdventureCard" => s.has_adventure = Some(true),
                "!token" | "nonToken" => s.token = Some(false),
                "token" => s.token = Some(true),
                "Basic" => s.supertypes.push(Supertype::Basic),
                // "With a basic land type" (Forge's `hasABasicLandType`): at least one of
                // the five (CR 205.3i); a selector's subtypes are "any of these".
                "hasABasicLandType" if s.subtypes.is_empty() => {
                    s.subtypes = ["Plains", "Island", "Swamp", "Mountain", "Forest"]
                        .iter()
                        .map(|t| t.to_string())
                        .collect();
                }
                "Legendary" => s.supertypes.push(Supertype::Legendary),
                "Snow" => s.supertypes.push(Supertype::Snow),
                // Colorless (CR 105.2c): none of the five colors.
                "Colorless" => {
                    s.not_colors = vec![
                        Color::White,
                        Color::Blue,
                        Color::Black,
                        Color::Red,
                        Color::Green,
                    ]
                }
                "tapped" => s.tapped = Some(true),
                "untapped" => s.tapped = Some(false),
                // "Those cards" of the chain being compiled (D-069) — not the memory of an
                // effect object, which `in_effect` reads.
                "IsRemembered" if self.memory && !self.in_effect => s.noted = Some(true),
                "!IsRemembered" if self.memory && !self.in_effect => s.noted = Some(false),
                "attacking" => s.attacking = Some(true),
                "blocking" => s.blocking = Some(true),
                // "Of the chosen type", "that aren't of the chosen type", "of the chosen
                // color" (CR 607.2d). One reference to a choice, and no subtype or color of
                // its own beside it (`Selector::chosen`).
                "ChosenType" | "IsNotChosenType" | "ChosenColor" => {
                    let of = match p {
                        "ChosenType" => OfChosen::Type,
                        "IsNotChosenType" => OfChosen::NotType,
                        _ => OfChosen::Color,
                    };
                    let clash = s.chosen.is_some()
                        || (of == OfChosen::Type && !s.subtypes.is_empty())
                        || (of == OfChosen::Color && !s.colors.is_empty());
                    if clash {
                        self.block(format!("filter:.{p} with another"));
                        ok = false;
                    }
                    s.chosen = Some(of);
                }
                _ => {
                    if let Some(c) = color(p) {
                        // Forge's `+` is "and": "Card.Red+White" is red and white, which
                        // `colors` (at least one of them) can't say.
                        if !s.colors.is_empty() {
                            self.block("filter:two colors");
                            ok = false;
                        }
                        s.colors.push(c);
                    } else if let Some(n) = p.strip_prefix("powerGE").and_then(|n| n.parse().ok()) {
                        s.power_min = Some(n);
                    } else if let Some(n) =
                        p.strip_prefix("toughnessGE").and_then(|n| n.parse().ok())
                    {
                        s.toughness_min = Some(n);
                    } else if let Some(n) = p.strip_prefix("powerLE").and_then(|n| n.parse().ok()) {
                        s.power_max = Some(n);
                    } else if let Some(n) = p
                        .strip_prefix("powerLT")
                        .and_then(|n| n.parse::<i32>().ok())
                    {
                        s.power_max = Some(n - 1);
                    } else if let Some(n) = p
                        .strip_prefix("powerGT")
                        .and_then(|n| n.parse::<i32>().ok())
                    {
                        s.power_min = Some(n + 1);
                    } else if let Some(n) =
                        p.strip_prefix("toughnessLE").and_then(|n| n.parse().ok())
                    {
                        s.toughness_max = Some(n);
                    } else if let Some(n) = p
                        .strip_prefix("toughnessLT")
                        .and_then(|n| n.parse::<i32>().ok())
                    {
                        s.toughness_max = Some(n - 1);
                    } else if let Some(n) = p
                        .strip_prefix("toughnessGT")
                        .and_then(|n| n.parse::<i32>().ok())
                    {
                        s.toughness_min = Some(n + 1);
                    // Mana value ("converted mana cost" in Forge's words).
                    } else if let Some(n) = p.strip_prefix("cmcLE").and_then(|n| n.parse().ok()) {
                        s.mana_value_max = Some(n);
                    } else if let Some(n) = p.strip_prefix("cmcGE").and_then(|n| n.parse().ok()) {
                        s.mana_value_min = Some(n);
                    } else if let Some(n) =
                        p.strip_prefix("cmcEQ").and_then(|n| n.parse::<u32>().ok())
                    {
                        s.mana_value_min = Some(n);
                        s.mana_value_max = Some(n);
                    } else if let Some(n) = p
                        .strip_prefix("cmcLT")
                        .and_then(|n| n.parse::<u32>().ok())
                        .filter(|n| *n > 0)
                    {
                        s.mana_value_max = Some(n - 1);
                    } else if let Some(n) =
                        p.strip_prefix("cmcGT").and_then(|n| n.parse::<u32>().ok())
                    {
                        s.mana_value_min = Some(n + 1);
                    } else if let Some(k) = p.strip_prefix("without").and_then(keyword) {
                        s.not_keywords.push(k);
                    } else if let Some(k) = p.strip_prefix("with").and_then(keyword) {
                        s.keywords.push(k);
                    } else if let Some(x) = p.strip_prefix("non") {
                        match card_type(x) {
                            Some(t) => s.not_types.push(t),
                            None if color(x).is_some() => {
                                s.not_colors.extend(color(x));
                            }
                            None if x == "Basic" => s.not_supertypes.push(Supertype::Basic),
                            None if x == "Legendary" => s.not_supertypes.push(Supertype::Legendary),
                            None if x == "Snow" => s.not_supertypes.push(Supertype::Snow),
                            None if self.known.subtypes.contains(x) => {
                                s.not_subtypes.push(x.to_string())
                            }
                            None => {
                                self.block(format!("filter:.{p}"));
                                ok = false;
                            }
                        }
                    } else if self.known.subtypes.contains(p) && s.subtypes.is_empty() {
                        // "Creature.Zombie": a subtype as a restriction.
                        s.subtypes.push(p.to_string());
                    } else {
                        let shape: String = p
                            .chars()
                            .map(|c| if c.is_ascii_digit() { 'N' } else { c })
                            .collect();
                        self.block(format!("filter:.{shape}"));
                        ok = false;
                    }
                }
            }
        }
        if !ok {
            return None;
        }
        if this {
            // `Card.Self` and nothing else.
            if head == "Card" && s == Selector::default() {
                return Some(Alt::This);
            }
            self.block("filter:.Self+");
            return None;
        }
        Some(Alt::Sel(s))
    }

    fn alts(&mut self, text: &str) -> Option<Vec<Alt>> {
        text.split(',').map(|a| self.alt(a.trim())).collect()
    }

    /// A filter that must be a single selector. Alternatives that are just different card
    /// types ("Artifact,Enchantment") are one selector with several types.
    fn selector(&mut self, text: &str) -> Option<Selector> {
        let alts = self.alts(text)?;
        let sels: Vec<Selector> = alts
            .into_iter()
            .map(|a| match a {
                Alt::Sel(s) => Some(s),
                Alt::This => None,
            })
            .collect::<Option<_>>()
            .or_else(|| {
                self.block("filter:Self-in-selector");
                None
            })?;
        merge(sels).or_else(|| {
            self.block("filter:alternatives");
            None
        })
    }

    /// A filter for cards that aren't on the battlefield (in a graveyard, a hand…). There
    /// "Permanent" is not "anything": it is a *permanent card* — not an instant or sorcery
    /// (CR 110.4a).
    fn card_selector(&mut self, text: &str) -> Option<Selector> {
        let mut s = owned(self.selector(text)?);
        let heads: Vec<&str> = text
            .split(',')
            .map(|a| a.trim().split('.').next().unwrap_or(""))
            .collect();
        if heads.contains(&"Permanent") {
            if heads.iter().any(|h| *h != "Permanent") {
                self.block("filter:Permanent-card-alternatives");
                return None;
            }
            for t in [CardType::Instant, CardType::Sorcery] {
                if !s.not_types.contains(&t) {
                    s.not_types.push(t);
                }
            }
        }
        Some(s)
    }

    /// The filter of a trigger's `ValidCard$`.
    /// Is the card a permanent card (not an instant or sorcery)?
    fn permanent(&self) -> bool {
        // What is given an ability is on the battlefield.
        self.granting > 0
            || !self
                .face
                .types
                .iter()
                .any(|t| t == "Instant" || t == "Sorcery")
    }

    fn obj_filter(&mut self, text: &str) -> Option<ObjFilter> {
        if self.in_effect {
            match text.split_once('.') {
                Some(("Card" | "Creature" | "Permanent", "IsRemembered")) => {
                    return Some(ObjFilter::Remembered);
                }
                Some(("Card" | "Creature" | "Permanent", "EffectSource")) => {
                    return Some(ObjFilter::This);
                }
                _ => {}
            }
        }
        // "Enchanted creature", "equipped creature": what this is attached to. The type
        // word says nothing more where it is what the card attaches to anyway.
        if let Some((kind, "EquippedBy" | "EnchantedBy" | "AttachedBy")) = text.split_once('.') {
            let implied = match kind {
                "Card" | "Permanent" => true,
                "Creature" => self.equipment || self.enchant == Some("Creature"),
                _ => false,
            };
            if implied {
                return Some(ObjFilter::Host);
            }
        }
        let alts = self.alts(text)?;
        match alts.as_slice() {
            [Alt::This] => Some(ObjFilter::This),
            [Alt::Sel(s)] => Some(ObjFilter::Matching(s.clone())),
            // "this or another X", in either order.
            [Alt::This, Alt::Sel(s)] | [Alt::Sel(s), Alt::This] if s.other => {
                let mut s = s.clone();
                s.other = false;
                Some(ObjFilter::ThisOr(s))
            }
            _ => {
                self.block("filter:trigger-alternatives");
                None
            }
        }
    }

    // ------------------------------------------------------------------ amounts

    /// A numeric parameter of an effect in a chain.
    fn amount(&mut self, text: &str, chain: &mut Chain) -> Option<Amount> {
        if let Ok(n) = text.trim_start_matches('+').parse::<u32>() {
            return Some(Amount::Fixed(n));
        }
        // "You gain life equal to the life lost this way": Forge's life loss sets
        // `AFLifeLost` to what the players lost as it resolves. It is the instruction just
        // before this one in the chain.
        if text == "AFLifeLost" {
            if matches!(chain.effects.last(), Some(Effect::LoseLife { .. })) {
                self.used_svars.insert(text.to_string());
                return Some(Amount::LifeLostThisWay);
            }
            self.block("expr:AFLifeLost not after a life loss");
            return None;
        }
        self.chain_targets = Some(chain.targets.len());
        self.read_target = false;
        let a = self.expr(text);
        self.chain_targets = None;
        if std::mem::take(&mut self.read_target) {
            chain.refers_to_targets.set(true);
        }
        let a = a?;
        match a {
            // The announced X of the cost, a property of the triggering object or of the
            // target: used directly, where it is needed (CR 608.2h).
            Amount::X
            | Amount::TriggeringPower
            | Amount::TriggeringToughness
            | Amount::TriggeringAmount
            | Amount::SacrificedPower
            | Amount::OfTarget { .. }
            | Amount::HandOf(_) => Some(a),
            // About what an earlier instruction did: counted as this one is applied.
            Amount::Noted(_) | Amount::NotedTotal(_) => Some(a),
            // Read off the battlefield: worked out once for the whole resolution (CR
            // 608.2h), as "X".
            _ => match &chain.x {
                None => {
                    chain.x = Some((text.to_string(), a));
                    chain.x_at = chain.effects.len();
                    Some(Amount::X)
                }
                Some((name, _)) if name == text => Some(Amount::X),
                Some(_) => {
                    self.block("expr:two-variables");
                    None
                }
            },
        }
    }

    /// The value of a script variable (`SVar:X:Count$…`).
    fn expr(&mut self, name: &str) -> Option<Amount> {
        // `AFLifeLost` and its kind: Forge's effects write their result into an SVar of
        // this name ("will be set by LoseLife"); the script only holds the starting value
        // (`Number$0`), which is not what the card means (D-078).
        if name
            .strip_prefix("AF")
            .is_some_and(|r| r.starts_with(char::is_uppercase))
        {
            self.block(format!("expr:set by an effect {name}"));
            return None;
        }
        if SET_WHILE_RESOLVING.contains(&name) {
            self.block(format!("expr:set while resolving {name}"));
            return None;
        }
        let Some(v) = self.svar(name) else {
            self.block(format!("expr:undefined {name}"));
            return None;
        };
        self.expr_text(v, 0)
    }

    /// An expression: a value, optionally followed by one arithmetic suffix
    /// (docs/forge/script-format.md §6.6: only the first `/…` counts).
    fn expr_text(&mut self, v: &'a str, depth: u8) -> Option<Amount> {
        if depth > 4 {
            self.block("expr:too deep");
            return None;
        }
        let (head, math) = match v.split_once('/') {
            Some((h, m)) => (h, Some(m)),
            None => (v, None),
        };
        let base = self.expr_head(head, v, depth)?;
        let Some(math) = math else {
            return Some(base);
        };
        let (op, n) = match math.split_once('.') {
            Some((op, n)) => (op, n.parse::<u32>().ok()),
            None => (math, None),
        };
        let calc = match (op, n) {
            ("Plus", Some(n)) => Some(Calc::Plus(n)),
            ("Minus", Some(n)) => Some(Calc::Minus(n)),
            ("NMinus", Some(n)) => Some(Calc::From(n)),
            ("Times", Some(n)) => Some(Calc::Times(n)),
            ("LimitMax", Some(n)) => Some(Calc::AtMost(n)),
            ("LimitMin", Some(n)) => Some(Calc::AtLeast(n)),
            ("Twice", None) => Some(Calc::Times(2)),
            ("Thrice", None) => Some(Calc::Times(3)),
            ("HalfUp", None) => Some(Calc::HalfUp),
            ("HalfDown", None) => Some(Calc::HalfDown),
            _ => None,
        };
        match calc {
            Some(calc) => Some(Amount::Calc(Box::new(base), calc)),
            None => {
                self.block(format!("expr:math {}", shape(op)));
                None
            }
        }
    }

    /// The value part of an expression (`whole` is for naming a blocker).
    fn expr_head(&mut self, v: &'a str, whole: &str, depth: u8) -> Option<Amount> {
        if v == "Count$xPaid" && self.has_x_cost {
            // In a spell or an activated ability: the X announced for it (CR 107.3a). In a
            // permanent's own trigger it is the X the permanent was cast with (CR 107.3m),
            // which the engine doesn't remember yet.
            if self.in_trigger {
                self.block("expr:Count$xPaid in a trigger");
                return None;
            }
            return Some(Amount::X);
        }
        if let Some(n) = v
            .strip_prefix("Number$")
            .and_then(|n| n.parse::<u32>().ok())
        {
            return Some(Amount::Fixed(n));
        }
        // Another variable, so that it can take a second arithmetic suffix.
        if let Some(name) = v.strip_prefix("SVar$") {
            let Some(inner) = self.svar(name) else {
                self.block(format!("expr:undefined {name}"));
                return None;
            };
            return self.expr_text(inner, depth + 1);
        }
        // "Its power": of the event's object, or of this permanent under another name.
        if let Some((name, what)) = v.split_once('$') {
            let amount = match (self.trig.named(name), what) {
                (Some(Named::It), "CardPower") => Some(Amount::TriggeringPower),
                (Some(Named::It), "CardToughness") => Some(Amount::TriggeringToughness),
                (Some(Named::This), "CardPower") => Some(Amount::SourcePower),
                (Some(Named::This), "CardToughness") => Some(Amount::SourceToughness),
                _ => None,
            };
            if amount.is_some() {
                return amount;
            }
        }
        // "The number of cards in that player's hand", "in target player's hand".
        if let Some((name, "CardsInHand")) = v.split_once('$') {
            let who = if self.in_trigger && self.trig.is_player(name) {
                Some(if self.trig.player_is_you {
                    self.took_player_for_you = true;
                    PlayerRef::You
                } else {
                    PlayerRef::TriggeringPlayer
                })
            } else if name == "TargetedPlayer" && self.chain_targets == Some(1) {
                self.read_target = true;
                Some(PlayerRef::Target(0))
            } else {
                None
            };
            if let Some(who) = who {
                return Some(if who == PlayerRef::You {
                    Amount::HandSize
                } else {
                    Amount::HandOf(who)
                });
            }
        }
        // "Its power", "its mana value": of the chain's one target, as it last existed if
        // it is gone (CR 608.2h). Forge: the targets of this instruction or the nearest
        // one before it that has any.
        if let Some(what) = v.strip_prefix("Targeted$") {
            let what = match what {
                "CardPower" => Some(Measure::Power),
                "CardToughness" => Some(Measure::Toughness),
                "CardManaCost" => Some(Measure::ManaValue),
                _ => None,
            };
            match (what, self.chain_targets) {
                (Some(what), Some(1)) => {
                    self.read_target = true;
                    return Some(Amount::OfTarget { slot: 0, what });
                }
                (Some(_), Some(_)) => {
                    self.block("expr:Targeted$ with no one target");
                    return None;
                }
                _ => {}
            }
        }
        // About "those cards" (D-069): how many; their total mana value, power, toughness
        // (each as it is now, or as it last was before it moved: CR 608.2h, which is what
        // `Remembered…` and `RememberedLKI…` come to here); how many are of a kind.
        if self.memory && !self.in_effect {
            let total = |m| Some(Amount::NotedTotal(m));
            match v {
                "Remembered$Amount" | "Count$RememberedSize" => {
                    return Some(Amount::Noted(Vec::new()));
                }
                "Remembered$CardManaCost" | "RememberedLKI$CardManaCost" => {
                    return total(Measure::ManaValue);
                }
                "Remembered$CardPower" | "RememberedLKI$CardPower" => return total(Measure::Power),
                "Remembered$CardToughness" | "RememberedLKI$CardToughness" => {
                    return total(Measure::Toughness);
                }
                _ => {}
            }
            if let Some(filter) = v.strip_prefix("Remembered$Valid ") {
                if filter.contains('$') {
                    self.block(format!("expr:{}", shape(whole)));
                    return None;
                }
                let cards = self.card_alts(filter, true)?;
                // Whose: the owner of a card, the controller of a permanent — those cards
                // can be either.
                if cards
                    .iter()
                    .any(|s| s.controller != Who::Any || s.owner != Who::Any)
                {
                    self.block("expr:Remembered$Valid whose");
                    return None;
                }
                return Some(Amount::Noted(cards));
            }
        }
        match v {
            // "That much damage", "that many", "that much life".
            v if self.trig.amount == Some(v) => return Some(Amount::TriggeringAmount),
            // "the sacrificed creature's power": what was sacrificed to pay the cost.
            "Sacrificed$CardPower" => return Some(Amount::SacrificedPower),
            // The card the ability is on.
            "Count$CardPower" => return Some(Amount::SourcePower),
            "Count$CardToughness" => return Some(Amount::SourceToughness),
            "Count$Domain" => return Some(Amount::Domain),
            "Count$CardsInYourHand" => return Some(Amount::HandSize),
            "Count$YourLifeTotal" => return Some(Amount::LifeTotal),
            _ => {}
        }
        // Over players in the game (docs/forge/script-format.md §6.5): how many there are,
        // the highest or lowest life total among them.
        if let Some((who, what)) = v
            .strip_prefix("PlayerCount")
            .and_then(|r| r.split_once('$'))
        {
            let who = match who {
                "" | "Players" => Some(Who::Any),
                "Opponents" => Some(Who::Opponent),
                _ => None,
            };
            let amount = who.and_then(|players| match what {
                "Amount" => Some(Amount::Players(players)),
                "HighestLifeTotal" => Some(Amount::LifeAmong {
                    players,
                    highest: true,
                }),
                "LowestLifeTotal" => Some(Amount::LifeAmong {
                    players,
                    highest: false,
                }),
                _ => None,
            });
            if amount.is_some() {
                return amount;
            }
        }
        if let Some(c) = v.strip_prefix("Count$Devotion.").and_then(color) {
            return Some(Amount::Devotion(vec![c]));
        }
        // CR 700.5: "devotion to [color 1] and [color 2]".
        if let Some((a, b)) = v
            .strip_prefix("Count$DevotionDual.")
            .and_then(|r| r.split_once('.'))
            && let (Some(a), Some(b)) = (color(a), color(b))
            && a != b
        {
            let mut cs = vec![a, b];
            cs.sort();
            return Some(Amount::Devotion(cs));
        }
        // `Count$Compare <variable> GE<n>.<then>.<otherwise>`.
        if let Some(rest) = v.strip_prefix("Count$Compare ") {
            let parsed = rest.split_once(' ').and_then(|(name, test)| {
                let mut parts = test.strip_prefix("GE")?.split('.');
                let at_least = parts.next()?.parse::<u32>().ok()?;
                let then = parts.next()?.parse::<u32>().ok()?;
                let otherwise = parts.next()?.parse::<u32>().ok()?;
                parts
                    .next()
                    .is_none()
                    .then_some((name, at_least, then, otherwise))
            });
            let Some((name, at_least, then, otherwise)) = parsed else {
                self.block(format!("expr:{}", shape(whole)));
                return None;
            };
            let Some(inner) = self.svar(name) else {
                self.block(format!("expr:undefined {name}"));
                return None;
            };
            let test = self.expr_text(inner, depth + 1)?;
            return Some(Amount::IfAtLeast {
                test: Box::new(test),
                at_least,
                then,
                otherwise,
            });
        }
        // Cards in a zone other than the battlefield: every player's, unless the filter
        // says whose (`Card.YouOwn`).
        for (prefix, zone) in [
            ("Count$ValidGraveyard ", CardZone::Graveyard),
            ("Count$ValidHand ", CardZone::Hand),
            ("Count$ValidLibrary ", CardZone::Library),
        ] {
            if let Some(filter) = v.strip_prefix(prefix) {
                if filter.contains('$') {
                    self.block(format!("expr:{}", shape(whole)));
                    return None;
                }
                let cards = self.card_alts(filter, true)?;
                return Some(Amount::CardsIn { zone, cards });
            }
        }
        if let Some(rest) = v.strip_prefix("Count$Valid ") {
            return match rest.split_once('$') {
                None => self.selector(rest).map(Amount::Count),
                Some((f, "CardPower")) => self.selector(f).map(Amount::TotalPower),
                Some((f, "GreatestCardPower")) => self.selector(f).map(|of| Amount::Greatest {
                    of,
                    what: Measure::Power,
                }),
                Some((f, "GreatestCardToughness")) => self.selector(f).map(|of| Amount::Greatest {
                    of,
                    what: Measure::Toughness,
                }),
                Some((f, "GreatestCardManaCost")) => self.selector(f).map(|of| Amount::Greatest {
                    of,
                    what: Measure::ManaValue,
                }),
                _ => {
                    self.block(format!("expr:{}", shape(whole)));
                    None
                }
            };
        }
        self.block(format!("expr:{}", shape(whole)));
        None
    }

    // ------------------------------------------------------------------ references

    /// One player named by Forge's `Defined$`-style name, other than "each player" and
    /// "each opponent": you; the controller or owner of the chain's one target; the
    /// player the trigger's event happened to; the controller of the event's object.
    /// `None` (nothing blocked yet) if the name is none of those.
    fn named_player(&mut self, name: &str, chain: &Chain, triggered: bool) -> Option<PlayerRef> {
        match name {
            "You" => Some(PlayerRef::You),
            // Who controls "those cards" (D-069), or who controls none of them: "each
            // player who can't [sacrifice] discards a card".
            "RememberedController" | "NonRememberedController"
                if self.memory && !self.in_effect =>
            {
                if chain.noted_at.is_none() {
                    self.block("remember:read before anything is remembered");
                    return None;
                }
                Some(if name == "RememberedController" {
                    PlayerRef::NotedControllers
                } else {
                    PlayerRef::NotNotedControllers
                })
            }
            // "Its controller" / "its owner": of the chain's one target (CR 608.2h). Forge
            // finds no controller for a player or a card in a graveyard, so those are not
            // compiled.
            "TargetedController" | "TargetedOwner" if chain.targets.len() == 1 => {
                if !matches!(
                    chain.targets[0],
                    TargetFilter::AnyTarget
                        | TargetFilter::Creature
                        | TargetFilter::Permanent(_)
                        | TargetFilter::Spell
                        | TargetFilter::SpellMatching(_)
                ) {
                    self.block(format!("defined:{name} of a player or card"));
                    return None;
                }
                chain.refers_to_targets.set(true);
                Some(if name == "TargetedController" {
                    PlayerRef::ControllerOf(0)
                } else {
                    PlayerRef::OwnerOf(0)
                })
            }
            // "That player": the player the event happened to — "you", where only the
            // ability's controller's events trigger it.
            name if triggered && self.trig.is_player(name) => {
                if self.trig.player_is_you {
                    self.took_player_for_you = true;
                    Some(PlayerRef::You)
                } else {
                    Some(PlayerRef::TriggeringPlayer)
                }
            }
            // "Its controller": whoever controlled the event's object last (CR 608.2h);
            // of this permanent, the ability's controller (CR 603.3a).
            name if triggered
                && name
                    .strip_suffix("Controller")
                    .is_some_and(|base| self.trig.named(base).is_some()) =>
            {
                match self.trig.named(name.trim_end_matches("Controller")) {
                    Some(Named::This) => Some(PlayerRef::You),
                    _ if self.trig.it_is_this => {
                        self.took_it_for_this = true;
                        Some(PlayerRef::You)
                    }
                    _ => Some(PlayerRef::TriggeringController),
                }
            }
            _ => None,
        }
    }

    fn player(&mut self, rd: &Rd<'a>, chain: &Chain, triggered: bool) -> Option<PlayerRef> {
        match rd.get("Defined") {
            None => Some(PlayerRef::You),
            Some("Player.Opponent" | "Opponent") => Some(PlayerRef::EachOpponent),
            Some("Player") => Some(PlayerRef::Each),
            Some("Targeted") => match chain.targets.len() {
                0 => {
                    self.block("defined:Targeted-without-target");
                    None
                }
                n => {
                    chain.refers_to_targets.set(true);
                    Some(PlayerRef::Target(n as u8 - 1))
                }
            },
            Some(name) => self.one_player(name, chain, triggered, "defined:player "),
        }
    }

    /// [`Tx::named_player`], or a blocker `<what> <name>`.
    fn one_player(
        &mut self,
        name: &str,
        chain: &Chain,
        triggered: bool,
        what: &str,
    ) -> Option<PlayerRef> {
        let before = self.blockers.len();
        let p = self.named_player(name, chain, triggered);
        if p.is_none() && self.blockers.len() == before {
            self.block(format!("{what}{name}"));
        }
        p
    }

    /// `Defined$` of an effect done to objects and players alike (damage). Forge's default
    /// is the card itself.
    fn recipient(&mut self, rd: &Rd<'a>, chain: &Chain, triggered: bool) -> Option<TargetRef> {
        match rd.rec.get("Defined") {
            Some(
                "You" | "Opponent" | "Player.Opponent" | "Player" | "TargetedController"
                | "TargetedOwner",
            ) => self.player(rd, chain, triggered).map(TargetRef::Players),
            // A player the event names.
            Some(name)
                if triggered
                    && (self.trig.is_player(name)
                        || name
                            .strip_suffix("Controller")
                            .is_some_and(|base| self.trig.named(base).is_some())) =>
            {
                self.player(rd, chain, triggered).map(TargetRef::Players)
            }
            Some("Targeted") => match chain.targets.len() {
                0 => {
                    rd.get("Defined");
                    self.block("defined:Targeted-without-target");
                    None
                }
                _ if chain.target_moved.is_some() => {
                    rd.get("Defined");
                    self.moved_target(chain).map(TargetRef::Object)
                }
                n => {
                    rd.get("Defined");
                    chain.refers_to_targets.set(true);
                    Some(TargetRef::Target(n as u8 - 1))
                }
            },
            _ => self.object(rd, chain, triggered).map(TargetRef::Object),
        }
    }

    /// `Defined$ Targeted` after a link moved the target: Forge follows the card. That is
    /// the card the link put into a public zone, which the rest of the effect can find
    /// (CR 400.7j) — "return target creature card to the battlefield; put a +1/+1 counter
    /// on it".
    fn moved_target(&mut self, chain: &Chain) -> Option<ObjRef> {
        match chain.target_moved {
            Some((_, "Battlefield" | "Graveyard" | "Exile"))
                if chain.noted_at.is_none() && chain.targets.len() == 1 =>
            {
                chain.note_moved_target.set(true);
                Some(ObjRef::Noted)
            }
            _ => {
                self.block("defined:Targeted after it moved");
                None
            }
        }
    }

    fn object(&mut self, rd: &Rd<'a>, chain: &Chain, triggered: bool) -> Option<ObjRef> {
        match rd.get("Defined") {
            None | Some("Self") => Some(ObjRef::This),
            // Forge's plain form follows the card to wherever it is now and the "LKICopy"
            // form is the snapshot from the event; for the effects that use this reference
            // (counters and pumps on a permanent) both act only if it is still the
            // permanent that triggered the ability, which is what `Triggering` means.
            Some(name) if triggered && self.trig.named(name).is_some() => {
                match self.trig.named(name) {
                    Some(Named::This) => Some(ObjRef::This),
                    _ => Some(ObjRef::Triggering),
                }
            }
            Some("Targeted") if chain.target_moved.is_some() => self.moved_target(chain),
            Some("Targeted") if !chain.targets.is_empty() => {
                chain.refers_to_targets.set(true);
                Some(ObjRef::Target(chain.targets.len() as u8 - 1))
            }
            // "Those cards", "that card", "it" (D-069): once the chain has noted some.
            Some(name) if self.memory && !self.in_effect && self.is_remembered(name) => {
                match chain.noted_at {
                    Some(_) => Some(ObjRef::Noted),
                    None => {
                        self.block("remember:read before anything is remembered");
                        None
                    }
                }
            }
            Some(other) => {
                self.block(format!("defined:card {other}"));
                None
            }
        }
    }

    /// `[instruction]. [Sacrifice / exile] them at the beginning of the next end step`
    /// (Forge's `AtEOT$`): the instruction is noted, and a delayed triggered ability about
    /// what it made is created (CR 603.7, 603.7c), its text on the card's `granted` list.
    fn at_end_of_turn(
        &mut self,
        first: Effect,
        what: &str,
        step: TriggerStep,
        whose: Who,
    ) -> Effect {
        let one = matches!(
            first,
            Effect::CopyPermanent {
                what: CopyWhat::Obj(_),
                count: Amount::Fixed(1),
                ..
            }
        );
        let effect = match what {
            "Sacrifice" => Effect::SacrificeIt {
                what: ObjRef::Noted,
            },
            _ => Effect::Exile {
                what: ObjRef::Noted,
            },
        };
        let ability = self.granted.len() as u16;
        self.granted.push(AbilityDef::Triggered {
            when: Trigger::BeginningOf { step, whose },
            intervening_if: None,
            body: Body::Effect(Mode {
                targets: Vec::new(),
                effect,
                groups: Vec::new(),
            }),
        });
        Effect::Seq(vec![
            Effect::Note(Box::new(first)),
            Effect::DelayedTrigger {
                ability,
                window: DelayWindow::Next,
                player: None,
                amount: None,
                objects: Some(ObjRef::Noted),
                one,
            },
        ])
    }

    /// Does `Defined$ name` mean "those cards" (D-069)? `Remembered`; in a delayed ability
    /// that remembers objects also `DelayTriggerRemembered[LKI]` (CR 603.7c).
    fn is_remembered(&self, name: &str) -> bool {
        name == "Remembered"
            || (self.in_delayed
                && matches!(name, "DelayTriggerRemembered" | "DelayTriggerRememberedLKI"))
    }

    /// `ValidCards$` of an "all" effect. Forge's default when it is missing differs from
    /// effect to effect, so it must be there.
    fn valid_cards(&mut self, rd: &Rd<'a>, api: &str) -> Option<Selector> {
        match rd.get("ValidCards") {
            Some(f) => self.selector(f),
            None => {
                self.block(format!("{api}:no-ValidCards"));
                None
            }
        }
    }

    /// `ValidTgts$` of an effect: adds a target slot and returns its index.
    fn target(&mut self, rd: &Rd<'a>, chain: &mut Chain, graveyard: bool) -> Option<u8> {
        let text = rd.get("ValidTgts")?;
        // Which of the two wins differs from one Forge effect to the next.
        if rd.rec.get("Defined").is_some() {
            self.block("param:target+Defined");
            return None;
        }
        for k in ["TargetUnique", "TargetType"] {
            if rd.rec.get(k).is_some() {
                self.block(format!("param:target.{k}"));
                return None;
            }
        }
        // "Two target creatures" (2, 2), "up to two target creatures" (0, 2). Numbers
        // only: "X target creatures" is not compiled.
        let count = |t: Option<&str>| t.map_or(Some(1), |t| t.parse::<u8>().ok());
        let (min, max) = match (count(rd.get("TargetMin")), count(rd.get("TargetMax"))) {
            (Some(min), Some(max)) if min <= max && (1..=6).contains(&max) => (min, max),
            _ => {
                self.block("param:target.TargetMin");
                return None;
            }
        };
        let filter = if graveyard {
            let mut s = self.card_selector(text)?;
            let whose = std::mem::take(&mut s.controller);
            TargetFilter::CardInGraveyard { cards: s, whose }
        } else {
            match text {
                "Any" => TargetFilter::AnyTarget,
                "Player" => TargetFilter::Player,
                "Opponent" | "Player.Opponent" => TargetFilter::Opponent,
                "Creature" => TargetFilter::Creature,
                _ => {
                    let alts = self.alts(text)?;
                    let sels: Option<Vec<Selector>> = alts
                        .into_iter()
                        .map(|a| match a {
                            Alt::Sel(s) => Some(s),
                            Alt::This => None,
                        })
                        .collect();
                    let Some(sels) = sels else {
                        self.block("filter:Self-as-target");
                        return None;
                    };
                    if sels.iter().any(|s| s.noted.is_some()) {
                        self.block("filter:IsRemembered-as-target");
                        return None;
                    }
                    TargetFilter::Permanent(merge_bare(sels))
                }
            }
        };
        let first = chain.targets.len() as u8;
        for _ in 0..max {
            chain.targets.push(filter.clone());
        }
        if (min, max) != (1, 1) {
            chain.groups.push(TargetGroup {
                first,
                count: max,
                min,
            });
            chain.fresh_group = Some((first, max));
        }
        Some(first)
    }

    /// `e` with every reference to target slot `from` pointing at slot `to` instead.
    fn retarget(e: &Effect, from: u8, to: u8) -> Option<Effect> {
        use serde_json::Value;
        fn walk(v: &mut Value, from: u8, to: u8) {
            match v {
                // `ObjRef`, `PlayerRef` and `TargetRef` all write a slot as {"Target": n}.
                Value::Object(m) => {
                    if m.len() == 1 && m.get("Target").and_then(Value::as_u64) == Some(from as u64)
                    {
                        m.insert("Target".to_string(), Value::from(to));
                        return;
                    }
                    m.values_mut().for_each(|x| walk(x, from, to));
                }
                Value::Array(a) => a.iter_mut().for_each(|x| walk(x, from, to)),
                _ => {}
            }
        }
        let mut v = serde_json::to_value(e).ok()?;
        walk(&mut v, from, to);
        serde_json::from_value(v).ok()
    }

    // ------------------------------------------------------------------ effects

    /// One effect record (not following its sub-ability). `None` = blocked.
    fn effect(
        &mut self,
        rec: &'a Record,
        chain: &mut Chain,
        triggered: bool,
        kind: &str,
    ) -> Option<Effect> {
        let Some((found, api)) = rec.head() else {
            self.block("record:no-api");
            return None;
        };
        // A trigger or sub-ability written as `AB$` with a cost is "you may pay …; if you
        // do, …" in Forge: not the same as a plain effect.
        if found != kind {
            self.block(format!("record:{found}$ where {kind}$ expected"));
            return None;
        }
        let rd = Rd::new(rec, api);
        rd.get("SubAbility");
        rd.get("Cost");
        chain.fresh_group = None;
        // "… unless [a player] pays [cost]" (docs/forge/effects.md §0.3): this
        // instruction only; the rest of the chain happens either way.
        let unless = (
            rd.get("UnlessCost"),
            rd.get("UnlessPayer"),
            rd.get("UnlessSwitched").is_some(),
        );
        let targets_before = chain.targets.len();
        // "If …" on this link alone (docs/forge/script-format.md §2.6): a failed condition
        // skips only this link.
        let condition = self.link_condition(&rd)?;
        // The first link is the one whose targets `RememberTargets$` remembers.
        let root = chain.effects.is_empty() && targets_before == 0;
        if rec.head().map(|h| h.0) == Some("AB") {
            // Read by `ability`: when and whether the ability can be activated.
            for k in [
                "IsPresent",
                "PresentCompare",
                "PresentZone",
                "SorcerySpeed",
                "ActivationZone",
                // Read by `ability`: "this ability costs {1} less to activate …".
                "ReduceCost",
            ] {
                rd.get(k);
            }
        }
        let out = match api {
            "DealDamage" => {
                let source = rd.get("DamageSource");
                let to = match self.target(&rd, chain, false) {
                    Some(slot) => Some(TargetRef::Target(slot)),
                    None => self.recipient(&rd, chain, triggered),
                };
                let by = match source {
                    None => DamageBy::This,
                    Some("TriggeredCard") if triggered => DamageBy::Triggering,
                    // "Target creature deals damage …": the chain's one target, a
                    // permanent (it deals the damage as it last existed, CR 608.2h).
                    Some("Targeted")
                        if chain.targets.len() == 1
                            && matches!(
                                chain.targets[0],
                                TargetFilter::Creature | TargetFilter::Permanent(_)
                            ) =>
                    {
                        chain.refers_to_targets.set(true);
                        DamageBy::Target(0)
                    }
                    Some(other) => {
                        self.block(format!("param:DealDamage.DamageSource={other}"));
                        return None;
                    }
                };
                let amount = rd.get("NumDmg").and_then(|n| self.amount(n, chain));
                Effect::DealDamage {
                    to: to?,
                    amount: amount?,
                    by,
                }
            }
            "Draw" | "Mill" | "GainLife" | "LoseLife" => {
                let key = match api {
                    "Draw" | "Mill" => "NumCards",
                    _ => "LifeAmount",
                };
                // "each player draws": its own effect.
                if api == "Draw" && rd.rec.get("Defined") == Some("Player") {
                    rd.get("Defined");
                    let n = rd.get(key).unwrap_or("1").parse::<u32>().ok().or_else(|| {
                        self.block("Draw:each-player-variable");
                        None
                    });
                    rd.finish(&mut self.blockers);
                    return n.map(|n| Effect::EachPlayer(EachPlayerAction::Draw(n)));
                }
                let slot = self.target(&rd, chain, false);
                let who = match slot {
                    Some(i) => Some(PlayerRef::Target(i)),
                    None => self.player(&rd, chain, triggered),
                };
                let amount = self.amount(rd.get(key).unwrap_or("1"), chain);
                let (who, amount) = (who?, amount?);
                // "May draw up to N cards": the drawer chooses how many.
                let upto = api == "Draw" && rd.get("Upto").is_some();
                if upto && matches!(who, PlayerRef::Each | PlayerRef::EachOpponent) {
                    self.block("param:Draw.Upto for several players");
                    return None;
                }
                let e = match api {
                    "Draw" if upto => Effect::DrawUpTo { who, amount },
                    "Draw" => Effect::Draw { who, amount },
                    "Mill" => Effect::Mill { who, amount },
                    "GainLife" => Effect::GainLife { who, amount },
                    _ => Effect::LoseLife { who, amount },
                };
                // "[They] may draw": Forge's draw asks the player who would draw, whatever
                // `OptionalDecider$` names. Compiled where it names that one player.
                match (api, rd.get("OptionalDecider")) {
                    (_, None) => e,
                    ("Draw", Some(name)) => {
                        // Forge only asks whether the key is there.
                        let decider = if name == "True" {
                            who
                        } else {
                            self.one_player(name, chain, triggered, "param:Draw.OptionalDecider=")?
                        };
                        match who {
                            PlayerRef::You if decider == who => Effect::May(Box::new(e)),
                            PlayerRef::ControllerOf(_)
                            | PlayerRef::OwnerOf(_)
                            | PlayerRef::TriggeringController
                            | PlayerRef::TriggeringPlayer
                                if decider == who =>
                            {
                                Effect::PlayerMay {
                                    who,
                                    effect: Box::new(e),
                                }
                            }
                            _ => {
                                self.block("param:Draw.OptionalDecider not the drawer");
                                return None;
                            }
                        }
                    }
                    (api, Some(_)) => {
                        self.block(format!("param:{api}.OptionalDecider"));
                        return None;
                    }
                }
            }
            // A spell or a non-mana ability that adds mana (CR 605.5): exactly that mana
            // ("R R R G G G": each type `Amount$` times), or `Amount$` mana of one type the
            // player chooses ("Any": any color; "Combo R G" once: {R} or {G}).
            "Mana" => {
                let letter = |w: &str| match w {
                    "W" => Some(ManaType::White),
                    "U" => Some(ManaType::Blue),
                    "B" => Some(ManaType::Black),
                    "R" => Some(ManaType::Red),
                    "G" => Some(ManaType::Green),
                    "C" => Some(ManaType::Colorless),
                    _ => None,
                };
                let words: Vec<&str> = rd
                    .get("Produced")
                    .unwrap_or("")
                    .split_whitespace()
                    .collect();
                let amount_text = rd.get("Amount").unwrap_or("1");
                let who = self.player(&rd, chain, triggered);
                let amount = self.amount(amount_text, chain);
                let (who, amount) = (who?, amount?);
                let colors: Vec<ManaType> =
                    Color::ALL.into_iter().map(ManaType::from_color).collect();
                match words.as_slice() {
                    ["Any"] => Effect::AddManaOf {
                        who,
                        of: colors,
                        amount,
                    },
                    ["Combo", rest @ ..] if amount == Amount::Fixed(1) && !rest.is_empty() => {
                        let of: Option<Vec<ManaType>> = match rest {
                            ["Any"] => Some(colors),
                            _ => rest.iter().map(|w| letter(w)).collect(),
                        };
                        match of {
                            Some(mut of) if of.len() > 1 => {
                                of.dedup();
                                Effect::AddManaOf { who, of, amount }
                            }
                            _ => {
                                self.block("Mana:produced");
                                return None;
                            }
                        }
                    }
                    [_, ..] => match words.iter().map(|w| letter(w)).collect::<Option<Vec<_>>>() {
                        Some(all) => {
                            // Each type once, as many times as it is listed.
                            let mut parts: Vec<Effect> = Vec::new();
                            for t in ManaType::ALL {
                                let n = all.iter().filter(|x| **x == t).count() as u32;
                                if n == 0 {
                                    continue;
                                }
                                let amount = match (&amount, n) {
                                    (a, 1) => a.clone(),
                                    (Amount::Fixed(k), n) => Amount::Fixed(k * n),
                                    (a, n) => Amount::Calc(Box::new(a.clone()), Calc::Times(n)),
                                };
                                parts.push(Effect::AddMana {
                                    who,
                                    mana: t,
                                    amount,
                                });
                            }
                            if parts.len() == 1 {
                                parts.remove(0)
                            } else {
                                Effect::Seq(parts)
                            }
                        }
                        None => {
                            self.block("Mana:produced");
                            return None;
                        }
                    },
                    [] => {
                        self.block("Mana:produced");
                        return None;
                    }
                }
            }
            "Destroy" => {
                let what = match self.target(&rd, chain, false) {
                    Some(slot) => Some(ObjRef::Target(slot)),
                    None => self.object(&rd, chain, triggered),
                };
                let destroy = Effect::Destroy { what: what? };
                // "It can't be regenerated" (CR 701.19c): Forge tests `NoRegen$` for
                // presence (DestroyEffect).
                if rd.get("NoRegen").is_some() {
                    Effect::CantBeRegenerated(Box::new(destroy))
                } else {
                    destroy
                }
            }
            // CR 701.19a: "regenerate [permanent]" — a regeneration shield (Forge's
            // RegenerateEffect: `Defined$` / `ValidTgts$`, the host by default).
            "Regenerate" => {
                let what = match self.target(&rd, chain, false) {
                    Some(slot) => Some(ObjRef::Target(slot)),
                    None if rd.rec.get("Defined").is_none() => Some(ObjRef::This),
                    None => self.object(&rd, chain, triggered),
                };
                Effect::Regenerate { what: what? }
            }
            // CR 701.34a. `Amount$ N`: "proliferate N times" — one after the other, each
            // its own choice (Forge's loop).
            "Proliferate" => {
                let times = match rd.get("Amount") {
                    None => Some(1),
                    Some(n) => n.parse::<usize>().ok().filter(|n| (1..=10).contains(n)),
                };
                match times {
                    Some(1) => Effect::Proliferate,
                    Some(n) => Effect::Seq(vec![Effect::Proliferate; n]),
                    None => {
                        self.block("param:Proliferate.Amount not a number");
                        return None;
                    }
                }
            }
            // "Untap up to N lands" (docs/forge/effects.md, Untap): the activator chooses
            // among every permanent on the battlefield that fits `UntapType$` (whoever
            // controls it) as it resolves. Forge offers untapped ones too, which can't be
            // untapped (CR 701.26b): the engine offers only tapped ones.
            "Untap" if rd.rec.get("UntapUpTo").is_some() => {
                rd.get("UntapUpTo");
                if !matches!(rd.get("Defined"), None | Some("You")) {
                    self.block("param:Untap.UntapUpTo by another player");
                    return None;
                }
                let what = match rd.get("UntapType") {
                    Some(f) => self.selector(f),
                    None => {
                        self.block("param:Untap.UntapUpTo without UntapType");
                        None
                    }
                };
                let count = rd.get("Amount").and_then(|n| self.amount(n, chain));
                Effect::UntapChosen {
                    what: what?,
                    count: count?,
                }
            }
            "Tap" | "Untap" => {
                // `ETB$`: "enters tapped/untapped", which is a replacement, not this effect.
                if rd.rec.get("ETB").is_some() {
                    self.block(format!("{api}:ETB"));
                    return None;
                }
                let what = match self.target(&rd, chain, false) {
                    Some(slot) => Some(ObjRef::Target(slot)),
                    None => self.object(&rd, chain, triggered),
                };
                match api {
                    "Tap" => Effect::Tap { what: what? },
                    _ => Effect::Untap { what: what? },
                }
            }
            // CR 701.27a: "transform this permanent" (docs/forge/effects.md: SetState). Forge
            // reads `Defined$` (default: the card itself) and `Mode$`; the other modes
            // (flip, turning face up or down, specializing) aren't transforming.
            "SetState" => {
                if rd.get("Mode") != Some("Transform") {
                    self.block(format!(
                        "SetState:mode {}",
                        rd.rec.get("Mode").unwrap_or("none")
                    ));
                    return None;
                }
                if rd.rec.get("ValidTgts").is_some() {
                    self.block("SetState:target");
                    return None;
                }
                match self.object(&rd, chain, triggered)? {
                    ObjRef::This => Effect::Transform { what: ObjRef::This },
                    _ => {
                        self.block("SetState:not itself");
                        return None;
                    }
                }
            }
            "TapAll" | "UntapAll" => {
                let sel = self.valid_cards(&rd, api);
                match api {
                    "TapAll" => Effect::TapAll(sel?),
                    _ => Effect::UntapAll(sel?),
                }
            }
            "Counter" => {
                // Only spells, and only as a target.
                if rd.get("TargetType") != Some("Spell") {
                    self.block("Counter:not-a-spell-target");
                    return None;
                }
                // `Destination$ Graveyard` is Forge's default written out (CounterEffect:
                // CR 701.6a, "put into its owner's graveyard").
                if rd.get("Destination").is_some_and(|d| d != "Graveyard") {
                    self.block("param:Counter.Destination");
                    return None;
                }
                for k in ["Defined", "TargetMin", "TargetMax"] {
                    if rd.rec.get(k).is_some() {
                        self.block(format!("param:Counter.{k}"));
                        return None;
                    }
                }
                let filter = match rd.get("ValidTgts") {
                    Some("Card") => TargetFilter::Spell,
                    Some(text) => {
                        let s = self.card_selector(text)?;
                        // What a spell can be asked: its types and colors.
                        let asked = Selector {
                            types: s.types.clone(),
                            not_types: s.not_types.clone(),
                            subtypes: s.subtypes.clone(),
                            not_subtypes: s.not_subtypes.clone(),
                            colors: s.colors.clone(),
                            ..Selector::default()
                        };
                        if asked != s {
                            self.block("Counter:filter");
                            return None;
                        }
                        TargetFilter::SpellMatching(s)
                    }
                    None => {
                        self.block("Counter:no-target");
                        return None;
                    }
                };
                chain.targets.push(filter);
                let slot = chain.targets.len() as u8 - 1;
                // What a later delayed trigger of the chain remembers of it (CR 603.7c):
                // only in a chain that forgets at its end.
                let (targets, cmc) = (
                    rd.get("RememberTargets").is_some(),
                    rd.get("RememberCounteredCMC").is_some(),
                );
                if targets || cmc {
                    if !self.memory {
                        self.block("param:Counter.Remember… kept after the chain");
                        return None;
                    }
                    chain.countered = Some((slot, targets, cmc));
                }
                Effect::Counter {
                    what: TargetRef::Target(slot),
                }
            }
            // "Look at the top N cards of [a player's] library, then put them back in any
            // order. You may shuffle." (docs/forge/effects.md): the activator looks and
            // orders (CR 401.4 is about cards put there by an effect, so the order is theirs
            // to choose), then may have that library shuffled.
            "RearrangeTopOfLibrary" => {
                let slot = self.target(&rd, chain, false);
                let who = match slot {
                    Some(i) => Some(PlayerRef::Target(i)),
                    None => self.player(&rd, chain, triggered),
                };
                let count = rd.get("NumCards").and_then(|n| self.amount(n, chain));
                let shuffle = rd.get("MayShuffle").is_some();
                let (who, count) = (who?, count?);
                if matches!(who, PlayerRef::Each | PlayerRef::EachOpponent) {
                    self.block("RearrangeTopOfLibrary:several libraries");
                    return None;
                }
                let dig = Effect::Dig {
                    who,
                    count,
                    reveal: false,
                    take: Take::Exactly(0),
                    cards: Vec::new(),
                    to: CardDest::LibraryTop,
                    reveal_taken: false,
                    rest: CardDest::LibraryTop,
                    rest_random: false,
                };
                if shuffle {
                    Effect::Seq(vec![dig, Effect::May(Box::new(Effect::Shuffle { who }))])
                } else {
                    dig
                }
            }
            // CR 603.7: "at the beginning of the next …, [do something]".
            "DelayedTrigger" => self.delayed_trigger(&rd, chain)?,
            "DestroyAll" => {
                // Forge destroys on the battlefield whatever this says.
                if !matches!(rd.get("Zone"), None | Some("Battlefield")) {
                    self.block("param:DestroyAll.Zone");
                    return None;
                }
                let sel = self.valid_cards(&rd, api);
                let destroy = Effect::DestroyAll(sel?);
                // "They can't be regenerated" (CR 701.19c).
                if rd.get("NoRegen").is_some() {
                    Effect::CantBeRegenerated(Box::new(destroy))
                } else {
                    destroy
                }
            }
            // "Choose a creature type. [Instructions about the chosen type.]" (CR 608.2c).
            // Forge keeps the choice on the card; the engine keeps it for the resolution
            // only, which is the same for a spell. A permanent's ability that chooses as it
            // resolves would leave the choice on the permanent for its other abilities.
            "ChooseType" | "ChooseColor" => {
                if !self.in_spell {
                    self.block(format!("{api}:in an ability of a permanent"));
                    return None;
                }
                Effect::Choose(self.choice(&rd, api)?)
            }
            "DamageAll" => {
                let amount = rd.get("NumDmg").and_then(|n| self.amount(n, chain));
                match (rd.rec.get("ValidCards"), rd.get("ValidPlayers")) {
                    (_, None) => {
                        let sel = self.valid_cards(&rd, api);
                        Effect::DamageEach {
                            each: sel?,
                            amount: amount?,
                        }
                    }
                    // Players only: "deals N damage to each player / each opponent".
                    (None, Some("Player")) => Effect::DamageEachPlayer { amount: amount? },
                    (None, Some("Player.Opponent" | "Opponent")) => Effect::DealDamage {
                        to: TargetRef::Players(PlayerRef::EachOpponent),
                        amount: amount?,
                        by: DamageBy::This,
                    },
                    (None, Some(other)) => {
                        self.block(format!("param:DamageAll.ValidPlayers={other}"));
                        return None;
                    }
                    // Creatures and players at once is one event the vocabulary lacks.
                    (Some(_), Some(_)) => {
                        self.block("DamageAll:cards+players");
                        return None;
                    }
                }
            }
            // Chosen as it resolves: from a hidden zone, or — with `Hidden$` — from a
            // public one ("return a creature card from your graveyard to your hand",
            // "return a creature you control to its owner's hand": no target).
            "ChangeZone"
                if (matches!(rec.get("Origin"), Some("Library" | "Hand"))
                    || (rec.get("Hidden").is_some()
                        && matches!(rec.get("Origin"), Some("Graveyard" | "Battlefield"))))
                    && rec.get("ValidTgts").is_none()
                    && rec.get("Defined").is_none() =>
            {
                self.choose_cards(&rd, chain, triggered)?
            }
            // "Look at / reveal the top N cards of your library; put some of them
            // somewhere and the rest somewhere else" (docs/forge/effects.md §16).
            "Dig" => {
                let slot = self.target(&rd, chain, false);
                let who = match slot {
                    Some(i) => Some(PlayerRef::Target(i)),
                    None => self.player(&rd, chain, triggered),
                };
                let count = rd.get("DigNum").and_then(|n| self.amount(n, chain));
                let optional = match rd.get("Optional") {
                    None => Some(false),
                    Some("True") => Some(true),
                    Some(_) => None,
                };
                // How many go to the first place: that many (as many as there are), "you
                // may", every one that qualifies, or any number.
                let take = match (rd.get("ChangeNum").unwrap_or("1"), optional) {
                    ("All", Some(false)) => Some(Take::All),
                    ("Any", Some(_)) | ("All", Some(true)) => Some(Take::Any),
                    (n, Some(optional)) => n.parse::<u32>().ok().map(|n| match optional {
                        true => Take::UpTo(n),
                        false => Take::Exactly(n),
                    }),
                    (_, None) => None,
                };
                if take.is_none() {
                    self.block("Dig:ChangeNum");
                }
                let valid = rd.get("ChangeValid");
                let cards = match valid {
                    None => Some(Vec::new()),
                    Some(f) => self.card_alts(f, true),
                };
                // A library position is a literal here, and the bottom is the default.
                let place = |tx: &mut Self,
                             zone: Option<&str>,
                             zone_default: &str,
                             pos: Option<&str>,
                             tapped: bool| {
                    match (zone.unwrap_or(zone_default), pos) {
                        ("Hand", None) => Some(CardDest::Hand),
                        ("Graveyard", None) => Some(CardDest::Graveyard),
                        ("Exile", None) => Some(CardDest::Exile),
                        ("Battlefield", None) => Some(CardDest::Battlefield { tapped }),
                        ("Library", None | Some("-1")) => Some(CardDest::LibraryBottom),
                        ("Library", Some("0")) => Some(CardDest::LibraryTop),
                        (z, _) => {
                            tx.block(format!("Dig:to {z}"));
                            None
                        }
                    }
                };
                let tapped = rd.get("Tapped").is_some();
                let to = place(
                    self,
                    rd.get("DestinationZone"),
                    "Hand",
                    rd.get("LibraryPosition"),
                    tapped,
                );
                if tapped && !matches!(to, Some(CardDest::Battlefield { .. })) {
                    self.block("Dig:Tapped");
                    return None;
                }
                let rest = place(
                    self,
                    rd.get("DestinationZone2"),
                    "Library",
                    rd.get("LibraryPosition2"),
                    false,
                );
                let reveal = rd.get("Reveal").is_some();
                let no_reveal = rd.get("NoReveal").is_some();
                // Some scripts reveal every card where the card says "look at … you may
                // reveal a [card] from among them": that shows opponents more than the
                // card does.
                // The same goes for looking where the card reveals, and for the order the
                // rest is put back in. The card's own text decides.
                let rest_random = rd.get("RestRandomOrder").is_some();
                if let Some(o) = &self.face.oracle {
                    let o = o.to_lowercase();
                    let looks = o.contains("look at the top");
                    let reveals = o.contains("reveal the top") || o.contains("reveals the top");
                    if (reveal && looks && !reveals) || (!reveal && reveals && !looks) {
                        self.block("Dig:look or reveal differs from the card");
                        return None;
                    }
                    let to_library = matches!(
                        rd.rec.get("DestinationZone2").unwrap_or("Library"),
                        "Library"
                    );
                    if to_library && rest_random != o.contains("random order") {
                        self.block("Dig:order of the rest differs from the card");
                        return None;
                    }
                }
                // Forge has the activator look, pick and order for every library. That is
                // right for "target player's library"; for "each player …" it is only
                // right when nobody has anything to decide.
                let (who, take, to, rest) = (who?, take?, to?, rest?);
                let in_library =
                    |d: CardDest| matches!(d, CardDest::LibraryTop | CardDest::LibraryBottom);
                if matches!(who, PlayerRef::Each | PlayerRef::EachOpponent)
                    && (take != Take::All || in_library(to) || (in_library(rest) && !rest_random))
                {
                    self.block("Dig:each player chooses");
                    return None;
                }
                // Another player's cards put onto the battlefield: Forge gives them to their
                // owner; by CR 110.2a it is whoever the card tells to put them there. Only
                // where nothing is chosen (the library's player reveals and puts them, as
                // the engine's text says) is that the owner for sure.
                let battlefield = |d: CardDest| matches!(d, CardDest::Battlefield { .. });
                let theirs = take == Take::All
                    && (rest_random || count == Some(Amount::Fixed(1)) || !in_library(rest))
                    && !(valid.is_some() && !no_reveal && !reveal);
                if who != PlayerRef::You && (battlefield(to) || battlefield(rest)) && !theirs {
                    self.block("Dig:another player's cards onto the battlefield, chosen by you");
                    return None;
                }
                Effect::Dig {
                    who,
                    count: count?,
                    reveal,
                    take,
                    // Cards picked for what they are are shown, unless the script says
                    // not ("put a card into your hand" needs no proof).
                    reveal_taken: valid.is_some() && !no_reveal,
                    cards: cards?,
                    to,
                    rest,
                    rest_random,
                }
            }
            // "Exile [this spell]", "shuffle [it] into its owner's library": where the
            // resolving spell goes instead of the graveyard (CR 608.2n).
            "ChangeZone"
                if rec.get("Origin") == Some("Stack")
                    && self.in_spell
                    && rec.get("ValidTgts").is_none()
                    && matches!(rec.get("Defined"), None | Some("Self" | "Parent")) =>
            {
                rd.get("Origin");
                rd.get("Defined");
                let shuffle = match rd.get("Shuffle") {
                    None => false,
                    Some("True") => true,
                    Some(_) => {
                        self.block("param:ChangeZone.Shuffle");
                        return None;
                    }
                };
                let to = match rd.get("Destination") {
                    Some("Exile") => CardDest::Exile,
                    Some("Hand") => CardDest::Hand,
                    Some("Library") => self.library_position(&rd)?,
                    other => {
                        self.block(format!("ChangeZone:Stack->{}", other.unwrap_or("?")));
                        return None;
                    }
                };
                Effect::ThisSpellTo { to, shuffle }
            }
            "ChangeZone" => {
                let origin = rd.get("Origin").unwrap_or("Battlefield");
                let dest = rd.get("Destination");
                // "Return it to its owner's hand", in an ability triggered by a permanent
                // dying: the card it became in the graveyard.
                let that_card = origin == "Graveyard"
                    && triggered
                    && rd.rec.get("ValidTgts").is_none()
                    && match rd.rec.get("Defined") {
                        Some(
                            "TriggeredNewCardLKICopy" | "TriggeredCard" | "TriggeredCardLKICopy",
                        ) => self.trig.died,
                        None | Some("Self") => self.trig.died_self,
                        _ => false,
                    };
                if that_card {
                    rd.get("Defined");
                }
                let what = match self.target(&rd, chain, origin == "Graveyard") {
                    _ if that_card => ObjRef::TriggeringCard,
                    Some(slot) => ObjRef::Target(slot),
                    // A permanent named some other way: "return this to its owner's hand"
                    // (the card itself is Forge's default).
                    None if origin == "Battlefield" => self.object(&rd, chain, triggered)?,
                    // "Return this card from your graveyard …", in an ability that works
                    // from the graveyard and has not moved the card to pay for itself.
                    None if origin == "Graveyard"
                        && self.from_zone == ActivationZone::Graveyard
                        && chain.effects.is_empty()
                        && matches!(rd.get("Defined"), None | Some("Self")) =>
                    {
                        ObjRef::ThisCard
                    }
                    // "Return those cards …" (D-069).
                    None if self.memory
                        && !self.in_effect
                        && rd.rec.get("Defined").is_some_and(|d| self.is_remembered(d)) =>
                    {
                        self.object(&rd, chain, triggered)?
                    }
                    None => {
                        self.block("ChangeZone:no-target");
                        return None;
                    }
                };
                // "… under your control" (Forge's `GainControl$ True`).
                let controller = match rd.get("GainControl") {
                    None => None,
                    Some("True") if dest == Some("Battlefield") => Some(PlayerRef::You),
                    Some(_) => {
                        self.block("param:ChangeZone.GainControl");
                        return None;
                    }
                };
                // `ChangeNum$ 1` says nothing about one target.
                if rd.rec.get("ChangeNum").is_some()
                    && (rd.get("ChangeNum") != Some("1") || !matches!(what, ObjRef::Target(_)))
                {
                    self.block("param:ChangeZone.ChangeNum");
                    return None;
                }
                let noted = match what {
                    ObjRef::Noted => Some(self.move_noted(&rd, chain, dest, controller)?),
                    _ => None,
                };
                if matches!(what, ObjRef::Target(_)) {
                    let to = match dest {
                        Some("Battlefield") => "Battlefield",
                        Some("Graveyard") => "Graveyard",
                        Some("Exile") => "Exile",
                        _ => "Hidden",
                    };
                    chain.target_moved = Some((chain.effects.len(), to));
                }
                if let Some(noted) = noted {
                    noted
                } else {
                    match (origin, dest) {
                        ("Battlefield" | "Graveyard", Some("Exile")) => Effect::Exile { what },
                        ("Battlefield" | "Graveyard", Some("Hand")) => {
                            Effect::ReturnToHand { what }
                        }
                        ("Graveyard", Some("Battlefield"))
                            if rd.rec.get("Tapped").is_none() && controller.is_none() =>
                        {
                            Effect::ReturnToBattlefield { what }
                        }
                        ("Graveyard", Some("Battlefield"))
                            if matches!(rd.rec.get("Tapped"), None | Some("True")) =>
                        {
                            Effect::PutInto {
                                what,
                                to: CardDest::Battlefield {
                                    tapped: rd.get("Tapped").is_some(),
                                },
                                shuffle: false,
                                controller,
                            }
                        }
                        // "Put target creature on top of its owner's library", "shuffle it
                        // into its owner's library".
                        ("Battlefield" | "Graveyard", Some("Library")) => {
                            let to = self.library_position(&rd)?;
                            let shuffle = match rd.get("Shuffle") {
                                None => false,
                                Some("True") => true,
                                Some(_) => {
                                    self.block("param:ChangeZone.Shuffle");
                                    return None;
                                }
                            };
                            Effect::PutInto {
                                what,
                                to,
                                shuffle,
                                controller: None,
                            }
                        }
                        (o, d) => {
                            self.block(format!("ChangeZone:{o}->{}", d.unwrap_or("?")));
                            return None;
                        }
                    }
                }
            }
            "ChangeZoneAll" => {
                let shape = (
                    rd.get("Origin"),
                    rd.get("Destination"),
                    rd.get("ChangeType"),
                );
                let slot = self.target(&rd, chain, false);
                match (shape, slot) {
                    ((Some("Graveyard"), Some("Exile"), Some("Card")), Some(i))
                        if chain.targets[i as usize] == TargetFilter::Player =>
                    {
                        Effect::ExileGraveyard {
                            who: PlayerRef::Target(i),
                        }
                    }
                    // "Return all creatures to their owners' hands", "exile all
                    // artifacts", "destroy"-less mass moves of permanents: everyone's.
                    ((Some("Battlefield"), Some(dest), Some(filter)), None)
                        if rd.rec.get("Defined").is_none() =>
                    {
                        let to = match dest {
                            "Hand" => CardDest::Hand,
                            "Exile" => CardDest::Exile,
                            "Graveyard" => CardDest::Graveyard,
                            _ => {
                                self.block("ChangeZoneAll:shape");
                                return None;
                            }
                        };
                        let cards = self.card_alts(filter, false)?;
                        Effect::MoveAll {
                            from: CardZone::Battlefield,
                            whose: PlayerRef::Each,
                            cards,
                            to,
                            shuffle: false,
                        }
                    }
                    _ => {
                        self.block("ChangeZoneAll:shape");
                        return None;
                    }
                }
            }
            // CR 701.22 / 701.25.
            "Scry" | "Surveil" => {
                let who = match self.target(&rd, chain, false) {
                    Some(i) if chain.targets[i as usize].allows_players() => {
                        Some(PlayerRef::Target(i))
                    }
                    Some(_) => {
                        self.block(format!("{api}:target"));
                        return None;
                    }
                    None => self.player(&rd, chain, triggered),
                };
                let key = if api == "Scry" { "ScryNum" } else { "Amount" };
                let amount = self.amount(rd.get(key).unwrap_or("1"), chain);
                match api {
                    "Scry" => Effect::Scry {
                        who: who?,
                        amount: amount?,
                    },
                    _ => Effect::Surveil {
                        who: who?,
                        amount: amount?,
                    },
                }
            }
            "Token" => {
                let script = rd.get("TokenScript");
                // "Target opponent creates …": a target of this instruction, for whom.
                let slot = self.target(&rd, chain, false);
                if slot.is_some() && !matches!(rd.rec.get("TokenOwner"), Some("Targeted")) {
                    self.block("Token:target not the creator");
                    return None;
                }
                let count = self.amount(rd.get("TokenAmount").unwrap_or("1"), chain);
                // Who creates the tokens, and so owns and controls them (CR 111.2): "its
                // controller creates …" — of a target, or of the event's object.
                let who = match rd.get("TokenOwner") {
                    None => Some(PlayerRef::You),
                    Some("Player") => Some(PlayerRef::Each),
                    Some("Player.Opponent" | "Opponent") => Some(PlayerRef::EachOpponent),
                    // "Target opponent creates …": the chain's one target, a player.
                    Some("Targeted" | "TargetedPlayer")
                        if chain.targets.len() == 1 && chain.targets[0].allows_players() =>
                    {
                        chain.refers_to_targets.set(true);
                        Some(PlayerRef::Target(0))
                    }
                    Some(name) => {
                        self.one_player(name, chain, triggered, "param:Token.TokenOwner=")
                    }
                };
                let Some(script) = script else {
                    self.block("Token:no-script");
                    return None;
                };
                self.tokens.insert(script.to_string());
                Effect::CreateToken {
                    token: script.to_string(),
                    count: count?,
                    who: who?,
                }
            }
            "PutCounter" => {
                let kind = match rd.get("CounterType") {
                    Some("P1P1") => CounterKind::PlusOne,
                    Some("M1M1") => CounterKind::MinusOne,
                    other => {
                        self.block(format!("counter:{}", other.unwrap_or("?")));
                        return None;
                    }
                };
                let slot = self.target(&rd, chain, false);
                let what = match slot {
                    Some(i) => Some(ObjRef::Target(i)),
                    None => self.object(&rd, chain, triggered),
                };
                let count = self.amount(rd.get("CounterNum").unwrap_or("1"), chain);
                Effect::PutCounters {
                    what: what?,
                    kind,
                    count: count?,
                }
            }
            // "Put a +1/+1 counter on each creature you control."
            "PutCounterAll" => {
                let kind = self.counter_kind(rd.get("CounterType"));
                let what = self.valid_cards(&rd, api);
                let count = self.amount(rd.get("CounterNum").unwrap_or("1"), chain);
                Effect::PutCountersAll {
                    what: what?,
                    kind: kind?,
                    count: count?,
                }
            }
            // "Becomes a 3/3 green Ape creature with trample until end of turn", "is an
            // artifact in addition to its other types": types, colors, base power and
            // toughness, keywords — for a turn, or for good (docs/forge/effects.md §17).
            "Animate" | "AnimateAll" => {
                let mut effects = Vec::new();
                // Layer 4: types added (nothing is removed: the `Remove…` keys are not
                // read, so a script that has them is not compiled).
                if let Some(types) = rd.get("Types") {
                    // (The chosen type of a resolving effect is not carried into what it
                    // makes last: only a static ability adds it.)
                    if types.split(',').any(|t| t.trim() == "ChosenType") {
                        self.block("Animate:Types ChosenType");
                        return None;
                    }
                    effects.push(self.added_types(types.split(','))?);
                }
                // Layer 5: only "becomes [these colors]", not "in addition".
                if let Some(colors) = rd.get("Colors") {
                    if rd.get("OverwriteColors") != Some("True") {
                        self.block("Animate:colors added");
                        return None;
                    }
                    let mut cs = Vec::new();
                    for c in colors.split(',').map(str::trim) {
                        match color(c) {
                            Some(c) => cs.push(c),
                            None if c == "Colorless" => {}
                            None => {
                                self.block(format!("Animate:color {}", shape(c)));
                                return None;
                            }
                        }
                    }
                    cs.sort();
                    effects.push(ContinuousEffect::SetColors(cs));
                }
                // Layer 6.
                if rd.get("RemoveAllAbilities").is_some() {
                    effects.push(ContinuousEffect::LoseAllAbilities);
                }
                if let Some(kw) = rd.get("Keywords") {
                    let mut ks = Vec::new();
                    for k in kw.split('&').map(str::trim) {
                        match keyword(k) {
                            Some(k) => ks.push(k),
                            None => {
                                self.block(format!("kw:{k}"));
                                return None;
                            }
                        }
                    }
                    effects.push(ContinuousEffect::AddKeywords(ks));
                }
                // "… and gains \"[ability]\"".
                let mut given = Vec::new();
                for (key, kind) in [
                    ("Abilities", Given::Activated),
                    ("Triggers", Given::Triggered),
                    ("staticAbilities", Given::Static),
                    ("Replacements", Given::Replacement),
                ] {
                    if let Some(names) = rd.get(key) {
                        given.extend(self.grant(kind, names, ',')?);
                    }
                }
                if !given.is_empty() {
                    effects.push(ContinuousEffect::GrantAbilities(given));
                }
                // Layer 7b: base power and toughness, both or neither.
                let number = |t: Option<&str>| t.map(|t| t.parse::<i32>().ok());
                match (number(rd.get("Power")), number(rd.get("Toughness"))) {
                    (None, None) => {}
                    (Some(Some(p)), Some(Some(t))) => effects.push(ContinuousEffect::SetPT(p, t)),
                    _ => {
                        self.block("Animate:power and toughness");
                        return None;
                    }
                }
                if effects.is_empty() {
                    self.block("Animate:nothing");
                    return None;
                }
                let until_end_of_turn = match rd.get("Duration") {
                    None => true,
                    Some("Permanent") => false,
                    Some(other) => {
                        self.block(format!("Animate:duration {other}"));
                        return None;
                    }
                };
                if api == "AnimateAll" {
                    let sel = self.valid_cards(&rd, api);
                    Effect::ApplyAll {
                        affected: sel?,
                        effects,
                        until_end_of_turn,
                    }
                } else {
                    let slot = self.target(&rd, chain, false);
                    let what = match slot {
                        Some(i) => Some(ObjRef::Target(i)),
                        None => self.object(&rd, chain, triggered),
                    };
                    Effect::Apply {
                        what: what?,
                        effects,
                        until_end_of_turn,
                    }
                }
            }
            // An effect object: abilities that work for a while on no permanent.
            "Effect" => {
                let own_target = self.target(&rd, chain, false);
                let last = (chain.targets.len() as u8).checked_sub(1);
                let about = match rd.get("RememberObjects") {
                    None if own_target.is_some() => {
                        self.block("Effect:target not remembered");
                        return None;
                    }
                    None => None,
                    Some("Self") => Some(ObjRef::This),
                    Some("ThisTargetedCard") if own_target.is_some() => {
                        own_target.map(ObjRef::Target)
                    }
                    Some("Targeted" | "ParentTarget") if own_target.or(last).is_some() => {
                        chain.refers_to_targets.set(true);
                        own_target.or(last).map(ObjRef::Target)
                    }
                    Some(name) if triggered && self.trig.named(name).is_some() => {
                        match self.trig.named(name) {
                            Some(Named::This) => Some(ObjRef::This),
                            _ => Some(ObjRef::Triggering),
                        }
                    }
                    Some(other) => {
                        self.block(format!("Effect:remembers {}", shape(other)));
                        return None;
                    }
                };
                // Only an object is remembered here (a player would silently be nothing).
                if let Some(ObjRef::Target(slot)) = about
                    && !matches!(
                        chain.targets.get(slot as usize),
                        Some(TargetFilter::Creature | TargetFilter::Permanent(_))
                    )
                {
                    self.block("Effect:remembers a target that isn't a permanent");
                    return None;
                }
                let (until_end_of_turn, while_source) = match rd.get("Duration") {
                    None => (true, false),
                    Some("Permanent") => (false, false),
                    Some("UntilHostLeavesPlayOrEOT") if !self.in_spell => (true, true),
                    Some("UntilHostLeavesPlay") if !self.in_spell => (false, true),
                    Some(other) => {
                        self.block(format!("Effect:duration {other}"));
                        return None;
                    }
                };
                // "Forget it when it leaves the battlefield": a permanent that leaves is
                // a new object anyway (CR 400.7), so this says nothing more here.
                for key in ["ExileOnMoved", "ForgetOnMoved"] {
                    match rd.get(key) {
                        None => {}
                        Some("Battlefield") if about.is_some() => {}
                        Some(zone) => {
                            self.block(format!("Effect:{key} {zone}"));
                            return None;
                        }
                    }
                }
                // What it carries. Each kind of rule has to be one a lasting effect can
                // carry in the engine: named here, so that what is missing can be ranked.
                let mut abilities = Vec::new();
                let was = std::mem::replace(&mut self.in_effect, true);
                let mut carried = Some(());
                for (key, kind) in [
                    ("StaticAbilities", Given::Static),
                    ("ReplacementEffects", Given::Replacement),
                ] {
                    let Some(names) = rd.get(key) else { continue };
                    for name in names.split(',').map(str::trim) {
                        let what = self
                            .records
                            .iter()
                            .find(|(n, _)| n == name)
                            .and_then(|(_, r)| r.get("Mode").or(r.get("Event")));
                        let known = matches!(
                            (kind, what),
                            (
                                Given::Static,
                                Some(
                                    "CantBlockBy"
                                        | "CantAttack"
                                        | "CantBlock"
                                        | "CantAttack,CantBlock"
                                        | "CantRegenerate"
                                        // (Only one whose "keywords" are rules: see below.)
                                        | "Continuous"
                                )
                            ) | (Given::Replacement, Some("DamageDone"))
                        );
                        if !known {
                            self.block(format!(
                                "Effect:{} {}",
                                if kind == Given::Static {
                                    "static"
                                } else {
                                    "repl"
                                },
                                what.unwrap_or("?")
                            ));
                            carried = None;
                            continue;
                        }
                        match self.grant(kind, name, ',') {
                            // A static ability that changes characteristics is not
                            // something a lasting effect carries.
                            Some(given)
                                if given.iter().any(|i| {
                                    matches!(self.granted[*i as usize], AbilityDef::Static { .. })
                                }) =>
                            {
                                self.block("Effect:static Continuous");
                                carried = None;
                            }
                            Some(given) => abilities.extend(given),
                            None => carried = None,
                        }
                    }
                }
                self.in_effect = was;
                for key in ["Triggers", "Abilities"] {
                    if rd.get(key).is_some() {
                        self.block(format!("Effect:{key}"));
                        carried = None;
                    }
                }
                // (The effect's name is shown nowhere.)
                rd.get("Name");
                carried?;
                if abilities.is_empty() {
                    self.block("Effect:nothing");
                    return None;
                }
                Effect::Lasting {
                    abilities,
                    about,
                    until_end_of_turn,
                    while_source,
                }
            }
            "Pump" | "PumpAll" => {
                let (effects, rules) = self.pump_effects(&rd, chain)?;
                // The rules are no change to the creatures (CR 113.12): they hold for
                // the turn as a lasting effect — about the one creature, or (CR 611.2c:
                // a rule isn't locked in) about whatever matches meanwhile.
                let mut parts = Vec::new();
                if api == "PumpAll" {
                    let sel = self.valid_cards(&rd, api)?;
                    if !effects.is_empty() {
                        parts.push(Effect::ApplyAll {
                            affected: sel.clone(),
                            effects,
                            until_end_of_turn: true,
                        });
                    }
                    if !rules.is_empty() {
                        let abilities = self.carry(&rules, &Affected::All(sel))?;
                        parts.push(Effect::Lasting {
                            abilities,
                            about: None,
                            until_end_of_turn: true,
                            while_source: false,
                        });
                    }
                } else {
                    let slot = self.target(&rd, chain, false);
                    let what = match slot {
                        Some(i) => Some(ObjRef::Target(i)),
                        None => self.object(&rd, chain, triggered),
                    }?;
                    if !effects.is_empty() {
                        parts.push(Effect::Apply {
                            what,
                            effects,
                            until_end_of_turn: true,
                        });
                    }
                    if !rules.is_empty() {
                        let abilities = self.carry(&rules, &Affected::Remembered)?;
                        parts.push(Effect::Lasting {
                            abilities,
                            about: Some(what),
                            until_end_of_turn: true,
                            while_source: false,
                        });
                    }
                }
                if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    Effect::Seq(parts)
                }
            }
            "Sacrifice" => {
                // "Each player sacrifices N creatures of their choice."
                let shape = (rd.get("Defined"), rd.get("SacValid"));
                let n = rd.get("Amount").unwrap_or("1").parse::<u32>().ok();
                match (shape, n) {
                    ((Some("Player"), Some("Creature")), Some(n)) => {
                        Effect::EachPlayer(EachPlayerAction::SacrificeCreature(n))
                    }
                    // "Sacrifice this."
                    ((None | Some("Self"), None | Some("Self")), _)
                        if rd.rec.get("ValidTgts").is_none() =>
                    {
                        if rd.rec.get("Amount").is_some() {
                            self.block("Sacrifice:shape");
                            return None;
                        }
                        Effect::SacrificeThis
                    }
                    ((_, Some(valid)), _) if valid != "Self" => {
                        let who = match self.target(&rd, chain, false) {
                            Some(i) if chain.targets[i as usize].allows_players() => {
                                Some(PlayerRef::Target(i))
                            }
                            Some(_) => {
                                self.block("Sacrifice:shape");
                                return None;
                            }
                            None => self.player(&rd, chain, triggered),
                        };
                        let what = self.selector(valid);
                        let amount = self.amount(rd.get("Amount").unwrap_or("1"), chain);
                        Effect::Sacrifice {
                            who: who?,
                            what: what?,
                            amount: amount?,
                        }
                    }
                    _ => {
                        self.block("Sacrifice:shape");
                        return None;
                    }
                }
            }
            "Discard" => {
                // "Each player discards N cards" (of their choice).
                let shape = (rd.get("Defined"), rd.get("Mode"));
                let n = rd.get("NumCards").unwrap_or("1").parse::<u32>().ok();
                match (shape, n) {
                    ((Some("Player"), Some("TgtChoose")), Some(n)) => {
                        Effect::EachPlayer(EachPlayerAction::Discard(n))
                    }
                    ((_, Some(mode @ ("TgtChoose" | "Hand"))), _) => {
                        let who = match self.target(&rd, chain, false) {
                            Some(i) if chain.targets[i as usize].allows_players() => {
                                Some(PlayerRef::Target(i))
                            }
                            Some(_) => {
                                self.block("Discard:shape");
                                return None;
                            }
                            None => self.player(&rd, chain, triggered),
                        };
                        if mode == "Hand" {
                            if rd.rec.get("NumCards").is_some() {
                                self.block("Discard:shape");
                                return None;
                            }
                            Effect::DiscardHand { who: who? }
                        } else {
                            let amount = self.amount(rd.get("NumCards").unwrap_or("1"), chain);
                            Effect::Discard {
                                who: who?,
                                amount: amount?,
                            }
                        }
                    }
                    ((_, mode), _) => {
                        self.block(format!("Discard:mode {}", mode.unwrap_or("?")));
                        return None;
                    }
                }
            }
            // "Create a token that's a copy of …" (docs/forge/effects.md §28).
            "CopyPermanent" => {
                let what = match self.target(&rd, chain, false) {
                    Some(slot) => CopyWhat::Obj(ObjRef::Target(slot)),
                    None => match rd.rec.get("Defined") {
                        // "Equipped creature", "enchanted creature".
                        Some("Equipped" | "Enchanted") => {
                            rd.get("Defined");
                            CopyWhat::Obj(ObjRef::Host)
                        }
                        // "For each token you control, create a token that's a copy of it."
                        Some(d) if d.starts_with("Valid ") => {
                            rd.get("Defined");
                            CopyWhat::Each(self.selector(&d["Valid ".len()..])?)
                        }
                        // A copy of the object as it last existed, even if it is gone
                        // (CR 111.12): not compiled — a copy is made of what is still
                        // there.
                        Some(d) if d.ends_with("LKICopy") => {
                            rd.get("Defined");
                            self.block("CopyPermanent:copy of what it last was");
                            return None;
                        }
                        _ => CopyWhat::Obj(self.object(&rd, chain, triggered)?),
                    },
                };
                let count = self.amount(rd.get("NumCopies").unwrap_or("1"), chain);
                let except = self.copy_except(&rd);
                let mut gains = Vec::new();
                if let Some(ks) = rd.get("PumpKeywords") {
                    for k in ks.split('&').map(str::trim) {
                        match keyword(k) {
                            Some(k) => gains.push(k),
                            None => {
                                self.block(format!("copy:keyword {k}"));
                                return None;
                            }
                        }
                    }
                }
                let tapped = rd.get("TokenTapped") == Some("True");
                // "Sacrifice / exile it at the beginning of the next end step": a delayed
                // triggered ability about the token (CR 603.7c) — delayed triggers that
                // remember objects aren't built (D-075 has players and amounts only).
                let copy = Effect::CopyPermanent {
                    what,
                    count: count?,
                    except: except?,
                    gains,
                    tapped,
                };
                // "Sacrifice / exile it at the beginning of the next end step": a delayed
                // triggered ability about the tokens just made (CR 603.7c) — they are
                // noted (D-069) and handed to it. Not in a chain that notes other things.
                match rd.get("AtEOT") {
                    None => copy,
                    Some(what) if !self.memory && at_eot(what).is_some() => {
                        let (action, step, whose) = at_eot(what).expect("checked");
                        self.at_end_of_turn(copy, action, step, whose)
                    }
                    Some(what) => {
                        self.block(format!("CopyPermanent:AtEOT {what}"));
                        return None;
                    }
                }
            }
            // "This permanent becomes a copy of …" for good (docs/forge/effects.md §43):
            // no `Duration$`, no `CloneTarget$`, no `Choices$` (all left unread).
            "Clone" => {
                let what = match self.target(&rd, chain, false) {
                    Some(slot) => ObjRef::Target(slot),
                    None => match rd.rec.get("Defined") {
                        None => {
                            self.block("Clone:copies nothing named");
                            return None;
                        }
                        Some(_) => self.object(&rd, chain, triggered)?,
                    },
                };
                if what == ObjRef::This {
                    self.block("Clone:a copy of itself");
                    return None;
                }
                Effect::BecomeCopy {
                    what,
                    except: self.copy_except(&rd)?,
                }
            }
            // "Forget them" as the chain's last link (`ClearRemembered$ True`): the end of
            // the resolution forgets anyway (D-069).
            "Cleanup" if self.memory => {
                rd.get("ClearRemembered");
                Effect::Seq(Vec::new())
            }
            other => {
                self.block(format!("api:{other}"));
                return None;
            }
        };
        // What this link remembers (D-069): only in a chain that forgets at its end.
        let remembers = self.memory && {
            let keys: &[&str] = match api {
                "ChangeZone" if root && chain.targets.len() > targets_before => {
                    &["RememberChanged", "RememberLKI", "RememberTargets"]
                }
                "ChangeZone" => &["RememberChanged", "RememberLKI"],
                "Dig" => &["RememberChanged"],
                "DestroyAll" => &["RememberDestroyed"],
                "Sacrifice" => &["RememberSacrificed"],
                "Token" => &["RememberTokens"],
                "Mill" => &["RememberMilled"],
                "Discard" => &["RememberDiscarded"],
                _ => &[],
            };
            // (Forge tests these for presence.)
            keys.iter().filter(|k| rd.get(k).is_some()).count() > 0
        };
        rd.finish(&mut self.blockers);
        // One "target" word with several targets: the instruction is done to each of
        // them, at the same time. Only for instructions that are one event each.
        let out = match chain.fresh_group.take() {
            Some((first, count)) if count > 1 => {
                let single = matches!(
                    out,
                    Effect::DealDamage { .. }
                        | Effect::Destroy { .. }
                        | Effect::Exile { .. }
                        | Effect::ReturnToHand { .. }
                        | Effect::Tap { .. }
                        | Effect::Untap { .. }
                        | Effect::PutCounters { .. }
                        | Effect::Apply { .. }
                        | Effect::GainLife { .. }
                        | Effect::LoseLife { .. }
                        | Effect::Draw { .. }
                        | Effect::Mill { .. }
                );
                let copies: Option<Vec<Effect>> = (0..count)
                    .map(|j| Self::retarget(&out, first, first + j))
                    .collect();
                match copies {
                    Some(copies) if single && copies[0] != copies[1] => Effect::Together(copies),
                    _ => {
                        self.block(format!("targets:several for {api}"));
                        return None;
                    }
                }
            }
            _ => out,
        };
        let out = if remembers {
            self.note(out, api, chain)?
        } else {
            // A move of "those cards" takes them where it puts them.
            if let Some(at) = noted_moved(&out) {
                chain.noted_at = Some(at);
            }
            out
        };
        let (Some(price), payer, switched) = unless else {
            return Some(match condition {
                None => out,
                Some(test) => Effect::If {
                    test,
                    then: Box::new(out),
                },
            });
        };
        // "Unless [they] pay {X}, where X is …" (CR 118.12, 107.3): the cost is {X}, with X
        // worked out as this instruction is reached (the chain's X).
        let cost = if price == "X" && self.face.svars.contains_key("X") {
            match self.amount("X", chain) {
                Some(Amount::X) => Cost {
                    mana: Some(ManaCost {
                        symbols: vec![card_model::ManaSymbol::X],
                    }),
                    ..Cost::default()
                },
                Some(_) => {
                    self.block("unless:cost X of something else");
                    return None;
                }
                None => return None,
            }
        } else {
            self.cost(price)?
        };
        if cost == Cost::default() || cost.discard_self || cost.exile_self {
            self.block("unless:cost");
            return None;
        }
        // Who pays: Forge's default is the controller of this instruction's target.
        let own_target =
            (chain.targets.len() == targets_before + 1).then_some(targets_before as u8);
        let who = match (payer, own_target) {
            (None | Some("TargetedController"), Some(slot)) => {
                // Forge has no controller for a player or a card in a graveyard.
                if !matches!(
                    chain.targets[slot as usize],
                    TargetFilter::AnyTarget
                        | TargetFilter::Creature
                        | TargetFilter::Permanent(_)
                        | TargetFilter::Spell
                        | TargetFilter::SpellMatching(_)
                ) {
                    self.block("unless:payer of a player or card");
                    return None;
                }
                PlayerRef::ControllerOf(slot)
            }
            (None, None) => {
                self.block("unless:payer without a target");
                return None;
            }
            // The controller of the chain's target, the player the event happened to, the
            // controller of its object, you.
            (Some(name), _) => self.one_player(name, chain, triggered, "unless:payer ")?,
        };
        if (cost.tap || cost.sacrifice_self) && who != PlayerRef::You {
            self.block("unless:cost uses the source");
            return None;
        }
        let nothing = Box::new(Effect::Seq(Vec::new()));
        let out = if switched {
            // "[They] may pay …. If they do, [this]."
            Effect::Pay {
                who,
                cost,
                paid: Box::new(out),
                unpaid: nothing,
            }
        } else {
            Effect::Pay {
                who,
                cost,
                paid: nothing,
                unpaid: Box::new(out),
            }
        };
        Some(match condition {
            None => out,
            Some(test) => Effect::If {
                test,
                then: Box::new(out),
            },
        })
    }

    /// A link that remembers what it does (D-069): its instruction, noted.
    fn note(&mut self, out: Effect, api: &str, chain: &mut Chain) -> Option<Effect> {
        // Forge's `DestroyAll` forgets everything remembered before.
        if api == "DestroyAll" && chain.noted_at.is_some() {
            self.block("remember:DestroyAll forgets what came before");
            return None;
        }
        if chain.noted_at.is_some() {
            self.block("remember:twice in one chain");
            return None;
        }
        // "You may search …": what is noted is what the search found, if there was one.
        let (out, may) = match out {
            Effect::May(inner) => (*inner, true),
            other => (other, false),
        };
        let out = match out {
            // A search whose cards stay in the library (Forge moves them to its top and
            // back) is a search whose found cards wait for what comes next.
            Effect::ChooseCards {
                from: CardZone::Library,
                to: CardDest::LibraryTop,
                shuffle: false,
                ..
            } => {
                let mut e = out;
                if let Effect::ChooseCards { to, .. } = &mut e {
                    *to = CardDest::Stay;
                }
                e
            }
            Effect::ChooseCards {
                from: CardZone::Library,
                to: CardDest::LibraryTop | CardDest::LibraryBottom,
                ..
            } => {
                self.block("remember:found cards put back in the library");
                return None;
            }
            Effect::Dig { to, rest, .. }
                if matches!(to, CardDest::LibraryTop | CardDest::LibraryBottom) || to == rest =>
            {
                self.block("remember:Dig that leaves what it takes in the library");
                return None;
            }
            other => other,
        };
        let Some(at) = noted_zone(&out) else {
            self.block(format!("remember:{api} of this shape"));
            return None;
        };
        chain.noted_at = Some(at);
        let noted = Effect::Note(Box::new(out));
        Some(if may {
            Effect::May(Box::new(noted))
        } else {
            noted
        })
    }

    /// `Condition…$` of one link of a chain: `ConditionPresent$` (+ `ConditionCompare$`)
    /// counts permanents on the battlefield — or, with `ConditionDefined$ Remembered`,
    /// "those cards" — and `ConditionCheckSVar$` (+ `ConditionSVarCompare$`) compares an
    /// amount; both are counted as the link is reached.
    fn link_condition(&mut self, rd: &Rd<'a>) -> Option<Option<Condition>> {
        let (present, check) = (
            rd.rec.get("ConditionPresent"),
            rd.rec.get("ConditionCheckSVar"),
        );
        if present.is_none() && check.is_none() {
            return Some(None);
        }
        rd.get("ConditionDescription");
        let mut out = None;
        if let Some(filter) = present {
            rd.get("ConditionPresent");
            let (cmp, n) = self.compare(rd.get("ConditionCompare").unwrap_or("GE1"))?;
            out = Some(match rd.rec.get("ConditionDefined") {
                None => {
                    let what = self.selector(filter)?;
                    match cmp {
                        Cmp::AtLeast => Condition::Control { what, at_least: n },
                        Cmp::AtMost => Condition::ControlAtMost { what, at_most: n },
                        _ => Condition::Compare {
                            amount: Amount::Count(what),
                            cmp,
                            n,
                        },
                    }
                }
                Some(name) if self.memory && !self.in_effect && self.is_remembered(name) => {
                    rd.get("ConditionDefined");
                    let cards = self.card_alts(filter, true)?;
                    if cards
                        .iter()
                        .any(|s| s.controller != Who::Any || s.owner != Who::Any)
                    {
                        self.block("condition:remembered whose");
                        return None;
                    }
                    Condition::Compare {
                        amount: Amount::Noted(cards),
                        cmp,
                        n,
                    }
                }
                Some(_) => {
                    self.block(format!("param:{}.ConditionDefined", rd.what));
                    return None;
                }
            });
        }
        if let Some(name) = check {
            if out.is_some() {
                self.block("condition:two");
                return None;
            }
            rd.get("ConditionCheckSVar");
            let (cmp, n) = self.compare(rd.get("ConditionSVarCompare").unwrap_or("GE1"))?;
            let amount = match name.parse::<u32>() {
                Ok(n) => Amount::Fixed(n),
                Err(_) => self.expr(name)?,
            };
            out = Some(Condition::Compare { amount, cmp, n });
        }
        Some(out)
    }

    /// "Return those cards to the battlefield", "… to their owners' hands": a move of the
    /// cards the chain noted (D-069), from where they are.
    fn move_noted(
        &mut self,
        rd: &Rd<'a>,
        chain: &mut Chain,
        dest: Option<&str>,
        controller: Option<PlayerRef>,
    ) -> Option<Effect> {
        let at = chain.noted_at;
        // Forge's `Origin$`: where the cards must be (`All`, or none: wherever they are).
        match rd.rec.get("Origin") {
            None | Some("All") => {}
            Some(o) if Some(o) == at => {}
            Some(_) => {
                self.block("ChangeZone:those cards are elsewhere");
                return None;
            }
        }
        // A card put into a hand can't be found again (CR 400.7j); cards a search found
        // are moved by Forge's searching form (shuffling), not this one.
        if matches!(at, Some("Hand" | "Library")) {
            self.block("ChangeZone:those cards in a hidden zone");
            return None;
        }
        let what = ObjRef::Noted;
        let no_shuffle = |tx: &mut Self| match rd.get("Shuffle") {
            None => Some(false),
            Some("True") => Some(true),
            Some(_) => {
                tx.block("param:ChangeZone.Shuffle");
                None
            }
        };
        Some(match dest {
            Some("Battlefield") => {
                let tapped = match rd.get("Tapped") {
                    None => false,
                    Some("True") => true,
                    Some(_) => {
                        self.block("param:ChangeZone.Tapped");
                        return None;
                    }
                };
                Effect::PutInto {
                    what,
                    to: CardDest::Battlefield { tapped },
                    shuffle: false,
                    controller,
                }
            }
            Some("Hand") => Effect::ReturnToHand { what },
            Some("Exile") => Effect::Exile { what },
            Some("Graveyard") => Effect::PutInto {
                what,
                to: CardDest::Graveyard,
                shuffle: false,
                controller: None,
            },
            Some("Library") => {
                let to = self.library_position(rd)?;
                let shuffle = no_shuffle(self)?;
                Effect::PutInto {
                    what,
                    to,
                    shuffle,
                    controller: None,
                }
            }
            other => {
                self.block(format!("ChangeZone:those cards->{}", other.unwrap_or("?")));
                return None;
            }
        })
    }

    /// `LibraryPosition$` of a move to a library: `0` (the default) is the top, `-1` the
    /// bottom.
    fn library_position(&mut self, rd: &Rd<'a>) -> Option<CardDest> {
        match rd.get("LibraryPosition") {
            None | Some("0") => Some(CardDest::LibraryTop),
            Some("-1") => Some(CardDest::LibraryBottom),
            Some(_) => {
                self.block("param:ChangeZone.LibraryPosition");
                None
            }
        }
    }

    /// The alternatives of a filter as selectors (a card qualifies if it matches any).
    /// `Card` alone is "anything": no selectors. Off the battlefield `Permanent` is a
    /// permanent card (CR 110.4a).
    fn card_alts(&mut self, text: &str, off_battlefield: bool) -> Option<Vec<Selector>> {
        if text.trim() == "Card" {
            return Some(Vec::new());
        }
        let mut out = Vec::new();
        for alt in text.split(',') {
            let alt = alt.trim();
            match self.alt(alt)? {
                Alt::Sel(mut s) => {
                    if off_battlefield {
                        s = owned(s);
                    }
                    if off_battlefield && alt.split('.').next() == Some("Permanent") {
                        s.not_types.extend([CardType::Instant, CardType::Sorcery]);
                    }
                    out.push(s);
                }
                Alt::This => {
                    self.block("filter:Self-among-cards");
                    return None;
                }
            }
        }
        Some(out)
    }

    /// A ChangeZone that takes cards out of a hidden zone as it resolves (Forge's
    /// "hidden-origin mode", docs/forge/effects.md §1): a library search (CR 701.23), or
    /// cards chosen from a hand.
    fn choose_cards(&mut self, rd: &Rd<'a>, chain: &mut Chain, triggered: bool) -> Option<Effect> {
        let from = match rd.get("Origin") {
            Some("Library") => CardZone::Library,
            Some("Graveyard") => CardZone::Graveyard,
            Some("Battlefield") => CardZone::Battlefield,
            _ => CardZone::Hand,
        };
        rd.get("Hidden");
        let public = matches!(from, CardZone::Graveyard | CardZone::Battlefield);
        // Whose zone: the activator's unless `DefinedPlayer$` says otherwise.
        let who = match rd.get("DefinedPlayer") {
            None => PlayerRef::You,
            Some("Player") => PlayerRef::Each,
            Some("Player.Opponent" | "Opponent") => PlayerRef::EachOpponent,
            // "Its controller searches their library": of the target, of the event's object.
            Some(name) => self.one_player(name, chain, triggered, "defined:player ")?,
        };
        let filter = rd.get("ChangeType");
        let cards = match filter {
            None => Some(Vec::new()),
            Some(f) => self.card_alts(f, from != CardZone::Battlefield),
        };
        // Choosing among "those cards" (D-069) looks at nothing new.
        if cards
            .as_ref()
            .is_some_and(|c| !c.is_empty() && c.iter().all(|s| s.noted == Some(true)))
        {
            rd.get("NoLooking");
        }
        // In a public zone Forge offers every player's cards that fit; the engine offers
        // the chooser's own. The same thing only when the filter says "your".
        if public
            && !cards
                .as_ref()
                .is_some_and(|c| !c.is_empty() && c.iter().all(|s| s.controller == Who::You))
        {
            self.block("ChangeZone:chosen among every player's cards");
            return None;
        }
        let count = self.amount(rd.get("ChangeNum").unwrap_or("1"), chain);
        // Without `Mandatory$` every pick may be declined (CR 701.23b).
        let must = rd.get("Mandatory").is_some();
        let to = match rd.get("Destination") {
            Some("Battlefield") => Some(CardDest::Battlefield {
                tapped: rd.get("Tapped").is_some(),
            }),
            Some("Hand") => Some(CardDest::Hand),
            Some("Graveyard") => Some(CardDest::Graveyard),
            Some("Exile") => Some(CardDest::Exile),
            Some("Library") => self.library_position(rd),
            other => {
                self.block(format!("ChangeZone:->{}", other.unwrap_or("?")));
                None
            }
        };
        let to = to?;
        if rd.rec.get("Tapped").is_some() && !matches!(to, CardDest::Battlefield { .. }) {
            self.block("param:ChangeZone.Tapped");
            return None;
        }
        // `Reorder$`: the player who chose the cards puts them on their library "in any
        // order" (Brainstorm). The engine always has them ordered by that player — the
        // owner, CR 401.4 — so it only needs to be where they go to a library.
        if rd.get("Reorder").is_some()
            && !matches!(to, CardDest::LibraryTop | CardDest::LibraryBottom)
        {
            self.block("param:ChangeZone.Reorder not to a library");
            return None;
        }
        // Forge reveals what was found when a quality was asked for and the cards don't
        // go to the battlefield ("search for a creature card, reveal it, …").
        let restricted = filter.is_some_and(|f| f.trim() != "Card");
        let reveal = !public
            && rd.get("NoReveal").is_none()
            && (rd.get("Reveal").is_some()
                || (restricted && !matches!(to, CardDest::Battlefield { .. })));
        // A library that was searched is shuffled (CR 701.23, the effects all say so)
        // unless the script says not to. `ShuffleNonMandatory$` only matters when a player
        // can't search at all, which nothing in this engine causes.
        rd.get("ShuffleNonMandatory");
        let no_shuffle = rd.get("NoShuffle").is_some() || rd.get("Shuffle") == Some("False");
        if let Some(v) = rd.rec.get("Shuffle")
            && (from != CardZone::Library || !matches!(v, "True" | "False"))
        {
            self.block("param:ChangeZone.Shuffle");
            return None;
        }
        let shuffle = from == CardZone::Library && !no_shuffle;
        // "… that share a land type" (CR 205.3i).
        let share_land_type = rd.get("ShareLandType").is_some();
        // "May search": Forge asks each searching player (docs/forge/effects.md §1), before
        // anything is looked at.
        let optional = rd.get("Optional").is_some();
        let effect = Effect::ChooseCards {
            who,
            from,
            cards: cards?,
            count: count?,
            must,
            to,
            reveal,
            shuffle,
            optional: optional && who != PlayerRef::You,
            share_land_type,
        };
        // "You may search…": one yes/no of the controller's.
        if optional {
            return Some(if who == PlayerRef::You {
                Effect::May(Box::new(effect))
            } else {
                effect
            });
        }
        // The card says "you may search" where the script has a plain search: Forge lets
        // the search find nothing and always shuffles, the card lets the player leave the
        // library alone. Where the card has one search and says "may" once, that search
        // is the optional one; anything less clear is not compiled.
        let oracle = self.face.oracle.as_deref().unwrap_or("").to_lowercase();
        let says_may = oracle.matches("may search").count();
        if from != CardZone::Library || self.optional_trigger || says_may == 0 {
            return Some(effect);
        }
        let searches = self
            .face
            .abilities
            .iter()
            .chain(self.records.iter().map(|(_, r)| r))
            .filter(|r| {
                r.head().is_some_and(|h| h.1 == "ChangeZone") && r.get("Origin") == Some("Library")
            })
            .count();
        if says_may == 1 && searches == 1 {
            // "You may search", or "that player may search": the searcher decides.
            Some(match effect {
                Effect::ChooseCards {
                    who: PlayerRef::You,
                    ..
                } => Effect::May(Box::new(effect)),
                Effect::ChooseCards {
                    who,
                    from,
                    cards,
                    count,
                    must,
                    to,
                    reveal,
                    shuffle,
                    share_land_type,
                    ..
                } => Effect::ChooseCards {
                    who,
                    from,
                    cards,
                    count,
                    must,
                    to,
                    reveal,
                    shuffle,
                    optional: true,
                    share_land_type,
                },
                other => other,
            })
        } else {
            self.block("ChangeZone:optional search differs from the card");
            None
        }
    }

    /// Types an effect adds ("is a creature in addition to its other types"): card types,
    /// basic land types and known subtypes. Supertypes are not added by anything yet.
    fn added_types<'t>(
        &mut self,
        words: impl Iterator<Item = &'t str>,
    ) -> Option<ContinuousEffect> {
        let words: Vec<&str> = words.map(str::trim).collect();
        // "Is the chosen type in addition to its other types" (CR 607.2d, 205.3d).
        if words.contains(&"ChosenType") {
            if words.len() == 1 {
                return Some(ContinuousEffect::AddChosenType);
            }
            self.block("type added:ChosenType and others");
            return None;
        }
        let (mut card_types, mut subtypes, mut land_types) = (Vec::new(), Vec::new(), Vec::new());
        for word in words {
            if let Some(t) = card_type(word) {
                card_types.push(t);
                continue;
            }
            match word {
                "Plains" => land_types.push(BasicLandType::Plains),
                "Island" => land_types.push(BasicLandType::Island),
                "Swamp" => land_types.push(BasicLandType::Swamp),
                "Mountain" => land_types.push(BasicLandType::Mountain),
                "Forest" => land_types.push(BasicLandType::Forest),
                // "Every basic land type" (CR 205.3i).
                "AllBasicLandType" => land_types.extend([
                    BasicLandType::Plains,
                    BasicLandType::Island,
                    BasicLandType::Swamp,
                    BasicLandType::Mountain,
                    BasicLandType::Forest,
                ]),
                w if self.known.subtypes.contains(w) => subtypes.push(w.to_string()),
                w => {
                    self.block(format!("type added:{}", shape(w)));
                    return None;
                }
            }
        }
        Some(ContinuousEffect::AddTypes {
            card_types,
            subtypes,
            land_types,
        })
    }

    /// `NumAtt$` / `NumDef$` / `KW$` of Pump and PumpAll (which last until end of turn):
    /// what changes about the creatures, and the rules that hold for them meanwhile
    /// ("can't block this turn").
    fn pump_effects(
        &mut self,
        rd: &Rd<'a>,
        chain: &mut Chain,
    ) -> Option<(Vec<ContinuousEffect>, Vec<Hidden>)> {
        let int =
            |t: Option<&str>| t.map_or(Some(0), |t| t.trim_start_matches('+').parse::<i32>().ok());
        let (att, def) = (rd.get("NumAtt"), rd.get("NumDef"));
        let mut effects = Vec::new();
        match (int(att), int(def)) {
            (Some(p), Some(t)) => {
                if (p, t) != (0, 0) {
                    effects.push(ContinuousEffect::ModifyPT(p, t));
                }
            }
            // "+X/+X", "-X/-0": worked out as the effect resolves.
            _ => {
                let mut part = |text: Option<&str>| -> Option<Signed> {
                    let text = text.unwrap_or("0");
                    let negative = text.starts_with('-');
                    let amount = self.amount(text.trim_start_matches(['+', '-']), chain)?;
                    Some(Signed { negative, amount })
                };
                let (power, toughness) = (part(att), part(def));
                effects.push(ContinuousEffect::ModifyPTBy {
                    power: Box::new(power?),
                    toughness: Box::new(toughness?),
                });
            }
        }
        let mut rules = Vec::new();
        if let Some(kw) = rd.get("KW") {
            let mut ks = Vec::new();
            for k in kw.split('&').map(str::trim) {
                match (keyword(k), hidden_rules(k)) {
                    (Some(k), _) => ks.push(k),
                    (None, Some(hidden)) => rules.extend(hidden),
                    (None, None) => {
                        self.block(format!("kw:{k}"));
                        return None;
                    }
                }
            }
            if !ks.is_empty() {
                effects.push(ContinuousEffect::AddKeywords(ks));
            }
        }
        if effects.is_empty() && rules.is_empty() {
            self.block("Pump:nothing");
            return None;
        }
        Some((effects, rules))
    }

    /// Does the chain starting at `first` keep its "remembered" cards to itself (D-069)?
    /// It remembers some, and its last link — its only `Cleanup` — forgets them. Then
    /// whatever reads them is in the chain, between the two (Forge's memory lives on the
    /// card, and no other chain of a compiled card leaves anything in it).
    fn local_memory(&self, first: &'a Record) -> (bool, bool) {
        let mut links = vec![first];
        while let Some(name) = links.last().and_then(|r| r.get("SubAbility")) {
            match self.records.iter().find(|(n, _)| n == name) {
                Some((_, r)) if links.len() < 32 => links.push(r),
                _ => break,
            }
        }
        let writes = links
            .iter()
            .any(|r| REMEMBER.iter().any(|k| r.get(k).is_some()));
        let cleanup = |r: &Record| r.head().map(|h| h.1) == Some("Cleanup");
        let forgets = links.last().is_some_and(|r| {
            cleanup(r)
                && r.get("ClearRemembered") == Some("True")
                && !r.params.iter().any(|(k, _)| k.starts_with("Condition"))
        }) && links.iter().filter(|r| cleanup(r)).count() == 1;
        (writes && forgets, writes)
    }

    /// A whole chain (an ability and its `SubAbility$` descendants) as one mode.
    fn mode(&mut self, first: &'a Record, triggered: bool, root: &str) -> Option<Mode> {
        let (local, writes) = self.local_memory(first);
        if writes && !local {
            self.block("remember:not forgotten when the chain ends");
        }
        // A delayed ability's objects are its "those cards" (CR 603.7c), forgotten as it
        // finishes resolving.
        let local = local || (self.delayed_noted.is_some() && !writes);
        let was = std::mem::replace(&mut self.memory, local);
        let mode = self.chain_mode(first, triggered, root);
        self.memory = was;
        mode
    }

    fn chain_mode(&mut self, first: &'a Record, triggered: bool, root: &str) -> Option<Mode> {
        let mut chain = Chain {
            noted_at: self.delayed_noted.take(),
            ..Chain::default()
        };
        let mut kind = root;
        // A trigger that runs an `AB$` with a cost: "you may pay [cost]. If you do, [the
        // whole ability]" (docs/forge/triggers-statics-replacements-keywords.md).
        let mut price = None;
        if triggered
            && root == "DB"
            && first.head().map(|h| h.0) == Some("AB")
            && let Some(text) = first.get("Cost")
        {
            kind = "AB";
            let cost = self.cost(text)?;
            if cost == Cost::default() || cost.discard_self || cost.exile_self {
                self.block("trigger:cost");
                return None;
            }
            price = Some(cost);
        }
        let mut next = Some(first);
        let mut ok = true;
        let mut steps = 0;
        while let Some(rec) = next {
            steps += 1;
            if steps > 32 {
                self.block("chain:too-long");
                return None;
            }
            match self.effect(rec, &mut chain, triggered, kind) {
                // (`Cleanup` at the end of a chain that remembers: nothing to do.)
                Some(Effect::Seq(v)) if v.is_empty() => {}
                Some(e) => chain.effects.push(e),
                None => ok = false,
            }
            kind = "DB";
            next = match rec.get("SubAbility") {
                None => None,
                Some(name) => match self.sub(name) {
                    Some(r) => Some(r),
                    None => {
                        ok = false;
                        None
                    }
                },
            };
        }
        // `Defined$ Targeted` means every target of the chain; with exactly one target slot
        // that is unambiguous.
        if chain.refers_to_targets.get() && chain.targets.len() != 1 {
            self.block("defined:Targeted-with-several-targets");
            ok = false;
        }
        if !ok {
            return None;
        }
        // The link that moved the target a later link refers to: noted (the only thing
        // the chain notes).
        if chain.note_moved_target.get() {
            let Some((i, _)) = chain.target_moved else {
                unreachable!("set only with a moved target")
            };
            let plain = matches!(
                chain.effects.get(i),
                Some(
                    Effect::Exile {
                        what: ObjRef::Target(_)
                    } | Effect::ReturnToBattlefield {
                        what: ObjRef::Target(_)
                    } | Effect::PutInto {
                        what: ObjRef::Target(_),
                        ..
                    }
                )
            );
            if !plain || format!("{:?}", chain.effects).contains("Note(") {
                self.block("defined:Targeted after it moved");
                return None;
            }
            let e = chain.effects[i].clone();
            chain.effects[i] = Effect::Note(Box::new(e));
        }
        // Cards a search left in the library for what comes next are shuffled away by the
        // end: a later search of that library shuffles it.
        if let Some(i) = chain
            .effects
            .iter()
            .position(|e| format!("{e:?}").contains("to: Stay"))
            && !chain.effects[i + 1..].iter().any(|e| {
                let t = format!("{e:?}");
                t.contains("from: Library") && t.contains("shuffle: true")
            })
        {
            self.block("remember:found cards not shuffled away");
            return None;
        }
        let mut effects = chain.effects;
        if let Some((_, amount)) = chain.x {
            effects.insert(chain.x_at.min(effects.len()), Effect::SetX(amount));
        }
        let effect = if effects.len() == 1 {
            effects.remove(0)
        } else {
            Effect::Seq(effects)
        };
        let effect = match price {
            None => effect,
            Some(cost) => Effect::Pay {
                who: PlayerRef::You,
                cost,
                paid: Box::new(effect),
                unpaid: Box::new(Effect::Seq(Vec::new())),
            },
        };
        // Slots left empty are simply missing from the end of the chosen targets.
        let slots = chain.targets.len() as u8;
        if chain
            .groups
            .iter()
            .any(|g| g.min < g.count && g.first + g.count != slots)
        {
            self.block("targets:up-to group before other targets");
            return None;
        }
        Some(Mode {
            targets: chain.targets,
            effect,
            groups: chain.groups,
        })
    }

    /// The record of a script variable that holds an ability.
    fn sub(&mut self, name: &str) -> Option<&'a Record> {
        self.used_svars.insert(name.to_string());
        let records = self.records;
        records
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, r)| r)
            .or_else(|| {
                self.block(format!("svar:missing {name}"));
                None
            })
    }

    /// The body of a spell, activated ability or trigger: a single mode, or a Charm.
    /// A delayed triggered ability (docs/forge/effects.md §24, CR 603.7): so far "at the
    /// beginning of [step]", remembering nothing, the countered spell's mana value, or its
    /// controller. The line is the trigger: `Mode$`, `Phase$`, `ValidPlayer$`, `Execute$`.
    fn delayed_trigger(&mut self, rd: &Rd<'a>, chain: &mut Chain) -> Option<Effect> {
        if rd.rec.get("Mode") != Some("Phase") {
            self.block(format!(
                "DelayedTrigger:mode {}",
                rd.rec.get("Mode").unwrap_or("?")
            ));
            return None;
        }
        rd.get("Mode");
        let mut steps = Vec::new();
        for name in rd.get("Phase").unwrap_or("").split(',') {
            match phase_step(name.trim()) {
                Some(s) => steps.push(s),
                None => {
                    self.block(format!("trig:Phase {}", name.trim()));
                    return None;
                }
            }
        }
        let whose = match rd.get("ValidPlayer") {
            None | Some("Player") => Who::Any,
            Some("You") => Who::You,
            Some("Opponent" | "Player.Opponent") => Who::Opponent,
            Some(other) => {
                self.block(format!("trig:Phase player {other}"));
                return None;
            }
        };
        let events: Vec<Trigger> = steps
            .into_iter()
            .map(|step| Trigger::BeginningOf { step, whose })
            .collect();
        let when = match events.len() {
            0 => {
                self.block("DelayedTrigger:no phase");
                return None;
            }
            1 => events.into_iter().next().expect("one"),
            _ => Trigger::Any(events),
        };
        let window = match (rd.get("ThisTurn").is_some(), rd.get("NextTurn").is_some()) {
            (false, false) => DelayWindow::Next,
            (true, false) => DelayWindow::ThisTurn,
            (false, true) => DelayWindow::NextTurn,
            (true, true) => {
                self.block("DelayedTrigger:this turn and next turn");
                return None;
            }
        };
        // CR 603.7b: one with a stated duration ("this turn") triggers each time; Forge's
        // (and the engine's) triggers once. Two different steps of one turn ("your main
        // phase this turn") differ, unless the card says "next".
        if window == DelayWindow::ThisTurn
            && matches!(when, Trigger::Any(_))
            && !self
                .face
                .oracle
                .as_deref()
                .is_some_and(|o| o.to_lowercase().contains("next"))
        {
            self.block("DelayedTrigger:this turn, more than once");
            return None;
        }
        // What it remembers, fixed as it is created (CR 603.7c).
        let mut player = None;
        let mut amount = None;
        if rd.get("RememberNumber").is_some() {
            match chain.countered {
                // "That spell's mana value": as it last was on the stack (CR 608.2h).
                Some((slot, _, true)) => {
                    amount = Some(Amount::OfTarget {
                        slot,
                        what: Measure::ManaValue,
                    });
                }
                _ => {
                    self.block("DelayedTrigger:RememberNumber of something else");
                    return None;
                }
            }
        }
        // The objects it is about (CR 603.7c), and the zone they are in: in the delayed
        // ability they are "those cards".
        let mut objects: Option<(ObjRef, &'static str)> = None;
        // They are one object ("it"), as far as the text goes.
        let mut one = false;
        match rd.get("RememberObjects") {
            None => {}
            // What this chain has just moved or made (D-069).
            Some("Remembered" | "RememberedLKI") if self.memory && !self.in_effect => {
                one = notes_one(chain);
                match chain.noted_at {
                    Some(at) => objects = Some((ObjRef::Noted, at)),
                    None => {
                        self.block("remember:read before anything is remembered");
                        return None;
                    }
                }
            }
            // The card a creature that died became ("return that card …").
            Some("TriggeredNewCardLKICopy") if self.in_trigger && self.trig.died => {
                objects = Some((ObjRef::TriggeringCard, "Graveyard"));
                one = true;
            }
            // The permanent the event is about — "that creature" that blocked or became
            // blocked, as it was then (Forge's `…LKICopy`); the delayed ability affects it
            // only while it is still that permanent (CR 603.7c).
            Some(name)
                if self.in_trigger
                    && !self.trig.died
                    && matches!(
                        name,
                        "TriggeredAttackerLKICopy"
                            | "TriggeredBlockerLKICopy"
                            | "TriggeredTargetLKICopy"
                    )
                    && self.trig.named(name).is_some() =>
            {
                let r = match self.trig.named(name) {
                    Some(Named::This) => ObjRef::This,
                    _ => ObjRef::Triggering,
                };
                objects = Some((r, "Battlefield"));
                one = true;
            }
            // The chain's one target, which no link moved ("destroy it at the beginning of
            // the next end step"): affected while it is still that object (CR 603.7c).
            Some("Targeted" | "ParentTarget")
                if chain.targets.len() == 1 && chain.target_moved.is_none() =>
            {
                chain.refers_to_targets.set(true);
                let at = match chain.targets[0] {
                    TargetFilter::CardInGraveyard { .. } => "Graveyard",
                    TargetFilter::Spell | TargetFilter::SpellMatching(_) => {
                        self.block("DelayedTrigger:remembers a spell");
                        return None;
                    }
                    _ if chain.targets[0].allows_players() => {
                        self.block("DelayedTrigger:remembers a target that may be a player");
                        return None;
                    }
                    _ => "Battlefield",
                };
                objects = Some((ObjRef::Target(0), at));
                one = true;
            }
            // "Its controller": of the countered spell's target.
            Some("RememberedController") => match chain.countered {
                Some((slot, true, _)) => player = Some(PlayerRef::ControllerOf(slot)),
                _ => {
                    self.block("DelayedTrigger:remembers the controller of something else");
                    return None;
                }
            },
            Some(other) => {
                // Remembered cards of a chain that doesn't keep them to itself (D-069:
                // they would outlive it), or of an effect object, are named as such.
                let why = match other {
                    "Remembered" | "RememberedLKI" if self.in_effect => " (in an effect)",
                    "Remembered" | "RememberedLKI" => " (not noted by this chain)",
                    "Targeted" | "ParentTarget" if chain.target_moved.is_some() => {
                        " (a target the chain moved)"
                    }
                    "Targeted" | "ParentTarget" => " (not the chain's one target)",
                    _ => "",
                };
                self.block(format!("DelayedTrigger:remembers {}{why}", shape(other)));
                return None;
            }
        }
        // The delayed ability: compiled as a trigger with that event, whose "that
        // player" / "that much" are what it remembers.
        let exec = rd.get("Execute").unwrap_or("");
        let saved = (
            self.in_trigger,
            self.optional_trigger,
            std::mem::take(&mut self.trig),
            self.memory,
        );
        self.in_trigger = true;
        self.optional_trigger = false;
        self.memory = false;
        // Its objects are "those cards" from the start (CR 603.7c).
        self.delayed_noted = objects.map(|(_, at)| at);
        let was_delayed = std::mem::replace(&mut self.in_delayed, objects.is_some());
        let mut trig = TrigCtx::of(&when);
        if player.is_some() {
            trig.players = &["DelayTriggerRemembered"];
            trig.player_is_you = false;
        }
        if amount.is_some() {
            trig.amount = Some("Count$TriggerRememberAmount");
        }
        self.trig = trig;
        let body = match self.sub(exec) {
            Some(r) => self.body(r, true, "DB"),
            None => {
                self.block("DelayedTrigger:no Execute");
                None
            }
        };
        (
            self.in_trigger,
            self.optional_trigger,
            self.trig,
            self.memory,
        ) = saved;
        self.delayed_noted = None;
        self.in_delayed = was_delayed;
        let index = self.granted.len() as u16;
        self.granted.push(AbilityDef::Triggered {
            when,
            intervening_if: None,
            body: body?,
        });
        let delayed = Effect::DelayedTrigger {
            ability: index,
            window,
            player,
            amount,
            objects: objects.map(|(r, _)| r),
            one: objects.is_some() && one,
        };
        // Forge's CounterEffect remembers the mana value before it tries to counter
        // ("should remember even that spell cannot be countered"): the delayed trigger is
        // made whether or not the spell was countered, with its mana value as it last was
        // on the stack (`OfTarget`, X included). A target illegal on resolution makes the
        // whole spell do nothing (CR 608.2b).
        Some(delayed)
    }

    fn body(&mut self, rec: &'a Record, triggered: bool, root: &str) -> Option<Body> {
        if rec.head().map(|h| h.1) != Some("Charm") {
            return self.mode(rec, triggered, root).map(Body::Effect);
        }
        if rec.head().map(|h| h.0) != Some(root) {
            self.block(format!("record:Charm not {root}$"));
            return None;
        }
        let rd = Rd::new(rec, "Charm");
        rd.get("Cost");
        let names: Vec<&str> = rd
            .get("Choices")
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .collect();
        let max = rd.get("CharmNum").unwrap_or("1").parse::<u8>().ok();
        let min = match rd.get("MinCharmNum") {
            None => max,
            Some(n) => n.parse::<u8>().ok(),
        };
        rd.finish(&mut self.blockers);
        let (Some(min), Some(max)) = (min, max) else {
            self.block("Charm:variable-count");
            return None;
        };
        let mut modes = Vec::new();
        for n in names {
            let rec = self.sub(n)?;
            modes.push(self.mode(rec, triggered, "DB")?);
        }
        Some(Body::Modal { min, max, modes })
    }

    // ------------------------------------------------------------------ conditions, costs

    /// "As long as …" on a static ability: `IsPresent$` (also "no …": `PresentCompare$
    /// EQ0`, `LE<n>`), or one of Forge's named conditions.
    fn static_condition(&mut self, rd: &Rd<'a>) -> Option<Option<Condition>> {
        let named = match rd.get("Condition") {
            None => None,
            Some("PlayerTurn") => Some(Condition::YourTurn),
            Some("NotPlayerTurn") => Some(Condition::NotYourTurn),
            // CR 702 ability words, by their definitions.
            Some("Threshold") => Some(Condition::CardsInGraveyard {
                cards: Selector::default(),
                at_least: 7,
            }),
            Some("Metalcraft") => Some(Condition::Control {
                what: Selector {
                    types: vec![CardType::Artifact],
                    controller: Who::You,
                    ..Selector::default()
                },
                at_least: 3,
            }),
            Some("Hellbent") => Some(Condition::HandAtMost(0)),
            Some(other) => {
                self.block(format!("condition:{other}"));
                return None;
            }
        };
        // "N or fewer": only for permanents on the battlefield.
        let at_most = match (rd.rec.get("IsPresent"), rd.rec.get("PresentCompare")) {
            (Some(_), Some("EQ0")) => Some(0),
            (Some(_), Some(c)) => c.strip_prefix("LE").and_then(|n| n.parse::<u32>().ok()),
            _ => None,
        };
        let present = match at_most {
            Some(at_most) if rd.rec.get("PresentZone").is_none() => {
                rd.get("PresentCompare");
                let what = rd.get("IsPresent").and_then(|f| self.selector(f))?;
                Some(Condition::ControlAtMost { what, at_most })
            }
            _ => self.condition(rd)?,
        };
        // A static ability's condition is asked inside the layer walk.
        if let Some(Condition::Compare { amount, .. }) = &present
            && !amount.countable_in_layers()
        {
            self.block("condition:not countable by a static ability");
            return None;
        }
        match (named, present) {
            (Some(_), Some(_)) => {
                self.block("condition:two");
                None
            }
            (a, b) => Some(a.or(b)),
        }
    }

    /// `GE3`, `EQ0`, `LT2`… against a number (Forge's default is `GE1`).
    fn compare(&mut self, text: &str) -> Option<(Cmp, u32)> {
        let parsed = text.split_at_checked(2).and_then(|(op, n)| {
            let n = n.parse::<u32>().ok()?;
            match op {
                "GE" => Some((Cmp::AtLeast, n)),
                "GT" => Some((Cmp::AtLeast, n + 1)),
                "LE" => Some((Cmp::AtMost, n)),
                "LT" => Some((Cmp::AtMost, n.checked_sub(1)?)),
                "EQ" => Some((Cmp::Exactly, n)),
                "NE" => Some((Cmp::Not, n)),
                _ => None,
            }
        });
        if parsed.is_none() {
            self.block("condition:compare");
        }
        parsed
    }

    /// When an ability works at all: `IsPresent$ <filter>` (+ `PresentCompare$`,
    /// `PresentZone$ Graveyard`) — "if you control …" — or `CheckSVar$ <amount>`
    /// (+ `SVarCompare$`) — "if [something counted] is N or more".
    fn condition(&mut self, rd: &Rd<'a>) -> Option<Option<Condition>> {
        let present = match rd.get("IsPresent") {
            None => None,
            Some(filter) => Some(self.present(rd, filter)?),
        };
        let checked = match rd.get("CheckSVar") {
            None => None,
            Some(name) => {
                let cmp = self.compare(rd.get("SVarCompare").unwrap_or("GE1"));
                let amount = match name.parse::<u32>() {
                    Ok(n) => Some(Amount::Fixed(n)),
                    Err(_) => self.expr(name),
                };
                let ((cmp, n), amount) = (cmp?, amount?);
                // It is asked before anything resolves: nothing about a spell, a source
                // or a triggering object can be in it.
                if !amount.stands_alone() {
                    self.block("condition:amount needs the ability");
                    return None;
                }
                Some(Condition::Compare { amount, cmp, n })
            }
        };
        match (present, checked) {
            (Some(_), Some(_)) => {
                self.block("condition:two");
                None
            }
            (a, b) => Some(a.or(b)),
        }
    }

    fn present(&mut self, rd: &Rd<'a>, filter: &'a str) -> Option<Condition> {
        // "If you control a commander" / "… your commander" (CR 903.3d).
        if rd.rec.get("PresentCompare").is_none() && rd.rec.get("PresentZone").is_none() {
            match filter {
                "Card.IsCommander+YouCtrl" => {
                    return Some(Condition::ControlCommander { own: false });
                }
                "Card.IsCommander+YouOwn+YouCtrl" | "Card.IsCommander+YouCtrl+YouOwn" => {
                    return Some(Condition::ControlCommander { own: true });
                }
                _ => {}
            }
        }
        let cmp = self.compare(rd.get("PresentCompare").unwrap_or("GE1"));
        let zone = rd.get("PresentZone");
        let sel = match zone {
            None => self.selector(filter),
            Some(_) => self.card_selector(filter),
        };
        let ((cmp, n), mut sel) = (cmp?, sel?);
        match (zone, cmp) {
            (None, Cmp::AtLeast) => Some(Condition::Control {
                what: sel,
                at_least: n,
            }),
            (None, Cmp::AtMost) => Some(Condition::ControlAtMost {
                what: sel,
                at_most: n,
            }),
            (None, Cmp::Exactly) if n == 0 => Some(Condition::ControlAtMost {
                what: sel,
                at_most: 0,
            }),
            (None, _) => Some(Condition::Compare {
                amount: Amount::Count(sel),
                cmp,
                n,
            }),
            (Some("Graveyard"), _) if sel.controller == Who::You => match cmp {
                Cmp::AtLeast => {
                    sel.controller = Who::Any;
                    Some(Condition::CardsInGraveyard {
                        cards: sel,
                        at_least: n,
                    })
                }
                _ => Some(Condition::Compare {
                    amount: Amount::CardsIn {
                        zone: CardZone::Graveyard,
                        cards: vec![sel],
                    },
                    cmp,
                    n,
                }),
            },
            (Some(z), _) => {
                self.block(format!("condition:zone {z}"));
                None
            }
        }
    }

    /// An amount that is counted without a spell or ability resolving (a replacement
    /// effect's "for each …"): a number, or a count of permanents.
    /// `own_x`: the X of the permanent spell itself may be used ("enters with X counters").
    fn counted(&mut self, text: &str, own_x: bool) -> Option<Amount> {
        if let Ok(n) = text.parse::<u32>() {
            return Some(Amount::Fixed(n));
        }
        match self.expr(text)? {
            a if a.stands_alone() => Some(a),
            Amount::X if own_x => Some(Amount::X),
            _ => {
                self.block("expr:not countable by a replacement");
                None
            }
        }
    }

    fn enters_with(kind: CounterKind, amount: Amount) -> ReplaceWith {
        match amount {
            Amount::Fixed(n) => ReplaceWith::EntersWithCounters { kind, n },
            amount => ReplaceWith::EntersWithCounted { kind, amount },
        }
    }

    fn counter_kind(&mut self, text: Option<&str>) -> Option<CounterKind> {
        let name = match text {
            Some("P1P1") => return Some(CounterKind::PlusOne),
            Some("M1M1") => return Some(CounterKind::MinusOne),
            Some(name) => name,
            None => {
                self.block("counter:?");
                return None;
            }
        };
        // Kinds with rules of their own (CR 122.1a–j) aren't built (D-084).
        let pt = |n: &str| {
            let mut c = n.chars();
            matches!(c.next(), Some('P' | 'M'))
                && n[1..]
                    .split(['P', 'M'])
                    .all(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
                && n[1..].contains(['P', 'M'])
        };
        let why = if pt(name) {
            Some("+X/+Y")
        } else if keyword(name).is_some()
            || matches!(
                name,
                "First Strike" | "Double Strike" | "Exalted" | "Decayed" | "Indestructible"
            )
        {
            Some("keyword counter")
        } else {
            match name {
                "SHIELD" | "STUN" | "LOYALTY" | "DEFENSE" | "FINALITY" | "HONE" => {
                    Some("rules of its own")
                }
                "POISON" | "ENERGY" | "EXPERIENCE" | "Experience" | "RAD" | "TICKET" => {
                    Some("on players")
                }
                _ => None,
            }
        };
        if let Some(why) = why {
            self.block(format!("counter:{name} ({why})"));
            return None;
        }
        // Forge's names are upper case words ("CHARGE", "LORE").
        if !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            self.block(format!("counter:{name}"));
            return None;
        }
        match card_model::CounterName::new(&name.replace('_', " ")) {
            Some(n) => Some(CounterKind::Named(n)),
            None => {
                self.block(format!("counter:{name}"));
                None
            }
        }
    }

    /// `K:etbCounter:<type>:<amount>[:<condition>[:<description>]]` — "this permanent
    /// enters with N counters on it".
    fn etb_counter(&mut self, k: &str) -> Option<AbilityDef> {
        let parts: Vec<&str> = k.split(':').collect();
        let kind = self.counter_kind(parts.get(1).copied());
        if !matches!(parts.get(3), None | Some(&"no Condition")) {
            self.block("kw:etbCounter-condition");
            return None;
        }
        let amount = self.counted(parts.get(2).copied().unwrap_or("1"), true);
        Some(AbilityDef::Replacement {
            replaces: Replaces::ThisEnters,
            with: Self::enters_with(kind?, amount?),
        })
    }

    /// `K:ETBReplacement:Other:<SVar>:Mandatory:Battlefield:<ValidCard>` whose SVar puts
    /// counters on the entering permanent — "each other [permanent] enters with …". (The
    /// other uses of this keyword — "as this enters, choose …", "enters as a copy" — are
    /// not compiled.)
    fn etb_replacement(&mut self, k: &str) -> Option<AbilityDef> {
        let parts: Vec<&str> = k.splitn(6, ':').collect();
        // "As this enters, choose a creature type / a color" (CR 614.1c, 614.12a).
        if let [_, "Other", svar] = parts.as_slice() {
            return self.etb_choice(svar);
        }
        // "You may have this enter as a copy of …" (CR 707.5, 614.1c).
        if parts.get(1) == Some(&"Copy") {
            return self.etb_copy(&parts);
        }
        let [_, "Other", svar, "Mandatory", "Battlefield", valid] = parts.as_slice() else {
            self.block("kw:ETBReplacement");
            return None;
        };
        let Some(rec) = self.records.iter().find(|(n, _)| n == svar).map(|(_, r)| r) else {
            self.block("kw:ETBReplacement");
            return None;
        };
        if rec.head() != Some(("DB", "PutCounter")) || rec.get("Defined") != Some("ReplacedCard") {
            self.block("kw:ETBReplacement");
            return None;
        }
        let rec = self.sub(svar)?;
        let rd = Rd::new(rec, "ETBReplacement PutCounter");
        rd.get("Defined");
        if rd.get("ETB") != Some("True") {
            self.block("kw:ETBReplacement");
            return None;
        }
        let kind = self.counter_kind(rd.get("CounterType"));
        let amount = self.counted(rd.get("CounterNum").unwrap_or("1"), false);
        rd.finish(&mut self.blockers);
        let sel = self.selector(valid);
        Some(AbilityDef::Replacement {
            replaces: Replaces::OtherEnters(sel?),
            with: Self::enters_with(kind?, amount?),
        })
    }

    /// `K:ETBReplacement:Other:<SVar>` whose SVar is `DB$ ChooseType | Type$ Creature` or
    /// `DB$ ChooseColor`, chosen by the permanent's controller: "As this enters, choose a
    /// creature type / a color." Any other effect there is named by its API.
    fn etb_choice(&mut self, svar: &str) -> Option<AbilityDef> {
        let Some(rec) = self.records.iter().find(|(n, _)| n == svar).map(|(_, r)| r) else {
            self.block("kw:ETBReplacement");
            return None;
        };
        let api = match rec.head() {
            Some(("DB", api @ ("ChooseType" | "ChooseColor"))) => api,
            Some((_, api)) => {
                self.block(format!("kw:ETBReplacement:{api}"));
                return None;
            }
            None => {
                self.block("kw:ETBReplacement");
                return None;
            }
        };
        let rec = self.sub(svar)?;
        let rd = Rd::new(rec, format!("ETBReplacement {api}"));
        // Applied to the entering card (as every choice here is).
        if !matches!(rd.get("ETB"), None | Some("True")) {
            self.block(format!("param:ETBReplacement {api}.ETB"));
        }
        let what = self.choice(&rd, api);
        if rd.get("SubAbility").is_some() {
            self.block(format!("kw:ETBReplacement:{api}+SubAbility"));
            return None;
        }
        rd.finish(&mut self.blockers);
        Some(AbilityDef::Replacement {
            replaces: Replaces::ThisEnters,
            with: ReplaceWith::Choose(what?),
        })
    }

    /// What a `ChooseType` / `ChooseColor` record has its controller choose: a creature
    /// type (`Type$ Creature`: any of CR 205.3m) or one of the five colors (CR 105.4). Other
    /// families of types, restricted lists, other choosers and several colors are named.
    fn choice(&mut self, rd: &Rd<'a>, api: &str) -> Option<ChoiceOf> {
        match rd.get("Defined") {
            None | Some("You") => {}
            Some(other) => {
                self.block(format!("{api}:chooser {}", shape(other)));
                return None;
            }
        }
        match api {
            "ChooseType" => match rd.get("Type") {
                Some("Creature") => Some(ChoiceOf::CreatureType),
                other => {
                    self.block(format!("ChooseType:{}", other.unwrap_or("?")));
                    None
                }
            },
            "ChooseColor" => Some(ChoiceOf::Color),
            _ => unreachable!("not a choice: {api}"),
        }
    }

    /// `K:ETBReplacement:Copy:<SVar>:Optional` with `DB$ Clone | Choices$ <filter>`: "you
    /// may have this enter [tapped] as a copy of [a permanent][, except …]" (CR 707.5,
    /// 614.1c).
    fn etb_copy(&mut self, parts: &[&str]) -> Option<AbilityDef> {
        let [_, "Copy", svar, "Optional"] = parts else {
            self.block("kw:ETBReplacement:Copy not optional");
            return None;
        };
        let rec = self.sub(svar)?;
        if rec.head() != Some(("DB", "Clone")) {
            self.block("kw:ETBReplacement:Copy not Clone");
            return None;
        }
        // Spark Double's "it enters with an additional counter …" is an effect made on
        // the side (CR 707.9e), not compiled.
        if rec.get("SubAbility").is_some() {
            self.block("Clone:SubAbility");
            return None;
        }
        let rd = Rd::new(rec, "Clone");
        let Some(choices) = rd.get("Choices") else {
            self.block("Clone:enters as a copy without Choices");
            return None;
        };
        let of = self.selector(choices);
        let tapped = rd.get("IntoPlayTapped") == Some("True");
        let except = self.copy_except(&rd);
        rd.finish(&mut self.blockers);
        Some(AbilityDef::Replacement {
            replaces: Replaces::ThisEnters,
            with: ReplaceWith::EntersAsCopy {
                of: of?,
                except: except?,
                tapped,
            },
        })
    }

    /// The exceptions of a copy effect (CR 707.9): `NonLegendary$`, `AddKeywords$`,
    /// `AddTypes$`, `SetPower$`/`SetToughness$`, `SetColor$`, `SetCreatureTypes$`. Any other
    /// modifier is left unread (and so blocks the card).
    fn copy_except(&mut self, rd: &Rd<'a>) -> Option<Vec<CopyExcept>> {
        let mut out = Vec::new();
        let mut ok = true;
        if rd.get("NonLegendary") == Some("True") {
            out.push(CopyExcept::NotLegendary);
        }
        if let Some(ks) = rd.get("AddKeywords") {
            let mut keywords = Vec::new();
            for k in ks.split('&').map(str::trim) {
                match keyword(k) {
                    Some(k) => keywords.push(k),
                    None => {
                        self.block(format!("copy:keyword {k}"));
                        ok = false;
                    }
                }
            }
            out.push(CopyExcept::AddKeywords(keywords));
        }
        if let Some(ts) = rd.get("AddTypes") {
            let (mut card_types, mut subtypes) = (Vec::new(), Vec::new());
            for t in ts.split('&').map(str::trim) {
                match card_type(t) {
                    Some(t) => card_types.push(t),
                    None if self.known.subtypes.contains(t) => subtypes.push(t.to_string()),
                    None => {
                        self.block(format!("copy:AddTypes {}", shape(t)));
                        ok = false;
                    }
                }
            }
            out.push(CopyExcept::AddTypes {
                card_types,
                subtypes,
            });
        }
        match (rd.get("SetPower"), rd.get("SetToughness")) {
            (None, None) => {}
            (Some(p), Some(t)) => match (p.parse::<i32>(), t.parse::<i32>()) {
                (Ok(power), Ok(toughness)) => out.push(CopyExcept::SetPT { power, toughness }),
                _ => {
                    self.block("copy:SetPower variable");
                    ok = false;
                }
            },
            _ => {
                self.block("copy:SetPower without SetToughness");
                ok = false;
            }
        }
        if let Some(cs) = rd.get("SetColor") {
            let mut colors = Vec::new();
            for c in cs.split(',').map(str::trim) {
                match color(c) {
                    Some(c) => colors.push(c),
                    None if c == "Colorless" => {}
                    None => {
                        self.block(format!("copy:SetColor {c}"));
                        ok = false;
                    }
                }
            }
            out.push(CopyExcept::SetColors(colors));
        }
        if let Some(ts) = rd.get("SetCreatureTypes") {
            let mut types = Vec::new();
            for t in ts.split_whitespace() {
                if self.known.subtypes.contains(t) {
                    types.push(t.to_string());
                } else {
                    self.block(format!("copy:SetCreatureTypes {}", shape(t)));
                    ok = false;
                }
            }
            out.push(CopyExcept::SetCreatureTypes(types));
        }
        ok.then_some(out)
    }

    /// `RestrictValid$` of a mana ability: `Spell.<card filter>` and
    /// `Activated.<permanent filter>+inZoneBattlefield`, separated by commas.
    fn mana_restriction(&mut self, text: &str) -> Option<ManaRestriction> {
        let mut out = ManaRestriction::default();
        for part in text.split(',').map(str::trim) {
            let (kind, props) = part.split_once('.').unwrap_or((part, ""));
            let mut props: Vec<&str> = props.split('+').filter(|p| !p.is_empty()).collect();
            let on_battlefield = props.contains(&"inZoneBattlefield");
            props.retain(|p| *p != "inZoneBattlefield");
            // The filter is about the spell's card / the ability's source. A card type
            // first is the head of our filters ("Creature+Dragon" = "Creature.Dragon").
            let filter = match props.split_first() {
                None => "Card".to_string(),
                Some((first, rest)) if card_type(first).is_some() => {
                    format!("{first}.{}", rest.join("+"))
                }
                Some(_) => format!("Card.{}", props.join("+")),
            };
            match (kind, on_battlefield) {
                ("Spell", false) => out.spells.push(self.selector(&filter)?),
                ("Activated", true) => out.abilities.push(self.selector(&filter)?),
                // Without `inZoneBattlefield`: an ability of such a source anywhere (a
                // card's cycling from the hand too).
                ("Activated", false) => out.sources.push(self.selector(&filter)?),
                _ => {
                    self.block(format!("Mana:RestrictValid {kind}"));
                    return None;
                }
            }
        }
        Some(out)
    }

    /// `AB$ Mana` / `AB$ ManaReflected`: a mana ability (CR 605.1a; D-049, D-052, D-066).
    fn mana_ability(&mut self, rec: &'a Record, api: &str) -> Option<AbilityDef> {
        let rd = Rd::new(rec, api);
        let cost = rd.get("Cost").and_then(|c| self.cost_of(c, true));
        let letter = |w: &&str| match *w {
            "W" => Some(ManaType::White),
            "U" => Some(ManaType::Blue),
            "B" => Some(ManaType::Black),
            "R" => Some(ManaType::Red),
            "G" => Some(ManaType::Green),
            "C" => Some(ManaType::Colorless),
            _ => None,
        };
        let colors =
            || -> Vec<ManaType> { Color::ALL.into_iter().map(ManaType::from_color).collect() };
        let produce = if api == "ManaReflected" {
            // "Add one mana of any color/type that a [Valid] could produce" (CR 106.7).
            let types = match rd.get("ColorOrType") {
                Some("Color") => Some(false),
                Some("Type") => Some(true),
                other => {
                    self.block(format!(
                        "ManaReflected:ColorOrType {}",
                        other.unwrap_or("?")
                    ));
                    None
                }
            };
            let property = rd.get("ReflectProperty");
            if property != Some("Produce") {
                self.block(format!("ManaReflected:{}", property.unwrap_or("?")));
            }
            let from = match rd.get("Valid") {
                Some(v) if !v.starts_with("Defined.") => self.selector(v),
                other => {
                    self.block(format!("ManaReflected:Valid {}", other.unwrap_or("?")));
                    None
                }
            };
            if rd.get("Amount").is_some_and(|a| a != "1") {
                self.block("ManaReflected:Amount");
                return None;
            }
            match (types, from, property) {
                (Some(types), Some(from), Some("Produce")) => {
                    Some(ManaProduce::Reflected { from, types })
                }
                _ => return None,
            }
        } else {
            // `Amount$`: how many times the whole of `Produced$` is made — a number, or a
            // variable counted as the ability resolves.
            let (times, counted) = match rd.get("Amount") {
                None => (Some(1), None),
                Some(n) => match n.parse::<u32>() {
                    Ok(n) => (Some(n).filter(|n| *n > 0), None),
                    Err(_) => match self.expr(n) {
                        // "For each [permanent you control]" (D-066).
                        Some(Amount::Count(sel)) if sel.tapped.is_none() => {
                            (None, Some(Amount::Count(sel)))
                        }
                        Some(_) => {
                            self.block("Mana:counted");
                            return None;
                        }
                        None => return None,
                    },
                },
            };
            let words: Vec<&str> = rd
                .get("Produced")
                .unwrap_or("")
                .split_whitespace()
                .collect();
            match (times, counted) {
                (_, Some(amount)) => match words.as_slice() {
                    // "Add X mana of any one color", "Add {B} for each Swamp you control".
                    ["Any"] => Some(ManaProduce::Counted {
                        of: colors(),
                        amount,
                    }),
                    [one] => letter(one).map(|t| ManaProduce::Counted {
                        of: vec![t],
                        amount,
                    }),
                    _ => None,
                },
                (Some(n), None) => match words.as_slice() {
                    // One colour of the player's choice, `n` mana of it.
                    ["Any"] if n == 1 => Some(ManaProduce::AnyColor),
                    ["Any"] => Some(ManaProduce::Times {
                        of: colors(),
                        count: n,
                    }),
                    // "Add {W} or {U}"; with `Amount$ n`, `n` mana each of any listed
                    // colour ("in any combination").
                    ["Combo", "Any"] if n == 1 => Some(ManaProduce::AnyColor),
                    ["Combo", "Any"] => Some(ManaProduce::Combination {
                        of: colors(),
                        count: n,
                    }),
                    ["Combo", "ColorIdentity"] if n == 1 => Some(ManaProduce::CommanderIdentity),
                    ["Combo", of @ ..] if !of.is_empty() => {
                        match of.iter().map(letter).collect::<Option<Vec<ManaType>>>() {
                            None => None,
                            Some(mut of) => {
                                of.dedup();
                                match (of.as_slice(), n) {
                                    ([one], 1) => Some(ManaProduce::One(*one)),
                                    ([one], n) => Some(ManaProduce::Times {
                                        of: vec![*one],
                                        count: n,
                                    }),
                                    (_, 1) => Some(ManaProduce::OneOf(of)),
                                    (_, n) => Some(ManaProduce::Combination { of, count: n }),
                                }
                            }
                        }
                    }
                    // Fixed mana: "C C", "G" with `Amount$ 3`, "U B", "C C R".
                    [_, ..] => match words.iter().map(letter).collect::<Option<Vec<ManaType>>>() {
                        None => None,
                        Some(all) => {
                            let all: Vec<ManaType> =
                                (0..n).flat_map(|_| all.iter().copied()).collect();
                            match all.as_slice() {
                                [one] => Some(ManaProduce::One(*one)),
                                [first, ..] if all.iter().all(|t| t == first) => {
                                    Some(ManaProduce::Times {
                                        of: vec![*first],
                                        count: all.len() as u32,
                                    })
                                }
                                _ => Some(ManaProduce::Fixed(all)),
                            }
                        }
                    },
                    [] => None,
                },
                (None, None) => None,
            }
        };
        // "Activate only if you control …".
        let only_if = self.condition(&rd);
        // "Spend this mana only to cast … spells / activate abilities of …".
        let spend = match rd.get("RestrictValid") {
            None => Some(None),
            Some(text) => self.mana_restriction(text).map(Some),
        };
        // What else it does (CR 605.3b): "This land deals 1 damage to you."
        let then = match rd.get("SubAbility") {
            None => Some(None),
            Some(name) => self.mana_side_effect(name).map(Some),
        };
        rd.finish(&mut self.blockers);
        let Some(produce) = produce else {
            self.block("Mana:produced");
            return None;
        };
        let (cost, only_if, spend, then) = (cost?, only_if?, spend?, then?);
        // D-066: `{T}` or a sacrifice, maybe mana and life; one permanent sacrificed.
        let uses_up = cost.tap || cost.sacrifice_self || cost.sacrifice.is_some();
        let other = cost.discard.is_some() || cost.discard_self || cost.exile_self;
        if !uses_up || other || cost.mana.as_ref().is_some_and(|m| m.has_x()) {
            self.block("Mana:cost");
            return None;
        }
        if cost.sacrifice.as_ref().is_some_and(|ch| ch.count != 1) {
            self.block("Mana:cost sacrifice N");
            return None;
        }
        if matches!(produce, ManaProduce::Counted { .. })
            && (cost.sacrifice_self || cost.sacrifice.is_some())
        {
            self.block("Mana:counted with sacrifice");
            return None;
        }
        Some(AbilityDef::Mana {
            cost,
            produce,
            only_if,
            spend,
            then,
        })
    }

    /// The `SubAbility$` of a mana ability: what it also does as it resolves. CR 605.1a /
    /// 605.5a: with a target, or moving a card to or from a library, it would not be a
    /// mana ability; only damage to, life loss and life gain of its controller are
    /// modelled (D-066).
    fn mana_side_effect(&mut self, name: &str) -> Option<Effect> {
        let rec = self.sub(name)?;
        let api = rec.head().map(|h| h.1).unwrap_or("?");
        if !matches!(api, "DealDamage" | "LoseLife" | "GainLife") {
            self.block(format!("Mana:sub {api}"));
            return None;
        }
        let mode = self.mode(rec, false, "DB")?;
        if !mode.targets.is_empty() {
            self.block("Mana:sub with a target");
            return None;
        }
        if !crate::mana_side_effect(&mode.effect) {
            self.block(format!("Mana:sub {api} form"));
            return None;
        }
        Some(mode.effect)
    }

    fn cost(&mut self, text: &str) -> Option<Cost> {
        self.cost_of(text, false)
    }

    /// A cost; `hybrid`: two-color hybrid mana (`RW` = `{R/W}`) is understood (D-066: only
    /// mana abilities can pay it so far).
    fn cost_of(&mut self, text: &str, hybrid: bool) -> Option<Cost> {
        let mut cost = Cost::default();
        let mut mana = String::new();
        for tok in cost_tokens(text) {
            let kind = tok.split('<').next().unwrap_or(tok);
            // `Name<Amount/Type/Description>`: the description is text only.
            let args: Vec<&str> = tok
                .strip_prefix(kind)
                .and_then(|r| r.strip_prefix('<'))
                .and_then(|r| r.strip_suffix('>'))
                .map(|r| r.splitn(3, '/').collect())
                .unwrap_or_default();
            let count = args.first().and_then(|n| n.parse::<u32>().ok());
            match (kind, tok.contains('<')) {
                ("T", false) => cost.tap = true,
                ("Sac", true) if args.get(1) == Some(&"CARDNAME") && count == Some(1) => {
                    cost.sacrifice_self = true
                }
                // "Discard this card" (from the hand), "exile this card from your
                // graveyard": costs of abilities that work from those zones.
                ("Discard", true)
                    if matches!(args.get(1), Some(&"CARDNAME" | &"NICKNAME"))
                        && count == Some(1) =>
                {
                    cost.discard_self = true
                }
                ("ExileFromGrave", true)
                    if matches!(args.get(1), Some(&"CARDNAME" | &"NICKNAME"))
                        && count == Some(1) =>
                {
                    cost.exile_self = true
                }
                // "Remove N [kind] counters from this" / "put N [kind] counters on this"
                // (`SubCounter<N/TYPE>`, `AddCounter<N/TYPE>`; on this card only — a third
                // argument names something else).
                ("SubCounter" | "AddCounter", true)
                    if count.is_some_and(|n| n > 0)
                        && (args.len() == 2
                            || matches!(args.get(2), Some(&"CARDNAME" | &"NICKNAME"))) =>
                {
                    let n = count.expect("checked");
                    let k = self.counter_kind(args.get(1).copied())?;
                    let slot = if kind == "SubCounter" {
                        &mut cost.remove_counters
                    } else {
                        &mut cost.add_counters
                    };
                    if slot.is_some() {
                        self.block(format!("cost:{kind} twice"));
                        return None;
                    }
                    *slot = Some((k, n));
                }
                ("Sac" | "Discard", true) => {
                    let (Some(count), Some(types)) = (count.filter(|n| *n > 0), args.get(1)) else {
                        self.block(format!("cost:{kind}-amount"));
                        return None;
                    };
                    // Alternatives are separated by `;` here ("an artifact or creature").
                    let mut what = Vec::new();
                    for alt in types.split(';') {
                        match self.alt(alt.trim())? {
                            Alt::Sel(s) => what.push(s),
                            Alt::This => {
                                self.block(format!("cost:{kind}-this"));
                                return None;
                            }
                        }
                    }
                    let slot = if kind == "Sac" {
                        &mut cost.sacrifice
                    } else {
                        // A card in a hand has types and subtypes; nothing else is asked.
                        let plain = what.iter().all(|s| {
                            *s == Selector {
                                types: s.types.clone(),
                                not_types: s.not_types.clone(),
                                subtypes: s.subtypes.clone(),
                                not_subtypes: s.not_subtypes.clone(),
                                ..Selector::default()
                            }
                        });
                        if !plain {
                            self.block("cost:Discard-filter");
                            return None;
                        }
                        &mut cost.discard
                    };
                    if slot.is_some() {
                        self.block(format!("cost:{kind}-twice"));
                        return None;
                    }
                    *slot = Some(CostChoice { count, what });
                }
                // "Exile N [cards] from your hand" (never the card itself: a spell is on the
                // stack as its cost is paid, and `Other` is required of an ability's cost
                // where it matters), "return N [permanents] you control to their owner's
                // hand" (CR 601.2h).
                ("ExileFromHand" | "Return", true) => {
                    let (Some(count), Some(types)) = (count.filter(|n| *n > 0), args.get(1)) else {
                        self.block(format!("cost:{kind}-amount"));
                        return None;
                    };
                    let mut what = Vec::new();
                    for alt in types.split(';') {
                        match self.alt(alt.trim())? {
                            Alt::Sel(s) => what.push(s),
                            Alt::This => {
                                self.block(format!("cost:{kind}-this"));
                                return None;
                            }
                        }
                    }
                    let slot = if kind == "Return" {
                        // Permanents they control: whose they are is the cost's own rule.
                        if what
                            .iter()
                            .any(|s| s.controller != Who::Any || s.owner != Who::Any)
                        {
                            self.block("cost:Return-whose");
                            return None;
                        }
                        &mut cost.return_to_hand
                    } else {
                        // A card in a hand: its types, subtypes and colors.
                        let plain = what.iter().all(|s| {
                            *s == Selector {
                                types: s.types.clone(),
                                not_types: s.not_types.clone(),
                                subtypes: s.subtypes.clone(),
                                not_subtypes: s.not_subtypes.clone(),
                                colors: s.colors.clone(),
                                not_colors: s.not_colors.clone(),
                                other: s.other,
                                ..Selector::default()
                            }
                        });
                        if !plain {
                            self.block("cost:ExileFromHand-filter");
                            return None;
                        }
                        &mut cost.exile_from_hand
                    };
                    if slot.is_some() {
                        self.block(format!("cost:{kind}-twice"));
                        return None;
                    }
                    *slot = Some(CostChoice { count, what });
                }
                ("PayLife", true) => match count {
                    Some(n) if cost.life == 0 && n > 0 => cost.life = n,
                    // "As an additional cost to cast this spell, pay X life" (CR 107.3a):
                    // X is announced, when the script says X is what was paid.
                    None if args.first() == Some(&"X")
                        && self.spell_cost
                        && !cost.life_x
                        && self.face.svars.get("X").is_some_and(|v| v == "Count$xPaid") =>
                    {
                        cost.life_x = true
                    }
                    _ => {
                        self.block("cost:PayLife-amount");
                        return None;
                    }
                },
                (_, false)
                    if tok.parse::<u32>().is_ok()
                        || matches!(tok, "W" | "U" | "B" | "R" | "G" | "C") =>
                {
                    mana.push_str(&format!("{{{tok}}}"))
                }
                // CR 107.4e: `RW` is `{R/W}`.
                (_, false)
                    if hybrid
                        && tok.len() == 2
                        && tok.chars().all(|c| "WUBRG".contains(c))
                        && tok.as_bytes()[0] != tok.as_bytes()[1] =>
                {
                    mana.push_str(&format!("{{{}/{}}}", &tok[..1], &tok[1..]))
                }
                _ => {
                    self.block(format!("cost:{kind}"));
                    return None;
                }
            }
        }
        if !mana.is_empty() {
            cost.mana = ManaCost::parse(&mana).ok();
        }
        Some(cost)
    }

    // ------------------------------------------------------------------ line kinds

    fn ability(&mut self, rec: &'a Record) -> Option<AbilityDef> {
        let (kind, api) = rec.head().or_else(|| {
            self.block("ability:no-head");
            None
        })?;
        match kind {
            "SP" => {
                // A spell's `Cost$` normally just repeats its mana cost; anything else is
                // an additional or alternative cost.
                if let Some(text) = rec.get("Cost") {
                    let printed = self.face.mana_cost.as_deref().unwrap_or("no cost");
                    let same = text.split_whitespace().eq(printed.split_whitespace());
                    if !same {
                        // The whole cost is restated: the card's mana cost plus what is
                        // paid in addition ("as an additional cost to cast this spell, …").
                        self.spell_cost = true;
                        let cost = self.cost(text);
                        self.spell_cost = false;
                        let mut cost = cost?;
                        // "Pay X life": X is the X paid, for the spell's text too.
                        if cost.life_x {
                            self.has_x_cost = true;
                        }
                        let mana = std::mem::take(&mut cost.mana);
                        let printed = mana_cost(printed).ok().flatten();
                        if mana != printed || cost.tap || cost.sacrifice_self {
                            self.block("spell:other-cost");
                            return None;
                        }
                        self.additional.push(AbilityDef::AdditionalCost(cost));
                    }
                }
                self.in_spell = true;
                let body = self.body(rec, false, "SP");
                self.in_spell = false;
                body.map(AbilityDef::Spell)
            }
            "AB" if api == "Mana" || api == "ManaReflected" => self.mana_ability(rec, api),
            "AB" => {
                let rd = Rd::new(rec, "activated");
                let cost = rd.get("Cost").and_then(|c| self.cost(c));
                let only_if = self.condition(&rd);
                let timing = match rd.get("SorcerySpeed") {
                    None => Timing::Instant,
                    Some("True") => Timing::Sorcery,
                    Some(_) => {
                        self.block("param:activated.SorcerySpeed");
                        return None;
                    }
                };
                // Where the card must be (CR 113.6); the battlefield unless stated.
                let from = match rd.get("ActivationZone") {
                    None | Some("Battlefield") => Some(ActivationZone::Battlefield),
                    Some("Hand") => Some(ActivationZone::Hand),
                    Some("Graveyard") => Some(ActivationZone::Graveyard),
                    Some(other) => {
                        self.block(format!("activated:zone {other}"));
                        None
                    }
                };
                self.from_zone = from.unwrap_or_default();
                // "This ability costs {1} less to activate for each …" (Forge's
                // CostAdjustment: `ReduceCost$` less generic mana; with `ReduceAmount$` it
                // would be other mana, not compiled). Counted as the cost is determined.
                let reduce = match (rec.get("ReduceCost"), rec.get("ReduceAmount")) {
                    (None, _) => Some(None),
                    (Some(_), Some(_)) => {
                        self.block("param:activated.ReduceAmount");
                        None
                    }
                    (Some(n), None) => match n.parse::<u32>() {
                        Ok(n) => Some(Some(Amount::Fixed(n))),
                        Err(_) => match self.expr(n) {
                            Some(a) if a.stands_alone() => Some(Some(a)),
                            Some(_) => {
                                self.block("param:activated.ReduceCost not countable");
                                None
                            }
                            None => None,
                        },
                    },
                };
                let body = self.body(rec, false, "AB");
                self.from_zone = ActivationZone::Battlefield;
                let (cost, from) = (cost?, from?);
                // The engine never lets a card be exiled for its own cost; from the hand a
                // script that doesn't say "another" would let it (D-072).
                if from == ActivationZone::Hand
                    && cost
                        .exile_from_hand
                        .as_ref()
                        .is_some_and(|c| c.what.iter().any(|s| !s.other))
                {
                    self.block("cost:ExileFromHand-maybe-itself");
                    return None;
                }
                if !zone_cost_fits(&cost, from) {
                    self.block("activated:cost does not fit the zone");
                    return None;
                }
                Some(AbilityDef::Activated {
                    cost,
                    timing,
                    only_if: only_if?,
                    body: body?,
                    from,
                    cycling: false,
                    reduce: reduce?,
                })
            }
            _ => {
                self.block(format!("ability:{kind}"));
                None
            }
        }
    }

    fn trigger_event(&mut self, rd: &Rd<'a>) -> Option<Trigger> {
        match rd.rec.get("Mode")? {
            "ChangesZone" => {
                let origin = rd.get("Origin").unwrap_or("Any");
                let dest = rd.get("Destination");
                let filter = rd.get("ValidCard").and_then(|f| self.obj_filter(f));
                match (origin, dest) {
                    ("Any", Some("Battlefield")) => filter.map(Trigger::Enters),
                    ("Battlefield", Some("Graveyard")) => filter.map(Trigger::Dies),
                    // "Leaves the battlefield": for anywhere (CR 603.6c).
                    ("Battlefield", None | Some("Any")) => filter.map(Trigger::Leaves),
                    (o, d) => {
                        self.block(format!("trig:ChangesZone {o}->{}", d.unwrap_or("?")));
                        None
                    }
                }
            }
            "Phase" => {
                let step = match rd.get("Phase").and_then(phase_step) {
                    Some(s) => Some(s),
                    None => {
                        self.block(format!("trig:Phase {}", rd.rec.get("Phase").unwrap_or("?")));
                        None
                    }
                };
                let whose = match rd.get("ValidPlayer") {
                    None | Some("Player") => Some(Who::Any),
                    Some("You") => Some(Who::You),
                    Some("Opponent" | "Player.Opponent") => Some(Who::Opponent),
                    Some(other) => {
                        self.block(format!("trig:Phase player {other}"));
                        None
                    }
                };
                Some(Trigger::BeginningOf {
                    step: step?,
                    whose: whose?,
                })
            }
            "SpellCast" => {
                let by = match rd.get("ValidActivatingPlayer") {
                    None => Some(Who::Any),
                    Some("You") => Some(Who::You),
                    Some("Opponent" | "Player.Opponent") => Some(Who::Opponent),
                    Some(other) => {
                        self.block(format!("trig:SpellCast player {other}"));
                        None
                    }
                };
                // Card types alone ("Creature", "Instant,Sorcery"), any "Card", or what the
                // spell has to be ("Card.nonCreature", "Spirit", "Card.MultiColor").
                let mut types = Vec::new();
                let mut spells = Vec::new();
                let mut any = false;
                for word in rd.get("ValidCard").unwrap_or("Card").split(',') {
                    match (word.trim(), card_type(word.trim())) {
                        ("Card", _) => any = true,
                        (_, Some(t)) => types.push(t),
                        (w, None) => {
                            let saved = self.blockers.clone();
                            match self.alt(w) {
                                // Whose spell it is, is `ValidActivatingPlayer$`.
                                Some(Alt::Sel(s)) if s.controller == Who::Any && !s.other => {
                                    spells.push(s)
                                }
                                _ => {
                                    self.blockers = saved;
                                    self.block(format!("trig:SpellCast card {}", shape(w)));
                                    return None;
                                }
                            }
                        }
                    }
                }
                if any {
                    // "Card" among the alternatives: any spell.
                    types.clear();
                    spells.clear();
                }
                // "… their first noncreature spell each turn" (Forge's TriggerSpellCast:
                // the spells matching `ValidCard$` the caster cast this turn, this one
                // included, compared; only `EQ<n>` is compiled).
                let nth = match rd.get("ActivatorThisTurnCast") {
                    None => Some(None),
                    Some(c) => match c.strip_prefix("EQ").and_then(|n| n.parse::<u32>().ok()) {
                        Some(n) if n > 0 => Some(Some(n)),
                        _ => {
                            self.block(format!(
                                "param:trig SpellCast.ActivatorThisTurnCast {}",
                                shape(c)
                            ));
                            None
                        }
                    },
                };
                Some(Trigger::Cast {
                    by: by?,
                    types,
                    spells,
                    nth: nth?,
                })
            }
            "Attacks" => {
                let filter = rd.get("ValidCard").and_then(|f| self.obj_filter(f));
                // "Attacks alone" (CR 506.5): Forge only tests that the key is there.
                match rd.get("Alone") {
                    None => filter.map(Trigger::Attacks),
                    Some(_) => filter.map(Trigger::AttacksAlone),
                }
            }
            // Damage being dealt (CR 120). `DamageDone`: once for each source and
            // recipient; `DamageDoneOnce`: once for each recipient ("is dealt damage",
            // "one or more creatures deal damage"); `DamageDealtOnce`: once for each
            // source ("deals damage").
            mode @ ("DamageDone" | "DamageDoneOnce" | "DamageDealtOnce") => {
                let by = match rd.get("ValidSource") {
                    None => Some(None),
                    Some(f) => self.obj_filter(f).map(Some),
                };
                let to = match rd.get("ValidTarget") {
                    None => Some(DamageTo::Anything),
                    Some("Player") => Some(DamageTo::Player(Who::Any)),
                    Some("You") => Some(DamageTo::Player(Who::You)),
                    Some("Opponent" | "Player.Opponent") => Some(DamageTo::Player(Who::Opponent)),
                    Some(f) => self.obj_filter(f).map(DamageTo::Permanent),
                };
                let combat = match rd.get("CombatDamage") {
                    None => Some(None),
                    Some("True") => Some(Some(true)),
                    Some("False") => Some(Some(false)),
                    Some(_) => {
                        self.block("param:trig DamageDone.CombatDamage value");
                        None
                    }
                };
                let per = match mode {
                    "DamageDone" => DamagePer::Each,
                    "DamageDoneOnce" => DamagePer::Recipient,
                    _ => DamagePer::Source,
                };
                Some(Trigger::Damage {
                    by: by?,
                    to: to?,
                    combat: combat?,
                    per,
                })
            }
            // "Whenever you attack [with N or more creatures]" (CR 508.1).
            "AttackersDeclared" => {
                let by = match rd.get("AttackingPlayer") {
                    None | Some("Player") => Some(Who::Any),
                    Some("You") => Some(Who::You),
                    Some("Opponent" | "Player.Opponent") => Some(Who::Opponent),
                    Some(other) => {
                        self.block(format!("trig:AttackersDeclared player {other}"));
                        None
                    }
                };
                let with = match rd.get("ValidAttackers") {
                    None | Some("Creature") => Some(None),
                    Some(f) => self.selector(f).map(Some),
                };
                let at_least = match rd.get("ValidAttackersAmount") {
                    None => Some(1),
                    Some(c) => match self.compare(c) {
                        Some((Cmp::AtLeast, n)) => Some(n),
                        Some(_) => {
                            self.block("trig:AttackersDeclared amount");
                            None
                        }
                        None => None,
                    },
                };
                Some(Trigger::PlayerAttacks {
                    by: by?,
                    with: with?,
                    at_least: at_least?,
                })
            }
            // Life gained or lost, cards drawn or discarded: by whom.
            mode @ ("LifeGained" | "LifeLost" | "Drawn" | "Discarded") => {
                let who = match (rd.get("ValidPlayer"), rd.get("ValidCard")) {
                    (None, None | Some("Card")) => Some(Who::Any),
                    (Some("Player"), None | Some("Card")) => Some(Who::Any),
                    (Some("You"), None | Some("Card")) => Some(Who::You),
                    (Some("Opponent" | "Player.Opponent"), None | Some("Card")) => {
                        Some(Who::Opponent)
                    }
                    // A card in a library or hand is its owner's.
                    (None, Some("Card.YouCtrl" | "Card.YouOwn")) if mode != "LifeGained" => {
                        Some(Who::You)
                    }
                    (None, Some("Card.OppCtrl" | "Card.OppOwn")) if mode != "LifeGained" => {
                        Some(Who::Opponent)
                    }
                    (p, c) => {
                        self.block(format!(
                            "trig:{mode} who {} {}",
                            p.unwrap_or("-"),
                            c.unwrap_or("-")
                        ));
                        None
                    }
                };
                match mode {
                    "LifeGained" => who.map(Trigger::GainsLife),
                    "LifeLost" => who.map(Trigger::LosesLife),
                    "Discarded" => who.map(Trigger::Discards),
                    _ => {
                        // `Number$ 2`: "your second card each turn".
                        let nth = match rd.get("Number") {
                            None => Some(None),
                            Some(n) => match n.parse::<u32>() {
                                Ok(n) if n > 0 => Some(Some(n)),
                                _ => {
                                    self.block("param:trig Drawn.Number value");
                                    None
                                }
                            },
                        };
                        Some(Trigger::Draws {
                            who: who?,
                            nth: nth?,
                        })
                    }
                }
            }
            "Sacrificed" => {
                let who = match rd.get("ValidPlayer") {
                    None | Some("Player") => Some(Who::Any),
                    Some("You") => Some(Who::You),
                    Some("Opponent" | "Player.Opponent") => Some(Who::Opponent),
                    Some(other) => {
                        self.block(format!("trig:Sacrificed player {other}"));
                        None
                    }
                };
                let what = match rd.get("ValidCard") {
                    None => Some(ObjFilter::Matching(Selector::default())),
                    Some(f) => self.obj_filter(f),
                };
                // "When you sacrifice this" would have to trigger from the graveyard.
                if what == Some(ObjFilter::This) {
                    self.block("trig:Sacrificed this");
                    return None;
                }
                Some(Trigger::Sacrifices {
                    who: who?,
                    what: what?,
                })
            }
            // Blockers being declared (CR 509.1h).
            // CR 603.2e, 701.26a: "whenever [a permanent] becomes tapped" (Forge's
            // TriggerTaps: `ValidCard$`; `Attacker$`, `ValidCause$`, `FirstTime$` … are not
            // read and block).
            "Taps" => rd
                .get("ValidCard")
                .and_then(|f| self.obj_filter(f))
                .map(Trigger::BecomesTapped),
            "Blocks" => {
                let blocker = rd.get("ValidCard").and_then(|f| self.obj_filter(f));
                match rd.get("ValidBlocked") {
                    None => blocker.map(Trigger::Blocks),
                    Some(f) => {
                        let attacker = self.obj_filter(f);
                        Some(Trigger::BlockedBy {
                            attacker: attacker?,
                            blocker: blocker?,
                        })
                    }
                }
            }
            "AttackerBlocked" => {
                let attacker = rd.get("ValidCard").and_then(|f| self.obj_filter(f));
                match rd.get("ValidBlocker") {
                    None => attacker.map(Trigger::BecomesBlocked),
                    // Once however many of those blockers there are: the same as once
                    // for each only where the blocker is this one creature.
                    Some("Card.Self") => Some(Trigger::BlockedBy {
                        attacker: attacker?,
                        blocker: ObjFilter::This,
                    }),
                    Some(_) => {
                        self.block("trig:AttackerBlocked by several");
                        None
                    }
                }
            }
            "AttackerBlockedByCreature" => {
                let attacker = rd.get("ValidCard").and_then(|f| self.obj_filter(f));
                let blocker = rd.get("ValidBlocker").and_then(|f| self.obj_filter(f));
                Some(Trigger::BlockedBy {
                    attacker: attacker?,
                    blocker: blocker?,
                })
            }
            "AttackerUnblocked" => rd
                .get("ValidCard")
                .and_then(|f| self.obj_filter(f))
                .map(Trigger::Unblocked),
            // CR 702.29c: "when you cycle this card"; "whenever [a player] cycles a card",
            // which a permanent watches for (the cycled card is never that permanent, so
            // "another card" says nothing more).
            "Cycled" => {
                let player = rd.get("ValidPlayer");
                match (rd.get("ValidCard"), player) {
                    (Some("Card.Self"), None) => Some(Trigger::Cycles {
                        by: Who::You,
                        this: true,
                    }),
                    (Some("Card" | "Card.Other"), None) => Some(Trigger::Cycles {
                        by: Who::Any,
                        this: false,
                    }),
                    (Some("Card.YouOwn" | "Card.YouCtrl" | "Card.Other+YouOwn"), None)
                    | (Some("Card" | "Card.Other"), Some("You")) => Some(Trigger::Cycles {
                        by: Who::You,
                        this: false,
                    }),
                    (card, _) => {
                        self.block(format!("trig:Cycled card {}", card.unwrap_or("?")));
                        None
                    }
                }
            }
            other => {
                self.block(format!("trig:{other}"));
                None
            }
        }
    }

    /// All `T:` lines. Two lines that run the same ability, one marked `Secondary$`, are
    /// one ability with two events ("enters or attacks").
    fn triggers(&mut self) -> Vec<(usize, AbilityDef)> {
        let mut out: Vec<(usize, &str, AbilityDef)> = Vec::new();
        let face = self.face;
        for (line, rec) in face.triggers.iter().enumerate() {
            self.trigger(line, rec, &mut out);
        }
        out.into_iter().map(|(line, _, a)| (line, a)).collect()
    }

    /// One trigger line: a triggered ability added to `out` (with its line and the name
    /// of what it executes), or a second event for an ability already there.
    fn trigger(
        &mut self,
        line: usize,
        rec: &'a Record,
        out: &mut Vec<(usize, &'a str, AbilityDef)>,
    ) {
        {
            let before = self.blockers.len();
            let rd = Rd::new(rec, format!("trig {}", rec.get("Mode").unwrap_or("?")));
            let event = self.trigger_event(&rd);
            // Without `TriggerZones$` a Forge trigger works from every zone, except that
            // "this enters", "this attacks" and leaves-the-battlefield triggers default to
            // the battlefield (docs/forge/triggers-statics-replacements-keywords.md).
            // The engine's triggers are battlefield abilities, so anything else must say so.
            let about_itself = rec
                .get("ValidCard")
                .is_some_and(|v| v.split(',').any(|alt| alt.trim() == "Card.Self"));
            let own_event = match (rec.get("Mode"), rec.get("Origin")) {
                // Forge's `correctZones`: leaving the battlefield -> the battlefield; a
                // `ValidCard` that includes the card itself -> the destination zone.
                (Some("ChangesZone"), Some("Battlefield")) => true,
                (Some("ChangesZone"), None | Some("Any")) => {
                    about_itself && rec.get("Destination") == Some("Battlefield")
                }
                // An attacking creature is on the battlefield.
                (Some("Attacks"), _) => rec.get("ValidCard") == Some("Card.Self"),
                // Only a permanent becomes tapped; and the Aura or Equipment attached to it
                // is on the battlefield too.
                (Some("Taps"), _) => matches!(
                    rec.get("ValidCard"),
                    Some("Card.Self" | "Card.AttachedBy" | "Creature.AttachedBy")
                ),
                // "When you cycle this card": from wherever the card went.
                (Some("Cycled"), _) => rec.get("ValidCard") == Some("Card.Self"),
                // A creature that blocks or is blocked is on the battlefield.
                (
                    Some(
                        "Blocks"
                        | "AttackerBlocked"
                        | "AttackerBlockedByCreature"
                        | "AttackerUnblocked",
                    ),
                    _,
                ) => ["ValidCard", "ValidBlocker", "ValidBlocked"]
                    .iter()
                    .any(|k| rec.get(k) == Some("Card.Self")),
                // A permanent that deals or is dealt damage is on the battlefield; so is
                // an Aura or Equipment whose host does. (An instant's own "whenever this
                // deals damage" would be something else.)
                (Some("DamageDone" | "DamageDoneOnce" | "DamageDealtOnce"), _) => {
                    let about = |key: &str| {
                        rec.get(key).is_some_and(|v| {
                            v == "Card.Self"
                                || v.ends_with(".EquippedBy")
                                || v.ends_with(".EnchantedBy")
                                || v.ends_with(".AttachedBy")
                        })
                    };
                    (about("ValidSource") || about("ValidTarget")) && self.permanent()
                }
                _ => false,
            };
            match rd.get("TriggerZones") {
                Some("Battlefield") => {}
                None if own_event => {}
                // An ability something is given works where that thing is given it.
                None if self.granting > 0 => {}
                // "Whenever a creature you control of the chosen type enters": only a
                // permanent has a choice (made as it entered), so it can trigger nowhere else.
                None if rec.get("ValidCard").is_some_and(|v| {
                    v.split(['.', '+', ','])
                        .any(|w| w == "ChosenType" || w == "ChosenColor")
                }) => {}
                None => self.block("trig:zone unstated"),
                Some(z) => self.block(format!("trig:zone {z}")),
            }
            // Who decides whether the ability does anything (decided once its event is
            // known, below).
            let decider = rd.get("OptionalDecider");
            let optional = decider.is_some();
            let secondary = match rd.get("Secondary") {
                None => false,
                Some("True") => true,
                // Forge only tests that the key is present.
                Some(_) => {
                    self.block("trig:Secondary value");
                    true
                }
            };
            let cond = self.condition(&rd);
            let exec = rd.get("Execute").unwrap_or("");
            rd.finish(&mut self.blockers);
            if secondary
                && let Some(event) = &event
                && ((self.it_is_you.contains(exec) && !TrigCtx::of(event).it_is_this)
                    || (self.player_is_you.contains(exec) && !TrigCtx::of(event).player_is_you))
            {
                self.block("trig:Secondary about another object");
                return;
            }
            if secondary
                && let Some(event) = &event
                && let Some((_, _, AbilityDef::Triggered { when, .. })) =
                    out.iter_mut().find(|(_, e, _)| *e == exec)
            {
                let first = std::mem::replace(when, Trigger::Any(Vec::new()));
                *when = match first {
                    Trigger::Any(mut v) => {
                        v.push(event.clone());
                        Trigger::Any(v)
                    }
                    one => Trigger::Any(vec![one, event.clone()]),
                };
                return;
            }
            if secondary {
                self.block("trig:Secondary-alone");
            }
            self.in_trigger = true;
            self.optional_trigger = optional;
            self.trig = event.as_ref().map(TrigCtx::of).unwrap_or_default();
            self.took_it_for_this = false;
            self.took_player_for_you = false;
            // "You may …", or "that creature's controller may …" (CR 603.5: decided as
            // the ability resolves). Forge asks this player whether the ability resolves.
            let decider = match decider {
                None => Some(None),
                Some(name) => self
                    .one_player(name, &Chain::default(), true, "trig:OptionalDecider ")
                    .map(Some),
            };
            let body = match self.sub(exec) {
                Some(r) => self.body(r, true, "DB"),
                None => None,
            };
            if std::mem::take(&mut self.took_it_for_this) {
                self.it_is_you.insert(exec.to_string());
            }
            if std::mem::take(&mut self.took_player_for_you) {
                self.player_is_you.insert(exec.to_string());
            }
            self.in_trigger = false;
            self.optional_trigger = false;
            self.trig = TrigCtx::default();
            let (Some(when), Some(intervening_if), Some(mut body), Some(decider)) =
                (event, cond, body, decider)
            else {
                self.explained(before, "trigger");
                return;
            };
            // "You may pay …. If you do, …" is the choice already: asking "may?" first
            // would ask twice.
            let priced = matches!(
                &body,
                Body::Effect(Mode {
                    effect: Effect::Pay {
                        who: PlayerRef::You,
                        ..
                    },
                    ..
                })
            );
            if priced && decider.is_some_and(|d| d != PlayerRef::You) {
                // Forge asks the decider, then the activator pays.
                self.block("trig:OptionalDecider with a cost");
                return;
            }
            // "You may [X]. If you do, [Y]": Forge runs the whole chain once its decider
            // says yes; the card does Y only if X happened.
            let if_you_do = rec.get("TriggerDescription").is_some_and(|d| {
                let d = d.to_lowercase();
                d.matches("if you do").count() + d.matches("this way").count() == 1
                    && d.contains("may ")
            });
            if decider.is_some()
                && !priced
                && if_you_do
                && let Body::Effect(m) = &mut body
                && let Effect::Seq(v) = &mut m.effect
            {
                // The first instruction that does something ("where X is" comes with it).
                let first = v
                    .iter()
                    .position(|e| !matches!(e, Effect::SetX(_)))
                    .unwrap_or(0);
                if first + 1 < v.len() {
                    let then = v.split_off(first + 1);
                    let first = std::mem::take(v);
                    m.effect = Effect::IfDone {
                        first: Box::new(Effect::Seq(first)),
                        then: Box::new(if then.len() == 1 {
                            then.into_iter().next().expect("one")
                        } else {
                            Effect::Seq(then)
                        }),
                    };
                }
            }
            if let Some(decider) = decider
                && !priced
            {
                // "You may": decided as the ability resolves.
                let wrap = |effect: Effect| match decider {
                    PlayerRef::You => Effect::May(Box::new(effect)),
                    who => Effect::PlayerMay {
                        who,
                        effect: Box::new(effect),
                    },
                };
                body = match body {
                    Body::Effect(m) => Body::Effect(Mode {
                        targets: m.targets,
                        effect: wrap(m.effect),
                        groups: m.groups,
                    }),
                    modal => {
                        self.block("trig:optional-modal");
                        modal
                    }
                };
            }
            out.push((
                line,
                exec,
                AbilityDef::Triggered {
                    when,
                    intervening_if,
                    body,
                },
            ));
        }
    }

    /// Rules about `affected` for a lasting effect to carry: added to the card's `granted`
    /// list. Returns their places in it.
    fn carry(&mut self, rules: &[Hidden], affected: &Affected) -> Option<Vec<u16>> {
        let mut out = Vec::new();
        for rule in rules {
            let Some(a) = hidden_ability(*rule, affected) else {
                self.block("rule:about several kinds of object");
                return None;
            };
            out.push(self.granted.len() as u16);
            self.granted.push(a);
        }
        Some(out)
    }

    /// Abilities other objects are given ("has \"…\""): each named script variable,
    /// compiled as an ability of whatever gets it and added to the card's `granted`
    /// list. Returns their places in that list.
    fn grant(&mut self, kind: Given, names: &str, separator: char) -> Option<Vec<u16>> {
        let mut out = Vec::new();
        // An ability that gives an ability that gives … itself would never end.
        if self.granting >= 3 {
            self.block("granted:nested too deep");
            return None;
        }
        for name in names.split(separator).map(str::trim) {
            let rec = self.sub(name)?;
            // Nothing about the ability being compiled around this one carries over.
            let outer = (
                self.in_trigger,
                self.optional_trigger,
                self.trig,
                self.from_zone,
                self.in_spell,
                self.has_x_cost,
                self.enchant,
                self.equipment,
            );
            let outer_additional = std::mem::take(&mut self.additional);
            let outer_took = (
                std::mem::take(&mut self.took_it_for_this),
                std::mem::take(&mut self.took_player_for_you),
            );
            self.in_trigger = false;
            self.optional_trigger = false;
            self.trig = TrigCtx::default();
            self.from_zone = ActivationZone::Battlefield;
            self.in_spell = false;
            self.has_x_cost = false;
            self.enchant = None;
            self.equipment = false;
            self.granting += 1;
            let before = self.blockers.len();
            let a = match kind {
                Given::Activated => match rec.head() {
                    Some(("AB", _)) => self.ability(rec),
                    _ => {
                        self.block("granted:not an activated ability");
                        None
                    }
                },
                Given::Triggered => {
                    let mut one = Vec::new();
                    self.trigger(0, rec, &mut one);
                    one.pop().map(|(_, _, a)| a)
                }
                Given::Static => self.static_ability(rec),
                Given::Replacement => self.replacement(rec),
            };
            self.granting -= 1;
            (
                self.in_trigger,
                self.optional_trigger,
                self.trig,
                self.from_zone,
                self.in_spell,
                self.has_x_cost,
                self.enchant,
                self.equipment,
            ) = outer;
            let extra = std::mem::replace(&mut self.additional, outer_additional);
            (self.took_it_for_this, self.took_player_for_you) = outer_took;
            let Some(a) = a else {
                self.explained(before, "granted ability");
                return None;
            };
            // ("Can't attack or block" is two abilities.)
            for a in std::iter::once(a).chain(extra) {
                out.push(self.granted.len() as u16);
                self.granted.push(a);
            }
        }
        Some(out)
    }

    fn static_ability(&mut self, rec: &'a Record) -> Option<AbilityDef> {
        self.nothing = false;
        let mode = rec.get("Mode").unwrap_or("?");
        let rd = Rd::new(rec, format!("static {mode}"));
        let out = match mode {
            // Rules about players, and about where cards may be played from.
            "Continuous" if Self::is_player_static(&rd) => self.player_static(&rd),
            "Continuous" => {
                let affected = match (rd.get("AffectedDefined"), rd.get("Affected")) {
                    (None, Some("Card.Self")) | (Some("Self"), None | Some("Card.Self")) => {
                        Some(Affected::This)
                    }
                    // "Enchanted creature …". Forge filters the enchanted permanent again
                    // with `Affected$`; that says nothing new only when it repeats what the
                    // Aura can enchant at all.
                    // "Equipped creature …": the same for an Equipment.
                    (Some("Equipped"), None | Some("Creature")) if self.equipment => {
                        Some(Affected::Enchanted)
                    }
                    (Some("Enchanted"), again) => match (self.enchant, again) {
                        (Some(_), None) => Some(Affected::Enchanted),
                        (Some(e), Some(a)) if e == a => Some(Affected::Enchanted),
                        (None, _) => {
                            self.block("static:Enchanted-without-Enchant");
                            None
                        }
                        (Some(_), Some(_)) => {
                            self.block("static:Enchanted-as-long-as");
                            None
                        }
                    },
                    (Some(other), _) => {
                        self.block(format!("static:AffectedDefined {other}"));
                        None
                    }
                    (None, Some(f)) => self.selector(f).map(Affected::All),
                    // A characteristic-defining ability is about its own card.
                    (None, None) if rd.rec.get("CharacteristicDefining").is_some() => {
                        Some(Affected::This)
                    }
                    (None, None) => {
                        self.block("static:no-Affected");
                        None
                    }
                };
                // Where the ability works: on the battlefield (Forge's default), or — for
                // what an effect object carries — the command zone.
                let mut from_graveyard = false;
                if rd.rec.get("CharacteristicDefining").is_none() {
                    match rd.get("EffectZone") {
                        None | Some("Battlefield") => {}
                        Some("Command") if self.in_effect => {}
                        // "As long as this card is in your graveyard, …" (CR 113.6b).
                        Some("Graveyard") if !self.in_effect && self.granting == 0 => {
                            from_graveyard = true;
                        }
                        Some(z) => {
                            self.block(format!("static:works from {}", shape(z)));
                            return None;
                        }
                    }
                }
                let int = |t: Option<&str>| {
                    t.map_or(Some(0), |t| t.trim_start_matches('+').parse::<i32>().ok())
                };
                let (att, def) = (rd.get("AddPower"), rd.get("AddToughness"));
                let mut effects = Vec::new();
                // "This creature's power and toughness are each equal to …" (CR 604.3).
                if rd.get("CharacteristicDefining").is_some() {
                    // It works everywhere, whatever zone the script names.
                    rd.get("EffectZone");
                    let part = |tx: &mut Self, key: &'a str| -> Option<Option<Box<Amount>>> {
                        let Some(text) = rd.get(key) else {
                            return Some(None);
                        };
                        let amount = match text.parse::<u32>() {
                            Ok(n) => Amount::Fixed(n),
                            Err(_) => tx.expr(text)?,
                        };
                        if !amount.countable_in_layers() {
                            tx.block("static:characteristic-defining amount");
                            return None;
                        }
                        Some(Some(Box::new(amount)))
                    };
                    let (power, toughness) = (part(self, "SetPower"), part(self, "SetToughness"));
                    if affected != Some(Affected::This) {
                        self.block("static:characteristic-defining not Self");
                        return None;
                    }
                    let (power, toughness) = (power?, toughness?);
                    self.defines_pt = (power.is_some(), toughness.is_some());
                    effects.push(ContinuousEffect::SetPTBy { power, toughness });
                }
                match (int(att), int(def)) {
                    (Some(p), Some(t)) => {
                        if (p, t) != (0, 0) {
                            effects.push(ContinuousEffect::ModifyPT(p, t));
                        }
                    }
                    // "+1/+1 for each [permanent]": counted continuously. Only counts of
                    // permanents — the layer walk can't evaluate anything else in place.
                    _ => {
                        let mut part = |text: Option<&str>| -> Option<Signed> {
                            let text = text.unwrap_or("0");
                            let negative = text.starts_with('-');
                            let name = text.trim_start_matches(['+', '-']);
                            let amount = match name.parse::<u32>() {
                                Ok(n) => Amount::Fixed(n),
                                Err(_) => self.expr(name)?,
                            };
                            match &amount {
                                Amount::Fixed(_) => {}
                                Amount::Count(s)
                                    if s.power_min.is_none() && s.toughness_min.is_none() => {}
                                _ => {
                                    self.block("static:variable-pt");
                                    return None;
                                }
                            }
                            Some(Signed { negative, amount })
                        };
                        let (power, toughness) = (part(att), part(def));
                        effects.push(ContinuousEffect::ModifyPTBy {
                            power: Box::new(power?),
                            toughness: Box::new(toughness?),
                        });
                    }
                }
                // Layer 4: "are [types] in addition to their other types".
                if let Some(types) = rd.get("AddType") {
                    effects.insert(0, self.added_types(types.split('&'))?);
                }
                // Layer 4: "isn't a creature" (card types only; the subtypes stay).
                if let Some(types) = rd.get("RemoveType") {
                    let mut ts = Vec::new();
                    for t in types.split('&').map(str::trim) {
                        match card_type(t) {
                            Some(t) => ts.push(t),
                            None => {
                                self.block(format!("static:RemoveType {}", shape(t)));
                                return None;
                            }
                        }
                    }
                    let at = effects
                        .iter()
                        .position(|e| !matches!(e, ContinuousEffect::AddTypes { .. }))
                        .unwrap_or(effects.len());
                    effects.insert(at, ContinuousEffect::RemoveTypes(ts));
                }
                // Layer 5: "is [color]" (not "in addition").
                if let Some(colors) = rd.get("SetColor") {
                    let mut cs = Vec::new();
                    for c in colors.split(',').map(str::trim) {
                        match color(c) {
                            Some(c) => cs.push(c),
                            None => {
                                self.block(format!("static:color {}", shape(c)));
                                return None;
                            }
                        }
                    }
                    cs.sort();
                    effects.push(ContinuousEffect::SetColors(cs));
                }
                // Layer 6: "lose [keyword]", "lose all abilities".
                if let Some(kw) = rd.get("RemoveKeyword") {
                    let mut ks = Vec::new();
                    for k in kw.split('&').map(str::trim) {
                        match keyword(k) {
                            Some(k) => ks.push(k),
                            None => {
                                self.block(format!("kw:{k}"));
                                return None;
                            }
                        }
                    }
                    effects.push(ContinuousEffect::LoseKeywords(ks));
                }
                if rd.get("RemoveAllAbilities").is_some() {
                    effects.push(ContinuousEffect::LoseAllAbilities);
                }
                // What one ability both takes away and gives is given ("loses all other
                // abilities and has flying"): Forge removes first.
                // (Some "keywords" are rules about the creatures, not abilities they have:
                // "can't block", "prevent all damage that would be dealt to [them]".)
                let mut rules = Vec::new();
                for key in ["AddKeyword", "AddHiddenKeyword"] {
                    let Some(kw) = rd.get(key) else { continue };
                    let mut ks = Vec::new();
                    for k in kw.split('&').map(str::trim) {
                        match (keyword(k), hidden_rules(k)) {
                            (Some(k), _) if key == "AddKeyword" => ks.push(k),
                            (_, Some(hidden)) => rules.extend(hidden),
                            _ => {
                                self.block(format!("kw:{k}"));
                                return None;
                            }
                        }
                    }
                    if !ks.is_empty() {
                        effects.push(ContinuousEffect::AddKeywords(ks));
                    }
                }
                // "Has \"[ability]\"" (CR 113.2c): script variables holding the abilities,
                // read as abilities of whatever is affected.
                let mut given = Vec::new();
                for (key, kind) in [
                    ("AddAbility", Given::Activated),
                    ("AddTrigger", Given::Triggered),
                    ("AddStaticAbility", Given::Static),
                    ("AddReplacementEffect", Given::Replacement),
                ] {
                    if let Some(names) = rd.get(key) {
                        given.extend(self.grant(kind, names, '&')?);
                    }
                }
                if !given.is_empty() {
                    effects.push(ContinuousEffect::GrantAbilities(given));
                }
                // `AddSVar$` copies script variables to the affected card so that what it
                // is given finds them there. Here they are found on this card anyway —
                // unless the copy goes by another name, which only hints for Forge's AI do.
                if let Some(names) = rd.get("AddSVar") {
                    for name in names.split('&').map(str::trim) {
                        let renamed = self
                            .svar(name)
                            .and_then(|v| v.strip_prefix("SVar:"))
                            .map(|v| v.split(':').next().unwrap_or(v));
                        if renamed.is_some_and(|n| !AI_HINTS.contains(&n)) {
                            self.block("static:AddSVar under another name");
                            return None;
                        }
                    }
                }
                // Layer 7b: "have base power and toughness N/N" (not a
                // characteristic-defining ability, which is read above).
                if rd.rec.get("CharacteristicDefining").is_none() {
                    let number = |t: Option<&str>| t.map(|t| t.parse::<i32>().ok());
                    match (number(rd.get("SetPower")), number(rd.get("SetToughness"))) {
                        (None, None) => {}
                        (Some(Some(p)), Some(Some(t))) => {
                            effects.push(ContinuousEffect::SetPT(p, t))
                        }
                        _ => {
                            self.block("static:base power and toughness");
                            return None;
                        }
                    }
                }
                // "You control [it]" (layer 2).
                match rd.get("GainControl") {
                    None => {}
                    Some("You") => effects.insert(0, ContinuousEffect::GainControl),
                    Some(other) => {
                        self.block(format!("param:static Continuous.GainControl={other}"));
                        return None;
                    }
                }
                if effects.is_empty() && rules.is_empty() {
                    // Name what wasn't understood: the parameters that weren't read.
                    let before = self.blockers.len();
                    rd.finish(&mut self.blockers);
                    if self.blockers.len() > before {
                        return None;
                    }
                    // A line that only copies hints for Forge's AI to what it affects
                    // (`AddSVar$`, checked above) says nothing about the rules.
                    if rd.rec.get("AddSVar").is_some() && only_ai_hints(rd.rec) {
                        self.nothing = true;
                        return None;
                    }
                    self.block("static:Continuous-nothing");
                    return None;
                }
                let only_if = self.static_condition(&rd)?;
                let affected = affected?;
                // The rules are abilities of their own, about the same permanents.
                let mut out = Vec::new();
                if !effects.is_empty() {
                    out.push(AbilityDef::Static {
                        affected: affected.clone(),
                        effects,
                        only_if: only_if.clone(),
                    });
                }
                if !rules.is_empty() && only_if.is_some() {
                    self.block("static:a rule under a condition");
                    return None;
                }
                for rule in rules {
                    match hidden_ability(rule, &affected) {
                        Some(a) => out.push(a),
                        None => {
                            self.block("rule:about several kinds of object");
                            return None;
                        }
                    }
                }
                // A static ability of a card in a graveyard: about permanents only (its own
                // card isn't one), one ability.
                if from_graveyard {
                    let about_permanents = matches!(affected, Affected::All(_));
                    if out.len() != 1 || !about_permanents {
                        self.block("static:from the graveyard about the card itself");
                        return None;
                    }
                    let a = out.remove(0);
                    return Some(AbilityDef::FromZone {
                        zone: ActivationZone::Graveyard,
                        ability: Box::new(a),
                    });
                }
                let first = out.remove(0);
                self.additional.extend(out);
                Some(first)
            }
            // Combat restrictions (CR 508.1c, 509.1b). One Forge line may carry several
            // modes ("can't attack or block").
            "CantAttack" | "CantBlock" | "CantAttack,CantBlock" => {
                let affected = rd.get("ValidCard").and_then(|f| self.affected(f));
                let affected = affected.or_else(|| {
                    self.block(format!("static:{mode}-who"));
                    None
                })?;
                let mut rules: Vec<CombatRule> = mode
                    .split(',')
                    .map(|m| match m {
                        "CantAttack" => CombatRule::CantAttack,
                        _ => CombatRule::CantBlock,
                    })
                    .collect();
                let first = rules.remove(0);
                for rule in rules {
                    self.additional.push(AbilityDef::CombatRule {
                        affected: affected.clone(),
                        rule,
                    });
                }
                Some(AbilityDef::CombatRule {
                    affected,
                    rule: first,
                })
            }
            // CR 701.19c: "[these] can't be regenerated".
            "CantRegenerate" => {
                let affected = rd.get("ValidCard").and_then(|f| self.affected(f));
                let affected = affected.or_else(|| {
                    self.block("static:CantRegenerate-who");
                    None
                })?;
                Some(AbilityDef::CantBeRegenerated(affected))
            }
            "CantBlockBy" => {
                let (attacker, blocker) = (rd.get("ValidAttacker"), rd.get("ValidBlocker"));
                let is_self = |f: &str| matches!(f, "Card.Self" | "Creature.Self");
                let source = |f: &str| {
                    matches!(
                        f,
                        "Card.EffectSource" | "Creature.EffectSource" | "Permanent.EffectSource"
                    )
                };
                match (attacker, blocker) {
                    // "[Those] can't block this creature" — in an effect object, whose
                    // source "this creature" is.
                    (Some(a), Some(b)) if self.in_effect && source(a) => {
                        let affected = self.affected(b);
                        Some(AbilityDef::CombatRule {
                            affected: affected?,
                            rule: CombatRule::CantBlockSource,
                        })
                    }
                    // "This can't block [those attackers]."
                    (Some(a), Some(b)) if is_self(b) && !is_self(a) => {
                        let sel = self.selector(a)?;
                        Some(AbilityDef::CombatRule {
                            affected: Affected::This,
                            rule: CombatRule::CantBlockAttackers(sel),
                        })
                    }
                    // "[It] can't be blocked [by those]."
                    (Some(a), by) => {
                        let affected = self.affected(a);
                        let by = match by {
                            None => Some(None),
                            Some(b) => self.selector(b).map(Some),
                        };
                        Some(AbilityDef::CombatRule {
                            affected: affected?,
                            rule: CombatRule::CantBeBlocked { by: by? },
                        })
                    }
                    (None, _) => {
                        self.block("static:CantBlockBy-who");
                        None
                    }
                }
            }
            "AlternativeCost" => self.alternative_cost(&rd),
            "ReduceCost" => {
                if rd.get("Type") != Some("Spell") {
                    self.block("static:ReduceCost-type");
                    return None;
                }
                let amount = rd.get("Amount").unwrap_or("1");
                match (
                    rd.get("ValidCard"),
                    rd.get("EffectZone"),
                    rd.get("Activator"),
                ) {
                    // "This spell costs {N} less to cast."
                    (Some("Card.Self"), Some("All"), None) => match amount.parse::<u32>() {
                        Ok(n) => Some(Amount::Fixed(n)),
                        Err(_) => self.expr(amount),
                    }
                    .map(AbilityDef::SelfCostReduction),
                    // "[These] spells you cast cost {N} less to cast." / with no `Activator$`,
                    // anyone's spells ("creature spells of the chosen type cost {2} less").
                    (Some(f), None | Some("Battlefield"), activator @ (Some("You") | None)) => {
                        let anyone = activator.is_none();
                        let spells = self.card_selector(f);
                        if anyone
                            && spells
                                .as_ref()
                                .is_some_and(|s| s.controller != Who::Any || s.owner != Who::Any)
                        {
                            self.block("static:ReduceCost-whose");
                            return None;
                        }
                        let n = amount.parse::<u32>().ok().or_else(|| {
                            self.block("static:ReduceCost-variable");
                            None
                        });
                        Some(AbilityDef::CostReduction {
                            spells: spells?,
                            amount: n?,
                            anyone,
                        })
                    }
                    _ => {
                        self.block("static:ReduceCost-shape");
                        None
                    }
                }
            }
            "Panharmonicon" => self.panharmonicon(&rd),
            other => {
                self.block(format!("static:{other}"));
                None
            }
        };
        rd.finish(&mut self.blockers);
        out
    }

    /// A cost paid rather than a spell's mana cost (CR 118.9): `0` is "without paying its
    /// mana cost" (no mana at all).
    fn alt_cost_of(&mut self, text: &str) -> Option<Cost> {
        let mut cost = self.cost(text)?;
        if cost
            .mana
            .as_ref()
            .is_some_and(|m| m.mana_value() == 0 && !m.has_x())
        {
            cost.mana = None;
        }
        if cost.tap || cost.sacrifice_self || cost.discard_self || cost.exile_self {
            self.block("altcost:names its own card");
            return None;
        }
        Some(cost)
    }

    /// CR 702.96a–c: the spell's text with every "target" changed to "each". Forge's reading
    /// (`CardFactoryUtil`, `Overload`): the spell ability without target restrictions, its
    /// objects `Defined$ Valid <ValidTgts>` — every object its target filter allows. Here:
    /// the spell's one instruction in its mass form (`Destroy` → `DestroyAll` of
    /// `ValidCards$ <ValidTgts>`, `ChangeZone` → `ChangeZoneAll`, …), compiled as any such
    /// line. A spell of more than one instruction, or whose targets may be players, is not
    /// compiled.
    fn overloaded(&mut self) -> Option<Body> {
        let [rec] = self.face.abilities.as_slice() else {
            self.block("kw:Overload-shape");
            return None;
        };
        let (kind, api) = rec.head()?;
        if kind != "SP" || rec.get("SubAbility").is_some() {
            self.block("kw:Overload of more than one instruction");
            return None;
        }
        let from_battlefield = matches!(rec.get("Origin"), Some("Battlefield"));
        let (all, key) = match api {
            "Destroy" => ("DestroyAll", "ValidCards"),
            "ChangeZone" if from_battlefield => ("ChangeZoneAll", "ChangeType"),
            "DealDamage" => ("DamageAll", "ValidCards"),
            "Tap" => ("TapAll", "ValidCards"),
            "Untap" => ("UntapAll", "ValidCards"),
            "Pump" => ("PumpAll", "ValidCards"),
            "PutCounter" => ("PutCounterAll", "ValidCards"),
            other => {
                self.block(format!("kw:Overload of {other}"));
                return None;
            }
        };
        let Some(valid) = rec.get("ValidTgts") else {
            self.block("kw:Overload without a target");
            return None;
        };
        let objects_only = valid.split(',').all(|v| {
            let head = v.trim().split('.').next().unwrap_or("");
            !player_word(head) && head != "Any"
        });
        if !objects_only || rec.get("TargetType").is_some() {
            self.block("kw:Overload of a target that may be a player or spell");
            return None;
        }
        let mut params = Vec::new();
        for (k, v) in &rec.params {
            match k.as_str() {
                "SP" => params.push(("SP".to_string(), all.to_string())),
                "ValidTgts" => params.push((key.to_string(), v.clone())),
                // How many targets: every one of them, overloaded.
                "TgtPrompt" | "TargetMin" | "TargetMax" | "AITgts" | "AITgtsStrict" => {}
                _ => params.push((k.clone(), v.clone())),
            }
        }
        // Compiled like the card's own lines; it lives as long as the compiler's borrows.
        let rec: &'a Record = Box::leak(Box::new(Record { params }));
        self.in_spell = true;
        let body = self.body(rec, false, "SP");
        self.in_spell = false;
        body
    }

    /// `S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ …`: "you may
    /// [cost] rather than pay this spell's mana cost", "if …, you may cast this spell without
    /// paying its mana cost" (CR 118.9). Forge offers it wherever the card is cast from
    /// (`EffectZone$ All`), to its controller (`ValidPlayer$ You`: the card's owner, the only
    /// one who casts it here), when the conditions hold as casting begins
    /// (`StaticAbility.checkConditions`). Alternative costs that other permanents give
    /// spells, abilities' alternative costs, `Announce$`, `XAlternative$`,
    /// `ManaRestriction$` are not compiled (named by their parameters).
    fn alternative_cost(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        let own = matches!(
            (rd.get("ValidSA"), rd.get("ValidCard")),
            (Some("Spell.Self"), None) | (Some("Spell"), Some("Card.Self"))
        );
        if !own {
            self.block("static:AlternativeCost of other spells or abilities");
            return None;
        }
        if !matches!(rd.get("ValidPlayer"), None | Some("You")) {
            self.block("static:AlternativeCost for another player");
            return None;
        }
        if rd.get("EffectZone") != Some("All") {
            self.block("static:AlternativeCost from one zone");
            return None;
        }
        let cost = match rd.get("Cost") {
            Some(text) => self.alt_cost_of(text),
            None => {
                self.block("static:AlternativeCost without a cost");
                None
            }
        };
        let named = match rd.get("Condition") {
            None => Some(None),
            Some("PlayerTurn") => Some(Some(Condition::YourTurn)),
            Some("NotPlayerTurn") => Some(Some(Condition::NotYourTurn)),
            Some(other) => {
                self.block(format!("condition:{other}"));
                None
            }
        };
        let present = self.condition(rd);
        let (cost, named, present) = (cost?, named?, present?);
        let only_if = match (named, present) {
            (Some(_), Some(_)) => {
                self.block("condition:two");
                return None;
            }
            (a, b) => a.or(b),
        };
        Some(AbilityDef::AlternativeCost(AltCost {
            kind: AltKind::Rather,
            cost,
            only_if,
        }))
    }

    /// `S:Mode$ Panharmonicon` (D-073, CR 603.2d): "if [an artifact or creature entering /
    /// a creature dying / a creature attacking] causes a triggered ability of [a permanent
    /// you control] to trigger, that ability triggers an additional time". `ValidCard$` is
    /// the trigger's source; `ValidMode$` the kind of trigger event, with `ValidCause$` (and
    /// `Origin$`/`Destination$` for zone changes) what caused it. Forge's `ValidZone$`
    /// default, the battlefield, is the only zone compiled.
    fn panharmonicon(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        if self.in_effect || self.granting > 0 {
            self.block("static:Panharmonicon elsewhere");
            return None;
        }
        let sources = match rd.get("ValidCard") {
            Some(f) => self.obj_filter(f),
            None => {
                self.block("static:Panharmonicon without ValidCard");
                None
            }
        };
        let mode = rd.get("ValidMode");
        let cause = rd.get("ValidCause");
        let (origin, dest) = (rd.get("Origin"), rd.get("Destination"));
        let zone_change = matches!(mode, Some("ChangesZone,ChangesZoneAll" | "ChangesZone"));
        let attack = matches!(
            mode,
            Some("Attacks,AttackersDeclared,AttackersDeclaredOneTarget" | "Attacks")
        );
        let cause = match (mode, cause, origin, dest) {
            (None, None, None, None) => Some(None),
            (Some(_), Some(c), None, Some("Battlefield")) if zone_change => {
                self.selector(c).map(|s| Some(TriggerCause::Enters(s)))
            }
            (Some(_), Some(c), Some("Battlefield"), Some("Graveyard")) if zone_change => {
                self.selector(c).map(|s| Some(TriggerCause::Dies(s)))
            }
            (Some(_), Some(c), None, None) if attack => {
                self.selector(c).map(|s| Some(TriggerCause::Attacks(s)))
            }
            (Some(m), ..) => {
                self.block(format!("static:Panharmonicon mode {}", shape(m)));
                None
            }
            _ => {
                self.block("static:Panharmonicon shape");
                None
            }
        };
        Some(AbilityDef::AdditionalTrigger {
            sources: sources?,
            cause: cause?,
        })
    }

    /// Is this `Mode$ Continuous` line (also) a rule about players or about where cards may
    /// be played from? Forge applies these keys to the affected *players* (`Affected$ You`)
    /// or lets the affected *cards* be played from their zone (`MayPlay$`).
    fn is_player_static(rd: &Rd<'a>) -> bool {
        const KEYS: [&str; 5] = [
            "SetMaxHandSize",
            "RaiseMaxHandSize",
            "AdjustLandPlays",
            "MayPlay",
            "MayLookAt",
        ];
        KEYS.iter().any(|k| rd.rec.get(k).is_some())
            || rd.rec.get("Affected").is_some_and(|a| {
                a.split(',')
                    .any(|alt| player_word(alt.trim().split('.').next().unwrap_or("")))
            })
    }

    /// A `Mode$ Continuous` line about players (`Affected$ You | SetMaxHandSize$ Unlimited`)
    /// or about cards that may be played from a zone (`Affected$ Land.YouOwn | MayPlay$ True
    /// | AffectedZone$ Graveyard`), compiled to [`AbilityDef::PlayerRule`]s. Card
    /// alternatives of the same line that are given keywords ("you and creatures you
    /// control have hexproof") become static abilities of their own. The first ability is
    /// returned, the rest go to `self.additional`.
    fn player_static(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        // (Looked at below; read here so that an early failure names only what failed.)
        for key in ["MayPlay", "MayLookAt", "AffectedZone", "EffectZone"] {
            rd.get(key);
        }
        if rd.get("AffectedDefined").is_some() {
            self.block("static:player rule AffectedDefined");
            return None;
        }
        let Some(affected) = rd.get("Affected") else {
            self.block("static:player rule without Affected");
            return None;
        };
        let mut players: Vec<Who> = Vec::new();
        let mut cards: Vec<&'a str> = Vec::new();
        for alt in affected.split(',').map(str::trim) {
            match alt {
                "You" => players.push(Who::You),
                "Player" => players.push(Who::Any),
                "Opponent" | "Player.Opponent" => players.push(Who::Opponent),
                _ if player_word(alt.split('.').next().unwrap_or("")) => {
                    self.block(format!("filter:player {}", shape(alt)));
                    return None;
                }
                _ => cards.push(alt),
            }
        }
        if players.len() > 1 {
            self.block("filter:several kinds of player");
            return None;
        }
        let only_if = self.static_condition(rd)?;
        let mut out: Vec<AbilityDef> = Vec::new();
        let rule = |who: Who, rule: PlayerRule| AbilityDef::PlayerRule {
            who,
            rule,
            only_if: only_if.clone(),
        };
        // Keywords: players have hexproof or shroud; permanents any keyword.
        let mut keywords: Vec<&'a str> = Vec::new();
        if let Some(kw) = rd.get("AddKeyword") {
            keywords = kw.split('&').map(str::trim).collect();
        }
        if let Some(&who) = players.first() {
            // A player's maximum hand size (CR 402.2).
            match rd.get("SetMaxHandSize") {
                None => {}
                Some("Unlimited") => out.push(rule(who, PlayerRule::NoMaximumHandSize)),
                Some(n) => match n.parse::<u32>() {
                    Ok(n) => out.push(rule(who, PlayerRule::MaximumHandSize(n))),
                    Err(_) => {
                        self.block("static:SetMaxHandSize variable");
                        return None;
                    }
                },
            }
            if let Some(n) = rd.get("RaiseMaxHandSize") {
                match n.trim_start_matches('+').parse::<i32>() {
                    Ok(n) => out.push(rule(who, PlayerRule::MaximumHandSizeBy(n))),
                    Err(_) => {
                        self.block("static:RaiseMaxHandSize variable");
                        return None;
                    }
                }
            }
            // Land plays (CR 305.2).
            match rd.get("AdjustLandPlays") {
                None => {}
                Some("Unlimited") => out.push(rule(who, PlayerRule::AnyNumberOfLandPlays)),
                Some(n) => match n.parse::<u32>() {
                    Ok(n) if n > 0 => out.push(rule(who, PlayerRule::AdditionalLandPlays(n))),
                    _ => {
                        self.block("static:AdjustLandPlays variable");
                        return None;
                    }
                },
            }
            for k in &keywords {
                match *k {
                    "Hexproof" => out.push(rule(who, PlayerRule::Hexproof)),
                    "Shroud" => out.push(rule(who, PlayerRule::Shroud)),
                    other => {
                        self.block(format!("player keyword:{}", shape(other)));
                        return None;
                    }
                }
            }
            if out.is_empty() {
                self.block("static:player rule-nothing");
                return None;
            }
        } else {
            // Only cards: what the line says about players means nothing for them.
            for key in ["SetMaxHandSize", "RaiseMaxHandSize", "AdjustLandPlays"] {
                if rd.get(key).is_some() {
                    self.block(format!("static:{key} for cards"));
                    return None;
                }
            }
        }
        if !cards.is_empty() {
            let may_play = rd.get("MayPlay");
            let may_look = rd.get("MayLookAt");
            if may_play.is_some() || may_look.is_some() {
                if !keywords.is_empty() {
                    self.block("static:MayPlay with keywords");
                    return None;
                }
                out.extend(self.play_from(rd, &cards, may_play, may_look, &only_if)?);
            } else if !keywords.is_empty() {
                // Each alternative is its own static ability: keywords given twice are
                // given once (CR 702: these keywords are redundant).
                let mut ks = Vec::new();
                for k in &keywords {
                    match keyword(k) {
                        Some(k) => ks.push(k),
                        None => {
                            self.block(format!("kw:{k}"));
                            return None;
                        }
                    }
                }
                // Only on the battlefield.
                if rd.get("AffectedZone").is_some_and(|z| z != "Battlefield") {
                    self.block("static:player rule AffectedZone");
                    return None;
                }
                if rd.get("EffectZone").is_some_and(|z| z != "Battlefield") {
                    self.block("static:player rule EffectZone");
                    return None;
                }
                for alt in &cards {
                    let affected = self.affected(alt)?;
                    out.push(AbilityDef::Static {
                        affected,
                        effects: vec![ContinuousEffect::AddKeywords(ks.clone())],
                        only_if: only_if.clone(),
                    });
                }
            } else {
                self.block("static:player rule with cards");
                return None;
            }
        } else if rd.get("MayPlay").is_some() || rd.get("MayLookAt").is_some() {
            self.block("static:MayPlay for players");
            return None;
        }
        if cards.is_empty() || !players.is_empty() {
            // A rule about players works where the permanent is.
            if rd.get("AffectedZone").is_some() {
                self.block("static:player rule AffectedZone");
                return None;
            }
        }
        if out.is_empty() {
            self.block("static:player rule-nothing");
            return None;
        }
        let first = out.remove(0);
        self.additional.extend(out);
        Some(first)
    }

    /// `MayPlay$ True` / `MayLookAt$ …` for the cards `alts` of a static ability: which
    /// cards of which zone the controller may play (CR 601.3, 116.2a) or look at (CR
    /// 401.5).
    fn play_from(
        &mut self,
        rd: &Rd<'a>,
        alts: &[&'a str],
        may_play: Option<&'a str>,
        may_look: Option<&'a str>,
        only_if: &Option<Condition>,
    ) -> Option<Vec<AbilityDef>> {
        let rule = |rule: PlayerRule| AbilityDef::PlayerRule {
            who: Who::You,
            rule,
            only_if: only_if.clone(),
        };
        if may_play.is_some_and(|v| v != "True") {
            self.block("static:MayPlay value");
            return None;
        }
        let mut zones: Vec<PlayZone> = Vec::new();
        let affected_zone = rd.get("AffectedZone");
        for z in affected_zone
            .unwrap_or("Battlefield")
            .split(',')
            .map(str::trim)
        {
            zones.push(match z {
                "Graveyard" => PlayZone::Graveyard,
                "Library" => PlayZone::LibraryTop,
                "Exile" => PlayZone::Exile,
                other => {
                    self.block(format!("static:MayPlay from {other}"));
                    return None;
                }
            });
        }
        let effect_zone = rd.get("EffectZone");
        let mut out = Vec::new();
        // "You may cast this card from your graveyard": the card's own ability, working
        // from the zones it may be cast from (Forge: `EffectZone$` = `AffectedZone$`).
        if alts == ["Card.Self"] {
            if may_look.is_some() {
                self.block("static:MayLookAt Self");
                return None;
            }
            let same = effect_zone.is_some_and(|e| {
                let mut a: Vec<&str> = e.split(',').map(str::trim).collect();
                let mut b: Vec<&str> = affected_zone
                    .unwrap_or("")
                    .split(',')
                    .map(str::trim)
                    .collect();
                a.sort_unstable();
                b.sort_unstable();
                a == b
            });
            if !same {
                self.block("static:MayPlay Self from another zone");
                return None;
            }
            if self.face.types.iter().any(|t| t == "Land") {
                self.block("static:MayPlay Self land");
                return None;
            }
            if zones.contains(&PlayZone::LibraryTop) {
                self.block("static:MayPlay Self from library");
                return None;
            }
            out.push(rule(PlayerRule::PlayThisFrom(zones)));
            return Some(out);
        }
        // Otherwise a permanent's ability, about the controller's own cards.
        if effect_zone.is_some_and(|z| z != "Battlefield") {
            self.block("static:MayPlay EffectZone");
            return None;
        }
        let mut sels = Vec::new();
        for alt in alts {
            let (head, props) = alt.split_once('.').unwrap_or((alt, ""));
            let mut kept: Vec<&str> = Vec::new();
            let (mut top, mut yours) = (false, false);
            for p in props.split('+').map(str::trim).filter(|p| !p.is_empty()) {
                match p {
                    "TopLibrary" => top = true,
                    // A card in a library or graveyard is controlled by its owner.
                    "YouCtrl" | "YouOwn" => yours = true,
                    // "You may play that card this turn": cards an effect remembers.
                    "IsRemembered" => {
                        self.block("static:MayPlay remembered cards");
                        return None;
                    }
                    other => kept.push(other),
                }
            }
            if !yours {
                self.block("static:MayPlay from every player's zone");
                return None;
            }
            if top != (zones == [PlayZone::LibraryTop]) {
                self.block("static:MayPlay library card not on top");
                return None;
            }
            let text = if kept.is_empty() {
                head.to_string()
            } else {
                format!("{head}.{}", kept.join("+"))
            };
            let mut s = self.card_selector(&text)?;
            if s != owned(s.clone()) || s.controller != Who::Any || s.other {
                self.block("static:MayPlay filter");
                return None;
            }
            // "Cast creature spells from the top of your library": a kind of card that
            // isn't a land is cast, never played as a land — not even a land that is also
            // that kind (a land creature). Only "cards" (`Card`), "permanent cards"
            // (`Permanent`) and lands by type (CR 205.3i) take in lands, as Oracle's "play
            // lands and cast spells" does.
            let land_kind = matches!(head, "Card" | "Permanent")
                || s.types.contains(&CardType::Land)
                || (s.types.is_empty()
                    && s.subtypes.iter().any(|t| LAND_TYPES.contains(&t.as_str())));
            if !land_kind && !s.not_types.contains(&CardType::Land) {
                s.not_types.push(CardType::Land);
            }
            sels.push(s);
        }
        // CR 401.5: the top card of the library, revealed or looked at.
        if let Some(look) = may_look {
            let all = sels.iter().all(|s| *s == Selector::default());
            if zones != [PlayZone::LibraryTop] || !all {
                self.block("static:MayLookAt shape");
                return None;
            }
            match (look, may_play) {
                ("Player", _) => out.push(rule(PlayerRule::TopCardRevealed)),
                ("You", _) | ("True", Some(_)) => out.push(rule(PlayerRule::LookAtTopCard)),
                (other, _) => {
                    self.block(format!("static:MayLookAt {other}"));
                    return None;
                }
            }
        }
        if may_play.is_some() {
            for zone in zones {
                out.push(rule(PlayerRule::PlayFrom {
                    zone,
                    cards: sels.clone(),
                }));
            }
        }
        Some(out)
    }

    /// Who a static rule is about: the card itself, what it is attached to, or everything
    /// matching a filter.
    fn affected(&mut self, text: &str) -> Option<Affected> {
        if self.in_effect {
            match text.split_once('.') {
                Some(("Card" | "Creature" | "Permanent", "IsRemembered")) => {
                    return Some(Affected::Remembered);
                }
                Some(("Card" | "Creature" | "Permanent", "EffectSource")) => {
                    return Some(Affected::This);
                }
                _ => {}
            }
        }
        match text {
            "Card.Self" | "Creature.Self" => Some(Affected::This),
            "Creature.EquippedBy" | "Card.EquippedBy" if self.equipment => {
                Some(Affected::Enchanted)
            }
            "Creature.EnchantedBy" | "Card.EnchantedBy" | "Permanent.EnchantedBy" => {
                // Only for an Aura whose enchant filter says no more than this one.
                match (self.enchant, text.split('.').next()) {
                    (Some("Creature"), Some("Creature" | "Card" | "Permanent")) => {
                        Some(Affected::Enchanted)
                    }
                    _ => {
                        self.block("static:EnchantedBy");
                        None
                    }
                }
            }
            _ => self.selector(text).map(Affected::All),
        }
    }

    fn replacement(&mut self, rec: &'a Record) -> Option<AbilityDef> {
        let event = rec.get("Event").unwrap_or("?");
        let rd = Rd::new(rec, format!("repl {event}"));
        // CR 615.1a: "Prevent all [combat] damage that would be dealt [to …] [by …]."
        if event == "DamageDone" && rd.rec.get("Prevent") == Some("True") {
            rd.get("Prevent");
            // ("Secondary": this line shares its printed sentence with another one.)
            rd.get("Secondary");
            // An effect object is in the command zone; a permanent's ability works on
            // the battlefield.
            let zone = match rd.get("ActiveZones") {
                None => true,
                Some("Command") => self.in_effect,
                Some("Battlefield") => !self.in_effect,
                Some(_) => false,
            };
            if !zone {
                self.block("repl:DamageDone zone");
            }
            let replaces = self.damage_replaces(&rd);
            rd.finish(&mut self.blockers);
            if !zone {
                return None;
            }
            return Some(AbilityDef::Replacement {
                replaces: replaces?,
                with: ReplaceWith::Prevent,
            });
        }
        // "If a source you control would deal damage to …, it deals double that damage /
        // that much damage plus 2 instead" (D-073).
        if event == "DamageDone"
            && rd.rec.get("PreventionEffect").is_none()
            && rd.rec.get("DamageTarget").is_none()
        {
            return self.damage_scaled(&rd);
        }
        if event == "Moved" && rd.rec.get("Destination") == Some("Battlefield") {
            return self.moved_enters(&rd);
        }
        if event == "Moved" && rd.rec.get("Destination") == Some("Graveyard") {
            return self.moved_to_graveyard(&rd);
        }
        if event == "CreateToken" {
            return self.replace_create_token(&rd);
        }
        if event == "AddCounter" {
            return self.replace_add_counter(&rd);
        }
        if event == "Counter" {
            return self.cant_be_countered(&rd);
        }
        if event == "GainLife" && rd.rec.get("Prevent").is_none() {
            return self.replace_gain_life(&rd);
        }
        // Not compiled yet: named by the event alone, so that it ranks as one feature.
        for key in [
            "ValidCard",
            "Destination",
            "ReplacementResult",
            "ReplaceWith",
        ] {
            rd.get(key);
        }
        rd.finish(&mut self.blockers);
        self.block(format!("repl:{event}"));
        None
    }

    /// `ValidSource$`, `ValidTarget$` and `IsCombat$` of a `DamageDone` replacement.
    fn damage_replaces(&mut self, rd: &Rd<'a>) -> Option<Replaces> {
        let by = match rd.get("ValidSource") {
            None => Some(None),
            Some(f) => self.damage_source(f),
        };
        let to = match rd.get("ValidTarget") {
            None => Some(DamageTo::Anything),
            Some(f) => self.damage_recipient(f),
        };
        let combat = match rd.get("IsCombat") {
            None => Some(None),
            Some("True") => Some(Some(true)),
            Some("False") => Some(Some(false)),
            Some(_) => {
                self.block("param:repl DamageDone.IsCombat value");
                None
            }
        };
        Some(Replaces::Damage {
            by: by?,
            to: to?,
            combat: combat?,
        })
    }

    /// What deals the damage: `Card,Emblem` is any source (`None`); `Card.YouCtrl,
    /// Emblem.YouCtrl` a source you control. An emblem alternative is read only where it
    /// mirrors a card alternative: this engine has no emblems (D-073).
    fn damage_source(&mut self, f: &str) -> Option<Option<ObjFilter>> {
        let alts: Vec<&str> = f.split(',').map(str::trim).collect();
        let mut cards = Vec::new();
        for a in &alts {
            match a.strip_prefix("Emblem") {
                Some(rest) => {
                    if !alts.contains(&format!("Card{rest}").as_str()) {
                        self.block(format!("repl:DamageDone source {}", shape(a)));
                        return None;
                    }
                }
                None => cards.push(*a),
            }
        }
        match cards.as_slice() {
            ["Card"] => Some(None),
            _ => self.obj_filter(&cards.join(",")).map(Some),
        }
    }

    /// Who or what is dealt the damage: players (`Player`, `You`, `Opponent`), permanents,
    /// or a player and permanents (`Permanent,Player`; `Opponent,Permanent.OppCtrl`).
    fn damage_recipient(&mut self, f: &str) -> Option<DamageTo> {
        let mut player: Option<Who> = None;
        let mut permanents = Vec::new();
        for a in f.split(',').map(str::trim) {
            let w = match a {
                "Player" => Some(Who::Any),
                "You" | "Player.You" => Some(Who::You),
                "Opponent" | "Player.Opponent" => Some(Who::Opponent),
                _ => None,
            };
            match (w, player) {
                (Some(w), None) => player = Some(w),
                (Some(_), Some(_)) => {
                    self.block("repl:DamageDone two kinds of player");
                    return None;
                }
                (None, _) => permanents.push(a),
            }
        }
        match (player, permanents.as_slice()) {
            (Some(w), []) => Some(DamageTo::Player(w)),
            (Some(Who::Any), ["Permanent"]) => Some(DamageTo::Anything),
            (None, _) => self
                .obj_filter(&permanents.join(","))
                .map(DamageTo::Permanent),
            (Some(player), _) => self
                .obj_filter(&permanents.join(","))
                .map(|permanent| DamageTo::Either { player, permanent }),
        }
    }

    /// `R:Event$ DamageDone | ReplaceWith$ X` with `SVar:X:DB$ ReplaceEffect | VarName$
    /// DamageAmount | VarValue$ Y` and `Y = ReplaceCount$DamageAmount/<math>` (D-073):
    /// "it deals double that damage / that much damage plus 2 instead".
    fn damage_scaled(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        let zone = self.replacement_zone(rd, "DamageDone");
        let replaces = self.damage_replaces(rd);
        rd.get("Secondary");
        let with = rd.get("ReplaceWith").and_then(|n| self.sub(n));
        rd.finish(&mut self.blockers);
        let calc = with.and_then(|w| {
            if w.head() != Some(("DB", "ReplaceEffect")) {
                self.block(format!("repl:DamageDone:{}", w.head().map_or("?", |h| h.1)));
                return None;
            }
            let rd = Rd::new(w, "repl DamageDone ReplaceEffect");
            let name = rd.get("VarName");
            let value = rd.get("VarValue");
            rd.finish(&mut self.blockers);
            match (name, value) {
                (Some("DamageAmount"), Some(v)) => {
                    match v.strip_prefix("ReplaceCount$DamageAmount/") {
                        Some(math) => self.replace_math(math),
                        None => self.replace_count(v, "DamageAmount"),
                    }
                }
                _ => {
                    self.block("repl:DamageDone:ReplaceEffect shape");
                    None
                }
            }
        });
        if !zone {
            return None;
        }
        Some(AbilityDef::Replacement {
            replaces: replaces?,
            with: ReplaceWith::Scale(calc?),
        })
    }

    /// Where a replacement effect of this kind works: a permanent's from the battlefield,
    /// an effect object's from the command zone (D-065). Anything else is a blocker.
    fn replacement_zone(&mut self, rd: &Rd<'a>, what: &str) -> bool {
        let ok = match rd.get("ActiveZones") {
            Some("Battlefield") => !self.in_effect,
            Some("Command") => self.in_effect,
            _ => false,
        };
        if !ok {
            self.block(format!("repl:{what} zone"));
        }
        ok
    }

    /// `ReplaceCount$<key>/<math>` (the SVar `name`): the event's number, changed —
    /// "twice that many", "that many plus one" (D-073).
    fn replace_count(&mut self, name: &str, key: &str) -> Option<Calc> {
        let v = self.svar(name).unwrap_or("");
        let Some(math) = v
            .strip_prefix("ReplaceCount$")
            .and_then(|r| r.strip_prefix(key))
            .and_then(|r| r.strip_prefix('/'))
        else {
            self.block(format!("repl:amount {}", shape(v)));
            return None;
        };
        self.replace_math(math)
    }

    /// Forge's `doXMath` operations that make a number of tokens, counters, damage or life
    /// larger or smaller: `Twice`, `Thrice`, `Plus.N`, `Minus.N`, `HalfUp`, `HalfDown`.
    fn replace_math(&mut self, math: &str) -> Option<Calc> {
        let (op, n) = match math.split_once('.') {
            Some((op, n)) => (op, n.parse::<u32>().ok()),
            None => (math, None),
        };
        let calc = match (op, n) {
            ("Twice", None) => Some(Calc::Times(2)),
            ("Thrice", None) => Some(Calc::Times(3)),
            ("Times", Some(n)) if n >= 2 => Some(Calc::Times(n)),
            ("Plus", Some(n)) if n >= 1 => Some(Calc::Plus(n)),
            ("Minus", Some(n)) if n >= 1 => Some(Calc::Minus(n)),
            ("HalfUp", None) => Some(Calc::HalfUp),
            ("HalfDown", None) => Some(Calc::HalfDown),
            _ => None,
        };
        if calc.is_none() {
            self.block(format!("repl:amount math {}", shape(math)));
        }
        calc
    }

    /// `R:Event$ GainLife | ValidPlayer$ You | ReplaceWith$ X` with `DB$ ReplaceEffect |
    /// VarName$ LifeGained | VarValue$ Y`, `Y = ReplaceCount$LifeGained/<math>` (D-073):
    /// "if you would gain life, you gain twice that much life / that much plus 1 instead".
    fn replace_gain_life(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        let zone = self.replacement_zone(rd, "GainLife");
        let who = match rd.get("ValidPlayer") {
            None | Some("Player") => Some(Who::Any),
            Some("You") => Some(Who::You),
            Some("Opponent" | "Player.Opponent") => Some(Who::Opponent),
            Some(p) => {
                self.block(format!("repl:GainLife.ValidPlayer {}", shape(p)));
                None
            }
        };
        // (A hint for Forge's AI.)
        rd.get("AILogic");
        let with = rd.get("ReplaceWith").and_then(|n| self.sub(n));
        rd.finish(&mut self.blockers);
        let calc = with.and_then(|w| {
            if w.head() != Some(("DB", "ReplaceEffect")) {
                self.block(format!("repl:GainLife:{}", w.head().map_or("?", |h| h.1)));
                return None;
            }
            let rd = Rd::new(w, "repl GainLife ReplaceEffect");
            let name = rd.get("VarName");
            let value = rd.get("VarValue");
            rd.finish(&mut self.blockers);
            match (name, value) {
                (Some("LifeGained"), Some(v)) => match v.strip_prefix("ReplaceCount$LifeGained/") {
                    Some(math) => self.replace_math(math),
                    None => self.replace_count(v, "LifeGained"),
                },
                _ => {
                    self.block("repl:GainLife:ReplaceEffect shape");
                    None
                }
            }
        });
        if !zone {
            return None;
        }
        Some(AbilityDef::Replacement {
            replaces: Replaces::GainLife(who?),
            with: ReplaceWith::Scale(calc?),
        })
    }

    /// `R:Event$ Counter | Layer$ CantHappen` (D-073): "this spell can't be countered"
    /// (`ValidCard$ Card.Self | ValidSA$ Spell`, no zone: it works on the stack) and
    /// "[creature] spells you control can't be countered" (`ActiveZones$ Battlefield` and
    /// `ValidSA$ Spell.<filter>` or `ValidCard$ <filter> | ValidSA$ Spell`).
    fn cant_be_countered(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        let cant = rd.get("Layer") == Some("CantHappen");
        let zone = rd.get("ActiveZones");
        let card = rd.get("ValidCard");
        let sa = rd.get("ValidSA");
        rd.get("Secondary");
        rd.finish(&mut self.blockers);
        if !cant || self.in_effect {
            self.block("repl:Counter:shape");
            return None;
        }
        // `Spell.Creature+YouCtrl` → `Creature.YouCtrl`: a card type first is the head.
        let spell_filter = |f: &str| -> Option<String> {
            let mut alts = Vec::new();
            for alt in f.split(',').map(str::trim) {
                let props = match alt.split_once('.') {
                    Some(("Spell", p)) => p,
                    None if alt == "Spell" => "",
                    _ => return None,
                };
                let props: Vec<&str> = props.split('+').filter(|p| !p.is_empty()).collect();
                alts.push(match props.split_first() {
                    None => "Card".to_string(),
                    Some((first, rest)) if card_type(first).is_some() => {
                        if rest.is_empty() {
                            first.to_string()
                        } else {
                            format!("{first}.{}", rest.join("+"))
                        }
                    }
                    Some(_) => format!("Card.{}", props.join("+")),
                });
            }
            Some(alts.join(","))
        };
        match (zone, card, sa) {
            (None, Some("Card.Self"), Some("Spell")) => {
                Some(AbilityDef::CantBeCountered { spells: None })
            }
            (Some("Battlefield"), None, Some(f)) => match spell_filter(f) {
                Some(f) => self
                    .selector(&f)
                    .map(|s| AbilityDef::CantBeCountered { spells: Some(s) }),
                None => {
                    self.block(format!("repl:Counter:ValidSA {}", shape(f)));
                    None
                }
            },
            (Some("Battlefield"), Some(f), Some("Spell")) if f != "Card.Self" => self
                .selector(f)
                .map(|s| AbilityDef::CantBeCountered { spells: Some(s) }),
            _ => {
                self.block("repl:Counter:shape");
                None
            }
        }
    }

    /// `R:Event$ CreateToken` (D-073): "if an effect would create one or more [creature]
    /// tokens under your control, it creates twice that many / those tokens plus a Food /
    /// that many 4/4 Angels instead" — `DB$ ReplaceToken | Type$ Amount|AddToken|ReplaceToken`
    /// (docs/forge/triggers-statics-replacements-keywords.md §3.3).
    fn replace_create_token(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        let zone = self.replacement_zone(rd, "CreateToken");
        // `ValidPlayer$` is who creates them; in this engine that is who controls them
        // (CR 111.2), which `ValidToken$`'s "YouCtrl" also says.
        let mut token = match rd.get("ValidToken") {
            None => Some(Selector::default()),
            Some(f) => self.selector(f),
        };
        match rd.get("ValidPlayer") {
            None => {}
            Some("You") => {
                if let Some(t) = &mut token {
                    t.controller = Who::You;
                }
            }
            Some(p) => {
                self.block(format!("repl:CreateToken.ValidPlayer {}", shape(p)));
                token = None;
            }
        }
        let effect_only = match rd.get("EffectOnly") {
            None => Some(false),
            Some("True") => Some(true),
            Some(_) => None,
        };
        let with = rd.get("ReplaceWith").and_then(|n| self.sub(n));
        rd.finish(&mut self.blockers);
        let with = with.and_then(|w| self.replace_token_with(w));
        if !zone {
            return None;
        }
        Some(AbilityDef::Replacement {
            replaces: Replaces::Tokens {
                token: token?,
                effect_only: effect_only?,
            },
            with: with?,
        })
    }

    /// `DB$ ReplaceToken | Type$ Amount [| Amount$ <math>]`, `Type$ AddToken | TokenScript$
    /// <scripts> [| Amount$ N]`, `Type$ ReplaceToken | TokenScript$ <scripts>`.
    fn replace_token_with(&mut self, rec: &'a Record) -> Option<ReplaceWith> {
        if rec.head() != Some(("DB", "ReplaceToken")) {
            self.block(format!(
                "repl:CreateToken:{}",
                rec.head().map_or("?", |h| h.1)
            ));
            return None;
        }
        let rd = Rd::new(rec, "repl ReplaceToken");
        let kind = rd.get("Type");
        let scripts = |tx: &mut Self, rd: &Rd<'a>| -> Option<Vec<String>> {
            let Some(s) = rd.get("TokenScript") else {
                tx.block("repl:CreateToken:no TokenScript");
                return None;
            };
            let v: Vec<String> = s.split(',').map(|t| t.trim().to_string()).collect();
            if v.iter().any(|t| t.is_empty() || t == "Chosen") {
                tx.block("repl:CreateToken:chosen token");
                return None;
            }
            tx.tokens.extend(v.iter().cloned());
            Some(v)
        };
        let with = match kind {
            Some("Amount") => {
                let math = rd.get("Amount").unwrap_or("Twice");
                self.replace_math(math).map(ReplaceWith::Scale)
            }
            Some("AddToken") => {
                let n = match rd.get("Amount") {
                    None => Some(None),
                    Some(n) => match n.parse::<u32>() {
                        Ok(n) if n > 0 => Some(Some(n)),
                        _ => {
                            self.block(format!("repl:CreateToken:AddToken amount {}", shape(n)));
                            None
                        }
                    },
                };
                match (scripts(self, &rd), n) {
                    (Some(tokens), Some(n)) => Some(ReplaceWith::AddTokens { tokens, n }),
                    _ => None,
                }
            }
            Some("ReplaceToken") => scripts(self, &rd).map(ReplaceWith::ReplaceTokens),
            other => {
                self.block(format!("repl:CreateToken:Type {}", other.unwrap_or("?")));
                None
            }
        };
        rd.finish(&mut self.blockers);
        with
    }

    /// `R:Event$ AddCounter` (D-073): "if one or more [+1/+1] counters would be put on a
    /// [creature you control], twice that many / that many plus one are put on it instead"
    /// — `DB$ ReplaceCounter | Amount$ X` with `X = ReplaceCount$CounterNum/<math>`.
    fn replace_add_counter(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        let zone = self.replacement_zone(rd, "AddCounter");
        if rd.get("ValidPlayer").is_some() {
            self.block("repl:AddCounter:counters on players");
        }
        // `inZoneBattlefield`: on a permanent — one entering too, as Forge judges the
        // permanent as it would exist there (CR 122.6). Every counter in this engine is on
        // a permanent, so the property changes nothing.
        let on = match rd.get("ValidCard") {
            Some(f) => {
                let alts: Vec<String> = f
                    .split(',')
                    .map(|a| {
                        let (head, props) = a.trim().split_once('.').unwrap_or((a.trim(), ""));
                        let props: Vec<&str> = props
                            .split('+')
                            .filter(|p| !p.is_empty() && *p != "inZoneBattlefield")
                            .collect();
                        if props.is_empty() {
                            head.to_string()
                        } else {
                            format!("{head}.{}", props.join("+"))
                        }
                    })
                    .collect();
                self.selector(&alts.join(","))
            }
            None => {
                self.block("repl:AddCounter:which");
                None
            }
        };
        let kind = match rd.get("ValidCounterType") {
            None => Some(None),
            Some(k) => self.counter_kind(Some(k)).map(Some),
        };
        let effect_only = match rd.get("EffectOnly") {
            None => Some(false),
            Some("True") => Some(true),
            Some(_) => None,
        };
        let with = rd.get("ReplaceWith").and_then(|n| self.sub(n));
        rd.finish(&mut self.blockers);
        let calc = with.and_then(|w| self.replace_counter_with(w, kind.flatten()));
        if !zone {
            return None;
        }
        Some(AbilityDef::Replacement {
            replaces: Replaces::Counters {
                on: on?,
                kind: kind?,
                effect_only: effect_only?,
            },
            with: ReplaceWith::Scale(calc?),
        })
    }

    /// `DB$ ReplaceCounter | Amount$ X [| ValidCounterType$ <the same kind>] [| ChooseCounter$
    /// True]`. (`ChooseCounter`: when counters from several players' effects are put at
    /// once, the affected player picks which the "plus one" is for; an event here comes
    /// from one effect.)
    fn replace_counter_with(&mut self, rec: &'a Record, kind: Option<CounterKind>) -> Option<Calc> {
        if rec.head() != Some(("DB", "ReplaceCounter")) {
            self.block(format!(
                "repl:AddCounter:{}",
                rec.head().map_or("?", |h| h.1)
            ));
            return None;
        }
        let rd = Rd::new(rec, "repl ReplaceCounter");
        let same_kind = match rd.get("ValidCounterType") {
            None => true,
            Some(k) => self.counter_kind(Some(k)).is_some_and(|k| Some(k) == kind),
        };
        rd.get("ChooseCounter");
        let amount = rd.get("Amount");
        rd.finish(&mut self.blockers);
        if !same_kind {
            self.block("repl:AddCounter:counter type differs");
            return None;
        }
        match amount {
            Some(x) => self.replace_count(x, "CounterNum"),
            None => {
                self.block("repl:AddCounter:no amount");
                None
            }
        }
    }

    /// `R:Event$ Moved | Destination$ Battlefield | ReplacementResult$ Updated` — how a
    /// permanent enters (CR 614.1c–d, 614.12): "this permanent enters tapped [unless …]",
    /// "as this enters, you may pay 2 life / reveal … If you don't, it enters tapped",
    /// "[permanents] your opponents control enter tapped", "… enters with counters".
    /// Text: docs/rules-notes/614.12-enters-tapped-unless.md.
    fn moved_enters(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        rd.get("Destination");
        let this = match rd.get("ValidCard") {
            Some("Card.Self") => None,
            Some(f) => Some(self.selector(f)),
            None => {
                self.block("repl:Moved enters:any card");
                Some(None)
            }
        };
        // Its own ability works wherever the card is (Forge: no `ActiveZones`); another
        // permanent's, from the battlefield (CR 614.12).
        let zones_ok = matches!(
            (rd.get("ActiveZones"), &this),
            (None, None) | (Some("Battlefield"), Some(_))
        );
        if !zones_ok {
            self.block("repl:Moved enters:zones");
        }
        // Without `Updated` the move is cancelled and the ability does something else
        // instead ("if this would enter, … instead").
        let updated = rd.get("ReplacementResult") == Some("Updated");
        let with = rd.get("ReplaceWith").and_then(|n| self.sub(n));
        rd.finish(&mut self.blockers);
        if !updated {
            if this.is_none()
                && zones_ok
                && let Some(w) = with
                && w.head() == Some(("DB", "Discard"))
            {
                return self.enters_instead(w).map(|with| AbilityDef::Replacement {
                    replaces: Replaces::ThisEnters,
                    with,
                });
            }
            self.block("repl:Moved enters instead");
            return None;
        }
        let with = with.and_then(|w| self.enters_with_record(w, this.is_none()));
        if !zones_ok || !updated {
            return None;
        }
        let replaces = match this {
            None => Replaces::ThisEnters,
            Some(sel) => Replaces::OtherEnters(sel?),
        };
        Some(AbilityDef::Replacement {
            replaces,
            with: with?,
        })
    }

    /// `R:Event$ Moved | Destination$ Graveyard` whose ability moves the card elsewhere
    /// instead (no `ReplacementResult$`): "if this creature would die, exile it instead",
    /// "if a creature an opponent controls would die, exile it instead" (CR 700.4), "if
    /// this card would be put into a graveyard from anywhere, [reveal it and] shuffle it
    /// into its owner's library instead".
    fn moved_to_graveyard(&mut self, rd: &Rd<'a>) -> Option<AbilityDef> {
        rd.get("Destination");
        let from_battlefield = match rd.get("Origin") {
            None => false,
            Some("Battlefield") => true,
            Some(_) => {
                self.block("repl:Moved to graveyard:origin");
                return None;
            }
        };
        // `ValidLKI$` judges the card as it last existed in its old zone; for one leaving
        // the battlefield that is what `ValidCard$` sees as the event is modified, too.
        let valid = match (rd.get("ValidCard"), rd.get("ValidLKI")) {
            (Some(v), None) | (None, Some(v)) => v,
            _ => {
                self.block("repl:Moved to graveyard:which card");
                return None;
            }
        };
        let zones = rd.get("ActiveZones");
        let replaced = rd.get("ReplacementResult").is_none();
        let with = rd.get("ReplaceWith").and_then(|n| self.sub(n));
        rd.finish(&mut self.blockers);
        let replaces = match (valid, from_battlefield, zones) {
            ("Card.Self", true, Some("Battlefield")) => Some(Replaces::ThisDies),
            // Its own: no zone stated, it works from every zone (Forge: no `ActiveZones`).
            ("Card.Self", false, None) => Some(Replaces::ThisToGraveyard),
            (f, true, Some("Battlefield")) if f != "Card.Self" => {
                self.selector(f).map(Replaces::Dies)
            }
            _ => {
                self.block("repl:Moved to graveyard:shape");
                None
            }
        };
        let with = with.and_then(|w| self.graveyard_instead(w, from_battlefield));
        if !replaced {
            self.block("repl:Moved to graveyard:result");
            return None;
        }
        Some(AbilityDef::Replacement {
            replaces: replaces?,
            with: with?,
        })
    }

    /// `DB$ ChangeZone | Defined$ ReplacedCard | Destination$ Exile` or `… Library |
    /// Shuffle$ True [| Reveal$ True]` (`Origin$ All`, or `Battlefield` for a card leaving
    /// it; `Hidden$ True` changes nothing for a defined card).
    fn graveyard_instead(
        &mut self,
        rec: &'a Record,
        from_battlefield: bool,
    ) -> Option<ReplaceWith> {
        if rec.head() != Some(("DB", "ChangeZone")) {
            self.block(format!(
                "repl:Moved to graveyard:{}",
                rec.head().map_or("?", |h| h.1)
            ));
            return None;
        }
        let rd = Rd::new(rec, "repl Moved ChangeZone");
        let origin = matches!(
            (rd.get("Origin"), from_battlefield),
            (Some("All"), _) | (Some("Battlefield"), true)
        );
        let defined = rd.get("Defined") == Some("ReplacedCard");
        let hidden = matches!(rd.get("Hidden"), None | Some("True"));
        let with = match (rd.get("Destination"), rd.get("Shuffle"), rd.get("Reveal")) {
            (Some("Exile"), None, None) => Some(ReplaceWith::Exile),
            (Some("Library"), Some("True"), None) => Some(ReplaceWith::ShuffleIntoLibrary),
            (Some("Library"), Some("True"), Some("True")) => {
                Some(ReplaceWith::RevealShuffleIntoLibrary)
            }
            _ => None,
        };
        if rd.get("SubAbility").is_some() {
            self.block("repl:Moved to graveyard:ChangeZone+SubAbility");
        }
        rd.finish(&mut self.blockers);
        if !origin || !defined || !hidden || with.is_none() {
            self.block("repl:Moved to graveyard:ChangeZone shape");
            return None;
        }
        with
    }

    /// "If this would enter, you may discard a [land] card instead. If you do, put it onto
    /// the battlefield. If you don't, put it into its owner's graveyard." Forge writes it as
    /// one fixed chain: discard (optional, remembered), then move the card to the graveyard
    /// if nothing was remembered, or to the battlefield if one was, then forget.
    fn enters_instead(&mut self, rec: &'a Record) -> Option<ReplaceWith> {
        let rd = Rd::new(rec, "repl Moved instead Discard");
        let valid = rd.get("DiscardValid");
        let shape = rd.get("Mode") == Some("TgtChoose")
            && rd.get("RememberDiscarded") == Some("True")
            && rd.get("Optional") == Some("True");
        rd.get("AILogic");
        let to_graveyard = rd.get("SubAbility").and_then(|n| self.sub(n));
        rd.finish(&mut self.blockers);
        let tail = to_graveyard.is_some_and(|r| self.instead_tail(r));
        if !shape || !tail {
            self.block("repl:Moved enters instead:shape");
            return None;
        }
        let sel = self.card_selector(valid?)?;
        Some(ReplaceWith::EntersOnlyIfYou(AsEnters::Discard(sel)))
    }

    /// The rest of [`Tx::enters_instead`]'s chain, exactly.
    fn instead_tail(&mut self, to_graveyard: &'a Record) -> bool {
        let (Some(x), Some(next)) = (
            to_graveyard.get("ConditionCheckSVar"),
            to_graveyard.get("SubAbility"),
        ) else {
            return false;
        };
        let moved = |dest: &str, cmp: &str| -> Vec<(String, String)> {
            [
                ("DB", "ChangeZone"),
                ("Origin", "All"),
                ("Destination", dest),
                ("Defined", "ReplacedCard"),
                ("ConditionCheckSVar", x),
                ("ConditionSVarCompare", cmp),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
        };
        let same = |rec: &Record, mut want: Vec<(String, String)>| {
            let mut have = rec.params.clone();
            have.sort();
            want.sort();
            have == want
        };
        let mut want = moved("Graveyard", "EQ0");
        want.push(("SubAbility".into(), next.to_string()));
        if !same(to_graveyard, want) {
            return false;
        }
        let Some(to_battlefield) = self.sub(next) else {
            return false;
        };
        let mut want = moved("Battlefield", "EQ1");
        if let Some(cleanup) = to_battlefield.get("SubAbility") {
            want.push(("SubAbility".into(), cleanup.to_string()));
            let Some(c) = self.sub(cleanup) else {
                return false;
            };
            if !same(
                c,
                vec![
                    ("DB".into(), "Cleanup".into()),
                    ("ClearRemembered".into(), "True".into()),
                ],
            ) {
                return false;
            }
        }
        same(to_battlefield, want) && self.svar(x) == Some("Remembered$Amount")
    }

    /// The `ReplaceWith$` ability of an "enters" replacement: `DB$ Tap | ETB$ True` (with
    /// a condition or an "unless" cost) or `DB$ PutCounter | ETB$ True`, on `Self` (its own
    /// entering, `own`) or `ReplacedCard` (another permanent's).
    fn enters_with_record(&mut self, rec: &'a Record, own: bool) -> Option<ReplaceWith> {
        let api = rec.head().map(|h| h.1).unwrap_or("?");
        if !matches!(rec.head(), Some(("DB", "Tap" | "PutCounter"))) {
            self.block(format!("repl:Moved enters:{api}"));
            return None;
        }
        let rd = Rd::new(rec, format!("repl Moved {api}"));
        let etb = rd.get("ETB") == Some("True");
        let whose = matches!(
            (own, rd.get("Defined")),
            // (An effect on a card is about the card itself unless it says otherwise.)
            (true, Some("Self") | None) | (false, Some("ReplacedCard"))
        );
        if rd.get("SubAbility").is_some() {
            self.block(format!("repl:Moved enters:{api}+SubAbility"));
        }
        let with = if api == "Tap" {
            self.enters_tapped(&rd, own)
        } else {
            let kind = self.counter_kind(rd.get("CounterType"));
            let amount = self.counted(rd.get("CounterNum").unwrap_or("1"), own && self.has_x_cost);
            match (kind, amount) {
                (Some(kind), Some(amount)) => Some(Self::enters_with(kind, amount)),
                _ => None,
            }
        };
        rd.finish(&mut self.blockers);
        if !etb || !whose {
            self.block(format!("repl:Moved enters:{api} not on the entering card"));
            return None;
        }
        with
    }

    /// "Enters tapped", "… unless [condition]" (`ConditionPresent$`/`ConditionCompare$`,
    /// `ConditionCheckSVar$`/`ConditionSVarCompare$`: when it is tapped), "… unless you
    /// [pay N life / reveal a … card]" (`UnlessCost$`). Only a permanent's own entering
    /// may have a condition or a choice ("you" is whoever it enters under the control of).
    fn enters_tapped(&mut self, rd: &Rd<'a>, own: bool) -> Option<ReplaceWith> {
        // When the tapping happens: an amount compared with a number.
        let tap_if = match (rd.get("ConditionPresent"), rd.get("ConditionCheckSVar")) {
            (None, None) => None,
            (Some(filter), None) => {
                let cmp = self.compare(rd.get("ConditionCompare").unwrap_or("GE1"));
                let sel = self.selector(filter);
                Some((Amount::Count(sel?), cmp?))
            }
            (None, Some(name)) => {
                let cmp = self.compare(rd.get("ConditionSVarCompare").unwrap_or("GE1"));
                let amount = match name.parse::<u32>() {
                    Ok(n) => Some(Amount::Fixed(n)),
                    Err(_) => self.expr(name),
                };
                let (amount, cmp) = (amount?, cmp?);
                if !amount.stands_alone() {
                    self.block("condition:amount needs the ability");
                    return None;
                }
                Some((amount, cmp))
            }
            (Some(_), Some(_)) => {
                self.block("condition:two");
                return None;
            }
        };
        let unless = match (rd.get("UnlessCost"), rd.get("UnlessPayer")) {
            (None, None) => None,
            (Some(cost), Some("You")) => Some(self.as_enters_cost(cost)?),
            _ => {
                self.block("repl:Moved enters:unless payer");
                return None;
            }
        };
        if !own && (tap_if.is_some() || unless.is_some()) {
            self.block("repl:Moved enters:others unless");
            return None;
        }
        match (tap_if, unless) {
            (None, None) => Some(ReplaceWith::EntersTapped),
            (None, Some(act)) => Some(ReplaceWith::EntersTappedUnlessYou(act)),
            (Some((amount, (cmp, n))), None) => {
                // Tapped if [amount cmp n]: untapped exactly when the opposite holds.
                let Some((cmp, n)) = negate(cmp, n) else {
                    self.block("condition:never or always");
                    return None;
                };
                Some(ReplaceWith::EntersTappedUnless(match (amount, cmp, n) {
                    (Amount::Count(what), Cmp::AtLeast, n) => {
                        Condition::Control { what, at_least: n }
                    }
                    // "Not none" is "at least one".
                    (Amount::Count(what), Cmp::Not, 0) => Condition::Control { what, at_least: 1 },
                    (Amount::Count(what), Cmp::AtMost, _)
                    | (Amount::Count(what), Cmp::Exactly, 0) => {
                        Condition::ControlAtMost { what, at_most: n }
                    }
                    (amount, cmp, n) => Condition::Compare { amount, cmp, n },
                }))
            }
            (Some(_), Some(_)) => {
                self.block("repl:Moved enters:condition and unless");
                None
            }
        }
    }

    /// `PayLife<N>` or `Reveal<1/<types>[/<text>]>` (card types or subtypes separated by
    /// `;`, any one of them).
    fn as_enters_cost(&mut self, cost: &str) -> Option<AsEnters> {
        if let Some(n) = cost
            .strip_prefix("PayLife<")
            .and_then(|r| r.strip_suffix('>'))
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|n| *n > 0)
        {
            return Some(AsEnters::PayLife(n));
        }
        let reveal = cost
            .strip_prefix("Reveal<1/")
            .and_then(|r| r.strip_suffix('>'))
            .map(|r| r.split('/').next().unwrap_or(""));
        if let Some(types) = reveal
            && !types.is_empty()
            && !types.contains(',')
            // Pseudo-types: the whole hand, cards sharing a color, the card itself.
            && !matches!(types, "Hand" | "SameColor" | "CARDNAME" | "NICKNAME")
        {
            let filter = types.split(';').collect::<Vec<_>>().join(",");
            return self.card_selector(&filter).map(AsEnters::Reveal);
        }
        self.block(format!("repl:Moved enters:unless {}", shape(cost)));
        None
    }
}

/// The comparison that holds exactly when `cmp n` doesn't; `None` if `cmp n` always or
/// never holds.
/// A step of Forge's `Phase$` (docs/forge/triggers-statics-replacements-keywords.md).
fn phase_step(name: &str) -> Option<TriggerStep> {
    match name {
        "Upkeep" => Some(TriggerStep::Upkeep),
        "Draw" => Some(TriggerStep::DrawStep),
        // CR 513.1a: "at end of turn" is "at the beginning of the end step".
        "End of Turn" | "End Of Turn" => Some(TriggerStep::EndStep),
        "BeginCombat" => Some(TriggerStep::BeginCombat),
        "EndCombat" => Some(TriggerStep::EndCombat),
        "Main1" => Some(TriggerStep::PrecombatMain),
        "Main2" => Some(TriggerStep::PostcombatMain),
        _ => None,
    }
}

fn negate(cmp: Cmp, n: u32) -> Option<(Cmp, u32)> {
    match cmp {
        Cmp::AtLeast => Some((Cmp::AtMost, n.checked_sub(1)?)),
        Cmp::AtMost => Some((Cmp::AtLeast, n.checked_add(1)?)),
        Cmp::Exactly => Some((Cmp::Not, n)),
        Cmp::Not => Some((Cmp::Exactly, n)),
    }
}

/// Several selectors that differ only in one "at least one of" list — card type, subtype
/// or color — each naming one, are one selector: "Island.YouCtrl,Mountain.YouCtrl" is
/// "an Island or a Mountain you control" (X∧a ∨ X∧b = X∧(a∨b)).
fn merge(mut sels: Vec<Selector>) -> Option<Selector> {
    if sels.len() == 1 {
        return sels.pop();
    }
    merge_on(&sels, |s| &mut s.types)
        .or_else(|| merge_on(&sels, |s| &mut s.subtypes))
        .or_else(|| merge_on(&sels, |s| &mut s.colors))
}

/// [`merge`] over one list of the selector.
fn merge_on<T: Clone + PartialEq>(
    sels: &[Selector],
    field: fn(&mut Selector) -> &mut Vec<T>,
) -> Option<Selector> {
    let mut rest: Option<Selector> = None;
    let mut any_of: Vec<T> = Vec::new();
    for s in sels {
        let mut bare = s.clone();
        let v = std::mem::take(field(&mut bare));
        let [one] = v.as_slice() else {
            return None;
        };
        match &rest {
            None => rest = Some(bare),
            Some(r) if *r == bare => {}
            Some(_) => return None,
        }
        if !any_of.contains(one) {
            any_of.push(one.clone());
        }
    }
    let mut out = rest?;
    *field(&mut out) = any_of;
    Some(out)
}

/// A list of alternatives with those that are just a card type ("Artifact", "Enchantment")
/// folded into one selector, placed where the first of them was.
fn merge_bare(sels: Vec<Selector>) -> Vec<Selector> {
    let bare = |s: &Selector| {
        s.types.len() == 1
            && Selector {
                types: Vec::new(),
                ..s.clone()
            } == Selector::default()
    };
    let mut out: Vec<Selector> = Vec::new();
    let mut folded: Option<usize> = None;
    for s in sels {
        match (bare(&s), folded) {
            (true, Some(i)) => out[i].types.extend(s.types),
            (true, None) => {
                folded = Some(out.len());
                out.push(s);
            }
            (false, _) => out.push(s),
        }
    }
    out
}

/// An expression with its numbers and filters blanked, for grouping blockers.
fn shape(v: &str) -> String {
    let head: String = v
        .chars()
        .map(|c| if c.is_ascii_digit() { 'N' } else { c })
        .collect();
    let head = head.split('/').next().unwrap_or("").to_string();
    match head.split_once("Valid ") {
        Some((a, _)) => format!("{a}Valid <filter>"),
        None => head.chars().take(40).collect(),
    }
}

/// Lines of script variables that hold abilities, parsed once.
fn ability_records(face: &Face) -> Vec<(String, Record)> {
    face.svars
        .iter()
        .filter(|(_, v)| v.contains('$') && (v.contains('|') || v.starts_with("DB$")))
        .map(|(n, v)| (n.clone(), Record::parse(v)))
        .collect()
}

/// Scripts found, by reading the compiled card against its Oracle text, to say something
/// other than the card does. The compiler can't see that, so they are named here and
/// never compile (invariant 8: a wrong card is worse than a missing one). Each entry says
/// what differs; remove it when the pinned Forge commit has the script corrected.
const SCRIPT_DIFFERS: &[(&str, &str)] = &[
    (
        "Knight of the Mists",
        "destroys target Knight *creature*; the card says \"target Knight\" (a Kindred Knight \
         permanent that isn't a creature is a legal target, CR 308.2)",
    ),
    (
        "Swarmyard Massacre",
        "counts Insects, Rats, Spiders and Squirrels you control; the card counts *creatures* \
         you control of those types (a noncreature permanent of such a type must not count)",
    ),
    (
        "Clash of Realities",
        "gives its trigger to Spirit *creatures*; the card says \"all Spirits\" (a noncreature \
         Kindred Spirit must get it too)",
    ),
    (
        "For the Ancestors",
        "takes creature cards of the chosen type; the card says \"cards of the chosen type\" (a \
         Kindred card of that type may be taken too)",
    ),
];

/// Script variables that Forge's own code sets while a spell or ability resolves
/// (`sa.setSVar(…)`): the value written in the script is only a placeholder
/// (`SVar:AFLifeLost:Number$0`). Reading one by its written value would be wrong, so each
/// is either compiled by its meaning or a blocker.
const SET_WHILE_RESOLVING: &[&str] = &[
    "AFLifeLost",
    "PreventedDamage",
    "CostCountersRemoved",
    "Votes",
    "VoteNum",
    "Wins",
    "Losses",
    "ShieldAmount",
    "AffectedX",
    "DifferentResults",
    "Doubles",
    "EvenResults",
    "OddResults",
    "MaxRolls",
    "Revealed",
    "TimesFlipped",
    "ModsThisTurn",
    "CasualtyPaid",
    "Sunburst",
    "StormCount",
    "GravestormCount",
    "PhySpent",
    "ReplicateAmount",
    "SquadAmount",
    "RampageCount",
    "CharmOrder",
    "HoneCounters",
    "NumTurns",
    "Dies",
    "MoveToHand",
];

/// Compile a script. `Err` lists every blocker found (not just the first), on every face.
pub fn compile(script: &Script, known: &Known) -> Result<Compiled, Blockers> {
    // How the faces relate (docs/forge/script-format.md §1.2). Only the layouts the engine
    // has rules for compile; each face is then compiled as a card of its own.
    let layout = match (script.alternate_mode.as_deref(), script.faces.len()) {
        (None, 1) => Some(Layout::Normal),
        (Some("Modal"), 2) => Some(Layout::Modal),
        (Some("DoubleFaced"), 2) => Some(Layout::Transform),
        (Some("Adventure"), 2) => Some(Layout::Adventure),
        (Some("Omen"), 2) => Some(Layout::Omen),
        (Some("Split"), 2) => Some(Layout::Split),
        _ => None,
    };
    let Some(layout) = layout else {
        return Err(Blockers::from([format!(
            "layout:{}",
            script.alternate_mode.as_deref().unwrap_or("multi-face")
        )]));
    };
    let mut blockers = Blockers::new();
    let mut tokens = BTreeSet::new();
    let mut faces = Vec::new();
    for face in &script.faces {
        match compile_face(face, known) {
            Ok(c) => {
                tokens.extend(c.tokens);
                faces.push(c.def);
            }
            Err(b) => blockers.extend(b),
        }
    }
    if !blockers.is_empty() {
        return Err(blockers);
    }
    if layout == Layout::Split {
        return split_card(faces).map(|def| Compiled { def, tokens });
    }
    let mut def = faces.remove(0);
    def.layout = layout;
    def.faces = faces;
    Ok(Compiled { def, tokens })
}

/// A split card (CR 709): the two halves, and the card they make combined (CR 709.4) —
/// what it is everywhere but on the stack. Only halves that are instants or sorceries with
/// nothing but their spell ability (and an additional cost to cast it): an ability that
/// works from a hand or graveyard would be the whole card's, and a Room's halves are
/// permanents with rules of their own (CR 709.5).
fn split_card(halves: Vec<CardDef>) -> Result<CardDef, Blockers> {
    let mut blockers = Blockers::new();
    for h in &halves {
        if h.creature_types.iter().any(|t| t == "Room") {
            blockers.insert("split:Room".to_string());
        } else if !h
            .card_types
            .iter()
            .all(|t| matches!(t, CardType::Instant | CardType::Sorcery))
        {
            blockers.insert("split:permanent half".to_string());
        }
        if !h.keywords.is_empty()
            || !h.granted.is_empty()
            || h.abilities
                .iter()
                .any(|a| !matches!(a, AbilityDef::Spell(_) | AbilityDef::AdditionalCost(_)))
        {
            blockers.insert("split:half with other abilities".to_string());
        }
    }
    if !blockers.is_empty() {
        return Err(blockers);
    }
    let union = |f: &dyn Fn(&CardDef) -> Vec<String>| {
        let mut out: Vec<String> = Vec::new();
        for h in &halves {
            for x in f(h) {
                if !out.contains(&x) {
                    out.push(x);
                }
            }
        }
        out
    };
    let mut card_types: Vec<CardType> = Vec::new();
    let mut supertypes: Vec<Supertype> = Vec::new();
    for h in &halves {
        for t in &h.card_types {
            if !card_types.contains(t) {
                card_types.push(*t);
            }
        }
        for t in &h.supertypes {
            if !supertypes.contains(t) {
                supertypes.push(*t);
            }
        }
    }
    // CR 709.4b: the combined mana cost; the colors come from it (or from the halves'
    // color indicators).
    let mana_cost = ManaCost {
        symbols: halves
            .iter()
            .flat_map(|h| h.mana_cost.iter().flat_map(|m| m.symbols.clone()))
            .collect(),
    };
    let colors = halves.iter().any(|h| h.colors.is_some()).then(|| {
        let mut c: Vec<Color> = halves.iter().flat_map(|h| h.printed_colors()).collect();
        c.sort();
        c.dedup();
        c
    });
    Ok(CardDef {
        // CR 709.4a: both names.
        name: union(&|h| vec![h.name.clone()]).join(" // "),
        mana_cost: Some(mana_cost),
        supertypes,
        card_types,
        creature_types: union(&|h| h.creature_types.clone()),
        colors,
        layout: Layout::Split,
        faces: halves,
        ..CardDef::default()
    })
}

/// Compile one face of a script as a card of its own.
fn compile_face(face: &Face, known: &Known) -> Result<Compiled, Blockers> {
    let early = Blockers::new();
    let parsed = ability_records(face);
    let mut tx = Tx {
        face,
        blockers: Raised {
            raised: early.len(),
            set: early,
        },
        used_svars: BTreeSet::new(),
        tokens: BTreeSet::new(),
        has_x_cost: false,
        spell_cost: false,
        defines_pt: (false, false),
        in_trigger: false,
        from_zone: ActivationZone::Battlefield,
        trig: TrigCtx::default(),
        optional_trigger: false,
        in_spell: false,
        additional: Vec::new(),
        enchant: None,
        equipment: face.types.iter().any(|t| t == "Equipment"),
        records: &parsed,
        known,
        granted: Vec::new(),
        granting: 0,
        in_effect: false,
        chain_targets: None,
        read_target: false,
        it_is_you: BTreeSet::new(),
        took_it_for_this: false,
        player_is_you: BTreeSet::new(),
        took_player_for_you: false,
        nothing: false,
        memory: false,
        delayed_noted: None,
        in_delayed: false,
    };
    let mut def = CardDef {
        name: face.name.clone(),
        ..CardDef::default()
    };
    for line in &face.unknown {
        tx.block(format!("line:{}", line.split(':').next().unwrap_or("?")));
    }
    // Comments (D-078): reviewed ones are ignored, as Forge does; one that says the script
    // is incomplete, or one nobody has read, blocks.
    for b in super::comments::blockers(&face.comments) {
        tx.block(b);
    }
    if SCRIPT_DIFFERS.iter().any(|(name, _)| *name == face.name) {
        tx.block("script:differs from the card");
    }
    for (what, present) in [
        ("line:Loyalty", face.loyalty.is_some()),
        ("line:Defense", face.defense.is_some()),
        ("line:Text", face.text.is_some()),
    ] {
        if present {
            tx.block(what);
        }
    }
    // Printed characteristics.
    match mana_cost(face.mana_cost.as_deref().unwrap_or("no cost")) {
        Ok(c) => {
            tx.has_x_cost = c.as_ref().is_some_and(ManaCost::has_x);
            // {X} in the cost is announced by the caster — unless the script defines X
            // as something else ("X is the target's mana value": the cost then follows
            // from the target). That isn't an announced X: not compiled.
            if tx.has_x_cost && face.svars.get("X").is_some_and(|v| v != "Count$xPaid") {
                tx.block("cost:X defined by the script");
            }
            def.mana_cost = c;
        }
        Err(b) => tx.block(b),
    }
    for word in &face.types {
        if let Some(t) = card_type(word) {
            def.card_types.push(t);
            continue;
        }
        match word.as_str() {
            "Basic" => def.supertypes.push(Supertype::Basic),
            "Legendary" => def.supertypes.push(Supertype::Legendary),
            "Snow" => def.supertypes.push(Supertype::Snow),
            "World" => def.supertypes.push(Supertype::World),
            // Basic land types on a land carry its mana ability (CR 305.6); every other
            // subtype, a land's included ("Desert", "Gate"), is kept by name.
            "Plains" | "Island" | "Swamp" | "Mountain" | "Forest"
                if def.card_types.contains(&CardType::Land) =>
            {
                def.land_types.push(match word.as_str() {
                    "Plains" => BasicLandType::Plains,
                    "Island" => BasicLandType::Island,
                    "Swamp" => BasicLandType::Swamp,
                    "Mountain" => BasicLandType::Mountain,
                    _ => BasicLandType::Forest,
                })
            }
            "Ongoing" => def.supertypes.push(Supertype::Ongoing),
            other => def.creature_types.push(other.to_string()),
        }
    }
    // "*" (also "1+*"): defined by a characteristic-defining ability, checked once the
    // abilities are compiled. Until then — and in a zone-less context — it is 0 (CR 208.2a).
    let mut starred = (false, false);
    if let Some((p, t)) = &face.pt {
        let value = |text: &str, starred: &mut bool| match text.parse::<i32>() {
            Ok(n) => Some(n),
            Err(_) if text.contains('*') => {
                *starred = true;
                Some(0)
            }
            Err(_) => None,
        };
        match (value(p, &mut starred.0), value(t, &mut starred.1)) {
            (Some(p), Some(t)) => {
                def.power = Some(p);
                def.toughness = Some(t);
            }
            _ => tx.block("pt:variable"),
        }
    }
    if let Some(colors) = &face.colors {
        let mut cs = Vec::new();
        for c in colors.split(',') {
            let mut w = c.trim().to_string();
            if let Some(f) = w.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            match color(&w) {
                Some(c) => cs.push(c),
                None => tx.block(format!("colors:{c}")),
            }
        }
        cs.sort();
        def.colors = Some(cs);
    }
    // Abilities, in the order the engine's own scripts use: as written, line kind by line
    // kind (the order within a kind is the script's).
    let mut abilities: Vec<(usize, AbilityDef)> = Vec::new();
    // Equip abilities are printed last.
    let mut equips: Vec<AbilityDef> = Vec::new();
    for k in &face.keywords {
        // `Enchant:<what>[:<description>]` (CR 702.5): what the Aura targets as a spell and
        // may be attached to. It is printed first.
        if let Some(rest) = k.strip_prefix("Enchant:") {
            let what = rest.split(':').next().unwrap_or(rest).trim();
            let filter = match what {
                _ if tx.enchant.is_some() => None,
                "Creature" => Some(TargetFilter::Creature),
                // Players can't be enchanted yet.
                "Player" | "Opponent" | "Player.Opponent" => None,
                // "Enchant land", "enchant creature you control", "enchant permanent".
                _ => {
                    let saved = tx.blockers.clone();
                    let sels: Option<Vec<Selector>> = tx.alts(what).and_then(|alts| {
                        alts.into_iter()
                            .map(|a| match a {
                                Alt::Sel(s) => Some(s),
                                Alt::This => None,
                            })
                            .collect()
                    });
                    // The filter's own blockers say less than "this Aura's enchant".
                    tx.blockers = saved;
                    sels.map(|s| TargetFilter::Permanent(merge_bare(s)))
                }
            };
            match filter {
                Some(f) => {
                    tx.enchant = Some(what);
                    abilities.push((0, AbilityDef::Enchant(f)));
                }
                None => {
                    tx.block(format!("enchant:{what}"));
                    tx.enchant.get_or_insert(what);
                }
            }
            continue;
        }
        // `Equip:<cost>` (CR 702.6a): "[cost]: Attach this permanent to target creature you
        // control. Activate only as a sorcery." Restricted equips ("equip legendary
        // creature") and extra parameters are not compiled.
        if let Some(rest) = k.strip_prefix("Equip:") {
            let cost = if rest.contains(':') {
                tx.block("kw:Equip-restricted");
                None
            } else {
                tx.cost(rest)
            };
            if let Some(cost) = cost {
                equips.push(AbilityDef::Activated {
                    from: ActivationZone::Battlefield,
                    cycling: false,
                    reduce: None,
                    cost,
                    timing: Timing::Sorcery,
                    only_if: None,
                    body: Body::Effect(Mode {
                        targets: vec![TargetFilter::Permanent(vec![Selector {
                            types: vec![CardType::Creature],
                            controller: Who::You,
                            ..Selector::default()
                        }])],
                        effect: Effect::AttachThis {
                            to: ObjRef::Target(0),
                        },
                        groups: Vec::new(),
                    }),
                });
            }
            continue;
        }
        // Keywords that are shorthand for an ability the engine has.
        let pump_one = |what: ObjRef| {
            Body::Effect(Mode {
                targets: Vec::new(),
                effect: Effect::Apply {
                    what,
                    effects: vec![ContinuousEffect::ModifyPT(1, 1)],
                    until_end_of_turn: true,
                },
                groups: Vec::new(),
            })
        };
        match k.as_str() {
            // CR 702.108a: "Whenever you cast a noncreature spell, this creature gets
            // +1/+1 until end of turn."
            "Prowess" => {
                abilities.push((
                    0,
                    AbilityDef::Triggered {
                        when: Trigger::Cast {
                            by: Who::You,
                            types: Vec::new(),
                            spells: vec![Selector {
                                not_types: vec![CardType::Creature],
                                ..Selector::default()
                            }],
                            nth: None,
                        },
                        intervening_if: None,
                        body: pump_one(ObjRef::This),
                    },
                ));
                continue;
            }
            // CR 702.83a: "Whenever a creature you control attacks alone, that creature
            // gets +1/+1 until end of turn."
            "Exalted" => {
                abilities.push((
                    0,
                    AbilityDef::Triggered {
                        when: Trigger::AttacksAlone(ObjFilter::Matching(Selector {
                            types: vec![CardType::Creature],
                            controller: Who::You,
                            ..Selector::default()
                        })),
                        intervening_if: None,
                        body: pump_one(ObjRef::Triggering),
                    },
                ));
                continue;
            }
            // CR 702.25a: "Whenever this creature becomes blocked by a creature without
            // flanking, the blocking creature gets -1/-1 until end of turn." The keyword
            // itself only marks who has flanking; it can't be granted on its own (each
            // instance triggers separately), so nothing else compiles to it.
            "Flanking" => {
                def.keywords.push(Keyword::Flanking);
                abilities.push((
                    0,
                    AbilityDef::Triggered {
                        when: Trigger::BlockedBy {
                            attacker: ObjFilter::This,
                            blocker: ObjFilter::Matching(Selector {
                                types: vec![CardType::Creature],
                                not_keywords: vec![Keyword::Flanking],
                                ..Selector::default()
                            }),
                        },
                        intervening_if: None,
                        body: Body::Effect(Mode {
                            targets: Vec::new(),
                            effect: Effect::Apply {
                                what: ObjRef::Triggering,
                                effects: vec![ContinuousEffect::ModifyPT(-1, -1)],
                                until_end_of_turn: true,
                            },
                            groups: Vec::new(),
                        }),
                    },
                ));
                continue;
            }
            // CR 702.114a: "This object is colorless", everywhere: the same as having no
            // color to begin with (its color identity still comes from its mana cost).
            "Devoid" => {
                def.colors = Some(Vec::new());
                continue;
            }
            _ => {}
        }
        // CR 702.45a: "Bushido N": "Whenever this creature blocks or becomes blocked, it
        // gets +N/+N until end of turn."
        if let Some(n) = k.strip_prefix("Bushido:") {
            match n.parse::<i32>() {
                Ok(n) => abilities.push((
                    0,
                    AbilityDef::Triggered {
                        when: Trigger::Any(vec![
                            Trigger::Blocks(ObjFilter::This),
                            Trigger::BecomesBlocked(ObjFilter::This),
                        ]),
                        intervening_if: None,
                        body: Body::Effect(Mode {
                            targets: Vec::new(),
                            effect: Effect::Apply {
                                what: ObjRef::This,
                                effects: vec![ContinuousEffect::ModifyPT(n, n)],
                                until_end_of_turn: true,
                            },
                            groups: Vec::new(),
                        }),
                    },
                )),
                Err(_) => tx.block("kw:Bushido amount"),
            }
            continue;
        }
        // CR 702.41a: "Affinity for [things]": "This spell costs {1} less to cast for each
        // [thing] you control."
        if let Some(rest) = k.strip_prefix("Affinity:") {
            let what = rest.split(':').next().unwrap_or(rest);
            let before = tx.blockers.len();
            match tx.selector(what) {
                Some(mut s) if s.controller == Who::Any && s.owner == Who::Any => {
                    s.controller = Who::You;
                    abilities.push((0, AbilityDef::SelfCostReduction(Amount::Count(s))));
                }
                Some(_) => tx.block("kw:Affinity for something controlled"),
                None => tx.explained(before, "keyword"),
            }
            continue;
        }
        // `Cycling:<cost>` (CR 702.29a): "[cost], discard this card: Draw a card", from the
        // hand. `TypeCycling:<type>:<cost>[:<words>]` (CR 702.29e): "… Search your library
        // for a [type] card, reveal it, put it into your hand, then shuffle."
        let cycling = k
            .strip_prefix("Cycling:")
            .map(|cost| (None, cost))
            .or_else(|| {
                let mut parts = k.strip_prefix("TypeCycling:")?.splitn(3, ':');
                Some((Some(parts.next()?), parts.next()?))
            });
        if let Some((what, cost)) = cycling {
            let before = tx.blockers.len();
            let cost = tx.cost(cost).map(|c| Cost {
                discard_self: true,
                ..c
            });
            let effect = match what {
                None => Some(Effect::Draw {
                    who: PlayerRef::You,
                    amount: Amount::Fixed(1),
                }),
                Some(what) => {
                    // "Basic landcycling" is written `Basic`.
                    let cards = match what {
                        "Basic" => Some(vec![Selector {
                            types: vec![CardType::Land],
                            supertypes: vec![Supertype::Basic],
                            ..Selector::default()
                        }]),
                        other => tx.card_alts(other, true),
                    };
                    cards.map(|cards| Effect::ChooseCards {
                        who: PlayerRef::You,
                        from: CardZone::Library,
                        cards,
                        count: Amount::Fixed(1),
                        must: false,
                        to: CardDest::Hand,
                        reveal: true,
                        shuffle: true,
                        optional: false,
                        share_land_type: false,
                    })
                }
            };
            match (cost, effect) {
                (Some(cost), Some(effect)) if cost.discard.is_none() => {
                    abilities.push((
                        0,
                        AbilityDef::Activated {
                            cost,
                            timing: Timing::Instant,
                            only_if: None,
                            body: Body::Effect(Mode {
                                targets: Vec::new(),
                                effect,
                                groups: Vec::new(),
                            }),
                            from: ActivationZone::Hand,
                            cycling: true,
                            reduce: None,
                        },
                    ));
                }
                _ => tx.explained(before, "keyword"),
            }
            continue;
        }
        // Alternative costs written as keywords, printed last (CR 702.34a flashback, 702.74a
        // evoke, 702.96a overload): `K:<name>:<cost>`, nothing after the cost.
        if let Some((name, text)) = k.split_once(':')
            && matches!(name, "Flashback" | "Evoke" | "Overload")
        {
            let before = tx.blockers.len();
            if text.contains(':') {
                tx.block(format!("kw:{name} with parameters"));
                continue;
            }
            let cost = tx.alt_cost_of(text);
            let kind = match name {
                "Flashback" => Some(AltKind::Flashback),
                "Evoke" => Some(AltKind::Evoke),
                _ => tx.overloaded().map(AltKind::Overload),
            };
            match (cost, kind) {
                (Some(cost), Some(kind)) => {
                    // "When this permanent enters, if its evoke cost was paid, its
                    // controller sacrifices it."
                    if matches!(kind, AltKind::Evoke) {
                        equips.push(AbilityDef::Triggered {
                            when: Trigger::Enters(ObjFilter::This),
                            intervening_if: Some(Condition::CastFor(AltKindName::Evoke)),
                            body: Body::Effect(Mode {
                                targets: Vec::new(),
                                effect: Effect::ControllerSacrificesThis,
                                groups: Vec::new(),
                            }),
                        });
                    }
                    equips.push(AbilityDef::AlternativeCost(AltCost {
                        kind,
                        cost,
                        only_if: None,
                    }));
                }
                _ => tx.explained(before, "keyword"),
            }
            continue;
        }
        if k.starts_with("etbCounter:") || k.starts_with("ETBReplacement:") {
            let before = tx.blockers.len();
            let a = if k.starts_with("etbCounter:") {
                tx.etb_counter(k)
            } else {
                tx.etb_replacement(k)
            };
            match a {
                Some(a) => abilities.push((0, a)),
                None => tx.explained(before, "keyword"),
            }
            continue;
        }
        // "This creature can't block.", "Prevent all damage that would be dealt to this
        // creature.": rules about the card, written as keywords.
        if let Some(rules) = hidden_rules(k) {
            for rule in rules {
                if let Some(a) = hidden_ability(rule, &Affected::This) {
                    abilities.push((0, a));
                }
            }
            continue;
        }
        // CR 702.73a: changeling, a characteristic-defining ability — printed only (one
        // given by another effect would not be a CDA, CR 604.3a, so `keyword` doesn't know it).
        if k == "Changeling" {
            def.keywords.push(Keyword::Changeling);
            continue;
        }
        match keyword(k) {
            Some(k) => def.keywords.push(k),
            None => {
                let name = k.split(':').next().unwrap_or(k);
                let name = name
                    .split(|c: char| c.is_ascii_digit())
                    .next()
                    .unwrap_or(name);
                tx.block(format!("kw:{}", name.trim()));
            }
        }
    }
    let order = line_order(face);
    for (i, rec) in face.abilities.iter().enumerate() {
        let before = tx.blockers.len();
        match tx.ability(rec) {
            Some(a) => {
                // "As an additional cost…" is printed before what the spell does.
                for extra in tx.additional.drain(..) {
                    abilities.push((order.abilities[i], extra));
                }
                abilities.push((order.abilities[i], a))
            }
            None => tx.explained(before, "ability"),
        }
    }
    // Merged ("Secondary") triggers stay at their first line's place.
    for (line, a) in tx.triggers() {
        abilities.push((order.triggers[line], a));
    }
    for (i, rec) in face.statics.iter().enumerate() {
        let before = tx.blockers.len();
        match tx.static_ability(rec) {
            Some(a) => {
                abilities.push((order.statics[i], a));
                for extra in tx.additional.drain(..) {
                    abilities.push((order.statics[i], extra));
                }
            }
            None if std::mem::take(&mut tx.nothing) && tx.blockers.len() == before => {}
            None => tx.explained(before, "static"),
        }
    }
    for (i, rec) in face.replacements.iter().enumerate() {
        let before = tx.blockers.len();
        match tx.replacement(rec) {
            Some(a) => abilities.push((order.replacements[i], a)),
            None => tx.explained(before, "replacement"),
        }
    }
    if (starred.0 && !tx.defines_pt.0) || (starred.1 && !tx.defines_pt.1) {
        tx.block("pt:variable");
    }
    abilities.extend(equips.into_iter().map(|a| (usize::MAX, a)));
    abilities.sort_by_key(|(i, _)| *i);
    def.abilities = abilities.into_iter().map(|(_, a)| a).collect();
    def.granted = std::mem::take(&mut tx.granted);
    // A variable holding an ability that nothing refers to means part of the card was
    // not understood.
    // (Only reported for a card that is otherwise understood: when something else is
    // blocked, the abilities it would have used are unused as a consequence.)
    if tx.blockers.is_empty() {
        for (name, _) in &parsed {
            if !tx.used_svars.contains(name) {
                tx.block("svar:unused-ability");
            }
        }
    }
    for b in chosen_blockers(&def) {
        tx.block(b);
    }
    // Forge scripts leave out some things the card says where its AI would never choose
    // otherwise ("you may put a counter", "another target creature" on an ability that
    // sacrifices its source). The card as compiled is read back against its Oracle text
    // for those words.
    if tx.blockers.is_empty()
        && let Some(oracle) = &face.oracle
    {
        for word in oracle_words_missing(oracle, &def) {
            tx.block(format!("oracle:{word}"));
        }
    }
    if tx.blockers.is_empty() {
        Ok(Compiled {
            def,
            tokens: tx.tokens,
        })
    } else {
        Err(tx.blockers.set)
    }
}

/// Where "the chosen type / color" is written somewhere the engine doesn't read it from
/// (D-076): named, never dropped.
fn chosen_blockers(def: &CardDef) -> Vec<&'static str> {
    let refers = |x: &dyn core::fmt::Debug| format!("{x:?}").contains("chosen: Some(");
    let mut out = Vec::new();
    for a in &def.abilities {
        match a {
            // "When this dies, … of the chosen type": the ability is put on the stack after
            // its source has left, and the choice isn't carried that far.
            AbilityDef::Triggered { when, body, .. }
                if refers(body)
                    && when.events().iter().any(|t| {
                        matches!(
                            t,
                            Trigger::Dies(ObjFilter::This) | Trigger::Leaves(ObjFilter::This)
                        )
                    }) =>
            {
                out.push("chosen:in a trigger on its source leaving")
            }
            // (A rule about players reads its source's choice: `players::may_play_with`.)
            AbilityDef::FromZone { .. } if refers(a) => out.push("chosen:in another zone"),
            AbilityDef::Mana {
                spend: Some(r),
                cost,
                ..
            } if refers(r) => {
                let not = r
                    .spells
                    .iter()
                    .chain(&r.abilities)
                    .chain(&r.sources)
                    .any(|s| s.chosen == Some(OfChosen::NotType));
                let several = r.spells.len() > 1 || r.abilities.len() > 1 || r.sources.len() > 1;
                if not || cost.sacrifice_self || cost.sacrifice.is_some() || several {
                    out.push("chosen:mana restriction shape");
                }
            }
            _ => {}
        }
    }
    // "Discard a card of the chosen type": the cards a discard cost may take are found
    // without the ability's source.
    let discards = def.abilities.iter().any(|a| match a {
        AbilityDef::Activated { cost, .. }
        | AbilityDef::Mana { cost, .. }
        | AbilityDef::AdditionalCost(cost) => cost.discard.as_ref().is_some_and(|d| refers(d)),
        _ => false,
    });
    if discards {
        out.push("chosen:in a discard cost");
    }
    // A delayed triggered ability (CR 603.7c) would have to keep the choice as it was when
    // the ability was created; it takes its source's at the time it triggers instead. No
    // card that compiles otherwise needs it: named, not built.
    let text = format!("{:?}", def.abilities);
    let delayed: Vec<usize> = text
        .split("DelayedTrigger { ability: ")
        .skip(1)
        .filter_map(|r| r.split(',').next()?.trim().parse().ok())
        .collect();
    for (i, a) in def.granted.iter().enumerate() {
        if refers(a) {
            out.push(if delayed.contains(&i) {
                "chosen:in a delayed trigger"
            } else {
                // An ability given to another object would look for that object's choice.
                "chosen:in an ability given to another object"
            });
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Rules text in lower case without "(reminder text)", split into what is said outright
/// and what is said in quotes (the abilities of tokens, and abilities given to others).
fn outright_and_quoted(text: &str) -> (String, String) {
    let (mut plain, mut inside) = (String::new(), String::new());
    let (mut parens, mut quoted) = (0u32, false);
    for c in text.chars() {
        match c {
            '(' => parens += 1,
            ')' => parens = parens.saturating_sub(1),
            '"' => {
                quoted = !quoted;
                inside.push(' ');
            }
            c if parens == 0 && !quoted => plain.extend(c.to_lowercase()),
            c if parens == 0 => inside.extend(c.to_lowercase()),
            _ => {}
        }
    }
    (plain, inside)
}

/// Words of the card's Oracle text that the compiled card has no counterpart for. What
/// the card says outright is compared with what the compiled card says outright; what it
/// gives to others in quotes, with the abilities the compiled card gives. (The quoted
/// abilities of tokens it makes are not compared: tokens are compiled on their own.)
///
/// This function reads `card_model::describe::rules_text`, so it depends on how that
/// text is worded. The phrases it relies on (pinned by `guard_phrases_are_still_written`
/// below; a wording change in `describe.rs` that breaks one fails that test instead of
/// silently changing which cards compile): the word "may" and the substring "up to"; the
/// word "unless"; the word "damage"; the words "another", "other" and "targets" (a
/// group of targets of one instruction, "each of one to two targets that are …");
/// "{t}" in costs; "attach this <type> to target creature you control" (Equip);
/// "its controller", "its owner", "that player", "target player", "target opponent",
/// "each player", "each opponent", "their"; "it deals"; "battlefield under your
/// control"; "you own" / "an opponent owns"; "can't be countered"; "an additional time".
fn oracle_words_missing(oracle: &str, def: &CardDef) -> Vec<&'static str> {
    let (plain, quoted) = outright_and_quoted(oracle);
    let text = card_model::describe::rules_text(def).join(" / ");
    let (compiled, compiled_quoted) = outright_and_quoted(&text);
    let has_word = |text: &str, word: &str| {
        text.split(|c: char| !c.is_ascii_alphabetic())
            .any(|w| w == word)
    };
    let mut missing = Vec::new();
    let mut compare = |plain: &str, compiled: &str| {
        // "You may": an optional instruction, or "up to" some number of things. "That
        // player may pay {2}. If they don't, …" is the same as "… unless that player pays
        // {2}" (CR 118.12a).
        let unless = compiled
            .split(|c: char| !c.is_ascii_alphabetic())
            .filter(|w| *w == "unless")
            .count();
        let plain_rest = plain.replacen("may pay", "pays", unless);
        if has_word(&plain_rest, "may") && !has_word(compiled, "may") && !compiled.contains("up to")
        {
            missing.push("says may");
        }
        // "Unless": a condition or a payment that lets it not happen ("enters tapped
        // unless you control an Island", "counter target spell unless its controller pays").
        if has_word(plain, "unless") && !has_word(compiled, "unless") {
            missing.push("says unless");
        }
        // "Deals 1 damage": not life loss, which some scripts write instead (no lifelink,
        // no prevention, no damage triggers).
        if has_word(plain, "damage") && !has_word(compiled, "damage") {
            missing.push("says damage");
        }
        // "A copy of …, except it has haste": the exceptions of a copy effect (CR 707.9)
        // are part of what the copy is.
        let sentences = |text: &str, all: &[&str]| {
            text.split(['.', '/', '•'])
                .filter(|s| all.iter().all(|w| s.contains(w)))
                .count()
        };
        if sentences(plain, &["copy", "except"]) > 0 && !has_word(compiled, "except") {
            missing.push("says except");
        }
        // "[The token] gains haste until end of turn": an effect that ends, which a script
        // of a token copy may leave lasting ("that token gains haste").
        let token_gains = compiled.contains("token gains") || compiled.contains("tokens gain");
        if token_gains
            && sentences(plain, &["gain", "until end of turn"])
                > sentences(compiled, &["gain", "until end of turn"])
        {
            missing.push("says gains until end of turn");
        }
        // "Another target creature": the ability's source can't be chosen.
        if (plain.contains("another target") || plain.contains("other target"))
            && !has_word(compiled, "another")
            && !has_word(compiled, "other")
            // (Several targets of one instruction are different objects anyway: CR 115.3.)
            && !has_word(compiled, "targets")
        {
            missing.push("another target");
        }
    };
    compare(&plain, &compiled);
    if !def.granted.is_empty() {
        compare(&quoted, &compiled_quoted);
    }
    // "Choose a creature type" / "of the chosen type": a choice, and what refers to it, are
    // said by the card exactly where the compiled card says them (D-076).
    for phrase in ["chosen type", "chosen color", "choose a creature type"] {
        // ("Choose a creature type. Draw a card for each permanent you control of that
        // type": the type just chosen.)
        let said = plain.replace("of that type", "of the chosen type");
        if said.contains(phrase) != compiled.contains(phrase) {
            missing.push("says chosen");
            break;
        }
    }
    // "{T}" in a cost: a script can leave it out (Lotus Ring's given "{T}, Sacrifice this
    // creature: …" is scripted as "Sacrifice this"), and a mana ability without it can be
    // activated again and again (D-066).
    let taps = |t: &str| t.matches("{t}").count();
    if taps(&plain) > taps(&compiled)
        || (!def.granted.is_empty() && taps(&quoted) > taps(&compiled_quoted))
    {
        missing.push("{T}");
    }
    // "Target creature you control" where the card says "target creature": some scripts
    // narrow a target to what Forge's AI should pick. (Equip is a keyword: its "target
    // creature you control" is reminder text.)
    // ("attach this Equipment to …": the card's own type, whatever `describe` calls it.)
    let mut aimed = compiled.clone();
    while let Some(i) = aimed.find("attach this ") {
        let rest = &aimed[i + "attach this ".len()..];
        let tail = " to target creature you control";
        match rest.find(tail) {
            Some(j) if !rest[..j].contains([' ', '.', '/', ':']) => {
                let end = i + "attach this ".len() + j + tail.len();
                aimed.replace_range(i..end, "");
            }
            _ => {
                // Not Equip: keep the rest of the text to be looked at.
                aimed.replace_range(i..i + "attach this ".len(), "");
            }
        }
    }
    let narrowed = aimed.split("target ").skip(1).any(|rest| {
        rest.split(['.', ':', '•', '/'])
            .next()
            .is_some_and(|phrase| phrase.contains("you control"))
    });
    if narrowed && !plain.contains("you control") && !plain.contains("your") {
        missing.push("target you control");
    }
    // "Its controller gains life", "that player discards a card": the card names a player
    // other than "you", and the compiled card must too (a script that writes `Defined$
    // You` there would give it to the wrong player).
    let other_player = [
        "its controller",
        "its owner ",
        "that player",
        "defending player",
        "that creature's controller",
        "that spell's controller",
        "that permanent's controller",
    ];
    let named = [
        "its controller",
        "its owner",
        "that player",
        "target player",
        "target opponent",
        "each player",
        "each opponent",
        "their",
    ];
    if other_player.iter().any(|w| plain.contains(w)) && !named.iter().any(|w| compiled.contains(w))
    {
        missing.push("another player");
    }
    // "That artifact deals damage equal to its mana value to …": the other object is the
    // source of the damage (its color, lifelink and deathtouch count), which some scripts
    // make the card itself.
    for noun in [
        "artifact",
        "creature",
        "permanent",
        "land",
        "enchantment",
        "planeswalker",
        "spell",
        "source",
    ] {
        if plain.contains(&format!("that {noun} deals")) && !compiled.contains("it deals") {
            missing.push("that one deals damage");
        }
    }
    // "Return it to the battlefield under your control" against "… under its owner's
    // control": some scripts give control where the card doesn't (or the other way).
    // ("Enters the battlefield under your control" is old wording for "you control".)
    // ("… onto the battlefield tapped under your control" counts too.)
    let under = |t: &str| t.replace("battlefield tapped under", "battlefield under");
    if under(&compiled).contains("battlefield under your control")
        != under(&plain).contains("to the battlefield under your control")
    {
        missing.push("under your control");
    }
    // Rules a script may leave out where Forge handles them elsewhere (D-073): "this spell
    // can't be countered", "… triggers an additional time".
    for phrase in ["can't be countered", "an additional time"] {
        if plain.contains(phrase) && !compiled.contains(phrase) {
            missing.push(if phrase == "an additional time" {
                "says an additional time"
            } else {
                "says can't be countered"
            });
        }
    }
    // "Is put into your graveyard from the battlefield" is about whose card it is; the
    // script may say whose creature it was.
    for (says, has, word) in [
        (
            "into your graveyard from the battlefield",
            "you own",
            "put into your graveyard",
        ),
        (
            "into an opponent's graveyard from the battlefield",
            "an opponent owns",
            "put into an opponent's graveyard",
        ),
    ] {
        if plain.contains(says) && !compiled.contains(has) {
            missing.push(word);
        }
    }
    missing.sort();
    missing.dedup();
    missing
}

/// Where each `A:`/`T:`/`S:`/`R:` line sits among the others, so abilities keep the
/// script's order.
struct LineOrder {
    abilities: Vec<usize>,
    triggers: Vec<usize>,
    statics: Vec<usize>,
    replacements: Vec<usize>,
}

fn line_order(face: &Face) -> LineOrder {
    let mut o = LineOrder {
        abilities: Vec::new(),
        triggers: Vec::new(),
        statics: Vec::new(),
        replacements: Vec::new(),
    };
    for (n, kind) in face.order.iter().enumerate() {
        match kind {
            'A' => o.abilities.push(n),
            'T' => o.triggers.push(n),
            'S' => o.statics.push(n),
            'R' => o.replacements.push(n),
            _ => {}
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::parse;

    fn known() -> Known {
        Known {
            subtypes: ["Elf", "Zombie", "Dragon", "Mountain", "Wizard"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }

    fn compile_text(text: &str) -> Result<Compiled, Blockers> {
        compile(&parse(text).unwrap(), &known())
    }

    // Invented cards in Forge's format.
    #[test]
    fn a_simple_creature_compiles_with_its_abilities_in_script_order() {
        let c = compile_text(
            "Name:Test Chanter\nManaCost:1 B\nTypes:Legendary Creature Zombie Wizard\nPT:2/3\nK:Flying\n\
             T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self,Zombie.Other+YouCtrl | Execute$ TrigDraw | TriggerDescription$ text\n\
             SVar:TrigDraw:DB$ Draw | Defined$ You | NumCards$ 1 | SubAbility$ DBLose\n\
             SVar:DBLose:DB$ LoseLife | LifeAmount$ 1\n\
             S:Mode$ Continuous | Affected$ Creature.Zombie+Other+YouCtrl | AddPower$ 1 | AddToughness$ 1 | Description$ text\n\
             A:AB$ PumpAll | Cost$ 1 B T | ValidCards$ Zombie | KW$ Menace | SpellDescription$ text\n\
             Oracle:text",
        )
        .unwrap();
        let d = &c.def;
        assert_eq!(d.name, "Test Chanter");
        assert_eq!(d.mana_cost.as_ref().unwrap().to_text(), "{1}{B}");
        assert_eq!(d.supertypes, [Supertype::Legendary]);
        assert_eq!(d.card_types, [CardType::Creature]);
        assert_eq!(d.creature_types, ["Zombie", "Wizard"]);
        assert_eq!((d.power, d.toughness), (Some(2), Some(3)));
        assert_eq!(d.keywords, [Keyword::Flying]);
        assert_eq!(d.abilities.len(), 3);
        // In the order written: trigger, static, activated.
        let AbilityDef::Triggered { when, body, .. } = &d.abilities[0] else {
            panic!("{:?}", d.abilities[0]);
        };
        // "This or another Zombie you control dies."
        let Trigger::Dies(ObjFilter::ThisOr(sel)) = when else {
            panic!("{when:?}");
        };
        assert_eq!(sel.subtypes, ["Zombie"]);
        assert_eq!(sel.controller, Who::You);
        assert!(!sel.other);
        let Body::Effect(mode) = body else { panic!() };
        assert!(matches!(&mode.effect, Effect::Seq(v) if v.len() == 2));
        assert!(matches!(d.abilities[1], AbilityDef::Static { .. }));
        let AbilityDef::Activated { cost, .. } = &d.abilities[2] else {
            panic!();
        };
        assert!(cost.tap && cost.mana.as_ref().unwrap().to_text() == "{1}{B}");
    }

    #[test]
    fn anything_not_understood_blocks_the_card_and_is_named() {
        let blockers = |text: &str| compile_text(text).unwrap_err();
        let base = "Name:Test Card\nManaCost:1 R\nTypes:Sorcery\n";
        // An unknown effect API.
        let b = blockers(&format!("{base}A:SP$ Fight | ValidTgts$ Creature"));
        assert!(b.contains("api:Fight"), "{b:?}");
        // A known API with a parameter the compiler doesn't read.
        let b = blockers(&format!(
            "{base}A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 2 | DivideEvenly$ True"
        ));
        assert_eq!(
            b.into_iter().collect::<Vec<_>>(),
            ["param:DealDamage.DivideEvenly"]
        );
        // A filter property that isn't a known subtype is not taken for one.
        let b = blockers(&format!(
            "{base}A:SP$ Destroy | ValidTgts$ Creature.DamagedBy"
        ));
        assert!(b.contains("filter:.DamagedBy"), "{b:?}");
        let b = blockers(&format!("{base}A:SP$ DestroyAll | ValidCards$ Gremlin"));
        assert!(b.contains("filter:Gremlin"), "{b:?}");
        // An expression form that isn't supported.
        let b = blockers(&format!(
            "{base}A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ X\nSVar:X:Count$YourPoisonCounters/ThirdUp"
        ));
        assert!(b.iter().any(|x| x.starts_with("expr:")), "{b:?}");
        // A keyword the engine doesn't have, an unknown line, a second face.
        let b = blockers(
            "Name:Test Card\nManaCost:G\nTypes:Creature Elf\nPT:1/1\nK:Banding\nMystery:1",
        );
        assert!(
            b.contains("kw:Banding") && b.contains("line:Mystery"),
            "{b:?}"
        );
        let b = blockers(
            "Name:A\nTypes:Creature Elf\nAlternateMode:Flip\nALTERNATE\nName:B\nTypes:Creature Elf",
        );
        assert!(b.contains("layout:Flip"), "{b:?}");
        // An ability variable nothing refers to.
        let b = blockers(&format!(
            "{base}A:SP$ Draw | NumCards$ 2\nSVar:DBExtra:DB$ GainLife | LifeAmount$ 3"
        ));
        assert!(b.contains("svar:unused-ability"), "{b:?}");
        // Descriptions and AI hints are not rules: they block nothing.
        assert!(compile_text(&format!(
            "{base}A:SP$ Draw | NumCards$ 2 | SpellDescription$ Draw two cards. | AILogic$ Whatever\nSVar:PlayMain1:TRUE\nDeckHas:Ability$Draw"
        ))
        .is_ok());
    }

    #[test]
    fn where_x_is_a_count_it_is_worked_out_once() {
        let c = compile_text(
            "Name:Test Blast\nManaCost:2 R\nTypes:Sorcery\nA:SP$ DamageAll | NumDmg$ X | ValidCards$ Creature\nSVar:X:Count$Valid Mountain.YouCtrl",
        )
        .unwrap();
        let AbilityDef::Spell(Body::Effect(mode)) = &c.def.abilities[0] else {
            panic!();
        };
        let Effect::Seq(v) = &mode.effect else {
            panic!()
        };
        assert!(matches!(&v[0], Effect::SetX(Amount::Count(s)) if s.subtypes == ["Mountain"]));
        assert!(matches!(
            &v[1],
            Effect::DamageEach {
                amount: Amount::X,
                ..
            }
        ));
    }

    #[test]
    fn mana_costs() {
        assert_eq!(mana_cost("no cost"), Ok(None));
        assert_eq!(
            mana_cost("X 2 G G").unwrap().unwrap().to_text(),
            "{X}{2}{G}{G}"
        );
        assert!(
            mana_cost("1 WU").is_err(),
            "hybrid mana isn't supported yet"
        );
    }

    /// `oracle_words_missing` reads `describe::rules_text`. Each phrase it depends on is
    /// rendered here from a small card, so a wording change in `describe.rs` fails this
    /// test instead of silently changing which cards compile.
    #[test]
    fn guard_phrases_are_still_written() {
        use card_model::{
            AbilityDef, Amount, Body, CardDef, CardType, Cost, DamageBy, Effect, Mode, ObjFilter,
            ObjRef, PlayerRef, Selector, TargetFilter, TargetGroup, TargetRef, Trigger, Who,
        };
        let spell =
            |targets: Vec<TargetFilter>, effect: Effect, groups: Vec<TargetGroup>| CardDef {
                name: "Test".into(),
                card_types: vec![CardType::Instant],
                abilities: vec![AbilityDef::Spell(Body::Effect(Mode {
                    targets,
                    effect,
                    groups,
                }))],
                ..CardDef::default()
            };
        let text = |def: &CardDef| {
            card_model::describe::rules_text(def)
                .join(" / ")
                .to_lowercase()
        };
        let has_word =
            |t: &str, w: &str| t.split(|c: char| !c.is_ascii_alphabetic()).any(|x| x == w);
        let draw = Effect::Draw {
            who: PlayerRef::You,
            amount: Amount::Fixed(1),
        };
        // "may"
        let t = text(&spell(vec![], Effect::May(Box::new(draw.clone())), vec![]));
        assert!(has_word(&t, "may"), "{t}");
        // "damage", "it deals"
        let t = text(&spell(
            vec![TargetFilter::AnyTarget],
            Effect::DealDamage {
                to: TargetRef::Target(0),
                amount: Amount::Fixed(2),
                by: DamageBy::Triggering,
            },
            vec![],
        ));
        assert!(has_word(&t, "damage") && t.contains("it deals"), "{t}");
        // "another" (a target that excludes the source)
        let another = Selector {
            types: vec![CardType::Creature],
            other: true,
            ..Selector::default()
        };
        let t = text(&spell(
            vec![TargetFilter::Permanent(vec![another])],
            Effect::Destroy {
                what: ObjRef::Target(0),
            },
            vec![],
        ));
        assert!(has_word(&t, "another"), "{t}");
        // "targets" (one word "target" for one to two targets)
        let t = text(&spell(
            vec![TargetFilter::Creature, TargetFilter::Creature],
            Effect::Together(vec![
                Effect::Destroy {
                    what: ObjRef::Target(0),
                },
                Effect::Destroy {
                    what: ObjRef::Target(1),
                },
            ]),
            vec![TargetGroup {
                first: 0,
                count: 2,
                min: 1,
            }],
        ));
        assert!(has_word(&t, "targets"), "{t}");
        // "that player", "its controller", "target player", "their"
        for (who, phrase) in [
            (PlayerRef::TriggeringPlayer, "that player"),
            (PlayerRef::TriggeringController, "its controller"),
            (PlayerRef::EachOpponent, "each opponent"),
            (PlayerRef::Each, "each player"),
        ] {
            let t = text(&spell(
                vec![],
                Effect::Draw {
                    who,
                    amount: Amount::Fixed(1),
                },
                vec![],
            ));
            assert!(t.contains(phrase), "{t}");
        }
        let t = text(&spell(
            vec![TargetFilter::Player],
            Effect::Draw {
                who: PlayerRef::Target(0),
                amount: Amount::Fixed(1),
            },
            vec![],
        ));
        assert!(t.contains("target player"), "{t}");
        // "{t}" in a cost
        let tapper = CardDef {
            name: "Test".into(),
            card_types: vec![CardType::Artifact],
            abilities: vec![AbilityDef::Activated {
                cost: Cost {
                    tap: true,
                    ..Cost::default()
                },
                timing: card_model::Timing::Instant,
                only_if: None,
                body: Body::Effect(Mode {
                    targets: vec![],
                    effect: draw.clone(),
                    groups: vec![],
                }),
                from: card_model::ActivationZone::Battlefield,
                cycling: false,
                reduce: None,
            }],
            ..CardDef::default()
        };
        assert!(text(&tapper).to_lowercase().contains("{t}"));
        // Equip: "attach this <type> to target creature you control" is not "narrowed".
        let equipment = CardDef {
            name: "Test".into(),
            card_types: vec![CardType::Artifact],
            creature_types: vec!["Equipment".into()],
            abilities: vec![AbilityDef::Activated {
                cost: Cost::default(),
                timing: card_model::Timing::Sorcery,
                only_if: None,
                body: Body::Effect(Mode {
                    targets: vec![TargetFilter::Permanent(vec![Selector {
                        types: vec![CardType::Creature],
                        controller: Who::You,
                        ..Selector::default()
                    }])],
                    effect: Effect::AttachThis {
                        to: ObjRef::Target(0),
                    },
                    groups: vec![],
                }),
                from: card_model::ActivationZone::Battlefield,
                cycling: false,
                reduce: None,
            }],
            ..CardDef::default()
        };
        let t = text(&equipment);
        assert!(
            t.contains("attach this equipment to target creature you control"),
            "{t}"
        );
        assert!(oracle_words_missing("equip {1}", &equipment).is_empty());
        // "says chosen" (D-076): "choose a creature type", "of the chosen type", "of the
        // chosen color" are written as the guard reads them.
        let banner = CardDef {
            name: "Test".into(),
            card_types: vec![CardType::Artifact],
            abilities: vec![
                AbilityDef::Replacement {
                    replaces: card_model::Replaces::ThisEnters,
                    with: card_model::ReplaceWith::Choose(card_model::ChoiceOf::CreatureType),
                },
                AbilityDef::Static {
                    affected: card_model::Affected::All(Selector {
                        types: vec![CardType::Creature],
                        chosen: Some(card_model::OfChosen::Type),
                        ..Selector::default()
                    }),
                    effects: vec![card_model::ContinuousEffect::ModifyPT(1, 1)],
                    only_if: None,
                },
                AbilityDef::Static {
                    affected: card_model::Affected::All(Selector {
                        types: vec![CardType::Creature],
                        chosen: Some(card_model::OfChosen::Color),
                        ..Selector::default()
                    }),
                    effects: vec![card_model::ContinuousEffect::ModifyPT(1, 1)],
                    only_if: None,
                },
            ],
            ..CardDef::default()
        };
        let t = text(&banner);
        assert!(
            t.contains("choose a creature type")
                && t.contains("chosen type")
                && t.contains("chosen color"),
            "{t}"
        );
        // ... but a narrowed target elsewhere still is.
        let narrowed = spell(
            vec![TargetFilter::Permanent(vec![Selector {
                types: vec![CardType::Creature],
                controller: Who::You,
                ..Selector::default()
            }])],
            Effect::Destroy {
                what: ObjRef::Target(0),
            },
            vec![],
        );
        assert_eq!(
            oracle_words_missing("destroy target creature.", &narrowed),
            ["target you control"]
        );
        // "you own": a creature put into your graveyard from the battlefield
        let dies = CardDef {
            name: "Test".into(),
            card_types: vec![CardType::Enchantment],
            abilities: vec![AbilityDef::Triggered {
                when: Trigger::Dies(ObjFilter::Matching(Selector {
                    types: vec![CardType::Creature],
                    owner: Who::You,
                    ..Selector::default()
                })),
                intervening_if: None,
                body: Body::Effect(Mode {
                    targets: vec![],
                    effect: draw.clone(),
                    groups: vec![],
                }),
            }],
            ..CardDef::default()
        };
        assert!(text(&dies).contains("you own"), "{}", text(&dies));
        // "can't be countered", "an additional time"
        let t = text(&CardDef {
            abilities: vec![AbilityDef::CantBeCountered { spells: None }],
            ..CardDef::default()
        });
        assert!(t.contains("can't be countered"), "{t}");
        let t = text(&CardDef {
            abilities: vec![AbilityDef::AdditionalTrigger {
                sources: ObjFilter::This,
                cause: None,
            }],
            ..CardDef::default()
        });
        assert!(t.contains("an additional time"), "{t}");
        // "except" (copy exceptions) and "token gains … until end of turn" (D-074).
        let t = text(&spell(
            vec![TargetFilter::Creature],
            Effect::CopyPermanent {
                what: card_model::CopyWhat::Obj(ObjRef::Target(0)),
                count: Amount::Fixed(1),
                except: vec![card_model::CopyExcept::NotLegendary],
                gains: vec![card_model::Keyword::Haste],
                tapped: false,
            },
            vec![],
        ));
        assert!(
            has_word(&t, "except") && t.contains("copy") && t.contains("token gains"),
            "{t}"
        );
        // "unless" and "battlefield under your control" are pinned by the cards in
        // forge_features.rs (Pay unless; return under your control).
    }
}
