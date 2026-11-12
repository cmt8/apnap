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
    CardType, Condition, Cost, Effect, Layout, Mode, ObjRef, PlayerRef, ReplaceWith, Replaces,
    Supertype, TargetFilter, TargetRef, Trigger, Who,
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
            Effect::Seq(v) | Effect::Together(v) => v.iter().for_each(|x| walk(x, out)),
            Effect::May(x)
            | Effect::Note(x)
            | Effect::If { then: x, .. }
            | Effect::PlayerMay { effect: x, .. } => walk(x, out),
            Effect::Pay { paid, unpaid, .. }
            | Effect::IfDone {
                first: paid,
                then: unpaid,
            } => {
                walk(paid, out);
                walk(unpaid, out);
            }
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
    // "… plus a Food token", "… that many Angels instead" (D-073).
    for a in def.abilities.iter().chain(&def.granted) {
        if let AbilityDef::Replacement {
            with: ReplaceWith::AddTokens { tokens, .. } | ReplaceWith::ReplaceTokens(tokens),
            ..
        } = a
        {
            out.extend(tokens.iter().cloned());
        }
    }
    out
}

/// Semantic checks for one script. Returns every problem found.
pub fn validate(script: &CardScript, strictness: Strictness) -> Vec<String> {
    let c = &script.card;
    let mut p = Vec::new();
    if strictness == Strictness::Strict && !script.verified {
        p.push("unverified card in strict mode (CLAUDE.md invariant 8)".into());
    }
    if script.verified && script.tests.is_empty() {
        p.push("verified cards must name at least one test (spec §10.2)".into());
    }
    p.extend(validate_face(c, None));
    // A card with several faces: each face is checked as a card of its own (CR 712.6).
    match (c.layout, c.faces.len()) {
        (Layout::Normal, 0)
        | (Layout::Modal | Layout::Transform | Layout::Adventure | Layout::Omen, 1)
        | (Layout::Split, 2) => {}
        (layout, n) => p.push(format!("a {layout:?} card with {n} other face(s)")),
    }
    if c.face_of.is_some() {
        p.push("face_of is set by the catalog, never written".into());
    }
    // CR 709: the halves of a split card are instants or sorceries (a Room's halves,
    // with their unlocking rules, aren't compiled), and the card is the two combined.
    if c.layout == Layout::Split {
        let halves = c.faces.iter().all(|f| {
            f.card_types
                .iter()
                .all(|t| matches!(t, CardType::Instant | CardType::Sorcery))
        });
        if !halves || !c.abilities.is_empty() || !c.keywords.is_empty() || !c.granted.is_empty() {
            p.push("a split card is two instant or sorcery halves combined".into());
        }
    }
    // CR 716.2, 721.2: the alternative characteristics are an instant or sorcery with
    // that spell type, and the card itself a permanent card.
    if c.layout.alternative_spell() {
        let kind = if c.layout == Layout::Adventure {
            "Adventure"
        } else {
            "Omen"
        };
        let ok = c.faces.first().is_some_and(|f| {
            (f.has_type(CardType::Instant) || f.has_type(CardType::Sorcery))
                && f.creature_types.iter().any(|t| t == kind)
        });
        if !ok || c.has_type(CardType::Instant) || c.has_type(CardType::Sorcery) {
            p.push(format!(
                "an {kind} card is a permanent card with an {kind} spell"
            ));
        }
    }
    for (i, f) in c.faces.iter().enumerate() {
        if !f.faces.is_empty() || f.layout != Layout::Normal || f.face_of.is_some() {
            p.push(format!(
                "face {}: a face has no faces or layout of its own",
                i + 1
            ));
        }
        if f.token != c.token {
            p.push(format!("face {}: a token face of a card", i + 1));
        }
        for m in validate_face(f, Some(c.layout)) {
            p.push(format!("face {} ({}): {m}", i + 1, f.name));
        }
    }
    p
}

/// The checks of one face of a card; `back` is the layout of the card when it is a back
/// face.
fn validate_face(c: &CardDef, back: Option<Layout>) -> Vec<String> {
    let mut p: Vec<String> = Vec::new();
    if c.name.trim().is_empty() {
        p.push("empty name".into());
    }
    let creature = c.has_type(CardType::Creature);
    if creature != (c.power.is_some() && c.toughness.is_some()) {
        p.push("power/toughness must be given exactly for creatures".into());
    }
    let land = c.has_type(CardType::Land);
    if land && c.mana_cost.is_some() {
        p.push("lands have no mana cost (they are played, not cast)".into());
    }
    // CR 712.2: the back face of a nonmodal double-faced card has no mana cost (it isn't
    // cast with it up); every other face that isn't a land has one.
    let transformed = back == Some(Layout::Transform);
    if !land && !c.token && c.mana_cost.is_none() && !transformed {
        p.push("non-land, non-token cards need a mana cost".into());
    }
    if transformed && c.mana_cost.is_some() {
        p.push("the back face of a transforming card has no mana cost".into());
    }
    // CR 107.4e: spells can't be paid for with hybrid mana yet (D-066).
    if c.mana_cost.as_ref().is_some_and(|m| {
        m.symbols
            .iter()
            .any(|s| matches!(s, card_model::ManaSymbol::Hybrid(..)))
    }) {
        p.push("a hybrid mana cost (D-066: not paid for yet)".into());
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
    // A split card as a whole (CR 709.4) has no text of its own: its halves' spell
    // abilities are theirs.
    if instant_or_sorcery && spells != 1 && c.layout != Layout::Split {
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
    // X in the mana cost, or "pay X life" in the additional cost (CR 107.3a).
    let card_has_x = c.mana_cost.as_ref().is_some_and(|m| m.has_x())
        || c.abilities
            .iter()
            .any(|a| matches!(a, AbilityDef::AdditionalCost(x) if x.life_x));
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
    // CR 607.2d: "the chosen type / color" needs an ability of the card that chooses it.
    for (refers, choice) in [
        (
            text.contains("chosen: Some(Type)")
                || text.contains("chosen: Some(NotType)")
                || text.contains("AddChosenType"),
            "Choose(CreatureType)",
        ),
        (text.contains("chosen: Some(Color)"), "Choose(Color)"),
    ] {
        if refers && !text.contains(choice) {
            p.push(format!("refers to a choice no ability makes ({choice})"));
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
                    matches!(replaces, Replaces::Damage { .. })
                        && matches!(with, ReplaceWith::Prevent | ReplaceWith::Scale(_))
                }
                // "You may play an additional land this turn" (CR 611.2c: a rule).
                AbilityDef::PlayerRule { rule, .. } => {
                    !matches!(rule, card_model::PlayerRule::PlayThisFrom(_))
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
                | AbilityDef::AlternativeCost(_)
                | AbilityDef::Enchant(_)
                | AbilityDef::SelfCostReduction(_)
                | AbilityDef::FromZone { .. } => false,
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
                                    | E::RemoveTypes(_)
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
                | AbilityDef::CostReduction { .. }
                | AbilityDef::AdditionalTrigger { .. } => true,
                // "This spell can't be countered" is about the card itself.
                AbilityDef::CantBeCountered { spells } => spells.is_some(),
                // "This card" may be cast from a zone: only its own, printed ability.
                AbilityDef::PlayerRule { rule, .. } => {
                    !matches!(rule, card_model::PlayerRule::PlayThisFrom(_))
                }
            };
            if !fits {
                p.push(at("not an ability another object can be given".into()));
                continue;
            }
        }
        // CR 107.4e: hybrid mana is paid only in the costs of mana abilities so far (D-066).
        if !matches!(a, AbilityDef::Mana { .. }) && format!("{a:?}").contains("Hybrid") {
            p.push(at(
                "hybrid mana outside a mana ability's cost (D-066)".into()
            ));
        }
        // "Those cards" (D-069) are known only to the resolving effect that noted them: a
        // selector may ask about them only where that effect filters with it itself.
        let text = format!("{a:?}");
        let asked = text.matches("noted: Some(").count();
        let allowed: usize = match a {
            AbilityDef::Spell(b)
            | AbilityDef::Activated { body: b, .. }
            | AbilityDef::Triggered { body: b, .. } => {
                b.modes().iter().map(|m| noted_selectors(&m.effect)).sum()
            }
            _ => 0,
        };
        if asked != allowed {
            p.push(at(
                "\"those cards\" asked about outside the effect that knows them".into(),
            ));
        }
        let resolving = matches!(
            a,
            AbilityDef::Spell(_) | AbilityDef::Activated { .. } | AbilityDef::Triggered { .. }
        );
        if !resolving && (text.contains("Noted") || text.contains("Note(")) {
            p.push(at("\"those cards\" outside a resolving effect".into()));
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
                    // Who drew or discarded, who cast the spell, whose step it is.
                    Trigger::Draws { .. }
                    | Trigger::Discards(_)
                    | Trigger::Cast { .. }
                    | Trigger::BeginningOf { .. }
                    | Trigger::Attacks(_)
                    | Trigger::AttacksAlone(_)
                    | Trigger::BecomesBlocked(_)
                    | Trigger::Unblocked(_) => needs_player,
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
            AbilityDef::PlayerRule { only_if, .. } => (only_if.as_ref(), false),
            AbilityDef::AlternativeCost(alt) => (alt.only_if.as_ref(), false),
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
                if cost.life_x {
                    p.push(at("\"pay X life\" is only a spell's additional cost".into()));
                }
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
                then,
                ..
            } => {
                // D-066: `{T}` or a sacrifice (so that every activation uses something
                // up), and maybe mana, life, "sacrifice a [permanent]". Nothing from a
                // hand or graveyard, no X, no discards.
                if !(cost.tap || cost.sacrifice_self || cost.sacrifice.is_some()) {
                    p.push(at(
                        "a mana ability must tap or sacrifice something (D-066)".into()
                    ));
                }
                if cost.discard.is_some()
                    || cost.discard_self
                    || cost.exile_self
                    || cost.mana.as_ref().is_some_and(|m| m.has_x())
                {
                    p.push(at("a mana ability cost that isn't modelled (D-066)".into()));
                }
                if cost.sacrifice.as_ref().is_some_and(|ch| ch.count != 1) {
                    p.push(at(
                        "a mana ability that sacrifices more than one permanent (D-066)".into(),
                    ));
                }
                check_cost(cost, &mut |m| p.push(at(m)));
                if produce.count() == 0 || produce.options().is_empty() {
                    p.push(at("a mana ability that adds nothing".into()));
                }
                match produce {
                    card_model::ManaProduce::Fixed(v) if v.len() < 2 => {
                        p.push(at("fixed mana of one symbol is `One`".into()))
                    }
                    // Counted as it resolves: tapping must not change the count, and
                    // nothing it sacrifices may (the planner counts before, D-066).
                    card_model::ManaProduce::Counted { amount, .. } => {
                        let plain = match amount {
                            Amount::Count(s) => s.tapped.is_none(),
                            _ => false,
                        };
                        if !plain || cost.sacrifice_self || cost.sacrifice.is_some() {
                            p.push(at("counted mana must count permanents, with no sacrifice in the cost (D-066)".into()));
                        }
                    }
                    _ => {}
                }
                // CR 605.3b: the rest of a mana ability happens as it resolves. Only what
                // can't make the mana abilities used to pay a cost change is allowed
                // (D-066): damage to, life loss and life gain of its controller.
                if let Some(e) = then
                    && !mana_side_effect(e)
                {
                    p.push(at(format!(
                        "a mana ability may only also deal damage to, or change the life of, its controller (D-066): {e:?}"
                    )));
                }
                if spend.as_ref().is_some_and(|r| {
                    r.spells.is_empty() && r.abilities.is_empty() && r.sources.is_empty()
                }) {
                    p.push(at("mana that can be spent on nothing".into()));
                }
            }
            AbilityDef::AdditionalTrigger { .. } => {}
            AbilityDef::CantBeCountered { spells } => {
                if spells.is_none() && c.card_types.iter().all(|t| *t == CardType::Land) {
                    p.push(at("a land is never a spell (CR 305.1)".into()));
                }
            }
            AbilityDef::FromZone { zone, ability } => {
                let fits = *zone == ActivationZone::Graveyard
                    && !granted
                    && matches!(&**ability, AbilityDef::Static {
                        affected: card_model::Affected::All(sel),
                        effects,
                        ..
                    } if !sel.looks_at_pt()
                        && effects.iter().all(|e| !matches!(
                            e,
                            card_model::ContinuousEffect::SetPTBy { .. }
                        )));
                if !fits {
                    p.push(at(
                        "only a static ability about permanents works from a graveyard".into(),
                    ));
                }
            }
            AbilityDef::PlayerRule { who, rule, .. } => {
                use card_model::PlayerRule as R;
                match rule {
                    R::AdditionalLandPlays(0) => p.push(at("no additional land".into())),
                    R::PlayFrom { cards, .. } => {
                        // The cards are in the player's own zone: whose they are and what
                        // is on the battlefield isn't asked (`card_matches`).
                        if cards.is_empty()
                            || cards.iter().any(|s| {
                                s.controller != Who::Any
                                    || s.owner != Who::Any
                                    || s.other
                                    || s.tapped.is_some()
                                    || s.attacking.is_some()
                                    || s.blocking.is_some()
                            })
                        {
                            p.push(at(
                                "cards a rule lets a player play: card properties only".into()
                            ));
                        }
                    }
                    R::PlayThisFrom(zones) if zones.is_empty() || *who != Who::You => p.push(at(
                        "\"cast this card from …\" names zones, for its owner".into(),
                    )),
                    _ => {}
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
                if cost.sacrifice.is_none()
                    && cost.discard.is_none()
                    && cost.life == 0
                    && !cost.life_x
                    && cost.exile_from_hand.is_none()
                    && cost.return_to_hand.is_none()
                {
                    p.push(at("an additional cost of nothing".into()));
                }
                if c.has_type(CardType::Land) {
                    p.push(at("lands aren't cast: no additional cost".into()));
                }
                check_cost(cost, &mut |m| p.push(at(m)));
            }
            AbilityDef::AlternativeCost(alt) => {
                use card_model::AltKind;
                // CR 118.9: paid rather than the mana cost; it doesn't name the card in a
                // zone, and isn't `{T}` (D-072).
                let cost = &alt.cost;
                if cost.tap
                    || cost.sacrifice_self
                    || cost.discard_self
                    || cost.exile_self
                    || cost.life_x
                {
                    p.push(at(
                        "an alternative cost doesn't tap or move its own card".into()
                    ));
                }
                if c.has_type(CardType::Land) {
                    p.push(at("lands aren't cast: no alternative cost".into()));
                }
                let additional = c.abilities.iter().find_map(|a| match a {
                    AbilityDef::AdditionalCost(x) => Some(x),
                    _ => None,
                });
                match cost.with(additional.unwrap_or(&Cost::default())) {
                    None => p.push(at(
                        "an alternative cost that chooses what the additional cost chooses".into(),
                    )),
                    Some(all) => {
                        check_cost(&all, &mut |m| p.push(at(m)));
                        if all.discard.is_some() && all.exile_from_hand.is_some() {
                            p.push(at(
                                "a cost that both discards and exiles from the hand".into()
                            ));
                        }
                    }
                }
                match &alt.kind {
                    AltKind::Flashback | AltKind::Overload(_) if !instant_or_sorcery => p.push(at(
                        "flashback and overload are on instants and sorceries".into(),
                    )),
                    AltKind::Evoke if instant_or_sorcery => {
                        p.push(at("evoke is on permanent cards".into()))
                    }
                    _ => {}
                }
                if let AltKind::Overload(body) = &alt.kind {
                    if body.modes().iter().any(|m| !m.targets.is_empty()) {
                        p.push(at("an overloaded spell has no targets (CR 702.96b)".into()));
                    }
                    check_body(body, card_has_x, &mut |m| p.push(at(m)));
                }
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
                // A token enters as it is created (CR 111.2), with no object yet for its
                // own "this enters …" effect to belong to (D-073).
                if c.token && *replaces == Replaces::ThisEnters {
                    p.push(at(
                        "a token's own \"enters\" replacement effect isn't supported".into(),
                    ));
                }
                let ok = matches!(
                    (replaces, with),
                    (Replaces::CreateTokens, ReplaceWith::Twice)
                        | (
                            Replaces::Tokens { .. },
                            ReplaceWith::Scale(_)
                                | ReplaceWith::AddTokens { .. }
                                | ReplaceWith::ReplaceTokens(_)
                        )
                        | (
                            Replaces::Counters { .. }
                                | Replaces::Damage { .. }
                                | Replaces::GainLife(_),
                            ReplaceWith::Scale(_)
                        )
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
                                // "As this enters, choose …" (CR 614.12a).
                                | ReplaceWith::Choose(_)
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
/// What a mana ability may do besides adding mana (D-066).
pub(crate) fn mana_side_effect(e: &Effect) -> bool {
    use card_model::{DamageBy, PlayerRef};
    match e {
        Effect::Seq(v) => !v.is_empty() && v.iter().all(mana_side_effect),
        Effect::DealDamage {
            to: TargetRef::Players(PlayerRef::You),
            amount: Amount::Fixed(_),
            by: DamageBy::This,
        }
        | Effect::LoseLife {
            who: PlayerRef::You,
            amount: Amount::Fixed(_),
        }
        | Effect::GainLife {
            who: PlayerRef::You,
            amount: Amount::Fixed(_),
        } => true,
        _ => false,
    }
}

fn check_cost(cost: &Cost, p: &mut dyn FnMut(String)) {
    // CR 601.2g–h: a permanent sacrificed for mana is gone; the engine keeps enough for one
    // choice of permanents, not for two (D-072).
    if cost.sacrifice.is_some() && cost.return_to_hand.is_some() {
        p("a cost that both sacrifices and returns permanents".into());
    }
    if let Some(e) = &cost.exile_from_hand {
        for s in &e.what {
            let asked = card_model::Selector {
                types: s.types.clone(),
                not_types: s.not_types.clone(),
                subtypes: s.subtypes.clone(),
                not_subtypes: s.not_subtypes.clone(),
                colors: s.colors.clone(),
                not_colors: s.not_colors.clone(),
                other: s.other,
                ..Default::default()
            };
            if asked != *s {
                p("cards exiled from a hand are asked for types, subtypes and colors".into());
            }
        }
    }
    for (what, choice) in [
        ("sacrifice", &cost.sacrifice),
        ("discard", &cost.discard),
        ("exile", &cost.exile_from_hand),
        ("return", &cost.return_to_hand),
    ] {
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

/// Does this instruction define X for what follows ("…where X is", `SetX`)?
fn first_sets_x(e: &Effect) -> bool {
    match e {
        Effect::SetX(_) => true,
        Effect::Seq(v) => v.iter().any(first_sets_x),
        _ => false,
    }
}

/// The target slots whose power, toughness or mana value an effect reads.
fn amounts_of_targets(e: &Effect, out: &mut Vec<u8>, hands: &mut Vec<PlayerRef>) {
    // Walk the serialized form: amounts sit in many places of many effects.
    fn walk(v: &serde_json::Value, out: &mut Vec<u8>, hands: &mut Vec<PlayerRef>) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(serde_json::Value::Object(t)) = m.get("OfTarget")
                    && let Some(slot) = t.get("slot").and_then(serde_json::Value::as_u64)
                {
                    out.push(slot as u8);
                }
                if let Some(who) = m.get("HandOf")
                    && let Ok(who) = serde_json::from_value::<PlayerRef>(who.clone())
                {
                    hands.push(who);
                }
                m.values().for_each(|x| walk(x, out, hands));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out, hands)),
            _ => {}
        }
    }
    if let Ok(v) = serde_json::to_value(e) {
        walk(&v, out, hands);
    }
    out.sort_unstable();
    out.dedup();
}

/// A selector used where "another" has no meaning must not say it.
fn selector(s: &card_model::Selector, p: &mut dyn FnMut(String)) {
    // "Of the chosen type" is the only subtype asked for; "of the chosen color" the only color.
    match s.chosen {
        Some(card_model::OfChosen::Type) if !s.subtypes.is_empty() => {
            p("a selector of the chosen type that names subtypes too".into())
        }
        Some(card_model::OfChosen::Color) if !s.colors.is_empty() => {
            p("a selector of the chosen color that names colors too".into())
        }
        _ => {}
    }
}

/// How many selectors of a one-shot effect ask about "those cards" where the resolving
/// effect filters with them itself (D-069): what it destroys, taps, chooses, moves….
fn noted_selectors(e: &Effect) -> usize {
    let n = |s: &card_model::Selector| usize::from(s.noted.is_some());
    match e {
        Effect::Seq(v) | Effect::Together(v) => v.iter().map(noted_selectors).sum(),
        Effect::May(x)
        | Effect::Note(x)
        | Effect::If { then: x, .. }
        | Effect::PlayerMay { effect: x, .. } => noted_selectors(x),
        Effect::Pay { paid, unpaid, .. }
        | Effect::IfDone {
            first: paid,
            then: unpaid,
        } => noted_selectors(paid) + noted_selectors(unpaid),
        Effect::DestroyAll(s) | Effect::TapAll(s) | Effect::UntapAll(s) => n(s),
        Effect::DamageEach { each: s, .. }
        | Effect::ApplyAll { affected: s, .. }
        | Effect::PutCountersAll { what: s, .. }
        | Effect::Sacrifice { what: s, .. } => n(s),
        Effect::ChooseCards { cards, .. }
        | Effect::Dig { cards, .. }
        | Effect::MoveAll { cards, .. } => cards.iter().map(n).sum(),
        _ => 0,
    }
}

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
    // "Its controller", "its power": of a target that is a spell or a permanent.
    let of_object =
        |what: &str, i: u8, p: &mut dyn FnMut(String)| match mode.targets.get(i as usize) {
            Some(TargetFilter::Player | TargetFilter::Opponent) => {
                p(format!("{what}({i}) refers to a player target"))
            }
            Some(TargetFilter::CardInGraveyard { .. }) => p(format!(
                "{what}({i}): a card in a graveyard has no controller"
            )),
            Some(_) => {}
            None => p(format!("{what}({i}) has no target slot")),
        };
    let player = |r: &PlayerRef, p: &mut dyn FnMut(String)| match r {
        PlayerRef::Target(i) => match mode.targets.get(*i as usize) {
            Some(f) if f.allows_players() => {}
            Some(f) => p(format!("PlayerRef::Target({i}) refers to a {f:?} target")),
            None => p(format!("PlayerRef::Target({i}) has no target slot")),
        },
        PlayerRef::ControllerOf(i) => of_object("PlayerRef::ControllerOf", *i, p),
        PlayerRef::OwnerOf(i) => of_object("PlayerRef::OwnerOf", *i, p),
        _ => {}
    };
    // Amounts read off a target, or off another player's hand.
    let (mut targets_read, mut hands) = (Vec::new(), Vec::new());
    amounts_of_targets(e, &mut targets_read, &mut hands);
    for i in targets_read {
        of_object("Amount::OfTarget", i, p);
    }
    for who in hands {
        match who {
            PlayerRef::Each
            | PlayerRef::EachOpponent
            | PlayerRef::NotedControllers
            | PlayerRef::NotNotedControllers => p("Amount::HandOf counts one player's hand".into()),
            other => player(&other, p),
        }
    }
    let stay = |d: &card_model::CardDest, p: &mut dyn FnMut(String)| {
        if *d == card_model::CardDest::Stay {
            p("cards left where they are, outside a noted search".into());
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
        Effect::Choose(_) => {}
        Effect::May(inner) => check_effect(inner, mode, has_x, p),
        Effect::IfDone { first, then } => {
            check_effect(first, mode, has_x, p);
            check_effect(then, mode, has_x || first_sets_x(first), p);
        }
        Effect::PlayerMay { who, effect } => {
            match who {
                PlayerRef::Each
                | PlayerRef::EachOpponent
                | PlayerRef::NotedControllers
                | PlayerRef::NotNotedControllers => p("one player decides a \"may\"".into()),
                // That is `May`.
                PlayerRef::You => p("PlayerMay by you: write May".into()),
                other => player(other, p),
            }
            check_effect(effect, mode, has_x, p);
        }
        Effect::Note(inner) => {
            let mut inner = (**inner).clone();
            match &mut inner {
                Effect::Note(_) => p("a noted instruction inside a noted instruction".into()),
                Effect::Seq(_)
                | Effect::May(_)
                | Effect::PlayerMay { .. }
                | Effect::If { .. }
                | Effect::IfDone { .. }
                | Effect::Pay { .. } => p("only a single instruction is noted".into()),
                // A search that leaves what it finds in the library, for what comes next.
                Effect::ChooseCards { from, to, .. } if *to == card_model::CardDest::Stay => {
                    if *from != card_model::CardZone::Library {
                        p("only a search leaves the cards it finds".into());
                    }
                    *to = card_model::CardDest::Hand;
                }
                // What it takes is told from the rest by where it goes; taken cards left
                // in the library aren't moved at all.
                Effect::Dig { to, rest, .. } => {
                    use card_model::CardDest as D;
                    let library = |d: &D| matches!(d, D::LibraryTop | D::LibraryBottom);
                    if library(to) || to == rest {
                        p(
                            "a noted Dig must take its cards out of the library, apart from \
                           the rest"
                                .into(),
                        )
                    }
                }
                _ => {}
            }
            check_effect(&inner, mode, has_x, p)
        }
        Effect::If { test, then } => {
            if let card_model::Condition::Compare { amount: a, .. } = test {
                amount(a, p);
            }
            check_effect(then, mode, has_x, p)
        }
        Effect::Pay {
            who,
            cost,
            paid,
            unpaid,
        } => {
            match who {
                PlayerRef::Each
                | PlayerRef::EachOpponent
                | PlayerRef::NotedControllers
                | PlayerRef::NotNotedControllers => {
                    p("a cost paid on resolution is paid by one player".into())
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
                    Effect::Seq(_)
                        | Effect::May(_)
                        | Effect::PlayerMay { .. }
                        | Effect::IfDone { .. }
                        | Effect::Together(_)
                        | Effect::SetX(_)
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
        Effect::DealDamage { to, amount: a, by } => {
            if let card_model::DamageBy::Target(i) = by {
                match mode.targets.get(*i as usize) {
                    Some(TargetFilter::Creature | TargetFilter::Permanent(_)) => {}
                    other => p(format!(
                        "DamageBy::Target({i}) needs a permanent, got {other:?}"
                    )),
                }
            }
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
        Effect::Tap { what } | Effect::Untap { what } | Effect::Transform { what } => obj(what, p),
        Effect::TapAll(s) | Effect::UntapAll(s) => selector(s, p),
        Effect::Proliferate => {}
        Effect::UntapChosen { what, count } => {
            selector(what, p);
            amount(count, p);
        }
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
        Effect::SacrificeThis | Effect::ControllerSacrificesThis => {}
        Effect::AttachThis { to } => {
            if *to == ObjRef::Noted {
                p("attach to one of those cards: which?".into());
            }
            obj(to, p)
        }
        Effect::ChooseCards {
            who,
            cards,
            count,
            to,
            ..
        } => {
            stay(to, p);
            player(who, p);
            amount(count, p);
            cards.iter().for_each(|s| selector(s, p));
        }
        Effect::Scry { who, amount: a } | Effect::Surveil { who, amount: a } => {
            player(who, p);
            amount(a, p);
        }
        Effect::Dig {
            who,
            count,
            cards,
            to,
            rest,
            ..
        } => {
            stay(to, p);
            stay(rest, p);
            player(who, p);
            amount(count, p);
            cards.iter().for_each(|s| selector(s, p));
        }
        Effect::PutInto {
            what,
            to,
            controller,
            ..
        } => {
            stay(to, p);
            if let Some(c) = controller {
                if !matches!(to, card_model::CardDest::Battlefield { .. }) {
                    p("under whose control, off the battlefield".into());
                }
                if matches!(
                    c,
                    PlayerRef::Each
                        | PlayerRef::EachOpponent
                        | PlayerRef::NotedControllers
                        | PlayerRef::NotNotedControllers
                ) {
                    p("under the control of one player".into());
                }
                player(c, p);
            }
            obj(what, p)
        }
        Effect::MoveAll {
            whose, cards, to, ..
        } => {
            stay(to, p);
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
        Effect::AddManaOf { who, of, amount: a } => {
            player(who, p);
            amount(a, p);
            if of.len() < 2 {
                p("AddManaOf with no choice is AddMana".into());
            }
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
                if *what == ObjRef::Noted {
                    p("a lasting effect about those cards: not yet".into());
                }
                obj(what, p);
            }
        }
        Effect::PutCountersAll { what, count, .. } => {
            selector(what, p);
            amount(count, p);
        }
        Effect::ThisSpellTo { to, .. } => {
            stay(to, p);
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
        if d.face_of.is_some() {
            p.push(format!("{} is a face of another card, not a card", d.name));
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
