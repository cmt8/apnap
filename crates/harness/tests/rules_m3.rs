//! Atomic rules tests for M3 — combat keywords (CR 510, 702), Auras and control (303.4,
//! 613.1b), Commander (903, 704.6c) and leaving the game with control effects (800.4a) —
//! each written from the verbatim text in docs/rules-notes/ (2026-11-06). Includes the
//! spec's M3 golden cases (commander tax across 3 casts; 21 commander damage across a death
//! and recast; a player leaving with their Aura on another player's creature and their
//! trigger on the stack).

use engine_core::events::{Event, LossReason};
use engine_core::layers::{chars, controller_of};
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, OptionDesc, PlayerId, Primitive, Response};
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);

fn scenario(players: u8, commander: bool) -> Scenario {
    let config = match (players, commander) {
        (n, true) => GameConfig::commander(n),
        (2, false) => GameConfig::constructed_1v1(),
        (n, false) => GameConfig::free_for_all(n),
    };
    let mut sc = Scenario::new(config, harness::catalog(), 13);
    for p in 0..players {
        sc.library(PlayerId(p), "Forest", 20);
    }
    sc
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

fn minimal(s: &mut Script) {
    let r = match &s.decision().prim {
        Primitive::PickN { min, .. } => Response::Pick((0..*min).collect()),
        Primitive::PickNumber { min, .. } => Response::Number(*min),
        Primitive::Allocate {
            total,
            per_min,
            per_max,
        } => {
            let mut v = per_min.clone();
            let mut left = total - per_min.iter().sum::<u32>();
            for (i, m) in per_max.iter().enumerate() {
                let add = left.min(m - v[i]);
                v[i] += add;
                left -= add;
            }
            Response::Allocate(v)
        }
        Primitive::Order => Response::Order((0..s.options().len()).collect()),
    };
    s.respond(r);
}

/// Answer minimally until `done` holds.
fn drive_until(s: &mut Script, done: impl Fn(&Script) -> bool) {
    for _ in 0..20_000 {
        if s.next.is_none() || done(s) {
            return;
        }
        minimal(s);
    }
    panic!("drive_until did not converge");
}

/// Declare `attackers` (by name) against `defender`; answer every other attacker "no".
fn attack(s: &mut Script, attackers: &[&str], defender: PlayerId) {
    drive_until(s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    while s.next.is_some() && s.kind() == ChoiceKind::DeclareAttackers {
        let pick = s.options().iter().position(|o| {
            matches!(o, OptionDesc::Attack { attacker, defender: d }
                if *d == defender && attackers.contains(&s.name(*attacker).as_str()))
        });
        s.respond(Response::Pick(pick.into_iter().collect()));
    }
}

/// Answer block decisions: `blocks` = (blocker, attacker) by name; others don't block.
fn block(s: &mut Script, blocks: &[(&str, &str)]) {
    drive_until(s, |s| s.kind() == ChoiceKind::DeclareBlockers);
    while s.next.is_some() && s.kind() == ChoiceKind::DeclareBlockers {
        let pick = s.options().iter().position(|o| {
            matches!(o, OptionDesc::Block { blocker, attacker }
                if blocks.contains(&(s.name(*blocker).as_str(), s.name(*attacker).as_str())))
        });
        s.respond(Response::Pick(pick.into_iter().collect()));
    }
}

fn combat_scenario() -> Scenario {
    let mut sc = scenario(2, false);
    sc.turn(P0, Step::BeginningOfCombat);
    sc
}

fn alive(s: &Script, owner: PlayerId, name: &str) -> bool {
    s.find(owner, Zone::Battlefield, name).is_some()
}

// --- Combat keywords ------------------------------------------------------------------------

/// CR 510.4: with first strike, only first/double strikers deal damage in the first damage
/// step; the 2/1 kills the 2/2 blocker before it can strike back.
#[test]
fn cr_510_4_first_strike_deals_damage_first() {
    let mut sc = combat_scenario();
    sc.card(P0, Zone::Battlefield, "Test Duelist");
    sc.card(P1, Zone::Battlefield, "Test Bruiser");
    let mut s = Script::new(sc.build());
    attack(&mut s, &["Test Duelist"], P1);
    block(&mut s, &[("Test Bruiser", "Test Duelist")]);
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert!(alive(&s, P0, "Test Duelist"));
    assert!(!alive(&s, P1, "Test Bruiser"));
    let damage_steps =
        s.count(|e| matches!(e, Event::StepBegan { step } if *step == Step::CombatDamage));
    assert_eq!(damage_steps, 2, "a second combat damage step (CR 510.4)");
}

/// CR 510.4 / 702.4b: double strike deals damage in both steps.
#[test]
fn cr_510_4_double_strike_deals_damage_twice() {
    let mut sc = combat_scenario();
    sc.card(P0, Zone::Battlefield, "Test Blademaster");
    let mut s = Script::new(sc.build());
    attack(&mut s, &["Test Blademaster"], P1);
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert_eq!(life(&s, P1), 16);
}

/// CR 702.19b: trample — lethal damage to the blocker, the rest may go to the player.
#[test]
fn cr_702_19b_trample_assigns_excess_to_player() {
    let mut sc = combat_scenario();
    sc.card(P0, Zone::Battlefield, "Test Trampler");
    sc.card(P1, Zone::Battlefield, "Test Sprout");
    let mut s = Script::new(sc.build());
    attack(&mut s, &["Test Trampler"], P1);
    block(&mut s, &[("Test Sprout", "Test Trampler")]);
    drive_until(&mut s, |s| s.kind() == ChoiceKind::AssignCombatDamage);
    assert_eq!(s.decision().prim, Primitive::PickNumber { min: 0, max: 3 });
    s.respond(Response::Number(3));
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert_eq!(life(&s, P1), 17);
    assert!(!alive(&s, P1, "Test Sprout"));
}

/// CR 702.2c: with deathtouch, 1 damage counts as lethal for trample's assignment.
#[test]
fn cr_702_2c_deathtouch_trample_one_is_lethal() {
    let mut sc = combat_scenario();
    sc.card(P0, Zone::Battlefield, "Test Reaper");
    sc.card(P1, Zone::Battlefield, "Test Behemoth");
    let mut s = Script::new(sc.build());
    attack(&mut s, &["Test Reaper"], P1);
    block(&mut s, &[("Test Behemoth", "Test Reaper")]);
    drive_until(&mut s, |s| s.kind() == ChoiceKind::AssignCombatDamage);
    assert_eq!(s.decision().prim, Primitive::PickNumber { min: 0, max: 1 });
    s.respond(Response::Number(1));
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert_eq!(life(&s, P1), 19);
    assert!(
        !alive(&s, P1, "Test Behemoth"),
        "1 deathtouch damage destroys it"
    );
}

/// CR 702.15b: damage from a lifelink source also gains its controller that much life.
#[test]
fn cr_702_15b_lifelink() {
    let mut sc = combat_scenario();
    sc.card(P0, Zone::Battlefield, "Test Lifebinder");
    let mut s = Script::new(sc.build());
    attack(&mut s, &["Test Lifebinder"], P1);
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert_eq!((life(&s, P0), life(&s, P1)), (22, 18));
}

/// CR 702.20b: attacking doesn't cause creatures with vigilance to tap.
#[test]
fn cr_702_20b_vigilance() {
    let mut sc = combat_scenario();
    let sentry = sc.card(P0, Zone::Battlefield, "Test Sentry");
    let bear = sc.card(P0, Zone::Battlefield, "Test Bruiser");
    let mut s = Script::new(sc.build());
    attack(&mut s, &["Test Sentry", "Test Bruiser"], P1);
    assert!(!s.st.object(sentry).tapped);
    assert!(s.st.object(bear).tapped);
}

/// CR 702.111b / 509.1b: a creature with menace can't be blocked except by two or more
/// creatures; an illegal declaration is undone and declared again.
#[test]
fn cr_702_111b_menace() {
    let mut sc = combat_scenario();
    sc.card(P0, Zone::Battlefield, "Test Marauder");
    sc.cards(P1, Zone::Battlefield, &["Test Sprout", "Test Golem"]);
    let mut s = Script::new(sc.build());
    attack(&mut s, &["Test Marauder"], P1);
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareBlockers);
    // First declaration: only the Sprout blocks — illegal, so P1 declares again.
    s.respond(Response::Pick(vec![0])); // Sprout blocks
    s.respond(Response::Pick(vec![])); // Golem doesn't
    assert_eq!(
        s.count(|e| matches!(e, Event::BlockDeclarationIllegal { .. })),
        1
    );
    assert_eq!(
        s.kind(),
        ChoiceKind::DeclareBlockers,
        "asked again from the start"
    );
    block(
        &mut s,
        &[
            ("Test Sprout", "Test Marauder"),
            ("Test Golem", "Test Marauder"),
        ],
    );
    let blocks = s.count(|e| matches!(e, Event::BlockerDeclared { .. }));
    assert_eq!(blocks, 2);
}

// --- Auras and control ----------------------------------------------------------------------

/// CR 613.1b / 303.4: "You control enchanted creature" — the Aura's controller controls it;
/// it is summoning sick for them (CR 302.6).
#[test]
fn cr_613_1b_aura_gains_control() {
    let mut sc = scenario(2, false);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 4]);
    sc.card(P0, Zone::Hand, "Test Puppetry");
    let bear = sc.card(P1, Zone::Battlefield, "Test Bruiser");
    let mut s = Script::new(sc.build());
    s.cast("Test Puppetry")
        .target("Test Bruiser")
        .pay()
        .pass_all();
    assert_eq!(controller_of(&s.st, bear), P0);
    assert_eq!(s.st.object(bear).owner, P1);
    assert!(
        s.st.object(bear).summoning_sick,
        "control changed this turn"
    );
    assert_eq!(s.count(|e| matches!(e, Event::ControlChanged { .. })), 1);
}

/// CR 303.4 / 704.5m: an Aura enters attached; when the enchanted creature leaves, the Aura
/// is put into its owner's graveyard.
#[test]
fn cr_303_4_aura_attaches_and_falls_off() {
    let mut sc = scenario(2, false);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Might", "Test Doom"]);
    let bear = sc.card(P0, Zone::Battlefield, "Test Bruiser");
    let mut s = Script::new(sc.build());
    s.cast("Test Might").target("Test Bruiser").pay().pass_all();
    let c = chars(&s.st, bear);
    assert_eq!((c.power, c.toughness), (Some(4), Some(4)));
    assert!(c.has(card_model::Keyword::Trample));
    s.cast("Test Doom").target("Test Bruiser").pay().pass_all();
    assert!(
        s.find(P0, Zone::Graveyard, "Test Might").is_some(),
        "704.5m"
    );
}

/// Spec M3 golden. CR 800.4a: when a player leaves, objects they own leave (their Aura →
/// its control effect ends, so the creature reverts to its owner), and abilities they
/// control on the stack cease to exist.
#[test]
fn cr_800_4a_leaving_player_aura_and_trigger() {
    let mut sc = scenario(4, false);
    let bear = sc.card(P1, Zone::Battlefield, "Test Bruiser");
    let aura = sc.card(P2, Zone::Battlefield, "Test Puppetry");
    sc.attach(aura, bear);
    sc.card(P2, Zone::Battlefield, "Test Watcher");
    sc.card(P3, Zone::Battlefield, "Test Sprout");
    sc.life(P2, 2);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Shock", "Test Shock"]);
    let mut s = Script::new(sc.build());
    assert_eq!(controller_of(&s.st, bear), P2);
    s.cast("Test Shock").target("Test Sprout").pay().pass_all();
    // The Sprout died: P2's Watcher trigger is on the stack.
    assert_eq!(s.st.stack.len(), 1);
    assert_eq!(s.st.stack[0].controller, P2);
    s.cast("Test Shock").target_player(P2).pay().pass_all();
    assert_eq!(
        s.count(|e| matches!(e, Event::PlayerLeftGame { player } if *player == P2)),
        1
    );
    assert!(s.st.stack.is_empty(), "P2's trigger ceased to exist");
    assert!(
        s.st.objects.contains_key(&bear),
        "the creature stays (it's P1's)"
    );
    assert_eq!(
        controller_of(&s.st, bear),
        P1,
        "the control effect ended with the Aura"
    );
}

/// CR 800.4a / 704.5m, the other direction: the enchanted creature's owner leaves; the
/// creature leaves the game with them and the Aura goes to its owner's graveyard.
#[test]
fn cr_800_4a_owner_of_enchanted_creature_leaves() {
    let mut sc = scenario(4, false);
    let bear = sc.card(P1, Zone::Battlefield, "Test Bruiser");
    let aura = sc.card(P2, Zone::Battlefield, "Test Puppetry");
    sc.attach(aura, bear);
    sc.life(P1, 2);
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    let mut s = Script::new(sc.build());
    s.cast("Test Shock").target_player(P1).pay().pass_all();
    assert!(!s.st.objects.contains_key(&bear));
    assert!(s.find(P2, Zone::Graveyard, "Test Puppetry").is_some());
}

// --- Commander --------------------------------------------------------------------------------

/// CR 903.7 / 103.4c / 903.6: Commander games start at 40 life with commanders in the
/// command zone.
#[test]
fn cr_903_7_commander_setup() {
    let st = harness::Setup::Commander { players: 4 }.game(1);
    assert!(st.players.iter().all(|p| p.life == 40));
    assert_eq!(st.zones.command.len(), 4);
    assert_eq!(st.commanders.len(), 4);
    assert!(st.zones.libraries.iter().all(|l| l.len() == 99));
}

/// Spec M3 golden. CR 903.8: "costs an additional {2} for each previous time the player
/// casting it has cast it from the command zone that game" — 4, then 6, then 8 mana.
#[test]
fn cr_903_8_commander_tax_across_three_casts() {
    let mut sc = scenario(2, true);
    sc.commander(P0, Zone::Command, "Test Warlord");
    sc.cards(P0, Zone::Battlefield, &["Forest"; 9]);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 9]);
    sc.cards(P1, Zone::Battlefield, &["Mountain"; 4]);
    sc.cards(P1, Zone::Hand, &["Test Doom", "Test Doom"]);
    let mut s = Script::new(sc.build());
    for round in 0..3 {
        s.cast("Test Warlord").pay().pass_all();
        if round < 2 {
            s.pass(); // P0 passes; P1 kills it
            s.cast("Test Doom").target("Test Warlord").pay().pass_all();
            assert_eq!(s.kind(), ChoiceKind::CommanderToCommandZone, "CR 903.9a");
            s.pick("yes", |_, o| *o == OptionDesc::Accept);
            assert!(s.find(P0, Zone::Command, "Test Warlord").is_some());
        }
    }
    // The total cost paid for each cast (CR 601.2h), from the event log.
    let paid: Vec<u32> = s
        .events
        .iter()
        .filter_map(|e| match e {
            Event::ManaPaid { player, amount } if *player == P0 => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(paid, [4, 6, 8]);
    let ci = s.st.commanders[0].1;
    assert_eq!(s.st.player(P0).commander_casts.get(&ci), Some(&3));
}

/// Spec M3 golden. CR 903.3 / 903.10a: commander damage follows the card across a death and
/// recast; 21 or more combat damage from the same commander loses the game (704.6c).
#[test]
fn cr_903_10a_commander_damage_across_death_and_recast() {
    let mut sc = scenario(2, true);
    let warlord = sc.commander(P0, Zone::Battlefield, "Test Warlord");
    sc.commander_damage(P1, warlord, 17);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 4]);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 2]);
    sc.card(P0, Zone::Hand, "Test Doom");
    let mut s = Script::new(sc.build());
    // P0 destroys its own commander, returns it, recasts it (a new object, same card).
    s.cast("Test Doom").target("Test Warlord").pay().pass_all();
    s.pick("yes", |_, o| *o == OptionDesc::Accept);
    s.cast("Test Warlord").pay().pass_all();
    let recast = s.find(P0, Zone::Battlefield, "Test Warlord").unwrap();
    assert_ne!(recast, warlord, "a new object (CR 400.7)");
    // Next turn it attacks: 17 + 4 = 21.
    drive_until(&mut s, |s| {
        s.st.turn.turn_number == 3 && s.st.turn.step == Step::BeginningOfCombat
    });
    attack(&mut s, &["Test Warlord"], P1);
    drive_until(&mut s, |s| s.next.is_none());
    assert_eq!(s.st.result.as_ref().unwrap().winner, Some(P0));
    assert!(life(&s, P1) > 0, "lost to commander damage, not life");
    assert_eq!(
        s.count(|e| matches!(
            e,
            Event::PlayerLost {
                reason: LossReason::CommanderDamage,
                ..
            }
        )),
        1
    );
}

/// CR 903.9a: a commander put into a graveyard since SBAs were last checked: its owner MAY
/// put it into the command zone; declining leaves it there (and it isn't asked again).
#[test]
fn cr_903_9a_commander_from_graveyard_to_command_zone() {
    for accept in [true, false] {
        let mut sc = scenario(2, true);
        sc.commander(P0, Zone::Battlefield, "Test Matriarch");
        sc.cards(P1, Zone::Battlefield, &["Mountain", "Mountain"]);
        sc.card(P1, Zone::Hand, "Test Doom");
        let mut s = Script::new(sc.build());
        s.pass();
        s.cast("Test Doom")
            .target("Test Matriarch")
            .pay()
            .pass_all();
        assert_eq!(
            (s.who(), s.kind()),
            (P0, ChoiceKind::CommanderToCommandZone)
        );
        let want = if accept {
            OptionDesc::Accept
        } else {
            OptionDesc::Decline
        };
        s.pick("choice", |_, o| *o == want);
        let zone = if accept {
            Zone::Command
        } else {
            Zone::Graveyard
        };
        assert!(s.find(P0, zone, "Test Matriarch").is_some());
        s.pass_all();
        assert_ne!(
            s.kind(),
            ChoiceKind::CommanderToCommandZone,
            "not asked again"
        );
    }
}

/// CR 903.9b: if a commander would be put into its owner's hand or library, its owner may
/// put it into the command zone instead (an optional replacement effect).
#[test]
fn cr_903_9b_commander_instead_of_hand() {
    for accept in [true, false] {
        let mut sc = scenario(2, true);
        sc.commander(P0, Zone::Battlefield, "Test Tyrant");
        sc.cards(P1, Zone::Battlefield, &["Forest", "Forest"]);
        sc.card(P1, Zone::Hand, "Test Recall");
        let mut s = Script::new(sc.build());
        s.pass();
        s.cast("Test Recall").target("Test Tyrant").pay().pass_all();
        assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::ChooseReplacement));
        let want = if accept {
            OptionDesc::Accept
        } else {
            OptionDesc::Decline
        };
        s.pick("choice", |_, o| *o == want);
        let zone = if accept { Zone::Command } else { Zone::Hand };
        assert!(s.find(P0, zone, "Test Tyrant").is_some(), "accept={accept}");
    }
}

/// CR 903.5a–d and 903.3: the four commander decks are legal; violations are reported.
#[test]
fn cr_903_5_commander_deck_validation() {
    let cat = harness::catalog();
    for f in harness::COMMANDER_DECKS {
        let d = harness::deck_file(f);
        assert!(card_db::validate_commander_deck(&cat, &d).is_empty(), "{f}");
    }
    let base = harness::deck_file("commander_matriarch.ron");
    let forest = cat.find("Forest").unwrap();
    // Each variant swaps one basic Forest for a violation (still 100 cards).
    let swap = |with: card_model::CardDefId| {
        let mut d = base.clone();
        let i = d.cards.iter().position(|c| *c == forest).unwrap();
        d.cards[i] = with;
        card_db::validate_commander_deck(&cat, &d).join("\n")
    };
    assert!(
        swap(cat.find("Test Shock").unwrap()).contains("903.5c"),
        "off-color"
    );
    let nonbasic = *base.cards.iter().find(|c| **c != forest).unwrap();
    assert!(swap(nonbasic).contains("903.5b"), "duplicate");
    assert!(
        swap(cat.find("Mountain").unwrap()).contains("903.5d"),
        "Mountain in mono-green"
    );
    let mut short = base.clone();
    short.cards.pop();
    assert!(
        card_db::validate_commander_deck(&cat, &short)
            .join("\n")
            .contains("903.5a")
    );
}
