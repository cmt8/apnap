//! The single mutation chokepoint (spec §8.1, CLAUDE.md invariant 2).
//!
//! Every observable change to the game is a [`GameAction`] passed to [`execute`]. In M0 the
//! action is performed directly; M2 inserts the CR 614–616 replacement/prevention pipeline
//! here (which is also where a `ChooseReplacement` suspension will originate).

use crate::events::{DamageTarget, Event, LossReason};
use crate::ids::{ObjectId, PlayerId};
use crate::mana::{ManaRequirement, ManaType};
use crate::objects::GameObject;
use crate::state::GameState;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameAction {
    /// CR 400.7: the moved object becomes a new object.
    MoveZone {
        object: ObjectId,
        to: Zone,
        position: LibraryPosition,
        /// Controller on the battlefield/stack; defaults to the owner.
        controller: Option<PlayerId>,
    },
    /// CR 121.1. Drawing from an empty library sets the CR 704.5b flag.
    Draw {
        player: PlayerId,
    },
    Shuffle {
        player: PlayerId,
    },
    Tap {
        object: ObjectId,
    },
    Untap {
        object: ObjectId,
    },
    AddMana {
        player: PlayerId,
        mana: ManaType,
    },
    PayMana {
        player: PlayerId,
        requirement: ManaRequirement,
    },
    /// CR 500.5: unspent mana empties as a step ends.
    EmptyManaPool {
        player: PlayerId,
    },
    /// CR 120.3: damage to a player causes life loss; to a creature, it is marked.
    Damage {
        source: ObjectId,
        target: DamageTarget,
        amount: u32,
        combat: bool,
    },
    /// CR 514.2.
    RemoveDamage {
        object: ObjectId,
    },
    LoseGame {
        player: PlayerId,
        reason: LossReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionResult {
    Done,
    /// The new object id after a zone change.
    Moved(ObjectId),
    /// The drawn card's new id, or `None` if the library was empty.
    Drew(Option<ObjectId>),
}

pub fn execute(st: &mut GameState, action: GameAction) -> ActionResult {
    st.generation += 1;
    match action {
        GameAction::MoveZone {
            object,
            to,
            position,
            controller,
        } => ActionResult::Moved(perform_move(st, object, to, position, controller)),
        GameAction::Draw { player } => {
            let Some(&top) = st.zones.libraries[player.index()].last() else {
                st.player_mut(player).drew_from_empty_library = true;
                st.emit(Event::DrewFromEmptyLibrary { player });
                return ActionResult::Drew(None);
            };
            let new = perform_move(st, top, Zone::Hand, LibraryPosition::Top, None);
            st.emit(Event::Drew { player, card: new });
            ActionResult::Drew(Some(new))
        }
        GameAction::Shuffle { player } => {
            let mut lib = core::mem::take(&mut st.zones.libraries[player.index()]);
            st.rng.shuffle(&mut lib);
            st.zones.libraries[player.index()] = lib;
            st.emit(Event::Shuffled { player });
            ActionResult::Done
        }
        GameAction::Tap { object } => {
            let o = st.object_mut(object);
            debug_assert!(!o.tapped, "tapping a tapped permanent");
            o.tapped = true;
            st.emit(Event::Tapped { object });
            ActionResult::Done
        }
        GameAction::Untap { object } => {
            st.object_mut(object).tapped = false;
            st.emit(Event::Untapped { object });
            ActionResult::Done
        }
        GameAction::AddMana { player, mana } => {
            st.player_mut(player).mana_pool.add(mana, 1);
            st.emit(Event::ManaAdded { player, mana });
            ActionResult::Done
        }
        GameAction::PayMana {
            player,
            requirement,
        } => {
            let pool = &mut st.player_mut(player).mana_pool;
            let before = pool.total();
            requirement.pay_from(pool);
            let amount = before - pool.total();
            st.emit(Event::ManaPaid { player, amount });
            ActionResult::Done
        }
        GameAction::EmptyManaPool { player } => {
            let pool = &mut st.player_mut(player).mana_pool;
            let amount = pool.total();
            *pool = Default::default();
            if amount > 0 {
                st.emit(Event::ManaEmptied { player, amount });
            }
            ActionResult::Done
        }
        GameAction::Damage {
            source,
            target,
            amount,
            combat,
        } => {
            if amount == 0 {
                return ActionResult::Done;
            }
            st.emit(Event::DamageDealt {
                source,
                target,
                amount,
                combat,
            });
            match target {
                DamageTarget::Player(p) => {
                    let delta = -(amount as i32);
                    let ps = st.player_mut(p);
                    ps.life += delta;
                    let life = ps.life;
                    st.emit(Event::LifeChanged {
                        player: p,
                        delta,
                        life,
                    });
                }
                DamageTarget::Object(o) => st.object_mut(o).damage_marked += amount,
            }
            ActionResult::Done
        }
        GameAction::RemoveDamage { object } => {
            st.object_mut(object).damage_marked = 0;
            st.emit(Event::DamageRemoved { object });
            ActionResult::Done
        }
        GameAction::LoseGame { player, reason } => {
            st.player_mut(player).has_lost = true;
            st.emit(Event::PlayerLost { player, reason });
            ActionResult::Done
        }
    }
}

/// Convenience wrapper: move an object and return its new id.
pub fn move_object(
    st: &mut GameState,
    object: ObjectId,
    to: Zone,
    position: LibraryPosition,
    controller: Option<PlayerId>,
) -> ObjectId {
    match execute(
        st,
        GameAction::MoveZone {
            object,
            to,
            position,
            controller,
        },
    ) {
        ActionResult::Moved(id) => id,
        other => unreachable!("move returned {other:?}"),
    }
}

/// `move_object` (spec §5): the only code that changes an object's zone.
fn perform_move(
    st: &mut GameState,
    id: ObjectId,
    to: Zone,
    position: LibraryPosition,
    controller: Option<PlayerId>,
) -> ObjectId {
    let old = st
        .objects
        .remove(&id)
        .unwrap_or_else(|| panic!("move of missing object {id:?}"));
    let list = st.zones.list_mut(old.zone, old.owner);
    let pos = list
        .iter()
        .position(|x| *x == id)
        .unwrap_or_else(|| panic!("{id:?} not in its zone list"));
    list.remove(pos);

    // CR 506.4: a permanent that leaves the battlefield is removed from combat.
    if old.zone == Zone::Battlefield
        && let Some(c) = st.combat.as_mut()
    {
        c.attackers.retain(|a| a.attacker != id);
        for a in &mut c.attackers {
            a.blockers.retain(|b| *b != id);
        }
    }

    // CR 400.7 (2026-11-06): a new object with no memory of its previous existence.
    let new_id = st.alloc_object_id();
    let timestamp = st.alloc_timestamp();
    let controller = match to {
        Zone::Battlefield | Zone::Stack => controller.unwrap_or(old.owner),
        _ => old.owner,
    };
    let obj = GameObject {
        id: new_id,
        card_instance: old.card_instance,
        card: old.card,
        owner: old.owner,
        controller,
        zone: to,
        timestamp,
        tapped: false,
        damage_marked: 0,
        summoning_sick: to == Zone::Battlefield,
    };
    st.objects.insert(new_id, obj);
    let list = st.zones.list_mut(to, old.owner);
    match (to, position) {
        (Zone::Library, LibraryPosition::Bottom) => list.insert(0, new_id),
        _ => list.push(new_id),
    }
    st.emit(Event::ZoneChange {
        old: id,
        new: new_id,
        card_instance: old.card_instance,
        from: old.zone,
        to,
    });
    new_id
}

/// Objects in `zone` (battlefield/stack/exile/command) or a player's zone, in index order.
pub fn zone_objects(st: &GameState, zone: Zone, owner: PlayerId) -> Vec<ObjectId> {
    st.zones.list(zone, owner).clone()
}
