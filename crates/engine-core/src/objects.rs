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
    /// Which face of a card with several is up (CR 712.6): 0 is the front face, `n` the
    /// card's face `n` (`CardDef::face`). Only ever non-zero on the stack or the
    /// battlefield: anywhere else a double-faced card has only its front face (CR 712.6a).
    #[serde(default, skip_serializing_if = "is_front")]
    pub face: u8,
    /// How many times this permanent has transformed (CR 701.27g: an ability of it that
    /// transforms it does so only if it hasn't transformed since the ability was put on
    /// the stack).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transforms: u32,
    /// In exile after resolving as an Adventure: the player who may cast it from there
    /// (CR 716.3d). Ends when it leaves exile (a new object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adventure: Option<PlayerId>,
}

fn is_front(face: &u8) -> bool {
    *face == 0
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl GameObject {
    pub fn counters_of(&self, kind: CounterKind) -> u32 {
        self.counters.get(&kind).copied().unwrap_or(0)
    }
}
