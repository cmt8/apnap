//! Copies of spells and abilities, and storm (decisions.md D-085). Invented cards in Forge's
//! script format, compiled the way real ones are and then played.
//! Rules text: docs/rules-notes/707-spell-copies.md.

use engine_core::stack::{StackKind, Target};
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, Event, GameConfig, OptionDesc, PlayerId, Primitive, Response};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn card(name: &str, cost: &str, types: &str, body: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{types}\n{body}\nOracle:Text.")
}

/// A card whose Oracle text says "you control" (the compiler reads it back).
fn yours(name: &str, cost: &str, types: &str, body: &str) -> String {
    format!(
        "Name:{name}\nManaCost:{cost}\nTypes:{types}\n{body}\nOracle:Copy target spell or ability you control. You may choose new targets for the copy."
    )
}

/// A card whose Oracle text allows new targets for its copies.
fn new_targets(card: String) -> String {
    card.replace(
        "Oracle:Text.",
        "Oracle:Copy it. You may choose new targets for the copy.",
    )
}

/// Put simultaneous triggers on the stack in the order offered.
fn order(s: &mut Script) {
    if s.kind() == ChoiceKind::OrderTriggers {
        let n = s.options().len();
        s.respond(Response::Order((0..n).collect()));
    }
}

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc.turn(P0, Step::PrecombatMain);
    sc
}

/// Answer every decision with its first option(s) until `done`.
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

fn settle(s: &mut Script) {
    drive_until(s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

fn offered_targets(s: &Script) -> Vec<Target> {
    s.options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Target { target } => Some(*target),
            _ => None,
        })
        .collect()
}

fn sear() -> String {
    card(
        "Test Arc Sear",
        "X R",
        "Instant",
        "A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ X | SpellDescription$ Text.\nSVar:X:Count$xPaid",
    )
}

/// "Copy target instant or sorcery spell. You may choose new targets for the copy."
fn twincast() -> String {
    new_targets(card(
        "Test Twinning",
        "U U",
        "Instant",
        "A:SP$ CopySpellAbility | ValidTgts$ Instant,Sorcery | TargetType$ Spell | MayChooseTarget$ True | SpellDescription$ Text.",
    ))
}

/// "Copy target instant or sorcery spell." (the copy keeps the targets).
fn echo() -> String {
    card(
        "Test Echoing",
        "U",
        "Instant",
        "A:SP$ CopySpellAbility | ValidTgts$ Instant,Sorcery | TargetType$ Spell | SpellDescription$ Text.",
    )
}

fn negate() -> String {
    card(
        "Test Denial",
        "U U",
        "Instant",
        "A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | SpellDescription$ Text.",
    )
}

// CR 707.10 (2026-11-06): "a copy of a spell isn't cast". CR 601.2i: "whenever … casts"
// abilities trigger as a spell becomes cast — not for a copy; "whenever you cast or copy"
// (magecraft; Forge: `SpellCastOrCopy`, run for a copy put on the stack too) sees both.
#[test]
fn a_copy_is_not_cast() {
    let caster = card(
        "Test Cast Watcher",
        "U",
        "Enchantment",
        "T:Mode$ SpellCast | ValidCard$ Instant,Sorcery | ValidActivatingPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
         SVar:TrigGain:DB$ GainLife | LifeAmount$ 1",
    );
    let magecraft = card(
        "Test Copy Watcher",
        "U",
        "Enchantment",
        "T:Mode$ SpellCastOrCopy | ValidCard$ Instant,Sorcery | ValidActivatingPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
         SVar:TrigGain:DB$ GainLife | LifeAmount$ 10",
    );
    let mut sc = scenario(&[&sear(), &twincast(), &caster, &magecraft]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &[
            "Mountain",
            "Mountain",
            "Island",
            "Island",
            "Test Cast Watcher",
            "Test Copy Watcher",
        ],
    );
    sc.cards(P0, Zone::Hand, &["Test Arc Sear", "Test Twinning"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Arc Sear").x(1).target_player(P1).pay();
    order(&mut s);
    s.cast("Test Twinning").target("Test Arc Sear").pay();
    order(&mut s);
    settle(&mut s);
    assert_eq!(life(&s, P1), 38, "the spell and its copy");
    assert_eq!(
        life(&s, P0),
        40 + 2 + 30,
        "two spells cast (1 each); two cast and one copied (10 each)"
    );
    assert_eq!(s.count(|e| matches!(e, Event::SpellCast { .. })), 2);
    assert_eq!(s.st.player(P0).cast_this_turn.len(), 2);
    assert_eq!(
        s.count(|e| matches!(e, Event::CopiedOnStack { controller, .. } if *controller == P0)),
        1
    );
    // The copy ceased to exist as it left the stack (CR 704.5e): one Arc Sear card.
    let sears =
        s.st.objects
            .values()
            .filter(|o| s.name(o.id) == "Test Arc Sear")
            .count();
    assert_eq!(sears, 1);
    let cat = forge_test_catalog(&[&magecraft, &twincast()], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Copy Watcher").unwrap()));
    assert_eq!(
        text,
        ["Whenever you cast or copy an instant or sorcery spell, you gain 10 life."]
    );
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Twinning").unwrap()));
    assert_eq!(
        text,
        ["Copy target instant or sorcery spell. You may choose new targets for the copy."]
    );
}

// CR 707.10: "A copy of a spell or ability copies both the characteristics of the spell or
// ability and all decisions made for it, including modes, targets, the value of X, and
// additional or alternative costs." Without "you may choose new targets" the copy keeps
// the targets (Forge: `CopySpellAbility` without `MayChooseTarget$`).
#[test]
fn a_copy_keeps_modes_x_and_targets() {
    let charm = card(
        "Test Pick Blast",
        "R",
        "Instant",
        "A:SP$ Charm | Choices$ DBGain,DBBlast\n\
         SVar:DBGain:DB$ GainLife | LifeAmount$ 3 | SpellDescription$ Text.\n\
         SVar:DBBlast:DB$ DealDamage | ValidTgts$ Any | NumDmg$ 2 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&sear(), &echo(), &charm]);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 5]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    sc.cards(
        P0,
        Zone::Hand,
        &[
            "Test Arc Sear",
            "Test Echoing",
            "Test Echoing",
            "Test Pick Blast",
        ],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Arc Sear").x(3).target_player(P1).pay();
    s.cast("Test Echoing").target("Test Arc Sear").pay();
    s.pass_all();
    // The copy is on the stack above the original, with X = 3 and the same target; no
    // question was asked.
    assert_eq!(s.kind(), ChoiceKind::Priority);
    let top = s.st.stack.last().unwrap().clone();
    let orig = s.st.stack[s.st.stack.len() - 2].clone();
    assert_ne!(top.id, orig.id);
    assert_eq!(
        (top.x, &top.targets, &top.modes),
        (3, &orig.targets, &orig.modes)
    );
    assert_eq!(top.kind, StackKind::Spell);
    assert!(s.st.object(top.id).spell_copy);
    assert_eq!(s.st.object(top.id).owner, P0);
    settle(&mut s);
    assert_eq!(life(&s, P1), 34);
    // A modal spell: the copy has the mode chosen for it.
    s.cast("Test Pick Blast").mode(1).target_player(P1).pay();
    s.cast("Test Echoing").target("Test Pick Blast").pay();
    settle(&mut s);
    assert_eq!(life(&s, P1), 30);
    assert_eq!(life(&s, P0), 40, "not the other mode");
}

// CR 707.10c: "The player may leave any number of the targets unchanged, even if those
// targets would be illegal. If the player chooses to change some or all of the targets, the
// new targets must be legal." The copy's controller chooses (CR 707.10).
#[test]
fn new_targets_for_the_copy_are_optional() {
    let bear = card("Test Bystander", "1 G", "Creature Bear", "PT:2/2");
    let mut sc = scenario(&[&sear(), &twincast(), &bear]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    sc.card(P0, Zone::Hand, "Test Arc Sear");
    let cub = sc.card(P0, Zone::Battlefield, "Test Bystander");
    sc.cards(P1, Zone::Battlefield, &["Island", "Island"]);
    sc.card(P1, Zone::Hand, "Test Twinning");
    let mut s = Script::new(sc.build());
    s.cast("Test Arc Sear").x(2).target_player(P1).pay().pass();
    s.cast("Test Twinning")
        .target("Test Arc Sear")
        .pay()
        .pass_all();
    // P1 controls the copy and chooses its targets: keep (first), or another legal one.
    let copy = s.st.stack.last().unwrap().id;
    assert_eq!(
        s.kind(),
        ChoiceKind::NewTarget {
            object: copy,
            slot: 0
        }
    );
    assert_eq!(s.who(), P1);
    assert_eq!(s.st.stack_object(copy).unwrap().controller, P1);
    let offered = offered_targets(&s);
    assert_eq!(offered[0], Target::Player(P1), "keep");
    assert!(offered.contains(&Target::Player(P0)));
    assert!(offered.contains(&Target::Object(cub)));
    assert!(!offered.contains(&Target::Object(copy)), "CR 115.5");
    s.target_object(cub);
    settle(&mut s);
    assert!(
        !s.st.objects.contains_key(&cub),
        "the copy killed the creature"
    );
    assert_eq!(life(&s, P1), 38, "the original kept its target");
}

// CR 707.10: a copy is a spell of its own: countering the original leaves it. CR 704.5e: a
// countered copy ceases to exist (it isn't put into a graveyard).
#[test]
fn countering_the_original_leaves_the_copy() {
    let mut sc = scenario(&[&sear(), &echo(), &negate()]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Island"]);
    sc.cards(P0, Zone::Hand, &["Test Arc Sear", "Test Echoing"]);
    sc.cards(P1, Zone::Battlefield, &["Island"; 4]);
    sc.cards(P1, Zone::Hand, &["Test Denial", "Test Denial"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Arc Sear").x(1).target_player(P1).pay();
    s.cast("Test Echoing")
        .target("Test Arc Sear")
        .pay()
        .pass_all();
    let copy = s.st.stack.last().unwrap().id;
    let orig = s.st.stack[0].id;
    s.pass();
    // P1 counters the original.
    s.cast("Test Denial").target_object(orig).pay();
    settle(&mut s);
    assert_eq!(life(&s, P1), 39, "the copy resolved");
    assert!(!s.st.objects.contains_key(&copy));
    assert_eq!(
        s.st.zones
            .list(Zone::Graveyard, P0)
            .iter()
            .filter(|&&c| s.name(c) == "Test Arc Sear")
            .count(),
        1,
        "only the card"
    );
    // A countered copy ceases to exist.
    let mut sc = scenario(&[&sear(), &echo(), &negate()]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Island"]);
    sc.cards(P0, Zone::Hand, &["Test Arc Sear", "Test Echoing"]);
    sc.cards(P1, Zone::Battlefield, &["Island"; 2]);
    sc.card(P1, Zone::Hand, "Test Denial");
    let mut s = Script::new(sc.build());
    s.cast("Test Arc Sear").x(1).target_player(P1).pay();
    s.cast("Test Echoing")
        .target("Test Arc Sear")
        .pay()
        .pass_all();
    let copy = s.st.stack.last().unwrap().id;
    s.pass();
    s.cast("Test Denial").target_object(copy).pay();
    settle(&mut s);
    assert!(!s.st.objects.contains_key(&copy));
    assert_eq!(life(&s, P1), 39, "the original resolved");
    assert!(
        s.count(|e| matches!(e, Event::CeasedToExist { object } if *object == copy)) == 1,
        "CR 704.5e"
    );
}

// CR 707.10f: "Some effects copy a permanent spell. As that copy resolves, it ceases being a
// copy of a spell and becomes a token permanent." CR 111.13: it is not "created". CR 707.9b:
// "except it isn't legendary" (Forge: `NonLegendary$ True`).
#[test]
fn a_copied_permanent_spell_becomes_a_token() {
    let lens = yours(
        "Test Mirror Lens",
        "4",
        "Artifact",
        "A:AB$ CopySpellAbility | Cost$ T | ValidTgts$ Permanent.YouCtrl | TargetType$ Spell | SpellDescription$ Text.",
    )
    .replace(" You may choose new targets for the copy.", "");
    let major = yours(
        "Test Double Minor",
        "G U",
        "Instant",
        "A:SP$ CopySpellAbility | ValidTgts$ Creature.YouCtrl | TargetType$ Spell | NonLegendary$ True | SpellDescription$ Text.",
    )
    .replace(" You may choose new targets for the copy.", "");
    let bear = card("Test Grizzly", "1 G", "Creature Bear", "PT:2/2");
    let hero = card(
        "Test Lone Hero",
        "1 G",
        "Legendary Creature Human",
        "PT:3/3",
    );
    let mut sc = scenario(&[&lens, &major, &bear, &hero]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 5]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Test Mirror Lens"]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Grizzly", "Test Lone Hero", "Test Double Minor"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Grizzly").pay();
    s.activate("Test Mirror Lens").target("Test Grizzly");
    settle(&mut s);
    let bears: Vec<_> =
        s.st.zones
            .battlefield
            .iter()
            .copied()
            .filter(|&id| s.name(id) == "Test Grizzly")
            .collect();
    assert_eq!(bears.len(), 2);
    assert_eq!(
        bears.iter().filter(|&&id| s.st.object(id).is_token).count(),
        1,
        "the copy became a token"
    );
    assert_eq!(s.count(|e| matches!(e, Event::TokenCreated { .. })), 0);
    // "Except it isn't legendary": both stay (CR 704.5j looks at legendary permanents).
    s.cast("Test Lone Hero").pay();
    s.cast("Test Double Minor").target("Test Lone Hero").pay();
    settle(&mut s);
    let heroes =
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| s.name(id) == "Test Lone Hero")
            .count();
    assert_eq!(heroes, 2);
    let cat = forge_test_catalog(&[&major], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Double Minor").unwrap()));
    assert_eq!(
        text,
        ["Copy target creature spell you control, except it isn't legendary."]
    );
}

// CR 707.10: copying a triggered ability puts a copy of it on the stack; CR 707.10b: "A copy
// of an ability has the same source as the original ability." Forge: `TargetType$
// Triggered.YouCtrl | ValidTgts$ Card,Emblem` (Strionic Resonator).
#[test]
fn copy_target_triggered_or_activated_ability() {
    let prism = yours(
        "Test Echo Prism",
        "2",
        "Artifact",
        "A:AB$ CopySpellAbility | Cost$ T | TargetType$ Triggered.YouCtrl | ValidTgts$ Card,Emblem | TgtPrompt$ Select target triggered ability you control | MayChooseTarget$ True | SpellDescription$ Text.",
    );
    let pup = card(
        "Test Life Pup",
        "1 W",
        "Creature Dog",
        "PT:1/1\n\
         T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ Text.\n\
         SVar:TrigGain:DB$ GainLife | LifeAmount$ 3",
    );
    let imp = card(
        "Test Spark Imp",
        "R",
        "Creature Goblin",
        "PT:1/1\nA:AB$ DealDamage | Cost$ T | ValidTgts$ Player | NumDmg$ 1 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&prism, &pup, &imp]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Plains", "Plains", "Test Echo Prism"],
    );
    sc.card(P0, Zone::Battlefield, "Test Spark Imp");
    sc.card(P0, Zone::Hand, "Test Life Pup");
    let mut s = Script::new(sc.build());
    // An activated ability isn't a triggered one.
    s.activate("Test Spark Imp").target_player(P1);
    assert!(
        !s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if s.name(*source) == "Test Echo Prism")),
        "no triggered ability to target"
    );
    settle(&mut s);
    s.cast("Test Life Pup").pay().pass_all();
    let pup_id = s.find(P0, Zone::Battlefield, "Test Life Pup").unwrap();
    let trigger = s.st.stack.last().unwrap().id;
    s.activate("Test Echo Prism").target_object(trigger);
    s.pass_all();
    let copy = s.st.stack.last().unwrap().clone();
    assert_ne!(copy.id, trigger);
    assert!(matches!(
        copy.kind,
        StackKind::Ability { source, triggered: true, .. } if source == pup_id
    ));
    settle(&mut s);
    assert_eq!(life(&s, P0), 46);
    let cat = forge_test_catalog(&[&prism], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Echo Prism").unwrap()));
    assert_eq!(
        text,
        [
            "{T}: Copy target triggered ability you control. You may choose new targets for the copy."
        ]
    );
}

// CR 608.2h: "copy that spell" uses the spell as it last existed on the stack if it has
// left it: countered in response, it is still copied. Forge: `SpellCast` trigger with
// `CopySpellAbility | Defined$ TriggeredSpellAbility` (Swarm Intelligence, Double Vision).
#[test]
fn whenever_you_cast_copy_that_spell() {
    let swarm = new_targets(card(
        "Test Swarm Mind",
        "6 U",
        "Enchantment",
        "T:Mode$ SpellCast | ValidCard$ Instant,Sorcery | ValidActivatingPlayer$ You | Execute$ TrigCopy | TriggerZones$ Battlefield | TriggerDescription$ Text.\n\
         SVar:TrigCopy:DB$ CopySpellAbility | Defined$ TriggeredSpellAbility | AILogic$ Always | MayChooseTarget$ True",
    ));
    let mut sc = scenario(&[&sear(), &swarm, &negate()]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Mountain", "Mountain", "Test Swarm Mind"],
    );
    sc.card(P0, Zone::Hand, "Test Arc Sear");
    sc.cards(P1, Zone::Battlefield, &["Island", "Island"]);
    sc.card(P1, Zone::Hand, "Test Denial");
    let mut s = Script::new(sc.build());
    s.cast("Test Arc Sear").x(1).target_player(P1).pay();
    let orig = s.find(P0, Zone::Stack, "Test Arc Sear").unwrap();
    assert_eq!(s.st.stack.len(), 2, "the trigger is above the spell");
    s.pass();
    s.cast("Test Denial").target_object(orig).pay();
    // The counter resolves, then the trigger: the copy is made all the same.
    s.pass_all();
    assert!(
        s.find(P0, Zone::Graveyard, "Test Arc Sear").is_some(),
        "countered"
    );
    s.pass_all();
    assert!(matches!(s.kind(), ChoiceKind::NewTarget { .. }));
    s.target_player(P1);
    settle(&mut s);
    assert_eq!(life(&s, P1), 39, "the copy");
    let cat = forge_test_catalog(&[&swarm], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Swarm Mind").unwrap()));
    assert_eq!(
        text,
        [
            "Whenever you cast an instant or sorcery spell, copy that spell. You may choose new targets for the copy."
        ]
    );
}

// CR 702.40a: "Storm is a triggered ability that functions on the stack. 'Storm' means 'When
// you cast this spell, copy it for each other spell that was cast before it this turn. If
// the spell has any targets, you may choose new targets for any of the copies.'" Every
// player's spells count; copies were never cast and don't.
#[test]
fn storm_counts_every_players_spells() {
    let gain = card(
        "Test Quick Gain",
        "W",
        "Instant",
        "A:SP$ GainLife | LifeAmount$ 1 | SpellDescription$ Text.",
    );
    let shot = "Name:Test Grape Storm\nManaCost:1 R\nTypes:Sorcery\nK:Storm\n\
        A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 1 | SpellDescription$ Text.\n\
        Oracle:Storm (When you cast this spell, copy it for each spell cast before it this turn. You may choose new targets for the copies.)\\nTest Grape Storm deals 1 damage to any target."
        .to_string();
    let mut sc = scenario(&[&gain, &shot]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Quick Gain", "Test Grape Storm"]);
    sc.cards(P1, Zone::Battlefield, &["Plains"]);
    sc.card(P1, Zone::Hand, "Test Quick Gain");
    let mut s = Script::new(sc.build());
    s.cast("Test Quick Gain").pay().pass();
    s.cast("Test Quick Gain").pay();
    settle(&mut s);
    s.cast("Test Grape Storm").target_player(P1).pay();
    // The storm trigger: two spells were cast before it (one of them P1's).
    let storm = s.st.stack.last().unwrap();
    assert!(matches!(
        storm.kind,
        StackKind::Ability {
            triggered: true,
            ..
        }
    ));
    assert_eq!(storm.facts.amount, 2);
    s.pass_all();
    for _ in 0..2 {
        assert!(matches!(s.kind(), ChoiceKind::NewTarget { .. }));
        s.target_player(P1);
    }
    settle(&mut s);
    assert_eq!(life(&s, P1), 41 - 3, "the spell and two copies");
    assert_eq!(
        s.st.players
            .iter()
            .map(|p| p.cast_this_turn.len())
            .sum::<usize>(),
        3
    );
    let cat = forge_test_catalog(&[&shot], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Grape Storm").unwrap()));
    assert_eq!(
        text[0],
        "When you cast this spell, copy it for each other spell that was cast before it this turn. If the spell has any targets, you may choose new targets for any of the copies."
    );
}

// CR 707.10: a copy on the stack is part of the game state: saved and loaded mid-stack, the
// game goes on the same.
#[test]
fn save_and_load_with_a_copy_on_the_stack() {
    let mut sc = scenario(&[&sear(), &echo()]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Mountain", "Mountain", "Mountain", "Island"],
    );
    sc.cards(P0, Zone::Hand, &["Test Arc Sear", "Test Echoing"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Arc Sear").x(2).target_player(P1).pay();
    s.cast("Test Echoing")
        .target("Test Arc Sear")
        .pay()
        .pass_all();
    let copy = s.st.stack.last().unwrap().id;
    assert!(s.st.object(copy).spell_copy);
    let saved = harness::save_json(&s.st);
    let loaded = harness::load_json(&saved).unwrap();
    assert_eq!(loaded.state_hash(), s.st.state_hash());
    let mut t = Script::new(loaded);
    settle(&mut s);
    settle(&mut t);
    assert_eq!(life(&t, P1), 36);
    assert_eq!(s.st.state_hash(), t.st.state_hash());
}

// CR 707.10, 608.2h: "Whenever you cast an instant or sorcery spell, copy it for each other
// instant and sorcery spell you've cast before it this turn" — counted as it triggers, of the
// trigger's own kind and caster (Forge: `TriggerObjectsCurrentCastSpells$Valid
// Sorcery.YouCtrl,Instant.YouCtrl/Minus.1`, Thousand-Year Storm).
#[test]
fn copy_it_for_each_other_spell_of_its_kind() {
    let storm = "Name:Test Millennium Storm\nManaCost:4 U R\nTypes:Enchantment\n\
        T:Mode$ SpellCast | ValidCard$ Instant,Sorcery | ValidActivatingPlayer$ You | Execute$ TrigCopy | TriggerZones$ Battlefield | TriggerDescription$ Text.\n\
        SVar:TrigCopy:DB$ CopySpellAbility | Defined$ TriggeredSpellAbility | Amount$ X | MayChooseTarget$ True\n\
        SVar:X:TriggerObjectsCurrentCastSpells$Valid Sorcery.YouCtrl,Instant.YouCtrl/Minus.1\n\
        Oracle:Whenever you cast an instant or sorcery spell, copy it for each other instant and sorcery spell you've cast before it this turn. You may choose new targets for the copies."
        .to_string();
    let gain = card(
        "Test Quick Gain",
        "W",
        "Instant",
        "A:SP$ GainLife | LifeAmount$ 1 | SpellDescription$ Text.",
    );
    let bear = card("Test Grizzly", "G", "Creature Bear", "PT:2/2");
    let mut sc = scenario(&[&storm, &gain, &bear]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Plains", "Plains", "Forest", "Test Millennium Storm"],
    );
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Quick Gain", "Test Grizzly", "Test Quick Gain"],
    );
    sc.card(P1, Zone::Battlefield, "Plains");
    sc.card(P1, Zone::Hand, "Test Quick Gain");
    let mut s = Script::new(sc.build());
    s.cast("Test Quick Gain").pay().pass();
    // An opponent's instant, cast this turn: not one "you've cast".
    s.cast("Test Quick Gain").pay();
    settle(&mut s);
    assert_eq!(life(&s, P0), 41, "no copy of the first");
    assert_eq!(life(&s, P1), 41);
    s.cast("Test Grizzly").pay();
    settle(&mut s);
    s.cast("Test Quick Gain").pay();
    settle(&mut s);
    assert_eq!(
        life(&s, P0),
        43,
        "the second and one copy; neither the creature nor the opponent's instant counts"
    );
    let cat = forge_test_catalog(&[&storm], &[]);
    let text =
        card_model::describe::rules_text(cat.get(cat.find("Test Millennium Storm").unwrap()));
    assert_eq!(
        text,
        [
            "Whenever you cast an instant or sorcery spell, copy that spell for each other such spell you've cast before it this turn. You may choose new targets for the copies."
        ]
    );
}

// Not compiled: what this batch doesn't build, and a script that allows new targets where
// the card doesn't.
#[test]
fn copies_not_built_are_named() {
    let reflect = card(
        "Test Reflection",
        "4 U",
        "Enchantment",
        "T:Mode$ SpellCast | ValidCard$ Instant | ValidActivatingPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigCopy | TriggerDescription$ Text.\n\
         SVar:TrigCopy:DB$ CopySpellAbility | Defined$ TriggeredSpellAbility | MayChooseTarget$ True",
    )
    .replace("Oracle:Text.", "Oracle:Whenever you cast an instant spell, copy that spell.");
    assert!(
        forge_test_blockers(&reflect)
            .iter()
            .any(|b| b == "oracle:says new targets")
    );
    // "Copy this spell for each …" of another player, copies for each target (Radiate).
    let radiate = card(
        "Test Radiance",
        "3 R R",
        "Instant",
        "A:SP$ CopySpellAbility | ValidTgts$ Instant,Sorcery | TargetType$ Spell | CopyForEachCanTarget$ Permanent | SpellDescription$ Text.",
    );
    assert!(
        forge_test_blockers(&radiate)
            .iter()
            .any(|b| b == "param:CopySpellAbility.CopyForEachCanTarget")
    );
}
