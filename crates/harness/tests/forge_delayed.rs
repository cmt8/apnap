//! Delayed triggered abilities that remember objects (D-081): "sacrifice it / exile it at
//! the beginning of the next end step", "exile target creature; return it at the beginning
//! of the next end step", "return that card to its owner's hand at the beginning of the
//! next end step". Invented cards in Forge's format, compiled the way real ones are, then
//! played. Rules text: docs/rules-notes/603-delayed.md.

use engine_core::layers::chars;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{
    ChoiceKind, Event, GameConfig, ObjectId, OptionDesc, PlayerId, Primitive, Response,
};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

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

/// Every permanent on the battlefield whose current name is `name`.
fn named(s: &Script, name: &str) -> Vec<ObjectId> {
    s.st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| chars(&s.st, id).name == name)
        .collect()
}

/// Answer everything with the least it allows until `done`.
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

/// To the cleanup step of the current turn (every end-step trigger has resolved).
fn to_cleanup(s: &mut Script) {
    let turn = s.st.turn.turn_number;
    drive_until(s, |s| {
        s.st.turn.turn_number != turn || s.st.turn.step == Step::Cleanup
    });
}

/// A 2/2 with "When this enters, you gain 3 life."
const HEALER: &str = "Name:Test Healer\nManaCost:1 W\nTypes:Creature Cleric\nPT:2/2\n\
    T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ When this enters, you gain 3 life.\n\
    SVar:TrigGain:DB$ GainLife | LifeAmount$ 3\nOracle:Text.";

fn twin(at_eot: &str) -> String {
    format!(
        "Name:Test Twin\nManaCost:W\nTypes:Sorcery\n\
         A:SP$ CopyPermanent | ValidTgts$ Creature.YouCtrl | TgtPrompt$ Select target creature you control | AddKeywords$ Haste | AtEOT$ {at_eot} | SpellDescription$ Text.\n\
         Oracle:Create a token that's a copy of target creature you control, except it has haste. {at_eot} it at the beginning of the next end step."
    )
}

// CR 603.7 (2026-11-06): "An effect may create a delayed triggered ability that can do
// something at a later time." CR 603.7b: it triggers only once — the next time its event
// occurs. CR 603.7c: it refers to the token it was created about. Forge: `CopyPermanent |
// AtEOT$ Sacrifice`.
#[test]
fn cr_603_7_sacrifice_the_token_at_the_beginning_of_the_next_end_step() {
    let twin = twin("Sacrifice");
    let mut sc = scenario(&[&twin, HEALER]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Twin");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Twin");
    s.target_object(orig).pay().pass_all();
    s.pass_all();
    let token = *named(&s, "Test Healer")
        .iter()
        .find(|&&id| id != orig)
        .unwrap();
    assert_eq!(s.st.delayed.len(), 1);
    assert_eq!(s.st.delayed[0].objects, vec![token]);
    assert_eq!(
        s.st.delayed[0].controller, P0,
        "CR 603.7d: the spell's controller"
    );
    to_cleanup(&mut s);
    assert_eq!(
        named(&s, "Test Healer"),
        vec![orig],
        "the token, not the card"
    );
    assert_eq!(s.count(|e| matches!(e, Event::Sacrificed { .. })), 1);
    assert!(s.st.delayed.is_empty(), "CR 603.7b: once");
    // Nothing more the next turn.
    let n = s.count(|e| matches!(e, Event::Triggered { .. }));
    drive_until(&mut s, |s| s.st.turn.turn_number >= 4);
    assert_eq!(s.count(|e| matches!(e, Event::Triggered { .. })), n);
}

// CR 603.7c: "if that object is no longer in the zone it's expected to be in at the time
// the delayed triggered ability resolves, the ability won't affect it." The token left;
// "exile it" does nothing to anything else.
#[test]
fn cr_603_7c_a_token_gone_meanwhile_is_not_affected() {
    let twin = twin("Exile");
    let mut sc = scenario(&[&twin, HEALER]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Twin");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Twin");
    s.target_object(orig).pay().pass_all();
    s.pass_all();
    let token = *named(&s, "Test Healer")
        .iter()
        .find(|&&id| id != orig)
        .unwrap();
    engine_core::actions::move_object(
        &mut s.st,
        token,
        Zone::Graveyard,
        engine_core::zones::LibraryPosition::Top,
        None,
    );
    to_cleanup(&mut s);
    assert_eq!(named(&s, "Test Healer"), vec![orig]);
    assert_eq!(
        s.count(|e| matches!(
            e,
            Event::ZoneChange {
                to: Zone::Exile,
                ..
            }
        )),
        0
    );
}

/// "Exile target creature you own. Return it to the battlefield under its owner's control
/// at the beginning of the next end step." (Forge's flicker shape.)
const FLICKER: &str = "Name:Test Flicker\nManaCost:W\nTypes:Instant\n\
    A:SP$ ChangeZone | ValidTgts$ Creature.YouOwn | TgtPrompt$ Select target creature you own | Origin$ Battlefield | Destination$ Exile | RememberChanged$ True | SubAbility$ DelTrig | SpellDescription$ Text.\n\
    SVar:DelTrig:DB$ DelayedTrigger | Mode$ Phase | Phase$ End of Turn | Execute$ TrigReturn | RememberObjects$ RememberedLKI | TriggerDescription$ Return it. | SubAbility$ DBCleanup\n\
    SVar:TrigReturn:DB$ ChangeZone | Origin$ Exile | Destination$ Battlefield | Defined$ DelayTriggerRememberedLKI\n\
    SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
    Oracle:Exile target creature you own. Return it to the battlefield under its owner's control at the beginning of the next end step.";

// CR 603.7c, 400.7 (2026-11-06): the creature exiled is a new object in exile; the delayed
// ability remembers that card — the one the effect put there — and returns it: a new
// permanent again, whose enters trigger triggers.
#[test]
fn cr_603_7c_exiled_until_the_next_end_step_comes_back() {
    let mut sc = scenario(&[FLICKER, HEALER]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Flicker");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Flicker");
    s.target_object(orig).pay().pass_all();
    let exiled = s.find(P0, Zone::Exile, "Test Healer").expect("exiled");
    assert_eq!(s.st.delayed[0].objects, vec![exiled], "the card in exile");
    // Save and load with it pending: it lives in the game state.
    let saved = harness::save_json(&s.st);
    let mut loaded = harness::load_json(&saved).unwrap();
    assert_eq!(loaded.delayed, s.st.delayed);
    to_cleanup(&mut s);
    let back = s
        .find(P0, Zone::Battlefield, "Test Healer")
        .expect("returned");
    assert_ne!(back, orig);
    assert_eq!(s.st.player(P0).life, 43, "it entered again");
    // The loaded game does the same.
    let answers: Vec<Response> = Vec::new();
    harness::feed(&mut loaded, &answers).unwrap();
    assert_eq!(
        loaded.state_hash(),
        harness::load_json(&saved).unwrap().state_hash()
    );
}

// CR 603.7c: the exiled card left exile (and is a new object in the graveyard) before the
// end step: the delayed ability doesn't return it.
#[test]
fn cr_603_7c_a_card_that_left_exile_stays_where_it_went() {
    let mut sc = scenario(&[FLICKER, HEALER]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Flicker");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Flicker");
    s.target_object(orig).pay().pass_all();
    let exiled = s.find(P0, Zone::Exile, "Test Healer").unwrap();
    engine_core::actions::move_object(
        &mut s.st,
        exiled,
        Zone::Graveyard,
        engine_core::zones::LibraryPosition::Top,
        None,
    );
    to_cleanup(&mut s);
    assert!(s.find(P0, Zone::Battlefield, "Test Healer").is_none());
    assert!(s.find(P0, Zone::Graveyard, "Test Healer").is_some());
}

// CR 603.7c, 400.7: "whenever another nontoken creature you control dies, return that card
// to its owner's hand at the beginning of the next end step" — the card in the graveyard.
#[test]
fn cr_603_7c_return_that_card_at_the_next_end_step() {
    let angel = "Name:Test Angel\nManaCost:3 W\nTypes:Creature Angel\nPT:4/4\n\
        T:Mode$ ChangesZone | ValidCard$ Creature.Other+nonToken+YouCtrl | Origin$ Battlefield | Destination$ Graveyard | Execute$ DelTrigReturn | TriggerZones$ Battlefield | TriggerDescription$ Text.\n\
        SVar:DelTrigReturn:DB$ DelayedTrigger | Mode$ Phase | Phase$ End Of Turn | Execute$ DBReturn | RememberObjects$ TriggeredNewCardLKICopy | TriggerDescription$ Return that card to its owner's hand at the beginning of the next end step.\n\
        SVar:DBReturn:DB$ ChangeZone | Defined$ DelayTriggerRememberedLKI | Origin$ Graveyard | Destination$ Hand\n\
        Oracle:Whenever another nontoken creature you control dies, return that card to its owner's hand at the beginning of the next end step.";
    let mut sc = scenario(&[angel, HEALER]);
    sc.card(P0, Zone::Battlefield, "Test Angel");
    let h = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    // It dies (a rule moves it): the trigger goes on the stack and resolves.
    engine_core::actions::execute(
        &mut s.st,
        engine_core::actions::GameAction::Damage {
            source: h,
            target: engine_core::events::DamageTarget::Object(h),
            amount: 5,
            combat: false,
            deathtouch: false,
        },
    );
    s.pass_all();
    s.pass_all();
    let card = s.find(P0, Zone::Graveyard, "Test Healer").expect("died");
    assert_eq!(s.st.delayed.len(), 1);
    assert_eq!(s.st.delayed[0].objects, vec![card]);
    to_cleanup(&mut s);
    assert!(s.find(P0, Zone::Hand, "Test Healer").is_some());
}

// CR 800.4a (2026-11-06): a player who leaves the game — the delayed ability they control
// never triggers; what it would have returned stays where it is.
#[test]
fn cr_800_4a_a_delayed_ability_of_a_player_who_left_never_triggers() {
    let mut sc = scenario_n(3, &[FLICKER, HEALER]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Flicker");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Flicker");
    s.target_object(orig).pay().pass_all();
    assert_eq!(s.st.delayed.len(), 1);
    // P0 leaves: its cards leave the game too (CR 800.4a); nothing triggers.
    engine_core::actions::execute(
        &mut s.st,
        engine_core::actions::GameAction::LoseGame {
            player: P0,
            reason: engine_core::events::LossReason::ZeroLife,
        },
    );
    engine_core::actions::execute(
        &mut s.st,
        engine_core::actions::GameAction::LeaveGame { player: P0 },
    );
    let before = s.count(|e| matches!(e, Event::Triggered { .. }));
    assert!(!s.st.in_game(P0));
    // Pass with the players left until the end of the turn.
    drive_until(&mut s, |s| s.st.turn.turn_number >= 3);
    assert_eq!(s.count(|e| matches!(e, Event::Triggered { .. })), before);
    let _ = P1;
}

#[test]
fn delayed_forms_not_compiled_are_named() {
    // A delayed ability about the target (an object the effect didn't move).
    let target = "Name:Test Later\nManaCost:W\nTypes:Instant\n\
        A:SP$ Pump | ValidTgts$ Creature | NumAtt$ +1 | SubAbility$ DelTrig | SpellDescription$ Text.\n\
        SVar:DelTrig:DB$ DelayedTrigger | Mode$ Phase | Phase$ End of Turn | Execute$ TrigDestroy | RememberObjects$ Targeted | TriggerDescription$ Text.\n\
        SVar:TrigDestroy:DB$ Destroy | Defined$ DelayTriggerRemembered\nOracle:Text.";
    assert!(forge_test_blockers(target).contains(&"DelayedTrigger:remembers Targeted".to_string()));
    // "… at end of combat" (another window) for a token copy.
    let combat = "Name:Test Combat Twin\nManaCost:R\nTypes:Sorcery\n\
        A:SP$ CopyPermanent | ValidTgts$ Creature | AtEOT$ ExileCombat | SpellDescription$ Text.\nOracle:Text.";
    assert!(forge_test_blockers(combat).contains(&"CopyPermanent:AtEOT ExileCombat".to_string()));
    let _ = (ChoiceKind::Priority, OptionDesc::Pass);
}
