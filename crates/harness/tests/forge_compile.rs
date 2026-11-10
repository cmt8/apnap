//! The Forge-script compiler (decisions.md D-046) checked against the cards that were
//! scripted and tested by hand: for each of them, compiling Forge's script must give the
//! same card. Needs the Scryfall cache and the Forge scripts
//! (`cargo run -p harness -- fetch-cards` and `-- fetch-forge`).

use card_model::{AbilityDef, CardDef};
use std::collections::BTreeMap;

/// A definition with the things that may legitimately differ made equal: keyword order,
/// how colours are stated, the catalog key.
fn normal(def: &CardDef) -> CardDef {
    let mut d = def.clone();
    d.keywords.sort();
    let mut colors = d.printed_colors();
    colors.sort();
    d.colors = Some(colors);
    d.key = None;
    d
}

fn abilities(def: &CardDef) -> Vec<String> {
    let mut v: Vec<String> = def
        .abilities
        .iter()
        .map(|a: &AbilityDef| format!("{a:?}"))
        .collect();
    v.sort();
    v
}

/// Cards whose hand-written script and Forge's script are both right but not identical,
/// each with the reason (checked by reading both).
const SAME_MEANING: &[(&str, &str)] = &[(
    "Regal Imperiosaur",
    "\"Other Dinosaurs you control get +1/+1\": Forge selects Dinosaur *creatures*, the hand \
     script any Dinosaur permanent. Only creatures have power and toughness, and both \
     selections are re-evaluated continuously, so they never differ in effect.",
)];

#[test]
fn compiling_forges_script_gives_the_hand_written_card() {
    let scripts = card_db::forge::read_scripts(&card_db::forge::cards_dir())
        .unwrap_or_else(|_| panic!("{}", card_db::forge::FETCH_HINT));
    assert!(!scripts.is_empty(), "{}", card_db::forge::FETCH_HINT);
    let mut by_name: BTreeMap<String, &str> = BTreeMap::new();
    for (_, text) in &scripts {
        if let Some(name) = text.lines().next().and_then(|l| l.strip_prefix("Name:")) {
            by_name.insert(name.trim().to_string(), text);
        }
    }
    let known = card_db::forge::Known::from_scripts(scripts.iter().map(|(_, t)| t.as_str()));
    let catalog = harness::full_catalog();
    let mut checked = 0;
    let mut problems: Vec<String> = Vec::new();
    for &(path, text) in cards::SCRIPTS {
        let hand = card_db::parse_real(card_db::Source { path, text }).unwrap();
        let Some(id) = catalog.find(&hand.name) else {
            continue;
        };
        let ours = catalog.get(id);
        let Some(forge_text) = by_name.get(&hand.name) else {
            problems.push(format!("{}: Forge has no script", hand.name));
            continue;
        };
        let script = card_db::forge::parse(forge_text).unwrap();
        match card_db::forge::compile(&script, &known) {
            Err(blockers) => problems.push(format!("{}: doesn't compile: {blockers:?}", hand.name)),
            Ok(compiled) => {
                checked += 1;
                let (a, b) = (normal(ours), normal(&compiled.def));
                let same = CardDef {
                    abilities: Vec::new(),
                    ..a.clone()
                } == CardDef {
                    abilities: Vec::new(),
                    ..b.clone()
                } && abilities(&a) == abilities(&b);
                if !same && !SAME_MEANING.iter().any(|(n, _)| *n == hand.name) {
                    problems.push(format!(
                        "{}:\n   hand:  {:?}\n   forge: {:?}",
                        hand.name,
                        (
                            &a.mana_cost,
                            &a.supertypes,
                            &a.card_types,
                            &a.creature_types,
                            a.power,
                            a.toughness,
                            &a.keywords,
                            abilities(&a)
                        ),
                        (
                            &b.mana_cost,
                            &b.supertypes,
                            &b.card_types,
                            &b.creature_types,
                            b.power,
                            b.toughness,
                            &b.keywords,
                            abilities(&b)
                        ),
                    ));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} of {} cards differ:\n{}",
        problems.len(),
        checked + problems.len(),
        problems.join("\n")
    );
    assert!(checked >= 88);
}
