//! Agents. Every agent implements `engine_core::Agent` and sees only a `PlayerView`
//! (CLAUDE.md invariant 6). Randomness comes from the agent's own seeded `GameRng`, never
//! from the game's RNG and never from `thread_rng` (invariants 1, 7).

use engine_core::decision::{PendingDecision, Primitive, Response};
use engine_core::rng::GameRng;
use engine_core::{Agent, PlayerView};

/// Answers every decision uniformly at random among shape-valid responses.
#[derive(Clone, Debug)]
pub struct RandomAgent {
    rng: GameRng,
}

impl RandomAgent {
    pub fn new(seed: u64) -> Self {
        RandomAgent {
            rng: GameRng::from_seed_u64(seed),
        }
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.rng.below(hi - lo + 1)
    }
}

impl Agent for RandomAgent {
    fn decide(&mut self, _view: &PlayerView, d: &PendingDecision) -> Response {
        let n = d.options.len();
        match &d.prim {
            Primitive::PickN { min, max } => {
                let max = (*max).min(n);
                let k = self.range(*min as u64, max as u64) as usize;
                let mut idx: Vec<usize> = (0..n).collect();
                self.rng.shuffle(&mut idx);
                idx.truncate(k);
                idx.sort_unstable();
                Response::Pick(idx)
            }
            Primitive::PickNumber { min, max } => {
                Response::Number(self.range(u64::from(*min), u64::from(*max)) as u32)
            }
            Primitive::Allocate {
                total,
                per_min,
                per_max,
            } => {
                let mut v = per_min.clone();
                let mut left = total - per_min.iter().sum::<u32>();
                while left > 0 {
                    let open: Vec<usize> = (0..n).filter(|&i| v[i] < per_max[i]).collect();
                    let i = open[self.rng.below(open.len() as u64) as usize];
                    v[i] += 1;
                    left -= 1;
                }
                Response::Allocate(v)
            }
            Primitive::Order => {
                let mut idx: Vec<usize> = (0..n).collect();
                self.rng.shuffle(&mut idx);
                Response::Order(idx)
            }
        }
    }
}
