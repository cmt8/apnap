//! Casting spells (CR 601.2) and activating non-mana abilities (CR 602.2), as one resumable
//! frame. Text: docs/rules-notes/601-casting-602-activating-605-mana.md.
//!
//! **No rewinds (D-019).** CR 601.2/602.2 say an uncompletable cast "returns to the moment
//! before". apnap never gets there: `can_begin` only offers a cast/activation if a complete
//! legal sequence exists, and every later step only offers choices that keep completion
//! possible (legal modes, payable X, targets that exist, mana abilities that keep the cost
//! payable). So any shape-valid response leads to a completed cast.

use super::{Flow, Frame, Input};
use crate::actions::{GameAction, execute, move_object};
use crate::decision::{ChoiceKind, OptionDesc, Primitive, Response};
use crate::events::Event;
use crate::ids::{ObjectId, PlayerId};
use crate::layers::{abilities, chars, controller_of};
use crate::mana::{ManaMask, ManaRequirement, ManaType, mask_of, types_in};
use crate::stack::Target;
use crate::state::GameState;
use crate::targeting::candidates;
use crate::triggers::{self, PendingTrigger};
use crate::zones::{LibraryPosition, Zone};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use card_model::{AbilityDef, Body, CardType, Keyword, ManaCost, Timing};
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
    /// Cast from the command zone (CR 903.8: counts toward future tax).
    pub from_command: Option<crate::ids::CardInstanceId>,
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
        },
        CastSource::Ability { source, index } => {
            match &st.card_def(*source).abilities[*index as usize] {
                AbilityDef::Activated { cost, body, .. } => Proposal {
                    body: Some(body.clone()),
                    cost: cost.mana.clone(),
                    tap: cost.tap.then_some(*source),
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

/// CR 601.2f: the total cost — the mana cost plus additional costs such as commander tax.
fn total(cost: &Option<ManaCost>, x: u32, tax: u32) -> ManaRequirement {
    let mut r = requirement(cost, x);
    r.generic += tax;
    r
}

/// CR 302.6 / 602.5a: can this permanent's `{T}` abilities be activated now?
pub(crate) fn can_tap_for_ability(st: &GameState, id: ObjectId) -> bool {
    let o = st.object(id);
    let c = chars(st, id);
    !o.tapped && (!c.is(CardType::Creature) || !o.summoning_sick || c.has(Keyword::Haste))
}

/// Untapped mana sources `p` controls (battlefield order) and the mana types each can make.
/// Every M1 mana ability costs exactly `{T}` (D-021), so one source = one mana.
pub(crate) fn mana_sources(
    st: &GameState,
    p: PlayerId,
    exclude: Option<ObjectId>,
) -> Vec<(ObjectId, ManaMask)> {
    st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| Some(id) != exclude && controller_of(st, id) == p)
        .filter_map(|id| {
            let c = chars(st, id);
            let mut mask: ManaMask = 0;
            if c.is(CardType::Land) {
                // CR 305.6: each basic land type grants "{T}: Add [mana]".
                for t in &c.land_types {
                    mask |= 1 << ManaType::from_color(t.produces()).index();
                }
            }
            for (_, a) in abilities(st, id) {
                if let AbilityDef::Mana { produce, .. } = a {
                    mask |= mask_of(&produce.options());
                }
            }
            (mask != 0 && can_tap_for_ability(st, id)).then_some((id, mask))
        })
        .collect()
}

fn feasible(st: &GameState, p: PlayerId, req: &ManaRequirement, exclude: Option<ObjectId>) -> bool {
    let masks: Vec<ManaMask> = mana_sources(st, p, exclude).iter().map(|s| s.1).collect();
    req.feasible(&st.player(p).mana_pool, &masks)
}

/// Mode indices whose every target slot has at least one legal choice (CR 700.2a).
fn legal_modes(st: &GameState, body: &Body, p: PlayerId, this: Option<ObjectId>) -> Vec<u8> {
    body.modes()
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.targets
                .iter()
                .all(|f| !candidates(st, *f, p, this).is_empty())
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
            let Some((_, AbilityDef::Activated { cost, timing, .. })) =
                abilities(st, source).into_iter().find(|(i, _)| *i == index)
            else {
                return false;
            };
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
    let tax = match &source {
        CastSource::Card(card) => commander_tax(st, p, *card),
        _ => 0,
    };
    feasible(st, p, &total(&prop.cost, 0, tax), prop.tap)
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
            from_command: None,
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
                            while feasible(st, p, &total(&prop.cost, max + 1, self.tax), prop.tap) {
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
                                .map(move |f| (k, *f))
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
                    if let Some(&(_, filter)) = slots.get(filled) {
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
                    self.stage = CastStage::Pay;
                }
                CastStage::Pay => {
                    // CR 601.2f: total cost, locked in. CR 601.2g: mana abilities.
                    let prop = self.proposal(st);
                    let req = total(&prop.cost, self.x, self.tax);
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
                            Some(alloc) => return self.finish(st, req, alloc, prop.tap),
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
                    let masks: Vec<ManaMask> = sources.iter().map(|s| s.1).collect();
                    let mut options = Vec::new();
                    for (i, &(source, mask)) in sources.iter().enumerate() {
                        let mut rest = masks.clone();
                        rest.remove(i);
                        for mana in types_in(mask) {
                            let mut pl = pool;
                            pl.add(mana, 1);
                            if req.feasible(&pl, &rest) {
                                options.push(OptionDesc::TapForMana { source, mana });
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
                    let req = total(&prop.cost, self.x, self.tax);
                    let mut alloc = [0u32; 6];
                    for (i, &n) in inp.allocation().iter().enumerate() {
                        let OptionDesc::Mana { mana } = inp.decision.options[i] else {
                            unreachable!()
                        };
                        alloc[mana.index()] = n;
                    }
                    return self.finish(st, req, alloc, prop.tap);
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
        Flow::done()
    }
}

/// CR 605.3b: activate a `{T}` mana ability; it resolves immediately.
pub(crate) fn tap_for_mana(st: &mut GameState, p: PlayerId, source: ObjectId, mana: ManaType) {
    execute(st, GameAction::Tap { object: source });
    execute(st, GameAction::AddMana { player: p, mana });
}
