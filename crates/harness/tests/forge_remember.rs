//! "Those cards" (D-069): Forge's "remembered" cards within one resolution — an
//! instruction remembers the cards it moved (or the tokens it made), later instructions
//! act on them or count them, the chain's last link forgets them. Invented cards in
//! Forge's format, compiled and played. Rules text: docs/rules-notes/608.2c-400.7j-those-cards.md.

use card_model::CounterKind;
use engine_core::layers::chars;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, Event, GameConfig, ObjectId, OptionDesc, PlayerId, Response};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn creature(name: &str, cost: &str, types: &str, pt: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:Creature {types}\nPT:{pt}\n{rest}\nOracle:Text.")
}

fn spell(name: &str, cost: &str, kind: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{kind}\n{rest}\nOracle:Text.")
}

/// The same, with Oracle text of its own (the compiler reads some words back).
fn spell_saying(name: &str, cost: &str, kind: &str, rest: &str, oracle: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{kind}\n{rest}\nOracle:{oracle}")
}

const SPIRIT: (&str, &str) = (
    "w_1_1_test_spirit",
    "Name:Test Spirit Token\nManaCost:no cost\nTypes:Creature Spirit\nPT:1/1\nOracle:",
);

fn build(cards: &[String], setup: impl FnOnce(&mut Scenario)) -> Script {
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = Scenario::new(
        GameConfig::commander(2),
        forge_test_catalog(&cards, &[SPIRIT]),
        31,
    );
    setup(&mut sc);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    Script::new(sc.build())
}

fn on_battlefield(s: &Script, name: &str) -> Vec<ObjectId> {
    s.st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| s.name(id) == name)
        .collect()
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

fn hand(s: &Script, p: PlayerId) -> usize {
    s.st.zones.list(Zone::Hand, p).len()
}

/// Pick these cards (by name, as many as listed) in the pending card choice.
fn choose_cards(s: &mut Script, names: &[&str]) {
    let mut picked = Vec::new();
    for name in names {
        let i = s
            .options()
            .iter()
            .enumerate()
            .position(|(i, o)| {
                matches!(o, OptionDesc::Card { card } if s.name(*card) == *name)
                    && !picked.contains(&i)
            })
            .unwrap_or_else(|| panic!("no {name} among {:?}", s.options()));
        picked.push(i);
    }
    s.respond(Response::Pick(picked));
}

// "Exile target creature you control, then return that card to the battlefield under its
// owner's control." Forge: `RememberTargets$` on the exile, `Defined$ Remembered` on the
// return, `Cleanup | ClearRemembered$ True` last.
const BLINK: &str = "A:SP$ ChangeZone | ValidTgts$ Creature.YouCtrl | Origin$ Battlefield | Destination$ Exile | RememberTargets$ True | SubAbility$ DBReturn | SpellDescription$ Text.\n\
    SVar:DBReturn:DB$ ChangeZone | Defined$ Remembered | Origin$ All | Destination$ Battlefield | SubAbility$ DBCleanup\n\
    SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True";

fn arrival() -> String {
    creature(
        "Test Arrival",
        "1 W",
        "Spirit",
        "2/2",
        "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ Text.\n\
         SVar:TrigGain:DB$ GainLife | LifeAmount$ 1",
    )
}

fn blink(name: &str, script: &str) -> String {
    spell_saying(
        name,
        "W",
        "Instant",
        script,
        "Exile target creature you control, then return that card to the battlefield under its owner's control.",
    )
}

// CR 400.7, 400.7j, 603.6a, 111.8 (2026-11-06): the exiled card returns as a new object —
// untapped, without its counters, its enters ability triggering again; a token that left
// the battlefield doesn't come back.
#[test]
fn flicker_returns_a_new_object_and_tokens_stay_gone() {
    let cards = [
        blink("Test Blink", BLINK),
        spell(
            "Test Conscription",
            "W",
            "Sorcery",
            "A:SP$ Token | TokenScript$ w_1_1_test_spirit | TokenOwner$ You | SpellDescription$ Text.",
        ),
        arrival(),
    ];
    let mut s = build(&cards, |sc| {
        sc.cards(P0, Zone::Battlefield, &["Plains"; 4]);
        sc.card(P0, Zone::Battlefield, "Test Arrival");
        sc.cards(
            P0,
            Zone::Hand,
            &["Test Blink", "Test Blink", "Test Conscription"],
        );
    });
    let old = on_battlefield(&s, "Test Arrival")[0];
    s.st.objects.get_mut(&old).unwrap().tapped = true;
    s.st.objects
        .get_mut(&old)
        .unwrap()
        .counters
        .insert(CounterKind::PlusOne, 2);
    let before = life(&s, P0);
    s.cast("Test Blink").target("Test Arrival").pay().pass_all();
    // Its enters ability is on the stack: resolve it.
    s.pass_all();
    let new = on_battlefield(&s, "Test Arrival");
    assert_eq!(new.len(), 1);
    assert_ne!(new[0], old, "a new object (CR 400.7)");
    let o = s.st.object(new[0]);
    assert!(!o.tapped);
    assert_eq!(o.counters_of(CounterKind::PlusOne), 0);
    assert_eq!(chars(&s.st, new[0]).controller, P0);
    assert_eq!(life(&s, P0), before + 1, "it entered again (CR 603.6a)");
    // A token: exiled, and gone.
    s.cast("Test Conscription").pay().pass_all();
    assert_eq!(on_battlefield(&s, "Test Spirit Token").len(), 1);
    s.cast("Test Blink")
        .target("Test Spirit Token")
        .pay()
        .pass_all();
    assert!(
        on_battlefield(&s, "Test Spirit Token").is_empty(),
        "CR 111.8"
    );
}

// CR 400.7j: "return those cards" moves every card the first instruction exiled, at once;
// "under your control" (Forge: `GainControl$ True`) — a creature card from an opponent's
// graveyard comes under its new controller, its owner unchanged.
#[test]
fn flicker_of_two_and_returning_under_your_control() {
    let cards = [
        spell(
            "Test Double Blink",
            "2 U",
            "Instant",
            "A:SP$ ChangeZone | ValidTgts$ Creature | TargetMin$ 2 | TargetMax$ 2 | Origin$ Battlefield | Destination$ Exile | RememberChanged$ True | SubAbility$ DBReturn | SpellDescription$ Text.\n\
             SVar:DBReturn:DB$ ChangeZone | Defined$ Remembered | Origin$ Exile | Destination$ Battlefield | SubAbility$ DBCleanup\n\
             SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True",
        ),
        spell_saying(
            "Test Revival",
            "B",
            "Sorcery",
            "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Battlefield | GainControl$ True | ValidTgts$ Creature | TgtPrompt$ Text | ChangeNum$ 1 | RememberLKI$ True | SubAbility$ DBLoseLife | SpellDescription$ Text.\n\
             SVar:DBLoseLife:DB$ LoseLife | Defined$ You | LifeAmount$ X | SubAbility$ DBCleanup\n\
             SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
             SVar:X:RememberedLKI$CardManaCost",
            "Put target creature card from a graveyard onto the battlefield under your control. You lose life equal to its mana value.",
        ),
    ];
    let mut s = build(&cards, |sc| {
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Island", "Island", "Island", "Swamp"],
        );
        sc.card(P0, Zone::Battlefield, "Test Golem");
        sc.card(P1, Zone::Battlefield, "Test Giant");
        sc.card(P1, Zone::Graveyard, "Test Golem");
        sc.cards(P0, Zone::Hand, &["Test Double Blink", "Test Revival"]);
    });
    let (golem, giant) = (
        on_battlefield(&s, "Test Golem")[0],
        on_battlefield(&s, "Test Giant")[0],
    );
    s.cast("Test Double Blink")
        .target("Test Golem")
        .target("Test Giant")
        .pay()
        .pass_all();
    let (golems, giants) = (
        on_battlefield(&s, "Test Golem"),
        on_battlefield(&s, "Test Giant"),
    );
    assert_eq!((golems.len(), giants.len()), (1, 1));
    assert!(golems[0] != golem && giants[0] != giant);
    // Each under its owner's control.
    assert_eq!(chars(&s.st, golems[0]).controller, P0);
    assert_eq!(chars(&s.st, giants[0]).controller, P1);
    assert_eq!(
        s.count(|e| matches!(
            e,
            Event::ZoneChange {
                to: Zone::Battlefield,
                ..
            }
        )),
        2
    );
    // Reanimating the opponent's Golem (mana value 3): mine to control, theirs to own.
    let before = life(&s, P0);
    let card = s.find(P1, Zone::Graveyard, "Test Golem").unwrap();
    s.cast("Test Revival").target_object(card).pay().pass_all();
    let theirs =
        s.st.zones
            .battlefield
            .iter()
            .copied()
            .find(|&id| s.name(id) == "Test Golem" && s.st.object(id).owner == P1)
            .expect("on the battlefield");
    assert_eq!(chars(&s.st, theirs).controller, P0);
    assert_eq!(life(&s, P0), before - 3, "its mana value");
}

// CR 701.8b, 614.6, 400.7j: "destroy all creatures; you gain 1 life for each destroyed this
// way; return those cards to the battlefield". An indestructible creature isn't destroyed
// (not counted); a creature a replacement effect exiles instead was destroyed (counted,
// as Forge counts it) but isn't in the graveyard to be returned.
#[test]
fn counting_what_was_destroyed_and_returning_only_what_is_where_it_was_put() {
    let cards = [
        spell(
            "Test Turnover",
            "W",
            "Sorcery",
            "A:SP$ DestroyAll | ValidCards$ Creature | RememberDestroyed$ True | SubAbility$ DBGain | SpellDescription$ Text.\n\
             SVar:DBGain:DB$ GainLife | LifeAmount$ X | SubAbility$ DBReturn\n\
             SVar:DBReturn:DB$ ChangeZone | Defined$ Remembered | Origin$ Graveyard | Destination$ Battlefield | SubAbility$ DBCleanup\n\
             SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
             SVar:X:Remembered$Amount",
        ),
        creature("Test Monolith", "4", "Golem", "2/2", "K:Indestructible"),
    ];
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Plains");
        sc.cards(P0, Zone::Battlefield, &["Test Golem", "Test Monolith"]);
        sc.card(P1, Zone::Battlefield, "Test Giant");
        sc.card(P0, Zone::Hand, "Test Turnover");
    });
    let golem = on_battlefield(&s, "Test Golem")[0];
    let before = life(&s, P0);
    s.cast("Test Turnover").pay().pass_all();
    assert_eq!(
        life(&s, P0),
        before + 2,
        "the Golem and the Giant, not the Monolith"
    );
    let back = on_battlefield(&s, "Test Golem");
    assert_eq!(back.len(), 1);
    assert_ne!(back[0], golem, "returned: a new object");
    assert_eq!(on_battlefield(&s, "Test Giant").len(), 1);

    // With every card that would go to a graveyard exiled instead.
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Plains");
        sc.card(P1, Zone::Battlefield, "Test Exiler");
        sc.cards(P0, Zone::Battlefield, &["Test Golem", "Test Monolith"]);
        sc.card(P0, Zone::Hand, "Test Turnover");
    });
    let before = life(&s, P0);
    s.cast("Test Turnover").pay().pass_all();
    assert_eq!(life(&s, P0), before + 1, "destroyed, though exiled instead");
    assert!(
        on_battlefield(&s, "Test Golem").is_empty(),
        "not found in exile"
    );
    assert!(s.find(P0, Zone::Exile, "Test Golem").is_some());
}

const CULTIVATION: &str = "A:SP$ ChangeZone | Origin$ Library | Destination$ Library | ChangeType$ Land.Basic | ChangeNum$ 2 | RememberChanged$ True | Reveal$ True | Shuffle$ False | SubAbility$ DBBattlefield | SpellDescription$ Text.\n\
    SVar:DBBattlefield:DB$ ChangeZone | Origin$ Library | Destination$ Battlefield | ChangeType$ Land.IsRemembered | ChangeNum$ 1 | Mandatory$ True | NoLooking$ True | Tapped$ True | Shuffle$ False | SubAbility$ DBHand\n\
    SVar:DBHand:DB$ ChangeZone | Origin$ Library | Destination$ Hand | ChangeType$ Land.IsRemembered | Mandatory$ True | NoLooking$ True | SubAbility$ DBCleanup\n\
    SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True";

// CR 608.2c, 701.23b/e (2026-11-06): "search your library for up to two basic land cards,
// reveal those cards, put one onto the battlefield tapped and the other into your hand,
// then shuffle" — one search, the choice among the cards found, one shuffle.
#[test]
fn search_two_put_one_onto_the_battlefield_and_the_other_into_hand() {
    let cards = [spell("Test Cultivation", "G", "Sorcery", CULTIVATION)];
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Forest");
        sc.card(P0, Zone::Hand, "Test Cultivation");
        sc.cards(P0, Zone::Library, &["Island", "Test Golem"]);
    });
    let lands_before = s.st.zones.battlefield.len();
    let hand_before = hand(&s, P0);
    s.cast("Test Cultivation").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::SearchLibrary);
    // (No Golem: only basic lands are offered.)
    assert!(
        s.options()
            .iter()
            .all(|o| matches!(o, OptionDesc::Card { card } if s.name(*card) != "Test Golem"))
    );
    choose_cards(&mut s, &["Forest", "Island"]);
    // Which goes onto the battlefield: one of the two found.
    assert_eq!(s.kind(), ChoiceKind::ChooseCards);
    assert_eq!(s.options().len(), 2);
    choose_cards(&mut s, &["Island"]);
    let island = on_battlefield(&s, "Island");
    assert_eq!(island.len(), 1);
    assert!(s.st.object(island[0]).tapped);
    assert_eq!(s.st.zones.battlefield.len(), lands_before + 1);
    // The Forest went to the hand (the spell left it): one more card there.
    assert_eq!(hand(&s, P0), hand_before - 1 + 1);
    assert_eq!(s.count(|e| matches!(e, Event::SearchedLibrary { .. })), 1);
    assert!(s.count(|e| matches!(e, Event::Revealed { .. })) >= 1);
    assert!(s.count(|e| matches!(e, Event::Shuffled { player } if *player == P0)) >= 1);
    assert_eq!(s.st.zones.list(Zone::Library, P0).len(), 20);
}

// CR 608.2c: with only one basic land found, it is the one put onto the battlefield.
#[test]
fn search_finding_one_card_puts_it_onto_the_battlefield() {
    let cards = [spell("Test Cultivation", "G", "Sorcery", CULTIVATION)];
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Forest");
        sc.card(P0, Zone::Hand, "Test Cultivation");
    });
    let hand_before = hand(&s, P0);
    s.cast("Test Cultivation").pay().pass_all();
    choose_cards(&mut s, &["Forest"]);
    // Nothing more to decide: the one card goes onto the battlefield.
    assert_eq!(s.kind(), ChoiceKind::Priority);
    assert_eq!(on_battlefield(&s, "Forest").len(), 2);
    assert_eq!(hand(&s, P0), hand_before - 1);
}

// CR 608.2h: "where X is that creature's power" — the sacrificed creature as it last
// existed on the battlefield (Forge: `RememberedLKI$CardPower`).
#[test]
fn the_power_of_the_sacrificed_creature_as_it_last_existed() {
    let cards = [creature(
        "Test Devourer",
        "B",
        "Horror",
        "1/1",
        "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigSac | TriggerDescription$ Text.\n\
         SVar:TrigSac:DB$ Sacrifice | Amount$ 1 | SacValid$ Creature.Other | RememberSacrificed$ True | SubAbility$ DBGain\n\
         SVar:DBGain:DB$ GainLife | LifeAmount$ X | SubAbility$ DBCleanup\n\
         SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
         SVar:X:RememberedLKI$CardPower",
    )];
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Swamp");
        sc.card(P0, Zone::Battlefield, "Test Golem");
        sc.card(P0, Zone::Hand, "Test Devourer");
    });
    let golem = on_battlefield(&s, "Test Golem")[0];
    s.st.objects
        .get_mut(&golem)
        .unwrap()
        .counters
        .insert(CounterKind::PlusOne, 2);
    let before = life(&s, P0);
    s.cast("Test Devourer").pay().pass_all().pass_all();
    assert_eq!(s.kind(), ChoiceKind::Sacrifice);
    choose_cards(&mut s, &["Test Golem"]);
    assert!(s.find(P0, Zone::Graveyard, "Test Golem").is_some());
    assert_eq!(life(&s, P0), before + 4, "2 printed + 2 counters");
}

// CR 608.2h: "reveal the top card of your library and put that card into your hand. You
// lose life equal to its mana value" — a card moved to a hidden zone is counted as it was.
#[test]
fn the_mana_value_of_the_card_put_into_hand() {
    let cards = [spell(
        "Test Bargain",
        "B",
        "Sorcery",
        "A:SP$ Dig | DigNum$ 1 | Reveal$ True | ChangeNum$ All | ChangeValid$ Card | DestinationZone$ Hand | RememberChanged$ True | SubAbility$ DBLose | SpellDescription$ Text.\n\
         SVar:DBLose:DB$ LoseLife | LifeAmount$ X | SubAbility$ DBCleanup\n\
         SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
         SVar:X:Remembered$CardManaCost",
    )];
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Swamp");
        sc.card(P0, Zone::Hand, "Test Bargain");
    });
    // The Golem (mana value 3) on top.
    let top = *s.st.zones.list(Zone::Library, P0).last().unwrap();
    let def = s.st.catalog.find("Test Golem").unwrap();
    s.st.objects.get_mut(&top).unwrap().card = def;
    let before = life(&s, P0);
    s.cast("Test Bargain").pay().pass_all();
    assert!(s.find(P0, Zone::Hand, "Test Golem").is_some());
    assert_eq!(life(&s, P0), before - 3);
}

// CR 608.2c, 608.2h: "create X tokens. If X is 2 or more, destroy all other creatures" —
// the tokens just created are "those cards" (Forge: `RememberTokens$`, `!IsRemembered`);
// a condition on one instruction (Forge: `ConditionCheckSVar$`).
#[test]
fn destroy_all_other_creatures_than_the_tokens_just_made() {
    let cards = [spell(
        "Test Coup",
        "X W",
        "Sorcery",
        "A:SP$ Token | TokenAmount$ X | TokenScript$ w_1_1_test_spirit | RememberTokens$ True | TokenOwner$ You | SubAbility$ DBDestroy | SpellDescription$ Text.\n\
         SVar:DBDestroy:DB$ DestroyAll | ValidCards$ Creature.!IsRemembered | ConditionCheckSVar$ X | ConditionSVarCompare$ GE2 | SubAbility$ DBCleanup\n\
         SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
         SVar:X:Count$xPaid",
    )];
    for (x, survivors) in [(1u32, 1usize), (2, 0)] {
        let mut s = build(&cards, |sc| {
            sc.cards(P0, Zone::Battlefield, &["Plains"; 3]);
            sc.card(P0, Zone::Battlefield, "Test Golem");
            sc.card(P0, Zone::Hand, "Test Coup");
        });
        s.cast("Test Coup").x(x).pay().pass_all();
        assert_eq!(
            on_battlefield(&s, "Test Spirit Token").len(),
            x as usize,
            "the tokens stay"
        );
        assert_eq!(on_battlefield(&s, "Test Golem").len(), survivors, "X = {x}");
    }
}

// CR 400.7j: "exile it, then return that card. If it's a Spirit, put a +1/+1 counter on
// it" — the returned permanent is still one of "those cards" (Forge: `Remembered$Valid`).
#[test]
fn a_counter_on_the_returned_card_if_it_is_of_a_kind() {
    let cards = [
        blink(
            "Test Essence",
            "A:SP$ ChangeZone | ValidTgts$ Creature.YouCtrl | Origin$ Battlefield | Destination$ Exile | RememberTargets$ True | SubAbility$ DBReturn | SpellDescription$ Text.\n\
             SVar:DBReturn:DB$ ChangeZone | Defined$ Remembered | Origin$ All | Destination$ Battlefield | SubAbility$ DBCounter\n\
             SVar:DBCounter:DB$ PutCounter | CounterType$ P1P1 | CounterNum$ 1 | Defined$ Remembered | ConditionCheckSVar$ X | SubAbility$ DBCleanup\n\
             SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
             SVar:X:Remembered$Valid Spirit",
        ),
        arrival(),
    ];
    let mut s = build(&cards, |sc| {
        sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
        sc.cards(P0, Zone::Battlefield, &["Test Arrival", "Test Golem"]);
        sc.cards(P0, Zone::Hand, &["Test Essence", "Test Essence"]);
    });
    s.cast("Test Essence")
        .target("Test Arrival")
        .pay()
        .pass_all();
    let spirit = on_battlefield(&s, "Test Arrival")[0];
    assert_eq!(s.st.object(spirit).counters_of(CounterKind::PlusOne), 1);
    s.pass_all(); // its enters trigger
    s.cast("Test Essence").target("Test Golem").pay().pass_all();
    let golem = on_battlefield(&s, "Test Golem")[0];
    assert_eq!(s.st.object(golem).counters_of(CounterKind::PlusOne), 0);
}

// CR 608.2c; Forge §2.6: a condition on one link skips that link alone (`ConditionPresent$`
// on the battlefield).
#[test]
fn an_instruction_with_a_condition_is_skipped_alone() {
    let cards = [spell(
        "Test Prospect",
        "G",
        "Instant",
        "A:SP$ Draw | NumCards$ 1 | ConditionPresent$ Land.YouCtrl | ConditionCompare$ GE3 | ConditionDescription$ Text | SubAbility$ DBGain | SpellDescription$ Text.\n\
         SVar:DBGain:DB$ GainLife | LifeAmount$ 2",
    )];
    for (lands, drawn) in [(2usize, 0usize), (3, 1)] {
        let mut s = build(&cards, |sc| {
            sc.cards(P0, Zone::Battlefield, &vec!["Forest"; lands]);
            sc.card(P0, Zone::Hand, "Test Prospect");
        });
        let (hand_before, before) = (hand(&s, P0), life(&s, P0));
        s.cast("Test Prospect").pay().pass_all();
        assert_eq!(hand(&s, P0), hand_before - 1 + drawn, "{lands} lands");
        assert_eq!(life(&s, P0), before + 2);
    }
}

// CR 608.2c: "each player sacrifices a creature; you gain 1 life for each creature
// sacrificed this way" (Forge: `RememberSacrificed$`); a discard remembered the same way.
#[test]
fn sacrifices_and_discards_this_way() {
    let cards = [
        spell(
            "Test Tithe",
            "B",
            "Sorcery",
            "A:SP$ Sacrifice | Defined$ Player | SacValid$ Creature | RememberSacrificed$ True | SubAbility$ DBGain | SpellDescription$ Text.\n\
             SVar:DBGain:DB$ GainLife | LifeAmount$ X | SubAbility$ DBCleanup\n\
             SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
             SVar:X:Remembered$Amount",
        ),
        spell(
            "Test Rummage",
            "R",
            "Sorcery",
            "A:SP$ Discard | Mode$ Hand | Defined$ You | RememberDiscarded$ True | SubAbility$ DBDraw | SpellDescription$ Text.\n\
             SVar:DBDraw:DB$ Draw | NumCards$ X | SubAbility$ DBCleanup\n\
             SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
             SVar:X:Remembered$Amount",
        ),
    ];
    let mut s = build(&cards, |sc| {
        sc.cards(P0, Zone::Battlefield, &["Swamp", "Mountain"]);
        sc.card(P0, Zone::Battlefield, "Test Golem");
        sc.card(P1, Zone::Battlefield, "Test Giant");
        sc.cards(
            P0,
            Zone::Hand,
            &["Test Tithe", "Test Rummage", "Forest", "Forest"],
        );
    });
    let before = life(&s, P0);
    s.cast("Test Tithe").pay().pass_all();
    choose_cards(&mut s, &["Test Golem"]);
    choose_cards(&mut s, &["Test Giant"]);
    assert_eq!(life(&s, P0), before + 2, "one creature each");
    // Discard the two Forests, draw two.
    s.cast("Test Rummage").pay().pass_all();
    assert_eq!(hand(&s, P0), 2);
    // Both spells, the Golem, the two Forests.
    assert_eq!(s.st.zones.list(Zone::Graveyard, P0).len(), 5);
}

// D-069: Forge's memory is compiled only where it is the chain's own — remembered, used,
// and forgotten by its last link. Anything else stays a named blocker.
#[test]
fn memory_that_outlives_the_chain_is_not_compiled() {
    let exile_then = |rest: &str| {
        spell(
            "Test Keeper",
            "W",
            "Instant",
            &format!(
                "A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Exile | RememberChanged$ True{rest} | SpellDescription$ Text."
            ),
        )
    };
    // Remembered, never forgotten: linked abilities, not "those cards".
    let blockers = forge_test_blockers(&exile_then(""));
    assert!(
        blockers
            .iter()
            .any(|b| b == "remember:not forgotten when the chain ends"),
        "{blockers:?}"
    );
    assert!(
        blockers
            .iter()
            .any(|b| b == "param:ChangeZone.RememberChanged")
    );
    // Forgotten, but not as the last link.
    let card = format!(
        "{}\nSVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True | SubAbility$ DBReturn\n\
         SVar:DBReturn:DB$ ChangeZone | Defined$ Remembered | Origin$ Exile | Destination$ Battlefield",
        exile_then(" | SubAbility$ DBCleanup")
    );
    assert!(
        forge_test_blockers(&card)
            .contains(&"remember:not forgotten when the chain ends".to_string())
    );
    // Read by another ability of the card (a static): blocked by its filter.
    let card = format!(
        "{}\nSVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
         S:Mode$ Continuous | Affected$ Creature.IsRemembered | AddPower$ 1 | Description$ Text.",
        exile_then(" | SubAbility$ DBCleanup")
    )
    .replace("Types:Instant", "Types:Enchantment")
    .replace("A:SP$", "A:AB$")
    .replace("| SpellDescription", "| Cost$ W | SpellDescription");
    assert!(
        forge_test_blockers(&card)
            .iter()
            .any(|b| b.contains("IsRemembered")),
        "{:?}",
        forge_test_blockers(&card)
    );
    // Handed to an effect object: memory that lasts beyond the resolution.
    let card = format!(
        "{}\nSVar:DBEffect:DB$ Effect | RememberObjects$ Remembered | StaticAbilities$ STPlay | SubAbility$ DBCleanup\n\
         SVar:STPlay:Mode$ Continuous | Affected$ Card.IsRemembered | AffectedZone$ Exile | MayPlay$ True | Description$ Text.\n\
         SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True",
        exile_then(" | SubAbility$ DBEffect")
    );
    assert!(
        forge_test_blockers(&card)
            .iter()
            .any(|b| b.starts_with("Effect:")),
        "{:?}",
        forge_test_blockers(&card)
    );
    // Cards put into a hand can't be found again (CR 400.7j).
    let card = spell(
        "Test Keeper",
        "W",
        "Instant",
        "A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Hand | RememberChanged$ True | SubAbility$ DBBack | SpellDescription$ Text.\n\
         SVar:DBBack:DB$ ChangeZone | Defined$ Remembered | Origin$ Hand | Destination$ Battlefield | SubAbility$ DBCleanup\n\
         SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True",
    );
    assert!(
        forge_test_blockers(&card).contains(&"ChangeZone:those cards in a hidden zone".to_string())
    );
    // "Under your control" where the card says "under its owner's control".
    let card = spell_saying(
        "Test Keeper",
        "W",
        "Instant",
        &BLINK.replace(
            "Destination$ Battlefield",
            "Destination$ Battlefield | GainControl$ True",
        ),
        "Exile target creature you control, then return that card to the battlefield under its owner's control.",
    );
    assert_eq!(forge_test_blockers(&card), ["oracle:under your control"]);
    // A target moved into a hand can't be found again (CR 400.7j); Forge's `Defined$
    // Targeted` would follow it.
    let card = spell(
        "Test Keeper",
        "U",
        "Instant",
        "A:SP$ ChangeZone | Origin$ Battlefield | Destination$ Hand | ValidTgts$ Creature | SubAbility$ DBPump | SpellDescription$ Text.\n\
         SVar:DBPump:DB$ Pump | Defined$ Targeted | KW$ Haste",
    );
    assert_eq!(
        forge_test_blockers(&card),
        ["defined:Targeted after it moved"]
    );
}

// CR 400.7j: "return target creature card from your graveyard to the battlefield; put two
// +1/+1 counters on it" — Forge's `Defined$ Targeted` after the move is the card where the
// move put it (the target itself is a card that is gone: CR 400.7).
#[test]
fn the_target_found_where_the_effect_moved_it() {
    let cards = [spell(
        "Test Rousing",
        "B",
        "Sorcery",
        "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Battlefield | ValidTgts$ Creature.YouOwn | SubAbility$ DBCounter | SpellDescription$ Text.\n\
         SVar:DBCounter:DB$ PutCounter | Defined$ Targeted | CounterType$ P1P1 | CounterNum$ 2",
    )];
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Swamp");
        sc.card(P0, Zone::Graveyard, "Test Golem");
        sc.card(P0, Zone::Hand, "Test Rousing");
    });
    s.cast("Test Rousing").target("Test Golem").pay().pass_all();
    let golem = on_battlefield(&s, "Test Golem");
    assert_eq!(golem.len(), 1);
    assert_eq!(s.st.object(golem[0]).counters_of(CounterKind::PlusOne), 2);
}

// CR 700.2: two chosen modes — what the first noted isn't "those cards" of the second.
#[test]
fn each_mode_has_its_own_those_cards() {
    let mode = |name: &str, target: &str| {
        format!(
            "SVar:{name}:DB$ ChangeZone | ValidTgts$ {target} | Origin$ Battlefield | Destination$ Exile | RememberChanged$ True | SubAbility$ {name}Back | SpellDescription$ Text.\n\
             SVar:{name}Back:DB$ ChangeZone | Defined$ Remembered | Origin$ Exile | Destination$ Battlefield | SubAbility$ {name}Clean\n\
             SVar:{name}Clean:DB$ Cleanup | ClearRemembered$ True"
        )
    };
    let charm = format!(
        "A:SP$ Charm | Choices$ DBOne,DBTwo | CharmNum$ 2 | MinCharmNum$ 1\n{}\n{}",
        mode("DBOne", "Creature.YouCtrl"),
        mode("DBTwo", "Creature.OppCtrl")
    );
    let cards = [spell_saying(
        "Test Shuffle Charm",
        "W",
        "Instant",
        &charm,
        "Choose one or both — • Exile target creature you control, then return it to the battlefield under its owner's control. • Exile target creature an opponent controls, then return it to the battlefield under its owner's control.",
    )];
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Plains");
        sc.card(P0, Zone::Battlefield, "Test Golem");
        sc.card(P1, Zone::Battlefield, "Test Giant");
        sc.card(P0, Zone::Hand, "Test Shuffle Charm");
    });
    s.cast("Test Shuffle Charm");
    s.respond(Response::Pick(vec![0, 1]));
    s.target("Test Golem").target("Test Giant").pay().pass_all();
    // Each came back once (a card returned twice would be an extra zone change).
    assert_eq!(
        s.count(|e| matches!(
            e,
            Event::ZoneChange {
                to: Zone::Battlefield,
                ..
            }
        )),
        2
    );
    assert_eq!(on_battlefield(&s, "Test Golem").len(), 1);
    assert_eq!(on_battlefield(&s, "Test Giant").len(), 1);
}

/// Answer `steps` decisions of `st` the same way whatever the copy: pass priority, else
/// the least a choice allows (at least its first option).
fn drive(st: &mut engine_core::GameState, steps: usize) -> Vec<Event> {
    use engine_core::Primitive;
    let mut events = Vec::new();
    for _ in 0..steps {
        let Some(d) = st.pending.clone() else { break };
        let r = match (&d.kind, &d.prim) {
            (ChoiceKind::Priority, _) => Response::Pick(vec![
                d.options
                    .iter()
                    .position(|o| matches!(o, OptionDesc::Pass))
                    .expect("pass"),
            ]),
            (_, Primitive::PickN { min, .. }) => Response::Pick((0..(*min).max(1)).collect()),
            (_, Primitive::Order) => Response::Order((0..d.options.len()).collect()),
            (_, Primitive::PickNumber { min, .. }) => Response::Number(*min),
            (_, p) => panic!("not expected here: {p:?}"),
        };
        events.extend(engine_core::step(st, r).expect("legal").events);
    }
    events
}

// D-069 (CLAUDE.md "Determinism"): a game suspended in the middle of a noted resolution —
// at the choice of which found land goes onto the battlefield — saves, loads and clones,
// and every copy carries on exactly as the original.
#[test]
fn a_noted_resolution_saves_loads_and_clones() {
    let cards = [spell("Test Cultivation", "G", "Sorcery", CULTIVATION)];
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Forest");
        sc.card(P0, Zone::Hand, "Test Cultivation");
        sc.cards(P0, Zone::Library, &["Island"]);
    });
    s.cast("Test Cultivation").pay().pass_all();
    choose_cards(&mut s, &["Forest", "Island"]);
    assert_eq!(s.kind(), ChoiceKind::ChooseCards, "mid-resolution");
    let json = harness::save_json(&s.st);
    let mut loaded = harness::load_json(&json).expect("loads");
    assert_eq!(loaded.state_hash(), s.st.state_hash());
    assert_eq!(harness::save_json(&loaded), json);
    let mut cloned = s.st.clone();
    let mut original = s.st;
    let a = drive(&mut original, 40);
    let b = drive(&mut loaded, 40);
    let c = drive(&mut cloned, 40);
    assert!(!a.is_empty());
    assert_eq!(a, b, "after load");
    assert_eq!(a, c, "after clone");
    assert_eq!(original.state_hash(), loaded.state_hash());
    assert_eq!(original.state_hash(), cloned.state_hash());
    assert_eq!(original.log_hash, loaded.log_hash);
}

const SHOCK: &str = "Name:Test Shockland\nManaCost:no cost\nTypes:Land Island Swamp\n\
    R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ DBTap | ReplacementResult$ Updated | Description$ Text.\n\
    SVar:DBTap:DB$ Tap | ETB$ True | Defined$ Self | UnlessCost$ PayLife<2> | UnlessPayer$ You\n\
    Oracle:Text.";

const GEM: &str = "Name:Test Gem\nManaCost:0\nTypes:Artifact\n\
    A:AB$ Mana | Cost$ T | Produced$ Any | SpellDescription$ Text.\n\
    R:Event$ Moved | Destination$ Battlefield | ValidCard$ Card.Self | ReplaceWith$ PayBeforeETB | Description$ Text.\n\
    SVar:PayBeforeETB:DB$ Discard | DiscardValid$ Land | Mode$ TgtChoose | RememberDiscarded$ True | Optional$ True | AILogic$ Always | SubAbility$ MoveToGraveyard\n\
    SVar:MoveToGraveyard:DB$ ChangeZone | Origin$ All | Destination$ Graveyard | Defined$ ReplacedCard | ConditionCheckSVar$ X | ConditionSVarCompare$ EQ0 | SubAbility$ MoveToBattlefield\n\
    SVar:MoveToBattlefield:DB$ ChangeZone | Origin$ All | Destination$ Battlefield | Defined$ ReplacedCard | ConditionCheckSVar$ X | ConditionSVarCompare$ EQ1 | SubAbility$ DBCleanup\n\
    SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
    SVar:X:Remembered$Amount\n\
    Oracle:Text.";

/// "Search your library for a [kind] card, put it onto the battlefield tapped, then
/// shuffle. Untap it."
fn fetching(kind: &str) -> String {
    spell(
        "Test Fetching",
        "G",
        "Sorcery",
        &format!(
            "A:SP$ ChangeZone | Origin$ Library | Destination$ Battlefield | Tapped$ True | ChangeType$ {kind} | ChangeNum$ 1 | RememberChanged$ True | SubAbility$ DBUntap | SpellDescription$ Text.\n\
             SVar:DBUntap:DB$ Untap | Defined$ Remembered | SubAbility$ DBCleanup\n\
             SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True"
        ),
    )
}

// D-067 with D-069 (CR 614.12a, 400.7j): a noted card that enters through an "as this
// enters" choice (a land that may enter untapped for 2 life) is found where it entered.
#[test]
fn a_noted_card_entering_with_a_choice_is_found() {
    for pay in [true, false] {
        let cards = [fetching("Land"), SHOCK.to_string()];
        let mut s = build(&cards, |sc| {
            sc.card(P0, Zone::Battlefield, "Forest");
            sc.card(P0, Zone::Hand, "Test Fetching");
            sc.card(P0, Zone::Library, "Test Shockland");
        });
        s.cast("Test Fetching").pay().pass_all();
        choose_cards(&mut s, &["Test Shockland"]);
        assert!(
            matches!(s.kind(), ChoiceKind::AsEnters { .. }),
            "{:?}",
            s.kind()
        );
        let answer = if pay {
            OptionDesc::Accept
        } else {
            OptionDesc::Decline
        };
        s.pick("as it enters", |_, o| *o == answer);
        let land = on_battlefield(&s, "Test Shockland");
        assert_eq!(land.len(), 1);
        assert!(
            !s.st.object(land[0]).tapped,
            "found and untapped (paid: {pay})"
        );
        assert_eq!(life(&s, P0), if pay { 38 } else { 40 });
        assert_eq!(s.kind(), ChoiceKind::Priority);
    }
}

// D-067 with D-069 (CR 614.6, 400.7j): a card a replacement puts elsewhere instead
// ("discard a land card instead … if you don't, into its owner's graveyard") isn't found
// on the battlefield; one that enters after the discard is.
#[test]
fn a_noted_card_put_elsewhere_instead_is_not_found() {
    for discard in [true, false] {
        let cards = [fetching("Artifact"), GEM.to_string()];
        let mut s = build(&cards, |sc| {
            sc.card(P0, Zone::Battlefield, "Forest");
            sc.cards(P0, Zone::Hand, &["Test Fetching", "Forest"]);
            sc.card(P0, Zone::Library, "Test Gem");
        });
        s.cast("Test Fetching").pay().pass_all();
        choose_cards(&mut s, &["Test Gem"]);
        assert!(
            matches!(s.kind(), ChoiceKind::AsEnters { .. }),
            "{:?}",
            s.kind()
        );
        if discard {
            s.pick(
                "the Forest",
                |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Forest"),
            );
            let gem = on_battlefield(&s, "Test Gem");
            assert_eq!(gem.len(), 1);
            assert!(!s.st.object(gem[0]).tapped, "found and untapped");
        } else {
            s.pick("no", |_, o| *o == OptionDesc::Decline);
            assert!(on_battlefield(&s, "Test Gem").is_empty());
            assert!(s.find(P0, Zone::Graveyard, "Test Gem").is_some());
        }
        assert_eq!(s.kind(), ChoiceKind::Priority);
    }
}

// CR 608.2c, 608.2h: "each player sacrifices a creature. Each player who can't discards a
// card; each player who sacrificed one draws a card" — the players who controlled those
// cards as they last existed (Forge: `NonRememberedController`, `RememberedController`).
#[test]
fn the_players_who_did_and_did_not_this_way() {
    let cards = [spell(
        "Test Levy",
        "B",
        "Sorcery",
        "A:SP$ Sacrifice | Defined$ Player | SacValid$ Creature | RememberSacrificed$ True | SubAbility$ DBDiscard | SpellDescription$ Text.\n\
         SVar:DBDiscard:DB$ Discard | Mode$ TgtChoose | NumCards$ 1 | Defined$ NonRememberedController | SubAbility$ DBDraw\n\
         SVar:DBDraw:DB$ Draw | NumCards$ 1 | Defined$ RememberedController | SubAbility$ DBCleanup\n\
         SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True",
    )];
    let mut s = build(&cards, |sc| {
        sc.card(P0, Zone::Battlefield, "Swamp");
        sc.card(P0, Zone::Battlefield, "Test Golem");
        sc.card(P0, Zone::Hand, "Test Levy");
        sc.cards(P1, Zone::Hand, &["Forest", "Island"]);
    });
    let p0_hand = hand(&s, P0);
    s.cast("Test Levy").pay().pass_all();
    // P0 sacrifices the Golem (P1 has no creature).
    choose_cards(&mut s, &["Test Golem"]);
    // P1 couldn't: P1 discards one of two.
    assert_eq!(s.who(), P1);
    assert_eq!(s.kind(), ChoiceKind::Discard);
    choose_cards(&mut s, &["Island"]);
    assert_eq!(hand(&s, P1), 1);
    // P0 sacrificed: P0 draws (the spell left the hand, a card came in).
    assert_eq!(hand(&s, P0), p0_hand);
    assert!(s.find(P0, Zone::Graveyard, "Test Golem").is_some());
}
