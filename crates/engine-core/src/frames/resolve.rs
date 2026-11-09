//! Resolving the top object of the stack (CR 608). Text: docs/rules-notes/608-resolution.md.
//!
//! An effect resolves as an explicit **work list** inside the frame (D-020): each item is
//! one instruction (CR 608.2c "in the order written"). Any item can suspend for a choice —
//! today the APNAP "each player" choices (CR 101.4); in M2, a replacement-effect choice
//! inside a single action — and resolution resumes exactly where it stopped.

use super::{Flow, Frame, Input};
use crate::actions::{GameAction, execute, move_object};
use crate::decision::{ChoiceKind, OptionDesc, Primitive};
use crate::events::{DamageTarget, Event};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::stack::{StackKind, Target};
use crate::state::GameState;
use crate::targeting::is_legal;
use crate::zones::{LibraryPosition, Zone};
use alloc::vec::Vec;
use card_model::{
    AbilityDef, Amount, Body, CardType, EachPlayerAction, Effect, PlayerRef, TargetFilter,
    TargetRef,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Work {
    /// Perform `effect` for the `k`-th chosen mode (targets = `targets[k]`).
    Eval { k: usize, effect: Effect },
    /// CR 101.4: players choose in APNAP order (later players know earlier choices,
    /// 101.4b); then all the actions happen simultaneously.
    EachPlayerChoose {
        action: EachPlayerAction,
        order: Vec<PlayerId>,
        next: usize,
        chosen: Vec<(PlayerId, Vec<ObjectId>)>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveFrame {
    /// The stack object being resolved (`None` until started).
    pub object: Option<ObjectId>,
    pub controller: PlayerId,
    /// What deals damage etc.: the spell itself, or the ability's source (CR 113.7a).
    pub source: ObjectId,
    pub x: u32,
    pub targets: Vec<Vec<Target>>,
    pub filters: Vec<Vec<TargetFilter>>,
    /// Remaining work; last = next.
    pub work: Vec<Work>,
}

impl ResolveFrame {
    pub(crate) fn new() -> Self {
        ResolveFrame {
            object: None,
            controller: PlayerId(0),
            source: ObjectId(0),
            x: 0,
            targets: Vec::new(),
            filters: Vec::new(),
            work: Vec::new(),
        }
    }

    fn target(&self, st: &GameState, k: usize, r: TargetRef) -> Option<Target> {
        let TargetRef::Target(i) = r;
        let t = *self.targets.get(k)?.get(i as usize)?;
        // CR 608.2b: illegal targets aren't affected.
        is_legal(
            st,
            self.filters[k][i as usize],
            t,
            self.controller,
            self.object,
        )
        .then_some(t)
    }

    fn player(&self, st: &GameState, k: usize, r: PlayerRef) -> Option<PlayerId> {
        match r {
            PlayerRef::You => st.in_game(self.controller).then_some(self.controller),
            PlayerRef::Target(i) => match self.target(st, k, TargetRef::Target(i))? {
                Target::Player(p) => Some(p),
                Target::Object(_) => None,
            },
        }
    }

    fn amount(&self, a: Amount) -> u32 {
        match a {
            Amount::Fixed(n) => n,
            Amount::X => self.x,
        }
    }

    pub(crate) fn run(mut self, st: &mut GameState, mut input: Option<Input>) -> Flow {
        if self.object.is_none()
            && let Some(flow) = self.start(st)
        {
            return flow;
        }
        while let Some(item) = self.work.pop() {
            match item {
                Work::Eval { k, effect } => self.eval(st, k, effect),
                Work::EachPlayerChoose {
                    action,
                    order,
                    mut next,
                    mut chosen,
                } => {
                    if let Some(inp) = input.take() {
                        let objs = inp
                            .picked()
                            .into_iter()
                            .map(|o| match o {
                                OptionDesc::Card { card } => *card,
                                _ => unreachable!(),
                            })
                            .collect();
                        chosen.push((order[next], objs));
                        next += 1;
                    }
                    while let Some(&p) = order.get(next) {
                        let (kind, n, pool) = each_player_options(st, action, p);
                        let n = n.min(pool.len());
                        if !st.in_game(p) || n == 0 {
                            chosen.push((p, Vec::new()));
                            next += 1;
                            continue;
                        }
                        self.work.push(Work::EachPlayerChoose {
                            action,
                            order,
                            next,
                            chosen,
                        });
                        let options = pool
                            .into_iter()
                            .map(|card| OptionDesc::Card { card })
                            .collect();
                        return Flow::ask(
                            Frame::Resolve(self),
                            p,
                            kind,
                            Primitive::PickN { min: n, max: n },
                            options,
                        );
                    }
                    // All players have chosen: the actions happen simultaneously.
                    for (p, objs) in chosen {
                        for o in objs {
                            let new =
                                move_object(st, o, Zone::Graveyard, LibraryPosition::Top, None);
                            st.emit(match action {
                                EachPlayerAction::Discard(_) => Event::Discarded {
                                    player: p,
                                    card: new,
                                },
                                _ => Event::Sacrificed {
                                    player: p,
                                    object: new,
                                },
                            });
                        }
                    }
                }
            }
        }
        self.finish(st);
        Flow::done()
    }

    /// CR 608.2a–b and 608.3. Returns `Some` if resolution ends here.
    fn start(&mut self, st: &mut GameState) -> Option<Flow> {
        let top = st.stack.last().expect("resolve with empty stack").clone();
        self.object = Some(top.id);
        self.controller = top.controller;
        self.x = top.x;
        self.targets = top.targets.clone();
        let body: Option<Body> = match &top.kind {
            StackKind::Spell => {
                self.source = top.id;
                let c = chars(st, top.id);
                if c.is_permanent_type() {
                    // CR 608.3a: it becomes a permanent under its controller's control.
                    st.emit(Event::Resolved { spell: top.id });
                    move_object(
                        st,
                        top.id,
                        Zone::Battlefield,
                        LibraryPosition::Top,
                        Some(top.controller),
                    );
                    return Some(Flow::done());
                }
                st.card_def(top.id).spell_ability().cloned()
            }
            StackKind::Ability {
                source,
                source_card,
                index,
            } => {
                self.source = *source;
                match &st.catalog.get(*source_card).abilities[*index as usize] {
                    AbilityDef::Activated { body, .. } => Some(body.clone()),
                    _ => unreachable!(),
                }
            }
        };
        let body = body.expect("non-permanent spells and abilities have a body");
        self.filters = top
            .modes
            .iter()
            .map(|&m| body.modes()[m as usize].targets.clone())
            .collect();
        // CR 608.2b: if it has targets and all of them are now illegal, it doesn't resolve.
        let mut any = false;
        let mut any_legal = false;
        for (k, ts) in self.targets.iter().enumerate() {
            for (i, &t) in ts.iter().enumerate() {
                any = true;
                any_legal |= is_legal(st, self.filters[k][i], t, self.controller, Some(top.id));
            }
        }
        if any && !any_legal {
            st.emit(Event::Fizzled { object: top.id });
            self.remove(st);
            return Some(Flow::done());
        }
        // CR 700.2: chosen modes are performed in printed order (modes are stored sorted).
        for (k, &m) in top.modes.iter().enumerate().rev() {
            self.work.push(Work::Eval {
                k,
                effect: body.modes()[m as usize].effect.clone(),
            });
        }
        None
    }

    fn eval(&mut self, st: &mut GameState, k: usize, effect: Effect) {
        match effect {
            Effect::Seq(v) => {
                for e in v.into_iter().rev() {
                    self.work.push(Work::Eval { k, effect: e });
                }
            }
            Effect::DealDamage { to, amount } => {
                let amount = self.amount(amount);
                if let Some(t) = self.target(st, k, to) {
                    let target = match t {
                        Target::Player(p) => DamageTarget::Player(p),
                        Target::Object(o) => DamageTarget::Object(o),
                    };
                    execute(
                        st,
                        GameAction::Damage {
                            source: self.source,
                            target,
                            amount,
                            combat: false,
                        },
                    );
                }
            }
            Effect::Draw { who, amount } => {
                if let Some(player) = self.player(st, k, who) {
                    for _ in 0..self.amount(amount) {
                        execute(st, GameAction::Draw { player });
                    }
                }
            }
            Effect::GainLife { who, amount } => {
                if let Some(player) = self.player(st, k, who) {
                    let amount = self.amount(amount);
                    execute(st, GameAction::GainLife { player, amount });
                }
            }
            Effect::LoseLife { who, amount } => {
                if let Some(player) = self.player(st, k, who) {
                    let amount = self.amount(amount);
                    execute(st, GameAction::LoseLife { player, amount });
                }
            }
            Effect::Counter { what } => {
                if let Some(Target::Object(spell)) = self.target(st, k, what) {
                    // CR 701.6a: a countered spell is put into its owner's graveyard.
                    st.emit(Event::Countered { object: spell });
                    move_object(st, spell, Zone::Graveyard, LibraryPosition::Top, None);
                }
            }
            Effect::EachPlayer(EachPlayerAction::Draw(n)) => {
                for player in st.apnap_order() {
                    for _ in 0..n {
                        execute(st, GameAction::Draw { player });
                    }
                }
            }
            Effect::EachPlayer(action) => self.work.push(Work::EachPlayerChoose {
                action,
                order: st.apnap_order(),
                next: 0,
                chosen: Vec::new(),
            }),
        }
    }

    /// CR 608.2n: the spell goes to the graveyard; an ability ceases to exist.
    /// CR 608.2m: if it already left the stack, there's nothing to do.
    fn finish(&mut self, st: &mut GameState) {
        let id = self.object.expect("started");
        if st.stack_object(id).is_some() {
            st.emit(Event::Resolved { spell: id });
            self.remove(st);
        }
    }

    /// Take the object off the stack (CR 608.2n; also CR 608.2b when it fizzles).
    fn remove(&mut self, st: &mut GameState) {
        let id = self.object.expect("started");
        match st.stack_object(id).map(|s| s.kind.clone()) {
            Some(StackKind::Spell) => {
                move_object(st, id, Zone::Graveyard, LibraryPosition::Top, None);
            }
            Some(StackKind::Ability { .. }) => {
                execute(st, GameAction::RemoveAbility { ability: id });
            }
            None => {}
        }
    }
}

/// The decision kind, count, and choosable objects for one player's part of an
/// each-player choice.
fn each_player_options(
    st: &GameState,
    action: EachPlayerAction,
    p: PlayerId,
) -> (ChoiceKind, usize, Vec<ObjectId>) {
    match action {
        EachPlayerAction::Discard(n) => (
            ChoiceKind::Discard,
            n as usize,
            st.zones.hands[p.index()].clone(),
        ),
        EachPlayerAction::SacrificeCreature(n) => (
            ChoiceKind::Sacrifice,
            n as usize,
            // CR 701.21a: only permanents they control.
            st.zones
                .battlefield
                .iter()
                .copied()
                .filter(|&id| st.object(id).controller == p && chars(st, id).is(CardType::Creature))
                .collect(),
        ),
        EachPlayerAction::Draw(_) => unreachable!("Draw has no choice"),
    }
}
