# CLAUDE.md: apnap — MTG rules engine (Commander + 1v1) and desktop app

Rust workspace. `docs/spec.md` is the architecture source of truth; deviations are logged in
`docs/decisions.md` (D-xxx) and must be told to the user before they are made. The
Comprehensive Rules (effective **2026-11-06**) are at `docs/rules/MagicCompRules.txt`
(gitignored — never commit it). When the spec and the CR disagree, **the CR wins**: flag it.

## Invariants (never violate)
1. engine-core has NO I/O, threads, wall clock, or ambient RNG. Randomness = `state.rng`
   only. engine-core and card-model are `#![no_std]` (D-003); keep them that way.
2. Every observable mutation goes through `actions::execute(GameAction)`. No direct writes
   to GameState from frame/effect code (bookkeeping fields of frames excepted).
3. Never read CardDef printed characteristics for objects on battlefield/stack — use
   `layers::chars(state, id)`.
4. A static ability's effect exists only if its source currently has that ability
   (re-check during the layer walk).
5. Option lists are emitted in stable order (timestamp, then seat order). Never iterate
   HashMap. IDs are monotonic u32.
6. The AI never sees GameState — only PlayerView (+ Determinizer for search).
7. AI budgets are iteration counts, never time.
8. Unverified cards do not load in strict mode. Never approximate a card.
9. Cite rules as `// CR 613.8 (2026-11-06)`. Before implementing a rules area, extract the
   current text into `docs/rules-notes/<area>.md` and write tests from that text, not from
   the spec. Mandatory for 603, 613, 614–616, 704, 800.4, 903.9 (spec paraphrases unverified).
10. Externally tagged serde enums only (RON limitation).
11. The engine never calls out. Anything that needs a player choice SUSPENDS: return
    `Flow::Suspend(frame, PendingDecision)` from a `Frame`. State needed after a suspension
    lives in the frame, never in a Rust local (D-002; rules in `frames/mod.rs`).
12. Illegal responses are rejected before any mutation (state hash unchanged).
13. No Wizards/Scryfall assets in the repo (card data/images are fetched at runtime into the
    XDG cache). Test cards are invented ("Test …"). Prior-art engines (Forge, XMage,
    MTG-Ichor, phase-rs, …) are design references only — never copy their code.

## Conventions
- Library: index 0 = bottom, last = top. Stack/graveyard: last = top. Battlefield: timestamp order.
- `Response::Pick` is a set; ordered answers use `Order`. `MulliganBottom/Order`: last listed
  ends up bottom-most.
- Events contain hidden info (engine truth); redact before showing to a player (D-014).
- New dependency in engine-core/card-model ⇒ add to `PURE_ALLOWLIST` in `crates/xtask` AND
  log a decision. Must be `no_std`-capable with `default-features = false`.

## Commands
```
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask check-deps                                    # dependency direction + purity
cargo build -p engine-core -p card-model --target thumbv7em-none-eabihf   # no_std proof
cargo run -p harness -- run --seed 42 --games 10          # random-agent games
cargo run -p harness -- determinism --seed 42 --runs 3    # identical hashes
cargo run -p harness -- run --seed 42 --save-replay r.json && cargo run -p harness -- replay r.json
cargo run -p harness -- bless                             # ONLY when a hash change is intended; review why
```

## Definition of done for any change
- fmt + clippy `-D warnings` clean; all tests pass; check-deps + no_std build pass.
- Rules work: an atomic test citing the CR number, written from `docs/rules-notes/`.
- Golden hashes unchanged, or re-blessed with the reason stated in the commit message.
- Determinism: same seed 3× identical; save/load and clone mid-game replay identically.

## Layout
- `crates/engine-core` — pure engine. `engine.rs` (new_game/start/step), `frames/`
  (continuation stack: setup, turn, priority/cast/resolve, combat), `actions.rs`
  (chokepoint, move_object), `sba.rs`, `layers.rs` (chars; printed-only until M2),
  `decision.rs`, `events.rs`, `state.rs`, `view.rs`, `rng.rs`, `hash.rs`.
- `crates/card-model` — card vocabulary types (no_std). `crates/cards` — card data (M0: test pool).
- `crates/ai` — agents (M0: RandomAgent). `crates/harness` — CLI + determinism/golden/property tests.
- `crates/card-db`, `crates/app` — stubs until M1/M4. `crates/xtask` — check-deps.

## Current milestone: M0 — DONE (2026-11-08). Next: M1 (do not start without the user's OK).
M0 delivered: workspace, CI, ids/zones/GameState, move_object, seeded RNG, step() with
PendingDecision suspension, serde round-trip, harness with random agents; all three M0
acceptance criteria tested (`crates/harness/tests/determinism.rs`).
Known M0 simplifications to remove later: see D-010.
Spec discrepancies already found: see the end of `docs/decisions.md`.
