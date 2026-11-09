//! The `watch`/`play` narrator names every object in every event (no `#id` placeholders),
//! and hides the opponent's draws in the player's log.

use harness::text::Narrator;
use harness::{PlayOptions, m0_game, play_random};

#[test]
fn narrator_names_everything() {
    for seed in 0..20 {
        let rec = play_random(seed, PlayOptions::default()).unwrap();
        let lines = Narrator::new(&m0_game(seed)).lines(&rec.events, None);
        assert!(lines.iter().any(|l| l.contains("game over")));
        for l in &lines {
            assert!(!l.contains('#'), "seed {seed}: unnamed object in {l:?}");
        }
        let hidden =
            Narrator::new(&m0_game(seed)).lines(&rec.events, Some(engine_core::PlayerId(0)));
        assert!(!hidden.iter().any(|l| l.starts_with("P1 draws Test")
            || l.starts_with("P1 draws Forest")
            || l.starts_with("P1 draws Mountain")));
    }
}
