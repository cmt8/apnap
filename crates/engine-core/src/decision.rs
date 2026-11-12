//! The decision boundary (spec §5): the engine SUSPENDS with a [`PendingDecision`] and the
//! driver answers with a [`Response`]. The engine never calls into the UI or AI.
//!
//! Four primitives describe the *shape* of an answer; [`ChoiceKind`] gives the semantic
//! context. Adding a decision = adding a `ChoiceKind` variant (exhaustive matches in UI/AI
//! then point at every site that must handle it).

use crate::ids::{ObjectId, PlayerId};
use crate::mana::ManaType;
use crate::stack::Target;
use alloc::vec::Vec;
use card_model::AbilityId;
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
    /// CR 601.2g–h: activate a mana ability while paying a cost (PickN), or choose which
    /// floating mana pays the generic part (Allocate over `OptionDesc::Mana`).
    PayMana,
    /// CR 508.1: one decision per potential attacker (pick 0 or 1 defender).
    DeclareAttackers,
    /// CR 509.1: one decision per potential blocker (pick 0 or 1 attacker).
    DeclareBlockers,
    /// CR 510.1c: divide a blocked creature's damage among its blockers.
    AssignCombatDamage,
    /// CR 603.3b: order your triggered abilities (Order; first listed goes on the stack
    /// first, so it resolves last).
    OrderTriggers,
    /// CR 704.5j: choose the legendary permanent to keep.
    LegendRule,
    ChooseReplacement,
    CommanderToCommandZone,
    SearchLibrary,
    /// CR 514.1 cleanup discard, and discard effects (CR 701.9).
    Discard,
    Sacrifice,
    MayAbility,
    PayUnlessCost,
    /// Order cards going on top of a library (CR 401.4; Order: the FIRST listed ends up on
    /// top).
    ScryOrder,
    ChooseColor,
    ChoosePlayer,
    /// Choose cards for an effect that puts them somewhere: from your hand or graveyard,
    /// among permanents, or among cards you are looking at (PickN over `OptionDesc::Card`).
    ChooseCards,
    /// CR 701.22a: choose which of the cards you are looking at go to the bottom of your
    /// library (PickN; the rest stay on top).
    ScryBottom,
    /// CR 701.25a: choose which of the cards you are looking at go to your graveyard
    /// (PickN; the rest stay on top).
    SurveilGraveyard,
    /// Order cards going to the bottom of a library (CR 401.4; Order: the LAST listed ends
    /// up bottom-most, as for `MulliganBottom`).
    OrderBottom,
    /// CR 303.4f: choose what an Aura that is being put onto the battlefield enchants.
    AuraHost,
    /// CR 614.12a: "as this enters, you may [pay 2 life / reveal a card]. If you don't, it
    /// enters tapped" — `ability` of `object` (the card that is entering). Options:
    /// `Accept` (pay) or the `Card`s that may be revealed, then `Decline`. `tapped`: it
    /// enters tapped anyway (another effect, or the instruction that puts it there).
    AsEnters {
        object: ObjectId,
        ability: AbilityId,
        tapped: bool,
    },
    /// CR 601.2h: choose the cards to exile from your hand for a cost (PickN over `Card`).
    ExileFromHand,
    /// CR 601.2h: choose the permanents to return to their owner's hand for a cost.
    ReturnToHand,
    /// "Untap up to N lands": choose the permanents to untap (PickN over
    /// `OptionDesc::Card`, tapped permanents only).
    Untap,
    /// CR 701.34a: proliferate — choose any number of the permanents that have a counter
    /// (PickN over `OptionDesc::Card`).
    Proliferate,
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
    /// Activate non-mana activated ability `ability` of `source` (CR 602).
    ActivateAbility {
        source: ObjectId,
        ability: AbilityId,
    },
    /// Activate a `{T}` mana ability of `source` for one `mana` (CR 605; includes the
    /// intrinsic abilities of basic land types, CR 305.6).
    TapForMana {
        source: ObjectId,
        mana: ManaType,
        /// Which of the source's mana abilities, when it isn't one of the plain ones
        /// ("{T}: Add one mana", which are all one way to tap it): an ability that makes
        /// more, sacrifices the source, or makes mana with a restriction.
        ability: Option<AbilityId>,
    },
    /// A mode of a modal spell or ability (CR 700.2), by printed index.
    Mode {
        index: u8,
    },
    /// A legal target (CR 601.2c).
    Target {
        target: Target,
    },
    /// A mana type: a recipient of an `Allocate` of generic payment (CR 601.2h).
    Mana {
        mana: ManaType,
    },
    /// A replacement effect: ability `ability` of `source` (CR 616.1); `nth` tells apart
    /// the times `source` has that ability.
    Replacement {
        source: ObjectId,
        ability: AbilityId,
        nth: u8,
    },
    /// A prevention shield (CR 615.7, 616.1).
    Shield {
        id: u32,
    },
    /// A replacement or prevention effect a lasting effect carries (CR 611.2, 616.1):
    /// ability `index` of `GameState::lasting` entry `id`.
    Lasting {
        id: u32,
        index: u8,
    },
    /// "Yes" for an optional choice (e.g. CR 903.9a/b: put the commander into the command
    /// zone).
    Accept,
    /// "No" for an optional choice (decline an optional replacement effect).
    Decline,
    /// A triggered ability waiting to go on the stack (CR 603.3b).
    Trigger {
        source: ObjectId,
        ability: AbilityId,
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
    /// Play face `face` (not the front) of a card as a land (CR 712.9: a modal
    /// double-faced card's land face). `PlayLand` plays the front face.
    PlayLandFace {
        card: ObjectId,
        face: u8,
    },
    /// Cast face `face` (not the front) of a card (CR 712.8b). `CastSpell` casts the front
    /// face.
    CastFace {
        card: ObjectId,
        face: u8,
    },
    /// Cast `card` with its alternative cost `ability` (CR 118.9, 601.2b): "without paying
    /// its mana cost", flashback, evoke, overload. Offered right after `CastSpell` for the
    /// same card, in printed order.
    CastWith {
        card: ObjectId,
        /// The face being cast (0: the front, as for `CastSpell`; else as for `CastFace`).
        face: u8,
        ability: AbilityId,
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
