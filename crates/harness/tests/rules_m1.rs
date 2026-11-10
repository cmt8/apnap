//! Atomic rules tests for M1, each written from the verbatim CR text in docs/rules-notes/
//! (2026-11-06) and named after the rule it checks. Scenarios are built with
//! `engine_core::testing::Scenario` and driven by `harness::script::Script`.

use engine_core::events::Event;
use engine_core::mana::ManaType;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, OptionDesc, PlayerId, Primitive, Response};
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);

fn scenario(players: u8) -> Scenario {
    let config = if players == 2 {
        GameConfig::constructed_1v1()
    } else {
        GameConfig::free_for_all(players)
    };
    let mut sc = Scenario::new(config, harness::catalog(), 7);
    for p in 0..players {
        sc.library(PlayerId(p), "Forest", 10);
    }
    sc
}

fn taps(s: &Script, name: &str, mana: ManaType) -> bool {
    s.has(|s, o| matches!(o, OptionDesc::TapForMana { source, mana: m, .. } if s.name(*source) == name && *m == mana))
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

/// Answer whatever comes up with the minimal legal response until `done` holds.
fn drive_until(s: &mut Script, done: impl Fn(&Script) -> bool) {
    for _ in 0..10_000 {
        if done(s) || s.next.is_none() {
            return;
        }
        let r = match &s.decision().prim {
            Primitive::PickN { min, .. } => Response::Pick((0..*min).collect()),
            Primitive::PickNumber { min, .. } => Response::Number(*min),
            Primitive::Allocate { total, per_max, .. } => {
                let mut v = vec![0; per_max.len()];
                v[0] = *total;
                Response::Allocate(v)
            }
            Primitive::Order => Response::Order((0..s.options().len()).collect()),
        };
        s.respond(r);
    }
    panic!("drive_until did not converge");
}

// --- CR 305.6 / 605: mana abilities ---------------------------------------------------

/// CR 305.6: "An object with the land card type and a basic land type has the intrinsic
/// ability '{T}: Add [mana symbol]' … For Mountains, {R}; and for Forests, {G}."
#[test]
fn cr_305_6_basic_land_mana() {
    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Mountain"]);
    let s = Script::new(sc.build());
    assert!(taps(&s, "Forest", ManaType::Green));
    assert!(taps(&s, "Mountain", ManaType::Red));
    assert!(!taps(&s, "Forest", ManaType::Red));
    assert!(!taps(&s, "Mountain", ManaType::Green));
}

/// CR 305.6: a land with two basic land types has both intrinsic mana abilities.
#[test]
fn cr_305_6_dual_land_either_color() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Dual");
    let s = Script::new(sc.build());
    assert!(taps(&s, "Test Dual", ManaType::Red));
    assert!(taps(&s, "Test Dual", ManaType::Green));
}

/// CR 602.5a: "A creature's activated ability with the tap symbol … can't be activated
/// unless the creature has been under its controller's control since the start of their
/// most recent turn." Applies to mana abilities of creatures, not to artifacts.
#[test]
fn cr_602_5a_summoning_sick_mana_creature() {
    let mut sc = scenario(2);
    let elf = sc.card(P0, Zone::Battlefield, "Test Elf");
    let pinger = sc.card(P0, Zone::Battlefield, "Test Pinger");
    let prism = sc.card(P0, Zone::Battlefield, "Test Prism");
    sc.sick(elf);
    sc.sick(pinger);
    sc.sick(prism);
    let s = Script::new(sc.build());
    assert!(
        !taps(&s, "Test Elf", ManaType::Green),
        "sick creature mana ability"
    );
    assert!(
        !s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == pinger)),
        "sick creature {{T}} ability"
    );
    assert!(
        taps(&s, "Test Prism", ManaType::Blue),
        "artifacts aren't affected"
    );

    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Test Elf", "Test Pinger"]);
    let s = Script::new(sc.build());
    assert!(taps(&s, "Test Elf", ManaType::Green));
    assert!(s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { .. })));
}

/// CR 605.3b: "An activated mana ability doesn't go on the stack … it resolves immediately
/// after it is activated." CR 117.3c: the player keeps priority.
#[test]
fn cr_605_3b_mana_ability_any_color() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Prism");
    let mut s = Script::new(sc.build());
    for t in [
        ManaType::White,
        ManaType::Blue,
        ManaType::Black,
        ManaType::Red,
        ManaType::Green,
    ] {
        assert!(taps(&s, "Test Prism", t), "{t:?}");
    }
    assert!(!taps(&s, "Test Prism", ManaType::Colorless));
    s.tap("Test Prism", ManaType::Blue);
    assert!(s.st.stack.is_empty());
    assert_eq!(s.st.player(P0).mana_pool.amounts[ManaType::Blue.index()], 1);
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Priority));
}

// --- CR 602 / 113.7a: activated abilities -----------------------------------------------

/// CR 602.2a: "That ability is created on the stack as an object that's not a card."
#[test]
fn cr_602_2_activated_ability_uses_stack() {
    let mut sc = scenario(2);
    let pinger = sc.card(P0, Zone::Battlefield, "Test Pinger");
    let mut s = Script::new(sc.build());
    s.activate("Test Pinger").target_player(P1);
    assert_eq!(s.st.stack.len(), 1);
    let ability = s.st.stack[0].id;
    assert!(
        !s.st.objects.contains_key(&ability),
        "an ability is not a card object"
    );
    assert!(s.st.object(pinger).tapped, "{{T}} cost paid");
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Priority), "CR 117.3c");
    s.pass_all();
    assert_eq!(life(&s, P1), 19);
    assert!(s.st.stack.is_empty());
    assert_eq!(s.count(|e| matches!(e, Event::AbilityRemoved { .. })), 1);
}

/// CR 113.7a: "Once activated or triggered, an ability exists on the stack independently
/// of its source. Destruction or removal of the source after that time won't affect the
/// ability."
#[test]
fn cr_113_7a_ability_resolves_after_source_dies() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Pinger");
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Test Shock");
    let mut s = Script::new(sc.build());
    s.activate("Test Pinger").target_player(P1).pass();
    s.cast("Test Shock").target("Test Pinger").pay();
    s.pass_all(); // Shock resolves; the Pinger dies to SBAs before anyone gets priority
    assert!(s.find(P0, Zone::Graveyard, "Test Pinger").is_some());
    assert_eq!(s.st.stack.len(), 1, "the ability is still on the stack");
    s.pass_all();
    assert_eq!(life(&s, P1), 19, "the ability resolved without its source");
}

// --- Keywords ----------------------------------------------------------------------------

/// CR 702.10b: "If a creature has haste, it can attack even if it hasn't been controlled
/// by its controller continuously since their most recent turn began."
#[test]
fn cr_702_10_haste_attacks_immediately() {
    let mut sc = scenario(2);
    let charger = sc.card(P0, Zone::Battlefield, "Test Charger");
    let bruiser = sc.card(P0, Zone::Battlefield, "Test Bruiser");
    sc.sick(charger);
    sc.sick(bruiser);
    let mut s = Script::new(sc.build());
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    assert!(
        s.options()
            .iter()
            .all(|o| matches!(o, OptionDesc::Attack { attacker, .. } if *attacker == charger))
    );
    s.respond(Response::Pick(vec![]));
    assert_ne!(
        s.kind(),
        ChoiceKind::DeclareAttackers,
        "the sick Bruiser isn't asked"
    );
}

/// CR 702.8a: "'Flash' means 'You may play this card any time you could cast an instant.'"
#[test]
fn cr_702_8_flash_creature_on_opponents_turn() {
    let mut sc = scenario(2);
    sc.cards(P1, Zone::Battlefield, &["Forest", "Forest"]);
    sc.cards(P1, Zone::Hand, &["Test Quickling", "Test Bruiser"]);
    let mut s = Script::new(sc.build());
    s.pass();
    assert_eq!(s.who(), P1);
    assert!(s.can_cast("Test Quickling"));
    assert!(
        !s.can_cast("Test Bruiser"),
        "no flash: sorcery timing only (CR 117.1a)"
    );
    s.cast("Test Quickling").pay().pass_all();
    assert!(s.find(P1, Zone::Battlefield, "Test Quickling").is_some());
}

// --- CR 117: priority --------------------------------------------------------------------

/// CR 117.1a: "A player may cast a noninstant spell during their main phase any time they
/// have priority and the stack is empty."
#[test]
fn cr_117_1a_sorcery_timing() {
    let mut sc = scenario(2);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Mountain", "Mountain", "Mountain", "Mountain"],
    );
    sc.cards(P0, Zone::Hand, &["Test Riot", "Test Shock"]);
    sc.cards(P1, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    sc.card(P1, Zone::Hand, "Test Riot");
    let mut s = Script::new(sc.build());
    assert!(s.can_cast("Test Riot"));
    s.cast("Test Shock").target_player(P1).pay();
    assert!(!s.can_cast("Test Riot"), "stack not empty");
    s.pass();
    assert_eq!(s.who(), P1);
    assert!(!s.can_cast("Test Riot"), "not P1's main phase");
}

/// CR 117.3b: "The active player receives priority after a spell or ability (other than a
/// mana ability) resolves."
#[test]
fn cr_117_3b_active_player_gets_priority_after_resolution() {
    let mut sc = scenario(2);
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Test Shock");
    let mut s = Script::new(sc.build());
    s.pass();
    s.cast("Test Shock").target_player(P0).pay();
    s.pass_all();
    assert_eq!(life(&s, P0), 18);
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Priority));
}

/// CR 117.3c: "If a player has priority when they cast a spell … that player receives
/// priority afterward."
#[test]
fn cr_117_3c_caster_gets_priority_again() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    let mut s = Script::new(sc.build());
    s.cast("Test Shock").target_player(P1).pay();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Priority));
    assert_eq!(s.st.stack.len(), 1);
}

/// CR 117.4: "If all players pass in succession (that is, if all players pass without
/// taking any actions in between passing), the spell or ability on top of the stack
/// resolves or, if the stack is empty, the phase or step ends."
#[test]
fn cr_117_4_all_pass_in_succession() {
    let mut sc = scenario(2);
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Test Shock");
    let mut s = Script::new(sc.build());
    s.pass(); // P0 passes, then P1 acts: the passes don't add up
    s.cast("Test Shock").target_player(P0).pay();
    s.pass(); // P1
    assert_eq!(s.st.turn.step, Step::PrecombatMain);
    s.pass(); // P0: both passed in succession with Shock on the stack → it resolves
    assert_eq!(life(&s, P0), 18);
    assert_eq!(
        s.st.turn.step,
        Step::PrecombatMain,
        "resolving doesn't end the step"
    );
    s.pass_all(); // empty stack, both pass → the step ends
    assert_eq!(s.st.turn.step, Step::BeginningOfCombat);
}

/// CR 117.5: "Each time a player would get priority, the game first performs all
/// applicable state-based actions …"
#[test]
fn cr_117_5_sbas_before_priority() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    sc.card(P1, Zone::Battlefield, "Test Bruiser");
    let mut s = Script::new(sc.build());
    s.cast("Test Shock").target("Test Bruiser").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::Priority);
    assert!(s.find(P1, Zone::Battlefield, "Test Bruiser").is_none());
    assert!(s.find(P1, Zone::Graveyard, "Test Bruiser").is_some());
}

// --- CR 601: casting ---------------------------------------------------------------------

/// CR 107.3a: the controller "chooses and announces the value of X as part of casting the
/// spell". apnap bounds X by what can actually be paid (D-019).
#[test]
fn cr_107_3a_x_announced_and_paid() {
    let mut sc = scenario(2);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Mountain", "Forest", "Forest", "Forest"],
    );
    sc.card(P0, Zone::Hand, "Test Blaze");
    let mut s = Script::new(sc.build());
    s.cast("Test Blaze");
    assert_eq!(s.kind(), ChoiceKind::ChooseX);
    assert_eq!(s.decision().prim, Primitive::PickNumber { min: 0, max: 3 });
    s.x(2).target_player(P1).pay();
    let untapped =
        s.st.zones
            .battlefield
            .iter()
            .filter(|id| !s.st.object(**id).tapped)
            .count();
    assert_eq!(untapped, 1, "paid {{2}}{{R}}");
    s.pass_all();
    assert_eq!(life(&s, P1), 18);
}

/// CR 601.2h: the player pays the total cost "in any order" — with floating mana of two
/// types available for a generic cost, the player chooses.
#[test]
fn cr_601_2h_generic_payment_choice() {
    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Charm");
    let mut s = Script::new(sc.build());
    s.tap("Mountain", ManaType::Red);
    s.tap("Forest", ManaType::Green);
    s.tap("Forest", ManaType::Green);
    s.cast("Test Charm").mode(2);
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    assert!(matches!(
        s.decision().prim,
        Primitive::Allocate { total: 1, .. }
    ));
    let red = s
        .options()
        .iter()
        .position(|o| {
            matches!(
                o,
                OptionDesc::Mana {
                    mana: ManaType::Red
                }
            )
        })
        .unwrap();
    let mut alloc = vec![0; s.options().len()];
    alloc[red] = 1;
    s.respond(Response::Allocate(alloc));
    let pool = s.st.player(P0).mana_pool;
    assert_eq!(pool.amounts[ManaType::Green.index()], 1);
    assert_eq!(pool.total(), 1);
}

/// CR 700.2a: "If one of the modes would be illegal (due to an inability to choose legal
/// targets, for example), that mode can't be chosen."
#[test]
fn cr_700_2a_modes_without_legal_targets_unavailable() {
    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Charm");
    let mut s = Script::new(sc.build());
    s.cast("Test Charm");
    assert_eq!(s.kind(), ChoiceKind::ChooseModes);
    let modes: Vec<_> = s.options().to_vec();
    assert_eq!(
        modes,
        [OptionDesc::Mode { index: 1 }, OptionDesc::Mode { index: 2 }],
        "no creature on the battlefield: mode 0 is unavailable"
    );

    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Charm");
    sc.card(P1, Zone::Battlefield, "Test Sprout");
    let mut s = Script::new(sc.build());
    s.cast("Test Charm");
    assert_eq!(s.options().len(), 3);
    s.mode(0).target("Test Sprout").pay().pass_all();
    assert!(s.find(P1, Zone::Graveyard, "Test Sprout").is_some());
}

/// CR 115.5: "A spell or ability on the stack is an illegal target for itself."
#[test]
fn cr_115_5_spell_cannot_target_itself() {
    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Bruiser");
    sc.cards(P1, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.card(P1, Zone::Hand, "Test Snuff");
    let mut s = Script::new(sc.build());
    s.pass();
    assert!(
        !s.can_cast("Test Snuff"),
        "no spell to target; it can't target itself"
    );
    drive_until(&mut s, |s| s.who() == P0 && s.can_cast("Test Bruiser"));
}

// --- CR 608 / 701.6: resolution ------------------------------------------------------------

/// CR 608.2b: "If all its targets … are now illegal, the spell or ability doesn't resolve.
/// It's removed from the stack and, if it's a spell, put into its owner's graveyard."
#[test]
fn cr_608_2b_fizzles_when_all_targets_illegal() {
    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Shock", "Test Shock"]);
    sc.card(P1, Zone::Battlefield, "Test Sprout");
    let mut s = Script::new(sc.build());
    s.cast("Test Shock").target("Test Sprout").pay();
    s.cast("Test Shock").target("Test Sprout").pay();
    s.pass_all(); // the second Shock resolves; the Sprout dies
    assert!(s.find(P1, Zone::Graveyard, "Test Sprout").is_some());
    s.pass_all(); // the first Shock: its only target is gone
    assert_eq!(s.count(|e| matches!(e, Event::Fizzled { .. })), 1);
    assert_eq!(s.count(|e| matches!(e, Event::DamageDealt { .. })), 1);
    assert_eq!(
        s.st.zones.graveyards[0].len(),
        2,
        "both Shocks in the graveyard"
    );
    assert!(s.st.stack.is_empty());
}

/// CR 701.6a: "To counter a spell or ability means to cancel it, removing it from the
/// stack. It doesn't resolve and none of its effects occur. A countered spell is put into
/// its owner's graveyard."
#[test]
fn cr_701_6a_countered_spell_to_graveyard() {
    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Bruiser");
    sc.cards(P1, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.card(P1, Zone::Hand, "Test Snuff");
    let mut s = Script::new(sc.build());
    s.cast("Test Bruiser").pay().pass();
    s.cast("Test Snuff");
    let targets = s.options().len();
    assert_eq!(
        targets, 1,
        "CR 115.5: only the Bruiser, not the Snuff itself"
    );
    s.target("Test Bruiser").pay().pass_all();
    assert_eq!(s.count(|e| matches!(e, Event::Countered { .. })), 1);
    assert!(s.find(P0, Zone::Graveyard, "Test Bruiser").is_some());
    assert!(s.find(P1, Zone::Graveyard, "Test Snuff").is_some());
    assert!(s.st.stack.is_empty());
}

// --- CR 101.4: APNAP ---------------------------------------------------------------------

/// CR 101.4: "the active player … makes any choices required, then the next player in turn
/// order … followed by the remaining nonactive players in turn order. Then the actions
/// happen simultaneously."
#[test]
fn cr_101_4_each_player_discards_in_apnap_order() {
    let mut sc = scenario(4);
    sc.turn(P1, Step::PrecombatMain);
    sc.cards(P1, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    sc.card(P1, Zone::Hand, "Test Riot");
    for p in [P0, P1, P2, P3] {
        sc.cards(p, Zone::Hand, &["Forest", "Forest"]);
    }
    let mut s = Script::new(sc.build());
    assert_eq!(s.who(), P1);
    s.cast("Test Riot").pay().pass_all();
    let mut order = Vec::new();
    while s.kind() == ChoiceKind::Discard {
        assert_eq!(
            s.count(|e| matches!(e, Event::Discarded { .. })),
            0,
            "simultaneous"
        );
        order.push(s.who());
        s.respond(Response::Pick(vec![0]));
    }
    assert_eq!(order, [P1, P2, P3, P0]);
    assert_eq!(s.count(|e| matches!(e, Event::Discarded { .. })), 4);
}

/// CR 101.4 + 701.21a: each player sacrifices; a player with no creature has no choice.
#[test]
fn cr_101_4_each_player_sacrifices_simultaneously() {
    let mut sc = scenario(4);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest", "Test Sprout"]);
    sc.card(P0, Zone::Hand, "Test Edict");
    sc.cards(P1, Zone::Battlefield, &["Test Bruiser", "Test Grazer"]);
    sc.card(P3, Zone::Battlefield, "Test Spark");
    let mut s = Script::new(sc.build());
    s.cast("Test Edict").pay().pass_all();
    let mut order = Vec::new();
    while s.kind() == ChoiceKind::Sacrifice {
        assert_eq!(s.count(|e| matches!(e, Event::Sacrificed { .. })), 0);
        order.push(s.who());
        s.respond(Response::Pick(vec![0]));
    }
    assert_eq!(order, [P0, P1, P3], "P2 controls no creature");
    assert_eq!(s.count(|e| matches!(e, Event::Sacrificed { .. })), 3);
    assert_eq!(
        s.st.zones
            .battlefield
            .iter()
            .filter(|id| s.st.object(**id).owner == P1)
            .count(),
        1
    );
}

// --- CR 800.4 / 104.2a: leaving the game ---------------------------------------------------

/// CR 800.4a: "all objects … owned by that player leave the game … Then, if that player
/// controlled any objects on the stack not represented by cards, those objects cease to
/// exist." Priority then continues with the players still in the game.
#[test]
fn cr_800_4a_owned_objects_and_abilities_leave() {
    let mut sc = scenario(4);
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    sc.card(P2, Zone::Battlefield, "Test Pinger");
    sc.card(P2, Zone::Hand, "Forest");
    sc.life(P2, 2);
    let mut s = Script::new(sc.build());
    s.pass().pass(); // P0, P1
    assert_eq!(s.who(), P2);
    s.activate("Test Pinger").target_player(P0);
    s.pass().pass(); // P2, P3
    s.cast("Test Shock").target_player(P2).pay(); // P0 responds
    s.pass_all(); // Shock resolves: P2 at 0, loses at the next SBA check
    assert!(s.st.player(P2).has_lost);
    assert_eq!(
        s.count(|e| matches!(e, Event::PlayerLeftGame { player } if *player == P2)),
        1
    );
    assert!(
        s.st.objects.values().all(|o| o.owner != P2),
        "P2's cards left the game"
    );
    assert!(s.st.stack.is_empty(), "P2's ability ceased to exist");
    assert_eq!(life(&s, P0), 20, "the Pinger ability never resolved");
    assert_eq!((s.who(), s.st.players_in_game()), (P0, 3));
    s.pass_all();
    assert_eq!(
        s.st.turn.step,
        Step::BeginningOfCombat,
        "3 passes now end the step"
    );
}

/// CR 800.4j: "If a player leaves the game during their turn, that turn continues to its
/// completion without an active player. If the active player would receive priority,
/// instead the next player in turn order receives priority". CR 800.4k: their next turn
/// doesn't begin.
#[test]
fn cr_800_4j_turn_continues_without_active_player() {
    let mut sc = scenario(3);
    sc.turn(P1, Step::PrecombatMain);
    sc.life(P1, 2);
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    let mut s = Script::new(sc.build());
    s.pass().pass(); // P1, P2
    s.cast("Test Shock").target_player(P1).pay().pass_all();
    assert!(s.st.player(P1).has_lost);
    assert_eq!(s.st.turn.active, P1, "the turn continues");
    assert_eq!((s.who(), s.st.turn.step), (P2, Step::PrecombatMain));
    drive_until(&mut s, |s| s.st.turn.turn_number == 2);
    assert_eq!(
        s.st.turn.active, P2,
        "P1's next turn never begins; P2 is next"
    );
    assert!(
        !s.events
            .iter()
            .any(|e| matches!(e, Event::TurnBegan { active, .. } if *active == P1)),
        "no turn for P1 after leaving"
    );
}

/// CR 104.2a: in a two-player game, the remaining player wins immediately — nobody
/// "leaves" a game that has ended.
#[test]
fn cr_104_2a_two_player_loss_ends_game() {
    let mut sc = scenario(2);
    sc.life(P1, 2);
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    let mut s = Script::new(sc.build());
    s.cast("Test Shock").target_player(P1).pay().pass_all();
    assert!(s.next.is_none());
    assert_eq!(s.st.result.as_ref().unwrap().winner, Some(P0));
    assert_eq!(s.count(|e| matches!(e, Event::PlayerLeftGame { .. })), 0);
}
