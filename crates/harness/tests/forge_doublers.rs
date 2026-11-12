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
