//! The most played gaps of the EDHREC top 1,000 (decisions.md D-082): changing targets, …
//! Invented cards in Forge's script format, compiled the way real ones are and then played.
//! Rules text: docs/rules-notes/d082-top1000.md.

use engine_core::stack::Target;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, Event, GameConfig, OptionDesc, PlayerId, Primitive, Response};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn card(name: &str, cost: &str, types: &str, body: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{types}\n{body}\nOracle:Text.")
}

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc.turn(P0, Step::PrecombatMain);
    sc
}

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

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

fn offered_targets(s: &Script) -> Vec<Target> {
    s.options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Target { target } => Some(*target),
            _ => None,
        })
        .collect()
}

fn sear() -> String {
    card(
        "Test Arc Sear",
        "X R",
        "Instant",
        "A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ X | SpellDescription$ Text.\nSVar:X:Count$xPaid",
    )
}

fn redirect() -> String {
    card(
        "Test Rerouting",
        "U U",
        "Instant",
        "A:SP$ ChangeTargets | TargetType$ Spell | Optional$ True | ValidTgts$ Card | SpellDescription$ Text.",
    )
}

fn misdirect() -> String {
    card(
        "Test Misaim",
        "U",
        "Instant",
        "A:SP$ ChangeTargets | TargetType$ Spell.singleTarget | ValidTgts$ Card | SpellDescription$ Text.",
    )
}

// CR 115.7d (2026-11-06): "If an effect allows a player to 'choose new targets' for a spell
// or ability, the player may leave any number of the targets unchanged, even if those
// targets would be illegal. If the player chooses to change some or all of the targets, the
// new targets must be legal …" The spell keeps its X. Forge: `SP$ ChangeTargets | TargetType$
// Spell | Optional$ True`.
#[test]
fn choose_new_targets_for_target_spell() {
    let bear = card("Test Bystander", "1 G", "Creature Bear", "PT:2/2");
    let mut sc = scenario(&[&sear(), &redirect(), &bear]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    sc.card(P0, Zone::Hand, "Test Arc Sear");
    sc.cards(P1, Zone::Battlefield, &["Island", "Island"]);
    sc.card(P1, Zone::Battlefield, "Test Bystander");
    sc.card(P1, Zone::Hand, "Test Rerouting");
    let mut s = Script::new(sc.build());
    s.cast("Test Arc Sear").x(2).target_player(P1).pay().pass();
    s.cast("Test Rerouting")
        .target("Test Arc Sear")
        .pay()
        .pass_all();
    assert_eq!(
        s.kind(),
        ChoiceKind::NewTarget {
            object: s.find(P0, Zone::Stack, "Test Arc Sear").unwrap(),
            slot: 0
        }
    );
    assert_eq!(s.who(), P1, "the Rerouting's controller chooses");
    // The current target first (to keep it), then every other legal one.
    let offered = offered_targets(&s);
    assert_eq!(offered[0], Target::Player(P1));
    assert!(offered.contains(&Target::Player(P0)));
    assert!(
        offered.iter().any(|t| matches!(t, Target::Object(_))),
        "the creature"
    );
    s.target_player(P0);
    assert_eq!(s.count(|e| matches!(e, Event::TargetsChanged { .. })), 1);
    s.pass_all();
    assert_eq!(
        life(&s, P0),
        38,
        "the spell kept X = 2 and hit its new target"
    );
    assert_eq!(life(&s, P1), 40);
    // Keeping the target is allowed (nothing changes).
    let mut sc = scenario(&[&sear(), &redirect()]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    sc.card(P0, Zone::Hand, "Test Arc Sear");
    sc.cards(P1, Zone::Battlefield, &["Island", "Island"]);
    sc.card(P1, Zone::Hand, "Test Rerouting");
    let mut s = Script::new(sc.build());
    s.cast("Test Arc Sear").x(1).target_player(P1).pay().pass();
    s.cast("Test Rerouting")
        .target("Test Arc Sear")
        .pay()
        .pass_all();
    s.target_player(P1);
    assert_eq!(s.count(|e| matches!(e, Event::TargetsChanged { .. })), 0);
    s.pass_all();
    assert_eq!(life(&s, P1), 39);
}

// CR 115.7a: "each target can be changed only to another legal target. If a target can't be
// changed to another legal target, the original target is unchanged". "Target spell with a
// single target": a spell with two targets can't be chosen. Forge: `TargetType$
// Spell.singleTarget`.
#[test]
fn change_the_target_of_a_spell_with_a_single_target() {
    let twin = card(
        "Test Twin Sear",
        "R",
        "Instant",
        "A:SP$ DealDamage | ValidTgts$ Player | TargetMin$ 2 | TargetMax$ 2 | NumDmg$ 1 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&sear(), &misdirect(), &twin]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Arc Sear", "Test Twin Sear"]);
    sc.cards(P1, Zone::Battlefield, &["Island"]);
    sc.card(P1, Zone::Hand, "Test Misaim");
    let mut s = Script::new(sc.build());
    // A spell with two targets: not a legal target for "a spell with a single target".
    s.cast("Test Twin Sear")
        .target_player(P0)
        .target_player(P1)
        .pay();
    s.cast("Test Arc Sear").x(1).target_player(P1).pay().pass();
    s.cast("Test Misaim");
    let twin_id = s.find(P0, Zone::Stack, "Test Twin Sear").unwrap();
    let arc_id = s.find(P0, Zone::Stack, "Test Arc Sear").unwrap();
    let offered = offered_targets(&s);
    assert!(offered.contains(&Target::Object(arc_id)));
    assert!(!offered.contains(&Target::Object(twin_id)));
    s.target_object(arc_id).pay().pass_all();
    // Not "keep": the only other legal target, P0, is chosen without a question.
    assert_eq!(
        s.count(|e| matches!(e, Event::TargetsChanged { object } if *object == arc_id)),
        1
    );
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(
        life(&s, P0),
        38,
        "1 from the Arc Sear, 1 from the Twin Sear"
    );
    assert_eq!(life(&s, P1), 39, "only the Twin Sear");
    // The text.
    let cat = forge_test_catalog(&[&misdirect()], &[]);
    let text = card_model::describe::rules_text(cat.get(cat.find("Test Misaim").unwrap()));
    assert_eq!(
        text,
        ["Change the target of target spell with a single target."]
    );
}

// CR 115.7 for an ability: "you may choose new targets for target spell or ability" (Forge:
// `TargetType$ Spell,Activated,Triggered`). The new target must be legal for the ability (a
// creature for "target creature").
#[test]
fn choose_new_targets_for_an_ability() {
    let swat = card(
        "Test Swat Away",
        "R",
        "Instant",
        "A:SP$ ChangeTargets | TargetType$ Spell,Activated,Triggered | Optional$ True | ValidTgts$ Card,Emblem | SpellDescription$ Text.",
    );
    let pinger = card(
        "Test Spark Imp",
        "R",
        "Creature Goblin",
        "PT:1/1\nA:AB$ DealDamage | Cost$ T | ValidTgts$ Creature | NumDmg$ 1 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&swat, &pinger]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Spark Imp");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Spark Imp");
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Test Swat Away");
    let mut s = Script::new(sc.build());
    s.activate("Test Spark Imp").target_object(theirs).pass();
    s.cast("Test Swat Away").pick("the ability", |_, o| {
        matches!(
            o,
            OptionDesc::Target {
                target: Target::Object(_)
            }
        )
    });
    s.pay().pass_all();
    let offered = offered_targets(&s);
    assert_eq!(offered[0], Target::Object(theirs), "keep");
    assert!(offered.contains(&Target::Object(mine)));
    assert!(
        !offered.iter().any(|t| matches!(t, Target::Player(_))),
        "only creatures"
    );
    s.target_object(mine);
    s.pass_all();
    assert!(
        !s.st.objects.contains_key(&mine),
        "the ability killed its own source"
    );
    assert!(s.st.objects.contains_key(&theirs));
    let _ = forge_test_blockers("Name:x");
}
