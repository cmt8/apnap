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

use crate::events::{DamageTarget, Event};
use crate::ids::{ObjectId, PlayerId};
use crate::layers::{Characteristics, Facts, abilities, chars, matches_parts, matching};
use crate::state::GameState;
use crate::turn::Step;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use card_model::{
    AbilityDef, CardDefId, Condition, DamagePer, DamageTo, ObjFilter, Selector, Trigger,
    TriggerStep, Who,
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
    /// What else the triggering event carries ("that player", "that much").
    #[serde(default)]
    pub facts: EventFacts,
}

/// What a triggering event carries besides its object, for the ability to refer to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventFacts {
    /// "That player": the player it happened to (the player dealt damage).
    pub player: Option<PlayerId>,
    /// "That much": its amount (the damage dealt).
    pub amount: u32,
    /// What the permanent that left the battlefield became in the zone it went to
    /// ("return it to its owner's hand").
    #[serde(default)]
    pub moved: Option<ObjectId>,
}

/// Last known information about an object that left the battlefield (CR 113.7a, 608.2h),
/// captured before the batch that moved it (CR 704.8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lki {
    pub card: CardDefId,
    pub chars: Characteristics,
    pub controller: PlayerId,
    pub owner: PlayerId,
    pub is_token: bool,
    pub tapped: bool,
    /// Whether it was an attacking / a blocking creature ("whenever an attacking creature
    /// dies").
    #[serde(default)]
    pub attacking: bool,
    #[serde(default)]
    pub blocking: bool,
}

pub(crate) fn snapshot(st: &GameState, id: ObjectId) -> Lki {
    let o = st.object(id);
    let (attacking, blocking) = crate::layers::combat_role(st, id);
    Lki {
        card: o.card,
        chars: chars(st, id),
        controller: crate::layers::controller_of(st, id),
        owner: o.owner,
        is_token: o.is_token,
        tapped: o.tapped,
        attacking,
        blocking,
    }
}

/// Does the object `id` (seen as `c` / `facts`) satisfy a trigger's object filter, for an
/// ability whose source is `source`, controlled by `controller`?
/// `host`: the permanent the source is attached to ("enchanted creature").
fn filter_hit(
    filter: &ObjFilter,
    id: ObjectId,
    c: &Characteristics,
    facts: Facts,
    source: ObjectId,
    controller: PlayerId,
    host: Option<ObjectId>,
) -> bool {
    let matches = |sel: &Selector| matches_parts(sel, c, facts, source, controller);
    match filter {
        ObjFilter::This => id == source,
        ObjFilter::Matching(sel) => matches(sel),
        // "This or another X": the source whatever it is, or a different object that is X.
        ObjFilter::ThisOr(sel) => id == source || matches(sel),
        ObjFilter::Host => host == Some(id),
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
        Some(Condition::YourTurn) => st.turn.active == controller,
        Some(Condition::NotYourTurn) => st.turn.active != controller,
        Some(Condition::ControlAtMost { what, at_most }) => {
            matching(st, what, source, controller).len() <= *at_most as usize
        }
        Some(Condition::HandAtMost(n)) => st.zones.hands[controller.index()].len() <= *n as usize,
        Some(Condition::Compare { amount, cmp, n }) => cmp.holds(
            crate::layers::count_amount(st, amount, source, controller),
            *n,
        ),
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
    /// What it is attached to (not known for a source that has left).
    host: Option<ObjectId>,
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
            host: st.object(id).attached_to,
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
    // Everything that left the battlefield, for any zone (CR 603.6c).
    let mut left: Vec<ObjectId> = Vec::new();
    // What each of them became where it went (CR 400.7).
    let mut became: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
    let dealt: Vec<Dealt> = events
        .iter()
        .filter_map(|e| match e {
            Event::DamageDealt {
                source,
                target,
                amount,
                combat,
            } => Some(Dealt {
                source: *source,
                target: *target,
                amount: *amount,
                combat: *combat,
            }),
            _ => None,
        })
        .collect();
    if !dealt.is_empty() {
        detect_damage(st, &dealt, lki);
    }
    for e in events {
        if let Event::ZoneChange {
            old,
            new,
            from: crate::zones::Zone::Battlefield,
            ..
        } = e
            && lki.contains_key(old)
        {
            left.push(*old);
            became.insert(*old, *new);
        }
        if let Event::Cycled { player, card } = e {
            detect_cycled(st, *player, *card);
        }
    }
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
                        let (attacking, blocking) = crate::layers::combat_role(st, e);
                        let facts = Facts {
                            id: e,
                            controller: c.controller,
                            owner: o.owner,
                            is_token: o.is_token,
                            tapped: o.tapped,
                            attacking,
                            blocking,
                        };
                        if filter_hit(filter, e, &c, facts, src.id, src.controller, src.host) {
                            found.push(PendingTrigger {
                                controller: src.controller,
                                source: src.id,
                                source_card: src.card,
                                index,
                                source_lki: None,
                                subject: Some(e),
                                subject_lki: None,
                                facts: EventFacts::default(),
                            });
                            conds.push(intervening_if.clone());
                        }
                    }
                }
            }
        }
    }
    if !left.is_empty() {
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
                host: None,
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
                for (filter, gone) in when.events().into_iter().filter_map(|t| match t {
                    Trigger::Dies(f) => Some((f, &died)),
                    Trigger::Leaves(f) => Some((f, &left)),
                    _ => None,
                }) {
                    for d in gone {
                        let l = &lki[d];
                        let facts = Facts {
                            id: *d,
                            controller: l.controller,
                            owner: l.owner,
                            is_token: l.is_token,
                            tapped: l.tapped,
                            attacking: l.attacking,
                            blocking: l.blocking,
                        };
                        if filter_hit(
                            filter,
                            *d,
                            &l.chars,
                            facts,
                            src.id,
                            src.controller,
                            src.host,
                        ) {
                            found.push(PendingTrigger {
                                controller: src.controller,
                                source: src.id,
                                source_card: src.card,
                                index,
                                source_lki: src.lki.clone(),
                                subject: Some(*d),
                                subject_lki: Some(l.chars.clone()),
                                facts: EventFacts {
                                    moved: became.get(d).copied(),
                                    ..EventFacts::default()
                                },
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
                        | (TriggerStep::BeginCombat, Step::BeginningOfCombat)
                        | (TriggerStep::EndCombat, Step::EndOfCombat)
                        | (TriggerStep::PrecombatMain, Step::PrecombatMain)
                        | (TriggerStep::PostcombatMain, Step::PostcombatMain)
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
                        facts: EventFacts::default(),
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
                // CR 702.83b: the only creature declared as an attacker.
                Trigger::AttacksAlone(f) if attackers.len() == 1 => Some(f),
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
                        owner: o.owner,
                        is_token: o.is_token,
                        tapped: o.tapped,
                        attacking: true,
                        blocking: false,
                    };
                    if filter_hit(
                        filter,
                        attacker,
                        &c,
                        facts,
                        src.id,
                        src.controller,
                        src.host,
                    ) {
                        found.push(PendingTrigger {
                            controller: src.controller,
                            source: src.id,
                            source_card: src.card,
                            index,
                            source_lki: None,
                            subject: Some(attacker),
                            subject_lki: None,
                            facts: EventFacts::default(),
                        });
                        conds.push(intervening_if.clone());
                    }
                }
            }
        }
    }
    queue_matching(st, found, conds);
}

/// Damage that was dealt (one source, one recipient).
struct Dealt {
    source: ObjectId,
    target: DamageTarget,
    amount: u32,
    combat: bool,
}

/// Does the object `id` — a permanent, a spell, or something that left the battlefield in
/// this batch — satisfy `filter` for an ability of `src`?
fn object_hit(
    st: &GameState,
    lki: &BTreeMap<ObjectId, Lki>,
    filter: &ObjFilter,
    id: ObjectId,
    src: &Source<'_>,
) -> bool {
    let (c, facts) = if let Some(l) = lki.get(&id) {
        (
            l.chars.clone(),
            Facts {
                id,
                controller: l.controller,
                owner: l.owner,
                is_token: l.is_token,
                tapped: l.tapped,
                attacking: l.attacking,
                blocking: l.blocking,
            },
        )
    } else if let Some(o) = st.objects.get(&id) {
        let c = chars(st, id);
        let (attacking, blocking) = crate::layers::combat_role(st, id);
        let facts = Facts {
            id,
            controller: c.controller,
            owner: o.owner,
            is_token: o.is_token,
            tapped: o.tapped,
            attacking,
            blocking,
        };
        (c, facts)
    } else {
        // Nothing is known about it any more: only "this" and "the enchanted permanent"
        // can still be told.
        return match filter {
            ObjFilter::This | ObjFilter::ThisOr(_) => id == src.id,
            ObjFilter::Host => src.host == Some(id),
            ObjFilter::Matching(_) => false,
        };
    };
    // Only a permanent is "a creature you control" (a creature spell deals no damage, but
    // an instant must not match "a permanent").
    let on_battlefield = lki.contains_key(&id)
        || st
            .objects
            .get(&id)
            .is_some_and(|o| o.zone == crate::zones::Zone::Battlefield);
    match filter {
        ObjFilter::Matching(_) if !on_battlefield => false,
        _ => filter_hit(filter, id, &c, facts, src.id, src.controller, src.host),
    }
}

/// Damage triggers (CR 120, 510.3a), from all the damage one batch dealt — simultaneously
/// (CR 510.2).
fn detect_damage(st: &mut GameState, dealt: &[Dealt], lki: &BTreeMap<ObjectId, Lki>) {
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
                let Trigger::Damage {
                    by,
                    to,
                    combat,
                    per,
                } = t
                else {
                    continue;
                };
                let hits = dealt.iter().filter(|d| {
                    combat.is_none_or(|c| c == d.combat)
                        && by
                            .as_ref()
                            .is_none_or(|f| object_hit(st, lki, f, d.source, &src))
                        && match (to, d.target) {
                            (DamageTo::Anything, _) => true,
                            (DamageTo::Player(w), DamageTarget::Player(p)) => {
                                who_matches(*w, p, src.controller)
                            }
                            (DamageTo::Permanent(f), DamageTarget::Object(o)) => {
                                object_hit(st, lki, f, o, &src)
                            }
                            _ => false,
                        }
                });
                // One trigger for each source and recipient, for each recipient, or for
                // each source; "that much" is the damage of the sources asked for
                // (CR 120.9). `None`: several different ones.
                struct Group {
                    source: ObjectId,
                    one_source: bool,
                    target: Option<DamageTarget>,
                    amount: u32,
                }
                let mut groups: Vec<Group> = Vec::new();
                for d in hits {
                    let same = |g: &Group| match per {
                        DamagePer::Each => g.source == d.source && g.target == Some(d.target),
                        DamagePer::Recipient => g.target == Some(d.target),
                        DamagePer::Source => g.source == d.source,
                    };
                    match groups.iter_mut().find(|g| same(g)) {
                        Some(g) => {
                            g.amount += d.amount;
                            g.one_source &= g.source == d.source;
                            if g.target != Some(d.target) {
                                g.target = None;
                            }
                        }
                        None => groups.push(Group {
                            source: d.source,
                            one_source: true,
                            target: Some(d.target),
                            amount: d.amount,
                        }),
                    }
                }
                for g in groups {
                    // "It": the permanent dealt damage; for damage to a player, the
                    // source (if there was just one).
                    let (subject, player) = match g.target {
                        Some(DamageTarget::Object(o)) => (Some(o), None),
                        Some(DamageTarget::Player(p)) => {
                            (g.one_source.then_some(g.source), Some(p))
                        }
                        None => (Some(g.source), None),
                    };
                    found.push(PendingTrigger {
                        controller: src.controller,
                        source: src.id,
                        source_card: src.card,
                        index,
                        source_lki: None,
                        subject,
                        subject_lki: None,
                        facts: EventFacts {
                            player,
                            amount: g.amount,
                            moved: None,
                        },
                    });
                    conds.push(intervening_if.clone());
                }
            }
        }
    }
    queue_matching(st, found, conds);
}

/// CR 702.29c: `player` cycled `card` (now wherever the discard put it). "Whenever [a
/// player] cycles a card" is watched from the battlefield; "when you cycle this card"
/// triggers from the zone the card went to.
fn detect_cycled(st: &mut GameState, player: PlayerId, card: ObjectId) {
    let mut sources = battlefield_sources(st);
    if st.objects.contains_key(&card) {
        sources.push(Source {
            id: card,
            card: st.object(card).card,
            controller: player,
            abilities: abilities(st, card),
            lki: None,
            host: None,
        });
    }
    let mut found = Vec::new();
    let mut conds = Vec::new();
    for src in sources {
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
                let hit = match t {
                    Trigger::Cycles { this: true, .. } => src.id == card,
                    Trigger::Cycles { by, this: false } => {
                        src.id != card && who_matches(*by, player, src.controller)
                    }
                    _ => false,
                };
                if hit {
                    found.push(PendingTrigger {
                        controller: src.controller,
                        source: src.id,
                        source_card: src.card,
                        index,
                        source_lki: None,
                        subject: Some(card),
                        subject_lki: None,
                        facts: EventFacts::default(),
                    });
                    conds.push(intervening_if.clone());
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
                if let Trigger::Cast { by, types, spells } = t
                    && who_matches(*by, caster, src.controller)
                    && ((types.is_empty() && spells.is_empty())
                        || types.iter().any(|t| spell_chars.is(*t))
                        || spells
                            .iter()
                            .any(|s| crate::targeting::card_matches(st, spell, s)))
                {
                    found.push(PendingTrigger {
                        controller: src.controller,
                        source: src.id,
                        source_card: src.card,
                        index,
                        source_lki: None,
                        subject: None,
                        subject_lki: None,
                        facts: EventFacts::default(),
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
            owner: you,
            is_token: false,
            tapped: false,
            attacking: false,
            blocking: false,
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
                None,
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
            you,
            None
        ));
        assert!(filter_hit(
            &ObjFilter::This,
            source,
            &chars_of(&["Elf"]),
            facts(1),
            source,
            you,
            None
        ));
        assert!(!filter_hit(
            &ObjFilter::This,
            ObjectId(2),
            &chars_of(&["Zombie"]),
            facts(2),
            source,
            you,
            None
        ));
    }
}
