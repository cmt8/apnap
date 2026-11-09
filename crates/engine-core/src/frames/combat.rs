//! Combat (CR 506–511 (2026-11-06)). Declarations are decomposed into one decision per
//! creature (spec §8.5) so option lists never enumerate whole attack configurations.

use super::{ApplyFrame, Flow, Frame, Input};
use crate::actions::{GameAction, execute};
use crate::decision::{ChoiceKind, OptionDesc, Primitive};
use crate::events::{DamageTarget, Event};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::replacement::Proposed;
use crate::state::{AttackInfo, CombatState, GameState};
use alloc::vec;
use alloc::vec::Vec;
use card_model::{CardType, Keyword};
use serde::{Deserialize, Serialize};

fn creatures_controlled_by(st: &GameState, p: PlayerId) -> Vec<ObjectId> {
    st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| st.object(id).controller == p && chars(st, id).is(CardType::Creature))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclareAttackersFrame {
    pub candidates: Vec<ObjectId>,
    pub next: usize,
    pub chosen: Vec<(ObjectId, PlayerId)>,
}

impl DeclareAttackersFrame {
    pub(crate) fn new(st: &GameState) -> Self {
        // CR 508.1a: untapped, and either haste or controlled continuously since the turn
        // began (CR 302.6, 702.10b).
        let candidates = creatures_controlled_by(st, st.turn.active)
            .into_iter()
            .filter(|&id| {
                let o = st.object(id);
                !o.tapped && (!o.summoning_sick || chars(st, id).has(card_model::Keyword::Haste))
            })
            .collect();
        DeclareAttackersFrame {
            candidates,
            next: 0,
            chosen: Vec::new(),
        }
    }

    pub(crate) fn run(mut self, st: &mut GameState, input: Option<Input>) -> Flow {
        let ap = st.turn.active;
        if let Some(inp) = input {
            if let Some(OptionDesc::Attack { attacker, defender }) = inp.picked_one().cloned() {
                self.chosen.push((attacker, defender));
            }
            self.next += 1;
        }
        if let Some(&creature) = self.candidates.get(self.next) {
            // CR 508.1b: which player each attacker attacks (planeswalkers/battles: M3).
            let options = st
                .opponents(ap)
                .into_iter()
                .map(|defender| OptionDesc::Attack {
                    attacker: creature,
                    defender,
                })
                .collect();
            return Flow::ask(
                Frame::DeclareAttackers(self),
                ap,
                ChoiceKind::DeclareAttackers,
                Primitive::PickN { min: 0, max: 1 },
                options,
            );
        }
        // CR 508.1c–e: no restrictions/requirements/banding in M0. CR 508.1f: tap attackers.
        for &(attacker, _) in &self.chosen {
            execute(st, GameAction::Tap { object: attacker });
        }
        // CR 508.1k: they become attacking creatures.
        let mut combat = CombatState::default();
        for &(attacker, defender) in &self.chosen {
            combat.attackers.push(AttackInfo {
                attacker,
                defender,
                blocked: false,
                blockers: Vec::new(),
            });
            st.emit(Event::AttackerDeclared { attacker, defender });
        }
        st.combat = Some(combat);
        Flow::done()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclareBlockersFrame {
    /// Defending players in APNAP order.
    pub defenders: Vec<PlayerId>,
    pub defender_idx: usize,
    /// Potential blockers of the current defender.
    pub candidates: Vec<ObjectId>,
    pub next: usize,
    pub chosen: Vec<(ObjectId, ObjectId)>,
}

impl DeclareBlockersFrame {
    pub(crate) fn new(st: &GameState) -> Self {
        let attacked: Vec<PlayerId> = st
            .combat
            .as_ref()
            .map(|c| c.attackers.iter().map(|a| a.defender).collect())
            .unwrap_or_default();
        // CR 802.4: each defending player, in APNAP order, declares all their blocks.
        let defenders = st
            .apnap_order()
            .into_iter()
            .filter(|p| attacked.contains(p))
            .collect();
        let mut f = DeclareBlockersFrame {
            defenders,
            defender_idx: 0,
            candidates: Vec::new(),
            next: 0,
            chosen: Vec::new(),
        };
        f.start_defender(st);
        f
    }

    pub(crate) fn run(mut self, st: &mut GameState, input: Option<Input>) -> Flow {
        if let Some(inp) = input {
            if let Some(OptionDesc::Block { blocker, attacker }) = inp.picked_one().cloned() {
                self.chosen.push((blocker, attacker));
            }
            self.next += 1;
        }
        while let Some(&defender) = self.defenders.get(self.defender_idx) {
            if let Some(&blocker) = self.candidates.get(self.next) {
                // CR 509.1a: block one creature attacking that player. CR 509.1b / 702.9b:
                // a creature with flying can't be blocked except by flying and/or reach.
                let bc = chars(st, blocker);
                let can_block_flyer = bc.has(Keyword::Flying) || bc.has(Keyword::Reach);
                let options: Vec<OptionDesc> = st
                    .combat
                    .as_ref()
                    .expect("combat")
                    .attackers
                    .iter()
                    .filter(|a| a.defender == defender)
                    .filter(|a| can_block_flyer || !chars(st, a.attacker).has(Keyword::Flying))
                    .map(|a| OptionDesc::Block {
                        blocker,
                        attacker: a.attacker,
                    })
                    .collect();
                if options.is_empty() {
                    self.next += 1;
                    continue;
                }
                return Flow::ask(
                    Frame::DeclareBlockers(self),
                    defender,
                    ChoiceKind::DeclareBlockers,
                    Primitive::PickN { min: 0, max: 1 },
                    options,
                );
            }
            self.defender_idx += 1;
            self.start_defender(st);
        }
        // CR 509.1b–f: no restrictions/requirements/costs in M0. CR 509.1g–h.
        let combat = st.combat.as_mut().expect("combat");
        for &(blocker, attacker) in &self.chosen {
            let a = combat
                .attackers
                .iter_mut()
                .find(|a| a.attacker == attacker)
                .expect("blocked attacker in combat");
            a.blocked = true;
            a.blockers.push(blocker);
        }
        for &(blocker, attacker) in &self.chosen {
            st.emit(Event::BlockerDeclared { blocker, attacker });
        }
        Flow::done()
    }

    fn start_defender(&mut self, st: &GameState) {
        self.next = 0;
        self.candidates = match self.defenders.get(self.defender_idx) {
            Some(&d) => creatures_controlled_by(st, d)
                .into_iter()
                .filter(|&id| !st.object(id).tapped)
                .collect(),
            None => Vec::new(),
        };
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatDamageFrame {
    /// Index into `combat.attackers` of the next attacker to assign for.
    pub next: usize,
    pub assignments: Vec<(ObjectId, DamageTarget, u32)>,
}

impl CombatDamageFrame {
    pub(crate) fn run(mut self, st: &mut GameState, input: Option<Input>) -> Flow {
        let ap = st.turn.active;
        let attackers = st
            .combat
            .as_ref()
            .map(|c| c.attackers.clone())
            .unwrap_or_default();
        if let Some(inp) = input {
            // CR 510.1c: damage divided among blockers as the attacker's controller chooses.
            let a = &attackers[self.next];
            for (i, &amount) in inp.allocation().iter().enumerate() {
                let OptionDesc::DamageRecipient { object } = inp.decision.options[i] else {
                    unreachable!()
                };
                if amount > 0 {
                    self.assignments
                        .push((a.attacker, DamageTarget::Object(object), amount));
                }
            }
            self.next += 1;
        }
        // CR 510.1: first the active player assigns for each attacking creature…
        while let Some(a) = attackers.get(self.next) {
            let power = chars(st, a.attacker).power.unwrap_or(0);
            if power <= 0 {
                // CR 510.1a
                self.next += 1;
                continue;
            }
            let power = power as u32;
            if !a.blocked {
                // CR 510.1b
                if st.in_game(a.defender) {
                    self.assignments
                        .push((a.attacker, DamageTarget::Player(a.defender), power));
                }
            } else {
                match a.blockers.as_slice() {
                    [] => {}
                    [b] => self
                        .assignments
                        .push((a.attacker, DamageTarget::Object(*b), power)),
                    many => {
                        let n = many.len();
                        let options = many
                            .iter()
                            .map(|&object| OptionDesc::DamageRecipient { object })
                            .collect();
                        let ask = Flow::ask(
                            Frame::CombatDamage(self),
                            ap,
                            ChoiceKind::AssignCombatDamage,
                            Primitive::Allocate {
                                total: power,
                                per_min: vec![0; n],
                                per_max: vec![power; n],
                            },
                            options,
                        );
                        return ask;
                    }
                }
            }
            self.next += 1;
        }
        // …then each defending player assigns for each blocking creature (CR 510.1d). In M0
        // a blocker blocks exactly one attacker, so no choice is needed.
        for a in &attackers {
            for &b in &a.blockers {
                let power = chars(st, b).power.unwrap_or(0);
                if power > 0 {
                    self.assignments
                        .push((b, DamageTarget::Object(a.attacker), power as u32));
                }
            }
        }
        // CR 510.2: all assigned combat damage is dealt simultaneously (as one batch through
        // the prevention/replacement pipeline).
        let events: Vec<Proposed> = core::mem::take(&mut self.assignments)
            .into_iter()
            .map(|(source, target, amount)| Proposed::Damage {
                source,
                deathtouch: chars(st, source).has(Keyword::Deathtouch),
                target,
                amount,
                combat: true,
            })
            .collect();
        if events.is_empty() {
            return Flow::done();
        }
        Flow::then(Frame::Apply(ApplyFrame::new(st, events)))
    }
}
