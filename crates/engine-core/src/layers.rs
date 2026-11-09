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
    AbilityDef, Affected, BasicLandType, CardDef, CardType, Color, ContinuousEffect, CounterKind,
    Keyword, ManaCost, Selector, Supertype, Who,
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
    /// Indices of the printed abilities the object still has (layer 6 can remove them).
    pub abilities: Vec<u16>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}

impl Characteristics {
    pub fn printed(d: &CardDef) -> Self {
        Characteristics {
            name: d.name.clone(),
            mana_cost: d.mana_cost.clone(),
            colors: d.printed_colors(),
            supertypes: d.supertypes.clone(),
            card_types: d.card_types.clone(),
            subtypes: d.creature_types.clone(),
            land_types: d.land_types.clone(),
            keywords: d.keywords.clone(),
            abilities: (0..d.abilities.len() as u16).collect(),
            power: d.power,
            toughness: d.toughness,
        }
    }

    pub fn is(&self, t: CardType) -> bool {
        self.card_types.contains(&t)
    }

    pub fn has(&self, k: Keyword) -> bool {
        self.keywords.contains(&k)
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

/// Memo of the layered battlefield view, keyed by `GameState::generation`.
#[derive(Clone, Default)]
pub struct LayerCache(RefCell<Option<(u64, Arc<View>)>>);

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
    Characteristics::printed(st.card_def(id))
}

/// The printed abilities `id` currently has, with their indices.
pub fn abilities(st: &GameState, id: ObjectId) -> Vec<(u16, &AbilityDef)> {
    let def = st.card_def(id);
    chars(st, id)
        .abilities
        .into_iter()
        .map(|i| (i, &def.abilities[i as usize]))
        .collect()
}

/// The layered view of every permanent on the battlefield.
pub fn view(st: &GameState) -> Arc<View> {
    if let Some((generation, v)) = &*st.layer_cache.0.borrow()
        && *generation == st.generation
    {
        return v.clone();
    }
    let v = Arc::new(compute(st));
    *st.layer_cache.0.borrow_mut() = Some((st.generation, v.clone()));
    v
}

/// Does object `o` (with characteristics `c`) match `sel`, relative to an ability whose
/// source is `source`, controlled by `controller`?
pub fn matches(
    sel: &Selector,
    c: &Characteristics,
    o: &GameObject,
    source: ObjectId,
    controller: PlayerId,
) -> bool {
    matches_parts(sel, c, o.id, o.controller, o.is_token, source, controller)
}

/// [`matches`] for an object that may no longer exist (last known information).
pub fn matches_parts(
    sel: &Selector,
    c: &Characteristics,
    id: ObjectId,
    object_controller: PlayerId,
    is_token: bool,
    source: ObjectId,
    controller: PlayerId,
) -> bool {
    (sel.types.is_empty() || sel.types.iter().any(|t| c.is(*t)))
        && !sel.not_types.iter().any(|t| c.is(*t))
        && sel.supertypes.iter().all(|s| c.supertypes.contains(s))
        && match sel.controller {
            Who::Any => true,
            Who::You => object_controller == controller,
            Who::Opponent => object_controller != controller,
        }
        && !(sel.other && id == source)
        && sel.token.is_none_or(|t| is_token == t)
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
        ContinuousEffect::AddTypes { .. } => Layer::L4,
        ContinuousEffect::SetColors(_) => Layer::L5,
        ContinuousEffect::AddKeywords(_)
        | ContinuousEffect::LoseKeywords(_)
        | ContinuousEffect::LoseAllAbilities => Layer::L6,
        ContinuousEffect::SetPT(..) => Layer::L7b,
        ContinuousEffect::ModifyPT(..) => Layer::L7c,
        ContinuousEffect::SwitchPT => Layer::L7d,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Static { source: ObjectId, index: u16 },
    Resolved(u32),
}

/// One continuous effect taking part in the walk.
struct Eff<'a> {
    key: Key,
    timestamp: Timestamp,
    source: ObjectId,
    controller: PlayerId,
    parts: &'a [ContinuousEffect],
    affected: Aff<'a>,
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
}

impl Walk<'_> {
    fn exists(&self, e: &Eff<'_>, view: &View) -> bool {
        match e.key {
            Key::Resolved(_) => true,
            Key::Static { source, index } => {
                self.started.contains_key(&e.key)
                    || view
                        .get(&source)
                        .is_some_and(|c| c.abilities.contains(&index))
            }
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
            Aff::Dynamic(Affected::All(sel)) => view
                .iter()
                .filter(|(id, c)| matches(sel, c, self.st.object(**id), e.source, e.controller))
                .map(|(id, _)| *id)
                .collect(),
        }
    }

    fn apply(&self, e: &Eff<'_>, layer: Layer, set: &[ObjectId], view: &mut View) {
        for part in e.parts.iter().filter(|p| layer_of(p) == layer) {
            for id in set {
                if let Some(c) = view.get_mut(id) {
                    apply_part(c, part);
                }
            }
        }
    }

    /// CR 613.8a: does `a` depend on `b`? (Same layer is given.) Applying `b` would change
    /// `a`'s existence or what it applies to. What our effects *do* never depends on other
    /// effects, and no CDAs exist yet (613.8a(c)).
    fn depends(&self, a: &Eff<'_>, b: &Eff<'_>, layer: Layer, view: &View) -> bool {
        if a.key == b.key {
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

fn apply_part(c: &mut Characteristics, part: &ContinuousEffect) {
    fn add<T: PartialEq + Clone>(v: &mut Vec<T>, items: &[T]) {
        for i in items {
            if !v.contains(i) {
                v.push(i.clone());
            }
        }
    }
    match part {
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
        .map(|&id| (id, Characteristics::printed(st.card_def(id))))
        .collect();

    // Every effect that could take part: statics of permanents (existence is checked per
    // layer against the view as computed so far) and resolved effects.
    let mut effs: Vec<Eff<'_>> = Vec::new();
    for &id in &st.zones.battlefield {
        let o = st.object(id);
        for (i, a) in st.card_def(id).abilities.iter().enumerate() {
            if let AbilityDef::Static { affected, effects } = a {
                effs.push(Eff {
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
                });
            }
        }
    }
    for ci in &st.continuous {
        effs.push(resolved(ci));
    }

    let mut walk = Walk {
        st,
        started: BTreeMap::new(),
    };
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
                        !live
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
            if let Key::Static { .. } = effs[pick].key {
                walk.started.entry(effs[pick].key).or_insert(set);
            }
            pending.retain(|&i| i != pick);
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
    }
}
