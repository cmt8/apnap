//! Card data.
//!
//! M0 ships only an **invented** vanilla test pool, built in Rust: basic lands (whose names
//! are basic land types, CR 305.6) and creatures with made-up "Test …" names. Real cards
//! become RON scripts in `cards/<letter>/<slug>.ron` from M1/M4, joined to Scryfall data
//! fetched at runtime (spec §12). No Wizards/Scryfall assets live in this repo.

use card_model::{BasicLandType, CardCatalog, CardDef, CardDefId, CardType, ManaCost, Supertype};

fn basic_land(name: &str, t: BasicLandType) -> CardDef {
    CardDef {
        name: name.into(),
        mana_cost: None,
        supertypes: vec![Supertype::Basic],
        card_types: vec![CardType::Land],
        land_types: vec![t],
        creature_types: vec![],
        power: None,
        toughness: None,
    }
}

fn vanilla(name: &str, cost: &str, power: i32, toughness: i32) -> CardDef {
    CardDef {
        name: name.into(),
        mana_cost: Some(ManaCost::parse(cost).expect("valid cost")),
        supertypes: vec![],
        card_types: vec![CardType::Creature],
        land_types: vec![],
        creature_types: vec!["Construct".into()],
        power: Some(power),
        toughness: Some(toughness),
    }
}

/// The M0 test pool: name, cost, power, toughness.
pub const M0_CREATURES: &[(&str, &str, i32, i32)] = &[
    ("Test Sprout", "{G}", 1, 1),
    ("Test Bruiser", "{1}{G}", 2, 2),
    ("Test Grazer", "{2}{G}", 3, 3),
    ("Test Behemoth", "{4}{G}{G}", 6, 6),
    ("Test Spark", "{R}", 1, 1),
    ("Test Raider", "{1}{R}", 2, 1),
    ("Test Brute", "{2}{R}", 3, 2),
    ("Test Giant", "{3}{R}{R}", 5, 4),
    ("Test Golem", "{3}", 2, 3),
    // Exercises CR 704.5f (toughness 0) every time it resolves.
    ("Test Wisp", "{1}", 1, 0),
];

/// Catalog with the two basic lands followed by `M0_CREATURES`.
pub fn m0_catalog() -> CardCatalog {
    let mut c = CardCatalog::default();
    c.add(basic_land("Forest", BasicLandType::Forest));
    c.add(basic_land("Mountain", BasicLandType::Mountain));
    for &(name, cost, p, t) in M0_CREATURES {
        c.add(vanilla(name, cost, p, t));
    }
    c
}

/// A 60-card red-green vanilla deck: 12 Forest, 12 Mountain, and 36 creatures
/// (4 each of eight test creatures, 2 Test Behemoth, 2 Test Wisp).
pub fn m0_deck(catalog: &CardCatalog) -> Vec<CardDefId> {
    let id = |n: &str| {
        catalog
            .find(n)
            .unwrap_or_else(|| panic!("{n} not in catalog"))
    };
    let mut deck = Vec::with_capacity(60);
    deck.extend(std::iter::repeat_n(id("Forest"), 12));
    deck.extend(std::iter::repeat_n(id("Mountain"), 12));
    for &(name, ..) in M0_CREATURES {
        let copies = match name {
            "Test Behemoth" => 2,
            "Test Wisp" => 2,
            _ => 4,
        };
        deck.extend(std::iter::repeat_n(id(name), copies));
    }
    assert_eq!(deck.len(), 60);
    deck
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_is_sixty_and_names_are_unique() {
        let c = m0_catalog();
        assert_eq!(m0_deck(&c).len(), 60);
        for (i, d) in c.defs.iter().enumerate() {
            assert_eq!(c.find(&d.name), Some(CardDefId(i as u32)));
        }
    }
}
