//! The turn driver (CR 500–514 (2026-11-06)).

use super::{
    CombatDamageFrame, DeclareAttackersFrame, DeclareBlockersFrame, Flow, Frame, Input,
    PriorityFrame,
};
use crate::actions::{GameAction, execute, move_object};
use crate::decision::{ChoiceKind, OptionDesc, Primitive};
use crate::events::Event;
use crate::ids::PlayerId;
use crate::sba;
use crate::state::GameState;
use crate::turn::Step;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepStage {
    /// The step begins: turn-based actions.
    Begin,
    /// The active player receives priority.
    Priority,
    /// CR 514.2–514.3a, after the cleanup discard.
    AfterCleanupDiscard,
    /// The step ends. `repeat_cleanup`: CR 514.3a "another cleanup step begins".
    End { repeat_cleanup: bool },
}

/// CR 500.1 / 103.8: begin `active`'s turn at its untap step.
pub(crate) fn begin_turn(st: &mut GameState, active: PlayerId) {
    st.turn.turn_number += 1;
    st.turn.active = active;
    st.turn.step = Step::Untap;
    for p in &mut st.players {
        p.lands_played_this_turn = 0;
    }
    // CR 302.6: permanents the active player has controlled continuously since their most
    // recent turn began are no longer summoning sick.
    for id in st.zones.battlefield.clone() {
        let o = st.object_mut(id);
        if o.controller == active {
            o.summoning_sick = false;
        }
    }
    st.emit(Event::TurnBegan {
        turn: st.turn.turn_number,
        active,
    });
}

impl StepStage {
    pub(crate) fn run(self, st: &mut GameState, input: Option<Input>) -> Flow {
        debug_assert!(input.is_none(), "step driver never suspends");
        let step = st.turn.step;
        let ap = st.turn.active;
        match self {
            StepStage::Begin => {
                st.emit(Event::StepBegan { step });
                match step {
                    Step::Untap => {
                        // CR 502.3: untap all permanents the active player controls.
                        for id in st.zones.battlefield.clone() {
                            let o = st.object(id);
                            if o.controller == ap && o.tapped {
                                execute(st, GameAction::Untap { object: id });
                            }
                        }
                        // CR 502.4: no player receives priority during the untap step.
                        Flow::then(Frame::Step(StepStage::End {
                            repeat_cleanup: false,
                        }))
                    }
                    Step::Draw => {
                        // CR 103.8a: in a two-player game the starting player skips the draw
                        // step of their first turn. CR 103.8c: not in other multiplayer games.
                        // CR 800.4j: no draw if the active player has left the game.
                        let skip = (st.turn.turn_number == 1
                            && st.num_players() == 2
                            && Some(ap) == st.turn.starting_player)
                            || !st.in_game(ap);
                        if !skip {
                            // CR 504.1
                            execute(st, GameAction::Draw { player: ap });
                        }
                        Flow::then(Frame::Step(StepStage::Priority))
                    }
                    Step::DeclareAttackers => Flow::Continue(vec![
                        Frame::Step(StepStage::Priority),
                        Frame::DeclareAttackers(DeclareAttackersFrame::new(st)),
                    ]),
                    Step::DeclareBlockers => Flow::Continue(vec![
                        Frame::Step(StepStage::Priority),
                        Frame::DeclareBlockers(DeclareBlockersFrame::new(st)),
                    ]),
                    Step::CombatDamage => Flow::Continue(vec![
                        Frame::Step(StepStage::Priority),
                        Frame::CombatDamage(CombatDamageFrame::default()),
                    ]),
                    Step::Cleanup => Flow::Continue(vec![
                        Frame::Step(StepStage::AfterCleanupDiscard),
                        Frame::CleanupDiscard { player: ap },
                    ]),
                    // CR 503.1, 505.6 (no Archenemy/Saga/Attraction actions in M0), 507
                    // (FFA: all opponents are defending players), 511.1, 513.1.
                    Step::Upkeep
                    | Step::PrecombatMain
                    | Step::BeginningOfCombat
                    | Step::EndOfCombat
                    | Step::PostcombatMain
                    | Step::End => Flow::then(Frame::Step(StepStage::Priority)),
                }
            }
            // CR 117.3a (CR 800.4j: the next player if the active player has left).
            StepStage::Priority => Flow::Continue(vec![
                Frame::Step(StepStage::End {
                    repeat_cleanup: false,
                }),
                Frame::Priority(PriorityFrame {
                    player: st.first_priority_player(),
                    passes: 0,
                }),
            ]),
            StepStage::AfterCleanupDiscard => {
                // CR 514.2: remove all marked damage (and end "until end of turn" effects).
                for id in st.zones.battlefield.clone() {
                    if st.object(id).damage_marked > 0 {
                        execute(st, GameAction::RemoveDamage { object: id });
                    }
                }
                // CR 514.3a / 704.3: check SBAs (and triggers, M2). If any were performed,
                // the active player gets priority and another cleanup step follows.
                let performed = sba::check_once(st);
                if st.is_over() {
                    return Flow::done();
                }
                if performed {
                    Flow::Continue(vec![
                        Frame::Step(StepStage::End {
                            repeat_cleanup: true,
                        }),
                        Frame::Priority(PriorityFrame {
                            player: st.first_priority_player(),
                            passes: 0,
                        }),
                    ])
                } else {
                    Flow::then(Frame::Step(StepStage::End {
                        repeat_cleanup: false,
                    }))
                }
            }
            StepStage::End { repeat_cleanup } => {
                // CR 500.5: unspent mana empties as each step and phase ends.
                for p in 0..st.num_players() {
                    execute(
                        st,
                        GameAction::EmptyManaPool {
                            player: PlayerId(p as u8),
                        },
                    );
                }
                if step == Step::EndOfCombat {
                    // CR 511.3: all creatures are removed from combat.
                    st.combat = None;
                }
                if repeat_cleanup {
                    return Flow::then(Frame::Step(StepStage::Begin));
                }
                let next = match step {
                    // CR 508.8: no attackers → skip declare blockers and combat damage.
                    Step::DeclareAttackers
                        if st.combat.as_ref().is_none_or(|c| c.attackers.is_empty()) =>
                    {
                        Some(Step::EndOfCombat)
                    }
                    s => s.next(),
                };
                match next {
                    Some(s) => st.turn.step = s,
                    None => {
                        let n = st.next_in_turn_order(ap);
                        begin_turn(st, n);
                    }
                }
                Flow::then(Frame::Step(StepStage::Begin))
            }
        }
    }
}

/// CR 514.1: the active player discards down to their maximum hand size.
pub(crate) fn cleanup_discard(st: &mut GameState, player: PlayerId, input: Option<Input>) -> Flow {
    let hand: Vec<_> = st.zones.hands[player.index()].clone();
    let excess = hand.len().saturating_sub(st.config.max_hand_size as usize);
    if let Some(inp) = input {
        for o in inp.picked() {
            let OptionDesc::Card { card } = *o else {
                unreachable!()
            };
            let new = move_object(st, card, Zone::Graveyard, LibraryPosition::Top, None);
            st.emit(Event::Discarded { player, card: new });
        }
        return Flow::done();
    }
    if excess == 0 {
        return Flow::done();
    }
    let options = hand
        .into_iter()
        .map(|card| OptionDesc::Card { card })
        .collect();
    Flow::ask(
        Frame::CleanupDiscard { player },
        player,
        ChoiceKind::Discard,
        Primitive::PickN {
            min: excess,
            max: excess,
        },
        options,
    )
}
