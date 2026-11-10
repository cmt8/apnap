//! Renders the UI to PNG files for a visual check. Ignored by default (needs a GPU or a
//! software renderer, and downloads card images into the user's cache):
//! `APNAP_SHOT_DIR=/some/dir cargo test -p app --test screenshot -- --ignored`.
//! `APNAP_SHOT_SETUP` picks the game by its menu label (default: the first one);
//! `APNAP_SHOT_SIZE=2560x1440` sets the window size.

use app::art::Art;
use app::ui::App;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::{Agent, PlayerId, player_view};

/// The window size: `APNAP_SHOT_SIZE=2560x1440`, default 1600x1000.
fn size() -> egui::Vec2 {
    std::env::var("APNAP_SHOT_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some(egui::vec2(w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(egui::vec2(1600.0, 1000.0))
}

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
        .with_size(size())
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
    // (attacks, blocks) pictured so far.
    let mut combat_shots = (0, 0);
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
            // Point at a card so the enlarged view next to the pointer is shown too:
            // a creature on the battlefield if there is one, otherwise a card in hand.
            let s = h.state().session.as_ref().unwrap();
            let v = s.view();
            let pick = v
                .battlefield
                .iter()
                .find(|o| o.power.is_some())
                .or(v.hand.first())
                .map(|o| format!("{}  ", o.name));
            if let Some(label) = pick {
                if let Some(card) = h.query_all_by_label_contains(&label).next() {
                    card.hover();
                }
                h.run_ok();
            }
            shot(&mut h, dir.join(format!("game-{i:03}.png")));
        }
        // Combat as it looks while it is being planned on the table: everything attacks,
        // or every creature that can blocks the first attacker it could.
        let planning = matches!(
            d.kind,
            engine_core::ChoiceKind::DeclareAttackers | engine_core::ChoiceKind::DeclareBlockers
        );
        let attacking = d.kind == engine_core::ChoiceKind::DeclareAttackers;
        let wanted = if attacking {
            combat_shots.0 < 3 && i > 20
        } else {
            combat_shots.1 < 4
        };
        if planning
            && wanted
            && let Some(plan) = h.state_mut().plan.as_mut()
        {
            if plan.attacking() {
                plan.all_attack();
            } else {
                for c in plan.candidates.clone() {
                    if let Some(engine_core::OptionDesc::Block { attacker, .. }) = c.options.first()
                    {
                        plan.drop_on_creature(c.creature, *attacker);
                    }
                }
            }
            let what = if plan.attacking() { "attack" } else { "block" };
            if !plan.choices.is_empty() {
                if attacking {
                    combat_shots.0 += 1;
                } else {
                    combat_shots.1 += 1;
                }
                h.remove_cursor();
                shot(&mut h, dir.join(format!("{what}-{i:03}.png")));
            }
        }
        let s = h.state_mut().session.as_mut().unwrap();
        let r = brain.decide(&player_view(&s.st, PlayerId(0)), &d);
        s.respond(r).unwrap();
    }
    shot(&mut h, dir.join("game-end.png"));
}
