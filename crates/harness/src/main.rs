//! `harness` CLI. `--setup` is `m0` (vanilla, 2 players) or `m1-Np` (M1 deck, N players);
//! the default is `m1-2p` for `play` and `m0` elsewhere.
//!   run [--setup S] [--seed N] [--games G] [--save-replay PATH]
//!   determinism [--setup S] [--seed N] [--runs R]
//!   fuzz [--setup S] [--seed N] [--games G] [--threads T]   panics/stuck games → exit 1
//!   replay PATH
//!   watch [--setup S] [--seed N]       narrate a random-agent game
//!   play [--setup S] [--seed N] [--save-replay PATH]   you (P0) vs random agents
//!   fetch-cards                        download the precons' printed card data into the cache
//!   coverage [--verbose]               per-precon script/verification status
//!   bless                              rewrite crates/harness/tests/golden/games.txt

use engine_core::{Agent, ChoiceKind, OptionDesc, PendingDecision, PlayerId, Primitive, Response};
use harness::text::{Narrator, board, decision_prompt};
use harness::{GOLDEN, PlayOptions, Replay, Setup, golden_line, play_random, run_replay};
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

fn setup(args: &[String], default: Setup) -> Setup {
    flag(args, "--setup").map_or(default, |v| {
        Setup::parse(&v).unwrap_or_else(|| panic!("bad --setup {v} (use m0 or m1-Np)"))
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("run");
    let opts = PlayOptions {
        setup: setup(&args, Setup::M0Vanilla),
        agents: flag(&args, "--agents").map_or(harness::Agents::Random, |a| {
            harness::Agents::parse(&a)
                .unwrap_or_else(|| panic!("bad --agents {a} (random | heuristic | h-vs-r)"))
        }),
        ..Default::default()
    };
    match cmd {
        "run" => {
            let seed = num(&args, "--seed", 42);
            let games = num(&args, "--games", 1);
            for s in seed..seed + games {
                match play_random(s, opts) {
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
                let rec = play_random(seed, opts).expect("game");
                println!(
                    "run {}: setup={} seed={seed} decisions={} events={} log_hash={:016x} state_hash={:016x}",
                    i + 1,
                    opts.setup.label(),
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
        "fuzz" => {
            let seed = num(&args, "--seed", 1);
            let games = num(&args, "--games", 100);
            let threads = num(&args, "--threads", 4) as usize;
            let results = harness::fuzz_with(opts, seed, games, threads);
            let mut wins = vec![0u64; usize::from(opts.setup.players())];
            let mut failed = 0;
            let (mut turns, mut decisions, mut draws) = (0u64, 0u64, 0u64);
            for (s, r) in &results {
                match r {
                    Ok(rec) => {
                        turns += u64::from(rec.turns);
                        decisions += rec.responses.len() as u64;
                        draws += u64::from(rec.result.as_ref().is_some_and(|r| r.winner.is_none()));
                        if let Some(w) = rec.result.as_ref().and_then(|r| r.winner) {
                            wins[w.index()] += 1;
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        eprintln!("seed={s} FAILED: {e}");
                    }
                }
            }
            let ok = results.len() as u64 - failed;
            println!(
                "fuzz {}: {} games, {ok} ok, {failed} failed, {draws} draws, avg turns {:.1}, avg decisions {:.0}",
                opts.setup.label(),
                results.len(),
                turns as f64 / ok.max(1) as f64,
                decisions as f64 / ok.max(1) as f64
            );
            println!("wins by seat: {wins:?}");
            if failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        "fetch-cards" => {
            // Downloads printed card data for the precon pool into the user's cache
            // (never into the repository).
            let names = harness::scryfall_card_names();
            match card_db::scryfall::fetch(&names) {
                Ok((data, fetched)) => {
                    println!(
                        "{} cards needed, {fetched} downloaded, {} now cached in {}",
                        names.len(),
                        data.cards.len(),
                        card_db::scryfall::cache_dir().display()
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("fetch failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "coverage" => {
            let data = match card_db::scryfall::cached(&harness::precon_card_names()) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            let verbose = args.iter().any(|a| a == "--verbose");
            let (mut total, mut done) = (0usize, 0usize);
            for file in harness::PRECON_DECKS {
                let d = harness::precon_list(file);
                let names: Vec<String> = d
                    .commander
                    .iter()
                    .cloned()
                    .chain(d.cards.iter().map(|(_, n)| n.clone()))
                    .collect();
                let mut counts = [0usize; 5];
                let mut lines = Vec::new();
                for n in &names {
                    let (i, note) = match harness::coverage_of(n, &data) {
                        harness::Coverage::BuiltIn => (0, String::new()),
                        harness::Coverage::Verified => (1, String::new()),
                        harness::Coverage::Unverified => (2, "scripted, not verified".into()),
                        harness::Coverage::Broken(p) => (3, format!("BROKEN: {}", p.join("; "))),
                        harness::Coverage::Missing { printed_problems }
                            if printed_problems.is_empty() =>
                        {
                            (4, "no script".into())
                        }
                        harness::Coverage::Missing { printed_problems } => (
                            4,
                            format!(
                                "no script; printed data needs: {}",
                                printed_problems.join("; ")
                            ),
                        ),
                    };
                    counts[i] += 1;
                    if i >= 2 && (verbose || i == 3) {
                        lines.push(format!("    {n}: {note}"));
                    }
                }
                let ok = counts[0] + counts[1];
                total += names.len();
                done += ok;
                println!(
                    "{}: {}/{} verified ({} built-in, {} verified, {} unverified, {} broken, {} missing)",
                    d.name,
                    ok,
                    names.len(),
                    counts[0],
                    counts[1],
                    counts[2],
                    counts[3],
                    counts[4]
                );
                for l in lines {
                    println!("{l}");
                }
            }
            println!(
                "total: {done}/{total} deck slots verified ({}%)",
                done * 100 / total.max(1)
            );
            ExitCode::SUCCESS
        }
        "watch" => {
            let seed = num(&args, "--seed", 42);
            let rec = play_random(seed, opts).expect("game");
            let mut n = Narrator::new(&opts.setup.game(seed));
            for line in n.lines(&rec.events, None) {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        "play" => play(&args),
        "bless" => {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/games.txt");
            let mut out = String::from(
                "# setup seed log_hash state_hash — regenerate ONLY with `cargo run -p harness -- bless`\n",
            );
            for &(setup, s) in GOLDEN {
                let opts = PlayOptions {
                    setup,
                    ..Default::default()
                };
                let rec = play_random(s, opts).expect("game");
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
            let only_pass_or_mana = d
                .options
                .iter()
                .all(|o| matches!(o, OptionDesc::Pass | OptionDesc::TapForMana { .. }));
            if d.kind == ChoiceKind::Priority && only_pass_or_mana {
                // Floating mana with nothing to spend it on: a UI stop, not a rules choice.
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
    let setup = setup(args, Setup::M1 { players: 2 });
    let mut st = setup.game(seed);
    let mut narrator = Narrator::new(&st);
    // Opponents: the heuristic AI by default (`--agents random` for random opponents).
    let kind = flag(args, "--agents").map_or(harness::Agents::Heuristic, |a| {
        harness::Agents::parse(&a).expect("bad --agents")
    });
    let mut bots: Vec<Box<dyn Agent>> = (0..setup.players())
        .map(|s| kind.make(seed, s, &st.catalog))
        .collect();
    let mut responses = Vec::new();
    println!(
        "apnap terminal play ({}) — you are P0 vs {} AI opponent(s), seed {seed}. 'q' quits.",
        setup.label(),
        setup.players() - 1
    );
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut out = engine_core::start(&mut st);
    loop {
        for l in narrator.lines(&out.events, Some(me)) {
            println!("{l}");
        }
        let Some(d) = out.next.clone() else { break };
        let r = if d.player != me {
            bots[d.player.index()].decide(&engine_core::player_view(&st, d.player), &d)
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
                    return save_and_exit(args, setup, seed, responses);
                };
                if line.trim() == "q" {
                    return save_and_exit(args, setup, seed, responses);
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
    save_and_exit(args, setup, seed, responses)
}

fn save_and_exit(args: &[String], setup: Setup, seed: u64, responses: Vec<Response>) -> ExitCode {
    if let Some(path) = flag(args, "--save-replay") {
        let replay = Replay {
            cr_version: engine_core::CR_VERSION.into(),
            setup,
            seed,
            responses,
        };
        std::fs::write(&path, serde_json::to_string_pretty(&replay).unwrap()).expect("write");
        println!("replay written to {path}");
    }
    ExitCode::SUCCESS
}
