//! Comment lines in Forge's card scripts (D-078). Forge skips a line whose first character
//! is `#`; this compiler ignores the comments somebody has read and blocks the rest, and
//! the ones that say a script is incomplete. Invented cards in Forge's format.

use engine_core::testing::Scenario;
use engine_core::zones::Zone;
use engine_core::{GameConfig, PlayerId};
use harness::script::Script;
use harness::{forge_test_blockers, forge_test_catalog};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn drain(comment: &str) -> String {
    format!(
        "Name:Test Drain\n\
         ManaCost:1 B\n\
         Types:Sorcery\n\
         {comment}\
         A:SP$ LoseLife | Cost$ 1 B | Defined$ Player.Opponent | LifeAmount$ 3 | SpellDescription$ Each opponent loses 3 life.\n\
         Oracle:Each opponent loses 3 life."
    )
}

fn play_drain(script: &str) -> (i32, i32) {
    let mut sc = Scenario::new(
        GameConfig::commander(2),
        forge_test_catalog(&[script], &[]),
        31,
    );
    for p in [P0, P1] {
        sc.library(p, "Forest", 20);
    }
    sc.cards(P0, Zone::Battlefield, &["Swamp", "Swamp"]);
    sc.card(P0, Zone::Hand, "Test Drain");
    let mut s = Script::new(sc.build());
    s.cast("Test Drain").pay().pass_all();
    (s.st.player(P0).life, s.st.player(P1).life)
}

// Forge (CardRules.Reader.readCard): `line.isEmpty() || line.charAt(0) == '#'` is skipped.
#[test]
fn a_reviewed_comment_changes_nothing() {
    let plain = play_drain(&drain(""));
    let with = play_drain(&drain(
        "# TODO: Improve the tutoring logic for the AI. Currently will generally look for the most expensive castable thing in the library (which can, of course, be used to advantage in properly constructed AI decks).\n# --- VARIANTS ---\n",
    ));
    assert_eq!(plain, with);
    assert_eq!(plain, (40, 37));
}

#[test]
fn a_comment_nobody_has_read_blocks_by_name() {
    let b = forge_test_blockers(&drain("# The second ability is easy to forget.\n"));
    assert_eq!(b, ["comment:unreviewed"]);
    // A reviewed text with something added is a different comment: exact entries are exact.
    let b = forge_test_blockers(&drain("# --- VARIANTS --- plus a note\n"));
    assert_eq!(b, ["comment:unreviewed"]);
}

#[test]
fn a_comment_that_says_the_script_is_incomplete_blocks_as_a_differing_script() {
    for c in [
        "# TODO\n",
        "# TODO: need EnchantedLKI because it isn't enchanted anymore if this is sacrificed\n",
        "# Doesn't follow the strict wording of the Oracle text currently. Something else.\n",
    ] {
        let b = forge_test_blockers(&drain(c));
        assert_eq!(b, ["script:differs from the card"], "{c}");
    }
}

// Forge only looks at column 0: an indented `#` is a line like any other (not a comment).
#[test]
fn an_indented_hash_is_not_a_comment() {
    let b = forge_test_blockers(&drain("  # --- VARIANTS ---\n"));
    assert_eq!(b.len(), 1, "{b:?}");
    assert!(b[0].starts_with("line:"), "{b:?}");
}

// A `#` inside a value is part of the value (Forge only tests the first character).
#[test]
fn a_hash_inside_a_value_is_untouched() {
    let card = "Name:Test Hashed\n\
         ManaCost:1 B\n\
         Types:Sorcery\n\
         A:SP$ LoseLife | Cost$ 1 B | Defined$ Player.Opponent | LifeAmount$ 3 | SpellDescription$ Each opponent loses 3 life # not a comment.\n\
         Oracle:Each opponent loses 3 life # not a comment.";
    assert!(forge_test_blockers(card).is_empty());
}

// Forge's effects write results into SVars named `AF…` ("AFLifeLost will be set by
// LoseLife"); the script holds only the starting value 0. Reading it as the constant 0
// would make "you gain life equal to the life lost this way" gain nothing.
#[test]
fn a_variable_an_effect_sets_is_not_its_starting_value() {
    let card = "Name:Test Tithe\n\
         ManaCost:3 B\n\
         Types:Sorcery\n\
         A:SP$ LoseLife | Cost$ 3 B | Defined$ Player.Opponent | LifeAmount$ 3 | SubAbility$ DBGainLife | SpellDescription$ Each opponent loses 3 life. You gain life equal to the life lost this way.\n\
         SVar:DBGainLife:DB$ GainLife | Defined$ You | LifeAmount$ AFLifeLost\n\
         SVar:AFLifeLost:Number$0\n\
         # AFLifeLost will be set by LoseLife\n\
         Oracle:Each opponent loses 3 life. You gain life equal to the life lost this way.";
    let b = forge_test_blockers(card);
    assert_eq!(b, ["expr:set by an effect AFLifeLost"]);
}
