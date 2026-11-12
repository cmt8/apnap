//! Sagas (D-084): lore counters, chapter abilities, the sacrifice after the final chapter.
//! Invented cards in Forge's format (`K:Chapter`). Rules text: docs/rules-notes/715-sagas.md
//! (CR 715 in this edition; 704.5s, 505.4).

use card_model::{AbilityDef, CounterKind, Trigger};
use engine_core::events::Event;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, ObjectId, OptionDesc, PlayerId, Primitive, Response};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// "I, II — You gain 2 life. III — Draw two cards."
const SAGA: &str = "Name:Test Chronicle\nManaCost:1 W\nTypes:Enchantment Saga\n\
    K:Chapter:3:DBGain,DBGain,DBDraw\n\
    SVar:DBGain:DB$ GainLife | LifeAmount$ 2 | SpellDescription$ You gain 2 life.\n\
    SVar:DBDraw:DB$ Draw | NumCards$ 2 | SpellDescription$ Draw two cards.\n\
    Oracle:(As this Saga enters and after your draw step, add a lore counter. Sacrifice after III.)\\nI, II — You gain 2 life.\\nIII — Draw two cards.";

/// "If one or more counters would be put on a permanent you control, twice that many are
/// put on it instead."
const SEASON: &str = "Name:Test Counter Season\nManaCost:2 G\nTypes:Enchantment\n\
    R:Event$ AddCounter | ActiveZones$ Battlefield | ValidCard$ Permanent.YouCtrl+inZoneBattlefield | EffectOnly$ True | ReplaceWith$ Repl | Description$ Text.\n\
    SVar:Repl:DB$ ReplaceCounter | ChooseCounter$ True | Amount$ X\n\
    SVar:X:ReplaceCount$CounterNum/Twice\nOracle:Text.";

const SPREAD: &str = "Name:Test Spread\nManaCost:U\nTypes:Sorcery\n\
    A:SP$ Proliferate | SpellDescription$ Text.\nOracle:Proliferate.";

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 37);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

fn lore(s: &Script, id: ObjectId) -> u32 {
    s.st.object(id).counters_of(CounterKind::LORE)
}

/// Pass priority, and answer anything else with the least it allows, until `done`.
fn drive_until(s: &mut Script, done: impl Fn(&Script) -> bool) {
    for _ in 0..5_000 {
        if s.next.is_none() || done(s) {
            return;
        }
        if s.kind() == ChoiceKind::Priority {
            s.pass();
            continue;
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

/// Until `player` has priority in their precombat main phase with an empty stack.
fn to_main_of(s: &mut Script, player: PlayerId) {
    drive_until(s, |s| {
        s.st.turn.active == player
            && s.st.turn.step == Step::PrecombatMain
            && s.kind() == ChoiceKind::Priority
            && s.st.stack.is_empty()
            && s.st.pending_triggers.is_empty()
    });
}

// CR 715.2, 715.2c (2026-11-06): each chapter symbol is a triggered ability; "I, II — X" is
// two of them. CR 715.3a: a Saga has the intrinsic "enters with a lore counter" ability.
#[test]
fn cr_715_2_chapter_symbols_compile_to_triggered_abilities() {
    let catalog = forge_test_catalog(&[SAGA], &[]);
    let def = catalog.get(catalog.find("Test Chronicle").unwrap());
    let chapters: Vec<u32> = def
        .abilities
        .iter()
        .filter_map(|a| match a {
            AbilityDef::Triggered {
                when: Trigger::Chapter(n),
                ..
            } => Some(*n),
            _ => None,
        })
        .collect();
    assert_eq!(chapters, vec![1, 2, 3]);
    let text = card_model::describe::rules_text(def).join(" / ");
    assert!(
        text.contains("This Saga enters with a lore counter on it.")
            && text.contains("I — You gain 2 life.")
            && text.contains("II — You gain 2 life.")
            && text.contains("III — Draw two cards."),
        "{text}"
    );
}

// CR 715.3a, 122.6, 715.2b (2026-11-06): the Saga enters with a lore counter — counters put
// on as it enters are counters put on it — so chapter I triggers. CR 505.4, 715.3c: as its
// controller's precombat main phase begins (not an opponent's), a lore counter is put on
// it, without the stack, and chapter II triggers. CR 704.5s, 715.4: after chapter III
// triggers the Saga stays until that ability has left the stack, then is sacrificed.
#[test]
fn cr_715_saga_reads_through_its_chapters_and_is_sacrificed() {
    let mut sc = scenario(&[SAGA]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Chronicle");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Chronicle").pay().pass_all();
    let saga = s.find(P0, Zone::Battlefield, "Test Chronicle").unwrap();
    assert_eq!(lore(&s, saga), 1);
    assert_eq!(s.st.stack.len(), 1, "chapter I is on the stack");
    s.pass_all();
    assert_eq!(s.st.player(P0).life, 42);

    // P1's turn: nothing for P0's Saga.
    to_main_of(&mut s, P1);
    assert_eq!(lore(&s, saga), 1);
    assert_eq!(s.st.player(P0).life, 42);

    // P0's next precombat main phase: the counter is there before anyone has priority, and
    // chapter II is on the stack once P0 first gets it.
    drive_until(&mut s, |s| {
        s.st.turn.active == P0
            && s.st.turn.step == Step::PrecombatMain
            && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(lore(&s, saga), 2);
    assert_eq!(
        s.st.stack.len(),
        1,
        "chapter II, triggered by the turn-based action"
    );
    s.pass_all();
    assert_eq!(s.st.player(P0).life, 44);

    let hand = s.st.zones.hands[P0.index()].len();
    to_main_of(&mut s, P1);
    drive_until(&mut s, |s| {
        s.st.turn.active == P0
            && s.st.turn.step == Step::PrecombatMain
            && s.kind() == ChoiceKind::Priority
    });
    // Three lore counters — its final chapter — but chapter III hasn't left the stack.
    assert_eq!(lore(&s, saga), 3);
    assert!(s.st.zones.battlefield.contains(&saga));
    assert_eq!(s.st.stack.len(), 1);
    s.pass_all();
    // (One card from the draw step since, two from chapter III.)
    assert_eq!(s.st.zones.hands[P0.index()].len(), hand + 3);
    assert!(!s.st.zones.battlefield.contains(&saga), "sacrificed");
    assert_eq!(
        s.count(|e| matches!(e, Event::Sacrificed { player, .. } if *player == P0)),
        1
    );
    assert_eq!(s.st.player(P0).life, 44, "chapter II triggered only once");
}

// CR 715.2b, 614.1a (2026-11-06): a doubler makes the Saga enter with two lore counters —
// chapters I and II both trigger (the count went from 0 to 2). CR 701.34a: proliferate
// puts a lore counter on it (doubled: two), and chapter III triggers, then 704.5s.
#[test]
fn cr_715_2b_chapters_trigger_for_every_number_the_count_passes() {
    let mut sc = scenario(&[SAGA, SEASON, SPREAD]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Plains", "Plains", "Island", "Test Counter Season"],
    );
    sc.cards(P0, Zone::Hand, &["Test Chronicle", "Test Spread"]);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Chronicle").pay().pass_all();
    let saga = s.find(P0, Zone::Battlefield, "Test Chronicle").unwrap();
    assert_eq!(lore(&s, saga), 2);
    // Two chapter abilities triggered at once: P0 orders them (CR 603.3b).
    if s.kind() == ChoiceKind::OrderTriggers {
        s.respond(Response::Order(vec![0, 1]));
    }
    assert_eq!(s.st.stack.len(), 2);
    s.pass_all().pass_all();
    assert_eq!(s.st.player(P0).life, 44);
    let hand = s.st.zones.hands[P0.index()].len();
    s.cast("Test Spread").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::Proliferate);
    s.pick("the Saga", |_, o| *o == OptionDesc::Card { card: saga });
    assert_eq!(lore(&s, saga), 4);
    assert_eq!(s.st.stack.len(), 1, "only chapter III: no chapter IV");
    s.pass_all();
    // (Test Spread left the hand; chapter III drew two.)
    assert_eq!(s.st.zones.hands[P0.index()].len(), hand - 1 + 2);
    assert!(!s.st.zones.battlefield.contains(&saga));
}

// CR 702.155 (2026-11-06): read ahead isn't built; a chapter ability on a card that isn't a
// Saga (no lore counter as it enters) isn't a card the engine plays.
#[test]
fn read_ahead_and_chapters_off_a_saga_are_blocked() {
    let ahead = SAGA.replace("K:Chapter", "K:Read ahead\nK:Chapter");
    assert!(
        forge_test_blockers(&ahead)
            .iter()
            .any(|b| b.contains("Read ahead")),
        "{:?}",
        forge_test_blockers(&ahead)
    );
    let not_saga = SAGA.replace("Types:Enchantment Saga", "Types:Enchantment");
    assert!(
        forge_test_blockers(&not_saga)
            .iter()
            .any(|b| b.starts_with("invalid:") && b.contains("chapter ability")),
        "{:?}",
        forge_test_blockers(&not_saga)
    );
}
