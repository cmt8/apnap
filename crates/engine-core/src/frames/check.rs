//! The state check (CR 117.5 / 704.3): perform state-based actions until none apply, then
//! put waiting triggered abilities on the stack, and repeat until neither happens.

use super::{ApplyFrame, Flow, Frame, Input, PutTriggersFrame};
use crate::decision::{ChoiceKind, OptionDesc};
use crate::ids::{ObjectId, PlayerId};
use crate::replacement::{MoveCause, Proposed};
use crate::sba;
use crate::state::GameState;
use crate::zones::Zone;
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateCheckFrame {
    /// Whether this check performed any SBA or put any trigger on the stack.
    pub acted: bool,
    pub stage: CheckStage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStage {
    Gather,
    /// CR 704.5j: each player with legend-rule groups chooses which one to keep, in APNAP order.
    Legend {
        events: Vec<Proposed>,
        groups: Vec<(PlayerId, Vec<ObjectId>)>,
        next: usize,
    },
}

impl StateCheckFrame {
    pub(crate) fn new() -> Self {
        StateCheckFrame {
            acted: false,
            stage: CheckStage::Gather,
        }
    }

    pub(crate) fn run(mut self, st: &mut GameState, mut input: Option<Input>) -> Flow {
        loop {
            if st.is_over() {
                return Flow::done();
            }
            match core::mem::replace(&mut self.stage, CheckStage::Gather) {
                CheckStage::Gather => {
                    let g = sba::gather(st);
                    if g.events.is_empty() && g.legend.is_empty() {
                        if !st.pending_triggers.is_empty() {
                            // CR 603.3 / 117.5: triggers go on the stack, then check again.
                            self.acted = true;
                            return Flow::Continue(vec![
                                Frame::StateCheck(self),
                                Frame::PutTriggers(PutTriggersFrame::new()),
                            ]);
                        }
                        st.last_check_acted = self.acted;
                        return Flow::done();
                    }
                    self.acted = true;
                    let order = st.apnap_order();
                    let mut groups = g.legend;
                    groups.sort_by_key(|(p, _)| order.iter().position(|q| q == p));
                    self.stage = CheckStage::Legend {
                        events: g.events,
                        groups,
                        next: 0,
                    };
                }
                CheckStage::Legend {
                    mut events,
                    groups,
                    mut next,
                } => {
                    if let Some(inp) = input.take() {
                        let Some(OptionDesc::Card { card: keep }) = inp.picked_one().cloned()
                        else {
                            unreachable!()
                        };
                        for &o in &groups[next].1 {
                            if o != keep {
                                events.push(Proposed::moving(o, Zone::Graveyard, MoveCause::Rule));
                            }
                        }
                        next += 1;
                    }
                    if let Some((player, ids)) = groups.get(next).cloned() {
                        let options = ids.iter().map(|&card| OptionDesc::Card { card }).collect();
                        self.stage = CheckStage::Legend {
                            events,
                            groups,
                            next,
                        };
                        return Flow::ask_one(
                            Frame::StateCheck(self),
                            player,
                            ChoiceKind::LegendRule,
                            options,
                        );
                    }
                    // CR 704.3: all applicable SBAs are performed simultaneously.
                    let apply = ApplyFrame::new(st, events);
                    return Flow::Continue(vec![Frame::StateCheck(self), Frame::Apply(apply)]);
                }
            }
        }
    }
}
