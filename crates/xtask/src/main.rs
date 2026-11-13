//! `cargo xtask bundle`: see `bundle.rs` (a folder to zip and send, D-095).
//!
//! `cargo xtask check-deps`
//!
//! 1. Internal edges: each workspace crate may depend (any kind) only on the crates listed in
//!    `ALLOWED_EDGES`. In particular nothing depends on `app`/`harness`, and `engine-core`
//!    depends on no other internal crate except `card-model` (spec §4: "app and ai depend on
//!    engine-core, never the reverse").
//! 2. Purity: the transitive *normal* dependencies of `engine-core` and `card-model` must be
//!    in `PURE_ALLOWLIST`. Adding a crate there is a design decision: log it in
//!    docs/decisions.md.
//! 3. The client boundary (D-092): client code — the app's UI (`ui.rs`, `cardview.rs`,
//!    `combat.rs`, `art.rs`), the narrator (`harness/src/text.rs`) and the `ai` crate — never
//!    names the engine's state (`GameState`, a session's `.st`) or what only the host can
//!    make from it (`player_view`, `seat_view`, `EventView::new`). A client is handed its
//!    view, its decision, its events and the catalog. The host side (`app/src/session.rs`,
//!    the harness driver, tests, and a file's `#[cfg(test)]` module) owns the engine.

mod bundle;

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, ExitCode};

const ALLOWED_EDGES: &[(&str, &[&str])] = &[
    ("card-model", &[]),
    ("engine-core", &["card-model"]),
    ("cards", &["card-model"]),
    ("card-db", &["card-model"]),
    ("ai", &["engine-core", "card-model"]),
    (
        "harness",
        &["engine-core", "card-model", "cards", "ai", "card-db"],
    ),
    (
        "app",
        &[
            "engine-core",
            "card-model",
            "cards",
            "ai",
            "card-db",
            "harness",
        ],
    ),
    ("xtask", &[]),
];

const PURE_ROOTS: &[&str] = &["engine-core", "card-model"];

/// External crates allowed (transitively) under the pure crates. serde's derive macro stack
/// runs at compile time only; ppv-lite86/zerocopy are rand_chacha's SIMD backend.
const PURE_ALLOWLIST: &[&str] = &[
    "card-model",
    "engine-core",
    "serde",
    "serde_core",
    "serde_derive",
    "proc-macro2",
    "quote",
    "syn",
    "unicode-ident",
    "rand_chacha",
    "rand_core",
    "ppv-lite86",
    "zerocopy",
    "zerocopy-derive",
];

/// Client source files (relative to the workspace root); every `.rs` file under a directory
/// ending in `/`.
const CLIENT_SOURCES: &[&str] = &[
    "crates/app/src/ui.rs",
    "crates/app/src/cardview.rs",
    "crates/app/src/combat.rs",
    "crates/app/src/art.rs",
    "crates/harness/src/text.rs",
    "crates/ai/src/",
];

/// What client code must not name: the state, and what only its owner can make from it.
const HOST_ONLY: &[&str] = &[
    "GameState",
    ".st",
    "player_view",
    "seat_view",
    "EventView::new",
];

fn client_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for s in CLIENT_SOURCES {
        let p = root.join(s);
        if s.ends_with('/') {
            let mut dir: Vec<_> = std::fs::read_dir(&p)
                .expect("a client source directory")
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "rs"))
                .collect();
            dir.sort();
            out.extend(dir);
        } else {
            assert!(
                p.is_file(),
                "client source {s} is missing: update CLIENT_SOURCES"
            );
            out.push(p);
        }
    }
    out
}

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Each (line number, word) of `HOST_ONLY` named in code (not comments) before the file's
/// `#[cfg(test)]` module.
fn boundary_violations(text: &str) -> Vec<(usize, &'static str)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.split("//").next().unwrap_or("");
        for word in HOST_ONLY {
            let mut from = 0;
            while let Some(i) = code[from..].find(word) {
                let at = from + i;
                let end = at + word.len();
                let before_ok =
                    word.starts_with('.') || !code[..at].chars().next_back().is_some_and(ident);
                let after_ok = !code[end..].chars().next().is_some_and(ident);
                if before_ok && after_ok {
                    out.push((n + 1, *word));
                }
                from = end;
            }
        }
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check-deps") => check_deps(),
        Some("bundle") => bundle::bundle(&args[1..]),
        _ => {
            eprintln!(
                "usage: cargo xtask check-deps\n       cargo xtask bundle [--out DIR] [--target TRIPLE] [--with-images]"
            );
            ExitCode::FAILURE
        }
    }
}

fn check_deps() -> ExitCode {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .expect("run cargo metadata");
    if !out.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&out.stderr));
        return ExitCode::FAILURE;
    }
    let meta: Value = serde_json::from_slice(&out.stdout).expect("metadata json");

    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for p in meta["packages"].as_array().unwrap() {
        names.insert(
            p["id"].as_str().unwrap().into(),
            p["name"].as_str().unwrap().into(),
        );
    }
    let members: BTreeSet<String> = meta["workspace_members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| names[id.as_str().unwrap()].clone())
        .collect();

    // node name -> [(dep name, is_normal)]
    let mut graph: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
    for node in meta["resolve"]["nodes"].as_array().unwrap() {
        let from = names[node["id"].as_str().unwrap()].clone();
        let deps = node["deps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                let to = names[d["pkg"].as_str().unwrap()].clone();
                let normal = d["dep_kinds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|k| k["kind"].is_null() || k["kind"] == "build");
                (to, normal)
            })
            .collect();
        graph.insert(from, deps);
    }

    let allowed: BTreeMap<&str, &[&str]> = ALLOWED_EDGES.iter().copied().collect();
    let mut errors = Vec::new();

    for m in &members {
        let Some(ok) = allowed.get(m.as_str()) else {
            errors.push(format!(
                "workspace crate `{m}` has no entry in ALLOWED_EDGES"
            ));
            continue;
        };
        for (dep, _) in graph.get(m).into_iter().flatten() {
            if members.contains(dep) && !ok.contains(&dep.as_str()) {
                errors.push(format!("forbidden internal dependency: {m} -> {dep}"));
            }
        }
    }

    for root in PURE_ROOTS {
        let mut stack = vec![root.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(n) = stack.pop() {
            if !seen.insert(n.clone()) {
                continue;
            }
            if !PURE_ALLOWLIST.contains(&n.as_str()) {
                errors.push(format!("`{n}` is not allowed under pure crate `{root}`"));
            }
            for (dep, normal) in graph.get(&n).into_iter().flatten() {
                if *normal {
                    stack.push(dep.clone());
                }
            }
        }
        println!("{root}: {} crates in normal dependency closure", seen.len());
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let clients = client_files(&root);
    for path in &clients {
        let text = std::fs::read_to_string(path).expect("read a client source file");
        for (line, word) in boundary_violations(&text) {
            errors.push(format!(
                "client code reaches into the host (D-092): {}:{line} names `{word}`",
                path.strip_prefix(&root).unwrap_or(path).display()
            ));
        }
    }
    println!("client boundary: {} files checked", clients.len());

    if errors.is_empty() {
        println!("check-deps: OK");
        ExitCode::SUCCESS
    } else {
        for e in &errors {
            eprintln!("check-deps: {e}");
        }
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::boundary_violations;

    #[test]
    fn the_boundary_check_finds_state_reads_and_nothing_else() {
        let src = "let t = &session.st.turn;\nfn f(st: &GameState) {}\n\
            let v = player_view(&x, p);\nlet a = s.stops; let b = v.step; // session.st\n\
            let c = MyGameStateish; let d = first.st_x;\n#[cfg(test)]\nlet e = s.st;";
        assert_eq!(
            boundary_violations(src),
            [(1, ".st"), (2, "GameState"), (3, "player_view")]
        );
    }
}
