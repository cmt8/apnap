//! Tapping permanents as a cost, convoke, improvise, crew (decisions.md D-087). Invented
//! cards in Forge's script format, compiled the way real ones are and then played.
//! Rules text: docs/rules-notes/d087-tap-costs.md.

use engine_core::ids::ObjectId;
use engine_core::layers::chars;
use engine_core::mana::ManaType;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, OptionDesc, PlayerId, Primitive, Response};
use harness::forge_test_catalog;
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn card(name: &str, cost: &str, types: &str, body: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{types}\n{body}\nOracle:Text.")
}

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc.turn(P0, Step::PrecombatMain);
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

fn settle(s: &mut Script) {
    drive_until(s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
}

fn can_activate(s: &Script, name: &str) -> bool {
    s.has(|s, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if s.name(*source) == name))
}

fn elf() -> String {
    card(
        "Test Mana Elf",
        "G",
        "Creature Elf",
        "PT:1/1\nA:AB$ Mana | Cost$ T | Produced$ G | SpellDescription$ Add {G}.",
    )
}

fn bear() -> String {
    card("Test Grizzly", "1 G", "Creature Bear", "PT:2/2")
}

// CR 601.2g–h (2026-11-06): mana abilities are activated before the cost is paid, so a
// creature tapped for mana can't also be tapped for "tap an untapped creature you control"
// (CR 118.3). CR 302.6 is about a creature's own `{T}` abilities: a creature that came under
// its controller's control this turn can be tapped for another permanent's cost. Forge:
// `Cost$ 1 tapXType<1/Creature>`.
#[test]
fn tap_an_untapped_creature_you_control() {
    let lever = card(
        "Test Drum Lever",
        "2",
        "Artifact",
        "A:AB$ Draw | Cost$ 1 tapXType<1/Creature> | NumCards$ 1 | SpellDescription$ Text.",
    );
    // Only the Elf: it can make the mana or be tapped, not both.
    let mut sc = scenario(&[&lever, &elf(), &bear()]);
    sc.cards(P0, Zone::Battlefield, &["Test Drum Lever", "Test Mana Elf"]);
    let s = Script::new(sc.build());
    assert!(!can_activate(&s, "Test Drum Lever"));
    // With a creature that arrived this turn: the Elf pays {1}, the newcomer is tapped.
    let mut sc = scenario(&[&lever, &elf(), &bear()]);
    sc.cards(P0, Zone::Battlefield, &["Test Drum Lever", "Test Mana Elf"]);
    let cub = sc.card(P0, Zone::Battlefield, "Test Grizzly");
    sc.sick(cub);
    let mut s = Script::new(sc.build());
    let hand = s.st.zones.hands[0].len();
    s.activate("Test Drum Lever");
    // The only mana: the Elf (the Grizzly has none).
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    s.pay();
    assert_eq!(s.kind(), ChoiceKind::TapForCost);
    assert_eq!(
        s.options(),
        [OptionDesc::Card { card: cub }],
        "not the tapped Elf"
    );
    s.respond(Response::Pick(vec![0]));
    assert!(s.st.object(cub).tapped);
    settle(&mut s);
    assert_eq!(s.st.zones.hands[0].len(), hand + 1);
    // Nothing untapped is left: not offered again.
    assert!(!can_activate(&s, "Test Drum Lever"));
    let cat = forge_test_catalog(&[&lever], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Drum Lever").unwrap()));
    assert_eq!(
        text,
        ["{1}, Tap an untapped creature you control: Draw a card."]
    );
}

// CR 702.51a–b: after the total cost is determined, each untapped creature tapped pays a
// mana of its color or one generic mana (never `{C}`). It isn't `{T}`: a creature that
// arrived this turn may convoke (CR 302.6). The spell is offered only if a payment exists.
#[test]
fn convoke_pays_with_creatures() {
    let gift = "Name:Test Convoked Gift\nManaCost:3 G\nTypes:Sorcery\nK:Convoke\n\
        A:SP$ GainLife | LifeAmount$ 5 | SpellDescription$ Text.\n\
        Oracle:Convoke\\nYou gain 5 life."
        .to_string();
    let soldier = card("Test Pikeman", "W", "Creature Soldier", "PT:1/1");
    // Two creatures and a Forest: four mana short of nothing, but only three units.
    let mut sc = scenario(&[&gift, &bear(), &soldier]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Test Grizzly", "Test Pikeman"],
    );
    sc.card(P0, Zone::Hand, "Test Convoked Gift");
    let s = Script::new(sc.build());
    assert!(!s.can_cast("Test Convoked Gift"));
    // Three creatures (one of them new) and a Forest.
    let mut sc = scenario(&[&gift, &bear(), &soldier]);
    sc.card(P0, Zone::Battlefield, "Forest");
    let cub = sc.card(P0, Zone::Battlefield, "Test Grizzly");
    let pike = sc.card(P0, Zone::Battlefield, "Test Pikeman");
    let late = sc.card(P0, Zone::Battlefield, "Test Pikeman");
    sc.sick(late);
    sc.card(P0, Zone::Hand, "Test Convoked Gift");
    let mut s = Script::new(sc.build());
    s.cast("Test Convoked Gift");
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    let opts = s.options().to_vec();
    let pays = |card: ObjectId, mana: Option<ManaType>| OptionDesc::TapToPay { source: card, mana };
    assert!(opts.contains(&pays(cub, Some(ManaType::Green))));
    assert!(opts.contains(&pays(cub, None)));
    assert!(opts.contains(&pays(pike, None)));
    assert!(opts.contains(&pays(late, None)), "CR 302.6 doesn't apply");
    assert!(
        !opts.contains(&pays(pike, Some(ManaType::White))),
        "no white mana in the cost"
    );
    // The Grizzly for {G}, the Pikemen for {2}, the Forest for the last {1}.
    s.pick("Grizzly for G", |_, o| {
        *o == pays(cub, Some(ManaType::Green))
    });
    s.pick("Pikeman", |_, o| *o == pays(pike, None));
    // With {G} paid by the Grizzly, its other use is gone.
    assert!(!s.options().contains(&pays(cub, None)));
    s.pick("the other Pikeman", |_, o| *o == pays(late, None));
    s.pay();
    settle(&mut s);
    assert_eq!(s.st.player(P0).life, 45);
    for c in [cub, pike, late] {
        assert!(s.st.object(c).tapped);
    }
}

// CR 702.126a: each untapped artifact tapped pays one generic mana.
#[test]
fn improvise_pays_generic_with_artifacts() {
    let whir = "Name:Test Improvised Draw\nManaCost:2 U\nTypes:Sorcery\nK:Improvise\n\
        A:SP$ Draw | NumCards$ 2 | SpellDescription$ Text.\n\
        Oracle:Improvise\\nDraw two cards."
        .to_string();
    let bauble = card("Test Bauble", "1", "Artifact", "");
    let mut sc = scenario(&[&whir, &bauble]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Test Bauble", "Test Bauble"],
    );
    sc.card(P0, Zone::Hand, "Test Improvised Draw");
    let s = Script::new(sc.build());
    assert!(
        !s.can_cast("Test Improvised Draw"),
        "an artifact can't pay {{U}}"
    );
    let mut sc = scenario(&[&whir, &bauble]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Island", "Test Bauble", "Test Bauble"],
    );
    sc.card(P0, Zone::Hand, "Test Improvised Draw");
    let mut s = Script::new(sc.build());
    let hand = s.st.zones.hands[0].len();
    s.cast("Test Improvised Draw");
    assert!(
        s.options()
            .iter()
            .all(|o| !matches!(o, OptionDesc::TapToPay { mana: Some(_), .. }))
    );
    settle(&mut s);
    assert_eq!(s.st.zones.hands[0].len(), hand - 1 + 2);
}

// CR 702.122a: "Crew N" — tap any number of other untapped creatures you control with total
// power N or greater: this becomes an artifact creature until end of turn (a layer-4 effect,
// CR 613.1d; it has its printed power and toughness, CR 301.7b). A Vehicle that isn't a
// creature can't attack. Creatures that came under their controller's control this turn can
// crew (it isn't `{T}`, CR 302.6).
#[test]
fn crew_makes_a_vehicle_a_creature_until_end_of_turn() {
    let cart = "Name:Test Cart\nManaCost:2\nTypes:Artifact Vehicle\nPT:3/3\nK:Crew:2\n\
        Oracle:Crew 2 (Tap any number of untapped creatures you control with total power 2 or greater: This Vehicle becomes an artifact creature until end of turn.)"
        .to_string();
    let squire = card("Test Squire", "W", "Creature Soldier", "PT:1/1");
    let mut sc = scenario(&[&cart, &squire, &bear()]);
    let vehicle = sc.card(P0, Zone::Battlefield, "Test Cart");
    let a = sc.card(P0, Zone::Battlefield, "Test Squire");
    let b = sc.card(P0, Zone::Battlefield, "Test Squire");
    sc.sick(b);
    let big = sc.card(P0, Zone::Battlefield, "Test Grizzly");
    let mut s = Script::new(sc.build());
    assert!(!chars(&s.st, vehicle).is(card_model::CardType::Creature));
    s.activate("Test Cart");
    assert_eq!(s.kind(), ChoiceKind::TapForCost);
    assert_eq!(
        s.options(),
        [
            OptionDesc::Card { card: a },
            OptionDesc::Card { card: b },
            OptionDesc::Card { card: big }
        ]
    );
    s.pick("a squire", |_, o| *o == OptionDesc::Card { card: a });
    // Power 1 so far: no stopping yet.
    assert!(!s.options().contains(&OptionDesc::Decline));
    s.pick("the new squire", |_, o| *o == OptionDesc::Card { card: b });
    // Enough: stop, or tap more.
    assert_eq!(
        s.options(),
        [OptionDesc::Decline, OptionDesc::Card { card: big }]
    );
    s.pick("stop", |_, o| *o == OptionDesc::Decline);
    assert!(s.st.object(a).tapped && s.st.object(b).tapped);
    assert!(!s.st.object(big).tapped);
    settle(&mut s);
    let c = chars(&s.st, vehicle);
    assert!(c.is(card_model::CardType::Creature) && c.is(card_model::CardType::Artifact));
    assert_eq!((c.power, c.toughness), (Some(3), Some(3)));
    // It attacks like any creature.
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    assert!(
        s.options()
            .iter()
            .any(|o| matches!(o, OptionDesc::Attack { attacker, .. } if *attacker == vehicle))
    );
    // Until end of turn.
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert!(!chars(&s.st, vehicle).is(card_model::CardType::Creature));
    let cat = forge_test_catalog(&[&cart], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Cart").unwrap()));
    assert_eq!(text, ["Crew 2"]);
}

// The compiled card is read back against its Oracle text: "tap two other untapped creatures
// you control" where the script would let the source itself be tapped is not compiled; with
// `{T}` in the same cost the source can't be tapped twice anyway.
#[test]
fn other_untapped_is_read_against_the_card() {
    let terror = "Name:Test Terror\nManaCost:4 U U\nTypes:Creature Octopus\nPT:6/6\n\
        A:AB$ Pump | Cost$ tapXType<2/Creature> | Defined$ Self | NumAtt$ +1 | SpellDescription$ Text.\n\
        Oracle:Tap two other untapped creatures you control: This creature gets +1/+0 until end of turn."
        .to_string();
    assert!(
        harness::forge_test_blockers(&terror)
            .iter()
            .any(|b| b == "oracle:says other untapped")
    );
    let trapper = "Name:Test Trapper\nManaCost:W\nTypes:Creature Merfolk\nPT:1/1\n\
        A:AB$ Draw | Cost$ 1 T tapXType<1/Creature> | NumCards$ 1 | SpellDescription$ Text.\n\
        Oracle:{1}, {T}, Tap another untapped creature you control: Draw a card."
        .to_string();
    assert!(harness::forge_test_blockers(&trapper).is_empty());
}

// A Vehicle that isn't a creature isn't asked about attacking.
#[test]
fn an_uncrewed_vehicle_cant_attack() {
    let cart = "Name:Test Cart\nManaCost:2\nTypes:Artifact Vehicle\nPT:3/3\nK:Crew:2\n\
        Oracle:Crew 2 (Tap any number of untapped creatures you control with total power 2 or greater: This Vehicle becomes an artifact creature until end of turn.)"
        .to_string();
    let mut sc = scenario(&[&cart, &bear()]);
    let vehicle = sc.card(P0, Zone::Battlefield, "Test Cart");
    sc.card(P0, Zone::Battlefield, "Test Grizzly");
    let mut s = Script::new(sc.build());
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    assert!(
        !s.options()
            .iter()
            .any(|o| matches!(o, OptionDesc::Attack { attacker, .. } if *attacker == vehicle))
    );
    let _ = P1;
}

fn can_attack_with(s: &Script, id: ObjectId) -> bool {
    s.options()
        .iter()
        .any(|o| matches!(o, OptionDesc::Attack { attacker, .. } if *attacker == id))
}

// CR 613.1b (layer 2) and 611.2a: "Gain control of target creature until end of turn. Untap
// that creature. It gains haste until end of turn." CR 302.6: a creature that came under its
// controller's control this turn can't attack unless it has haste. Control returns as the
// effect ends. Without a duration the control change lasts (CR 611.2a). Forge:
// `GainControl | LoseControl$ EOT | Untap$ True | AddKWs$ Haste`.
#[test]
fn gain_control_until_end_of_turn_or_for_good() {
    let treason = card(
        "Test Treason",
        "2 R",
        "Sorcery",
        "A:SP$ GainControl | ValidTgts$ Creature | LoseControl$ EOT | Untap$ True | AddKWs$ Haste | SpellDescription$ Text.",
    )
    .replace(
        "Oracle:Text.",
        "Oracle:Gain control of target creature until end of turn. Untap that creature. It gains haste until end of turn.",
    );
    let piper = card(
        "Test Piper",
        "2 B B",
        "Sorcery",
        "A:SP$ GainControl | ValidTgts$ Creature | SpellDescription$ Text.",
    )
    .replace("Oracle:Text.", "Oracle:Gain control of target creature.");
    let mut sc = scenario(&[&treason, &piper, &bear()]);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 4]);
    sc.cards(P0, Zone::Hand, &["Test Treason", "Test Piper"]);
    let theirs = sc.card(P1, Zone::Battlefield, "Test Grizzly");
    sc.tapped(theirs);
    let other = sc.card(P1, Zone::Battlefield, "Test Grizzly");
    let mut s = Script::new(sc.build());
    s.cast("Test Treason").target_object(theirs).pay();
    settle(&mut s);
    assert_eq!(chars(&s.st, theirs).controller, P0);
    assert!(!s.st.object(theirs).tapped, "untapped");
    assert!(chars(&s.st, theirs).has(card_model::Keyword::Haste));
    // For good, without haste: it can't attack this turn (CR 302.6).
    s.cast("Test Piper").target_object(other).pay();
    settle(&mut s);
    assert_eq!(chars(&s.st, other).controller, P0);
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    assert!(can_attack_with(&s, theirs), "haste");
    assert!(!can_attack_with(&s, other), "summoning sick");
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert_eq!(chars(&s.st, theirs).controller, P1, "until end of turn");
    assert_eq!(chars(&s.st, other).controller, P0, "for good");
    let cat = forge_test_catalog(&[&treason, &piper], &[]);
    let text = |n: &str| card_model::describe::rules_text(cat.get(cat.find(n).unwrap()));
    assert_eq!(
        text("Test Treason"),
        [
            "Gain control of target creature until end of turn. Then untap that creature. Then that creature has haste until end of turn."
        ]
    );
    assert_eq!(text("Test Piper"), ["Gain control of target creature."]);
}

// Text only: a delayed "at the beginning of the next combat this turn" triggers once (CR
// 603.7c), so it doesn't read "each combat".
#[test]
fn gain_control_at_the_next_combat_reads_next() {
    let khan = card(
        "Test Khan Wrath",
        "2 R",
        "Sorcery",
        "A:SP$ DealDamage | ValidTgts$ Creature | NumDmg$ 4 | SubAbility$ DBDelTrig | SpellDescription$ Text.\n\
         SVar:DBDelTrig:DB$ DelayedTrigger | ThisTurn$ True | Mode$ Phase | Phase$ BeginCombat | RememberObjects$ Targeted | Execute$ TrigGainControl | TriggerDescription$ Text.\n\
         SVar:TrigGainControl:DB$ GainControl | Defined$ DelayTriggerRememberedLKI | LoseControl$ EOT | Untap$ True | AddKWs$ Haste",
    )
    .replace(
        "Oracle:Text.",
        "Oracle:Test Khan Wrath deals 4 damage to target creature. At the beginning of the next combat this turn, gain control of that creature until end of turn. Untap it. It gains haste until end of turn.",
    );
    let cat = forge_test_catalog(&[&khan], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Khan Wrath").unwrap()));
    assert!(
        text[0].contains("at the beginning of the next combat this turn, gain control of it"),
        "{text:?}"
    );
}
