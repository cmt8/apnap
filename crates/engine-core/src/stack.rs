//! The stack (CR 405). Spells are card objects in `GameState::objects` with `zone == Stack`;
//! activated abilities on the stack are objects that are **not cards** (CR 602.2a) and live
//! only here. `GameState::stack` is the single source of truth for stack order (last = top).

use crate::ids::{ObjectId, PlayerId};
use crate::layers::Characteristics;
use alloc::vec::Vec;
use card_model::{AbilityId, CardDefId};
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
    /// An activated or triggered ability of `source`, whose card is `source_card`.
    /// `ability` is its text: of that card, or of the card that gave `source` the ability.
    Ability {
        source: ObjectId,
        source_card: CardDefId,
        ability: AbilityId,
        /// A triggered ability (CR 603) rather than an activated one (CR 602).
        triggered: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackObject {
    pub id: ObjectId,
    pub controller: PlayerId,
    pub kind: StackKind,
    /// For abilities: the source as it last existed on the battlefield, once it has left
    /// (CR 113.7a, 608.2h). `None` while the source is still there.
    pub source_lki: Option<Characteristics>,
    /// For triggered abilities: the object whose event triggered it ("it", "that
    /// creature"), and that object as it last existed once it has left the battlefield.
    #[serde(default)]
    pub subject: Option<ObjectId>,
    #[serde(default)]
    pub subject_lki: Option<Characteristics>,
    /// Chosen mode indices, ascending (non-modal bodies: `[0]`). Empty for permanent spells.
    pub modes: Vec<u8>,
    /// Announced X (CR 107.3a); 0 if none.
    pub x: u32,
    /// `targets[k][slot]` for the k-th chosen mode.
    pub targets: Vec<Vec<Target>>,
    /// The total power, as they last existed, of the permanents sacrificed to pay for this
    /// ("the sacrificed creature's power", CR 608.2h).
    #[serde(default)]
    pub sacrificed_power: u32,
    /// For triggered abilities: what else the triggering event carries.
    #[serde(default)]
    pub facts: crate::triggers::EventFacts,
    /// For an ability of a permanent: how many times the permanent had transformed when
    /// the ability was put on the stack (CR 701.27g).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_transforms: Option<u32>,
    /// For spells: the alternative cost paid to cast it (CR 118.9, 601.2b), an ability of
    /// its card. It decides where the card goes as it leaves the stack (flashback, CR
    /// 702.34a) and what its text is (overload, CR 702.96a).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<AbilityId>,
    /// For abilities: what was chosen for the source as it entered, as it was when the
    /// ability was put on the stack — "the chosen type" in the ability's text (CR 607.2d),
    /// also once the source has left (CR 608.2h: a permanent's choice never changes while
    /// it is the same object).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chosen: Vec<card_model::Chosen>,
}

impl StackObject {
    pub fn is_ability(&self) -> bool {
        matches!(self.kind, StackKind::Ability { .. })
    }

    pub fn all_targets(&self) -> impl Iterator<Item = &Target> {
        self.targets.iter().flatten()
    }
}
