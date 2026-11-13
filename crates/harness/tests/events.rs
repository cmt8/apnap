//! What the events say (D-090 part 2): enough for a client to log, animate and explain a
//! game without reading `GameState` — each new field, and that events still round-trip.

use engine_core::events::{Event, StackExit, TapCause};
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{GameConfig, OptionDesc, PlayerId};
use harness::script::Script;
use harness::{Setup, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const BEAST: (&str, &str) = (
    "g_3_3_test_beast",
    "Name:Test Beast Token\nManaCost:no cost\nColors:green\nTypes:Creature Beast\nPT:3/3\nOracle:",
);

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(
        GameConfig::commander(2),
        forge_test_catalog(cards, &[BEAST]),
        53,
    );
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

const THINK: &str = "Name:Test Think Again\nManaCost:1 U\nTypes:Instant\n\
    K:Flashback:U\n\
    A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
    Oracle:Draw a card.\\nFlashback {U}";

// CR 601.2i, 118.9, 608.2: a cast says its face and its alternative cost; the resolution
// says when it begins (before it is done, `Resolved`); paying says which mana and what
// tapped it for mana (CR 605).
#[test]
fn a_flashback_cast_and_its_resolution() {
    let mut sc = scenario(&[THINK]);
    let island = sc.card(P0, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Graveyard, "Test Think Again");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast_with("Test Think Again").pay().pass_all();
    let cast = s
        .events
        .iter()
        .find_map(|e| match e {
            Event::SpellCast {
                spell, alt, face, ..
            } => Some((*spell, *alt, *face)),
            _ => None,
        })
        .expect("cast");
    let alt = cast.1.expect("for an alternative cost");
    assert!(matches!(
        s.st.catalog.ability(alt),
        card_model::AbilityDef::AlternativeCost(c) if c.kind == card_model::AltKind::Flashback
    ));
    assert_eq!(cast.2, 0);
    let begun = s
        .events
        .iter()
        .position(|e| matches!(e, Event::ResolutionBegan { object } if *object == cast.0));
    let done = s
        .events
        .iter()
        .position(|e| matches!(e, Event::Resolved { spell } if *spell == cast.0));
    assert!(begun.is_some() && begun < done, "begun before resolved");
    assert!(s.events.iter().any(|e| matches!(e,
        Event::ManaPaid { player, amount: 1, mana } if *player == P0
            && *mana == [engine_core::mana::ManaType::Blue])));
    assert!(s.events.iter().any(|e| matches!(e,
        Event::ManaAdded { source: Some(src), restricted: false, .. } if *src == island)));
    assert!(s.events.iter().any(|e| matches!(e,
        Event::Tapped { object, cause: TapCause::Mana } if *object == island)));
}

const PINGER: &str = "Name:Test Event Pinger\nManaCost:1 R\nTypes:Creature Goblin\nPT:1/1\n\
    A:AB$ DealDamage | Cost$ T | ValidTgts$ Any | NumDmg$ 1 | SpellDescription$ CARDNAME deals 1 damage to any target.\n\
    Oracle:{T}: This creature deals 1 damage to any target.";
const MAYBE: &str = "Name:Test Maybe\nManaCost:G\nTypes:Creature Cleric\nPT:1/1\n\
    T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | OptionalDecider$ You | Execute$ TrigGain | TriggerDescription$ When this creature enters, you may gain 3 life.\n\
    SVar:TrigGain:DB$ GainLife | LifeAmount$ 3\n\
    Oracle:When this creature enters, you may gain 3 life.";

// CR 602.2a: an activated ability says which ability it is (its text) and why it left the
// stack; a tap for a cost says so. CR 700.4: the creature it kills "dies" — the move says it
// was a creature as it last existed.
#[test]
fn an_activated_ability_its_cost_and_a_death() {
    let mut sc = scenario(&[PINGER]);
    let pinger = sc.card(P0, Zone::Battlefield, "Test Event Pinger");
    let elf = sc.card(P1, Zone::Battlefield, "Test Elf");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.activate("Test Event Pinger");
    s.target_object(elf).pass_all();
    let (ability, text) = s
        .events
        .iter()
        .find_map(|e| match e {
            Event::AbilityActivated {
                ability,
                text,
                triggered: false,
                ..
            } => Some((*ability, *text)),
            _ => None,
        })
        .expect("activated");
    assert_eq!(text.card, s.st.object(pinger).card);
    assert!(s.events.iter().any(|e| matches!(e,
        Event::Tapped { object, cause: TapCause::Cost } if *object == pinger)));
    assert!(s.events.iter().any(|e| matches!(e,
        Event::AbilityRemoved { ability: a, reason: StackExit::Resolved } if *a == ability)));
    assert!(s.events.iter().any(|e| matches!(e,
        Event::ZoneChange { old, from: Zone::Battlefield, to: Zone::Graveyard, creature: true, .. }
            if *old == elf)));
}

// CR 608.2d: "you may" declined is said.
#[test]
fn a_declined_may() {
    let mut sc = scenario(&[MAYBE]);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Maybe");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Maybe").pay().pass_all();
    while s.kind() == engine_core::ChoiceKind::Priority {
        s.pass();
    }
    s.pick("no", |_, o| matches!(o, OptionDesc::Decline));
    assert!(s.events.iter().any(|e| matches!(e,
        Event::MayChosen { player, yes: false, .. } if *player == P0)));
}

const HORDE: &str = "Name:Test Horde\nManaCost:G\nTypes:Sorcery\n\
    A:SP$ Token | TokenScript$ g_3_3_test_beast | TokenAmount$ 10001 | SpellDescription$ Create 10,001 3/3 green Beast creature tokens.\n\
    Oracle:Create 10,001 3/3 green Beast creature tokens.";

// The engine's limit (`MAX_TOKENS_AT_ONCE`, approved by the user): when it cuts a count, an
// event says so.
#[test]
fn the_token_limit_is_said() {
    let mut sc = scenario(&[HORDE]);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Horde");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Horde").pay().pass_all();
    assert!(
        s.events.iter().any(|e| matches!(e,
        Event::TokensCapped { controller, wanted: 10_001, made: 10_000, .. } if *controller == P0))
    );
}

// Invariant 10: every event of a random four-player game serialises and comes
// back the same (externally tagged).
#[test]
fn events_round_trip() {
    let setup = Setup::parse("m2-4p").expect("m2");
    let rec = harness::play_random(
        3,
        harness::PlayOptions {
            setup,
            ..Default::default()
        },
    )
    .expect("game");
    let json = serde_json::to_string(&rec.events).unwrap();
    let back: Vec<Event> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rec.events);
    assert!(
        rec.events
            .iter()
            .any(|e| matches!(e, Event::ResolutionBegan { .. }))
    );
}

const LEAK: &str = "Name:Test Leak\nManaCost:U\nTypes:Instant\n\
    A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | UnlessCost$ 3 | UnlessPayer$ TargetedController | SpellDescription$ Counter target spell unless its controller pays {3}.\n\
    Oracle:Counter target spell unless its controller pays {3}.";

// CR 118.12: whether "unless … pays" was paid is said (here: it can't be).
#[test]
fn an_unpaid_unless() {
    let mut sc = scenario(&[THINK, LEAK]);
    sc.card(P0, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Hand, "Test Think Again");
    sc.card(P1, Zone::Battlefield, "Island");
    sc.card(P1, Zone::Hand, "Test Leak");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Think Again").pay().pass();
    s.cast("Test Leak")
        .target("Test Think Again")
        .pay()
        .pass_all();
    assert!(s.events.iter().any(|e| matches!(e,
        Event::UnlessPaid { player, paid: false, .. } if *player == P0)));
}
