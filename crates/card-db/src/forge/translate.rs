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
    AbilityDef, Affected, Amount, BasicLandType, Body, CardDef, CardDest, CardType, CardZone,
    Color, CombatRule, Condition, ContinuousEffect, Cost, CostChoice, CounterKind, DamageBy,
    EachPlayerAction, Effect, Keyword, ManaCost, ManaProduce, ManaType, Mode, ObjFilter, ObjRef,
    PlayerRef, ReplaceWith, Replaces, Selector, Signed, Supertype, TargetFilter, TargetRef, Timing,
    Trigger, TriggerStep, Who,
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
const TEXT_ONLY: [&str; 14] = [
    "ChangeTypeDesc",
    "ChangeNumDesc",
    "DefinedDesc",
    "SelectPrompt",
    "OptionalPrompt",
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
        for (i, (k, _)) in self.rec.params.iter().enumerate() {
            let k = k.as_str();
            // Forge takes the last of two parameters with the same key; rather than rely
            // on that, a record that repeats a key isn't compiled.
            if self.rec.params[..i].iter().any(|(other, _)| other == k) {
                blockers.insert(format!("param:{}.{k} (twice)", self.what));
            }
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
// Short-lived values while one filter is read: the size difference costs nothing.
#[allow(clippy::large_enum_variant)]
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
    /// Some link said `Defined$ Targeted` — in Forge, every target of the whole chain.
    refers_to_targets: std::cell::Cell<bool>,
}

struct Tx<'a> {
    face: &'a Face,
    blockers: Blockers,
    used_svars: BTreeSet<String>,
    tokens: BTreeSet<String>,
    has_x_cost: bool,
    /// Compiling the body of a triggered ability (where "X" is not the X being paid).
    in_trigger: bool,
    /// Additional costs of the spell, found on its `SP$` line.
    additional: Vec<AbilityDef>,
    /// The filter of the card's `K:Enchant:<filter>` line, if it is an Aura.
    enchant: Option<&'a str>,
    /// The card is an Equipment (its statics may speak of the "equipped creature").
    equipment: bool,
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

/// The tokens of a cost: separated by spaces, except inside `<…>`
/// (`Sac<1/Creature.Other/another creature>` is one token).
fn cost_tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut depth) = (None, 0u32);
    for (i, c) in text.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                out.push(&text[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    out
}

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
        "Defender" => Keyword::Defender,
        "Fear" => Keyword::Fear,
        "Hexproof" => Keyword::Hexproof,
        "Indestructible" => Keyword::Indestructible,
        "Intimidate" => Keyword::Intimidate,
        "Shroud" => Keyword::Shroud,
        "Landwalk:Plains" => Keyword::Landwalk(BasicLandType::Plains),
        "Landwalk:Island" => Keyword::Landwalk(BasicLandType::Island),
        "Landwalk:Swamp" => Keyword::Landwalk(BasicLandType::Swamp),
        "Landwalk:Mountain" => Keyword::Landwalk(BasicLandType::Mountain),
        "Landwalk:Forest" => Keyword::Landwalk(BasicLandType::Forest),
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

    /// A line that produced nothing must have said why. If no blocker was recorded since
    /// `before` (a count of blockers), the line would silently vanish from the card: that
    /// is a hole in the compiler, and the card is blocked for it.
    fn explained(&mut self, before: usize, what: &str) {
        if self.blockers.len() == before {
            self.block(format!("unexplained:{what}"));
        }
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
                "Basic" => s.supertypes.push(Supertype::Basic),
                "Legendary" => s.supertypes.push(Supertype::Legendary),
                "Snow" => s.supertypes.push(Supertype::Snow),
                "tapped" => s.tapped = Some(true),
                "untapped" => s.tapped = Some(false),
                "attacking" => s.attacking = Some(true),
                "blocking" => s.blocking = Some(true),
                _ => {
                    if let Some(c) = color(p) {
                        s.colors.push(c);
                    } else if let Some(n) = p.strip_prefix("powerGE").and_then(|n| n.parse().ok()) {
                        s.power_min = Some(n);
                    } else if let Some(n) =
                        p.strip_prefix("toughnessGE").and_then(|n| n.parse().ok())
                    {
                        s.toughness_min = Some(n);
                    } else if let Some(n) = p.strip_prefix("powerLE").and_then(|n| n.parse().ok()) {
                        s.power_max = Some(n);
                    } else if let Some(k) = p.strip_prefix("without").and_then(keyword) {
                        s.not_keywords.push(k);
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

    /// A filter for cards that aren't on the battlefield (in a graveyard, a hand…). There
    /// "Permanent" is not "anything": it is a *permanent card* — not an instant or sorcery
    /// (CR 110.4a).
    fn card_selector(&mut self, text: &str) -> Option<Selector> {
        let mut s = self.selector(text)?;
        let heads: Vec<&str> = text
            .split(',')
            .map(|a| a.trim().split('.').next().unwrap_or(""))
            .collect();
        if heads.contains(&"Permanent") {
            if heads.iter().any(|h| *h != "Permanent") {
                self.block("filter:Permanent-card-alternatives");
                return None;
            }
            for t in [CardType::Instant, CardType::Sorcery] {
                if !s.not_types.contains(&t) {
                    s.not_types.push(t);
                }
            }
        }
        Some(s)
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
            // In a spell or an activated ability: the X announced for it (CR 107.3a). In a
            // permanent's own trigger it is the X the permanent was cast with (CR 107.3m),
            // which the engine doesn't remember yet.
            if self.in_trigger {
                self.block("expr:Count$xPaid in a trigger");
                return None;
            }
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

    fn player(&mut self, rd: &Rd<'a>, chain: &Chain, triggered: bool) -> Option<PlayerRef> {
        match rd.get("Defined") {
            None | Some("You") => Some(PlayerRef::You),
            Some("Player.Opponent" | "Opponent") => Some(PlayerRef::EachOpponent),
            Some("Player") => Some(PlayerRef::Each),
            // The rules' "its controller" is whoever controlled it last (CR 608.2h).
            Some("TriggeredCardController") if triggered => Some(PlayerRef::TriggeringController),
            Some("Targeted") => match chain.targets.len() {
                0 => {
                    self.block("defined:Targeted-without-target");
                    None
                }
                n => {
                    chain.refers_to_targets.set(true);
                    Some(PlayerRef::Target(n as u8 - 1))
                }
            },
            Some(other) => {
                self.block(format!("defined:player {other}"));
                None
            }
        }
    }

    /// `Defined$` of an effect done to objects and players alike (damage). Forge's default
    /// is the card itself.
    fn recipient(&mut self, rd: &Rd<'a>, chain: &Chain, triggered: bool) -> Option<TargetRef> {
        match rd.rec.get("Defined") {
            Some("You" | "Opponent" | "Player.Opponent" | "Player" | "TriggeredCardController") => {
                self.player(rd, chain, triggered).map(TargetRef::Players)
            }
            Some("Targeted") => match chain.targets.len() {
                0 => {
                    rd.get("Defined");
                    self.block("defined:Targeted-without-target");
                    None
                }
                n => {
                    rd.get("Defined");
                    chain.refers_to_targets.set(true);
                    Some(TargetRef::Target(n as u8 - 1))
                }
            },
            _ => self.object(rd, chain, triggered).map(TargetRef::Object),
        }
    }

    fn object(&mut self, rd: &Rd<'a>, chain: &Chain, triggered: bool) -> Option<ObjRef> {
        match rd.get("Defined") {
            None | Some("Self") => Some(ObjRef::This),
            // Forge's plain form follows the card to wherever it is now and the "LKICopy"
            // form is the snapshot from the event; for the effects that use this reference
            // (counters and pumps on a permanent) both act only if it is still the
            // permanent that triggered the ability, which is what `Triggering` means.
            Some("TriggeredCard" | "TriggeredCardLKICopy") if triggered => Some(ObjRef::Triggering),
            Some("Targeted") if !chain.targets.is_empty() => {
                chain.refers_to_targets.set(true);
                Some(ObjRef::Target(chain.targets.len() as u8 - 1))
            }
            Some(other) => {
                self.block(format!("defined:card {other}"));
                None
            }
        }
    }

    /// `ValidCards$` of an "all" effect. Forge's default when it is missing differs from
    /// effect to effect, so it must be there.
    fn valid_cards(&mut self, rd: &Rd<'a>, api: &str) -> Option<Selector> {
        match rd.get("ValidCards") {
            Some(f) => self.selector(f),
            None => {
                self.block(format!("{api}:no-ValidCards"));
                None
            }
        }
    }

    /// `ValidTgts$` of an effect: adds a target slot and returns its index.
    fn target(&mut self, rd: &Rd<'a>, chain: &mut Chain, graveyard: bool) -> Option<u8> {
        let text = rd.get("ValidTgts")?;
        // Which of the two wins differs from one Forge effect to the next.
        if rd.rec.get("Defined").is_some() {
            self.block("param:target+Defined");
            return None;
        }
        for k in ["TargetMin", "TargetMax", "TargetUnique", "TargetType"] {
            if rd.rec.get(k).is_some() {
                self.block(format!("param:target.{k}"));
                return None;
            }
        }
        let filter = if graveyard {
            let mut s = self.card_selector(text)?;
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
    fn effect(
        &mut self,
        rec: &'a Record,
        chain: &mut Chain,
        triggered: bool,
        kind: &str,
    ) -> Option<Effect> {
        let Some((found, api)) = rec.head() else {
            self.block("record:no-api");
            return None;
        };
        // A trigger or sub-ability written as `AB$` with a cost is "you may pay …; if you
        // do, …" in Forge: not the same as a plain effect.
        if found != kind {
            self.block(format!("record:{found}$ where {kind}$ expected"));
            return None;
        }
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
                let to = match self.target(&rd, chain, false) {
                    Some(slot) => Some(TargetRef::Target(slot)),
                    None => self.recipient(&rd, chain, triggered),
                };
                let amount = rd.get("NumDmg").and_then(|n| self.amount(n, chain));
                Effect::DealDamage {
                    to: to?,
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
                    None => self.player(&rd, chain, triggered),
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
                let what = match self.target(&rd, chain, false) {
                    Some(slot) => Some(ObjRef::Target(slot)),
                    None => self.object(&rd, chain, triggered),
                };
                Effect::Destroy { what: what? }
            }
            "Tap" | "Untap" => {
                // `ETB$`: "enters tapped/untapped", which is a replacement, not this effect.
                if rd.rec.get("ETB").is_some() {
                    self.block(format!("{api}:ETB"));
                    return None;
                }
                let what = match self.target(&rd, chain, false) {
                    Some(slot) => Some(ObjRef::Target(slot)),
                    None => self.object(&rd, chain, triggered),
                };
                match api {
                    "Tap" => Effect::Tap { what: what? },
                    _ => Effect::Untap { what: what? },
                }
            }
            "TapAll" | "UntapAll" => {
                let sel = self.valid_cards(&rd, api);
                match api {
                    "TapAll" => Effect::TapAll(sel?),
                    _ => Effect::UntapAll(sel?),
                }
            }
            "Counter" => {
                // Only spells, and only as a target.
                if rd.get("TargetType") != Some("Spell") {
                    self.block("Counter:not-a-spell-target");
                    return None;
                }
                for k in ["Defined", "TargetMin", "TargetMax"] {
                    if rd.rec.get(k).is_some() {
                        self.block(format!("param:Counter.{k}"));
                        return None;
                    }
                }
                let filter = match rd.get("ValidTgts") {
                    Some("Card") => TargetFilter::Spell,
                    Some(text) => {
                        let s = self.card_selector(text)?;
                        // What a spell can be asked: its types and colors.
                        let asked = Selector {
                            types: s.types.clone(),
                            not_types: s.not_types.clone(),
                            subtypes: s.subtypes.clone(),
                            not_subtypes: s.not_subtypes.clone(),
                            colors: s.colors.clone(),
                            ..Selector::default()
                        };
                        if asked != s {
                            self.block("Counter:filter");
                            return None;
                        }
                        TargetFilter::SpellMatching(s)
                    }
                    None => {
                        self.block("Counter:no-target");
                        return None;
                    }
                };
                chain.targets.push(filter);
                Effect::Counter {
                    what: TargetRef::Target(chain.targets.len() as u8 - 1),
                }
            }
            "DestroyAll" => {
                let sel = self.valid_cards(&rd, api);
                Effect::DestroyAll(sel?)
            }
            "DamageAll" => {
                let amount = rd.get("NumDmg").and_then(|n| self.amount(n, chain));
                match (rd.rec.get("ValidCards"), rd.get("ValidPlayers")) {
                    (_, None) => {
                        let sel = self.valid_cards(&rd, api);
                        Effect::DamageEach {
                            each: sel?,
                            amount: amount?,
                        }
                    }
                    // Players only: "deals N damage to each player / each opponent".
                    (None, Some("Player")) => Effect::DamageEachPlayer { amount: amount? },
                    (None, Some("Player.Opponent" | "Opponent")) => Effect::DealDamage {
                        to: TargetRef::Players(PlayerRef::EachOpponent),
                        amount: amount?,
                        by: DamageBy::This,
                    },
                    (None, Some(other)) => {
                        self.block(format!("param:DamageAll.ValidPlayers={other}"));
                        return None;
                    }
                    // Creatures and players at once is one event the vocabulary lacks.
                    (Some(_), Some(_)) => {
                        self.block("DamageAll:cards+players");
                        return None;
                    }
                }
            }
            "ChangeZone"
                if matches!(rec.get("Origin"), Some("Library" | "Hand"))
                    && rec.get("ValidTgts").is_none()
                    && rec.get("Defined").is_none() =>
            {
                self.choose_cards(&rd, chain, triggered)?
            }
            "ChangeZone" => {
                let origin = rd.get("Origin").unwrap_or("Battlefield");
                let dest = rd.get("Destination");
                let what = match self.target(&rd, chain, origin == "Graveyard") {
                    Some(slot) => ObjRef::Target(slot),
                    // A permanent named some other way: "return this to its owner's hand".
                    None if origin == "Battlefield" && rd.rec.get("Defined").is_some() => {
                        self.object(&rd, chain, triggered)?
                    }
                    None => {
                        self.block("ChangeZone:no-target");
                        return None;
                    }
                };
                match (origin, dest) {
                    ("Battlefield" | "Graveyard", Some("Exile")) => Effect::Exile { what },
                    ("Battlefield" | "Graveyard", Some("Hand")) => Effect::ReturnToHand { what },
                    ("Graveyard", Some("Battlefield")) => Effect::ReturnToBattlefield { what },
                    // "Put target creature on top of its owner's library", "shuffle it
                    // into its owner's library".
                    ("Battlefield", Some("Library")) => {
                        let to = self.library_position(&rd)?;
                        let shuffle = match rd.get("Shuffle") {
                            None => false,
                            Some("True") => true,
                            Some(_) => {
                                self.block("param:ChangeZone.Shuffle");
                                return None;
                            }
                        };
                        Effect::PutInto { what, to, shuffle }
                    }
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
                    // "Return all creatures to their owners' hands", "exile all
                    // artifacts", "destroy"-less mass moves of permanents: everyone's.
                    ((Some("Battlefield"), Some(dest), Some(filter)), None)
                        if rd.rec.get("Defined").is_none() =>
                    {
                        let to = match dest {
                            "Hand" => CardDest::Hand,
                            "Exile" => CardDest::Exile,
                            "Graveyard" => CardDest::Graveyard,
                            _ => {
                                self.block("ChangeZoneAll:shape");
                                return None;
                            }
                        };
                        let cards = self.card_alts(filter, false)?;
                        Effect::MoveAll {
                            from: CardZone::Battlefield,
                            whose: PlayerRef::Each,
                            cards,
                            to,
                            shuffle: false,
                        }
                    }
                    _ => {
                        self.block("ChangeZoneAll:shape");
                        return None;
                    }
                }
            }
            // CR 701.22 / 701.25.
            "Scry" | "Surveil" => {
                let who = match self.target(&rd, chain, false) {
                    Some(i) if chain.targets[i as usize].allows_players() => {
                        Some(PlayerRef::Target(i))
                    }
                    Some(_) => {
                        self.block(format!("{api}:target"));
                        return None;
                    }
                    None => self.player(&rd, chain, triggered),
                };
                let key = if api == "Scry" { "ScryNum" } else { "Amount" };
                let amount = self.amount(rd.get(key).unwrap_or("1"), chain);
                match api {
                    "Scry" => Effect::Scry {
                        who: who?,
                        amount: amount?,
                    },
                    _ => Effect::Surveil {
                        who: who?,
                        amount: amount?,
                    },
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
                let effects = self.pump_effects(&rd, chain)?;
                if api == "PumpAll" {
                    let sel = self.valid_cards(&rd, api);
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
                    // "Sacrifice this."
                    ((None | Some("Self"), None | Some("Self")), _)
                        if rd.rec.get("ValidTgts").is_none() =>
                    {
                        if rd.rec.get("Amount").is_some() {
                            self.block("Sacrifice:shape");
                            return None;
                        }
                        Effect::SacrificeThis
                    }
                    ((_, Some(valid)), _) if valid != "Self" => {
                        let who = match self.target(&rd, chain, false) {
                            Some(i) if chain.targets[i as usize].allows_players() => {
                                Some(PlayerRef::Target(i))
                            }
                            Some(_) => {
                                self.block("Sacrifice:shape");
                                return None;
                            }
                            None => self.player(&rd, chain, triggered),
                        };
                        let what = self.selector(valid);
                        let amount = self.amount(rd.get("Amount").unwrap_or("1"), chain);
                        Effect::Sacrifice {
                            who: who?,
                            what: what?,
                            amount: amount?,
                        }
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
                    ((_, Some(mode @ ("TgtChoose" | "Hand"))), _) => {
                        let who = match self.target(&rd, chain, false) {
                            Some(i) if chain.targets[i as usize].allows_players() => {
                                Some(PlayerRef::Target(i))
                            }
                            Some(_) => {
                                self.block("Discard:shape");
                                return None;
                            }
                            None => self.player(&rd, chain, triggered),
                        };
                        if mode == "Hand" {
                            if rd.rec.get("NumCards").is_some() {
                                self.block("Discard:shape");
                                return None;
                            }
                            Effect::DiscardHand { who: who? }
                        } else {
                            let amount = self.amount(rd.get("NumCards").unwrap_or("1"), chain);
                            Effect::Discard {
                                who: who?,
                                amount: amount?,
                            }
                        }
                    }
                    ((_, mode), _) => {
                        self.block(format!("Discard:mode {}", mode.unwrap_or("?")));
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

    /// `LibraryPosition$` of a move to a library: `0` (the default) is the top, `-1` the
    /// bottom.
    fn library_position(&mut self, rd: &Rd<'a>) -> Option<CardDest> {
        match rd.get("LibraryPosition") {
            None | Some("0") => Some(CardDest::LibraryTop),
            Some("-1") => Some(CardDest::LibraryBottom),
            Some(_) => {
                self.block("param:ChangeZone.LibraryPosition");
                None
            }
        }
    }

    /// The alternatives of a filter as selectors (a card qualifies if it matches any).
    /// `Card` alone is "anything": no selectors. Off the battlefield `Permanent` is a
    /// permanent card (CR 110.4a).
    fn card_alts(&mut self, text: &str, off_battlefield: bool) -> Option<Vec<Selector>> {
        if text.trim() == "Card" {
            return Some(Vec::new());
        }
        let mut out = Vec::new();
        for alt in text.split(',') {
            let alt = alt.trim();
            match self.alt(alt)? {
                Alt::Sel(mut s) => {
                    if off_battlefield && alt.split('.').next() == Some("Permanent") {
                        s.not_types.extend([CardType::Instant, CardType::Sorcery]);
                    }
                    out.push(s);
                }
                Alt::This => {
                    self.block("filter:Self-among-cards");
                    return None;
                }
            }
        }
        Some(out)
    }

    /// A ChangeZone that takes cards out of a hidden zone as it resolves (Forge's
    /// "hidden-origin mode", docs/forge/effects.md §1): a library search (CR 701.23), or
    /// cards chosen from a hand.
    fn choose_cards(&mut self, rd: &Rd<'a>, chain: &mut Chain, triggered: bool) -> Option<Effect> {
        let from = match rd.get("Origin") {
            Some("Library") => CardZone::Library,
            _ => CardZone::Hand,
        };
        // Whose zone: the activator's unless `DefinedPlayer$` says otherwise.
        let who = match rd.get("DefinedPlayer") {
            None | Some("You") => PlayerRef::You,
            Some("Player") => PlayerRef::Each,
            Some("Player.Opponent" | "Opponent") => PlayerRef::EachOpponent,
            Some("TriggeredCardController") if triggered => PlayerRef::TriggeringController,
            Some(other) => {
                self.block(format!("defined:player {other}"));
                return None;
            }
        };
        let filter = rd.get("ChangeType");
        let cards = match filter {
            None => Some(Vec::new()),
            Some(f) => self.card_alts(f, true),
        };
        let count = self.amount(rd.get("ChangeNum").unwrap_or("1"), chain);
        // Without `Mandatory$` every pick may be declined (CR 701.23b).
        let must = rd.get("Mandatory").is_some();
        let to = match rd.get("Destination") {
            Some("Battlefield") => Some(CardDest::Battlefield {
                tapped: rd.get("Tapped").is_some(),
            }),
            Some("Hand") => Some(CardDest::Hand),
            Some("Graveyard") => Some(CardDest::Graveyard),
            Some("Exile") => Some(CardDest::Exile),
            Some("Library") => self.library_position(rd),
            other => {
                self.block(format!("ChangeZone:->{}", other.unwrap_or("?")));
                None
            }
        };
        let to = to?;
        if rd.rec.get("Tapped").is_some() && !matches!(to, CardDest::Battlefield { .. }) {
            self.block("param:ChangeZone.Tapped");
            return None;
        }
        // Forge reveals what was found when a quality was asked for and the cards don't
        // go to the battlefield ("search for a creature card, reveal it, …").
        let restricted = filter.is_some_and(|f| f.trim() != "Card");
        let reveal = rd.get("NoReveal").is_none()
            && (rd.get("Reveal").is_some()
                || (restricted && !matches!(to, CardDest::Battlefield { .. })));
        // A library that was searched is shuffled (CR 701.23, the effects all say so)
        // unless the script says not to. `ShuffleNonMandatory$` only matters when a player
        // can't search at all, which nothing in this engine causes.
        rd.get("ShuffleNonMandatory");
        let no_shuffle = rd.get("NoShuffle").is_some() || rd.get("Shuffle") == Some("False");
        if let Some(v) = rd.rec.get("Shuffle")
            && (from != CardZone::Library || !matches!(v, "True" | "False"))
        {
            self.block("param:ChangeZone.Shuffle");
            return None;
        }
        let shuffle = from == CardZone::Library && !no_shuffle;
        let effect = Effect::ChooseCards {
            who,
            from,
            cards: cards?,
            count: count?,
            must,
            to,
            reveal,
            shuffle,
        };
        // "You may search…": one yes/no before anything is looked at.
        Some(match rd.get("Optional") {
            None => effect,
            Some(_) => Effect::May(Box::new(effect)),
        })
    }

    /// `NumAtt$` / `NumDef$` / `KW$` of Pump and PumpAll (which last until end of turn).
    fn pump_effects(&mut self, rd: &Rd<'a>, chain: &mut Chain) -> Option<Vec<ContinuousEffect>> {
        let int =
            |t: Option<&str>| t.map_or(Some(0), |t| t.trim_start_matches('+').parse::<i32>().ok());
        let (att, def) = (rd.get("NumAtt"), rd.get("NumDef"));
        let mut effects = Vec::new();
        match (int(att), int(def)) {
            (Some(p), Some(t)) => {
                if (p, t) != (0, 0) {
                    effects.push(ContinuousEffect::ModifyPT(p, t));
                }
            }
            // "+X/+X", "-X/-0": worked out as the effect resolves.
            _ => {
                let mut part = |text: Option<&str>| -> Option<Signed> {
                    let text = text.unwrap_or("0");
                    let negative = text.starts_with('-');
                    let amount = self.amount(text.trim_start_matches(['+', '-']), chain)?;
                    Some(Signed { negative, amount })
                };
                let (power, toughness) = (part(att), part(def));
                effects.push(ContinuousEffect::ModifyPTBy {
                    power: Box::new(power?),
                    toughness: Box::new(toughness?),
                });
            }
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
    fn mode(&mut self, first: &'a Record, triggered: bool, root: &str) -> Option<Mode> {
        let mut chain = Chain::default();
        let mut kind = root;
        let mut next = Some(first);
        let mut ok = true;
        let mut steps = 0;
        while let Some(rec) = next {
            steps += 1;
            if steps > 32 {
                self.block("chain:too-long");
                return None;
            }
            match self.effect(rec, &mut chain, triggered, kind) {
                Some(e) => chain.effects.push(e),
                None => ok = false,
            }
            kind = "DB";
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
        // `Defined$ Targeted` means every target of the chain; with exactly one target slot
        // that is unambiguous.
        if chain.refers_to_targets.get() && chain.targets.len() != 1 {
            self.block("defined:Targeted-with-several-targets");
            ok = false;
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
    fn body(&mut self, rec: &'a Record, triggered: bool, root: &str) -> Option<Body> {
        if rec.head().map(|h| h.1) != Some("Charm") {
            return self.mode(rec, triggered, root).map(Body::Effect);
        }
        if rec.head().map(|h| h.0) != Some(root) {
            self.block(format!("record:Charm not {root}$"));
            return None;
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
            modes.push(self.mode(rec, triggered, "DB")?);
        }
        Some(Body::Modal { min, max, modes })
    }

    // ------------------------------------------------------------------ conditions, costs

    /// "As long as …" on a static ability: `IsPresent$` (also "no …": `PresentCompare$
    /// EQ0`, `LE<n>`), or one of Forge's named conditions.
    fn static_condition(&mut self, rd: &Rd<'a>) -> Option<Option<Condition>> {
        let named = match rd.get("Condition") {
            None => None,
            Some("PlayerTurn") => Some(Condition::YourTurn),
            Some("NotPlayerTurn") => Some(Condition::NotYourTurn),
            // CR 702 ability words, by their definitions.
            Some("Threshold") => Some(Condition::CardsInGraveyard {
                cards: Selector::default(),
                at_least: 7,
            }),
            Some("Metalcraft") => Some(Condition::Control {
                what: Selector {
                    types: vec![CardType::Artifact],
                    controller: Who::You,
                    ..Selector::default()
                },
                at_least: 3,
            }),
            Some("Hellbent") => Some(Condition::HandAtMost(0)),
            Some(other) => {
                self.block(format!("condition:{other}"));
                return None;
            }
        };
        // "N or fewer": only for permanents on the battlefield.
        let at_most = match (rd.rec.get("IsPresent"), rd.rec.get("PresentCompare")) {
            (Some(_), Some("EQ0")) => Some(0),
            (Some(_), Some(c)) => c.strip_prefix("LE").and_then(|n| n.parse::<u32>().ok()),
            _ => None,
        };
        let present = match at_most {
            Some(at_most) if rd.rec.get("PresentZone").is_none() => {
                rd.get("PresentCompare");
                let what = rd.get("IsPresent").and_then(|f| self.selector(f))?;
                Some(Condition::ControlAtMost { what, at_most })
            }
            _ => self.condition(rd)?,
        };
        match (named, present) {
            (Some(_), Some(_)) => {
                self.block("condition:two");
                None
            }
            (a, b) => Some(a.or(b)),
        }
    }

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
        let sel = match zone {
            None => self.selector(filter),
            Some(_) => self.card_selector(filter),
        };
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
        for tok in cost_tokens(text) {
            let kind = tok.split('<').next().unwrap_or(tok);
            // `Name<Amount/Type/Description>`: the description is text only.
            let args: Vec<&str> = tok
                .strip_prefix(kind)
                .and_then(|r| r.strip_prefix('<'))
                .and_then(|r| r.strip_suffix('>'))
                .map(|r| r.splitn(3, '/').collect())
                .unwrap_or_default();
            let count = args.first().and_then(|n| n.parse::<u32>().ok());
            match (kind, tok.contains('<')) {
                ("T", false) => cost.tap = true,
                ("Sac", true) if args.get(1) == Some(&"CARDNAME") && count == Some(1) => {
                    cost.sacrifice_self = true
                }
                ("Sac" | "Discard", true) => {
                    let (Some(count), Some(types)) = (count.filter(|n| *n > 0), args.get(1)) else {
                        self.block(format!("cost:{kind}-amount"));
                        return None;
                    };
                    // Alternatives are separated by `;` here ("an artifact or creature").
                    let mut what = Vec::new();
                    for alt in types.split(';') {
                        match self.alt(alt.trim())? {
                            Alt::Sel(s) => what.push(s),
                            Alt::This => {
                                self.block(format!("cost:{kind}-this"));
                                return None;
                            }
                        }
                    }
                    let slot = if kind == "Sac" {
                        &mut cost.sacrifice
                    } else {
                        // A card in a hand has types and subtypes; nothing else is asked.
                        let plain = what.iter().all(|s| {
                            *s == Selector {
                                types: s.types.clone(),
                                not_types: s.not_types.clone(),
                                subtypes: s.subtypes.clone(),
                                not_subtypes: s.not_subtypes.clone(),
                                ..Selector::default()
                            }
                        });
                        if !plain {
                            self.block("cost:Discard-filter");
                            return None;
                        }
                        &mut cost.discard
                    };
                    if slot.is_some() {
                        self.block(format!("cost:{kind}-twice"));
                        return None;
                    }
                    *slot = Some(CostChoice { count, what });
                }
                ("PayLife", true) => match count {
                    Some(n) if cost.life == 0 && n > 0 => cost.life = n,
                    _ => {
                        self.block("cost:PayLife-amount");
                        return None;
                    }
                },
                (_, false)
                    if tok.parse::<u32>().is_ok()
                        || matches!(tok, "W" | "U" | "B" | "R" | "G" | "C") =>
                {
                    mana.push_str(&format!("{{{tok}}}"))
                }
                _ => {
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
                // A spell's `Cost$` normally just repeats its mana cost; anything else is
                // an additional or alternative cost.
                if let Some(text) = rec.get("Cost") {
                    let printed = self.face.mana_cost.as_deref().unwrap_or("no cost");
                    let same = text.split_whitespace().eq(printed.split_whitespace());
                    if !same {
                        // The whole cost is restated: the card's mana cost plus what is
                        // paid in addition ("as an additional cost to cast this spell, …").
                        let mut cost = self.cost(text)?;
                        let mana = std::mem::take(&mut cost.mana);
                        let printed = mana_cost(printed).ok().flatten();
                        if mana != printed || cost.tap || cost.sacrifice_self {
                            self.block("spell:other-cost");
                            return None;
                        }
                        self.additional.push(AbilityDef::AdditionalCost(cost));
                    }
                }
                self.body(rec, false, "SP").map(AbilityDef::Spell)
            }
            "AB" if api == "Mana" => {
                let rd = Rd::new(rec, "Mana");
                let cost = rd.get("Cost").and_then(|c| self.cost(c));
                let letter = |w: &&str| match *w {
                    "W" => Some(ManaType::White),
                    "U" => Some(ManaType::Blue),
                    "B" => Some(ManaType::Black),
                    "R" => Some(ManaType::Red),
                    "G" => Some(ManaType::Green),
                    "C" => Some(ManaType::Colorless),
                    _ => None,
                };
                let colors = || -> Vec<ManaType> {
                    Color::ALL.into_iter().map(ManaType::from_color).collect()
                };
                // `Amount$`: how many times the whole of `Produced$` is made.
                let times = match rd.get("Amount") {
                    None => Some(1),
                    Some(n) => n.parse::<u32>().ok().filter(|n| *n > 0),
                };
                let words: Vec<&str> = rd
                    .get("Produced")
                    .unwrap_or("")
                    .split_whitespace()
                    .collect();
                let produce = times.and_then(|n| match words.as_slice() {
                    // One colour of the player's choice, `n` mana of it.
                    ["Any"] if n == 1 => Some(ManaProduce::AnyColor),
                    ["Any"] => Some(ManaProduce::Times {
                        of: colors(),
                        count: n,
                    }),
                    // "Add {W} or {U}": each mana any of the listed colours. More than
                    // one such mana ("in any combination") is not modelled.
                    ["Combo", "Any"] if n == 1 => Some(ManaProduce::AnyColor),
                    ["Combo", of @ ..] if n == 1 && !of.is_empty() => {
                        let mut of: Vec<ManaType> = of.iter().map(letter).collect::<Option<_>>()?;
                        of.dedup();
                        match of.as_slice() {
                            [one] => Some(ManaProduce::One(*one)),
                            _ => Some(ManaProduce::OneOf(of)),
                        }
                    }
                    // Fixed mana: "C C", "G" with `Amount$ 3`. Only one type at a time.
                    [first, ..] => {
                        let all: Vec<ManaType> = words.iter().map(letter).collect::<Option<_>>()?;
                        let t = letter(first)?;
                        if all.iter().any(|x| *x != t) {
                            return None;
                        }
                        match all.len() as u32 * n {
                            1 => Some(ManaProduce::One(t)),
                            count => Some(ManaProduce::Times { of: vec![t], count }),
                        }
                    }
                    [] => None,
                });
                rd.finish(&mut self.blockers);
                let Some(produce) = produce else {
                    self.block("Mana:produced");
                    return None;
                };
                let cost = cost?;
                let just_tap = Cost {
                    tap: true,
                    ..Cost::default()
                };
                if cost != just_tap {
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
                let body = self.body(rec, false, "AB");
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
            let before = self.blockers.len();
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
            let secondary = match rd.get("Secondary") {
                None => false,
                Some("True") => true,
                // Forge only tests that the key is present.
                Some(_) => {
                    self.block("trig:Secondary value");
                    true
                }
            };
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
            self.in_trigger = true;
            let body = match self.sub(exec) {
                Some(r) => self.body(r, true, "DB"),
                None => None,
            };
            self.in_trigger = false;
            let (Some(when), Some(intervening_if), Some(mut body)) = (event, cond, body) else {
                self.explained(before, "trigger");
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
                let affected = match (rd.get("AffectedDefined"), rd.get("Affected")) {
                    (None, Some("Card.Self")) | (Some("Self"), None | Some("Card.Self")) => {
                        Some(Affected::This)
                    }
                    // "Enchanted creature …". Forge filters the enchanted permanent again
                    // with `Affected$`; that says nothing new only when it repeats what the
                    // Aura can enchant at all.
                    // "Equipped creature …": the same for an Equipment.
                    (Some("Equipped"), None | Some("Creature")) if self.equipment => {
                        Some(Affected::Enchanted)
                    }
                    (Some("Enchanted"), again) => match (self.enchant, again) {
                        (Some(_), None) => Some(Affected::Enchanted),
                        (Some(e), Some(a)) if e == a => Some(Affected::Enchanted),
                        (None, _) => {
                            self.block("static:Enchanted-without-Enchant");
                            None
                        }
                        (Some(_), Some(_)) => {
                            self.block("static:Enchanted-as-long-as");
                            None
                        }
                    },
                    (Some(other), _) => {
                        self.block(format!("static:AffectedDefined {other}"));
                        None
                    }
                    (None, Some(f)) => self.selector(f).map(Affected::All),
                    (None, None) => {
                        self.block("static:no-Affected");
                        None
                    }
                };
                let int = |t: Option<&str>| {
                    t.map_or(Some(0), |t| t.trim_start_matches('+').parse::<i32>().ok())
                };
                let (att, def) = (rd.get("AddPower"), rd.get("AddToughness"));
                let mut effects = Vec::new();
                match (int(att), int(def)) {
                    (Some(p), Some(t)) => {
                        if (p, t) != (0, 0) {
                            effects.push(ContinuousEffect::ModifyPT(p, t));
                        }
                    }
                    // "+1/+1 for each [permanent]": counted continuously. Only counts of
                    // permanents — the layer walk can't evaluate anything else in place.
                    _ => {
                        let mut part = |text: Option<&str>| -> Option<Signed> {
                            let text = text.unwrap_or("0");
                            let negative = text.starts_with('-');
                            let name = text.trim_start_matches(['+', '-']);
                            let amount = match name.parse::<u32>() {
                                Ok(n) => Amount::Fixed(n),
                                Err(_) => self.expr(name)?,
                            };
                            match &amount {
                                Amount::Fixed(_) => {}
                                Amount::Count(s)
                                    if s.power_min.is_none() && s.toughness_min.is_none() => {}
                                _ => {
                                    self.block("static:variable-pt");
                                    return None;
                                }
                            }
                            Some(Signed { negative, amount })
                        };
                        let (power, toughness) = (part(att), part(def));
                        effects.push(ContinuousEffect::ModifyPTBy {
                            power: Box::new(power?),
                            toughness: Box::new(toughness?),
                        });
                    }
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
                // "You control [it]" (layer 2).
                match rd.get("GainControl") {
                    None => {}
                    Some("You") => effects.insert(0, ContinuousEffect::GainControl),
                    Some(other) => {
                        self.block(format!("param:static Continuous.GainControl={other}"));
                        return None;
                    }
                }
                if effects.is_empty() {
                    self.block("static:Continuous-nothing");
                    return None;
                }
                let only_if = self.static_condition(&rd)?;
                affected.map(|affected| AbilityDef::Static {
                    affected,
                    effects,
                    only_if,
                })
            }
            // Combat restrictions (CR 508.1c, 509.1b). One Forge line may carry several
            // modes ("can't attack or block").
            "CantAttack" | "CantBlock" | "CantAttack,CantBlock" => {
                let affected = rd.get("ValidCard").and_then(|f| self.affected(f));
                let affected = affected.or_else(|| {
                    self.block(format!("static:{mode}-who"));
                    None
                })?;
                let mut rules: Vec<CombatRule> = mode
                    .split(',')
                    .map(|m| match m {
                        "CantAttack" => CombatRule::CantAttack,
                        _ => CombatRule::CantBlock,
                    })
                    .collect();
                let first = rules.remove(0);
                for rule in rules {
                    self.additional.push(AbilityDef::CombatRule {
                        affected: affected.clone(),
                        rule,
                    });
                }
                Some(AbilityDef::CombatRule {
                    affected,
                    rule: first,
                })
            }
            "CantBlockBy" => {
                let (attacker, blocker) = (rd.get("ValidAttacker"), rd.get("ValidBlocker"));
                let is_self = |f: &str| matches!(f, "Card.Self" | "Creature.Self");
                match (attacker, blocker) {
                    // "This can't block [those attackers]."
                    (Some(a), Some(b)) if is_self(b) && !is_self(a) => {
                        let sel = self.selector(a)?;
                        Some(AbilityDef::CombatRule {
                            affected: Affected::This,
                            rule: CombatRule::CantBlockAttackers(sel),
                        })
                    }
                    // "[It] can't be blocked [by those]."
                    (Some(a), by) => {
                        let affected = self.affected(a);
                        let by = match by {
                            None => Some(None),
                            Some(b) => self.selector(b).map(Some),
                        };
                        Some(AbilityDef::CombatRule {
                            affected: affected?,
                            rule: CombatRule::CantBeBlocked { by: by? },
                        })
                    }
                    (None, _) => {
                        self.block("static:CantBlockBy-who");
                        None
                    }
                }
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
                        let spells = self.card_selector(f);
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

    /// Who a static rule is about: the card itself, what it is attached to, or everything
    /// matching a filter.
    fn affected(&mut self, text: &str) -> Option<Affected> {
        match text {
            "Card.Self" | "Creature.Self" => Some(Affected::This),
            "Creature.EquippedBy" | "Card.EquippedBy" if self.equipment => {
                Some(Affected::Enchanted)
            }
            "Creature.EnchantedBy" | "Card.EnchantedBy" | "Permanent.EnchantedBy" => {
                // Only for an Aura whose enchant filter says no more than this one.
                match (self.enchant, text.split('.').next()) {
                    (Some("Creature"), Some("Creature" | "Card" | "Permanent")) => {
                        Some(Affected::Enchanted)
                    }
                    _ => {
                        self.block("static:EnchantedBy");
                        None
                    }
                }
            }
            _ => self.selector(text).map(Affected::All),
        }
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
        in_trigger: false,
        additional: Vec::new(),
        enchant: None,
        equipment: face.types.iter().any(|t| t == "Equipment"),
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
            // Basic land types on a land carry its mana ability (CR 305.6); every other
            // subtype, a land's included ("Desert", "Gate"), is kept by name.
            "Plains" | "Island" | "Swamp" | "Mountain" | "Forest"
                if def.card_types.contains(&CardType::Land) =>
            {
                def.land_types.push(match word.as_str() {
                    "Plains" => BasicLandType::Plains,
                    "Island" => BasicLandType::Island,
                    "Swamp" => BasicLandType::Swamp,
                    "Mountain" => BasicLandType::Mountain,
                    _ => BasicLandType::Forest,
                })
            }
            "Ongoing" => def.supertypes.push(Supertype::Ongoing),
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
    // Abilities, in the order the engine's own scripts use: as written, line kind by line
    // kind (the order within a kind is the script's).
    let mut abilities: Vec<(usize, AbilityDef)> = Vec::new();
    // Equip abilities are printed last.
    let mut equips: Vec<AbilityDef> = Vec::new();
    for k in &face.keywords {
        // `Enchant:<what>[:<description>]` (CR 702.5): what the Aura targets as a spell and
        // may be attached to. It is printed first.
        if let Some(rest) = k.strip_prefix("Enchant:") {
            let what = rest.split(':').next().unwrap_or(rest).trim();
            let filter = match what {
                _ if tx.enchant.is_some() => None,
                "Creature" => Some(TargetFilter::Creature),
                // Players can't be enchanted yet.
                "Player" | "Opponent" | "Player.Opponent" => None,
                // "Enchant land", "enchant creature you control", "enchant permanent".
                _ => {
                    let saved = tx.blockers.clone();
                    let sels: Option<Vec<Selector>> = tx.alts(what).and_then(|alts| {
                        alts.into_iter()
                            .map(|a| match a {
                                Alt::Sel(s) => Some(s),
                                Alt::This => None,
                            })
                            .collect()
                    });
                    // The filter's own blockers say less than "this Aura's enchant".
                    tx.blockers = saved;
                    sels.map(|s| TargetFilter::Permanent(merge_bare(s)))
                }
            };
            match filter {
                Some(f) => {
                    tx.enchant = Some(what);
                    abilities.push((0, AbilityDef::Enchant(f)));
                }
                None => {
                    tx.block(format!("enchant:{what}"));
                    tx.enchant.get_or_insert(what);
                }
            }
            continue;
        }
        // `Equip:<cost>` (CR 702.6a): "[cost]: Attach this permanent to target creature you
        // control. Activate only as a sorcery." Restricted equips ("equip legendary
        // creature") and extra parameters are not compiled.
        if let Some(rest) = k.strip_prefix("Equip:") {
            let cost = if rest.contains(':') {
                tx.block("kw:Equip-restricted");
                None
            } else {
                tx.cost(rest)
            };
            if let Some(cost) = cost {
                equips.push(AbilityDef::Activated {
                    cost,
                    timing: Timing::Sorcery,
                    only_if: None,
                    body: Body::Effect(Mode {
                        targets: vec![TargetFilter::Permanent(vec![Selector {
                            types: vec![CardType::Creature],
                            controller: Who::You,
                            ..Selector::default()
                        }])],
                        effect: Effect::AttachThis {
                            to: ObjRef::Target(0),
                        },
                    }),
                });
            }
            continue;
        }
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
    let order = line_order(face);
    for (i, rec) in face.abilities.iter().enumerate() {
        let before = tx.blockers.len();
        match tx.ability(rec) {
            Some(a) => {
                // "As an additional cost…" is printed before what the spell does.
                for extra in tx.additional.drain(..) {
                    abilities.push((order.abilities[i], extra));
                }
                abilities.push((order.abilities[i], a))
            }
            None => tx.explained(before, "ability"),
        }
    }
    // Merged ("Secondary") triggers stay at their first line's place.
    for (line, a) in tx.triggers() {
        abilities.push((order.triggers[line], a));
    }
    for (i, rec) in face.statics.iter().enumerate() {
        let before = tx.blockers.len();
        match tx.static_ability(rec) {
            Some(a) => {
                abilities.push((order.statics[i], a));
                for extra in tx.additional.drain(..) {
                    abilities.push((order.statics[i], extra));
                }
            }
            None => tx.explained(before, "static"),
        }
    }
    for (i, rec) in face.replacements.iter().enumerate() {
        let before = tx.blockers.len();
        match tx.replacement(rec) {
            Some(a) => abilities.push((order.replacements[i], a)),
            None => tx.explained(before, "replacement"),
        }
    }
    abilities.extend(equips.into_iter().map(|a| (usize::MAX, a)));
    abilities.sort_by_key(|(i, _)| *i);
    def.abilities = abilities.into_iter().map(|(_, a)| a).collect();
    // A variable holding an ability that nothing refers to means part of the card was
    // not understood.
    // (Only reported for a card that is otherwise understood: when something else is
    // blocked, the abilities it would have used are unused as a consequence.)
    if tx.blockers.is_empty() {
        for (name, _) in &parsed {
            if !tx.used_svars.contains(name) {
                tx.block("svar:unused-ability");
            }
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
        let b = blockers(
            "Name:Test Card\nManaCost:G\nTypes:Creature Elf\nPT:1/1\nK:Banding\nMystery:1",
        );
        assert!(
            b.contains("kw:Banding") && b.contains("line:Mystery"),
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
