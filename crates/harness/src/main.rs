//! `harness` CLI. Commands:
//!   run [--seed N] [--games G] [--save-replay PATH]
//!   determinism [--seed N] [--runs R]
//!   replay PATH
//!   bless                       (rewrite crates/harness/tests/golden/m0.txt)

use harness::{GOLDEN_SEEDS, PlayOptions, Replay, golden_line, play_random, run_replay};
use std::process::ExitCode;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn num(args: &[String], name: &str, default: u64) -> u64 {
    flag(args, name).map_or(default, |v| {
        v.parse().unwrap_or_else(|_| panic!("bad {name}"))
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("run");
    match cmd {
        "run" => {
            let seed = num(&args, "--seed", 42);
            let games = num(&args, "--games", 1);
            for s in seed..seed + games {
                match play_random(s, PlayOptions::default()) {
                    Ok(rec) => {
                        println!(
                            "seed={s} winner={:?} turns={} decisions={} events={} log_hash={:016x} state_hash={:016x}",
                            rec.result.as_ref().and_then(|r| r.winner),
                            rec.turns,
                            rec.responses.len(),
                            rec.events.len(),
                            rec.log_hash,
                            rec.state_hash
                        );
                        if let Some(path) = flag(&args, "--save-replay") {
                            let json = serde_json::to_string_pretty(&rec.replay()).unwrap();
                            std::fs::write(&path, json).expect("write replay");
                            println!("replay written to {path}");
                        }
                    }
                    Err(e) => {
                        eprintln!("seed={s} FAILED: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            ExitCode::SUCCESS
        }
        "determinism" => {
            let seed = num(&args, "--seed", 42);
            let runs = num(&args, "--runs", 3);
            let mut hashes = Vec::new();
            for i in 0..runs {
                let rec = play_random(seed, PlayOptions::default()).expect("game");
                println!(
                    "run {}: seed={seed} decisions={} events={} log_hash={:016x} state_hash={:016x}",
                    i + 1,
                    rec.responses.len(),
                    rec.events.len(),
                    rec.log_hash,
                    rec.state_hash
                );
                hashes.push((rec.log_hash, rec.state_hash));
            }
            if hashes.windows(2).all(|w| w[0] == w[1]) {
                println!("OK: {runs} runs identical");
                ExitCode::SUCCESS
            } else {
                eprintln!("MISMATCH across runs");
                ExitCode::FAILURE
            }
        }
        "replay" => {
            let path = args.get(1).expect("replay PATH");
            let replay: Replay =
                serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("parse");
            let (st, events) = run_replay(&replay).expect("replay");
            println!(
                "replayed {} responses, {} events, over={} log_hash={:016x} state_hash={:016x}",
                replay.responses.len(),
                events.len(),
                st.is_over(),
                st.log_hash,
                st.state_hash()
            );
            ExitCode::SUCCESS
        }
        "bless" => {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/m0.txt");
            let mut out = String::from(
                "# seed log_hash state_hash — regenerate ONLY with `cargo run -p harness -- bless`\n",
            );
            for &s in GOLDEN_SEEDS {
                let rec = play_random(s, PlayOptions::default()).expect("game");
                out.push_str(&golden_line(&rec));
                out.push('\n');
            }
            std::fs::write(path, &out).expect("write golden");
            print!("{out}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command {other}; see crates/harness/src/main.rs");
            ExitCode::FAILURE
        }
    }
}
