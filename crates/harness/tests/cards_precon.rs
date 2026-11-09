//! Tests for real cards in the M4 precons (decisions.md D-034). They need the printed card
//! data in the local Scryfall cache: run `cargo run -p harness -- fetch-cards` once.
//! Every verified script in `crates/cards/scripts` names at least one test in this file.

use engine_core::layers::chars;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, OptionDesc, PlayerId, Primitive, Response};
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scenario() -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), harness::full_catalog(), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

fn drive_until(s: &mut Script, done: impl Fn(&Script) -> bool) {
    for _ in 0..5_000 {
        if s.next.is_none() || done(s) {
            return;
        }
        let r = match &s.decision().prim {
            Primitive::PickN { min, .. } => Response::Pick((0..*min).collect()),
            Primitive::PickNumber { min, .. } => Response::Number(*min),
            Primitive::Allocate { per_min, .. } => Response::Allocate(per_min.clone()),
            Primitive::Order => Response::Order((0..s.options().len()).collect()),
        };
        s.respond(r);
    }
    panic!("drive_until did not converge");
}

/// Every verified real-card script names tests that exist in this file.
#[test]
fn every_verified_real_card_names_existing_tests() {
    let this = include_str!("cards_precon.rs");
    let mut verified = 0;
    for &(path, text) in cards::SCRIPTS {
        let script =
            card_db::parse_real(card_db::Source { path, text }).unwrap_or_else(|e| panic!("{e}"));
        if !script.verified {
            continue;
        }
        verified += 1;
        assert!(!script.tests.is_empty(), "{} names no test", script.name);
        for t in &script.tests {
            assert!(
                this.contains(&format!("fn {t}(")),
                "{} names missing test `{t}`",
                script.name
            );
        }
        assert!(
            harness::full_catalog().find(&script.name).is_some(),
            "{} didn't load",
            script.name
        );
    }
    assert!(verified >= 3);
}

#[test]
fn gigantosaurus_is_a_vanilla_10_10() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Forest"; 5]);
    sc.card(P0, Zone::Hand, "Gigantosaurus");
    let mut s = Script::new(sc.build());
    s.cast("Gigantosaurus").pay().pass_all();
    let id = s
        .find(P0, Zone::Battlefield, "Gigantosaurus")
        .expect("resolved");
    let c = chars(&s.st, id);
    assert_eq!((c.power, c.toughness), (Some(10), Some(10)));
    assert!(c.keywords.is_empty() && c.abilities.is_empty());
    assert_eq!(c.mana_cost.unwrap().mana_value(), 5);
    assert_eq!(c.subtypes, ["Dinosaur"]);
}

#[test]
fn segovian_angel_has_flying_and_vigilance() {
    let mut sc = scenario();
    sc.turn(P0, Step::BeginningOfCombat);
    let angel = sc.card(P0, Zone::Battlefield, "Segovian Angel");
    sc.card(P1, Zone::Battlefield, "Test Bruiser"); // no flying or reach: can't block it
    let mut s = Script::new(sc.build());
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    s.pick(
        "attack",
        |_, o| matches!(o, OptionDesc::Attack { attacker, .. } if *attacker == angel),
    );
    assert!(
        !s.st.object(angel).tapped,
        "vigilance: attacking doesn't tap it"
    );
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert!(
        !s.answered
            .iter()
            .any(|d| d.kind == ChoiceKind::DeclareBlockers),
        "flying: the ground creature was never offered a block"
    );
    assert_eq!(s.st.player(P1).life, 39);
}

#[test]
fn terrian_world_tyrant_is_a_legendary_9_7() {
    let mut sc = scenario();
    let id = sc.card(P0, Zone::Battlefield, "Terrian, World Tyrant");
    let s = Script::new(sc.build());
    let c = chars(&s.st, id);
    assert_eq!((c.power, c.toughness), (Some(9), Some(7)));
    assert!(c.supertypes.contains(&card_model::Supertype::Legendary));
    assert_eq!(c.subtypes, ["Dinosaur", "Ooze"]);
    assert_eq!(c.colors, [card_model::Color::Green]);
}
