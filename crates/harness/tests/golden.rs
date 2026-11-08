//! Golden hashes (spec §10.3). These pin the exact event stream and final state for fixed
//! seeds, on every platform CI runs on. Regenerate ONLY via `cargo run -p harness -- bless`
//! and review why they changed.

use harness::{PlayOptions, golden_line, play_random};

#[test]
fn golden_m0_hashes() {
    let text = include_str!("golden/m0.txt");
    let mut n = 0;
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let seed: u64 = line.split_whitespace().next().unwrap().parse().unwrap();
        let rec = play_random(seed, PlayOptions::default()).unwrap();
        assert_eq!(
            golden_line(&rec),
            line,
            "golden hash changed for seed {seed}"
        );
        n += 1;
    }
    assert_eq!(n, harness::GOLDEN_SEEDS.len());
}
