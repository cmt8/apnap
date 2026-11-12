//! Regeneration and "can't be regenerated" (decisions.md D-079): invented cards in Forge's
//! script format, compiled the way real ones are and then played. Rules text:
//! docs/rules-notes/701-regenerate.md.

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

fn card(name: &str, cost: &str, types: &str, body: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{types}\n{body}\nOracle:Text.")
}

fn troll() -> String {
    card(
        "Test Troll",
        "1 G",
        "Creature Troll\nPT:2/2",
        "A:AB$ Regenerate | Cost$ G | SpellDescription$ Text.",
    )
}

fn doom() -> String {
    card(
        "Test Ruin",
        "B",
        "Instant",
        "A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Text.",
    )
}

fn pure_doom() -> String {
    card(
        "Test Pure Doom",
        "B",
        "Instant",
        "A:SP$ Destroy | ValidTgts$ Creature | NoRegen$ True | SpellDescription$ Text.",
    )
}

fn zap() -> String {
    card(
        "Test Zap",
        "R",
        "Instant",
        "A:SP$ DealDamage | ValidTgts$ Creature | NumDmg$ 3 | SpellDescription$ Text.",
    )
}

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc.turn(P0, Step::PrecombatMain);
    sc
}

fn regenerated(s: &Script, id: ObjectId) -> usize {
    s.count(|e| matches!(e, Event::Regenerated { object } if *object == id))
}

/// Activate the Troll's regeneration and let it resolve.
fn shield(s: &mut Script) {
    s.activate("Test Troll").pay().pass_all();
}

// CR 701.19a (2026-11-06): "The next time [permanent] would be destroyed this turn, instead
// remove all damage marked on it and its controller taps it." CR 614.8: a
// destruction-replacement effect; CR 701.8c. Forge: `AB$ Regenerate` (the host by default).
// The shield is used once: a second destruction destroys it.
#[test]
fn a_shield_replaces_destruction_once() {
    let mut sc = scenario(&[&troll(), &doom()]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Swamp", "Swamp"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    sc.cards(P0, Zone::Hand, &["Test Ruin", "Test Ruin"]);
    let mut s = Script::new(sc.build());
    shield(&mut s);
    assert_eq!(s.st.shields.len(), 1);
    assert!(!s.st.object(t).tapped);
    s.cast("Test Ruin").target("Test Troll").pay().pass_all();
    assert!(s.st.objects.contains_key(&t), "it regenerated");
    assert!(s.st.object(t).tapped, "its controller tapped it");
    assert_eq!(regenerated(&s, t), 1);
    assert!(s.st.shields.is_empty(), "used up");
    s.cast("Test Ruin").target("Test Troll").pay().pass_all();
    assert!(!s.st.objects.contains_key(&t));
    assert!(s.find(P0, Zone::Graveyard, "Test Troll").is_some());
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Graveyard, "Test Troll").unwrap()),
    );
    assert_eq!(text, ["{G}: Regenerate this creature."]);
}

// CR 704.5g / 704.5h, 702.2b: lethal damage and damage from a source with deathtouch
// destroy the creature — "Regeneration can replace this event". All damage is removed.
#[test]
fn a_shield_replaces_lethal_damage_and_deathtouch() {
    let fang = card(
        "Test Fang",
        "B",
        "Creature Snake\nPT:1/1",
        "K:Deathtouch\nA:AB$ DealDamage | Cost$ T | ValidTgts$ Creature | NumDmg$ 1 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&troll(), &zap(), &fang]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Mountain"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    sc.card(P0, Zone::Hand, "Test Zap");
    let mut s = Script::new(sc.build());
    shield(&mut s);
    s.cast("Test Zap").target("Test Troll").pay().pass_all();
    assert!(s.st.objects.contains_key(&t));
    assert_eq!(s.st.object(t).damage_marked, 0, "all damage removed");
    assert_eq!(regenerated(&s, t), 1);
    // Deathtouch.
    let mut sc = scenario(&[&troll(), &fang]);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    sc.card(P1, Zone::Battlefield, "Test Fang");
    let mut s = Script::new(sc.build());
    shield(&mut s);
    s.pass();
    // P1 pings the Troll with the deathtouch creature.
    s.activate("Test Fang").target("Test Troll").pass_all();
    assert!(s.st.objects.contains_key(&t), "it regenerated");
    assert_eq!(regenerated(&s, t), 1);
    assert_eq!(s.st.object(t).damage_marked, 0);
}

// CR 701.21a: "Sacrificing a permanent doesn't destroy it, so regeneration … can't affect
// this action." CR 704.5f: toughness 0 or less — "Regeneration can't replace this event."
#[test]
fn a_shield_doesnt_replace_sacrifice_or_toughness_zero() {
    let altar = card(
        "Test Altar",
        "1",
        "Artifact",
        "A:AB$ GainLife | Cost$ Sac<1/Creature> | LifeAmount$ 1 | SpellDescription$ Text.",
    );
    let shrink = card(
        "Test Wither Away",
        "B",
        "Instant",
        "A:SP$ Pump | ValidTgts$ Creature | NumAtt$ -5 | NumDef$ -5 | IsCurse$ True | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&troll(), &altar, &shrink]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Swamp"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    sc.card(P0, Zone::Battlefield, "Test Altar");
    let mut s = Script::new(sc.build());
    shield(&mut s);
    s.activate("Test Altar");
    drive_until(&mut s, |s| s.kind() == ChoiceKind::Priority);
    s.pass_all();
    assert!(!s.st.objects.contains_key(&t), "sacrificed");
    assert_eq!(regenerated(&s, t), 0);
    // Toughness 0.
    let mut sc = scenario(&[&troll(), &shrink]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Swamp"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    sc.card(P0, Zone::Hand, "Test Wither Away");
    let mut s = Script::new(sc.build());
    shield(&mut s);
    s.cast("Test Wither Away")
        .target("Test Troll")
        .pay()
        .pass_all();
    assert!(!s.st.objects.contains_key(&t));
    assert_eq!(regenerated(&s, t), 0);
}

/// Answer everything with the least it allows until `done`.
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

// CR 701.19a: "If it's an attacking or blocking creature, remove it from combat." (CR
// 506.4.) A vigilant attacker (untapped while attacking) regenerates: it is tapped and is no
// longer attacking.
#[test]
fn a_regenerating_attacker_is_removed_from_combat() {
    let watcher = card(
        "Test Watch Troll",
        "1 G",
        "Creature Troll\nPT:2/2",
        "K:Vigilance\nA:AB$ Regenerate | Cost$ G | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&watcher, &doom()]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Swamp"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Watch Troll");
    sc.card(P0, Zone::Hand, "Test Ruin");
    sc.turn(P0, Step::BeginningOfCombat);
    let mut s = Script::new(sc.build());
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    let i = s
        .options()
        .iter()
        .position(|o| matches!(o, OptionDesc::Attack { attacker, .. } if *attacker == t))
        .expect("may attack");
    s.respond(Response::Pick(vec![i]));
    drive_until(&mut s, |s| {
        s.kind() == ChoiceKind::Priority && s.who() == P0
    });
    let attacking = |s: &Script| {
        s.st.combat
            .as_ref()
            .is_some_and(|c| c.attackers.iter().any(|a| a.attacker == t))
    };
    assert!(attacking(&s));
    s.activate("Test Watch Troll").pay().pass_all();
    s.cast("Test Ruin")
        .target("Test Watch Troll")
        .pay()
        .pass_all();
    assert!(s.st.objects.contains_key(&t));
    assert!(s.st.object(t).tapped);
    assert!(!attacking(&s), "removed from combat");
}

// CR 701.19a: "the next time [it] would be destroyed this turn": the shield is gone at the
// cleanup step (CR 514.2).
#[test]
fn a_shield_ends_at_cleanup() {
    let mut sc = scenario(&[&troll()]);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    sc.card(P0, Zone::Battlefield, "Test Troll");
    sc.turn(P0, Step::End);
    let mut s = Script::new(sc.build());
    shield(&mut s);
    assert_eq!(s.st.shields.len(), 1);
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert!(s.st.shields.is_empty());
}

// CR 701.19c: effects that say a permanent can't be regenerated "cause regeneration shields
// to not be applied" — the shield can still be made, and stays. Forge: `NoRegen$ True` on a
// `Destroy` / `DestroyAll`.
#[test]
fn cant_be_regenerated_on_a_destroy_instruction() {
    let wrath = card(
        "Test Final Wrath",
        "2 B",
        "Sorcery",
        "A:SP$ DestroyAll | ValidCards$ Creature | NoRegen$ True | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&troll(), &pure_doom(), &wrath]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Swamp"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    sc.card(P0, Zone::Hand, "Test Pure Doom");
    let mut s = Script::new(sc.build());
    shield(&mut s);
    s.cast("Test Pure Doom")
        .target("Test Troll")
        .pay()
        .pass_all();
    assert!(!s.st.objects.contains_key(&t));
    assert_eq!(regenerated(&s, t), 0);
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Graveyard, "Test Pure Doom").unwrap()),
    );
    assert_eq!(text, ["Destroy target creature. It can't be regenerated."]);
    // "Destroy all creatures. They can't be regenerated."
    let mut sc = scenario(&[&troll(), &wrath]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Swamp", "Swamp", "Swamp"],
    );
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    sc.card(P0, Zone::Hand, "Test Final Wrath");
    let mut s = Script::new(sc.build());
    shield(&mut s);
    s.cast("Test Final Wrath").pay().pass_all();
    assert!(!s.st.objects.contains_key(&t));
    assert_eq!(regenerated(&s, t), 0);
}

// CR 701.19c, 611.2: "Target creature can't be regenerated this turn" is a rule that lasts
// (a lasting effect about the creature), not an ability of it. Forge: `SP$ Effect |
// StaticAbilities$ NoRegen` with `Mode$ CantRegenerate | ValidCard$ Card.IsRemembered`.
#[test]
fn cant_be_regenerated_this_turn_is_a_lasting_rule() {
    let curse = card(
        "Test Unmending",
        "R",
        "Instant",
        "A:SP$ Effect | ValidTgts$ Creature | RememberObjects$ Targeted | ForgetOnMoved$ Battlefield | StaticAbilities$ NoRegen | SpellDescription$ Text.\n\
         SVar:NoRegen:Mode$ CantRegenerate | ValidCard$ Card.IsRemembered | Description$ Text.",
    );
    let mut sc = scenario(&[&troll(), &doom(), &curse]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Swamp", "Mountain"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    sc.cards(P0, Zone::Hand, &["Test Unmending", "Test Ruin"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Unmending")
        .target("Test Troll")
        .pay()
        .pass_all();
    // The shield can still be made (CR 701.19c) …
    shield(&mut s);
    assert_eq!(s.st.shields.len(), 1);
    // … but isn't applied.
    s.cast("Test Ruin").target("Test Troll").pay().pass_all();
    assert!(!s.st.objects.contains_key(&t));
    assert_eq!(regenerated(&s, t), 0);
    let text = card_model::describe::rules_text(
        s.st.card_def(s.find(P0, Zone::Graveyard, "Test Unmending").unwrap()),
    );
    assert_eq!(text, ["Target creature can't be regenerated this turn."]);
    // Not built yet: "when it regenerates this way" and static regeneration (701.19b).
    let mending = card(
        "Test Mending",
        "G",
        "Instant",
        "A:SP$ Regenerate | ValidTgts$ Creature | RegenerationAbility$ DBLife | SpellDescription$ Text.\n\
         SVar:DBLife:DB$ GainLife | LifeAmount$ 2",
    );
    assert!(
        forge_test_blockers(&mending)
            .iter()
            .any(|b| b.contains("RegenerationAbility")),
        "{:?}",
        forge_test_blockers(&mending)
    );
}

fn striker(name: &str) -> String {
    card(
        name,
        "2 R",
        "Creature Human Knight\nPT:3/3",
        "K:First Strike",
    )
}

/// Answer the least until `done`, but declare `attackers` and the `blocks` (blocker,
/// attacker) when asked.
fn fight(
    s: &mut Script,
    attackers: &[ObjectId],
    blocks: &[(ObjectId, ObjectId)],
    done: impl Fn(&Script) -> bool,
) {
    for _ in 0..5_000 {
        if s.next.is_none() || done(s) {
            return;
        }
        let pick = match s.kind() {
            ChoiceKind::DeclareAttackers => s.options().iter().position(
                |o| matches!(o, OptionDesc::Attack { attacker, .. } if attackers.contains(attacker)),
            ),
            ChoiceKind::DeclareBlockers => s.options().iter().position(|o| {
                matches!(o, OptionDesc::Block { blocker, attacker } if blocks.contains(&(*blocker, *attacker)))
            }),
            _ => None,
        };
        let r = match (pick, &s.decision().prim) {
            (Some(i), _) => Response::Pick(vec![i]),
            (None, Primitive::PickN { min, .. }) => Response::Pick((0..*min).collect()),
            (None, Primitive::PickNumber { min, .. }) => Response::Number(*min),
            (None, Primitive::Allocate { per_min, .. }) => Response::Allocate(per_min.clone()),
            (None, Primitive::Order) => Response::Order((0..s.options().len()).collect()),
        };
        s.respond(r);
    }
    panic!("fight did not converge");
}

// CR 701.19a (2026-11-06): "If it's an attacking or blocking creature, remove it from
// combat." CR 506.4: "A creature that's removed from combat stops being an attacking,
// blocking, blocked, and/or unblocked creature." CR 510.4 / 510.1: only creatures still in
// combat deal combat damage in the second combat damage step. A blocker with a shield takes
// lethal first-strike damage: it regenerates (tapped, no damage, out of combat) and deals no
// damage in the regular step; the attacker stays blocked (CR 509.1h), so the defending
// player takes nothing.
#[test]
fn a_regenerated_blocker_leaves_combat_and_deals_no_damage() {
    let mut sc = scenario(&[&troll(), &striker("Test Lancer")]);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    let lancer = sc.card(P1, Zone::Battlefield, "Test Lancer");
    sc.turn(P1, Step::BeginningOfCombat);
    let mut s = Script::new(sc.build());
    s.pass();
    // P0 makes the shield at the beginning of combat.
    shield(&mut s);
    fight(&mut s, &[lancer], &[(t, lancer)], |s| {
        s.st.turn.step == Step::EndOfCombat
    });
    assert!(s.st.objects.contains_key(&t), "it regenerated");
    assert_eq!(regenerated(&s, t), 1);
    assert!(s.st.object(t).tapped);
    assert_eq!(s.st.object(t).damage_marked, 0);
    assert_eq!(
        s.st.object(lancer).damage_marked,
        0,
        "the blocker dealt no damage"
    );
    assert_eq!(s.st.player(P0).life, 40, "the attacker stayed blocked");
}

// The same for an attacker: blocked by a first striker, it regenerates in the first-strike
// step, is no longer attacking, and deals no damage in the regular step.
#[test]
fn a_regenerated_attacker_leaves_combat_and_deals_no_damage() {
    let mut sc = scenario(&[&troll(), &striker("Test Lancer")]);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    let t = sc.card(P0, Zone::Battlefield, "Test Troll");
    let lancer = sc.card(P1, Zone::Battlefield, "Test Lancer");
    sc.turn(P0, Step::BeginningOfCombat);
    let mut s = Script::new(sc.build());
    shield(&mut s);
    fight(&mut s, &[t], &[(lancer, t)], |s| {
        s.st.turn.step == Step::EndOfCombat
    });
    assert!(s.st.objects.contains_key(&t));
    assert_eq!(regenerated(&s, t), 1);
    assert_eq!(s.st.object(t).damage_marked, 0);
    assert_eq!(
        s.st.object(lancer).damage_marked,
        0,
        "the attacker dealt no damage"
    );
    assert_eq!(s.st.player(P1).life, 40);
    let attacking =
        s.st.combat
            .as_ref()
            .is_some_and(|c| c.attackers.iter().any(|a| a.attacker == t));
    assert!(!attacking, "no longer attacking");
}

// CR 118.12 with CR 701.19c: "you may pay {U}. If you don't, destroy target creature. It
// can't be regenerated." The rider is part of what happens if nothing is paid; the text must
// not read as though paying avoided the rider. Forge: `NoRegen$ True | UnlessCost$ U |
// UnlessPayer$ You`.
#[test]
fn a_rider_inside_an_unless_cost_reads_as_part_of_the_unpaid_instruction() {
    let mist = card(
        "Test Mist Rider",
        "2 U",
        "Creature Human Knight\nPT:2/2",
        "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigDestroy | TriggerDescription$ Text.\n\
         SVar:TrigDestroy:DB$ Destroy | ValidTgts$ Creature | NoRegen$ True | UnlessCost$ U | UnlessPayer$ You",
    );
    let cat = forge_test_catalog(&[&mist], &[]);
    let def = cat.get(cat.find("Test Mist Rider").unwrap());
    let text = card_model::describe::rules_text(def).join(" ");
    assert!(
        text.contains(
            "you may pay {U}. If you don't, destroy target creature. It can't be regenerated."
        ),
        "{text}"
    );
}
