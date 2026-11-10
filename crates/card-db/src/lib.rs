//! Card database: loads RON card scripts and deck lists, validates them, and compiles a
//! [`CardCatalog`] for the engine (spec §7). A malformed or semantically wrong script fails
//! **at load time** with its file path, never at game time.
//!
//! Two kinds of card file: self-contained scripts ([`CardScript`]: invented "Test …" cards,
//! token definitions, basic lands) and real-card scripts ([`real::RealCardScript`]: behaviour
//! only; printed data comes from the Scryfall cache, [`scryfall`]).

pub mod images;
pub mod real;
pub mod scryfall;

use card_model::{
    AbilityDef, Amount, Body, CardCatalog, CardDef, CardDefId, CardScript, CardType, Color, Effect,
    ManaCost, ManaProduce, ManaSymbol, ManaType, Mode, ObjRef, PlayerRef, ReplaceWith, Replaces,
    Supertype, TargetFilter, TargetRef,
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

/// Load, validate and compile self-contained card scripts into a catalog (in source order).
pub fn load_catalog(
    sources: &[Source<'_>],
    strictness: Strictness,
) -> Result<CardCatalog, Vec<LoadError>> {
    load_full_catalog(sources, &[], &scryfall::ScryfallData::default(), strictness)
}

/// Like [`load_catalog`], then appends the real cards in `scripts` (in source order),
/// compiled against the printed data in `data`. Self-contained cards keep their ids.
pub fn load_full_catalog(
    sources: &[Source<'_>],
    scripts: &[Source<'_>],
    data: &scryfall::ScryfallData,
    strictness: Strictness,
) -> Result<CardCatalog, Vec<LoadError>> {
    let mut errors = Vec::new();
    let mut entries: Vec<(&str, CardScript)> = Vec::new();
    for src in sources {
        match parse_script(*src) {
            Ok(s) => entries.push((src.path, s)),
            Err(e) => errors.push(e),
        }
    }
    for src in scripts {
        match compile_real(*src, data) {
            Ok(s) => entries.push((src.path, s)),
            Err(mut e) => errors.append(&mut e),
        }
    }
    let mut catalog = CardCatalog::default();
    let mut paths: Vec<&str> = Vec::new();
    let mut names = BTreeSet::new();
    for (path, script) in entries {
        let problems = validate(&script, strictness);
        // Definitions are found by key if they have one (tokens that share a name).
        let called = script.card.key.clone().unwrap_or(script.card.name.clone());
        if !names.insert(called.clone()) {
            errors.push(err(path, format!("duplicate card name {called:?}")));
        }
        if script.card.key.is_some() && !script.card.token {
            errors.push(err(path, "only token definitions have a key"));
        }
        if problems.is_empty() {
            catalog.add(script.card);
            paths.push(path);
        } else {
            errors.extend(problems.into_iter().map(|m| err(path, m)));
        }
    }
    // Second pass: CreateToken must name a token definition in the catalog.
    for (def, path) in catalog.defs.iter().zip(&paths) {
        for name in token_names(def) {
            match catalog.find(&name) {
                Some(id) if catalog.get(id).token => {}
                Some(_) => errors.push(err(path, format!("{name:?} is not a token definition"))),
                None => errors.push(err(path, format!("unknown token {name:?}"))),
            }
        }
        if def.token && def.mana_cost.is_some() {
            errors.push(err(path, "tokens have no mana cost (CR 111.3)"));
        }
    }
    if errors.is_empty() {
        Ok(catalog)
    } else {
        Err(errors)
    }
}

/// Parse a real-card script without compiling it.
pub fn parse_real(src: Source<'_>) -> Result<real::RealCardScript, LoadError> {
    ron::from_str(src.text).map_err(|e| err(src.path, e.to_string()))
}

/// Parse a real-card script and compile it against its printed data.
pub fn compile_real(
    src: Source<'_>,
    data: &scryfall::ScryfallData,
) -> Result<CardScript, Vec<LoadError>> {
    let script = parse_real(src).map_err(|e| vec![e])?;
    let Some(card) = data.get(&script.name) else {
        return Err(vec![err(
            src.path,
            format!("{:?}: {}", script.name, scryfall::FETCH_HINT),
        )]);
    };
    match real::compile(&script, card) {
        Ok(card) => Ok(CardScript {
            verified: script.verified,
            tests: script.tests,
            card,
        }),
        Err(problems) => Err(problems.into_iter().map(|m| err(src.path, m)).collect()),
    }
}

fn bodies(def: &CardDef) -> Vec<&Body> {
    def.abilities
        .iter()
        .filter_map(|a| match a {
            AbilityDef::Spell(b)
            | AbilityDef::Activated { body: b, .. }
            | AbilityDef::Triggered { body: b, .. } => Some(b),
            _ => None,
        })
        .collect()
}

fn token_names(def: &CardDef) -> Vec<String> {
    fn walk(e: &Effect, out: &mut Vec<String>) {
        match e {
            Effect::Seq(v) => v.iter().for_each(|x| walk(x, out)),
            Effect::CreateToken { token, .. } => out.push(token.clone()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for b in bodies(def) {
        for m in b.modes() {
            walk(&m.effect, &mut out);
        }
    }
    out
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
    if !land && !c.token && c.mana_cost.is_none() {
        p.push("non-land, non-token cards need a mana cost".into());
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
            AbilityDef::Triggered { body, .. } => {
                check_body(body, false, &mut |m| p.push(at(m)));
            }
            AbilityDef::Static { affected, effects } => {
                if effects.is_empty() {
                    p.push(at("a static ability needs at least one effect".into()));
                }
                // The affected set is worked out inside the layer walk, before power and
                // toughness are final (CR 613.4): it can't depend on them.
                if let card_model::Affected::All(s) = affected
                    && (s.power_min.is_some() || s.toughness_min.is_some())
                {
                    p.push(at(
                        "a static ability can't select by power or toughness (CR 613)".into(),
                    ));
                }
            }
            AbilityDef::Enchant(f) => {
                if !(c.has_type(CardType::Enchantment)
                    && c.creature_types.iter().any(|t| t == "Aura"))
                {
                    p.push(at("only Auras have enchant (CR 702.5a)".into()));
                }
                if *f != TargetFilter::Creature {
                    p.push(at("M3 supports only \"Enchant creature\" (D-029)".into()));
                }
            }
            AbilityDef::Replacement { replaces, with } => {
                let ok = matches!(
                    (replaces, with),
                    (Replaces::CreateTokens, ReplaceWith::Twice)
                        | (Replaces::PutCountersOnYourPermanents, ReplaceWith::Twice)
                        | (Replaces::CardToGraveyard, ReplaceWith::Exile)
                        | (
                            Replaces::ThisDies,
                            ReplaceWith::Exile | ReplaceWith::ShuffleIntoLibrary
                        )
                        | (
                            Replaces::ThisEnters,
                            ReplaceWith::EntersTapped | ReplaceWith::EntersWithCounters { .. }
                        )
                );
                if !ok {
                    p.push(at(format!(
                        "unsupported replacement {replaces:?} -> {with:?}"
                    )));
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
            p(format!(
                "bad mode count {min}..={max} for {} modes",
                modes.len()
            ));
        }
    }
    for (mi, mode) in body.modes().iter().enumerate() {
        check_effect(&mode.effect, mode, has_x, &mut |m| {
            p(format!("mode {mi}: {m}"))
        });
    }
}

/// A selector used where "another" has no meaning must not say it.
fn selector(_s: &card_model::Selector, _p: &mut dyn FnMut(String)) {}

fn check_effect(e: &Effect, mode: &Mode, has_x: bool, p: &mut dyn FnMut(String)) {
    let slot = |t: &TargetRef| {
        let TargetRef::Target(i) = t;
        mode.targets.get(*i as usize)
    };
    let amount = |a: &Amount, p: &mut dyn FnMut(String)| match a {
        Amount::X if !has_x => {
            p("Amount::X used but nothing defines X (no {X} in the cost, no SetX)".into());
        }
        Amount::Count(s) => selector(s, p),
        _ => {}
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
    let obj = |r: &ObjRef, p: &mut dyn FnMut(String)| {
        if let ObjRef::Target(i) = r {
            match mode.targets.get(*i as usize) {
                Some(f) if f.allows_permanents() => {}
                Some(f) => p(format!("ObjRef::Target({i}) refers to a {f:?} target")),
                None => p(format!("ObjRef::Target({i}) has no target slot")),
            }
        }
    };
    match e {
        Effect::Seq(v) => {
            // "…where X is …": a SetX defines X for the instructions after it.
            let mut has_x = has_x;
            for x in v {
                check_effect(x, mode, has_x, p);
                has_x |= matches!(x, Effect::SetX(_));
            }
        }
        Effect::SetX(a) => amount(a, p),
        Effect::DestroyAll(s) => selector(s, p),
        Effect::DamageEach { each, amount: a } => {
            selector(each, p);
            amount(a, p);
        }
        Effect::Exile { what } => obj(what, p),
        Effect::ExileGraveyard { who } => player(who, p),
        Effect::Mill { who, amount: a } => {
            player(who, p);
            amount(a, p);
        }
        Effect::ApplyAll {
            affected, effects, ..
        } => {
            selector(affected, p);
            if effects.is_empty() {
                p("ApplyAll needs at least one effect".into());
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
        Effect::DamageEachPlayer { amount: a } => amount(a, p),
        Effect::Destroy { what } | Effect::ReturnToHand { what } | Effect::Apply { what, .. } => {
            obj(what, p)
        }
        Effect::PutCounters { what, count, .. } => {
            obj(what, p);
            amount(count, p);
        }
        Effect::CreateToken { count, who, .. } => {
            amount(count, p);
            player(who, p);
        }
        Effect::PreventNext { to, .. } => {
            if slot(to).is_none() {
                p(format!("{to:?} has no target slot"));
            }
        }
    }
}

/// A deck list file: `Deck(name: "...", cards: [(12, "Forest"), ...])`.
#[derive(Clone, Debug, Deserialize)]
pub struct DeckFile {
    pub name: String,
    /// CR 903.3: the commander (Commander decks only), not counted in `cards`.
    #[serde(default)]
    pub commander: Option<String>,
    pub cards: Vec<(u32, String)>,
}

/// Parse a deck list without resolving its card names.
pub fn parse_deck_file(src: Source<'_>) -> Result<DeckFile, LoadError> {
    ron::from_str(src.text).map_err(|e| err(src.path, e.to_string()))
}

/// A deck resolved against a catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deck {
    pub name: String,
    pub commander: Option<CardDefId>,
    pub cards: Vec<CardDefId>,
}

/// Parse a deck list (with an optional commander) and resolve names against `catalog`.
pub fn load_deck_file(src: Source<'_>, catalog: &CardCatalog) -> Result<Deck, LoadError> {
    let deck: DeckFile = ron::from_str(src.text).map_err(|e| err(src.path, e.to_string()))?;
    let find = |name: &str| {
        catalog
            .find(name)
            .ok_or_else(|| err(src.path, format!("unknown card {name:?}")))
    };
    let mut cards = Vec::new();
    for (n, name) in &deck.cards {
        let id = find(name)?;
        cards.extend(std::iter::repeat_n(id, *n as usize));
    }
    let commander = deck.commander.as_deref().map(find).transpose()?;
    Ok(Deck {
        name: deck.name,
        commander,
        cards,
    })
}

/// CR 903.4: a card's color identity — the colors of mana symbols in its mana cost or rules
/// text, plus its color indicator. (Basic land types are handled by CR 903.5d.)
pub fn color_identity(def: &CardDef) -> Vec<Color> {
    let mut out: Vec<Color> = def.colors.clone().unwrap_or_default();
    let add_cost = |m: &ManaCost, out: &mut Vec<Color>| {
        for s in &m.symbols {
            if let ManaSymbol::Colored(c) = s {
                out.push(*c);
            }
        }
    };
    if let Some(m) = &def.mana_cost {
        add_cost(m, &mut out);
    }
    for a in &def.abilities {
        match a {
            AbilityDef::Activated { cost, .. } | AbilityDef::Mana { cost, .. } => {
                if let Some(m) = &cost.mana {
                    add_cost(m, &mut out);
                }
                if let AbilityDef::Mana {
                    produce: ManaProduce::One(t),
                    ..
                } = a
                    && let Some(c) = Color::ALL
                        .into_iter()
                        .find(|c| ManaType::from_color(*c) == *t)
                {
                    out.push(c);
                }
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

/// CR 903.5: Commander deck construction rules (and 903.3 for the commander).
pub fn validate_commander_deck(catalog: &CardCatalog, deck: &Deck) -> Vec<String> {
    let mut p = Vec::new();
    let Some(cmd) = deck.commander else {
        return vec!["a Commander deck needs a commander (CR 903.3)".into()];
    };
    let c = catalog.get(cmd);
    if !(c.has_type(CardType::Creature) && c.supertypes.contains(&Supertype::Legendary)) {
        p.push(format!("{} isn't a legendary creature (CR 903.3)", c.name));
    }
    if deck.cards.len() + 1 != 100 {
        p.push(format!(
            "{} cards including the commander; must be exactly 100 (CR 903.5a)",
            deck.cards.len() + 1
        ));
    }
    let identity = color_identity(c);
    let mut seen = BTreeSet::new();
    for &id in deck.cards.iter().chain(std::iter::once(&cmd)) {
        let d = catalog.get(id);
        if d.token {
            p.push(format!("{} is a token definition", d.name));
        }
        if !d.supertypes.contains(&Supertype::Basic) && !seen.insert(id) {
            p.push(format!("more than one {} (CR 903.5b)", d.name));
        }
        if let Some(bad) = color_identity(d)
            .into_iter()
            .find(|x| !identity.contains(x))
        {
            p.push(format!(
                "{} has {bad:?} outside the commander's color identity (CR 903.5c)",
                d.name
            ));
        }
        if let Some(t) = d
            .land_types
            .iter()
            .find(|t| !identity.contains(&t.produces()))
        {
            p.push(format!(
                "{} has the basic land type {t:?} (CR 903.5d)",
                d.name
            ));
        }
    }
    p
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
