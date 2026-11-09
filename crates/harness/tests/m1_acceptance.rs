//! M1 acceptance (spec §13 as amended): every verified card is covered by a test, and
//! four random agents complete games without panics or stuck games. The full
//! 1,000-game soak runs in CI: `harness fuzz --setup m1-4p --games 1000`.

use harness::{PlayOptions, Setup, fuzz, play_random};

/// Spec §10.2: "every `verified: true` card names at least one test. CI fails otherwise."
/// And the named tests must exist.
#[test]
fn every_verified_card_names_existing_tests() {
    const TEST_SOURCES: &[&str] = &[
        include_str!("rules_m0.rs"),
        include_str!("rules_m1.rs"),
        include_str!("rules_m2.rs"),
        include_str!("determinism.rs"),
    ];
    let sources: Vec<card_db::Source<'_>> = cards::CARDS
        .iter()
        .map(|&(path, text)| card_db::Source { path, text })
        .collect();
    let declared = card_db::declared_tests(&sources);
    assert_eq!(declared.len(), cards::CARDS.len(), "every card file parses");
    for (card, tests) in declared {
        assert!(!tests.is_empty(), "{card} names no test");
        for t in tests {
            let needle = format!("fn {t}(");
            assert!(
                TEST_SOURCES.iter().any(|s| s.contains(&needle)),
                "{card} names test `{t}`, which doesn't exist"
            );
        }
    }
}

#[test]
fn four_random_agents_complete_100_games() {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let results = fuzz(Setup::M1 { players: 4 }, 10_000, 100, threads);
    let failures: Vec<String> = results
        .iter()
        .filter_map(|(s, r)| r.as_ref().err().map(|e| format!("seed {s}: {e}")))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Structural invariants after every single step (slow, so fewer games).
#[test]
fn four_player_games_keep_invariants() {
    for seed in 0..8 {
        let opts = PlayOptions {
            setup: Setup::M1 { players: 4 },
            check_invariants: true,
            ..Default::default()
        };
        play_random(seed, opts).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    }
}
