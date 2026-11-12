//! Renders the UI to PNG files for a visual check. Ignored by default (needs a GPU or a
//! software renderer, and downloads card images into the user's cache):
//! `APNAP_SHOT_DIR=/some/dir cargo test -p app --test screenshot -- --ignored`.
//! `APNAP_SHOT_SETUP` picks the game by its menu label (default: the first one);
//! `APNAP_SHOT_SIZE=2560x1440` sets the window size.

use app::art::Art;
use app::ui::App;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use engine_core::{Agent, PlayerId, player_view};

/// The window size: `APNAP_SHOT_SIZE=2560x1440`, default 1600x1000.
fn size() -> egui::Vec2 {
    std::env::var("APNAP_SHOT_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some(egui::vec2(w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(egui::vec2(1600.0, 1000.0))
}

/// Run frames until every requested image has arrived, then save a PNG.
fn shot(h: &mut Harness<'_, App>, path: std::path::PathBuf) {
    for _ in 0..600 {
        h.run_ok();
        if h.state().art.pending == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    h.run_ok();
    h.render().unwrap().save(path).unwrap();
}

#[test]
#[ignore = "needs a renderer and the network; writes PNGs to $APNAP_SHOT_DIR"]
fn render_menu_and_mid_game() {
    let Some(dir) = std::env::var_os("APNAP_SHOT_DIR").map(std::path::PathBuf::from) else {
        panic!("set APNAP_SHOT_DIR");
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::builder()
        .with_size(size())
        .with_max_steps(64)
        .wgpu()
        .build_ui_state(
            |ui, app: &mut App| app::ui::ui(app, ui),
            App::new(dir.join("saves"), Art::new(true), 0),
        );
    shot(&mut h, dir.join("menu.png"));
    if let Ok(setup) = std::env::var("APNAP_SHOT_SETUP") {
        h.get_by_label(&setup).click();
        h.run_ok();
    }
    h.state_mut().seed = 5;
    h.get_by_label("Start game").click();
    h.run_ok();
    // Let the heuristic AI play our seat for a while so the board fills up.
    let catalog = h.state().session.as_ref().unwrap().st.catalog.clone();
    let mut brain = ai::HeuristicAgent::new(5, catalog);
    let mut shots = 0;
    // (attacks, blocks) pictured so far.
    let mut combat_shots = (0, 0);
    for i in 0..400 {
        h.run_ok();
        let s = h.state_mut().session.as_mut().unwrap();
        let Some(d) = s.human_decision().cloned() else {
            break;
        };
        if matches!(i, 2 | 60 | 150 | 260)
            || (shots < 4 && d.kind == engine_core::ChoiceKind::DeclareBlockers)
        {
            shots += 1;
            // Point at a card so the enlarged view next to the pointer is shown too:
            // a creature on the battlefield if there is one, otherwise a card in hand.
            let s = h.state().session.as_ref().unwrap();
            let v = s.view();
            let pick = v
                .battlefield
                .iter()
                .find(|o| o.power.is_some())
                .or(v.hand.first())
                .map(|o| format!("{}  ", o.name));
            if let Some(label) = pick {
                if let Some(card) = h.query_all_by_label_contains(&label).next() {
                    card.hover();
                }
                h.run_ok();
            }
            shot(&mut h, dir.join(format!("game-{i:03}.png")));
        }
        // Combat as it looks while it is being planned on the table: everything attacks,
        // or every creature that can blocks the first attacker it could.
        let planning = matches!(
            d.kind,
            engine_core::ChoiceKind::DeclareAttackers | engine_core::ChoiceKind::DeclareBlockers
        );
        let attacking = d.kind == engine_core::ChoiceKind::DeclareAttackers;
        let wanted = if attacking {
            combat_shots.0 < 3 && i > 20
        } else {
            combat_shots.1 < 4
        };
        if planning
            && wanted
            && let Some(plan) = h.state_mut().plan.as_mut()
        {
            if plan.attacking() {
                plan.all_attack();
            } else {
                for c in plan.candidates.clone() {
                    if let Some(engine_core::OptionDesc::Block { attacker, .. }) = c.options.first()
                    {
                        plan.drop_on_creature(c.creature, *attacker);
                    }
                }
            }
            let what = if plan.attacking() { "attack" } else { "block" };
            if !plan.choices.is_empty() {
                if attacking {
                    combat_shots.0 += 1;
                } else {
                    combat_shots.1 += 1;
                }
                h.remove_cursor();
                shot(&mut h, dir.join(format!("{what}-{i:03}.png")));
            }
        }
        let s = h.state_mut().session.as_mut().unwrap();
        let r = brain.decide(&player_view(&s.st, PlayerId(0)), &d);
        s.respond(r).unwrap();
    }
    shot(&mut h, dir.join("game-end.png"));
    // Looking through a graveyard.
    let with_cards = {
        let v = h.state().session.as_ref().unwrap().view();
        v.players
            .iter()
            .position(|p| !p.graveyard.is_empty())
            .map(|i| PlayerId(i as u8))
    };
    if let Some(p) = with_cards {
        h.remove_cursor();
        h.state_mut().zone_view = Some((p, app::ui::Pile::Graveyard));
        shot(&mut h, dir.join("pile.png"));
    }
}

/// Counters on permanents (D-084): a Saga with two lore counters and a creature with a +1/+1
/// counter, each shown as a badge: `counters.png`.
#[test]
#[ignore = "needs a renderer; writes PNGs to $APNAP_SHOT_DIR"]
fn render_counters() {
    use card_model::CounterKind;
    use engine_core::testing::Scenario;
    use engine_core::zones::Zone;
    let Some(dir) = std::env::var_os("APNAP_SHOT_DIR").map(std::path::PathBuf::from) else {
        panic!("set APNAP_SHOT_DIR");
    };
    std::fs::create_dir_all(&dir).unwrap();
    let saga = "Name:Test Chronicle\nManaCost:1 W\nTypes:Enchantment Saga\n\
        K:Chapter:3:DBGain,DBGain,DBDraw\n\
        SVar:DBGain:DB$ GainLife | LifeAmount$ 2 | SpellDescription$ You gain 2 life.\n\
        SVar:DBDraw:DB$ Draw | NumCards$ 2 | SpellDescription$ Draw two cards.\n\
        Oracle:Text.";
    let (p0, p1) = (PlayerId(0), PlayerId(1));
    let mut sc = Scenario::new(
        engine_core::GameConfig::commander(2),
        harness::forge_test_catalog(&[saga], &[]),
        5,
    );
    for p in [p0, p1] {
        sc.library(p, "Forest", 20);
    }
    sc.cards(p0, Zone::Battlefield, &["Forest"; 2]);
    let chronicle = sc.card(p0, Zone::Battlefield, "Test Chronicle");
    let golem = sc.card(p0, Zone::Battlefield, "Test Golem");
    sc.turn(p0, engine_core::turn::Step::PrecombatMain);
    let mut st = sc.build();
    st.objects
        .get_mut(&chronicle)
        .unwrap()
        .counters
        .insert(CounterKind::LORE, 2);
    st.objects
        .get_mut(&golem)
        .unwrap()
        .counters
        .insert(CounterKind::PlusOne, 1);
    let mut app = App::with_saves_dir(dir.join("saves"));
    app.session = Some(app::session::Session::from_state(
        st,
        Some(p0),
        harness::Agents::Random,
    ));
    app.screen = app::ui::Screen::Game;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_max_steps(64)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app::ui::ui(app, ui), app);
    shot(&mut h, dir.join("counters.png"));
}

/// A planeswalker (D-086): its loyalty badge, and an attack planned at it (the attacker's
/// arrow points at the planeswalker): `planeswalker.png`.
#[test]
#[ignore = "needs a renderer; writes PNGs to $APNAP_SHOT_DIR"]
fn render_planeswalker() {
    use card_model::CounterKind;
    use engine_core::testing::Scenario;
    use engine_core::zones::Zone;
    let Some(dir) = std::env::var_os("APNAP_SHOT_DIR").map(std::path::PathBuf::from) else {
        panic!("set APNAP_SHOT_DIR");
    };
    std::fs::create_dir_all(&dir).unwrap();
    let walker = "Name:Test Sage Walker\nManaCost:2 W\nTypes:Legendary Planeswalker Ajani\nLoyalty:3\n\
        A:AB$ GainLife | Cost$ AddCounter<1/LOYALTY> | Planeswalker$ True | LifeAmount$ 2 | SpellDescription$ You gain 2 life.\n\
        A:AB$ DealDamage | Cost$ SubCounter<2/LOYALTY> | Planeswalker$ True | ValidTgts$ Creature | NumDmg$ 3 | SpellDescription$ CARDNAME deals 3 damage to target creature.\n\
        Oracle:Text.";
    let (p0, p1) = (PlayerId(0), PlayerId(1));
    let mut sc = Scenario::new(
        engine_core::GameConfig::commander(2),
        harness::forge_test_catalog(&[walker], &[]),
        5,
    );
    for p in [p0, p1] {
        sc.library(p, "Forest", 20);
    }
    sc.cards(p1, Zone::Battlefield, &["Plains"; 2]);
    let pw = sc.card(p1, Zone::Battlefield, "Test Sage Walker");
    let golem = sc.card(p0, Zone::Battlefield, "Test Golem");
    sc.card(p0, Zone::Battlefield, "Test Golem");
    sc.turn(p0, engine_core::turn::Step::BeginningOfCombat);
    let mut st = sc.build();
    st.objects
        .get_mut(&pw)
        .unwrap()
        .counters
        .insert(CounterKind::LOYALTY, 4);
    let mut app = App::with_saves_dir(dir.join("saves"));
    app.session = Some(app::session::Session::from_state(
        st,
        Some(p0),
        harness::Agents::Random,
    ));
    app.screen = app::ui::Screen::Game;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_max_steps(64)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app::ui::ui(app, ui), app);
    h.run_ok();
    let plan = h.state_mut().plan.as_mut().expect("planning the attack");
    assert!(plan.drop_on_planeswalker(golem, pw));
    h.remove_cursor();
    shot(&mut h, dir.join("planeswalker.png"));
}

/// Proxies (cards drawn from their definitions, with generated rules text) of a modal card
/// and of a card that makes tokens, next to each other: `proxies.png`.
#[test]
#[ignore = "needs a renderer; writes PNGs to $APNAP_SHOT_DIR"]
fn render_proxies() {
    use card_model::{
        AbilityDef, Amount, Body, CardCatalog, CardDef, CardType, Cost, Effect, Keyword, ManaCost,
        ManaProduce, ManaType, Mode, PlayerRef,
    };
    let Some(dir) = std::env::var_os("APNAP_SHOT_DIR").map(std::path::PathBuf::from) else {
        panic!("set APNAP_SHOT_DIR");
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mode = |effect| Mode {
        targets: Vec::new(),
        effect,
        groups: Vec::new(),
    };
    let draw = Effect::Draw {
        who: PlayerRef::You,
        amount: Amount::Fixed(2),
    };
    let gain = Effect::GainLife {
        who: PlayerRef::You,
        amount: Amount::Fixed(3),
    };
    let make = |key: &str, n: u32| Effect::CreateToken {
        token: key.into(),
        count: Amount::Fixed(n),
        who: PlayerRef::You,
    };
    let treasure = CardDef {
        name: "Treasure Token".into(),
        key: Some("treasure".into()),
        token: true,
        card_types: vec![CardType::Artifact],
        creature_types: vec!["Treasure".into()],
        abilities: vec![AbilityDef::Mana {
            cost: Cost {
                tap: true,
                sacrifice_self: true,
                ..Cost::default()
            },
            produce: ManaProduce::AnyColor,
            only_if: None,
            spend: None,
            then: None,
        }],
        ..CardDef::default()
    };
    let spawn = CardDef {
        name: "Eldrazi Spawn Token".into(),
        key: Some("spawn".into()),
        token: true,
        card_types: vec![CardType::Creature],
        creature_types: vec!["Eldrazi".into(), "Spawn".into()],
        power: Some(0),
        toughness: Some(1),
        colors: Some(Vec::new()),
        abilities: vec![AbilityDef::Mana {
            cost: Cost {
                sacrifice_self: true,
                ..Cost::default()
            },
            produce: ManaProduce::One(ManaType::Colorless),
            only_if: None,
            spend: None,
            then: None,
        }],
        ..CardDef::default()
    };
    let mut cat = CardCatalog::default();
    cat.add(treasure);
    cat.add(spawn);
    let charm = CardDef {
        name: "Test Charm".into(),
        mana_cost: ManaCost::parse("{1}{G}{W}").ok(),
        card_types: vec![CardType::Instant],
        abilities: vec![AbilityDef::Spell(Body::Modal {
            min: 1,
            max: 1,
            modes: vec![mode(draw), mode(gain), mode(make("treasure", 1))],
        })],
        ..CardDef::default()
    };
    let maker = CardDef {
        name: "Test Hatchery".into(),
        mana_cost: ManaCost::parse("{3}{U}").ok(),
        card_types: vec![CardType::Creature],
        creature_types: vec!["Eldrazi".into()],
        power: Some(2),
        toughness: Some(3),
        colors: Some(Vec::new()),
        keywords: vec![Keyword::Flying],
        abilities: vec![AbilityDef::Triggered {
            when: card_model::Trigger::Enters(card_model::ObjFilter::This),
            intervening_if: None,
            body: Body::Effect(mode(Effect::Seq(vec![
                make("spawn", 2),
                make("treasure", 1),
            ]))),
        }],
        ..CardDef::default()
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 680.0))
        .wgpu()
        .build_ui_state(
            |ui, (art, cards, cat): &mut (Art, Vec<CardDef>, CardCatalog)| {
                let r = ui.max_rect();
                for (i, def) in cards.iter().enumerate() {
                    let w = 440.0;
                    let rect = egui::Rect::from_min_size(
                        r.min + egui::vec2(20.0 + i as f32 * (w + 20.0), 20.0),
                        egui::vec2(w, w * 1.4),
                    );
                    app::cardview::paint_proxy(ui, art, rect, def, Some(cat));
                }
            },
            (Art::disabled(), vec![charm, maker], cat),
        );
    h.run_ok();
    h.render().unwrap().save(dir.join("proxies.png")).unwrap();
}
