//! The game log (`harness::text::Narrator`) lets a reader who knows Magic check a game from
//! the text alone, and shows a player nothing that player could not know (decisions.md
//! D-014). Invented cards in Forge's script format, played with `Script`; the events are
//! run through a Narrator built from the state the game started from.

use engine_core::events::Event;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{GameConfig, GameState, ObjectId, OptionDesc, PlayerId};
use harness::script::Script;
use harness::text::Narrator;
use harness::{forge_test_catalog, m0_game};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

/// A game ready to play, and the state it started from (for the narrator).
fn game(sc: Scenario) -> (Script, GameState) {
    let st = sc.build();
    (Script::new(st.clone()), st)
}

fn log(start: &GameState, s: &Script, viewer: Option<PlayerId>) -> Vec<String> {
    Narrator::new(start).lines(&s.events, viewer)
}

fn pos(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle:?} in:\n{}", lines.join("\n")))
}

fn indent(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

const ZAP: &str = "Name:Test Zapping\nManaCost:R\nTypes:Instant\n\
    A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 3 | SpellDescription$ Text.\nOracle:Text.";
const BANISH: &str = "Name:Test Banishing\nManaCost:W\nTypes:Instant\n\
    A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Exile | SpellDescription$ Text.\nOracle:Text.";
const UNSUMMON: &str = "Name:Test Unsummoning\nManaCost:U\nTypes:Instant\n\
    A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Hand | SpellDescription$ Text.\nOracle:Text.";
const MILL: &str = "Name:Test Milling\nManaCost:U\nTypes:Sorcery\n\
    A:SP$ Mill | NumCards$ 2 | Defined$ You | SpellDescription$ Text.\nOracle:Text.";
const SCRIBE: &str = "Name:Test Scribe\nManaCost:1\nTypes:Artifact\n\
    A:AB$ Draw | Cost$ 1 | NumCards$ 1 | SpellDescription$ Text.\nOracle:Text.";
const BLADE: &str = "Name:Test Blade\nManaCost:1\nTypes:Artifact Equipment\nK:Equip:1\n\
    S:Mode$ Continuous | AffectedDefined$ Equipped | Affected$ Creature | AddPower$ 2 | AddToughness$ 2 | Description$ Text.\nOracle:Text.";
const HEAL: &str = "Name:Test Healing\nManaCost:W\nTypes:Instant\n\
    A:SP$ GainLife | LifeAmount$ 3 | SpellDescription$ Text.\nOracle:Text.";
const GROWER: &str = "Name:Test Grower\nManaCost:1\nTypes:Artifact\n\
    A:AB$ PutCounter | Cost$ 1 | ValidTgts$ Creature | CounterType$ P1P1 | CounterNum$ 1 | SpellDescription$ Text.\nOracle:Text.";
/// An enters trigger that needs a creature an opponent controls.
const BULLY: &str = "Name:Test Bullying Ogre\nManaCost:R\nTypes:Creature Ogre\nPT:1/1\n\
    T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigDestroy | TriggerDescription$ Text.\n\
    SVar:TrigDestroy:DB$ Destroy | ValidTgts$ Creature.OppCtrl\nOracle:Text.";
const CYCLER: &str =
    "Name:Test Dune Beast\nManaCost:5 R\nTypes:Creature Beast\nPT:4/4\nK:Cycling:2\nOracle:Text.";
const STEPPE: &str = "Name:Test Steppe\nManaCost:no cost\nTypes:Land Plains\nOracle:Text.";
const LANDCYCLER: &str = "Name:Test Dune Strider\nManaCost:5 W\nTypes:Creature Camel\nPT:4/4\nK:TypeCycling:Plains:2\nOracle:Text.";

// Problem 1: no targets chosen prints a line that says so, not a bare "targeting ".
#[test]
fn zero_targets_say_so() {
    let mut sc = scenario(&[]);
    let hand = sc.card(P0, Zone::Hand, "Test Elf");
    let st = sc.build();
    let ev = [Event::TargetsChosen {
        object: hand,
        targets: vec![],
    }];
    let lines = Narrator::new(&st).lines(&ev, None);
    assert_eq!(lines, ["  with no targets"]);
}

// Problem 2 (CR 601.2c): the engine emits TargetsChosen before SpellCast; the log shows the
// cast first, then its targets and payment.
#[test]
fn targets_and_payment_follow_the_cast_line() {
    let mut sc = scenario(&[ZAP]);
    sc.cards(P0, Zone::Battlefield, &["Mountain"]);
    sc.card(P0, Zone::Hand, "Test Zapping");
    let (mut s, st) = game(sc);
    s.cast("Test Zapping").target_player(P1).pay().pass_all();
    let lines = log(&st, &s, None);
    let cast = pos(&lines, "P0 casts Test Zapping");
    assert!(pos(&lines, "targeting P1") > cast, "{lines:#?}");
    assert!(pos(&lines, "(paid 1: Mountain") > cast, "{lines:#?}");
    // ... and before the resolution.
    assert!(pos(&lines, "Test Zapping (P0) resolves") > pos(&lines, "targeting P1"));
    // Two spells in a row: each target line sits under its own cast.
    let mut sc = scenario(&[ZAP]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Zapping", "Test Zapping"]);
    let (mut s, st) = game(sc);
    s.cast("Test Zapping").target_player(P1).pay();
    s.cast("Test Zapping").target_player(P0).pay();
    let lines = log(&st, &s, None);
    let casts: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains("casts Test Zapping"))
        .collect();
    assert_eq!(casts.len(), 2);
    assert!(lines[casts[0] + 1].contains("targeting P1"), "{lines:#?}");
    assert!(lines[casts[1] + 1].contains("targeting P0"), "{lines:#?}");
}

// Problem 3 (CR 608.2): "X resolves" comes first and what it does is indented under it,
// for an ability and for a spell whose effects the engine emits before its `Resolved`.
#[test]
fn effects_are_nested_under_their_resolution_line() {
    let mut sc = scenario(&[SCRIBE, ZAP]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Mountain", "Mountain", "Test Scribe"],
    );
    sc.card(P0, Zone::Hand, "Test Zapping");
    let (mut s, st) = game(sc);
    s.activate("Test Scribe").pay().pass_all();
    s.cast("Test Zapping").target_player(P1).pay().pass_all();
    let lines = log(&st, &s, None);
    let res = pos(&lines, "ability of Test Scribe (P0) resolves");
    let draw = lines
        .iter()
        .rposition(|l| l.contains("P0 draws"))
        .expect("draw");
    assert!(draw > res, "{lines:#?}");
    assert!(indent(&lines[draw]) > indent(&lines[res]));
    let res = pos(&lines, "Test Zapping (P0) resolves");
    let dmg = pos(&lines, "Test Zapping (P0) deals 3 damage to P1");
    assert!(
        dmg > res && indent(&lines[dmg]) > indent(&lines[res]),
        "{lines:#?}"
    );
}

// Problem 4 (CR 603.3d): a triggered ability is "triggers" / "puts trigger ... on the
// stack", never "activates"; with no legal target it is removed and the log says so.
#[test]
fn triggers_are_not_activations_and_no_target_is_reported() {
    let mut sc = scenario(&[BULLY, SCRIBE]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Mountain", "Mountain", "Test Scribe"],
    );
    sc.card(P0, Zone::Hand, "Test Bullying Ogre");
    let (mut s, st) = game(sc);
    s.cast("Test Bullying Ogre").pay().pass_all();
    let lines = log(&st, &s, None);
    assert!(!lines.iter().any(|l| l.contains("activates")), "{lines:#?}");
    let trig = pos(&lines, "Test Bullying Ogre (P0) triggers");
    let put = pos(&lines, "puts trigger of Test Bullying Ogre (P0)");
    let gone = pos(&lines, "is removed from the stack: no legal target");
    assert!(trig < put && put < gone, "{lines:#?}");
    // An activated ability of a permanent that has a trigger is still an activation.
    s.activate("Test Scribe").pay().pass_all();
    let lines = log(&st, &s, None);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("P0 activates ability of Test Scribe"))
    );
}

// Problem 4/9: a trigger that does resolve is told apart by a short tag of its text.
#[test]
fn trigger_lines_carry_a_tag_of_the_ability_text() {
    let mut sc = scenario(&[BULLY]);
    sc.cards(P0, Zone::Battlefield, &["Mountain"]);
    sc.card(P0, Zone::Hand, "Test Bullying Ogre");
    sc.card(P1, Zone::Battlefield, "Test Golem");
    let (mut s, st) = game(sc);
    s.cast("Test Bullying Ogre").pay().pass_all();
    s.target("Test Golem").pass_all();
    let lines = log(&st, &s, None);
    let put = lines
        .iter()
        .find(|l| l.contains("puts trigger of Test Bullying Ogre"))
        .unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(put.contains('['), "{put}");
    for l in &lines {
        assert!(l.chars().count() < 120, "too long: {l}");
    }
    assert!(
        lines
            .iter()
            .any(|l| l.contains("trigger of Test Bullying Ogre") && l.ends_with("resolves"))
    );
}

// Problem 5: cycling reveals nothing; the log says "activates ability ... from hand" and
// "cycles", not "reveals".
#[test]
fn cycling_is_not_a_reveal() {
    let mut sc = scenario(&[CYCLER]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    let beast = sc.card(P0, Zone::Hand, "Test Dune Beast");
    let (mut s, st) = game(sc);
    s.pick(
        "cycle",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == beast),
    )
    .pay()
    .pass_all();
    let lines = log(&st, &s, None);
    assert!(!lines.iter().any(|l| l.contains("reveals")), "{lines:#?}");
    let act = pos(
        &lines,
        "P0 activates ability of Test Dune Beast (P0) from hand",
    );
    let cyc = pos(&lines, "P0 cycles Test Dune Beast");
    assert!(act < cyc, "{lines:#?}");
    assert!(!lines.iter().any(|l| l.contains("discards")), "{lines:#?}");
}

// Problem 6: exile, bounce, mill, and returning a commander to the command zone.
#[test]
fn zone_moves_are_narrated() {
    let mut sc = scenario(&[BANISH, UNSUMMON, MILL]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Island", "Island"]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Banishing", "Test Unsummoning", "Test Milling"],
    );
    sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P1, Zone::Battlefield, "Test Elf");
    let (mut s, st) = game(sc);
    s.cast("Test Banishing")
        .target("Test Golem")
        .pay()
        .pass_all();
    s.cast("Test Unsummoning")
        .target("Test Elf")
        .pay()
        .pass_all();
    s.cast("Test Milling").pay().pass_all();
    let lines = log(&st, &s, None);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("Test Golem (P1) is exiled")),
        "{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("Test Elf (P1) is returned to hand")),
        "{lines:#?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("is put into the graveyard from the library"))
            .count(),
        2,
        "{lines:#?}"
    );
}

#[test]
fn a_commander_going_to_the_command_zone_is_narrated() {
    // Synthetic: the engine's move from the graveyard to the command zone.
    let mut sc = scenario(&[]);
    let elf = sc.card(P0, Zone::Graveyard, "Test Elf");
    let st = sc.build();
    let ev = [Event::ZoneChange {
        old: elf,
        new: ObjectId(900),
        card_instance: engine_core::CardInstanceId(1),
        from: Zone::Graveyard,
        to: Zone::Command,
    }];
    let lines = Narrator::new(&st).lines(&ev, None);
    assert_eq!(lines, ["  Test Elf (P0) is put into the command zone"]);
}

// Problem 6, hidden information: library to hand (not a draw) and hand to library name the
// card only to its owner (and to the omniscient narrator).
#[test]
fn hidden_zone_moves_are_named_only_to_who_may_know() {
    let mut sc = scenario(&[]);
    let top = sc.card(P0, Zone::Library, "Test Elf");
    let held = sc.card(P0, Zone::Hand, "Test Golem");
    let st = sc.build();
    let up = Event::ZoneChange {
        old: top,
        new: ObjectId(900),
        card_instance: engine_core::CardInstanceId(1),
        from: Zone::Library,
        to: Zone::Hand,
    };
    let down = Event::ZoneChange {
        old: held,
        new: ObjectId(901),
        card_instance: engine_core::CardInstanceId(2),
        from: Zone::Hand,
        to: Zone::Library,
    };
    let ev = [up, down];
    let omniscient = Narrator::new(&st).lines(&ev, None);
    let owner = Narrator::new(&st).lines(&ev, Some(P0));
    let other = Narrator::new(&st).lines(&ev, Some(P1));
    assert_eq!(omniscient, owner);
    assert_eq!(
        owner,
        [
            "  P0 puts Test Elf (P0) from the library into hand",
            "  P0 puts Test Golem (P0) from hand into the library"
        ]
    );
    assert_eq!(
        other,
        [
            "  P0 puts a card from the library into hand",
            "  P0 puts a card from hand into the library"
        ]
    );
    for l in &other {
        assert!(!l.contains("Test"), "{l}");
    }
}

// Problem 6, hidden information, played for real: searching with landcycling.
#[test]
fn a_search_to_hand_hides_the_card_from_opponents() {
    let mut sc = scenario(&[LANDCYCLER, STEPPE]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Dune Strider");
    sc.card(P0, Zone::Library, "Plains");
    let (mut s, st) = game(sc);
    s.activate("Test Dune Strider").pay().pass_all();
    // A search asks which card to take.
    assert_eq!(s.kind(), engine_core::ChoiceKind::SearchLibrary);
    s.pick(
        "the basic Plains",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Plains"),
    );
    let mine = log(&st, &s, Some(P0));
    let theirs = log(&st, &s, Some(P1));
    assert!(
        mine.iter()
            .any(|l| l.contains("puts Plains (P0) from the library into hand")),
        "{mine:#?}"
    );
    assert!(
        theirs
            .iter()
            .any(|l| l.contains("puts a card from the library into hand")),
        "{theirs:#?}"
    );
    assert!(
        !theirs
            .iter()
            .any(|l| l.contains("Plains (P0) from the library")),
        "{theirs:#?}"
    );
}

// Problem 7: counters, attaching.
#[test]
fn counters_and_attaching_are_narrated() {
    let mut sc = scenario(&[GROWER, BLADE]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Forest", "Test Grower", "Test Blade"],
    );
    sc.card(P0, Zone::Battlefield, "Test Elf");
    let (mut s, st) = game(sc);
    s.activate("Test Grower")
        .target("Test Elf")
        .pay()
        .pass_all();
    s.activate("Test Blade").target("Test Elf").pay().pass_all();
    let lines = log(&st, &s, None);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("Test Elf (P0) gets 1 +1/+1 counter")),
        "{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("Test Blade (P0) is now attached to Test Elf (P0)")),
        "{lines:#?}"
    );
}

// Problem 7: a permanent tapped by a cost or an effect is told; tapping for mana and
// attacking are folded into the payment line / the attack line.
#[test]
fn tapping_for_mana_is_folded_into_the_payment() {
    let mut sc = scenario(&[ZAP]);
    sc.cards(P0, Zone::Battlefield, &["Mountain"]);
    sc.card(P0, Zone::Hand, "Test Zapping");
    let (mut s, st) = game(sc);
    s.cast("Test Zapping").target_player(P1).pay();
    let lines = log(&st, &s, None);
    assert!(!lines.iter().any(|l| l.contains("taps")), "{lines:#?}");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("(paid 1: Mountain -> {R})")),
        "{lines:#?}"
    );
}

// Problem 8: life changes say how much and the new total; damage to a player and the life
// it cost are one line.
#[test]
fn life_changes_carry_amount_and_total() {
    let mut sc = scenario(&[ZAP, HEAL]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Plains"]);
    sc.cards(P0, Zone::Hand, &["Test Zapping", "Test Healing"]);
    let (mut s, st) = game(sc);
    s.cast("Test Zapping").target_player(P1).pay().pass_all();
    s.cast("Test Healing").pay().pass_all();
    let lines = log(&st, &s, None);
    let dmg = lines
        .iter()
        .find(|l| l.contains("deals 3 damage to P1"))
        .unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(dmg.ends_with("(P1 at 37)"), "{dmg}");
    assert!(!lines.iter().any(|l| l.contains("is at")), "{lines:#?}");
    assert!(
        lines.iter().any(|l| l.trim() == "P0 gains 3 (43)"),
        "{lines:#?}"
    );
}

// Problem 9: lines stay short.
#[test]
fn lines_are_short_in_real_games() {
    for seed in 0..10 {
        let rec = harness::play_random(seed, harness::PlayOptions::default()).unwrap();
        let lines = Narrator::new(&m0_game(seed)).lines(&rec.events, None);
        for l in lines {
            if !l.contains(" draws ") {
                assert!(l.chars().count() < 120, "seed {seed}: {l}");
            }
        }
    }
}

// Attackers are tapped by declaring them: "attacks" says it, no "taps" line.
#[test]
fn attacking_does_not_also_tap() {
    let mut sc = scenario(&[]);
    let a = sc.card(P0, Zone::Battlefield, "Test Elf");
    let b = sc.card(P0, Zone::Battlefield, "Test Golem");
    let st = sc.build();
    let ev = [
        Event::StepBegan {
            step: engine_core::turn::Step::DeclareAttackers,
        },
        Event::Tapped { object: a },
        Event::Tapped { object: b },
        Event::AttackerDeclared {
            attacker: a,
            defender: P1,
        },
        Event::AttackerDeclared {
            attacker: b,
            defender: P1,
        },
    ];
    let lines = Narrator::new(&st).lines(&ev, None);
    assert_eq!(
        lines,
        ["  Test Elf (P0) attacks P1", "  Test Golem (P0) attacks P1"]
    );
}

// Every nested effect sits under a resolution line, and draws are still hidden.
#[test]
fn drawing_stays_hidden_inside_a_resolution() {
    let mut sc = scenario(&[SCRIBE]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Test Scribe"]);
    let (mut s, st) = game(sc);
    s.activate("Test Scribe").pay().pass_all();
    let theirs = log(&st, &s, Some(P1));
    assert!(
        theirs.iter().any(|l| l.trim() == "P0 draws a card"),
        "{theirs:#?}"
    );
    let mine = log(&st, &s, Some(P0));
    assert!(
        mine.iter().any(|l| l.trim().starts_with("P0 draws Forest")),
        "{mine:#?}"
    );
}
