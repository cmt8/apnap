//! Rules about players and about where cards may be played from (decisions.md D-070):
//! maximum hand size, additional land plays, hexproof and shroud on players, playing lands
//! and casting spells from the graveyard and the top of the library, the top card of a
//! library revealed or looked at. Invented cards in Forge's script format, compiled the
//! way real ones are and then played. Rules text:
//! docs/rules-notes/402-305-401.5-rules-about-players.md.

use engine_core::players;
use engine_core::stack::Target;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
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

fn creature(name: &str, cost: &str, types: &str, pt: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:Creature {types}\nPT:{pt}\n{rest}\nOracle:Text.")
}

fn artifact(name: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:2\nTypes:Artifact\n{rest}\nOracle:Text.")
}

/// "Creatures lose all abilities."
fn hush() -> String {
    "Name:Test Hush\nManaCost:2 U\nTypes:Enchantment\n\
     S:Mode$ Continuous | Affected$ Creature | RemoveAllAbilities$ True | Description$ Text.\n\
     Oracle:Text."
        .to_string()
}

fn lands_offered(s: &Script) -> Vec<ObjectId> {
    s.options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::PlayLand { card } => Some(*card),
            _ => None,
        })
        .collect()
}

fn play(s: &mut Script, card: ObjectId) {
    s.pick(
        "play the land",
        |_, o| matches!(o, OptionDesc::PlayLand { card: c } if *c == card),
    );
}

fn can_cast_card(s: &Script, card: ObjectId) -> bool {
    s.has(|_, o| matches!(o, OptionDesc::CastSpell { card: c } if *c == card))
}

fn can_target_player(s: &Script, p: PlayerId) -> bool {
    s.has(|_, o| matches!(o, OptionDesc::Target { target: Target::Player(q) } if *q == p))
}

// ---------------------------------------------------------------------------------------
// Maximum hand size

fn archivist() -> String {
    creature(
        "Test Archivist",
        "1 U",
        "Human Wizard",
        "1/1",
        "S:Mode$ Continuous | Affected$ You | SetMaxHandSize$ Unlimited | Description$ Text.",
    )
}

/// Go from the end step through cleanup; the number of cards `p` was asked to discard.
fn cleanup_discards(s: &mut Script, p: PlayerId) -> usize {
    let mut n = 0;
    let turn = s.st.turn.turn_number;
    drive_until(s, |s| {
        s.kind() == ChoiceKind::Discard || s.st.turn.turn_number != turn
    });
    if s.next.is_some() && s.kind() == ChoiceKind::Discard {
        assert_eq!(s.who(), p);
        if let Primitive::PickN { min, .. } = s.decision().prim {
            n = min;
        }
        s.respond(Response::Pick((0..n).collect()));
    }
    n
}

// CR 402.2 (2026-11-06): "Each player has a maximum hand size, which is normally seven
// cards. … as part of their cleanup step, the player must discard excess cards down to the
// maximum hand size." CR 514.1. Forge: `Affected$ You | SetMaxHandSize$ Unlimited`.
#[test]
fn no_maximum_hand_size_means_no_cleanup_discard() {
    let mut sc = scenario(&[&archivist()]);
    sc.card(P0, Zone::Battlefield, "Test Archivist");
    sc.cards(P0, Zone::Hand, &["Forest"; 10]);
    sc.turn(P0, Step::End);
    let mut s = Script::new(sc.build());
    assert_eq!(players::max_hand_size(&s.st, P0), None);
    assert_eq!(players::max_hand_size(&s.st, P1), Some(7));
    assert_eq!(cleanup_discards(&mut s, P0), 0);
    assert_eq!(s.st.zones.list(Zone::Hand, P0).len(), 10);
}

// The rule is the controller's only, and ends when the permanent leaves the battlefield.
#[test]
fn no_maximum_hand_size_is_only_the_controllers_and_ends_with_the_permanent() {
    let mut sc = scenario_n(3, &[&archivist()]);
    sc.card(P1, Zone::Battlefield, "Test Archivist");
    sc.cards(P0, Zone::Hand, &["Forest"; 9]);
    sc.turn(P0, Step::End);
    let mut s = Script::new(sc.build());
    assert_eq!(players::max_hand_size(&s.st, P1), None);
    assert_eq!(players::max_hand_size(&s.st, P2), Some(7));
    assert_eq!(cleanup_discards(&mut s, P0), 2, "P0 has the normal maximum");

    // P0's archivist, while it is there.
    let mut sc = scenario(&[&archivist()]);
    sc.card(P0, Zone::Battlefield, "Test Archivist");
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.cards(P0, Zone::Hand, &["Forest"; 9]);
    sc.card(P0, Zone::Hand, "Test Recall");
    sc.turn(P0, Step::End);
    let mut s = Script::new(sc.build());
    s.cast("Test Recall")
        .target("Test Archivist")
        .pay()
        .pass_all();
    assert_eq!(players::max_hand_size(&s.st, P0), Some(7));
    // Nine Forests and the archivist back in hand: three over seven.
    assert_eq!(cleanup_discards(&mut s, P0), 3);
}

// CR 613.11: rules about a maximum hand size apply in timestamp order. "Each opponent's
// maximum hand size is reduced by two" (Forge: `Affected$ Opponent | RaiseMaxHandSize$ -2`)
// and "no maximum hand size": the later one wins over the earlier.
#[test]
fn maximum_hand_size_rules_apply_in_timestamp_order() {
    let gag = artifact(
        "Test Gag",
        "S:Mode$ Continuous | Affected$ Opponent | RaiseMaxHandSize$ -2 | Description$ Text.",
    );
    let seven = artifact(
        "Test Seven",
        "S:Mode$ Continuous | Affected$ Player | SetMaxHandSize$ 4 | Description$ Text.",
    );
    let mut sc = scenario(&[&gag, &archivist(), &seven]);
    sc.card(P1, Zone::Battlefield, "Test Gag");
    let mut s = Script::new(sc.build());
    assert_eq!(players::max_hand_size(&s.st, P0), Some(5));
    assert_eq!(players::max_hand_size(&s.st, P1), Some(7));
    // P0's later "no maximum hand size" wins.
    let mut sc = scenario(&[&gag, &archivist(), &seven]);
    sc.card(P1, Zone::Battlefield, "Test Gag");
    sc.card(P0, Zone::Battlefield, "Test Archivist");
    s = Script::new(sc.build());
    assert_eq!(players::max_hand_size(&s.st, P0), None);
    // A later "maximum hand size is four" for each player, then the reduction.
    let mut sc = scenario(&[&gag, &archivist(), &seven]);
    sc.card(P0, Zone::Battlefield, "Test Seven");
    sc.card(P1, Zone::Battlefield, "Test Gag");
    s = Script::new(sc.build());
    assert_eq!(players::max_hand_size(&s.st, P0), Some(2));
    assert_eq!(players::max_hand_size(&s.st, P1), Some(4));
    // At cleanup P0 discards down to two.
    let mut sc = scenario(&[&gag, &archivist(), &seven]);
    sc.card(P0, Zone::Battlefield, "Test Seven");
    sc.card(P1, Zone::Battlefield, "Test Gag");
    sc.cards(P0, Zone::Hand, &["Forest"; 3]);
    sc.turn(P0, Step::End);
    let mut s = Script::new(sc.build());
    assert_eq!(cleanup_discards(&mut s, P0), 1);
}

// ---------------------------------------------------------------------------------------
// Land plays

fn explorer() -> String {
    creature(
        "Test Explorer",
        "G",
        "Elf Scout",
        "1/1",
        "S:Mode$ Continuous | Affected$ You | AdjustLandPlays$ 1 | Description$ Text.",
    )
}

/// P0 in their precombat main phase with `hand` lands in hand.
fn main_phase(sc: &mut Scenario, hand: usize) -> Vec<ObjectId> {
    sc.turn(P0, Step::PrecombatMain);
    sc.cards(P0, Zone::Hand, &vec!["Forest"; hand])
}

// CR 305.2 (2026-11-06): "A player can normally play one land during their turn; however,
// continuous effects may increase this number." CR 305.2a: compare the number they can play
// with the number they have played. Forge: `Affected$ You | AdjustLandPlays$ 1`.
#[test]
fn an_additional_land_play_each_turn() {
    let mut sc = scenario(&[&explorer()]);
    sc.card(P0, Zone::Battlefield, "Test Explorer");
    let lands = main_phase(&mut sc, 3);
    let mut s = Script::new(sc.build());
    assert_eq!(players::land_plays(&s.st, P0), Some(2));
    play(&mut s, lands[0]);
    assert_eq!(lands_offered(&s).len(), 2, "a second land");
    play(&mut s, lands[1]);
    assert!(lands_offered(&s).is_empty(), "no third land");
    assert_eq!(s.st.player(P0).lands_played_this_turn, 2);
}

// Two such permanents: two additional lands. The opponent's doesn't count for P0, in a
// three-player game.
#[test]
fn additional_land_plays_add_up_and_are_the_controllers() {
    let mut sc = scenario_n(3, &[&explorer()]);
    sc.cards(P0, Zone::Battlefield, &["Test Explorer", "Test Explorer"]);
    sc.card(P1, Zone::Battlefield, "Test Explorer");
    let lands = main_phase(&mut sc, 4);
    let mut s = Script::new(sc.build());
    assert_eq!(players::land_plays(&s.st, P0), Some(3));
    assert_eq!(players::land_plays(&s.st, P1), Some(2));
    assert_eq!(players::land_plays(&s.st, P2), Some(1));
    for &l in &lands[..3] {
        play(&mut s, l);
    }
    assert!(lands_offered(&s).is_empty());
}

// CR 305.2b: "A player can't play a land … if the number of lands the player can play this
// turn is equal to or less than the number of lands they have already played this turn."
// The extra play is gone when the permanent leaves, or loses its abilities (CLAUDE.md
// invariant 4).
#[test]
fn the_additional_land_play_ends_with_the_permanent_or_its_abilities() {
    let mut sc = scenario(&[&explorer()]);
    sc.card(P0, Zone::Battlefield, "Test Explorer");
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Recall");
    let lands = main_phase(&mut sc, 2);
    let mut s = Script::new(sc.build());
    play(&mut s, lands[0]);
    s.cast("Test Recall")
        .target("Test Explorer")
        .pay()
        .pass_all();
    assert_eq!(players::land_plays(&s.st, P0), Some(1));
    assert!(lands_offered(&s).is_empty(), "one land played, one allowed");

    let mut sc = scenario(&[&explorer(), &hush()]);
    sc.card(P0, Zone::Battlefield, "Test Explorer");
    sc.card(P1, Zone::Battlefield, "Test Hush");
    main_phase(&mut sc, 2);
    let s = Script::new(sc.build());
    assert_eq!(players::land_plays(&s.st, P0), Some(1));
}

// A one-shot "you may play an additional land this turn" (Forge: `SP$ Effect |
// StaticAbilities$ X` with `X:Mode$ Continuous | Affected$ You | AdjustLandPlays$ 1`): a
// rule that lasts until the turn ends (CR 611.2c, 514.2). Cast after the land drop, it still
// allows one more (CR 305.2a compares the counts).
#[test]
fn an_additional_land_this_turn() {
    let wander = "Name:Test Wandering\n\
        ManaCost:G\n\
        Types:Sorcery\n\
        A:SP$ Effect | StaticAbilities$ Extra | SubAbility$ DBDraw | SpellDescription$ Text.\n\
        SVar:Extra:Mode$ Continuous | Affected$ You | AdjustLandPlays$ 1 | Description$ Text.\n\
        SVar:DBDraw:DB$ Draw | Defined$ You | NumCards$ 1\n\
        Oracle:You may play an additional land this turn. Draw a card.";
    let mut sc = scenario(&[wander]);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Wandering");
    let lands = main_phase(&mut sc, 2);
    let mut s = Script::new(sc.build());
    play(&mut s, lands[0]);
    assert!(lands_offered(&s).is_empty());
    s.cast("Test Wandering").pay().pass_all();
    assert_eq!(players::land_plays(&s.st, P0), Some(2));
    assert_eq!(players::land_plays(&s.st, P1), Some(1));
    assert!(!lands_offered(&s).is_empty(), "one more land this turn");
    // The next turn: one land again.
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert_eq!(players::land_plays(&s.st, P0), Some(1));
}

// ---------------------------------------------------------------------------------------
// Hexproof and shroud on players

fn shock_options(s: &mut Script) {
    s.cast("Test Shock");
}

// CR 702.11c (2026-11-06): "'Hexproof' on a player means 'You can't be the target of spells
// or abilities your opponents control.'" Forge: `Affected$ You | AddKeyword$ Hexproof`.
#[test]
fn a_player_with_hexproof_cant_be_targeted_by_opponents() {
    let ward = artifact(
        "Test Veil",
        "S:Mode$ Continuous | Affected$ You | AddKeyword$ Hexproof | Description$ Text.",
    );
    let mut sc = scenario_n(3, &[&ward]);
    sc.card(P0, Zone::Battlefield, "Test Veil");
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Test Shock");
    sc.turn(P1, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    assert!(players::has_hexproof(&s.st, P0));
    shock_options(&mut s);
    assert!(!can_target_player(&s, P0), "an opponent's spell");
    assert!(can_target_player(&s, P1));
    assert!(can_target_player(&s, P2));

    // Its controller may target themselves.
    let mut sc = scenario(&[&ward]);
    sc.card(P0, Zone::Battlefield, "Test Veil");
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    shock_options(&mut s);
    assert!(can_target_player(&s, P0));
}

// CR 702.18a: "Shroud" means "This permanent or player can't be the target of spells or
// abilities." Not even its own.
#[test]
fn a_player_with_shroud_cant_be_targeted_at_all() {
    let mask = artifact(
        "Test Mask",
        "S:Mode$ Continuous | Affected$ You | AddKeyword$ Shroud | Description$ Text.",
    );
    let mut sc = scenario(&[&mask]);
    sc.card(P0, Zone::Battlefield, "Test Mask");
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    shock_options(&mut s);
    assert!(!can_target_player(&s, P0));
    assert!(can_target_player(&s, P1));
}

// "You and other creatures you control have hexproof" (Forge: `Affected$ You,
// Creature.YouCtrl+Other | AddKeyword$ Hexproof`): a rule about the player and a static
// ability about the creatures. It ends when the source loses its abilities.
#[test]
fn you_and_your_creatures_have_hexproof() {
    let guardian = creature(
        "Test Guardian",
        "3 G",
        "Angel",
        "3/4",
        "S:Mode$ Continuous | Affected$ You,Creature.YouCtrl+Other | AddKeyword$ Hexproof | Description$ Text.",
    );
    let mut sc = scenario(&[&guardian, &hush()]);
    sc.card(P0, Zone::Battlefield, "Test Guardian");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Test Shock");
    sc.turn(P1, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    shock_options(&mut s);
    assert!(!can_target_player(&s, P0));
    assert!(
        !s.has(|_, o| matches!(o, OptionDesc::Target { target: Target::Object(t) } if *t == golem))
    );
    assert!(s.has(|s, o| matches!(o, OptionDesc::Target { target: Target::Object(t) } if s.name(*t) == "Test Guardian")));

    let mut sc = scenario(&[&guardian, &hush()]);
    sc.card(P0, Zone::Battlefield, "Test Guardian");
    sc.card(P1, Zone::Battlefield, "Test Hush");
    let s = Script::new(sc.build());
    assert!(!players::has_hexproof(&s.st, P0));
}

// ---------------------------------------------------------------------------------------
// Playing lands and casting spells from other zones

fn crucible() -> String {
    artifact(
        "Test Crucible",
        "S:Mode$ Continuous | Affected$ Land.YouOwn | MayPlay$ True | AffectedZone$ Graveyard | Description$ Text.",
    )
}

// CR 601.3 / 116.2a (2026-11-06): "To play a land, a player puts that land onto the
// battlefield from the zone it was in (usually that player's hand)." A rule may let a
// player play lands from their graveyard; it is still the turn's land play (CR 305.2a).
// Forge: `Affected$ Land.YouOwn | MayPlay$ True | AffectedZone$ Graveyard`.
#[test]
fn lands_from_your_graveyard_are_your_land_play() {
    let mut sc = scenario_n(3, &[&crucible()]);
    sc.card(P0, Zone::Battlefield, "Test Crucible");
    let mine = sc.card(P0, Zone::Graveyard, "Forest");
    let theirs = sc.card(P1, Zone::Graveyard, "Forest");
    let in_hand = main_phase(&mut sc, 1);
    let mut s = Script::new(sc.build());
    let offered = lands_offered(&s);
    assert!(offered.contains(&mine));
    assert!(offered.contains(&in_hand[0]));
    assert!(!offered.contains(&theirs), "only your own graveyard");
    play(&mut s, mine);
    let on = s.find(P0, Zone::Battlefield, "Forest");
    assert!(on.is_some());
    assert_eq!(s.st.player(P0).lands_played_this_turn, 1);
    assert!(lands_offered(&s).is_empty(), "that was the land play");
}

// A permission to play from a zone doesn't change when: not with a spell on the stack,
// not in another player's turn; and not once the permanent is gone.
#[test]
fn a_land_from_the_graveyard_keeps_the_timing_of_a_land_play() {
    let mut sc = scenario(&[&crucible()]);
    sc.card(P0, Zone::Battlefield, "Test Crucible");
    let mine = sc.card(P0, Zone::Graveyard, "Forest");
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    sc.turn(P0, Step::Upkeep);
    let mut s = Script::new(sc.build());
    assert!(!lands_offered(&s).contains(&mine), "not in the upkeep");
    drive_until(&mut s, |s| s.st.turn.step == Step::PrecombatMain);
    assert!(lands_offered(&s).contains(&mine));
    s.cast("Test Shock").target_player(P1).pay();
    drive_until(&mut s, |s| s.kind() == ChoiceKind::Priority);
    assert!(
        !lands_offered(&s).contains(&mine),
        "not with a spell on the stack"
    );

    // P1's turn: P1 has no such rule, P0 can't play lands.
    let mut sc = scenario(&[&crucible()]);
    sc.card(P0, Zone::Battlefield, "Test Crucible");
    sc.card(P1, Zone::Graveyard, "Forest");
    sc.turn(P1, Step::PrecombatMain);
    let s = Script::new(sc.build());
    assert!(lands_offered(&s).is_empty());
}

fn courser() -> String {
    creature(
        "Test Courser",
        "1 G G",
        "Centaur",
        "2/4",
        "S:Mode$ Continuous | Affected$ Card.TopLibrary+YouCtrl | AffectedZone$ Library | MayLookAt$ Player | Description$ Text.\n\
         S:Mode$ Continuous | Affected$ Land.TopLibrary+YouCtrl | AffectedZone$ Library | MayPlay$ True | Description$ Text.",
    )
}

// CR 401.5: "Some effects tell a player to play with the top card of their library
// revealed". Every player sees it in their view; a land on top may be played (CR 116.2a).
// Forge: `Affected$ Card.TopLibrary+YouCtrl | AffectedZone$ Library | MayLookAt$ Player`
// and `Affected$ Land.TopLibrary+YouCtrl | … | MayPlay$ True`.
#[test]
fn play_with_the_top_card_revealed_and_lands_from_it() {
    let mut sc = scenario_n(3, &[&courser()]);
    sc.card(P0, Zone::Battlefield, "Test Courser");
    main_phase(&mut sc, 0);
    let mut s = Script::new(sc.build());
    let top = players::library_top(&s.st, P0).unwrap();
    for viewer in [P0, P1, P2] {
        let v = engine_core::player_view(&s.st, viewer);
        assert_eq!(v.players[0].library_top.as_ref().map(|o| o.id), Some(top));
        // Nobody sees the top of another library.
        assert!(v.players[1].library_top.is_none());
    }
    assert_eq!(lands_offered(&s), vec![top]);
    play(&mut s, top);
    assert!(s.find(P0, Zone::Battlefield, "Forest").is_some());
    assert_eq!(s.st.zones.list(Zone::Library, P0).len(), 19);
    assert!(lands_offered(&s).is_empty(), "the land play is used");
    // The new top card is revealed too.
    let new_top = players::library_top(&s.st, P0).unwrap();
    let v = engine_core::player_view(&s.st, P1);
    assert_eq!(
        v.players[0].library_top.as_ref().map(|o| o.id),
        Some(new_top)
    );
}

// A creature on top isn't a land: the rule doesn't let it be cast.
#[test]
fn only_what_the_rule_names_may_be_played_from_the_top() {
    let mut sc = scenario(&[&courser()]);
    sc.card(P0, Zone::Battlefield, "Test Courser");
    sc.cards(P0, Zone::Battlefield, &["Forest"; 3]);
    let elf = sc.card(P0, Zone::Library, "Test Elf");
    main_phase(&mut sc, 0);
    let s = Script::new(sc.build());
    assert_eq!(players::library_top(&s.st, P0), Some(elf));
    assert!(lands_offered(&s).is_empty());
    assert!(!can_cast_card(&s, elf));
}

// "You may look at the top card of your library any time. You may cast creature spells
// from the top of your library." Only its owner sees it (CR 401.5); the spell is cast as
// usual, with its timing and cost (CR 601.3).
#[test]
fn look_at_the_top_card_and_cast_creatures_from_it() {
    let augur = creature(
        "Test Augur",
        "1 G G",
        "Elf Druid",
        "2/3",
        "S:Mode$ Continuous | Affected$ Card.TopLibrary+YouCtrl | AffectedZone$ Library | MayLookAt$ You | Description$ Text.\n\
         S:Mode$ Continuous | Affected$ Creature.TopLibrary+YouCtrl+nonLand | AffectedZone$ Library | MayPlay$ True | Description$ Text.",
    );
    let mut sc = scenario(&[&augur]);
    sc.card(P0, Zone::Battlefield, "Test Augur");
    sc.card(P0, Zone::Battlefield, "Forest");
    let elf = sc.card(P0, Zone::Library, "Test Elf");
    main_phase(&mut sc, 0);
    let mut s = Script::new(sc.build());
    let mine = engine_core::player_view(&s.st, P0);
    assert_eq!(
        mine.players[0].library_top.as_ref().map(|o| o.id),
        Some(elf)
    );
    let theirs = engine_core::player_view(&s.st, P1);
    assert!(
        theirs.players[0].library_top.is_none(),
        "only its owner looks at it"
    );
    assert!(can_cast_card(&s, elf));
    s.pick(
        "cast the elf",
        |_, o| matches!(o, OptionDesc::CastSpell { card } if *card == elf),
    )
    .pay()
    .pass_all();
    assert!(s.find(P0, Zone::Battlefield, "Test Elf").is_some());
    // The Forest now on top is no creature.
    let top = players::library_top(&s.st, P0).unwrap();
    assert!(!can_cast_card(&s, top));
    assert!(lands_offered(&s).is_empty(), "no rule about lands");
}

// "You may cast this card from your graveyard as long as you control a Zombie" (Forge:
// `Affected$ Card.Self | AffectedZone$ Graveyard | EffectZone$ Graveyard | MayPlay$ True |
// IsPresent$ Zombie.YouCtrl`): the card's own ability, working from the graveyard (CR
// 113.6f).
#[test]
fn cast_this_card_from_your_graveyard() {
    let crawler = creature(
        "Test Crawler",
        "B",
        "Zombie",
        "2/1",
        "S:Mode$ Continuous | Affected$ Card.Self | AffectedZone$ Graveyard | EffectZone$ Graveyard | MayPlay$ True | IsPresent$ Zombie.YouCtrl | Description$ Text.",
    );
    let mut sc = scenario(&[&crawler]);
    let gy = sc.card(P0, Zone::Graveyard, "Test Crawler");
    sc.card(P0, Zone::Battlefield, "Swamp");
    main_phase(&mut sc, 0);
    let s = Script::new(sc.build());
    assert!(!can_cast_card(&s, gy), "no Zombie");

    let mut sc = scenario(&[&crawler]);
    let gy = sc.card(P0, Zone::Graveyard, "Test Crawler");
    let other = sc.card(P1, Zone::Graveyard, "Test Crawler");
    sc.card(P0, Zone::Battlefield, "Swamp");
    sc.card(P0, Zone::Battlefield, "Test Crawler");
    sc.card(P1, Zone::Battlefield, "Test Crawler");
    main_phase(&mut sc, 0);
    let mut s = Script::new(sc.build());
    assert!(can_cast_card(&s, gy));
    assert!(!can_cast_card(&s, other), "another player's card");
    s.pick(
        "cast from the graveyard",
        |_, o| matches!(o, OptionDesc::CastSpell { card } if *card == gy),
    )
    .pay()
    .pass_all();
    assert_eq!(
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| s.st.object(id).owner == P0 && s.name(id) == "Test Crawler")
            .count(),
        2
    );
}

// ---------------------------------------------------------------------------------------
// What stays blocked

#[test]
fn forms_not_compiled_yet_are_named() {
    let base =
        |line: &str| format!("Name:Test Rule\nManaCost:2\nTypes:Artifact\n{line}\nOracle:Text.");
    let has = |line: &str, blocker: &str| {
        let b = forge_test_blockers(&base(line));
        assert!(b.iter().any(|x| x == blocker), "{line}: {b:?}");
    };
    // An alternative cost (CR 118.9) is not modelled.
    has(
        "S:Mode$ Continuous | Affected$ Card.nonLand+YouOwn | MayPlay$ True | MayPlayWithoutManaCost$ True | AffectedZone$ Hand | Description$ Text.",
        "param:static Continuous.MayPlayWithoutManaCost",
    );
    // Every player's graveyard.
    has(
        "S:Mode$ Continuous | Affected$ Creature | MayPlay$ True | AffectedZone$ Graveyard | Description$ Text.",
        "static:MayPlay from every player's zone",
    );
    // Once each turn.
    has(
        "S:Mode$ Continuous | Affected$ Land.YouOwn | MayPlay$ True | MayPlayLimit$ 1 | AffectedZone$ Graveyard | Description$ Text.",
        "param:static Continuous.MayPlayLimit",
    );
    // A hand size that is counted.
    has(
        "S:Mode$ Continuous | Affected$ You | SetMaxHandSize$ X | Description$ Text.\nSVar:X:Count$Valid Creature.YouCtrl",
        "static:SetMaxHandSize variable",
    );
    // A card in the graveyard whose ability works on the battlefield only.
    has(
        "S:Mode$ Continuous | Affected$ Card.Self | AffectedZone$ Graveyard | MayPlay$ True | Description$ Text.",
        "static:MayPlay Self from another zone",
    );
    // A player keyword that isn't hexproof or shroud.
    has(
        "S:Mode$ Continuous | Affected$ You | AddKeyword$ Protection from everything | Description$ Text.",
        "player keyword:Protection from everything",
    );
    // Players matched by a property.
    has(
        "S:Mode$ Continuous | Affected$ Player.Active | AdjustLandPlays$ 1 | Description$ Text.",
        "filter:player Player.Active",
    );
    // Cards an effect remembers (impulse draw).
    has(
        "S:Mode$ Continuous | Affected$ Card.IsRemembered | MayPlay$ True | AffectedZone$ Exile | Description$ Text.",
        "static:MayPlay remembered cards",
    );
}

// ---------------------------------------------------------------------------------------
// "As long as your devotion to red is less than five, this isn't a creature."

fn smith_god() -> String {
    "Name:Test Forge God\n\
     ManaCost:3 R\n\
     Types:Legendary Enchantment Creature God\n\
     PT:6/5\n\
     K:Indestructible\n\
     S:Mode$ Continuous | AffectedDefined$ Self | RemoveType$ Creature | CheckSVar$ X | SVarCompare$ LT5 | Description$ Text.\n\
     SVar:X:Count$Devotion.Red\n\
     Oracle:Indestructible\\nAs long as your devotion to red is less than five, this isn't a creature."
        .to_string()
}

// CR 700.5 (2026-11-06): devotion to red is the number of {R} among the mana costs of
// permanents that player controls; CR 613.1d: removing a card type is a layer-4 effect;
// CR 208.3: a noncreature permanent has no power or toughness. Forge: `AffectedDefined$
// Self | RemoveType$ Creature | CheckSVar$ X | SVarCompare$ LT5`, `Count$Devotion.Red`.
#[test]
fn a_god_is_a_creature_only_with_enough_devotion() {
    use card_model::CardType;
    use engine_core::layers::chars;
    let mut sc = scenario_n(3, &[&smith_god()]);
    let god = sc.card(P0, Zone::Battlefield, "Test Forge God");
    sc.card(P0, Zone::Battlefield, "Test Giant"); // {3}{R}{R}
    sc.cards(P1, Zone::Battlefield, &["Test Giant", "Test Giant"]);
    let s = Script::new(sc.build());
    let c = chars(&s.st, god);
    assert!(
        !c.is(CardType::Creature),
        "devotion 3: the opponent's red doesn't count"
    );
    assert!(c.is(CardType::Enchantment));
    assert!(c.has_subtype("God"), "still a God");
    assert_eq!((c.power, c.toughness), (None, None));

    let mut sc = scenario_n(3, &[&smith_god()]);
    let god = sc.card(P0, Zone::Battlefield, "Test Forge God");
    sc.cards(P0, Zone::Battlefield, &["Test Giant", "Test Giant"]);
    let s = Script::new(sc.build());
    let c = chars(&s.st, god);
    assert!(c.is(CardType::Creature), "devotion 5");
    assert_eq!((c.power, c.toughness), (Some(6), Some(5)));
}

// CR 506.4: an attacking creature that stops being a creature is removed from combat.
#[test]
fn a_god_that_stops_being_a_creature_leaves_combat() {
    let mut sc = scenario(&[&smith_god()]);
    let god = sc.card(P0, Zone::Battlefield, "Test Forge God");
    sc.cards(P0, Zone::Battlefield, &["Test Giant", "Test Giant"]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Recall");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    // Attack with the God alone.
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    while s.next.is_some() && s.kind() == ChoiceKind::DeclareAttackers {
        let a = match s.options().first() {
            Some(OptionDesc::Attack { attacker, .. }) => *attacker,
            other => panic!("{other:?}"),
        };
        s.respond(Response::Pick(if a == god { vec![0] } else { vec![] }));
    }
    assert!(engine_core::layers::combat_role(&s.st, god).0);
    drive_until(&mut s, |s| {
        s.kind() == ChoiceKind::Priority && s.who() == P0
    });
    s.cast("Test Recall").target("Test Giant").pay().pass_all();
    assert!(
        !engine_core::layers::combat_role(&s.st, god).0,
        "removed from combat"
    );
    let life = s.st.player(P1).life;
    drive_until(&mut s, |s| s.st.turn.step == Step::PostcombatMain);
    assert_eq!(s.st.player(P1).life, life, "it deals no combat damage");
}

// A line that only copies hints for Forge's AI to what it affects says nothing about the
// rules: the card compiles without it.
#[test]
fn a_line_of_ai_hints_only_is_no_ability() {
    let card = "Name:Test Hinted\nManaCost:1 W\nTypes:Creature Human\nPT:2/2\n\
        S:Mode$ Continuous | Affected$ Creature.YouCtrl | AddSVar$ AE\n\
        SVar:AE:SVar:HasAttackEffect:TRUE\nOracle:Text.";
    assert_eq!(forge_test_blockers(card), [] as [&str; 0]);
    let cat = forge_test_catalog(&[card], &[]);
    let def = cat.get(cat.find("Test Hinted").unwrap());
    assert!(def.abilities.is_empty());
    // Anything else on such a line is named.
    let other = card.replace("AddSVar$ AE", "AddSVar$ AE | CanBlockAny$ True");
    assert!(
        forge_test_blockers(&other)
            .iter()
            .any(|b| b == "param:static Continuous.CanBlockAny")
    );
}

// ---------------------------------------------------------------------------------------
// "As long as this card is in your graveyard and you control a Mountain, creatures you
// control have haste."

/// A land with the type Mountain (so that the word is a known subtype to the compiler).
const PEAK: &str = "Name:Test Peak\nTypes:Land Mountain\nOracle:";

fn fury() -> String {
    creature(
        "Test Fury",
        "3 R",
        "Incarnation",
        "2/2",
        "S:Mode$ Continuous | Affected$ Creature.YouCtrl | EffectZone$ Graveyard | AddKeyword$ Haste | IsPresent$ Mountain.YouCtrl | Description$ Text.",
    )
}

// CR 113.6b (2026-11-06): "An ability that states which zones it functions in functions
// only from those zones." Forge: `EffectZone$ Graveyard` on a `Mode$ Continuous` line.
// "You" is the card's owner.
#[test]
fn a_static_ability_that_works_from_the_graveyard() {
    use card_model::Keyword;
    use engine_core::layers::chars;
    let mut sc = scenario_n(3, &[&fury(), PEAK]);
    sc.card(P0, Zone::Graveyard, "Test Fury");
    sc.card(P0, Zone::Battlefield, "Mountain");
    let mine = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P1, Zone::Battlefield, "Mountain");
    let s = Script::new(sc.build());
    assert!(chars(&s.st, mine).has(Keyword::Haste));
    assert!(
        !chars(&s.st, theirs).has(Keyword::Haste),
        "the owner's creatures only"
    );

    // Without a Mountain: nothing.
    let mut sc = scenario(&[&fury(), PEAK]);
    sc.card(P0, Zone::Graveyard, "Test Fury");
    let mine = sc.card(P0, Zone::Battlefield, "Test Golem");
    let s = Script::new(sc.build());
    assert!(!chars(&s.st, mine).has(Keyword::Haste));

    // On the battlefield the ability doesn't work, not even for the card itself.
    let mut sc = scenario(&[&fury(), PEAK]);
    let fury = sc.card(P0, Zone::Battlefield, "Test Fury");
    sc.card(P0, Zone::Battlefield, "Mountain");
    let mine = sc.card(P0, Zone::Battlefield, "Test Golem");
    let s = Script::new(sc.build());
    assert!(!chars(&s.st, mine).has(Keyword::Haste));
    assert!(!chars(&s.st, fury).has(Keyword::Haste));
}

// Once the card leaves the graveyard, the effect is gone.
#[test]
fn the_graveyard_ability_ends_when_the_card_leaves_the_graveyard() {
    use card_model::Keyword;
    use engine_core::layers::chars;
    let exile = "Name:Test Unearthing\nManaCost:B\nTypes:Instant\n\
        A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Exile | ValidTgts$ Card | TgtPrompt$ Text | SpellDescription$ Text.\n\
        Oracle:Exile target card from a graveyard.";
    let mut sc = scenario(&[&fury(), exile, PEAK]);
    sc.card(P0, Zone::Graveyard, "Test Fury");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Swamp"]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Unearthing");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    assert!(chars(&s.st, mine).has(Keyword::Haste));
    s.cast("Test Unearthing")
        .target("Test Fury")
        .pay()
        .pass_all();
    assert!(s.find(P0, Zone::Exile, "Test Fury").is_some());
    assert!(!chars(&s.st, mine).has(Keyword::Haste));
}

// CR 205.3i, 305.7 (2026-11-06): "every basic land type in addition to their other types"
// (Forge: `AddType$ AllBasicLandType`); each basic land type gives its mana ability
// (CR 305.6).
#[test]
fn lands_are_every_basic_land_type() {
    use engine_core::mana::ManaType;
    let omen = "Name:Test Omen\nManaCost:1 G\nTypes:Enchantment\n\
        S:Mode$ Continuous | Affected$ Land.YouCtrl | AddType$ AllBasicLandType | Description$ Text.\n\
        Oracle:Text.";
    let mut sc = scenario(&[omen, PEAK]);
    sc.card(P0, Zone::Battlefield, "Test Omen");
    let peak = sc.card(P0, Zone::Battlefield, "Test Peak");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Peak");
    sc.turn(P0, Step::PrecombatMain);
    let s = Script::new(sc.build());
    let colors = |id: ObjectId| -> Vec<ManaType> {
        s.options()
            .iter()
            .filter_map(|o| match o {
                OptionDesc::TapForMana { source, mana, .. } if *source == id => Some(*mana),
                _ => None,
            })
            .collect()
    };
    let mine = colors(peak);
    for m in [
        ManaType::White,
        ManaType::Blue,
        ManaType::Black,
        ManaType::Red,
        ManaType::Green,
    ] {
        assert!(mine.contains(&m), "{m:?}: {mine:?}");
    }
    let c = engine_core::layers::chars(&s.st, theirs);
    assert!(c.has_subtype("Mountain") && !c.has_subtype("Island"));
}

// ---------------------------------------------------------------------------------------
// With D-067 (how a permanent enters) and D-068 (its controller, that player)

/// A land with its own replacement effect `ReplaceWith$ DBTap` with these parameters.
fn tap_land(name: &str, types: &str, tap: &str) -> String {
    format!(
        "Name:{name}\n\
         ManaCost:no cost\n\
         Types:{types}\n\
         R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ DBTap | ReplacementResult$ Updated | Description$ Text.\n\
         SVar:DBTap:DB$ Tap | ETB$ True | Defined$ Self{tap}\n\
         Oracle:Text."
    )
}

fn shock_land() -> String {
    tap_land(
        "Test Shoal",
        "Land Island Swamp",
        " | UnlessCost$ PayLife<2> | UnlessPayer$ You",
    )
}

// CR 116.2a, 614.12a (2026-11-06): a land played from the graveyard enters the battlefield
// like one played from the hand: "as this enters, you may pay 2 life" is still asked.
#[test]
fn a_land_from_the_graveyard_is_asked_as_it_enters() {
    let shoal = shock_land();
    let mut sc = scenario(&[&crucible(), &shoal]);
    sc.card(P0, Zone::Battlefield, "Test Crucible");
    let land = sc.card(P0, Zone::Graveyard, "Test Shoal");
    main_phase(&mut sc, 0);
    let mut s = Script::new(sc.build());
    play(&mut s, land);
    assert!(
        matches!(s.kind(), ChoiceKind::AsEnters { .. }),
        "asked as it enters: {:?}",
        s.kind()
    );
    s.pick("pay", |_, o| *o == OptionDesc::Accept);
    assert_eq!(s.st.player(P0).life, 38);
    let on = s
        .find(P0, Zone::Battlefield, "Test Shoal")
        .expect("entered");
    assert!(!s.st.object(on).tapped);
    assert_eq!(s.st.player(P0).lands_played_this_turn, 1);
}

// The same from the top of the library; declining, it enters tapped.
#[test]
fn a_land_from_the_top_of_the_library_is_asked_as_it_enters() {
    let shoal = shock_land();
    let mut sc = scenario(&[&courser(), &shoal]);
    sc.card(P0, Zone::Battlefield, "Test Courser");
    let land = sc.card(P0, Zone::Library, "Test Shoal");
    main_phase(&mut sc, 0);
    let mut s = Script::new(sc.build());
    assert_eq!(players::library_top(&s.st, P0), Some(land));
    play(&mut s, land);
    assert!(matches!(s.kind(), ChoiceKind::AsEnters { .. }));
    s.pick("don't pay", |_, o| *o == OptionDesc::Decline);
    assert_eq!(s.st.player(P0).life, 40);
    let on = s
        .find(P0, Zone::Battlefield, "Test Shoal")
        .expect("entered");
    assert!(s.st.object(on).tapped);
}

// CR 305.2: an additional land play is a land play like the first, lands that enter tapped
// included.
#[test]
fn additional_land_plays_with_lands_that_enter_tapped() {
    let slow = tap_land("Test Mire", "Land Swamp", "");
    let mut sc = scenario(&[&explorer(), &slow]);
    sc.card(P0, Zone::Battlefield, "Test Explorer");
    sc.cards(P0, Zone::Hand, &["Test Mire", "Test Mire", "Test Mire"]);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    for _ in 0..2 {
        let next = lands_offered(&s)[0];
        play(&mut s, next);
    }
    assert!(lands_offered(&s).is_empty(), "two land plays");
    let mires: Vec<_> =
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| s.name(id) == "Test Mire")
            .collect();
    assert_eq!(mires.len(), 2);
    assert!(mires.iter().all(|&&id| s.st.object(id).tapped));
}

// CR 115.10a (2026-11-06): a spell or ability targets only what it says "target" about.
// "Destroy target creature. Its controller loses 2 life" doesn't target the player, so a
// player with hexproof or shroud still loses the life (Forge: `Defined$ TargetedController`).
#[test]
fn hexproof_and_shroud_on_a_player_dont_stop_its_controller_effects() {
    let ruin = "Name:Test Ruin\nManaCost:B\nTypes:Sorcery\n\
        A:SP$ Destroy | ValidTgts$ Creature | TgtPrompt$ Text | SubAbility$ DBLose | SpellDescription$ Text.\n\
        SVar:DBLose:DB$ LoseLife | Defined$ TargetedController | LifeAmount$ 2\n\
        Oracle:Destroy target creature. Its controller loses 2 life.";
    for keyword in ["Hexproof", "Shroud"] {
        let ward = artifact(
            "Test Veil",
            &format!(
                "S:Mode$ Continuous | Affected$ You | AddKeyword$ {keyword} | Description$ Text."
            ),
        );
        let mut sc = scenario(&[&ward, ruin]);
        sc.card(P1, Zone::Battlefield, "Test Veil");
        let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
        sc.card(P0, Zone::Battlefield, "Swamp");
        sc.card(P0, Zone::Hand, "Test Ruin");
        sc.turn(P0, Step::PrecombatMain);
        let mut s = Script::new(sc.build());
        s.cast("Test Ruin");
        s.target_object(golem).pay().pass_all();
        assert!(!s.st.objects.contains_key(&golem), "{keyword}: destroyed");
        assert_eq!(
            s.st.player(P1).life,
            38,
            "{keyword}: its controller loses 2"
        );
    }
}
