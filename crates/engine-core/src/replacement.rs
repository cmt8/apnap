//! Replacement and prevention effects (CR 614–616 (2026-11-06)).
//! Text: docs/rules-notes/614-616-replacement-prevention.md.
//!
//! Every replaceable game event is first built as a [`Proposed`] event and run through
//! `frames::ApplyFrame` (D-024): the applicable effects are found here, the affected
//! player chooses among them when there is more than one (CR 616.1, `ChooseReplacement`),
//! each effect applies at most once per event (CR 614.5), applicability is re-evaluated after
//! every application (CR 616.1f, 616.2), and only then is the (modified) event performed.

use crate::events::{DamageTarget, LossReason};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::abilities;
use crate::stack::Target;
use crate::state::GameState;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec::Vec;
use card_model::{AbilityDef, CardDefId, CounterKind, ReplaceWith, Replaces};
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
        }
    }
}

/// Identifies one replacement/prevention effect, so it applies at most once per event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RepKey {
    /// Replacement ability `index` of `source` (a permanent, or the entering object itself).
    Static { source: ObjectId, index: u16 },
    /// A prevention shield (CR 615.7).
    Shield(u32),
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
        for (index, a) in abilities(st, source) {
            if let AbilityDef::Replacement { replaces, .. } = a
                && applies(st, source, replaces, &p.event)
            {
                out.push(RepKey::Static { source, index });
            }
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
        for (i, a) in st.card_def(*object).abilities.iter().enumerate() {
            if let AbilityDef::Replacement {
                replaces: Replaces::ThisEnters,
                ..
            } = a
            {
                out.push(RepKey::Static {
                    source: *object,
                    index: i as u16,
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

fn applies(st: &GameState, source: ObjectId, replaces: &Replaces, ev: &Proposed) -> bool {
    let you = crate::layers::controller_of(st, source);
    match (replaces, ev) {
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
        ) => st.objects.get(object).is_some_and(|o| !o.is_token),
        (
            Replaces::ThisDies,
            Proposed::Move {
                object,
                to: Zone::Graveyard,
                ..
            },
        ) => *object == source && st.object(source).zone == Zone::Battlefield,
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
        RepKey::Static { source, index } => {
            let AbilityDef::Replacement { with, .. } =
                &st.card_def(source).abilities[index as usize]
            else {
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
            st.emit(crate::events::Event::Replaced { source });
            Some(modify(ev, with))
        }
    }
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
            ReplaceWith::Exile | ReplaceWith::ShuffleIntoLibrary,
        ) => {
            let library = with == ReplaceWith::ShuffleIntoLibrary;
            *to = if library { Zone::Library } else { Zone::Exile };
            *position = LibraryPosition::Top;
            *tapped = false;
            counters.clear();
            *shuffle = library;
            *attach_to = None;
        }
        (Proposed::Move { tapped, .. }, ReplaceWith::EntersTapped) => *tapped = true,
        (Proposed::Move { counters, .. }, ReplaceWith::EntersWithCounters { kind, n }) => {
            if *n > 0 {
                counters.push((*kind, *n))
            }
        }
        (e, w) => unreachable!("{w:?} can't modify {e:?} (applicability checked)"),
    }
    ev
}
