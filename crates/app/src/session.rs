//! A game session: the engine state, the AI seats, the human's stops, and the response log.
//! No graphics here, so everything is testable headlessly; `ui.rs` only draws it.
//!
//! The session owns `GameState`. AI agents are handed only their `PlayerView` (CLAUDE.md
//! invariant 6). "Stops" are a UI concern (spec §6): the engine always asks, and the session
//! answers trivial priority decisions for the human according to the configured stops.

use engine_core::ObjectId;
use engine_core::decision::{ChoiceKind, OptionDesc, PendingDecision, Primitive, Response};
use engine_core::turn::Step;
use engine_core::{Agent, GameState, IllegalResponse, PlayerId, PlayerView, player_view};
use harness::text::Narrator;
use harness::{Agents, Replay, Setup};
use std::collections::{BTreeMap, BTreeSet};

/// Steps at which the human is offered priority even when the stack is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stops {
    pub mine: BTreeSet<Step>,
    pub theirs: BTreeSet<Step>,
    /// Tap lands automatically when paying costs (same responses a player could give).
    pub auto_pay: bool,
    /// Keep priority after casting a spell or activating an ability. Off (the default):
    /// priority is passed automatically while the top of the stack is the human's own.
    pub hold_priority: bool,
}

impl Default for Stops {
    fn default() -> Self {
        Stops {
            mine: [Step::PrecombatMain, Step::PostcombatMain]
                .into_iter()
                .collect(),
            theirs: BTreeSet::new(),
            auto_pay: true,
            hold_priority: false,
        }
    }
}

/// The creature a per-creature decision is about (the attacker or blocker being asked).
pub fn subject(d: &PendingDecision) -> Option<ObjectId> {
    d.options.iter().find_map(|o| match o {
        OptionDesc::Attack { attacker, .. } | OptionDesc::AttackPlaneswalker { attacker, .. } => {
            Some(*attacker)
        }
        OptionDesc::Block { blocker, .. } => Some(*blocker),
        _ => None,
    })
}

/// One creature the human is asked about while declaring attackers or blockers, with what
/// it may do (attack one of these players / block one of these attackers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub creature: ObjectId,
    pub options: Vec<OptionDesc>,
}

/// A whole attack or block declaration. The engine asks about one creature at a time (CR
/// 508.1 / 509.1 are declared as a whole; the engine's protocol is per creature); the UI
/// lets the human plan all of it on the board and then answers creature by creature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub kind: ChoiceKind,
    pub candidates: Vec<Candidate>,
}

pub struct Session {
    pub setup: Setup,
    pub seed: u64,
    pub opponents: Agents,
    pub st: GameState,
    /// `None` = spectating (all seats are AI) or viewing a replay.
    pub human: Option<PlayerId>,
    agents: Vec<Box<dyn Agent>>,
    /// Every response so far, from every seat: the save file (spec §10.3).
    pub responses: Vec<Response>,
    /// The event log from the human's point of view (opponents' draws hidden).
    pub log: Vec<String>,
    narrator: Narrator,
    pub stops: Stops,
    /// Pass priority automatically until this turn is over, whatever happens.
    pub pass_turn: Option<u32>,
    /// Pass priority automatically until this turn is over, unless an opponent puts
    /// something on the stack.
    pub yield_turn: Option<u32>,
    /// Replay viewing: the remaining recorded responses (front = next).
    pub replay_queue: Option<std::collections::VecDeque<Response>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session({} seed {})", self.setup.label(), self.seed)
    }
}

impl Session {
    pub fn new(setup: Setup, seed: u64, human: Option<PlayerId>, opponents: Agents) -> Self {
        let mut st = setup.game(seed);
        let narrator = Narrator::new(&st);
        let agents = (0..setup.players())
            .map(|s| opponents.make(seed, s, &st.catalog))
            .collect();
        let out = engine_core::start(&mut st);
        let mut s = Session {
            setup,
            seed,
            opponents,
            st,
            human,
            agents,
            responses: Vec::new(),
            log: Vec::new(),
            narrator,
            stops: Stops::default(),
            pass_turn: None,
            yield_turn: None,
            replay_queue: None,
        };
        s.narrate(&out.events);
        s
    }

    /// Wrap an existing position (a test scenario). Saving such a session isn't meaningful:
    /// it has no setup or seed to rebuild from.
    pub fn from_state(mut st: GameState, human: Option<PlayerId>, opponents: Agents) -> Self {
        let narrator = Narrator::new(&st);
        let agents = (0..st.num_players() as u8)
            .map(|s| opponents.make(st.seed, s, &st.catalog))
            .collect();
        // Runs to the first decision if the position hasn't been started yet.
        let out = engine_core::start(&mut st);
        let mut s = Session {
            setup: Setup::M0Vanilla,
            seed: st.seed,
            opponents,
            st,
            human,
            agents,
            responses: Vec::new(),
            log: Vec::new(),
            narrator,
            stops: Stops::default(),
            pass_turn: None,
            yield_turn: None,
            replay_queue: None,
        };
        s.narrate(&out.events);
        s
    }

    fn narrate(&mut self, events: &[engine_core::Event]) {
        let lines = self.narrator.lines(events, self.human);
        self.log.extend(
            lines
                .into_iter()
                .map(|l| l.trim_start_matches('\n').to_string()),
        );
    }

    /// The seat whose view is shown: the human, or seat 0 when spectating.
    pub fn viewer(&self) -> PlayerId {
        self.human.unwrap_or(PlayerId(0))
    }

    pub fn view(&self) -> PlayerView {
        player_view(&self.st, self.viewer())
    }

    pub fn is_over(&self) -> bool {
        self.st.is_over()
    }

    /// The decision waiting for the human, if any.
    pub fn human_decision(&self) -> Option<&PendingDecision> {
        self.st
            .pending
            .as_ref()
            .filter(|d| Some(d.player) == self.human)
    }

    fn apply(&mut self, r: Response) -> Result<(), IllegalResponse> {
        let out = engine_core::step(&mut self.st, r.clone())?;
        self.responses.push(r);
        self.narrate(&out.events);
        Ok(())
    }

    /// The human answers the pending decision.
    pub fn respond(&mut self, r: Response) -> Result<(), IllegalResponse> {
        if self.human_decision().is_none() {
            return Err(IllegalResponse::NoPendingDecision);
        }
        self.apply(r)
    }

    /// The declaration the human is in the middle of being asked for, from the creature
    /// currently asked about onwards. Found by answering "no" for each creature on a copy
    /// of the game; nothing here is hidden from the human (they are their own creatures).
    pub fn declaration(&self) -> Option<Declaration> {
        let kind = self.human_decision()?.kind;
        if !matches!(
            kind,
            ChoiceKind::DeclareAttackers | ChoiceKind::DeclareBlockers
        ) {
            return None;
        }
        let mut sim = self.st.clone();
        let mut candidates = Vec::new();
        while let Some(d) = sim.pending.clone() {
            if Some(d.player) != self.human || d.kind != kind || candidates.len() >= 1_000 {
                break;
            }
            let Some(creature) = subject(&d) else {
                break;
            };
            // An illegal block declaration starts over (CR 734): the same creature again.
            if candidates
                .iter()
                .any(|c: &Candidate| c.creature == creature)
            {
                break;
            }
            candidates.push(Candidate {
                creature,
                options: d.options,
            });
            if engine_core::step(&mut sim, Response::Pick(Vec::new())).is_err() {
                break;
            }
        }
        Some(Declaration { kind, candidates })
    }

    /// Answer a whole declaration: `choices` gives the option chosen for each creature that
    /// attacks or blocks; every other creature asked about doesn't.
    pub fn declare(
        &mut self,
        choices: &BTreeMap<ObjectId, OptionDesc>,
    ) -> Result<(), IllegalResponse> {
        let Some(declaration) = self.declaration() else {
            return Err(IllegalResponse::NoPendingDecision);
        };
        for c in &declaration.candidates {
            let Some(d) = self.human_decision().cloned() else {
                break;
            };
            if d.kind != declaration.kind || subject(&d) != Some(c.creature) {
                break;
            }
            let pick = choices
                .get(&c.creature)
                .and_then(|o| d.options.iter().position(|x| x == o));
            self.apply(Response::Pick(pick.into_iter().collect()))?;
        }
        Ok(())
    }

    /// Start passing priority automatically for the rest of this turn.
    pub fn pass_the_turn(&mut self) {
        self.pass_turn = Some(self.st.turn.turn_number);
    }

    /// Pass priority automatically for the rest of this turn, but stop as soon as an
    /// opponent's spell or ability is on the stack.
    pub fn yield_the_turn(&mut self) {
        self.yield_turn = Some(self.st.turn.turn_number);
    }

    /// Who controls the top object of the stack: the human (`Some(true)`), someone else
    /// (`Some(false)`), or the stack is empty.
    fn top_is_mine(&self) -> Option<bool> {
        self.st
            .stack
            .last()
            .map(|s| Some(s.controller) == self.human)
    }

    /// How the session ranks a way to pay mana for the player (lower first), or `None`
    /// if it must not choose it for them: a plain tap first; then mana that costs mana (a
    /// Signet) or a token that sacrifices itself (Treasure); never life, damage to the
    /// player, a chosen sacrifice, or a nontoken permanent sacrificing itself (D-066).
    /// Which floating mana pays a mana ability's own cost is chosen for them.
    fn auto_pay_price(&self, o: &OptionDesc) -> Option<u32> {
        match o {
            OptionDesc::Mana { .. } => Some(0),
            OptionDesc::TapForMana { ability: None, .. } => Some(0),
            OptionDesc::TapForMana {
                source,
                ability: Some(a),
                ..
            } => match self.st.catalog.ability(*a) {
                card_model::AbilityDef::Mana { cost, then, .. } => {
                    let token = self.st.objects.get(source).is_some_and(|x| x.is_token);
                    if cost.life > 0
                        || then.is_some()
                        || cost.sacrifice.is_some()
                        || (cost.sacrifice_self && !token)
                    {
                        None
                    } else {
                        Some(u32::from(cost.mana.is_some()) + 2 * u32::from(cost.sacrifice_self))
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// The response the session gives on the human's behalf, if the decision is trivial
    /// under the current stops.
    pub fn auto_response(&self, d: &PendingDecision) -> Option<Response> {
        let n = d.options.len();
        // Passing the rest of the turn includes not attacking. (Blocks are always asked.)
        let turn = Some(self.st.turn.turn_number);
        if d.kind == ChoiceKind::DeclareAttackers
            && (self.pass_turn == turn || self.yield_turn == turn)
            && matches!(d.prim, Primitive::PickN { min: 0, .. })
        {
            return Some(Response::Pick(Vec::new()));
        }
        match (&d.prim, d.kind) {
            (Primitive::PickNumber { min, max }, _) if min == max => {
                return Some(Response::Number(*min));
            }
            (Primitive::PickN { min, max }, kind) => {
                // Mana is paid for the player only in the cheapest way there is (D-054,
                // D-066): never with life, damage to themselves, or by sacrificing a
                // permanent that isn't a token — even when that is the only way left.
                if kind == ChoiceKind::PayMana {
                    if !self.stops.auto_pay {
                        // The only way left is taken, unless it costs more than mana.
                        let only =
                            n == 1 && *min == 1 && self.auto_pay_price(&d.options[0]).is_some();
                        return only.then(|| Response::Pick(vec![0]));
                    }
                    return d
                        .options
                        .iter()
                        .enumerate()
                        .filter_map(|(i, o)| self.auto_pay_price(o).map(|p| (p, i)))
                        .min()
                        .map(|(_, i)| Response::Pick(vec![i]));
                }
                if *min == n && *max == n {
                    return Some(Response::Pick((0..n).collect()));
                }
                if n == 1 && *min == 1 {
                    return Some(Response::Pick(vec![0]));
                }
            }
            (Primitive::Allocate { total, per_max, .. }, ChoiceKind::PayMana)
                if self.stops.auto_pay =>
            {
                // Spend the most plentiful floating mana on the generic part.
                let mut alloc = vec![0u32; per_max.len()];
                let mut left = *total;
                let mut order: Vec<usize> = (0..per_max.len()).collect();
                order.sort_by_key(|&i| std::cmp::Reverse(per_max[i]));
                for i in order {
                    let add = left.min(per_max[i]);
                    alloc[i] = add;
                    left -= add;
                }
                return Some(Response::Allocate(alloc));
            }
            _ => {}
        }
        if d.kind != ChoiceKind::Priority {
            return None;
        }
        let pass = d.options.iter().position(|o| *o == OptionDesc::Pass)?;
        let pass = Some(Response::Pick(vec![pass]));
        let only_pass_or_mana = d
            .options
            .iter()
            .all(|o| matches!(o, OptionDesc::Pass | OptionDesc::TapForMana { .. }));
        if only_pass_or_mana || self.pass_turn == Some(self.st.turn.turn_number) {
            return pass;
        }
        let top = self.top_is_mine();
        // Yielding the turn: everything but an opponent's spell or ability.
        if self.yield_turn == Some(self.st.turn.turn_number) && top != Some(false) {
            return pass;
        }
        // Their own spell or ability is on top: let it resolve (unless holding priority).
        if top == Some(true) && !self.stops.hold_priority {
            return pass;
        }
        if self.st.stack.is_empty() {
            let mine = Some(self.st.turn.active) == self.human;
            let stops = if mine {
                &self.stops.mine
            } else {
                &self.stops.theirs
            };
            if !stops.contains(&self.st.turn.step) {
                return pass;
            }
        }
        None
    }

    /// Run AI decisions and automatic human responses until the human must decide, the
    /// game ends, or `budget` decisions have been made. Returns how many were made.
    pub fn pump(&mut self, budget: usize) -> usize {
        self.pump_inner(budget, false).0
    }

    /// Like [`Session::pump`], but also stops as soon as an AI seat has done something the
    /// log shows (played a land, cast a spell, attacked…), so the UI can let the human see
    /// it before the game moves on. Returns true if it stopped for that reason.
    pub fn pump_visible(&mut self, budget: usize) -> bool {
        self.pump_inner(budget, true).1
    }

    fn pump_inner(&mut self, budget: usize, stop_when_visible: bool) -> (usize, bool) {
        let mut done = 0;
        while done < budget {
            if self
                .pass_turn
                .is_some_and(|t| t != self.st.turn.turn_number)
            {
                self.pass_turn = None;
            }
            // A yield ends with the turn, or when an opponent acts.
            if self
                .yield_turn
                .is_some_and(|t| t != self.st.turn.turn_number || self.top_is_mine() == Some(false))
            {
                self.yield_turn = None;
            }
            let Some(d) = self.st.pending.clone() else {
                break;
            };
            let r = if let Some(q) = &mut self.replay_queue {
                match q.pop_front() {
                    Some(r) => r,
                    None => break,
                }
            } else if Some(d.player) == self.human {
                match self.auto_response(&d) {
                    Some(r) => r,
                    None => break,
                }
            } else {
                let view = player_view(&self.st, d.player);
                self.agents[d.player.index()].decide(&view, &d)
            };
            let by_ai = self.replay_queue.is_none() && Some(d.player) != self.human;
            let logged = self.log.len();
            if self.apply(r).is_err() {
                self.log
                    .push("(a recorded response no longer applies; stopping)".into());
                self.replay_queue = None;
                break;
            }
            done += 1;
            if stop_when_visible && by_ai && self.log.len() > logged {
                return (done, true);
            }
        }
        (done, false)
    }

    /// Everything needed to rebuild this game (spec §10.3).
    pub fn save(&self) -> Replay {
        Replay {
            cr_version: engine_core::CR_VERSION.into(),
            setup: self.setup,
            seed: self.seed,
            responses: self.responses.clone(),
        }
    }

    /// Rebuild a saved game and continue it. The AI agents are asked every recorded AI
    /// decision again so their internal state matches; the recorded response is what's
    /// applied.
    pub fn load(replay: &Replay, human: Option<PlayerId>, opponents: Agents) -> Self {
        let mut s = Session::new(replay.setup, replay.seed, human, opponents);
        for r in &replay.responses {
            let Some(d) = s.st.pending.clone() else { break };
            if Some(d.player) != human {
                let view = player_view(&s.st, d.player);
                let _ = s.agents[d.player.index()].decide(&view, &d);
            }
            if s.apply(r.clone()).is_err() {
                s.log
                    .push("(the save file doesn't match this version of the engine)".into());
                break;
            }
        }
        s
    }

    /// View a saved game from the start, one recorded response at a time.
    pub fn replay(replay: &Replay) -> Self {
        let mut s = Session::new(replay.setup, replay.seed, None, Agents::Random);
        s.replay_queue = Some(replay.responses.iter().cloned().collect());
        s
    }
}
