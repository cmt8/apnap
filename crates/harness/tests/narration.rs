//! The game log (`harness::text::Narrator`) lets a reader who knows Magic check a game from
//! the text alone, and shows a player nothing that player could not know (decisions.md
//! D-014). Invented cards in Forge's script format, played with `Script`; the events are
//! run through a Narrator built from the state the game started from.

use engine_core::client::Seat;
use engine_core::events::Event;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{GameConfig, GameState, ObjectId, OptionDesc, PlayerId};
use harness::script::Script;
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
    harness::narrate(
        start,
        &s.events,
        viewer.map_or(Seat::Everyone, Seat::Player),
    )
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
const PUPA: &str = "Name:Test Pupa\nManaCost:G\nTypes:Creature Insect\nPT:2/2\n\
    A:AB$ SetState | Cost$ 1 | Defined$ Self | Mode$ Transform | SpellDescription$ Transform CARDNAME.\n\
    AlternateMode:DoubleFaced\nOracle:Text.\n\nALTERNATE\n\nName:Test Moth\nManaCost:no cost\nColors:white\n\
    Types:Creature Insect\nPT:4/4\nK:Flying\nOracle:Flying";
const GEYSER: &str = "Name:Test Geyser\nManaCost:2\nTypes:Artifact\n\
    A:AB$ Mana | Cost$ T | Produced$ R | Amount$ 3 | SpellDescription$ Add RRR.\nOracle:Text.";
const MIND: &str = "Name:Test Upkeeper\nManaCost:1\nTypes:Artifact\n\
    A:AB$ Draw | Cost$ 1 | NumCards$ 1 | SpellDescription$ Text.\nOracle:Text.";
const LANDCYCLER: &str = "Name:Test Dune Strider\nManaCost:5 W\nTypes:Creature Camel\nPT:4/4\nK:TypeCycling:Plains:2\nOracle:Plainscycling {2} ({2}, Discard this card: Search your library for a Plains card, reveal it, put it into your hand, then shuffle.)";

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
    let lines = harness::narrate(&st, &ev, Seat::Everyone);
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
    assert!(
        pos(&lines, "(paid 1 from the pool: {R})") > cast,
        "{lines:#?}"
    );
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
        creature: false,
        face: 0,
        position: None,
    }];
    let lines = harness::narrate(&st, &ev, Seat::Everyone);
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
        creature: false,
        face: 0,
        position: None,
    };
    let down = Event::ZoneChange {
        old: held,
        new: ObjectId(901),
        card_instance: engine_core::CardInstanceId(2),
        from: Zone::Hand,
        to: Zone::Library,
        creature: false,
        face: 0,
        position: Some(engine_core::zones::LibraryPosition::Top),
    };
    let ev = [up, down];
    let omniscient = harness::narrate(&st, &ev, Seat::Everyone);
    let owner = harness::narrate(&st, &ev, Seat::Player(P0));
    let other = harness::narrate(&st, &ev, Seat::Player(P1));
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
    // Landcycling reveals the card it finds (CR 702.29e, 701.20a): everyone sees it go.
    assert!(
        theirs.iter().any(|l| l.contains("P0 reveals Plains (P0)"))
            && theirs
                .iter()
                .any(|l| l.contains("puts Plains (P0) from the library into hand"))
            && !theirs.iter().any(|l| l.contains("finds nothing")),
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
    // Mana is told when it is produced; the payment says what was spent from the pool.
    let tap = pos(&lines, "P0 taps Mountain for {R}");
    assert!(
        pos(&lines, "(paid 1 from the pool: {R})") > tap,
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
        let lines = harness::narrate(&m0_game(seed), &rec.events, Seat::Everyone);
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
        Event::Tapped {
            object: a,
            cause: engine_core::events::TapCause::Attack,
        },
        Event::Tapped {
            object: b,
            cause: engine_core::events::TapCause::Attack,
        },
        Event::AttackerDeclared {
            attacker: a,
            defender: P1,
        },
        Event::AttackerDeclared {
            attacker: b,
            defender: P1,
        },
    ];
    let lines = harness::narrate(&st, &ev, Seat::Everyone);
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

// Defect 1: a draw by an upkeep ability and the draw step's draw are two lines, the first
// nested under its resolution, the second at the outer level.
#[test]
fn an_upkeep_draw_and_the_draw_step_draw_are_separate_lines() {
    let mut sc = scenario(&[MIND]);
    sc.cards(P1, Zone::Battlefield, &["Forest", "Test Upkeeper"]);
    let (mut s, st) = game(sc);
    // Play on to P1's upkeep.
    let mut guard = 0;
    while !(s.st.turn.step == engine_core::turn::Step::Upkeep
        && s.st.turn.active == P1
        && s.kind() == engine_core::ChoiceKind::Priority)
    {
        s.pass_all();
        guard += 1;
        assert!(guard < 400);
    }
    assert_eq!(s.who(), P1);
    s.activate("Test Upkeeper").pay().pass_all();
    s.pass_all();
    let lines = log(&st, &s, None);
    let res = lines
        .iter()
        .rposition(|l| l.contains("ability of Test Upkeeper (P1) resolves"))
        .expect("resolves");
    let mut draws = lines[res..].iter().filter(|l| l.contains("P1 draws"));
    let first = draws.next().expect("ability draw");
    let second = draws.next().unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(indent(first) > indent(second), "{lines:#?}");
    assert!(!first.contains(", "), "one card each: {first}");
}

// Defect 2: nesting ends when the resolving object is done.
#[test]
fn nesting_ends_after_a_trigger_resolves() {
    let mut sc = scenario(&[BULLY, ZAP]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Bullying Ogre", "Test Zapping"]);
    sc.card(P1, Zone::Battlefield, "Test Golem");
    let (mut s, st) = game(sc);
    s.cast("Test Bullying Ogre").pay().pass_all();
    s.target("Test Golem").pass_all();
    s.cast("Test Zapping").target_player(P1).pay();
    let lines = log(&st, &s, None);
    let c = pos(&lines, "P0 casts Test Zapping");
    assert_eq!(indent(&lines[c]), 0, "{lines:#?}");
}

// Defect 4: mana that spells and abilities add is told, grouped.
#[test]
fn mana_added_by_an_ability_is_narrated() {
    let mut sc = scenario(&[GEYSER]);
    sc.card(P0, Zone::Battlefield, "Test Geyser");
    let (mut s, st) = game(sc);
    s.tap("Test Geyser", engine_core::mana::ManaType::Red);
    let lines = log(&st, &s, None);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("P0 taps Test Geyser for {R}{R}{R}")),
        "{lines:#?}"
    );
}

// Defect 6: a search that takes nothing says so; one that takes a card does not.
#[test]
fn a_failed_search_finds_nothing() {
    let mut sc = scenario(&[LANDCYCLER, STEPPE]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Dune Strider");
    sc.card(P0, Zone::Library, "Plains");
    let (mut s, st) = game(sc);
    s.activate("Test Dune Strider").pay().pass_all();
    s.respond(engine_core::Response::Pick(vec![]));
    s.pass_all();
    let lines = log(&st, &s, None);
    let l = pos(&lines, "searches their library");
    assert!(pos(&lines, "P0 finds nothing") > l, "{lines:#?}");

    let mut sc = scenario(&[LANDCYCLER, STEPPE]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Dune Strider");
    sc.card(P0, Zone::Library, "Plains");
    let (mut s, st) = game(sc);
    s.activate("Test Dune Strider").pay().pass_all();
    s.pick(
        "the basic Plains",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Plains"),
    );
    s.pass_all();
    let lines = log(&st, &s, None);
    assert!(
        !lines.iter().any(|l| l.contains("finds nothing")),
        "{lines:#?}"
    );
}

// Defect 7: a permanent that attacked or blocked was a creature when it died.
#[test]
fn something_that_fought_dies() {
    let mut sc = scenario(&[SCRIBE]);
    let id = sc.card(P0, Zone::Battlefield, "Test Scribe");
    let st = sc.build();
    let ev = [
        Event::AttackerDeclared {
            attacker: id,
            defender: P1,
        },
        Event::ZoneChange {
            old: id,
            new: ObjectId(900),
            card_instance: engine_core::CardInstanceId(1),
            from: Zone::Battlefield,
            to: Zone::Graveyard,
            creature: false,
            face: 0,
            position: None,
        },
    ];
    let lines = harness::narrate(&st, &ev, Seat::Everyone);
    assert!(lines[1].ends_with("dies"), "{lines:#?}");
}

// Defect 8: headings for combat, the second main phase, and upkeep/end only if used.
#[test]
fn step_headings_appear_when_something_happens() {
    use engine_core::turn::Step;
    let mut sc = scenario(&[]);
    let a = sc.card(P0, Zone::Battlefield, "Test Elf");
    let st = sc.build();
    let ev = [
        Event::StepBegan { step: Step::Upkeep },
        Event::StepBegan { step: Step::Draw },
        Event::StepBegan {
            step: Step::BeginningOfCombat,
        },
        Event::StepBegan {
            step: Step::DeclareAttackers,
        },
        Event::AttackerDeclared {
            attacker: a,
            defender: P1,
        },
        Event::StepBegan {
            step: Step::PostcombatMain,
        },
        Event::LifeChanged {
            player: P0,
            delta: 1,
            life: 41,
        },
        Event::StepBegan { step: Step::End },
        Event::StepBegan {
            step: Step::Cleanup,
        },
    ];
    let lines = harness::narrate(&st, &ev, Seat::Everyone);
    assert_eq!(
        lines,
        [
            "  -- combat --",
            "  Test Elf (P0) attacks P1",
            "  -- second main --",
            "  P0 gains 1 (41)"
        ]
    );
}

// Defect 3: a transformed permanent is named by the face that is up.
#[test]
fn a_transformed_permanent_is_named_by_its_face() {
    let mut sc = scenario(&[PUPA]);
    let pupa = sc.card(P0, Zone::Battlefield, "Test Pupa");
    let st = sc.build();
    let ev = [
        Event::Transformed {
            object: pupa,
            face: 1,
        },
        Event::AttackerDeclared {
            attacker: pupa,
            defender: P1,
        },
    ];
    let lines = harness::narrate(&st, &ev, Seat::Everyone);
    assert!(lines[1].contains("Test Moth (P0) attacks P1"), "{lines:#?}");
}

// CR 701.20 (D-089 A.1): a card found or looked at is revealed only where the card says so.
// "Look at the top six cards … put up to two creature cards … onto the battlefield" and
// "search your library for up to three creature cards, put them into your graveyard": the
// cards become public by arriving; nothing is revealed on the way, and another seat's log
// shows no reveal and none of the cards left behind.
#[test]
fn no_reveal_the_card_doesnt_make() {
    let company = "Name:Test Company\nManaCost:3 G\nTypes:Instant\n\
        A:SP$ Dig | DigNum$ 6 | ChangeNum$ 2 | Optional$ True | ChangeValid$ Creature.cmcLE3 | DestinationZone$ Battlefield | SpellDescription$ Text.\n\
        Oracle:Look at the top six cards of your library. Put up to two creature cards with mana value 3 or less from among them onto the battlefield. Put the rest on the bottom of your library in any order.";
    let buried = "Name:Test Burial\nManaCost:2 B\nTypes:Sorcery\n\
        A:SP$ ChangeZone | Origin$ Library | Destination$ Graveyard | ChangeType$ Creature | ChangeNum$ 3 | SpellDescription$ Text.\n\
        Oracle:Search your library for up to three creature cards, put them into your graveyard, then shuffle.";
    let mut sc = scenario(&[company, buried]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Forest", "Forest", "Forest"],
    );
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp", "Swamp"]);
    sc.cards(P0, Zone::Hand, &["Test Company", "Test Burial"]);
    sc.cards(P0, Zone::Library, &["Test Elf", "Test Elf", "Test Elf"]);
    let (mut s, st) = game(sc);
    s.cast("Test Company").pay().pass_all();
    s.respond(engine_core::Response::Pick(vec![0]));
    while s.kind() != engine_core::ChoiceKind::Priority {
        let n = s.options().len();
        s.respond(engine_core::Response::Order((0..n).collect()));
    }
    s.cast("Test Burial").pay().pass_all();
    s.respond(engine_core::Response::Pick(vec![0]));
    assert_eq!(
        s.count(|e| matches!(e, Event::Revealed { .. })),
        0,
        "nothing revealed"
    );
    let theirs = log(&st, &s, Some(P1));
    assert!(!theirs.iter().any(|l| l.contains("reveal")), "{theirs:#?}");
    // P1's redacted stream: no reveal at all.
    let mut view = engine_core::client::EventView::new(&st, Seat::Player(P1));
    let seen = view.redact(&s.events);
    assert!(!seen.iter().any(|e| matches!(e, Event::Revealed { .. })));
}
