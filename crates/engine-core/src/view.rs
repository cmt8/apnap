//! `PlayerView`: a redacted, per-player projection of the game (spec §5). Agents (AI and UI)
//! only ever see this, never `GameState` (CLAUDE.md invariant 6). Opponents' hands and all
//! libraries are counts only.

use crate::decision::PendingDecision;
use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::mana::ManaPool;
use crate::state::{CombatState, GameState};
use crate::turn::Step;
use alloc::string::String;
use alloc::vec::Vec;
use card_model::CardType;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectView {
    pub id: ObjectId,
    pub name: String,
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub card_types: Vec<CardType>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub tapped: bool,
    pub damage_marked: u32,
    pub summoning_sick: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerPublic {
    pub id: PlayerId,
    pub life: i32,
    pub has_lost: bool,
    pub hand_size: usize,
    pub library_size: usize,
    pub mana_pool: ManaPool,
    pub graveyard: Vec<ObjectView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerView {
    pub viewer: PlayerId,
    pub turn_number: u32,
    pub active: PlayerId,
    pub step: Step,
    pub players: Vec<PlayerPublic>,
    /// The viewer's own hand.
    pub hand: Vec<ObjectView>,
    pub battlefield: Vec<ObjectView>,
    pub stack: Vec<ObjectView>,
    pub exile: Vec<ObjectView>,
    pub combat: Option<CombatState>,
    /// The pending decision, if it is the viewer's.
    pub decision: Option<PendingDecision>,
}

fn object_view(st: &GameState, id: ObjectId) -> ObjectView {
    let o = st.object(id);
    let c = chars(st, id);
    ObjectView {
        id,
        name: c.name,
        owner: o.owner,
        controller: o.controller,
        card_types: c.card_types,
        power: c.power,
        toughness: c.toughness,
        tapped: o.tapped,
        damage_marked: o.damage_marked,
        summoning_sick: o.summoning_sick,
    }
}

fn views(st: &GameState, ids: &[ObjectId]) -> Vec<ObjectView> {
    ids.iter().map(|&id| object_view(st, id)).collect()
}

pub fn player_view(st: &GameState, viewer: PlayerId) -> PlayerView {
    PlayerView {
        viewer,
        turn_number: st.turn.turn_number,
        active: st.turn.active,
        step: st.turn.step,
        players: st
            .players
            .iter()
            .map(|p| PlayerPublic {
                id: p.id,
                life: p.life,
                has_lost: p.has_lost,
                hand_size: st.zones.hands[p.id.index()].len(),
                library_size: st.zones.libraries[p.id.index()].len(),
                mana_pool: p.mana_pool,
                graveyard: views(st, &st.zones.graveyards[p.id.index()]),
            })
            .collect(),
        hand: views(st, &st.zones.hands[viewer.index()]),
        battlefield: views(st, &st.zones.battlefield),
        stack: views(st, &st.zones.stack),
        exile: views(st, &st.zones.exile),
        combat: st.combat.clone(),
        decision: st.pending.clone().filter(|d| d.player == viewer),
    }
}
