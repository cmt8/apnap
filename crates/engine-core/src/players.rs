//! Rules about players (CR 613.10, 613.11): maximum hand size (402.2), how many lands a
//! player may play (305.2), hexproof and shroud on players (702.11c, 702.18a), the top card
//! of a library being revealed or looked at (401.5), and the cards a player may play or
//! cast from zones other than their hand (601.3, 116.2a). Text:
//! docs/rules-notes/402-305-401.5-rules-about-players.md.
//!
//! None of this is stored: like the layered characteristics, it is read off the static
//! abilities permanents currently have (CLAUDE.md invariant 4) and the lasting effects in
//! force ("you may play an additional land this turn", CR 611.2c) whenever it is asked.

use crate::effects::lasting_abilities;
use crate::ids::{ObjectId, PlayerId, Timestamp};
use crate::layers::{abilities, controller_of};
use crate::state::GameState;
use crate::zones::Zone;
use alloc::vec::Vec;
use card_model::{AbilityDef, CardType, PlayZone, PlayerRule, Who};

/// One rule about players in force: whose ability it is and what it says.
#[derive(Clone, Copy, Debug)]
pub struct InForce<'a> {
    /// The permanent with the ability, or the source of the lasting effect.
    pub source: ObjectId,
    /// "You".
    pub controller: PlayerId,
    pub timestamp: Timestamp,
    pub who: Who,
    pub rule: &'a PlayerRule,
}

impl InForce<'_> {
    /// Is player `p` one of the players the rule is about?
    pub fn about(&self, p: PlayerId) -> bool {
        match self.who {
            Who::Any => true,
            Who::You => p == self.controller,
            Who::Opponent => p != self.controller,
        }
    }
}

/// Every rule about players in force, in timestamp order (CR 613.11): the static abilities
/// of permanents whose condition holds now, and those lasting effects carry.
pub fn in_force(st: &GameState) -> Vec<InForce<'_>> {
    let mut out = Vec::new();
    // Permanents, then emblems (CR 114.4: their abilities work from the command zone).
    let sources: Vec<crate::ids::ObjectId> = st
        .zones
        .battlefield
        .iter()
        .copied()
        .chain(crate::effects::emblems(st))
        .collect();
    for id in sources {
        let controller = controller_of(st, id);
        for (_, a) in abilities(st, id) {
            // ("Cast this card from your graveyard" works off the battlefield only.)
            if let AbilityDef::PlayerRule { who, rule, only_if } = a
                && !matches!(rule, PlayerRule::PlayThisFrom(_))
                && crate::triggers::condition_holds(st, only_if.as_ref(), controller, id)
            {
                out.push(InForce {
                    source: id,
                    controller,
                    timestamp: st.object(id).timestamp,
                    who: *who,
                    rule,
                });
            }
        }
    }
    for (e, _, a) in lasting_abilities(st) {
        if let AbilityDef::PlayerRule { who, rule, only_if } = a
            && crate::triggers::condition_holds(st, only_if.as_ref(), e.controller, e.source)
        {
            out.push(InForce {
                source: e.source,
                controller: e.controller,
                timestamp: e.timestamp,
                who: *who,
                rule,
            });
        }
    }
    // Stable: battlefield order (itself timestamp order), then lasting effects.
    out.sort_by_key(|r| r.timestamp);
    out
}

/// The rules in force about player `p`.
fn about(st: &GameState, p: PlayerId) -> impl Iterator<Item = InForce<'_>> {
    in_force(st).into_iter().filter(move |r| r.about(p))
}

/// CR 402.2: `p`'s maximum hand size; `None` if they have none. Effects that change it
/// apply in timestamp order (CR 613.11).
pub fn max_hand_size(st: &GameState, p: PlayerId) -> Option<u32> {
    let mut size = Some(st.config.max_hand_size as i64);
    for r in about(st, p) {
        match r.rule {
            PlayerRule::NoMaximumHandSize => size = None,
            PlayerRule::MaximumHandSize(n) => size = Some(*n as i64),
            PlayerRule::MaximumHandSizeBy(n) => size = size.map(|s| s + *n as i64),
            _ => {}
        }
    }
    size.map(|s| s.max(0) as u32)
}

/// CR 305.2: how many lands `p` can play this turn; `None` = any number.
pub fn land_plays(st: &GameState, p: PlayerId) -> Option<u32> {
    let mut n = 1u32;
    for r in about(st, p) {
        match r.rule {
            PlayerRule::AdditionalLandPlays(k) => n = n.saturating_add(*k),
            PlayerRule::AnyNumberOfLandPlays => return None,
            _ => {}
        }
    }
    Some(n)
}

/// CR 305.2a–b: may `p` play a land now, as far as the number of lands goes?
pub fn land_play_left(st: &GameState, p: PlayerId) -> bool {
    land_plays(st, p).is_none_or(|n| st.player(p).lands_played_this_turn < n)
}

/// CR 702.11c: `p` has hexproof.
pub fn has_hexproof(st: &GameState, p: PlayerId) -> bool {
    about(st, p).any(|r| matches!(r.rule, PlayerRule::Hexproof))
}

/// CR 702.18a: `p` has shroud.
pub fn has_shroud(st: &GameState, p: PlayerId) -> bool {
    about(st, p).any(|r| matches!(r.rule, PlayerRule::Shroud))
}

/// The top card of `owner`'s library, if any.
pub fn library_top(st: &GameState, owner: PlayerId) -> Option<ObjectId> {
    st.zones.libraries[owner.index()].last().copied()
}

/// CR 401.5: may `viewer` see the top card of `owner`'s library now? Everyone, if `owner`
/// plays with it revealed; `owner`, if they may look at it any time.
///
/// While a spell is being cast or an ability activated, no top card is shown (CR 401.5: a
/// new top card is not revealed until the spell becomes cast). The engine doesn't remember
/// which card was on top as the cast began, so it shows none until the cast is complete.
pub fn sees_library_top(st: &GameState, viewer: PlayerId, owner: PlayerId) -> bool {
    if casting(st) {
        return false;
    }
    about(st, owner).any(|r| match r.rule {
        PlayerRule::TopCardRevealed => true,
        PlayerRule::LookAtTopCard => viewer == owner,
        _ => false,
    })
}

/// A spell is being cast or an ability activated (CR 601.2, 602.2).
fn casting(st: &GameState) -> bool {
    st.frames.iter().any(|f| {
        matches!(
            f,
            crate::frames::Frame::Cast(c)
                if matches!(
                    c.source,
                    crate::frames::CastSource::Card(_)
                        | crate::frames::CastSource::Face { .. }
                        | crate::frames::CastSource::Ability { .. }
                )
        )
    })
}

/// The zone `card` is in, as a zone a rule may let its owner play it from.
fn play_zone(st: &GameState, card: ObjectId) -> Option<PlayZone> {
    let o = st.object(card);
    match o.zone {
        Zone::Graveyard => Some(PlayZone::Graveyard),
        Zone::Exile => Some(PlayZone::Exile),
        Zone::Library if library_top(st, o.owner) == Some(card) => Some(PlayZone::LibraryTop),
        _ => None,
    }
}

/// CR 601.3 / 116.2a: does a rule let `p` play (a land) or cast (anything else) `card` from
/// the zone it is in — their graveyard, exile, the top of their library? (From the hand,
/// and a commander from the command zone, the game's own rules allow it.) Only cards `p`
/// owns: every rule so far is about the player's own zones.
pub fn may_play_from(st: &GameState, p: PlayerId, card: ObjectId) -> bool {
    may_play_face_from(st, p, card, 0)
}

/// [`may_play_from`] for one face of the card (0 = its front): a rule about "lands" or
/// "creature spells" is judged by the face that would be played or cast (CR 601.3e,
/// 712.8c, 712.9: "you may play lands from your graveyard" lets a modal double-faced card
/// be played as its land face, not cast as its spell face).
pub fn may_play_face_from(st: &GameState, p: PlayerId, card: ObjectId, face: u8) -> bool {
    let rules: Vec<InForce<'_>> = about(st, p).collect();
    may_play_with(st, p, card, face, &rules)
}

/// May `p` play or cast `card` with face `face` up from where it is now: from their hand,
/// a card they sent on an Adventure (CR 716.3d: as itself, never as the Adventure again),
/// or where a rule lets them ([`may_play_face_from`]). (A commander in the command zone is
/// the casting rules' own business, CR 903.8.)
pub fn may_play_face(st: &GameState, p: PlayerId, card: ObjectId, face: u8) -> bool {
    let o = st.object(card);
    (o.zone == Zone::Hand && o.owner == p)
        || (o.zone == Zone::Exile && o.adventure == Some(p) && face == 0)
        || may_play_face_from(st, p, card, face)
}

fn may_play_with(
    st: &GameState,
    p: PlayerId,
    card: ObjectId,
    face: u8,
    rules: &[InForce<'_>],
) -> bool {
    let Some(o) = st.objects.get(&card) else {
        return false;
    };
    if o.owner != p {
        return false;
    }
    let Some(zone) = play_zone(st, card) else {
        return false;
    };
    // What a rule would match: a card in that zone. (Cheap tests first.)
    let ruled = |st: &GameState| {
        rules.iter().any(|r| match r.rule {
            PlayerRule::PlayFrom { zone: z, cards } => {
                *z == zone
                    && cards.iter().any(|s| {
                        // "Creature spells of the chosen type" (CR 607.2d).
                        let s = crate::chosen::bind_for(st, s, r.source);
                        crate::targeting::card_matches_as(st, card, face, &s)
                    })
            }
            _ => false,
        })
    };
    // "You may cast this card from your graveyard": the card's own ability, which works
    // from that zone (CR 113.6f). (Off the battlefield a card has only the abilities
    // printed on it: a quick look there first.)
    let printed = st.card_def(card).abilities.iter().any(|a| {
        matches!(
            a,
            AbilityDef::PlayerRule {
                rule: PlayerRule::PlayThisFrom(_),
                ..
            }
        )
    });
    let own = printed && abilities(st, card).into_iter().any(|(_, a)| {
        matches!(a, AbilityDef::PlayerRule { rule: PlayerRule::PlayThisFrom(zones), only_if, .. }
            if zones.contains(&zone)
                && crate::triggers::condition_holds(st, only_if.as_ref(), p, card))
    });
    own || ruled(st)
}

/// The cards outside `p`'s hand that a rule lets them play or cast now (whatever the
/// timing), in a stable order: graveyard (bottom up), exile, the top of their library.
pub fn playable_elsewhere(st: &GameState, p: PlayerId) -> Vec<ObjectId> {
    let rules: Vec<InForce<'_>> = about(st, p)
        .filter(|r| matches!(r.rule, PlayerRule::PlayFrom { .. }))
        .collect();
    let mut cards: Vec<ObjectId> = st.zones.graveyards[p.index()].clone();
    cards.extend(st.zones.exile.iter().copied());
    cards.extend(library_top(st, p));
    // Any face it may be played or cast with (CR 712.8b, 712.9).
    cards.retain(|&c| {
        core::iter::once(0)
            .chain(crate::frames::cast::castable_faces(st, c))
            .any(|f| may_play_with(st, p, c, f, &rules))
    });
    cards
}

/// Is `card` a land card (it is played, not cast)?
pub fn is_land(st: &GameState, card: ObjectId) -> bool {
    crate::layers::chars(st, card).is(CardType::Land)
}
