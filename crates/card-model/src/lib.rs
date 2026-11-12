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

pub mod chosen;
pub mod describe;

pub use chosen::{CREATURE_TYPES, ChoiceOf, Chosen, CreatureType, OfChosen, is_creature_type};

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

/// Index of a [`CardDef`] inside a [`CardCatalog`]. Dense, assigned in catalog order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardDefId(pub u32);

/// One ability text in a catalog: ability `index` of card `card` — of the abilities
/// printed on it, or (`granted`) of the abilities it gives to other objects
/// ([`CardDef::granted`]). An object can have abilities of other cards (CR 113.2c), so
/// an ability is never named by a bare index into its object's own card.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AbilityId {
    pub card: CardDefId,
    pub index: u16,
    pub granted: bool,
}

impl AbilityId {
    /// Printed ability `index` of `card`.
    pub fn printed(card: CardDefId, index: u16) -> Self {
        AbilityId {
            card,
            index,
            granted: false,
        }
    }
}

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
    /// A two-color hybrid symbol such as `{R/W}` (CR 107.4e): paid with either color. So
    /// far only in the costs of mana abilities (D-066).
    Hybrid(Color, Color),
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
                ManaSymbol::Colored(_) | ManaSymbol::Colorless | ManaSymbol::Hybrid(..) => 1,
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
                ManaSymbol::Hybrid(a, b) => {
                    let letter =
                        |c: &Color| ManaType::from_color(*c).symbol().trim_matches(['{', '}']);
                    let _ = core::fmt::Write::write_fmt(
                        &mut out,
                        format_args!("{{{}/{}}}", letter(a), letter(b)),
                    );
                }
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
            let color = |l: &str| match l {
                "W" => Some(Color::White),
                "U" => Some(Color::Blue),
                "B" => Some(Color::Black),
                "R" => Some(Color::Red),
                "G" => Some(Color::Green),
                _ => None,
            };
            if let Some((a, b)) = sym.split_once('/') {
                match (color(a), color(b)) {
                    (Some(a), Some(b)) if a != b => symbols.push(ManaSymbol::Hybrid(a, b)),
                    _ => return Err(ParseManaCostError),
                }
                continue;
            }
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
    /// Abilities written on this card that it doesn't have itself. Ones it gives to
    /// other objects (or to itself): the quoted text in "enchanted creature has
    /// \"{T}: …\"", referred to by index from [`ContinuousEffect::GrantAbilities`]; the
    /// object that gets one is its source. And ones that work for a while on no object at
    /// all ([`Effect::Lasting`]): "target creature can't be blocked this turn".
    pub granted: Vec<AbilityDef>,
    /// How the card's faces relate (CR 712, 716, 709). [`Layout::Normal`] for a card with
    /// one face.
    #[serde(skip_serializing_if = "Layout::is_normal")]
    pub layout: Layout,
    /// The card's other faces, in order: the back face of a double-faced card. Each is a
    /// whole face of its own (name, cost, types, abilities); this definition is the front
    /// face. In a catalog each face is also an entry of its own ([`CardCatalog::face`]),
    /// so an ability written on a back face is named like any other ([`AbilityId`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<CardDef>,
    /// Set by the catalog on the entry it makes for another face of a card: that card.
    /// Such an entry is never a card of its own (no deck holds it, `find` skips it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face_of: Option<CardDefId>,
}

/// How the faces of a card relate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Layout {
    /// One face.
    #[default]
    Normal,
    /// A modal double-faced card (CR 712.3): either face may be cast, or played as a land
    /// (CR 712.8b, 712.9); it is put onto the stack or battlefield with that face up.
    Modal,
    /// A nonmodal ("transforming") double-faced card (CR 712.2): cast and played with its
    /// front face up; a permanent turns over by transforming (CR 701.27).
    Transform,
    /// An adventurer card (CR 716): face 1 is the Adventure, an instant or sorcery it may
    /// be cast as; as that spell resolves it is exiled, and may then be cast as its
    /// creature from exile (CR 716.3d). Everywhere else only its normal characteristics.
    Adventure,
    /// An omen card (CR 721): like an adventurer, but the Omen spell is shuffled into its
    /// owner's library as it resolves (CR 721.3d).
    Omen,
    /// A split card (CR 709): faces 1 and 2 are its halves, one of which is cast (CR
    /// 709.3); this definition is the two combined, which is what the card is everywhere
    /// but on the stack (CR 709.4): both names, the combined mana cost, every type.
    Split,
}

impl Layout {
    pub fn is_normal(&self) -> bool {
        *self == Layout::Normal
    }

    /// A double-faced card (CR 712.1): its back face is a face of its own, up only on the
    /// stack or battlefield.
    pub fn double_faced(self) -> bool {
        matches!(self, Layout::Modal | Layout::Transform)
    }

    /// The card is cast only as one of its other faces (the halves of a split card, CR
    /// 709.3), never as this definition.
    pub fn cast_by_faces_only(self) -> bool {
        self == Layout::Split
    }

    /// Face 1 is an alternative set of characteristics the card may have as a spell (CR
    /// 716.2, 721.2), not a face of its own on the battlefield.
    pub fn alternative_spell(self) -> bool {
        matches!(self, Layout::Adventure | Layout::Omen)
    }
}

impl CardDef {
    /// Face `face` of this card: 0 is this definition (the front face), 1 the first of
    /// [`CardDef::faces`]. Panics on a face the card doesn't have.
    pub fn face(&self, face: u8) -> &CardDef {
        match face {
            0 => self,
            n => &self.faces[n as usize - 1],
        }
    }

    /// Every face of the card, front first.
    pub fn all_faces(&self) -> impl Iterator<Item = &CardDef> {
        core::iter::once(self).chain(self.faces.iter())
    }

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
            // CR 105.2, 107.4e: a hybrid symbol is both its colors.
            let colors: &[Color] = match s {
                ManaSymbol::Colored(c) => core::slice::from_ref(c),
                ManaSymbol::Hybrid(a, b) => &[*a, *b],
                _ => &[],
            };
            for c in colors {
                if !out.contains(c) {
                    out.push(*c);
                }
            }
        }
        out.sort();
        out
    }

    pub fn has_type(&self, t: CardType) -> bool {
        self.card_types.contains(&t)
    }

    /// The ability `id` names on this card (`id.card` is this card).
    pub fn ability(&self, id: AbilityId) -> &AbilityDef {
        if id.granted {
            &self.granted[id.index as usize]
        } else {
            &self.abilities[id.index as usize]
        }
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
    /// CR 702.3: can't attack.
    Defender,
    /// CR 702.36: can't be blocked except by artifact and/or black creatures.
    Fear,
    /// CR 702.11: can't be the target of spells or abilities its controller's opponents
    /// control.
    Hexproof,
    /// CR 702.12: can't be destroyed.
    Indestructible,
    /// CR 702.13: can't be blocked except by artifact creatures and/or creatures that
    /// share a color with it.
    Intimidate,
    /// CR 702.18: can't be the target of spells or abilities.
    Shroud,
    /// CR 702.14: "[type]walk" — can't be blocked as long as the defending player controls
    /// a land of that type.
    Landwalk(BasicLandType),
    /// CR 702.28b: can't be blocked by creatures without shadow, and can't block creatures
    /// without it.
    Shadow,
    /// CR 702.31b: can't be blocked by creatures without horsemanship.
    Horsemanship,
    /// CR 702.25a: "Whenever this creature becomes blocked by a creature without
    /// flanking, the blocking creature gets -1/-1 until end of turn." The keyword marks
    /// who has it; the trigger is an ability of the card.
    Flanking,
    /// CR 702.73a: "This object is every creature type." A characteristic-defining ability
    /// (CR 604.3): it works in every zone and is applied first in layer 4 (CR 613.3), so an
    /// object that loses it later in the walk (layer 6) is still every creature type.
    Changeling,
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
        /// The zone the card must be in for the ability to be activated (CR 113.6).
        #[serde(default)]
        from: ActivationZone,
        /// A cycling ability (CR 702.29a, 702.29f: typecycling is cycling). Discarding
        /// the card to pay for it is "cycling" it (CR 702.29c).
        #[serde(default)]
        cycling: bool,
    },
    /// A static ability: "[Selector] spells you cast cost {N} less to cast" (generic mana
    /// only, CR 118.7a). The selector is matched against the spell being cast.
    CostReduction {
        spells: Selector,
        amount: u32,
        /// Spells any player casts ("creature spells of the chosen type cost {2} less"),
        /// not only the controller's ("… spells you cast").
        #[serde(default, skip_serializing_if = "core::ops::Not::not")]
        anyone: bool,
    },
    /// "This spell costs {N} less to cast[, where N is …]": functions while it is cast.
    SelfCostReduction(Amount),
    /// An activated mana ability (CR 605.1a): no target, adds mana, resolves at once
    /// (CR 605.3b). Its cost includes `{T}` or a sacrifice (D-066); it may also cost mana
    /// and life.
    Mana {
        cost: Cost,
        produce: ManaProduce,
        /// "Activate only if [condition]" (CR 602.5b).
        #[serde(default)]
        only_if: Option<Condition>,
        /// "Spend this mana only …" (CR 106.6).
        #[serde(default)]
        spend: Option<ManaRestriction>,
        /// What else it does as it resolves, after adding the mana (CR 605.3b): "This
        /// land deals 1 damage to you", "You gain 2 life". Nothing with a target (CR
        /// 605.5a) and nothing that moves a card to or from a library (CR 605.1a).
        #[serde(default)]
        then: Option<Effect>,
    },
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
        /// "… as long as [condition]" / "during your turn, …": the effect exists only
        /// while this holds for the source's controller.
        #[serde(default)]
        only_if: Option<Condition>,
    },
    /// A static ability generating a replacement effect (CR 614).
    Replacement {
        replaces: Replaces,
        with: ReplaceWith,
    },
    /// CR 702.5: "Enchant [filter]" — what an Aura spell targets and what the Aura can
    /// enchant (CR 303.4a).
    Enchant(TargetFilter),
    /// "As an additional cost to cast this spell, [sacrifice a creature / discard a card /
    /// pay N life]" (CR 601.2b, 601.2f). Only the non-mana parts of the cost are used.
    AdditionalCost(Cost),
    /// A static ability restricting how the `affected` creatures attack or block
    /// (CR 508.1c, 509.1b): "can't attack", "can't block", "can't be blocked".
    CombatRule {
        affected: Affected,
        rule: CombatRule,
    },
    /// A static ability that changes a rule of the game for players rather than any
    /// object's characteristics (CR 613.10, 613.11): "you have no maximum hand size", "you
    /// may play an additional land on each of your turns", "you have hexproof", "you may
    /// play lands from your graveyard". `who` is relative to the ability's controller; the
    /// rule holds while `only_if` holds for that controller.
    PlayerRule {
        who: Who,
        rule: PlayerRule,
        #[serde(default)]
        only_if: Option<Condition>,
    },
    /// A static ability that functions only while its card is in `zone` (CR 113.6b): "as
    /// long as this card is in your graveyard [and you control a Mountain], creatures you
    /// control have haste". So far only a [`AbilityDef::Static`] about permanents, from the
    /// graveyard; "you" is the card's owner.
    FromZone {
        zone: ActivationZone,
        ability: Box<AbilityDef>,
    },
    /// An alternative cost of the card's own (CR 118.9): a way to cast it, offered beside
    /// casting it for its mana cost.
    AlternativeCost(AltCost),
    /// CR 603.2d: "If [`cause`] causes a triggered ability of [`sources`] to trigger, that
    /// ability triggers an additional time." Not a replacement effect: it is counted as the
    /// ability triggers, from permanents on the battlefield (or, for an ability that looks
    /// back in time, as they last existed there). `sources` is matched against the source
    /// of the triggered ability, relative to this ability's source and controller; `cause:
    /// None` = whatever made it trigger (D-073).
    AdditionalTrigger {
        sources: ObjFilter,
        cause: Option<TriggerCause>,
    },
    /// CR 101.2, 701.6: "This spell can't be countered" (`spells: None`: an ability of the
    /// spell itself, which works on the stack) or "[`spells`] can't be countered" (an
    /// ability of a permanent; the spell is matched on the stack relative to this
    /// ability's source and controller). Not a replacement effect (CR 614.17): a spell or
    /// ability that would counter it does nothing to it (D-073).
    CantBeCountered { spells: Option<Selector> },
}

/// What made a triggered ability trigger, for [`AbilityDef::AdditionalTrigger`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggerCause {
    /// "[A permanent matching this] entering the battlefield": the ability triggered on its
    /// entering (CR 603.6a), judged as it is on the battlefield.
    Enters(Selector),
    /// "[A creature matching this] dying": the ability triggered on its being put into a
    /// graveyard from the battlefield (CR 700.4), judged as it last existed there.
    Dies(Selector),
    /// "[A creature matching this] attacking": the ability triggered on its being declared
    /// as an attacker, alone or with others (CR 508.1m).
    Attacks(Selector),
}

/// What an [`AbilityDef::PlayerRule`] changes for the players it is about.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerRule {
    /// CR 402.2: "[You have] no maximum hand size".
    NoMaximumHandSize,
    /// "[Your] maximum hand size is N" (CR 402.2). Several of these and the next apply in
    /// timestamp order (CR 613.11).
    MaximumHandSize(u32),
    /// "[Your] maximum hand size is increased (reduced) by N".
    MaximumHandSizeBy(i32),
    /// CR 305.2: "[You] may play N additional lands on each of [your] turns".
    AdditionalLandPlays(u32),
    /// CR 305.2: "[You] may play any number of lands on each of [your] turns".
    AnyNumberOfLandPlays,
    /// CR 702.11c: "[You have] hexproof" — can't be the target of spells or abilities
    /// [your] opponents control.
    Hexproof,
    /// CR 702.18a: "[You have] shroud" — can't be the target of spells or abilities.
    Shroud,
    /// CR 401.5: "Play with the top card of your library revealed": every player sees it.
    TopCardRevealed,
    /// CR 401.5: "You may look at the top card of your library any time."
    LookAtTopCard,
    /// CR 601.3, 116.2a: "[You] may play lands and cast spells from [zone]" — the cards of
    /// the player's own `zone` that match one of `cards` (a land is played, anything else
    /// is cast). When and how is unchanged: a land is still the turn's land play, a spell
    /// is cast at the time and for the cost it always is.
    PlayFrom {
        zone: PlayZone,
        cards: Vec<Selector>,
    },
    /// "You may cast this card from your graveyard [or from exile]": an ability of the
    /// card itself, which functions in those zones (CR 113.6f). Whoever owns the card is
    /// "you".
    PlayThisFrom(Vec<PlayZone>),
}

/// A zone a card may be played or cast from by a [`PlayerRule`] (its owner's, for the
/// zones each player has).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayZone {
    Graveyard,
    /// The top card of the library only.
    LibraryTop,
    Exile,
}

/// What a [`AbilityDef::CombatRule`] forbids its affected creatures.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CombatRule {
    /// "can't attack"
    CantAttack,
    /// "can't block"
    CantBlock,
    /// "can't be blocked" (`by: None`), or "can't be blocked by [creatures matching]". The
    /// selector is relative to the ability's source and controller.
    CantBeBlocked { by: Option<Selector> },
    /// "can't block [attackers matching]" ("can block only creatures with flying" is
    /// "can't block creatures without flying").
    CantBlockAttackers(Selector),
    /// "can't block this creature": the source of the ability, or of the lasting effect
    /// that carries the rule ("target creature can't block this creature this turn").
    CantBlockSource,
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
    /// "attacking" (CR 506.4: from being declared as an attacker until it is removed from
    /// combat or combat ends) / not attacking.
    pub attacking: Option<bool>,
    /// "blocking" / not blocking.
    pub blocking: Option<bool>,
    /// "with power N or less".
    pub power_max: Option<i32>,
    /// "without flying": has none of these keywords.
    pub not_keywords: Vec<Keyword>,
    /// "with mana value N or greater" / "N or less" (CR 202.3; both = "exactly N").
    pub mana_value_min: Option<u32>,
    pub mana_value_max: Option<u32>,
    /// Must have one of these names ("a card named …"; empty = any).
    pub names: Vec<String>,
    /// Must be none of these colors ("nonblack").
    pub not_colors: Vec<Color>,
    /// Must have none of these supertypes ("nonbasic", "nonlegendary").
    pub not_supertypes: Vec<Supertype>,
    /// "with toughness N or less".
    pub toughness_max: Option<i32>,
    /// "A creature you own": who owns the permanent, whoever controls it (CR 108.3).
    /// Not used for cards outside the battlefield, where `controller` is the owner.
    #[serde(default)]
    pub owner: Who,
    /// "That has an Adventure" (CR 716.2a): an adventurer card, whether or not it is
    /// using its Adventure's characteristics now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_adventure: Option<bool>,
    /// `Some(true)`: one of "those cards" — what an earlier instruction of the resolving
    /// spell or ability noted ([`Effect::Note`]); `Some(false)`: not one of them ("destroy
    /// all other creatures" after creating tokens). Only the resolving effect knows them:
    /// card-db allows this only in the selectors of one-shot effects it filters itself.
    #[serde(default)]
    pub noted: Option<bool>,
    /// "Of the chosen type" / "that aren't of the chosen type" / "of the chosen color":
    /// compared with the choice of the ability's source (CR 607.2d) where the selector is
    /// used. A selector with `Type` has no `subtypes` of its own, with `Color` no `colors`
    /// (card-db sees to that).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen: Option<OfChosen>,
}

impl Selector {
    /// Does it ask about power or toughness? Those are final only after the whole layer
    /// walk, so such a selector can't be used inside it (CR 613.4).
    pub fn looks_at_pt(&self) -> bool {
        self.power_min.is_some()
            || self.power_max.is_some()
            || self.toughness_min.is_some()
            || self.toughness_max.is_some()
    }
}

/// What a trigger condition or replacement watches: the source itself, or matching objects.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjFilter {
    This,
    Matching(Selector),
    /// "this creature or another [selector]": the source itself whatever it is, or any
    /// *other* object matching the selector.
    ThisOr(Selector),
    /// "Enchanted creature", "equipped creature": the permanent the source is attached to.
    Host,
    /// One of the objects a lasting effect is about ([`Effect::Lasting`]).
    Remembered,
}

/// Trigger conditions (CR 603.1). M2 vocabulary.
// Built once when the catalog loads, then only read.
#[allow(clippy::large_enum_variant)]
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
        /// "… a noncreature spell", "a Spirit or Arcane spell", "a multicolored spell":
        /// the spell is one of `types` or matches one of these (both empty = any spell).
        /// What the spell is on the stack is compared; who controls it is `by`.
        #[serde(default)]
        spells: Vec<Selector>,
    },
    /// CR 508.1m: "Whenever [filter] attacks" — it is declared as an attacker.
    Attacks(ObjFilter),
    /// One ability with several trigger events: "Whenever this creature enters or attacks".
    Any(Vec<Trigger>),
    /// CR 702.29c: "When you cycle this card" (`this`) — it triggers from wherever the card
    /// went — or "Whenever [who] cycles a card", watched from the battlefield.
    Cycles { by: Who, this: bool },
    /// CR 603.6c: "When/Whenever [filter] leaves the battlefield" — for any zone; looks
    /// back in time (CR 603.10a).
    Leaves(ObjFilter),
    /// Damage being dealt (CR 120): "whenever this creature deals combat damage to a
    /// player", "whenever a creature you control deals damage to an opponent", "whenever
    /// this creature is dealt damage". In the ability, "that player" is the player dealt
    /// damage and "that much" the amount; "it"/"that creature" (`ObjRef::Triggering`) is
    /// the permanent dealt damage, or — when a player was dealt it — the source of the
    /// damage.
    Damage {
        /// What dealt it (`None`: any source).
        by: Option<ObjFilter>,
        to: DamageTo,
        /// Only combat damage (`Some(true)`), only noncombat damage (`Some(false)`), or
        /// either.
        combat: Option<bool>,
        per: DamagePer,
    },
    /// CR 702.83b, 506.5: "Whenever [filter] attacks alone" — it is the only creature
    /// declared as an attacker.
    AttacksAlone(ObjFilter),
    /// "Whenever [who] attacks [with N or more creatures]": once for a declaration of
    /// attackers (CR 508.1) with at least `at_least` attackers that match `with` (any
    /// attacker, if `None`).
    PlayerAttacks {
        by: Who,
        with: Option<Selector>,
        at_least: u32,
    },
    /// "Whenever [who] gains life" (CR 119.9); "that much" is the life gained.
    GainsLife(Who),
    /// "Whenever [who] loses life" — damage included (CR 119.3); "that much" is the loss.
    LosesLife(Who),
    /// "Whenever [who] draws a card" (CR 121.1); `nth`: only their Nth card of the turn
    /// ("whenever you draw your second card each turn").
    Draws { who: Who, nth: Option<u32> },
    /// "Whenever [who] discards a card" (CR 701.9); "it" is the card, where it went.
    Discards(Who),
    /// "Whenever [who] sacrifices [a permanent]" (CR 701.21); looks back (CR 603.10a).
    Sacrifices { who: Who, what: ObjFilter },
    /// CR 509.1h: "Whenever [filter] blocks" — once, however many creatures it blocks.
    Blocks(ObjFilter),
    /// CR 509.1h: "Whenever [filter] becomes blocked" — once, by however many creatures.
    BecomesBlocked(ObjFilter),
    /// "Whenever [filter] attacks and isn't blocked": no blocker was declared for it.
    Unblocked(ObjFilter),
    /// "Whenever [blocker] blocks [attacker]" / "whenever [attacker] becomes blocked by
    /// [blocker]": once for each such pair. "It"/"that creature" is the blocker — or the
    /// attacker, where the blocker is this permanent (or what it is attached to).
    BlockedBy {
        attacker: ObjFilter,
        blocker: ObjFilter,
    },
}

/// What a damage trigger's damage was dealt to.
// Built once when the catalog loads, then only read.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DamageTo {
    /// A player or a permanent.
    Anything,
    Player(Who),
    Permanent(ObjFilter),
    /// "An opponent or a permanent an opponent controls": a player or a permanent (D-073).
    Either {
        player: Who,
        permanent: ObjFilter,
    },
}

/// How damage dealt at the same time counts for a damage trigger (CR 510.2: combat damage
/// is dealt simultaneously; CR 120.9: "that much" is only the damage of the sources asked
/// for).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DamagePer {
    /// Once for each source and each recipient: "whenever this creature deals damage to a
    /// player".
    Each,
    /// Once for each recipient, whatever number of sources: "whenever this creature is
    /// dealt damage", "whenever one or more creatures you control deal combat damage to a
    /// player".
    Recipient,
    /// Once for each source, whatever number of recipients: "whenever this creature deals
    /// damage".
    Source,
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
    /// "At the beginning of combat" (the beginning of combat step, CR 507).
    BeginCombat,
    /// "At end of combat" (as the end of combat step begins, CR 511.2).
    EndCombat,
    /// "At the beginning of [the] precombat main phase" (CR 505.1).
    PrecombatMain,
    /// "At the beginning of [the] postcombat main phase".
    PostcombatMain,
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
    /// "during your turn" / "during turns other than yours".
    YourTurn,
    NotYourTurn,
    /// "as long as there are N or fewer [selector]" on the battlefield ("you control no
    /// untapped lands" is `at_most: 0`).
    ControlAtMost {
        what: Selector,
        at_most: u32,
    },
    /// "as long as you have N or fewer cards in hand" (hellbent is `0`).
    HandAtMost(u32),
    /// "if [something counted] is N or more / N or less / exactly N / not N": "if you
    /// control exactly one creature", "if there are four or more card types among …".
    /// The amount must stand alone (`Amount::stands_alone`); in a static ability it must
    /// be countable inside the layer walk (`Amount::countable_in_layers`).
    Compare {
        amount: Amount,
        cmp: Cmp,
        n: u32,
    },
    /// "If you control a commander" (CR 903.3d: a permanent that is a commander);
    /// `own`: one you own as well ("if you control your commander"). Not inside the layer
    /// walk (card-db refuses it in a static ability's condition).
    ControlCommander {
        own: bool,
    },
    /// "If its [evoke] cost was paid": the permanent the ability is on was the spell cast
    /// with that alternative cost (CR 702.74a). Only `Evoke` so far.
    CastFor(AltKindName),
}

impl Condition {
    /// The selectors of what it counts.
    pub fn selectors_mut(&mut self, f: &mut dyn FnMut(&mut Selector)) {
        match self {
            Condition::Control { what, .. } | Condition::ControlAtMost { what, .. } => f(what),
            Condition::CardsInGraveyard { cards, .. } => f(cards),
            Condition::Compare { amount, .. } => amount.selectors_mut(f),
            Condition::LifeAtMost(_)
            | Condition::LifeAtLeast(_)
            | Condition::YourTurn
            | Condition::NotYourTurn
            | Condition::HandAtMost(_)
            | Condition::ControlCommander { .. }
            | Condition::CastFor(_) => {}
        }
    }
}

/// An [`AltKind`] by name, for conditions about how a spell was cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AltKindName {
    Evoke,
}

/// How a counted amount is compared with a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cmp {
    AtLeast,
    AtMost,
    Exactly,
    Not,
}

impl Cmp {
    pub fn holds(self, value: u32, n: u32) -> bool {
        match self {
            Cmp::AtLeast => value >= n,
            Cmp::AtMost => value <= n,
            Cmp::Exactly => value == n,
            Cmp::Not => value != n,
        }
    }
}

// Built once when the catalog loads, then only read (as `AbilityDef`).
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Affected {
    /// The source permanent itself.
    This,
    /// The object the source is attached to ("enchanted creature", CR 303.4m).
    Enchanted,
    /// Every permanent matching the selector (re-evaluated continuously, CR 611.3a).
    All(Selector),
    /// The objects a lasting effect is about ([`Effect::Lasting`]): "target creature can't
    /// be blocked this turn".
    Remembered,
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
    /// Layer 7a: a characteristic-defining ability (CR 604.3, 613.4a) — "this creature's
    /// power and toughness are each equal to the number of …". Only in a static ability
    /// that affects its own source; it functions in every zone. `None` leaves that
    /// characteristic as printed ("*/4").
    SetPTBy {
        power: Option<Box<Amount>>,
        toughness: Option<Box<Amount>>,
    },
    /// Layer 6: "has [quoted ability]" / "gains [quoted ability]" (CR 113.2c) — the
    /// abilities with these indices in [`CardDef::granted`] of the card this effect is
    /// written on. The affected object is the source of what it gains.
    GrantAbilities(Vec<u16>),
    /// Layer 4: "isn't a [card type]" — the object loses these card types (its subtypes
    /// stay, as a God that isn't a creature is still a God). CR 208.3: a permanent that
    /// isn't a creature has no power or toughness.
    RemoveTypes(Vec<CardType>),
    /// Layer 4: "is the chosen type in addition to its other types" — the creature type
    /// chosen for the effect's source (CR 607.2d). Nothing if none was chosen (CR 607.5a);
    /// only a creature or kindred gains it (CR 205.3d).
    AddChosenType,
}

/// Events a static replacement effect watches for (CR 614). M2 vocabulary.
// Built once when the catalog loads, then only read (as `AbilityDef`).
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// "Each other [selector] enters …" (CR 614.1c): another permanent matching the
    /// selector — as it would exist on the battlefield (CR 614.12), relative to this
    /// ability's source and controller — would enter the battlefield.
    OtherEnters(Selector),
    /// "If [a source] would deal [combat] damage [to …]" (CR 614.1a, 615.1a). `by: None` =
    /// any source; `combat: None` = any damage.
    Damage {
        by: Option<ObjFilter>,
        to: DamageTo,
        combat: Option<bool>,
    },
    /// "If [a creature an opponent controls] would die" (CR 700.4): a permanent matching
    /// the selector — relative to this ability's source and controller, as the permanent
    /// last existed on the battlefield — would be put into a graveyard from the battlefield.
    Dies(Selector),
    /// "If this card would be put into a graveyard from anywhere" (CR 113.6b: it functions
    /// in every zone).
    ThisToGraveyard,
    /// "If [an effect would create] one or more [`token`] tokens [would be created] [under
    /// your control]" (CR 614.16, 111.2): each kind of token of the event is matched as it
    /// would be created — its printed characteristics, controlled by the player creating it
    /// — relative to this ability's source and controller. `effect_only`: "if an effect
    /// would …" (every token in this engine is created by an effect, so it changes only
    /// the text).
    Tokens { token: Selector, effect_only: bool },
    /// "If one or more [`kind`] counters would be put on [a permanent matching `on`]" (CR
    /// 614.16, 122.6: counters a permanent enters with too, matched as it would exist on
    /// the battlefield). `kind: None` = counters of any kind; `effect_only`: "if an effect
    /// would put …" (every counter in this engine is put by an effect or as a permanent
    /// enters, which 614.16 counts; it changes only the text).
    Counters {
        on: Selector,
        kind: Option<CounterKind>,
        effect_only: bool,
    },
    /// "If [you / an opponent / a player] would gain life" (CR 119.10: a gain of 0 is no
    /// event), lifelink's included (CR 120.4c). Relative to this ability's controller.
    GainLife(Who),
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// "enters with a [kind] counter on it for each …": counted as the event is modified
    /// (so not counting the permanent that is entering), relative to the controller of
    /// the ability's source. For the permanent itself, `Amount::X` is the X it was cast
    /// with (CR 107.3m).
    EntersWithCounted { kind: CounterKind, amount: Amount },
    /// "Prevent that damage" (CR 615.1a): all of it.
    Prevent,
    /// "[This permanent] enters tapped unless [condition]" (CR 614.1d). The condition is
    /// judged as the event is modified — on the battlefield as it is before the permanent
    /// enters, so it never counts itself nor anything entering with it — for the player
    /// it will enter under the control of (CR 109.5).
    EntersTappedUnless(Condition),
    /// "As this permanent enters, you may [do something]. If you don't, it enters tapped."
    /// (CR 614.12a: chosen as the event is modified, by the player it will enter under the
    /// control of; nothing uses the stack.)
    EntersTappedUnlessYou(AsEnters),
    /// "If this permanent would enter, you may [do something] instead. If you do, put it
    /// onto the battlefield. If you don't, put it into its owner's graveyard." (CR 614.1a,
    /// 614.12a: chosen as the event is modified, by the player it would enter under the
    /// control of.)
    EntersOnlyIfYou(AsEnters),
    /// "… reveal it and shuffle it into its owner's library instead" (CR 701.20a).
    RevealShuffleIntoLibrary,
    /// "… [twice / that many plus one / half] that many … instead" (CR 614.1a): the number
    /// the event is about — tokens of the kinds the effect applies to, counters of the kind
    /// it applies to, damage, life — changed by `Calc` (D-073). Numbers saturate.
    Scale(Calc),
    /// "… those tokens plus [`n` / that many] [these] tokens are created instead" (CR
    /// 614.1a): `n: None` = as many of each as there are tokens the effect applies to.
    AddTokens { tokens: Vec<String>, n: Option<u32> },
    /// "… that many [these] tokens are created instead" / "instead create one of each":
    /// every token the effect applies to is replaced by one of each of these (D-073).
    ReplaceTokens(Vec<String>),
    /// "As this permanent enters, choose a creature type / a color" (CR 614.1c, 614.12a):
    /// chosen as the event is modified, by the player it will enter under the control of;
    /// the permanent has the choice from the moment it is on the battlefield (CR 607.2d).
    Choose(ChoiceOf),
    /// "You may have this permanent enter [tapped] as a copy of [a permanent matching `of`]
    /// [, except …]" (CR 614.1c, 707.5): chosen as the event is modified (CR 614.12a) by
    /// the player it will enter under the control of, among the permanents on the
    /// battlefield before it enters; `of` is relative to that player. Choosing nothing,
    /// it enters as itself. `tapped`: it enters tapped if it enters as a copy.
    EntersAsCopy {
        of: Selector,
        #[serde(default)]
        except: Vec<CopyExcept>,
        #[serde(default)]
        tapped: bool,
    },
}

/// An exception to the copying process (CR 707.9): it becomes part of the copy's copiable
/// values, so a copy of the copy has it too (CR 707.3, 707.9a–b).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CopyExcept {
    /// "except it isn't legendary" (CR 707.9b).
    NotLegendary,
    /// "except it has [keywords]" (CR 707.9a).
    AddKeywords(Vec<Keyword>),
    /// "except it's a [types] in addition to its other types" (CR 707.9b).
    AddTypes {
        #[serde(default)]
        card_types: Vec<CardType>,
        #[serde(default)]
        subtypes: Vec<String>,
    },
    /// "except it's [P]/[T]" — base power and toughness (CR 707.9b); a
    /// characteristic-defining ability of the original that defines them isn't copied
    /// (CR 707.9d).
    SetPT { power: i32, toughness: i32 },
    /// "except it's [colors]" (CR 707.9b, 707.9d: no color indicator or color-defining
    /// ability of the original is copied). Empty: colorless.
    SetColors(Vec<Color>),
    /// "except its creature types are [subtypes]": the creature types are replaced (its
    /// other subtypes stay).
    SetCreatureTypes(Vec<String>),
    /// "except it has [these abilities]" — "this ability", "'At the beginning of …'"
    /// (CR 707.9a): indices into [`CardDef::granted`] of the card the copy effect is
    /// written on.
    HasAbilities(Vec<u16>),
}

/// What a token copy effect copies.
// Built once when the catalog loads, then only read: the size difference costs nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CopyWhat {
    /// One object: "a copy of target creature", "of this creature", "of equipped
    /// creature", "of that artifact".
    Obj(ObjRef),
    /// "For each [permanent matching], create a token that's a copy of it": every
    /// permanent matching as this resolves (relative to the controller and source).
    Each(Selector),
}

/// What a player may do as a permanent enters (CR 614.12a) so that it enters untapped.
// Built once when the catalog loads, then only read (as `AbilityDef`).
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AsEnters {
    /// "pay N life" (CR 119.4: only a player with at least N life can).
    PayLife(u32),
    /// "reveal a [selector] card from your hand" (CR 701.20; the card stays there).
    Reveal(Selector),
    /// "discard a [selector] card" (CR 701.9).
    Discard(Selector),
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
    /// One "target" word that takes several targets ("two target creatures", "up to two
    /// target creatures"): see [`TargetGroup`].
    #[serde(default)]
    pub groups: Vec<TargetGroup>,
}

/// The slots `first .. first + count` of a mode's targets are one instance of the word
/// "target" (CR 115.3): they have the same filter, must be different objects or players,
/// and at least `min` of them are chosen. A group with `min < count` ("up to …") must be
/// the mode's last slots: slots left empty are simply not there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetGroup {
    pub first: u8,
    pub count: u8,
    pub min: u8,
}

impl Mode {
    /// The group slot `i` belongs to, if any.
    pub fn group_of(&self, i: usize) -> Option<&TargetGroup> {
        self.groups
            .iter()
            .find(|g| (g.first as usize..g.first as usize + g.count as usize).contains(&i))
    }
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
    /// A spell on the stack with these qualities: "target creature spell", "target
    /// noncreature spell", "target instant or sorcery spell". Only the card types,
    /// subtypes and colors of the selector are looked at.
    SpellMatching(Selector),
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
    /// "Sacrifice N [permanents]": ones the player controls, chosen as the cost is paid
    /// (CR 601.2h, 701.21a).
    pub sacrifice: Option<CostChoice>,
    /// "Discard N [cards]" from the player's hand, chosen as the cost is paid.
    pub discard: Option<CostChoice>,
    /// "Pay N life" (CR 119.4: only a player with at least that much life can).
    pub life: u32,
    /// "Discard this card": for an ability activated from the hand (CR 113.6j).
    pub discard_self: bool,
    /// "Exile this card from your graveyard": for an ability activated from the graveyard.
    pub exile_self: bool,
    /// "Exile N [cards] from your hand", chosen as the cost is paid (never the card being
    /// cast: it is on the stack by then).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exile_from_hand: Option<CostChoice>,
    /// "Return N [permanents] you control to their owner's hand", chosen as the cost is paid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_to_hand: Option<CostChoice>,
    /// "Pay X life", X announced as the spell is cast (CR 107.3a): only in a spell's
    /// additional cost (card-db), on top of `life`.
    #[serde(skip_serializing_if = "core::ops::Not::not")]
    pub life_x: bool,
}

impl Cost {
    /// The non-mana parts of `self` and of `other` together: an alternative cost and the
    /// spell's additional cost are paid together (CR 118.9d, 601.2f). `None` when both
    /// choose the same kind of thing (card-db refuses such a card).
    pub fn with(&self, other: &Cost) -> Option<Cost> {
        fn one<T: Clone>(a: &Option<T>, b: &Option<T>) -> Option<Option<T>> {
            match (a, b) {
                (Some(_), Some(_)) => None,
                (a, b) => Some(a.clone().or_else(|| b.clone())),
            }
        }
        Some(Cost {
            mana: self.mana.clone(),
            tap: self.tap || other.tap,
            sacrifice_self: self.sacrifice_self || other.sacrifice_self,
            sacrifice: one(&self.sacrifice, &other.sacrifice)?,
            discard: one(&self.discard, &other.discard)?,
            life: self.life + other.life,
            discard_self: self.discard_self || other.discard_self,
            exile_self: self.exile_self || other.exile_self,
            exile_from_hand: one(&self.exile_from_hand, &other.exile_from_hand)?,
            return_to_hand: one(&self.return_to_hand, &other.return_to_hand)?,
            life_x: self.life_x || other.life_x,
        })
    }
}

/// A cost that may be paid rather than a spell's mana cost (CR 118.9), and what choosing it
/// means for the spell ([`AbilityDef::AlternativeCost`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AltCost {
    pub kind: AltKind,
    /// What is paid rather than the mana cost: `cost.mana` (none: "without paying its mana
    /// cost") and the other parts. The spell's additional costs, cost increases (commander
    /// tax) and reductions apply to it as well (CR 118.9d).
    pub cost: Cost,
    /// "If [condition], you may …": checked for the caster as casting begins.
    #[serde(default)]
    pub only_if: Option<Condition>,
}

/// Which kind of alternative cost an [`AltCost`] is.
// Built once when the catalog loads, then only read (as `AbilityDef`).
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AltKind {
    /// "You may [cost] rather than pay this spell's mana cost" / "you may cast this spell
    /// without paying its mana cost" (CR 118.9): from wherever the card may be cast.
    Rather,
    /// CR 702.34a: "You may cast this card from your graveyard by paying [cost] rather than
    /// paying its mana cost" and "If the flashback cost was paid, exile this card instead
    /// of putting it anywhere else any time it would leave the stack."
    Flashback,
    /// CR 702.74a: "You may cast this card by paying [cost] rather than paying its mana
    /// cost"; the card's "when this enters, if its evoke cost was paid, its controller
    /// sacrifices it" is a triggered ability of its own (`Condition::CastFor`).
    Evoke,
    /// CR 702.96a: "You may choose to pay [cost] rather than pay this spell's mana cost"
    /// and, if you did, the spell's text with "target" changed to "each": `Body` is that
    /// text (no targets, CR 702.96b).
    Overload(Body),
}

/// Where the card with an activated ability must be for the ability to be activated
/// (CR 113.6: the battlefield unless the ability says or implies otherwise).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActivationZone {
    #[default]
    Battlefield,
    /// The owner's hand: cycling (CR 702.29a), "[cost], discard this card: …".
    Hand,
    /// The owner's graveyard: "[cost]: Return this card from your graveyard to your hand".
    Graveyard,
}

/// Things chosen to pay a cost with: `count` of those matching any of `what`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CostChoice {
    pub count: u32,
    /// "an artifact or creature" is two selectors. `other` is relative to the ability's
    /// source ("sacrifice another creature").
    pub what: Vec<Selector>,
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
    /// "Add {W} or {U}": one mana of one of these types.
    OneOf(Vec<ManaType>),
    /// `count` mana of ONE type chosen from `of`: "Add {C}{C}" (`of` = colorless only),
    /// "Add three mana of any one color" (`of` = the five colors).
    Times {
        of: Vec<ManaType>,
        count: u32,
    },
    /// "Add one mana of any color in your commander's color identity" (CR 903.4; with no
    /// commander it adds nothing, CR 903.4f).
    CommanderIdentity,
    /// Exactly this mana, of at least two types: "Add {U}{B}", "Add {C}{C}{R}" (D-066).
    Fixed(Vec<ManaType>),
    /// `count` mana, each of any type in `of` independently: "Add two mana in any
    /// combination of colors", "Add two mana in any combination of {R} and/or {G}".
    Combination {
        of: Vec<ManaType>,
        count: u32,
    },
    /// `amount` mana of ONE type chosen from `of`, the amount counted as the ability
    /// resolves: "Add {G} for each creature you control", "Add X mana of any one color,
    /// where X is the number of enchantments you control".
    Counted {
        of: Vec<ManaType>,
        amount: Amount,
    },
    /// "Add one mana of any color (`types`: any type) that a [`from`] could produce"
    /// (CR 106.7): Exotic Orchard, Reflecting Pool. `from` is relative to the ability's
    /// controller.
    Reflected {
        from: Selector,
        types: bool,
    },
}

/// "Spend this mana only to cast [spells] or activate abilities of [permanents]" (CR 106.6).
/// The mana can pay for a spell matching any of `spells`, or an ability of a permanent
/// matching any of `abilities` or of a source matching any of `sources`; the selectors are
/// relative to the player spending it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ManaRestriction {
    #[serde(default)]
    pub spells: Vec<Selector>,
    #[serde(default)]
    pub abilities: Vec<Selector>,
    /// Abilities whose source matches any of these wherever it is ("activate abilities of
    /// artifact sources", "activate an ability": the empty selector). `abilities` is about
    /// permanents only (D-066).
    #[serde(default)]
    pub sources: Vec<Selector>,
}

impl ManaProduce {
    /// The types the player chooses between (for [`ManaProduce::Fixed`], the types it
    /// makes). For [`ManaProduce::CommanderIdentity`] these are the five colors, and for
    /// [`ManaProduce::Reflected`] all it could ever be: the game narrows them.
    pub fn options(&self) -> Vec<ManaType> {
        match self {
            ManaProduce::One(t) => alloc::vec![*t],
            ManaProduce::AnyColor | ManaProduce::CommanderIdentity => Color::ALL
                .iter()
                .map(|c| ManaType::from_color(*c))
                .collect(),
            ManaProduce::OneOf(of)
            | ManaProduce::Times { of, .. }
            | ManaProduce::Combination { of, .. }
            | ManaProduce::Counted { of, .. } => of.clone(),
            ManaProduce::Fixed(v) => {
                let mut out: Vec<ManaType> = Vec::new();
                for t in v {
                    if !out.contains(t) {
                        out.push(*t);
                    }
                }
                out
            }
            ManaProduce::Reflected { types, .. } => ManaType::ALL
                .iter()
                .copied()
                .filter(|t| *types || *t != ManaType::Colorless)
                .collect(),
        }
    }

    /// How many mana one activation adds, when that is fixed (not for
    /// [`ManaProduce::Counted`], which counts as it resolves).
    pub fn count(&self) -> u32 {
        match self {
            ManaProduce::Times { count, .. } | ManaProduce::Combination { count, .. } => *count,
            ManaProduce::Fixed(v) => v.len() as u32,
            _ => 1,
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
    /// CR 701.26a: tap the permanent (nothing happens if it is already tapped).
    Tap {
        what: ObjRef,
    },
    /// CR 701.26b: untap the permanent.
    Untap {
        what: ObjRef,
    },
    /// "Tap all [selector]."
    TapAll(Selector),
    /// "Untap all [selector]."
    UntapAll(Selector),
    /// CR 701.9: the players each discard that many cards of their choice (all they have,
    /// if fewer). They choose in APNAP order, then discard at once (CR 101.4).
    Discard {
        who: PlayerRef,
        amount: Amount,
    },
    /// The players each discard their hand.
    DiscardHand {
        who: PlayerRef,
    },
    /// CR 701.21: the players each sacrifice that many permanents of their choice among
    /// those they control that match (all of them, if fewer). APNAP order as for discards.
    Sacrifice {
        who: PlayerRef,
        what: Selector,
        amount: Amount,
    },
    /// "Sacrifice this": only if it is still on the battlefield under the control of the
    /// ability's controller (CR 701.21a).
    SacrificeThis,
    /// "Attach this permanent to [it]" (CR 701.3; the body of equip, CR 702.6a).
    AttachThis {
        to: ObjRef,
    },
    /// The players each look through one of their own zones as this resolves and choose
    /// cards there, which are then put somewhere — not targets. From a library this is a
    /// search (CR 701.23): "search your library for up to two basic land cards, put them
    /// onto the battlefield tapped, then shuffle". From a hand: "you may put a creature
    /// card from your hand onto the battlefield". Several players choose in APNAP order,
    /// then every card moves at once (CR 701.23i, 101.4).
    ChooseCards {
        who: PlayerRef,
        from: CardZone,
        /// A card qualifies if it matches any of these. `controller` is the card's owner
        /// outside the battlefield.
        cards: Vec<Selector>,
        count: Amount,
        /// The player must choose `count` cards (all that qualify, if fewer). Otherwise
        /// any number up to `count`: CR 701.23b for a search with a stated quality, "you
        /// may" for the other zones.
        #[serde(default)]
        must: bool,
        to: CardDest,
        /// The chosen cards are revealed (CR 701.20a; 701.23e: only if the effect says so).
        #[serde(default)]
        reveal: bool,
        /// Then the player shuffles their library (CR 701.24). When the cards go on top
        /// of that library, it is shuffled first (CR 701.24b).
        #[serde(default)]
        shuffle: bool,
        /// Each of the players "may search": each decides for themselves, before looking,
        /// whether to (and one who doesn't, doesn't shuffle either).
        #[serde(default)]
        optional: bool,
        /// "… cards that share a land type" (CR 205.3i): every card chosen has a land type
        /// that every other chosen card has too. Chosen one at a time.
        #[serde(default, skip_serializing_if = "is_false")]
        share_land_type: bool,
    },
    /// CR 701.22: scry N.
    Scry {
        who: PlayerRef,
        amount: Amount,
    },
    /// CR 701.25: surveil N.
    Surveil {
        who: PlayerRef,
        amount: Amount,
    },
    /// "Look at the top N cards of your library. Put [some of them] [somewhere] and the
    /// rest [somewhere else]."
    Dig {
        who: PlayerRef,
        count: Amount,
        /// The cards are revealed to everyone rather than looked at by the player.
        #[serde(default)]
        reveal: bool,
        take: Take,
        /// Which of the cards may be taken (empty = any of them).
        #[serde(default)]
        cards: Vec<Selector>,
        /// Where the taken cards go.
        to: CardDest,
        /// The taken cards are revealed ("you may reveal a creature card from among them
        /// and put it into your hand").
        #[serde(default)]
        reveal_taken: bool,
        /// Where the rest go.
        rest: CardDest,
        /// The rest go there in a random order (otherwise the player orders cards that go
        /// to a library, CR 401.4).
        #[serde(default)]
        rest_random: bool,
    },
    /// Put a card or permanent somewhere: "put target creature on top of its owner's
    /// library", "shuffle it into its owner's library" (`shuffle`: the owner then shuffles,
    /// CR 701.24c — even if the object didn't get there).
    PutInto {
        what: ObjRef,
        to: CardDest,
        #[serde(default)]
        shuffle: bool,
        /// Onto the battlefield under this player's control ("return that card to the
        /// battlefield under your control"); `None`: under its owner's.
        #[serde(default)]
        controller: Option<PlayerRef>,
    },
    /// Everything in a zone that matches, at once: "return all creatures to their owners'
    /// hands", "exile all artifacts", "return all creature cards from your graveyard to
    /// the battlefield", "shuffle your graveyard into your library". `whose`: on the
    /// battlefield, permanents those players control; in graveyards, theirs.
    MoveAll {
        from: CardZone,
        whose: PlayerRef,
        cards: Vec<Selector>,
        to: CardDest,
        /// Then those players shuffle their libraries (CR 701.24d: even if nothing moved).
        #[serde(default)]
        shuffle: bool,
    },
    /// A spell or a non-mana ability adds mana (CR 106.4, 605.5): "add {R} for each tapped
    /// land your opponents control", "add {G}{G}". `amount` mana of the one type.
    AddMana {
        who: PlayerRef,
        mana: ManaType,
        amount: Amount,
    },
    /// Several instructions that happen at the same time: "destroy two target creatures"
    /// is one event, not one after the other. Only for instructions that are a single
    /// event or change each (no choices, nothing done in steps).
    Together(Vec<Effect>),
    /// "Put N [kind] counters on each [permanent]" — all at once.
    PutCountersAll {
        what: Selector,
        kind: CounterKind,
        count: Amount,
    },
    /// "Exile [this spell]", "shuffle [this spell] into its owner's library", "return it
    /// to its owner's hand": where the resolving instant or sorcery goes as it finishes
    /// resolving, instead of its owner's graveyard (CR 608.2n).
    ThisSpellTo {
        to: CardDest,
        /// Then its owner shuffles their library.
        #[serde(default)]
        shuffle: bool,
    },
    /// A cost paid while the spell or ability resolves (CR 118.12): "[`unpaid`] unless
    /// [who] pays [cost]", or "[who] may pay [cost]. If they do, [`paid`]". The player is
    /// asked only if they can pay; one of the two instructions is usually empty.
    Pay {
        who: PlayerRef,
        cost: Cost,
        paid: Box<Effect>,
        unpaid: Box<Effect>,
    },
    /// CR 611.2: for a while, abilities work without being on any object — "target
    /// creature can't be blocked this turn", "prevent all combat damage that would be
    /// dealt this turn". `abilities` are indices into [`CardDef::granted`] of this card;
    /// in them "this" is the source of the spell or ability that made the effect, "you"
    /// its controller, and `Remembered` what the effect is `about`.
    Lasting {
        abilities: Vec<u16>,
        /// The object the effect is about; the effect ends when it leaves the battlefield.
        about: Option<ObjRef>,
        /// Ends at cleanup (CR 514.2); otherwise it lasts.
        until_end_of_turn: bool,
        /// Ends when its source leaves the battlefield ("for as long as …").
        while_source: bool,
    },
    /// "[Who] may [effect]": a player other than the controller decides as this resolves
    /// ("that creature's controller may draw a card"). `who` is one player; a player who
    /// has left the game decides nothing, and nothing happens. (With `who` = you this is
    /// [`Effect::May`], which is what the compiler writes then.)
    PlayerMay {
        who: PlayerRef,
        effect: Box<Effect>,
    },
    /// "[first]. If you do, [then]": `then` happens only if `first` did something (CR
    /// 608.2c, 614.6) — "you may exile it. If you do, return another target card…" does
    /// nothing more if the card to exile is already gone.
    IfDone {
        first: Box<Effect>,
        then: Box<Effect>,
    },
    /// A spell or a non-mana ability adds `amount` mana of ONE type its controller chooses
    /// from `of` as it resolves (CR 106.4, 605.5a): "add one mana of any color" (Lotus
    /// Cobra's landfall, which is not a mana ability), "add three mana of any one color".
    AddManaOf {
        who: PlayerRef,
        of: Vec<ManaType>,
        amount: Amount,
    },
    /// CR 701.27a: "transform [it]" — turn a double-faced permanent over so that its
    /// other face is up. It stays the same object (CR 701.27c).
    Transform {
        what: ObjRef,
    },
    /// Perform the instruction, and remember what it did for the rest of this resolution
    /// (CR 608.2c, 400.7j): the cards it moved — as the objects they became — and the
    /// tokens it created are "those cards" ([`ObjRef::Noted`], [`Selector::noted`],
    /// [`Amount::Noted`]). Of a [`Effect::Dig`], only the cards put in its first place.
    /// Forge's "remembered" cards of a chain that forgets them when it ends (D-069); not
    /// the same as what a lasting effect is about (`ObjFilter::Remembered`).
    Note(Box<Effect>),
    /// "If [condition], [instruction]": the condition is checked as this instruction is
    /// reached (CR 608.2c); amounts in it are counted then, and may count "those cards".
    If {
        test: Condition,
        then: Box<Effect>,
    },
    /// "Its controller sacrifices it" of the ability's source, whoever controls it now
    /// (CR 702.74a, evoke; 701.21a): unlike [`Effect::SacrificeThis`] not only by the
    /// ability's controller.
    ControllerSacrificesThis,
    /// "Untap up to N lands": the controller chooses up to `count` tapped permanents
    /// matching `what` (anyone's, unless the selector says whose) as this resolves and
    /// untaps them (CR 701.26b). Not targets.
    UntapChosen {
        what: Selector,
        count: Amount,
    },
    /// CR 701.34a: "Proliferate" — the controller chooses any number of permanents that
    /// have a counter and gives each another counter of each kind already there, all at
    /// once. (Players have no counters in this engine yet: no card that gives them
    /// compiles. When one does, they must be offered too.)
    Proliferate,
    /// CR 603.7: create a delayed triggered ability — "at the beginning of the next
    /// turn's upkeep, draw a card", "at the beginning of your next main phase, add {C}
    /// equal to that spell's mana value". `ability` indexes [`CardDef::granted`]: an
    /// [`AbilityDef::Triggered`] (so far only "at the beginning of [step]"). It triggers
    /// once (CR 603.7c) and is then gone; its controller is this spell or ability's
    /// controller (CR 603.7d) and its source this one's source (CR 603.7e).
    DelayedTrigger {
        ability: u16,
        window: DelayWindow,
        /// "That player" of the delayed ability, fixed now (CR 603.7c: it refers to
        /// what was determined as the delayed ability was created).
        #[serde(default)]
        player: Option<PlayerRef>,
        /// "That much" of the delayed ability, fixed now.
        #[serde(default)]
        amount: Option<Amount>,
    },
    /// "[Who] may draw up to N cards": the player who would draw chooses how many, from 0
    /// to N, as it resolves (CR 107.1c, 608.2d), then draws that many.
    DrawUpTo {
        who: PlayerRef,
        amount: Amount,
    },
    /// CR 701.24a: the players shuffle their libraries.
    Shuffle {
        who: PlayerRef,
    },
    /// "Choose a creature type / a color.": the controller chooses as this resolves (CR
    /// 608.2c); "the chosen type" in the instructions after it is that choice (CR 607.2d).
    Choose(ChoiceOf),
    /// CR 707, 111.12: "create `count` tokens that are copies of [`what`][, except …]"
    /// under the controller's control. A copy of an object that is gone isn't made.
    CopyPermanent {
        what: CopyWhat,
        count: Amount,
        /// Part of the tokens' copiable values (CR 707.9).
        #[serde(default)]
        except: Vec<CopyExcept>,
        /// Not copiable: "that token gains haste" — for as long as it is on the
        /// battlefield (CR 611.2).
        #[serde(default)]
        gains: Vec<Keyword>,
        /// The tokens enter tapped.
        #[serde(default)]
        tapped: bool,
    },
    /// CR 707.4: "[this permanent] becomes a copy of [`what`][, except …]" — for good.
    /// Its noncopy effects stay; nothing enters or leaves.
    BecomeCopy {
        what: ObjRef,
        #[serde(default)]
        except: Vec<CopyExcept>,
    },
}

/// When a delayed triggered ability ([`Effect::DelayedTrigger`]) can trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DelayWindow {
    /// The next time its event happens, whenever that is.
    Next,
    /// Only this turn ("… this turn"): it ends at cleanup (CR 514.2).
    ThisTurn,
    /// Only during the next turn ("at the beginning of the next turn's upkeep").
    NextTurn,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Effect {
    /// Every selector in the instruction — what it acts on, what it counts — for `f` to
    /// look at or change ("of the chosen type" is filled in as a spell resolves).
    pub fn selectors_mut(&mut self, f: &mut dyn FnMut(&mut Selector)) {
        let cost = |c: &mut Cost, f: &mut dyn FnMut(&mut Selector)| {
            for choice in [&mut c.sacrifice, &mut c.discard].into_iter().flatten() {
                choice.what.iter_mut().for_each(&mut *f);
            }
        };
        let parts = |es: &mut Vec<ContinuousEffect>, f: &mut dyn FnMut(&mut Selector)| {
            for e in es {
                e.selectors_mut(f);
            }
        };
        match self {
            Effect::Seq(v) | Effect::Together(v) => v.iter_mut().for_each(|e| e.selectors_mut(f)),
            Effect::May(e) | Effect::PlayerMay { effect: e, .. } | Effect::Note(e) => {
                e.selectors_mut(f)
            }
            Effect::If { test, then } => {
                test.selectors_mut(f);
                then.selectors_mut(f);
            }
            Effect::IfDone { first, then } => {
                first.selectors_mut(f);
                then.selectors_mut(f);
            }
            Effect::Pay {
                cost: c,
                paid,
                unpaid,
                ..
            } => {
                cost(c, f);
                paid.selectors_mut(f);
                unpaid.selectors_mut(f);
            }
            Effect::DealDamage { amount, .. }
            | Effect::Draw { amount, .. }
            | Effect::GainLife { amount, .. }
            | Effect::LoseLife { amount, .. }
            | Effect::DamageEachPlayer { amount }
            | Effect::Mill { amount, .. }
            | Effect::SetX(amount)
            | Effect::Discard { amount, .. }
            | Effect::Scry { amount, .. }
            | Effect::Surveil { amount, .. }
            | Effect::AddMana { amount, .. }
            | Effect::AddManaOf { amount, .. }
            | Effect::DrawUpTo { amount, .. }
            | Effect::CreateToken { count: amount, .. }
            | Effect::PutCounters { count: amount, .. } => amount.selectors_mut(f),
            Effect::DestroyAll(s) | Effect::TapAll(s) | Effect::UntapAll(s) => f(s),
            Effect::DamageEach { each, amount } => {
                f(each);
                amount.selectors_mut(f);
            }
            Effect::Sacrifice { what, amount, .. }
            | Effect::UntapChosen {
                what,
                count: amount,
            }
            | Effect::PutCountersAll {
                what,
                count: amount,
                ..
            } => {
                f(what);
                amount.selectors_mut(f);
            }
            Effect::ChooseCards { cards, count, .. } | Effect::Dig { cards, count, .. } => {
                cards.iter_mut().for_each(&mut *f);
                count.selectors_mut(f);
            }
            Effect::MoveAll { cards, .. } => cards.iter_mut().for_each(&mut *f),
            Effect::CopyPermanent { what, count, .. } => {
                if let CopyWhat::Each(s) = what {
                    f(s);
                }
                count.selectors_mut(f);
            }
            // (The delayed ability's own text is an ability of the card: not here.)
            Effect::DelayedTrigger { amount, .. } => {
                if let Some(a) = amount {
                    a.selectors_mut(f);
                }
            }
            Effect::Apply { effects, .. } => parts(effects, f),
            Effect::ApplyAll {
                affected, effects, ..
            } => {
                f(affected);
                parts(effects, f);
            }
            Effect::Counter { .. }
            | Effect::EachPlayer(_)
            | Effect::Destroy { .. }
            | Effect::ReturnToHand { .. }
            | Effect::PreventNext { .. }
            | Effect::Exile { .. }
            | Effect::ExileGraveyard { .. }
            | Effect::ReturnToBattlefield { .. }
            | Effect::Tap { .. }
            | Effect::Untap { .. }
            | Effect::DiscardHand { .. }
            | Effect::SacrificeThis
            | Effect::AttachThis { .. }
            | Effect::PutInto { .. }
            | Effect::ThisSpellTo { .. }
            | Effect::Transform { .. }
            | Effect::ControllerSacrificesThis
            | Effect::Proliferate
            | Effect::Shuffle { .. }
            | Effect::Lasting { .. }
            | Effect::Choose(_)
            | Effect::BecomeCopy { .. } => {}
        }
    }

    /// Does any selector of the instruction refer to a choice ("of the chosen type")?
    pub fn refers_to_chosen(&self) -> bool {
        let mut found = false;
        self.clone()
            .selectors_mut(&mut |s| found |= s.chosen.is_some());
        found
    }
}

impl ContinuousEffect {
    /// The selectors of the amounts in it ("+1/+1 for each Elf you control").
    pub fn selectors_mut(&mut self, f: &mut dyn FnMut(&mut Selector)) {
        match self {
            ContinuousEffect::ModifyPTBy { power, toughness } => {
                power.amount.selectors_mut(f);
                toughness.amount.selectors_mut(f);
            }
            ContinuousEffect::SetPTBy { power, toughness } => {
                for a in [power, toughness].into_iter().flatten() {
                    a.selectors_mut(f);
                }
            }
            _ => {}
        }
    }
}

impl Amount {
    /// The selectors of what it counts.
    pub fn selectors_mut(&mut self, f: &mut dyn FnMut(&mut Selector)) {
        match self {
            Amount::Count(s) | Amount::TotalPower(s) | Amount::Greatest { of: s, .. } => f(s),
            Amount::CardsIn { cards, .. } | Amount::Noted(cards) => cards.iter_mut().for_each(f),
            Amount::Calc(base, _) => base.selectors_mut(f),
            Amount::IfAtLeast { test, .. } => test.selectors_mut(f),
            Amount::Fixed(_)
            | Amount::TriggeringPower
            | Amount::TriggeringToughness
            | Amount::X
            | Amount::SourcePower
            | Amount::SacrificedPower
            | Amount::SourceToughness
            | Amount::Domain
            | Amount::Devotion(_)
            | Amount::HandSize
            | Amount::LifeTotal
            | Amount::TriggeringAmount
            | Amount::Players(_)
            | Amount::LifeAmong { .. }
            | Amount::OfTarget { .. }
            | Amount::HandOf(_)
            | Amount::NotedTotal(_)
            | Amount::LifeLostThisWay => {}
        }
    }
}

/// A zone an effect takes cards out of as it resolves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CardZone {
    Library,
    Hand,
    Graveyard,
    Battlefield,
}

/// Where an effect puts cards. A card always goes to its owner's hand, library or
/// graveyard (CR 400.3); onto the battlefield it comes under its owner's control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CardDest {
    Hand,
    Battlefield {
        #[serde(default)]
        tapped: bool,
    },
    Graveyard,
    Exile,
    LibraryTop,
    LibraryBottom,
    /// Nowhere: the cards chosen stay where they are, found ("search your library for up
    /// to two basic land cards, reveal those cards, put one onto the battlefield and the
    /// other into your hand"). Only for a search whose cards are noted ([`Effect::Note`])
    /// for the instructions after it.
    Stay,
}

/// How many of the cards a [`Effect::Dig`] looks at are taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Take {
    /// That many (as many as qualify, if fewer).
    Exactly(u32),
    /// "You may": up to that many.
    UpTo(u32),
    /// Every card that qualifies; no choice.
    All,
    /// Any number of those that qualify.
    Any,
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
    /// "This card", where the ability was activated or triggered from outside the
    /// battlefield (a graveyard, a hand): the source for as long as it is still that
    /// object (CR 400.7).
    ThisCard,
    /// "Return it to its owner's hand", of a permanent whose leaving the battlefield
    /// triggered the ability: the card it became in the zone it went to, for as long as it
    /// is still there (CR 400.7).
    TriggeringCard,
    /// "Those cards", "that card", "it": each of the cards an earlier instruction of this
    /// resolution noted ([`Effect::Note`]), while it is still where this resolution put it
    /// — in a public zone (CR 400.7j), or found by a search and not moved.
    Noted,
    /// The permanent this is attached to: "equipped creature", "enchanted creature"
    /// (CR 301.5, 303.4).
    Host,
}

/// Which object deals the damage of a `DealDamage`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DamageBy {
    /// The spell, or the ability's source.
    #[default]
    This,
    /// The object whose event triggered the ability ("it deals damage …").
    Triggering,
    /// The spell or permanent chosen for target slot `n` ("target creature deals damage
    /// to itself equal to its power"): as it is, or as it last existed (CR 608.2h). No
    /// damage if the target was illegal as resolution began (CR 608.2b).
    Target(u8),
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

/// What an effect is done to when that can be an object or a player (damage).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetRef {
    /// Target slot `n` of the current mode.
    Target(u8),
    /// Players that aren't targets: "to you", "to each opponent".
    Players(PlayerRef),
    /// A permanent that isn't a target: "to itself", "to that creature".
    Object(ObjRef),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerRef {
    /// The controller of the spell or ability.
    You,
    /// The player chosen for target slot `n` (the slot must allow players).
    Target(u8),
    /// "each opponent" (of the controller), all at once.
    EachOpponent,
    /// "each player", all at once.
    Each,
    /// "its controller": who controls the object whose event triggered the ability — or
    /// controlled it last, if it has left the battlefield (CR 608.2h).
    TriggeringController,
    /// "That player": the player the triggering event happened to (the player dealt
    /// damage).
    TriggeringPlayer,
    /// "Its controller": the controller of the spell or permanent chosen for target slot
    /// `n` ("counter target spell unless its controller pays {3}"): as it is now, or as
    /// it last existed where it was targeted if it has left (CR 608.2h). Nobody, if the
    /// target was illegal as the spell or ability began to resolve (CR 608.2b).
    ControllerOf(u8),
    /// "Its owner": the owner of the spell or permanent chosen for target slot `n`
    /// (CR 108.3) — not its controller, if someone else controls it.
    OwnerOf(u8),
    /// The players who control "those cards" ([`Effect::Note`]) — as each is now if it is
    /// on the battlefield, otherwise as it last was before it moved (CR 608.2h: "each
    /// player who sacrificed a creature this way").
    NotedControllers,
    /// The players in the game who control none of "those cards" (as for
    /// [`PlayerRef::NotedControllers`]): "each player who didn't sacrifice a creature this
    /// way".
    NotNotedControllers,
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
    /// "the sacrificed creature's power": the total power, as they last existed on the
    /// battlefield, of the permanents sacrificed to pay this ability's cost (CR 608.2h).
    SacrificedPower,
    /// The source's toughness (as [`Amount::SourcePower`]).
    SourceToughness,
    /// "the number of [cards] in [your graveyard / all graveyards / your hand …]". A card
    /// outside the battlefield has its owner where a selector says "you control" (empty
    /// `cards` = every card).
    CardsIn {
        zone: CardZone,
        cards: Vec<Selector>,
    },
    /// An amount, then arithmetic: "twice the number of …", "… plus 1".
    Calc(Box<Amount>, Calc),
    /// "`then` if [test] is `at_least` or more, otherwise `otherwise`".
    IfAtLeast {
        test: Box<Amount>,
        at_least: u32,
        then: u32,
        otherwise: u32,
    },
    /// Domain: the number of basic land types among lands you control.
    Domain,
    /// Your devotion to these colors (CR 700.5): the mana symbols of those colors in the
    /// mana costs of permanents you control.
    Devotion(Vec<Color>),
    /// The greatest power / toughness / mana value among the matching permanents (0 if
    /// there are none).
    Greatest {
        of: Selector,
        what: Measure,
    },
    /// The number of cards in your hand.
    HandSize,
    /// Your life total (0 if it is negative).
    LifeTotal,
    /// "That much", "that many": the amount of the triggering event (the damage dealt).
    TriggeringAmount,
    /// The number of players in the game who are `who` relative to you ("the number of
    /// opponents you have", CR 102.2–3; players who have left the game don't count).
    Players(Who),
    /// The highest or lowest life total among the players in the game who are `who`
    /// relative to you (0 if it is negative, or if there are no such players).
    LifeAmong {
        players: Who,
        highest: bool,
    },
    /// "Its power", "its toughness", "its mana value" of the spell or permanent chosen for
    /// target slot `slot`: as it is now, or as it last existed where it was targeted if it
    /// has left (CR 608.2h) — "exile target creature; its controller gains life equal to
    /// its power". 0 if the target was illegal as resolution began (CR 608.2b).
    OfTarget {
        slot: u8,
        what: Measure,
    },
    /// The number of cards in the hand of one player other than you: "that player's
    /// hand", "target player's hand" (as the effect is applied, CR 608.2h). 0 for a
    /// player who has left the game.
    HandOf(PlayerRef),
    /// The number of "those cards" ([`Effect::Note`]) that match any of the selectors (no
    /// selectors: all of them) — "for each creature destroyed this way", "if it's a
    /// Spirit". Wherever they went, they count; each as it is now if this resolution can
    /// still find it, otherwise as it last was before it moved (CR 608.2h).
    Noted(Vec<Selector>),
    /// The total power, toughness or mana value of "those cards" ("its mana value", "that
    /// creature's power"), each as for [`Amount::Noted`].
    NotedTotal(Measure),
    /// "The life lost this way": the life the players lost to the most recent "lose
    /// life" instruction of this resolution — after replacement effects, so what was
    /// actually lost (CR 119.3, 608.2c). 0 before any.
    LifeLostThisWay,
}

impl Amount {
    /// Can it be worked out from the game alone, without a spell or ability resolving
    /// (no X, no source or triggering object, nothing sacrificed)? Then a replacement
    /// effect can use it.
    pub fn stands_alone(&self) -> bool {
        match self {
            Amount::Fixed(_)
            | Amount::Count(_)
            | Amount::TotalPower(_)
            | Amount::CardsIn { .. }
            | Amount::Domain
            | Amount::Devotion(_)
            | Amount::Greatest { .. }
            | Amount::HandSize
            | Amount::LifeTotal
            | Amount::Players(_)
            | Amount::LifeAmong { .. } => true,
            Amount::Calc(base, _) => base.stands_alone(),
            Amount::IfAtLeast { test, .. } => test.stands_alone(),
            Amount::X
            | Amount::SourcePower
            | Amount::SourceToughness
            | Amount::TriggeringPower
            | Amount::TriggeringToughness
            | Amount::TriggeringAmount
            | Amount::SacrificedPower
            | Amount::OfTarget { .. }
            | Amount::HandOf(_)
            | Amount::Noted(_)
            | Amount::NotedTotal(_)
            | Amount::LifeLostThisWay => false,
        }
    }

    /// Can a static ability's "+N/+N for each …" use it? It is re-counted inside the layer
    /// walk (CR 613.4c), so it must not depend on what the walk is still working out:
    /// permanents counted without looking at power or toughness, cards in other zones,
    /// hand sizes and life totals, and arithmetic on those.
    pub fn countable_in_layers(&self) -> bool {
        match self {
            Amount::Fixed(_)
            | Amount::HandSize
            | Amount::LifeTotal
            | Amount::Players(_)
            | Amount::LifeAmong { .. }
            | Amount::Domain
            | Amount::Devotion(_) => true,
            Amount::Count(s) => !s.looks_at_pt(),
            Amount::CardsIn { zone, cards } => {
                *zone != CardZone::Battlefield && cards.iter().all(|s| !s.looks_at_pt())
            }
            Amount::Calc(base, _) => base.countable_in_layers(),
            Amount::IfAtLeast { test, .. } => test.countable_in_layers(),
            _ => false,
        }
    }
}

/// Arithmetic on an [`Amount`]. Results never go below 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Calc {
    Plus(u32),
    Minus(u32),
    /// `n` minus the amount.
    From(u32),
    Times(u32),
    HalfUp,
    HalfDown,
    /// No more than.
    AtMost(u32),
    /// No less than.
    AtLeast(u32),
}

impl Calc {
    pub fn apply(self, v: u32) -> u32 {
        match self {
            Calc::Plus(n) => v.saturating_add(n),
            Calc::Minus(n) => v.saturating_sub(n),
            Calc::From(n) => n.saturating_sub(v),
            Calc::Times(n) => v.saturating_mul(n),
            Calc::HalfUp => v.div_ceil(2),
            Calc::HalfDown => v / 2,
            Calc::AtMost(n) => v.min(n),
            Calc::AtLeast(n) => v.max(n),
        }
    }
}

/// What of a permanent is measured ([`Amount::Greatest`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Measure {
    Power,
    Toughness,
    ManaValue,
}

/// CR 903.4: a card's color identity — the colors of mana symbols in its mana cost or rules
/// text, plus its color indicator. (Basic land types are handled by CR 903.5d.)
pub fn color_identity(def: &CardDef) -> Vec<Color> {
    let mut out: Vec<Color> = def.colors.clone().unwrap_or_default();
    let add_cost = |m: &ManaCost, out: &mut Vec<Color>| {
        for s in &m.symbols {
            match s {
                ManaSymbol::Colored(c) => out.push(*c),
                // CR 903.4: a hybrid symbol counts as both its colors.
                ManaSymbol::Hybrid(a, b) => out.extend([*a, *b]),
                _ => {}
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
                // CR 903.4: the mana symbols in the rules text. "{T}: Add {W} or {U}" has
                // two; "one mana of any color" and "of any one color" have none.
                if let AbilityDef::Mana { produce, .. } = a {
                    let named: Vec<ManaType> = match produce {
                        ManaProduce::One(t) => alloc::vec![*t],
                        ManaProduce::AnyColor | ManaProduce::CommanderIdentity => Vec::new(),
                        ManaProduce::OneOf(of) => of.clone(),
                        ManaProduce::Times { of, .. }
                        | ManaProduce::Combination { of, .. }
                        | ManaProduce::Counted { of, .. }
                            if of.len() < 5 =>
                        {
                            of.clone()
                        }
                        ManaProduce::Times { .. }
                        | ManaProduce::Combination { .. }
                        | ManaProduce::Counted { .. }
                        | ManaProduce::Reflected { .. } => Vec::new(),
                        ManaProduce::Fixed(v) => v.clone(),
                    };
                    for t in named {
                        if let Some(c) = Color::ALL
                            .into_iter()
                            .find(|c| ManaType::from_color(*c) == t)
                        {
                            out.push(c);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // CR 903.4d: the back face of a double-faced card counts too.
    for f in &def.faces {
        out.extend(color_identity(f));
    }
    out.sort();
    out.dedup();
    out
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

    pub fn ability(&self, id: AbilityId) -> &AbilityDef {
        self.get(id.card).ability(id)
    }

    /// Add a card. Its other faces get entries of their own right after it, so face `n`
    /// of the card at `id` is at `id + n` ([`CardCatalog::face`]).
    pub fn add(&mut self, def: CardDef) -> CardDefId {
        let id = CardDefId(u32::try_from(self.defs.len()).expect("catalog overflow"));
        let faces: Vec<CardDef> = def
            .faces
            .iter()
            .map(|f| CardDef {
                face_of: Some(id),
                layout: def.layout,
                ..f.clone()
            })
            .collect();
        self.defs.push(def);
        self.defs.extend(faces);
        id
    }

    /// The entry of face `face` of the card `card` (0: the card itself, its front face).
    pub fn face(&self, card: CardDefId, face: u8) -> CardDefId {
        debug_assert!(
            face as usize <= self.get(card).faces.len(),
            "no face {face}"
        );
        CardDefId(card.0 + u32::from(face))
    }

    /// The card a catalog entry is a face of (itself for a front face).
    pub fn card_of(&self, entry: CardDefId) -> CardDefId {
        self.get(entry).face_of.unwrap_or(entry)
    }

    /// The definition called `name`: by its key if it has one, otherwise by its name. The
    /// entries of back faces are not cards of their own and are never found.
    pub fn find(&self, name: &str) -> Option<CardDefId> {
        self.defs
            .iter()
            .position(|d| d.face_of.is_none() && d.key.as_deref().unwrap_or(&d.name) == name)
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
