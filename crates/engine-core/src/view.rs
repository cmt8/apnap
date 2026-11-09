//! `PlayerView`: a redacted, per-player projection of the game (spec §5). Agents (AI and UI)
//! only ever see this, never `GameState` (CLAUDE.md invariant 6). Opponents' hands and all
//! libraries are counts only.

use crate::decision::PendingDecision;
use crate::ids::CardInstanceId;
use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::mana::ManaPool;
use crate::stack::{StackKind, Target};
use crate::state::{CombatState, GameState};
use crate::turn::Step;
use alloc::string::String;
use alloc::vec::Vec;
use card_model::{CardType, CounterKind, Keyword};
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
    pub keywords: Vec<Keyword>,
    pub mana_value: u32,
    pub counters: Vec<(CounterKind, u32)>,
    /// CR 903.3: a commander (public information).
    pub is_commander: bool,
    pub card_instance: CardInstanceId,
    pub attached_to: Option<ObjectId>,
}

/// A spell or ability on the stack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackView {
    pub id: ObjectId,
    pub controller: PlayerId,
    /// `Some(source)` for an activated ability; `None` for a spell.
    pub ability_of: Option<ObjectId>,
    /// The spell's name, or the name of the ability's source card.
    pub name: String,
    pub modes: Vec<u8>,
    pub x: u32,
    pub targets: Vec<Target>,
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
    /// CR 903.10a: combat damage taken from each commander card.
    pub commander_damage: Vec<(CardInstanceId, u32)>,
    /// CR 903.8: casts from the command zone, per commander card.
    pub commander_casts: Vec<(CardInstanceId, u32)>,
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
    /// Bottom to top. Everything on the stack is public.
    pub stack: Vec<StackView>,
    pub exile: Vec<ObjectView>,
    /// The command zone (public).
    pub command: Vec<ObjectView>,
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
        controller: crate::layers::controller_of(st, id),
        card_types: c.card_types.to_vec(),
        power: c.power,
        toughness: c.toughness,
        tapped: o.tapped,
        damage_marked: o.damage_marked,
        summoning_sick: o.summoning_sick,
        mana_value: c.mana_cost.as_ref().map_or(0, |m| m.mana_value()),
        keywords: c.keywords,
        counters: o.counters.iter().map(|(k, n)| (*k, *n)).collect(),
        is_commander: st.is_commander(id),
        card_instance: o.card_instance,
        attached_to: o.attached_to,
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
                commander_damage: p.commander_damage.iter().map(|(c, n)| (*c, *n)).collect(),
                commander_casts: p.commander_casts.iter().map(|(c, n)| (*c, *n)).collect(),
            })
            .collect(),
        hand: views(st, &st.zones.hands[viewer.index()]),
        battlefield: views(st, &st.zones.battlefield),
        stack: st
            .stack
            .iter()
            .map(|so| StackView {
                id: so.id,
                controller: so.controller,
                ability_of: match so.kind {
                    StackKind::Spell => None,
                    StackKind::Ability { source, .. } => Some(source),
                },
                name: match &so.kind {
                    StackKind::Spell => st.card_def(so.id).name.clone(),
                    StackKind::Ability { source_card, .. } => {
                        st.catalog.get(*source_card).name.clone()
                    }
                },
                modes: so.modes.clone(),
                x: so.x,
                targets: so.all_targets().copied().collect(),
            })
            .collect(),
        exile: views(st, &st.zones.exile),
        command: views(st, &st.zones.command),
        combat: st.combat.clone(),
        decision: st.pending.clone().filter(|d| d.player == viewer),
    }
}
