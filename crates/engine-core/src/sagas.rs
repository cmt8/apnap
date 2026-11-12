//! Sagas (CR 715 (2026-11-06)). Text: docs/rules-notes/715-sagas.md.
//!
//! A chapter ability is a triggered ability (`Trigger::Chapter`, detected in
//! `triggers::detect_events`); the lore counter a Saga enters with is its intrinsic
//! replacement ability, compiled onto the card (CR 715.3a). Here: the lore counter put on as
//! the precombat main phase begins (CR 505.4, 715.3c) and the sacrifice after the final
//! chapter (CR 704.5s, 715.4).

use crate::ids::{ObjectId, PlayerId};
use crate::layers::{abilities, chars};
use crate::replacement::{MoveCause, Proposed};
use crate::stack::StackKind;
use crate::state::GameState;
use crate::zones::Zone;
use alloc::vec::Vec;
use card_model::{AbilityDef, AbilityId, CounterKind, Trigger};

/// The chapter number of a chapter ability (CR 715.2a).
pub fn chapter_of(a: &AbilityDef) -> Option<u32> {
    match a {
        AbilityDef::Triggered {
            when: Trigger::Chapter(n),
            ..
        } => Some(*n),
        _ => None,
    }
}

/// CR 715.2d: a Saga's final chapter number — the greatest among the chapter abilities it
/// has now (0 if it has none).
pub fn final_chapter(st: &GameState, id: ObjectId) -> u32 {
    abilities(st, id)
        .into_iter()
        .filter_map(|(_, a)| chapter_of(a))
        .max()
        .unwrap_or(0)
}

/// A Saga permanent with one or more chapter abilities (CR 505.4, 704.5s).
fn is_working_saga(st: &GameState, id: ObjectId) -> bool {
    chars(st, id).has_subtype("Saga") && final_chapter(st, id) > 0
}

/// CR 505.4, 715.3c: as `player`'s precombat main phase begins, they put a lore counter on
/// each Saga they control with one or more chapter abilities — one batch, replaceable like
/// any counters being put on a permanent (CR 614.1a), not using the stack.
pub(crate) fn lore_as_main_phase_begins(st: &GameState, player: PlayerId) -> Vec<Proposed> {
    st.zones
        .battlefield
        .iter()
        .filter(|&&id| crate::layers::controller_of(st, id) == player && is_working_saga(st, id))
        .map(|&object| Proposed::PutCounters {
            object,
            kind: CounterKind::LORE,
            count: 1,
        })
        .collect()
}

/// CR 704.5s, 715.4: Sagas whose controller sacrifices them — at least as many lore
/// counters as the final chapter number, and no chapter ability of theirs triggered but not
/// yet gone from the stack (waiting to be put on it, or on it).
pub(crate) fn finished(st: &GameState) -> Vec<ObjectId> {
    let chapter = |ability: AbilityId| chapter_of(st.catalog.ability(ability)).is_some();
    st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| {
            if !is_working_saga(st, id) {
                return false;
            }
            let lore = st.object(id).counters_of(CounterKind::LORE);
            let waiting = st
                .pending_triggers
                .iter()
                .any(|t| t.source == id && chapter(t.ability))
                || st.stack.iter().any(|s| {
                    matches!(s.kind, StackKind::Ability { source, ability, triggered: true, .. }
                        if source == id && chapter(ability))
                });
            lore >= final_chapter(st, id) && !waiting
        })
        .collect()
}

/// The sacrifices of [`finished`], as events.
pub(crate) fn sacrifices(st: &GameState) -> Vec<Proposed> {
    finished(st)
        .into_iter()
        .map(|id| Proposed::moving(id, Zone::Graveyard, MoveCause::Sacrifice))
        .collect()
}
