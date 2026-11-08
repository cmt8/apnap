//! Targets (CR 115): who/what may be chosen, and whether a chosen target is still legal on
//! resolution (CR 608.2b). One function decides both, so choosing and checking can't drift.

use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::stack::{StackKind, Target};
use crate::state::GameState;
use crate::zones::Zone;
use alloc::vec::Vec;
use card_model::{CardType, TargetFilter};

/// Whether `t` satisfies `filter` for a spell/ability controlled by `controller` whose own
/// stack object is `this` (CR 115.5: it can't target itself).
pub fn is_legal(
    st: &GameState,
    filter: TargetFilter,
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
                    TargetFilter::Creature | TargetFilter::Spell => false,
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
            match filter {
                TargetFilter::AnyTarget | TargetFilter::Creature => {
                    o.zone == Zone::Battlefield && chars(st, id).is(CardType::Creature)
                }
                TargetFilter::Spell => st
                    .stack_object(id)
                    .is_some_and(|s| s.kind == StackKind::Spell),
                TargetFilter::Player | TargetFilter::Opponent => false,
            }
        }
    }
}

/// Every legal choice for `filter`, in stable order: objects by stack/battlefield order
/// (timestamp order), then players by seat (CLAUDE.md invariant 5).
pub fn candidates(
    st: &GameState,
    filter: TargetFilter,
    controller: PlayerId,
    this: Option<ObjectId>,
) -> Vec<Target> {
    let mut out: Vec<Target> = Vec::new();
    match filter {
        TargetFilter::Spell => out.extend(st.stack.iter().map(|s| Target::Object(s.id))),
        TargetFilter::AnyTarget | TargetFilter::Creature => {
            out.extend(st.zones.battlefield.iter().map(|&id| Target::Object(id)));
        }
        TargetFilter::Player | TargetFilter::Opponent => {}
    }
    if filter.allows_players() {
        out.extend((0..st.num_players()).map(|i| Target::Player(PlayerId(i as u8))));
    }
    out.retain(|t| is_legal(st, filter, *t, controller, this));
    out
}
