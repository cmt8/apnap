//! The most played Commander cards that no feature family covers (decisions.md D-075):
//! each feature built for them, with invented cards in Forge's script format compiled the
//! way real ones are and then played. Rules text: docs/rules-notes/d075-commander-staples.md.

use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, ObjectId, OptionDesc, PlayerId, Primitive, Response};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

fn scenario_n(players: u8, cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(
        GameConfig::commander(players),
        forge_test_catalog(cards, &[]),
        31,
    );
    for p in 0..players {
        sc.library(PlayerId(p), "Forest", 20);
    }
    sc
}

fn scenario(cards: &[&str]) -> Scenario {
    scenario_n(2, cards)
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

/// A card in Forge's format: `body` is the ability lines.
fn card(name: &str, cost: &str, types: &str, body: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{types}\n{body}\nOracle:Text.")
}

/// Answer everything with the least it allows (pass, pick nothing) until `done`.
#[allow(dead_code)]
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

/// Pick the cards `ids` among the options of a card choice.
#[allow(dead_code)]
fn pick_cards(s: &mut Script, ids: &[ObjectId]) {
    let picks: Vec<usize> = s
        .options()
        .iter()
        .enumerate()
        .filter(|(_, o)| matches!(o, OptionDesc::Card { card } if ids.contains(card)))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(picks.len(), ids.len(), "not all offered: {:?}", s.options());
    s.respond(Response::Pick(picks));
}

// ---------------------------------------------------------------------------------------
// The life lost this way

fn leech_rite() -> String {
    card(
        "Test Leech Rite",
        "X B B",
        "Sorcery",
        "A:SP$ LoseLife | Defined$ Player.Opponent | LifeAmount$ X | SubAbility$ DBGainLife | SpellDescription$ Text.\n\
         SVar:DBGainLife:DB$ GainLife | Defined$ You | LifeAmount$ AFLifeLost\n\
         SVar:X:Count$xPaid\n\
         SVar:AFLifeLost:Number$0\n\
         # AFLifeLost will be set by LoseLife",
    )
}

// CR 119.3 (2026-11-06): "If an effect causes a player to gain life or lose life, that
// player's life total is adjusted accordingly." CR 608.2c: the instructions in the order
// written. "Each opponent loses X life. You gain life equal to the life lost this way":
// what both opponents lost, together. Forge: `LifeAmount$ AFLifeLost`, which its life loss
// sets as it resolves (`SVar:AFLifeLost:Number$0` is a placeholder).
#[test]
fn life_lost_this_way_is_what_the_opponents_lost() {
    let mut sc = scenario_n(3, &[&leech_rite()]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 5]);
    sc.card(P0, Zone::Hand, "Test Leech Rite");
    let mut s = Script::new(sc.build());
    s.cast("Test Leech Rite").x(3).pay().pass_all();
    assert_eq!((life(&s, P1), life(&s, P2)), (37, 37));
    assert_eq!(life(&s, P0), 46);
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Graveyard, "Test Leech Rite").unwrap()),
    );
    assert!(
        text.join(" ")
            .contains("you gain life equal to the life lost this way"),
        "{text:?}"
    );
}

// CR 119.3: an effect that makes players lose 0 life adjusts nothing, so 0 is gained.
#[test]
fn life_lost_this_way_is_zero_when_nothing_was_lost() {
    let mut sc = scenario(&[&leech_rite()]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 2]);
    sc.card(P0, Zone::Hand, "Test Leech Rite");
    let mut s = Script::new(sc.build());
    s.cast("Test Leech Rite").x(0).pay().pass_all();
    assert_eq!((life(&s, P0), life(&s, P1)), (40, 40));
}

// Invariant 15: a variable Forge's own code sets while resolving is never read by the
// value written in the script. Before D-075, Scholar of Athreos's "you gain life equal
// to the life lost this way" compiled as "you gain 0 life" (`Number$0`).
#[test]
fn a_value_forge_sets_while_resolving_is_never_read_as_written() {
    // Not after a life loss: no meaning to give it.
    let alone = card(
        "Test Odd Rite",
        "B",
        "Sorcery",
        "A:SP$ GainLife | Defined$ You | LifeAmount$ AFLifeLost | SpellDescription$ Text.\n\
         SVar:AFLifeLost:Number$0",
    );
    assert_eq!(
        forge_test_blockers(&alone),
        ["expr:AFLifeLost not after a life loss"]
    );
    // Another variable Forge fills in (the damage a prevention effect prevented).
    let prevented = card(
        "Test Mended Rite",
        "W",
        "Sorcery",
        "A:SP$ GainLife | Defined$ You | LifeAmount$ PreventedDamage | SpellDescription$ Text.\n\
         SVar:PreventedDamage:Number$0",
    );
    assert_eq!(
        forge_test_blockers(&prevented),
        ["expr:set while resolving PreventedDamage"]
    );
}

// Invariant 15: a comment line is ignored only once it has been read and found to say
// nothing about what the card does; any other comment blocks the card (it may say the
// script differs from the card).
#[test]
fn comments_that_were_read_are_ignored_others_block() {
    let tutor = |comment: &str| {
        card(
            "Test Tutor",
            "1 B",
            "Sorcery",
            &format!(
                "A:SP$ ChangeZone | Origin$ Library | Destination$ Hand | ChangeType$ Card | ChangeNum$ 1 | Mandatory$ True | SpellDescription$ Text.\n{comment}"
            ),
        )
    };
    assert!(
        forge_test_blockers(&tutor(
            "# TODO: Improve the tutoring logic for the AI. Currently will generally look for the most expensive castable thing in the library."
        ))
        .is_empty()
    );
    assert_eq!(
        forge_test_blockers(&tutor(
            "# Doesn't follow the strict wording of the Oracle text currently."
        )),
        ["line:# Doesn't follow the strict wording of the Oracle text currently."]
    );
    // The tutor itself: any card, into the hand, then shuffle.
    let mut sc = scenario(&[&tutor("# TODO: improve AI tutoring logic")]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 2]);
    sc.card(P0, Zone::Hand, "Test Tutor");
    let island = sc.card(P0, Zone::Library, "Island");
    let mut s = Script::new(sc.build());
    s.cast("Test Tutor").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::SearchLibrary);
    pick_cards(&mut s, &[island]);
    assert!(s.find(P0, Zone::Hand, "Island").is_some());
}

// ---------------------------------------------------------------------------------------
// "In any order"

// CR 401.4 (2026-11-06): "If an effect puts two or more cards in a specific position in a
// library at the same time, the owner of those cards may arrange them in any order." Forge:
// `Reorder$ True` on a `ChangeZone` from the hand to the library ("draw three cards, then
// put two cards from your hand on top of your library in any order").
#[test]
fn cards_put_on_top_from_the_hand_are_ordered_by_their_owner() {
    let storm = card(
        "Test Mindstorm",
        "U",
        "Instant",
        "A:SP$ Draw | NumCards$ 3 | SubAbility$ DBPut | SpellDescription$ Text.\n\
         SVar:DBPut:DB$ ChangeZone | Origin$ Hand | Destination$ Library | ChangeNum$ 2 | Mandatory$ True | Reorder$ True",
    );
    let mut sc = scenario(&[&storm]);
    sc.card(P0, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Hand, "Test Mindstorm");
    let swamp = sc.card(P0, Zone::Hand, "Swamp");
    let mountain = sc.card(P0, Zone::Hand, "Mountain");
    let mut s = Script::new(sc.build());
    s.cast("Test Mindstorm").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::ChooseCards);
    assert_eq!(s.st.zones.list(Zone::Hand, P0).len(), 5);
    pick_cards(&mut s, &[swamp, mountain]);
    // Now the order: the Mountain on top.
    assert_eq!(s.who(), P0);
    let order: Vec<usize> = [mountain, swamp]
        .iter()
        .map(|c| {
            s.options()
                .iter()
                .position(|o| matches!(o, OptionDesc::Card { card } if card == c))
                .unwrap()
        })
        .collect();
    s.respond(Response::Order(order));
    let library = s.st.zones.list(Zone::Library, P0);
    let top: Vec<String> = library.iter().rev().take(2).map(|&c| s.name(c)).collect();
    assert_eq!(top, ["Mountain", "Swamp"]);
    assert_eq!(s.st.zones.list(Zone::Hand, P0).len(), 3);
    // To anywhere but a library it means nothing the compiler knows.
    let odd = card(
        "Test Odd Storm",
        "U",
        "Instant",
        "A:SP$ ChangeZone | Origin$ Hand | Destination$ Exile | ChangeNum$ 2 | Mandatory$ True | Reorder$ True | SpellDescription$ Text.",
    );
    assert_eq!(
        forge_test_blockers(&odd),
        ["param:ChangeZone.Reorder not to a library"]
    );
}
