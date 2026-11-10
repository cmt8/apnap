//! Renders the UI to PNG files for a visual check. Ignored by default (needs a GPU or a
//! software renderer, and downloads card images into the user's cache):
//! `APNAP_SHOT_DIR=/some/dir cargo test -p app --test screenshot -- --ignored`.
//! `APNAP_SHOT_SETUP` picks the game by its menu label (default: the first one).

use app::art::Art;
use app::ui::App;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::{Agent, PlayerId, player_view};

/// Run frames until every requested image has arrived, then save a PNG.
fn shot(h: &mut Harness<'_, App>, path: std::path::PathBuf) {
    for _ in 0..600 {
        h.run_ok();
        if h.state().art.pending == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    h.run_ok();
    h.render().unwrap().save(path).unwrap();
}

#[test]
#[ignore = "needs a renderer and the network; writes PNGs to $APNAP_SHOT_DIR"]
fn render_menu_and_mid_game() {
    let Some(dir) = std::env::var_os("APNAP_SHOT_DIR").map(std::path::PathBuf::from) else {
        panic!("set APNAP_SHOT_DIR");
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .with_max_steps(64)
        .wgpu()
        .build_ui_state(
            |ui, app: &mut App| app::ui::ui(app, ui),
            App::new(dir.join("saves"), Art::new(true), 0),
        );
    shot(&mut h, dir.join("menu.png"));
    if let Ok(setup) = std::env::var("APNAP_SHOT_SETUP") {
        h.get_by_label(&setup).click();
        h.run_ok();
    }
    h.state_mut().seed = 5;
    h.get_by_label("Start game").click();
    h.run_ok();
    // Let the heuristic AI play our seat for a while so the board fills up.
    let catalog = h.state().session.as_ref().unwrap().st.catalog.clone();
    let mut brain = ai::HeuristicAgent::new(5, catalog);
    let mut shots = 0;
    for i in 0..400 {
        h.run_ok();
        let s = h.state_mut().session.as_mut().unwrap();
        let Some(d) = s.human_decision().cloned() else {
            break;
        };
        if matches!(i, 2 | 60 | 150 | 260)
            || (shots < 4 && d.kind == engine_core::ChoiceKind::DeclareBlockers)
        {
            shots += 1;
            // Point at a card in hand so the enlarged view is shown too.
            let s = h.state().session.as_ref().unwrap();
            let v = s.view();
            let pick = v.hand.first().or(v.battlefield.first());
            let preview = pick.map(|o| app::ui::Preview {
                card: o.card,
                object: Some(o.id),
            });
            h.state_mut().preview = preview;
            shot(&mut h, dir.join(format!("game-{i:03}.png")));
        }
        let s = h.state_mut().session.as_mut().unwrap();
        let r = brain.decide(&player_view(&s.st, PlayerId(0)), &d);
        s.respond(r).unwrap();
    }
    shot(&mut h, dir.join("game-end.png"));
}
