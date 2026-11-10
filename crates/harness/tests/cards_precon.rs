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
    assert!(verified >= 27);
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
                OptionDesc::TapForMana { source, mana } if *source == ready => Some(*mana),
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
