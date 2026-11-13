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
                    });
                }
                if seen_after {
                    self.identified(*new, out);
                }
            }
            Event::Shuffled { player } => {
                // CR 701.20d: revealed cards that are shuffled stop being revealed.
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
                out.push(e.clone());
                for c in cards {
                    self.identified(*c, out);
                }
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
                out.push(Event::LookedAt {
                    player: *player,
                    cards: cards.iter().map(|c| self.id(*c)).collect(),
                });
                if mine {
                    for c in cards {
                        self.identified(*c, out);
                    }
                }
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

/// `e` with each object id it names passed through `f`.
fn map_ids(e: &Event, f: &mut dyn FnMut(ObjectId) -> ObjectId) -> Event {
    let t = |t: &Target, f: &mut dyn FnMut(ObjectId) -> ObjectId| match t {
        Target::Object(o) => Target::Object(f(*o)),
        p => *p,
    };
    let dt = |d: &DamageTarget, f: &mut dyn FnMut(ObjectId) -> ObjectId| match d {
        DamageTarget::Object(o) => DamageTarget::Object(f(*o)),
        p => *p,
    };
    use Event as E;
    match e {
        E::Drew { player, card } => E::Drew {
            player: *player,
            card: f(*card),
        },
        E::AbilityActivated {
            player,
            ability,
            source,
        } => E::AbilityActivated {
            player: *player,
            ability: f(*ability),
            source: f(*source),
        },
        E::ModesChosen { object, modes } => E::ModesChosen {
            object: f(*object),
            modes: modes.clone(),
        },
        E::XChosen { object, x } => E::XChosen {
            object: f(*object),
            x: *x,
        },
        E::TargetsChosen { object, targets } => {
            let object = f(*object);
            E::TargetsChosen {
                object,
                targets: targets.iter().map(|x| t(x, f)).collect(),
            }
        }
        E::Fizzled { object } => E::Fizzled { object: f(*object) },
        E::Countered { object } => E::Countered { object: f(*object) },
        E::AbilityRemoved { ability } => E::AbilityRemoved {
            ability: f(*ability),
        },
        E::Sacrificed { player, object } => E::Sacrificed {
            player: *player,
            object: f(*object),
        },
        E::ObjectLeftGame { object } => E::ObjectLeftGame { object: f(*object) },
        E::TokenCreated {
            object,
            card,
            controller,
        } => E::TokenCreated {
            object: f(*object),
            card: *card,
            controller: *controller,
        },
        E::CountersPut { object, kind, n } => E::CountersPut {
            object: f(*object),
            kind: *kind,
            n: *n,
        },
        E::CountersRemoved { object, kind, n } => E::CountersRemoved {
            object: f(*object),
            kind: *kind,
            n: *n,
        },
        E::CeasedToExist { object } => E::CeasedToExist { object: f(*object) },
        E::ContinuousEffectBegan { id, source } => E::ContinuousEffectBegan {
            id: *id,
            source: f(*source),
        },
        E::DamagePrevented { target, amount } => E::DamagePrevented {
            target: dt(target, f),
            amount: *amount,
        },
        E::Replaced { source } => E::Replaced { source: f(*source) },
        E::Attached { aura, to } => E::Attached {
            aura: f(*aura),
            to: f(*to),
        },
        E::ControlChanged { object, controller } => E::ControlChanged {
            object: f(*object),
            controller: *controller,
        },
        E::Triggered {
            source,
            controller,
            ability,
        } => E::Triggered {
            source: f(*source),
            controller: *controller,
            ability: *ability,
        },
        E::LandPlayed { player, land } => E::LandPlayed {
            player: *player,
            land: f(*land),
        },
        E::SpellCast { player, spell } => E::SpellCast {
            player: *player,
            spell: f(*spell),
        },
        E::Resolved { spell } => E::Resolved { spell: f(*spell) },
        E::Tapped { object } => E::Tapped { object: f(*object) },
        E::Untapped { object } => E::Untapped { object: f(*object) },
        E::AttackerDeclared { attacker, defender } => E::AttackerDeclared {
            attacker: f(*attacker),
            defender: *defender,
        },
        E::BlockerDeclared { blocker, attacker } => E::BlockerDeclared {
            blocker: f(*blocker),
            attacker: f(*attacker),
        },
        E::DamageDealt {
            source,
            target,
            amount,
            combat,
        } => E::DamageDealt {
            source: f(*source),
            target: dt(target, f),
            amount: *amount,
            combat: *combat,
        },
        E::DamageRemoved { object } => E::DamageRemoved { object: f(*object) },
        E::Discarded { player, card } => E::Discarded {
            player: *player,
            card: f(*card),
        },
        E::Unattached { object } => E::Unattached { object: f(*object) },
        E::Revealed { player, cards } => E::Revealed {
            player: *player,
            cards: cards.iter().map(|c| f(*c)).collect(),
        },
        E::LookedAt { player, cards } => E::LookedAt {
            player: *player,
            cards: cards.iter().map(|c| f(*c)).collect(),
        },
        E::Cycled { player, card } => E::Cycled {
            player: *player,
            card: f(*card),
        },
        E::RemovedFromCombat { object } => E::RemovedFromCombat { object: f(*object) },
        E::Transformed { object, face } => E::Transformed {
            object: f(*object),
            face: *face,
        },
        E::Chose {
            player,
            object,
            chosen,
        } => E::Chose {
            player: *player,
            object: f(*object),
            chosen: *chosen,
        },
        E::Regenerated { object } => E::Regenerated { object: f(*object) },
        E::BecameCopy { object, card } => E::BecameCopy {
            object: f(*object),
            card: *card,
        },
        E::TargetsChanged { object } => E::TargetsChanged { object: f(*object) },
        E::CopiedOnStack {
            copy,
            original,
            controller,
        } => E::CopiedOnStack {
            copy: f(*copy),
            original: f(*original),
            controller: *controller,
        },
        E::AttacksPlaneswalker {
            attacker,
            planeswalker,
        } => E::AttacksPlaneswalker {
            attacker: f(*attacker),
            planeswalker: f(*planeswalker),
        },
        E::EmblemCreated {
            object,
            player,
            card,
            abilities,
        } => E::EmblemCreated {
            object: f(*object),
            player: *player,
            card: *card,
            abilities: abilities.clone(),
        },
        E::Identified {
            object,
            card,
            face,
            owner,
        } => E::Identified {
            object: f(*object),
            card: *card,
            face: *face,
            owner: *owner,
        },
        E::MovedUnseen {
            owner,
            old,
            new,
            from,
            to,
        } => E::MovedUnseen {
            owner: *owner,
            old: f(*old),
            new: f(*new),
            from: *from,
            to: *to,
        },
        E::ZoneChange {
            old,
            new,
            card_instance,
            from,
            to,
        } => E::ZoneChange {
            old: f(*old),
            new: f(*new),
            card_instance: *card_instance,
            from: *from,
            to: *to,
        },
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
        | E::ManaAdded { .. }
        | E::ManaPaid { .. }
        | E::ManaEmptied { .. }
        | E::LifeChanged { .. }
        | E::PlayerLost { .. }
        | E::GameOver { .. }
        | E::SearchedLibrary { .. }
        | E::LookedAtLibrary { .. }
        | E::Scried { .. }
        | E::Surveilled { .. }
        | E::LibraryArranged { .. } => e.clone(),
    }
}
