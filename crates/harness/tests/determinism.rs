//! M0 acceptance criteria (spec §13):
//!  1. two players with vanilla creatures and basic lands play a full game via random agents;
//!  2. same seed + responses → identical event hash across 3 runs and across a mid-game
//!     save/load;
//!  3. `clone()` of a mid-game state, then the same responses → identical events.

mod common;

use common::{M0_LABELS, interesting_points, label};
use engine_core::state::GameState;
use harness::{PlayOptions, feed, load_json, m0_game, play_random, save_json};
use std::collections::BTreeSet;

// Seed 4 exercises every M0 decision kind (incl. cleanup discard) in one game.
const SEEDS: &[u64] = &[1, 2, 3, 4, 42, 1337];

fn record(seed: u64) -> harness::GameRecord {
    play_random(
        seed,
        PlayOptions {
            check_invariants: true,
            ..Default::default()
        },
    )
    .unwrap()
}

/// Rebuild the state as it was just before response `k` was submitted.
fn state_at(rec: &harness::GameRecord, k: usize) -> GameState {
    let mut st = m0_game(rec.seed);
    let events = feed(&mut st, &rec.responses[..k]).unwrap();
    assert_eq!(
        events,
        rec.events[..prefix_len(rec, k)],
        "prefix events differ at k={k}"
    );
    st
}

fn prefix_len(rec: &harness::GameRecord, k: usize) -> usize {
    rec.events_before
        .get(k)
        .copied()
        .unwrap_or(rec.events.len())
}

#[test]
fn criterion_1_full_game_completes() {
    for &seed in SEEDS {
        let rec = record(seed);
        let result = rec.result.as_ref().expect("game finished");
        assert!(result.winner.is_some() || rec.turns > 0);
        assert!(rec.turns >= 2, "seed {seed}: suspiciously short game");
    }
}

#[test]
fn criterion_2a_three_runs_identical() {
    for &seed in SEEDS {
        let runs: Vec<_> = (0..3).map(|_| record(seed)).collect();
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
    for &seed in SEEDS {
        let rec = record(seed);
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
    for &seed in SEEDS {
        let rec = record(seed);
        for k in interesting_points(&rec, 37) {
            let st = state_at(&rec, k);
            if k < rec.decisions.len() {
                covered.insert(label(&rec, k));
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
    for l in M0_LABELS {
        assert!(
            covered.contains(*l),
            "save/load never suspended at {l}; add a seed"
        );
    }
}

#[test]
fn criterion_3_clone_mid_game_replays_identically() {
    let mut covered = BTreeSet::new();
    for &seed in SEEDS {
        let rec = record(seed);
        for k in interesting_points(&rec, 41) {
            let mut original = state_at(&rec, k);
            let mut cloned = original.clone();
            if k < rec.decisions.len() {
                covered.insert(label(&rec, k));
            }
            let a = feed(&mut cloned, &rec.responses[k..]).unwrap();
            let b = feed(&mut original, &rec.responses[k..]).unwrap();
            assert_eq!(a, b, "seed {seed} k={k}: clone diverged");
            assert_eq!(a, rec.events[prefix_len(&rec, k)..]);
            assert_eq!(cloned.log_hash, rec.log_hash);
            assert_eq!(cloned.state_hash(), original.state_hash());
        }
    }
    for l in M0_LABELS {
        assert!(
            covered.contains(*l),
            "clone test never suspended at {l}; add a seed"
        );
    }
}

#[test]
fn illegal_responses_leave_state_unchanged() {
    use engine_core::{IllegalResponse, Response};
    let rec = record(42);
    for k in interesting_points(&rec, 53) {
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
