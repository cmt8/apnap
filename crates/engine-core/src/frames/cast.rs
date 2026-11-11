//! Casting spells (CR 601.2) and activating non-mana abilities (CR 602.2), as one resumable
//! frame. Text: docs/rules-notes/601-casting-602-activating-605-mana.md.
//!
//! **No rewinds (D-019).** CR 601.2/602.2 say an uncompletable cast "returns to the moment
//! before". apnap never gets there: `can_begin` only offers a cast/activation if a complete
//! legal sequence exists, and every later step only offers choices that keep completion
//! possible (legal modes, payable X, targets that exist, mana abilities that keep the cost
//! payable). So any shape-valid response leads to a completed cast.

use super::mana::{PayGoal, goal_feasible, pay_options};
use super::{ApplyFrame, Flow, Frame, Input};
use crate::actions::{GameAction, execute, move_object};
use crate::decision::{ChoiceKind, OptionDesc, Primitive, Response};
use crate::events::Event;
use crate::ids::{ObjectId, PlayerId};
use crate::layers::{abilities, chars, controller_of};
use crate::mana::{ManaMask, ManaPool, ManaRequirement, ManaType};
use crate::replacement::{MoveCause, Proposed};
use crate::stack::Target;
use crate::state::GameState;
use crate::targeting::candidates;
use crate::triggers::{self, PendingTrigger};
use crate::zones::{LibraryPosition, Zone};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use card_model::{AbilityDef, AbilityId, Body, CardType, Cost, Keyword, ManaCost, Timing};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastSource {
    /// Cast this card (from hand in M1).
    Card(ObjectId),
    /// Activate ability `ability` of this object.
    Ability {
        source: ObjectId,
        ability: AbilityId,
    },
    /// Put a triggered ability on the stack (CR 603.3): modes and targets as in 601.2b–c,
    /// no costs; with no legal choice it's removed instead (603.3c–d).
    Trigger(Box<PendingTrigger>),
    /// Pay a cost while a spell or ability resolves (CR 118.12): nothing goes on the
    /// stack. `source` is the object the resolving ability is on ("sacrifice another
    /// creature" is relative to it; `{T}` and "sacrifice this" are it).
    Payment { source: ObjectId, cost: Box<Cost> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastStage {
    Announce,
    Modes,
    X,
    Targets,
    /// CR 601.2h: choose what to sacrifice / discard for the cost. This comes after the
    /// mana abilities (CR 601.2g): what was sacrificed for mana can't be chosen any more.
    ChooseSacrifice,
    ChooseDiscard,
    /// CR 601.2g: mana abilities, until the pool covers the mana in the cost.
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
    /// Which mana in the pool pays the generic part of the cost (per mana type).
    #[serde(default)]
    pub generic: [u32; 6],
    /// Per chosen mode: no more targets will be chosen for it ("up to two targets": the
    /// player stopped, or nothing else can be targeted).
    #[serde(default)]
    pub targets_closed: Vec<bool>,
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
pub(crate) fn sacrifice_candidates(
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
        CastSource::Payment { source, .. } => *source,
    }
}

/// The object an ability is on: what its "another target …" is other than. A spell has
/// none (it can't target itself, CR 115.5).
fn ability_source(source: &CastSource) -> Option<ObjectId> {
    match source {
        CastSource::Card(_) => None,
        CastSource::Ability { source, .. } | CastSource::Payment { source, .. } => Some(*source),
        CastSource::Trigger(t) => Some(t.source),
    }
}

/// CR 601.2h / 119.4: can `p` pay the non-mana parts of the cost at all?
fn extra_payable(st: &GameState, p: PlayerId, source: &CastSource, prop: &Proposal) -> bool {
    let this = cost_source(source);
    let card = match source {
        CastSource::Card(card) => Some(*card),
        _ => None,
    };
    // The card that is itself discarded for the cost can't also be the chosen discard.
    let card = card.or(prop.extra.discard_self.then_some(this));
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
                        groups: Vec::new(),
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
        CastSource::Ability { source, ability } => match st.catalog.ability(*ability) {
            AbilityDef::Activated { cost, body, .. } => Proposal {
                body: Some(body.clone()),
                cost: cost.mana.clone(),
                tap: cost.tap.then_some(*source),
                sacrifice: cost.sacrifice_self.then_some(*source),
                extra: cost.clone(),
            },
            other => unreachable!("not an activated ability: {other:?}"),
        },
        // A cost paid on resolution: `{T}` and "sacrifice this" are the ability's source,
        // if the payer still controls it.
        CastSource::Payment { source, cost } => {
            let here = |st: &GameState| {
                st.objects
                    .get(source)
                    .is_some_and(|o| o.zone == Zone::Battlefield)
            };
            Proposal {
                body: None,
                cost: cost.mana.clone(),
                tap: (cost.tap && here(st)).then_some(*source),
                sacrifice: (cost.sacrifice_self && here(st)).then_some(*source),
                extra: (**cost).clone(),
            }
        }
        CastSource::Trigger(t) => match st.catalog.ability(t.ability) {
            AbilityDef::Triggered { body, .. } => Proposal {
                body: Some(body.clone()),
                cost: None,
                tap: None,
                sacrifice: None,
                extra: Cost::default(),
            },
            other => unreachable!("not a triggered ability: {other:?}"),
        },
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
        owner: st.object(card).owner,
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

/// What mana is being spent on (CR 106.6: some mana can only be spent in certain ways).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Payee {
    /// A spell: the card being cast (in hand before it is announced, then on the stack).
    Spell(ObjectId),
    /// An activated ability of this permanent.
    Ability(ObjectId),
    /// A cost paid while something resolves (CR 118.12): neither casting a spell nor
    /// activating an ability, so mana restricted to those can't pay it.
    Cost,
}

fn payee_of(source: &CastSource) -> Option<Payee> {
    match source {
        CastSource::Card(card) => Some(Payee::Spell(*card)),
        CastSource::Ability { source, .. } => Some(Payee::Ability(*source)),
        CastSource::Trigger(_) => None,
        CastSource::Payment { .. } => Some(Payee::Cost),
    }
}

/// CR 106.6: may mana restricted by `r` pay for `payee`, as `p` spends it?
pub(crate) fn may_spend(
    st: &GameState,
    p: PlayerId,
    r: &card_model::ManaRestriction,
    payee: Payee,
) -> bool {
    match payee {
        Payee::Spell(card) => {
            let c = chars(st, card);
            let facts = crate::layers::Facts {
                id: card,
                controller: p,
                owner: st.object(card).owner,
                is_token: false,
                tapped: false,
                attacking: false,
                blocking: false,
            };
            r.spells
                .iter()
                .any(|s| crate::layers::matches_parts(s, &c, facts, card, p))
        }
        Payee::Ability(source) => {
            let c = chars(st, source);
            r.abilities
                .iter()
                .any(|s| crate::layers::matches(st, s, &c, st.object(source), source, p))
        }
        Payee::Cost => false,
    }
}

/// The restriction on a unit of restricted mana in a pool.
fn restriction<'a>(
    st: &'a GameState,
    r: &crate::state::RestrictedMana,
) -> &'a card_model::ManaRestriction {
    match st.catalog.ability(r.ability) {
        AbilityDef::Mana {
            spend: Some(spend), ..
        } => spend,
        other => unreachable!("not a restricted mana ability: {other:?}"),
    }
}

/// The mana `p` could spend on `payee`: the unrestricted pool plus the restricted mana
/// that may pay for it (CR 106.6).
pub(crate) fn usable_pool(st: &GameState, p: PlayerId, payee: Option<Payee>) -> ManaPool {
    let mut pool = st.player(p).mana_pool;
    if let Some(payee) = payee {
        for r in &st.player(p).restricted_mana {
            if may_spend(st, p, restriction(st, r), payee) {
                pool.add(r.mana, r.amount);
            }
        }
    }
    pool
}

/// CR 903.4: the colors of mana "in your commander's color identity", as a mask. Empty
/// without a commander (CR 903.4f: the ability then adds nothing).
pub(crate) fn identity_mask(st: &GameState, p: PlayerId) -> ManaMask {
    let mut mask = 0;
    for (owner, instance) in &st.commanders {
        if *owner != p {
            continue;
        }
        // CR 903.4a: the identity established for the card as the game began.
        if let Some(o) = st.objects.values().find(|o| o.card_instance == *instance) {
            for c in card_model::color_identity(st.catalog.get(o.card)) {
                mask |= 1 << ManaType::from_color(c).index();
            }
        }
    }
    mask
}

/// What paying for `source` asks (CR 601.2f–h), for the planner (`frames::mana`).
fn goal(
    source: &CastSource,
    prop: &Proposal,
    payee: Option<Payee>,
    req: &ManaRequirement,
) -> PayGoal {
    PayGoal {
        req: *req,
        payee,
        tap: prop.tap,
        sacrifice: prop.sacrifice,
        sac: prop.extra.sacrifice.clone(),
        sac_source: cost_source(source),
        life: prop.extra.life,
    }
}

/// Can `p` pay the mana `req` for `source` (with what is in their pool and the mana
/// abilities they could activate), and still pay the rest of its cost?
fn feasible(
    st: &GameState,
    p: PlayerId,
    source: &CastSource,
    prop: &Proposal,
    payee: Option<Payee>,
    req: &ManaRequirement,
) -> bool {
    goal_feasible(st, p, &goal(source, prop, payee, req))
}

/// CR 118.3: can `p` pay `cost` right now — for an ability of `source` that is resolving?
/// (`{T}` and "sacrifice this" need the source to be theirs and able.)
pub(crate) fn can_pay(st: &GameState, p: PlayerId, source: ObjectId, cost: &Cost) -> bool {
    let own = st
        .objects
        .get(&source)
        .is_some_and(|o| o.zone == Zone::Battlefield && controller_of(st, source) == p);
    if (cost.tap || cost.sacrifice_self) && !own {
        return false;
    }
    if cost.tap && !can_tap_for_ability(st, source) {
        return false;
    }
    let src = CastSource::Payment {
        source,
        cost: Box::new(cost.clone()),
    };
    let prop = proposal(st, &src);
    extra_payable(st, p, &src, &prop)
        && feasible(
            st,
            p,
            &src,
            &prop,
            payee_of(&src),
            &total(&prop.cost, 0, 0, 0),
        )
}

/// Mode indices whose every target slot has at least one legal choice (CR 700.2a).
fn legal_modes(
    st: &GameState,
    body: &Body,
    p: PlayerId,
    this: Option<ObjectId>,
    source: Option<ObjectId>,
) -> Vec<u8> {
    body.modes()
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.targets.iter().enumerate().all(|(i, f)| {
                match m.group_of(i) {
                    None => !candidates(st, f, p, this, source).is_empty(),
                    // CR 115.3: a group needs `min` different targets (checked once).
                    Some(g) => {
                        i != g.first as usize
                            || candidates(st, f, p, this, source).len() >= g.min as usize
                    }
                }
            })
        })
        .map(|(i, _)| i as u8)
        .collect()
}

fn body_possible(st: &GameState, body: &Body, p: PlayerId, source: Option<ObjectId>) -> bool {
    let n = legal_modes(st, body, p, None, source).len();
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
        CastSource::Trigger(_) | CastSource::Payment { .. } => return false,
        CastSource::Ability { source, ability } => {
            let (source, ability) = (*source, *ability);
            let o = st.object(source);
            // CLAUDE.md invariant 4: only abilities the object currently has.
            let Some((
                _,
                AbilityDef::Activated {
                    cost,
                    timing,
                    only_if,
                    from,
                    ..
                },
            )) = abilities(st, source)
                .into_iter()
                .find(|(i, _)| *i == ability)
            else {
                return false;
            };
            // CR 113.6: the zone the ability works from. Off the battlefield a card is
            // its owner's to use.
            let here = match from {
                card_model::ActivationZone::Battlefield => {
                    o.zone == Zone::Battlefield && controller_of(st, source) == p
                }
                card_model::ActivationZone::Hand => o.zone == Zone::Hand && o.owner == p,
                card_model::ActivationZone::Graveyard => o.zone == Zone::Graveyard && o.owner == p,
            };
            if !here {
                return false;
            }
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
        && !body_possible(st, b, p, ability_source(&source))
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
    let payee = payee_of(&source);
    feasible(
        st,
        p,
        &source,
        &prop,
        payee,
        &total(&prop.cost, 0, tax, discount),
    )
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
            generic: [0; 6],
            targets_closed: Vec::new(),
        }
    }

    pub(crate) fn trigger(t: PendingTrigger) -> Self {
        CastFrame::new(t.controller, CastSource::Trigger(Box::new(t)))
    }

    /// `player` pays `cost` now, for an ability of `source` that is resolving. Only for a
    /// cost `can_pay` says they can pay.
    pub(crate) fn payment(player: PlayerId, source: ObjectId, cost: Cost) -> Self {
        let mut f = CastFrame::new(
            player,
            CastSource::Payment {
                source,
                cost: Box::new(cost),
            },
        );
        f.stage = CastStage::Pay;
        f
    }

    /// The proposal's body/cost, read through the *current* source object.
    fn proposal(&self, st: &GameState) -> Proposal {
        match &self.source {
            CastSource::Card(_) => proposal(st, &CastSource::Card(self.object.expect("announced"))),
            s => proposal(st, s),
        }
    }

    /// What the mana is spent on: the spell as it is now (on the stack), or the ability's
    /// source.
    fn payee(&self) -> Option<Payee> {
        match &self.source {
            CastSource::Card(_) => self.object.map(Payee::Spell),
            s => payee_of(s),
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
                        CastSource::Ability { source, ability } => {
                            let source_card = st.object(*source).card;
                            // CR 602.2a: a card whose ability is activated from a hidden
                            // zone is revealed.
                            if st.object(*source).zone == Zone::Hand {
                                st.emit(Event::Revealed {
                                    player: p,
                                    cards: vec![*source],
                                });
                            }
                            put(
                                st,
                                GameAction::PutAbilityOnStack {
                                    controller: p,
                                    source: *source,
                                    source_card,
                                    ability: *ability,
                                    triggered: false,
                                    source_lki: None,
                                    subject: None,
                                    subject_lki: None,
                                    facts: Default::default(),
                                },
                            )
                        }
                        CastSource::Payment { .. } => {
                            unreachable!("a payment starts at the Pay stage")
                        }
                        // CR 603.3: the triggered ability becomes the topmost object.
                        CastSource::Trigger(t) => put(
                            st,
                            GameAction::PutAbilityOnStack {
                                controller: t.controller,
                                source: t.source,
                                source_card: t.source_card,
                                ability: t.ability,
                                triggered: true,
                                source_lki: t.source_lki.clone(),
                                subject: t.subject,
                                subject_lki: t.subject_lki.clone(),
                                facts: t.facts,
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
                                let legal = legal_modes(
                                    st,
                                    &body,
                                    p,
                                    self.object,
                                    ability_source(&self.source),
                                );
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
                            let payee = self.payee();
                            while feasible(
                                st,
                                p,
                                &self.source,
                                &prop,
                                payee,
                                &total(&prop.cost, max + 1, self.tax, self.discount),
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
                    // The slots of a target group take different targets, and those beyond
                    // its minimum may be left empty ("up to two target creatures").
                    let body = self.proposal(st).body;
                    let modes: Vec<card_model::Mode> = self
                        .modes
                        .iter()
                        .map(|&m| body.as_ref().unwrap().modes()[m as usize].clone())
                        .collect();
                    if self.targets_closed.len() != modes.len() {
                        self.targets_closed = vec![false; modes.len()];
                    }
                    let next = (0..modes.len()).find(|&k| {
                        !self.targets_closed[k] && self.targets[k].len() < modes[k].targets.len()
                    });
                    if let Some(inp) = input.take() {
                        let k = next.expect("asked about a target slot");
                        match inp.picked_one().cloned() {
                            Some(OptionDesc::Target { target }) => self.targets[k].push(target),
                            None => self.targets_closed[k] = true,
                            Some(other) => unreachable!("{other:?}"),
                        }
                        continue;
                    }
                    if let Some(k) = next {
                        let i = self.targets[k].len();
                        let mode = &modes[k];
                        let group = mode.group_of(i);
                        let optional = group.is_some_and(|g| i >= (g.first + g.min) as usize);
                        let taken: &[Target] = match group {
                            Some(g) => &self.targets[k][g.first as usize..],
                            None => &[],
                        };
                        let options: Vec<OptionDesc> = candidates(
                            st,
                            &mode.targets[i],
                            p,
                            self.object,
                            ability_source(&self.source),
                        )
                        .into_iter()
                        .filter(|t| !taken.contains(t))
                        .map(|target| OptionDesc::Target { target })
                        .collect();
                        if options.is_empty() {
                            if optional {
                                self.targets_closed[k] = true;
                                continue;
                            }
                            // CR 603.3d (only triggers can get here, D-019).
                            debug_assert!(self.is_trigger());
                            return self.remove_trigger(st);
                        }
                        let slot = modes[..k].iter().map(|m| m.targets.len()).sum::<usize>() + i;
                        return self.ask(
                            ChoiceKind::ChooseTargets { slot: slot as u8 },
                            Primitive::PickN {
                                min: usize::from(!optional),
                                max: 1,
                            },
                            options,
                        );
                    }
                    let object = self.object.unwrap();
                    if modes.iter().any(|m| !m.targets.is_empty()) {
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
                    self.stage = CastStage::Pay;
                }
                // CR 601.2h: which permanents / cards pay the cost, chosen once the mana
                // abilities are done (CR 601.2g): a land can be tapped for mana and then
                // sacrificed, but a Treasure sacrificed for mana is gone. They leave with
                // the rest of the payment (`finish`).
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
                    if sacrificing {
                        self.stage = CastStage::ChooseDiscard;
                    } else {
                        let prop = self.proposal(st);
                        return self.finish(st, prop);
                    }
                }
                CastStage::Pay => {
                    // CR 601.2f: total cost, locked in. CR 601.2g: mana abilities.
                    let prop = self.proposal(st);
                    let req = total(&prop.cost, self.x, self.tax, self.discount);
                    let payee = self.payee();
                    if let Some(inp) = input.take() {
                        let Some(OptionDesc::TapForMana {
                            source,
                            mana,
                            ability,
                        }) = inp.picked_one().cloned()
                        else {
                            unreachable!()
                        };
                        // CR 605.3b: the mana ability resolves at once (its own costs and
                        // choices first, D-066); this frame carries on after.
                        let g = goal(&self.source, &prop, payee, &req);
                        return super::mana::activate(
                            st,
                            p,
                            Frame::Cast(self),
                            source,
                            mana,
                            ability,
                            Some(g),
                        );
                    }
                    let pool = usable_pool(st, p, payee);
                    if req.payable_from(&pool) {
                        match req.forced_generic(&pool) {
                            Some(alloc) => {
                                self.generic = alloc;
                                self.stage = CastStage::ChooseSacrifice;
                                continue;
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
                    let options = pay_options(st, p, &goal(&self.source, &prop, payee, &req));
                    assert!(
                        !options.is_empty(),
                        "payment dead-ended (D-019 violated): {p:?} paying {req:?} for {:?} \
                         ({}); pool {pool:?}",
                        self.source,
                        st.objects
                            .get(&cost_source(&self.source))
                            .map_or("gone", |o| st.catalog.get(o.card).name.as_str()),
                    );
                    return self.ask(
                        ChoiceKind::PayMana,
                        Primitive::PickN { min: 1, max: 1 },
                        options,
                    );
                }
                CastStage::PayGeneric => {
                    let inp = input.take().expect("generic allocation");
                    let mut alloc = [0u32; 6];
                    for (i, &n) in inp.allocation().iter().enumerate() {
                        let OptionDesc::Mana { mana } = inp.decision.options[i] else {
                            unreachable!()
                        };
                        alloc[mana.index()] = n;
                    }
                    self.generic = alloc;
                    self.stage = CastStage::ChooseSacrifice;
                }
            }
        }
    }

    /// CR 601.2h: pay; CR 601.2i: the spell becomes cast / the ability becomes activated.
    fn finish(self, st: &mut GameState, prop: Proposal) -> Flow {
        let p = self.player;
        let req = total(&prop.cost, self.x, self.tax, self.discount);
        let (tap, sacrifice, life) = (prop.tap, prop.sacrifice, prop.extra.life);
        if req.total() > 0 {
            // CR 106.6: restricted mana that may pay for this is spent first (it is the
            // least useful to keep).
            if let Some(payee) = self.payee() {
                let mut need = [0u32; 6];
                for (t, n) in need.iter_mut().enumerate() {
                    *n = req.specific[t] + self.generic[t];
                }
                let mut release: Vec<(u32, u32)> = Vec::new();
                for (i, r) in st.player(p).restricted_mana.iter().enumerate() {
                    let n = &mut need[r.mana.index()];
                    let amount = r.amount.min(*n);
                    if amount > 0 && may_spend(st, p, restriction(st, r), payee) {
                        *n -= amount;
                        release.push((i as u32, amount));
                    }
                }
                // Last first: an emptied entry is removed.
                for (entry, amount) in release.into_iter().rev() {
                    execute(
                        st,
                        GameAction::ReleaseRestrictedMana {
                            player: p,
                            entry,
                            amount,
                        },
                    );
                }
            }
            execute(
                st,
                GameAction::PayMana {
                    player: p,
                    requirement: req,
                    generic: self.generic,
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
        if !self.pay_sacrifice.is_empty()
            && let Some(object) = self.object
        {
            // CR 608.2h: "the sacrificed creature's power" is its power as it last existed.
            let power = self
                .pay_sacrifice
                .iter()
                .filter(|id| on_battlefield(st, id))
                .map(|&id| chars(st, id).power.unwrap_or(0).max(0) as u32)
                .sum();
            execute(st, GameAction::SetSacrificedPower { object, power });
        }
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
        // "Discard this card" / "exile this card from your graveyard": the card the
        // ability is on, where the ability works from.
        if let CastSource::Ability { source, ability } = &self.source
            && let Some(o) = st.objects.get(source)
        {
            if prop.extra.discard_self && o.zone == Zone::Hand {
                // CR 702.29c: discarding it for its cycling ability is cycling it.
                let cycling = matches!(
                    st.catalog.ability(*ability),
                    AbilityDef::Activated { cycling: true, .. }
                );
                let cause = if cycling {
                    MoveCause::Cycle
                } else {
                    MoveCause::Discard
                };
                events.push(Proposed::moving(*source, Zone::Graveyard, cause));
            }
            if prop.extra.exile_self && o.zone == Zone::Graveyard {
                events.push(Proposed::moving(*source, Zone::Exile, MoveCause::Rule));
            }
        }
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
