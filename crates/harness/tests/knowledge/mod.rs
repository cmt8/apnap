//! What one seat may know, kept independently of the engine's redaction (D-090, D-092):
//! where each object is, and which hidden ones the seat has been shown (CR 400.2, 402.3,
//! 701.20). Shared by `client_view.rs` (streams and views) and `client_boundary.rs` (what goes
//! over the wire).
#![allow(dead_code)]

use engine_core::client::{HIDDEN, Seat, object_ids};
use engine_core::events::Event;
use engine_core::stack::Target;
use engine_core::view::{ObjectView, PlayerView};
use engine_core::zones::Zone;
use engine_core::{GameState, ObjectId, PlayerId};
use std::collections::{BTreeMap, BTreeSet};

pub struct Model {
    pub seat: Seat,
    objects: BTreeMap<ObjectId, (Zone, PlayerId)>,
    shown: BTreeSet<ObjectId>,
}

impl Model {
    pub fn new(st: &GameState, seat: Seat) -> Self {
        Model {
            seat,
            objects: st
                .objects
                .values()
                .map(|o| (o.id, (o.zone, o.owner)))
                .collect(),
            shown: BTreeSet::new(),
        }
    }

    pub fn knows(&self, id: ObjectId) -> bool {
        if self.seat == Seat::Everyone {
            return true;
        }
        match self.objects.get(&id) {
            // Abilities on the stack, objects made since that aren't cards in hidden zones.
            None => true,
            Some((Zone::Library, _)) => self.shown.contains(&id),
            Some((Zone::Hand, owner)) => {
                self.seat == Seat::Player(*owner) || self.shown.contains(&id)
            }
            Some(_) => true,
        }
    }

    /// The objects in hidden zones the seat doesn't know now.
    pub fn unknown(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.objects.keys().copied().filter(|id| !self.knows(*id))
    }

    pub fn apply(&mut self, e: &Event) {
        match e {
            Event::ZoneChange { old, new, to, .. } => {
                let owner = self.objects.remove(old).map_or(PlayerId(0), |(_, o)| o);
                self.shown.remove(old);
                self.objects.insert(*new, (*to, owner));
            }
            Event::TokenCreated {
                object, controller, ..
            } => {
                self.objects
                    .insert(*object, (Zone::Battlefield, *controller));
            }
            Event::Revealed { cards, .. } => self.shown.extend(cards.iter().copied()),
            Event::LookedAt { player, cards } if self.seat == Seat::Player(*player) => {
                self.shown.extend(cards.iter().copied())
            }
            // CR 701.23a: the searcher looked through the library and knows what it found.
            Event::SearchFound { player, cards } if self.seat == Seat::Player(*player) => {
                self.shown.extend(cards.iter().copied())
            }
            // CR 701.20d: shuffled or otherwise reordered (put on the top or the bottom).
            Event::Shuffled { player } | Event::LibraryArranged { player, .. } => {
                let lib: Vec<ObjectId> = self
                    .objects
                    .iter()
                    .filter(|(_, (z, o))| *z == Zone::Library && o == player)
                    .map(|(id, _)| *id)
                    .collect();
                for id in lib {
                    self.shown.remove(&id);
                }
            }
            _ => {}
        }
    }

    /// Check what the seat was told of the engine event `truth`, then learn it.
    pub fn check(&mut self, truth: &Event, told: &[Event]) {
        let before: Vec<(ObjectId, bool)> = told
            .iter()
            .flat_map(object_ids)
            .map(|id| (id, id == HIDDEN || self.knows(id)))
            .collect();
        self.apply(truth);
        for e in told {
            match e {
                Event::Identified { object, .. } => {
                    assert!(self.knows(*object), "{:?} told {e:?}", self.seat)
                }
                Event::ZoneChange { old, new, .. } | Event::MovedUnseen { old, new, .. } => {
                    let old_ok = *old == HIDDEN || before.iter().any(|(id, k)| id == old && *k);
                    assert!(old_ok, "{:?} told {e:?} for {truth:?}", self.seat);
                    assert!(
                        *new == HIDDEN || self.knows(*new),
                        "{:?} told {e:?} for {truth:?}",
                        self.seat
                    );
                }
                _ => {
                    for id in object_ids(e) {
                        let ok = id == HIDDEN
                            || before.iter().any(|(i, k)| *i == id && *k)
                            || self.knows(id);
                        assert!(ok, "{:?} told {e:?} for {truth:?}", self.seat);
                    }
                }
            }
        }
    }

    /// Every object `v` (this seat's view) names is one the seat's events have made known,
    /// but for the pending decision and what it shows the seat deciding, and a library's
    /// top card a rule lets the seat see (CR 401.5). Also: what each kind of seat gets.
    pub fn check_view(&self, st: &GameState, v: &PlayerView) {
        let seat = self.seat;
        assert_eq!(v.seat, seat);
        let mut named: Vec<ObjectId> = Vec::new();
        let objects = |list: &[ObjectView], named: &mut Vec<ObjectId>| {
            for o in list {
                named.push(o.id);
                named.extend(o.attached_to);
            }
        };
        objects(&v.battlefield, &mut named);
        objects(&v.exile, &mut named);
        objects(&v.command, &mut named);
        objects(&v.hand, &mut named);
        for p in &v.players {
            objects(&p.graveyard, &mut named);
            named.extend(p.emblems.iter().map(|e| e.id));
            let all = seat == Seat::Everyone;
            assert_eq!(
                p.hand.is_some(),
                all,
                "{seat:?}: only Everyone sees every hand"
            );
            assert_eq!(p.library.is_some(), all, "{seat:?}: and every library");
            if let (Some(h), Some(l)) = (&p.hand, &p.library) {
                let ids = |l: &[ObjectView]| l.iter().map(|o| o.id).collect::<Vec<_>>();
                assert_eq!(ids(h), st.zones.hands[p.id.index()]);
                assert_eq!(ids(l), st.zones.libraries[p.id.index()]);
            }
            if let Some(top) = &p.library_top
                && seat != Seat::Everyone
                && seat != Seat::Player(p.id)
            {
                assert!(
                    p.rules.contains(&card_model::PlayerRule::TopCardRevealed),
                    "{seat:?} sees {:?}'s top card {}",
                    p.id,
                    top.name
                );
            }
        }
        for s in &v.stack {
            named.push(s.id);
            named.extend(s.ability_of);
            named.extend(s.about);
            named.extend(s.targets.iter().filter_map(|t| match t {
                Target::Object(o) => Some(*o),
                Target::Player(_) => None,
            }));
        }
        for c in v.combat.iter().flat_map(|c| &c.attackers) {
            named.push(c.attacker);
            named.extend(c.blockers.iter().copied());
            named.extend(c.planeswalker);
        }
        named.extend(v.lasting.iter().filter_map(|l| l.about));
        for id in named {
            assert!(
                self.knows(id),
                "{seat:?}'s view names {id:?}, which it may not know"
            );
        }
        if let Seat::Player(p) = seat {
            for o in &v.hand {
                assert_eq!(st.object(o.id).owner, p, "only the viewer's own hand");
                assert_eq!(st.object(o.id).zone, Zone::Hand);
            }
        } else {
            assert!(v.hand.is_empty(), "{seat:?} has no hand");
        }
        // The decision goes to the player deciding (and to Everyone); who decides is public.
        let decider = st.pending.as_ref().map(|d| d.player);
        let gets = match seat {
            Seat::Player(p) => decider == Some(p),
            Seat::Spectator => false,
            Seat::Everyone => decider.is_some(),
        };
        assert_eq!(v.decision.is_some(), gets, "{seat:?}");
        if !gets {
            assert!(v.revealed.is_empty(), "{seat:?} is shown nothing");
            assert!(v.declaration.is_empty(), "{seat:?} sees no one's plan of attack");
        }
        // The preview names only objects every seat may know (creatures on the battlefield,
        // players, planeswalkers).
        for a in &v.declaration {
            assert!(self.knows(a.creature));
        }
        assert_eq!(v.viewer().is_some(), matches!(seat, Seat::Player(_)));
        assert_eq!(v.waiting, st.pending.as_ref().map(|d| (d.player, d.kind)));
        assert_eq!(v.decision_count, st.decision_count);
        assert_eq!(v.result, st.result);
    }
}
