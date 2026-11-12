//! The most played Commander cards that no feature family covers (decisions.md D-075):
//! each feature built for them, with invented cards in Forge's script format compiled the
//! way real ones are and then played. Rules text: docs/rules-notes/d075-commander-staples.md.

use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, ObjectId, OptionDesc, PlayerId, Primitive, Response};
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

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

/// A card in Forge's format: `body` is the ability lines.
fn card(name: &str, cost: &str, types: &str, body: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{types}\n{body}\nOracle:Text.")
}

/// Answer everything with the least it allows (pass, pick nothing) until `done`.
#[allow(dead_code)]
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

/// Pick the cards `ids` among the options of a card choice.
#[allow(dead_code)]
fn pick_cards(s: &mut Script, ids: &[ObjectId]) {
    let picks: Vec<usize> = s
        .options()
        .iter()
        .enumerate()
        .filter(|(_, o)| matches!(o, OptionDesc::Card { card } if ids.contains(card)))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(picks.len(), ids.len(), "not all offered: {:?}", s.options());
    s.respond(Response::Pick(picks));
}

// ---------------------------------------------------------------------------------------
// The life lost this way

fn leech_rite() -> String {
    card(
        "Test Leech Rite",
        "X B B",
        "Sorcery",
        "A:SP$ LoseLife | Defined$ Player.Opponent | LifeAmount$ X | SubAbility$ DBGainLife | SpellDescription$ Text.\n\
         SVar:DBGainLife:DB$ GainLife | Defined$ You | LifeAmount$ AFLifeLost\n\
         SVar:X:Count$xPaid\n\
         SVar:AFLifeLost:Number$0\n\
         # AFLifeLost will be set by LoseLife",
    )
}

// CR 119.3 (2026-11-06): "If an effect causes a player to gain life or lose life, that
// player's life total is adjusted accordingly." CR 608.2c: the instructions in the order
// written. "Each opponent loses X life. You gain life equal to the life lost this way":
// what both opponents lost, together. Forge: `LifeAmount$ AFLifeLost`, which its life loss
// sets as it resolves (`SVar:AFLifeLost:Number$0` is a placeholder).
#[test]
fn life_lost_this_way_is_what_the_opponents_lost() {
    let mut sc = scenario_n(3, &[&leech_rite()]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 5]);
    sc.card(P0, Zone::Hand, "Test Leech Rite");
    let mut s = Script::new(sc.build());
    s.cast("Test Leech Rite").x(3).pay().pass_all();
    assert_eq!((life(&s, P1), life(&s, P2)), (37, 37));
    assert_eq!(life(&s, P0), 46);
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Graveyard, "Test Leech Rite").unwrap()),
    );
    assert!(
        text.join(" ")
            .contains("you gain life equal to the life lost this way"),
        "{text:?}"
    );
}

// CR 119.3: an effect that makes players lose 0 life adjusts nothing, so 0 is gained.
#[test]
fn life_lost_this_way_is_zero_when_nothing_was_lost() {
    let mut sc = scenario(&[&leech_rite()]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 2]);
    sc.card(P0, Zone::Hand, "Test Leech Rite");
    let mut s = Script::new(sc.build());
    s.cast("Test Leech Rite").x(0).pay().pass_all();
    assert_eq!((life(&s, P0), life(&s, P1)), (40, 40));
}

// Invariant 15: a variable Forge's own code sets while resolving is never read by the
// value written in the script. Before D-075, Scholar of Athreos's "you gain life equal
// to the life lost this way" compiled as "you gain 0 life" (`Number$0`).
#[test]
fn a_value_forge_sets_while_resolving_is_never_read_as_written() {
    // Not after a life loss: no meaning to give it.
    let alone = card(
        "Test Odd Rite",
        "B",
        "Sorcery",
        "A:SP$ GainLife | Defined$ You | LifeAmount$ AFLifeLost | SpellDescription$ Text.\n\
         SVar:AFLifeLost:Number$0",
    );
    assert_eq!(
        forge_test_blockers(&alone),
        ["expr:AFLifeLost not after a life loss"]
    );
    // Another variable Forge fills in (the damage a prevention effect prevented).
    let prevented = card(
        "Test Mended Rite",
        "W",
        "Sorcery",
        "A:SP$ GainLife | Defined$ You | LifeAmount$ PreventedDamage | SpellDescription$ Text.\n\
         SVar:PreventedDamage:Number$0",
    );
    assert_eq!(
        forge_test_blockers(&prevented),
        ["expr:set while resolving PreventedDamage"]
    );
}

// ---------------------------------------------------------------------------------------
// "In any order"

// CR 401.4 (2026-11-06): "If an effect puts two or more cards in a specific position in a
// library at the same time, the owner of those cards may arrange them in any order." Forge:
// `Reorder$ True` on a `ChangeZone` from the hand to the library ("draw three cards, then
// put two cards from your hand on top of your library in any order").
#[test]
fn cards_put_on_top_from_the_hand_are_ordered_by_their_owner() {
    let storm = card(
        "Test Mindstorm",
        "U",
        "Instant",
        "A:SP$ Draw | NumCards$ 3 | SubAbility$ DBPut | SpellDescription$ Text.\n\
         SVar:DBPut:DB$ ChangeZone | Origin$ Hand | Destination$ Library | ChangeNum$ 2 | Mandatory$ True | Reorder$ True",
    );
    let mut sc = scenario(&[&storm]);
    sc.card(P0, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Hand, "Test Mindstorm");
    let swamp = sc.card(P0, Zone::Hand, "Swamp");
    let mountain = sc.card(P0, Zone::Hand, "Mountain");
    let mut s = Script::new(sc.build());
    s.cast("Test Mindstorm").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::ChooseCards);
    assert_eq!(s.st.zones.list(Zone::Hand, P0).len(), 5);
    pick_cards(&mut s, &[swamp, mountain]);
    // Now the order: the Mountain on top.
    assert_eq!(s.who(), P0);
    let order: Vec<usize> = [mountain, swamp]
        .iter()
        .map(|c| {
            s.options()
                .iter()
                .position(|o| matches!(o, OptionDesc::Card { card } if card == c))
                .unwrap()
        })
        .collect();
    s.respond(Response::Order(order));
    let library = s.st.zones.list(Zone::Library, P0);
    let top: Vec<String> = library.iter().rev().take(2).map(|&c| s.name(c)).collect();
    assert_eq!(top, ["Mountain", "Swamp"]);
    assert_eq!(s.st.zones.list(Zone::Hand, P0).len(), 3);
    // To anywhere but a library it means nothing the compiler knows.
    let odd = card(
        "Test Odd Storm",
        "U",
        "Instant",
        "A:SP$ ChangeZone | Origin$ Hand | Destination$ Exile | ChangeNum$ 2 | Mandatory$ True | Reorder$ True | SpellDescription$ Text.",
    );
    assert_eq!(
        forge_test_blockers(&odd),
        ["param:ChangeZone.Reorder not to a library"]
    );
}

// ---------------------------------------------------------------------------------------
// Untap up to N

// CR 701.26b (2026-11-06): "To untap a permanent, rotate it back to the upright position
// from a sideways position. Only tapped permanents can be untapped." Forge: `DB$ Untap |
// UntapUpTo$ True | UntapType$ Land | Amount$ 2` — "untap up to two lands", any lands,
// chosen as it resolves (not targets).
#[test]
fn untap_up_to_n_lands_chooses_among_tapped_lands_of_anyone() {
    let snap = card(
        "Test Unsnap",
        "1 U",
        "Instant",
        "A:SP$ Untap | UntapUpTo$ True | UntapType$ Land | Amount$ 2 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&snap]);
    let islands = sc.cards(P0, Zone::Battlefield, &["Island", "Island", "Island"]);
    let untapped = sc.card(P0, Zone::Battlefield, "Forest");
    let theirs = sc.card(P1, Zone::Battlefield, "Swamp");
    sc.tapped(islands[2]);
    sc.tapped(theirs);
    sc.card(P0, Zone::Hand, "Test Unsnap");
    let mut s = Script::new(sc.build());
    s.cast("Test Unsnap").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::Untap);
    assert_eq!(s.who(), P0);
    // The two Islands tapped to pay, the one tapped before, and the opponent's Swamp:
    // not the untapped Forest.
    let offered: Vec<ObjectId> = s
        .options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Card { card } => Some(*card),
            _ => None,
        })
        .collect();
    assert_eq!(offered.len(), 4);
    assert!(offered.contains(&theirs) && !offered.contains(&untapped));
    assert!(matches!(
        s.decision().prim,
        Primitive::PickN { min: 0, max: 2 }
    ));
    pick_cards(&mut s, &[islands[0], theirs]);
    assert!(!s.st.object(islands[0]).tapped);
    assert!(!s.st.object(theirs).tapped);
    assert!(s.st.object(islands[1]).tapped);
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Graveyard, "Test Unsnap").unwrap()),
    );
    assert_eq!(text, ["Untap up to 2 lands."]);
    // Chosen by another player: not compiled.
    let theirs = card(
        "Test Odd Unsnap",
        "1 U",
        "Instant",
        "A:SP$ Untap | UntapUpTo$ True | UntapType$ Land | Amount$ 2 | Defined$ Player.Opponent | SpellDescription$ Text.",
    );
    assert_eq!(
        forge_test_blockers(&theirs),
        ["param:Untap.UntapUpTo by another player"]
    );
}

// ---------------------------------------------------------------------------------------
// Proliferate

fn with_counters(
    st: &mut engine_core::GameState,
    id: ObjectId,
    kind: card_model::CounterKind,
    n: u32,
) {
    st.objects.get_mut(&id).unwrap().counters.insert(kind, n);
}

fn counters_of(s: &Script, id: ObjectId, kind: card_model::CounterKind) -> u32 {
    s.st.object(id).counters.get(&kind).copied().unwrap_or(0)
}

// CR 701.34a (2026-11-06): "To proliferate means to choose any number of permanents and/or
// players that have a counter, then give each one additional counter of each kind that
// permanent or player already has." Forge: `SP$ Proliferate`; `Amount$ 2` is "proliferate
// twice" (two choices, one after the other).
#[test]
fn proliferate_gives_each_chosen_permanent_another_counter_of_each_kind() {
    use card_model::CounterKind::{MinusOne, PlusOne};
    let spread = card(
        "Test Spread",
        "1 U",
        "Sorcery",
        "A:SP$ Proliferate | SpellDescription$ Text.",
    );
    let bear = "Name:Test Cub\nManaCost:1 G\nTypes:Creature Bear\nPT:4/4\nOracle:Text.";
    let mut sc = scenario(&[&spread, bear]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Cub");
    let plain = sc.card(P0, Zone::Battlefield, "Test Cub");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Cub");
    let skipped = sc.card(P1, Zone::Battlefield, "Test Cub");
    sc.card(P0, Zone::Hand, "Test Spread");
    let mut st = sc.build();
    with_counters(&mut st, mine, PlusOne, 2);
    with_counters(&mut st, theirs, MinusOne, 1);
    with_counters(&mut st, skipped, PlusOne, 1);
    let mut s = Script::new(st);
    s.cast("Test Spread").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::Proliferate);
    // Only permanents that have a counter, the opponent's included.
    let offered: Vec<ObjectId> = s
        .options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Card { card } => Some(*card),
            _ => None,
        })
        .collect();
    assert_eq!(offered, [mine, theirs, skipped]);
    assert!(!offered.contains(&plain));
    pick_cards(&mut s, &[mine, theirs]);
    assert_eq!(counters_of(&s, mine, PlusOne), 3);
    assert_eq!(counters_of(&s, theirs, MinusOne), 2);
    assert_eq!(counters_of(&s, skipped, PlusOne), 1);
    // Twice: two choices.
    let twice = card(
        "Test Double Spread",
        "1 U",
        "Sorcery",
        "A:SP$ Proliferate | Amount$ 2 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&twice, bear]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Cub");
    sc.card(P0, Zone::Hand, "Test Double Spread");
    let mut st = sc.build();
    with_counters(&mut st, mine, PlusOne, 1);
    let mut s = Script::new(st);
    s.cast("Test Double Spread").pay().pass_all();
    pick_cards(&mut s, &[mine]);
    assert_eq!(s.kind(), ChoiceKind::Proliferate);
    pick_cards(&mut s, &[mine]);
    assert_eq!(counters_of(&s, mine, PlusOne), 3);
    assert_eq!(
        card_model::describe::rules_text(
            s.st.card_def(s.find(P0, Zone::Graveyard, "Test Double Spread").unwrap())
        ),
        ["Proliferate. Then proliferate."]
    );
}

// ---------------------------------------------------------------------------------------
// With D-072's alternative costs

// CR 119.3 (2026-11-06): a player who loses more life than they have still loses all of
// it — their life total goes below 0 (CR 119.6: a player can have a negative life total;
// 704.5a then makes them lose). So "you gain life equal to the life lost this way" counts
// all of it, here for a spell cast for its flashback cost (CR 702.34a).
#[test]
fn life_lost_this_way_counts_a_loss_below_zero_and_works_with_flashback() {
    let rite = card(
        "Test Late Rite",
        "3 B",
        "Sorcery",
        "K:Flashback:4 B\n\
         A:SP$ LoseLife | Defined$ Player.Opponent | LifeAmount$ 3 | SubAbility$ DBGainLife | SpellDescription$ Text.\n\
         SVar:DBGainLife:DB$ GainLife | Defined$ You | LifeAmount$ AFLifeLost\n\
         SVar:AFLifeLost:Number$0",
    );
    let mut sc = scenario_n(3, &[&rite]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 5]);
    sc.card(P0, Zone::Graveyard, "Test Late Rite");
    sc.life(P1, 1);
    sc.turn(P0, engine_core::turn::Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    assert!(!s.can_cast("Test Late Rite"));
    s.cast_with("Test Late Rite").pay();
    // Before it resolves P1 is still in the game at 1 life.
    s.pass_all();
    assert_eq!(life(&s, P2), 37);
    assert_eq!(life(&s, P0), 46, "3 from each opponent, P1's 3 included");
    assert!(s.find(P0, Zone::Exile, "Test Late Rite").is_some());
}

// CR 701.26b with CR 118.9 / 601.2h: a land returned to its owner's hand to pay an
// alternative cost is no longer a permanent; "untap up to two lands" can't choose it.
#[test]
fn untap_up_to_n_after_a_land_was_returned_to_pay_the_cost() {
    let gush = card(
        "Test Rush",
        "4 U",
        "Instant",
        "S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ Return<1/Land> | Description$ Text.\n\
         A:SP$ Untap | UntapUpTo$ True | UntapType$ Land | Amount$ 2 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&gush]);
    let island = sc.card(P0, Zone::Battlefield, "Island");
    let forest = sc.card(P0, Zone::Battlefield, "Forest");
    let swamp = sc.card(P0, Zone::Battlefield, "Swamp");
    for l in [island, forest, swamp] {
        sc.tapped(l);
    }
    sc.card(P0, Zone::Hand, "Test Rush");
    let mut s = Script::new(sc.build());
    s.cast_with("Test Rush");
    assert_eq!(s.kind(), ChoiceKind::ReturnToHand);
    pick_cards(&mut s, &[island]);
    s.pass_all();
    assert!(s.find(P0, Zone::Hand, "Island").is_some());
    assert_eq!(s.kind(), ChoiceKind::Untap);
    let offered: Vec<ObjectId> = s
        .options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Card { card } => Some(*card),
            _ => None,
        })
        .collect();
    assert_eq!(offered, [forest, swamp]);
}

// ---------------------------------------------------------------------------------------
// Cards that share a land type

// CR 205.3i (2026-11-06): "Lands have their own unique set of subtypes; these subtypes are
// called land types." CR 701.23b: cards with a stated quality needn't be found. Forge:
// `ChangeZone … | ChangeNum$ 2 | ShareLandType$ True` ("search your library for up to two
// basic land cards that share a land type"): chosen one at a time, each further card
// sharing a land type with each one chosen before.
#[test]
fn search_for_lands_that_share_a_land_type() {
    let landscape = card(
        "Test Vista",
        "no cost",
        "Land",
        "A:AB$ ChangeZone | Cost$ 2 T Sac<1/CARDNAME> | Origin$ Library | Destination$ Battlefield | ChangeType$ Land.Basic | Tapped$ True | ChangeNum$ 2 | ShareLandType$ True | SpellDescription$ Text.",
    );
    let mut sc = Scenario::new(
        GameConfig::commander(2),
        forge_test_catalog(&[&landscape], &[]),
        31,
    );
    sc.library(P1, "Forest", 20);
    let islands = sc.cards(P0, Zone::Library, &["Island", "Island"]);
    let mountain = sc.card(P0, Zone::Library, "Mountain");
    sc.card(P0, Zone::Battlefield, "Test Vista");
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
    let mut s = Script::new(sc.build());
    s.activate("Test Vista").pay();
    s.pass_all();
    assert_eq!(s.kind(), ChoiceKind::SearchLibrary);
    assert!(matches!(
        s.decision().prim,
        Primitive::PickN { min: 0, max: 1 }
    ));
    assert_eq!(s.options().len(), 3);
    pick_cards(&mut s, &[islands[0]]);
    // The second: only a card sharing a land type with the Island.
    assert_eq!(s.kind(), ChoiceKind::SearchLibrary);
    let offered: Vec<ObjectId> = s
        .options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Card { card } => Some(*card),
            _ => None,
        })
        .collect();
    assert_eq!(offered, [islands[1]]);
    assert!(!offered.contains(&mountain));
    pick_cards(&mut s, &[islands[1]]);
    let on_field: Vec<ObjectId> =
        s.st.zones
            .battlefield
            .iter()
            .copied()
            .filter(|&o| s.name(o) == "Island")
            .collect();
    assert_eq!(on_field.len(), 2);
    assert!(on_field.iter().all(|&o| s.st.object(o).tapped));
    assert_eq!(
        s.count(|e| matches!(e, engine_core::Event::SearchedLibrary { .. })),
        1
    );
    // Stopping after one card is allowed ("up to two").
    let mut sc = Scenario::new(
        GameConfig::commander(2),
        forge_test_catalog(&[&landscape], &[]),
        31,
    );
    sc.library(P1, "Forest", 20);
    sc.cards(P0, Zone::Library, &["Island", "Island"]);
    let mountain = sc.card(P0, Zone::Library, "Mountain");
    sc.card(P0, Zone::Battlefield, "Test Vista");
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
    let mut s = Script::new(sc.build());
    s.activate("Test Vista").pay();
    s.pass_all();
    pick_cards(&mut s, &[mountain]);
    // Nothing else shares a land type with the Mountain: no second question.
    assert_ne!(
        s.next.as_ref().map(|d| d.kind),
        Some(ChoiceKind::SearchLibrary)
    );
    assert!(s.find(P0, Zone::Battlefield, "Mountain").is_some());
    assert!(s.find(P0, Zone::Battlefield, "Island").is_none());
}

// ---------------------------------------------------------------------------------------
// Delayed triggered abilities

/// Answer with the least until `who` has priority in `step` of their own turn, with an
/// empty stack.
fn drive_to(s: &mut Script, who: PlayerId, step: engine_core::turn::Step) {
    drive_until(s, |s| {
        s.kind() == ChoiceKind::Priority
            && s.who() == who
            && s.st.turn.active == who
            && s.st.turn.step == step
            && s.st.stack.is_empty()
    });
}

const BIG: &str = "Name:Test Big Idea\nManaCost:X G\nTypes:Sorcery\n\
    A:SP$ Draw | NumCards$ 1 | SpellDescription$ Text.\nSVar:X:Count$xPaid\nOracle:Text.";

// CR 603.7a (2026-11-06): "A delayed triggered ability is created during the resolution of
// a spell or ability … it triggers when its trigger event occurs" — once (603.7c), and
// what it refers to was determined as it was created. Forge: `DB$ DelayedTrigger | Mode$
// Phase | Phase$ Main1,Main2 | ValidPlayer$ You | RememberNumber$ True` after a counter
// with `RememberCounteredCMC$` ("counter target spell; at the beginning of your next main
// phase, add {C} equal to that spell's mana value"). The mana value is the one it had on
// the stack, X included (CR 202.3e).
#[test]
fn a_delayed_trigger_adds_mana_equal_to_the_countered_spells_mana_value() {
    let drain = card(
        "Test Siphon",
        "U U",
        "Instant",
        "A:SP$ Counter | TargetType$ Spell | RememberCounteredCMC$ True | ValidTgts$ Card | SubAbility$ DBDelTrig | SpellDescription$ Text.\n\
         SVar:DBDelTrig:DB$ DelayedTrigger | Mode$ Phase | Phase$ Main1,Main2 | ValidPlayer$ You | Execute$ AddMana | RememberNumber$ True | SubAbility$ DBCleanup\n\
         SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True\n\
         SVar:AddMana:DB$ Mana | Produced$ C | Amount$ X\n\
         SVar:X:Count$TriggerRememberAmount",
    );
    let mut sc = scenario(&[&drain, BIG]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Forest", "Forest", "Island", "Island"],
    );
    sc.cards(P0, Zone::Hand, &["Test Big Idea", "Test Siphon"]);
    sc.turn(P0, engine_core::turn::Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Big Idea").x(2).pay();
    s.cast("Test Siphon").target("Test Big Idea").pay();
    s.pass_all();
    assert!(s.find(P0, Zone::Graveyard, "Test Big Idea").is_some());
    assert_eq!(s.st.delayed.len(), 1);
    // Not now: at the beginning of the next main phase (this turn's second one).
    drive_to(&mut s, P0, engine_core::turn::Step::PostcombatMain);
    assert_eq!(
        s.st.player(P0).mana_pool.total(),
        3,
        "{{2}}{{G}} cast with X = 2"
    );
    assert!(s.st.delayed.is_empty(), "it triggers only once");
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Graveyard, "Test Siphon").unwrap()),
    )
    .join(" ");
    assert!(
        text.contains("at the beginning of your next precombat main phase"),
        "{text}"
    );
}

// CR 603.7c: "A delayed triggered ability that refers to a particular object still affects
// it even if the object changes zones" — here the player: "its controller may draw up to
// two cards at the beginning of the next turn's upkeep. You draw a card at the beginning of
// the next turn's upkeep." Forge: `NextTurn$ True`, `RememberObjects$ RememberedController`
// with `RememberTargets$` on the counter, and `Draw | Upto$ True` (the drawer chooses how
// many).
#[test]
fn delayed_triggers_of_the_next_turn_remember_a_player() {
    let denial = card(
        "Test Refusal",
        "1 U",
        "Instant",
        "A:SP$ Counter | TargetType$ Spell | RememberTargets$ True | ValidTgts$ Card | SubAbility$ DelTrigSlowtrip | SpellDescription$ Text.\n\
         SVar:DelTrigSlowtrip:DB$ DelayedTrigger | NextTurn$ True | Mode$ Phase | Phase$ Upkeep | ValidPlayer$ Player | Execute$ DrawSlowtrip | SubAbility$ DelTrigDrawTwo\n\
         SVar:DrawSlowtrip:DB$ Draw | Defined$ You\n\
         SVar:DelTrigDrawTwo:DB$ DelayedTrigger | NextTurn$ True | Mode$ Phase | Phase$ Upkeep | ValidPlayer$ Player | RememberObjects$ RememberedController | Execute$ DrawTwo | SubAbility$ DBCleanup\n\
         SVar:DrawTwo:DB$ Draw | NumCards$ 2 | Defined$ DelayTriggerRemembered | Upto$ True\n\
         SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True",
    );
    let mut sc = scenario(&[&denial, BIG]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Big Idea");
    sc.cards(P1, Zone::Battlefield, &["Island", "Island"]);
    sc.card(P1, Zone::Hand, "Test Refusal");
    sc.turn(P0, engine_core::turn::Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Big Idea").x(1).pay().pass();
    // P1 counters it.
    s.cast("Test Refusal").target("Test Big Idea").pay();
    s.pass_all();
    assert_eq!(s.st.delayed.len(), 2);
    let hands = |s: &Script| {
        (
            s.st.zones.list(Zone::Hand, P0).len(),
            s.st.zones.list(Zone::Hand, P1).len(),
        )
    };
    let before = hands(&s);
    // Nothing during the rest of this turn (P0's end step included).
    drive_until(&mut s, |s| {
        s.st.turn.active == P1 && s.st.turn.step == engine_core::turn::Step::Upkeep
    });
    assert_eq!(hands(&s), before);
    // P1's upkeep: P1 orders their two triggers, then P0 chooses how many to draw.
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DrawUpTo);
    assert_eq!(s.who(), P0);
    assert!(matches!(
        s.decision().prim,
        Primitive::PickNumber { min: 0, max: 2 }
    ));
    s.respond(Response::Number(1));
    drive_until(&mut s, |s| s.st.delayed.is_empty() && s.st.stack.is_empty());
    let after = hands(&s);
    assert_eq!(after.0, before.0 + 1, "P0 chose to draw one");
    // P1 drew its slowtrip card (P1 has not reached its draw step yet).
    assert_eq!(s.st.turn.step, engine_core::turn::Step::Upkeep);
    assert_eq!(after.1, before.1 + 1);
}

// CR 514.2: "… 'this turn' effects end" at cleanup; a delayed trigger for "this turn" that
// hasn't triggered is gone then. Forge: `ThisTurn$ True`.
#[test]
fn a_delayed_trigger_for_this_turn_ends_at_cleanup() {
    let conduit = card(
        "Test Conduit",
        "2 R",
        "Artifact",
        "A:AB$ DelayedTrigger | Cost$ T | Mode$ Phase | Phase$ Main2 | ValidPlayer$ You | ThisTurn$ True | Execute$ TrigMana | SpellDescription$ Text.\n\
         SVar:TrigMana:DB$ Mana | Produced$ R | Amount$ 1",
    );
    let mut sc = scenario(&[&conduit]);
    sc.card(P0, Zone::Battlefield, "Test Conduit");
    sc.turn(P0, engine_core::turn::Step::PostcombatMain);
    let mut s = Script::new(sc.build());
    s.activate("Test Conduit").pass_all();
    assert_eq!(s.st.delayed.len(), 1);
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert!(s.st.delayed.is_empty(), "over at P0's cleanup");
    // A delayed trigger that remembers objects is not compiled yet.
    let returns = card(
        "Test Blink",
        "W",
        "Instant",
        "A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Exile | RememberChanged$ True | SubAbility$ DelTrig | SpellDescription$ Text.\n\
         SVar:DelTrig:DB$ DelayedTrigger | Mode$ Phase | Phase$ End of Turn | Execute$ TrigReturn | RememberObjects$ Remembered | SubAbility$ DBCleanup\n\
         SVar:TrigReturn:DB$ ChangeZone | Defined$ DelayTriggerRemembered | Origin$ Exile | Destination$ Battlefield\n\
         SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True",
    );
    assert!(
        forge_test_blockers(&returns).contains(&"DelayedTrigger:remembers objects".to_string())
    );
}

// ---------------------------------------------------------------------------------------
// Text only

// Forge's `CostDesc$` is how the cost reads in the ability's description
// (`SpellAbility.getDescription`); the cost is `Cost$`. A card compiles the same with or
// without it, and the discard is still an additional cost (CR 601.2b, 601.2h).
#[test]
fn a_cost_description_is_text_only() {
    let with = |desc: &str| {
        card(
            "Test Score",
            "3 R",
            "Instant",
            &format!(
                "A:SP$ Draw | Cost$ 3 R Discard<1/Card/card> {desc}| NumCards$ 2 | Defined$ You | SpellDescription$ Text."
            ),
        )
    };
    let a = forge_test_catalog(
        &[&with("| CostDesc$ As an additional cost, discard a card. ")],
        &[],
    );
    let b = forge_test_catalog(&[&with("")], &[]);
    let def = |c: &card_model::CardCatalog| c.get(c.find("Test Score").unwrap()).clone();
    assert_eq!(def(&a), def(&b));
    assert!(
        card_model::describe::rules_text(&def(&a))
            .join(" ")
            .contains("discard a card")
    );
}

// ---------------------------------------------------------------------------------------
// Look at the top N cards and put them back in any order

// CR 401.4 (2026-11-06) and CR 701.24a ("To shuffle a library or a face-down pile of
// cards, randomize the cards within it so that no player knows their order."). Forge: `SP$ RearrangeTopOfLibrary | Defined$ You | NumCards$ 3 |
// MayShuffle$ True` ("look at the top three cards of your library, then put them back in
// any order. You may shuffle."). The controller looks and orders, whoever's library it is.
#[test]
fn look_at_the_top_cards_put_them_back_in_any_order_then_maybe_shuffle() {
    let ponder = card(
        "Test Musing",
        "U",
        "Sorcery",
        "A:SP$ RearrangeTopOfLibrary | Defined$ You | NumCards$ 3 | MayShuffle$ True | SubAbility$ DBDraw | SpellDescription$ Text.\n\
         SVar:DBDraw:DB$ Draw | Defined$ You | NumCards$ 1",
    );
    let mut sc = scenario(&[&ponder]);
    let island = sc.card(P0, Zone::Library, "Island");
    let swamp = sc.card(P0, Zone::Library, "Swamp");
    let mountain = sc.card(P0, Zone::Library, "Mountain");
    sc.card(P0, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Hand, "Test Musing");
    sc.turn(P0, engine_core::turn::Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Musing").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::ScryOrder);
    assert_eq!(s.who(), P0);
    let order: Vec<usize> = [swamp, island, mountain]
        .iter()
        .map(|c| {
            s.options()
                .iter()
                .position(|o| matches!(o, OptionDesc::Card { card } if card == c))
                .unwrap()
        })
        .collect();
    s.respond(Response::Order(order));
    // "You may shuffle": no.
    assert_eq!(s.kind(), ChoiceKind::MayAbility);
    s.pick("decline", |_, o| matches!(o, OptionDesc::Decline));
    assert!(
        s.find(P0, Zone::Hand, "Swamp").is_some(),
        "the new top card was drawn"
    );
    let library = s.st.zones.list(Zone::Library, P0);
    let top: Vec<String> = library.iter().rev().take(2).map(|&c| s.name(c)).collect();
    assert_eq!(top, ["Island", "Mountain"]);
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Graveyard, "Test Musing").unwrap()),
    );
    assert_eq!(
        text,
        [
            "You look at the top 3 cards of your library, then put them back in any order. Then you may shuffle your library. Then you draw a card."
        ]
    );
    // Accepting shuffles it.
    let mut sc = scenario(&[&ponder]);
    sc.cards(P0, Zone::Library, &["Island", "Swamp", "Mountain"]);
    sc.card(P0, Zone::Battlefield, "Island");
    sc.card(P0, Zone::Hand, "Test Musing");
    sc.turn(P0, engine_core::turn::Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Musing").pay().pass_all();
    s.respond(Response::Order(vec![0, 1, 2]));
    s.pick("accept", |_, o| matches!(o, OptionDesc::Accept));
    assert_eq!(
        s.count(|e| matches!(e, engine_core::Event::Shuffled { player } if *player == P0)),
        1
    );
}
