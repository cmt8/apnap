//! Choices of a creature type or a color (CR 614.12a, 607.2d (2026-11-06)). Text:
//! docs/rules-notes/614.12a-607.2d-chosen-type-color.md.
//!
//! A permanent's choice ("as this enters, choose a creature type") is stored on the object
//! (`GameObject::chosen`), a choice made while a spell or ability resolves in its resolve
//! frame. A selector that says "of the chosen type" (`Selector::chosen`) is *bound* before
//! it is matched: the reference is replaced by the chosen value itself, so every reader
//! (layers, triggers, replacement effects, costs, resolution) compares the same way.

use crate::decision::OptionDesc;
use crate::ids::{ObjectId, PlayerId};
use crate::state::GameState;
use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec::Vec;
use card_model::{CardType, ChoiceOf, Chosen, Color, CreatureType, OfChosen, Selector};

/// `sel` with its reference to a choice replaced by the value chosen (one of `chosen`). With
/// nothing of that kind chosen (CR 607.5a: undefined) the reference stays, and then "of the
/// chosen …" matches nothing and "not of the chosen type" everything (`layers::matches_parts`).
pub fn bind<'a>(sel: &'a Selector, chosen: &[Chosen]) -> Cow<'a, Selector> {
    let Some(of) = sel.chosen else {
        return Cow::Borrowed(sel);
    };
    let ty = chosen.iter().find_map(|c| match c {
        Chosen::CreatureType(t) => Some(*t),
        Chosen::Color(_) => None,
    });
    let color = chosen.iter().find_map(|c| match c {
        Chosen::Color(c) => Some(*c),
        Chosen::CreatureType(_) => None,
    });
    let mut s = sel.clone();
    match (of, ty, color) {
        (OfChosen::Type, Some(t), _) => s.subtypes = alloc::vec![t.name().to_string()],
        (OfChosen::NotType, Some(t), _) => s.not_subtypes.push(t.name().to_string()),
        (OfChosen::Color, _, Some(c)) => s.colors = alloc::vec![c],
        _ => return Cow::Borrowed(sel),
    }
    s.chosen = None;
    Cow::Owned(s)
}

/// What was chosen for `source` as it entered — the object's own choice while it exists;
/// once it has left, its last known choice as an ability on the stack remembers it.
pub fn of_source(st: &GameState, source: ObjectId) -> &[Chosen] {
    if let Some(o) = st.objects.get(&source) {
        return &o.chosen;
    }
    st.stack
        .iter()
        .rev()
        .find(|s| {
            matches!(s.kind, crate::stack::StackKind::Ability { source: x, .. } if x == source)
        })
        .map_or(&[], |s| s.chosen.as_slice())
}

/// [`bind`] with the choice of the ability's source `source`.
pub fn bind_for<'a>(st: &GameState, sel: &'a Selector, source: ObjectId) -> Cow<'a, Selector> {
    if sel.chosen.is_none() {
        return Cow::Borrowed(sel);
    }
    bind(sel, of_source(st, source))
}

/// The options for `player` choosing `what`, in a stable order (CLAUDE.md invariant 5).
/// A creature type: every one of CR 205.3m may be chosen; those among `player`'s own cards
/// (in every zone — their own deck is known to them) come first, the most common first,
/// then the rest alphabetically. A color: the five, in WUBRG order (CR 105.4).
pub fn options(st: &GameState, player: PlayerId, what: ChoiceOf) -> Vec<OptionDesc> {
    match what {
        ChoiceOf::Color => Color::ALL
            .iter()
            .map(|&color| OptionDesc::Color { color })
            .collect(),
        ChoiceOf::CreatureType => {
            let mut count: BTreeMap<CreatureType, u32> = BTreeMap::new();
            for o in st.objects.values().filter(|o| o.owner == player) {
                let def = st.catalog.get(o.card);
                if !(def.has_type(CardType::Creature) || def.has_type(CardType::Kindred))
                    || def.has_keyword(card_model::Keyword::Changeling)
                {
                    continue;
                }
                for t in def.creature_types.iter().filter_map(|n| CreatureType::named(n)) {
                    *count.entry(t).or_default() += 1;
                }
            }
            let mut present: Vec<(CreatureType, u32)> = count.into_iter().collect();
            present.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.name().cmp(b.0.name())));
            let mut out: Vec<OptionDesc> = present
                .iter()
                .map(|(name, _)| OptionDesc::CreatureType { name: *name })
                .collect();
            out.extend(
                CreatureType::all()
                    .filter(|t| !present.iter().any(|(p, _)| p == t))
                    .map(|name| OptionDesc::CreatureType { name }),
            );
            out
        }
    }
}

/// The value an answer to [`options`] chose.
pub fn answer(o: &OptionDesc) -> Chosen {
    match o {
        OptionDesc::CreatureType { name } => Chosen::CreatureType(*name),
        OptionDesc::Color { color } => Chosen::Color(*color),
        other => unreachable!("not a creature type or a color: {other:?}"),
    }
}
