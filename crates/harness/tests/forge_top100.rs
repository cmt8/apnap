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
