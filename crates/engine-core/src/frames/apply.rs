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
    MoveCause, Pending, Proposed, RepKey, affected_player, applicable, apply, apply_chosen,
    entry_choice,
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
    /// An effect applying to the front event that waits for its controller's choice
    /// (CR 614.12a: "as this enters, you may pay 2 life").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choosing: Option<RepKey>,
}

impl ApplyFrame {
    /// CR 616.1: when several players must choose at once, they do so in APNAP order — so
    /// events are processed grouped by affected player, in APNAP order (stable within a player).
    pub fn new(st: &GameState, events: Vec<Proposed>) -> Self {
        let order = st.apnap_order();
        let rank = |p| order.iter().position(|q| *q == p).unwrap_or(order.len());
        let mut queue: Vec<(usize, Pending)> = events
            .into_iter()
            // CR 702.12b: a permanent with indestructible can't be destroyed — the event
            // doesn't happen (CR 614.17), so nothing can replace it either.
            .filter(|event| {
                !matches!(event, Proposed::Move { object, cause: MoveCause::Destroy, .. }
                    if crate::layers::chars(st, *object).has(card_model::Keyword::Indestructible))
            })
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
            choosing: None,
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

    /// Apply `key` to the front event, unless it asks its controller something first
    /// (CR 614.12a): then the question to suspend on.
    fn apply_or_ask(&mut self, st: &mut GameState, key: RepKey) -> Option<Flow> {
        // CR 614.13a: what moves in this batch can't be chosen for its cost.
        let moving: Vec<ObjectId> = self
            .queue
            .iter()
            .map(|p| &p.event)
            .chain(self.ready.iter())
            .filter_map(|e| match e {
                Proposed::Move { object, .. } => Some(*object),
                _ => None,
            })
            .collect();
        if let Some((player, kind, options)) = entry_choice(st, key, &self.queue[0].event, &moving)
        {
            self.choosing = Some(key);
            return Some(Flow::ask_one(
                Frame::Apply(self.clone()),
                player,
                kind,
                options,
            ));
        }
        self.apply_front(st, key);
        None
    }

    pub(crate) fn run(mut self, st: &mut GameState, input: Option<Input>) -> Flow {
        if let Some(inp) = input {
            if let Some(key) = self.choosing.take() {
                // The answer to an "as this enters" choice.
                let answer = inp.picked_one().cloned().expect("one option is picked");
                let mut p = self.queue.remove(0);
                p.applied.push(key);
                let (event, caused) = apply_chosen(st, key, p.event, &answer);
                self.queue.insert(
                    0,
                    Pending {
                        event,
                        applied: p.applied,
                    },
                );
                if !caused.is_empty() {
                    // Life paid now, before the permanent enters (CR 119.4, 614.12a); the
                    // rest of this batch carries on afterwards.
                    let pay = ApplyFrame::new(st, caused);
                    return Flow::Continue(alloc::vec![Frame::Apply(self), Frame::Apply(pay)]);
                }
            } else {
                let key = match inp.picked_one().cloned() {
                    Some(OptionDesc::Replacement {
                        source,
                        ability,
                        nth,
                    }) => Some(RepKey::Static {
                        source,
                        ability,
                        nth,
                    }),
                    Some(OptionDesc::Shield { id }) => Some(RepKey::Shield(id)),
                    Some(OptionDesc::Lasting { id, index }) => Some(RepKey::Lasting { id, index }),
                    Some(OptionDesc::Accept) => Some(RepKey::CommanderReturn),
                    // Declining an optional effect: it won't be offered again for this event.
                    Some(OptionDesc::Decline) => {
                        self.queue[0].applied.push(RepKey::CommanderReturn);
                        None
                    }
                    o => unreachable!("not a replacement option: {o:?}"),
                };
                if let Some(key) = key
                    && let Some(ask) = self.apply_or_ask(st, key)
                {
                    return ask;
                }
            }
        }
        while let Some(front) = self.queue.first() {
            let apps = applicable(st, front);
            match apps.as_slice() {
                // CR 616.1g, 111.2: creating the tokens is modified first; then each token
                // enters the battlefield, an event of its own that what modifies how
                // permanents enter applies to (D-073).
                [] if matches!(front.event, Proposed::CreateTokens { .. }) => {
                    let p = self.queue.remove(0);
                    let entering = token_entries(&p.event).into_iter().map(|event| Pending {
                        event,
                        applied: Vec::new(),
                    });
                    self.queue.splice(0..0, entering);
                }
                [] => {
                    let p = self.queue.remove(0);
                    self.ready.push(p.event);
                }
                // CR 616.1f: one applicable (mandatory) effect simply applies; then re-check.
                [only] if !only.optional() => {
                    let only = *only;
                    if let Some(ask) = self.apply_or_ask(st, only) {
                        return ask;
                    }
                }
                // CR 616.1 / 616.1e: the affected player chooses one to apply.
                many => {
                    let player = affected_player(st, &front.event);
                    let mut options: Vec<OptionDesc> = many
                        .iter()
                        .map(|k| match *k {
                            RepKey::Static {
                                source,
                                ability,
                                nth,
                            } => OptionDesc::Replacement {
                                source,
                                ability,
                                nth,
                            },
                            RepKey::Shield(id) => OptionDesc::Shield { id },
                            RepKey::Lasting { id, index } => OptionDesc::Lasting { id, index },
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

/// The most tokens of one kind one event creates. Doublers make numbers grow without
/// bound (CR 614.5 example: each applies once, but they multiply); counts saturate at
/// `u32::MAX`, and no game can hold that many objects. A deliberate limit (D-073): far
/// above any board a game is played to (the harness abandons a game at 1,000 permanents).
pub const MAX_TOKENS_AT_ONCE: u32 = 10_000;

/// One `TokenEnters` event for each token a fully modified `CreateTokens` event creates,
/// kind by kind, in order (CR 111.2: each token enters the battlefield).
fn token_entries(ev: &Proposed) -> Vec<Proposed> {
    let Proposed::CreateTokens { controller, .. } = ev else {
        return Vec::new();
    };
    let controller = *controller;
    crate::replacement::token_kinds(ev)
        .into_iter()
        .flat_map(|(token, count)| {
            // A runaway doubling can't make more objects than a game can hold; the harness
            // abandons such a game long before this (D-073).
            (0..count.min(MAX_TOKENS_AT_ONCE)).map(move |_| Proposed::TokenEnters {
                token,
                controller,
                tapped: false,
                counters: Vec::new(),
            })
        })
        .collect()
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
                    // CR 702.29c–d: cycling a card is discarding it.
                    MoveCause::Cycle => {
                        st.emit(Event::Discarded {
                            player: owner,
                            card: new,
                        });
                        st.emit(Event::Cycled {
                            player: owner,
                            card: new,
                        });
                    }
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
            // (Made into `TokenEnters` events before it is ready.)
            Proposed::CreateTokens { .. } => {}
            Proposed::TokenEnters {
                token,
                controller,
                tapped,
                counters,
            } => {
                // CR 800.4b: no tokens for a player who has left the game.
                if st.in_game(controller) {
                    execute(
                        st,
                        GameAction::CreateToken {
                            token,
                            controller,
                            tapped,
                            counters,
                        },
                    );
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
