//! Card data, as RON files (spec §7), embedded by `build.rs`:
//! - `data/<letter>/<slug>.ron` — self-contained cards: invented "Test …" cards, token
//!   definitions and basic lands ([`CARDS`]).
//! - `scripts/<letter>/<slug>.ron` — real cards: our encoding of their behaviour only,
//!   keyed by Scryfall oracle id ([`SCRIPTS`]). Printed data (cost, types, P/T) is fetched
//!   from Scryfall at runtime into the user's cache; none of it lives in this repository
//!   (CLAUDE.md invariant 13).
//! - `decks/*.ron` — deck lists ([`DECKS`]).
//!
//! This crate only embeds the files; `card-db` parses and validates them.

include!(concat!(env!("OUT_DIR"), "/embedded.rs"));

/// Contents of the deck file named `file` (e.g. `"m1_red_green.ron"`).
pub fn deck(file: &str) -> Option<&'static str> {
    DECKS
        .iter()
        .find(|(p, _)| p.ends_with(&format!("/{file}")))
        .map(|(_, t)| *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_embedded_in_sorted_order() {
        assert!(CARDS.len() >= 70, "{} cards", CARDS.len());
        assert!(DECKS.len() >= 11);
        for list in [CARDS, SCRIPTS, DECKS] {
            assert!(
                list.windows(2).all(|w| w[0].0 < w[1].0),
                "sorted, no duplicates"
            );
        }
        assert!(deck("precon_wretched_ranks.ron").is_some());
    }
}
