//! Plain-English rules text generated from a card's ability definitions. The UI shows it for
//! every card, so what a player reads is exactly what the engine does (for real cards the
//! UI can show Scryfall's Oracle text next to it). Every match is exhaustive: adding
//! vocabulary without describing it is a compile error.

use crate::{
    AbilityDef, Affected, Amount, Body, CardDef, CardType, Color, Condition, ContinuousEffect,
    Cost, CounterKind, DamageBy, EachPlayerAction, Effect, Keyword, ManaProduce, Mode, ObjFilter,
    ObjRef, PlayerRef, ReplaceWith, Replaces, Selector, TargetFilter, TargetRef, Timing, Trigger,
    TriggerStep, Who,
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

fn amount(a: &Amount) -> String {
    match a {
        Amount::Fixed(n) => n.to_string(),
        Amount::X => "X".into(),
        Amount::SourcePower => "its power".into(),
        Amount::Count(s) => format!("the number of {}", selector(s, true)),
        Amount::TotalPower(s) => format!("the total power of {}", selector(s, true)),
        Amount::TriggeringPower => "its power".into(),
        Amount::TriggeringToughness => "its toughness".into(),
    }
}

fn target_filter(f: &TargetFilter) -> String {
    match f {
        TargetFilter::AnyTarget => "any target".into(),
        TargetFilter::Creature => "target creature".into(),
        TargetFilter::Player => "target player".into(),
        TargetFilter::Opponent => "target opponent".into(),
        TargetFilter::Spell => "target spell".into(),
        TargetFilter::SpellMatching(s) => format!("target {} spell", spell_kind(s)),
        TargetFilter::Permanent(selectors) => {
            let parts: Vec<String> = selectors.iter().map(|s| selector(s, false)).collect();
            format!("target {}", join(parts, "or"))
        }
        TargetFilter::CardInGraveyard { cards, whose } => format!(
            "target {} from {}",
            card_noun(cards),
            match whose {
                Who::Any => "a graveyard",
                Who::You => "your graveyard",
                Who::Opponent => "an opponent's graveyard",
            }
        ),
    }
}

fn with_article(s: &str) -> String {
    let vowel = s.starts_with(['a', 'e', 'i', 'o', 'u', 'A', 'E', 'I', 'O', 'U']);
    if s.starts_with("another ") {
        s.to_string()
    } else if vowel {
        format!("an {s}")
    } else {
        format!("a {s}")
    }
}

/// "creature", "noncreature", "instant or sorcery", "blue Wizard".
fn spell_kind(s: &Selector) -> String {
    let mut words: Vec<String> = Vec::new();
    words.extend(s.colors.iter().map(|c| color(*c).to_string()));
    words.extend(s.not_types.iter().map(|t| format!("non{}", card_type(*t))));
    let types: Vec<String> = s.types.iter().map(|t| card_type(*t).to_string()).collect();
    words.extend(s.subtypes.iter().cloned());
    if !types.is_empty() {
        words.push(join(types, "or"));
    }
    words.join(" ")
}

fn target(mode: &Mode, r: TargetRef) -> String {
    match r {
        TargetRef::Target(i) => mode
            .targets
            .get(i as usize)
            .map_or("the target".to_string(), target_filter),
        TargetRef::Players(p) => player(mode, p),
        TargetRef::Object(o) => obj(mode, o),
    }
}

fn obj(mode: &Mode, r: ObjRef) -> String {
    match r {
        ObjRef::Target(i) => target(mode, TargetRef::Target(i)),
        ObjRef::This => "this permanent".into(),
        ObjRef::Triggering => "it".into(),
    }
}

fn player(mode: &Mode, r: PlayerRef) -> String {
    match r {
        PlayerRef::You => "you".into(),
        PlayerRef::Target(i) => target(mode, TargetRef::Target(i)),
        PlayerRef::EachOpponent => "each opponent".into(),
        PlayerRef::Each => "each player".into(),
        PlayerRef::TriggeringController => "its controller".into(),
    }
}

/// "you draw" vs "target player draws".
fn verb(who: &str, base: &str, third: &str) -> String {
    if who == "you" {
        format!("you {base}")
    } else {
        format!("{who} {third}")
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
    for c in &s.colors {
        out.push_str(&format!("{c:?} ").to_lowercase());
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
    // "Dinosaurs", "Dragon creatures", "creatures", "permanents".
    let subtypes = s.subtypes.join(" or ");
    let noun = match (s.types.is_empty(), subtypes.is_empty()) {
        (true, true) => "permanent".to_string(),
        (true, false) => subtypes,
        (false, true) => types(&s.types, "or"),
        (false, false) => format!("{subtypes} {}", types(&s.types, "or")),
    };
    out.push_str(&noun);
    if plural {
        out.push('s');
    }
    if s.token == Some(true) {
        out.push_str(if plural { " tokens" } else { " token" });
    }
    for k in &s.keywords {
        out.push_str(&format!(" with {}", keyword(*k)));
    }
    match (s.power_min, s.toughness_min) {
        (Some(p), _) => out.push_str(&format!(" with power {p} or greater")),
        (None, Some(t)) => out.push_str(&format!(" with toughness {t} or greater")),
        (None, None) => {}
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
    out
}

fn counter(k: CounterKind) -> &'static str {
    match k {
        CounterKind::PlusOne => "+1/+1",
        CounterKind::MinusOne => "-1/-1",
    }
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
        .map(|e| match e {
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
                let part = |s: &crate::Signed| {
                    format!(
                        "{}{}",
                        if s.negative { "-" } else { "+" },
                        amount(&s.amount)
                    )
                };
                format!("{get} {}/{}", part(power), part(toughness))
            }
            ContinuousEffect::SwitchPT => format!("{become_} power-and-toughness switched"),
        })
        .collect();
    join(parts, "and")
}

fn effect(mode: &Mode, e: &Effect) -> String {
    match e {
        Effect::Seq(v) => v
            .iter()
            .map(|x| effect(mode, x))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(". Then "),
        Effect::DealDamage { to, amount: a, by } => {
            let who = match by {
                DamageBy::This => "this",
                DamageBy::Triggering => "it",
            };
            format!("{who} deals {} damage to {}", amount(a), target(mode, *to))
        }
        Effect::DamageEachPlayer { amount: a } => {
            format!("this deals {} damage to each player", amount(a))
        }
        Effect::Draw { who, amount: a } => {
            let n = amount(a);
            let cards = if n == "1" {
                "a card".to_string()
            } else {
                format!("{n} cards")
            };
            format!("{} {cards}", verb(&player(mode, *who), "draw", "draws"))
        }
        Effect::GainLife { who, amount: a } => {
            format!(
                "{} {} life",
                verb(&player(mode, *who), "gain", "gains"),
                amount(a)
            )
        }
        Effect::LoseLife { who, amount: a } => {
            format!(
                "{} {} life",
                verb(&player(mode, *who), "lose", "loses"),
                amount(a)
            )
        }
        Effect::Counter { what } => format!("counter {}", target(mode, *what)),
        Effect::EachPlayer(a) => match a {
            EachPlayerAction::Discard(n) => format!("each player discards {n} card(s)"),
            EachPlayerAction::SacrificeCreature(n) => {
                format!("each player sacrifices {n} creature(s)")
            }
            EachPlayerAction::Draw(n) => format!("each player draws {n} card(s)"),
        },
        Effect::Destroy { what } => format!("destroy {}", obj(mode, *what)),
        Effect::ReturnToHand { what } => {
            format!("return {} to its owner's hand", obj(mode, *what))
        }
        Effect::CreateToken { token, count, who } => format!(
            "{} {} {token} token(s)",
            verb(&player(mode, *who), "create", "creates"),
            amount(count)
        ),
        Effect::PutCounters { what, kind, count } => format!(
            "put {} {} counter(s) on {}",
            amount(count),
            counter(*kind),
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
            "this deals {} damage to each {}",
            amount(a),
            selector(each, false)
        ),
        Effect::Exile { what } => format!("exile {}", obj(mode, *what)),
        Effect::ExileGraveyard { who } => match who {
            PlayerRef::You => "exile your graveyard".to_string(),
            other => format!("exile {}'s graveyard", player(mode, *other)),
        },
        Effect::Mill { who, amount: a } => {
            let n = amount(a);
            let cards = if n == "1" {
                "a card".to_string()
            } else {
                format!("{n} cards")
            };
            format!("{} {cards}", verb(&player(mode, *who), "mill", "mills"))
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
        Effect::SetX(a) => format!("X is {}", amount(a)),
        Effect::Tap { what } => format!("tap {}", obj(mode, *what)),
        Effect::Untap { what } => format!("untap {}", obj(mode, *what)),
        Effect::TapAll(s) => format!("tap all {}", selector(s, true)),
        Effect::UntapAll(s) => format!("untap all {}", selector(s, true)),
        Effect::Discard { who, amount: a } => {
            let n = amount(a);
            let cards = if n == "1" {
                "a card".to_string()
            } else {
                format!("{n} cards")
            };
            format!(
                "{} {cards}",
                verb(&player(mode, *who), "discard", "discards")
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
            let n = amount(a);
            let things = if n == "1" {
                with_article(&selector(what, false))
            } else {
                format!("{n} {}", selector(what, true))
            };
            format!(
                "{} {things}",
                verb(&player(mode, *who), "sacrifice", "sacrifices")
            )
        }
        Effect::SacrificeThis => "sacrifice this permanent".to_string(),
        Effect::May(inner) => format!("you may {}", effect(mode, inner)),
        Effect::ReturnToBattlefield { what } => {
            format!("return {} to the battlefield", obj(mode, *what))
        }
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
        Body::Effect(m) => sentence(effect(m, &m.effect)),
        Body::Modal { min, max, modes } => {
            let n = if min == max {
                format!("Choose {min}")
            } else {
                format!("Choose {min} to {max}")
            };
            let list: Vec<String> = modes
                .iter()
                .map(|m| sentence(effect(m, &m.effect)))
                .collect();
            format!("{n} — {}", list.join(" • "))
        }
    }
}

/// "creature card", "Zombie card", "card".
fn card_noun(s: &Selector) -> String {
    let mut words: Vec<String> = s.subtypes.clone();
    if !s.types.is_empty() {
        words.push(types(&s.types, "or"));
    }
    words.push("card".into());
    words.join(" ")
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn cost(c: &Cost) -> String {
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
        let what = join(what, "or");
        if c.count == 1 {
            with_article(&what)
        } else if cards {
            format!("{} {what}s", c.count)
        } else {
            format!("{} {what}", c.count)
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
    parts.join(", ")
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
            if s.other { text } else { format!("a {text}") }
        }
        ObjFilter::ThisOr(s) => format!("this permanent or another {}", selector(s, false)),
    }
}

fn trigger(t: &Trigger) -> String {
    match t {
        Trigger::Attacks(f) => format!("Whenever {} attacks", obj_filter(f)),
        Trigger::Any(v) => {
            // "Whenever this permanent enters or attacks".
            let parts: Vec<String> = v.iter().map(trigger).collect();
            let mut out = parts.first().cloned().unwrap_or_default();
            for p in parts.iter().skip(1) {
                let words: Vec<&str> = p.split(' ').collect();
                out.push_str(&format!(" or {}", words.last().copied().unwrap_or("")));
            }
            out
        }
        Trigger::Enters(f) => format!("Whenever {} enters", obj_filter(f)),
        Trigger::Dies(f) => format!("Whenever {} dies", obj_filter(f)),
        Trigger::BeginningOf { step, whose } => format!(
            "At the beginning of {} {}",
            who_possessive(*whose),
            match step {
                TriggerStep::Upkeep => "upkeep",
                TriggerStep::DrawStep => "draw step",
                TriggerStep::EndStep => "end step",
            }
        ),
        Trigger::Cast { by, types: ts } => {
            let kinds = types(ts, "or");
            let article = if kinds.starts_with(['a', 'e', 'i', 'o', 'u']) {
                "an"
            } else {
                "a"
            };
            format!(
                "Whenever {} {article} {}spell",
                match by {
                    Who::Any => "a player casts",
                    Who::You => "you cast",
                    Who::Opponent => "an opponent casts",
                },
                if kinds.is_empty() {
                    String::new()
                } else {
                    format!("{kinds} ")
                }
            )
        }
    }
}

fn condition(c: &Condition) -> String {
    match c {
        Condition::LifeAtMost(n) => format!("if you have {n} or less life"),
        Condition::LifeAtLeast(n) => format!("if you have {n} or more life"),
        Condition::CardsInGraveyard { cards, at_least } => format!(
            "if you have {at_least} or more {}s in your graveyard",
            card_noun(cards)
        ),
        Condition::Control { what, at_least } => {
            if *at_least == 1 {
                format!(
                    "if you control a {}",
                    selector(what, false).replace(" you control", "")
                )
            } else {
                format!(
                    "if you control {at_least} or more {}",
                    selector(what, true).replace(" you control", "")
                )
            }
        }
    }
}

fn ability(a: &AbilityDef) -> String {
    match a {
        AbilityDef::Spell(b) => body(b),
        AbilityDef::Activated {
            cost: c,
            timing,
            only_if,
            body: b,
        } => format!(
            "{}: {}{}{}",
            cost(c),
            body(b),
            match timing {
                Timing::Instant => "",
                Timing::Sorcery => " Activate only as a sorcery.",
            },
            only_if.as_ref().map_or(String::new(), |c| format!(
                " Activate only {}.",
                condition(c)
            ))
        ),
        AbilityDef::Mana { cost: c, produce } => format!(
            "{}: Add {}.",
            cost(c),
            match produce {
                ManaProduce::One(t) => t.symbol().to_string(),
                ManaProduce::AnyColor => "one mana of any color".to_string(),
            }
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
            let mut lower = text.clone();
            if let Some(first) = lower.chars().next() {
                let l: String = first.to_lowercase().collect();
                lower.replace_range(..first.len_utf8(), &l);
            }
            format!("{}{cond}, {lower}", trigger(when))
        }
        AbilityDef::Static { affected, effects } => {
            let (subject, plural) = match affected {
                Affected::This => ("This permanent".to_string(), false),
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
                return format!("You control {}.", subject.to_lowercase());
            }
            format!("{subject} {}.", continuous(effects, plural))
        }
        AbilityDef::Replacement { replaces, with } => {
            let what = match replaces {
                Replaces::CreateTokens => "If an effect would create tokens under your control",
                Replaces::PutCountersOnYourPermanents => {
                    "If an effect would put counters on a permanent you control"
                }
                Replaces::CardToGraveyard => {
                    "If a card would be put into a graveyard from anywhere"
                }
                Replaces::ThisDies => "If this creature would die",
                Replaces::ThisEnters => "This permanent",
            };
            match with {
                ReplaceWith::Twice => format!("{what}, it does so twice as many instead."),
                ReplaceWith::Exile => format!("{what}, exile it instead."),
                ReplaceWith::ShuffleIntoLibrary => {
                    format!("{what}, shuffle it into its owner's library instead.")
                }
                ReplaceWith::EntersTapped => format!("{what} enters tapped."),
                ReplaceWith::EntersWithCounters { kind, n } => {
                    format!(
                        "{what} enters with {n} {} counter(s) on it.",
                        counter(*kind)
                    )
                }
            }
        }
        AbilityDef::CostReduction { spells, amount: n } => format!(
            "{} spells you cast cost {{{n}}} less to cast.",
            capitalize(selector(spells, false).replace("permanent", "").trim())
        ),
        AbilityDef::AdditionalCost(c) => {
            let mut text = cost(c);
            if let Some(first) = text.chars().next() {
                let lower: String = first.to_lowercase().collect();
                text.replace_range(..first.len_utf8(), &lower);
            }
            format!("As an additional cost to cast this spell, {text}.")
        }
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
    }
}

/// One mode of a modal spell or ability, as a sentence (for the mode-choice UI).
pub fn mode_text(m: &Mode) -> String {
    sentence(effect(m, &m.effect))
}

/// The card's rules text, one line per keyword group / ability.
pub fn rules_text(def: &CardDef) -> Vec<String> {
    let mut lines = Vec::new();
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
    lines.extend(def.abilities.iter().map(ability));
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
