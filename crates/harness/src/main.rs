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
//!   fetch-forge                        clone Forge's card scripts (pinned commit) into the cache
//!   fetch-bulk                         Scryfall's Oracle card file (every card) into the cache
//!   forge-coverage [--top N] [--ranked [N]]  how many Forge scripts compile; what blocks the rest (by EDHREC rank)
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

/// The feature a blocker belongs to: "param:trig Drawn.Number" and "trig:Drawn …" are both
/// the trigger mode `trig:Drawn`; "param:Pump.KW" is the effect `api:Pump`.
fn family(blocker: &str) -> String {
    let word = |s: &str| {
        s.split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or(s)
            .to_string()
    };
    if let Some(rest) = blocker.strip_prefix("param:") {
        return match rest.split_once(' ') {
            Some((kind @ ("trig" | "static" | "repl"), rest)) => format!("{kind}:{}", word(rest)),
            _ => format!("api:{}", word(rest)),
        };
    }
    match blocker.split_once(':') {
        Some((kind @ ("trig" | "static" | "repl" | "kw" | "api" | "cost"), rest)) => {
            format!("{kind}:{}", word(rest))
        }
        Some((kind, _)) if !kind.contains(' ') => {
            // "Dig:each player chooses", "ChangeZone:no-target": the effect.
            if kind.chars().next().is_some_and(char::is_uppercase) {
                format!("api:{kind}")
            } else {
                blocker.to_string()
            }
        }
        _ => blocker.to_string(),
    }
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
            let (mut failed, mut runaway) = (0u64, 0u64);
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
                    // A board that grows without bound: abandoned, not a failure.
                    Err(e @ harness::HarnessError::Runaway { .. }) => {
                        runaway += 1;
                        eprintln!("seed={s} abandoned: {e}");
                    }
                    Err(e) => {
                        failed += 1;
                        eprintln!("seed={s} FAILED: {e}");
                    }
                }
            }
            let ok = results.len() as u64 - failed - runaway;
            println!(
                "fuzz {}: {} games, {ok} ok, {failed} failed, {runaway} abandoned (runaway board), {draws} draws, avg turns {:.1}, avg decisions {:.0}",
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
        "fetch-forge" => match card_db::forge::fetch() {
            Ok(n) => {
                println!(
                    "{n} Forge card scripts at commit {} in {}",
                    &card_db::forge::FORGE_COMMIT[..8],
                    card_db::forge::repo_dir().display()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("fetch failed: {e}");
                ExitCode::FAILURE
            }
        },
        "fetch-bulk" => match card_db::scryfall::fetch_bulk() {
            Ok(bytes) => {
                println!(
                    "Scryfall Oracle card file: {:.1} MB in {}",
                    bytes as f64 / 1e6,
                    card_db::scryfall::cache_dir().display()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("fetch failed: {e}");
                ExitCode::FAILURE
            }
        },
        "forge-coverage" => {
            // How many of Forge's card scripts compile, and which missing feature blocks
            // the most cards (decisions.md D-046).
            let bulk = card_db::scryfall::load_bulk();
            if let Err(e) = &bulk {
                eprintln!("(no cross-check against Scryfall: {e})");
            }
            let lib = match card_db::forge::Library::load(
                &card_db::forge::cards_dir(),
                &card_db::forge::tokens_dir(),
                bulk.as_ref().ok(),
            ) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            let top = num(&args, "--top", 40) as usize;
            // `--sample N`: print every N-th compiled card as the engine understands it,
            // next to its Oracle text, for reading over.
            let sample = num(&args, "--sample", 0) as usize;
            // `--with TEXT`: sample only among the cards whose text, as the engine understands
            // them, contains TEXT (to read over what a new feature compiled).
            let with = args
                .iter()
                .position(|a| a == "--with")
                .and_then(|i| args.get(i + 1));
            if sample > 0 {
                let shown = lib.cards.iter().filter(|(_, c)| {
                    with.is_none_or(|t| {
                        card_model::describe::rules_text(&c.def)
                            .iter()
                            .any(|line| line.contains(t.as_str()))
                    })
                });
                for (i, (name, c)) in shown.enumerate() {
                    if i % sample != sample / 2 {
                        continue;
                    }
                    let oracle = bulk
                        .as_ref()
                        .ok()
                        .and_then(|d| d.get(name))
                        .and_then(|c| c.oracle_text.clone())
                        .unwrap_or_default();
                    println!(
                        "== {} | {} | {}\n   oracle: {}\n   engine: {}",
                        name,
                        c.def
                            .mana_cost
                            .as_ref()
                            .map_or(String::new(), |m| m.to_text()),
                        card_model::describe::type_line(&c.def),
                        oracle.replace('\n', " / "),
                        card_model::describe::rules_text(&c.def).join(" / ")
                    );
                }
            }
            // `--why <text>`: the blocked cards with a blocker containing <text>, each with
            // everything that blocks it.
            if let Some(i) = args.iter().position(|a| a == "--why")
                && let Some(text) = args.get(i + 1)
            {
                let hits: Vec<_> = lib
                    .blocked
                    .iter()
                    .filter(|(_, b)| b.iter().any(|x| x.contains(text.as_str())))
                    .collect();
                // The first 60, or with `--all` every one.
                let limit = if args.iter().any(|a| a == "--all") {
                    usize::MAX
                } else {
                    60
                };
                for (name, b) in hits.iter().take(limit) {
                    let all: Vec<&str> = b.iter().map(String::as_str).collect();
                    println!("{name}: {}", all.join(" | "));
                }
                println!("({} cards)", hits.len());
                return ExitCode::SUCCESS;
            }
            let total = lib.cards.len() + lib.blocked.len();
            println!(
                "{} of {total} Forge card scripts compile ({:.1}%); {} unreadable; tokens: {} of {} compile",
                lib.cards.len(),
                100.0 * lib.cards.len() as f64 / total.max(1) as f64,
                lib.unreadable,
                lib.tokens.len(),
                lib.tokens.len() + lib.blocked_tokens.len(),
            );
            if let Ok(bulk) = &bulk {
                // The cards that matter: real ones, per Scryfall.
                let real: Vec<&String> = bulk
                    .cards
                    .iter()
                    .filter(|(_, c)| c.is_real())
                    .map(|(n, _)| n)
                    .collect();
                let front = |n: &str| n.split(" // ").next().unwrap_or(n).to_string();
                let ok = real
                    .iter()
                    .filter(|n| lib.cards.contains_key(&front(n)))
                    .count();
                let none = real
                    .iter()
                    .filter(|n| {
                        !lib.cards.contains_key(&front(n)) && !lib.blocked.contains_key(&front(n))
                    })
                    .count();
                println!(
                    "{ok} of {} real cards (per Scryfall) compile ({:.1}%); Forge has no script for {none}",
                    real.len(),
                    100.0 * ok as f64 / real.len().max(1) as f64
                );
            }
            // `--ranked [N]`: the same by EDHREC popularity (Commander-legal cards), with the
            // blockers of the top N (default 1,000) broken down.
            if let Some(i) = args.iter().position(|a| a == "--ranked") {
                let n = args
                    .get(i + 1)
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(1000);
                match card_db::scryfall::load_bulk_ranks() {
                    Ok(ranks) => {
                        let report = harness::ranked::ranked(
                            ranks,
                            &|s| lib.cards.contains_key(s),
                            &lib.blocked,
                            n,
                        );
                        print!("{report}");
                    }
                    Err(e) => println!("(no ranking: {e})"),
                }
            }
            // How many cards each blocker stops, and for how many it is the only one.
            // `--families`: rank the kinds of feature instead of the single blockers. A
            // trigger mode, static mode, effect or keyword counts together with its own
            // parameters ("trig:Drawn" with "param:trig Drawn.ValidCard"), which never
            // block alone.
            if args.iter().any(|a| a == "--families") {
                let mut rows: std::collections::BTreeMap<String, (usize, usize)> =
                    Default::default();
                for blockers in lib.blocked.values() {
                    let mut fams: Vec<String> = blockers.iter().map(|b| family(b)).collect();
                    fams.sort();
                    fams.dedup();
                    for f in &fams {
                        let e = rows.entry(f.clone()).or_default();
                        e.0 += 1;
                        if fams.len() == 1 {
                            e.1 += 1;
                        }
                    }
                }
                let mut rows: Vec<(String, (usize, usize))> = rows.into_iter().collect();
                rows.sort_by_key(|a| std::cmp::Reverse((a.1.1, a.1.0)));
                println!("family — cards it alone blocks / cards it blocks at all");
                for (name, (all, alone)) in rows.iter().take(top) {
                    println!("  {alone:6} / {all:6}  {name}");
                }
                return ExitCode::SUCCESS;
            }
            let mut blocks: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
            let mut one_away = 0usize;
            for blockers in lib.blocked.values() {
                if blockers.len() == 1 {
                    one_away += 1;
                }
                for b in blockers {
                    let e = blocks.entry(b.as_str()).or_default();
                    e.0 += 1;
                    if blockers.len() == 1 {
                        e.1 += 1;
                    }
                }
            }
            println!("{one_away} cards are one feature away");
            let mut rows: Vec<(&str, (usize, usize))> = blocks.into_iter().collect();
            rows.sort_by_key(|a| std::cmp::Reverse((a.1.1, a.1.0)));
            if top > 0 {
                println!("\nblocker — cards it alone blocks / cards it blocks at all");
            }
            for (name, (all, alone)) in rows.iter().take(top) {
                println!("  {alone:6} / {all:6}  {name}");
            }
            println!("  ({} distinct blockers)", rows.len());
            // Features that only ever block together (an Aura needs both "enchant" and
            // "enchanted creature gets…") never show up as "alone": list the sets of two
            // or three blockers that are all that stops the most cards.
            let mut sets: std::collections::BTreeMap<Vec<&str>, usize> = Default::default();
            for blockers in lib.blocked.values() {
                if (2..=3).contains(&blockers.len()) {
                    *sets
                        .entry(blockers.iter().map(String::as_str).collect())
                        .or_default() += 1;
                }
            }
            let mut sets: Vec<(Vec<&str>, usize)> = sets.into_iter().collect();
            sets.sort_by_key(|(names, n)| (std::cmp::Reverse(*n), names.clone()));
            if top > 0 {
                println!("\nsets of two or three blockers — cards they alone block together");
            }
            for (names, n) in sets.iter().take(top / 2) {
                println!("  {n:6}  {}", names.join("  +  "));
            }
            ExitCode::SUCCESS
        }
        "coverage" => {
            // The four precons: how many of each deck's cards compile from Forge's scripts.
            let lib = match harness::try_forge_library() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            let base = harness::catalog();
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
                let mut ok = 0;
                let mut lines = Vec::new();
                for n in &names {
                    if base.find(n).is_some() || lib.cards.contains_key(n) {
                        ok += 1;
                    } else {
                        let why = lib
                            .blocked
                            .get(n)
                            .map_or("Forge has no script".to_string(), |b| {
                                b.iter().cloned().collect::<Vec<_>>().join(", ")
                            });
                        lines.push(format!("    {n}: {why}"));
                    }
                }
                total += names.len();
                done += ok;
                println!("{}: {ok}/{} cards compile", d.name, names.len());
                if verbose {
                    for l in lines {
                        println!("{l}");
                    }
                }
            }
            println!(
                "total: {done}/{total} deck slots ({}%)",
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
