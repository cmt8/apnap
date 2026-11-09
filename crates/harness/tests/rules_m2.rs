//! Atomic rules tests for M2 — layers (CR 613), replacement/prevention (614–616), triggers
//! (603) and state-based actions (704) — each written from the verbatim text in
//! docs/rules-notes/ (2026-11-06). Includes the spec's seven M2 golden cases and the CR's
//! own worked examples (613.4d, 613.5, 613.6).

use engine_core::events::Event;
use engine_core::layers::chars;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, ObjectId, OptionDesc, PlayerId, Primitive, Response};
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
    let mut sc = Scenario::new(config, harness::catalog(), 11);
    for p in 0..players {
        sc.library(PlayerId(p), "Forest", 10);
    }
    sc
}

fn pt(s: &Script, id: ObjectId) -> (i32, i32) {
    let c = chars(&s.st, id);
    (c.power.unwrap(), c.toughness.unwrap())
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

fn on_battlefield(s: &Script, owner: PlayerId, name: &str) -> Vec<ObjectId> {
    s.st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| s.st.object(id).owner == owner && s.name(id) == name)
        .collect()
}

/// Answer whatever comes up with the minimal legal response until `done` holds.
fn drive_until(s: &mut Script, done: impl Fn(&Script) -> bool) {
    for _ in 0..10_000 {
        if s.next.is_none() || done(s) {
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

// --- CR 613: layers ------------------------------------------------------------------------

/// CR 613.5, the CR's own example: a 2/2 with a +1/+1 counter, "+4/+4 until end of turn",
/// "creatures you control get +0/+2", then "becomes 0/1 until end of turn" is 5/8.
#[test]
fn cr_613_5_worked_example_is_5_8() {
    let mut sc = scenario(2);
    let ogre = sc.card(P0, Zone::Battlefield, "Test Bruiser");
    sc.cards(P0, Zone::Battlefield, &["Forest"; 6]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Bolster", "Test Growth", "Test Bulwark", "Test Shrink"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Bolster")
        .target("Test Bruiser")
        .pay()
        .pass_all();
    assert_eq!(pt(&s, ogre), (3, 3));
    s.cast("Test Growth")
        .target("Test Bruiser")
        .pay()
        .pass_all();
    assert_eq!(pt(&s, ogre), (7, 7));
    s.cast("Test Bulwark").pay().pass_all();
    assert_eq!(pt(&s, ogre), (7, 9));
    s.cast("Test Shrink")
        .target("Test Bruiser")
        .pay()
        .pass_all();
    assert_eq!(
        pt(&s, ogre),
        (5, 8),
        "0/1 (7b), then +4/+4, +0/+2 and the counter (7c)"
    );
}

/// Spec M2 golden: an anthem, -1/-1 counters and "becomes 0/1" (7b before 7c).
#[test]
fn cr_613_anthem_counters_and_becomes_0_1() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Anthem");
    let bear = sc.card(P0, Zone::Battlefield, "Test Bruiser");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Forest"]);
    sc.cards(P0, Zone::Hand, &["Test Wither", "Test Shrink"]);
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, bear), (3, 3));
    s.cast("Test Wither")
        .target("Test Bruiser")
        .pay()
        .pass_all();
    assert_eq!(pt(&s, bear), (1, 1), "2/2 +1/+1 -2/-2");
    s.cast("Test Shrink")
        .target("Test Bruiser")
        .pay()
        .pass_all();
    // 0/1 base, +1/+1 anthem, -2/-2 counters = -1/0 → CR 704.5f.
    assert!(s.find(P0, Zone::Graveyard, "Test Bruiser").is_some());
}

/// CR 613.4d example: switching applies after all other P/T effects, so a 2/1 that is
/// switched and then gets +4/+4 is 5/6.
#[test]
fn cr_613_4d_switch_applies_last() {
    let mut sc = scenario(2);
    let raider = sc.card(P0, Zone::Battlefield, "Test Raider");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Forest"]);
    sc.cards(P0, Zone::Hand, &["Test Twist", "Test Growth"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Twist").target("Test Raider").pay().pass_all();
    assert_eq!(pt(&s, raider), (1, 2));
    s.cast("Test Growth").target("Test Raider").pay().pass_all();
    assert_eq!(pt(&s, raider), (5, 6), "unswitched 6/5, switched 5/6");
}

/// CR 613.6: "If an effect starts to apply in one layer … it will continue to be applied
/// to the same set of objects in each other applicable layer". The CR example: "All
/// noncreature artifacts become 2/2 artifact creatures" — the 7b part still applies to them
/// although they're creatures by then.
#[test]
fn cr_613_6_effect_keeps_its_set_across_layers() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Assembly");
    let prism = sc.card(P0, Zone::Battlefield, "Test Prism");
    let s = Script::new(sc.build());
    let c = chars(&s.st, prism);
    assert!(c.is(card_model::CardType::Creature) && c.is(card_model::CardType::Artifact));
    assert_eq!((c.power, c.toughness), (Some(2), Some(2)));
}

/// CR 613.8a–b: "All lands are 1/1 creatures" depends on "All artifacts are Forest lands"
/// (the latter changes what the former applies to), so it waits for it even though it
/// has the earlier timestamp. Timestamp order alone would leave the artifact unanimated.
#[test]
fn cr_613_8_dependency_overrides_timestamp() {
    for awakening_first in [true, false] {
        let mut sc = scenario(2);
        if awakening_first {
            sc.card(P0, Zone::Battlefield, "Test Awakening");
            sc.card(P0, Zone::Battlefield, "Test Overgrowth");
        } else {
            sc.card(P0, Zone::Battlefield, "Test Overgrowth");
            sc.card(P0, Zone::Battlefield, "Test Awakening");
        }
        let prism = sc.card(P0, Zone::Battlefield, "Test Prism");
        let s = Script::new(sc.build());
        let c = chars(&s.st, prism);
        assert!(c.is(card_model::CardType::Land), "{awakening_first}");
        assert!(c.land_types.contains(&card_model::BasicLandType::Forest));
        assert!(
            c.is(card_model::CardType::Creature),
            "dependency: {awakening_first}"
        );
        assert_eq!((c.power, c.toughness), (Some(1), Some(1)));
    }
}

/// CR 604.2 / 613.1f: an ability-removing effect (layer 6) removes an anthem's source
/// ability before layer 7c, so the anthem's effect never begins to apply.
#[test]
fn cr_613_removing_the_source_ability_ends_its_effect() {
    let mut sc = scenario(2);
    let captain = sc.card(P0, Zone::Battlefield, "Test Captain");
    let bear = sc.card(P0, Zone::Battlefield, "Test Bruiser");
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, bear), (3, 3));

    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Captain");
    sc.card(P0, Zone::Battlefield, "Test Bruiser");
    sc.card(P1, Zone::Battlefield, "Test Silence");
    let stinger = sc.card(P0, Zone::Battlefield, "Test Stinger");
    let s = Script::new(sc.build());
    let bear = on_battlefield(&s, P0, "Test Bruiser")[0];
    let captain2 = on_battlefield(&s, P0, "Test Captain")[0];
    assert_eq!(pt(&s, bear), (2, 2), "the Captain lost its anthem ability");
    assert!(chars(&s.st, captain2).abilities.is_empty());
    assert!(!chars(&s.st, stinger).has(card_model::Keyword::Deathtouch));
    let _ = captain;
}

// --- CR 614–616: replacement and prevention ------------------------------------------------

/// Spec M2 golden (Doubling Season + token creation). CR 614.16 / 122.6: "would create
/// tokens" and "would put counters" effects, including counters a permanent enters with.
#[test]
fn cr_614_16_doubler_doubles_tokens_and_counters() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Doubler");
    sc.cards(P0, Zone::Battlefield, &["Forest"; 7]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Herald", "Test Bolster", "Test Sprouting"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Herald").pay().pass_all(); // resolves; its ETB trigger goes on the stack
    assert_eq!(s.st.stack.len(), 1, "the Herald's enters trigger");
    s.pass_all();
    let soldiers = on_battlefield(&s, P0, "Test Soldier");
    assert_eq!(soldiers.len(), 2, "one token, doubled");
    assert!(soldiers.iter().all(|t| s.st.object(*t).is_token));

    s.cast("Test Bolster")
        .target("Test Herald")
        .pay()
        .pass_all();
    let herald = on_battlefield(&s, P0, "Test Herald")[0];
    assert_eq!(
        s.st.object(herald)
            .counters_of(card_model::CounterKind::PlusOne),
        2
    );

    s.cast("Test Sprouting").pay().pass_all();
    let sprout = on_battlefield(&s, P0, "Test Sprouting")[0];
    assert_eq!(pt(&s, sprout), (4, 4), "enters with 2 counters, doubled");
}

/// CR 614.1c–d: "This land enters tapped" modifies how it enters.
#[test]
fn cr_614_1c_enters_tapped() {
    let mut sc = scenario(2);
    sc.cards(P0, Zone::Hand, &["Test Gate"]);
    let mut s = Script::new(sc.build());
    s.pick(
        "play Test Gate",
        |s, o| matches!(o, OptionDesc::PlayLand { card } if s.name(*card) == "Test Gate"),
    );
    let gate = on_battlefield(&s, P0, "Test Gate")[0];
    assert!(s.st.object(gate).tapped);
}

/// Spec M2 golden + CR 616.1's own example: "If a card would be put into a graveyard from
/// anywhere, instead exile it" vs "If this creature would die, instead shuffle it into its
/// owner's library": the affected object's controller chooses; the other does nothing.
#[test]
fn cr_616_1_affected_controller_chooses_between_replacements() {
    for choose_phoenix in [true, false] {
        let mut sc = scenario(2);
        sc.card(P0, Zone::Battlefield, "Test Exiler");
        sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
        sc.card(P0, Zone::Hand, "Test Doom");
        let phoenix = sc.card(P1, Zone::Battlefield, "Test Phoenix");
        let mut s = Script::new(sc.build());
        s.cast("Test Doom").target("Test Phoenix").pay().pass_all();
        assert_eq!((s.who(), s.kind()), (P1, ChoiceKind::ChooseReplacement));
        assert_eq!(s.options().len(), 2);
        s.pick("replacement", |_, o| {
            matches!(o, OptionDesc::Replacement { source, .. } if (*source == phoenix) == choose_phoenix)
        });
        if choose_phoenix {
            assert!(s.find(P1, Zone::Library, "Test Phoenix").is_some());
            assert!(s.find(P1, Zone::Exile, "Test Phoenix").is_none());
        } else {
            assert!(s.find(P1, Zone::Exile, "Test Phoenix").is_some());
        }
        assert!(s.find(P1, Zone::Graveyard, "Test Phoenix").is_none());
        // Doom itself would go to the graveyard: only the Exiler applies, no choice.
        assert!(s.find(P0, Zone::Exile, "Test Doom").is_some());
    }
}

/// CR 615.7: "Prevent the next 3 damage that would be dealt to [target] this turn" works
/// like a shield across events.
#[test]
fn cr_615_7_prevention_shield() {
    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Ward", "Test Shock", "Test Shock"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Ward").target_player(P0).pay().pass_all();
    s.cast("Test Shock").target_player(P0).pay().pass_all();
    assert_eq!(life(&s, P0), 20, "2 of the 3 prevented");
    s.cast("Test Shock").target_player(P0).pay().pass_all();
    assert_eq!(life(&s, P0), 19, "1 prevented, 1 dealt");
    let prevented: u32 = s
        .events
        .iter()
        .filter_map(|e| match e {
            Event::DamagePrevented { amount, .. } => Some(*amount),
            _ => None,
        })
        .sum();
    assert_eq!(prevented, 3);
}

// --- CR 603: triggered abilities -----------------------------------------------------------

/// Spec M2 golden: a dies trigger reads last known information. CR 603.10a / 608.2h: the
/// Martyr was 3/3 (anthem) when it died, so "life equal to its power" is 3, not 2.
#[test]
fn cr_603_10a_dies_trigger_reads_lki_power() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Anthem");
    sc.card(P0, Zone::Battlefield, "Test Martyr");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.card(P0, Zone::Hand, "Test Doom");
    let mut s = Script::new(sc.build());
    s.cast("Test Doom").target("Test Martyr").pay().pass_all();
    assert_eq!(s.st.stack.len(), 1, "the dies trigger");
    s.pass_all();
    assert_eq!(life(&s, P0), 23);
}

/// CR 603.3b: each player, in APNAP order, puts their triggers on the stack in the order
/// they choose — the active player's go on first, so the nonactive player's resolve first.
#[test]
fn cr_603_3b_triggers_stack_in_apnap_order() {
    let mut sc = scenario(2);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Watcher", "Test Sprout", "Forest", "Forest"],
    );
    sc.card(P0, Zone::Hand, "Test Edict");
    sc.cards(P1, Zone::Battlefield, &["Test Watcher", "Test Spark"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Edict").pay().pass_all();
    // Each player sacrifices their non-Watcher creature (APNAP: P0 then P1).
    for keep_out in ["Test Sprout", "Test Spark"] {
        assert_eq!(s.kind(), ChoiceKind::Sacrifice);
        s.pick(
            keep_out,
            |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == keep_out),
        );
    }
    // Both Watchers see both deaths: two triggers each.
    let mut orderers = Vec::new();
    while s.kind() == ChoiceKind::OrderTriggers {
        orderers.push(s.who());
        s.respond(Response::Order(vec![0, 1]));
    }
    assert_eq!(orderers, [P0, P1]);
    assert_eq!(s.st.stack.len(), 4);
    assert_eq!(
        s.st.stack.last().unwrap().controller,
        P1,
        "the NAP's triggers are on top"
    );
    s.pass_all();
    assert_eq!(
        (life(&s, P0), life(&s, P1)),
        (20, 21),
        "P1's trigger resolved first"
    );
}

/// CR 603.4: an intervening "if" is checked when the trigger event occurs and again on
/// resolution.
#[test]
fn cr_603_4_intervening_if_checked_twice() {
    for (start_life, respond) in [(15, false), (10, false), (10, true)] {
        let mut sc = scenario(2);
        sc.turn(P1, Step::End);
        sc.card(P0, Zone::Battlefield, "Test Sentinel");
        sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
        sc.card(P0, Zone::Hand, "Test Charm");
        sc.life(P0, start_life);
        let mut s = Script::new(sc.build());
        drive_until(&mut s, |s| {
            s.st.turn.active == P0 && s.st.turn.step == Step::Upkeep
        });
        let triggered = !s.st.stack.is_empty();
        assert_eq!(
            triggered,
            start_life <= 10,
            "condition on the trigger event"
        );
        if respond {
            s.cast("Test Charm")
                .mode(1)
                .target_player(P0)
                .pay()
                .pass_all();
            assert_eq!(life(&s, P0), 13);
        }
        s.pass_all();
        let expected = match (start_life, respond) {
            (15, _) => 15,
            (_, false) => 12,
            (_, true) => 13, // removed on resolution: no longer 10 or less
        };
        assert_eq!(
            life(&s, P0),
            expected,
            "start {start_life}, respond {respond}"
        );
    }
}

/// CR 603.3d: a triggered ability's targets are chosen as it's put on the stack.
#[test]
fn cr_603_3d_trigger_targets_chosen_when_put_on_stack() {
    let mut sc = scenario(2);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    sc.card(P0, Zone::Hand, "Test Pyre");
    let mut s = Script::new(sc.build());
    s.cast("Test Pyre").pay().pass_all();
    assert_eq!(
        (s.who(), s.kind()),
        (P0, ChoiceKind::ChooseTargets { slot: 0 })
    );
    s.target_player(P1).pass_all();
    assert_eq!(life(&s, P1), 19);
}

/// CR 601.2i / 603.2: "whenever you cast" triggers on casting; it resolves before the spell.
#[test]
fn cr_603_2_cast_trigger() {
    let mut sc = scenario(2);
    let prodigy = sc.card(P0, Zone::Battlefield, "Test Prodigy");
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Shock");
    let mut s = Script::new(sc.build());
    s.cast("Test Shock").target_player(P1).pay();
    assert_eq!(s.st.stack.len(), 2, "the trigger went on top of the Shock");
    s.pass_all();
    assert_eq!(pt(&s, prodigy), (2, 2));
    assert_eq!(life(&s, P1), 20, "the Shock hasn't resolved yet");
    s.pass_all();
    assert_eq!(life(&s, P1), 18);
}

// --- CR 704: state-based actions -----------------------------------------------------------

/// Spec M2 golden. CR 704.5j: "that player chooses one of them, and the rest are put into
/// their owners' graveyards."
#[test]
fn cr_704_5j_legend_rule() {
    let mut sc = scenario(2);
    let old = sc.card(P0, Zone::Battlefield, "Test Legend");
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Legend");
    let mut s = Script::new(sc.build());
    s.cast("Test Legend").pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::LegendRule));
    assert_eq!(s.options().len(), 2);
    s.pick(
        "the new one",
        |_, o| matches!(o, OptionDesc::Card { card } if *card != old),
    );
    assert_eq!(on_battlefield(&s, P0, "Test Legend").len(), 1);
    assert!(s.find(P0, Zone::Graveyard, "Test Legend").is_some());
}

/// CR 704.5h: a creature dealt damage by a deathtouch source is destroyed.
#[test]
fn cr_704_5h_deathtouch_destroys() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Stinger");
    sc.card(P1, Zone::Battlefield, "Test Behemoth");
    let mut s = Script::new(sc.build());
    s.activate("Test Stinger")
        .target("Test Behemoth")
        .pass_all();
    assert!(s.find(P1, Zone::Graveyard, "Test Behemoth").is_some());
}

/// CR 704.5q: N +1/+1 and N -1/-1 counters are removed.
#[test]
fn cr_704_5q_counters_annihilate() {
    let mut sc = scenario(2);
    let bear = sc.card(P0, Zone::Battlefield, "Test Bruiser");
    sc.cards(P0, Zone::Battlefield, &["Forest", "Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Bolster", "Test Wither"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Bolster")
        .target("Test Bruiser")
        .pay()
        .pass_all();
    s.cast("Test Wither")
        .target("Test Bruiser")
        .pay()
        .pass_all();
    let o = s.st.object(bear);
    assert_eq!(o.counters_of(card_model::CounterKind::PlusOne), 0);
    assert_eq!(o.counters_of(card_model::CounterKind::MinusOne), 1);
    assert_eq!(pt(&s, bear), (1, 1));
}

/// CR 704.5d / 111.7: a token that leaves the battlefield ceases to exist.
#[test]
fn cr_704_5d_token_ceases_to_exist() {
    let mut sc = scenario(2);
    sc.card(P1, Zone::Battlefield, "Test Soldier");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.card(P0, Zone::Hand, "Test Doom");
    let mut s = Script::new(sc.build());
    s.cast("Test Doom").target("Test Soldier").pay().pass_all();
    assert_eq!(s.count(|e| matches!(e, Event::CeasedToExist { .. })), 1);
    assert!(s.st.objects.values().all(|o| !o.is_token));
}

/// Spec M2 golden (simultaneous lethal damage to multiple players). CR 704.3 / 704.5a: both
/// players lose in the same SBA check; in a multiplayer game they leave (800.4a), and if
/// every remaining player loses at once it's a draw (CR 104.4a).
#[test]
fn cr_704_5a_simultaneous_lethal_damage_to_several_players() {
    let mut sc = scenario(4);
    sc.life(P1, 3);
    sc.life(P2, 3);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    sc.card(P0, Zone::Hand, "Test Inferno");
    let mut s = Script::new(sc.build());
    s.cast("Test Inferno").pay().pass_all();
    let losses: Vec<_> = s
        .events
        .iter()
        .filter_map(|e| match e {
            Event::PlayerLost { player, .. } => Some(*player),
            _ => None,
        })
        .collect();
    assert_eq!(losses, [P1, P2]);
    assert_eq!(s.st.players_in_game(), 2);
    assert_eq!((life(&s, P0), life(&s, P3)), (17, 17));
    assert!(s.next.is_some(), "the game continues");

    let mut sc = scenario(2);
    sc.life(P0, 3);
    sc.life(P1, 3);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    sc.card(P0, Zone::Hand, "Test Inferno");
    let mut s = Script::new(sc.build());
    s.cast("Test Inferno").pay().pass_all();
    assert!(s.next.is_none());
    assert_eq!(
        s.st.result.as_ref().unwrap().winner,
        None,
        "CR 104.4a: a draw"
    );
}

// --- CR 702.9 / 702.17: flying and reach ---------------------------------------------------

/// CR 702.9b: "A creature with flying can't be blocked except by creatures with flying
/// and/or reach."
#[test]
fn cr_702_9b_flying_and_reach() {
    let mut sc = scenario(2);
    sc.card(P0, Zone::Battlefield, "Test Hawk");
    sc.card(P1, Zone::Battlefield, "Test Bruiser");
    let spider = sc.card(P1, Zone::Battlefield, "Test Spider");
    let mut s = Script::new(sc.build());
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    s.respond(Response::Pick(vec![0])); // the Hawk attacks P1
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareBlockers);
    let mut asked = Vec::new();
    while s.kind() == ChoiceKind::DeclareBlockers {
        for o in s.options() {
            if let OptionDesc::Block { blocker, .. } = o {
                asked.push(*blocker);
            }
        }
        s.respond(Response::Pick(vec![]));
    }
    assert_eq!(
        asked,
        [spider],
        "only the reach creature may block the flyer"
    );
}
