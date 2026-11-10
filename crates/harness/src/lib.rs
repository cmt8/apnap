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
    /// M3: a Commander game (CR 903) with `players` players, using the four commander decks
    /// in seat order (`decks/commander_*.ron`).
    Commander { players: u8 },
    /// M4: 60-card decks built only from verified real cards (`decks/real_*.ron`, D-038),
    /// alternating by seat. Needs the Scryfall cache.
    Real { players: u8 },
    /// A soak over every card that compiles from Forge's scripts (D-046): each seat gets a
    /// 60-card two-colour deck drawn at random (by the game's seed) from all of them.
    Forge { players: u8 },
}

/// The real-card decks, in seat order.
pub const REAL_DECKS: &[&str] = &["real_stompy.ron", "real_angels.ron", "real_zombies.ron"];

/// The Commander decks, in seat order.
pub const COMMANDER_DECKS: &[&str] = &[
    "commander_warlord.ron",
    "commander_matriarch.ron",
    "commander_tyrant.ron",
    "commander_wildheart.ron",
];

impl Setup {
    pub fn players(self) -> u8 {
        match self {
            Setup::M0Vanilla => 2,
            Setup::M1 { players }
            | Setup::M2 { players }
            | Setup::Commander { players }
            | Setup::Real { players }
            | Setup::Forge { players } => players,
        }
    }

    /// Short label used in golden files and CLI output: `m0`, `m1-4p`, …
    pub fn label(self) -> String {
        match self {
            Setup::M0Vanilla => "m0".into(),
            Setup::M1 { players } => format!("m1-{players}p"),
            Setup::M2 { players } => format!("m2-{players}p"),
            Setup::Commander { players } => format!("cmd-{players}p"),
            Setup::Real { players } => format!("real-{players}p"),
            Setup::Forge { players } => format!("forge-{players}p"),
        }
    }

    pub fn parse(label: &str) -> Option<Setup> {
        if label == "m0" {
            return Some(Setup::M0Vanilla);
        }
        if let Some(rest) = label.strip_prefix("cmd-") {
            let n = rest.strip_suffix('p')?.parse().ok()?;
            return Some(Setup::Commander { players: n });
        }
        if let Some(rest) = label.strip_prefix("forge-") {
            let n = rest.strip_suffix('p')?.parse().ok()?;
            return Some(Setup::Forge { players: n });
        }
        if let Some(rest) = label.strip_prefix("real-") {
            let n = rest.strip_suffix('p')?.parse().ok()?;
            return Some(Setup::Real { players: n });
        }
        if let Some(rest) = label.strip_prefix("m2-") {
            let n = rest.strip_suffix('p')?.parse().ok()?;
            return Some(Setup::M2 { players: n });
        }
        let n = label.strip_prefix("m1-")?.strip_suffix('p')?.parse().ok()?;
        Some(Setup::M1 { players: n })
    }

    pub fn game(self, seed: u64) -> GameState {
        if let Setup::Commander { players } = self {
            let decks: Vec<card_db::Deck> = (0..players as usize)
                .map(|i| deck_file(COMMANDER_DECKS[i % COMMANDER_DECKS.len()]))
                .collect();
            let cards: Vec<Vec<CardDefId>> = decks.iter().map(|d| d.cards.clone()).collect();
            let commanders: Vec<Option<CardDefId>> = decks.iter().map(|d| d.commander).collect();
            return engine_core::new_game_with_commanders(
                GameConfig::commander(players),
                catalog(),
                &cards,
                &commanders,
                seed,
            )
            .expect("valid commander setup");
        }
        if let Setup::Real { players } = self {
            let config = if players == 2 {
                GameConfig::constructed_1v1()
            } else {
                GameConfig::free_for_all(players)
            };
            // The cards are compiled from Forge's scripts (D-046); the game's catalog
            // holds only the cards of the decks in play and their tokens.
            let files: Vec<&str> = (0..players as usize)
                .map(|i| REAL_DECKS[i % REAL_DECKS.len()])
                .collect();
            let lists: Vec<card_db::DeckFile> = files.iter().map(|f| precon_list(f)).collect();
            let names = lists
                .iter()
                .flat_map(|d| d.cards.iter().map(|(_, n)| n.as_str()));
            let catalog = Arc::new(
                forge_library()
                    .catalog_for(&catalog(), names)
                    .unwrap_or_else(|missing| panic!("cards that don't compile: {missing:?}")),
            );
            let decks: Vec<Vec<CardDefId>> = files
                .iter()
                .map(|file| {
                    let text = cards::deck(file).unwrap_or_else(|| panic!("no deck {file}"));
                    let path = format!("crates/cards/decks/{file}");
                    card_db::load_deck(card_db::Source { path: &path, text }, &catalog)
                        .unwrap_or_else(|e| panic!("{e}"))
                })
                .collect();
            return engine_core::new_game(config, catalog, &decks, seed).expect("valid setup");
        }
        if let Setup::Forge { players } = self {
            let config = if players == 2 {
                GameConfig::constructed_1v1()
            } else {
                GameConfig::free_for_all(players)
            };
            let lists = forge_random_decks(&forge_library(), players, seed);
            let names = lists.iter().flatten().map(String::as_str);
            let catalog = Arc::new(
                forge_library()
                    .catalog_for(&catalog(), names)
                    .unwrap_or_else(|missing| panic!("cards that don't compile: {missing:?}")),
            );
            let decks: Vec<Vec<CardDefId>> = lists
                .iter()
                .map(|list| {
                    list.iter()
                        .map(|n| catalog.find(n).expect("in the catalog"))
                        .collect()
                })
                .collect();
            return engine_core::new_game(config, catalog, &decks, seed).expect("valid setup");
        }
        let (config, deck_file) = match self {
            Setup::M0Vanilla => (GameConfig::constructed_1v1(), "m0_vanilla.ron"),
            Setup::M1 { players: 2 } => (GameConfig::constructed_1v1(), "m1_red_green.ron"),
            Setup::M1 { players } => (GameConfig::free_for_all(players), "m1_red_green.ron"),
            Setup::M2 { players: 2 } => (GameConfig::constructed_1v1(), "m2_mixed.ron"),
            Setup::M2 { players } => (GameConfig::free_for_all(players), "m2_mixed.ron"),
            Setup::Commander { .. } | Setup::Real { .. } | Setup::Forge { .. } => {
                unreachable!("handled above")
            }
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

/// The four M4 precons (decisions.md D-034), in seat order.
pub const PRECON_DECKS: &[&str] = &[
    "precon_calling_all_angels.ron",
    "precon_wretched_ranks.ron",
    "precon_reign_of_dragons.ron",
    "precon_tramplesaurus_rex.ron",
];

/// A precon's unresolved list: (deck name, commander, [(count, card name)]).
pub fn precon_list(file: &str) -> card_db::DeckFile {
    let text = cards::deck(file).unwrap_or_else(|| panic!("no deck {file}"));
    let path = format!("crates/cards/decks/{file}");
    card_db::parse_deck_file(card_db::Source { path: &path, text })
        .unwrap_or_else(|e| panic!("{e}"))
}

/// Every card name the precons need from Scryfall (everything except the self-contained
/// basic lands), sorted and de-duplicated.
pub fn precon_card_names() -> Vec<String> {
    let have = catalog();
    let mut names: Vec<String> = PRECON_DECKS
        .iter()
        .flat_map(|f| {
            let d = precon_list(f);
            d.cards.into_iter().map(|(_, n)| n).chain(d.commander)
        })
        .filter(|n| have.find(n).is_none())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Every card name whose printed data the app and tools want cached: the precon pool plus
/// the basic lands (self-contained cards, but they have real printings to show).
pub fn scryfall_card_names() -> Vec<String> {
    let have = catalog();
    let mut names = precon_card_names();
    names.extend(
        have.defs
            .iter()
            .filter(|d| d.supertypes.contains(&card_model::Supertype::Basic))
            .map(|d| d.name.clone()),
    );
    names.sort();
    names.dedup();
    names
}

/// Where a precon card stands (spec §10.6 coverage report).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// A self-contained card (basic land).
    BuiltIn,
    /// Scripted, verified, and compiles against its printed data.
    Verified,
    /// Scripted but not yet verified.
    Unverified,
    /// A script exists but doesn't load: the problems.
    Broken(Vec<String>),
    /// No script yet. `printed_ok` = its printed data is representable today.
    Missing { printed_problems: Vec<String> },
}

/// Coverage of one card name against the scripts in `crates/cards/scripts`.
pub fn coverage_of(name: &str, data: &card_db::scryfall::ScryfallData) -> Coverage {
    if catalog().find(name).is_some() {
        return Coverage::BuiltIn;
    }
    let script = cards::SCRIPTS.iter().find_map(|&(path, text)| {
        let src = card_db::Source { path, text };
        card_db::parse_real(src)
            .ok()
            .filter(|s| s.name == name)
            .map(|s| (src, s))
    });
    match script {
        Some((src, s)) => match card_db::compile_real(src, data) {
            Ok(cs) => {
                let problems = card_db::validate(&cs, card_db::Strictness::AllowUnverified);
                if !problems.is_empty() {
                    Coverage::Broken(problems)
                } else if s.verified {
                    Coverage::Verified
                } else {
                    Coverage::Unverified
                }
            }
            Err(errs) => Coverage::Broken(errs.into_iter().map(|e| e.msg).collect()),
        },
        None => {
            // Dry run: can the printed data be represented at all?
            let printed_problems = match data.get(name) {
                None => vec![card_db::scryfall::FETCH_HINT.to_string()],
                Some(card) => {
                    let empty = card_db::real::RealCardScript {
                        oracle_id: card.oracle_id.clone().unwrap_or_default(),
                        name: card.name.clone(),
                        verified: false,
                        tests: vec![],
                        keywords: vec![],
                        abilities: vec![],
                    };
                    card_db::real::compile(&empty, card)
                        .err()
                        .unwrap_or_default()
                }
            };
            Coverage::Missing { printed_problems }
        }
    }
}

/// A deck file (with its commander, if any) from `crates/cards/decks/<file>`.
pub fn deck_file(file: &str) -> card_db::Deck {
    let text = cards::deck(file).unwrap_or_else(|| panic!("no deck {file}"));
    let path = format!("crates/cards/decks/{file}");
    card_db::load_deck_file(card_db::Source { path: &path, text }, &catalog())
        .unwrap_or_else(|e| panic!("{e}"))
}

/// Random decks for [`Setup::Forge`]: per seat two colours, 36 cards that cost only those
/// colours (any number of copies), and 12 of each of the two basic lands. Determined by
/// `seed` alone.
pub fn forge_random_decks(
    lib: &card_db::forge::Library,
    players: u8,
    seed: u64,
) -> Vec<Vec<String>> {
    use card_model::{Color, ManaSymbol};
    // SplitMix64: all that's needed is a reproducible stream.
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut next = move |n: usize| -> usize {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % n as u64) as usize
    };
    const BASICS: [(Color, &str); 5] = [
        (Color::White, "Plains"),
        (Color::Blue, "Island"),
        (Color::Black, "Swamp"),
        (Color::Red, "Mountain"),
        (Color::Green, "Forest"),
    ];
    (0..players)
        .map(|_| {
            let a = next(5);
            let b = (a + 1 + next(4)) % 5;
            let colors = [BASICS[a].0, BASICS[b].0];
            let pool: Vec<&String> = lib
                .cards
                .iter()
                .filter(|(_, c)| {
                    c.def.mana_cost.as_ref().is_none_or(|m| {
                        m.symbols.iter().all(|s| match s {
                            ManaSymbol::Colored(c) => colors.contains(c),
                            _ => true,
                        })
                    })
                })
                .map(|(name, _)| name)
                .collect();
            let mut deck: Vec<String> = (0..36).map(|_| pool[next(pool.len())].clone()).collect();
            for land in [BASICS[a].1, BASICS[b].1] {
                deck.extend(std::iter::repeat_n(land.to_string(), 12));
            }
            deck
        })
        .collect()
}

/// Every card that compiles from Forge's scripts, cross-checked against Scryfall's bulk
/// data (decisions.md D-046). Panics with the fetch hints if either is missing: run
/// `harness fetch-forge` and `harness fetch-bulk` once.
pub fn forge_library() -> Arc<card_db::forge::Library> {
    static LIBRARY: OnceLock<Arc<card_db::forge::Library>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| Arc::new(try_forge_library().unwrap_or_else(|e| panic!("{e}"))))
        .clone()
}

pub fn try_forge_library() -> Result<card_db::forge::Library, String> {
    let bulk = card_db::scryfall::load_bulk().map_err(|e| e.to_string())?;
    card_db::forge::Library::load(
        &card_db::forge::cards_dir(),
        &card_db::forge::tokens_dir(),
        Some(&bulk),
    )
}

/// A catalog for a feature test of the Forge compiler (CLAUDE.md invariant 15): the
/// self-contained cards plus invented cards written in Forge's script format, compiled the
/// way real ones are. `tokens` are `(script name, script)`. Panics, naming the blockers,
/// if any of them doesn't compile. Only the subtypes on these scripts' type lines are
/// known to the compiler.
pub fn forge_test_catalog(cards: &[&str], tokens: &[(&str, &str)]) -> Arc<CardCatalog> {
    let cards: Vec<(String, String)> = cards
        .iter()
        .enumerate()
        .map(|(i, t)| (format!("test_{i}"), t.to_string()))
        .collect();
    let tokens: Vec<(String, String)> = tokens
        .iter()
        .map(|(n, t)| (n.to_string(), t.to_string()))
        .collect();
    let lib = card_db::forge::Library::build(&cards, &tokens, None);
    assert_eq!(lib.unreadable, 0, "a test script could not be read");
    assert!(lib.blocked.is_empty(), "blocked: {:?}", lib.blocked);
    assert!(
        lib.blocked_tokens.is_empty(),
        "blocked tokens: {:?}",
        lib.blocked_tokens
    );
    let names: Vec<&str> = lib.cards.keys().map(String::as_str).collect();
    for name in &names {
        assert!(
            catalog().find(name).is_none(),
            "{name} is already a self-contained card: pick another name"
        );
    }
    Arc::new(
        lib.catalog_for(&catalog(), names)
            .unwrap_or_else(|e| panic!("{e:?}")),
    )
}

/// The blockers the Forge compiler reports for one invented card (empty = it compiles).
pub fn forge_test_blockers(card: &str) -> Vec<String> {
    let lib = card_db::forge::Library::build(&[("test".to_string(), card.to_string())], &[], None);
    assert_eq!(lib.unreadable, 0, "the test script could not be read");
    lib.blocked.into_values().flatten().collect()
}

/// Is the printed data the real cards need in the local cache? `Err` carries the hint
/// (which command downloads it).
pub fn real_cards_ready() -> Result<(), String> {
    card_db::scryfall::cached(&precon_card_names()).map_err(|e| e.to_string())?;
    if !card_db::forge::cards_dir().exists() {
        return Err(card_db::forge::FETCH_HINT.to_string());
    }
    card_db::scryfall::load_bulk()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The self-contained cards plus every verified real-card script, compiled against the
/// printed data in the local Scryfall cache. Panics with the fetch hint if the cache is
/// missing (decisions.md D-035): run `cargo run -p harness -- fetch-cards` once.
pub fn full_catalog() -> Arc<CardCatalog> {
    static CATALOG: OnceLock<Arc<CardCatalog>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let data =
                card_db::scryfall::cached(&precon_card_names()).unwrap_or_else(|e| panic!("{e}"));
            let src =
                |list: &'static [(&'static str, &'static str)]| -> Vec<card_db::Source<'static>> {
                    list.iter()
                        .map(|&(path, text)| card_db::Source { path, text })
                        .collect()
                };
            // Unverified scripts are skipped in strict play: only verified ones are loaded.
            let verified: Vec<card_db::Source<'static>> = src(cards::SCRIPTS)
                .into_iter()
                .filter(|s| card_db::parse_real(*s).is_ok_and(|r| r.verified))
                .collect();
            match card_db::load_full_catalog(
                &src(cards::CARDS),
                &verified,
                &data,
                card_db::Strictness::Strict,
            ) {
                Ok(c) => Arc::new(c),
                Err(errs) => {
                    let msg: Vec<String> = errs.iter().map(ToString::to_string).collect();
                    panic!("real-card scripts failed to load:\n{}", msg.join("\n"))
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
    pub agents: Agents,
    pub max_decisions: Option<usize>,
    /// Run `GameState::check_invariants` after every step (slow; tests).
    pub check_invariants: bool,
}

/// Which agents sit at the table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Agents {
    /// Every seat plays uniformly random legal responses.
    #[default]
    Random,
    /// Every seat is the heuristic agent (spec §9 Phase A).
    Heuristic,
    /// Seat 0 is the heuristic agent; the rest are random.
    HeuristicVsRandom,
}

impl Agents {
    pub fn parse(s: &str) -> Option<Agents> {
        match s {
            "random" => Some(Agents::Random),
            "heuristic" => Some(Agents::Heuristic),
            "h-vs-r" => Some(Agents::HeuristicVsRandom),
            _ => None,
        }
    }

    /// The agent for `seat` (each seeded independently of the game RNG). `catalog` is the
    /// game's card catalog (public card text, which the heuristic agent reads).
    pub fn make(self, seed: u64, seat: u8, catalog: &Arc<CardCatalog>) -> Box<dyn Agent> {
        let s = agent_seed(seed, seat);
        let heuristic = match self {
            Agents::Random => false,
            Agents::Heuristic => true,
            Agents::HeuristicVsRandom => seat == 0,
        };
        if heuristic {
            Box::new(ai::HeuristicAgent::new(s, catalog.clone()))
        } else {
            Box::new(RandomAgent::new(s))
        }
    }
}

/// Play a full game with the configured agents (random by default).
pub fn play_random(seed: u64, opts: PlayOptions) -> Result<GameRecord, HarnessError> {
    let mut st = opts.setup.game(seed);
    let mut agents: Vec<Box<dyn Agent>> = (0..st.num_players() as u8)
        .map(|s| opts.agents.make(seed, s, &st.catalog))
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
    (Setup::Commander { players: 2 }, 1),
    (Setup::Commander { players: 4 }, 1),
    (Setup::Commander { players: 4 }, 42),
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
    let base = PlayOptions {
        setup,
        ..Default::default()
    };
    fuzz_with(base, seed, games, threads)
}

/// [`fuzz`] with full options (setup, agents, invariant checks).
pub fn fuzz_with(
    base: PlayOptions,
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
                            let opts = base;
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
