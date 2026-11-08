//! The complete game state. `Clone + Serialize`: cloning or round-tripping a state at any
//! suspension point and feeding it the same responses reproduces the same events.

use crate::config::GameConfig;
use crate::decision::PendingDecision;
use crate::events::Event;
use crate::frames::Frame;
use crate::hash::{StableHasher, stable_hash};
use crate::ids::{ObjectId, PlayerId};
use crate::mana::ManaPool;
use crate::objects::GameObject;
use crate::rng::GameRng;
use crate::turn::TurnState;
use crate::zones::{Zone, ZoneIndex};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use card_model::{CardCatalog, CardDef};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerState {
    pub id: PlayerId,
    pub life: i32,
    pub has_lost: bool,
    pub mana_pool: ManaPool,
    pub lands_played_this_turn: u32,
    /// CR 704.5b: attempted to draw from an empty library since SBAs were last checked.
    pub drew_from_empty_library: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackInfo {
    pub attacker: ObjectId,
    pub defender: PlayerId,
    /// CR 509.1h: stays true even if every blocker is removed from combat.
    pub blocked: bool,
    /// Blockers still in combat, in declaration order.
    pub blockers: Vec<ObjectId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatState {
    pub attackers: Vec<AttackInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameResult {
    /// `None` = draw (CR 104.4a).
    pub winner: Option<PlayerId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameState {
    pub config: GameConfig,
    pub seed: u64,
    /// Card definitions for this game, serialized once (D-005).
    pub catalog: Arc<CardCatalog>,
    /// The ONLY randomness source (CLAUDE.md invariant 1).
    pub rng: GameRng,
    pub next_object_id: u32,
    pub next_card_instance: u32,
    pub next_timestamp: u64,
    pub objects: BTreeMap<ObjectId, GameObject>,
    pub players: Vec<PlayerState>,
    pub zones: ZoneIndex,
    pub turn: TurnState,
    pub combat: Option<CombatState>,
    /// Explicit continuation stack (spec's `ResolutionFrame`s); last = top. Everything that
    /// can suspend for a decision lives here as plain data (D-002).
    pub frames: Vec<Frame>,
    /// The decision the engine is suspended on. `None` only before `start` or after game over.
    pub pending: Option<PendingDecision>,
    pub result: Option<GameResult>,
    /// Bumped by every performed action; future layer-cache key (spec §8.3).
    pub generation: u64,
    /// Rolling FNV-1a hash of every event emitted so far (D-007).
    pub log_hash: u64,
    pub event_count: u64,
    /// Number of responses accepted so far.
    pub decision_count: u64,
    /// Events emitted during the current `step` call. Always empty at a suspension point.
    #[serde(skip)]
    pub(crate) out: Vec<Event>,
}

impl GameState {
    pub fn object(&self, id: ObjectId) -> &GameObject {
        self.objects
            .get(&id)
            .unwrap_or_else(|| panic!("no object {id:?}"))
    }

    pub(crate) fn object_mut(&mut self, id: ObjectId) -> &mut GameObject {
        self.objects
            .get_mut(&id)
            .unwrap_or_else(|| panic!("no object {id:?}"))
    }

    /// Printed card data. Engine code must not use this for objects on the battlefield or
    /// stack; use `layers::chars` (CLAUDE.md invariant 3).
    pub fn card_def(&self, id: ObjectId) -> &CardDef {
        self.catalog.get(self.object(id).card)
    }

    pub fn player(&self, p: PlayerId) -> &PlayerState {
        &self.players[p.index()]
    }

    pub(crate) fn player_mut(&mut self, p: PlayerId) -> &mut PlayerState {
        &mut self.players[p.index()]
    }

    pub fn num_players(&self) -> usize {
        self.players.len()
    }

    pub fn in_game(&self, p: PlayerId) -> bool {
        !self.players[p.index()].has_lost
    }

    pub fn players_in_game(&self) -> usize {
        self.players.iter().filter(|p| !p.has_lost).count()
    }

    /// Next player in turn order (clockwise = ascending seat) who is still in the game.
    pub fn next_in_turn_order(&self, p: PlayerId) -> PlayerId {
        let n = self.num_players();
        for k in 1..=n {
            let q = PlayerId(((p.index() + k) % n) as u8);
            if self.in_game(q) {
                return q;
            }
        }
        p
    }

    /// Players in turn order starting from `first`, skipping players who left.
    pub fn turn_order_from(&self, first: PlayerId) -> Vec<PlayerId> {
        let n = self.num_players();
        (0..n)
            .map(|k| PlayerId(((first.index() + k) % n) as u8))
            .filter(|q| self.in_game(*q))
            .collect()
    }

    /// CR 101.4 (2026-11-06): active player, then nonactive players in turn order.
    pub fn apnap_order(&self) -> Vec<PlayerId> {
        self.turn_order_from(self.turn.active)
    }

    pub fn opponents(&self, p: PlayerId) -> Vec<PlayerId> {
        self.turn_order_from(p)
            .into_iter()
            .filter(|q| *q != p)
            .collect()
    }

    pub fn is_over(&self) -> bool {
        self.result.is_some()
    }

    /// Stable hash of the entire serialized state (D-007).
    pub fn state_hash(&self) -> u64 {
        stable_hash(self)
    }

    pub(crate) fn emit(&mut self, e: Event) {
        let mut h = StableHasher::resume(self.log_hash);
        h.update(&e);
        self.log_hash = h.finish();
        self.event_count += 1;
        self.out.push(e);
    }

    pub(crate) fn alloc_object_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_object_id);
        self.next_object_id += 1;
        id
    }

    pub(crate) fn alloc_timestamp(&mut self) -> crate::ids::Timestamp {
        let t = crate::ids::Timestamp(self.next_timestamp);
        self.next_timestamp += 1;
        t
    }

    /// Structural invariants (spec §10.4): every object is in exactly one zone list, that
    /// list matches its `zone`/`owner`, and no zone lists a missing object.
    pub fn check_invariants(&self) -> Result<(), String> {
        let mut seen: BTreeMap<ObjectId, Zone> = BTreeMap::new();
        for (zone, owner, list) in self.zones.all_lists() {
            for id in list {
                if let Some(prev) = seen.insert(*id, zone) {
                    return Err(format!("{id:?} listed in {prev:?} and {zone:?}"));
                }
                let Some(obj) = self.objects.get(id) else {
                    return Err(format!("{id:?} in {zone:?} has no object"));
                };
                if obj.zone != zone {
                    return Err(format!(
                        "{id:?} listed in {zone:?} but obj.zone={:?}",
                        obj.zone
                    ));
                }
                if let Some(o) = owner
                    && obj.owner != o
                {
                    return Err(format!(
                        "{id:?} in {o:?}'s {zone:?} but owned by {:?}",
                        obj.owner
                    ));
                }
            }
        }
        if seen.len() != self.objects.len() {
            return Err(format!(
                "{} objects but {} listed in zones",
                self.objects.len(),
                seen.len()
            ));
        }
        if !self.out.is_empty() && self.pending.is_some() {
            return Err("undrained events at a suspension point".into());
        }
        Ok(())
    }
}
