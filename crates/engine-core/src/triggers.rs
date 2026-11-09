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
use crate::layers::{Characteristics, abilities, chars, matches_parts};
use crate::state::GameState;
use crate::turn::Step;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use card_model::{AbilityDef, CardDefId, Condition, ObjFilter, Trigger, TriggerStep, Who};
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
}

/// Last known information about an object that left the battlefield (CR 113.7a, 608.2h),
/// captured before the batch that moved it (CR 704.8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lki {
    pub card: CardDefId,
    pub chars: Characteristics,
    pub controller: PlayerId,
    pub is_token: bool,
}

pub(crate) fn snapshot(st: &GameState, id: ObjectId) -> Lki {
    let o = st.object(id);
    Lki {
        card: o.card,
        chars: chars(st, id),
        controller: crate::layers::controller_of(st, id),
        is_token: o.is_token,
    }
}

/// CR 603.4: an intervening "if" clause, for the ability's controller.
pub(crate) fn condition_holds(
    st: &GameState,
    cond: Option<Condition>,
    controller: PlayerId,
) -> bool {
    let life = st.player(controller).life;
    match cond {
        None => true,
        Some(Condition::LifeAtMost(n)) => life <= n,
        Some(Condition::LifeAtLeast(n)) => life >= n,
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
        if condition_holds(st, c, t.controller) {
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
                    when: Trigger::Enters(filter),
                    intervening_if,
                    ..
                } = a
                else {
                    continue;
                };
                for &e in &entered {
                    if !st.objects.contains_key(&e) {
                        continue;
                    }
                    let hit = match filter {
                        ObjFilter::This => e == src.id,
                        ObjFilter::Matching(sel) => {
                            let o = st.object(e);
                            let c = chars(st, e);
                            matches_parts(
                                sel,
                                &c,
                                e,
                                c.controller,
                                o.is_token,
                                src.id,
                                src.controller,
                            )
                        }
                    };
                    if hit {
                        found.push(PendingTrigger {
                            controller: src.controller,
                            source: src.id,
                            source_card: src.card,
                            index,
                            source_lki: None,
                        });
                        conds.push(*intervening_if);
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
                    when: Trigger::Dies(filter),
                    intervening_if,
                    ..
                } = a
                else {
                    continue;
                };
                for d in &died {
                    let l = &lki[d];
                    let hit = match filter {
                        ObjFilter::This => *d == src.id,
                        ObjFilter::Matching(sel) => matches_parts(
                            sel,
                            &l.chars,
                            *d,
                            l.controller,
                            l.is_token,
                            src.id,
                            src.controller,
                        ),
                    };
                    if hit {
                        found.push(PendingTrigger {
                            controller: src.controller,
                            source: src.id,
                            source_card: src.card,
                            index,
                            source_lki: src.lki.clone(),
                        });
                        conds.push(*intervening_if);
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
            if let AbilityDef::Triggered {
                when: Trigger::BeginningOf { step: ts, whose },
                intervening_if,
                ..
            } = a
            {
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
                    });
                    conds.push(*intervening_if);
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
            if let AbilityDef::Triggered {
                when: Trigger::Cast { by, types },
                intervening_if,
                ..
            } = a
                && who_matches(*by, caster, src.controller)
                && (types.is_empty() || types.iter().any(|t| spell_chars.is(*t)))
            {
                found.push(PendingTrigger {
                    controller: src.controller,
                    source: src.id,
                    source_card: src.card,
                    index,
                    source_lki: None,
                });
                conds.push(*intervening_if);
            }
        }
    }
    queue_matching(st, found, conds);
}
