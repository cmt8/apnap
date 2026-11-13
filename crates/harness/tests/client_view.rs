//! What a client sees (D-090, D-092): a seat's redacted event stream (`client::EventView`)
//! and its `PlayerView` (a player's, a spectator's, everyone's) carry no card a seat may not
//! know. Rules text: docs/rules-notes/400-hidden-information.md (CR 400.2, 400.7, 401.2,
//! 402.3, 701.20).

mod knowledge;

use engine_core::client::{EventView, HIDDEN, Seat, object_ids};
use engine_core::events::Event;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{Agent, GameConfig, GameState, ObjectId, OptionDesc, PlayerId, player_view};
use harness::script::Script;
use harness::{Setup, forge_test_catalog};
use knowledge::Model;
use std::collections::BTreeSet;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// A whole random game: every seat's stream (and a spectator's, and everyone's) checked event
/// by event against the knowledge model, and every seat's view at every decision against what
/// its stream has made known.
fn play_and_check(setup: Setup, seed: u64) -> usize {
    let mut st = setup.game(seed);
    let n = st.num_players();
    let seats: Vec<Seat> = (0..n)
        .map(|p| Seat::Player(PlayerId(p as u8)))
        .chain([Seat::Spectator, Seat::Everyone])
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
        for m in &models {
            m.check_view(&st, &engine_core::seat_view(&st, m.seat));
        }
        let Some(d) = out.next.clone() else { break };
        decisions += 1;
        if decisions > 100_000 {
            break;
        }
        let r = agents[d.player.index()].decide(&player_view(&st, d.player), &d);
        out = engine_core::step(&mut st, r).expect("legal");
    }
    assert!(identified > 0);
    decisions
}

/// `forge` games of every compiled card and `m2` of the M2 test decks, on four threads.
fn many(forge: u64, m2: u64) {
    let f = Setup::parse("forge-4p").expect("forge setup");
    let m = Setup::parse("m2-4p").expect("m2");
    let games: Vec<(Setup, u64)> = (1..=forge)
        .map(|s| (f, s))
        .chain((1..=m2).map(|s| (m, s)))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let total: usize = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let mut decisions = 0;
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(setup, seed)) = games.get(i) else {
                            break decisions;
                        };
                        decisions += play_and_check(setup, seed);
                    }
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().expect("a game")).sum()
    });
    assert!(total > 100 * games.len());
}

// CR 400.2, 400.7, 402.3, 701.20 (2026-11-06): over random games of every compiled card, no
// seat's stream names an object of a hidden zone it may not know, no id links a hidden card
// to what it was or becomes, and no seat's view names what its stream hasn't made known.
#[test]
fn random_games_leak_nothing() {
    many(24, 6);
}

/// The long run, for the soak: `APNAP_LEAK_GAMES` forge-4p games (default 200).
#[test]
#[ignore]
fn random_games_leak_nothing_long() {
    let n = std::env::var("APNAP_LEAK_GAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    many(n, n / 10);
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
        Event::MovedUnseen { owner, from: Zone::Library, to: Zone::Hand, old, new, .. }
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

/// "Put a card from your hand on top of your library."
const STASH: &str = "Name:Test Stash\nManaCost:U\nTypes:Sorcery\n\
    A:SP$ ChangeZone | Origin$ Hand | Destination$ Library | ChangeType$ Card | ChangeNum$ 1 | Mandatory$ True | SpellDescription$ Put a card from your hand on top of your library.\n\
    Oracle:Put a card from your hand on top of your library.";

// D-090 part 2, CR 701.23a: what a search found. The searcher is told which cards; the
// opponent only how many (each HIDDEN) — the same search, read by each seat.
#[test]
fn a_search_result_is_named_only_to_the_searcher() {
    let mut sc = scenario(&[DIG]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    sc.card(P0, Zone::Hand, "Test Quiet Tutor");
    sc.card(P0, Zone::Library, "Plains");
    sc.turn(P0, Step::PrecombatMain);
    let start = sc.build();
    let mut s = Script::new(start.clone());
    s.cast("Test Quiet Tutor").pay().pass_all();
    s.pick(
        "Plains",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Plains"),
    );
    let found = |t: &[Event]| {
        t.iter()
            .find_map(|e| match e {
                Event::SearchFound { player, cards } if *player == P0 => Some(cards.clone()),
                _ => None,
            })
            .expect("a search result")
    };
    let mine = found(&told(&start, &s, Seat::Player(P0)));
    let theirs = found(&told(&start, &s, Seat::Player(P1)));
    let watching = found(&told(&start, &s, Seat::Spectator));
    assert_eq!(mine.len(), 1);
    assert_ne!(mine[0], HIDDEN, "the searcher knows the card");
    assert_eq!(theirs, [HIDDEN], "the opponent: one card, not which");
    assert_eq!(watching, [HIDDEN], "a spectator likewise");
}

// D-090 part 2, CR 401.4: where a card went in a library is public; which card isn't. The
// opponent is told a card of P0's went from hand to the top of the library.
#[test]
fn library_position_is_public_the_card_is_not() {
    let mut sc = scenario(&[STASH]);
    sc.cards(P0, Zone::Battlefield, &["Island"]);
    sc.card(P0, Zone::Hand, "Test Stash");
    sc.card(P0, Zone::Hand, "Test Golem");
    sc.turn(P0, Step::PrecombatMain);
    let start = sc.build();
    let mut s = Script::new(start.clone());
    s.cast("Test Stash").pay().pass_all();
    if s.kind() != engine_core::ChoiceKind::Priority {
        let first = s.options()[0].clone();
        s.pick("the Golem", |_, o| *o == first);
    }
    let theirs = told(&start, &s, Seat::Player(P1));
    let golem = s.st.catalog.find("Test Golem").unwrap();
    assert!(!identified(&theirs).contains(&golem), "{theirs:#?}");
    assert!(
        theirs.iter().any(|e| matches!(e,
            Event::MovedUnseen { owner, from: Zone::Hand, to: Zone::Library, old, new,
                position: Some(engine_core::zones::LibraryPosition::Top) }
                if *owner == P0 && *old == HIDDEN && *new == HIDDEN)),
        "{theirs:#?}"
    );
}

// D-092, CR 400.2, 701.23a: one search, four views of it. The searcher gets the decision and
// the library it searches; the opponent and a spectator get neither — only who decides and
// what kind of decision, and how many cards each hidden zone has; everyone (a replay) gets
// every hand and library and the decision.
#[test]
fn each_seat_sees_its_own_view_of_a_search() {
    let mut sc = scenario(&[DIG]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    sc.card(P0, Zone::Hand, "Test Quiet Tutor");
    sc.card(P0, Zone::Hand, "Test Golem");
    sc.card(P0, Zone::Library, "Plains");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Quiet Tutor").pay().pass_all();
    let d = s.st.pending.clone().expect("the search");
    assert_eq!(d.player, P0);
    let names = |l: &[engine_core::view::ObjectView]| -> Vec<String> {
        l.iter().map(|o| o.name.clone()).collect()
    };
    let searcher = engine_core::seat_view(&s.st, Seat::Player(P0));
    assert_eq!(searcher.decision.as_ref(), Some(&d));
    assert!(names(&searcher.revealed).contains(&"Plains".to_string()));
    assert_eq!(names(&searcher.hand), ["Test Golem"]);
    for seat in [Seat::Player(P1), Seat::Spectator] {
        let v = engine_core::seat_view(&s.st, seat);
        assert_eq!(v.seat, seat);
        assert!(v.decision.is_none(), "{seat:?} gets no options");
        assert!(
            v.revealed.is_empty(),
            "{seat:?} sees nothing of the library"
        );
        assert_eq!(
            v.waiting,
            Some((P0, d.kind)),
            "but who decides, and what kind"
        );
        let p0 = &v.players[0];
        assert!(
            p0.hand.is_none() && p0.library.is_none(),
            "{seat:?}: counts only"
        );
        assert_eq!(p0.hand_size, 1);
        assert!(p0.library_top.is_none());
        let json = serde_json::to_string(&v).unwrap();
        assert!(
            !json.contains("Test Golem"),
            "{seat:?} never reads P0's hand"
        );
        assert!(!json.contains("Plains"), "or the library searched");
        if seat == Seat::Spectator {
            assert!(v.hand.is_empty(), "a spectator has no hand");
        }
    }
    let all = engine_core::seat_view(&s.st, Seat::Everyone);
    assert_eq!(all.decision.as_ref(), Some(&d));
    let p0 = &all.players[0];
    assert_eq!(names(p0.hand.as_deref().unwrap()), ["Test Golem"]);
    assert!(names(p0.library.as_deref().unwrap()).contains(&"Plains".to_string()));
    assert_eq!(all.decision_count, s.st.decision_count);
}

// D-093: a view that is no player's has no "me" — `viewer()` is `None` for a spectator and
// for everyone, and an agent handed one fails loudly instead of playing as player 0.
#[test]
fn only_a_players_view_has_a_viewer() {
    let mut st = Setup::parse("m1-2p").expect("m1").game(3);
    engine_core::start(&mut st);
    assert_eq!(
        engine_core::seat_view(&st, Seat::Player(P1)).viewer(),
        Some(P1)
    );
    for seat in [Seat::Spectator, Seat::Everyone] {
        assert_eq!(engine_core::seat_view(&st, seat).viewer(), None, "{seat:?}");
    }
}

#[test]
#[should_panic(expected = "heuristic agent was handed a Spectator view")]
fn an_agent_handed_a_spectators_view_fails_loudly() {
    let mut st = Setup::parse("m1-2p").expect("m1").game(3);
    let d = engine_core::start(&mut st).next.expect("a decision");
    let view = engine_core::seat_view(&st, Seat::Spectator);
    let mut ai = ai::HeuristicAgent::new(1, st.catalog.clone());
    ai.decide(&view, &d);
}
