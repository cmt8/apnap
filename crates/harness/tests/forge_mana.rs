//! Mana abilities beyond "{T}: Add one mana" (D-066), compiled from invented cards in
//! Forge's script format and played: several mana at once, mana abilities that cost mana,
//! life or a sacrifice, that do something else as well, that count, and that make what
//! another permanent could make. Rules text:
//! docs/rules-notes/605-mana-abilities-costs-and-yields.md.

use ai::HeuristicAgent;
use engine_core::mana::ManaType;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{Agent, ChoiceKind, GameConfig, ObjectId, OptionDesc, PlayerId, Response};
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

fn pool(s: &Script, t: ManaType) -> u32 {
    s.st.player(P0).mana_pool.amounts[t.index()]
}

fn life(s: &Script, p: PlayerId) -> i32 {
    s.st.player(p).life
}

fn zone(s: &Script, id: ObjectId) -> Option<Zone> {
    s.st.objects.get(&id).map(|o| o.zone)
}

fn tapped(s: &Script, id: ObjectId) -> bool {
    s.st.object(id).tapped
}

/// The mana `source` is offered for now.
fn offered(s: &Script, source: ObjectId) -> Vec<ManaType> {
    s.options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::TapForMana {
                source: x, mana, ..
            } if *x == source => Some(*mana),
            _ => None,
        })
        .collect()
}

fn pick_card(s: &mut Script, id: ObjectId) {
    s.pick(
        "a card",
        |_, o| matches!(o, OptionDesc::Card { card } if *card == id),
    );
}

fn pick_mana(s: &mut Script, t: ManaType) {
    s.pick(
        "a mana type",
        |_, o| matches!(o, OptionDesc::Mana { mana } if *mana == t),
    );
}

fn creature(name: &str, cost: &str) -> String {
    format!("Name:{name}\nManaCost:{cost}\nTypes:Creature Human\nPT:2/2\nOracle:")
}

const BEAR: &str = "Name:Test Cub\nManaCost:G\nTypes:Creature Bear\nPT:1/1\nOracle:";

// CR 106.3 / 605.1a (2026-11-06): one activation can add mana of different types.
// Forge: `Produced$ G U` (letters separated by spaces: exactly that mana).
#[test]
fn one_activation_adds_two_different_mana() {
    let chamber = "Name:Test Chamber\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ G U | SpellDescription$ Add {G}{U}.\n\
        Oracle:{T}: Add {G}{U}.";
    let duo = creature("Test Duo", "G U");
    let mut sc = scenario(&[chamber, &duo]);
    let land = sc.card(P0, Zone::Battlefield, "Test Chamber");
    sc.card(P0, Zone::Hand, "Test Duo");
    let mut s = Script::new(sc.build());
    assert!(s.can_cast("Test Duo"), "one land pays {{G}}{{U}}");
    assert_eq!(offered(&s, land), [ManaType::Green]);
    s.tap("Test Chamber", ManaType::Green);
    assert_eq!(
        (pool(&s, ManaType::Green), pool(&s, ManaType::Blue)),
        (1, 1)
    );
    assert!(tapped(&s, land));
    s.cast("Test Duo").pay().pass_all();
    let duo = s.find(P0, Zone::Battlefield, "Test Duo");
    assert!(duo.is_some());
    let text = card_model::describe::ability_text(
        s.st.catalog.get(s.st.object(land).card),
        &s.st.catalog.get(s.st.object(land).card).abilities[0],
    );
    assert_eq!(text, "{T}: Add {G}{U}.");
}

// CR 601.2g / 605.3a: a mana ability's own mana cost is paid like any cost: from the
// pool, after other mana abilities. Forge: `Cost$ 1 T | Produced$ U B` (a Signet).
#[test]
fn a_mana_ability_that_costs_mana() {
    let signet = "Name:Test Signet\nManaCost:2\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ 1 T | Produced$ U B | SpellDescription$ Add {U}{B}.\n\
        Oracle:{1}, {T}: Add {U}{B}.";
    let duo = creature("Test Duo", "U B");
    // Alone it can't pay for itself.
    let mut sc = scenario(&[signet, &duo]);
    sc.card(P0, Zone::Battlefield, "Test Signet");
    sc.card(P0, Zone::Hand, "Test Duo");
    let s = Script::new(sc.build());
    assert!(!s.can_cast("Test Duo"));
    assert!(
        s.options()
            .iter()
            .all(|o| !matches!(o, OptionDesc::TapForMana { .. })),
        "nothing in the pool to pay {{1}} with"
    );
    // With a Forest: the Forest pays {1}, the Signet makes {U}{B}.
    let mut sc = scenario(&[signet, &duo]);
    let forest = sc.card(P0, Zone::Battlefield, "Forest");
    let rock = sc.card(P0, Zone::Battlefield, "Test Signet");
    sc.card(P0, Zone::Hand, "Test Duo");
    let mut s = Script::new(sc.build());
    assert!(s.can_cast("Test Duo"));
    s.cast("Test Duo");
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    assert_eq!(offered(&s, rock), [] as [ManaType; 0], "the pool is empty");
    assert_eq!(offered(&s, forest), [ManaType::Green]);
    s.tap("Forest", ManaType::Green);
    assert_eq!(offered(&s, rock), [ManaType::Blue]);
    s.tap("Test Signet", ManaType::Blue);
    s.pass_all();
    assert!(s.find(P0, Zone::Battlefield, "Test Duo").is_some());
    assert!(tapped(&s, forest) && tapped(&s, rock));
    assert_eq!(s.st.player(P0).mana_pool.total(), 0);
}

// CR 605.3b: a mana ability resolves at once, all of it: the pain land's damage is dealt
// as the mana is added, without the stack. Forge: `SubAbility$ DBPain` with
// `DB$ DealDamage | NumDmg$ 1 | Defined$ You`; `DB$ GainLife | LifeAmount$ 2`.
#[test]
fn what_a_mana_ability_also_does_happens_at_once() {
    let reef = "Name:Test Reef\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ C | SpellDescription$ Add {C}.\n\
        A:AB$ Mana | Cost$ T | Produced$ Combo U R | SubAbility$ DBPain | SpellDescription$ Add {U} or {R}. CARDNAME deals 1 damage to you.\n\
        SVar:DBPain:DB$ DealDamage | NumDmg$ 1 | Defined$ You\n\
        Oracle:{T}: Add {C}.\\n{T}: Add {U} or {R}. Test Reef deals 1 damage to you.";
    let tomb = "Name:Test Tomb\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ C | Amount$ 2 | SubAbility$ DBPain | SpellDescription$ Add {C}{C}. CARDNAME deals 2 damage to you.\n\
        SVar:DBPain:DB$ DealDamage | NumDmg$ 2 | Defined$ You\n\
        Oracle:{T}: Add {C}{C}. Test Tomb deals 2 damage to you.";
    let henge = "Name:Test Henge\nManaCost:5\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ T | Produced$ G | Amount$ 2 | SubAbility$ DBGainLife | SpellDescription$ Add {G}{G}. You gain 2 life.\n\
        SVar:DBGainLife:DB$ GainLife | LifeAmount$ 2\n\
        Oracle:{T}: Add {G}{G}. You gain 2 life.";
    let mut sc = scenario(&[reef, tomb, henge]);
    let reefs = sc.cards(P0, Zone::Battlefield, &["Test Reef", "Test Reef"]);
    sc.card(P0, Zone::Battlefield, "Test Tomb");
    sc.card(P0, Zone::Battlefield, "Test Henge");
    let mut s = Script::new(sc.build());
    assert_eq!(
        offered(&s, reefs[0]),
        [ManaType::Colorless, ManaType::Blue, ManaType::Red]
    );
    s.tap("Test Reef", ManaType::Blue);
    assert_eq!(life(&s, P0), 39, "1 damage");
    assert_eq!(pool(&s, ManaType::Blue), 1);
    assert!(s.st.stack.is_empty(), "CR 605.3b: no stack");
    assert_eq!(s.kind(), ChoiceKind::Priority, "P0 keeps priority");
    s.tap("Test Reef", ManaType::Colorless);
    assert_eq!(life(&s, P0), 39, "{{C}} does no damage");
    s.tap("Test Tomb", ManaType::Colorless);
    assert_eq!((life(&s, P0), pool(&s, ManaType::Colorless)), (37, 3));
    s.tap("Test Henge", ManaType::Green);
    assert_eq!((life(&s, P0), pool(&s, ManaType::Green)), (39, 2));
    let def = s.st.catalog.get(s.st.object(reefs[0]).card);
    assert_eq!(
        card_model::describe::ability_text(def, &def.abilities[1]),
        "{T}: Add {U} or {R}. This deals 1 damage to you."
    );
}

// CR 106.1b, 605.3b: "Add {G} for each creature you control" counts as it resolves.
// Forge: `Amount$ X` with `SVar:X:Count$Valid Creature.YouCtrl`; `Produced$ Any` with a
// counted amount is "X mana of any one color".
#[test]
fn counted_mana() {
    let cradle = "Name:Test Cradle\nManaCost:no cost\nTypes:Legendary Land\n\
        A:AB$ Mana | Cost$ T | Produced$ G | Amount$ X | SpellDescription$ Add {G} for each creature you control.\n\
        SVar:X:Count$Valid Creature.YouCtrl\n\
        Oracle:{T}: Add {G} for each creature you control.";
    let weaver = "Name:Test Weaver\nManaCost:1 G\nTypes:Enchantment Creature Dryad\nPT:2/2\n\
        A:AB$ Mana | Cost$ T | Produced$ Any | Amount$ X | SpellDescription$ Add X mana of any one color, where X is the number of enchantments you control.\n\
        SVar:X:Count$Valid Enchantment.YouCtrl\n\
        Oracle:{T}: Add X mana of any one color, where X is the number of enchantments you control.";
    let mut sc = scenario(&[cradle, weaver, BEAR]);
    let land = sc.card(P0, Zone::Battlefield, "Test Cradle");
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Cub", "Test Cub", "Test Weaver"],
    );
    let mut s = Script::new(sc.build());
    s.tap("Test Cradle", ManaType::Green);
    assert_eq!(pool(&s, ManaType::Green), 3, "three creatures");
    assert!(tapped(&s, land));
    s.tap("Test Weaver", ManaType::Red);
    assert_eq!(pool(&s, ManaType::Red), 1, "one enchantment");
}

// CR 601.2g: Cabal Coffers' shape — `{2}, {T}: Add {B} for each [land] you control` —
// pays its {2} from two lands and still counts them.
#[test]
fn counted_mana_that_costs_mana_pays_for_a_spell() {
    let coffers = "Name:Test Coffers\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ 2 T | Produced$ B | Amount$ X | SpellDescription$ Add {B} for each land you control.\n\
        SVar:X:Count$Valid Land.YouCtrl\n\
        Oracle:{2}, {T}: Add {B} for each land you control.";
    let horror = creature("Test Horror", "B B B B B");
    let mut sc = scenario(&[coffers, &horror]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Swamp", "Swamp", "Swamp", "Test Coffers"],
    );
    sc.card(P0, Zone::Hand, "Test Horror");
    let mut s = Script::new(sc.build());
    assert!(
        s.can_cast("Test Horror"),
        "two Swamps pay {{2}}: four {{B}}, and one more"
    );
    s.cast("Test Horror");
    s.tap("Swamp", ManaType::Black)
        .tap("Swamp", ManaType::Black);
    s.tap("Test Coffers", ManaType::Black);
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    s.pay().pass_all();
    assert!(s.find(P0, Zone::Battlefield, "Test Horror").is_some());
}

// "Add two mana in any combination of {R} and/or {G}": each mana's type is chosen.
// Forge: `Produced$ Combo R G | Amount$ 2`.
#[test]
fn mana_in_any_combination() {
    let fountain = "Name:Test Fountain\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ Combo R G | Amount$ 2 | SpellDescription$ Add two mana in any combination of {R} and/or {G}.\n\
        Oracle:{T}: Add two mana in any combination of {R} and/or {G}.";
    let duo = creature("Test Duo", "R G");
    let mut sc = scenario(&[fountain, &duo]);
    sc.card(P0, Zone::Battlefield, "Test Fountain");
    sc.card(P0, Zone::Hand, "Test Duo");
    let mut s = Script::new(sc.build());
    s.tap("Test Fountain", ManaType::Red);
    assert_eq!(s.kind(), ChoiceKind::ChooseColor, "the second mana's type");
    pick_mana(&mut s, ManaType::Green);
    assert_eq!((pool(&s, ManaType::Red), pool(&s, ManaType::Green)), (1, 1));
    s.cast("Test Duo").pay().pass_all();
    assert!(s.find(P0, Zone::Battlefield, "Test Duo").is_some());
    // While paying, only a choice that keeps the cost payable is offered.
    let mut sc = scenario(&[fountain, &duo]);
    sc.card(P0, Zone::Battlefield, "Test Fountain");
    sc.card(P0, Zone::Hand, "Test Duo");
    let mut s = Script::new(sc.build());
    s.cast("Test Duo").tap("Test Fountain", ManaType::Red);
    assert!(
        s.next
            .as_ref()
            .is_none_or(|d| d.kind != ChoiceKind::ChooseColor),
        "{{G}} is forced"
    );
    s.pass_all();
    assert!(s.find(P0, Zone::Battlefield, "Test Duo").is_some());
}

// CR 119.4, 118.3b: paying life is part of the mana ability's cost.
// Forge: `Cost$ T PayLife<1>`.
#[test]
fn a_mana_ability_that_costs_life() {
    let confluence = "Name:Test Confluence\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T PayLife<1> | Produced$ Any | Amount$ 1 | SpellDescription$ Add one mana of any color.\n\
        Oracle:{T}, Pay 1 life: Add one mana of any color.";
    let mut sc = scenario(&[confluence]);
    sc.card(P0, Zone::Battlefield, "Test Confluence");
    let mut s = Script::new(sc.build());
    s.tap("Test Confluence", ManaType::White);
    assert_eq!((life(&s, P0), pool(&s, ManaType::White)), (39, 1));
    // CR 119.4: with less life than it asks for, it can't be activated.
    let dear = confluence.replace("PayLife<1>", "PayLife<2>");
    let mut sc = scenario(&[&dear]);
    sc.card(P0, Zone::Battlefield, "Test Confluence");
    sc.life(P0, 1);
    let s = Script::new(sc.build());
    assert!(
        s.options()
            .iter()
            .all(|o| !matches!(o, OptionDesc::TapForMana { .. }))
    );
}

// CR 601.2g–h, 701.21a: "Sacrifice a creature: Add {C}{C}" — the creature is chosen as the
// ability is activated. A mana creature can be tapped for mana first and then be
// sacrificed; sacrificing it first would leave the cost unpayable, so that isn't offered
// (D-019). Forge: `Cost$ Sac<1/Creature>`.
#[test]
fn sacrificing_a_creature_for_mana() {
    let altar = "Name:Test Altar\nManaCost:3\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ Sac<1/Creature> | Produced$ C | Amount$ 2 | SpellDescription$ Add {C}{C}.\n\
        Oracle:Sacrifice a creature: Add {C}{C}.";
    let mystic = "Name:Test Mystic\nManaCost:G\nTypes:Creature Elf\nPT:1/1\n\
        A:AB$ Mana | Cost$ T | Produced$ G | SpellDescription$ Add {G}.\n\
        Oracle:{T}: Add {G}.";
    let hulk = creature("Test Hulk", "2 G");
    let mut sc = scenario(&[altar, mystic, &hulk]);
    let rock = sc.card(P0, Zone::Battlefield, "Test Altar");
    let elf = sc.card(P0, Zone::Battlefield, "Test Mystic");
    sc.card(P0, Zone::Hand, "Test Hulk");
    let mut s = Script::new(sc.build());
    assert!(
        s.can_cast("Test Hulk"),
        "{{G}} from the elf, {{C}}{{C}} from sacrificing it"
    );
    s.cast("Test Hulk");
    assert_eq!(
        offered(&s, rock),
        [] as [ManaType; 0],
        "not before the elf is tapped"
    );
    s.tap("Test Mystic", ManaType::Green);
    assert_eq!(offered(&s, rock), [ManaType::Colorless]);
    s.tap("Test Altar", ManaType::Colorless);
    assert_eq!(s.kind(), ChoiceKind::Sacrifice);
    pick_card(&mut s, elf);
    s.pass_all();
    assert_eq!(zone(&s, elf), None, "the elf left the battlefield");
    assert!(s.find(P0, Zone::Graveyard, "Test Mystic").is_some());
    assert!(s.find(P0, Zone::Battlefield, "Test Hulk").is_some());
    // At priority it may be activated as often as there are creatures.
    let mut sc = scenario(&[altar, BEAR]);
    sc.card(P0, Zone::Battlefield, "Test Altar");
    sc.cards(P0, Zone::Battlefield, &["Test Cub", "Test Cub"]);
    let mut s = Script::new(sc.build());
    for n in 1..=2 {
        s.tap("Test Altar", ManaType::Colorless);
        let cub = s.find(P0, Zone::Battlefield, "Test Cub").unwrap();
        pick_card(&mut s, cub);
        assert_eq!(pool(&s, ManaType::Colorless), 2 * n);
    }
    assert!(
        s.options()
            .iter()
            .all(|o| !matches!(o, OptionDesc::TapForMana { .. }))
    );
}

// The cost's own "sacrifice a creature" (CR 601.2h, chosen after the mana) and a mana
// ability that sacrifices a creature need different creatures.
#[test]
fn a_sacrifice_for_mana_leaves_one_for_the_cost() {
    let altar = "Name:Test Altar\nManaCost:3\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ Sac<1/Creature> | Produced$ C | Amount$ 2 | SpellDescription$ Add {C}{C}.\n\
        Oracle:Sacrifice a creature: Add {C}{C}.";
    let bargain = "Name:Test Bargain\nManaCost:2\nTypes:Sorcery\n\
        A:SP$ Draw | Cost$ 2 Sac<1/Creature> | NumCards$ 2 | SpellDescription$ Text.\n\
        Oracle:As an additional cost to cast this spell, sacrifice a creature.\\nDraw two cards.";
    let mut sc = scenario(&[altar, bargain, BEAR]);
    sc.card(P0, Zone::Battlefield, "Test Altar");
    sc.card(P0, Zone::Battlefield, "Test Cub");
    sc.card(P0, Zone::Hand, "Test Bargain");
    let s = Script::new(sc.build());
    assert!(!s.can_cast("Test Bargain"), "one creature can't pay both");
    let mut sc = scenario(&[altar, bargain, BEAR]);
    sc.card(P0, Zone::Battlefield, "Test Altar");
    let cubs = sc.cards(P0, Zone::Battlefield, &["Test Cub", "Test Cub"]);
    sc.card(P0, Zone::Hand, "Test Bargain");
    let mut s = Script::new(sc.build());
    assert!(s.can_cast("Test Bargain"));
    s.cast("Test Bargain")
        .tap("Test Altar", ManaType::Colorless);
    pick_card(&mut s, cubs[0]);
    assert_eq!(s.kind(), ChoiceKind::Sacrifice, "the cost's own sacrifice");
    pick_card(&mut s, cubs[1]);
    assert_eq!(s.st.zones.list(Zone::Graveyard, P0).len(), 2);
}

// `{T}, Sacrifice a creature: Add {B}{B}` (Forge: `Cost$ T Sac<1/Creature>`), and
// `{1}, {T}, Sacrifice this: Add one mana of any color` with a "when this is put into a
// graveyard" trigger (Forge: `Sac<1/CARDNAME>`).
#[test]
fn tap_and_sacrifice_costs() {
    let tower = "Name:Test Tower\nManaCost:no cost\nTypes:Legendary Land\n\
        A:AB$ Mana | Cost$ T | Produced$ C | SpellDescription$ Add {C}.\n\
        A:AB$ Mana | Cost$ T Sac<1/Creature> | Produced$ B | Amount$ 2 | SpellDescription$ Add {B}{B}.\n\
        Oracle:{T}: Add {C}.\\n{T}, Sacrifice a creature: Add {B}{B}.";
    let star = "Name:Test Star\nManaCost:1\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ 1 T Sac<1/CARDNAME> | Produced$ Any | SpellDescription$ Add one mana of any color.\n\
        T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self | Execute$ TrigDraw | TriggerDescription$ When CARDNAME is put into a graveyard from the battlefield, draw a card.\n\
        SVar:TrigDraw:DB$ Draw | Defined$ TriggeredCardController | NumCards$ 1\n\
        Oracle:{1}, {T}, Sacrifice this artifact: Add one mana of any color.\\nWhen this artifact is put into a graveyard from the battlefield, draw a card.";
    let mut sc = scenario(&[tower, star, BEAR]);
    let land = sc.card(P0, Zone::Battlefield, "Test Tower");
    let cub = sc.card(P0, Zone::Battlefield, "Test Cub");
    sc.card(P0, Zone::Battlefield, "Forest");
    let gem = sc.card(P0, Zone::Battlefield, "Test Star");
    let mut s = Script::new(sc.build());
    assert_eq!(offered(&s, land), [ManaType::Colorless, ManaType::Black]);
    s.pick("tower for BB", |_, o| {
        matches!(o, OptionDesc::TapForMana { source, mana: ManaType::Black, .. } if *source == land)
    });
    pick_card(&mut s, cub);
    assert_eq!(pool(&s, ManaType::Black), 2);
    assert_eq!(zone(&s, cub), None);
    // The Star's {1} is paid from the pool; a Black is the only mana there.
    s.tap("Forest", ManaType::Green);
    s.tap("Test Star", ManaType::Blue);
    assert_eq!(
        s.kind(),
        ChoiceKind::PayMana,
        "{{G}} or {{B}} pays the {{1}}"
    );
    pick_mana(&mut s, ManaType::Green);
    assert_eq!(zone(&s, gem), None);
    assert_eq!(
        (
            pool(&s, ManaType::Black),
            pool(&s, ManaType::Blue),
            pool(&s, ManaType::Green)
        ),
        (2, 1, 0)
    );
    // Its trigger is waiting for the next time a player would receive priority.
    let before = s.st.zones.list(Zone::Hand, P0).len();
    s.pass_all();
    assert_eq!(
        s.st.zones.list(Zone::Hand, P0).len(),
        before + 1,
        "drew a card"
    );
}

// CR 106.7: "one mana of any color that a land an opponent controls could produce" — what
// their abilities would make if they resolved, costs ignored; a permanent that reflects
// counts what it would reflect. Forge: `AB$ ManaReflected | ColorOrType$ Color |
// Valid$ Land.OppCtrl | ReflectProperty$ Produce`.
#[test]
fn mana_another_permanent_could_produce() {
    let orchard = "Name:Test Orchard\nManaCost:no cost\nTypes:Land\n\
        A:AB$ ManaReflected | Cost$ T | ColorOrType$ Color | Valid$ Land.OppCtrl | ReflectProperty$ Produce | SpellDescription$ Add one mana of any color that a land an opponent controls could produce.\n\
        Oracle:{T}: Add one mana of any color that a land an opponent controls could produce.";
    let pool_land = "Name:Test Pool\nManaCost:no cost\nTypes:Land\n\
        A:AB$ ManaReflected | Cost$ T | ColorOrType$ Type | Valid$ Land.YouCtrl | ReflectProperty$ Produce | SpellDescription$ Add one mana of any type that a land you control could produce.\n\
        Oracle:{T}: Add one mana of any type that a land you control could produce.";
    let waste = "Name:Test Waste\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ C | SpellDescription$ Add {C}.\nOracle:{T}: Add {C}.";
    let mut sc = scenario(&[orchard, pool_land, waste]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Orchard");
    sc.cards(P1, Zone::Battlefield, &["Island", "Mountain", "Test Waste"]);
    let s = Script::new(sc.build());
    assert_eq!(
        offered(&s, mine),
        [ManaType::Blue, ManaType::Red],
        "colors only"
    );
    // The opponent's own Orchard reflects P0's lands: P0's Forest makes it "could produce
    // {G}", so P0's Orchard could produce {G} too.
    let mut sc = scenario(&[orchard, pool_land, waste]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Orchard");
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.cards(P1, Zone::Battlefield, &["Island", "Test Orchard"]);
    let s = Script::new(sc.build());
    assert_eq!(offered(&s, mine), [ManaType::Blue, ManaType::Green]);
    // Nothing to reflect: nothing to add.
    let mut sc = scenario(&[orchard, pool_land, waste]);
    let mine = sc.card(P0, Zone::Battlefield, "Test Orchard");
    sc.card(P1, Zone::Battlefield, "Test Waste");
    let s = Script::new(sc.build());
    assert_eq!(
        offered(&s, mine),
        [] as [ManaType; 0],
        "{{C}} is not a color"
    );
    // "Any type" includes colorless.
    let mut sc = scenario(&[orchard, pool_land, waste]);
    let pond = sc.card(P0, Zone::Battlefield, "Test Pool");
    sc.cards(P0, Zone::Battlefield, &["Test Waste", "Swamp"]);
    let s = Script::new(sc.build());
    assert_eq!(offered(&s, pond), [ManaType::Black, ManaType::Colorless]);
}

// The heuristic agent pays with what costs nothing more before what costs life, deals
// damage or sacrifices a creature.
#[test]
fn the_heuristic_agent_taps_free_mana_first() {
    let confluence = "Name:Test Confluence\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T PayLife<1> | Produced$ Any | Amount$ 1 | SpellDescription$ Add one mana of any color.\n\
        Oracle:{T}, Pay 1 life: Add one mana of any color.";
    let altar = "Name:Test Altar\nManaCost:3\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ Sac<1/Creature> | Produced$ C | Amount$ 2 | SpellDescription$ Add {C}{C}.\n\
        Oracle:Sacrifice a creature: Add {C}{C}.";
    let hulk = creature("Test Hulk", "1 G");
    let mut sc = scenario(&[confluence, altar, BEAR, &hulk]);
    sc.card(P0, Zone::Battlefield, "Test Confluence");
    sc.card(P0, Zone::Battlefield, "Test Altar");
    sc.card(P0, Zone::Battlefield, "Test Cub");
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    sc.card(P0, Zone::Hand, "Test Hulk");
    let mut s = Script::new(sc.build());
    s.cast("Test Hulk");
    let mut ai = HeuristicAgent::new(1, s.st.catalog.clone());
    while s
        .next
        .as_ref()
        .is_some_and(|d| d.kind == ChoiceKind::PayMana)
    {
        let d = s.decision().clone();
        let r = ai.decide(&engine_core::player_view(&s.st, P0), &d);
        let Response::Pick(i) = &r else {
            panic!("{r:?}")
        };
        let name = match &d.options[i[0]] {
            OptionDesc::TapForMana { source, .. } => s.name(*source),
            other => panic!("{other:?}"),
        };
        assert_eq!(name, "Forest");
        s.respond(r);
    }
    assert_eq!(life(&s, P0), 40);
    assert!(s.find(P0, Zone::Battlefield, "Test Cub").is_some());
}

// The heuristic agent doesn't cast a spell it could only pay for by sacrificing its own
// creatures for mana (it may still use the altar once the spell is paid for otherwise).
#[test]
fn the_heuristic_agent_does_not_sacrifice_its_board_for_mana() {
    let altar = "Name:Test Altar\nManaCost:3\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ Sac<1/Creature> | Produced$ C | Amount$ 2 | SpellDescription$ Add {C}{C}.\n\
        Oracle:Sacrifice a creature: Add {C}{C}.";
    let idol = "Name:Test Idol\nManaCost:4\nTypes:Artifact\nOracle:";
    let decide = |lands: usize| {
        let mut sc = scenario(&[altar, idol, BEAR]);
        sc.card(P0, Zone::Battlefield, "Test Altar");
        sc.cards(P0, Zone::Battlefield, &["Test Cub", "Test Cub"]);
        for _ in 0..lands {
            sc.card(P0, Zone::Battlefield, "Forest");
        }
        sc.card(P0, Zone::Hand, "Test Idol");
        let s = Script::new(sc.build());
        assert!(s.can_cast("Test Idol"), "the engine offers it");
        let mut ai = HeuristicAgent::new(1, s.st.catalog.clone());
        let d = s.decision().clone();
        let r = ai.decide(&engine_core::player_view(&s.st, P0), &d);
        let Response::Pick(i) = r else { panic!() };
        matches!(d.options[i[0]], OptionDesc::CastSpell { .. })
    };
    assert!(!decide(0), "only by sacrificing both creatures");
    assert!(decide(4), "four lands pay for it");
}

// CR 107.4e: a hybrid symbol in a mana ability's cost ("{R/W}, {T}: Add {R}{R}, {R}{W},
// or {W}{W}" — a filter land) is paid with either color. Forge: `Cost$ RW T`.
#[test]
fn a_hybrid_cost_on_a_mana_ability() {
    let prairie = "Name:Test Prairie\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ C | SpellDescription$ Add {C}.\n\
        A:AB$ Mana | Cost$ RW T | Produced$ Combo R W | Amount$ 2 | SpellDescription$ Add {R}{R}, {R}{W}, or {W}{W}.\n\
        Oracle:{T}: Add {C}.\\n{R/W}, {T}: Add {R}{R}, {R}{W}, or {W}{W}.";
    let knight = creature("Test Knight", "W W");
    let mut sc = scenario(&[prairie, &knight]);
    let land = sc.card(P0, Zone::Battlefield, "Test Prairie");
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Knight");
    let mut s = Script::new(sc.build());
    assert!(s.can_cast("Test Knight"), "{{R}} pays {{R/W}}: {{W}}{{W}}");
    let def = s.st.catalog.get(s.st.object(land).card);
    assert_eq!(
        card_model::describe::ability_text(def, &def.abilities[1]),
        "{R/W}, {T}: Add 2 mana in any combination of {R} and/or {W}."
    );
    s.cast("Test Knight").tap("Mountain", ManaType::Red);
    s.pick("the filter", |_, o| {
        matches!(o, OptionDesc::TapForMana { source, mana: ManaType::White, ability: Some(_) } if *source == land)
    });
    s.pass_all();
    assert!(s.find(P0, Zone::Battlefield, "Test Knight").is_some());
    // With both colors floating, the player chooses which pays {R/W}.
    let mut sc = scenario(&[prairie, &knight]);
    sc.card(P0, Zone::Battlefield, "Test Prairie");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Plains"]);
    let mut s = Script::new(sc.build());
    s.tap("Mountain", ManaType::Red)
        .tap("Plains", ManaType::White);
    s.pick("the filter", |_, o| {
        matches!(
            o,
            OptionDesc::TapForMana {
                mana: ManaType::Red,
                ability: Some(_),
                ..
            }
        )
    });
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    assert_eq!(s.options().len(), 2);
    pick_mana(&mut s, ManaType::White);
    assert_eq!(s.kind(), ChoiceKind::ChooseColor, "the second mana");
    pick_mana(&mut s, ManaType::Red);
    assert_eq!((pool(&s, ManaType::Red), pool(&s, ManaType::White)), (3, 0));
    // A Forest can't pay {R/W}.
    let mut sc = scenario(&[prairie, &knight]);
    sc.card(P0, Zone::Battlefield, "Test Prairie");
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Knight");
    let s = Script::new(sc.build());
    assert!(!s.can_cast("Test Knight"));
    // Hybrid mana anywhere else is not compiled yet.
    let spell = "Name:Test Hybrid Spell\nManaCost:2\nTypes:Artifact\n\
        A:AB$ Draw | Cost$ RW T | NumCards$ 1 | SpellDescription$ Draw a card.\nOracle:Text.";
    assert_eq!(forge_test_blockers(spell), ["cost:RW"]);
}

// CR 118.12a, 605.3a: another player paying "unless that player pays {N}" during a
// resolution (D-068) can use any mana ability: a Signet whose {1} a land pays, or a pain
// land's {C}. The heuristic agent is asked only when the planner says it can pay; it pays,
// and with the mana that costs it nothing else.
#[test]
fn an_opponent_pays_unless_with_any_mana_ability() {
    let tithe = |n: u32| {
        format!(
            "Name:Test Tithe\nManaCost:2 U\nTypes:Enchantment\n\
             T:Mode$ SpellCast | ValidCard$ Card | ValidActivatingPlayer$ Opponent | TriggerZones$ Battlefield | Execute$ TrigDraw | TriggerDescription$ Text.\n\
             SVar:TrigDraw:DB$ Draw | Defined$ You | UnlessCost$ {n} | UnlessPayer$ TriggeredActivator | NumCards$ 1\n\
             Oracle:Text."
        )
    };
    let signet = "Name:Test Signet\nManaCost:2\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ 1 T | Produced$ U B | SpellDescription$ Add {U}{B}.\n\
        Oracle:{1}, {T}: Add {U}{B}.";
    let reef = "Name:Test Reef\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ C | SpellDescription$ Add {C}.\n\
        A:AB$ Mana | Cost$ T | Produced$ Combo U R | SubAbility$ DBPain | SpellDescription$ Add {U} or {R}. CARDNAME deals 1 damage to you.\n\
        SVar:DBPain:DB$ DealDamage | NumDmg$ 1 | Defined$ You\n\
        Oracle:{T}: Add {C}.\\n{T}: Add {U} or {R}. Test Reef deals 1 damage to you.";
    let play = |n: u32, theirs: &[&str]| {
        let tithe = tithe(n);
        let mut sc = scenario(&[&tithe, signet, reef, BEAR]);
        sc.card(P0, Zone::Battlefield, "Test Tithe");
        sc.card(P1, Zone::Battlefield, "Forest");
        sc.cards(P1, Zone::Battlefield, theirs);
        sc.card(P1, Zone::Hand, "Test Cub");
        sc.turn(P1, engine_core::turn::Step::PrecombatMain);
        let mut s = Script::new(sc.build());
        s.cast("Test Cub").tap("Forest", ManaType::Green);
        let mut ai = HeuristicAgent::new(1, s.st.catalog.clone());
        let mut asked = false;
        for _ in 0..200 {
            let Some(d) = s.next.clone() else { break };
            if d.kind == ChoiceKind::Priority && s.st.stack.is_empty() {
                break;
            }
            if d.player == P1 && d.kind != ChoiceKind::Priority {
                asked |= d.kind == ChoiceKind::PayUnlessCost;
                let r = ai.decide(&engine_core::player_view(&s.st, P1), &d);
                s.respond(r);
            } else if d.kind == ChoiceKind::Priority {
                s.pass();
            } else {
                s.respond(Response::Pick(vec![0]));
            }
        }
        let hand = s.st.zones.list(Zone::Hand, P0).len();
        (asked, hand, life(&s, P1))
    };
    // {2}: the second Forest pays the Signet's {1}, the Signet makes {U}{B}.
    assert_eq!(play(2, &["Forest", "Test Signet"]), (true, 0, 40));
    // Without the Signet one Forest can't pay {2}: not asked, P0 draws.
    assert_eq!(play(2, &["Forest"]), (false, 1, 40));
    // {1} from a pain land: its {C}, no damage.
    assert_eq!(play(1, &["Test Reef"]), (true, 0, 40));
}

// What stays blocked (D-066), each with its name.
#[test]
fn mana_abilities_that_stay_blocked() {
    let base = "Name:Test Rock\nManaCost:2\nTypes:Artifact\n";
    let block = |line: &str| forge_test_blockers(&format!("{base}{line}\nOracle:Text."));
    // CR 605.1a: drawing a card moves it from a library: not a mana ability.
    assert_eq!(
        block(
            "A:AB$ Mana | Cost$ T | Produced$ G | SubAbility$ DBDraw | SpellDescription$ T.\nSVar:DBDraw:DB$ Draw"
        ),
        ["Mana:sub Draw"]
    );
    // CR 605.5a: with a target it isn't a mana ability.
    assert_eq!(
        block(
            "A:AB$ Mana | Cost$ T | Produced$ G | SubAbility$ DBHit | SpellDescription$ T.\nSVar:DBHit:DB$ DealDamage | ValidTgts$ Any | NumDmg$ 1"
        ),
        ["Mana:sub with a target"]
    );
    assert_eq!(
        block("A:AB$ Mana | Cost$ Sac<2/Creature> | Produced$ C | SpellDescription$ T."),
        ["Mana:cost sacrifice N"]
    );
    assert_eq!(
        block(
            "A:AB$ Mana | Cost$ T Sac<1/Creature> | Produced$ G | Amount$ X | SpellDescription$ T.\nSVar:X:Count$Valid Creature.YouCtrl"
        ),
        ["Mana:counted with sacrifice"]
    );
    assert_eq!(
        block(
            "A:AB$ Mana | Cost$ T | Produced$ G | Amount$ X | SpellDescription$ T.\nSVar:X:Count$CardPower"
        ),
        ["Mana:counted"]
    );
    assert_eq!(
        block(
            "A:AB$ Mana | Cost$ T | Produced$ Special EachColorAmong_Valid Permanent.YouCtrl | SpellDescription$ T."
        ),
        ["Mana:produced"]
    );
    assert_eq!(
        block(
            "A:AB$ ManaReflected | Cost$ T | ColorOrType$ Color | Valid$ Land.OppCtrl | ReflectProperty$ Is | SpellDescription$ T."
        ),
        ["ManaReflected:Is"]
    );
    assert_eq!(
        block(
            "A:AB$ Mana | Cost$ T | Produced$ Any | TriggersWhenSpent$ TrigScry | SpellDescription$ T.\nSVar:TrigScry:Mode$ SpellCast | Execute$ DBScry\nSVar:DBScry:DB$ Scry | ScryNum$ 1"
        ),
        ["param:Mana.TriggersWhenSpent"]
    );
    // Accepted forms compile.
    assert_eq!(
        block("A:AB$ Mana | Cost$ 1 T | Produced$ W U | SpellDescription$ T."),
        [] as [&str; 0]
    );
}

// CR 106.6, 109.2: "spend this mana only to activate abilities of Dragons" is about Dragon
// permanents; "… of Dragon sources" and "… to activate an ability" also cover a card's
// ability activated from a hand (cycling). Forge: `RestrictValid$ Activated.Dragon+
// inZoneBattlefield` / `Activated.Dragon` / `Activated`.
#[test]
fn mana_for_abilities_of_permanents_or_of_any_source() {
    let mage = |name: &str, restrict: &str| {
        format!(
            "Name:{name}\nManaCost:1\nTypes:Artifact\n\
             A:AB$ Mana | Cost$ T | Produced$ C | RestrictValid$ {restrict} | SpellDescription$ Text.\n\
             Oracle:Text."
        )
    };
    let permanents = mage("Test Lair", "Activated.Dragon+inZoneBattlefield");
    let sources = mage("Test Hoard", "Activated.Dragon");
    let any = mage("Test Workshop", "Activated");
    let drake = "Name:Test Drake\nManaCost:4 R\nTypes:Creature Dragon\nPT:4/4\nK:Cycling:1\nOracle:Cycling {1}";
    let trinket = "Name:Test Trinket\nManaCost:1\nTypes:Artifact\nOracle:";
    let cards = [permanents.as_str(), &sources, &any, drake, trinket];
    for (rock, cycles) in [
        ("Test Lair", false),
        ("Test Hoard", true),
        ("Test Workshop", true),
    ] {
        let mut sc = scenario(&cards);
        sc.card(P0, Zone::Battlefield, rock);
        let card = sc.card(P0, Zone::Hand, "Test Drake");
        sc.card(P0, Zone::Hand, "Test Trinket");
        let s = Script::new(sc.build());
        let can_cycle = s
            .has(|_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == card));
        assert_eq!(can_cycle, cycles, "{rock}");
        assert!(!s.can_cast("Test Trinket"), "{rock}: not for spells");
    }
}

// CR 605.5a, 605.1b: a triggered ability that adds mana but triggers on something other
// than a mana ability (a land entering) is not a mana ability: it uses the stack, and the
// color is chosen as it resolves. CR 605.5b: a spell that adds mana is a spell.
// Forge: `DB$ Mana | Produced$ Any` in a trigger; `SP$ Mana | Produced$ R R G G`.
#[test]
fn effects_that_add_mana_of_a_chosen_color() {
    let cobra = "Name:Test Cobra\nManaCost:1 G U\nTypes:Creature Snake\nPT:2/1\n\
        T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Land.YouCtrl | TriggerZones$ Battlefield | Execute$ TrigMana | TriggerDescription$ Text.\n\
        SVar:TrigMana:DB$ Mana | Produced$ Any\n\
        Oracle:Landfall — Whenever a land you control enters, add one mana of any color.";
    let ritual = "Name:Test Rite\nManaCost:2 R\nTypes:Instant\n\
        A:SP$ Mana | Produced$ R R G G | SpellDescription$ Add {R}{R}{G}{G}.\n\
        Oracle:Add {R}{R}{G}{G}.";
    let mut sc = scenario(&[cobra, ritual]);
    sc.card(P0, Zone::Battlefield, "Test Cobra");
    sc.card(P0, Zone::Hand, "Forest");
    sc.card(P0, Zone::Hand, "Test Rite");
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    let mut s = Script::new(sc.build());
    s.pick(
        "play Forest",
        |s, o| matches!(o, OptionDesc::PlayLand { card } if s.name(*card) == "Forest"),
    );
    assert_eq!(s.st.stack.len(), 1, "the trigger uses the stack");
    s.pass_all();
    assert_eq!(s.kind(), ChoiceKind::ChooseColor);
    assert_eq!(s.options().len(), 5);
    pick_mana(&mut s, ManaType::Blue);
    assert_eq!(pool(&s, ManaType::Blue), 1);
    s.cast("Test Rite").pay().pass_all();
    assert_eq!((pool(&s, ManaType::Red), pool(&s, ManaType::Green)), (2, 2));
    let def = s.st.catalog.get(
        s.st.object(s.find(P0, Zone::Battlefield, "Test Cobra").unwrap())
            .card,
    );
    assert_eq!(
        card_model::describe::ability_text(def, &def.abilities[0]),
        "Whenever a land you control enters, you add one mana of any color."
    );
}

// Found by the soak (precon-4p, heuristic agents): mana that may only be spent on Dragons,
// made two at a time for {1}, pays a Dragon's ability. The second mana's type is chosen
// after the first; while it is open it must still count toward the cost being paid
// (D-019: the payment must not dead-end after the land pays the {1}).
#[test]
fn restricted_mana_of_open_types_still_pays() {
    let orb = "Name:Test Orb\nManaCost:1 R\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ 1 T | Produced$ Combo Any | Amount$ 2 | RestrictValid$ Spell.Dragon,Activated.Dragon+inZoneBattlefield | SpellDescription$ Text.\n\
        Oracle:{1}, {T}: Add two mana in any combination of colors. Spend this mana only to cast Dragon spells or activate abilities of Dragons.";
    let wyrm = "Name:Test Wyrm\nManaCost:3 R\nTypes:Creature Dragon\nPT:2/2\n\
        A:AB$ Pump | Cost$ 1 R | Defined$ Self | NumAtt$ +1 | SpellDescription$ Text.\n\
        Oracle:{1}{R}: This creature gets +1/+0 until end of turn.";
    let mut sc = scenario(&[orb, wyrm]);
    sc.card(P0, Zone::Battlefield, "Mountain");
    let rock = sc.card(P0, Zone::Battlefield, "Test Orb");
    let wyrm = sc.card(P0, Zone::Battlefield, "Test Wyrm");
    let mut s = Script::new(sc.build());
    s.activate("Test Wyrm");
    assert_eq!(
        offered(&s, rock),
        [] as [ManaType; 0],
        "its {{1}} isn't in the pool yet"
    );
    s.tap("Mountain", ManaType::Red);
    s.tap("Test Orb", ManaType::Red);
    assert_eq!(s.kind(), ChoiceKind::ChooseColor);
    pick_mana(&mut s, ManaType::Green);
    s.pass_all();
    assert_eq!(engine_core::layers::chars(&s.st, wyrm).power, Some(3));
}
