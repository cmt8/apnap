//! Golden hashes (spec §10.3). These pin the exact event stream and final state for fixed
//! (setup, seed) pairs, on every platform CI runs on. Regenerate ONLY via
//! `cargo run -p harness -- bless` and review why they changed.

use harness::{PlayOptions, Setup, golden_line, play_random};

#[test]
fn golden_hashes() {
    let text = include_str!("golden/games.txt");
    let mut n = 0;
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let mut parts = line.split_whitespace();
        let setup = Setup::parse(parts.next().unwrap()).expect("setup label");
        let seed: u64 = parts.next().unwrap().parse().expect("seed");
        let opts = PlayOptions {
            setup,
            ..Default::default()
        };
        let rec = play_random(seed, opts).unwrap();
        assert_eq!(
            golden_line(&rec),
            line,
            "golden hash changed for {} seed {seed}",
            setup.label()
        );
        n += 1;
    }
    assert_eq!(n, harness::GOLDEN.len());
}
