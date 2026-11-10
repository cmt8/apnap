//! Compile a Forge card script into a [`CardDef`] (decisions.md D-046).
//!
//! The rule is strict: a card compiles only if **every** line, parameter, filter property
//! and expression of its script is understood. Anything else is reported as a *blocker* (a
//! short feature name such as `api:Fight` or `param:Pump.Duration`), never dropped or
//! guessed. `harness forge-coverage` counts blockers over all cards to decide what to build
//! next.
//!
//! What each script feature means is documented in docs/forge/ (written from Forge's
//! sources); what it compiles to here must have a test in this crate or in the engine.

use super::parse::{Face, Record, Script};
use card_model::{
    AbilityDef, Affected, Amount, BasicLandType, Body, CardDef, CardType, Color, Condition,
    ContinuousEffect, Cost, CounterKind, DamageBy, EachPlayerAction, Effect, Keyword, ManaCost,
    ManaProduce, ManaType, Mode, ObjFilter, ObjRef, PlayerRef, ReplaceWith, Replaces, Selector,
    Supertype, TargetFilter, TargetRef, Timing, Trigger, TriggerStep, Who,
};
use std::cell::RefCell;
use std::collections::BTreeSet;

/// Why a script doesn't compile: the features it needs that aren't supported yet.
pub type Blockers = BTreeSet<String>;

/// A compiled card, and the token scripts its effects create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    pub def: CardDef,
    pub tokens: BTreeSet<String>,
}

/// Parameters that never affect the rules: descriptions shown to players, prompts, and
/// hints for Forge's AI. (docs/forge/script-format.md, "keys a translator can ignore".)
const TEXT_ONLY: [&str; 9] = [
    "SpellDescription",
    "TriggerDescription",
    "StackDescription",
    "Description",
    "ValidDescription",
    "TgtPrompt",
    "AILogic",
    "IsCurse",
    "PrecostDesc",
];

/// A record being read: remembers which parameters were looked at, so that any that
/// weren't can be reported.
struct Rd<'a> {
    rec: &'a Record,
    what: String,
    seen: RefCell<BTreeSet<&'a str>>,
}

impl<'a> Rd<'a> {
    fn new(rec: &'a Record, what: impl Into<String>) -> Rd<'a> {
        Rd {
            rec,
            what: what.into(),
            seen: RefCell::new(BTreeSet::new()),
        }
    }

    fn get(&self, key: &'a str) -> Option<&'a str> {
        self.seen.borrow_mut().insert(key);
        self.rec.get(key)
    }

    /// Report every parameter that wasn't read.
    fn finish(&self, blockers: &mut Blockers) {
        let seen = self.seen.borrow();
        for (k, _) in &self.rec.params {
            let k = k.as_str();
            if seen.contains(k)
                || TEXT_ONLY.contains(&k)
                || matches!(k, "SP" | "AB" | "DB" | "ST" | "Mode" | "Event")
            {
                continue;
            }
            blockers.insert(format!("param:{}.{k}", self.what));
        }
    }
}

/// One alternative of a filter ("Zombie.Other+YouCtrl").
#[derive(Clone, Debug, PartialEq, Eq)]
enum Alt {
    /// `Card.Self`
    This,
    Sel(Selector),
}

/// The targets and effects of one mode, as they are collected along a sub-ability chain.
#[derive(Default)]
struct Chain {
    targets: Vec<TargetFilter>,
    effects: Vec<Effect>,
    /// "…where X is [amount]": the script variable that `Amount::X` stands for.
    x: Option<(String, Amount)>,
}

struct Tx<'a> {
    face: &'a Face,
    blockers: Blockers,
    used_svars: BTreeSet<String>,
    tokens: BTreeSet<String>,
    has_x_cost: bool,
    /// The script variables that hold abilities, parsed.
    records: &'a [(String, Record)],
    known: &'a Known,
}

/// What the compiler needs to know about the whole card pool.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Known {
    /// Every subtype (creature type, land type, artifact type, …) that exists. A word in a
    /// filter is taken as a subtype only if it is one of these — otherwise it is some
    /// property the compiler doesn't know, and the card is blocked rather than misread.
    pub subtypes: BTreeSet<String>,
}

impl Known {
    /// The subtypes used on the type lines of `scripts` (card or token script texts).
    pub fn from_scripts<'s>(scripts: impl Iterator<Item = &'s str>) -> Known {
        let mut subtypes = BTreeSet::new();
        for text in scripts {
            for line in text.lines() {
                let Some(words) = line.strip_prefix("Types:") else {
                    continue;
                };
                for w in words.split_whitespace() {
                    let super_or_card = card_type(w).is_some()
                        || matches!(
                            w,
                            "Basic" | "Legendary" | "Snow" | "World" | "Ongoing" | "Tribal"
                        );
                    if !super_or_card {
                        subtypes.insert(w.to_string());
                    }
                }
            }
        }
        Known { subtypes }
    }
}

const CARD_TYPES: [(&str, CardType); 9] = [
    ("Artifact", CardType::Artifact),
    ("Battle", CardType::Battle),
    ("Creature", CardType::Creature),
    ("Enchantment", CardType::Enchantment),
    ("Instant", CardType::Instant),
    ("Kindred", CardType::Kindred),
    ("Land", CardType::Land),
    ("Planeswalker", CardType::Planeswalker),
    ("Sorcery", CardType::Sorcery),
];

fn card_type(word: &str) -> Option<CardType> {
    CARD_TYPES.iter().find(|(n, _)| *n == word).map(|(_, t)| *t)
}

fn color(word: &str) -> Option<Color> {
    Some(match word {
        "White" => Color::White,
        "Blue" => Color::Blue,
        "Black" => Color::Black,
        "Red" => Color::Red,
        "Green" => Color::Green,
        _ => return None,
    })
}

/// The keywords the engine implements, by their name in a `K:` line or a `KW$` parameter.
pub fn keyword(name: &str) -> Option<Keyword> {
    Some(match name {
        "Deathtouch" => Keyword::Deathtouch,
        "Double Strike" => Keyword::DoubleStrike,
        "First Strike" => Keyword::FirstStrike,
        "Flash" => Keyword::Flash,
        "Flying" => Keyword::Flying,
        "Haste" => Keyword::Haste,
        "Lifelink" => Keyword::Lifelink,
        "Menace" => Keyword::Menace,
        "Reach" => Keyword::Reach,
        "Trample" => Keyword::Trample,
        "Vigilance" => Keyword::Vigilance,
        _ => return None,
    })
}

/// `ManaCost:` → our cost text. `Ok(None)` for "no cost".
pub fn mana_cost(text: &str) -> Result<Option<ManaCost>, String> {
    if text == "no cost" {
        return Ok(None);
    }
    let mut out = String::new();
    for shard in text.split_whitespace() {
        let ok = shard.parse::<u32>().is_ok()
            || matches!(shard, "W" | "U" | "B" | "R" | "G" | "C" | "X");
        if !ok {
            return Err(format!("mana:{shard}"));
        }
        out.push_str(&format!("{{{shard}}}"));
    }
    ManaCost::parse(&out)
        .map(Some)
        .map_err(|_| format!("mana:{text}"))
}

impl<'a> Tx<'a> {
    fn block(&mut self, what: impl Into<String>) {
        self.blockers.insert(what.into());
    }

    fn svar(&mut self, name: &str) -> Option<&'a str> {
        let v = self.face.svars.get(name)?;
        self.used_svars.insert(name.to_string());
        Some(v.as_str())
    }

    // ------------------------------------------------------------------ filters

    /// One alternative of a filter. `None` (with a blocker recorded) if not understood.
    fn alt(&mut self, text: &str) -> Option<Alt> {
        let mut parts = text.split('.');
        let head = parts.next().unwrap_or("").trim();
        let props: Vec<&str> = parts
            .flat_map(|p| p.split('+'))
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let mut s = Selector::default();
        match head {
            "Card" | "Permanent" => {}
            _ => match card_type(head) {
                Some(t) => s.types.push(t),
                None if self.known.subtypes.contains(head) => s.subtypes.push(head.to_string()),
                None => {
                    self.block(format!("filter:{head}"));
                    return None;
                }
            },
        }
        let mut this = false;
        let mut ok = true;
        for p in props {
            match p {
                "Self" => this = true,
                "Other" => s.other = true,
                "YouCtrl" | "YouOwn" => s.controller = Who::You,
                "OppCtrl" | "OppOwn" => s.controller = Who::Opponent,
                "!token" | "nonToken" => s.token = Some(false),
                "token" => s.token = Some(true),
                "tapped" => s.tapped = Some(true),
                "untapped" => s.tapped = Some(false),
                _ => {
                    if let Some(c) = color(p) {
                        s.colors.push(c);
                    } else if let Some(n) = p.strip_prefix("powerGE").and_then(|n| n.parse().ok()) {
                        s.power_min = Some(n);
                    } else if let Some(n) =
                        p.strip_prefix("toughnessGE").and_then(|n| n.parse().ok())
                    {
                        s.toughness_min = Some(n);
                    } else if let Some(k) = p.strip_prefix("with").and_then(keyword) {
                        s.keywords.push(k);
                    } else if let Some(x) = p.strip_prefix("non") {
                        match card_type(x) {
                            Some(t) => s.not_types.push(t),
                            None if self.known.subtypes.contains(x) => {
                                s.not_subtypes.push(x.to_string())
                            }
                            None => {
                                self.block(format!("filter:.{p}"));
                                ok = false;
                            }
                        }
                    } else if self.known.subtypes.contains(p) && s.subtypes.is_empty() {
                        // "Creature.Zombie": a subtype as a restriction.
                        s.subtypes.push(p.to_string());
                    } else {
                        let shape: String = p
                            .chars()
                            .map(|c| if c.is_ascii_digit() { 'N' } else { c })
                            .collect();
                        self.block(format!("filter:.{shape}"));
                        ok = false;
                    }
                }
            }
        }
        if !ok {
            return None;
        }
        if this {
            // `Card.Self` and nothing else.
            if head == "Card" && s == Selector::default() {
                return Some(Alt::This);
            }
            self.block("filter:.Self+");
            return None;
        }
        Some(Alt::Sel(s))
    }

    fn alts(&mut self, text: &str) -> Option<Vec<Alt>> {
        text.split(',').map(|a| self.alt(a.trim())).collect()
    }

    /// A filter that must be a single selector. Alternatives that are just different card
    /// types ("Artifact,Enchantment") are one selector with several types.
    fn selector(&mut self, text: &str) -> Option<Selector> {
        let alts = self.alts(text)?;
        let sels: Vec<Selector> = alts
            .into_iter()
            .map(|a| match a {
                Alt::Sel(s) => Some(s),
                Alt::This => None,
            })
            .collect::<Option<_>>()
            .or_else(|| {
                self.block("filter:Self-in-selector");
                None
            })?;
        merge(sels).or_else(|| {
            self.block("filter:alternatives");
            None
        })
    }

    /// The filter of a trigger's `ValidCard$`.
    fn obj_filter(&mut self, text: &str) -> Option<ObjFilter> {
        let alts = self.alts(text)?;
        match alts.as_slice() {
            [Alt::This] => Some(ObjFilter::This),
            [Alt::Sel(s)] => Some(ObjFilter::Matching(s.clone())),
            // "this or another X", in either order.
            [Alt::This, Alt::Sel(s)] | [Alt::Sel(s), Alt::This] if s.other => {
                let mut s = s.clone();
                s.other = false;
                Some(ObjFilter::ThisOr(s))
            }
            _ => {
                self.block("filter:trigger-alternatives");
                None
            }
        }
    }

    // ------------------------------------------------------------------ amounts

    /// A numeric parameter of an effect in a chain.
    fn amount(&mut self, text: &str, chain: &mut Chain) -> Option<Amount> {
        if let Ok(n) = text.trim_start_matches('+').parse::<u32>() {
            return Some(Amount::Fixed(n));
        }
        let a = self.expr(text)?;
        match a {
            // The announced X of the cost, or a property of the triggering object: used
            // directly.
            Amount::X | Amount::TriggeringPower | Amount::TriggeringToughness => Some(a),
            // Read off the battlefield: worked out once for the whole resolution (CR
            // 608.2h), as "X".
            _ => match &chain.x {
                None => {
                    chain.x = Some((text.to_string(), a));
                    Some(Amount::X)
                }
                Some((name, _)) if name == text => Some(Amount::X),
                Some(_) => {
                    self.block("expr:two-variables");
                    None
                }
            },
        }
    }

    /// The value of a script variable (`SVar:X:Count$…`).
    fn expr(&mut self, name: &str) -> Option<Amount> {
        let Some(v) = self.svar(name) else {
            self.block(format!("expr:undefined {name}"));
            return None;
        };
        if v == "Count$xPaid" && self.has_x_cost {
            return Some(Amount::X);
        }
        if v == "TriggeredCard$CardPower" {
            return Some(Amount::TriggeringPower);
        }
        if v == "TriggeredCard$CardToughness" {
            return Some(Amount::TriggeringToughness);
        }
        if let Some(rest) = v.strip_prefix("Count$Valid ") {
            return match rest.split_once('$') {
                None if !rest.contains('/') => self.selector(rest).map(Amount::Count),
                Some((f, "CardPower")) => self.selector(f).map(Amount::TotalPower),
                _ => {
                    self.block(format!("expr:{}", shape(v)));
                    None
                }
            };
        }
        self.block(format!("expr:{}", shape(v)));
        None
    }

    // ------------------------------------------------------------------ references

    fn player(&mut self, rd: &Rd<'a>, chain: &Chain) -> Option<PlayerRef> {
        match rd.get("Defined") {
            None | Some("You") => Some(PlayerRef::You),
            Some("Player.Opponent" | "Opponent") => Some(PlayerRef::EachOpponent),
            Some("Targeted") => match chain.targets.len() {
                0 => {
                    self.block("defined:Targeted-without-target");
                    None
                }
                n => Some(PlayerRef::Target(n as u8 - 1)),
            },
            Some(other) => {
                self.block(format!("defined:player {other}"));
                None
            }
        }
    }

    fn object(&mut self, rd: &Rd<'a>, chain: &Chain, triggered: bool) -> Option<ObjRef> {
        match rd.get("Defined") {
            None | Some("Self") => Some(ObjRef::This),
            Some("TriggeredCard" | "TriggeredCardLKICopy") if triggered => Some(ObjRef::Triggering),
            Some("Targeted") if !chain.targets.is_empty() => {
                Some(ObjRef::Target(chain.targets.len() as u8 - 1))
            }
            Some(other) => {
                self.block(format!("defined:card {other}"));
                None
            }
        }
    }

    /// `ValidTgts$` of an effect: adds a target slot and returns its index.
    fn target(&mut self, rd: &Rd<'a>, chain: &mut Chain, graveyard: bool) -> Option<u8> {
        let text = rd.get("ValidTgts")?;
        for k in ["TargetMin", "TargetMax", "TargetUnique", "TargetType"] {
            if rd.rec.get(k).is_some() {
                self.block(format!("param:target.{k}"));
                return None;
            }
        }
        let filter = if graveyard {
            let mut s = self.selector(text)?;
            let whose = std::mem::take(&mut s.controller);
            TargetFilter::CardInGraveyard { cards: s, whose }
        } else {
            match text {
                "Any" => TargetFilter::AnyTarget,
                "Player" => TargetFilter::Player,
                "Opponent" | "Player.Opponent" => TargetFilter::Opponent,
                "Creature" => TargetFilter::Creature,
                _ => {
                    let alts = self.alts(text)?;
                    let sels: Option<Vec<Selector>> = alts
                        .into_iter()
                        .map(|a| match a {
                            Alt::Sel(s) => Some(s),
                            Alt::This => None,
                        })
                        .collect();
                    let Some(sels) = sels else {
                        self.block("filter:Self-as-target");
                        return None;
                    };
                    TargetFilter::Permanent(merge_bare(sels))
                }
            }
        };
        chain.targets.push(filter);
        Some(chain.targets.len() as u8 - 1)
    }

    // ------------------------------------------------------------------ effects

    /// One effect record (not following its sub-ability). `None` = blocked.
    fn effect(&mut self, rec: &'a Record, chain: &mut Chain, triggered: bool) -> Option<Effect> {
        let Some((_, api)) = rec.head() else {
            self.block("record:no-api");
            return None;
        };
        let rd = Rd::new(rec, api);
        rd.get("SubAbility");
        rd.get("Cost");
        if rec.head().map(|h| h.0) == Some("AB") {
            // Read by `ability`: when and whether the ability can be activated.
            for k in ["IsPresent", "PresentCompare", "PresentZone", "SorcerySpeed"] {
                rd.get(k);
            }
        }
        let out = match api {
            "DealDamage" => {
                let by = match rd.get("DamageSource") {
                    None => DamageBy::This,
                    Some("TriggeredCard") if triggered => DamageBy::Triggering,
                    Some(other) => {
                        self.block(format!("param:DealDamage.DamageSource={other}"));
                        return None;
                    }
                };
                let slot = self.target(&rd, chain, false).or_else(|| {
                    self.block("DealDamage:no-target");
                    None
                });
                let amount = rd.get("NumDmg").and_then(|n| self.amount(n, chain));
                Effect::DealDamage {
                    to: TargetRef::Target(slot?),
                    amount: amount?,
                    by,
                }
            }
            "Draw" | "Mill" | "GainLife" | "LoseLife" => {
                let key = match api {
                    "Draw" | "Mill" => "NumCards",
                    _ => "LifeAmount",
                };
                // "each player draws": its own effect.
                if api == "Draw" && rd.rec.get("Defined") == Some("Player") {
                    rd.get("Defined");
                    let n = rd.get(key).unwrap_or("1").parse::<u32>().ok().or_else(|| {
                        self.block("Draw:each-player-variable");
                        None
                    });
                    rd.finish(&mut self.blockers);
                    return n.map(|n| Effect::EachPlayer(EachPlayerAction::Draw(n)));
                }
                let slot = self.target(&rd, chain, false);
                let who = match slot {
                    Some(i) => Some(PlayerRef::Target(i)),
                    None => self.player(&rd, chain),
                };
                let amount = self.amount(rd.get(key).unwrap_or("1"), chain);
                let (who, amount) = (who?, amount?);
                match api {
                    "Draw" => Effect::Draw { who, amount },
                    "Mill" => Effect::Mill { who, amount },
                    "GainLife" => Effect::GainLife { who, amount },
                    _ => Effect::LoseLife { who, amount },
                }
            }
            "Destroy" => {
                let slot = self.target(&rd, chain, false).or_else(|| {
                    self.block("Destroy:no-target");
                    None
                });
                Effect::Destroy {
                    what: ObjRef::Target(slot?),
                }
            }
            "DestroyAll" => {
                let sel = rd.get("ValidCards").and_then(|f| self.selector(f));
                Effect::DestroyAll(sel?)
            }
            "DamageAll" => {
                let sel = rd.get("ValidCards").and_then(|f| self.selector(f));
                let amount = rd.get("NumDmg").and_then(|n| self.amount(n, chain));
                Effect::DamageEach {
                    each: sel?,
                    amount: amount?,
                }
            }
            "ChangeZone" => {
                let origin = rd.get("Origin").unwrap_or("Battlefield");
                let dest = rd.get("Destination");
                let slot = self.target(&rd, chain, origin == "Graveyard").or_else(|| {
                    self.block("ChangeZone:no-target");
                    None
                });
                let what = ObjRef::Target(slot?);
                match (origin, dest) {
                    ("Battlefield" | "Graveyard", Some("Exile")) => Effect::Exile { what },
                    ("Battlefield" | "Graveyard", Some("Hand")) => Effect::ReturnToHand { what },
                    ("Graveyard", Some("Battlefield")) => Effect::ReturnToBattlefield { what },
                    (o, d) => {
                        self.block(format!("ChangeZone:{o}->{}", d.unwrap_or("?")));
                        return None;
                    }
                }
            }
            "ChangeZoneAll" => {
                let shape = (
                    rd.get("Origin"),
                    rd.get("Destination"),
                    rd.get("ChangeType"),
                );
                let slot = self.target(&rd, chain, false);
                match (shape, slot) {
                    ((Some("Graveyard"), Some("Exile"), Some("Card")), Some(i))
                        if chain.targets[i as usize] == TargetFilter::Player =>
                    {
                        Effect::ExileGraveyard {
                            who: PlayerRef::Target(i),
                        }
                    }
                    _ => {
                        self.block("ChangeZoneAll:shape");
                        return None;
                    }
                }
            }
            "Token" => {
                let script = rd.get("TokenScript");
                let count = self.amount(rd.get("TokenAmount").unwrap_or("1"), chain);
                match rd.get("TokenOwner") {
                    None | Some("You") => {}
                    Some(other) => {
                        self.block(format!("param:Token.TokenOwner={other}"));
                        return None;
                    }
                }
                let Some(script) = script else {
                    self.block("Token:no-script");
                    return None;
                };
                self.tokens.insert(script.to_string());
                Effect::CreateToken {
                    token: script.to_string(),
                    count: count?,
                    who: PlayerRef::You,
                }
            }
            "PutCounter" => {
                let kind = match rd.get("CounterType") {
                    Some("P1P1") => CounterKind::PlusOne,
                    Some("M1M1") => CounterKind::MinusOne,
                    other => {
                        self.block(format!("counter:{}", other.unwrap_or("?")));
                        return None;
                    }
                };
                let slot = self.target(&rd, chain, false);
                let what = match slot {
                    Some(i) => Some(ObjRef::Target(i)),
                    None => self.object(&rd, chain, triggered),
                };
                let count = self.amount(rd.get("CounterNum").unwrap_or("1"), chain);
                Effect::PutCounters {
                    what: what?,
                    kind,
                    count: count?,
                }
            }
            "Pump" | "PumpAll" => {
                let effects = self.pump_effects(&rd)?;
                if api == "PumpAll" {
                    let sel = rd.get("ValidCards").and_then(|f| self.selector(f));
                    Effect::ApplyAll {
                        affected: sel?,
                        effects,
                        until_end_of_turn: true,
                    }
                } else {
                    let slot = self.target(&rd, chain, false);
                    let what = match slot {
                        Some(i) => Some(ObjRef::Target(i)),
                        None => self.object(&rd, chain, triggered),
                    };
                    Effect::Apply {
                        what: what?,
                        effects,
                        until_end_of_turn: true,
                    }
                }
            }
            "Sacrifice" => {
                // "Each player sacrifices N creatures of their choice."
                let shape = (rd.get("Defined"), rd.get("SacValid"));
                let n = rd.get("Amount").unwrap_or("1").parse::<u32>().ok();
                match (shape, n) {
                    ((Some("Player"), Some("Creature")), Some(n)) => {
                        Effect::EachPlayer(EachPlayerAction::SacrificeCreature(n))
                    }
                    _ => {
                        self.block("Sacrifice:shape");
                        return None;
                    }
                }
            }
            "Discard" => {
                // "Each player discards N cards" (of their choice).
                let shape = (rd.get("Defined"), rd.get("Mode"));
                let n = rd.get("NumCards").unwrap_or("1").parse::<u32>().ok();
                match (shape, n) {
                    ((Some("Player"), Some("TgtChoose")), Some(n)) => {
                        Effect::EachPlayer(EachPlayerAction::Discard(n))
                    }
                    _ => {
                        self.block("Discard:shape");
                        return None;
                    }
                }
            }
            other => {
                self.block(format!("api:{other}"));
                return None;
            }
        };
        rd.finish(&mut self.blockers);
        Some(out)
    }

    /// `NumAtt$` / `NumDef$` / `KW$` of Pump and PumpAll (which last until end of turn).
    fn pump_effects(&mut self, rd: &Rd<'a>) -> Option<Vec<ContinuousEffect>> {
        let int =
            |t: Option<&str>| t.map_or(Some(0), |t| t.trim_start_matches('+').parse::<i32>().ok());
        let (p, t) = (int(rd.get("NumAtt")), int(rd.get("NumDef")));
        let (Some(p), Some(t)) = (p, t) else {
            self.block("Pump:variable-amount");
            return None;
        };
        let mut effects = Vec::new();
        if (p, t) != (0, 0) {
            effects.push(ContinuousEffect::ModifyPT(p, t));
        }
        if let Some(kw) = rd.get("KW") {
            let mut ks = Vec::new();
            for k in kw.split('&').map(str::trim) {
                match keyword(k) {
                    Some(k) => ks.push(k),
                    None => {
                        self.block(format!("kw:{k}"));
                        return None;
                    }
                }
            }
            effects.push(ContinuousEffect::AddKeywords(ks));
        }
        if effects.is_empty() {
            self.block("Pump:nothing");
            return None;
        }
        Some(effects)
    }

    /// A whole chain (an ability and its `SubAbility$` descendants) as one mode.
    fn mode(&mut self, first: &'a Record, triggered: bool) -> Option<Mode> {
        let mut chain = Chain::default();
        let mut next = Some(first);
        let mut ok = true;
        let mut steps = 0;
        while let Some(rec) = next {
            steps += 1;
            if steps > 32 {
                self.block("chain:too-long");
                return None;
            }
            match self.effect(rec, &mut chain, triggered) {
                Some(e) => chain.effects.push(e),
                None => ok = false,
            }
            next = match rec.get("SubAbility") {
                None => None,
                Some(name) => match self.sub(name) {
                    Some(r) => Some(r),
                    None => {
                        ok = false;
                        None
                    }
                },
            };
        }
        if !ok {
            return None;
        }
        let mut effects = chain.effects;
        if let Some((_, amount)) = chain.x {
            effects.insert(0, Effect::SetX(amount));
        }
        let effect = if effects.len() == 1 {
            effects.remove(0)
        } else {
            Effect::Seq(effects)
        };
        Some(Mode {
            targets: chain.targets,
            effect,
        })
    }

    /// The record of a script variable that holds an ability.
    fn sub(&mut self, name: &str) -> Option<&'a Record> {
        self.used_svars.insert(name.to_string());
        let records = self.records;
        records
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, r)| r)
            .or_else(|| {
                self.block(format!("svar:missing {name}"));
                None
            })
    }

    /// The body of a spell, activated ability or trigger: a single mode, or a Charm.
    fn body(&mut self, rec: &'a Record, triggered: bool) -> Option<Body> {
        if rec.head().map(|h| h.1) != Some("Charm") {
            return self.mode(rec, triggered).map(Body::Effect);
        }
        let rd = Rd::new(rec, "Charm");
        rd.get("Cost");
        let names: Vec<&str> = rd
            .get("Choices")
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .collect();
        let max = rd.get("CharmNum").unwrap_or("1").parse::<u8>().ok();
        let min = match rd.get("MinCharmNum") {
            None => max,
            Some(n) => n.parse::<u8>().ok(),
        };
        rd.finish(&mut self.blockers);
        let (Some(min), Some(max)) = (min, max) else {
            self.block("Charm:variable-count");
            return None;
        };
        let mut modes = Vec::new();
        for n in names {
            let rec = self.sub(n)?;
            modes.push(self.mode(rec, triggered)?);
        }
        Some(Body::Modal { min, max, modes })
    }

    // ------------------------------------------------------------------ conditions, costs

    /// `IsPresent$ <filter>` (+ `PresentCompare$ GE<n>`, `PresentZone$ Graveyard`).
    fn condition(&mut self, rd: &Rd<'a>) -> Option<Option<Condition>> {
        let Some(filter) = rd.get("IsPresent") else {
            return Some(None);
        };
        let at_least = match rd.get("PresentCompare") {
            None => Some(1),
            Some(c) => c.strip_prefix("GE").and_then(|n| n.parse::<u32>().ok()),
        };
        let zone = rd.get("PresentZone");
        let sel = self.selector(filter);
        let (Some(at_least), Some(mut sel)) = (at_least, sel) else {
            self.block("condition:compare");
            return None;
        };
        match zone {
            None => Some(Some(Condition::Control {
                what: sel,
                at_least,
            })),
            Some("Graveyard") if sel.controller == Who::You => {
                sel.controller = Who::Any;
                Some(Some(Condition::CardsInGraveyard {
                    cards: sel,
                    at_least,
                }))
            }
            Some(z) => {
                self.block(format!("condition:zone {z}"));
                None
            }
        }
    }

    fn cost(&mut self, text: &str) -> Option<Cost> {
        let mut cost = Cost::default();
        let mut mana = String::new();
        for tok in text.split_whitespace() {
            match tok {
                "T" => cost.tap = true,
                "Sac<1/CARDNAME>" => cost.sacrifice_self = true,
                _ if tok.parse::<u32>().is_ok()
                    || matches!(tok, "W" | "U" | "B" | "R" | "G" | "C") =>
                {
                    mana.push_str(&format!("{{{tok}}}"))
                }
                _ => {
                    let kind = tok.split('<').next().unwrap_or(tok);
                    self.block(format!("cost:{kind}"));
                    return None;
                }
            }
        }
        if !mana.is_empty() {
            cost.mana = ManaCost::parse(&mana).ok();
        }
        Some(cost)
    }

    // ------------------------------------------------------------------ line kinds

    fn ability(&mut self, rec: &'a Record) -> Option<AbilityDef> {
        let (kind, api) = rec.head().or_else(|| {
            self.block("ability:no-head");
            None
        })?;
        match kind {
            "SP" => {
                if rec.get("Cost").is_some() {
                    self.block("spell:Cost");
                    return None;
                }
                self.body(rec, false).map(AbilityDef::Spell)
            }
            "AB" if api == "Mana" => {
                let rd = Rd::new(rec, "Mana");
                let cost = rd.get("Cost").and_then(|c| self.cost(c));
                let produce = match (rd.get("Produced"), rd.get("Amount")) {
                    (Some("Any"), None) => Some(ManaProduce::AnyColor),
                    (Some(p), None) => match p {
                        "W" => Some(ManaType::White),
                        "U" => Some(ManaType::Blue),
                        "B" => Some(ManaType::Black),
                        "R" => Some(ManaType::Red),
                        "G" => Some(ManaType::Green),
                        "C" => Some(ManaType::Colorless),
                        _ => None,
                    }
                    .map(ManaProduce::One),
                    _ => None,
                };
                rd.finish(&mut self.blockers);
                let Some(produce) = produce else {
                    self.block("Mana:produced");
                    return None;
                };
                let cost = cost?;
                if !(cost.tap && cost.mana.is_none() && !cost.sacrifice_self) {
                    self.block("Mana:cost");
                    return None;
                }
                Some(AbilityDef::Mana { cost, produce })
            }
            "AB" => {
                let rd = Rd::new(rec, "activated");
                let cost = rd.get("Cost").and_then(|c| self.cost(c));
                let only_if = self.condition(&rd);
                let timing = match rd.get("SorcerySpeed") {
                    None => Timing::Instant,
                    Some("True") => Timing::Sorcery,
                    Some(_) => {
                        self.block("param:activated.SorcerySpeed");
                        return None;
                    }
                };
                let body = self.body(rec, false);
                Some(AbilityDef::Activated {
                    cost: cost?,
                    timing,
                    only_if: only_if?,
                    body: body?,
                })
            }
            _ => {
                self.block(format!("ability:{kind}"));
                None
            }
        }
    }

    fn trigger_event(&mut self, rd: &Rd<'a>) -> Option<Trigger> {
        match rd.rec.get("Mode")? {
            "ChangesZone" => {
                let origin = rd.get("Origin").unwrap_or("Any");
                let dest = rd.get("Destination");
                let filter = rd.get("ValidCard").and_then(|f| self.obj_filter(f));
                match (origin, dest) {
                    ("Any", Some("Battlefield")) => filter.map(Trigger::Enters),
                    ("Battlefield", Some("Graveyard")) => filter.map(Trigger::Dies),
                    (o, d) => {
                        self.block(format!("trig:ChangesZone {o}->{}", d.unwrap_or("?")));
                        None
                    }
                }
            }
            "Phase" => {
                let step = match rd.get("Phase") {
                    Some("Upkeep") => Some(TriggerStep::Upkeep),
                    other => {
                        self.block(format!("trig:Phase {}", other.unwrap_or("?")));
                        None
                    }
                };
                let whose = match rd.get("ValidPlayer") {
                    None => Some(Who::Any),
                    Some("You") => Some(Who::You),
                    Some("Opponent" | "Player.Opponent") => Some(Who::Opponent),
                    Some(other) => {
                        self.block(format!("trig:Phase player {other}"));
                        None
                    }
                };
                Some(Trigger::BeginningOf {
                    step: step?,
                    whose: whose?,
                })
            }
            "SpellCast" => {
                let by = match rd.get("ValidActivatingPlayer") {
                    None => Some(Who::Any),
                    Some("You") => Some(Who::You),
                    Some("Opponent" | "Player.Opponent") => Some(Who::Opponent),
                    Some(other) => {
                        self.block(format!("trig:SpellCast player {other}"));
                        None
                    }
                };
                // Only card types: "Creature", "Instant,Sorcery", or any "Card".
                let mut types = Vec::new();
                for word in rd.get("ValidCard").unwrap_or("Card").split(',') {
                    match (word.trim(), card_type(word.trim())) {
                        ("Card", _) => {}
                        (_, Some(t)) => types.push(t),
                        (w, None) => {
                            self.block(format!("trig:SpellCast card {}", shape(w)));
                            return None;
                        }
                    }
                }
                Some(Trigger::Cast { by: by?, types })
            }
            "Attacks" => rd
                .get("ValidCard")
                .and_then(|f| self.obj_filter(f))
                .map(Trigger::Attacks),
            other => {
                self.block(format!("trig:{other}"));
                None
            }
        }
    }

    /// All `T:` lines. Two lines that run the same ability, one marked `Secondary$`, are
    /// one ability with two events ("enters or attacks").
    fn triggers(&mut self) -> Vec<(usize, AbilityDef)> {
        let mut out: Vec<(usize, &str, AbilityDef)> = Vec::new();
        let face = self.face;
        for (line, rec) in face.triggers.iter().enumerate() {
            let rd = Rd::new(rec, format!("trig {}", rec.get("Mode").unwrap_or("?")));
            let event = self.trigger_event(&rd);
            // Without `TriggerZones$` a Forge trigger works from every zone, except that
            // "this enters", "this attacks" and leaves-the-battlefield triggers default to
            // the battlefield (docs/forge/triggers-statics-replacements-keywords.md).
            // The engine's triggers are battlefield abilities, so anything else must say so.
            let about_itself = rec
                .get("ValidCard")
                .is_some_and(|v| v.split(',').any(|alt| alt.trim() == "Card.Self"));
            let own_event = match (rec.get("Mode"), rec.get("Origin")) {
                // Forge's `correctZones`: leaving the battlefield -> the battlefield; a
                // `ValidCard` that includes the card itself -> the destination zone.
                (Some("ChangesZone"), Some("Battlefield")) => true,
                (Some("ChangesZone"), None | Some("Any")) => {
                    about_itself && rec.get("Destination") == Some("Battlefield")
                }
                // An attacking creature is on the battlefield.
                (Some("Attacks"), _) => rec.get("ValidCard") == Some("Card.Self"),
                _ => false,
            };
            match rd.get("TriggerZones") {
                Some("Battlefield") => {}
                None if own_event => {}
                None => self.block("trig:zone unstated"),
                Some(z) => self.block(format!("trig:zone {z}")),
            }
            let optional = match rd.get("OptionalDecider") {
                None => false,
                Some("You") => true,
                Some(o) => {
                    self.block(format!("trig:OptionalDecider {o}"));
                    false
                }
            };
            let secondary = rd.get("Secondary") == Some("True");
            let cond = self.condition(&rd);
            let exec = rd.get("Execute").unwrap_or("");
            rd.finish(&mut self.blockers);
            if secondary
                && let Some(event) = &event
                && let Some((_, _, AbilityDef::Triggered { when, .. })) =
                    out.iter_mut().find(|(_, e, _)| *e == exec)
            {
                let first = std::mem::replace(when, Trigger::Any(Vec::new()));
                *when = match first {
                    Trigger::Any(mut v) => {
                        v.push(event.clone());
                        Trigger::Any(v)
                    }
                    one => Trigger::Any(vec![one, event.clone()]),
                };
                continue;
            }
            if secondary {
                self.block("trig:Secondary-alone");
            }
            let body = match self.sub(exec) {
                Some(r) => self.body(r, true),
                None => None,
            };
            let (Some(when), Some(intervening_if), Some(mut body)) = (event, cond, body) else {
                continue;
            };
            if optional {
                // "You may": decided as the ability resolves.
                body = match body {
                    Body::Effect(m) => Body::Effect(Mode {
                        targets: m.targets,
                        effect: Effect::May(Box::new(m.effect)),
                    }),
                    modal => {
                        self.block("trig:optional-modal");
                        modal
                    }
                };
            }
            out.push((
                line,
                exec,
                AbilityDef::Triggered {
                    when,
                    intervening_if,
                    body,
                },
            ));
        }
        out.into_iter().map(|(line, _, a)| (line, a)).collect()
    }

    fn static_ability(&mut self, rec: &'a Record) -> Option<AbilityDef> {
        let mode = rec.get("Mode").unwrap_or("?");
        let rd = Rd::new(rec, format!("static {mode}"));
        let out = match mode {
            "Continuous" => {
                let affected = match rd.get("Affected") {
                    Some("Card.Self") => Some(Affected::This),
                    Some(f) => self.selector(f).map(Affected::All),
                    None => {
                        self.block("static:no-Affected");
                        None
                    }
                };
                let int = |t: Option<&str>| {
                    t.map_or(Some(0), |t| t.trim_start_matches('+').parse::<i32>().ok())
                };
                let (p, t) = (int(rd.get("AddPower")), int(rd.get("AddToughness")));
                let (Some(p), Some(t)) = (p, t) else {
                    self.block("static:variable-pt");
                    return None;
                };
                let mut effects = Vec::new();
                if (p, t) != (0, 0) {
                    effects.push(ContinuousEffect::ModifyPT(p, t));
                }
                if let Some(kw) = rd.get("AddKeyword") {
                    let mut ks = Vec::new();
                    for k in kw.split('&').map(str::trim) {
                        match keyword(k) {
                            Some(k) => ks.push(k),
                            None => {
                                self.block(format!("kw:{k}"));
                                return None;
                            }
                        }
                    }
                    effects.push(ContinuousEffect::AddKeywords(ks));
                }
                if effects.is_empty() {
                    self.block("static:Continuous-nothing");
                    return None;
                }
                affected.map(|affected| AbilityDef::Static { affected, effects })
            }
            "ReduceCost" => {
                if rd.get("Type") != Some("Spell") {
                    self.block("static:ReduceCost-type");
                    return None;
                }
                let amount = rd.get("Amount").unwrap_or("1");
                match (
                    rd.get("ValidCard"),
                    rd.get("EffectZone"),
                    rd.get("Activator"),
                ) {
                    // "This spell costs {N} less to cast."
                    (Some("Card.Self"), Some("All"), None) => match amount.parse::<u32>() {
                        Ok(n) => Some(Amount::Fixed(n)),
                        Err(_) => self.expr(amount),
                    }
                    .map(AbilityDef::SelfCostReduction),
                    // "[These] spells you cast cost {N} less to cast."
                    (Some(f), None | Some("Battlefield"), Some("You")) => {
                        let spells = self.selector(f);
                        let n = amount.parse::<u32>().ok().or_else(|| {
                            self.block("static:ReduceCost-variable");
                            None
                        });
                        Some(AbilityDef::CostReduction {
                            spells: spells?,
                            amount: n?,
                        })
                    }
                    _ => {
                        self.block("static:ReduceCost-shape");
                        None
                    }
                }
            }
            other => {
                self.block(format!("static:{other}"));
                None
            }
        };
        rd.finish(&mut self.blockers);
        out
    }

    fn replacement(&mut self, rec: &'a Record) -> Option<AbilityDef> {
        let event = rec.get("Event").unwrap_or("?");
        let rd = Rd::new(rec, format!("repl {event}"));
        // "This permanent enters tapped."
        let shape = (
            event,
            rd.get("ValidCard"),
            rd.get("Destination"),
            rd.get("ReplacementResult"),
        );
        let with = rd.get("ReplaceWith").and_then(|n| self.sub(n));
        rd.finish(&mut self.blockers);
        let tapped = with.is_some_and(|w| {
            w.params
                == [
                    ("DB".to_string(), "Tap".to_string()),
                    ("Defined".to_string(), "Self".to_string()),
                    ("ETB".to_string(), "True".to_string()),
                ]
        });
        if shape
            == (
                "Moved",
                Some("Card.Self"),
                Some("Battlefield"),
                Some("Updated"),
            )
            && tapped
        {
            return Some(AbilityDef::Replacement {
                replaces: Replaces::ThisEnters,
                with: ReplaceWith::EntersTapped,
            });
        }
        self.block(format!("repl:{event}"));
        None
    }
}

/// Several selectors that differ only in card type are one selector.
fn merge(mut sels: Vec<Selector>) -> Option<Selector> {
    if sels.len() == 1 {
        return sels.pop();
    }
    let mut types = Vec::new();
    for s in &sels {
        let mut bare = s.clone();
        let t = std::mem::take(&mut bare.types);
        if bare != Selector::default() || t.len() != 1 {
            return None;
        }
        types.extend(t);
    }
    Some(Selector {
        types,
        ..Selector::default()
    })
}

/// A list of alternatives with those that are just a card type ("Artifact", "Enchantment")
/// folded into one selector, placed where the first of them was.
fn merge_bare(sels: Vec<Selector>) -> Vec<Selector> {
    let bare = |s: &Selector| {
        s.types.len() == 1
            && Selector {
                types: Vec::new(),
                ..s.clone()
            } == Selector::default()
    };
    let mut out: Vec<Selector> = Vec::new();
    let mut folded: Option<usize> = None;
    for s in sels {
        match (bare(&s), folded) {
            (true, Some(i)) => out[i].types.extend(s.types),
            (true, None) => {
                folded = Some(out.len());
                out.push(s);
            }
            (false, _) => out.push(s),
        }
    }
    out
}

/// An expression with its numbers and filters blanked, for grouping blockers.
fn shape(v: &str) -> String {
    let head: String = v
        .chars()
        .map(|c| if c.is_ascii_digit() { 'N' } else { c })
        .collect();
    let head = head.split('/').next().unwrap_or("").to_string();
    match head.split_once("Valid ") {
        Some((a, _)) => format!("{a}Valid <filter>"),
        None => head.chars().take(40).collect(),
    }
}

/// Lines of script variables that hold abilities, parsed once.
fn ability_records(face: &Face) -> Vec<(String, Record)> {
    face.svars
        .iter()
        .filter(|(_, v)| v.contains('$') && (v.contains('|') || v.starts_with("DB$")))
        .map(|(n, v)| (n.clone(), Record::parse(v)))
        .collect()
}

/// Compile a script. `Err` lists every blocker found (not just the first).
pub fn compile(script: &Script, known: &Known) -> Result<Compiled, Blockers> {
    let mut early = Blockers::new();
    if script.faces.len() != 1 || script.alternate_mode.is_some() {
        early.insert(format!(
            "layout:{}",
            script.alternate_mode.as_deref().unwrap_or("multi-face")
        ));
        return Err(early);
    }
    let face = &script.faces[0];
    let parsed = ability_records(face);
    let mut tx = Tx {
        face,
        blockers: early,
        used_svars: BTreeSet::new(),
        tokens: BTreeSet::new(),
        has_x_cost: false,
        records: &parsed,
        known,
    };
    let mut def = CardDef {
        name: face.name.clone(),
        ..CardDef::default()
    };
    for line in &face.unknown {
        tx.block(format!("line:{}", line.split(':').next().unwrap_or("?")));
    }
    for (what, present) in [
        ("line:Loyalty", face.loyalty.is_some()),
        ("line:Defense", face.defense.is_some()),
        ("line:Text", face.text.is_some()),
    ] {
        if present {
            tx.block(what);
        }
    }
    // Printed characteristics.
    match mana_cost(face.mana_cost.as_deref().unwrap_or("no cost")) {
        Ok(c) => {
            tx.has_x_cost = c.as_ref().is_some_and(ManaCost::has_x);
            def.mana_cost = c;
        }
        Err(b) => tx.block(b),
    }
    for word in &face.types {
        if let Some(t) = card_type(word) {
            def.card_types.push(t);
            continue;
        }
        match word.as_str() {
            "Basic" => def.supertypes.push(Supertype::Basic),
            "Legendary" => def.supertypes.push(Supertype::Legendary),
            "Snow" => def.supertypes.push(Supertype::Snow),
            "World" => def.supertypes.push(Supertype::World),
            "Plains" => def.land_types.push(BasicLandType::Plains),
            "Island" => def.land_types.push(BasicLandType::Island),
            "Swamp" => def.land_types.push(BasicLandType::Swamp),
            "Mountain" => def.land_types.push(BasicLandType::Mountain),
            "Forest" => def.land_types.push(BasicLandType::Forest),
            _ if def.card_types.contains(&CardType::Land)
                && !def.card_types.contains(&CardType::Creature) =>
            {
                tx.block("type:land-subtype")
            }
            other => def.creature_types.push(other.to_string()),
        }
    }
    if let Some((p, t)) = &face.pt {
        match (p.parse::<i32>(), t.parse::<i32>()) {
            (Ok(p), Ok(t)) => {
                def.power = Some(p);
                def.toughness = Some(t);
            }
            _ => tx.block("pt:variable"),
        }
    }
    if let Some(colors) = &face.colors {
        let mut cs = Vec::new();
        for c in colors.split(',') {
            let mut w = c.trim().to_string();
            if let Some(f) = w.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            match color(&w) {
                Some(c) => cs.push(c),
                None => tx.block(format!("colors:{c}")),
            }
        }
        cs.sort();
        def.colors = Some(cs);
    }
    for k in &face.keywords {
        match keyword(k) {
            Some(k) => def.keywords.push(k),
            None => {
                let name = k.split(':').next().unwrap_or(k);
                let name = name
                    .split(|c: char| c.is_ascii_digit())
                    .next()
                    .unwrap_or(name);
                tx.block(format!("kw:{}", name.trim()));
            }
        }
    }
    // Abilities, in the order the engine's own scripts use: as written, line kind by line
    // kind (the order within a kind is the script's).
    let mut abilities: Vec<(usize, AbilityDef)> = Vec::new();
    let order = line_order(face);
    for (i, rec) in face.abilities.iter().enumerate() {
        if let Some(a) = tx.ability(rec) {
            abilities.push((order.abilities[i], a));
        }
    }
    // Merged ("Secondary") triggers stay at their first line's place.
    for (line, a) in tx.triggers() {
        abilities.push((order.triggers[line], a));
    }
    for (i, rec) in face.statics.iter().enumerate() {
        if let Some(a) = tx.static_ability(rec) {
            abilities.push((order.statics[i], a));
        }
    }
    for (i, rec) in face.replacements.iter().enumerate() {
        if let Some(a) = tx.replacement(rec) {
            abilities.push((order.replacements[i], a));
        }
    }
    abilities.sort_by_key(|(i, _)| *i);
    def.abilities = abilities.into_iter().map(|(_, a)| a).collect();
    // A variable holding an ability that nothing refers to means part of the card was
    // not understood.
    for (name, _) in &parsed {
        if !tx.used_svars.contains(name) {
            tx.block("svar:unused-ability");
        }
    }
    if tx.blockers.is_empty() {
        Ok(Compiled {
            def,
            tokens: tx.tokens,
        })
    } else {
        Err(tx.blockers)
    }
}

/// Where each `A:`/`T:`/`S:`/`R:` line sits among the others, so abilities keep the
/// script's order.
struct LineOrder {
    abilities: Vec<usize>,
    triggers: Vec<usize>,
    statics: Vec<usize>,
    replacements: Vec<usize>,
}

fn line_order(face: &Face) -> LineOrder {
    let mut o = LineOrder {
        abilities: Vec::new(),
        triggers: Vec::new(),
        statics: Vec::new(),
        replacements: Vec::new(),
    };
    for (n, kind) in face.order.iter().enumerate() {
        match kind {
            'A' => o.abilities.push(n),
            'T' => o.triggers.push(n),
            'S' => o.statics.push(n),
            'R' => o.replacements.push(n),
            _ => {}
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::parse;

    fn known() -> Known {
        Known {
            subtypes: ["Elf", "Zombie", "Dragon", "Mountain", "Wizard"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }

    fn compile_text(text: &str) -> Result<Compiled, Blockers> {
        compile(&parse(text).unwrap(), &known())
    }

    // Invented cards in Forge's format.
    #[test]
    fn a_simple_creature_compiles_with_its_abilities_in_script_order() {
        let c = compile_text(
            "Name:Test Chanter\nManaCost:1 B\nTypes:Legendary Creature Zombie Wizard\nPT:2/3\nK:Flying\n\
             T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self,Zombie.Other+YouCtrl | Execute$ TrigDraw | TriggerDescription$ text\n\
             SVar:TrigDraw:DB$ Draw | Defined$ You | NumCards$ 1 | SubAbility$ DBLose\n\
             SVar:DBLose:DB$ LoseLife | LifeAmount$ 1\n\
             S:Mode$ Continuous | Affected$ Creature.Zombie+Other+YouCtrl | AddPower$ 1 | AddToughness$ 1 | Description$ text\n\
             A:AB$ PumpAll | Cost$ 1 B T | ValidCards$ Zombie | KW$ Menace | SpellDescription$ text\n\
             Oracle:text",
        )
        .unwrap();
        let d = &c.def;
        assert_eq!(d.name, "Test Chanter");
        assert_eq!(d.mana_cost.as_ref().unwrap().to_text(), "{1}{B}");
        assert_eq!(d.supertypes, [Supertype::Legendary]);
        assert_eq!(d.card_types, [CardType::Creature]);
        assert_eq!(d.creature_types, ["Zombie", "Wizard"]);
        assert_eq!((d.power, d.toughness), (Some(2), Some(3)));
        assert_eq!(d.keywords, [Keyword::Flying]);
        assert_eq!(d.abilities.len(), 3);
        // In the order written: trigger, static, activated.
        let AbilityDef::Triggered { when, body, .. } = &d.abilities[0] else {
            panic!("{:?}", d.abilities[0]);
        };
        // "This or another Zombie you control dies."
        let Trigger::Dies(ObjFilter::ThisOr(sel)) = when else {
            panic!("{when:?}");
        };
        assert_eq!(sel.subtypes, ["Zombie"]);
        assert_eq!(sel.controller, Who::You);
        assert!(!sel.other);
        let Body::Effect(mode) = body else { panic!() };
        assert!(matches!(&mode.effect, Effect::Seq(v) if v.len() == 2));
        assert!(matches!(d.abilities[1], AbilityDef::Static { .. }));
        let AbilityDef::Activated { cost, .. } = &d.abilities[2] else {
            panic!();
        };
        assert!(cost.tap && cost.mana.as_ref().unwrap().to_text() == "{1}{B}");
    }

    #[test]
    fn anything_not_understood_blocks_the_card_and_is_named() {
        let blockers = |text: &str| compile_text(text).unwrap_err();
        let base = "Name:Test Card\nManaCost:1 R\nTypes:Sorcery\n";
        // An unknown effect API.
        let b = blockers(&format!("{base}A:SP$ Fight | ValidTgts$ Creature"));
        assert!(b.contains("api:Fight"), "{b:?}");
        // A known API with a parameter the compiler doesn't read.
        let b = blockers(&format!(
            "{base}A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 2 | DivideEvenly$ True"
        ));
        assert_eq!(
            b.into_iter().collect::<Vec<_>>(),
            ["param:DealDamage.DivideEvenly"]
        );
        // A filter property that isn't a known subtype is not taken for one.
        let b = blockers(&format!(
            "{base}A:SP$ Destroy | ValidTgts$ Creature.DamagedBy"
        ));
        assert!(b.contains("filter:.DamagedBy"), "{b:?}");
        let b = blockers(&format!("{base}A:SP$ DestroyAll | ValidCards$ Gremlin"));
        assert!(b.contains("filter:Gremlin"), "{b:?}");
        // An expression form that isn't supported.
        let b = blockers(&format!(
            "{base}A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ X\nSVar:X:Count$YourLifeTotal/HalfUp"
        ));
        assert!(b.iter().any(|x| x.starts_with("expr:")), "{b:?}");
        // A keyword the engine doesn't have, an unknown line, a second face.
        let b =
            blockers("Name:Test Card\nManaCost:G\nTypes:Creature Elf\nPT:1/1\nK:Shroud\nMystery:1");
        assert!(
            b.contains("kw:Shroud") && b.contains("line:Mystery"),
            "{b:?}"
        );
        let b = blockers(
            "Name:A\nTypes:Creature Elf\nAlternateMode:DoubleFaced\nALTERNATE\nName:B\nTypes:Creature Elf",
        );
        assert!(b.contains("layout:DoubleFaced"), "{b:?}");
        // An ability variable nothing refers to.
        let b = blockers(&format!(
            "{base}A:SP$ Draw | NumCards$ 2\nSVar:DBExtra:DB$ GainLife | LifeAmount$ 3"
        ));
        assert!(b.contains("svar:unused-ability"), "{b:?}");
        // Descriptions and AI hints are not rules: they block nothing.
        assert!(compile_text(&format!(
            "{base}A:SP$ Draw | NumCards$ 2 | SpellDescription$ Draw two cards. | AILogic$ Whatever\nSVar:PlayMain1:TRUE\nDeckHas:Ability$Draw"
        ))
        .is_ok());
    }

    #[test]
    fn where_x_is_a_count_it_is_worked_out_once() {
        let c = compile_text(
            "Name:Test Blast\nManaCost:2 R\nTypes:Sorcery\nA:SP$ DamageAll | NumDmg$ X | ValidCards$ Creature\nSVar:X:Count$Valid Mountain.YouCtrl",
        )
        .unwrap();
        let AbilityDef::Spell(Body::Effect(mode)) = &c.def.abilities[0] else {
            panic!();
        };
        let Effect::Seq(v) = &mode.effect else {
            panic!()
        };
        assert!(matches!(&v[0], Effect::SetX(Amount::Count(s)) if s.subtypes == ["Mountain"]));
        assert!(matches!(
            &v[1],
            Effect::DamageEach {
                amount: Amount::X,
                ..
            }
        ));
    }

    #[test]
    fn mana_costs() {
        assert_eq!(mana_cost("no cost"), Ok(None));
        assert_eq!(
            mana_cost("X 2 G G").unwrap().unwrap().to_text(),
            "{X}{2}{G}{G}"
        );
        assert!(
            mana_cost("1 WU").is_err(),
            "hybrid mana isn't supported yet"
        );
    }
}
