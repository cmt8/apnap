//! Replacement effects that change how much (D-073): Forge's `R:Event$ CreateToken` and
//! `R:Event$ AddCounter` ("twice that many", "that many plus one", "those tokens plus a
//! Food", "that many Angels instead"). Invented cards in Forge's format, compiled the way
//! real ones are, then played. Rules text:
//! docs/rules-notes/614-doublers-603.2d-additional-triggers.md and
//! docs/rules-notes/614-616-replacement-prevention.md.

use engine_core::layers::chars;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, ObjectId, OptionDesc, PlayerId};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

/// Subtypes are known to the compiler from the type lines of the cards it compiles.
const TYPES: &str = "Name:Test Typeholder\nManaCost:1\n\
    Types:Artifact Treasure Food Clue\nOracle:Text.";

const DRAGON_TYPE: &str =
    "Name:Test Dragonholder\nManaCost:1\nTypes:Creature Dragon\nPT:1/1\nOracle:Text.";

const TOKENS: [(&str, &str); 6] = [
    (
        "r_5_5_test_dragon",
        "Name:Test Dragon Token\nManaCost:no cost\nTypes:Creature Dragon\nPT:5/5\nOracle:Text.",
    ),
    (
        "w_1_1_test_soldier",
        "Name:Test Soldier Token\nManaCost:no cost\nTypes:Creature Soldier\nPT:1/1\nOracle:Text.",
    ),
    (
        "w_4_4_test_angel",
        "Name:Test Angel Token\nManaCost:no cost\nTypes:Creature Angel\nPT:4/4\nOracle:Text.",
    ),
    (
        "c_a_test_treasure",
        "Name:Test Treasure Token\nManaCost:no cost\nTypes:Artifact Treasure\nOracle:Text.",
    ),
    (
        "c_a_test_food",
        "Name:Test Food Token\nManaCost:no cost\nTypes:Artifact Food\nOracle:Text.",
    ),
    (
        "c_a_test_clue",
        "Name:Test Clue Token\nManaCost:no cost\nTypes:Artifact Clue\nOracle:Text.",
    ),
];

fn card(name: &str, types: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:2 G\nTypes:{types}\n{rest}\nOracle:Text.")
}

/// A token replacement effect of an enchantment: `R:` parameters, then the `ReplaceWith$`.
fn token_effect(name: &str, params: &str, with: &str) -> String {
    let params = if params.is_empty() {
        String::new()
    } else {
        format!(" | {params}")
    };
    card(
        name,
        "Enchantment",
        &format!(
            "R:Event$ CreateToken | ActiveZones$ Battlefield{params} | ReplaceWith$ Repl | Description$ Text.\n\
             SVar:Repl:{with}"
        ),
    )
}

fn season() -> String {
    token_effect(
        "Test Season",
        "ValidToken$ Card.YouCtrl | EffectOnly$ True",
        "DB$ ReplaceToken | Type$ Amount",
    )
}

fn vigor() -> String {
    token_effect("Test Vigor", "", "DB$ ReplaceToken | Type$ Amount")
}

fn took() -> String {
    token_effect(
        "Test Took",
        "ValidToken$ Card.YouCtrl",
        "DB$ ReplaceToken | Type$ AddToken | Amount$ 1 | TokenScript$ c_a_test_food",
    )
}

fn visitation() -> String {
    token_effect(
        "Test Visitation",
        "ValidToken$ Creature.YouCtrl",
        "DB$ ReplaceToken | Type$ ReplaceToken | TokenScript$ w_4_4_test_angel",
    )
}

fn xorn() -> String {
    token_effect(
        "Test Xorn",
        "ValidPlayer$ You | ValidToken$ Treasure",
        "DB$ ReplaceToken | Type$ AddToken | Amount$ 1 | TokenScript$ c_a_test_treasure",
    )
}

fn manufactor() -> String {
    token_effect(
        "Test Manufactor",
        "ValidPlayer$ You | ValidToken$ Clue,Food,Treasure",
        "DB$ ReplaceToken | Type$ ReplaceToken | TokenScript$ c_a_test_clue,c_a_test_food,c_a_test_treasure",
    )
}

fn chatter() -> String {
    token_effect(
        "Test Chatter",
        "ValidToken$ Card.YouCtrl",
        "DB$ ReplaceToken | Type$ AddToken | TokenScript$ c_a_test_food",
    )
}

fn token_spell(name: &str, token: &str, n: u32, owner: &str) -> String {
    format!(
        "Name:{name}\nManaCost:W\nTypes:Sorcery\n\
         A:SP$ Token | TokenAmount$ {n} | TokenScript$ {token}{owner} | SpellDescription$ Text.\nOracle:Text."
    )
}

fn token_spells() -> Vec<String> {
    vec![
        token_spell("Test Muster", "w_1_1_test_soldier", 1, ""),
        token_spell("Test Plunder", "c_a_test_treasure", 2, ""),
        token_spell(
            "Test Rally",
            "w_1_1_test_soldier",
            1,
            " | TokenOwner$ Player",
        ),
    ]
}

fn scenario_n(players: u8, cards: &[String]) -> Scenario {
    let mut all: Vec<&str> = cards.iter().map(String::as_str).collect();
    all.push(TYPES);
    all.push(DRAGON_TYPE);
    let mut sc = Scenario::new(
        GameConfig::commander(players),
        forge_test_catalog(&all, &TOKENS),
        31,
    );
    for p in 0..players {
        sc.library(PlayerId(p), "Forest", 20);
    }
    sc
}

fn with_spells(cards: &[String]) -> Vec<String> {
    let mut v = cards.to_vec();
    v.extend(token_spells());
    v
}

fn count(s: &Script, p: PlayerId, name: &str) -> usize {
    s.st.zones
        .battlefield
        .iter()
        .filter(|&&id| s.name(id) == name && chars(&s.st, id).controller == p)
        .count()
}

/// Choose, among the replacement effects offered, the one of `source`.
fn choose(s: &mut Script, source: &str) {
    assert_eq!(s.kind(), ChoiceKind::ChooseReplacement);
    let source = source.to_string();
    s.pick(
        &format!("the effect of {source}"),
        move |s, o| matches!(o, OptionDesc::Replacement { source: x, .. } if s.name(*x) == source),
    );
}

const SOLDIER: &str = "Test Soldier Token";
const ANGEL: &str = "Test Angel Token";
const TREASURE: &str = "Test Treasure Token";
const FOOD: &str = "Test Food Token";
const CLUE: &str = "Test Clue Token";

// ---------------------------------------------------------------------------------------
// Tokens

// CR 614.16, 614.1a (2026-11-06): "if an effect would create one or more tokens under your
// control, it creates twice that many of those tokens instead". CR 111.2: only tokens
// created under its controller's control.
#[test]
fn cr_614_16_twice_that_many_tokens_under_your_control() {
    let mut sc = scenario_n(2, &with_spells(&[season()]));
    sc.card(P0, Zone::Battlefield, "Test Season");
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Muster");
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    assert_eq!(count(&s, P0, SOLDIER), 2);
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Battlefield, "Test Season").unwrap()),
    )
    .join(" ");
    assert!(text.contains("twice that many"), "{text}");
}

// CR 614.5, 616.1 (2026-11-06): two such effects each apply once; their controller (the
// affected player) chooses the order — 1 becomes 2 becomes 4.
#[test]
fn cr_614_5_two_doublers_make_four() {
    let mut sc = scenario_n(2, &with_spells(&[season()]));
    sc.cards(P0, Zone::Battlefield, &["Test Season", "Test Season"]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    sc.card(P0, Zone::Hand, "Test Muster");
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::ChooseReplacement);
    assert_eq!(s.who(), P0);
    assert_eq!(s.options().len(), 2);
    s.respond(engine_core::Response::Pick(vec![0]));
    assert_eq!(count(&s, P0, SOLDIER), 4);
}

// CR 616.1, 616.1f (2026-11-06): the order matters — doubled then "plus a Food" is two
// Soldiers and a Food; "plus a Food" then doubled is two Soldiers and two Foods.
#[test]
fn cr_616_1_order_of_twice_and_plus_one() {
    for (first, soldiers, foods) in [("Test Season", 2, 1), ("Test Took", 2, 2)] {
        let mut sc = scenario_n(2, &with_spells(&[season(), took()]));
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Test Season", "Test Took", "Plains"],
        );
        sc.card(P0, Zone::Hand, "Test Muster");
        let mut s = Script::new(sc.build());
        s.cast("Test Muster").pay().pass_all();
        choose(&mut s, first);
        assert_eq!(count(&s, P0, SOLDIER), soldiers, "{first} first");
        assert_eq!(count(&s, P0, FOOD), foods, "{first} first");
        assert_eq!(s.kind(), ChoiceKind::Priority);
    }
}

// CR 614.1a (2026-11-06): "if one or more creature tokens would be created under your
// control, that many 4/4 Angels are created instead" — only creature tokens; doubled in
// either order (two Angels from one Soldier).
#[test]
fn creature_tokens_are_replaced_by_angels() {
    for first in ["Test Season", "Test Visitation"] {
        let mut sc = scenario_n(2, &with_spells(&[season(), visitation()]));
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Test Season", "Test Visitation", "Plains"],
        );
        sc.card(P0, Zone::Hand, "Test Muster");
        let mut s = Script::new(sc.build());
        s.cast("Test Muster").pay().pass_all();
        choose(&mut s, first);
        assert_eq!(count(&s, P0, ANGEL), 2, "{first} first");
        assert_eq!(count(&s, P0, SOLDIER), 0);
    }
    // A Treasure isn't a creature token.
    let mut sc = scenario_n(2, &with_spells(&[visitation()]));
    sc.cards(P0, Zone::Battlefield, &["Test Visitation", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Plunder");
    let mut s = Script::new(sc.build());
    s.cast("Test Plunder").pay().pass_all();
    assert_eq!(count(&s, P0, TREASURE), 2);
    assert_eq!(count(&s, P0, ANGEL), 0);
}

// "If you would create one or more Treasure tokens, instead create those tokens plus an
// additional Treasure token": once per event, whatever the number; not for Soldiers.
#[test]
fn an_additional_treasure_once_per_event() {
    let mut sc = scenario_n(2, &with_spells(&[xorn()]));
    sc.cards(P0, Zone::Battlefield, &["Test Xorn", "Plains", "Plains"]);
    sc.cards(P0, Zone::Hand, &["Test Plunder", "Test Muster"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Plunder").pay().pass_all();
    assert_eq!(count(&s, P0, TREASURE), 3);
    s.cast("Test Muster").pay().pass_all();
    assert_eq!(count(&s, P0, SOLDIER), 1);
    assert_eq!(count(&s, P0, TREASURE), 3);
}

// "Those tokens plus that many Food tokens" (no fixed number): as many as there were.
#[test]
fn plus_that_many_tokens() {
    let mut sc = scenario_n(2, &with_spells(&[chatter()]));
    sc.cards(P0, Zone::Battlefield, &["Test Chatter", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Plunder");
    let mut s = Script::new(sc.build());
    s.cast("Test Plunder").pay().pass_all();
    assert_eq!(count(&s, P0, TREASURE), 2);
    assert_eq!(count(&s, P0, FOOD), 2);
}

// "If you would create a Clue, Food, or Treasure token, instead create one of each" — two
// Treasures become two of each; what that makes is still one event (CR 616.1f), so "plus
// an additional Food" adds one Food, not one per kind.
#[test]
fn one_of_each_is_one_event() {
    let mut sc = scenario_n(2, &with_spells(&[manufactor()]));
    sc.cards(P0, Zone::Battlefield, &["Test Manufactor", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Plunder");
    let mut s = Script::new(sc.build());
    s.cast("Test Plunder").pay().pass_all();
    assert_eq!(
        (
            count(&s, P0, CLUE),
            count(&s, P0, FOOD),
            count(&s, P0, TREASURE)
        ),
        (2, 2, 2)
    );

    let mut sc = scenario_n(2, &with_spells(&[manufactor(), took()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Manufactor", "Test Took", "Plains"],
    );
    sc.card(P0, Zone::Hand, "Test Plunder");
    let mut s = Script::new(sc.build());
    s.cast("Test Plunder").pay().pass_all();
    choose(&mut s, "Test Manufactor");
    // Took's Food came after: Manufactor has already applied (CR 614.5).
    assert_eq!(
        (
            count(&s, P0, CLUE),
            count(&s, P0, FOOD),
            count(&s, P0, TREASURE)
        ),
        (2, 3, 2)
    );
    // The other order: Took adds a Food, then Manufactor makes one of each of all three.
    let mut sc = scenario_n(2, &with_spells(&[manufactor(), took()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Manufactor", "Test Took", "Plains"],
    );
    sc.card(P0, Zone::Hand, "Test Plunder");
    let mut s = Script::new(sc.build());
    s.cast("Test Plunder").pay().pass_all();
    choose(&mut s, "Test Took");
    assert_eq!(
        (
            count(&s, P0, CLUE),
            count(&s, P0, FOOD),
            count(&s, P0, TREASURE)
        ),
        (3, 3, 3)
    );
}

// CR 111.2, 616.1 (2026-11-06): in a three-player game, "if one or more tokens would be
// created" applies to every player's tokens; "under your control" only to its
// controller's. Each player's event is theirs (affected player).
#[test]
fn three_players_whose_tokens() {
    let mut sc = scenario_n(3, &with_spells(&[season(), vigor()]));
    sc.cards(P1, Zone::Battlefield, &["Test Season"]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    sc.card(P0, Zone::Hand, "Test Rally");
    let mut s = Script::new(sc.build());
    s.cast("Test Rally").pay().pass_all();
    assert_eq!(
        (
            count(&s, P0, SOLDIER),
            count(&s, P1, SOLDIER),
            count(&s, P2, SOLDIER)
        ),
        (1, 2, 1)
    );

    let mut sc = scenario_n(3, &with_spells(&[season(), vigor()]));
    sc.cards(P2, Zone::Battlefield, &["Test Vigor"]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    sc.card(P0, Zone::Hand, "Test Rally");
    let mut s = Script::new(sc.build());
    s.cast("Test Rally").pay().pass_all();
    assert_eq!(
        (
            count(&s, P0, SOLDIER),
            count(&s, P1, SOLDIER),
            count(&s, P2, SOLDIER)
        ),
        (2, 2, 2)
    );
}

// CLAUDE.md invariant 4: the effect exists only while its source is on the battlefield
// with the ability — not once it has left, nor while it has lost its abilities.
#[test]
fn the_effect_ends_with_its_source() {
    let smite = "Name:Test Unmaking\nManaCost:W\nTypes:Instant\n\
        A:SP$ Destroy | ValidTgts$ Enchantment | SpellDescription$ Text.\nOracle:Text.";
    let mut cards = with_spells(&[season()]);
    cards.push(smite.to_string());
    let mut sc = scenario_n(2, &cards);
    sc.cards(P0, Zone::Battlefield, &["Test Season", "Plains", "Plains"]);
    sc.cards(P0, Zone::Hand, &["Test Unmaking", "Test Muster"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Unmaking")
        .target("Test Season")
        .pay()
        .pass_all();
    s.cast("Test Muster").pay().pass_all();
    assert_eq!(count(&s, P0, SOLDIER), 1);

    // A creature with the effect that loses all its abilities until end of turn.
    let doubler = card(
        "Test Doubling Beast",
        "Creature Beast",
        "PT:2/2\n\
         R:Event$ CreateToken | ActiveZones$ Battlefield | ValidToken$ Card.YouCtrl | ReplaceWith$ Repl | Description$ Text.\n\
         SVar:Repl:DB$ ReplaceToken | Type$ Amount",
    );
    let blank = "Name:Test Blanking\nManaCost:W\nTypes:Instant\n\
        A:SP$ Animate | ValidTgts$ Creature | Power$ 0 | Toughness$ 1 | RemoveAllAbilities$ True | SpellDescription$ Text.\nOracle:Text.";
    let mut cards = with_spells(&[doubler]);
    cards.push(blank.to_string());
    let mut sc = scenario_n(2, &cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Doubling Beast", "Plains", "Plains", "Plains"],
    );
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Muster", "Test Muster", "Test Blanking"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    assert_eq!(count(&s, P0, SOLDIER), 2);
    s.cast("Test Blanking")
        .target("Test Doubling Beast")
        .pay()
        .pass_all();
    s.cast("Test Muster").pay().pass_all();
    assert_eq!(count(&s, P0, SOLDIER), 3);
}

// ---------------------------------------------------------------------------------------
// Counters

fn counter_effect(name: &str, params: &str, math: &str) -> String {
    card(
        name,
        "Enchantment",
        &format!(
            "R:Event$ AddCounter | ActiveZones$ Battlefield | {params} | ReplaceWith$ Repl | Description$ Text.\n\
             SVar:Repl:DB$ ReplaceCounter | ChooseCounter$ True | Amount$ X\n\
             SVar:X:ReplaceCount$CounterNum/{math}"
        ),
    )
}

fn scales() -> String {
    counter_effect(
        "Test Scales",
        "ValidCard$ Creature.YouCtrl+inZoneBattlefield | ValidCounterType$ P1P1",
        "Plus.1",
    )
}

fn season_counters() -> String {
    counter_effect(
        "Test Counter Season",
        "ValidCard$ Permanent.YouCtrl+inZoneBattlefield | EffectOnly$ True",
        "Twice",
    )
}

fn constrictor() -> String {
    counter_effect(
        "Test Constrictor",
        "ValidCard$ Artifact.YouCtrl+inZoneBattlefield,Creature.YouCtrl+inZoneBattlefield",
        "Plus.1",
    )
}

fn counter_cards(extra: &[String]) -> Vec<String> {
    let mut v = extra.to_vec();
    v.push(
        "Name:Test Swelling\nManaCost:G\nTypes:Instant\n\
         A:SP$ PutCounter | ValidTgts$ Creature | CounterType$ P1P1 | CounterNum$ 1 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    );
    v.push(
        "Name:Test Blight\nManaCost:G\nTypes:Instant\n\
         A:SP$ PutCounter | ValidTgts$ Creature | CounterType$ M1M1 | CounterNum$ 1 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    );
    v.push(
        "Name:Test Hatchling\nManaCost:G\nTypes:Creature Lizard\nPT:0/0\nK:etbCounter:P1P1:2\nOracle:Text."
            .to_string(),
    );
    v
}

fn counters(s: &Script, id: ObjectId, kind: card_model::CounterKind) -> u32 {
    s.st.object(id).counters_of(kind)
}

use card_model::CounterKind::{MinusOne, PlusOne};

// CR 614.1a, 122.1 (2026-11-06): "that many plus one +1/+1 counters are put on it instead";
// only on its controller's creatures, only +1/+1 counters.
#[test]
fn that_many_plus_one_counters() {
    let mut sc = scenario_n(2, &counter_cards(&[scales()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Scales", "Forest", "Forest", "Forest"],
    );
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Swelling", "Test Swelling", "Test Blight"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Swelling")
        .target_object(golem)
        .pay()
        .pass_all();
    assert_eq!(counters(&s, golem, PlusOne), 2);
    s.cast("Test Swelling")
        .target_object(theirs)
        .pay()
        .pass_all();
    assert_eq!(counters(&s, theirs, PlusOne), 1);
    s.cast("Test Blight").target_object(golem).pay().pass_all();
    // (CR 704.5q then removes one of each.)
    assert_eq!(counters(&s, golem, MinusOne), 0);
    assert_eq!(counters(&s, golem, PlusOne), 1);
}

// CR 616.1, 616.1f (2026-11-06): "plus one" then "twice" makes 4 from 1; "twice" then
// "plus one" makes 3 — the affected creature's controller chooses.
#[test]
fn cr_616_1_order_of_plus_one_and_twice_counters() {
    for (first, n) in [("Test Scales", 4), ("Test Counter Season", 3)] {
        let mut sc = scenario_n(2, &counter_cards(&[scales(), season_counters()]));
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Test Scales", "Test Counter Season", "Forest"],
        );
        let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
        sc.card(P0, Zone::Hand, "Test Swelling");
        let mut s = Script::new(sc.build());
        s.cast("Test Swelling")
            .target_object(golem)
            .pay()
            .pass_all();
        assert_eq!(s.who(), P0);
        choose(&mut s, first);
        assert_eq!(counters(&s, golem, PlusOne), n, "{first} first");
    }
}

// CR 122.6, 614.16 (2026-11-06): counters a permanent enters with are counters put on it.
#[test]
fn cr_122_6_entering_with_counters() {
    let mut sc = scenario_n(2, &counter_cards(&[scales(), season_counters()]));
    sc.cards(P0, Zone::Battlefield, &["Test Scales", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Hatchling");
    let mut s = Script::new(sc.build());
    s.cast("Test Hatchling").pay().pass_all();
    let h = s
        .find(P0, Zone::Battlefield, "Test Hatchling")
        .expect("entered");
    assert_eq!(counters(&s, h, PlusOne), 3);
}

// "One or more counters [of any kind]": -1/-1 counters too, each kind plus one.
#[test]
fn any_kind_of_counter() {
    let mut sc = scenario_n(2, &counter_cards(&[constrictor()]));
    sc.cards(P0, Zone::Battlefield, &["Test Constrictor", "Forest"]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem"); // 2/3
    sc.card(P0, Zone::Hand, "Test Blight");
    let mut s = Script::new(sc.build());
    s.cast("Test Blight").target_object(golem).pay().pass_all();
    assert_eq!(counters(&s, golem, MinusOne), 2);
}

// CR 616.1 (2026-11-06): the affected player picks the order; the heuristic AI picks the
// one that gives it more +1/+1 counters ("plus one" before "twice": 4, not 3).
#[test]
fn the_ai_orders_doublers_to_get_more() {
    use engine_core::Agent;
    let cards = counter_cards(&[scales(), season_counters()]);
    let mut sc = scenario_n(2, &cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Counter Season", "Test Scales", "Forest"],
    );
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Swelling");
    let mut s = Script::new(sc.build());
    s.cast("Test Swelling")
        .target_object(golem)
        .pay()
        .pass_all();
    assert_eq!(s.kind(), ChoiceKind::ChooseReplacement);
    let mut ai = ai::HeuristicAgent::new(1, s.st.catalog.clone());
    let view = engine_core::player_view(&s.st, P0);
    let r = ai.decide(&view, s.decision());
    s.respond(r);
    assert_eq!(counters(&s, golem, PlusOne), 4);
}

// ---------------------------------------------------------------------------------------
// Tokens entering the battlefield (CR 111.2, 111.6, 614.12, 616.1g)

/// "Each [filter] enters with an additional +1/+1 counter on it" / "… enters tapped", an
/// effect of another permanent (`DB$ PutCounter | ETB$ True | Defined$ ReplacedCard`).
fn enters_effect(name: &str, valid: &str, with: &str) -> String {
    card(
        name,
        "Artifact",
        &format!(
            "R:Event$ Moved | ActiveZones$ Battlefield | ValidCard$ {valid} | Destination$ Battlefield | ReplacementResult$ Updated | ReplaceWith$ E | Description$ Text.\n\
             SVar:E:{with}"
        ),
    )
}

fn globe() -> String {
    enters_effect(
        "Test Globe",
        "Creature.Dragon+YouCtrl",
        "DB$ PutCounter | ETB$ True | Defined$ ReplacedCard | CounterType$ P1P1 | CounterNum$ 1",
    )
}

fn tapper() -> String {
    enters_effect(
        "Test Gatekeeper",
        "Creature.OppCtrl",
        "DB$ Tap | ETB$ True | Defined$ ReplacedCard",
    )
}

fn token_entry_cards(extra: &[String]) -> Vec<String> {
    let mut v = with_spells(extra);
    v.push(token_spell("Test Hatch", "r_5_5_test_dragon", 1, ""));
    v
}

fn dragons(s: &Script, p: PlayerId) -> Vec<ObjectId> {
    s.st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| s.name(id) == "Test Dragon Token" && chars(&s.st, id).controller == p)
        .collect()
}

// CR 111.2, 111.6, 614.1c, 614.12 (2026-11-06): a created token enters the battlefield, so
// another permanent's "each Dragon you control enters with an additional +1/+1 counter"
// applies to it, judged as the token would exist there. The counter is on it as it
// enters: a 6/6.
#[test]
fn cr_111_6_a_created_token_enters_with_a_counter() {
    let mut sc = scenario_n(2, &token_entry_cards(&[globe()]));
    sc.cards(P0, Zone::Battlefield, &["Test Globe", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Hatch");
    let mut s = Script::new(sc.build());
    s.cast("Test Hatch").pay().pass_all();
    let d = dragons(&s, P0);
    assert_eq!(d.len(), 1);
    assert_eq!(counters(&s, d[0], PlusOne), 1);
    let c = chars(&s.st, d[0]);
    assert_eq!((c.power, c.toughness), (Some(6), Some(6)));
    // A Soldier isn't a Dragon.
    let mut sc = scenario_n(2, &token_entry_cards(&[globe()]));
    sc.cards(P0, Zone::Battlefield, &["Test Globe", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Muster");
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    let soldier = s.find(P0, Zone::Battlefield, SOLDIER).unwrap();
    assert_eq!(counters(&s, soldier, PlusOne), 0);
}

// CR 111.6, 614.1d (2026-11-06): "creatures your opponents control enter tapped" — an
// opponent's token enters tapped (it doesn't become tapped, CR 603.2e); the effect's
// controller's own tokens don't.
#[test]
fn cr_111_6_an_opponents_token_enters_tapped() {
    let mut sc = scenario_n(2, &token_entry_cards(&[tapper()]));
    sc.cards(P1, Zone::Battlefield, &["Test Gatekeeper"]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    sc.cards(P0, Zone::Hand, &["Test Muster"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    let soldier = s.find(P0, Zone::Battlefield, SOLDIER).unwrap();
    assert!(s.st.object(soldier).tapped);
    assert_eq!(
        s.count(|e| matches!(e, engine_core::Event::Tapped { object } if *object == soldier)),
        0
    );

    let mut sc = scenario_n(2, &token_entry_cards(&[tapper()]));
    sc.cards(P0, Zone::Battlefield, &["Test Gatekeeper", "Plains"]);
    sc.cards(P0, Zone::Hand, &["Test Muster"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    let soldier = s.find(P0, Zone::Battlefield, SOLDIER).unwrap();
    assert!(!s.st.object(soldier).tapped);
}

// CR 616.1g (2026-11-06): creating the tokens is modified first (twice that many), then
// each token's entering (an additional counter on each). CR 616.2, 122.6: the counter a
// token would enter with is then doubled too. Each token is its own entering event.
#[test]
fn cr_616_1g_doubled_tokens_each_enter_with_a_counter() {
    let mut sc = scenario_n(2, &token_entry_cards(&[globe(), season()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Globe", "Test Season", "Plains"],
    );
    sc.card(P0, Zone::Hand, "Test Hatch");
    let mut s = Script::new(sc.build());
    s.cast("Test Hatch").pay().pass_all();
    let d = dragons(&s, P0);
    assert_eq!(d.len(), 2);
    for &x in &d {
        assert_eq!(counters(&s, x, PlusOne), 1);
    }

    let mut sc = scenario_n(
        2,
        &token_entry_cards(&[globe(), season(), season_counters()]),
    );
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Globe", "Test Season", "Test Counter Season", "Plains"],
    );
    sc.card(P0, Zone::Hand, "Test Hatch");
    let mut s = Script::new(sc.build());
    s.cast("Test Hatch").pay().pass_all();
    let d = dragons(&s, P0);
    assert_eq!(d.len(), 2);
    for &x in &d {
        assert_eq!(counters(&s, x, PlusOne), 2);
    }
    assert_eq!(s.kind(), ChoiceKind::Priority);
}

// CR 614.12 (2026-11-06): which effects apply is judged on the permanent as it would exist
// on the battlefield, "taking into account … continuous effects that already exist and
// would apply to the permanent": a Soldier token under "creatures you control are Dragons"
// gets the Dragon counter — and so does a creature card cast from the hand.
#[test]
fn cr_614_12_continuous_effects_that_would_apply_count() {
    let lord = card(
        "Test Dragonlord",
        "Enchantment",
        "S:Mode$ Continuous | Affected$ Creature.YouCtrl | AddType$ Dragon | Description$ Text.",
    );
    let mut cards = token_entry_cards(&[globe(), lord]);
    cards.push(
        "Name:Test Recruit\nManaCost:W\nTypes:Creature Soldier\nPT:1/1\nOracle:Text.".to_string(),
    );
    let mut sc = scenario_n(2, &cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Globe", "Test Dragonlord", "Plains", "Plains"],
    );
    sc.cards(P0, Zone::Hand, &["Test Muster", "Test Recruit"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    let soldier = s.find(P0, Zone::Battlefield, SOLDIER).unwrap();
    assert_eq!(counters(&s, soldier, PlusOne), 1);
    s.cast("Test Recruit").pay().pass_all();
    let recruit = s.find(P0, Zone::Battlefield, "Test Recruit").unwrap();
    assert_eq!(counters(&s, recruit, PlusOne), 1);
}

// CR 800.4b (2026-11-06): no tokens are created for a player who has left the game.
#[test]
fn cr_800_4b_no_tokens_for_a_player_who_left() {
    let mut sc = scenario_n(3, &token_entry_cards(&[vigor()]));
    sc.cards(P0, Zone::Battlefield, &["Test Vigor", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Rally");
    let mut st = sc.build();
    st.players[2].has_lost = true;
    let mut s = Script::new(st);
    s.cast("Test Rally").pay().pass_all();
    assert_eq!(
        (
            count(&s, P0, SOLDIER),
            count(&s, P1, SOLDIER),
            count(&s, P2, SOLDIER)
        ),
        (2, 2, 0)
    );
}

// ---------------------------------------------------------------------------------------
// "Can't be countered" (CR 101.2, 701.6a, 614.17)

fn counter_war_cards() -> Vec<String> {
    vec![
        "Name:Test Unstoppable Word\nManaCost:W\nTypes:Instant\n\
         R:Event$ Counter | ValidCard$ Card.Self | ValidSA$ Spell | Layer$ CantHappen | Description$ Text.\n\
         A:SP$ GainLife | LifeAmount$ 3 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
        "Name:Test Denial\nManaCost:U\nTypes:Instant\n\
         A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | SubAbility$ DBDraw | SpellDescription$ Text.\n\
         SVar:DBDraw:DB$ Draw | NumCards$ 1\nOracle:Text."
            .to_string(),
        "Name:Test Wild Shelter\nManaCost:G\nTypes:Enchantment\n\
         R:Event$ Counter | ValidSA$ Spell.Creature+YouCtrl | Layer$ CantHappen | ActiveZones$ Battlefield | Description$ Text.\nOracle:Text."
            .to_string(),
        "Name:Test Recruit\nManaCost:W\nTypes:Creature Soldier\nPT:1/1\nOracle:Text."
            .to_string(),
    ]
}

fn hand_size(s: &Script, p: PlayerId) -> usize {
    s.st.zones.list(Zone::Hand, p).len()
}

// CR 101.2, 701.6a (2026-11-06): "this spell can't be countered" — a spell that would
// counter it still resolves (it is a legal target), does nothing to it, and does the rest
// of what it does; the spell then resolves.
#[test]
fn cr_101_2_this_spell_cant_be_countered() {
    let mut sc = scenario_n(2, &counter_war_cards());
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    sc.cards(P1, Zone::Battlefield, &["Island"]);
    sc.card(P0, Zone::Hand, "Test Unstoppable Word");
    sc.card(P1, Zone::Hand, "Test Denial");
    let mut s = Script::new(sc.build());
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Hand, "Test Unstoppable Word").unwrap()),
    )
    .join(" ");
    assert!(text.contains("can't be countered"), "{text}");
    s.cast("Test Unstoppable Word").pay().pass();
    assert_eq!(s.who(), P1);
    let cards = hand_size(&s, P1);
    s.cast("Test Denial").target("Test Unstoppable Word").pay();
    s.pass_all();
    // The Denial resolved: its controller drew; the Word is still on the stack.
    assert_eq!(hand_size(&s, P1), cards);
    assert!(s.find(P0, Zone::Stack, "Test Unstoppable Word").is_some());
    assert_eq!(
        s.count(|e| matches!(e, engine_core::Event::Countered { .. })),
        0
    );
    s.pass_all();
    assert_eq!(s.st.player(P0).life, 43);
    assert!(
        s.find(P0, Zone::Graveyard, "Test Unstoppable Word")
            .is_some()
    );
}

// "Creature spells you control can't be countered": its controller's creature spells only.
#[test]
fn creature_spells_you_control_cant_be_countered() {
    for (shelter_owner, countered) in [(P0, false), (P1, true)] {
        let mut sc = scenario_n(2, &counter_war_cards());
        sc.cards(P0, Zone::Battlefield, &["Plains"]);
        sc.cards(P1, Zone::Battlefield, &["Island"]);
        sc.card(shelter_owner, Zone::Battlefield, "Test Wild Shelter");
        sc.card(P0, Zone::Hand, "Test Recruit");
        sc.card(P1, Zone::Hand, "Test Denial");
        let mut s = Script::new(sc.build());
        s.cast("Test Recruit").pay().pass();
        s.cast("Test Denial").target("Test Recruit").pay();
        s.pass_all();
        s.pass_all();
        assert_eq!(
            s.find(P0, Zone::Graveyard, "Test Recruit").is_some(),
            countered,
            "shelter of {shelter_owner:?}"
        );
        assert_eq!(
            s.find(P0, Zone::Battlefield, "Test Recruit").is_some(),
            !countered
        );
    }
}

#[test]
fn cant_be_countered_forms_left_blocked() {
    let blocked = |card: &str, what: &str| {
        let b = forge_test_blockers(card);
        assert!(b.iter().any(|x| x.contains(what)), "{what}: {b:?}");
    };
    // Only by some spells.
    blocked(
        "Name:Test Choosy\nManaCost:G\nTypes:Enchantment\n\
         R:Event$ Counter | ValidSA$ Spell.YouCtrl | ValidCause$ Spell.Blue | Layer$ CantHappen | ActiveZones$ Battlefield | Description$ Text.\nOracle:Text.",
        "param:repl Counter.ValidCause",
    );
    // Abilities too.
    blocked(
        "Name:Test Silence\nManaCost:G\nTypes:Enchantment\n\
         R:Event$ Counter | ValidSA$ Spell,Activated,Triggered | Layer$ CantHappen | ActiveZones$ Battlefield | Description$ Text.\nOracle:Text.",
        "repl:Counter:ValidSA",
    );
    // Under a condition.
    blocked(
        "Name:Test Sometimes\nManaCost:G\nTypes:Instant\n\
         R:Event$ Counter | ValidCard$ Card.Self | ValidSA$ Spell | Layer$ CantHappen | CheckSVar$ X | SVarCompare$ GE5 | Description$ Text.\n\
         A:SP$ GainLife | LifeAmount$ 3 | SpellDescription$ Text.\nSVar:X:Count$CardsInYourHand\nOracle:Text.",
        "param:repl Counter.CheckSVar",
    );
}

// ---------------------------------------------------------------------------------------
// "That ability triggers an additional time" (CR 603.2d)

fn harmonicon() -> String {
    card(
        "Test Harmonicon",
        "Artifact",
        "S:Mode$ Panharmonicon | ValidMode$ ChangesZone,ChangesZoneAll | ValidCard$ Permanent.YouCtrl | ValidCause$ Artifact,Creature | Destination$ Battlefield | Description$ Text.",
    )
}

fn mourner() -> String {
    card(
        "Test Mourner",
        "Creature Human",
        "PT:2/2\n\
         S:Mode$ Panharmonicon | ValidMode$ ChangesZone,ChangesZoneAll | ValidCard$ Permanent.YouCtrl | ValidCause$ Creature | Origin$ Battlefield | Destination$ Graveyard | Description$ Text.",
    )
}

fn warcaller() -> String {
    card(
        "Test Warcaller",
        "Enchantment",
        "S:Mode$ Panharmonicon | ValidMode$ Attacks,AttackersDeclared,AttackersDeclaredOneTarget | ValidCard$ Permanent.YouCtrl | ValidCause$ Creature | Description$ Text.",
    )
}

fn legend_echo() -> String {
    card(
        "Test Legend Echo",
        "Enchantment",
        "S:Mode$ Panharmonicon | ValidCard$ Creature.YouCtrl+Legendary | Description$ Text.",
    )
}

/// An enchantment that gains its controller 1 life on a trigger `when`.
fn watcher(name: &str, when: &str) -> String {
    card(
        name,
        "Enchantment",
        &format!(
            "T:{when} | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:DB$ GainLife | LifeAmount$ 1"
        ),
    )
}

fn trigger_cards(extra: &[String]) -> Vec<String> {
    let mut v = extra.to_vec();
    v.push(watcher(
        "Test Arrival Watch",
        "Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Permanent",
    ));
    v.push(watcher(
        "Test Death Watch",
        "Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Creature",
    ));
    v.push(watcher(
        "Test Battle Watch",
        "Mode$ Attacks | ValidCard$ Creature.YouCtrl",
    ));
    v.push(
        "Name:Test Recruit\nManaCost:W\nTypes:Creature Soldier\nPT:1/1\nOracle:Text.".to_string(),
    );
    v.push(
        "Name:Test Wrath\nManaCost:W\nTypes:Sorcery\n\
         A:SP$ DestroyAll | ValidCards$ Creature | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    );
    v.push(
        "Name:Test Banishing\nManaCost:W\nTypes:Instant\n\
         A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Exile | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    );
    v.push(
        "Name:Test Old King\nManaCost:W\nTypes:Legendary Creature Human\nPT:2/2\n\
         T:Mode$ Phase | Phase$ Upkeep | ValidPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
         SVar:TrigGain:DB$ GainLife | LifeAmount$ 1\nOracle:Text."
            .to_string(),
    );
    v
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

/// Resolve everything on the stack (each trigger instance is its own ability).
fn resolve_all(s: &mut Script) {
    for _ in 0..50 {
        order_triggers(s);
        if s.st.stack.is_empty() || s.kind() != ChoiceKind::Priority {
            return;
        }
        s.pass_all();
    }
}

/// Put triggered abilities on the stack in the order offered (CR 603.3b).
fn order_triggers(s: &mut Script) {
    while s.next.is_some() && s.kind() == ChoiceKind::OrderTriggers {
        let n = s.options().len();
        s.respond(engine_core::Response::Order((0..n).collect()));
    }
}

// CR 603.2d (2026-11-06): "if an artifact or creature entering causes a triggered ability
// of a permanent you control to trigger, that ability triggers an additional time" — two
// instances, each put on the stack on its own; two such effects add (three), they don't
// multiply.
#[test]
fn cr_603_2d_an_additional_time_and_two_effects_add() {
    for (harmonicons, gained) in [(0, 1), (1, 2), (2, 3)] {
        let mut sc = scenario_n(2, &trigger_cards(&[harmonicon()]));
        sc.cards(P0, Zone::Battlefield, &["Test Arrival Watch", "Plains"]);
        for _ in 0..harmonicons {
            sc.card(P0, Zone::Battlefield, "Test Harmonicon");
        }
        sc.card(P0, Zone::Hand, "Test Recruit");
        let mut s = Script::new(sc.build());
        s.cast("Test Recruit").pay().pass_all();
        order_triggers(&mut s);
        let triggered = s.count(|e| matches!(e, engine_core::Event::Triggered { .. }));
        assert_eq!(triggered, gained as usize, "{harmonicons} Harmonicons");
        assert_eq!(s.st.stack.len(), gained as usize);
        resolve_all(&mut s);
        assert_eq!(life(&s, P0), 40 + gained, "{harmonicons} Harmonicons");
    }
}

// CR 603.2d: only what it says causes it — a land entering isn't an artifact or creature;
// and only abilities of permanents its controller controls.
#[test]
fn cr_603_2d_only_the_cause_and_sources_it_names() {
    let mut sc = scenario_n(2, &trigger_cards(&[harmonicon()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Arrival Watch", "Test Harmonicon"],
    );
    sc.card(P0, Zone::Hand, "Forest");
    let mut s = Script::new(sc.build());
    s.pick(
        "play Forest",
        |s, o| matches!(o, OptionDesc::PlayLand { card } if s.name(*card) == "Forest"),
    );
    resolve_all(&mut s);
    assert_eq!(life(&s, P0), 41);

    // The opponent's Harmonicon doesn't make P0's ability trigger again.
    let mut sc = scenario_n(2, &trigger_cards(&[harmonicon()]));
    sc.cards(P0, Zone::Battlefield, &["Test Arrival Watch", "Plains"]);
    sc.card(P1, Zone::Battlefield, "Test Harmonicon");
    sc.card(P0, Zone::Hand, "Test Recruit");
    let mut s = Script::new(sc.build());
    s.cast("Test Recruit").pay().pass_all();
    resolve_all(&mut s);
    assert_eq!(life(&s, P0), 41);
}

// CR 603.2d, 603.10a, 700.4 (2026-11-06): "if a creature dying causes …" — a creature
// exiled doesn't die; abilities that look back in time see the effect of a creature that
// dies at the same time (three creatures die: three triggers, each twice).
#[test]
fn cr_603_2d_dying_and_looking_back() {
    let mut sc = scenario_n(2, &trigger_cards(&[mourner()]));
    sc.cards(P0, Zone::Battlefield, &["Test Death Watch", "Plains"]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Mourner", "Test Golem", "Test Golem"],
    );
    sc.card(P0, Zone::Hand, "Test Wrath");
    let mut s = Script::new(sc.build());
    s.cast("Test Wrath").pay().pass_all();
    resolve_all(&mut s);
    assert_eq!(life(&s, P0), 46);

    let mut sc = scenario_n(2, &trigger_cards(&[mourner()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Death Watch", "Test Mourner", "Plains"],
    );
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Banishing");
    let mut s = Script::new(sc.build());
    s.cast("Test Banishing")
        .target_object(golem)
        .pay()
        .pass_all();
    resolve_all(&mut s);
    assert_eq!(life(&s, P0), 40);
}

// CR 603.2d, 508.1m: "if a creature attacking causes …".
#[test]
fn cr_603_2d_attacking() {
    let mut sc = scenario_n(2, &trigger_cards(&[warcaller()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Battle Watch", "Test Warcaller"],
    );
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let mut s = Script::new(sc.build());
    let mut asked = 0;
    for _ in 0..200 {
        if s.next.is_none() {
            break;
        }
        match s.kind() {
            ChoiceKind::DeclareAttackers => {
                let pick = match s.options().first() {
                    Some(OptionDesc::Attack { attacker, .. }) if *attacker == golem => vec![0],
                    _ => vec![],
                };
                asked += 1;
                s.respond(engine_core::Response::Pick(pick));
            }
            ChoiceKind::OrderTriggers => order_triggers(&mut s),
            ChoiceKind::Priority if asked > 0 && s.st.stack.is_empty() => break,
            ChoiceKind::Priority => {
                s.pass();
            }
            _ => {
                s.respond(engine_core::Response::Pick(vec![]));
            }
        }
    }
    assert!(asked > 0, "attackers were declared");
    assert_eq!(life(&s, P0), 42);
}

// CR 603.2d: "if a triggered ability of a legendary creature you control triggers" — any
// trigger event ("at the beginning of your upkeep").
#[test]
fn cr_603_2d_any_trigger_of_a_kind_of_source() {
    let mut sc = scenario_n(2, &trigger_cards(&[legend_echo()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Legend Echo", "Test Old King"],
    );
    sc.turn(P1, engine_core::turn::Step::End);
    let mut s = Script::new(sc.build());
    for _ in 0..50 {
        order_triggers(&mut s);
        if s.st.turn.active == P0 && s.st.turn.step == engine_core::turn::Step::Upkeep {
            break;
        }
        if s.kind() == ChoiceKind::Priority {
            s.pass();
        } else {
            s.respond(engine_core::Response::Pick(vec![]));
        }
    }
    assert_eq!(s.st.stack.len(), 2);
    resolve_all(&mut s);
    assert_eq!(life(&s, P0), 42);
}

#[test]
fn additional_trigger_forms_left_blocked() {
    let blocked = |card: &str, what: &str| {
        let b = forge_test_blockers(card);
        assert!(b.iter().any(|x| x.contains(what)), "{what}: {b:?}");
    };
    blocked(
        "Name:Test Lifesong\nManaCost:G\nTypes:Enchantment\n\
         S:Mode$ Panharmonicon | ValidMode$ LifeGained | ValidPlayer$ You | ValidCard$ Permanent.YouCtrl | Description$ Text.\nOracle:Text.",
        "static:Panharmonicon mode LifeGained",
    );
    blocked(
        "Name:Test Room Echo\nManaCost:G\nTypes:Enchantment\n\
         S:Mode$ Panharmonicon | ValidMode$ RoomEntered | ValidCard$ Card.YouOwn | ValidZone$ Command | Description$ Text.\nOracle:Text.",
        "param:static Panharmonicon.ValidZone",
    );
}

// CR 616.1g, 614.12a (2026-11-06): one batch in which a land asks its "as this enters"
// choice (D-067) and a token is created: the choice is asked while the token's entering
// waits in the same batch; then the token enters with the Dragon counter, and both are put
// onto the battlefield together.
#[test]
fn a_token_enters_in_a_batch_with_an_as_enters_choice() {
    use engine_core::frames::{ApplyFrame, Frame};
    use engine_core::replacement::{MoveCause, Proposed};
    let mut cards = token_entry_cards(&[globe()]);
    cards.push(
        "Name:Test Shockland\nManaCost:no cost\nTypes:Land Island Swamp\n\
         R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ DBTap | ReplacementResult$ Updated | Description$ Text.\n\
         SVar:DBTap:DB$ Tap | ETB$ True | Defined$ Self | UnlessCost$ PayLife<2> | UnlessPayer$ You\nOracle:Text."
            .to_string(),
    );
    let mut sc = scenario_n(2, &cards);
    sc.cards(P0, Zone::Battlefield, &["Test Globe"]);
    let land = sc.card(P0, Zone::Hand, "Test Shockland");
    let mut st = sc.build();
    let dragon = st.catalog.find("r_5_5_test_dragon").expect("token");
    let batch = ApplyFrame::new(
        &st,
        vec![
            Proposed::moving(land, Zone::Battlefield, MoveCause::Rule),
            Proposed::CreateTokens {
                token: dragon,
                count: 1,
                controller: P0,
                more: Vec::new(),
            },
        ],
    );
    st.frames.push(Frame::Apply(batch));
    let mut s = Script::new(st);
    assert!(
        matches!(s.kind(), ChoiceKind::AsEnters { .. }),
        "{:?}",
        s.kind()
    );
    assert!(dragons(&s, P0).is_empty(), "nothing has entered yet");
    s.pick("pay", |_, o| *o == OptionDesc::Accept);
    let d = dragons(&s, P0);
    assert_eq!(d.len(), 1);
    assert_eq!(counters(&s, d[0], PlusOne), 1);
    let shock = s
        .find(P0, Zone::Battlefield, "Test Shockland")
        .expect("entered");
    assert!(!s.st.object(shock).tapped);
    assert_eq!(s.st.player(P0).life, 38);
}

// CR 603.6a, 616.1g (2026-11-06): doubled tokens each enter — "whenever a permanent
// enters" triggers once for each of them.
#[test]
fn doubled_tokens_each_trigger_on_entering() {
    let mut cards = token_entry_cards(&[season()]);
    cards.push(watcher(
        "Test Arrival Watch",
        "Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Permanent",
    ));
    let mut sc = scenario_n(2, &cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Season", "Test Arrival Watch", "Plains"],
    );
    sc.card(P0, Zone::Hand, "Test Hatch");
    let mut s = Script::new(sc.build());
    s.cast("Test Hatch").pay().pass_all();
    order_triggers(&mut s);
    assert_eq!(dragons(&s, P0).len(), 2);
    assert_eq!(
        s.count(|e| matches!(e, engine_core::Event::Triggered { .. })),
        2
    );
    resolve_all(&mut s);
    assert_eq!(s.st.player(P0).life, 42);
}

// ---------------------------------------------------------------------------------------
// Damage that is doubled or increased (CR 614.1a, 120.4b, 616.1)

fn damage_effect(name: &str, params: &str, math: &str) -> String {
    card(
        name,
        "Enchantment",
        &format!(
            "R:Event$ DamageDone | ActiveZones$ Battlefield | {params} | ReplaceWith$ Dmg | Description$ Text.\n\
             SVar:Dmg:DB$ ReplaceEffect | VarName$ DamageAmount | VarValue$ X\n\
             SVar:X:ReplaceCount$DamageAmount/{math}"
        ),
    )
}

fn twin_gods() -> String {
    damage_effect(
        "Test Twin Gods",
        "ValidSource$ Card.YouCtrl,Emblem.YouCtrl | ValidTarget$ Permanent,Player",
        "Twice",
    )
}

fn thane() -> String {
    damage_effect(
        "Test Thane",
        "ValidSource$ Card.YouCtrl,Emblem.YouCtrl | ValidTarget$ Player.Opponent,Permanent.OppCtrl",
        "Plus.2",
    )
}

fn damage_cards(extra: &[String]) -> Vec<String> {
    let mut v = extra.to_vec();
    v.push(
        "Name:Test Bolt\nManaCost:R\nTypes:Instant\n\
         A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 3 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    );
    v.push(
        "Name:Test Lifelinker\nManaCost:W\nTypes:Creature Cleric\nPT:1/1\nK:Lifelink\n\
         A:AB$ DealDamage | Cost$ T | ValidTgts$ Any | NumDmg$ 1 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    );
    v.push(
        "Name:Test Aegis\nManaCost:W\nTypes:Enchantment\n\
         R:Event$ DamageDone | ActiveZones$ Battlefield | ValidTarget$ You | Prevent$ True | Description$ Text.\nOracle:Text."
            .to_string(),
    );
    v
}

fn damage(s: &Script, id: ObjectId) -> u32 {
    s.st.object(id).damage_marked
}

// CR 614.1a, 120.4b (2026-11-06): "if a source you control would deal damage to a
// permanent or player, it deals double that damage instead" — to a player and to a
// permanent; not damage from an opponent's source.
#[test]
fn double_damage_from_your_sources() {
    let mut sc = scenario_n(2, &damage_cards(&[twin_gods()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Twin Gods", "Mountain", "Mountain"],
    );
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Bolt", "Test Bolt"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Bolt").target_player(P1).pay().pass_all();
    assert_eq!(life(&s, P1), 34);
    s.cast("Test Bolt").target_object(golem).pay().pass_all();
    // (6 is lethal for the 2/3: it died.)
    assert!(
        s.st.objects
            .get(&golem)
            .is_none_or(|o| o.zone != Zone::Battlefield)
    );
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Battlefield, "Test Twin Gods").unwrap()),
    )
    .join(" ");
    assert!(text.contains("double that damage"), "{text}");

    // The opponent's Bolt isn't doubled.
    let mut sc = scenario_n(2, &damage_cards(&[twin_gods()]));
    sc.cards(P0, Zone::Battlefield, &["Test Twin Gods"]);
    sc.cards(P1, Zone::Battlefield, &["Mountain"]);
    sc.turn(P1, engine_core::turn::Step::PrecombatMain);
    sc.card(P1, Zone::Hand, "Test Bolt");
    let mut s = Script::new(sc.build());
    s.cast("Test Bolt").target_player(P0).pay().pass_all();
    assert_eq!(life(&s, P0), 37);
}

// "To an opponent or a permanent an opponent controls … that much damage plus 2" — not to
// its controller's own permanents.
#[test]
fn plus_two_to_opponents_and_their_permanents() {
    let mut sc = scenario_n(2, &damage_cards(&[thane()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Thane", "Mountain", "Mountain"],
    );
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    let mine = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Bolt", "Test Bolt"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Bolt").target_player(P1).pay().pass_all();
    assert_eq!(life(&s, P1), 35);
    let _ = theirs;
    s.cast("Test Bolt").target_object(mine).pay().pass_all();
    assert!(
        s.st.objects
            .get(&mine)
            .is_none_or(|o| o.zone != Zone::Battlefield)
    );
    // 3 damage: lethal for the 2/3 either way; the other Golem is untouched.
    assert_eq!(damage(&s, theirs), 0);
}

// CR 616.1, 614.5 (2026-11-06): the player dealt damage chooses the order — plus 2 then
// double is 10, double then plus 2 is 8 — and the heuristic AI picks the smaller.
#[test]
fn cr_616_1_the_damaged_player_orders_them() {
    for (first, dealt) in [("Test Thane", 10), ("Test Twin Gods", 8)] {
        let mut sc = scenario_n(2, &damage_cards(&[twin_gods(), thane()]));
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Test Twin Gods", "Test Thane", "Mountain"],
        );
        sc.card(P0, Zone::Hand, "Test Bolt");
        let mut s = Script::new(sc.build());
        s.cast("Test Bolt").target_player(P1).pay().pass_all();
        assert_eq!(s.who(), P1, "the damaged player chooses");
        choose(&mut s, first);
        assert_eq!(life(&s, P1), 40 - dealt, "{first} first");
    }
    use engine_core::Agent;
    let mut sc = scenario_n(2, &damage_cards(&[twin_gods(), thane()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Thane", "Test Twin Gods", "Mountain"],
    );
    sc.card(P0, Zone::Hand, "Test Bolt");
    let mut s = Script::new(sc.build());
    s.cast("Test Bolt").target_player(P1).pay().pass_all();
    let mut ai = ai::HeuristicAgent::new(1, s.st.catalog.clone());
    let view = engine_core::player_view(&s.st, P1);
    let r = ai.decide(&view, s.decision());
    s.respond(r);
    assert_eq!(life(&s, P1), 32);
}

// CR 615.1a, 615.6: prevented damage isn't dealt, doubled or not; CR 702.15b: lifelink
// gains the damage actually dealt — doubled.
#[test]
fn prevention_and_lifelink_with_doubling() {
    let mut sc = scenario_n(2, &damage_cards(&[twin_gods()]));
    sc.cards(P0, Zone::Battlefield, &["Test Twin Gods", "Mountain"]);
    sc.cards(P1, Zone::Battlefield, &["Test Aegis"]);
    sc.card(P0, Zone::Hand, "Test Bolt");
    let mut s = Script::new(sc.build());
    s.cast("Test Bolt").target_player(P1).pay().pass();
    if s.kind() == ChoiceKind::ChooseReplacement {
        // Either order: nothing is dealt.
        s.respond(engine_core::Response::Pick(vec![0]));
    }
    s.pass_all();
    if s.kind() == ChoiceKind::ChooseReplacement {
        s.respond(engine_core::Response::Pick(vec![0]));
    }
    assert_eq!(life(&s, P1), 40);
    assert_eq!(
        s.count(|e| matches!(e, engine_core::Event::DamageDealt { .. })),
        0
    );

    let mut sc = scenario_n(2, &damage_cards(&[twin_gods()]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Twin Gods", "Test Lifelinker"],
    );
    let mut s = Script::new(sc.build());
    s.activate("Test Lifelinker").target_player(P1).pass_all();
    assert_eq!(life(&s, P1), 38);
    assert_eq!(life(&s, P0), 42);
}

// In a three-player game, "to an opponent" is to each of its controller's opponents, each
// its own event; damage to its controller isn't increased.
#[test]
fn three_players_damage_to_each_opponent() {
    let sweep = "Name:Test Searing Wind\nManaCost:R\nTypes:Sorcery\n\
        A:SP$ DamageAll | NumDmg$ 1 | ValidPlayers$ Player | SpellDescription$ Text.\nOracle:Text.";
    let mut cards = damage_cards(&[thane()]);
    cards.push(sweep.to_string());
    let mut sc = scenario_n(3, &cards);
    sc.cards(P0, Zone::Battlefield, &["Test Thane", "Mountain"]);
    sc.card(P0, Zone::Hand, "Test Searing Wind");
    let mut s = Script::new(sc.build());
    s.cast("Test Searing Wind").pay().pass_all();
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (39, 37, 37));
}

#[test]
fn damage_forms_left_blocked() {
    let blocked = |card: &str, what: &str| {
        let b = forge_test_blockers(card);
        assert!(b.iter().any(|x| x.contains(what)), "{what}: {b:?}");
    };
    // An emblem alternative with no card alternative like it.
    blocked(
        &damage_effect(
            "Test Emblem Only",
            "ValidSource$ Emblem.YouCtrl | ValidTarget$ Player",
            "Twice",
        ),
        "repl:DamageDone source",
    );
    // Redirection.
    blocked(
        &card(
            "Test Redirector",
            "Enchantment",
            "R:Event$ DamageDone | ActiveZones$ Battlefield | ValidTarget$ You | ReplaceWith$ Dmg | DamageTarget$ Self | Description$ Text.\n\
             SVar:Dmg:DB$ ReplaceSplitDamage | DamageTarget$ Self",
        ),
        "repl:DamageDone",
    );
}

// ---------------------------------------------------------------------------------------
// Life gained that is doubled or increased (CR 119.10, 120.4c, 616.1)

fn life_effect(name: &str, math: &str) -> String {
    card(
        name,
        "Enchantment",
        &format!(
            "R:Event$ GainLife | ActiveZones$ Battlefield | ValidPlayer$ You | ReplaceWith$ Gain | AILogic$ DoubleLife | Description$ Text.\n\
             SVar:Gain:DB$ ReplaceEffect | VarName$ LifeGained | VarValue$ X\n\
             SVar:X:ReplaceCount$LifeGained/{math}"
        ),
    )
}

fn life_cards(extra: &[String]) -> Vec<String> {
    let mut v = damage_cards(extra);
    v.push(
        "Name:Test Salve\nManaCost:W\nTypes:Instant\n\
         A:SP$ GainLife | LifeAmount$ 3 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    );
    v.push(watcher(
        "Test Life Watch",
        "Mode$ LifeGained | ValidPlayer$ You",
    ));
    v
}

// CR 119.10, 614.1a (2026-11-06): "if you would gain life, you gain twice that much life
// instead" — its controller's gains only; "whenever you gain life" triggers once.
#[test]
fn twice_that_much_life() {
    let mut sc = scenario_n(2, &life_cards(&[life_effect("Test Faithmender", "Twice")]));
    sc.cards(P0, Zone::Battlefield, &["Test Faithmender", "Plains"]);
    sc.cards(P1, Zone::Battlefield, &["Plains"]);
    sc.card(P0, Zone::Hand, "Test Salve");
    sc.card(P1, Zone::Hand, "Test Salve");
    let mut s = Script::new(sc.build());
    s.cast("Test Salve").pay().pass();
    s.cast("Test Salve").pay().pass_all();
    s.pass_all();
    assert_eq!((life(&s, P0), life(&s, P1)), (46, 43));
}

// CR 120.4c, 702.15b (2026-11-06): lifelink's life gain is a result of the damage, modified
// by replacement effects on life gain — doubled; with the damage doubled too, it gains
// what was dealt, doubled again.
#[test]
fn cr_120_4c_lifelink_gain_is_doubled() {
    let mut sc = scenario_n(2, &life_cards(&[life_effect("Test Faithmender", "Twice")]));
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Faithmender", "Test Lifelinker"],
    );
    let mut s = Script::new(sc.build());
    s.activate("Test Lifelinker").target_player(P1).pass_all();
    assert_eq!((life(&s, P0), life(&s, P1)), (42, 39));

    let mut sc = scenario_n(
        2,
        &life_cards(&[life_effect("Test Faithmender", "Twice"), twin_gods()]),
    );
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Faithmender", "Test Twin Gods", "Test Lifelinker"],
    );
    let mut s = Script::new(sc.build());
    s.activate("Test Lifelinker").target_player(P1).pass_all();
    // 2 dealt, 4 gained, as one gain.
    assert_eq!((life(&s, P0), life(&s, P1)), (44, 38));
    assert_eq!(
        s.count(|e| matches!(
            e,
            engine_core::Event::LifeChanged { player, delta, .. } if *player == P0 && *delta > 0
        )),
        1
    );
}

// CR 616.1 (2026-11-06): the player gaining life orders "plus one" and "twice": 3 becomes
// 8 or 7.
#[test]
fn cr_616_1_order_of_life_gain_effects() {
    for (first, gained) in [("Test Plus Life", 8), ("Test Faithmender", 7)] {
        let mut sc = scenario_n(
            2,
            &life_cards(&[
                life_effect("Test Faithmender", "Twice"),
                life_effect("Test Plus Life", "Plus.1"),
            ]),
        );
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Test Faithmender", "Test Plus Life", "Plains"],
        );
        sc.card(P0, Zone::Hand, "Test Salve");
        let mut s = Script::new(sc.build());
        s.cast("Test Salve").pay().pass_all();
        choose(&mut s, first);
        assert_eq!(life(&s, P0), 40 + gained, "{first} first");
    }
}

// ---------------------------------------------------------------------------------------
// What doesn't compile.

#[test]
fn forms_left_blocked() {
    let blocked = |card: &str, what: &str| {
        let b = forge_test_blockers(card);
        assert!(b.iter().any(|x| x.contains(what)), "{what}: {b:?}");
    };
    // "You may" token replacements.
    blocked(
        &card(
            "Test Maybe",
            "Enchantment",
            "R:Event$ CreateToken | ActiveZones$ Battlefield | ValidToken$ Card.YouCtrl | Optional$ True | ReplaceWith$ R | Description$ Text.\n\
             SVar:R:DB$ ReplaceToken | Type$ ReplaceToken | TokenScript$ c_a_test_clue",
        ),
        "param:repl CreateToken.Optional",
    );
    // Counters on players.
    blocked(
        &card(
            "Test Poisoner",
            "Enchantment",
            "R:Event$ AddCounter | ActiveZones$ Battlefield | ValidPlayer$ You | ReplaceWith$ R | Description$ Text.\n\
             SVar:R:DB$ ReplaceCounter | Amount$ X\nSVar:X:ReplaceCount$CounterNum/Plus.1",
        ),
        "repl:AddCounter:counters on players",
    );
    // A copy of a chosen permanent instead.
    blocked(
        &card(
            "Test Mirror",
            "Enchantment",
            "R:Event$ CreateToken | ActiveZones$ Battlefield | ValidToken$ Card.YouCtrl | ReplaceWith$ R | Description$ Text.\n\
             SVar:R:DB$ ReplaceToken | Type$ ReplaceToken | TokenScript$ Chosen | ValidChoices$ Creature.YouCtrl",
        ),
        "repl:CreateToken:chosen token",
    );
    // Someone else gets them.
    blocked(
        &card(
            "Test Thief",
            "Enchantment",
            "R:Event$ CreateToken | ActiveZones$ Battlefield | ValidToken$ Card.OppCtrl | ReplaceWith$ R | Description$ Text.\n\
             SVar:R:DB$ ReplaceToken | Type$ ReplaceController | NewController$ You",
        ),
        "repl:CreateToken:Type ReplaceController",
    );
    // From another zone.
    blocked(
        &card(
            "Test Graveyard Doubler",
            "Enchantment",
            "R:Event$ CreateToken | ActiveZones$ Graveyard | ReplaceWith$ R | Description$ Text.\n\
             SVar:R:DB$ ReplaceToken | Type$ Amount",
        ),
        "repl:CreateToken zone",
    );
}
