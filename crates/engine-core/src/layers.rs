//! Characteristic computation (CR 613). M0: the printed view only. The full layer system
//! (1a–7d, timestamps, dependencies) lands in M2 behind this same function, which is why
//! all engine code already goes through `chars` (CLAUDE.md invariant 3).

use crate::ids::ObjectId;
use crate::state::GameState;
use alloc::string::String;
use alloc::vec::Vec;
use card_model::{BasicLandType, CardType, ManaCost, Supertype};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Characteristics {
    pub name: String,
    pub mana_cost: Option<ManaCost>,
    pub supertypes: Vec<Supertype>,
    pub card_types: Vec<CardType>,
    pub land_types: Vec<BasicLandType>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}

impl Characteristics {
    pub fn is(&self, t: CardType) -> bool {
        self.card_types.contains(&t)
    }
}

/// The current characteristics of `id`.
pub fn chars(st: &GameState, id: ObjectId) -> Characteristics {
    let d = st.card_def(id);
    Characteristics {
        name: d.name.clone(),
        mana_cost: d.mana_cost.clone(),
        supertypes: d.supertypes.clone(),
        card_types: d.card_types.clone(),
        land_types: d.land_types.clone(),
        power: d.power,
        toughness: d.toughness,
    }
}
