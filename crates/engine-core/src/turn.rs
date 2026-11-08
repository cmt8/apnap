//! Turn structure (CR 500–514).

use crate::ids::PlayerId;
use serde::{Deserialize, Serialize};

/// Steps, and main phases (which have no steps), in turn order (CR 500.1 (2026-11-06)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Step {
    Untap,
    Upkeep,
    Draw,
    PrecombatMain,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndOfCombat,
    PostcombatMain,
    End,
    Cleanup,
}

impl Step {
    /// The following step in the same turn, or `None` after cleanup.
    pub fn next(self) -> Option<Step> {
        use Step::*;
        Some(match self {
            Untap => Upkeep,
            Upkeep => Draw,
            Draw => PrecombatMain,
            PrecombatMain => BeginningOfCombat,
            BeginningOfCombat => DeclareAttackers,
            DeclareAttackers => DeclareBlockers,
            DeclareBlockers => CombatDamage,
            CombatDamage => EndOfCombat,
            EndOfCombat => PostcombatMain,
            PostcombatMain => End,
            End => Cleanup,
            Cleanup => return None,
        })
    }

    pub fn is_main_phase(self) -> bool {
        matches!(self, Step::PrecombatMain | Step::PostcombatMain)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnState {
    /// 0 before the first turn begins.
    pub turn_number: u32,
    pub active: PlayerId,
    pub step: Step,
    /// CR 103.1 (2026-11-06). `None` until chosen.
    pub starting_player: Option<PlayerId>,
}
