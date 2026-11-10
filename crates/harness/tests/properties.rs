//! Property tests (spec §10.4) over random seeds and setups (M0, M1 with 2–4 players).

use harness::{PlayOptions, Setup, feed, load_json, play_random, save_json};
use proptest::prelude::*;

fn setup_strategy() -> impl Strategy<Value = Setup> {
    prop_oneof![
        Just(Setup::M0Vanilla),
        (2u8..=4).prop_map(|players| Setup::M1 { players }),
        (2u8..=4).prop_map(|players| Setup::M2 { players }),
        (2u8..=4).prop_map(|players| Setup::Commander { players }),
        (2u8..=4).prop_map(|players| Setup::Real { players }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

    /// No panic; every object in exactly one zone after every step; the game terminates.
    #[test]
    fn random_games_keep_invariants(setup in setup_strategy(), seed in any::<u64>()) {
        let opts = PlayOptions { setup, check_invariants: true, ..Default::default() };
        let rec = play_random(seed, opts).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert!(rec.result.is_some());
    }

    /// serde round-trip at an arbitrary suspension point yields the same state hash and the
    /// same future.
    #[test]
    fn roundtrip_at_random_point(setup in setup_strategy(), seed in any::<u64>(), frac in 0.0f64..1.0) {
        let rec = play_random(seed, PlayOptions { setup, ..Default::default() }).unwrap();
        let k = ((rec.responses.len() as f64) * frac) as usize;
        let mut st = setup.game(seed);
        feed(&mut st, &rec.responses[..k]).unwrap();
        let mut loaded = load_json(&save_json(&st)).unwrap();
        prop_assert_eq!(loaded.state_hash(), st.state_hash());
        feed(&mut loaded, &rec.responses[k..]).unwrap();
        prop_assert_eq!(loaded.log_hash, rec.log_hash);
    }
}
