//! Property tests (spec §10.4) over random seeds.

use harness::{PlayOptions, feed, load_json, m0_game, play_random, save_json};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// No panic; every object in exactly one zone after every step; the game terminates.
    #[test]
    fn random_games_keep_invariants(seed in any::<u64>()) {
        let rec = play_random(seed, PlayOptions { check_invariants: true, ..Default::default() })
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert!(rec.result.is_some());
    }

    /// serde round-trip at an arbitrary suspension point yields the same state hash and the
    /// same future.
    #[test]
    fn roundtrip_at_random_point(seed in any::<u64>(), frac in 0.0f64..1.0) {
        let rec = play_random(seed, PlayOptions::default()).unwrap();
        let k = ((rec.responses.len() as f64) * frac) as usize;
        let mut st = m0_game(seed);
        feed(&mut st, &rec.responses[..k]).unwrap();
        let mut loaded = load_json(&save_json(&st)).unwrap();
        prop_assert_eq!(loaded.state_hash(), st.state_hash());
        feed(&mut loaded, &rec.responses[k..]).unwrap();
        prop_assert_eq!(loaded.log_hash, rec.log_hash);
    }
}
