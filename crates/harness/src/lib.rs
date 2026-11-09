//! Headless game running. Not part of the pure engine: may use std, files, threads.

pub mod script;
pub mod text;

use ai::RandomAgent;
use card_model::{CardCatalog, CardDefId};
use engine_core::rng::splitmix64;
use engine_core::state::GameResult;
use engine_core::{Agent, Event, GameConfig, GameState, Response, StepOutput, player_view};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::{Arc, OnceLock};

/// Default cap on decisions per game (stuck-loop detection).
pub const MAX_DECISIONS: usize = 200_000;

/// Which game to set up. Recorded in replays so they can be rebuilt.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Setup {
    /// M0: two players, vanilla creatures and basic lands (`decks/m0_vanilla.ron`).
    #[default]
    M0Vanilla,
    /// M1: `players` players with the M1 deck (`decks/m1_red_green.ron`); 1v1 constructed
    /// rules for 2, free-for-all (CR 806, attack multiple players) otherwise.
    M1 { players: u8 },
    /// M2: `players` players with the M2 deck (`decks/m2_mixed.ron`).
    M2 { players: u8 },
}

impl Setup {
    pub fn players(self) -> u8 {
        match self {
            Setup::M0Vanilla => 2,
            Setup::M1 { players } | Setup::M2 { players } => players,
        }
    }

    /// Short label used in golden files and CLI output: `m0`, `m1-4p`, …
    pub fn label(self) -> String {
        match self {
            Setup::M0Vanilla => "m0".into(),
            Setup::M1 { players } => format!("m1-{players}p"),
            Setup::M2 { players } => format!("m2-{players}p"),
        }
    }

    pub fn parse(label: &str) -> Option<Setup> {
        if label == "m0" {
            return Some(Setup::M0Vanilla);
        }
        if let Some(rest) = label.strip_prefix("m2-") {
            let n = rest.strip_suffix('p')?.parse().ok()?;
            return Some(Setup::M2 { players: n });
        }
        let n = label.strip_prefix("m1-")?.strip_suffix('p')?.parse().ok()?;
        Some(Setup::M1 { players: n })
    }

    pub fn game(self, seed: u64) -> GameState {
        let (config, deck_file) = match self {
            Setup::M0Vanilla => (GameConfig::constructed_1v1(), "m0_vanilla.ron"),
            Setup::M1 { players: 2 } => (GameConfig::constructed_1v1(), "m1_red_green.ron"),
            Setup::M1 { players } => (GameConfig::free_for_all(players), "m1_red_green.ron"),
            Setup::M2 { players: 2 } => (GameConfig::constructed_1v1(), "m2_mixed.ron"),
            Setup::M2 { players } => (GameConfig::free_for_all(players), "m2_mixed.ron"),
        };
        let d = deck(deck_file);
        let decks = vec![d; config.num_players as usize];
        engine_core::new_game(config, catalog(), &decks, seed).expect("valid setup")
    }
}

/// Every card in `crates/cards`, loaded and validated in strict mode, once.
pub fn catalog() -> Arc<CardCatalog> {
    static CATALOG: OnceLock<Arc<CardCatalog>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let sources: Vec<card_db::Source<'_>> = cards::CARDS
                .iter()
                .map(|&(path, text)| card_db::Source { path, text })
                .collect();
            match card_db::load_catalog(&sources, card_db::Strictness::Strict) {
                Ok(c) => Arc::new(c),
                Err(errs) => {
                    let msg: Vec<String> = errs.iter().map(ToString::to_string).collect();
                    panic!("card data failed to load:\n{}", msg.join("\n"))
                }
            }
        })
        .clone()
}

/// A deck from `crates/cards/decks/<file>`, resolved against [`catalog`].
pub fn deck(file: &str) -> Vec<CardDefId> {
    let text = cards::deck(file).unwrap_or_else(|| panic!("no deck {file}"));
    let path = format!("crates/cards/decks/{file}");
    card_db::load_deck(card_db::Source { path: &path, text }, &catalog())
        .unwrap_or_else(|e| panic!("{e}"))
}

/// A recorded game: everything needed to replay it bit-for-bit.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Replay {
    pub cr_version: String,
    #[serde(default)]
    pub setup: Setup,
    pub seed: u64,
    pub responses: Vec<Response>,
}

#[derive(Clone, Debug)]
pub struct GameRecord {
    pub setup: Setup,
    pub seed: u64,
    pub responses: Vec<Response>,
    /// Every event, in order.
    pub events: Vec<Event>,
    /// `events_before[k]` = number of events emitted before response `k` was submitted.
    pub events_before: Vec<usize>,
    /// The decision each response answered.
    pub decisions: Vec<engine_core::PendingDecision>,
    pub log_hash: u64,
    pub state_hash: u64,
    pub result: Option<GameResult>,
    pub turns: u32,
}

impl GameRecord {
    pub fn replay(&self) -> Replay {
        Replay {
            cr_version: engine_core::CR_VERSION.into(),
            setup: self.setup,
            seed: self.seed,
            responses: self.responses.clone(),
        }
    }
}

#[derive(Debug)]
pub enum HarnessError {
    Stuck {
        seed: u64,
        decisions: usize,
    },
    Illegal {
        at: usize,
        err: engine_core::IllegalResponse,
    },
    Invariant {
        at: usize,
        msg: String,
    },
    ReplayEndedEarly {
        at: usize,
    },
    Panic {
        seed: u64,
        msg: String,
    },
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for HarnessError {}

/// The M0 setup: two players, 60-card vanilla decks, constructed rules.
pub fn m0_game(seed: u64) -> GameState {
    Setup::M0Vanilla.game(seed)
}

/// Per-seat agent seed, independent of the game RNG stream.
pub fn agent_seed(game_seed: u64, seat: u8) -> u64 {
    let mut s = game_seed ^ (0xA6E7_5EED_0000_0000 | u64::from(seat));
    splitmix64(&mut s)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PlayOptions {
    pub setup: Setup,
    pub max_decisions: Option<usize>,
    /// Run `GameState::check_invariants` after every step (slow; tests).
    pub check_invariants: bool,
}

/// Play a full game with random agents.
pub fn play_random(seed: u64, opts: PlayOptions) -> Result<GameRecord, HarnessError> {
    let mut st = opts.setup.game(seed);
    let mut agents: Vec<RandomAgent> = (0..st.num_players() as u8)
        .map(|s| RandomAgent::new(agent_seed(seed, s)))
        .collect();
    let max = opts.max_decisions.unwrap_or(MAX_DECISIONS);
    let mut rec = GameRecord {
        setup: opts.setup,
        seed,
        responses: Vec::new(),
        events: Vec::new(),
        events_before: Vec::new(),
        decisions: Vec::new(),
        log_hash: 0,
        state_hash: 0,
        result: None,
        turns: 0,
    };
    let mut out = engine_core::start(&mut st);
    loop {
        rec.events.append(&mut out.events);
        if opts.check_invariants {
            st.check_invariants()
                .map_err(|msg| HarnessError::Invariant {
                    at: rec.responses.len(),
                    msg,
                })?;
        }
        let Some(d) = out.next else { break };
        if rec.responses.len() >= max {
            return Err(HarnessError::Stuck {
                seed,
                decisions: max,
            });
        }
        let view = player_view(&st, d.player);
        let r = agents[d.player.index()].decide(&view, &d);
        rec.events_before.push(rec.events.len());
        rec.decisions.push(d);
        rec.responses.push(r.clone());
        out = engine_core::step(&mut st, r).map_err(|err| HarnessError::Illegal {
            at: rec.responses.len() - 1,
            err,
        })?;
    }
    rec.log_hash = st.log_hash;
    rec.state_hash = st.state_hash();
    rec.result = st.result.clone();
    rec.turns = st.turn.turn_number;
    Ok(rec)
}

/// Feed `responses` to `st` (which must be suspended or freshly created), returning all
/// events emitted, including those from `start` if the game had not started.
pub fn feed(st: &mut GameState, responses: &[Response]) -> Result<Vec<Event>, HarnessError> {
    let mut events = Vec::new();
    let StepOutput {
        events: mut e,
        mut next,
    } = engine_core::start(st);
    events.append(&mut e);
    for (i, r) in responses.iter().enumerate() {
        if next.is_none() {
            return Err(HarnessError::ReplayEndedEarly { at: i });
        }
        let mut out =
            engine_core::step(st, r.clone()).map_err(|err| HarnessError::Illegal { at: i, err })?;
        events.append(&mut out.events);
        next = out.next;
    }
    Ok(events)
}

/// Replay a recorded game from scratch.
pub fn run_replay(replay: &Replay) -> Result<(GameState, Vec<Event>), HarnessError> {
    let mut st = replay.setup.game(replay.seed);
    let events = feed(&mut st, &replay.responses)?;
    Ok((st, events))
}

pub fn save_json(st: &GameState) -> String {
    serde_json::to_string(st).expect("GameState serializes")
}

pub fn load_json(s: &str) -> Result<GameState, serde_json::Error> {
    serde_json::from_str(s)
}

/// Golden games: `setup seed log_hash state_hash` per line in `tests/golden/games.txt`.
pub const GOLDEN: &[(Setup, u64)] = &[
    (Setup::M0Vanilla, 1),
    (Setup::M0Vanilla, 42),
    (Setup::M0Vanilla, 0xC0FFEE),
    (Setup::M1 { players: 2 }, 1),
    (Setup::M1 { players: 2 }, 42),
    (Setup::M1 { players: 4 }, 1),
    (Setup::M1 { players: 4 }, 42),
    (Setup::M1 { players: 4 }, 1337),
    (Setup::M2 { players: 2 }, 1),
    (Setup::M2 { players: 2 }, 42),
    (Setup::M2 { players: 4 }, 1),
    (Setup::M2 { players: 4 }, 42),
];

pub fn golden_line(rec: &GameRecord) -> String {
    format!(
        "{} {} {:016x} {:016x}",
        rec.setup.label(),
        rec.seed,
        rec.log_hash,
        rec.state_hash
    )
}

/// Play `games` random games (seeds `seed..seed+games`) on `threads` OS threads. Each game is
/// independent and deterministic, so the results don't depend on the thread count.
pub fn fuzz(
    setup: Setup,
    seed: u64,
    games: u64,
    threads: usize,
) -> Vec<(u64, Result<GameRecord, HarnessError>)> {
    let threads = threads.max(1);
    let mut out: Vec<(u64, Result<GameRecord, HarnessError>)> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                sc.spawn(move || {
                    (seed + t as u64..seed + games)
                        .step_by(threads)
                        .map(|s| {
                            let opts = PlayOptions {
                                setup,
                                ..Default::default()
                            };
                            let r = std::panic::catch_unwind(|| play_random(s, opts))
                                .unwrap_or_else(|p| {
                                    Err(HarnessError::Panic {
                                        seed: s,
                                        msg: panic_message(&p),
                                    })
                                });
                            (s, r)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("fuzz thread"))
            .collect()
    });
    out.sort_by_key(|(s, _)| *s);
    out
}

fn panic_message(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<String>()
        .cloned()
        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "panic".into())
}
