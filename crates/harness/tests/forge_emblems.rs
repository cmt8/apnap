//! Emblems (D-088): objects in the command zone with abilities and nothing else. Invented
//! cards in Forge's format (`DB$ Effect | Name$ Emblem — … | Duration$ Permanent`). Rules
//! text: docs/rules-notes/114-emblems.md (CR 114, 800.4a).

use engine_core::events::Event;
use engine_core::stack::Target;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, ObjectId, OptionDesc, PlayerId};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

/// "You get an emblem with "Creatures you control get +1/+1.""
const RALLY: &str = "Name:Test Rally Emblem\nManaCost:W\nTypes:Sorcery\n\
    A:SP$ Effect | Name$ Emblem — Test Rally | StaticAbilities$ STPump | Duration$ Permanent | SpellDescription$ You get an emblem with \"Creatures you control get +1/+1.\"\n\
    SVar:STPump:Mode$ Continuous | Affected$ Creature.YouCtrl | AffectedZone$ Battlefield | AddPower$ 1 | AddToughness$ 1 | Description$ Creatures you control get +1/+1.\n\
    Oracle:You get an emblem with \"Creatures you control get +1/+1.\"";

/// "You get an emblem with "Whenever you cast a spell, this emblem deals 2 damage to any
/// target.""
const SPARK: &str = "Name:Test Spark Emblem\nManaCost:R\nTypes:Sorcery\n\
    A:SP$ Effect | Name$ Emblem — Test Spark | Triggers$ TrigCast | Duration$ Permanent | SpellDescription$ You get an emblem.\n\
    SVar:TrigCast:Mode$ SpellCast | ValidActivatingPlayer$ You | TriggerZones$ Command | Execute$ DBDmg | TriggerDescription$ Whenever you cast a spell, this emblem deals 2 damage to any target.\n\
    SVar:DBDmg:DB$ DealDamage | ValidTgts$ Any | NumDmg$ 2\n\
    Oracle:You get an emblem with \"Whenever you cast a spell, this emblem deals 2 damage to any target.\"";

/// "You get an emblem with "Prevent all damage that would be dealt to you.""
const WARD: &str = "Name:Test Ward Emblem\nManaCost:W\nTypes:Sorcery\n\
    A:SP$ Effect | Name$ Emblem — Test Ward | ReplacementEffects$ RPrevent | Duration$ Permanent | SpellDescription$ You get an emblem.\n\
    SVar:RPrevent:Event$ DamageDone | ValidTarget$ You | Prevent$ True | Description$ Prevent all damage that would be dealt to you.\n\
    Oracle:You get an emblem with \"Prevent all damage that would be dealt to you.\"";

const ZAP: &str = "Name:Test Zap\nManaCost:R\nTypes:Instant\n\
    A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 3 | SpellDescription$ Test Zap deals 3 damage to any target.\n\
    Oracle:Test Zap deals 3 damage to any target.";

fn scenario(cards: &[&str], players: u8) -> Scenario {
    let config = if players == 2 {
        GameConfig::commander(2)
    } else {
        GameConfig::commander(players)
    };
    let mut sc = Scenario::new(config, forge_test_catalog(cards, &[]), 43);
    for p in 0..players {
        sc.library(PlayerId(p), "Forest", 20);
    }
    sc
}

fn emblems(s: &Script, p: PlayerId) -> Vec<ObjectId> {
    s.st.zones
        .command
        .iter()
        .copied()
        .filter(|id| {
            let o = s.st.object(*id);
            o.emblem.is_some() && o.owner == p
        })
        .collect()
}

fn pt(s: &Script, id: ObjectId) -> (i32, i32) {
    let c = engine_core::layers::chars(&s.st, id);
    (c.power.unwrap(), c.toughness.unwrap())
}

// CR 114.2, 114.3, 114.4, 114.5 (2026-11-06): the emblem goes to the command zone, owned and
// controlled by the player who got it, with no characteristics but its abilities, which
// work from there; it is no permanent. Two emblems are two objects and both work.
#[test]
fn cr_114_an_emblems_static_ability_works_from_the_command_zone() {
    let mut sc = scenario(&[RALLY], 2);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Rally Emblem", "Test Rally Emblem"]);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    let before = pt(&s, golem);
    s.cast("Test Rally Emblem").pay().pass_all();
    let e = emblems(&s, P0);
    assert_eq!(e.len(), 1);
    let o = s.st.object(e[0]);
    assert_eq!((o.zone, o.owner, o.controller), (Zone::Command, P0, P0));
    let c = engine_core::layers::chars(&s.st, e[0]);
    assert!(c.name.is_empty() && c.card_types.is_empty() && c.colors.is_empty());
    assert!(c.mana_cost.is_none() && c.abilities.len() == 1);
    assert!(
        !s.st.zones.battlefield.contains(&e[0]),
        "CR 114.5: not a permanent"
    );
    assert_eq!(
        s.count(|ev| matches!(ev, Event::EmblemCreated { player, .. } if *player == P0)),
        1
    );
    assert_eq!(pt(&s, golem), (before.0 + 1, before.1 + 1));
    assert_eq!(
        pt(&s, theirs),
        before,
        "only creatures its controller controls"
    );
    s.cast("Test Rally Emblem").pay().pass_all();
    assert_eq!(emblems(&s, P0).len(), 2);
    assert_eq!(pt(&s, golem), (before.0 + 2, before.1 + 2));
}

// CR 114.4 (2026-11-06): a triggered ability of an emblem triggers from the command zone;
// "this emblem deals damage" — the emblem is the source. CR 114.5 / 115.4: an emblem can't
// be targeted ("any target" is creatures, players, planeswalkers, battles).
#[test]
fn cr_114_4_an_emblems_triggered_ability_and_damage_from_it() {
    let mut sc = scenario(&[SPARK, ZAP], 2);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Spark Emblem", "Test Zap"]);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Spark Emblem").pay().pass_all();
    let emblem = emblems(&s, P0)[0];
    s.cast("Test Zap");
    // The Zap's own target: no emblem among them.
    assert!(!s.has(
        |_, o| matches!(o, OptionDesc::Target { target: Target::Object(id) } if *id == emblem)
    ));
    s.target_player(P1).pay();
    // The emblem's trigger, put on the stack above the Zap: its target.
    assert_eq!(s.kind(), ChoiceKind::ChooseTargets { slot: 0 });
    s.target_player(P1).pass_all();
    assert_eq!(s.st.player(P1).life, 38, "2 from the emblem");
    assert_eq!(
        s.count(|ev| matches!(ev, Event::DamageDealt { source, .. } if *source == emblem)),
        1
    );
    s.pass_all();
    assert_eq!(s.st.player(P1).life, 35);
}

// CR 114.4, 615 (2026-11-06): a replacement (prevention) effect of an emblem applies.
#[test]
fn cr_114_4_an_emblems_prevention_effect() {
    let mut sc = scenario(&[WARD, ZAP], 2);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    sc.card(P0, Zone::Hand, "Test Ward Emblem");
    sc.cards(P1, Zone::Battlefield, &["Mountain"]);
    sc.card(P1, Zone::Hand, "Test Zap");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Ward Emblem").pay().pass_all();
    assert_eq!(emblems(&s, P0).len(), 1);
    // P0 passes; P1 zaps P0 (an instant): the damage is prevented.
    s.pass();
    assert_eq!(s.who(), P1);
    s.cast("Test Zap");
    s.target_player(P0).pay().pass_all();
    assert_eq!(s.st.player(P0).life, 40);
}

// CR 800.4a (2026-11-06): when a player leaves the game, the objects they own leave too —
// their emblems among them.
#[test]
fn cr_800_4a_emblems_leave_with_their_owner() {
    let mut sc = scenario(&[RALLY], 3);
    sc.cards(P1, Zone::Battlefield, &["Plains"]);
    sc.card(P1, Zone::Hand, "Test Rally Emblem");
    sc.turn(P1, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Rally Emblem").pay().pass_all();
    let e = emblems(&s, P1);
    assert_eq!(e.len(), 1);
    engine_core::actions::execute(
        &mut s.st,
        engine_core::actions::GameAction::LeaveGame { player: P1 },
    );
    assert!(!s.st.objects.contains_key(&e[0]));
    assert!(emblems(&s, P1).is_empty());
    let _ = P2;
}

#[test]
fn cr_114_2_emblem_text_and_what_stays_blocked() {
    let catalog = forge_test_catalog(&[SPARK], &[]);
    let def = catalog.get(catalog.find("Test Spark Emblem").unwrap());
    let text = card_model::describe::rules_text(def).join(" / ");
    assert_eq!(
        text,
        "You get an emblem with \"Whenever you cast a spell, this emblem deals 2 damage to any target.\""
    );
    // An emblem's activated abilities (Forge `Abilities$`) aren't compiled.
    let activated = RALLY.replace("StaticAbilities$ STPump", "Abilities$ ABDraw")
        + "\nSVar:ABDraw:AB$ Draw | Cost$ 1 | SpellDescription$ Draw a card.";
    assert!(
        forge_test_blockers(&activated).contains(&"emblem:activated abilities".to_string()),
        "{:?}",
        forge_test_blockers(&activated)
    );
}
