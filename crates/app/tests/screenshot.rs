//! Renders the UI to PNG files for a visual check. Ignored by default (needs a GPU or a
//! software renderer): `APNAP_SHOT_DIR=/some/dir cargo test -p app --test screenshot -- --ignored`.

use app::ui::App;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::{Agent, PlayerId, player_view};

#[test]
#[ignore = "needs a renderer; writes PNGs to $APNAP_SHOT_DIR"]
fn render_menu_and_mid_game() {
    let Some(dir) = std::env::var_os("APNAP_SHOT_DIR").map(std::path::PathBuf::from) else {
        panic!("set APNAP_SHOT_DIR");
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 950.0))
        .with_max_steps(64)
        .wgpu()
        .build_ui_state(
            |ui, app: &mut App| app::ui::ui(app, ui),
            App::with_saves_dir(dir.join("saves")),
        );
    h.run_ok();
    h.render().unwrap().save(dir.join("menu.png")).unwrap();
    h.state_mut().seed = 5;
    h.get_by_label("Start game").click();
    h.run_ok();
    // Let the heuristic AI play our seat for a while so the board fills up.
    let mut brain = ai::HeuristicAgent::new(5, harness::catalog());
    let mut shots = 0;
    for i in 0..400 {
        h.run_ok();
        let s = h.state_mut().session.as_mut().unwrap();
        let Some(d) = s.human_decision().cloned() else {
            break;
        };
        if matches!(i, 60 | 150 | 260)
            || (shots < 4 && d.kind == engine_core::ChoiceKind::DeclareBlockers)
        {
            shots += 1;
            h.run_ok();
            h.render()
                .unwrap()
                .save(dir.join(format!("game-{i:03}.png")))
                .unwrap();
        }
        let s = h.state_mut().session.as_mut().unwrap();
        let r = brain.decide(&player_view(&s.st, PlayerId(0)), &d);
        s.respond(r).unwrap();
    }
    h.run_ok();
    h.render().unwrap().save(dir.join("game-end.png")).unwrap();
}
