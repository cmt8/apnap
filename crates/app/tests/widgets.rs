//! Targeted widget tests: decisions that random games rarely give the human are set up as
//! scenarios and answered through the UI by clicking (Allocate, Order, multi-select PickN),
//! plus clicking cards and players on the board instead of the option buttons.

use app::session::Session;
use app::ui::{App, Screen};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, GameState, PlayerId, Primitive};
use harness::Agents;
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scenario() -> Scenario {
    let mut sc = Scenario::new(GameConfig::constructed_1v1(), harness::catalog(), 21);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

fn harness_for(st: GameState) -> Harness<'static, App> {
    let mut app = App::with_saves_dir(std::env::temp_dir().join("apnap-test-widgets-unused"));
    app.session = Some(Session::from_state(st, Some(P0), Agents::Random));
    app.screen = Screen::Game;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 1000.0))
        .with_max_steps(64)
        .build_ui_state(|ui, app: &mut App| app::ui::ui(app, ui), app);
    h.run_ok();
    h
}

fn button(h: &mut Harness<'_, App>, label: &str) {
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, label)
        .next()
        .unwrap_or_else(|| panic!("no button {label:?}"))
        .click();
    h.run_ok();
}

fn button_containing(h: &mut Harness<'_, App>, text: &str) {
    h.query_all_by_label_contains(text)
        .next()
        .unwrap_or_else(|| panic!("nothing labelled like {text:?}"))
        .click();
    h.run_ok();
}

fn decision(h: &Harness<'_, App>) -> (ChoiceKind, Primitive) {
    let d = h
        .state()
        .session
        .as_ref()
        .unwrap()
        .human_decision()
        .expect("human decision")
        .clone();
    (d.kind, d.prim)
}

/// CR 510.1c: divide a blocked attacker's damage among its blockers — the Allocate widget.
#[test]
fn allocate_widget_divides_combat_damage() {
    let mut sc = scenario();
    sc.turn(P0, Step::BeginningOfCombat);
    sc.card(P0, Zone::Battlefield, "Test Grazer"); // 3/3
    sc.cards(P1, Zone::Battlefield, &["Test Sprout", "Test Spark"]); // two 1/1s
    let mut s = Script::new(sc.build());
    // Drive the engine to the damage division: P0 attacks, P1 double-blocks.
    while s.kind() != ChoiceKind::AssignCombatDamage {
        let r = match s.kind() {
            ChoiceKind::DeclareAttackers | ChoiceKind::DeclareBlockers => {
                engine_core::Response::Pick(vec![0])
            }
            _ => engine_core::Response::Pick(vec![0]), // pass
        };
        s.respond(r);
    }
    let mut h = harness_for(s.st);
    let (kind, prim) = decision(&h);
    assert_eq!(kind, ChoiceKind::AssignCombatDamage);
    assert!(matches!(prim, Primitive::Allocate { total: 3, .. }));
    // Confirm is disabled until all 3 damage is assigned: 2 to the Sprout, 1 to the Spark.
    button_containing(&mut h, "+ Test Sprout");
    button_containing(&mut h, "+ Test Sprout");
    button_containing(&mut h, "+ Test Spark");
    button(&mut h, "Confirm");
    h.run_ok();
    let s = h.state().session.as_ref().unwrap();
    assert!(
        s.log.iter().any(|l| l.contains("deals 2 to Test Sprout")),
        "{:?}",
        s.log
    );
    assert!(s.log.iter().any(|l| l.contains("deals 1 to Test Spark")));
}

/// CR 603.3b: order your triggered abilities — the Order widget. Also plays the spell by
/// clicking the card in hand and the target on the board, not the option buttons.
#[test]
fn order_widget_orders_triggers_and_cards_are_clickable() {
    let mut sc = scenario();
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Watcher", "Test Martyr", "Mountain", "Mountain"],
    );
    sc.card(P0, Zone::Hand, "Test Doom");
    let mut h = harness_for(sc.build());
    assert_eq!(decision(&h).0, ChoiceKind::Priority);
    // Click the card in hand (it has exactly one option: cast it)…
    button_containing(&mut h, "Test Doom  {1}{R}");
    assert_eq!(decision(&h).0, ChoiceKind::ChooseTargets { slot: 0 });
    // …then click the creature on the battlefield as the target (our own Martyr).
    button_containing(&mut h, "Test Martyr  {1}{G}");
    // Mana is paid automatically and, with nothing else to do, the session passes priority
    // for us. Doom resolves and the Martyr dies: its own trigger and the Watcher's trigger
    // both belong to P0.
    let (kind, prim) = decision(&h);
    assert_eq!((kind, prim), (ChoiceKind::OrderTriggers, Primitive::Order));
    button(&mut h, "Down 1"); // swap the two
    button(&mut h, "Confirm");
    let s = h.state().session.as_ref().unwrap();
    // Both triggers went on the stack and (with automatic passes) have resolved: the Martyr's
    // gains 2 life (its power) and the Watcher's gains 1.
    assert_eq!(s.st.player(P0).life, 23);
    // The trigger listed last went on the stack last (it resolves first).
    let order: Vec<_> = s
        .responses
        .iter()
        .filter_map(|r| match r {
            engine_core::Response::Order(o) => Some(o.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(order, [vec![1, 0]]);
}

/// A multi-select PickN (discard two at cleanup) with toggle buttons and Confirm.
#[test]
fn multi_select_widget_discards_to_hand_size() {
    let mut sc = scenario();
    sc.turn(P0, Step::End);
    sc.cards(P0, Zone::Hand, &["Forest"; 5]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Sprout", "Test Golem", "Test Giant", "Test Brute"],
    );
    let mut h = harness_for(sc.build());
    let (kind, prim) = decision(&h);
    assert_eq!(kind, ChoiceKind::Discard);
    assert_eq!(prim, Primitive::PickN { min: 2, max: 2 });
    // Toggle one on and off again, then choose two by clicking cards in the hand row.
    button_containing(&mut h, "Test Giant  {3}{R}{R}");
    button_containing(&mut h, "Test Giant  {3}{R}{R}");
    assert!(h.state().picked.is_empty());
    button_containing(&mut h, "Test Giant  {3}{R}{R}");
    button_containing(&mut h, "Test Brute  {2}{R}");
    button(&mut h, "Confirm");
    let s = h.state().session.as_ref().unwrap();
    let discarded: Vec<&String> = s.log.iter().filter(|l| l.contains("P0 discards")).collect();
    assert_eq!(discarded.len(), 2, "{:?}", s.log);
    assert!(discarded.iter().any(|l| l.contains("Test Giant")));
    assert!(discarded.iter().any(|l| l.contains("Test Brute")));
    let discards = s
        .responses
        .iter()
        .any(|r| matches!(r, engine_core::Response::Pick(p) if p.len() == 2));
    assert!(discards);
}

/// Clicking a player header chooses the option anchored to that player (a target).
#[test]
fn players_are_clickable_targets() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    let mut h = harness_for(sc.build());
    button_containing(&mut h, "Test Shock  {R}");
    assert_eq!(decision(&h).0, ChoiceKind::ChooseTargets { slot: 0 });
    button_containing(&mut h, "P1 — 20 life");
    button(&mut h, "Pass priority");
    let s = h.state().session.as_ref().unwrap();
    assert_eq!(s.st.player(P1).life, 18);
}

/// Pointing at a card shows it enlarged (next to the pointer, not in a side panel); moving
/// off it takes the enlarged view away.
#[test]
fn pointing_at_a_card_enlarges_it() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Shock");
    let mut h = harness_for(sc.build());
    assert!(h.query_by_label("Test Golem (enlarged)").is_none());
    assert!(h.state().zoomed.is_none());
    for name in ["Test Golem", "Test Shock"] {
        h.query_all_by_label_contains(&format!("{name}  "))
            .next()
            .unwrap_or_else(|| panic!("no card {name}"))
            .hover();
        h.run_ok();
        assert!(
            h.query_by_label(&format!("{name} (enlarged)")).is_some(),
            "{name}"
        );
        let zoomed = h.state().zoomed.expect("a card is enlarged");
        let s = h.state().session.as_ref().unwrap();
        assert_eq!(s.st.catalog.get(zoomed.card).name, name);
    }
    h.get_by_label("Menu").hover();
    h.run_ok();
    assert!(h.query_by_label("Test Shock (enlarged)").is_none());
    assert!(h.state().zoomed.is_none());
}

/// Each player's plate shows their life; hand, library and graveyard are icons with a
/// count; exile appears only when that player has exiled cards. Clicking the graveyard or
/// exile counter opens the pile to look through.
#[test]
fn graveyards_and_exile_can_be_looked_through() {
    use app::ui::Pile;
    let mut sc = scenario();
    sc.cards(P0, Zone::Graveyard, &["Test Golem", "Test Sprout"]);
    sc.card(P1, Zone::Exile, "Test Giant");
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    let mut h = harness_for(sc.build());
    for label in [
        "P0 (you) — 20 life",
        "P1 — 20 life",
        "P0 hand: 1 card",
        "P0 library: 20 cards",
        "P0 graveyard: 2 cards",
        "P1 graveyard: 0 cards",
        "P1 exile: 1 card",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert!(
        h.query_by_label_contains("P0 exile").is_none(),
        "nothing exiled: not shown"
    );
    let cards_named = |h: &Harness<'_, App>, name: &str| {
        h.query_all_by_label_contains(&format!("{name}  ")).count()
    };
    assert_eq!(cards_named(&h, "Test Golem"), 0);

    button(&mut h, "P0 graveyard: 2 cards");
    assert_eq!(h.state().zone_view, Some((P0, Pile::Graveyard)));
    assert!(h.query_by_label_contains("Your graveyard").is_some());
    assert_eq!(cards_named(&h, "Test Golem"), 1);
    assert_eq!(cards_named(&h, "Test Sprout"), 1);
    // Pointing at a card in the pile enlarges it like any other.
    h.query_all_by_label_contains("Test Golem  ")
        .next()
        .unwrap()
        .hover();
    h.run_ok();
    assert!(h.query_by_label("Test Golem (enlarged)").is_some());
    // Clicking the counter again closes the pile.
    button(&mut h, "P0 graveyard: 2 cards");
    assert_eq!(h.state().zone_view, None);
    assert_eq!(cards_named(&h, "Test Golem"), 0);

    button(&mut h, "P1 exile: 1 card");
    assert_eq!(h.state().zone_view, Some((P1, Pile::Exile)));
    assert!(h.query_by_label_contains("P1's exiled cards").is_some());
    assert_eq!(cards_named(&h, "Test Giant"), 1);
    // So does Escape.
    h.key_press(egui::Key::Escape);
    h.run_ok();
    assert_eq!(h.state().zone_view, None);
}
