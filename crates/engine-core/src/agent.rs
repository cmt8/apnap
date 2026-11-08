//! The interface shared by the UI and every AI (spec §5).

use crate::decision::{PendingDecision, Response};
use crate::view::PlayerView;

/// Anything that can answer decisions. Agents see only a [`PlayerView`] (never
/// `GameState`), which structurally prevents omniscient AI (CLAUDE.md invariant 6).
pub trait Agent: Send {
    fn decide(&mut self, view: &PlayerView, decision: &PendingDecision) -> Response;
}
