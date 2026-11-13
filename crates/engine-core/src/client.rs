//! What one seat may know: its version of the event stream (D-090). Rules text:
//! docs/rules-notes/400-hidden-information.md (CR 400.2, 400.7, 401, 402, 701.20, 708).
//!
//! The engine's events ([`crate::StepOutput::truth`]) are engine truth: they name every
//! object, hidden or not. A client (a UI, an agent, a network peer) must see them only
//! through an [`EventView`] for its seat, which
//! - keeps everything about public zones (battlefield, stack, graveyards, exile, command);
//! - keeps the seat's own hand, and what was revealed to everyone or looked at by the seat;
//! - turns every other object of a hidden zone (a library, another player's hand) into
//!   [`HIDDEN`]: the event still says that a card moved, from where to where and whose, but
//!   not which — and since an object that changes zones is a new object (CR 400.7), the ids
//!   it does carry never link a card in a hidden zone to what it was before or becomes after
//!   (a card drawn on turn 3 can't be matched to the card played on turn 5);
//! - adds [`Event::Identified`] for each object the seat newly gets to see (a card entering
//!   a public zone, drawn into its hand, revealed, looked at): the card it is. Nothing else
//!   in the redacted stream names a card in a hidden zone.
//!
//! Identities that become public later don't leak earlier hidden ones: the hidden ids were
//! never given out. An [`EventView`] is plain data (serialisable), and redaction is
//! deterministic.

use crate::events::{DamageTarget, Event};
use crate::ids::{ObjectId, PlayerId};
use crate::stack::Target;
use crate::state::GameState;
use crate::zones::Zone;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use card_model::CardDefId;
use serde::{Deserialize, Serialize};

/// What a redacted event says instead of the id of an object the seat may not identify.
pub const HIDDEN: ObjectId = ObjectId(u32::MAX);

/// Whose view of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Seat {
    /// A player: public information, their own hand, what is revealed, what they look at.
    Player(PlayerId),
    /// Someone watching who isn't playing: public information and what is revealed.
    Spectator,
    /// Engine truth (replays, debugging, a referee): nothing hidden.
    Everyone,
}

/// What the view remembers of an object it has been told about (engine-side truth, never
/// handed out).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Known {
    zone: Zone,
    owner: PlayerId,
    card: CardDefId,
    face: u8,
    /// Revealed to everyone, or looked at by this seat, while in a hidden zone (CR 701.20).
    shown: bool,
}

/// One seat's view of the event stream: feed it every batch of engine events, in order,
/// and hand on only what [`EventView::redact`] returns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventView {
    seat: Seat,
    objects: BTreeMap<ObjectId, Known>,
}

impl EventView {
    /// A view for `seat`, starting from the game as it is now (call it before the first
    /// event the seat should see: right after `new_game`, or at a save point).
    pub fn new(st: &GameState, seat: Seat) -> Self {
        let objects = st
            .objects
            .values()
            .map(|o| {
                (
                    o.id,
                    Known {
                        zone: o.zone,
                        owner: o.owner,
                        card: o.card,
                        face: o.face,
                        shown: false,
                    },
                )
            })
            .collect();
        EventView { seat, objects }
    }

    pub fn seat(&self) -> Seat {
        self.seat
    }

    /// May the seat identify this object as it is now? Objects the view knows nothing of
    /// (abilities on the stack, emblems made since) are public.
    fn visible(&self, id: ObjectId) -> bool {
        match self.objects.get(&id) {
            None => true,
            Some(k) => self.sees(k),
        }
    }

    fn sees(&self, k: &Known) -> bool {
        match (self.seat, k.zone) {
            (Seat::Everyone, _) => true,
            (_, Zone::Library) => k.shown,
            (Seat::Player(p), Zone::Hand) => k.owner == p || k.shown,
            (_, Zone::Hand) => k.shown,
            _ => true,
        }
    }

    fn id(&self, id: ObjectId) -> ObjectId {
        if self.visible(id) { id } else { HIDDEN }
    }

    fn identified(&self, id: ObjectId, out: &mut Vec<Event>) {
        if let Some(k) = self.objects.get(&id)
            && self.sees(k)
        {
            out.push(Event::Identified {
                object: id,
                card: k.card,
                face: k.face,
                owner: k.owner,
            });
        }
    }

    /// This seat's version of a batch of engine events (in order; the view keeps track of
    /// where each object is). Pure and deterministic.
    pub fn redact(&mut self, events: &[Event]) -> Vec<Event> {
        let mut out = Vec::new();
        for e in events {
            self.one(e, &mut out);
        }
        out
    }

    fn one(&mut self, e: &Event, out: &mut Vec<Event>) {
        match e {
            Event::ZoneChange {
                old,
                new,
                card_instance,
                from,
                to,
                position,
                ..
            } => {
                let before = self.objects.get(old).copied();
                let seen_before = self.visible(*old);
                let k = Known {
                    zone: *to,
                    owner: before.map_or(PlayerId(0), |k| k.owner),
                    card: before.map_or(CardDefId(0), |k| k.card),
                    face: before.map_or(0, |k| k.face),
                    shown: false,
                };
                self.objects.remove(old);
                if before.is_some() {
                    self.objects.insert(*new, k);
                }
                let seen_after = self.visible(*new);
                if seen_before && seen_after {
                    out.push(e.clone());
                } else {
                    // That a card of theirs moved, not which (CR 400.2); the card instance,
                    // which would link the two objects, isn't given (CR 400.7).
                    let _ = card_instance;
                    out.push(Event::MovedUnseen {
                        owner: k.owner,
                        old: if seen_before { *old } else { HIDDEN },
                        new: if seen_after { *new } else { HIDDEN },
                        from: *from,
                        to: *to,
                        position: *position,
                    });
                }
                if seen_after {
                    self.identified(*new, out);
                }
            }
            // CR 701.20d: "If cards in a player's library are shuffled or otherwise
            // reordered, any revealed cards that are reordered stop being revealed" — put on
            // its top or bottom (in a random order or not) as much as shuffled.
            Event::Shuffled { player } | Event::LibraryArranged { player, .. } => {
                for k in self.objects.values_mut() {
                    if k.zone == Zone::Library && k.owner == *player {
                        k.shown = false;
                    }
                }
                out.push(e.clone());
            }
            Event::Revealed { player, cards } => {
                // CR 701.20a: shown to all players.
                for c in cards {
                    if let Some(k) = self.objects.get_mut(c) {
                        k.shown = true;
                    }
                }
                // Each card is named before the reveal that shows it, so a client can say
                // what was revealed.
                for c in cards {
                    self.identified(*c, out);
                }
                out.push(e.clone());
                let _ = player;
            }
            Event::LookedAt { player, cards } => {
                // CR 701.20e: shown only to the player who looks.
                let mine = matches!(self.seat, Seat::Player(p) if p == *player)
                    || self.seat == Seat::Everyone;
                if mine {
                    for c in cards {
                        if let Some(k) = self.objects.get_mut(c) {
                            k.shown = true;
                        }
                    }
                }
                if mine {
                    for c in cards {
                        self.identified(*c, out);
                    }
                }
                out.push(Event::LookedAt {
                    player: *player,
                    cards: cards.iter().map(|c| self.id(*c)).collect(),
                });
            }
            // CR 701.23a: the searcher looked at the library and knows what they found;
            // everyone else learns how many cards (each `HIDDEN`), unless they are revealed.
            Event::SearchFound { player, cards } => {
                let mine = matches!(self.seat, Seat::Player(p) if p == *player)
                    || self.seat == Seat::Everyone;
                if mine {
                    for c in cards {
                        if let Some(k) = self.objects.get_mut(c) {
                            k.shown = true;
                        }
                        self.identified(*c, out);
                    }
                }
                out.push(Event::SearchFound {
                    player: *player,
                    cards: cards.iter().map(|c| self.id(*c)).collect(),
                });
            }
            Event::TokenCreated { object, card, .. } => {
                self.objects.insert(
                    *object,
                    Known {
                        zone: Zone::Battlefield,
                        owner: match e {
                            Event::TokenCreated { controller, .. } => *controller,
                            _ => PlayerId(0),
                        },
                        card: *card,
                        face: 0,
                        shown: false,
                    },
                );
                out.push(e.clone());
            }
            Event::Transformed { object, face } => {
                if let Some(k) = self.objects.get_mut(object) {
                    k.face = *face;
                }
                out.push(e.clone());
            }
            Event::CeasedToExist { object } | Event::ObjectLeftGame { object } => {
                out.push(map_ids(e, &mut |id| self.id(id)));
                self.objects.remove(object);
            }
            other => out.push(map_ids(other, &mut |id| self.id(id))),
        }
    }
}

/// Every object id `e` names, in field order (for a client that indexes events by object).
pub fn object_ids(e: &Event) -> Vec<ObjectId> {
    let mut out = Vec::new();
    map_ids(e, &mut |id| {
        out.push(id);
        id
    });
    out
}

/// `e` with each object id it names passed through `f`.
fn map_ids(e: &Event, f: &mut dyn FnMut(ObjectId) -> ObjectId) -> Event {
    let mut e = e.clone();
    let mut one = |id: &mut ObjectId| *id = f(*id);
    use Event as E;
    match &mut e {
        E::Drew { card, .. } | E::Discarded { card, .. } | E::Cycled { card, .. } => one(card),
        E::AbilityActivated {
            ability, source, ..
        } => {
            one(ability);
            one(source);
        }
        E::ModesChosen { object, .. }
        | E::XChosen { object, .. }
        | E::Fizzled { object }
        | E::Countered { object }
        | E::Sacrificed { object, .. }
        | E::ObjectLeftGame { object }
        | E::TokenCreated { object, .. }
        | E::CountersPut { object, .. }
        | E::CountersRemoved { object, .. }
        | E::CeasedToExist { object }
        | E::ControlChanged { object, .. }
        | E::Tapped { object, .. }
        | E::Untapped { object, .. }
        | E::DamageRemoved { object }
        | E::Unattached { object }
        | E::RemovedFromCombat { object }
        | E::Transformed { object, .. }
        | E::Chose { object, .. }
        | E::Regenerated { object }
        | E::BecameCopy { object, .. }
        | E::TargetsChanged { object }
        | E::EmblemCreated { object, .. }
        | E::Identified { object, .. }
        | E::ResolutionBegan { object }
        | E::MayChosen { object, .. }
        | E::UnlessPaid { object, .. } => one(object),
        E::TargetsChosen { object, targets } => {
            one(object);
            for t in targets {
                if let Target::Object(o) = t {
                    one(o);
                }
            }
        }
        E::AbilityRemoved { ability, .. } => one(ability),
        E::ContinuousEffectBegan {
            source,
            affected,
            about,
            ..
        } => {
            one(source);
            affected.iter_mut().for_each(&mut one);
            if let Some(a) = about {
                one(a);
            }
        }
        E::DamagePrevented { target, .. } => {
            if let DamageTarget::Object(o) = target {
                one(o);
            }
        }
        E::Replaced { source } => one(source),
        E::Attached { aura, to } => {
            one(aura);
            one(to);
        }
        E::Triggered { source, .. } => one(source),
        E::PaidToSave { at_stake, .. } => one(at_stake),
        E::LandPlayed { land, .. } => one(land),
        E::SpellCast { spell, .. } => one(spell),
        E::Resolved { spell } => one(spell),
        E::ManaAdded { source, .. } => {
            if let Some(s) = source {
                one(s);
            }
        }
        E::AttackerDeclared { attacker, .. } => one(attacker),
        E::BlockerDeclared { blocker, attacker } => {
            one(blocker);
            one(attacker);
        }
        E::DamageDealt { source, target, .. } => {
            one(source);
            if let DamageTarget::Object(o) = target {
                one(o);
            }
        }
        E::Revealed { cards, .. } | E::LookedAt { cards, .. } | E::SearchFound { cards, .. } => {
            cards.iter_mut().for_each(&mut one)
        }
        E::CopiedOnStack { copy, original, .. } => {
            one(copy);
            one(original);
        }
        E::AttacksPlaneswalker {
            attacker,
            planeswalker,
        } => {
            one(attacker);
            one(planeswalker);
        }
        E::ZoneChange { old, new, .. } | E::MovedUnseen { old, new, .. } => {
            one(old);
            one(new);
        }
        // No object in them.
        E::GameStarted { .. }
        | E::StartingPlayerChosen { .. }
        | E::Shuffled { .. }
        | E::MulliganDeclared { .. }
        | E::TurnBegan { .. }
        | E::StepBegan { .. }
        | E::DrewFromEmptyLibrary { .. }
        | E::PriorityPassed { .. }
        | E::PlayerLeftGame { .. }
        | E::BlockDeclarationIllegal { .. }
        | E::ManaPaid { .. }
        | E::ManaEmptied { .. }
        | E::LifeChanged { .. }
        | E::PlayerLost { .. }
        | E::GameOver { .. }
        | E::SearchedLibrary { .. }
        | E::LookedAtLibrary { .. }
        | E::Scried { .. }
        | E::Surveilled { .. }
        | E::LibraryArranged { .. }
        | E::TokensCapped { .. } => {}
    }
    e
}
