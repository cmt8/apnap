//! Starting the game (CR 103 (2026-11-06)).

use super::{Flow, Frame, Input, StepStage};
use crate::actions::{GameAction, execute, move_object};
use crate::decision::{ChoiceKind, OptionDesc, Primitive};
use crate::events::Event;
use crate::ids::{ObjectId, PlayerId};
use crate::state::GameState;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetupFrame {
    Begin,
    /// CR 103.1: `chooser` decides who takes the first turn.
    AwaitStartingPlayer {
        chooser: PlayerId,
    },
    /// CR 103.8: mulligans are done; the starting player takes their first turn.
    StartFirstTurn,
}

impl SetupFrame {
    pub(crate) fn run(self, st: &mut GameState, input: Option<Input>) -> Flow {
        match self {
            SetupFrame::Begin => {
                st.emit(Event::GameStarted {
                    seed: st.seed,
                    num_players: st.num_players() as u8,
                });
                // CR 103.1: "any mutually agreeable method (flipping a coin, rolling dice…)".
                let chooser = PlayerId(st.rng.below(st.num_players() as u64) as u8);
                let options = (0..st.num_players())
                    .map(|i| OptionDesc::Player {
                        player: PlayerId(i as u8),
                    })
                    .collect();
                Flow::ask_one(
                    Frame::Setup(SetupFrame::AwaitStartingPlayer { chooser }),
                    chooser,
                    ChoiceKind::ChooseStartingPlayer,
                    options,
                )
            }
            SetupFrame::AwaitStartingPlayer { chooser } => {
                let input = input.expect("starting player decision");
                let Some(OptionDesc::Player { player: starting }) = input.picked_one().cloned()
                else {
                    unreachable!()
                };
                st.turn.starting_player = Some(starting);
                st.emit(Event::StartingPlayerChosen { chooser, starting });
                let order = st.turn_order_from(starting);
                // CR 103.2: no additional steps in M0 (commanders → M3).
                // CR 103.3: each player shuffles their deck.
                for &p in &order {
                    execute(st, GameAction::Shuffle { player: p });
                }
                // CR 103.4: starting life was set by `new_game` from the config.
                // CR 103.5: each player draws their starting hand.
                for &p in &order {
                    draw_n(st, p, st.config.starting_hand_size);
                }
                let n = st.num_players();
                Flow::Continue(vec![
                    Frame::Setup(SetupFrame::StartFirstTurn),
                    Frame::Mulligan(MulliganFrame {
                        order,
                        kept: vec![false; n],
                        mulligans: vec![0; n],
                        stage: MulliganStage::Declare {
                            next: 0,
                            taking: Vec::new(),
                        },
                    }),
                ])
            }
            SetupFrame::StartFirstTurn => {
                // CR 103.6: no opening-hand actions in M0. CR 103.8: first turn.
                let starting = st.turn.starting_player.expect("starting player");
                crate::frames::turn::begin_turn(st, starting);
                Flow::then(Frame::Step(StepStage::Begin))
            }
        }
    }
}

fn draw_n(st: &mut GameState, p: PlayerId, n: u32) {
    for _ in 0..n {
        execute(st, GameAction::Draw { player: p });
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MulliganFrame {
    /// Turn order beginning with the starting player.
    pub order: Vec<PlayerId>,
    /// By seat.
    pub kept: Vec<bool>,
    /// Mulligans taken, by seat.
    pub mulligans: Vec<u32>,
    pub stage: MulliganStage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MulliganStage {
    /// Declarations in turn order; `taking` collects players who declared a mulligan.
    Declare { next: usize, taking: Vec<PlayerId> },
    /// Each mulliganing player (in turn order) chooses cards to put on the bottom.
    Bottom { taking: Vec<PlayerId>, next: usize },
    /// …and then orders them (only when two or more).
    BottomOrder {
        taking: Vec<PlayerId>,
        next: usize,
        cards: Vec<ObjectId>,
    },
}

impl MulliganFrame {
    /// Mulligans that count toward the bottom count / mulligan limit (CR 103.5c).
    fn counted(&self, st: &GameState, p: PlayerId) -> u32 {
        let m = self.mulligans[p.index()];
        if st.config.is_multiplayer() {
            m.saturating_sub(1)
        } else {
            m
        }
    }

    /// CR 103.5: "A player can take mulligans until their opening hand would be zero cards,
    /// after which they may not take further mulligans."
    fn may_mulligan(&self, st: &GameState, p: PlayerId) -> bool {
        self.counted(st, p) < st.config.starting_hand_size
    }

    pub(crate) fn run(mut self, st: &mut GameState, mut input: Option<Input>) -> Flow {
        loop {
            match core::mem::replace(
                &mut self.stage,
                MulliganStage::Declare {
                    next: 0,
                    taking: Vec::new(),
                },
            ) {
                MulliganStage::Declare {
                    mut next,
                    mut taking,
                } => {
                    if let Some(inp) = input.take() {
                        let p = self.order[next];
                        let mulligan = matches!(inp.picked_one(), Some(OptionDesc::Mulligan));
                        st.emit(Event::MulliganDeclared {
                            player: p,
                            mulligan,
                        });
                        if mulligan {
                            taking.push(p);
                        } else {
                            self.kept[p.index()] = true;
                        }
                        next += 1;
                    }
                    // First the starting player declares, then each other player in turn order.
                    while next < self.order.len() {
                        let p = self.order[next];
                        if self.kept[p.index()] {
                            next += 1;
                        } else if !self.may_mulligan(st, p) {
                            self.kept[p.index()] = true;
                            next += 1;
                        } else {
                            self.stage = MulliganStage::Declare { next, taking };
                            return Flow::ask_one(
                                Frame::Mulligan(self),
                                p,
                                ChoiceKind::Mulligan,
                                vec![OptionDesc::Keep, OptionDesc::Mulligan],
                            );
                        }
                    }
                    if taking.is_empty() {
                        // No player took a mulligan: the process is complete.
                        return Flow::done();
                    }
                    // All players who decided to take mulligans do so at the same time.
                    for &p in &taking {
                        for card in st.zones.hands[p.index()].clone() {
                            move_object(st, card, Zone::Library, LibraryPosition::Top, None);
                        }
                        execute(st, GameAction::Shuffle { player: p });
                        draw_n(st, p, st.config.starting_hand_size);
                        self.mulligans[p.index()] += 1;
                    }
                    self.stage = MulliganStage::Bottom { taking, next: 0 };
                }
                MulliganStage::Bottom { taking, mut next } => {
                    if let Some(inp) = input.take() {
                        let cards: Vec<ObjectId> = inp
                            .picked()
                            .into_iter()
                            .map(|o| match o {
                                OptionDesc::Card { card } => *card,
                                _ => unreachable!(),
                            })
                            .collect();
                        if cards.len() >= 2 {
                            let p = taking[next];
                            let options = cards
                                .iter()
                                .map(|&card| OptionDesc::Card { card })
                                .collect();
                            self.stage = MulliganStage::BottomOrder {
                                taking,
                                next,
                                cards,
                            };
                            return Flow::ask(
                                Frame::Mulligan(self),
                                p,
                                ChoiceKind::MulliganBottom,
                                Primitive::Order,
                                options,
                            );
                        }
                        put_on_bottom(st, &cards);
                        next += 1;
                    }
                    while next < taking.len() {
                        let p = taking[next];
                        let n = self.counted(st, p) as usize;
                        let hand = st.zones.hands[p.index()].clone();
                        let n = n.min(hand.len());
                        if n == 0 {
                            next += 1;
                            continue;
                        }
                        self.stage = MulliganStage::Bottom { taking, next };
                        let options = hand
                            .into_iter()
                            .map(|card| OptionDesc::Card { card })
                            .collect();
                        return Flow::ask(
                            Frame::Mulligan(self),
                            p,
                            ChoiceKind::MulliganBottom,
                            Primitive::PickN { min: n, max: n },
                            options,
                        );
                    }
                    // Repeat the declaration round until no player takes a mulligan.
                    self.stage = MulliganStage::Declare {
                        next: 0,
                        taking: Vec::new(),
                    };
                }
                MulliganStage::BottomOrder {
                    taking,
                    next,
                    cards,
                } => {
                    let inp = input.take().expect("bottom order decision");
                    let ordered: Vec<ObjectId> = inp.order().iter().map(|&i| cards[i]).collect();
                    put_on_bottom(st, &ordered);
                    self.stage = MulliganStage::Bottom {
                        taking,
                        next: next + 1,
                    };
                }
            }
        }
    }
}

/// Put cards on the bottom one at a time in the given order: the last one ends up bottom-most.
fn put_on_bottom(st: &mut GameState, cards: &[ObjectId]) {
    for &c in cards {
        move_object(st, c, Zone::Library, LibraryPosition::Bottom, None);
    }
}
