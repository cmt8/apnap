//! The table must not jump around while a game is played: the bars around it keep their
//! size whether or not the human has something to decide, whose turn it is, and whether
//! card images are loading. (Reported as "random flickering (weird resizing) during
//! actions".)

use app::ui::App;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::{Agent, PlayerId, player_view};
use std::collections::BTreeSet;

const P0: PlayerId = PlayerId(0);

#[test]
fn the_table_keeps_its_place_as_decisions_come_and_go() {
    let dir = std::env::temp_dir().join(format!("apnap-test-layout-{}", std::process::id()));
    // A pause after every opponent action, so there are frames with nothing to decide.
    let app = App::new(dir, app::art::Art::disabled(), 1);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 1000.0))
        .with_max_steps(8)
        .build_ui_state(|ui, app: &mut App| app::ui::ui(app, ui), app);
    h.step();
    h.get_by_label("60-card free-for-all — 4 players (M2 test deck)")
        .click();
    h.step();
    h.state_mut().seed = 11;
    h.get_by_label("Start game").click();
    h.step();
    h.step();
    let catalog = h.state().session.as_ref().unwrap().st.catalog.clone();
    let mut brain = ai::HeuristicAgent::new(11, catalog);
    // Every place the table was drawn while the human held cards (an empty hand gives its
    // row to the table, on purpose), and what the frames looked like.
    let mut places: BTreeSet<[i32; 4]> = BTreeSet::new();
    let (mut deciding, mut waiting, mut turns) = (0, 0, BTreeSet::new());
    let mut had_hand = false;
    for _ in 0..600 {
        std::thread::sleep(std::time::Duration::from_millis(1));
        h.step();
        let table = h.state().table_rect;
        let s = h.state_mut().session.as_mut().unwrap();
        if s.is_over() {
            break;
        }
        let hand = !s.view().hand.is_empty();
        turns.insert(s.st.turn.active);
        let d = s.human_decision().cloned();
        match &d {
            Some(_) => deciding += 1,
            None => waiting += 1,
        }
        // (A panel takes its new size one frame after its content changes: skip the frame
        // in which the hand went from empty to not.)
        let settled = hand == had_hand;
        had_hand = hand;
        if hand && settled {
            places.insert([
                table.min.x.round() as i32,
                table.min.y.round() as i32,
                table.max.x.round() as i32,
                table.max.y.round() as i32,
            ]);
        }
        if let Some(d) = d {
            let r = brain.decide(&player_view(&s.st, P0), &d);
            s.respond(r).unwrap();
        }
    }
    assert!(deciding > 20 && waiting > 20, "{deciding} / {waiting}");
    assert!(turns.len() > 1, "only one player's turn was seen");
    assert_eq!(places.len(), 1, "the table moved: {places:?}");
}
