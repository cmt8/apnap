//! Combat (CR 506–511 (2026-11-06)). Declarations are decomposed into one decision per
//! creature (spec §8.5) so option lists never enumerate whole attack configurations.

use super::{ApplyFrame, Flow, Frame, Input};
use crate::actions::{GameAction, execute};
use crate::decision::{ChoiceKind, OptionDesc, Primitive, Response};
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
        .filter(|&id| {
            let c = chars(st, id);
            c.controller == p && c.is(CardType::Creature)
        })
        .collect()
}

/// CR 509.1b: may `blocker` block `attacker` (which attacks `defender`), as far as evasion
/// abilities go? (Menace counts blockers and is checked on the whole declaration.)
pub(crate) fn can_block(
    st: &GameState,
    blocker: ObjectId,
    attacker: ObjectId,
    defender: PlayerId,
) -> bool {
    let (a, b) = (chars(st, attacker), chars(st, blocker));
    let artifact = b.is(CardType::Artifact);
    // CR 702.9b: flying — only by flying and/or reach.
    if a.has(Keyword::Flying) && !(b.has(Keyword::Flying) || b.has(Keyword::Reach)) {
        return false;
    }
    // CR 702.36b: fear — only by artifact and/or black creatures.
    if a.has(Keyword::Fear) && !(artifact || b.colors.contains(&card_model::Color::Black)) {
        return false;
    }
    // CR 702.13b: intimidate — only by artifact creatures and/or creatures that share a
    // color with it.
    if a.has(Keyword::Intimidate) && !(artifact || b.colors.iter().any(|c| a.colors.contains(c))) {
        return false;
    }
    // CR 702.14c: landwalk — not at all while the defending player controls such a land.
    for k in &a.keywords {
        if let Keyword::Landwalk(t) = k {
            let has_land = st.zones.battlefield.iter().any(|&id| {
                let c = chars(st, id);
                c.controller == defender && c.is(CardType::Land) && c.land_types.contains(t)
            });
            if has_land {
                return false;
            }
        }
    }
    true
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
                let c = chars(st, id);
                // CR 702.3b: a creature with defender can't attack.
                !o.tapped
                    && (!o.summoning_sick || c.has(Keyword::Haste))
                    && !c.has(Keyword::Defender)
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
        // CR 508.1c–e: no restrictions/requirements/banding yet. CR 508.1f: tap attackers
        // (CR 702.20b: unless they have vigilance).
        for &(attacker, _) in &self.chosen {
            if !chars(st, attacker).has(Keyword::Vigilance) {
                execute(st, GameAction::Tap { object: attacker });
            }
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
        // "Attacking creatures you control get …": the layered view depends on combat.
        st.generation += 1;
        // CR 508.1m: abilities that trigger on attackers being declared trigger now.
        let attackers: Vec<ObjectId> = self.chosen.iter().map(|(a, _)| *a).collect();
        crate::triggers::detect_attacks(st, &attackers);
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
                let options: Vec<OptionDesc> = st
                    .combat
                    .as_ref()
                    .expect("combat")
                    .attackers
                    .iter()
                    .filter(|a| a.defender == defender)
                    .filter(|a| can_block(st, blocker, a.attacker, defender))
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
            // CR 509.1b / 702.111b: a creature with menace can't be blocked except by two or
            // more creatures. An illegal declaration returns to before it (CR 734): nothing
            // has happened yet, so this defender simply declares again.
            if !self.menace_ok(st, defender) {
                let mine: Vec<ObjectId> = self.candidates.clone();
                self.chosen.retain(|(b, _)| !mine.contains(b));
                self.next = 0;
                st.emit(Event::BlockDeclarationIllegal { player: defender });
                continue;
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
        st.generation += 1;
        for &(blocker, attacker) in &self.chosen {
            st.emit(Event::BlockerDeclared { blocker, attacker });
        }
        Flow::done()
    }

    fn menace_ok(&self, st: &GameState, defender: PlayerId) -> bool {
        let combat = st.combat.as_ref().expect("combat");
        combat
            .attackers
            .iter()
            .filter(|a| a.defender == defender && chars(st, a.attacker).has(Keyword::Menace))
            .all(|a| {
                let n = self
                    .chosen
                    .iter()
                    .filter(|(b, at)| *at == a.attacker && self.candidates.contains(b))
                    .count();
                n != 1
            })
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
    pub started: bool,
    /// CR 510.4: the creatures that assign combat damage in this step.
    pub assigners: Vec<ObjectId>,
    /// Index into `combat.attackers` of the next attacker to assign for.
    pub next: usize,
    /// Trample: the amount the current attacker assigns to the player (chosen first).
    pub to_player: Option<u32>,
    pub assignments: Vec<(ObjectId, DamageTarget, u32)>,
}

impl CombatDamageFrame {
    /// CR 510.4 / 702.7b: who assigns damage in this combat damage step.
    fn choose_assigners(st: &mut GameState) -> Vec<ObjectId> {
        let combat = st.combat.as_ref().expect("combat");
        let mut all: Vec<ObjectId> = combat.attackers.iter().map(|a| a.attacker).collect();
        for a in &combat.attackers {
            all.extend(a.blockers.iter().copied());
        }
        let strikes = |st: &GameState, id: ObjectId, k: Keyword| chars(st, id).has(k);
        if combat.damage_step == 0 {
            let first: Vec<ObjectId> = all
                .iter()
                .copied()
                .filter(|&c| {
                    strikes(st, c, Keyword::FirstStrike) || strikes(st, c, Keyword::DoubleStrike)
                })
                .collect();
            if first.is_empty() {
                return all;
            }
            let c = st.combat.as_mut().expect("combat");
            c.first_strikers = first.clone();
            c.extra_damage_step = true;
            first
        } else {
            // The second step: creatures that had neither first nor double strike as the first
            // step began, plus those that currently have double strike.
            let had_first = combat.first_strikers.clone();
            all.into_iter()
                .filter(|&c| !had_first.contains(&c) || strikes(st, c, Keyword::DoubleStrike))
                .collect()
        }
    }

    /// CR 702.19b / 702.2c: damage that is lethal to `blocker` now, counting damage already
    /// marked and damage assigned to it earlier in this step.
    fn lethal(&self, st: &GameState, blocker: ObjectId, deathtouch: bool) -> u32 {
        let c = chars(st, blocker);
        let already: u32 = self
            .assignments
            .iter()
            .filter(|(_, t, _)| *t == DamageTarget::Object(blocker))
            .map(|(_, _, n)| *n)
            .sum();
        let remaining =
            (c.toughness.unwrap_or(0) - st.object(blocker).damage_marked as i32 - already as i32)
                .max(0) as u32;
        if deathtouch {
            remaining.min(1)
        } else {
            remaining
        }
    }

    pub(crate) fn run(mut self, st: &mut GameState, input: Option<Input>) -> Flow {
        let ap = st.turn.active;
        if !self.started {
            self.started = true;
            self.assigners = Self::choose_assigners(st);
        }
        let attackers = st
            .combat
            .as_ref()
            .map(|c| c.attackers.clone())
            .unwrap_or_default();
        if let Some(inp) = input {
            match &inp.response {
                // Trample, part 1: how much goes to the player.
                Response::Number(n) => self.to_player = Some(*n),
                // CR 510.1c: damage divided among blockers as the attacker's controller chooses.
                Response::Allocate(v) => {
                    let a = &attackers[self.next];
                    for (i, &amount) in v.iter().enumerate() {
                        let OptionDesc::DamageRecipient { object } = inp.decision.options[i] else {
                            unreachable!()
                        };
                        if amount > 0 {
                            self.assignments.push((
                                a.attacker,
                                DamageTarget::Object(object),
                                amount,
                            ));
                        }
                    }
                    self.finish_attacker(&attackers);
                }
                r => unreachable!("{r:?}"),
            }
        }
        // CR 510.1: first the active player assigns for each attacking creature…
        while let Some(a) = attackers.get(self.next) {
            let c = chars(st, a.attacker);
            let power = c.power.unwrap_or(0);
            if power <= 0 || !self.assigners.contains(&a.attacker) {
                // CR 510.1a / 510.4
                self.next += 1;
                continue;
            }
            let power = power as u32;
            let trample = c.has(Keyword::Trample);
            let deathtouch = c.has(Keyword::Deathtouch);
            let player = st
                .in_game(a.defender)
                .then_some(DamageTarget::Player(a.defender));
            if !a.blocked || (a.blockers.is_empty() && trample) {
                // CR 510.1b, 702.19d
                if let Some(t) = player {
                    self.assignments.push((a.attacker, t, power));
                }
                self.next += 1;
                continue;
            }
            if a.blockers.is_empty() {
                self.next += 1;
                continue;
            }
            let lethal: Vec<u32> = a
                .blockers
                .iter()
                .map(|&b| self.lethal(st, b, deathtouch))
                .collect();
            let to_player = if trample {
                // CR 702.19b: lethal to every blocker first; the excess may go to the player.
                let max = power.saturating_sub(lethal.iter().sum());
                match self.to_player {
                    Some(n) => n,
                    None if max > 0 && player.is_some() => {
                        return Flow::ask(
                            Frame::CombatDamage(self),
                            ap,
                            ChoiceKind::AssignCombatDamage,
                            Primitive::PickNumber { min: 0, max },
                            Vec::new(),
                        );
                    }
                    None => 0,
                }
            } else {
                0
            };
            if to_player > 0
                && let Some(t) = player
            {
                self.assignments.push((a.attacker, t, to_player));
            }
            let rest = power - to_player;
            match a.blockers.as_slice() {
                [b] => {
                    if rest > 0 {
                        self.assignments
                            .push((a.attacker, DamageTarget::Object(*b), rest));
                    }
                    self.finish_attacker(&attackers);
                }
                many if rest > 0 => {
                    let n = many.len();
                    let per_min = if to_player > 0 { lethal } else { vec![0; n] };
                    let options = many
                        .iter()
                        .map(|&object| OptionDesc::DamageRecipient { object })
                        .collect();
                    return Flow::ask(
                        Frame::CombatDamage(self),
                        ap,
                        ChoiceKind::AssignCombatDamage,
                        Primitive::Allocate {
                            total: rest,
                            per_min,
                            per_max: vec![rest; n],
                        },
                        options,
                    );
                }
                _ => self.finish_attacker(&attackers),
            }
        }
        // …then each defending player assigns for each blocking creature (CR 510.1d). No card
        // lets a creature block more than one attacker yet, so no choice is needed.
        for a in &attackers {
            for &b in &a.blockers {
                if !self.assigners.contains(&b) {
                    continue;
                }
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
            .map(|(source, target, amount)| {
                let c = chars(st, source);
                Proposed::Damage {
                    source,
                    deathtouch: c.has(Keyword::Deathtouch),
                    lifelink: c.has(Keyword::Lifelink),
                    target,
                    amount,
                    combat: true,
                }
            })
            .collect();
        if events.is_empty() {
            return Flow::done();
        }
        Flow::then(Frame::Apply(ApplyFrame::new(st, events)))
    }

    fn finish_attacker(&mut self, _attackers: &[AttackInfo]) {
        self.to_player = None;
        self.next += 1;
    }
}
