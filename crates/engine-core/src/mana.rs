//! Mana pools, cost requirements and exact payment feasibility (CR 106, 601.2g–h).

use alloc::vec;
use alloc::vec::Vec;
use card_model::{ManaCost, ManaSymbol};
use serde::{Deserialize, Serialize};

pub use card_model::ManaType;

/// A set of mana types, as a bit mask over `ManaType::index()`.
pub type ManaMask = u8;

pub fn mask_of(types: &[ManaType]) -> ManaMask {
    types.iter().fold(0, |m, t| m | (1 << t.index()))
}

pub fn types_in(mask: ManaMask) -> Vec<ManaType> {
    ManaType::ALL
        .iter()
        .copied()
        .filter(|t| mask & (1 << t.index()) != 0)
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaPool {
    pub amounts: [u32; 6],
}

impl ManaPool {
    pub fn add(&mut self, t: ManaType, n: u32) {
        self.amounts[t.index()] += n;
    }

    pub fn total(&self) -> u32 {
        self.amounts.iter().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// An unpaid mana requirement: generic plus per-type specific requirements.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaRequirement {
    pub generic: u32,
    /// Indexed by `ManaType::index()`; the colorless slot means `{C}`.
    pub specific: [u32; 6],
}

impl ManaRequirement {
    /// The requirement for `cost` with `{X}` = `x` (CR 107.3a).
    pub fn from_cost(cost: &ManaCost, x: u32) -> Self {
        let mut r = ManaRequirement::default();
        for s in &cost.symbols {
            match s {
                ManaSymbol::Generic(n) => r.generic += n,
                ManaSymbol::Colored(c) => r.specific[ManaType::from_color(*c).index()] += 1,
                ManaSymbol::Colorless => r.specific[ManaType::Colorless.index()] += 1,
                ManaSymbol::X => r.generic += x,
            }
        }
        r
    }

    pub fn total(&self) -> u32 {
        self.generic + self.specific.iter().sum::<u32>()
    }

    /// Whether `pool` alone can pay the whole requirement.
    pub fn payable_from(&self, pool: &ManaPool) -> bool {
        self.feasible(pool, &[])
    }

    /// Whether `pool` plus untapped sources can pay the requirement. Each source makes ONE
    /// mana of any type in its mask (M1: every mana ability costs exactly `{T}`, D-021).
    ///
    /// Exact: pool mana pays matching specific symbols first (always optimal, since a source
    /// could pay generic instead), then the remaining specific symbols are matched to
    /// sources by bipartite matching (augmenting paths), and generic is paid by whatever is
    /// left.
    pub fn feasible(&self, pool: &ManaPool, sources: &[ManaMask]) -> bool {
        let mut leftover_pool = 0u32;
        let mut units: Vec<usize> = Vec::new();
        for t in 0..6 {
            let from_pool = pool.amounts[t].min(self.specific[t]);
            leftover_pool += pool.amounts[t] - from_pool;
            for _ in 0..self.specific[t] - from_pool {
                units.push(t);
            }
        }
        if units.len() > sources.len() {
            return false;
        }
        let mut owner: Vec<Option<usize>> = vec![None; sources.len()];
        for u in 0..units.len() {
            let mut seen = vec![false; sources.len()];
            if !augment(u, &units, sources, &mut owner, &mut seen) {
                return false;
            }
        }
        let free_sources = (sources.len() - units.len()) as u32;
        self.generic <= leftover_pool + free_sources
    }

    /// After specific symbols are paid from `pool`, how much of each type is left to put
    /// toward generic. Caller must have checked `payable_from`.
    pub fn generic_candidates(&self, pool: &ManaPool) -> [u32; 6] {
        let mut left = [0u32; 6];
        for (t, l) in left.iter_mut().enumerate() {
            *l = pool.amounts[t] - self.specific[t];
        }
        left
    }

    /// CR 601.2h: the player chooses which mana pays generic only if that is ambiguous.
    /// Returns the forced allocation (per type) or `None` if the player must choose.
    pub fn forced_generic(&self, pool: &ManaPool) -> Option<[u32; 6]> {
        let left = self.generic_candidates(pool);
        let kinds = left.iter().filter(|n| **n > 0).count();
        let available: u32 = left.iter().sum();
        if self.generic == 0 {
            Some([0; 6])
        } else if available == self.generic {
            Some(left)
        } else if kinds == 1 {
            let mut a = [0u32; 6];
            for t in 0..6 {
                a[t] = left[t].min(self.generic);
            }
            Some(a)
        } else {
            None
        }
    }

    /// Remove the specific symbols plus `generic_alloc` from `pool`.
    pub fn pay(&self, pool: &mut ManaPool, generic_alloc: &[u32; 6]) {
        debug_assert_eq!(generic_alloc.iter().sum::<u32>(), self.generic);
        for t in 0..6 {
            pool.amounts[t] -= self.specific[t] + generic_alloc[t];
        }
    }
}

fn augment(
    u: usize,
    units: &[usize],
    sources: &[ManaMask],
    owner: &mut [Option<usize>],
    seen: &mut [bool],
) -> bool {
    for s in 0..sources.len() {
        if sources[s] & (1 << units[u]) == 0 || seen[s] {
            continue;
        }
        seen[s] = true;
        if owner[s].is_none_or(|v| augment(v, units, sources, owner, seen)) {
            owner[s] = Some(u);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str) -> ManaRequirement {
        ManaRequirement::from_cost(&ManaCost::parse(text).unwrap(), 0)
    }

    const G: ManaMask = 1 << 4;
    const R: ManaMask = 1 << 3;
    const ANY: ManaMask = 0b11111;

    #[test]
    fn feasibility_with_single_and_multi_type_sources() {
        let pool = ManaPool::default();
        assert!(req("{1}{G}").feasible(&pool, &[G, R]));
        assert!(!req("{G}{G}").feasible(&pool, &[G, R]));
        // A dual (R|G) must be saved for the second G: matching finds it.
        assert!(req("{G}{R}").feasible(&pool, &[R | G, G]));
        assert!(req("{G}{G}{R}").feasible(&pool, &[R | G, G, ANY]));
        assert!(!req("{G}{G}{R}").feasible(&pool, &[R | G, G]));
    }

    #[test]
    fn x_adds_generic() {
        let r = ManaRequirement::from_cost(&ManaCost::parse("{X}{R}").unwrap(), 3);
        assert_eq!(r.total(), 4);
        assert!(r.feasible(&ManaPool::default(), &[R, G, G, G]));
        assert!(!r.feasible(&ManaPool::default(), &[R, G, G]));
    }

    #[test]
    fn generic_choice_only_when_ambiguous() {
        let mut p = ManaPool::default();
        p.add(ManaType::Green, 2);
        p.add(ManaType::Red, 1);
        let r = req("{1}{G}");
        assert!(r.payable_from(&p));
        assert_eq!(r.forced_generic(&p), None, "G or R could pay the {{1}}");
        let mut p2 = ManaPool::default();
        p2.add(ManaType::Green, 3);
        assert!(r.forced_generic(&p2).is_some());
        let mut alloc = [0; 6];
        alloc[ManaType::Red.index()] = 1;
        r.pay(&mut p, &alloc);
        assert_eq!(p.amounts[ManaType::Green.index()], 1);
        assert_eq!(p.total(), 1);
    }
}
