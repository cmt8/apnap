//! Revealing cards until one is found (decisions.md D-089; Forge's `DigUntil`). Invented
//! cards in Forge's script format, compiled the way real ones are and then played.
//! Rules text: docs/rules-notes/d089-ward-dig-until.md.

use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, Event, GameConfig, GameState, PlayerId, Primitive, Response};
use harness::forge_test_catalog;
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    sc.turn(P0, Step::PrecombatMain);
    sc
}

fn settle(s: &mut Script) {
    for _ in 0..5_000 {
        if s.next.is_none() || (s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority) {
            return;
        }
        let r = match &s.decision().prim {
            Primitive::PickN { min, .. } => Response::Pick((0..(*min).max(1)).collect()),
            Primitive::PickNumber { min, .. } => Response::Number(*min),
            Primitive::Allocate { per_min, .. } => Response::Allocate(per_min.clone()),
            Primitive::Order => Response::Order((0..s.options().len()).collect()),
        };
        s.respond(r);
    }
    panic!("did not settle");
}

fn names(s: &Script, zone: Zone, p: PlayerId) -> Vec<String> {
    s.st.zones
        .list(zone, p)
        .iter()
        .map(|&c| s.name(c))
        .collect()
}

const LOOKOUT: &str = "Name:Test Lookout\nManaCost:2 G\nTypes:Creature Elf\nPT:1/3\n\
    T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigDigUntil | TriggerDescription$ Text.\n\
    SVar:TrigDigUntil:DB$ DigUntil | Valid$ Land | ValidDescription$ land | FoundDestination$ Battlefield | Tapped$ True | RevealedDestination$ Library | RevealedLibraryPosition$ -1 | RevealRandomOrder$ True\n\
    Oracle:When this creature enters, reveal cards from the top of your library until you reveal a land card. Put that card onto the battlefield tapped and the rest on the bottom of your library in a random order.";

// CR 701.20 (2026-11-06): "reveal cards from the top of your library until you reveal a land
// card. Put that card onto the battlefield tapped and the rest on the bottom of your library
// in a random order." Every card revealed is revealed to every player (the events, and
// another seat's log, name them), and where each went.
#[test]
fn reveal_until_a_land_onto_the_battlefield() {
    let mut sc = scenario(&[LOOKOUT]);
    sc.library(P0, "Forest", 3);
    // Top of the library last: Elf, Elf, then the Forests.
    sc.cards(P0, Zone::Library, &["Test Elf", "Test Elf"]);
    sc.library(P1, "Forest", 5);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Lookout");
    let st0: GameState = sc.build();
    let mut s = Script::new(st0.clone());
    s.cast("Test Lookout").pay();
    settle(&mut s);
    let revealed: Vec<Vec<String>> = s
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Revealed { player, cards } if *player == P0 => Some(
                cards
                    .iter()
                    .map(|&c| st0.catalog.get(st0.object(c).card).name.clone())
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(revealed, [["Test Elf", "Test Elf", "Forest"]]);
    let forests =
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| s.name(id) == "Forest" && s.st.object(id).tapped)
            .count();
    assert_eq!(
        forests,
        3 + 1,
        "the land found, tapped (and the three paid with)"
    );
    let lib = names(&s, Zone::Library, P0);
    assert_eq!(
        &lib[..2],
        ["Test Elf", "Test Elf"],
        "the rest at the bottom"
    );
    // Another seat sees the revealed cards by name.
    let theirs = harness::text::narrate(&st0, &s.events, engine_core::client::Seat::Player(P1));
    // P1's own stream (redacted): the reveal is there, with the cards it shows.
    let mut view = engine_core::client::EventView::new(&st0, engine_core::client::Seat::Player(P1));
    let seen = view.redact(&s.events);
    let shown = seen
        .iter()
        .find_map(|e| match e {
            Event::Revealed { player, cards } if *player == P0 => Some(cards.clone()),
            _ => None,
        })
        .expect("revealed to P1 too");
    assert_eq!(shown.len(), 3);
    assert!(!shown.contains(&engine_core::client::HIDDEN));
    assert!(
        theirs
            .iter()
            .any(|l| l.contains("reveals") && l.contains("Test Elf")),
        "{theirs:#?}"
    );
    let cat = forge_test_catalog(&[LOOKOUT], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Lookout").unwrap()));
    assert_eq!(
        text,
        [
            "When this creature enters, reveal cards from the top of your library until you reveal a land card. Put that card onto the battlefield tapped and the rest on the bottom of your library in a random order."
        ]
    );
}

// "Until you reveal a basic land card. Put that card into your hand and all other cards
// revealed this way into your graveyard." Nothing found: every card is revealed and put away.
#[test]
fn reveal_until_into_hand_rest_to_graveyard() {
    let druid = "Name:Test Hermit\nManaCost:1 G\nTypes:Creature Druid\nPT:1/1\n\
        A:AB$ DigUntil | Cost$ G | Valid$ Land.Basic | ValidDescription$ basic land | FoundDestination$ Hand | RevealedDestination$ Graveyard | SpellDescription$ Text.\n\
        Oracle:{G}: Reveal cards from the top of your library until you reveal a basic land card. Put that card into your hand and all other cards revealed this way into your graveyard.";
    let mut sc = scenario(&[druid]);
    sc.library(P0, "Forest", 1);
    sc.cards(P0, Zone::Library, &["Test Elf", "Test Golem"]);
    sc.library(P1, "Forest", 5);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest", "Test Hermit"]);
    let mut s = Script::new(sc.build());
    s.activate("Test Hermit").pay();
    settle(&mut s);
    assert!(s.find(P0, Zone::Hand, "Forest").is_some());
    assert_eq!(names(&s, Zone::Graveyard, P0), ["Test Golem", "Test Elf"]);
    assert!(s.st.zones.list(Zone::Library, P0).is_empty());
    // Again with nothing left: nothing is revealed.
    s.activate("Test Hermit").pay();
    settle(&mut s);
    assert_eq!(s.count(|e| matches!(e, Event::Revealed { .. })), 1);
}

// "Each opponent reveals cards from the top of their library until they reveal a land card,
// then puts those cards into their graveyard" (no `FoundDestination$`: the found card goes
// with the rest).
#[test]
fn each_opponent_reveals_until() {
    let grind = "Name:Test Grinder\nManaCost:2 U B\nTypes:Creature Horror\nPT:2/2\n\
        A:AB$ DigUntil | Cost$ U | Defined$ Player.Opponent | Valid$ Land | ValidDescription$ land | RevealedDestination$ Graveyard | SpellDescription$ Text.\n\
        Oracle:{U}: Each opponent reveals cards from the top of their library until they reveal a land card, then puts those cards into their graveyard.";
    let mut sc = scenario(&[grind]);
    sc.library(P0, "Forest", 5);
    sc.library(P1, "Island", 2);
    sc.cards(P1, Zone::Library, &["Test Elf", "Test Golem"]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Test Grinder"]);
    let mut s = Script::new(sc.build());
    s.activate("Test Grinder").pay();
    settle(&mut s);
    let mut gy = names(&s, Zone::Graveyard, P1);
    gy.sort();
    assert_eq!(gy, ["Island", "Test Elf", "Test Golem"]);
    assert_eq!(names(&s, Zone::Graveyard, P1).len(), 3);
    assert_eq!(s.st.zones.list(Zone::Library, P1).len(), 1);
}

// CR 701.20d: "If cards in a player's library are shuffled or otherwise reordered, any
// revealed cards that are reordered stop being revealed." The rest put on the bottom (in a
// random order) is no longer known to another seat: drawing one later tells P1 nothing.
#[test]
fn the_rest_put_on_the_bottom_stops_being_revealed() {
    let draw = "Name:Test Glimpse\nManaCost:G\nTypes:Sorcery\n\
        A:SP$ Draw | NumCards$ 2 | SpellDescription$ Text.\nOracle:Draw two cards.";
    let mut sc = scenario(&[LOOKOUT, draw]);
    // Bottom to top: Forest, Elf, Elf. The dig takes the Forest; the Elves go to the bottom.
    sc.card(P0, Zone::Library, "Forest");
    sc.cards(P0, Zone::Library, &["Test Elf", "Test Elf"]);
    sc.library(P1, "Forest", 5);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Forest", "Forest", "Forest"],
    );
    sc.cards(P0, Zone::Hand, &["Test Lookout", "Test Glimpse"]);
    let st0: GameState = sc.build();
    let mut s = Script::new(st0.clone());
    s.cast("Test Lookout").pay();
    settle(&mut s);
    let before = s.events.len();
    s.cast("Test Glimpse").pay();
    settle(&mut s);
    let mut view = engine_core::client::EventView::new(&st0, engine_core::client::Seat::Player(P1));
    let seen = view.redact(&s.events);
    // The draws, as P1 is told them: unseen moves of hidden cards.
    let draws: Vec<&Event> = seen
        .iter()
        .filter(|e| {
            matches!(e, Event::MovedUnseen { from: Zone::Library, to: Zone::Hand, old, .. }
                if *old == engine_core::client::HIDDEN)
        })
        .collect();
    assert_eq!(draws.len(), 2, "{:?}", &seen[before.min(seen.len())..]);
    assert!(
        !seen.iter().any(|e| matches!(
            e,
            Event::ZoneChange {
                from: Zone::Library,
                to: Zone::Hand,
                ..
            }
        )),
        "P1 doesn't follow the drawn Elves"
    );
}
