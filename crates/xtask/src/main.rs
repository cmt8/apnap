//! `cargo xtask check-deps`
//!
//! 1. Internal edges: each workspace crate may depend (any kind) only on the crates listed in
//!    `ALLOWED_EDGES`. In particular nothing depends on `app`/`harness`, and `engine-core`
//!    depends on no other internal crate except `card-model` (spec §4: "app and ai depend on
//!    engine-core, never the reverse").
//! 2. Purity: the transitive *normal* dependencies of `engine-core` and `card-model` must be
//!    in `PURE_ALLOWLIST`. Adding a crate there is a design decision: log it in
//!    docs/decisions.md.

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
        &["engine-core", "card-model", "cards", "ai", "card-db"],
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

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    match cmd.as_str() {
        "check-deps" => check_deps(),
        _ => {
            eprintln!("usage: cargo xtask check-deps");
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
