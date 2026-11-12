//! Copies (D-074): token copies (Forge's `CopyPermanent`), permanents that enter as a copy
//! (`K:ETBReplacement:Copy` + `DB$ Clone`) or become one (`Clone`). Invented cards in
//! Forge's format, compiled the way real ones are, then played. Rules text:
//! docs/rules-notes/707-copies.md.

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
/// it has haste."
fn mirror_goblin() -> String {
    creature(
        "Test Mirror Goblin",
        "2 R",
        "Creature Goblin",
        "2/2",
        "A:AB$ CopyPermanent | Cost$ T | ValidTgts$ Creature.nonLegendary+YouCtrl | TgtPrompt$ Select target nonlegendary creature you control | AddKeywords$ Haste | SpellDescription$ Create a token that's a copy of target nonlegendary creature you control, except it has haste.\n\
         Oracle:{T}: Create a token that's a copy of target nonlegendary creature you control, except it has haste.",
    )
}

fn activate_on(s: &mut Script, source: ObjectId, target: ObjectId) {
    s.pick(
        "activate",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source: x, .. } if *x == source),
    );
    s.target_object(target);
    s.pay();
}

// ---------------------------------------------------------------------------------------
// Token copies.

// CR 707.2 (2026-11-06): the copy acquires the copiable values — name, types, power and
// toughness, abilities (its enters trigger triggers, CR 707.5) — but not counters or other
// effects. CR 707.9a: "except it has haste" is part of the copy. CR 111.2: the token is
// its creator's.
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
}

// ---------------------------------------------------------------------------------------
// Entering as a copy.

/// "You may have this creature enter as a copy of any creature on the battlefield." (A
/// cheap one: {W}.)
fn test_clone() -> String {
    creature(
        "Test Clone",
        "W",
        "Creature Shapeshifter",
        "0/0",
        "K:ETBReplacement:Copy:DBCopy:Optional\n\
         SVar:DBCopy:DB$ Clone | Choices$ Creature.Other | SpellDescription$ You may have CARDNAME enter as a copy of any creature on the battlefield.\n\
         Oracle:You may have Test Clone enter as a copy of any creature on the battlefield.",
    )
}

fn choose_copy(s: &mut Script, of: ObjectId) {
    assert!(
        matches!(s.kind(), ChoiceKind::AsEnters { .. }),
        "asked as it enters: {:?}",
        s.kind()
    );
    s.pick("copy", |_, o| *o == OptionDesc::Card { card: of });
}

// CR 707.5, 614.12a (2026-11-06): the choice is made as it enters, among the creatures on
// the battlefield; it enters as the copy — its copied enters trigger triggers. CR 707.2:
// counters on the original are not copied; the copy is still a card (not a token).
#[test]
fn cr_707_5_clone_enters_as_a_copy_and_its_enters_trigger_triggers() {
    let (clone, healer) = (test_clone(), healer());
    let mut sc = scenario(&[&clone, &healer]);
    sc.card(P0, Zone::Battlefield, "Plains");
    let orig = sc.card(P1, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Clone");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    engine_core::actions::execute(
        &mut s.st,
        engine_core::actions::GameAction::PutCounters {
            object: orig,
            kind: card_model::CounterKind::PlusOne,
            n: 2,
        },
    );
    s.cast("Test Clone").pay().pass_all();
    assert_eq!(s.who(), P0);
    assert_eq!(
        s.options(),
        [OptionDesc::Card { card: orig }, OptionDesc::Decline],
        "any creature, or nothing"
    );
    assert!(
        s.find(P0, Zone::Battlefield, "Test Clone").is_none(),
        "not there yet"
    );
    choose_copy(&mut s, orig);
    let copy = s
        .find(P0, Zone::Battlefield, "Test Clone")
        .expect("the card entered");
    assert!(!s.st.object(copy).is_token);
    assert_eq!(looks(&s, copy), ("Test Healer".into(), Some(2), Some(2)));
    assert!(s.st.object(copy).counters.is_empty());
    s.pass_all();
    assert_eq!(life(&s, P0), 43, "the copy's enters trigger");
    assert_eq!(life(&s, P1), 40);
}

// CR 707.5: choosing nothing, it enters as itself — a 0/0 that dies (CR 704.5f).
#[test]
fn cr_707_5_clone_choosing_nothing_enters_as_itself() {
    let (clone, healer) = (test_clone(), healer());
    let mut sc = scenario(&[&clone, &healer]);
    sc.card(P0, Zone::Battlefield, "Plains");
    sc.card(P1, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Clone");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Clone").pay().pass_all();
    s.pick("nothing", |_, o| *o == OptionDesc::Decline);
    assert!(
        s.find(P0, Zone::Graveyard, "Test Clone").is_some(),
        "a 0/0 dies"
    );
    assert_eq!(s.count(|e| matches!(e, Event::BecameCopy { .. })), 0);
}

// CR 707.5: the copied "enters with" ability applies (Skyshroud Behemoth example).
#[test]
fn cr_707_5_the_copied_enters_with_counters_applies() {
    let clone = test_clone();
    let big = creature(
        "Test Grower",
        "2 G",
        "Creature Beast",
        "1/1",
        "K:etbCounter:P1P1:2",
    );
    let mut sc = scenario(&[&clone, &big]);
    sc.card(P0, Zone::Battlefield, "Plains");
    let orig = sc.card(P1, Zone::Battlefield, "Test Grower");
    sc.card(P0, Zone::Hand, "Test Clone");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Clone").pay().pass_all();
    choose_copy(&mut s, orig);
    let copy = s.find(P0, Zone::Battlefield, "Test Clone").unwrap();
    assert_eq!(looks(&s, copy), ("Test Grower".into(), Some(3), Some(3)));
    assert_eq!(
        s.st.object(copy)
            .counters_of(card_model::CounterKind::PlusOne),
        2
    );
}

/// A land that may enter as a copy of a land (no "tapped").
fn mimic_land() -> String {
    "Name:Test Mimic Land\nManaCost:no cost\nTypes:Land\n\
     K:ETBReplacement:Copy:DBCopy:Optional\n\
     SVar:DBCopy:DB$ Clone | Choices$ Land.Other | SpellDescription$ You may have CARDNAME enter as a copy of any land on the battlefield.\n\
     Oracle:You may have Test Mimic Land enter as a copy of any land on the battlefield."
        .to_string()
}

fn shockland() -> String {
    "Name:Test Shockland\nManaCost:no cost\nTypes:Land Island Swamp\n\
     R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ DBTap | ReplacementResult$ Updated | Description$ Text.\n\
     SVar:DBTap:DB$ Tap | ETB$ True | Defined$ Self | UnlessCost$ PayLife<2> | UnlessPayer$ You\n\
     Oracle:As this land enters, you may pay 2 life. If you don't, it enters tapped."
        .to_string()
}

// CR 707.5, 614.12a: a land entering as a copy of a shock land is asked the copied "as
// this enters, you may pay 2 life" after it chose what to copy.
#[test]
fn cr_614_12a_copied_as_enters_choice_is_asked_after_the_copy_choice() {
    let (mimic, shock) = (mimic_land(), shockland());
    let mut sc = scenario(&[&mimic, &shock]);
    let orig = sc.card(P1, Zone::Battlefield, "Test Shockland");
    sc.card(P0, Zone::Hand, "Test Mimic Land");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.pick(
        "play",
        |s, o| matches!(o, OptionDesc::PlayLand { card } if s.name(*card) == "Test Mimic Land"),
    );
    choose_copy(&mut s, orig);
    assert!(
        matches!(s.kind(), ChoiceKind::AsEnters { .. }),
        "then the copied choice"
    );
    assert_eq!(s.options(), [OptionDesc::Accept, OptionDesc::Decline]);
    s.pick("pay", |_, o| *o == OptionDesc::Accept);
    let land = s.find(P0, Zone::Battlefield, "Test Mimic Land").unwrap();
    assert_eq!(chars(&s.st, land).name, "Test Shockland");
    assert!(!s.st.object(land).tapped);
    assert_eq!(life(&s, P0), 38);
}

// CR 707.3, 707.9a (2026-11-06): a copy of a copy copies the copiable values the first copy
// has — the exception ("except it has haste") included.
#[test]
fn cr_707_3_a_copy_of_a_copy_has_the_exceptions() {
    let (goblin, healer, clone) = (mirror_goblin(), healer(), test_clone());
    let mut sc = scenario(&[&goblin, &healer, &clone]);
    sc.card(P0, Zone::Battlefield, "Plains");
    let gob = sc.card(P0, Zone::Battlefield, "Test Mirror Goblin");
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Clone");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    activate_on(&mut s, gob, orig);
    s.pass_all();
    s.pass_all();
    let token = *named(&s, "Test Healer")
        .iter()
        .find(|&&id| id != orig)
        .unwrap();
    s.cast("Test Clone").pay().pass_all();
    choose_copy(&mut s, token);
    let copy = s.find(P0, Zone::Battlefield, "Test Clone").unwrap();
    assert!(chars(&s.st, copy).has(card_model::Keyword::Haste));
    assert_eq!(looks(&s, copy), ("Test Healer".into(), Some(2), Some(2)));
}

fn warlord() -> String {
    creature(
        "Test Overlord",
        "1 R",
        "Legendary Creature Human Warrior",
        "3/3",
        "",
    )
}

// CR 903.3 (2026-11-06): the commander designation is no copiable value — a copy of a
// commander is no commander. CR 707.2b: the copy stays what it is when the original leaves.
#[test]
fn cr_707_2b_a_copy_of_a_commander_is_no_commander_and_outlives_it() {
    let (clone, lord) = (test_clone(), warlord());
    let mut sc = scenario(&[&clone, &lord]);
    sc.card(P0, Zone::Battlefield, "Plains");
    let cmd = sc.commander(P1, Zone::Battlefield, "Test Overlord");
    sc.card(P0, Zone::Hand, "Test Clone");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Clone").pay().pass_all();
    choose_copy(&mut s, cmd);
    let copy = s.find(P0, Zone::Battlefield, "Test Clone").unwrap();
    assert!(s.st.is_commander(cmd));
    assert!(!s.st.is_commander(copy));
    let view = engine_core::player_view(&s.st, P0);
    let ov = view.battlefield.iter().find(|o| o.id == copy).unwrap();
    assert!(!ov.is_commander);
    assert_eq!(ov.name, "Test Overlord");
    // The original leaves; the copy doesn't change.
    engine_core::actions::move_object(
        &mut s.st,
        cmd,
        Zone::Exile,
        engine_core::zones::LibraryPosition::Top,
        None,
    );
    assert_eq!(looks(&s, copy), ("Test Overlord".into(), Some(3), Some(3)));
}

// CR 704.5j (2026-11-06): a copy of a legendary permanent has its name and supertype — the
// legend rule applies to its controller's two. CR 707.9b: "except it isn't legendary"
// (Forge's `NonLegendary$`) — it doesn't.
#[test]
fn cr_704_5j_legend_rule_and_except_it_isnt_legendary() {
    let (lord, clone) = (warlord(), test_clone());
    let nonlegend = creature(
        "Test Imposter",
        "W",
        "Creature Shapeshifter",
        "0/0",
        "K:ETBReplacement:Copy:DBCopy:Optional\n\
         SVar:DBCopy:DB$ Clone | Choices$ Creature.Other | NonLegendary$ True | SpellDescription$ Text.\n\
         Oracle:You may have Test Imposter enter as a copy of any creature on the battlefield, except it isn't legendary.",
    );
    let mut sc = scenario(&[&lord, &clone, &nonlegend]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    let orig = sc.card(P0, Zone::Battlefield, "Test Overlord");
    sc.cards(P0, Zone::Hand, &["Test Imposter", "Test Clone"]);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Imposter").pay().pass_all();
    choose_copy(&mut s, orig);
    let imp = s.find(P0, Zone::Battlefield, "Test Imposter").unwrap();
    assert_eq!(chars(&s.st, imp).name, "Test Overlord");
    assert!(chars(&s.st, imp).supertypes.is_empty());
    assert_eq!(named(&s, "Test Overlord").len(), 2, "no legend rule");
    s.cast("Test Clone").pay().pass_all();
    choose_copy(&mut s, orig);
    assert_eq!(s.kind(), ChoiceKind::LegendRule, "two legendary Warlords");
}

// ---------------------------------------------------------------------------------------
// Token copies and token replacement effects (D-073).

fn season() -> String {
    "Name:Test Season\nManaCost:2\nTypes:Enchantment\n\
     R:Event$ CreateToken | ActiveZones$ Battlefield | ValidToken$ Card.YouCtrl | EffectOnly$ True | ReplaceWith$ Repl | Description$ Text.\n\
     SVar:Repl:DB$ ReplaceToken | Type$ Amount\n\
     Oracle:Text."
        .to_string()
}

// CR 111.2, 614.1a, 707.2 (2026-11-06): a token copy is created like any token — "twice
// that many" doubles it — and each of them is the copy, with what the instruction says
// about it (its haste).
#[test]
fn cr_614_1a_a_token_copy_is_doubled_and_each_is_a_copy() {
    let (goblin, healer, season) = (mirror_goblin(), healer(), season());
    let mut sc = scenario(&[&goblin, &healer, &season]);
    let gob = sc.card(P0, Zone::Battlefield, "Test Mirror Goblin");
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Battlefield, "Test Season");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    activate_on(&mut s, gob, orig);
    s.pass_all();
    drive_until(&mut s, |s| {
        s.kind() == ChoiceKind::Priority
            && s.st.stack.is_empty()
            && s.st.turn.step == Step::PrecombatMain
    });
    let tokens: Vec<ObjectId> = named(&s, "Test Healer")
        .into_iter()
        .filter(|&id| id != orig)
        .collect();
    assert_eq!(tokens.len(), 2);
    for &t in &tokens {
        assert!(chars(&s.st, t).has(card_model::Keyword::Haste));
    }
    assert_eq!(life(&s, P0), 46, "two enters triggers");
}

// CR 707.5, 614.1c (2026-11-06): a token copy of a land that enters tapped enters tapped:
// the copied "enters" replacement effects apply as it is created.
#[test]
fn cr_707_5_a_token_copy_has_the_copied_enters_replacements() {
    let sketch = "Name:Test Sketching\nManaCost:W\nTypes:Sorcery\n\
        A:SP$ CopyPermanent | ValidTgts$ Land | TgtPrompt$ Select target land | SpellDescription$ Text.\n\
        Oracle:Create a token that's a copy of target land.";
    let tapland = "Name:Test Tapland\nManaCost:no cost\nTypes:Land\n\
        R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ ETBTapped | ReplacementResult$ Updated | Description$ Text.\n\
        SVar:ETBTapped:DB$ Tap | ETB$ True | Defined$ Self\nOracle:This land enters tapped.";
    let mut sc = scenario(&[sketch, tapland]);
    sc.card(P0, Zone::Battlefield, "Plains");
    let orig = sc.card(P1, Zone::Battlefield, "Test Tapland");
    sc.card(P0, Zone::Hand, "Test Sketching");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Sketching");
    s.target_object(orig).pay().pass_all();
    let token = *named(&s, "Test Tapland")
        .iter()
        .find(|&&id| id != orig)
        .unwrap();
    assert!(s.st.object(token).tapped);
    assert_eq!(s.st.object(token).controller, P0);
}

// CR 707.2, 611.2 (2026-11-06). Forge: `Defined$ Equipped | NonLegendary$ True |
// PumpKeywords$ Haste` — "create a token that's a copy of equipped creature, except it
// isn't legendary. That token gains haste." Not legendary is copiable; haste isn't: a copy
// of the token is not legendary and has no haste.
#[test]
fn equipped_creature_copied_nonlegendary_and_the_token_gains_haste() {
    let helm = "Name:Test Helm\nManaCost:1\nTypes:Artifact Equipment\nK:Equip:1\n\
        A:AB$ CopyPermanent | Cost$ 1 | Defined$ Equipped | PumpKeywords$ Haste | NonLegendary$ True | SpellDescription$ Text.\n\
        Oracle:{1}: Create a token that's a copy of equipped creature, except it isn't legendary. That token gains haste.\\nEquip {1}";
    let (lord, clone) = (warlord(), test_clone());
    let mut sc = scenario(&[helm, &lord, &clone]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    let helm_id = sc.card(P0, Zone::Battlefield, "Test Helm");
    let orig = sc.card(P0, Zone::Battlefield, "Test Overlord");
    sc.attach(helm_id, orig);
    sc.card(P0, Zone::Hand, "Test Clone");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.pick(
        "activate",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == helm_id),
    );
    s.pay().pass_all();
    let token = *named(&s, "Test Overlord")
        .iter()
        .find(|&&id| id != orig)
        .unwrap();
    let c = chars(&s.st, token);
    assert!(c.supertypes.is_empty() && c.has(card_model::Keyword::Haste));
    assert_eq!(named(&s, "Test Overlord").len(), 2, "no legend rule");
    s.cast("Test Clone").pay().pass_all();
    choose_copy(&mut s, token);
    let copy = s.find(P0, Zone::Battlefield, "Test Clone").unwrap();
    let c = chars(&s.st, copy);
    assert!(c.supertypes.is_empty(), "the exception is copied");
    assert!(
        !c.has(card_model::Keyword::Haste),
        "the gained haste is not"
    );
}

// Forge: `Defined$ Valid Permanent.token+YouCtrl` — "for each token you control, create a
// token that's a copy of that permanent".
#[test]
fn a_copy_of_each_token_you_control() {
    let harvest = "Name:Test Harvest\nManaCost:W\nTypes:Instant\n\
        A:SP$ CopyPermanent | Defined$ Valid Permanent.token+YouCtrl | SpellDescription$ Text.\n\
        Oracle:For each token you control, create a token that's a copy of that permanent.";
    let (goblin, healer) = (mirror_goblin(), healer());
    let mut sc = scenario(&[harvest, &goblin, &healer]);
    sc.card(P0, Zone::Battlefield, "Plains");
    let gob = sc.card(P0, Zone::Battlefield, "Test Mirror Goblin");
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Harvest");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    activate_on(&mut s, gob, orig);
    s.pass_all();
    s.pass_all();
    assert_eq!(named(&s, "Test Healer").len(), 2);
    s.cast("Test Harvest").pay().pass_all();
    s.pass_all();
    let healers = named(&s, "Test Healer");
    assert_eq!(
        healers.len(),
        3,
        "one more: a copy of the token, not of the card"
    );
    let newest = *healers.last().unwrap();
    assert!(
        chars(&s.st, newest).has(card_model::Keyword::Haste),
        "copied exception"
    );
}

// CR 707.4 (2026-11-06): a permanent that becomes a copy stays the same object: it doesn't
// enter or leave. Forge: `AB$ Clone | ValidTgts$ Land`.
#[test]
fn cr_707_4_a_permanent_becomes_a_copy_of_a_target() {
    let stage = "Name:Test Stage\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Clone | Cost$ T | ValidTgts$ Land | TgtPrompt$ Select target land | SpellDescription$ Text.\n\
        Oracle:{T}: This land becomes a copy of target land.";
    let shock = shockland();
    let mut sc = scenario(&[stage, &shock]);
    let st_id = sc.card(P0, Zone::Battlefield, "Test Stage");
    let orig = sc.card(P1, Zone::Battlefield, "Test Shockland");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.pick(
        "activate",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == st_id),
    );
    s.target_object(orig).pass_all();
    assert_eq!(chars(&s.st, st_id).name, "Test Shockland");
    assert!(s.st.objects.contains_key(&st_id), "the same object");
    assert_eq!(
        s.count(|e| matches!(e, Event::ZoneChange { old, .. } if *old == st_id)),
        0,
        "it didn't enter or leave"
    );
    assert_eq!(chars(&s.st, st_id).land_types.len(), 2);
}

// ---------------------------------------------------------------------------------------
// What isn't compiled.

#[test]
fn copy_forms_not_compiled_are_named() {
    // "… except it enters with an additional +1/+1 counter" (an effect on the side,
    // CR 707.9e).
    let spark = "Name:Test Spark\nManaCost:3 U\nTypes:Creature Illusion\nPT:0/0\n\
        K:ETBReplacement:Copy:DBCopy:Optional\n\
        SVar:DBCopy:DB$ Clone | Choices$ Creature.Other+YouCtrl | SubAbility$ DBPump | SpellDescription$ Text.\n\
        SVar:DBPump:DB$ PutCounter | Defined$ Self | CounterType$ P1P1 | ETB$ True\nOracle:Text.";
    assert!(forge_test_blockers(spark).contains(&"Clone:SubAbility".to_string()));
    // "Sacrifice it at the beginning of the next end step": a delayed trigger about the
    // token (CR 603.7c) — not built.
    let twin = "Name:Test Twin\nManaCost:R\nTypes:Sorcery\n\
        A:SP$ CopyPermanent | ValidTgts$ Creature | AtEOT$ Sacrifice | SpellDescription$ Text.\nOracle:Text.";
    assert!(
        forge_test_blockers(twin)
            .contains(&"CopyPermanent:AtEOT (a delayed trigger about the token)".to_string())
    );
    // Two abilities blocked by the same name: both are explained (the second isn't
    // `unexplained:` because the name was already raised).
    let twice = "Name:Test Twice\nManaCost:R\nTypes:Creature Human\nPT:1/1\n\
        A:AB$ CopyPermanent | Cost$ T | ValidTgts$ Creature | AtEOT$ Sacrifice | SpellDescription$ Text.\n\
        A:AB$ CopyPermanent | Cost$ 1 T | ValidTgts$ Creature | NumCopies$ 2 | AtEOT$ Sacrifice | SpellDescription$ Text.\n\
        Oracle:Text.";
    assert_eq!(
        forge_test_blockers(twice),
        ["CopyPermanent:AtEOT (a delayed trigger about the token)"]
    );
    // "Becomes a copy … until end of turn".
    let mirror = "Name:Test Mirror\nManaCost:2\nTypes:Artifact\n\
        A:AB$ Clone | Cost$ 2 | ValidTgts$ Artifact | Duration$ UntilEndOfTurn | SpellDescription$ Text.\nOracle:Text.";
    assert!(forge_test_blockers(mirror).contains(&"param:Clone.Duration".to_string()));
    // A token copy of what an object was, once it is gone (CR 111.12).
    let works = "Name:Test Works\nManaCost:5\nTypes:Artifact\n\
        T:Mode$ ChangesZone | ValidCard$ Artifact.Other+YouCtrl | Origin$ Any | Destination$ Battlefield | TriggerZones$ Battlefield | Execute$ TrigCopy | TriggerDescription$ Text.\n\
        SVar:TrigCopy:DB$ CopyPermanent | Defined$ TriggeredCardLKICopy\nOracle:Text.";
    assert!(
        forge_test_blockers(works).contains(&"CopyPermanent:copy of what it last was".to_string())
    );
    // A script that drops "except" from the card.
    let dropped = "Name:Test Dropped\nManaCost:W\nTypes:Sorcery\n\
        A:SP$ CopyPermanent | ValidTgts$ Creature | SpellDescription$ Text.\n\
        Oracle:Create a token that's a copy of target creature, except it has flying.";
    assert!(forge_test_blockers(dropped).contains(&"oracle:says except".to_string()));
}

// ---------------------------------------------------------------------------------------
// Copies of a permanent with an "as this enters, choose …" ability (D-076).

const CHOOSE_CT: &str = "K:ETBReplacement:Other:ChooseCT\n\
    SVar:ChooseCT:DB$ ChooseType | Defined$ You | Type$ Creature | SpellDescription$ As CARDNAME enters, choose a creature type.";

fn kin_banner() -> String {
    format!(
        "Name:Test Kin Banner\nManaCost:3\nTypes:Artifact\n{CHOOSE_CT}\n\
         S:Mode$ Continuous | Affected$ Creature.ChosenType+YouCtrl | AddPower$ 1 | AddToughness$ 1 | Description$ Text.\n\
         Oracle:As this artifact enters, choose a creature type.\\nCreatures you control of the chosen type get +1/+1."
    )
}

fn pick_type(s: &mut Script, ty: &str) {
    assert!(
        matches!(s.kind(), ChoiceKind::Choose { .. }),
        "asked to choose: {:?}",
        s.kind()
    );
    let ty = ty.to_string();
    s.pick(
        &format!("choose {ty}"),
        move |_, o| matches!(o, OptionDesc::CreatureType { name } if name.name() == ty),
    );
}

fn chosen_type(s: &Script, id: ObjectId) -> Option<&'static str> {
    s.st.object(id).chosen.iter().find_map(|c| match c {
        card_model::Chosen::CreatureType(t) => Some(t.name()),
        card_model::Chosen::Color(_) => None,
    })
}

// CR 707.6 (2026-11-06): "any choices that have been made for that permanent aren't
// copied. Instead, if an object enters the battlefield as a copy of another permanent, the
// object's controller will get to make any 'as [this] enters the battlefield' choices for
// it." The copy choice comes first (CR 614.12a), then the copied one.
#[test]
fn cr_707_6_a_copy_makes_its_own_as_enters_choice() {
    let banner = kin_banner();
    let mimic = "Name:Test Mimic Artifact\nManaCost:1\nTypes:Artifact\n\
        K:ETBReplacement:Copy:DBCopy:Optional\n\
        SVar:DBCopy:DB$ Clone | Choices$ Artifact.Other | SpellDescription$ Text.\n\
        Oracle:You may have this artifact enter as a copy of any artifact on the battlefield.";
    let sketch = "Name:Test Sketching\nManaCost:W\nTypes:Sorcery\n\
        A:SP$ CopyPermanent | ValidTgts$ Artifact | TgtPrompt$ Select target artifact | SpellDescription$ Text.\n\
        Oracle:Create a token that's a copy of target artifact.";
    let mut sc = scenario(&[&banner, mimic, sketch]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains", "Plains"]);
    sc.card(P1, Zone::Battlefield, "Plains");
    let orig = sc.card(P1, Zone::Battlefield, "Test Kin Banner");
    sc.cards(P0, Zone::Hand, &["Test Mimic Artifact", "Test Sketching"]);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Mimic Artifact").pay().pass_all();
    choose_copy(&mut s, orig);
    assert_eq!(s.who(), P0);
    pick_type(&mut s, "Goblin");
    let copy = s
        .find(P0, Zone::Battlefield, "Test Mimic Artifact")
        .unwrap();
    assert_eq!(chars(&s.st, copy).name, "Test Kin Banner");
    assert_eq!(chosen_type(&s, copy), Some("Goblin"));
    // A token copy is asked too (CR 707.5, 614.12).
    s.cast("Test Sketching");
    s.target_object(orig).pay().pass_all();
    pick_type(&mut s, "Elf");
    let token = *named(&s, "Test Kin Banner")
        .iter()
        .find(|&&id| id != orig && id != copy)
        .unwrap();
    assert!(s.st.object(token).is_token);
    assert_eq!(chosen_type(&s, token), Some("Elf"));
}

// ---------------------------------------------------------------------------------------
// Views, save/load.

// CR 707.2: what a copy is, is public: the view shows the copied card (its picture and
// text) and the card the object is itself.
#[test]
fn the_view_shows_a_copy_as_what_it_copies() {
    let (clone, healer) = (test_clone(), healer());
    let mut sc = scenario(&[&clone, &healer]);
    sc.card(P0, Zone::Battlefield, "Plains");
    let orig = sc.card(P1, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Clone");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Clone").pay().pass_all();
    choose_copy(&mut s, orig);
    let copy = s.find(P0, Zone::Battlefield, "Test Clone").unwrap();
    for viewer in [P0, P1] {
        let v = engine_core::player_view(&s.st, viewer);
        let ov = v.battlefield.iter().find(|o| o.id == copy).unwrap();
        assert_eq!(
            ov.card,
            s.st.object(orig).card,
            "the picture of the original"
        );
        assert_eq!(ov.printed, Some(s.st.object(copy).card), "and what it is");
        let plain = v.battlefield.iter().find(|o| o.id == orig).unwrap();
        assert_eq!(plain.printed, None);
    }
}

// Determinism (CLAUDE.md definition of done): a game with a copy on the battlefield and a
// token copy saved and loaded — or cloned — mid-game goes on exactly as the original.
#[test]
fn save_load_and_clone_with_copies_replay_identically() {
    let (goblin, healer, clone) = (mirror_goblin(), healer(), test_clone());
    let mut sc = scenario(&[&goblin, &healer, &clone]);
    sc.card(P0, Zone::Battlefield, "Plains");
    let gob = sc.card(P0, Zone::Battlefield, "Test Mirror Goblin");
    let orig = sc.card(P0, Zone::Battlefield, "Test Healer");
    sc.card(P0, Zone::Hand, "Test Clone");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    activate_on(&mut s, gob, orig);
    s.pass_all();
    s.pass_all();
    s.cast("Test Clone").pay().pass_all();
    // Suspended on the copy choice, with a token copy on the battlefield.
    assert!(matches!(s.kind(), ChoiceKind::AsEnters { .. }));
    let saved = harness::save_json(&s.st);
    let mut loaded = harness::load_json(&saved).unwrap();
    let mut cloned = s.st.clone();
    assert_eq!(loaded.state_hash(), s.st.state_hash());
    let mut answers = Vec::new();
    for _ in 0..200 {
        let Some(d) = s.next.clone() else { break };
        let r = match &d.prim {
            Primitive::PickN { min, .. } => {
                Response::Pick((0..(*min).max(1).min(d.options.len())).collect())
            }
            Primitive::PickNumber { min, .. } => Response::Number(*min),
            Primitive::Allocate { per_min, .. } => Response::Allocate(per_min.clone()),
            Primitive::Order => Response::Order((0..d.options.len()).collect()),
        };
        answers.push(r.clone());
        s.respond(r);
    }
    for st in [&mut loaded, &mut cloned] {
        harness::feed(st, &answers).unwrap();
        assert_eq!(st.state_hash(), s.st.state_hash());
        assert_eq!(st.log_hash, s.st.log_hash);
    }
}

// ---------------------------------------------------------------------------------------
// Double-faced permanents (CR 707.8, 707.8a) and regeneration (D-079).

const LARVA: &str = "Name:Test Larva\n\
    ManaCost:G\n\
    Types:Creature Insect\n\
    PT:2/2\n\
    A:AB$ SetState | Cost$ W | Defined$ Self | Mode$ Transform | SpellDescription$ Transform CARDNAME.\n\
    AlternateMode:DoubleFaced\n\
    Oracle:{W}: Transform Test Larva.\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Imago\n\
    ManaCost:no cost\n\
    Colors:white\n\
    Types:Creature Insect\n\
    PT:4/4\n\
    K:Flying\n\
    Oracle:Flying";

const SKETCH: &str = "Name:Test Sketching\nManaCost:W\nTypes:Sorcery\n\
    A:SP$ CopyPermanent | ValidTgts$ Creature | TgtPrompt$ Select target creature | SpellDescription$ Text.\n\
    Oracle:Create a token that's a copy of target creature.";

// CR 707.8a (2026-11-06): "If an effect creates a token that is a copy of a double-faced
// permanent …, the resulting token is a double-faced token that has both a front face and
// a back face. … If the token is a copy of a double-faced permanent with its back face up,
// the token enters the battlefield with its back face up." CR 712.7: a double-faced token
// can transform. CR 707.8: a Clone isn't double-faced — it copies the face that is up.
#[test]
fn cr_707_8a_a_token_copy_of_a_double_faced_permanent_has_both_faces() {
    let clone = test_clone();
    let mut sc = scenario(&[LARVA, SKETCH, &clone]);
    sc.cards(P0, Zone::Battlefield, &["Plains"; 4]);
    let orig = sc.card(P0, Zone::Battlefield, "Test Larva");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Sketching", "Test Sketching", "Test Clone"],
    );
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Sketching");
    s.target_object(orig).pay().pass_all();
    let token = *named(&s, "Test Larva")
        .iter()
        .find(|&&id| id != orig)
        .unwrap();
    assert_eq!(looks(&s, token), ("Test Larva".into(), Some(2), Some(2)));
    // It transforms, and then is its back face.
    s.pick(
        "transform the token",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == token),
    );
    s.pay().pass_all();
    assert_eq!(looks(&s, token), ("Test Imago".into(), Some(4), Some(4)));
    assert!(chars(&s.st, token).has(card_model::Keyword::Flying));
    let v = engine_core::player_view(&s.st, P0);
    let ov = v.battlefield.iter().find(|o| o.id == token).unwrap();
    assert_eq!(
        (ov.card, ov.face),
        (s.st.object(orig).card, 1),
        "both faces shown"
    );
    // A copy of it with its back face up enters with its back face up.
    s.cast("Test Sketching");
    s.target_object(token).pay().pass_all();
    let second = *named(&s, "Test Imago")
        .iter()
        .find(|&&id| id != token)
        .unwrap();
    assert_eq!(s.st.object(second).face, 1);
    assert_eq!(looks(&s, second), ("Test Imago".into(), Some(4), Some(4)));
    // A Clone copying the back face is that face only: not double-faced (CR 707.8).
    s.cast("Test Clone").pay().pass_all();
    choose_copy(&mut s, token);
    let c = s.find(P0, Zone::Battlefield, "Test Clone").unwrap();
    assert_eq!(chars(&s.st, c).name, "Test Imago");
    assert_eq!(
        chars(&s.st, c).mana_value(),
        0,
        "a copy of a back face: CR 202.3b"
    );
}

// D-079, CR 701.19a, 707.2 (2026-11-06): a copy has the copied "{W}: Regenerate this
// creature" — it regenerates itself; the shield is the object's (a status of the copy, not
// a copiable value): destroyed, the copy is tapped instead and stays.
#[test]
fn a_copy_regenerates_with_the_copied_ability() {
    let regen = creature(
        "Test Troll",
        "1 W",
        "Creature Troll",
        "2/2",
        "A:AB$ Regenerate | Cost$ W | SpellDescription$ Regenerate CARDNAME.\n\
         Oracle:{W}: Regenerate Test Troll.",
    );
    let smite = "Name:Test Smiting\nManaCost:W\nTypes:Instant\n\
        A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Text.\nOracle:Destroy target creature.";
    let clone = test_clone();
    let mut sc = scenario(&[&regen, smite, &clone]);
    sc.cards(P0, Zone::Battlefield, &["Plains"; 3]);
    let orig = sc.card(P1, Zone::Battlefield, "Test Troll");
    sc.cards(P0, Zone::Hand, &["Test Clone", "Test Smiting"]);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Clone").pay().pass_all();
    choose_copy(&mut s, orig);
    let copy = s.find(P0, Zone::Battlefield, "Test Clone").unwrap();
    s.pick(
        "regenerate the copy",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == copy),
    );
    s.pay().pass_all();
    s.cast("Test Smiting");
    s.target_object(copy).pay().pass_all();
    assert!(
        s.st.objects.contains_key(&copy),
        "regenerated, not destroyed"
    );
    assert!(s.st.object(copy).tapped);
    assert_eq!(
        s.count(|e| matches!(e, Event::Regenerated { object } if *object == copy)),
        1
    );
}

// ---------------------------------------------------------------------------------------
// Copies of Auras and Equipment (CR 303.4f–g, 301.5).

const BLESSING: &str = "Name:Test Blessing\nManaCost:W\nTypes:Enchantment Aura\nK:Enchant:Creature\n\
    S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | AddPower$ 2 | AddToughness$ 2 | Description$ Text.\n\
    Oracle:Enchant creature\\nEnchanted creature gets +2/+2.";

const MIRROR_ENCHANTMENT: &str = "Name:Test Mirror Enchantment\nManaCost:W\nTypes:Enchantment\n\
    K:ETBReplacement:Copy:DBCopy:Optional\n\
    SVar:DBCopy:DB$ Clone | Choices$ Enchantment.Other | SpellDescription$ Text.\n\
    Oracle:You may have this enchantment enter as a copy of any enchantment on the battlefield.";

// CR 303.4f (2026-11-06): an Aura entering the battlefield other than by resolving as an
// Aura spell — here a permanent entering as a copy of one, and a token copy of one — is
// attached to something its controller chooses as it enters (not targeted). CR 303.4g:
// with nothing it could enchant, it doesn't enter: a token isn't created.
#[test]
fn cr_303_4f_a_copy_of_an_aura_enchants_what_its_controller_chooses() {
    let sketch = "Name:Test Sketching\nManaCost:W\nTypes:Sorcery\n\
        A:SP$ CopyPermanent | ValidTgts$ Enchantment | TgtPrompt$ Select target enchantment | SpellDescription$ Text.\n\
        Oracle:Create a token that's a copy of target enchantment.";
    let healer = healer();
    let mut sc = scenario(&[BLESSING, MIRROR_ENCHANTMENT, sketch, &healer]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Healer");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Healer");
    let aura = sc.card(P0, Zone::Battlefield, "Test Blessing");
    sc.attach(aura, mine);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Mirror Enchantment", "Test Sketching"],
    );
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    assert_eq!(looks(&s, mine).1, Some(4));
    s.cast("Test Mirror Enchantment").pay().pass_all();
    choose_copy(&mut s, aura);
    assert_eq!(s.kind(), ChoiceKind::AuraHost);
    assert_eq!(s.who(), P0);
    assert_eq!(
        s.options(),
        [
            OptionDesc::Card { card: mine },
            OptionDesc::Card { card: theirs }
        ]
    );
    s.pick("their Healer", |_, o| {
        *o == OptionDesc::Card { card: theirs }
    });
    let copy = s
        .find(P0, Zone::Battlefield, "Test Mirror Enchantment")
        .expect("it entered and stays");
    assert_eq!(s.st.object(copy).attached_to, Some(theirs));
    assert_eq!(looks(&s, theirs).1, Some(4));
    // A token copy of it, too.
    s.cast("Test Sketching");
    s.target_object(aura).pay();
    s.pass_all();
    s.pick("my Healer", |_, o| *o == OptionDesc::Card { card: mine });
    assert_eq!(looks(&s, mine).1, Some(6), "two Blessings");
    assert_eq!(named(&s, "Test Blessing").len(), 3);
}

// CR 303.4g: "If an Aura is entering the battlefield and there is no legal object or
// player for it to enchant, the Aura remains in its current zone, unless that zone is the
// stack. In that case, the Aura is put into its owner's graveyard instead of entering the
// battlefield. If the Aura is a token, it isn't created."
#[test]
fn cr_303_4g_a_copy_of_an_aura_with_nothing_to_enchant_doesnt_enter() {
    // "Enchant creature you control": P1's, on P1's creature; P0 controls no creature.
    let ward = "Name:Test Ownward\nManaCost:W\nTypes:Enchantment Aura\n\
        K:Enchant:Creature.YouCtrl:creature you control\n\
        Oracle:Enchant creature you control";
    let healer = healer();
    let mut sc = scenario(&[ward, MIRROR_ENCHANTMENT, &healer]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    let h = sc.card(P1, Zone::Battlefield, "Test Healer");
    let aura = sc.card(P1, Zone::Battlefield, "Test Ownward");
    sc.attach(aura, h);
    sc.card(P0, Zone::Hand, "Test Mirror Enchantment");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Mirror Enchantment").pay().pass_all();
    choose_copy(&mut s, aura);
    assert!(
        s.find(P0, Zone::Graveyard, "Test Mirror Enchantment")
            .is_some(),
        "from the stack to the graveyard"
    );
    assert!(
        s.find(P0, Zone::Battlefield, "Test Mirror Enchantment")
            .is_none()
    );
}

// CR 301.5 (2026-11-06): an Equipment copy enters unattached; the original stays on its
// creature.
#[test]
fn cr_301_5_an_equipment_copy_enters_unattached() {
    let axe = "Name:Test Axe\nManaCost:1\nTypes:Artifact Equipment\nK:Equip:1\n\
        S:Mode$ Continuous | AffectedDefined$ Equipped | Affected$ Creature | AddPower$ 2 | Description$ Text.\n\
        Oracle:Equipped creature gets +2/+0.\\nEquip {1}";
    let steel = "Name:Test Steel\nManaCost:1\nTypes:Artifact\n\
        K:ETBReplacement:Copy:DBCopy:Optional\n\
        SVar:DBCopy:DB$ Clone | Choices$ Artifact.Other | SpellDescription$ Text.\n\
        Oracle:You may have this artifact enter as a copy of any artifact on the battlefield.";
    let healer = healer();
    let mut sc = scenario(&[axe, steel, &healer]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    let h = sc.card(P0, Zone::Battlefield, "Test Healer");
    let eq = sc.card(P0, Zone::Battlefield, "Test Axe");
    sc.attach(eq, h);
    sc.card(P0, Zone::Hand, "Test Steel");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Steel").pay().pass_all();
    choose_copy(&mut s, eq);
    let copy = s.find(P0, Zone::Battlefield, "Test Steel").unwrap();
    assert_eq!(chars(&s.st, copy).name, "Test Axe");
    assert_eq!(s.st.object(copy).attached_to, None);
    assert_eq!(s.st.object(eq).attached_to, Some(h));
    assert_eq!(looks(&s, h).1, Some(4));
}
