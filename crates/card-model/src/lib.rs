//! Card vocabulary shared by the engine, the card database and card scripts.
//!
//! `no_std` + `alloc`: this crate is a dependency of `engine-core` and must stay pure.
//! All enums are **externally tagged** (serde default) because RON cannot express
//! internally tagged or untagged enums (spec §7.2, CLAUDE.md invariant 10).
//!
//! M0 scope: printed characteristics only (vanilla creatures, basic lands). The ability
//! vocabulary (`AbilityDef`, `Effect`, ...) arrives in M1/M2.
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

/// Index of a [`CardDef`] inside a [`CardCatalog`]. Dense, assigned in catalog order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardDefId(pub u32);

/// The five colors (CR 105.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
}

impl Color {
    pub const ALL: [Color; 5] = [
        Color::White,
        Color::Blue,
        Color::Black,
        Color::Red,
        Color::Green,
    ];
}

/// One symbol of a printed mana cost (CR 107.4). Hybrid/Phyrexian/X arrive with M1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ManaSymbol {
    Generic(u32),
    Colored(Color),
    /// `{C}`: specifically colorless mana.
    Colorless,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ManaCost {
    pub symbols: Vec<ManaSymbol>,
}

impl ManaCost {
    /// Mana value (CR 202.3).
    pub fn mana_value(&self) -> u32 {
        self.symbols
            .iter()
            .map(|s| match s {
                ManaSymbol::Generic(n) => *n,
                ManaSymbol::Colored(_) | ManaSymbol::Colorless => 1,
            })
            .sum()
    }

    /// Parse the textual form used by Oracle text, e.g. `"{2}{G}{G}"`.
    pub fn parse(text: &str) -> Result<ManaCost, ParseManaCostError> {
        let mut symbols = Vec::new();
        let mut rest = text.trim();
        while !rest.is_empty() {
            let Some(body) = rest.strip_prefix('{') else {
                return Err(ParseManaCostError);
            };
            let Some(end) = body.find('}') else {
                return Err(ParseManaCostError);
            };
            let sym = &body[..end];
            rest = &body[end + 1..];
            symbols.push(match sym {
                "W" => ManaSymbol::Colored(Color::White),
                "U" => ManaSymbol::Colored(Color::Blue),
                "B" => ManaSymbol::Colored(Color::Black),
                "R" => ManaSymbol::Colored(Color::Red),
                "G" => ManaSymbol::Colored(Color::Green),
                "C" => ManaSymbol::Colorless,
                n => ManaSymbol::Generic(n.parse().map_err(|_| ParseManaCostError)?),
            });
        }
        Ok(ManaCost { symbols })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseManaCostError;

impl fmt::Display for ParseManaCostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid mana cost")
    }
}

impl core::error::Error for ParseManaCostError {}

/// Card types (CR 205.2a).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CardType {
    Artifact,
    Battle,
    Creature,
    Enchantment,
    Instant,
    Kindred,
    Land,
    Planeswalker,
    Sorcery,
}

/// Supertypes (CR 205.4a).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Supertype {
    Basic,
    Legendary,
    Ongoing,
    Snow,
    World,
}

/// Basic land types (CR 205.3i). Each grants an intrinsic mana ability (CR 305.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BasicLandType {
    Plains,
    Island,
    Swamp,
    Mountain,
    Forest,
}

impl BasicLandType {
    /// CR 305.6: Plains → W, Island → U, Swamp → B, Mountain → R, Forest → G.
    pub fn produces(self) -> Color {
        match self {
            BasicLandType::Plains => Color::White,
            BasicLandType::Island => Color::Blue,
            BasicLandType::Swamp => Color::Black,
            BasicLandType::Mountain => Color::Red,
            BasicLandType::Forest => Color::Green,
        }
    }
}

/// Immutable printed card data. The engine never reads this directly for objects on the
/// battlefield or stack; it goes through `engine_core::layers::chars` (CLAUDE.md invariant 3).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardDef {
    pub name: String,
    pub mana_cost: Option<ManaCost>,
    pub supertypes: Vec<Supertype>,
    pub card_types: Vec<CardType>,
    pub land_types: Vec<BasicLandType>,
    pub creature_types: Vec<String>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}

impl CardDef {
    pub fn has_type(&self, t: CardType) -> bool {
        self.card_types.contains(&t)
    }
}

/// The set of card definitions a game uses. Stored once per game (behind an `Arc`) and
/// referenced by [`CardDefId`] from every object, so saves are self-contained without
/// duplicating card data per object (docs/decisions.md D-005).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardCatalog {
    pub defs: Vec<CardDef>,
}

impl CardCatalog {
    pub fn get(&self, id: CardDefId) -> &CardDef {
        &self.defs[id.0 as usize]
    }

    pub fn add(&mut self, def: CardDef) -> CardDefId {
        let id = CardDefId(u32::try_from(self.defs.len()).expect("catalog overflow"));
        self.defs.push(def);
        id
    }

    pub fn find(&self, name: &str) -> Option<CardDefId> {
        self.defs
            .iter()
            .position(|d| d.name == name)
            .map(|i| CardDefId(i as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mana_cost() {
        let c = ManaCost::parse("{2}{G}{G}").unwrap();
        assert_eq!(
            c.symbols,
            [
                ManaSymbol::Generic(2),
                ManaSymbol::Colored(Color::Green),
                ManaSymbol::Colored(Color::Green)
            ]
        );
        assert_eq!(c.mana_value(), 4);
        assert!(ManaCost::parse("{2}{G").is_err());
        assert!(ManaCost::parse("G").is_err());
    }

    #[test]
    fn externally_tagged_roundtrip() {
        let c = ManaCost::parse("{1}{C}{R}").unwrap();
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(
            s,
            r#"{"symbols":[{"Generic":1},"Colorless",{"Colored":"Red"}]}"#
        );
        assert_eq!(serde_json::from_str::<ManaCost>(&s).unwrap(), c);
    }
}
