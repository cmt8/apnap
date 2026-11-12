//! Mana abilities (CR 605) beyond "{T}: Add one mana" (D-066): the ways a player can make
//! mana now, whether a cost can be paid with them (the planner), what to offer while
//! paying, and activating one. Text: docs/rules-notes/605-mana-abilities-costs-and-yields.md.
//!
//! **The planner** answers "can `p` pay this cost, with what is in their pool and the mana
//! abilities they could activate?" (D-019: nothing is offered that can't be completed).
//! It works on a snapshot of the mana abilities ([`ManaWay`]) and is exact for the ways
//! the engine knows, under one conservative rule (D-066): the mana *cost* of a mana
//! ability (a Signet's `{1}`) is paid from the pool or from abilities that cost no mana —
//! never from the mana another mana-costing ability makes (a chain of Signets each paying
//! for the next is not found). Everything is one bipartite matching between units of mana
//! wanted (each symbol of the cost, each symbol of the chosen abilities' costs) and units
//! of mana that could be had; which mana-costing or sacrificing abilities to use is
//! enumerated around it.
//!
//! **Offering choices.** While paying, an activation (and each choice inside one: what to
//! sacrifice, which floating mana pays its own cost, the type of each mana it makes) is
//! offered only if the cost stays payable afterwards. For a plain tap the planner's
//! snapshot is updated directly; for anything that sacrifices, costs mana or life, or makes
//! mana of several types, the activation is tried on a copy of the game and the snapshot
//! is rebuilt from it — exactly what the next step will see.

use super::cast::{
    Payee, can_tap_for_ability, identity_mask, may_spend, sacrifice_candidates, tap_candidates,
    usable_pool,
};
use super::resolve::ResolveFrame;
use super::{ApplyFrame, Flow, Frame, Input};
use crate::actions::{GameAction, execute, move_object};
use crate::decision::{ChoiceKind, OptionDesc, Primitive};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::{abilities, chars, controller_of};
use crate::mana::{ManaMask, ManaPool, ManaRequirement, ManaType, mask_of, types_in};
use crate::replacement::{MoveCause, Proposed};
use crate::state::GameState;
use crate::triggers;
use crate::zones::{LibraryPosition, Zone};
use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;
use card_model::{AbilityDef, AbilityId, CardType, Cost, CostChoice, Effect, ManaProduce};
use serde::{Deserialize, Serialize};

/// Every mana type.
const ALL: ManaMask = 0b11_1111;
/// A unit that pays only generic mana: a creature or artifact tapped for convoke or
/// improvise pays `{1}` (or a mana of its color), never `{C}` (CR 702.51a, 702.126a).
/// Every generic unit wanted accepts it.
const GENERIC: ManaMask = 1 << 6;

/// What one activation makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Yield {
    /// `count` mana, all of ONE type chosen from `mask`.
    One { mask: ManaMask, count: u32 },
    /// One mana per entry, each of a type chosen from its own mask ("{U}{B}", "two mana
    /// in any combination of colors").
    Units(Vec<ManaMask>),
}

impl Yield {
    fn count(&self) -> u32 {
        match self {
            Yield::One { count, .. } => *count,
            Yield::Units(v) => v.len() as u32,
        }
    }

    /// The choices for the first mana it makes (what `OptionDesc::TapForMana::mana` says).
    fn first(&self) -> ManaMask {
        match self {
            Yield::One { mask, .. } => *mask,
            Yield::Units(v) => v.first().copied().unwrap_or(0),
        }
    }
}

/// One way to make mana now: one mana ability of one permanent. A permanent's plain
/// "{T}: Add one mana" abilities (including what its basic land types give it) are merged
/// into one way with `ability: None`; ways with the same `id` follow each other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManaWay {
    pub id: ObjectId,
    pub ability: Option<AbilityId>,
    /// `{T}` in its cost: tapping the permanent for one way excludes the others.
    pub tap: bool,
    /// "Sacrifice this" in its cost (Treasure, CR 111.10a).
    pub sac_self: bool,
    /// The mana in its cost (a Signet's `{1}`).
    pub cost: ManaRequirement,
    /// Life paid as part of its cost (CR 119.4).
    pub life: u32,
    /// Damage and life loss its effect does to its controller (a pain land's 1 damage).
    pub risk: u32,
    /// "Sacrifice a creature" in its cost: how many, and what could be chosen.
    pub sac: Option<(u32, Vec<ObjectId>)>,
    pub yields: Yield,
    /// Its mana can only be spent in certain ways (CR 106.6).
    pub restricted: bool,
    /// Whether it can be activated, or how much it makes, depends on other permanents
    /// ("activate only if", "for each", "could produce").
    pub dependent: bool,
    /// Nothing but `{T}` (and maybe "sacrifice this") for mana of one chosen type: done
    /// in one step (`tap_for_mana`), as before D-066.
    pub simple: bool,
    /// Not a mana ability: tapping the creature or artifact pays one mana of the spell's
    /// cost itself (convoke, improvise: CR 702.51a, 702.126a). Offered as
    /// `OptionDesc::TapToPay`.
    pub pays: bool,
}

impl ManaWay {
    /// It costs mana or a chosen sacrifice: one of the abilities the planner chooses
    /// whether to use.
    fn complex(&self) -> bool {
        self.cost.total() > 0 || self.sac.is_some()
    }

    /// The most activations of it that could be used in one payment.
    fn max_uses(&self) -> u32 {
        match &self.sac {
            Some((n, cands)) if !self.tap && !self.sac_self => (cands.len() as u32) / (*n).max(1),
            _ => 1,
        }
    }
}

/// The plain part of a permanent's mana: what each basic land type and each "{T}: Add one
/// mana" ability gives it.
fn plain_mask(st: &GameState, id: ObjectId) -> ManaMask {
    let c = chars(st, id);
    let mut plain: ManaMask = 0;
    if c.is(CardType::Land) && !c.land_abilities_lost {
        // CR 305.6: each basic land type grants "{T}: Add [mana]".
        for t in &c.land_types {
            plain |= 1 << ManaType::from_color(t.produces()).index();
        }
    }
    plain
}

/// CR 106.7: the types of mana the permanents matching `from` (for `p`) could produce.
/// Abilities that themselves reflect are followed, each permanent once.
fn reflected_mask(
    st: &GameState,
    p: PlayerId,
    from: &card_model::Selector,
    source: ObjectId,
    seen: &mut BTreeSet<ObjectId>,
) -> ManaMask {
    let mut mask: ManaMask = 0;
    for id in crate::layers::matching(st, from, source, p) {
        if !seen.insert(id) {
            continue;
        }
        mask |= plain_mask(st, id);
        let controller = controller_of(st, id);
        for (_, a) in abilities(st, id) {
            if let AbilityDef::Mana { produce, .. } = a {
                mask |= match produce {
                    ManaProduce::CommanderIdentity => identity_mask(st, controller),
                    ManaProduce::Reflected { from, types } => {
                        reflected_mask(st, controller, from, id, seen)
                            & if *types { ALL } else { ALL & !colorless() }
                    }
                    ManaProduce::Counted { of, amount } => {
                        if crate::layers::count_amount(st, amount, id, controller) > 0 {
                            mask_of(of)
                        } else {
                            0
                        }
                    }
                    other => mask_of(&other.options()),
                };
            }
        }
    }
    mask
}

fn colorless() -> ManaMask {
    1 << ManaType::Colorless.index()
}

/// What `produce` of ability on `id` makes now, for `p`.
pub(crate) fn yields_now(
    st: &GameState,
    p: PlayerId,
    id: ObjectId,
    produce: &ManaProduce,
) -> Yield {
    match produce {
        ManaProduce::CommanderIdentity => Yield::One {
            mask: identity_mask(st, p),
            count: 1,
        },
        ManaProduce::Fixed(v) => Yield::Units(v.iter().map(|t| 1 << t.index()).collect()),
        ManaProduce::Combination { of, count } => Yield::Units(vec![mask_of(of); *count as usize]),
        ManaProduce::Counted { of, amount } => Yield::One {
            mask: mask_of(of),
            count: crate::layers::count_amount(st, amount, id, p),
        },
        ManaProduce::Reflected { from, types } => {
            let mut seen = BTreeSet::new();
            seen.insert(id);
            let mask = reflected_mask(st, p, from, id, &mut seen);
            Yield::One {
                mask: if *types { mask } else { mask & !colorless() },
                count: 1,
            }
        }
        other => Yield::One {
            mask: mask_of(&other.options()),
            count: other.count(),
        },
    }
}

/// The damage and life loss to its controller that a mana ability's effect does.
fn risk_of(e: &Effect) -> u32 {
    use card_model::{Amount, PlayerRef, TargetRef};
    match e {
        Effect::Seq(v) => v.iter().map(risk_of).sum(),
        Effect::DealDamage {
            to: TargetRef::Players(PlayerRef::You),
            amount: Amount::Fixed(n),
            ..
        }
        | Effect::LoseLife {
            who: PlayerRef::You,
            amount: Amount::Fixed(n),
        } => *n,
        _ => 0,
    }
}

/// The mana abilities `p` could activate now, in battlefield order (CR 605.3a): the
/// permanent can be tapped if the ability taps it, "activate only if" holds, life and
/// sacrifices can be paid. (Whether its mana cost can be paid is the planner's question.)
/// With a `payee`, abilities whose mana couldn't be spent on it (CR 106.6) are left out.
/// `exclude` is a permanent that is tapped for the cost being paid.
pub(crate) fn mana_ways(
    st: &GameState,
    p: PlayerId,
    exclude: Option<ObjectId>,
    payee: Option<Payee>,
) -> Vec<ManaWay> {
    let mut out = Vec::new();
    let life = st.player(p).life;
    for &id in &st.zones.battlefield {
        if Some(id) == exclude || controller_of(st, id) != p {
            continue;
        }
        let can_tap = can_tap_for_ability(st, id);
        let start = out.len();
        let mut plain: ManaMask = if can_tap { plain_mask(st, id) } else { 0 };
        for (index, a) in abilities(st, id) {
            let AbilityDef::Mana {
                cost,
                produce,
                only_if,
                spend,
                then,
            } = a
            else {
                continue;
            };
            if (cost.tap && !can_tap) || i64::from(cost.life) > i64::from(life) {
                continue;
            }
            // "Activate only if …" (CR 602.5b).
            if !triggers::condition_holds(st, only_if.as_ref(), p, id) {
                continue;
            }
            if let (Some(r), Some(payee)) = (spend, payee)
                && !may_spend(st, p, r, &st.object(id).chosen, payee)
            {
                continue;
            }
            let sac = match &cost.sacrifice {
                Some(ch) => {
                    let cands = sacrifice_candidates(st, p, ch, id, None);
                    if cands.len() < ch.count as usize {
                        continue;
                    }
                    Some((ch.count, cands))
                }
                None => None,
            };
            let yields = yields_now(st, p, id, produce);
            // An activation that would make no mana now is not offered.
            if yields.first() == 0 || yields.count() == 0 {
                continue;
            }
            let cost_mana = cost
                .mana
                .as_ref()
                .map(|m| ManaRequirement::from_cost(m, 0))
                .unwrap_or_default();
            let simple = cost.tap
                && cost_mana.total() == 0
                && cost.life == 0
                && sac.is_none()
                && then.is_none()
                && matches!(yields, Yield::One { .. });
            if simple
                && !cost.sacrifice_self
                && spend.is_none()
                && matches!(yields, Yield::One { count: 1, .. })
                && matches!(
                    produce,
                    ManaProduce::One(_)
                        | ManaProduce::AnyColor
                        | ManaProduce::OneOf(_)
                        | ManaProduce::CommanderIdentity
                )
            {
                plain |= yields.first();
                continue;
            }
            out.push(ManaWay {
                id,
                ability: Some(index),
                tap: cost.tap,
                sac_self: cost.sacrifice_self,
                cost: cost_mana,
                life: cost.life,
                risk: then.as_ref().map_or(0, risk_of),
                sac,
                yields,
                restricted: spend.is_some(),
                dependent: only_if.is_some()
                    || matches!(
                        produce,
                        ManaProduce::Counted { .. } | ManaProduce::Reflected { .. }
                    ),
                simple,
                pays: false,
            });
        }
        if plain != 0 {
            out.insert(
                start,
                ManaWay {
                    id,
                    ability: None,
                    tap: true,
                    sac_self: false,
                    cost: ManaRequirement::default(),
                    life: 0,
                    risk: 0,
                    sac: None,
                    yields: Yield::One {
                        mask: plain,
                        count: 1,
                    },
                    restricted: false,
                    dependent: false,
                    simple: true,
                    pays: false,
                },
            );
        }
    }
    out
}

// ---------------------------------------------------------------------- the planner

/// Everything the planner needs: the cost and what could pay it.
#[derive(Clone, Debug)]
pub(crate) struct PlanInput {
    /// The mana to pay.
    pub req: ManaRequirement,
    /// Unrestricted mana in the pool: it can pay anything.
    pub pool: ManaPool,
    /// Restricted mana in the pool that may pay `req` (and nothing else here).
    pub restricted: ManaPool,
    /// Mana about to be made whose type is still to be chosen, as units that could be had.
    pub floating: Vec<Have>,
    pub ways: Vec<ManaWay>,
    pub life: i64,
    /// Life the cost itself asks for.
    pub life_cost: u32,
    /// "Sacrifice N [permanents]" in the cost itself: how many, and the candidates.
    pub sac: Option<(u32, Vec<ObjectId>)>,
    /// "Tap N untapped [permanents] you control" in the cost itself: how many, and the
    /// candidates (none of them can be tapped for mana as well).
    pub tap: Option<(u32, Vec<ObjectId>)>,
}

/// A unit of mana wanted: its acceptable types, and whether it is part of a mana
/// ability's cost (which only mana that costs no mana to make can pay).
type Want = (ManaMask, bool);
/// A unit of mana that could be had: the types it can be when paying the cost, and when
/// paying a mana ability's cost.
type Have = (ManaMask, ManaMask);

/// Is there a matching that gives every wanted unit its own unit of mana?
fn matching(want: &[Want], have: &[Have]) -> bool {
    if want.len() > have.len() {
        return false;
    }
    fn augment(
        u: usize,
        want: &[Want],
        have: &[Have],
        owner: &mut [Option<usize>],
        seen: &mut [bool],
    ) -> bool {
        let (mask, for_cost) = want[u];
        for s in 0..have.len() {
            let fits = if for_cost { have[s].1 } else { have[s].0 };
            if fits & mask == 0 || seen[s] {
                continue;
            }
            seen[s] = true;
            if owner[s].is_none_or(|v| augment(v, want, have, owner, seen)) {
                owner[s] = Some(u);
                return true;
            }
        }
        false
    }
    let mut owner = vec![None; have.len()];
    // Specific units first: they have fewer choices.
    let mut order: Vec<usize> = (0..want.len()).collect();
    order.sort_by_key(|&u| (want[u].0.count_ones(), u));
    for u in order {
        let mut seen = vec![false; have.len()];
        if !augment(u, want, have, &mut owner, &mut seen) {
            return false;
        }
    }
    true
}

fn wants(req: &ManaRequirement, for_cost: bool, times: u32, out: &mut Vec<Want>) {
    for _ in 0..times {
        for t in 0..6 {
            for _ in 0..req.specific[t] {
                out.push((1 << t, for_cost));
            }
        }
        for m in req.hybrid_masks() {
            out.push((m, for_cost));
        }
        for _ in 0..req.generic {
            out.push((ALL | GENERIC, for_cost));
        }
    }
}

/// How many multi-mana ways with a real choice of ONE type are tried with each type
/// (D-049); beyond that each counts as a single mana, which can only under-estimate.
const BRANCHING: usize = 4;
/// How many combinations of mana-costing and sacrificing abilities are tried.
const COMBINATIONS: u32 = 4096;

/// Interchangeable permanents that sacrifice themselves for mana: what each makes, and
/// which groups they are.
type Kind = (Vec<(Yield, bool)>, Vec<usize>);

/// The alternatives of one permanent's ways that cost nothing but tapping it (or
/// sacrificing it): each a list of (yield, restricted).
struct Group<'a> {
    ways: Vec<&'a ManaWay>,
}

impl Group<'_> {
    /// All alternatives make one mana: one unit with every type any of them could make.
    fn merged(&self) -> Option<Have> {
        let single = self
            .ways
            .iter()
            .all(|w| matches!(w.yields, Yield::One { count: 1, .. }) || w.yields.count() == 0);
        single.then(|| {
            self.ways.iter().fold((0, 0), |(a, b), w| {
                let m = w.yields.first();
                (a | m, if w.restricted { b } else { b | m })
            })
        })
    }

    fn max_units(&self) -> u32 {
        self.ways
            .iter()
            .map(|w| w.yields.count())
            .max()
            .unwrap_or(0)
    }
}

/// Can `inp.req` be paid (CR 601.2g–h)?
pub(crate) fn plan(inp: &PlanInput) -> bool {
    let mut ways: Vec<&ManaWay> = inp.ways.iter().collect();
    // CR 119.4: life paid can't be more than the player has. When the life the cost and
    // every life-paying ability ask for, plus all the damage mana abilities would deal
    // to the player, is more than their life, the life-paying abilities aren't used (and
    // if the cost itself pays life, neither are the ones that deal damage).
    let paying = inp.life_cost + ways.iter().map(|w| w.life).sum::<u32>();
    if paying > 0 {
        let risk: u32 = ways.iter().map(|w| w.risk).sum();
        if i64::from(paying + risk) > inp.life {
            ways.retain(|w| w.life == 0);
            let risk: u32 = ways.iter().map(|w| w.risk).sum();
            if inp.life_cost > 0 && i64::from(inp.life_cost + risk) > inp.life {
                ways.retain(|w| w.risk == 0);
            }
        }
        if i64::from(inp.life_cost) > inp.life {
            return false;
        }
    }
    let total = inp.req.total();
    // An upper bound on the mana there could be.
    let mut most = u64::from(inp.pool.total() + inp.restricted.total()) + inp.floating.len() as u64;
    for w in &ways {
        let net = w.yields.count().saturating_sub(w.cost.total());
        most += u64::from(net) * u64::from(w.max_uses());
    }
    if most < u64::from(total) {
        return false;
    }
    let complex: Vec<&ManaWay> = ways.iter().copied().filter(|w| w.complex()).collect();
    // What abilities may sacrifice (D-066): not what sacrifices itself for mana, not a
    // permanent with a mana-costing or sacrificing ability (the order in which they are
    // used could matter), and not what the cost itself may sacrifice if some of that
    // sacrifices itself for mana (Treasures: how many of those are used is chosen below).
    let mut banned: BTreeSet<ObjectId> = BTreeSet::new();
    if let Some((_, cands)) = &inp.sac
        && ways.iter().any(|w| w.sac_self && cands.contains(&w.id))
    {
        banned.extend(cands.iter().copied());
    }
    for w in &ways {
        if w.sac_self || w.complex() {
            banned.insert(w.id);
        }
    }
    let ctx = Ctx {
        inp,
        ways: &ways,
        complex: &complex,
        banned: &banned,
    };
    let mut budget = COMBINATIONS;
    let mut uses = vec![0u32; complex.len()];
    ctx.combos(0, &mut uses, &mut budget)
}

struct Ctx<'a> {
    inp: &'a PlanInput,
    ways: &'a [&'a ManaWay],
    complex: &'a [&'a ManaWay],
    banned: &'a BTreeSet<ObjectId>,
}

impl Ctx<'_> {
    /// Try each number of uses of each mana-costing or sacrificing ability.
    fn combos(&self, i: usize, uses: &mut Vec<u32>, budget: &mut u32) -> bool {
        if i == self.complex.len() {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            return self.leaf(uses);
        }
        let w = self.complex[i];
        // One tap / one sacrifice of a permanent.
        let taken = (w.tap || w.sac_self)
            && self.complex[..i]
                .iter()
                .zip(uses.iter())
                .any(|(x, &k)| k > 0 && x.id == w.id && (x.tap || x.sac_self));
        let max = if taken { 0 } else { w.max_uses() };
        for k in 0..=max {
            uses[i] = k;
            if self.combos(i + 1, uses, budget) {
                return true;
            }
            if *budget == 0 {
                break;
            }
        }
        uses[i] = 0;
        false
    }

    fn leaf(&self, uses: &[u32]) -> bool {
        let inp = self.inp;
        // The chosen abilities' sacrifices: distinct permanents (a bipartite matching).
        let mut slots: Vec<Vec<ObjectId>> = Vec::new();
        for (w, &k) in self.complex.iter().zip(uses) {
            if let Some((n, cands)) = &w.sac {
                let allowed: Vec<ObjectId> = cands
                    .iter()
                    .copied()
                    .filter(|c| !self.banned.contains(c))
                    .collect();
                for _ in 0..k * n {
                    slots.push(allowed.clone());
                }
            }
        }
        // Permanents tapped or sacrificed by the chosen abilities.
        let gone: Vec<ObjectId> = self
            .complex
            .iter()
            .zip(uses)
            .filter(|(w, k)| **k > 0 && (w.tap || w.sac_self))
            .map(|(w, _)| w.id)
            .collect();
        // The cost's own taps and sacrifices need their permanents too (chosen after the
        // mana).
        for (n, cands) in inp.sac.iter().chain(inp.tap.iter()) {
            let left: Vec<ObjectId> = cands
                .iter()
                .copied()
                .filter(|c| !gone.contains(c))
                .collect();
            for _ in 0..*n {
                slots.push(left.clone());
            }
        }
        if !assign(&slots) {
            return false;
        }
        let mut want: Vec<Want> = Vec::new();
        wants(&inp.req, false, 1, &mut want);
        for (w, &k) in self.complex.iter().zip(uses) {
            wants(&w.cost, true, k, &mut want);
        }
        let mut have: Vec<Have> = Vec::new();
        for t in 0..6 {
            for _ in 0..inp.pool.amounts[t] {
                have.push((1 << t, 1 << t));
            }
            for _ in 0..inp.restricted.amounts[t] {
                have.push((1 << t, 0));
            }
        }
        have.extend(inp.floating.iter().copied());
        // What the chosen abilities make can pay the cost, not other abilities' costs.
        let mut choose: Vec<(ManaMask, u32)> = Vec::new();
        for (w, &k) in self.complex.iter().zip(uses) {
            for _ in 0..k {
                match &w.yields {
                    Yield::Units(v) => have.extend(v.iter().map(|&m| (m, 0))),
                    Yield::One { mask, count } if mask.count_ones() == 1 || *count == 1 => {
                        have.extend((0..*count).map(|_| (*mask, 0)))
                    }
                    Yield::One { mask, count } => choose.push((*mask, *count)),
                }
            }
        }
        // The cost's own sacrifice: permanents that sacrifice themselves for mana and could
        // be chosen for it are limited, so that enough remain.
        let mut limit_max = usize::MAX;
        let mut cost_cands: Vec<ObjectId> = Vec::new();
        // (A cost that both taps and sacrifices chosen permanents is refused by card-db.)
        let tapping = inp.tap.is_some();
        if let Some((n, cands)) = inp.tap.as_ref().or(inp.sac.as_ref()) {
            cost_cands = cands
                .iter()
                .copied()
                .filter(|c| !gone.contains(c))
                .collect();
            if cost_cands.len() < *n as usize {
                return false;
            }
            limit_max = cost_cands.len() - *n as usize;
        }
        let mut groups: Vec<Group> = Vec::new();
        let mut limited: Vec<Group> = Vec::new();
        let mut i = 0;
        while i < self.ways.len() {
            let id = self.ways[i].id;
            let run = self.ways[i..].iter().take_while(|w| w.id == id).count();
            let simple: Vec<&ManaWay> = self.ways[i..i + run]
                .iter()
                .copied()
                .filter(|w| !w.complex())
                .collect();
            i += run;
            if simple.is_empty() || gone.contains(&id) {
                continue;
            }
            let g = Group { ways: simple };
            // A permanent the cost may tap is used up by any way of making mana with it;
            // one the cost may sacrifice only by sacrificing it for mana.
            if cost_cands.contains(&id) && (tapping || g.ways.iter().any(|w| w.sac_self)) {
                limited.push(g);
            } else {
                groups.push(g);
            }
        }
        // Limited groups that make the same mana are interchangeable: choose how many of
        // each kind to use, most first.
        let mut kinds: Vec<Kind> = Vec::new();
        for (gi, g) in limited.iter().enumerate() {
            let sig: Vec<(Yield, bool)> = g
                .ways
                .iter()
                .map(|w| (w.yields.clone(), w.restricted))
                .collect();
            match kinds.iter_mut().find(|(s, _)| *s == sig) {
                Some((_, v)) => v.push(gi),
                None => kinds.push((sig, vec![gi])),
            }
        }
        let mut take = Vec::new();
        self.limited(
            &kinds, &limited, &groups, &mut take, limit_max, &want, &have, &choose,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn limited(
        &self,
        kinds: &[Kind],
        limited: &[Group],
        groups: &[Group],
        take: &mut Vec<usize>,
        left: usize,
        want: &[Want],
        have: &[Have],
        choose: &[(ManaMask, u32)],
    ) -> bool {
        if take.len() == kinds.len() {
            let mut all: Vec<&Group> = groups.iter().collect();
            for ((_, members), &n) in kinds.iter().zip(take.iter()) {
                all.extend(members.iter().take(n).map(|&gi| &limited[gi]));
            }
            let mut have = have.to_vec();
            let mut branching: Vec<&Group> = Vec::new();
            for g in all {
                match g.merged() {
                    Some(h) => have.push(h),
                    None => branching.push(g),
                }
            }
            if have.len()
                + branching
                    .iter()
                    .map(|g| g.max_units() as usize)
                    .sum::<usize>()
                + choose.iter().map(|c| c.1 as usize).sum::<usize>()
                < want.len()
            {
                return false;
            }
            return chosen_types(choose, &mut have, BRANCHING, &mut |have, branches| {
                alternatives(&branching, have, branches, want)
            });
        }
        let members = kinds[take.len()].1.len();
        for n in (0..=members.min(left)).rev() {
            take.push(n);
            let ok = self.limited(kinds, limited, groups, take, left - n, want, have, choose);
            take.pop();
            if ok {
                return true;
            }
        }
        false
    }
}

/// Each slot gets its own permanent from its candidates.
fn assign(slots: &[Vec<ObjectId>]) -> bool {
    let mut owner: Vec<(ObjectId, usize)> = Vec::new();
    fn augment(
        u: usize,
        slots: &[Vec<ObjectId>],
        owner: &mut Vec<(ObjectId, usize)>,
        seen: &mut BTreeSet<ObjectId>,
    ) -> bool {
        for &c in &slots[u] {
            if !seen.insert(c) {
                continue;
            }
            match owner.iter().position(|(o, _)| *o == c) {
                None => {
                    owner.push((c, u));
                    return true;
                }
                Some(at) => {
                    let v = owner[at].1;
                    if augment(v, slots, owner, seen) {
                        let at = owner
                            .iter()
                            .position(|(o, s)| *o == c && *s == v)
                            .expect("owned");
                        owner[at].1 = u;
                        return true;
                    }
                }
            }
        }
        false
    }
    for u in 0..slots.len() {
        let mut seen = BTreeSet::new();
        if !augment(u, slots, &mut owner, &mut seen) {
            return false;
        }
    }
    true
}

/// Mana of one chosen type from mana-costing abilities: try each type (within `branches`).
fn chosen_types(
    choose: &[(ManaMask, u32)],
    have: &mut Vec<Have>,
    branches: usize,
    then: &mut dyn FnMut(&mut Vec<Have>, usize) -> bool,
) -> bool {
    let Some(((mask, count), rest)) = choose.split_first() else {
        return then(have, branches);
    };
    if branches == 0 {
        have.push((*mask, 0));
        let ok = chosen_types(rest, have, 0, then);
        have.pop();
        return ok;
    }
    for t in types_in(*mask) {
        let n = have.len();
        have.extend((0..*count).map(|_| (1 << t.index(), 0)));
        let ok = chosen_types(rest, have, branches - 1, then);
        have.truncate(n);
        if ok {
            return true;
        }
    }
    false
}

/// Choose one way for each permanent with several that make more than one mana, and a
/// type for each that makes several of one chosen type; then match.
fn alternatives(groups: &[&Group], have: &mut Vec<Have>, branches: usize, want: &[Want]) -> bool {
    let Some((g, rest)) = groups.split_first() else {
        return matching(want, have);
    };
    g.ways.iter().any(|w| {
        let cost_mask = |m: ManaMask| if w.restricted { 0 } else { m };
        let n = have.len();
        let ok = match &w.yields {
            Yield::Units(v) => {
                have.extend(v.iter().map(|&m| (m, cost_mask(m))));
                alternatives(rest, have, branches, want)
            }
            Yield::One { mask, count }
                if *count <= 1 || mask.count_ones() == 1 || branches == 0 =>
            {
                // One unit per mana when the type is fixed; a choice beyond the branching
                // budget counts as a single mana (never more than it can really give).
                let k = if *count <= 1 || mask.count_ones() == 1 {
                    *count
                } else {
                    1
                };
                have.extend((0..k).map(|_| (*mask, cost_mask(*mask))));
                alternatives(rest, have, branches, want)
            }
            Yield::One { mask, count } => types_in(*mask).into_iter().any(|t| {
                let m = 1 << t.index();
                let n = have.len();
                have.extend((0..*count).map(|_| (m, cost_mask(m))));
                let ok = alternatives(rest, have, branches - 1, want);
                have.truncate(n);
                ok
            }),
        };
        have.truncate(n);
        ok
    })
}

// ---------------------------------------------------------------------- goals

/// What a payment is for: enough to rebuild the planner's input from any game state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayGoal {
    pub req: ManaRequirement,
    pub(crate) payee: Option<Payee>,
    /// The permanent tapped for the cost (`{T}`): not tapped for mana.
    pub tap: Option<ObjectId>,
    /// The permanent the cost sacrifices ("sacrifice this").
    pub sacrifice: Option<ObjectId>,
    /// "Sacrifice N [permanents]" in the cost, relative to `sac_source`.
    pub sac: Option<CostChoice>,
    pub sac_source: ObjectId,
    /// Life the cost asks for.
    pub life: u32,
    /// "Tap N untapped [permanents] you control" in the cost, relative to `sac_source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tap_cost: Option<CostChoice>,
    /// What may be tapped to pay the spell's mana (convoke, improvise).
    #[serde(default, skip_serializing_if = "TapPays::is_none")]
    pub tap_pays: TapPays,
}

/// CR 702.51, 702.126: the permanents a spell's mana may be paid with by tapping them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TapPays {
    /// Convoke: untapped creatures (each for `{1}` or a mana of its color).
    pub creatures: bool,
    /// Improvise: untapped artifacts (each for `{1}`).
    pub artifacts: bool,
}

impl TapPays {
    fn is_none(&self) -> bool {
        !self.creatures && !self.artifacts
    }
}

/// CR 702.51a, 702.126a: one unit for each untapped creature (convoke) or artifact
/// (improvise) `p` controls — not a mana ability, so a creature needn't have been under
/// their control since the turn began (CR 302.6) — next to that permanent's mana ways.
fn add_pays(st: &GameState, p: PlayerId, goal: &PayGoal, ways: &mut Vec<ManaWay>) {
    if goal.tap_pays.is_none() {
        return;
    }
    for &id in &st.zones.battlefield {
        if Some(id) == goal.tap || st.object(id).tapped || controller_of(st, id) != p {
            continue;
        }
        let c = chars(st, id);
        let creature = goal.tap_pays.creatures && c.is(CardType::Creature);
        let artifact = goal.tap_pays.artifacts && c.is(CardType::Artifact);
        if !creature && !artifact {
            continue;
        }
        let mut mask = GENERIC;
        if creature {
            for col in &c.colors {
                mask |= 1 << ManaType::from_color(*col).index();
            }
        }
        let way = ManaWay {
            id,
            ability: None,
            tap: true,
            sac_self: false,
            cost: ManaRequirement::default(),
            life: 0,
            risk: 0,
            sac: None,
            yields: Yield::One { mask, count: 1 },
            // It pays the spell, never a mana ability's cost.
            restricted: true,
            dependent: false,
            simple: true,
            pays: true,
        };
        match ways.iter().rposition(|w| w.id == id) {
            Some(i) => ways.insert(i + 1, way),
            None => ways.push(way),
        }
    }
}

/// The planner's input for `goal`, as the game is now.
pub(crate) fn plan_input(st: &GameState, p: PlayerId, goal: &PayGoal) -> PlanInput {
    let mut ways = mana_ways(st, p, goal.tap, goal.payee);
    add_pays(st, p, goal, &mut ways);
    // "Sacrifice this" in the cost being paid: it can't be sacrificed for mana as well,
    // nor be sacrificed by a mana ability; neither can the permanent the cost taps, nor
    // the source of what is being paid for (D-066, conservative: its last known
    // information would decide what restricted mana may pay for it).
    ways.retain(|w| !(w.sac_self && Some(w.id) == goal.sacrifice));
    for w in &mut ways {
        if let Some((_, cands)) = &mut w.sac {
            cands.retain(|c| {
                Some(*c) != goal.sacrifice && Some(*c) != goal.tap && *c != goal.sac_source
            });
        }
    }
    let pool = st.player(p).mana_pool;
    let usable = usable_pool(st, p, goal.payee);
    let mut restricted = ManaPool::default();
    for t in 0..6 {
        restricted.amounts[t] = usable.amounts[t] - pool.amounts[t];
    }
    PlanInput {
        req: goal.req,
        pool,
        restricted,
        floating: Vec::new(),
        ways,
        life: i64::from(st.player(p).life),
        life_cost: goal.life,
        sac: goal.sac.as_ref().map(|ch| {
            (
                ch.count,
                sacrifice_candidates(st, p, ch, goal.sac_source, goal.sacrifice),
            )
        }),
        tap: goal.tap_cost.as_ref().map(|ch| {
            (
                ch.count,
                tap_candidates(st, p, ch, goal.sac_source, goal.tap),
            )
        }),
    }
}

pub(crate) fn goal_feasible(st: &GameState, p: PlayerId, goal: &PayGoal) -> bool {
    plan(&plan_input(st, p, goal))
}

/// The ways to pay `g` generic mana from the unrestricted `pool` (after `specific`):
/// each a list of `g` types, in a stable order.
fn allocations(pool: &ManaPool, req: &ManaRequirement) -> Vec<Vec<ManaType>> {
    // CR 107.4e: first the color paying each hybrid symbol, in `HYBRID_PAIRS` order.
    if req.hybrid_units() > 0 {
        let masks = req.hybrid_masks();
        let mut out = Vec::new();
        fn choose(
            masks: &[ManaMask],
            chosen: &mut Vec<ManaType>,
            req: &ManaRequirement,
            pool: &ManaPool,
            out: &mut Vec<Vec<ManaType>>,
        ) {
            let Some((&m, rest)) = masks.split_first() else {
                for generic in allocations(pool, &req.with_hybrid(chosen)) {
                    let mut a = chosen.clone();
                    a.extend(generic);
                    out.push(a);
                }
                return;
            };
            for t in types_in(m) {
                chosen.push(t);
                choose(rest, chosen, req, pool, out);
                chosen.pop();
            }
        }
        choose(&masks, &mut Vec::new(), req, pool, &mut out);
        return out;
    }
    if !req.payable_from(pool) {
        return Vec::new();
    }
    let left = req.generic_candidates(pool);
    let mut out = Vec::new();
    fn go(
        left: &[u32; 6],
        t: usize,
        need: u32,
        cur: &mut Vec<ManaType>,
        out: &mut Vec<Vec<ManaType>>,
    ) {
        if need == 0 {
            out.push(cur.clone());
            return;
        }
        if t == 6 {
            return;
        }
        for k in (0..=need.min(left[t])).rev() {
            for _ in 0..k {
                cur.push(ManaType::ALL[t]);
            }
            go(left, t + 1, need - k, cur, out);
            cur.truncate(cur.len() - k as usize);
        }
    }
    go(&left, 0, req.generic, &mut Vec::new(), &mut out);
    out
}

/// The cost `req` paid with `paid` (the colors for its hybrid symbols, then the mana for
/// its generic part): what `GameAction::PayMana` takes.
fn payment(req: &ManaRequirement, paid: &[ManaType]) -> (ManaRequirement, [u32; 6]) {
    let h = req.hybrid_units() as usize;
    (req.with_hybrid(&paid[..h]), alloc_of(&paid[h..]))
}

fn alloc_of(paid: &[ManaType]) -> [u32; 6] {
    let mut a = [0u32; 6];
    for t in paid {
        a[t.index()] += 1;
    }
    a
}

/// One activation of `w` on a copy of the game, as far as it matters for paying: its
/// costs (tap, mana, life, sacrifices) and the mana it makes (the first mana `mana`, the
/// rest left as types still to be chosen). Damage it would deal is taken as life lost.
fn simulate(
    st: &GameState,
    p: PlayerId,
    w: &ManaWay,
    mana: ManaType,
    sacrificed: &[ObjectId],
    paid: &[ManaType],
) -> (GameState, Vec<Have>) {
    let mut sim = st.clone();
    if w.tap {
        execute(&mut sim, GameAction::Tap { object: w.id });
    }
    if w.cost.total() > 0 {
        let (requirement, generic) = payment(&w.cost, paid);
        execute(
            &mut sim,
            GameAction::PayMana {
                player: p,
                requirement,
                generic,
            },
        );
    }
    let lose = w.life + w.risk;
    if lose > 0 {
        execute(
            &mut sim,
            GameAction::LoseLife {
                player: p,
                amount: lose,
            },
        );
    }
    let mut gone: Vec<ObjectId> = sacrificed.to_vec();
    if w.sac_self {
        gone.push(w.id);
    }
    for id in gone {
        if sim
            .objects
            .get(&id)
            .is_some_and(|o| o.zone == Zone::Battlefield)
        {
            move_object(&mut sim, id, Zone::Graveyard, LibraryPosition::Top, None);
        }
    }
    let mut floating = Vec::new();
    let add = |sim: &mut GameState, t: ManaType| match (w.restricted, w.ability) {
        (true, Some(ability)) => {
            execute(
                sim,
                GameAction::AddRestrictedMana {
                    player: p,
                    mana: t,
                    ability,
                    chosen: super::cast::restriction_choice(st, ability, w.id),
                },
            );
        }
        _ => {
            execute(sim, GameAction::AddMana { player: p, mana: t });
        }
    };
    match &w.yields {
        Yield::One { count, .. } => {
            for _ in 0..*count {
                add(&mut sim, mana);
            }
        }
        Yield::Units(v) => {
            add(&mut sim, mana);
            // Restricted mana pays only what it may be spent on: the cost being paid
            // (the way is offered for it), not other mana abilities' costs.
            floating.extend(
                v.iter()
                    .skip(1)
                    .map(|&m| (m, if w.restricted { 0 } else { m })),
            );
        }
    }
    (sim, floating)
}

/// Would the cost of `goal` still be payable after activating `w` for `mana`, with
/// `sacrificed` and `paid` (or, if empty while the ability needs them, some choice of
/// them)?
fn option_keeps(
    st: &GameState,
    p: PlayerId,
    goal: &PayGoal,
    inp: &PlanInput,
    w: &ManaWay,
    mana: ManaType,
) -> bool {
    let dependent = inp.ways.iter().any(|x| x.dependent);
    if w.simple && !(w.sac_self && dependent) {
        // Nothing else changes: the planner's snapshot is updated directly.
        let mut next = inp.clone();
        next.ways.retain(|x| x.id != w.id);
        if w.sac_self
            && let Some((_, cands)) = &mut next.sac
        {
            cands.retain(|c| *c != w.id);
        }
        // Tapped (or gone): no longer one the cost could tap.
        if let Some((_, cands)) = &mut next.tap {
            cands.retain(|c| *c != w.id);
        }
        let Yield::One { count, .. } = w.yields else {
            unreachable!("a simple way makes one type")
        };
        if w.restricted {
            next.restricted.add(mana, count);
        } else {
            next.pool.add(mana, count);
        }
        return plan(&next);
    }
    sacrifice_choices(w, Some(goal))
        .into_iter()
        .any(|sac| choice_keeps(st, p, goal, w, mana, &sac, None))
}

/// The sacrifices the ability could be activated with (one candidate each; an empty list
/// when it sacrifices nothing chosen). Never what the cost being paid taps or sacrifices,
/// nor the source of what is being paid for.
fn sacrifice_choices(w: &ManaWay, goal: Option<&PayGoal>) -> Vec<Vec<ObjectId>> {
    match &w.sac {
        Some((_, cands)) => cands
            .iter()
            .filter(|&&c| {
                goal.is_none_or(|g| Some(c) != g.tap && Some(c) != g.sacrifice && c != g.sac_source)
            })
            .map(|&c| vec![c])
            .collect(),
        None => vec![Vec::new()],
    }
}

/// With `sacrificed` chosen (and `paid`, if given; otherwise any way to pay its mana
/// cost), does the cost stay payable?
fn choice_keeps(
    st: &GameState,
    p: PlayerId,
    goal: &PayGoal,
    w: &ManaWay,
    mana: ManaType,
    sacrificed: &[ObjectId],
    paid: Option<&[ManaType]>,
) -> bool {
    let pool = st.player(p).mana_pool;
    let options = match paid {
        Some(paid) => vec![paid.to_vec()],
        None => allocations(&pool, &w.cost),
    };
    options.into_iter().any(|paid| {
        let (sim, floating) = simulate(st, p, w, mana, sacrificed, &paid);
        let mut inp = plan_input(&sim, p, goal);
        inp.floating = floating;
        plan(&inp)
    })
}

/// The mana abilities `p` may activate while paying for `goal` (CR 601.2g), each only if
/// the cost stays payable (D-019).
pub(crate) fn pay_options(st: &GameState, p: PlayerId, goal: &PayGoal) -> Vec<OptionDesc> {
    let inp = plan_input(st, p, goal);
    let mut options = Vec::new();
    for w in &inp.ways {
        // Convoke / improvise: tapping it pays one mana of the cost — a mana of its color,
        // or one generic mana — if the rest stays payable.
        if w.pays {
            let mask = w.yields.first();
            let mut paying: Vec<Option<ManaType>> = types_in(mask)
                .into_iter()
                .filter(|t| inp.req.specific[t.index()] > 0)
                .map(Some)
                .collect();
            if mask & GENERIC != 0 && inp.req.generic > 0 {
                paying.push(None);
            }
            for mana in paying {
                let mut next = inp.clone();
                next.ways.retain(|x| x.id != w.id);
                if let Some((_, cands)) = &mut next.tap {
                    cands.retain(|c| *c != w.id);
                }
                match mana {
                    Some(t) => next.req.specific[t.index()] -= 1,
                    None => next.req.generic -= 1,
                }
                if plan(&next) {
                    options.push(OptionDesc::TapToPay { source: w.id, mana });
                }
            }
            continue;
        }
        // Its own mana cost is paid from the pool (CR 605.3a: tap for that first).
        if !w.cost.payable_from(&inp.pool) {
            continue;
        }
        for mana in types_in(w.yields.first()) {
            let option = OptionDesc::TapForMana {
                source: w.id,
                mana,
                ability: w.ability,
            };
            if options.contains(&option) {
                continue;
            }
            if option_keeps(st, p, goal, &inp, w, mana) {
                options.push(option);
            }
        }
    }
    options
}

/// The mana abilities `p` may activate with priority (CR 117.1d, 605.3a): the mana floats.
pub(crate) fn priority_options(st: &GameState, p: PlayerId) -> Vec<OptionDesc> {
    let pool = st.player(p).mana_pool;
    let mut options = Vec::new();
    for w in mana_ways(st, p, None, None) {
        if !w.cost.payable_from(&pool) {
            continue;
        }
        for mana in types_in(w.yields.first()) {
            let option = OptionDesc::TapForMana {
                source: w.id,
                mana,
                ability: w.ability,
            };
            // An ability the permanent has twice is one option.
            if !options.contains(&option) {
                options.push(option);
            }
        }
    }
    options
}

/// The way an offered `TapForMana` names.
pub(crate) fn find_way(
    st: &GameState,
    p: PlayerId,
    source: ObjectId,
    mana: ManaType,
    ability: Option<AbilityId>,
    payee: Option<Payee>,
) -> ManaWay {
    mana_ways(st, p, None, payee)
        .into_iter()
        .find(|w| {
            w.id == source && w.ability == ability && w.yields.first() & (1 << mana.index()) != 0
        })
        .expect("an offered mana ability")
}

/// CR 605.3b: activate a mana ability that is nothing but `{T}` (and maybe "sacrifice
/// this") for mana of one type. It resolves immediately, adding as many mana of the
/// chosen type as the ability makes. Returns the rest of its cost — "sacrifice this" —
/// for the caller to apply (a replaceable event).
pub(crate) fn tap_for_mana(
    st: &mut GameState,
    p: PlayerId,
    way: &ManaWay,
    mana: ManaType,
) -> Vec<Proposed> {
    debug_assert!(way.simple);
    let Yield::One { count, .. } = way.yields else {
        unreachable!("a simple way makes one type")
    };
    // CR 106.6: mana with a spending restriction is kept apart.
    let restricted = way.ability.filter(|_| way.restricted);
    execute(st, GameAction::Tap { object: way.id });
    for _ in 0..count {
        execute(
            st,
            match restricted {
                Some(ability) => GameAction::AddRestrictedMana {
                    player: p,
                    mana,
                    ability,
                    chosen: super::cast::restriction_choice(st, ability, way.id),
                },
                None => GameAction::AddMana { player: p, mana },
            },
        );
    }
    if way.sac_self {
        vec![Proposed::moving(
            way.id,
            Zone::Graveyard,
            MoveCause::Sacrifice,
        )]
    } else {
        Vec::new()
    }
}

/// Start activating the mana ability `source`/`ability` for `mana` (an offered option):
/// done at once if it is a plain tap; otherwise a [`ManaFrame`] runs. `then` is the frame
/// that carries on afterwards.
pub(crate) fn activate(
    st: &mut GameState,
    p: PlayerId,
    then: Frame,
    source: ObjectId,
    mana: ManaType,
    ability: Option<AbilityId>,
    goal: Option<PayGoal>,
) -> Flow {
    let payee = goal.as_ref().and_then(|g| g.payee);
    let way = find_way(st, p, source, mana, ability, payee);
    if way.simple {
        let cost = tap_for_mana(st, p, &way, mana);
        return if cost.is_empty() {
            Flow::then(then)
        } else {
            // CR 605.3b: the mana ability resolves at once; its own cost (the sacrifice)
            // is paid now, and the frame carries on after.
            Flow::Continue(vec![then, Frame::Apply(ApplyFrame::new(st, cost))])
        };
    }
    let frame = ManaFrame {
        player: p,
        source,
        ability: ability.expect("a mana ability with more than a tap is named"),
        mana,
        goal,
        stage: ManaStage::Sacrifice,
        sacrifice: Vec::new(),
        paid: Vec::new(),
        chosen: Vec::new(),
        lki: None,
    };
    Flow::Continue(vec![then, Frame::Mana(alloc::boxed::Box::new(frame))])
}

// ---------------------------------------------------------------------- activation

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaStage {
    /// Choose what to sacrifice for its cost.
    Sacrifice,
    /// Choose which floating mana pays the generic part of its cost.
    Generic,
    /// CR 602.2h: pay its costs.
    Pay,
    /// CR 605.3b: it resolves: the type of each further mana it makes is chosen.
    Types,
    /// Add the mana, then do the rest of what it says.
    Produce,
}

/// Activating one mana ability that needs more than a tap (CR 605.3, 602.2): choices,
/// costs, then it resolves at once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaFrame {
    pub player: PlayerId,
    pub source: ObjectId,
    pub ability: AbilityId,
    /// The type of the first mana it makes (from the option).
    pub mana: ManaType,
    /// What is being paid for, if anything: choices that would make it unpayable aren't
    /// offered (D-019).
    pub goal: Option<PayGoal>,
    pub stage: ManaStage,
    pub sacrifice: Vec<ObjectId>,
    /// The floating mana chosen for the generic part of its cost, one per mana.
    pub paid: Vec<ManaType>,
    /// The types chosen for the mana it makes after the first.
    pub chosen: Vec<ManaType>,
    /// The source as it was when activated (for what it does if it is gone).
    pub lki: Option<crate::layers::Characteristics>,
}

impl ManaFrame {
    fn def(&self, st: &GameState) -> (Cost, ManaProduce, Option<Effect>) {
        match st.catalog.ability(self.ability) {
            AbilityDef::Mana {
                cost,
                produce,
                then,
                ..
            } => (cost.clone(), produce.clone(), then.clone()),
            other => unreachable!("not a mana ability: {other:?}"),
        }
    }

    /// The way as it was offered (only valid before its costs are paid).
    fn way(&self, st: &GameState) -> ManaWay {
        let payee = self.goal.as_ref().and_then(|g| g.payee);
        find_way(
            st,
            self.player,
            self.source,
            self.mana,
            Some(self.ability),
            payee,
        )
    }

    fn ask(self, kind: ChoiceKind, options: Vec<OptionDesc>) -> Flow {
        let p = self.player;
        Flow::ask(
            Frame::Mana(alloc::boxed::Box::new(self)),
            p,
            kind,
            Primitive::PickN { min: 1, max: 1 },
            options,
        )
    }

    pub(crate) fn run(mut self, st: &mut GameState, mut input: Option<Input>) -> Flow {
        let p = self.player;
        loop {
            match self.stage {
                ManaStage::Sacrifice => {
                    if let Some(inp) = input.take() {
                        match inp.picked_one() {
                            Some(OptionDesc::Card { card }) => self.sacrifice = vec![*card],
                            other => unreachable!("{other:?}"),
                        }
                        self.stage = ManaStage::Generic;
                        continue;
                    }
                    let w = self.way(st);
                    if w.sac.is_none() {
                        self.stage = ManaStage::Generic;
                        continue;
                    }
                    let options: Vec<OptionDesc> = sacrifice_choices(&w, self.goal.as_ref())
                        .into_iter()
                        .filter(|sac| {
                            self.goal
                                .as_ref()
                                .is_none_or(|g| choice_keeps(st, p, g, &w, self.mana, sac, None))
                        })
                        .map(|sac| OptionDesc::Card { card: sac[0] })
                        .collect();
                    assert!(
                        !options.is_empty(),
                        "mana ability offered with nothing to sacrifice"
                    );
                    return self.ask(ChoiceKind::Sacrifice, options);
                }
                ManaStage::Generic => {
                    if let Some(inp) = input.take() {
                        match inp.picked_one() {
                            Some(OptionDesc::Mana { mana }) => self.paid.push(*mana),
                            other => unreachable!("{other:?}"),
                        }
                    }
                    let w = self.way(st);
                    if self.paid.len() as u32 >= w.cost.hybrid_units() + w.cost.generic {
                        self.stage = ManaStage::Pay;
                        continue;
                    }
                    // Every complete way to pay that keeps the cost being paid payable
                    // (the colors for hybrid symbols first, then the generic part, each in
                    // a fixed order); the next mana is asked only if that is a real choice.
                    let pool = st.player(p).mana_pool;
                    let fits: Vec<Vec<ManaType>> = allocations(&pool, &w.cost)
                        .into_iter()
                        .filter(|a| a.starts_with(&self.paid))
                        .filter(|a| {
                            self.goal.as_ref().is_none_or(|g| {
                                choice_keeps(st, p, g, &w, self.mana, &self.sacrifice, Some(a))
                            })
                        })
                        .collect();
                    let mut next: Vec<ManaType> = Vec::new();
                    for a in &fits {
                        let t = a[self.paid.len()];
                        if !next.contains(&t) {
                            next.push(t);
                        }
                    }
                    next.sort();
                    match next.as_slice() {
                        [] => unreachable!("mana ability offered with no way to pay for it"),
                        [only] => self.paid.push(*only),
                        _ => {
                            let options = next
                                .into_iter()
                                .map(|mana| OptionDesc::Mana { mana })
                                .collect();
                            return self.ask(ChoiceKind::PayMana, options);
                        }
                    }
                }
                ManaStage::Pay => {
                    // CR 602.2h / 605.3a: pay the costs; then it resolves (CR 605.3b).
                    let w = self.way(st);
                    self.lki = Some(chars(st, self.source));
                    if w.tap {
                        execute(
                            st,
                            GameAction::Tap {
                                object: self.source,
                            },
                        );
                    }
                    if w.cost.total() > 0 {
                        let (requirement, generic) = payment(&w.cost, &self.paid);
                        execute(
                            st,
                            GameAction::PayMana {
                                player: p,
                                requirement,
                                generic,
                            },
                        );
                    }
                    let mut events: Vec<Proposed> = self
                        .sacrifice
                        .iter()
                        .chain(w.sac_self.then_some(&self.source))
                        .map(|&id| Proposed::moving(id, Zone::Graveyard, MoveCause::Sacrifice))
                        .collect();
                    if w.life > 0 {
                        // CR 119.4: paying life is losing that much life.
                        events.push(Proposed::LoseLife {
                            player: p,
                            amount: w.life,
                        });
                    }
                    self.stage = ManaStage::Types;
                    if !events.is_empty() {
                        let apply = Frame::Apply(ApplyFrame::new(st, events));
                        return Flow::Continue(vec![
                            Frame::Mana(alloc::boxed::Box::new(self)),
                            apply,
                        ]);
                    }
                }
                ManaStage::Types => {
                    if let Some(inp) = input.take() {
                        match inp.picked_one() {
                            Some(OptionDesc::Mana { mana }) => self.chosen.push(*mana),
                            other => unreachable!("{other:?}"),
                        }
                    }
                    let (_, produce, _) = self.def(st);
                    let masks: Vec<ManaMask> = match yields_now(st, p, self.source, &produce) {
                        Yield::Units(v) => v.into_iter().skip(1).collect(),
                        Yield::One { .. } => Vec::new(),
                    };
                    let Some(&mask) = masks.get(self.chosen.len()) else {
                        self.stage = ManaStage::Produce;
                        continue;
                    };
                    let rest: Vec<ManaMask> = masks[self.chosen.len() + 1..].to_vec();
                    let restricted = matches!(
                        st.catalog.ability(self.ability),
                        AbilityDef::Mana { spend: Some(_), .. }
                    );
                    let next: Vec<ManaType> = types_in(mask)
                        .into_iter()
                        .filter(|&t| match &self.goal {
                            None => true,
                            Some(g) => {
                                let mut inp = plan_input(st, p, g);
                                let mut made = vec![self.mana];
                                made.extend(self.chosen.iter().copied());
                                made.push(t);
                                for m in made {
                                    if restricted {
                                        inp.restricted.add(m, 1);
                                    } else {
                                        inp.pool.add(m, 1);
                                    }
                                }
                                inp.floating.extend(
                                    rest.iter().map(|&m| (m, if restricted { 0 } else { m })),
                                );
                                plan(&inp)
                            }
                        })
                        .collect();
                    match next.as_slice() {
                        [] => unreachable!("no type keeps the cost payable"),
                        [only] => self.chosen.push(*only),
                        _ => {
                            let options = next
                                .into_iter()
                                .map(|mana| OptionDesc::Mana { mana })
                                .collect();
                            return self.ask(ChoiceKind::ChooseColor, options);
                        }
                    }
                }
                ManaStage::Produce => {
                    let (_, produce, then) = self.def(st);
                    let restricted = matches!(
                        st.catalog.ability(self.ability),
                        AbilityDef::Mana { spend: Some(_), .. }
                    );
                    let made: Vec<ManaType> = match yields_now(st, p, self.source, &produce) {
                        // CR 106.5: mana of a type it can't make now is no mana.
                        Yield::One { mask, count } if mask & (1 << self.mana.index()) != 0 => {
                            vec![self.mana; count as usize]
                        }
                        Yield::One { .. } => Vec::new(),
                        Yield::Units(_) => {
                            let mut v = vec![self.mana];
                            v.extend(self.chosen.iter().copied());
                            v
                        }
                    };
                    for mana in made {
                        execute(
                            st,
                            if restricted {
                                GameAction::AddRestrictedMana {
                                    player: p,
                                    mana,
                                    ability: self.ability,
                                    chosen: super::cast::restriction_choice(
                                        st,
                                        self.ability,
                                        self.source,
                                    ),
                                }
                            } else {
                                GameAction::AddMana { player: p, mana }
                            },
                        );
                    }
                    return match then {
                        None => Flow::done(),
                        Some(effect) => Flow::then(Frame::Resolve(alloc::boxed::Box::new(
                            ResolveFrame::mana_ability(
                                p,
                                self.source,
                                self.ability.card,
                                effect,
                                self.lki.clone(),
                            ),
                        ))),
                    };
                }
            }
        }
    }
}
