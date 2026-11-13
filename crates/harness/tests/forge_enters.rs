//! How a permanent enters (D-067): Forge's `R:Event$ Moved | Destination$ Battlefield`
//! replacement effects — "enters tapped unless …", "as this enters, you may pay 2 life /
//! reveal a card; if you don't, it enters tapped", "creatures your opponents control enter
//! tapped". Invented cards in Forge's format, compiled the way real ones are, then played.
//! Rules text: docs/rules-notes/614.12-enters-tapped-unless.md.

use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{Agent, ChoiceKind, Event, GameConfig, ObjectId, OptionDesc, PlayerId, Response};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

/// Subtypes are known to the compiler from the type lines of the cards it compiles.
const TYPES: &str = "Name:Test Typeholder\n\
    ManaCost:no cost\n\
    Types:Land Island Swamp Mountain\n\
    Oracle:Text.";

fn scenario_n(players: u8, cards: &[&str]) -> Scenario {
    let mut all = cards.to_vec();
    all.push(TYPES);
    let mut sc = Scenario::new(
        GameConfig::commander(players),
        forge_test_catalog(&all, &[]),
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

fn play(s: &mut Script, name: &str) {
    let name = name.to_string();
    s.pick(
        &format!("play {name}"),
        move |s, o| matches!(o, OptionDesc::PlayLand { card } if s.name(*card) == name),
    );
}

fn on_battlefield(s: &Script, owner: PlayerId, name: &str) -> ObjectId {
    s.find(owner, Zone::Battlefield, name)
        .unwrap_or_else(|| panic!("{name} is on the battlefield"))
}

fn tapped(s: &Script, owner: PlayerId, name: &str) -> bool {
    s.st.object(on_battlefield(s, owner, name)).tapped
}

fn asks_as_it_enters(s: &Script) -> bool {
    matches!(s.kind(), ChoiceKind::AsEnters { .. })
}

/// A land whose own replacement effect is `ReplaceWith$ DBTap` with these parameters.
fn land(name: &str, types: &str, tap: &str) -> String {
    format!(
        "Name:{name}\n\
         ManaCost:no cost\n\
         Types:{types}\n\
         R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ DBTap | ReplacementResult$ Updated | Description$ Text.\n\
         SVar:DBTap:DB$ Tap | ETB$ True | Defined$ Self | {tap}\n\
         Oracle:Text."
    )
}

fn shockland() -> String {
    land(
        "Test Shockland",
        "Land Island Swamp",
        "UnlessCost$ PayLife<2> | UnlessPayer$ You",
    )
}

// ---------------------------------------------------------------------------------------
// "As this enters, you may pay 2 life. If you don't, it enters tapped."

// CR 614.12a (2026-11-06): the choice is made before the permanent enters — as the land is
// played, with no priority in between. CR 119.4: paying life is losing that much life.
// CR 110.5b: it enters untapped.
#[test]
fn cr_614_12a_pay_life_as_it_enters_and_it_enters_untapped() {
    let shock = shockland();
    let mut sc = scenario(&[&shock]);
    sc.card(P0, Zone::Hand, "Test Shockland");
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Shockland");
    assert!(asks_as_it_enters(&s), "asked as it enters: {:?}", s.kind());
    assert_eq!(s.who(), P0);
    assert_eq!(s.options(), [OptionDesc::Accept, OptionDesc::Decline]);
    // Not yet on the battlefield while the choice is made.
    assert!(s.find(P0, Zone::Battlefield, "Test Shockland").is_none());
    let before = s.events.len();
    s.pick("pay", |_, o| *o == OptionDesc::Accept);
    assert_eq!(s.st.player(P0).life, 38);
    assert!(!tapped(&s, P0, "Test Shockland"));
    // The life is paid first, then the land enters.
    let new = &s.events[before..];
    let lost = new
        .iter()
        .position(|e| matches!(e, Event::LifeChanged { player, delta: -2, .. } if *player == P0))
        .expect("life paid");
    let entered = new
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::ZoneChange {
                    to: Zone::Battlefield,
                    ..
                }
            )
        })
        .expect("entered");
    assert!(lost < entered);
    // Back to its controller's priority: the land tapped for mana from its land types.
    assert_eq!(s.kind(), ChoiceKind::Priority);
}

// CR 614.12a: not paying — it enters tapped. CR 110.5b: entering tapped is not becoming
// tapped (no "becomes tapped" event).
#[test]
fn cr_614_12a_not_paying_it_enters_tapped_without_becoming_tapped() {
    let shock = shockland();
    let mut sc = scenario(&[&shock]);
    sc.card(P0, Zone::Hand, "Test Shockland");
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Shockland");
    s.pick("don't pay", |_, o| *o == OptionDesc::Decline);
    assert_eq!(s.st.player(P0).life, 40);
    assert!(tapped(&s, P0, "Test Shockland"));
    assert_eq!(s.count(|e| matches!(e, Event::Tapped { .. })), 0);
}

// CR 119.4 / 118.3: a player with less life than the payment can't pay it — nothing to
// choose, the land enters tapped.
#[test]
fn cr_119_4_too_little_life_to_pay_enters_tapped() {
    let shock = shockland();
    let mut sc = scenario(&[&shock]);
    sc.card(P0, Zone::Hand, "Test Shockland");
    sc.life(P0, 1);
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Shockland");
    assert_eq!(s.kind(), ChoiceKind::Priority, "no choice offered");
    assert!(tapped(&s, P0, "Test Shockland"));
    assert_eq!(s.st.player(P0).life, 1);
}

// CR 119.4: paying life is losing life — "whenever you lose life" triggers.
#[test]
fn cr_119_4_paying_life_as_it_enters_is_losing_life() {
    let shock = shockland();
    let watcher = "Name:Test Lossmonitor\n\
        ManaCost:1 B\n\
        Types:Creature Horror\n\
        PT:1/1\n\
        T:Mode$ LifeLost | ValidPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigDraw | TriggerDescription$ Text.\n\
        SVar:TrigDraw:DB$ Draw | NumCards$ 1\n\
        Oracle:Text.";
    let mut sc = scenario(&[&shock, watcher]);
    sc.card(P0, Zone::Hand, "Test Shockland");
    sc.card(P0, Zone::Battlefield, "Test Lossmonitor");
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Shockland");
    s.pick("pay", |_, o| *o == OptionDesc::Accept);
    assert_eq!(s.st.stack.len(), 1, "the trigger is on the stack");
    let hand = s.st.zones.list(Zone::Hand, P0).len();
    s.pass_all();
    assert_eq!(s.st.zones.list(Zone::Hand, P0).len(), hand + 1);
}

// CR 614.12b: permanents entering at once — the second choice sees the life already paid
// for the first, so the combined payments can't exceed what the player has.
#[test]
fn cr_614_12b_two_entering_at_once_pay_from_one_life_total() {
    let shock = shockland();
    let expanse = "Name:Test Expanse\n\
        ManaCost:G\n\
        Types:Sorcery\n\
        A:SP$ ChangeZone | Origin$ Hand | Destination$ Battlefield | ChangeType$ Land | ChangeNum$ 2 | SpellDescription$ Text.\n\
        Oracle:Text.";
    let mut sc = scenario(&[&shock, expanse]);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Expanse", "Test Shockland", "Test Shockland"],
    );
    sc.life(P0, 3);
    let mut s = Script::new(sc.build());
    s.cast("Test Expanse").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::ChooseCards);
    s.respond(Response::Pick(vec![0, 1]));
    assert!(asks_as_it_enters(&s));
    s.pick("pay", |_, o| *o == OptionDesc::Accept);
    // 1 life left: the second can't be paid for.
    assert!(!asks_as_it_enters(&s), "no second choice: {:?}", s.kind());
    assert_eq!(s.st.player(P0).life, 1);
    let lands: Vec<bool> =
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| s.name(id) == "Test Shockland")
            .map(|&id| s.st.object(id).tapped)
            .collect();
    assert_eq!(lands.len(), 2);
    assert_eq!(
        lands.iter().filter(|t| **t).count(),
        1,
        "one paid, one tapped"
    );
}

// ---------------------------------------------------------------------------------------
// "Enters tapped unless [condition]."

// CR 614.12: judged as it would enter, on the battlefield as it is before it enters.
// Forge: `ConditionPresent$ Island.YouCtrl,Mountain.YouCtrl | ConditionCompare$ EQ0` —
// tapped when there are none: "unless you control an Island or a Mountain".
#[test]
fn cr_614_12_enters_tapped_unless_you_control_a_type() {
    let check = land(
        "Test Checkland",
        "Land",
        "ConditionPresent$ Island.YouCtrl,Mountain.YouCtrl | ConditionCompare$ EQ0",
    );
    // Without: tapped (an opponent's Island doesn't count).
    let mut sc = scenario(&[&check]);
    sc.card(P0, Zone::Hand, "Test Checkland");
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P1, Zone::Battlefield, "Island");
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Checkland");
    assert!(tapped(&s, P0, "Test Checkland"));
    // With a Mountain: untapped.
    let mut sc = scenario(&[&check]);
    sc.card(P0, Zone::Hand, "Test Checkland");
    sc.card(P0, Zone::Battlefield, "Mountain");
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Checkland");
    assert!(!tapped(&s, P0, "Test Checkland"));
    assert_eq!(s.count(|e| matches!(e, Event::Replaced { .. })), 0);
}

// CR 614.12: "two or more other lands" counts the battlefield before it enters — the land
// itself never counts. Forge: `ConditionPresent$ Land.YouCtrl | ConditionCompare$ LT2`.
#[test]
fn cr_614_12_the_entering_land_does_not_count_itself() {
    let slow = land(
        "Test Slowland",
        "Land",
        "ConditionPresent$ Land.YouCtrl | ConditionCompare$ LT2",
    );
    let mut sc = scenario(&[&slow]);
    sc.card(P0, Zone::Hand, "Test Slowland");
    sc.card(P0, Zone::Battlefield, "Forest");
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Slowland");
    assert!(tapped(&s, P0, "Test Slowland"), "one other land: tapped");
    let mut sc = scenario(&[&slow]);
    sc.card(P0, Zone::Hand, "Test Slowland");
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Slowland");
    assert!(
        !tapped(&s, P0, "Test Slowland"),
        "two other lands: untapped"
    );
}

// CR 614.12: lands entering at the same time don't see each other — neither is on the
// battlefield while the event is being modified.
#[test]
fn cr_614_12_lands_entering_together_do_not_see_each_other() {
    let check = land(
        "Test Checkland",
        "Land",
        "ConditionPresent$ Island.YouCtrl | ConditionCompare$ EQ0",
    );
    let expanse = "Name:Test Expanse\n\
        ManaCost:G\n\
        Types:Sorcery\n\
        A:SP$ ChangeZone | Origin$ Hand | Destination$ Battlefield | ChangeType$ Land | ChangeNum$ 2 | SpellDescription$ Text.\n\
        Oracle:Text.";
    let mut sc = scenario(&[&check, expanse]);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Expanse", "Test Checkland", "Island"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Expanse").pay().pass_all();
    s.respond(Response::Pick(vec![0, 1]));
    assert!(
        tapped(&s, P0, "Test Checkland"),
        "the Island entered with it"
    );
    assert!(!tapped(&s, P0, "Island"));
}

// CR 102.2–3: "unless you have two or more opponents" — players still in the game.
// Forge: `ConditionCheckSVar$ X | ConditionSVarCompare$ LT2` with
// `SVar:X:PlayerCountOpponents$Amount`.
#[test]
fn cr_102_3_enters_tapped_unless_two_or_more_opponents() {
    let bond = format!(
        "{}\nSVar:X:PlayerCountOpponents$Amount",
        land(
            "Test Bondland",
            "Land",
            "ConditionCheckSVar$ X | ConditionSVarCompare$ LT2",
        )
    );
    let mut sc = scenario_n(2, &[&bond]);
    sc.card(P0, Zone::Hand, "Test Bondland");
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Bondland");
    assert!(tapped(&s, P0, "Test Bondland"), "one opponent");
    let mut sc = scenario_n(3, &[&bond]);
    sc.card(P0, Zone::Hand, "Test Bondland");
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Bondland");
    assert!(!tapped(&s, P0, "Test Bondland"), "two opponents");
    // One of the two has left the game: one opponent.
    let mut sc = scenario_n(3, &[&bond]);
    sc.card(P0, Zone::Hand, "Test Bondland");
    let mut st = sc.build();
    st.players[P2.index()].has_lost = true;
    let mut s = Script::new(st);
    play(&mut s, "Test Bondland");
    assert!(
        tapped(&s, P0, "Test Bondland"),
        "a player who left is no opponent"
    );
}

// Forge: `PlayerCountPlayers$LowestLifeTotal` — "unless a player has 13 or less life".
#[test]
fn enters_tapped_unless_a_player_has_little_life() {
    let low = format!(
        "{}\nSVar:X:PlayerCountPlayers$LowestLifeTotal",
        land(
            "Test Lowland",
            "Land",
            "ConditionCheckSVar$ X | ConditionSVarCompare$ GT13",
        )
    );
    let mut sc = scenario(&[&low]);
    sc.card(P0, Zone::Hand, "Test Lowland");
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Lowland");
    assert!(tapped(&s, P0, "Test Lowland"));
    let mut sc = scenario(&[&low]);
    sc.card(P0, Zone::Hand, "Test Lowland");
    sc.life(P1, 13);
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Lowland");
    assert!(!tapped(&s, P0, "Test Lowland"), "an opponent at 13");
}

// ---------------------------------------------------------------------------------------
// "As this enters, you may reveal a [type] card from your hand. If you don't, it enters
// tapped."

// CR 614.12a, 701.20a–b: the revealed card is shown to all and stays in the hand.
#[test]
fn cr_701_20_reveal_a_card_as_it_enters() {
    let reveal = land(
        "Test Revealland",
        "Land",
        "UnlessCost$ Reveal<1/Island;Swamp/Island or Swamp> | UnlessPayer$ You",
    )
    .replace(
        "Oracle:Text.",
        "Oracle:As this land enters, you may reveal an Island or Swamp card from your hand. If you don't, this land enters tapped.",
    );
    let mut sc = scenario(&[&reveal]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Revealland", "Swamp", "Forest", "Island"],
    );
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Revealland");
    assert!(asks_as_it_enters(&s));
    let names: Vec<String> = s.options().iter().map(|o| s.describe(o)).collect();
    assert_eq!(names, ["Swamp", "Island", "Decline"]);
    s.pick(
        "the Island",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Island"),
    );
    assert!(!tapped(&s, P0, "Test Revealland"));
    let island = s.find(P0, Zone::Hand, "Island").expect("still in hand");
    assert_eq!(
        s.count(|e| matches!(e, Event::Revealed { player, cards } if *player == P0 && *cards == [island])),
        1
    );
    // Declining: tapped.
    let mut sc = scenario(&[&reveal]);
    sc.cards(P0, Zone::Hand, &["Test Revealland", "Island"]);
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Revealland");
    s.pick("no", |_, o| *o == OptionDesc::Decline);
    assert!(tapped(&s, P0, "Test Revealland"));
    // Nothing to reveal: no choice, tapped.
    let mut sc = scenario(&[&reveal]);
    sc.cards(P0, Zone::Hand, &["Test Revealland", "Forest"]);
    let mut s = Script::new(sc.build());
    play(&mut s, "Test Revealland");
    assert_eq!(s.kind(), ChoiceKind::Priority);
    assert!(tapped(&s, P0, "Test Revealland"));
}

// ---------------------------------------------------------------------------------------
// Other permanents' "enter tapped".

// CR 614.1d, 614.12: "creatures your opponents control enter tapped" — judged by who will
// control the entering creature.
#[test]
fn cr_614_1d_creatures_your_opponents_control_enter_tapped() {
    let consul = "Name:Test Consul\n\
        ManaCost:W\n\
        Types:Enchantment\n\
        R:Event$ Moved | ValidCard$ Creature.OppCtrl | Destination$ Battlefield | ReplaceWith$ ETBTapped | ReplacementResult$ Updated | ActiveZones$ Battlefield | Description$ Text.\n\
        SVar:ETBTapped:DB$ Tap | ETB$ True | Defined$ ReplacedCard\n\
        Oracle:Text.";
    let mut sc = scenario(&[consul]);
    sc.card(P0, Zone::Battlefield, "Test Consul");
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    sc.card(P0, Zone::Hand, "Test Elf");
    sc.cards(P1, Zone::Battlefield, &["Forest"]);
    sc.card(P1, Zone::Hand, "Test Elf");
    sc.turn(P1, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Elf").pay().pass_all();
    assert!(
        tapped(&s, P1, "Test Elf"),
        "an opponent's creature enters tapped"
    );
    assert_eq!(
        s.count(|e| matches!(e, Event::Tapped { .. })),
        1,
        "only the Forest"
    );
    // The enchantment's controller's own creatures don't.
    let mut sc = scenario(&[consul]);
    sc.card(P0, Zone::Battlefield, "Test Consul");
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    sc.card(P0, Zone::Hand, "Test Elf");
    let mut s = Script::new(sc.build());
    s.cast("Test Elf").pay().pass_all();
    assert!(!tapped(&s, P0, "Test Elf"));
}

// CR 614.12, example: "Permanents enter tapped" doesn't affect the permanent itself.
#[test]
fn cr_614_12_permanents_enter_tapped_does_not_affect_itself() {
    let orb = "Name:Test Orb\n\
        ManaCost:1\n\
        Types:Artifact\n\
        R:Event$ Moved | ValidCard$ Permanent | Destination$ Battlefield | ReplaceWith$ ETBTapped | ReplacementResult$ Updated | ActiveZones$ Battlefield | Description$ Text.\n\
        SVar:ETBTapped:DB$ Tap | ETB$ True | Defined$ ReplacedCard\n\
        Oracle:Text.";
    let mut sc = scenario(&[orb]);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    sc.cards(P0, Zone::Hand, &["Test Orb", "Forest"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Orb").pay().pass_all();
    assert!(!tapped(&s, P0, "Test Orb"));
    play(&mut s, "Forest");
    let forests: Vec<bool> =
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| s.name(id) == "Forest")
            .map(|&id| s.st.object(id).tapped)
            .collect();
    assert_eq!(
        forests,
        [true, true],
        "paid with the first, the second entered tapped"
    );
}

// The same event, putting counters: `DB$ PutCounter | ETB$ True` on itself (no `Defined$`:
// Forge's default is the card itself) or on `ReplacedCard`.
#[test]
fn enters_with_counters_as_a_moved_replacement() {
    let own = "Name:Test Swole\n\
        ManaCost:G\n\
        Types:Creature Elf\n\
        PT:1/1\n\
        R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ C | ReplacementResult$ Updated | Description$ Text.\n\
        SVar:C:DB$ PutCounter | ETB$ True | CounterType$ P1P1 | CounterNum$ 2\n\
        Oracle:Text.";
    let mut sc = scenario(&[own]);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Swole");
    let mut s = Script::new(sc.build());
    s.cast("Test Swole").pay().pass_all();
    let id = on_battlefield(&s, P0, "Test Swole");
    assert_eq!(
        s.st.object(id)
            .counters
            .get(&card_model::CounterKind::PlusOne)
            .copied(),
        Some(2)
    );
}

// ---------------------------------------------------------------------------------------
// "If this would enter, you may discard a land card instead. If you do, put it onto the
// battlefield. If you don't, put it into its owner's graveyard."

const GEM: &str = "Name:Test Gem\n\
    ManaCost:0\n\
    Types:Artifact\n\
    A:AB$ Mana | Cost$ T | Produced$ Any | SpellDescription$ Text.\n\
    R:Event$ Moved | Destination$ Battlefield | ValidCard$ Card.Self | ReplaceWith$ PayBeforeETB | Description$ Text.\n\
    SVar:PayBeforeETB:DB$ Discard | DiscardValid$ Land | Mode$ TgtChoose | RememberDiscarded$ True | Optional$ True | AILogic$ Always | SubAbility$ MoveToGraveyard\n\
    SVar:MoveToGraveyard:DB$ ChangeZone | Origin$ All | Destination$ Graveyard | Defined$ ReplacedCard | ConditionCheckSVar$ X | ConditionSVarCompare$ EQ0 | SubAbility$ MoveToBattlefield\n\
    SVar:MoveToBattlefield:DB$ ChangeZone | Origin$ All | Destination$ Battlefield | Defined$ ReplacedCard | ConditionCheckSVar$ X | ConditionSVarCompare$ EQ1 | SubAbility$ DBCleanup\n\
    SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
    SVar:X:Remembered$Amount\n\
    Oracle:Text.";

// CR 614.1a, 614.12a, 701.9: the discard is chosen as the spell would enter and happens
// before it does; not discarding puts it into the graveyard instead.
#[test]
fn cr_614_12a_discard_a_land_instead_or_it_goes_to_the_graveyard() {
    let mut sc = scenario(&[GEM]);
    sc.cards(P0, Zone::Hand, &["Test Gem", "Forest", "Test Elf"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Gem").pass_all();
    assert!(asks_as_it_enters(&s), "{:?}", s.kind());
    let names: Vec<String> = s.options().iter().map(|o| s.describe(o)).collect();
    assert_eq!(names, ["Forest", "Decline"], "land cards only");
    s.pick(
        "the Forest",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Forest"),
    );
    on_battlefield(&s, P0, "Test Gem");
    assert!(s.find(P0, Zone::Graveyard, "Forest").is_some());
    assert_eq!(
        s.count(|e| matches!(e, Event::Discarded { player, .. } if *player == P0)),
        1
    );
    // Declining: into the graveyard, from the stack.
    let mut sc = scenario(&[GEM]);
    sc.cards(P0, Zone::Hand, &["Test Gem", "Forest"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Gem").pass_all();
    s.pick("no", |_, o| *o == OptionDesc::Decline);
    assert!(s.find(P0, Zone::Graveyard, "Test Gem").is_some());
    assert!(s.find(P0, Zone::Hand, "Forest").is_some());
    // No land card to discard: into the graveyard, nothing asked.
    let mut sc = scenario(&[GEM]);
    sc.cards(P0, Zone::Hand, &["Test Gem", "Test Elf"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Gem").pass_all();
    assert_eq!(s.kind(), ChoiceKind::Priority);
    assert!(s.find(P0, Zone::Graveyard, "Test Gem").is_some());
}

// Forge's chain must be exactly the idiom: anything else in it is not understood.
#[test]
fn discard_instead_with_another_chain_blocks() {
    let odd = GEM.replace("ConditionSVarCompare$ EQ1", "ConditionSVarCompare$ GE1");
    assert!(forge_test_blockers(&odd).contains(&"repl:Moved enters instead:shape".to_string()));
    let mandatory = GEM.replace(" | Optional$ True", "");
    assert!(
        forge_test_blockers(&mandatory).contains(&"repl:Moved enters instead:shape".to_string())
    );
}

// ---------------------------------------------------------------------------------------
// The same event to the graveyard: "… instead".

const SLAY: &str = "Name:Test Slay\n\
    ManaCost:B\n\
    Types:Instant\n\
    A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Text.\n\
    Oracle:Text.";

// CR 700.4, 614.1a: "if a creature an opponent controls would die, exile it instead" —
// judged as it is on the battlefield; the effect's controller's own creatures die.
#[test]
fn cr_700_4_a_creature_an_opponent_controls_would_die_exile_it_instead() {
    let stone = "Name:Test Stone\n\
        ManaCost:1\n\
        Types:Artifact\n\
        R:Event$ Moved | ActiveZones$ Battlefield | Origin$ Battlefield | Destination$ Graveyard | ValidLKI$ Creature.OppCtrl | ReplaceWith$ Exile | Description$ Text.\n\
        SVar:Exile:DB$ ChangeZone | Hidden$ True | Origin$ All | Destination$ Exile | Defined$ ReplacedCard\n\
        Oracle:Text.";
    let mut sc = scenario(&[stone, SLAY]);
    sc.card(P0, Zone::Battlefield, "Test Stone");
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
    sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P1, Zone::Battlefield, "Test Giant");
    sc.cards(P0, Zone::Hand, &["Test Slay", "Test Slay"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Slay").target("Test Giant").pay().pass_all();
    assert!(s.find(P1, Zone::Exile, "Test Giant").is_some());
    assert!(s.find(P1, Zone::Graveyard, "Test Giant").is_none());
    s.cast("Test Slay").target("Test Golem").pay().pass_all();
    assert!(
        s.find(P0, Zone::Graveyard, "Test Golem").is_some(),
        "its controller's own"
    );
}

// CR 113.6b: "if this card would be put into a graveyard from anywhere" works in every
// zone; CR 701.20a: it is revealed, then shuffled into its owner's library.
#[test]
fn cr_113_6b_put_into_a_graveyard_from_anywhere_shuffle_it_into_the_library_instead() {
    let colossus = "Name:Test Colossus\n\
        ManaCost:7\n\
        Types:Artifact Creature Golem\n\
        PT:7/7\n\
        R:Event$ Moved | Destination$ Graveyard | ValidCard$ Card.Self | ReplaceWith$ DBShuffle | Description$ Text.\n\
        SVar:DBShuffle:DB$ ChangeZone | Hidden$ True | Origin$ All | Destination$ Library | Defined$ ReplacedCard | Reveal$ True | Shuffle$ True\n\
        Oracle:If Test Colossus would be put into a graveyard from anywhere, reveal Test Colossus and shuffle it into its owner's library instead.";
    let mill = "Name:Test Mill\n\
        ManaCost:U\n\
        Types:Sorcery\n\
        A:SP$ Mill | NumCards$ 1 | Defined$ You | SpellDescription$ Text.\n\
        Oracle:Text.";
    // From the battlefield.
    let mut sc = scenario(&[colossus, SLAY]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    sc.card(P1, Zone::Battlefield, "Test Colossus");
    sc.card(P0, Zone::Hand, "Test Slay");
    let mut s = Script::new(sc.build());
    s.cast("Test Slay").target("Test Colossus").pay().pass_all();
    let card = s
        .find(P1, Zone::Library, "Test Colossus")
        .expect("in its library");
    assert!(s.find(P1, Zone::Graveyard, "Test Colossus").is_none());
    let _ = card;
    assert_eq!(
        s.count(|e| matches!(e, Event::Revealed { player, .. } if *player == P1)),
        1
    );
    assert_eq!(
        s.count(|e| matches!(e, Event::Shuffled { player } if *player == P1)),
        1
    );
    // From the library (milled).
    let mut sc = scenario(&[colossus, mill]);
    sc.cards(P0, Zone::Battlefield, &["Island"]);
    sc.card(P0, Zone::Library, "Test Colossus");
    sc.card(P0, Zone::Hand, "Test Mill");
    let mut s = Script::new(sc.build());
    s.cast("Test Mill").pay().pass_all();
    assert!(s.find(P0, Zone::Library, "Test Colossus").is_some());
    assert!(
        s.st.zones
            .list(Zone::Graveyard, P0)
            .iter()
            .all(|&c| s.name(c) != "Test Colossus")
    );
}

// The Oracle guard: a card that says "unless" compiles only if the engine's text does too
// (a script that drops the condition would make an always-tapped land).
#[test]
fn oracle_unless_without_an_unless_blocks() {
    let card = "Name:Test Dropland\n\
        ManaCost:no cost\n\
        Types:Land\n\
        R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ T | ReplacementResult$ Updated | Description$ Text.\n\
        SVar:T:DB$ Tap | ETB$ True | Defined$ Self\n\
        Oracle:This land enters tapped unless you control a basic land.";
    assert_eq!(forge_test_blockers(card), ["oracle:says unless"]);
    let honest = card.replace(" unless you control a basic land", "");
    assert_eq!(forge_test_blockers(&honest), [] as [&str; 0]);
}

#[test]
fn graveyard_forms_not_compiled_yet_block() {
    let base = "Name:Test Warden\n\
        ManaCost:1\n\
        Types:Artifact\n\
        R:Event$ Moved | ActiveZones$ Battlefield | Origin$ Battlefield | Destination$ Graveyard | ValidLKI$ Creature.OppCtrl | ReplaceWith$ Exile | Description$ Text.\n\
        SVar:Exile:DB$ ChangeZone | Hidden$ True | Origin$ All | Destination$ Exile | Defined$ ReplacedCard\n\
        Oracle:Text.";
    // "… exile it instead. When you do, …": more than the replacement.
    let more = base.replace(
        "Defined$ ReplacedCard",
        "Defined$ ReplacedCard | SubAbility$ D\nSVar:D:DB$ Draw | NumCards$ 1",
    );
    assert!(
        forge_test_blockers(&more)
            .contains(&"repl:Moved to graveyard:ChangeZone+SubAbility".to_string())
    );
    // "If a card would be put into an opponent's graveyard from anywhere": no vocabulary
    // for other cards from anywhere.
    let anywhere = base.replace(" | Origin$ Battlefield", "");
    assert_eq!(
        forge_test_blockers(&anywhere),
        ["repl:Moved to graveyard:shape"]
    );
    // Somewhere else than exile or the library.
    let hand = base.replace("Destination$ Exile", "Destination$ Hand");
    assert_eq!(
        forge_test_blockers(&hand),
        ["repl:Moved to graveyard:ChangeZone shape"]
    );
}

// ---------------------------------------------------------------------------------------
// What is not compiled (CLAUDE.md invariant 15).

#[test]
fn enters_forms_not_compiled_yet_block() {
    // "If this would enter, … instead": the move is replaced, not modified.
    let instead = "Name:Test Gem\n\
        ManaCost:0\n\
        Types:Artifact\n\
        R:Event$ Moved | Destination$ Battlefield | ValidCard$ Card.Self | ReplaceWith$ G | Description$ Text.\n\
        SVar:G:DB$ ChangeZone | Origin$ All | Destination$ Graveyard | Defined$ ReplacedCard\n\
        Oracle:Text.";
    assert!(forge_test_blockers(instead).contains(&"repl:Moved enters instead".to_string()));
    // Another permanent's entering under a condition: "you" would be the wrong player.
    let others = "Name:Test Warden\n\
        ManaCost:W\n\
        Types:Enchantment\n\
        R:Event$ Moved | ValidCard$ Creature.OppCtrl | Destination$ Battlefield | ReplaceWith$ T | ReplacementResult$ Updated | ActiveZones$ Battlefield | Description$ Text.\n\
        SVar:T:DB$ Tap | ETB$ True | Defined$ ReplacedCard | ConditionPresent$ Creature.YouCtrl | ConditionCompare$ EQ0\n\
        Oracle:Text.";
    assert_eq!(
        forge_test_blockers(others),
        ["repl:Moved enters:others unless"]
    );
    // A mana "unless" cost; another payer.
    let mana = land("Test Toll", "Land", "UnlessCost$ 1 | UnlessPayer$ You");
    assert_eq!(forge_test_blockers(&mana), ["repl:Moved enters:unless N"]);
    let theirs = land(
        "Test Toll",
        "Land",
        "UnlessCost$ PayLife<2> | UnlessPayer$ Opponent",
    );
    assert_eq!(
        forge_test_blockers(&theirs),
        ["repl:Moved enters:unless payer"]
    );
    // Tapped and something more.
    let more = land(
        "Test Toll",
        "Land",
        "SubAbility$ DBDraw\nSVar:DBDraw:DB$ Draw | NumCards$ 1",
    );
    assert!(forge_test_blockers(&more).contains(&"repl:Moved enters:Tap+SubAbility".to_string()));
    // From the command zone (an effect), or its own ability from the battlefield only.
    let zones = "Name:Test Warden\n\
        ManaCost:W\n\
        Types:Enchantment\n\
        R:Event$ Moved | ValidCard$ Creature | Destination$ Battlefield | ReplaceWith$ T | ReplacementResult$ Updated | ActiveZones$ Command | Description$ Text.\n\
        SVar:T:DB$ Tap | ETB$ True | Defined$ ReplacedCard\n\
        Oracle:Text.";
    assert_eq!(forge_test_blockers(zones), ["repl:Moved enters:zones"]);
}

// Forge's `+` is "and": "Card.Red+White" is a red and white card, which a selector's
// colors ("at least one of") can't say.
#[test]
fn two_colors_in_one_filter_block() {
    let card = "Name:Test Mimicry\n\
        ManaCost:1 R\n\
        Types:Instant\n\
        A:SP$ Destroy | ValidTgts$ Creature.Red+White | SpellDescription$ Text.\n\
        Oracle:Text.";
    assert_eq!(forge_test_blockers(card), ["filter:two colors"]);
    // Alternatives are "or": "Card.Red,Card.White" is one selector.
    let or = card.replace("Creature.Red+White", "Creature.Red,Creature.White");
    assert_eq!(forge_test_blockers(&or), [] as [&str; 0]);
}

// ---------------------------------------------------------------------------------------
// The heuristic agent's choice.

fn shock_decision(hand: &[&str], untapped: usize, life: i32) -> OptionDesc {
    let shock = shockland();
    let mut sc = scenario(&[&shock]);
    let mut names = vec!["Test Shockland"];
    names.extend_from_slice(hand);
    sc.cards(P0, Zone::Hand, &names);
    sc.cards(P0, Zone::Battlefield, &vec!["Forest"; untapped]);
    sc.life(P0, life);
    let mut s = Script::new(sc.build());
    let catalog = s.st.catalog.clone();
    play(&mut s, "Test Shockland");
    assert!(asks_as_it_enters(&s));
    let mut ai = ai::HeuristicAgent::new(1, catalog);
    let view = engine_core::player_view(&s.st, P0);
    let Response::Pick(i) = ai.decide(&view, s.decision()) else {
        panic!("a pick")
    };
    s.options()[i[0]].clone()
}

// The agent pays 2 life when the mana is wanted now and it has life to spare.
#[test]
fn heuristic_pays_life_for_an_untapped_land_only_when_it_wants_the_mana() {
    // Two Forests and a three-drop in hand: pays.
    assert_eq!(shock_decision(&["Test Golem"], 2, 40), OptionDesc::Accept);
    // Nothing to spend it on.
    assert_eq!(shock_decision(&[], 2, 40), OptionDesc::Decline);
    // Low on life.
    assert_eq!(shock_decision(&["Test Golem"], 2, 11), OptionDesc::Decline);
}

// A script known to differ from its card never compiles, whatever it says (invariant 8).
#[test]
fn a_script_known_to_differ_from_its_card_blocks() {
    let card = "Name:Swarmyard Massacre\n\
        ManaCost:3 B B\n\
        Types:Sorcery\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        Oracle:Draw a card.";
    assert_eq!(forge_test_blockers(card), ["script:differs from the card"]);
    let other = card.replace("Swarmyard Massacre", "Test Massacre");
    assert_eq!(forge_test_blockers(&other), [] as [&str; 0]);
}
