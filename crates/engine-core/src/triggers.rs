//! Triggered abilities (CR 603 (2026-11-06)). Text: docs/rules-notes/603-triggered-abilities.md.
//!
//! Triggers are detected from events that were actually **performed** — after the
//! replacement pipeline (CR 603.2g) — and queued in `GameState::pending_triggers`. They go
//! on the stack the next time a player would receive priority (CR 603.3), via
//! `frames::PutTriggersFrame`.
//!
//! - Enters-the-battlefield triggers are checked after a whole simultaneous batch, against
//!   every permanent including the newcomers (CR 603.6a).
//! - "Dies" triggers look back in time (CR 603.10a): the dying object and any other objects
//!   that left in the same batch are seen as they last existed (`Lki`).

use crate::events::Event;
use crate::ids::{ObjectId, PlayerId};
use crate::layers::{Characteristics, Facts, abilities, chars, matches_parts, matching};
use crate::state::GameState;
use crate::turn::Step;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use card_model::{
    AbilityDef, CardDefId, Condition, ObjFilter, Selector, Trigger, TriggerStep, Who,
};
use serde::{Deserialize, Serialize};

/// A triggered ability that has triggered but isn't on the stack yet (CR 603.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTrigger {
    /// CR 603.3a: the player who controlled the source when it triggered.
    pub controller: PlayerId,
    pub source: ObjectId,
    pub source_card: CardDefId,
    /// Index of the triggered ability on `source_card`.
    pub index: u16,
    /// The source as it last existed, if it had already left the battlefield (look-back
    /// triggers, CR 603.10a; LKI for "its power", CR 608.2h).
    pub source_lki: Option<Characteristics>,
    /// The object whose event triggered it (what entered, died or attacked), and — if it
    /// had already left the battlefield — that object as it last existed.
    #[serde(default)]
    pub subject: Option<ObjectId>,
    #[serde(default)]
    pub subject_lki: Option<Characteristics>,
}

/// Last known information about an object that left the battlefield (CR 113.7a, 608.2h),
/// captured before the batch that moved it (CR 704.8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lki {
    pub card: CardDefId,
    pub chars: Characteristics,
    pub controller: PlayerId,
    pub is_token: bool,
    pub tapped: bool,
}

pub(crate) fn snapshot(st: &GameState, id: ObjectId) -> Lki {
    let o = st.object(id);
    Lki {
        card: o.card,
        chars: chars(st, id),
        controller: crate::layers::controller_of(st, id),
        is_token: o.is_token,
        tapped: o.tapped,
    }
}

/// Does the object `id` (seen as `c` / `facts`) satisfy a trigger's object filter, for an
/// ability whose source is `source`, controlled by `controller`?
fn filter_hit(
    filter: &ObjFilter,
    id: ObjectId,
    c: &Characteristics,
    facts: Facts,
    source: ObjectId,
    controller: PlayerId,
) -> bool {
    let matches = |sel: &Selector| matches_parts(sel, c, facts, source, controller);
    match filter {
        ObjFilter::This => id == source,
        ObjFilter::Matching(sel) => matches(sel),
        // "This or another X": the source whatever it is, or a different object that is X.
        ObjFilter::ThisOr(sel) => id == source || matches(sel),
    }
}

/// CR 603.4: an intervening "if" clause, for the ability's controller.
pub(crate) fn condition_holds(
    st: &GameState,
    cond: Option<&Condition>,
    controller: PlayerId,
    source: ObjectId,
) -> bool {
    let life = st.player(controller).life;
    match cond {
        None => true,
        Some(Condition::LifeAtMost(n)) => life <= *n,
        Some(Condition::LifeAtLeast(n)) => life >= *n,
        Some(Condition::Control { what, at_least }) => {
            matching(st, what, source, controller).len() >= *at_least as usize
        }
        Some(Condition::CardsInGraveyard { cards, at_least }) => {
            st.zones
                .list(crate::zones::Zone::Graveyard, controller)
                .iter()
                .filter(|&&id| crate::targeting::card_matches(st, id, cards))
                .count()
                >= *at_least as usize
        }
    }
}

fn who_matches(who: Who, player: PlayerId, controller: PlayerId) -> bool {
    match who {
        Who::Any => true,
        Who::You => player == controller,
        Who::Opponent => player != controller,
    }
}

/// Queue a trigger (engine bookkeeping, recorded in the event log so it is hashed).
fn queue(st: &mut GameState, t: PendingTrigger) {
    st.emit(Event::Triggered {
        source: t.source,
        controller: t.controller,
        index: t.index,
    });
    st.pending_triggers.push(t);
}

/// A potential trigger source: a permanent now, or one that just left (looks back).
struct Source<'a> {
    id: ObjectId,
    card: CardDefId,
    controller: PlayerId,
    abilities: Vec<(u16, &'a AbilityDef)>,
    lki: Option<Characteristics>,
}

fn battlefield_sources(st: &GameState) -> Vec<Source<'_>> {
    st.zones
        .battlefield
        .iter()
        .map(|&id| Source {
            id,
            card: st.object(id).card,
            controller: crate::layers::controller_of(st, id),
            abilities: abilities(st, id),
            lki: None,
        })
        .collect()
}

fn queue_matching(st: &mut GameState, found: Vec<PendingTrigger>, conds: Vec<Option<Condition>>) {
    for (t, c) in found.into_iter().zip(conds) {
        // CR 603.4: the intervening "if" must be true when the trigger event occurs.
        if condition_holds(st, c.as_ref(), t.controller, t.source) {
            queue(st, t);
        }
    }
}

/// Detect triggers from a batch of performed events. `lki` holds every object that left
/// the battlefield in this batch, as it existed just before the batch.
pub(crate) fn detect_events(st: &mut GameState, events: &[Event], lki: &BTreeMap<ObjectId, Lki>) {
    let mut entered: Vec<ObjectId> = Vec::new();
    let mut died: Vec<ObjectId> = Vec::new();
    for e in events {
        match e {
            Event::ZoneChange {
                new,
                to: crate::zones::Zone::Battlefield,
                ..
            }
            | Event::TokenCreated { object: new, .. } => entered.push(*new),
            Event::ZoneChange {
                old,
                from: crate::zones::Zone::Battlefield,
                to: crate::zones::Zone::Graveyard,
                ..
            } if lki.contains_key(old) => died.push(*old),
            _ => {}
        }
    }
    let mut found = Vec::new();
    let mut conds = Vec::new();
    if !entered.is_empty() {
        // CR 603.6a: every permanent on the battlefield, including the newcomers.
        for src in battlefield_sources(st) {
            for &(index, a) in &src.abilities {
                let AbilityDef::Triggered {
                    when,
                    intervening_if,
                    ..
                } = a
                else {
                    continue;
                };
                for filter in when.events().into_iter().filter_map(|t| match t {
                    Trigger::Enters(f) => Some(f),
                    _ => None,
                }) {
                    for &e in &entered {
                        if !st.objects.contains_key(&e) {
                            continue;
                        }
                        let o = st.object(e);
                        let c = chars(st, e);
                        let facts = Facts {
                            id: e,
                            controller: c.controller,
                            is_token: o.is_token,
                            tapped: o.tapped,
                        };
                        if filter_hit(filter, e, &c, facts, src.id, src.controller) {
                            found.push(PendingTrigger {
                                controller: src.controller,
                                source: src.id,
                                source_card: src.card,
                                index,
                                source_lki: None,
                                subject: Some(e),
                                subject_lki: None,
                            });
                            conds.push(intervening_if.clone());
                        }
                    }
                }
            }
        }
    }
    if !died.is_empty() {
        // CR 603.10a: look back in time — current permanents plus everything that left in
        // this batch, as it last existed.
        let mut sources = battlefield_sources(st);
        for (&id, l) in lki {
            sources.push(Source {
                id,
                card: l.card,
                controller: l.controller,
                abilities: l
                    .chars
                    .abilities
                    .iter()
                    .map(|&i| (i, &st.catalog.get(l.card).abilities[i as usize]))
                    .collect(),
                lki: Some(l.chars.clone()),
            });
        }
        for src in &sources {
            for &(index, a) in &src.abilities {
                let AbilityDef::Triggered {
                    when,
                    intervening_if,
                    ..
                } = a
                else {
                    continue;
                };
                for filter in when.events().into_iter().filter_map(|t| match t {
                    Trigger::Dies(f) => Some(f),
                    _ => None,
                }) {
                    for d in &died {
                        let l = &lki[d];
                        let facts = Facts {
                            id: *d,
                            controller: l.controller,
                            is_token: l.is_token,
                            tapped: l.tapped,
                        };
                        if filter_hit(filter, *d, &l.chars, facts, src.id, src.controller) {
                            found.push(PendingTrigger {
                                controller: src.controller,
                                source: src.id,
                                source_card: src.card,
                                index,
                                source_lki: src.lki.clone(),
                                subject: Some(*d),
                                subject_lki: Some(l.chars.clone()),
                            });
                            conds.push(intervening_if.clone());
                        }
                    }
                }
            }
        }
    }
    queue_matching(st, found, conds);
}

/// CR 603.2b: "at the beginning of" triggers, when a step begins.
pub(crate) fn detect_step(st: &mut GameState, step: Step) {
    let active = st.turn.active;
    let mut found = Vec::new();
    let mut conds = Vec::new();
    for src in battlefield_sources(st) {
        for &(index, a) in &src.abilities {
            let AbilityDef::Triggered {
                when,
                intervening_if,
                ..
            } = a
            else {
                continue;
            };
            for t in when.events() {
                let Trigger::BeginningOf { step: ts, whose } = t else {
                    continue;
                };
                let step_matches = matches!(
                    (ts, step),
                    (TriggerStep::Upkeep, Step::Upkeep)
                        | (TriggerStep::DrawStep, Step::Draw)
                        | (TriggerStep::EndStep, Step::End)
                );
                if step_matches && who_matches(*whose, active, src.controller) {
                    found.push(PendingTrigger {
                        controller: src.controller,
                        source: src.id,
                        source_card: src.card,
                        index,
                        source_lki: None,
                        subject: None,
                        subject_lki: None,
                    });
                    conds.push(intervening_if.clone());
                }
            }
        }
    }
    queue_matching(st, found, conds);
}

/// CR 508.1m: "whenever [a creature] attacks" triggers, as attackers are declared.
pub(crate) fn detect_attacks(st: &mut GameState, attackers: &[ObjectId]) {
    let mut found = Vec::new();
    let mut conds = Vec::new();
    for src in battlefield_sources(st) {
        for &(index, a) in &src.abilities {
            let AbilityDef::Triggered {
                when,
                intervening_if,
                ..
            } = a
            else {
                continue;
            };
            for filter in when.events().into_iter().filter_map(|t| match t {
                Trigger::Attacks(f) => Some(f),
                _ => None,
            }) {
                for &attacker in attackers {
                    if !st.objects.contains_key(&attacker) {
                        continue;
                    }
                    let o = st.object(attacker);
                    let c = chars(st, attacker);
                    let facts = Facts {
                        id: attacker,
                        controller: c.controller,
                        is_token: o.is_token,
                        tapped: o.tapped,
                    };
                    if filter_hit(filter, attacker, &c, facts, src.id, src.controller) {
                        found.push(PendingTrigger {
                            controller: src.controller,
                            source: src.id,
                            source_card: src.card,
                            index,
                            source_lki: None,
                            subject: Some(attacker),
                            subject_lki: None,
                        });
                        conds.push(intervening_if.clone());
                    }
                }
            }
        }
    }
    queue_matching(st, found, conds);
}

/// "Whenever [a player] casts a spell" triggers (at CR 601.2i).
pub(crate) fn detect_cast(st: &mut GameState, caster: PlayerId, spell: ObjectId) {
    let spell_chars = chars(st, spell);
    let mut found = Vec::new();
    let mut conds = Vec::new();
    for src in battlefield_sources(st) {
        for &(index, a) in &src.abilities {
            let AbilityDef::Triggered {
                when,
                intervening_if,
                ..
            } = a
            else {
                continue;
            };
            for t in when.events() {
                if let Trigger::Cast { by, types } = t
                    && who_matches(*by, caster, src.controller)
                    && (types.is_empty() || types.iter().any(|t| spell_chars.is(*t)))
                {
                    found.push(PendingTrigger {
                        controller: src.controller,
                        source: src.id,
                        source_card: src.card,
                        index,
                        source_lki: None,
                        subject: None,
                        subject_lki: None,
                    });
                    conds.push(intervening_if.clone());
                }
            }
        }
    }
    queue_matching(st, found, conds);
}

#[cfg(test)]
mod tests {
    use super::*;
    use card_model::CardDef;

    /// "This creature or another Zombie": the source counts whatever it is; anything else
    /// has to match.
    #[test]
    fn this_or_another_matches_the_source_itself_and_other_matching_objects() {
        let you = PlayerId(0);
        let chars_of = |types: &[&str]| {
            Characteristics::printed(
                &CardDef {
                    name: "Test".into(),
                    creature_types: types
                        .iter()
                        .map(|t| alloc::string::String::from(*t))
                        .collect(),
                    ..CardDef::default()
                },
                you,
            )
        };
        let facts = |id: u32| Facts {
            id: ObjectId(id),
            controller: you,
            is_token: false,
            tapped: false,
        };
        let filter = ObjFilter::ThisOr(Selector {
            subtypes: alloc::vec!["Zombie".into()],
            ..Selector::default()
        });
        let source = ObjectId(1);
        let hit = |id: u32, types: &[&str]| {
            filter_hit(
                &filter,
                ObjectId(id),
                &chars_of(types),
                facts(id),
                source,
                you,
            )
        };
        assert!(hit(1, &["Elf"]), "the source, though it isn't a Zombie");
        assert!(hit(2, &["Zombie"]), "another Zombie");
        assert!(!hit(2, &["Elf"]), "another creature that isn't one");
        // The plain filters, for contrast.
        let only = ObjFilter::Matching(Selector {
            subtypes: alloc::vec!["Zombie".into()],
            ..Selector::default()
        });
        assert!(!filter_hit(
            &only,
            source,
            &chars_of(&["Elf"]),
            facts(1),
            source,
            you
        ));
        assert!(filter_hit(
            &ObjFilter::This,
            source,
            &chars_of(&["Elf"]),
            facts(1),
            source,
            you
        ));
        assert!(!filter_hit(
            &ObjFilter::This,
            ObjectId(2),
            &chars_of(&["Zombie"]),
            facts(2),
            source,
            you
        ));
    }
}
