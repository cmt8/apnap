//! Effects for, and choices by, a player other than "you" (decisions.md D-068): "its
//! controller", "its owner", "that player". Invented cards written in Forge's script format
//! are compiled the way real ones are and then played, in three- and four-player games
//! where "that player" can't be confused with "an opponent". Rules text:
//! docs/rules-notes/608.2h-its-controller-that-player.md.

use card_model::{Effect, Measure, PlayerRef};
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::view::player_view;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, Event, GameConfig, ObjectId, OptionDesc, PlayerId, Response};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);

const BEAST: (&str, &str) = (
    "g_3_3_test_beast",
    "Name:Test Beast Token\nManaCost:no cost\nColors:green\nTypes:Creature Beast\nPT:3/3\nOracle:",
);

fn scenario(players: u8, cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(
        GameConfig::commander(players),
        forge_test_catalog(cards, &[BEAST]),
        31,
    );
    for p in 0..players {
        sc.library(PlayerId(p), "Forest", 20);
    }
    sc
}

fn spell(name: &str, cost: &str, kind: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{kind}\n{rest}\nOracle:Text.")
}

fn permanent(name: &str, cost: &str, kind: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{kind}\n{rest}\nOracle:Text.")
}

fn creature(name: &str, cost: &str, pt: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:Creature Elf\nPT:{pt}\n{rest}\nOracle:Text.")
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

fn hand(s: &Script, p: PlayerId) -> usize {
    s.st.zones.list(Zone::Hand, p).len()
}

fn on_battlefield(s: &Script, id: ObjectId) -> bool {
    s.st.objects
        .get(&id)
        .is_some_and(|o| o.zone == Zone::Battlefield)
}

/// The permanents named `name` on the battlefield, with (owner, controller).
fn permanents(s: &Script, name: &str) -> Vec<(PlayerId, PlayerId)> {
    s.st.zones
        .battlefield
        .iter()
        .filter(|id| s.name(**id) == name)
        .map(|id| {
            (
                s.st.object(*id).owner,
                engine_core::layers::chars(&s.st, *id).controller,
            )
        })
        .collect()
}

fn shuffles(s: &Script, p: PlayerId) -> usize {
    s.count(|e| matches!(e, Event::Shuffled { player } if *player == p))
}

/// Pass priority until the stack is empty and someone has priority again.
fn settle(s: &mut Script) {
    for _ in 0..50 {
        if s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority {
            return;
        }
        assert_eq!(s.kind(), ChoiceKind::Priority, "settle hit {:?}", s.kind());
        s.pass();
    }
    panic!("the stack did not empty");
}

/// Pass priority until a decision other than priority comes up.
fn pass_until_asked(s: &mut Script) {
    for _ in 0..50 {
        if s.kind() != ChoiceKind::Priority {
            return;
        }
        s.pass();
    }
    panic!("nobody was asked anything");
}

fn accept(s: &mut Script, who: PlayerId, kind: ChoiceKind) {
    pass_until_asked(s);
    assert_eq!((s.who(), s.kind()), (who, kind));
    s.pick("accept", |_, o| matches!(o, OptionDesc::Accept));
}

fn decline(s: &mut Script, who: PlayerId, kind: ChoiceKind) {
    pass_until_asked(s);
    assert_eq!((s.who(), s.kind()), (who, kind));
    s.pick("decline", |_, o| matches!(o, OptionDesc::Decline));
}

/// The only ability of the only spell/ability card `name` in the catalog, as compiled.
fn compiled_effect(s: &Script, name: &str) -> Effect {
    let def = s.st.catalog.get(s.st.catalog.find(name).unwrap());
    match &def.abilities[..] {
        [card_model::AbilityDef::Spell(card_model::Body::Effect(m))] => m.effect.clone(),
        other => panic!("{other:?}"),
    }
}

// A static ability that makes the creatures of its controller bigger, so that a power read
// off the battlefield differs from the printed one.
fn banner() -> String {
    permanent(
        "Test Banner",
        "2",
        "Artifact",
        "S:Mode$ Continuous | Affected$ Creature.YouCtrl | AddPower$ 2 | Description$ Text.",
    )
}

// ---------------------------------------------------------------------------------------
// "Its controller", "its power": of a target, as it last existed.

// CR 608.2h (2026-11-06): the information comes from the object as it last existed on the
// battlefield once it has left. Forge: `GainLife | Defined$ TargetedController | LifeAmount$
// X` with `SVar:X:Targeted$CardPower` after exiling the target (Swords to Plowshares).
#[test]
fn its_controller_gains_life_equal_to_its_power() {
    let cards = [
        spell(
            "Test Plowing",
            "W",
            "Instant",
            "A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Exile | SubAbility$ DBGainLife | SpellDescription$ Text.\n\
             SVar:DBGainLife:DB$ GainLife | Defined$ TargetedController | LifeAmount$ X\n\
             SVar:X:Targeted$CardPower",
        ),
        banner(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Plains");
    let giant = sc.card(P2, Zone::Battlefield, "Test Giant"); // 5/4
    sc.card(P2, Zone::Battlefield, "Test Banner"); // 7/4 while P2 controls it
    sc.card(P0, Zone::Hand, "Test Plowing");
    let mut s = Script::new(sc.build());
    assert_eq!(
        compiled_effect(&s, "Test Plowing"),
        Effect::Seq(vec![
            Effect::Exile {
                what: card_model::ObjRef::Target(0)
            },
            Effect::GainLife {
                who: PlayerRef::ControllerOf(0),
                amount: card_model::Amount::OfTarget {
                    slot: 0,
                    what: Measure::Power
                },
            },
        ])
    );
    s.cast("Test Plowing").target_object(giant).pay();
    settle(&mut s);
    assert!(!on_battlefield(&s, giant));
    // Its controller — not the caster, not the other opponent — gains its power as it
    // last was (7, with the Banner), not as printed (5).
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (40, 40, 47));
}

// CR 608.2h (2026-11-06): last known information is the object as it *last* existed there —
// changed by an earlier instruction of the same spell, not as resolution began.
#[test]
fn its_power_as_it_last_existed_during_this_resolution() {
    let cards = [spell(
        "Test Uplifting",
        "W",
        "Instant",
        "A:SP$ Pump | ValidTgts$ Creature | NumAtt$ +3 | SubAbility$ DBExile | SpellDescription$ Text.\n\
         SVar:DBExile:DB$ ChangeZone | Defined$ Targeted | Origin$ Battlefield | Destination$ Exile | SubAbility$ DBGainLife\n\
         SVar:DBGainLife:DB$ GainLife | Defined$ TargetedController | LifeAmount$ X\n\
         SVar:X:Targeted$CardPower",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Plains");
    let giant = sc.card(P1, Zone::Battlefield, "Test Giant"); // 5/4
    sc.card(P0, Zone::Hand, "Test Uplifting");
    let mut s = Script::new(sc.build());
    s.cast("Test Uplifting").target_object(giant).pay();
    settle(&mut s);
    assert!(!on_battlefield(&s, giant));
    assert_eq!(
        (life(&s, P0), life(&s, P1)),
        (40, 48),
        "5 + 3, as it was when exiled"
    );
}

// CR 108.3, 109.4, 608.2h (2026-11-06): its owner is who brought it into the game; its
// controller is whoever controls it now — here not the same player. Forge: `Defined$
// TargetedOwner` and `Defined$ TargetedController`.
#[test]
fn its_owner_and_its_controller_differ_under_a_control_change() {
    let cards = [spell(
        "Test Reckoning",
        "B",
        "Instant",
        "A:SP$ Destroy | ValidTgts$ Creature | SubAbility$ DBGain | SpellDescription$ Text.\n\
         SVar:DBGain:DB$ GainLife | Defined$ TargetedOwner | LifeAmount$ 4 | SubAbility$ DBLose\n\
         SVar:DBLose:DB$ LoseLife | Defined$ TargetedController | LifeAmount$ 2",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    // P1's Golem, stolen by P0's Puppetry.
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    let puppetry = sc.card(P0, Zone::Battlefield, "Test Puppetry");
    sc.attach(puppetry, golem);
    sc.card(P2, Zone::Battlefield, "Swamp");
    sc.card(P2, Zone::Hand, "Test Reckoning");
    sc.turn(P2, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    assert_eq!(engine_core::layers::chars(&s.st, golem).controller, P0);
    s.cast("Test Reckoning").target_object(golem).pay();
    settle(&mut s);
    assert!(s.find(P1, Zone::Graveyard, "Test Golem").is_some());
    // The owner gains 4, the controller loses 2, the caster neither.
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (38, 44, 40));
}

// CR 608.2b (2026-11-06): "If part of the effect requires information about an illegal
// target, it fails to determine any such information. Any part of the effect that requires
// that information won't happen." One mode's target is gone; the other mode still happens.
#[test]
fn an_illegal_target_tells_nothing() {
    let cards = [
        spell(
            "Test Dilemma",
            "W",
            "Instant",
            "A:SP$ Charm | Choices$ DBShatter,DBPlow | CharmNum$ 2\n\
             SVar:DBShatter:DB$ Destroy | ValidTgts$ Artifact | SpellDescription$ Text.\n\
             SVar:DBPlow:DB$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Exile | SubAbility$ DBGainLife | SpellDescription$ Text.\n\
             SVar:DBGainLife:DB$ GainLife | Defined$ TargetedController | LifeAmount$ X\n\
             SVar:X:Targeted$CardPower",
        ),
        banner(),
        spell(
            "Test Smiting",
            "B",
            "Instant",
            "A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Plains");
    let banner = sc.card(P1, Zone::Battlefield, "Test Banner");
    let giant = sc.card(P1, Zone::Battlefield, "Test Giant");
    sc.card(P0, Zone::Hand, "Test Dilemma");
    sc.card(P1, Zone::Battlefield, "Swamp");
    sc.card(P1, Zone::Hand, "Test Smiting");
    let mut s = Script::new(sc.build());
    s.cast("Test Dilemma");
    assert_eq!(s.kind(), ChoiceKind::ChooseModes);
    s.respond(Response::Pick(vec![0, 1]));
    s.target_object(banner).target_object(giant).pay();
    // P1 answers by destroying their own Giant: it is no longer a legal target.
    s.pass();
    assert_eq!(s.who(), P1);
    s.cast("Test Smiting").target_object(giant).pay();
    settle(&mut s);
    assert!(!on_battlefield(&s, banner), "the other mode still happens");
    assert!(s.find(P1, Zone::Graveyard, "Test Giant").is_some());
    assert_eq!(
        life(&s, P1),
        40,
        "nobody gains life for a target that was illegal"
    );
}

// CR 111.2, 701.6a (2026-11-06): the player who creates a token owns it and it enters under
// their control. Forge: `Token | TokenOwner$ TargetedController` after destroying a
// permanent (Beast Within) or countering a spell (Swan Song: its controller is the
// spell's).
#[test]
fn its_controller_creates_the_token() {
    let cards = [
        spell(
            "Test Within",
            "G",
            "Instant",
            "A:SP$ Destroy | ValidTgts$ Permanent | AITgts$ Card.cmcGE4 | SubAbility$ DBToken | SpellDescription$ Text.\n\
             SVar:DBToken:DB$ Token | TokenScript$ g_3_3_test_beast | TokenOwner$ TargetedController",
        ),
        spell(
            "Test Song",
            "U",
            "Instant",
            "A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | SubAbility$ DBToken | SpellDescription$ Text.\n\
             SVar:DBToken:DB$ Token | TokenAmount$ 2 | TokenScript$ g_3_3_test_beast | TokenOwner$ TargetedController",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    // A permanent: its controller gets the token, not the caster or the other opponents.
    let mut sc = scenario(4, &cards);
    sc.card(P0, Zone::Battlefield, "Forest");
    let golem = sc.card(P2, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Within");
    let mut s = Script::new(sc.build());
    s.cast("Test Within").target_object(golem).pay();
    settle(&mut s);
    assert!(!on_battlefield(&s, golem));
    assert_eq!(permanents(&s, "Test Beast Token"), [(P2, P2)]);

    // A spell: the player who cast it.
    let mut sc = scenario(4, &cards);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest", "Forest"]);
    sc.card(P3, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Hand, "Test Golem");
    sc.card(P3, Zone::Hand, "Test Song");
    let mut s = Script::new(sc.build());
    s.cast("Test Golem").pay().pass();
    // P1 and P2 pass; P3 counters it.
    s.pass().pass();
    assert_eq!(s.who(), P3);
    s.cast("Test Song").target("Test Golem").pay();
    settle(&mut s);
    assert!(s.find(P0, Zone::Graveyard, "Test Golem").is_some());
    assert_eq!(permanents(&s, "Test Beast Token"), [(P0, P0), (P0, P0)]);
}

// CR 608.2d, 701.23a–b (2026-11-06): "its controller may search their library" — that
// player decides, and only that player sees their library. Forge: `ChangeZone | Origin$
// Library | DefinedPlayer$ TargetedController | Optional$ True` (Path to Exile).
#[test]
fn its_controller_may_search_their_library() {
    let cards = [spell(
        "Test Pathway",
        "W",
        "Instant",
        "A:SP$ ChangeZone | Origin$ Battlefield | Destination$ Exile | ValidTgts$ Creature | SubAbility$ DBChange | SpellDescription$ Text.\n\
         SVar:DBChange:DB$ ChangeZone | Optional$ True | Origin$ Library | Destination$ Battlefield | Tapped$ True | ChangeType$ Land.Basic | DefinedPlayer$ TargetedController | ShuffleNonMandatory$ True",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = || {
        let mut sc = scenario(3, &cards);
        sc.card(P0, Zone::Battlefield, "Plains");
        sc.card(P1, Zone::Battlefield, "Test Giant");
        sc.card(P0, Zone::Hand, "Test Pathway");
        let mut s = Script::new(sc.build());
        s.cast("Test Pathway").target("Test Giant").pay();
        s.pass_all();
        s
    };
    let library_shown = |s: &Script, viewer: PlayerId| {
        player_view(&s.st, viewer)
            .revealed
            .iter()
            .filter(|o| s.st.object(o.id).zone == Zone::Library)
            .count()
    };
    // P1, not the caster, is asked — before looking at anything.
    let mut s = build();
    assert_eq!((s.who(), s.kind()), (P1, ChoiceKind::MayAbility));
    assert_eq!(library_shown(&s, P1), 0);
    accept(&mut s, P1, ChoiceKind::MayAbility);
    assert_eq!((s.who(), s.kind()), (P1, ChoiceKind::SearchLibrary));
    // The searcher sees their library; the caster sees none of it.
    assert_eq!(library_shown(&s, P1), 20);
    assert_eq!(library_shown(&s, P0), 0);
    s.respond(Response::Pick(vec![0]));
    settle(&mut s);
    let lands = permanents(&s, "Forest");
    assert_eq!(lands, [(P1, P1)]);
    let forest =
        s.st.zones
            .battlefield
            .iter()
            .find(|id| s.name(**id) == "Forest")
            .unwrap();
    assert!(s.st.object(*forest).tapped);
    assert_eq!(shuffles(&s, P1), 1);
    // Declined: no search, no shuffle.
    let mut s = build();
    decline(&mut s, P1, ChoiceKind::MayAbility);
    settle(&mut s);
    assert!(permanents(&s, "Forest").is_empty());
    assert_eq!(shuffles(&s, P1), 0);
}

// CR 101.4, 701.23 (2026-11-06): "each player may search their library": each decides for
// themselves, in APNAP order. (Before D-068 the caster decided for everyone.)
#[test]
fn each_player_may_search() {
    let cards = [spell(
        "Test Frontier",
        "G",
        "Sorcery",
        "A:SP$ ChangeZone | Origin$ Library | Destination$ Battlefield | ChangeType$ Land.Basic | DefinedPlayer$ Player | Optional$ True | SpellDescription$ Text.",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Frontier");
    let mut s = Script::new(sc.build());
    s.cast("Test Frontier").pay().pass_all();
    accept(&mut s, P0, ChoiceKind::MayAbility);
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::SearchLibrary));
    s.respond(Response::Pick(vec![0]));
    decline(&mut s, P1, ChoiceKind::MayAbility);
    accept(&mut s, P2, ChoiceKind::MayAbility);
    s.respond(Response::Pick(vec![0]));
    settle(&mut s);
    // Two lands entered (P0 already had one): P0's and P2's; P1 neither searched nor
    // shuffled.
    assert_eq!(permanents(&s, "Forest"), [(P0, P0), (P0, P0), (P2, P2)]);
    assert_eq!(
        (shuffles(&s, P0), shuffles(&s, P1), shuffles(&s, P2)),
        (1, 0, 1)
    );
}

// CR 118.12a, 603.5 (2026-11-06): "you may draw a card unless that player pays {1}" — the
// player who cast the spell is asked to pay; if they don't, the ability's controller may
// draw. Forge: `SpellCast` + `Draw | UnlessCost$ 1 | UnlessPayer$ TriggeredActivator |
// OptionalDecider$ You` (Rhystic Study).
#[test]
fn you_may_draw_unless_that_player_pays() {
    let cards = [
        permanent(
            "Test Studies",
            "2 U",
            "Enchantment",
            "T:Mode$ SpellCast | ValidCard$ Card | ValidActivatingPlayer$ Opponent | TriggerZones$ Battlefield | Execute$ TrigDraw | TriggerDescription$ Text.\n\
             SVar:TrigDraw:DB$ Draw | Defined$ You | UnlessCost$ 1 | UnlessPayer$ TriggeredActivator | NumCards$ 1 | OptionalDecider$ You",
        ),
        creature("Test Sapling", "G", "1/1", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = |lands: usize| {
        let mut sc = scenario(3, &cards);
        sc.card(P0, Zone::Battlefield, "Test Studies");
        sc.cards(P2, Zone::Battlefield, &vec!["Forest"; lands]);
        sc.card(P2, Zone::Hand, "Test Sapling");
        sc.turn(P2, Step::PrecombatMain);
        let mut s = Script::new(sc.build());
        s.cast("Test Sapling").pay();
        s
    };
    // P2 cast it: P2 — not P1, not the ability's controller — is asked to pay, and pays.
    let mut s = build(2);
    accept(&mut s, P2, ChoiceKind::PayUnlessCost);
    s.pay();
    settle(&mut s);
    assert_eq!(hand(&s, P0), 0);
    // P2 doesn't pay: P0 may draw, and does.
    let mut s = build(2);
    decline(&mut s, P2, ChoiceKind::PayUnlessCost);
    accept(&mut s, P0, ChoiceKind::MayAbility);
    settle(&mut s);
    assert_eq!(hand(&s, P0), 1);
    // P2 can't pay: nobody is asked to; P0 may decline the card.
    let mut s = build(1);
    decline(&mut s, P0, ChoiceKind::MayAbility);
    settle(&mut s);
    assert_eq!(hand(&s, P0), 0);
}

// CR 603.5, 608.2d (2026-11-06): "that creature's controller may draw a card" — that player
// decides, not the controller of the ability. Forge: `OptionalDecider$
// TriggeredCardController` on the trigger (Fecundity); of the ability's own source
// ("when this dies, you may draw") it is the ability's controller (CR 603.3a).
#[test]
fn that_creatures_controller_may_draw() {
    let cards = [
        permanent(
            "Test Fertility",
            "2 G",
            "Enchantment",
            "T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Creature | TriggerZones$ Battlefield | Execute$ TrigDraw | OptionalDecider$ TriggeredCardController | TriggerDescription$ Text.\n\
             SVar:TrigDraw:DB$ Draw | Defined$ TriggeredCardController | NumCards$ 1",
        ),
        creature(
            "Test Mourner",
            "1",
            "1/1",
            "T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self | Execute$ TrigDraw | OptionalDecider$ TriggeredCardController | TriggerDescription$ Text.\n\
             SVar:TrigDraw:DB$ Draw | Defined$ TriggeredCardController | NumCards$ 1",
        ),
        spell(
            "Test Smiting",
            "B",
            "Instant",
            "A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Test Fertility");
    sc.card(P0, Zone::Battlefield, "Swamp");
    let golem = sc.card(P2, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Smiting");
    let mut s = Script::new(sc.build());
    s.cast("Test Smiting").target_object(golem).pay();
    s.pass_all();
    accept(&mut s, P2, ChoiceKind::MayAbility);
    settle(&mut s);
    assert_eq!((hand(&s, P0), hand(&s, P1), hand(&s, P2)), (0, 0, 1));

    // "When this dies, you may draw": the ability's controller decides and draws.
    let mourner = {
        let s = Script::new(scenario(3, &cards).build());
        let def = s.st.catalog.get(s.st.catalog.find("Test Mourner").unwrap());
        def.abilities[0].clone()
    };
    let card_model::AbilityDef::Triggered { body, .. } = mourner else {
        panic!()
    };
    assert!(matches!(
        &body.modes()[0].effect,
        Effect::May(inner) if **inner == Effect::Draw { who: PlayerRef::You, amount: card_model::Amount::Fixed(1) }
    ));
}

// CR 603.3a, 608.2h (2026-11-06): "that player" of a cast trigger is who cast the spell;
// of "at the beginning of each player's upkeep", the player whose upkeep it is. Forge:
// `TriggeredActivator` of `SpellCast`, `TriggeredPlayer` of `Phase`. (Before D-068 an
// opponent's cast trigger that hit "its controller" did nothing at all.)
#[test]
fn that_player_of_cast_and_step_triggers() {
    let cards = [
        permanent(
            "Test Spellsting",
            "2 R",
            "Enchantment",
            "T:Mode$ SpellCast | ValidCard$ Card | TriggerZones$ Battlefield | Execute$ TrigDmg | TriggerDescription$ Text.\n\
             SVar:TrigDmg:DB$ DealDamage | Defined$ TriggeredActivator | NumDmg$ 1",
        ),
        permanent(
            "Test Tablet",
            "2",
            "Artifact",
            "T:Mode$ Phase | Phase$ Upkeep | ValidPlayer$ Player | TriggerZones$ Battlefield | Execute$ TrigLose | TriggerDescription$ Text.\n\
             SVar:TrigLose:DB$ LoseLife | Defined$ TriggeredPlayer | LifeAmount$ 2",
        ),
        creature("Test Sapling", "G", "1/1", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Test Spellsting");
    sc.card(P2, Zone::Battlefield, "Forest");
    sc.card(P2, Zone::Hand, "Test Sapling");
    sc.turn(P2, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Sapling").pay();
    settle(&mut s);
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (40, 40, 39));

    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Test Tablet");
    sc.turn(P0, Step::End);
    let mut s = Script::new(sc.build());
    // P1's upkeep: only P1 loses life.
    for _ in 0..200 {
        if s.st.turn.active == P1 && s.st.turn.step == Step::Draw {
            break;
        }
        s.pass();
    }
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (40, 38, 40));
}

// CR 800.4a–b (2026-11-06): a player who has left the game is no one an effect applies to —
// no token is created for them, they decide and pay nothing.
#[test]
fn a_player_who_left_gets_nothing() {
    let cards = [
        permanent(
            "Test Spellsting",
            "2 R",
            "Enchantment",
            "T:Mode$ SpellCast | ValidCard$ Card | ValidActivatingPlayer$ Opponent | TriggerZones$ Battlefield | Execute$ TrigDmg | TriggerDescription$ Text.\n\
             SVar:TrigDmg:DB$ DealDamage | Defined$ TriggeredActivator | NumDmg$ 1",
        ),
        permanent(
            "Test Gifting",
            "2 G",
            "Enchantment",
            "T:Mode$ SpellCast | ValidCard$ Card | ValidActivatingPlayer$ Opponent | TriggerZones$ Battlefield | Execute$ TrigToken | TriggerDescription$ Text.\n\
             SVar:TrigToken:DB$ Token | TokenScript$ g_3_3_test_beast | TokenOwner$ TriggeredActivator | UnlessCost$ 1 | UnlessPayer$ TriggeredActivator",
        ),
        creature("Test Sapling", "G", "1/1", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.cards(P0, Zone::Battlefield, &["Test Spellsting", "Test Gifting"]);
    sc.cards(P2, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P2, Zone::Hand, "Test Sapling");
    sc.life(P2, 1);
    sc.turn(P2, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Sapling").pay();
    // P0 orders the two triggers so that the damage resolves first (listed last).
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::OrderTriggers));
    let sting = s
        .options()
        .iter()
        .position(|o| matches!(o, OptionDesc::Trigger { source, .. } if s.name(*source) == "Test Spellsting"))
        .unwrap();
    let mut order: Vec<usize> = (0..s.options().len()).filter(|i| *i != sting).collect();
    order.push(sting);
    s.respond(Response::Order(order));
    // Nobody is asked anything: P2 is gone before the second trigger resolves.
    for _ in 0..20 {
        if s.st.stack.is_empty() {
            break;
        }
        assert_eq!(
            s.kind(),
            ChoiceKind::Priority,
            "asked {:?} of {:?}",
            s.kind(),
            s.who()
        );
        s.pass();
    }
    assert!(!s.st.in_game(P2));
    assert!(permanents(&s, "Test Beast Token").is_empty());
}

// CR 202.3e, 608.2h (2026-11-06): "its mana value" — of a permanent as it last existed, of
// a spell on the stack with X as announced. Forge: `Targeted$CardManaCost`.
#[test]
fn its_mana_value() {
    let cards = [
        spell(
            "Test Swarming",
            "B",
            "Sorcery",
            "A:SP$ Destroy | ValidTgts$ Creature.OppCtrl | SubAbility$ DBLoseLife | SpellDescription$ Text.\n\
             SVar:DBLoseLife:DB$ LoseLife | Defined$ You | LifeAmount$ X\n\
             SVar:X:Targeted$CardManaCost",
        ),
        spell(
            "Test Lesson",
            "U",
            "Instant",
            "A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | SubAbility$ DBGain | SpellDescription$ Text.\n\
             SVar:DBGain:DB$ GainLife | Defined$ TargetedController | LifeAmount$ X\n\
             SVar:X:Targeted$CardManaCost",
        ),
        spell(
            "Test Blast",
            "X R",
            "Sorcery",
            "A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ X | SpellDescription$ Text.\nSVar:X:Count$xPaid",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Swamp");
    let giant = sc.card(P1, Zone::Battlefield, "Test Giant"); // {3}{R}{R}
    sc.card(P0, Zone::Hand, "Test Swarming");
    let mut s = Script::new(sc.build());
    s.cast("Test Swarming").target_object(giant).pay();
    settle(&mut s);
    assert_eq!(life(&s, P0), 35);

    // X = 3: the spell's mana value on the stack is 4; its controller gains 4.
    let mut sc = scenario(3, &cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 4]);
    sc.card(P1, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Hand, "Test Blast");
    sc.card(P1, Zone::Hand, "Test Lesson");
    let mut s = Script::new(sc.build());
    s.cast("Test Blast").x(3).target_player(P2).pay().pass();
    assert_eq!(s.who(), P1);
    s.cast("Test Lesson").target("Test Blast").pay();
    settle(&mut s);
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (44, 40, 40));
}

// ---------------------------------------------------------------------------------------
// What the compiler accepts, and what it doesn't.

// Forge's `AITgts$` only says which targets its AI prefers (docs/forge/script-format.md,
// "AI only"): the card is the same with or without it.
#[test]
fn ai_targeting_hints_change_nothing() {
    let with = spell(
        "Test Within",
        "G",
        "Instant",
        "A:SP$ Destroy | ValidTgts$ Permanent | AITgts$ Card.cmcGE4 | AITgtsStrict$ True | SpellDescription$ Text.",
    );
    let without = with.replace(" | AITgts$ Card.cmcGE4 | AITgtsStrict$ True", "");
    assert_eq!(forge_test_blockers(&with), [] as [&str; 0]);
    let a = forge_test_catalog(&[&with], &[]);
    let b = forge_test_catalog(&[&without], &[]);
    let def = |c: &card_model::CardCatalog| c.get(c.find("Test Within").unwrap()).clone();
    assert_eq!(def(&a), def(&b));
}

// Forms that stay blocked: Forge finds no controller for a targeted player or a card in a
// graveyard; "its power" with several targets is ambiguous; a draw whose "may" is decided
// by someone other than the player who would draw; a trigger whose decider isn't the one
// who pays; "its controller" taken for "you" in an ability that has a second event about
// some other object; the owner of a spell being cast.
#[test]
fn forms_left_blocked() {
    let b = forge_test_blockers(&spell(
        "Test Riddle",
        "U",
        "Sorcery",
        "A:SP$ Draw | ValidTgts$ Player | NumCards$ 1 | SubAbility$ DBLose | SpellDescription$ Text.\n\
         SVar:DBLose:DB$ LoseLife | Defined$ TargetedController | LifeAmount$ 1",
    ));
    assert!(
        b.iter()
            .any(|x| x == "defined:TargetedController of a player or card"),
        "{b:?}"
    );
    let b = forge_test_blockers(&spell(
        "Test Riddle",
        "U",
        "Sorcery",
        "A:SP$ Destroy | ValidTgts$ Creature | SubAbility$ DBDestroy | SpellDescription$ Text.\n\
         SVar:DBDestroy:DB$ Destroy | ValidTgts$ Artifact | SubAbility$ DBGain\n\
         SVar:DBGain:DB$ GainLife | LifeAmount$ X\n\
         SVar:X:Targeted$CardPower",
    ));
    assert!(
        b.iter().any(|x| x == "expr:Targeted$ with no one target"),
        "{b:?}"
    );
    let b = forge_test_blockers(&permanent(
        "Test Riddle",
        "U",
        "Enchantment",
        "T:Mode$ SpellCast | ValidCard$ Card | TriggerZones$ Battlefield | Execute$ TrigDraw | TriggerDescription$ Text.\n\
         SVar:TrigDraw:DB$ Draw | Defined$ You | OptionalDecider$ TriggeredActivator",
    ));
    assert!(
        b.iter()
            .any(|x| x == "param:Draw.OptionalDecider not the drawer"),
        "{b:?}"
    );
    let b = forge_test_blockers(&permanent(
        "Test Riddle",
        "U",
        "Enchantment",
        "T:Mode$ SpellCast | ValidCard$ Card | TriggerZones$ Battlefield | Execute$ TrigDraw | OptionalDecider$ TriggeredActivator | TriggerDescription$ Text.\n\
         SVar:TrigDraw:AB$ Draw | Cost$ 1 | Defined$ You",
    ));
    assert!(
        b.iter().any(|x| x == "trig:OptionalDecider with a cost"),
        "{b:?}"
    );
    let b = forge_test_blockers(&creature(
        "Test Riddle",
        "U",
        "1/1",
        "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigDraw | TriggerDescription$ Text.\n\
         T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Creature.Other | TriggerZones$ Battlefield | Execute$ TrigDraw | Secondary$ True | TriggerDescription$ Text.\n\
         SVar:TrigDraw:DB$ Draw | Defined$ TriggeredCardController",
    ));
    assert!(
        b.iter().any(|x| x == "trig:Secondary about another object"),
        "{b:?}"
    );
    let b = forge_test_blockers(&permanent(
        "Test Riddle",
        "U",
        "Enchantment",
        "T:Mode$ SpellCast | ValidCard$ Card | TriggerZones$ Battlefield | Execute$ TrigLose | TriggerDescription$ Text.\n\
         SVar:TrigLose:DB$ LoseLife | Defined$ TriggeredCardOwner | LifeAmount$ 1",
    ));
    assert!(
        b.iter().any(|x| x == "defined:player TriggeredCardOwner"),
        "{b:?}"
    );
}

// Read back against the card's text: a card that says "that artifact deals damage" must
// not compile to damage dealt by the card itself, which some scripts do (the source's
// colors, lifelink and deathtouch would be the wrong ones).
#[test]
fn damage_dealt_by_another_object_is_not_this_cards() {
    let card = "Name:Test Tinkerer\nManaCost:1 R\nTypes:Creature Goblin\nPT:1/2\n\
        A:AB$ Destroy | Cost$ R T | ValidTgts$ Artifact | SubAbility$ DBDamage | SpellDescription$ Text.\n\
        SVar:DBDamage:DB$ DealDamage | Defined$ Self | NumDmg$ X\n\
        SVar:X:Targeted$CardManaCost\n\
        Oracle:{R}, {T}: Destroy target artifact. That artifact deals damage equal to its mana value to Test Tinkerer.";
    assert_eq!(forge_test_blockers(card), ["oracle:that one deals damage"]);
    // "Deals 1 damage" compiled as life loss (which some scripts write).
    let card = "Name:Test Brawler\nManaCost:1 R\nTypes:Creature Goblin\nPT:1/2\n\
        T:Mode$ Attacks | ValidCard$ Card.Self | Execute$ TrigLose | TriggerDescription$ Text.\n\
        SVar:TrigLose:DB$ LoseLife | Defined$ TriggeredDefendingPlayer | LifeAmount$ 1\n\
        Oracle:Whenever this creature attacks, it deals 1 damage to defending player.";
    assert_eq!(forge_test_blockers(card), ["oracle:says damage"]);
}

// Read back against the card's text: where the card says "its controller" (or "that
// player", "defending player"), the compiled card must name some player other than "you".
#[test]
fn a_card_that_names_another_player_must_not_compile_to_you() {
    let card = "Name:Test Mercy\nManaCost:W\nTypes:Instant\n\
        A:SP$ Destroy | ValidTgts$ Creature | SubAbility$ DBGain | SpellDescription$ Text.\n\
        SVar:DBGain:DB$ GainLife | Defined$ You | LifeAmount$ 3\n\
        Oracle:Destroy target creature. Its controller gains 3 life.";
    assert_eq!(forge_test_blockers(card), ["oracle:another player"]);
    let right = card.replace("Defined$ You", "Defined$ TargetedController");
    assert_eq!(forge_test_blockers(&right), [] as [&str; 0]);
}

// CR 608.2h, 702.15b (2026-11-06): "target creature deals damage to itself equal to its
// power" — the target is the source of the damage: its lifelink gains its controller life.
// Forge: `DealDamage | DamageSource$ Targeted`. A card in a graveyard dealing damage is not
// compiled (the engine knows nothing about a target there).
#[test]
fn a_target_deals_the_damage() {
    let cards = [
        spell(
            "Test Penance",
            "W",
            "Sorcery",
            "A:SP$ DealDamage | ValidTgts$ Creature | DamageSource$ Targeted | NumDmg$ X | SpellDescription$ Text.\n\
             SVar:X:Targeted$CardPower",
        ),
        creature("Test Leech", "B", "3/3", "K:Lifelink"),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Plains");
    let leech = sc.card(P1, Zone::Battlefield, "Test Leech");
    sc.card(P0, Zone::Hand, "Test Penance");
    let mut s = Script::new(sc.build());
    s.cast("Test Penance").target_object(leech).pay();
    settle(&mut s);
    assert!(!on_battlefield(&s, leech));
    assert_eq!((life(&s, P0), life(&s, P1)), (40, 43));
    let b = forge_test_blockers(&creature(
        "Test Exorcist",
        "W",
        "1/1",
        "A:AB$ ChangeZone | Cost$ T | ValidTgts$ Creature | TgtZone$ Graveyard | Origin$ Graveyard | Destination$ Exile | SubAbility$ DBDamage | SpellDescription$ Text.\n\
         SVar:DBDamage:DB$ DealDamage | Defined$ Self | NumDmg$ 1 | DamageSource$ Targeted",
    ));
    assert!(
        b.iter()
            .any(|x| x == "param:DealDamage.DamageSource=Targeted"),
        "{b:?}"
    );
}

// ---------------------------------------------------------------------------------------
// The defending player; players other than you creating tokens; another player's hand.

/// Go to the declaration of attackers and attack `defender` with `attacker` (and with
/// nothing else).
fn attack(s: &mut Script, attacker: ObjectId, defender: PlayerId) {
    for _ in 0..200 {
        if s.kind() == ChoiceKind::DeclareAttackers {
            break;
        }
        s.pass();
    }
    while s.next.is_some() && s.kind() == ChoiceKind::DeclareAttackers {
        let pick = s.options().iter().position(|o| {
            matches!(o, OptionDesc::Attack { attacker: a, defender: d } if *a == attacker && *d == defender)
        });
        s.respond(Response::Pick(pick.into_iter().collect()));
    }
}

// CR 506.2, 508.1m, 509.1h (2026-11-06): "defending player" of an attack trigger, and of a
// creature that attacks and isn't blocked, is the player it attacks — not every opponent.
// Forge: `TriggeredDefendingPlayer` of `Attacks` and `AttackerUnblocked`.
#[test]
fn the_defending_player() {
    let cards = [
        creature(
            "Test Raidmaster",
            "B",
            "2/2",
            "T:Mode$ Attacks | ValidCard$ Card.Self | Execute$ TrigLose | TriggerDescription$ Text.\n\
             SVar:TrigLose:DB$ LoseLife | Defined$ TriggeredDefendingPlayer | LifeAmount$ 3",
        ),
        creature(
            "Test Nightstalker",
            "B",
            "2/2",
            "T:Mode$ AttackerUnblocked | ValidCard$ Card.Self | Execute$ TrigDiscard | TriggerDescription$ Text.\n\
             SVar:TrigDiscard:DB$ Discard | Defined$ TriggeredDefendingPlayer | NumCards$ 1 | Mode$ TgtChoose",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    let marauder = sc.card(P0, Zone::Battlefield, "Test Raidmaster");
    let mut s = Script::new(sc.build());
    attack(&mut s, marauder, P2);
    settle(&mut s);
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (40, 40, 37));

    let mut sc = scenario(3, &cards);
    let stalker = sc.card(P0, Zone::Battlefield, "Test Nightstalker");
    sc.cards(P1, Zone::Hand, &["Forest", "Forest"]);
    sc.cards(P2, Zone::Hand, &["Forest", "Forest"]);
    let mut s = Script::new(sc.build());
    attack(&mut s, stalker, P1);
    for _ in 0..50 {
        if s.kind() == ChoiceKind::Discard {
            break;
        }
        match s.kind() {
            ChoiceKind::DeclareBlockers => {
                s.respond(Response::Pick(vec![]));
            }
            _ => {
                s.pass();
            }
        }
    }
    assert_eq!((s.who(), s.kind()), (P1, ChoiceKind::Discard));
    s.respond(Response::Pick(vec![0]));
    assert_eq!((hand(&s, P1), hand(&s, P2)), (1, 2));
}

// CR 111.2 (2026-11-06): "each player creates", "target opponent creates": each of them
// owns and controls their tokens. Forge: `TokenOwner$ Player`, `TokenOwner$ Targeted`.
#[test]
fn other_players_create_tokens() {
    let cards = [
        spell(
            "Test Resurgence",
            "G",
            "Sorcery",
            "A:SP$ Token | TokenScript$ g_3_3_test_beast | TokenOwner$ Player | SpellDescription$ Text.",
        ),
        spell(
            "Test Hunting",
            "G",
            "Sorcery",
            "A:SP$ Token | ValidTgts$ Opponent | TokenAmount$ 2 | TokenScript$ g_3_3_test_beast | TokenOwner$ Targeted | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.cards(P0, Zone::Hand, &["Test Resurgence", "Test Hunting"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Resurgence").pay();
    settle(&mut s);
    assert_eq!(
        permanents(&s, "Test Beast Token"),
        [(P0, P0), (P1, P1), (P2, P2)]
    );
    s.cast("Test Hunting").target_player(P2).pay();
    settle(&mut s);
    let theirs = permanents(&s, "Test Beast Token")
        .into_iter()
        .filter(|t| *t == (P2, P2))
        .count();
    assert_eq!(theirs, 3);
}

// CR 608.2h (2026-11-06): "the number of cards in that player's hand" is counted as the
// effect is applied. Forge: `TargetedPlayer$CardsInHand`, `TriggeredPlayer$CardsInHand`.
#[test]
fn the_number_of_cards_in_that_players_hand() {
    let cards = [
        spell(
            "Test Gaze",
            "R",
            "Instant",
            "A:SP$ DealDamage | ValidTgts$ Player | NumDmg$ X | SpellDescription$ Text.\n\
             SVar:X:TargetedPlayer$CardsInHand",
        ),
        permanent(
            "Test Vise",
            "3",
            "Artifact",
            "T:Mode$ Phase | Phase$ Upkeep | ValidPlayer$ Opponent | TriggerZones$ Battlefield | Execute$ TrigDmg | TriggerDescription$ Text.\n\
             SVar:TrigDmg:DB$ DealDamage | Defined$ TriggeredPlayer | NumDmg$ X\n\
             SVar:X:TriggeredPlayer$CardsInHand/Minus.1",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Gaze");
    sc.cards(P1, Zone::Hand, &["Forest"; 2]);
    sc.cards(P2, Zone::Hand, &["Forest"; 5]);
    let mut s = Script::new(sc.build());
    s.cast("Test Gaze").target_player(P2).pay();
    settle(&mut s);
    assert_eq!((life(&s, P1), life(&s, P2)), (40, 35));

    // P1's upkeep (before P1's draw): 3 cards in hand, minus 1.
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Test Vise");
    sc.cards(P1, Zone::Hand, &["Forest"; 3]);
    sc.turn(P0, Step::End);
    let mut s = Script::new(sc.build());
    for _ in 0..200 {
        if s.st.turn.active == P1 && s.st.turn.step == Step::Draw {
            break;
        }
        s.pass();
    }
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (40, 38, 40));
}

// The heuristic agent, asked by an opponent's "unless that player pays {1}", pays: the
// cost is offered only when it can be afforded, and paying denies the opponent a card.
#[test]
fn the_ai_pays_an_opponents_tax() {
    use ai::HeuristicAgent;
    use engine_core::Agent;
    let cards = [
        permanent(
            "Test Studies",
            "2 U",
            "Enchantment",
            "T:Mode$ SpellCast | ValidCard$ Card | ValidActivatingPlayer$ Opponent | TriggerZones$ Battlefield | Execute$ TrigDraw | TriggerDescription$ Text.\n\
             SVar:TrigDraw:DB$ Draw | Defined$ You | UnlessCost$ 1 | UnlessPayer$ TriggeredActivator | NumCards$ 1 | OptionalDecider$ You",
        ),
        creature("Test Sapling", "G", "1/1", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(3, &cards);
    sc.card(P0, Zone::Battlefield, "Test Studies");
    sc.cards(P2, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P2, Zone::Hand, "Test Sapling");
    sc.turn(P2, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Sapling").pay();
    pass_until_asked(&mut s);
    assert_eq!((s.who(), s.kind()), (P2, ChoiceKind::PayUnlessCost));
    let mut ai = HeuristicAgent::new(7, s.st.catalog.clone());
    let d = s.decision().clone();
    let r = ai.decide(&player_view(&s.st, P2), &d);
    let Response::Pick(picked) = r else { panic!() };
    assert_eq!(d.options[picked[0]], OptionDesc::Accept);
}
