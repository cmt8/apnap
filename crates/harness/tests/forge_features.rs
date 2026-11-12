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
    // The Altar: mana abilities first (CR 601.2g), then the choice of what to sacrifice
    // (CR 601.2h) — either of my creatures (not the opponent's).
    s.activate("Test Altar").pay();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Sacrifice));
    assert_eq!(s.options().len(), 2);
    s.pick(
        "the Golem",
        |_, o| matches!(o, OptionDesc::Card { card } if *card == golem),
    );
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
    s.activate("Test Altar").pay();
    assert_eq!(s.options().len(), 1);
    s.respond(Response::Pick(vec![0])).pass_all();
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
    s.cast("Test Insight").pay();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Discard));
    assert_eq!(s.options().len(), 1);
    s.respond(Response::Pick(vec![0])).pass_all();
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
    s.cast("Test Offering").pay();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::Sacrifice));
    s.respond(Response::Pick(vec![0]));
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

// Different mana from one activation ("{W}{U}"), several mana "in any combination", mana
// abilities that cost mana or a sacrifice of something else: compiled since D-066 (their
// meaning is tested in forge_mana.rs). Two sacrifices at once are not.
#[test]
fn mana_forms_not_compiled() {
    let rock = |mana: &str| {
        format!(
            "Name:Test Prism Rock\nManaCost:2\nTypes:Artifact\nA:AB$ Mana | Cost$ T | {mana} | SpellDescription$ Text.\nOracle:Text."
        )
    };
    for ok in [
        rock("Produced$ W U"),
        rock("Produced$ Combo W U | Amount$ 2"),
        rock("Produced$ W").replace("Cost$ T", "Cost$ 1 T"),
        rock("Produced$ W").replace("Cost$ T", "Cost$ T Sac<1/Creature>"),
    ] {
        assert_eq!(forge_test_blockers(&ok), [] as [&str; 0], "{ok}");
    }
    assert_eq!(
        forge_test_blockers(&rock("Produced$ W").replace("Cost$ T", "Cost$ T Sac<2/Creature>")),
        ["Mana:cost sacrifice N"]
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

// ---------------------------------------------------------------------------------------
// Mana: Treasure, commander identity, conditions, restrictions, effects that add mana
// (D-052).

const TREASURE: (&str, &str) = (
    "c_a_test_treasure_sac",
    "Name:Test Treasure Token\nManaCost:no cost\nTypes:Artifact Treasure\n\
     A:AB$ Mana | Cost$ T Sac<1/CARDNAME/this token> | Produced$ Any | Amount$ 1 | SpellDescription$ Text.\nOracle:Text.",
);

fn prospector() -> String {
    creature(
        "Test Prospector",
        "R",
        "Goblin",
        "1/1",
        "T:Mode$ ChangesZone | ValidCard$ Card.Self | Origin$ Any | Destination$ Battlefield | Execute$ TrigTreasure | TriggerDescription$ Text.\n\
         SVar:TrigTreasure:DB$ Token | TokenAmount$ 2 | TokenScript$ c_a_test_treasure_sac | TokenOwner$ You",
    )
}

fn treasures(s: &Script, p: PlayerId) -> Vec<ObjectId> {
    s.st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| s.name(id) == "Test Treasure Token" && chars(&s.st, id).controller == p)
        .collect()
}

// CR 111.10a, 605.3b, 601.2g–h (2026-11-06). Forge: `Cost$ T Sac<1/CARDNAME>` on an
// `AB$ Mana` — "{T}, Sacrifice this token: Add one mana of any color."
#[test]
fn treasure_is_sacrificed_for_mana() {
    let cards = [
        prospector(),
        spell(
            "Test Verdant",
            "G G",
            "Instant",
            "A:SP$ GainLife | LifeAmount$ 3 | SpellDescription$ Text.",
        ),
        "Name:Test Smelter\nManaCost:1\nTypes:Artifact\n\
         A:AB$ Draw | Cost$ 1 Sac<1/Artifact.Other/another artifact> | NumCards$ 1 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = |battlefield: &[&str], hand: &[&str]| {
        let catalog = forge_test_catalog(&cards, &[TREASURE]);
        let mut sc = Scenario::new(GameConfig::commander(2), catalog, 31);
        for p in [P0, P1] {
            sc.library(p, "Forest", 20);
        }
        sc.cards(P0, Zone::Battlefield, battlefield);
        sc.cards(P0, Zone::Hand, hand);
        Script::new(sc.build())
    };
    // The Prospector makes two Treasures; they pay {G}{G} (any colour) and are gone.
    let mut s = build(&["Mountain"], &["Test Prospector", "Test Verdant"]);
    // (The spell resolves, then its enters trigger.)
    s.cast("Test Prospector").pay().pass_all().pass_all();
    assert_eq!(treasures(&s, P0).len(), 2);
    assert!(s.can_cast("Test Verdant"));
    s.cast("Test Verdant");
    // Each tap sacrifices the Treasure at once (CR 605.3b), before the spell is paid for.
    s.respond(Response::Pick(vec![0]));
    assert_eq!(treasures(&s, P0).len(), 1);
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    s.pay();
    assert_eq!(treasures(&s, P0).len(), 0);
    s.pass_all();
    assert_eq!(life(&s, P0), 43);

    // A Treasure can't pay twice: the Smelter's "{1}, sacrifice another artifact" needs one
    // Treasure for the mana and another artifact to sacrifice.
    let mut s = build(&["Mountain", "Test Smelter"], &["Test Prospector"]);
    s.cast("Test Prospector").pay().pass_all().pass_all();
    let can_smelt = |s: &Script| {
        s.has(|s, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if s.name(*source) == "Test Smelter"))
    };
    // Mountain tapped for the Prospector: two Treasures left — one for {1}, one to sacrifice.
    assert!(can_smelt(&s));
    s.activate("Test Smelter");
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    s.respond(Response::Pick(vec![0]));
    // The mana is in: the other Treasure is the only thing left to sacrifice.
    assert_eq!(s.kind(), ChoiceKind::Sacrifice);
    assert_eq!(s.options().len(), 1);
    s.respond(Response::Pick(vec![0])).pass_all();
    assert_eq!(treasures(&s, P0).len(), 0);
    assert_eq!(hand(&s, P0), 1);

    // One Treasure and no other mana: it can't be both the mana and the sacrifice.
    let s = build(&["Test Smelter", TREASURE.0], &[]);
    assert!(!can_smelt(&s));
    // With a Mountain for the mana, the Treasure is the sacrifice — so it isn't offered
    // as mana.
    let mut s = build(&["Test Smelter", TREASURE.0, "Mountain"], &[]);
    assert!(can_smelt(&s));
    s.activate("Test Smelter");
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    assert!(!s.has(
        |s, o| matches!(o, OptionDesc::TapForMana { source, .. } if s.name(*source) == "Test Treasure Token")
    ));
    s.pay();
    assert_eq!(s.kind(), ChoiceKind::Sacrifice);
}

// CR 903.4, 903.4f (2026-11-06). Forge: `Produced$ Combo ColorIdentity` — "Add one mana of
// any color in your commander's color identity."
#[test]
fn mana_of_the_commanders_color_identity() {
    let cards = [
        "Name:Test Signet\nManaCost:2\nTypes:Artifact\n\
         A:AB$ Mana | Cost$ T | Produced$ Combo ColorIdentity | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
        // Green by its cost, white by the mana symbol in its rules text (CR 903.4).
        "Name:Test Warden\nManaCost:1 G\nTypes:Legendary Creature Elf\nPT:2/2\n\
         A:AB$ GainLife | Cost$ W | LifeAmount$ 1 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.commander(P0, Zone::Command, "Test Warden");
    let signet = sc.card(P0, Zone::Battlefield, "Test Signet");
    let other = sc.card(P1, Zone::Battlefield, "Test Signet");
    let mut s = Script::new(sc.build());
    let colors = |s: &Script, id: ObjectId| -> Vec<engine_core::mana::ManaType> {
        s.options()
            .iter()
            .filter_map(|o| match o {
                OptionDesc::TapForMana { source, mana, .. } if *source == id => Some(*mana),
                _ => None,
            })
            .collect()
    };
    use engine_core::mana::ManaType;
    assert_eq!(colors(&s, signet), [ManaType::White, ManaType::Green]);
    // P1 has no commander: their Signet adds nothing (CR 903.4f), so it isn't offered.
    s.pass();
    assert_eq!(s.who(), P1);
    assert_eq!(colors(&s, other), []);
}

// CR 602.5b, 605.1a (2026-11-06). Forge: `IsPresent$ Land.YouCtrl | PresentCompare$ GE5` on
// an `AB$ Mana` — "Activate only if you control five or more lands."
#[test]
fn a_mana_ability_with_an_activation_condition() {
    let cards = [
        "Name:Test Temple\nManaCost:no cost\nTypes:Land\n\
         A:AB$ Mana | Cost$ T | Produced$ C | Amount$ 2 | IsPresent$ Land.YouCtrl | PresentCompare$ GE5 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
        creature("Test Colossus", "5", "Golem", "5/5", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Temple", "Forest", "Forest", "Forest"],
    );
    sc.cards(P0, Zone::Hand, &["Forest", "Test Colossus"]);
    let mut s = Script::new(sc.build());
    let temple = |s: &Script| {
        s.has(|s, o| matches!(o, OptionDesc::TapForMana { source, .. } if s.name(*source) == "Test Temple"))
    };
    // Four lands: the Temple can't be tapped for mana.
    assert!(!temple(&s));
    assert!(!s.can_cast("Test Colossus"));
    // The fifth: four Forests alone don't pay {5}; with the Temple's {C}{C} they do.
    s.pick("play the Forest", |_, o| {
        matches!(o, OptionDesc::PlayLand { .. })
    });
    assert!(temple(&s));
    assert!(s.can_cast("Test Colossus"));
}

// CR 106.6, 106.4 (2026-11-06). Forge: `RestrictValid$ Spell.Angel` — "Spend this mana only
// to cast an Angel spell."
#[test]
fn mana_that_can_only_be_spent_on_some_spells() {
    let cards = [
        creature(
            "Test Font",
            "W",
            "Human",
            "1/1",
            "A:AB$ Mana | Cost$ T | Produced$ W | RestrictValid$ Spell.Angel | SpellDescription$ Text.",
        ),
        creature("Test Seraph", "W", "Angel", "2/2", "K:Flying"),
        creature("Test Squire", "W", "Human", "1/1", ""),
        // An Angel that costs {1}{W}: restricted mana pays generic costs too.
        creature("Test Archon", "1 W", "Angel", "2/2", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = |lands: &[&str]| {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Test Font"]);
        sc.cards(P0, Zone::Battlefield, lands);
        sc.cards(
            P0,
            Zone::Hand,
            &["Test Seraph", "Test Squire", "Test Archon"],
        );
        Script::new(sc.build())
    };
    // Only the Font: the Angel is castable, the Human is not.
    let mut s = build(&[]);
    assert!(s.can_cast("Test Seraph"));
    assert!(!s.can_cast("Test Squire"));
    assert!(!s.can_cast("Test Archon"));
    // Tapped ahead of time, the mana keeps its restriction while it floats…
    s.tap("Test Font", engine_core::mana::ManaType::White);
    assert_eq!(s.st.player(P0).restricted_mana.len(), 1);
    assert_eq!(mana_pool(&s, P0), 0);
    assert!(!s.can_cast("Test Squire"));
    assert!(s.can_cast("Test Seraph"));
    // …and a player sees it as part of their unspent mana.
    let view = engine_core::view::player_view(&s.st, P0);
    assert_eq!(view.players[0].mana_pool.amounts.iter().sum::<u32>(), 1);
    assert_eq!(view.players[0].restricted_mana.iter().sum::<u32>(), 1);
    s.cast("Test Seraph");
    assert!(s.st.player(P0).restricted_mana.is_empty());
    assert!(s.find(P0, Zone::Stack, "Test Seraph").is_some());
    // With a Plains as well: the Font's mana pays the Herald's generic {1}; the Squire is
    // paid by the Plains alone.
    let mut s = build(&["Plains"]);
    assert!(s.can_cast("Test Archon") && s.can_cast("Test Squire"));
    s.cast("Test Squire");
    // The Font isn't offered for the Squire.
    assert!(!s.has(
        |s, o| matches!(o, OptionDesc::TapForMana { source, .. } if s.name(*source) == "Test Font")
    ));
    s.pay().pass_all();
    // The Plains is tapped; the Font still casts the Angel, and nothing else.
    assert!(s.can_cast("Test Seraph") && !s.can_cast("Test Archon"));
    // CR 106.4: unspent restricted mana empties with the rest at the end of the step.
    let mut s = build(&[]);
    s.tap("Test Font", engine_core::mana::ManaType::White);
    s.pass_all();
    assert!(s.st.player(P0).restricted_mana.is_empty());
}

// CR 106.4, 605.5b (2026-11-06). Forge: `SP$ Mana | Produced$ R | Amount$ X` — a spell that
// adds mana (not a mana ability: it uses the stack).
#[test]
fn a_spell_that_adds_mana() {
    let cards = [spell(
        "Test Geyser",
        "R",
        "Sorcery",
        "A:SP$ Mana | Produced$ R | Amount$ X | SpellDescription$ Text.\nSVar:X:Count$Valid Land.tapped+OppCtrl",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain"]);
    sc.card(P0, Zone::Hand, "Test Geyser");
    for _ in 0..3 {
        let land = sc.card(P1, Zone::Battlefield, "Forest");
        sc.tapped(land);
    }
    sc.card(P1, Zone::Battlefield, "Forest");
    let mut s = Script::new(sc.build());
    s.cast("Test Geyser").pay();
    assert_eq!(s.st.stack.len(), 1, "on the stack: it can be responded to");
    s.pass_all();
    assert_eq!(
        s.st.player(P0).mana_pool.amounts[engine_core::mana::ManaType::Red.index()],
        3
    );
}

// CR 608.2h, 601.2h (2026-11-06). Forge: `SVar:X:Sacrificed$CardPower` — "where X is the
// sacrificed creature's power": its power as it last existed on the battlefield.
#[test]
fn the_power_of_the_creature_sacrificed_as_a_cost() {
    let cards = [creature(
        "Test Ghoulcaller",
        "B",
        "Human",
        "1/1",
        "A:AB$ GainLife | Cost$ T Sac<1/Creature.Other/another creature> | LifeAmount$ X | SpellDescription$ Text.\nSVar:X:Sacrificed$CardPower",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Test Ghoulcaller"]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let power = {
        let st = sc.build();
        chars(&st, golem).power.unwrap()
    };
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Test Ghoulcaller", "Test Golem"]);
    let mut s = Script::new(sc.build());
    s.activate("Test Ghoulcaller");
    assert_eq!(s.kind(), ChoiceKind::Sacrifice);
    s.respond(Response::Pick(vec![0])).pass_all();
    assert_eq!(life(&s, P0), 40 + power);
    // Without a sacrifice in the cost there is no such creature: not a card.
    let bad = creature(
        "Test Ghoulcaller",
        "B",
        "Human",
        "1/1",
        "A:AB$ GainLife | Cost$ T | LifeAmount$ X | SpellDescription$ Text.\nSVar:X:Sacrificed$CardPower",
    );
    let b = forge_test_blockers(&bad);
    assert!(b.iter().any(|x| x.starts_with("invalid:")), "{b:?}");
}

// ---------------------------------------------------------------------------------------
// Entering with counters (D-052).

fn counters(s: &Script, id: ObjectId) -> u32 {
    s.st.object(id)
        .counters
        .get(&card_model::CounterKind::PlusOne)
        .copied()
        .unwrap_or(0)
}

// CR 614.1c, 614.12, 122.6 (2026-11-06). Forge: `K:etbCounter:P1P1:<n or count>` — "this
// creature enters with N +1/+1 counters on it".
#[test]
fn entering_with_counters() {
    let cards = [
        // CR 107.3m: "enters with X +1/+1 counters": the X it was cast with.
        creature(
            "Test Hydra",
            "X G",
            "Hydra",
            "0/0",
            "K:etbCounter:P1P1:X\nSVar:X:Count$xPaid",
        ),
        creature("Test Hatchling", "G", "Hydra", "0/0", "K:etbCounter:P1P1:2"),
        creature(
            "Test Swarmling",
            "G",
            "Insect",
            "0/0",
            "K:etbCounter:P1P1:X\nSVar:X:Count$Valid Land.YouCtrl",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest", "Forest"]);
    sc.cards(
        P1,
        Zone::Battlefield,
        &["Forest", "Forest", "Forest", "Forest"],
    );
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Hatchling", "Test Swarmling", "Test Hydra"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Hatchling").pay().pass_all();
    let hatchling = s.find(P0, Zone::Battlefield, "Test Hatchling").unwrap();
    assert_eq!(counters(&s, hatchling), 2);
    assert_eq!(pt(&s, hatchling), (2, 2));
    // "…for each land you control": mine, not the opponent's four.
    s.cast("Test Swarmling").pay().pass_all();
    let swarmling = s.find(P0, Zone::Battlefield, "Test Swarmling").unwrap();
    assert_eq!(counters(&s, swarmling), 3);
    // The Hydra, cast for X = 2 with three Forests.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Hydra");
    let mut s = Script::new(sc.build());
    s.cast("Test Hydra").x(2).pay().pass_all();
    let hydra = s.find(P0, Zone::Battlefield, "Test Hydra").unwrap();
    assert_eq!(counters(&s, hydra), 2);
    // Kinds of counters with rules of their own aren't built (D-084; other kinds are:
    // forge_counters.rs).
    let stunned =
        "Name:Test Battery\nManaCost:1\nTypes:Artifact\nK:etbCounter:STUN:3\nOracle:Text.";
    assert_eq!(
        forge_test_blockers(stunned),
        ["counter:STUN (rules of its own)"]
    );
}

// CR 614.1c, 614.12 (2026-11-06). Forge:
// `K:ETBReplacement:Other:<SVar>:Mandatory:Battlefield:Creature.Angel+YouCtrl+Other` with
// `DB$ PutCounter | ETB$ True | Defined$ ReplacedCard` — "each other Angel you control
// enters with an additional +1/+1 counter on it for each Angel you already control".
#[test]
fn other_permanents_enter_with_counters_for_each() {
    let cards = [
        creature(
            "Test Abbess",
            "W",
            "Angel",
            "2/2",
            "K:ETBReplacement:Other:AddExtraCounter:Mandatory:Battlefield:Creature.Angel+YouCtrl+Other\n\
             SVar:AddExtraCounter:DB$ PutCounter | ETB$ True | Defined$ ReplacedCard | CounterType$ P1P1 | CounterNum$ X | SpellDescription$ Text.\n\
             SVar:X:Count$Valid Angel.YouCtrl",
        ),
        creature("Test Seraph", "W", "Angel", "2/2", ""),
        creature("Test Squire", "W", "Human", "1/1", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Plains", "Plains", "Plains", "Plains"],
    );
    sc.cards(P1, Zone::Battlefield, &["Plains", "Test Seraph"]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Abbess", "Test Seraph", "Test Seraph", "Test Squire"],
    );
    sc.card(P1, Zone::Hand, "Test Seraph");
    let mut s = Script::new(sc.build());
    // The Matriarch herself gets nothing: her ability isn't there while she enters.
    s.cast("Test Abbess").pay().pass_all();
    let matriarch = s.find(P0, Zone::Battlefield, "Test Abbess").unwrap();
    assert_eq!(counters(&s, matriarch), 0);
    // The first Seraph: one Angel already there (the opponent's doesn't count).
    s.cast("Test Seraph").pay().pass_all();
    let mine = |s: &Script| -> Vec<u32> {
        s.st.zones
            .battlefield
            .iter()
            .filter(|&&id| s.name(id) == "Test Seraph" && s.st.object(id).owner == P0)
            .map(|&id| counters(s, id))
            .collect()
    };
    assert_eq!(mine(&s), [1]);
    // The second: two Angels already there.
    s.cast("Test Seraph").pay().pass_all();
    assert_eq!(mine(&s), [1, 2]);
    // Not an Angel: nothing.
    s.cast("Test Squire").pay().pass_all();
    let squire = s.find(P0, Zone::Battlefield, "Test Squire").unwrap();
    assert_eq!(counters(&s, squire), 0);
    // "As this enters, choose …" is the same keyword with another effect: D-076, tested in
    // forge_chosen.rs. Other effects there are named by their API.
    let banner = "Name:Test Banner\nManaCost:3\nTypes:Artifact\nK:ETBReplacement:Other:ChooseP\n\
        SVar:ChooseP:DB$ ChoosePlayer | Defined$ You | SpellDescription$ Text.\nOracle:Text.";
    assert_eq!(
        forge_test_blockers(banner),
        ["kw:ETBReplacement:ChoosePlayer"]
    );
}

// ---------------------------------------------------------------------------------------
// "Look at the top N cards" (D-055).

// CR 701.20e, 401.4 (2026-11-06). Forge: `Dig | DigNum$ 3 | ChangeValid$ Creature |
// Optional$ True` — "look at the top three cards of your library; you may reveal a creature
// card from among them and put it into your hand; put the rest on the bottom in any order".
#[test]
fn looking_at_the_top_cards_and_taking_one() {
    let cards = [spell(
        "Test Scouting",
        "G",
        "Sorcery",
        "A:SP$ Dig | DigNum$ 3 | ChangeNum$ 1 | Optional$ True | ChangeValid$ Creature | SpellDescription$ Text.",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    sc.card(P0, Zone::Library, "Test Giant"); // fourth from the top: not looked at
    sc.card(P0, Zone::Library, "Test Golem");
    sc.card(P0, Zone::Library, "Plains");
    sc.card(P0, Zone::Library, "Test Elf"); // top: Elf, Plains, Golem, Giant, Forests…
    sc.card(P0, Zone::Hand, "Test Scouting");
    let mut s = Script::new(sc.build());
    s.cast("Test Scouting").pay().pass_all();
    // Only the creatures among the three may be taken, and taking none is allowed.
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::ChooseCards));
    assert_eq!(s.decision().prim, Primitive::PickN { min: 0, max: 1 });
    assert_eq!(option_names(&s), ["Test Elf", "Test Golem"]);
    // The player sees all three; the opponent sees none of them.
    assert_eq!(engine_core::player_view(&s.st, P0).revealed.len(), 3);
    assert!(engine_core::player_view(&s.st, P1).revealed.is_empty());
    s.pick(
        "the Golem",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Test Golem"),
    );
    // The rest go to the bottom in an order the player chooses (last listed bottom-most).
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::OrderBottom));
    assert_eq!(option_names(&s), ["Test Elf", "Plains"]);
    s.respond(Response::Order(vec![1, 0]));
    assert!(s.find(P0, Zone::Hand, "Test Golem").is_some());
    let lib = library(&s, P0);
    assert_eq!(lib[0], "Test Giant", "the fourth card is now on top");
    assert_eq!(&lib[lib.len() - 2..], ["Plains", "Test Elf"]);
    // The card taken for being a creature was revealed; what was looked at was not.
    assert_eq!(
        s.count(|e| matches!(e, Event::Revealed { cards, .. } if cards.len() == 1)),
        1
    );
    assert_eq!(
        s.count(
            |e| matches!(e, Event::LookedAt { player, cards } if *player == P0 && cards.len() == 3)
        ),
        1
    );
}

// Forge: `Dig | ValidTgts$ Player | DestinationZone$ Graveyard | LibraryPosition2$ 0` — "look
// at the top two cards of target player's library; put one of them into their graveyard".
// The caster looks and chooses, not the library's owner.
#[test]
fn looking_at_another_players_library() {
    let cards = [spell(
        "Test Espionage",
        "U",
        "Sorcery",
        "A:SP$ Dig | ValidTgts$ Player | DestinationZone$ Graveyard | DigNum$ 2 | LibraryPosition2$ 0 | SpellDescription$ Text.",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island"]);
    sc.card(P1, Zone::Library, "Test Golem");
    sc.card(P1, Zone::Library, "Test Elf"); // P1's top: Elf, Golem
    sc.card(P0, Zone::Hand, "Test Espionage");
    let mut s = Script::new(sc.build());
    s.cast("Test Espionage").target_player(P1).pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::ChooseCards));
    assert_eq!(s.decision().prim, Primitive::PickN { min: 1, max: 1 });
    assert_eq!(option_names(&s), ["Test Elf", "Test Golem"]);
    // The caster sees P1's cards; P1 doesn't get to look.
    assert_eq!(engine_core::player_view(&s.st, P0).revealed.len(), 2);
    assert!(engine_core::player_view(&s.st, P1).revealed.is_empty());
    s.pick(
        "the Elf",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Test Elf"),
    );
    assert!(s.find(P1, Zone::Graveyard, "Test Elf").is_some());
    assert_eq!(library(&s, P1)[0], "Test Golem", "the other stays on top");
}

// Forge: `Dig | Defined$ Player | Reveal$ True | ChangeNum$ All | ChangeValid$ Land |
// DestinationZone$ Battlefield | Tapped$ True | DestinationZone2$ Exile` — "each player
// reveals the top two cards of their library, puts all land cards revealed this way onto
// the battlefield tapped, and exiles the rest". Nobody chooses anything.
#[test]
fn each_player_reveals_and_keeps_the_lands() {
    let clearing = |extra: &str| {
        spell(
            "Test Clearing",
            "G",
            "Sorcery",
            &format!(
                "A:SP$ Dig | DigNum$ 2 | Defined$ Player | Reveal$ True | ChangeNum$ All | ChangeValid$ Land | DestinationZone$ Battlefield | Tapped$ True | DestinationZone2$ Exile{extra} | SpellDescription$ Text."
            ),
        )
    };
    let card = clearing("");
    let mut sc = scenario(&[&card]);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    sc.card(P0, Zone::Library, "Test Golem");
    sc.card(P0, Zone::Library, "Plains");
    sc.card(P1, Zone::Library, "Test Elf");
    sc.card(P1, Zone::Library, "Test Giant");
    sc.card(P0, Zone::Hand, "Test Clearing");
    let mut s = Script::new(sc.build());
    s.cast("Test Clearing").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::Priority, "nothing to choose");
    let plains = s.find(P0, Zone::Battlefield, "Plains").expect("the land");
    assert!(s.st.object(plains).tapped);
    assert!(s.find(P0, Zone::Exile, "Test Golem").is_some());
    assert!(s.find(P1, Zone::Exile, "Test Elf").is_some());
    assert!(s.find(P1, Zone::Exile, "Test Giant").is_some());
    assert_eq!(
        s.count(|e| matches!(e, Event::Revealed { cards, .. } if cards.len() == 2)),
        2
    );
    // "Each player looks at … and puts one into their hand": Forge would have the caster
    // choose for everyone, which is not what such a card says. Not compiled.
    let each = spell(
        "Test Sharing",
        "U",
        "Sorcery",
        "A:SP$ Dig | DigNum$ 2 | Defined$ Player | SpellDescription$ Text.",
    );
    assert_eq!(forge_test_blockers(&each), ["Dig:each player chooses"]);
    // A script that reveals where the card looks (or orders where the card says random)
    // would play differently from the card: the card's text decides.
    let look = "Name:Test Peeking\nManaCost:U\nTypes:Sorcery\n\
        A:SP$ Dig | DigNum$ 3 | Reveal$ True | ChangeValid$ Creature | SpellDescription$ Text.\n\
        Oracle:Look at the top three cards of your library. Put a creature card from among them into your hand and the rest on the bottom of your library in any order.";
    assert_eq!(
        forge_test_blockers(look),
        ["Dig:look or reveal differs from the card"]
    );
    let order = look
        .replace(" | Reveal$ True", "")
        .replace("in any order", "in a random order");
    assert_eq!(
        forge_test_blockers(&order),
        ["Dig:order of the rest differs from the card"]
    );
}

// ---------------------------------------------------------------------------------------
// One "target" word with several targets (D-055).

// CR 115.3, 601.2c (2026-11-06). Forge: `TargetMin$ 2 | TargetMax$ 2` — "destroy two target
// creatures": two different creatures, destroyed at the same time.
#[test]
fn two_target_creatures() {
    let cards = [
        spell(
            "Test Twin Doom",
            "B",
            "Sorcery",
            "A:SP$ Destroy | ValidTgts$ Creature | TargetMin$ 2 | TargetMax$ 2 | SpellDescription$ Text.",
        ),
        // "Whenever another creature dies, you gain 1 life."
        creature(
            "Test Mourner",
            "B",
            "Cleric",
            "1/1",
            "T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Creature.Other | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:DB$ GainLife | LifeAmount$ 1",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    // One creature on the battlefield: there aren't two different targets.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Twin Doom");
    let s = Script::new(sc.build());
    assert!(!s.can_cast("Test Twin Doom"));
    // Two Mourners of the opponent's: each sees the other die (they die together), so
    // their controller gains 2 life.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    let a = sc.card(P1, Zone::Battlefield, "Test Mourner");
    let b = sc.card(P1, Zone::Battlefield, "Test Mourner");
    sc.card(P0, Zone::Hand, "Test Twin Doom");
    let mut s = Script::new(sc.build());
    s.cast("Test Twin Doom");
    assert_eq!(s.kind(), ChoiceKind::ChooseTargets { slot: 0 });
    assert_eq!(s.decision().prim, Primitive::PickN { min: 1, max: 1 });
    assert_eq!(s.options().len(), 2);
    s.target_object(a);
    // The second target must be another creature.
    assert_eq!(s.kind(), ChoiceKind::ChooseTargets { slot: 1 });
    assert_eq!(s.options().len(), 1);
    s.target_object(b).pay().pass_all();
    assert!(!on_battlefield(&s, a) && !on_battlefield(&s, b));
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(life(&s, P1), 42, "each Mourner saw the other die");
}

// CR 115.3, 601.2c, 608.2b (2026-11-06). Forge: `TargetMin$ 0 | TargetMax$ 2` — "up to two
// target creatures each get +2/+2 until end of turn".
#[test]
fn up_to_two_target_creatures() {
    let cards = [spell(
        "Test Rally",
        "G",
        "Instant",
        "A:SP$ Pump | ValidTgts$ Creature | TargetMin$ 0 | TargetMax$ 2 | NumAtt$ +2 | NumDef$ +2 | SpellDescription$ Text.",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = || {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Forest"]);
        let elf = sc.card(P0, Zone::Battlefield, "Test Elf");
        let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
        sc.card(P0, Zone::Hand, "Test Rally");
        (Script::new(sc.build()), elf, golem)
    };
    // Both.
    let (mut s, elf, golem) = build();
    let (elf_pt, golem_pt) = (pt(&s, elf), pt(&s, golem));
    s.cast("Test Rally");
    assert_eq!(s.decision().prim, Primitive::PickN { min: 0, max: 1 });
    s.target_object(elf).target_object(golem).pay().pass_all();
    assert_eq!(pt(&s, elf), (elf_pt.0 + 2, elf_pt.1 + 2));
    assert_eq!(pt(&s, golem), (golem_pt.0 + 2, golem_pt.1 + 2));
    // One, then "no more".
    let (mut s, elf, golem) = build();
    s.cast("Test Rally").target_object(golem);
    assert_eq!(s.kind(), ChoiceKind::ChooseTargets { slot: 1 });
    s.respond(Response::Pick(vec![])).pay().pass_all();
    assert_eq!(pt(&s, golem), (golem_pt.0 + 2, golem_pt.1 + 2));
    assert_eq!(pt(&s, elf), elf_pt);
    // None at all: the spell is still cast, and resolves doing nothing (it has no
    // targets, so CR 608.2b doesn't stop it).
    let (mut s, elf, golem) = build();
    s.cast("Test Rally").respond(Response::Pick(vec![])).pay();
    assert_eq!(s.st.stack.len(), 1);
    s.pass_all();
    assert!(s.st.stack.is_empty());
    assert_eq!((pt(&s, elf), pt(&s, golem)), (elf_pt, golem_pt));
    assert_eq!(s.count(|e| matches!(e, Event::Fizzled { .. })), 0);
    // With no creature anywhere it can be cast all the same.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    sc.card(P0, Zone::Hand, "Test Rally");
    let mut s = Script::new(sc.build());
    assert!(s.can_cast("Test Rally"));
    s.cast("Test Rally").pay();
    assert_eq!(s.st.stack.len(), 1);
}

// What isn't compiled: several targets for an instruction that isn't a single event, a
// number of targets that isn't a number, and an "up to" group followed by other targets
// (unchosen targets are simply missing from the end of the list).
#[test]
fn target_groups_not_compiled() {
    let several = spell(
        "Test Regrowth",
        "G",
        "Sorcery",
        "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Battlefield | ValidTgts$ Creature.YouOwn | TargetMin$ 2 | TargetMax$ 2 | SpellDescription$ Text.",
    );
    assert_eq!(
        forge_test_blockers(&several),
        ["targets:several for ChangeZone"]
    );
    let variable = spell(
        "Test Hail",
        "X R",
        "Sorcery",
        "A:SP$ DealDamage | ValidTgts$ Creature | TargetMin$ X | TargetMax$ X | NumDmg$ 1 | SpellDescription$ Text.\nSVar:X:Count$xPaid",
    );
    assert!(
        forge_test_blockers(&variable)
            .iter()
            .any(|b| b == "param:target.TargetMin")
    );
    let before = spell(
        "Test Tangle",
        "G",
        "Sorcery",
        "A:SP$ Tap | ValidTgts$ Creature | TargetMin$ 0 | TargetMax$ 2 | SubAbility$ DBDmg | SpellDescription$ Text.\n\
         SVar:DBDmg:DB$ DealDamage | ValidTgts$ Player | NumDmg$ 1",
    );
    assert_eq!(
        forge_test_blockers(&before),
        ["targets:up-to group before other targets"]
    );
}

// ---------------------------------------------------------------------------------------
// Amounts: cards in other zones, arithmetic, conditional values, domain, devotion (D-055).

fn gain(name: &str, cost: &str, x: &str) -> String {
    spell(
        name,
        cost,
        "Instant",
        &format!("A:SP$ GainLife | LifeAmount$ X | SpellDescription$ Text.\nSVar:X:{x}"),
    )
}

// CR 608.2h (2026-11-06): an amount is worked out when the instruction that needs it is
// applied — after the instructions before it. "Draw a card. You gain life equal to the
// number of cards in your hand."
#[test]
fn an_amount_is_counted_when_its_instruction_is_applied() {
    let cards = [spell(
        "Test Insight Surge",
        "U",
        "Sorcery",
        "A:SP$ Draw | NumCards$ 2 | SubAbility$ DBGain | SpellDescription$ Text.\n\
         SVar:DBGain:DB$ GainLife | LifeAmount$ X\nSVar:X:Count$CardsInYourHand",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island"]);
    sc.cards(P0, Zone::Hand, &["Test Insight Surge", "Forest"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Insight Surge").pay().pass_all();
    // The Forest and the two cards drawn: 3 (not the 1 card that was there before).
    assert_eq!(hand(&s, P0), 3);
    assert_eq!(life(&s, P0), 43);
}

// Forge's expression language (docs/forge/script-format.md §6): `Count$ValidGraveyard`,
// `Count$ValidHand`, arithmetic suffixes, `Count$Compare`, `SVar$`, `Number$`.
#[test]
fn counting_cards_in_other_zones_and_arithmetic() {
    let cards = [
        gain("Test Requiem", "W", "Count$ValidGraveyard Creature.YouOwn"),
        gain("Test Lament", "W", "Count$ValidGraveyard Creature"),
        gain("Test Doubling", "W", "Count$Valid Creature.YouCtrl/Twice"),
        gain("Test Surplus", "W", "Count$Valid Creature.YouCtrl/Plus.3"),
        gain("Test Halving", "W", "Count$ValidHand Card.YouOwn/HalfUp"),
        // "3 if you control two or more creatures, otherwise 1", through another variable.
        format!(
            "{}\nSVar:Y:Count$Valid Creature.YouCtrl",
            gain("Test Verdict", "W", "Count$Compare Y GE2.3.1")
        ),
        format!(
            "{}\nSVar:Y:Count$Valid Creature.YouCtrl/Times.2",
            gain("Test Chain", "W", "SVar$Y/Plus.1")
        ),
        gain("Test Seven", "W", "Number$7"),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let gained = |spell: &str, mine: usize| -> i32 {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Plains"]);
        for _ in 0..mine {
            sc.card(P0, Zone::Battlefield, "Test Elf");
        }
        sc.card(P1, Zone::Battlefield, "Test Golem");
        sc.cards(P0, Zone::Graveyard, &["Test Elf", "Test Golem", "Forest"]);
        sc.cards(P1, Zone::Graveyard, &["Test Elf"]);
        // The spell and four more cards in hand.
        sc.cards(
            P0,
            Zone::Hand,
            &[spell, "Forest", "Forest", "Forest", "Forest"],
        );
        let mut s = Script::new(sc.build());
        s.cast(spell).pay().pass_all();
        life(&s, P0) - 40
    };
    assert_eq!(
        gained("Test Requiem", 0),
        2,
        "creature cards in my graveyard"
    );
    assert_eq!(gained("Test Lament", 0), 3, "in all graveyards");
    assert_eq!(gained("Test Doubling", 2), 4);
    assert_eq!(gained("Test Surplus", 2), 5);
    // Four cards in hand once the spell is on the stack.
    assert_eq!(gained("Test Halving", 0), 2);
    assert_eq!(gained("Test Verdict", 2), 3);
    assert_eq!(gained("Test Verdict", 1), 1);
    assert_eq!(gained("Test Chain", 2), 5);
    assert_eq!(gained("Test Seven", 0), 7);
    // Only the first arithmetic suffix counts in Forge; what isn't understood is named.
    assert_eq!(
        forge_test_blockers(&gain("Test Odd", "W", "Count$Valid Creature.YouCtrl/Pow.2")),
        ["expr:math Pow"]
    );
}

// CR 700.5 (devotion), "domain", and the greatest power among creatures you control.
#[test]
fn devotion_domain_and_the_greatest_power() {
    let cards = [
        gain("Test Piety", "W", "Count$Devotion.Green"),
        gain("Test Dominion", "W", "Count$Domain"),
        gain(
            "Test Prowess",
            "W",
            "Count$Valid Creature.YouCtrl$GreatestCardPower",
        ),
        creature("Test Devotee", "G G", "Elf", "1/1", ""),
        creature("Test Zealot", "1 G", "Elf", "4/4", ""),
        // {G} in its cost, but not a permanent of mine: an opponent's.
        creature("Test Heretic", "G", "Elf", "9/9", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let gained = |spell: &str| -> i32 {
        let mut sc = scenario(&cards);
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Plains", "Plains", "Forest", "Swamp"],
        );
        sc.cards(P0, Zone::Battlefield, &["Test Devotee", "Test Zealot"]);
        sc.cards(P1, Zone::Battlefield, &["Test Heretic", "Island"]);
        sc.card(P0, Zone::Hand, spell);
        let mut s = Script::new(sc.build());
        s.cast(spell).pay().pass_all();
        life(&s, P0) - 40
    };
    // {G}{G} + {1}{G}: three green mana symbols among my permanents.
    assert_eq!(gained("Test Piety"), 3);
    // Plains, Forest, Swamp: three basic land types among my lands.
    assert_eq!(gained("Test Dominion"), 3);
    assert_eq!(gained("Test Prowess"), 4);
}

// ---------------------------------------------------------------------------------------
// Characteristic-defining abilities (D-055).

// CR 604.3, 613.4a, 208.2a (2026-11-06). Forge: `PT:*/*` with `S:Mode$ Continuous |
// CharacteristicDefining$ True | SetPower$ X | SetToughness$ X` — "this creature's power
// and toughness are each equal to the number of lands you control".
#[test]
fn power_and_toughness_defined_by_a_count() {
    let cda = |name: &str, pt: &str, set: &str, x: &str| {
        format!(
            "Name:{name}\nManaCost:G\nTypes:Creature Elemental\nPT:{pt}\n\
             S:Mode$ Continuous | CharacteristicDefining$ True | {set} | Description$ Text.\n\
             SVar:X:{x}\nOracle:Text."
        )
    };
    let cards = [
        cda(
            "Test Landwarden",
            "*/*",
            "SetPower$ X | SetToughness$ X",
            "Count$Valid Land.YouCtrl",
        ),
        // "*/3": only the power is defined.
        cda(
            "Test Handreader",
            "*/3",
            "SetPower$ X",
            "Count$CardsInYourHand",
        ),
        // Counts creature cards in all graveyards — where the Landwarden may be lying.
        cda(
            "Test Gravefeeder",
            "*/*",
            "SetPower$ X | SetToughness$ X",
            "Count$ValidGraveyard Creature",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.cards(P1, Zone::Battlefield, &["Forest", "Forest", "Forest"]);
    let warden = sc.card(P0, Zone::Battlefield, "Test Landwarden");
    let reader = sc.card(P0, Zone::Battlefield, "Test Handreader");
    let feeder = sc.card(P0, Zone::Battlefield, "Test Gravefeeder");
    let in_hand = sc.card(P0, Zone::Hand, "Test Landwarden");
    sc.card(P0, Zone::Hand, "Forest");
    // A Landwarden in a graveyard is a creature card there (and has its own count).
    let dead = sc.card(P1, Zone::Graveyard, "Test Landwarden");
    sc.card(P1, Zone::Graveyard, "Test Elf");
    let mut s = Script::new(sc.build());
    // On the battlefield: my two lands, not the opponent's three.
    assert_eq!(pt(&s, warden), (2, 2));
    // It works in every zone: in my hand it counts my lands, in the opponent's graveyard
    // the opponent's.
    assert_eq!(chars(&s.st, in_hand).power, Some(2));
    assert_eq!(chars(&s.st, dead).power, Some(3));
    // Two cards in hand; toughness as printed.
    assert_eq!(pt(&s, reader), (2, 3));
    // Two creature cards in graveyards.
    assert_eq!(pt(&s, feeder), (2, 2));
    // Playing a land: the Landwarden grows, the Handreader shrinks — continuously.
    s.pick("play the Forest", |_, o| {
        matches!(o, OptionDesc::PlayLand { .. })
    });
    assert_eq!(pt(&s, warden), (3, 3));
    assert_eq!(pt(&s, reader), (1, 3));
    assert_eq!(chars(&s.st, in_hand).power, Some(3));
    // What it can't be: a "*" nothing defines, and a count the layers can't make.
    let undefined = "Name:Test Riddle\nManaCost:G\nTypes:Creature Elemental\nPT:*/*\nOracle:Text.";
    assert_eq!(forge_test_blockers(undefined), ["pt:variable"]);
    let by_x = cda(
        "Test Riddle",
        "*/*",
        "SetPower$ X | SetToughness$ X",
        "Count$Valid Creature.powerGE4",
    );
    let b = forge_test_blockers(&by_x);
    assert!(
        b.iter()
            .any(|x| x == "static:characteristic-defining amount"),
        "{b:?}"
    );
}

// CR 704.5f (2026-11-06): a creature whose toughness is defined as 0 dies.
#[test]
fn a_creature_defined_as_zero_toughness_dies() {
    let cards = [
        "Name:Test Landwarden\nManaCost:G\nTypes:Creature Elemental\nPT:*/*\n\
         S:Mode$ Continuous | CharacteristicDefining$ True | SetPower$ X | SetToughness$ X | Description$ Text.\n\
         SVar:X:Count$Valid Land.YouCtrl\nOracle:Text.",
    ];
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    sc.card(P0, Zone::Hand, "Test Landwarden");
    // The opponent casts it with no land of their own... P0 has one Forest: a 1/1.
    let mut s = Script::new(sc.build());
    s.cast("Test Landwarden").pay().pass_all();
    let warden = s.find(P0, Zone::Battlefield, "Test Landwarden").unwrap();
    assert_eq!(pt(&s, warden), (1, 1));
    // Without the Forest it would be 0/0: a fresh game where it is put onto the
    // battlefield with no lands at all.
    let mut sc = scenario(&cards);
    let doomed = sc.card(P0, Zone::Battlefield, "Test Landwarden");
    let s = Script::new(sc.build());
    assert!(
        !on_battlefield(&s, doomed),
        "0 toughness: put into the graveyard"
    );
    assert!(s.find(P0, Zone::Graveyard, "Test Landwarden").is_some());
}

// Found by the soak: a static ability's condition looked at the cards in a graveyard, one
// of them had a "*" defined by counting permanents, and working that out asked for the very
// layered view that was being built — without end. Inside the layer walk a card that isn't
// on the battlefield is looked at as printed.
#[test]
fn a_condition_on_graveyard_cards_meets_a_card_with_a_defined_power() {
    let cards = [
        "Name:Test Landwarden\nManaCost:G\nTypes:Creature Elemental\nPT:*/*\n\
         S:Mode$ Continuous | CharacteristicDefining$ True | SetPower$ X | SetToughness$ X | Description$ Text.\n\
         SVar:X:Count$Valid Land.YouCtrl\nOracle:Text."
            .to_string(),
        creature(
            "Test Sage",
            "G",
            "Druid",
            "1/1",
            "S:Mode$ Continuous | Affected$ Card.Self | AddPower$ 2 | AddToughness$ 2 | IsPresent$ Creature.YouOwn | PresentZone$ Graveyard | PresentCompare$ GE2 | Description$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    let sage = sc.card(P0, Zone::Battlefield, "Test Sage");
    let warden = sc.card(P0, Zone::Battlefield, "Test Landwarden");
    sc.cards(P0, Zone::Graveyard, &["Test Landwarden", "Test Landwarden"]);
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, sage), (3, 3), "two creature cards in the graveyard");
    assert_eq!(pt(&s, warden), (1, 1));
}

// ---------------------------------------------------------------------------------------
// Cards chosen as an effect resolves, without targeting (D-055).

// CR 608.2d (2026-11-06). Forge: `ChangeZone | Origin$ Graveyard | Destination$ Hand |
// Hidden$ True | ChangeType$ Creature.YouOwn` — "return a creature card from your graveyard
// to your hand": nothing is targeted, the card is chosen on resolution.
#[test]
fn a_card_chosen_from_your_graveyard_on_resolution() {
    let cards = [
        spell(
            "Test Recollection",
            "B",
            "Sorcery",
            "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Hand | Hidden$ True | Mandatory$ True | ChangeType$ Creature.YouOwn | ChangeNum$ 1 | SpellDescription$ Text.",
        ),
        // Forge would offer every graveyard's creature cards for this filter; the engine's
        // choice is among the chooser's own cards. Not the same: not compiled.
        spell(
            "Test Graverobbing",
            "B",
            "Sorcery",
            "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Hand | Hidden$ True | ChangeType$ Creature | SpellDescription$ Text.",
        ),
    ];
    assert_eq!(
        forge_test_blockers(&cards[1]),
        ["ChangeZone:chosen among every player's cards"]
    );
    let cards = [cards[0].as_str()];
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    sc.cards(P0, Zone::Graveyard, &["Test Elf", "Forest", "Test Golem"]);
    sc.cards(P1, Zone::Graveyard, &["Test Giant"]);
    sc.card(P0, Zone::Hand, "Test Recollection");
    let mut s = Script::new(sc.build());
    // No target is chosen as it is cast.
    s.cast("Test Recollection").pay();
    assert_eq!(s.kind(), ChoiceKind::Priority);
    assert!(s.st.stack[0].targets.iter().all(Vec::is_empty));
    s.pass_all();
    // On resolution: my creature cards, not the land, not the opponent's Giant.
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::ChooseCards));
    assert_eq!(s.decision().prim, Primitive::PickN { min: 1, max: 1 });
    assert_eq!(option_names(&s), ["Test Elf", "Test Golem"]);
    s.pick(
        "the Golem",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Test Golem"),
    );
    assert!(s.find(P0, Zone::Hand, "Test Golem").is_some());
    assert!(s.find(P0, Zone::Graveyard, "Test Elf").is_some());
    // Public cards: nothing is "revealed".
    assert_eq!(s.count(|e| matches!(e, Event::Revealed { .. })), 0);
}

// Forge: `ChangeZone | Origin$ Battlefield | Destination$ Hand | Hidden$ True | Mandatory$
// True | ChangeType$ Creature.YouCtrl` — "when this enters, return a creature you control
// to its owner's hand" (it may be the creature itself); and a ChangeZone with neither a
// target nor `Defined$` moves the card it is on.
#[test]
fn returning_a_creature_you_control_and_returning_this() {
    let cards = [
        creature(
            "Test Gatecrasher",
            "U",
            "Drake",
            "2/2",
            "T:Mode$ ChangesZone | ValidCard$ Card.Self | Origin$ Any | Destination$ Battlefield | Execute$ TrigBounce | TriggerDescription$ Text.\n\
             SVar:TrigBounce:DB$ ChangeZone | Origin$ Battlefield | Destination$ Hand | Hidden$ True | Mandatory$ True | ChangeType$ Creature.YouCtrl | ChangeNum$ 1",
        ),
        creature(
            "Test Homing Sprite",
            "U",
            "Faerie",
            "1/1",
            "A:AB$ ChangeZone | Cost$ U | Origin$ Battlefield | Destination$ Hand | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    let elf = sc.card(P0, Zone::Battlefield, "Test Elf");
    let sprite = sc.card(P0, Zone::Battlefield, "Test Homing Sprite");
    sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Gatecrasher");
    let mut s = Script::new(sc.build());
    s.cast("Test Gatecrasher").pay().pass_all();
    // The trigger resolves: one of my three creatures (not the opponent's Golem), and one
    // must be returned.
    s.pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::ChooseCards));
    assert_eq!(s.decision().prim, Primitive::PickN { min: 1, max: 1 });
    assert_eq!(s.options().len(), 3);
    s.pick(
        "the Elf",
        |_, o| matches!(o, OptionDesc::Card { card } if *card == elf),
    );
    assert!(s.find(P0, Zone::Hand, "Test Elf").is_some());
    // "{U}: Return this creature to its owner's hand."
    s.activate("Test Homing Sprite").pay().pass_all();
    assert!(!on_battlefield(&s, sprite));
    assert!(s.find(P0, Zone::Hand, "Test Homing Sprite").is_some());
}

// ---------------------------------------------------------------------------------------
// Filters: mana value, "non-" colors and supertypes, toughness at most (D-055).

fn target_names(s: &Script) -> Vec<String> {
    s.options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Target {
                target: engine_core::stack::Target::Object(id),
            } => Some(s.name(*id)),
            _ => None,
        })
        .collect()
}

// CR 202.3, 105.2 (2026-11-06). Forge filter properties `cmcLE2`, `nonBlack`, `nonBasic`,
// `toughnessLE2`.
#[test]
fn filters_by_mana_value_color_and_supertype() {
    let kill = |name: &str, filter: &str| {
        spell(
            name,
            "B",
            "Instant",
            &format!("A:SP$ Destroy | ValidTgts$ {filter} | SpellDescription$ Text."),
        )
    };
    let cards = [
        kill("Test Cheap Shot", "Creature.cmcLE2"),
        kill("Test Costly Shot", "Creature.cmcGE3"),
        kill("Test Exact Shot", "Creature.cmcEQ1"),
        kill("Test Pale Shot", "Creature.nonBlack"),
        kill("Test Frail Shot", "Creature.toughnessLE2"),
        kill("Test Wasteland Shot", "Land.nonBasic"),
        creature("Test Imp", "B", "Imp", "1/1", ""),
        creature("Test Knight", "1 W", "Knight", "2/2", ""),
        creature("Test Ogre", "2 R", "Ogre", "3/3", ""),
        "Name:Test Tower\nManaCost:no cost\nTypes:Land\nA:AB$ Mana | Cost$ T | Produced$ C | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let targets = |spell: &str| -> Vec<String> {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Swamp"]);
        sc.cards(
            P1,
            Zone::Battlefield,
            &[
                "Test Imp",
                "Test Knight",
                "Test Ogre",
                "Forest",
                "Test Tower",
            ],
        );
        sc.card(P0, Zone::Hand, spell);
        let mut s = Script::new(sc.build());
        s.cast(spell);
        target_names(&s)
    };
    assert_eq!(targets("Test Cheap Shot"), ["Test Imp", "Test Knight"]);
    assert_eq!(targets("Test Costly Shot"), ["Test Ogre"]);
    assert_eq!(targets("Test Exact Shot"), ["Test Imp"]);
    assert_eq!(targets("Test Pale Shot"), ["Test Knight", "Test Ogre"]);
    assert_eq!(targets("Test Frail Shot"), ["Test Imp", "Test Knight"]);
    assert_eq!(targets("Test Wasteland Shot"), ["Test Tower"]);
}

// A card in a graveyard is matched on everything the filter says, not only its types:
// "return target green card from your graveyard to your hand" used to accept any card.
#[test]
fn graveyard_targets_are_matched_on_color_and_mana_value() {
    let fetch = |name: &str, filter: &str| {
        spell(
            name,
            "G",
            "Sorcery",
            &format!(
                "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Hand | ValidTgts$ {filter} | SpellDescription$ Text."
            ),
        )
    };
    let cards = [
        fetch("Test Regrowth", "Card.Green+YouCtrl"),
        fetch("Test Salvage", "Creature.cmcLE1+YouCtrl"),
        creature("Test Seedling", "G", "Plant", "0/1", ""),
        creature("Test Imp", "B", "Imp", "1/1", ""),
        creature("Test Ogre", "2 R", "Ogre", "3/3", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let targets = |spell: &str| -> Vec<String> {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Forest"]);
        sc.cards(
            P0,
            Zone::Graveyard,
            &["Test Seedling", "Test Imp", "Test Ogre", "Swamp"],
        );
        sc.card(P0, Zone::Hand, spell);
        let mut s = Script::new(sc.build());
        s.cast(spell);
        target_names(&s)
    };
    assert_eq!(targets("Test Regrowth"), ["Test Seedling"]);
    assert_eq!(targets("Test Salvage"), ["Test Seedling", "Test Imp"]);
}

// CR 107.3 (2026-11-06): {X} in a cost is announced. A script that defines X as something
// else ("X is the target's mana value") describes a cost that follows from a choice — not
// an X the caster may set to 0. Found compiled that way; now a blocker.
#[test]
fn an_x_cost_the_script_defines_is_not_an_announced_x() {
    let card = spell(
        "Test Exhumation",
        "X B",
        "Sorcery",
        "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Battlefield | ValidTgts$ Creature.YouCtrl | SpellDescription$ Text.\nSVar:X:Targeted$CardManaCost",
    );
    assert_eq!(forge_test_blockers(&card), ["cost:X defined by the script"]);
}

// CR 122.1, 608.2c (2026-11-06). Forge: `PutCounterAll | ValidCards$ Creature.YouCtrl |
// CounterType$ P1P1 | CounterNum$ 1` — "put a +1/+1 counter on each creature you control".
#[test]
fn counters_on_each_creature_you_control() {
    let cards = [spell(
        "Test Muster",
        "G",
        "Sorcery",
        "A:SP$ PutCounterAll | ValidCards$ Creature.YouCtrl | CounterType$ P1P1 | CounterNum$ 2 | SpellDescription$ Text.",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    let mine = sc.cards(P0, Zone::Battlefield, &["Test Elf", "Test Golem"]);
    let theirs = sc.card(P1, Zone::Battlefield, "Test Giant");
    sc.card(P0, Zone::Hand, "Test Muster");
    let mut s = Script::new(sc.build());
    s.cast("Test Muster").pay().pass_all();
    assert_eq!(counters(&s, mine[0]), 2);
    assert_eq!(counters(&s, mine[1]), 2);
    assert_eq!(counters(&s, theirs), 0);
    // One event for all of them.
    assert_eq!(
        s.count(|e| matches!(e, Event::CountersPut { .. })),
        2,
        "two creatures, one batch"
    );
    let other = spell(
        "Test Charge Up",
        "G",
        "Sorcery",
        "A:SP$ PutCounterAll | ValidCards$ Artifact | CounterType$ SHIELD | CounterNum$ 1 | SpellDescription$ Text.",
    );
    assert_eq!(
        forge_test_blockers(&other),
        ["counter:SHIELD (rules of its own)"]
    );
}

// ---------------------------------------------------------------------------------------
// Abilities that work from the hand or the graveyard; cycling (D-056).
// Text: docs/rules-notes/702.29-cycling-113.6-ability-zones.md.

fn can_activate(s: &Script, id: ObjectId) -> bool {
    s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == id))
}

// CR 702.29a, 602.2a, 113.6 (2026-11-06). Forge: `K:Cycling:2`.
#[test]
fn cycling_discards_the_card_from_hand_and_draws() {
    let cards = [creature(
        "Test Dune Beast",
        "5 R",
        "Beast",
        "4/4",
        "K:Cycling:2",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
    let in_hand = sc.card(P0, Zone::Hand, "Test Dune Beast");
    let in_play = sc.card(P0, Zone::Battlefield, "Test Dune Beast");
    let theirs = sc.card(P1, Zone::Hand, "Test Dune Beast");
    let mut s = Script::new(sc.build());
    // Only from the hand, and only its owner.
    assert!(can_activate(&s, in_hand));
    assert!(!can_activate(&s, in_play));
    assert!(!can_activate(&s, theirs));
    s.pick(
        "cycle",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == in_hand),
    )
    .pay();
    // The cost is paid: the card was shown and discarded, the ability is on the stack.
    assert_eq!(
        s.count(|e| matches!(e, Event::Revealed { player, cards } if *player == P0 && cards == &vec![in_hand])),
        1
    );
    assert_eq!(s.count(|e| matches!(e, Event::Discarded { .. })), 1);
    assert_eq!(s.count(|e| matches!(e, Event::Cycled { .. })), 1);
    assert!(s.find(P0, Zone::Graveyard, "Test Dune Beast").is_some());
    assert_eq!((hand(&s, P0), s.st.stack.len()), (0, 1));
    s.pass_all();
    assert_eq!(hand(&s, P0), 1, "draw a card");
    // No mana left: the opponent's copy stays theirs, and nothing more can be cycled.
    assert!(!s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { .. })));
}

// CR 702.29e–f (2026-11-06). Forge: `K:TypeCycling:Plains:2`, `K:TypeCycling:Basic:1`
// ("basic landcycling").
#[test]
fn typecycling_searches_for_a_card_of_the_type() {
    let cards = [
        creature(
            "Test Dune Strider",
            "5 W",
            "Camel",
            "4/4",
            "K:TypeCycling:Plains:2",
        ),
        creature(
            "Test Wayfinder",
            "5 G",
            "Scout",
            "4/4",
            "K:TypeCycling:Basic:1",
        ),
        // (A word in a filter is a subtype only if some card has it.)
        "Name:Test Steppe\nManaCost:no cost\nTypes:Land Plains\nOracle:Text.".to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.library(P0, "Plains", 1);
    sc.library(P0, "Test Steppe", 1);
    sc.library(P0, "Test Elf", 1);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Test Dune Strider", "Test Wayfinder"]);
    let mut s = Script::new(sc.build());
    s.activate("Test Dune Strider").pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::SearchLibrary));
    // The two Plains cards (basic or not); the search may fail to find (CR 701.23b).
    assert_eq!(s.decision().prim, Primitive::PickN { min: 0, max: 1 });
    assert_eq!(s.options().len(), 2);
    s.pick(
        "the basic Plains",
        |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Plains"),
    );
    assert!(s.find(P0, Zone::Hand, "Plains").is_some());
    assert!(s.count(|e| matches!(e, Event::Revealed { cards, .. } if cards.len() == 1)) >= 2);
    assert_eq!(s.count(|e| matches!(e, Event::Cycled { .. })), 1);
    // Basic landcycling: any basic land card (twenty Forests), not the Elf.
    s.activate("Test Wayfinder").pay().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::SearchLibrary));
    assert_eq!(s.options().len(), 20);
    assert_eq!(s.count(|e| matches!(e, Event::Cycled { .. })), 2);
}

// CR 702.29c–d (2026-11-06). Forge: `T:Mode$ Cycled | ValidCard$ Card.Self` ("when you
// cycle this card", from wherever it went) and `ValidCard$ Card.Other+YouOwn |
// TriggerZones$ Battlefield` ("whenever you cycle another card").
#[test]
fn cycle_triggers_on_the_card_and_on_permanents() {
    let cards = [
        spell(
            "Test Sandblast",
            "3 R",
            "Instant",
            "A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 3 | SpellDescription$ Text.\n\
             K:Cycling:R\n\
             T:Mode$ Cycled | ValidCard$ Card.Self | Execute$ TrigDmg | TriggerDescription$ Text.\n\
             SVar:TrigDmg:DB$ DealDamage | ValidTgts$ Any | NumDmg$ 1",
        ),
        creature(
            "Test Dune Watcher",
            "W",
            "Fox",
            "1/1",
            "T:Mode$ Cycled | ValidCard$ Card.Other+YouOwn | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:DB$ GainLife | LifeAmount$ 1",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Test Dune Watcher"]);
    sc.card(P1, Zone::Battlefield, "Test Dune Watcher");
    sc.card(P0, Zone::Hand, "Test Sandblast");
    let mut s = Script::new(sc.build());
    s.activate("Test Sandblast").pay();
    // Both of my triggers triggered while the cost was paid; the opponent's Watcher only
    // watches its own controller. They go on the stack above the cycling ability.
    if s.kind() == ChoiceKind::OrderTriggers {
        let n = s.options().len();
        assert_eq!(n, 2);
        s.respond(Response::Order((0..n).collect()));
    }
    s.target_player(P1);
    assert_eq!(s.st.stack.len(), 3);
    drive_until(&mut s, |s| s.st.stack.len() == 1);
    // The triggers have resolved; the draw hasn't happened yet.
    assert_eq!((life(&s, P0), life(&s, P1)), (41, 39));
    assert_eq!(hand(&s, P0), 0);
    s.pass_all();
    assert_eq!(hand(&s, P0), 1);
    assert_eq!(s.count(|e| matches!(e, Event::Discarded { .. })), 1);
}

// CR 113.6j, 113.6b (2026-11-06). Forge: `ActivationZone$ Hand` with `Discard<1/CARDNAME>`,
// `ActivationZone$ Graveyard` with `ExileFromGrave<1/CARDNAME>`.
#[test]
fn abilities_that_work_from_the_hand_or_the_graveyard() {
    let cards = [
        creature(
            "Test Channeler",
            "3 R",
            "Shaman",
            "2/2",
            "A:AB$ DealDamage | Cost$ 1 R Discard<1/CARDNAME> | ActivationZone$ Hand | ValidTgts$ Any | NumDmg$ 2 | SpellDescription$ Text.",
        ),
        creature(
            "Test Kind Soul",
            "3 W",
            "Spirit",
            "2/2",
            "A:AB$ GainLife | Cost$ 1 ExileFromGrave<1/CARDNAME> | ActivationZone$ Graveyard | LifeAmount$ 3 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 4]);
    let channeler = sc.card(P0, Zone::Hand, "Test Channeler");
    let channeler_in_play = sc.card(P0, Zone::Battlefield, "Test Channeler");
    let channeler_dead = sc.card(P0, Zone::Graveyard, "Test Channeler");
    let soul = sc.card(P0, Zone::Graveyard, "Test Kind Soul");
    let soul_in_hand = sc.card(P0, Zone::Hand, "Test Kind Soul");
    let soul_in_play = sc.card(P0, Zone::Battlefield, "Test Kind Soul");
    let their_soul = sc.card(P1, Zone::Graveyard, "Test Kind Soul");
    let mut s = Script::new(sc.build());
    for (id, yes) in [
        (channeler, true),
        (channeler_in_play, false),
        (channeler_dead, false),
        (soul, true),
        (soul_in_hand, false),
        (soul_in_play, false),
        (their_soul, false),
    ] {
        assert_eq!(can_activate(&s, id), yes, "{}", s.name(id));
    }
    s.pick(
        "channel",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == channeler),
    );
    s.target_player(P1).pay().pass_all();
    assert_eq!(life(&s, P1), 38);
    assert_eq!(s.st.zones.list(Zone::Graveyard, P0).len(), 3);
    // A discard, but not cycling.
    assert_eq!(s.count(|e| matches!(e, Event::Discarded { .. })), 1);
    assert_eq!(s.count(|e| matches!(e, Event::Cycled { .. })), 0);
    s.pick(
        "the soul",
        |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == soul),
    );
    s.pay();
    assert!(s.find(P0, Zone::Exile, "Test Kind Soul").is_some());
    s.pass_all();
    assert_eq!(life(&s, P0), 43);

    // A cost that uses the card where the ability doesn't work is not compiled.
    for script in [
        "A:AB$ GainLife | Cost$ T | ActivationZone$ Hand | LifeAmount$ 3 | SpellDescription$ Text.",
        "A:AB$ GainLife | Cost$ 1 Discard<1/CARDNAME> | LifeAmount$ 3 | SpellDescription$ Text.",
        "A:AB$ GainLife | Cost$ 1 ExileFromGrave<1/CARDNAME> | ActivationZone$ Hand | LifeAmount$ 3 | SpellDescription$ Text.",
    ] {
        let b = forge_test_blockers(&creature("Test Misfit", "W", "Spirit", "1/1", script));
        assert_eq!(b, ["activated:cost does not fit the zone"], "{script}");
    }
    let b = forge_test_blockers(&creature(
        "Test Misfit",
        "W",
        "Spirit",
        "1/1",
        "A:AB$ GainLife | Cost$ 1 | ActivationZone$ Exile | LifeAmount$ 3 | SpellDescription$ Text.",
    ));
    assert_eq!(b, ["activated:zone Exile"]);
}

// CR 400.7 (2026-11-06). Forge: `AB$ ChangeZone | Origin$ Graveyard | Destination$ Hand |
// ActivationZone$ Graveyard` — "return this card from your graveyard to your hand".
#[test]
fn returning_this_card_from_the_graveyard() {
    let cards = [
        creature(
            "Test Firebird",
            "3 R",
            "Phoenix",
            "2/2",
            "A:AB$ ChangeZone | Cost$ R | Origin$ Graveyard | Destination$ Hand | ActivationZone$ Graveyard | SpellDescription$ Text.",
        ),
        creature(
            "Test Revenant",
            "3 B",
            "Zombie",
            "2/2",
            "A:AB$ ChangeZone | Cost$ B | Origin$ Graveyard | Destination$ Battlefield | Tapped$ True | ActivationZone$ Graveyard | SpellDescription$ Text.",
        ),
        spell(
            "Test Grave Purge",
            "B",
            "Instant",
            "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Exile | ValidTgts$ Card | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = || {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Mountain", "Swamp"]);
        sc.card(P1, Zone::Battlefield, "Swamp");
        sc.cards(P0, Zone::Graveyard, &["Test Firebird", "Test Revenant"]);
        sc.card(P1, Zone::Hand, "Test Grave Purge");
        Script::new(sc.build())
    };
    let mut s = build();
    s.activate("Test Firebird").pay().pass_all();
    assert!(s.find(P0, Zone::Hand, "Test Firebird").is_some());
    s.activate("Test Revenant").pay().pass_all();
    let revenant = s.find(P0, Zone::Battlefield, "Test Revenant").unwrap();
    assert!(s.st.object(revenant).tapped);

    // In response the card is exiled: the ability does nothing — it doesn't follow the
    // card to another zone.
    let mut s = build();
    s.activate("Test Firebird").pay().pass();
    s.cast("Test Grave Purge").target("Test Firebird").pay();
    drive_until(&mut s, |s| s.st.stack.is_empty());
    assert!(s.find(P0, Zone::Exile, "Test Firebird").is_some());
    assert!(s.find(P0, Zone::Hand, "Test Firebird").is_none());
}

// "Another target creature", "another target creature card": not the object the ability
// is on. Forge: `ValidTgts$ Creature.Other`, `Creature.YouOwn+Other`.
#[test]
fn another_target_is_not_the_abilitys_source() {
    let cards = [
        creature(
            "Test Mentor",
            "W",
            "Cleric",
            "1/1",
            "A:AB$ PutCounter | Cost$ 1 | ValidTgts$ Creature.Other | CounterType$ P1P1 | CounterNum$ 1 | SpellDescription$ Text.",
        ),
        creature(
            "Test Brigadier",
            "W",
            "Soldier",
            "2/2",
            "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigCounter | TriggerDescription$ Text.\n\
             SVar:TrigCounter:DB$ PutCounter | ValidTgts$ Creature.Other | CounterType$ P1P1 | CounterNum$ 2",
        ),
        creature(
            "Test Soultender",
            "B",
            "Spirit",
            "1/1",
            "A:AB$ ChangeZone | Cost$ B ExileFromGrave<1/CARDNAME> | ActivationZone$ Graveyard | Origin$ Graveyard | Destination$ Hand | ValidTgts$ Creature.YouOwn+Other | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    // Alone, the Mentor has nothing to target: its ability can't be activated at all.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    let mentor = sc.card(P0, Zone::Battlefield, "Test Mentor");
    sc.card(P0, Zone::Hand, "Test Brigadier");
    let mut s = Script::new(sc.build());
    assert!(!can_activate(&s, mentor));
    // The Captain enters: its trigger may target the Mentor, not the Captain.
    s.cast("Test Brigadier").pay().pass_all();
    assert!(matches!(s.kind(), ChoiceKind::ChooseTargets { .. }));
    assert_eq!(target_names(&s), ["Test Mentor"]);
    s.target("Test Mentor").pass_all();
    assert_eq!(pt(&s, mentor), (3, 3));
    // Now the Mentor can target the Captain — and only the Captain.
    s.activate("Test Mentor");
    assert_eq!(target_names(&s), ["Test Brigadier"]);

    // In the graveyard: the Soultender can't return itself (it is exiled to pay).
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    let tender = sc.card(P0, Zone::Graveyard, "Test Soultender");
    let s = Script::new(sc.build());
    assert!(!can_activate(&s, tender));
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    sc.cards(P0, Zone::Graveyard, &["Test Soultender", "Test Elf"]);
    let mut s = Script::new(sc.build());
    s.activate("Test Soultender");
    assert_eq!(target_names(&s), ["Test Elf"]);
}

// ---------------------------------------------------------------------------------------
// More trigger events and counted conditions (D-057).
// Text: docs/rules-notes/603-triggered-abilities.md.

fn bell(name: &str, trigger: &str, gain: u32) -> String {
    format!(
        "Name:{name}\nManaCost:1\nTypes:Enchantment\n\
         T:Mode$ {trigger} | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
         SVar:TrigGain:DB$ GainLife | LifeAmount$ {gain}\nOracle:Text."
    )
}

fn at(s: &Script, p: PlayerId, step: Step) -> bool {
    s.st.turn.active == p && s.st.turn.step == step
}

// CR 603.2b, 505.1, 507.2, 511.2, 513.1a (2026-11-06). Forge: `Mode$ Phase` with `Phase$
// Main1 | BeginCombat | EndCombat | End of Turn` and `ValidPlayer$ You | Opponent | Player`.
#[test]
fn step_triggers_at_main_phase_combat_and_end_step() {
    let cards = [
        bell(
            "Test Dawn Chime",
            "Phase | Phase$ Main1 | ValidPlayer$ You",
            1000,
        ),
        bell(
            "Test War Drum",
            "Phase | Phase$ BeginCombat | ValidPlayer$ You",
            1,
        ),
        bell(
            "Test Truce Bell",
            "Phase | Phase$ EndCombat | ValidPlayer$ Player",
            10,
        ),
        bell(
            "Test Vesper",
            "Phase | Phase$ End of Turn | ValidPlayer$ Opponent",
            100,
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &[
            "Test Dawn Chime",
            "Test War Drum",
            "Test Truce Bell",
            "Test Vesper",
        ],
    );
    sc.turn(P0, Step::Upkeep);
    let mut s = Script::new(sc.build());
    // My precombat main phase, then the beginning of my combat. (A trigger is on the
    // stack before anyone gets priority in the step.)
    drive_until(&mut s, |s| {
        at(s, P0, Step::PrecombatMain) && s.st.stack.is_empty()
    });
    assert_eq!(life(&s, P0), 1040);
    drive_until(&mut s, |s| {
        at(s, P0, Step::BeginningOfCombat) && s.st.stack.is_empty()
    });
    assert_eq!(life(&s, P0), 1041);
    // "At end of combat": there was no attack, the step still happens (CR 508.8 skips
    // only the declare blockers and combat damage steps).
    drive_until(&mut s, |s| at(s, P0, Step::PostcombatMain));
    assert_eq!(life(&s, P0), 1051);
    // My own end step isn't "each opponent's end step".
    drive_until(&mut s, |s| at(s, P1, Step::Upkeep));
    assert_eq!(life(&s, P0), 1051);
    // The opponent's turn: no main-phase or combat trigger of mine, but each end of combat
    // and their end step.
    drive_until(&mut s, |s| at(s, P0, Step::Upkeep));
    assert_eq!(life(&s, P0), 1161);
}

// CR 603.6c, 603.10a (2026-11-06). Forge: `Mode$ ChangesZone | Origin$ Battlefield` with
// `Destination$ Any` or none — "leaves the battlefield", for any zone, looking back.
#[test]
fn leaves_the_battlefield_triggers_for_any_zone() {
    let mover = |name: &str, dest: &str| {
        spell(
            name,
            "U",
            "Instant",
            &format!(
                "A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ {dest} | SpellDescription$ Text."
            ),
        )
    };
    let cards = [
        creature(
            "Test Departing Spirit",
            "U",
            "Spirit",
            "1/1",
            "T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Any | ValidCard$ Card.Self | Execute$ TrigDraw | TriggerDescription$ Text.\n\
             SVar:TrigDraw:DB$ Draw | NumCards$ 1",
        ),
        bell(
            "Test Mourner",
            "ChangesZone | Origin$ Battlefield | ValidCard$ Creature.YouCtrl",
            1,
        ),
        mover("Test Unsummoning", "Hand"),
        mover("Test Banishing", "Exile"),
        spell(
            "Test Drowning",
            "U",
            "Instant",
            "A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island"; 4]);
    sc.card(P0, Zone::Battlefield, "Test Mourner");
    let spirits = sc.cards(P0, Zone::Battlefield, &["Test Departing Spirit"; 3]);
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(
        P0,
        Zone::Hand,
        &[
            "Test Unsummoning",
            "Test Banishing",
            "Test Drowning",
            "Test Unsummoning",
        ],
    );
    let mut s = Script::new(sc.build());
    let draws = |s: &Script| s.count(|e| matches!(e, Event::Drew { player, .. } if *player == P0));
    for (i, (spell, to)) in [
        ("Test Unsummoning", Zone::Hand),
        ("Test Banishing", Zone::Exile),
        ("Test Drowning", Zone::Graveyard),
    ]
    .into_iter()
    .enumerate()
    {
        s.cast(spell).target_object(spirits[i]).pay();
        drive_until(&mut s, |s| {
            s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
        });
        assert!(s.find(P0, to, "Test Departing Spirit").is_some(), "{spell}");
        // The Spirit's own trigger (it looks back: the Spirit is gone) and the Mourner's.
        assert_eq!((draws(&s), life(&s, P0)), (i + 1, 41 + i as i32), "{spell}");
    }
    // An opponent's creature leaving isn't "a creature you control".
    s.cast("Test Unsummoning").target_object(golem).pay();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!((draws(&s), life(&s, P0)), (3, 43));
}

// CR 601.2i (2026-11-06). Forge: `Mode$ SpellCast` with `ValidCard$ Card.nonCreature`,
// `Elf,Goblin`, `Card.cmcGE4`, `Card.Blue`: what the spell is; `ValidActivatingPlayer$`:
// whose it is.
#[test]
fn cast_triggers_ask_what_the_spell_is() {
    let cards = [
        bell(
            "Test Chanter",
            "SpellCast | ValidCard$ Card.nonCreature | ValidActivatingPlayer$ You",
            1,
        ),
        bell("Test Kindred Bell", "SpellCast | ValidCard$ Elf,Goblin", 10),
        bell(
            "Test Weighty Bell",
            "SpellCast | ValidCard$ Card.cmcGE4 | ValidActivatingPlayer$ You",
            100,
        ),
        bell(
            "Test Azure Bell",
            "SpellCast | ValidCard$ Card.Blue | ValidActivatingPlayer$ Opponent",
            1000,
        ),
        creature("Test Sproutling", "G", "Elf", "1/1", ""),
        creature("Test Cave Sneak", "R", "Goblin", "1/1", ""),
        creature("Test Colossus", "4", "Golem", "4/4", ""),
        spell(
            "Test Pondering",
            "U",
            "Instant",
            "A:SP$ Draw | NumCards$ 1 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &[
            "Test Chanter",
            "Test Kindred Bell",
            "Test Weighty Bell",
            "Test Azure Bell",
        ],
    );
    sc.cards(P0, Zone::Battlefield, &["Forest", "Island"]);
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 4]);
    sc.card(P1, Zone::Battlefield, "Island");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Sproutling", "Test Pondering", "Test Colossus"],
    );
    sc.card(P1, Zone::Hand, "Test Pondering");
    let mut s = Script::new(sc.build());
    let settle = |s: &mut Script| {
        drive_until(s, |s| {
            s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
        })
    };
    // An Elf creature spell: the Kindred Bell only.
    s.cast("Test Sproutling").pay();
    settle(&mut s);
    assert_eq!(life(&s, P0), 50);
    // My blue instant: noncreature (+1); blue, but not an opponent's.
    s.cast("Test Pondering").pay();
    // In response the opponent casts theirs: the Azure Bell (+1000), not the Chanter.
    // (The Chanter's trigger is still on the stack, above my spell.)
    drive_until(&mut s, |s| {
        s.who() == P1 && s.kind() == ChoiceKind::Priority
    });
    assert_eq!((life(&s, P0), s.st.stack.len()), (50, 2));
    s.cast("Test Pondering").pay();
    settle(&mut s);
    assert_eq!(life(&s, P0), 1051);
    // Mana value 4, a creature that is neither Elf nor Goblin.
    s.cast("Test Colossus").pay();
    settle(&mut s);
    assert_eq!(life(&s, P0), 1151);
}

// CR 603.4 (2026-11-06). Forge: `IsPresent$ … | PresentCompare$ EQ1` and `CheckSVar$ X |
// SVarCompare$ GE3` — "if [something counted] is exactly / at least N", asked when the
// event happens and again as the ability resolves; on a static ability, "as long as".
#[test]
fn conditions_that_compare_a_count() {
    let cards = [
        bell(
            "Test Lone Vigil",
            "Phase | Phase$ BeginCombat | ValidPlayer$ You | IsPresent$ Creature.YouCtrl | PresentCompare$ EQ1",
            5,
        ),
        format!(
            "{}\nSVar:X:Count$Valid Land.YouCtrl",
            bell(
                "Test Land Census",
                "Phase | Phase$ BeginCombat | ValidPlayer$ You | CheckSVar$ X | SVarCompare$ GE3",
                100,
            )
            .replace("\nOracle:Text.", "")
        ) + "\nOracle:Text.",
        creature(
            "Test Scholar",
            "U",
            "Wizard",
            "1/1",
            "S:Mode$ Continuous | Affected$ Card.Self | AddPower$ 2 | AddToughness$ 2 | CheckSVar$ X | SVarCompare$ LT2 | Description$ Text.\n\
             SVar:X:Count$CardsInYourHand",
        ),
        SMITE.to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = |creatures: usize, lands: usize| {
        let mut sc = scenario(&cards);
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Test Lone Vigil", "Test Land Census"],
        );
        sc.cards(P0, Zone::Battlefield, &vec!["Test Golem"; creatures]);
        sc.cards(P0, Zone::Battlefield, &vec!["Forest"; lands]);
        sc.card(P1, Zone::Battlefield, "Swamp");
        sc.card(P1, Zone::Hand, "Test Smiting");
        Script::new(sc.build())
    };
    let combat = |s: &mut Script| {
        drive_until(s, |s| {
            at(s, P0, Step::DeclareAttackers) || at(s, P0, Step::PostcombatMain)
        })
    };
    // Exactly one creature, three lands: both.
    let mut s = build(1, 3);
    combat(&mut s);
    assert_eq!(life(&s, P0), 145);
    // Two creatures, two lands: neither.
    let mut s = build(2, 2);
    combat(&mut s);
    assert_eq!(life(&s, P0), 40);
    // No creature: "exactly one" is not "at most one".
    let mut s = build(0, 3);
    combat(&mut s);
    assert_eq!(life(&s, P0), 140);
    // The one creature dies in response to the trigger: asked again on resolution, the
    // condition fails and the ability does nothing.
    let mut s = build(1, 0);
    drive_until(&mut s, |s| {
        s.who() == P1 && s.kind() == ChoiceKind::Priority && !s.st.stack.is_empty()
    });
    s.cast("Test Smiting").target("Test Golem").pay();
    combat(&mut s);
    assert_eq!(life(&s, P0), 40);

    // "As long as you have fewer than two cards in hand, this gets +2/+2."
    let mut sc = scenario(&cards);
    let scholar = sc.card(P0, Zone::Battlefield, "Test Scholar");
    sc.cards(P0, Zone::Hand, &["Forest", "Forest"]);
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, scholar), (1, 1));
    s.pick("a land", |_, o| matches!(o, OptionDesc::PlayLand { .. }));
    assert_eq!(pt(&s, scholar), (3, 3));
}

// ---------------------------------------------------------------------------------------
// Damage triggers; "that player", "that much"; the enchanted creature (D-058).
// Text: docs/rules-notes/603-triggered-abilities.md (last section).

fn gain_x(name: &str, pt: &str, kw: &str, trigger: &str) -> String {
    creature(
        name,
        "2",
        "Beast",
        pt,
        &format!(
            "{kw}T:Mode$ {trigger} | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:DB$ GainLife | LifeAmount$ X\n\
             SVar:X:TriggerCount$DamageAmount"
        ),
    )
}

/// Play combat out to the postcombat main phase, assigning combat damage where asked: the
/// least each blocker must get and whatever is left to the last one; a trampler's excess
/// to the player.
fn fight(s: &mut Script) {
    loop {
        drive_until(s, |s| {
            s.kind() == ChoiceKind::AssignCombatDamage || s.st.turn.step == Step::PostcombatMain
        });
        if s.next.is_none() || s.kind() != ChoiceKind::AssignCombatDamage {
            return;
        }
        match s.decision().prim.clone() {
            Primitive::Allocate { total, per_min, .. } => {
                let mut given = per_min.clone();
                *given.last_mut().unwrap() += total - per_min.iter().sum::<u32>();
                s.respond(Response::Allocate(given));
            }
            // Trample: how much goes on to the player — all that may.
            Primitive::PickNumber { max, .. } => {
                s.respond(Response::Number(max));
            }
            other => panic!("{other:?}"),
        }
    }
}

fn library_size(s: &Script, p: PlayerId) -> usize {
    s.st.zones.list(Zone::Library, p).len()
}

// CR 120.2, 510.3a (2026-11-06). Forge: `Mode$ DamageDone` with `ValidSource$ Card.Self`,
// `ValidTarget$ Player | Opponent`, `CombatDamage$ True`; in the ability `Defined$
// TriggeredTarget` ("that player") and `TriggerCount$DamageAmount` ("that many").
#[test]
fn deals_damage_triggers_by_recipient_and_kind_of_damage() {
    let cards = [
        creature(
            "Test Cutpurse",
            "U",
            "Rogue",
            "3/1",
            "T:Mode$ DamageDone | ValidSource$ Card.Self | ValidTarget$ Player | CombatDamage$ True | Execute$ TrigMill | TriggerDescription$ Text.\n\
             SVar:TrigMill:DB$ Mill | Defined$ TriggeredTarget | NumCards$ X\n\
             SVar:X:TriggerCount$DamageAmount",
        ),
        creature(
            "Test Sparkmage",
            "R",
            "Shaman",
            "1/1",
            "A:AB$ DealDamage | Cost$ T | ValidTgts$ Any | NumDmg$ 1 | SpellDescription$ Text.\n\
             T:Mode$ DamageDone | ValidSource$ Card.Self | ValidTarget$ Opponent | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:DB$ GainLife | LifeAmount$ 1",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    let thief = sc.card(P0, Zone::Battlefield, "Test Cutpurse");
    let mages = sc.cards(P0, Zone::Battlefield, &["Test Sparkmage", "Test Sparkmage"]);
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    let mut s = Script::new(sc.build());
    let (mine, theirs) = (library_size(&s, P0), library_size(&s, P1));
    // Noncombat damage to a creature: nothing. To the opponent: the Sparkmage's trigger.
    let ping = |s: &mut Script, mage: ObjectId| {
        s.pick(
            "ping",
            |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == mage),
        );
    };
    ping(&mut s, mages[0]);
    s.target_object(golem).pass_all();
    assert_eq!(life(&s, P0), 40);
    ping(&mut s, mages[1]);
    s.target_player(P1);
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!((life(&s, P0), life(&s, P1)), (41, 39));
    // Combat damage to the player: that player mills that many cards.
    attack_with(&mut s, &[thief]);
    drive_until(&mut s, |s| at(s, P0, Step::PostcombatMain));
    assert_eq!(life(&s, P1), 36);
    assert_eq!(
        (library_size(&s, P0), library_size(&s, P1)),
        (mine, theirs - 3)
    );
}

// CR 510.2, 120.9 (2026-11-06). Forge: `DamageDoneOnce` (once for each recipient: "is
// dealt damage", "one or more creatures you control deal combat damage to a player") and
// `DamageDealtOnce` (once for each source: "deals damage").
#[test]
fn simultaneous_damage_triggers_once_where_the_card_says_so() {
    let cards = [
        gain_x(
            "Test Enraged Beast",
            "1/9",
            "",
            "DamageDoneOnce | ValidTarget$ Card.Self",
        ),
        gain_x(
            "Test Soulful Giant",
            "5/5",
            "K:Trample\n",
            "DamageDealtOnce | ValidSource$ Card.Self",
        ),
        bell(
            "Test War Banner",
            "DamageDoneOnce | CombatDamage$ True | ValidSource$ Creature.YouCtrl | ValidTarget$ Player",
            100,
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let triggers = |s: &Script, id: ObjectId| {
        s.count(|e| matches!(e, Event::Triggered { source, .. } if *source == id))
    };
    // Blocked by two creatures at once: 4 damage, one trigger, "that much" is all of it.
    let mut sc = scenario(&cards);
    let beast = sc.card(P0, Zone::Battlefield, "Test Enraged Beast");
    let blockers = sc.cards(P1, Zone::Battlefield, &["Test Golem", "Test Golem"]);
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[beast]);
    block_with(&mut s, &[(blockers[0], beast), (blockers[1], beast)]);
    fight(&mut s);
    assert_eq!((triggers(&s, beast), life(&s, P0)), (1, 44));

    // A trampler deals 3 to its blocker and 2 to the player: one "deals damage" trigger
    // for 5. Two creatures connect: the Banner triggers once.
    let mut sc = scenario(&cards);
    let giant = sc.card(P0, Zone::Battlefield, "Test Soulful Giant");
    let elf = sc.card(P0, Zone::Battlefield, "Test Elf");
    let banner = sc.card(P0, Zone::Battlefield, "Test War Banner");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[giant, elf]);
    block_with(&mut s, &[(golem, giant)]);
    fight(&mut s);
    assert_eq!(life(&s, P1), 40 - 2 - 1);
    assert_eq!((triggers(&s, giant), triggers(&s, banner)), (1, 1));
    assert_eq!(life(&s, P0), 40 + 5 + 100);
}

// Forge: `ValidSource$ Card.AttachedBy`, `ValidCard$ Card.EnchantedBy` — "enchanted
// creature" as what a trigger is about (CR 303.4, 603.10a for its dying).
#[test]
fn triggers_about_the_enchanted_creature() {
    let cards = [
        "Name:Test Mantle\nManaCost:U\nTypes:Enchantment Aura\nK:Enchant:Creature\n\
         T:Mode$ DamageDone | ValidSource$ Card.AttachedBy | ValidTarget$ Player | CombatDamage$ True | TriggerZones$ Battlefield | Execute$ TrigDraw | TriggerDescription$ Text.\n\
         SVar:TrigDraw:DB$ Draw | NumCards$ 1\n\
         T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.EnchantedBy | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
         SVar:TrigGain:DB$ GainLife | LifeAmount$ 7\nOracle:Text."
            .to_string(),
        SMITE.to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    let worn = sc.card(P0, Zone::Battlefield, "Test Elf");
    let bare = sc.card(P0, Zone::Battlefield, "Test Elf");
    let mantle = sc.card(P0, Zone::Battlefield, "Test Mantle");
    sc.attach(mantle, worn);
    sc.card(P0, Zone::Battlefield, "Swamp");
    sc.card(P0, Zone::Hand, "Test Smiting");
    let mut s = Script::new(sc.build());
    let draws = |s: &Script| s.count(|e| matches!(e, Event::Drew { player, .. } if *player == P0));
    // Both connect; only the enchanted one's damage draws a card.
    attack_with(&mut s, &[worn, bare]);
    drive_until(&mut s, |s| at(s, P0, Step::PostcombatMain));
    assert_eq!((life(&s, P1), draws(&s)), (38, 1));
    // The other creature dies: nothing. The enchanted one dies: the Aura sees it go.
    s.cast("Test Smiting").target_object(worn).pay();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(life(&s, P0), 47);
    assert!(s.find(P0, Zone::Graveyard, "Test Mantle").is_some());
}

// What a Forge script leaves out that the card says is found by reading the compiled card
// back against its Oracle text.
#[test]
fn words_of_the_oracle_text_the_script_leaves_out() {
    let card = |oracle: &str, rest: &str| {
        format!(
            "Name:Test Oddity\nManaCost:G\nTypes:Creature Ooze\nPT:1/1\n{rest}\nOracle:{oracle}"
        )
    };
    let upkeep_counter = "T:Mode$ Phase | Phase$ Upkeep | ValidPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigCounter | TriggerDescription$ Text.\n\
        SVar:TrigCounter:DB$ PutCounter | Defined$ Self | CounterType$ P1P1 | CounterNum$ 1";
    // The script puts the counter; the card lets its controller choose.
    assert_eq!(
        forge_test_blockers(&card(
            "At the beginning of your upkeep, you may put a +1/+1 counter on Test Oddity.",
            upkeep_counter
        )),
        ["oracle:says may"]
    );
    assert!(
        forge_test_blockers(&card(
            "At the beginning of your upkeep, put a +1/+1 counter on Test Oddity. (You may not skip this.)",
            upkeep_counter
        ))
        .is_empty()
    );
    // "Another target creature" on an ability whose script targets any creature.
    let pump = "A:AB$ Pump | Cost$ Sac<1/CARDNAME> | ValidTgts$ Creature | NumAtt$ +1 | SpellDescription$ Text.";
    assert_eq!(
        forge_test_blockers(&card(
            "Sacrifice Test Oddity: Another target creature gets +1/+0 until end of turn.",
            pump
        )),
        ["oracle:another target"]
    );
    assert!(
        forge_test_blockers(&card(
            "Sacrifice Test Oddity: Another target creature gets +1/+0 until end of turn.",
            &pump.replace("ValidTgts$ Creature", "ValidTgts$ Creature.Other")
        ))
        .is_empty()
    );

    // "Target creature" on the card, "target creature you control" in the script (a hint
    // for Forge's AI that became a rule).
    let own = pump.replace("ValidTgts$ Creature", "ValidTgts$ Creature.YouCtrl+Other");
    assert_eq!(
        forge_test_blockers(&card(
            "Sacrifice Test Oddity: Another target creature gets +1/+0 until end of turn.",
            &own
        )),
        ["oracle:target you control"]
    );
    assert!(
        forge_test_blockers(&card(
            "Sacrifice Test Oddity: Another target creature you control gets +1/+0 until end of turn.",
            &own
        ))
        .is_empty()
    );

    // "You may search": a plain search in the script is the optional one when the card has
    // one search and says "may" once — the player can leave the library unshuffled.
    let seeker = card(
        "When Test Oddity enters, you may search your library for a basic land card, reveal it, put it into your hand, then shuffle.",
        "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigSearch | TriggerDescription$ Text.\n\
         SVar:TrigSearch:DB$ ChangeZone | Origin$ Library | Destination$ Hand | ChangeType$ Land.Basic | ChangeNum$ 1",
    );
    let mut sc = scenario(&[&seeker]);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Oddity");
    let mut s = Script::new(sc.build());
    s.cast("Test Oddity").pay().pass_all().pass_all();
    assert_eq!((s.who(), s.kind()), (P0, ChoiceKind::MayAbility));
    let shuffles = s.count(|e| matches!(e, Event::Shuffled { .. }));
    s.pick("no", |_, o| matches!(o, OptionDesc::Decline));
    assert_eq!(s.count(|e| matches!(e, Event::Shuffled { .. })), shuffles);
    assert_eq!(s.kind(), ChoiceKind::Priority);
}

// ---------------------------------------------------------------------------------------
// The card a dead permanent became; spells that go elsewhere; ownership (D-059).

const SEIZURE: &str = "Name:Test Seizure\nManaCost:2 U U\nTypes:Enchantment Aura\nK:Enchant:Creature\n\
    S:Mode$ Continuous | AffectedDefined$ Enchanted | GainControl$ You | Description$ Text.\nOracle:Text.";

// CR 400.7, 603.6c (2026-11-06). Forge: `Defined$ TriggeredNewCardLKICopy` in a "dies"
// trigger — "return it to its owner's hand": the card where it went, while it is there.
// `Creature.YouOwn`: whose card it is, whoever controlled it (CR 108.3).
#[test]
fn the_card_a_dead_permanent_became() {
    let cards = [
        creature(
            "Test Loyal Hound",
            "W",
            "Dog",
            "1/1",
            "T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self | Execute$ TrigReturn | TriggerDescription$ Text.\n\
             SVar:TrigReturn:DB$ ChangeZone | Origin$ Graveyard | Destination$ Hand | Defined$ TriggeredNewCardLKICopy",
        ),
        "Name:Test Mortician\nManaCost:B\nTypes:Enchantment\n\
         T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Creature.YouOwn | TriggerZones$ Battlefield | Execute$ TrigTop | TriggerDescription$ Text.\n\
         SVar:TrigTop:DB$ ChangeZone | Origin$ Graveyard | Destination$ Library | LibraryPosition$ 0 | Defined$ TriggeredNewCardLKICopy\nOracle:Text."
            .to_string(),
        spell(
            "Test Grave Purge",
            "B",
            "Instant",
            "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Exile | ValidTgts$ Card | SpellDescription$ Text.",
        ),
        SMITE.to_string(),
        SEIZURE.to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let settle = |s: &mut Script| {
        drive_until(s, |s| {
            s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
        })
    };
    // The Hound dies and comes back to its owner's hand.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
    sc.card(P1, Zone::Battlefield, "Swamp");
    sc.card(P0, Zone::Battlefield, "Test Loyal Hound");
    sc.cards(P0, Zone::Hand, &["Test Smiting", "Test Smiting"]);
    sc.card(P1, Zone::Hand, "Test Grave Purge");
    let mut s = Script::new(sc.build());
    s.cast("Test Smiting").target("Test Loyal Hound").pay();
    settle(&mut s);
    assert!(s.find(P0, Zone::Hand, "Test Loyal Hound").is_some());
    // Again — but in response to the trigger the card is exiled from the graveyard: the
    // ability doesn't follow it there.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    sc.card(P1, Zone::Battlefield, "Swamp");
    sc.card(P0, Zone::Battlefield, "Test Loyal Hound");
    sc.card(P0, Zone::Hand, "Test Smiting");
    sc.card(P1, Zone::Hand, "Test Grave Purge");
    let mut s = Script::new(sc.build());
    s.cast("Test Smiting")
        .target("Test Loyal Hound")
        .pay()
        .pass_all();
    // The trigger is on the stack; the opponent responds.
    drive_until(&mut s, |s| {
        s.who() == P1 && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(s.st.stack.len(), 1);
    s.cast("Test Grave Purge").target("Test Loyal Hound").pay();
    settle(&mut s);
    assert!(s.find(P0, Zone::Exile, "Test Loyal Hound").is_some());
    assert!(s.find(P0, Zone::Hand, "Test Loyal Hound").is_none());

    // "A creature you own": my Elf under the opponent's control dies — into my graveyard,
    // and the Mortician puts that card on top of my library. Their Golem under my control
    // is not mine.
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp", "Test Mortician"]);
    let elf = sc.card(P0, Zone::Battlefield, "Test Elf");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Seizure");
    sc.attach(theirs, elf);
    let mine = sc.card(P0, Zone::Battlefield, "Test Seizure");
    sc.attach(mine, golem);
    sc.cards(P0, Zone::Hand, &["Test Smiting", "Test Smiting"]);
    let mut s = Script::new(sc.build());
    assert_eq!(chars(&s.st, elf).controller, P1);
    assert_eq!(chars(&s.st, golem).controller, P0);
    s.cast("Test Smiting").target_object(golem).pay();
    settle(&mut s);
    assert!(s.find(P1, Zone::Graveyard, "Test Golem").is_some());
    s.cast("Test Smiting").target_object(elf).pay();
    settle(&mut s);
    assert!(s.find(P0, Zone::Graveyard, "Test Elf").is_none());
    let top = *s.st.zones.list(Zone::Library, P0).last().unwrap();
    assert_eq!(s.name(top), "Test Elf");
}

// CR 608.2n (2026-11-06). Forge: `DB$ ChangeZone | Origin$ Stack | Destination$ Exile` and
// `… Destination$ Library | Shuffle$ True | Defined$ Parent` on a spell: where the spell
// goes as it finishes resolving. A countered spell goes to the graveyard as always.
#[test]
fn spells_that_go_somewhere_else_when_they_resolve() {
    let cards = [
        spell(
            "Test Insight",
            "U",
            "Sorcery",
            "A:SP$ Draw | NumCards$ 1 | SubAbility$ DBExile | SpellDescription$ Text.\n\
             SVar:DBExile:DB$ ChangeZone | Origin$ Stack | Destination$ Exile",
        ),
        spell(
            "Test Beacon",
            "U",
            "Sorcery",
            "A:SP$ GainLife | LifeAmount$ 3 | SubAbility$ DBShuffle | SpellDescription$ Text.\n\
             SVar:DBShuffle:DB$ ChangeZone | Origin$ Stack | Destination$ Library | Shuffle$ True | Defined$ Parent",
        ),
        spell(
            "Test Negation",
            "U",
            "Instant",
            "A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island"; 3]);
    sc.card(P1, Zone::Battlefield, "Island");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Insight", "Test Beacon", "Test Insight"],
    );
    sc.card(P1, Zone::Hand, "Test Negation");
    let mut s = Script::new(sc.build());
    s.cast("Test Insight").pay().pass_all();
    assert!(s.find(P0, Zone::Exile, "Test Insight").is_some());
    assert!(s.find(P0, Zone::Graveyard, "Test Insight").is_none());
    let (library, shuffles) = (
        library_size(&s, P0),
        s.count(|e| matches!(e, Event::Shuffled { .. })),
    );
    s.cast("Test Beacon").pay().pass_all();
    assert_eq!(life(&s, P0), 43);
    assert_eq!(library_size(&s, P0), library + 1);
    assert_eq!(
        s.count(|e| matches!(e, Event::Shuffled { .. })),
        shuffles + 1
    );
    // Countered: it never resolves, so it goes where countered spells go.
    s.cast("Test Insight").pay().pass();
    s.cast("Test Negation")
        .target("Test Insight")
        .pay()
        .pass_all();
    assert!(s.find(P0, Zone::Graveyard, "Test Insight").is_some());
}

// Forge: `ChangeZone | Origin$ Graveyard | Destination$ Library` with a target — "put
// target card from your graveyard on top of your library" (CR 401.4 for one card: no
// order to choose).
#[test]
fn a_card_from_a_graveyard_to_a_library() {
    let cards = [spell(
        "Test Reclaiming",
        "G",
        "Sorcery",
        "A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Library | LibraryPosition$ 0 | ValidTgts$ Card.YouOwn | SpellDescription$ Text.",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Graveyard, "Test Elf");
    sc.card(P1, Zone::Graveyard, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Reclaiming");
    let mut s = Script::new(sc.build());
    s.cast("Test Reclaiming");
    // Only my own graveyard's card: with one legal target it is chosen at once.
    if matches!(s.kind(), ChoiceKind::ChooseTargets { .. }) {
        assert_eq!(target_names(&s), ["Test Elf"]);
        s.target("Test Elf");
    }
    s.pay().pass_all();
    let top = *s.st.zones.list(Zone::Library, P0).last().unwrap();
    assert_eq!(s.name(top), "Test Elf");
}

// ---------------------------------------------------------------------------------------
// Keywords that are shorthand for abilities the engine has (D-060).

// CR 702.28b, 702.31b (2026-11-06). Forge: `K:Shadow`, `K:Horsemanship`.
#[test]
fn shadow_and_horsemanship() {
    let cards = [
        creature("Test Shade", "B", "Shade", "1/1", "K:Shadow"),
        creature("Test Rider", "W", "Soldier", "1/1", "K:Horsemanship"),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let blockers_of = |attacker: &str| {
        let mut sc = scenario(&cards);
        let a = sc.card(P0, Zone::Battlefield, attacker);
        sc.cards(
            P1,
            Zone::Battlefield,
            &["Test Shade", "Test Rider", "Test Golem"],
        );
        let mut s = Script::new(sc.build());
        attack_with(&mut s, &[a]);
        offered_blockers(&mut s, a)
    };
    // Shadow is blocked only by shadow, and blocks only shadow.
    assert_eq!(blockers_of("Test Shade"), ["Test Shade"]);
    // Horsemanship is blocked only by horsemanship, but blocks anything (without shadow).
    assert_eq!(blockers_of("Test Rider"), ["Test Rider"]);
    assert_eq!(blockers_of("Test Golem"), ["Test Rider", "Test Golem"]);
}

// CR 702.108a, 702.83a–b, 506.5 (2026-11-06). Forge: `K:Prowess`, `K:Exalted`, and
// `Mode$ Attacks | Alone$ True`.
#[test]
fn prowess_and_exalted() {
    let cards = [
        creature("Test Monk", "U", "Monk", "1/1", "K:Prowess"),
        creature("Test Squire", "W", "Soldier", "1/1", "K:Exalted"),
        bell(
            "Test Lone Bell",
            "Attacks | ValidCard$ Creature.YouCtrl | Alone$ True",
            5,
        ),
        spell(
            "Test Pondering",
            "U",
            "Instant",
            "A:SP$ Draw | NumCards$ 1 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island", "Forest"]);
    let monk = sc.card(P0, Zone::Battlefield, "Test Monk");
    sc.cards(P0, Zone::Hand, &["Test Pondering", "Test Elf"]);
    let mut s = Script::new(sc.build());
    // A creature spell: nothing. A noncreature spell: +1/+1 until end of turn.
    s.cast("Test Elf").pay().pass_all();
    assert_eq!(pt(&s, monk), (1, 1));
    s.cast("Test Pondering").pay();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(pt(&s, monk), (2, 2));
    drive_until(&mut s, |s| at(s, P1, Step::Upkeep));
    assert_eq!(pt(&s, monk), (1, 1));

    // Exalted: the one attacker gets +1/+1 for each instance; two attackers get nothing.
    let build = || {
        let mut sc = scenario(&cards);
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Test Squire", "Test Squire", "Test Lone Bell"],
        );
        let golems = sc.cards(P0, Zone::Battlefield, &["Test Golem", "Test Golem"]);
        (Script::new(sc.build()), golems)
    };
    let (mut s, golems) = build();
    attack_with(&mut s, &[golems[0]]);
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() != ChoiceKind::OrderTriggers
    });
    assert_eq!(pt(&s, golems[0]), (4, 5));
    assert_eq!(pt(&s, golems[1]), (2, 3));
    assert_eq!(life(&s, P0), 45);
    let (mut s, golems) = build();
    attack_with(&mut s, &golems);
    drive_until(&mut s, |s| at(s, P0, Step::PostcombatMain));
    assert_eq!(life(&s, P0), 40);
    assert_eq!(life(&s, P1), 36, "two unpumped 2/3s");
}

// CR 702.114a, 702.41a (2026-11-06). Forge: `K:Devoid`, `K:Affinity:Artifact`.
#[test]
fn devoid_and_affinity() {
    let cards = [
        creature("Test Voidling", "1 R", "Eldrazi", "2/1", "K:Devoid"),
        creature(
            "Test Scrapling",
            "4",
            "Construct",
            "2/2",
            "K:Affinity:Artifact",
        ),
        "Name:Test Trinket\nManaCost:1\nTypes:Artifact\nOracle:Text.".to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    let on_table = sc.card(P0, Zone::Battlefield, "Test Voidling");
    let in_hand = sc.card(P0, Zone::Hand, "Test Voidling");
    // Two artifacts I control; the opponent's doesn't count.
    sc.cards(P0, Zone::Battlefield, &["Test Trinket", "Test Trinket"]);
    sc.card(P1, Zone::Battlefield, "Test Trinket");
    sc.card(P0, Zone::Hand, "Test Scrapling");
    let mut s = Script::new(sc.build());
    // Colorless everywhere, though its mana cost is red — which is still its identity.
    assert!(chars(&s.st, on_table).colors.is_empty());
    assert!(chars(&s.st, in_hand).colors.is_empty());
    let def = s.st.catalog.get(s.st.object(in_hand).card);
    assert_eq!(card_model::color_identity(def), [card_model::Color::Red]);
    // {4} less {1} for each of my two artifacts: two lands pay for it.
    assert!(s.can_cast("Test Scrapling"));
    s.cast("Test Scrapling").pay().pass_all();
    assert!(s.find(P0, Zone::Battlefield, "Test Scrapling").is_some());
}

// ---------------------------------------------------------------------------------------
// More events: attacking, blocking, life, draws, discards, sacrifices (D-061).

// CR 508.1m (2026-11-06). Forge: `Mode$ AttackersDeclared | AttackingPlayer$ You` with
// `ValidAttackersAmount$ GE2` — "whenever you attack [with two or more creatures]": once
// for the declaration.
#[test]
fn whenever_you_attack() {
    let cards = [
        bell(
            "Test Muster Bell",
            "AttackersDeclared | AttackingPlayer$ You",
            1,
        ),
        bell(
            "Test Horde Bell",
            "AttackersDeclared | AttackingPlayer$ You | ValidAttackers$ Creature | ValidAttackersAmount$ GE2",
            10,
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = || {
        let mut sc = scenario(&cards);
        sc.cards(
            P0,
            Zone::Battlefield,
            &["Test Muster Bell", "Test Horde Bell"],
        );
        let mine = sc.cards(P0, Zone::Battlefield, &["Test Golem", "Test Golem"]);
        let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
        (Script::new(sc.build()), mine, theirs)
    };
    let (mut s, mine, _) = build();
    attack_with(&mut s, &mine[..1]);
    drive_until(&mut s, |s| at(s, P0, Step::PostcombatMain));
    assert_eq!(life(&s, P0), 41);
    let (mut s, mine, theirs) = build();
    attack_with(&mut s, &mine);
    drive_until(&mut s, |s| at(s, P0, Step::PostcombatMain));
    assert_eq!(life(&s, P0), 51, "once, not once for each attacker");
    // The opponent attacking isn't "you attack"; and with nobody attacking nothing happens.
    drive_until(&mut s, |s| at(s, P1, Step::PrecombatMain));
    attack_with(&mut s, &[theirs]);
    drive_until(&mut s, |s| at(s, P1, Step::PostcombatMain));
    assert_eq!(life(&s, P0), 51 - 2);
}

// CR 119.9, 121.1, 701.9, 701.21 (2026-11-06). Forge: `LifeGained`, `Drawn` (with
// `Number$ 2`: the second card of a turn), `Discarded`, `Sacrificed`.
#[test]
fn life_draw_discard_and_sacrifice_triggers() {
    let cards = [
        format!(
            "{}\nSVar:X:TriggerCount$LifeAmount\nOracle:Text.",
            "Name:Test Sanguine Bell\nManaCost:1\nTypes:Enchantment\n\
             T:Mode$ LifeGained | ValidPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigDrain | TriggerDescription$ Text.\n\
             SVar:TrigDrain:DB$ LoseLife | Defined$ Player.Opponent | LifeAmount$ X"
        ),
        bell("Test Second Bell", "Drawn | ValidCard$ Card.YouCtrl | Number$ 2", 10),
        bell("Test Draw Bell", "Drawn | ValidCard$ Card.YouCtrl", 1),
        bell("Test Loss Bell", "Discarded | ValidCard$ Card.YouCtrl", 100),
        bell(
            "Test Scrap Bell",
            "Sacrificed | ValidCard$ Artifact | ValidPlayer$ You",
            1000,
        ),
        spell(
            "Test Healing",
            "W",
            "Instant",
            "A:SP$ GainLife | LifeAmount$ 3 | SpellDescription$ Text.",
        ),
        spell(
            "Test Divination",
            "U",
            "Instant",
            "A:SP$ Draw | NumCards$ 3 | SpellDescription$ Text.",
        ),
        creature("Test Dune Beast", "5 R", "Beast", "4/4", "K:Cycling:1"),
        "Name:Test Bauble\nManaCost:1\nTypes:Artifact\n\
         A:AB$ Mill | Cost$ Sac<1/CARDNAME> | Defined$ Player.Opponent | NumCards$ 1 | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
        creature(
            "Test Votary",
            "W",
            "Cleric",
            "1/1",
            "A:AB$ Mill | Cost$ Sac<1/CARDNAME> | Defined$ Player.Opponent | NumCards$ 1 | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let settle = |s: &mut Script| {
        drive_until(s, |s| {
            s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
        })
    };
    // "Whenever you gain life, each opponent loses that much life."
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Test Sanguine Bell"]);
    sc.card(P0, Zone::Hand, "Test Healing");
    let mut s = Script::new(sc.build());
    s.cast("Test Healing").pay();
    settle(&mut s);
    assert_eq!((life(&s, P0), life(&s, P1)), (43, 37));

    // Draws: +1 for each card, +10 for the second card of the turn only.
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &[
            "Island",
            "Island",
            "Test Second Bell",
            "Test Draw Bell",
            "Test Loss Bell",
        ],
    );
    sc.cards(P0, Zone::Hand, &["Test Divination", "Test Dune Beast"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Divination").pay();
    settle(&mut s);
    assert_eq!(life(&s, P0), 40 + 3 + 10);
    // Cycling: a discard (+100), and the fourth card of the turn (+1).
    s.activate("Test Dune Beast").pay();
    settle(&mut s);
    assert_eq!(life(&s, P0), 53 + 100 + 1);

    // Sacrificing an artifact, not a creature.
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Scrap Bell", "Test Bauble", "Test Votary"],
    );
    let mut s = Script::new(sc.build());
    s.activate("Test Votary");
    settle(&mut s);
    assert_eq!(life(&s, P0), 40);
    s.activate("Test Bauble");
    settle(&mut s);
    assert_eq!(life(&s, P0), 1040);
}

// CR 509.3a–d, 509.3g, 702.45a, 702.25a (2026-11-06). Forge: `Blocks`, `AttackerBlocked`,
// `AttackerBlockedByCreature`, `AttackerUnblocked`; `K:Bushido:1`, `K:Flanking`.
#[test]
fn blocking_triggers() {
    let gains = |name: &str, pt: &str, trigger: &str, n: u32| {
        creature(
            name,
            "2",
            "Soldier",
            pt,
            &format!(
                "T:Mode$ {trigger} | Execute$ TrigGain | TriggerDescription$ Text.\n\
                 SVar:TrigGain:DB$ GainLife | LifeAmount$ {n}"
            ),
        )
    };
    let cards = [
        gains("Test Gatewarden", "0/5", "Blocks | ValidCard$ Card.Self", 3),
        gains(
            "Test Brawler",
            "1/7",
            "AttackerBlocked | ValidCard$ Card.Self",
            10,
        ),
        gains(
            "Test Sneak",
            "1/1",
            "AttackerUnblocked | ValidCard$ Card.Self",
            100,
        ),
        creature(
            "Test Needler",
            "2",
            "Insect",
            "1/7",
            "T:Mode$ AttackerBlockedByCreature | ValidCard$ Card.Self | ValidBlocker$ Creature | Execute$ TrigDmg | TriggerDescription$ Text.\n\
             SVar:TrigDmg:DB$ DealDamage | Defined$ TriggeredBlockerLKICopy | NumDmg$ 1",
        ),
        creature("Test Samurai", "2", "Samurai", "2/2", "K:Bushido:1"),
        creature("Test Lancer", "2", "Knight", "2/2", "K:Flanking"),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    // Blocked by two creatures: "becomes blocked" once; "blocks" once for the Sentinel.
    let mut sc = scenario(&cards);
    let brawler = sc.card(P0, Zone::Battlefield, "Test Brawler");
    let sneak = sc.card(P0, Zone::Battlefield, "Test Sneak");
    let sentinel = sc.card(P1, Zone::Battlefield, "Test Gatewarden");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[brawler, sneak]);
    block_with(&mut s, &[(sentinel, brawler), (golem, brawler)]);
    fight(&mut s);
    // Mine: becomes blocked (+10), the Sneak isn't blocked (+100). Theirs: blocks (+3),
    // less the Sneak's 1 damage.
    assert_eq!((life(&s, P0), life(&s, P1)), (150, 40 + 3 - 1));

    // "Becomes blocked by a creature": once for each blocker, and it is that creature.
    let mut sc = scenario(&cards);
    let stinger = sc.card(P0, Zone::Battlefield, "Test Needler");
    let elves = sc.cards(P1, Zone::Battlefield, &["Test Elf", "Test Elf"]);
    let idle = sc.card(P1, Zone::Battlefield, "Test Elf");
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[stinger]);
    block_with(&mut s, &[(elves[0], stinger), (elves[1], stinger)]);
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() != ChoiceKind::OrderTriggers
    });
    assert!(!on_battlefield(&s, elves[0]) && !on_battlefield(&s, elves[1]));
    assert!(on_battlefield(&s, idle));

    // Bushido: +1/+1 when it blocks or becomes blocked. Flanking: a blocker without
    // flanking gets -1/-1 (the 1/1 Elf dies), one with flanking doesn't.
    let mut sc = scenario(&cards);
    let samurai = sc.card(P0, Zone::Battlefield, "Test Samurai");
    let lancer = sc.card(P0, Zone::Battlefield, "Test Lancer");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    let elf = sc.card(P1, Zone::Battlefield, "Test Elf");
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[samurai, lancer]);
    block_with(&mut s, &[(golem, samurai), (elf, lancer)]);
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() != ChoiceKind::OrderTriggers
    });
    assert_eq!(pt(&s, samurai), (3, 3));
    assert!(!on_battlefield(&s, elf));
    let mut sc = scenario(&cards);
    let lancer = sc.card(P0, Zone::Battlefield, "Test Lancer");
    let other = sc.card(P1, Zone::Battlefield, "Test Lancer");
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[lancer]);
    block_with(&mut s, &[(other, lancer)]);
    drive_until(&mut s, |s| s.kind() == ChoiceKind::Priority);
    assert!(s.st.stack.is_empty());
    assert_eq!(pt(&s, other), (2, 2));
    // Granting flanking alone would not give the ability: it is not a keyword a script
    // can hand out.
    let b = forge_test_blockers(&creature(
        "Test Banneret",
        "2",
        "Knight",
        "2/2",
        "S:Mode$ Continuous | Affected$ Creature.Other+YouCtrl | AddKeyword$ Flanking | Description$ Text.",
    ));
    assert_eq!(b, ["kw:Flanking"]);
}

// ---------------------------------------------------------------------------------------
// Animate: types, colors, keywords and base power/toughness for a turn or for good (D-062).

// CR 613.1d–f, 613.4b, 611.2a–b (2026-11-06). Forge: `Animate` with `Types$`, `Colors$` +
// `OverwriteColors$`, `Keywords$`, `Power$`/`Toughness$`, `RemoveAllAbilities$`, and
// `Duration$ Permanent`; `AnimateAll`.
#[test]
fn becoming_a_creature_for_a_turn_or_for_good() {
    let cards = [
        "Name:Test Thicket\nManaCost:no cost\nTypes:Land\n\
         A:AB$ Mana | Cost$ T | Produced$ G | SpellDescription$ Text.\n\
         A:AB$ Animate | Cost$ 1 | Defined$ Self | Power$ 3 | Toughness$ 3 | Types$ Creature,Ape | Colors$ Green | OverwriteColors$ True | Keywords$ Trample | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
        "Name:Test Idol\nManaCost:2\nTypes:Artifact\n\
         A:AB$ Animate | Cost$ 2 | Defined$ Self | Power$ 4 | Toughness$ 4 | Types$ Creature,Ape | Duration$ Permanent | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
        spell(
            "Test Humbling",
            "W",
            "Instant",
            "A:SP$ Animate | ValidTgts$ Creature | Power$ 0 | Toughness$ 1 | RemoveAllAbilities$ True | SpellDescription$ Text.",
        ),
        spell(
            "Test Flatlining",
            "B",
            "Instant",
            "A:SP$ AnimateAll | ValidCards$ Creature.OppCtrl | Power$ 0 | Toughness$ 1 | SpellDescription$ Text.",
        ),
        creature("Test Wind Drake", "U", "Drake", "2/2", "K:Flying"),
        creature("Test Ape Lord", "G", "Ape", "1/1", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Forest", "Forest", "Plains", "Swamp"],
    );
    let thicket = sc.card(P0, Zone::Battlefield, "Test Thicket");
    let idol = sc.card(P0, Zone::Battlefield, "Test Idol");
    let drake = sc.card(P1, Zone::Battlefield, "Test Wind Drake");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Humbling", "Test Flatlining"]);
    let mut s = Script::new(sc.build());
    use card_model::{CardType, Color};
    let click = |s: &mut Script, id: ObjectId| {
        s.pick(
            "activate",
            |_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == id),
        );
    };
    // The land becomes a 3/3 green Ape creature with trample — and is still a land.
    click(&mut s, thicket);
    s.pay().pass_all();
    let c = chars(&s.st, thicket);
    assert!(c.is(CardType::Creature) && c.is(CardType::Land) && c.has_subtype("Ape"));
    assert_eq!(
        (c.colors.clone(), pt(&s, thicket)),
        (vec![Color::Green], (3, 3))
    );
    assert!(c.has(Keyword::Trample));
    // The artifact becomes a 4/4 Ape creature for good.
    click(&mut s, idol);
    s.pay().pass_all();
    assert_eq!(pt(&s, idol), (4, 4));
    // "Loses all abilities and has base power and toughness 0/1" for the Drake; then every
    // creature the opponent controls is 0/1 (the Golem too), mine aren't.
    s.cast("Test Humbling")
        .target_object(drake)
        .pay()
        .pass_all();
    assert_eq!(pt(&s, drake), (0, 1));
    assert!(!chars(&s.st, drake).has(Keyword::Flying));
    s.cast("Test Flatlining").pay().pass_all();
    assert_eq!((pt(&s, golem), pt(&s, idol)), ((0, 1), (4, 4)));
    // Next turn: the land is only a land again, the Idol is still a creature, the
    // opponent's creatures are themselves.
    drive_until(&mut s, |s| at(s, P1, Step::Upkeep));
    let c = chars(&s.st, thicket);
    assert!(!c.is(CardType::Creature) && c.is(CardType::Land) && c.power.is_none());
    assert!(c.colors.is_empty());
    assert_eq!(pt(&s, idol), (4, 4));
    assert_eq!((pt(&s, golem), pt(&s, drake)), ((2, 3), (2, 2)));
    assert!(chars(&s.st, drake).has(Keyword::Flying));

    // What isn't understood isn't compiled: types "instead of" the others, colors "in
    // addition", a duration tied to something else.
    for (script, blocker) in [
        (
            "A:SP$ Animate | ValidTgts$ Creature | Types$ Ape | RemoveCreatureTypes$ True | SpellDescription$ Text.",
            "param:Animate.RemoveCreatureTypes",
        ),
        (
            "A:SP$ Animate | ValidTgts$ Creature | Colors$ Green | SpellDescription$ Text.",
            "Animate:colors added",
        ),
        (
            "A:SP$ Animate | ValidTgts$ Creature | Types$ Ape | Duration$ UntilHostLeavesPlay | SpellDescription$ Text.",
            "Animate:duration UntilHostLeavesPlay",
        ),
    ] {
        // (With a card that makes "Ape" a known subtype.)
        let lib = card_db::forge::Library::build(
            &[
                (
                    "a".to_string(),
                    spell("Test Oddment", "G", "Instant", script),
                ),
                (
                    "b".to_string(),
                    creature("Test Ape Lord", "G", "Ape", "1/1", ""),
                ),
            ],
            &[],
            None,
        );
        assert_eq!(
            lib.blocked
                .get("Test Oddment")
                .map(|b| b.iter().cloned().collect::<Vec<_>>()),
            Some(vec![blocker.to_string()]),
            "{script}"
        );
    }
}

// CR 613.1d–f, 613.4b (2026-11-06). Forge: `S:Mode$ Continuous` with `AddType$`, `SetPower$`
// / `SetToughness$`, `SetColor$`, `RemoveKeyword$`, `RemoveAllAbilities$`; an Equipment's
// `CantBlockBy` about the equipped creature.
#[test]
fn statics_that_change_types_colors_and_base_power() {
    let cards = [
        "Name:Test Emergence\nManaCost:G\nTypes:Enchantment\n\
         S:Mode$ Continuous | Affected$ Land.YouCtrl | SetPower$ 2 | SetToughness$ 2 | AddType$ Creature | AddKeyword$ First Strike | Description$ Text.\nOracle:Text."
            .to_string(),
        "Name:Test Humbler\nManaCost:W\nTypes:Enchantment\n\
         S:Mode$ Continuous | Affected$ Creature.OppCtrl+nonLand | RemoveAllAbilities$ True | SetPower$ 1 | SetToughness$ 1 | Description$ Text.\nOracle:Text."
            .to_string(),
        "Name:Test Leaden Boots\nManaCost:B\nTypes:Enchantment Aura\nK:Enchant:Creature\n\
         S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | RemoveKeyword$ Flying | SetColor$ Black | Description$ Text.\nOracle:Text."
            .to_string(),
        "Name:Test Silk Cloak\nManaCost:3\nTypes:Artifact Equipment\nK:Equip:2\n\
         S:Mode$ CantBlockBy | ValidAttacker$ Creature.EquippedBy | Description$ Text.\nOracle:Text."
            .to_string(),
        creature("Test Wind Drake", "U", "Drake", "2/2", "K:Flying"),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    use card_model::{CardType, Color};
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Test Emergence", "Test Humbler"]);
    let forest = sc.card(P0, Zone::Battlefield, "Forest");
    let their_forest = sc.card(P1, Zone::Battlefield, "Forest");
    let mine = sc.card(P0, Zone::Battlefield, "Test Wind Drake");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Wind Drake");
    let boots = sc.card(P0, Zone::Battlefield, "Test Leaden Boots");
    sc.attach(boots, mine);
    let cloak = sc.card(P0, Zone::Battlefield, "Test Silk Cloak");
    sc.attach(cloak, forest);
    let s = Script::new(sc.build());
    // My lands are 2/2 creatures with first strike, still lands; the opponent's aren't.
    let c = chars(&s.st, forest);
    assert!(c.is(CardType::Creature) && c.is(CardType::Land) && c.has(Keyword::FirstStrike));
    assert_eq!(pt(&s, forest), (2, 2));
    assert!(!chars(&s.st, their_forest).is(CardType::Creature));
    // Their creatures lose all abilities and are 1/1; mine keeps its size, but the Boots
    // take its flying and make it black.
    let c = chars(&s.st, theirs);
    assert!(!c.has(Keyword::Flying));
    assert_eq!(pt(&s, theirs), (1, 1));
    let c = chars(&s.st, mine);
    assert!(!c.has(Keyword::Flying));
    assert_eq!(
        (c.colors.clone(), pt(&s, mine)),
        (vec![Color::Black], (2, 2))
    );
    // The equipped creature (the animated Forest) can't be blocked; the Drake can.
    let mut s = s;
    attack_with(&mut s, &[forest, mine]);
    assert!(offered_blockers(&mut s, forest).is_empty());
}

// ---------------------------------------------------------------------------------------
// Costs paid while a spell or ability resolves (D-063).
// Text: docs/rules-notes/601-casting-602-activating-605-mana.md (last section).

fn yes(s: &mut Script) {
    assert_eq!(s.kind(), ChoiceKind::PayUnlessCost);
    s.pick("pay", |_, o| matches!(o, OptionDesc::Accept));
}

fn no(s: &mut Script) {
    assert_eq!(s.kind(), ChoiceKind::PayUnlessCost);
    s.pick("don't pay", |_, o| matches!(o, OptionDesc::Decline));
}

// CR 118.12, 118.3, 701.6a (2026-11-06). Forge: `Counter | UnlessCost$ 2` — "counter target
// spell unless its controller pays {2}".
#[test]
fn counter_target_spell_unless_its_controller_pays() {
    let cards = [spell(
        "Test Tithing",
        "U",
        "Instant",
        "A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | UnlessCost$ 2 | SpellDescription$ Text.",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = |lands: usize| {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &vec!["Forest"; lands]);
        sc.card(P1, Zone::Battlefield, "Island");
        sc.card(P0, Zone::Hand, "Test Golem"); // {3}
        sc.card(P1, Zone::Hand, "Test Tithing");
        let mut s = Script::new(sc.build());
        s.cast("Test Golem").pay().pass();
        s.cast("Test Tithing").target("Test Golem").pay().pass_all();
        s
    };
    // Two lands left: the caster is asked, pays {2} with them, and the spell resolves.
    let mut s = build(5);
    assert_eq!(s.who(), P0);
    yes(&mut s);
    s.pay();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert!(s.find(P0, Zone::Battlefield, "Test Golem").is_some());
    let untapped = |s: &Script| {
        s.st.zones
            .battlefield
            .iter()
            .filter(|id| s.name(**id) == "Forest" && !s.st.object(**id).tapped)
            .count()
    };
    assert_eq!(untapped(&s), 0);
    // They may decline; then it is countered.
    let mut s = build(5);
    no(&mut s);
    assert!(s.find(P0, Zone::Graveyard, "Test Golem").is_some());
    assert_eq!(untapped(&s), 2);
    // One land left: the cost can't be paid, so nobody is asked.
    let s = build(4);
    assert_eq!(s.kind(), ChoiceKind::Priority);
    assert!(s.find(P0, Zone::Graveyard, "Test Golem").is_some());
}

// CR 118.12 (2026-11-06). Forge: a trigger that runs an `AB$` with a `Cost$` — "you may
// pay [cost]. If you do, …" — and `UnlessCost$` with `UnlessPayer$ You` — "sacrifice this
// unless you pay …".
#[test]
fn paying_as_an_ability_resolves() {
    let cards = [
        creature(
            "Test Scribe",
            "W",
            "Cleric",
            "1/1",
            "T:Mode$ SpellCast | ValidCard$ Card | ValidActivatingPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:AB$ GainLife | Cost$ 1 | LifeAmount$ 3",
        ),
        creature(
            "Test Ogre",
            "R",
            "Ogre",
            "3/3",
            "T:Mode$ Attacks | ValidCard$ Card.Self | Execute$ TrigPump | TriggerDescription$ Text.\n\
             SVar:TrigPump:AB$ Pump | Cost$ Sac<1/Creature.Other/another creature> | Defined$ Self | NumAtt$ +2 | NumDef$ +2",
        ),
        creature(
            "Test Efreet",
            "B",
            "Efreet",
            "3/3",
            "T:Mode$ Phase | Phase$ Upkeep | ValidPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigSac | TriggerDescription$ Text.\n\
             SVar:TrigSac:DB$ Sacrifice | UnlessPayer$ You | UnlessCost$ B B",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    // "Whenever you cast a spell, you may pay {1}. If you do, you gain 3 life."
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Forest", "Forest", "Test Scribe"],
    );
    sc.cards(P0, Zone::Hand, &["Test Elf", "Test Elf", "Test Elf"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Elf").pay().pass_all();
    assert_eq!(s.who(), P0);
    yes(&mut s);
    s.pay();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(life(&s, P0), 43);
    // The last Forest pays for the second Elf: nothing is left to pay with, nothing is
    // asked, nothing is gained.
    s.cast("Test Elf").pay();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(life(&s, P0), 43);
    assert_eq!(s.count(|e| matches!(e, Event::Triggered { .. })), 2);

    // "Whenever this attacks, you may sacrifice another creature. If you do, it gets
    // +2/+2": the sacrifice is chosen as the cost is paid.
    let mut sc = scenario(&cards);
    let ogre = sc.card(P0, Zone::Battlefield, "Test Ogre");
    let elf = sc.card(P0, Zone::Battlefield, "Test Elf");
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[ogre]);
    drive_until(&mut s, |s| s.kind() != ChoiceKind::Priority);
    yes(&mut s);
    if s.kind() == ChoiceKind::Sacrifice {
        s.respond(Response::Pick(vec![0]));
    }
    assert!(!on_battlefield(&s, elf));
    assert_eq!(pt(&s, ogre), (5, 5));
    // Alone, it has nothing to sacrifice: no question.
    let mut sc = scenario(&cards);
    let ogre = sc.card(P0, Zone::Battlefield, "Test Ogre");
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[ogre]);
    drive_until(&mut s, |s| at(s, P0, Step::PostcombatMain));
    assert_eq!(life(&s, P1), 37);

    // "At the beginning of your upkeep, sacrifice this unless you pay {B}{B}."
    for pay in [true, false] {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
        let efreet = sc.card(P0, Zone::Battlefield, "Test Efreet");
        sc.turn(P1, Step::End);
        let mut s = Script::new(sc.build());
        drive_until(&mut s, |s| s.kind() == ChoiceKind::PayUnlessCost);
        assert!(at(&s, P0, Step::Upkeep));
        if pay {
            yes(&mut s);
            s.pay();
        } else {
            no(&mut s);
        }
        drive_until(&mut s, |s| at(s, P0, Step::Draw));
        assert_eq!(on_battlefield(&s, efreet), pay);
    }
}

// CR 608.2h (2026-11-06). Forge: `Defined$ TargetedController` — "its controller", who
// controlled the target as the spell began to resolve, though the target is gone by then.
#[test]
fn the_targets_controller() {
    let cards = [spell(
        "Test Despoiling",
        "B",
        "Sorcery",
        "A:SP$ Destroy | ValidTgts$ Creature | SubAbility$ DBLose | SpellDescription$ Text.\n\
         SVar:DBLose:DB$ LoseLife | Defined$ TargetedController | LifeAmount$ 2",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.card(P0, Zone::Battlefield, "Swamp");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Despoiling");
    let mut s = Script::new(sc.build());
    s.cast("Test Despoiling")
        .target_object(golem)
        .pay()
        .pass_all();
    assert!(!on_battlefield(&s, golem));
    assert_eq!((life(&s, P0), life(&s, P1)), (40, 38));
}

// CR 106.6, 118.3 (2026-11-06): mana that can be spent only on certain spells can't pay a
// cost paid on resolution, so a player with nothing else isn't asked to pay. (Found by
// the soak: the engine asked, the player accepted, and the payment had no way to finish.)
#[test]
fn restricted_mana_cant_pay_a_cost_on_resolution() {
    let cards = [
        creature(
            "Test Scribe",
            "W",
            "Cleric",
            "1/1",
            "T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Creature.Other+YouCtrl | TriggerZones$ Battlefield | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:AB$ GainLife | Cost$ 1 | LifeAmount$ 3",
        ),
        "Name:Test Cloister\nManaCost:no cost\nTypes:Land\n\
         A:AB$ Mana | Cost$ T | Produced$ W | RestrictValid$ Spell.Angel | SpellDescription$ Text.\nOracle:Text."
            .to_string(),
        creature("Test Seraph", "W", "Angel", "1/1", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = |spare: &str| {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Plains", spare, "Test Scribe"]);
        sc.card(P0, Zone::Hand, "Test Seraph");
        let mut s = Script::new(sc.build());
        // Pay for the Angel with the Plains.
        s.cast("Test Seraph");
        s.tap("Plains", engine_core::mana::ManaType::White);
        s.pay().pass_all().pass_all();
        s
    };
    // Only the Cloister is left: its mana couldn't pay {1} here, so there is no question.
    let s = build("Test Cloister");
    assert_eq!(s.kind(), ChoiceKind::Priority);
    assert_eq!(life(&s, P0), 40);
    // With a second Plains there is.
    let mut s = build("Plains");
    yes(&mut s);
    s.pay();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(life(&s, P0), 43);
}

// ---------------------------------------------------------------------------------------
// Abilities given to other objects (D-064).

fn given(s: &Script, id: ObjectId) -> usize {
    chars(&s.st, id)
        .abilities
        .iter()
        .filter(|a| a.granted)
        .count()
}

const PING: &str =
    "SVar:Ping:AB$ DealDamage | Cost$ T | ValidTgts$ Any | NumDmg$ 1 | SpellDescription$ Text.";

fn ember_gift() -> String {
    aura(
        "Test Ember Gift",
        "R",
        "Creature",
        &format!(
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | AddAbility$ Ping | Description$ Text.\n{PING}"
        ),
    )
}

// CR 113.1a, 113.2c, 113.10 (2026-11-06): an effect can give an object an ability; the
// object that has it is its source, and whoever controls that object may activate it.
// Forge: `AddAbility$ <SVar>` on a Continuous static — "Enchanted creature has "{T}: This
// creature deals 1 damage to any target."" The Aura itself has no such ability.
#[test]
fn an_aura_gives_the_enchanted_creature_an_activated_ability() {
    let cards = [
        ember_gift(),
        spell(
            "Test Unravel",
            "W",
            "Instant",
            "A:SP$ Destroy | ValidTgts$ Enchantment | SpellDescription$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Plains"]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let other = sc.card(P0, Zone::Battlefield, "Test Giant");
    sc.cards(P0, Zone::Hand, &["Test Ember Gift", "Test Unravel"]);
    let mut s = Script::new(sc.build());
    assert!(!can_activate(&s, golem));
    s.cast("Test Ember Gift")
        .target_object(golem)
        .pay()
        .pass_all();
    let gift = s.find(P0, Zone::Battlefield, "Test Ember Gift").unwrap();
    assert_eq!(
        (given(&s, golem), given(&s, other), given(&s, gift)),
        (1, 0, 0)
    );
    assert!(can_activate(&s, golem) && !can_activate(&s, gift) && !can_activate(&s, other));
    // The Golem is the source: it taps for the cost, and it deals the damage.
    s.activate("Test Golem").target_player(P1).pass_all();
    assert!(s.st.object(golem).tapped);
    assert_eq!(life(&s, P1), 39);
    assert_eq!(
        s.count(|e| matches!(e, Event::DamageDealt { source, .. } if *source == golem)),
        1
    );
    // The Aura goes: so does the ability.
    s.cast("Test Unravel").target_object(gift).pay().pass_all();
    assert_eq!(given(&s, golem), 0);
}

// CR 113.2c (2026-11-06): "If an object has multiple instances of the same ability, each
// instance functions independently." Forge: `AddTrigger$ <SVar>` — "Insect creatures have
// "Whenever this creature attacks, you gain 1 life."" Two such lords: it triggers twice.
// `AddSVar$` copies script variables for Forge's use; a hint for its AI means nothing here.
#[test]
fn a_lord_gives_a_triggered_ability_and_two_lords_give_it_twice() {
    let singer = creature(
        "Test Hive Singer",
        "1 G",
        "Insect",
        "1/1",
        "S:Mode$ Continuous | Affected$ Creature.Insect | AddTrigger$ AttackTrig | AddSVar$ AE & TrigGain | Description$ Text.\n\
         SVar:AttackTrig:Mode$ Attacks | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ Text.\n\
         SVar:TrigGain:DB$ GainLife | LifeAmount$ 1\n\
         SVar:AE:SVar:HasAttackEffect:TRUE",
    );
    for lords in [1usize, 2] {
        let mut sc = scenario(&[&singer]);
        let singers = sc.cards(P0, Zone::Battlefield, &vec!["Test Hive Singer"; lords]);
        let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
        let mut s = Script::new(sc.build());
        assert_eq!(given(&s, singers[0]), lords);
        assert_eq!(given(&s, golem), 0, "not an Insect");
        attack_with(&mut s, &[singers[0], golem]);
        drive_until(&mut s, |s| s.st.turn.step == Step::PostcombatMain);
        assert_eq!(life(&s, P0), 40 + lords as i32);
    }
    // Under another name, a copied variable could mean something to what is given: only
    // the names Forge's AI reads are known to mean nothing.
    let odd = singer.replace("SVar:HasAttackEffect:TRUE", "SVar:X:Count$Valid Creature");
    assert_eq!(
        forge_test_blockers(&odd),
        ["static:AddSVar under another name"]
    );
}

// CR 603.10a (2026-11-06): a leaves-the-battlefield ability looks back in time — at the
// creature as it last existed, with the abilities it had been given. "Enchanted creature
// has "When this creature dies, you gain 3 life.""
#[test]
fn a_given_dies_trigger_is_found_on_the_creature_as_it_last_existed() {
    let cards = [
        aura(
            "Test Last Gift",
            "W",
            "Creature",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | AddTrigger$ DiesTrig | Description$ Text.\n\
             SVar:DiesTrig:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:DB$ GainLife | LifeAmount$ 3",
        ),
        SMITE.to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Swamp"]);
    // The opponent's creature: the ability is its controller's.
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Last Gift", "Test Smiting"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Last Gift")
        .target_object(golem)
        .pay()
        .pass_all();
    // (The spell resolves, then the trigger.)
    s.cast("Test Smiting")
        .target_object(golem)
        .pay()
        .pass_all()
        .pass_all();
    assert!(!on_battlefield(&s, golem));
    assert_eq!((life(&s, P0), life(&s, P1)), (40, 43));
}

// CR 605.1a, 106.12, 302.6 (2026-11-06). Forge: `AddAbility$` of an `AB$ Mana` — "Enchanted
// land has "{T}: Add two mana of any one color."" A Forest then has two ways to be tapped
// for {G}: the player says which. "Creatures you control have "{T}: Add {G}."": not one
// that came under your control this turn.
#[test]
fn given_mana_abilities() {
    use engine_core::mana::ManaType;
    let cards = [
        aura(
            "Test Bounty",
            "R",
            "Land",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | AddAbility$ TwoMana | Description$ Text.\n\
             SVar:TwoMana:AB$ Mana | Cost$ T | Produced$ Any | Amount$ 2 | SpellDescription$ Text.",
        ),
        creature(
            "Test Grove Tender",
            "1 G",
            "Druid",
            "1/1",
            "S:Mode$ Continuous | Affected$ Creature.YouCtrl | AddAbility$ GreenMana | Description$ Text.\n\
             SVar:GreenMana:AB$ Mana | Cost$ T | Produced$ G | SpellDescription$ Text.",
        ),
        creature("Test Sapling", "G", "Plant", "0/1", ""),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    // (The Mountain first: it is the first source offered for the Aura's {R}.)
    let lands = sc.cards(P0, Zone::Battlefield, &["Mountain", "Forest"]);
    let forest = lands[1];
    sc.card(P0, Zone::Hand, "Test Bounty");
    let mut s = Script::new(sc.build());
    s.cast("Test Bounty").target_object(forest).pay().pass_all();
    let ways = |s: &Script, mana: ManaType| -> Vec<bool> {
        s.options()
            .iter()
            .filter_map(|o| match o {
                OptionDesc::TapForMana {
                    source,
                    mana: m,
                    ability,
                } if *source == forest && *m == mana => Some(ability.is_some()),
                _ => None,
            })
            .collect()
    };
    // Its own "{T}: Add {G}" (CR 305.6) and the ability it was given.
    assert_eq!(ways(&s, ManaType::Green), [false, true]);
    assert_eq!(ways(&s, ManaType::Blue), [true]);
    s.pick("two green", |_, o| {
        matches!(o, OptionDesc::TapForMana { source, mana: ManaType::Green, ability: Some(_) }
            if *source == forest)
    });
    assert_eq!(mana_pool(&s, P0), 2);
    assert!(s.st.object(forest).tapped);

    let mut sc = scenario(&cards);
    let tender = sc.card(P0, Zone::Battlefield, "Test Grove Tender");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Test Sapling");
    let mut s = Script::new(sc.build());
    let taps = |s: &Script, id: ObjectId| {
        s.has(|_, o| matches!(o, OptionDesc::TapForMana { source, .. } if *source == id))
    };
    assert!(taps(&s, tender) && taps(&s, golem) && !taps(&s, theirs));
    // Paid for by a creature's mana.
    s.cast("Test Sapling").pay().pass_all();
    let sapling = s.find(P0, Zone::Battlefield, "Test Sapling").unwrap();
    assert_eq!(given(&s, sapling), 1);
    assert!(!taps(&s, sapling), "summoning sick (CR 302.6)");
}

// CR 113.3d, 613.1f, 613.7a (2026-11-06): a static ability an object is given works like
// any other — its effect starts applying in the layer after abilities are given, and
// "you" is the controller of the object that has it. Forge: `AddStaticAbility$ <SVar>` —
// "Enchanted creature has "Other creatures you control get +1/+1."" Given twice, it
// applies twice (CR 113.2c).
#[test]
fn a_given_static_ability_works_for_the_controller_of_what_has_it() {
    let cards = [aura(
        "Test Banner Gift",
        "W",
        "Creature",
        "S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | AddStaticAbility$ Anthem | Description$ Text.\n\
         SVar:Anthem:Mode$ Continuous | Affected$ Creature.Other+YouCtrl | AddPower$ 1 | AddToughness$ 1 | Description$ Text.",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Golem"); // 2/3
    let giant = sc.card(P1, Zone::Battlefield, "Test Giant"); // 5/4
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Test Banner Gift", "Test Banner Gift"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Banner Gift")
        .target_object(giant)
        .pay()
        .pass_all();
    assert_eq!(pt(&s, theirs), (3, 4), "the Giant's controller's creature");
    assert_eq!(pt(&s, giant), (5, 4), "other creatures");
    assert_eq!(pt(&s, mine), (2, 3), "not the Aura's controller's");
    s.cast("Test Banner Gift")
        .target_object(giant)
        .pay()
        .pass_all();
    assert_eq!(pt(&s, theirs), (4, 5));
}

// CR 613.1f, 613.7, 113.10c (2026-11-06): giving and removing abilities happen in one
// layer, in timestamp order — the later effect prevails. One ability that does both ("loses
// all other abilities and has …") takes away first.
#[test]
fn abilities_given_and_lost_in_timestamp_order() {
    let hush = aura(
        "Test Hush",
        "U",
        "Creature",
        "S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | RemoveAllAbilities$ True | Description$ Text.",
    );
    let rewrite = aura(
        "Test Rewrite",
        "U",
        "Creature",
        &format!(
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | RemoveAllAbilities$ True | AddKeyword$ Flying | AddAbility$ Ping | Description$ Text.\n{PING}"
        ),
    );
    let cards = [
        ember_gift(),
        hush,
        rewrite,
        creature("Test Lookout", "1 W", "Bird", "1/1", "K:Vigilance"),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let play = |order: &[&str]| {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Mountain", "Island"]);
        let bird = sc.card(P0, Zone::Battlefield, "Test Lookout");
        sc.cards(P0, Zone::Hand, order);
        let mut s = Script::new(sc.build());
        for name in order {
            s.cast(name).target_object(bird).pay().pass_all();
        }
        let c = chars(&s.st, bird);
        (c.abilities.len(), c.keywords.clone())
    };
    assert_eq!(play(&["Test Ember Gift"]), (1, vec![Keyword::Vigilance]));
    assert_eq!(play(&["Test Ember Gift", "Test Hush"]), (0, vec![]));
    assert_eq!(play(&["Test Hush", "Test Ember Gift"]), (1, vec![]));
    assert_eq!(play(&["Test Rewrite"]), (1, vec![Keyword::Flying]));
}

// CR 305.6 (2026-11-06): a land with a basic land type "has the intrinsic ability
// "{T}: Add [mana]"" — an ability it has, so one it loses with all its abilities.
#[test]
fn a_land_that_loses_all_abilities_loses_the_mana_ability_of_its_type() {
    let cards = [aura(
        "Test Fallow",
        "R",
        "Land",
        "S:Mode$ Continuous | AffectedDefined$ Enchanted | RemoveAllAbilities$ True | Description$ Text.",
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    let lands = sc.cards(P0, Zone::Battlefield, &["Mountain", "Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Fallow");
    let mut s = Script::new(sc.build());
    s.cast("Test Fallow")
        .target_object(lands[1])
        .pay()
        .pass_all();
    let taps = |s: &Script, id: ObjectId| {
        s.has(|_, o| matches!(o, OptionDesc::TapForMana { source, .. } if *source == id))
    };
    assert!(!s.st.object(lands[1]).tapped);
    assert!(!taps(&s, lands[1]) && taps(&s, lands[2]));
    assert!(
        chars(&s.st, lands[1]).has_subtype("Forest"),
        "still a Forest"
    );
}

// CR 611.2a–c, 514.2 (2026-11-06): a spell can give abilities for a while. Forge: Animate
// with `Abilities$`, `Triggers$`, `staticAbilities$` (comma-separated script variables) —
// "Until end of turn, target creature gains "{T}: …", "Whenever this creature attacks, …"
// and "This creature has haste.""
#[test]
fn a_spell_gives_abilities_until_end_of_turn() {
    let cards = [spell(
        "Test Kindle",
        "R",
        "Instant",
        &format!(
            "A:SP$ Animate | ValidTgts$ Creature | Abilities$ Ping | Triggers$ AttackTrig | staticAbilities$ Swift | SpellDescription$ Text.\n\
             {PING}\n\
             SVar:AttackTrig:Mode$ Attacks | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:DB$ GainLife | LifeAmount$ 2\n\
             SVar:Swift:Mode$ Continuous | Affected$ Card.Self | AddKeyword$ Haste | Description$ Text."
        ),
    )];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.card(P0, Zone::Battlefield, "Mountain");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let giant = sc.card(P0, Zone::Battlefield, "Test Giant");
    sc.card(P0, Zone::Hand, "Test Kindle");
    let mut s = Script::new(sc.build());
    s.cast("Test Kindle").target_object(golem).pay().pass_all();
    assert_eq!((given(&s, golem), given(&s, giant)), (3, 0));
    // The static ability it was given gives it haste in the same layer.
    assert!(has(&s, golem, Keyword::Haste));
    assert!(can_activate(&s, golem));
    attack_with(&mut s, &[golem]);
    drive_until(&mut s, |s| s.st.turn.step == Step::PostcombatMain);
    assert_eq!(life(&s, P0), 42);
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert_eq!(given(&s, golem), 0);
    assert!(!has(&s, golem, Keyword::Haste));
}

// CR 611.3c (2026-11-06): continuous effects apply to a permanent as it enters, "before
// determining whether the permanent will cause an ability to trigger when it enters". "As
// long as you control an artifact, this creature has "When this creature enters, you gain
// 4 life."" — it has that ability as it enters, or it doesn't.
#[test]
fn an_enters_trigger_the_permanent_is_given_as_it_enters() {
    let cards = [
        creature(
            "Test Reveler",
            "1 W",
            "Human",
            "2/2",
            "S:Mode$ Continuous | Affected$ Card.Self | AddTrigger$ Arrive | IsPresent$ Artifact.YouCtrl | Description$ Text.\n\
             SVar:Arrive:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:DB$ GainLife | LifeAmount$ 4",
        ),
        "Name:Test Automaton\nManaCost:2\nTypes:Artifact Creature Construct\nPT:1/1\nOracle:"
            .to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    for artifact in [false, true] {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Plains", "Plains"]);
        if artifact {
            sc.card(P0, Zone::Battlefield, "Test Automaton");
        }
        sc.card(P0, Zone::Hand, "Test Reveler");
        let mut s = Script::new(sc.build());
        s.cast("Test Reveler").pay().pass_all().pass_all();
        assert_eq!(life(&s, P0), if artifact { 44 } else { 40 });
    }
}

// What can't be given. An ability about casting, about a card as it enters, or of a layer
// that is over when abilities are given (CR 613.1f) is refused by the card checks; a
// script variable that isn't an activated ability, and an ability that gives itself, by
// the compiler.
#[test]
fn abilities_that_cant_be_given() {
    let lord = |rest: &str| creature("Test Odd Lord", "2", "Golem", "2/2", rest);
    let b = forge_test_blockers(&lord(
        "S:Mode$ Continuous | Affected$ Land | AddReplacementEffect$ Slow | Description$ Text.\n\
         SVar:Slow:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplacementResult$ Updated | ReplaceWith$ ETBTapped\n\
         SVar:ETBTapped:DB$ Tap | Defined$ Self | ETB$ True",
    ));
    assert!(
        b.iter()
            .any(|x| x.starts_with("invalid:") && x.contains("another object can be given")),
        "{b:?}"
    );
    // A given static ability that changes types: layer 4 is over by layer 6.
    let b = forge_test_blockers(&lord(
        "S:Mode$ Continuous | Affected$ Creature | AddStaticAbility$ Wall | Description$ Text.\n\
         SVar:Wall:Mode$ Continuous | Affected$ Card.Self | AddType$ Golem | Description$ Text.",
    ));
    assert!(
        b.iter()
            .any(|x| x.starts_with("invalid:") && x.contains("another object can be given")),
        "{b:?}"
    );
    assert_eq!(
        forge_test_blockers(&lord(
            "S:Mode$ Continuous | Affected$ Creature | AddAbility$ Cast | Description$ Text.\n\
             SVar:Cast:SP$ Draw | NumCards$ 1 | SpellDescription$ Text.",
        )),
        ["granted:not an activated ability"]
    );
    assert_eq!(
        forge_test_blockers(&lord(
            "S:Mode$ Continuous | Affected$ Creature | AddStaticAbility$ Loop | Description$ Text.\n\
             SVar:Loop:Mode$ Continuous | Affected$ Creature | AddStaticAbility$ Loop | Description$ Text.",
        )),
        ["granted:nested too deep"]
    );
}

// ---------------------------------------------------------------------------------------
// Effects that last on no object (Forge's effect objects, D-065).

const SLIPSTREAM: &str = "Name:Test Slipstream\nManaCost:U\nTypes:Instant\n\
    A:SP$ Effect | ValidTgts$ Creature | RememberObjects$ Targeted | ExileOnMoved$ Battlefield | StaticAbilities$ Unblockable | SpellDescription$ Text.\n\
    SVar:Unblockable:Mode$ CantBlockBy | ValidAttacker$ Card.IsRemembered | Description$ Text.\nOracle:Text.";

// CR 611.2a–b, 509.1b, 514.2 (2026-11-06): "Target creature can't be blocked this turn" —
// a rule for a while, about that creature. CR 113.12: it "is neither granting an ability
// nor setting a characteristic", so losing all abilities doesn't end it. Forge: `Effect`
// with `RememberObjects$ Targeted` and a `StaticAbilities$` of `Mode$ CantBlockBy |
// ValidAttacker$ Card.IsRemembered`.
#[test]
fn target_creature_cant_be_blocked_this_turn() {
    let hush = aura(
        "Test Hush",
        "U",
        "Creature",
        "S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | RemoveAllAbilities$ True | Description$ Text.",
    );
    let cards = [SLIPSTREAM, &hush, SMITE];
    let build = || {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Island", "Island", "Swamp"]);
        let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
        let giant = sc.card(P0, Zone::Battlefield, "Test Giant");
        sc.card(P1, Zone::Battlefield, "Test Golem");
        sc.cards(
            P0,
            Zone::Hand,
            &["Test Slipstream", "Test Hush", "Test Smiting"],
        );
        let mut s = Script::new(sc.build());
        s.cast("Test Slipstream")
            .target_object(golem)
            .pay()
            .pass_all();
        (s, golem, giant)
    };
    // The Golem can't be blocked; the Giant, which the effect isn't about, can.
    let (mut s, golem, giant) = build();
    assert_eq!(given(&s, golem), 0, "not an ability of the creature");
    s.cast("Test Hush").target_object(golem).pay().pass_all();
    attack_with(&mut s, &[golem, giant]);
    assert_eq!(offered_blockers(&mut s, golem), [] as [&str; 0]);
    let (mut s, golem, giant) = build();
    attack_with(&mut s, &[golem, giant]);
    assert_eq!(offered_blockers(&mut s, giant), ["Test Golem"]);
    // It lasts until the turn ends.
    assert_eq!(engine_core::view::player_view(&s.st, P1).lasting.len(), 1);
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert!(s.st.lasting.is_empty());
    // And it is over when the creature it is about is gone (CR 400.7).
    let (mut s, golem, _) = build();
    s.cast("Test Smiting").target_object(golem).pay().pass_all();
    assert!(engine_core::view::player_view(&s.st, P0).lasting.is_empty());
}

// Forge: `ValidBlocker$ <filter>` — "can't be blocked by creatures with power 2 or less";
// `ValidAttacker$ Creature.EffectSource | ValidBlocker$ Creature.IsRemembered` — "target
// creature can't block this creature this turn"; `RememberObjects$ Self` — "this creature
// can't be blocked this turn"; `Duration$ UntilHostLeavesPlayOrEOT`.
#[test]
fn cant_be_blocked_by_some_creatures_and_cant_block_this_creature() {
    let cards = [
        creature(
            "Test Tunnel Guide",
            "1 G",
            "Dwarf",
            "1/1",
            "A:AB$ Effect | Cost$ 1 | ValidTgts$ Creature | RememberObjects$ Targeted | StaticAbilities$ Low | SpellDescription$ Text.\n\
             SVar:Low:Mode$ CantBlockBy | ValidAttacker$ Creature.IsRemembered | ValidBlocker$ Creature.powerLE2 | Description$ Text.",
        ),
        creature(
            "Test Skitterer",
            "1 R",
            "Insect",
            "2/1",
            "A:AB$ Effect | Cost$ R | ValidTgts$ Creature | RememberObjects$ Targeted | ExileOnMoved$ Battlefield | StaticAbilities$ NotMe | Duration$ UntilHostLeavesPlayOrEOT | SpellDescription$ Text.\n\
             SVar:NotMe:Mode$ CantBlockBy | ValidAttacker$ Creature.EffectSource | ValidBlocker$ Creature.IsRemembered | Description$ Text.",
        ),
        creature(
            "Test Shade Runner",
            "1 U",
            "Rogue",
            "1/1",
            "A:AB$ Effect | Cost$ U | RememberObjects$ Self | ExileOnMoved$ Battlefield | StaticAbilities$ Unblockable | SpellDescription$ Text.\n\
             SVar:Unblockable:Mode$ CantBlockBy | ValidAttacker$ Card.IsRemembered | Description$ Text.",
        ),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = || {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Mountain", "Island", "Forest"]);
        let mine = sc.cards(
            P0,
            Zone::Battlefield,
            &[
                "Test Tunnel Guide",
                "Test Skitterer",
                "Test Shade Runner",
                "Test Golem",
            ],
        );
        let theirs = sc.cards(P1, Zone::Battlefield, &["Test Golem", "Test Giant"]);
        (Script::new(sc.build()), mine, theirs)
    };
    // The Golem (power 2) can't block the guided creature; the Giant can.
    let (mut s, mine, _) = build();
    s.activate("Test Tunnel Guide")
        .target_object(mine[3])
        .pay()
        .pass_all();
    attack_with(&mut s, &mine);
    assert_eq!(offered_blockers(&mut s, mine[3]), ["Test Giant"]);
    // The Giant can't block the Skitterer — but it can block anything else.
    let (mut s, mine, theirs) = build();
    s.activate("Test Skitterer")
        .target_object(theirs[1])
        .pay()
        .pass_all();
    attack_with(&mut s, &mine);
    assert_eq!(offered_blockers(&mut s, mine[1]), ["Test Golem"]);
    let (mut s, mine, theirs) = build();
    s.activate("Test Skitterer")
        .target_object(theirs[1])
        .pay()
        .pass_all();
    attack_with(&mut s, &mine);
    assert_eq!(
        offered_blockers(&mut s, mine[3]),
        ["Test Golem", "Test Giant"]
    );
    // "This creature can't be blocked this turn."
    let (mut s, mine, _) = build();
    s.activate("Test Shade Runner").pay().pass_all();
    attack_with(&mut s, &mine);
    assert_eq!(offered_blockers(&mut s, mine[2]), [] as [&str; 0]);
}

fn prevented(s: &Script) -> u32 {
    s.events
        .iter()
        .filter_map(|e| match e {
            Event::DamagePrevented { amount, .. } => Some(*amount),
            _ => None,
        })
        .sum()
}

// CR 615.1a, 611.2c (2026-11-06): "Prevent all combat damage that would be dealt this
// turn" — a prevention effect for a while, about no object in particular; damage that
// isn't combat damage is dealt. Forge: `Effect | ReplacementEffects$ <SVar>` with
// `Event$ DamageDone | Prevent$ True | IsCombat$ True | ActiveZones$ Command`.
#[test]
fn prevent_all_combat_damage_this_turn() {
    let cards = [
        spell(
            "Test Haze",
            "G",
            "Instant",
            "A:SP$ Effect | ReplacementEffects$ RPrevent | SpellDescription$ Text.\n\
             SVar:RPrevent:Event$ DamageDone | Prevent$ True | IsCombat$ True | ActiveZones$ Command | Description$ Text.",
        ),
        ZAP.to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Mountain"]);
    let giant = sc.card(P0, Zone::Battlefield, "Test Giant"); // 5/4
    sc.cards(P0, Zone::Hand, &["Test Haze", "Test Zapping"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Haze").pay().pass_all();
    s.cast("Test Zapping").target_player(P1).pay().pass_all();
    assert_eq!(life(&s, P1), 37, "not combat damage");
    attack_with(&mut s, &[giant]);
    drive_until(&mut s, |s| s.st.turn.step == Step::PostcombatMain);
    assert_eq!(life(&s, P1), 37, "the Giant's 5 combat damage is prevented");
    assert_eq!(prevented(&s), 5);
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert!(s.st.lasting.is_empty());
}

// CR 615.1a (2026-11-06). Forge: `ValidSource$ Card.IsRemembered` — "prevent all combat
// damage that would be dealt by target creature this turn"; `ValidTarget$
// Card.IsRemembered` — "prevent all damage that would be dealt to target creature this
// turn".
#[test]
fn prevent_damage_by_or_to_a_target_creature() {
    let cards = [
        spell(
            "Test Muzzling",
            "W",
            "Instant",
            "A:SP$ Effect | ValidTgts$ Creature | RememberObjects$ Targeted | ExileOnMoved$ Battlefield | ReplacementEffects$ RPrevent | SpellDescription$ Text.\n\
             SVar:RPrevent:Event$ DamageDone | Prevent$ True | IsCombat$ True | ValidSource$ Card.IsRemembered | Description$ Text.",
        ),
        spell(
            "Test Sheltering",
            "W",
            "Instant",
            "A:SP$ Effect | ValidTgts$ Creature | RememberObjects$ Targeted | ExileOnMoved$ Battlefield | ReplacementEffects$ RPrevent | SpellDescription$ Text.\n\
             SVar:RPrevent:Event$ DamageDone | Prevent$ True | ValidTarget$ Card.IsRemembered | Description$ Text.",
        ),
        ZAP.to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Plains", "Plains", "Mountain", "Mountain"],
    );
    let giant = sc.card(P0, Zone::Battlefield, "Test Giant"); // 5/4
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem"); // 2/3
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(
        P0,
        Zone::Hand,
        &[
            "Test Muzzling",
            "Test Sheltering",
            "Test Zapping",
            "Test Zapping",
        ],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Sheltering")
        .target_object(golem)
        .pay()
        .pass_all();
    s.cast("Test Zapping").target_object(golem).pay().pass_all();
    assert!(on_battlefield(&s, golem));
    assert_eq!(s.st.object(golem).damage_marked, 0);
    assert_eq!(prevented(&s), 3);
    // Only that creature: the other Golem takes its 3 and dies.
    s.cast("Test Zapping")
        .target_object(theirs)
        .pay()
        .pass_all();
    assert!(!on_battlefield(&s, theirs));
    // The muzzled Giant deals no combat damage; the Golem beside it deals its 2.
    s.cast("Test Muzzling")
        .target_object(giant)
        .pay()
        .pass_all();
    attack_with(&mut s, &[giant, golem]);
    drive_until(&mut s, |s| s.st.turn.step == Step::PostcombatMain);
    assert_eq!(life(&s, P1), 38);
    assert_eq!(prevented(&s), 3 + 5);
}

// CR 615.1a, 614.1a (2026-11-06): a permanent's own prevention effect. Forge: an `R:` line
// `Event$ DamageDone | Prevent$ True` with `ValidTarget$` / `ValidSource$` / `IsCombat$` —
// "Prevent all noncombat damage that would be dealt to creatures you control", "Prevent
// all damage that would be dealt by enchanted creature."
#[test]
fn permanents_that_prevent_damage() {
    let cards = [
        "Name:Test Idol\nManaCost:2\nTypes:Artifact\n\
         R:Event$ DamageDone | ActiveZones$ Battlefield | Prevent$ True | ValidTarget$ Creature.YouCtrl | IsCombat$ False | Description$ Text.\nOracle:Text."
            .to_string(),
        aura(
            "Test Pacifism",
            "W",
            "Creature",
            "R:Event$ DamageDone | ActiveZones$ Battlefield | Prevent$ True | ValidSource$ Creature.EnchantedBy | Description$ Text.",
        ),
        ZAP.to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Mountain", "Mountain"]);
    sc.card(P0, Zone::Battlefield, "Test Idol");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let giant = sc.card(P0, Zone::Battlefield, "Test Giant");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Pacifism", "Test Zapping", "Test Zapping"],
    );
    let mut s = Script::new(sc.build());
    // Noncombat damage to my creatures is prevented; to another player's, it isn't.
    s.cast("Test Zapping").target_object(golem).pay().pass_all();
    assert_eq!(s.st.object(golem).damage_marked, 0);
    s.cast("Test Zapping")
        .target_object(theirs)
        .pay()
        .pass_all();
    assert!(!on_battlefield(&s, theirs));
    // The enchanted Giant deals no damage; the Golem does.
    s.cast("Test Pacifism")
        .target_object(giant)
        .pay()
        .pass_all();
    attack_with(&mut s, &[giant, golem]);
    drive_until(&mut s, |s| s.st.turn.step == Step::PostcombatMain);
    assert_eq!(life(&s, P1), 38);
}

// What an effect object can't carry yet is named, kind by kind; and what an effect is
// about has to be a permanent.
#[test]
fn effect_objects_not_compiled() {
    let with = |effect: &str, svar: &str| {
        spell(
            "Test Oddment",
            "U",
            "Instant",
            &format!("A:SP$ Effect | {effect} | SpellDescription$ Text.\n{svar}"),
        )
    };
    assert_eq!(
        forge_test_blockers(&with(
            "Triggers$ Later",
            "SVar:Later:Mode$ Phase | Phase$ Upkeep | Execute$ TrigDraw | TriggerDescription$ Text.\nSVar:TrigDraw:DB$ Draw"
        )),
        ["Effect:Triggers"]
    );
    assert_eq!(
        forge_test_blockers(&with(
            "StaticAbilities$ Big",
            "SVar:Big:Mode$ Continuous | Affected$ Creature.YouCtrl | AddPower$ 1 | Description$ Text."
        )),
        ["Effect:static Continuous"]
    );
    assert_eq!(
        forge_test_blockers(&with(
            "ValidTgts$ Player | RememberObjects$ Targeted | StaticAbilities$ No",
            "SVar:No:Mode$ CantAttack | ValidCard$ Creature | Description$ Text."
        )),
        ["Effect:remembers a target that isn't a permanent"]
    );
    assert_eq!(
        forge_test_blockers(&with(
            "StaticAbilities$ No | Duration$ UntilEndOfCombat",
            "SVar:No:Mode$ CantAttack | ValidCard$ Creature | Description$ Text."
        )),
        ["Effect:duration UntilEndOfCombat"]
    );
    // "You have hexproof" is about a player: "You" is never a subtype of permanents. It is
    // a rule about the player (D-070, tests in forge_players.rs).
    let mask = "Name:Test Warding Mask\nManaCost:2 W\nTypes:Enchantment\n\
         S:Mode$ Continuous | Affected$ You | AddKeyword$ Hexproof | Description$ Text.\nOracle:Text.";
    assert_eq!(forge_test_blockers(mask), [] as [&str; 0]);
    let cat = forge_test_catalog(&[mask], &[]);
    assert!(matches!(
        cat.get(cat.find("Test Warding Mask").unwrap())
            .abilities
            .as_slice(),
        [card_model::AbilityDef::PlayerRule {
            rule: card_model::PlayerRule::Hexproof,
            ..
        }]
    ));
}

// CR 113.12, 611.2c (2026-11-06): "can't block" is a rule about a creature, not an ability
// it has. Forge writes such rules as keywords: Pump with `KW$ HIDDEN CARDNAME can't
// block.` — "target creature can't block this turn"; PumpAll — "creatures without flying
// can't block this turn", which, being a rule and no change of characteristics, also holds
// for a creature that arrives afterwards.
#[test]
fn creatures_that_cant_block_this_turn() {
    let cards = [
        spell(
            "Test Stunning",
            "R",
            "Instant",
            "A:SP$ Pump | ValidTgts$ Creature | KW$ HIDDEN CARDNAME can't block. | IsCurse$ True | SpellDescription$ Text.",
        ),
        spell(
            "Test Tremor",
            "R",
            "Sorcery",
            "A:SP$ PumpAll | ValidCards$ Creature.withoutFlying | KW$ HIDDEN CARDNAME can't block. | IsCurse$ True | SpellDescription$ Text.",
        ),
        creature("Test Ambusher", "G", "Cat", "1/1", "K:Flash"),
        creature("Test Gull", "U", "Bird", "1/1", "K:Flying"),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let build = |hand: &[&str]| {
        let mut sc = scenario(&cards);
        sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain"]);
        let giant = sc.card(P0, Zone::Battlefield, "Test Giant");
        sc.cards(
            P1,
            Zone::Battlefield,
            &["Forest", "Test Golem", "Test Gull"],
        );
        sc.card(P1, Zone::Hand, "Test Ambusher");
        sc.cards(P0, Zone::Hand, hand);
        (Script::new(sc.build()), giant)
    };
    // One creature: the others may block.
    let (mut s, giant) = build(&["Test Stunning"]);
    let golem = s.find(P1, Zone::Battlefield, "Test Golem").unwrap();
    s.cast("Test Stunning")
        .target_object(golem)
        .pay()
        .pass_all();
    assert_eq!(given(&s, golem), 0);
    attack_with(&mut s, &[giant]);
    assert_eq!(offered_blockers(&mut s, giant), ["Test Gull"]);
    // Every creature without flying — also the one that comes in at the beginning of
    // combat, after the spell has resolved.
    let (mut s, giant) = build(&["Test Tremor"]);
    s.cast("Test Tremor").pay().pass_all();
    drive_until(&mut s, |s| {
        s.st.turn.step == Step::BeginningOfCombat
            && s.who() == P1
            && s.kind() == ChoiceKind::Priority
    });
    s.cast("Test Ambusher").pay().pass_all();
    assert!(s.find(P1, Zone::Battlefield, "Test Ambusher").is_some());
    attack_with(&mut s, &[giant]);
    assert_eq!(offered_blockers(&mut s, giant), ["Test Gull"]);
}

// Forge writes rules about a card as keyword lines — `K:CARDNAME can't block.`,
// `K:Prevent all combat damage that would be dealt to and dealt by CARDNAME.` — and
// about other permanents as `AddHiddenKeyword$` on a static ability ("enchanted creature
// can't attack or block").
#[test]
fn rules_written_as_keywords() {
    let cards = [
        creature(
            "Test Mistform",
            "1 U",
            "Illusion",
            "0/2",
            "K:Prevent all combat damage that would be dealt to and dealt by CARDNAME.",
        ),
        creature("Test Craven", "B", "Rat", "3/1", "K:CARDNAME can't block."),
        aura(
            "Test Shackles",
            "W",
            "Creature",
            "S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | AddHiddenKeyword$ CARDNAME can't attack or block. | Description$ Text.",
        ),
        ZAP.to_string(),
    ];
    let cards: Vec<&str> = cards.iter().map(String::as_str).collect();
    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Mountain"]);
    let giant = sc.card(P0, Zone::Battlefield, "Test Giant");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let mist = sc.card(P1, Zone::Battlefield, "Test Mistform");
    sc.card(P1, Zone::Battlefield, "Test Craven");
    sc.cards(P0, Zone::Hand, &["Test Shackles", "Test Zapping"]);
    let mut s = Script::new(sc.build());
    assert_eq!(chars(&s.st, mist).keywords, [] as [Keyword; 0]);
    // Not combat damage: the Mistform takes it.
    s.cast("Test Zapping").target_object(mist).pay().pass_all();
    assert!(!on_battlefield(&s, mist));

    let mut sc = scenario(&cards);
    sc.cards(P0, Zone::Battlefield, &["Plains", "Mountain"]);
    let giant2 = sc.card(P0, Zone::Battlefield, "Test Giant");
    let golem2 = sc.card(P0, Zone::Battlefield, "Test Golem");
    let mist = sc.card(P1, Zone::Battlefield, "Test Mistform");
    sc.card(P1, Zone::Battlefield, "Test Craven");
    sc.card(P0, Zone::Hand, "Test Shackles");
    let mut s = Script::new(sc.build());
    let _ = (giant, golem);
    // The shackled Golem isn't asked to attack; the Craven isn't offered as a blocker.
    s.cast("Test Shackles")
        .target_object(golem2)
        .pay()
        .pass_all();
    let asked = attack_with(&mut s, &[giant2]);
    assert_eq!(asked, [giant2]);
    // The Mistform blocks the Giant: no combat damage either way.
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareBlockers);
    let offered: Vec<String> = s
        .options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Block { blocker, .. } => Some(s.name(*blocker)),
            _ => None,
        })
        .collect();
    assert_eq!(offered, ["Test Mistform"]);
    block_with(&mut s, &[(mist, giant2)]);
    fight(&mut s);
    drive_until(&mut s, |s| s.st.turn.step == Step::PostcombatMain);
    assert!(on_battlefield(&s, mist) && life(&s, P1) == 40);
    assert_eq!(s.st.object(giant2).damage_marked, 0);
}
