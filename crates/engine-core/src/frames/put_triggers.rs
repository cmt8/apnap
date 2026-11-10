//! Putting triggered abilities on the stack (CR 603.3 (2026-11-06)).
//!
//! CR 603.3b: each player, in APNAP order, puts the triggered abilities they control on the
//! stack in any order they choose (an `OrderTriggers` decision when they have two or more).
//! The two-part split (abilities that trigger on another ability triggering go second) has no
//! M2 card yet, so every trigger is in the first part. Each ability then goes through the
//! announce/modes/targets steps (603.3c–d) as a `CastFrame`.

use super::{CastFrame, Flow, Frame, Input};
use crate::decision::{ChoiceKind, OptionDesc, Primitive};
use crate::ids::PlayerId;
use crate::state::GameState;
use crate::triggers::PendingTrigger;
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PutTriggersFrame {
    pub started: bool,
    pub order: Vec<PlayerId>,
    pub next_player: usize,
    /// Triggers taken from the queue, not yet ordered.
    pub holding: Vec<PendingTrigger>,
    /// The triggers the current player is ordering, in option order.
    pub asking: Vec<PendingTrigger>,
    /// Ordered triggers left to put on the stack (front = next).
    pub ready: Vec<PendingTrigger>,
}

impl PutTriggersFrame {
    pub(crate) fn new() -> Self {
        PutTriggersFrame {
            started: false,
            order: Vec::new(),
            next_player: 0,
            holding: Vec::new(),
            asking: Vec::new(),
            ready: Vec::new(),
        }
    }

    pub(crate) fn run(mut self, st: &mut GameState, input: Option<Input>) -> Flow {
        if !self.started {
            self.started = true;
            self.holding = core::mem::take(&mut st.pending_triggers);
            self.order = st.apnap_order();
        }
        if let Some(inp) = input {
            // First listed is put on the stack first (so it resolves last).
            let asking = core::mem::take(&mut self.asking);
            for &i in inp.order() {
                self.ready.push(asking[i].clone());
            }
            self.next_player += 1;
        }
        while let Some(&p) = self.order.get(self.next_player) {
            let mine: Vec<PendingTrigger> = self
                .holding
                .iter()
                .filter(|t| t.controller == p)
                .cloned()
                .collect();
            self.holding.retain(|t| t.controller != p);
            if mine.len() >= 2 {
                let options = mine
                    .iter()
                    .map(|t| OptionDesc::Trigger {
                        source: t.source,
                        ability: t.ability,
                    })
                    .collect();
                self.asking = mine;
                return Flow::ask(
                    Frame::PutTriggers(self),
                    p,
                    ChoiceKind::OrderTriggers,
                    Primitive::Order,
                    options,
                );
            }
            self.ready.extend(mine);
            self.next_player += 1;
        }
        // Triggers controlled by players who left the game aren't put on the stack (800.4d).
        if self.ready.is_empty() {
            return Flow::done();
        }
        let t = self.ready.remove(0);
        Flow::Continue(vec![
            Frame::PutTriggers(self),
            Frame::Cast(CastFrame::trigger(t)),
        ])
    }
}
