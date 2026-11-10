//! M4 acceptance (spec §13): "The human can complete a game in the app using only the mouse:
//! every `ChoiceKind` has a widget" and "Save/load and replay a game from file in the app".
//!
//! A robot plays through the real UI headlessly (egui_kittest): it asks the heuristic AI
//! what to answer, then gives that answer **only by clicking widgets**, exactly as a person
//! with a mouse would. No response is ever submitted except through a click.

use app::ui::{App, option_label};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::decision::{PendingDecision, Primitive, Response};
use engine_core::{Agent, PlayerId, player_view};
use std::collections::BTreeSet;
use std::path::PathBuf;

const HUMAN: PlayerId = PlayerId(0);

fn harness(saves: PathBuf) -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(1500.0, 1000.0))
        .with_max_steps(64)
        .build_ui_state(
            |ui, app: &mut App| app::ui::ui(app, ui),
            App::with_saves_dir(saves),
        )
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("apnap-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Click the `nth` button whose label is exactly `label`.
fn click(h: &mut Harness<'_, App>, label: &str, nth: usize) {
    let node = h
        .query_all_by_role_and_label(egui::accesskit::Role::Button, label)
        .nth(nth)
        .unwrap_or_else(|| panic!("no button #{nth} labelled {label:?}"));
    node.click();
    h.run_ok();
}

/// Give `r` for decision `d` using only clicks.
fn answer_by_clicking(h: &mut Harness<'_, App>, d: &PendingDecision, r: &Response) {
    let labels: Vec<String> = {
        let s = h.state().session.as_ref().unwrap();
        let v = s.view();
        d.options
            .iter()
            .map(|o| option_label(&v, &s.st.catalog, d, o))
            .collect()
    };
    // Which of several identically-labelled buttons is option `i`.
    let nth = |i: usize| labels[..i].iter().filter(|l| **l == labels[i]).count();
    match (&d.prim, r) {
        (Primitive::PickN { max, .. }, Response::Pick(picks)) if *max <= 1 => match picks.first() {
            Some(&i) => click(h, &labels[i], nth(i)),
            None => {
                let none = match d.kind {
                    engine_core::ChoiceKind::DeclareAttackers => "Don't attack",
                    engine_core::ChoiceKind::DeclareBlockers => "Don't block",
                    _ => "None",
                };
                click(h, none, 0);
            }
        },
        (Primitive::PickN { .. }, Response::Pick(picks)) => {
            for &i in picks {
                click(h, &labels[i], nth(i));
            }
            click(h, "Confirm", 0);
        }
        (Primitive::PickNumber { min, .. }, Response::Number(n)) => {
            click(h, &format!("Min ({min})"), 0);
            for _ in *min..*n {
                click(h, "+", 0);
            }
            click(h, "Confirm", 0);
        }
        (Primitive::Allocate { per_min, .. }, Response::Allocate(alloc)) => {
            for (i, (&want, &start)) in alloc.iter().zip(per_min).enumerate() {
                for _ in start..want {
                    click(h, &format!("+ {}", labels[i]), nth(i));
                }
            }
            click(h, "Confirm", 0);
        }
        (Primitive::Order, Response::Order(order)) => {
            // Bubble the on-screen order (initially 0..n) into the wanted order with "Up" clicks.
            let mut cur: Vec<usize> = (0..order.len()).collect();
            for (pos, &want) in order.iter().enumerate() {
                let mut at = cur.iter().position(|x| *x == want).unwrap();
                while at > pos {
                    click(h, &format!("Up {}", at + 1), 0);
                    cur.swap(at, at - 1);
                    at -= 1;
                }
            }
            click(h, "Confirm", 0);
        }
        (p, r) => panic!("response {r:?} doesn't fit {p:?}"),
    }
}

/// Play the current game to the end through the UI. Returns the decision kinds answered.
fn play_out(h: &mut Harness<'_, App>, seed: u64, max_decisions: usize) -> BTreeSet<String> {
    let catalog = h
        .state()
        .session
        .as_ref()
        .expect("in a game")
        .st
        .catalog
        .clone();
    let mut brain = ai::HeuristicAgent::new(seed, catalog);
    let mut kinds = BTreeSet::new();
    for _ in 0..max_decisions {
        h.run_ok();
        let s = h.state().session.as_ref().expect("in a game");
        if s.is_over() {
            return kinds;
        }
        let d = s.human_decision().expect("the human is to decide").clone();
        let r = brain.decide(&player_view(&s.st, HUMAN), &d);
        let before = s.st.decision_count;
        kinds.insert(format!("{:?}/{}", d.kind, shape(&d.prim)));
        answer_by_clicking(h, &d, &r);
        let after = h.state().session.as_ref().unwrap().st.decision_count;
        assert!(
            after > before,
            "clicking didn't answer {:?} ({:?})",
            d.kind,
            d.prim
        );
    }
    panic!("game not finished after {max_decisions} human decisions");
}

fn shape(p: &Primitive) -> &'static str {
    match p {
        Primitive::PickN { .. } => "PickN",
        Primitive::PickNumber { .. } => "Number",
        Primitive::Allocate { .. } => "Allocate",
        Primitive::Order => "Order",
    }
}

fn start(h: &mut Harness<'_, App>, setup_label: &str, seed: u64) {
    h.run_ok();
    h.get_by_label(setup_label).click();
    h.run_ok();
    h.state_mut().seed = seed;
    click(h, "Start game", 0);
    assert!(h.state().session.is_some());
}

#[test]
fn a_game_can_be_played_to_the_end_with_the_mouse_alone() {
    let mut kinds = BTreeSet::new();
    for (setup, seed, auto_pay) in [
        ("Commander — 1 v 1 (test decks)", 3, true),
        ("Commander — 4 players (test decks)", 5, true),
        ("60-card duel (M2 test deck)", 13, false),
        ("60-card duel (M2 test deck)", 45, true),
        // Real cards (needs the Scryfall cache: `cargo run -p harness -- fetch-cards`).
        ("60-card duel — real cards (Stompy vs Angels)", 2, true),
        ("60-card free-for-all — 4 players, real cards", 3, false),
    ] {
        let mut h = harness(temp_dir("play"));
        start(&mut h, setup, seed);
        if !auto_pay {
            h.get_by_label("Auto-pay mana").click();
            h.run_ok();
            assert!(!h.state().session.as_ref().unwrap().stops.auto_pay);
        }
        kinds.extend(play_out(&mut h, seed, 6_000));
        let s = h.state().session.as_ref().unwrap();
        assert!(s.is_over(), "{setup} seed {seed}");
        assert!(h.query_by_label_contains("Game over").is_some());
    }
    // PickN and Number widgets were exercised here; Allocate and Order have their own
    // targeted tests in `widgets.rs`.
    for shape in ["PickN", "Number"] {
        assert!(
            kinds.iter().any(|k| k.ends_with(shape)),
            "no {shape} decision seen: {kinds:?}"
        );
    }
    for kind in [
        "Priority",
        "Mulligan",
        "DeclareAttackers",
        "DeclareBlockers",
        "ChooseTargets",
        "PayMana",
    ] {
        assert!(
            kinds.iter().any(|k| k.starts_with(kind)),
            "no {kind} decision seen: {kinds:?}"
        );
    }
    println!("decision kinds answered by clicking: {kinds:#?}");
}

#[test]
fn save_load_and_replay_from_file() {
    let dir = temp_dir("save");
    let mut h = harness(dir.clone());
    start(&mut h, "60-card duel (M2 test deck)", 7);
    let mut brain = ai::HeuristicAgent::new(7, harness::catalog());
    // Play 40 human decisions, then save from the UI.
    for _ in 0..40 {
        h.run_ok();
        let s = h.state().session.as_ref().unwrap();
        let Some(d) = s.human_decision().cloned() else {
            break;
        };
        let r = brain.decide(&player_view(&s.st, HUMAN), &d);
        answer_by_clicking(&mut h, &d, &r);
    }
    click(&mut h, "Save", 0);
    let (hash, log_hash, responses) = {
        let s = h.state().session.as_ref().unwrap();
        (s.st.state_hash(), s.st.log_hash, s.responses.len())
    };
    assert_eq!(h.state().saves.len(), 1, "{}", h.state().status);
    let file = h.state().saves[0]
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // Back to the menu, continue the saved game: identical state.
    click(&mut h, "Menu", 0);
    click(&mut h, &format!("Continue {file}"), 0);
    let s = h.state().session.as_ref().unwrap();
    assert_eq!(
        (s.st.state_hash(), s.st.log_hash, s.responses.len()),
        (hash, log_hash, responses)
    );
    assert!(s.human_decision().is_some() || s.is_over());

    // Watch the replay: stepping through every recorded response reaches the same state.
    click(&mut h, "Menu", 0);
    click(&mut h, &format!("Watch replay {file}"), 0);
    assert_eq!(h.state().session.as_ref().unwrap().responses.len(), 0);
    for _ in 0..(responses / 25 + 1) {
        click(&mut h, "Step 25", 0);
    }
    let s = h.state().session.as_ref().unwrap();
    assert_eq!(
        (s.st.state_hash(), s.st.log_hash),
        (hash, log_hash),
        "replay diverged"
    );
    let _ = std::fs::remove_dir_all(dir);
}
