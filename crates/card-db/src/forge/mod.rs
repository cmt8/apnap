//! Forge card scripts (github.com/Card-Forge/forge, GPL v3) as a source of card behaviour
//! (decisions.md D-046). `parse` reads a script losslessly; `translate` compiles it into a
//! `CardDef`, refusing any card that has a line or parameter it doesn't understand. The
//! scripts themselves are fetched into the user's cache, never committed.

pub mod library;
pub mod parse;
pub mod translate;

pub use library::{Library, printed_mismatch};

pub use parse::{Face, Record, Script, parse};
pub use translate::{Blockers, Compiled, Known, compile};

use std::path::{Path, PathBuf};

/// The Forge commit the scripts are read from. Pinned so that everyone compiles the same
/// cards; bump it deliberately (card behaviour changes with it).
pub const FORGE_COMMIT: &str = "3d5c95bcaa592f6df72538b63d5d2d890e46dbfd";
pub const FORGE_REPO: &str = "https://github.com/Card-Forge/forge.git";

/// How to get the scripts, shown whenever they are missing.
pub const FETCH_HINT: &str = "Forge's card scripts are not in the local cache; run `cargo run -p harness -- fetch-forge` (needs git and network)";

/// The sparse clone of Forge in the cache directory.
pub fn repo_dir() -> PathBuf {
    crate::scryfall::cache_dir().join("forge").join("repo")
}

pub fn cards_dir() -> PathBuf {
    repo_dir().join("forge-gui").join("res").join("cardsfolder")
}

pub fn tokens_dir() -> PathBuf {
    repo_dir()
        .join("forge-gui")
        .join("res")
        .join("tokenscripts")
}

/// Every script under `dir` as (file stem, text), sorted by file stem.
pub fn read_scripts(dir: &Path) -> std::io::Result<Vec<(String, String)>> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else if path.extension().is_some_and(|x| x == "txt") {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                out.push((stem, std::fs::read_to_string(&path)?));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// Clone (or update) the pinned Forge commit into the cache: only the card and token
/// scripts are checked out. Returns the number of card scripts present.
pub fn fetch() -> Result<usize, String> {
    let dir = repo_dir();
    let git = |args: &[&str], cwd: &Path| -> Result<String, String> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("couldn't run git: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    };
    if !dir.join(".git").exists() {
        let parent = dir.parent().expect("has parent");
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        git(
            &[
                "clone",
                "--quiet",
                "--no-checkout",
                "--depth",
                "1",
                "--filter=blob:none",
                "--sparse",
                FORGE_REPO,
                "repo",
            ],
            parent,
        )?;
    }
    git(
        &[
            "sparse-checkout",
            "add",
            "forge-gui/res/cardsfolder",
            "forge-gui/res/tokenscripts",
        ],
        &dir,
    )?;
    if git(&["rev-parse", "HEAD"], &dir).ok().as_deref() != Some(FORGE_COMMIT) {
        git(
            &["fetch", "--quiet", "--depth", "1", "origin", FORGE_COMMIT],
            &dir,
        )?;
    }
    git(&["checkout", "--quiet", FORGE_COMMIT], &dir)?;
    read_scripts(&cards_dir())
        .map(|s| s.len())
        .map_err(|e| e.to_string())
}
