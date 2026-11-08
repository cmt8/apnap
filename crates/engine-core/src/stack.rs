//! The stack (CR 405). Spells are card objects in `GameState::objects` with `zone == Stack`;
//! activated abilities on the stack are objects that are **not cards** (CR 602.2a) and live
//! only here. `GameState::stack` is the single source of truth for stack order (last = top).

use crate::ids::{ObjectId, PlayerId};
use alloc::vec::Vec;
use card_model::CardDefId;
use serde::{Deserialize, Serialize};

/// A chosen target (CR 115).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Target {
    Player(PlayerId),
    /// A permanent, or a spell on the stack.
    Object(ObjectId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StackKind {
    /// The spell is the card object with the same id.
    Spell,
    /// An activated ability of `source` (printed ability `index` of `source_card`).
    Ability {
        source: ObjectId,
        source_card: CardDefId,
        index: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackObject {
    pub id: ObjectId,
    pub controller: PlayerId,
    pub kind: StackKind,
    /// Chosen mode indices, ascending (non-modal bodies: `[0]`). Empty for permanent spells.
    pub modes: Vec<u8>,
    /// Announced X (CR 107.3a); 0 if none.
    pub x: u32,
    /// `targets[k][slot]` for the k-th chosen mode.
    pub targets: Vec<Vec<Target>>,
}

impl StackObject {
    pub fn is_ability(&self) -> bool {
        matches!(self.kind, StackKind::Ability { .. })
    }

    pub fn all_targets(&self) -> impl Iterator<Item = &Target> {
        self.targets.iter().flatten()
    }
}
