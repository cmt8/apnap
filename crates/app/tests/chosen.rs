//! "As this enters, choose a creature type" in the app (D-076): every creature type may be
//! chosen, so the picker is a search field over the list, the types of the player's own
//! cards first.

use app::session::Session;
use app::ui::{App, Screen};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, GameState, PlayerId};
use harness::Agents;
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const BANNER: &str = "Name:Test Kin Banner\nManaCost:3\nTypes:Artifact\n\
    K:ETBReplacement:Other:ChooseCT\n\
    SVar:ChooseCT:DB$ ChooseType | Defined$ You | Type$ Creature | SpellDescription$ As CARDNAME enters, choose a creature type.\n\
    S:Mode$ Continuous | Affected$ Creature.ChosenType+YouCtrl | AddPower$ 1 | AddToughness$ 1 | Description$ Text.\n\
    Oracle:As this artifact enters, choose a creature type.\\nCreatures you control of the chosen type get +1/+1.";
const ELF: &str = "Name:Test Treetop Elf\nManaCost:G\nTypes:Creature Elf\nPT:1/1\nOracle:Text.";

fn harness_for(st: GameState) -> Harness<'static, App> {
    let mut app = App::with_saves_dir(std::env::temp_dir().join("apnap-test-chosen-unused"));
    app.session = Some(Session::from_state(st, Some(P0), Agents::Random));
    app.screen = Screen::Game;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 1000.0))
        .with_max_steps(64)
        .build_ui_state(|ui, app: &mut App| app::ui::ui(app, ui), app);
    h.run_ok();
    h
}

/// The state with P0 being asked which creature type their banner chooses.
fn asked() -> GameState {
    let mut sc = Scenario::new(
        GameConfig::commander(2),
        harness::forge_test_catalog(&[BANNER, ELF], &[]),
        5,
    );
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc.cards(P0, Zone::Battlefield, &["Forest"; 3]);
    sc.card(P0, Zone::Battlefield, "Test Treetop Elf");
    sc.card(P0, Zone::Hand, "Test Kin Banner");
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Banner").pay().pass_all();
    assert!(matches!(s.kind(), ChoiceKind::Choose { .. }));
    s.st
}

fn banner_choice(h: &Harness<'_, App>) -> Option<String> {
    let st = &h.state().session.as_ref().unwrap().st;
    st.zones.battlefield.iter().find_map(|id| {
        let o = st.object(*id);
        (st.card_def(*id).name == "Test Kin Banner").then(|| match o.chosen.first() {
            Some(card_model::Chosen::CreatureType(t)) => t.name().to_string(),
            other => format!("{other:?}"),
        })
    })
}

#[test]
fn a_creature_type_is_found_by_typing_and_chosen_with_a_click() {
    let mut h = harness_for(asked());
    // The title is the ability's text; the player's own type is offered first.
    assert!(
        h.query_all_by_label_contains("As Test Kin Banner enters, choose a creature type")
            .next()
            .is_some()
    );
    assert!(
        h.query_all_by_role_and_label(egui::accesskit::Role::Button, "Elf")
            .next()
            .is_some()
    );
    // Not every type is a button at once: typing narrows the list.
    assert!(
        h.query_all_by_role_and_label(egui::accesskit::Role::Button, "Zombie")
            .next()
            .is_none()
    );
    h.query_by_role(egui::accesskit::Role::TextInput)
        .expect("a search field")
        .click();
    h.run_ok();
    h.query_by_role(egui::accesskit::Role::TextInput)
        .unwrap()
        .type_text("zomb");
    h.run_ok();
    assert!(
        h.query_all_by_role_and_label(egui::accesskit::Role::Button, "Elf")
            .next()
            .is_none()
    );
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, "Zombie")
        .next()
        .expect("Zombie is offered")
        .click();
    h.run_ok();
    let s = h.state().session.as_ref().unwrap();
    assert_eq!(
        banner_choice(&h).as_deref(),
        Some("Zombie"),
        "{:?} {:?}",
        s.responses,
        s.log
    );
    // The search is cleared for the next decision.
    assert!(h.state().search.is_empty());
}

#[test]
fn the_chosen_type_is_shown_on_the_card() {
    let mut h = harness_for(asked());
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, "Elf")
        .next()
        .unwrap()
        .click();
    h.run_ok();
    assert_eq!(banner_choice(&h).as_deref(), Some("Elf"));
    assert_eq!(
        app::ui::chosen_line(card_model::Chosen::CreatureType(
            card_model::CreatureType::named("Elf").unwrap()
        )),
        "Chosen type: Elf"
    );
}
