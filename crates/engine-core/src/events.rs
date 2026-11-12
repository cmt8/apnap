//! Events: what DID happen (post-replacement, once M2 lands). Every event is folded into
//! `GameState::log_hash` as it is emitted, so the hash is a fingerprint of the whole game.
//!
//! Events are engine-level truth and include hidden information (e.g. which card was
//! drawn). UIs must render a per-player redacted log (TODO M4).

use crate::ids::{CardInstanceId, ObjectId, PlayerId};
use crate::mana::ManaType;
use crate::stack::Target;
use crate::turn::Step;
use crate::zones::Zone;
use alloc::vec::Vec;
use card_model::CounterKind;
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
    /// CR 704.6c / 903.10a: 21 or more combat damage from one commander.
    CommanderDamage,
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
    /// CR 602.2a: an ability object was created on the stack.
    AbilityActivated {
        player: PlayerId,
        ability: ObjectId,
        source: ObjectId,
    },
    /// CR 601.2b / 700.2a
    ModesChosen {
        object: ObjectId,
        modes: Vec<u8>,
    },
    /// CR 107.3a
    XChosen {
        object: ObjectId,
        x: u32,
    },
    /// CR 601.2c
    TargetsChosen {
        object: ObjectId,
        targets: Vec<Target>,
    },
    /// CR 608.2b: every target illegal; the spell or ability doesn't resolve.
    Fizzled {
        object: ObjectId,
    },
    /// CR 701.6a
    Countered {
        object: ObjectId,
    },
    /// An ability left the stack (CR 608.2n) or ceased to exist (CR 800.4a).
    AbilityRemoved {
        ability: ObjectId,
    },
    /// CR 701.21a
    Sacrificed {
        player: PlayerId,
        object: ObjectId,
    },
    /// CR 800.4a: an object left the game with its owner.
    ObjectLeftGame {
        object: ObjectId,
    },
    /// CR 800.4a
    PlayerLeftGame {
        player: PlayerId,
    },
    /// CR 111.1
    TokenCreated {
        object: ObjectId,
        card: card_model::CardDefId,
        controller: PlayerId,
    },
    /// CR 122
    CountersPut {
        object: ObjectId,
        kind: CounterKind,
        n: u32,
    },
    CountersRemoved {
        object: ObjectId,
        kind: CounterKind,
        n: u32,
    },
    /// CR 704.5d
    CeasedToExist {
        object: ObjectId,
    },
    /// CR 611.2
    ContinuousEffectBegan {
        id: u32,
        source: ObjectId,
    },
    /// CR 615: `amount` damage that would have been dealt to `target` was prevented.
    DamagePrevented {
        target: DamageTarget,
        amount: u32,
    },
    /// CR 614: a replacement effect from `source` modified an event.
    Replaced {
        source: ObjectId,
    },
    /// CR 303.4
    Attached {
        aura: ObjectId,
        to: ObjectId,
    },
    /// CR 613.1b: the permanent's controller is now `controller`.
    ControlChanged {
        object: ObjectId,
        controller: PlayerId,
    },
    /// CR 509.1 / 734: a block declaration broke a restriction (menace) and was undone.
    BlockDeclarationIllegal {
        player: PlayerId,
    },
    /// CR 603.2: a triggered ability of `source` triggered.
    Triggered {
        source: ObjectId,
        controller: PlayerId,
        ability: card_model::AbilityId,
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
    /// CR 704.5n: an Equipment became unattached.
    Unattached {
        object: ObjectId,
    },
    /// CR 701.23a: `player` searched their library (looked at every card in it).
    SearchedLibrary {
        player: PlayerId,
    },
    /// CR 701.20a: these cards were shown to all players.
    Revealed {
        player: PlayerId,
        cards: Vec<ObjectId>,
    },
    /// CR 701.20e: `player` looked at the top `count` cards of their library.
    LookedAtLibrary {
        player: PlayerId,
        count: u32,
    },
    /// CR 701.22: `player` scried `count`, putting `bottom` of the cards on the bottom.
    Scried {
        player: PlayerId,
        count: u32,
        bottom: u32,
    },
    /// CR 701.25: `player` surveilled `count`, putting `graveyard` of the cards into their
    /// graveyard.
    Surveilled {
        player: PlayerId,
        count: u32,
        graveyard: u32,
    },
    /// Cards of `player`'s library were put on its top and/or bottom without leaving it.
    LibraryArranged {
        player: PlayerId,
        top: u32,
        bottom: u32,
    },
    /// CR 701.20e: `player` looked at these cards (hidden from everyone else).
    LookedAt {
        player: PlayerId,
        cards: Vec<ObjectId>,
    },
    /// CR 702.29c: `player` cycled `card` — discarded it to pay for its cycling ability.
    /// Follows the `Discarded` event for the same card.
    Cycled {
        player: PlayerId,
        card: ObjectId,
    },
    /// CR 506.4: a permanent stopped being a creature and was removed from combat.
    RemovedFromCombat {
        object: ObjectId,
    },
    /// CR 701.27a: the permanent turned over; `face` is the face now up. It is the same
    /// object (CR 701.27c).
    Transformed {
        object: ObjectId,
        face: u8,
    },
    /// `player` chose `chosen` for `object`: as the permanent entered (CR 614.12a), or for
    /// the spell or ability `object` while it resolved (CR 608.2c). Public.
    Chose {
        player: PlayerId,
        object: ObjectId,
        chosen: card_model::Chosen,
    },
}
