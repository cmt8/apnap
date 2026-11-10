//! What each script feature the Forge compiler accepts *means* (CLAUDE.md invariant 15,
//! decisions.md D-046): invented cards written in Forge's script format are compiled the
//! way real ones are and then played. One test per feature, not per card.

use engine_core::layers::chars;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{GameConfig, ObjectId, PlayerId};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

fn pt(s: &Script, id: ObjectId) -> (i32, i32) {
    let c = chars(&s.st, id);
    (
        c.power.expect("has power"),
        c.toughness.expect("has toughness"),
    )
}

// CR 608.2h (2026-11-06): an amount an effect needs is determined once, as it is applied.
// CR 611.2c: the set of objects a resolving spell's continuous effect affects is fixed too.
// Forge: `NumAtt$ +X` with `SVar:X:Count$Valid …`.
#[test]
fn pump_all_by_a_counted_amount_is_fixed_on_resolution() {
    let rally = "Name:Test Muster\n\
        ManaCost:1 W\n\
        Types:Sorcery\n\
        A:SP$ PumpAll | Cost$ 1 W | ValidCards$ Creature.YouCtrl | NumAtt$ +X | NumDef$ +X | SpellDescription$ Text.\n\
        SVar:X:Count$Valid Creature.YouCtrl\n\
        Oracle:Text.";
    let mut sc = scenario(&[rally]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains", "Forest"]);
    let golems = sc.cards(P0, Zone::Battlefield, &["Test Golem"; 2]); // 2/3
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Muster", "Test Elf"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    for &g in &golems {
        assert_eq!(pt(&s, g), (4, 5), "two creatures: +2/+2");
    }
    assert_eq!(pt(&s, theirs), (2, 3));
    // A third creature arrives: the bonus neither grows nor reaches it.
    s.cast("Test Elf").pay().pass_all();
    let elf = s.find(P0, Zone::Battlefield, "Test Elf").unwrap();
    assert_eq!(pt(&s, elf), (1, 1));
    assert_eq!(pt(&s, golems[0]), (4, 5));
}

// Forge: `NumAtt$ -X | NumDef$ -X` on a targeted Pump: "-X/-X", the sign is the script's.
#[test]
fn pump_by_a_negative_counted_amount() {
    let sap = "Name:Test Sapping\n\
        ManaCost:B\n\
        Types:Instant\n\
        A:SP$ Pump | Cost$ B | ValidTgts$ Creature | NumAtt$ -X | NumDef$ -X | IsCurse$ True | SpellDescription$ Text.\n\
        SVar:X:Count$Valid Land.YouCtrl\n\
        Oracle:Text.";
    let mut sc = scenario(&[sap]);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
    let giant = sc.card(P1, Zone::Battlefield, "Test Giant"); // 5/4
    sc.card(P0, Zone::Hand, "Test Sapping");
    let mut s = Script::new(sc.build());
    s.cast("Test Sapping").target("Test Giant").pay().pass_all();
    assert_eq!(pt(&s, giant), (3, 2), "two lands: -2/-2");
}

// Forge: a spell's `Cost$` normally repeats `ManaCost:`. When it says anything else the
// spell has an additional or alternative cost, which must not be silently dropped.
#[test]
fn a_spell_cost_that_differs_from_the_mana_cost_blocks() {
    let card = "Name:Test Offering\n\
        ManaCost:1 B\n\
        Types:Sorcery\n\
        A:SP$ Draw | Cost$ 1 B Sac<1/Creature> | NumCards$ 2 | SpellDescription$ Text.\n\
        Oracle:Text.";
    assert_eq!(forge_test_blockers(card), ["spell:additional-cost"]);
    let plain = card.replace(" Sac<1/Creature>", "");
    assert_eq!(forge_test_blockers(&plain), [] as [&str; 0]);
}
