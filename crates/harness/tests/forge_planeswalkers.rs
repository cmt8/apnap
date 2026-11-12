//! Planeswalkers (D-086): loyalty, loyalty abilities, damage to planeswalkers, attacking
//! them. Invented cards in Forge's format. Rules text: docs/rules-notes/306-planeswalkers.md
//! (CR 306, 606, 115.4, 120.3c, 704.5i, 506.3–506.4, 508.1b, 509.1a, 510.1b, 702.19).

use card_model::{AbilityDef, CounterKind, Timing};
use engine_core::events::Event;
use engine_core::stack::Target;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{
    ChoiceKind, GameConfig, GameState, ObjectId, OptionDesc, PlayerId, Primitive, Response,
};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// Loyalty 3. "+1: You gain 2 life." "−2: deals 3 damage to target creature." "−X: Draw X
/// cards." "0: Draw a card."
const WALKER: &str = "Name:Test Sage Walker\nManaCost:2 W\nTypes:Legendary Planeswalker Ajani\nLoyalty:3\n\
    A:AB$ GainLife | Cost$ AddCounter<1/LOYALTY> | Planeswalker$ True | LifeAmount$ 2 | SpellDescription$ You gain 2 life.\n\
    A:AB$ DealDamage | Cost$ SubCounter<2/LOYALTY> | Planeswalker$ True | ValidTgts$ Creature | NumDmg$ 3 | SpellDescription$ CARDNAME deals 3 damage to target creature.\n\
    A:AB$ Draw | Cost$ SubCounter<X/LOYALTY> | Planeswalker$ True | NumCards$ X | SpellDescription$ Draw X cards.\n\
    SVar:X:Count$xPaid\n\
    A:AB$ Draw | Cost$ AddCounter<0/LOYALTY> | Planeswalker$ True | Ultimate$ True | NumCards$ 1 | SpellDescription$ Draw a card.\n\
    Oracle:[+1]: You gain 2 life.\\n[−2]: Test Sage Walker deals 3 damage to target creature.\\n[−X]: Draw X cards.\\n[0]: Draw a card.";

const ZAP: &str = "Name:Test Zap\nManaCost:R\nTypes:Instant\n\
    A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 2 | SpellDescription$ Test Zap deals 2 damage to any target.\n\
    Oracle:Test Zap deals 2 damage to any target.";

const BRUISER: &str = "Name:Test Walker Ogre\nManaCost:2 R\nTypes:Creature Ogre\nPT:3/3\nOracle:";
const STOMPER: &str =
    "Name:Test Stomper\nManaCost:4 G\nTypes:Creature Beast\nPT:5/5\nK:Trample\nOracle:Trample";
const CHUMP: &str = "Name:Test Chump\nManaCost:W\nTypes:Creature Soldier\nPT:1/1\nOracle:";

/// "If one or more counters would be put on a permanent you control, twice that many are
/// put on it instead."
const SEASON: &str = "Name:Test Counter Season\nManaCost:2 G\nTypes:Enchantment\n\
    R:Event$ AddCounter | ActiveZones$ Battlefield | ValidCard$ Permanent.YouCtrl+inZoneBattlefield | EffectOnly$ True | ReplaceWith$ Repl | Description$ Text.\n\
    SVar:Repl:DB$ ReplaceCounter | ChooseCounter$ True | Amount$ X\n\
    SVar:X:ReplaceCount$CounterNum/Twice\nOracle:Text.";

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 41);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

fn loyalty(st: &GameState, id: ObjectId) -> u32 {
    st.object(id).counters_of(CounterKind::LOYALTY)
}

/// A walker already on the battlefield with `n` loyalty counters.
fn with_loyalty(mut st: GameState, id: ObjectId, n: u32) -> GameState {
    st.objects
        .get_mut(&id)
        .unwrap()
        .counters
        .insert(CounterKind::LOYALTY, n);
    st
}

/// Is loyalty ability `index` (0 is the intrinsic "enters with", 1 is "+1", …) of `pw`
/// offered?
fn offered(s: &Script, pw: ObjectId, index: u16) -> bool {
    s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { source, ability } if *source == pw && ability.index == index))
}

fn activate(s: &mut Script, pw: ObjectId, index: u16) {
    s.pick(&format!("loyalty ability {index}"), |_, o| {
        matches!(o, OptionDesc::ActivateAbility { source, ability } if *source == pw && ability.index == index)
    });
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

// CR 306.5b (2026-11-06): a planeswalker enters with its printed loyalty in loyalty counters;
// CR 606.2: an ability with a loyalty symbol in its cost is a loyalty ability, written
// "[+1]: …".
#[test]
fn cr_306_5_compiled_loyalty_and_loyalty_abilities() {
    let catalog = forge_test_catalog(&[WALKER], &[]);
    let def = catalog.get(catalog.find("Test Sage Walker").unwrap());
    assert_eq!(def.loyalty, Some(3));
    let loyalty_abilities = def
        .abilities
        .iter()
        .filter(|a| {
            matches!(
                a,
                AbilityDef::Activated {
                    timing: Timing::Loyalty,
                    ..
                }
            )
        })
        .count();
    assert_eq!(loyalty_abilities, 4);
    let text = card_model::describe::rules_text(def).join(" / ");
    assert!(
        text.contains("This planeswalker enters with three loyalty counters on it.")
            && text.contains("[+1]: You gain 2 life.")
            && text.contains("[\u{2212}2]: This planeswalker deals 3 damage to target creature.")
            && text.contains("[\u{2212}X]: Draw X cards.")
            && text.contains("[0]: Draw a card."),
        "{text}"
    );
}

// CR 306.5b, 606.3, 606.4 (2026-11-06): cast, it enters with three loyalty counters; "+1"
// puts one on it as the cost; only one loyalty ability of it per turn, as a sorcery; the
// next turn again.
#[test]
fn cr_606_3_one_loyalty_ability_per_turn_as_a_sorcery() {
    let mut sc = scenario(&[WALKER, ZAP]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains", "Plains"]);
    sc.card(P0, Zone::Hand, "Test Sage Walker");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Sage Walker").pay().pass_all();
    let pw = s.find(P0, Zone::Battlefield, "Test Sage Walker").unwrap();
    assert_eq!(loyalty(&s.st, pw), 3);
    assert!(offered(&s, pw, 1));
    activate(&mut s, pw, 1);
    assert_eq!(loyalty(&s.st, pw), 4, "the cost is paid as it is activated");
    // On the stack: not even another one, and not at instant speed.
    assert!(!offered(&s, pw, 4));
    s.pass_all();
    assert_eq!(s.st.player(P0).life, 42);
    assert!(!offered(&s, pw, 4), "one loyalty ability per turn");
    // The opponent's turn: never.
    drive_until(&mut s, |s| {
        s.st.turn.active == P1 && s.st.turn.step == Step::PrecombatMain && s.who() == P0
    });
    assert!(!offered(&s, pw, 4));
    drive_until(&mut s, |s| {
        s.st.turn.active == P0
            && s.st.turn.step == Step::PrecombatMain
            && s.kind() == ChoiceKind::Priority
    });
    assert!(offered(&s, pw, 4), "a new turn");
    // "[0]": no counter put on or removed.
    activate(&mut s, pw, 4);
    assert_eq!(loyalty(&s.st, pw), 4);
}

// CR 606.6 (2026-11-06): a negative loyalty cost can't be paid with fewer counters; "−X"
// can remove at most as many as it has. CR 704.5i, 306.9: at loyalty 0 it goes to its
// owner's graveyard.
#[test]
fn cr_606_6_minus_abilities_and_704_5i_zero_loyalty() {
    let mut sc = scenario(&[WALKER, BRUISER]);
    let pw = sc.card(P0, Zone::Battlefield, "Test Sage Walker");
    sc.card(P1, Zone::Battlefield, "Test Walker Ogre");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(with_loyalty(sc.build(), pw, 1));
    assert!(!offered(&s, pw, 2), "−2 needs two counters");
    assert!(offered(&s, pw, 3));
    activate(&mut s, pw, 3);
    assert_eq!(s.kind(), ChoiceKind::ChooseX);
    assert!(matches!(
        s.decision().prim,
        Primitive::PickNumber { min: 0, max: 1 }
    ));
    let hand = s.st.zones.hands[P0.index()].len();
    s.x(1);
    // Zero loyalty: gone before the ability even resolves (CR 704.3), which still does.
    s.pass_all();
    assert_eq!(s.st.zones.hands[P0.index()].len(), hand + 1);
    assert!(!s.st.zones.battlefield.contains(&pw));
    assert!(s.find(P0, Zone::Graveyard, "Test Sage Walker").is_some());
}

// CR 115.4, 120.3c, 306.8 (2026-11-06): "any target" includes planeswalkers; damage dealt to
// a planeswalker removes that many loyalty counters (it isn't marked); then 704.5i.
#[test]
fn cr_120_3c_damage_to_a_planeswalker_removes_loyalty() {
    let mut sc = scenario(&[WALKER, ZAP]);
    let pw = sc.card(P1, Zone::Battlefield, "Test Sage Walker");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Zap", "Test Zap"]);
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(with_loyalty(sc.build(), pw, 3));
    s.cast("Test Zap");
    s.target_object(pw).pay().pass_all();
    assert_eq!(loyalty(&s.st, pw), 1);
    assert_eq!(s.st.object(pw).damage_marked, 0);
    assert_eq!(s.st.player(P1).life, 40);
    s.cast("Test Zap");
    s.target_object(pw).pay().pass_all();
    assert!(!s.st.zones.battlefield.contains(&pw), "704.5i");
}

/// To P0's declare-attackers decision for `attacker`.
fn to_attack(s: &mut Script, attacker: ObjectId) {
    drive_until(s, |s| {
        s.kind() == ChoiceKind::DeclareAttackers
            && s.options().iter().any(|o| match o {
                OptionDesc::Attack { attacker: a, .. }
                | OptionDesc::AttackPlaneswalker { attacker: a, .. } => *a == attacker,
                _ => false,
            })
    });
}

// CR 506.3, 508.1b (2026-11-06): a creature may attack a planeswalker a defending player
// controls — offered after that player, the planeswalkers in timestamp order. CR 509.1a:
// that player's creatures may block it. CR 510.1b: unblocked, it deals its damage to the
// planeswalker, not the player.
#[test]
fn cr_508_1b_attacking_a_planeswalker() {
    let mut sc = scenario(&[WALKER, BRUISER]);
    let pw = sc.card(P1, Zone::Battlefield, "Test Sage Walker");
    let ogre = sc.card(P0, Zone::Battlefield, "Test Walker Ogre");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(with_loyalty(sc.build(), pw, 5));
    to_attack(&mut s, ogre);
    assert_eq!(
        s.options(),
        &[
            OptionDesc::Attack {
                attacker: ogre,
                defender: P1
            },
            OptionDesc::AttackPlaneswalker {
                attacker: ogre,
                planeswalker: pw
            },
        ]
    );
    s.pick("attack the planeswalker", |_, o| {
        matches!(o, OptionDesc::AttackPlaneswalker { .. })
    });
    assert_eq!(
        s.count(
            |e| matches!(e, Event::AttacksPlaneswalker { attacker, planeswalker }
            if *attacker == ogre && *planeswalker == pw)
        ),
        1
    );
    let combat = s.st.combat.as_ref().unwrap();
    assert_eq!(combat.attackers[0].defender, P1, "CR 508.5");
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert_eq!(loyalty(&s.st, pw), 2);
    assert_eq!(s.st.player(P1).life, 40);
}

// CR 702.19b, 702.19f (2026-11-06): a creature with trample attacking a planeswalker assigns
// its excess to the planeswalker, never to its controller. CR 509.1a: blocked by a creature
// of the planeswalker's controller.
#[test]
fn cr_702_19f_trample_over_a_blocker_goes_to_the_planeswalker() {
    let mut sc = scenario(&[WALKER, STOMPER, CHUMP]);
    let pw = sc.card(P1, Zone::Battlefield, "Test Sage Walker");
    let beast = sc.card(P0, Zone::Battlefield, "Test Stomper");
    let chump = sc.card(P1, Zone::Battlefield, "Test Chump");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(with_loyalty(sc.build(), pw, 9));
    to_attack(&mut s, beast);
    s.pick("attack the planeswalker", |_, o| {
        matches!(o, OptionDesc::AttackPlaneswalker { .. })
    });
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareBlockers);
    s.pick("chump", |_, o| {
        matches!(o, OptionDesc::Block { blocker, attacker } if *blocker == chump && *attacker == beast)
    });
    drive_until(&mut s, |s| s.kind() == ChoiceKind::AssignCombatDamage);
    // Lethal (1) to the blocker first: up to 4 to the planeswalker.
    assert!(matches!(
        s.decision().prim,
        Primitive::PickNumber { min: 0, max: 4 }
    ));
    s.respond(Response::Number(4));
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert_eq!(loyalty(&s.st, pw), 5);
    assert_eq!(s.st.player(P1).life, 40);
    assert!(!s.st.zones.battlefield.contains(&chump));
}

// CR 506.4, 506.4c, 510.1b (2026-11-06): a planeswalker that leaves the battlefield is
// removed from combat; the creature attacking it stays attacking but, unblocked, deals no
// combat damage.
#[test]
fn cr_506_4c_attacking_a_planeswalker_that_left() {
    let mut sc = scenario(&[WALKER, BRUISER, ZAP]);
    let pw = sc.card(P1, Zone::Battlefield, "Test Sage Walker");
    let ogre = sc.card(P0, Zone::Battlefield, "Test Walker Ogre");
    sc.cards(P0, Zone::Battlefield, &["Mountain"]);
    sc.card(P0, Zone::Hand, "Test Zap");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(with_loyalty(sc.build(), pw, 2));
    to_attack(&mut s, ogre);
    s.pick("attack the planeswalker", |_, o| {
        matches!(o, OptionDesc::AttackPlaneswalker { .. })
    });
    // In the declare attackers step, P0 zaps the planeswalker.
    drive_until(&mut s, |s| {
        s.st.turn.step == Step::DeclareAttackers && s.kind() == ChoiceKind::Priority
    });
    s.cast("Test Zap");
    s.target_object(pw).pay().pass_all();
    assert!(!s.st.zones.battlefield.contains(&pw));
    assert!(
        s.st.combat
            .as_ref()
            .unwrap()
            .attackers
            .iter()
            .any(|a| a.attacker == ogre),
        "still attacking"
    );
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert_eq!(s.st.player(P1).life, 40, "no damage to the player");
    assert_eq!(
        s.count(
            |e| matches!(e, Event::DamageDealt { source, combat: true, .. } if *source == ogre)
        ),
        0
    );
}

// CR 306.4, 704.5j (2026-11-06): legendary planeswalkers follow the legend rule.
#[test]
fn cr_306_4_the_legend_rule_for_planeswalkers() {
    let mut sc = scenario(&[WALKER]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains", "Plains"]);
    let first = sc.card(P0, Zone::Battlefield, "Test Sage Walker");
    sc.card(P0, Zone::Hand, "Test Sage Walker");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(with_loyalty(sc.build(), first, 3));
    s.cast("Test Sage Walker").pay().pass_all();
    // The controller chooses one to keep.
    assert!(matches!(s.kind(), ChoiceKind::LegendRule));
    s.respond(Response::Pick(vec![0]));
    let left =
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| s.name(id) == "Test Sage Walker")
            .count();
    assert_eq!(left, 1);
}

// CR 306.5b, 614.1c, 606.4 (2026-11-06): the starting loyalty is a replacement effect a
// counter doubler modifies; so is a "+1" cost ("this cost may be modified by other
// effects").
#[test]
fn cr_606_4_doublers_and_loyalty() {
    let mut sc = scenario(&[WALKER, SEASON]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains", "Plains"]);
    sc.card(P0, Zone::Battlefield, "Test Counter Season");
    sc.card(P0, Zone::Hand, "Test Sage Walker");
    sc.turn(P0, Step::PrecombatMain);
    let mut s = Script::new(sc.build());
    s.cast("Test Sage Walker").pay().pass_all();
    let pw = s.find(P0, Zone::Battlefield, "Test Sage Walker").unwrap();
    assert_eq!(loyalty(&s.st, pw), 6);
    activate(&mut s, pw, 1);
    assert_eq!(loyalty(&s.st, pw), 8);
}

// CR 114 (emblems) and CR 605.1a (a loyalty ability that adds mana isn't a mana ability)
// aren't built: named.
#[test]
fn emblems_and_mana_loyalty_abilities_are_blocked() {
    let emblem = WALKER.replace(
        "A:AB$ Draw | Cost$ AddCounter<0/LOYALTY> | Planeswalker$ True | Ultimate$ True | NumCards$ 1 | SpellDescription$ Draw a card.",
        "A:AB$ Effect | Cost$ SubCounter<6/LOYALTY> | Planeswalker$ True | Name$ Emblem — Test | StaticAbilities$ STPump | Duration$ Permanent | SpellDescription$ You get an emblem.\n\
         SVar:STPump:Mode$ Continuous | Affected$ Creature.YouCtrl | AddPower$ 1 | Description$ Creatures you control get +1/+0.",
    );
    assert!(
        forge_test_blockers(&emblem).contains(&"emblem (CR 114)".to_string()),
        "{:?}",
        forge_test_blockers(&emblem)
    );
    let mana = WALKER.replace(
        "A:AB$ GainLife | Cost$ AddCounter<1/LOYALTY> | Planeswalker$ True | LifeAmount$ 2 | SpellDescription$ You gain 2 life.",
        "A:AB$ Mana | Cost$ AddCounter<1/LOYALTY> | Planeswalker$ True | Produced$ R | Amount$ 2 | SpellDescription$ Add {R}{R}.",
    );
    assert!(
        forge_test_blockers(&mana)
            .iter()
            .any(|b| b.starts_with("loyalty:adds mana")),
        "{:?}",
        forge_test_blockers(&mana)
    );
    let _ = Target::Player(P0);
}

/// What the heuristic agent answers P0's pending decision with (it sees only P0's view).
fn heuristic(s: &Script) -> OptionDesc {
    use engine_core::Agent;
    let mut brain = ai::HeuristicAgent::new(3, s.st.catalog.clone());
    let d = s.decision().clone();
    let r = brain.decide(&engine_core::player_view(&s.st, P0), &d);
    match r {
        Response::Pick(v) if v.len() == 1 => d.options[v[0]].clone(),
        other => panic!("{other:?}"),
    }
}

// D-086: the heuristic agent attacks an opposing planeswalker its attacker can kill when
// hitting the player isn't better, and uses a loyalty ability on its turn.
#[test]
fn the_heuristic_agent_attacks_planeswalkers_and_uses_loyalty_abilities() {
    let mut sc = scenario(&[WALKER, BRUISER]);
    let theirs = sc.card(P1, Zone::Battlefield, "Test Sage Walker");
    let mine = sc.card(P0, Zone::Battlefield, "Test Sage Walker");
    let ogre = sc.card(P0, Zone::Battlefield, "Test Walker Ogre");
    sc.turn(P0, Step::PrecombatMain);
    let st = with_loyalty(sc.build(), theirs, 3);
    let mut s = Script::new(with_loyalty(st, mine, 3));
    // Its own planeswalker: "+1" rather than nothing.
    assert!(
        matches!(heuristic(&s), OptionDesc::ActivateAbility { source, ability } if source == mine && ability.index == 1),
        "{:?}",
        heuristic(&s)
    );
    to_attack(&mut s, ogre);
    assert_eq!(
        heuristic(&s),
        OptionDesc::AttackPlaneswalker {
            attacker: ogre,
            planeswalker: theirs
        }
    );
}
