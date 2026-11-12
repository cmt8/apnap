//! Cards with more than one face (decisions.md D-071): invented cards in Forge's script
//! format, compiled the way real ones are and played. Rules text:
//! docs/rules-notes/712-double-faced-cards.md, 701.27-transform.md.

use card_model::{CardType, Layout};
use engine_core::layers::chars;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, ObjectId, OptionDesc, PlayerId, Primitive, Response};
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

/// The only object of `owner` in `zone` made from the card whose front face is `front`.
fn the(s: &Script, owner: PlayerId, zone: Zone, front: &str) -> ObjectId {
    let found: Vec<ObjectId> = s
        .st
        .objects
        .values()
        .filter(|o| o.owner == owner && o.zone == zone && s.st.catalog.get(o.card).name == front)
        .map(|o| o.id)
        .collect();
    assert_eq!(found.len(), 1, "{front} in {zone:?}: {found:?}");
    found[0]
}

fn option_names(s: &Script) -> Vec<String> {
    s.options()
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Card { card } => Some(s.name(*card)),
            _ => None,
        })
        .collect()
}

// A modal double-faced card: an instant whose back face is a land that enters tapped.
const TIDE: &str = "Name:Test Tide Charm\n\
    ManaCost:1 U\n\
    Types:Instant\n\
    A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
    AlternateMode:Modal\n\
    Oracle:Draw a card.\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Tide Flat\n\
    ManaCost:no cost\n\
    Types:Land\n\
    R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplacementResult$ Updated | ReplaceWith$ ETBTapped | Description$ CARDNAME enters tapped.\n\
    SVar:ETBTapped:DB$ Tap | Defined$ Self | ETB$ True\n\
    A:AB$ Mana | Cost$ T | Produced$ U | SpellDescription$ Add {U}.\n\
    Oracle:Test Tide Flat enters tapped.\\n{T}: Add {U}.";

// A modal double-faced card whose back face is a creature spell with its own cost.
const KILN: &str = "Name:Test Kiln Blast\n\
    ManaCost:R\n\
    Types:Sorcery\n\
    A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 2 | SpellDescription$ Text.\n\
    AlternateMode:Modal\n\
    Oracle:Text.\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Kiln Golem\n\
    ManaCost:3 B\n\
    Types:Artifact Creature Golem\n\
    PT:3/3\n\
    Oracle:";

// Two land faces (a "pathway").
const PATH: &str = "Name:Test Dawn Path\n\
    ManaCost:no cost\n\
    Types:Land\n\
    A:AB$ Mana | Cost$ T | Produced$ W | SpellDescription$ Add {W}.\n\
    AlternateMode:Modal\n\
    Oracle:{T}: Add {W}.\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Dusk Path\n\
    ManaCost:no cost\n\
    Types:Land\n\
    A:AB$ Mana | Cost$ T | Produced$ B | SpellDescription$ Add {B}.\n\
    Oracle:{T}: Add {B}.";

#[test]
fn a_modal_card_compiles_with_both_faces() {
    let cat = forge_test_catalog(&[TIDE], &[]);
    let id = cat.find("Test Tide Charm").expect("the card");
    let def = cat.get(id);
    assert_eq!(def.layout, Layout::Modal);
    assert_eq!(def.faces.len(), 1);
    // The back face is an entry of its own, right after the card, and never a card.
    let back = cat.face(id, 1);
    assert_eq!(cat.get(back).name, "Test Tide Flat");
    assert_eq!(cat.get(back).face_of, Some(id));
    assert_eq!(cat.card_of(back), id);
    assert!(cat.find("Test Tide Flat").is_none());
    // CR 903.4d: both faces count for color identity ({U} on both).
    let kiln = forge_test_catalog(&[KILN], &[]);
    let k = kiln.get(kiln.find("Test Kiln Blast").unwrap());
    assert_eq!(
        card_model::color_identity(k),
        [card_model::Color::Black, card_model::Color::Red]
    );
}

// CR 712.6a (2026-11-06): in a hand a double-faced card has only its front face's
// characteristics. CR 712.8b, 712.9: either face may be cast or played as a land.
#[test]
fn a_modal_card_in_hand_offers_both_faces() {
    let mut sc = scenario(&[TIDE]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    let card = sc.card(P0, Zone::Hand, "Test Tide Charm");
    let s = Script::new(sc.build());
    let c = chars(&s.st, card);
    assert_eq!(c.name, "Test Tide Charm");
    assert_eq!(c.card_types, [CardType::Instant]);
    assert_eq!(c.mana_value(), 2);
    assert!(s.can_cast("Test Tide Charm"));
    assert!(s.has(|_, o| matches!(o, OptionDesc::PlayLandFace { card: c, face: 1 } if *c == card)));
    // The front face isn't a land: no plain land play of it.
    assert!(!s.has(|_, o| matches!(o, OptionDesc::PlayLand { .. })));
}

// CR 712.9 / 305.1: playing the land face uses the land drop; CR 712.6e: on the battlefield
// it has only that face's characteristics — its own "enters tapped" applies (CR 614.12),
// and its mana ability.
#[test]
fn playing_the_land_face_of_a_modal_card() {
    let mut sc = scenario(&[TIDE]);
    sc.cards(P0, Zone::Hand, &["Test Tide Charm", "Forest"]);
    let mut s = Script::new(sc.build());
    s.play_face("Test Tide Flat");
    let flat = the(&s, P0, Zone::Battlefield, "Test Tide Charm");
    let c = chars(&s.st, flat);
    assert_eq!(c.name, "Test Tide Flat");
    assert_eq!(c.card_types, [CardType::Land]);
    assert!(
        s.st.object(flat).tapped,
        "its own replacement: enters tapped"
    );
    // That was the land for the turn.
    assert!(!s.has(|_, o| matches!(o, OptionDesc::PlayLand { .. })));
    // The view says which face is up.
    let v = engine_core::player_view(&s.st, P1);
    let seen = v.battlefield.iter().find(|o| o.id == flat).unwrap();
    assert_eq!((seen.face, seen.name.as_str()), (1, "Test Tide Flat"));
}

// CR 712.8b–c: the back face is cast for its own cost and only it is evaluated; CR 712.6e
// on the stack, CR 712.10 it becomes a permanent with that face up. CR 712.6a: once it
// leaves the battlefield it is the front face again.
#[test]
fn casting_the_back_face_of_a_modal_card() {
    let mut sc = scenario(&[KILN]);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp", "Swamp"]);
    let card = sc.card(P0, Zone::Hand, "Test Kiln Blast");
    let mut s = Script::new(sc.build());
    // {3}{B}: three Swamps aren't enough for the Golem; {R} isn't there for the Blast.
    assert!(!s.can_cast("Test Kiln Blast"));
    assert!(!s.has(|_, o| matches!(o, OptionDesc::CastFace { .. })));
    let mut sc = scenario(&[KILN]);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp", "Swamp", "Swamp"]);
    sc.card(P0, Zone::Hand, "Test Kiln Blast");
    s = Script::new(sc.build());
    let _ = card;
    s.cast_face("Test Kiln Golem");
    let spell = the(&s, P0, Zone::Stack, "Test Kiln Blast");
    let c = chars(&s.st, spell);
    assert_eq!(c.name, "Test Kiln Golem");
    assert!(c.is(CardType::Creature) && !c.is(CardType::Sorcery));
    assert_eq!(c.mana_value(), 4, "its own mana cost (CR 712.6e)");
    s.pay().pass_all();
    let golem = the(&s, P0, Zone::Battlefield, "Test Kiln Blast");
    let c = chars(&s.st, golem);
    assert_eq!(
        (c.name.as_str(), c.power, c.toughness),
        ("Test Kiln Golem", Some(3), Some(3))
    );
    assert_eq!(s.st.object(golem).face, 1);
}

// CR 712.6a: in a graveyard (or library, or exile) only the front face's characteristics.
#[test]
fn a_modal_permanent_that_dies_is_its_front_face_again() {
    let kill = "Name:Test Ending\nManaCost:B\nTypes:Instant\n\
        A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Text.\nOracle:Text.";
    let mut sc = scenario(&[KILN, kill]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Swamp", "Swamp", "Swamp", "Swamp", "Swamp"],
    );
    sc.cards(P0, Zone::Hand, &["Test Kiln Blast", "Test Ending"]);
    let mut s = Script::new(sc.build());
    s.cast_face("Test Kiln Golem").pay().pass_all();
    s.cast("Test Ending")
        .target("Test Kiln Golem")
        .pay()
        .pass_all();
    let dead = the(&s, P0, Zone::Graveyard, "Test Kiln Blast");
    let c = chars(&s.st, dead);
    assert_eq!(c.name, "Test Kiln Blast");
    assert_eq!(c.card_types, [CardType::Sorcery]);
    assert_eq!(c.mana_value(), 1);
    assert_eq!(s.st.object(dead).face, 0);
}

// CR 712.6a: a search of a library for a land card doesn't find a card whose back face is a
// land.
#[test]
fn a_library_search_sees_only_the_front_face() {
    let search = "Name:Test Prospecting\nManaCost:G\nTypes:Sorcery\n\
        A:SP$ ChangeZone | Origin$ Library | Destination$ Hand | ChangeType$ Land | ChangeNum$ 1 | SpellDescription$ Text.\n\
        Oracle:Text.";
    let mut sc = Scenario::new(
        GameConfig::commander(2),
        forge_test_catalog(&[TIDE, search, PATH], &[]),
        31,
    );
    sc.card(P0, Zone::Library, "Test Tide Charm");
    sc.card(P0, Zone::Library, "Test Dawn Path");
    sc.card(P0, Zone::Library, "Island");
    sc.library(P1, "Forest", 5);
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Hand, "Test Prospecting");
    let mut s = Script::new(sc.build());
    s.cast("Test Prospecting").pay().pass_all();
    assert_eq!(s.kind(), ChoiceKind::SearchLibrary);
    let mut names = option_names(&s);
    names.sort();
    assert_eq!(names, ["Island", "Test Dawn Path"]);
}

// CR 712.9: a card with two land faces is played with either up; each makes its own mana.
#[test]
fn a_card_with_two_land_faces() {
    let mut sc = scenario(&[PATH]);
    sc.card(P0, Zone::Hand, "Test Dawn Path");
    let mut s = Script::new(sc.build());
    assert!(s.has(|_, o| matches!(o, OptionDesc::PlayLand { .. })));
    s.play_face("Test Dusk Path");
    let path = the(&s, P0, Zone::Battlefield, "Test Dawn Path");
    assert!(!s.st.object(path).tapped);
    assert!(s.has(|_, o| matches!(o, OptionDesc::TapForMana { source, mana: engine_core::mana::ManaType::Black, .. } if *source == path)));
    assert!(!s.has(|_, o| matches!(
        o,
        OptionDesc::TapForMana {
            mana: engine_core::mana::ManaType::White,
            ..
        }
    )));
}

// CR 712.6a: an instant cast with its front face goes to the graveyard; casting it doesn't
// use the land drop.
#[test]
fn casting_the_front_face_of_a_modal_card() {
    let mut sc = scenario(&[TIDE]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    sc.cards(P0, Zone::Hand, &["Test Tide Charm", "Forest"]);
    let mut s = Script::new(sc.build());
    let before = s.st.zones.list(Zone::Hand, P0).len();
    s.cast("Test Tide Charm").pay().pass_all();
    assert_eq!(
        s.st.zones.list(Zone::Hand, P0).len(),
        before,
        "-1 cast, +1 drawn"
    );
    the(&s, P0, Zone::Graveyard, "Test Tide Charm");
    assert!(s.has(|_, o| matches!(o, OptionDesc::PlayLand { .. })));
}

// The compiler needs every face understood: a back face it can't compile blocks the card.
#[test]
fn a_face_that_does_not_compile_blocks_the_card() {
    let odd = PATH.replace(
        "A:AB$ Mana | Cost$ T | Produced$ B | SpellDescription$ Add {B}.",
        "A:AB$ Subgame | Cost$ T | SpellDescription$ Text.",
    );
    assert_eq!(forge_test_blockers(&odd), ["api:Subgame"]);
    // Layouts without rules here stay blocked.
    for (mode, blocker) in [
        ("Flip", "layout:Flip"),
        ("Meld", "layout:Meld"),
        ("Prepare", "layout:Prepare"),
        ("Specialize", "layout:Specialize"),
    ] {
        let card = PATH.replace("AlternateMode:Modal", &format!("AlternateMode:{mode}"));
        assert_eq!(forge_test_blockers(&card), [blocker]);
    }
    // An adventurer's or omen's other face must be its Adventure or Omen spell.
    for mode in ["Adventure", "Omen"] {
        let card = PATH.replace("AlternateMode:Modal", &format!("AlternateMode:{mode}"));
        assert_eq!(
            forge_test_blockers(&card),
            [format!(
                "invalid:an {mode} card is a permanent card with an {mode} spell"
            )]
        );
    }
}

// ---------------------------------------------------------------------------------------
// Transforming double-faced cards (CR 701.27, 712.2).

// "{1}: Transform this." A 2/2 whose back face is a 4/4 flier with a color indicator, and
// whose back face would gain life as it enters.
const PUPA: &str = "Name:Test Pupa\n\
    ManaCost:G\n\
    Types:Creature Insect\n\
    PT:2/2\n\
    A:AB$ SetState | Cost$ 1 | Defined$ Self | Mode$ Transform | SpellDescription$ Transform CARDNAME.\n\
    AlternateMode:DoubleFaced\n\
    Oracle:{1}: Transform Test Pupa.\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Moth\n\
    ManaCost:no cost\n\
    Colors:white\n\
    Types:Creature Insect\n\
    PT:4/4\n\
    K:Flying\n\
    T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigGain | TriggerDescription$ When CARDNAME enters, you gain 5 life.\n\
    SVar:TrigGain:DB$ GainLife | Defined$ You | LifeAmount$ 5\n\
    Oracle:Flying\\nWhen Test Moth enters, you gain 5 life.";

// "At the beginning of your end step, if you control two or more creatures, transform it."
const RITE: &str = "Name:Test Rite\n\
    ManaCost:1 G\n\
    Types:Enchantment\n\
    T:Mode$ Phase | Phase$ End of Turn | ValidPlayer$ You | TriggerZones$ Battlefield | Execute$ DBTransform | IsPresent$ Creature.YouCtrl | PresentCompare$ GE2 | TriggerDescription$ At the beginning of your end step, if you control two or more creatures, transform CARDNAME.\n\
    SVar:DBTransform:DB$ SetState | Mode$ Transform\n\
    AlternateMode:DoubleFaced\n\
    Oracle:At the beginning of your end step, if you control two or more creatures, transform Test Rite.\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Rite Grove\n\
    ManaCost:no cost\n\
    Colors:green\n\
    Types:Land\n\
    A:AB$ Mana | Cost$ T | Produced$ G | Amount$ 2 | SpellDescription$ Add {G}{G}.\n\
    Oracle:{T}: Add {G}{G}.";

#[test]
fn a_transforming_card_compiles_and_only_its_front_face_is_cast() {
    let cat = forge_test_catalog(&[PUPA], &[]);
    let id = cat.find("Test Pupa").unwrap();
    assert_eq!(cat.get(id).layout, Layout::Transform);
    let back = cat.get(cat.face(id, 1));
    assert_eq!(back.name, "Test Moth");
    assert!(back.mana_cost.is_none());
    // CR 903.4d: the back face's color indicator counts for color identity.
    assert_eq!(
        card_model::color_identity(cat.get(id)),
        [card_model::Color::White, card_model::Color::Green]
    );
    // CR 712.8: cast with its front face up; the back face is never offered.
    let mut sc = scenario(&[PUPA]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Forest", "Forest", "Forest", "Forest"],
    );
    sc.card(P0, Zone::Hand, "Test Pupa");
    let mut s = Script::new(sc.build());
    assert!(s.can_cast("Test Pupa"));
    assert!(!s.has(|_, o| matches!(o, OptionDesc::CastFace { .. })));
    s.cast("Test Pupa").pay().pass_all();
    let pupa = the(&s, P0, Zone::Battlefield, "Test Pupa");
    assert_eq!(chars(&s.st, pupa).name, "Test Pupa");
}

// CR 701.27a, 701.27c, 712.15: transforming turns the permanent over; it is the same
// object — counters and damage stay, it doesn't "enter" (no enters trigger). CR 712.6d /
// 202.3b: its back face has the front face's mana value. CR 613.7g: a new timestamp.
#[test]
fn transforming_a_permanent_keeps_the_object() {
    let mut sc = scenario(&[PUPA]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
    let pupa = sc.card(P0, Zone::Battlefield, "Test Pupa");
    let mut s = Script::new(sc.build());
    let ts = s.st.object(pupa).timestamp;
    let life = s.st.player(P0).life;
    s.activate("Test Pupa").pay().pass_all();
    assert!(s.st.objects.contains_key(&pupa), "the same object");
    let o = s.st.object(pupa);
    assert_eq!((o.face, o.transforms), (1, 1));
    assert!(o.timestamp > ts, "a new timestamp");
    let c = chars(&s.st, pupa);
    assert_eq!(c.name, "Test Moth");
    assert_eq!((c.power, c.toughness), (Some(4), Some(4)));
    assert!(c.has(card_model::Keyword::Flying));
    assert_eq!(c.colors, [card_model::Color::White]);
    assert!(c.mana_cost.is_none());
    assert_eq!(c.mana_value(), 1, "the front face's mana value");
    assert_eq!(
        s.st.player(P0).life,
        life,
        "it didn't enter the battlefield"
    );
    assert_eq!(
        s.count(
            |e| matches!(e, engine_core::Event::Transformed { object, face: 1 } if *object == pupa)
        ),
        1
    );
    // Its back face has no transform ability: it stays a Moth.
    assert!(
        !s.has(|_, o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == pupa))
    );
    // The view shows the face that is up.
    let v = engine_core::player_view(&s.st, P1);
    let seen = v.battlefield.iter().find(|o| o.id == pupa).unwrap();
    assert_eq!((seen.face, seen.name.as_str()), (1, "Test Moth"));
}

// CR 701.27g: an ability of a permanent that transforms it does nothing if the permanent
// has transformed since the ability was put on the stack.
#[test]
fn a_permanent_transforms_once_for_two_abilities_on_the_stack() {
    let both = PUPA.replace(
        "Oracle:Flying\\nWhen Test Moth enters, you gain 5 life.",
        "A:AB$ SetState | Cost$ 1 | Defined$ Self | Mode$ Transform | SpellDescription$ Transform CARDNAME.\n\
         Oracle:Flying\\nWhen Test Moth enters, you gain 5 life.\\n{1}: Transform Test Moth.",
    );
    let mut sc = scenario(&[&both]);
    sc.cards(P0, Zone::Battlefield, &["Forest", "Forest", "Forest"]);
    let pupa = sc.card(P0, Zone::Battlefield, "Test Pupa");
    let mut s = Script::new(sc.build());
    // Activated twice, both on the stack.
    s.activate("Test Pupa").pay();
    s.activate("Test Pupa").pay();
    assert_eq!(s.st.stack.len(), 2);
    s.pass_all();
    assert_eq!(s.st.object(pupa).face, 1, "the first one transformed it");
    s.pass_all();
    assert_eq!(s.st.object(pupa).face, 1, "the second one did nothing");
    assert_eq!(s.st.object(pupa).transforms, 1);
    // A new activation turns it back.
    s.activate("Test Moth").pay().pass_all();
    assert_eq!(s.st.object(pupa).face, 0);
}

// CR 712.6a: a transformed permanent that leaves the battlefield is its front face again.
#[test]
fn a_transformed_permanent_that_dies_is_its_front_face() {
    let kill = "Name:Test Ending\nManaCost:B\nTypes:Instant\n\
        A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Text.\nOracle:Text.";
    let mut sc = scenario(&[PUPA, kill]);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
    sc.card(P0, Zone::Battlefield, "Test Pupa");
    sc.card(P0, Zone::Hand, "Test Ending");
    let mut s = Script::new(sc.build());
    s.activate("Test Pupa").pay().pass_all();
    s.cast("Test Ending").target("Test Moth").pay().pass_all();
    let dead = the(&s, P0, Zone::Graveyard, "Test Pupa");
    let c = chars(&s.st, dead);
    assert_eq!((c.name.as_str(), c.power), ("Test Pupa", Some(2)));
    assert_eq!(s.st.object(dead).face, 0);
}

// A triggered ability that transforms its own permanent (`DB$ SetState`, `Defined$` left
// out: the card itself), into a land.
#[test]
fn a_trigger_transforms_its_permanent_into_a_land() {
    let mut sc = scenario(&[RITE]);
    let rite = sc.card(P0, Zone::Battlefield, "Test Rite");
    sc.cards(P0, Zone::Battlefield, &["Test Golem", "Test Golem"]);
    let mut s = Script::new(sc.build());
    for _ in 0..200 {
        if s.st.object(rite).face == 1 || s.next.is_none() {
            break;
        }
        pick_min(&mut s);
    }
    let c = chars(&s.st, rite);
    assert_eq!(c.name, "Test Rite Grove");
    assert_eq!(c.card_types, [CardType::Land]);
    assert_eq!(c.colors, [card_model::Color::Green]);
}

// Only "transform this permanent" compiles: other modes of Forge's SetState, and another
// object, stay blocked.
#[test]
fn other_set_state_forms_block() {
    let flip = PUPA.replace("Mode$ Transform", "Mode$ Flip");
    assert_eq!(forge_test_blockers(&flip), ["SetState:mode Flip"]);
    let other = PUPA.replace(
        "Defined$ Self | Mode$ Transform",
        "ValidTgts$ Creature | Mode$ Transform",
    );
    assert!(forge_test_blockers(&other).contains(&"SetState:target".to_string()));
}

fn pick_min(s: &mut Script) {
    let r = match &s.decision().prim {
        Primitive::PickN { min, .. } => Response::Pick((0..*min).collect()),
        Primitive::PickNumber { min, .. } => Response::Number(*min),
        Primitive::Allocate { per_min, .. } => Response::Allocate(per_min.clone()),
        Primitive::Order => Response::Order((0..s.options().len()).collect()),
    };
    s.respond(r);
}
// ---------------------------------------------------------------------------------------
// Adventurer cards (CR 716) and omen cards (CR 721).

// A 3/3 creature whose Adventure is an instant: 2 damage to any target.
const KNIGHT: &str = "Name:Test Errant Knight\n\
    ManaCost:2 R\n\
    Types:Creature Human Knight\n\
    PT:3/3\n\
    AlternateMode:Adventure\n\
    Oracle:\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Quest Strike\n\
    ManaCost:R\n\
    Types:Instant Adventure\n\
    A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 2 | SpellDescription$ CARDNAME deals 2 damage to any target.\n\
    Oracle:Test Quest Strike deals 2 damage to any target.";

// A creature whose Omen is a sorcery: draw two cards.
const SEER: &str = "Name:Test Omen Seer\n\
    ManaCost:4 U\n\
    Types:Creature Sphinx\n\
    PT:4/4\n\
    AlternateMode:Omen\n\
    Oracle:\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Portent\n\
    ManaCost:1 U\n\
    Types:Sorcery Omen\n\
    A:SP$ Draw | NumCards$ 2 | SpellDescription$ Draw two cards.\n\
    Oracle:Draw two cards.";

const COUNTER: &str = "Name:Test Denial\nManaCost:U\nTypes:Instant\n\
    A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | SpellDescription$ Text.\nOracle:Text.";

// CR 716.4: in a hand an adventurer card has only its normal characteristics; CR 716.3:
// it is cast normally or as an Adventure (CR 716.3a: then only the Adventure is evaluated).
#[test]
fn an_adventurer_card_in_hand() {
    let mut sc = scenario(&[KNIGHT]);
    sc.card(P0, Zone::Battlefield, "Mountain");
    let card = sc.card(P0, Zone::Hand, "Test Errant Knight");
    let s = Script::new(sc.build());
    let c = chars(&s.st, card);
    assert_eq!(c.name, "Test Errant Knight");
    assert_eq!(c.mana_value(), 3);
    // One Mountain: not the Knight, but its Adventure.
    assert!(!s.can_cast("Test Errant Knight"));
    assert!(s.has(|_, o| matches!(o, OptionDesc::CastFace { card: c, face: 1 } if *c == card)));
    // No land face, nothing to play.
    assert!(!s.has(|_, o| matches!(o, OptionDesc::PlayLandFace { .. })));
}

// CR 716.2a: "a creature spell that has an Adventure" — Forge's `AdventureCard` — is an
// adventurer cast as its creature, whether or not it went on its Adventure first; the
// Adventure spell itself has an Adventure too, but it isn't a creature spell.
#[test]
fn a_creature_spell_that_has_an_adventure() {
    let inn = "Name:Test Innkeeper\nManaCost:G\nTypes:Creature Human\nPT:1/1\n\
        T:Mode$ SpellCast | ValidCard$ Creature.AdventureCard | ValidActivatingPlayer$ You | Execute$ TrigDraw | TriggerZones$ Battlefield | TriggerDescription$ Text.\n\
        SVar:TrigDraw:DB$ Draw | Defined$ You | NumCards$ 1\n\
        Oracle:Whenever you cast a creature spell that has an Adventure, draw a card.";
    let mut sc = scenario(&[KNIGHT, inn]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Mountain", "Mountain", "Mountain", "Mountain", "Mountain"],
    );
    sc.card(P0, Zone::Battlefield, "Test Innkeeper");
    sc.cards(P0, Zone::Hand, &["Test Errant Knight", "Test Golem"]);
    let mut s = Script::new(sc.build());
    let drew = |s: &Script| {
        s.count(|e| matches!(e, engine_core::Event::Drew { player, .. } if *player == P0))
    };
    let before = drew(&s);
    // The Adventure: not a creature spell.
    s.cast_face("Test Quest Strike")
        .target_player(P1)
        .pay()
        .pass_all();
    assert_eq!(drew(&s), before);
    // The creature from exile: a creature spell that has an Adventure.
    s.cast("Test Errant Knight").pay().pass_all();
    assert_eq!(drew(&s), before + 1);
    let golem = s.find(P0, Zone::Hand, "Test Golem").unwrap();
    assert!(!chars(&s.st, golem).has_adventure);
}

// CR 716.3b: on the stack as an Adventure it has only the Adventure's characteristics.
// CR 716.3d: as it resolves it is exiled, and its controller may cast the creature from
// exile — but not as an Adventure again.
#[test]
fn going_on_an_adventure_and_casting_the_creature_from_exile() {
    let mut sc = scenario(&[KNIGHT]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Mountain", "Mountain", "Mountain", "Mountain"],
    );
    sc.card(P0, Zone::Hand, "Test Errant Knight");
    let mut s = Script::new(sc.build());
    s.cast_face("Test Quest Strike");
    let spell = the(&s, P0, Zone::Stack, "Test Errant Knight");
    let c = chars(&s.st, spell);
    assert_eq!(c.name, "Test Quest Strike");
    assert!(c.is(CardType::Instant) && !c.is(CardType::Creature));
    assert!(c.has_subtype("Adventure"));
    assert_eq!(c.mana_value(), 1);
    s.target_player(P1).pay().pass_all();
    assert_eq!(s.st.player(P1).life, 38);
    let exiled = the(&s, P0, Zone::Exile, "Test Errant Knight");
    assert_eq!(chars(&s.st, exiled).name, "Test Errant Knight", "CR 716.4");
    assert_eq!(s.st.object(exiled).adventure, Some(P0));
    // Its controller may cast the creature from exile; not the Adventure.
    assert!(s.can_cast("Test Errant Knight"));
    assert!(!s.has(|_, o| matches!(o, OptionDesc::CastFace { .. })));
    // Nobody else may.
    let v = engine_core::player_view(&s.st, P1);
    assert!(v.exile.iter().any(|o| o.id == exiled));
    s.cast("Test Errant Knight").pay().pass_all();
    let knight = the(&s, P0, Zone::Battlefield, "Test Errant Knight");
    assert_eq!(chars(&s.st, knight).power, Some(3));
    assert_eq!(s.st.object(knight).adventure, None);
}

// CR 716.3d is about resolving: a countered Adventure goes to its owner's graveyard.
#[test]
fn a_countered_adventure_goes_to_the_graveyard() {
    let mut sc = scenario(&[KNIGHT, COUNTER]);
    sc.cards(P0, Zone::Battlefield, &["Mountain"]);
    sc.card(P0, Zone::Hand, "Test Errant Knight");
    sc.card(P1, Zone::Battlefield, "Island");
    sc.card(P1, Zone::Hand, "Test Denial");
    let mut s = Script::new(sc.build());
    s.cast_face("Test Quest Strike").target_player(P1).pay();
    s.pass();
    s.cast("Test Denial")
        .target("Test Quest Strike")
        .pay()
        .pass_all();
    s.pass_all();
    let card = the(&s, P0, Zone::Graveyard, "Test Errant Knight");
    assert_eq!(s.st.object(card).adventure, None);
    assert!(!s.can_cast("Test Errant Knight"));
}

// Cast normally, an adventurer card is just its creature.
#[test]
fn an_adventurer_cast_normally_is_a_creature() {
    let mut sc = scenario(&[KNIGHT]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Mountain", "Mountain"]);
    sc.card(P0, Zone::Hand, "Test Errant Knight");
    let mut s = Script::new(sc.build());
    s.cast("Test Errant Knight").pay().pass_all();
    let knight = the(&s, P0, Zone::Battlefield, "Test Errant Knight");
    assert_eq!(s.st.object(knight).face, 0);
}

// CR 721.3d: an Omen shuffles into its owner's library as it resolves.
#[test]
fn an_omen_shuffles_into_its_owners_library() {
    let mut sc = scenario(&[SEER]);
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    sc.card(P0, Zone::Hand, "Test Omen Seer");
    let mut s = Script::new(sc.build());
    let library = s.st.zones.list(Zone::Library, P0).len();
    s.cast_face("Test Portent").pay().pass_all();
    the(&s, P0, Zone::Library, "Test Omen Seer");
    assert_eq!(s.st.zones.list(Zone::Library, P0).len(), library + 1 - 2);
    assert!(s.st.zones.list(Zone::Graveyard, P0).is_empty());
    assert!(
        s.count(|e| matches!(e, engine_core::Event::Shuffled { player } if *player == P0)) >= 1
    );
}

// CR 716.3d: "that player may *play* it" — a land with an Adventure is played from exile
// (with the land drop) once its Adventure has resolved.
#[test]
fn a_land_sent_on_an_adventure_is_played_from_exile() {
    let tower = "Name:Test Watchtower\n\
        ManaCost:no cost\n\
        Types:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ R | SpellDescription$ Add {R}.\n\
        AlternateMode:Adventure\n\
        Oracle:{T}: Add {R}.\n\
        \n\
        ALTERNATE\n\
        \n\
        Name:Test Signal Fire\n\
        ManaCost:R\n\
        Types:Instant Adventure\n\
        A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 1 | SpellDescription$ Text.\n\
        Oracle:Text.";
    let mut sc = scenario(&[tower]);
    sc.card(P0, Zone::Battlefield, "Mountain");
    sc.card(P0, Zone::Hand, "Test Watchtower");
    let mut s = Script::new(sc.build());
    s.cast_face("Test Signal Fire")
        .target_player(P1)
        .pay()
        .pass_all();
    let exiled = the(&s, P0, Zone::Exile, "Test Watchtower");
    assert!(s.has(|_, o| matches!(o, OptionDesc::PlayLand { card } if *card == exiled)));
    s.pick("play the Watchtower", |_, o| {
        matches!(o, OptionDesc::PlayLand { .. })
    });
    the(&s, P0, Zone::Battlefield, "Test Watchtower");
    assert_eq!(s.st.player(P0).lands_played_this_turn, 1);
}

// ---------------------------------------------------------------------------------------
// Determinism with cards whose back face is up: a mid-game save/load and a clone replay
// the same responses to the same events and hashes.

#[test]
fn save_load_and_clone_with_back_faces_up() {
    let mut sc = scenario(&[TIDE, PUPA, KILN]);
    sc.cards(
        P0,
        Zone::Battlefield,
        &["Swamp", "Swamp", "Swamp", "Swamp", "Forest"],
    );
    sc.card(P0, Zone::Battlefield, "Test Pupa");
    sc.cards(P0, Zone::Hand, &["Test Tide Charm", "Test Kiln Blast"]);
    let mut s = Script::new(sc.build());
    s.play_face("Test Tide Flat");
    s.activate("Test Pupa").pay().pass_all();
    s.cast_face("Test Kiln Golem");
    // Mid-cast, with a transformed permanent and a land face on the battlefield and a back
    // face on the stack.
    let start = s.st.clone();
    let mut responses = Vec::new();
    let mut events = Vec::new();
    for _ in 0..400 {
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
        events.extend(out.events);
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
// Split cards (CR 709).

const BLAZE: &str = "Name:Test Scorch\n\
    ManaCost:1 R\n\
    Types:Instant\n\
    A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 2 | SpellDescription$ CARDNAME deals 2 damage to any target.\n\
    AlternateMode:Split\n\
    Oracle:Test Scorch deals 2 damage to any target.\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Frost\n\
    ManaCost:2 U\n\
    Types:Sorcery\n\
    A:SP$ Draw | NumCards$ 2 | SpellDescription$ Draw two cards.\n\
    Oracle:Draw two cards.";

// CR 709.4: everywhere but the stack a split card is its two halves combined — both names,
// the combined mana cost (and mana value and colors from it, CR 709.4b), every type.
#[test]
fn a_split_card_in_hand_is_both_halves() {
    let mut sc = scenario(&[BLAZE]);
    let card = sc.card(P0, Zone::Hand, "Test Scorch // Test Frost");
    let s = Script::new(sc.build());
    let c = chars(&s.st, card);
    assert_eq!(c.name, "Test Scorch // Test Frost");
    assert_eq!(c.mana_value(), 5);
    assert_eq!(c.colors, [card_model::Color::Blue, card_model::Color::Red]);
    assert!(c.is(CardType::Instant) && c.is(CardType::Sorcery));
    let cat = &s.st.catalog;
    let id = cat.find("Test Scorch // Test Frost").unwrap();
    assert_eq!(cat.get(id).layout, Layout::Split);
    assert_eq!(cat.get(cat.face(id, 2)).name, "Test Frost");
}

// CR 709.3: a player chooses which half to cast; CR 709.3a only that half is evaluated
// (its own timing: the sorcery half only at sorcery speed); CR 709.3b on the stack only
// that half exists. The card is never cast whole.
#[test]
fn casting_one_half_of_a_split_card() {
    let mut sc = scenario(&[BLAZE]);
    sc.cards(P0, Zone::Battlefield, &["Mountain", "Island", "Island"]);
    let card = sc.card(P0, Zone::Hand, "Test Scorch // Test Frost");
    let mut s = Script::new(sc.build());
    assert!(!s.has(|_, o| matches!(o, OptionDesc::CastSpell { .. })));
    for face in [1, 2] {
        assert!(s.has(|_, o| matches!(o, OptionDesc::CastFace { card: c, face: f } if *c == card && *f == face)));
    }
    s.cast_face("Test Scorch");
    let spell = the(&s, P0, Zone::Stack, "Test Scorch // Test Frost");
    let c = chars(&s.st, spell);
    assert_eq!(c.name, "Test Scorch");
    assert_eq!(c.mana_value(), 2);
    assert_eq!(c.card_types, [CardType::Instant]);
    s.target_player(P1).pay().pass_all();
    assert_eq!(s.st.player(P1).life, 38);
    // In the graveyard it is the whole card again.
    let dead = the(&s, P0, Zone::Graveyard, "Test Scorch // Test Frost");
    assert_eq!(chars(&s.st, dead).mana_value(), 5);
}

// CR 709.3a: on an opponent's turn only the instant half can be cast.
#[test]
fn only_the_instant_half_at_instant_speed() {
    let mut sc = scenario(&[BLAZE]);
    sc.cards(P1, Zone::Battlefield, &["Mountain", "Island", "Island"]);
    let card = sc.card(P1, Zone::Hand, "Test Scorch // Test Frost");
    let s = Script::new(sc.build());
    assert_eq!(s.who(), P0);
    let mut s = s;
    s.pass();
    assert_eq!(s.who(), P1);
    assert!(s.has(|_, o| matches!(o, OptionDesc::CastFace { card: c, face: 1 } if *c == card)));
    assert!(!s.has(|_, o| matches!(o, OptionDesc::CastFace { face: 2, .. })));
}

// Rooms (CR 709.5), fused and aftermath halves (CR 702.102, 702.127) stay blocked.
#[test]
fn split_cards_with_rules_of_their_own_block() {
    let room = BLAZE
        .replace("Types:Instant", "Types:Enchantment Room")
        .replace("Types:Sorcery", "Types:Enchantment Room");
    let b = forge_test_blockers(&room);
    assert!(b.contains(&"split:Room".to_string()), "{b:?}");
    let fuse = BLAZE.replace("AlternateMode:Split", "K:Fuse\nAlternateMode:Split");
    assert!(
        forge_test_blockers(&fuse)
            .iter()
            .any(|b| b.starts_with("kw:"))
    );
    let after = BLAZE.replace("Name:Test Frost\n", "Name:Test Frost\nK:Aftermath\n");
    assert!(
        forge_test_blockers(&after)
            .iter()
            .any(|b| b.starts_with("kw:"))
    );
}

// ---------------------------------------------------------------------------------------
// Faces together with how a permanent enters (D-067) and "its controller" (D-068).

/// Subtypes are known to the compiler from the type lines of the cards it compiles.
const ISLAND_TYPE: &str = "Name:Test Typeholder\nManaCost:no cost\nTypes:Land Island\nOracle:Text.";

/// A modal card with two land faces: the front enters tapped; the back asks "as this
/// enters, you may pay 3 life; if you don't, it enters tapped".
const MIRE: &str = "Name:Test Sunken Path\n\
    ManaCost:no cost\n\
    Types:Land\n\
    R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplacementResult$ Updated | ReplaceWith$ ETBTapped | Description$ CARDNAME enters tapped.\n\
    SVar:ETBTapped:DB$ Tap | Defined$ Self | ETB$ True\n\
    A:AB$ Mana | Cost$ T | Produced$ U | SpellDescription$ Add {U}.\n\
    AlternateMode:Modal\n\
    Oracle:Test Sunken Path enters tapped.\\n{T}: Add {U}.\n\
    \n\
    ALTERNATE\n\
    \n\
    Name:Test Sunken Mire\n\
    ManaCost:no cost\n\
    Types:Land\n\
    R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ DBTap | ReplacementResult$ Updated | Description$ As CARDNAME enters, you may pay 3 life. If you don't, it enters tapped.\n\
    SVar:DBTap:DB$ Tap | ETB$ True | Defined$ Self | UnlessCost$ PayLife<3> | UnlessPayer$ You\n\
    A:AB$ Mana | Cost$ T | Produced$ B | SpellDescription$ Add {B}.\n\
    Oracle:As Test Sunken Mire enters, you may pay 3 life. If you don't, it enters tapped.\\n{T}: Add {B}.";

// CR 712.9, 614.12, 614.12a (2026-11-06): a land face played is checked as it would enter —
// with that face's "as this enters" choice, never the other face's "enters tapped".
#[test]
fn a_land_face_asks_its_own_as_it_enters_choice() {
    // The back face: asked; paying 3 life, it enters untapped.
    let mut sc = scenario(&[MIRE]);
    sc.card(P0, Zone::Hand, "Test Sunken Path");
    let mut s = Script::new(sc.build());
    s.play_face("Test Sunken Mire");
    assert!(
        matches!(s.kind(), ChoiceKind::AsEnters { .. }),
        "{:?}",
        s.kind()
    );
    assert_eq!(s.options(), [OptionDesc::Accept, OptionDesc::Decline]);
    s.pick("pay", |_, o| *o == OptionDesc::Accept);
    let land = the(&s, P0, Zone::Battlefield, "Test Sunken Path");
    assert_eq!(s.st.object(land).face, 1);
    assert!(
        !s.st.object(land).tapped,
        "the front's \"enters tapped\" isn't consulted"
    );
    assert_eq!(s.st.player(P0).life, 37);
    // Declining: tapped.
    let mut sc = scenario(&[MIRE]);
    sc.card(P0, Zone::Hand, "Test Sunken Path");
    let mut s = Script::new(sc.build());
    s.play_face("Test Sunken Mire");
    s.pick("decline", |_, o| *o == OptionDesc::Decline);
    let land = the(&s, P0, Zone::Battlefield, "Test Sunken Path");
    assert!(s.st.object(land).tapped);
    assert_eq!(s.st.player(P0).life, 40);
    // The front face: its own "enters tapped", and no choice.
    let mut sc = scenario(&[MIRE]);
    sc.card(P0, Zone::Hand, "Test Sunken Path");
    let mut s = Script::new(sc.build());
    s.pick("play the front", |_, o| {
        matches!(o, OptionDesc::PlayLand { .. })
    });
    assert_eq!(s.kind(), ChoiceKind::Priority);
    let land = the(&s, P0, Zone::Battlefield, "Test Sunken Path");
    assert_eq!(s.st.object(land).face, 0);
    assert!(s.st.object(land).tapped);
    assert_eq!(s.st.player(P0).life, 40);
}

// CR 614.12: "enters tapped unless you control an Island" on a back land face, judged
// before it enters.
#[test]
fn a_land_face_that_enters_tapped_unless() {
    let charm = "Name:Test Check Charm\n\
        ManaCost:1 U\n\
        Types:Instant\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\n\
        AlternateMode:Modal\n\
        Oracle:Draw a card.\n\
        \n\
        ALTERNATE\n\
        \n\
        Name:Test Check Shore\n\
        ManaCost:no cost\n\
        Types:Land\n\
        R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ DBTap | ReplacementResult$ Updated | Description$ Text.\n\
        SVar:DBTap:DB$ Tap | ETB$ True | Defined$ Self | ConditionPresent$ Island.YouCtrl | ConditionCompare$ EQ0\n\
        A:AB$ Mana | Cost$ T | Produced$ U | SpellDescription$ Add {U}.\n\
        Oracle:Text.";
    for (island, tapped) in [(false, true), (true, false)] {
        let mut sc = scenario(&[charm, ISLAND_TYPE]);
        if island {
            sc.card(P0, Zone::Battlefield, "Island");
        }
        sc.card(P0, Zone::Hand, "Test Check Charm");
        let mut s = Script::new(sc.build());
        s.play_face("Test Check Shore");
        let land = the(&s, P0, Zone::Battlefield, "Test Check Charm");
        assert_eq!(s.st.object(land).tapped, tapped, "with an Island: {island}");
    }
}

// CR 608.2h, 716.3b, 709.3b: "its controller gains life equal to its mana value" of a
// countered spell cast as an Adventure, or as one half of a split card — the mana value of
// what is on the stack; the countered Adventure goes to the graveyard (CR 716.3d is about
// resolving).
#[test]
fn its_controller_and_mana_value_of_a_countered_face() {
    let lesson = "Name:Test Lesson\nManaCost:U\nTypes:Instant\n\
        A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | SubAbility$ DBGain | SpellDescription$ Text.\n\
        SVar:DBGain:DB$ GainLife | Defined$ TargetedController | LifeAmount$ X\n\
        SVar:X:Targeted$CardManaCost\nOracle:Text.";
    let scorch = "Name:Test Scorch\nManaCost:1 R\nTypes:Instant\n\
        A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 2 | SpellDescription$ Text.\n\
        AlternateMode:Split\nOracle:Text.\n\nALTERNATE\n\nName:Test Frost\nManaCost:2 U\nTypes:Sorcery\n\
        A:SP$ Draw | NumCards$ 2 | SpellDescription$ Draw two cards.\nOracle:Draw two cards.";
    for (card, face, mv) in [
        ("Test Errant Knight", "Test Quest Strike", 1),
        ("Test Scorch // Test Frost", "Test Scorch", 2),
    ] {
        let mut sc = scenario(&[KNIGHT, lesson, scorch]);
        sc.cards(P1, Zone::Battlefield, &["Mountain", "Mountain"]);
        sc.card(P1, Zone::Hand, card);
        sc.card(P0, Zone::Battlefield, "Island");
        sc.card(P0, Zone::Hand, "Test Lesson");
        sc.turn(P1, engine_core::turn::Step::PrecombatMain);
        let mut s = Script::new(sc.build());
        s.cast_face(face).target_player(P0).pay().pass();
        assert_eq!(s.who(), P0);
        s.cast("Test Lesson").target(face).pay();
        for _ in 0..10 {
            if s.st.stack.is_empty() {
                break;
            }
            s.pass();
        }
        assert_eq!(s.st.player(P1).life, 40 + mv, "{face}");
        assert_eq!(s.st.player(P0).life, 40);
        let dead = s
            .find(P1, Zone::Graveyard, card)
            .unwrap_or_else(|| panic!("{card} in the graveyard"));
        assert_eq!(s.st.object(dead).adventure, None);
    }
}

// CR 202.3b, 608.2h: "its mana value" of a transformed permanent is its front face's.
#[test]
fn mana_value_of_a_transformed_permanent_as_a_target() {
    let swarming = "Name:Test Swarming\nManaCost:B\nTypes:Sorcery\n\
        A:SP$ Destroy | ValidTgts$ Creature | SubAbility$ DBLoseLife | SpellDescription$ Text.\n\
        SVar:DBLoseLife:DB$ LoseLife | Defined$ You | LifeAmount$ X\n\
        SVar:X:Targeted$CardManaCost\nOracle:Text.";
    let mut sc = scenario(&[PUPA, swarming]);
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Forest"]);
    let pupa = sc.card(P0, Zone::Battlefield, "Test Pupa"); // {G}
    sc.card(P0, Zone::Hand, "Test Swarming");
    let mut s = Script::new(sc.build());
    s.tap("Forest", engine_core::mana::ManaType::Green);
    s.activate("Test Pupa").pay().pass_all();
    assert_eq!(s.st.object(pupa).face, 1);
    s.cast("Test Swarming").target_object(pupa).pay().pass_all();
    assert_eq!(
        s.st.player(P0).life,
        39,
        "the Moth's mana value is the Pupa's: 1"
    );
}

// ---------------------------------------------------------------------------------------
// Faces together with playing from other zones (D-070) and mana abilities (D-066).

const CRUCIBLE: &str = "Name:Test Crucible\nManaCost:3\nTypes:Artifact\n\
    S:Mode$ Continuous | Affected$ Land.YouOwn | MayPlay$ True | AffectedZone$ Graveyard | Description$ Text.\n\
    Oracle:You may play lands from your graveyard.";

// CR 601.3e, 712.9 (2026-11-06): "you may play lands from your graveyard" is judged by the
// face that would be played — a modal card whose back face is a land may be played as that
// land face from the graveyard (it is the turn's land play, CR 305.2a), but its front
// face, an instant, may not be cast from there. CR 712.6a: in the graveyard it is an
// instant card; it enters with the land face up.
#[test]
fn a_land_face_played_from_the_graveyard() {
    let mut sc = scenario(&[TIDE, CRUCIBLE]);
    sc.card(P0, Zone::Battlefield, "Test Crucible");
    sc.cards(P0, Zone::Battlefield, &["Island", "Island"]);
    let card = sc.card(P0, Zone::Graveyard, "Test Tide Charm");
    let mut s = Script::new(sc.build());
    assert!(chars(&s.st, card).is(CardType::Instant));
    assert!(s.has(|_, o| matches!(o, OptionDesc::PlayLandFace { card: c, face: 1 } if *c == card)));
    assert!(!s.has(|_, o| matches!(o, OptionDesc::PlayLand { card: c } if *c == card)));
    assert!(!s.has(|_, o| matches!(o, OptionDesc::CastSpell { card: c } if *c == card)));
    s.play_face("Test Tide Flat");
    let land = the(&s, P0, Zone::Battlefield, "Test Tide Charm");
    assert_eq!(s.st.object(land).face, 1);
    assert!(
        s.st.object(land).tapped,
        "the land face's own \"enters tapped\""
    );
    assert_eq!(s.st.player(P0).lands_played_this_turn, 1);
    // Without the rule: nothing from the graveyard.
    let mut sc = scenario(&[TIDE, CRUCIBLE]);
    let card = sc.card(P0, Zone::Graveyard, "Test Tide Charm");
    let s = Script::new(sc.build());
    assert!(!s.has(|_, o| matches!(o, OptionDesc::PlayLandFace { card: c, .. } if *c == card)));
}

// CR 601.3e, 712.8c: "you may cast creature spells from your graveyard" lets a modal card
// whose back face is a creature be cast as that face (and not as its sorcery front). CR
// 106.6: mana that may be spent only on creature spells pays for it — the restriction
// looks at the face being cast. CR 601.2g, 605.3a: a mana ability that costs {1} (a
// Signet) is paid from the pool on the way.
#[test]
fn a_creature_face_cast_from_the_graveyard_with_restricted_mana_and_a_signet() {
    let necro = "Name:Test Necrostone\nManaCost:3\nTypes:Artifact\n\
        S:Mode$ Continuous | Affected$ Creature.YouOwn | MayPlay$ True | AffectedZone$ Graveyard | Description$ Text.\n\
        Oracle:You may cast creature spells from your graveyard.";
    let signet = "Name:Test Signet\nManaCost:2\nTypes:Artifact\n\
        A:AB$ Mana | Cost$ 1 T | Produced$ U B | SpellDescription$ Add {U}{B}.\n\
        Oracle:{1}, {T}: Add {U}{B}.";
    let cavern = "Name:Test Cavern\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ C | RestrictValid$ Spell.Creature | SpellDescription$ Add {C}. Spend this mana only to cast creature spells.\n\
        Oracle:{T}: Add {C}. Spend this mana only to cast creature spells.";
    let mut sc = scenario(&[KILN, necro, signet, cavern]);
    sc.card(P0, Zone::Battlefield, "Test Necrostone");
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Battlefield, "Test Signet");
    sc.card(P0, Zone::Battlefield, "Test Cavern");
    sc.card(P0, Zone::Battlefield, "Plains");
    let card = sc.card(P0, Zone::Graveyard, "Test Kiln Blast");
    let mut s = Script::new(sc.build());
    // {3}{B}: the Signet's {U}{B} (paid for by the Forest), the Cavern's {C} — which only
    // a creature spell may spend — and the Plains.
    assert!(!s.has(|_, o| matches!(o, OptionDesc::CastSpell { card: c } if *c == card)));
    assert!(s.has(|_, o| matches!(o, OptionDesc::CastFace { card: c, face: 1 } if *c == card)));
    s.cast_face("Test Kiln Golem");
    s.tap("Forest", engine_core::mana::ManaType::Green);
    s.tap("Test Signet", engine_core::mana::ManaType::Blue);
    s.tap("Test Cavern", engine_core::mana::ManaType::Colorless);
    s.pay();
    s.pass_all();
    let golem = the(&s, P0, Zone::Battlefield, "Test Kiln Blast");
    assert_eq!(chars(&s.st, golem).name, "Test Kiln Golem");
    // Without the Cavern there is not enough mana: the face isn't offered.
    let mut sc = scenario(&[KILN, necro, signet, cavern]);
    sc.card(P0, Zone::Battlefield, "Test Necrostone");
    sc.card(P0, Zone::Battlefield, "Forest");
    sc.card(P0, Zone::Battlefield, "Test Signet");
    sc.card(P0, Zone::Battlefield, "Plains");
    let card = sc.card(P0, Zone::Graveyard, "Test Kiln Blast");
    let s = Script::new(sc.build());
    assert!(!s.has(|_, o| matches!(o, OptionDesc::CastFace { card: c, .. } if *c == card)));
}

// CR 106.6: the same restricted mana doesn't pay for the sorcery face of the same card —
// both faces cost {2}; two creature-only mana pay only for the creature face.
#[test]
fn restricted_mana_looks_at_the_face_being_cast() {
    let cavern = "Name:Test Cavern\nManaCost:no cost\nTypes:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ C | RestrictValid$ Spell.Creature | SpellDescription$ Add {C}. Spend this mana only to cast creature spells.\n\
        Oracle:{T}: Add {C}. Spend this mana only to cast creature spells.";
    let twin = "Name:Test Twin Rite\nManaCost:2\nTypes:Sorcery\n\
        A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\nAlternateMode:Modal\nOracle:Draw a card.\n\
        \nALTERNATE\n\nName:Test Twin Golem\nManaCost:2\nTypes:Artifact Creature Golem\nPT:2/2\nOracle:";
    let mut sc = scenario(&[twin, cavern]);
    sc.cards(P0, Zone::Battlefield, &["Test Cavern", "Test Cavern"]);
    let card = sc.card(P0, Zone::Hand, "Test Twin Rite");
    let mut s = Script::new(sc.build());
    assert!(s.has(|_, o| matches!(o, OptionDesc::CastFace { card: c, face: 1 } if *c == card)));
    assert!(
        !s.can_cast("Test Twin Rite"),
        "the sorcery face can't spend it"
    );
    s.cast_face("Test Twin Golem").pay().pass_all();
    let golem = the(&s, P0, Zone::Battlefield, "Test Twin Rite");
    assert_eq!(chars(&s.st, golem).name, "Test Twin Golem");
}

// CR 701.27c, 712.6d: a transformed permanent whose back face is a land taps for that
// face's mana — here two mana in any combination of {R} and/or {G} (D-066).
#[test]
fn a_transformed_land_face_taps_for_a_combination() {
    let seed = "Name:Test Seedling\n\
        ManaCost:G\n\
        Types:Creature Plant\n\
        PT:1/1\n\
        A:AB$ SetState | Cost$ 1 | Defined$ Self | Mode$ Transform | SpellDescription$ Transform CARDNAME.\n\
        AlternateMode:DoubleFaced\n\
        Oracle:{1}: Transform Test Seedling.\n\
        \n\
        ALTERNATE\n\
        \n\
        Name:Test Heartgrove\n\
        ManaCost:no cost\n\
        Colors:green\n\
        Types:Land\n\
        A:AB$ Mana | Cost$ T | Produced$ Combo R G | Amount$ 2 | SpellDescription$ Add two mana in any combination of {R} and/or {G}.\n\
        Oracle:{T}: Add two mana in any combination of {R} and/or {G}.";
    let mut sc = scenario(&[seed]);
    sc.card(P0, Zone::Battlefield, "Forest");
    let it = sc.card(P0, Zone::Battlefield, "Test Seedling");
    let mut s = Script::new(sc.build());
    // The front face has no mana ability.
    assert!(!s.has(|_, o| matches!(o, OptionDesc::TapForMana { source, .. } if *source == it)));
    s.activate("Test Seedling").pay().pass_all();
    assert_eq!(chars(&s.st, it).card_types, [CardType::Land]);
    s.tap("Test Heartgrove", engine_core::mana::ManaType::Red);
    assert_eq!(s.kind(), ChoiceKind::ChooseColor, "the second mana's type");
    s.pick("green", |_, o| {
        matches!(
            o,
            OptionDesc::Mana {
                mana: engine_core::mana::ManaType::Green
            }
        )
    });
    let pool = s.st.player(P0).mana_pool;
    assert_eq!(
        (
            pool.amounts[engine_core::mana::ManaType::Red.index()],
            pool.amounts[engine_core::mana::ManaType::Green.index()]
        ),
        (1, 1)
    );
}

// CR 608.2c, 701.27g: "you may transform this. If you do, you gain 3 life" — the life only
// if it did transform: not when it already transformed after the ability was put on the
// stack (then the instruction does nothing).
#[test]
fn if_you_do_after_a_transform() {
    let watcher = "Name:Test Lookout\n\
        ManaCost:G\n\
        Types:Creature Human\n\
        PT:1/1\n\
        T:Mode$ SpellCast | ValidCard$ Creature | ValidActivatingPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigTransform | OptionalDecider$ You | TriggerDescription$ Whenever you cast a creature spell, you may transform CARDNAME. If you do, you gain 3 life.\n\
        SVar:TrigTransform:DB$ SetState | Defined$ Self | Mode$ Transform | SubAbility$ DBGain\n\
        SVar:DBGain:DB$ GainLife | LifeAmount$ 3\n\
        A:AB$ SetState | Cost$ 1 | Defined$ Self | Mode$ Transform | SpellDescription$ Transform CARDNAME.\n\
        AlternateMode:DoubleFaced\n\
        Oracle:Whenever you cast a creature spell, you may transform Test Lookout. If you do, you gain 3 life.\\n{1}: Transform Test Lookout.\n\
        \n\
        ALTERNATE\n\
        \n\
        Name:Test Howler\n\
        ManaCost:no cost\n\
        Colors:green\n\
        Types:Creature Werewolf\n\
        PT:3/3\n\
        Oracle:";
    let run = |respond: bool| {
        let mut sc = scenario(&[watcher]);
        sc.cards(P0, Zone::Battlefield, &["Forest", "Forest"]);
        let it = sc.card(P0, Zone::Battlefield, "Test Lookout");
        sc.card(P0, Zone::Hand, "Test Elf");
        let mut s = Script::new(sc.build());
        s.cast("Test Elf").pay();
        // The trigger is on the stack above the Elf.
        if respond {
            s.activate("Test Lookout").pay();
            s.pass_all();
            assert_eq!(s.st.object(it).face, 1, "transformed in response");
        }
        for _ in 0..20 {
            if s.st.stack.is_empty() || s.next.is_none() {
                break;
            }
            if s.kind() == ChoiceKind::MayAbility {
                s.pick("yes", |_, o| *o == OptionDesc::Accept);
            } else {
                s.pass();
            }
        }
        (s.st.object(it).face, s.st.player(P0).life)
    };
    assert_eq!(run(false), (1, 43), "transformed: the life");
    assert_eq!(run(true), (1, 40), "already transformed: nothing, no life");
}
