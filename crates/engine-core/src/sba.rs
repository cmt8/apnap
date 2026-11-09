//! State-based actions (CR 704 (2026-11-06); verbatim text in docs/rules-notes/704-sba.md).
//!
//! `gather` looks at ONE snapshot of the game and returns every applicable SBA as proposed
//! events (plus legend-rule groups that need a choice). `frames::StateCheckFrame` asks the
//! legend-rule choices, then performs everything simultaneously as one event batch through
//! the replacement pipeline (CR 704.3; destruction is replaceable, 704.5g). SBAs are never
//! checked during resolution: only when a player would receive priority (CR 117.5) and in
//! the cleanup step (CR 514.3a).
//!
//! Implemented: 704.5a, b, d, f, g, h, j, q. (704.5i and 704.5m–p need planeswalkers and
//! attachments: D-023.)

use crate::actions::{GameAction, execute};
use crate::events::{Event, LossReason};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::replacement::{MoveCause, Proposed};
use crate::state::{GameResult, GameState};
use crate::zones::Zone;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use card_model::{CardType, CounterKind, Supertype};

#[derive(Debug, Default)]
pub(crate) struct Gathered {
    pub events: Vec<Proposed>,
    /// CR 704.5j: (controller, legendary permanents sharing a name).
    pub legend: Vec<(PlayerId, Vec<ObjectId>)>,
}

pub(crate) fn gather(st: &mut GameState) -> Gathered {
    let mut g = Gathered::default();
    for p in &st.players {
        if p.has_lost {
            continue;
        }
        if p.life <= 0 {
            // CR 704.5a
            g.events.push(Proposed::LoseGame {
                player: p.id,
                reason: LossReason::ZeroLife,
            });
        } else if p.drew_from_empty_library {
            // CR 704.5b
            g.events.push(Proposed::LoseGame {
                player: p.id,
                reason: LossReason::DrewFromEmptyLibrary,
            });
        }
    }
    // 704.5b and 704.5h look back only to the previous check.
    for p in &mut st.players {
        p.drew_from_empty_library = false;
    }

    // CR 704.5d: a token in a zone other than the battlefield ceases to exist.
    for o in st.objects.values() {
        if o.is_token && o.zone != Zone::Battlefield {
            g.events.push(Proposed::CeaseToExist { object: o.id });
        }
    }

    let mut legends: BTreeMap<(PlayerId, String), Vec<ObjectId>> = BTreeMap::new();
    let mut dt_flags: Vec<ObjectId> = Vec::new();
    for &id in &st.zones.battlefield {
        let c = chars(st, id);
        let o = st.object(id);
        if o.deathtouch_damage {
            dt_flags.push(id);
        }
        if c.is(CardType::Creature) {
            let toughness = c.toughness.unwrap_or(0);
            if toughness <= 0 {
                // CR 704.5f
                g.events
                    .push(Proposed::moving(id, Zone::Graveyard, MoveCause::Rule));
            } else if o.damage_marked >= toughness as u32
                || (o.deathtouch_damage && o.damage_marked > 0)
            {
                // CR 704.5g lethal damage / 704.5h deathtouch: destroyed.
                g.events
                    .push(Proposed::moving(id, Zone::Graveyard, MoveCause::Destroy));
            }
        }
        if c.supertypes.contains(&Supertype::Legendary) {
            legends
                .entry((o.controller, c.name.clone()))
                .or_default()
                .push(id);
        }
        // CR 704.5q: +1/+1 and -1/-1 counters annihilate.
        let n = o
            .counters_of(CounterKind::PlusOne)
            .min(o.counters_of(CounterKind::MinusOne));
        if n > 0 {
            for kind in [CounterKind::PlusOne, CounterKind::MinusOne] {
                g.events.push(Proposed::RemoveCounters {
                    object: id,
                    kind,
                    n,
                });
            }
        }
    }
    for id in dt_flags {
        st.object_mut(id).deathtouch_damage = false;
    }
    g.legend = legends
        .into_iter()
        .filter(|(_, ids)| ids.len() >= 2)
        .map(|((p, _), ids)| (p, ids))
        .collect();
    g
}

/// After a batch that made players lose: end the game if at most one player remains
/// (CR 104.2a / 104.4a); otherwise each loser leaves the game (CR 800.4a).
pub(crate) fn settle_losses(st: &mut GameState, lost: Vec<PlayerId>) {
    let remaining: Vec<PlayerId> = st
        .players
        .iter()
        .filter(|p| !p.has_lost)
        .map(|p| p.id)
        .collect();
    match remaining.as_slice() {
        [] => finish(st, None),
        [w] => finish(st, Some(*w)),
        _ => {
            for player in lost {
                execute(st, GameAction::LeaveGame { player });
            }
        }
    }
}

fn finish(st: &mut GameState, winner: Option<PlayerId>) {
    if st.result.is_none() {
        st.result = Some(GameResult { winner });
        st.emit(Event::GameOver { winner });
    }
}
