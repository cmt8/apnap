//! Card database: loads RON card scripts and deck lists, validates them, and compiles a
//! [`CardCatalog`] for the engine (spec §7). A malformed or semantically wrong script fails
//! **at load time** with its file path, never at game time.
//!
//! Scryfall bulk-data import (printed characteristics for real cards, fetched at runtime
//! into the XDG cache, spec §12) arrives in M4. Until then every script carries its own
//! printed characteristics, and only invented "Test …" cards and basic lands exist.

use card_model::{
    AbilityDef, Amount, Body, CardCatalog, CardDef, CardDefId, CardScript, CardType, Effect, Mode,
    PlayerRef, TargetFilter, TargetRef,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fmt;

/// A card or deck file: its repo-relative path (for error messages) and contents.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub path: &'a str,
    pub text: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strictness {
    /// Unverified cards are rejected (the default for play; CLAUDE.md invariant 8).
    Strict,
    /// Unverified cards load (authoring and coverage tooling only).
    AllowUnverified,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadError {
    pub path: String,
    pub msg: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.msg)
    }
}

impl std::error::Error for LoadError {}

fn err(path: &str, msg: impl Into<String>) -> LoadError {
    LoadError {
        path: path.into(),
        msg: msg.into(),
    }
}

/// Parse one card script without validating it.
pub fn parse_script(src: Source<'_>) -> Result<CardScript, LoadError> {
    ron::from_str(src.text).map_err(|e| err(src.path, e.to_string()))
}

/// Load, validate and compile card scripts into a catalog (in source order).
pub fn load_catalog(
    sources: &[Source<'_>],
    strictness: Strictness,
) -> Result<CardCatalog, Vec<LoadError>> {
    let mut errors = Vec::new();
    let mut catalog = CardCatalog::default();
    let mut names = BTreeSet::new();
    for src in sources {
        let script = match parse_script(*src) {
            Ok(s) => s,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let problems = validate(&script, strictness);
        if !names.insert(script.card.name.clone()) {
            errors.push(err(src.path, format!("duplicate card name {:?}", script.card.name)));
        }
        if problems.is_empty() {
            catalog.add(script.card);
        } else {
            errors.extend(problems.into_iter().map(|m| err(src.path, m)));
        }
    }
    if errors.is_empty() {
        Ok(catalog)
    } else {
        Err(errors)
    }
}

/// Semantic checks for one script. Returns every problem found.
pub fn validate(script: &CardScript, strictness: Strictness) -> Vec<String> {
    let c = &script.card;
    let mut p = Vec::new();
    if c.name.trim().is_empty() {
        p.push("empty name".into());
    }
    if strictness == Strictness::Strict && !script.verified {
        p.push("unverified card in strict mode (CLAUDE.md invariant 8)".into());
    }
    if script.verified && script.tests.is_empty() {
        p.push("verified cards must name at least one test (spec §10.2)".into());
    }
    let creature = c.has_type(CardType::Creature);
    if creature != (c.power.is_some() && c.toughness.is_some()) {
        p.push("power/toughness must be given exactly for creatures".into());
    }
    let land = c.has_type(CardType::Land);
    if land && c.mana_cost.is_some() {
        p.push("lands have no mana cost (they are played, not cast)".into());
    }
    if !land && c.mana_cost.is_none() {
        p.push("non-land cards need a mana cost (M1)".into());
    }
    if !c.land_types.is_empty() && !land {
        p.push("basic land types on a non-land".into());
    }
    let instant_or_sorcery = c.has_type(CardType::Instant) || c.has_type(CardType::Sorcery);
    let spells = c
        .abilities
        .iter()
        .filter(|a| matches!(a, AbilityDef::Spell(_)))
        .count();
    if instant_or_sorcery && spells != 1 {
        p.push("an instant or sorcery needs exactly one Spell ability".into());
    }
    if !instant_or_sorcery && spells != 0 {
        p.push("only instants and sorceries have a Spell ability".into());
    }
    let mut kw = c.keywords.clone();
    kw.sort();
    kw.dedup();
    if kw.len() != c.keywords.len() {
        p.push("duplicate keyword".into());
    }
    let card_has_x = c.mana_cost.as_ref().is_some_and(|m| m.has_x());
    for (i, a) in c.abilities.iter().enumerate() {
        let at = |m: String| format!("ability {i}: {m}");
        match a {
            AbilityDef::Spell(body) => check_body(body, card_has_x, &mut |m| p.push(at(m))),
            AbilityDef::Activated { cost, body, .. } => {
                let has_x = cost.mana.as_ref().is_some_and(|m| m.has_x());
                if cost.mana.is_none() && !cost.tap {
                    p.push(at("activated ability with no cost".into()));
                }
                check_body(body, has_x, &mut |m| p.push(at(m)));
            }
            AbilityDef::Mana { cost, .. } => {
                if !(cost.tap && cost.mana.is_none()) {
                    p.push(at("M1 mana abilities must cost exactly {T} (D-021)".into()));
                }
            }
        }
    }
    p
}

fn check_body(body: &Body, has_x: bool, p: &mut dyn FnMut(String)) {
    if let Body::Modal { min, max, modes } = body {
        if modes.len() < 2 {
            p("a modal body needs at least two modes (CR 700.2)".into());
        }
        if *min < 1 || min > max || *max as usize > modes.len() {
            p(format!("bad mode count {min}..={max} for {} modes", modes.len()));
        }
    }
    for (mi, mode) in body.modes().iter().enumerate() {
        check_effect(&mode.effect, mode, has_x, &mut |m| p(format!("mode {mi}: {m}")));
    }
}

fn check_effect(e: &Effect, mode: &Mode, has_x: bool, p: &mut dyn FnMut(String)) {
    let slot = |t: &TargetRef| {
        let TargetRef::Target(i) = t;
        mode.targets.get(*i as usize).copied()
    };
    let amount = |a: &Amount, p: &mut dyn FnMut(String)| {
        if matches!(a, Amount::X) && !has_x {
            p("Amount::X used but the cost has no {X}".into());
        }
    };
    let player = |r: &PlayerRef, p: &mut dyn FnMut(String)| {
        if let PlayerRef::Target(i) = r {
            match mode.targets.get(*i as usize) {
                Some(f) if f.allows_players() => {}
                Some(f) => p(format!("PlayerRef::Target({i}) refers to a {f:?} target")),
                None => p(format!("PlayerRef::Target({i}) has no target slot")),
            }
        }
    };
    match e {
        Effect::Seq(v) => {
            for x in v {
                check_effect(x, mode, has_x, p);
            }
        }
        Effect::DealDamage { to, amount: a } => {
            match slot(to) {
                None => p(format!("{to:?} has no target slot")),
                Some(TargetFilter::Spell) => p("can't deal damage to a spell".into()),
                Some(_) => {}
            }
            amount(a, p);
        }
        Effect::Draw { who, amount: a }
        | Effect::GainLife { who, amount: a }
        | Effect::LoseLife { who, amount: a } => {
            player(who, p);
            amount(a, p);
        }
        Effect::Counter { what } => match slot(what) {
            Some(TargetFilter::Spell) => {}
            other => p(format!("Counter needs a Spell target slot, got {other:?}")),
        },
        Effect::EachPlayer(_) => {}
    }
}

/// A deck list file: `Deck(name: "...", cards: [(12, "Forest"), ...])`.
#[derive(Clone, Debug, Deserialize)]
pub struct DeckFile {
    pub name: String,
    pub cards: Vec<(u32, String)>,
}

/// Parse a deck list and resolve its card names against `catalog`, in list order.
pub fn load_deck(src: Source<'_>, catalog: &CardCatalog) -> Result<Vec<CardDefId>, LoadError> {
    let deck: DeckFile = ron::from_str(src.text).map_err(|e| err(src.path, e.to_string()))?;
    let mut out = Vec::new();
    for (n, name) in &deck.cards {
        let id = catalog
            .find(name)
            .ok_or_else(|| err(src.path, format!("unknown card {name:?}")))?;
        out.extend(std::iter::repeat_n(id, *n as usize));
    }
    Ok(out)
}

/// `(card name, test names)` for every parseable script; used by the CI check that every
/// verified card's tests exist.
pub fn declared_tests(sources: &[Source<'_>]) -> Vec<(String, Vec<String>)> {
    sources
        .iter()
        .filter_map(|s| parse_script(*s).ok())
        .map(|s| (s.card.name, s.tests))
        .collect()
}

/// Printed definition lookup helper for tooling.
pub fn def<'a>(catalog: &'a CardCatalog, name: &str) -> Option<&'a CardDef> {
    catalog.find(name).map(|id| catalog.get(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(text: &str) -> Source<'_> {
        Source {
            path: "test.ron",
            text,
        }
    }

    const SHOCK: &str = r#"CardScript(
        verified: true,
        tests: ["t"],
        card: (
            name: "Test Shock",
            mana_cost: Some("{R}"),
            card_types: [Instant],
            abilities: [Spell(Effect((
                targets: [AnyTarget],
                effect: DealDamage(to: Target(0), amount: Fixed(2)),
            )))],
        ),
    )"#;

    #[test]
    fn loads_a_valid_script() {
        let cat = load_catalog(&[src(SHOCK)], Strictness::Strict).unwrap();
        assert_eq!(cat.defs[0].name, "Test Shock");
    }

    #[test]
    fn rejects_bad_target_slot_and_x_without_cost() {
        let bad = SHOCK
            .replace("to: Target(0)", "to: Target(1)")
            .replace("Fixed(2)", "X");
        let errs = load_catalog(&[src(&bad)], Strictness::Strict).unwrap_err();
        let all: String = errs.iter().map(|e| e.msg.clone() + "\n").collect();
        assert!(all.contains("Target(1)"), "{all}");
        assert!(all.contains("Amount::X"), "{all}");
    }

    #[test]
    fn rejects_unverified_in_strict_mode_and_typos_at_parse_time() {
        let unverified = SHOCK.replace("verified: true", "verified: false");
        assert!(load_catalog(&[src(&unverified)], Strictness::Strict).is_err());
        assert!(load_catalog(&[src(&unverified)], Strictness::AllowUnverified).is_ok());
        let typo = SHOCK.replace("DealDamage", "DealDamagee");
        let e = load_catalog(&[src(&typo)], Strictness::AllowUnverified).unwrap_err();
        assert!(e[0].msg.contains("DealDamagee"), "{}", e[0].msg);
    }
}
