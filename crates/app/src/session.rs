//! A game session: the engine state, the AI seats, the human's stops, and the response log.
//! No graphics here, so everything is testable headlessly; `ui.rs` only draws it.
//!
//! The session owns `GameState`. AI agents are handed only their `PlayerView` (CLAUDE.md
//! invariant 6). "Stops" are a UI concern (spec §6): the engine always asks, and the session
//! answers trivial priority decisions for the human according to the configured stops.

use engine_core::decision::{ChoiceKind, OptionDesc, PendingDecision, Primitive, Response};
use engine_core::turn::Step;
use engine_core::{Agent, GameState, IllegalResponse, PlayerId, PlayerView, player_view};
use harness::text::Narrator;
use harness::{Agents, Replay, Setup};
use std::collections::BTreeSet;

/// Steps at which the human is offered priority even when the stack is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stops {
    pub mine: BTreeSet<Step>,
    pub theirs: BTreeSet<Step>,
    /// Tap lands automatically when paying costs (same responses a player could give).
    pub auto_pay: bool,
}

impl Default for Stops {
    fn default() -> Self {
        Stops {
            mine: [Step::PrecombatMain, Step::PostcombatMain]
                .into_iter()
                .collect(),
            theirs: BTreeSet::new(),
            auto_pay: true,
        }
    }
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
    /// Pass priority automatically until this turn is over.
    pub pass_turn: Option<u32>,
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
            .map(|s| opponents.make(seed, s))
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
            .map(|s| opponents.make(st.seed, s))
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

    /// Start passing priority automatically for the rest of this turn.
    pub fn pass_the_turn(&mut self) {
        self.pass_turn = Some(self.st.turn.turn_number);
    }

    /// The response the session gives on the human's behalf, if the decision is trivial
    /// under the current stops.
    pub fn auto_response(&self, d: &PendingDecision) -> Option<Response> {
        let n = d.options.len();
        match (&d.prim, d.kind) {
            (Primitive::PickNumber { min, max }, _) if min == max => {
                return Some(Response::Number(*min));
            }
            (Primitive::PickN { min, max }, kind) => {
                if *min == n && *max == n {
                    return Some(Response::Pick((0..n).collect()));
                }
                if n == 1 && *min == 1 {
                    return Some(Response::Pick(vec![0]));
                }
                if kind == ChoiceKind::PayMana && self.stops.auto_pay {
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
        let mut done = 0;
        while done < budget {
            if self
                .pass_turn
                .is_some_and(|t| t != self.st.turn.turn_number)
            {
                self.pass_turn = None;
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
            if self.apply(r).is_err() {
                self.log
                    .push("(a recorded response no longer applies; stopping)".into());
                self.replay_queue = None;
                break;
            }
            done += 1;
        }
        done
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
