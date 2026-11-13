//! "Becomes the target" and ward (decisions.md D-089). Invented cards in Forge's script
//! format, compiled the way real ones are and then played.
//! Rules text: docs/rules-notes/d089-ward-dig-until.md.

use engine_core::stack::Target;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, Event, GameConfig, OptionDesc, PlayerId, Primitive, Response};
use harness::forge_test_catalog;
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn card(name: &str, cost: &str, types: &str, body: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{types}\n{body}\nOracle:Text.")
}

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc.turn(P1, Step::PrecombatMain);
    sc
}

fn drive_until(s: &mut Script, done: impl Fn(&Script) -> bool) {
    for _ in 0..5_000 {
        if s.next.is_none() || done(s) {
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
    panic!("drive_until did not converge");
}

/// Pass priority until the ward payment is asked (or nothing is left on the stack).
fn to_ward(s: &mut Script) {
    drive_until(s, |s| {
        matches!(s.kind(), ChoiceKind::PayToSave { .. })
            || (s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority)
    });
}

fn settle(s: &mut Script) {
    drive_until(s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
}

fn warded() -> String {
    "Name:Test Warded Bear\nManaCost:1 G\nTypes:Creature Bear\nPT:2/2\nK:Ward:2\n\
     Oracle:Ward {2} (Whenever this creature becomes the target of a spell or ability an opponent controls, counter it unless that player pays {2}.)"
        .to_string()
}

fn shock() -> String {
    card(
        "Test Jolt",
        "R",
        "Instant",
        "A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 2 | SpellDescription$ Text.",
    )
}

// CR 702.21a (2026-11-06): "Whenever this permanent becomes the target of a spell or ability
// an opponent controls, counter that spell or ability unless that player pays [cost]." The
// decision states the cost and what is at stake; the events say whether it was paid and what
// was countered. Its controller's own spells don't trigger it.
#[test]
fn ward_counters_a_spell_unless_its_controller_pays() {
    let mut sc = scenario(&[&warded(), &shock()]);
    let bear = sc.card(P0, Zone::Battlefield, "Test Warded Bear");
    sc.cards(P1, Zone::Battlefield, &["Mountain"; 5]);
    sc.cards(P1, Zone::Hand, &["Test Jolt", "Test Jolt"]);
    let st0 = sc.build();
    let mut s = Script::new(st0.clone());
    s.cast("Test Jolt").target_object(bear).pay();
    let jolt = s.st.stack[0].id;
    // The ward trigger is on the stack, P0's, about the Jolt.
    let view = engine_core::player_view(&s.st, P1);
    let top = view.stack.last().unwrap();
    assert_eq!((top.controller, top.ability_of), (P0, Some(bear)));
    assert_eq!(top.about, Some(jolt), "the spell it would counter");
    to_ward(&mut s);
    assert_eq!(s.kind(), ChoiceKind::PayToSave { at_stake: jolt });
    assert_eq!(s.who(), P1);
    assert!(matches!(
        &s.options()[0],
        OptionDesc::PayCost { cost } if card_model::describe::cost_text(cost) == "{2}"
    ));
    s.pick("don't pay", |_, o| *o == OptionDesc::Decline);
    assert_eq!(
        s.count(
            |e| matches!(e, Event::PaidToSave { player, at_stake, paid: false }
            if *player == P1 && *at_stake == jolt)
        ),
        1
    );
    assert_eq!(
        s.count(|e| matches!(e, Event::Countered { object } if *object == jolt)),
        1
    );
    settle(&mut s);
    assert!(s.st.objects.contains_key(&bear), "the Jolt was countered");
    // What the other seat (P0) is told: P1 didn't pay, the Jolt was countered.
    let mut view = engine_core::client::EventView::new(&st0, engine_core::client::Seat::Player(P0));
    let seen = view.redact(&s.events);
    assert!(seen.iter().any(
        |e| matches!(e, Event::PaidToSave { player, at_stake, paid: false }
        if *player == P1 && *at_stake == jolt)
    ));
    assert!(
        seen.iter()
            .any(|e| matches!(e, Event::Countered { object } if *object == jolt))
    );
    // Paying: the spell resolves.
    s.cast("Test Jolt").target_object(bear).pay();
    to_ward(&mut s);
    s.pick("pay", |_, o| matches!(o, OptionDesc::PayCost { .. }));
    settle(&mut s);
    assert!(!s.st.objects.contains_key(&bear), "the Jolt resolved");
}

// CR 603.2c: one trigger for a spell that targets the permanent with two target words; and
// an ability (not only a spell) is countered too (CR 701.6a: removed from the stack).
#[test]
fn ward_triggers_once_per_spell_and_counters_abilities() {
    let twin = card(
        "Test Double Jolt",
        "R",
        "Instant",
        "A:SP$ DealDamage | ValidTgts$ Creature | NumDmg$ 1 | SubAbility$ DBTwo | SpellDescription$ Text.\n\
         SVar:DBTwo:DB$ DealDamage | ValidTgts$ Creature | NumDmg$ 1",
    );
    let pinger = card(
        "Test Spark Imp",
        "R",
        "Creature Goblin",
        "PT:1/1\nA:AB$ DealDamage | Cost$ T | ValidTgts$ Creature | NumDmg$ 1 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&warded(), &twin, &pinger]);
    let bear = sc.card(P0, Zone::Battlefield, "Test Warded Bear");
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Battlefield, "Test Spark Imp");
    sc.card(P1, Zone::Hand, "Test Double Jolt");
    let mut s = Script::new(sc.build());
    s.cast("Test Double Jolt")
        .target_object(bear)
        .target_object(bear)
        .pay();
    let wards = s
        .st
        .stack
        .iter()
        .filter(|so| matches!(so.kind, engine_core::stack::StackKind::Ability { source, .. } if source == bear))
        .count()
        + s.st.pending_triggers.len();
    assert_eq!(wards, 1, "one trigger for one spell");
    // P1 can't pay {2} (no mana left): countered without a question.
    settle(&mut s);
    assert!(s.st.objects.contains_key(&bear));
    // An activated ability is countered too.
    s.activate("Test Spark Imp").target_object(bear);
    let ping = s.st.stack[0].id;
    settle(&mut s);
    assert!(s.count(|e| matches!(e, Event::Countered { object } if *object == ping)) == 1);
    assert_eq!(s.st.object(bear).damage_marked, 0);
}

// CR 115.7 / 707.10c: a target changed to the permanent, and a copy put on the stack
// targeting it, "become" targets: ward triggers. A target kept when new targets are chosen
// doesn't become one again.
#[test]
fn ward_sees_changed_targets_and_copies() {
    let reroute = card(
        "Test Rerouting",
        "U",
        "Instant",
        "A:SP$ ChangeTargets | TargetType$ Spell | Optional$ True | ValidTgts$ Card | SpellDescription$ Text.",
    )
    .replace(
        "Oracle:Text.",
        "Oracle:You may choose new targets for target spell.",
    );
    let echo = card(
        "Test Echoing",
        "U",
        "Instant",
        "A:SP$ CopySpellAbility | ValidTgts$ Instant,Sorcery | TargetType$ Spell | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&warded(), &shock(), &reroute, &echo]);
    let bear = sc.card(P0, Zone::Battlefield, "Test Warded Bear");
    sc.cards(
        P1,
        Zone::Battlefield,
        &["Mountain", "Island", "Island", "Island"],
    );
    sc.cards(
        P1,
        Zone::Hand,
        &["Test Jolt", "Test Rerouting", "Test Echoing"],
    );
    let mut s = Script::new(sc.build());
    // The Jolt at P0; rerouted to the bear: ward triggers.
    s.cast("Test Jolt").target_player(P0).pay();
    let jolt = s.st.stack[0].id;
    s.cast("Test Rerouting").target_object(jolt).pay();
    drive_until(&mut s, |s| matches!(s.kind(), ChoiceKind::NewTarget { .. }));
    s.target_object(bear);
    to_ward(&mut s);
    assert_eq!(s.kind(), ChoiceKind::PayToSave { at_stake: jolt });
    s.pick("don't pay", |_, o| *o == OptionDesc::Decline);
    settle(&mut s);
    assert!(s.st.objects.contains_key(&bear));
    assert_eq!(s.st.player(P0).life, 40);
    // A copy of a spell targeting the bear (P1 copies P0's own Jolt? no: P1's): the copy
    // becomes a spell targeting it as it is put on the stack.
    let mut sc = scenario(&[&warded(), &shock(), &echo]);
    let bear = sc.card(P0, Zone::Battlefield, "Test Warded Bear");
    sc.cards(
        P1,
        Zone::Battlefield,
        &["Mountain", "Island", "Island", "Island"],
    );
    sc.cards(P1, Zone::Hand, &["Test Jolt", "Test Echoing"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Jolt").target_object(bear).pay();
    // Respond to the ward trigger with the copy (P1 holds priority).
    s.cast("Test Echoing")
        .pick("the Jolt", |s, o| {
            matches!(o, OptionDesc::Target { target: Target::Object(id) }
                if s.st.stack_object(*id).is_some_and(|so| so.kind == engine_core::stack::StackKind::Spell))
        })
        .pay();
    to_ward(&mut s);
    // Two ward payments asked: the copy's first (its trigger is on top), then the Jolt's.
    let mut asked = 0;
    while matches!(s.kind(), ChoiceKind::PayToSave { .. }) {
        asked += 1;
        s.pick("don't pay", |_, o| *o == OptionDesc::Decline);
        to_ward(&mut s);
    }
    assert_eq!(asked, 2);
    settle(&mut s);
    assert!(s.st.objects.contains_key(&bear));
}

// CR 702.21a with a cost that isn't mana: "Ward—Pay 3 life".
#[test]
fn ward_pay_life() {
    let warded = "Name:Test Life Ward\nManaCost:1 B\nTypes:Creature Horror\nPT:2/2\nK:Ward:PayLife<3>\n\
        Oracle:Ward—Pay 3 life. (Whenever this creature becomes the target of a spell or ability an opponent controls, counter it unless that player pays 3 life.)"
        .to_string();
    let mut sc = scenario(&[&warded, &shock()]);
    let horror = sc.card(P0, Zone::Battlefield, "Test Life Ward");
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Test Jolt");
    let mut s = Script::new(sc.build());
    s.cast("Test Jolt").target_object(horror).pay();
    to_ward(&mut s);
    assert!(matches!(
        &s.options()[0],
        OptionDesc::PayCost { cost } if cost.life == 3
    ));
    s.pick("pay", |_, o| matches!(o, OptionDesc::PayCost { .. }));
    settle(&mut s);
    assert_eq!(s.st.player(P1).life, 37);
    assert!(!s.st.objects.contains_key(&horror));
    let cat = forge_test_catalog(&[&warded, &self::warded()], &[]);
    let text = |n: &str| card_model::describe::rules_text(cat.get(cat.find(n).unwrap()));
    assert_eq!(text("Test Life Ward"), ["Ward—Pay 3 life"]);
    assert_eq!(text("Test Warded Bear"), ["Ward {2}"]);
}

// "When this creature becomes the target of a spell or ability, sacrifice it" (Forge:
// `Mode$ BecomesTarget | ValidTarget$ Card.Self`): the same event, from any player's spell.
#[test]
fn sacrifice_it_when_it_becomes_a_target() {
    let illusion = "Name:Test Phantasm\nManaCost:U\nTypes:Creature Illusion\nPT:3/3\n\
        T:Mode$ BecomesTarget | ValidTarget$ Card.Self | TriggerZones$ Battlefield | Execute$ TrigSac | TriggerDescription$ Text.\n\
        SVar:TrigSac:DB$ Sacrifice | Defined$ Self\n\
        Oracle:When this creature becomes the target of a spell or ability, sacrifice it."
        .to_string();
    let mut sc = scenario(&[&illusion, &shock()]);
    let ph = sc.card(P0, Zone::Battlefield, "Test Phantasm");
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Test Jolt");
    let mut s = Script::new(sc.build());
    s.cast("Test Jolt").target_object(ph).pay();
    settle(&mut s);
    assert!(s.find(P0, Zone::Graveyard, "Test Phantasm").is_some());
    assert_eq!(
        s.count(|e| matches!(e, Event::Countered { .. })),
        0,
        "the Jolt fizzled, it wasn't countered"
    );
}

// Text only: a trigger that only spells can make counters "that spell" (ward's own words,
// CR 702.21a, are "that spell or ability").
#[test]
fn a_spell_only_ward_says_that_spell() {
    let thorn = "Name:Test Spell Thorn\nManaCost:2 G\nTypes:Creature Plant\nPT:3/3\n\
        T:Mode$ BecomesTarget | ValidTarget$ Card.Self | ValidSource$ Spell.OppCtrl | TriggerZones$ Battlefield | Execute$ TrigCounter | TriggerDescription$ Text.\n\
        SVar:TrigCounter:DB$ Counter | Defined$ TriggeredSourceSA | UnlessCost$ Discard<1/Card> | UnlessPayer$ TriggeredSourceSAController\n\
        Oracle:Whenever this creature becomes the target of a spell an opponent controls, counter that spell unless its controller discards a card."
        .to_string();
    let cat = forge_test_catalog(&[&thorn], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Spell Thorn").unwrap()));
    assert!(text[0].contains("counter that spell unless"), "{text:?}");
    assert!(!text[0].contains("or ability"), "{text:?}");
}
