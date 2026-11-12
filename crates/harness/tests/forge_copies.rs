//! Copies (D-074): token copies (Forge's `CopyPermanent`), permanents that enter as a copy
//! (`K:ETBReplacement:Copy` + `DB$ Clone`) or become one (`Clone`), the delayed "sacrifice
//! it at the beginning of the next end step" (`AtEOT$`), and copies of spells and abilities
//! (`CopySpellAbility`, `K:Storm`). Invented cards in Forge's format, compiled the way real
//! ones are, then played. Rules text: docs/rules-notes/707-copies.md.

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

/// Subtypes are known to the compiler from the type lines of the cards it compiles.
const TYPES: &str = "Name:Test Typeholder\n\
    ManaCost:no cost\n\
    Types:Land Island Swamp Mountain\n\
    Oracle:Text.";

fn scenario(cards: &[&str]) -> Scenario {
    let mut all = cards.to_vec();
    all.push(TYPES);
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(&all, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

/// A card in Forge's format; its Oracle text is "Text." unless `rest` has an `Oracle:` line
/// (the compiler's Oracle guards read it).
fn creature(name: &str, cost: &str, types: &str, pt: &str, rest: &str) -> String {
    if rest.contains("Oracle:") {
        format!("Name:{name}\nManaCost:{cost}\nTypes:{types}\nPT:{pt}\n{rest}")
    } else {
        format!("Name:{name}\nManaCost:{cost}\nTypes:{types}\nPT:{pt}\n{rest}\nOracle:Text.")
    }
}

/// What the object is now: its name, power and toughness.
fn looks(s: &Script, id: ObjectId) -> (String, Option<i32>, Option<i32>) {
    let c = chars(&s.st, id);
    (c.name, c.power, c.toughness)
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

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

/// Answer everything with the least it allows (pass, pick nothing) until `done`.
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

/// A 2/2 with "When this enters, you gain 3 life."
fn healer() -> String {
    creature(
        "Test Healer",
        "1 W",
        "Creature Cleric",
        "2/2",
        "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ When this enters, you gain 3 life.\n\
         SVar:TrigGain:DB$ GainLife | LifeAmount$ 3",
    )
}

/// "{T}: Create a token that's a copy of target nonlegendary creature you control, except
/// it has haste. Sacrifice it at the beginning of the next end step."
fn mirror_goblin() -> String {
    creature(
        "Test Mirror Goblin",
        "2 R",
        "Creature Goblin",
        "2/2",
        "A:AB$ CopyPermanent | Cost$ T | ValidTgts$ Creature.nonLegendary+YouCtrl | TgtPrompt$ Select target nonlegendary creature you control | AddKeywords$ Haste | AtEOT$ Sacrifice | SpellDescription$ Create a token that's a copy of target nonlegendary creature you control, except it has haste. Sacrifice it at the beginning of the next end step.\n\
         Oracle:{T}: Create a token that's a copy of target nonlegendary creature you control, except it has haste. Sacrifice it at the beginning of the next end step.",
    )
}

fn clone_card() -> String {
    creature(
        "Test Clone",
        "3 U",
        "Creature Shapeshifter",
        "0/0",
        "K:ETBReplacement:Copy:DBCopy:Optional\n\
         SVar:DBCopy:DB$ Clone | Choices$ Creature.Other | SpellDescription$ You may have CARDNAME enter as a copy of any creature on the battlefield.",
    )
}

fn activate_on(s: &mut Script, source: ObjectId, target: ObjectId) {
    s.pick("activate", |_, o| {
        matches!(o, OptionDesc::ActivateAbility { source: x, .. } if *x == source)
    });
    s.target_object(target);
    s.pay();
}

// ---------------------------------------------------------------------------------------
// Token copies.

// CR 707.2 (2026-11-06): the copy acquires the copiable values — name, types, power and
// toughness, abilities (its enters trigger triggers, CR 707.5) — but not counters or other
// effects. CR 707.9a: "except it has haste" is part of the copy. CR 111.2: the token is
// its creator's. CR 603.7: "sacrifice it at the beginning of the next end step" is a
// delayed triggered ability that triggers once.
#[test]
fn cr_707_2_token_copy_has_the_copiable_values_and_the_exception() {
    let goblin = mirror_goblin();
    let healer = healer();
    let mut sc = scenario(&[&goblin, &healer]);
    let gob = sc.card(P0, Zone::Battlefield, "Test Mirror Goblin");
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    // A +1/+1 counter and a pump on the original are not copied.
    engine_core::actions::execute(
        &mut s.st,
        engine_core::actions::GameAction::PutCounters {
            object: orig,
            kind: card_model::CounterKind::PlusOne,
            n: 1,
        },
    );
    activate_on(&mut s, gob, orig);
    s.pass_all();
    // The token's enters trigger is on the stack: it resolves.
    assert_eq!(s.kind(), ChoiceKind::Priority);
    s.pass_all();
    let healers = named(&s, "Test Healer");
    assert_eq!(healers.len(), 2, "the original and the token");
    let token = *healers.iter().find(|&&id| id != orig).unwrap();
    let o = s.st.object(token);
    assert!(o.is_token);
    assert_eq!(o.controller, P0);
    assert_eq!(looks(&s, token), ("Test Healer".into(), Some(2), Some(2)));
    assert_eq!(looks(&s, orig), ("Test Healer".into(), Some(3), Some(3)));
    let c = chars(&s.st, token);
    assert!(c.has(card_model::Keyword::Haste), "except it has haste");
    assert!(!chars(&s.st, orig).has(card_model::Keyword::Haste));
    assert_eq!(life(&s, P0), 43, "the copied enters trigger");
    assert!(s.count(|e| matches!(e, Event::BecameCopy { object, .. } if *object == token)) == 1);
    // At the beginning of the end step it is sacrificed — once.
    drive_until(&mut s, |s| s.st.turn.step == Step::End && s.kind() == ChoiceKind::Priority);
    s.pass_all();
    assert!(!s.st.objects.contains_key(&token) || s.st.object(token).zone != Zone::Battlefield);
    assert_eq!(named(&s, "Test Healer"), vec![orig]);
    assert!(s.st.delayed.is_empty(), "a delayed trigger triggers only once");
    assert_eq!(
        s.count(|e| matches!(e, Event::Sacrificed { .. })),
        1,
        "sacrificed, not destroyed"
    );
}
