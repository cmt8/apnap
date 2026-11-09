//! The continuation stack (D-002). Every point at which the engine can stop for a decision
//! is a variant here, holding its progress as plain serializable data. The driver pops the
//! top frame and runs it; a frame either continues (pushing follow-up frames, last = top)
//! or suspends (re-pushing itself with updated progress and returning a `PendingDecision`).
//!
//! Rules for frame code:
//! - A frame receives `Some(input)` only if it is the frame that suspended.
//! - All game mutation goes through `actions::execute`.
//! - Nothing that is needed after a suspension may live in a Rust local; put it in the frame.

mod apply;
mod cast;
mod check;
mod combat;
mod priority;
mod put_triggers;
mod resolve;
mod setup;
mod turn;

pub use apply::ApplyFrame;
pub use check::{CheckStage, StateCheckFrame};
pub use put_triggers::PutTriggersFrame;

pub use cast::{CastFrame, CastSource, CastStage};
pub use combat::{CombatDamageFrame, DeclareAttackersFrame, DeclareBlockersFrame};
pub use priority::PriorityFrame;
pub use resolve::{ResolveFrame, Work};
pub use setup::{MulliganFrame, MulliganStage, SetupFrame};
pub use turn::StepStage;

use crate::decision::{ChoiceKind, OptionDesc, PendingDecision, Primitive, Response};
use crate::ids::PlayerId;
use crate::state::GameState;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    /// CR 103: starting the game.
    Setup(SetupFrame),
    /// CR 103.5: mulligans.
    Mulligan(MulliganFrame),
    /// The turn driver for the current step (`GameState::turn.step`).
    Step(StepStage),
    /// CR 117: a player would receive priority.
    Priority(PriorityFrame),
    /// CR 601.2 / 602.2: casting a spell or activating an ability.
    Cast(CastFrame),
    /// CR 608: resolve the top object of the stack.
    Resolve(Box<ResolveFrame>),
    /// CR 508.1
    DeclareAttackers(DeclareAttackersFrame),
    /// CR 509.1
    DeclareBlockers(DeclareBlockersFrame),
    /// CR 510.1–510.2
    CombatDamage(CombatDamageFrame),
    /// CR 514.1
    CleanupDiscard { player: PlayerId },
    /// CR 614–616: a batch of events through the replacement pipeline, then performed.
    Apply(ApplyFrame),
    /// CR 117.5 / 704.3: SBAs and triggers until neither happens.
    StateCheck(StateCheckFrame),
    /// CR 603.3: put waiting triggered abilities on the stack.
    PutTriggers(PutTriggersFrame),
}

/// The validated answer to the decision a frame suspended on.
#[derive(Debug)]
pub(crate) struct Input {
    pub decision: PendingDecision,
    pub response: Response,
}

impl Input {
    /// Chosen options for a `PickN` decision, in the order given.
    pub fn picked(&self) -> Vec<&OptionDesc> {
        match &self.response {
            Response::Pick(idx) => idx.iter().map(|&i| &self.decision.options[i]).collect(),
            r => unreachable!("expected Pick, got {r:?} (validated earlier)"),
        }
    }

    /// The single chosen option of a `PickN { max: 1 }` decision, if any.
    pub fn picked_one(&self) -> Option<&OptionDesc> {
        self.picked().first().copied()
    }

    pub fn allocation(&self) -> &[u32] {
        match &self.response {
            Response::Allocate(v) => v,
            r => unreachable!("expected Allocate, got {r:?}"),
        }
    }

    pub fn order(&self) -> &[usize] {
        match &self.response {
            Response::Order(v) => v,
            r => unreachable!("expected Order, got {r:?}"),
        }
    }
}

pub(crate) enum Flow {
    /// Frame finished; push these (last = new top). Empty = just pop.
    Continue(Vec<Frame>),
    /// Re-push `Frame` and stop with this decision.
    Suspend(Box<Frame>, PendingDecision),
}

impl Flow {
    pub fn done() -> Flow {
        Flow::Continue(Vec::new())
    }

    pub fn then(f: Frame) -> Flow {
        Flow::Continue(vec![f])
    }

    pub fn ask(
        frame: Frame,
        player: PlayerId,
        kind: ChoiceKind,
        prim: Primitive,
        options: Vec<OptionDesc>,
    ) -> Flow {
        Flow::Suspend(
            Box::new(frame),
            PendingDecision {
                player,
                kind,
                prim,
                options,
            },
        )
    }

    /// Single-choice `PickN { min: 1, max: 1 }`.
    pub fn ask_one(
        frame: Frame,
        player: PlayerId,
        kind: ChoiceKind,
        options: Vec<OptionDesc>,
    ) -> Flow {
        Flow::ask(
            frame,
            player,
            kind,
            Primitive::PickN { min: 1, max: 1 },
            options,
        )
    }
}

impl Frame {
    pub(crate) fn run(self, st: &mut GameState, input: Option<Input>) -> Flow {
        match self {
            Frame::Setup(f) => f.run(st, input),
            Frame::Mulligan(f) => f.run(st, input),
            Frame::Step(s) => s.run(st, input),
            Frame::Priority(f) => f.run(st, input),
            Frame::Cast(f) => f.run(st, input),
            Frame::Resolve(f) => (*f).run(st, input),
            Frame::DeclareAttackers(f) => f.run(st, input),
            Frame::DeclareBlockers(f) => f.run(st, input),
            Frame::CombatDamage(f) => f.run(st, input),
            Frame::CleanupDiscard { player } => turn::cleanup_discard(st, player, input),
            Frame::Apply(f) => f.run(st, input),
            Frame::StateCheck(f) => f.run(st, input),
            Frame::PutTriggers(f) => f.run(st, input),
        }
    }
}
