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
cargo run -p harness -- watch --seed 42                   # narrated random-agent game
cargo run -p harness -- play [--seed N]                   # you (P0) vs a random agent in the terminal
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

## Current milestone: M1 — IN PROGRESS (started 2026-11-08). M0 done 2026-11-08.
M1 scope (spec §13 as amended): CR 117/601.2/602/605/608 casting + abilities, 4-player turn
order, CR 800.4a (pulled forward), RON test cards via card-db, resumable resolution steps,
remove D-010 simplifications. Acceptance: atomic tests 117.3–117.5, 101.4, 608.2b fizzle,
800.4a; 4 random agents x 1,000 games, 0 panics.

### M1 WIP handoff (branch `m1-wip`, 2026-11-08) — DOES NOT COMPILE YET; `main` is green
Done on the branch: rules-notes for 101.4/117/601/602/605/608/800.4 (also on main);
card-model vocabulary (Keyword, AbilityDef, Body/Mode, TargetFilter, Cost, Effect, ManaType,
`{X}`, ManaCost as text, CardScript); card-db RON loader + validation (+tests); RON card files
and decks in crates/cards (M0 vanilla pool + M1 test cards); engine: mana.rs (bipartite
feasibility, generic-allocation choice), stack.rs, targeting.rs, actions.rs (stack-aware
moves, PutAbilityOnStack, SetStackChoices, RemoveAbility, GainLife/LoseLife, LeaveGame =
CR 800.4a), sba.rs (multiplayer losers leave), frames/cast.rs (601.2/602.2 pipeline),
frames/resolve.rs (work list, fizzle, counter, APNAP each-player choices),
frames/priority.rs (abilities, flash, floating mana), first_priority_player (800.4j).
Next steps, in order:
1. Re-apply the new `Event` variants (AbilityActivated, ModesChosen, XChosen, TargetsChosen,
   Fizzled, Countered, AbilityRemoved, Sacrificed, ObjectLeftGame, PlayerLeftGame; events.rs
   needs `use crate::stack::Target; use alloc::vec::Vec;`) and `OptionDesc` variants
   (ActivateAbility{source,index}, Mode{index}, Target{target}, Mana{mana}; decision.rs needs
   `use crate::stack::Target;`) — the scripted edit didn't apply because rustfmt had
   reflowed those enums.
2. Fix the remaining compile errors: engine.rs (`stack: Vec::new()`, drop the 2-player limit),
   view.rs (stack view from `st.stack`), `remove(_fizzled)` param in resolve.rs.
3. harness: load cards via card-db (`cards::CARDS`/`DECKS`), m0_game = m0_vanilla deck,
   m1_game(seed, players) = m1_red_green; text.rs option_line for new variants; scenario
   builder + driver for atomic tests; `fuzz --games --players --threads`.
4. Tests: rules_m1.rs (the test names cited in the card files + rules-notes), every verified
   card's tests exist, 4-player determinism/save-load/clone, 1,000 4p random games.
5. Re-bless golden hashes (catalog/state shape changed; explain in the commit). Log D-018
   (800.4 pulled into M1), D-019 (no rewinds: only completable choices), D-020 (resolution
   work list), D-021 (M1 mana abilities cost exactly {T}) in docs/decisions.md.

### M0 summary
M0 delivered: workspace, CI, ids/zones/GameState, move_object, seeded RNG, step() with
PendingDecision suspension, serde round-trip, harness with random agents; all three M0
acceptance criteria tested (`crates/harness/tests/determinism.rs`).
Known M0 simplifications to remove later: see D-010.
Spec discrepancies already found: see the end of `docs/decisions.md`.
