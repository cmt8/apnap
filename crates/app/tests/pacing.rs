//! The opponents' turns must be visible. Before this, the session ran every AI decision in
//! one frame, so a human with nothing to do saw their own turns 1, 5 and 9 back to back —
//! "it let me play three lands on turn 1".

use app::session::Session;
use app::ui::App;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::decision::OptionDesc;
use engine_core::{Agent, PlayerId, Response, player_view};
use harness::{Agents, Setup};

const P0: PlayerId = PlayerId(0);

/// Play `human_decisions` decisions for seat 0 with the heuristic brain; between them let
/// the AI seats run with `advance`. Returns the session.
fn play(seed: u64, human_decisions: usize, mut advance: impl FnMut(&mut Session)) -> Session {
    let mut s = Session::new(Setup::M2 { players: 4 }, seed, Some(P0), Agents::Heuristic);
    let mut brain = ai::HeuristicAgent::new(seed, s.st.catalog.clone());
    for _ in 0..human_decisions {
        advance(&mut s);
        let Some(d) = s.human_decision().cloned() else {
            break;
        };
        let r = brain.decide(&player_view(&s.st, P0), &d);
        s.respond(r).unwrap();
    }
    s
}

#[test]
fn pausing_for_ai_actions_shows_each_one_and_changes_nothing() {
    // (turn, log lines added) for every pause.
    let mut pauses: Vec<(u32, usize)> = Vec::new();
    let paced = play(11, 60, |s| {
        loop {
            let (turn, logged) = (s.st.turn.turn_number, s.log.len());
            if !s.pump_visible(5_000) {
                break;
            }
            // It stopped because an AI seat did something the log shows…
            assert!(s.log.len() > logged);
            // …and no further than the turn after the one it started in.
            assert!(s.st.turn.turn_number <= turn + 1);
            pauses.push((s.st.turn.turn_number, s.log.len() - logged));
        }
    });
    // Each opponent's turn is seen as several separate steps, not skipped in one go.
    let on_turn = |t: u32| pauses.iter().filter(|(turn, _)| *turn == t).count();
    let busiest = (1..=paced.st.turn.turn_number).map(on_turn).max().unwrap();
    assert!(busiest >= 2, "pauses: {pauses:?}");
    assert!(pauses.len() >= 20, "pauses: {pauses:?}");

    // Pausing is presentation only: the game is the same as when the AI runs flat out.
    let instant = play(11, 60, |s| {
        s.pump(5_000);
    });
    assert_eq!(paced.responses, instant.responses);
    assert_eq!(paced.st.state_hash(), instant.st.state_hash());
    assert_eq!(paced.log, instant.log);
}

#[test]
fn only_one_land_can_be_played_each_turn() {
    // CR 305.2: after a land is played, no land play is offered again that turn — the
    // human's land plays happen on strictly increasing turns, all their own.
    let mut s = Session::new(Setup::M2 { players: 4 }, 3, Some(P0), Agents::Heuristic);
    let mut land_turns: Vec<u32> = Vec::new();
    for _ in 0..400 {
        s.pump(10_000);
        let Some(d) = s.human_decision().cloned() else {
            break;
        };
        let land = d
            .options
            .iter()
            .position(|o| matches!(o, OptionDesc::PlayLand { .. }));
        let r = match land {
            Some(i) => {
                assert_eq!(s.st.turn.active, P0);
                land_turns.push(s.st.turn.turn_number);
                Response::Pick(vec![i])
            }
            None => {
                let mut brain = ai::RandomAgent::new(s.responses.len() as u64);
                brain.decide(&player_view(&s.st, P0), &d)
            }
        };
        s.respond(r).unwrap();
        if land_turns.len() >= 5 {
            break;
        }
    }
    assert!(land_turns.len() >= 3, "{land_turns:?}");
    assert!(land_turns.windows(2).all(|w| w[0] < w[1]), "{land_turns:?}");
}

#[test]
fn the_app_waits_after_an_opponent_acts() {
    let dir = std::env::temp_dir().join(format!("apnap-test-pacing-{}", std::process::id()));
    // A pause far longer than the test: once the app pauses, it stays paused.
    let app = App::new(dir, app::art::Art::disabled(), 3_600_000);
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
    let catalog = h.state().session.as_ref().unwrap().st.catalog.clone();
    let mut brain = ai::HeuristicAgent::new(11, catalog);
    // Answer the human's decisions until the app is waiting on an opponent's action.
    let mut paused = false;
    for _ in 0..200 {
        h.step();
        let s = h.state_mut().session.as_mut().unwrap();
        match s.human_decision().cloned() {
            Some(d) => {
                let r = brain.decide(&player_view(&s.st, P0), &d);
                s.respond(r).unwrap();
            }
            None => {
                paused = true;
                break;
            }
        }
    }
    assert!(paused, "an opponent never acted");
    let before = h.state().session.as_ref().unwrap().responses.len();
    for _ in 0..5 {
        h.step();
    }
    let s = h.state().session.as_ref().unwrap();
    assert_eq!(
        s.responses.len(),
        before,
        "the game ran on during the pause"
    );
    assert!(!s.is_over());
    assert!(h.query_by_label_contains("is acting").is_some());
    // "Instant" turns the pauses off: the game runs to the human's next decision.
    h.get_by_label("Opponents act:").click();
    h.step();
    h.step();
    h.get_by_label("Instant").click();
    h.step();
    h.step();
    let s = h.state().session.as_ref().unwrap();
    assert!(s.responses.len() > before);
    assert!(s.human_decision().is_some() || s.is_over());
}
