//! `harness bundle-data`: copy what a game needs out of the local cache into a portable
//! data folder (D-095), so a copy of the program can run on a machine that has never
//! fetched anything: Forge's card and token scripts (GPL v3, with their licence), Scryfall's
//! Oracle card file, and the printed data of the precon cards. Card images and mana symbols
//! are copied only when asked: they are not ours to license, and the app downloads them.
//!
//! The folder has the cache's own layout, so the program reads it through
//! `card_db::scryfall::cache_dir` like any cache.

use std::path::{Path, PathBuf};

/// What was copied.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Copied {
    pub files: u64,
    pub bytes: u64,
}

/// One thing the bundle needs: where it is under the cache, and the command that fetches
/// it when it is missing.
struct Part {
    path: &'static [&'static str],
    fetch: &'static str,
    /// Missing is an error (otherwise it is skipped).
    required: bool,
}

const PARTS: &[Part] = &[
    Part {
        path: &["forge", "repo", "forge-gui", "res", "cardsfolder"],
        fetch: "fetch-forge",
        required: true,
    },
    Part {
        path: &["forge", "repo", "forge-gui", "res", "tokenscripts"],
        fetch: "fetch-forge",
        required: true,
    },
    // Forge's licence travels with its scripts.
    Part {
        path: &["forge", "repo", "LICENSE"],
        fetch: "fetch-forge",
        required: true,
    },
    Part {
        path: &["bulk", "oracle-cards.jsonl.gz"],
        fetch: "fetch-bulk",
        required: true,
    },
    Part {
        path: &["scryfall", "cards.json"],
        fetch: "fetch-cards",
        required: true,
    },
];

const IMAGE_PARTS: &[Part] = &[
    Part {
        path: &["scryfall", "images"],
        fetch: "",
        required: false,
    },
    Part {
        path: &["scryfall", "symbols"],
        fetch: "",
        required: false,
    },
];

fn join(root: &Path, parts: &[&str]) -> PathBuf {
    parts.iter().fold(root.to_path_buf(), |p, s| p.join(s))
}

/// Copy the bundle's parts from the cache `from` into the data folder `to` (created).
/// `Err` names what is missing and the command that fetches it; nothing is copied then.
pub fn copy_data(from: &Path, to: &Path, images: bool) -> Result<Copied, String> {
    if from == to {
        return Err(format!(
            "{} is the cache itself: choose another folder",
            to.display()
        ));
    }
    let missing: Vec<String> = PARTS
        .iter()
        .filter(|p| p.required && !join(from, p.path).exists())
        .map(|p| {
            format!(
                "{} is missing: run `cargo run --release -p harness -- {}`",
                join(from, p.path).display(),
                p.fetch
            )
        })
        .collect();
    if !missing.is_empty() {
        return Err(missing.join("\n"));
    }
    let mut copied = Copied::default();
    let parts = PARTS.iter().chain(IMAGE_PARTS.iter().filter(|_| images));
    for part in parts {
        let src = join(from, part.path);
        if !src.exists() {
            continue;
        }
        copy_tree(&src, &join(to, part.path), &mut copied)
            .map_err(|e| format!("copying {}: {e}", src.display()))?;
    }
    Ok(copied)
}

fn copy_tree(src: &Path, dst: &Path, copied: &mut Copied) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        let mut entries: Vec<PathBuf> = std::fs::read_dir(src)?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<_, _>>()?;
        entries.sort();
        for entry in entries {
            let name = entry.file_name().expect("a directory entry has a name");
            copy_tree(&entry, &dst.join(name), copied)?;
        }
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        copied.bytes += std::fs::copy(src, dst)?;
        copied.files += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("apnap-bundle-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, parts: &[&str], text: &str) {
        let path = join(root, parts);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn a_cache(root: &Path) {
        let res = ["forge", "repo", "forge-gui", "res"];
        write(
            root,
            &[&res[..], &["cardsfolder", "t", "test_card.txt"]].concat(),
            "Name:Test Card",
        );
        write(
            root,
            &[&res[..], &["tokenscripts", "test_token.txt"]].concat(),
            "Name:Test Token",
        );
        write(root, &["forge", "repo", "LICENSE"], "GPL");
        write(root, &["forge", "repo", ".git", "config"], "not copied");
        write(root, &["bulk", "oracle-cards.jsonl.gz"], "bulk");
        write(root, &["scryfall", "cards.json"], "{}");
        write(root, &["scryfall", "images", "a.jpg"], "image");
    }

    // D-095: the bundle has the cache's layout and only what a game needs.
    #[test]
    fn the_data_folder_has_the_scripts_and_card_data_but_no_images_unless_asked() {
        let (from, to) = (scratch("from"), scratch("to"));
        a_cache(&from);
        let copied = copy_data(&from, &to, false).unwrap();
        assert_eq!(copied.files, 5);
        let res = to.join("forge/repo/forge-gui/res");
        assert!(res.join("cardsfolder/t/test_card.txt").is_file());
        assert!(res.join("tokenscripts/test_token.txt").is_file());
        assert!(to.join("forge/repo/LICENSE").is_file());
        assert!(to.join("bulk/oracle-cards.jsonl.gz").is_file());
        assert!(to.join("scryfall/cards.json").is_file());
        assert!(!to.join("forge/repo/.git").exists());
        assert!(!to.join("scryfall/images").exists());

        let copied = copy_data(&from, &to, true).unwrap();
        assert_eq!(copied.files, 6);
        assert!(to.join("scryfall/images/a.jpg").is_file());
        std::fs::remove_dir_all(from).unwrap();
        std::fs::remove_dir_all(to).unwrap();
    }

    #[test]
    fn a_missing_part_is_named_with_its_fetch_command_and_nothing_is_copied() {
        let (from, to) = (scratch("from-missing"), scratch("to-missing"));
        a_cache(&from);
        std::fs::remove_file(from.join("bulk/oracle-cards.jsonl.gz")).unwrap();
        let err = copy_data(&from, &to, false).unwrap_err();
        assert!(err.contains("oracle-cards.jsonl.gz"), "{err}");
        assert!(err.contains("fetch-bulk"), "{err}");
        assert_eq!(std::fs::read_dir(&to).unwrap().count(), 0);
        assert!(copy_data(&from, &from, false).is_err());
        std::fs::remove_dir_all(from).unwrap();
        std::fs::remove_dir_all(to).unwrap();
    }
}
