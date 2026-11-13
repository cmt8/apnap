//! Events: what DID happen (post-replacement, once M2 lands). Every event is folded into
//! `GameState::log_hash` as it is emitted, so the hash is a fingerprint of the whole game.
//!
//! Events are engine-level truth and include hidden information (e.g. which card was
//! drawn). A client sees them only through `client::EventView::redact` for its seat (D-090).

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

/// Why a permanent became tapped or untapped (D-090 part 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TapCause {
    /// Paying a cost ({T}, "tap an untapped creature you control", crew).
    Cost,
    /// Tapped for mana: a mana ability, or as part of the payment planner's choices.
    Mana,
    /// Declared as an attacker (CR 508.1f).
    Attack,
    /// A resolving spell or ability ("tap target creature", "untap all lands").
    Effect,
    /// The untap step (CR 502.3).
    UntapStep,
}

/// Why an ability left the stack (D-090 part 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StackExit {
    /// It resolved.
    Resolved,
    /// It didn't: all its targets were illegal (CR 608.2b, `Fizzled`).
    NoLegalTargets,
    /// A spell or ability countered it (CR 701.6).
    Countered,
    /// Its controller left the game (CR 800.4a).
    ControllerLeft,
    /// Putting it on the stack was abandoned: a triggered ability with no legal mode or
    /// target (CR 603.3c–d).
    Abandoned,
    /// Its intervening "if" was false as it would resolve (CR 603.4): it did nothing.
    ConditionFalse,
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
        /// As it last existed where it was (CR 608.2h): a creature (it "dies" if it went
        /// from the battlefield to a graveyard, CR 700.4), and the face it had up there
        /// (CR 712.6). D-090 part 2.
        creature: bool,
        face: u8,
        /// Where in the library it went (top or bottom), if it went to one (CR 401.4).
        position: Option<crate::zones::LibraryPosition>,
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
    /// An activated or (`triggered`) triggered ability was put on the stack as the object
    /// `ability`; its text is `text` (CR 602.2a, 603.3). A triggered one is the one a
    /// `Triggered` event of the same `source` and `text` announced.
    AbilityActivated {
        player: PlayerId,
        ability: ObjectId,
        source: ObjectId,
        text: card_model::AbilityId,
        triggered: bool,
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
    /// The ability `ability` left the stack without resolving, or after resolving: why.
    AbilityRemoved {
        ability: ObjectId,
        reason: StackExit,
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
    /// CR 611.2: a resolving spell or ability made a continuous effect: `effects` on the
    /// objects `affected` (locked in, CR 611.2c), or rules that hold on no object
    /// (`abilities` of `card`, about `about`: "can't be blocked this turn"), until end of
    /// turn or for good. What it does is written in `effects` / `abilities`.
    ContinuousEffectBegan {
        id: u32,
        source: ObjectId,
        card: card_model::CardDefId,
        effects: Vec<card_model::ContinuousEffect>,
        affected: Vec<ObjectId>,
        abilities: Vec<card_model::AbilityId>,
        about: Option<ObjectId>,
        until_end_of_turn: bool,
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
    /// CR 601.2i: `spell` became cast, with face `face` up (CR 712.8), for its alternative
    /// cost `alt` if one was chosen (CR 118.9: flashback, evoke, overload, "without paying
    /// its mana cost"), and `tax` generic mana of commander tax in its total cost (CR 903.8).
    SpellCast {
        player: PlayerId,
        spell: ObjectId,
        face: u8,
        alt: Option<card_model::AbilityId>,
        tax: u32,
    },
    Resolved {
        spell: ObjectId,
    },
    Tapped {
        object: ObjectId,
        cause: TapCause,
    },
    Untapped {
        object: ObjectId,
        cause: TapCause,
    },
    /// One mana added to `player`'s pool, by `source` (a mana ability's permanent, or the
    /// source of the resolving spell or ability); `restricted`: it can be spent only as its
    /// ability says (CR 106.6).
    ManaAdded {
        player: PlayerId,
        mana: ManaType,
        source: Option<ObjectId>,
        restricted: bool,
    },
    /// `player` paid `amount` mana from their pool: these (one entry per mana).
    ManaPaid {
        player: PlayerId,
        amount: u32,
        mana: Vec<ManaType>,
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
    /// CR 701.19a: the permanent regenerated instead of being destroyed (damage removed,
    /// tapped, removed from combat: those have their own events before this one).
    Regenerated {
        object: ObjectId,
    },
    /// CR 707.4 / 707.5: `object` is now a copy of `card` (it became one, or entered or was
    /// created as one).
    BecameCopy {
        object: ObjectId,
        card: card_model::CardDefId,
    },
    /// CR 115.7: the spell or ability `object` has new targets (see the stack).
    TargetsChanged {
        object: ObjectId,
    },
    /// CR 707.10: `controller` put `copy`, a copy of the spell or ability `original`, on
    /// the stack (not cast, not activated).
    CopiedOnStack {
        copy: ObjectId,
        original: ObjectId,
        controller: PlayerId,
    },
    /// CR 508.1b: the attacker just declared (`AttackerDeclared`, with the planeswalker's
    /// controller as the defending player) attacks this planeswalker.
    AttacksPlaneswalker {
        attacker: ObjectId,
        planeswalker: ObjectId,
    },
    /// CR 114.2: `player` got the emblem `object` (in the command zone), whose abilities
    /// are these of `card`'s granted list. Public.
    EmblemCreated {
        object: ObjectId,
        player: PlayerId,
        card: card_model::CardDefId,
        abilities: alloc::vec::Vec<u16>,
    },
    /// Never emitted by the engine: added by [`crate::client::EventView::redact`] when the
    /// seat gets to see `object` (it entered a public zone, was drawn into the seat's hand,
    /// was revealed or looked at): it is `card`, with face `face` up.
    Identified {
        object: ObjectId,
        card: card_model::CardDefId,
        face: u8,
        owner: PlayerId,
    },
    /// Never emitted by the engine: what [`crate::client::EventView::redact`] says instead
    /// of a `ZoneChange` the seat can't follow (one side is in a hidden zone it may not see):
    /// a card of `owner`'s moved from `from` to `to`. The unseen side's id is
    /// [`crate::client::HIDDEN`] (CR 400.2, 400.7).
    MovedUnseen {
        owner: PlayerId,
        old: ObjectId,
        new: ObjectId,
        from: crate::zones::Zone,
        to: crate::zones::Zone,
        /// Where in the library it went, if it went to one: public (CR 401.4).
        position: Option<crate::zones::LibraryPosition>,
    },
    /// CR 702.21a: `player` chose to pay (`paid`) or not to pay the cost that keeps the
    /// spell or ability `at_stake` from being countered.
    PaidToSave {
        player: PlayerId,
        at_stake: ObjectId,
        paid: bool,
    },
    /// CR 608.2: the spell or ability `object` (on top of the stack) begins to resolve.
    ResolutionBegan {
        object: ObjectId,
    },
    /// "You may …" (CR 608.2d): `player` chose, as `object` resolved, whether to do it.
    MayChosen {
        player: PlayerId,
        object: ObjectId,
        yes: bool,
    },
    /// "… unless [player] pays …" (CR 118.12): whether they paid, as `object` resolved.
    UnlessPaid {
        player: PlayerId,
        object: ObjectId,
        paid: bool,
    },
    /// What `player`'s search of a library found (CR 701.23a), possibly nothing: those cards,
    /// still in the library. Another seat may learn only how many, unless they are revealed.
    SearchFound {
        player: PlayerId,
        cards: Vec<ObjectId>,
    },
    /// An effect would have created `wanted` tokens of one kind at once; at most `made` were
    /// (`effects::MAX_TOKENS_AT_ONCE`, a limit of this engine the user approved, not a rule).
    TokensCapped {
        controller: PlayerId,
        token: card_model::CardDefId,
        wanted: u64,
        made: u32,
    },
}
