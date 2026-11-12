//! Other ways to pay for a spell (decisions.md D-072): alternative costs ("you may [cost]
//! rather than pay this spell's mana cost", "if you control a commander, you may cast this
//! spell without paying its mana cost"), flashback, evoke, overload, and the costs they use
//! (exile cards from your hand, return permanents to your hand). Invented cards in Forge's
//! script format, compiled the way real ones are and played. Rules text:
//! docs/rules-notes/118.9-alternative-costs.md.

use engine_core::events::Event;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, OptionDesc, PlayerId, Primitive, Response};
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

/// P0 in their precombat main phase.
fn main_phase(sc: &mut Scenario) {
    sc.turn(P0, Step::PrecombatMain);
}

fn in_zone(s: &Script, p: PlayerId, zone: Zone, name: &str) -> bool {
    s.find(p, zone, name).is_some()
}

fn hand(s: &Script, p: PlayerId) -> usize {
    s.st.zones.list(Zone::Hand, p).len()
}

/// Pick the cards named `names` in a `PickN` over cards.
fn choose(s: &mut Script, kind: ChoiceKind, names: &[&str]) {
    assert_eq!(s.kind(), kind);
    let picks: Vec<usize> = names
        .iter()
        .map(|n| {
            s.options()
                .iter()
                .position(|o| matches!(o, OptionDesc::Card { card } if s.name(*card) == *n))
                .unwrap_or_else(|| panic!("{n} not offered for {kind:?}"))
        })
        .collect();
    s.respond(Response::Pick(picks));
}

fn offered_cards(s: &Script) -> Vec<String> {
    s.options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Card { card } => Some(s.name(*card)),
            _ => None,
        })
        .collect()
}

const CAPTAIN: &str = "Name:Test Squad Captain\nManaCost:2 W\nTypes:Legendary Creature Human Soldier\n\
    PT:2/2\nOracle:";
const CUB: &str = "Name:Test Cub\nManaCost:G\nTypes:Creature Bear\nPT:1/1\nOracle:";
const SPRITE: &str = "Name:Test Sprite\nManaCost:U\nTypes:Creature Faerie\nPT:1/1\nOracle:";
const PEARL: &str = "Name:Test Pearl\nManaCost:U\nTypes:Instant\n\
    A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\nOracle:Draw a card.";

/// "If you control a commander, you may cast this spell without paying its mana cost.
/// Draw two cards."
const GUARD: &str = "Name:Test Guardian Draw\nManaCost:2 U\nTypes:Instant\n\
    S:Mode$ AlternativeCost | ValidSA$ Spell | ValidCard$ Card.Self | ValidPlayer$ You | Cost$ 0 | EffectZone$ All | IsPresent$ Card.IsCommander+YouCtrl | Description$ If you control a commander, you may cast this spell without paying its mana cost.\n\
    A:SP$ Draw | NumCards$ 2 | SpellDescription$ Draw two cards.\n\
    Oracle:If you control a commander, you may cast this spell without paying its mana cost.\\nDraw two cards.";

fn cast_with_options(s: &Script) -> usize {
    s.options()
        .iter()
        .filter(|o| matches!(o, OptionDesc::CastWith { .. }))
        .count()
}

// CR 118.9 (2026-11-06): "You may cast [this object] without paying its mana cost." CR
// 903.3d: "If an effect refers to controlling a commander, it refers to a permanent on the
// battlefield that is a commander." The condition is checked as casting begins.
// Forge: `S:Mode$ AlternativeCost | … | Cost$ 0 | IsPresent$ Card.IsCommander+YouCtrl`.
#[test]
fn free_spell_if_you_control_a_commander() {
    // No commander anywhere; a commander in the command zone: not offered.
    for zone in [None, Some(Zone::Command)] {
        let mut sc = scenario(&[GUARD, CAPTAIN]);
        if let Some(z) = zone {
            sc.commander(P0, z, "Test Squad Captain");
        }
        sc.card(P0, Zone::Hand, "Test Guardian Draw");
        let s = Script::new(sc.build());
        assert!(!s.can_cast_with("Test Guardian Draw"), "{zone:?}");
        assert!(!s.can_cast("Test Guardian Draw"), "no mana");
    }
    // An opponent's commander they control doesn't count.
    let mut sc = scenario(&[GUARD, CAPTAIN]);
    sc.commander(P1, Zone::Battlefield, "Test Squad Captain");
    sc.card(P0, Zone::Hand, "Test Guardian Draw");
    let s = Script::new(sc.build());
    assert!(!s.can_cast_with("Test Guardian Draw"));
    // P0's commander on the battlefield: cast for nothing, with no land at all.
    let mut sc = scenario(&[GUARD, CAPTAIN]);
    sc.commander(P0, Zone::Battlefield, "Test Squad Captain");
    sc.card(P0, Zone::Hand, "Test Guardian Draw");
    let mut s = Script::new(sc.build());
    assert!(
        !s.can_cast("Test Guardian Draw"),
        "its mana cost can't be paid"
    );
    assert_eq!(cast_with_options(&s), 1);
    let before = hand(&s, P0);
    s.cast_with("Test Guardian Draw");
    assert_eq!(s.kind(), ChoiceKind::Priority, "nothing to pay");
    s.pass_all();
    assert_eq!(hand(&s, P0), before - 1 + 2);
    assert!(in_zone(&s, P0, Zone::Graveyard, "Test Guardian Draw"));
}

// CR 118.9c / 202.3: an alternative cost doesn't change the spell's mana cost; CR 903.8:
// commander tax is an additional cost, and CR 118.9d: additional costs and cost increases
// apply to the alternative cost — a commander cast "without paying its mana cost" from the
// command zone still costs {2} per earlier cast.
#[test]
fn commander_tax_applies_to_a_free_commander() {
    let partner = "Name:Test Free Captain\nManaCost:4 U U\nTypes:Legendary Creature Human\nPT:3/3\n\
        S:Mode$ AlternativeCost | ValidSA$ Spell | ValidCard$ Card.Self | ValidPlayer$ You | Cost$ 0 | EffectZone$ All | IsPresent$ Card.IsCommander+YouCtrl | Description$ Text.\n\
        Oracle:";
    let build = |casts: u32, islands: usize| {
        let mut sc = scenario(&[partner, CAPTAIN]);
        sc.commander(P0, Zone::Battlefield, "Test Squad Captain");
        let free = sc.commander(P0, Zone::Command, "Test Free Captain");
        sc.cards(P0, Zone::Battlefield, &vec!["Island"; islands]);
        main_phase(&mut sc);
        let mut st = sc.build();
        let ci = st.object(free).card_instance;
        st.players[0].commander_casts.insert(ci, casts);
        Script::new(st)
    };
    // Never cast before: free.
    let mut s = build(0, 0);
    s.cast_with("Test Free Captain");
    assert_eq!(s.kind(), ChoiceKind::Priority);
    s.pass_all();
    assert!(in_zone(&s, P0, Zone::Battlefield, "Test Free Captain"));
    // Cast once before: {2} of tax, which one Island can't pay.
    let s = build(1, 1);
    assert!(!s.can_cast_with("Test Free Captain"));
    let mut s = build(1, 2);
    s.cast_with("Test Free Captain");
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    s.pay().pass_all();
    assert!(in_zone(&s, P0, Zone::Battlefield, "Test Free Captain"));
    assert!(
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| s.name(id) == "Island")
            .all(|&id| s.st.object(id).tapped)
    );
    // CR 903.8: the cast counts toward future tax.
    let ci =
        s.st.objects
            .values()
            .find(|o| s.st.catalog.get(o.card).name == "Test Free Captain")
            .unwrap()
            .card_instance;
    assert_eq!(s.st.player(P0).commander_casts.get(&ci), Some(&2));
}

// CR 107.3b: "If a player is casting a spell that has an {X} in its mana cost … and an
// effect lets that player cast that spell while paying neither its mana cost nor an
// alternative cost that includes X, then the only legal choice for X is 0." CR 202.3e: on
// the stack its mana value counts X as 0 then.
#[test]
fn without_paying_its_mana_cost_x_is_zero() {
    let bolt = "Name:Test Free Burst\nManaCost:X R\nTypes:Instant\n\
        S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ 0 | Description$ Text.\n\
        A:SP$ GainLife | LifeAmount$ X | SpellDescription$ You gain X life.\n\
        SVar:X:Count$xPaid\nOracle:";
    let mut sc = scenario(&[bolt]);
    sc.card(P0, Zone::Hand, "Test Free Burst");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    let mut s = Script::new(sc.build());
    let life = s.st.player(P0).life;
    s.cast_with("Test Free Burst");
    assert_eq!(s.kind(), ChoiceKind::Priority, "no X is asked: it is 0");
    s.pass_all();
    assert_eq!(s.st.player(P0).life, life);
    // Cast for its mana cost, X is announced as usual.
    let mut sc = scenario(&[bolt]);
    sc.card(P0, Zone::Hand, "Test Free Burst");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Free Burst").x(2).pay().pass_all();
    assert_eq!(s.st.player(P0).life, life + 2);
}

/// "You may pay 1 life and exile a blue card from your hand rather than pay this spell's
/// mana cost. Draw a card."
const WILL: &str = "Name:Test Will\nManaCost:3 U U\nTypes:Instant\n\
    S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ PayLife<1> ExileFromHand<1/Card.Blue+Other> | Description$ Text.\n\
    A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
    Oracle:You may pay 1 life and exile a blue card from your hand rather than pay this spell's mana cost.\\nDraw a card.";

// CR 118.9: "You may [action] rather than pay [this object's] mana cost." CR 601.2h: the
// cards are chosen as the cost is paid; not the card being cast (it is on the stack).
// Forge: `Cost$ PayLife<1> ExileFromHand<1/Card.Blue+Other>`.
#[test]
fn pay_life_and_exile_a_blue_card_rather_than_the_mana_cost() {
    // Only a green card besides it: not offered.
    let mut sc = scenario(&[WILL, PEARL, CUB]);
    sc.card(P0, Zone::Hand, "Test Will");
    sc.card(P0, Zone::Hand, "Test Cub");
    let s = Script::new(sc.build());
    assert!(!s.can_cast_with("Test Will"));
    // A blue card: offered; only the blue card may be exiled.
    let mut sc = scenario(&[WILL, PEARL, CUB]);
    sc.card(P0, Zone::Hand, "Test Will");
    sc.card(P0, Zone::Hand, "Test Cub");
    sc.card(P0, Zone::Hand, "Test Pearl");
    let mut s = Script::new(sc.build());
    let life = s.st.player(P0).life;
    s.cast_with("Test Will");
    assert_eq!(s.kind(), ChoiceKind::ExileFromHand);
    assert_eq!(offered_cards(&s), ["Test Pearl"]);
    choose(&mut s, ChoiceKind::ExileFromHand, &["Test Pearl"]);
    s.pass_all();
    assert_eq!(s.st.player(P0).life, life - 1);
    assert!(in_zone(&s, P0, Zone::Exile, "Test Pearl"));
    assert!(in_zone(&s, P0, Zone::Hand, "Test Cub"));
    assert!(in_zone(&s, P0, Zone::Graveyard, "Test Will"));
    // CR 119.4: with no life to pay, it can't be cast that way.
    let mut sc = scenario(&[WILL, PEARL]);
    sc.card(P0, Zone::Hand, "Test Will");
    sc.card(P0, Zone::Hand, "Test Pearl");
    sc.life(P0, 0);
    let s = Script::new(sc.build());
    assert!(!s.can_cast_with("Test Will"));
}

// CR 118.9, 601.2g–h: "You may return an Island you control to its owner's hand rather
// than pay this spell's mana cost." The Island may be tapped for mana first (it is still
// there). Forge: `Cost$ Return<1/Island>`.
#[test]
fn return_an_island_rather_than_the_mana_cost() {
    let daze = "Name:Test Daze\nManaCost:1 U\nTypes:Instant\n\
        S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ Return<1/Island> | Description$ Text.\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        Oracle:You may return an Island you control to its owner's hand rather than pay this spell's mana cost.\\nDraw a card.";
    // No Island (a Forest): not offered.
    let mut sc = scenario(&[daze, ISLE]);
    sc.card(P0, Zone::Hand, "Test Daze");
    sc.card(P0, Zone::Battlefield, "Forest");
    let s = Script::new(sc.build());
    assert!(!s.can_cast_with("Test Daze"));
    // A tapped Island is enough; an opponent's Island is not.
    let mut sc = scenario(&[daze, ISLE]);
    sc.card(P0, Zone::Hand, "Test Daze");
    sc.card(P1, Zone::Battlefield, "Island");
    let island = sc.card(P0, Zone::Battlefield, "Island");
    sc.tapped(island);
    let mut s = Script::new(sc.build());
    s.cast_with("Test Daze");
    assert_eq!(s.kind(), ChoiceKind::ReturnToHand);
    assert_eq!(s.options().len(), 1, "only P0's Island");
    s.respond(Response::Pick(vec![0]));
    s.pass_all();
    assert!(in_zone(&s, P0, Zone::Hand, "Island"));
    assert!(in_zone(&s, P1, Zone::Battlefield, "Island"));
}

// CR 601.2b, 601.2g–h: return-to-hand as the cost of an activated ability ("{1}, Return a
// land you control to its owner's hand: Draw a card."). Forge: `Cost$ 1 Return<1/Land>`.
#[test]
fn return_a_land_as_an_activation_cost() {
    let moon = "Name:Test Moonfolk\nManaCost:1 U\nTypes:Creature Moonfolk\nPT:1/1\n\
        A:AB$ Draw | Cost$ 1 Return<1/Land> | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        Oracle:{1}, Return a land you control to its owner's hand: Draw a card.";
    let mut sc = scenario(&[moon]);
    sc.card(P0, Zone::Battlefield, "Test Moonfolk");
    sc.card(P0, Zone::Battlefield, "Forest");
    let mut s = Script::new(sc.build());
    let before = hand(&s, P0);
    // The one Forest both pays {1} and goes back to the hand.
    s.activate("Test Moonfolk").pay();
    assert_eq!(s.kind(), ChoiceKind::ReturnToHand);
    s.respond(Response::Pick(vec![0]));
    s.pass_all();
    assert_eq!(hand(&s, P0), before + 2, "the Forest and a card");
}

// CR 118.9: "You may sacrifice a nontoken blue creature rather than pay this spell's mana
// cost." Forge: `Cost$ Sac<1/Creature.Blue+!token/…>`.
#[test]
fn sacrifice_a_blue_creature_rather_than_the_mana_cost() {
    let flare = "Name:Test Flare\nManaCost:1 U U\nTypes:Instant\n\
        S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ Sac<1/Creature.Blue+!token/a nontoken blue creature> | Description$ Text.\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        Oracle:You may sacrifice a nontoken blue creature rather than pay this spell's mana cost.\\nDraw a card.";
    let mut sc = scenario(&[flare, SPRITE, CUB]);
    sc.card(P0, Zone::Hand, "Test Flare");
    sc.card(P0, Zone::Battlefield, "Test Cub");
    let s = Script::new(sc.build());
    assert!(!s.can_cast_with("Test Flare"), "a green creature only");
    let mut sc = scenario(&[flare, SPRITE, CUB]);
    sc.card(P0, Zone::Hand, "Test Flare");
    sc.card(P0, Zone::Battlefield, "Test Cub");
    sc.card(P0, Zone::Battlefield, "Test Sprite");
    let mut s = Script::new(sc.build());
    s.cast_with("Test Flare");
    choose(&mut s, ChoiceKind::Sacrifice, &["Test Sprite"]);
    s.pass_all();
    assert!(in_zone(&s, P0, Zone::Graveyard, "Test Sprite"));
    assert!(in_zone(&s, P0, Zone::Battlefield, "Test Cub"));
}

// CR 118.9 with a condition: "If it's not your turn, you may exile a green card from your
// hand rather than pay this spell's mana cost." Forge: `Condition$ NotPlayerTurn`.
#[test]
fn an_alternative_cost_only_on_another_players_turn() {
    let vigor = "Name:Test Vigor\nManaCost:2 G G\nTypes:Instant\n\
        S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ ExileFromHand<1/Card.Green+Other> | Condition$ NotPlayerTurn | Description$ Text.\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        Oracle:If it's not your turn, you may exile a green card from your hand rather than pay this spell's mana cost.\\nDraw a card.";
    for (active, offered) in [(P0, false), (P1, true)] {
        let mut sc = scenario(&[vigor, CUB]);
        sc.card(P0, Zone::Hand, "Test Vigor");
        sc.card(P0, Zone::Hand, "Test Cub");
        sc.turn(active, Step::PrecombatMain);
        let mut s = Script::new(sc.build());
        if active == P1 {
            s.pass();
        }
        assert_eq!(s.who(), P0);
        assert_eq!(s.can_cast_with("Test Vigor"), offered, "{active:?}'s turn");
    }
}

// CR 118.9d: "If an alternative cost is being paid to cast a spell, any additional costs …
// are applied to that alternative cost." The spell's "as an additional cost, discard a
// card" is still paid when it is cast without paying its mana cost.
#[test]
fn additional_costs_still_apply_to_an_alternative_cost() {
    let lesson = "Name:Test Lesson\nManaCost:3 U\nTypes:Sorcery\n\
        S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ 0 | Description$ Text.\n\
        A:SP$ Draw | Cost$ 3 U Discard<1/Card> | NumCards$ 2 | SpellDescription$ Draw two cards.\n\
        Oracle:";
    let mut sc = scenario(&[lesson, CUB]);
    sc.card(P0, Zone::Hand, "Test Lesson");
    main_phase(&mut sc);
    let s = Script::new(sc.build());
    assert!(!s.can_cast_with("Test Lesson"), "nothing to discard");
    let mut sc = scenario(&[lesson, CUB]);
    sc.card(P0, Zone::Hand, "Test Lesson");
    sc.card(P0, Zone::Hand, "Test Cub");
    main_phase(&mut sc);
    let mut s = Script::new(sc.build());
    s.cast_with("Test Lesson");
    choose(&mut s, ChoiceKind::Discard, &["Test Cub"]);
    s.pass_all();
    assert!(in_zone(&s, P0, Zone::Graveyard, "Test Cub"));
    assert_eq!(hand(&s, P0), 2);
}

// CR 601.2f–h, 605.3a: an alternative cost's mana is paid like any mana cost, also with a
// mana ability that itself costs mana (a Signet: the Forest pays its {1}).
#[test]
fn an_alternative_mana_cost_paid_with_a_signet() {
    let signet = "Name:Test Signet\nManaCost:2\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ 1 T | Produced$ U B | SpellDescription$ Add {U}{B}.\n\
        Oracle:{1}, {T}: Add {U}{B}.";
    let cheap = "Name:Test Cheap Insight\nManaCost:5 U U\nTypes:Instant\n\
        S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ U B | Description$ Text.\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        Oracle:You may pay {U}{B} rather than pay this spell's mana cost.\\nDraw a card.";
    let mut sc = scenario(&[signet, cheap]);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Battlefield, "Test Signet");
    sc.card(P0, Zone::Hand, "Test Cheap Insight");
    let mut s = Script::new(sc.build());
    assert!(!s.can_cast("Test Cheap Insight"));
    s.cast_with("Test Cheap Insight");
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    s.pay().pass_all();
    assert!(in_zone(&s, P0, Zone::Graveyard, "Test Cheap Insight"));
    assert_eq!(s.st.player(P0).mana_pool.total(), 0);
}

const DRIFTER: &str = "Name:Test Drifter\nManaCost:4 U\nTypes:Creature Elemental\nPT:2/2\n\
    K:Flying\n\
    K:Evoke:1\n\
    T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigDraw | TriggerDescription$ When CARDNAME enters, draw two cards.\n\
    SVar:TrigDraw:DB$ Draw | Defined$ You | NumCards$ 2\n\
    Oracle:Flying\\nWhen this creature enters, draw two cards.\\nEvoke {1} (You may cast this spell for its evoke cost. If you do, it's sacrificed when it enters.)";

// CR 106.6: restricted mana looks at the spell, not at the cost it pays: "spend this mana
// only to cast creature spells" pays a creature spell's evoke cost, and no instant's
// alternative cost.
#[test]
fn restricted_mana_pays_an_alternative_cost_of_a_spell_it_may_pay_for() {
    let cave = "Name:Test Cavern\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ C | RestrictValid$ Spell.Creature | SpellDescription$ Add {C}. Spend this mana only to cast creature spells.\n\
        Oracle:{T}: Add {C}. Spend this mana only to cast creature spells.";
    let cheap = "Name:Test Cheap Thought\nManaCost:5 U\nTypes:Instant\n\
        S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ 1 | Description$ Text.\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        Oracle:You may pay {1} rather than pay this spell's mana cost.\\nDraw a card.";
    let mut sc = scenario(&[cave, cheap, DRIFTER]);
    sc.card(P0, Zone::Battlefield, "Test Cavern");
    sc.card(P0, Zone::Hand, "Test Cheap Thought");
    sc.card(P0, Zone::Hand, "Test Drifter");
    main_phase(&mut sc);
    let mut s = Script::new(sc.build());
    assert!(!s.can_cast_with("Test Cheap Thought"), "an instant");
    s.cast_with("Test Drifter").pay();
    assert_eq!(s.kind(), ChoiceKind::Priority);
    s.pass_all();
}

// CR 702.74a: "Evoke [cost]" means "You may cast this card by paying [cost] rather than
// paying its mana cost" and "When this permanent enters, if its evoke cost was paid, its
// controller sacrifices it." Both enter triggers happen; cast normally it stays.
#[test]
fn evoke_sacrifices_it_when_it_enters_only_if_evoked() {
    let mut sc = scenario(&[DRIFTER]);
    sc.card(P0, Zone::Hand, "Test Drifter");
    sc.card(P0, Zone::Battlefield, "Island");
    main_phase(&mut sc);
    let mut s = Script::new(sc.build());
    let before = hand(&s, P0);
    assert!(!s.can_cast("Test Drifter"));
    s.cast_with("Test Drifter").pay().pass_all();
    // Two triggers: order them (any order), then resolve both.
    for _ in 0..10 {
        match s.kind() {
            ChoiceKind::OrderTriggers => {
                let n = s.options().len();
                s.respond(Response::Order((0..n).collect()));
            }
            ChoiceKind::Priority if !s.st.stack.is_empty() => {
                s.pass_all();
            }
            _ => break,
        }
    }
    assert!(in_zone(&s, P0, Zone::Graveyard, "Test Drifter"));
    assert_eq!(hand(&s, P0), before - 1 + 2);
    // Cast for its mana cost: it stays.
    let mut sc = scenario(&[DRIFTER]);
    sc.card(P0, Zone::Hand, "Test Drifter");
    sc.cards(P0, Zone::Battlefield, &["Island"; 5]);
    main_phase(&mut sc);
    let mut s = Script::new(sc.build());
    s.cast("Test Drifter").pay().pass_all();
    for _ in 0..10 {
        if s.kind() == ChoiceKind::Priority && !s.st.stack.is_empty() {
            s.pass_all();
        } else {
            break;
        }
    }
    assert!(in_zone(&s, P0, Zone::Battlefield, "Test Drifter"));
}

// CR 702.74a with a cost that isn't mana: "Evoke—Exile a white card from your hand."
#[test]
fn evoke_by_exiling_a_card() {
    let solo = "Name:Test Lonely\nManaCost:3 W W\nTypes:Creature Elemental\nPT:3/2\n\
        K:Evoke:ExileFromHand<1/Card.White+Other/white card>\n\
        Oracle:Evoke—Exile a white card from your hand.";
    let white = "Name:Test Dove\nManaCost:W\nTypes:Creature Bird\nPT:1/1\nOracle:";
    let mut sc = scenario(&[solo, white]);
    sc.card(P0, Zone::Hand, "Test Lonely");
    sc.card(P0, Zone::Hand, "Test Dove");
    main_phase(&mut sc);
    let mut s = Script::new(sc.build());
    s.cast_with("Test Lonely");
    choose(&mut s, ChoiceKind::ExileFromHand, &["Test Dove"]);
    s.pass_all();
    // Its sacrifice trigger.
    s.pass_all();
    assert!(in_zone(&s, P0, Zone::Exile, "Test Dove"));
    assert!(in_zone(&s, P0, Zone::Graveyard, "Test Lonely"));
}

const LOOTING: &str = "Name:Test Looting\nManaCost:R\nTypes:Sorcery\n\
    K:Flashback:2 R\n\
    A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
    Oracle:Draw a card.\\nFlashback {2}{R}";

// CR 702.34a: "Flashback [cost]" means "You may cast this card from your graveyard … by
// paying [cost] rather than paying its mana cost" and "If the flashback cost was paid,
// exile this card instead of putting it anywhere else any time it would leave the stack."
#[test]
fn flashback_casts_from_the_graveyard_and_exiles() {
    let mut sc = scenario(&[LOOTING]);
    sc.card(P0, Zone::Graveyard, "Test Looting");
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    main_phase(&mut sc);
    let mut s = Script::new(sc.build());
    assert!(
        !s.can_cast("Test Looting"),
        "not from the graveyard for its mana cost"
    );
    s.cast_with("Test Looting").pay().pass_all();
    assert!(in_zone(&s, P0, Zone::Exile, "Test Looting"));
    assert!(!in_zone(&s, P0, Zone::Graveyard, "Test Looting"));
    // From the hand: no flashback; cast normally it goes to the graveyard.
    let mut sc = scenario(&[LOOTING]);
    sc.card(P0, Zone::Hand, "Test Looting");
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    main_phase(&mut sc);
    let mut s = Script::new(sc.build());
    assert_eq!(cast_with_options(&s), 0);
    s.cast("Test Looting").pay().pass_all();
    assert!(in_zone(&s, P0, Zone::Graveyard, "Test Looting"));
    // CR 702.34a / 307.1: a sorcery with flashback still has sorcery timing.
    let mut sc = scenario(&[LOOTING]);
    sc.card(P0, Zone::Graveyard, "Test Looting");
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    sc.turn(P0, Step::Upkeep);
    let s = Script::new(sc.build());
    assert!(!s.can_cast_with("Test Looting"));
}

// CR 701.6a and 702.34a: a countered flashback spell is exiled, not put into the
// graveyard.
#[test]
fn a_countered_flashback_spell_is_exiled() {
    let think = "Name:Test Think Again\nManaCost:1 U\nTypes:Instant\n\
        K:Flashback:U\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        Oracle:Draw a card.\\nFlashback {U}";
    let counter = "Name:Test Denial\nManaCost:U\nTypes:Instant\n\
        A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | Destination$ Graveyard | SpellDescription$ Counter target spell.\n\
        Oracle:Counter target spell.";
    let mut sc = scenario(&[think, counter]);
    sc.card(P0, Zone::Graveyard, "Test Think Again");
    sc.card(P0, Zone::Battlefield, "Island");
    sc.card(P1, Zone::Hand, "Test Denial");
    sc.card(P1, Zone::Battlefield, "Island");
    let mut s = Script::new(sc.build());
    s.cast_with("Test Think Again").pay().pass();
    assert_eq!(s.who(), P1);
    s.cast("Test Denial")
        .target("Test Think Again")
        .pay()
        .pass_all();
    assert!(in_zone(&s, P0, Zone::Exile, "Test Think Again"));
    assert!(
        in_zone(&s, P1, Zone::Graveyard, "Test Denial"),
        "Destination$ Graveyard"
    );
    assert_eq!(s.count(|e| matches!(e, Event::Countered { .. })), 1);
}

// CR 702.34a with a cost that isn't mana: "Flashback—Sacrifice two creatures."
#[test]
fn flashback_by_sacrificing() {
    let ret = "Name:Test Return\nManaCost:2 B B\nTypes:Sorcery\n\
        K:Flashback:Sac<2/Creature>\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        Oracle:Draw a card.\\nFlashback—Sacrifice two creatures.";
    let mut sc = scenario(&[ret, CUB]);
    sc.card(P0, Zone::Graveyard, "Test Return");
    sc.card(P0, Zone::Battlefield, "Test Cub");
    main_phase(&mut sc);
    let s = Script::new(sc.build());
    assert!(!s.can_cast_with("Test Return"), "one creature");
    let mut sc = scenario(&[ret, CUB]);
    sc.card(P0, Zone::Graveyard, "Test Return");
    sc.cards(P0, Zone::Battlefield, &["Test Cub", "Test Cub"]);
    main_phase(&mut sc);
    let mut s = Script::new(sc.build());
    s.cast_with("Test Return");
    assert_eq!(s.kind(), ChoiceKind::Sacrifice);
    s.respond(Response::Pick(vec![0, 1]));
    s.pass_all();
    assert!(in_zone(&s, P0, Zone::Exile, "Test Return"));
    assert_eq!(s.st.zones.list(Zone::Graveyard, P0).len(), 2);
}

const RIFT: &str = "Name:Test Rift\nManaCost:1 U\nTypes:Instant\n\
    A:SP$ ChangeZone | ValidTgts$ Permanent.nonLand+YouDontCtrl | TgtPrompt$ Select target nonland permanent you don't control | Origin$ Battlefield | Destination$ Hand | SpellDescription$ Return target nonland permanent you don't control to its owner's hand.\n\
    K:Overload:3 U\n\
    Oracle:Return target nonland permanent you don't control to its owner's hand.\\nOverload {3}{U}";

// CR 702.96a–b: overload changes "target" to "each"; the overloaded spell has no targets
// and affects every object that matches. "You don't control" is every other player's in a
// game without teams (CR 102.3).
#[test]
fn overload_returns_each_nonland_permanent_you_dont_control() {
    let mut sc = scenario_n(3, &[RIFT, CUB, SPRITE]);
    sc.card(P0, Zone::Hand, "Test Rift");
    sc.cards(P0, Zone::Battlefield, &["Island"; 4]);
    sc.card(P0, Zone::Battlefield, "Test Sprite");
    sc.card(P1, Zone::Battlefield, "Test Cub");
    sc.card(P1, Zone::Battlefield, "Forest");
    sc.card(P2, Zone::Battlefield, "Test Sprite");
    let mut s = Script::new(sc.build());
    s.cast_with("Test Rift");
    assert_eq!(s.kind(), ChoiceKind::PayMana, "no target is chosen");
    s.pay().pass_all();
    assert!(in_zone(&s, P1, Zone::Hand, "Test Cub"));
    assert!(in_zone(&s, P2, Zone::Hand, "Test Sprite"));
    assert!(in_zone(&s, P1, Zone::Battlefield, "Forest"), "a land");
    assert!(
        in_zone(&s, P0, Zone::Battlefield, "Test Sprite"),
        "P0's own"
    );
    // Not overloaded: one target, among other players' nonland permanents.
    let mut sc = scenario_n(3, &[RIFT, CUB, SPRITE]);
    sc.card(P0, Zone::Hand, "Test Rift");
    sc.cards(P0, Zone::Battlefield, &["Island"; 2]);
    sc.card(P0, Zone::Battlefield, "Test Sprite");
    sc.card(P1, Zone::Battlefield, "Test Cub");
    sc.card(P2, Zone::Battlefield, "Test Sprite");
    let mut s = Script::new(sc.build());
    s.cast("Test Rift");
    assert!(matches!(s.kind(), ChoiceKind::ChooseTargets { .. }));
    assert_eq!(s.options().len(), 2);
    s.target("Test Cub").pay().pass_all();
    assert!(in_zone(&s, P1, Zone::Hand, "Test Cub"));
    assert!(in_zone(&s, P2, Zone::Battlefield, "Test Sprite"));
}

// CR 702.96b: an overloaded spell needs no targets, so it can be cast when nothing could be
// targeted (D-019: casting is offered only when it can be completed).
#[test]
fn overload_needs_no_target() {
    let mut sc = scenario(&[RIFT]);
    sc.card(P0, Zone::Hand, "Test Rift");
    sc.cards(P0, Zone::Battlefield, &["Island"; 4]);
    let s = Script::new(sc.build());
    assert!(!s.can_cast("Test Rift"), "no target");
    assert!(s.can_cast_with("Test Rift"));
}

// CR 903.3d with "your commander": a Lieutenant's "as long as you control your commander".
// Forge: `IsPresent$ Card.IsCommander+YouOwn+YouCtrl` on a static ability.
#[test]
fn as_long_as_you_control_your_commander() {
    let lieutenant = "Name:Test Lieutenant\nManaCost:2 W\nTypes:Creature Human\nPT:2/2\n\
        S:Mode$ Continuous | Affected$ Card.Self | AddPower$ 2 | AddToughness$ 2 | IsPresent$ Card.IsCommander+YouOwn+YouCtrl | Description$ Text.\n\
        Oracle:";
    for (zone, power) in [(Zone::Battlefield, 4), (Zone::Command, 2)] {
        let mut sc = scenario(&[lieutenant, CAPTAIN]);
        let lt = sc.card(P0, Zone::Battlefield, "Test Lieutenant");
        sc.commander(P0, zone, "Test Squad Captain");
        let s = Script::new(sc.build());
        assert_eq!(engine_core::layers::chars(&s.st, lt).power, Some(power));
    }
    // Another player's commander doesn't count.
    let mut sc = scenario(&[lieutenant, CAPTAIN]);
    let lt = sc.card(P0, Zone::Battlefield, "Test Lieutenant");
    sc.commander(P1, Zone::Battlefield, "Test Squad Captain");
    let s = Script::new(sc.build());
    assert_eq!(engine_core::layers::chars(&s.st, lt).power, Some(2));
}

// CR 601.3 and 118.9a: a rule that lets a player cast spells from their graveyard is a
// permission, not an alternative cost; a free spell cast from there under it is cast for
// its one alternative cost (D-070's `PlayFrom` with D-072).
#[test]
fn a_free_spell_cast_from_the_graveyard_under_a_rule() {
    let crypt = "Name:Test Crypt\nManaCost:2\nTypes:Artifact\n\
        S:Mode$ Continuous | Affected$ Instant.YouOwn | MayPlay$ True | AffectedZone$ Graveyard | Description$ Text.\n\
        Oracle:";
    let mut sc = scenario(&[GUARD, CAPTAIN, crypt]);
    sc.card(P0, Zone::Battlefield, "Test Crypt");
    sc.commander(P0, Zone::Battlefield, "Test Squad Captain");
    sc.card(P0, Zone::Graveyard, "Test Guardian Draw");
    let mut s = Script::new(sc.build());
    assert_eq!(cast_with_options(&s), 1);
    s.cast_with("Test Guardian Draw");
    assert_eq!(s.kind(), ChoiceKind::Priority);
    s.pass_all();
    assert!(in_zone(&s, P0, Zone::Graveyard, "Test Guardian Draw"));
    assert_eq!(hand(&s, P0), 2);
    // Without the rule it can't be cast from the graveyard at all.
    let mut sc = scenario(&[GUARD, CAPTAIN]);
    sc.commander(P0, Zone::Battlefield, "Test Squad Captain");
    sc.card(P0, Zone::Graveyard, "Test Guardian Draw");
    let s = Script::new(sc.build());
    assert_eq!(cast_with_options(&s), 0);
}

// The engine's text of the compiled cards.
#[test]
fn texts_of_alternative_costs() {
    let cat = forge_test_catalog(&[GUARD, WILL, LOOTING, RIFT, DRIFTER], &[]);
    let text = |name: &str| card_model::describe::rules_text(cat.get(cat.find(name).unwrap()));
    assert_eq!(
        text("Test Guardian Draw")[0],
        "If you control a commander, you may cast this spell without paying its mana cost."
    );
    assert_eq!(
        text("Test Will")[0],
        "You may pay 1 life and exile a blue card from your hand rather than pay this \
         spell's mana cost."
    );
    assert!(text("Test Looting").contains(&"Flashback {2}{R}".to_string()));
    assert!(text("Test Rift").contains(&"Overload {3}{U}".to_string()));
    assert!(text("Test Drifter").contains(&"Evoke {1}".to_string()));
}

// What isn't compiled is named (CLAUDE.md invariant 15).
#[test]
fn forms_left_blocked() {
    let base = |line: &str| {
        format!(
            "Name:Test Blocked\nManaCost:2 U\nTypes:Instant\n{line}\n\
             A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\nOracle:"
        )
    };
    let has = |card: String, b: &str| {
        let blockers = forge_test_blockers(&card);
        assert!(blockers.iter().any(|x| x == b), "{b}: {blockers:?}");
    };
    // An alternative cost a permanent gives other spells (Dream Halls, Omniscience).
    has(
        base(
            "S:Mode$ AlternativeCost | ValidSA$ Spell | Cost$ 0 | EffectZone$ All | Description$ Text.",
        ),
        "static:AlternativeCost of other spells or abilities",
    );
    // `Announce$` (an X chosen for the alternative cost), not read.
    has(
        base(
            "S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ 1 | Announce$ X | Description$ Text.",
        ),
        "param:static AlternativeCost.Announce",
    );
    // Works only from one zone.
    has(
        base("S:Mode$ AlternativeCost | ValidSA$ Spell.Self | Cost$ 1 | Description$ Text."),
        "static:AlternativeCost from one zone",
    );
    // Flashback with extra parameters ("this spell costs {X} less to cast this way").
    has(
        base("K:Flashback:8 U:ReduceCost$ X:Text"),
        "kw:Flashback with parameters",
    );
    // Overload of a spell of two instructions.
    has(
        "Name:Test Blocked\nManaCost:R\nTypes:Instant\n\
         A:SP$ Destroy | ValidTgts$ Artifact | SubAbility$ DBDraw | SpellDescription$ Text.\n\
         SVar:DBDraw:DB$ Draw | NumCards$ 1\nK:Overload:2 R\nOracle:"
            .to_string(),
        "kw:Overload of more than one instruction",
    );
    // An ability activated from the hand whose cost could exile its own card.
    has(
        "Name:Test Blocked\nManaCost:R\nTypes:Creature Elf\nPT:1/1\n\
         A:AB$ Draw | Cost$ ExileFromHand<1/Card> | ActivationZone$ Hand | NumCards$ 1 | SpellDescription$ Text.\nOracle:"
            .to_string(),
        "cost:ExileFromHand-maybe-itself",
    );
    // A counterspell that puts the spell somewhere else.
    has(
        "Name:Test Blocked\nManaCost:U\nTypes:Instant\n\
         A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | Destination$ Exile | SpellDescription$ Text.\nOracle:"
            .to_string(),
        "param:Counter.Destination",
    );
}

// The heuristic agent takes a free spell when its condition holds and doesn't pitch cards.
#[test]
fn the_heuristic_agent_takes_free_spells_and_keeps_its_cards() {
    use engine_core::agent::Agent;
    let mut sc = scenario(&[GUARD, CAPTAIN, WILL, PEARL]);
    sc.commander(P0, Zone::Battlefield, "Test Squad Captain");
    sc.card(P0, Zone::Hand, "Test Guardian Draw");
    sc.card(P0, Zone::Hand, "Test Will");
    sc.card(P0, Zone::Hand, "Test Pearl");
    main_phase(&mut sc);
    let s = Script::new(sc.build());
    let view = engine_core::player_view(&s.st, P0);
    let mut ai = ai::HeuristicAgent::new(1, s.st.catalog.clone());
    let r = ai.decide(&view, s.decision());
    let Response::Pick(p) = r else { panic!() };
    match &s.options()[p[0]] {
        OptionDesc::CastWith { card, .. } => assert_eq!(s.name(*card), "Test Guardian Draw"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(s.decision().prim, Primitive::PickN { .. }));
}

/// A land with the Island type, so that the test catalog knows the subtype.
const ISLE: &str = "Name:Test Isle\nManaCost:no cost\nTypes:Land Island\nOracle:";
