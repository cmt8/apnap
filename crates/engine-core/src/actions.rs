//! The single mutation chokepoint (spec §8.1, CLAUDE.md invariant 2).
//!
//! Every observable change to the game is a [`GameAction`] passed to [`execute`]. Until M2
//! the action is performed directly; M2 inserts the CR 614–616 replacement/prevention
//! pipeline here (which is also where a `ChooseReplacement` suspension will originate; the
//! resolution work list, D-020, is what lets the caller resume around it).

use crate::events::{DamageTarget, Event, LossReason};
use crate::ids::{ObjectId, PlayerId};
use crate::mana::{ManaRequirement, ManaType};
use crate::objects::GameObject;
use crate::stack::{StackKind, StackObject, Target};
use crate::state::GameState;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec::Vec;
use card_model::CardDefId;

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
    Draw { player: PlayerId },
    Shuffle { player: PlayerId },
    Tap { object: ObjectId },
    Untap { object: ObjectId },
    AddMana { player: PlayerId, mana: ManaType },
    /// CR 601.2h: pay specific symbols plus the chosen generic allocation.
    PayMana {
        player: PlayerId,
        requirement: ManaRequirement,
        generic: [u32; 6],
    },
    /// CR 500.5: unspent mana empties as a step ends.
    EmptyManaPool { player: PlayerId },
    /// CR 120.3: damage to a player causes life loss; to a creature, it is marked.
    Damage {
        source: ObjectId,
        target: DamageTarget,
        amount: u32,
        combat: bool,
    },
    GainLife { player: PlayerId, amount: u32 },
    LoseLife { player: PlayerId, amount: u32 },
    /// CR 514.2.
    RemoveDamage { object: ObjectId },
    LoseGame { player: PlayerId, reason: LossReason },
    /// CR 602.2a: create an activated ability object on top of the stack.
    PutAbilityOnStack {
        controller: PlayerId,
        source: ObjectId,
        source_card: CardDefId,
        index: u16,
    },
    /// CR 601.2b–c: record the announced modes, X and targets of a stack object.
    SetStackChoices {
        object: ObjectId,
        modes: Vec<u8>,
        x: u32,
        targets: Vec<Vec<Target>>,
    },
    /// CR 608.2n / 608.2b / 701.6a: an ability leaves the stack and ceases to exist.
    RemoveAbility { ability: ObjectId },
    /// CR 800.4a: a player leaves a multiplayer game.
    LeaveGame { player: PlayerId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionResult {
    Done,
    /// The new object id after a zone change (or the new ability object).
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
            generic,
        } => {
            requirement.pay(&mut st.player_mut(player).mana_pool, &generic);
            st.emit(Event::ManaPaid {
                player,
                amount: requirement.total(),
            });
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
                DamageTarget::Player(p) => change_life(st, p, -(amount as i32)),
                DamageTarget::Object(o) => st.object_mut(o).damage_marked += amount,
            }
            ActionResult::Done
        }
        GameAction::GainLife { player, amount } => {
            if amount > 0 {
                change_life(st, player, amount as i32);
            }
            ActionResult::Done
        }
        GameAction::LoseLife { player, amount } => {
            if amount > 0 {
                change_life(st, player, -(amount as i32));
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
        GameAction::PutAbilityOnStack {
            controller,
            source,
            source_card,
            index,
        } => {
            let id = st.alloc_object_id();
            st.stack.push(StackObject {
                id,
                controller,
                kind: StackKind::Ability {
                    source,
                    source_card,
                    index,
                },
                modes: Vec::new(),
                x: 0,
                targets: Vec::new(),
            });
            st.emit(Event::AbilityActivated {
                player: controller,
                ability: id,
                source,
            });
            ActionResult::Moved(id)
        }
        GameAction::SetStackChoices {
            object,
            modes,
            x,
            targets,
        } => {
            let so = st
                .stack
                .iter_mut()
                .find(|s| s.id == object)
                .expect("stack object");
            so.modes = modes;
            so.x = x;
            so.targets = targets;
            ActionResult::Done
        }
        GameAction::RemoveAbility { ability } => {
            st.stack.retain(|s| s.id != ability);
            st.emit(Event::AbilityRemoved { ability });
            ActionResult::Done
        }
        GameAction::LeaveGame { player } => {
            leave_game(st, player);
            ActionResult::Done
        }
    }
}

fn change_life(st: &mut GameState, p: PlayerId, delta: i32) {
    let ps = st.player_mut(p);
    ps.life += delta;
    let life = ps.life;
    st.emit(Event::LifeChanged {
        player: p,
        delta,
        life,
    });
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

/// Take `id` out of its zone (list, stack, combat). Returns the removed object.
fn detach(st: &mut GameState, id: ObjectId) -> GameObject {
    let old = st
        .objects
        .remove(&id)
        .unwrap_or_else(|| panic!("move of missing object {id:?}"));
    if old.zone == Zone::Stack {
        let before = st.stack.len();
        st.stack.retain(|s| s.id != id);
        debug_assert_eq!(st.stack.len() + 1, before, "{id:?} not on the stack");
    } else {
        let list = st.zones.list_mut(old.zone, old.owner);
        let pos = list
            .iter()
            .position(|x| *x == id)
            .unwrap_or_else(|| panic!("{id:?} not in its zone list"));
        list.remove(pos);
    }
    // CR 506.4: a permanent that leaves the battlefield is removed from combat.
    if old.zone == Zone::Battlefield
        && let Some(c) = st.combat.as_mut()
    {
        c.attackers.retain(|a| a.attacker != id);
        for a in &mut c.attackers {
            a.blockers.retain(|b| *b != id);
        }
    }
    old
}

/// `move_object` (spec §5): the only code that changes an object's zone.
fn perform_move(
    st: &mut GameState,
    id: ObjectId,
    to: Zone,
    position: LibraryPosition,
    controller: Option<PlayerId>,
) -> ObjectId {
    let old = detach(st, id);
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
    if to == Zone::Stack {
        st.stack.push(StackObject {
            id: new_id,
            controller,
            kind: StackKind::Spell,
            modes: Vec::new(),
            x: 0,
            targets: Vec::new(),
        });
    } else {
        let list = st.zones.list_mut(to, old.owner);
        match (to, position) {
            (Zone::Library, LibraryPosition::Bottom) => list.insert(0, new_id),
            _ => list.push(new_id),
        }
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

/// CR 800.4a (2026-11-06), in order. M1 has no control-changing effects, so "effects which
/// give that player control of any objects or players end" has nothing to do yet (M3).
fn leave_game(st: &mut GameState, p: PlayerId) {
    // (1) All objects owned by that player leave the game.
    let owned: Vec<ObjectId> = st
        .objects
        .values()
        .filter(|o| o.owner == p)
        .map(|o| o.id)
        .collect();
    for id in owned {
        detach(st, id);
        st.emit(Event::ObjectLeftGame { object: id });
    }
    // (2) Objects on the stack they control that aren't represented by cards cease to exist.
    let abilities: Vec<ObjectId> = st
        .stack
        .iter()
        .filter(|s| s.controller == p && s.is_ability())
        .map(|s| s.id)
        .collect();
    for ability in abilities {
        st.stack.retain(|s| s.id != ability);
        st.emit(Event::AbilityRemoved { ability });
    }
    // (3) Objects still controlled by that player are exiled.
    let controlled: Vec<ObjectId> = st
        .objects
        .values()
        .filter(|o| o.controller == p && matches!(o.zone, Zone::Battlefield | Zone::Stack))
        .map(|o| o.id)
        .collect();
    for id in controlled {
        perform_move(st, id, Zone::Exile, LibraryPosition::Top, None);
    }
    // Their mana pool and life no longer matter; priority is handled by the priority frame.
    st.player_mut(p).mana_pool = Default::default();
    st.emit(Event::PlayerLeftGame { player: p });
}
