//! Tests for real cards in the M4 precons (decisions.md D-034). They need the printed card
//! data in the local Scryfall cache: run `cargo run -p harness -- fetch-cards` once.
//! Every verified script in `crates/cards/scripts` names at least one test in this file.

use card_model::Keyword;
use engine_core::layers::chars;
use engine_core::mana::ManaType;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{
    ChoiceKind, Event, GameConfig, ObjectId, OptionDesc, PlayerId, Primitive, Response,
};
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn scenario() -> Scenario {
    let mut sc = Scenario::new(GameConfig::commander(2), harness::full_catalog(), 31);
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

fn has(s: &Script, id: ObjectId, k: Keyword) -> bool {
    chars(&s.st, id).keywords.contains(&k)
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

fn hand(s: &Script, p: PlayerId) -> usize {
    s.st.zones.list(Zone::Hand, p).len()
}

fn pool(s: &Script, p: PlayerId, t: ManaType) -> u32 {
    s.st.player(p).mana_pool.amounts[t.index()]
}

fn triggers(s: &Script) -> usize {
    s.count(|e| matches!(e, Event::Triggered { .. }))
}

/// Pass priority around until the stack is empty (answering nothing else).
fn resolve_all(s: &mut Script) {
    for _ in 0..50 {
        if s.st.stack.is_empty() || s.kind() != ChoiceKind::Priority {
            return;
        }
        s.pass();
    }
    panic!("the stack never emptied");
}

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

/// Every verified real-card script names tests that exist in this file.
#[test]
fn every_verified_real_card_names_existing_tests() {
    let this = include_str!("cards_precon.rs");
    let mut verified = 0;
    for &(path, text) in cards::SCRIPTS {
        let script =
            card_db::parse_real(card_db::Source { path, text }).unwrap_or_else(|e| panic!("{e}"));
        if !script.verified {
            continue;
        }
        verified += 1;
        assert!(!script.tests.is_empty(), "{} names no test", script.name);
        for t in &script.tests {
            assert!(
                this.contains(&format!("fn {t}(")),
                "{} names missing test `{t}`",
                script.name
            );
        }
        assert!(
            harness::full_catalog().find(&script.name).is_some(),
            "{} didn't load",
            script.name
        );
    }
    assert!(verified >= 88);
}

#[test]
fn gigantosaurus_is_a_vanilla_10_10() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Forest"; 5]);
    sc.card(P0, Zone::Hand, "Gigantosaurus");
    let mut s = Script::new(sc.build());
    s.cast("Gigantosaurus").pay().pass_all();
    let id = s
        .find(P0, Zone::Battlefield, "Gigantosaurus")
        .expect("resolved");
    let c = chars(&s.st, id);
    assert_eq!((c.power, c.toughness), (Some(10), Some(10)));
    assert!(c.keywords.is_empty() && c.abilities.is_empty());
    assert_eq!(c.mana_cost.unwrap().mana_value(), 5);
    assert_eq!(c.subtypes, ["Dinosaur"]);
}

#[test]
fn segovian_angel_has_flying_and_vigilance() {
    let mut sc = scenario();
    sc.turn(P0, Step::BeginningOfCombat);
    let angel = sc.card(P0, Zone::Battlefield, "Segovian Angel");
    sc.card(P1, Zone::Battlefield, "Test Bruiser"); // no flying or reach: can't block it
    let mut s = Script::new(sc.build());
    drive_until(&mut s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    s.pick(
        "attack",
        |_, o| matches!(o, OptionDesc::Attack { attacker, .. } if *attacker == angel),
    );
    assert!(
        !s.st.object(angel).tapped,
        "vigilance: attacking doesn't tap it"
    );
    drive_until(&mut s, |s| s.st.turn.step == Step::EndOfCombat);
    assert!(
        !s.answered
            .iter()
            .any(|d| d.kind == ChoiceKind::DeclareBlockers),
        "flying: the ground creature was never offered a block"
    );
    assert_eq!(s.st.player(P1).life, 39);
}

#[test]
fn terrian_world_tyrant_is_a_legendary_9_7() {
    let mut sc = scenario();
    let id = sc.card(P0, Zone::Battlefield, "Terrian, World Tyrant");
    let s = Script::new(sc.build());
    let c = chars(&s.st, id);
    assert_eq!((c.power, c.toughness), (Some(9), Some(7)));
    assert!(c.supertypes.contains(&card_model::Supertype::Legendary));
    assert_eq!(c.subtypes, ["Dinosaur", "Ooze"]);
    assert_eq!(c.colors, [card_model::Color::Green]);
}

/// "{T}: Add {G}." on a 1/1 Elf Druid; summoning sickness applies (CR 302.6).
fn mana_elf(name: &str) {
    let mut sc = scenario();
    let ready = sc.card(P0, Zone::Battlefield, name);
    let sick = sc.card(P0, Zone::Battlefield, name);
    sc.sick(sick);
    let mut s = Script::new(sc.build());
    let c = chars(&s.st, ready);
    assert_eq!((c.power, c.toughness), (Some(1), Some(1)));
    assert_eq!(c.subtypes, ["Elf", "Druid"]);
    assert_eq!(c.mana_cost.unwrap().to_text(), "{G}");
    let taps = |s: &Script, id: ObjectId| {
        s.has(|_, o| matches!(o, OptionDesc::TapForMana { source, .. } if *source == id))
    };
    assert!(taps(&s, ready));
    assert!(!taps(&s, sick), "a summoning-sick creature can't {{T}}");
    s.tap(name, ManaType::Green);
    assert!(s.st.object(ready).tapped);
    assert_eq!(pool(&s, P0, ManaType::Green), 1);
    assert_eq!(s.st.player(P0).mana_pool.total(), 1);
}

#[test]
fn elvish_mystic_taps_for_green() {
    mana_elf("Elvish Mystic");
}

#[test]
fn fyndhorn_elves_taps_for_green() {
    mana_elf("Fyndhorn Elves");
}

#[test]
fn llanowar_elves_taps_for_green() {
    mana_elf("Llanowar Elves");
}

#[test]
fn harmonize_draws_three() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Forest"; 4]);
    sc.card(P0, Zone::Hand, "Harmonize");
    let mut s = Script::new(sc.build());
    s.cast("Harmonize").pay().pass_all();
    assert_eq!(hand(&s, P0), 3);
    assert_eq!(hand(&s, P1), 0);
    assert!(s.find(P0, Zone::Graveyard, "Harmonize").is_some());
}

#[test]
fn nights_whisper_draws_two_and_costs_two_life() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 2]);
    sc.card(P0, Zone::Hand, "Night's Whisper");
    let mut s = Script::new(sc.build());
    s.cast("Night's Whisper").pay().pass_all();
    assert_eq!((hand(&s, P0), life(&s, P0)), (2, 38));
    assert_eq!((hand(&s, P1), life(&s, P1)), (0, 40));
}

#[test]
fn ambitions_cost_draws_three_and_costs_three_life() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 4]);
    sc.card(P0, Zone::Hand, "Ambition's Cost");
    let mut s = Script::new(sc.build());
    s.cast("Ambition's Cost").pay().pass_all();
    assert_eq!((hand(&s, P0), life(&s, P0)), (3, 37));
}

#[test]
fn lightning_bolt_deals_three_to_any_target() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 2]);
    sc.cards(P0, Zone::Hand, &["Lightning Bolt"; 2]);
    sc.card(P1, Zone::Battlefield, "Test Golem"); // 2/3
    let mut s = Script::new(sc.build());
    // An instant for {R}.
    let bolt = s.find(P0, Zone::Hand, "Lightning Bolt").unwrap();
    let c = chars(&s.st, bolt);
    assert_eq!(c.mana_cost.as_ref().unwrap().to_text(), "{R}");
    assert!(c.is(card_model::CardType::Instant));
    s.cast("Lightning Bolt").target_player(P1).pay().pass_all();
    assert_eq!(life(&s, P1), 37);
    s.cast("Lightning Bolt")
        .target("Test Golem")
        .pay()
        .pass_all();
    assert!(s.find(P1, Zone::Graveyard, "Test Golem").is_some());
}

#[test]
fn sign_in_blood_can_target_any_player() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 4]);
    sc.cards(P0, Zone::Hand, &["Sign in Blood"; 2]);
    let mut s = Script::new(sc.build());
    s.cast("Sign in Blood").target_player(P1).pay().pass_all();
    assert_eq!((hand(&s, P1), life(&s, P1)), (2, 38));
    assert_eq!((hand(&s, P0), life(&s, P0)), (1, 40));
    s.cast("Sign in Blood").target_player(P0).pay().pass_all();
    assert_eq!((hand(&s, P0), life(&s, P0)), (2, 38));
}

#[test]
fn beast_whisperer_draws_on_creature_spells_only() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Beast Whisperer");
    sc.cards(P0, Zone::Battlefield, &["Forest", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Llanowar Elves", "Lightning Bolt"]);
    let mut s = Script::new(sc.build());
    // The trigger goes on the stack above the creature spell and resolves first.
    s.cast("Llanowar Elves").pay();
    assert_eq!((s.st.stack.len(), triggers(&s)), (2, 1));
    s.pass_all();
    assert_eq!(hand(&s, P0), 2, "drew before the Elves resolved");
    assert_eq!(s.st.stack.len(), 1);
    resolve_all(&mut s);
    // A noncreature spell doesn't trigger it.
    s.cast("Lightning Bolt").target_player(P1).pay();
    assert_eq!((s.st.stack.len(), triggers(&s)), (1, 1));
}

#[test]
fn inspiring_overseer_gains_a_life_and_draws() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Plains"; 3]);
    sc.card(P0, Zone::Hand, "Inspiring Overseer");
    let mut s = Script::new(sc.build());
    s.cast("Inspiring Overseer").pay();
    resolve_all(&mut s);
    let id = s.find(P0, Zone::Battlefield, "Inspiring Overseer").unwrap();
    assert_eq!(pt(&s, id), (2, 1));
    assert!(has(&s, id, Keyword::Flying));
    assert_eq!(chars(&s.st, id).subtypes, ["Angel", "Cleric"]);
    assert_eq!((life(&s, P0), hand(&s, P0), triggers(&s)), (41, 1, 1));
}

#[test]
fn phyrexian_arena_draws_and_drains_each_upkeep() {
    let mut sc = scenario();
    sc.turn(P1, Step::End);
    sc.card(P0, Zone::Battlefield, "Phyrexian Arena");
    let mut s = Script::new(sc.build());
    // Into P0's turn: the upkeep trigger, then the normal draw.
    drive_until(&mut s, |s| {
        s.st.turn.active == P0 && s.st.turn.step == Step::PrecombatMain
    });
    assert_eq!((life(&s, P0), hand(&s, P0), triggers(&s)), (39, 2, 1));
    // "your upkeep": nothing on the opponent's.
    drive_until(&mut s, |s| {
        s.st.turn.active == P1 && s.st.turn.step == Step::PrecombatMain
    });
    assert_eq!((life(&s, P0), life(&s, P1), triggers(&s)), (39, 40, 1));
}

#[test]
fn managorger_hydra_grows_with_every_spell() {
    let mut sc = scenario();
    let hydra = sc.card(P0, Zone::Battlefield, "Managorger Hydra");
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Lightning Bolt");
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Lightning Bolt");
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, hydra), (1, 1));
    assert!(has(&s, hydra, Keyword::Trample));
    // "Whenever a player casts a spell": its controller's spell, then an opponent's.
    s.cast("Lightning Bolt").target_player(P1).pay();
    assert_eq!(triggers(&s), 1);
    s.pass();
    assert_eq!(s.who(), P1);
    s.cast("Lightning Bolt").target_player(P0).pay();
    assert_eq!(triggers(&s), 2);
    resolve_all(&mut s);
    assert_eq!(pt(&s, hydra), (3, 3));
    assert_eq!((life(&s, P0), life(&s, P1)), (37, 37));
}

#[test]
fn dazzling_angel_gains_life_for_other_creatures() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Dazzling Angel");
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Plains", "Plains", "Plains"],
    );
    sc.cards(P0, Zone::Hand, &["Llanowar Elves", "Dazzling Angel"]);
    let mut s = Script::new(sc.build());
    s.cast("Llanowar Elves").pay();
    resolve_all(&mut s);
    assert_eq!((life(&s, P0), triggers(&s)), (41, 1));
    // A second Angel entering: the first one sees it; the new one doesn't see itself.
    s.cast("Dazzling Angel").pay();
    resolve_all(&mut s);
    assert_eq!((life(&s, P0), triggers(&s)), (42, 2));
    let id = s.find(P0, Zone::Battlefield, "Dazzling Angel").unwrap();
    assert_eq!(pt(&s, id), (2, 3));
    assert!(has(&s, id, Keyword::Flying));
}

#[test]
fn radiant_fountain_gains_two_and_taps_for_colorless() {
    let mut sc = scenario();
    sc.card(P0, Zone::Hand, "Radiant Fountain");
    let mut s = Script::new(sc.build());
    s.pick("play the land", |_, o| {
        matches!(o, OptionDesc::PlayLand { .. })
    });
    resolve_all(&mut s);
    assert_eq!((life(&s, P0), triggers(&s)), (42, 1));
    let id = s.find(P0, Zone::Battlefield, "Radiant Fountain").unwrap();
    assert!(chars(&s.st, id).is(card_model::CardType::Land));
    s.tap("Radiant Fountain", ManaType::Colorless);
    assert_eq!(pool(&s, P0, ManaType::Colorless), 1);
}

#[test]
fn diamonds_enter_tapped_and_tap_for_their_color() {
    for (name, mana) in [
        ("Charcoal Diamond", ManaType::Black),
        ("Fire Diamond", ManaType::Red),
        ("Marble Diamond", ManaType::White),
    ] {
        let mut sc = scenario();
        sc.cards(P0, Zone::Battlefield, &["Forest"; 2]);
        sc.card(P0, Zone::Hand, name);
        let ready = sc.card(P0, Zone::Battlefield, name);
        let mut s = Script::new(sc.build());
        s.cast(name).pay().pass_all();
        let ids: Vec<ObjectId> =
            s.st.zones
                .battlefield
                .iter()
                .copied()
                .filter(|id| s.name(*id) == name)
                .collect();
        assert_eq!(ids.len(), 2, "{name} resolved");
        let entered = ids.into_iter().find(|id| *id != ready).unwrap();
        assert!(s.st.object(entered).tapped, "{name} enters tapped");
        assert!(chars(&s.st, entered).is(card_model::CardType::Artifact));
        // The one that was already there taps for exactly its colour.
        let offered: Vec<ManaType> = s
            .options()
            .iter()
            .filter_map(|o| match o {
                OptionDesc::TapForMana { source, mana, .. } if *source == ready => Some(*mana),
                _ => None,
            })
            .collect();
        assert_eq!(offered, [mana], "{name}");
        s.tap(name, mana);
        assert_eq!(pool(&s, P0, mana), 1);
    }
}

#[test]
fn always_watching_pumps_nontoken_creatures_only() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Always Watching");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem"); // 2/3
    let token = sc.card(P0, Zone::Battlefield, "Test Soldier"); // a 1/1 token
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, golem), (3, 4));
    assert!(has(&s, golem, Keyword::Vigilance));
    assert_eq!(pt(&s, token), (1, 1));
    assert!(!has(&s, token, Keyword::Vigilance));
    assert_eq!(pt(&s, theirs), (2, 3));
    assert!(!has(&s, theirs, Keyword::Vigilance));
}

#[test]
fn thraben_watcher_pumps_other_nontoken_creatures() {
    let mut sc = scenario();
    let watcher = sc.card(P0, Zone::Battlefield, "Thraben Watcher");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let token = sc.card(P0, Zone::Battlefield, "Test Soldier");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    let s = Script::new(sc.build());
    // "Other": the Watcher itself stays a 2/2 (with its own flying and vigilance).
    assert_eq!(pt(&s, watcher), (2, 2));
    assert!(has(&s, watcher, Keyword::Flying) && has(&s, watcher, Keyword::Vigilance));
    assert_eq!(pt(&s, golem), (3, 4));
    assert!(has(&s, golem, Keyword::Vigilance));
    assert_eq!(pt(&s, token), (1, 1));
    assert!(!has(&s, token, Keyword::Vigilance));
    assert_eq!(pt(&s, theirs), (2, 3));
}

#[test]
fn fleshbag_marauder_makes_each_player_sacrifice() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 3]);
    sc.card(P0, Zone::Battlefield, "Test Sprout");
    sc.card(P0, Zone::Hand, "Fleshbag Marauder");
    sc.cards(P1, Zone::Battlefield, &["Test Golem", "Test Sprout"]);
    let mut s = Script::new(sc.build());
    s.cast("Fleshbag Marauder").pay();
    // Resolve the creature, then its enters trigger; each player chooses (APNAP order).
    let mut choosers = Vec::new();
    for _ in 0..20 {
        match s.kind() {
            ChoiceKind::Sacrifice => {
                choosers.push(s.who());
                // P0 may keep the Marauder itself; both players give up their Sprout.
                s.pick(
                    "the Sprout",
                    |s, o| matches!(o, OptionDesc::Card { card } if s.name(*card) == "Test Sprout"),
                );
            }
            ChoiceKind::Priority if !s.st.stack.is_empty() => {
                s.pass();
            }
            _ => break,
        }
    }
    assert_eq!(choosers, [P0, P1]);
    let marauder = s.find(P0, Zone::Battlefield, "Fleshbag Marauder").unwrap();
    assert_eq!(pt(&s, marauder), (3, 1));
    assert_eq!(chars(&s.st, marauder).subtypes, ["Zombie", "Warrior"]);
    assert!(s.find(P0, Zone::Graveyard, "Test Sprout").is_some());
    assert!(s.find(P1, Zone::Graveyard, "Test Sprout").is_some());
    assert!(s.find(P1, Zone::Battlefield, "Test Golem").is_some());
}

#[test]
fn regal_imperiosaur_pumps_other_dinosaurs() {
    let mut sc = scenario();
    let regal = sc.card(P0, Zone::Battlefield, "Regal Imperiosaur");
    let dino = sc.card(P0, Zone::Battlefield, "Gigantosaurus");
    let elf = sc.card(P0, Zone::Battlefield, "Llanowar Elves");
    let theirs = sc.card(P1, Zone::Battlefield, "Gigantosaurus");
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, regal), (5, 4), "other Dinosaurs: not itself");
    assert_eq!(pt(&s, dino), (11, 11));
    assert_eq!(pt(&s, elf), (1, 1), "not a Dinosaur");
    assert_eq!(pt(&s, theirs), (10, 10), "you control");
}

#[test]
fn crucible_of_fire_pumps_your_dragon_creatures() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Crucible of Fire");
    let dragon = sc.card(P0, Zone::Battlefield, "Test Wyrmling"); // a 2/2 Dragon
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Wyrmling");
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, dragon), (5, 5));
    assert_eq!(pt(&s, golem), (2, 3));
    assert_eq!(pt(&s, theirs), (2, 2));
}

#[test]
fn youthful_valkyrie_grows_when_another_angel_enters() {
    let mut sc = scenario();
    let valkyrie = sc.card(P0, Zone::Battlefield, "Youthful Valkyrie");
    sc.cards(P0, Zone::Battlefield, &["Plains", "Forest"]);
    sc.cards(P0, Zone::Hand, &["Segovian Angel", "Llanowar Elves"]);
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, valkyrie), (1, 3));
    assert!(has(&s, valkyrie, Keyword::Flying));
    s.cast("Segovian Angel").pay();
    resolve_all(&mut s);
    assert_eq!((pt(&s, valkyrie), triggers(&s)), ((2, 4), 1));
    // Not an Angel: nothing.
    s.cast("Llanowar Elves").pay();
    resolve_all(&mut s);
    assert_eq!((pt(&s, valkyrie), triggers(&s)), ((2, 4), 1));
}

#[test]
fn champion_of_the_perished_grows_when_another_zombie_enters() {
    let mut sc = scenario();
    let champion = sc.card(P0, Zone::Battlefield, "Champion of the Perished");
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Forest"]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Champion of the Perished", "Llanowar Elves"],
    );
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, champion), (1, 1));
    s.cast("Champion of the Perished").pay();
    resolve_all(&mut s);
    // The first one grows; the one that just entered doesn't see itself.
    assert_eq!((pt(&s, champion), triggers(&s)), ((2, 2), 1));
    s.cast("Llanowar Elves").pay();
    resolve_all(&mut s);
    assert_eq!((pt(&s, champion), triggers(&s)), ((2, 2), 1));
}

// ------------------------------------------------------------------------------------------
// Batch 2: mass effects, permanent targets, exile, tokens, attack triggers (D-041)
// ------------------------------------------------------------------------------------------

fn named(s: &Script, name: &str) -> Vec<ObjectId> {
    s.st.zones
        .battlefield
        .iter()
        .copied()
        .filter(|id| s.name(*id) == name)
        .collect()
}

fn in_zone(s: &Script, p: PlayerId, zone: Zone) -> usize {
    s.st.zones.list(zone, p).len()
}

fn can_target(s: &Script, id: ObjectId) -> bool {
    use engine_core::stack::Target;
    s.has(|_, o| matches!(o, OptionDesc::Target { target: Target::Object(t) } if *t == id))
}

/// Declare exactly `attackers` (at the first player offered) and nothing else.
fn attack_with(s: &mut Script, attackers: &[ObjectId]) {
    drive_until(s, |s| s.kind() == ChoiceKind::DeclareAttackers);
    while s.next.is_some() && s.kind() == ChoiceKind::DeclareAttackers {
        let asked = match s.options().first() {
            Some(OptionDesc::Attack { attacker, .. }) => *attacker,
            other => panic!("{other:?}"),
        };
        let pick = if attackers.contains(&asked) {
            vec![0]
        } else {
            vec![]
        };
        s.respond(Response::Pick(pick));
    }
}

/// From the opponent's end step into P0's first main phase (through P0's upkeep).
fn through_upkeep(sc: Scenario) -> Script {
    let mut s = Script::new(sc.build());
    drive_until(&mut s, |s| {
        s.st.turn.active == P0 && s.st.turn.step == Step::PrecombatMain
    });
    s
}

#[test]
fn birds_of_paradise_fly_and_tap_for_any_color() {
    let mut sc = scenario();
    let birds = sc.card(P0, Zone::Battlefield, "Birds of Paradise");
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, birds), (0, 1));
    assert!(has(&s, birds, Keyword::Flying));
    let mut offered: Vec<ManaType> = s
        .options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::TapForMana { source, mana, .. } if *source == birds => Some(*mana),
            _ => None,
        })
        .collect();
    offered.sort_by_key(|m| m.index());
    assert_eq!(
        offered,
        [
            ManaType::White,
            ManaType::Blue,
            ManaType::Black,
            ManaType::Red,
            ManaType::Green
        ]
    );
    s.tap("Birds of Paradise", ManaType::Blue);
    assert_eq!(pool(&s, P0, ManaType::Blue), 1);
}

#[test]
fn seraph_sanctuary_gains_life_for_itself_and_for_angels() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Plains");
    sc.cards(P0, Zone::Hand, &["Seraph Sanctuary", "Segovian Angel"]);
    let mut s = Script::new(sc.build());
    s.pick("play the land", |_, o| {
        matches!(o, OptionDesc::PlayLand { .. })
    });
    resolve_all(&mut s);
    assert_eq!((life(&s, P0), triggers(&s)), (41, 1), "it isn't an Angel");
    s.cast("Segovian Angel").pay();
    resolve_all(&mut s);
    assert_eq!((life(&s, P0), triggers(&s)), (42, 2));
    s.tap("Seraph Sanctuary", ManaType::Colorless);
    assert_eq!(pool(&s, P0, ManaType::Colorless), 1);
}

#[test]
fn lyra_dawnbringer_gives_other_angels_a_bonus_and_lifelink() {
    let mut sc = scenario();
    let lyra = sc.card(P0, Zone::Battlefield, "Lyra Dawnbringer");
    let angel = sc.card(P0, Zone::Battlefield, "Segovian Angel");
    let elf = sc.card(P0, Zone::Battlefield, "Llanowar Elves");
    let theirs = sc.card(P1, Zone::Battlefield, "Segovian Angel");
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, lyra), (5, 5));
    for k in [Keyword::Flying, Keyword::FirstStrike, Keyword::Lifelink] {
        assert!(has(&s, lyra, k), "{k:?}");
    }
    assert_eq!(pt(&s, angel), (2, 2));
    assert!(has(&s, angel, Keyword::Lifelink));
    assert_eq!(pt(&s, elf), (1, 1));
    assert!(!has(&s, elf, Keyword::Lifelink));
    assert_eq!(pt(&s, theirs), (1, 1));
    assert!(!has(&s, theirs, Keyword::Lifelink));
}

#[test]
fn day_of_judgment_destroys_all_creatures() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Plains"; 4]);
    sc.cards(P0, Zone::Battlefield, &["Test Golem", "Always Watching"]);
    sc.cards(P1, Zone::Battlefield, &["Gigantosaurus", "Test Sprout"]);
    sc.card(P0, Zone::Hand, "Day of Judgment");
    let mut s = Script::new(sc.build());
    s.cast("Day of Judgment").pay().pass_all();
    assert_eq!(
        in_zone(&s, P0, Zone::Graveyard),
        2,
        "the Golem and the spell"
    );
    assert_eq!(in_zone(&s, P1, Zone::Graveyard), 2);
    assert_eq!(named(&s, "Plains").len(), 4);
    assert_eq!(named(&s, "Always Watching").len(), 1);
}

#[test]
fn cleansing_nova_destroys_creatures_or_artifacts_and_enchantments() {
    for mode in [0u8, 1] {
        let mut sc = scenario();
        sc.cards(P0, Zone::Battlefield, &["Plains"; 5]);
        sc.cards(P0, Zone::Battlefield, &["Test Golem", "Always Watching"]);
        sc.cards(P1, Zone::Battlefield, &["Test Sprout", "Marble Diamond"]);
        sc.card(P0, Zone::Hand, "Cleansing Nova");
        let mut s = Script::new(sc.build());
        s.cast("Cleansing Nova").mode(mode).pay().pass_all();
        let creatures = named(&s, "Test Golem").len() + named(&s, "Test Sprout").len();
        let others = named(&s, "Always Watching").len() + named(&s, "Marble Diamond").len();
        assert_eq!((creatures, others), if mode == 0 { (0, 2) } else { (2, 0) });
        assert_eq!(named(&s, "Plains").len(), 5);
    }
}

#[test]
fn sunblast_angel_destroys_tapped_creatures() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Plains"; 6]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.tapped(mine);
    let theirs = sc.card(P1, Zone::Battlefield, "Gigantosaurus");
    sc.tapped(theirs);
    sc.card(P1, Zone::Battlefield, "Test Sprout");
    sc.card(P0, Zone::Hand, "Sunblast Angel");
    let mut s = Script::new(sc.build());
    s.cast("Sunblast Angel").pay();
    resolve_all(&mut s);
    assert!(named(&s, "Test Golem").is_empty() && named(&s, "Gigantosaurus").is_empty());
    assert_eq!(named(&s, "Test Sprout").len(), 1, "untapped: spared");
    let angel = named(&s, "Sunblast Angel")[0];
    assert_eq!(pt(&s, angel), (4, 5));
    assert!(has(&s, angel, Keyword::Flying));
}

#[test]
fn breath_weapon_spares_dragons() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    sc.card(P0, Zone::Battlefield, "Test Wyrmling"); // a 2/2 Dragon
    sc.card(P0, Zone::Battlefield, "Test Sprout"); // 1/1
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem"); // 2/3
    sc.card(P0, Zone::Hand, "Breath Weapon");
    let mut s = Script::new(sc.build());
    s.cast("Breath Weapon").pay().pass_all();
    assert_eq!(named(&s, "Test Wyrmling").len(), 1);
    assert!(named(&s, "Test Sprout").is_empty());
    assert_eq!(s.st.object(golem).damage_marked, 2);
    assert_eq!((life(&s, P0), life(&s, P1)), (40, 40));
}

#[test]
fn chain_reaction_counts_the_creatures() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 4]);
    sc.card(P0, Zone::Battlefield, "Test Golem"); // 2/3
    sc.card(P1, Zone::Battlefield, "Test Grazer"); // 3/3
    let big = sc.card(P1, Zone::Battlefield, "Gigantosaurus"); // 10/10
    sc.card(P0, Zone::Hand, "Chain Reaction");
    let mut s = Script::new(sc.build());
    s.cast("Chain Reaction").pay().pass_all();
    // Three creatures: 3 damage to each.
    assert!(named(&s, "Test Golem").is_empty() && named(&s, "Test Grazer").is_empty());
    assert_eq!(s.st.object(big).damage_marked, 3);
}

#[test]
fn go_for_the_throat_cant_target_artifact_creatures() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 2]);
    // Test Assembly makes noncreature artifacts 2/2 creatures: the Prism is an artifact
    // creature.
    sc.card(P1, Zone::Battlefield, "Test Assembly");
    let prism = sc.card(P1, Zone::Battlefield, "Test Prism");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Go for the Throat");
    let mut s = Script::new(sc.build());
    assert!(chars(&s.st, prism).is(card_model::CardType::Creature));
    s.cast("Go for the Throat");
    assert!(can_target(&s, golem));
    assert!(!can_target(&s, prism), "nonartifact creatures only");
    s.target_object(golem).pay().pass_all();
    assert!(named(&s, "Test Golem").is_empty());
}

#[test]
fn invoke_the_divine_destroys_an_artifact_or_enchantment() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Plains"; 3]);
    let diamond = sc.card(P1, Zone::Battlefield, "Fire Diamond");
    let aura = sc.card(P1, Zone::Battlefield, "Always Watching");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Invoke the Divine");
    let mut s = Script::new(sc.build());
    s.cast("Invoke the Divine");
    assert!(can_target(&s, diamond) && can_target(&s, aura) && !can_target(&s, golem));
    s.target_object(diamond).pay().pass_all();
    assert!(named(&s, "Fire Diamond").is_empty());
    assert_eq!(life(&s, P0), 44);
}

#[test]
fn withering_torment_destroys_a_creature_or_enchantment() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 3]);
    let diamond = sc.card(P1, Zone::Battlefield, "Fire Diamond");
    let aura = sc.card(P1, Zone::Battlefield, "Always Watching");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Withering Torment");
    let mut s = Script::new(sc.build());
    s.cast("Withering Torment");
    assert!(can_target(&s, golem) && can_target(&s, aura) && !can_target(&s, diamond));
    s.target_object(aura).pay().pass_all();
    assert!(named(&s, "Always Watching").is_empty());
    assert_eq!(life(&s, P0), 38);
}

#[test]
fn destroy_evil_needs_toughness_four_or_an_enchantment() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Plains"; 4]);
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem"); // toughness 3
    let big = sc.card(P1, Zone::Battlefield, "Gigantosaurus");
    let aura = sc.card(P1, Zone::Battlefield, "Bad Moon");
    sc.cards(P0, Zone::Hand, &["Destroy Evil"; 2]);
    let mut s = Script::new(sc.build());
    s.cast("Destroy Evil").mode(0);
    assert!(can_target(&s, big) && !can_target(&s, golem) && !can_target(&s, aura));
    s.target_object(big).pay().pass_all();
    assert!(named(&s, "Gigantosaurus").is_empty());
    s.cast("Destroy Evil").mode(1);
    assert!(can_target(&s, aura) && !can_target(&s, golem));
    s.target_object(aura).pay().pass_all();
    assert!(named(&s, "Bad Moon").is_empty());
}

#[test]
fn abrade_burns_a_creature_or_destroys_an_artifact() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 4]);
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    let diamond = sc.card(P1, Zone::Battlefield, "Marble Diamond");
    sc.cards(P0, Zone::Hand, &["Abrade"; 2]);
    let mut s = Script::new(sc.build());
    s.cast("Abrade").mode(0);
    assert!(can_target(&s, golem) && !can_target(&s, diamond));
    s.target_object(golem).pay().pass_all();
    assert!(named(&s, "Test Golem").is_empty(), "3 damage kills a 2/3");
    s.cast("Abrade").mode(1);
    s.target_object(diamond).pay().pass_all();
    assert!(named(&s, "Marble Diamond").is_empty());
}

#[test]
fn exorcise_exiles_an_artifact_enchantment_or_big_creature() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Plains"; 2]);
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem"); // power 2
    let big = sc.card(P1, Zone::Battlefield, "Gigantosaurus");
    let aura = sc.card(P1, Zone::Battlefield, "Always Watching");
    let diamond = sc.card(P1, Zone::Battlefield, "Fire Diamond");
    sc.card(P0, Zone::Hand, "Exorcise");
    let mut s = Script::new(sc.build());
    s.cast("Exorcise");
    assert!(can_target(&s, big) && can_target(&s, aura) && can_target(&s, diamond));
    assert!(!can_target(&s, golem), "power 4 or greater");
    s.target_object(big).pay().pass_all();
    assert!(named(&s, "Gigantosaurus").is_empty());
    assert_eq!(in_zone(&s, P1, Zone::Exile), 1);
    assert_eq!(in_zone(&s, P1, Zone::Graveyard), 0);
}

#[test]
fn vengeful_dead_drains_when_it_or_another_zombie_dies() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    sc.card(P0, Zone::Battlefield, "Vengeful Dead"); // 3/2
    sc.card(P1, Zone::Battlefield, "Champion of the Perished"); // a Zombie, an opponent's
    sc.card(P1, Zone::Battlefield, "Test Sprout");
    sc.cards(P0, Zone::Hand, &["Lightning Bolt"; 3]);
    let mut s = Script::new(sc.build());
    // Not a Zombie: nothing.
    s.cast("Lightning Bolt").target("Test Sprout").pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), life(&s, P1)), (0, 40));
    // "Another Zombie": anyone's.
    s.cast("Lightning Bolt")
        .target("Champion of the Perished")
        .pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), life(&s, P1), life(&s, P0)), (1, 39, 40));
    // Itself.
    s.cast("Lightning Bolt").target("Vengeful Dead").pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), life(&s, P1)), (2, 38));
}

#[test]
fn undead_augur_draws_when_it_or_your_zombie_dies() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    sc.card(P0, Zone::Battlefield, "Undead Augur"); // 2/2
    sc.card(P0, Zone::Battlefield, "Champion of the Perished");
    sc.card(P1, Zone::Battlefield, "Champion of the Perished");
    sc.cards(P0, Zone::Hand, &["Lightning Bolt"; 3]);
    let mut s = Script::new(sc.build());
    // An opponent's Zombie: "you control" — nothing.
    let theirs = s
        .find(P1, Zone::Battlefield, "Champion of the Perished")
        .unwrap();
    s.cast("Lightning Bolt").target_object(theirs).pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), hand(&s, P0), life(&s, P0)), (0, 2, 40));
    let mine = s
        .find(P0, Zone::Battlefield, "Champion of the Perished")
        .unwrap();
    s.cast("Lightning Bolt").target_object(mine).pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), hand(&s, P0), life(&s, P0)), (1, 2, 39));
    s.cast("Lightning Bolt").target("Undead Augur").pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), hand(&s, P0), life(&s, P0)), (2, 2, 38));
}

#[test]
fn open_the_graves_makes_a_zombie_for_each_nontoken_creature() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 2]);
    sc.card(P0, Zone::Battlefield, "Open the Graves");
    sc.card(P0, Zone::Battlefield, "Test Sprout");
    sc.cards(P0, Zone::Hand, &["Lightning Bolt"; 2]);
    let mut s = Script::new(sc.build());
    s.cast("Lightning Bolt").target("Test Sprout").pay();
    resolve_all(&mut s);
    let zombies = named(&s, "Zombie Token");
    assert_eq!(zombies.len(), 1);
    let z = zombies[0];
    assert_eq!(pt(&s, z), (2, 2));
    let c = chars(&s.st, z);
    // CR 111.4: a token is named for its subtype plus the word "Token".
    assert_eq!(c.name, "Zombie Token");
    assert_eq!(c.colors, [card_model::Color::Black]);
    assert_eq!(c.subtypes, ["Zombie"]);
    assert!(s.st.object(z).is_token);
    // The token dying makes nothing ("nontoken").
    s.cast("Lightning Bolt").target("Zombie Token").pay();
    resolve_all(&mut s);
    assert!(named(&s, "Zombie Token").is_empty());
    assert_eq!(triggers(&s), 1);
}

#[test]
fn lilianas_mastery_pumps_zombies_and_makes_two() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 5]);
    let dead = sc.card(P0, Zone::Battlefield, "Vengeful Dead"); // a 3/2 Zombie
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Vengeful Dead");
    sc.card(P0, Zone::Hand, "Liliana's Mastery");
    let mut s = Script::new(sc.build());
    s.cast("Liliana's Mastery").pay();
    resolve_all(&mut s);
    let zombies = named(&s, "Zombie Token");
    assert_eq!(zombies.len(), 2);
    assert!(zombies.iter().all(|z| pt(&s, *z) == (3, 3)));
    assert_eq!(pt(&s, dead), (4, 3));
    assert_eq!(pt(&s, golem), (2, 3));
    assert_eq!(pt(&s, theirs), (3, 2));
}

#[test]
fn arasta_makes_spiders_when_opponents_cast_instants_and_sorceries() {
    let mut sc = scenario();
    let arasta = sc.card(P0, Zone::Battlefield, "Arasta of the Endless Web");
    sc.cards(P0, Zone::Battlefield, &["Mountain"]);
    sc.card(P0, Zone::Hand, "Lightning Bolt");
    sc.cards(P1, Zone::Battlefield, &["Mountain", "Forest"]);
    sc.cards(P1, Zone::Hand, &["Lightning Bolt"]);
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, arasta), (3, 5));
    assert!(has(&s, arasta, Keyword::Reach));
    // Your own instant: nothing.
    s.cast("Lightning Bolt").target_player(P1).pay();
    assert_eq!(triggers(&s), 0);
    s.pass();
    // An opponent's instant, in response.
    assert_eq!(s.who(), P1);
    s.cast("Lightning Bolt").target_player(P0).pay();
    assert_eq!(triggers(&s), 1);
    resolve_all(&mut s);
    let spiders = named(&s, "Spider Token");
    assert_eq!(spiders.len(), 1);
    assert_eq!(pt(&s, spiders[0]), (1, 2));
    assert!(has(&s, spiders[0], Keyword::Reach));
    assert_eq!(chars(&s.st, spiders[0]).colors, [card_model::Color::Green]);
    assert_eq!(s.st.object(spiders[0]).controller, P0);
}

#[test]
fn bishop_of_wings_gains_life_and_leaves_spirits() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Bishop of Wings");
    sc.cards(P0, Zone::Battlefield, &["Plains", "Mountain"]);
    sc.cards(P0, Zone::Hand, &["Segovian Angel", "Lightning Bolt"]);
    let mut s = Script::new(sc.build());
    s.cast("Segovian Angel").pay();
    resolve_all(&mut s);
    assert_eq!(life(&s, P0), 44);
    s.cast("Lightning Bolt").target("Segovian Angel").pay();
    resolve_all(&mut s);
    let spirits = named(&s, "Spirit Token");
    assert_eq!(spirits.len(), 1);
    assert_eq!(pt(&s, spirits[0]), (1, 1));
    assert!(has(&s, spirits[0], Keyword::Flying));
    assert_eq!(chars(&s.st, spirits[0]).colors, [card_model::Color::White]);
    assert_eq!(triggers(&s), 2);
}

#[test]
fn headless_rider_replaces_nontoken_zombies() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 3]);
    sc.card(P0, Zone::Battlefield, "Headless Rider"); // 3/1
    sc.card(P0, Zone::Battlefield, "Champion of the Perished");
    sc.cards(P0, Zone::Hand, &["Lightning Bolt"; 3]);
    let mut s = Script::new(sc.build());
    s.cast("Lightning Bolt")
        .target("Champion of the Perished")
        .pay();
    resolve_all(&mut s);
    assert_eq!(named(&s, "Zombie Token").len(), 1);
    // A Zombie token dying: "nontoken" — nothing.
    s.cast("Lightning Bolt").target("Zombie Token").pay();
    resolve_all(&mut s);
    assert_eq!((named(&s, "Zombie Token").len(), triggers(&s)), (0, 1));
    // The Rider itself.
    s.cast("Lightning Bolt").target("Headless Rider").pay();
    resolve_all(&mut s);
    assert_eq!((named(&s, "Zombie Token").len(), triggers(&s)), (1, 2));
}

#[test]
fn grave_titan_makes_zombies_when_it_enters_or_attacks() {
    // Enters.
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 6]);
    sc.card(P0, Zone::Hand, "Grave Titan");
    let mut s = Script::new(sc.build());
    s.cast("Grave Titan").pay();
    resolve_all(&mut s);
    let titan = named(&s, "Grave Titan")[0];
    assert_eq!(pt(&s, titan), (6, 6));
    assert!(has(&s, titan, Keyword::Deathtouch));
    assert_eq!((named(&s, "Zombie Token").len(), triggers(&s)), (2, 1));
    // Attacks (CR 508.1m): the trigger resolves in the declare attackers step.
    let mut sc = scenario();
    sc.turn(P0, Step::BeginningOfCombat);
    let titan = sc.card(P0, Zone::Battlefield, "Grave Titan");
    let mut s = Script::new(sc.build());
    attack_with(&mut s, &[titan]);
    assert_eq!(triggers(&s), 1);
    assert_eq!(s.st.turn.step, Step::DeclareAttackers);
    resolve_all(&mut s);
    assert_eq!(named(&s, "Zombie Token").len(), 2);
}

#[test]
fn utvara_hellkite_makes_a_dragon_for_each_attacking_dragon() {
    let mut sc = scenario();
    sc.turn(P0, Step::BeginningOfCombat);
    let utvara = sc.card(P0, Zone::Battlefield, "Utvara Hellkite");
    let wyrmling = sc.card(P0, Zone::Battlefield, "Test Wyrmling");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, utvara), (6, 6));
    attack_with(&mut s, &[utvara, wyrmling, golem]);
    // One trigger for each attacking Dragon (itself and the Wyrmling), none for the Golem.
    assert_eq!(triggers(&s), 2);
    // Both are P0's: P0 orders them (CR 603.3b).
    assert_eq!(s.kind(), ChoiceKind::OrderTriggers);
    s.respond(Response::Order(vec![0, 1]));
    resolve_all(&mut s);
    let tokens: Vec<ObjectId> = named(&s, "Dragon Token");
    assert_eq!(tokens.len(), 2);
    for t in tokens {
        assert_eq!(pt(&s, t), (6, 6));
        assert!(has(&s, t, Keyword::Flying));
        assert_eq!(chars(&s.st, t).colors, [card_model::Color::Red]);
    }
}

#[test]
fn bad_moon_pumps_every_black_creature() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Bad Moon");
    let mine = sc.card(P0, Zone::Battlefield, "Champion of the Perished"); // black 1/1
    let theirs = sc.card(P1, Zone::Battlefield, "Fleshbag Marauder"); // black 3/1
    let green = sc.card(P0, Zone::Battlefield, "Llanowar Elves");
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, mine), (2, 2));
    assert_eq!(pt(&s, theirs), (4, 2), "not only yours");
    assert_eq!(pt(&s, green), (1, 1));
}

#[test]
fn dragonmaster_outcast_makes_a_dragon_with_six_lands() {
    for (lands, dragons) in [(5usize, 0usize), (6, 1)] {
        let mut sc = scenario();
        sc.turn(P1, Step::End);
        sc.card(P0, Zone::Battlefield, "Dragonmaster Outcast");
        sc.cards(P0, Zone::Battlefield, &vec!["Mountain"; lands]);
        let s = through_upkeep(sc);
        let tokens = named(&s, "Dragon Token");
        assert_eq!(tokens.len(), dragons, "{lands} lands");
        for t in tokens {
            assert_eq!(pt(&s, t), (5, 5));
            assert!(has(&s, t, Keyword::Flying));
        }
    }
}

#[test]
fn colossal_majesty_draws_with_a_big_creature() {
    for (creature, cards) in [("Test Golem", 1usize), ("Gigantosaurus", 2)] {
        let mut sc = scenario();
        sc.turn(P1, Step::End);
        sc.card(P0, Zone::Battlefield, "Colossal Majesty");
        sc.card(P0, Zone::Battlefield, creature);
        let s = through_upkeep(sc);
        // The draw step's card, plus one if a creature has power 4 or greater.
        assert_eq!(hand(&s, P0), cards, "{creature}");
    }
}

#[test]
fn elemental_bond_draws_for_power_three_or_more() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Elemental Bond");
    sc.cards(P0, Zone::Battlefield, &["Forest"; 4]);
    sc.cards(P0, Zone::Hand, &["Llanowar Elves", "Regal Imperiosaur"]);
    let mut s = Script::new(sc.build());
    s.cast("Llanowar Elves").pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), hand(&s, P0)), (0, 1));
    s.cast("Regal Imperiosaur").pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), hand(&s, P0)), (1, 1));
}

#[test]
fn garruks_uprising_draws_and_gives_trample() {
    // Without a creature of power 4 or greater, entering draws nothing.
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Forest"; 8]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Hand, &["Garruk's Uprising", "Gigantosaurus"]);
    let mut s = Script::new(sc.build());
    s.cast("Garruk's Uprising").pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), hand(&s, P0)), (0, 1));
    assert!(has(&s, golem, Keyword::Trample));
    // A big creature entering draws.
    s.cast("Gigantosaurus").pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), hand(&s, P0)), (1, 1));
    // With one already there, the enchantment entering draws too.
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Forest"; 3]);
    sc.card(P0, Zone::Battlefield, "Gigantosaurus");
    sc.card(P0, Zone::Hand, "Garruk's Uprising");
    let mut s = Script::new(sc.build());
    s.cast("Garruk's Uprising").pay();
    resolve_all(&mut s);
    assert_eq!((triggers(&s), hand(&s, P0)), (1, 1));
}

#[test]
fn mire_triton_mills_two_and_gains_two() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 2]);
    sc.card(P0, Zone::Hand, "Mire Triton");
    let mut s = Script::new(sc.build());
    let library = in_zone(&s, P0, Zone::Library);
    s.cast("Mire Triton").pay();
    resolve_all(&mut s);
    assert_eq!(in_zone(&s, P0, Zone::Library), library - 2);
    assert_eq!(in_zone(&s, P0, Zone::Graveyard), 2);
    assert_eq!(life(&s, P0), 42);
    let triton = named(&s, "Mire Triton")[0];
    assert!(has(&s, triton, Keyword::Deathtouch));
}

#[test]
fn terror_of_mount_velus_gives_double_strike() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 7]);
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Hand, "Terror of Mount Velus");
    let mut s = Script::new(sc.build());
    s.cast("Terror of Mount Velus").pay();
    resolve_all(&mut s);
    let terror = named(&s, "Terror of Mount Velus")[0];
    assert!(has(&s, terror, Keyword::Flying) && has(&s, terror, Keyword::DoubleStrike));
    assert!(has(&s, golem, Keyword::DoubleStrike));
    assert!(!has(&s, theirs, Keyword::DoubleStrike));
    // "Until end of turn."
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert!(!has(&s, golem, Keyword::DoubleStrike));
    assert!(
        has(&s, terror, Keyword::DoubleStrike),
        "its own keyword stays"
    );
}

#[test]
fn noxious_ghoul_shrinks_non_zombies() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 6]);
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem"); // 2/3
    sc.card(P1, Zone::Battlefield, "Test Sprout"); // 1/1
    let zombie = sc.card(P1, Zone::Battlefield, "Vengeful Dead"); // a 3/2 Zombie
    sc.cards(
        P0,
        Zone::Hand,
        &["Noxious Ghoul", "Champion of the Perished"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Noxious Ghoul").pay();
    resolve_all(&mut s);
    assert_eq!(pt(&s, golem), (1, 2));
    assert!(named(&s, "Test Sprout").is_empty(), "a 1/1 becomes 0/0");
    assert_eq!(pt(&s, zombie), (3, 2), "Zombies are spared");
    // Another Zombie entering does it again.
    s.cast("Champion of the Perished").pay();
    resolve_all(&mut s);
    assert_eq!(pt(&s, golem), (0, 1));
    assert_eq!(pt(&s, named(&s, "Noxious Ghoul")[0]), (3, 3));
}

#[test]
fn graveborn_muse_draws_and_drains_per_zombie() {
    let mut sc = scenario();
    sc.turn(P1, Step::End);
    sc.card(P0, Zone::Battlefield, "Graveborn Muse"); // a Zombie Spirit
    sc.card(P0, Zone::Battlefield, "Champion of the Perished");
    sc.card(P1, Zone::Battlefield, "Vengeful Dead"); // not yours
    let s = through_upkeep(sc);
    // X = 2: two cards and 2 life, then the draw step's card.
    assert_eq!((hand(&s, P0), life(&s, P0)), (3, 38));
}

#[test]
fn angel_of_finality_exiles_a_graveyard() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Plains"; 4]);
    sc.cards(P0, Zone::Graveyard, &["Test Golem"]);
    sc.cards(P1, Zone::Graveyard, &["Test Golem", "Test Sprout"]);
    sc.card(P0, Zone::Hand, "Angel of Finality");
    let mut s = Script::new(sc.build());
    s.cast("Angel of Finality").pay().pass_all();
    // The enters trigger targets a player as it is put on the stack.
    assert_eq!(s.kind(), ChoiceKind::ChooseTargets { slot: 0 });
    s.target_player(P1);
    resolve_all(&mut s);
    assert_eq!(in_zone(&s, P1, Zone::Graveyard), 0);
    assert_eq!(in_zone(&s, P1, Zone::Exile), 2);
    assert_eq!(in_zone(&s, P0, Zone::Graveyard), 1);
    let angel = named(&s, "Angel of Finality")[0];
    assert_eq!(pt(&s, angel), (3, 4));
    assert!(has(&s, angel, Keyword::Flying));
}

#[test]
fn bojuka_bog_enters_tapped_and_exiles_a_graveyard() {
    let mut sc = scenario();
    sc.cards(P1, Zone::Graveyard, &["Test Golem", "Test Sprout"]);
    sc.card(P0, Zone::Hand, "Bojuka Bog");
    let mut s = Script::new(sc.build());
    s.pick("play the land", |_, o| {
        matches!(o, OptionDesc::PlayLand { .. })
    });
    assert_eq!(s.kind(), ChoiceKind::ChooseTargets { slot: 0 });
    s.target_player(P1);
    resolve_all(&mut s);
    let bog = named(&s, "Bojuka Bog")[0];
    assert!(s.st.object(bog).tapped);
    assert_eq!(in_zone(&s, P1, Zone::Exile), 2);
    assert_eq!(in_zone(&s, P1, Zone::Graveyard), 0);
}

// ------------------------------------------------------------------------------------------
// Batch 3: sacrifice as a cost, "activate only if" (D-042)
// ------------------------------------------------------------------------------------------

fn can_activate(s: &Script, id: ObjectId) -> bool {
    s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == id))
}

#[test]
fn lathliss_makes_dragons_and_pumps_them() {
    let mut sc = scenario();
    let lathliss = sc.card(P0, Zone::Battlefield, "Lathliss, Dragon Queen");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 4]);
    sc.card(P0, Zone::Hand, "Test Wyrmling"); // a nontoken Dragon
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, lathliss), (6, 6));
    assert!(has(&s, lathliss, Keyword::Flying));
    s.cast("Test Wyrmling").pay();
    resolve_all(&mut s);
    // One 5/5 Dragon for the Wyrmling; the token itself is a token, so no chain.
    let tokens = named(&s, "Dragon Token");
    assert_eq!((tokens.len(), triggers(&s)), (1, 1));
    assert_eq!(pt(&s, tokens[0]), (5, 5));
    // {1}{R}: Dragons you control get +1/+0 until end of turn.
    s.activate("Lathliss, Dragon Queen").pay().pass_all();
    assert_eq!(pt(&s, lathliss), (7, 6));
    assert_eq!(pt(&s, tokens[0]), (6, 5));
    assert_eq!(pt(&s, named(&s, "Test Wyrmling")[0]), (3, 2));
    assert_eq!(pt(&s, golem), (2, 3));
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert_eq!(pt(&s, lathliss), (6, 6));
}

#[test]
fn lord_of_the_accursed_pumps_zombies_and_gives_menace() {
    let mut sc = scenario();
    let lord = sc.card(P0, Zone::Battlefield, "Lord of the Accursed");
    let mine = sc.card(P0, Zone::Battlefield, "Champion of the Perished");
    let theirs = sc.card(P1, Zone::Battlefield, "Champion of the Perished");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 2]);
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, lord), (2, 3), "other Zombies");
    assert_eq!(pt(&s, mine), (2, 2));
    assert_eq!(pt(&s, theirs), (1, 1), "you control");
    s.activate("Lord of the Accursed").pay().pass_all();
    assert!(s.st.object(lord).tapped);
    // "All Zombies": the opponent's too, and the Lord itself.
    for z in [lord, mine, theirs] {
        assert!(has(&s, z, Keyword::Menace));
    }
    assert!(!has(&s, golem, Keyword::Menace));
}

#[test]
fn geier_reach_sanitarium_makes_everyone_loot() {
    let mut sc = scenario();
    let land = sc.card(P0, Zone::Battlefield, "Geier Reach Sanitarium");
    sc.cards(P0, Zone::Battlefield, &["Forest"; 2]);
    sc.card(P0, Zone::Hand, "Test Golem");
    sc.card(P1, Zone::Hand, "Test Sprout");
    let mut s = Script::new(sc.build());
    let c = chars(&s.st, land);
    assert!(c.is(card_model::CardType::Land));
    assert!(c.supertypes.contains(&card_model::Supertype::Legendary));
    let libraries = (
        in_zone(&s, P0, Zone::Library),
        in_zone(&s, P1, Zone::Library),
    );
    s.activate("Geier Reach Sanitarium").pay().pass_all();
    // Each player draws, then each player discards (their choice, in turn order).
    let mut discarders = Vec::new();
    while s.next.is_some() && s.kind() == ChoiceKind::Discard {
        discarders.push(s.who());
        s.respond(Response::Pick(vec![0]));
    }
    assert_eq!(discarders, [P0, P1]);
    for (p, before) in [(P0, libraries.0), (P1, libraries.1)] {
        assert_eq!(in_zone(&s, p, Zone::Library), before - 1);
        assert_eq!(hand(&s, p), 1);
        assert_eq!(in_zone(&s, p, Zone::Graveyard), 1);
    }
}

#[test]
fn mind_stone_taps_for_mana_or_is_sacrificed_for_a_card() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Mind Stone");
    sc.card(P0, Zone::Battlefield, "Forest");
    let mut s = Script::new(sc.build());
    let offered = s.has(|s, o| {
        matches!(o, OptionDesc::TapForMana { source, mana: ManaType::Colorless, .. }
            if s.name(*source) == "Mind Stone")
    });
    assert!(offered);
    // {1}, {T}, sacrifice: draw a card. The {1} has to come from somewhere else.
    s.activate("Mind Stone").pay().pass_all();
    assert_eq!(hand(&s, P0), 1);
    assert!(named(&s, "Mind Stone").is_empty());
    assert!(s.find(P0, Zone::Graveyard, "Mind Stone").is_some());
    assert_eq!(s.count(|e| matches!(e, Event::Sacrificed { .. })), 1);
}

#[test]
fn thrashing_brontodon_is_sacrificed_to_destroy_an_artifact_or_enchantment() {
    let mut sc = scenario();
    let bronto = sc.card(P0, Zone::Battlefield, "Thrashing Brontodon");
    sc.card(P0, Zone::Battlefield, "Forest");
    // Sacrificing it is dying (CR 700.4): Open the Graves sees a nontoken creature die.
    sc.card(P0, Zone::Battlefield, "Open the Graves");
    let diamond = sc.card(P1, Zone::Battlefield, "Fire Diamond");
    let golem = sc.card(P1, Zone::Battlefield, "Test Golem");
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, bronto), (3, 4));
    s.activate("Thrashing Brontodon");
    assert!(can_target(&s, diamond) && !can_target(&s, golem));
    s.target_object(diamond).pay();
    // The cost is paid as the ability is activated: it is already in the graveyard.
    assert!(named(&s, "Thrashing Brontodon").is_empty());
    assert!(s.find(P0, Zone::Graveyard, "Thrashing Brontodon").is_some());
    resolve_all(&mut s);
    assert!(named(&s, "Fire Diamond").is_empty());
    assert_eq!(named(&s, "Zombie Token").len(), 1);
}

#[test]
fn infernal_idol_taps_for_black_or_is_sacrificed_for_cards() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Infernal Idol");
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 3]);
    let mut s = Script::new(sc.build());
    let offered = s.has(|s, o| {
        matches!(o, OptionDesc::TapForMana { source, mana: ManaType::Black, .. }
            if s.name(*source) == "Infernal Idol")
    });
    assert!(offered);
    s.activate("Infernal Idol").pay().pass_all();
    assert_eq!((hand(&s, P0), life(&s, P0)), (2, 38));
    assert!(s.find(P0, Zone::Graveyard, "Infernal Idol").is_some());
}

#[test]
fn bonders_enclave_draws_only_with_a_big_creature() {
    for (creature, allowed) in [("Test Golem", false), ("Gigantosaurus", true)] {
        let mut sc = scenario();
        let enclave = sc.card(P0, Zone::Battlefield, "Bonders' Enclave");
        sc.cards(P0, Zone::Battlefield, &["Forest"; 3]);
        sc.card(P0, Zone::Battlefield, creature);
        let mut s = Script::new(sc.build());
        assert_eq!(can_activate(&s, enclave), allowed, "{creature}");
        if allowed {
            s.activate("Bonders' Enclave").pay().pass_all();
            assert_eq!(hand(&s, P0), 1);
        }
    }
}

// ------------------------------------------------------------------------------------------
// Batch 4: cost reductions (CR 601.2f, 118.7a) (D-043)
// ------------------------------------------------------------------------------------------

/// Can P0 cast `spell` from hand with `lands` copies of `land` and `permanent` in play?
fn castable_with(permanent: &str, land: &str, lands: usize, spell: &str) -> bool {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, permanent);
    sc.cards(P0, Zone::Battlefield, &vec![land; lands]);
    sc.card(P0, Zone::Hand, spell);
    Script::new(sc.build()).can_cast(spell)
}

/// The mana P0 paid for spells so far.
fn mana_paid(s: &Script) -> u32 {
    s.events
        .iter()
        .map(|e| match e {
            Event::ManaPaid { amount, .. } => *amount,
            _ => 0,
        })
        .sum()
}

#[test]
fn dragonlords_servant_makes_dragon_spells_cheaper() {
    // Utvara Hellkite costs {6}{R}{R}: seven lands with the Servant, not six.
    assert!(castable_with(
        "Dragonlord's Servant",
        "Mountain",
        7,
        "Utvara Hellkite"
    ));
    assert!(!castable_with(
        "Dragonlord's Servant",
        "Mountain",
        6,
        "Utvara Hellkite"
    ));
    // Not a Dragon: full price ({1}{R}{R}… here Lightning Bolt's {R} is untouched, and a
    // 4-mana non-Dragon still needs four).
    assert!(!castable_with(
        "Dragonlord's Servant",
        "Mountain",
        3,
        "Chain Reaction"
    ));
    assert!(castable_with(
        "Dragonlord's Servant",
        "Mountain",
        4,
        "Chain Reaction"
    ));
    // An opponent's Servant doesn't help you.
    let mut sc = scenario();
    sc.card(P1, Zone::Battlefield, "Dragonlord's Servant");
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 7]);
    sc.card(P0, Zone::Hand, "Utvara Hellkite");
    assert!(!Script::new(sc.build()).can_cast("Utvara Hellkite"));
    // And what is paid is the reduced cost.
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Dragonlord's Servant");
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 8]);
    sc.card(P0, Zone::Hand, "Utvara Hellkite");
    let mut s = Script::new(sc.build());
    s.cast("Utvara Hellkite").pay();
    assert_eq!(mana_paid(&s), 7);
}

#[test]
fn dragonspeaker_shaman_takes_two_off_dragon_spells() {
    assert!(castable_with(
        "Dragonspeaker Shaman",
        "Mountain",
        6,
        "Utvara Hellkite"
    ));
    assert!(!castable_with(
        "Dragonspeaker Shaman",
        "Mountain",
        5,
        "Utvara Hellkite"
    ));
    // CR 118.7a: only generic mana comes off — a {1}{R} Dragon still costs {R}.
    assert!(castable_with(
        "Dragonspeaker Shaman",
        "Mountain",
        1,
        "Test Wyrmling"
    ));
    assert!(!castable_with(
        "Dragonspeaker Shaman",
        "Forest",
        1,
        "Test Wyrmling"
    ));
}

#[test]
fn starnheim_aspirant_takes_two_off_angel_spells() {
    // Angel of Finality costs {3}{W}.
    assert!(castable_with(
        "Starnheim Aspirant",
        "Plains",
        2,
        "Angel of Finality"
    ));
    assert!(!castable_with(
        "Starnheim Aspirant",
        "Plains",
        1,
        "Angel of Finality"
    ));
    assert!(!castable_with(
        "Starnheim Aspirant",
        "Plains",
        3,
        "Day of Judgment"
    ));
}

#[test]
fn undead_warchief_discounts_and_pumps_zombies() {
    // Vengeful Dead costs {3}{B}.
    assert!(castable_with(
        "Undead Warchief",
        "Swamp",
        3,
        "Vengeful Dead"
    ));
    assert!(!castable_with(
        "Undead Warchief",
        "Swamp",
        2,
        "Vengeful Dead"
    ));
    let mut sc = scenario();
    let chief = sc.card(P0, Zone::Battlefield, "Undead Warchief");
    let zombie = sc.card(P0, Zone::Battlefield, "Vengeful Dead");
    let theirs = sc.card(P1, Zone::Battlefield, "Vengeful Dead");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, chief), (3, 2), "a Zombie itself: 1/1 +2/+1");
    assert_eq!(pt(&s, zombie), (5, 3));
    assert_eq!(pt(&s, theirs), (3, 2));
    assert_eq!(pt(&s, golem), (2, 3));
}

#[test]
fn bontus_monument_discounts_black_creatures_and_drains() {
    assert!(castable_with(
        "Bontu's Monument",
        "Swamp",
        3,
        "Vengeful Dead"
    ));
    assert!(!castable_with(
        "Bontu's Monument",
        "Swamp",
        2,
        "Vengeful Dead"
    ));
    // A black noncreature spell isn't discounted (Night's Whisper, {1}{B}).
    assert!(!castable_with(
        "Bontu's Monument",
        "Swamp",
        1,
        "Night's Whisper"
    ));
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Bontu's Monument");
    sc.cards(P0, Zone::Battlefield, &["Forest", "Swamp", "Swamp"]);
    sc.cards(P0, Zone::Hand, &["Llanowar Elves", "Night's Whisper"]);
    let mut s = Script::new(sc.build());
    // Any creature spell drains, whatever its colour.
    s.cast("Llanowar Elves").pay();
    resolve_all(&mut s);
    assert_eq!((life(&s, P0), life(&s, P1), triggers(&s)), (41, 39, 1));
    s.cast("Night's Whisper").pay();
    resolve_all(&mut s);
    assert_eq!((life(&s, P1), triggers(&s)), (39, 1));
}

#[test]
fn rhonass_monument_discounts_green_creatures_and_pumps_one() {
    // Regal Imperiosaur costs {1}{G}{G}.
    assert!(castable_with(
        "Rhonas's Monument",
        "Forest",
        2,
        "Regal Imperiosaur"
    ));
    assert!(!castable_with(
        "Rhonas's Monument",
        "Forest",
        1,
        "Regal Imperiosaur"
    ));
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Rhonas's Monument");
    let golem = sc.card(P0, Zone::Battlefield, "Test Golem");
    let theirs = sc.card(P1, Zone::Battlefield, "Test Golem");
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Llanowar Elves");
    let mut s = Script::new(sc.build());
    s.cast("Llanowar Elves").pay();
    // The trigger targets a creature you control.
    assert_eq!(s.kind(), ChoiceKind::ChooseTargets { slot: 0 });
    assert!(can_target(&s, golem) && !can_target(&s, theirs));
    s.target_object(golem);
    resolve_all(&mut s);
    assert_eq!(pt(&s, golem), (4, 5));
    assert!(has(&s, golem, Keyword::Trample));
}

#[test]
fn goreclaw_discounts_big_creatures_and_leads_the_charge() {
    let goreclaw = "Goreclaw, Terror of Qal Sisma";
    // Regal Imperiosaur is a 5/4 for {1}{G}{G}: only its generic mana comes off.
    assert!(castable_with(goreclaw, "Forest", 2, "Regal Imperiosaur"));
    assert!(!castable_with(goreclaw, "Forest", 1, "Regal Imperiosaur"));
    // Power 2: full price ({2}{G}{G} for Beast Whisperer).
    assert!(!castable_with(goreclaw, "Forest", 3, "Beast Whisperer"));
    let mut sc = scenario();
    sc.turn(P0, Step::BeginningOfCombat);
    let bear = sc.card(P0, Zone::Battlefield, goreclaw);
    let big = sc.card(P0, Zone::Battlefield, "Gigantosaurus");
    let small = sc.card(P0, Zone::Battlefield, "Test Golem");
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, bear), (4, 3));
    attack_with(&mut s, &[bear]);
    resolve_all(&mut s);
    assert_eq!(pt(&s, bear), (5, 4));
    assert_eq!(pt(&s, big), (11, 11));
    assert!(has(&s, bear, Keyword::Trample) && has(&s, big, Keyword::Trample));
    assert_eq!(pt(&s, small), (2, 3));
    assert!(!has(&s, small, Keyword::Trample));
}

#[test]
fn blasphemous_act_gets_cheaper_and_deals_thirteen() {
    let setup = |lands: usize| {
        let mut sc = scenario();
        sc.cards(P0, Zone::Battlefield, &vec!["Mountain"; lands]);
        sc.cards(P0, Zone::Battlefield, &["Test Golem", "Test Sprout"]);
        sc.cards(
            P1,
            Zone::Battlefield,
            &["Gigantosaurus", "Test Grazer", "Test Golem"],
        );
        sc.card(P0, Zone::Hand, "Blasphemous Act");
        Script::new(sc.build())
    };
    // {8}{R} with five creatures on the battlefield: {3}{R}.
    assert!(!setup(3).can_cast("Blasphemous Act"));
    let mut s = setup(4);
    s.cast("Blasphemous Act").pay().pass_all();
    assert_eq!(mana_paid(&s), 4);
    // 13 damage kills even a 10/10.
    for name in ["Test Golem", "Test Sprout", "Gigantosaurus", "Test Grazer"] {
        assert!(named(&s, name).is_empty(), "{name}");
    }
}

#[test]
fn ghalta_costs_less_by_your_total_power() {
    let ghalta = "Ghalta, Primal Hunger";
    // {10}{G}{G}: a 10/10 of your own takes off all the generic mana.
    assert!(castable_with("Gigantosaurus", "Forest", 2, ghalta));
    assert!(!castable_with("Gigantosaurus", "Forest", 1, ghalta));
    // A 2/3 takes off two: ten lands, not nine.
    assert!(castable_with("Test Golem", "Forest", 10, ghalta));
    assert!(!castable_with("Test Golem", "Forest", 9, ghalta));
    // An opponent's creatures don't count.
    let mut sc = scenario();
    sc.card(P1, Zone::Battlefield, "Gigantosaurus");
    sc.cards(P0, Zone::Battlefield, &["Forest"; 11]);
    sc.card(P0, Zone::Hand, ghalta);
    assert!(!Script::new(sc.build()).can_cast(ghalta));
    let mut sc = scenario();
    let g = sc.card(P0, Zone::Battlefield, ghalta);
    let s = Script::new(sc.build());
    assert_eq!(pt(&s, g), (12, 12));
    assert!(has(&s, g, Keyword::Trample));
    assert!(
        chars(&s.st, g)
            .supertypes
            .contains(&card_model::Supertype::Legendary)
    );
}

// ------------------------------------------------------------------------------------------
// Batch 5: cards in graveyards as targets, "you may" (D-044)
// ------------------------------------------------------------------------------------------

fn yes(s: &mut Script) {
    assert_eq!(s.kind(), ChoiceKind::MayAbility);
    s.pick("yes", |_, o| matches!(o, OptionDesc::Accept));
}

fn no(s: &mut Script) {
    assert_eq!(s.kind(), ChoiceKind::MayAbility);
    s.pick("no", |_, o| matches!(o, OptionDesc::Decline));
}

#[test]
fn reya_dawnbringer_may_return_a_creature_each_upkeep() {
    for accept in [true, false] {
        let mut sc = scenario();
        sc.turn(P1, Step::End);
        let reya = sc.card(P0, Zone::Battlefield, "Reya Dawnbringer");
        let golem = sc.card(P0, Zone::Graveyard, "Test Golem");
        let spell = sc.card(P0, Zone::Graveyard, "Lightning Bolt");
        let theirs = sc.card(P1, Zone::Graveyard, "Test Golem");
        let mut s = Script::new(sc.build());
        assert_eq!(pt(&s, reya), (4, 6));
        assert!(has(&s, reya, Keyword::Flying));
        // The target is chosen as the trigger goes on the stack: a creature card in your
        // own graveyard.
        drive_until(&mut s, |s| {
            matches!(s.kind(), ChoiceKind::ChooseTargets { .. })
        });
        assert_eq!(s.st.turn.step, Step::Upkeep);
        assert!(can_target(&s, golem) && !can_target(&s, spell) && !can_target(&s, theirs));
        s.target_object(golem);
        drive_until(&mut s, |s| s.kind() == ChoiceKind::MayAbility);
        // "You may": chosen as it resolves.
        if accept {
            yes(&mut s);
            assert_eq!(named(&s, "Test Golem").len(), 1);
            assert_eq!(in_zone(&s, P0, Zone::Graveyard), 1);
        } else {
            no(&mut s);
            assert!(named(&s, "Test Golem").is_empty());
            assert_eq!(in_zone(&s, P0, Zone::Graveyard), 2);
        }
    }
    // Nothing to target: the trigger isn't put on the stack (CR 603.3d).
    let mut sc = scenario();
    sc.turn(P1, Step::End);
    sc.card(P0, Zone::Battlefield, "Reya Dawnbringer");
    let s = through_upkeep(sc);
    assert!(!s.answered.iter().any(|d| d.kind == ChoiceKind::MayAbility));
}

#[test]
fn lord_of_the_undead_pumps_zombies_and_recovers_them() {
    let mut sc = scenario();
    let lord = sc.card(P0, Zone::Battlefield, "Lord of the Undead");
    let theirs = sc.card(P1, Zone::Battlefield, "Vengeful Dead");
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 2]);
    let zombie = sc.card(P0, Zone::Graveyard, "Vengeful Dead");
    let golem = sc.card(P0, Zone::Graveyard, "Test Golem");
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, lord), (2, 2), "other Zombies");
    assert_eq!(pt(&s, theirs), (4, 3), "anyone's Zombie creatures");
    s.activate("Lord of the Undead");
    assert!(
        can_target(&s, zombie) && !can_target(&s, golem),
        "a Zombie card"
    );
    s.target_object(zombie).pay().pass_all();
    assert_eq!(hand(&s, P0), 1);
    assert!(s.find(P0, Zone::Hand, "Vengeful Dead").is_some());
}

#[test]
fn memorial_to_folly_is_sacrificed_to_recover_a_creature() {
    let mut sc = scenario();
    sc.card(P0, Zone::Hand, "Memorial to Folly");
    let mut s = Script::new(sc.build());
    s.pick("play the land", |_, o| {
        matches!(o, OptionDesc::PlayLand { .. })
    });
    assert!(s.st.object(named(&s, "Memorial to Folly")[0]).tapped);
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Memorial to Folly");
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 3]);
    let golem = sc.card(P0, Zone::Graveyard, "Test Golem");
    let mut s = Script::new(sc.build());
    s.activate("Memorial to Folly")
        .target_object(golem)
        .pay()
        .pass_all();
    assert!(s.find(P0, Zone::Hand, "Test Golem").is_some());
    assert!(s.find(P0, Zone::Graveyard, "Memorial to Folly").is_some());
}

#[test]
fn oversold_cemetery_needs_four_creature_cards() {
    for (creatures, triggers_at_upkeep) in [(3usize, false), (4, true)] {
        let mut sc = scenario();
        sc.turn(P1, Step::End);
        sc.card(P0, Zone::Battlefield, "Oversold Cemetery");
        sc.cards(P0, Zone::Graveyard, &vec!["Test Golem"; creatures]);
        sc.card(P0, Zone::Graveyard, "Lightning Bolt"); // not a creature card
        let mut s = Script::new(sc.build());
        drive_until(&mut s, |s| {
            matches!(s.kind(), ChoiceKind::ChooseTargets { .. })
                || (s.st.turn.active == P0 && s.st.turn.step == Step::PrecombatMain)
        });
        let triggered = matches!(s.kind(), ChoiceKind::ChooseTargets { .. });
        assert_eq!(triggered, triggers_at_upkeep, "{creatures} creature cards");
        if triggered {
            s.respond(Response::Pick(vec![0]));
            drive_until(&mut s, |s| s.kind() == ChoiceKind::MayAbility);
            yes(&mut s);
            // The Golem, and then the draw step's card.
            drive_until(&mut s, |s| s.st.turn.step == Step::PrecombatMain);
            assert_eq!(hand(&s, P0), 2);
            assert_eq!(in_zone(&s, P0, Zone::Graveyard), creatures);
        }
    }
}

#[test]
fn cemetery_reaper_exiles_a_creature_card_for_a_zombie() {
    let mut sc = scenario();
    let reaper = sc.card(P0, Zone::Battlefield, "Cemetery Reaper");
    let mine = sc.card(P0, Zone::Battlefield, "Vengeful Dead");
    let theirs_zombie = sc.card(P1, Zone::Battlefield, "Vengeful Dead");
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 3]);
    let theirs = sc.card(P1, Zone::Graveyard, "Test Golem");
    let bolt = sc.card(P1, Zone::Graveyard, "Lightning Bolt");
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, reaper), (2, 2));
    assert_eq!(pt(&s, mine), (4, 3));
    assert_eq!(pt(&s, theirs_zombie), (3, 2), "you control");
    s.activate("Cemetery Reaper");
    assert!(
        can_target(&s, theirs) && !can_target(&s, bolt),
        "any graveyard's creature card"
    );
    s.target_object(theirs).pay().pass_all();
    assert_eq!(in_zone(&s, P1, Zone::Exile), 1);
    // The new Zombie gets the Reaper's bonus.
    let tokens = named(&s, "Zombie Token");
    assert_eq!(tokens.len(), 1);
    assert_eq!(pt(&s, tokens[0]), (3, 3));
}

#[test]
fn garruks_packleader_may_draw_for_big_creatures() {
    let mut sc = scenario();
    let pack = sc.card(P0, Zone::Battlefield, "Garruk's Packleader");
    sc.cards(P0, Zone::Battlefield, &["Forest"; 7]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Llanowar Elves", "Regal Imperiosaur", "Regal Imperiosaur"],
    );
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, pack), (4, 4));
    s.cast("Llanowar Elves").pay();
    resolve_all(&mut s);
    assert_eq!(triggers(&s), 0, "power 1");
    s.cast("Regal Imperiosaur").pay();
    drive_until(&mut s, |s| s.kind() == ChoiceKind::MayAbility);
    yes(&mut s);
    assert_eq!(hand(&s, P0), 2, "drew one");
    s.cast("Regal Imperiosaur").pay();
    drive_until(&mut s, |s| s.kind() == ChoiceKind::MayAbility);
    no(&mut s);
    assert_eq!(hand(&s, P0), 1);
    assert_eq!(triggers(&s), 2);
}

// ------------------------------------------------------------------------------------------
// Batch 6: "it" — the object that triggered the ability (CR 608.2h) (D-045)
// ------------------------------------------------------------------------------------------

/// (source, amount) of every instance of noncombat damage dealt so far.
fn damage_dealt(s: &Script) -> Vec<(ObjectId, u32)> {
    s.events
        .iter()
        .filter_map(|e| match e {
            Event::DamageDealt {
                source,
                amount,
                combat: false,
                ..
            } => Some((*source, *amount)),
            _ => None,
        })
        .collect()
}

#[test]
fn warstorm_surge_makes_creatures_deal_their_power() {
    let mut sc = scenario();
    let surge = sc.card(P0, Zone::Battlefield, "Warstorm Surge");
    sc.cards(P0, Zone::Battlefield, &["Forest"; 3]);
    sc.card(P0, Zone::Hand, "Regal Imperiosaur"); // 5/4
    sc.card(P1, Zone::Battlefield, "Forest");
    sc.card(P1, Zone::Hand, "Llanowar Elves");
    let mut s = Script::new(sc.build());
    s.cast("Regal Imperiosaur").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::ChooseTargets { slot: 0 });
    s.target_player(P1);
    resolve_all(&mut s);
    assert_eq!(life(&s, P1), 35);
    // "It deals damage": the creature is the source, not the enchantment.
    let dino = named(&s, "Regal Imperiosaur")[0];
    assert_eq!(damage_dealt(&s), [(dino, 5)]);
    assert_ne!(dino, surge);
    assert_eq!(triggers(&s), 1);
}

#[test]
fn scourge_of_valkas_burns_for_each_dragon() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 8]);
    sc.card(P0, Zone::Battlefield, "Test Wyrmling"); // a Dragon already there
    sc.cards(P0, Zone::Hand, &["Scourge of Valkas", "Test Wyrmling"]);
    let mut s = Script::new(sc.build());
    // It enters: X = 2 Dragons.
    s.cast("Scourge of Valkas").pay().pass_all();
    s.target_player(P1);
    resolve_all(&mut s);
    assert_eq!(life(&s, P1), 38);
    let scourge = named(&s, "Scourge of Valkas")[0];
    assert_eq!(pt(&s, scourge), (4, 4));
    assert!(has(&s, scourge, Keyword::Flying));
    // Another Dragon: X = 3, dealt by that Dragon.
    s.cast("Test Wyrmling").pay().pass_all();
    s.target_player(P1);
    resolve_all(&mut s);
    assert_eq!(life(&s, P1), 35);
    let dealt = damage_dealt(&s);
    assert_eq!(dealt.len(), 2);
    assert_eq!(dealt[0], (scourge, 2));
    assert_ne!(dealt[1].0, scourge);
    assert_eq!(dealt[1].1, 3);
    // {R}: +1/+0 until end of turn.
    s.activate("Scourge of Valkas").pay().pass_all();
    assert_eq!(pt(&s, scourge), (5, 4));
}

#[test]
fn dragon_tempest_hastes_fliers_and_burns_for_dragons() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Dragon Tempest");
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Plains", "Mountain", "Mountain", "Forest"],
    );
    sc.cards(
        P0,
        Zone::Hand,
        &["Segovian Angel", "Test Wyrmling", "Llanowar Elves"],
    );
    let mut s = Script::new(sc.build());
    // A flier: haste, and (not a Dragon) no damage.
    s.cast("Segovian Angel").pay();
    resolve_all(&mut s);
    let angel = named(&s, "Segovian Angel")[0];
    assert!(has(&s, angel, Keyword::Haste));
    assert_eq!(triggers(&s), 1);
    // A Dragon without flying: X = 1 damage, no haste.
    s.cast("Test Wyrmling").pay().pass_all();
    s.target_player(P1);
    resolve_all(&mut s);
    let wyrmling = named(&s, "Test Wyrmling")[0];
    assert!(!has(&s, wyrmling, Keyword::Haste));
    assert_eq!(life(&s, P1), 39);
    assert_eq!(damage_dealt(&s), [(wyrmling, 1)]);
    // Neither.
    s.cast("Llanowar Elves").pay();
    resolve_all(&mut s);
    assert_eq!(triggers(&s), 2);
    drive_until(&mut s, |s| s.st.turn.active == P1);
    assert!(!has(&s, angel, Keyword::Haste), "until end of turn");
}

#[test]
fn verdant_suns_avatar_gains_life_equal_to_toughness() {
    let mut sc = scenario();
    sc.cards(P0, Zone::Battlefield, &["Forest"; 12]);
    sc.cards(P0, Zone::Hand, &["Verdant Sun's Avatar", "Test Golem"]);
    sc.card(P1, Zone::Battlefield, "Forest");
    let mut s = Script::new(sc.build());
    // "This creature or another creature you control": itself, a 5/5.
    s.cast("Verdant Sun's Avatar").pay();
    resolve_all(&mut s);
    assert_eq!(life(&s, P0), 45);
    // A 2/3.
    s.cast("Test Golem").pay();
    resolve_all(&mut s);
    assert_eq!(life(&s, P0), 48);
    assert_eq!(triggers(&s), 2);
}

#[test]
fn surrak_and_goreclaw_welcome_new_creatures() {
    let mut sc = scenario();
    let pair = sc.card(P0, Zone::Battlefield, "Surrak and Goreclaw");
    sc.card(P0, Zone::Battlefield, "Open the Graves");
    sc.card(P0, Zone::Battlefield, "Test Sprout");
    sc.cards(P0, Zone::Battlefield, &["Mountain"; 6]);
    sc.cards(P0, Zone::Hand, &["Test Golem", "Lightning Bolt"]);
    let mut s = Script::new(sc.build());
    assert_eq!(pt(&s, pair), (6, 5));
    assert!(has(&s, pair, Keyword::Trample));
    s.cast("Test Golem").pay();
    resolve_all(&mut s);
    let golem = named(&s, "Test Golem")[0];
    assert_eq!(pt(&s, golem), (3, 4), "a +1/+1 counter");
    assert!(has(&s, golem, Keyword::Haste) && has(&s, golem, Keyword::Trample));
    assert!(!has(&s, pair, Keyword::Haste));
    // A token entering (Open the Graves replaces the Sprout): "nontoken" — nothing.
    let before = triggers(&s);
    s.cast("Lightning Bolt").target("Test Sprout").pay();
    resolve_all(&mut s);
    let zombie = named(&s, "Zombie Token")[0];
    assert_eq!(pt(&s, zombie), (2, 2));
    assert!(has(&s, zombie, Keyword::Trample) && !has(&s, zombie, Keyword::Haste));
    // Only Open the Graves triggered since.
    assert_eq!(triggers(&s), before + 1);
}

/// CR 608.2h: if the creature that triggered the ability has left the battlefield, its last
/// known information is used — and it, as it last existed, is still what deals the damage.
#[test]
fn warstorm_surge_uses_last_known_power_if_the_creature_is_gone() {
    let mut sc = scenario();
    sc.card(P0, Zone::Battlefield, "Warstorm Surge");
    sc.card(P0, Zone::Battlefield, "Always Watching"); // nontoken creatures you control +1/+1
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Llanowar Elves"); // 1/1, so 2/2 here
    sc.card(P1, Zone::Battlefield, "Mountain");
    sc.card(P1, Zone::Hand, "Lightning Bolt");
    let mut s = Script::new(sc.build());
    s.cast("Llanowar Elves").pay().pass_all();
    s.target_player(P1);
    let elves = named(&s, "Llanowar Elves")[0];
    // In response, the opponent kills the Elves.
    s.pass();
    assert_eq!(s.who(), P1);
    s.cast("Lightning Bolt").target_object(elves).pay();
    resolve_all(&mut s);
    assert!(named(&s, "Llanowar Elves").is_empty());
    // Its power when it was last on the battlefield was 2.
    assert_eq!(life(&s, P1), 38);
    assert!(damage_dealt(&s).contains(&(elves, 2)));
}
