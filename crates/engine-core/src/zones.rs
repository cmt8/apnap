//! Zones (CR 400) and the ordered per-zone object index.

use crate::ids::{ObjectId, PlayerId};
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Zone {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Stack,
    Exile,
    Command,
}

impl Zone {
    /// CR 400.1 (2026-11-06): library, hand and graveyard are per-player zones.
    pub fn is_per_player(self) -> bool {
        matches!(self, Zone::Library | Zone::Hand | Zone::Graveyard)
    }
}

/// Where in a library an object goes (CR 401).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryPosition {
    Top,
    Bottom,
}

/// Ordered contents of every zone.
///
/// Ordering conventions (documented in CLAUDE.md):
/// - library: index 0 = **bottom**, last = **top** (draw = pop).
/// - graveyard: last = most recently put there (top).
/// - stack: last = top.
/// - battlefield/hand/exile/command: insertion order (= timestamp order for the battlefield).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneIndex {
    pub libraries: Vec<Vec<ObjectId>>,
    pub hands: Vec<Vec<ObjectId>>,
    pub graveyards: Vec<Vec<ObjectId>>,
    pub battlefield: Vec<ObjectId>,
    pub stack: Vec<ObjectId>,
    pub exile: Vec<ObjectId>,
    pub command: Vec<ObjectId>,
}

impl ZoneIndex {
    pub fn new(num_players: usize) -> Self {
        ZoneIndex {
            libraries: (0..num_players).map(|_| Vec::new()).collect(),
            hands: (0..num_players).map(|_| Vec::new()).collect(),
            graveyards: (0..num_players).map(|_| Vec::new()).collect(),
            ..Default::default()
        }
    }

    /// The list for `zone`; `owner` selects the per-player list where relevant.
    pub fn list(&self, zone: Zone, owner: PlayerId) -> &Vec<ObjectId> {
        match zone {
            Zone::Library => &self.libraries[owner.index()],
            Zone::Hand => &self.hands[owner.index()],
            Zone::Graveyard => &self.graveyards[owner.index()],
            Zone::Battlefield => &self.battlefield,
            Zone::Stack => &self.stack,
            Zone::Exile => &self.exile,
            Zone::Command => &self.command,
        }
    }

    pub(crate) fn list_mut(&mut self, zone: Zone, owner: PlayerId) -> &mut Vec<ObjectId> {
        match zone {
            Zone::Library => &mut self.libraries[owner.index()],
            Zone::Hand => &mut self.hands[owner.index()],
            Zone::Graveyard => &mut self.graveyards[owner.index()],
            Zone::Battlefield => &mut self.battlefield,
            Zone::Stack => &mut self.stack,
            Zone::Exile => &mut self.exile,
            Zone::Command => &mut self.command,
        }
    }

    /// Every (zone, owner-or-None, list) triple, in a fixed order. Used by invariant checks.
    pub fn all_lists(&self) -> Vec<(Zone, Option<PlayerId>, &Vec<ObjectId>)> {
        let mut out = Vec::new();
        for (zone, lists) in [
            (Zone::Library, &self.libraries),
            (Zone::Hand, &self.hands),
            (Zone::Graveyard, &self.graveyards),
        ] {
            for (i, l) in lists.iter().enumerate() {
                out.push((zone, Some(PlayerId(i as u8)), l));
            }
        }
        out.push((Zone::Battlefield, None, &self.battlefield));
        out.push((Zone::Stack, None, &self.stack));
        out.push((Zone::Exile, None, &self.exile));
        out.push((Zone::Command, None, &self.command));
        out
    }
}
