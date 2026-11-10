//! The replacement pipeline as a frame (D-024): a batch of proposed events goes through
//! CR 614–616, suspending for `ChooseReplacement` when the affected player must choose,
//! and is then performed simultaneously. Text: docs/rules-notes/614-616-replacement-prevention.md.

use super::{Flow, Frame, Input};
use crate::actions::{GameAction, execute};
use crate::decision::{ChoiceKind, OptionDesc};
use crate::events::Event;
use crate::ids::ObjectId;
use crate::layers::controller_of;
use crate::replacement::{
    MoveCause, Pending, Proposed, RepKey, affected_player, applicable, apply,
};
use crate::stack::StackKind;
use crate::state::GameState;
use crate::triggers::{Lki, detect_events, snapshot};
use crate::zones::Zone;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyFrame {
    /// Events still being modified; the front one is in progress.
    pub queue: Vec<Pending>,
    /// Fully modified events, performed together at the end.
    pub ready: Vec<Proposed>,
}

impl ApplyFrame {
    /// CR 616.1: when several players must choose at once, they do so in APNAP order — so
    /// events are processed grouped by affected player, in APNAP order (stable within a player).
    pub fn new(st: &GameState, events: Vec<Proposed>) -> Self {
        let order = st.apnap_order();
        let rank = |p| order.iter().position(|q| *q == p).unwrap_or(order.len());
        let mut queue: Vec<(usize, Pending)> = events
            .into_iter()
            .map(|event| {
                (
                    rank(affected_player(st, &event)),
                    Pending {
                        event,
                        applied: Vec::new(),
                    },
                )
            })
            .collect();
        queue.sort_by_key(|(r, _)| *r);
        ApplyFrame {
            queue: queue.into_iter().map(|(_, p)| p).collect(),
            ready: Vec::new(),
        }
    }

    /// Apply `key` to the front event (CR 614.5: record it so it can't apply again).
    fn apply_front(&mut self, st: &mut GameState, key: RepKey) {
        let mut p = self.queue.remove(0);
        p.applied.push(key);
        if let Some(event) = apply(st, key, p.event) {
            self.queue.insert(
                0,
                Pending {
                    event,
                    applied: p.applied,
                },
            );
        }
    }

    pub(crate) fn run(mut self, st: &mut GameState, input: Option<Input>) -> Flow {
        if let Some(inp) = input {
            match inp.picked_one().cloned() {
                Some(OptionDesc::Replacement { source, index }) => {
                    self.apply_front(st, RepKey::Static { source, index })
                }
                Some(OptionDesc::Shield { id }) => self.apply_front(st, RepKey::Shield(id)),
                Some(OptionDesc::Accept) => self.apply_front(st, RepKey::CommanderReturn),
                // Declining an optional effect: it won't be offered again for this event.
                Some(OptionDesc::Decline) => self.queue[0].applied.push(RepKey::CommanderReturn),
                o => unreachable!("not a replacement option: {o:?}"),
            }
        }
        while let Some(front) = self.queue.first() {
            let apps = applicable(st, front);
            match apps.as_slice() {
                [] => {
                    let p = self.queue.remove(0);
                    self.ready.push(p.event);
                }
                // CR 616.1f: one applicable (mandatory) effect simply applies; then re-check.
                [only] if !only.optional() => self.apply_front(st, *only),
                // CR 616.1 / 616.1e: the affected player chooses one to apply.
                many => {
                    let player = affected_player(st, &front.event);
                    let mut options: Vec<OptionDesc> = many
                        .iter()
                        .map(|k| match *k {
                            RepKey::Static { source, index } => {
                                OptionDesc::Replacement { source, index }
                            }
                            RepKey::Shield(id) => OptionDesc::Shield { id },
                            RepKey::CommanderReturn => OptionDesc::Accept,
                        })
                        .collect();
                    // An optional effect may be declined (CR 903.9b "may").
                    if many.iter().any(|k| k.optional()) {
                        options.push(OptionDesc::Decline);
                    }
                    return Flow::ask_one(
                        Frame::Apply(self),
                        player,
                        ChoiceKind::ChooseReplacement,
                        options,
                    );
                }
            }
        }
        perform(st, core::mem::take(&mut self.ready));
        Flow::done()
    }
}

/// Perform fully modified events simultaneously, then detect triggers (CR 603.2g).
pub(crate) fn perform(st: &mut GameState, events: Vec<Proposed>) {
    // CR 704.8 / 603.10a: last known information for everything leaving the battlefield,
    // captured before any of the batch happens.
    let mut lki: BTreeMap<ObjectId, Lki> = BTreeMap::new();
    for ev in &events {
        if let Proposed::Move { object, .. } = ev
            && st
                .objects
                .get(object)
                .is_some_and(|o| o.zone == Zone::Battlefield)
        {
            lki.insert(*object, snapshot(st, *object));
        }
    }
    // CR 608.2h: …and the object that triggered them ("it", "that creature").
    for (id, l) in &lki {
        let abilities: Vec<ObjectId> = st
            .stack
            .iter()
            .filter(|s| s.subject == Some(*id) && s.subject_lki.is_none())
            .map(|s| s.id)
            .collect();
        for ability in abilities {
            execute(
                st,
                GameAction::RecordSubjectLki {
                    ability,
                    chars: l.chars.clone(),
                },
            );
        }
    }
    // CR 113.7a: abilities on the stack remember a source that is leaving.
    for (id, l) in &lki {
        let sources: Vec<ObjectId> = st
            .stack
            .iter()
            .filter(|s| matches!(s.kind, StackKind::Ability { source, .. } if source == *id))
            .filter(|s| s.source_lki.is_none())
            .map(|s| s.id)
            .collect();
        for ability in sources {
            execute(
                st,
                GameAction::RecordSourceLki {
                    ability,
                    chars: l.chars.clone(),
                },
            );
        }
    }
    let mark = st.out.len();
    let mut lost = Vec::new();
    for ev in events {
        match ev {
            Proposed::Move {
                object,
                to,
                position,
                controller,
                cause,
                tapped,
                counters,
                shuffle,
                attach_to,
            } => {
                let Some(o) = st.objects.get(&object) else {
                    continue; // already gone (e.g. moved earlier in this batch)
                };
                // CR 111.8: a token that has left the battlefield can't move again.
                if o.is_token && o.zone != Zone::Battlefield {
                    continue;
                }
                let (owner, was_controller) = (o.owner, o.controller);
                let new = match execute(
                    st,
                    GameAction::MoveZone {
                        object,
                        to,
                        position,
                        controller,
                        tapped,
                        counters,
                    },
                ) {
                    crate::actions::ActionResult::Moved(id) => id,
                    r => unreachable!("{r:?}"),
                };
                match cause {
                    MoveCause::Discard => st.emit(Event::Discarded {
                        player: owner,
                        card: new,
                    }),
                    MoveCause::Sacrifice => st.emit(Event::Sacrificed {
                        player: was_controller,
                        object: new,
                    }),
                    MoveCause::PlayLand => st.emit(Event::LandPlayed {
                        player: controller.unwrap_or(owner),
                        land: new,
                    }),
                    _ => {}
                }
                if shuffle {
                    execute(st, GameAction::Shuffle { player: owner });
                }
                if let Some(target) = attach_to
                    && to == Zone::Battlefield
                {
                    // CR 303.4: the Aura enters attached.
                    execute(
                        st,
                        GameAction::Attach {
                            aura: new,
                            to: target,
                        },
                    );
                }
            }
            Proposed::Damage {
                source,
                deathtouch,
                lifelink,
                target,
                amount,
                combat,
            } => {
                // CR 702.15b: lifelink's life gain is part of the damage event; the source's
                // controller (its owner if it has none) gains the life.
                let gainer = lki
                    .get(&source)
                    .map(|l| l.controller)
                    .or_else(|| st.objects.get(&source).map(|o| controller_of(st, o.id)));
                execute(
                    st,
                    GameAction::Damage {
                        source,
                        target,
                        amount,
                        combat,
                        deathtouch,
                    },
                );
                if lifelink
                    && amount > 0
                    && let Some(player) = gainer
                {
                    execute(st, GameAction::GainLife { player, amount });
                }
            }
            Proposed::Draw { player } => {
                execute(st, GameAction::Draw { player });
            }
            Proposed::CreateTokens {
                token,
                count,
                controller,
            } => {
                // CR 800.4b: no tokens for a player who has left the game.
                if st.in_game(controller) {
                    for _ in 0..count {
                        execute(st, GameAction::CreateToken { token, controller });
                    }
                }
            }
            Proposed::PutCounters {
                object,
                kind,
                count,
            } => {
                if st
                    .objects
                    .get(&object)
                    .is_some_and(|o| o.zone == Zone::Battlefield)
                {
                    execute(
                        st,
                        GameAction::PutCounters {
                            object,
                            kind,
                            n: count,
                        },
                    );
                }
            }
            Proposed::GainLife { player, amount } => {
                execute(st, GameAction::GainLife { player, amount });
            }
            Proposed::LoseLife { player, amount } => {
                execute(st, GameAction::LoseLife { player, amount });
            }
            Proposed::LoseGame { player, reason } => {
                if st.in_game(player) {
                    execute(st, GameAction::LoseGame { player, reason });
                    lost.push(player);
                }
            }
            Proposed::RemoveCounters { object, kind, n } => {
                if st.objects.contains_key(&object) {
                    execute(st, GameAction::RemoveCounters { object, kind, n });
                }
            }
            Proposed::CeaseToExist { object } => {
                if st.objects.contains_key(&object) {
                    execute(st, GameAction::CeaseToExist { object });
                }
            }
        }
    }
    if !lost.is_empty() {
        crate::sba::settle_losses(st, lost);
    }
    let performed: Vec<Event> = st.out[mark..].to_vec();
    if !st.is_over() {
        detect_events(st, &performed, &lki);
    }
}
