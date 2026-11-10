//! Every card that compiles from Forge's scripts, checked against Scryfall's printed data,
//! and the per-game catalogs built from them.
//!
//! A game's `CardCatalog` is part of its state (saved and hashed with it), so it holds only
//! the cards in that game's decks and the tokens those cards can make — never the whole
//! library.

use super::translate::{Blockers, Compiled, Known, compile};
use crate::real::parse_type_line;
use crate::scryfall::{ScryfallCard, ScryfallData};
use card_model::{CardCatalog, CardDef};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Library {
    /// Compiled cards by name.
    pub cards: BTreeMap<String, Compiled>,
    /// Compiled token definitions by token script name (their catalog key).
    pub tokens: BTreeMap<String, Compiled>,
    /// Cards that don't compile, with what blocks each.
    pub blocked: BTreeMap<String, Blockers>,
    /// Token scripts that don't compile.
    pub blocked_tokens: BTreeMap<String, Blockers>,
    /// Scripts that couldn't even be read.
    pub unreadable: usize,
    pub known: Known,
}

/// Where the compiled card disagrees with Scryfall's printed data for the same name. An
/// independent check: Forge's script and Scryfall's record were written by different people.
pub fn printed_mismatch(def: &CardDef, card: &ScryfallCard) -> Vec<String> {
    let mut out = Vec::new();
    let cost = def
        .mana_cost
        .as_ref()
        .map_or(String::new(), |m| m.to_text());
    if cost != card.mana_cost.clone().unwrap_or_default() {
        out.push("printed:mana cost".to_string());
    }
    match parse_type_line(&card.type_line) {
        Ok((supertypes, card_types, land_types, subtypes)) => {
            let set = |v: Vec<String>| v.into_iter().collect::<BTreeSet<_>>();
            let names = |v: &[String]| set(v.to_vec());
            if set(def.supertypes.iter().map(|s| format!("{s:?}")).collect())
                != set(supertypes.iter().map(|s| format!("{s:?}")).collect())
                || set(def.card_types.iter().map(|s| format!("{s:?}")).collect())
                    != set(card_types.iter().map(|s| format!("{s:?}")).collect())
                || set(def.land_types.iter().map(|s| format!("{s:?}")).collect())
                    != set(land_types.iter().map(|s| format!("{s:?}")).collect())
                || names(&def.creature_types) != names(&subtypes)
            {
                out.push("printed:type line".to_string());
            }
        }
        Err(_) => out.push("printed:type line unreadable".to_string()),
    }
    let pt = |n: Option<i32>| n.map(|n| n.to_string());
    if pt(def.power) != card.power || pt(def.toughness) != card.toughness {
        out.push("printed:power/toughness".to_string());
    }
    out
}

/// What the definition checks every hand-written card passes (`crate::validate`) say about
/// a compiled one, as blockers: an instant without a spell ability, an effect that refers
/// to a target slot of the wrong kind… The compiler shouldn't produce these; if it does,
/// the card must not load.
fn invalid(def: &CardDef) -> Blockers {
    let script = card_model::CardScript {
        verified: true,
        tests: vec!["forge_features".to_string()],
        card: def.clone(),
    };
    crate::validate(&script, crate::Strictness::Strict)
        .into_iter()
        .map(|p| format!("invalid:{p}"))
        .collect()
}

impl Library {
    /// Compile every script. With `scryfall`, a card whose printed characteristics disagree
    /// with Scryfall's is blocked rather than loaded.
    pub fn build(
        card_scripts: &[(String, String)],
        token_scripts: &[(String, String)],
        scryfall: Option<&ScryfallData>,
    ) -> Library {
        let known = Known::from_scripts(
            card_scripts
                .iter()
                .chain(token_scripts)
                .map(|(_, t)| t.as_str()),
        );
        let mut lib = Library {
            known,
            ..Library::default()
        };
        for (stem, text) in token_scripts {
            match super::parse(text).map_err(|e| Blockers::from([format!("unreadable: {e}")])) {
                Ok(script) => match compile(&script, &lib.known) {
                    Ok(mut c) => {
                        c.def.token = true;
                        c.def.key = Some(stem.clone());
                        let problems = invalid(&c.def);
                        if problems.is_empty() {
                            lib.tokens.insert(stem.clone(), c);
                        } else {
                            lib.blocked_tokens.insert(stem.clone(), problems);
                        }
                    }
                    Err(b) => {
                        lib.blocked_tokens.insert(stem.clone(), b);
                    }
                },
                Err(b) => {
                    lib.blocked_tokens.insert(stem.clone(), b);
                }
            }
        }
        for (_, text) in card_scripts {
            let Ok(script) = super::parse(text) else {
                lib.unreadable += 1;
                continue;
            };
            let name = script.faces[0].name.clone();
            let result = compile(&script, &lib.known).and_then(|c| {
                let mut blockers = invalid(&c.def);
                if let Some(card) = scryfall.and_then(|d| d.get(&name)) {
                    blockers.extend(printed_mismatch(&c.def, card));
                }
                if blockers.is_empty() {
                    Ok(c)
                } else {
                    Err(blockers)
                }
            });
            match result {
                Ok(c) => {
                    lib.cards.insert(name, c);
                }
                Err(b) => {
                    lib.blocked.insert(name, b);
                }
            }
        }
        // A card is only usable if every token it can make is.
        loop {
            let broken: Vec<(String, String)> = lib
                .cards
                .iter()
                .flat_map(|(n, c)| c.tokens.iter().map(move |t| (n.clone(), t.clone())))
                .filter(|(_, t)| !lib.tokens.contains_key(t))
                .collect();
            let broken_tokens: Vec<(String, String)> = lib
                .tokens
                .iter()
                .flat_map(|(n, c)| c.tokens.iter().map(move |t| (n.clone(), t.clone())))
                .filter(|(_, t)| !lib.tokens.contains_key(t))
                .collect();
            if broken.is_empty() && broken_tokens.is_empty() {
                break;
            }
            for (name, token) in broken {
                lib.cards.remove(&name);
                lib.blocked
                    .entry(name)
                    .or_default()
                    .insert(format!("token:{token}"));
            }
            for (name, token) in broken_tokens {
                lib.tokens.remove(&name);
                lib.blocked_tokens
                    .entry(name)
                    .or_default()
                    .insert(format!("token:{token}"));
            }
        }
        lib
    }

    /// Read the scripts from the cache directories and compile them.
    pub fn load(
        cards_dir: &Path,
        tokens_dir: &Path,
        scryfall: Option<&ScryfallData>,
    ) -> Result<Library, String> {
        let cards = super::read_scripts(cards_dir).map_err(|e| e.to_string())?;
        if cards.is_empty() {
            return Err(super::FETCH_HINT.to_string());
        }
        let tokens = super::read_scripts(tokens_dir).map_err(|e| e.to_string())?;
        Ok(Library::build(&cards, &tokens, scryfall))
    }

    /// The catalog for a game: `base` (the self-contained cards — basic lands, test cards)
    /// plus the named cards and every token they can make. Names already in `base` come
    /// from there. `Err` lists the names that aren't available, each with its blockers.
    pub fn catalog_for<'a>(
        &self,
        base: &CardCatalog,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<CardCatalog, Vec<String>> {
        let mut catalog = base.clone();
        let mut missing = Vec::new();
        let mut tokens: BTreeSet<&str> = BTreeSet::new();
        let wanted: BTreeSet<&str> = names.into_iter().collect();
        for name in wanted {
            if catalog.find(name).is_some() {
                continue;
            }
            match self.cards.get(name) {
                Some(c) => {
                    tokens.extend(c.tokens.iter().map(String::as_str));
                    catalog.add(c.def.clone());
                }
                None => missing.push(match self.blocked.get(name) {
                    Some(b) => format!(
                        "{name}: {}",
                        b.iter().cloned().collect::<Vec<_>>().join(", ")
                    ),
                    None => format!("{name}: Forge has no script"),
                }),
            }
        }
        // Tokens, and the tokens of tokens.
        let mut queue: Vec<&str> = tokens.into_iter().collect();
        let mut added: BTreeSet<&str> = BTreeSet::new();
        while let Some(t) = queue.pop() {
            if !added.insert(t) || catalog.find(t).is_some() {
                continue;
            }
            // `build` guarantees every token of a compiled card is here.
            let c = &self.tokens[t];
            queue.extend(c.tokens.iter().map(String::as_str));
            catalog.add(c.def.clone());
        }
        if missing.is_empty() {
            Ok(catalog)
        } else {
            Err(missing)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripts(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    // Invented cards and tokens in Forge's format.
    const MAKER: &str = "Name:Test Hatchery\nManaCost:2 G\nTypes:Enchantment\nT:Mode$ Phase | Phase$ Upkeep | ValidPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigToken\nSVar:TrigToken:DB$ Token | TokenScript$ g_1_1_test_sprite | TokenOwner$ You";
    const BEAR: &str = "Name:Test Bear\nManaCost:1 G\nTypes:Creature Bear\nPT:2/2";
    const ODD: &str = "Name:Test Oddity\nManaCost:1 U\nTypes:Sorcery\nA:SP$ Subgame | Who$ Knows";
    const NEEDS_MISSING: &str = "Name:Test Orphanage\nManaCost:1 W\nTypes:Sorcery\nA:SP$ Token | TokenScript$ w_9_9_missing";
    const SPRITE: &str = "Name:Sprite Token\nManaCost:no cost\nColors:green\nTypes:Creature Sprite\nPT:1/1\nK:Flying";

    #[test]
    fn a_game_catalog_holds_only_its_cards_and_their_tokens() {
        let lib = Library::build(
            &scripts(&[("a", MAKER), ("b", BEAR), ("c", ODD), ("d", NEEDS_MISSING)]),
            &scripts(&[("g_1_1_test_sprite", SPRITE)]),
            None,
        );
        assert_eq!(lib.cards.len(), 2);
        assert!(lib.blocked["Test Oddity"].contains("api:Subgame"));
        // A card whose token doesn't exist is not usable.
        assert!(lib.blocked["Test Orphanage"].contains("token:w_9_9_missing"));
        let base = CardCatalog::default();
        let catalog = lib.catalog_for(&base, ["Test Hatchery"]).unwrap();
        // The enchantment and its token; not the Bear.
        assert_eq!(catalog.defs.len(), 2);
        let token = catalog.get(catalog.find("g_1_1_test_sprite").unwrap());
        assert!(token.token);
        assert_eq!(token.name, "Sprite Token");
        assert_eq!(token.colors, Some(vec![card_model::Color::Green]));
        assert!(catalog.find("Test Bear").is_none());
        // Asking for something that doesn't compile says why.
        let err = lib
            .catalog_for(&base, ["Test Bear", "Test Oddity", "Nonesuch"])
            .unwrap_err();
        assert_eq!(err.len(), 2);
        assert!(
            err.iter()
                .any(|e| e.starts_with("Test Oddity: api:Subgame"))
        );
        assert!(err.iter().any(|e| e == "Nonesuch: Forge has no script"));
    }

    #[test]
    fn printed_data_is_checked_against_scryfall() {
        let card = |cost: &str, types: &str, pt: Option<(&str, &str)>| -> ScryfallCard {
            serde_json::from_value(serde_json::json!({
                "name": "Test Bear", "mana_cost": cost, "type_line": types,
                "power": pt.map(|p| p.0), "toughness": pt.map(|p| p.1),
            }))
            .unwrap()
        };
        let known = Known::from_scripts([BEAR].into_iter());
        let def = compile(&super::super::parse(BEAR).unwrap(), &known)
            .unwrap()
            .def;
        assert!(
            printed_mismatch(&def, &card("{1}{G}", "Creature — Bear", Some(("2", "2")))).is_empty()
        );
        assert_eq!(
            printed_mismatch(&def, &card("{2}{G}", "Creature — Bear", Some(("2", "2")))),
            ["printed:mana cost"]
        );
        assert_eq!(
            printed_mismatch(
                &def,
                &card("{1}{G}", "Legendary Creature — Bear", Some(("2", "2")))
            ),
            ["printed:type line"]
        );
        assert_eq!(
            printed_mismatch(&def, &card("{1}{G}", "Creature — Bear", Some(("2", "3")))),
            ["printed:power/toughness"]
        );
        // In a library, such a card is blocked.
        let mut data = ScryfallData::default();
        data.cards.insert(
            "Test Bear".into(),
            card("{1}{G}", "Creature — Bear", Some(("3", "3"))),
        );
        let lib = Library::build(&scripts(&[("b", BEAR)]), &[], Some(&data));
        assert!(lib.cards.is_empty());
        assert!(lib.blocked["Test Bear"].contains("printed:power/toughness"));
    }
}
