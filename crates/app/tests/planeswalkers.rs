//! Planeswalkers in the app (D-086): loyalty abilities are offered like other activated
//! abilities (a click on the planeswalker), and an attack can point at a planeswalker on
//! the table (click it, then the creatures; or drag a creature onto it). Driven with real
//! pointer presses at the places the cards are drawn.

use app::session::Session;
use app::ui::{App, Screen};
use card_model::CounterKind;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{GameConfig, GameState, ObjectId, PlayerId};
use harness::Agents;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const WALKER: &str = "Name:Test Sage Walker\nManaCost:2 W\nTypes:Legendary Planeswalker Ajani\nLoyalty:3\n\
    A:AB$ GainLife | Cost$ AddCounter<1/LOYALTY> | Planeswalker$ True | LifeAmount$ 2 | SpellDescription$ You gain 2 life.\n\
    A:AB$ DealDamage | Cost$ SubCounter<2/LOYALTY> | Planeswalker$ True | ValidTgts$ Creature | NumDmg$ 3 | SpellDescription$ CARDNAME deals 3 damage to target creature.\n\
    Oracle:[+1]: You gain 2 life.\\n[−2]: Test Sage Walker deals 3 damage to target creature.";
const OGRE: &str = "Name:Test Walker Ogre\nManaCost:2 R\nTypes:Creature Ogre\nPT:3/3\nOracle:";

fn scenario() -> Scenario {
    let mut sc = Scenario::new(
        GameConfig::commander(2),
        harness::forge_test_catalog(&[WALKER, OGRE], &[]),
        23,
    );
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

fn with_loyalty(mut st: GameState, id: ObjectId, n: u32) -> GameState {
    st.objects
        .get_mut(&id)
        .unwrap()
        .counters
        .insert(CounterKind::LOYALTY, n);
    st
}

fn harness_for(st: GameState) -> Harness<'static, App> {
    let mut app = App::with_saves_dir(std::env::temp_dir().join("apnap-test-pw-unused"));
    app.session = Some(Session::from_state(st, Some(P0), Agents::Random));
    app.screen = Screen::Game;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .with_max_steps(64)
        .build_ui_state(|ui, app: &mut App| app::ui::ui(app, ui), app);
    h.run_ok();
    h
}

fn click_card(h: &mut Harness<'_, App>, id: ObjectId) {
    let pos = h
        .state()
        .card_rects
        .get(&id)
        .unwrap_or_else(|| panic!("{id:?} isn't on the table"))
        .center();
    h.drag_at(pos);
    h.run_ok();
    h.drop_at(pos);
    h.run_ok();
}

fn button(h: &mut Harness<'_, App>, label: &str) {
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, label)
        .next()
        .unwrap_or_else(|| panic!("no button {label:?}"))
        .click();
    h.run_ok();
}

fn loyalty(h: &Harness<'_, App>, id: ObjectId) -> u32 {
    h.state()
        .session
        .as_ref()
        .unwrap()
        .st
        .object(id)
        .counters_of(CounterKind::LOYALTY)
}

// CR 606.2–606.3: a loyalty ability is activated like any activated ability — a click on the
// planeswalker offers it, by its loyalty symbol.
#[test]
fn loyalty_abilities_are_offered_by_clicking_the_planeswalker() {
    let mut sc = scenario();
    let pw = sc.card(P0, Zone::Battlefield, "Test Sage Walker");
    sc.card(P1, Zone::Battlefield, "Test Walker Ogre");
    sc.turn(P0, Step::PrecombatMain);
    let mut h = harness_for(with_loyalty(sc.build(), pw, 3));
    click_card(&mut h, pw);
    let label = "Test Sage Walker — [+1]: You gain 2 life.";
    button(&mut h, label);
    assert_eq!(loyalty(&h, pw), 4);
}

// CR 506.3, 508.1b: in the attack plan a planeswalker is clicked, then the creature that
// attacks it; confirmed, the engine's combat has it attacking that planeswalker. An attack
// on the player still works as before.
#[test]
fn an_attack_points_at_a_planeswalker_on_the_table() {
    let mut sc = scenario();
    let pw = sc.card(P1, Zone::Battlefield, "Test Sage Walker");
    let ogre = sc.card(P0, Zone::Battlefield, "Test Walker Ogre");
    let other = sc.card(P0, Zone::Battlefield, "Test Walker Ogre");
    sc.turn(P0, Step::BeginningOfCombat);
    let mut h = harness_for(with_loyalty(sc.build(), pw, 5));
    let plan = h.state().plan.clone().expect("planning the attack");
    assert_eq!(plan.planeswalkers(), [pw]);
    click_card(&mut h, pw);
    assert_eq!(
        h.state().plan.as_ref().unwrap().target_planeswalker,
        Some(pw)
    );
    click_card(&mut h, ogre);
    assert_eq!(
        h.state().plan.as_ref().unwrap().attacks_planeswalker(ogre),
        Some(pw)
    );
    // Back to the player for the other one.
    click_card(&mut h, pw);
    click_card(&mut h, other);
    assert_eq!(h.state().plan.as_ref().unwrap().attacks(other), Some(P1));
    button(&mut h, "Attack with 2");
    // (The opponent has no creatures: combat runs to its end without asking anyone.)
    let log = &h.state().session.as_ref().unwrap().log;
    let said = |line: &str| log.iter().any(|l| l.contains(line));
    assert!(
        said("Test Walker Ogre (P0) deals 3 combat damage to Test Sage Walker (P1)")
            && said("3 loyalty counter(s) removed from Test Sage Walker (P1)")
            && said("Test Walker Ogre (P0) deals 3 combat damage to P1 (P1 at 37)"),
        "{log:#?}"
    );
}
