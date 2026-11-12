//! The single mutation chokepoint (spec §8.1, CLAUDE.md invariant 2).
//!
//! Every observable change to the game is a [`GameAction`] passed to [`execute`]. Until M2
//! the action is performed directly; M2 inserts the CR 614–616 replacement/prevention
//! pipeline here (which is also where a `ChooseReplacement` suspension will originate; the
//! resolution work list, D-020, is what lets the caller resume around it).

use crate::effects::{ContinuousInstance, Shield};
use crate::events::{DamageTarget, Event, LossReason};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::Characteristics;
use crate::mana::{ManaRequirement, ManaType};
use crate::objects::GameObject;
use crate::stack::{StackKind, StackObject, Target};
use crate::state::GameState;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec::Vec;
use card_model::{AbilityId, CardDefId, ContinuousEffect, CounterKind};

// An action is built and executed on the spot, never stored: the size of the largest
// variant (the one that carries last known information) doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameAction {
    /// CR 400.7: the moved object becomes a new object.
    MoveZone {
        object: ObjectId,
        to: Zone,
        position: LibraryPosition,
        /// Controller on the battlefield/stack; defaults to the owner.
        controller: Option<PlayerId>,
        /// Entering the battlefield tapped (CR 614.1c–d).
        tapped: bool,
        /// Counters it enters with (CR 122.6).
        counters: Vec<(CounterKind, u32)>,
        /// The face it is put onto the stack or battlefield with (CR 712.8–712.11); 0 for
        /// the default: the front face, or from the stack the face that was up there.
        face: u8,
    },
    /// CR 111.1: create a token (from a token definition) on the battlefield.
    CreateToken {
        token: CardDefId,
        controller: PlayerId,
        /// How it enters, as replacement effects modified it (CR 614.1c–d, 111.6).
        tapped: bool,
        counters: Vec<(CounterKind, u32)>,
    },
    /// CR 122.1: put counters on a permanent.
    PutCounters {
        object: ObjectId,
        kind: CounterKind,
        n: u32,
    },
    /// CR 704.5q: remove counters.
    RemoveCounters {
        object: ObjectId,
        kind: CounterKind,
        n: u32,
    },
    /// CR 704.5d / 111.7: a token outside the battlefield ceases to exist.
    CeaseToExist {
        object: ObjectId,
    },
    /// CR 611.2: a continuous effect from a resolving spell or ability begins.
    AddContinuous {
        source: ObjectId,
        controller: PlayerId,
        affected: Vec<ObjectId>,
        effects: Vec<ContinuousEffect>,
        until_end_of_turn: bool,
        /// The card the effects are written on.
        card: CardDefId,
    },
    /// CR 615.7: create a prevention shield.
    AddShield {
        target: Target,
        amount: u32,
    },
    /// CR 611.2: abilities begin to work for a while on no object.
    AddLasting {
        source: ObjectId,
        controller: PlayerId,
        abilities: Vec<AbilityId>,
        about: Option<ObjectId>,
        until_end_of_turn: bool,
        while_source: bool,
    },
    /// CR 603.7: a delayed triggered ability is created (its `id` is assigned here).
    AddDelayed {
        delayed: crate::effects::Delayed,
    },
    /// CR 603.7c: it has triggered (or can't any more) and is gone.
    RemoveDelayed {
        id: u32,
    },
    /// Use up `n` of a shield's remaining prevention.
    ConsumeShield {
        id: u32,
        n: u32,
    },
    /// CR 514.2: "until end of turn" and "this turn" effects end.
    ExpireEndOfTurn,
    /// CR 303.4: attach an Aura to an object.
    Attach {
        aura: ObjectId,
        to: ObjectId,
    },
    /// CR 701.3c: a permanent attached to a different object gets a new timestamp.
    NewTimestamp {
        object: ObjectId,
    },
    /// CR 704.5n: an Equipment becomes unattached (it stays on the battlefield).
    Unattach {
        object: ObjectId,
    },
    /// CR 613.1b / 302.6: a permanent's (layered) controller changed; it is summoning sick
    /// for its new controller.
    ControlChanged {
        object: ObjectId,
        controller: PlayerId,
    },
    /// CR 903.8: a commander was cast from the command zone (the tax grows).
    CommanderCast {
        player: PlayerId,
        card: crate::ids::CardInstanceId,
    },
    /// CR 113.7a: an ability on the stack remembers its source as it last existed.
    RecordSourceLki {
        ability: ObjectId,
        chars: Characteristics,
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
    /// CR 701.27: a double-faced permanent turns over.
    Transform {
        object: ObjectId,
    },
    /// CR 716.3d: an Adventure spell is resolving: once exiled, `player` may cast it.
    SendOnAdventure {
        spell: ObjectId,
        player: PlayerId,
    },
    Untap {
        object: ObjectId,
    },
    AddMana {
        player: PlayerId,
        mana: ManaType,
    },
    /// CR 601.2h: pay specific symbols plus the chosen generic allocation.
    PayMana {
        player: PlayerId,
        requirement: ManaRequirement,
        generic: [u32; 6],
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
        /// The source had deathtouch (CR 702.2b, 704.5h).
        deathtouch: bool,
    },
    GainLife {
        player: PlayerId,
        amount: u32,
    },
    LoseLife {
        player: PlayerId,
        amount: u32,
    },
    /// CR 514.2.
    RemoveDamage {
        object: ObjectId,
    },
    LoseGame {
        player: PlayerId,
        reason: LossReason,
    },
    /// CR 602.2a / 603.3: create an activated or triggered ability object on the stack.
    PutAbilityOnStack {
        controller: PlayerId,
        source: ObjectId,
        source_card: CardDefId,
        ability: AbilityId,
        triggered: bool,
        source_lki: Option<Characteristics>,
        /// The object whose event triggered the ability, and its last known information.
        subject: Option<ObjectId>,
        subject_lki: Option<Characteristics>,
        facts: crate::triggers::EventFacts,
    },
    /// CR 608.2h: an ability on the stack remembers the triggering object as it last
    /// existed, once that object leaves the battlefield.
    RecordSubjectLki {
        ability: ObjectId,
        chars: Characteristics,
    },
    /// CR 601.2b–c: record the announced modes, X and targets of a stack object.
    SetStackChoices {
        object: ObjectId,
        modes: Vec<u8>,
        x: u32,
        targets: Vec<Vec<Target>>,
    },
    /// CR 608.2n / 608.2b / 701.6a: an ability leaves the stack and ceases to exist.
    RemoveAbility {
        ability: ObjectId,
    },
    /// CR 800.4a: a player leaves a multiplayer game.
    LeaveGame {
        player: PlayerId,
    },
    /// CR 401.4 / 701.22a: put cards that are in `player`'s library on its top and/or its
    /// bottom. They stay in the library, so they stay the same objects (CR 400.7 is about
    /// changing zones).
    ArrangeLibrary {
        player: PlayerId,
        /// The first listed ends up on top.
        top: Vec<ObjectId>,
        /// The last listed ends up bottom-most.
        bottom: Vec<ObjectId>,
        /// The bottom cards go in a random order instead of the listed one.
        random_bottom: bool,
    },
    /// CR 106.6: add one mana that can only be spent as mana ability `ability` says.
    AddRestrictedMana {
        player: PlayerId,
        mana: ManaType,
        ability: AbilityId,
    },
    /// CR 106.6: `amount` of the restricted mana at `entry` is about to pay for something
    /// it may be spent on: it joins the rest of the pool, to be paid with it.
    ReleaseRestrictedMana {
        player: PlayerId,
        entry: u32,
        amount: u32,
    },
    /// CR 608.2h: record on a stack object the total power of what was sacrificed to pay
    /// for it ("where X is the sacrificed creature's power").
    SetSacrificedPower {
        object: ObjectId,
        power: u32,
    },
    /// CR 506.4: an attacking or blocking creature that stopped being a creature is
    /// removed from combat.
    RemoveFromCombat {
        object: ObjectId,
    },
    /// CR 601.2b: the spell is being cast with this alternative cost (an ability of its
    /// card).
    SetCastAlt {
        object: ObjectId,
        ability: card_model::AbilityId,
    },
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
            tapped,
            counters,
            face,
        } => {
            let new = perform_move(st, object, to, position, controller, face);
            if to == Zone::Battlefield {
                let o = st.object_mut(new);
                o.tapped = tapped;
                for (kind, n) in counters {
                    let have = o.counters.entry(kind).or_default();
                    *have = have.saturating_add(n);
                }
            }
            ActionResult::Moved(new)
        }
        GameAction::CreateToken {
            token,
            controller,
            tapped,
            counters,
        } => {
            let id = st.alloc_object_id();
            let timestamp = st.alloc_timestamp();
            let card_instance = crate::ids::CardInstanceId(st.next_card_instance);
            st.next_card_instance += 1;
            st.objects.insert(
                id,
                GameObject {
                    id,
                    card_instance,
                    card: token,
                    owner: controller,
                    controller,
                    zone: Zone::Battlefield,
                    timestamp,
                    tapped,
                    damage_marked: 0,
                    summoning_sick: true,
                    counters: Default::default(),
                    is_token: true,
                    deathtouch_damage: false,
                    attached_to: None,
                    last_controller: controller,
                    face: 0,
                    transforms: 0,
                    adventure: None,
                    cast_with: None,
                },
            );
            let o = st.object_mut(id);
            for (kind, n) in counters {
                let have = o.counters.entry(kind).or_default();
                *have = have.saturating_add(n);
            }
            st.zones.battlefield.push(id);
            st.emit(Event::TokenCreated {
                object: id,
                card: token,
                controller,
            });
            ActionResult::Moved(id)
        }
        GameAction::PutCounters { object, kind, n } => {
            if n > 0 {
                let have = st.object_mut(object).counters.entry(kind).or_default();
                *have = have.saturating_add(n);
                st.emit(Event::CountersPut { object, kind, n });
            }
            ActionResult::Done
        }
        GameAction::RemoveCounters { object, kind, n } => {
            let o = st.object_mut(object);
            let have = o.counters.entry(kind).or_default();
            *have -= n.min(*have);
            if *have == 0 {
                o.counters.remove(&kind);
            }
            st.emit(Event::CountersRemoved { object, kind, n });
            ActionResult::Done
        }
        GameAction::CeaseToExist { object } => {
            detach(st, object);
            st.emit(Event::CeasedToExist { object });
            ActionResult::Done
        }
        GameAction::AddContinuous {
            source,
            controller,
            affected,
            effects,
            until_end_of_turn,
            card,
        } => {
            let id = st.next_effect_id;
            st.next_effect_id += 1;
            let timestamp = st.alloc_timestamp();
            st.continuous.push(ContinuousInstance {
                id,
                timestamp,
                source,
                controller,
                affected,
                effects,
                until_end_of_turn,
                card,
            });
            st.emit(Event::ContinuousEffectBegan { id, source });
            ActionResult::Done
        }
        GameAction::AddLasting {
            source,
            controller,
            abilities,
            about,
            until_end_of_turn,
            while_source,
        } => {
            let id = st.next_effect_id;
            st.next_effect_id += 1;
            let timestamp = st.alloc_timestamp();
            st.lasting.push(crate::effects::Lasting {
                id,
                timestamp,
                source,
                controller,
                abilities,
                about,
                until_end_of_turn,
                while_source,
            });
            st.emit(Event::ContinuousEffectBegan { id, source });
            ActionResult::Done
        }
        GameAction::AddDelayed { mut delayed } => {
            delayed.id = st.next_effect_id;
            st.next_effect_id += 1;
            st.delayed.push(delayed);
            ActionResult::Done
        }
        GameAction::RemoveDelayed { id } => {
            st.delayed.retain(|d| d.id != id);
            ActionResult::Done
        }
        GameAction::AddShield { target, amount } => {
            let id = st.next_effect_id;
            st.next_effect_id += 1;
            let timestamp = st.alloc_timestamp();
            st.shields.push(Shield {
                id,
                timestamp,
                target,
                remaining: amount,
            });
            ActionResult::Done
        }
        GameAction::ConsumeShield { id, n } => {
            if let Some(sh) = st.shields.iter_mut().find(|s| s.id == id) {
                sh.remaining -= n.min(sh.remaining);
            }
            st.shields.retain(|s| s.remaining > 0);
            ActionResult::Done
        }
        GameAction::CommanderCast { player, card } => {
            *st.player_mut(player)
                .commander_casts
                .entry(card)
                .or_default() += 1;
            ActionResult::Done
        }
        GameAction::Attach { aura, to } => {
            st.object_mut(aura).attached_to = Some(to);
            st.emit(Event::Attached { aura, to });
            ActionResult::Done
        }
        GameAction::NewTimestamp { object } => {
            let t = st.alloc_timestamp();
            st.object_mut(object).timestamp = t;
            ActionResult::Done
        }
        GameAction::Unattach { object } => {
            st.object_mut(object).attached_to = None;
            st.emit(Event::Unattached { object });
            ActionResult::Done
        }
        GameAction::ControlChanged { object, controller } => {
            let o = st.object_mut(object);
            o.last_controller = controller;
            o.summoning_sick = true;
            // CR 506.4: a permanent whose controller changes is removed from combat. (An
            // attacker it was blocking stays blocked, CR 509.1h.)
            if let Some(c) = st.combat.as_mut() {
                c.attackers.retain(|a| a.attacker != object);
                for a in &mut c.attackers {
                    a.blockers.retain(|b| *b != object);
                }
                st.generation += 1;
            }
            st.emit(Event::ControlChanged { object, controller });
            ActionResult::Done
        }
        GameAction::RemoveFromCombat { object } => {
            // (An attacker it was blocking stays blocked, CR 509.1h.)
            if let Some(c) = st.combat.as_mut() {
                c.attackers.retain(|a| a.attacker != object);
                for a in &mut c.attackers {
                    a.blockers.retain(|b| *b != object);
                }
                st.generation += 1;
            }
            st.emit(Event::RemovedFromCombat { object });
            ActionResult::Done
        }
        GameAction::RecordSourceLki { ability, chars } => {
            if let Some(so) = st.stack.iter_mut().find(|s| s.id == ability) {
                so.source_lki = Some(chars);
            }
            ActionResult::Done
        }
        GameAction::RecordSubjectLki { ability, chars } => {
            if let Some(so) = st.stack.iter_mut().find(|s| s.id == ability) {
                so.subject_lki = Some(chars);
            }
            ActionResult::Done
        }
        GameAction::ExpireEndOfTurn => {
            st.continuous.retain(|c| !c.until_end_of_turn);
            st.shields.clear();
            // (And the ones that are over because what they were about is gone.)
            let over: Vec<u32> = st
                .lasting
                .iter()
                .filter(|e| e.until_end_of_turn || !e.live(st))
                .map(|e| e.id)
                .collect();
            st.lasting.retain(|e| !over.contains(&e.id));
            // Delayed triggered abilities for this turn only (CR 514.2).
            let turn = st.turn.turn_number;
            st.delayed.retain(|d| d.until_turn.is_none_or(|t| t > turn));
            ActionResult::Done
        }
        GameAction::Draw { player } => {
            let Some(&top) = st.zones.libraries[player.index()].last() else {
                st.player_mut(player).drew_from_empty_library = true;
                st.emit(Event::DrewFromEmptyLibrary { player });
                return ActionResult::Drew(None);
            };
            let new = perform_move(st, top, Zone::Hand, LibraryPosition::Top, None, 0);
            st.player_mut(player).drawn_this_turn += 1;
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
        GameAction::SendOnAdventure { spell, player } => {
            st.object_mut(spell).adventure = Some(player);
            ActionResult::Done
        }
        GameAction::Transform { object } => {
            // CR 701.27a: turn it over; CR 613.7g: a new timestamp; CR 701.27c: the same
            // object, with its counters, damage and attachments.
            let timestamp = st.alloc_timestamp();
            let o = st.object_mut(object);
            o.face = if o.face == 0 { 1 } else { 0 };
            o.transforms += 1;
            o.timestamp = timestamp;
            let face = o.face;
            st.emit(Event::Transformed { object, face });
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
        GameAction::AddRestrictedMana {
            player,
            mana,
            ability,
        } => {
            let pool = &mut st.player_mut(player).restricted_mana;
            match pool
                .iter_mut()
                .find(|r| r.mana == mana && r.ability == ability)
            {
                Some(r) => r.amount += 1,
                None => pool.push(crate::state::RestrictedMana {
                    mana,
                    amount: 1,
                    ability,
                }),
            }
            st.emit(Event::ManaAdded { player, mana });
            ActionResult::Done
        }
        GameAction::ReleaseRestrictedMana {
            player,
            entry,
            amount,
        } => {
            let pl = st.player_mut(player);
            let r = &mut pl.restricted_mana[entry as usize];
            debug_assert!(r.amount >= amount);
            r.amount -= amount;
            let mana = r.mana;
            pl.mana_pool.add(mana, amount);
            pl.restricted_mana.retain(|r| r.amount > 0);
            ActionResult::Done
        }
        GameAction::SetSacrificedPower { object, power } => {
            if let Some(so) = st.stack.iter_mut().find(|s| s.id == object) {
                so.sacrificed_power = power;
            }
            ActionResult::Done
        }
        GameAction::EmptyManaPool { player } => {
            let restricted: u32 = st
                .player(player)
                .restricted_mana
                .iter()
                .map(|r| r.amount)
                .sum();
            st.player_mut(player).restricted_mana.clear();
            let pool = &mut st.player_mut(player).mana_pool;
            let amount = pool.total() + restricted;
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
            deathtouch,
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
            // CR 903.10a: combat damage dealt to a player by a commander is tallied per
            // commander card (CR 903.3).
            if combat
                && let DamageTarget::Player(p) = target
                && st.is_commander(source)
            {
                let card = st.object(source).card_instance;
                let tally = st.player_mut(p).commander_damage.entry(card).or_default();
                *tally = tally.saturating_add(amount);
            }
            match target {
                DamageTarget::Player(p) => change_life(st, p, -life_amount(amount)),
                DamageTarget::Object(o) => {
                    let obj = st.object_mut(o);
                    obj.damage_marked = obj.damage_marked.saturating_add(amount);
                    obj.deathtouch_damage |= deathtouch;
                }
            }
            ActionResult::Done
        }
        GameAction::GainLife { player, amount } => {
            if amount > 0 {
                change_life(st, player, life_amount(amount));
            }
            ActionResult::Done
        }
        GameAction::LoseLife { player, amount } => {
            if amount > 0 {
                change_life(st, player, -life_amount(amount));
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
            ability,
            triggered,
            source_lki,
            subject,
            subject_lki,
            facts,
        } => {
            let id = st.alloc_object_id();
            st.stack.push(StackObject {
                id,
                controller,
                kind: StackKind::Ability {
                    source,
                    source_card,
                    ability,
                    triggered,
                },
                source_lki,
                subject,
                subject_lki,
                modes: Vec::new(),
                x: 0,
                targets: Vec::new(),
                sacrificed_power: 0,
                facts,
                // Only a double-faced permanent can transform (CR 712.7).
                source_transforms: st
                    .objects
                    .get(&source)
                    .filter(|o| {
                        o.zone == Zone::Battlefield && st.catalog.get(o.card).layout.double_faced()
                    })
                    .map(|o| o.transforms),
                alt: None,
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
        GameAction::SetCastAlt { object, ability } => {
            if let Some(so) = st.stack.iter_mut().find(|s| s.id == object) {
                so.alt = Some(ability);
            }
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
        GameAction::ArrangeLibrary {
            player,
            top,
            mut bottom,
            random_bottom,
        } => {
            let mut lib = core::mem::take(&mut st.zones.libraries[player.index()]);
            let here = |id: &ObjectId| lib.contains(id);
            let top: Vec<ObjectId> = top.into_iter().filter(here).collect();
            bottom.retain(|id| lib.contains(id) && !top.contains(id));
            if random_bottom {
                st.rng.shuffle(&mut bottom);
            }
            lib.retain(|id| !top.contains(id) && !bottom.contains(id));
            // Index 0 is the bottom of a library and the last entry its top.
            let mut arranged: Vec<ObjectId> = bottom.iter().rev().copied().collect();
            arranged.append(&mut lib);
            arranged.extend(top.iter().rev().copied());
            st.zones.libraries[player.index()] = arranged;
            st.emit(Event::LibraryArranged {
                player,
                top: top.len() as u32,
                bottom: bottom.len() as u32,
            });
            ActionResult::Done
        }
    }
}

/// A damage or life amount as a change of a life total. Amounts saturate (doubled
/// damage, D-073); a life total can't hold more than `i32::MAX`.
fn life_amount(amount: u32) -> i32 {
    i32::try_from(amount).unwrap_or(i32::MAX)
}

fn change_life(st: &mut GameState, p: PlayerId, delta: i32) {
    let ps = st.player_mut(p);
    ps.life = ps.life.saturating_add(delta);
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
            tapped: false,
            counters: Vec::new(),
            face: 0,
        },
    ) {
        ActionResult::Moved(id) => id,
        other => unreachable!("move returned {other:?}"),
    }
}

/// `move_object`, putting the card onto the stack or battlefield with face `face` up
/// (CR 712.8b: a modal double-faced card cast with its back face).
pub fn move_object_face(
    st: &mut GameState,
    object: ObjectId,
    to: Zone,
    controller: Option<PlayerId>,
    face: u8,
) -> ObjectId {
    match execute(
        st,
        GameAction::MoveZone {
            object,
            to,
            position: LibraryPosition::Top,
            controller,
            tapped: false,
            counters: Vec::new(),
            face,
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
    face: u8,
) -> ObjectId {
    // CR 702.74a: a permanent spell's alternative cost is known to the permanent it becomes
    // ("if its evoke cost was paid").
    let cast_with = match to {
        Zone::Battlefield => st.stack_object(id).and_then(|s| s.alt),
        _ => None,
    };
    let old = detach(st, id);
    // CR 712.6a: off the stack and battlefield a card has only its front face up. CR
    // 712.10: a resolving spell becomes a permanent with the face it had on the stack.
    let face = match to {
        Zone::Battlefield | Zone::Stack if face != 0 => face,
        Zone::Battlefield if old.zone == Zone::Stack => old.face,
        _ => 0,
    };
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
        counters: Default::default(),
        is_token: old.is_token,
        deathtouch_damage: false,
        attached_to: None,
        last_controller: controller,
        face,
        transforms: 0,
        // CR 716.3d: what was sent on an Adventure may be cast while it stays exiled.
        adventure: if old.zone == Zone::Stack && to == Zone::Exile {
            old.adventure
        } else {
            None
        },
        cast_with,
    };
    st.objects.insert(new_id, obj);
    if to == Zone::Stack {
        st.stack.push(StackObject {
            id: new_id,
            controller,
            kind: StackKind::Spell,
            source_lki: None,
            subject: None,
            subject_lki: None,
            modes: Vec::new(),
            x: 0,
            targets: Vec::new(),
            sacrificed_power: 0,
            facts: Default::default(),
            source_transforms: None,
            alt: None,
        });
    } else {
        let list = st.zones.list_mut(to, old.owner);
        match (to, position) {
            (Zone::Library, LibraryPosition::Bottom) => list.insert(0, new_id),
            _ => list.push(new_id),
        }
    }
    // CR 903.9a: remember commanders put into a graveyard or exile until SBAs are checked.
    if matches!(to, Zone::Graveyard | Zone::Exile)
        && st.commanders.iter().any(|(_, c)| *c == old.card_instance)
    {
        st.commander_moved.push(new_id);
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
    // Control effects from objects that just left have ended (layer 2 is recomputed).
    st.generation += 1;
    // (3) Objects still controlled by that player are exiled.
    let controlled: Vec<ObjectId> = st
        .objects
        .values()
        .filter(|o| matches!(o.zone, Zone::Battlefield | Zone::Stack))
        .map(|o| o.id)
        .filter(|&id| crate::layers::controller_of(st, id) == p)
        .collect();
    for id in controlled {
        perform_move(st, id, Zone::Exile, LibraryPosition::Top, None, 0);
    }
    // The layered view was worked out (by `controller_of`) before those moves, at this
    // generation: it is stale now. (Found by the D-069 soak: a reanimated permanent of
    // another player's, exiled as its controller left, was still in the view.) Dropping
    // the derived cache leaves the serialized state — and its hash — as it was.
    st.layer_cache.invalidate();
    // Their mana pool and life no longer matter; priority is handled by the priority frame.
    st.player_mut(p).mana_pool = Default::default();
    st.player_mut(p).restricted_mana.clear();
    st.emit(Event::PlayerLeftGame { player: p });
}
