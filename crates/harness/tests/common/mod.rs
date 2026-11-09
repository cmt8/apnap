#![allow(dead_code)]

use engine_core::{ChoiceKind, Primitive};
use harness::GameRecord;

/// A coarse label for a decision: kind + primitive shape.
pub fn label(rec: &GameRecord, k: usize) -> String {
    let d = &rec.decisions[k];
    let shape = match d.prim {
        Primitive::PickN { .. } => "PickN",
        Primitive::PickNumber { .. } => "Number",
        Primitive::Allocate { .. } => "Allocate",
        Primitive::Order => "Order",
    };
    format!("{:?}/{shape}", d.kind)
}

/// Every decision label M0 can produce. Save/load + clone tests must hit all of them.
pub const M0_LABELS: &[&str] = &[
    "ChooseStartingPlayer/PickN",
    "Mulligan/PickN",
    "MulliganBottom/PickN",
    "MulliganBottom/Order",
    "Priority/PickN",
    "PayMana/PickN",
    "DeclareAttackers/PickN",
    "DeclareBlockers/PickN",
    "AssignCombatDamage/Allocate",
    "Discard/PickN",
];

/// M1 adds these (on top of the M0 labels). `Sacrifice/PickN` is a suspension in the
/// middle of resolution (APNAP each-player choice).
pub const M1_LABELS: &[&str] = &[
    "Priority/PickN",
    "PayMana/PickN",
    "PayMana/Allocate",
    "ChooseModes/PickN",
    "ChooseX/Number",
    "ChooseTargets { slot: 0 }/PickN",
    "Sacrifice/PickN",
    "Discard/PickN",
    "DeclareAttackers/PickN",
    "DeclareBlockers/PickN",
];

/// Indices worth suspending at: the first occurrence of each label, a mid-payment point
/// (PayMana right after another PayMana: mana already in pool), and every `stride`-th.
pub fn interesting_points(rec: &GameRecord, stride: usize) -> Vec<usize> {
    let mut ks = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for k in 0..rec.decisions.len() {
        if seen.insert(label(rec, k)) {
            ks.push(k);
        }
    }
    if let Some(k) = (1..rec.decisions.len()).find(|&k| {
        rec.decisions[k].kind == ChoiceKind::PayMana
            && rec.decisions[k - 1].kind == ChoiceKind::PayMana
    }) {
        ks.push(k);
    }
    ks.extend((0..rec.decisions.len()).step_by(stride));
    ks.push(rec.decisions.len()); // after the last response (game over)
    ks.sort_unstable();
    ks.dedup();
    ks
}
