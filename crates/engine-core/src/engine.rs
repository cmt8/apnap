//! The engine's public entry points: `new_game`, `start`, `step`.

use crate::config::GameConfig;
use crate::decision::{IllegalResponse, PendingDecision, Response};
use crate::events::Event;
use crate::frames::{Frame, Input, SetupFrame};
use crate::ids::{CardInstanceId, PlayerId};
use crate::mana::ManaPool;
use crate::objects::GameObject;
use crate::rng::GameRng;
use crate::state::{GameState, PlayerState};
use crate::turn::{Step, TurnState};
use crate::zones::{Zone, ZoneIndex};
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use card_model::{CardCatalog, CardDefId};
use core::fmt;
use serde::{Deserialize, Serialize};

/// Result of advancing the engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepOutput {
    pub events: Vec<Event>,
    /// `None` = the game is over.
    pub next: Option<PendingDecision>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupError {
    /// Supported: 2 to 8 players.
    UnsupportedPlayerCount(u8),
    DeckCountMismatch {
        decks: usize,
        players: u8,
    },
    UnknownCard(CardDefId),
    /// CR 903.3: a commander must be a legendary creature card.
    InvalidCommander(CardDefId),
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl core::error::Error for SetupError {}

/// Create a game. Decks are given in list order; they are shuffled during setup (CR 103.3).
/// Call [`start`] to run until the first decision.
pub fn new_game(
    config: GameConfig,
    catalog: Arc<CardCatalog>,
    decks: &[Vec<CardDefId>],
    seed: u64,
) -> Result<GameState, SetupError> {
    new_game_with_commanders(config, catalog, decks, &[], seed)
}

/// A game where `commanders[seat]` (if given) is that player's commander (CR 903.3). Decks
/// exclude the commander. CR 903.6 / 103.2c: commanders start in the command zone.
pub fn new_game_with_commanders(
    config: GameConfig,
    catalog: Arc<CardCatalog>,
    decks: &[Vec<CardDefId>],
    commanders: &[Option<CardDefId>],
    seed: u64,
) -> Result<GameState, SetupError> {
    if !(2..=8).contains(&config.num_players) {
        return Err(SetupError::UnsupportedPlayerCount(config.num_players));
    }
    if decks.len() != config.num_players as usize {
        return Err(SetupError::DeckCountMismatch {
            decks: decks.len(),
            players: config.num_players,
        });
    }
    let n = config.num_players as usize;
    let mut st = GameState {
        players: (0..n)
            .map(|i| PlayerState {
                id: PlayerId(i as u8),
                life: config.starting_life,
                has_lost: false,
                mana_pool: ManaPool::default(),
                lands_played_this_turn: 0,
                drew_from_empty_library: false,
                commander_damage: BTreeMap::new(),
                commander_casts: BTreeMap::new(),
                restricted_mana: Vec::new(),
                drawn_this_turn: 0,
            })
            .collect(),
        config,
        seed,
        catalog,
        rng: GameRng::from_seed_u64(seed),
        next_object_id: 0,
        next_card_instance: 0,
        next_timestamp: 0,
        objects: BTreeMap::new(),
        zones: ZoneIndex::new(n),
        stack: Vec::new(),
        continuous: Vec::new(),
        shields: Vec::new(),
        lasting: Vec::new(),
        delayed: Vec::new(),
        next_effect_id: 0,
        commanders: Vec::new(),
        commander_cards: BTreeMap::new(),
        commander_moved: Vec::new(),
        pending_triggers: Vec::new(),
        last_check_acted: false,
        layer_cache: Default::default(),
        turn: TurnState {
            turn_number: 0,
            active: PlayerId(0),
            step: Step::Untap,
            starting_player: None,
        },
        combat: None,
        frames: vec![Frame::Setup(SetupFrame::Begin)],
        pending: None,
        result: None,
        generation: 0,
        log_hash: crate::hash::StableHasher::default().finish(),
        event_count: 0,
        decision_count: 0,
        out: Vec::new(),
    };
    for (seat, deck) in decks.iter().enumerate() {
        let owner = PlayerId(seat as u8);
        let commander = commanders.get(seat).copied().flatten();
        let entries = commander
            .map(|c| (c, Zone::Command))
            .into_iter()
            .chain(deck.iter().map(|&c| (c, Zone::Library)));
        for (card, zone) in entries {
            if card.0 as usize >= st.catalog.defs.len() {
                return Err(SetupError::UnknownCard(card));
            }
            let id = st.alloc_object_id();
            let timestamp = st.alloc_timestamp();
            let card_instance = CardInstanceId(st.next_card_instance);
            st.next_card_instance += 1;
            st.objects.insert(
                id,
                GameObject {
                    id,
                    card_instance,
                    card,
                    owner,
                    controller: owner,
                    zone,
                    timestamp,
                    tapped: false,
                    damage_marked: 0,
                    summoning_sick: false,
                    counters: Default::default(),
                    is_token: false,
                    deathtouch_damage: false,
                    attached_to: None,
                    last_controller: owner,
                    copy: None,
                    spell_copy: false,
                },
            );
            if zone == Zone::Command {
                let def = st.catalog.get(card);
                // CR 903.3: a legendary creature card.
                if !(def.has_type(card_model::CardType::Creature)
                    && def.supertypes.contains(&card_model::Supertype::Legendary))
                {
                    return Err(SetupError::InvalidCommander(card));
                }
                st.commanders.push((owner, card_instance));
                st.commander_cards.insert(card_instance, card);
                st.zones.command.push(id);
            } else {
                // CR 401.1: each player's deck becomes their library.
                st.zones.libraries[seat].push(id);
            }
        }
    }
    Ok(st)
}

/// Run until the first decision (or return the current one if already suspended).
pub fn start(st: &mut GameState) -> StepOutput {
    if st.pending.is_some() || st.is_over() {
        return StepOutput {
            events: Vec::new(),
            next: st.pending.clone(),
        };
    }
    run(st, None)
}

/// Answer the current pending decision and run until the next one.
///
/// An illegal response is rejected **before any mutation**: the state is left unchanged.
pub fn step(st: &mut GameState, resp: Response) -> Result<StepOutput, IllegalResponse> {
    let pending = st
        .pending
        .as_ref()
        .ok_or(IllegalResponse::NoPendingDecision)?;
    pending.validate(&resp)?;
    let decision = st.pending.take().expect("checked above");
    st.decision_count += 1;
    Ok(run(
        st,
        Some(Input {
            decision,
            response: resp,
        }),
    ))
}

fn run(st: &mut GameState, mut input: Option<Input>) -> StepOutput {
    loop {
        if st.is_over() {
            st.frames.clear();
            break;
        }
        let frame = st
            .frames
            .pop()
            .expect("frame stack exhausted without game over");
        let flow = frame.run(st, input.take());
        crate::layers::sync_control(st);
        match flow {
            crate::frames::Flow::Continue(frames) => st.frames.extend(frames),
            crate::frames::Flow::Suspend(frame, decision) => {
                st.frames.push(*frame);
                st.pending = Some(decision);
                break;
            }
        }
        debug_assert!(input.is_none(), "input not consumed by the suspended frame");
    }
    StepOutput {
        events: core::mem::take(&mut st.out),
        next: st.pending.clone(),
    }
}
