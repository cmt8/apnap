//! The state check (CR 117.5 / 704.3): perform state-based actions until none apply, then
//! put waiting triggered abilities on the stack, and repeat until neither happens.
//!
//! Two SBAs need a player's choice before the batch is performed: the legend rule (CR 704.5j,
//! which one to keep) and the commander's return (CR 903.9a / 704.6d, "may"). Choices are
//! made in APNAP order; then everything is performed simultaneously as one batch.

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
    /// CR 704.5j legend-rule groups, then CR 903.9a commander returns, in APNAP order.
    Choices {
        events: Vec<Proposed>,
        groups: Vec<(PlayerId, Vec<ObjectId>)>,
        commanders: Vec<(PlayerId, ObjectId)>,
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
                    // Something was done on the spot (CR 704.5n): what it changed — a
                    // creature that lost its Equipment's toughness — is checked next round.
                    if g.changed
                        && g.events.is_empty()
                        && g.legend.is_empty()
                        && g.commander.is_empty()
                    {
                        self.acted = true;
                        continue;
                    }
                    if g.events.is_empty() && g.legend.is_empty() && g.commander.is_empty() {
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
                    let order = st.apnap_order();
                    let rank = |p: &PlayerId| order.iter().position(|q| q == p);
                    let mut groups = g.legend;
                    groups.sort_by_key(|(p, _)| rank(p));
                    let mut commanders: Vec<(PlayerId, ObjectId)> = g
                        .commander
                        .into_iter()
                        .map(|id| (st.object(id).owner, id))
                        .collect();
                    commanders.sort_by_key(|(p, _)| rank(p));
                    self.stage = CheckStage::Choices {
                        events: g.events,
                        groups,
                        commanders,
                        next: 0,
                    };
                }
                CheckStage::Choices {
                    mut events,
                    groups,
                    commanders,
                    mut next,
                } => {
                    if let Some(inp) = input.take() {
                        match inp.picked_one().cloned() {
                            Some(OptionDesc::Card { card: keep }) => {
                                for &o in &groups[next].1 {
                                    if o != keep {
                                        events.push(Proposed::moving(
                                            o,
                                            Zone::Graveyard,
                                            MoveCause::Rule,
                                        ));
                                    }
                                }
                            }
                            // CR 903.9a: the owner chose to put it into the command zone.
                            Some(OptionDesc::Accept) => {
                                let (_, id) = commanders[next - groups.len()];
                                events.push(Proposed::moving(id, Zone::Command, MoveCause::Rule));
                            }
                            Some(OptionDesc::Decline) => {}
                            o => unreachable!("{o:?}"),
                        }
                        next += 1;
                    }
                    if let Some((player, ids)) = groups.get(next).cloned() {
                        let options = ids.iter().map(|&card| OptionDesc::Card { card }).collect();
                        self.stage = CheckStage::Choices {
                            events,
                            groups,
                            commanders,
                            next,
                        };
                        return Flow::ask_one(
                            Frame::StateCheck(self),
                            player,
                            ChoiceKind::LegendRule,
                            options,
                        );
                    }
                    if let Some(&(owner, _)) = commanders.get(next - groups.len()) {
                        self.stage = CheckStage::Choices {
                            events,
                            groups,
                            commanders,
                            next,
                        };
                        return Flow::ask_one(
                            Frame::StateCheck(self),
                            owner,
                            ChoiceKind::CommanderToCommandZone,
                            vec![OptionDesc::Accept, OptionDesc::Decline],
                        );
                    }
                    if events.is_empty() {
                        // Only declined optional SBAs: nothing was performed (check again).
                        continue;
                    }
                    // CR 704.3: all applicable SBAs are performed simultaneously.
                    self.acted = true;
                    let apply = ApplyFrame::new(st, events);
                    return Flow::Continue(vec![Frame::StateCheck(self), Frame::Apply(apply)]);
                }
            }
        }
    }
}
