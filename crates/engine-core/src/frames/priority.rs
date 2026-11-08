//! Priority (CR 117), casting (CR 601.2) and resolution (CR 608), M0 subset: lands and
//! vanilla permanent spells.

use super::{Flow, Frame, Input};
use crate::actions::{GameAction, execute, move_object};
use crate::decision::{ChoiceKind, OptionDesc};
use crate::events::Event;
use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::mana::{ManaRequirement, ManaType};
use crate::sba;
use crate::state::GameState;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec;
use alloc::vec::Vec;
use card_model::CardType;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorityFrame {
    /// The player who would receive priority.
    pub player: PlayerId,
    /// Consecutive passes so far (CR 117.4 "pass in succession").
    pub passes: u32,
}

impl PriorityFrame {
    pub(crate) fn run(self, st: &mut GameState, input: Option<Input>) -> Flow {
        let Some(inp) = input else {
            // CR 117.5 / 704.3: before a player would receive priority, perform SBAs
            // repeatedly until none are performed (then put triggers on the stack: M2).
            while sba::check_once(st) {
                if st.is_over() {
                    return Flow::done();
                }
            }
            let options = priority_options(st, self.player);
            let p = self.player;
            return Flow::ask_one(Frame::Priority(self), p, ChoiceKind::Priority, options);
        };
        let p = self.player;
        match inp.picked_one().cloned().expect("priority pick") {
            OptionDesc::Pass => {
                // CR 117.3d
                st.emit(Event::PriorityPassed { player: p });
                let passes = self.passes + 1;
                // CR 117.4: only players still in the game count.
                if passes as usize >= st.players_in_game() {
                    if st.zones.stack.is_empty() {
                        // CR 500.2: the step or phase ends.
                        Flow::done()
                    } else {
                        // CR 117.4: the top object resolves; CR 117.3b: then AP gets priority.
                        Flow::Continue(vec![
                            Frame::Priority(PriorityFrame {
                                player: st.turn.active,
                                passes: 0,
                            }),
                            Frame::Resolve,
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
                Flow::then(Frame::Priority(PriorityFrame {
                    player: p,
                    passes: 0,
                }))
            }
            OptionDesc::CastSpell { card } => Flow::Continue(vec![
                // CR 117.3c: the caster receives priority afterward.
                Frame::Priority(PriorityFrame {
                    player: p,
                    passes: 0,
                }),
                Frame::CastSpell(CastFrame {
                    player: p,
                    card,
                    stage: CastStage::Announce,
                }),
            ]),
            o => unreachable!("not a priority option: {o:?}"),
        }
    }
}

/// Untapped lands `p` controls and the mana each one's intrinsic ability makes (CR 305.6).
///
/// M0 limitation (D-010): only lands with exactly one basic land type are mana sources.
fn mana_sources(st: &GameState, p: PlayerId) -> Vec<(ObjectId, ManaType)> {
    st.zones
        .battlefield
        .iter()
        .filter_map(|&id| {
            let o = st.object(id);
            if o.controller != p || o.tapped {
                return None;
            }
            let c = chars(st, id);
            match (c.is(CardType::Land), c.land_types.as_slice()) {
                (true, [t]) => Some((id, ManaType::from_color(t.produces()))),
                _ => None,
            }
        })
        .collect()
}

fn source_counts(sources: &[(ObjectId, ManaType)]) -> [u32; 6] {
    let mut counts = [0u32; 6];
    for (_, t) in sources {
        counts[t.index()] += 1;
    }
    counts
}

fn can_pay(st: &GameState, p: PlayerId, req: &ManaRequirement) -> bool {
    req.feasible(
        &st.player(p).mana_pool,
        &source_counts(&mana_sources(st, p)),
    )
}

/// Legal priority actions in stable order: Pass, then lands, then spells (hand order).
fn priority_options(st: &GameState, p: PlayerId) -> Vec<OptionDesc> {
    let mut opts = vec![OptionDesc::Pass];
    // CR 305.1 / 307.1: sorcery timing: own main phase, empty stack.
    let sorcery_timing =
        st.turn.active == p && st.turn.step.is_main_phase() && st.zones.stack.is_empty();
    if !sorcery_timing {
        return opts;
    }
    let hand = &st.zones.hands[p.index()];
    // CR 305.2: one land per turn.
    if st.player(p).lands_played_this_turn < 1 {
        for &card in hand {
            if chars(st, card).is(CardType::Land) {
                opts.push(OptionDesc::PlayLand { card });
            }
        }
    }
    for &card in hand {
        let c = chars(st, card);
        if c.is(CardType::Land) {
            continue;
        }
        // M0: only creature spells exist in the test pool.
        if c.is(CardType::Creature) {
            let req = ManaRequirement::from_cost(&c.mana_cost.clone().unwrap_or_default());
            if can_pay(st, p, &req) {
                opts.push(OptionDesc::CastSpell { card });
            }
        }
    }
    opts
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastFrame {
    pub player: PlayerId,
    /// The card in hand before `Announce`, the spell on the stack after.
    pub card: ObjectId,
    pub stage: CastStage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastStage {
    Announce,
    /// CR 601.2g–h: activating mana abilities, then paying the locked-in total cost.
    Pay {
        cost: ManaRequirement,
    },
}

impl CastFrame {
    pub(crate) fn run(mut self, st: &mut GameState, mut input: Option<Input>) -> Flow {
        let p = self.player;
        if let CastStage::Announce = self.stage {
            // CR 601.2a: move the card to the stack; the caster becomes its controller.
            self.card = move_object(st, self.card, Zone::Stack, LibraryPosition::Top, Some(p));
            // CR 601.2b–e: no modes, targets or divisions for vanilla spells.
            // CR 601.2f: total cost = mana cost (no additional costs/reductions in M0).
            let c = chars(st, self.card);
            let cost = ManaRequirement::from_cost(&c.mana_cost.unwrap_or_default());
            self.stage = CastStage::Pay { cost };
        }
        let CastStage::Pay { cost } = self.stage else {
            unreachable!()
        };
        if let Some(inp) = input.take() {
            let Some(OptionDesc::TapForMana { source, mana }) = inp.picked_one().cloned() else {
                unreachable!()
            };
            // CR 605.3: a mana ability is activated and resolves immediately.
            execute(st, GameAction::Tap { object: source });
            execute(st, GameAction::AddMana { player: p, mana });
        }
        if cost.payable_from(&st.player(p).mana_pool) {
            // CR 601.2h: pay the total cost. CR 601.2i: the spell becomes cast.
            execute(
                st,
                GameAction::PayMana {
                    player: p,
                    requirement: cost,
                },
            );
            st.emit(Event::SpellCast {
                player: p,
                spell: self.card,
            });
            return Flow::done();
        }
        // CR 601.2g: activate mana abilities. Only offer sources that keep the cost payable,
        // so a payment can never dead-end (the cast option was offered only if payable).
        let sources = mana_sources(st, p);
        let counts = source_counts(&sources);
        let pool = st.player(p).mana_pool;
        let options: Vec<OptionDesc> = sources
            .iter()
            .filter(|(_, t)| {
                let mut c = counts;
                c[t.index()] -= 1;
                let mut pl = pool;
                pl.add(*t, 1);
                cost.feasible(&pl, &c)
            })
            .map(|&(source, mana)| OptionDesc::TapForMana { source, mana })
            .collect();
        assert!(
            !options.is_empty(),
            "cast of {:?} cannot be paid",
            self.card
        );
        Flow::ask_one(Frame::CastSpell(self), p, ChoiceKind::PayMana, options)
    }
}

/// CR 608.3: a permanent spell resolves by entering the battlefield under its controller.
pub(crate) fn resolve_top(st: &mut GameState, input: Option<Input>) -> Flow {
    debug_assert!(input.is_none());
    let spell = *st.zones.stack.last().expect("resolve with empty stack");
    let controller = st.object(spell).controller;
    let c = chars(st, spell);
    st.emit(Event::Resolved { spell });
    let permanent = [
        CardType::Artifact,
        CardType::Battle,
        CardType::Creature,
        CardType::Enchantment,
        CardType::Land,
        CardType::Planeswalker,
    ]
    .iter()
    .any(|t| c.is(*t));
    if permanent {
        move_object(
            st,
            spell,
            Zone::Battlefield,
            LibraryPosition::Top,
            Some(controller),
        );
    } else {
        // CR 608.2n: instants/sorceries go to the graveyard (none in M0).
        move_object(st, spell, Zone::Graveyard, LibraryPosition::Top, None);
    }
    Flow::done()
}
