//! Alternative costs (CR 118.9, 601.2b, 601.2f (2026-11-06)): "rather than pay this spell's
//! mana cost", "without paying its mana cost", flashback (702.34a), evoke (702.74a),
//! overload (702.96a). Text: docs/rules-notes/118.9-alternative-costs.md; D-072.
//!
//! An alternative cost is chosen as the player begins to cast the spell — it is its own
//! priority option (`OptionDesc::CastWith`), so the choice is announced before anything
//! else (CR 601.2b) and is only offered when the cast could be completed with it (D-019).
//! It yields an ordinary mana cost and extra costs for the cast frame to pay: nothing in
//! the payment is specific to it. Commander tax and cost reductions still apply (CR 118.9d,
//! 903.8); with no mana cost there is no X to announce (CR 107.3b).

use crate::ids::{ObjectId, PlayerId};
use crate::state::GameState;
use crate::zones::Zone;
use alloc::vec::Vec;
use card_model::{AbilityDef, AbilityId, AltCost, AltKind, AltKindName, Body, Cost, ManaCost};

/// The card's own alternative costs, in printed order (CLAUDE.md invariant 5).
pub(crate) fn alternatives(st: &GameState, card: ObjectId) -> Vec<AbilityId> {
    let def = st.object(card).card;
    st.card_def(card)
        .abilities
        .iter()
        .enumerate()
        .filter(|(_, a)| matches!(a, AbilityDef::AlternativeCost(_)))
        .map(|(i, _)| AbilityId::printed(def, i as u16))
        .collect()
}

pub(crate) fn alt_cost(st: &GameState, ability: AbilityId) -> &AltCost {
    match st.catalog.ability(ability) {
        AbilityDef::AlternativeCost(a) => a,
        other => unreachable!("not an alternative cost: {other:?}"),
    }
}

/// CR 601.3: may `p` begin to cast `card` from where it is with `ability`? `normally`
/// says whether they may cast it from there at all (their hand, their commander, a rule
/// such as "you may cast spells from your graveyard"). Flashback is itself the
/// permission to cast the card from its owner's graveyard (CR 702.34a), and only there;
/// the other kinds work wherever the card may be cast. The condition ("if you control a
/// commander", "if it's not your turn") is checked now, as casting begins.
pub(crate) fn may_begin(
    st: &GameState,
    p: PlayerId,
    card: ObjectId,
    ability: AbilityId,
    normally: bool,
) -> bool {
    let a = alt_cost(st, ability);
    let o = st.object(card);
    let from = match a.kind {
        AltKind::Flashback => o.owner == p && o.zone == Zone::Graveyard,
        _ => normally,
    };
    from && crate::triggers::condition_holds(st, a.only_if.as_ref(), p, card)
}

/// CR 601.2f, 118.9c–d: the spell's cost with the alternative cost `ability` chosen: its
/// mana part replaces the mana cost (none: "without paying its mana cost"); its other parts
/// are paid together with the spell's additional costs. Overload changes the spell's text
/// (CR 702.96c): its body is the one without targets.
pub(super) fn apply(
    st: &GameState,
    ability: AbilityId,
    mana: &mut Option<ManaCost>,
    body: &mut Option<Body>,
    extra: &mut Cost,
) {
    let a = alt_cost(st, ability);
    *mana = a.cost.mana.clone();
    *extra = a
        .cost
        .with(extra)
        .expect("card-db refuses an alternative cost that clashes with the additional cost");
    if let AltKind::Overload(b) = &a.kind {
        *body = Some(b.clone());
    }
}

/// What the spell `spell` on the stack does: its text, as changed by the alternative cost
/// it was cast with (CR 702.96c: overload).
pub(crate) fn spell_body(st: &GameState, spell: ObjectId) -> Option<Body> {
    if let Some(a) = st.stack_object(spell).and_then(|s| s.alt)
        && let AltKind::Overload(b) = &alt_cost(st, a).kind
    {
        return Some(b.clone());
    }
    st.card_def(spell).spell_ability().cloned()
}

/// CR 702.34a: the flashback ability of a spell cast with it, which exiles the card
/// instead of putting it anywhere else as it leaves the stack.
pub(crate) fn exiled_on_leaving(st: &GameState, spell: ObjectId) -> Option<AbilityId> {
    let o = st.objects.get(&spell)?;
    if o.zone != Zone::Stack {
        return None;
    }
    let a = st.stack_object(spell)?.alt?;
    matches!(alt_cost(st, a).kind, AltKind::Flashback).then_some(a)
}

/// "If its evoke cost was paid" (CR 702.74a): `object` is the permanent that spell became.
pub(crate) fn cast_for(st: &GameState, object: ObjectId, kind: AltKindName) -> bool {
    st.objects
        .get(&object)
        .and_then(|o| o.cast_with)
        .is_some_and(|a| match (kind, &alt_cost(st, a).kind) {
            (AltKindName::Evoke, AltKind::Evoke) => true,
            (AltKindName::Evoke, _) => false,
        })
}
