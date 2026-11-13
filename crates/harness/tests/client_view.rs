//! What a client sees (D-090): a seat's redacted event stream (`client::EventView`) and its
//! `PlayerView` carry no card a seat may not know. Rules text:
//! docs/rules-notes/400-hidden-information.md (CR 400.2, 400.7, 401.2, 402.3, 701.20).

use engine_core::client::{EventView, HIDDEN, Seat, object_ids};
use engine_core::events::Event;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{Agent, GameConfig, GameState, ObjectId, OptionDesc, PlayerId, player_view};
use harness::script::Script;
use harness::{Setup, forge_test_catalog};
use std::collections::{BTreeMap, BTreeSet};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// What one seat may know, kept independently of the engine's redaction: where each object
/// is, and which hidden ones the seat has been shown (CR 400.2, 402.3, 701.20).
struct Model {
    seat: Seat,
    objects: BTreeMap<ObjectId, (Zone, PlayerId)>,
    shown: BTreeSet<ObjectId>,
}

impl Model {
    fn new(st: &GameState, seat: Seat) -> Self {
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

    fn knows(&self, id: ObjectId) -> bool {
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

    fn apply(&mut self, e: &Event) {
        match e {
            Event::ZoneChange { old, new, to, .. } => {
                let owner = self.objects.remove(old).map_or(P0, |(_, o)| o);
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
    fn check(&mut self, truth: &Event, told: &[Event]) {
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
}

/// Every card a `PlayerView` names is one its viewer may know.
fn check_view(st: &GameState, viewer: PlayerId) {
    let v = player_view(st, viewer);
    let decides = st.pending.as_ref().is_some_and(|d| d.player == viewer);
    let mut named: Vec<ObjectId> = Vec::new();
    named.extend(v.battlefield.iter().map(|o| o.id));
    named.extend(v.exile.iter().map(|o| o.id));
    named.extend(v.command.iter().map(|o| o.id));
    for p in &v.players {
        named.extend(p.graveyard.iter().map(|o| o.id));
    }
    for id in named {
        let z = st.object(id).zone;
        assert!(
            !matches!(z, Zone::Library | Zone::Hand),
            "public lists name a hidden card"
        );
    }
    for o in &v.hand {
        assert_eq!(st.object(o.id).owner, viewer, "only the viewer's own hand");
        assert_eq!(st.object(o.id).zone, Zone::Hand);
    }
    if !decides {
        assert!(v.decision.is_none() && v.revealed.is_empty());
    }
    // Who the game waits for is public; what they may choose from isn't.
    assert_eq!(v.waiting, st.pending.as_ref().map(|d| (d.player, d.kind)));
}

/// A whole random game: every seat's stream (and a spectator's) checked event by event
/// against the knowledge model, and every seat's view at every decision.
fn play_and_check(setup: Setup, seed: u64) -> usize {
    let mut st = setup.game(seed);
    let n = st.num_players();
    let seats: Vec<Seat> = (0..n)
        .map(|p| Seat::Player(PlayerId(p as u8)))
        .chain([Seat::Spectator])
        .collect();
    let mut views: Vec<EventView> = seats.iter().map(|s| EventView::new(&st, *s)).collect();
    let mut models: Vec<Model> = seats.iter().map(|s| Model::new(&st, *s)).collect();
    let mut agents: Vec<ai::RandomAgent> = (0..n)
        .map(|p| ai::RandomAgent::new(seed * 31 + p as u64))
        .collect();
    let mut out = engine_core::start(&mut st);
    let mut decisions = 0;
    let mut identified = 0;
    loop {
        for e in &out.truth {
            for (v, m) in views.iter_mut().zip(models.iter_mut()) {
                let told = v.redact(std::slice::from_ref(e));
                identified += told
                    .iter()
                    .filter(|t| matches!(t, Event::Identified { .. }))
                    .count();
                m.check(e, &told);
            }
        }
        for p in 0..n {
            check_view(&st, PlayerId(p as u8));
        }
        let Some(d) = out.next.clone() else { break };
        decisions += 1;
        if decisions > 3_000 {
            break;
        }
        let r = agents[d.player.index()].decide(&player_view(&st, d.player), &d);
        out = engine_core::step(&mut st, r).expect("legal");
    }
    assert!(identified > 0);
    decisions
}

// CR 400.2, 400.7, 402.3, 701.20 (2026-11-06): over random games of every compiled card, no
// seat's stream names an object of a hidden zone it may not know, and no id links a hidden
// card to what it was or becomes.
#[test]
fn random_games_leak_nothing() {
    let setup = Setup::parse("forge-4p").expect("forge setup");
    let mut total = 0;
    for seed in 1..=4 {
        total += play_and_check(setup, seed);
    }
    assert!(total > 100);
    let m2 = Setup::parse("m2-4p").expect("m2");
    for seed in 1..=3 {
        play_and_check(m2, seed);
    }
}

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 47);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

/// Each seat's version of `s`'s events since `start`.
fn told(start: &GameState, s: &Script, seat: Seat) -> Vec<Event> {
    EventView::new(start, seat).redact(&s.events)
}

fn identified(told: &[Event]) -> Vec<card_model::CardDefId> {
    told.iter()
        .filter_map(|e| match e {
            Event::Identified { card, .. } => Some(*card),
            _ => None,
        })
        .collect()
}

const DIG: &str = "Name:Test Quiet Tutor\nManaCost:B\nTypes:Sorcery\n\
    A:SP$ ChangeZone | Origin$ Library | Destination$ Hand | ChangeType$ Card | ChangeNum$ 1 | Mandatory$ True | SpellDescription$ Search your library for a card, put it into your hand, then shuffle.\n\
    Oracle:Search your library for a card, put it into your hand, then shuffle.";
const LOUD: &str = "Name:Test Loud Tutor\nManaCost:G\nTypes:Sorcery\n\
    A:SP$ ChangeZone | Origin$ Library | Destination$ Hand | ChangeType$ Land.Basic | ChangeNum$ 1 | Mandatory$ True | SpellDescription$ Search your library for a basic land card, reveal it, put it into your hand, then shuffle.\n\
    Oracle:Search your library for a basic land card, reveal it, put it into your hand, then shuffle.";
const PEEK: &str = "Name:Test Peek\nManaCost:U\nTypes:Sorcery\n\
    A:SP$ Dig | DigNum$ 3 | ChangeNum$ 1 | DestinationZone$ Hand | SpellDescription$ Look at the top three cards of your library. Put one of them into your hand and the rest on the bottom of your library in any order.\n\
    Oracle:Look at the top three cards of your library. Put one of them into your hand and the rest on the bottom of your library in any order.";
const MILL: &str = "Name:Test Mill\nManaCost:U\nTypes:Sorcery\n\
    A:SP$ Mill | NumCards$ 2 | ValidTgts$ Player | SpellDescription$ Target player mills two cards.\n\
    Oracle:Target player mills two cards.";

// CR 401.2, 402.3: a draw — its owner is told the card, everyone else that a card was drawn.
// CR 701.23a, 701.20a: a tutor that doesn't reveal tells the others only that a card went
// from the library to the hand; one that reveals tells everyone which.
#[test]
fn cr_402_3_draws_and_tutors() {
    let mut sc = scenario(&[DIG, LOUD]);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Forest"]);
    sc.cards(P0, Zone::Hand, &["Test Quiet Tutor", "Test Loud Tutor"]);
    sc.card(P0, Zone::Library, "Plains");
    sc.turn(P0, Step::PrecombatMain);
    let start = sc.build();
    let mut s = Script::new(start.clone());
    let plains = s.st.catalog.find("Plains").unwrap();
    s.cast("Test Quiet Tutor").pay().pass_all();
    s.pick(
        "Plains",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Plains"),
    );
    let mine = told(&start, &s, Seat::Player(P0));
    let theirs = told(&start, &s, Seat::Player(P1));
    assert!(identified(&mine).contains(&plains), "the owner knows");
    assert!(!identified(&theirs).contains(&plains), "the others don't");
    assert!(theirs.iter().any(|e| matches!(e,
        Event::MovedUnseen { owner, from: Zone::Library, to: Zone::Hand, old, new }
            if *owner == P0 && *old == HIDDEN && *new == HIDDEN)));
    // A revealing tutor (for a Forest of the library).
    let start = s.st.clone();
    let before = s.events.len();
    s.cast("Test Loud Tutor").pay().pass_all();
    s.pick(
        "Forest",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Forest"),
    );
    let forest = s.st.catalog.find("Forest").unwrap();
    let theirs = EventView::new(&start, Seat::Player(P1)).redact(&s.events[before..]);
    assert!(theirs.iter().any(|e| matches!(e, Event::Revealed { .. })));
    assert!(
        identified(&theirs).contains(&forest),
        "revealed: everyone knows"
    );
}

// CR 701.20e: looking at cards shows them only to the player who looks. CR 400.2: milled
// cards go to a public zone — everyone knows them, but not where in the library they were.
#[test]
fn cr_701_20e_looking_and_milling() {
    let mut sc = scenario(&[PEEK, MILL]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    sc.cards(P0, Zone::Hand, &["Test Peek", "Test Mill"]);
    sc.turn(P0, Step::PrecombatMain);
    let start = sc.build();
    let mut s = Script::new(start.clone());
    s.cast("Test Peek").pay().pass_all();
    // Take the first card offered; the rest go to the bottom.
    let first = s.options()[0].clone();
    s.pick("one", |_, o| *o == first);
    while s.kind() != engine_core::ChoiceKind::Priority {
        let n = s.options().len();
        s.respond(engine_core::Response::Order((0..n).collect()));
    }
    let mine = told(&start, &s, Seat::Player(P0));
    let theirs = told(&start, &s, Seat::Player(P1));
    let looked = |t: &[Event]| {
        t.iter()
            .find_map(|e| match e {
                Event::LookedAt { cards, .. } => Some(cards.clone()),
                _ => None,
            })
            .expect("a look")
    };
    assert!(looked(&mine).iter().all(|c| *c != HIDDEN));
    assert!(looked(&theirs).iter().all(|c| *c == HIDDEN));
    // Mill the opponent.
    let start = s.st.clone();
    let before = s.events.len();
    s.cast("Test Mill");
    s.target_player(P1).pay().pass_all();
    let theirs = EventView::new(&start, Seat::Player(P0)).redact(&s.events[before..]);
    let milled: Vec<&Event> = theirs
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::MovedUnseen {
                    from: Zone::Library,
                    to: Zone::Graveyard,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(milled.len(), 2);
    for e in milled {
        let Event::MovedUnseen { old, new, .. } = e else {
            unreachable!()
        };
        assert_eq!(*old, HIDDEN, "not where it was in the library");
        assert_ne!(*new, HIDDEN, "the graveyard card is public");
    }
}

// CR 103.5, 402.3: a mulligan shuffles the hand away — the others never learn the cards.
// The stream is serialisable and the same every time (deterministic).
#[test]
fn mulligans_and_a_spectator() {
    let setup = Setup::parse("m1-2p").expect("m1");
    let start = setup.game(7);
    let mut st = start.clone();
    let mut out = engine_core::start(&mut st);
    let mut truth = out.truth.clone();
    // Mulligan once, then keep, for every seat that is asked.
    let mut asked = BTreeSet::new();
    while let Some(d) = out.next.clone() {
        let r = match d
            .options
            .iter()
            .position(|o| matches!(o, OptionDesc::Mulligan))
        {
            Some(i) if asked.insert(d.player) => engine_core::Response::Pick(vec![i]),
            _ => ai::RandomAgent::new(1).decide(&player_view(&st, d.player), &d),
        };
        out = engine_core::step(&mut st, r).unwrap();
        truth.extend(out.truth.clone());
        if st.turn.turn_number >= 1 {
            break;
        }
    }
    let p1_hand_cards: Vec<ObjectId> = st.zones.hands[1].clone();
    for seat in [Seat::Player(P0), Seat::Spectator] {
        let told = EventView::new(&start, seat).redact(&truth);
        let again = EventView::new(&start, seat).redact(&truth);
        assert_eq!(told, again, "deterministic");
        let json = serde_json::to_string(&told).unwrap();
        let back: Vec<Event> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, told, "serialisable");
        for e in &told {
            for id in object_ids(e) {
                assert!(
                    !p1_hand_cards.contains(&id),
                    "{seat:?} sees P1's hand: {e:?}"
                );
            }
        }
        // A spectator sees no hand at all.
        if seat == Seat::Spectator {
            for e in &told {
                if let Event::Identified { object, .. } = e {
                    assert!(
                        st.objects.get(object).is_none_or(|o| o.zone != Zone::Hand),
                        "{e:?}"
                    );
                }
            }
        }
    }
}
