//! Replacement and prevention effects (CR 614–616 (2026-11-06)).
//! Text: docs/rules-notes/614-616-replacement-prevention.md.
//!
//! Every replaceable game event is first built as a [`Proposed`] event and run through
//! `frames::ApplyFrame` (D-024): the applicable effects are found here, the affected
//! player chooses among them when there is more than one (CR 616.1, `ChooseReplacement`),
//! each effect applies at most once per event (CR 614.5), applicability is re-evaluated after
//! every application (CR 616.1f, 616.2), and only then is the (modified) event performed.

use crate::effects::Bearer;
use crate::events::{DamageTarget, LossReason};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::abilities;
use crate::stack::Target;
use crate::state::GameState;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec::Vec;
use card_model::{
    AbilityDef, AbilityId, CardDefId, CounterKind, DamageTo, ObjFilter, ReplaceWith, Replaces,
};
use serde::{Deserialize, Serialize};

/// Why an object is moving (for events and for "destroy" vs other moves, CR 701.8b).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveCause {
    /// A rule moved it (e.g. SBA 704.5f, legend rule).
    Rule,
    /// CR 701.8
    Destroy,
    /// CR 701.21
    Sacrifice,
    /// CR 701.9
    Discard,
    /// A spell finishing resolution (CR 608.2n) or a permanent spell resolving (608.3a).
    Resolve,
    /// CR 701.6a
    Countered,
    /// CR 608.2b
    Fizzled,
    /// CR 305.1
    PlayLand,
    /// CR 702.29c: discarded to pay the cost of its own cycling ability.
    Cycle,
}

/// A game event that hasn't happened yet (CR 614.1: replacement effects watch for events
/// that *would* happen).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Proposed {
    Move {
        object: ObjectId,
        to: Zone,
        position: LibraryPosition,
        controller: Option<PlayerId>,
        cause: MoveCause,
        /// Enters the battlefield tapped / with counters (CR 614.1c–d, 122.6).
        tapped: bool,
        counters: Vec<(CounterKind, u32)>,
        /// Shuffle its new library afterwards ("shuffle it into its owner's library").
        shuffle: bool,
        /// An Aura entering attached to this object (CR 303.4).
        attach_to: Option<ObjectId>,
        /// It enters as a copy (CR 707.5, 614.1c): its copiable values.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        copy: Option<alloc::boxed::Box<crate::copy::Copiable>>,
    },
    Damage {
        source: ObjectId,
        deathtouch: bool,
        /// CR 702.15b: the source has lifelink.
        lifelink: bool,
        target: DamageTarget,
        amount: u32,
        combat: bool,
    },
    Draw {
        player: PlayerId,
    },
    CreateTokens {
        token: CardDefId,
        count: u32,
        controller: PlayerId,
        /// The tokens are copies (CR 707, 111.12): of what, and what comes with them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        copy: Option<alloc::boxed::Box<TokenCopy>>,
    },
    PutCounters {
        object: ObjectId,
        kind: CounterKind,
        count: u32,
    },
    GainLife {
        player: PlayerId,
        amount: u32,
    },
    LoseLife {
        player: PlayerId,
        amount: u32,
    },
    /// Not replaceable in M2 (no "can't lose" cards yet; CR 704.7 noted).
    LoseGame {
        player: PlayerId,
        reason: LossReason,
    },
    /// CR 704.5q (not replaceable).
    RemoveCounters {
        object: ObjectId,
        kind: CounterKind,
        n: u32,
    },
    /// CR 704.5d (not replaceable).
    CeaseToExist {
        object: ObjectId,
    },
}

/// Tokens created as copies (CR 707.2): their copiable values, and what else the
/// instruction that creates them says about each of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCopy {
    pub values: crate::copy::Copiable,
    pub tapped: bool,
    /// Not copiable (CR 707.2): keywords each token gains for as long as it is on the
    /// battlefield ("that token gains haste").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gains: Vec<card_model::Keyword>,
    /// A delayed triggered ability about each token (CR 603.7): "sacrifice it at the
    /// beginning of the next end step".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_end: Option<AbilityId>,
    /// The source of the spell or ability that creates them, its controller, and the card
    /// its text is written on (CR 603.7d–e, 611.2).
    pub source: ObjectId,
    pub controller: PlayerId,
    pub card: CardDefId,
}

impl Proposed {
    /// A plain move with no entering modifications.
    pub fn moving(object: ObjectId, to: Zone, cause: MoveCause) -> Self {
        Proposed::Move {
            object,
            to,
            position: LibraryPosition::Top,
            controller: None,
            cause,
            tapped: false,
            counters: Vec::new(),
            shuffle: false,
            attach_to: None,
            copy: None,
        }
    }
}

/// Identifies one replacement/prevention effect, so it applies at most once per event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RepKey {
    /// Replacement ability `ability` of `source` (a permanent, or the entering object
    /// itself); `nth` tells apart the times `source` has that ability.
    Static {
        source: ObjectId,
        ability: AbilityId,
        nth: u8,
    },
    /// A prevention shield (CR 615.7).
    Shield(u32),
    /// Ability `index` of lasting effect `id` (CR 611.2).
    Lasting { id: u32, index: u8 },
    /// CR 903.9b: "If a commander would be put into its owner's hand or library from
    /// anywhere, its owner may put it into the command zone instead." Optional; a rule, not an
    /// ability of any object.
    CommanderReturn,
}

impl RepKey {
    /// Optional effects may be declined (the affected player is always asked).
    pub fn optional(self) -> bool {
        matches!(self, RepKey::CommanderReturn)
    }
}

/// A proposed event plus the effects already applied to it (CR 614.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub event: Proposed,
    pub applied: Vec<RepKey>,
}

/// CR 616.1: the player who chooses among effects modifying this event — the affected
/// object's controller (its owner if it has no controller), or the affected player.
pub fn affected_player(st: &GameState, ev: &Proposed) -> PlayerId {
    let of = |o: ObjectId| {
        st.objects
            .get(&o)
            .map(|x| match x.zone {
                Zone::Battlefield => crate::layers::controller_of(st, x.id),
                Zone::Stack => x.controller,
                _ => x.owner,
            })
            .unwrap_or(st.turn.active)
    };
    match ev {
        Proposed::Move { object, .. }
        | Proposed::PutCounters { object, .. }
        | Proposed::RemoveCounters { object, .. }
        | Proposed::CeaseToExist { object } => of(*object),
        Proposed::Damage { target, .. } => match target {
            DamageTarget::Player(p) => *p,
            DamageTarget::Object(o) => of(*o),
        },
        Proposed::Draw { player }
        | Proposed::GainLife { player, .. }
        | Proposed::LoseLife { player, .. }
        | Proposed::LoseGame { player, .. } => *player,
        Proposed::CreateTokens { controller, .. } => *controller,
    }
}

/// Effects that would modify this event now, in stable order (sources by battlefield
/// order, then the entering object, then shields), minus those already applied.
pub fn applicable(st: &GameState, p: &Pending) -> Vec<RepKey> {
    let mut out = Vec::new();
    for &source in &st.zones.battlefield {
        // CLAUDE.md invariant 4: only abilities the source currently has.
        let has = abilities(st, source);
        for (k, (ability, a)) in has.iter().enumerate() {
            let who = Bearer {
                source,
                controller: crate::layers::controller_of(st, source),
                about: &[],
            };
            if let AbilityDef::Replacement { replaces, .. } = a
                && applies(st, who, replaces, &p.event)
            {
                // An ability the source was given twice is two effects.
                let nth = has[..k].iter().filter(|(b, _)| b == ability).count() as u8;
                out.push(RepKey::Static {
                    source,
                    ability: *ability,
                    nth,
                });
            }
        }
    }
    // What lasting effects carry: "prevent all combat damage that would be dealt this turn".
    for (e, index, a) in crate::effects::lasting_abilities(st) {
        let who = Bearer {
            source: e.source,
            controller: e.controller,
            about: e.about.as_slice(),
        };
        if let AbilityDef::Replacement { replaces, .. } = a
            && applies(st, who, replaces, &p.event)
        {
            out.push(RepKey::Lasting {
                id: e.id,
                index: index as u8,
            });
        }
    }
    // CR 614.12: an entering permanent's own "enters …" replacement effects apply too,
    // checked against the permanent as it would exist on the battlefield.
    if let Proposed::Move {
        object,
        to: Zone::Battlefield,
        ..
    } = &p.event
        && st
            .objects
            .get(object)
            .is_some_and(|o| o.zone != Zone::Battlefield)
    {
        // CR 707.5: one that enters as a copy has the copied text's "enters with" and "as
        // this enters" abilities instead of its own.
        for ability in entering_abilities(st, &p.event) {
            if let AbilityDef::Replacement {
                replaces: Replaces::ThisEnters,
                with,
            } = st.catalog.ability(ability)
                && !entering_unchanged(st, with, &p.event)
            {
                out.push(RepKey::Static {
                    source: *object,
                    ability,
                    nth: 0,
                });
            }
        }
    }
    // CR 113.6b: "if this card would be put into a graveyard from anywhere" works wherever
    // the card is (on the battlefield it is one of the abilities above).
    if let Proposed::Move {
        object,
        to: Zone::Graveyard,
        ..
    } = &p.event
        && st
            .objects
            .get(object)
            .is_some_and(|o| o.zone != Zone::Battlefield)
    {
        for (i, a) in st.card_def(*object).abilities.iter().enumerate() {
            if let AbilityDef::Replacement {
                replaces: Replaces::ThisToGraveyard,
                ..
            } = a
            {
                out.push(RepKey::Static {
                    source: *object,
                    ability: AbilityId::printed(st.object(*object).card, i as u16),
                    nth: 0,
                });
            }
        }
    }
    if let Proposed::Damage { target, amount, .. } = &p.event
        && *amount > 0
    {
        let t = match target {
            DamageTarget::Player(q) => Target::Player(*q),
            DamageTarget::Object(o) => Target::Object(*o),
        };
        for sh in &st.shields {
            if sh.target == t && sh.remaining > 0 {
                out.push(RepKey::Shield(sh.id));
            }
        }
    }
    // CR 903.9b
    if let Proposed::Move {
        object,
        to: Zone::Hand | Zone::Library,
        ..
    } = &p.event
        && st.is_commander(*object)
    {
        out.push(RepKey::CommanderReturn);
    }
    out.retain(|k| !p.applied.contains(k));
    out
}

/// The abilities a permanent entering with `ev` will have as it enters (CR 614.12): its
/// own, or — if it enters as a copy — the copiable values' (CR 707.5).
pub fn entering_abilities(st: &GameState, ev: &Proposed) -> Vec<AbilityId> {
    let Proposed::Move { object, copy, .. } = ev else {
        return Vec::new();
    };
    let Some(o) = st.objects.get(object) else {
        return Vec::new();
    };
    match copy {
        Some(v) => crate::copy::characteristics(&st.catalog, v, o.owner).abilities,
        None => crate::copy::base(st, *object, o.owner).abilities,
    }
}

/// The player a permanent entering with this event will enter under the control of
/// (CR 110.2, 110.2a): "you" for its own "as this enters" abilities (CR 109.5).
pub fn entering_controller(st: &GameState, ev: &Proposed) -> Option<PlayerId> {
    match ev {
        Proposed::Move {
            object,
            to: Zone::Battlefield,
            controller,
            ..
        } => st
            .objects
            .get(object)
            .map(|o| controller.unwrap_or(o.owner)),
        _ => None,
    }
}

/// "Enters tapped unless [condition]" whose condition holds: the effect doesn't modify the
/// event (judged on the battlefield as it is before the permanent enters, CR 614.12).
fn entering_unchanged(st: &GameState, with: &ReplaceWith, ev: &Proposed) -> bool {
    let ReplaceWith::EntersTappedUnless(cond) = with else {
        return false;
    };
    let (Some(you), Proposed::Move { object, .. }) = (entering_controller(st, ev), ev) else {
        return false;
    };
    crate::triggers::condition_holds(st, Some(cond), you, *object)
}

/// A choice an effect asks for as it is applied (CR 614.12a): "as this enters, you may
/// pay 2 life" / "… reveal an Island card from your hand". `None` when there is nothing to
/// choose — the effect doesn't ask for one, or the player can't do it (CR 118.3, 119.4: too
/// little life; no such card) — and the effect simply applies.
pub fn entry_choice(
    st: &GameState,
    key: RepKey,
    ev: &Proposed,
    moving: &[ObjectId],
) -> Option<(
    PlayerId,
    crate::decision::ChoiceKind,
    Vec<crate::decision::OptionDesc>,
)> {
    use crate::decision::{ChoiceKind, OptionDesc};
    let RepKey::Static {
        source, ability, ..
    } = key
    else {
        return None;
    };
    let act = match st.catalog.ability(ability) {
        AbilityDef::Replacement {
            with: ReplaceWith::EntersTappedUnlessYou(act) | ReplaceWith::EntersOnlyIfYou(act),
            ..
        } => act,
        // CR 707.5, 614.12a: "you may have this enter as a copy of [a permanent]" — any of
        // those on the battlefield as it is before the permanent enters; "you" is the
        // player it will enter under the control of.
        AbilityDef::Replacement {
            with: ReplaceWith::EntersAsCopy { of, .. },
            ..
        } => {
            let player = entering_controller(st, ev)?;
            let Proposed::Move { object, tapped, .. } = ev else {
                return None;
            };
            let mut options: Vec<OptionDesc> = crate::layers::matching(st, of, *object, player)
                .into_iter()
                .filter(|c| c != object)
                .map(|card| OptionDesc::Card { card })
                .collect();
            if options.is_empty() {
                return None;
            }
            options.push(OptionDesc::Decline);
            return Some((
                player,
                ChoiceKind::AsEnters {
                    object: source,
                    ability,
                    tapped: *tapped,
                },
                options,
            ));
        }
        _ => return None,
    };
    let player = entering_controller(st, ev)?;
    let Proposed::Move { object, tapped, .. } = ev else {
        return None;
    };
    let mut options = match act {
        card_model::AsEnters::PayLife(n) => {
            if st.player(player).life < *n as i32 {
                return None;
            }
            alloc::vec![OptionDesc::Accept]
        }
        // CR 614.13a: not the card that is becoming the permanent, nor any other card
        // moving at the same time.
        card_model::AsEnters::Reveal(sel) | card_model::AsEnters::Discard(sel) => st
            .zones
            .list(Zone::Hand, player)
            .iter()
            .filter(|&&c| {
                c != *object && !moving.contains(&c) && crate::targeting::card_matches(st, c, sel)
            })
            .map(|&card| OptionDesc::Card { card })
            .collect(),
    };
    if options.is_empty() {
        return None;
    }
    options.push(OptionDesc::Decline);
    Some((
        player,
        ChoiceKind::AsEnters {
            object: source,
            ability,
            tapped: *tapped,
        },
        options,
    ))
}

/// Apply `key` — an effect that asked for a choice (see [`entry_choice`]) — with the
/// answer. Returns the modified event and the events the answer itself causes: paying life
/// is losing it (CR 119.4), a replaceable event of its own that happens now, before the
/// permanent enters.
pub fn apply_chosen(
    st: &mut GameState,
    key: RepKey,
    ev: Proposed,
    answer: &crate::decision::OptionDesc,
) -> (Proposed, Vec<Proposed>) {
    use crate::decision::OptionDesc;
    let RepKey::Static {
        source, ability, ..
    } = key
    else {
        unreachable!("only an ability asks as it applies")
    };
    // CR 707.5: it enters as a copy of the chosen permanent — with the copiable values
    // that permanent has now (CR 707.2b), and the exceptions (CR 707.9).
    if let AbilityDef::Replacement {
        with: ReplaceWith::EntersAsCopy { except, tapped, .. },
        ..
    } = st.catalog.ability(ability).clone()
    {
        let mut ev = ev;
        if let (OptionDesc::Card { card }, Proposed::Move { copy, tapped: t, .. }) =
            (answer, &mut ev)
        {
            *copy = Some(alloc::boxed::Box::new(
                crate::copy::values(st, *card).with(ability.card, &except),
            ));
            *t |= tapped;
            st.emit(crate::events::Event::Replaced { source });
        }
        return (ev, Vec::new());
    }
    let (act, otherwise) = match st.catalog.ability(ability).clone() {
        AbilityDef::Replacement {
            with: ReplaceWith::EntersTappedUnlessYou(act),
            ..
        } => (act, ReplaceWith::EntersTapped),
        AbilityDef::Replacement {
            with: ReplaceWith::EntersOnlyIfYou(act),
            ..
        } => (act.clone(), ReplaceWith::EntersOnlyIfYou(act)),
        _ => unreachable!("only \"as this enters, you may …\" asks"),
    };
    st.emit(crate::events::Event::Replaced { source });
    let player = entering_controller(st, &ev).expect("an entering permanent");
    match (act, answer) {
        (card_model::AsEnters::PayLife(n), OptionDesc::Accept) => {
            (ev, alloc::vec![Proposed::LoseLife { player, amount: n }])
        }
        (card_model::AsEnters::Reveal(_), OptionDesc::Card { card }) => {
            // CR 701.20a–b: shown to all players; it stays in the hand.
            st.emit(crate::events::Event::Revealed {
                player,
                cards: alloc::vec![*card],
            });
            (ev, Vec::new())
        }
        // CR 701.9: discarded now, before the permanent enters.
        (card_model::AsEnters::Discard(_), OptionDesc::Card { card }) => (
            ev,
            alloc::vec![Proposed::moving(*card, Zone::Graveyard, MoveCause::Discard)],
        ),
        (_, OptionDesc::Decline) => (modify(ev, otherwise), Vec::new()),
        (_, o) => unreachable!("not an answer to an \"as this enters\" choice: {o:?}"),
    }
}

/// Is `id` — something that deals or is dealt damage — what `filter` says, for `who`?
/// A source that has left is judged as it last existed, if the ability dealing the damage
/// remembers it (CR 608.2h).
fn damage_object_hit(st: &GameState, filter: &ObjFilter, id: ObjectId, who: Bearer<'_>) -> bool {
    let sel = match filter {
        ObjFilter::This => return id == who.source,
        ObjFilter::Remembered => return who.about.contains(&id),
        ObjFilter::Host => {
            return st.objects.get(&who.source).and_then(|o| o.attached_to) == Some(id);
        }
        ObjFilter::ThisOr(_) if id == who.source => return true,
        ObjFilter::Matching(sel) | ObjFilter::ThisOr(sel) => sel,
    };
    let (c, facts) = match st.objects.get(&id) {
        Some(o) => {
            let c = crate::layers::chars(st, id);
            let (attacking, blocking) = crate::layers::combat_role(st, id);
            let facts = crate::layers::Facts {
                id,
                controller: c.controller,
                owner: o.owner,
                is_token: o.is_token,
                tapped: o.tapped,
                attacking,
                blocking,
            };
            (c, facts)
        }
        None => {
            let lki = st.stack.last().and_then(|top| match &top.kind {
                crate::stack::StackKind::Ability { source, .. } if *source == id => {
                    top.source_lki.clone()
                }
                _ => None,
            });
            let Some(c) = lki else { return false };
            let facts = crate::layers::Facts {
                id,
                controller: c.controller,
                owner: c.controller,
                is_token: false,
                tapped: false,
                attacking: false,
                blocking: false,
            };
            (c, facts)
        }
    };
    crate::layers::matches_parts(sel, &c, facts, who.source, who.controller)
}

fn applies(st: &GameState, who: Bearer<'_>, replaces: &Replaces, ev: &Proposed) -> bool {
    let (source, you) = (who.source, who.controller);
    match (replaces, ev) {
        // CR 615.1a: "prevent all [combat] damage that would be dealt [to …] [by …]".
        (
            Replaces::Damage { by, to, combat },
            Proposed::Damage {
                source: dealer,
                target,
                amount,
                combat: in_combat,
                ..
            },
        ) => {
            *amount > 0
                && combat.is_none_or(|c| c == *in_combat)
                && by
                    .as_ref()
                    .is_none_or(|f| damage_object_hit(st, f, *dealer, who))
                && match (to, target) {
                    (DamageTo::Anything, _) => true,
                    (DamageTo::Player(w), DamageTarget::Player(p)) => match w {
                        card_model::Who::Any => true,
                        card_model::Who::You => *p == you,
                        card_model::Who::Opponent => *p != you,
                    },
                    (DamageTo::Permanent(f), DamageTarget::Object(o)) => {
                        damage_object_hit(st, f, *o, who)
                    }
                    _ => false,
                }
        }
        (
            Replaces::CreateTokens,
            Proposed::CreateTokens {
                controller, count, ..
            },
        ) => *controller == you && *count > 0,
        (Replaces::PutCountersOnYourPermanents, Proposed::PutCounters { object, count, .. }) => {
            *count > 0
                && st.objects.get(object).is_some_and(|o| {
                    o.zone == Zone::Battlefield && crate::layers::controller_of(st, o.id) == you
                })
        }
        // CR 614.16 / 122.6: counters it enters with count too.
        (
            Replaces::PutCountersOnYourPermanents,
            Proposed::Move {
                object,
                to: Zone::Battlefield,
                controller,
                counters,
                ..
            },
        ) => !counters.is_empty() && controller.unwrap_or_else(|| st.object(*object).owner) == you,
        (
            Replaces::CardToGraveyard,
            Proposed::Move {
                object,
                to: Zone::Graveyard,
                ..
            },
        ) => st.objects.get(object).is_some_and(crate::copy::is_card),
        (
            Replaces::ThisDies,
            Proposed::Move {
                object,
                to: Zone::Graveyard,
                ..
            },
        ) => *object == source && st.object(source).zone == Zone::Battlefield,
        // CR 700.4: "dies" — from the battlefield, judged as it is there now (its last
        // existence on the battlefield).
        (
            Replaces::Dies(sel),
            Proposed::Move {
                object,
                to: Zone::Graveyard,
                ..
            },
        ) => st.objects.get(object).is_some_and(|o| {
            o.zone == Zone::Battlefield && {
                let (attacking, blocking) = crate::layers::combat_role(st, *object);
                crate::layers::matches_parts(
                    sel,
                    &crate::layers::chars(st, *object),
                    crate::layers::Facts {
                        id: *object,
                        controller: crate::layers::controller_of(st, *object),
                        owner: o.owner,
                        is_token: o.is_token,
                        tapped: o.tapped,
                        attacking,
                        blocking,
                    },
                    source,
                    you,
                )
            }
        }),
        // From the battlefield; from anywhere else see `applicable`.
        (
            Replaces::ThisToGraveyard,
            Proposed::Move {
                object,
                to: Zone::Graveyard,
                ..
            },
        ) => *object == source,
        // CR 614.12: checked against the permanent as it would exist on the battlefield.
        (
            Replaces::OtherEnters(sel),
            Proposed::Move {
                object,
                to: Zone::Battlefield,
                controller,
                ..
            },
        ) => st.objects.get(object).is_some_and(|o| {
            o.zone != Zone::Battlefield
                && *object != source
                && crate::layers::matches_parts(
                    sel,
                    &crate::layers::chars(st, *object),
                    crate::layers::Facts {
                        id: *object,
                        controller: controller.unwrap_or(o.owner),
                        owner: o.owner,
                        is_token: o.is_token,
                        tapped: false,
                        attacking: false,
                        blocking: false,
                    },
                    source,
                    you,
                )
        }),
        _ => false,
    }
}

/// Apply `key` to `ev`. `None` = the event no longer happens (e.g. all damage prevented,
/// CR 614.7a). Shield use is recorded through the action chokepoint.
pub fn apply(st: &mut GameState, key: RepKey, ev: Proposed) -> Option<Proposed> {
    match key {
        RepKey::Shield(id) => {
            let Proposed::Damage {
                source,
                deathtouch,
                lifelink,
                target,
                amount,
                combat,
            } = ev
            else {
                unreachable!("shields only apply to damage")
            };
            let remaining = st
                .shields
                .iter()
                .find(|s| s.id == id)
                .map_or(0, |s| s.remaining);
            let n = remaining.min(amount);
            crate::actions::execute(st, crate::actions::GameAction::ConsumeShield { id, n });
            st.emit(crate::events::Event::DamagePrevented { target, amount: n });
            (amount > n).then_some(Proposed::Damage {
                source,
                deathtouch,
                lifelink,
                target,
                amount: amount - n,
                combat,
            })
        }
        RepKey::CommanderReturn => {
            let mut ev = ev;
            if let Proposed::Move {
                to,
                position,
                shuffle,
                counters,
                tapped,
                attach_to,
                ..
            } = &mut ev
            {
                *to = Zone::Command;
                *position = LibraryPosition::Top;
                *shuffle = false;
                counters.clear();
                *tapped = false;
                *attach_to = None;
            }
            Some(ev)
        }
        RepKey::Lasting { id, index } => {
            let e = st
                .lasting
                .iter()
                .find(|e| e.id == id)
                .expect("a lasting effect in force");
            let source = e.source;
            let AbilityDef::Replacement { with, .. } =
                st.catalog.ability(e.abilities[index as usize])
            else {
                unreachable!()
            };
            let with = with.clone();
            st.emit(crate::events::Event::Replaced { source });
            replace(st, ev, with)
        }
        RepKey::Static {
            source, ability, ..
        } => {
            let AbilityDef::Replacement { with, .. } = st.catalog.ability(ability) else {
                unreachable!()
            };
            let mut with = with.clone();
            // "…for each …": counted now, before the permanent has entered.
            if let ReplaceWith::EntersWithCounted { kind, amount } = &with {
                let you = match (&ev, st.object(source).zone) {
                    (_, Zone::Battlefield) => crate::layers::controller_of(st, source),
                    // Its own "enters with": whoever will control it.
                    (Proposed::Move { controller, .. }, _) => {
                        controller.unwrap_or(st.object(source).owner)
                    }
                    _ => st.object(source).owner,
                };
                let n = match amount {
                    // CR 107.3m: the X the permanent spell was cast with (0 if it isn't
                    // entering from the stack).
                    card_model::Amount::X => st.stack_object(source).map_or(0, |so| so.x),
                    amount => crate::layers::count_amount(st, amount, source, you),
                };
                with = ReplaceWith::EntersWithCounters { kind: *kind, n };
            }
            // "Enters tapped unless …" whose condition has come to hold meanwhile; "you may
            // have this enter as a copy" with nothing on the battlefield to copy.
            if entering_unchanged(st, &with, &ev) || matches!(with, ReplaceWith::EntersAsCopy { .. })
            {
                return Some(ev);
            }
            st.emit(crate::events::Event::Replaced { source });
            replace(st, ev, with)
        }
    }
}

/// The event as `with` changes it; `None` if it no longer happens (CR 615.1a: prevented
/// damage isn't dealt).
fn replace(st: &mut GameState, ev: Proposed, with: ReplaceWith) -> Option<Proposed> {
    // CR 701.20a: shown to all players first.
    if with == ReplaceWith::RevealShuffleIntoLibrary
        && let Proposed::Move { object, .. } = &ev
    {
        let player = st.object(*object).owner;
        st.emit(crate::events::Event::Revealed {
            player,
            cards: alloc::vec![*object],
        });
    }
    if with == ReplaceWith::Prevent {
        if let Proposed::Damage { target, amount, .. } = &ev {
            st.emit(crate::events::Event::DamagePrevented {
                target: *target,
                amount: *amount,
            });
        }
        return None;
    }
    Some(modify(ev, with))
}

fn modify(mut ev: Proposed, with: ReplaceWith) -> Proposed {
    match (&mut ev, &with) {
        (Proposed::CreateTokens { count, .. }, ReplaceWith::Twice)
        | (Proposed::PutCounters { count, .. }, ReplaceWith::Twice) => *count *= 2,
        (Proposed::Move { counters, .. }, ReplaceWith::Twice) => {
            for (_, n) in counters.iter_mut() {
                *n *= 2;
            }
        }
        (
            Proposed::Move {
                to,
                position,
                tapped,
                counters,
                shuffle,
                attach_to,
                ..
            },
            ReplaceWith::Exile
            | ReplaceWith::ShuffleIntoLibrary
            | ReplaceWith::RevealShuffleIntoLibrary,
        ) => {
            let library = with != ReplaceWith::Exile;
            *to = if library { Zone::Library } else { Zone::Exile };
            *position = LibraryPosition::Top;
            *tapped = false;
            counters.clear();
            *shuffle = library;
            *attach_to = None;
        }
        // "If you don't, put it into its owner's graveyard" (reached only when nothing was
        // done instead).
        (
            Proposed::Move {
                to,
                position,
                tapped,
                counters,
                shuffle,
                attach_to,
                ..
            },
            ReplaceWith::EntersOnlyIfYou(_),
        ) => {
            *to = Zone::Graveyard;
            *position = LibraryPosition::Top;
            *tapped = false;
            counters.clear();
            *shuffle = false;
            *attach_to = None;
        }
        // (Reached only when the condition doesn't hold / nothing was paid or revealed.)
        (
            Proposed::Move { tapped, .. },
            ReplaceWith::EntersTapped
            | ReplaceWith::EntersTappedUnless(_)
            | ReplaceWith::EntersTappedUnlessYou(_),
        ) => *tapped = true,
        (Proposed::Move { counters, .. }, ReplaceWith::EntersWithCounters { kind, n }) => {
            if *n > 0 {
                counters.push((*kind, *n))
            }
        }
        (e, w) => unreachable!("{w:?} can't modify {e:?} (applicability checked)"),
    }
    ev
}
