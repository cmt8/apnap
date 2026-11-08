# Design decisions

Newest last. Each entry: decision, why, and where it lives. "Spec" = `docs/spec.md`.
CR = Comprehensive Rules effective 2026-11-06.

## D-001 Workspace layout (M0, 2026-11-08)
Crate names follow spec §4 (`engine-core`, `card-model`, `card-db`, `cards`, `ai`, `harness`,
`app`) at the repo root (spec draws `mtg/`). Added `xtask` (repo automation, `cargo xtask …`
alias in `.cargo/config.toml`). Toolchain pinned in `rust-toolchain.toml` (1.99.1), edition 2024.
`card-db` and `app` are stubs; `app` has no egui dependency until M4.

## D-002 Suspend/resume model: a unified, serializable continuation stack (M0)
**The engine never calls out.** `GameState.frames: Vec<Frame>` is an explicit continuation
stack (the spec's `ResolutionFrame`s). *Everything* that can wait for a player is a `Frame`
variant holding its progress as data: game setup, mulligans, the step driver, priority,
casting (including a partially paid cost), resolution, attack/block declarations, combat
damage division, cleanup discard. The driver pops the top frame and runs it; a frame either
`Continue`s (pushing follow-ups) or `Suspend`s (re-pushing itself + returning a
`PendingDecision`).
- The pending decision is stored **in** `GameState.pending`, so a loaded save knows what it
  is waiting on (spec implies, doesn't state).
- `step()` validates the response's shape against the stored `Primitive` **before any
  mutation**; illegal responses leave the state bit-identical (tested).
- Options are structured `OptionDesc` values; frames read the chosen option back from the
  stored decision, so the option list is the single source of truth for what an index means.
- M0 invariant: every option offered is legal, so every shape-valid response is legal. M1+
  will need end-of-declaration validation for some decisions (attack requirements, 508.1d);
  that will be a frame-level check that still runs before mutation.
- Rules for frame code are at the top of `crates/engine-core/src/frames/mod.rs`.
- Why unified rather than spec §6's `turn.cursor` + separate resolution frames: one
  mechanism, one place where suspension happens, and nested cases (a replacement choice
  during a draw during resolution during a cast…) are just deeper stacks.

## D-003 engine-core and card-model are `#![no_std]` + `alloc` (M0; user decision)
The compiler forbids `std::fs`, `std::thread`, `std::time`, `HashMap`. Enforced three ways:
`no_std` itself; CI builds both crates for `thumbv7em-none-eabihf` (catches a dependency
silently pulling in std); `cargo xtask check-deps` (internal edge table + an allowlist of
the pure crates' transitive dependencies). Adding a crate to `PURE_ALLOWLIST` is a design
decision and must be logged here. Bonus: WASM stays possible.

## D-004 Formats are configuration from day one (M0) — **spec change**
Spec §1 scopes v1 to Commander and lists other formats as a non-goal, and §5 hard-codes
life 40. The user wants 60-card 1v1 too. `GameConfig { format, num_players,
starting_life, starting_hand_size, max_hand_size }` with `constructed_1v1()` (CR 103.4: 20)
and `commander(n)` (CR 103.4c: 40). Format-dependent rules read the config: CR 103.8a
(two-player starter skips first draw) vs 103.8c, CR 103.5c (free first mulligan in
multiplayer). M0 runs 1v1 constructed.

## D-005 Card catalog stored once per game; objects hold `CardDefId` — **spec change**
Spec §5 puts `card: Arc<CardDef>` on every `GameObject`, which would serialize a full card
definition per object into every save. Instead `GameState.catalog: Arc<CardCatalog>` is
serialized once and objects store a `CardDefId(u32)`. Clone stays a refcount bump; saves
stay self-contained.

## D-006 Randomness: ChaCha8 via `next_u64` only; our own algorithms on top (M0)
`rand`'s `seed_from_u64`, `gen_range` and `shuffle` are not guaranteed value-stable across
versions, which would silently break every replay on a dependency bump. `GameRng` wraps
`ChaCha8Rng` and implements SplitMix64 seeding, unbiased rejection sampling (`below`) and
Fisher–Yates itself. Serialization is ours too (seed, stream, word position): independent
of rand_chacha's serde format, and avoids its `serde` feature, which enables serde's `std`.
`stream_is_value_stable` pins the first outputs for seed 42. Agents have their own
`GameRng` seeded by `harness::agent_seed(game_seed, seat)`; they never touch the game RNG.

## D-007 Stable hashing for golden tests (M0)
`core::hash::Hash`/`DefaultHasher` aren't stable across Rust releases and `usize` differs by
platform. `hash.rs` feeds a value's **serde data model** into FNV-1a-64 through a minimal
canonical `Serializer` (tagged primitives, LE bytes, `u64` lengths, variants by index).
`GameState.log_hash` folds in every event as it is emitted (resumable across save/load);
`state_hash()` hashes the whole state. Golden hashes for fixed seeds live in
`crates/harness/tests/golden/m0.txt`; regenerate only with `cargo run -p harness -- bless`.
CI runs the golden test on Linux, macOS and Windows: this is the cross-platform
determinism check.

## D-008 Primitive shapes take their size from `options` (M0) — minor spec change
Spec's `Allocate { recipients, .. }` and `Order { n }` duplicate `options.len()`; dropped.
`Allocate` per-recipient bounds are indexed like `options`. `Response::Pick` is a set
(order ignored); ordered answers always use `Order`.

## D-009 The stack zone is a list of object ids in M0
`zones.stack: Vec<ObjectId>` (top = last). Spec's `StackItem` (targets, modes, X, ability
source/LKI) arrives with M1, when abilities on the stack need representation.

## D-010 Temporary M0 simplifications (each must be removed by the milestone noted)
- **Mana:** only lands with exactly one basic land type are sources; the engine pays generic
  from the pool in a fixed order (C, W, U, B, R, G). PayMana only offers sources that keep the
  cost payable (exact for single-type sources). → M1 casting pipeline.
- **Two players only:** `new_game` rejects other counts, because eliminating a player in a
  multiplayer game requires CR 800.4a. → M1 (turn order for 4) / M3 (800.4).
- **Blockers block exactly one attacker** (true without abilities). Multiple blockers per
  attacker ARE supported, with damage division via `Allocate` (D-016).
- **Sorcery-speed creature spells only**; no instants, abilities, triggers, replacements.
These match the spec's M0 scope ("vanilla creatures and basic lands").

## D-011 Mulligans follow the CR 103.5 text literally (M0)
CR 103.5 puts cards on the bottom *as part of each mulligan* ("draws a new hand … then puts
a number of those cards equal to the number of times that player has taken a mulligan on the
bottom"), not after keeping. Implemented that way: PickN for which cards, then Order when 2+.
Mulligan limit per 103.5 ("until their opening hand would be zero cards"); 103.5c free first
mulligan in multiplayer.

## D-012 Verbatim CR excerpts are committed in `docs/rules-notes/` (user decision 2026-11-08)
The full CR txt stays gitignored (`docs/rules/`). Excerpts needed to write tests are quoted
verbatim in `docs/rules-notes/<area>.md`. Revisit before ever publishing the repo
(Fan Content Policy excludes "verbatim copying and reposting", spec §12).

## D-013 The mutation chokepoint exists from M0
`actions::execute(GameAction) -> ActionResult` is the only mutator of observable state;
`move_object` (new `ObjectId` + timestamp per CR 400.7, removes from combat per 506.4) is a
wrapper over it. M2 inserts the CR 614–616 pipeline inside `execute`. Open M2 question:
`execute` will need to suspend for `ChooseReplacement`; the plan is to make the calling
frames resumable around actions (a pending-action frame), not to add callbacks.

## D-014 Events include hidden information
Events are engine truth (e.g. `Drew { card }`). The event hash covers them all. A per-player
redacted log is needed before any event reaches a UI or an agent (M4).

## D-015 Save format: JSON in M0
Harness saves/loads with `serde_json` (debuggable). The state format is serde-agnostic;
the app may choose a compact binary format in M4.

## D-016 Combat damage division per current CR 510.1c (M0)
"If two or more creatures are blocking it, it assigns its combat damage to those creatures
divided as its controller chooses among them." No damage assignment order (removed by the
2025 rules). Implemented as an `Allocate` decision per multiply-blocked attacker.

---

## Spec discrepancies found (CR wins; reported to the user 2026-11-08)
1. **CR 903.9b** still exists: a commander that would go to its owner's hand or library may
   go to the command zone instead (a replacement effect that "may apply more than once…
   exception to rule 614.5"). Spec §8.1 calls this "older rules". Both 903.9a (SBA,
   graveyard/exile) and 903.9b apply. → M3.
2. **CR 603.3b** is a *two-part* APNAP process: first each player (APNAP) puts triggers
   "with a trigger condition that isn't another ability triggering" on the stack, then each
   player (APNAP) puts the remaining ones. Spec §6 omits this. → M2.
3. **CR 800.4a** is more specific than spec §8.7: objects the player owns leave; control
   effects end; objects on the stack *not represented by cards* cease to exist; anything
   still controlled by them is **exiled**; priority passes if they had it. → M3.
4. Formats (D-004), per-object `Arc<CardDef>` (D-005), primitive fields (D-008).
5. Spec §11 "the AI runs on worker threads with cloned state" contradicts invariant 6 (AI
   sees only `PlayerView` + Determinizer). The app should send `PlayerView` (and, for search
   AIs, a Determinizer-produced sample), not the real state.
6. Rand / hash stability (D-006, D-007) are additions the spec doesn't mention.
