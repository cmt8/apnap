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
4. A static ability's effect exists only if its source currently has that ability when the
   effect would begin to apply (checked during the layer walk); once it has applied in one
   layer it keeps applying to the same objects in later layers (CR 613.6, D-023).
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
    `Flow::ask(...)` from a `Frame`. State needed after a suspension lives in the frame,
    never in a Rust local (D-002; rules in `frames/mod.rs`).
14. Replaceable events (zone moves incl. dying/destroy/discard/ETB, damage, draws, tokens,
    counters, life) are `replacement::Proposed` events pushed as `Frame::Apply` — never
    `execute`d directly from frame code (D-024). Triggers are detected only from what an
    Apply batch performed (D-025).
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
cargo run -p harness -- run --seed 42 --games 10          # random-agent games (--setup m0 | m1-Np)
cargo run --release -p harness -- fuzz --setup m2-4p --games 1000 --threads 8   # soak (m0 | m1-Np | m2-Np)
cargo run -p harness -- determinism --seed 42 --runs 3    # identical hashes
cargo run -p harness -- watch --seed 42                   # narrated random-agent game
cargo run -p harness -- play [--setup m1-4p] [--seed N]   # you (P0) vs random agents in the terminal
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
  (continuation stack: setup, turn, priority, cast, resolve, combat), `actions.rs`
  (chokepoint, move_object, LeaveGame = 800.4a), `sba.rs`, `layers.rs` (chars; printed-only until M2),
  `decision.rs`, `events.rs`, `state.rs`, `view.rs`, `rng.rs`, `hash.rs`.
- `crates/card-model` — card vocabulary types (no_std; externally tagged; ManaCost as text).
- `crates/cards` — RON card scripts `data/<letter>/<slug>.ron` and `decks/*.ron` (invented
  "Test …" cards + basic lands). Adding a file? Add it to `CARDS`/`DECKS` (a test checks).
- `crates/card-db` — parses + validates RON into a `CardCatalog` (strict mode rejects
  unverified cards; every verified card must name existing tests). Scryfall import: M4.
- `crates/ai` — agents (RandomAgent). `crates/harness` — CLI, `Setup` (m0 / m1-Np),
  `script::Script` test driver, determinism/golden/property/rules tests.
- `crates/app` — stub until M4. `crates/xtask` — check-deps.
- Engine extras: `stack.rs` (StackObject), `targeting.rs` (choose + 608.2b legality in one
  place), `frames/cast.rs` (601.2/602.2, and putting triggers on the stack), `frames/resolve.rs`
  (work list), `replacement.rs` + `frames/apply.rs` (614–616 pipeline), `triggers.rs` +
  `frames/put_triggers.rs` (603), `frames/check.rs` (117.5/704.3), `effects.rs` (resolved
  continuous effects, shields), `testing.rs` (feature `test-support`: scenario builder, D-022).

## Current milestone: M2 — DONE (2026-11-08). Next: M3 (do not start without the user's OK).
M2 delivered: layers (CR 613: every slot, timestamps, 613.8 dependency, 613.6 locking;
effects for layers 4–7), replacement/prevention pipeline (614–616, `frames/apply.rs`,
D-024), triggers + LKI (603, D-025/D-026), state check (117.5/704.3) with 704.5d/h/j/q,
tokens, counters, flying/reach blocking (D-027). Tests: `rules_m2.rs` (spec's 7 golden
cases + CR 613.4d/613.5/613.6 examples; mutation-checked), determinism/save-load/clone over
M2 setups incl. mid-pipeline suspensions; CI soak m2-4p x1000. Limits: D-028.
M3 starts with: rules-notes for 506–511 (combat keywords), 903 (Commander), the rest of 800.4
(control-change cases), then combat keywords, Commander, Auras/control effects (layer 2).

### M1 summary
M1 delivered priority/casting/abilities/resolution, 2–8 players with CR 800.4a/j/k, RON
cards via card-db, the scenario builder; decisions D-018–D-022, limits D-021.

### M0 summary
M0 delivered: workspace, CI, ids/zones/GameState, move_object, seeded RNG, step() with
PendingDecision suspension, serde round-trip, harness with random agents; all three M0
acceptance criteria tested (`crates/harness/tests/determinism.rs`).
Known M0 simplifications to remove later: see D-010.
Spec discrepancies already found: see the end of `docs/decisions.md`.
