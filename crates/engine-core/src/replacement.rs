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
        /// The face it enters the stack or battlefield with (CR 712.9: a land face played);
        /// 0 for the default (`actions::GameAction::MoveZone`).
        #[serde(default, skip_serializing_if = "is_zero")]
        face: u8,
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
        /// Other kinds of tokens created by the same event — added or substituted by
        /// replacement effects ("those tokens plus a Food token", "instead create a Clue, a
        /// Food and a Treasure"), so that further effects see one event (CR 616.1f, D-073).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        more: Vec<(CardDefId, u32)>,
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
    /// One token, created by a [`Proposed::CreateTokens`] event whose own replacement
    /// effects have all been applied, entering the battlefield (CR 111.2, 616.1g: entering
    /// is an event contained in creating it). What modifies how a permanent enters applies
    /// to it as to a card moved there (CR 111.6, 614.1c–d, 614.12): judged as the token
    /// would exist on the battlefield (D-073).
    TokenEnters {
        token: CardDefId,
        controller: PlayerId,
        tapped: bool,
        counters: Vec<(CounterKind, u32)>,
        /// A token that's a copy (CR 707.2): what it copied, and what comes with it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        copy: Option<alloc::boxed::Box<TokenCopy>>,
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
    /// The source of the spell or ability that creates them, its controller, and the card
    /// its text is written on (CR 611.2).
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
            face: 0,
            copy: None,
        }
    }
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

/// The face `object` would have up on the battlefield after a move to it with `face`
/// (CR 712.10–712.11): what its own "enters" replacement effects are read from (CR 614.12).
pub(crate) fn entering_face(st: &GameState, object: ObjectId, face: u8) -> u8 {
    let o = st.object(object);
    match face {
        0 if o.zone == Zone::Stack => o.face,
        f => f,
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
        Proposed::CreateTokens { controller, .. } | Proposed::TokenEnters { controller, .. } => {
            *controller
        }
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
    // CR 707.5, 614.12: a token that's a copy has the copied "enters …" abilities. (Not
    // "if this would enter, you may … instead": a token has no graveyard to go to instead,
    // and no such card is copied as a token here.)
    if let Proposed::TokenEnters { copy: Some(_), .. } = &p.event {
        for ability in entering_abilities(st, &p.event) {
            if let AbilityDef::Replacement {
                replaces: Replaces::ThisEnters,
                with,
            } = st.catalog.ability(ability)
                && !matches!(with, ReplaceWith::EntersOnlyIfYou(_))
                && !entering_unchanged(st, with, &p.event)
            {
                out.push(RepKey::Static {
                    source: crate::copy::ENTERING_TOKEN,
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
        // The face it has up there: off the battlefield and stack only its front face
        // (CR 712.6a).
        for (i, a) in st.card_def(*object).abilities.iter().enumerate() {
            if let AbilityDef::Replacement {
                replaces: Replaces::ThisToGraveyard,
                ..
            } = a
            {
                out.push(RepKey::Static {
                    source: *object,
                    ability: AbilityId::printed(st.face_card(*object), i as u16),
                    nth: 0,
                });
            }
        }
    }
    // CR 702.34a: "If the flashback cost was paid, exile this card instead of putting it
    // anywhere else any time it would leave the stack."
    if let Proposed::Move { object, to, .. } = &p.event
        && *to != Zone::Exile
        && let Some(ability) = crate::frames::altcost::exiled_on_leaving(st, *object)
    {
        out.push(RepKey::Static {
            source: *object,
            ability,
            nth: 0,
        });
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
    match ev {
        Proposed::Move {
            object, copy, face, ..
        } => {
            let Some(o) = st.objects.get(object) else {
                return Vec::new();
            };
            match copy {
                Some(v) => crate::copy::characteristics(&st.catalog, v, o.owner).abilities,
                // The face it will have up there (CR 712.9–712.11).
                None => {
                    let face = entering_face(st, *object, *face);
                    crate::copy::base(st, *object, face, o.owner).abilities
                }
            }
        }
        // A token that's a copy has the copied text's (CR 707.5, 707.8a example): a token's
        // own definition never has such abilities (card-db).
        Proposed::TokenEnters {
            copy: Some(c),
            controller,
            ..
        } => crate::copy::characteristics(&st.catalog, &c.values, *controller).abilities,
        _ => Vec::new(),
    }
}

/// The object entering with this event — the card moved, or [`crate::copy::ENTERING_TOKEN`]
/// for a token — and whether it enters tapped so far.
fn entering_object(ev: &Proposed) -> Option<(ObjectId, bool)> {
    match ev {
        Proposed::Move { object, tapped, .. } => Some((*object, *tapped)),
        Proposed::TokenEnters { tapped, .. } => Some((crate::copy::ENTERING_TOKEN, *tapped)),
        _ => None,
    }
}

/// The player a permanent entering with this event will enter under the control of
/// (CR 110.2, 110.2a): "you" for its own "as this enters" abilities (CR 109.5).
pub fn entering_controller(st: &GameState, ev: &Proposed) -> Option<PlayerId> {
    match ev {
        Proposed::TokenEnters { controller, .. } => Some(*controller),
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
    let Some(you) = entering_controller(st, ev) else {
        return false;
    };
    let object = match ev {
        Proposed::Move { object, .. } => *object,
        _ => crate::copy::ENTERING_TOKEN,
    };
    crate::triggers::condition_holds(st, Some(cond), you, object)
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
            let (object, tapped) = entering_object(ev)?;
            let mut options: Vec<OptionDesc> = crate::layers::matching(st, of, object, player)
                .into_iter()
                .filter(|c| *c != object)
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
                    tapped,
                },
                options,
            ));
        }
        _ => return None,
    };
    let player = entering_controller(st, ev)?;
    let (object, tapped) = entering_object(ev)?;
    let (object, tapped) = (&object, &tapped);
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
        if let OptionDesc::Card { card } = answer {
            let values = crate::copy::values(st, *card).with(ability.card, &except);
            match &mut ev {
                Proposed::Move {
                    copy, tapped: t, ..
                } => {
                    *copy = Some(alloc::boxed::Box::new(values));
                    *t |= tapped;
                }
                // CR 707.8a example: a token copy of a Clone becomes a copy of what it
                // chose; what comes with the token stays.
                Proposed::TokenEnters {
                    copy: Some(c),
                    tapped: t,
                    ..
                } => {
                    c.values = values;
                    *t |= tapped;
                }
                _ => {}
            }
            if source != crate::copy::ENTERING_TOKEN {
                st.emit(crate::events::Event::Replaced { source });
            }
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
    if source != crate::copy::ENTERING_TOKEN {
        st.emit(crate::events::Event::Replaced { source });
    }
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
                    (DamageTo::Permanent(f), DamageTarget::Object(o))
                    | (DamageTo::Either { permanent: f, .. }, DamageTarget::Object(o)) => {
                        damage_object_hit(st, f, *o, who)
                    }
                    (DamageTo::Either { player: w, .. }, DamageTarget::Player(p)) => match w {
                        card_model::Who::Any => true,
                        card_model::Who::You => *p == you,
                        card_model::Who::Opponent => *p != you,
                    },
                    _ => false,
                }
        }
        (
            Replaces::CreateTokens,
            Proposed::CreateTokens {
                controller, count, ..
            },
        ) => *controller == you && *count > 0,
        // CR 119.10: no event for 0 life.
        (Replaces::GainLife(w), Proposed::GainLife { player, amount }) => {
            *amount > 0
                && match w {
                    card_model::Who::Any => true,
                    card_model::Who::You => *player == you,
                    card_model::Who::Opponent => *player != you,
                }
        }
        // CR 614.16: at least one token of the event is of a kind the effect is about.
        (
            Replaces::Tokens { token, .. },
            ev @ Proposed::CreateTokens {
                controller,
                token: first,
                copy,
                ..
            },
        ) => token_kinds(ev).iter().any(|&(t, n)| {
            n > 0 && token_hit(st, t, *controller, token, who, copy_of(copy, *first, t))
        }),
        (
            Replaces::Counters { on, kind, .. },
            Proposed::PutCounters {
                object,
                kind: k,
                count,
            },
        ) => {
            *count > 0
                && kind.is_none_or(|kind| kind == *k)
                && st.objects.get(object).is_some_and(|o| {
                    o.zone == Zone::Battlefield && {
                        let (attacking, blocking) = crate::layers::combat_role(st, *object);
                        crate::layers::matches_parts(
                            on,
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
                })
        }
        // CR 122.6 / 614.12: counters it enters with, matched as it would exist on the
        // battlefield (another permanent's effect only if that one is already there).
        (
            Replaces::Counters { on, kind, .. },
            ev @ (Proposed::Move {
                to: Zone::Battlefield,
                ..
            }
            | Proposed::TokenEnters { .. }),
        ) => {
            entering_counters(ev)
                .iter()
                .any(|(k, n)| *n > 0 && kind.is_none_or(|kind| kind == *k))
                && entering_matches(st, ev, on, source, you)
        }
        // A token entering (CR 111.6): as a card moved onto the battlefield (the arms for
        // `Proposed::Move` below).
        (Replaces::OtherEnters(sel), ev @ Proposed::TokenEnters { .. }) => {
            entering_matches(st, ev, sel, source, you)
        }
        (
            Replaces::PutCountersOnYourPermanents,
            Proposed::TokenEnters {
                controller,
                counters,
                ..
            },
        ) => !counters.is_empty() && *controller == you,
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
            ev @ Proposed::Move {
                to: Zone::Battlefield,
                ..
            },
        ) => entering_matches(st, ev, sel, source, you),
        _ => false,
    }
}

/// The counters a permanent would enter the battlefield with (CR 122.6).
fn entering_counters(ev: &Proposed) -> &[(CounterKind, u32)] {
    match ev {
        Proposed::Move { counters, .. } | Proposed::TokenEnters { counters, .. } => counters,
        _ => &[],
    }
}

/// Does the permanent `ev` would put onto the battlefield match `sel` as it would exist
/// there (CR 614.12), for an ability of `source` controlled by `you`? A card or token
/// moving there from another zone, or a token being created (CR 111.2, 111.6). Never the
/// ability's own source: its own entering is `Replaces::ThisEnters`.
fn entering_matches(
    st: &GameState,
    ev: &Proposed,
    sel: &card_model::Selector,
    source: ObjectId,
    you: PlayerId,
) -> bool {
    match ev {
        Proposed::Move { object, .. } if *object == source => false,
        _ => as_it_would_exist(st, ev)
            .is_some_and(|(c, facts)| crate::layers::matches_parts(sel, &c, facts, source, you)),
    }
}

/// The permanent `ev` would put onto the battlefield, as it would exist there (CR 614.12):
/// with the counters and tapped state the event has so far, the continuous effects of its
/// own static abilities and those that already exist and would apply to it. Worked out on
/// a copy of the game in which the event has happened (D-073); `None` if `ev` puts no
/// permanent onto the battlefield. Its id is the copy's: never an object of the game.
fn as_it_would_exist(
    st: &GameState,
    ev: &Proposed,
) -> Option<(crate::layers::Characteristics, crate::layers::Facts)> {
    use crate::actions::{ActionResult, GameAction, execute};
    let action = match ev {
        Proposed::Move {
            object,
            to: Zone::Battlefield,
            position,
            controller,
            tapped,
            counters,
            face,
            copy,
            ..
        } => {
            if st.objects.get(object)?.zone == Zone::Battlefield {
                return None;
            }
            // (The face it enters with, as `perform` will move it: CR 712.8–712.11.)
            GameAction::MoveZone {
                object: *object,
                to: Zone::Battlefield,
                position: *position,
                controller: *controller,
                tapped: *tapped,
                counters: counters.clone(),
                face: *face,
                copy: copy.clone(),
            }
        }
        // Its characteristics are those its creator defines (CR 111.3): for a copy, the
        // copiable values (CR 707.2).
        Proposed::TokenEnters {
            token,
            controller,
            tapped,
            counters,
            copy,
        } => GameAction::CreateToken {
            token: *token,
            controller: *controller,
            tapped: *tapped,
            counters: counters.clone(),
            copy: copy
                .as_ref()
                .map(|c| alloc::boxed::Box::new(c.values.clone())),
        },
        _ => return None,
    };
    let mut copy = st.clone();
    let ActionResult::Moved(id) = execute(&mut copy, action) else {
        return None;
    };
    let o = copy.object(id);
    let facts = crate::layers::Facts {
        id,
        controller: crate::layers::controller_of(&copy, id),
        owner: o.owner,
        is_token: o.is_token,
        tapped: false,
        attacking: false,
        blocking: false,
    };
    Some((crate::layers::chars(&copy, id), facts))
}

/// Every kind of token an event creates, with how many (CR 616.1f: one event).
pub fn token_kinds(ev: &Proposed) -> Vec<(CardDefId, u32)> {
    match ev {
        Proposed::CreateTokens {
            token, count, more, ..
        } => core::iter::once((*token, *count))
            .chain(more.iter().copied())
            .collect(),
        _ => Vec::new(),
    }
}

/// Is a token of kind `token`, about to be created under `controller`'s control, what
/// `sel` says (for the ability of `who`)? Judged as it would be created: its printed
/// characteristics (CR 111.4).
fn token_hit(
    st: &GameState,
    token: CardDefId,
    controller: PlayerId,
    sel: &card_model::Selector,
    who: Bearer<'_>,
    copy: Option<&crate::copy::Copiable>,
) -> bool {
    // A token that's a copy has the copiable values (CR 707.2).
    let c = match copy {
        Some(v) => crate::copy::characteristics(&st.catalog, v, controller),
        None => crate::layers::Characteristics::printed(token, st.catalog.get(token), controller),
    };
    crate::layers::matches_parts(
        sel,
        &c,
        crate::layers::Facts {
            // Not an object yet: never the ability's source.
            id: ObjectId(u32::MAX),
            controller,
            owner: controller,
            is_token: true,
            tapped: false,
            attacking: false,
            blocking: false,
        },
        who.source,
        who.controller,
    )
}

/// The copiable values of the tokens of kind `t` of an event whose first kind is `first`.
fn copy_of(
    copy: &Option<alloc::boxed::Box<TokenCopy>>,
    first: CardDefId,
    t: CardDefId,
) -> Option<&crate::copy::Copiable> {
    copy.as_ref().filter(|_| t == first).map(|c| &c.values)
}

/// The event after a token replacement effect — about tokens matching `token` — has
/// modified it. Kinds that end up the same are merged, in first-seen order.
fn replace_tokens(
    st: &GameState,
    ev: Proposed,
    token: &card_model::Selector,
    who: Bearer<'_>,
    with: &ReplaceWith,
) -> Proposed {
    let Proposed::CreateTokens {
        controller,
        token: first,
        ref copy,
        ..
    } = ev
    else {
        unreachable!("a token replacement on {ev:?}")
    };
    let copy = copy.clone();
    let kinds = token_kinds(&ev);
    let hit = |t: CardDefId| token_hit(st, t, controller, token, who, copy_of(&copy, first, t));
    let find = |key: &alloc::string::String| st.catalog.find(key).expect("validated by card-db");
    let mut out: Vec<(CardDefId, u32)> = Vec::new();
    let mut add = |t: CardDefId, n: u32| match out.iter_mut().find(|(k, _)| *k == t) {
        Some((_, m)) => *m = m.saturating_add(n),
        None => out.push((t, n)),
    };
    match with {
        ReplaceWith::Scale(c) => {
            for (t, n) in kinds {
                add(t, if hit(t) { c.apply(n) } else { n });
            }
        }
        ReplaceWith::Twice => {
            for (t, n) in kinds {
                add(t, if hit(t) { n.saturating_mul(2) } else { n });
            }
        }
        ReplaceWith::AddTokens { tokens, n } => {
            let those = kinds
                .iter()
                .filter(|(t, _)| hit(*t))
                .fold(0u32, |a, (_, n)| a.saturating_add(*n));
            for (t, n) in kinds {
                add(t, n);
            }
            for key in tokens {
                add(find(key), n.unwrap_or(those));
            }
        }
        ReplaceWith::ReplaceTokens(tokens) => {
            for (t, n) in kinds {
                if hit(t) {
                    for key in tokens {
                        add(find(key), n);
                    }
                } else {
                    add(t, n);
                }
            }
        }
        w => unreachable!("{w:?} doesn't modify tokens"),
    }
    // The copies (CR 707) stay copies while the kind they are is still the first; a
    // replacement that substituted them made other tokens.
    let replaced = matches!(with, ReplaceWith::ReplaceTokens(_)) && hit(first);
    let (token, count) = out.remove(0);
    Proposed::CreateTokens {
        token,
        count,
        controller,
        more: out,
        copy: copy.filter(|_| token == first && !replaced),
    }
}

/// The event after a counter replacement effect — about counters of `kind` (any, if
/// `None`) — has modified it: "twice that many", "that many plus one" of each kind.
fn replace_counters(ev: Proposed, kind: Option<CounterKind>, c: card_model::Calc) -> Proposed {
    let ours = |k: CounterKind| kind.is_none_or(|kind| kind == k);
    let mut ev = ev;
    match &mut ev {
        Proposed::PutCounters { kind: k, count, .. } if ours(*k) => *count = c.apply(*count),
        Proposed::Move { counters, .. } | Proposed::TokenEnters { counters, .. } => {
            for (k, n) in counters.iter_mut() {
                if ours(*k) && *n > 0 {
                    *n = c.apply(*n);
                }
            }
        }
        _ => {}
    }
    ev
}

/// What a `Replaces` about tokens or counters makes of `with` on `ev`, if that is what
/// it is (D-073); `None` for every other replacement effect.
fn replace_amounts(
    st: &GameState,
    who: Bearer<'_>,
    replaces: &Replaces,
    with: &ReplaceWith,
    ev: &Proposed,
) -> Option<Proposed> {
    match (replaces, with) {
        (Replaces::Tokens { token, .. }, _) => {
            Some(replace_tokens(st, ev.clone(), token, who, with))
        }
        (Replaces::Counters { kind, .. }, ReplaceWith::Scale(c)) => {
            Some(replace_counters(ev.clone(), *kind, *c))
        }
        // CR 614.1a, 120.4b: "it deals double that damage / that much damage plus 2
        // instead" — to this recipient (each recipient's damage is its own event). Doubled
        // to nothing ("minus one" of 1) is no damage (CR 614.7a): the event stays, with 0,
        // and deals none.
        (Replaces::Damage { .. }, ReplaceWith::Scale(c)) => {
            let mut ev = ev.clone();
            if let Proposed::Damage { amount, .. } = &mut ev {
                *amount = c.apply(*amount);
            }
            Some(ev)
        }
        // "You gain twice that much life instead."
        (Replaces::GainLife(_), ReplaceWith::Scale(c)) => {
            let mut ev = ev.clone();
            if let Proposed::GainLife { amount, .. } = &mut ev {
                *amount = c.apply(*amount);
            }
            Some(ev)
        }
        (Replaces::Counters { kind, .. }, ReplaceWith::Twice) => Some(replace_counters(
            ev.clone(),
            *kind,
            card_model::Calc::Times(2),
        )),
        _ => None,
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
            let AbilityDef::Replacement { with, replaces } =
                st.catalog.ability(e.abilities[index as usize])
            else {
                unreachable!()
            };
            let who = Bearer {
                source,
                controller: e.controller,
                about: e.about.as_slice(),
            };
            if let Some(ev) = replace_amounts(st, who, replaces, with, &ev) {
                st.emit(crate::events::Event::Replaced { source });
                return Some(ev);
            }
            let with = with.clone();
            st.emit(crate::events::Event::Replaced { source });
            replace(st, ev, with)
        }
        RepKey::Static {
            source, ability, ..
        } => {
            // CR 702.34a: a flashback spell leaving the stack is exiled instead.
            if let AbilityDef::AlternativeCost(_) = st.catalog.ability(ability) {
                let mut ev = ev;
                if let Proposed::Move {
                    to,
                    position,
                    shuffle,
                    controller,
                    ..
                } = &mut ev
                {
                    *to = Zone::Exile;
                    *position = LibraryPosition::Top;
                    *shuffle = false;
                    *controller = None;
                }
                st.emit(crate::events::Event::Replaced { source });
                return Some(ev);
            }
            let AbilityDef::Replacement { with, replaces } = st.catalog.ability(ability) else {
                unreachable!()
            };
            if st
                .objects
                .get(&source)
                .is_some_and(|o| o.zone == Zone::Battlefield)
            {
                let who = Bearer {
                    source,
                    controller: crate::layers::controller_of(st, source),
                    about: &[],
                };
                if let Some(ev) = replace_amounts(st, who, replaces, with, &ev) {
                    st.emit(crate::events::Event::Replaced { source });
                    return Some(ev);
                }
            }
            let mut with = with.clone();
            // "…for each …": counted now, before the permanent has entered.
            if let ReplaceWith::EntersWithCounted { kind, amount } = &with {
                let you = match (&ev, st.objects.get(&source).map(|o| o.zone)) {
                    (_, Some(Zone::Battlefield)) => crate::layers::controller_of(st, source),
                    // Its own "enters with": whoever will control it.
                    (Proposed::Move { controller, .. }, _) => {
                        controller.unwrap_or(st.object(source).owner)
                    }
                    (Proposed::TokenEnters { controller, .. }, _) => *controller,
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
            if entering_unchanged(st, &with, &ev)
                || matches!(with, ReplaceWith::EntersAsCopy { .. })
            {
                return Some(ev);
            }
            if source != crate::copy::ENTERING_TOKEN {
                st.emit(crate::events::Event::Replaced { source });
            }
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
        (Proposed::CreateTokens { count, more, .. }, ReplaceWith::Twice) => {
            *count = count.saturating_mul(2);
            for (_, n) in more.iter_mut() {
                *n = n.saturating_mul(2);
            }
        }
        (Proposed::PutCounters { count, .. }, ReplaceWith::Twice) => {
            *count = count.saturating_mul(2)
        }
        (
            Proposed::Move { counters, .. } | Proposed::TokenEnters { counters, .. },
            ReplaceWith::Twice,
        ) => {
            for (_, n) in counters.iter_mut() {
                *n = n.saturating_mul(2);
            }
        }
        // A token entering (CR 111.6): another permanent's "[these] enter tapped / with
        // counters" (its own "this enters …" can't be on a token: card-db, D-073).
        (
            Proposed::TokenEnters { tapped, .. },
            ReplaceWith::EntersTapped
            | ReplaceWith::EntersTappedUnless(_)
            | ReplaceWith::EntersTappedUnlessYou(_),
        ) => *tapped = true,
        (Proposed::TokenEnters { counters, .. }, ReplaceWith::EntersWithCounters { kind, n }) => {
            if *n > 0 {
                counters.push((*kind, *n))
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

/// CR 101.2, 701.6a, 614.17: a spell that "can't be countered" — by an ability of its own
/// ("this spell can't be countered", which works on the stack) or of a permanent
/// ("creature spells you control can't be countered", read off what permanents have now,
/// CLAUDE.md invariant 4). Countering it does nothing to it (D-073).
pub fn cant_be_countered(st: &GameState, spell: ObjectId) -> bool {
    let Some(o) = st.objects.get(&spell) else {
        return false;
    };
    if o.zone != Zone::Stack
        || !st
            .stack
            .iter()
            .any(|s| s.id == spell && matches!(s.kind, crate::stack::StackKind::Spell))
    {
        return false;
    }
    let own = abilities(st, spell)
        .into_iter()
        .any(|(_, a)| matches!(a, AbilityDef::CantBeCountered { spells: None }));
    own || st.zones.battlefield.iter().any(|&source| {
        abilities(st, source).into_iter().any(|(_, a)| {
            let AbilityDef::CantBeCountered { spells: Some(sel) } = a else {
                return false;
            };
            let c = crate::layers::chars(st, spell);
            crate::layers::matches_parts(
                sel,
                &c,
                crate::layers::Facts {
                    id: spell,
                    controller: c.controller,
                    owner: o.owner,
                    is_token: false,
                    tapped: false,
                    attacking: false,
                    blocking: false,
                },
                source,
                crate::layers::controller_of(st, source),
            )
        })
    })
}
