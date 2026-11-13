//! The `watch`/`play` narrator names every object in every event (no `#id` placeholders),
//! and hides the opponent's draws in the player's log.

use engine_core::client::Seat;
use harness::text::narrate;
use harness::{PlayOptions, m0_game, play_random};

#[test]
fn narrator_names_everything() {
    for seed in 0..20 {
        let rec = play_random(seed, PlayOptions::default()).unwrap();
        let lines = narrate(&m0_game(seed), &rec.events, Seat::Everyone);
        assert!(lines.iter().any(|l| l.contains("game over")));
        for l in &lines {
            assert!(!l.contains('#'), "seed {seed}: unnamed object in {l:?}");
        }
        let hidden = narrate(
            &m0_game(seed),
            &rec.events,
            Seat::Player(engine_core::PlayerId(0)),
        );
        for l in &hidden {
            assert!(
                !l.contains('#'),
                "seed {seed}: unnamed object in P0's log: {l:?}"
            );
        }
        assert!(!hidden.iter().any(|l| l.starts_with("P1 draws Test")
            || l.starts_with("P1 draws Forest")
            || l.starts_with("P1 draws Mountain")));
    }
}
