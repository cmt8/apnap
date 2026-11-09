//! Determinism acceptance criteria (spec §13 M0, re-checked for every M1 setup):
//!  1. random agents play full games (M0: two players, vanilla; M1: 2 and 4 players);
//!  2. same seed + responses → identical event hash across 3 runs and across a mid-game
//!     save/load;
//!  3. `clone()` of a mid-game state, then the same responses → identical events.
//!
//! Save/load and clone are exercised at the first occurrence of every decision kind
//! (including mid-cast payment and mid-resolution APNAP choices) plus regular samples.

mod common;

use common::{M0_LABELS, M1_LABELS, M2_LABELS, interesting_points, label};
use engine_core::state::GameState;
use harness::{GameRecord, PlayOptions, Setup, feed, load_json, play_random, save_json};
use std::collections::BTreeSet;

const M1_2P: Setup = Setup::M1 { players: 2 };
const M1_4P: Setup = Setup::M1 { players: 4 };
const M2_2P: Setup = Setup::M2 { players: 2 };
const M2_4P: Setup = Setup::M2 { players: 4 };

/// (setup, seed). M0 seed 4 exercises every M0 decision kind (incl. cleanup discard).
const CASES: &[(Setup, u64)] = &[
    (Setup::M0Vanilla, 1),
    (Setup::M0Vanilla, 2),
    (Setup::M0Vanilla, 4),
    (Setup::M0Vanilla, 42),
    (M1_2P, 1),
    (M1_2P, 2),
    (M1_2P, 3),
    (M1_4P, 1),
    (M1_4P, 2),
    // M2 2p seeds chosen to cover the rare decisions: 13 OrderTriggers, 45 LegendRule,
    // 28 ChooseReplacement (mid-action suspensions).
    (M2_2P, 13),
    (M2_2P, 45),
    (M2_2P, 28),
    (M2_4P, 1),
];

/// Number of sample points per game (besides first-of-each-kind points).
const SAMPLES: usize = 25;

fn record(setup: Setup, seed: u64) -> GameRecord {
    play_random(
        seed,
        PlayOptions {
            setup,
            check_invariants: true,
            ..Default::default()
        },
    )
    .unwrap()
}

fn points(rec: &GameRecord) -> Vec<usize> {
    interesting_points(rec, (rec.decisions.len() / SAMPLES).max(1))
}

/// Which milestone's label list a setup must cover.
fn tier(setup: Setup) -> u8 {
    match setup {
        Setup::M0Vanilla => 0,
        Setup::M1 { .. } => 1,
        Setup::M2 { .. } => 2,
    }
}

/// The labels each tier's cases must cover between them.
fn assert_covered(covered: &BTreeSet<(u8, String)>, what: &str) {
    for (t, labels) in [(0, M0_LABELS), (1, M1_LABELS), (2, M2_LABELS)] {
        for l in labels {
            assert!(
                covered.contains(&(t, l.to_string())),
                "{what} never suspended at {l} in M{t} cases; add a seed"
            );
        }
    }
}

/// Rebuild the state as it was just before response `k` was submitted.
fn state_at(rec: &GameRecord, k: usize) -> GameState {
    let mut st = rec.setup.game(rec.seed);
    let events = feed(&mut st, &rec.responses[..k]).unwrap();
    assert_eq!(
        events,
        rec.events[..prefix_len(rec, k)],
        "prefix events differ at k={k}"
    );
    st
}

fn prefix_len(rec: &GameRecord, k: usize) -> usize {
    rec.events_before
        .get(k)
        .copied()
        .unwrap_or(rec.events.len())
}

#[test]
fn criterion_1_full_game_completes() {
    for &(setup, seed) in CASES {
        let rec = record(setup, seed);
        let result = rec.result.as_ref().expect("game finished");
        assert!(result.winner.is_some() || rec.turns > 0);
        assert!(rec.turns >= 2, "seed {seed}: suspiciously short game");
    }
}

#[test]
fn criterion_2a_three_runs_identical() {
    for &(setup, seed) in CASES {
        let runs: Vec<_> = (0..3).map(|_| record(setup, seed)).collect();
        for r in &runs[1..] {
            assert_eq!(
                r.log_hash, runs[0].log_hash,
                "seed {seed}: event hash differs"
            );
            assert_eq!(
                r.state_hash, runs[0].state_hash,
                "seed {seed}: state hash differs"
            );
            assert_eq!(r.responses, runs[0].responses);
            assert_eq!(r.events, runs[0].events);
        }
    }
}

#[test]
fn criterion_2b_replay_from_responses_is_identical() {
    for &(setup, seed) in CASES {
        let rec = record(setup, seed);
        let (st, events) = harness::run_replay(&rec.replay()).unwrap();
        assert_eq!(events, rec.events);
        assert_eq!(st.log_hash, rec.log_hash);
        assert_eq!(st.state_hash(), rec.state_hash);
        assert!(st.is_over());
    }
}

#[test]
fn criterion_2c_save_load_mid_game_replays_identically() {
    let mut covered = BTreeSet::new();
    for &(setup, seed) in CASES {
        let rec = record(setup, seed);
        let m1 = tier(setup);
        for k in points(&rec) {
            let st = state_at(&rec, k);
            if k < rec.decisions.len() {
                covered.insert((m1, label(&rec, k)));
                assert_eq!(st.pending.as_ref(), Some(&rec.decisions[k]));
            }
            let json = save_json(&st);
            let mut loaded = load_json(&json).expect("load");
            assert_eq!(
                loaded.state_hash(),
                st.state_hash(),
                "seed {seed} k={k}: round-trip hash"
            );
            assert_eq!(
                save_json(&loaded),
                json,
                "seed {seed} k={k}: re-serialization differs"
            );
            let suffix = feed(&mut loaded, &rec.responses[k..]).unwrap();
            assert_eq!(
                suffix,
                rec.events[prefix_len(&rec, k)..],
                "seed {seed} k={k}: events after load"
            );
            assert_eq!(
                loaded.log_hash, rec.log_hash,
                "seed {seed} k={k}: event hash after load"
            );
            assert_eq!(loaded.state_hash(), rec.state_hash);
        }
    }
    assert_covered(&covered, "save/load");
}

#[test]
fn criterion_3_clone_mid_game_replays_identically() {
    let mut covered = BTreeSet::new();
    for &(setup, seed) in CASES {
        let rec = record(setup, seed);
        let m1 = tier(setup);
        for k in points(&rec) {
            let mut original = state_at(&rec, k);
            let mut cloned = original.clone();
            if k < rec.decisions.len() {
                covered.insert((m1, label(&rec, k)));
            }
            let a = feed(&mut cloned, &rec.responses[k..]).unwrap();
            let b = feed(&mut original, &rec.responses[k..]).unwrap();
            assert_eq!(a, b, "seed {seed} k={k}: clone diverged");
            assert_eq!(a, rec.events[prefix_len(&rec, k)..]);
            assert_eq!(cloned.log_hash, rec.log_hash);
            assert_eq!(cloned.state_hash(), original.state_hash());
        }
    }
    assert_covered(&covered, "clone");
}

#[test]
fn illegal_responses_leave_state_unchanged() {
    use engine_core::{IllegalResponse, Response};
    for &(setup, seed) in &[(Setup::M0Vanilla, 42), (M1_4P, 1)] {
        let rec = record(setup, seed);
        for k in points(&rec) {
            if k >= rec.decisions.len() {
                let mut st = state_at(&rec, k);
                assert_eq!(
                    engine_core::step(&mut st, Response::Pick(vec![0])),
                    Err(IllegalResponse::NoPendingDecision)
                );
                continue;
            }
            let mut st = state_at(&rec, k);
            let before = st.state_hash();
            let n = rec.decisions[k].options.len();
            let bad = [
                Response::Number(u32::MAX),
                Response::Pick(vec![n]),
                Response::Pick(vec![0; n + 2]),
                Response::Order(vec![0; n + 1]),
                Response::Allocate(vec![u32::MAX; n.max(1)]),
            ];
            for r in bad {
                if rec.decisions[k].validate(&r).is_ok() {
                    continue;
                }
                assert!(
                    engine_core::step(&mut st, r.clone()).is_err(),
                    "k={k} accepted {r:?}"
                );
                assert_eq!(
                    st.state_hash(),
                    before,
                    "k={k}: rejected {r:?} mutated state"
                );
            }
            let rest = feed(&mut st, &rec.responses[k..]).unwrap();
            assert_eq!(rest, rec.events[prefix_len(&rec, k)..]);
        }
    }
}
