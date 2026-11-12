//! Characteristic computation: the layer system (CR 613 (2026-11-06)).
//! Text: docs/rules-notes/613-layers.md.
//!
//! `chars(st, id)` returns an object's current characteristics. For permanents they come
//! from a layered view of the whole battlefield, recomputed when `GameState::generation`
//! changes (every performed action bumps it) and memoized in between.
//!
//! The walk:
//! - Starts from printed values (CR 613.1).
//! - Visits every layer/sublayer slot in order: 1a, 1b, 2, 3, 4, 5, 6, 7a, 7b, 7c, 7d.
//!   Only layers 4–7 have effect vocabulary so far (D-023).
//! - Within a layer, orders effects by dependency, then timestamp (CR 613.7–613.8): pick an
//!   effect that depends on no other unapplied effect, earliest timestamp first. If none
//!   qualifies (a dependency loop), take the earliest timestamp. Re-evaluate after each
//!   application (613.8c).
//! - A static ability's effect exists only while its source has that ability *when the
//!   effect would begin to apply* (CR 604.2). Once it has applied in one layer it keeps
//!   applying, to the same set of objects, in later layers (CR 613.6) — CLAUDE.md
//!   invariant 4 as qualified by 613.6.
//! - Effects from resolved spells use the set locked in when they began (CR 611.2c).
//! - 7c effects and counters are additions, so their relative order doesn't matter.

use crate::effects::ContinuousInstance;
use crate::ids::{ObjectId, PlayerId, Timestamp};
use crate::objects::GameObject;
use crate::state::GameState;
use crate::zones::Zone;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use card_model::{
    AbilityDef, AbilityId, Affected, Amount, BasicLandType, CardDef, CardDefId, CardType, Color,
    Condition, ContinuousEffect, CounterKind, Keyword, ManaCost, Selector, Supertype, Who,
};
use core::cell::RefCell;
use core::fmt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Characteristics {
    pub name: String,
    pub mana_cost: Option<ManaCost>,
    pub colors: Vec<Color>,
    pub supertypes: Vec<Supertype>,
    pub card_types: Vec<CardType>,
    /// Creature types and other non-land subtypes.
    pub subtypes: Vec<String>,
    pub land_types: Vec<BasicLandType>,
    pub keywords: Vec<Keyword>,
    /// The abilities the object has: the printed ones it still has (layer 6 can remove
    /// them) and the ones other objects or effects give it (CR 113.2c). The same ability
    /// can be listed more than once: given twice, it is had twice (CR 113.2c).
    pub abilities: Vec<AbilityId>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    /// Layer 2 (CR 613.1b). Off the battlefield/stack: the owner.
    pub controller: PlayerId,
    /// It lost all abilities (layer 6) — also the "{T}: Add [mana]" its basic land types
    /// give it (CR 305.6: an ability it has, though no text says so).
    #[serde(default)]
    pub land_abilities_lost: bool,
    /// It is every creature type (CR 702.73a, changeling): a characteristic-defining
    /// ability, applied before anything else in layer 4 (CR 613.3), in every zone.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub all_creature_types: bool,
}

impl Characteristics {
    /// `card` is `d`'s id in the catalog.
    pub fn printed(card: CardDefId, d: &CardDef, controller: PlayerId) -> Self {
        Characteristics {
            controller,
            name: d.name.clone(),
            mana_cost: d.mana_cost.clone(),
            colors: d.printed_colors(),
            supertypes: d.supertypes.clone(),
            card_types: d.card_types.clone(),
            subtypes: d.creature_types.clone(),
            land_types: d.land_types.clone(),
            keywords: d.keywords.clone(),
            abilities: (0..d.abilities.len() as u16)
                .map(|i| AbilityId::printed(card, i))
                .collect(),
            power: d.power,
            toughness: d.toughness,
            land_abilities_lost: false,
            all_creature_types: d.has_keyword(Keyword::Changeling),
        }
    }

    pub fn is(&self, t: CardType) -> bool {
        self.card_types.contains(&t)
    }

    pub fn has(&self, k: Keyword) -> bool {
        self.keywords.contains(&k)
    }

    /// Does it have this subtype (CR 205.3)? Basic land types are subtypes too: "Mountains
    /// you control" counts every land with the type Mountain.
    pub fn has_subtype(&self, name: &str) -> bool {
        self.subtypes.iter().any(|s| s == name)
            || (self.all_creature_types && card_model::is_creature_type(name))
            || self.land_types.iter().any(|t| {
                name == match t {
                    BasicLandType::Plains => "Plains",
                    BasicLandType::Island => "Island",
                    BasicLandType::Swamp => "Swamp",
                    BasicLandType::Mountain => "Mountain",
                    BasicLandType::Forest => "Forest",
                }
            })
    }

    /// CR 110.4: permanent card types.
    pub fn is_permanent_type(&self) -> bool {
        [
            CardType::Artifact,
            CardType::Battle,
            CardType::Creature,
            CardType::Enchantment,
            CardType::Land,
            CardType::Planeswalker,
        ]
        .iter()
        .any(|t| self.is(*t))
    }
}

type View = BTreeMap<ObjectId, Characteristics>;

/// Memo of the layered battlefield view, keyed by `GameState::generation`. The flag says
/// the view is being worked out right now: nothing asked from inside the layer walk may
/// need the finished view (see [`chars`]).
#[derive(Clone, Default)]
pub struct LayerCache(RefCell<Option<(u64, Arc<View>)>>, core::cell::Cell<bool>);

impl fmt::Debug for LayerCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LayerCache")
    }
}

/// The current characteristics of object `id` (CLAUDE.md invariant 3).
pub fn chars(st: &GameState, id: ObjectId) -> Characteristics {
    if st.object(id).zone == Zone::Battlefield
        && let Some(c) = view(st).get(&id)
    {
        return c.clone();
    }
    let o = st.object(id);
    let def = st.card_def(id);
    let mut c = Characteristics::printed(o.card, def, o.controller);
    // Inside the layer walk only what is printed is looked at for a card that isn't on
    // the battlefield (its types — never its power or toughness: card-db sees to that), and
    // working out its "*" could need the very view being built.
    if st.layer_cache.1.get() {
        return c;
    }
    // CR 604.3: characteristic-defining abilities function in every zone.
    for a in &def.abilities {
        if let AbilityDef::Static {
            affected: Affected::This,
            effects,
            ..
        } = a
        {
            for e in effects {
                if let ContinuousEffect::SetPTBy { power, toughness } = e {
                    let you = if o.zone == Zone::Stack {
                        o.controller
                    } else {
                        o.owner
                    };
                    if let Some(p) = power {
                        c.power = Some(count_amount(st, p, id, you) as i32);
                    }
                    if let Some(t) = toughness {
                        c.toughness = Some(count_amount(st, t, id, you) as i32);
                    }
                }
            }
        }
    }
    c
}

/// Notice permanents whose layered controller changed since the engine last looked, and
/// record it (summoning sickness, CR 302.6). Called by the driver after every frame step.
pub(crate) fn sync_control(st: &mut GameState) {
    let v = view(st);
    let changed: Vec<(ObjectId, PlayerId)> = v
        .iter()
        .filter(|(id, c)| st.object(**id).last_controller != c.controller)
        .map(|(id, c)| (*id, c.controller))
        .collect();
    for (object, controller) in changed {
        crate::actions::execute(
            st,
            crate::actions::GameAction::ControlChanged { object, controller },
        );
    }
    // CR 506.4: an attacking or blocking creature that stops being a creature (a God
    // whose devotion fell) is removed from combat.
    if let Some(c) = &st.combat {
        let v = view(st);
        let mut gone: Vec<ObjectId> = Vec::new();
        for a in &c.attackers {
            for id in core::iter::once(a.attacker).chain(a.blockers.iter().copied()) {
                if v.get(&id).is_some_and(|ch| !ch.is(CardType::Creature)) && !gone.contains(&id) {
                    gone.push(id);
                }
            }
        }
        for object in gone {
            crate::actions::execute(st, crate::actions::GameAction::RemoveFromCombat { object });
        }
    }
}

/// The current controller of `id`: layered on the battlefield (CR 613.1b), the controller of
/// a spell on the stack, otherwise its owner.
pub fn controller_of(st: &GameState, id: ObjectId) -> PlayerId {
    let o = st.object(id);
    if o.zone == Zone::Battlefield
        && let Some(c) = view(st).get(&id)
    {
        return c.controller;
    }
    o.controller
}

/// The abilities `id` currently has — printed on it or given to it — with their names.
pub fn abilities(st: &GameState, id: ObjectId) -> Vec<(AbilityId, &AbilityDef)> {
    chars(st, id)
        .abilities
        .into_iter()
        .map(|a| (a, st.catalog.ability(a)))
        .collect()
}

/// The layered view of every permanent on the battlefield.
pub fn view(st: &GameState) -> Arc<View> {
    if let Some((generation, v)) = &*st.layer_cache.0.borrow()
        && *generation == st.generation
    {
        return v.clone();
    }
    debug_assert!(
        !st.layer_cache.1.get(),
        "the layered view was asked for while it was being worked out"
    );
    st.layer_cache.1.set(true);
    let v = Arc::new(compute(st));
    st.layer_cache.1.set(false);
    *st.layer_cache.0.borrow_mut() = Some((st.generation, v.clone()));
    v
}

/// Does object `o` (with characteristics `c`) match `sel`, relative to an ability whose
/// source is `source`, controlled by `controller`?
pub fn matches(
    st: &GameState,
    sel: &Selector,
    c: &Characteristics,
    o: &GameObject,
    source: ObjectId,
    controller: PlayerId,
) -> bool {
    let (attacking, blocking) = combat_role(st, o.id);
    let facts = Facts {
        id: o.id,
        controller: c.controller,
        owner: o.owner,
        is_token: o.is_token,
        tapped: o.tapped,
        attacking,
        blocking,
    };
    // "Of the chosen type": the choice of the ability's source (CR 607.2d).
    let sel = crate::chosen::bind_for(st, sel, source);
    matches_parts(&sel, c, facts, source, controller)
}

/// Is `id` an attacking creature / a blocking creature right now (CR 506.4)?
pub fn combat_role(st: &GameState, id: ObjectId) -> (bool, bool) {
    match &st.combat {
        None => (false, false),
        Some(c) => (
            c.attackers.iter().any(|a| a.attacker == id),
            c.attackers.iter().any(|a| a.blockers.contains(&id)),
        ),
    }
}

/// What a selector looks at besides an object's characteristics.
#[derive(Clone, Copy, Debug)]
pub struct Facts {
    pub id: ObjectId,
    pub controller: PlayerId,
    pub owner: PlayerId,
    pub is_token: bool,
    pub tapped: bool,
    pub attacking: bool,
    pub blocking: bool,
}

/// The permanents an amount counts, if it counts permanents ("the number of …", possibly
/// with arithmetic on it).
fn counted_selector(a: &card_model::Amount) -> Option<&Selector> {
    use card_model::Amount;
    match a {
        Amount::Count(s) => Some(s),
        Amount::Calc(base, _) => counted_selector(base),
        Amount::IfAtLeast { test, .. } => counted_selector(test),
        _ => None,
    }
}

/// An amount read off the battlefield ("the number of …", "the total power of …"); 0 for
/// amounts that depend on a resolving spell or ability.
/// Is `player` `who`, relative to `you`?
fn who_is(who: card_model::Who, player: PlayerId, you: PlayerId) -> bool {
    match who {
        card_model::Who::Any => true,
        card_model::Who::You => player == you,
        card_model::Who::Opponent => player != you,
    }
}

pub fn count_amount(
    st: &GameState,
    a: &card_model::Amount,
    source: ObjectId,
    controller: PlayerId,
) -> u32 {
    use card_model::Amount;
    match a {
        Amount::Fixed(n) => *n,
        Amount::Count(sel) => matching(st, sel, source, controller).len() as u32,
        Amount::TotalPower(sel) => matching(st, sel, source, controller)
            .into_iter()
            .map(|id| chars(st, id).power.unwrap_or(0).max(0) as u32)
            .sum(),
        Amount::CardsIn { zone, cards } => {
            // A card outside the battlefield is looked at as printed unless its power or
            // toughness is asked about (then a characteristic-defining ability counts,
            // CR 604.3). The layer walk only ever gets here the first way.
            let by_pt = cards.iter().any(|s| s.looks_at_pt());
            let fits = |id: &ObjectId| {
                cards.is_empty() || {
                    let o = st.object(*id);
                    let c = if by_pt || o.zone == Zone::Battlefield {
                        chars(st, *id)
                    } else {
                        Characteristics::printed(o.card, st.card_def(*id), o.owner)
                    };
                    cards
                        .iter()
                        .any(|s| matches(st, s, &c, st.object(*id), source, controller))
                }
            };
            match zone {
                card_model::CardZone::Battlefield => {
                    st.zones.battlefield.iter().filter(|id| fits(id)).count() as u32
                }
                zone => {
                    let zone = match zone {
                        card_model::CardZone::Hand => Zone::Hand,
                        card_model::CardZone::Library => Zone::Library,
                        _ => Zone::Graveyard,
                    };
                    st.players
                        .iter()
                        .map(|p| {
                            st.zones
                                .list(zone, p.id)
                                .iter()
                                .filter(|id| fits(id))
                                .count()
                        })
                        .sum::<usize>() as u32
                }
            }
        }
        Amount::Calc(base, op) => op.apply(count_amount(st, base, source, controller)),
        Amount::IfAtLeast {
            test,
            at_least,
            then,
            otherwise,
        } => {
            if count_amount(st, test, source, controller) >= *at_least {
                *then
            } else {
                *otherwise
            }
        }
        // CR 305.6 / "domain".
        Amount::Domain => {
            let mut types: Vec<card_model::BasicLandType> = Vec::new();
            for &id in &st.zones.battlefield {
                let c = chars(st, id);
                if c.controller == controller && c.is(CardType::Land) {
                    for t in &c.land_types {
                        if !types.contains(t) {
                            types.push(*t);
                        }
                    }
                }
            }
            types.len() as u32
        }
        // CR 700.5.
        Amount::Devotion(colors) => st
            .zones
            .battlefield
            .iter()
            .map(|&id| chars(st, id))
            .filter(|c| c.controller == controller)
            .map(|c| {
                c.mana_cost.as_ref().map_or(0, |m| {
                    m.symbols
                        .iter()
                        .filter(|s| {
                            matches!(s, card_model::ManaSymbol::Colored(x) if colors.contains(x))
                        })
                        .count() as u32
                })
            })
            .sum(),
        Amount::Greatest { of, what } => matching(st, of, source, controller)
            .into_iter()
            .map(|id| {
                let c = chars(st, id);
                match what {
                    card_model::Measure::Power => c.power.unwrap_or(0).max(0) as u32,
                    card_model::Measure::Toughness => c.toughness.unwrap_or(0).max(0) as u32,
                    card_model::Measure::ManaValue => {
                        c.mana_cost.as_ref().map_or(0, |m| m.mana_value())
                    }
                }
            })
            .max()
            .unwrap_or(0),
        Amount::HandSize => st.zones.list(Zone::Hand, controller).len() as u32,
        Amount::LifeTotal => st.player(controller).life.max(0) as u32,
        // CR 102.2–3: players still in the game.
        Amount::Players(who) => st
            .players
            .iter()
            .filter(|p| !p.has_lost && who_is(*who, p.id, controller))
            .count() as u32,
        Amount::LifeAmong { players, highest } => {
            let lives = st
                .players
                .iter()
                .filter(|p| !p.has_lost && who_is(*players, p.id, controller))
                .map(|p| p.life.max(0) as u32);
            if *highest { lives.max() } else { lives.min() }.unwrap_or(0)
        }
        Amount::X
        | Amount::SourcePower
        | Amount::SourceToughness
        | Amount::TriggeringPower
        | Amount::TriggeringToughness
        | Amount::TriggeringAmount
        | Amount::SacrificedPower
        | Amount::OfTarget { .. }
        | Amount::HandOf(_) => 0,
    }
}

/// Every permanent matching `sel` right now, in battlefield (timestamp) order.
pub fn matching(
    st: &GameState,
    sel: &Selector,
    source: ObjectId,
    controller: PlayerId,
) -> Vec<ObjectId> {
    st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| matches(st, sel, &chars(st, id), st.object(id), source, controller))
        .collect()
}

/// [`matches`] for an object that may no longer exist (last known information).
pub fn matches_parts(
    sel: &Selector,
    c: &Characteristics,
    facts: Facts,
    source: ObjectId,
    controller: PlayerId,
) -> bool {
    let Facts {
        id,
        controller: object_controller,
        owner,
        is_token,
        tapped,
        attacking,
        blocking,
    } = facts;
    (sel.types.is_empty() || sel.types.iter().any(|t| c.is(*t)))
        && !sel.not_types.iter().any(|t| c.is(*t))
        && sel.supertypes.iter().all(|s| c.supertypes.contains(s))
        && (sel.subtypes.is_empty() || sel.subtypes.iter().any(|s| c.has_subtype(s)))
        && !sel.not_subtypes.iter().any(|s| c.has_subtype(s))
        && (sel.colors.is_empty() || sel.colors.iter().any(|x| c.colors.contains(x)))
        && sel
            .power_min
            .is_none_or(|n| c.power.is_some_and(|p| p >= n))
        && sel
            .toughness_min
            .is_none_or(|n| c.toughness.is_some_and(|t| t >= n))
        && sel.tapped.is_none_or(|t| tapped == t)
        // "attacking or blocking": either is enough when both are asked for.
        && match (sel.attacking, sel.blocking) {
            (Some(true), Some(true)) => attacking || blocking,
            (a, b) => a.is_none_or(|a| attacking == a) && b.is_none_or(|b| blocking == b),
        }
        && sel.keywords.iter().all(|k| c.has(*k))
        && !sel.not_keywords.iter().any(|k| c.has(*k))
        && sel
            .power_max
            .is_none_or(|n| c.power.is_some_and(|p| p <= n))
        && match sel.controller {
            Who::Any => true,
            Who::You => object_controller == controller,
            Who::Opponent => object_controller != controller,
        }
        && match sel.owner {
            Who::Any => true,
            Who::You => owner == controller,
            Who::Opponent => owner != controller,
        }
        && !(sel.other && id == source)
        && sel.token.is_none_or(|t| is_token == t)
        && {
            // CR 202.3: no mana cost is mana value 0.
            let mv = c.mana_cost.as_ref().map_or(0, |m| m.mana_value());
            sel.mana_value_min.is_none_or(|n| mv >= n) && sel.mana_value_max.is_none_or(|n| mv <= n)
        }
        && (sel.names.is_empty() || sel.names.contains(&c.name))
        && !sel.not_colors.iter().any(|x| c.colors.contains(x))
        && !sel.not_supertypes.iter().any(|x| c.supertypes.contains(x))
        && sel
            .toughness_max
            .is_none_or(|n| c.toughness.is_some_and(|t| t <= n))
        // A reference to a choice still here wasn't bound (`chosen::bind`): nothing was
        // chosen (CR 607.5a) — "of the chosen type" is nothing, "not of it" everything.
        && match sel.chosen {
            None | Some(card_model::OfChosen::NotType) => true,
            Some(card_model::OfChosen::Type | card_model::OfChosen::Color) => false,
        }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    L1a,
    L1b,
    L2,
    L3,
    L4,
    L5,
    L6,
    L7a,
    L7b,
    L7c,
    L7d,
}

const LAYERS: [Layer; 11] = [
    Layer::L1a,
    Layer::L1b,
    Layer::L2,
    Layer::L3,
    Layer::L4,
    Layer::L5,
    Layer::L6,
    Layer::L7a,
    Layer::L7b,
    Layer::L7c,
    Layer::L7d,
];

fn layer_of(e: &ContinuousEffect) -> Layer {
    match e {
        ContinuousEffect::GainControl => Layer::L2,
        ContinuousEffect::AddTypes { .. } => Layer::L4,
        ContinuousEffect::SetColors(_) => Layer::L5,
        ContinuousEffect::AddKeywords(_)
        | ContinuousEffect::LoseKeywords(_)
        | ContinuousEffect::LoseAllAbilities
        | ContinuousEffect::GrantAbilities(_) => Layer::L6,
        ContinuousEffect::SetPTBy { .. } => Layer::L7a,
        ContinuousEffect::SetPT(..) => Layer::L7b,
        ContinuousEffect::ModifyPT(..) | ContinuousEffect::ModifyPTBy { .. } => Layer::L7c,
        ContinuousEffect::SwitchPT => Layer::L7d,
        ContinuousEffect::RemoveTypes(_) | ContinuousEffect::AddChosenType => Layer::L4,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    /// Printed static ability `index` of `source`.
    Static {
        source: ObjectId,
        index: u16,
    },
    Resolved(u32),
    /// A static ability an object was given in this walk (layer 6), numbered as found.
    Granted(u32),
    /// Printed ability `index` of card `source`, a static ability that works from the zone
    /// the card is in (`AbilityDef::FromZone`, CR 113.6b).
    FromZone {
        source: ObjectId,
        index: u16,
    },
}

/// One continuous effect taking part in the walk.
struct Eff<'a> {
    key: Key,
    timestamp: Timestamp,
    source: ObjectId,
    controller: PlayerId,
    parts: &'a [ContinuousEffect],
    affected: Aff<'a>,
    /// "as long as …": a static effect exists only while this holds.
    cond: Option<&'a Condition>,
    /// The card the effect is written on: `GrantAbilities` names abilities of its
    /// `granted` list.
    card: CardDefId,
    /// For a static ability: the ability its source must still have (invariant 4).
    held: Option<AbilityId>,
}

enum Aff<'a> {
    /// CR 611.2c: fixed when a resolved effect began.
    Locked(&'a [ObjectId]),
    /// CR 611.3a: whatever matches now.
    Dynamic(&'a Affected),
}

struct Walk<'a> {
    st: &'a GameState,
    /// CR 613.6: static effects that already applied in an earlier layer, with their set.
    started: BTreeMap<Key, Vec<ObjectId>>,
    /// Given static abilities that were lost again before they applied. (Being given the
    /// same ability once more makes a new effect, not this one.)
    lost: Vec<Key>,
}

impl Walk<'_> {
    fn exists(&self, e: &Eff<'_>, view: &View) -> bool {
        if matches!(e.key, Key::FromZone { .. }) {
            return self.started.contains_key(&e.key) || self.holds(e, view);
        }
        match e.held {
            None => true,
            Some(held) => {
                self.started.contains_key(&e.key)
                    || (!self.lost.contains(&e.key)
                        && view
                            .get(&e.source)
                            .is_some_and(|c| c.abilities.contains(&held))
                        && self.holds(e, view))
            }
        }
    }

    /// Does the effect's "as long as" condition hold, judged by the view as computed so
    /// far? (Never through `chars`: that would be this computation calling itself.)
    fn holds(&self, e: &Eff<'_>, view: &View) -> bool {
        let Some(cond) = e.cond else {
            return true;
        };
        let you = self.you(e, view);
        let count = |sel: &Selector| {
            view.iter()
                .filter(|(id, c)| matches(self.st, sel, c, self.st.object(**id), e.source, you))
                .count()
        };
        match cond {
            Condition::LifeAtMost(n) => self.st.player(you).life <= *n,
            Condition::LifeAtLeast(n) => self.st.player(you).life >= *n,
            Condition::Control { what, at_least } => count(what) >= *at_least as usize,
            Condition::ControlAtMost { what, at_most } => count(what) <= *at_most as usize,
            // Cards in a graveyard have their printed characteristics.
            Condition::CardsInGraveyard { cards, at_least } => {
                self.st
                    .zones
                    .list(Zone::Graveyard, you)
                    .iter()
                    .filter(|&&id| {
                        let cards = crate::chosen::bind_for(self.st, cards, e.source);
                        crate::targeting::card_matches(self.st, id, &cards)
                    })
                    .count()
                    >= *at_least as usize
            }
            Condition::YourTurn => self.st.turn.active == you,
            Condition::NotYourTurn => self.st.turn.active != you,
            Condition::HandAtMost(n) => self.st.zones.hands[you.index()].len() <= *n as usize,
            // (card-db: only amounts the walk can count in place.)
            Condition::Compare { amount, cmp, n } => {
                cmp.holds(self.amount(amount, e, view).max(0) as u32, *n)
            }
        }
    }

    /// The value of "+N" in a static "+N/+N for each …" right now.
    fn amount(&self, a: &Amount, e: &Eff<'_>, view: &View) -> i32 {
        match a {
            Amount::Fixed(n) => *n as i32,
            Amount::Count(sel) => {
                let you = self.you(e, view);
                view.iter()
                    .filter(|(id, c)| matches(self.st, sel, c, self.st.object(**id), e.source, you))
                    .count() as i32
            }
            // Lands' types are settled in layer 4, mana costs are never changed.
            Amount::Domain => {
                let you = self.you(e, view);
                let mut types: Vec<card_model::BasicLandType> = Vec::new();
                for c in view.values() {
                    if c.controller == you && c.is(CardType::Land) {
                        for t in &c.land_types {
                            if !types.contains(t) {
                                types.push(*t);
                            }
                        }
                    }
                }
                types.len() as i32
            }
            Amount::Devotion(colors) => {
                let you = self.you(e, view);
                view.values()
                    .filter(|c| c.controller == you)
                    .map(|c| {
                        c.mana_cost.as_ref().map_or(0, |m| {
                            m.symbols
                                .iter()
                                .filter(|s| {
                                    matches!(s, card_model::ManaSymbol::Colored(x)
                                        if colors.contains(x))
                                })
                                .count() as i32
                        })
                    })
                    .sum()
            }
            // Nothing the walk is working out: cards in other zones, hands, life totals.
            Amount::CardsIn { .. }
            | Amount::HandSize
            | Amount::LifeTotal
            | Amount::Players(_)
            | Amount::LifeAmong { .. } => {
                count_amount(self.st, a, e.source, self.you(e, view)) as i32
            }
            Amount::Calc(base, op) => op.apply(self.amount(base, e, view).max(0) as u32) as i32,
            Amount::IfAtLeast {
                test,
                at_least,
                then,
                otherwise,
            } => {
                if self.amount(test, e, view) >= *at_least as i32 {
                    *then as i32
                } else {
                    *otherwise as i32
                }
            }
            // card-db rejects anything else in a static ability
            // (`Amount::countable_in_layers`).
            _ => 0,
        }
    }

    /// "You" for this effect: a static ability's source's current controller (layer 2 may
    /// have changed it), or the controller of a resolved effect.
    fn you(&self, e: &Eff<'_>, view: &View) -> PlayerId {
        match e.key {
            Key::Static { .. } | Key::Granted(_) => {
                view.get(&e.source).map_or(e.controller, |c| c.controller)
            }
            Key::Resolved(_) | Key::FromZone { .. } => e.controller,
        }
    }

    fn affected(&self, e: &Eff<'_>, view: &View) -> Vec<ObjectId> {
        if let Some(set) = self.started.get(&e.key) {
            return set
                .iter()
                .copied()
                .filter(|o| view.contains_key(o))
                .collect();
        }
        match e.affected {
            Aff::Locked(ids) => ids
                .iter()
                .copied()
                .filter(|o| view.contains_key(o))
                .collect(),
            Aff::Dynamic(Affected::This) => {
                if view.contains_key(&e.source) {
                    alloc::vec![e.source]
                } else {
                    Vec::new()
                }
            }
            Aff::Dynamic(Affected::All(sel)) => {
                let you = self.you(e, view);
                view.iter()
                    .filter(|(id, c)| matches(self.st, sel, c, self.st.object(**id), e.source, you))
                    .map(|(id, _)| *id)
                    .collect()
            }
            // Only lasting effects are about something; they change no characteristics
            // (card-db sees to that).
            Aff::Dynamic(Affected::Remembered) => Vec::new(),
            // CR 303.4m: "enchanted [object]" = whatever the source is attached to.
            Aff::Dynamic(Affected::Enchanted) => self
                .st
                .objects
                .get(&e.source)
                .and_then(|o| o.attached_to)
                .filter(|t| view.contains_key(t))
                .into_iter()
                .collect(),
        }
    }

    fn apply(&self, e: &Eff<'_>, layer: Layer, set: &[ObjectId], view: &mut View) {
        let you = self.you(e, view);
        for part in e.parts.iter().filter(|p| layer_of(p) == layer) {
            // "+X/+X for each …" in a static ability: counted now (CR 611.3a: it isn't
            // locked in), before anything is changed.
            let counted;
            let part = match part {
                // CR 613.4a: a characteristic-defining ability sets power and/or toughness.
                ContinuousEffect::SetPTBy { power, toughness } => {
                    let p = power.as_ref().map(|a| self.amount(a, e, view));
                    let t = toughness.as_ref().map(|a| self.amount(a, e, view));
                    for id in set {
                        if let Some(c) = view.get_mut(id) {
                            if let Some(p) = p {
                                c.power = Some(p);
                            }
                            if let Some(t) = t {
                                c.toughness = Some(t);
                            }
                        }
                    }
                    continue;
                }
                // CR 113.2c: the object has the ability (once more, if it had it).
                ContinuousEffect::GrantAbilities(indices) => {
                    for id in set {
                        if let Some(c) = view.get_mut(id) {
                            c.abilities.extend(indices.iter().map(|&index| AbilityId {
                                card: e.card,
                                index,
                                granted: true,
                            }));
                        }
                    }
                    continue;
                }
                // CR 607.2d: the type chosen for the effect's source; nothing if none was
                // (CR 607.5a). CR 205.3d: only a creature or kindred can gain it.
                ContinuousEffect::AddChosenType => {
                    let chosen = crate::chosen::of_source(self.st, e.source);
                    if let Some(t) = chosen.iter().find_map(|c| match c {
                        card_model::Chosen::CreatureType(t) => Some(t.name()),
                        card_model::Chosen::Color(_) => None,
                    }) {
                        for id in set {
                            if let Some(c) = view.get_mut(id)
                                && (c.is(CardType::Creature) || c.is(CardType::Kindred))
                                && !c.subtypes.iter().any(|s| s == t)
                            {
                                c.subtypes.push(t.into());
                            }
                        }
                    }
                    continue;
                }
                ContinuousEffect::ModifyPTBy { power, toughness } => {
                    let value = |s: &card_model::Signed| {
                        let n = self.amount(&s.amount, e, view);
                        if s.negative { -n } else { n }
                    };
                    counted = ContinuousEffect::ModifyPT(value(power), value(toughness));
                    &counted
                }
                other => other,
            };
            for id in set {
                if let Some(c) = view.get_mut(id) {
                    apply_part(c, part, you);
                }
            }
        }
    }

    /// Can anything still change whether `a` exists or what it applies to? Not for a
    /// resolved effect (it exists until it ends and its set was locked in, CR 611.2c), nor
    /// for a static effect that began applying in an earlier layer (CR 613.6).
    fn can_depend(&self, a: &Eff<'_>) -> bool {
        (a.held.is_some() || matches!(a.key, Key::FromZone { .. }))
            && !self.started.contains_key(&a.key)
    }

    /// A cheap, conservative pre-check for [`Walk::depends`]: could what `b` does in this
    /// layer change `a`'s existence or the set `a` applies to? `false` only when it
    /// certainly can't — then the full comparison (which copies the whole view) is skipped.
    /// Without this, many effects in one layer cost a copy of the view per pair of them.
    fn could_matter(a: &Eff<'_>, b: &Eff<'_>, layer: Layer) -> bool {
        let affected = match a.affected {
            Aff::Dynamic(Affected::All(sel)) => Some(sel),
            // "This" and "enchanted": the set doesn't look at characteristics. Only the
            // ability's existence can change.
            Aff::Dynamic(Affected::This | Affected::Enchanted | Affected::Remembered) => None,
            Aff::Locked(_) => return false,
        };
        // The permanents an "as long as" condition counts are looked at the same way.
        let counted = match a.cond {
            Some(Condition::Control { what, .. } | Condition::ControlAtMost { what, .. }) => {
                Some(what)
            }
            Some(Condition::Compare { amount, .. }) => counted_selector(amount),
            _ => None,
        };
        b.parts.iter().filter(|p| layer_of(p) == layer).any(|p| {
            [affected, counted].into_iter().flatten().any(|s| match p {
                ContinuousEffect::AddTypes { .. }
                | ContinuousEffect::RemoveTypes(_)
                | ContinuousEffect::AddChosenType => true,
                ContinuousEffect::SetColors(_) => !s.colors.is_empty(),
                ContinuousEffect::AddKeywords(_) | ContinuousEffect::LoseKeywords(_) => {
                    !s.keywords.is_empty() || !s.not_keywords.is_empty()
                }
                ContinuousEffect::SetPT(..)
                | ContinuousEffect::SetPTBy { .. }
                | ContinuousEffect::ModifyPT(..)
                | ContinuousEffect::ModifyPTBy { .. }
                | ContinuousEffect::SwitchPT => s.looks_at_pt(),
                ContinuousEffect::LoseAllAbilities | ContinuousEffect::GainControl => true,
                // No selector looks at abilities other than keywords.
                ContinuousEffect::GrantAbilities(_) => false,
            }) || matches!(
                p,
                // The ability itself can be removed; whose it is changes "you".
                ContinuousEffect::LoseAllAbilities | ContinuousEffect::GainControl
            )
        })
    }

    /// CR 613.8a: does `a` depend on `b`? (Same layer is given.) Applying `b` would change
    /// `a`'s existence or what it applies to. What our effects *do* never depends on other
    /// effects, and no CDAs exist yet (613.8a(c)).
    fn depends(&self, a: &Eff<'_>, b: &Eff<'_>, layer: Layer, view: &View) -> bool {
        if a.key == b.key || !self.can_depend(a) || !Self::could_matter(a, b, layer) {
            return false;
        }
        let before = (self.exists(a, view), self.affected(a, view));
        let mut after_view = view.clone();
        let b_set = self.affected(b, view);
        self.apply(b, layer, &b_set, &mut after_view);
        let after = (self.exists(a, &after_view), self.affected(a, &after_view));
        before != after
    }
}

fn apply_part(c: &mut Characteristics, part: &ContinuousEffect, you: PlayerId) {
    fn add<T: PartialEq + Clone>(v: &mut Vec<T>, items: &[T]) {
        for i in items {
            if !v.contains(i) {
                v.push(i.clone());
            }
        }
    }
    match part {
        // Layer 2: "you control [it]".
        ContinuousEffect::GainControl => c.controller = you,
        ContinuousEffect::AddTypes {
            card_types,
            subtypes,
            land_types,
        } => {
            add(&mut c.card_types, card_types);
            add(&mut c.subtypes, subtypes);
            add(&mut c.land_types, land_types);
        }
        ContinuousEffect::SetColors(colors) => {
            c.colors = colors.clone();
            c.colors.sort();
        }
        ContinuousEffect::AddKeywords(k) => add(&mut c.keywords, k),
        ContinuousEffect::LoseKeywords(k) => c.keywords.retain(|x| !k.contains(x)),
        ContinuousEffect::LoseAllAbilities => {
            c.keywords.clear();
            c.abilities.clear();
            c.land_abilities_lost = true;
        }
        ContinuousEffect::SetPT(p, t) => {
            c.power = Some(*p);
            c.toughness = Some(*t);
        }
        ContinuousEffect::ModifyPT(p, t) => {
            if let (Some(pw), Some(tg)) = (c.power.as_mut(), c.toughness.as_mut()) {
                *pw += p;
                *tg += t;
            }
        }
        // Fixed into a `ModifyPT` when the effect began (frames::resolve); never stored.
        ContinuousEffect::ModifyPTBy { .. } => {}
        // Counted by the walk (`Walk::apply`).
        ContinuousEffect::SetPTBy { .. } => {}
        // Done by the walk, which knows the card the abilities are written on — and the
        // effect's source, whose choice it is.
        ContinuousEffect::GrantAbilities(_) | ContinuousEffect::AddChosenType => {}
        // CR 205.1: a type-removing effect; subtypes are kept (a God that isn't a
        // creature is still a God).
        ContinuousEffect::RemoveTypes(ts) => c.card_types.retain(|t| !ts.contains(t)),
        ContinuousEffect::SwitchPT => {
            if let (Some(p), Some(t)) = (c.power, c.toughness) {
                c.power = Some(t);
                c.toughness = Some(p);
            }
        }
    }
}

fn compute(st: &GameState) -> View {
    let mut view: View = st
        .zones
        .battlefield
        .iter()
        .map(|&id| {
            let o = st.object(id);
            (
                id,
                Characteristics::printed(o.card, st.card_def(id), o.controller),
            )
        })
        .collect();

    // Every effect that could take part: statics of permanents (existence is checked per
    // layer against the view as computed so far) and resolved effects.
    let mut effs: Vec<Eff<'_>> = Vec::new();
    for &id in &st.zones.battlefield {
        let o = st.object(id);
        for (i, a) in st.card_def(id).abilities.iter().enumerate() {
            if let AbilityDef::Static {
                affected,
                effects,
                only_if,
            } = a
            {
                effs.push(Eff {
                    cond: only_if.as_ref(),
                    key: Key::Static {
                        source: id,
                        index: i as u16,
                    },
                    // CR 613.7a: a static ability's effect has its object's timestamp.
                    timestamp: o.timestamp,
                    source: id,
                    controller: o.controller,
                    parts: effects,
                    affected: Aff::Dynamic(affected),
                    card: o.card,
                    held: Some(AbilityId::printed(o.card, i as u16)),
                });
            }
        }
    }
    // CR 113.6b: static abilities of cards in graveyards that work from there ("as long as
    // this card is in your graveyard, …"). Off the battlefield a card has only the
    // abilities printed on it; "you" is its owner.
    for p in 0..st.num_players() {
        for &id in st.zones.list(Zone::Graveyard, PlayerId(p as u8)) {
            let o = st.object(id);
            for (i, a) in st.card_def(id).abilities.iter().enumerate() {
                if let AbilityDef::FromZone {
                    zone: card_model::ActivationZone::Graveyard,
                    ability,
                } = a
                    && let AbilityDef::Static {
                        affected,
                        effects,
                        only_if,
                    } = &**ability
                {
                    effs.push(Eff {
                        cond: only_if.as_ref(),
                        key: Key::FromZone {
                            source: id,
                            index: i as u16,
                        },
                        timestamp: o.timestamp,
                        source: id,
                        controller: o.owner,
                        parts: effects,
                        affected: Aff::Dynamic(affected),
                        card: o.card,
                        held: None,
                    });
                }
            }
        }
    }
    for ci in &st.continuous {
        effs.push(resolved(ci));
    }

    let mut walk = Walk {
        st,
        started: BTreeMap::new(),
        lost: Vec::new(),
    };
    let mut granted = 0;
    for layer in LAYERS {
        let mut pending: Vec<usize> = (0..effs.len())
            .filter(|&i| effs[i].parts.iter().any(|p| layer_of(p) == layer))
            .collect();
        loop {
            let live: Vec<usize> = pending
                .iter()
                .copied()
                .filter(|&i| walk.exists(&effs[i], &view))
                .collect();
            if live.is_empty() {
                break;
            }
            let free: Vec<usize> = if layer == Layer::L7c {
                // Additions commute: dependency can't change the result (see module docs).
                live.clone()
            } else {
                live.iter()
                    .copied()
                    .filter(|&a| {
                        // Most effects can't depend on anything (see `can_depend`): only
                        // the others are compared with every live effect.
                        !walk.can_depend(&effs[a])
                            || !live
                                .iter()
                                .any(|&b| b != a && walk.depends(&effs[a], &effs[b], layer, &view))
                    })
                    .collect()
            };
            // CR 613.8b: a dependency loop falls back to timestamp order.
            let candidates = if free.is_empty() { &live } else { &free };
            let pick = *candidates
                .iter()
                .min_by_key(|&&i| (effs[i].timestamp, effs[i].key))
                .expect("non-empty");
            let set = walk.affected(&effs[pick], &view);
            walk.apply(&effs[pick], layer, &set, &mut view);
            pending.retain(|&i| i != pick);
            if layer == Layer::L6 {
                let here = |p: &&ContinuousEffect| layer_of(p) == layer;
                // Abilities that were given and are lost before they applied.
                if effs[pick]
                    .parts
                    .iter()
                    .filter(here)
                    .any(|p| matches!(p, ContinuousEffect::LoseAllAbilities))
                {
                    for e in &effs {
                        if matches!(e.key, Key::Granted(_))
                            && set.contains(&e.source)
                            && !walk.started.contains_key(&e.key)
                            && !walk.lost.contains(&e.key)
                        {
                            walk.lost.push(e.key);
                        }
                    }
                }
                // A static ability that was just given starts to work (CR 113.2c). Its
                // parts in earlier layers never apply: the walk is past them.
                let given: Vec<AbilityId> = effs[pick]
                    .parts
                    .iter()
                    .filter(here)
                    .filter_map(|p| match p {
                        ContinuousEffect::GrantAbilities(indices) => Some(indices),
                        _ => None,
                    })
                    .flatten()
                    .map(|&index| AbilityId {
                        card: effs[pick].card,
                        index,
                        granted: true,
                    })
                    .collect();
                for &ability in &given {
                    let AbilityDef::Static {
                        affected,
                        effects,
                        only_if,
                    } = st.catalog.ability(ability)
                    else {
                        continue;
                    };
                    for &source in set.iter().filter(|o| view.contains_key(o)) {
                        let o = st.object(source);
                        effs.push(Eff {
                            key: Key::Granted(granted),
                            // CR 613.7a: the later of its object's timestamp and that of
                            // the effect that gave the ability.
                            timestamp: o.timestamp.max(effs[pick].timestamp),
                            source,
                            controller: o.controller,
                            parts: effects,
                            affected: Aff::Dynamic(affected),
                            cond: only_if.as_ref(),
                            card: ability.card,
                            held: Some(ability),
                        });
                        granted += 1;
                        if effects.iter().any(|p| layer_of(p) == layer) {
                            pending.push(effs.len() - 1);
                        }
                    }
                }
            }
            if effs[pick].held.is_some() || matches!(effs[pick].key, Key::FromZone { .. }) {
                walk.started.entry(effs[pick].key).or_insert(set);
            }
        }
        if layer == Layer::L7c {
            // CR 613.4c / 122.1a: +1/+1 and -1/-1 counters.
            for (id, c) in view.iter_mut() {
                let o = st.object(*id);
                let n = o.counters_of(CounterKind::PlusOne) as i32
                    - o.counters_of(CounterKind::MinusOne) as i32;
                if let (Some(p), Some(t)) = (c.power.as_mut(), c.toughness.as_mut()) {
                    *p += n;
                    *t += n;
                }
            }
        }
    }
    // CR 208.3: "A noncreature permanent has no power or toughness, even if it's a card
    // with a power and toughness printed on it" (a God that isn't a creature).
    for c in view.values_mut() {
        if !c.is(CardType::Creature) {
            c.power = None;
            c.toughness = None;
        }
    }
    view
}

fn resolved(ci: &ContinuousInstance) -> Eff<'_> {
    Eff {
        key: Key::Resolved(ci.id),
        timestamp: ci.timestamp,
        source: ci.source,
        controller: ci.controller,
        parts: &ci.effects,
        affected: Aff::Locked(&ci.affected),
        cond: None,
        card: ci.card,
        held: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CR 205.3i: basic land types are subtypes, so a selector for "Mountains" matches a
    /// land with the type Mountain (and a creature type is still found the usual way).
    #[test]
    fn selectors_treat_basic_land_types_as_subtypes() {
        let you = PlayerId(0);
        let land = Characteristics::printed(
            CardDefId(0),
            &CardDef {
                name: "Test Peak".into(),
                card_types: alloc::vec![CardType::Land],
                land_types: alloc::vec![BasicLandType::Mountain, BasicLandType::Forest],
                ..CardDef::default()
            },
            you,
        );
        assert!(land.has_subtype("Mountain") && land.has_subtype("Forest"));
        assert!(!land.has_subtype("Island") && !land.has_subtype("Elf"));
        let facts = Facts {
            id: ObjectId(1),
            controller: you,
            owner: you,
            is_token: false,
            tapped: false,
            attacking: false,
            blocking: false,
        };
        let mountains = Selector {
            subtypes: alloc::vec!["Mountain".into()],
            controller: card_model::Who::You,
            ..Selector::default()
        };
        assert!(matches_parts(&mountains, &land, facts, ObjectId(9), you));
        let non_mountain = Selector {
            not_subtypes: alloc::vec!["Mountain".into()],
            ..Selector::default()
        };
        assert!(!matches_parts(
            &non_mountain,
            &land,
            facts,
            ObjectId(9),
            you
        ));
        let elf = Characteristics::printed(
            CardDefId(1),
            &CardDef {
                name: "Test Elf".into(),
                card_types: alloc::vec![CardType::Creature],
                creature_types: alloc::vec!["Elf".into()],
                ..CardDef::default()
            },
            you,
        );
        assert!(elf.has_subtype("Elf") && !elf.has_subtype("Mountain"));
        assert!(!matches_parts(&mountains, &elf, facts, ObjectId(9), you));
    }
}
