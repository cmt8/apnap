//! Auto-pay (D-054, D-066): the session pays mana for the human in the cheapest way there
//! is — a Treasure when nothing cheaper can pay — but never with life, damage to
//! themselves, or by sacrificing a permanent that isn't a token.

use app::session::Session;
use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, GameConfig, OptionDesc, PlayerId, Response};
use harness::Agents;
use harness::script::Script;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const CARDS: &[&str] = &[
    "Name:Test Confluence\nManaCost:no cost\nTypes:Land\n\
     A:AB$ Mana | Cost$ T PayLife<1> | Produced$ Any | SpellDescription$ Add one mana of any color.\n\
     Oracle:{T}, Pay 1 life: Add one mana of any color.",
    "Name:Test Reef\nManaCost:no cost\nTypes:Land\n\
     A:AB$ Mana | Cost$ T | Produced$ Combo G R | SubAbility$ DBPain | SpellDescription$ Add {G} or {R}. CARDNAME deals 1 damage to you.\n\
     SVar:DBPain:DB$ DealDamage | NumDmg$ 1 | Defined$ You\n\
     Oracle:{T}: Add {G} or {R}. Test Reef deals 1 damage to you.",
    "Name:Test Petal\nManaCost:0\nTypes:Artifact\n\
     A:AB$ Mana | Cost$ T Sac<1/CARDNAME> | Produced$ Any | SpellDescription$ Add one mana of any color.\n\
     Oracle:{T}, Sacrifice this artifact: Add one mana of any color.",
    "Name:Test Altar\nManaCost:3\nTypes:Artifact\n\
     A:AB$ Mana | Cost$ Sac<1/Creature> | Produced$ C | Amount$ 2 | SpellDescription$ Add {C}{C}.\n\
     Oracle:Sacrifice a creature: Add {C}{C}.",
    "Name:Test Hoard\nManaCost:2\nTypes:Artifact\n\
     T:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigToken | TriggerDescription$ Text.\n\
     SVar:TrigToken:DB$ Token | TokenScript$ test_treasure\n\
     Oracle:When this artifact enters, create a Treasure token.",
    "Name:Test Seedling\nManaCost:1\nTypes:Creature Elf\nPT:1/1\nOracle:",
];

const TREASURE: (&str, &str) = (
    "test_treasure",
    "Name:Treasure Token\nManaCost:no cost\nTypes:Artifact Treasure\n\
     A:AB$ Mana | Cost$ T Sac<1/CARDNAME> | Produced$ Any | SpellDescription$ Add one mana of any color.\n\
     Oracle:{T}, Sacrifice this token: Add one mana of any color.",
);

/// P0 casts Test Seedling ({G}) with only `sources` on the battlefield; what does auto-pay
/// answer for the first mana payment?
fn auto_pay(sources: &[&str]) -> Option<String> {
    let catalog = harness::forge_test_catalog(CARDS, &[TREASURE]);
    let mut sc = Scenario::new(GameConfig::constructed_1v1(), catalog, 21);
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc.cards(P0, Zone::Battlefield, sources);
    sc.card(P0, Zone::Battlefield, "Test Seedling");
    sc.card(P0, Zone::Hand, "Test Seedling");
    let mut s = Script::new(sc.build());
    s.cast("Test Seedling");
    assert_eq!(s.kind(), ChoiceKind::PayMana);
    let session = Session::from_state(s.st.clone(), Some(P0), Agents::Random);
    let d = s.decision().clone();
    session.auto_response(&d).map(|r| {
        let Response::Pick(i) = r else {
            panic!("{r:?}")
        };
        match &d.options[i[0]] {
            OptionDesc::TapForMana { source, .. } => s.name(*source),
            other => format!("{other:?}"),
        }
    })
}

#[test]
fn auto_pay_uses_a_treasure_when_nothing_cheaper_pays() {
    // The Treasure is a token: it's spent when nothing else can pay.
    assert_eq!(
        auto_pay(&["Test Confluence", "test_treasure", "Test Petal"]).as_deref(),
        Some("Treasure Token")
    );
    // A Forest comes before it.
    assert_eq!(
        auto_pay(&["test_treasure", "Forest"]).as_deref(),
        Some("Forest")
    );
}

#[test]
fn auto_pay_never_pays_life_damage_or_a_nontoken_permanent() {
    for sources in [
        &["Test Confluence"][..],
        &["Test Reef"],
        &["Test Petal"],
        &["Test Altar"],
        &["Test Confluence", "Test Reef", "Test Petal", "Test Altar"],
    ] {
        assert_eq!(auto_pay(sources), None, "{sources:?}");
    }
}
