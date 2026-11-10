//! Continuous effects from resolved spells/abilities (CR 611.2) and prevention shields
//! (CR 615.7). Static abilities' effects are NOT stored: they are derived from the
//! permanents that currently have those abilities on every layer walk (CR 604.2, 611.3a).

use crate::ids::{ObjectId, PlayerId, Timestamp};
use crate::stack::Target;
use alloc::vec::Vec;
use card_model::ContinuousEffect;
use serde::{Deserialize, Serialize};

/// A continuous effect created by a resolving spell or ability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuousInstance {
    pub id: u32,
    /// CR 613.7b: received when the effect is created.
    pub timestamp: Timestamp,
    pub source: ObjectId,
    pub controller: PlayerId,
    /// CR 611.2c: the affected set is locked in when the effect begins.
    pub affected: Vec<ObjectId>,
    pub effects: Vec<ContinuousEffect>,
    /// Ends at cleanup (CR 514.2); otherwise lasts until the end of the game (611.2a).
    pub until_end_of_turn: bool,
    /// The card the effects are written on (`GrantAbilities` names abilities of it).
    pub card: card_model::CardDefId,
}

/// CR 615.7: "prevent the next N damage that would be dealt to [target] this turn".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shield {
    pub id: u32,
    pub timestamp: Timestamp,
    pub target: Target,
    pub remaining: u32,
}
