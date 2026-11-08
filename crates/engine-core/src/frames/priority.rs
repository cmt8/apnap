//! Priority (CR 117 (2026-11-06)). Text: docs/rules-notes/117-priority-101.4-apnap.md.

use super::cast::{CastFrame, CastSource, can_begin, mana_sources, sorcery_timing, tap_for_mana};
use super::resolve::ResolveFrame;
use super::{Flow, Frame, Input};
use crate::actions::move_object;
use crate::decision::{ChoiceKind, OptionDesc};
use crate::events::Event;
use crate::ids::PlayerId;
use crate::layers::chars;
use crate::mana::types_in;
use crate::sba;
use crate::state::GameState;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec;
use alloc::vec::Vec;
use card_model::{AbilityDef, CardType};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorityFrame {
    /// The player who would receive priority.
    pub player: PlayerId,
    /// Consecutive passes so far (CR 117.4 "pass in succession").
    pub passes: u32,
}

impl PriorityFrame {
    pub(crate) fn run(mut self, st: &mut GameState, input: Option<Input>) -> Flow {
        let Some(inp) = input else {
            // CR 117.5 / 704.3: before a player would receive priority, perform SBAs
            // repeatedly until none are performed (then put triggers on the stack: M2).
            while sba::check_once(st) {
                if st.is_over() {
                    return Flow::done();
                }
            }
            // CR 800.4a: if that player left the game, priority passes to the next player
            // in turn order who's still in the game.
            if !st.in_game(self.player) {
                self.player = st.next_in_turn_order(self.player);
            }
            let options = priority_options(st, self.player);
            let p = self.player;
            return Flow::ask_one(Frame::Priority(self), p, ChoiceKind::Priority, options);
        };
        let p = self.player;
        let again = Frame::Priority(PriorityFrame {
            player: p,
            passes: 0,
        });
        match inp.picked_one().cloned().expect("priority pick") {
            OptionDesc::Pass => {
                // CR 117.3d
                st.emit(Event::PriorityPassed { player: p });
                let passes = self.passes + 1;
                // CR 117.4: only players still in the game count.
                if passes as usize >= st.players_in_game() {
                    if st.stack.is_empty() {
                        // CR 500.2: the step or phase ends.
                        Flow::done()
                    } else {
                        // CR 117.4: the top object resolves; CR 117.3b: then the active
                        // player (CR 800.4j: or the next player) receives priority.
                        Flow::Continue(vec![
                            Frame::Priority(PriorityFrame {
                                player: st.first_priority_player(),
                                passes: 0,
                            }),
                            Frame::Resolve(ResolveFrame::new()),
                        ])
                    }
                } else {
                    Flow::then(Frame::Priority(PriorityFrame {
                        player: st.next_in_turn_order(p),
                        passes,
                    }))
                }
            }
            OptionDesc::PlayLand { card } => {
                // CR 116.2a / 305.1: a special action; the player receives priority again (117.3c).
                let land = move_object(st, card, Zone::Battlefield, LibraryPosition::Top, Some(p));
                st.player_mut(p).lands_played_this_turn += 1;
                st.emit(Event::LandPlayed { player: p, land });
                Flow::then(again)
            }
            // CR 117.3c: the caster/activator receives priority afterward.
            OptionDesc::CastSpell { card } => Flow::Continue(vec![
                again,
                Frame::Cast(CastFrame::new(p, CastSource::Card(card))),
            ]),
            OptionDesc::ActivateAbility { source, index } => Flow::Continue(vec![
                again,
                Frame::Cast(CastFrame::new(p, CastSource::Ability { source, index })),
            ]),
            // CR 117.1d / 605.3a: mana abilities with priority; the mana floats.
            OptionDesc::TapForMana { source, mana } => {
                tap_for_mana(st, p, source, mana);
                Flow::then(again)
            }
            o => unreachable!("not a priority option: {o:?}"),
        }
    }
}

/// Legal priority actions in stable order: Pass, lands, spells (hand order), activated
/// abilities, then mana abilities (battlefield order).
fn priority_options(st: &GameState, p: PlayerId) -> Vec<OptionDesc> {
    let mut opts = vec![OptionDesc::Pass];
    let hand = &st.zones.hands[p.index()];
    // CR 305.1–305.2: one land per turn, sorcery timing.
    if sorcery_timing(st, p) && st.player(p).lands_played_this_turn < 1 {
        for &card in hand {
            if chars(st, card).is(CardType::Land) {
                opts.push(OptionDesc::PlayLand { card });
            }
        }
    }
    for &card in hand {
        if can_begin(st, p, CastSource::Card(card)) {
            opts.push(OptionDesc::CastSpell { card });
        }
    }
    for &source in &st.zones.battlefield {
        if st.object(source).controller != p {
            continue;
        }
        for (i, a) in chars(st, source).abilities.iter().enumerate() {
            let index = i as u16;
            if matches!(a, AbilityDef::Activated { .. })
                && can_begin(st, p, CastSource::Ability { source, index })
            {
                opts.push(OptionDesc::ActivateAbility { source, index });
            }
        }
    }
    for (source, mask) in mana_sources(st, p, None) {
        for mana in types_in(mask) {
            opts.push(OptionDesc::TapForMana { source, mana });
        }
    }
    opts
}
