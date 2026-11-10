//! What each script feature the Forge compiler accepts *means* (CLAUDE.md invariant 15,
//! decisions.md D-046): invented cards written in Forge's script format are compiled the
//! way real ones are and then played. One test per feature, not per card.

use engine_core::layers::chars;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{GameConfig, ObjectId, PlayerId};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scenario(cards: &[&str]) -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(cards, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

fn pt(s: &Script, id: ObjectId) -> (i32, i32) {
    let c = chars(&s.st, id);
    (
        c.power.expect("has power"),
        c.toughness.expect("has toughness"),
    )
}

// CR 608.2h (2026-11-06): an amount an effect needs is determined once, as it is applied.
// CR 611.2c: the set of objects a resolving spell's continuous effect affects is fixed too.
// Forge: `NumAtt$ +X` with `SVar:X:Count$Valid …`.
#[test]
fn pump_all_by_a_counted_amount_is_fixed_on_resolution() {
    let rally = "Name:Test Muster\n\
        ManaCost:1 W\n\
        Types:Sorcery\n\
        A:SP$ PumpAll | Cost$ 1 W | ValidCards$ Creature.YouCtrl | NumAtt$ +X | NumDef$ +X | SpellDescription$ Text.\n\
        SVar:X:Count$Valid Creature.YouCtrl\n\
        Oracle:Text.";
    let mut sc = scenario(&[rally]);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains", "Forest"]);
    let golems = sc.cards(P0, Zone::Battlefield, &["Test Golem"; 2]); // 2/3
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Muster", "Test Elf"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    for &g in &golems {
        assert_eq!(pt(&s, g), (4, 5), "two creatures: +2/+2");
    }
    assert_eq!(pt(&s, theirs), (2, 3));
    // A third creature arrives: the bonus neither grows nor reaches it.
    s.cast("Test Elf").pay().pass_all();
    let elf = s.find(P0, Zone::Battlefield, "Test Elf").unwrap();
    assert_eq!(pt(&s, elf), (1, 1));
    assert_eq!(pt(&s, golems[0]), (4, 5));
}

// Forge: `NumAtt$ -X | NumDef$ -X` on a targeted Pump: "-X/-X", the sign is the script's.
#[test]
fn pump_by_a_negative_counted_amount() {
    let sap = "Name:Test Sapping\n\
        ManaCost:B\n\
        Types:Instant\n\
        A:SP$ Pump | Cost$ B | ValidTgts$ Creature | NumAtt$ -X | NumDef$ -X | IsCurse$ True | SpellDescription$ Text.\n\
        SVar:X:Count$Valid Land.YouCtrl\n\
        Oracle:Text.";
    let mut sc = scenario(&[sap]);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
    let giant = sc.card(P1, Zone::Battlefield, "Test Giant"); // 5/4
    sc.card(P0, Zone::Hand, "Test Sapping");
    let mut s = Script::new(sc.build());
    s.cast("Test Sapping").target("Test Giant").pay().pass_all();
    assert_eq!(pt(&s, giant), (3, 2), "two lands: -2/-2");
}

// Forge: a spell's `Cost$` restates the whole cost. Mana that differs from `ManaCost:` is
// some other way of casting the spell, not an additional cost.
#[test]
fn a_spell_cost_with_different_mana_blocks() {
    let card = "Name:Test Bargain\n\
        ManaCost:1 B\n\
        Types:Sorcery\n\
        A:SP$ Draw | Cost$ 2 B | NumCards$ 2 | SpellDescription$ Text.\n\
        Oracle:Text.";
    assert_eq!(forge_test_blockers(card), ["spell:other-cost"]);
    let plain = card.replace("Cost$ 2 B", "Cost$ 1 B");
    assert_eq!(forge_test_blockers(&plain), [] as [&str; 0]);
}

// ---------------------------------------------------------------------------------------
// Helpers for the tests below.

use card_model::Keyword;
use engine_core::turn::Step;
use engine_core::{ChoiceKind, OptionDesc, Primitive, Response};

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

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

fn hand(s: &Script, p: PlayerId) -> usize {
    s.st.zones.list(Zone::Hand, p).len()
}

fn on_battlefield(s: &Script, id: ObjectId) -> bool {
    s.st.objects
        .get(&id)
        .is_some_and(|o| o.zone == Zone::Battlefield)
}

fn can_target(s: &Script, id: ObjectId) -> bool {
    use engine_core::stack::Target;
    s.has(|_, o| matches!(o, OptionDesc::Target { target: Target::Object(t) } if *t == id))
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

/// Go to the declaration of attackers; attack with `attackers`. Returns every creature the
/// active player was asked about.
fn attack_with(s: &mut Script, attackers: &[ObjectId]) -> Vec<ObjectId> {
    let mut asked = Vec::new();
    drive_until(s, |s| {
        s.kind() == ChoiceKind::DeclareAttackers || s.st.turn.step == Step::PostcombatMain
    });
    while s.next.is_some() && s.kind() == ChoiceKind::DeclareAttackers {
        let a = match s.options().first() {
            Some(OptionDesc::Attack { attacker, .. }) => *attacker,
            other => panic!("{other:?}"),
        };
        asked.push(a);
        let pick = if attackers.contains(&a) {
            vec![0]
        } else {
            vec![]
        };
        s.respond(Response::Pick(pick));
    }
    asked
}

/// After attackers are declared: the names of the creatures that are offered a block of
/// `attacker` (none of them blocks).
fn offered_blockers(s: &mut Script, attacker: ObjectId) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        drive_until(s, |s| {
            s.kind() == ChoiceKind::DeclareBlockers || s.st.turn.step == Step::PostcombatMain
        });
        if s.next.is_none() || s.kind() != ChoiceKind::DeclareBlockers {
            return out;
        }
        for o in s.options() {
            if let OptionDesc::Block {
                blocker,
                attacker: a,
            } = o
                && *a == attacker
            {
                out.push(s.name(*blocker));
            }
        }
        s.respond(Response::Pick(vec![]));
    }
}

fn creature(name: &str, cost: &str, types: &str, pt: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:Creature {types}\nPT:{pt}\n{rest}\nOracle:Text.")
}

fn spell(name: &str, cost: &str, kind: &str, rest: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:{kind}\n{rest}\nOracle:Text.")
}

const SMITE: &str = "Name:Test Smiting\nManaCost:B\nTypes:Instant\n\
    A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Text.\nOracle:Text.";
const ZAP: &str = "Name:Test Zapping\nManaCost:R\nTypes:Instant\n\
    A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 3 | SpellDescription$ Text.\nOracle:Text.";

// ---------------------------------------------------------------------------------------
// Keywords.

// CR 702.3b (2026-11-06): a creature with defender can't attack.
#[test]
fn defender_cant_attack() {
    let wall = creature("Test Rampart", "1 W", "Wall", "0/4", "K:Defender");
    let mut sc = scenario(&[&wall]);
    let wall = sc.card(P0, Zone::Battlefield, "Test Rampart");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let mut s = Script::new(sc.build());
    assert!(chars(&s.st, wall).has(Keyword::Defender));
    let asked = attack_with(&mut s, &[]);
    assert_eq!(asked, [golem], "only the Golem may be declared");
}

// CR 702.12b (2026-11-06): can't be destroyed — not by "destroy", not by lethal damage
// (704.5g). CR 701.21a: sacrificing isn't destroying.
#[test]
fn indestructible_survives_destruction_and_lethal_damage_but_not_sacrifice() {
    let monolith = creature("Test Monolith", "4", "Golem", "2/2", "K:Indestructible");
    let decree = spell(
        "Test Decree",
        "B",
        "Sorcery",
        "A:SP$ Sacrifice | ValidTgts$ Player | SacValid$ Creature | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&monolith, SMITE, ZAP, &decree]);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp", "Mountain"]);
    let monolith = sc.card(P1, Zone::Battlefield, "Test Monolith");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Smiting", "Test Zapping", "Test Decree"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Smiting")
        .target_object(monolith)
        .pay()
        .pass_all();
    assert!(on_battlefield(&s, monolith), "destroy does nothing");
    s.cast("Test Zapping")
        .target_object(monolith)
        .pay()
        .pass_all();
    assert!(
        on_battlefield(&s, monolith),
        "lethal damage doesn't destroy it"
    );
    assert_eq!(s.st.object(monolith).damage_marked, 3);
    s.cast("Test Decree").target_player(P1).pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P1, ChoiceKind::Sacrifice));
    s.respond(Response::Pick(vec![0]));
    assert!(!on_battlefield(&s, monolith), "it can be sacrificed");
    assert!(s.find(P1, Zone::Graveyard, "Test Monolith").is_some());
}

// CR 702.11b (2026-11-06): hexproof — not a target for opponents; its controller may
// target it. CR 702.18a: shroud — not a target for anyone.
#[test]
fn hexproof_and_shroud_restrict_targeting() {
    let stalker = creature("Test Stalker", "1 G", "Beast", "2/2", "K:Hexproof");
    let hermit = creature("Test Hermit", "1 G", "Beast", "2/2", "K:Shroud");
    let mut sc = scenario(&[&stalker, &hermit, SMITE]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Stalker");
    let my_hermit = sc.card(P0, Zone::Battlefield, "Test Hermit");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Stalker");
    let their_golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Smiting");
    let mut s = Script::new(sc.build());
    s.cast("Test Smiting");
    assert!(can_target(&s, mine), "my own hexproof creature");
    assert!(!can_target(&s, theirs), "an opponent's hexproof creature");
    assert!(!can_target(&s, my_hermit), "shroud, even my own");
    assert!(can_target(&s, their_golem));
}

// CR 702.36b (fear), 702.13b (intimidate), 702.14c (landwalk) (2026-11-06).
#[test]
fn fear_intimidate_and_landwalk_limit_who_may_block() {
    let cards = [
        creature("Test Phantom", "2 B", "Spirit", "2/2", "K:Fear"),
        creature("Test Bully", "2 R", "Ogre", "3/3", "K:Intimidate"),
        creature("Test Marsher", "2 B", "Zombie", "2/2", "K:Landwalk:Swamp"),
        creature("Test Ghoul", "B", "Zombie", "2/2", ""),
        creature("Test Firebrand", "R", "Goblin", "1/1", ""),
        "Name:Test Automaton\nManaCost:2\nTypes:Artifact Creature Construct\nPT:1/1\nOracle:"
            .to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let blockers = [
        "Test Golem",
        "Test Ghoul",
        "Test Firebrand",
        "Test Automaton",
    ];
    let setup = |attacker: &str, swamp: bool| {
        let mut sc = scenario(&cards);
        let a = sc.card(P0, Zone::Battlefield, attacker);
        sc.cards(P1, Zone::Battlefield, &blockers);
        if swamp {
            sc.card(P1, Zone::Battlefield, "Swamp");
        }
        let mut s = Script::new(sc.build());
        attack_with(&mut s, &[a]);
        offered_blockers(&mut s, a)
    };
    // Fear: artifact creatures and black creatures.
    assert_eq!(
        setup("Test Phantom", false),
        ["Test Ghoul", "Test Automaton"]
    );
    // Intimidate (a red creature): artifact creatures and red creatures.
    assert_eq!(
        setup("Test Bully", false),
        ["Test Firebrand", "Test Automaton"]
    );
    // Swampwalk: anyone, unless the defending player controls a Swamp.
    assert_eq!(setup("Test Marsher", false), blockers);
    assert_eq!(setup("Test Marsher", true), [] as [&str; 0]);
}

// ---------------------------------------------------------------------------------------
// Effects.

// CR 701.6a (2026-11-06). Forge: `Counter | TargetType$ Spell | ValidTgts$ Creature` —
// "counter target creature spell".
#[test]
fn counter_target_spell_of_a_kind() {
    let dispel = spell(
        "Test Rebuff",
        "U",
        "Instant",
        "A:SP$ Counter | TargetType$ Spell | ValidTgts$ Creature | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&dispel, ZAP]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Mountain"]);
    sc.cards(P1, Zone::Battlefield, &["Island", "Island"]);
    sc.cards(P0, Zone::Hand, &["Test Elf", "Test Zapping"]);
    sc.cards(P1, Zone::Hand, &["Test Rebuff", "Test Rebuff"]);
    let mut s = Script::new(sc.build());
    // An instant isn't a creature spell: nothing to target.
    s.cast("Test Zapping").target_player(P1).pay().pass();
    assert_eq!(s.who(), P1);
    assert!(!s.can_cast("Test Rebuff"));
    s.pass();
    assert_eq!(life(&s, P1), 37);
    // A creature spell is countered: it goes to its owner's graveyard.
    s.cast("Test Elf").pay().pass();
    assert_eq!(s.who(), P1);
    let elf = s.find(P0, Zone::Stack, "Test Elf").unwrap();
    s.cast("Test Rebuff").target_object(elf).pay().pass_all();
    assert!(s.find(P0, Zone::Graveyard, "Test Elf").is_some());
    assert!(s.find(P0, Zone::Battlefield, "Test Elf").is_none());
}

// CR 701.26 (2026-11-06). Forge: Tap / Untap (a target, or the card itself by default),
// TapAll / UntapAll (`ValidCards$`).
#[test]
fn tap_and_untap_effects() {
    let cards = [
        spell(
            "Test Freeze",
            "U",
            "Instant",
            "A:SP$ Tap | ValidTgts$ Creature | SpellDescription$ Text.",
        ),
        spell(
            "Test Thaw",
            "U",
            "Instant",
            "A:SP$ Untap | ValidTgts$ Creature | SpellDescription$ Text.",
        ),
        spell(
            "Test Lull",
            "U",
            "Instant",
            "A:SP$ TapAll | ValidCards$ Creature.OppCtrl | SpellDescription$ Text.",
        ),
        spell(
            "Test Reveille",
            "U",
            "Instant",
            "A:SP$ UntapAll | ValidCards$ Creature.YouCtrl | SpellDescription$ Text.",
        ),
        creature(
            "Test Napper",
            "1 U",
            "Drake",
            "1/1",
            "A:AB$ Untap | Cost$ U | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island"; 6]);
    let napper = sc.card(P0, Zone::Battlefield, "Test Napper");
    let mine = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.cards(P1, Zone::Battlefield, &["Test Golem", "Test Elf"]);
    sc.tapped(napper);
    sc.tapped(mine);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Freeze", "Test Thaw", "Test Lull", "Test Reveille"],
    );
    let mut s = Script::new(sc.build());
    let tapped = |s: &Script, id: ObjectId| s.st.object(id).tapped;
    s.activate("Test Napper").pay().pass_all();
    assert!(!tapped(&s, napper), "untaps itself");
    s.cast("Test Freeze")
        .target_object(theirs[0])
        .pay()
        .pass_all();
    assert!(tapped(&s, theirs[0]) && !tapped(&s, theirs[1]));
    s.cast("Test Thaw")
        .target_object(theirs[0])
        .pay()
        .pass_all();
    assert!(!tapped(&s, theirs[0]));
    s.cast("Test Lull").pay().pass_all();
    assert!(tapped(&s, theirs[0]) && tapped(&s, theirs[1]));
    assert!(!tapped(&s, napper), "only the opponents' creatures");
    assert!(tapped(&s, mine), "still tapped from before");
    s.cast("Test Reveille").pay().pass_all();
    assert!(!tapped(&s, mine));
    assert!(tapped(&s, theirs[0]), "only mine untap");
}

// Forge: `DealDamage | Defined$ …` — damage to players and permanents that aren't targets.
// Each opponent is dealt the damage at once; "its controller" is the controller of the
// permanent whose event triggered the ability, as it last was (CR 608.2h).
#[test]
fn damage_to_defined_players_and_permanents() {
    let etb = "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigDmg | TriggerDescription$ Text.\n\
        SVar:TrigDmg:DB$ DealDamage | Defined$ Player.Opponent | NumDmg$ 2";
    let dies = "T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Creature | TriggerZones$ Battlefield | Execute$ TrigDmg | TriggerDescription$ Text.\n\
        SVar:TrigDmg:DB$ DealDamage | Defined$ TriggeredCardController | NumDmg$ 2";
    let cards = [
        creature("Test Scorcher", "R", "Elemental", "1/1", etb),
        format!("Name:Test Tollstone\nManaCost:4\nTypes:Artifact\n{dies}\nOracle:Text."),
        spell(
            "Test Backfire",
            "R",
            "Sorcery",
            "A:SP$ DealDamage | ValidTgts$ Creature | NumDmg$ 2 | SubAbility$ DBSelf | SpellDescription$ Text.\n\
             SVar:DBSelf:DB$ DealDamage | Defined$ You | NumDmg$ 1",
        ),
        creature(
            "Test Flagellant",
            "1 R",
            "Human",
            "2/2",
            "A:AB$ DealDamage | Cost$ R | Defined$ Self | NumDmg$ 1 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario_n(3, &cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 4]);
    sc.card(P0, Zone::Battlefield, "Test Tollstone");
    let flagellant = sc.card(P0, Zone::Battlefield, "Test Flagellant");
    let elf = sc.card(P2, Zone::Battlefield, "Test Elf");
    sc.cards(P0, Zone::Hand, &["Test Scorcher", "Test Backfire"]);
    let mut s = Script::new(sc.build());
    // "…deals 2 damage to each opponent."
    s.cast("Test Scorcher").pay().pass_all();
    s.pass_all();
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (40, 38, 38));
    // 2 to the Elf and 1 to you; the Elf dies and its controller (P2) is dealt 2.
    s.cast("Test Backfire").target_object(elf).pay().pass_all();
    s.pass_all();
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (39, 38, 36));
    // "…deals 1 damage to itself", twice: it dies, and its controller (P0) is dealt 2.
    s.activate("Test Flagellant").pay().pass_all();
    assert_eq!(s.st.object(flagellant).damage_marked, 1);
    s.activate("Test Flagellant").pay().pass_all();
    s.pass_all();
    assert!(!on_battlefield(&s, flagellant));
    assert_eq!(life(&s, P0), 37);
}

// Forge: `Defined$ Self` (the default) on Destroy and on a ChangeZone from the battlefield:
// "destroy this", "return this to its owner's hand". `Defined$ Player` on a life effect:
// each player.
#[test]
fn effects_on_the_card_itself_and_on_each_player() {
    let bounce = "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Creature.Other | TriggerZones$ Battlefield | Execute$ TrigReturn | TriggerDescription$ Text.\n\
        SVar:TrigReturn:DB$ ChangeZone | Defined$ Self | Origin$ Battlefield | Destination$ Hand";
    let cards = [
        creature("Test Recluse", "2 U", "Drake", "3/3", bounce),
        creature(
            "Test Doomed",
            "B",
            "Zombie",
            "2/2",
            "A:AB$ Destroy | Cost$ B | Defined$ Self | SpellDescription$ Text.",
        ),
        spell(
            "Test Tithe",
            "B",
            "Sorcery",
            "A:SP$ LoseLife | Defined$ Player | LifeAmount$ 3 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario_n(3, &cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp", "Forest"]);
    sc.card(P0, Zone::Battlefield, "Test Recluse");
    let doomed = sc.card(P0, Zone::Battlefield, "Test Doomed");
    sc.cards(P0, Zone::Hand, &["Test Elf", "Test Tithe"]);
    let mut s = Script::new(sc.build());
    s.activate("Test Doomed").pay().pass_all();
    assert!(!on_battlefield(&s, doomed));
    assert!(s.find(P0, Zone::Graveyard, "Test Doomed").is_some());
    s.cast("Test Elf").pay().pass_all();
    s.pass_all();
    assert!(s.find(P0, Zone::Hand, "Test Recluse").is_some());
    s.cast("Test Tithe").pay().pass_all();
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (37, 37, 37));
}

// CR 701.9a/b (2026-11-06): the discarding player chooses. CR 101.4: several players choose
// in APNAP order, then discard at once. Forge: `Discard | Mode$ TgtChoose` (and `Hand`).
#[test]
fn discard_by_target_player_each_opponent_and_whole_hand() {
    let cards = [
        spell(
            "Test Extortion",
            "B",
            "Sorcery",
            "A:SP$ Discard | ValidTgts$ Player | NumCards$ 2 | Mode$ TgtChoose | SpellDescription$ Text.",
        ),
        spell(
            "Test Rumors",
            "B",
            "Sorcery",
            "A:SP$ Discard | Defined$ Player.Opponent | NumCards$ 1 | Mode$ TgtChoose | SpellDescription$ Text.",
        ),
        spell(
            "Test Amnesia",
            "B",
            "Sorcery",
            "A:SP$ Discard | ValidTgts$ Player | Mode$ Hand | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario_n(3, &cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 3]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Extortion", "Test Rumors", "Test Amnesia"],
    );
    sc.cards(
        P1,
        Zone::Hand,
        &["Forest", "Test Elf", "Test Golem", "Island"],
    );
    sc.cards(P2, Zone::Hand, &["Forest", "Test Elf"]);
    let mut s = Script::new(sc.build());
    // Target player discards two: that player picks which.
    s.cast("Test Extortion").target_player(P1).pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P1, ChoiceKind::Discard));
    assert_eq!(s.decision().prim, Primitive::PickN { min: 2, max: 2 });
    let picks: Vec<usize> = s
        .options()
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            matches!(o, OptionDesc::Card { card } if ["Forest", "Island"].contains(&s.name(*card).as_str()))
        })
        .map(|(i, _)| i)
        .collect();
    s.respond(Response::Pick(picks));
    assert_eq!(hand(&s, P1), 2);
    assert!(s.find(P1, Zone::Hand, "Test Elf").is_some());
    assert!(s.find(P1, Zone::Graveyard, "Island").is_some());
    // Each opponent discards one: P1 chooses, then P2; the caster discards nothing.
    s.cast("Test Rumors").pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P1, ChoiceKind::Discard));
    s.respond(Response::Pick(vec![0]));
    assert_eq!((s.who(), s.kind()), (P2, ChoiceKind::Discard));
    assert_eq!(
        hand(&s, P1),
        2,
        "nothing is discarded until all have chosen"
    );
    s.respond(Response::Pick(vec![0]));
    assert_eq!((hand(&s, P0), hand(&s, P1), hand(&s, P2)), (1, 1, 1));
    // Target player discards their hand: no choice.
    s.cast("Test Amnesia").target_player(P2).pay().pass_all();
    assert_eq!((hand(&s, P1), hand(&s, P2)), (1, 0));
}

// CR 701.21a (2026-11-06): only the controller sacrifices, and chooses which. Forge:
// `Sacrifice | SacValid$ <filter>` for players, and with no `SacValid$` "sacrifice this".
#[test]
fn sacrifice_by_a_player_and_of_the_card_itself() {
    let etb_sac = "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigSac | TriggerDescription$ Text.\n\
        SVar:TrigSac:DB$ Sacrifice";
    let cards = [
        spell(
            "Test Decree",
            "B",
            "Sorcery",
            "A:SP$ Sacrifice | ValidTgts$ Opponent | SacValid$ Creature | SpellDescription$ Text.",
        ),
        spell(
            "Test Fallow",
            "G",
            "Sorcery",
            "A:SP$ Sacrifice | Defined$ You | SacValid$ Land | Amount$ 2 | SpellDescription$ Text.",
        ),
        creature("Test Mayfly", "G", "Insect", "3/3", etb_sac),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Forest", "Forest"]);
    sc.card(P0, Zone::Battlefield, "Test Golem");
    let elf = sc.card(P1, Zone::Battlefield, "Test Elf");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P1, Zone::Battlefield, "Island");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Decree", "Test Mayfly", "Test Fallow"],
    );
    let mut s = Script::new(sc.build());
    // The opponent chooses among their own creatures.
    s.cast("Test Decree").target_player(P1).pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P1, ChoiceKind::Sacrifice));
    assert_eq!(s.options().len(), 2, "their creatures, not their land");
    s.pick(
        "the Elf",
        |_, o| matches!(o, OptionDesc::Card { card } if *card == elf),
    );
    assert!(!on_battlefield(&s, elf) && on_battlefield(&s, golem));
    // "When this enters, sacrifice it."
    s.cast("Test Mayfly").pay().pass_all();
    s.pass_all();
    assert!(s.find(P0, Zone::Graveyard, "Test Mayfly").is_some());
    // "Sacrifice two lands": you have three, and choose.
    s.cast("Test Fallow").pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Sacrifice));
    assert_eq!(s.decision().prim, Primitive::PickN { min: 2, max: 2 });
    assert_eq!(s.options().len(), 3);
    s.respond(Response::Pick(vec![0, 1]));
    let lands =
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| {
                chars(&s.st, id).controller == P0 && chars(&s.st, id).is(card_model::CardType::Land)
            })
            .count();
    assert_eq!(lands, 1);
}

// CR 603.3a / 608.2h (2026-11-06). Forge writes a permanent's own "when this dies, draw a
// card" with `Defined$ TriggeredCardController`: the player who controlled it.
#[test]
fn the_controller_of_the_permanent_that_died() {
    let dies = "T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self | Execute$ TrigDraw | TriggerDescription$ Text.\n\
        SVar:TrigDraw:DB$ Draw | Defined$ TriggeredCardController | NumCards$ 1";
    let bird = creature("Test Halcyon", "3 U", "Bird", "2/2", dies);
    let mut sc = scenario(&[&bird, SMITE]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    let bird = sc.card(P1, Zone::Battlefield, "Test Halcyon");
    sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Smiting");
    let mut s = Script::new(sc.build());
    s.cast("Test Smiting").target_object(bird).pay().pass_all();
    s.pass_all();
    assert_eq!((hand(&s, P0), hand(&s, P1)), (0, 1), "its controller draws");
}

// Forge: `DamageAll | ValidPlayers$ …` with no `ValidCards$` is damage to players only.
// (A compiler that looked only for `ValidCards$` once produced a spell with no effect.)
#[test]
fn damage_all_to_players_only() {
    let cards = [
        spell(
            "Test Rift",
            "R",
            "Sorcery",
            "A:SP$ DamageAll | NumDmg$ 4 | ValidPlayers$ Player | ValidDescription$ each player. | SpellDescription$ Text.",
        ),
        spell(
            "Test Flare",
            "R",
            "Sorcery",
            "A:SP$ DamageAll | NumDmg$ 2 | ValidPlayers$ Player.Opponent | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario_n(3, &cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 2]);
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Rift", "Test Flare"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Rift").pay().pass_all();
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (36, 36, 36));
    assert_eq!(s.st.object(golem).damage_marked, 0, "players only");
    s.cast("Test Flare").pay().pass_all();
    assert_eq!((life(&s, P0), life(&s, P1), life(&s, P2)), (36, 34, 34));
    // Creatures and players together would be one simultaneous event: not compiled yet.
    let both = spell(
        "Test Quake",
        "R",
        "Sorcery",
        "A:SP$ DamageAll | NumDmg$ 1 | ValidCards$ Creature | ValidPlayers$ Player | SpellDescription$ Text.",
    );
    assert_eq!(forge_test_blockers(&both), ["DamageAll:cards+players"]);
}

// CLAUDE.md invariant 15: a line that produces nothing must be a named blocker — it must
// never just vanish from the card.
#[test]
fn a_line_that_compiles_to_nothing_blocks_the_card() {
    // No `ValidCards$` on an "all" effect: Forge's default differs per effect.
    let card = spell(
        "Test Void",
        "B",
        "Sorcery",
        "A:SP$ DestroyAll | SpellDescription$ Text.",
    );
    assert_eq!(forge_test_blockers(&card), ["DestroyAll:no-ValidCards"]);
    // A required parameter is missing: caught even though no code names it.
    let card = spell(
        "Test Blank",
        "R",
        "Sorcery",
        "A:SP$ DealDamage | ValidTgts$ Any | SpellDescription$ Text.",
    );
    assert_eq!(forge_test_blockers(&card), ["unexplained:ability"]);
}

// CR 107.3m (2026-11-06): in a permanent's own enters trigger, X is the X it was cast with.
// The engine doesn't keep that value yet, so such a trigger must not compile to "the X of
// this ability" (which nothing defines).
#[test]
fn x_of_the_permanent_spell_in_its_enters_trigger_is_not_compiled_yet() {
    let etb = "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigDmg | TriggerDescription$ Text.\n\
        SVar:TrigDmg:DB$ DealDamage | ValidTgts$ Any | NumDmg$ X\n\
        SVar:X:Count$xPaid";
    let card = creature("Test Hydra", "X R", "Hydra", "1/1", etb);
    assert_eq!(
        forge_test_blockers(&card),
        ["expr:Count$xPaid in a trigger"]
    );
}

// ---------------------------------------------------------------------------------------
// Costs.

// CR 601.2h, 701.21a, 118.3 (2026-11-06). Forge: `Cost$ … Sac<1/Creature>` — sacrifice a
// permanent of your choice as the cost is paid; `Creature.Other` is "another creature".
#[test]
fn sacrificing_a_chosen_permanent_as_a_cost() {
    let cards = [
        "Name:Test Altar\nManaCost:2\nTypes:Artifact\n\
         A:AB$ Draw | Cost$ 1 Sac<1/Creature> | NumCards$ 1 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
        creature(
            "Test Cannibal",
            "1 B",
            "Ogre",
            "3/3",
            "A:AB$ GainLife | Cost$ Sac<1/Creature.Other/another creature> | LifeAmount$ 2 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp", "Test Altar"]);
    let cannibal = sc.card(P0, Zone::Battlefield, "Test Cannibal");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P1, Zone::Battlefield, "Test Golem");
    let mut s = Script::new(sc.build());
    // The Altar: either of my creatures (not the opponent's).
    s.activate("Test Altar");
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Sacrifice));
    assert_eq!(s.options().len(), 2);
    s.pick(
        "the Golem",
        |_, o| matches!(o, OptionDesc::Card { card } if *card == golem),
    );
    s.pay();
    // Paid on activation: the Golem is gone while the ability is still on the stack.
    assert!(!on_battlefield(&s, golem));
    assert_eq!(s.st.stack.len(), 1);
    assert_eq!(hand(&s, P0), 0);
    s.pass_all();
    assert_eq!(hand(&s, P0), 1);
    // "Sacrifice another creature": the Cannibal is now my only creature — it can't pay
    // with itself, so the ability isn't offered; the Altar still is.
    assert!(!s.has(
        |s, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if s.name(*source) == "Test Cannibal")
    ));
    s.activate("Test Altar");
    assert_eq!(s.options().len(), 1);
    s.respond(Response::Pick(vec![0])).pay().pass_all();
    assert!(!on_battlefield(&s, cannibal));
    // Nothing left to sacrifice (CR 118.3): the Altar can't be activated.
    assert!(!s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { .. })));
}

// CR 601.2h, 119.4 (2026-11-06). Forge: `Discard<1/Card>` and `PayLife<2>` in a cost.
#[test]
fn discarding_and_paying_life_as_costs() {
    let cards = [
        creature(
            "Test Rummager",
            "1 R",
            "Goblin",
            "1/1",
            "A:AB$ DealDamage | Cost$ Discard<1/Card> | ValidTgts$ Player | NumDmg$ 1 | SpellDescription$ Text.",
        ),
        creature(
            "Test Bloodletter",
            "1 B",
            "Vampire",
            "1/1",
            "A:AB$ Draw | Cost$ PayLife<2> | NumCards$ 1 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let can = |s: &Script, name: &str| {
        s.has(|s, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if s.name(*source) == name))
    };
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Rummager", "Test Bloodletter"],
    );
    sc.cards(P0, Zone::Hand, &["Forest"]);
    sc.life(P0, 3);
    let mut s = Script::new(sc.build());
    // Discard: chosen from hand, gone at once; with an empty hand it can't be paid.
    s.activate("Test Rummager").target_player(P1);
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Discard));
    s.respond(Response::Pick(vec![0]));
    assert!(s.find(P0, Zone::Graveyard, "Forest").is_some());
    s.pass_all();
    assert_eq!(life(&s, P1), 39);
    assert!(!can(&s, "Test Rummager"));
    // Life: 3 -> 1 on activation; then 1 < 2, so it can't be paid again.
    s.activate("Test Bloodletter");
    assert_eq!(life(&s, P0), 1);
    s.pass_all();
    assert_eq!(hand(&s, P0), 1);
    assert!(!can(&s, "Test Bloodletter"));
}

// CR 118.8, 601.2b/h (2026-11-06). Forge: `A:SP$ … | Cost$ <mana cost> Sac<1/Creature>` —
// "as an additional cost to cast this spell, sacrifice a creature".
#[test]
fn additional_costs_of_spells() {
    let cards = [
        spell(
            "Test Offering",
            "1 B",
            "Sorcery",
            "A:SP$ Draw | Cost$ 1 B Sac<1/Creature> | NumCards$ 2 | SpellDescription$ Text.",
        ),
        spell(
            "Test Insight",
            "U",
            "Instant",
            "A:SP$ Draw | Cost$ U Discard<1/Card> | NumCards$ 2 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp", "Island"]);
    sc.cards(P0, Zone::Hand, &["Test Insight", "Test Offering"]);
    let elf = sc.card(P1, Zone::Battlefield, "Test Elf");
    let mut s = Script::new(sc.build());
    // No creature of mine to sacrifice: the Offering can't be cast.
    assert!(!s.can_cast("Test Offering"));
    assert!(on_battlefield(&s, elf));
    // The Insight discards another card: the Offering is the only candidate (the Insight
    // itself is on the stack by then).
    assert!(s.can_cast("Test Insight"));
    s.cast("Test Insight");
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Discard));
    assert_eq!(s.options().len(), 1);
    s.respond(Response::Pick(vec![0])).pay().pass_all();
    assert!(s.find(P0, Zone::Graveyard, "Test Offering").is_some());
    assert_eq!(hand(&s, P0), 2, "drew two");
    // With only itself in hand, a spell that needs a discard can't be cast.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp", "Island"]);
    sc.card(P0, Zone::Hand, "Test Insight");
    let s = Script::new(sc.build());
    assert!(!s.can_cast("Test Insight"));
    // The Offering with a creature: sacrificed as the spell is cast.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Offering");
    let mut s = Script::new(sc.build());
    s.cast("Test Offering");
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Sacrifice));
    s.respond(Response::Pick(vec![0])).pay();
    assert!(!on_battlefield(&s, golem));
    assert!(s.find(P0, Zone::Stack, "Test Offering").is_some());
    s.pass_all();
    assert_eq!(hand(&s, P0), 2);
}

// ---------------------------------------------------------------------------------------
// Auras.

fn aura(name: &str, cost: &str, enchant: &str, rest: &str) -> String {
    format!(
        "Name:{name}\nManaCost:{cost}\nTypes:Enchantment Aura\nK:Enchant:{enchant}\n{rest}\nOracle:Text."
    )
}

// CR 702.5a, 303.4 (2026-11-06). Forge: `K:Enchant:Creature` and a static with
// `AffectedDefined$ Enchanted` — "Enchant creature. Enchanted creature gets +1/+2 and has
// flying." `GainControl$ You`: "You control enchanted creature."
#[test]
fn auras_enchant_a_creature_and_affect_it() {
    let cards = [
        aura(
            "Test Blessing",
            "W",
            "Creature",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | AddPower$ 1 | AddToughness$ 2 | AddKeyword$ Flying | Description$ Text.",
        ),
        aura(
            "Test Thrall",
            "2 U U",
            "Creature",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | GainControl$ You | Description$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Plains", "Island", "Island", "Island", "Island"],
    );
    let mine = sc.card(P0, Zone::Battlefield, "Test Golem"); // 2/3
    let theirs = sc.card(P1, Zone::Battlefield, "Test Giant"); // 5/4
    sc.cards(P0, Zone::Hand, &["Test Blessing", "Test Thrall"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Blessing").target_object(mine).pay().pass_all();
    let blessing = s.find(P0, Zone::Battlefield, "Test Blessing").unwrap();
    assert_eq!(s.st.object(blessing).attached_to, Some(mine));
    assert_eq!(pt(&s, mine), (3, 5));
    assert!(chars(&s.st, mine).has(Keyword::Flying));
    assert_eq!(pt(&s, theirs), (5, 4), "only the enchanted creature");
    s.cast("Test Thrall").target_object(theirs).pay().pass_all();
    assert_eq!(chars(&s.st, theirs).controller, P0);
}

// CR 303.4c, 702.18a (2026-11-06): shroud stops targeting, not being enchanted — an Aura
// already attached stays. (The same test decides both in the engine; they must differ.)
#[test]
fn an_aura_stays_on_a_creature_with_shroud() {
    let cards = [
        aura(
            "Test Blessing",
            "W",
            "Creature",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | AddPower$ 1 | AddToughness$ 1 | Description$ Text.",
        ),
        creature("Test Hermit", "1 G", "Beast", "2/2", "K:Shroud"),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    let hermit = sc.card(P0, Zone::Battlefield, "Test Hermit");
    let blessing = sc.card(P0, Zone::Battlefield, "Test Blessing");
    sc.attach(blessing, hermit);
    let mut s = Script::new(sc.build());
    // State-based actions have been checked: both are still there.
    s.pass_all();
    assert!(on_battlefield(&s, blessing));
    assert_eq!(pt(&s, hermit), (3, 3));
}

// Forge: `AffectedDefined$ Self` is "this permanent". What isn't compiled: Auras that
// enchant a player, and an `Affected$` that narrows the enchanted
// permanent ("as long as enchanted creature is white").
#[test]
fn statics_about_the_card_itself_and_aura_forms_not_compiled() {
    let brawler = creature(
        "Test Brawler",
        "1 G",
        "Ogre",
        "2/2",
        "S:Mode$ Continuous | AffectedDefined$ Self | AddPower$ 1 | AddToughness$ 1 | Description$ Text.",
    );
    let mut sc = scenario(&[&brawler]);
    let b = sc.card(P0, Zone::Battlefield, "Test Brawler");
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, b), (3, 3));
    let player_aura = aura(
        "Test Hexing",
        "B",
        "Player",
        "S:Mode$ Continuous | Affected$ Creature.YouCtrl | AddPower$ 1 | Description$ Text.",
    );
    assert_eq!(forge_test_blockers(&player_aura), ["enchant:Player"]);
    let narrowed = aura(
        "Test Favor",
        "W",
        "Creature",
        "S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature.White | AddPower$ 1 | Description$ Text.",
    );
    assert_eq!(
        forge_test_blockers(&narrowed),
        ["static:Enchanted-as-long-as"]
    );
}

// ---------------------------------------------------------------------------------------
// Attacking and blocking creatures.

/// Declare blocks: each `(blocker, attacker)` pair blocks; nobody else does.
fn block_with(s: &mut Script, blocks: &[(ObjectId, ObjectId)]) {
    drive_until(s, |s| {
        s.kind() == ChoiceKind::DeclareBlockers || s.st.turn.step == Step::PostcombatMain
    });
    while s.next.is_some() && s.kind() == ChoiceKind::DeclareBlockers {
        let pick = s.options().iter().position(|o| {
            matches!(o, OptionDesc::Block { blocker, attacker } if blocks.contains(&(*blocker, *attacker)))
        });
        s.respond(Response::Pick(pick.into_iter().collect()));
    }
}

// CR 506.4 (2026-11-06): a creature is "attacking" from its declaration until it leaves
// combat or combat ends; "blocking" likewise. Forge: the filter properties `.attacking` and
// `.blocking` — in an effect, a target, and a static ability.
#[test]
fn attacking_and_blocking_creatures_in_filters() {
    let cards = [
        spell(
            "Test Warcry",
            "R",
            "Instant",
            "A:SP$ PumpAll | ValidCards$ Creature.attacking | NumAtt$ +2 | SpellDescription$ Text.",
        ),
        spell(
            "Test Ambush",
            "W",
            "Instant",
            "A:SP$ Destroy | ValidTgts$ Creature.attacking | SpellDescription$ Text.",
        ),
        spell(
            "Test Bulwark Stance",
            "W",
            "Instant",
            "A:SP$ PumpAll | ValidCards$ Creature.blocking | NumDef$ +3 | SpellDescription$ Text.",
        ),
        "Name:Test Banner\nManaCost:2\nTypes:Artifact\n\
         S:Mode$ Continuous | Affected$ Creature.attacking+YouCtrl | AddPower$ 1 | Description$ Text.\nOracle:Text."
            .to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Test Banner"]);
    sc.cards(P1, Zone::Battlefield, &["Plains", "Plains"]);
    let attacker = sc.card(P0, Zone::Battlefield, "Test Golem"); // 2/3
    let home = sc.card(P0, Zone::Battlefield, "Test Giant"); // 5/4
    let blocker = sc.card(P1, Zone::Battlefield, "Test Golem");
    let idle = sc.card(P1, Zone::Battlefield, "Test Elf"); // 1/1
    sc.cards(P0, Zone::Hand, &["Test Warcry"]);
    sc.cards(P1, Zone::Hand, &["Test Ambush", "Test Bulwark Stance"]);
    let mut s = Script::new(sc.build());
    // Nobody is attacking yet: no bonus, and nothing for the Ambush to target.
    assert_eq!(pt(&s, attacker), (2, 3));
    attack_with(&mut s, &[attacker]);
    // The Banner: +1/+0 while it attacks. The creature that stayed home gets nothing.
    assert_eq!(s.st.turn.step, Step::DeclareAttackers);
    assert_eq!(pt(&s, attacker), (3, 3));
    assert_eq!(pt(&s, home), (5, 4));
    // "Attacking creatures get +2/+0 until end of turn."
    s.cast("Test Warcry").pay().pass_all();
    assert_eq!(pt(&s, attacker), (5, 3));
    assert_eq!(pt(&s, home), (5, 4));
    // The Golem blocks; "blocking creatures get +0/+3" reaches only it.
    block_with(&mut s, &[(blocker, attacker)]);
    assert_eq!(s.st.turn.step, Step::DeclareBlockers);
    assert_eq!(s.who(), P0);
    s.pass();
    assert_eq!(s.who(), P1);
    s.cast("Test Bulwark Stance").pay().pass_all();
    assert_eq!(pt(&s, blocker), (2, 6));
    assert_eq!(pt(&s, idle), (1, 1));
    // "Destroy target attacking creature": only the attacker is a legal target.
    s.pass();
    assert_eq!(s.who(), P1);
    s.cast("Test Ambush");
    assert!(can_target(&s, attacker));
    assert!(!can_target(&s, home) && !can_target(&s, blocker));
    s.target_object(attacker).pay().pass_all();
    assert!(!on_battlefield(&s, attacker));
    // After combat nothing is attacking: the bonus is gone with the role.
    drive_until(&mut s, |s| s.st.turn.step == Step::PostcombatMain);
    assert_eq!(pt(&s, home), (5, 4));
}

// CR 506.4 (2026-11-06): a permanent whose controller changes is removed from combat.
#[test]
fn a_creature_that_changes_controller_leaves_combat() {
    let seize = "Name:Test Seizure\nManaCost:2 U U\nTypes:Enchantment Aura\nK:Flash\nK:Enchant:Creature\n\
        S:Mode$ Continuous | AffectedDefined$ Enchanted | GainControl$ You | Description$ Text.\nOracle:Text.";
    let mut sc = scenario(&[seize]);
    sc.cards(P1, Zone::Battlefield, &["Island"; 4]);
    let attacker = sc.card(P0, Zone::Battlefield, "Test Giant"); // 5/4
    sc.card(P1, Zone::Hand, "Test Seizure");
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[attacker]);
    assert_eq!(
        engine_core::layers::combat_role(&s.st, attacker),
        (true, false)
    );
    s.pass();
    assert_eq!(s.who(), P1);
    s.cast("Test Seizure")
        .target_object(attacker)
        .pay()
        .pass_all();
    assert_eq!(chars(&s.st, attacker).controller, P1);
    assert_eq!(
        engine_core::layers::combat_role(&s.st, attacker),
        (false, false)
    );
    // No combat damage is dealt: it is no longer attacking.
    drive_until(&mut s, |s| s.st.turn.step == Step::PostcombatMain);
    assert_eq!((life(&s, P0), life(&s, P1)), (40, 40));
}

// ---------------------------------------------------------------------------------------
// Static abilities with a condition, and "+N for each".

fn has(s: &Script, id: ObjectId, k: Keyword) -> bool {
    chars(&s.st, id).has(k)
}

// CR 604.2, 611.3a (2026-11-06): a static ability's effect applies while its condition is
// true, and is not locked in. Forge: `IsPresent$ <filter>` ("as long as you control …"),
// with `PresentCompare$ EQ0` "as long as you control no …".
#[test]
fn a_static_ability_as_long_as_something_is_present_or_absent() {
    let cards = [
        creature(
            "Test Tinkerer",
            "1 U",
            "Vedalken",
            "2/2",
            "S:Mode$ Continuous | AffectedDefined$ Self | AddPower$ 1 | AddToughness$ 1 | IsPresent$ Artifact.YouCtrl | Description$ Text.",
        ),
        creature(
            "Test Luddite",
            "1 R",
            "Goblin",
            "2/2",
            "S:Mode$ Continuous | AffectedDefined$ Self | AddKeyword$ Haste | IsPresent$ Artifact.YouCtrl | PresentCompare$ EQ0 | Description$ Text.",
        ),
        "Name:Test Automaton\nManaCost:2\nTypes:Artifact Creature Construct\nPT:1/1\nOracle:"
            .to_string(),
        SMITE.to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island", "Swamp"]);
    let tinkerer = sc.card(P0, Zone::Battlefield, "Test Tinkerer");
    let luddite = sc.card(P0, Zone::Battlefield, "Test Luddite");
    // An opponent's artifact doesn't count for "you control".
    sc.card(P1, Zone::Battlefield, "Test Automaton");
    sc.cards(P0, Zone::Hand, &["Test Automaton", "Test Smiting"]);
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, tinkerer), (2, 2));
    assert!(has(&s, luddite, Keyword::Haste));
    s.cast("Test Automaton").pay().pass_all();
    assert_eq!(pt(&s, tinkerer), (3, 3));
    assert!(!has(&s, luddite, Keyword::Haste));
    // The artifact leaves: both go back.
    let mine = s.find(P0, Zone::Battlefield, "Test Automaton").unwrap();
    s.cast("Test Smiting").target_object(mine).pay().pass_all();
    assert_eq!(pt(&s, tinkerer), (2, 2));
    assert!(has(&s, luddite, Keyword::Haste));
}

// Forge's named conditions on statics: `PlayerTurn` ("during your turn"), `NotPlayerTurn`,
// `Threshold` (seven or more cards in your graveyard), `Metalcraft` (three or more
// artifacts), `Hellbent` (no cards in hand).
#[test]
fn named_conditions_on_static_abilities() {
    let with = |name: &str, cond: &str| {
        creature(
            name,
            "1 W",
            "Human",
            "2/2",
            &format!(
                "S:Mode$ Continuous | AffectedDefined$ Self | AddPower$ 2 | Condition$ {cond} | Description$ Text."
            ),
        )
    };
    let cards = [
        with("Test Dayguard", "PlayerTurn"),
        with("Test Nightguard", "NotPlayerTurn"),
        with("Test Elder", "Threshold"),
        with("Test Smith", "Metalcraft"),
        with("Test Gambler", "Hellbent"),
        "Name:Test Trinket\nManaCost:1\nTypes:Artifact\nOracle:".to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let power = |s: &Script, id: ObjectId| chars(&s.st, id).power.unwrap();
    let mut sc = scenario(&cards);
    let day = sc.card(P0, Zone::Battlefield, "Test Dayguard");
    let night = sc.card(P0, Zone::Battlefield, "Test Nightguard");
    let elder = sc.card(P0, Zone::Battlefield, "Test Elder");
    let smith = sc.card(P0, Zone::Battlefield, "Test Smith");
    let gambler = sc.card(P0, Zone::Battlefield, "Test Gambler");
    sc.cards(P0, Zone::Graveyard, &["Forest"; 6]);
    sc.cards(P0, Zone::Battlefield, &["Test Trinket"; 2]);
    let mut s = Script::new(sc.build());
    // P0's turn, six cards in the graveyard, two artifacts, an empty hand.
    assert_eq!(
        [day, night, elder, smith, gambler].map(|c| power(&s, c)),
        [4, 2, 2, 2, 4]
    );
    // On P1's turn (P0 has drawn nothing new yet: still hellbent).
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert_eq!([day, night].map(|c| power(&s, c)), [2, 4]);
    // A seventh card in the graveyard, a third artifact, a card in hand.
    let mut sc = scenario(&cards);
    let elder = sc.card(P0, Zone::Battlefield, "Test Elder");
    let smith = sc.card(P0, Zone::Battlefield, "Test Smith");
    let gambler = sc.card(P0, Zone::Battlefield, "Test Gambler");
    sc.cards(P0, Zone::Graveyard, &["Forest"; 7]);
    sc.cards(P0, Zone::Battlefield, &["Test Trinket"; 3]);
    sc.card(P0, Zone::Hand, "Forest");
    let s = Script::new(sc.build());
    assert_eq!([elder, smith, gambler].map(|c| power(&s, c)), [4, 4, 2]);
}

// CR 611.3a (2026-11-06): a static "+1/+1 for each …" is re-counted all the time (unlike a
// resolved "+X/+X until end of turn", CR 608.2h). Forge: `AddPower$ X` with
// `SVar:X:Count$Valid …`.
#[test]
fn a_static_bonus_for_each_permanent_is_counted_continuously() {
    let lord = "Name:Test Packleader\nManaCost:2 G\nTypes:Creature Wolf\nPT:1/1\n\
        S:Mode$ Continuous | AffectedDefined$ Self | AddPower$ X | AddToughness$ X | Description$ Text.\n\
        SVar:X:Count$Valid Creature.Other+YouCtrl\nOracle:Text.";
    let mut sc = scenario(&[lord, SMITE]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Swamp"]);
    let lord = sc.card(P0, Zone::Battlefield, "Test Packleader");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Elf", "Test Smiting"]);
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, lord), (2, 2), "one other creature of mine");
    s.cast("Test Elf").pay().pass_all();
    assert_eq!(pt(&s, lord), (3, 3));
    s.cast("Test Smiting").target_object(golem).pay().pass_all();
    assert_eq!(pt(&s, lord), (2, 2));
}

// CR 613.8 (2026-11-06): an effect depends on another in the same layer if applying the
// other first would change whether it exists; then the other is applied first, whatever
// their timestamps. Here the static ability is older than the resolved "gains reach", and
// by timestamps alone it would apply before its condition turned false.
#[test]
fn a_condition_can_depend_on_another_effect_in_the_same_layer() {
    let cards = [
        creature(
            "Test Skymaster",
            "2 U",
            "Wizard",
            "1/1",
            "S:Mode$ Continuous | Affected$ Creature.YouCtrl | AddKeyword$ Flying | IsPresent$ Creature.YouCtrl+withReach | PresentCompare$ EQ0 | Description$ Text.",
        ),
        spell(
            "Test Stretch",
            "G",
            "Instant",
            "A:SP$ Pump | ValidTgts$ Creature | KW$ Reach | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    let master = sc.card(P0, Zone::Battlefield, "Test Skymaster");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Stretch");
    let mut s = Script::new(sc.build());
    // "…as long as you control no creature with reach."
    assert!(has(&s, master, Keyword::Flying) && has(&s, golem, Keyword::Flying));
    s.cast("Test Stretch").target_object(golem).pay().pass_all();
    assert!(has(&s, golem, Keyword::Reach));
    assert!(!has(&s, golem, Keyword::Flying) && !has(&s, master, Keyword::Flying));
}

// CR 110.4a (2026-11-06): off the battlefield, "permanent" means a permanent *card* — not an
// instant or sorcery. Forge writes it with the same word (`Permanent`) as "any permanent".
#[test]
fn permanent_cards_in_a_graveyard_are_not_instants_or_sorceries() {
    let cards = [
        creature(
            "Test Delver",
            "1 B",
            "Zombie",
            "2/2",
            "S:Mode$ Continuous | AffectedDefined$ Self | AddPower$ 1 | IsPresent$ Permanent.YouOwn | PresentZone$ Graveyard | PresentCompare$ GE2 | Description$ Text.",
        ),
        spell(
            "Test Unearthing",
            "B",
            "Sorcery",
            "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Hand | ValidTgts$ Permanent.YouOwn | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.card(P0, Zone::Battlefield, "Swamp");
    let delver = sc.card(P0, Zone::Battlefield, "Test Delver");
    // A land card, and two sorceries: one permanent card.
    let forest = sc.card(P0, Zone::Graveyard, "Forest");
    let sorceries = sc.cards(P0, Zone::Graveyard, &["Test Unearthing"; 2]);
    sc.card(P0, Zone::Hand, "Test Unearthing");
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, delver), (2, 2), "one permanent card isn't two");
    s.cast("Test Unearthing");
    assert!(can_target(&s, forest));
    assert!(!can_target(&s, sorceries[0]));
}

// ---------------------------------------------------------------------------------------
// Combat restrictions from static abilities.

// CR 508.1c, 509.1b (2026-11-06): restrictions on attacking and blocking. Forge: the static
// modes `CantBlock`, `CantBlockBy` (with or without `ValidBlocker$`), `CantAttack`, and
// several modes on one line ("can't attack or block").
#[test]
fn static_abilities_that_restrict_attacking_and_blocking() {
    let cards = [
        creature(
            "Test Craven",
            "R",
            "Goblin",
            "2/2",
            "S:Mode$ CantBlock | ValidCard$ Card.Self | Description$ Text.",
        ),
        creature(
            "Test Wraith",
            "1 U",
            "Spirit",
            "1/1",
            "S:Mode$ CantBlockBy | ValidAttacker$ Creature.Self | Description$ Text.",
        ),
        creature(
            "Test Skulker",
            "1 B",
            "Rogue",
            "3/1",
            "S:Mode$ CantBlockBy | ValidAttacker$ Creature.Self | ValidBlocker$ Creature.powerLE2 | Description$ Text.",
        ),
        creature(
            "Test Lookout",
            "1 G",
            "Archer",
            "1/4",
            "K:Reach\nS:Mode$ CantBlockBy | ValidAttacker$ Creature.withoutFlying | ValidBlocker$ Creature.Self | Description$ Text.",
        ),
        creature("Test Kite", "1 U", "Bird", "1/1", "K:Flying"),
        aura(
            "Test Shackles",
            "1 W",
            "Creature",
            "S:Mode$ CantAttack,CantBlock | ValidCard$ Creature.EnchantedBy | Description$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let defenders = ["Test Craven", "Test Golem", "Test Giant", "Test Lookout"];
    let blockers_of = |attacker: &str| {
        let mut sc = scenario(&cards);
        let a = sc.card(P0, Zone::Battlefield, attacker);
        sc.cards(P1, Zone::Battlefield, &defenders);
        let mut s = Script::new(sc.build());
        attack_with(&mut s, &[a]);
        offered_blockers(&mut s, a)
    };
    // A plain attacker: the Craven can't block; the Lookout can't block a creature without
    // flying.
    assert_eq!(blockers_of("Test Brute"), ["Test Golem", "Test Giant"]);
    // A flyer: only the Lookout (reach) could, and it may.
    assert_eq!(blockers_of("Test Kite"), ["Test Lookout"]);
    // "Can't be blocked."
    assert_eq!(blockers_of("Test Wraith"), [] as [&str; 0]);
    // "Can't be blocked by creatures with power 2 or less": the Golem is 2/3.
    assert_eq!(blockers_of("Test Skulker"), ["Test Giant"]);

    // "Enchanted creature can't attack or block."
    let mut sc = scenario(&cards);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let giant = sc.card(P0, Zone::Battlefield, "Test Giant");
    let shackles = sc.card(P1, Zone::Battlefield, "Test Shackles");
    sc.attach(shackles, giant);
    let mut s = Script::new(sc.build());
    let asked = attack_with(&mut s, &[]);
    assert_eq!(asked, [golem], "the shackled Giant isn't asked");
    let mut sc = scenario(&cards);
    let attacker = sc.card(P0, Zone::Battlefield, "Test Brute");
    let giant = sc.card(P1, Zone::Battlefield, "Test Giant");
    sc.card(P1, Zone::Battlefield, "Test Golem");
    let shackles = sc.card(P0, Zone::Battlefield, "Test Shackles");
    sc.attach(shackles, giant);
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[attacker]);
    assert_eq!(offered_blockers(&mut s, attacker), ["Test Golem"]);
}

// Forge filter properties `.withoutFlying` ("without flying") and `.powerLE<n>` ("with
// power n or less"), here in effects.
#[test]
fn filters_without_a_keyword_and_with_power_at_most() {
    let cards = [
        spell(
            "Test Tremor",
            "R",
            "Sorcery",
            "A:SP$ DamageAll | ValidCards$ Creature.withoutFlying | NumDmg$ 1 | SpellDescription$ Text.",
        ),
        spell(
            "Test Culling",
            "B",
            "Sorcery",
            "A:SP$ DestroyAll | ValidCards$ Creature.powerLE2 | SpellDescription$ Text.",
        ),
        creature("Test Kite", "1 U", "Bird", "1/1", "K:Flying"),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Swamp"]);
    let kite = sc.card(P1, Zone::Battlefield, "Test Kite");
    let elf = sc.card(P1, Zone::Battlefield, "Test Elf"); // 1/1
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem"); // 2/3
    let giant = sc.card(P1, Zone::Battlefield, "Test Giant"); // 5/4
    sc.cards(P0, Zone::Hand, &["Test Tremor", "Test Culling"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Tremor").pay().pass_all();
    assert!(on_battlefield(&s, kite), "it flies");
    assert!(!on_battlefield(&s, elf));
    assert_eq!(s.st.object(golem).damage_marked, 1);
    s.cast("Test Culling").pay().pass_all();
    assert!(!on_battlefield(&s, kite) && !on_battlefield(&s, golem));
    assert!(on_battlefield(&s, giant));
}

// ---------------------------------------------------------------------------------------
// Equipment, and Auras that enchant other things than creatures.

// CR 702.6a, 301.5, 704.5n, 701.3 (2026-11-06). Forge: `K:Equip:<cost>` and a static with
// `AffectedDefined$ Equipped`.
#[test]
fn equipment_attaches_moves_and_falls_off() {
    let blade = "Name:Test Blade\nManaCost:1\nTypes:Artifact Equipment\nK:Equip:1\n\
        S:Mode$ Continuous | AffectedDefined$ Equipped | Affected$ Creature | AddPower$ 2 | AddToughness$ 2 | AddKeyword$ First Strike | Description$ Text.\nOracle:Text.";
    let mut sc = scenario(&[blade, SMITE, ZAP]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Forest", "Swamp", "Mountain"],
    );
    let blade = sc.card(P0, Zone::Battlefield, "Test Blade");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem"); // 2/3
    let elf = sc.card(P0, Zone::Battlefield, "Test Elf"); // 1/1
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Smiting", "Test Zapping"]);
    let mut s = Script::new(sc.build());
    // It is on the battlefield unattached (CR 301.5b); equip targets only my creatures.
    assert_eq!(s.st.object(blade).attached_to, None);
    s.activate("Test Blade");
    assert!(can_target(&s, golem) && can_target(&s, elf));
    assert!(!can_target(&s, theirs), "equip: a creature you control");
    s.target_object(elf).pay().pass_all();
    assert_eq!(s.st.object(blade).attached_to, Some(elf));
    assert_eq!(pt(&s, elf), (3, 3));
    assert!(has(&s, elf, Keyword::FirstStrike));
    // Equip again: it moves, the bonus with it.
    s.activate("Test Blade")
        .target_object(golem)
        .pay()
        .pass_all();
    assert_eq!(s.st.object(blade).attached_to, Some(golem));
    assert_eq!((pt(&s, elf), pt(&s, golem)), ((1, 1), (4, 5)));
    // 3 damage on a 4/5; then the creature is destroyed: the Equipment stays on the
    // battlefield, unattached (CR 704.5n).
    s.cast("Test Zapping").target_object(golem).pay().pass_all();
    assert!(on_battlefield(&s, golem));
    s.cast("Test Smiting").target_object(golem).pay().pass_all();
    assert!(!on_battlefield(&s, golem));
    assert!(on_battlefield(&s, blade));
    assert_eq!(s.st.object(blade).attached_to, None);
}

// CR 704.5g, 611.3a (2026-11-06): the Equipment's bonus lasts only while it is attached —
// moving it away from a damaged creature it was keeping alive kills that creature.
#[test]
fn a_creature_kept_alive_by_equipment_dies_when_it_moves_away() {
    let plate = "Name:Test Bulwark Plate\nManaCost:1\nTypes:Artifact Equipment\nK:Equip:1\n\
        S:Mode$ Continuous | AffectedDefined$ Equipped | AddToughness$ 3 | Description$ Text.\nOracle:Text.";
    let mut sc = scenario(&[plate, ZAP]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Forest"]);
    let plate = sc.card(P0, Zone::Battlefield, "Test Bulwark Plate");
    let elf = sc.card(P0, Zone::Battlefield, "Test Elf"); // 1/1, 1/4 with the plate
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.attach(plate, elf);
    sc.card(P0, Zone::Hand, "Test Zapping");
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, elf), (1, 4));
    s.cast("Test Zapping").target_object(elf).pay().pass_all();
    assert!(on_battlefield(&s, elf), "3 damage on toughness 4");
    s.activate("Test Bulwark Plate")
        .target_object(golem)
        .pay()
        .pass_all();
    assert_eq!(s.st.object(plate).attached_to, Some(golem));
    assert!(!on_battlefield(&s, elf), "3 damage on toughness 1");
}

// Forge: `K:Enchant:<filter>` for other things than creatures: "enchant land", "enchant
// creature you control". The Aura falls off when what it enchants stops matching
// (CR 704.5m).
#[test]
fn auras_that_enchant_lands_or_your_own_creatures() {
    let cards = [
        aura(
            "Test Overgrowing",
            "G",
            "Land",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | AddKeyword$ Indestructible | Description$ Text.",
        ),
        aura(
            "Test Devotion",
            "W",
            "Creature.YouCtrl",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | AddPower$ 1 | AddToughness$ 1 | Description$ Text.",
        ),
        aura(
            "Test Thrall",
            "2 U U",
            "Creature",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | GainControl$ You | Description$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    let forest = sc.cards(P0, Zone::Battlefield, &["Forest", "Plains"])[0];
    sc.cards(P1, Zone::Battlefield, &["Island"; 4]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Overgrowing", "Test Devotion"]);
    sc.card(P1, Zone::Hand, "Test Thrall");
    let mut s = Script::new(sc.build());
    // "Enchant land": lands, not creatures.
    s.cast("Test Overgrowing");
    assert!(can_target(&s, forest) && !can_target(&s, golem));
    s.target_object(forest).pay().pass_all();
    assert!(has(&s, forest, Keyword::Indestructible));
    // "Enchant creature you control": not the opponent's.
    s.cast("Test Devotion");
    assert!(can_target(&s, golem) && !can_target(&s, theirs));
    s.target_object(golem).pay().pass_all();
    assert_eq!(pt(&s, golem), (3, 4));
    let devotion = s.find(P0, Zone::Battlefield, "Test Devotion").unwrap();
    // The opponent takes control of the creature on their turn: it is no longer "a
    // creature you control" for the Aura's controller, and the Aura goes to the graveyard.
    drive_until(&mut s, |s| {
        s.st.turn.active == P1 && s.st.turn.step == Step::PrecombatMain && s.who() == P1
    });
    s.cast("Test Thrall").target_object(golem).pay().pass_all();
    assert_eq!(chars(&s.st, golem).controller, P1);
    assert!(!on_battlefield(&s, devotion));
    assert!(s.find(P0, Zone::Graveyard, "Test Devotion").is_some());
}

// ---------------------------------------------------------------------------------------
// Mana abilities.

fn mana_pool(s: &Script, p: PlayerId) -> u32 {
    s.st.player(p).mana_pool.amounts.iter().sum()
}

// CR 605.1a, 106.4 (2026-11-06). Forge: `Produced$ Combo W U` — "{T}: Add {W} or {U}";
// `Produced$ C C` — two mana from one source, the excess stays in the pool;
// `Produced$ Any | Amount$ 3` — three mana of any ONE colour.
#[test]
fn mana_abilities_with_a_choice_of_type_or_several_mana() {
    let land = |name: &str, mana: &str| {
        format!(
            "Name:{name}\nManaCost:no cost\nTypes:Land\nA:AB$ Mana | Cost$ T | {mana} | SpellDescription$ Text.\nOracle:Text."
        )
    };
    let rock = |name: &str, mana: &str| {
        format!(
            "Name:{name}\nManaCost:1\nTypes:Artifact\nA:AB$ Mana | Cost$ T | {mana} | SpellDescription$ Text.\nOracle:Text."
        )
    };
    let cards = [
        land("Test Estuary", "Produced$ Combo W U"),
        rock("Test Lodestone", "Produced$ C C"),
        rock("Test Bloom", "Produced$ Any | Amount$ 3"),
        spell(
            "Test Azure",
            "U",
            "Instant",
            "A:SP$ Draw | NumCards$ 1 | SpellDescription$ Text.",
        ),
        spell(
            "Test Ivory",
            "W",
            "Instant",
            "A:SP$ GainLife | LifeAmount$ 1 | SpellDescription$ Text.",
        ),
        spell(
            "Test Bicolor",
            "W U",
            "Instant",
            "A:SP$ GainLife | LifeAmount$ 2 | SpellDescription$ Text.",
        ),
        spell(
            "Test Triple",
            "G G G",
            "Instant",
            "A:SP$ GainLife | LifeAmount$ 3 | SpellDescription$ Text.",
        ),
        spell(
            "Test Trinket Spell",
            "1",
            "Instant",
            "A:SP$ GainLife | LifeAmount$ 1 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let hand = [
        "Test Azure",
        "Test Ivory",
        "Test Bicolor",
        "Test Triple",
        "Test Trinket Spell",
        "Test Elf",
        "Test Golem",
    ];
    let with = |permanents: &[&str]| {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, permanents);
        sc.cards(P0, Zone::Hand, &hand);
        Script::new(sc.build())
    };
    let castable =
        |s: &Script| -> Vec<&str> { hand.iter().copied().filter(|c| s.can_cast(c)).collect() };
    // "{T}: Add {W} or {U}": either colour (or generic), nothing else.
    let s = with(&["Test Estuary"]);
    assert_eq!(
        castable(&s),
        ["Test Azure", "Test Ivory", "Test Trinket Spell"]
    );
    // Two of them pay {W}{U}.
    let s = with(&["Test Estuary", "Test Estuary"]);
    assert!(s.can_cast("Test Bicolor"));
    // "{T}: Add {C}{C}": two generic from one tap — with a Forest, {3}.
    let s = with(&["Test Lodestone"]);
    assert_eq!(castable(&s), ["Test Trinket Spell"]);
    let mut s = with(&["Test Lodestone", "Forest"]);
    assert!(s.can_cast("Test Golem") && s.can_cast("Test Elf"));
    // Paying {1} with it leaves one colorless mana floating.
    s.tap("Test Lodestone", engine_core::mana::ManaType::Colorless);
    assert_eq!(mana_pool(&s, P0), 2);
    s.cast("Test Trinket Spell").pay();
    assert_eq!(mana_pool(&s, P0), 1);
    // "Add three mana of any one color": {G}{G}{G} yes, {W}{U} no — one colour only.
    let s = with(&["Test Bloom"]);
    assert!(s.can_cast("Test Triple") && s.can_cast("Test Golem"));
    assert!(!s.can_cast("Test Bicolor"));
    let mut s = with(&["Test Bloom"]);
    s.cast("Test Triple").pay().pass_all();
    assert_eq!(life(&s, P0), 43);
    assert_eq!(mana_pool(&s, P0), 0);
}

// What isn't compiled: different mana from one activation ("{W}{U}"), and several mana "in
// any combination". And a multi-mana ability next to another mana ability of the same
// permanent isn't a valid card for the payment planner.
#[test]
fn mana_forms_not_compiled() {
    let rock = |mana: &str| {
        format!(
            "Name:Test Prism Rock\nManaCost:2\nTypes:Artifact\nA:AB$ Mana | Cost$ T | {mana} | SpellDescription$ Text.\nOracle:Text."
        )
    };
    assert_eq!(
        forge_test_blockers(&rock("Produced$ W U")),
        ["Mana:produced"]
    );
    assert_eq!(
        forge_test_blockers(&rock("Produced$ Combo W U | Amount$ 2")),
        ["Mana:produced"]
    );
    let two = "Name:Test Twin Rock\nManaCost:2\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ T | Produced$ C C | SpellDescription$ Text.\n\
        A:AB$ Mana | Cost$ T | Produced$ G | SpellDescription$ Text.\nOracle:Text.";
    let b = forge_test_blockers(two);
    assert!(
        b.iter()
            .any(|x| x.starts_with("invalid:") && x.contains("only mana ability")),
        "{b:?}"
    );
}

// ---------------------------------------------------------------------------------------
// Libraries: searching, scry, surveil, putting cards back.

use engine_core::Event;

fn library(s: &Script, p: PlayerId) -> Vec<String> {
    // Top card first.
    s.st.zones
        .list(Zone::Library, p)
        .iter()
        .rev()
        .map(|&id| s.name(id))
        .collect()
}

fn option_names(s: &Script) -> Vec<String> {
    s.options().iter().map(|o| s.describe(o)).collect()
}

// CR 701.23a–b, 701.24 (2026-11-06). Forge: an untargeted `ChangeZone | Origin$ Library` —
// "search your library for a basic land card, put it onto the battlefield tapped, then
// shuffle". Only cards of the stated quality are offered; finding none is allowed.
#[test]
fn searching_a_library_for_a_card_with_a_quality() {
    let growth = spell(
        "Test Sprawl",
        "1 G",
        "Sorcery",
        "A:SP$ ChangeZone | Origin$ Library | Destination$ Battlefield | ChangeType$ Land.Basic | ChangeTypeDesc$ basic land | Tapped$ True | ChangeNum$ 1 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&growth]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Forest", "Forest", "Forest"],
    );
    // On top of the twenty Forests: a creature card and an Island.
    sc.card(P0, Zone::Library, "Island");
    sc.card(P0, Zone::Library, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Sprawl", "Test Sprawl"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Sprawl").pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::SearchLibrary));
    assert_eq!(s.decision().prim, Primitive::PickN { min: 0, max: 1 });
    let names = option_names(&s);
    assert_eq!(names.len(), 21, "twenty Forests and the Island: {names:?}");
    assert!(!names.contains(&"Test Golem".to_string()));
    // While it is being searched, the searcher sees the library; nobody else does.
    let mine = engine_core::player_view(&s.st, P0);
    let theirs = engine_core::player_view(&s.st, P1);
    assert_eq!(mine.revealed.len(), 22);
    assert!(theirs.revealed.is_empty());
    s.pick(
        "the Island",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Island"),
    );
    let island = s
        .find(P0, Zone::Battlefield, "Island")
        .expect("on the battlefield");
    assert!(s.st.object(island).tapped, "it enters tapped");
    assert_eq!(
        s.count(|e| matches!(e, Event::Shuffled { player } if *player == P0)),
        1
    );
    assert_eq!(s.st.zones.list(Zone::Library, P0).len(), 21);
    // CR 701.23b: a player needn't find a card of a stated quality.
    s.cast("Test Sprawl").pay().pass_all();
    s.respond(Response::Pick(vec![]));
    assert_eq!(s.st.zones.list(Zone::Library, P0).len(), 21);
    assert_eq!(
        s.count(|e| matches!(e, Event::Shuffled { .. })),
        2,
        "still shuffled"
    );
}

// CR 701.23d, 701.24b (2026-11-06): a search for "a card" must find one; "then shuffle and
// put that card on top" shuffles first. Forge: `Destination$ Library | LibraryPosition$ 0 |
// Mandatory$ True`. And a card put into the hand for a stated quality is revealed.
#[test]
fn tutoring_to_the_top_of_the_library_and_to_hand() {
    let cards = [
        spell(
            "Test Foresight",
            "B",
            "Instant",
            "A:SP$ ChangeZone | Origin$ Library | Destination$ Library | LibraryPosition$ 0 | ChangeType$ Card | ChangeNum$ 1 | Mandatory$ True | SpellDescription$ Text.",
        ),
        spell(
            "Test Summons",
            "G",
            "Sorcery",
            "A:SP$ ChangeZone | Origin$ Library | Destination$ Hand | ChangeType$ Creature | ChangeNum$ 1 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Forest"]);
    let golem = sc.card(P0, Zone::Library, "Test Golem");
    sc.library(P0, "Forest", 3); // the Golem is under three more Forests
    sc.cards(P0, Zone::Hand, &["Test Foresight", "Test Summons"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Foresight").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::SearchLibrary);
    assert_eq!(
        s.decision().prim,
        Primitive::PickN { min: 1, max: 1 },
        "must find"
    );
    s.pick(
        "the Golem",
        |_, o| matches!(o, OptionDesc::Card { card } if *card == golem),
    );
    assert_eq!(
        library(&s, P0)[0],
        "Test Golem",
        "on top, after the shuffle"
    );
    assert_eq!(s.st.zones.list(Zone::Library, P0).len(), 24);
    // "Search for a creature card, reveal it, put it into your hand, then shuffle."
    s.cast("Test Summons").pay().pass_all();
    assert_eq!(option_names(&s), ["Test Golem"]);
    s.respond(Response::Pick(vec![0]));
    assert!(s.find(P0, Zone::Hand, "Test Golem").is_some());
    assert_eq!(
        s.count(|e| matches!(e, Event::Revealed { player, .. } if *player == P0)),
        1
    );
    assert_eq!(s.count(|e| matches!(e, Event::Shuffled { .. })), 2);
}

// Forge: `ChangeZone | Origin$ Hand | Destination$ Battlefield | ChangeType$ Land` — "you
// may put a land card from your hand onto the battlefield" (not a search: no shuffle).
#[test]
fn putting_a_card_from_hand_onto_the_battlefield() {
    let explore = spell(
        "Test Homestead",
        "G",
        "Sorcery",
        "A:SP$ ChangeZone | Origin$ Hand | Destination$ Battlefield | ChangeType$ Land | ChangeNum$ 1 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&explore]);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    sc.cards(P0, Zone::Hand, &["Test Homestead", "Island", "Test Golem"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Homestead").pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::ChooseCards));
    assert_eq!(option_names(&s), ["Island"]);
    s.respond(Response::Pick(vec![0]));
    let island = s.find(P0, Zone::Battlefield, "Island").unwrap();
    assert!(!s.st.object(island).tapped);
    assert_eq!(s.count(|e| matches!(e, Event::Shuffled { .. })), 0);
}

// CR 701.22a (scry), 701.25a (surveil) (2026-11-06). Forge: `Scry | ScryNum$ 2`,
// `Surveil | Amount$ 2`.
#[test]
fn scry_and_surveil() {
    let cards = [
        spell(
            "Test Omen",
            "U",
            "Instant",
            "A:SP$ Scry | ScryNum$ 2 | SpellDescription$ Text.",
        ),
        spell(
            "Test Sift",
            "B",
            "Instant",
            "A:SP$ Surveil | Amount$ 2 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island", "Swamp"]);
    sc.card(P0, Zone::Library, "Test Giant");
    sc.card(P0, Zone::Library, "Test Golem");
    sc.card(P0, Zone::Library, "Test Elf"); // top: Elf, Golem, Giant, Forests…
    sc.cards(P0, Zone::Hand, &["Test Omen", "Test Sift"]);
    let mut s = Script::new(sc.build());
    // Scry 2: look at the Elf and the Golem; the Elf goes to the bottom.
    s.cast("Test Omen").pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::ScryBottom));
    assert_eq!(s.decision().prim, Primitive::PickN { min: 0, max: 2 });
    assert_eq!(option_names(&s), ["Test Elf", "Test Golem"]);
    assert!(engine_core::player_view(&s.st, P1).revealed.is_empty());
    s.pick(
        "the Elf",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Test Elf"),
    );
    let lib = library(&s, P0);
    assert_eq!(&lib[..2], ["Test Golem", "Test Giant"]);
    assert_eq!(lib.last().unwrap(), "Test Elf");
    assert_eq!(lib.len(), 23);
    // Surveil 2: the Golem to the graveyard, the Giant stays on top.
    s.cast("Test Sift").pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::SurveilGraveyard));
    assert_eq!(option_names(&s), ["Test Golem", "Test Giant"]);
    s.pick(
        "the Golem",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Test Golem"),
    );
    assert!(s.find(P0, Zone::Graveyard, "Test Golem").is_some());
    assert_eq!(library(&s, P0)[0], "Test Giant");
}

// CR 401.4 (2026-11-06): several cards kept on top by a scry go back in an order the player
// chooses.
#[test]
fn scry_orders_the_cards_kept_on_top() {
    let omen = spell(
        "Test Omen",
        "U",
        "Instant",
        "A:SP$ Scry | ScryNum$ 2 | SpellDescription$ Text.",
    );
    let mut sc = scenario(&[&omen]);
    sc.cards(P0, Zone::Battlefield, &["Island"]);
    sc.card(P0, Zone::Library, "Test Golem");
    sc.card(P0, Zone::Library, "Test Elf");
    sc.card(P0, Zone::Hand, "Test Omen");
    let mut s = Script::new(sc.build());
    s.cast("Test Omen").pay().pass_all();
    s.respond(Response::Pick(vec![])); // keep both
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::ScryOrder));
    assert_eq!(option_names(&s), ["Test Elf", "Test Golem"]);
    // The first listed ends up on top: put the Golem first.
    s.respond(Response::Order(vec![1, 0]));
    assert_eq!(&library(&s, P0)[..2], ["Test Golem", "Test Elf"]);
}

// Forge: `ChangeZone | Origin$ Battlefield | Destination$ Library` with `LibraryPosition$`
// and `Shuffle$ True` — "put target creature on top of its owner's library" / "shuffle it
// into its owner's library" (CR 701.24c); `ChangeZoneAll` from the battlefield — "return
// all creatures to their owners' hands".
#[test]
fn putting_permanents_into_libraries_and_mass_bounce() {
    let cards = [
        spell(
            "Test Rewind",
            "U",
            "Instant",
            "A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Library | LibraryPosition$ 0 | SpellDescription$ Text.",
        ),
        spell(
            "Test Oblivion",
            "U",
            "Instant",
            "A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Library | Shuffle$ True | SpellDescription$ Text.",
        ),
        spell(
            "Test Tide",
            "U",
            "Sorcery",
            "A:SP$ ChangeZoneAll | ChangeType$ Creature | Origin$ Battlefield | Destination$ Hand | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island"; 3]);
    let giant = sc.card(P1, Zone::Battlefield, "Test Giant");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P1, Zone::Battlefield, "Test Elf");
    sc.card(P0, Zone::Battlefield, "Test Elf");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Rewind", "Test Oblivion", "Test Tide"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Rewind").target_object(giant).pay().pass_all();
    assert_eq!(
        library(&s, P1)[0],
        "Test Giant",
        "on top of its owner's library"
    );
    s.cast("Test Oblivion")
        .target_object(golem)
        .pay()
        .pass_all();
    assert_eq!(s.st.zones.list(Zone::Library, P1).len(), 22);
    assert_eq!(
        s.count(|e| matches!(e, Event::Shuffled { player } if *player == P1)),
        1
    );
    s.cast("Test Tide").pay().pass_all();
    assert!(s.find(P1, Zone::Hand, "Test Elf").is_some());
    assert!(s.find(P0, Zone::Hand, "Test Elf").is_some());
    assert_eq!(s.count(|e| matches!(e, Event::Shuffled { .. })), 1);
}

// CR 303.4f–g (2026-11-06): an Aura put onto the battlefield without being cast enters
// attached to something its controller chooses that it could enchant; with nothing to
// enchant it stays where it is.
#[test]
fn an_aura_returned_to_the_battlefield_chooses_what_it_enchants() {
    let cards = [
        aura(
            "Test Blessing",
            "W",
            "Creature",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | AddPower$ 1 | AddToughness$ 1 | Description$ Text.",
        ),
        spell(
            "Test Restoration",
            "W",
            "Sorcery",
            "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Battlefield | ValidTgts$ Enchantment.YouOwn | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    // With creatures around: the player picks one, whoever controls it.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.card(P1, Zone::Battlefield, "Test Elf");
    sc.card(P0, Zone::Graveyard, "Test Blessing");
    sc.card(P0, Zone::Hand, "Test Restoration");
    let mut s = Script::new(sc.build());
    s.cast("Test Restoration")
        .target("Test Blessing")
        .pay()
        .pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::AuraHost));
    assert_eq!(s.options().len(), 2);
    let pick = s
        .options()
        .iter()
        .position(|o| s.describe(o).contains("Test Golem"))
        .expect("the Golem is offered");
    s.respond(Response::Pick(vec![pick]));
    let blessing = s.find(P0, Zone::Battlefield, "Test Blessing").unwrap();
    assert_eq!(s.st.object(blessing).attached_to, Some(golem));
    assert_eq!(pt(&s, golem), (3, 4));
    // With nothing to enchant, it stays in the graveyard.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    sc.card(P0, Zone::Graveyard, "Test Blessing");
    sc.card(P0, Zone::Hand, "Test Restoration");
    let mut s = Script::new(sc.build());
    s.cast("Test Restoration")
        .target("Test Blessing")
        .pay()
        .pass_all();
    assert!(s.find(P0, Zone::Graveyard, "Test Blessing").is_some());
    assert!(s.find(P0, Zone::Battlefield, "Test Blessing").is_none());
}
