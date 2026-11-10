//! Casting spells (CR 601.2) and activating non-mana abilities (CR 602.2), as one resumable
//! frame. Text: docs/rules-notes/601-casting-602-activating-605-mana.md.
//!
//! **No rewinds (D-019).** CR 601.2/602.2 say an uncompletable cast "returns to the moment
//! before". apnap never gets there: `can_begin` only offers a cast/activation if a complete
//! legal sequence exists, and every later step only offers choices that keep completion
//! possible (legal modes, payable X, targets that exist, mana abilities that keep the cost
//! payable). So any shape-valid response leads to a completed cast.

use super::{ApplyFrame, Flow, Frame, Input};
use crate::actions::{GameAction, execute, move_object};
use crate::decision::{ChoiceKind, OptionDesc, Primitive, Response};
use crate::events::Event;
use crate::ids::{ObjectId, PlayerId};
use crate::layers::{abilities, chars, controller_of};
use crate::mana::{ManaMask, ManaPool, ManaRequirement, ManaType, mask_of, types_in};
use crate::replacement::{MoveCause, Proposed};
use crate::stack::Target;
use crate::state::GameState;
use crate::targeting::candidates;
use crate::triggers::{self, PendingTrigger};
use crate::zones::{LibraryPosition, Zone};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use card_model::{AbilityDef, Body, CardType, Cost, Keyword, ManaCost, Timing};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastSource {
    /// Cast this card (from hand in M1).
    Card(ObjectId),
    /// Activate printed ability `index` of this permanent.
    Ability { source: ObjectId, index: u16 },
    /// Put a triggered ability on the stack (CR 603.3): modes and targets as in 601.2b–c,
    /// no costs; with no legal choice it's removed instead (603.3c–d).
    Trigger(Box<PendingTrigger>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastStage {
    Announce,
    Modes,
    X,
    Targets,
    /// CR 601.2h: choose what to sacrifice / discard for the cost (paid with the mana).
    ChooseSacrifice,
    ChooseDiscard,
    Pay,
    PayGeneric,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastFrame {
    pub player: PlayerId,
    pub source: CastSource,
    /// The stack object once announced (CR 601.2a / 602.2a).
    pub object: Option<ObjectId>,
    pub stage: CastStage,
    pub modes: Vec<u8>,
    pub x: u32,
    pub targets: Vec<Vec<Target>>,
    /// CR 903.8: commander tax (generic mana), locked in when the cast is announced.
    pub tax: u32,
    /// CR 601.2f: generic mana taken off by cost reductions, locked in with the tax.
    #[serde(default)]
    pub discount: u32,
    /// Cast from the command zone (CR 903.8: counts toward future tax).
    pub from_command: Option<crate::ids::CardInstanceId>,
    /// The permanents chosen to be sacrificed, and the cards chosen to be discarded, as
    /// part of the cost.
    #[serde(default)]
    pub pay_sacrifice: Vec<ObjectId>,
    #[serde(default)]
    pub pay_discard: Vec<ObjectId>,
}

/// CR 903.8: the commander tax `p` would pay to cast `card` from the command zone now.
pub(crate) fn commander_tax(st: &GameState, p: PlayerId, card: ObjectId) -> u32 {
    let o = st.object(card);
    if o.zone == Zone::Command {
        2 * st
            .player(p)
            .commander_casts
            .get(&o.card_instance)
            .copied()
            .unwrap_or(0)
    } else {
        0
    }
}

/// What is being cast or activated, resolved from the catalog.
struct Proposal {
    body: Option<Body>,
    cost: Option<ManaCost>,
    /// `{T}` in the cost: this permanent.
    tap: Option<ObjectId>,
    /// "Sacrifice this permanent" in the cost.
    sacrifice: Option<ObjectId>,
    /// The parts of the cost that aren't mana or this permanent: chosen sacrifices and
    /// discards, life. For a spell, its additional cost.
    extra: Cost,
}

/// What `p` could sacrifice to pay `choice` for a cost whose source is `source`.
fn sacrifice_candidates(
    st: &GameState,
    p: PlayerId,
    choice: &card_model::CostChoice,
    source: ObjectId,
    not: Option<ObjectId>,
) -> Vec<ObjectId> {
    st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| Some(id) != not)
        .filter(|&id| {
            let c = chars(st, id);
            // CR 701.21a: only a permanent they control.
            c.controller == p
                && choice
                    .what
                    .iter()
                    .any(|s| crate::layers::matches(st, s, &c, st.object(id), source, p))
        })
        .collect()
}

/// What `p` could discard to pay `choice` (never the card being cast: `not`).
fn discard_candidates(
    st: &GameState,
    p: PlayerId,
    choice: &card_model::CostChoice,
    not: Option<ObjectId>,
) -> Vec<ObjectId> {
    st.zones.hands[p.index()]
        .iter()
        .copied()
        .filter(|&id| Some(id) != not)
        .filter(|&id| {
            choice
                .what
                .iter()
                .any(|s| crate::targeting::card_matches(st, id, s))
        })
        .collect()
}

/// The object a cost's "another" is relative to, and that pays `{T}`/"sacrifice this".
fn cost_source(source: &CastSource) -> ObjectId {
    match source {
        CastSource::Card(card) => *card,
        CastSource::Ability { source, .. } => *source,
        CastSource::Trigger(t) => t.source,
    }
}

/// CR 601.2h / 119.4: can `p` pay the non-mana parts of the cost at all?
fn extra_payable(st: &GameState, p: PlayerId, source: &CastSource, prop: &Proposal) -> bool {
    let this = cost_source(source);
    let card = match source {
        CastSource::Card(card) => Some(*card),
        _ => None,
    };
    prop.extra.sacrifice.as_ref().is_none_or(|ch| {
        sacrifice_candidates(st, p, ch, this, prop.sacrifice).len() >= ch.count as usize
    }) && prop
        .extra
        .discard
        .as_ref()
        .is_none_or(|ch| discard_candidates(st, p, ch, card).len() >= ch.count as usize)
        && st.player(p).life >= prop.extra.life as i32
}

fn proposal(st: &GameState, source: &CastSource) -> Proposal {
    match source {
        CastSource::Card(card) => Proposal {
            // CR 303.4a: an Aura spell targets what its enchant ability allows.
            body: st.card_def(*card).spell_ability().cloned().or_else(|| {
                st.card_def(*card).enchant().map(|f| {
                    Body::Effect(card_model::Mode {
                        targets: vec![f],
                        effect: card_model::Effect::Seq(Vec::new()),
                    })
                })
            }),
            cost: chars(st, *card).mana_cost,
            tap: None,
            sacrifice: None,
            // "As an additional cost to cast this spell, …"
            extra: st
                .card_def(*card)
                .abilities
                .iter()
                .find_map(|a| match a {
                    AbilityDef::AdditionalCost(c) => Some(c.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
        },
        CastSource::Ability { source, index } => {
            match &st.card_def(*source).abilities[*index as usize] {
                AbilityDef::Activated { cost, body, .. } => Proposal {
                    body: Some(body.clone()),
                    cost: cost.mana.clone(),
                    tap: cost.tap.then_some(*source),
                    sacrifice: cost.sacrifice_self.then_some(*source),
                    extra: cost.clone(),
                },
                other => unreachable!("not an activated ability: {other:?}"),
            }
        }
        CastSource::Trigger(t) => {
            match &st.catalog.get(t.source_card).abilities[t.index as usize] {
                AbilityDef::Triggered { body, .. } => Proposal {
                    body: Some(body.clone()),
                    cost: None,
                    tap: None,
                    sacrifice: None,
                    extra: Cost::default(),
                },
                other => unreachable!("not a triggered ability: {other:?}"),
            }
        }
    }
}

fn requirement(cost: &Option<ManaCost>, x: u32) -> ManaRequirement {
    cost.as_ref()
        .map(|c| ManaRequirement::from_cost(c, x))
        .unwrap_or_default()
}

/// CR 601.2f: the total cost — the mana cost plus additional costs such as commander tax,
/// minus cost reductions. CR 118.7a: a reduction by generic mana only touches the generic
/// part, which can't go below nothing.
fn total(cost: &Option<ManaCost>, x: u32, tax: u32, discount: u32) -> ManaRequirement {
    let mut r = requirement(cost, x);
    r.generic = (r.generic + tax).saturating_sub(discount);
    r
}

/// CR 601.2f: the generic mana cost reductions take off `card` as `p` casts it — from
/// permanents `p` controls ("Dragon spells you cast cost {1} less") and from the spell
/// itself ("this spell costs {1} less for each creature on the battlefield").
pub(crate) fn cost_reduction(st: &GameState, p: PlayerId, card: ObjectId) -> u32 {
    let c = chars(st, card);
    let facts = crate::layers::Facts {
        id: card,
        controller: p,
        is_token: false,
        tapped: false,
        attacking: false,
        blocking: false,
    };
    let mut total = 0;
    for &id in &st.zones.battlefield {
        if controller_of(st, id) != p {
            continue;
        }
        // CLAUDE.md invariant 4: only abilities the permanent currently has.
        for (_, a) in abilities(st, id) {
            if let AbilityDef::CostReduction { spells, amount } = a
                && crate::layers::matches_parts(spells, &c, facts, id, p)
            {
                total += amount;
            }
        }
    }
    for a in &st.card_def(card).abilities {
        if let AbilityDef::SelfCostReduction(amount) = a {
            total += crate::layers::count_amount(st, amount, card, p);
        }
    }
    total
}

/// CR 302.6 / 602.5a: can this permanent's `{T}` abilities be activated now?
pub(crate) fn can_tap_for_ability(st: &GameState, id: ObjectId) -> bool {
    let o = st.object(id);
    let c = chars(st, id);
    !o.tapped && (!c.is(CardType::Creature) || !o.summoning_sick || c.has(Keyword::Haste))
}

/// Untapped mana sources `p` controls (battlefield order) and the mana types each can make.
/// Every M1 mana ability costs exactly `{T}` (D-021), so one source = one mana.
/// A permanent that can be tapped for mana now: one activation adds `count` mana, all of
/// one type chosen from `mask` (CR 605). `count` is 1 except for "Add {C}{C}", "add three
/// mana of any one color" (card-db: such an ability is the permanent's only mana ability).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ManaSource {
    pub id: ObjectId,
    pub mask: ManaMask,
    pub count: u32,
}

pub(crate) fn mana_sources(
    st: &GameState,
    p: PlayerId,
    exclude: Option<ObjectId>,
) -> Vec<ManaSource> {
    st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| Some(id) != exclude && controller_of(st, id) == p)
        .filter_map(|id| {
            let c = chars(st, id);
            let mut mask: ManaMask = 0;
            let mut count = 1;
            if c.is(CardType::Land) {
                // CR 305.6: each basic land type grants "{T}: Add [mana]".
                for t in &c.land_types {
                    mask |= 1 << ManaType::from_color(t.produces()).index();
                }
            }
            for (_, a) in abilities(st, id) {
                if let AbilityDef::Mana { produce, .. } = a {
                    mask |= mask_of(&produce.options());
                    count = count.max(produce.count());
                }
            }
            (mask != 0 && can_tap_for_ability(st, id)).then_some(ManaSource { id, mask, count })
        })
        .collect()
}

/// Can `req` be paid from `pool` plus `sources` (CR 601.2g–h)? Exact: a source that makes
/// several mana of one *chosen* type is tried with each choice. (Mana beyond the cost is
/// allowed: it stays in the pool.)
pub(crate) fn payable(req: &ManaRequirement, pool: &ManaPool, sources: &[ManaSource]) -> bool {
    // How many multi-mana sources with a real choice are tried exhaustively (5 types each).
    const BRANCHING: usize = 4;
    fn go(
        req: &ManaRequirement,
        pool: &ManaPool,
        sources: &[ManaSource],
        masks: &mut Vec<ManaMask>,
        branches: usize,
    ) -> bool {
        let Some((first, rest)) = sources.split_first() else {
            return req.feasible(pool, masks);
        };
        let single = first.mask.count_ones() == 1;
        if first.count == 1 || single || branches == 0 {
            // One unit per mana when the type is fixed; a choice beyond the branching
            // budget counts as a single mana (never more than it can really give).
            let n = if first.count == 1 || single {
                first.count
            } else {
                1
            };
            for _ in 0..n {
                masks.push(first.mask);
            }
            let ok = go(req, pool, rest, masks, branches);
            masks.truncate(masks.len() - n as usize);
            return ok;
        }
        for t in types_in(first.mask) {
            for _ in 0..first.count {
                masks.push(1 << t.index());
            }
            let ok = go(req, pool, rest, masks, branches - 1);
            masks.truncate(masks.len() - first.count as usize);
            if ok {
                return true;
            }
        }
        false
    }
    go(req, pool, sources, &mut Vec::new(), BRANCHING)
}

fn feasible(st: &GameState, p: PlayerId, req: &ManaRequirement, exclude: Option<ObjectId>) -> bool {
    payable(req, &st.player(p).mana_pool, &mana_sources(st, p, exclude))
}

/// Mode indices whose every target slot has at least one legal choice (CR 700.2a).
fn legal_modes(st: &GameState, body: &Body, p: PlayerId, this: Option<ObjectId>) -> Vec<u8> {
    body.modes()
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.targets
                .iter()
                .all(|f| !candidates(st, f, p, this).is_empty())
        })
        .map(|(i, _)| i as u8)
        .collect()
}

fn body_possible(st: &GameState, body: &Body, p: PlayerId) -> bool {
    let n = legal_modes(st, body, p, None).len();
    match body {
        Body::Effect(_) => n == 1,
        Body::Modal { min, .. } => n >= *min as usize,
    }
}

/// CR 117.1a / 307.1: sorcery timing.
pub(crate) fn sorcery_timing(st: &GameState, p: PlayerId) -> bool {
    st.turn.active == p && st.turn.step.is_main_phase() && st.stack.is_empty()
}

/// Whether `p` may begin casting/activating `source` now and could complete it (D-019).
pub(crate) fn can_begin(st: &GameState, p: PlayerId, source: CastSource) -> bool {
    match &source {
        CastSource::Card(card) => {
            let o = st.object(*card);
            // From your hand, or your commander from the command zone (CR 903.8).
            let from_ok = o.owner == p
                && (o.zone == Zone::Hand || (o.zone == Zone::Command && st.is_commander(*card)));
            let c = chars(st, *card);
            if !from_ok || c.is(CardType::Land) {
                return false;
            }
            // CR 117.1a, 702.8a
            let instant_speed = c.is(CardType::Instant) || c.has(Keyword::Flash);
            if !instant_speed && !sorcery_timing(st, p) {
                return false;
            }
        }
        CastSource::Trigger(_) => return false,
        CastSource::Ability { source, index } => {
            let (source, index) = (*source, *index);
            let o = st.object(source);
            if o.zone != Zone::Battlefield || controller_of(st, source) != p {
                return false;
            }
            // CLAUDE.md invariant 4: only abilities the permanent currently has.
            let Some((
                _,
                AbilityDef::Activated {
                    cost,
                    timing,
                    only_if,
                    ..
                },
            )) = abilities(st, source).into_iter().find(|(i, _)| *i == index)
            else {
                return false;
            };
            // "Activate only if …" (CR 602.5b).
            if !triggers::condition_holds(st, only_if.as_ref(), p, source) {
                return false;
            }
            if *timing == Timing::Sorcery && !sorcery_timing(st, p) {
                return false;
            }
            if cost.tap && !can_tap_for_ability(st, source) {
                return false;
            }
        }
    }
    let prop = proposal(st, &source);
    if let Some(b) = &prop.body
        && !body_possible(st, b, p)
    {
        return false;
    }
    if !extra_payable(st, p, &source, &prop) {
        return false;
    }
    let (tax, discount) = match &source {
        CastSource::Card(card) => (commander_tax(st, p, *card), cost_reduction(st, p, *card)),
        _ => (0, 0),
    };
    feasible(st, p, &total(&prop.cost, 0, tax, discount), prop.tap)
}

impl CastFrame {
    pub(crate) fn new(player: PlayerId, source: CastSource) -> Self {
        CastFrame {
            player,
            source,
            object: None,
            stage: CastStage::Announce,
            modes: Vec::new(),
            x: 0,
            targets: Vec::new(),
            tax: 0,
            discount: 0,
            from_command: None,
            pay_sacrifice: Vec::new(),
            pay_discard: Vec::new(),
        }
    }

    pub(crate) fn trigger(t: PendingTrigger) -> Self {
        CastFrame::new(t.controller, CastSource::Trigger(Box::new(t)))
    }

    /// The proposal's body/cost, read through the *current* source object.
    fn proposal(&self, st: &GameState) -> Proposal {
        match &self.source {
            CastSource::Card(_) => proposal(st, &CastSource::Card(self.object.expect("announced"))),
            s => proposal(st, s),
        }
    }

    fn is_trigger(&self) -> bool {
        matches!(self.source, CastSource::Trigger(_))
    }

    /// CR 603.3c–d: a triggered ability with no legal mode/target is removed from the stack.
    fn remove_trigger(self, st: &mut GameState) -> Flow {
        execute(
            st,
            GameAction::RemoveAbility {
                ability: self.object.expect("announced"),
            },
        );
        Flow::done()
    }

    fn ask(self, kind: ChoiceKind, prim: Primitive, options: Vec<OptionDesc>) -> Flow {
        let p = self.player;
        Flow::ask(Frame::Cast(self), p, kind, prim, options)
    }

    pub(crate) fn run(mut self, st: &mut GameState, mut input: Option<Input>) -> Flow {
        let p = self.player;
        loop {
            match self.stage {
                CastStage::Announce => {
                    if let CastSource::Card(card) = &self.source {
                        // CR 903.8 / 601.2f: commander tax, determined before the card moves.
                        self.tax = commander_tax(st, p, *card);
                        self.discount = cost_reduction(st, p, *card);
                        if st.object(*card).zone == Zone::Command {
                            self.from_command = Some(st.object(*card).card_instance);
                        }
                    }
                    let put = |st: &mut GameState, a: GameAction| match execute(st, a) {
                        crate::actions::ActionResult::Moved(id) => id,
                        r => unreachable!("{r:?}"),
                    };
                    self.object = Some(match &self.source {
                        // CR 601.2a: move the card to the stack; the caster controls it.
                        CastSource::Card(card) => {
                            move_object(st, *card, Zone::Stack, LibraryPosition::Top, Some(p))
                        }
                        // CR 602.2a: the ability is created on the stack.
                        CastSource::Ability { source, index } => {
                            let source_card = st.object(*source).card;
                            put(
                                st,
                                GameAction::PutAbilityOnStack {
                                    controller: p,
                                    source: *source,
                                    source_card,
                                    index: *index,
                                    triggered: false,
                                    source_lki: None,
                                    subject: None,
                                    subject_lki: None,
                                },
                            )
                        }
                        // CR 603.3: the triggered ability becomes the topmost object.
                        CastSource::Trigger(t) => put(
                            st,
                            GameAction::PutAbilityOnStack {
                                controller: t.controller,
                                source: t.source,
                                source_card: t.source_card,
                                index: t.index,
                                triggered: true,
                                source_lki: t.source_lki.clone(),
                                subject: t.subject,
                                subject_lki: t.subject_lki.clone(),
                            },
                        ),
                    });
                    self.stage = CastStage::Modes;
                }
                CastStage::Modes => {
                    // CR 601.2b / 700.2a: announce modes; only legal modes are offered.
                    match self.proposal(st).body {
                        None => self.modes = Vec::new(),
                        Some(Body::Effect(_)) => self.modes = vec![0],
                        Some(body @ Body::Modal { min, max, .. }) => {
                            if let Some(inp) = input.take() {
                                let mut modes: Vec<u8> = inp
                                    .picked()
                                    .into_iter()
                                    .map(|o| match o {
                                        OptionDesc::Mode { index } => *index,
                                        _ => unreachable!(),
                                    })
                                    .collect();
                                modes.sort_unstable();
                                st.emit(Event::ModesChosen {
                                    object: self.object.unwrap(),
                                    modes: modes.clone(),
                                });
                                self.modes = modes;
                            } else {
                                let legal = legal_modes(st, &body, p, self.object);
                                if legal.len() < min as usize {
                                    // CR 603.3c (only triggers can get here, D-019).
                                    debug_assert!(self.is_trigger());
                                    return self.remove_trigger(st);
                                }
                                let max = (max as usize).min(legal.len());
                                let options = legal
                                    .into_iter()
                                    .map(|index| OptionDesc::Mode { index })
                                    .collect();
                                return self.ask(
                                    ChoiceKind::ChooseModes,
                                    Primitive::PickN {
                                        min: min as usize,
                                        max,
                                    },
                                    options,
                                );
                            }
                        }
                    }
                    self.targets = vec![Vec::new(); self.modes.len()];
                    self.stage = CastStage::X;
                }
                CastStage::X => {
                    // CR 107.3a / 601.2b: announce X; bounded by what can be paid.
                    let prop = self.proposal(st);
                    if prop.cost.as_ref().is_some_and(|c| c.has_x()) {
                        if let Some(inp) = input.take() {
                            let Response::Number(x) = inp.response else {
                                unreachable!()
                            };
                            self.x = x;
                            st.emit(Event::XChosen {
                                object: self.object.unwrap(),
                                x,
                            });
                        } else {
                            let mut max = 0;
                            while feasible(
                                st,
                                p,
                                &total(&prop.cost, max + 1, self.tax, self.discount),
                                prop.tap,
                            ) {
                                max += 1;
                            }
                            return self.ask(
                                ChoiceKind::ChooseX,
                                Primitive::PickNumber { min: 0, max },
                                Vec::new(),
                            );
                        }
                    }
                    self.stage = CastStage::Targets;
                }
                CastStage::Targets => {
                    // CR 601.2c: one target per target slot of each chosen mode, in order.
                    let body = self.proposal(st).body;
                    let slots: Vec<(usize, card_model::TargetFilter)> = self
                        .modes
                        .iter()
                        .enumerate()
                        .flat_map(|(k, &m)| {
                            body.as_ref().unwrap().modes()[m as usize]
                                .targets
                                .iter()
                                .map(move |f| (k, f.clone()))
                        })
                        .collect();
                    let filled: usize = self.targets.iter().map(Vec::len).sum();
                    if let Some(inp) = input.take() {
                        let Some(OptionDesc::Target { target }) = inp.picked_one().cloned() else {
                            unreachable!()
                        };
                        self.targets[slots[filled].0].push(target);
                        continue;
                    }
                    if let Some((_, filter)) = slots.get(filled) {
                        let options: Vec<OptionDesc> = candidates(st, filter, p, self.object)
                            .into_iter()
                            .map(|target| OptionDesc::Target { target })
                            .collect();
                        if options.is_empty() {
                            // CR 603.3d (only triggers can get here, D-019).
                            debug_assert!(self.is_trigger());
                            return self.remove_trigger(st);
                        }
                        return self.ask(
                            ChoiceKind::ChooseTargets { slot: filled as u8 },
                            Primitive::PickN { min: 1, max: 1 },
                            options,
                        );
                    }
                    let object = self.object.unwrap();
                    if !slots.is_empty() {
                        st.emit(Event::TargetsChosen {
                            object,
                            targets: self.targets.iter().flatten().copied().collect(),
                        });
                    }
                    execute(
                        st,
                        GameAction::SetStackChoices {
                            object,
                            modes: self.modes.clone(),
                            x: self.x,
                            targets: self.targets.clone(),
                        },
                    );
                    // CR 601.2e: legality is guaranteed by construction (D-019).
                    self.stage = CastStage::ChooseSacrifice;
                }
                // CR 601.2h: which permanents / cards pay the cost. They are chosen here
                // and leave with the rest of the payment (`finish`), so a land chosen to be
                // sacrificed can still be tapped for mana first.
                CastStage::ChooseSacrifice | CastStage::ChooseDiscard => {
                    let sacrificing = self.stage == CastStage::ChooseSacrifice;
                    if let Some(inp) = input.take() {
                        let picked: Vec<ObjectId> = inp
                            .picked()
                            .into_iter()
                            .map(|o| match o {
                                OptionDesc::Card { card } => *card,
                                other => unreachable!("{other:?}"),
                            })
                            .collect();
                        if sacrificing {
                            self.pay_sacrifice = picked;
                        } else {
                            self.pay_discard = picked;
                        }
                    } else {
                        let prop = self.proposal(st);
                        let this = cost_source(&self.source);
                        let (choice, pool, kind) = if sacrificing {
                            let pool =
                                prop.extra.sacrifice.as_ref().map(|ch| {
                                    sacrifice_candidates(st, p, ch, this, prop.sacrifice)
                                });
                            (prop.extra.sacrifice.clone(), pool, ChoiceKind::Sacrifice)
                        } else {
                            // The card being cast is on the stack already, not in hand.
                            let pool = prop
                                .extra
                                .discard
                                .as_ref()
                                .map(|ch| discard_candidates(st, p, ch, None));
                            (prop.extra.discard.clone(), pool, ChoiceKind::Discard)
                        };
                        if let (Some(choice), Some(pool)) = (choice, pool) {
                            let n = choice.count as usize;
                            assert!(pool.len() >= n, "cost not payable: D-019 violated");
                            let options = pool
                                .into_iter()
                                .map(|card| OptionDesc::Card { card })
                                .collect();
                            return self.ask(kind, Primitive::PickN { min: n, max: n }, options);
                        }
                    }
                    self.stage = if sacrificing {
                        CastStage::ChooseDiscard
                    } else {
                        CastStage::Pay
                    };
                }
                CastStage::Pay => {
                    // CR 601.2f: total cost, locked in. CR 601.2g: mana abilities.
                    let prop = self.proposal(st);
                    let req = total(&prop.cost, self.x, self.tax, self.discount);
                    if let Some(inp) = input.take() {
                        let Some(OptionDesc::TapForMana { source, mana }) =
                            inp.picked_one().cloned()
                        else {
                            unreachable!()
                        };
                        tap_for_mana(st, p, source, mana);
                    }
                    let pool = st.player(p).mana_pool;
                    if req.payable_from(&pool) {
                        match req.forced_generic(&pool) {
                            Some(alloc) => {
                                return self.finish(
                                    st,
                                    req,
                                    alloc,
                                    prop.tap,
                                    prop.sacrifice,
                                    prop.extra.life,
                                );
                            }
                            None => {
                                // CR 601.2h: the player picks which floating mana pays generic.
                                self.stage = CastStage::PayGeneric;
                                let left = req.generic_candidates(&pool);
                                let types: Vec<ManaType> = ManaType::ALL
                                    .into_iter()
                                    .filter(|t| left[t.index()] > 0)
                                    .collect();
                                let options = types
                                    .iter()
                                    .map(|&mana| OptionDesc::Mana { mana })
                                    .collect();
                                return self.ask(
                                    ChoiceKind::PayMana,
                                    Primitive::Allocate {
                                        total: req.generic,
                                        per_min: vec![0; types.len()],
                                        per_max: types.iter().map(|t| left[t.index()]).collect(),
                                    },
                                    options,
                                );
                            }
                        }
                    }
                    let sources = mana_sources(st, p, prop.tap);
                    let mut options = Vec::new();
                    for (i, src) in sources.iter().enumerate() {
                        let mut rest = sources.clone();
                        rest.remove(i);
                        for mana in types_in(src.mask) {
                            let mut pl = pool;
                            pl.add(mana, src.count);
                            if payable(&req, &pl, &rest) {
                                options.push(OptionDesc::TapForMana {
                                    source: src.id,
                                    mana,
                                });
                            }
                        }
                    }
                    assert!(!options.is_empty(), "payment dead-ended: D-019 violated");
                    return self.ask(
                        ChoiceKind::PayMana,
                        Primitive::PickN { min: 1, max: 1 },
                        options,
                    );
                }
                CastStage::PayGeneric => {
                    let inp = input.take().expect("generic allocation");
                    let prop = self.proposal(st);
                    let req = total(&prop.cost, self.x, self.tax, self.discount);
                    let mut alloc = [0u32; 6];
                    for (i, &n) in inp.allocation().iter().enumerate() {
                        let OptionDesc::Mana { mana } = inp.decision.options[i] else {
                            unreachable!()
                        };
                        alloc[mana.index()] = n;
                    }
                    return self.finish(st, req, alloc, prop.tap, prop.sacrifice, prop.extra.life);
                }
            }
        }
    }

    /// CR 601.2h: pay; CR 601.2i: the spell becomes cast / the ability becomes activated.
    fn finish(
        self,
        st: &mut GameState,
        req: ManaRequirement,
        generic: [u32; 6],
        tap: Option<ObjectId>,
        sacrifice: Option<ObjectId>,
        life: u32,
    ) -> Flow {
        let p = self.player;
        if req.total() > 0 {
            execute(
                st,
                GameAction::PayMana {
                    player: p,
                    requirement: req,
                    generic,
                },
            );
        }
        if let Some(source) = tap {
            execute(st, GameAction::Tap { object: source });
        }
        if let Some(card) = self.from_command {
            execute(st, GameAction::CommanderCast { player: p, card });
        }
        if let CastSource::Card(_) = self.source {
            let spell = self.object.unwrap();
            st.emit(Event::SpellCast { player: p, spell });
            // CR 601.2i: "whenever ... casts" abilities trigger now.
            triggers::detect_cast(st, p, spell);
        }
        // The rest of the cost, paid at once (CR 601.2h). Sacrifices (CR 701.21a), discards
        // and life are events like any other: they go through the replacement pipeline
        // and can trigger ("dies", "whenever you discard", "whenever you lose life").
        let on_battlefield = |st: &GameState, id: &ObjectId| {
            st.objects
                .get(id)
                .is_some_and(|o| o.zone == Zone::Battlefield)
        };
        let mut events: Vec<Proposed> = sacrifice
            .iter()
            .chain(self.pay_sacrifice.iter())
            .filter(|id| on_battlefield(st, id))
            .map(|&id| Proposed::moving(id, Zone::Graveyard, MoveCause::Sacrifice))
            .collect();
        events.extend(
            self.pay_discard
                .iter()
                .filter(|id| st.objects.get(id).is_some_and(|o| o.zone == Zone::Hand))
                .map(|&id| Proposed::moving(id, Zone::Graveyard, MoveCause::Discard)),
        );
        if life > 0 {
            // CR 119.4: paying life is losing that much life.
            events.push(Proposed::LoseLife {
                player: p,
                amount: life,
            });
        }
        if events.is_empty() {
            Flow::done()
        } else {
            Flow::then(Frame::Apply(ApplyFrame::new(st, events)))
        }
    }
}

/// CR 605.3b: activate a `{T}` mana ability; it resolves immediately, adding as many mana
/// of the chosen type as the ability makes.
pub(crate) fn tap_for_mana(st: &mut GameState, p: PlayerId, source: ObjectId, mana: ManaType) {
    let count = mana_sources(st, p, None)
        .iter()
        .find(|s| s.id == source)
        .map_or(1, |s| s.count);
    execute(st, GameAction::Tap { object: source });
    for _ in 0..count {
        execute(st, GameAction::AddMana { player: p, mana });
    }
}
