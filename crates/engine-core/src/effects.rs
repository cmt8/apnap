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
    /// CR 701.19a: a regeneration shield on the permanent `target` (used up once), not a
    /// prevention shield. Serialized only when set.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub regenerate: bool,
}

/// What a resolved spell or ability leaves behind that isn't a change of characteristics
/// (CR 611.2): abilities that work for a while without being on any object — "target
/// creature can't be blocked this turn", "prevent all combat damage that would be dealt
/// this turn". In them "this" is `source`, "you" is `controller`, and `Remembered` is
/// `about`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lasting {
    pub id: u32,
    pub timestamp: Timestamp,
    /// The source of the spell or ability that made it.
    pub source: ObjectId,
    pub controller: PlayerId,
    pub abilities: Vec<card_model::AbilityId>,
    /// The object the effect is about ("target creature …"). A new object (CR 400.7)
    /// isn't it: the effect is over when this one is gone.
    pub about: Option<ObjectId>,
    /// Ends at cleanup (CR 514.2).
    pub until_end_of_turn: bool,
    /// Ends when `source` leaves the battlefield.
    pub while_source: bool,
}

impl Lasting {
    /// Is it still in force?
    pub fn live(&self, st: &crate::state::GameState) -> bool {
        self.about.is_none_or(|o| st.objects.contains_key(&o))
            && (!self.while_source
                || st
                    .objects
                    .get(&self.source)
                    .is_some_and(|o| o.zone == crate::zones::Zone::Battlefield))
    }
}

/// CR 603.7: a delayed triggered ability, waiting for its event. It triggers once and is
/// then gone (603.7c); `controller` controls it (603.7d) and `source` is its source (603.7e)
/// — which may have left its zone meanwhile (`source_lki` is it as the ability was made).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delayed {
    pub id: u32,
    pub ability: card_model::AbilityId,
    pub source: ObjectId,
    pub source_card: card_model::CardDefId,
    pub source_lki: Option<crate::layers::Characteristics>,
    pub controller: PlayerId,
    /// "That player", "that much": fixed as it was made.
    pub facts: crate::triggers::EventFacts,
    /// It can trigger from the beginning of this turn on ("at the beginning of the next
    /// turn's upkeep" can't trigger this turn).
    pub from_turn: u32,
    /// It ends at the cleanup step of this turn (CR 514.2), if it hasn't triggered.
    pub until_turn: Option<u32>,
    /// The objects it is about (CR 603.7c): in it they are "those cards", affected only
    /// while each is still the same object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<ObjectId>,
}

/// Who an ability is being read for: an object that has it, or a lasting effect that
/// carries it.
#[derive(Clone, Copy, Debug)]
pub struct Bearer<'a> {
    /// "This": the object with the ability, or the source of the lasting effect.
    pub source: ObjectId,
    /// "You".
    pub controller: PlayerId,
    /// What a lasting effect is about (`Remembered`); nothing for an object's own ability.
    pub about: &'a [ObjectId],
}

/// CR 114: the emblems in the command zone, in the order they were made (CR 114.4: their
/// abilities work from there).
pub fn emblems(st: &crate::state::GameState) -> Vec<ObjectId> {
    st.zones
        .command
        .iter()
        .copied()
        .filter(|id| st.object(*id).emblem.is_some())
        .collect()
}

/// The lasting effects in force, with each ability they carry.
pub fn lasting_abilities(
    st: &crate::state::GameState,
) -> impl Iterator<Item = (&Lasting, usize, &card_model::AbilityDef)> {
    st.lasting.iter().filter(|e| e.live(st)).flat_map(move |e| {
        e.abilities
            .iter()
            .enumerate()
            .map(move |(i, a)| (e, i, st.catalog.ability(*a)))
    })
}
