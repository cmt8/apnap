//! Headless game running. Not part of the pure engine: may use std, files, threads.

use ai::RandomAgent;
use engine_core::rng::splitmix64;
use engine_core::state::GameResult;
use engine_core::{Agent, Event, GameConfig, GameState, Response, StepOutput, player_view};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

/// Default cap on decisions per game (stuck-loop detection).
pub const MAX_DECISIONS: usize = 200_000;

/// A recorded game: everything needed to replay it bit-for-bit.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Replay {
    pub cr_version: String,
    pub seed: u64,
    pub responses: Vec<Response>,
}

#[derive(Clone, Debug)]
pub struct GameRecord {
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
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for HarnessError {}

/// The M0 setup: two players, 60-card vanilla decks, constructed rules.
pub fn m0_game(seed: u64) -> GameState {
    let catalog = cards::m0_catalog();
    let deck = cards::m0_deck(&catalog);
    engine_core::new_game(
        GameConfig::constructed_1v1(),
        Arc::new(catalog),
        &[deck.clone(), deck],
        seed,
    )
    .expect("valid M0 setup")
}

/// Per-seat agent seed, independent of the game RNG stream.
pub fn agent_seed(game_seed: u64, seat: u8) -> u64 {
    let mut s = game_seed ^ (0xA6E7_5EED_0000_0000 | u64::from(seat));
    splitmix64(&mut s)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PlayOptions {
    pub max_decisions: Option<usize>,
    /// Run `GameState::check_invariants` after every step (slow; tests).
    pub check_invariants: bool,
}

/// Play a full M0 game with random agents.
pub fn play_random(seed: u64, opts: PlayOptions) -> Result<GameRecord, HarnessError> {
    let mut st = m0_game(seed);
    let mut agents: Vec<RandomAgent> = (0..st.num_players() as u8)
        .map(|s| RandomAgent::new(agent_seed(seed, s)))
        .collect();
    let max = opts.max_decisions.unwrap_or(MAX_DECISIONS);
    let mut rec = GameRecord {
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
    let mut st = m0_game(replay.seed);
    let events = feed(&mut st, &replay.responses)?;
    Ok((st, events))
}

pub fn save_json(st: &GameState) -> String {
    serde_json::to_string(st).expect("GameState serializes")
}

pub fn load_json(s: &str) -> Result<GameState, serde_json::Error> {
    serde_json::from_str(s)
}

/// Golden hashes: `seed log_hash state_hash` per line.
pub const GOLDEN_SEEDS: &[u64] = &[1, 2, 3, 42, 1337, 0xC0FFEE];

pub fn golden_line(rec: &GameRecord) -> String {
    format!("{} {:016x} {:016x}", rec.seed, rec.log_hash, rec.state_hash)
}
