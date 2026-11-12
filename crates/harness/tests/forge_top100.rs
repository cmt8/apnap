//! The last gaps in the EDHREC top 100 (decisions.md D-080): abilities that cost less to
//! activate, "becomes tapped" triggers, "an opponent's first … each turn". Invented cards in
//! Forge's script format, compiled the way real ones are and then played. Rules text:
//! docs/rules-notes/d080-top100.md.

use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{
    ChoiceKind, Event, GameConfig, ObjectId, OptionDesc, PlayerId, Primitive, Response,
};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

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
    sc.turn(P0, Step::PrecombatMain);
    sc
}

/// Answer everything with the least it allows until `done`.
fn drive_until(s: &mut Script, done: impl Fn(&Script) -> bool) {
    for _ in 0..5_000 {
        if s.next.is_none() || done(s) {
            return;
        }
        let r = match &s.decision().prim {
            Primitive::PickN { min, .. } => Response::Pick((0..*min).collect()),
            Primitive::PickNumber { min, .. } => Response::Number(*min),
            Primitive::Allocate { per_min, .. } => Response::Allocate(per_min.clone()),
            Primitive::Order => Response::Order((0..s.options().len()).collect()),
        };
        s.respond(r);
    }
    panic!("drive_until did not converge");
}

fn legend(name: &str) -> String {
    card(name, "1 W", "Legendary Creature Human Soldier\nPT:2/2", "")
}

/// A channel land: "{3}{U}, Discard this card: Return target creature to its owner's hand.
/// This ability costs {1} less to activate for each legendary creature you control."
fn channel() -> String {
    card(
        "Test Sky Shrine",
        "no cost",
        "Legendary Land",
        "A:AB$ Mana | Cost$ T | Produced$ U | SpellDescription$ Text.\n\
         A:AB$ ChangeZone | Cost$ 3 U Discard<1/CARDNAME> | ActivationZone$ Hand | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Hand | ReduceCost$ X | SpellDescription$ Text.\n\
         SVar:X:Count$Valid Creature.Legendary+YouCtrl",
    )
}

fn can_activate(s: &Script, name: &str) -> bool {
    s.has(|s, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if s.name(*source) == name))
}

// CR 602.2b (2026-11-06): activating an ability follows 601.2b–i; CR 601.2f: "The total cost
// is the mana cost …, plus all additional costs and cost increases, and minus all cost
// reductions." CR 118.7a: a reduction by generic mana affects only the generic component.
// Forge: `ReduceCost$ X` on an `AB$` (CostAdjustment: X less generic mana), with
// `ActivationZone$ Hand` and `Discard<1/CARDNAME>` (channel).
#[test]
fn an_ability_costs_one_less_for_each_legendary_creature() {
    // Two legendary creatures: {3}{U} costs {1}{U}; two Islands are enough.
    let mut sc = scenario(&[&channel(), &legend("Test Hero"), &legend("Test Heroine")]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    sc.cards(P0, Zone::Battlefield, &["Test Hero", "Test Heroine"]);
    let target = sc.card(P1, Zone::Battlefield, "Test Hero");
    sc.card(P0, Zone::Hand, "Test Sky Shrine");
    let mut s = Script::new(sc.build());
    assert!(can_activate(&s, "Test Sky Shrine"));
    s.activate("Test Sky Shrine").target_object(target).pay();
    assert!(
        s.find(P0, Zone::Graveyard, "Test Sky Shrine").is_some(),
        "discarded"
    );
    s.pass_all();
    assert!(s.find(P1, Zone::Hand, "Test Hero").is_some());
    let islands_tapped =
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&o| s.name(o) == "Island" && s.st.object(o).tapped)
            .count();
    assert_eq!(islands_tapped, 2);
    // One legendary creature: {2}{U} — two Islands are not enough, the ability isn't offered.
    let mut sc = scenario(&[&channel(), &legend("Test Hero")]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    sc.card(P0, Zone::Battlefield, "Test Hero");
    sc.card(P1, Zone::Battlefield, "Test Hero");
    sc.card(P0, Zone::Hand, "Test Sky Shrine");
    let s = Script::new(sc.build());
    assert!(!can_activate(&s, "Test Sky Shrine"));
    // CR 118.7a: never the colored part — four legendary creatures still leave {U}.
    let mut sc = scenario(&[&channel(), &legend("Test Hero")]);
    sc.cards(P0, Zone::Battlefield, &["Test Hero"; 4]);
    sc.card(P1, Zone::Battlefield, "Test Hero");
    sc.card(P0, Zone::Hand, "Test Sky Shrine");
    let s = Script::new(sc.build());
    assert!(
        !can_activate(&s, "Test Sky Shrine"),
        "{{U}} must still be paid"
    );
    let cat = forge_test_catalog(&[&channel()], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Sky Shrine").unwrap()));
    assert_eq!(
        text[1],
        "{3}{U}, Discard this card: Return target creature to its owner's hand. This ability costs {1} less to activate for each legendary creature you control."
    );
}

// CR 113.7a: "if the source is no longer in the zone it's expected to be in at that time, its
// last known information is used. The source can still perform the action even though it no
// longer exists." A channel land discarded for its own cost deals the damage.
// CR 205.3i: "a land card with a basic land type" (Forge's `hasABasicLandType`).
#[test]
fn a_discarded_source_deals_damage_and_a_land_with_a_basic_land_type_is_found() {
    let seat = card(
        "Test Seat of Arms",
        "no cost",
        "Legendary Land",
        "A:AB$ DealDamage | Cost$ 2 W Discard<1/CARDNAME> | ActivationZone$ Hand | ValidTgts$ Creature | NumDmg$ 4 | SpellDescription$ Text.",
    );
    let bear = card("Test Thicket Brute", "1 G", "Creature Bear", "PT:3/3");
    let mut sc = scenario(&[&seat, &bear]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains", "Plains"]);
    let brute = sc.card(P1, Zone::Battlefield, "Test Thicket Brute");
    sc.card(P0, Zone::Hand, "Test Seat of Arms");
    let mut s = Script::new(sc.build());
    s.activate("Test Seat of Arms")
        .target_object(brute)
        .pay()
        .pass_all();
    assert!(!s.st.objects.contains_key(&brute), "4 damage to a 3/3");
    // A search for a land card with a basic land type: a basic Forest and a nonbasic Swamp
    // land qualify, a land with no basic land type doesn't.
    let survey = card(
        "Test Survey",
        "G",
        "Sorcery",
        "A:SP$ ChangeZone | Origin$ Library | Destination$ Battlefield | ChangeType$ Land.hasABasicLandType | ChangeNum$ 1 | SpellDescription$ Text.",
    );
    let marsh = card("Test Marsh", "no cost", "Land Swamp", "");
    let waste = card("Test Waste", "no cost", "Land", "");
    let mut sc = Scenario::new(
        GameConfig::commander(2),
        forge_test_catalog(&[&survey, &marsh, &waste], &[]),
        31,
    );
    sc.library(P1, "Forest", 20);
    sc.cards(P0, Zone::Library, &["Test Marsh", "Test Waste", "Forest"]);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Survey");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Survey").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::SearchLibrary);
    let names: Vec<String> = s
        .options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Card { card } => Some(s.name(*card)),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Forest", "Test Marsh"]);
    let _ = (
        Event::Shuffled { player: P0 },
        ObjectId(0),
        forge_test_blockers("Name:x"),
    );
}

fn brass() -> String {
    card(
        "Test Brass Gate",
        "no cost",
        "Land",
        "A:AB$ Mana | Cost$ T | Produced$ R | SpellDescription$ Text.\n\
         T:Mode$ Taps | ValidCard$ Card.Self | Execute$ TrigDamage | TriggerZones$ Battlefield | TriggerDescription$ Text.\n\
         SVar:TrigDamage:DB$ DealDamage | Defined$ You | NumDmg$ 1",
    )
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

// CR 603.2e (2026-11-06): "An ability that triggers when a permanent 'becomes tapped' …
// doesn't trigger if the permanent enters the battlefield in that state." CR 701.26a: "Only
// untapped permanents can be tapped." The trigger event is the tapping, whatever taps it:
// a mana ability's cost (the trigger waits for the spell being cast, CR 603.3), or another
// player's effect. Forge: `T:Mode$ Taps | ValidCard$ Card.Self`.
#[test]
fn becomes_tapped_triggers_on_every_tapping_and_never_on_a_tapped_permanent() {
    let bolt = card(
        "Test Ember Flick",
        "R",
        "Instant",
        "A:SP$ DealDamage | ValidTgts$ Player | NumDmg$ 1 | SpellDescription$ Text.",
    );
    let freeze = card(
        "Test Hoarfrost Grip",
        "U",
        "Instant",
        "A:SP$ Tap | ValidTgts$ Permanent | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&brass(), &bolt, &freeze]);
    let gate = sc.card(P0, Zone::Battlefield, "Test Brass Gate");
    sc.card(P0, Zone::Hand, "Test Ember Flick");
    let mut s = Script::new(sc.build());
    s.cast("Test Ember Flick").target_player(P1).pay();
    // Tapped for mana while casting: the trigger goes on the stack above the spell.
    assert_eq!(s.st.stack.len(), 2);
    s.pass_all();
    assert_eq!(
        life(&s, P0),
        39,
        "the land dealt 1 damage to its controller"
    );
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(life(&s, P1), 39);
    // Tapping it again does nothing: it is tapped (CR 701.26a), so nothing triggers.
    let mut sc = scenario(&[&brass(), &freeze]);
    let gate2 = sc.card(P0, Zone::Battlefield, "Test Brass Gate");
    sc.tapped(gate2);
    sc.card(P0, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Hand, "Test Hoarfrost Grip");
    let mut s = Script::new(sc.build());
    s.cast("Test Hoarfrost Grip")
        .target_object(gate2)
        .pay()
        .pass_all();
    assert_eq!(life(&s, P0), 40);
    assert_eq!(s.count(|e| matches!(e, Event::Triggered { .. })), 0);
    // An opponent's effect taps it: it triggers, and its controller is dealt the damage.
    let mut sc = scenario(&[&brass(), &freeze]);
    let gate3 = sc.card(P0, Zone::Battlefield, "Test Brass Gate");
    sc.card(P1, Zone::Battlefield, "Island");
    sc.card(P1, Zone::Hand, "Test Hoarfrost Grip");
    let mut s = Script::new(sc.build());
    s.pass();
    s.cast("Test Hoarfrost Grip")
        .target_object(gate3)
        .pay()
        .pass_all();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(life(&s, P0), 39);
    let _ = gate;
}

// CR 603.2e: entering the battlefield tapped is not becoming tapped.
#[test]
fn entering_tapped_is_not_becoming_tapped() {
    let slow = card(
        "Test Slow Brass",
        "no cost",
        "Land",
        "R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplacementResult$ Updated | ReplaceWith$ ETBTapped | Description$ Text.\n\
         SVar:ETBTapped:DB$ Tap | Defined$ Self | ETB$ True\n\
         T:Mode$ Taps | ValidCard$ Card.Self | Execute$ TrigDamage | TriggerZones$ Battlefield | TriggerDescription$ Text.\n\
         SVar:TrigDamage:DB$ DealDamage | Defined$ You | NumDmg$ 1",
    );
    let mut sc = scenario(&[&slow]);
    sc.card(P0, Zone::Hand, "Test Slow Brass");
    let mut s = Script::new(sc.build());
    s.pick(
        "play",
        |s, o| matches!(o, OptionDesc::PlayLand { card } if s.name(*card) == "Test Slow Brass"),
    );
    let land = s.find(P0, Zone::Battlefield, "Test Slow Brass").unwrap();
    assert!(s.st.object(land).tapped);
    assert_eq!(s.count(|e| matches!(e, Event::Triggered { .. })), 0);
    let text = card_model::describe::rules_text(s.st.card_def(land));
    assert_eq!(
        text[1],
        "Whenever this land becomes tapped, this land deals 1 damage to you."
    );
}

fn sentinel() -> String {
    card(
        "Test Watchful Sentinel",
        "W",
        "Artifact Creature Human Soldier\nPT:1/1",
        "T:Mode$ SpellCast | ValidCard$ Card.nonCreature | ValidActivatingPlayer$ Player.Opponent | ActivatorThisTurnCast$ EQ1 | Execute$ TrigDraw | TriggerZones$ Battlefield | TriggerDescription$ Text.\n\
         SVar:TrigDraw:DB$ Draw | Defined$ You | UnlessCost$ X | UnlessPayer$ TriggeredActivator | NumCards$ 1\n\
         SVar:X:Count$CardPower",
    )
}

fn notion() -> String {
    card(
        "Test Passing Notion",
        "U",
        "Sorcery",
        "A:SP$ GainLife | LifeAmount$ 1 | SpellDescription$ Text.",
    )
}

fn hand(s: &Script, p: PlayerId) -> usize {
    s.st.zones.list(Zone::Hand, p).len()
}

// CR 601.2i (2026-11-06): a spell "becomes cast"; "whenever an opponent casts their first
// noncreature spell each turn" counts that player's noncreature spells cast this turn, each
// judged as it was cast, this one included (Forge: `ActivatorThisTurnCast$ EQ1` with
// `ValidCard$ Card.nonCreature`). CR 118.12: "draw a card unless that player pays {X}, where
// X is this creature's power" (`UnlessCost$ X`, X worked out as it resolves, CR 107.3,
// 608.2h).
#[test]
fn an_opponents_first_noncreature_spell_each_turn() {
    let bear = card("Test Plain Cub", "G", "Creature Bear", "PT:2/2");
    let mut sc = scenario(&[&sentinel(), &notion(), &bear]);
    sc.card(P0, Zone::Battlefield, "Test Watchful Sentinel");
    sc.cards(
        P1,
        Zone::Battlefield,
        &["Island", "Island", "Island", "Forest"],
    );
    sc.cards(
        P1,
        Zone::Hand,
        &[
            "Test Plain Cub",
            "Test Passing Notion",
            "Test Passing Notion",
        ],
    );
    sc.turn(P1, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    // A creature spell first: not a noncreature spell, nothing triggers.
    s.cast("Test Plain Cub").pay();
    assert_eq!(s.count(|e| matches!(e, Event::Triggered { .. })), 0);
    s.pass_all();
    // The first noncreature spell: it triggers; P1 may pay {1} (the Sentinel's power).
    let before = hand(&s, P0);
    s.cast("Test Passing Notion").pay();
    assert_eq!(s.count(|e| matches!(e, Event::Triggered { .. })), 1);
    drive_until(&mut s, |s| s.kind() == ChoiceKind::PayUnlessCost);
    assert_eq!(s.who(), P1);
    s.pick("decline", |_, o| matches!(o, OptionDesc::Decline));
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(hand(&s, P0), before + 1, "P1 didn't pay: P0 drew");
    // The second noncreature spell this turn: no trigger.
    s.cast("Test Passing Notion").pay();
    assert_eq!(s.count(|e| matches!(e, Event::Triggered { .. })), 1);
    let cat = forge_test_catalog(&[&sentinel()], &[]);
    let text =
        card_model::describe::rules_text(cat.get(cat.find("Test Watchful Sentinel").unwrap()));
    assert_eq!(
        text,
        [
            "Whenever an opponent casts their first noncreature spell each turn, X is this creature's power. Then draw a card unless that player pays {X}."
        ]
    );
}

// The same next turn: "each turn" counts this turn's spells only. And paying {X} is
// paying the amount X is as the ability resolves: with power 3 that is {3}, which a player
// with two untapped lands can't pay, so they aren't asked (CR 118.3).
#[test]
fn each_turn_starts_over_and_x_is_the_power_as_it_resolves() {
    let pump = card(
        "Test Sentinel Rally",
        "W",
        "Instant",
        "A:SP$ Pump | ValidTgts$ Creature | NumAtt$ +2 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&sentinel(), &notion(), &pump]);
    let sentinel = sc.card(P0, Zone::Battlefield, "Test Watchful Sentinel");
    sc.card(P0, Zone::Battlefield, "Plains");
    sc.card(P0, Zone::Hand, "Test Sentinel Rally");
    sc.cards(P1, Zone::Battlefield, &["Island", "Island", "Island"]);
    sc.cards(P1, Zone::Hand, &["Test Passing Notion"]);
    sc.turn(P1, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    let before = hand(&s, P0);
    s.cast("Test Passing Notion").pay();
    // P1 passes with the trigger on the stack; P0 makes the Sentinel 3/1.
    s.pass();
    s.cast("Test Sentinel Rally")
        .target_object(sentinel)
        .pay()
        .pass_all();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert!(
        !s.answered
            .iter()
            .any(|d| d.kind == ChoiceKind::PayUnlessCost),
        "{{3}} can't be paid with two lands"
    );
    assert_eq!(hand(&s, P0), before, "P0 cast the Rally from its hand");
    assert_eq!(
        s.count(|e| matches!(e, Event::Drew { player, .. } if *player == P0)),
        1
    );
    // Next turn (P0's), then P1's turn again: P1's first noncreature spell triggers anew.
    let turn = s.st.turn.turn_number;
    drive_until(&mut s, |s| {
        s.st.turn.turn_number > turn && s.st.turn.active == P1
    });
    assert!(s.st.player(P1).cast_this_turn.is_empty());
}
