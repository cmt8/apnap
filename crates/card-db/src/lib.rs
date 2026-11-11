//! Card database: loads RON card scripts and deck lists, validates them, and compiles a
//! [`CardCatalog`] for the engine (spec §7). A malformed or semantically wrong script fails
//! **at load time** with its file path, never at game time.
//!
//! Two kinds of card file: self-contained scripts ([`CardScript`]: invented "Test …" cards,
//! token definitions, basic lands) and real-card scripts ([`real::RealCardScript`]: behaviour
//! only; printed data comes from the Scryfall cache, [`scryfall`]).

pub mod forge;
pub mod images;
pub mod real;
pub mod scryfall;

use card_model::{
    AbilityDef, ActivationZone, Amount, Body, CardCatalog, CardDef, CardDefId, CardScript,
    CardType, Condition, Cost, Effect, Mode, ObjRef, PlayerRef, ReplaceWith, Replaces, Supertype,
    TargetFilter, TargetRef, Trigger,
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
    // "Has [quoted ability]" names an ability of the card's `granted` list.
    let text = format!("{:?}", (&c.abilities, &c.granted));
    for list in text.split("GrantAbilities([").skip(1) {
        let list = list.split(']').next().unwrap_or_default();
        if list.is_empty() {
            p.push("an effect that gives no ability".into());
        }
        for index in list.split(", ").filter(|i| !i.is_empty()) {
            if index
                .parse::<usize>()
                .map_or(true, |i| i >= c.granted.len())
            {
                p.push(format!("no granted ability {index}"));
            }
        }
    }
    // The abilities lasting effects carry ("target creature can't be blocked this turn").
    let mut lasting: Vec<usize> = Vec::new();
    for list in text.split("Lasting { abilities: [").skip(1) {
        let list = list.split(']').next().unwrap_or_default();
        if list.is_empty() {
            p.push("a lasting effect of nothing".into());
        }
        for index in list.split(", ").filter(|i| !i.is_empty()) {
            match index.parse::<usize>() {
                Ok(i) if i < c.granted.len() => lasting.push(i),
                _ => p.push(format!("no granted ability {index}")),
            }
        }
    }
    if text.contains("while_source: true") && instant_or_sorcery {
        p.push("an effect of a spell can't last while its source stays".into());
    }
    let printed = c.abilities.iter().enumerate().map(|(i, a)| (i, a, false));
    let given = c.granted.iter().enumerate().map(|(i, a)| (i, a, true));
    for (i, a, granted) in printed.chain(given) {
        let at = |m: String| {
            if granted {
                format!("granted ability {i}: {m}")
            } else {
                format!("ability {i}: {m}")
            }
        };
        // What a lasting effect carries works on no object: rules about combat and
        // prevention so far. Only there does anything mean "what the effect is about".
        let carried = granted && lasting.contains(&i);
        if carried {
            let fits = match a {
                AbilityDef::CombatRule { affected, .. } => {
                    *affected != card_model::Affected::Enchanted
                }
                AbilityDef::Replacement { replaces, with } => {
                    matches!(replaces, Replaces::Damage { .. }) && *with == ReplaceWith::Prevent
                }
                _ => false,
            };
            if !fits {
                p.push(at("not something a lasting effect can carry (yet)".into()));
                continue;
            }
        } else if format!("{a:?}").contains("Remembered") {
            p.push(at(
                "only what a lasting effect carries is about something remembered".into(),
            ));
        }
        // An ability another object is given works on the battlefield, for whatever has
        // it: nothing about casting, about the card it is written on, or about a layer
        // that is over by the time abilities are given (CR 613.1f).
        if granted && !carried {
            let fits = match a {
                AbilityDef::Spell(_)
                | AbilityDef::AdditionalCost(_)
                | AbilityDef::Enchant(_)
                | AbilityDef::SelfCostReduction(_) => false,
                AbilityDef::Activated { from, cycling, .. } => {
                    *from == ActivationZone::Battlefield && !*cycling
                }
                AbilityDef::Static {
                    affected, effects, ..
                } => {
                    *affected != card_model::Affected::Enchanted
                        && effects.iter().all(|e| {
                            use card_model::ContinuousEffect as E;
                            !matches!(
                                e,
                                E::GainControl
                                    | E::AddTypes { .. }
                                    | E::SetColors(_)
                                    | E::SetPTBy { .. }
                            )
                        })
                }
                AbilityDef::CombatRule { affected, .. } => {
                    *affected != card_model::Affected::Enchanted
                }
                AbilityDef::Replacement { replaces, .. } => *replaces != Replaces::ThisEnters,
                AbilityDef::Mana { .. }
                | AbilityDef::Triggered { .. }
                | AbilityDef::CostReduction { .. } => true,
            };
            if !fits {
                p.push(at("not an ability another object can be given".into()));
                continue;
            }
        }
        // "The sacrificed creature's power" needs a sacrifice chosen as its cost is paid.
        if format!("{a:?}").contains("SacrificedPower") {
            let paid = match a {
                AbilityDef::Activated { cost, .. } => cost.sacrifice.is_some(),
                AbilityDef::Spell(_) => c
                    .abilities
                    .iter()
                    .any(|x| matches!(x, AbilityDef::AdditionalCost(c) if c.sacrifice.is_some())),
                _ => false,
            };
            if !paid {
                p.push(at(
                    "the sacrificed creature's power, but nothing is sacrificed as a cost".into(),
                ));
            }
        }
        // Only an instant or sorcery spell's own effect says where the spell goes; "that
        // card" needs a permanent that left the battlefield.
        if !matches!(a, AbilityDef::Spell(_)) && format!("{a:?}").contains("ThisSpellTo") {
            p.push(at("ThisSpellTo outside a spell ability".into()));
        }
        if format!("{a:?}").contains("TriggeringCard") {
            let left = match a {
                AbilityDef::Triggered { when, .. } => when
                    .events()
                    .iter()
                    .all(|t| matches!(t, Trigger::Dies(_) | Trigger::Leaves(_))),
                _ => false,
            };
            if !left {
                p.push(at(
                    "\"that card\" needs a trigger on a permanent leaving the battlefield".into(),
                ));
            }
        }
        // "That player" and "that much" need an event that has them.
        let text = format!("{a:?}");
        for (word, needs_player) in [("TriggeringPlayer", true), ("TriggeringAmount", false)] {
            if !text.contains(word) {
                continue;
            }
            let given = match a {
                AbilityDef::Triggered { when, .. } => when.events().iter().all(|t| match t {
                    Trigger::Damage { to, .. } => {
                        !needs_player || matches!(to, card_model::DamageTo::Player(_))
                    }
                    Trigger::GainsLife(_) | Trigger::LosesLife(_) => true,
                    Trigger::Draws { .. } | Trigger::Discards(_) => needs_player,
                    _ => false,
                }),
                _ => false,
            };
            if !given {
                p.push(at(format!(
                    "{word}: the ability's trigger event doesn't have one"
                )));
            }
        }
        // "If [something counted] is N or more" is asked with no spell or ability
        // resolving, and for a static ability inside the layer walk.
        let (cond, in_static) = match a {
            AbilityDef::Activated { only_if, .. } | AbilityDef::Mana { only_if, .. } => {
                (only_if.as_ref(), false)
            }
            AbilityDef::Triggered { intervening_if, .. } => (intervening_if.as_ref(), false),
            AbilityDef::Static { only_if, .. } => (only_if.as_ref(), true),
            _ => (None, false),
        };
        if let Some(card_model::Condition::Compare { amount, .. }) = cond {
            if !amount.stands_alone() {
                p.push(at(
                    "a condition can't count something that needs the ability to resolve".into(),
                ));
            }
            if in_static && !amount.countable_in_layers() {
                p.push(at(
                    "a static ability's condition can't count that (CR 613.4c)".into(),
                ));
            }
        }
        // "This card" (off the battlefield) means something only where the ability works
        // from a hand or graveyard, or triggers on the card being cycled.
        if format!("{a:?}").contains("ThisCard") {
            let off_battlefield = match a {
                AbilityDef::Activated { from, .. } => *from != ActivationZone::Battlefield,
                AbilityDef::Triggered { when, .. } => when
                    .events()
                    .iter()
                    .all(|t| matches!(t, Trigger::Cycles { this: true, .. })),
                _ => false,
            };
            if !off_battlefield {
                p.push(at(
                    "\"this card\" in an ability that works on the battlefield".into(),
                ));
            }
        }
        match a {
            AbilityDef::Spell(body) => check_body(body, card_has_x, &mut |m| p.push(at(m))),
            AbilityDef::Activated {
                cost,
                body,
                from,
                cycling,
                ..
            } => {
                // CR 113.6: the cost can name the card only where the ability works from.
                let on_battlefield = cost.tap || cost.sacrifice_self;
                let fits = match from {
                    ActivationZone::Battlefield => !cost.discard_self && !cost.exile_self,
                    ActivationZone::Hand => !on_battlefield && !cost.exile_self,
                    ActivationZone::Graveyard => !on_battlefield && !cost.discard_self,
                };
                if !fits {
                    p.push(at(
                        "the cost uses the card in a zone the ability doesn't work from".into(),
                    ));
                }
                if *cycling && !(*from == ActivationZone::Hand && cost.discard_self) {
                    p.push(at(
                        "cycling is \"[cost], discard this card\" from the hand (CR 702.29a)"
                            .into(),
                    ));
                }
                let has_x = cost.mana.as_ref().is_some_and(|m| m.has_x());
                if *cost == Cost::default() {
                    p.push(at("activated ability with no cost".into()));
                }
                check_cost(cost, &mut |m| p.push(at(m)));
                check_body(body, has_x, &mut |m| p.push(at(m)));
            }
            AbilityDef::Mana {
                cost,
                produce,
                spend,
                ..
            } => {
                // `{T}`, optionally with "sacrifice this" (D-052). A mana ability that
                // costs mana, or anything chosen, is not modelled.
                let allowed = Cost {
                    tap: true,
                    sacrifice_self: cost.sacrifice_self,
                    ..Cost::default()
                };
                if *cost != allowed {
                    p.push(at(
                        "a mana ability must cost {T}, or {T} and sacrificing itself (D-052)"
                            .into(),
                    ));
                }
                if produce.count() == 0 || produce.options().is_empty() {
                    p.push(at("a mana ability that adds nothing".into()));
                }
                if spend
                    .as_ref()
                    .is_some_and(|r| r.spells.is_empty() && r.abilities.is_empty())
                {
                    p.push(at("mana that can be spent on nothing".into()));
                }
            }
            AbilityDef::CombatRule { affected, .. } => {
                // "Enchanted creature can't attack", "equipped creature can't be blocked":
                // only on something that is attached to a creature.
                if matches!(affected, card_model::Affected::Enchanted)
                    && !c
                        .creature_types
                        .iter()
                        .any(|t| t == "Aura" || t == "Equipment")
                {
                    p.push(at(
                        "only an Aura or Equipment has an enchanted or equipped creature".into(),
                    ));
                }
            }
            AbilityDef::AdditionalCost(cost) => {
                if cost.mana.is_some()
                    || cost.tap
                    || cost.sacrifice_self
                    || cost.discard_self
                    || cost.exile_self
                {
                    p.push(at(
                        "an additional cost is a sacrifice, a discard or life, nothing else".into(),
                    ));
                }
                if cost.sacrifice.is_none() && cost.discard.is_none() && cost.life == 0 {
                    p.push(at("an additional cost of nothing".into()));
                }
                if c.has_type(CardType::Land) {
                    p.push(at("lands aren't cast: no additional cost".into()));
                }
                check_cost(cost, &mut |m| p.push(at(m)));
            }
            AbilityDef::Triggered { body, .. } => {
                check_body(body, false, &mut |m| p.push(at(m)));
            }
            AbilityDef::Static {
                affected,
                effects,
                only_if,
            } => {
                if effects.is_empty() {
                    p.push(at("a static ability needs at least one effect".into()));
                }
                // "+1/+1 for each [selector]" is re-counted inside the layer walk: only a
                // count of permanents that doesn't look at power or toughness can be
                // (CR 613.4c; anything else would depend on the order of other effects).
                for e in effects {
                    // CR 604.3: a characteristic-defining ability defines its own card.
                    if let card_model::ContinuousEffect::SetPTBy { power, toughness } = e {
                        if *affected != card_model::Affected::This || only_if.is_some() {
                            p.push(at(
                                "a characteristic-defining ability is about its own card, \
                                 unconditionally"
                                    .into(),
                            ));
                        }
                        if power.is_none() && toughness.is_none() {
                            p.push(at(
                                "a characteristic-defining ability that defines nothing".into()
                            ));
                        }
                        if [power, toughness]
                            .into_iter()
                            .flatten()
                            .any(|a| !a.countable_in_layers())
                        {
                            p.push(at(
                                "a characteristic-defining ability can't count that (CR 613.4a)"
                                    .into(),
                            ));
                        }
                        if c.power.is_none() || c.toughness.is_none() {
                            p.push(at(
                                "defines the power or toughness of a card without them".into()
                            ));
                        }
                    }
                    if let card_model::ContinuousEffect::ModifyPTBy { power, toughness } = e {
                        for a in [&power.amount, &toughness.amount] {
                            if !a.countable_in_layers() {
                                p.push(at("a static ModifyPTBy can only count permanents \
                                           (not by power or toughness)"
                                    .into()));
                            }
                        }
                    }
                }
                // The affected set is worked out inside the layer walk, before power and
                // toughness are final (CR 613.4): it can't depend on them.
                if let card_model::Affected::All(s) = affected
                    && s.looks_at_pt()
                {
                    p.push(at(
                        "a static ability can't select by power or toughness (CR 613)".into(),
                    ));
                }
            }
            AbilityDef::CostReduction { amount, .. } => {
                if *amount == 0 {
                    p.push(at("a cost reduction of nothing".into()));
                }
            }
            AbilityDef::SelfCostReduction(a) => {
                if matches!(a, Amount::X | Amount::SourcePower) {
                    p.push(at(
                        "a spell's own cost reduction can't depend on X or its power".into(),
                    ));
                }
            }
            AbilityDef::Enchant(f) => {
                if !(c.has_type(CardType::Enchantment)
                    && c.creature_types.iter().any(|t| t == "Aura"))
                {
                    p.push(at("only Auras have enchant (CR 702.5a)".into()));
                }
                if !matches!(f, TargetFilter::Creature | TargetFilter::Permanent(_)) {
                    p.push(at("an Aura enchants a permanent (players: not yet)".into()));
                }
            }
            AbilityDef::Replacement { replaces, with } => {
                let ok = matches!(
                    (replaces, with),
                    (Replaces::CreateTokens, ReplaceWith::Twice)
                        | (Replaces::Damage { .. }, ReplaceWith::Prevent)
                        | (Replaces::PutCountersOnYourPermanents, ReplaceWith::Twice)
                        | (Replaces::CardToGraveyard, ReplaceWith::Exile)
                        | (
                            Replaces::ThisDies | Replaces::Dies(_) | Replaces::ThisToGraveyard,
                            ReplaceWith::Exile
                                | ReplaceWith::ShuffleIntoLibrary
                                | ReplaceWith::RevealShuffleIntoLibrary
                        )
                        | (
                            Replaces::ThisEnters,
                            ReplaceWith::EntersTapped
                                | ReplaceWith::EntersWithCounters { .. }
                                | ReplaceWith::EntersWithCounted { .. }
                                // Its own: "you" is whoever it enters under the control of.
                                | ReplaceWith::EntersTappedUnless(_)
                                | ReplaceWith::EntersTappedUnlessYou(_)
                                | ReplaceWith::EntersOnlyIfYou(_)
                        )
                        | (
                            Replaces::OtherEnters(_),
                            ReplaceWith::EntersTapped
                                | ReplaceWith::EntersWithCounters { .. }
                                | ReplaceWith::EntersWithCounted { .. }
                        )
                );
                // Counted without a resolving spell or ability to supply X or a trigger.
                // (Its own X — "enters with X counters" — is the X it was cast with.)
                let own_x = *replaces == Replaces::ThisEnters && card_has_x;
                if let ReplaceWith::EntersWithCounted { amount, .. } = with
                    && !amount.stands_alone()
                    && !(own_x && *amount == Amount::X)
                {
                    p.push(at(
                        "counters counted from something a replacement can't know".into(),
                    ));
                }
                // Judged before the permanent enters, with no spell or ability resolving.
                if let ReplaceWith::EntersTappedUnless(Condition::Compare { amount, .. }) = with
                    && !amount.stands_alone()
                {
                    p.push(at(
                        "a condition counted from something a replacement can't know".into(),
                    ));
                }
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
        // CR 115.3: the slots of a target group.
        let n = mode.targets.len();
        for g in &mode.groups {
            let (first, end) = (g.first as usize, g.first as usize + g.count as usize);
            if g.count == 0 || g.min > g.count || end > n || (g.count == 1 && g.min == 1) {
                p(format!("mode {mi}: bad target group {g:?}"));
                continue;
            }
            if mode.targets[first..end]
                .iter()
                .any(|f| *f != mode.targets[first])
            {
                p(format!(
                    "mode {mi}: a target group's slots must have one filter"
                ));
            }
            if g.min < g.count && end != n {
                p(format!(
                    "mode {mi}: an \"up to\" target group must be the last targets"
                ));
            }
            if mode
                .groups
                .iter()
                .filter(|o| (o.first..o.first + o.count).contains(&g.first))
                .count()
                != 1
            {
                p(format!("mode {mi}: overlapping target groups"));
            }
        }
    }
}

/// The chosen parts of a cost.
fn check_cost(cost: &Cost, p: &mut dyn FnMut(String)) {
    for (what, choice) in [("sacrifice", &cost.sacrifice), ("discard", &cost.discard)] {
        if let Some(ch) = choice {
            if ch.count == 0 {
                p(format!("a cost to {what} nothing"));
            }
            if ch.what.is_empty() {
                p(format!("a cost to {what} needs at least one selector"));
            }
        }
    }
    // Cards in a hand are matched by card type and subtype only.
    if let Some(d) = &cost.discard {
        for s in &d.what {
            let asked = card_model::Selector {
                types: s.types.clone(),
                not_types: s.not_types.clone(),
                subtypes: s.subtypes.clone(),
                not_subtypes: s.not_subtypes.clone(),
                ..Default::default()
            };
            if asked != *s {
                p("a discard cost can only ask for card types and subtypes".into());
            }
        }
    }
}

/// A selector used where "another" has no meaning must not say it.
fn selector(_s: &card_model::Selector, _p: &mut dyn FnMut(String)) {}

fn check_effect(e: &Effect, mode: &Mode, has_x: bool, p: &mut dyn FnMut(String)) {
    let slot = |t: &TargetRef| match t {
        TargetRef::Target(i) => mode.targets.get(*i as usize),
        TargetRef::Players(_) | TargetRef::Object(_) => None,
    };
    #[allow(clippy::type_complexity)]
    fn check_amount(
        a: &Amount,
        has_x: bool,
        selector: &dyn Fn(&card_model::Selector, &mut dyn FnMut(String)),
        p: &mut dyn FnMut(String),
    ) {
        match a {
            Amount::X if !has_x => {
                p("Amount::X used but nothing defines X (no {X} in the cost, no SetX)".into());
            }
            Amount::Count(s) | Amount::TotalPower(s) | Amount::Greatest { of: s, .. } => {
                selector(s, p)
            }
            Amount::CardsIn { cards, .. } => cards.iter().for_each(|s| selector(s, p)),
            Amount::Calc(base, _) => check_amount(base, has_x, selector, p),
            Amount::IfAtLeast { test, .. } => check_amount(test, has_x, selector, p),
            Amount::Devotion(colors) if colors.is_empty() => {
                p("devotion to no color".into());
            }
            _ => {}
        }
    }
    let amount = |a: &Amount, p: &mut dyn FnMut(String)| check_amount(a, has_x, &selector, p);
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
        Effect::May(inner) => check_effect(inner, mode, has_x, p),
        Effect::Pay {
            who,
            cost,
            paid,
            unpaid,
        } => {
            match who {
                PlayerRef::Each | PlayerRef::EachOpponent => {
                    p("a cost paid on resolution is paid by one player".into())
                }
                PlayerRef::ControllerOf(i) => {
                    if mode.targets.get(*i as usize).is_none() {
                        p(format!("PlayerRef::ControllerOf({i}) has no target slot"));
                    }
                }
                other => player(other, p),
            }
            if *cost == Cost::default() {
                p("a cost of nothing".into());
            }
            if cost.discard_self || cost.exile_self {
                p("a cost paid on resolution can't use the card in hand or graveyard".into());
            }
            // `{T}` and "sacrifice this" are the ability's source: its controller's.
            if (cost.tap || cost.sacrifice_self) && *who != PlayerRef::You {
                p("only the ability's controller can tap or sacrifice its source".into());
            }
            check_cost(cost, p);
            check_effect(paid, mode, has_x, p);
            check_effect(unpaid, mode, has_x, p);
        }
        Effect::Together(v) => {
            for x in v {
                if matches!(
                    x,
                    Effect::Seq(_) | Effect::May(_) | Effect::Together(_) | Effect::SetX(_)
                ) {
                    p("Together holds single instructions only".into());
                }
                check_effect(x, mode, has_x, p);
            }
        }
        Effect::ReturnToBattlefield { what } => match what {
            ObjRef::Target(i)
                if matches!(
                    mode.targets.get(*i as usize),
                    Some(TargetFilter::CardInGraveyard { .. })
                ) => {}
            other => p(format!(
                "ReturnToBattlefield needs a card-in-graveyard target, got {other:?}"
            )),
        },
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
        Effect::DealDamage { to, amount: a, .. } => {
            match to {
                TargetRef::Players(r) => player(r, p),
                TargetRef::Object(r) => obj(r, p),
                TargetRef::Target(_) => match slot(to) {
                    None => p(format!("{to:?} has no target slot")),
                    Some(TargetFilter::Spell | TargetFilter::SpellMatching(_)) => {
                        p("can't deal damage to a spell".into())
                    }
                    Some(_) => {}
                },
            }
            amount(a, p);
        }
        Effect::Tap { what } | Effect::Untap { what } => obj(what, p),
        Effect::TapAll(s) | Effect::UntapAll(s) => selector(s, p),
        Effect::Discard { who, amount: a } => {
            player(who, p);
            amount(a, p);
        }
        Effect::DiscardHand { who } => player(who, p),
        Effect::Sacrifice {
            who,
            what,
            amount: a,
        } => {
            player(who, p);
            selector(what, p);
            amount(a, p);
        }
        Effect::SacrificeThis => {}
        Effect::AttachThis { to } => obj(to, p),
        Effect::ChooseCards {
            who, cards, count, ..
        } => {
            player(who, p);
            amount(count, p);
            cards.iter().for_each(|s| selector(s, p));
        }
        Effect::Scry { who, amount: a } | Effect::Surveil { who, amount: a } => {
            player(who, p);
            amount(a, p);
        }
        Effect::Dig {
            who, count, cards, ..
        } => {
            player(who, p);
            amount(count, p);
            cards.iter().for_each(|s| selector(s, p));
        }
        Effect::PutInto { what, .. } => obj(what, p),
        Effect::MoveAll { whose, cards, .. } => {
            player(whose, p);
            cards.iter().for_each(|s| selector(s, p));
        }
        Effect::Draw { who, amount: a }
        | Effect::GainLife { who, amount: a }
        | Effect::AddMana { who, amount: a, .. }
        | Effect::LoseLife { who, amount: a } => {
            player(who, p);
            amount(a, p);
        }
        Effect::Counter { what } => match slot(what) {
            Some(TargetFilter::Spell | TargetFilter::SpellMatching(_)) => {}
            other => p(format!("Counter needs a Spell target slot, got {other:?}")),
        },
        Effect::EachPlayer(_) => {}
        Effect::DamageEachPlayer { amount: a } => amount(a, p),
        Effect::Destroy { what } | Effect::ReturnToHand { what } | Effect::Apply { what, .. } => {
            obj(what, p)
        }
        Effect::Lasting { about, .. } => {
            if let Some(what) = about {
                obj(what, p);
            }
        }
        Effect::PutCountersAll { what, count, .. } => {
            selector(what, p);
            amount(count, p);
        }
        Effect::ThisSpellTo { to, .. } => {
            if matches!(
                to,
                card_model::CardDest::Battlefield { .. } | card_model::CardDest::Graveyard
            ) {
                p(
                    "a resolving spell goes to exile, a hand or a library, if not the graveyard"
                        .into(),
                );
            }
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

pub use card_model::color_identity;

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
