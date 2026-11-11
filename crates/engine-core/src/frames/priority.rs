//! Priority (CR 117 (2026-11-06)). Text: docs/rules-notes/117-priority-101.4-apnap.md.

use super::cast::{CastFrame, CastSource, can_begin, mana_sources, sorcery_timing, tap_for_mana};
use super::resolve::ResolveFrame;
use super::{ApplyFrame, Flow, Frame, Input, StateCheckFrame};
use crate::decision::{ChoiceKind, OptionDesc};
use crate::events::Event;
use crate::ids::PlayerId;
use crate::layers::{abilities, chars};
use crate::mana::types_in;
use crate::replacement::{MoveCause, Proposed};
use crate::state::GameState;
use crate::zones::Zone;
use alloc::boxed::Box;
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
    /// The CR 117.5 state check has been done for this priority.
    pub checked: bool,
}

impl PriorityFrame {
    pub(crate) fn new(player: PlayerId, passes: u32) -> Self {
        PriorityFrame {
            player,
            passes,
            checked: false,
        }
    }

    pub(crate) fn run(mut self, st: &mut GameState, input: Option<Input>) -> Flow {
        let Some(inp) = input else {
            // CR 117.5 / 704.3: before a player would receive priority, perform SBAs until
            // none are performed, put triggers on the stack, and repeat (StateCheckFrame).
            if !self.checked {
                self.checked = true;
                return Flow::Continue(vec![
                    Frame::Priority(self),
                    Frame::StateCheck(StateCheckFrame::new()),
                ]);
            }
            if st.is_over() {
                return Flow::done();
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
        let again = Frame::Priority(PriorityFrame::new(p, 0));
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
                            Frame::Priority(PriorityFrame::new(st.first_priority_player(), 0)),
                            Frame::Resolve(Box::new(ResolveFrame::new())),
                        ])
                    }
                } else {
                    Flow::then(Frame::Priority(PriorityFrame::new(
                        st.next_in_turn_order(p),
                        passes,
                    )))
                }
            }
            OptionDesc::PlayLand { card } => {
                // CR 116.2a / 305.1: a special action; the player receives priority again
                // (117.3c). Entering the battlefield is replaceable ("enters tapped").
                st.player_mut(p).lands_played_this_turn += 1;
                let mut mv = Proposed::moving(card, Zone::Battlefield, MoveCause::PlayLand);
                if let Proposed::Move { controller, .. } = &mut mv {
                    *controller = Some(p);
                }
                Flow::Continue(vec![again, Frame::Apply(ApplyFrame::new(st, vec![mv]))])
            }
            // CR 117.3c: the caster/activator receives priority afterward.
            OptionDesc::CastSpell { card } => Flow::Continue(vec![
                again,
                Frame::Cast(CastFrame::new(p, CastSource::Card(card))),
            ]),
            OptionDesc::ActivateAbility { source, ability } => Flow::Continue(vec![
                again,
                Frame::Cast(CastFrame::new(p, CastSource::Ability { source, ability })),
            ]),
            // CR 117.1d / 605.3a: mana abilities with priority; the mana floats.
            OptionDesc::TapForMana {
                source,
                mana,
                ability,
            } => {
                // The ability's own cost may include sacrificing its source (Treasure).
                let cost = tap_for_mana(st, p, source, mana, ability, None);
                if cost.is_empty() {
                    Flow::then(again)
                } else {
                    Flow::Continue(vec![again, Frame::Apply(ApplyFrame::new(st, cost))])
                }
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
    // Cards a rule lets them play from another zone: their graveyard, the top of their
    // library (CR 601.3, 116.2a).
    let elsewhere = crate::players::playable_elsewhere(st, p);
    // CR 305.1–305.2: lands, as many as they may play this turn, sorcery timing — from the
    // hand, or from where a rule lets them.
    if sorcery_timing(st, p) && crate::players::land_play_left(st, p) {
        for &card in hand.iter().chain(&elsewhere) {
            if chars(st, card).is(CardType::Land) {
                opts.push(OptionDesc::PlayLand { card });
            }
        }
    }
    // CR 903.8: a player may cast a commander they own from the command zone.
    let commanders = st
        .zones
        .command
        .iter()
        .filter(|&&c| st.object(c).owner == p);
    for &card in hand.iter().chain(commanders).chain(&elsewhere) {
        if can_begin(st, p, CastSource::Card(card)) {
            opts.push(OptionDesc::CastSpell { card });
        }
    }
    for &source in &st.zones.battlefield {
        if crate::layers::controller_of(st, source) != p {
            continue;
        }
        for (ability, a) in abilities(st, source) {
            let option = OptionDesc::ActivateAbility { source, ability };
            // An ability it was given twice is one thing to do.
            if matches!(a, AbilityDef::Activated { .. })
                && !opts.contains(&option)
                && can_begin(st, p, CastSource::Ability { source, ability })
            {
                opts.push(option);
            }
        }
    }
    // CR 113.6: abilities that work from the hand (cycling, CR 702.29a) or the graveyard.
    let graveyard = &st.zones.graveyards[p.index()];
    for &source in hand.iter().chain(graveyard) {
        for (ability, a) in abilities(st, source) {
            let option = OptionDesc::ActivateAbility { source, ability };
            // An ability it was given twice is one thing to do.
            if matches!(a, AbilityDef::Activated { .. })
                && !opts.contains(&option)
                && can_begin(st, p, CastSource::Ability { source, ability })
            {
                opts.push(option);
            }
        }
    }
    for src in mana_sources(st, p, None, None) {
        for mana in types_in(src.mask) {
            let option = OptionDesc::TapForMana {
                source: src.id,
                mana,
                ability: src.ability,
            };
            // An ability the permanent has twice is one option.
            if !opts.contains(&option) {
                opts.push(option);
            }
        }
    }
    opts
}
