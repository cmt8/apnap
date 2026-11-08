//! `harness` CLI. Commands:
//!   run [--seed N] [--games G] [--save-replay PATH]
//!   determinism [--seed N] [--runs R]
//!   replay PATH
//!   watch [--seed N]            narrate a random-agent game
//!   play [--seed N] [--save-replay PATH]   you (P0) vs a random agent, in the terminal
//!   bless                       (rewrite crates/harness/tests/golden/m0.txt)

use ai::RandomAgent;
use engine_core::{Agent, ChoiceKind, OptionDesc, PendingDecision, PlayerId, Primitive, Response};
use harness::text::{Narrator, board, decision_prompt};
use harness::{
    GOLDEN_SEEDS, PlayOptions, Replay, agent_seed, golden_line, m0_game, play_random, run_replay,
};
use std::io::{BufRead, Write};
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
        "watch" => {
            let seed = num(&args, "--seed", 42);
            let rec = play_random(seed, PlayOptions::default()).expect("game");
            let mut n = Narrator::new(&m0_game(seed));
            for line in n.lines(&rec.events, None) {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        "play" => play(&args),
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

/// Trivially forced decisions the terminal UI answers for you (a UI concern, spec §6
/// "stops"): priority when passing is the only option, and picks with exactly one answer.
fn forced(d: &PendingDecision) -> Option<Response> {
    match d.prim {
        Primitive::PickN { min, max } => {
            let n = d.options.len();
            if d.kind == ChoiceKind::Priority && n == 1 {
                Some(Response::Pick(vec![0]))
            } else if min == n && max == n {
                Some(Response::Pick((0..n).collect()))
            } else if n == 1 && min == 1 {
                Some(Response::Pick(vec![0]))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_response(d: &PendingDecision, line: &str) -> Result<Response, String> {
    let nums: Result<Vec<u64>, _> = line.split_whitespace().map(str::parse).collect();
    let nums = nums.map_err(|_| "enter numbers separated by spaces".to_string())?;
    let r = match d.prim {
        Primitive::PickN { .. } => Response::Pick(nums.iter().map(|&x| x as usize).collect()),
        Primitive::PickNumber { .. } => match nums.as_slice() {
            [x] => Response::Number(*x as u32),
            _ => return Err("enter one number".into()),
        },
        Primitive::Allocate { .. } => Response::Allocate(nums.iter().map(|&x| x as u32).collect()),
        Primitive::Order => Response::Order(nums.iter().map(|&x| x as usize).collect()),
    };
    d.validate(&r).map_err(|e| format!("not legal: {e}"))?;
    Ok(r)
}

fn play(args: &[String]) -> ExitCode {
    let seed = flag(args, "--seed").map_or_else(
        || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_secs())
        },
        |v| v.parse().expect("bad --seed"),
    );
    let me = PlayerId(0);
    let mut st = m0_game(seed);
    let mut narrator = Narrator::new(&st);
    let mut bot = RandomAgent::new(agent_seed(seed, 1));
    let mut responses = Vec::new();
    println!(
        "apnap M0 terminal play — you are P0, seed {seed}. 'q' quits. Cards are vanilla test cards."
    );
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut out = engine_core::start(&mut st);
    loop {
        for l in narrator.lines(&out.events, Some(PlayerId(1))) {
            println!("{l}");
        }
        let Some(d) = out.next.clone() else { break };
        let r = if d.player != me {
            bot.decide(&engine_core::player_view(&st, d.player), &d)
        } else if let Some(r) = forced(&d) {
            r
        } else {
            let v = engine_core::player_view(&st, me);
            if d.kind == ChoiceKind::Priority || matches!(d.options.first(), Some(OptionDesc::Keep))
            {
                print!("\n{}", board(&v));
            }
            loop {
                print!("{}> ", decision_prompt(&v, &d));
                std::io::stdout().flush().ok();
                let Some(Ok(line)) = lines.next() else {
                    return save_and_exit(args, seed, responses);
                };
                if line.trim() == "q" {
                    return save_and_exit(args, seed, responses);
                }
                match parse_response(&d, &line) {
                    Ok(r) => break r,
                    Err(e) => println!("  {e}"),
                }
            }
        };
        responses.push(r.clone());
        out = engine_core::step(&mut st, r).expect("validated response");
    }
    save_and_exit(args, seed, responses)
}

fn save_and_exit(args: &[String], seed: u64, responses: Vec<Response>) -> ExitCode {
    if let Some(path) = flag(args, "--save-replay") {
        let replay = Replay {
            cr_version: engine_core::CR_VERSION.into(),
            seed,
            responses,
        };
        std::fs::write(&path, serde_json::to_string_pretty(&replay).unwrap()).expect("write");
        println!("replay written to {path}");
    }
    ExitCode::SUCCESS
}
