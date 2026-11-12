//! Targets (CR 115): who/what may be chosen, and whether a chosen target is still legal on
//! resolution (CR 608.2b). One function decides both, so choosing and checking can't drift.

use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::stack::{StackKind, Target};
use crate::state::GameState;
use crate::zones::Zone;
use alloc::vec::Vec;
use card_model::{CardType, Keyword, TargetFilter};

/// Whether `t` is a legal target for `filter`, for a spell/ability controlled by
/// `controller` whose own stack object is `this` (CR 115.5: it can't target itself): it is
/// what the filter describes, and nothing stops it from being targeted. `source` is the
/// object an ability is on: what "another target …" is other than.
pub fn is_legal(
    st: &GameState,
    filter: &TargetFilter,
    t: Target,
    controller: PlayerId,
    this: Option<ObjectId>,
    source: Option<ObjectId>,
) -> bool {
    // CR 702.18a (shroud) and 702.11c (hexproof) on players.
    if let Target::Player(p) = t
        && (crate::players::has_shroud(st, p)
            || (p != controller && crate::players::has_hexproof(st, p)))
    {
        return false;
    }
    // CR 702.18a (shroud) and 702.11b (hexproof): abilities of permanents.
    if let Target::Object(id) = t
        && let Some(o) = st.objects.get(&id)
        && o.zone == Zone::Battlefield
    {
        let c = chars(st, id);
        if c.has(Keyword::Shroud) || (c.has(Keyword::Hexproof) && c.controller != controller) {
            return false;
        }
    }
    describes(st, filter, t, controller, this, source)
}

/// Whether `t` is what `filter` describes — without asking whether it can be targeted.
/// This is the test for what an Aura may stay attached to (CR 303.4c): hexproof and shroud
/// stop targeting, they don't make an Aura fall off.
pub fn describes(
    st: &GameState,
    filter: &TargetFilter,
    t: Target,
    controller: PlayerId,
    this: Option<ObjectId>,
    source: Option<ObjectId>,
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
                        && card_matches(st, id, &bind_target(st, sel, source))
                        && {
                            let c = chars(st, id);
                            sel.colors.is_empty() || sel.colors.iter().any(|x| c.colors.contains(x))
                        }
                }
                TargetFilter::Permanent(selectors) => {
                    // "Another target permanent": not the ability's source. (A spell
                    // can't target itself anyway, CR 115.5.)
                    let source = source.unwrap_or(ObjectId(u32::MAX));
                    o.zone == Zone::Battlefield && {
                        let c = chars(st, id);
                        selectors
                            .iter()
                            .any(|s| crate::layers::matches(st, s, &c, o, source, controller))
                    }
                }
                TargetFilter::CardInGraveyard { cards, whose } => {
                    o.zone == Zone::Graveyard
                        && match whose {
                            card_model::Who::Any => true,
                            card_model::Who::You => o.owner == controller,
                            card_model::Who::Opponent => o.owner != controller,
                        }
                        && card_matches(st, id, &bind_target(st, cards, source))
                        // "Another target card": not the card the ability is on.
                        && !(cards.other && Some(id) == source)
                }
                TargetFilter::Player | TargetFilter::Opponent => false,
            }
        }
    }
}

/// "Target creature card of the chosen type": the choice of the ability's source (CR
/// 607.2d).
fn bind_target<'a>(
    st: &GameState,
    sel: &'a card_model::Selector,
    source: Option<ObjectId>,
) -> alloc::borrow::Cow<'a, card_model::Selector> {
    match source {
        Some(source) => crate::chosen::bind_for(st, sel, source),
        None => alloc::borrow::Cow::Borrowed(sel),
    }
}

/// Is the card `id` (in a graveyard, hand, library…) one of the cards `sel` names? What
/// a card has wherever it is (CR 109.2) is compared: its types, colors, mana value, name,
/// power and toughness… Whose card it is isn't asked here (`controller` and `other` are
/// the caller's business).
pub fn card_matches(st: &GameState, id: ObjectId, sel: &card_model::Selector) -> bool {
    card_matches_as(st, id, st.object(id).face, sel)
}

/// [`card_matches`] for the card with face `face` up: what a rule that lets a player play
/// or cast "a land card" / "creature spells" from a zone looks at for the face they would
/// play or cast (CR 601.3e, 712.8c, 712.9).
pub fn card_matches_as(st: &GameState, id: ObjectId, face: u8, sel: &card_model::Selector) -> bool {
    let c = crate::layers::chars_as(st, id, face);
    let o = st.object(id);
    let what = card_model::Selector {
        controller: card_model::Who::Any,
        owner: card_model::Who::Any,
        other: false,
        ..sel.clone()
    };
    crate::layers::matches_parts(
        &what,
        &c,
        crate::layers::Facts {
            id,
            controller: o.owner,
            owner: o.owner,
            is_token: o.is_token,
            tapped: false,
            attacking: false,
            blocking: false,
        },
        id,
        o.owner,
    )
}

/// Every legal choice for `filter`, in stable order: objects by stack/battlefield order
/// (timestamp order), then players by seat (CLAUDE.md invariant 5).
pub fn candidates(
    st: &GameState,
    filter: &TargetFilter,
    controller: PlayerId,
    this: Option<ObjectId>,
    source: Option<ObjectId>,
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
    out.retain(|t| is_legal(st, filter, *t, controller, this, source));
    out
}
