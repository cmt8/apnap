//! Mana pools and cost payment (CR 106, 601.2g–h). M0: basic-land mana and printed costs.

use card_model::{Color, ManaCost, ManaSymbol};
use serde::{Deserialize, Serialize};

/// The six mana types (CR 106.1b): five colors plus colorless.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ManaType {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

impl ManaType {
    pub const ALL: [ManaType; 6] = [
        ManaType::White,
        ManaType::Blue,
        ManaType::Black,
        ManaType::Red,
        ManaType::Green,
        ManaType::Colorless,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_color(c: Color) -> Self {
        match c {
            Color::White => ManaType::White,
            Color::Blue => ManaType::Blue,
            Color::Black => ManaType::Black,
            Color::Red => ManaType::Red,
            Color::Green => ManaType::Green,
        }
    }
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
    pub fn from_cost(cost: &ManaCost) -> Self {
        let mut r = ManaRequirement::default();
        for s in &cost.symbols {
            match s {
                ManaSymbol::Generic(n) => r.generic += n,
                ManaSymbol::Colored(c) => r.specific[ManaType::from_color(*c).index()] += 1,
                ManaSymbol::Colorless => r.specific[ManaType::Colorless.index()] += 1,
            }
        }
        r
    }

    /// Whether `pool` alone can pay the whole requirement.
    pub fn payable_from(&self, pool: &ManaPool) -> bool {
        self.feasible(pool, &[0; 6])
    }

    /// Whether `pool` plus untapped single-type sources (`sources[t]` = number of sources
    /// producing exactly one mana of type `t`) can pay the requirement.
    ///
    /// Exact for single-type sources (all M0 sources are basic lands): each specific
    /// shortfall needs a source of that type, and every leftover mana or source pays generic.
    pub fn feasible(&self, pool: &ManaPool, sources: &[u32; 6]) -> bool {
        let mut leftover_pool = 0u32;
        let mut leftover_sources = 0u32;
        for t in ManaType::ALL.map(ManaType::index) {
            let from_pool = pool.amounts[t].min(self.specific[t]);
            let need = self.specific[t] - from_pool;
            if need > sources[t] {
                return false;
            }
            leftover_pool += pool.amounts[t] - from_pool;
            leftover_sources += sources[t] - need;
        }
        self.generic <= leftover_pool + leftover_sources
    }

    /// Remove the mana to pay this requirement from `pool`. Specific symbols first; generic
    /// is then paid from the remaining mana in a fixed order (colorless, then W, U, B, R, G).
    /// Caller must have checked `payable_from`.
    ///
    /// M0 simplification (D-010): the engine picks which pool mana pays generic. With only
    /// basic-land mana this never changes what the player can do afterwards *for M0 cards*;
    /// M1's casting pipeline makes this a player choice where it matters.
    pub fn pay_from(&self, pool: &mut ManaPool) {
        debug_assert!(self.payable_from(pool));
        for t in 0..6 {
            pool.amounts[t] -= self.specific[t];
        }
        let mut generic = self.generic;
        let order = [5usize, 0, 1, 2, 3, 4];
        for t in order {
            let take = pool.amounts[t].min(generic);
            pool.amounts[t] -= take;
            generic -= take;
        }
        debug_assert_eq!(generic, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str) -> ManaRequirement {
        ManaRequirement::from_cost(&ManaCost::parse(text).unwrap())
    }

    #[test]
    fn feasibility_respects_colors() {
        let g = ManaType::Green.index();
        let r = ManaType::Red.index();
        let mut sources = [0u32; 6];
        sources[g] = 1;
        sources[r] = 1;
        let pool = ManaPool::default();
        assert!(req("{1}{G}").feasible(&pool, &sources));
        assert!(!req("{G}{G}").feasible(&pool, &sources));
        // After tapping the Forest for generic... the requirement is now unmeetable:
        let mut p = ManaPool::default();
        p.add(ManaType::Green, 1);
        let mut only_red = [0u32; 6];
        only_red[r] = 1;
        assert!(req("{1}{G}").feasible(&p, &only_red));
        assert!(!req("{G}{G}").feasible(&p, &only_red));
    }

    #[test]
    fn pay_from_pool() {
        let mut p = ManaPool::default();
        p.add(ManaType::Green, 2);
        p.add(ManaType::Red, 1);
        let r = req("{1}{G}");
        assert!(r.payable_from(&p));
        r.pay_from(&mut p);
        assert_eq!(p.total(), 1);
    }
}
