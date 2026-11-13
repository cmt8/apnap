//! `cargo xtask bundle [--out DIR] [--target TRIPLE] [--with-images]` (D-095)
//!
//! Makes one folder that can be zipped and sent to someone: the desktop app, the terminal
//! harness, and an `apnap-data` folder with everything a game reads (Forge's card scripts,
//! Scryfall's Oracle card file, the precons' printed data), taken from this machine's
//! cache. The programs use an `apnap-data` folder next to them before any cache of the
//! user's (`card_db::scryfall::cache_dir`), so nothing has to be fetched or installed.
//!
//! The bundle carries what the GPL asks of a binary that is handed on: the licence texts
//! and the source it was built from (`git archive` of HEAD). Card images are left out
//! unless `--with-images` is given: they are Wizards of the Coast's art, not covered by
//! any licence here, and the app downloads the ones it shows.
//!
//! Nothing under the output folder belongs in the repository (`/dist` is ignored).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Marks a folder as one this command made (and so may replace).
const MARKER: &str = ".apnap-bundle";
const DATA_DIR: &str = "apnap-data";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the workspace root exists")
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let shown = format!("{cmd:?}");
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{shown} failed ({s})")),
        Err(e) => Err(format!("couldn't run {shown}: {e}")),
    }
}

fn output(cmd: &mut Command) -> Result<String, String> {
    let shown = format!("{cmd:?}");
    let out = cmd
        .output()
        .map_err(|e| format!("couldn't run {shown}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{shown} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// May `out` be (re)made? Only if it doesn't exist, is empty, or is a bundle made here.
fn replaceable(out: &Path) -> Result<(), String> {
    if !out.exists() {
        return Ok(());
    }
    if !out.is_dir() {
        return Err(format!("{} exists and is not a folder", out.display()));
    }
    let empty = std::fs::read_dir(out)
        .map_err(|e| e.to_string())?
        .next()
        .is_none();
    if empty || out.join(MARKER).is_file() {
        return Ok(());
    }
    Err(format!(
        "{} exists and wasn't made by `cargo xtask bundle`: remove it or pass another --out",
        out.display()
    ))
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                dir_size(&path)
            } else {
                e.metadata().map_or(0, |m| m.len())
            }
        })
        .sum()
}

fn readme(commit: &str, dirty: bool, images: bool, windows: bool) -> String {
    let (app, harness) = if windows {
        ("apnap.exe", "apnap-harness.exe")
    } else {
        ("./apnap", "./apnap-harness")
    };
    let built_from = if dirty {
        format!("{commit} plus uncommitted changes (not in the source archive)")
    } else {
        commit.to_string()
    };
    let pictures = if images {
        "The card images this copy was made with are included."
    } else {
        "Card images are not included: the app downloads the ones it shows from Scryfall\n\
         (into apnap-data) when it is online, and draws plain cards when it is not."
    };
    format!(
        "apnap - a Magic: The Gathering rules engine with a reference desktop app\n\
         \n\
         Run:   {app}\n\
         \u{20}      {harness} play --setup precon-4p     (the same game in a terminal)\n\
         \n\
         Keep this folder together. The programs read everything from the apnap-data folder\n\
         next to them and write saved games and downloaded images there; nothing is\n\
         installed and nothing else on the machine is touched.\n\
         \n\
         {pictures}\n\
         \n\
         What is in here\n\
         \u{20} apnap, apnap-harness      built from commit {built_from}\n\
         \u{20} apnap-source.tar.gz       the source of that commit\n\
         \u{20} LICENSE                   apnap is GPL-3.0-or-later\n\
         \u{20} apnap-data/forge          card scripts from Forge (github.com/Card-Forge/forge),\n\
         \u{20}                           GPL v3; its licence is in apnap-data/forge/repo/LICENSE\n\
         \u{20} apnap-data/bulk, scryfall card data from Scryfall (scryfall.com)\n\
         \n\
         apnap is unofficial and not affiliated with, endorsed or sponsored by Wizards of the\n\
         Coast. Magic: The Gathering, its card names and card text are property of Wizards of\n\
         the Coast LLC.\n"
    )
}

fn build(args: &[String]) -> Result<PathBuf, String> {
    let root = root();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = flag(args, "--out").map_or_else(|| root.join("dist").join("apnap"), PathBuf::from);
    let target = flag(args, "--target");
    let images = args.iter().any(|a| a == "--with-images");
    let windows = target
        .as_deref()
        .map_or(cfg!(windows), |t| t.contains("windows"));
    let exe = if windows { ".exe" } else { "" };
    replaceable(&out)?;

    // The programs. The harness is built for this machine too: it copies the data.
    println!("building the app and the harness (release)...");
    let mut cmd = Command::new(&cargo);
    cmd.current_dir(&root).args([
        "build",
        "--release",
        "--locked",
        "-p",
        "app",
        "-p",
        "harness",
    ]);
    if let Some(t) = &target {
        cmd.args(["--target", t]);
    }
    run(&mut cmd)?;
    let host_dir = root.join("target").join("release");
    if target.is_some() {
        run(Command::new(&cargo).current_dir(&root).args([
            "build",
            "--release",
            "--locked",
            "-p",
            "harness",
        ]))?;
    }
    let built = target.as_ref().map_or_else(
        || host_dir.clone(),
        |t| root.join("target").join(t).join("release"),
    );

    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| format!("removing {}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("creating {}: {e}", out.display()))?;
    std::fs::write(out.join(MARKER), "made by `cargo xtask bundle`\n")
        .map_err(|e| e.to_string())?;
    let copy = |from: PathBuf, to: &str| -> Result<(), String> {
        std::fs::copy(&from, out.join(to))
            .map(|_| ())
            .map_err(|e| format!("copying {}: {e}", from.display()))
    };
    copy(built.join(format!("app{exe}")), &format!("apnap{exe}"))?;
    copy(
        built.join(format!("harness{exe}")),
        &format!("apnap-harness{exe}"),
    )?;
    copy(root.join("LICENSE"), "LICENSE")?;

    // The data, out of this machine's cache.
    let host_exe = if cfg!(windows) { ".exe" } else { "" };
    let mut cmd = Command::new(host_dir.join(format!("harness{host_exe}")));
    cmd.current_dir(&root)
        .arg("bundle-data")
        .arg("--out")
        .arg(out.join(DATA_DIR));
    if images {
        cmd.arg("--with-images");
    }
    run(&mut cmd)?;

    // The source the programs were built from (GPL: it goes where the binaries go).
    let git = |args: &[&str]| output(Command::new("git").current_dir(&root).args(args));
    let commit = git(&["rev-parse", "--short=12", "HEAD"])?;
    let dirty = !git(&["status", "--porcelain"])?.is_empty();
    let archive = out.join("apnap-source.tar.gz");
    run(Command::new("git")
        .current_dir(&root)
        .args(["archive", "--format=tar.gz", "--prefix=apnap-source/", "-o"])
        .arg(&archive)
        .arg("HEAD"))?;
    std::fs::write(
        out.join("README.txt"),
        readme(&commit, dirty, images, windows),
    )
    .map_err(|e| e.to_string())?;
    if dirty {
        println!(
            "warning: the working tree has uncommitted changes; the source archive is {commit} without them"
        );
    }

    // Proof that the folder stands on its own: the bundled harness plays a precon game with
    // no cache of the user's to fall back on. (Only when it can run on this machine.)
    if target.is_none() {
        let empty = std::env::temp_dir().join(format!("apnap-bundle-check-{}", std::process::id()));
        std::fs::create_dir_all(&empty).map_err(|e| e.to_string())?;
        println!("checking the bundle with an empty user cache...");
        let result = run(Command::new(out.join(format!("apnap-harness{exe}")))
            .current_dir(&out)
            .env_remove("APNAP_CACHE_DIR")
            .env("XDG_CACHE_HOME", &empty)
            .env("HOME", &empty)
            .env("LOCALAPPDATA", &empty)
            .args(["fuzz", "--setup", "precon-4p", "--games", "1"]));
        let _ = std::fs::remove_dir_all(&empty);
        result.map_err(|e| format!("the bundle doesn't stand on its own: {e}"))?;
    } else {
        println!("not checked: the bundled programs are for another target and can't run here");
    }
    Ok(out)
}

pub fn bundle(args: &[String]) -> ExitCode {
    match build(args) {
        Ok(out) => {
            println!(
                "bundle: {} ({:.0} MB). Zip that folder and send it.",
                out.display(),
                dir_size(&out) as f64 / 1e6
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bundle failed: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // D-095: only a folder this command made (or an empty one) is ever replaced.
    #[test]
    fn only_a_bundle_or_an_empty_folder_is_replaced() {
        let dir = std::env::temp_dir().join(format!("apnap-xtask-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(replaceable(&dir).is_ok());
        std::fs::create_dir_all(&dir).unwrap();
        assert!(replaceable(&dir).is_ok());
        std::fs::write(dir.join("someone's file"), b"x").unwrap();
        assert!(replaceable(&dir).is_err());
        std::fs::write(dir.join(MARKER), b"").unwrap();
        assert!(replaceable(&dir).is_ok());
        assert!(replaceable(&dir.join("someone's file")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_readme_says_what_it_was_built_from_and_whether_images_are_inside() {
        let text = readme("abc123", true, false, false);
        assert!(text.contains("abc123 plus uncommitted changes"));
        assert!(text.contains("Card images are not included"));
        assert!(text.contains("./apnap-harness play"));
        let text = readme("abc123", false, true, true);
        assert!(text.contains("commit abc123\n"));
        assert!(text.contains("are included"));
        assert!(text.contains("apnap.exe"));
        assert!(text.is_ascii());
    }
}
