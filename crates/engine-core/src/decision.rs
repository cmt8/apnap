//! The decision boundary (spec §5): the engine SUSPENDS with a [`PendingDecision`] and the
//! driver answers with a [`Response`]. The engine never calls into the UI or AI.
//!
//! Four primitives describe the *shape* of an answer; [`ChoiceKind`] gives the semantic
//! context. Adding a decision = adding a `ChoiceKind` variant (exhaustive matches in UI/AI
//! then point at every site that must handle it).

use crate::ids::{ObjectId, PlayerId};
use crate::mana::ManaType;
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

/// What the outside world submits: always an answer to the CURRENT pending decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    /// Indices into `PendingDecision::options` (a set; order is not significant).
    Pick(Vec<usize>),
    Number(u32),
    /// One amount per option (recipient), in option order.
    Allocate(Vec<u32>),
    /// A permutation of `0..n` (option indices); the meaning of "first" is documented on
    /// the `ChoiceKind` that asks.
    Order(Vec<usize>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDecision {
    pub player: PlayerId,
    pub kind: ChoiceKind,
    pub prim: Primitive,
    /// Stable order (timestamp/seat order). Every option offered is legal.
    pub options: Vec<OptionDesc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Primitive {
    PickN {
        min: usize,
        max: usize,
    },
    PickNumber {
        min: u32,
        max: u32,
    },
    /// Distribute exactly `total` among `options.len()` recipients within per-recipient bounds.
    Allocate {
        total: u32,
        per_min: Vec<u32>,
        per_max: Vec<u32>,
    },
    /// Order all `options.len()` options.
    Order,
}

/// Semantic context for a decision. Exhaustive by design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChoiceKind {
    /// CR 103.1: the chosen player picks who takes the first turn.
    ChooseStartingPlayer,
    /// CR 103.5: keep or mulligan.
    Mulligan,
    /// CR 103.5: choose cards to put on the bottom (PickN), then their order (Order:
    /// first listed is put on the bottom first, so the LAST listed ends up bottom-most).
    MulliganBottom,
    Priority,
    ChooseTargets {
        slot: u8,
    },
    ChooseModes,
    ChooseX,
    /// CR 601.2g–h: activate a mana ability while paying a cost.
    PayMana,
    /// CR 508.1: one decision per potential attacker (pick 0 or 1 defender).
    DeclareAttackers,
    /// CR 509.1: one decision per potential blocker (pick 0 or 1 attacker).
    DeclareBlockers,
    /// CR 510.1c: divide a blocked creature's damage among its blockers.
    AssignCombatDamage,
    OrderTriggers,
    ChooseReplacement,
    CommanderToCommandZone,
    SearchLibrary,
    /// CR 514.1 cleanup discard (and later discard effects).
    Discard,
    Sacrifice,
    MayAbility,
    PayUnlessCost,
    ScryOrder,
    ChooseColor,
    ChoosePlayer,
}

/// A structured description of one option. Frames read the chosen option back from the
/// stored `PendingDecision`, so options are the single source of truth for what an index means.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptionDesc {
    Pass,
    PlayLand {
        card: ObjectId,
    },
    CastSpell {
        card: ObjectId,
    },
    /// Activate a basic land's intrinsic mana ability (CR 305.6).
    TapForMana {
        source: ObjectId,
        mana: ManaType,
    },
    Player {
        player: PlayerId,
    },
    Keep,
    Mulligan,
    /// A card (hand discard, mulligan bottom, ordering).
    Card {
        card: ObjectId,
    },
    Attack {
        attacker: ObjectId,
        defender: PlayerId,
    },
    Block {
        blocker: ObjectId,
        attacker: ObjectId,
    },
    /// A recipient of divided damage.
    DamageRecipient {
        object: ObjectId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IllegalResponse {
    NoPendingDecision,
    WrongShape,
    WrongCount { got: usize },
    IndexOutOfRange { index: usize },
    DuplicateIndex { index: usize },
    NumberOutOfRange { got: u32 },
    AllocationOutOfBounds { recipient: usize },
    AllocationTotal { got: u64, expected: u32 },
    NotAPermutation,
}

impl fmt::Display for IllegalResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl core::error::Error for IllegalResponse {}

fn check_indices(idx: &[usize], n: usize) -> Result<(), IllegalResponse> {
    for (i, &a) in idx.iter().enumerate() {
        if a >= n {
            return Err(IllegalResponse::IndexOutOfRange { index: a });
        }
        if idx[..i].contains(&a) {
            return Err(IllegalResponse::DuplicateIndex { index: a });
        }
    }
    Ok(())
}

impl PendingDecision {
    /// Shape validation. In M0 every shape-valid response is also semantically legal
    /// because only legal options are offered (CLAUDE.md "decision invariants").
    pub fn validate(&self, r: &Response) -> Result<(), IllegalResponse> {
        let n = self.options.len();
        match (&self.prim, r) {
            (Primitive::PickN { min, max }, Response::Pick(idx)) => {
                if idx.len() < *min || idx.len() > *max {
                    return Err(IllegalResponse::WrongCount { got: idx.len() });
                }
                check_indices(idx, n)
            }
            (Primitive::PickNumber { min, max }, Response::Number(x)) => {
                if x < min || x > max {
                    Err(IllegalResponse::NumberOutOfRange { got: *x })
                } else {
                    Ok(())
                }
            }
            (
                Primitive::Allocate {
                    total,
                    per_min,
                    per_max,
                },
                Response::Allocate(v),
            ) => {
                if v.len() != n {
                    return Err(IllegalResponse::WrongCount { got: v.len() });
                }
                for (i, a) in v.iter().enumerate() {
                    if *a < per_min[i] || *a > per_max[i] {
                        return Err(IllegalResponse::AllocationOutOfBounds { recipient: i });
                    }
                }
                let sum: u64 = v.iter().map(|a| u64::from(*a)).sum();
                if sum != u64::from(*total) {
                    return Err(IllegalResponse::AllocationTotal {
                        got: sum,
                        expected: *total,
                    });
                }
                Ok(())
            }
            (Primitive::Order, Response::Order(v)) => {
                if v.len() != n {
                    return Err(IllegalResponse::NotAPermutation);
                }
                check_indices(v, n).map_err(|_| IllegalResponse::NotAPermutation)
            }
            _ => Err(IllegalResponse::WrongShape),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn d(prim: Primitive, n: usize) -> PendingDecision {
        PendingDecision {
            player: PlayerId(0),
            kind: ChoiceKind::Priority,
            prim,
            options: vec![OptionDesc::Pass; n],
        }
    }

    #[test]
    fn pick_validation() {
        let p = d(Primitive::PickN { min: 1, max: 2 }, 3);
        assert!(p.validate(&Response::Pick(vec![0])).is_ok());
        assert!(p.validate(&Response::Pick(vec![2, 0])).is_ok());
        assert!(p.validate(&Response::Pick(vec![])).is_err());
        assert!(p.validate(&Response::Pick(vec![0, 0])).is_err());
        assert!(p.validate(&Response::Pick(vec![3])).is_err());
        assert!(p.validate(&Response::Number(1)).is_err());
    }

    #[test]
    fn allocate_and_order_validation() {
        let a = d(
            Primitive::Allocate {
                total: 4,
                per_min: vec![0, 0],
                per_max: vec![4, 4],
            },
            2,
        );
        assert!(a.validate(&Response::Allocate(vec![1, 3])).is_ok());
        assert!(a.validate(&Response::Allocate(vec![1, 2])).is_err());
        assert!(a.validate(&Response::Allocate(vec![4])).is_err());
        let o = d(Primitive::Order, 3);
        assert!(o.validate(&Response::Order(vec![2, 0, 1])).is_ok());
        assert!(o.validate(&Response::Order(vec![2, 2, 1])).is_err());
        assert!(o.validate(&Response::Order(vec![0, 1])).is_err());
    }
}
