//! "As this enters, choose a creature type / a color", "the chosen type", spells that choose
//! as they resolve, changeling (D-076). Invented cards in Forge's format, compiled the way
//! real ones are, then played. Rules text: docs/rules-notes/614.12a-607.2d-chosen-type-color.md.

use engine_core::layers::chars;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{
    Agent, ChoiceKind, Event, GameConfig, ObjectId, OptionDesc, PlayerId, Primitive, Response,
};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const ELF: &str = "Name:Test Treetop Elf\nManaCost:G\nTypes:Creature Elf\nPT:1/1\nOracle:Text.";
const TALL_ELF: &str =
    "Name:Test Canopy Elf\nManaCost:1 G\nTypes:Creature Elf\nPT:2/2\nOracle:Text.";
const GOBLIN: &str =
    "Name:Test Cave Goblin\nManaCost:G\nTypes:Creature Goblin\nPT:1/1\nOracle:Text.";
const SHIFTER: &str = "Name:Test Many-Faced\nManaCost:G\nTypes:Creature Shapeshifter\nPT:1/1\n\
    K:Changeling\nOracle:Changeling (This card is every creature type.)";

const CHOOSE_CT: &str = "K:ETBReplacement:Other:ChooseCT\n\
    SVar:ChooseCT:DB$ ChooseType | Defined$ You | Type$ Creature | AILogic$ MostProminentInComputerDeck | SpellDescription$ As CARDNAME enters, choose a creature type.";

/// "Creatures you control of the chosen type get +1/+1."
fn banner(name: &str) -> String {
    format!(
        "Name:{name}\nManaCost:3\nTypes:Artifact\n{CHOOSE_CT}\n\
         S:Mode$ Continuous | Affected$ Creature.ChosenType+YouCtrl | AddPower$ 1 | AddToughness$ 1 | Description$ Text.\n\
         Oracle:As this artifact enters, choose a creature type.\\nCreatures you control of the chosen type get +1/+1."
    )
}

fn scenario(cards: &[&str]) -> Scenario {
    let mut all = cards.to_vec();
    all.extend([ELF, TALL_ELF, GOBLIN, SHIFTER]);
    let mut sc = Scenario::new(GameConfig::commander(2), forge_test_catalog(&all, &[]), 31);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc
}

fn on(s: &Script, owner: PlayerId, name: &str) -> ObjectId {
    s.find(owner, Zone::Battlefield, name)
        .unwrap_or_else(|| panic!("{name} is on the battlefield"))
}

fn pt(s: &Script, id: ObjectId) -> (i32, i32) {
    let c = chars(&s.st, id);
    (c.power.unwrap(), c.toughness.unwrap())
}

fn choose(s: &mut Script, ty: &str) {
    assert!(
        matches!(s.kind(), ChoiceKind::Choose { .. }),
        "asked to choose: {:?}",
        s.kind()
    );
    let ty = ty.to_string();
    s.pick(
        &format!("choose {ty}"),
        move |_, o| matches!(o, OptionDesc::CreatureType { name } if name.name() == ty),
    );
}

fn chosen_type(s: &Script, id: ObjectId) -> Option<&'static str> {
    s.st.object(id).chosen.iter().find_map(|c| match c {
        card_model::Chosen::CreatureType(t) => Some(t.name()),
        card_model::Chosen::Color(_) => None,
    })
}

// ---------------------------------------------------------------------------------------
// "As this enters, choose a creature type."

// CR 614.12a (2026-11-06): "If a replacement effect that modifies how a permanent enters
// the battlefield requires a choice, that choice is made before the permanent enters the
// battlefield." CR 603.6d: a static ability, not a trigger. CR 205.3m: any creature type
// may be chosen — the ones among the chooser's own cards first, most common first.
#[test]
fn cr_614_12a_a_creature_type_is_chosen_as_it_enters() {
    let b = banner("Test Kin Banner");
    let mut sc = scenario(&[&b]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 4]);
    let elves = sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Treetop Elf", "Test Treetop Elf"],
    );
    let theirs = sc.card(P1, Zone::Battlefield, "Test Treetop Elf");
    sc.cards(P0, Zone::Hand, &["Test Kin Banner", "Test Cave Goblin"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Banner").pay().pass_all();
    // Asked while it resolves, before it is on the battlefield, by its controller.
    let ChoiceKind::Choose { what, ability, .. } = s.kind() else {
        panic!("asked {:?}", s.kind())
    };
    assert_eq!(what, card_model::ChoiceOf::CreatureType);
    assert!(ability.is_some(), "an as-enters choice names its ability");
    assert_eq!(s.who(), P0);
    assert!(s.find(P0, Zone::Battlefield, "Test Kin Banner").is_none());
    assert_eq!(s.decision().prim, Primitive::PickN { min: 1, max: 1 });
    let names: Vec<&str> = s
        .options()
        .iter()
        .map(|o| match o {
            OptionDesc::CreatureType { name } => name.name(),
            other => panic!("not a creature type: {other:?}"),
        })
        .collect();
    assert_eq!(names.len(), card_model::CREATURE_TYPES.len());
    // Two Elves and a Goblin among P0's cards; then the rest alphabetically.
    assert_eq!(&names[..4], ["Elf", "Goblin", "Advisor", "Aenar"]);
    choose(&mut s, "Elf");
    let banner = on(&s, P0, "Test Kin Banner");
    assert_eq!(chosen_type(&s, banner), Some("Elf"));
    assert_eq!(
        s.count(|e| matches!(e, Event::Chose { player, object, .. } if *player == P0 && *object == banner)),
        1
    );
    for &e in &elves {
        assert_eq!(pt(&s, e), (2, 2));
    }
    assert_eq!(pt(&s, theirs), (1, 1), "not one P0 controls");
    s.cast("Test Cave Goblin").pay().pass_all();
    assert_eq!(pt(&s, on(&s, P0, "Test Cave Goblin")), (1, 1));
    // Public (CR 614.12a): every player's view shows it.
    let view = engine_core::view::player_view(&s.st, P1);
    let o = view.battlefield.iter().find(|o| o.id == banner).unwrap();
    assert_eq!(o.chosen.len(), 1);
}

// The order of the options comes from the chooser's own cards, hand and library included
// (D-014: hidden information). Only the chooser sees the decision; the events say only what
// was chosen, once it is.
#[test]
fn the_order_of_the_options_is_seen_by_the_chooser_only() {
    let b = banner("Test Kin Banner");
    let mut sc = scenario(&[&b]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 3]);
    for _ in 0..3 {
        sc.card(P0, Zone::Library, "Test Cave Goblin");
    }
    sc.card(P0, Zone::Hand, "Test Kin Banner");
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Banner").pay().pass_all();
    assert!(matches!(s.options()[0], OptionDesc::CreatureType { name } if name.name() == "Goblin"));
    let mine = engine_core::view::player_view(&s.st, P0);
    assert!(mine.decision.is_some());
    let theirs = engine_core::view::player_view(&s.st, P1);
    assert!(
        theirs.decision.is_none(),
        "another player doesn't see the options"
    );
    let before = s.events.len();
    choose(&mut s, "Elf");
    // What everyone learns: the choice itself (CR 614.12a), not the list it was made from.
    let shown: Vec<String> = s.events[before..]
        .iter()
        .map(|e| format!("{e:?}"))
        .filter(|e| e.contains("Goblin"))
        .collect();
    assert!(shown.is_empty(), "{shown:?}");
}

// CR 607.2d: "the chosen type" of each permanent is its own choice. CR 702.73a: a
// changeling is every creature type, so both apply to it.
#[test]
fn cr_607_2d_two_permanents_with_different_choices() {
    let b = banner("Test Kin Banner");
    let mut sc = scenario(&[&b]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 6]);
    let elf = sc.card(P0, Zone::Battlefield, "Test Treetop Elf");
    let goblin = sc.card(P0, Zone::Battlefield, "Test Cave Goblin");
    let shifter = sc.card(P0, Zone::Battlefield, "Test Many-Faced");
    sc.cards(P0, Zone::Hand, &["Test Kin Banner", "Test Kin Banner"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Banner").pay().pass_all();
    choose(&mut s, "Elf");
    s.cast("Test Kin Banner").pay().pass_all();
    choose(&mut s, "Goblin");
    assert_eq!(pt(&s, elf), (2, 2));
    assert_eq!(pt(&s, goblin), (2, 2));
    assert_eq!(pt(&s, shifter), (3, 3), "every creature type: both");
}

// CR 400.7: a permanent that leaves the battlefield and returns is a new object with no
// choice; CR 614.12a: it chooses again as it enters again.
#[test]
fn cr_400_7_leaving_and_returning_chooses_again() {
    let b = banner("Test Kin Banner");
    let bounce = "Name:Test Recoil\nManaCost:U\nTypes:Instant\n\
        A:SP$ ChangeZone | ValidTgts$ Artifact | TgtPrompt$ Select target artifact | Origin$ Battlefield | Destination$ Hand | SpellDescription$ Text.\n\
        Oracle:Return target artifact to its owner's hand.";
    let mut sc = scenario(&[&b, bounce]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 6]);
    sc.card(P0, Zone::Battlefield, "Island");
    let elf = sc.card(P0, Zone::Battlefield, "Test Treetop Elf");
    let goblin = sc.card(P0, Zone::Battlefield, "Test Cave Goblin");
    sc.cards(P0, Zone::Hand, &["Test Kin Banner", "Test Recoil"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Banner").pay().pass_all();
    choose(&mut s, "Elf");
    assert_eq!(pt(&s, elf), (2, 2));
    s.cast("Test Recoil")
        .target("Test Kin Banner")
        .pay()
        .pass_all();
    let card = s.find(P0, Zone::Hand, "Test Kin Banner").expect("in hand");
    assert!(s.st.object(card).chosen.is_empty(), "a new object");
    assert_eq!(pt(&s, elf), (1, 1));
    s.cast("Test Kin Banner").pay().pass_all();
    choose(&mut s, "Goblin");
    assert_eq!(pt(&s, elf), (1, 1));
    assert_eq!(pt(&s, goblin), (2, 2));
}

// CR 614.12a: the choice is made however the permanent is put onto the battlefield — here
// by a spell's effect, from the graveyard.
#[test]
fn cr_614_12a_put_onto_the_battlefield_by_an_effect_still_chooses() {
    let b = banner("Test Kin Banner");
    let raise = "Name:Test Reassembly\nManaCost:W\nTypes:Sorcery\n\
        A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Battlefield | ValidTgts$ Artifact.YouCtrl | TgtPrompt$ Select target artifact card in your graveyard | SpellDescription$ Text.\n\
        Oracle:Return target artifact card from your graveyard to the battlefield.";
    let mut sc = scenario(&[&b, raise]);
    sc.cards(P0, Zone::Battlefield, &["Plains"]);
    let elf = sc.card(P0, Zone::Battlefield, "Test Treetop Elf");
    sc.card(P0, Zone::Graveyard, "Test Kin Banner");
    sc.card(P0, Zone::Hand, "Test Reassembly");
    let mut s = Script::new(sc.build());
    s.cast("Test Reassembly")
        .target("Test Kin Banner")
        .pay()
        .pass_all();
    assert_eq!(s.who(), P0);
    choose(&mut s, "Elf");
    assert_eq!(pt(&s, elf), (2, 2));
}

// CR 607.2d with a type-changing effect in layer 4 (613.1d): "this creature is the chosen
// type in addition to its other types" and "other creatures you control of the chosen type
// get +1/+1" — each static ability reads only its own source's choice.
#[test]
fn cr_607_2d_a_static_ability_reads_its_own_sources_choice() {
    let mimic = format!(
        "Name:Test Mimic Golem\nManaCost:3\nTypes:Artifact Creature Construct\nPT:2/2\n{CHOOSE_CT}\n\
         S:Mode$ Continuous | AffectedDefined$ Self | AddType$ ChosenType | Description$ Text.\n\
         S:Mode$ Continuous | Affected$ Creature.ChosenType+Other+YouCtrl | AddPower$ 1 | AddToughness$ 1 | Description$ Text.\n\
         Oracle:As this creature enters, choose a creature type.\\nThis creature is the chosen type in addition to its other types.\\nOther creatures you control of the chosen type get +1/+1."
    );
    let mut sc = scenario(&[&mimic]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 6]);
    let elf = sc.card(P0, Zone::Battlefield, "Test Treetop Elf");
    sc.cards(P0, Zone::Hand, &["Test Mimic Golem", "Test Mimic Golem"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Mimic Golem").pay().pass_all();
    choose(&mut s, "Elf");
    s.cast("Test Mimic Golem").pay().pass_all();
    choose(&mut s, "Goblin");
    let golems: Vec<ObjectId> =
        s.st.zones
            .battlefield
            .iter()
            .copied()
            .filter(|&id| s.name(id) == "Test Mimic Golem")
            .collect();
    let (a, b) = (golems[0], golems[1]);
    assert!(chars(&s.st, a).has_subtype("Elf") && chars(&s.st, a).has_subtype("Construct"));
    assert!(!chars(&s.st, a).has_subtype("Goblin"));
    assert!(chars(&s.st, b).has_subtype("Goblin") && !chars(&s.st, b).has_subtype("Elf"));
    assert_eq!(pt(&s, a), (2, 2), "the Goblin one doesn't pump an Elf");
    assert_eq!(pt(&s, b), (2, 2), "the Elf one doesn't pump a Goblin");
    assert_eq!(pt(&s, elf), (2, 2));
}

// CR 205.3d: "An object can't gain a subtype that doesn't correspond to one of that
// object's types": a noncreature artifact doesn't become the chosen creature type.
#[test]
fn cr_205_3d_only_a_creature_gains_the_chosen_type() {
    let idol = format!(
        "Name:Test Idol\nManaCost:2\nTypes:Artifact\n{CHOOSE_CT}\n\
         S:Mode$ Continuous | AffectedDefined$ Self | AddType$ ChosenType | Description$ Text.\n\
         Oracle:As this artifact enters, choose a creature type.\\nThis artifact is the chosen type in addition to its other types."
    );
    let mut sc = scenario(&[&idol]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 2]);
    sc.card(P0, Zone::Hand, "Test Idol");
    let mut s = Script::new(sc.build());
    s.cast("Test Idol").pay().pass_all();
    choose(&mut s, "Elf");
    let idol = on(&s, P0, "Test Idol");
    assert!(!chars(&s.st, idol).has_subtype("Elf"));
}

// ---------------------------------------------------------------------------------------
// What reads the choice: cost reductions, cast triggers, entering with counters, mana.

// CR 601.2f, 607.2d: "Creature spells you cast of the chosen type cost {1} less."
// CR 604.3 / 702.73a: a changeling card in hand is every creature type too.
#[test]
fn cost_reduction_of_the_chosen_type_and_a_changeling_in_hand() {
    let horn = format!(
        "Name:Test Kin Horn\nManaCost:3\nTypes:Artifact\n{CHOOSE_CT}\n\
         S:Mode$ ReduceCost | ValidCard$ Creature.ChosenType | Type$ Spell | Activator$ You | Amount$ 1 | Description$ Text.\n\
         Oracle:As this artifact enters, choose a creature type.\\nCreature spells you cast of the chosen type cost {{1}} less to cast."
    );
    let shifter2 = "Name:Test Wide Mimic\nManaCost:1 G\nTypes:Creature Shapeshifter\nPT:2/2\n\
        K:Changeling\nOracle:Changeling (This card is every creature type.)";
    let mut sc = scenario(&[&horn, shifter2]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 4]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Kin Horn", "Test Canopy Elf", "Test Wide Mimic"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Horn").pay().pass_all();
    choose(&mut s, "Elf");
    // One Forest left: {1}{G} costs {G}.
    assert!(s.can_cast("Test Canopy Elf"));
    s.cast("Test Canopy Elf").pay().pass_all();
    assert!(s.find(P0, Zone::Battlefield, "Test Canopy Elf").is_some());
    assert!(!s.can_cast("Test Wide Mimic"), "no mana left");
    // A new turn's mana: the changeling also costs one less (a spell, every type).
    let mut sc = scenario(&[&horn, shifter2]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 4]);
    sc.cards(P0, Zone::Hand, &["Test Kin Horn", "Test Wide Mimic"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Horn").pay().pass_all();
    choose(&mut s, "Goblin");
    assert!(s.can_cast("Test Wide Mimic"));
}

// Forge's `ReduceCost` with no `Activator$`: anyone's spells. "Creature spells of the
// chosen type cost {2} less to cast" — the opponent's too.
#[test]
fn cost_reduction_of_anyones_spells() {
    let incubator = format!(
        "Name:Test Brood Vat\nManaCost:3\nTypes:Artifact\n{CHOOSE_CT}\n\
         S:Mode$ ReduceCost | ValidCard$ Creature.ChosenType | Type$ Spell | Amount$ 1 | Description$ Text.\n\
         Oracle:As this artifact enters, choose a creature type.\\nCreature spells of the chosen type cost {{1}} less to cast."
    );
    let lib = harness::forge_test_catalog(&[&incubator], &[]);
    let id = lib.find("Test Brood Vat").unwrap();
    assert!(lib.get(id).abilities.iter().any(|a| matches!(
        a,
        card_model::AbilityDef::CostReduction { anyone: true, .. }
    )));
    let mut sc = scenario(&[&incubator]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 3]);
    sc.card(P1, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Brood Vat");
    sc.card(P1, Zone::Hand, "Test Canopy Elf");
    let mut s = Script::new(sc.build());
    s.cast("Test Brood Vat").pay().pass_all();
    choose(&mut s, "Elf");
    drive_until(&mut s, |s| {
        s.st.turn.active == P1
            && s.st.turn.step == engine_core::turn::Step::PrecombatMain
            && s.kind() == ChoiceKind::Priority
            && s.who() == P1
    });
    // One Forest: {1}{G} costs {G}.
    assert!(s.can_cast("Test Canopy Elf"));
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

// CR 603.2, 607.2d: "Whenever you cast a creature spell of the chosen type, draw a card."
#[test]
fn cast_trigger_for_spells_of_the_chosen_type() {
    let flag = format!(
        "Name:Test War Flag\nManaCost:3\nTypes:Artifact\n{CHOOSE_CT}\n\
         T:Mode$ SpellCast | ValidCard$ Creature.ChosenType | ValidActivatingPlayer$ You | Execute$ TrigDraw | TriggerZones$ Battlefield | TriggerDescription$ Text.\n\
         SVar:TrigDraw:DB$ Draw | Defined$ You\n\
         Oracle:As this artifact enters, choose a creature type.\\nWhenever you cast a creature spell of the chosen type, draw a card."
    );
    let mut sc = scenario(&[&flag]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 5]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test War Flag", "Test Treetop Elf", "Test Cave Goblin"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test War Flag").pay().pass_all();
    choose(&mut s, "Elf");
    let hand = |s: &Script| s.st.zones.hands[0].len();
    let before = hand(&s);
    s.cast("Test Treetop Elf").pay().pass_all().pass_all();
    assert_eq!(hand(&s), before - 1 + 1, "cast an Elf, drew a card");
    let before = hand(&s);
    s.cast("Test Cave Goblin").pay().pass_all();
    assert_eq!(hand(&s), before - 1, "no card for a Goblin");
}

// CR 614.1c, 614.12: "Each other creature you control of the chosen type enters with an
// additional +1/+1 counter on it."
#[test]
fn other_creatures_of_the_chosen_type_enter_with_a_counter() {
    let mimic = format!(
        "Name:Test Brass Mimic\nManaCost:2\nTypes:Artifact Creature Shapeshifter\nPT:2/1\n{CHOOSE_CT}\n\
         K:ETBReplacement:Other:AddExtraCounter:Mandatory:Battlefield:Creature.ChosenType+Other+YouCtrl\n\
         SVar:AddExtraCounter:DB$ PutCounter | ETB$ True | Defined$ ReplacedCard | CounterType$ P1P1 | CounterNum$ 1 | SpellDescription$ Text.\n\
         Oracle:As this creature enters, choose a creature type.\\nEach other creature you control of the chosen type enters with an additional +1/+1 counter on it."
    );
    let mut sc = scenario(&[&mimic]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 4]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Brass Mimic", "Test Treetop Elf", "Test Cave Goblin"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Brass Mimic").pay().pass_all();
    choose(&mut s, "Elf");
    s.cast("Test Treetop Elf").pay().pass_all();
    s.cast("Test Cave Goblin").pay().pass_all();
    assert_eq!(pt(&s, on(&s, P0, "Test Treetop Elf")), (2, 2));
    assert_eq!(pt(&s, on(&s, P0, "Test Cave Goblin")), (1, 1));
}

// CR 106.6, 607.2d: "Add one mana of any color. Spend this mana only to cast a creature
// spell of the chosen type" — the type is the one chosen for the mana's source.
#[test]
fn mana_restricted_to_creature_spells_of_the_chosen_type() {
    let pillar = format!(
        "Name:Test Kin Pillar\nManaCost:no cost\nTypes:Land\n{CHOOSE_CT}\n\
         A:AB$ Mana | Cost$ T | Produced$ Any | RestrictValid$ Spell.Creature+ChosenType | SpellDescription$ Text.\n\
         Oracle:As this land enters, choose a creature type.\\n{{T}}: Add one mana of any color. Spend this mana only to cast a creature spell of the chosen type."
    );
    let mut sc = scenario(&[&pillar]);
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Kin Pillar", "Test Treetop Elf", "Test Cave Goblin"],
    );
    let mut s = Script::new(sc.build());
    s.pick(
        "play the land",
        |s, o| matches!(o, OptionDesc::PlayLand { card } if s.name(*card) == "Test Kin Pillar"),
    );
    choose(&mut s, "Elf");
    assert!(s.can_cast("Test Treetop Elf"));
    assert!(!s.can_cast("Test Cave Goblin"), "not of the chosen type");
    s.cast("Test Treetop Elf").pay().pass_all();
    assert!(s.find(P0, Zone::Battlefield, "Test Treetop Elf").is_some());
}

// CR 608.2h, 607.2d: an activated ability refers to its source's choice also when the
// source has left before it resolves. "{T}: Look at the top three cards of your library.
// You may reveal a creature card of the chosen type from among them and put it into your
// hand."
#[test]
fn an_ability_keeps_its_sources_choice_after_the_source_left() {
    let icon = format!(
        "Name:Test Kin Totem\nManaCost:1\nTypes:Artifact\n{CHOOSE_CT}\n\
         A:AB$ Dig | Cost$ T | DigNum$ 3 | ChangeNum$ 1 | Optional$ True | ChangeValid$ Creature.ChosenType | DestinationZone$ Hand | RestRandomOrder$ True | SpellDescription$ Text.\n\
         Oracle:As this artifact enters, choose a creature type.\\n{{T}}: Look at the top three cards of your library. You may reveal a creature card of the chosen type from among them and put it into your hand. Put the rest on the bottom of your library in a random order."
    );
    let bounce = "Name:Test Recoil\nManaCost:U\nTypes:Instant\n\
        A:SP$ ChangeZone | ValidTgts$ Artifact | TgtPrompt$ Select target artifact | Origin$ Battlefield | Destination$ Hand | SpellDescription$ Text.\n\
        Oracle:Return target artifact to its owner's hand.";
    let mut sc = scenario(&[&icon, bounce]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    sc.card(P0, Zone::Library, "Test Cave Goblin");
    sc.card(P0, Zone::Library, "Test Treetop Elf");
    sc.card(P0, Zone::Hand, "Test Recoil");
    sc.card(P0, Zone::Hand, "Test Kin Totem");
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Totem").pay().pass_all();
    choose(&mut s, "Elf");
    s.activate("Test Kin Totem").pay();
    // In response, the totem goes back to its owner's hand.
    s.cast("Test Recoil")
        .target("Test Kin Totem")
        .pay()
        .pass_all();
    assert!(s.find(P0, Zone::Hand, "Test Kin Totem").is_some());
    s.pass_all();
    // Elf, Goblin, Forest on top: only the Elf may be taken.
    assert_eq!(s.kind(), ChoiceKind::ChooseCards);
    let offered: Vec<String> = s
        .options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Card { card } => Some(s.name(*card)),
            _ => None,
        })
        .collect();
    assert_eq!(offered, ["Test Treetop Elf"]);
}

// Forge: a `ChangesZone` trigger without `TriggerZones$` whose filter needs the chosen type
// can only trigger where the card has a choice — on the battlefield. "Whenever a creature
// you control of the chosen type enters or attacks, draw a card."
#[test]
fn enters_trigger_of_the_chosen_type() {
    let disc = format!(
        "Name:Test Kin Study\nManaCost:2 U\nTypes:Enchantment\n{CHOOSE_CT}\n\
         T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Creature.ChosenType+YouCtrl | Execute$ TrigDraw | TriggerDescription$ Text.\n\
         SVar:TrigDraw:DB$ Draw | Defined$ You | NumCards$ 1\n\
         Oracle:As this enchantment enters, choose a creature type.\\nWhenever a creature you control of the chosen type enters, draw a card."
    );
    let mut sc = scenario(&[&disc]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Island", "Island", "Island", "Forest", "Forest"],
    );
    sc.cards(
        P0,
        Zone::Hand,
        &["Test Kin Study", "Test Treetop Elf", "Test Cave Goblin"],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Study").pay().pass_all();
    choose(&mut s, "Goblin");
    let hand = |s: &Script| s.st.zones.hands[0].len();
    let before = hand(&s);
    s.cast("Test Treetop Elf").pay().pass_all();
    assert_eq!(hand(&s), before - 1);
    s.cast("Test Cave Goblin").pay().pass_all().pass_all();
    assert_eq!(hand(&s), before - 1, "cast a Goblin, drew a card");
}

// CR 603.2d, 607.2d: "If a triggered ability of another creature you control of the chosen
// type triggers, it triggers an additional time" (D-073's `Panharmonicon`, read with this
// permanent's choice).
#[test]
fn additional_trigger_for_creatures_of_the_chosen_type() {
    let throne = format!(
        "Name:Test Wandering Seat\nManaCost:4\nTypes:Artifact Creature Golem\nPT:4/4\n{CHOOSE_CT}\n\
         S:Mode$ Continuous | AffectedDefined$ Self | AddType$ ChosenType | Description$ Text.\n\
         S:Mode$ Panharmonicon | ValidCard$ Creature.Other+YouCtrl+ChosenType | Description$ Text.\n\
         Oracle:As this creature enters, choose a creature type.\\nThis creature is the chosen type in addition to its other types.\\nIf a triggered ability of another creature you control of the chosen type triggers, it triggers an additional time."
    );
    let greeter = |name: &str, ty: &str| {
        format!(
            "Name:{name}\nManaCost:G\nTypes:Creature {ty}\nPT:1/1\n\
             T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ Text.\n\
             SVar:TrigGain:DB$ GainLife | Defined$ You | LifeAmount$ 1\n\
             Oracle:When this creature enters, you gain 1 life."
        )
    };
    let elf = greeter("Test Greeting Elf", "Elf");
    let goblin = greeter("Test Greeting Goblin", "Goblin");
    let mut sc = scenario(&[&throne, &elf, &goblin]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 6]);
    sc.cards(
        P0,
        Zone::Hand,
        &[
            "Test Wandering Seat",
            "Test Greeting Elf",
            "Test Greeting Goblin",
        ],
    );
    let mut s = Script::new(sc.build());
    s.cast("Test Wandering Seat").pay().pass_all();
    choose(&mut s, "Elf");
    let life = s.st.player(P0).life;
    s.cast("Test Greeting Elf").pay().pass_all();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(s.st.player(P0).life, life + 2, "an Elf's trigger twice");
    s.cast("Test Greeting Goblin").pay().pass_all();
    drive_until(&mut s, |s| {
        s.st.stack.is_empty() && s.kind() == ChoiceKind::Priority
    });
    assert_eq!(s.st.player(P0).life, life + 3, "a Goblin's once");
}

// ---------------------------------------------------------------------------------------
// "Choose a creature type" as a spell resolves (CR 608.2c).

// "Choose a creature type. Destroy all creatures that aren't of the chosen type." The choice
// is the spell's; no object keeps it.
#[test]
fn cr_608_2c_a_spell_chooses_as_it_resolves() {
    let purge = "Name:Test Kin Purge\nManaCost:B\nTypes:Sorcery\n\
        A:SP$ ChooseType | Defined$ You | Type$ Creature | SubAbility$ DBDestroy | SpellDescription$ Text.\n\
        SVar:DBDestroy:DB$ DestroyAll | ValidCards$ Creature.IsNotChosenType | Zone$ Battlefield\n\
        Oracle:Choose a creature type. Destroy all creatures that aren't of the chosen type.";
    let mut sc = scenario(&[purge]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"]);
    let elf = sc.card(P0, Zone::Battlefield, "Test Treetop Elf");
    let goblin = sc.card(P1, Zone::Battlefield, "Test Cave Goblin");
    let shifter = sc.card(P1, Zone::Battlefield, "Test Many-Faced");
    sc.card(P0, Zone::Hand, "Test Kin Purge");
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Purge").pay().pass_all();
    let ChoiceKind::Choose { ability, .. } = s.kind() else {
        panic!("{:?}", s.kind())
    };
    assert_eq!(ability, None, "chosen as the spell resolves");
    choose(&mut s, "Elf");
    assert!(
        s.st.objects
            .get(&elf)
            .is_some_and(|o| o.zone == Zone::Battlefield)
    );
    assert!(s.st.objects.contains_key(&shifter), "every creature type");
    assert!(!s.st.objects.contains_key(&goblin), "destroyed");
    assert!(
        s.st.objects.values().all(|o| o.chosen.is_empty()),
        "no object keeps the spell's choice"
    );
}

// "Choose a creature type. Draw a card for each permanent you control of that type."
#[test]
fn a_spell_counts_the_chosen_type() {
    let melody = "Name:Test Kin Song\nManaCost:U\nTypes:Sorcery\n\
        A:SP$ ChooseType | Defined$ You | Type$ Creature | SubAbility$ DBDraw | SpellDescription$ Text.\n\
        SVar:DBDraw:DB$ Draw | NumCards$ X\n\
        SVar:X:Count$Valid Permanent.ChosenType+YouCtrl\n\
        Oracle:Choose a creature type. Draw a card for each permanent you control of that type.";
    let mut sc = scenario(&[melody]);
    sc.cards(P0, Zone::Battlefield, &["Island"]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &[
            "Test Treetop Elf",
            "Test Treetop Elf",
            "Test Cave Goblin",
            "Test Many-Faced",
        ],
    );
    sc.card(P1, Zone::Battlefield, "Test Treetop Elf");
    sc.card(P0, Zone::Hand, "Test Kin Song");
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Song").pay().pass_all();
    choose(&mut s, "Elf");
    assert_eq!(s.st.zones.hands[0].len(), 3, "two Elves and a changeling");
}

// ---------------------------------------------------------------------------------------
// "As this enters, choose a color."

// CR 105.4: "If a player is asked to choose a color, they must choose one of the five
// colors." "Creatures you control of the chosen color get +1/+1."
#[test]
fn cr_105_4_a_color_is_chosen_as_it_enters() {
    let hall = "Name:Test Hue Hall\nManaCost:3\nTypes:Artifact\n\
        K:ETBReplacement:Other:ChooseColor\n\
        SVar:ChooseColor:DB$ ChooseColor | Defined$ You | AILogic$ MostProminentInComputerDeck | SpellDescription$ As CARDNAME enters, choose a color.\n\
        S:Mode$ Continuous | Affected$ Creature.ChosenColor+YouCtrl | AddPower$ 1 | AddToughness$ 1 | Description$ Text.\n\
        Oracle:As this artifact enters, choose a color.\\nCreatures you control of the chosen color get +1/+1.";
    let red = "Name:Test Ember Imp\nManaCost:R\nTypes:Creature Imp\nPT:1/1\nOracle:Text.";
    let mut sc = scenario(&[hall, red]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 3]);
    let elf = sc.card(P0, Zone::Battlefield, "Test Treetop Elf");
    let imp = sc.card(P0, Zone::Battlefield, "Test Ember Imp");
    sc.card(P0, Zone::Hand, "Test Hue Hall");
    let mut s = Script::new(sc.build());
    s.cast("Test Hue Hall").pay().pass_all();
    assert!(matches!(
        s.kind(),
        ChoiceKind::Choose {
            what: card_model::ChoiceOf::Color,
            ..
        }
    ));
    let colors: Vec<card_model::Color> = s
        .options()
        .iter()
        .map(|o| match o {
            OptionDesc::Color { color } => *color,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(colors, card_model::Color::ALL);
    s.pick("green", |_, o| {
        *o == OptionDesc::Color {
            color: card_model::Color::Green,
        }
    });
    assert_eq!(pt(&s, elf), (2, 2));
    assert_eq!(pt(&s, imp), (1, 1));
}

// ---------------------------------------------------------------------------------------
// Changeling (CR 702.73a).

// "Changeling is a characteristic-defining ability. 'Changeling' means 'This object is every
// creature type.' This ability works everywhere" — in the graveyard too (CR 604.3).
#[test]
fn cr_702_73a_changeling_is_every_creature_type_everywhere() {
    let raise = "Name:Test Elf Call\nManaCost:G\nTypes:Sorcery\n\
        A:SP$ ChangeZone | Origin$ Graveyard | Destination$ Hand | ValidTgts$ Elf.YouCtrl | TgtPrompt$ Select target Elf card in your graveyard | SpellDescription$ Text.\n\
        Oracle:Return target Elf card from your graveyard to your hand.";
    let mut sc = scenario(&[raise]);
    sc.cards(P0, Zone::Battlefield, &["Forest"]);
    let card = sc.card(P0, Zone::Graveyard, "Test Many-Faced");
    sc.card(P0, Zone::Hand, "Test Elf Call");
    let mut s = Script::new(sc.build());
    let c = chars(&s.st, card);
    assert!(c.has_subtype("Elf") && c.has_subtype("Time Lord") && c.has_subtype("C'tan"));
    assert!(
        !c.has_subtype("Forest") && !c.has_subtype("Equipment"),
        "creature types only"
    );
    s.cast("Test Elf Call")
        .target("Test Many-Faced")
        .pay()
        .pass_all();
    assert!(s.find(P0, Zone::Hand, "Test Many-Faced").is_some());
}

// ---------------------------------------------------------------------------------------
// Determinism: save/load and clone with choices made (and one being asked).

#[test]
fn save_load_and_clone_with_choices_made() {
    let b = banner("Test Kin Banner");
    let purge = "Name:Test Kin Purge\nManaCost:B\nTypes:Sorcery\n\
        A:SP$ ChooseType | Defined$ You | Type$ Creature | SubAbility$ DBDestroy | SpellDescription$ Text.\n\
        SVar:DBDestroy:DB$ DestroyAll | ValidCards$ Creature.IsNotChosenType | Zone$ Battlefield\n\
        Oracle:Choose a creature type. Destroy all creatures that aren't of the chosen type.";
    let mut sc = scenario(&[&b, purge]);
    sc.cards(P0, Zone::Battlefield, &["Swamp"; 5]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Treetop Elf", "Test Cave Goblin"],
    );
    sc.cards(P0, Zone::Hand, &["Test Kin Banner", "Test Kin Purge"]);
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Banner").pay().pass_all();
    choose(&mut s, "Goblin");
    s.cast("Test Kin Purge").pay().pass_all();
    // Mid-resolution, asked for the spell's type, with a permanent's choice made.
    assert!(matches!(s.kind(), ChoiceKind::Choose { .. }));
    let start = s.st.clone();
    let mut responses = Vec::new();
    let mut events = Vec::new();
    for _ in 0..300 {
        if s.next.is_none() {
            break;
        }
        let r = match &s.decision().prim {
            Primitive::PickN { min, max } => {
                Response::Pick((0..(*min).max((*max).min(1))).collect())
            }
            Primitive::PickNumber { min, .. } => Response::Number(*min),
            Primitive::Allocate { per_min, .. } => Response::Allocate(per_min.clone()),
            Primitive::Order => Response::Order((0..s.options().len()).collect()),
        };
        let out = engine_core::step(&mut s.st, r.clone()).expect("legal");
        events.extend(out.truth);
        s.next = out.next;
        responses.push(r);
    }
    assert!(responses.len() > 20);
    let json = harness::save_json(&start);
    let mut loaded = harness::load_json(&json).expect("loads");
    assert_eq!(loaded.state_hash(), start.state_hash());
    assert_eq!(harness::save_json(&loaded), json);
    let mut cloned = start.clone();
    assert_eq!(harness::feed(&mut loaded, &responses).unwrap(), events);
    assert_eq!(harness::feed(&mut cloned, &responses).unwrap(), events);
    assert_eq!(loaded.state_hash(), s.st.state_hash());
    assert_eq!(cloned.log_hash, s.st.log_hash);
}

// ---------------------------------------------------------------------------------------
// The agents and the log.

// The heuristic agent chooses the creature type most common among its own creatures; the
// narrator says what was chosen (CR 614.12a: public).
#[test]
fn heuristic_chooses_its_most_common_type_and_the_log_says_so() {
    let b = banner("Test Kin Banner");
    let mut sc = scenario(&[&b]);
    sc.cards(P0, Zone::Battlefield, &["Forest"; 3]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Test Cave Goblin", "Test Cave Goblin"],
    );
    sc.card(P0, Zone::Graveyard, "Test Cave Goblin");
    // More Elves in the library, which the agent can't see: the engine lists Elf first.
    for _ in 0..5 {
        sc.card(P0, Zone::Library, "Test Treetop Elf");
    }
    sc.card(P0, Zone::Hand, "Test Kin Banner");
    let mut s = Script::new(sc.build());
    s.cast("Test Kin Banner").pay().pass_all();
    assert!(matches!(s.options()[0], OptionDesc::CreatureType { name } if name.name() == "Elf"));
    let mut agent = ai::HeuristicAgent::new(1, s.st.catalog.clone());
    let view = engine_core::view::player_view(&s.st, P0);
    let r = agent.decide(&view, s.decision());
    let Response::Pick(i) = &r else { panic!() };
    assert!(
        matches!(s.options()[i[0]], OptionDesc::CreatureType { name } if name.name() == "Goblin")
    );
    let before = s.events.len();
    s.respond(r);
    let lines = harness::text::narrate(
        &s.st,
        &s.events[before..],
        engine_core::client::Seat::Everyone,
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("P0 chooses Goblin for Test Kin Banner")),
        "{lines:?}"
    );
}

// ---------------------------------------------------------------------------------------
// What is not compiled (CLAUDE.md invariant 15): named, never guessed.

#[test]
fn choice_forms_not_compiled_block() {
    let with = |svar: &str| {
        format!(
            "Name:Test Odd Banner\nManaCost:3\nTypes:Artifact\nK:ETBReplacement:Other:Pick\n\
             SVar:Pick:{svar}\nOracle:As this artifact enters, choose."
        )
    };
    // "Choose a basic land type", a card type: other families of types.
    assert!(
        forge_test_blockers(&with("DB$ ChooseType | Type$ Basic Land"))
            .contains(&"ChooseType:Basic Land".to_string())
    );
    // "Choose a color other than white": a restricted list.
    assert!(
        forge_test_blockers(&with("DB$ ChooseColor | Exclude$ white"))
            .contains(&"param:ETBReplacement ChooseColor.Exclude".to_string())
    );
    // "An opponent chooses".
    assert!(
        forge_test_blockers(&with("DB$ ChooseType | Type$ Creature | Defined$ Opponent"))
            .contains(&"ChooseType:chooser Opponent".to_string())
    );
    // "As this enters, choose a player": not built.
    assert!(
        forge_test_blockers(&with(
            "DB$ ChoosePlayer | Defined$ You | Choices$ Player.Opponent"
        ))
        .contains(&"kw:ETBReplacement:ChoosePlayer".to_string())
    );
    // Something more as it enters.
    assert!(
        forge_test_blockers(&with("DB$ ChooseType | Type$ Creature | SubAbility$ X"))
            .contains(&"kw:ETBReplacement:ChooseType+SubAbility".to_string())
    );
    // "Enters as a copy" compiles (D-074, tested in forge_copies.rs).
    let copy = "Name:Test Odd Copy\nManaCost:3\nTypes:Artifact\nK:ETBReplacement:Copy:DBCopy:Optional\n\
        SVar:DBCopy:DB$ Clone | Choices$ Artifact.Other\nOracle:Text.";
    assert!(forge_test_blockers(copy).is_empty());
}

#[test]
fn chosen_references_not_read_block() {
    // A permanent's ability that chooses as it resolves would leave the choice on it.
    let totem = "Name:Test Odd Totem\nManaCost:2\nTypes:Artifact\n\
         A:AB$ ChooseType | Cost$ T | Type$ Creature | SubAbility$ DBPump | SpellDescription$ Text.\n\
         SVar:DBPump:DB$ PumpAll | ValidCards$ Creature.ChosenType | NumAtt$ +1 | SpellDescription$ Text.\n\
         Oracle:{T}: Choose a creature type. Creatures of the chosen type get +1/+0 until end of turn.";
    assert!(
        forge_test_blockers(totem).contains(&"ChooseType:in an ability of a permanent".to_string())
    );
    // "When this dies, … of the chosen type": the choice isn't carried past its leaving.
    let dying = format!(
        "Name:Test Odd Martyr\nManaCost:2\nTypes:Creature Elf\nPT:1/1\n{CHOOSE_CT}\n\
         T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self | Execute$ TrigPump | TriggerDescription$ Text.\n\
         SVar:TrigPump:DB$ PumpAll | ValidCards$ Creature.ChosenType+YouCtrl | NumAtt$ +1 | NumDef$ +1\n\
         Oracle:As this creature enters, choose a creature type.\\nWhen this creature dies, creatures you control of the chosen type get +1/+1 until end of turn."
    );
    assert!(
        forge_test_blockers(&dying)
            .contains(&"chosen:in a trigger on its source leaving".to_string())
    );
    // "Not of the chosen type" in a mana restriction.
    let mana = format!(
        "Name:Test Odd Spring\nManaCost:no cost\nTypes:Land\n{CHOOSE_CT}\n\
         A:AB$ Mana | Cost$ T | Produced$ Any | RestrictValid$ Spell.Creature+IsNotChosenType | SpellDescription$ Text.\n\
         Oracle:As this land enters, choose a creature type.\\n{{T}}: Add one mana of any color. Spend this mana only to cast a creature spell that isn't of the chosen type."
    );
    assert!(forge_test_blockers(&mana).contains(&"chosen:mana restriction shape".to_string()));
    // A subtype beside "the chosen type".
    let both = format!(
        "Name:Test Odd Flag\nManaCost:3\nTypes:Artifact Creature Elf\nPT:1/1\n{CHOOSE_CT}\n\
         S:Mode$ Continuous | Affected$ Elf.ChosenType+YouCtrl | AddPower$ 1 | Description$ Text.\n\
         Oracle:As this artifact enters, choose a creature type.\\nElves you control of the chosen type get +1/+0."
    );
    assert!(
        forge_test_blockers(&both).contains(&"filter:.ChosenType with another".to_string()),
        "{:?}",
        forge_test_blockers(&both)
    );
    // "The chosen type" with no ability that chooses: not a valid card.
    let orphan = "Name:Test Odd Orphan\nManaCost:3\nTypes:Artifact\n\
        S:Mode$ Continuous | Affected$ Creature.ChosenType+YouCtrl | AddPower$ 1 | Description$ Text.\n\
        Oracle:Creatures you control of the chosen type get +1/+0.";
    assert!(
        forge_test_blockers(orphan)
            .iter()
            .any(|b| b.starts_with("invalid:")),
        "{:?}",
        forge_test_blockers(orphan)
    );
}

// CR 603.7c: a delayed triggered ability refers to what was fixed as it was created; the
// engine would read its source's choice when it triggers. Not built: named.
#[test]
fn chosen_type_in_a_delayed_trigger_blocks() {
    let later = "Name:Test Odd Echo\nManaCost:U\nTypes:Sorcery\n\
        A:SP$ ChooseType | Defined$ You | Type$ Creature | SubAbility$ DBDelay | SpellDescription$ Text.\n\
        SVar:DBDelay:DB$ DelayedTrigger | Mode$ Phase | Phase$ End of Turn | Execute$ TrigDraw | TriggerDescription$ Text.\n\
        SVar:TrigDraw:DB$ Draw | Defined$ You | NumCards$ X\n\
        SVar:X:Count$Valid Creature.ChosenType+YouCtrl\n\
        Oracle:Choose a creature type. At the beginning of the next end step, draw a card for each creature you control of the chosen type.";
    let b = forge_test_blockers(later);
    assert!(
        b.contains(&"chosen:in a delayed trigger".to_string()),
        "{b:?}"
    );
}

// The Oracle guard (D-076): a card that says "the chosen type" where the compiled card
// doesn't (or the reverse) is blocked.
#[test]
fn oracle_chosen_guard() {
    let quiet = "Name:Test Quiet Flag\nManaCost:3\nTypes:Artifact\n\
        S:Mode$ Continuous | Affected$ Creature.YouCtrl | AddPower$ 1 | Description$ Text.\n\
        Oracle:Creatures you control of the chosen type get +1/+0.";
    assert!(forge_test_blockers(quiet).contains(&"oracle:says chosen".to_string()));
    let loud = format!(
        "Name:Test Loud Flag\nManaCost:3\nTypes:Artifact\n{CHOOSE_CT}\n\
         S:Mode$ Continuous | Affected$ Creature.ChosenType+YouCtrl | AddPower$ 1 | Description$ Text.\n\
         Oracle:Creatures you control get +1/+0."
    );
    assert!(forge_test_blockers(&loud).contains(&"oracle:says chosen".to_string()));
}
