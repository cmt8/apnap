//! Resolving the top object of the stack (CR 608). Text: docs/rules-notes/608-resolution.md.
//!
//! An effect resolves as an explicit **work list** inside the frame (D-020): each item is
//! one instruction (CR 608.2c "in the order written"). An instruction that changes the game
//! produces proposed events, which go through the replacement pipeline in an `ApplyFrame`
//! pushed on top of this frame (D-024). Resolution then resumes with the next item. Any
//! item can suspend for a choice (APNAP each-player choices, CR 101.4; replacement choices
//! inside an action, CR 616.1) and resume exactly where it stopped.

use super::{ApplyFrame, Flow, Frame, Input};
use crate::actions::{GameAction, execute};
use crate::decision::{ChoiceKind, OptionDesc, Primitive};
use crate::events::{DamageTarget, Event};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::{Characteristics, chars};
use crate::replacement::{MoveCause, Proposed};
use crate::stack::{StackKind, Target};
use crate::state::GameState;
use crate::targeting::is_legal;
use crate::triggers::condition_holds;
use crate::zones::Zone;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use card_model::{
    AbilityDef, Amount, Body, CardType, EachPlayerAction, Effect, Keyword, ObjRef, PlayerRef,
    TargetFilter, TargetRef,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Work {
    /// Perform `effect` for the `k`-th chosen mode (targets = `targets[k]`).
    Eval { k: usize, effect: Box<Effect> },
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
    /// The ability's source as it last existed, if it has left the battlefield.
    pub source_lki: Option<Characteristics>,
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
            source_lki: None,
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
            &self.filters[k][i as usize],
            t,
            self.controller,
            self.object,
        )
        .then_some(t)
    }

    /// The players an effect applies to (none if a targeted player is no longer legal).
    fn players(&self, st: &GameState, k: usize, r: PlayerRef) -> Vec<PlayerId> {
        match r {
            PlayerRef::You => st
                .in_game(self.controller)
                .then_some(self.controller)
                .into_iter()
                .collect(),
            PlayerRef::Target(i) => match self.target(st, k, TargetRef::Target(i)) {
                Some(Target::Player(p)) => vec![p],
                _ => Vec::new(),
            },
            // In turn order from the controller (CR 101.4); the effects are simultaneous.
            PlayerRef::EachOpponent => st.opponents(self.controller),
        }
    }

    /// The one player an effect applies to (effects that can't be spread over several).
    fn player(&self, st: &GameState, k: usize, r: PlayerRef) -> Option<PlayerId> {
        self.players(st, k, r).into_iter().next()
    }

    /// An object the effect refers to, if it's still where the effect expects it.
    fn obj(&self, st: &GameState, k: usize, r: ObjRef) -> Option<ObjectId> {
        match r {
            ObjRef::Target(i) => match self.target(st, k, TargetRef::Target(i))? {
                Target::Object(o) => Some(o),
                Target::Player(_) => None,
            },
            ObjRef::This => st
                .objects
                .get(&self.source)
                .is_some_and(|o| o.zone == Zone::Battlefield)
                .then_some(self.source),
        }
    }

    /// The source's current characteristics, or its last known ones (CR 608.2h, 113.7a).
    fn source_chars(&self, st: &GameState) -> Option<Characteristics> {
        if st.objects.contains_key(&self.source) {
            Some(chars(st, self.source))
        } else {
            self.source_lki.clone()
        }
    }

    fn amount(&self, st: &GameState, a: &Amount) -> u32 {
        match a {
            Amount::Fixed(n) => *n,
            Amount::X => self.x,
            Amount::Count(_) | Amount::TotalPower(_) => {
                crate::layers::count_amount(st, a, self.source, self.controller)
            }
            Amount::SourcePower => self
                .source_chars(st)
                .and_then(|c| c.power)
                .unwrap_or(0)
                .max(0) as u32,
        }
    }

    fn damage(&self, st: &GameState, target: DamageTarget, amount: u32) -> Proposed {
        let c = self.source_chars(st);
        Proposed::Damage {
            source: self.source,
            deathtouch: c.as_ref().is_some_and(|c| c.has(Keyword::Deathtouch)),
            lifelink: c.as_ref().is_some_and(|c| c.has(Keyword::Lifelink)),
            target,
            amount,
            combat: false,
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
                Work::Eval { k, effect } => {
                    let events = self.eval(st, k, *effect);
                    if !events.is_empty() {
                        let apply = ApplyFrame::new(st, events);
                        return Flow::Continue(vec![
                            Frame::Resolve(Box::new(self)),
                            Frame::Apply(apply),
                        ]);
                    }
                }
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
                            Frame::Resolve(Box::new(self)),
                            p,
                            kind,
                            Primitive::PickN { min: n, max: n },
                            options,
                        );
                    }
                    // All players have chosen: the actions happen simultaneously.
                    let cause = match action {
                        EachPlayerAction::Discard(_) => MoveCause::Discard,
                        _ => MoveCause::Sacrifice,
                    };
                    let events: Vec<Proposed> = chosen
                        .into_iter()
                        .flat_map(|(_, objs)| objs)
                        .map(|o| Proposed::moving(o, Zone::Graveyard, cause))
                        .collect();
                    if !events.is_empty() {
                        let apply = ApplyFrame::new(st, events);
                        return Flow::Continue(vec![
                            Frame::Resolve(Box::new(self)),
                            Frame::Apply(apply),
                        ]);
                    }
                }
            }
        }
        self.finish(st)
    }

    /// CR 608.2a–b and 608.3. Returns `Some` if resolution ends here.
    fn start(&mut self, st: &mut GameState) -> Option<Flow> {
        let top = st.stack.last().expect("resolve with empty stack").clone();
        self.object = Some(top.id);
        self.controller = top.controller;
        self.x = top.x;
        self.targets = top.targets.clone();
        self.source_lki = top.source_lki.clone();
        let body: Body = match &top.kind {
            StackKind::Spell => {
                self.source = top.id;
                if chars(st, top.id).is_permanent_type() {
                    // CR 608.3b: an Aura spell checks its target; if it's illegal, the spell
                    // doesn't resolve and goes to its owner's graveyard.
                    let mut attach = None;
                    if let Some(filter) = st.card_def(top.id).enchant() {
                        let target = top.targets.first().and_then(|t| t.first()).copied();
                        match target {
                            Some(t @ Target::Object(o))
                                if is_legal(st, &filter, t, top.controller, Some(top.id)) =>
                            {
                                attach = Some(o)
                            }
                            _ => {
                                st.emit(Event::Fizzled { object: top.id });
                                return Some(self.leave_stack(st, MoveCause::Fizzled));
                            }
                        }
                    }
                    // CR 608.3a: it becomes a permanent under its controller's control (an
                    // Aura enters attached to its target, CR 303.4).
                    st.emit(Event::Resolved { spell: top.id });
                    let mut mv = Proposed::moving(top.id, Zone::Battlefield, MoveCause::Resolve);
                    if let Proposed::Move {
                        controller,
                        attach_to,
                        ..
                    } = &mut mv
                    {
                        *controller = Some(top.controller);
                        *attach_to = attach;
                    }
                    return Some(Flow::then(Frame::Apply(ApplyFrame::new(st, vec![mv]))));
                }
                st.card_def(top.id)
                    .spell_ability()
                    .cloned()
                    .expect("instants and sorceries have a spell ability")
            }
            StackKind::Ability {
                source,
                source_card,
                index,
                ..
            } => {
                self.source = *source;
                match &st.catalog.get(*source_card).abilities[*index as usize] {
                    AbilityDef::Activated { body, .. } => body.clone(),
                    AbilityDef::Triggered {
                        body,
                        intervening_if,
                        ..
                    } => {
                        // CR 603.4 / 608.2a: re-check an intervening "if" on resolution.
                        if !condition_holds(st, intervening_if.as_ref(), top.controller, *source) {
                            execute(st, GameAction::RemoveAbility { ability: top.id });
                            return Some(Flow::done());
                        }
                        body.clone()
                    }
                    other => unreachable!("not a stack ability: {other:?}"),
                }
            }
        };
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
                any_legal |= is_legal(st, &self.filters[k][i], t, self.controller, Some(top.id));
            }
        }
        if any && !any_legal {
            st.emit(Event::Fizzled { object: top.id });
            return Some(self.leave_stack(st, MoveCause::Fizzled));
        }
        // CR 700.2: chosen modes are performed in printed order (modes are stored sorted).
        for (k, &m) in top.modes.iter().enumerate().rev() {
            self.work.push(Work::Eval {
                k,
                effect: Box::new(body.modes()[m as usize].effect.clone()),
            });
        }
        None
    }

    /// One instruction. Returns the events it would cause (empty if none or done directly).
    fn eval(&mut self, st: &mut GameState, k: usize, effect: Effect) -> Vec<Proposed> {
        match effect {
            Effect::Seq(v) => {
                for e in v.into_iter().rev() {
                    self.work.push(Work::Eval {
                        k,
                        effect: Box::new(e),
                    });
                }
                Vec::new()
            }
            Effect::DealDamage { to, amount } => {
                let amount = self.amount(st, &amount);
                match self.target(st, k, to) {
                    Some(Target::Player(p)) => {
                        vec![self.damage(st, DamageTarget::Player(p), amount)]
                    }
                    Some(Target::Object(o)) => {
                        vec![self.damage(st, DamageTarget::Object(o), amount)]
                    }
                    None => Vec::new(),
                }
            }
            Effect::DamageEachPlayer { amount } => {
                let amount = self.amount(st, &amount);
                st.apnap_order()
                    .into_iter()
                    .map(|p| self.damage(st, DamageTarget::Player(p), amount))
                    .collect()
            }
            Effect::Draw { who, amount } => {
                let n = self.amount(st, &amount);
                self.players(st, k, who)
                    .into_iter()
                    .flat_map(|player| (0..n).map(move |_| Proposed::Draw { player }))
                    .collect()
            }
            Effect::GainLife { who, amount } => {
                let amount = self.amount(st, &amount);
                self.players(st, k, who)
                    .into_iter()
                    .map(|player| Proposed::GainLife { player, amount })
                    .collect()
            }
            Effect::LoseLife { who, amount } => {
                let amount = self.amount(st, &amount);
                self.players(st, k, who)
                    .into_iter()
                    .map(|player| Proposed::LoseLife { player, amount })
                    .collect()
            }
            Effect::SetX(amount) => {
                self.x = self.amount(st, &amount);
                Vec::new()
            }
            Effect::DestroyAll(sel) => {
                crate::layers::matching(st, &sel, self.source, self.controller)
                    .into_iter()
                    .map(|o| Proposed::moving(o, Zone::Graveyard, MoveCause::Destroy))
                    .collect()
            }
            Effect::DamageEach { each, amount } => {
                let amount = self.amount(st, &amount);
                crate::layers::matching(st, &each, self.source, self.controller)
                    .into_iter()
                    .map(|o| self.damage(st, DamageTarget::Object(o), amount))
                    .collect()
            }
            Effect::Exile { what } => match self.obj(st, k, what) {
                // CR 701.13a
                Some(o) => vec![Proposed::moving(o, Zone::Exile, MoveCause::Rule)],
                None => Vec::new(),
            },
            Effect::ExileGraveyard { who } => self
                .players(st, k, who)
                .into_iter()
                .flat_map(|p| st.zones.list(Zone::Graveyard, p).clone())
                .map(|o| Proposed::moving(o, Zone::Exile, MoveCause::Rule))
                .collect(),
            Effect::Mill { who, amount } => {
                // CR 701.17a: the top N cards (the library's last entries), all at once.
                let n = self.amount(st, &amount) as usize;
                self.players(st, k, who)
                    .into_iter()
                    .flat_map(|p| {
                        let library = st.zones.list(Zone::Library, p);
                        let take = n.min(library.len());
                        library[library.len() - take..]
                            .iter()
                            .rev()
                            .copied()
                            .collect::<Vec<_>>()
                    })
                    .map(|o| Proposed::moving(o, Zone::Graveyard, MoveCause::Rule))
                    .collect()
            }
            Effect::ApplyAll {
                affected,
                effects,
                until_end_of_turn,
            } => {
                // CR 611.2c: the affected set is locked in now.
                let affected = crate::layers::matching(st, &affected, self.source, self.controller);
                if !affected.is_empty() {
                    execute(
                        st,
                        GameAction::AddContinuous {
                            source: self.source,
                            controller: self.controller,
                            affected,
                            effects,
                            until_end_of_turn,
                        },
                    );
                }
                Vec::new()
            }
            Effect::Counter { what } => match self.target(st, k, what) {
                // CR 701.6a: a countered spell is put into its owner's graveyard.
                Some(Target::Object(spell)) => {
                    st.emit(Event::Countered { object: spell });
                    vec![Proposed::moving(
                        spell,
                        Zone::Graveyard,
                        MoveCause::Countered,
                    )]
                }
                _ => Vec::new(),
            },
            Effect::ReturnToHand { what } => match self.obj(st, k, what) {
                Some(o) => vec![Proposed::moving(o, Zone::Hand, MoveCause::Rule)],
                None => Vec::new(),
            },
            Effect::Destroy { what } => match self.obj(st, k, what) {
                // CR 701.8a
                Some(o) => vec![Proposed::moving(o, Zone::Graveyard, MoveCause::Destroy)],
                None => Vec::new(),
            },
            Effect::CreateToken { token, count, who } => {
                let token = st.catalog.find(&token).expect("validated by card-db");
                match self.player(st, k, who) {
                    Some(controller) => vec![Proposed::CreateTokens {
                        token,
                        count: self.amount(st, &count),
                        controller,
                    }],
                    None => Vec::new(),
                }
            }
            Effect::PutCounters { what, kind, count } => match self.obj(st, k, what) {
                Some(object) => vec![Proposed::PutCounters {
                    object,
                    kind,
                    count: self.amount(st, &count),
                }],
                None => Vec::new(),
            },
            Effect::Apply {
                what,
                effects,
                until_end_of_turn,
            } => {
                // CR 611.2c: the affected set is locked in now.
                if let Some(o) = self.obj(st, k, what) {
                    execute(
                        st,
                        GameAction::AddContinuous {
                            source: self.source,
                            controller: self.controller,
                            affected: vec![o],
                            effects,
                            until_end_of_turn,
                        },
                    );
                }
                Vec::new()
            }
            Effect::PreventNext { to, amount } => {
                if let Some(target) = self.target(st, k, to) {
                    execute(st, GameAction::AddShield { target, amount });
                }
                Vec::new()
            }
            Effect::EachPlayer(EachPlayerAction::Draw(n)) => st
                .apnap_order()
                .into_iter()
                .flat_map(|player| (0..n).map(move |_| Proposed::Draw { player }))
                .collect(),
            Effect::EachPlayer(action) => {
                self.work.push(Work::EachPlayerChoose {
                    action,
                    order: st.apnap_order(),
                    next: 0,
                    chosen: Vec::new(),
                });
                Vec::new()
            }
        }
    }

    /// CR 608.2n: as the final part of resolution, the spell goes to its owner's graveyard;
    /// an ability ceases to exist. CR 608.2m: if it already left the stack, nothing to do.
    fn finish(self, st: &mut GameState) -> Flow {
        let id = self.object.expect("started");
        if st.stack_object(id).is_none() {
            return Flow::done();
        }
        st.emit(Event::Resolved { spell: id });
        self.leave_stack(st, MoveCause::Resolve)
    }

    /// Take the object off the stack (CR 608.2n, or 608.2b when it fizzles). A spell card
    /// moving to the graveyard is a replaceable event.
    fn leave_stack(&self, st: &mut GameState, cause: MoveCause) -> Flow {
        let id = self.object.expect("started");
        match st.stack_object(id).map(|s| s.kind.clone()) {
            Some(StackKind::Spell) => {
                let mv = Proposed::moving(id, Zone::Graveyard, cause);
                Flow::then(Frame::Apply(ApplyFrame::new(st, vec![mv])))
            }
            Some(StackKind::Ability { .. }) => {
                execute(st, GameAction::RemoveAbility { ability: id });
                Flow::done()
            }
            None => Flow::done(),
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
                .filter(|&id| {
                    let c = chars(st, id);
                    c.controller == p && c.is(CardType::Creature)
                })
                .collect(),
        ),
        EachPlayerAction::Draw(_) => unreachable!("Draw has no choice"),
    }
}
