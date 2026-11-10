//! Combat is declared on the table, not through a list: click creatures to attack (and a
//! player's name to choose who), click a blocker and then the attacker it blocks, or drag
//! one onto the other; then one button (or the space bar) confirms the whole declaration.
//! These tests drive that with real pointer presses at the places the cards are drawn.

use app::session::Session;
use app::ui::{App, Screen};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, GameState, ObjectId, PlayerId, Response};
use harness::Agents;
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);

fn scenario(players: u8) -> Scenario {
    let config = if players == 2 {
        GameConfig::constructed_1v1()
    } else {
        GameConfig::free_for_all(players)
    };
    let mut sc = Scenario::new(config, harness::catalog(), 21);
    for p in 0..players {
        sc.library(PlayerId(p), "Forest", 20);
    }
    sc
}

fn harness_for(st: GameState) -> Harness<'static, App> {
    let mut app = App::with_saves_dir(std::env::temp_dir().join("apnap-test-combat-unused"));
    app.session = Some(Session::from_state(st, Some(P0), Agents::Random));
    app.screen = Screen::Game;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .with_max_steps(64)
        .build_ui_state(|ui, app: &mut App| app::ui::ui(app, ui), app);
    h.run_ok();
    h
}

fn card_center(h: &Harness<'_, App>, id: ObjectId) -> egui::Pos2 {
    h.state()
        .card_rects
        .get(&id)
        .unwrap_or_else(|| panic!("{id:?} isn't on the table"))
        .center()
}

fn click_card(h: &mut Harness<'_, App>, id: ObjectId) {
    let pos = card_center(h, id);
    h.drag_at(pos);
    h.run_ok();
    h.drop_at(pos);
    h.run_ok();
}

/// Press on `from`, move to `to`, release.
fn drag(h: &mut Harness<'_, App>, from: egui::Pos2, to: egui::Pos2) {
    h.drag_at(from);
    h.run_ok();
    h.hover_at(from + (to - from) * 0.5);
    h.run_ok();
    h.hover_at(to);
    h.run_ok();
    h.drop_at(to);
    h.run_ok();
}

fn button(h: &mut Harness<'_, App>, label: &str) {
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, label)
        .next()
        .unwrap_or_else(|| panic!("no button {label:?}"))
        .click();
    h.run_ok();
}

fn has_button(h: &Harness<'_, App>, label: &str) -> bool {
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, label)
        .next()
        .is_some()
}

fn logged(h: &Harness<'_, App>, what: &str) -> bool {
    let s = h.state().session.as_ref().unwrap();
    s.log.iter().any(|l| l.contains(what))
}

#[test]
fn attackers_are_chosen_on_the_table_and_sent_at_different_players() {
    let mut sc = scenario(3);
    sc.turn(P0, Step::BeginningOfCombat);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let bruiser = sc.card(P0, Zone::Battlefield, "Test Bruiser");
    let sprout = sc.card(P0, Zone::Battlefield, "Test Sprout");
    let mut h = harness_for(sc.build());
    // The whole attack is planned at once: no creature-by-creature questions.
    let plan = h.state().plan.clone().expect("planning the attack");
    assert_eq!(plan.kind, ChoiceKind::DeclareAttackers);
    assert_eq!(plan.candidates.len(), 3);
    assert_eq!((plan.defenders(), plan.target), (vec![P1, P2], Some(P1)));
    assert!(has_button(&h, "No attacks"));
    assert!(!has_button(&h, "attack P1"), "no list of attack options");

    // "All attack", then "Clear".
    button(&mut h, "All attack");
    assert_eq!(h.state().plan.as_ref().unwrap().choices.len(), 3);
    assert!(has_button(&h, "Attack with 3"));
    button(&mut h, "Clear");
    assert!(h.state().plan.as_ref().unwrap().choices.is_empty());

    // Click a creature: it attacks the chosen player (P1). Click it again: it stands down.
    click_card(&mut h, golem);
    assert_eq!(h.state().plan.as_ref().unwrap().attacks(golem), Some(P1));
    click_card(&mut h, golem);
    assert_eq!(h.state().plan.as_ref().unwrap().attacks(golem), None);
    click_card(&mut h, golem);
    // Click another player's name: the next creature goes there.
    h.query_all_by_label_contains("P2 — ")
        .next()
        .unwrap()
        .click();
    h.run_ok();
    assert_eq!(h.state().plan.as_ref().unwrap().target, Some(P2));
    click_card(&mut h, bruiser);
    // Drag the third creature onto P1's side of the table.
    let p1_side = h.state().player_rects[&P1];
    let from = card_center(&h, sprout);
    drag(&mut h, from, p1_side.center());
    let plan = h.state().plan.clone().unwrap();
    assert_eq!(
        (
            plan.attacks(golem),
            plan.attacks(bruiser),
            plan.attacks(sprout)
        ),
        (Some(P1), Some(P2), Some(P1))
    );
    // Nothing has been declared yet: the engine is still asking.
    assert!(!logged(&h, "attacks"));

    button(&mut h, "Attack with 3");
    assert!(h.state().plan.is_none());
    assert!(logged(&h, "Test Golem (P0) attacks P1"));
    assert!(logged(&h, "Test Bruiser (P0) attacks P2"));
    assert!(logged(&h, "Test Sprout (P0) attacks P1"));
}

#[test]
fn no_attacks_is_one_click_or_the_space_bar() {
    let mut sc = scenario(2);
    sc.turn(P0, Step::BeginningOfCombat);
    sc.cards(P0, Zone::Battlefield, &["Test Golem", "Test Bruiser"]);
    let mut h = harness_for(sc.build());
    assert!(h.state().plan.is_some());
    h.key_press(egui::Key::Space);
    h.run_ok();
    assert!(h.state().plan.is_none());
    assert!(!logged(&h, "attacks"));
    let s = h.state().session.as_ref().unwrap();
    // Two creatures were asked about; neither attacks.
    let declined = s
        .responses
        .iter()
        .filter(|r| matches!(r, Response::Pick(p) if p.is_empty()))
        .count();
    assert_eq!(declined, 2);
}

#[test]
fn blockers_are_assigned_by_clicking_or_dragging_onto_the_attacker() {
    let mut sc = scenario(2);
    sc.turn(P1, Step::BeginningOfCombat);
    let grazer = sc.card(P1, Zone::Battlefield, "Test Grazer"); // 3/3
    let bruiser = sc.card(P1, Zone::Battlefield, "Test Bruiser"); // 2/2
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem"); // 2/3
    let sprout = sc.card(P0, Zone::Battlefield, "Test Sprout"); // 1/1
    let spark = sc.card(P0, Zone::Battlefield, "Test Spark"); // 1/1
    // P1 attacks with both creatures; then the game is handed to the app.
    let mut s = Script::new(sc.build());
    while s.kind() != ChoiceKind::DeclareBlockers {
        s.respond(Response::Pick(vec![0])); // attack P0 / pass priority
    }
    let mut h = harness_for(s.st);
    let plan = h.state().plan.clone().expect("planning the block");
    assert_eq!(plan.kind, ChoiceKind::DeclareBlockers);
    assert_eq!(plan.candidates.len(), 3);
    assert!(has_button(&h, "No blocks"));

    // Click a blocker: it waits for its attacker. Click the attacker: blocked.
    click_card(&mut h, golem);
    assert_eq!(h.state().plan.as_ref().unwrap().selected, Some(golem));
    assert!(
        h.query_by_label_contains("Now click the attacker Test Golem blocks")
            .is_some()
    );
    click_card(&mut h, grazer);
    let plan = h.state().plan.clone().unwrap();
    assert_eq!((plan.blocks(golem), plan.selected), (Some(grazer), None));
    // Drag another blocker onto the other attacker.
    let (from, to) = (card_center(&h, sprout), card_center(&h, bruiser));
    drag(&mut h, from, to);
    assert_eq!(
        h.state().plan.as_ref().unwrap().blocks(sprout),
        Some(bruiser)
    );
    // Select the third and think better of it.
    click_card(&mut h, spark);
    click_card(&mut h, spark);
    let plan = h.state().plan.clone().unwrap();
    assert_eq!((plan.selected, plan.blocks(spark)), (None, None));
    assert!(
        !logged(&h, "blocks"),
        "nothing is declared until it is confirmed"
    );

    button(&mut h, "Block with 2");
    assert!(logged(&h, "Test Golem (P0) blocks Test Grazer (P1)"));
    assert!(logged(&h, "Test Sprout (P0) blocks Test Bruiser (P1)"));
    assert!(!logged(&h, "Test Spark (P0) blocks"));
}

#[test]
fn four_players_sit_in_a_grid() {
    let mut sc = scenario(4);
    for p in [P0, P1, P2, P3] {
        sc.card(p, Zone::Battlefield, "Test Golem");
    }
    sc.card(P0, Zone::Hand, "Test Golem");
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    let h = harness_for(sc.build());
    let seat = |p: PlayerId| h.state().player_rects[&p];
    // The viewer bottom left, then clockwise in turn order: P1 above, P2 top right, P3
    // bottom right.
    let (me, p1, p2, p3) = (seat(P0), seat(P1), seat(P2), seat(P3));
    assert!(
        p1.bottom() <= me.top() && p2.bottom() <= p3.top(),
        "two rows"
    );
    assert!(
        p1.right() <= p2.left() && me.right() <= p3.left(),
        "two columns"
    );
    assert!((p1.left() - me.left()).abs() < 2.0 && (p2.left() - p3.left()).abs() < 2.0);
    assert!((p1.top() - p2.top()).abs() < 2.0 && (me.top() - p3.top()).abs() < 2.0);
    // Equal shares of the table.
    assert!((p1.width() - p2.width()).abs() < 2.0 && (me.width() - p3.width()).abs() < 2.0);
    // Everything is on screen.
    let screen = h.ctx.content_rect();
    for r in [me, p1, p2, p3] {
        assert!(screen.contains_rect(r), "{r:?}");
    }
}

#[test]
fn options_on_the_table_are_not_also_listed_unless_asked_for() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    let mut h = harness_for(sc.build());
    // The card in hand is how the spell is cast; the main button passes priority.
    assert!(has_button(&h, "Pass priority"));
    assert!(!has_button(&h, "cast Test Shock"));
    assert!(h.query_by_label_contains("outlined in gold").is_some());
    h.get_by_label("List every option").click();
    h.run_ok();
    assert!(has_button(&h, "cast Test Shock"));
    h.get_by_label("List every option").click();
    h.run_ok();
    assert!(!has_button(&h, "cast Test Shock"));
    // The space bar is the main button: it passes priority.
    let before = h.state().session.as_ref().unwrap().responses.len();
    h.key_press(egui::Key::Space);
    h.run_ok();
    assert!(h.state().session.as_ref().unwrap().responses.len() > before);
}

/// CR 702.111b: a creature with menace can't be blocked except by two or more creatures.
/// The engine sends a single block back (CR 734); the app says why and the block is
/// planned again.
#[test]
fn an_illegal_block_is_sent_back_with_the_reason() {
    let mut sc = scenario(2);
    sc.turn(P1, Step::BeginningOfCombat);
    let marauder = sc.card(P1, Zone::Battlefield, "Test Marauder"); // menace
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let sprout = sc.card(P0, Zone::Battlefield, "Test Sprout");
    let mut s = Script::new(sc.build());
    while s.kind() != ChoiceKind::DeclareBlockers {
        s.respond(Response::Pick(vec![0]));
    }
    let mut h = harness_for(s.st);
    // One attacker: a click on a blocker is enough to block it.
    click_card(&mut h, golem);
    assert_eq!(
        h.state().plan.as_ref().unwrap().blocks(golem),
        Some(marauder)
    );
    button(&mut h, "Block with 1");
    assert!(h.state().status.contains("menace"), "{}", h.state().status);
    assert!(!logged(&h, "blocks"));
    let plan = h.state().plan.clone().expect("planning the block again");
    assert!(plan.choices.is_empty());
    // Two blockers are fine.
    click_card(&mut h, golem);
    click_card(&mut h, sprout);
    button(&mut h, "Block with 2");
    assert!(logged(&h, "Test Golem (P0) blocks Test Marauder (P1)"));
    assert!(logged(&h, "Test Sprout (P0) blocks Test Marauder (P1)"));
    assert!(h.state().status.is_empty());
}
