//! The turn driver (CR 500–514 (2026-11-06)).

use super::{
    ApplyFrame, CombatDamageFrame, DeclareAttackersFrame, DeclareBlockersFrame, Flow, Frame, Input,
    PriorityFrame, StateCheckFrame,
};
use crate::actions::{GameAction, execute};
use crate::decision::{ChoiceKind, OptionDesc, Primitive};
use crate::events::Event;
use crate::ids::PlayerId;
use crate::replacement::{MoveCause, Proposed};
use crate::state::GameState;
use crate::triggers;
use crate::turn::Step;
use crate::zones::Zone;
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
    /// CR 514.3a, after the state check.
    CleanupChecked,
    /// The step ends. `repeat_cleanup`: CR 514.3a "another cleanup step begins".
    End { repeat_cleanup: bool },
}

/// CR 500.1 / 103.8: begin `active`'s turn at its untap step.
pub(crate) fn begin_turn(st: &mut GameState, active: PlayerId) {
    st.turn.turn_number += 1;
    st.turn.active = active;
    st.turn.step = Step::Untap;
    // "During your turn, …": static abilities can look at whose turn it is.
    st.generation += 1;
    for p in &mut st.players {
        p.lands_played_this_turn = 0;
    }
    // CR 302.6: permanents the active player has controlled continuously since their most
    // recent turn began are no longer summoning sick.
    for id in st.zones.battlefield.clone() {
        let o = st.object_mut(id);
        if o.last_controller == active {
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
                // CR 603.2b / 500.6: "at the beginning of" triggers trigger now; they go on
                // the stack the next time a player would receive priority (502.4 for untap).
                triggers::detect_step(st, step);
                match step {
                    Step::Untap => {
                        // CR 502.3: untap all permanents the active player controls.
                        for id in st.zones.battlefield.clone() {
                            let o = st.object(id);
                            if o.tapped && crate::layers::controller_of(st, id) == ap {
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
                        if skip {
                            return Flow::then(Frame::Step(StepStage::Priority));
                        }
                        // CR 504.1 (a draw is replaceable, CR 614.11).
                        let draw = ApplyFrame::new(st, vec![Proposed::Draw { player: ap }]);
                        Flow::Continue(vec![Frame::Step(StepStage::Priority), Frame::Apply(draw)])
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
                Frame::Priority(PriorityFrame::new(st.first_priority_player(), 0)),
            ]),
            StepStage::AfterCleanupDiscard => {
                // CR 514.2: simultaneously, all marked damage is removed and "until end of
                // turn" / "this turn" effects end.
                for id in st.zones.battlefield.clone() {
                    if st.object(id).damage_marked > 0 {
                        execute(st, GameAction::RemoveDamage { object: id });
                    }
                }
                execute(st, GameAction::ExpireEndOfTurn);
                // CR 514.3a / 704.3: check SBAs and triggers.
                Flow::Continue(vec![
                    Frame::Step(StepStage::CleanupChecked),
                    Frame::StateCheck(StateCheckFrame::new()),
                ])
            }
            StepStage::CleanupChecked => {
                if st.is_over() {
                    return Flow::done();
                }
                // CR 514.3a: if SBAs were performed or triggers put on the stack, the active
                // player gets priority, and then another cleanup step begins.
                if st.last_check_acted {
                    Flow::Continue(vec![
                        Frame::Step(StepStage::End {
                            repeat_cleanup: true,
                        }),
                        Frame::Priority(PriorityFrame::new(st.first_priority_player(), 0)),
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
                // CR 510.4: after a first-strike damage step comes a second damage step.
                if step == Step::CombatDamage
                    && let Some(c) = st.combat.as_mut()
                    && c.extra_damage_step
                    && c.damage_step == 0
                {
                    c.damage_step = 1;
                    return Flow::then(Frame::Step(StepStage::Begin));
                }
                if step == Step::EndOfCombat {
                    // CR 511.3: all creatures are removed from combat.
                    st.combat = None;
                    st.generation += 1;
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
        // CR 701.9a: discarded cards go to the graveyard (replaceable, e.g. "exile instead").
        let moves = inp
            .picked()
            .into_iter()
            .map(|o| match o {
                OptionDesc::Card { card } => {
                    Proposed::moving(*card, Zone::Graveyard, MoveCause::Discard)
                }
                _ => unreachable!(),
            })
            .collect();
        return Flow::then(Frame::Apply(ApplyFrame::new(st, moves)));
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
