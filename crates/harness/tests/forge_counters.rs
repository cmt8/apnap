//! Counters of any kind (D-084): a counter has a name; only +1/+1 and -1/-1 do anything by
//! themselves. "Enters with N [kind] counters", "put / remove N [kind] counters" as costs
//! and effects, doublers and proliferate on any kind. Invented cards in Forge's format.
//! Rules text: docs/rules-notes/122-counters.md.

use card_model::CounterKind;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, ObjectId, OptionDesc, PlayerId};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

fn charge() -> CounterKind {
    CounterKind::Named(card_model::CounterName::new("charge").unwrap())
}

fn n(s: &Script, id: ObjectId, kind: CounterKind) -> u32 {
    s.st.object(id).counters_of(kind)
}

/// "This artifact enters with two charge counters on it. {T}, Remove a charge counter from
/// this artifact: Target player loses 2 life."
const PURSE: &str = "Name:Test Purse\nManaCost:2\nTypes:Artifact\n\
    K:etbCounter:CHARGE:2\n\
    A:AB$ LoseLife | Cost$ T SubCounter<1/CHARGE> | ValidTgts$ Player | LifeAmount$ 2 | SpellDescription$ Text.\n\
    Oracle:This artifact enters with two charge counters on it.\\n{T}, Remove a charge counter from this artifact: Target player loses 2 life.";

// CR 122.1, 122.6 (2026-11-06): a permanent enters with counters of a kind; a cost removes
// them (CR 118.3: it can't be paid without them); they do nothing else by themselves.
#[test]
fn cr_122_1_charge_counters_enter_and_pay_a_cost() {
    let mut sc = scenario(&[PURSE]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Purse");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Purse").pay().pass_all();
    let purse = s.find(P0, Zone::Battlefield, "Test Purse").unwrap();
    assert_eq!(n(&s, purse, charge()), 2);
    s.activate("Test Purse");
    s.target_player(P1).pass_all();
    assert_eq!(s.st.player(P1).life, 38);
    assert_eq!(n(&s, purse, charge()), 1);
    // One without counters (put onto the battlefield as it is, no "enters with"): its
    // ability can't be activated.
    let mut sc = scenario(&[PURSE]);
    let empty = sc.card(P0, Zone::Battlefield, "Test Purse");
    sc.turn(P0, Step::PrecombatMain);
    let s = Script::new(sc.build());
    assert!(
        !s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == empty)),
        "no charge counter: the cost can't be paid"
    );
}

// CR 122.3, 704.5q: +1/+1 and -1/-1 counters annihilate; no other kind does. CR 122.2: a
// permanent that leaves keeps none of its counters.
#[test]
fn cr_122_3_only_plus_and_minus_counters_annihilate() {
    let mut sc = scenario(&[PURSE]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let purse = sc.card(P0, Zone::Battlefield, "Test Purse");
    sc.turn(P0, Step::PrecombatMain);
    let mut st = sc.build();
    for (id, kind, k) in [
        (golem, CounterKind::PlusOne, 2),
        (golem, CounterKind::MinusOne, 1),
        (golem, charge(), 3),
        (purse, charge(), 3),
    ] {
        st.objects.get_mut(&id).unwrap().counters.insert(kind, k);
    }
    let mut s = Script::new(st);
    s.pass();
    assert_eq!(n(&s, golem, CounterKind::PlusOne), 1);
    assert_eq!(n(&s, golem, CounterKind::MinusOne), 0);
    assert_eq!(n(&s, golem, charge()), 3);
    let card = engine_core::actions::move_object(
        &mut s.st,
        purse,
        Zone::Hand,
        engine_core::zones::LibraryPosition::Top,
        None,
    );
    assert!(s.st.object(card).counters.is_empty());
}

/// "If one or more counters would be put on a permanent you control, twice that many are
/// put on it instead."
const SEASON: &str = "Name:Test Counter Season\nManaCost:2 G\nTypes:Enchantment\n\
    R:Event$ AddCounter | ActiveZones$ Battlefield | ValidCard$ Permanent.YouCtrl+inZoneBattlefield | EffectOnly$ True | ReplaceWith$ Repl | Description$ Text.\n\
    SVar:Repl:DB$ ReplaceCounter | ChooseCounter$ True | Amount$ X\n\
    SVar:X:ReplaceCount$CounterNum/Twice\nOracle:Text.";

const SPREAD: &str = "Name:Test Spread\nManaCost:U\nTypes:Sorcery\n\
    A:SP$ Proliferate | SpellDescription$ Text.\nOracle:Proliferate.";

// CR 614.1a, 122.6: a counter doubler doubles counters of any kind an effect puts — those a
// permanent enters with too. CR 701.34a: proliferate gives one more of each kind there.
#[test]
fn cr_701_34a_doublers_and_proliferate_with_any_kind() {
    let mut sc = scenario(&[PURSE, SEASON, SPREAD]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Plains", "Plains", "Island", "Test Counter Season"],
    );
    sc.cards(P0, Zone::Hand, &["Test Purse", "Test Spread"]);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Purse").pay().pass_all();
    let purse = s.find(P0, Zone::Battlefield, "Test Purse").unwrap();
    assert_eq!(n(&s, purse, charge()), 4, "twice two");
    s.cast("Test Spread").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::Proliferate);
    s.pick("the Purse", |_, o| *o == OptionDesc::Card { card: purse });
    assert_eq!(n(&s, purse, charge()), 6, "one more, doubled");
}

// CR 118.12, 122.6: "sacrifice it unless you remove a +1/+1 counter from it" — a cost paid
// as the ability resolves; and "put a -1/-1 counter on this: untap it" as a cost.
#[test]
fn counters_put_and_removed_as_costs() {
    let druid = "Name:Test Druid\nManaCost:1 G\nTypes:Creature Elf Druid\nPT:0/2\n\
        A:AB$ Mana | Cost$ T | Produced$ G | SpellDescription$ Add {G}.\n\
        A:AB$ Untap | Cost$ AddCounter<1/M1M1> | SpellDescription$ Untap CARDNAME.\n\
        Oracle:{T}: Add {G}.\\nPut a -1/-1 counter on this creature: Untap this creature.";
    let mut sc = scenario(&[druid]);
    let d = sc.card(P0, Zone::Battlefield, "Test Druid");
    sc.tapped(d);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.pick(
        "untap",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == d),
    );
    s.pass_all();
    assert!(!s.st.object(d).tapped);
    assert_eq!(n(&s, d, CounterKind::MinusOne), 1);
}

#[test]
fn counter_kinds_with_rules_of_their_own_are_named() {
    for (kind, why) in [
        ("SHIELD", "rules of its own"),
        ("STUN", "rules of its own"),
        ("LOYALTY", "rules of its own"),
        ("P1P0", "+X/+Y"),
        ("Flying", "keyword counter"),
        ("POISON", "on players"),
    ] {
        let card = format!(
            "Name:Test Odd\nManaCost:1\nTypes:Artifact\nK:etbCounter:{kind}:1\nOracle:Text."
        );
        assert!(
            forge_test_blockers(&card).contains(&format!("counter:{kind} ({why})")),
            "{kind}: {:?}",
            forge_test_blockers(&card)
        );
    }
}
