//! Scenario builder for atomic rules tests (spec §10.1, XMage-style: place cards, then
//! drive with responses). Enabled only by the `test-support` feature.
//!
//! Building a scenario writes `GameState` directly instead of going through
//! `actions::execute` — the one sanctioned exception to CLAUDE.md invariant 2, because it
//! constructs a position rather than playing to it. Everything after `build()` is normal play.

use crate::config::GameConfig;
use crate::frames::{Frame, PriorityFrame, StepStage};
use crate::ids::{CardInstanceId, ObjectId, PlayerId};
use crate::objects::GameObject;
use crate::stack::{StackKind, StackObject};
use crate::state::GameState;
use crate::turn::Step;
use crate::zones::Zone;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use card_model::CardCatalog;

#[derive(Debug)]
pub struct Scenario {
    pub st: GameState,
}

impl Scenario {
    /// No cards anywhere; turn 1, `PlayerId(0)` active in the precombat main phase.
    pub fn new(config: GameConfig, catalog: Arc<CardCatalog>, seed: u64) -> Self {
        let n = config.num_players as usize;
        let mut st = crate::new_game(config, catalog, &vec![Vec::new(); n], seed)
            .expect("valid scenario config");
        st.frames.clear();
        st.turn.turn_number = 1;
        st.turn.active = PlayerId(0);
        st.turn.step = Step::PrecombatMain;
        st.turn.starting_player = Some(PlayerId(0));
        Scenario { st }
    }

    /// Put a card named `name`, owned by `owner`, into `zone`. Library cards go on top.
    /// Battlefield permanents are not summoning sick (see [`Scenario::sick`]).
    pub fn card(&mut self, owner: PlayerId, zone: Zone, name: &str) -> ObjectId {
        let card = self
            .st
            .catalog
            .find(name)
            .unwrap_or_else(|| panic!("no card named {name:?}"));
        let id = self.st.alloc_object_id();
        let timestamp = self.st.alloc_timestamp();
        let card_instance = CardInstanceId(self.st.next_card_instance);
        self.st.next_card_instance += 1;
        self.st.objects.insert(
            id,
            GameObject {
                id,
                card_instance,
                card,
                owner,
                controller: owner,
                zone,
                timestamp,
                tapped: false,
                damage_marked: 0,
                summoning_sick: false,
                counters: Default::default(),
                is_token: self.st.catalog.get(card).token,
                deathtouch_damage: false,
                attached_to: None,
                last_controller: owner,
                face: 0,
                transforms: 0,
                adventure: None,
                cast_with: None,
                chosen: Vec::new(),
                copy: None,
                spell_copy: false,
            },
        );
        if zone == Zone::Stack {
            self.st.stack.push(StackObject {
                id,
                controller: owner,
                kind: StackKind::Spell,
                source_lki: None,
                subject: None,
                subject_lki: None,
                modes: Vec::new(),
                x: 0,
                targets: Vec::new(),
                sacrificed_power: 0,
                facts: Default::default(),
                source_transforms: None,
                alt: None,
                chosen: Vec::new(),
            });
        } else {
            self.st.zones.list_mut(zone, owner).push(id);
        }
        id
    }

    pub fn cards(&mut self, owner: PlayerId, zone: Zone, names: &[&str]) -> Vec<ObjectId> {
        names.iter().map(|n| self.card(owner, zone, n)).collect()
    }

    /// `n` copies of `name` in `owner`'s library (so draws don't deck anyone).
    pub fn library(&mut self, owner: PlayerId, name: &str, n: usize) {
        for _ in 0..n {
            self.card(owner, Zone::Library, name);
        }
    }

    /// Put `name` into `zone` as `owner`'s commander (CR 903.3).
    pub fn commander(&mut self, owner: PlayerId, zone: Zone, name: &str) -> ObjectId {
        let id = self.card(owner, zone, name);
        let ci = self.st.object(id).card_instance;
        self.st.commanders.push((owner, ci));
        self.st.commander_cards.insert(ci, self.st.object(id).card);
        id
    }

    /// Attach Aura `aura` to `target` (CR 303.4).
    pub fn attach(&mut self, aura: ObjectId, target: ObjectId) {
        self.st.object_mut(aura).attached_to = Some(target);
    }

    /// Set the combat damage `victim` has taken from commander object `commander`.
    pub fn commander_damage(&mut self, victim: PlayerId, commander: ObjectId, n: u32) {
        let ci = self.st.object(commander).card_instance;
        self.st.player_mut(victim).commander_damage.insert(ci, n);
    }

    pub fn sick(&mut self, id: ObjectId) {
        self.st.object_mut(id).summoning_sick = true;
    }

    pub fn tapped(&mut self, id: ObjectId) {
        self.st.object_mut(id).tapped = true;
    }

    pub fn life(&mut self, p: PlayerId, life: i32) {
        self.st.player_mut(p).life = life;
    }

    pub fn turn(&mut self, active: PlayerId, step: Step) {
        self.st.turn.active = active;
        self.st.turn.step = step;
    }

    /// The scenario begins with the active player about to receive priority in the current
    /// step (state-based actions are checked first, as always). Call `engine_core::start`.
    pub fn build(mut self) -> GameState {
        self.st.generation += 1; // invalidate the layer cache after direct writes
        let player = self.st.first_priority_player();
        self.st.frames = vec![
            Frame::Step(StepStage::End {
                repeat_cleanup: false,
            }),
            Frame::Priority(PriorityFrame::new(player, 0)),
        ];
        self.st
    }
}
