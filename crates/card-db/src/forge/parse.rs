//! A lossless reader for Forge card scripts (docs/forge/script-format.md). It only splits
//! the text into its parts; what the parts mean is `translate`'s business.

use std::collections::BTreeMap;

/// A whole script file: one face, or several for split / transforming / adventure cards.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Script {
    pub faces: Vec<Face>,
    /// `AlternateMode:` — how the faces relate (DoubleFaced, Split, Adventure, Flip, …).
    pub alternate_mode: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Face {
    pub name: String,
    /// The `ManaCost:` line as written ("1 W W", "no cost").
    pub mana_cost: Option<String>,
    /// The words of the `Types:` line.
    pub types: Vec<String>,
    /// `PT:` as (power, toughness) text.
    pub pt: Option<(String, String)>,
    pub loyalty: Option<String>,
    pub defense: Option<String>,
    pub colors: Option<String>,
    /// `K:` lines, without the prefix.
    pub keywords: Vec<String>,
    /// `A:` lines (spell and activated abilities).
    pub abilities: Vec<Record>,
    /// `T:` lines.
    pub triggers: Vec<Record>,
    /// `S:` lines.
    pub statics: Vec<Record>,
    /// `R:` lines.
    pub replacements: Vec<Record>,
    /// `SVar:Name:Value`.
    pub svars: BTreeMap<String, String>,
    pub oracle: Option<String>,
    pub text: Option<String>,
    /// Lines that never affect the rules (deck-building and AI hints).
    pub hints: Vec<String>,
    /// Comment lines (`# …`), without the `#`, in the order written. Forge's reader skips
    /// a line whose first character is `#` (CardRules.Reader.readCard); the compiler
    /// reviews the text (`comments.rs`) before ignoring it.
    pub comments: Vec<String>,
    /// Lines this reader doesn't know. A card with any of these can't be loaded.
    pub unknown: Vec<String>,
    /// The kinds (`A`, `T`, `S`, `R`) of the ability lines in the order they were written,
    /// so a card's abilities can keep their printed order.
    pub order: Vec<char>,
}

/// `Key$ Value | Key$ Value | …`, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub params: Vec<(String, String)>,
}

impl Record {
    pub fn parse(text: &str) -> Record {
        let params = text
            .split('|')
            .filter_map(|part| {
                let part = part.trim();
                let (k, v) = part.split_once('$')?;
                Some((k.trim().to_string(), v.trim().to_string()))
            })
            .collect();
        Record { params }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// `SP$ DealDamage` → ("SP", "DealDamage"): the kind of ability and its effect API.
    pub fn head(&self) -> Option<(&str, &str)> {
        let (k, v) = self.params.first()?;
        matches!(k.as_str(), "SP" | "AB" | "DB" | "ST").then_some((k.as_str(), v.as_str()))
    }
}

const HINTS: [&str; 4] = ["DeckHints", "DeckHas", "DeckNeeds", "AI"];

pub fn parse(text: &str) -> Result<Script, String> {
    let mut script = Script::default();
    let mut face = Face::default();
    let mut any = false;
    for raw in text.lines() {
        // Forge: `line.isEmpty() || line.charAt(0) == '#'` on the raw line. Only a `#` in
        // column 0 starts a comment; a `#` anywhere else is part of a value. (Indented
        // `#` lines stay unknown lines: Forge would parse them as a key.)
        if let Some(comment) = raw.strip_prefix('#') {
            face.comments
                .push(comment.split_whitespace().collect::<Vec<_>>().join(" "));
            continue;
        }
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line == "ALTERNATE" {
            script.faces.push(std::mem::take(&mut face));
            continue;
        }
        any = true;
        let Some((key, value)) = line.split_once(':') else {
            face.unknown.push(line.to_string());
            continue;
        };
        let value = value.trim();
        match key {
            "Name" => face.name = value.to_string(),
            "ManaCost" => face.mana_cost = Some(value.to_string()),
            "Types" => face.types = value.split_whitespace().map(str::to_string).collect(),
            "PT" => match value.split_once('/') {
                Some((p, t)) => face.pt = Some((p.trim().to_string(), t.trim().to_string())),
                None => face.unknown.push(line.to_string()),
            },
            "Loyalty" => face.loyalty = Some(value.to_string()),
            "Defense" => face.defense = Some(value.to_string()),
            "Colors" => face.colors = Some(value.to_string()),
            "K" => face.keywords.push(value.to_string()),
            "A" => {
                face.abilities.push(Record::parse(value));
                face.order.push('A');
            }
            "T" => {
                face.triggers.push(Record::parse(value));
                face.order.push('T');
            }
            "S" => {
                face.statics.push(Record::parse(value));
                face.order.push('S');
            }
            "R" => {
                face.replacements.push(Record::parse(value));
                face.order.push('R');
            }
            "SVar" => match value.split_once(':') {
                Some((name, v)) => {
                    face.svars
                        .insert(name.trim().to_string(), v.trim().to_string());
                }
                None => face.unknown.push(line.to_string()),
            },
            "Oracle" => face.oracle = Some(value.to_string()),
            "Text" => face.text = Some(value.to_string()),
            "AlternateMode" => script.alternate_mode = Some(value.to_string()),
            k if HINTS.contains(&k) => face.hints.push(line.to_string()),
            _ => face.unknown.push(line.to_string()),
        }
    }
    if !any {
        return Err("empty script".into());
    }
    script.faces.push(face);
    if script.faces[0].name.is_empty() {
        return Err("no Name: line".into());
    }
    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;

    // An invented card in Forge's format.
    const SAMPLE: &str = "Name:Test Emberling\nManaCost:1 R\nTypes:Legendary Creature Elemental Shaman\nPT:2/1\nK:Haste\nK:Ward:2\nT:Mode$ ChangesZone | Origin$ Any | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigDmg | TriggerDescription$ When CARDNAME enters, it deals 1 damage to any target.\nSVar:TrigDmg:DB$ DealDamage | ValidTgts$ Any | NumDmg$ 1\nA:AB$ Pump | Cost$ R T | NumAtt$ +1| SpellDescription$ CARDNAME gets +1/+0 until end of turn.\nS:Mode$ Continuous | Affected$ Creature.YouCtrl | AddKeyword$ Haste\nR:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplaceWith$ ETBTapped\nSVar:X:Count$Valid Creature.YouCtrl\nDeckHints:Type$Elemental\nAI:RemoveDeck:All\nOracle:Haste\\nWard {2}";

    #[test]
    fn a_script_is_split_into_its_parts() {
        let s = parse(SAMPLE).unwrap();
        assert_eq!(s.faces.len(), 1);
        let f = &s.faces[0];
        assert_eq!(f.name, "Test Emberling");
        assert_eq!(f.mana_cost.as_deref(), Some("1 R"));
        assert_eq!(f.types, ["Legendary", "Creature", "Elemental", "Shaman"]);
        assert_eq!(f.pt, Some(("2".into(), "1".into())));
        assert_eq!(f.keywords, ["Haste", "Ward:2"]);
        assert_eq!(f.triggers.len(), 1);
        assert_eq!(f.triggers[0].get("Mode"), Some("ChangesZone"));
        assert_eq!(f.triggers[0].get("Execute"), Some("TrigDmg"));
        assert_eq!(f.triggers[0].head(), None);
        let a = &f.abilities[0];
        assert_eq!(a.head(), Some(("AB", "Pump")));
        // No space before the bar: still split.
        assert_eq!(a.get("NumAtt"), Some("+1"));
        assert_eq!(a.get("Cost"), Some("R T"));
        assert_eq!(f.statics[0].get("Affected"), Some("Creature.YouCtrl"));
        assert_eq!(f.replacements[0].get("Event"), Some("Moved"));
        // An SVar's value may itself contain colons and dollars.
        assert_eq!(
            f.svars["TrigDmg"],
            "DB$ DealDamage | ValidTgts$ Any | NumDmg$ 1"
        );
        assert_eq!(f.svars["X"], "Count$Valid Creature.YouCtrl");
        assert_eq!(f.order, ['T', 'A', 'S', 'R']);
        assert_eq!(f.hints.len(), 2);
        assert!(f.unknown.is_empty());
        assert_eq!(f.oracle.as_deref(), Some("Haste\\nWard {2}"));
    }

    #[test]
    fn faces_and_unknown_lines() {
        let s = parse("Name:Test Front\nTypes:Creature Elf\nAlternateMode:DoubleFaced\nMystery:1\nALTERNATE\nName:Test Back\nTypes:Creature Elf Werewolf").unwrap();
        assert_eq!(s.alternate_mode.as_deref(), Some("DoubleFaced"));
        assert_eq!(s.faces.len(), 2);
        assert_eq!(s.faces[1].name, "Test Back");
        assert_eq!(s.faces[0].unknown, ["Mystery:1"]);
        assert!(parse("").is_err());
        // A comment line is kept as a comment, not an unknown line; `#` inside a value stays.
        let s = parse("# top\nName:Test C\nTypes:Creature Elf\n#  spaced   out\nSVar:X:Count$A # not a comment").unwrap();
        assert_eq!(s.faces[0].comments, ["top", "spaced out"]);
        assert!(s.faces[0].unknown.is_empty());
        assert_eq!(s.faces[0].svars["X"], "Count$A # not a comment");
        assert!(parse("# only a comment").is_err());
        assert!(parse("Types:Creature").is_err());
    }
}
