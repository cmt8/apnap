//! Card data, as RON files (spec §7): `data/<first-letter>/<slug>.ron` card scripts and
//! `decks/*.ron` deck lists. This crate only embeds the files; `card-db` parses and validates
//! them. Only invented "Test …" cards and basic lands live here: no Wizards/Scryfall assets
//! (CLAUDE.md invariant 13). A test checks this list matches the files on disk.

macro_rules! card {
    ($p:literal) => {
        (concat!("crates/cards/data/", $p), include_str!(concat!("../data/", $p)))
    };
}

macro_rules! deck {
    ($p:literal) => {
        (concat!("crates/cards/decks/", $p), include_str!(concat!("../decks/", $p)))
    };
}

/// `(path, contents)` of every card script.
pub const CARDS: &[(&str, &str)] = &[
    card!("f/forest.ron"),
    card!("m/mountain.ron"),
    card!("t/test_behemoth.ron"),
    card!("t/test_blaze.ron"),
    card!("t/test_bruiser.ron"),
    card!("t/test_brute.ron"),
    card!("t/test_charger.ron"),
    card!("t/test_charm.ron"),
    card!("t/test_dual.ron"),
    card!("t/test_edict.ron"),
    card!("t/test_elf.ron"),
    card!("t/test_giant.ron"),
    card!("t/test_golem.ron"),
    card!("t/test_grazer.ron"),
    card!("t/test_pinger.ron"),
    card!("t/test_prism.ron"),
    card!("t/test_quickling.ron"),
    card!("t/test_raider.ron"),
    card!("t/test_riot.ron"),
    card!("t/test_shock.ron"),
    card!("t/test_snuff.ron"),
    card!("t/test_spark.ron"),
    card!("t/test_sprout.ron"),
    card!("t/test_wisp.ron"),
];

/// `(path, contents)` of every deck list.
pub const DECKS: &[(&str, &str)] = &[
    deck!("m0_vanilla.ron"),
    deck!("m1_red_green.ron"),
];

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

    fn ron_files(dir: &str) -> Vec<String> {
        fn walk(d: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "ron") {
                    out.push(p.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
        let mut out = Vec::new();
        walk(&base, &mut out);
        let prefix = base.to_string_lossy().replace('\\', "/");
        let mut rel: Vec<String> = out
            .into_iter()
            .map(|p| format!("crates/cards/{dir}{}", &p[prefix.len()..]))
            .collect();
        rel.sort();
        rel
    }

    #[test]
    fn embedded_lists_match_files_on_disk() {
        let cards: Vec<String> = CARDS.iter().map(|(p, _)| p.to_string()).collect();
        assert_eq!(cards, ron_files("data"), "update CARDS in crates/cards/src/lib.rs");
        let decks: Vec<String> = DECKS.iter().map(|(p, _)| p.to_string()).collect();
        assert_eq!(decks, ron_files("decks"), "update DECKS in crates/cards/src/lib.rs");
    }
}
