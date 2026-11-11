//! Heuristic agent v1 (spec §9 Phase A).
//!
//! Sees only its `PlayerView` (CLAUDE.md invariant 6) plus public card text from the catalog.
//! All scoring is integer arithmetic (no floats: spec §15 pitfall 4); ties are broken by
//! option order, so it is fully deterministic.
//!
//! Commander awareness (the documented Forge failures, spec §2): every blocking plan first
//! prevents lethal commander damage (CR 903.10a) — counting damage already taken from that
//! commander — then lethal life loss, then makes value blocks. Attacks favour lethal
//! (life or commander damage) and the highest-threat opponent.

use crate::RandomAgent;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use card_model::{
    AbilityDef, Body, CardCatalog, CardType, ContinuousEffect, CounterKind, Effect, Keyword,
};
use engine_core::decision::{ChoiceKind, OptionDesc, PendingDecision, Primitive, Response};
use engine_core::stack::Target;
use engine_core::view::{ObjectView, PlayerView};
use engine_core::{Agent, ObjectId, PlayerId};

/// Whether an effect is meant for opponents or for yourself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Polarity {
    Harm,
    Help,
    Neutral,
}

fn effect_polarity(e: &Effect) -> Polarity {
    match e {
        Effect::Seq(v) => v
            .iter()
            .map(effect_polarity)
            .find(|p| *p != Polarity::Neutral)
            .unwrap_or(Polarity::Neutral),
        Effect::DealDamage { .. }
        | Effect::Destroy { .. }
        | Effect::Counter { .. }
        | Effect::LoseLife { .. }
        | Effect::ReturnToHand { .. }
        | Effect::Exile { .. }
        | Effect::ExileGraveyard { .. }
        | Effect::Tap { .. }
        | Effect::DamageEachPlayer { .. } => Polarity::Harm,
        // Pointed at a player, these are meant for an opponent.
        Effect::Discard { who, .. }
        | Effect::DiscardHand { who }
        | Effect::Sacrifice { who, .. } => match who {
            card_model::PlayerRef::Target(_) => Polarity::Harm,
            _ => Polarity::Neutral,
        },
        Effect::Untap { .. } | Effect::AttachThis { .. } => Polarity::Help,
        // Putting a permanent into its owner's library or hand: aimed at an opponent's.
        Effect::PutInto { .. } => Polarity::Harm,
        Effect::ChooseCards { .. }
        | Effect::Scry { .. }
        | Effect::Surveil { .. }
        | Effect::Dig { .. }
        | Effect::MoveAll { .. } => Polarity::Neutral,
        Effect::TapAll(_) | Effect::UntapAll(_) | Effect::SacrificeThis => Polarity::Neutral,
        Effect::PutCountersAll { .. } | Effect::ThisSpellTo { .. } => Polarity::Neutral,
        Effect::PutCounters { kind, .. } => match kind {
            CounterKind::PlusOne => Polarity::Help,
            CounterKind::MinusOne => Polarity::Harm,
        },
        Effect::Apply { effects, .. } => continuous_polarity(effects),
        // What it does is in abilities written elsewhere on the card: no preference.
        Effect::Lasting { .. } => Polarity::Neutral,
        Effect::May(inner) => effect_polarity(inner),
        // What happens either way: paying, or not paying.
        Effect::Pay { paid, unpaid, .. } => match effect_polarity(unpaid) {
            Polarity::Neutral => effect_polarity(paid),
            p => p,
        },
        Effect::Together(v) => v.first().map_or(Polarity::Neutral, effect_polarity),
        Effect::GainLife { .. }
        | Effect::AddMana { .. }
        | Effect::AddManaOf { .. }
        | Effect::ReturnToBattlefield { .. }
        | Effect::Draw { .. }
        | Effect::CreateToken { .. }
        | Effect::PreventNext { .. } => Polarity::Help,
        // No target to be for or against: these hit everything that matches.
        Effect::EachPlayer(_)
        | Effect::DestroyAll(_)
        | Effect::DamageEach { .. }
        | Effect::ApplyAll { .. }
        | Effect::Mill { .. }
        | Effect::SetX(_) => Polarity::Neutral,
    }
}

fn continuous_polarity(effects: &[ContinuousEffect]) -> Polarity {
    for e in effects {
        match e {
            ContinuousEffect::GainControl
            | ContinuousEffect::LoseAllAbilities
            | ContinuousEffect::LoseKeywords(_) => return Polarity::Harm,
            ContinuousEffect::SetPT(p, t) => {
                return if *p + *t <= 2 {
                    Polarity::Harm
                } else {
                    Polarity::Help
                };
            }
            ContinuousEffect::ModifyPT(p, t) => {
                return if *p + *t < 0 {
                    Polarity::Harm
                } else {
                    Polarity::Help
                };
            }
            ContinuousEffect::ModifyPTBy { power, toughness } => {
                return if power.negative || toughness.negative {
                    Polarity::Harm
                } else {
                    Polarity::Help
                };
            }
            ContinuousEffect::AddKeywords(_)
            | ContinuousEffect::AddTypes { .. }
            | ContinuousEffect::GrantAbilities(_) => {
                return Polarity::Help;
            }
            ContinuousEffect::SetColors(_)
            | ContinuousEffect::SwitchPT
            | ContinuousEffect::SetPTBy { .. } => {}
        }
    }
    Polarity::Neutral
}

fn kw_value(k: Keyword) -> i64 {
    match k {
        Keyword::DoubleStrike => 4,
        Keyword::Flying | Keyword::Deathtouch => 3,
        Keyword::FirstStrike | Keyword::Trample | Keyword::Lifelink | Keyword::Menace => 2,
        Keyword::Indestructible => 4,
        Keyword::Hexproof | Keyword::Shroud => 2,
        Keyword::Fear
        | Keyword::Intimidate
        | Keyword::Landwalk(_)
        | Keyword::Shadow
        | Keyword::Horsemanship => 2,
        Keyword::Vigilance
        | Keyword::Haste
        | Keyword::Reach
        | Keyword::Flash
        | Keyword::Flanking => 1,
        Keyword::Defender => -1,
    }
}

/// A rough value of a permanent (higher = more important).
fn value(o: &ObjectView) -> i64 {
    let p = i64::from(o.power.unwrap_or(0));
    let t = i64::from(o.toughness.unwrap_or(0));
    let kw: i64 = o.keywords.iter().map(|k| kw_value(*k)).sum();
    let base = if o.card_types.contains(&CardType::Creature) {
        3 * p + 2 * t + kw
    } else {
        i64::from(o.mana_value) * 2
    };
    base + i64::from(o.mana_value) + if o.is_commander { 6 } else { 0 }
}

fn has(o: &ObjectView, k: Keyword) -> bool {
    o.keywords.contains(&k)
}

fn power(o: &ObjectView) -> i64 {
    i64::from(o.power.unwrap_or(0)).max(0)
}

/// Damage still needed to destroy `o` (CR 704.5g).
fn remaining_toughness(o: &ObjectView) -> i64 {
    (i64::from(o.toughness.unwrap_or(0)) - i64::from(o.damage_marked)).max(0)
}

#[derive(Debug)]
pub struct HeuristicAgent {
    catalog: Arc<CardCatalog>,
    fallback: RandomAgent,
    /// The plan for the current combat's blocks: blocker → attacker.
    block_plan: BTreeMap<ObjectId, ObjectId>,
    block_key: Option<(u32, Vec<ObjectId>)>,
    /// The blockers already asked about in this combat. Being asked about one again means
    /// the declaration was illegal (a planned block wasn't possible, leaving a creature
    /// with menace blocked by one) and must be made anew.
    block_asked: Vec<ObjectId>,
    /// The plan for the current combat's attacks: attacker → defender.
    attack_plan: BTreeMap<ObjectId, PlayerId>,
    attack_key: Option<u32>,
    /// The modes chosen for the spell currently being cast (targets come next).
    last_modes: Vec<u8>,
    /// How often each ability was activated this turn (`ability_turn`). An ability that
    /// costs nothing would otherwise be activated every time this player has priority.
    ability_uses: BTreeMap<(ObjectId, card_model::AbilityId), u8>,
    ability_turn: u32,
}

/// How many times the heuristic agent activates one ability in a turn.
const ABILITY_USES_PER_TURN: u8 = 3;

impl HeuristicAgent {
    pub fn new(seed: u64, catalog: Arc<CardCatalog>) -> Self {
        HeuristicAgent {
            catalog,
            fallback: RandomAgent::new(seed),
            block_plan: BTreeMap::new(),
            block_key: None,
            block_asked: Vec::new(),
            attack_plan: BTreeMap::new(),
            attack_key: None,
            last_modes: Vec::new(),
            ability_uses: BTreeMap::new(),
            ability_turn: 0,
        }
    }

    fn me(v: &PlayerView) -> PlayerId {
        v.viewer
    }

    fn opponents(v: &PlayerView) -> Vec<PlayerId> {
        v.players
            .iter()
            .filter(|p| p.id != v.viewer && !p.has_lost)
            .map(|p| p.id)
            .collect()
    }

    fn find(v: &PlayerView, id: ObjectId) -> Option<&ObjectView> {
        v.hand
            .iter()
            .chain(&v.battlefield)
            .chain(&v.command)
            .chain(&v.exile)
            .chain(v.players.iter().flat_map(|p| &p.graveyard))
            // Cards the pending decision lets this player look at (a library search…).
            .chain(&v.revealed)
            .find(|o| o.id == id)
    }

    fn creatures_of(v: &PlayerView, p: PlayerId) -> impl Iterator<Item = &ObjectView> {
        v.battlefield
            .iter()
            .filter(move |o| o.controller == p && o.card_types.contains(&CardType::Creature))
    }

    fn life(v: &PlayerView, p: PlayerId) -> i64 {
        i64::from(v.players[p.index()].life)
    }

    /// Threat assessment T(q) (spec §9): board power aimed at everyone + how close q is to
    /// winning.
    fn threat(v: &PlayerView, q: PlayerId) -> i64 {
        let board: i64 = Self::creatures_of(v, q).map(value).sum();
        board + Self::life(v, q) / 2 + v.players[q.index()].hand_size as i64
    }

    /// Commander damage `victim` has taken from commander card `attacker`.
    fn commander_damage_taken(v: &PlayerView, victim: PlayerId, attacker: &ObjectView) -> i64 {
        v.players[victim.index()]
            .commander_damage
            .iter()
            .find(|(c, _)| *c == attacker.card_instance)
            .map_or(0, |(_, n)| i64::from(*n))
    }

    // --- Spells -----------------------------------------------------------------------------

    /// The body of the spell or ability currently being put on the stack (the top object).
    fn top_body(&self, v: &PlayerView) -> Option<Body> {
        let top = v.stack.last()?;
        let def = self.catalog.get(top.card);
        match top.ability {
            None => def.spell_ability().cloned().or_else(|| {
                def.enchant().map(|f| {
                    let effects: Vec<ContinuousEffect> = def
                        .abilities
                        .iter()
                        .find_map(|a| match a {
                            AbilityDef::Static { effects, .. } => Some(effects.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    Body::Effect(card_model::Mode {
                        targets: vec![f],
                        groups: Vec::new(),
                        effect: Effect::Apply {
                            what: card_model::ObjRef::Target(0),
                            effects,
                            until_end_of_turn: false,
                        },
                    })
                })
            }),
            Some(a) => match self.catalog.ability(a) {
                AbilityDef::Activated { body, .. } | AbilityDef::Triggered { body, .. } => {
                    Some(body.clone())
                }
                _ => None,
            },
        }
    }

    fn mode_polarity(body: &Body, mode: u8) -> Polarity {
        body.modes()
            .get(mode as usize)
            .map_or(Polarity::Neutral, |m| effect_polarity(&m.effect))
    }

    /// How good it is to point a `polarity` effect (dealing `dmg` damage, if any) at `t`.
    fn target_score(v: &PlayerView, t: &Target, polarity: Polarity, dmg: i64) -> i64 {
        let me = Self::me(v);
        match (t, polarity) {
            (Target::Player(q), Polarity::Harm) if *q != me => {
                let lethal = if dmg > 0 && Self::life(v, *q) <= dmg {
                    10_000
                } else {
                    0
                };
                lethal + Self::threat(v, *q) / 4
            }
            (Target::Player(q), Polarity::Help) if *q == me => 5,
            (Target::Object(id), _) => match Self::find(v, *id) {
                Some(o) => {
                    let mine = o.controller == me;
                    match polarity {
                        Polarity::Harm if !mine => {
                            let kills = dmg > 0 && remaining_toughness(o) <= dmg;
                            value(o) + if kills || dmg == 0 { 20 } else { -15 }
                        }
                        Polarity::Help if mine => value(o),
                        _ => -1_000,
                    }
                }
                // A spell on the stack (counterspells).
                None => match v.stack.iter().find(|s| s.id == *id) {
                    Some(s) if s.controller != me && polarity == Polarity::Harm => 10,
                    _ => -1_000,
                },
            },
            _ => -1_000,
        }
    }

    fn damage_of(body: &Body, mode: u8) -> i64 {
        fn walk(e: &Effect) -> i64 {
            match e {
                Effect::DealDamage { amount, .. } => match amount {
                    card_model::Amount::Fixed(n) => i64::from(*n),
                    _ => 2,
                },
                Effect::Seq(v) => v.iter().map(walk).max().unwrap_or(0),
                _ => 0,
            }
        }
        body.modes()
            .get(mode as usize)
            .map_or(0, |m| walk(&m.effect))
    }

    /// The best a spell/ability could do now, judged by its best target (or 0/positive for
    /// untargeted effects). Negative = don't.
    fn spell_score(
        &self,
        v: &PlayerView,
        card: card_model::CardDefId,
        ability: Option<card_model::AbilityId>,
    ) -> i64 {
        let def = self.catalog.get(card);
        let body = match ability {
            Some(a) => match self.catalog.ability(a) {
                AbilityDef::Activated { body, .. } => Some(body.clone()),
                _ => None,
            },
            None => def.spell_ability().cloned(),
        };
        let me = Self::me(v);
        let Some(body) = body else {
            // A permanent spell (or Aura).
            if let Some(_f) = def.enchant() {
                let harm = def.abilities.iter().any(|a| {
                    matches!(a, AbilityDef::Static { effects, .. } if continuous_polarity(effects) == Polarity::Harm)
                });
                let theirs = v
                    .battlefield
                    .iter()
                    .filter(|o| o.card_types.contains(&CardType::Creature))
                    .filter(|o| (o.controller != me) == harm)
                    .map(value)
                    .max();
                return theirs.map_or(-1, |b| b + 5);
            }
            return 10 + i64::from(def.mana_cost.as_ref().map_or(0, |m| m.mana_value())) * 5;
        };
        let mut best = -1;
        for (mi, m) in body.modes().iter().enumerate() {
            let pol = effect_polarity(&m.effect);
            let dmg = Self::damage_of(&body, mi as u8);
            let s = if m.targets.is_empty() {
                match &m.effect {
                    Effect::DamageEachPlayer { .. } => {
                        // Only if it kills someone and doesn't kill me.
                        let d = dmg.max(3);
                        let kills = Self::opponents(v).iter().any(|q| Self::life(v, *q) <= d);
                        if kills && Self::life(v, me) > d {
                            500
                        } else {
                            -1
                        }
                    }
                    Effect::EachPlayer(card_model::EachPlayerAction::SacrificeCreature(_)) => {
                        let mine = Self::creatures_of(v, me).count();
                        let theirs = Self::opponents(v)
                            .iter()
                            .filter(|q| Self::creatures_of(v, **q).count() > 0)
                            .count();
                        if mine <= 1 && theirs >= 1 { 20 } else { -1 }
                    }
                    Effect::EachPlayer(card_model::EachPlayerAction::Discard(_)) => {
                        if v.hand.len() <= 2 { 10 } else { -1 }
                    }
                    _ => 8,
                }
            } else {
                // Score against every visible target of the first slot.
                let mut cands: Vec<Target> = v
                    .battlefield
                    .iter()
                    .map(|o| Target::Object(o.id))
                    .chain(
                        v.players
                            .iter()
                            .filter(|p| !p.has_lost)
                            .map(|p| Target::Player(p.id)),
                    )
                    .chain(v.stack.iter().map(|s| Target::Object(s.id)))
                    .collect();
                if matches!(
                    m.targets[0],
                    card_model::TargetFilter::Spell | card_model::TargetFilter::SpellMatching(_)
                ) {
                    cands.retain(
                        |t| matches!(t, Target::Object(id) if v.stack.iter().any(|s| s.id == *id)),
                    );
                }
                cands
                    .iter()
                    .map(|t| Self::target_score(v, t, pol, dmg))
                    .max()
                    .unwrap_or(-1)
            };
            best = best.max(s);
        }
        best
    }

    fn priority(&mut self, v: &PlayerView, d: &PendingDecision) -> Response {
        let me = Self::me(v);
        if self.ability_turn != v.turn_number {
            self.ability_turn = v.turn_number;
            self.ability_uses.clear();
        }
        // 1. Play a land (prefer one that doesn't enter tapped: basics first).
        let lands: Vec<usize> = d
            .options
            .iter()
            .enumerate()
            .filter(|(_, o)| matches!(o, OptionDesc::PlayLand { .. }))
            .map(|(i, _)| i)
            .collect();
        if let Some(&i) = lands.iter().find(|&&i| match &d.options[i] {
            OptionDesc::PlayLand { card } => Self::find(v, *card).is_some_and(|o| {
                self.catalog
                    .get(o.card)
                    .supertypes
                    .contains(&card_model::Supertype::Basic)
            }),
            _ => false,
        }) {
            return Response::Pick(vec![i]);
        }
        if let Some(&i) = lands.first() {
            return Response::Pick(vec![i]);
        }
        // 2. The best spell or ability with a positive score.
        let my_main = v.active == me;
        let mut best: Option<(i64, usize)> = None;
        for (i, o) in d.options.iter().enumerate() {
            let s = match o {
                OptionDesc::CastSpell { card } => {
                    let Some(ov) = Self::find(v, *card) else {
                        continue;
                    };
                    let def = self.catalog.get(ov.card);
                    let permanent =
                        !def.has_type(CardType::Instant) && !def.has_type(CardType::Sorcery);
                    // Creatures with flash: on an opponent's turn only when the stack is empty.
                    if permanent && !my_main && !v.stack.is_empty() {
                        continue;
                    }
                    self.spell_score(v, ov.card, None) + if ov.is_commander { 20 } else { 0 }
                }
                OptionDesc::ActivateAbility { source, ability } => {
                    let used = self.ability_uses.get(&(*source, *ability)).copied();
                    if used.unwrap_or(0) >= ABILITY_USES_PER_TURN {
                        continue;
                    }
                    match Self::find(v, *source) {
                        Some(ov) => self.spell_score(v, ov.card, Some(*ability)) - 5,
                        None => continue,
                    }
                }
                _ => continue,
            };
            if s > 0 && best.is_none_or(|(b, _)| s > b) {
                best = Some((s, i));
            }
        }
        match best {
            Some((_, i)) => {
                if let OptionDesc::ActivateAbility { source, ability } = &d.options[i] {
                    *self.ability_uses.entry((*source, *ability)).or_insert(0) += 1;
                }
                Response::Pick(vec![i])
            }
            None => Response::Pick(vec![
                d.options
                    .iter()
                    .position(|o| matches!(o, OptionDesc::Pass))
                    .unwrap_or(0),
            ]),
        }
    }

    // --- Combat -----------------------------------------------------------------------------

    fn plan_attacks(&mut self, v: &PlayerView) {
        let me = Self::me(v);
        self.attack_plan.clear();
        let opps = Self::opponents(v);
        let mut mine: Vec<&ObjectView> = Self::creatures_of(v, me)
            .filter(|o| !o.tapped && (!o.summoning_sick || has(o, Keyword::Haste)))
            .collect();
        mine.sort_by_key(|o| -value(o));
        // Crackback: keep a blocker home if opponents' total power threatens our life.
        let opp_power: i64 = opps
            .iter()
            .flat_map(|q| Self::creatures_of(v, *q))
            .map(power)
            .sum();
        let keep_home = Self::life(v, me) <= opp_power;
        let mut kept = false;
        for a in mine {
            let mut best: Option<(i64, PlayerId)> = None;
            for &q in &opps {
                let blockers: Vec<&ObjectView> =
                    Self::creatures_of(v, q).filter(|b| !b.tapped).collect();
                let can_block = |b: &&ObjectView| {
                    !has(a, Keyword::Flying) || has(b, Keyword::Flying) || has(b, Keyword::Reach)
                };
                let able: Vec<&&ObjectView> = blockers.iter().filter(|b| can_block(b)).collect();
                let menace_safe = has(a, Keyword::Menace) && able.len() < 2;
                let killers = able
                    .iter()
                    .filter(|b| power(b) >= remaining_toughness(a) || has(b, Keyword::Deathtouch))
                    .count();
                let safe = able.is_empty() || menace_safe || killers == 0;
                if !safe {
                    continue;
                }
                let mut score = Self::threat(v, q) + power(a) * 10;
                if Self::life(v, q) <= power(a) {
                    score += 10_000;
                }
                if a.is_commander && Self::commander_damage_taken(v, q, a) + power(a) >= 21 {
                    score += 10_000;
                }
                if best.is_none_or(|(b, _)| score > b) {
                    best = Some((score, q));
                }
            }
            if let Some((_, q)) = best {
                if keep_home && !kept && !has(a, Keyword::Vigilance) {
                    kept = true; // the most valuable creature stays home as a blocker
                    continue;
                }
                self.attack_plan.insert(a.id, q);
            }
        }
    }

    fn declare_attacker(&mut self, v: &PlayerView, d: &PendingDecision) -> Response {
        if self.attack_key != Some(v.turn_number) {
            self.attack_key = Some(v.turn_number);
            self.plan_attacks(v);
        }
        let pick = d.options.iter().position(|o| {
            matches!(o, OptionDesc::Attack { attacker, defender }
                if self.attack_plan.get(attacker) == Some(defender))
        });
        Response::Pick(pick.into_iter().collect())
    }

    /// Plan every block for this combat at once (blocks are asked one creature at a time).
    fn plan_blocks(&mut self, v: &PlayerView) {
        let me = Self::me(v);
        self.block_plan.clear();
        let Some(combat) = &v.combat else { return };
        let mut attackers: Vec<&ObjectView> = combat
            .attackers
            .iter()
            .filter(|a| a.defender == me)
            .filter_map(|a| Self::find(v, a.attacker))
            .collect();
        attackers.sort_by_key(|a| -power(a));
        let mut free: Vec<&ObjectView> = Self::creatures_of(v, me).filter(|b| !b.tapped).collect();
        free.sort_by_key(|b| value(b));
        let can_block = |b: &ObjectView, a: &ObjectView| {
            !has(a, Keyword::Flying) || has(b, Keyword::Flying) || has(b, Keyword::Reach)
        };
        let mut blocked: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();
        // How much damage gets through if `a` is blocked by blockers with toughness `soak`.
        let through = |a: &ObjectView, soak: i64| -> i64 {
            if has(a, Keyword::Trample) {
                (power(a) - soak).max(0)
            } else {
                0
            }
        };
        let take = |a: &ObjectView,
                    free: &mut Vec<&ObjectView>,
                    blocked: &mut BTreeMap<ObjectId, Vec<ObjectId>>,
                    prefer_survivor: bool|
         -> bool {
            let need = if has(a, Keyword::Menace) { 2 } else { 1 };
            let mut chosen: Vec<usize> = Vec::new();
            let order: Vec<usize> = if prefer_survivor {
                // Survivors first, then killers, then the cheapest.
                let mut idx: Vec<usize> = (0..free.len()).collect();
                idx.sort_by_key(|&i| {
                    let b = free[i];
                    let survives =
                        remaining_toughness(b) > power(a) && !has(a, Keyword::Deathtouch);
                    (!survives, power(b) < remaining_toughness(a), value(b))
                });
                idx
            } else {
                (0..free.len()).collect()
            };
            for i in order {
                if can_block(free[i], a) {
                    chosen.push(i);
                    if chosen.len() == need {
                        break;
                    }
                }
            }
            if chosen.len() < need {
                return false;
            }
            chosen.sort_unstable();
            for i in chosen.into_iter().rev() {
                let b = free.remove(i);
                blocked.entry(a.id).or_default().push(b.id);
            }
            true
        };
        // 1. Lethal commander damage (CR 903.10a): block it whatever it costs.
        for a in attackers.iter().filter(|a| a.is_commander) {
            if Self::commander_damage_taken(v, me, a) + power(a) >= 21 {
                take(a, &mut free, &mut blocked, true);
            }
        }
        // 2. Lethal damage to life: chump the biggest unblocked attackers until we survive.
        let incoming = |blocked: &BTreeMap<ObjectId, Vec<ObjectId>>| -> i64 {
            attackers
                .iter()
                .map(|a| match blocked.get(&a.id) {
                    None => power(a),
                    Some(bs) => {
                        let soak: i64 = bs
                            .iter()
                            .filter_map(|b| Self::find(v, *b))
                            .map(remaining_toughness)
                            .sum();
                        through(a, soak)
                    }
                })
                .sum()
        };
        for a in &attackers {
            if incoming(&blocked) < Self::life(v, me) {
                break;
            }
            if !blocked.contains_key(&a.id) {
                take(a, &mut free, &mut blocked, true);
            }
        }
        // 3. Value blocks: a blocker that survives, or trades with something worth more.
        for a in &attackers {
            if blocked.contains_key(&a.id) || has(a, Keyword::Menace) {
                continue;
            }
            if let Some(i) = free.iter().position(|b| {
                let survives = remaining_toughness(b) > power(a) && !has(a, Keyword::Deathtouch);
                let kills = power(b) >= remaining_toughness(a) || has(b, Keyword::Deathtouch);
                can_block(b, a) && (survives || (kills && value(b) < value(a)))
            }) {
                let b = free.remove(i);
                blocked.entry(a.id).or_default().push(b.id);
            }
        }
        for (a, bs) in blocked {
            for b in bs {
                self.block_plan.insert(b, a);
            }
        }
    }

    fn declare_blocker(&mut self, v: &PlayerView, d: &PendingDecision) -> Response {
        let key: Vec<ObjectId> = v
            .combat
            .as_ref()
            .map(|c| c.attackers.iter().map(|a| a.attacker).collect())
            .unwrap_or_default();
        if self.block_key.as_ref() != Some(&(v.turn_number, key.clone())) {
            self.block_key = Some((v.turn_number, key));
            self.block_asked.clear();
            self.plan_blocks(v);
        }
        // The plan doesn't know every restriction on blocking; the engine does. If the
        // declaration came back as illegal, declare no blocks: that is always legal, and
        // repeating the plan would never end.
        if let Some(OptionDesc::Block { blocker, .. }) = d.options.first() {
            if self.block_asked.contains(blocker) {
                self.block_plan.clear();
            } else {
                self.block_asked.push(*blocker);
            }
        }
        let pick = d.options.iter().position(|o| {
            matches!(o, OptionDesc::Block { blocker, attacker }
                if self.block_plan.get(blocker) == Some(attacker))
        });
        Response::Pick(pick.into_iter().collect())
    }

    // --- Generic choices --------------------------------------------------------------------

    /// Pick exactly `n` options with the lowest `keep` value (discard, sacrifice, bottom).
    fn pick_worst(
        &self,
        v: &PlayerView,
        d: &PendingDecision,
        n: usize,
        keep: impl Fn(&ObjectView) -> i64,
    ) -> Response {
        let mut idx: Vec<(i64, usize)> = d
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| match o {
                OptionDesc::Card { card } => (Self::find(v, *card).map_or(0, &keep), i),
                _ => (0, i),
            })
            .collect();
        idx.sort();
        let mut picks: Vec<usize> = idx.into_iter().take(n).map(|(_, i)| i).collect();
        picks.sort_unstable();
        Response::Pick(picks)
    }

    fn hand_keep_value(v: &PlayerView, o: &ObjectView) -> i64 {
        let lands_out = v
            .battlefield
            .iter()
            .filter(|b| b.controller == v.viewer && b.card_types.contains(&CardType::Land))
            .count() as i64;
        if o.card_types.contains(&CardType::Land) {
            if lands_out >= 5 {
                0
            } else {
                60 - lands_out * 5
            }
        } else {
            40 - i64::from(o.mana_value) + if o.is_commander { 100 } else { 0 }
        }
    }

    fn choose_targets(&mut self, v: &PlayerView, d: &PendingDecision, slot: u8) -> Response {
        let body = self.top_body(v);
        let mode = self.last_modes.get(slot as usize).copied().unwrap_or(0);
        let (pol, dmg) = match &body {
            Some(b) => (Self::mode_polarity(b, mode), Self::damage_of(b, mode)),
            None => (Polarity::Harm, 0),
        };
        let best = d
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| match o {
                OptionDesc::Target { target } => (Self::target_score(v, target, pol, dmg), i),
                _ => (-1_000_000, i),
            })
            .max_by_key(|(s, i)| (*s, core::cmp::Reverse(*i)))
            .map_or(0, |(_, i)| i);
        Response::Pick(vec![best])
    }

    fn choose_modes(&mut self, v: &PlayerView, d: &PendingDecision, min: usize) -> Response {
        let body = self.top_body(v);
        let mut scored: Vec<(i64, usize)> = d
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let OptionDesc::Mode { index } = o else {
                    return (0, i);
                };
                let Some(b) = &body else { return (0, i) };
                let m = &b.modes()[*index as usize];
                let pol = effect_polarity(&m.effect);
                let dmg = Self::damage_of(b, *index);
                let s = if m.targets.is_empty() {
                    6
                } else {
                    let cands: Vec<Target> = v
                        .battlefield
                        .iter()
                        .map(|o| Target::Object(o.id))
                        .chain(
                            v.players
                                .iter()
                                .filter(|p| !p.has_lost)
                                .map(|p| Target::Player(p.id)),
                        )
                        .collect();
                    cands
                        .iter()
                        .map(|t| Self::target_score(v, t, pol, dmg))
                        .max()
                        .unwrap_or(-1)
                };
                (s, i)
            })
            .collect();
        scored.sort_by_key(|(s, i)| (-*s, *i));
        let mut picks: Vec<usize> = scored.iter().take(min.max(1)).map(|(_, i)| *i).collect();
        picks.sort_unstable();
        self.last_modes = picks
            .iter()
            .filter_map(|&i| match d.options[i] {
                OptionDesc::Mode { index } => Some(index),
                _ => None,
            })
            .collect();
        Response::Pick(picks)
    }

    fn allocate_damage(
        &self,
        v: &PlayerView,
        d: &PendingDecision,
        total: u32,
        per_min: &[u32],
        per_max: &[u32],
    ) -> Response {
        let mut alloc: Vec<u32> = per_min.to_vec();
        let mut left = total - per_min.iter().sum::<u32>();
        // Kill the most valuable recipients first.
        let mut order: Vec<usize> = (0..d.options.len()).collect();
        order.sort_by_key(|&i| match &d.options[i] {
            OptionDesc::DamageRecipient { object } => -Self::find(v, *object).map_or(0, value),
            _ => 0,
        });
        for &i in &order {
            if let OptionDesc::DamageRecipient { object } = &d.options[i] {
                let need = Self::find(v, *object).map_or(0, remaining_toughness) as u32;
                let add = need
                    .saturating_sub(alloc[i])
                    .min(left)
                    .min(per_max[i] - alloc[i]);
                alloc[i] += add;
                left -= add;
            }
        }
        for &i in &order {
            let add = left.min(per_max[i] - alloc[i]);
            alloc[i] += add;
            left -= add;
        }
        Response::Allocate(alloc)
    }
}

impl Agent for HeuristicAgent {
    fn decide(&mut self, v: &PlayerView, d: &PendingDecision) -> Response {
        let me = Self::me(v);
        let r = match (d.kind, &d.prim) {
            (ChoiceKind::ChooseStartingPlayer, _) => Response::Pick(vec![
                d.options
                    .iter()
                    .position(|o| matches!(o, OptionDesc::Player { player } if *player == me))
                    .unwrap_or(0),
            ]),
            (ChoiceKind::Mulligan, _) => {
                let lands = v
                    .hand
                    .iter()
                    .filter(|o| o.card_types.contains(&CardType::Land))
                    .count();
                let keep = (2..=5).contains(&lands) || v.hand.len() <= 5;
                let want = if keep {
                    OptionDesc::Keep
                } else {
                    OptionDesc::Mulligan
                };
                Response::Pick(vec![d.options.iter().position(|o| *o == want).unwrap_or(0)])
            }
            (ChoiceKind::MulliganBottom | ChoiceKind::Discard, Primitive::PickN { min, .. }) => {
                self.pick_worst(v, d, *min, |o| Self::hand_keep_value(v, o))
            }
            (ChoiceKind::Sacrifice, Primitive::PickN { min, .. }) => {
                self.pick_worst(v, d, *min, value)
            }
            // A search, or cards to put somewhere from hand: as many as allowed, the most
            // expensive first.
            (ChoiceKind::SearchLibrary | ChoiceKind::ChooseCards, Primitive::PickN { max, .. }) => {
                let mut ranked: Vec<(i64, usize)> = d
                    .options
                    .iter()
                    .enumerate()
                    .map(|(i, o)| match o {
                        OptionDesc::Card { card } => (
                            Self::find(v, *card).map_or(0, |c| i64::from(c.mana_value)),
                            i,
                        ),
                        _ => (0, i),
                    })
                    .collect();
                ranked.sort_by_key(|(v, i)| (std::cmp::Reverse(*v), *i));
                let mut picks: Vec<usize> = ranked.iter().take(*max).map(|(_, i)| *i).collect();
                picks.sort_unstable();
                Response::Pick(picks)
            }
            // Scry and surveil: keep what is there (the least a player must do).
            (
                ChoiceKind::ScryBottom | ChoiceKind::SurveilGraveyard,
                Primitive::PickN { min, .. },
            ) => Response::Pick((0..*min).collect()),
            (ChoiceKind::LegendRule, _) => {
                let best = d
                    .options
                    .iter()
                    .enumerate()
                    .max_by_key(|(i, o)| match o {
                        OptionDesc::Card { card } => (Self::find(v, *card).map_or(0, value), *i),
                        _ => (0, *i),
                    })
                    .map_or(0, |(i, _)| i);
                Response::Pick(vec![best])
            }
            (ChoiceKind::Priority, _) => self.priority(v, d),
            (ChoiceKind::DeclareAttackers, _) => self.declare_attacker(v, d),
            (ChoiceKind::DeclareBlockers, _) => self.declare_blocker(v, d),
            (ChoiceKind::ChooseModes, Primitive::PickN { min, .. }) => {
                self.choose_modes(v, d, *min)
            }
            (ChoiceKind::ChooseTargets { slot }, _) => self.choose_targets(v, d, slot),
            (ChoiceKind::ChooseX, Primitive::PickNumber { max, .. }) => Response::Number(*max),
            // Trample: as much as allowed to the player (lethal to blockers is required first).
            (ChoiceKind::AssignCombatDamage, Primitive::PickNumber { max, .. }) => {
                Response::Number(*max)
            }
            (
                ChoiceKind::AssignCombatDamage,
                Primitive::Allocate {
                    total,
                    per_min,
                    per_max,
                },
            ) => self.allocate_damage(v, d, *total, per_min, per_max),
            // Prefer keeping the commander; otherwise the first offered effect.
            (ChoiceKind::ChooseReplacement | ChoiceKind::CommanderToCommandZone, _) => {
                Response::Pick(vec![
                    d.options
                        .iter()
                        .position(|o| *o == OptionDesc::Accept)
                        .unwrap_or(0),
                ])
            }
            (ChoiceKind::OrderTriggers, Primitive::Order)
            | (ChoiceKind::MulliganBottom, Primitive::Order) => {
                Response::Order((0..d.options.len()).collect())
            }
            (ChoiceKind::PayMana, Primitive::Allocate { total, per_max, .. }) => {
                // Spend the most plentiful floating mana on generic costs.
                let mut alloc = vec![0u32; per_max.len()];
                let mut left = *total;
                let mut order: Vec<usize> = (0..per_max.len()).collect();
                order.sort_by_key(|&i| core::cmp::Reverse(per_max[i]));
                for i in order {
                    let add = left.min(per_max[i]);
                    alloc[i] += add;
                    left -= add;
                }
                Response::Allocate(alloc)
            }
            (ChoiceKind::PayMana, Primitive::PickN { .. }) => {
                // The cheapest mana first: tap what stays before sacrificing what doesn't
                // (Treasure), before paying mana for mana, taking damage or paying life,
                // and sacrifice another permanent last (D-066).
                let price = |o: &OptionDesc| match o {
                    OptionDesc::TapForMana {
                        ability: Some(a), ..
                    } => match self.catalog.ability(*a) {
                        AbilityDef::Mana { cost, then, .. } => {
                            u32::from(cost.sacrifice_self) * 2
                                + u32::from(cost.mana.is_some())
                                + u32::from(then.is_some()) * 3
                                + cost.life * 4
                                + u32::from(cost.sacrifice.is_some()) * 20
                        }
                        _ => 0,
                    },
                    _ => 0,
                };
                let best = (0..d.options.len())
                    .min_by_key(|&i| (price(&d.options[i]), i))
                    .unwrap_or(0);
                Response::Pick(vec![best])
            }
            // Paying is nearly always right: it is offered only when it can be afforded,
            // and it buys the effect or stops the harm.
            (ChoiceKind::PayUnlessCost, _) => Response::Pick(vec![
                d.options
                    .iter()
                    .position(|o| matches!(o, OptionDesc::Accept))
                    .unwrap_or(0),
            ]),
            _ => return self.fallback.decide(v, d),
        };
        if d.validate(&r).is_ok() {
            r
        } else {
            self.fallback.decide(v, d)
        }
    }
}
