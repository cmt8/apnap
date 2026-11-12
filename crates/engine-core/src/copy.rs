//! Copies (CR 707 (2026-11-06)). Text: docs/rules-notes/707-copies.md.
//!
//! An object's **copiable values** (CR 707.2) are the printed values of one card (or token
//! definition, or face) plus the exceptions of the copy effects that made them (CR 707.9).
//! An object that is a copy carries them in [`GameObject::copy`]: layer 1a (CR 613.2a) is
//! "start from these instead of the printed card" — [`base`] is where the layer walk and
//! [`crate::layers::chars`] begin. The values are fixed when the copy effect begins: a
//! later change to the original doesn't change the copy (CR 707.2b), and a copy of the
//! copy copies them, exceptions included (CR 707.3, 707.9a–b). Everything that is not a
//! copiable value — counters, status, Auras, other effects, being a commander — is not
//! copied, because it isn't part of them.
//!
//! An ability of a copy is named by where its text is written (`AbilityId`): the copied
//! card's abilities are that card's ids; one an exception adds ("except it has this
//! ability") is an entry of the `granted` list of the card the copy effect is written on.

use crate::ids::{ObjectId, PlayerId};
use crate::layers::Characteristics;
use crate::objects::GameObject;
use crate::state::GameState;
use alloc::vec::Vec;
use card_model::{
    AbilityDef, AbilityId, Affected, BasicLandType, CardCatalog, CardDef, CardDefId,
    ContinuousEffect, CopyExcept, Supertype,
};
use serde::{Deserialize, Serialize};

/// The source of a token's own "enters …" replacement effects (a token that's a copy has
/// the copied text's, CR 707.5) while it is entering: it isn't an object yet (D-073).
pub const ENTERING_TOKEN: ObjectId = ObjectId(u32::MAX);

/// One exception of a copy effect (CR 707.9), with the card the copy effect is written on
/// (an ability it adds is named by that card).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exception {
    pub card: CardDefId,
    pub what: CopyExcept,
}

/// CR 707.2: copiable values — the printed values of `card`, as modified by the exceptions
/// of the copy effects that made them, in the order they were made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Copiable {
    pub card: CardDefId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub except: Vec<Exception>,
}

impl Copiable {
    /// These values with more exceptions, written on `card` (CR 707.9b: the final values
    /// become the copy's copiable values).
    pub fn with(mut self, card: CardDefId, except: &[CopyExcept]) -> Self {
        self.except.extend(except.iter().map(|what| Exception {
            card,
            what: what.clone(),
        }));
        self
    }
}

/// The copiable values of `id` (CR 707.2, 707.3): those of the copy effect it is under, or
/// what is printed on the face that is up (CR 707.8: a double-faced permanent is copied as
/// the face it shows; the catalog has an entry for each face, D-071).
pub fn values(st: &GameState, id: ObjectId) -> Copiable {
    of_object(&st.catalog, st.object(id))
}

/// [`values`] of an object at hand.
pub fn of_object(catalog: &CardCatalog, o: &GameObject) -> Copiable {
    match &o.copy {
        Some(c) => (**c).clone(),
        None => Copiable {
            card: catalog.face(o.card, o.face),
            except: Vec::new(),
        },
    }
}

/// The card (face) whose printed text `id` has: for a copy, the card it copies.
pub fn text_card(st: &GameState, id: ObjectId) -> CardDefId {
    let o = st.object(id);
    o.copy.as_ref().map_or_else(|| st.face_card(id), |c| c.card)
}

/// [`text_card`]'s definition.
pub fn text_def(st: &GameState, id: ObjectId) -> &CardDef {
    st.catalog.get(text_card(st, id))
}

/// The characteristics `id` starts the layer walk with (CR 613.1, 613.2a): its copiable
/// values, before any other effect — for an object that is no copy, its face `face`.
/// (A copy of the back face of a transforming card has no mana cost: mana value 0,
/// CR 202.3b.)
pub fn base(st: &GameState, id: ObjectId, face: u8, controller: PlayerId) -> Characteristics {
    let o = st.object(id);
    match &o.copy {
        None => Characteristics::of_face(st, id, face, controller),
        Some(c) => characteristics(&st.catalog, c, controller),
    }
}

/// The characteristics given by copiable values (CR 707.2, 707.9).
pub fn characteristics(
    catalog: &CardCatalog,
    v: &Copiable,
    controller: PlayerId,
) -> Characteristics {
    let mut c = Characteristics::printed(v.card, catalog.get(v.card), controller);
    for e in &v.except {
        match &e.what {
            CopyExcept::NotLegendary => c.supertypes.retain(|s| *s != Supertype::Legendary),
            CopyExcept::AddKeywords(ks) => {
                for k in ks {
                    if !c.keywords.contains(k) {
                        c.keywords.push(*k);
                    }
                }
            }
            CopyExcept::AddTypes {
                card_types,
                subtypes,
            } => {
                for t in card_types {
                    if !c.card_types.contains(t) {
                        c.card_types.push(*t);
                    }
                }
                for s in subtypes {
                    match land_type(s) {
                        Some(t) if !c.land_types.contains(&t) => c.land_types.push(t),
                        Some(_) => {}
                        None if !c.subtypes.contains(s) => c.subtypes.push(s.clone()),
                        None => {}
                    }
                }
            }
            // CR 707.9d: a characteristic-defining ability of the original that defines
            // power and toughness isn't copied.
            CopyExcept::SetPT { power, toughness } => {
                c.power = Some(*power);
                c.toughness = Some(*toughness);
                c.abilities.retain(|a| !defines_pt(catalog.ability(*a)));
            }
            // CR 707.9d: nor its color indicator (colors are set outright here).
            CopyExcept::SetColors(cs) => {
                c.colors = cs.clone();
                c.colors.sort();
            }
            CopyExcept::SetCreatureTypes(ts) => c.subtypes = ts.clone(),
            CopyExcept::HasAbilities(indices) => {
                c.abilities.extend(indices.iter().map(|&index| AbilityId {
                    card: e.card,
                    index,
                    granted: true,
                }))
            }
        }
    }
    c
}

/// A characteristic-defining ability that sets power and/or toughness (CR 604.3).
fn defines_pt(a: &AbilityDef) -> bool {
    matches!(a, AbilityDef::Static { affected: Affected::This, effects, .. }
        if effects.iter().any(|e| matches!(e, ContinuousEffect::SetPTBy { .. })))
}

fn land_type(name: &str) -> Option<BasicLandType> {
    Some(match name {
        "Plains" => BasicLandType::Plains,
        "Island" => BasicLandType::Island,
        "Swamp" => BasicLandType::Swamp,
        "Mountain" => BasicLandType::Mountain,
        "Forest" => BasicLandType::Forest,
        _ => return None,
    })
}

/// What comes with a token copy just created (`id`): the keywords it gains (not copiable,
/// CR 707.2: an effect of the instruction, for as long as it is on the battlefield) and a
/// delayed triggered ability about it (CR 603.7: "sacrifice it at the beginning of the
/// next end step").
pub(crate) fn token_extras(st: &mut GameState, c: &crate::replacement::TokenCopy, id: ObjectId) {
    use crate::actions::{GameAction, execute};
    if !c.gains.is_empty() {
        execute(
            st,
            GameAction::AddContinuous {
                source: c.source,
                controller: c.controller,
                affected: alloc::vec![id],
                effects: alloc::vec![ContinuousEffect::AddKeywords(c.gains.clone())],
                until_end_of_turn: false,
                card: c.card,
            },
        );
    }
    if let Some(ability) = c.at_end {
        execute(
            st,
            GameAction::AddDelayed {
                source: c.source,
                controller: c.controller,
                ability,
                about: Some(id),
            },
        );
    }
}

/// Is `id` a card (CR 108.2)? Not a token (CR 111.6) nor a copy of a spell (CR 707.10).
pub fn is_card(o: &GameObject) -> bool {
    !o.is_token && !o.spell_copy
}
