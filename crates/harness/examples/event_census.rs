//! Count event kinds across random games: `cargo run --release -p harness --example
//! event_census -- m1-4p 50`. A quick check that every mechanic actually occurs.

use harness::{PlayOptions, Setup, play_random};
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let setup = Setup::parse(args.first().map_or("m1-4p", |s| s.as_str())).expect("setup");
    let games: u64 = args.get(1).map_or(50, |s| s.parse().unwrap());
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    for seed in 0..games {
        let rec = play_random(
            seed,
            PlayOptions {
                setup,
                ..Default::default()
            },
        )
        .unwrap();
        for e in &rec.events {
            let name = format!("{e:?}");
            let name = name.split([' ', '{', '(']).next().unwrap().to_string();
            *counts.entry(name).or_default() += 1;
        }
        for d in &rec.decisions {
            *kinds
                .entry(format!(
                    "{:?}/{:?}",
                    d.kind,
                    std::mem::discriminant(&d.prim)
                ))
                .or_default() += 1;
        }
    }
    for (k, v) in counts {
        println!("{v:>9}  {k}");
    }
    println!("--- decisions");
    for (k, v) in kinds {
        println!("{v:>9}  {k}");
    }
}
