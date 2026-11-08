//! Events: what DID happen (post-replacement, once M2 lands). Every event is folded into
//! `GameState::log_hash` as it is emitted, so the hash is a fingerprint of the whole game.
//!
//! Events are engine-level truth and include hidden information (e.g. which card was
//! drawn). UIs must render a per-player redacted log (TODO M4).

use crate::ids::{CardInstanceId, ObjectId, PlayerId};
use crate::mana::ManaType;
use crate::turn::Step;
use crate::zones::Zone;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageTarget {
    Player(PlayerId),
    Object(ObjectId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LossReason {
    /// CR 704.5a
    ZeroLife,
    /// CR 704.5b
    DrewFromEmptyLibrary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    GameStarted {
        seed: u64,
        num_players: u8,
    },
    StartingPlayerChosen {
        chooser: PlayerId,
        starting: PlayerId,
    },
    Shuffled {
        player: PlayerId,
    },
    MulliganDeclared {
        player: PlayerId,
        mulligan: bool,
    },
    TurnBegan {
        turn: u32,
        active: PlayerId,
    },
    StepBegan {
        step: Step,
    },
    ZoneChange {
        old: ObjectId,
        new: ObjectId,
        card_instance: CardInstanceId,
        from: Zone,
        to: Zone,
    },
    Drew {
        player: PlayerId,
        card: ObjectId,
    },
    DrewFromEmptyLibrary {
        player: PlayerId,
    },
    PriorityPassed {
        player: PlayerId,
    },
    LandPlayed {
        player: PlayerId,
        land: ObjectId,
    },
    SpellCast {
        player: PlayerId,
        spell: ObjectId,
    },
    Resolved {
        spell: ObjectId,
    },
    Tapped {
        object: ObjectId,
    },
    Untapped {
        object: ObjectId,
    },
    ManaAdded {
        player: PlayerId,
        mana: ManaType,
    },
    ManaPaid {
        player: PlayerId,
        amount: u32,
    },
    ManaEmptied {
        player: PlayerId,
        amount: u32,
    },
    AttackerDeclared {
        attacker: ObjectId,
        defender: PlayerId,
    },
    BlockerDeclared {
        blocker: ObjectId,
        attacker: ObjectId,
    },
    DamageDealt {
        source: ObjectId,
        target: DamageTarget,
        amount: u32,
        combat: bool,
    },
    LifeChanged {
        player: PlayerId,
        delta: i32,
        life: i32,
    },
    DamageRemoved {
        object: ObjectId,
    },
    Discarded {
        player: PlayerId,
        card: ObjectId,
    },
    PlayerLost {
        player: PlayerId,
        reason: LossReason,
    },
    /// `winner: None` = draw (CR 104.4a).
    GameOver {
        winner: Option<PlayerId>,
    },
}
