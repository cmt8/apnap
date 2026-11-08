//! Characteristic computation (CR 613). Until M2 this is the printed view only. The full
//! layer system (1a–7d, timestamps, dependencies) lands in M2 behind this same function,
//! which is why all engine code already goes through `chars` (CLAUDE.md invariant 3).
//!
//! `Characteristics` borrows from the game state; M2 will back it with the memoized layered
//! view instead of the printed `CardDef`.

use crate::ids::ObjectId;
use crate::state::GameState;
use card_model::{AbilityDef, BasicLandType, CardType, Keyword, ManaCost, Supertype};

#[derive(Clone, Copy, Debug)]
pub struct Characteristics<'a> {
    pub name: &'a str,
    pub mana_cost: Option<&'a ManaCost>,
    pub supertypes: &'a [Supertype],
    pub card_types: &'a [CardType],
    pub land_types: &'a [BasicLandType],
    pub keywords: &'a [Keyword],
    pub abilities: &'a [AbilityDef],
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}

impl Characteristics<'_> {
    pub fn is(&self, t: CardType) -> bool {
        self.card_types.contains(&t)
    }

    pub fn has(&self, k: Keyword) -> bool {
        self.keywords.contains(&k)
    }

    /// CR 110.4: permanent card types.
    pub fn is_permanent_type(&self) -> bool {
        [
            CardType::Artifact,
            CardType::Battle,
            CardType::Creature,
            CardType::Enchantment,
            CardType::Land,
            CardType::Planeswalker,
        ]
        .iter()
        .any(|t| self.is(*t))
    }
}

/// The current characteristics of card object `id`.
pub fn chars(st: &GameState, id: ObjectId) -> Characteristics<'_> {
    let d = st.card_def(id);
    Characteristics {
        name: &d.name,
        mana_cost: d.mana_cost.as_ref(),
        supertypes: &d.supertypes,
        card_types: &d.card_types,
        land_types: &d.land_types,
        keywords: &d.keywords,
        abilities: &d.abilities,
        power: d.power,
        toughness: d.toughness,
    }
}
