//! Targets (CR 115): who/what may be chosen, and whether a chosen target is still legal on
//! resolution (CR 608.2b). One function decides both, so choosing and checking can't drift.

use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::stack::{StackKind, Target};
use crate::state::GameState;
use crate::zones::Zone;
use alloc::vec::Vec;
use card_model::{CardType, Keyword, TargetFilter};

/// Whether `t` satisfies `filter` for a spell/ability controlled by `controller` whose own
/// stack object is `this` (CR 115.5: it can't target itself).
pub fn is_legal(
    st: &GameState,
    filter: &TargetFilter,
    t: Target,
    controller: PlayerId,
    this: Option<ObjectId>,
) -> bool {
    match t {
        Target::Player(p) => {
            st.in_game(p)
                && match filter {
                    TargetFilter::AnyTarget | TargetFilter::Player => true,
                    TargetFilter::Opponent => p != controller,
                    TargetFilter::Creature
                    | TargetFilter::Spell
                    | TargetFilter::SpellMatching(_)
                    | TargetFilter::Permanent(_)
                    | TargetFilter::CardInGraveyard { .. } => false,
                }
        }
        Target::Object(id) => {
            if Some(id) == this {
                return false;
            }
            // CR 400.7 / 608.2b: an object that changed zones is a new object, so a missing
            // id means the target left the zone it was in.
            let Some(o) = st.objects.get(&id) else {
                return false;
            };
            // CR 702.18a (shroud) and 702.11b (hexproof): abilities of permanents.
            if o.zone == Zone::Battlefield {
                let c = chars(st, id);
                if c.has(Keyword::Shroud)
                    || (c.has(Keyword::Hexproof) && c.controller != controller)
                {
                    return false;
                }
            }
            match filter {
                TargetFilter::AnyTarget | TargetFilter::Creature => {
                    o.zone == Zone::Battlefield && chars(st, id).is(CardType::Creature)
                }
                TargetFilter::Spell => st
                    .stack_object(id)
                    .is_some_and(|s| s.kind == StackKind::Spell),
                TargetFilter::SpellMatching(sel) => {
                    st.stack_object(id)
                        .is_some_and(|s| s.kind == StackKind::Spell)
                        && card_matches(st, id, sel)
                        && {
                            let c = chars(st, id);
                            sel.colors.is_empty() || sel.colors.iter().any(|x| c.colors.contains(x))
                        }
                }
                TargetFilter::Permanent(selectors) => {
                    // "Other" has no meaning for a target (it can't be the spell itself).
                    let none = ObjectId(u32::MAX);
                    o.zone == Zone::Battlefield && {
                        let c = chars(st, id);
                        selectors
                            .iter()
                            .any(|s| crate::layers::matches(s, &c, o, none, controller))
                    }
                }
                TargetFilter::CardInGraveyard { cards, whose } => {
                    o.zone == Zone::Graveyard
                        && match whose {
                            card_model::Who::Any => true,
                            card_model::Who::You => o.owner == controller,
                            card_model::Who::Opponent => o.owner != controller,
                        }
                        && card_matches(st, id, cards)
                }
                TargetFilter::Player | TargetFilter::Opponent => false,
            }
        }
    }
}

/// Is the card `id` (in a graveyard, hand, library…) one of the cards `sel` names? Only
/// card types and subtypes are compared — what a card has wherever it is (CR 109.2).
pub fn card_matches(st: &GameState, id: ObjectId, sel: &card_model::Selector) -> bool {
    let c = chars(st, id);
    (sel.types.is_empty() || sel.types.iter().any(|t| c.is(*t)))
        && !sel.not_types.iter().any(|t| c.is(*t))
        && (sel.subtypes.is_empty() || sel.subtypes.iter().any(|s| c.has_subtype(s)))
        && !sel.not_subtypes.iter().any(|s| c.has_subtype(s))
}

/// Every legal choice for `filter`, in stable order: objects by stack/battlefield order
/// (timestamp order), then players by seat (CLAUDE.md invariant 5).
pub fn candidates(
    st: &GameState,
    filter: &TargetFilter,
    controller: PlayerId,
    this: Option<ObjectId>,
) -> Vec<Target> {
    let mut out: Vec<Target> = Vec::new();
    match filter {
        TargetFilter::Spell | TargetFilter::SpellMatching(_) => {
            out.extend(st.stack.iter().map(|s| Target::Object(s.id)))
        }
        TargetFilter::AnyTarget | TargetFilter::Creature | TargetFilter::Permanent(_) => {
            out.extend(st.zones.battlefield.iter().map(|&id| Target::Object(id)));
        }
        TargetFilter::CardInGraveyard { .. } => {
            // Graveyards in seat order, each from the bottom up.
            for p in 0..st.num_players() {
                let cards = st.zones.list(Zone::Graveyard, PlayerId(p as u8));
                out.extend(cards.iter().map(|&id| Target::Object(id)));
            }
        }
        TargetFilter::Player | TargetFilter::Opponent => {}
    }
    if filter.allows_players() {
        out.extend((0..st.num_players()).map(|i| Target::Player(PlayerId(i as u8))));
    }
    out.retain(|t| is_legal(st, filter, *t, controller, this));
    out
}
