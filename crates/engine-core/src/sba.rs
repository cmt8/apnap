//! State-based actions (CR 704 (2026-11-06); verbatim text in docs/rules-notes/704-sba.md).
//!
//! M0 implements 704.5a, 704.5b, 704.5f and 704.5g. All applicable SBAs are gathered from
//! ONE snapshot and then performed simultaneously as a single event (CR 704.3). SBAs are
//! never checked during resolution: the only callers are the priority frame (CR 117.5) and
//! the cleanup step (CR 514.3a).

use crate::actions::{GameAction, execute, move_object};
use crate::events::{Event, LossReason};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::state::{GameResult, GameState};
use crate::zones::{LibraryPosition, Zone};
use alloc::vec::Vec;
use card_model::CardType;

/// Perform one SBA check. Returns whether any SBA was performed. The caller repeats the
/// check while this returns true (CR 704.3).
pub(crate) fn check_once(st: &mut GameState) -> bool {
    // --- Gather (single snapshot) ---
    let mut losers: Vec<(PlayerId, LossReason)> = Vec::new();
    for p in &st.players {
        if p.has_lost {
            continue;
        }
        if p.life <= 0 {
            // CR 704.5a
            losers.push((p.id, LossReason::ZeroLife));
        } else if p.drew_from_empty_library {
            // CR 704.5b
            losers.push((p.id, LossReason::DrewFromEmptyLibrary));
        }
    }
    // CR 704.5b looks back only to the previous check.
    for p in &mut st.players {
        p.drew_from_empty_library = false;
    }

    let mut to_graveyard: Vec<ObjectId> = Vec::new();
    for &id in &st.zones.battlefield {
        let c = chars(st, id);
        if !c.is(CardType::Creature) {
            continue;
        }
        let toughness = c.toughness.unwrap_or(0);
        if toughness <= 0 {
            // CR 704.5f
            to_graveyard.push(id);
        } else if st.object(id).damage_marked >= toughness as u32 {
            // CR 704.5g: lethal damage → destroyed (CR 701.8: moved to owner's graveyard).
            to_graveyard.push(id);
        }
    }

    if losers.is_empty() && to_graveyard.is_empty() {
        return false;
    }

    // --- Perform simultaneously ---
    for id in to_graveyard {
        move_object(st, id, Zone::Graveyard, LibraryPosition::Top, None);
    }
    let any_loss = !losers.is_empty();
    for (player, reason) in losers {
        execute(st, GameAction::LoseGame { player, reason });
    }
    if any_loss {
        check_game_over(st);
    }
    true
}

/// CR 104.2a / 104.4a (2026-11-06).
fn check_game_over(st: &mut GameState) {
    if st.result.is_some() {
        return;
    }
    let remaining: Vec<PlayerId> = st
        .players
        .iter()
        .filter(|p| !p.has_lost)
        .map(|p| p.id)
        .collect();
    match remaining.as_slice() {
        [] => finish(st, None),
        [w] => finish(st, Some(*w)),
        _ => {
            // TODO(M3): CR 800.4a — a player leaving a multiplayer game. `new_game` rejects
            // >2 players until then, so this is unreachable in M0.
            unreachable!("multiplayer elimination requires CR 800.4 (M3)")
        }
    }
}

fn finish(st: &mut GameState, winner: Option<PlayerId>) {
    st.result = Some(GameResult { winner });
    st.emit(Event::GameOver { winner });
}
