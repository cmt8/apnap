//! Declaring attackers and blockers on the board instead of through a list.
//!
//! The engine asks about one creature at a time. The human plans the whole declaration here
//! — click a creature to send it in, click a blocker and then the attacker it blocks, or drag
//! one onto the other — and confirms once; the session then answers the engine creature by
//! creature (`Session::declare`). Nothing in a plan is binding until it is confirmed, and a
//! plan can only hold choices the engine offered.

use crate::session::{Candidate, Declaration};
use engine_core::decision::{ChoiceKind, OptionDesc};
use engine_core::{ObjectId, PlayerId};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The engine's decision counter when this declaration began (a new count means a new
    /// declaration, or that this one was sent back as illegal).
    pub decision_id: u64,
    pub kind: ChoiceKind,
    pub candidates: Vec<Candidate>,
    /// What each attacking or blocking creature does. Creatures not in here do nothing.
    pub choices: BTreeMap<ObjectId, OptionDesc>,
    /// Attacks: the player a creature is sent at when it is clicked.
    pub target: Option<PlayerId>,
    /// Attacks: the planeswalker a creature is sent at when it is clicked, rather than
    /// `target` (CR 506.3: chosen by clicking the planeswalker).
    pub target_planeswalker: Option<ObjectId>,
    /// Blocks: the blocker that was clicked and is waiting for its attacker.
    pub selected: Option<ObjectId>,
}

impl Plan {
    pub fn new(decision_id: u64, declaration: Declaration) -> Plan {
        let mut plan = Plan {
            decision_id,
            kind: declaration.kind,
            candidates: declaration.candidates,
            choices: BTreeMap::new(),
            target: None,
            target_planeswalker: None,
            selected: None,
        };
        plan.target = plan.defenders().first().copied();
        plan
    }

    pub fn attacking(&self) -> bool {
        self.kind == ChoiceKind::DeclareAttackers
    }

    pub fn candidate(&self, creature: ObjectId) -> Option<&Candidate> {
        self.candidates.iter().find(|c| c.creature == creature)
    }

    /// Every player that can be attacked, in the order the engine lists them.
    pub fn defenders(&self) -> Vec<PlayerId> {
        let mut out = Vec::new();
        for o in self.candidates.iter().flat_map(|c| &c.options) {
            if let OptionDesc::Attack { defender, .. } = o
                && !out.contains(defender)
            {
                out.push(*defender);
            }
        }
        out
    }

    /// Every planeswalker that can be attacked, in the order the engine lists them.
    pub fn planeswalkers(&self) -> Vec<ObjectId> {
        let mut out = Vec::new();
        for o in self.candidates.iter().flat_map(|c| &c.options) {
            if let OptionDesc::AttackPlaneswalker { planeswalker, .. } = o
                && !out.contains(planeswalker)
            {
                out.push(*planeswalker);
            }
        }
        out
    }

    fn attack_option(&self, creature: ObjectId, defender: PlayerId) -> Option<OptionDesc> {
        self.candidate(creature)?
            .options
            .iter()
            .find(|o| matches!(o, OptionDesc::Attack { defender: d, .. } if *d == defender))
            .cloned()
    }

    fn planeswalker_option(&self, creature: ObjectId, pw: ObjectId) -> Option<OptionDesc> {
        self.candidate(creature)?
            .options
            .iter()
            .find(|o| matches!(o, OptionDesc::AttackPlaneswalker { planeswalker, .. } if *planeswalker == pw))
            .cloned()
    }

    /// What a clicked creature attacks: the chosen planeswalker, else the chosen player.
    fn aimed_option(&self, creature: ObjectId) -> Option<OptionDesc> {
        match self.target_planeswalker {
            Some(pw) => self.planeswalker_option(creature, pw),
            None => self.target.and_then(|t| self.attack_option(creature, t)),
        }
    }

    fn block_option(&self, blocker: ObjectId, attacker: ObjectId) -> Option<OptionDesc> {
        self.candidate(blocker)?
            .options
            .iter()
            .find(|o| matches!(o, OptionDesc::Block { attacker: a, .. } if *a == attacker))
            .cloned()
    }

    /// Can `blocker` block `attacker`?
    pub fn can_block(&self, blocker: ObjectId, attacker: ObjectId) -> bool {
        self.block_option(blocker, attacker).is_some()
    }

    /// The player `creature` is planned to attack.
    pub fn attacks(&self, creature: ObjectId) -> Option<PlayerId> {
        match self.choices.get(&creature) {
            Some(OptionDesc::Attack { defender, .. }) => Some(*defender),
            _ => None,
        }
    }

    /// The planeswalker `creature` is planned to attack.
    pub fn attacks_planeswalker(&self, creature: ObjectId) -> Option<ObjectId> {
        match self.choices.get(&creature) {
            Some(OptionDesc::AttackPlaneswalker { planeswalker, .. }) => Some(*planeswalker),
            _ => None,
        }
    }

    /// The attacker `creature` is planned to block.
    pub fn blocks(&self, creature: ObjectId) -> Option<ObjectId> {
        match self.choices.get(&creature) {
            Some(OptionDesc::Block { attacker, .. }) => Some(*attacker),
            _ => None,
        }
    }

    /// A click on one of the creatures being asked about.
    pub fn click_creature(&mut self, creature: ObjectId) {
        let Some(c) = self.candidate(creature).cloned() else {
            return;
        };
        if self.attacking() {
            // Toggle: attack the chosen player or planeswalker, or stand down.
            let wanted = self
                .aimed_option(creature)
                .or_else(|| c.options.first().cloned());
            match wanted {
                Some(o) if self.choices.get(&creature) != Some(&o) => {
                    self.choices.insert(creature, o);
                }
                _ => {
                    self.choices.remove(&creature);
                }
            }
        } else if self.choices.remove(&creature).is_some() {
            self.selected = None;
        } else if let [only] = c.options.as_slice() {
            // One attacker it could block: no need to ask which.
            self.choices.insert(creature, only.clone());
            self.selected = None;
        } else {
            self.selected = (self.selected != Some(creature)).then_some(creature);
        }
    }

    /// A click on an attacking creature while a blocker is waiting for one.
    pub fn click_attacker(&mut self, attacker: ObjectId) {
        if let Some(blocker) = self.selected
            && let Some(o) = self.block_option(blocker, attacker)
        {
            self.choices.insert(blocker, o);
            self.selected = None;
        }
    }

    /// A click on a player: new attackers go at them from now on.
    pub fn click_player(&mut self, player: PlayerId) {
        if self.attacking() && self.defenders().contains(&player) {
            self.target = Some(player);
            self.target_planeswalker = None;
        }
    }

    /// A click on a planeswalker that can be attacked: new attackers go at it from now on
    /// (clicking it again goes back to the player). Returns whether it was one.
    pub fn click_planeswalker(&mut self, pw: ObjectId) -> bool {
        if !self.attacking() || !self.planeswalkers().contains(&pw) {
            return false;
        }
        self.target_planeswalker = (self.target_planeswalker != Some(pw)).then_some(pw);
        true
    }

    /// `creature` was dragged onto the planeswalker `pw`: attack it. Returns whether that is
    /// possible.
    pub fn drop_on_planeswalker(&mut self, creature: ObjectId, pw: ObjectId) -> bool {
        match self.planeswalker_option(creature, pw) {
            Some(o) if self.attacking() => {
                self.choices.insert(creature, o);
                true
            }
            _ => false,
        }
    }

    /// `creature` was dragged onto `player`: attack them. Returns whether that is possible.
    pub fn drop_on_player(&mut self, creature: ObjectId, player: PlayerId) -> bool {
        match self.attack_option(creature, player) {
            Some(o) if self.attacking() => {
                self.choices.insert(creature, o);
                true
            }
            _ => false,
        }
    }

    /// `creature` was dragged onto `other`: block it. Returns whether that is possible.
    pub fn drop_on_creature(&mut self, creature: ObjectId, other: ObjectId) -> bool {
        match self.block_option(creature, other) {
            Some(o) if !self.attacking() => {
                self.choices.insert(creature, o);
                self.selected = None;
                true
            }
            _ => false,
        }
    }

    /// Every creature that can attacks the chosen player (or whoever it can attack).
    pub fn all_attack(&mut self) {
        if !self.attacking() {
            return;
        }
        for c in self.candidates.clone() {
            let o = self
                .aimed_option(c.creature)
                .or_else(|| c.options.first().cloned());
            if let Some(o) = o {
                self.choices.insert(c.creature, o);
            }
        }
    }

    pub fn clear(&mut self) {
        self.choices.clear();
        self.selected = None;
    }

    /// The label of the button that confirms the plan.
    pub fn confirm_label(&self) -> String {
        match (self.attacking(), self.choices.len()) {
            (true, 0) => "No attacks".into(),
            (true, n) => format!("Attack with {n}"),
            (false, 0) => "No blocks".into(),
            (false, n) => format!("Block with {n}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P1: PlayerId = PlayerId(1);
    const P2: PlayerId = PlayerId(2);

    fn attack_plan() -> Plan {
        let cand = |n: u32| Candidate {
            creature: ObjectId(n),
            options: [P1, P2]
                .into_iter()
                .map(|defender| OptionDesc::Attack {
                    attacker: ObjectId(n),
                    defender,
                })
                .collect(),
        };
        Plan::new(
            7,
            Declaration {
                kind: ChoiceKind::DeclareAttackers,
                candidates: vec![cand(10), cand(11), cand(12)],
            },
        )
    }

    /// Blockers 20 and 21 can each block attackers 30 and 31; blocker 22 only 30.
    fn block_plan() -> Plan {
        let cand = |b: u32, attackers: &[u32]| Candidate {
            creature: ObjectId(b),
            options: attackers
                .iter()
                .map(|a| OptionDesc::Block {
                    blocker: ObjectId(b),
                    attacker: ObjectId(*a),
                })
                .collect(),
        };
        Plan::new(
            9,
            Declaration {
                kind: ChoiceKind::DeclareBlockers,
                candidates: vec![cand(20, &[30, 31]), cand(21, &[30, 31]), cand(22, &[30])],
            },
        )
    }

    #[test]
    fn clicking_creatures_toggles_attackers_at_the_chosen_player() {
        let mut p = attack_plan();
        assert_eq!(p.defenders(), [P1, P2]);
        assert_eq!(p.target, Some(P1));
        assert_eq!(p.confirm_label(), "No attacks");
        p.click_creature(ObjectId(10));
        assert_eq!(p.attacks(ObjectId(10)), Some(P1));
        // Choose another player: the next creature goes there, and clicking the first one
        // again re-aims it rather than standing it down.
        p.click_player(P2);
        p.click_creature(ObjectId(11));
        p.click_creature(ObjectId(10));
        assert_eq!(p.attacks(ObjectId(10)), Some(P2));
        assert_eq!(p.attacks(ObjectId(11)), Some(P2));
        assert_eq!(p.confirm_label(), "Attack with 2");
        // Clicking an attacker already aimed at the chosen player stands it down.
        p.click_creature(ObjectId(11));
        assert_eq!(p.attacks(ObjectId(11)), None);
        // Not one of the creatures asked about: nothing happens.
        p.click_creature(ObjectId(99));
        p.click_player(PlayerId(0));
        assert_eq!((p.choices.len(), p.target), (1, Some(P2)));
    }

    /// Attackers 10 and 11 may attack P1 or P1's planeswalker 40.
    fn planeswalker_plan() -> Plan {
        let cand = |n: u32| Candidate {
            creature: ObjectId(n),
            options: vec![
                OptionDesc::Attack {
                    attacker: ObjectId(n),
                    defender: P1,
                },
                OptionDesc::AttackPlaneswalker {
                    attacker: ObjectId(n),
                    planeswalker: ObjectId(40),
                },
            ],
        };
        Plan::new(
            8,
            Declaration {
                kind: ChoiceKind::DeclareAttackers,
                candidates: vec![cand(10), cand(11)],
            },
        )
    }

    // CR 506.3, 508.1b: a planeswalker is clicked (or a creature dropped onto it) to attack it.
    #[test]
    fn attacking_a_planeswalker_on_the_table() {
        let mut p = planeswalker_plan();
        assert_eq!(p.planeswalkers(), [ObjectId(40)]);
        p.click_creature(ObjectId(10));
        assert_eq!(p.attacks(ObjectId(10)), Some(P1));
        assert!(p.click_planeswalker(ObjectId(40)));
        p.click_creature(ObjectId(11));
        assert_eq!(p.attacks_planeswalker(ObjectId(11)), Some(ObjectId(40)));
        assert_eq!(p.attacks(ObjectId(11)), None);
        // Clicking the player goes back to attacking the player.
        p.click_player(P1);
        p.click_creature(ObjectId(11));
        assert_eq!(p.attacks(ObjectId(11)), Some(P1));
        assert!(p.drop_on_planeswalker(ObjectId(10), ObjectId(40)));
        assert_eq!(p.attacks_planeswalker(ObjectId(10)), Some(ObjectId(40)));
        assert!(!p.click_planeswalker(ObjectId(41)), "not attackable");
        assert!(!p.drop_on_planeswalker(ObjectId(10), ObjectId(41)));
    }

    #[test]
    fn all_attack_and_clear() {
        let mut p = attack_plan();
        p.click_player(P2);
        p.all_attack();
        assert_eq!(p.choices.len(), 3);
        assert!(
            p.candidates
                .iter()
                .all(|c| p.attacks(c.creature) == Some(P2))
        );
        assert_eq!(p.confirm_label(), "Attack with 3");
        p.clear();
        assert_eq!(p.confirm_label(), "No attacks");
    }

    #[test]
    fn dragging_a_creature_onto_a_player_attacks_that_player() {
        let mut p = attack_plan();
        assert!(p.drop_on_player(ObjectId(12), P2));
        assert_eq!(p.attacks(ObjectId(12)), Some(P2));
        assert!(
            !p.drop_on_player(ObjectId(12), PlayerId(0)),
            "not attackable"
        );
        assert!(!p.drop_on_player(ObjectId(99), P1), "not an attacker");
        assert!(
            !p.drop_on_creature(ObjectId(12), ObjectId(10)),
            "not blocking"
        );
        assert_eq!(p.choices.len(), 1);
    }

    #[test]
    fn a_blocker_is_clicked_and_then_the_attacker_it_blocks() {
        let mut p = block_plan();
        assert_eq!(p.confirm_label(), "No blocks");
        p.click_creature(ObjectId(20));
        assert_eq!(p.selected, Some(ObjectId(20)));
        assert!(p.can_block(ObjectId(20), ObjectId(31)));
        p.click_attacker(ObjectId(31));
        assert_eq!(
            (p.blocks(ObjectId(20)), p.selected),
            (Some(ObjectId(31)), None)
        );
        // Only one attacker it could block: one click is enough.
        p.click_creature(ObjectId(22));
        assert_eq!(p.blocks(ObjectId(22)), Some(ObjectId(30)));
        assert_eq!(p.confirm_label(), "Block with 2");
        // Clicking a blocker again takes it out of the block.
        p.click_creature(ObjectId(20));
        assert_eq!(p.blocks(ObjectId(20)), None);
        // An attacker clicked with no blocker waiting, or one it can't block: nothing.
        p.click_attacker(ObjectId(30));
        p.click_creature(ObjectId(22)); // unblock
        p.click_creature(ObjectId(21));
        p.click_attacker(ObjectId(99));
        assert!(p.choices.is_empty());
        assert_eq!(p.selected, Some(ObjectId(21)));
        // Clicking the waiting blocker again deselects it.
        p.click_creature(ObjectId(21));
        assert_eq!(p.selected, None);
    }

    #[test]
    fn dragging_a_blocker_onto_an_attacker_blocks_it() {
        let mut p = block_plan();
        assert!(p.drop_on_creature(ObjectId(21), ObjectId(30)));
        assert_eq!(p.blocks(ObjectId(21)), Some(ObjectId(30)));
        assert!(
            !p.drop_on_creature(ObjectId(22), ObjectId(31)),
            "can't block that one"
        );
        assert!(!p.drop_on_player(ObjectId(21), P1));
        // Two creatures may block the same attacker.
        assert!(p.drop_on_creature(ObjectId(20), ObjectId(30)));
        assert_eq!(p.confirm_label(), "Block with 2");
    }
}
