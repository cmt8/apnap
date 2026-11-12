//! Game objects (CR 109) and their per-object mutable status.

use crate::ids::{CardInstanceId, ObjectId, PlayerId, Timestamp};
use crate::zones::Zone;
use alloc::collections::BTreeMap;
use card_model::{CardDefId, CounterKind};
use serde::{Deserialize, Serialize};

/// A game object. Printed data lives in the catalog (`card`); characteristics are NEVER
/// stored here — always query `layers::chars` (CLAUDE.md invariant 3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameObject {
    pub id: ObjectId,
    pub card_instance: CardInstanceId,
    pub card: CardDefId,
    pub owner: PlayerId,
    /// Meaningful only on the battlefield and stack (CR 108.4); equals owner elsewhere.
    pub controller: PlayerId,
    pub zone: Zone,
    pub timestamp: Timestamp,
    pub tapped: bool,
    pub damage_marked: u32,
    /// CR 302.6 (2026-11-06): true until its controller's next turn begins.
    pub summoning_sick: bool,
    /// CR 122: counters on this object.
    pub counters: BTreeMap<CounterKind, u32>,
    /// CR 111: a token (not a card). Its `card` is a token definition in the catalog.
    pub is_token: bool,
    /// CR 704.5h: dealt damage by a source with deathtouch since SBAs were last checked.
    pub deathtouch_damage: bool,
    /// CR 303.4: the object this Aura is attached to.
    pub attached_to: Option<ObjectId>,
    /// The layered controller last seen by the engine; a change marks the permanent
    /// summoning sick (CR 302.6). Bookkeeping for `layers::sync_control`.
    pub last_controller: PlayerId,
    /// CR 707.2: the copiable values of the copy effect it is under (layer 1a), if it is a
    /// copy — a token copy, a permanent that entered or became a copy, a copy of a spell.
    /// Not carried to a new object (CR 400.7), except that a copy of a permanent spell
    /// stays what it is as it becomes a token (CR 608.3f).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<alloc::boxed::Box<crate::copy::Copiable>>,
    /// CR 707.10: a copy of a spell — not a card; it ceases to exist anywhere but on the
    /// stack (CR 704.5e).
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub spell_copy: bool,
}

impl GameObject {
    pub fn counters_of(&self, kind: CounterKind) -> u32 {
        self.counters.get(&kind).copied().unwrap_or(0)
    }
}
