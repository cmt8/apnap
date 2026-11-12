//! Plain-English rules text generated from a card's ability definitions. The UI shows it for
//! every card, so what a player reads is exactly what the engine does (for real cards the
//! UI can show Scryfall's Oracle text next to it). Every match is exhaustive: adding
//! vocabulary without describing it is a compile error.

use crate::{
    AbilityDef, ActivationZone, Affected, Amount, AsEnters, Body, Calc, CardDef, CardDest,
    CardType, CardZone, ChoiceOf, Cmp, Color, CombatRule, Condition, ContinuousEffect, CopyExcept,
    CopyWhat, Cost, CounterKind, DamageBy, DamagePer, DamageTo, DelayWindow, EachPlayerAction,
    Effect, Keyword, ManaProduce, Measure, Mode, ObjFilter, ObjRef, OfChosen, PlayerRef,
    ReplaceWith, Replaces, Selector, Take, TargetFilter, TargetRef, Timing, Trigger, TriggerStep,
    Who,
};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub fn keyword(k: Keyword) -> &'static str {
    match k {
        Keyword::Deathtouch => "Deathtouch",
        Keyword::DoubleStrike => "Double strike",
        Keyword::FirstStrike => "First strike",
        Keyword::Flash => "Flash",
        Keyword::Flying => "Flying",
        Keyword::Haste => "Haste",
        Keyword::Lifelink => "Lifelink",
        Keyword::Menace => "Menace",
        Keyword::Reach => "Reach",
        Keyword::Trample => "Trample",
        Keyword::Vigilance => "Vigilance",
        Keyword::Defender => "Defender",
        Keyword::Fear => "Fear",
        Keyword::Hexproof => "Hexproof",
        Keyword::Indestructible => "Indestructible",
        Keyword::Intimidate => "Intimidate",
        Keyword::Shroud => "Shroud",
        Keyword::Shadow => "Shadow",
        Keyword::Horsemanship => "Horsemanship",
        Keyword::Flanking => "Flanking",
        Keyword::Changeling => "Changeling",
        Keyword::Convoke => "Convoke",
        Keyword::Improvise => "Improvise",
        Keyword::Landwalk(crate::BasicLandType::Plains) => "Plainswalk",
        Keyword::Landwalk(crate::BasicLandType::Island) => "Islandwalk",
        Keyword::Landwalk(crate::BasicLandType::Swamp) => "Swampwalk",
        Keyword::Landwalk(crate::BasicLandType::Mountain) => "Mountainwalk",
        Keyword::Landwalk(crate::BasicLandType::Forest) => "Forestwalk",
    }
}

fn card_type(t: CardType) -> &'static str {
    match t {
        CardType::Artifact => "artifact",
        CardType::Battle => "battle",
        CardType::Creature => "creature",
        CardType::Enchantment => "enchantment",
        CardType::Instant => "instant",
        CardType::Kindred => "kindred",
        CardType::Land => "land",
        CardType::Planeswalker => "planeswalker",
        CardType::Sorcery => "sorcery",
    }
}

fn color(c: Color) -> &'static str {
    match c {
        Color::White => "white",
        Color::Blue => "blue",
        Color::Black => "black",
        Color::Red => "red",
        Color::Green => "green",
    }
}

fn join(items: Vec<String>, last: &str) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        n => format!("{} {last} {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

fn types(ts: &[CardType], last: &str) -> String {
    join(ts.iter().map(|t| card_type(*t).to_string()).collect(), last)
}

/// "artifacts or enchantments": each type in the plural, not just the last.
fn types_plural(ts: &[CardType], last: &str) -> String {
    join(ts.iter().map(|t| pluralize(card_type(*t))).collect(), last)
}

/// Marks the start and end of the text of a "target …" word, with the index of the
/// target slot between the first two marks: `TARGET 0 TARGET target creature TARGET_END`.
/// [`resolve_targets`] says "that creature" for a later mention of the same slot.
const TARGET: char = '\u{7}';
const TARGET_END: char = '\u{8}';
/// Brackets "how many|key|amount" of a token an effect creates until [`expand_tokens`]
/// describes it from its definition.
const TOKEN: char = '\u{9}';

/// The plural of an English noun as Oracle text has it, for the last word of `s`:
/// "Elf" is "Elves", "Plains" are "Plains", "Eldrazi" are "Eldrazi" (CR 205.3m lists the
/// creature types).
fn pluralize(s: &str) -> String {
    let (head, word) = match s.rfind(' ') {
        Some(i) => s.split_at(i + 1),
        None => ("", s),
    };
    let plural = match word {
        // The same in the plural.
        "Plains" | "Equipment" | "Aetherborn" | "Aurochs" | "Bison" | "Borg" | "Caribou"
        | "Djinn" | "Efreet" | "Eldrazi" | "Fish" | "Hirogen" | "Jellyfish" | "Kavu"
        | "Kithkin" | "Kor" | "Merfolk" | "Metathran" | "Moonfolk" | "Myr" | "Nephilim"
        | "Ocampa" | "Sheep" | "Slith" | "Soltari" | "Starfish" | "Thalakos" | "Treefolk"
        | "Trill" | "Vedalken" | "Vorta" | "Xindi" | "Zubera" | "Dauthi" | "Lammasu" | "Gorn" => {
            word.to_string()
        }
        "Elf" => "Elves".to_string(),
        "Dwarf" => "Dwarves".to_string(),
        "Wolf" => "Wolves".to_string(),
        "Werewolf" => "Werewolves".to_string(),
        "Mouse" => "Mice".to_string(),
        "Fungus" => "Fungi".to_string(),
        "Cyclops" => "Cyclopes".to_string(),
        "Homunculus" => "Homunculi".to_string(),
        "Pegasus" => "Pegasi".to_string(),
        "Ox" => "Oxen".to_string(),
        "Child" => "Children".to_string(),
        w if w.ends_with(['s', 'x', 'z']) || w.ends_with("ch") || w.ends_with("sh") => {
            format!("{w}es")
        }
        w if w.ends_with('y')
            && w.len() > 1
            && !w[..w.len() - 1].ends_with(['a', 'e', 'i', 'o', 'u']) =>
        {
            format!("{}ies", &w[..w.len() - 1])
        }
        w => format!("{w}s"),
    };
    format!("{head}{plural}")
}

/// The number in running text: Oracle spells out small numbers of things ("two cards")
/// and keeps digits for "+1/+1", "{2}", life, damage and X.
fn num(n: u32) -> String {
    match n {
        1 => "one".to_string(),
        2 => "two".to_string(),
        3 => "three".to_string(),
        4 => "four".to_string(),
        5 => "five".to_string(),
        6 => "six".to_string(),
        7 => "seven".to_string(),
        8 => "eight".to_string(),
        9 => "nine".to_string(),
        10 => "ten".to_string(),
        n => n.to_string(),
    }
}

/// A Roman numeral (CR 715.2a: I represents 1, II represents 2, …).
pub fn roman(mut n: u32) -> String {
    const DIGITS: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, digits) in DIGITS {
        while n >= value {
            out.push_str(digits);
            n -= value;
        }
    }
    out
}

/// "second", "third" …
fn ordinal(n: u32) -> String {
    match n {
        1 => "first".to_string(),
        2 => "second".to_string(),
        3 => "third".to_string(),
        4 => "fourth".to_string(),
        5 => "fifth".to_string(),
        6 => "sixth".to_string(),
        7 => "seventh".to_string(),
        8 => "eighth".to_string(),
        9 => "ninth".to_string(),
        10 => "tenth".to_string(),
        n => format!("{n}th"),
    }
}

/// How many of a countable thing an amount is: "a card", "two cards", "X cards", "that
/// many cards", "a number of cards equal to its power".
fn quantity(a: &Amount, one: &str, many: &str) -> String {
    match a {
        Amount::Fixed(1) => with_article(one),
        Amount::Fixed(n) => format!("{} {many}", num(*n)),
        Amount::X => format!("X {many}"),
        Amount::TriggeringAmount => format!("that many {many}"),
        a => format!("a number of {many} equal to {}", amount(a)),
    }
}

/// How much of an uncountable thing (life, damage): "3 life", "X life", "that much life",
/// "life equal to its power".
fn amount_of(a: &Amount, thing: &str) -> String {
    match a {
        Amount::Fixed(_) | Amount::X => format!("{} {thing}", amount(a)),
        Amount::TriggeringAmount => format!("that much {thing}"),
        a => format!("{thing} equal to {}", amount(a)),
    }
}

fn amount(a: &Amount) -> String {
    match a {
        Amount::Fixed(n) => n.to_string(),
        Amount::X => "X".into(),
        Amount::SourcePower => "its power".into(),
        // (Permanents are counted, so with nobody named: "on the battlefield".)
        Amount::Count(s) if s.controller == Who::Any && s.owner == Who::Any => {
            format!("the number of {} on the battlefield", selector(s, true))
        }
        Amount::Count(s) => format!("the number of {}", selector(s, true)),
        Amount::TotalPower(s) => format!("the total power of {}", selector(s, true)),
        Amount::TriggeringPower => "its power".into(),
        Amount::TriggeringToughness => "its toughness".into(),
        Amount::TriggeringAmount => "that much".into(),
        Amount::SacrificedPower => "the sacrificed creature's power".into(),
        Amount::SourceToughness => "its toughness".into(),
        Amount::CardsIn { zone, cards } => {
            let what = card_list(cards, true);
            // "…you own" in a hand, graveyard or library is "your".
            let yours = !cards.is_empty() && cards.iter().all(|s| s.controller == Who::You);
            let place = match (zone, yours) {
                (CardZone::Graveyard, true) => "your graveyard",
                (CardZone::Graveyard, false) => "all graveyards",
                (CardZone::Hand, true) => "your hand",
                (CardZone::Hand, false) => "all hands",
                (CardZone::Library, true) => "your library",
                (CardZone::Library, false) => "all libraries",
                (CardZone::Battlefield, _) => "the battlefield",
            };
            format!("the number of {} in {place}", what.replace(" you own", ""))
        }
        Amount::Calc(base, op) => {
            let v = amount(base);
            match op {
                Calc::Plus(n) => format!("{v} plus {n}"),
                Calc::Minus(n) => format!("{v} minus {n}"),
                Calc::From(n) => format!("{n} minus {v}"),
                Calc::Times(2) => format!("twice {v}"),
                Calc::Times(n) => format!("{n} times {v}"),
                Calc::HalfUp => format!("half {v}, rounded up"),
                Calc::HalfDown => format!("half {v}, rounded down"),
                Calc::AtMost(n) => format!("{v} (at most {n})"),
                Calc::AtLeast(n) => format!("{v} (at least {n})"),
            }
        }
        Amount::IfAtLeast {
            test,
            at_least,
            then,
            otherwise,
        } => format!(
            "{then} if {} is {at_least} or more, otherwise {otherwise}",
            amount(test)
        ),
        Amount::Domain => "the number of basic land types among lands you control".into(),
        Amount::Devotion(colors) => format!(
            "your devotion to {}",
            join(
                colors.iter().map(|c| color(*c).to_string()).collect(),
                "and"
            )
        ),
        Amount::Greatest { of, what } => format!(
            "the greatest {} among {}",
            match what {
                Measure::Power => "power",
                Measure::Toughness => "toughness",
                Measure::ManaValue => "mana value",
            },
            selector(of, true)
        ),
        Amount::HandSize => "the number of cards in your hand".into(),
        Amount::LifeTotal => "your life total".into(),
        Amount::Players(who) => match who {
            Who::Opponent => "the number of opponents you have".into(),
            Who::You => "1".into(),
            Who::Any => "the number of players".into(),
        },
        Amount::LifeAmong { players, highest } => format!(
            "the {} life total among {}",
            if *highest { "highest" } else { "lowest" },
            match players {
                Who::Opponent => "your opponents",
                Who::You => "you",
                Who::Any => "all players",
            }
        ),
        Amount::HandOf(who) => {
            let who = match who {
                PlayerRef::TriggeringPlayer => "that player",
                PlayerRef::Target(_) => "target player",
                PlayerRef::ControllerOf(_) | PlayerRef::TriggeringController => "its controller",
                PlayerRef::OwnerOf(_) => "its owner",
                PlayerRef::You => "you",
                PlayerRef::Each
                | PlayerRef::EachOpponent
                | PlayerRef::NotedControllers
                | PlayerRef::NotNotedControllers => "those players",
            };
            format!("the number of cards in {who}'s hand")
        }
        Amount::OfTarget { what, .. } => match what {
            Measure::Power => "its power",
            Measure::Toughness => "its toughness",
            Measure::ManaValue => "its mana value",
        }
        .into(),
        Amount::Noted(cards) if cards.is_empty() => "the number of those cards".into(),
        Amount::Noted(cards) => format!(
            "the number of those cards that are {}",
            join(
                cards
                    .iter()
                    .map(|s| with_article(&card_list(core::slice::from_ref(s), false)))
                    .collect(),
                "or"
            )
        ),
        Amount::NotedTotal(what) => format!(
            "the total {} of those cards",
            match what {
                Measure::Power => "power",
                Measure::Toughness => "toughness",
                Measure::ManaValue => "mana value",
            }
        ),
        Amount::LifeLostThisWay => "the life lost this way".into(),
        Amount::CountersOnThis(k) => {
            format!("the number of {} counters on this permanent", counter(*k))
        }
    }
}

fn target_filter(f: &TargetFilter) -> String {
    let text = target_filter_words(f);
    // "Another target Elf card", not "target another Elf card".
    match text.strip_prefix("target another ") {
        Some(rest) => format!("another target {rest}"),
        None => text,
    }
}

/// "target creatures", "other target artifacts or enchantments", "targets": what a group
/// of targets of one "target" word may be. For "one to two targets" (the first must be
/// chosen, the second may be) it is "targets that are creatures": the word "targets" says
/// they are different ones (CR 115.3), which "target creatures" would not. (card-db's
/// `oracle_words_missing` accepts the word "targets" as "another target": this wording is
/// shaped for that guard, and pinned by its test.)
fn target_filter_plural(f: &TargetFilter, plain: bool) -> String {
    let target = |what: String| {
        if plain {
            format!("target {what}")
        } else {
            format!("targets that are {what}")
        }
    };
    let text = match f {
        TargetFilter::AnyTarget => return "targets".to_string(),
        TargetFilter::Creature => target("creatures".to_string()),
        TargetFilter::Player => "target players".to_string(),
        TargetFilter::Opponent => "target opponents".to_string(),
        TargetFilter::Spell => target("spells".to_string()),
        TargetFilter::OnStack { abilities, .. } => target(
            if *abilities {
                "spells or abilities"
            } else {
                "spells"
            }
            .to_string(),
        ),
        TargetFilter::SpellMatching(s) => target(format!("{} spells", spell_kind(s))),
        TargetFilter::AbilityOnStack {
            activated,
            triggered,
            yours,
        } => target(format!(
            "{} abilities{}",
            ability_kind(*activated, *triggered),
            if *yours { " you control" } else { "" }
        )),
        TargetFilter::Permanent(selectors) => {
            let parts: Vec<String> = selectors.iter().map(|s| selector(s, true)).collect();
            target(join(parts, "or"))
        }
        TargetFilter::CardInGraveyard { cards, whose } => target(format!(
            "{} from {}",
            card_list(core::slice::from_ref(cards), true),
            match whose {
                Who::Any => "a graveyard",
                Who::You => "your graveyard",
                Who::Opponent => "an opponent's graveyard",
            }
        )),
    };
    match text.strip_prefix("target other ") {
        Some(rest) => format!("other target {rest}"),
        None => text,
    }
}

/// `text` with its first target mark (and the words in it) replaced by `with`.
fn replace_first_target(text: &str, with: &str) -> Option<String> {
    let start = text.find(TARGET)?;
    let end = text[start..].find(TARGET_END)? + start;
    Some(format!(
        "{}{with}{}",
        &text[..start],
        &text[end + TARGET_END.len_utf8()..]
    ))
}

/// The player an instruction makes do something, if it is about one.
fn acting_player(e: &Effect) -> Option<PlayerRef> {
    match e {
        Effect::Draw { who, .. }
        | Effect::Mill { who, .. }
        | Effect::Discard { who, .. }
        | Effect::DiscardHand { who }
        | Effect::GainLife { who, .. }
        | Effect::LoseLife { who, .. }
        | Effect::Scry { who, .. }
        | Effect::Surveil { who, .. }
        | Effect::Sacrifice { who, .. }
        | Effect::CreateToken { who, .. } => Some(*who),
        // The sentence that starts it.
        Effect::Note(inner) | Effect::IfDone { first: inner, .. } => acting_player(inner),
        Effect::Seq(v) => v.first().and_then(acting_player),
        _ => None,
    }
}

/// "draws" -> "draw", "discards" -> "discard", "scries" -> "scry", "searches" -> "search".
fn base_verb(third: &str) -> String {
    if third.ends_with("ches") || third.ends_with("shes") || third.ends_with("sses") {
        third[..third.len() - 2].to_string()
    } else if let Some(stem) = third.strip_suffix("ies") {
        format!("{stem}y")
    } else {
        third.strip_suffix('s').unwrap_or(third).to_string()
    }
}

fn target_filter_words(f: &TargetFilter) -> String {
    match f {
        TargetFilter::AnyTarget => "any target".into(),
        TargetFilter::Creature => "target creature".into(),
        TargetFilter::Player => "target player".into(),
        TargetFilter::Opponent => "target opponent".into(),
        TargetFilter::Spell => "target spell".into(),
        TargetFilter::OnStack {
            abilities,
            single_target,
        } => format!(
            "target spell{}{}",
            if *abilities { " or ability" } else { "" },
            if *single_target {
                " with a single target"
            } else {
                ""
            }
        ),
        TargetFilter::SpellMatching(s) => {
            format!(
                "target {} spell{}{}",
                spell_kind(s),
                of_chosen(s, false),
                if s.controller == Who::You {
                    " you control"
                } else {
                    ""
                }
            )
        }
        TargetFilter::AbilityOnStack {
            activated,
            triggered,
            yours,
        } => format!(
            "target {} ability{}",
            ability_kind(*activated, *triggered),
            if *yours { " you control" } else { "" }
        ),
        TargetFilter::Permanent(selectors) => {
            let parts: Vec<String> = selectors.iter().map(|s| selector(s, false)).collect();
            format!("target {}", join(parts, "or"))
        }
        TargetFilter::CardInGraveyard { cards, whose } => format!(
            "target {} from {}",
            card_list(core::slice::from_ref(cards), false),
            match whose {
                Who::Any => "a graveyard",
                Who::You => "your graveyard",
                Who::Opponent => "an opponent's graveyard",
            }
        ),
    }
}

/// "an opponent or a permanent an opponent controls".
fn damaged(to: &DamageTo) -> String {
    match to {
        DamageTo::Anything => "a permanent or player".to_string(),
        DamageTo::Player(Who::Any) => "a player".to_string(),
        DamageTo::Player(Who::You) => "you".to_string(),
        DamageTo::Player(Who::Opponent) => "an opponent".to_string(),
        DamageTo::Permanent(ObjFilter::Matching(s)) => with_article(&selector(s, false)),
        DamageTo::Permanent(f) => obj_filter(f),
        DamageTo::Either { player, permanent } => format!(
            "{} or {}",
            damaged(&DamageTo::Player(*player)),
            damaged(&DamageTo::Permanent(permanent.clone()))
        ),
    }
}

/// "If an effect would create one or more creature tokens under your control".
fn tokens_would(token: &Selector, effect_only: bool) -> String {
    let kind = spell_kind(token);
    let tokens = if kind.is_empty() {
        "tokens".to_string()
    } else {
        format!("{kind} tokens")
    };
    let whose = match token.controller {
        Who::Any => "",
        Who::You => " under your control",
        Who::Opponent => " under an opponent's control",
    };
    if effect_only {
        format!("If an effect would create one or more {tokens}{whose}")
    } else {
        format!("If one or more {tokens} would be created{whose}")
    }
}

/// "Clue, Food and Treasure": tokens named by their definitions (filled in by
/// `expand_tokens`).
fn token_names(keys: &[String]) -> String {
    join(
        keys.iter()
            .map(|k| format!("{TOKEN}nm|{k}{TOKEN}"))
            .collect(),
        "and",
    )
}

/// "twice that many", "that many plus 1", "half that much, rounded down".
fn scaled(c: Calc, v: &str) -> String {
    match c {
        Calc::Plus(n) => format!("{v} plus {n}"),
        Calc::Minus(n) => format!("{v} minus {n}"),
        Calc::From(n) => format!("{n} minus {v}"),
        Calc::Times(2) => format!("twice {v}"),
        Calc::Times(3) => format!("three times {v}"),
        Calc::Times(n) => format!("{n} times {v}"),
        Calc::HalfUp => format!("half {v}, rounded up,"),
        Calc::HalfDown => format!("half {v}, rounded down,"),
        Calc::AtMost(n) => format!("{v} (at most {n})"),
        Calc::AtLeast(n) => format!("{v} (at least {n})"),
    }
}

fn with_article(s: &str) -> String {
    if s.starts_with("another ") {
        s.to_string()
    } else if takes_an(s) {
        format!("an {s}")
    } else {
        format!("a {s}")
    }
}

/// "a creature type", "a color".
fn choice_of(c: ChoiceOf) -> &'static str {
    match c {
        ChoiceOf::CreatureType => "a creature type",
        ChoiceOf::Color => "a color",
    }
}

/// " of the chosen type", " that aren't of the chosen type", " of the chosen color": what a
/// selector says about a choice, after its noun (empty if it says nothing).
fn of_chosen(s: &Selector, plural: bool) -> &'static str {
    match s.chosen {
        None => "",
        Some(OfChosen::Type) => " of the chosen type",
        Some(OfChosen::NotType) if plural => " that aren't of the chosen type",
        Some(OfChosen::NotType) => " that isn't of the chosen type",
        Some(OfChosen::Color) => " of the chosen color",
    }
}

/// Does the first word of `s` start with a vowel sound ("an artifact", "an Aura", "an
/// Elf", "an 8/8", "a unicorn", "a one-shot")?
fn takes_an(s: &str) -> bool {
    let word: String = s
        .chars()
        .take_while(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let mut chars = word.chars();
    match chars.next() {
        Some('a' | 'e' | 'i') => !(word.starts_with("eu") || word.starts_with("ew")),
        Some('o') => !(word.starts_with("one") || word.starts_with("once")),
        Some('u') => {
            !(word.starts_with("uni")
                || word.starts_with("use")
                || word.starts_with("usu")
                || word.starts_with("uti"))
        }
        Some('h') => {
            word.starts_with("hour") || word.starts_with("honor") || word.starts_with("heir")
        }
        Some('x') => chars.next().is_none_or(|c| !c.is_alphabetic()),
        Some('8') => true,
        Some('1') => word.starts_with("11") || word.starts_with("18"),
        _ => false,
    }
}

/// "creature", "noncreature", "instant or sorcery", "blue Wizard".
fn spell_kind(s: &Selector) -> String {
    let mut words: Vec<String> = Vec::new();
    words.extend(
        s.not_supertypes
            .iter()
            .map(|t| format!("non{}", format!("{t:?}").to_lowercase())),
    );
    words.extend(s.supertypes.iter().map(|t| format!("{t:?}").to_lowercase()));
    // Each list is "at least one of".
    if !s.colors.is_empty() {
        words.push(join(
            s.colors.iter().map(|c| color(*c).to_string()).collect(),
            "or",
        ));
    }
    if s.not_colors.len() == 5 {
        words.push("colorless".to_string());
    } else {
        words.extend(s.not_colors.iter().map(|c| format!("non{}", color(*c))));
    }
    // "Permanent spell" (CR 110.4b): neither instant nor sorcery.
    let permanent = s.types.is_empty()
        && s.not_types.len() == 2
        && s.not_types.contains(&CardType::Instant)
        && s.not_types.contains(&CardType::Sorcery);
    if !permanent {
        words.extend(s.not_types.iter().map(|t| format!("non{}", card_type(*t))));
    }
    let types: Vec<String> = s.types.iter().map(|t| card_type(*t).to_string()).collect();
    if !s.subtypes.is_empty() {
        words.push(join(s.subtypes.clone(), "or"));
    }
    if !types.is_empty() {
        words.push(join(types, "or"));
    }
    if permanent {
        words.push("permanent".to_string());
    }
    words.join(" ")
}

fn target(mode: &Mode, r: TargetRef) -> String {
    match r {
        TargetRef::Target(i) => {
            let text = mode
                .targets
                .get(i as usize)
                .map_or("the target".to_string(), target_filter);
            // A slot that may stay empty (CR 115.3: "up to one target …").
            let text = match mode.group_of(i as usize) {
                Some(g) if g.count == 1 && g.min == 0 => match text.strip_prefix("another ") {
                    Some(rest) => format!("up to one other {rest}"),
                    None => format!("up to one {text}"),
                },
                _ => text,
            };
            // Marked, so that a later mention of the same target can say "that creature".
            format!("{TARGET}{i}{TARGET}{text}{TARGET_END}")
        }
        TargetRef::Players(p) => player(mode, p),
        TargetRef::Object(o) => obj(mode, o),
    }
}

/// What a later instruction calls a target already named: "that creature", "that player".
/// `None` when the words would not say which of several targets is meant.
fn later_mention(mode: &Mode, i: usize) -> Option<String> {
    let word = |f: &TargetFilter| -> String {
        match f {
            TargetFilter::Creature => "that creature".to_string(),
            TargetFilter::Player | TargetFilter::Opponent => "that player".to_string(),
            TargetFilter::Spell | TargetFilter::SpellMatching(_) => "that spell".to_string(),
            TargetFilter::OnStack {
                abilities: false, ..
            } => "that spell".to_string(),
            TargetFilter::OnStack {
                abilities: true, ..
            } => "it".to_string(),
            TargetFilter::AbilityOnStack { .. } => "that ability".to_string(),
            TargetFilter::AnyTarget => "it".to_string(),
            TargetFilter::CardInGraveyard { .. } => "that card".to_string(),
            TargetFilter::Permanent(sels) => match sels.as_slice() {
                [s] if s.types.len() == 1 && s.subtypes.is_empty() => {
                    format!("that {}", card_type(s.types[0]))
                }
                [s] if s.types.is_empty() && s.subtypes.len() == 1 => {
                    format!("that {}", s.subtypes[0])
                }
                _ => "that permanent".to_string(),
            },
        }
    };
    let mine = word(mode.targets.get(i)?);
    // Another target that would be called the same: don't say which.
    let group = mode.group_of(i).map(|g| g.first as usize);
    let clash = mode.targets.iter().enumerate().any(|(j, f)| {
        j != i
            && word(f) == mine
            && !(group.is_some() && mode.group_of(j).map(|g| g.first as usize) == group)
    });
    if clash { None } else { Some(mine) }
}

/// The text of an instruction with its target marks resolved: the first mention of a
/// target says "target creature", later ones "that creature".
fn resolve_targets(mode: &Mode, text: &str) -> String {
    let mut out = String::new();
    let mut seen: Vec<usize> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(TARGET) {
        out.push_str(&rest[..start]);
        let after = &rest[start + TARGET.len_utf8()..];
        let Some(mid) = after.find(TARGET) else {
            rest = after;
            break;
        };
        let index: usize = after[..mid].parse().unwrap_or(usize::MAX);
        let after = &after[mid + TARGET.len_utf8()..];
        let Some(end) = after.find(TARGET_END) else {
            rest = after;
            break;
        };
        let shown = &after[..end];
        rest = &after[end + TARGET_END.len_utf8()..];
        if seen.contains(&index) {
            match later_mention(mode, index) {
                Some(words) => out.push_str(&words),
                None => out.push_str(shown),
            }
        } else {
            seen.push(index);
            out.push_str(shown);
        }
    }
    out.push_str(rest);
    out
}

/// `text` without its target marks (the words as they were first said).
fn plain_targets(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    let mut index = false;
    for c in text.chars() {
        match c {
            TARGET if !inside => {
                inside = true;
                index = true;
            }
            TARGET if index => index = false,
            TARGET_END => inside = false,
            _ if inside && index => {}
            c => out.push(c),
        }
    }
    out
}

/// The instruction a mode performs, as text.
fn mode_effect(m: &Mode) -> String {
    resolve_targets(m, &effect(m, &m.effect))
}

fn obj(mode: &Mode, r: ObjRef) -> String {
    match r {
        ObjRef::Target(i) => target(mode, TargetRef::Target(i)),
        ObjRef::This => "this permanent".into(),
        ObjRef::Triggering => "it".into(),
        ObjRef::ThisCard => "this card".into(),
        ObjRef::TriggeringCard => "that card".into(),
        ObjRef::Noted => "those cards".into(),
        // `rules_text` says "equipped" for an Equipment.
        ObjRef::Host => "enchanted creature".into(),
    }
}

/// ", except it isn't legendary and it has haste" (CR 707.9); empty for none.
fn copy_except(except: &[CopyExcept]) -> String {
    let parts: Vec<String> = except
        .iter()
        .map(|e| match e {
            CopyExcept::NotLegendary => "it isn't legendary".to_string(),
            CopyExcept::AddKeywords(ks) => format!(
                "it has {}",
                join(
                    ks.iter().map(|k| keyword(*k).to_lowercase()).collect(),
                    "and"
                )
            ),
            CopyExcept::AddTypes {
                card_types,
                subtypes,
            } => {
                let mut words: Vec<String> = subtypes.clone();
                words.extend(card_types.iter().map(|t| card_type(*t).to_string()));
                format!(
                    "it's {} in addition to its other types",
                    with_article(&words.join(" "))
                )
            }
            CopyExcept::SetPT { power, toughness } => format!("it's {power}/{toughness}"),
            CopyExcept::SetColors(cs) if cs.is_empty() => "it's colorless".to_string(),
            CopyExcept::SetColors(cs) => format!(
                "it's {}",
                join(cs.iter().map(|c| color(*c).to_string()).collect(), "and")
            ),
            CopyExcept::SetCreatureTypes(ts) => format!("it's {}", with_article(&ts.join(" "))),
            CopyExcept::HasAbilities(indices) => format!(
                "it has {}",
                join(
                    indices
                        .iter()
                        .map(|i| format!("{GRANTED}{i}{GRANTED}"))
                        .collect(),
                    "and"
                )
            ),
        })
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(", except {}", join(parts, "and"))
    }
}

fn player(mode: &Mode, r: PlayerRef) -> String {
    match r {
        PlayerRef::You => "you".into(),
        PlayerRef::Target(i) => target(mode, TargetRef::Target(i)),
        PlayerRef::EachOpponent => "each opponent".into(),
        PlayerRef::Each => "each player".into(),
        PlayerRef::TriggeringController => "its controller".into(),
        PlayerRef::TriggeringPlayer => "that player".into(),
        PlayerRef::ControllerOf(_) => "its controller".into(),
        PlayerRef::OwnerOf(_) => "its owner".into(),
        PlayerRef::NotedControllers => "their controllers".into(),
        PlayerRef::NotNotedControllers => "each player who controlled none of those cards".into(),
    }
}

/// "draw" (an instruction to its controller has no subject) vs "target player draws".
fn verb(who: &str, base: &str, third: &str) -> String {
    if who == "you" {
        base.to_string()
    } else {
        format!("{who} {third}")
    }
}

/// "you gain" (Oracle keeps "you" for life) vs "target player gains".
fn verb_you(who: &str, base: &str, third: &str) -> String {
    if who == "you" {
        format!("you {base}")
    } else {
        format!("{who} {third}")
    }
}

/// `rest` said of `who`: "search your library" for the controller, "target player
/// searches their library" for another.
fn lead(who: &str, rest: &str) -> String {
    if who == "you" {
        rest.to_string()
    } else {
        format!("{who} {rest}")
    }
}

fn selector(s: &Selector, plural: bool) -> String {
    let mut out = String::new();
    if s.other {
        out.push_str(if plural { "other " } else { "another " });
    }
    if s.token == Some(false) {
        out.push_str("nontoken ");
    }
    if s.tapped == Some(true) {
        out.push_str("tapped ");
    } else if s.tapped == Some(false) {
        out.push_str("untapped ");
    }
    match (s.attacking, s.blocking) {
        (Some(true), Some(true)) => out.push_str("attacking or blocking "),
        (Some(true), _) => out.push_str("attacking "),
        (_, Some(true)) => out.push_str("blocking "),
        (Some(false), Some(false)) => out.push_str("nonattacking, nonblocking "),
        (Some(false), _) => out.push_str("nonattacking "),
        (_, Some(false)) => out.push_str("nonblocking "),
        (None, None) => {}
    }
    // At least one of them: "white or blue permanent".
    if !s.colors.is_empty() {
        let colors = s.colors.iter().map(|c| color(*c).to_string()).collect();
        out.push_str(&join(colors, "or"));
        out.push(' ');
    }
    if s.not_colors.len() == 5 {
        out.push_str("colorless ");
    } else {
        for c in &s.not_colors {
            out.push_str(&format!("non{c:?} ").to_lowercase());
        }
    }
    for st in &s.not_supertypes {
        out.push_str(&format!("non{st:?} ").to_lowercase());
    }
    for st in &s.not_subtypes {
        out.push_str(&format!("non-{st} "));
    }
    for st in &s.supertypes {
        out.push_str(&format!("{st:?} ").to_lowercase());
    }
    for t in &s.not_types {
        out.push_str(&format!("non{} ", card_type(*t)));
    }
    // "Dinosaurs", "Dragon creatures", "creatures", "permanents"; "Elf tokens",
    // "creature tokens" (the type is an adjective of "token").
    let token = s.token == Some(true);
    let noun = match (s.types.is_empty(), s.subtypes.is_empty()) {
        (true, true) if token => String::new(),
        (true, true) if plural => "permanents".to_string(),
        (true, true) => "permanent".to_string(),
        (true, false) if plural && !token => {
            join(s.subtypes.iter().map(|t| pluralize(t)).collect(), "or")
        }
        (true, false) => s.subtypes.join(" or "),
        (false, true) if plural && !token => types_plural(&s.types, "or"),
        (false, true) => types(&s.types, "or"),
        (false, false) if plural && !token => {
            format!(
                "{} {}",
                s.subtypes.join(" or "),
                types_plural(&s.types, "or")
            )
        }
        (false, false) => format!("{} {}", s.subtypes.join(" or "), types(&s.types, "or")),
    };
    out.push_str(&noun);
    if token {
        if !noun.is_empty() {
            out.push(' ');
        }
        out.push_str(if plural { "tokens" } else { "token" });
    }
    match s.has_adventure {
        Some(true) => out.push_str(if plural {
            " that have an Adventure"
        } else {
            " that has an Adventure"
        }),
        Some(false) => out.push_str(" without an Adventure"),
        None => {}
    }
    for k in &s.keywords {
        out.push_str(&format!(" with {}", keyword(*k).to_lowercase()));
    }
    for k in &s.not_keywords {
        out.push_str(&format!(" without {}", keyword(*k).to_lowercase()));
    }
    out.push_str(&power_toughness(s));
    out.push_str(&mana_value(s));
    if !s.names.is_empty() {
        out.push_str(&format!(" named {}", s.names.join(" or ")));
    }
    out.push_str(noted(s.noted));
    match s.owner {
        Who::Any => {}
        Who::You => out.push_str(" you own"),
        Who::Opponent => out.push_str(if plural {
            " your opponents own"
        } else {
            " an opponent owns"
        }),
    }
    if s.owner != Who::Any && s.controller != Who::Any {
        out.push_str(" and");
    }
    match s.controller {
        Who::Any => {}
        Who::You => out.push_str(" you control"),
        Who::Opponent => out.push_str(if plural {
            " your opponents control"
        } else {
            " an opponent controls"
        }),
    }
    // "Creatures you control of the chosen type".
    out.push_str(of_chosen(s, plural));
    out
}

pub fn counter(k: CounterKind) -> String {
    match k {
        CounterKind::PlusOne => "+1/+1".into(),
        CounterKind::MinusOne => "-1/-1".into(),
        CounterKind::Named(n) => n.as_str().into(),
    }
}

/// "a +1/+1 counter", "two -1/-1 counters", "X +1/+1 counters".
fn counters_phrase(count: &Amount, kind: CounterKind) -> String {
    quantity(
        count,
        &format!("{} counter", counter(kind)),
        &format!("{} counters", counter(kind)),
    )
}

fn signed(n: i32) -> String {
    if n >= 0 {
        format!("+{n}")
    } else {
        n.to_string()
    }
}

/// "gets +1/+1 and has flying" for a subject (`plural` picks get/gets, have/has).
fn continuous(effects: &[ContinuousEffect], plural: bool) -> String {
    let (get, have, lose, is, become_) = if plural {
        ("get", "have", "lose", "are", "become")
    } else {
        ("gets", "has", "loses", "is", "becomes")
    };
    let kws = |ks: &[Keyword]| {
        join(
            ks.iter().map(|k| keyword(*k).to_lowercase()).collect(),
            "and",
        )
    };
    let parts: Vec<String> = effects
        .iter()
        .enumerate()
        .map(|(i, e)| match e {
            ContinuousEffect::GainControl => "is under your control".to_string(),
            ContinuousEffect::AddTypes {
                card_types,
                subtypes,
                land_types,
            } => {
                let mut names: Vec<String> = card_types
                    .iter()
                    .map(|t| format!("{}{}", card_type(*t), if plural { "s" } else { "" }))
                    .collect();
                names.extend(land_types.iter().map(|t| format!("{t:?}")));
                names.extend(subtypes.iter().cloned());
                format!("{is} also {}", join(names, "and"))
            }
            ContinuousEffect::SetColors(cs) => {
                format!(
                    "{is} {}",
                    join(cs.iter().map(|c| color(*c).to_string()).collect(), "and")
                )
            }
            ContinuousEffect::AddKeywords(ks) => format!("{have} {}", kws(ks)),
            ContinuousEffect::LoseKeywords(ks) => format!("{lose} {}", kws(ks)),
            ContinuousEffect::LoseAllAbilities => format!("{lose} all abilities"),
            ContinuousEffect::SetPT(p, t) => format!("{have} base power and toughness {p}/{t}"),
            // A zero takes the sign of the other half: "-1/-0", as cards print it.
            ContinuousEffect::ModifyPT(p, t) => {
                let half = |n: i32, other: i32| {
                    if n == 0 && other < 0 {
                        "-0".to_string()
                    } else {
                        signed(n)
                    }
                };
                format!("{get} {}/{}", half(*p, *t), half(*t, *p))
            }
            ContinuousEffect::ModifyPTBy { power, toughness } => {
                let sign = |s: &crate::Signed| if s.negative { "-" } else { "+" };
                let part = |s: &crate::Signed| format!("{}{}", sign(s), amount(&s.amount));
                // "+X/+X, where X is the number of …" when one count is used for both.
                let counted = |s: &crate::Signed| !matches!(s.amount, Amount::Fixed(_) | Amount::X);
                // "+1/+0 for each artifact you control", "-1/-1 for each Elf": each half is
                // a count of the same permanents, or zero.
                fn each(s: &crate::Signed) -> Option<Option<&Selector>> {
                    match &s.amount {
                        Amount::Count(sel) => Some(Some(sel)),
                        Amount::Fixed(0) => Some(None),
                        _ => None,
                    }
                }
                if let (Some(p), Some(t)) = (each(power), each(toughness))
                    && let Some(sel) = p.or(t)
                    && p.is_none_or(|x| x == sel)
                    && t.is_none_or(|x| x == sel)
                {
                    let half = |s: &crate::Signed, n: Option<&Selector>, other: &crate::Signed| {
                        let negative = if n.is_some() {
                            s.negative
                        } else {
                            other.negative
                        };
                        format!(
                            "{}{}",
                            if negative { "-" } else { "+" },
                            if n.is_some() { 1 } else { 0 }
                        )
                    };
                    let place = if sel.controller == Who::Any && sel.owner == Who::Any {
                        " on the battlefield"
                    } else {
                        ""
                    };
                    return format!(
                        "{get} {}/{} for each {}{place}",
                        half(power, p, toughness),
                        half(toughness, t, power),
                        selector(sel, false)
                    );
                }
                if power.amount == toughness.amount && counted(power) {
                    format!(
                        "{get} {}X/{}X, where X is {}",
                        sign(power),
                        sign(toughness),
                        amount(&power.amount)
                    )
                } else {
                    format!("{get} {}/{}", part(power), part(toughness))
                }
            }
            ContinuousEffect::SwitchPT => format!("{become_} power-and-toughness switched"),
            ContinuousEffect::AddChosenType => {
                let its = if plural { "their" } else { "its" };
                format!("{is} the chosen type in addition to {its} other types")
            }
            ContinuousEffect::RemoveTypes(ts) => {
                let not = if plural { "aren't" } else { "isn't" };
                let names: Vec<String> = ts
                    .iter()
                    .map(|t| {
                        let name = card_type(*t);
                        if plural {
                            format!("{name}s")
                        } else {
                            with_article(name)
                        }
                    })
                    .collect();
                format!("{not} {}", join(names, "or"))
            }
            ContinuousEffect::SetPTBy { power, toughness } => match (power, toughness) {
                (Some(p), Some(t)) if p == t => {
                    format!("{have} power and toughness each equal to {}", amount(p))
                }
                (Some(p), Some(t)) => format!(
                    "{have} power equal to {} and toughness equal to {}",
                    amount(p),
                    amount(t)
                ),
                (Some(p), None) => format!("{have} power equal to {}", amount(p)),
                (None, Some(t)) => format!("{have} toughness equal to {}", amount(t)),
                (None, None) => String::new(),
            },
            // The quoted texts are filled in by `quote_granted`, which has the card.
            ContinuousEffect::GrantAbilities(indices) => {
                let quoted = join(
                    indices
                        .iter()
                        .map(|i| format!("{GRANTED}{i}{GRANTED}"))
                        .collect(),
                    "and",
                );
                // "Has flying and \"…\"": one "has" for keywords and quoted abilities.
                match i.checked_sub(1).map(|j| &effects[j]) {
                    Some(ContinuousEffect::AddKeywords(_)) => quoted,
                    _ => format!("{have} {quoted}"),
                }
            }
        })
        .collect();
    join(parts, "and")
}

/// Brackets the index of a granted ability until its text is filled in.
const GRANTED: char = '\u{1}';
/// Brackets "index|what it is about" of an ability a lasting effect carries.
const LASTING: char = '\u{2}';
/// Starts the "what it is about" of a delayed triggered ability: when it can trigger.
const DELAYED: char = '\u{3}';

/// The text of a delayed ability about objects (CR 603.7c), worded as cards say it: "it"
/// for one object, "those cards" for several; a card put onto the battlefield with no
/// controller named goes there under its owner's control (CR 110.2a).
fn delayed_objects(text: &str, one: bool) -> String {
    let (it, its, owners) = if one {
        ("it", "its", "its owner's")
    } else {
        ("those cards", "their", "their owner's")
    };
    let _ = its;
    text.replace(
        "put those cards onto the battlefield tapped under your control",
        &format!("return {it} to the battlefield tapped under your control"),
    )
    .replace(
        "put those cards onto the battlefield under your control",
        &format!("return {it} to the battlefield under your control"),
    )
    .replace(
        "put those cards onto the battlefield tapped",
        &format!("return {it} to the battlefield tapped under {owners} control"),
    )
    .replace(
        "put those cards onto the battlefield",
        &format!("return {it} to the battlefield under {owners} control"),
    )
    .replace("you sacrifice those cards", &format!("sacrifice {it}"))
    .replace("those cards", it)
}

/// Replace each marker `continuous` left with the quoted text of that ability of
/// `def.granted` (which can quote others in turn).
fn quote_granted(def: &CardDef, text: &str, top: bool) -> String {
    // The abilities of lasting effects first, said outright: "target creature can't be
    // blocked" for "That permanent can't be blocked."
    let mut said = String::new();
    let mut parts = text.split(LASTING);
    while let Some(plain) = parts.next() {
        said.push_str(plain);
        let Some(marker) = parts.next() else { break };
        let (index, about) = marker.split_once('|').unwrap_or((marker, ""));
        match index.parse::<usize>().ok().and_then(|i| def.granted.get(i)) {
            Some(a) => {
                let mut text = ability(a);
                // A delayed triggered ability (CR 603.7): "at the beginning of the next
                // upkeep", "… of your next main phase", "… of the next turn's upkeep".
                let delayed = about.strip_prefix(DELAYED);
                if let Some(d) = delayed {
                    let mut parts = d.split('|');
                    let when = parts.next().unwrap_or("");
                    let much = parts.next().unwrap_or("");
                    let objects = parts.next().unwrap_or("");
                    if !much.is_empty() {
                        text = text.replace("that much", much);
                    }
                    if !objects.is_empty() {
                        text = delayed_objects(&text, objects == "it");
                    }
                    let next = if when == "next-turn" {
                        "the next turn's"
                    } else {
                        "the next"
                    };
                    text = text
                        .replace(
                            "At the beginning of each player's ",
                            &format!("at the beginning of {next} "),
                        )
                        .replace(
                            "At the beginning of each opponent's ",
                            "at the beginning of the next opponent's ",
                        )
                        .replace(
                            "At the beginning of your ",
                            "at the beginning of your next ",
                        )
                        .replace(
                            "At end of combat on each opponent's turn",
                            "at end of combat on an opponent's turn",
                        )
                        .replace("At end of combat", "at end of combat");
                    if when == "this-turn" {
                        text = text.replacen(", ", " this turn, ", 1);
                    }
                }
                let about = if delayed.is_some() { "" } else { about };
                if !about.is_empty() {
                    text = text
                        .replace("That permanent", about)
                        .replace("that permanent", about);
                }
                let mut text = text.trim_end_matches('.');
                // A rule about players for one turn: "you may play an additional land
                // this turn", not "on each of your turns this turn".
                if matches!(a, AbilityDef::PlayerRule { .. }) {
                    for each in [" on each of your turns", " on each of their turns"] {
                        text = text.strip_suffix(each).unwrap_or(text);
                    }
                }
                // (Lower case inside a sentence: "…. Then target creature can't …".)
                let starts = said.is_empty() || said.ends_with(": ") || said.ends_with("• ");
                let mut chars = text.chars();
                if let Some(first) = chars.next() {
                    if starts {
                        said.extend(first.to_uppercase());
                    } else {
                        said.extend(first.to_lowercase());
                    }
                    said.push_str(chars.as_str());
                }
            }
            None => said.push_str("[a missing ability]"),
        }
    }
    // "this permanent" is what the card is: "this creature", "this Aura" — but in the
    // quoted text of an ability it gives to others, those others.
    let typed;
    let text = if top {
        let word = this_word(def);
        let mut t = said
            .replace("this permanent", &format!("this {word}"))
            .replace("This permanent", &capitalize(&format!("this {word}")));
        // Only a creature "dies" (CR 700.4); anything else is put into a graveyard.
        if !matches!(word.as_str(), "creature" | "token" | "permanent" | "spell") {
            t = t
                .replace(
                    &format!("this {word} enters or dies"),
                    &format!("this {word} enters or is put into a graveyard from the battlefield"),
                )
                .replace(
                    &format!("this {word} dies"),
                    &format!("this {word} is put into a graveyard from the battlefield"),
                );
        }
        typed = t;
        typed.as_str()
    } else {
        said.as_str()
    };
    let mut out = String::new();
    let mut parts = text.split(GRANTED);
    // A quotation that ends a sentence ends it with its own full stop.
    let mut ends_sentence = false;
    while let Some(plain) = parts.next() {
        out.push_str(match plain.strip_prefix('.') {
            Some(rest) if ends_sentence => rest,
            _ => plain,
        });
        ends_sentence = false;
        let Some(index) = parts.next() else { break };
        match index.parse::<usize>().ok().and_then(|i| def.granted.get(i)) {
            Some(a) => {
                out.push('"');
                out.push_str(&quote_granted(def, &ability(a), false));
                out.push('"');
                ends_sentence = out.ends_with(".\"");
            }
            None => out.push_str("[a missing ability]"),
        }
    }
    out
}

fn effect(mode: &Mode, e: &Effect) -> String {
    match e {
        Effect::CopyPermanent {
            what,
            count,
            except,
            gains,
            tapped,
        } => {
            let n = amount(count);
            let one = n == "1";
            let tokens = match (one, *tapped) {
                (true, false) => "a token that's".to_string(),
                (true, true) => "a tapped token that's".to_string(),
                (false, false) => format!("{n} tokens that are"),
                (false, true) => format!("{n} tapped tokens that are"),
            };
            let copies = if one { "a copy" } else { "copies" };
            let except = copy_except(except);
            let mut out = match what {
                CopyWhat::Obj(r) => {
                    format!("you create {tokens} {copies} of {}{except}", obj(mode, *r))
                }
                CopyWhat::Each(s) => format!(
                    "for each {}, you create {tokens} {copies} of that permanent{except}",
                    selector(s, false)
                ),
            };
            let several = !one || matches!(what, CopyWhat::Each(_));
            if !gains.is_empty() {
                out.push_str(&format!(
                    ". {} {}",
                    if several {
                        "Those tokens gain"
                    } else {
                        "That token gains"
                    },
                    join(
                        gains.iter().map(|k| keyword(*k).to_lowercase()).collect(),
                        "and"
                    )
                ));
            }
            out
        }
        Effect::SacrificeIt { what } => format!("you sacrifice {}", obj(mode, *what)),
        Effect::CopySpell {
            what,
            count,
            new_targets,
            except,
        } => {
            let it = match what {
                // Storm: "When you cast this spell, copy it …".
                ObjRef::This if *count == Amount::TriggeringAmount => "it".to_string(),
                ObjRef::This => "this spell".to_string(),
                ObjRef::Triggering => "that spell".to_string(),
                other => obj(mode, *other),
            };
            let (times, several) = match count {
                Amount::Fixed(1) => (String::new(), false),
                Amount::Fixed(2) => (" twice".to_string(), true),
                // Storm (CR 702.40a); "copy it for each other instant and sorcery spell
                // you've cast before it this turn".
                Amount::TriggeringAmount if *what == ObjRef::This => (
                    " for each other spell that was cast before it this turn".to_string(),
                    true,
                ),
                Amount::TriggeringAmount => (
                    " for each other such spell you've cast before it this turn".to_string(),
                    true,
                ),
                n => (format!(" {} times", amount(n)), true),
            };
            let storm = *what == ObjRef::This && *count == Amount::TriggeringAmount;
            format!(
                "copy {it}{times}{}{}",
                copy_except(except),
                match (*new_targets, several) {
                    (false, _) => "",
                    // CR 702.40a.
                    (true, _) if storm => {
                        ". If the spell has any targets, you may choose new targets for any of the copies"
                    }
                    (true, false) => ". You may choose new targets for the copy",
                    (true, true) => ". You may choose new targets for the copies",
                }
            )
        }
        Effect::BecomeCopy { what, except } => format!(
            "this permanent becomes a copy of {}{}",
            obj(mode, *what),
            copy_except(except)
        ),
        // The abilities' texts are filled in by `quote_granted`, which has the card.
        Effect::Lasting {
            abilities,
            about,
            until_end_of_turn,
            ..
        } => {
            let about = about.map(|r| obj(mode, r)).unwrap_or_default();
            let parts = abilities
                .iter()
                .map(|i| format!("{LASTING}{i}|{about}{LASTING}"))
                .collect();
            let turn = if *until_end_of_turn { " this turn" } else { "" };
            format!("{}{turn}", join(parts, "and"))
        }
        Effect::Choose(what) => format!("choose {}", choice_of(*what)),
        Effect::Seq(v) => v
            .iter()
            .map(|x| effect(mode, x))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(". Then "),
        Effect::DealDamage { to, amount: a, by } => {
            let who = match by {
                DamageBy::This => "this permanent".to_string(),
                DamageBy::Triggering => "it".to_string(),
                DamageBy::Target(i) => target(mode, TargetRef::Target(*i)),
            };
            let to = match (by, to) {
                (DamageBy::Target(i), TargetRef::Target(j)) if i == j => "itself".to_string(),
                _ => target(mode, *to),
            };
            format!("{who} deals {} to {to}", amount_of(a, "damage"))
        }
        Effect::DamageEachPlayer { amount: a } => {
            format!(
                "this permanent deals {} to each player",
                amount_of(a, "damage")
            )
        }
        Effect::Draw { who, amount: a } => {
            format!(
                "{} {}",
                verb(&player(mode, *who), "draw", "draws"),
                quantity(a, "card", "cards")
            )
        }
        Effect::AddMana {
            who,
            mana,
            amount: a,
        } => match a {
            Amount::Fixed(n) => format!(
                "{} {}",
                verb(&player(mode, *who), "add", "adds"),
                mana.symbol().repeat(*n as usize)
            ),
            Amount::X => format!(
                "{} X mana of type {}",
                verb(&player(mode, *who), "add", "adds"),
                mana.symbol()
            ),
            Amount::Count(s) => format!(
                "{} {} for each {}",
                verb(&player(mode, *who), "add", "adds"),
                mana.symbol(),
                selector(s, false)
            ),
            a => format!(
                "{} X mana of type {}, where X is {}",
                verb(&player(mode, *who), "add", "adds"),
                mana.symbol(),
                amount(a)
            ),
        },
        Effect::AddManaOf { who, of, amount: a } => {
            let kind = if of.len() == 5 {
                "any one color".to_string()
            } else {
                join(of.iter().map(|t| t.symbol().to_string()).collect(), "or")
            };
            let add = verb(&player(mode, *who), "add", "adds");
            match a {
                Amount::Fixed(1) if of.len() == 5 => format!("{add} one mana of any color"),
                Amount::Fixed(1) => format!("{add} {kind}"),
                Amount::Fixed(n) => format!("{add} {n} mana of {kind}"),
                Amount::X => format!("{add} X mana of {kind}"),
                a => format!("{add} X mana of {kind}, where X is {}", amount(a)),
            }
        }
        Effect::GainLife {
            who,
            amount: Amount::LifeLostThisWay,
        } => format!(
            "{} life equal to the life lost this way",
            verb_you(&player(mode, *who), "gain", "gains"),
        ),
        Effect::GainLife { who, amount: a } => {
            format!(
                "{} {}",
                verb_you(&player(mode, *who), "gain", "gains"),
                amount_of(a, "life")
            )
        }
        Effect::LoseLife { who, amount: a } => {
            format!(
                "{} {}",
                verb_you(&player(mode, *who), "lose", "loses"),
                amount_of(a, "life")
            )
        }
        Effect::Counter { what } => format!("counter {}", target(mode, *what)),
        Effect::EachPlayer(a) => match a {
            EachPlayerAction::Discard(n) => format!(
                "each player discards {}",
                quantity(&Amount::Fixed(*n), "card", "cards")
            ),
            EachPlayerAction::SacrificeCreature(n) => {
                format!(
                    "each player sacrifices {}",
                    quantity(&Amount::Fixed(*n), "creature", "creatures")
                )
            }
            EachPlayerAction::Draw(n) => format!(
                "each player draws {}",
                quantity(&Amount::Fixed(*n), "card", "cards")
            ),
        },
        Effect::Destroy { what } => format!("destroy {}", obj(mode, *what)),
        Effect::ReturnToHand { what } => {
            format!("return {} to its owner's hand", obj(mode, *what))
        }
        Effect::CreateToken { token, count, who } => {
            // The token's description is filled in by `expand_tokens`, which has the
            // card catalog.
            let how_many = match count {
                Amount::Fixed(1) => "one".to_string(),
                Amount::Fixed(n) => format!("n:{}", num(*n)),
                Amount::X => "n:X".to_string(),
                Amount::TriggeringAmount => "n:that many".to_string(),
                a => format!("eq:{}", amount(a)),
            };
            format!(
                "{} {TOKEN}{how_many}|{token}{TOKEN}",
                verb(&player(mode, *who), "create", "creates"),
            )
        }
        Effect::PutCountersAll { what, kind, count } => format!(
            "put {} on each {}",
            counters_phrase(count, *kind),
            selector(what, false)
        ),
        Effect::PutCounters { what, kind, count } => format!(
            "put {} on {}",
            counters_phrase(count, *kind),
            obj(mode, *what)
        ),
        Effect::Apply {
            what,
            effects,
            until_end_of_turn,
        } => format!(
            "{} {}{}",
            obj(mode, *what),
            continuous(effects, false),
            if *until_end_of_turn {
                " until end of turn"
            } else {
                ""
            }
        ),
        Effect::PreventNext { to, amount: n } => format!(
            "prevent the next {n} damage that would be dealt to {} this turn",
            target(mode, *to)
        ),
        Effect::DestroyAll(s) => format!("destroy all {}", selector(s, true)),
        Effect::DamageEach { each, amount: a } => format!(
            "this permanent deals {} to each {}",
            amount_of(a, "damage"),
            selector(each, false)
        ),
        Effect::Exile { what } => format!("exile {}", obj(mode, *what)),
        Effect::ExileGraveyard { who } => match who {
            PlayerRef::You => "exile your graveyard".to_string(),
            other => format!("exile {}'s graveyard", player(mode, *other)),
        },
        Effect::Mill { who, amount: a } => {
            format!(
                "{} {}",
                verb(&player(mode, *who), "mill", "mills"),
                quantity(a, "card", "cards")
            )
        }
        Effect::ApplyAll {
            affected,
            effects,
            until_end_of_turn,
        } => format!(
            "{} {}{}",
            selector(affected, true),
            continuous(effects, true),
            if *until_end_of_turn {
                " until end of turn"
            } else {
                ""
            }
        ),
        // "X is this creature's power": "its" could be read as another object's.
        Effect::SetX(Amount::SourcePower) => "X is this permanent's power".to_string(),
        Effect::SetX(Amount::SourceToughness) => "X is this permanent's toughness".to_string(),
        Effect::SetX(a) => format!("X is {}", amount(a)),
        Effect::Tap { what } => format!("tap {}", obj(mode, *what)),
        Effect::Transform { what } => format!("transform {}", obj(mode, *what)),
        Effect::Untap { what } => format!("untap {}", obj(mode, *what)),
        Effect::TapAll(s) => format!("tap all {}", selector(s, true)),
        Effect::UntapAll(s) => format!("untap all {}", selector(s, true)),
        Effect::Discard { who, amount: a } => {
            format!(
                "{} {}",
                verb(&player(mode, *who), "discard", "discards"),
                quantity(a, "card", "cards")
            )
        }
        Effect::DiscardHand { who } => match who {
            PlayerRef::You => "discard your hand".to_string(),
            other => format!("{} discards their hand", player(mode, *other)),
        },
        Effect::Sacrifice {
            who,
            what,
            amount: a,
        } => {
            let things = match a {
                Amount::Fixed(1) => with_article(&selector(what, false)),
                Amount::Fixed(n) => format!("{} {}", num(*n), selector(what, true)),
                a => format!("{} {}", amount(a), selector(what, true)),
            };
            format!(
                "{} {things}",
                verb(&player(mode, *who), "sacrifice", "sacrifices")
            )
        }
        Effect::SacrificeThis => "sacrifice this permanent".to_string(),
        Effect::ControllerSacrificesThis => "its controller sacrifices it".to_string(),
        // (card-db's `oracle_words_missing` strips "attach this <type> to target creature you
        // control" as Equip's reminder text: keep this wording.)
        // The delayed ability's own text, filled in by `quote_granted` ("at the beginning
        // of the next upkeep, …").
        Effect::DelayedTrigger {
            ability,
            window,
            amount: much,
            objects,
            one,
            ..
        } => {
            let when = match window {
                DelayWindow::Next => "next",
                DelayWindow::ThisTurn => "this-turn",
                DelayWindow::NextTurn => "next-turn",
            };
            // What "that much" of the delayed ability is.
            let much = match much {
                Some(Amount::OfTarget {
                    slot,
                    what: Measure::ManaValue,
                }) if matches!(
                    mode.targets.get(*slot as usize),
                    Some(TargetFilter::Spell | TargetFilter::SpellMatching(_))
                ) =>
                {
                    "that spell's mana value".to_string()
                }
                Some(a) => amount(a),
                None => String::new(),
            };
            // Whom it is about: "it" (one object), "those cards", or nothing.
            let about = match (objects, one) {
                (None, _) => "",
                (Some(_), true) => "it",
                (Some(_), false) => "them",
            };
            format!("{LASTING}{ability}|{DELAYED}{when}|{much}|{about}{LASTING}")
        }
        Effect::AttachThis { to } => format!("attach this permanent to {}", obj(mode, *to)),
        // "You may have that player shuffle."
        Effect::May(inner) if matches!(**inner, Effect::Shuffle { who } if who != PlayerRef::You) =>
        {
            let Effect::Shuffle { who } = **inner else {
                unreachable!()
            };
            format!("you may have {} shuffle their library", player(mode, who))
        }
        Effect::May(inner) => {
            let text = effect(mode, inner);
            // "You may draw a card", not "you may you draw a card"; and an instruction
            // that is a choice already says "you may".
            if text.starts_with("you may ") {
                text
            } else if let Some(rest) = text.strip_prefix("this permanent deals ") {
                format!("you may have this permanent deal {rest}")
            } else if let Some(who) = acting_player(inner).filter(|p| *p != PlayerRef::You) {
                // "You may have target opponent lose 1 life", not "you may target opponent
                // loses 1 life".
                let who = player(mode, who);
                match text.strip_prefix(&format!("{who} ")) {
                    Some(rest) => {
                        let (verb, tail) = rest.split_once(' ').unwrap_or((rest, ""));
                        format!("you may have {who} {} {tail}", base_verb(verb))
                    }
                    None => format!("you may {text}"),
                }
            } else if let Effect::Apply {
                what,
                effects,
                until_end_of_turn,
            } = &**inner
            {
                format!(
                    "you may have {} {}{}",
                    obj(mode, *what),
                    continuous(effects, true),
                    if *until_end_of_turn {
                        " until end of turn"
                    } else {
                        ""
                    }
                )
            } else {
                format!("you may {}", text.strip_prefix("you ").unwrap_or(&text))
            }
        }
        Effect::IfDone { first, then } => {
            format!("{}. If you do, {}", effect(mode, first), effect(mode, then))
        }
        Effect::Proliferate => "proliferate".to_string(),
        Effect::ChangeTargets { what, single } => {
            if *single {
                format!("change the target of {}", target(mode, *what))
            } else {
                format!("you may choose new targets for {}", target(mode, *what))
            }
        }
        Effect::Regenerate { what } => format!("regenerate {}", obj(mode, *what)),
        Effect::CantBeRegenerated(inner) => format!(
            "{}. {} can't be regenerated",
            effect(mode, inner),
            if matches!(**inner, Effect::DestroyAll(_)) {
                "They"
            } else {
                "It"
            }
        ),
        Effect::Shuffle { who } => format!(
            "{} {} library",
            verb(&player(mode, *who), "shuffle", "shuffles"),
            if player(mode, *who) == "you" {
                "your"
            } else {
                "their"
            }
        ),
        Effect::DrawUpTo { who, amount: a } => format!(
            "{} draw up to {}",
            match player(mode, *who).as_str() {
                "you" => "you may".to_string(),
                other => format!("{other} may"),
            },
            match a {
                Amount::Fixed(1) => "one card".to_string(),
                Amount::Fixed(n) => format!("{} cards", num(*n)),
                a => format!("{} cards", amount(a)),
            }
        ),
        Effect::UntapChosen { what, count } => {
            let n = match count {
                Amount::Fixed(k) => num(*k),
                a => amount(a),
            };
            format!("untap up to {n} {}", selector(what, true))
        }
        // "Its controller may draw a card": the instruction in the decider's own words.
        Effect::PlayerMay { who, effect: inner } => {
            let who = player(mode, *who);
            let text = effect(mode, inner);
            let rest = match text.strip_prefix(&format!("{who} ")) {
                // "its controller draws a card" -> "draw a card".
                Some(rest) => {
                    let (verb, tail) = rest.split_once(' ').unwrap_or((rest, ""));
                    if verb == "may" {
                        // Already a choice of theirs ("its controller may search …").
                        return text;
                    }
                    format!("{} {tail}", base_verb(verb))
                }
                None => text,
            };
            format!("{who} may {}", rest.trim_end())
        }
        // The same instruction for each target of one "target" word.
        Effect::Together(v) => {
            let parts: Vec<String> = v.iter().map(|e| effect(mode, e)).collect();
            // One "target" word with several targets reads the same for each of them.
            let group = mode.groups.iter().find(|g| g.count as usize == v.len());
            let same = parts
                .windows(2)
                .all(|w| plain_targets(&w[0]) == plain_targets(&w[1]));
            match (same, group) {
                (true, Some(g)) => {
                    let n = match (g.min, g.count) {
                        (0, n) => format!("up to {}", num(n.into())),
                        (m, n) if m == n => num(n.into()),
                        (m, n) => format!("{} to {}", num(m.into()), num(n.into())),
                    };
                    let slot = mode.targets.get(g.first as usize);
                    // "Exile each of up to two target artifacts or enchantments": the
                    // one instruction with its target word for all of them.
                    let each = format!(
                        "each of {n} {}",
                        slot.map_or("targets".to_string(), |f| target_filter_plural(
                            f,
                            g.min == 0 || g.min == g.count
                        ))
                    );
                    match replace_first_target(&parts[0], &each) {
                        Some(text) => text,
                        None => format!("{}, for each of {n} targets", plain_targets(&parts[0])),
                    }
                }
                _ => join(parts, "and"),
            }
        }
        Effect::ReturnToBattlefield { what } => {
            format!("return {} to the battlefield", obj(mode, *what))
        }
        Effect::ChooseCards {
            who,
            from,
            cards,
            count,
            must,
            to,
            reveal,
            shuffle,
            optional,
            share_land_type,
        } => {
            let who = player(mode, *who);
            let you = who == "you";
            let their = if you { "your" } else { "their" };
            let n = match count {
                Amount::Fixed(k) => num(*k),
                a => amount(a),
            };
            let one = *count == Amount::Fixed(1);
            let noun = if *from == CardZone::Battlefield {
                let parts: Vec<String> = cards.iter().map(|s| selector(s, !one)).collect();
                if parts.is_empty() {
                    if one { "permanent" } else { "permanents" }.to_string()
                } else {
                    join(parts, "or")
                }
            } else {
                card_list(cards, !one)
            };
            let what = match (one, *must) {
                (true, _) => with_article(&noun),
                (false, true) => format!("{n} {noun}"),
                (false, false) => format!("up to {n} {noun}"),
            };
            let what = if *share_land_type {
                format!("{what} that share a land type")
            } else {
                what
            };
            let it = if one { "it" } else { "them" };
            let s = |base: &str| {
                if you {
                    base.to_string()
                } else if base.ends_with("ch") || base.ends_with("sh") {
                    format!("{base}es")
                } else {
                    format!("{base}s")
                }
            };
            // CR 701.24b: "search, shuffle, then put the card on top" shuffles before the
            // found cards are placed (the engine does so for a search that puts cards
            // into the same library).
            let shuffle_first = *shuffle
                && *from == CardZone::Library
                && matches!(to, CardDest::LibraryTop | CardDest::LibraryBottom);
            let that = if one { "that card" } else { "those cards" };
            let mut out = match from {
                CardZone::Library if *optional => {
                    let mut t = format!("{who} may search {their} library for {what}");
                    if *reveal {
                        t.push_str(&format!(", reveal {it}"));
                    }
                    if shuffle_first {
                        t.push_str(&format!(
                            ", then shuffle and put {that} {}",
                            dest(*to, their)
                        ));
                    } else if *to != CardDest::Stay {
                        t.push_str(&format!(", put {it} {}", dest(*to, their)));
                    }
                    t
                }
                // Choosing among cards found before: no new search.
                CardZone::Library
                    if !cards.is_empty() && cards.iter().all(|c| c.noted == Some(true)) =>
                {
                    if shuffle_first {
                        lead(
                            &who,
                            &format!(
                                "{}, then {} {what} {}",
                                s("shuffle"),
                                s("put"),
                                dest(*to, their)
                            ),
                        )
                    } else {
                        lead(&who, &format!("{} {what} {}", s("put"), dest(*to, their)))
                    }
                }
                CardZone::Library => {
                    let mut t = lead(&who, &format!("{} {their} library for {what}", s("search")));
                    if *reveal {
                        t.push_str(&format!(", {} {it}", s("reveal")));
                    }
                    if shuffle_first {
                        t.push_str(&format!(
                            ", then {} and {} {that} {}",
                            s("shuffle"),
                            s("put"),
                            dest(*to, their)
                        ));
                    } else if *to != CardDest::Stay {
                        t.push_str(&format!(", {} {it} {}", s("put"), dest(*to, their)));
                    }
                    t
                }
                CardZone::Hand | CardZone::Graveyard => {
                    let zone = if *from == CardZone::Hand {
                        "hand"
                    } else {
                        "graveyard"
                    };
                    let rest = format!("{what} from {their} {zone} {}", dest(*to, their));
                    let mut t = if *must {
                        lead(&who, &format!("{} {rest}", s("put")))
                    } else {
                        format!("{who} may put {rest}")
                    };
                    if *reveal {
                        t.push_str(" (revealed)");
                    }
                    t
                }
                CardZone::Battlefield => {
                    let rest = format!("{what} {}", dest(*to, "its owner's"));
                    if *must {
                        lead(&who, &format!("{} {rest}", s("put")))
                    } else {
                        format!("{who} may put {rest}")
                    }
                }
            };
            if *shuffle && !shuffle_first {
                let verb = if *optional {
                    "shuffle".to_string()
                } else {
                    s("shuffle")
                };
                out.push_str(&format!(", then {verb}"));
            }
            out
        }
        Effect::Scry { who, amount: a } => {
            format!(
                "{} {}",
                verb(&player(mode, *who), "scry", "scries"),
                amount(a)
            )
        }
        Effect::Surveil { who, amount: a } => {
            format!(
                "{} {}",
                verb(&player(mode, *who), "surveil", "surveils"),
                amount(a)
            )
        }
        Effect::Dig {
            who,
            count,
            reveal,
            take,
            cards,
            to,
            reveal_taken,
            rest,
            rest_random,
        } => {
            // The effect's controller looks and chooses, whoever's library it is. ("Each
            // player reveals …" has nothing to choose.)
            let each = matches!(who, PlayerRef::Each | PlayerRef::EachOpponent);
            let owner = player(mode, *who);
            // Nothing for the effect's controller to decide: every card that qualifies goes
            // one way, the rest the other (and is not put back in an order of their
            // choosing). Then the library's player is the one who reveals and puts them —
            // and controls what they put onto the battlefield (CR 110.2a).
            let rest_unordered = *rest_random
                || *count == Amount::Fixed(1)
                || !matches!(rest, CardDest::LibraryTop | CardDest::LibraryBottom);
            let theirs = *take == Take::All && rest_unordered && !(*reveal_taken && !*reveal);
            let (who, their) = match (owner.as_str(), each || theirs) {
                ("you", _) => ("you".to_string(), "your".to_string()),
                (_, true) => (owner.clone(), "their".to_string()),
                (_, false) => ("you".to_string(), format!("{owner}'s")),
            };
            let you = who == "you";
            let their = their.as_str();
            let n = match count {
                Amount::Fixed(k) => num(*k),
                a => amount(a),
            };
            let look = match (*reveal, you) {
                (true, true) => "reveal",
                (true, false) => "reveals",
                (false, true) => "look at",
                (false, false) => "looks at",
            };
            let top = match count {
                Amount::Fixed(1) => format!("the top card of {their} library"),
                Amount::Fixed(_) | Amount::X => format!("the top {n} cards of {their} library"),
                Amount::TriggeringAmount => {
                    format!("that many cards from the top of {their} library")
                }
                _ => format!("a number of cards from the top of {their} library equal to {n}"),
            };
            let noun = |plural: bool| card_list(cards, plural);
            let shown = if *reveal_taken && !*reveal {
                "reveal "
            } else {
                ""
            };
            if *take == Take::Exactly(0)
                && *rest == CardDest::LibraryTop
                && !*rest_random
                && !*reveal
            {
                let looked = lead(&who, &format!("{look} {top}"));
                return if *count == Amount::Fixed(1) {
                    looked
                } else {
                    format!("{looked}, then put them back in any order")
                };
            }
            // What is taken, whether that is a choice, and what "it" is afterwards.
            let (may, things, pronoun) = match take {
                Take::Exactly(1) => (false, with_article(&noun(false)), "it"),
                Take::Exactly(k) => (false, format!("{} {}", num(*k), noun(true)), "them"),
                Take::UpTo(1) => (true, with_article(&noun(false)), "it"),
                Take::UpTo(k) => (true, format!("up to {} {}", num(*k), noun(true)), "them"),
                Take::All => (false, format!("all {}", noun(true)), "them"),
                Take::Any => (true, format!("any number of {}", noun(true)), "them"),
            };
            let where_to = dest(*to, their);
            let taken = match (shown.is_empty(), may) {
                (true, true) => format!("you may put {things} from among them {where_to}"),
                (true, false) if you => format!("put {things} from among them {where_to}"),
                (true, false) => format!("{who} puts {things} from among them {where_to}"),
                (false, true) => {
                    format!("you may reveal {things} from among them and put {pronoun} {where_to}")
                }
                (false, false) => {
                    format!("reveal {things} from among them and put {pronoun} {where_to}")
                }
            };
            format!(
                "{}. Then {taken} and the rest {}{}",
                lead(&who, &format!("{look} {top}")),
                dest(*rest, their),
                if *rest_random {
                    " in a random order"
                } else {
                    ""
                }
            )
        }
        Effect::Pay {
            who,
            cost: c,
            paid,
            unpaid,
        } => {
            let nothing = |e: &Effect| matches!(e, Effect::Seq(v) if v.is_empty());
            let who = player(mode, *who);
            let you = who == "you";
            // "pay {2}", "sacrifice a creature", "pay 2 life": as an action of the player.
            let action = |third: bool| {
                let mut parts: Vec<String> = Vec::new();
                for part in cost(c).split(", ") {
                    let text = if part.starts_with('{') {
                        format!("pay {part}")
                    } else {
                        // "Return two Forests …" → "return two Forests …".
                        let mut chars = part.chars();
                        chars
                            .next()
                            .into_iter()
                            .flat_map(|c| c.to_lowercase())
                            .chain(chars)
                            .collect()
                    };
                    parts.push(match (third, text.split_once(' ')) {
                        (true, Some((v, rest))) => format!("{v}s {rest}"),
                        _ => text,
                    });
                }
                join(parts, "and")
            };
            // "… It can't be regenerated" must not read as what the payment avoids.
            if nothing(paid) && matches!(**unpaid, Effect::CantBeRegenerated(_)) {
                let dont = if you { "you don't" } else { "they don't" };
                format!(
                    "{who} may {}. If {dont}, {}",
                    action(false),
                    effect(mode, unpaid)
                )
            } else if nothing(paid) {
                format!("{} unless {who} {}", effect(mode, unpaid), action(!you))
            } else {
                let does = if you { "you do" } else { "they do" };
                let mut out = format!(
                    "{who} may {}. If {does}, {}",
                    action(false),
                    effect(mode, paid)
                );
                if !nothing(unpaid) {
                    out.push_str(&format!(". Otherwise, {}", effect(mode, unpaid)));
                }
                out
            }
        }
        Effect::ThisSpellTo { to, shuffle } => {
            if *shuffle {
                "shuffle this spell into its owner's library".to_string()
            } else if *to == CardDest::Exile {
                "exile this spell".to_string()
            } else {
                format!("put this spell {}", dest(*to, "its owner's"))
            }
        }
        Effect::PutInto {
            what,
            to,
            shuffle,
            controller,
        } => {
            if *shuffle {
                format!("shuffle {} into its owner's library", obj(mode, *what))
            } else {
                let under = match controller {
                    None => String::new(),
                    Some(PlayerRef::You) => " under your control".to_string(),
                    Some(p) => format!(" under the control of {}", player(mode, *p)),
                };
                format!(
                    "put {} {}{under}",
                    obj(mode, *what),
                    dest(*to, "its owner's")
                )
            }
        }
        // What the instruction does is what it says; the instructions after it say
        // "those cards".
        Effect::Note(inner) => effect(mode, inner),
        Effect::If { test, then } => format!("{}, {}", condition(test), effect(mode, then)),
        Effect::MoveAll {
            from,
            whose,
            cards,
            to,
            shuffle,
            controller,
        } => {
            let whose_text = match whose {
                PlayerRef::Each => String::new(),
                PlayerRef::You => "your".to_string(),
                other => format!("{}'s", player(mode, *other)),
            };
            let what = match from {
                CardZone::Battlefield => {
                    let parts: Vec<String> = cards.iter().map(|s| selector(s, true)).collect();
                    let list = if parts.is_empty() {
                        "permanents".to_string()
                    } else {
                        join(parts, "and")
                    };
                    match whose {
                        PlayerRef::Each => format!("all {list}"),
                        PlayerRef::You => format!("all {list} you control"),
                        other => format!("all {list} {} controls", player(mode, *other)),
                    }
                }
                zone => {
                    let zone = match zone {
                        CardZone::Library => "library",
                        CardZone::Hand => "hand",
                        _ => "graveyard",
                    };
                    // "All land cards from your graveyard": whose they are is the zone's.
                    let list = if whose_text.is_empty() {
                        card_list(cards, true)
                    } else {
                        card_list(cards, true)
                            .replace(" you own", "")
                            .replace(" an opponent owns", "")
                    };
                    if whose_text.is_empty() {
                        format!("all {list} from all {zone}s")
                    } else {
                        format!("all {list} from {whose_text} {zone}")
                    }
                }
            };
            let mut out = match to {
                CardDest::Hand => format!("return {what} to their owners' hands"),
                CardDest::Exile => format!("exile {what}"),
                CardDest::Battlefield { tapped: false } => {
                    format!("put {what} onto the battlefield")
                }
                CardDest::Battlefield { tapped: true } => {
                    format!("put {what} onto the battlefield tapped")
                }
                CardDest::Graveyard => format!("put {what} into their owners' graveyards"),
                CardDest::LibraryTop if *shuffle => {
                    format!("shuffle {what} into their owners' libraries")
                }
                CardDest::LibraryTop => format!("put {what} on top of their owners' libraries"),
                CardDest::LibraryBottom => {
                    format!("put {what} on the bottom of their owners' libraries")
                }
                CardDest::Stay => format!("leave {what} where they are"),
            };
            if let Some(p) = controller {
                out.push_str(&format!(" under {} control", possessive(&player(mode, *p))));
            }
            if *shuffle && *to != CardDest::LibraryTop {
                out.push_str(", then those players shuffle");
            }
            out
        }
    }
}

/// "your", "target player's".
fn possessive(who: &str) -> String {
    match who {
        "you" => "your".to_string(),
        other => format!("{other}'s"),
    }
}

/// "into your hand", "onto the battlefield tapped", "on top of their library".
fn dest(to: CardDest, whose: &str) -> String {
    match to {
        CardDest::Hand => format!("into {whose} hand"),
        CardDest::Battlefield { tapped: false } => "onto the battlefield".to_string(),
        CardDest::Battlefield { tapped: true } => "onto the battlefield tapped".to_string(),
        CardDest::Graveyard => format!("into {whose} graveyard"),
        CardDest::Exile => "into exile".to_string(),
        CardDest::LibraryTop => format!("on top of {whose} library"),
        CardDest::LibraryBottom => format!("on the bottom of {whose} library"),
        CardDest::Stay => "where they are".to_string(),
    }
}

/// Cards in a hand, library or graveyard: "basic land card", "creature cards with mana
/// value 3 or less", "card named Test Elf", "artifact or enchantment cards".
fn card_list(cards: &[Selector], plural: bool) -> String {
    // "A land card with a basic land type" (CR 205.3i): any of the five.
    card_list_words(cards, plural)
        .replace(
            "Plains or Island or Swamp or Mountain or Forest land cards",
            "land cards with a basic land type",
        )
        .replace(
            "Plains or Island or Swamp or Mountain or Forest land card",
            "land card with a basic land type",
        )
}

fn card_list_words(cards: &[Selector], plural: bool) -> String {
    let noun = if plural { "cards" } else { "card" };
    if cards.is_empty() {
        return noun.to_string();
    }
    // Selectors that differ only in an exact mana value are one: "creature card with
    // mana value 4, 5 or 6".
    // And ones that differ only in their card types are one ("creature or land card"),
    // or only in their colors ("white or blue card"): each list means "at least one of".
    let mut merged: Vec<Selector> = Vec::new();
    for s in cards {
        let no_types = |s: &Selector| Selector {
            types: Vec::new(),
            ..s.clone()
        };
        let no_colors = |s: &Selector| Selector {
            colors: Vec::new(),
            ..s.clone()
        };
        if let Some(m) = merged
            .iter_mut()
            .find(|m| !m.types.is_empty() && !s.types.is_empty() && no_types(m) == no_types(s))
        {
            for t in &s.types {
                if !m.types.contains(t) {
                    m.types.push(*t);
                }
            }
        } else if let Some(m) = merged
            .iter_mut()
            .find(|m| !m.colors.is_empty() && !s.colors.is_empty() && no_colors(m) == no_colors(s))
        {
            for c in &s.colors {
                if !m.colors.contains(c) {
                    m.colors.push(*c);
                }
            }
        } else {
            merged.push(s.clone());
        }
    }
    let mut groups: Vec<(Selector, Vec<u32>)> = Vec::new();
    for s in &merged {
        match (s.mana_value_min, s.mana_value_max) {
            (Some(a), Some(b)) if a == b => {
                let base = Selector {
                    mana_value_min: None,
                    mana_value_max: None,
                    ..s.clone()
                };
                match groups.iter_mut().find(|(g, v)| *g == base && !v.is_empty()) {
                    Some((_, values)) => values.push(a),
                    None => groups.push((base, alloc::vec![a])),
                }
            }
            _ => groups.push((s.clone(), Vec::new())),
        }
    }
    let parts: Vec<String> = groups
        .iter()
        .map(|(s, values)| {
            let mut words: Vec<String> = Vec::new();
            if s.other {
                words.push(if plural { "other" } else { "another" }.to_string());
            }
            if !s.colors.is_empty() {
                words.push(join(
                    s.colors.iter().map(|c| color(*c).to_string()).collect(),
                    "or",
                ));
            }
            if s.not_colors.len() == 5 {
                words.push("colorless".to_string());
            } else {
                words.extend(s.not_colors.iter().map(|c| format!("non{}", color(*c))));
            }
            words.extend(s.supertypes.iter().map(|t| format!("{t:?}").to_lowercase()));
            words.extend(s.not_subtypes.iter().map(|t| format!("non-{t}")));
            // "Permanent card" (CR 110.4a): neither instant nor sorcery.
            let permanent_card = s.types.is_empty()
                && s.not_types.len() == 2
                && s.not_types.contains(&CardType::Instant)
                && s.not_types.contains(&CardType::Sorcery);
            if !permanent_card {
                words.extend(s.not_types.iter().map(|t| format!("non{}", card_type(*t))));
            }
            if !s.subtypes.is_empty() {
                words.push(s.subtypes.join(" or "));
            }
            // "Dragon permanent card".
            if permanent_card {
                words.push("permanent".to_string());
            }
            if !s.types.is_empty() {
                words.push(types(&s.types, "or"));
            }
            match s.token {
                Some(true) => words.push("token".to_string()),
                Some(false) => words.insert(0, "nontoken".to_string()),
                None => {}
            }
            words.push(noun.to_string());
            let mut out = words.join(" ");
            for k in &s.keywords {
                out.push_str(&format!(" with {}", keyword(*k).to_lowercase()));
            }
            out.push_str(&power_toughness(s));
            if values.is_empty() {
                out.push_str(&mana_value(s));
            } else {
                out.push_str(&format!(
                    " with mana value {}",
                    join(values.iter().map(|v| v.to_string()).collect(), "or")
                ));
            }
            if !s.names.is_empty() {
                out.push_str(&format!(" named {}", s.names.join(" or ")));
            }
            match s.has_adventure {
                Some(true) if plural => out.push_str(" that have an Adventure"),
                Some(true) => out.push_str(" that has an Adventure"),
                Some(false) => out.push_str(" without an Adventure"),
                None => {}
            }
            out.push_str(noted(s.noted));
            match s.controller {
                Who::Any => {}
                Who::You => out.push_str(" you own"),
                Who::Opponent => out.push_str(" an opponent owns"),
            }
            out.push_str(of_chosen(s, plural));
            out
        })
        .collect();
    join(parts, "or")
}

/// " among those cards" / " other than those cards" (empty if the selector doesn't ask).
fn noted(n: Option<bool>) -> &'static str {
    match n {
        None => "",
        Some(true) => " among those cards",
        Some(false) => " other than those cards",
    }
}

/// " with power 2 or less and toughness 3 or greater": every bound the selector sets
/// (empty if it sets none).
fn power_toughness(s: &Selector) -> String {
    let mut bounds: Vec<String> = Vec::new();
    for (name, min, max) in [
        ("power", s.power_min, s.power_max),
        ("toughness", s.toughness_min, s.toughness_max),
    ] {
        match (min, max) {
            (Some(a), Some(b)) if a == b => bounds.push(format!("{name} {a}")),
            (Some(a), Some(b)) => bounds.push(format!("{name} {a} to {b}")),
            (Some(a), None) => bounds.push(format!("{name} {a} or greater")),
            (None, Some(b)) => bounds.push(format!("{name} {b} or less")),
            (None, None) => {}
        }
    }
    if bounds.is_empty() {
        String::new()
    } else {
        format!(" with {}", join(bounds, "and"))
    }
}

/// " with mana value 3 or less" (empty if the selector doesn't ask).
fn mana_value(s: &Selector) -> String {
    if let Some(a) = &s.mana_value_at_most {
        return match &**a {
            Amount::X => " with mana value X or less".to_string(),
            a => format!(" with mana value less than or equal to {}", amount(a)),
        };
    }
    match (s.mana_value_min, s.mana_value_max) {
        (None, None) => String::new(),
        (Some(a), Some(b)) if a == b => format!(" with mana value {a}"),
        (Some(a), Some(b)) => format!(" with mana value {a} to {b}"),
        (Some(a), None) => format!(" with mana value {a} or greater"),
        (None, Some(b)) => format!(" with mana value {b} or less"),
    }
}

fn sentence(mut s: String) -> String {
    if let Some(first) = s.chars().next() {
        let upper: String = first.to_uppercase().collect();
        s.replace_range(..first.len_utf8(), &upper);
    }
    if !s.ends_with('.') {
        s.push('.');
    }
    s
}

fn body(b: &Body) -> String {
    match b {
        Body::Effect(m) => sentence(mode_effect(m)),
        Body::Modal { min, max, modes } => {
            let (min, max) = (u32::from(*min), u32::from(*max));
            let n = if min == max {
                format!("Choose {}", num(min))
            } else if min == 1 && max as usize == modes.len() && modes.len() == 2 {
                "Choose one or both".to_string()
            } else if min == 1 && max as usize == modes.len() {
                "Choose one or more".to_string()
            } else if min == 0 {
                format!("Choose up to {}", num(max))
            } else {
                format!("Choose {} to {}", num(min), num(max))
            };
            // One bullet for each mode, as the card prints them.
            let list: Vec<String> = modes
                .iter()
                .map(|m| format!("• {}", sentence(mode_effect(m))))
                .collect();
            format!("{n} — {}", list.join(" "))
        }
    }
}

/// "creature card", "Zombie card", "card".
fn card_noun(s: &Selector) -> String {
    let mut words: Vec<String> = Vec::new();
    if !s.subtypes.is_empty() {
        words.push(join(s.subtypes.clone(), "or"));
    }
    if !s.types.is_empty() {
        words.push(types(&s.types, "or"));
    }
    words.push("card".into());
    words.join(" ") + of_chosen(s, false)
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn cost(c: &Cost) -> String {
    cost_parts(c).join(", ")
}

/// A card in a hand by what is asked of it: "a green card", "a nonland card", "an Island
/// card".
fn hand_card_noun(s: &Selector) -> String {
    let mut words: Vec<String> = Vec::new();
    for c in &s.not_colors {
        words.push(format!("non{}", color(*c)));
    }
    if !s.colors.is_empty() {
        words.push(join(
            s.colors.iter().map(|c| color(*c).to_string()).collect(),
            "or",
        ));
    }
    for t in &s.not_types {
        words.push(format!("non{}", card_type(*t)));
    }
    words.push(card_noun(s));
    words.join(" ")
}

/// The parts of a cost, each as an instruction ("Sacrifice a creature", "Pay 2 life").
fn cost_parts(c: &Cost) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(m) = &c.mana {
        parts.push(m.to_text());
    }
    if c.tap {
        parts.push("{T}".to_string());
    }
    if c.sacrifice_self {
        parts.push("Sacrifice this permanent".to_string());
    }
    if let Some((kind, n)) = c.add_counters {
        parts.push(format!(
            "Put {} on this permanent",
            counters_phrase(&Amount::Fixed(n), kind)
        ));
    }
    if let Some((kind, n)) = c.remove_counters {
        parts.push(format!(
            "Remove {} from this permanent",
            counters_phrase(&Amount::Fixed(n), kind)
        ));
    }
    let chosen = |c: &crate::CostChoice, cards: bool| {
        let what: Vec<String> = c
            .what
            .iter()
            .map(|s| {
                if cards {
                    card_noun(s)
                } else {
                    selector(s, c.count != 1)
                }
            })
            .collect();
        if c.count == 1 {
            with_article(&join(what, "or"))
        } else if cards {
            // "two creature cards", "two creature cards or land cards".
            let what = c
                .what
                .iter()
                .map(|s| format!("{}s", card_noun(s)))
                .collect();
            format!("{} {}", num(c.count), join(what, "or"))
        } else {
            format!("{} {}", num(c.count), join(what, "or"))
        }
    };
    if let Some(s) = &c.sacrifice {
        parts.push(format!("Sacrifice {}", chosen(s, false)));
    }
    if let Some(d) = &c.discard {
        parts.push(format!("Discard {}", chosen(d, true)));
    }
    if c.life > 0 {
        parts.push(format!("Pay {} life", c.life));
    }
    if c.life_x {
        parts.push("Pay X life".to_string());
    }
    if c.discard_self {
        parts.push("Discard this card".to_string());
    }
    if c.exile_self {
        parts.push("Exile this card from your graveyard".to_string());
    }
    if let Some(e) = &c.exile_from_hand {
        let what = join(e.what.iter().map(hand_card_noun).collect(), "or");
        parts.push(if e.count == 1 {
            format!("Exile {} from your hand", with_article(&what))
        } else {
            format!("Exile {} {what}s from your hand", number_word(e.count))
        });
    }
    if let Some(t) = &c.tap_others {
        // "Tap another untapped Citizen you control".
        let other = t.what.iter().all(|s| s.other);
        let what: Vec<String> = t
            .what
            .iter()
            .map(|s| {
                let mut s = s.clone();
                s.controller = Who::You;
                s.other = false;
                selector(&s, t.count != 1)
            })
            .collect();
        let what = join(what, "or");
        parts.push(match (t.count, other) {
            (1, false) => format!("Tap an untapped {what}"),
            (1, true) => format!("Tap another untapped {what}"),
            (n, false) => format!("Tap {} untapped {what}", num(n)),
            (n, true) => format!("Tap {} other untapped {what}", num(n)),
        });
    }
    if let Some(n) = c.crew {
        parts.push(format!(
            "Tap any number of other untapped creatures you control with total power {n} or greater"
        ));
    }
    if let Some(r) = &c.return_to_hand {
        let mut what: Vec<String> = Vec::new();
        for s in &r.what {
            let mut s = s.clone();
            s.controller = Who::You;
            what.push(selector(&s, r.count != 1));
        }
        let what = join(what, "or");
        parts.push(if r.count == 1 {
            format!("Return {} to its owner's hand", with_article(&what))
        } else {
            format!(
                "Return {} {what} to their owner's hand",
                number_word(r.count)
            )
        });
    }
    parts
}

/// A cost as words inside a sentence: "pay {U}", "exile a green card from your hand",
/// "pay {1} and return a basic land you control to its owner's hand".
fn cost_words(c: &Cost) -> String {
    let mut parts = Vec::new();
    if let Some(m) = &c.mana {
        parts.push(format!("pay {}", m.to_text()));
    }
    let rest = Cost {
        mana: None,
        ..c.clone()
    };
    for p in cost_parts(&rest) {
        let mut chars = p.chars();
        let first: String = chars
            .next()
            .into_iter()
            .flat_map(|c| c.to_lowercase())
            .collect();
        parts.push(first + chars.as_str());
    }
    join(parts, "and")
}

/// "Flashback {2}{R}", "Evoke—Exile a white card from your hand".
fn keyword_cost(name: &str, c: &Cost) -> String {
    let rest = Cost {
        mana: None,
        ..c.clone()
    };
    match (&c.mana, rest == Cost::default()) {
        (Some(m), true) => format!("{name} {}", m.to_text()),
        (None, true) => format!("{name} {{0}}"),
        // "Flashback—{1}{G}, Pay 3 life", "Evoke—Exile a white card from your hand".
        _ => format!("{name}\u{2014}{}", cost(c)),
    }
}

fn alt_cost(a: &crate::AltCost) -> String {
    match &a.kind {
        crate::AltKind::Rather => {
            let free = a.cost == Cost::default();
            let text = if free {
                "you may cast this spell without paying its mana cost".to_string()
            } else {
                format!(
                    "you may {} rather than pay this spell's mana cost",
                    cost_words(&a.cost)
                )
            };
            match &a.only_if {
                Some(c) => format!("{}, {text}.", capitalize(&condition(c))),
                None => format!("{}.", capitalize(&text)),
            }
        }
        crate::AltKind::Flashback => keyword_cost("Flashback", &a.cost),
        crate::AltKind::Evoke => keyword_cost("Evoke", &a.cost),
        crate::AltKind::Overload(_) => keyword_cost("Overload", &a.cost),
    }
}

fn who_possessive(w: Who) -> &'static str {
    match w {
        Who::Any => "each player's",
        Who::You => "your",
        Who::Opponent => "each opponent's",
    }
}

fn obj_filter(f: &ObjFilter) -> String {
    match f {
        ObjFilter::This => "this permanent".into(),
        ObjFilter::Matching(s) => {
            let text = selector(s, false);
            if s.other { text } else { with_article(&text) }
        }
        ObjFilter::ThisOr(s) => format!("this permanent or another {}", selector(s, false)),
        ObjFilter::Host => "enchanted or equipped permanent".into(),
        ObjFilter::Remembered => "that permanent".into(),
    }
}

/// "When this creature enters" (it happens once), "Whenever a creature enters".
fn trigger(t: &Trigger) -> String {
    let text = trigger_event(t);
    // Entering, dying and leaving happen to the object once: "When this creature enters
    // or dies".
    fn once(t: &Trigger) -> bool {
        match t {
            Trigger::Any(v) => v.iter().all(once),
            t => matches!(
                t,
                Trigger::Enters(ObjFilter::This)
                    | Trigger::Dies(ObjFilter::This)
                    | Trigger::Leaves(ObjFilter::This)
            ),
        }
    }
    let once = once(t);
    match text.strip_prefix("Whenever ") {
        Some(rest) if once => format!("When {rest}"),
        _ => text,
    }
}

fn trigger_event(t: &Trigger) -> String {
    match t {
        Trigger::Attacks(f) => format!("Whenever {} attacks", obj_filter(f)),
        Trigger::AttacksAlone(f) => format!("Whenever {} attacks alone", obj_filter(f)),
        Trigger::PlayerAttacks { by, with, at_least } => {
            let who = match by {
                Who::Any => "a player attacks",
                Who::You => "you attack",
                Who::Opponent => "an opponent attacks",
            };
            match (with, at_least) {
                (None, 0 | 1) => format!("Whenever {who}"),
                (None, n) => format!("Whenever {who} with {} or more creatures", num(*n)),
                (Some(s), 0 | 1) => {
                    format!("Whenever {who} with one or more {}", selector(s, true))
                }
                (Some(s), n) => format!(
                    "Whenever {who} with {} or more {}",
                    num(*n),
                    selector(s, true)
                ),
            }
        }
        Trigger::GainsLife(who) => match who {
            Who::Any => "Whenever a player gains life".to_string(),
            Who::You => "Whenever you gain life".to_string(),
            Who::Opponent => "Whenever an opponent gains life".to_string(),
        },
        Trigger::LosesLife(who) => match who {
            Who::Any => "Whenever a player loses life".to_string(),
            Who::You => "Whenever you lose life".to_string(),
            Who::Opponent => "Whenever an opponent loses life".to_string(),
        },
        Trigger::Draws { who, nth } => {
            let who_draws = match who {
                Who::Any => "a player draws",
                Who::You => "you draw",
                Who::Opponent => "an opponent draws",
            };
            match nth {
                None => format!("Whenever {who_draws} a card"),
                // "Whenever you draw your second card each turn".
                Some(n) => format!(
                    "Whenever {who_draws} {} {} card each turn",
                    if *who == Who::You { "your" } else { "their" },
                    ordinal(*n)
                ),
            }
        }
        Trigger::Discards(who) => match who {
            Who::Any => "Whenever a player discards a card".to_string(),
            Who::You => "Whenever you discard a card".to_string(),
            Who::Opponent => "Whenever an opponent discards a card".to_string(),
        },
        Trigger::Sacrifices { who, what } => format!(
            "Whenever {} {}",
            match who {
                Who::Any => "a player sacrifices",
                Who::You => "you sacrifice",
                Who::Opponent => "an opponent sacrifices",
            },
            obj_filter(what)
        ),
        Trigger::Blocks(f) => format!("Whenever {} blocks", obj_filter(f)),
        Trigger::BecomesTapped(f) => format!("Whenever {} becomes tapped", obj_filter(f)),
        // CR 715.2b.
        Trigger::Chapter(n) => format!(
            "When one or more lore counters are put onto this Saga, if the number of lore \
             counters on it was less than {n} and became at least {n}"
        ),
        Trigger::BecomesBlocked(f) => format!("Whenever {} becomes blocked", obj_filter(f)),
        Trigger::Unblocked(f) => format!("Whenever {} attacks and isn't blocked", obj_filter(f)),
        Trigger::BlockedBy { attacker, blocker } => format!(
            "Whenever {} blocks {}",
            obj_filter(blocker),
            obj_filter(attacker)
        ),
        Trigger::Cycles { this: true, .. } => "When you cycle this card".to_string(),
        Trigger::Cycles { by, this: false } => format!(
            "Whenever {} a card",
            match by {
                Who::Any => "a player cycles",
                Who::You => "you cycle",
                Who::Opponent => "an opponent cycles",
            }
        ),
        Trigger::Any(v) => {
            // "Whenever this permanent enters or attacks".
            let parts: Vec<String> = v.iter().map(trigger_event).collect();
            let mut out = parts.first().cloned().unwrap_or_default();
            for p in parts.iter().skip(1) {
                // What differs from the first event, after the words they share ("this
                // permanent enters or deals combat damage to a player").
                let first: Vec<&str> = out.split(' ').collect();
                let words: Vec<&str> = p.split(' ').collect();
                let shared = first
                    .iter()
                    .zip(&words)
                    .take_while(|(a, b)| a == b)
                    .count()
                    .max(1);
                out.push_str(&format!(" or {}", words[shared..].join(" ")));
            }
            out
        }
        Trigger::Enters(f) => format!("Whenever {} enters", obj_filter(f)),
        Trigger::Dies(f) => format!("Whenever {} dies", obj_filter(f)),
        Trigger::Leaves(f) => format!("Whenever {} leaves the battlefield", obj_filter(f)),
        Trigger::Damage {
            by,
            to,
            combat,
            per,
        } => {
            let kind = match combat {
                Some(true) => "combat damage",
                Some(false) => "noncombat damage",
                None => "damage",
            };
            let whom = match to {
                DamageTo::Anything => String::new(),
                DamageTo::Player(Who::Any) => " to a player".to_string(),
                DamageTo::Player(Who::You) => " to you".to_string(),
                DamageTo::Player(Who::Opponent) => " to an opponent".to_string(),
                DamageTo::Permanent(f) => format!(" to {}", obj_filter(f)),
                DamageTo::Either { .. } => format!(" to {}", damaged(to)),
            };
            match (by, per) {
                // "Whenever this creature is dealt damage".
                (None, _) => match to {
                    DamageTo::Permanent(f) => format!("Whenever {} is dealt {kind}", obj_filter(f)),
                    DamageTo::Player(Who::You) => format!("Whenever you are dealt {kind}"),
                    DamageTo::Player(Who::Opponent) => {
                        format!("Whenever an opponent is dealt {kind}")
                    }
                    DamageTo::Player(Who::Any) => format!("Whenever a player is dealt {kind}"),
                    DamageTo::Anything => format!("Whenever {kind} is dealt"),
                    DamageTo::Either { .. } => {
                        format!("Whenever {} is dealt {kind}", damaged(to))
                    }
                },
                (Some(ObjFilter::Matching(s)), DamagePer::Recipient) => format!(
                    "Whenever one or more {} deal {kind}{whom}",
                    selector(s, true)
                ),
                (Some(f), _) => format!("Whenever {} deals {kind}{whom}", obj_filter(f)),
            }
        }
        // CR 511.2: "at end of combat" (as the end of combat step begins).
        Trigger::BeginningOf {
            step: TriggerStep::EndCombat,
            whose,
        } => match whose {
            Who::Any => "At end of combat".to_string(),
            Who::You => "At end of combat on your turn".to_string(),
            Who::Opponent => "At end of combat on each opponent's turn".to_string(),
        },
        Trigger::BeginningOf {
            step: TriggerStep::BeginCombat,
            whose,
        } => match whose {
            Who::Any => "At the beginning of each combat".to_string(),
            Who::You => "At the beginning of combat on your turn".to_string(),
            Who::Opponent => "At the beginning of combat on each opponent's turn".to_string(),
        },
        Trigger::BeginningOf { step, whose } => format!(
            "At the beginning of {} {}",
            who_possessive(*whose),
            match step {
                TriggerStep::Upkeep => "upkeep",
                TriggerStep::DrawStep => "draw step",
                TriggerStep::EndStep => "end step",
                TriggerStep::PrecombatMain => "precombat main phase",
                TriggerStep::PostcombatMain => "postcombat main phase",
                TriggerStep::BeginCombat | TriggerStep::EndCombat => "combat",
            }
        ),
        Trigger::CastThis => "When you cast this spell".to_string(),
        Trigger::Cast {
            by,
            types: ts,
            spells,
            nth,
            copies,
        } => {
            // "a creature or artifact spell", "a noncreature spell", "a Spirit spell".
            let mut kinds: Vec<String> = Vec::new();
            if !ts.is_empty() {
                kinds.push(types(ts, "or"));
            }
            if !spells.is_empty() {
                kinds.push(as_spells(&card_list(spells, false)));
            }
            let what = match kinds.as_slice() {
                [] => "spell".to_string(),
                _ if spells.is_empty() => format!("{} spell", kinds[0]),
                _ if ts.is_empty() => join(kinds, "or"),
                _ => format!("{} spell or {}", kinds[0], join(kinds[1..].to_vec(), "or")),
            };
            match nth {
                // "Whenever you cast or copy an instant or sorcery spell" (magecraft).
                None if *copies => format!(
                    "Whenever {} {}",
                    match by {
                        Who::Any => "a player casts or copies",
                        Who::You => "you cast or copy",
                        Who::Opponent => "an opponent casts or copies",
                    },
                    with_article(&what)
                ),
                None => format!(
                    "Whenever {} {}",
                    match by {
                        Who::Any => "a player casts",
                        Who::You => "you cast",
                        Who::Opponent => "an opponent casts",
                    },
                    with_article(&what)
                ),
                // "Whenever an opponent casts their first noncreature spell each turn".
                Some(n) => format!(
                    "Whenever {} {} {what} each turn",
                    match by {
                        Who::Any => "a player casts their",
                        Who::You => "you cast your",
                        Who::Opponent => "an opponent casts their",
                    },
                    match n {
                        1 => "first".to_string(),
                        2 => "second".to_string(),
                        3 => "third".to_string(),
                        n => format!("{n}th"),
                    }
                ),
            }
        }
    }
}

/// "activated or triggered", "triggered", "activated".
fn ability_kind(activated: bool, triggered: bool) -> &'static str {
    match (activated, triggered) {
        (true, true) => "activated or triggered",
        (false, true) => "triggered",
        _ => "activated",
    }
}

/// "creature card with mana value 4" -> "creature spell with mana value 4".
fn as_spells(cards: &str) -> String {
    cards
        .split(' ')
        .map(|w| match w {
            "card" => "spell".to_string(),
            "cards" => "spells".to_string(),
            "card," => "spell,".to_string(),
            "cards," => "spells,".to_string(),
            w => w.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn condition(c: &Condition) -> String {
    match c {
        Condition::Compare { amount: a, cmp, n } => {
            let what = amount(a);
            match cmp {
                Cmp::AtLeast => format!("if {what} is {n} or more"),
                Cmp::AtMost if *n == 0 => format!("if {what} is 0"),
                Cmp::AtMost => format!("if {what} is {n} or less"),
                Cmp::Exactly => format!("if {what} is exactly {n}"),
                Cmp::Not => format!("if {what} is not {n}"),
            }
        }
        Condition::LifeAtMost(n) => format!("if you have {n} or less life"),
        Condition::LifeAtLeast(n) => format!("if you have {n} or more life"),
        Condition::CardsInGraveyard { cards, at_least } => format!(
            "if you have {} or more {}s in your graveyard",
            num(*at_least),
            card_noun(cards)
        ),
        Condition::YourTurn => "if it's your turn".to_string(),
        Condition::NotYourTurn => "if it's not your turn".to_string(),
        Condition::ControlAtMost { what, at_most } => {
            let there = if what.controller == Who::You {
                "you control"
            } else {
                "there are"
            };
            let things = selector(what, true).replace(" you control", "");
            if *at_most == 0 {
                format!("if {there} no {things}")
            } else {
                format!("if {there} {} or fewer {things}", num(*at_most))
            }
        }
        Condition::HandAtMost(0) => "if you have no cards in hand".to_string(),
        Condition::HandAtMost(n) => format!("if you have {} or fewer cards in hand", num(*n)),
        Condition::Control { what, at_least } => {
            // "you control a Swamp" / "an opponent controls an Island" / "there is a Wall".
            let mut bare = what.clone();
            bare.controller = Who::Any;
            let (things, plural) = if *at_least == 1 {
                (with_article(&selector(&bare, false)), false)
            } else {
                (
                    format!("{} or more {}", num(*at_least), selector(&bare, true)),
                    true,
                )
            };
            match what.controller {
                Who::You => format!("if you control {things}"),
                // Counted over all of them together.
                Who::Opponent if plural => format!("if your opponents control {things}"),
                Who::Opponent => format!("if an opponent controls {things}"),
                Who::Any if plural => format!("if there are {things} on the battlefield"),
                Who::Any => format!("if there is {things} on the battlefield"),
            }
        }
        Condition::ControlCommander { own: false } => "if you control a commander".to_string(),
        Condition::ControlCommander { own: true } => "if you control your commander".to_string(),
        Condition::CastFor(crate::AltKindName::Evoke) => "if its evoke cost was paid".to_string(),
    }
}

/// "unless you control two or fewer lands" for a land that enters: the lands it counts
/// are the *other* ones, since it is not on the battlefield yet.
fn counting_others(c: &Condition) -> Condition {
    let plain_lands = |s: &Selector| {
        s.types == [CardType::Land]
            && s.subtypes.is_empty()
            && s.supertypes.is_empty()
            && s.not_supertypes.is_empty()
            && !s.other
    };
    match c {
        Condition::ControlAtMost { what, at_most }
            if plain_lands(what) && what.controller == Who::You =>
        {
            Condition::ControlAtMost {
                what: Selector {
                    other: true,
                    ..what.clone()
                },
                at_most: *at_most,
            }
        }
        Condition::Control { what, at_least }
            if plain_lands(what) && what.controller == Who::You =>
        {
            Condition::Control {
                what: Selector {
                    other: true,
                    ..what.clone()
                },
                at_least: *at_least,
            }
        }
        c => c.clone(),
    }
}

/// What a player may do as a permanent enters: "pay 2 life", "reveal an Island card from
/// your hand".
fn as_enters(act: &AsEnters) -> String {
    match act {
        AsEnters::PayLife(n) => format!("pay {n} life"),
        AsEnters::Reveal(s) => format!("reveal {} from your hand", with_article(&card_noun(s))),
        AsEnters::Discard(s) => format!("discard {}", with_article(&card_noun(s))),
    }
}

fn ability(a: &AbilityDef) -> String {
    match a {
        AbilityDef::Spell(b) => body(b),
        // CR 702.29a: "Cycling [cost]" means "[Cost], Discard this card: Draw a card."
        AbilityDef::Activated {
            cost: c,
            body: b,
            cycling: true,
            ..
        } if body(b) == "Draw a card." => {
            let rest = cost(&Cost {
                discard_self: false,
                ..c.clone()
            });
            if c.mana.is_some() && c.life == 0 && c.discard.is_none() && c.sacrifice.is_none() {
                format!("Cycling {rest}")
            } else {
                format!("Cycling—{rest}")
            }
        }
        // CR 702.122a: "Crew N" means "Tap any number of other untapped creatures you
        // control with total power N or greater: This permanent becomes an artifact creature
        // until end of turn."
        AbilityDef::Activated {
            cost: Cost { crew: Some(n), .. },
            body:
                Body::Effect(Mode {
                    effect:
                        Effect::Apply {
                            what: ObjRef::This,
                            effects,
                            until_end_of_turn: true,
                        },
                    targets,
                    ..
                }),
            timing: Timing::Instant,
            only_if: None,
            from: ActivationZone::Battlefield,
            reduce: None,
            ..
        } if targets.is_empty()
            && matches!(effects.as_slice(), [ContinuousEffect::AddTypes {
                card_types,
                subtypes,
                land_types,
            }] if subtypes.is_empty()
                && land_types.is_empty()
                && card_types.as_slice() == [CardType::Artifact, CardType::Creature]) =>
        {
            format!("Crew {n}")
        }
        AbilityDef::Activated {
            cost: c,
            timing,
            only_if,
            body: b,
            from,
            cycling: _,
            reduce,
        } => format!(
            "{}: {}{}{}{}",
            cost(c),
            // Where the ability works from, unless its cost already says (CR 113.6).
            match from {
                ActivationZone::Graveyard if !c.exile_self => {
                    let text = body(b);
                    if text.contains("this card") {
                        text.replace("this card", "this card from your graveyard")
                    } else {
                        format!("{text} Activate only if this card is in your graveyard.")
                    }
                }
                ActivationZone::Hand if !c.discard_self =>
                    format!("{} Activate only if this card is in your hand.", body(b)),
                _ => body(b),
            },
            match timing {
                Timing::Instant => "",
                Timing::Sorcery => " Activate only as a sorcery.",
            },
            only_if.as_ref().map_or(String::new(), |c| format!(
                " Activate only {}.",
                condition(c)
            )),
            // CR 602.2b, 601.2f: "This ability costs {1} less to activate for each …".
            match reduce {
                None => String::new(),
                Some(Amount::Count(s)) => {
                    let each = selector(s, false);
                    let each = each
                        .strip_prefix("another ")
                        .map_or(each.clone(), |rest| format!("other {rest}"));
                    format!(" This ability costs {{1}} less to activate for each {each}.")
                }
                Some(a) => format!(
                    " This ability costs {{X}} less to activate, where X is {}.",
                    amount(a)
                ),
            }
        ),
        AbilityDef::Mana {
            cost: c,
            produce,
            only_if,
            spend,
            then,
        } => format!(
            "{}: Add {}.{}{}{}",
            cost(c),
            match produce {
                ManaProduce::One(t) => t.symbol().to_string(),
                ManaProduce::AnyColor => "one mana of any color".to_string(),
                ManaProduce::CommanderIdentity =>
                    "one mana of any color in your commander's color identity".to_string(),
                ManaProduce::OneOf(of) =>
                    join(of.iter().map(|t| t.symbol().to_string()).collect(), "or",),
                ManaProduce::Times { of, count } => match of.as_slice() {
                    [t] => t.symbol().repeat(*count as usize),
                    _ if of.len() == 5 => format!("{count} mana of any one color"),
                    _ => format!(
                        "{count} mana of one of {}",
                        join(of.iter().map(|t| t.symbol().to_string()).collect(), "or")
                    ),
                },
                ManaProduce::Fixed(v) => v.iter().map(|t| t.symbol()).collect::<String>(),
                ManaProduce::Combination { of, count } if of.len() == 5 =>
                    format!("{} mana in any combination of colors", count),
                ManaProduce::Combination { of, count } => format!(
                    "{} mana in any combination of {}",
                    count,
                    join(
                        of.iter().map(|t| t.symbol().to_string()).collect(),
                        "and/or"
                    )
                ),
                ManaProduce::Counted { of, amount: a } => match (of.as_slice(), a) {
                    ([t], Amount::Count(s)) =>
                        format!("{} for each {}", t.symbol(), selector(s, false)),
                    ([t], a) => format!("an amount of {} equal to {}", t.symbol(), amount(a)),
                    _ if of.len() == 5 =>
                        format!("X mana of any one color, where X is {}", amount(a)),
                    _ => format!(
                        "X mana of one of {}, where X is {}",
                        join(of.iter().map(|t| t.symbol().to_string()).collect(), "or"),
                        amount(a)
                    ),
                },
                ManaProduce::Reflected { from, types } => format!(
                    "one mana of any {} that {} could produce",
                    if *types { "type" } else { "color" },
                    { with_article(&selector(from, false)) }
                ),
            },
            then.as_ref().map_or(String::new(), |e| {
                let text = effect(
                    &Mode {
                        targets: Vec::new(),
                        groups: Vec::new(),
                        effect: Effect::Seq(Vec::new()),
                    },
                    e,
                );
                format!(" {}.", capitalize(&text))
            }),
            spend.as_ref().map_or(String::new(), |r| {
                let mut uses: Vec<String> = Vec::new();
                if !r.spells.is_empty() {
                    // "Cast Elf or Goblin spells", "cast creature spells of the chosen type".
                    uses.push(format!(
                        "cast {} spells{}",
                        join(r.spells.iter().map(spell_kind).collect(), "or"),
                        r.spells.first().map_or("", |s| of_chosen(s, true))
                    ));
                }
                if !r.abilities.is_empty() {
                    uses.push(format!(
                        "activate abilities of {}",
                        join(
                            r.abilities.iter().map(|s| selector(s, true)).collect(),
                            "or"
                        )
                    ));
                }
                if r.sources.iter().any(|s| *s == Selector::default()) {
                    uses.push("activate an ability".to_string());
                } else if !r.sources.is_empty() {
                    uses.push(format!(
                        "activate abilities of {} sources{}",
                        join(r.sources.iter().map(spell_kind).collect(), "or"),
                        r.sources.first().map_or("", |s| of_chosen(s, true))
                    ));
                }
                format!(" Spend this mana only to {}.", join(uses, "or"))
            }),
            only_if.as_ref().map_or(String::new(), |c| format!(
                " Activate only {}.",
                condition(c)
            ))
        ),
        AbilityDef::Triggered {
            when,
            intervening_if,
            body: b,
        } => {
            let cond = intervening_if
                .as_ref()
                .map_or(String::new(), |c| format!(", {}", condition(c)));
            let text = body(b);
            // CR 715.2a: a chapter symbol, "III — [effect]".
            if let Trigger::Chapter(n) = when
                && intervening_if.is_none()
            {
                return format!("{} — {text}", roman(*n));
            }
            let mut lower = text.clone();
            // ("X is its power": X stays a capital.)
            if !text.starts_with("X ")
                && let Some(first) = lower.chars().next()
            {
                let l: String = first.to_lowercase().collect();
                lower.replace_range(..first.len_utf8(), &l);
            }
            format!("{}{cond}, {lower}", trigger(when))
        }
        AbilityDef::Static {
            affected,
            effects,
            only_if,
        } => {
            // "… as long as you control a Swamp."
            let when = only_if.as_ref().map_or(String::new(), |c| {
                let c = condition(c);
                format!(" as long as {}", c.strip_prefix("if ").unwrap_or(&c))
            });
            let (subject, plural) = match affected {
                Affected::This => ("This permanent".to_string(), false),
                Affected::Remembered => ("That permanent".to_string(), false),
                Affected::Enchanted => ("Enchanted creature".to_string(), false),
                Affected::All(s) => (
                    sentence(selector(s, true))
                        .trim_end_matches('.')
                        .to_string(),
                    true,
                ),
            };
            // "You control enchanted creature." reads better than "is under your control".
            if matches!(effects.as_slice(), [ContinuousEffect::GainControl]) {
                return format!("You control {}{when}.", subject.to_lowercase());
            }
            format!("{subject} {}{when}.", continuous(effects, plural))
        }
        AbilityDef::Replacement {
            replaces: Replaces::Damage { by, to, combat },
            with,
        } => {
            let kind = match combat {
                Some(true) => "combat damage",
                Some(false) => "noncombat damage",
                None => "damage",
            };
            let dealt_to = damaged(to);
            let to = match to {
                DamageTo::Anything => String::new(),
                DamageTo::Player(Who::Any) => " to players".to_string(),
                DamageTo::Player(Who::You) => " to you".to_string(),
                DamageTo::Player(Who::Opponent) => " to your opponents".to_string(),
                DamageTo::Permanent(ObjFilter::Matching(s)) => {
                    format!(" to {}", selector(s, true))
                }
                DamageTo::Permanent(f) => format!(" to {}", obj_filter(f)),
                DamageTo::Either { .. } => format!(" to {}", damaged(to)),
            };
            let source = match by {
                None => "a source".to_string(),
                Some(ObjFilter::Matching(s)) if s.types.is_empty() && s.subtypes.is_empty() => {
                    with_article(&selector(s, false).replacen("permanent", "source", 1))
                }
                Some(ObjFilter::Matching(s)) => with_article(&selector(s, false)),
                Some(f) => obj_filter(f),
            };
            let by = match by {
                None => String::new(),
                Some(ObjFilter::Matching(s)) => format!(" by {}", selector(s, true)),
                Some(f) => format!(" by {}", obj_filter(f)),
            };
            match with {
                ReplaceWith::Prevent => {
                    format!("Prevent all {kind} that would be dealt{to}{by}.")
                }
                // "If a source you control would deal damage to an opponent, it deals
                // double that damage to that player instead."
                ReplaceWith::Scale(c) => format!(
                    "If {source} would deal {kind} to {dealt_to}, it deals {} instead.",
                    match c {
                        Calc::Times(2) => "double that damage".to_string(),
                        Calc::Times(3) => "triple that damage".to_string(),
                        Calc::Plus(n) => format!("that much damage plus {n}"),
                        Calc::Minus(n) => format!("that much damage minus {n}"),
                        Calc::HalfDown => "half that damage, rounded down,".to_string(),
                        Calc::HalfUp => "half that damage, rounded up,".to_string(),
                        c => format!("{} damage", scaled(*c, "that much")),
                    }
                ),
                other => format!("If {kind} would be dealt{to}{by}: {other:?}."),
            }
        }
        AbilityDef::Replacement { replaces, with } => {
            let others;
            let what = match replaces {
                Replaces::OtherEnters(s) => {
                    // "Each other creature you control enters …", not "each another".
                    let bare = Selector {
                        other: false,
                        ..s.clone()
                    };
                    others = format!(
                        "Each {}{}",
                        if s.other { "other " } else { "" },
                        selector(&bare, false)
                    );
                    others.as_str()
                }
                Replaces::CreateTokens => "If an effect would create tokens under your control",
                Replaces::PutCountersOnYourPermanents => {
                    "If an effect would put counters on a permanent you control"
                }
                Replaces::CardToGraveyard => {
                    "If a card would be put into a graveyard from anywhere"
                }
                Replaces::ThisDies => "If this creature would die",
                Replaces::ThisEnters => "This permanent",
                Replaces::Dies(s) => {
                    others = format!("If {} would die", with_article(&selector(s, false)));
                    others.as_str()
                }
                Replaces::ThisToGraveyard => {
                    "If this card would be put into a graveyard from anywhere"
                }
                // Written out by the arm above.
                Replaces::Damage { .. } => "If damage would be dealt",
                Replaces::GainLife(w) => match w {
                    Who::You => "If you would gain life",
                    Who::Opponent => "If an opponent would gain life",
                    Who::Any => "If a player would gain life",
                },
                Replaces::Tokens { token, effect_only } => {
                    others = tokens_would(token, *effect_only);
                    others.as_str()
                }
                Replaces::Counters {
                    on,
                    kind,
                    effect_only,
                } => {
                    let kind = kind.map_or(String::new(), |k| format!("{} ", counter(k)));
                    let on = with_article(&selector(on, false));
                    others = if *effect_only {
                        format!("If an effect would put one or more {kind}counters on {on}")
                    } else {
                        format!("If one or more {kind}counters would be put on {on}")
                    };
                    others.as_str()
                }
            };
            match with {
                ReplaceWith::Twice => format!("{what}, it does so twice as many instead."),
                // "If you would gain life, you gain twice that much life instead."
                ReplaceWith::Scale(c) if matches!(replaces, Replaces::GainLife(_)) => {
                    let gain = match replaces {
                        Replaces::GainLife(Who::You) => "you gain",
                        _ => "that player gains",
                    };
                    let much = match c {
                        Calc::Plus(n) => format!("that much life plus {n}"),
                        c => format!("{} life", scaled(*c, "that much")),
                    };
                    format!("{what}, {gain} {much} instead.")
                }
                ReplaceWith::Scale(c) => {
                    let noun = match replaces {
                        Replaces::Tokens { .. } => "of those tokens are created",
                        Replaces::Counters { .. } => "of those counters are put on it",
                        Replaces::GainLife(_) => "life is gained",
                        _ => "happen",
                    };
                    format!("{what}, {} {noun} instead.", scaled(*c, "that many"))
                }
                ReplaceWith::AddTokens { tokens, n } => {
                    let n = n.map_or("that many".to_string(), num);
                    format!(
                        "{what}, those tokens plus {n} {} tokens are created instead.",
                        token_names(tokens)
                    )
                }
                ReplaceWith::ReplaceTokens(tokens) => format!(
                    "{what}, that many {} tokens are created instead.",
                    token_names(tokens)
                ),
                ReplaceWith::Exile => format!("{what}, exile it instead."),
                ReplaceWith::ShuffleIntoLibrary => {
                    format!("{what}, shuffle it into its owner's library instead.")
                }
                ReplaceWith::RevealShuffleIntoLibrary => {
                    format!("{what}, reveal it and shuffle it into its owner's library instead.")
                }
                ReplaceWith::Prevent => format!("{what}, prevent it."),
                ReplaceWith::EntersTapped => format!("{what} enters tapped."),
                ReplaceWith::EntersTappedUnless(c) => {
                    // The land is not on the battlefield yet: "other lands" (CR 614.1d).
                    let c = condition(&counting_others(c));
                    format!(
                        "{what} enters tapped unless {}.",
                        c.strip_prefix("if ").unwrap_or(&c)
                    )
                }
                ReplaceWith::EntersTappedUnlessYou(act) => format!(
                    "As {} enters, you may {}. If you don't, it enters tapped.",
                    what.to_lowercase(),
                    as_enters(act)
                ),
                ReplaceWith::EntersOnlyIfYou(act) => format!(
                    "If {} would enter, you may {} instead. If you do, put it onto the \
                     battlefield. If you don't, put it into its owner's graveyard.",
                    what.to_lowercase(),
                    as_enters(act)
                ),
                ReplaceWith::Choose(c) => format!(
                    "As {} enters, choose {}.",
                    what.to_lowercase(),
                    choice_of(*c)
                ),
                ReplaceWith::EntersWithCounters { kind, n } => {
                    // Another permanent's counters are put on it besides its own.
                    let extra = matches!(replaces, Replaces::OtherEnters(_));
                    let counters = match (n, extra) {
                        (1, true) => format!("an additional {} counter", counter(*kind)),
                        (n, true) => format!("{} additional {} counters", num(*n), counter(*kind)),
                        (n, false) => counters_phrase(&Amount::Fixed(*n), *kind),
                    };
                    format!("{what} enters with {counters} on it.")
                }
                ReplaceWith::EntersAsCopy { of, except, tapped } => format!(
                    "You may have {} enter{} as a copy of {}{}.",
                    what.to_lowercase(),
                    if *tapped { " tapped" } else { "" },
                    match of.controller {
                        Who::Any => format!(
                            "any {} on the battlefield",
                            selector(
                                &Selector {
                                    other: false,
                                    ..of.clone()
                                },
                                false
                            )
                        ),
                        _ => with_article(&selector(
                            &Selector {
                                other: false,
                                ..of.clone()
                            },
                            false
                        )),
                    },
                    copy_except(except)
                ),
                ReplaceWith::EntersWithCounted { kind, amount: a } => {
                    let extra = if matches!(replaces, Replaces::OtherEnters(_)) {
                        "additional "
                    } else {
                        ""
                    };
                    match a {
                        Amount::X => format!(
                            "{what} enters with X {extra}{} counters on it.",
                            counter(*kind)
                        ),
                        a => format!(
                            "{what} enters with X {extra}{} counters on it, where X is {}.",
                            counter(*kind),
                            amount(a)
                        ),
                    }
                }
            }
        }
        AbilityDef::CantBeCountered { spells: None } => "This spell can't be countered.".into(),
        AbilityDef::CantBeRegenerated(affected) => {
            let subject = match affected {
                Affected::This => "This permanent".to_string(),
                Affected::Remembered => "That permanent".to_string(),
                Affected::Enchanted => "Enchanted creature".to_string(),
                Affected::All(s) => sentence(selector(s, true))
                    .trim_end_matches('.')
                    .to_string(),
            };
            format!("{subject} can't be regenerated.")
        }
        AbilityDef::CantBeCountered { spells: Some(s) } => {
            // "Creature spells you control can't be countered."
            let whose = match s.controller {
                Who::Any => "",
                Who::You => " you control",
                Who::Opponent => " your opponents control",
            };
            let text = selector(
                &Selector {
                    controller: Who::Any,
                    ..s.clone()
                },
                false,
            );
            let (kind, with) = match text.find(" with ") {
                Some(i) => (&text[..i], &text[i..]),
                None => (text.as_str(), ""),
            };
            let kind = kind.replace("permanent", "");
            let kind = kind.trim();
            let spells = if kind.is_empty() {
                "Spells".to_string()
            } else {
                format!("{} spells", capitalize(kind))
            };
            format!("{spells}{with}{whose} can't be countered.")
        }
        AbilityDef::AdditionalTrigger { sources, cause } => {
            let sources = match sources {
                ObjFilter::Matching(s) => with_article(&selector(s, false)),
                f => obj_filter(f),
            };
            let cause = cause.as_ref().map(|c| {
                let (s, verb) = match c {
                    crate::TriggerCause::Enters(s) => (s, "entering"),
                    crate::TriggerCause::Dies(s) => (s, "dying"),
                    crate::TriggerCause::Attacks(s) => (s, "attacking"),
                };
                format!("{} {verb}", with_article(&selector(s, false)))
            });
            match cause {
                Some(c) => format!(
                    "If {c} causes a triggered ability of {sources} to trigger, that ability \
                     triggers an additional time."
                ),
                None => format!(
                    "If a triggered ability of {sources} triggers, that ability triggers an \
                     additional time."
                ),
            }
        }
        AbilityDef::CostReduction {
            spells,
            amount: n,
            anyone,
        } => {
            // "Colorless spells you cast with mana value 7 or greater cost {1} less".
            let text = selector(spells, false);
            // "… spells you cast with power 4 or greater", "… of the chosen type".
            let split = [" with ", " of the chosen ", " that isn't "]
                .iter()
                .filter_map(|w| text.find(w))
                .min();
            let (kind, with) = match split {
                Some(i) => (&text[..i], &text[i..]),
                None => (text.as_str(), ""),
            };
            let kind = kind.replace("permanent", "");
            let kind = kind.trim();
            let spells = if kind.is_empty() {
                "Spells".to_string()
            } else {
                format!("{} spells", capitalize(kind))
            };
            let cast = if *anyone { "" } else { " you cast" };
            format!("{spells}{cast}{with} cost {{{n}}} less to cast.")
        }
        AbilityDef::CombatRule { affected, rule } => {
            let (subject, plural) = match affected {
                Affected::This => ("This creature".to_string(), false),
                Affected::Remembered => ("That permanent".to_string(), false),
                Affected::Enchanted => ("Enchanted creature".to_string(), false),
                Affected::All(s) => (
                    sentence(selector(s, true))
                        .trim_end_matches('.')
                        .to_string(),
                    true,
                ),
            };
            let _ = plural;
            match rule {
                CombatRule::CantAttack => format!("{subject} can't attack."),
                CombatRule::CantBlock => format!("{subject} can't block."),
                CombatRule::CantBeBlocked { by: None } => format!("{subject} can't be blocked."),
                CombatRule::CantBeBlocked { by: Some(s) } => {
                    format!("{subject} can't be blocked by {}.", selector(s, true))
                }
                CombatRule::CantBlockAttackers(s) => {
                    format!("{subject} can't block {}.", selector(s, true))
                }
                CombatRule::CantBlockSource => format!("{subject} can't block this creature."),
            }
        }
        AbilityDef::AdditionalCost(c) => {
            let mut text = cost(c);
            if let Some(first) = text.chars().next() {
                let lower: String = first.to_lowercase().collect();
                text.replace_range(..first.len_utf8(), &lower);
            }
            format!("As an additional cost to cast this spell, {text}.")
        }
        AbilityDef::AlternativeCost(a) => alt_cost(a),
        AbilityDef::SelfCostReduction(a) => match a {
            Amount::Fixed(n) => format!("This spell costs {{{n}}} less to cast."),
            other => format!(
                "This spell costs {{X}} less to cast, where X is {}.",
                amount(other)
            ),
        },
        AbilityDef::Enchant(f) => {
            format!("Enchant {}", target_filter(f).trim_start_matches("target "))
        }
        AbilityDef::FromZone { zone, ability: a } => {
            let place = match zone {
                ActivationZone::Graveyard => "in your graveyard",
                ActivationZone::Hand => "in your hand",
                ActivationZone::Battlefield => "on the battlefield",
            };
            let text = ability(a);
            let mut chars = text.chars();
            let rest: String = match chars.next() {
                Some(first) => first.to_lowercase().chain(chars).collect(),
                None => String::new(),
            };
            format!("As long as this card is {place}, {rest}")
        }
        AbilityDef::PlayerRule { who, rule, only_if } => {
            let text = player_rule(*who, rule);
            match only_if {
                None => format!("{text}."),
                // "During each of your turns, you may …"
                Some(Condition::YourTurn) => {
                    let mut chars = text.chars();
                    let rest: String = match chars.next() {
                        Some(first) => first.to_lowercase().chain(chars).collect(),
                        None => String::new(),
                    };
                    format!("During each of your turns, {rest}.")
                }
                Some(c) => {
                    let c = condition(c);
                    format!("{text} as long as {}.", c.strip_prefix("if ").unwrap_or(&c))
                }
            }
        }
    }
}

/// "two", "three" …: how many additional lands.
fn number_word(n: u32) -> String {
    match n {
        1 => "an".into(),
        2 => "two".into(),
        3 => "three".into(),
        4 => "four".into(),
        n => n.to_string(),
    }
}

/// A [`crate::PlayerRule`] for the players `who` names, without a full stop.
fn player_rule(who: Who, rule: &crate::PlayerRule) -> String {
    use crate::{PlayZone, PlayerRule as R};
    // (subject, possessive, verb "have", "may")
    let (subject, whose, has) = match who {
        Who::You => ("You", "your", "have"),
        Who::Opponent => ("Each opponent", "their", "has"),
        Who::Any => ("Each player", "their", "has"),
    };
    let possessive = match who {
        Who::You => "Your".to_string(),
        Who::Opponent => "Each opponent's".to_string(),
        Who::Any => "Each player's".to_string(),
    };
    match rule {
        R::NoMaximumHandSize => format!("{subject} {has} no maximum hand size"),
        R::MaximumHandSize(n) => format!("{possessive} maximum hand size is {n}"),
        R::MaximumHandSizeBy(n) if *n >= 0 => {
            format!("{possessive} maximum hand size is increased by {n}")
        }
        R::MaximumHandSizeBy(n) => {
            format!("{possessive} maximum hand size is reduced by {}", -n)
        }
        R::AdditionalLandPlays(n) => format!(
            "{subject} may play {} additional land{} on each of {whose} turns",
            number_word(*n),
            if *n == 1 { "" } else { "s" }
        ),
        R::AnyNumberOfLandPlays => {
            format!("{subject} may play any number of lands on each of {whose} turns")
        }
        R::Hexproof => format!("{subject} {has} hexproof"),
        R::Shroud => format!("{subject} {has} shroud"),
        R::TopCardRevealed => match who {
            Who::You => "Play with the top card of your library revealed".to_string(),
            _ => format!("{subject} plays with the top card of their library revealed"),
        },
        R::LookAtTopCard => {
            format!("{subject} may look at the top card of {whose} library any time")
        }
        R::PlayFrom { zone, cards } => {
            let from = match zone {
                PlayZone::Graveyard => format!("{whose} graveyard"),
                PlayZone::LibraryTop => format!("the top of {whose} library"),
                PlayZone::Exile => "exile".to_string(),
            };
            let lands = Selector {
                types: alloc::vec![CardType::Land],
                ..Selector::default()
            };
            let what: Vec<String> = cards
                .iter()
                .map(|s| {
                    if *s == Selector::default() {
                        "play lands and cast spells".to_string()
                    } else if *s == lands {
                        "play lands".to_string()
                    } else if s.not_types.contains(&CardType::Land) {
                        let mut spell = s.clone();
                        spell.not_types.retain(|t| *t != CardType::Land);
                        let cards = card_list(core::slice::from_ref(&spell), true);
                        let spells = match cards.strip_suffix("cards") {
                            Some(kind) => format!("{kind}spells"),
                            None => cards.replacen(" cards ", " spells ", 1),
                        };
                        format!("cast {spells}")
                    } else {
                        format!("play {}", card_list(core::slice::from_ref(s), true))
                    }
                })
                .collect();
            format!("{subject} may {} from {from}", join(what, "and"))
        }
        R::PlayThisFrom(zones) => {
            let from: Vec<String> = zones
                .iter()
                .map(|z| match z {
                    PlayZone::Graveyard => format!("from {whose} graveyard"),
                    PlayZone::LibraryTop => format!("from the top of {whose} library"),
                    PlayZone::Exile => "from exile".to_string(),
                })
                .collect();
            format!("{subject} may cast this card {}", join(from, "or"))
        }
    }
}

/// Looks up a token definition by the key effects create it by ([`Effect::CreateToken`]).
/// Without one, a token is named by its key.
pub type TokenLookup<'c> = &'c dyn Fn(&str) -> Option<&'c CardDef>;

fn no_tokens<'c>(_: &str) -> Option<&'c CardDef> {
    None
}

/// What the card calls itself in its text: "creature", "Aura", "land", "token", "spell".
pub fn this_word(def: &CardDef) -> String {
    let is = |t: &str| def.creature_types.iter().any(|c| c == t);
    if def.token {
        return "token".to_string();
    }
    for special in ["Aura", "Equipment", "Vehicle", "Saga", "Class", "Siege"] {
        if is(special) {
            return special.to_string();
        }
    }
    let t = &def.card_types;
    let has = |x: CardType| t.contains(&x);
    // ("This permanent isn't a creature as long as …" must not say "this creature".)
    let sometimes_not_creature = def.abilities.iter().any(|a| {
        matches!(a, AbilityDef::Static { effects, .. }
            if effects.iter().any(|e| matches!(e, ContinuousEffect::RemoveTypes(ts) if ts.contains(&CardType::Creature))))
    });
    if has(CardType::Creature) && !sometimes_not_creature {
        "creature".to_string()
    } else if (has(CardType::Instant) || has(CardType::Sorcery))
        && !t.iter().any(|x| {
            matches!(
                x,
                CardType::Artifact
                    | CardType::Enchantment
                    | CardType::Land
                    | CardType::Planeswalker
                    | CardType::Battle
            )
        })
    {
        "spell".to_string()
    } else {
        // Exactly one permanent type to name; otherwise just "permanent".
        let permanent: Vec<&CardType> = t
            .iter()
            .filter(|x| {
                matches!(
                    x,
                    CardType::Artifact
                        | CardType::Enchantment
                        | CardType::Land
                        | CardType::Planeswalker
                        | CardType::Battle
                )
            })
            .collect();
        match permanent.as_slice() {
            [one] => card_type(**one).to_string(),
            _ => "permanent".to_string(),
        }
    }
}

/// The text with its markers resolved: quoted abilities, the card's own type, and the
/// tokens an effect creates (described from `tokens`).
fn render(
    def: &CardDef,
    text: &str,
    tokens: TokenLookup<'_>,
    depth: u32,
    own: bool,
    seen: &mut Vec<String>,
) -> String {
    let text = quote_granted(def, text, own);
    expand_tokens(&text, tokens, depth, seen)
}

/// Replace each token marker `effect` left with the token's description.
///
/// A predefined token (Treasure, Food …) is followed, the first time `seen` hasn't got it,
/// by Oracle's reminder, built from the token's own abilities: "(It's an artifact with
/// "{T}, Sacrifice this token: Add one mana of any color.")".
fn expand_tokens(
    text: &str,
    tokens: TokenLookup<'_>,
    depth: u32,
    seen: &mut Vec<String>,
) -> String {
    let mut out = String::new();
    let mut parts = text.split(TOKEN);
    while let Some(plain) = parts.next() {
        // A quotation that ends a sentence ends it with its own full stop.
        let plain = match plain.strip_prefix('.') {
            Some(rest) if out.ends_with(".\"") => rest,
            _ => plain,
        };
        out.push_str(plain);
        let Some(marker) = parts.next() else { break };
        let (how, key) = marker.split_once('|').unwrap_or((marker, ""));
        let (before, after, reminder) = match tokens(key) {
            Some(def) => token_phrase(def, tokens, depth),
            None => (format!("{key} "), String::new(), None),
        };
        let desc =
            |plural: bool| format!("{before}{}{after}", if plural { "tokens" } else { "token" });
        if how == "nm" {
            out.push_str(before.trim_end());
            continue;
        }
        let plural = how != "one";
        match how.split_once(':') {
            Some(("n", count)) => out.push_str(&format!("{count} {}", desc(true))),
            Some((_, amount)) => {
                out.push_str(&format!("a number of {} equal to {amount}", desc(true)))
            }
            None => out.push_str(&with_article(&desc(false))),
        }
        if let Some(items) = reminder
            && !seen.iter().any(|k| k == key)
        {
            seen.push(key.to_string());
            out.push_str(&if plural {
                format!(" (They're artifacts with {items})")
            } else {
                format!(" (It's an artifact with {items})")
            });
        }
    }
    out
}

/// A token as Oracle text names it, around the word "token": "1/1 white Spirit creature
/// " + "token" + " with flying", or "Treasure " + "token" + "".
fn token_phrase(
    def: &CardDef,
    tokens: TokenLookup<'_>,
    depth: u32,
) -> (String, String, Option<String>) {
    let subtypes: String = def
        .land_types
        .iter()
        .map(|t| format!("{t:?}"))
        .chain(def.creature_types.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ");
    // Predefined artifact tokens (CR 111.10) are named by their type alone.
    const PREDEFINED: [&str; 9] = [
        "Treasure",
        "Food",
        "Clue",
        "Blood",
        "Gold",
        "Powerstone",
        "Map",
        "Junk",
        "Incubator",
    ];
    if def.card_types == [CardType::Artifact]
        && def.creature_types.len() == 1
        && def.land_types.is_empty()
        && PREDEFINED.contains(&subtypes.as_str())
    {
        let none: TokenLookup<'_> = &no_tokens;
        let tokens = if depth > 4 { none } else { tokens };
        let items: Vec<String> = def
            .abilities
            .iter()
            .map(|a| {
                let text = render(def, &ability(a), tokens, depth + 1, true, &mut Vec::new());
                format!("\"{text}\"")
            })
            .collect();
        let reminder = (!items.is_empty()).then(|| join(items, "and"));
        return (format!("{subtypes} "), String::new(), reminder);
    }
    let mut before = String::new();
    for st in &def.supertypes {
        before.push_str(&format!("{st:?} ").to_lowercase());
    }
    if let (Some(p), Some(t)) = (def.power, def.toughness) {
        before.push_str(&format!("{p}/{t} "));
    }
    match &def.colors {
        Some(cs) if !cs.is_empty() => {
            before.push_str(&join(
                cs.iter().map(|c| color(*c).to_string()).collect(),
                "and",
            ));
            before.push(' ');
        }
        _ => before.push_str("colorless "),
    }
    // Named like its types, unless it has a name of its own.
    let name = def.name.strip_suffix(" Token").unwrap_or(&def.name);
    if subtypes.is_empty() {
        before.push_str(&format!("{name} "));
    } else {
        before.push_str(&format!("{subtypes} "));
    }
    let mut kinds: Vec<CardType> = def.card_types.clone();
    kinds.sort_by_key(|t| match t {
        CardType::Kindred => 0,
        CardType::Artifact => 1,
        CardType::Enchantment => 2,
        CardType::Land => 3,
        CardType::Creature => 5,
        _ => 4,
    });
    for t in &kinds {
        before.push_str(&format!("{} ", card_type(*t)));
    }
    let mut with: Vec<String> = def
        .keywords
        .iter()
        .map(|k| keyword(*k).to_lowercase())
        .collect();
    let none: TokenLookup<'_> = &no_tokens;
    let tokens = if depth > 4 { none } else { tokens };
    for a in &def.abilities {
        with.push(format!(
            "\"{}\"",
            render(def, &ability(a), tokens, depth + 1, true, &mut Vec::new())
        ));
    }
    let mut after = String::new();
    if !subtypes.is_empty() && name != subtypes {
        after.push_str(&format!(" named {name}"));
    }
    if !with.is_empty() {
        after.push_str(&format!(" with {}", join(with, "and")));
    }
    (before, after, None)
}

/// One mode of a modal spell or ability, as a sentence (for the mode-choice UI).
/// `def` is the card the mode is written on.
pub fn mode_text(def: &CardDef, m: &Mode) -> String {
    mode_text_in(def, m, &no_tokens)
}

/// [`mode_text`], describing the tokens it creates from `tokens`.
pub fn mode_text_in(def: &CardDef, m: &Mode, tokens: TokenLookup<'_>) -> String {
    render(
        def,
        &sentence(mode_effect(m)),
        tokens,
        0,
        true,
        &mut Vec::new(),
    )
}

/// One ability as rules text ("Cycling {2}", "{T}: Add {G}."). `def` is the card the
/// ability is written on (for an ability another card grants: that card).
pub fn ability_text(def: &CardDef, a: &AbilityDef) -> String {
    ability_text_in(def, a, &no_tokens)
}

/// [`ability_text`], describing the tokens it creates from `tokens`.
pub fn ability_text_in(def: &CardDef, a: &AbilityDef, tokens: TokenLookup<'_>) -> String {
    // An ability the card gives to something else is about that something: "this
    // permanent" is not the card.
    let own = !(def.granted.contains(a) && !def.abilities.contains(a));
    render(def, &ability(a), tokens, 0, own, &mut Vec::new())
}

/// The card's rules text, one line per keyword group / ability.
pub fn rules_text(def: &CardDef) -> Vec<String> {
    rules_text_in(def, &no_tokens)
}

/// [`rules_text`], describing the tokens it creates from `tokens` (a token is created
/// by the key its definition has in the catalog).
pub fn rules_text_in(def: &CardDef, tokens: TokenLookup<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    // CR 702.114a: Devoid is recorded as "has no color" on a card with a mana cost.
    if !def.token && def.mana_cost.is_some() && def.colors.as_ref().is_some_and(Vec::is_empty) {
        lines.push("Devoid".to_string());
    }
    if !def.keywords.is_empty() {
        let names: Vec<&str> = def.keywords.iter().map(|k| keyword(*k)).collect();
        lines.push(names.join(", "));
    }
    // CR 305.6: basic land types grant intrinsic mana abilities.
    for t in &def.land_types {
        lines.push(format!(
            "{{T}}: Add {}.",
            crate::ManaType::from_color(t.produces()).symbol()
        ));
    }
    // What happens as the permanent enters first, as cards print it ("This land enters
    // tapped."), then the other abilities in their order.
    let enters = |a: &&AbilityDef| {
        matches!(
            a,
            AbilityDef::Replacement {
                replaces: Replaces::ThisEnters,
                ..
            }
        )
    };
    // (A token's reminder text is given the first time the card names it.)
    let mut seen = Vec::new();
    for a in def
        .abilities
        .iter()
        .filter(enters)
        .chain(def.abilities.iter().filter(|a| !enters(a)))
    {
        lines.push(render(def, &ability(a), tokens, 0, true, &mut seen));
    }
    // What an attached permanent calls the thing it is attached to.
    let host = if def.creature_types.iter().any(|t| t == "Equipment") {
        Some("quipped creature")
    } else {
        def.abilities.iter().find_map(|a| match a {
            AbilityDef::Enchant(TargetFilter::Creature) => None,
            AbilityDef::Enchant(TargetFilter::Permanent(sels)) => Some(match sels.as_slice() {
                [s] if s.types == [CardType::Land] => "nchanted land",
                [s] if s.types == [CardType::Artifact] => "nchanted artifact",
                [s] if s.types == [CardType::Creature] => "nchanted creature",
                _ => "nchanted permanent",
            }),
            _ => None,
        })
    };
    if let Some(host) = host {
        for line in &mut lines {
            *line = line.replace("nchanted creature", host);
        }
    }
    lines
}

/// "Legendary Creature — Elf Warrior".
pub fn type_line(def: &CardDef) -> String {
    let mut left: Vec<String> = def.supertypes.iter().map(|s| format!("{s:?}")).collect();
    left.extend(def.card_types.iter().map(|t| format!("{t:?}")));
    let mut right: Vec<String> = def.land_types.iter().map(|t| format!("{t:?}")).collect();
    right.extend(def.creature_types.iter().cloned());
    if def.token {
        left.insert(0, "Token".into());
    }
    if right.is_empty() {
        left.join(" ")
    } else {
        format!("{} — {}", left.join(" "), right.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DelayWindow;
    use alloc::boxed::Box;
    use alloc::vec;

    fn mode(effect: Effect) -> Mode {
        Mode {
            targets: Vec::new(),
            effect,
            groups: Vec::new(),
        }
    }

    fn with_targets(targets: Vec<TargetFilter>, effect: Effect) -> Mode {
        Mode {
            targets,
            effect,
            groups: Vec::new(),
        }
    }

    fn spell(m: Mode) -> CardDef {
        CardDef {
            name: "Test Spell".into(),
            card_types: vec![CardType::Instant],
            abilities: vec![AbilityDef::Spell(Body::Effect(m))],
            ..CardDef::default()
        }
    }

    fn text(def: &CardDef) -> String {
        rules_text(def).join(" / ")
    }

    fn creature(types: &[&str]) -> Selector {
        Selector {
            types: vec![CardType::Creature],
            subtypes: types.iter().map(|s| s.to_string()).collect(),
            ..Selector::default()
        }
    }

    #[test]
    fn articles_follow_the_sound() {
        for (word, expected) in [
            ("artifact", "an artifact"),
            ("enchantment", "an enchantment"),
            ("Aura", "an Aura"),
            ("Angel", "an Angel"),
            ("Elf", "an Elf"),
            ("Ox", "an Ox"),
            ("creature", "a creature"),
            ("Human", "a Human"),
            ("Unicorn", "a Unicorn"),
            ("untapped creature", "an untapped creature"),
            ("8/8 green Beast", "an 8/8 green Beast"),
            ("1/1 white Spirit", "a 1/1 white Spirit"),
            ("X/X Construct", "an X/X Construct"),
            ("another creature", "another creature"),
        ] {
            assert_eq!(with_article(word), expected);
        }
    }

    #[test]
    fn plurals_of_creature_types_and_lands() {
        // CR 205.3m: the irregular ones.
        for (one, many) in [
            ("Elf", "Elves"),
            ("Dwarf", "Dwarves"),
            ("Wolf", "Wolves"),
            ("Werewolf", "Werewolves"),
            ("Mouse", "Mice"),
            ("Fungus", "Fungi"),
            ("Cyclops", "Cyclopes"),
            ("Sphinx", "Sphinxes"),
            ("Octopus", "Octopuses"),
            ("Ally", "Allies"),
            ("Harpy", "Harpies"),
            ("Plains", "Plains"),
            ("Eldrazi", "Eldrazi"),
            ("Sheep", "Sheep"),
            ("Fish", "Fish"),
            ("Merfolk", "Merfolk"),
            ("Treefolk", "Treefolk"),
            ("Aurochs", "Aurochs"),
            ("Djinn", "Djinn"),
            ("Goblin", "Goblins"),
            ("Time Lord", "Time Lords"),
            ("artifact", "artifacts"),
            ("Island", "Islands"),
            ("Swamp", "Swamps"),
        ] {
            assert_eq!(pluralize(one), many);
        }
    }

    #[test]
    fn numbers_in_running_text_are_words() {
        assert_eq!(num(1), "one");
        assert_eq!(num(2), "two");
        assert_eq!(num(7), "seven");
        assert_eq!(num(10), "ten");
        assert_eq!(num(11), "11");
        let def = spell(mode(Effect::Draw {
            who: PlayerRef::You,
            amount: Amount::Fixed(7),
        }));
        assert_eq!(text(&def), "Draw seven cards.");
        let def = spell(mode(Effect::Draw {
            who: PlayerRef::You,
            amount: Amount::Fixed(1),
        }));
        assert_eq!(text(&def), "Draw a card.");
        // Life and damage keep their digits.
        let def = spell(mode(Effect::GainLife {
            who: PlayerRef::You,
            amount: Amount::Fixed(3),
        }));
        assert_eq!(text(&def), "You gain 3 life.");
    }

    #[test]
    fn counters_and_amounts_read_as_cards_do() {
        let counters = |count| {
            text(&spell(mode(Effect::PutCounters {
                what: ObjRef::This,
                kind: CounterKind::PlusOne,
                count,
            })))
        };
        assert_eq!(
            counters(Amount::Fixed(1)),
            "Put a +1/+1 counter on this spell."
        );
        assert_eq!(
            counters(Amount::Fixed(2)),
            "Put two +1/+1 counters on this spell."
        );
        assert_eq!(counters(Amount::X), "Put X +1/+1 counters on this spell.");
        assert_eq!(
            counters(Amount::SourcePower),
            "Put a number of +1/+1 counters equal to its power on this spell."
        );
        // "that much", "life equal to": no "gain its power life".
        let life = |amount| {
            text(&spell(mode(Effect::GainLife {
                who: PlayerRef::You,
                amount,
            })))
        };
        assert_eq!(life(Amount::TriggeringAmount), "You gain that much life.");
        assert_eq!(
            life(Amount::SourcePower),
            "You gain life equal to its power."
        );
        let draw = |amount| {
            text(&spell(mode(Effect::Draw {
                who: PlayerRef::You,
                amount,
            })))
        };
        assert_eq!(draw(Amount::TriggeringAmount), "Draw that many cards.");
        assert_eq!(
            draw(Amount::SourcePower),
            "Draw a number of cards equal to its power."
        );
        let mill = text(&spell(mode(Effect::Mill {
            who: PlayerRef::EachOpponent,
            amount: Amount::TriggeringAmount,
        })));
        assert_eq!(mill, "Each opponent mills that many cards.");
        let lose = text(&spell(mode(Effect::LoseLife {
            who: PlayerRef::You,
            amount: Amount::NotedTotal(Measure::ManaValue),
        })));
        assert_eq!(
            lose,
            "You lose life equal to the total mana value of those cards."
        );
        // "X mana … where X is X" says it once.
        let mana = text(&spell(mode(Effect::AddMana {
            who: PlayerRef::You,
            mana: crate::ManaType::Red,
            amount: Amount::X,
        })));
        assert_eq!(mana, "Add X mana of type {R}.");
    }

    #[test]
    fn selectors_use_the_right_plural() {
        let both = Selector {
            types: vec![CardType::Artifact, CardType::Enchantment],
            ..Selector::default()
        };
        assert_eq!(selector(&both, true), "artifacts or enchantments");
        assert_eq!(selector(&both, false), "artifact or enchantment");
        assert_eq!(selector(&creature(&["Elf"]), true), "Elf creatures");
        let elves = Selector {
            subtypes: vec!["Elf".into(), "Dwarf".into()],
            ..Selector::default()
        };
        assert_eq!(selector(&elves, true), "Elves or Dwarves");
        let plains = Selector {
            subtypes: vec!["Plains".into()],
            ..Selector::default()
        };
        assert_eq!(selector(&plains, true), "Plains");
        assert_eq!(selector(&plains, false), "Plains");
        let tokens = Selector {
            types: vec![CardType::Creature],
            token: Some(true),
            ..Selector::default()
        };
        assert_eq!(selector(&tokens, true), "creature tokens");
        assert_eq!(selector(&tokens, false), "creature token");
        let any_token = Selector {
            token: Some(true),
            ..Selector::default()
        };
        assert_eq!(selector(&any_token, true), "tokens");
        let flying = Selector {
            keywords: vec![Keyword::Defender],
            ..creature(&[])
        };
        assert_eq!(selector(&flying, false), "creature with defender");
    }

    #[test]
    fn each_other_creature_not_each_another() {
        let def = CardDef {
            name: "Test Lord".into(),
            card_types: vec![CardType::Creature],
            abilities: vec![AbilityDef::Replacement {
                replaces: Replaces::OtherEnters(Selector {
                    other: true,
                    controller: Who::You,
                    ..creature(&["Angel"])
                }),
                with: ReplaceWith::EntersWithCounters {
                    kind: CounterKind::PlusOne,
                    n: 1,
                },
            }],
            ..CardDef::default()
        };
        assert_eq!(
            text(&def),
            "Each other Angel creature you control enters with an additional +1/+1 counter on it."
        );
    }

    #[test]
    fn card_lists_keep_every_bound() {
        // Imperial Recruiter: "a creature card with power 2 or less".
        let small = Selector {
            types: vec![CardType::Creature],
            power_max: Some(2),
            ..Selector::default()
        };
        assert_eq!(
            card_list(core::slice::from_ref(&small), false),
            "creature card with power 2 or less"
        );
        let tough = Selector {
            toughness_max: Some(2),
            ..small.clone()
        };
        assert_eq!(
            card_list(core::slice::from_ref(&tough), false),
            "creature card with power 2 or less and toughness 2 or less"
        );
        let both = Selector {
            power_min: Some(3),
            toughness_min: Some(4),
            ..Selector::default()
        };
        assert_eq!(
            card_list(core::slice::from_ref(&both), true),
            "cards with power 3 or greater and toughness 4 or greater"
        );
        // The same bounds on a permanent selector.
        assert_eq!(
            selector(&both, true),
            "permanents with power 3 or greater and toughness 4 or greater"
        );
        assert_eq!(
            selector(&tough, false),
            "creature with power 2 or less and toughness 2 or less"
        );
    }

    #[test]
    fn card_lists_merge_what_differs_in_one_thing() {
        let spell_with = |v: u32| Selector {
            types: vec![CardType::Creature],
            mana_value_min: Some(v),
            mana_value_max: Some(v),
            ..Selector::default()
        };
        assert_eq!(
            card_list(&[spell_with(4), spell_with(5), spell_with(6)], false),
            "creature card with mana value 4, 5 or 6"
        );
        let kind = |t| Selector {
            types: vec![t],
            ..Selector::default()
        };
        assert_eq!(
            card_list(&[kind(CardType::Creature), kind(CardType::Land)], false),
            "creature or land card"
        );
        let def = CardDef {
            name: "Test Unsealing".into(),
            card_types: vec![CardType::Enchantment],
            abilities: vec![AbilityDef::Triggered {
                when: Trigger::Cast {
                    by: Who::You,
                    types: Vec::new(),
                    spells: vec![spell_with(4), spell_with(5), spell_with(6)],
                    nth: None,
                    copies: false,
                },
                intervening_if: None,
                body: Body::Effect(mode(Effect::Draw {
                    who: PlayerRef::You,
                    amount: Amount::Fixed(1),
                })),
            }],
            ..CardDef::default()
        };
        assert_eq!(
            text(&def),
            "Whenever you cast a creature spell with mana value 4, 5 or 6, draw a card."
        );
    }

    fn search(to: CardDest, shuffle: bool, reveal: bool) -> CardDef {
        spell(mode(Effect::ChooseCards {
            who: PlayerRef::You,
            from: CardZone::Library,
            cards: Vec::new(),
            count: Amount::Fixed(1),
            must: false,
            to,
            reveal,
            shuffle,
            optional: false,
            share_land_type: false,
        }))
    }

    #[test]
    fn searching_to_the_top_shuffles_first() {
        // CR 701.24b; the engine shuffles before it places the card.
        assert_eq!(
            text(&search(CardDest::LibraryTop, true, false)),
            "Search your library for a card, then shuffle and put that card on top of your library."
        );
        assert_eq!(
            text(&search(CardDest::LibraryTop, true, true)),
            "Search your library for a card, reveal it, then shuffle and put that card on top of your library."
        );
        // To the hand, the shuffle comes after the card has left.
        assert_eq!(
            text(&search(CardDest::Hand, true, true)),
            "Search your library for a card, reveal it, put it into your hand, then shuffle."
        );
        // A search that reveals cards it puts into the graveyard does reveal them.
        assert_eq!(
            text(&search(CardDest::Graveyard, true, true)),
            "Search your library for a card, reveal it, put it into your graveyard, then shuffle."
        );
    }

    #[test]
    fn digging_names_what_it_does_with_the_card() {
        let land = Selector {
            types: vec![CardType::Land],
            ..Selector::default()
        };
        let dig = |take, reveal_taken| {
            text(&spell(mode(Effect::Dig {
                who: PlayerRef::You,
                count: Amount::Fixed(4),
                reveal: true,
                take,
                cards: vec![land.clone()],
                to: CardDest::Hand,
                reveal_taken,
                rest: CardDest::Graveyard,
                rest_random: false,
            })))
        };
        assert_eq!(
            dig(Take::UpTo(1), false),
            "Reveal the top four cards of your library. Then you may put a land card from among them into your hand and the rest into your graveyard."
        );
        assert_eq!(
            dig(Take::Any, true),
            "Reveal the top four cards of your library. Then you may put any number of land cards from among them into your hand and the rest into your graveyard."
        );
    }

    #[test]
    fn a_follow_up_on_the_same_target_says_that() {
        let creature_target = vec![TargetFilter::Creature];
        let m = with_targets(
            creature_target.clone(),
            Effect::Seq(vec![
                Effect::PutCounters {
                    what: ObjRef::Target(0),
                    kind: CounterKind::PlusOne,
                    count: Amount::Fixed(1),
                },
                Effect::Untap {
                    what: ObjRef::Target(0),
                },
            ]),
        );
        assert_eq!(
            text(&spell(m)),
            "Put a +1/+1 counter on target creature. Then untap that creature."
        );
        // A player target too.
        let m = with_targets(
            vec![TargetFilter::Player],
            Effect::Seq(vec![
                Effect::Draw {
                    who: PlayerRef::Target(0),
                    amount: Amount::Fixed(2),
                },
                Effect::LoseLife {
                    who: PlayerRef::Target(0),
                    amount: Amount::Fixed(2),
                },
            ]),
        );
        assert_eq!(
            text(&spell(m)),
            "Target player draws two cards. Then that player loses 2 life."
        );
        // Two different targets that would be called the same stay "target creature".
        let m = with_targets(
            vec![TargetFilter::Creature, TargetFilter::Creature],
            Effect::Seq(vec![
                Effect::Untap {
                    what: ObjRef::Target(0),
                },
                Effect::Tap {
                    what: ObjRef::Target(1),
                },
                Effect::Untap {
                    what: ObjRef::Target(0),
                },
            ]),
        );
        assert_eq!(
            text(&spell(m)),
            "Untap target creature. Then tap target creature. Then untap target creature."
        );
    }

    #[test]
    fn one_word_target_for_several_targets() {
        let mut m = with_targets(
            vec![TargetFilter::Creature, TargetFilter::Creature],
            Effect::Together(vec![
                Effect::Destroy {
                    what: ObjRef::Target(0),
                },
                Effect::Destroy {
                    what: ObjRef::Target(1),
                },
            ]),
        );
        m.groups = vec![crate::TargetGroup {
            first: 0,
            count: 2,
            min: 0,
        }];
        assert_eq!(
            text(&spell(m.clone())),
            "Destroy each of up to two target creatures."
        );
        m.groups[0].min = 2;
        assert_eq!(
            text(&spell(m.clone())),
            "Destroy each of two target creatures."
        );
        // "One to two" keeps the word "targets": they are different ones.
        m.groups[0].min = 1;
        assert_eq!(
            text(&spell(m)),
            "Destroy each of one to two targets that are creatures."
        );
    }

    #[test]
    fn a_choice_for_another_player_is_something_you_may_have_them_do() {
        let m = with_targets(
            vec![TargetFilter::Opponent],
            Effect::May(Box::new(Effect::LoseLife {
                who: PlayerRef::Target(0),
                amount: Amount::Fixed(1),
            })),
        );
        assert_eq!(text(&spell(m)), "You may have target opponent lose 1 life.");
        let m = mode(Effect::May(Box::new(Effect::LoseLife {
            who: PlayerRef::TriggeringController,
            amount: Amount::Fixed(1),
        })));
        assert_eq!(text(&spell(m)), "You may have its controller lose 1 life.");
        let m = with_targets(
            vec![TargetFilter::Creature],
            Effect::May(Box::new(Effect::Apply {
                what: ObjRef::Target(0),
                effects: vec![ContinuousEffect::ModifyPT(2, 0)],
                until_end_of_turn: true,
            })),
        );
        assert_eq!(
            text(&spell(m)),
            "You may have target creature get +2/+0 until end of turn."
        );
        // For yourself it is just "you may".
        let m = mode(Effect::May(Box::new(Effect::Draw {
            who: PlayerRef::You,
            amount: Amount::Fixed(1),
        })));
        assert_eq!(text(&spell(m)), "You may draw a card.");
    }

    #[test]
    fn modal_spells_bullet_each_mode() {
        let draw = mode(Effect::Draw {
            who: PlayerRef::You,
            amount: Amount::Fixed(1),
        });
        let gain = mode(Effect::GainLife {
            who: PlayerRef::You,
            amount: Amount::Fixed(3),
        });
        let modal = |min, max| {
            text(&CardDef {
                name: "Test Charm".into(),
                card_types: vec![CardType::Instant],
                abilities: vec![AbilityDef::Spell(Body::Modal {
                    min,
                    max,
                    modes: vec![draw.clone(), gain.clone()],
                })],
                ..CardDef::default()
            })
        };
        assert_eq!(
            modal(1, 1),
            "Choose one — • Draw a card. • You gain 3 life."
        );
        assert_eq!(
            modal(2, 2),
            "Choose two — • Draw a card. • You gain 3 life."
        );
        assert_eq!(
            modal(1, 2),
            "Choose one or both — • Draw a card. • You gain 3 life."
        );
        assert_eq!(
            modal(0, 2),
            "Choose up to two — • Draw a card. • You gain 3 life."
        );
    }

    fn token(name: &str, key: &str) -> CardDef {
        CardDef {
            name: name.into(),
            key: Some(key.into()),
            token: true,
            ..CardDef::default()
        }
    }

    #[test]
    fn tokens_are_described_by_their_definition() {
        let treasure = CardDef {
            card_types: vec![CardType::Artifact],
            creature_types: vec!["Treasure".into()],
            abilities: vec![AbilityDef::Mana {
                cost: Cost {
                    tap: true,
                    sacrifice_self: true,
                    ..Cost::default()
                },
                produce: ManaProduce::AnyColor,
                only_if: None,
                spend: None,
                then: None,
            }],
            ..token("Treasure Token", "c_a_treasure_sac")
        };
        let spawn = CardDef {
            card_types: vec![CardType::Creature],
            creature_types: vec!["Eldrazi".into(), "Spawn".into()],
            power: Some(0),
            toughness: Some(1),
            colors: Some(Vec::new()),
            abilities: vec![AbilityDef::Mana {
                cost: Cost {
                    sacrifice_self: true,
                    ..Cost::default()
                },
                produce: ManaProduce::One(crate::ManaType::Colorless),
                only_if: None,
                spend: None,
                then: None,
            }],
            ..token("Eldrazi Spawn Token", "c_0_1_eldrazi_spawn_sac")
        };
        let spirit = CardDef {
            card_types: vec![CardType::Creature],
            creature_types: vec!["Spirit".into()],
            power: Some(1),
            toughness: Some(1),
            colors: Some(vec![Color::White]),
            keywords: vec![Keyword::Flying],
            ..token("Spirit Token", "w_1_1_spirit_flying")
        };
        let dryad = CardDef {
            card_types: vec![CardType::Creature, CardType::Land],
            land_types: vec![crate::BasicLandType::Forest],
            creature_types: vec!["Dryad".into()],
            power: Some(1),
            toughness: Some(1),
            colors: Some(vec![Color::Green]),
            ..token("Forest Dryad Token", "g_1_1_forest_dryad")
        };
        let defs = [treasure, spawn, spirit, dryad];
        let lookup = |key: &str| defs.iter().find(|d| d.key.as_deref() == Some(key));
        let make = |key: &str, count| {
            let def = spell(mode(Effect::CreateToken {
                token: key.to_string(),
                count,
                who: PlayerRef::You,
            }));
            (text(&def), rules_text_in(&def, &lookup).join(" / "))
        };
        // Without definitions the key is all there is to say.
        let (bare, _) = make("c_a_treasure_sac", Amount::Fixed(1));
        assert_eq!(bare, "Create a c_a_treasure_sac token.");
        // A predefined token comes with Oracle's reminder, from its own abilities.
        assert_eq!(
            make("c_a_treasure_sac", Amount::Fixed(1)).1,
            "Create a Treasure token (It's an artifact with \"{T}, Sacrifice this token: Add one mana of any color.\")."
        );
        assert_eq!(
            make("c_a_treasure_sac", Amount::Fixed(3)).1,
            "Create three Treasure tokens (They're artifacts with \"{T}, Sacrifice this token: Add one mana of any color.\")."
        );
        // ... the first time only.
        let twice = spell(Mode {
            targets: Vec::new(),
            effect: Effect::Seq(vec![
                Effect::CreateToken {
                    token: "c_a_treasure_sac".into(),
                    count: Amount::Fixed(1),
                    who: PlayerRef::You,
                },
                Effect::CreateToken {
                    token: "c_a_treasure_sac".into(),
                    count: Amount::Fixed(1),
                    who: PlayerRef::You,
                },
            ]),
            groups: Vec::new(),
        });
        assert_eq!(
            rules_text_in(&twice, &lookup).join(" / "),
            "Create a Treasure token (It's an artifact with \"{T}, Sacrifice this token: Add one mana of any color.\"). Then create a Treasure token."
        );
        assert_eq!(
            make("c_0_1_eldrazi_spawn_sac", Amount::Fixed(1)).1,
            "Create a 0/1 colorless Eldrazi Spawn creature token with \"Sacrifice this token: Add {C}.\""
        );
        assert_eq!(
            make("w_1_1_spirit_flying", Amount::Fixed(2)).1,
            "Create two 1/1 white Spirit creature tokens with flying."
        );
        assert_eq!(
            make("w_1_1_spirit_flying", Amount::X).1,
            "Create X 1/1 white Spirit creature tokens with flying."
        );
        assert_eq!(
            make("w_1_1_spirit_flying", Amount::SourcePower).1,
            "Create a number of 1/1 white Spirit creature tokens with flying equal to its power."
        );
        // Land types are part of the name.
        assert_eq!(
            make("g_1_1_forest_dryad", Amount::Fixed(1)).1,
            "Create a 1/1 green Forest Dryad land creature token named Dryad."
                .replace(" named Dryad", "")
        );
    }

    #[test]
    fn devoid_is_said() {
        let def = CardDef {
            name: "Test Eldrazi".into(),
            mana_cost: crate::ManaCost::parse("{2}{U}").ok(),
            card_types: vec![CardType::Creature],
            colors: Some(Vec::new()),
            keywords: vec![Keyword::Flying],
            ..CardDef::default()
        };
        assert_eq!(rules_text(&def), ["Devoid", "Flying"]);
        // A token with no colors is just colorless.
        let spawn = CardDef {
            colors: Some(Vec::new()),
            token: true,
            ..CardDef::default()
        };
        assert!(rules_text(&spawn).is_empty());
    }

    fn trigger_def(card_types: Vec<CardType>, creature_types: Vec<&str>, when: Trigger) -> CardDef {
        CardDef {
            name: "Test Card".into(),
            card_types,
            creature_types: creature_types.into_iter().map(String::from).collect(),
            abilities: vec![AbilityDef::Triggered {
                when,
                intervening_if: None,
                body: Body::Effect(mode(Effect::Draw {
                    who: PlayerRef::You,
                    amount: Amount::Fixed(1),
                })),
            }],
            ..CardDef::default()
        }
    }

    #[test]
    fn when_for_its_own_entering_and_the_card_by_its_type() {
        let c = |t, subtypes| trigger_def(t, subtypes, Trigger::Enters(ObjFilter::This));
        assert_eq!(
            text(&c(vec![CardType::Creature], vec![])),
            "When this creature enters, draw a card."
        );
        assert_eq!(
            text(&c(vec![CardType::Land], vec![])),
            "When this land enters, draw a card."
        );
        assert_eq!(
            text(&c(vec![CardType::Artifact], vec![])),
            "When this artifact enters, draw a card."
        );
        assert_eq!(
            text(&c(vec![CardType::Enchantment], vec!["Aura"])),
            "When this Aura enters, draw a card."
        );
        assert_eq!(
            text(&c(vec![CardType::Artifact], vec!["Equipment"])),
            "When this Equipment enters, draw a card."
        );
        // Two permanent types and no creature: no single word.
        assert_eq!(
            text(&c(vec![CardType::Artifact, CardType::Land], vec![])),
            "When this permanent enters, draw a card."
        );
        // Others entering are "whenever".
        let other = trigger_def(
            vec![CardType::Creature],
            vec![],
            Trigger::Enters(ObjFilter::Matching(creature(&[]))),
        );
        assert_eq!(text(&other), "Whenever a creature enters, draw a card.");
        // Entering or dying: once each; entering or attacking: any number of times.
        let both = trigger_def(
            vec![CardType::Creature],
            vec![],
            Trigger::Any(vec![
                Trigger::Enters(ObjFilter::This),
                Trigger::Dies(ObjFilter::This),
            ]),
        );
        assert_eq!(
            text(&both),
            "When this creature enters or dies, draw a card."
        );
        let attacks = trigger_def(
            vec![CardType::Creature],
            vec![],
            Trigger::Any(vec![
                Trigger::Enters(ObjFilter::This),
                Trigger::Attacks(ObjFilter::This),
            ]),
        );
        assert_eq!(
            text(&attacks),
            "Whenever this creature enters or attacks, draw a card."
        );
        // Only creatures "die".
        let artifact = trigger_def(
            vec![CardType::Artifact],
            vec![],
            Trigger::Dies(ObjFilter::This),
        );
        assert_eq!(
            text(&artifact),
            "When this artifact is put into a graveyard from the battlefield, draw a card."
        );
        // A spell is "this spell".
        let m = mode(Effect::SacrificeThis);
        assert_eq!(text(&spell(m)), "Sacrifice this spell.");
    }

    #[test]
    fn the_second_card_each_turn() {
        let def = trigger_def(
            vec![CardType::Creature],
            vec![],
            Trigger::Draws {
                who: Who::You,
                nth: Some(2),
            },
        );
        assert_eq!(
            text(&def),
            "Whenever you draw your second card each turn, draw a card."
        );
        let def = trigger_def(
            vec![CardType::Creature],
            vec![],
            Trigger::Draws {
                who: Who::Opponent,
                nth: Some(3),
            },
        );
        assert_eq!(
            text(&def),
            "Whenever an opponent draws their third card each turn, draw a card."
        );
    }

    #[test]
    fn a_land_that_enters_tapped_counts_the_other_lands() {
        let lands = |n| Condition::ControlAtMost {
            what: Selector {
                types: vec![CardType::Land],
                controller: Who::You,
                ..Selector::default()
            },
            at_most: n,
        };
        let def = CardDef {
            name: "Test Fastland".into(),
            card_types: vec![CardType::Land],
            abilities: vec![AbilityDef::Replacement {
                replaces: Replaces::ThisEnters,
                with: ReplaceWith::EntersTappedUnless(lands(2)),
            }],
            ..CardDef::default()
        };
        assert_eq!(
            text(&def),
            "This land enters tapped unless you control two or fewer other lands."
        );
        // Basic lands are not "other" lands: the battle lands do not say so.
        let basics = Condition::Control {
            what: Selector {
                types: vec![CardType::Land],
                supertypes: vec![crate::Supertype::Basic],
                controller: Who::You,
                ..Selector::default()
            },
            at_least: 2,
        };
        let def = CardDef {
            abilities: vec![AbilityDef::Replacement {
                replaces: Replaces::ThisEnters,
                with: ReplaceWith::EntersTappedUnless(basics),
            }],
            ..def
        };
        assert_eq!(
            text(&def),
            "This land enters tapped unless you control two or more basic lands."
        );
    }

    #[test]
    fn what_a_land_does_as_it_enters_comes_first() {
        let def = CardDef {
            name: "Test Gate".into(),
            card_types: vec![CardType::Land],
            abilities: vec![
                AbilityDef::Mana {
                    cost: Cost {
                        tap: true,
                        ..Cost::default()
                    },
                    produce: ManaProduce::One(crate::ManaType::Green),
                    only_if: None,
                    spend: None,
                    then: None,
                },
                AbilityDef::Replacement {
                    replaces: Replaces::ThisEnters,
                    with: ReplaceWith::EntersTapped,
                },
            ],
            ..CardDef::default()
        };
        assert_eq!(
            rules_text(&def),
            ["This land enters tapped.", "{T}: Add {G}."]
        );
    }

    #[test]
    fn a_creature_that_sometimes_is_none_does_not_say_creature() {
        let def = CardDef {
            name: "Test God".into(),
            card_types: vec![CardType::Enchantment, CardType::Creature],
            abilities: vec![AbilityDef::Static {
                affected: Affected::This,
                effects: vec![ContinuousEffect::RemoveTypes(vec![CardType::Creature])],
                only_if: None,
            }],
            ..CardDef::default()
        };
        assert_eq!(text(&def), "This enchantment isn't a creature.");
    }

    #[test]
    fn an_ability_given_to_others_is_not_about_the_card_that_gives_it() {
        let ping = AbilityDef::Spell(Body::Effect(with_targets(
            vec![TargetFilter::AnyTarget],
            Effect::DealDamage {
                to: TargetRef::Target(0),
                amount: Amount::Fixed(1),
                by: DamageBy::This,
            },
        )));
        let aura = CardDef {
            name: "Test Aura".into(),
            card_types: vec![CardType::Enchantment],
            creature_types: vec!["Aura".into()],
            abilities: vec![ping.clone()],
            granted: vec![ping.clone()],
            ..CardDef::default()
        };
        // On the Aura itself it is the Aura; as the Aura gives it, it is whatever has it.
        let given = CardDef {
            abilities: Vec::new(),
            ..aura.clone()
        };
        assert_eq!(
            ability_text(&given, &given.granted[0]),
            "This permanent deals 1 damage to any target."
        );
        assert_eq!(
            ability_text(&aura, &aura.abilities[0]),
            "This Aura deals 1 damage to any target."
        );
    }

    #[test]
    fn equipment_and_other_subtypes_with_the_same_plural() {
        assert_eq!(pluralize("Equipment"), "Equipment");
        let dwarves = Selector {
            subtypes: vec!["Dwarf".into(), "Equipment".into()],
            ..Selector::default()
        };
        assert_eq!(selector(&dwarves, true), "Dwarves or Equipment");
    }

    #[test]
    fn a_whole_optional_sequence_starts_with_who_does_the_first_thing() {
        // Ob Nixilis: "you may have target player lose 3 life. If you do, put …".
        let m = with_targets(
            vec![TargetFilter::Player],
            Effect::May(Box::new(Effect::IfDone {
                first: Box::new(Effect::LoseLife {
                    who: PlayerRef::Target(0),
                    amount: Amount::Fixed(3),
                }),
                then: Box::new(Effect::PutCounters {
                    what: ObjRef::This,
                    kind: CounterKind::PlusOne,
                    count: Amount::Fixed(3),
                }),
            })),
        );
        assert_eq!(
            text(&spell(m)),
            "You may have target player lose 3 life. If you do, put three +1/+1 counters on this spell."
        );
    }

    #[test]
    fn shuffling_and_drawing_up_to_and_looking_at_the_top() {
        let text_of = |e| text(&spell(mode(e)));
        assert_eq!(
            text_of(Effect::Shuffle {
                who: PlayerRef::You
            }),
            "Shuffle your library."
        );
        assert_eq!(
            text_of(Effect::DrawUpTo {
                who: PlayerRef::You,
                amount: Amount::Fixed(2)
            }),
            "You may draw up to two cards."
        );
        assert_eq!(
            text_of(Effect::DrawUpTo {
                who: PlayerRef::You,
                amount: Amount::Fixed(1)
            }),
            "You may draw up to one card."
        );
        let look = |count| {
            text_of(Effect::Dig {
                who: PlayerRef::You,
                count,
                reveal: false,
                take: Take::Exactly(0),
                cards: Vec::new(),
                to: CardDest::Hand,
                reveal_taken: false,
                rest: CardDest::LibraryTop,
                rest_random: false,
            })
        };
        assert_eq!(
            look(Amount::Fixed(3)),
            "Look at the top three cards of your library, then put them back in any order."
        );
        assert_eq!(
            look(Amount::Fixed(1)),
            "Look at the top card of your library."
        );
    }

    #[test]
    fn a_delayed_trigger_says_when_and_what_much() {
        let delayed = AbilityDef::Triggered {
            when: Trigger::BeginningOf {
                step: TriggerStep::Upkeep,
                whose: Who::Any,
            },
            intervening_if: None,
            body: Body::Effect(mode(Effect::Draw {
                who: PlayerRef::You,
                amount: Amount::Fixed(1),
            })),
        };
        let def = CardDef {
            name: "Test Delay".into(),
            card_types: vec![CardType::Instant],
            abilities: vec![AbilityDef::Spell(Body::Effect(mode(
                Effect::DelayedTrigger {
                    ability: 0,
                    window: DelayWindow::NextTurn,
                    player: None,
                    amount: None,
                    objects: None,
                    one: false,
                },
            )))],
            granted: vec![delayed],
            ..CardDef::default()
        };
        assert_eq!(
            text(&def),
            "At the beginning of the next turn's upkeep, draw a card."
        );
    }

    #[test]
    fn a_search_for_cards_that_share_a_land_type() {
        let basic = Selector {
            types: vec![CardType::Land],
            supertypes: vec![crate::Supertype::Basic],
            ..Selector::default()
        };
        let def = spell(mode(Effect::ChooseCards {
            who: PlayerRef::You,
            from: CardZone::Library,
            cards: vec![basic],
            count: Amount::Fixed(2),
            must: false,
            to: CardDest::Battlefield { tapped: true },
            reveal: false,
            shuffle: true,
            optional: false,
            share_land_type: true,
        }));
        assert_eq!(
            text(&def),
            "Search your library for up to two basic land cards that share a land type, put them onto the battlefield tapped, then shuffle."
        );
    }
}
