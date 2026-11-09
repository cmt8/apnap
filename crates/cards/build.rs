//! Embeds every `.ron` file under `data/` (invented, self-contained test cards and basic
//! lands), `scripts/` (real cards: behaviour only, keyed by Scryfall oracle id) and `decks/`
//! into the crate, in sorted path order (stable `CardDefId`s). No hand-maintained list.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "ron") {
            out.push(p);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut code = String::new();
    for (konst, dir) in [
        ("CARDS", "data"),
        ("SCRIPTS", "scripts"),
        ("DECKS", "decks"),
    ] {
        println!("cargo:rerun-if-changed={dir}");
        let mut files = Vec::new();
        walk(&root.join(dir), &mut files);
        let mut rel: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        rel.sort();
        writeln!(
            code,
            "/// `(repo path, contents)` of every file under `{dir}/`, sorted by path."
        )
        .unwrap();
        writeln!(code, "pub const {konst}: &[(&str, &str)] = &[").unwrap();
        for r in rel {
            writeln!(
                code,
                "    (\"crates/cards/{r}\", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{r}\"))),"
            )
            .unwrap();
        }
        writeln!(code, "];").unwrap();
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded.rs");
    std::fs::write(out, code).unwrap();
}
