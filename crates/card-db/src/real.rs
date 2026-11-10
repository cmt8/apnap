//! Real cards: a script holds only *our encoding of the card's behaviour* (keywords and
//! abilities), keyed by Scryfall oracle id. The printed characteristics — mana cost, type
//! line, power/toughness, colors — come from the Scryfall cache at load time (spec §7.1).
//! A card whose printed data we can't represent exactly (hybrid mana, `*` power, split
//! layouts, …) fails to compile: never approximate a card (CLAUDE.md invariant 8).

use crate::scryfall::ScryfallCard;
use card_model::{
    AbilityDef, BasicLandType, CardDef, CardType, Color, Keyword, ManaCost, Supertype,
};
use serde::{Deserialize, Serialize};

/// A real card's script file (`crates/cards/scripts/<letter>/<slug>.ron`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "RealCard")]
pub struct RealCardScript {
    /// Scryfall oracle id: the join key (names can change; oracle ids don't).
    pub oracle_id: String,
    pub name: String,
    /// Only verified cards load in strict mode.
    pub verified: bool,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(default)]
    pub abilities: Vec<AbilityDef>,
}

fn keyword_name(k: Keyword) -> &'static str {
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
    }
}

/// Supertypes, card types, basic land types, other subtypes.
pub type TypeLine = (
    Vec<Supertype>,
    Vec<CardType>,
    Vec<BasicLandType>,
    Vec<String>,
);

/// Parse a Scryfall type line ("Legendary Creature — Angel Wizard").
pub fn parse_type_line(line: &str) -> Result<TypeLine, String> {
    if line.contains("//") {
        return Err(format!("multi-faced type line {line:?} isn't supported"));
    }
    let (left, right) = match line.split_once('—') {
        Some((l, r)) => (l, r),
        None => (line, ""),
    };
    let mut supertypes = Vec::new();
    let mut card_types = Vec::new();
    for w in left.split_whitespace() {
        match w {
            "Basic" => supertypes.push(Supertype::Basic),
            "Legendary" => supertypes.push(Supertype::Legendary),
            "Ongoing" => supertypes.push(Supertype::Ongoing),
            "Snow" => supertypes.push(Supertype::Snow),
            "World" => supertypes.push(Supertype::World),
            "Artifact" => card_types.push(CardType::Artifact),
            "Battle" => card_types.push(CardType::Battle),
            "Creature" => card_types.push(CardType::Creature),
            "Enchantment" => card_types.push(CardType::Enchantment),
            "Instant" => card_types.push(CardType::Instant),
            "Kindred" => card_types.push(CardType::Kindred),
            "Land" => card_types.push(CardType::Land),
            "Planeswalker" => card_types.push(CardType::Planeswalker),
            "Sorcery" => card_types.push(CardType::Sorcery),
            other => return Err(format!("unknown type {other:?} in {line:?}")),
        }
    }
    let mut land_types = Vec::new();
    let mut subtypes = Vec::new();
    for w in right.split_whitespace() {
        let basic = match w {
            "Plains" => Some(BasicLandType::Plains),
            "Island" => Some(BasicLandType::Island),
            "Swamp" => Some(BasicLandType::Swamp),
            "Mountain" => Some(BasicLandType::Mountain),
            "Forest" => Some(BasicLandType::Forest),
            _ => None,
        };
        match basic {
            Some(b) if card_types.contains(&CardType::Land) => land_types.push(b),
            _ => subtypes.push(w.to_string()),
        }
    }
    Ok((supertypes, card_types, land_types, subtypes))
}

fn parse_color(c: &str) -> Result<Color, String> {
    Ok(match c {
        "W" => Color::White,
        "U" => Color::Blue,
        "B" => Color::Black,
        "R" => Color::Red,
        "G" => Color::Green,
        other => return Err(format!("unknown color {other:?}")),
    })
}

/// Combine a script with its printed data into a card definition. Returns every problem.
pub fn compile(script: &RealCardScript, card: &ScryfallCard) -> Result<CardDef, Vec<String>> {
    let mut p = Vec::new();
    if card.oracle_id.as_deref() != Some(script.oracle_id.as_str()) {
        p.push(format!(
            "oracle id mismatch: script {:?}, Scryfall {:?}",
            script.oracle_id, card.oracle_id
        ));
    }
    if card.name != script.name {
        p.push(format!(
            "name mismatch: script {:?}, Scryfall {:?}",
            script.name, card.name
        ));
    }
    if card.layout != "normal" && card.layout != "saga" {
        p.push(format!("layout {:?} isn't supported", card.layout));
    }
    let (supertypes, card_types, land_types, subtypes) = match parse_type_line(&card.type_line) {
        Ok(t) => t,
        Err(e) => {
            p.push(e);
            Default::default()
        }
    };
    let mana_cost = match card.mana_cost.as_deref() {
        None | Some("") => None,
        Some(text) => match ManaCost::parse(text) {
            Ok(c) => Some(c),
            Err(_) => {
                p.push(format!(
                    "mana cost {text:?} has symbols the engine doesn't support"
                ));
                None
            }
        },
    };
    let mut pt = |v: &Option<String>, what: &str| match v {
        None => None,
        Some(s) => {
            match s.parse::<i32>() {
                Ok(n) => Some(n),
                Err(_) => {
                    p.push(format!("{what} {s:?} isn't a fixed number (needs a characteristic-defining ability)"));
                    None
                }
            }
        }
    };
    let power = pt(&card.power, "power");
    let toughness = pt(&card.toughness, "toughness");
    let mut colors = Vec::new();
    for c in card.colors.iter().flatten() {
        match parse_color(c) {
            Ok(c) => colors.push(c),
            Err(e) => p.push(e),
        }
    }
    colors.sort();
    // Every keyword the script claims must be one Scryfall lists for the card.
    for k in &script.keywords {
        if !card
            .keywords
            .iter()
            .any(|s| s.eq_ignore_ascii_case(keyword_name(*k)))
        {
            p.push(format!(
                "script has keyword {k:?}, which Scryfall doesn't list for this card"
            ));
        }
    }
    if !p.is_empty() {
        return Err(p);
    }
    Ok(CardDef {
        name: script.name.clone(),
        mana_cost,
        supertypes,
        card_types,
        land_types,
        creature_types: subtypes,
        power,
        toughness,
        keywords: script.keywords.clone(),
        abilities: script.abilities.clone(),
        colors: Some(colors),
        token: false,
        key: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(type_line: &str, cost: Option<&str>, pt: Option<(&str, &str)>) -> ScryfallCard {
        ScryfallCard {
            oracle_id: Some("id-1".into()),
            name: "Example".into(),
            layout: "normal".into(),
            mana_cost: cost.map(Into::into),
            type_line: type_line.into(),
            oracle_text: None,
            power: pt.map(|x| x.0.into()),
            toughness: pt.map(|x| x.1.into()),
            colors: Some(vec!["W".into()]),
            color_identity: vec!["W".into()],
            keywords: vec!["Flying".into()],
            id: None,
            image_uris: None,
            artist: None,
            set: None,
            collector_number: None,
        }
    }

    fn script(keywords: Vec<Keyword>) -> RealCardScript {
        RealCardScript {
            oracle_id: "id-1".into(),
            name: "Example".into(),
            verified: true,
            tests: vec!["t".into()],
            keywords,
            abilities: vec![],
        }
    }

    #[test]
    fn compiles_printed_data() {
        let c = card(
            "Legendary Creature — Angel Wizard",
            Some("{2}{W}{W}"),
            Some(("3", "4")),
        );
        let d = compile(&script(vec![Keyword::Flying]), &c).unwrap();
        assert_eq!(d.supertypes, [Supertype::Legendary]);
        assert_eq!(d.card_types, [CardType::Creature]);
        assert_eq!(d.creature_types, ["Angel", "Wizard"]);
        assert_eq!((d.power, d.toughness), (Some(3), Some(4)));
        assert_eq!(d.mana_cost.unwrap().mana_value(), 4);
        assert_eq!(d.colors, Some(vec![Color::White]));
    }

    #[test]
    fn refuses_what_it_cannot_represent() {
        let star = card("Creature — Avatar", Some("{3}{W}"), Some(("*", "*")));
        assert!(
            compile(&script(vec![]), &star)
                .unwrap_err()
                .join(" ")
                .contains("fixed number")
        );
        let hybrid = card("Creature — Cat", Some("{W/B}"), Some(("1", "1")));
        assert!(
            compile(&script(vec![]), &hybrid)
                .unwrap_err()
                .join(" ")
                .contains("mana cost")
        );
        let wrong_kw = card("Creature — Cat", Some("{W}"), Some(("1", "1")));
        assert!(compile(&script(vec![Keyword::Trample]), &wrong_kw).is_err());
        let land = card("Land — Forest Plains", None, None);
        let d = compile(&script(vec![]), &land).unwrap();
        assert_eq!(d.land_types, [BasicLandType::Forest, BasicLandType::Plains]);
    }
}
