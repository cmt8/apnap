//! Card vocabulary shared by the engine, the card database and card scripts.
//!
//! `no_std` + `alloc`: this crate is a dependency of `engine-core` and must stay pure.
//! All enums are **externally tagged** (serde default) because RON cannot express
//! internally tagged or untagged enums (spec §7.2, CLAUDE.md invariant 10).
//!
//! M1 scope: printed characteristics, keywords (haste, flash), spell abilities with modes and
//! targets, activated abilities, and `{T}` mana abilities. Triggers, statics and
//! replacements arrive in M2. Card files (`CardScript`) are parsed and validated by `card-db`.
#![no_std]

extern crate alloc;

pub mod describe;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

/// Index of a [`CardDef`] inside a [`CardCatalog`]. Dense, assigned in catalog order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardDefId(pub u32);

/// The five colors (CR 105.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
}

impl Color {
    pub const ALL: [Color; 5] = [
        Color::White,
        Color::Blue,
        Color::Black,
        Color::Red,
        Color::Green,
    ];
}

/// One symbol of a printed mana cost (CR 107.4). Hybrid/Phyrexian/X arrive with M1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ManaSymbol {
    Generic(u32),
    Colored(Color),
    /// `{C}`: specifically colorless mana.
    Colorless,
    /// `{X}` (CR 107.3): its value is announced while casting.
    X,
}

/// The six mana types (CR 106.1b): five colors plus colorless.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ManaType {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

impl ManaType {
    pub const ALL: [ManaType; 6] = [
        ManaType::White,
        ManaType::Blue,
        ManaType::Black,
        ManaType::Red,
        ManaType::Green,
        ManaType::Colorless,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_color(c: Color) -> Self {
        match c {
            Color::White => ManaType::White,
            Color::Blue => ManaType::Blue,
            Color::Black => ManaType::Black,
            Color::Red => ManaType::Red,
            Color::Green => ManaType::Green,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            ManaType::White => "{W}",
            ManaType::Blue => "{U}",
            ManaType::Black => "{B}",
            ManaType::Red => "{R}",
            ManaType::Green => "{G}",
            ManaType::Colorless => "{C}",
        }
    }
}

/// A printed mana cost. Serialized as its Oracle text form, e.g. `"{X}{R}"`, so card files
/// stay readable (it is a plain string, which RON handles natively).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaCost {
    pub symbols: Vec<ManaSymbol>,
}

impl Serialize for ManaCost {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_text())
    }
}

impl<'de> Deserialize<'de> for ManaCost {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        ManaCost::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl ManaCost {
    /// Mana value (CR 202.3).
    pub fn mana_value(&self) -> u32 {
        self.symbols
            .iter()
            .map(|s| match s {
                ManaSymbol::Generic(n) => *n,
                ManaSymbol::Colored(_) | ManaSymbol::Colorless => 1,
                // CR 202.3e: X is 0 everywhere except on the stack.
                ManaSymbol::X => 0,
            })
            .sum()
    }

    pub fn has_x(&self) -> bool {
        self.symbols.contains(&ManaSymbol::X)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for s in &self.symbols {
            match s {
                ManaSymbol::Generic(n) => {
                    let _ = core::fmt::Write::write_fmt(&mut out, format_args!("{{{n}}}"));
                }
                ManaSymbol::Colored(c) => out.push_str(ManaType::from_color(*c).symbol()),
                ManaSymbol::Colorless => out.push_str("{C}"),
                ManaSymbol::X => out.push_str("{X}"),
            }
        }
        out
    }

    /// Parse the textual form used by Oracle text, e.g. `"{2}{G}{G}"`.
    pub fn parse(text: &str) -> Result<ManaCost, ParseManaCostError> {
        let mut symbols = Vec::new();
        let mut rest = text.trim();
        while !rest.is_empty() {
            let Some(body) = rest.strip_prefix('{') else {
                return Err(ParseManaCostError);
            };
            let Some(end) = body.find('}') else {
                return Err(ParseManaCostError);
            };
            let sym = &body[..end];
            rest = &body[end + 1..];
            symbols.push(match sym {
                "W" => ManaSymbol::Colored(Color::White),
                "U" => ManaSymbol::Colored(Color::Blue),
                "B" => ManaSymbol::Colored(Color::Black),
                "R" => ManaSymbol::Colored(Color::Red),
                "G" => ManaSymbol::Colored(Color::Green),
                "C" => ManaSymbol::Colorless,
                "X" => ManaSymbol::X,
                n => ManaSymbol::Generic(n.parse().map_err(|_| ParseManaCostError)?),
            });
        }
        Ok(ManaCost { symbols })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseManaCostError;

impl fmt::Display for ParseManaCostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid mana cost")
    }
}

impl core::error::Error for ParseManaCostError {}

/// Card types (CR 205.2a).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CardType {
    Artifact,
    Battle,
    Creature,
    Enchantment,
    Instant,
    Kindred,
    Land,
    Planeswalker,
    Sorcery,
}

/// Supertypes (CR 205.4a).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Supertype {
    Basic,
    Legendary,
    Ongoing,
    Snow,
    World,
}

/// Basic land types (CR 205.3i). Each grants an intrinsic mana ability (CR 305.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BasicLandType {
    Plains,
    Island,
    Swamp,
    Mountain,
    Forest,
}

impl BasicLandType {
    /// CR 305.6: Plains → W, Island → U, Swamp → B, Mountain → R, Forest → G.
    pub fn produces(self) -> Color {
        match self {
            BasicLandType::Plains => Color::White,
            BasicLandType::Island => Color::Blue,
            BasicLandType::Swamp => Color::Black,
            BasicLandType::Mountain => Color::Red,
            BasicLandType::Forest => Color::Green,
        }
    }
}

/// Immutable printed card data. The engine never reads this directly for objects on the
/// battlefield or stack; it goes through `engine_core::layers::chars` (CLAUDE.md invariant 3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct CardDef {
    pub name: String,
    pub mana_cost: Option<ManaCost>,
    pub supertypes: Vec<Supertype>,
    pub card_types: Vec<CardType>,
    pub land_types: Vec<BasicLandType>,
    pub creature_types: Vec<String>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub keywords: Vec<Keyword>,
    pub abilities: Vec<AbilityDef>,
    /// Color indicator / token colors. `None` = derived from the mana cost (CR 202.2).
    pub colors: Option<Vec<Color>>,
    /// A token definition (CR 111): only created by effects, never in decks.
    pub token: bool,
    /// What effects call this definition when its name isn't unique: several different
    /// tokens are all named "Dragon" (CR 111.4), so they are told apart by key.
    pub key: Option<String>,
}

impl CardDef {
    /// CR 702.5a: the Aura's enchant restriction, if any.
    pub fn enchant(&self) -> Option<TargetFilter> {
        self.abilities.iter().find_map(|a| match a {
            AbilityDef::Enchant(f) => Some(f.clone()),
            _ => None,
        })
    }

    /// Printed colors (CR 105.2, 202.2).
    pub fn printed_colors(&self) -> Vec<Color> {
        if let Some(c) = &self.colors {
            return c.clone();
        }
        let mut out: Vec<Color> = Vec::new();
        for s in self.mana_cost.iter().flat_map(|m| &m.symbols) {
            if let ManaSymbol::Colored(c) = s
                && !out.contains(c)
            {
                out.push(*c);
            }
        }
        out.sort();
        out
    }

    pub fn has_type(&self, t: CardType) -> bool {
        self.card_types.contains(&t)
    }

    pub fn has_keyword(&self, k: Keyword) -> bool {
        self.keywords.contains(&k)
    }

    /// The card's spell ability (instants and sorceries), if any.
    pub fn spell_ability(&self) -> Option<&Body> {
        self.abilities.iter().find_map(|a| match a {
            AbilityDef::Spell(b) => Some(b),
            _ => None,
        })
    }
}

/// Keyword abilities (CR 702). M1: the two that change timing rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Keyword {
    /// CR 702.2: lethal from any damage (SBA 704.5h, and 702.2c for assignment).
    Deathtouch,
    /// CR 702.4: deals combat damage in both damage steps.
    DoubleStrike,
    /// CR 702.7: deals combat damage in the first damage step.
    FirstStrike,
    /// CR 702.15: its damage also gains its controller that much life.
    Lifelink,
    /// CR 702.111: can't be blocked except by two or more creatures.
    Menace,
    /// CR 702.19: excess combat damage may go to the player.
    Trample,
    /// CR 702.20: attacking doesn't cause it to tap.
    Vigilance,
    /// CR 702.8
    Flash,
    /// CR 702.9: can't be blocked except by creatures with flying and/or reach.
    Flying,
    /// CR 702.10
    Haste,
    /// CR 702.17: can block creatures with flying.
    Reach,
}

/// One ability printed on a card (CR 113).
// Card definitions are built once when the catalog loads and then only read; the size
// difference between variants costs nothing that matters.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AbilityDef {
    /// The spell ability of an instant or sorcery (CR 113.3a).
    Spell(Body),
    /// A non-mana activated ability (CR 602): `[cost]: [effect]`.
    Activated {
        cost: Cost,
        #[serde(default)]
        timing: Timing,
        /// "Activate only if [condition]" (for the ability's controller).
        #[serde(default)]
        only_if: Option<Condition>,
        body: Body,
    },
    /// A static ability: "[Selector] spells you cast cost {N} less to cast" (generic mana
    /// only, CR 118.7a). The selector is matched against the spell being cast.
    CostReduction { spells: Selector, amount: u32 },
    /// "This spell costs {N} less to cast[, where N is …]": functions while it is cast.
    SelfCostReduction(Amount),
    /// An activated mana ability (CR 605.1a). M1: the cost must be exactly `{T}`.
    Mana { cost: Cost, produce: ManaProduce },
    /// A triggered ability (CR 603): "[When/Whenever/At] `when`, [if `intervening_if`,] `body`."
    Triggered {
        when: Trigger,
        #[serde(default)]
        intervening_if: Option<Condition>,
        body: Body,
    },
    /// A static ability generating a continuous effect (CR 604, 611.3): `effects` apply to
    /// `affected`. One ability = one effect that may have parts in several layers (CR 613.6).
    Static {
        affected: Affected,
        effects: Vec<ContinuousEffect>,
    },
    /// A static ability generating a replacement effect (CR 614).
    Replacement {
        replaces: Replaces,
        with: ReplaceWith,
    },
    /// CR 702.5: "Enchant [filter]" — what an Aura spell targets and what the Aura can
    /// enchant (CR 303.4a).
    Enchant(TargetFilter),
}

/// Whose / which player, relative to the ability's controller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Who {
    #[default]
    Any,
    You,
    Opponent,
}

/// Matches permanents (or, for "dies", the object as it last existed on the battlefield).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Selector {
    /// Must have at least one of these card types (empty = any).
    pub types: Vec<CardType>,
    /// Must have none of these card types ("noncreature").
    pub not_types: Vec<CardType>,
    pub supertypes: Vec<Supertype>,
    /// Must have at least one of these subtypes (empty = any): "Dinosaurs", "Dragon
    /// creatures". Subtypes are compared as the object currently has them (CR 205.3).
    pub subtypes: Vec<String>,
    /// Must have none of these subtypes ("non-Dragon", "non-Zombie").
    pub not_subtypes: Vec<String>,
    /// Must be at least one of these colors (empty = any): "black creatures".
    pub colors: Vec<Color>,
    /// "with power N or greater" / "with toughness N or greater". Read from the object's
    /// final characteristics, so these are for targets, triggers, conditions and one-shot
    /// effects — not for the affected set of a static ability (card-db rejects that).
    pub power_min: Option<i32>,
    pub toughness_min: Option<i32>,
    /// `Some(true)` = tapped only, `Some(false)` = untapped only.
    pub tapped: Option<bool>,
    /// Must have all of these keywords ("with flying").
    pub keywords: Vec<Keyword>,
    pub controller: Who,
    /// "another" / "other": excludes the ability's own source.
    pub other: bool,
    /// `Some(false)` = nontoken.
    pub token: Option<bool>,
}

/// What a trigger condition or replacement watches: the source itself, or matching objects.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjFilter {
    This,
    Matching(Selector),
    /// "this creature or another [selector]": the source itself whatever it is, or any
    /// *other* object matching the selector.
    ThisOr(Selector),
}

/// Trigger conditions (CR 603.1). M2 vocabulary.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Trigger {
    /// CR 603.6a: "When/Whenever [filter] enters".
    Enters(ObjFilter),
    /// "When/Whenever [filter] dies" — leaves the battlefield for a graveyard; looks back in
    /// time (CR 603.10a).
    Dies(ObjFilter),
    /// CR 603.2b: "At the beginning of [whose] [step]".
    BeginningOf { step: TriggerStep, whose: Who },
    /// "Whenever [who] casts a spell [of these types]" (empty = any spell).
    Cast {
        by: Who,
        #[serde(default)]
        types: Vec<CardType>,
    },
    /// CR 508.1m: "Whenever [filter] attacks" — it is declared as an attacker.
    Attacks(ObjFilter),
    /// One ability with several trigger events: "Whenever this creature enters or attacks".
    Any(Vec<Trigger>),
}

impl Trigger {
    /// The single trigger events this ability watches for.
    pub fn events(&self) -> Vec<&Trigger> {
        match self {
            Trigger::Any(v) => v.iter().flat_map(Trigger::events).collect(),
            other => alloc::vec![other],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggerStep {
    Upkeep,
    DrawStep,
    EndStep,
}

/// An intervening "if" clause (CR 603.4), evaluated for the ability's controller.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Condition {
    LifeAtMost(i32),
    LifeAtLeast(i32),
    /// "if you control N or more [selector]" (the selector is relative to the ability's
    /// controller and source).
    Control {
        what: Selector,
        at_least: u32,
    },
    /// "if you have N or more [cards] in your graveyard" (card types and subtypes only).
    CardsInGraveyard {
        cards: Selector,
        at_least: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Affected {
    /// The source permanent itself.
    This,
    /// The object the source is attached to ("enchanted creature", CR 303.4m).
    Enchanted,
    /// Every permanent matching the selector (re-evaluated continuously, CR 611.3a).
    All(Selector),
}

/// The parts of a continuous effect, each applied in its own layer (CR 613.1, 613.6).
/// Layers 1a/1b/3 have no vocabulary yet (D-023); the layer walk has their slots.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContinuousEffect {
    /// Layer 2: "You control [affected]" — the effect's controller gains control.
    GainControl,
    /// Layer 4: add card types and/or subtypes ("is also a creature").
    AddTypes {
        #[serde(default)]
        card_types: Vec<CardType>,
        #[serde(default)]
        subtypes: Vec<String>,
        #[serde(default)]
        land_types: Vec<BasicLandType>,
    },
    /// Layer 5
    SetColors(Vec<Color>),
    /// Layer 6
    AddKeywords(Vec<Keyword>),
    /// Layer 6
    LoseKeywords(Vec<Keyword>),
    /// Layer 6: "loses all abilities".
    LoseAllAbilities,
    /// Layer 7b: "has base power and toughness P/T" / "becomes P/T".
    SetPT(i32, i32),
    /// Layer 7c: "+P/+T".
    ModifyPT(i32, i32),
    /// Layer 7d: switch power and toughness.
    SwitchPT,
    /// Layer 7c: "+X/+X" (or "-X/-0" …) with amounts that are worked out when the spell or
    /// ability resolves and then stay fixed (CR 608.2h). Only in one-shot effects (`Apply`,
    /// `ApplyAll`): the engine turns it into a plain `ModifyPT` as the effect begins.
    ModifyPTBy {
        power: Box<Signed>,
        toughness: Box<Signed>,
    },
}

/// Events a static replacement effect watches for (CR 614). M2 vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Replaces {
    /// "If an effect would create one or more tokens under your control" (CR 614.16).
    CreateTokens,
    /// "If an effect would put one or more counters on a permanent you control" (CR 614.16;
    /// includes counters a permanent enters with, CR 122.6).
    PutCountersOnYourPermanents,
    /// "If a card would be put into a graveyard from anywhere".
    CardToGraveyard,
    /// "If this creature would die" (battlefield → graveyard).
    ThisDies,
    /// "This permanent enters …" (CR 614.1c–d): a self ETB replacement.
    ThisEnters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReplaceWith {
    /// "… twice that many … instead" (tokens or counters).
    Twice,
    /// "… exile it instead".
    Exile,
    /// "… shuffle it into its owner's library instead".
    ShuffleIntoLibrary,
    /// "enters tapped".
    EntersTapped,
    /// "enters with N [kind] counters".
    EntersWithCounters { kind: CounterKind, n: u32 },
}

/// An amount with a sign: "+X" or "-X".
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Signed {
    #[serde(default)]
    pub negative: bool,
    pub amount: Amount,
}

/// Counter kinds (CR 122). M2: the two P/T counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CounterKind {
    /// +1/+1 (CR 122.1a)
    PlusOne,
    /// -1/-1
    MinusOne,
}

/// What a spell or ability does: either a single effect or a modal choice (CR 700.2).
// Card definitions are built once when the catalog loads and then only read; the size
// difference between variants costs nothing that matters.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Body {
    Effect(Mode),
    /// "Choose `min`..=`max` —" followed by modes (CR 700.2). Modes are distinct (700.2d).
    Modal {
        min: u8,
        max: u8,
        modes: Vec<Mode>,
    },
}

impl Body {
    pub fn modes(&self) -> &[Mode] {
        match self {
            Body::Effect(m) => core::slice::from_ref(m),
            Body::Modal { modes, .. } => modes,
        }
    }

    pub fn is_modal(&self) -> bool {
        matches!(self, Body::Modal { .. })
    }
}

/// One mode: its targets (one per "target" word, CR 115.3) and its effect. `Target(i)`
/// inside `effect` refers to `targets[i]` of this mode.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Mode {
    #[serde(default)]
    pub targets: Vec<TargetFilter>,
    pub effect: Effect,
}

/// What a single target may be (CR 115).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetFilter {
    /// CR 115.4: a creature, player, planeswalker or battle (M1: creature or player).
    AnyTarget,
    Creature,
    Player,
    Opponent,
    /// A spell on the stack.
    Spell,
    /// A permanent matching any one of the selectors: "target artifact or enchantment",
    /// "target nonartifact creature", "target artifact, enchantment, or creature with
    /// power 4 or greater". `controller` is relative to the spell or ability's controller;
    /// `other` isn't meaningful for a target.
    Permanent(Vec<Selector>),
    /// A card in a graveyard: "target creature card from your graveyard", "target Zombie
    /// card from your graveyard", "target creature card from a graveyard". Only the card
    /// types and subtypes of `cards` are looked at; `whose` is whose graveyard.
    CardInGraveyard {
        cards: Selector,
        whose: Who,
    },
}

impl TargetFilter {
    /// Can an object effect (destroy, exile, return…) be pointed at this slot?
    pub fn allows_permanents(&self) -> bool {
        matches!(
            self,
            TargetFilter::AnyTarget
                | TargetFilter::Creature
                | TargetFilter::Permanent(_)
                | TargetFilter::CardInGraveyard { .. }
        )
    }

    pub fn allows_players(&self) -> bool {
        matches!(
            self,
            TargetFilter::AnyTarget | TargetFilter::Player | TargetFilter::Opponent
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Cost {
    pub mana: Option<ManaCost>,
    /// `{T}`
    pub tap: bool,
    /// "Sacrifice this [permanent]" (CR 701.21: its controller moves it to its owner's
    /// graveyard; it can't be regenerated or otherwise saved, but the move is replaceable).
    pub sacrifice_self: bool,
}

/// When an activated ability may be activated (CR 602.5d).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timing {
    #[default]
    Instant,
    /// "Activate only as a sorcery."
    Sorcery,
}

/// The mana a mana ability adds (one mana in M1).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ManaProduce {
    One(ManaType),
    /// "Add one mana of any color."
    AnyColor,
}

impl ManaProduce {
    pub fn options(&self) -> Vec<ManaType> {
        match self {
            ManaProduce::One(t) => alloc::vec![*t],
            ManaProduce::AnyColor => Color::ALL
                .iter()
                .map(|c| ManaType::from_color(*c))
                .collect(),
        }
    }
}

/// A one-shot effect (CR 609). M1 vocabulary.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Effect {
    /// Instructions in the order written (CR 608.2c).
    Seq(Vec<Effect>),
    /// The spell or the ability's source deals damage (CR 120).
    DealDamage {
        to: TargetRef,
        amount: Amount,
        #[serde(default)]
        by: DamageBy,
    },
    Draw {
        who: PlayerRef,
        amount: Amount,
    },
    GainLife {
        who: PlayerRef,
        amount: Amount,
    },
    LoseLife {
        who: PlayerRef,
        amount: Amount,
    },
    /// CR 701.6
    Counter {
        what: TargetRef,
    },
    /// Each player, in APNAP order, makes the choice; then all actions happen at once (CR 101.4).
    EachPlayer(EachPlayerAction),
    /// The source deals `amount` damage to each player, simultaneously.
    DamageEachPlayer {
        amount: Amount,
    },
    /// CR 701.8: destroy.
    Destroy {
        what: ObjRef,
    },
    /// "Return [it] to its owner's hand."
    ReturnToHand {
        what: ObjRef,
    },
    /// CR 111: create `count` tokens (a token card in the catalog) under `who`'s control.
    CreateToken {
        token: String,
        count: Amount,
        who: PlayerRef,
    },
    /// CR 122: put counters on a permanent.
    PutCounters {
        what: ObjRef,
        kind: CounterKind,
        count: Amount,
    },
    /// A continuous effect from a resolving spell/ability (CR 611.2): its affected object is
    /// locked in when it begins (611.2c).
    Apply {
        what: ObjRef,
        effects: Vec<ContinuousEffect>,
        #[serde(default)]
        until_end_of_turn: bool,
    },
    /// CR 615.7: "Prevent the next N damage that would be dealt to [target] this turn."
    PreventNext {
        to: TargetRef,
        amount: u32,
    },
    /// "Destroy all [selector]" — everything matching as it resolves, at once.
    DestroyAll(Selector),
    /// "[Source] deals N damage to each [selector]" — at once (CR 120.3).
    DamageEach {
        each: Selector,
        amount: Amount,
    },
    /// CR 701.13: exile.
    Exile {
        what: ObjRef,
    },
    /// "Exile [player]'s graveyard": every card in it, at once.
    ExileGraveyard {
        who: PlayerRef,
    },
    /// CR 701.17: mill — put the top N cards of the library into the graveyard.
    Mill {
        who: PlayerRef,
        amount: Amount,
    },
    /// A continuous effect on every permanent matching the selector as this resolves; the
    /// set is locked in then (CR 611.2c): "creatures you control gain … until end of turn".
    ApplyAll {
        affected: Selector,
        effects: Vec<ContinuousEffect>,
        #[serde(default)]
        until_end_of_turn: bool,
    },
    /// "You may [effect]": the controller chooses as this resolves (CR 603.5 for triggers:
    /// the choice is made on resolution, targets are still chosen when it is put on the
    /// stack).
    May(Box<Effect>),
    /// "Return [it] to the battlefield" (a card from your graveyard, under your control).
    ReturnToBattlefield {
        what: ObjRef,
    },
    /// "…where X is [amount]": works X out once, now; later `Amount::X` in the same
    /// resolution uses it (CR 608.2h: determined once, when the effect is applied).
    SetX(Amount),
}

/// An object an effect refers to: a target slot, or the source itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjRef {
    Target(u8),
    /// The ability's source ("this creature").
    This,
    /// The object whose event triggered the ability ("it", "that creature"): the creature
    /// that entered, died or attacked.
    Triggering,
}

/// Which object deals the damage of a `DealDamage`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DamageBy {
    /// The spell, or the ability's source.
    #[default]
    This,
    /// The object whose event triggered the ability ("it deals damage …").
    Triggering,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EachPlayerAction {
    /// CR 701.9: each player discards `n` cards of their choice.
    Discard(u32),
    /// CR 701.21: each player sacrifices `n` creatures of their choice.
    SacrificeCreature(u32),
    /// Each player draws `n` (no choice; performed in APNAP order).
    Draw(u32),
}

/// Refers to target slot `n` of the current mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetRef {
    Target(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerRef {
    /// The controller of the spell or ability.
    You,
    /// The player chosen for target slot `n` (the slot must allow players).
    Target(u8),
    /// "each opponent" (of the controller), all at once.
    EachOpponent,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Amount {
    Fixed(u32),
    /// "the number of [selector]" on the battlefield as this resolves (relative to the
    /// controller and source).
    Count(Selector),
    /// "the total power of [selector]" (negative powers count as 0).
    TotalPower(Selector),
    /// The power / toughness of the object whose event triggered the ability ("its power",
    /// "that creature's toughness"): current, or last known if it has left (CR 608.2h).
    TriggeringPower,
    TriggeringToughness,
    /// The announced value of X (CR 107.3a).
    X,
    /// The source's power: current if it's still on the battlefield, otherwise as it last
    /// existed there (CR 608.2h, 113.7a).
    SourcePower,
}

/// A card file: the definition plus authoring metadata (spec §7.1). Parsed by `card-db`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardScript {
    /// Only verified cards load in strict mode (CLAUDE.md invariant 8).
    pub verified: bool,
    /// Names of tests covering this card; every verified card must name at least one.
    #[serde(default)]
    pub tests: Vec<String>,
    pub card: CardDef,
}

/// The set of card definitions a game uses. Stored once per game (behind an `Arc`) and
/// referenced by [`CardDefId`] from every object, so saves are self-contained without
/// duplicating card data per object (docs/decisions.md D-005).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardCatalog {
    pub defs: Vec<CardDef>,
}

impl CardCatalog {
    pub fn get(&self, id: CardDefId) -> &CardDef {
        &self.defs[id.0 as usize]
    }

    pub fn add(&mut self, def: CardDef) -> CardDefId {
        let id = CardDefId(u32::try_from(self.defs.len()).expect("catalog overflow"));
        self.defs.push(def);
        id
    }

    /// The definition called `name`: by its key if it has one, otherwise by its name.
    pub fn find(&self, name: &str) -> Option<CardDefId> {
        self.defs
            .iter()
            .position(|d| d.key.as_deref().unwrap_or(&d.name) == name)
            .map(|i| CardDefId(i as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mana_cost() {
        let c = ManaCost::parse("{2}{G}{G}").unwrap();
        assert_eq!(
            c.symbols,
            [
                ManaSymbol::Generic(2),
                ManaSymbol::Colored(Color::Green),
                ManaSymbol::Colored(Color::Green)
            ]
        );
        assert_eq!(c.mana_value(), 4);
        assert!(ManaCost::parse("{2}{G").is_err());
        assert!(ManaCost::parse("G").is_err());
    }

    #[test]
    fn mana_cost_serializes_as_text() {
        let c = ManaCost::parse("{X}{1}{C}{R}").unwrap();
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(s, r#""{X}{1}{C}{R}""#);
        assert_eq!(serde_json::from_str::<ManaCost>(&s).unwrap(), c);
        assert!(c.has_x());
        assert_eq!(c.mana_value(), 3);
    }

    #[test]
    fn externally_tagged_effects() {
        let e = Effect::DealDamage {
            to: TargetRef::Target(0),
            amount: Amount::X,
            by: DamageBy::This,
        };
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(
            s,
            r#"{"DealDamage":{"to":{"Target":0},"amount":"X","by":"This"}}"#
        );
    }
}
