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
    XDG cache). Test cards are invented ("Test …"). The project is GPL-3.0-or-later (D-046):
    Forge's card scripts (GPL v3) are compiled into our cards, and its sources may be read
    to learn what a script feature means — but the scripts are fetched into the cache, not
    committed (they contain card names and Oracle text), and engine code is written for
    this engine, not transplanted. Non-GPL-compatible engines stay design references only.
15. The Forge compiler (`card_db::forge`) never drops or guesses: a card compiles only if
    every line, parameter, filter property and expression of its script is understood;
    anything else is a named blocker. A word in a filter is a subtype only if it is a
    known subtype. Every script feature the compiler accepts needs a test of its meaning.

## Conventions
- Library: index 0 = bottom, last = top. Stack/graveyard: last = top. Battlefield: timestamp order.
- `Response::Pick` is a set; ordered answers use `Order`. `MulliganBottom/Order`: last listed
  ends up bottom-most.
- Events contain hidden info (engine truth); redact before showing to a player (D-014).
- An ability is named by `AbilityId { card, index, granted }` — where its text is written —
  never by a bare index into its object's card: an object can have abilities of other
  cards (`CardDef::granted`, `ContinuousEffect::GrantAbilities`, D-064), and have one twice.
  Read abilities through `layers::abilities(st, id)` and `st.catalog.ability(id)`.
- A rule that holds for a while on no permanent ("can't be blocked this turn", "prevent all
  combat damage this turn") is an `effects::Lasting`, never an ability given to the
  creature (CR 113.12). Whatever reads abilities of permanents for combat rules or
  replacement effects reads `effects::lasting_abilities(st)` too.
- A rule about a player ("no maximum hand size", extra land plays, "you have hexproof",
  "play with the top card of your library revealed", "you may play lands from your
  graveyard") is an `AbilityDef::PlayerRule`, read through `engine_core::players` (D-070),
  never a flag written into `GameState`.
- A card with several faces (D-071) is one `CardDef` with `faces` and a `Layout`; each face
  is also a catalog entry (`catalog.face(card, n)`, `CardDef::face_of`). In every zone but
  the battlefield and the stack a card has its front face's characteristics (CR 712.8a);
  which face a permanent or spell shows is `GameObject::face`, read through
  `layers::chars` / `chars_as`. A permission to play or cast from a zone is judged by the
  face played (`players::may_play_face_from`, CR 601.3e). Tests: `forge_faces.rs`.
- "Those cards" (D-069): what an effect just moved is remembered only within the chain
  that remembers it and forgets it as its last link (`Effect::Note`, `ObjRef::Noted`,
  `Selector::noted`, `Amount::Noted*`, `PlayerRef::NotedControllers`); this is not
  `Affected/ObjFilter::Remembered`. `Effect::If` is a condition counted when the instruction
  is reached; `Effect::IfDone` is "if you do". Tests: `forge_remember.rs`.
- An alternative cost (CR 118.9: "rather than pay", "without paying its mana cost",
  flashback, evoke, overload) is `AbilityDef::AlternativeCost`, chosen as
  `OptionDesc::CastWith { card, face, ability }` and turned into an ordinary cost before
  payment (`frames/altcost.rs`, D-072); never a second payment path. Tests: `forge_altcost.rs`.
- Effects that change how much (tokens, counters, damage, life) are
  `ReplaceWith::Scale/AddTokens/ReplaceTokens` on `Replaces::Tokens/Counters/Damage/GainLife`
  (D-073). A created token enters through `Proposed::TokenEnters`, one per token, and is
  modified like any permanent entering (CR 111.2, 614.12); "as it would exist on the
  battlefield" is `replacement::as_it_would_exist`. "Triggers an additional time" is
  `AbilityDef::AdditionalTrigger`, counted in `triggers::queue_caused` (CR 603.2d); "can't be
  countered" is `AbilityDef::CantBeCountered`. One event creates at most
  `MAX_TOKENS_AT_ONCE` = 10,000 tokens of a kind (a deviation from the CR, silent for now).
  Tests: `forge_doublers.rs`.
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
cargo run -p harness -- play [--setup cmd-4p] [--seed N]  # you (P0) vs random agents in the terminal
cargo run --release -p harness -- fuzz --setup cmd-4p --agents h-vs-r --games 200   # AI strength
cargo run -p harness -- run --seed 42 --save-replay r.json && cargo run -p harness -- replay r.json
cargo run -p harness -- bless                             # ONLY when a hash change is intended; review why
cargo run --release -p app                                # the desktop app (you are P0); downloads card images into ~/.cache/apnap
APNAP_SHOT_DIR=/tmp/shots cargo test -p app --test screenshot -- --ignored   # render the UI to PNGs (APNAP_SHOT_SETUP="<menu label>", APNAP_SHOT_SIZE=2560x1440)
cargo run --release -p harness -- fuzz --setup real-4p --games 1000 --threads 8      # real-card decks
cargo run --release -p harness -- fuzz --setup precon-4p --games 200 --threads 8     # the four precons, as far as they compile (D-053); first entry in the app's menu
cargo run -p harness -- fetch-cards                       # printed data for the precons + basics -> ~/.cache/apnap (tests need it)
cargo run -p harness -- coverage [--verbose]              # which precon cards are scripted/verified
cargo run --release -p harness -- fetch-forge             # Forge's card scripts (pinned commit) -> ~/.cache/apnap/forge (git, network)
cargo run --release -p harness -- forge-coverage [--top N] [--families] [--sample N [--with TEXT]] [--why TEXT [--all]]   # how many of ALL cards compile; what blocks the rest (--families: by kind of feature); --why lists the cards behind a blocker
cargo run --release -p harness -- forge-coverage --top 0 --ranked [N]   # compile rate by EDHREC rank tiers; blockers and most played blocked cards in the top N (default 1000); needs the bulk file (fetch-bulk)
cargo run --release -p harness -- fuzz --setup forge-4p --games 400 --threads 8      # random decks drawn from EVERY compiled card (finds what precon decks never reach)
APNAP_TRACE=1 cargo run --release -p harness -- run --seed N --setup forge-4p        # a game that doesn't end: where it is, every 2,000 decisions
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
- `crates/cards` — `data/` (self-contained: invented "Test …" cards, tokens, basics),
  `scripts/` (real cards: behaviour only, D-035), `decks/*.ron`. Embedded by `build.rs`.
- `crates/card-db/src/forge/` — `parse.rs` (lossless reader of Forge scripts),
  `translate.rs` (the compiler; `Blockers`), `mod.rs` (pinned commit, fetch, paths).
- `crates/card-db` — parses + validates RON into a `CardCatalog` (strict mode rejects
  unverified cards; every verified card must name existing tests); `scryfall.rs` (runtime
  fetch + cache), `real.rs` (script + printed data -> CardDef).
- `crates/ai` — agents: RandomAgent, HeuristicAgent (D-033). `crates/harness` — CLI, `Setup`
  (m0 / m1-Np / m2-Np / cmd-Np / real-Np = decks of verified real cards, D-038 /
  precon-Np = the precons minus the cards that don't compile yet, D-053),
  `--agents random|heuristic|h-vs-r`,
  `script::Script` test driver, determinism/golden/property/rules tests.
- `crates/app` — the desktop app: `session.rs` (game + AI seats + stops + save/replay, no
  graphics; `declaration`/`declare` = a whole attack or block), `ui.rs` (egui view: the
  table grid, decisions, pacing, the enlarged card next to the pointer), `combat.rs`
  (planning attacks/blocks on the table, D-040), `cardview.rs` (card widget: image or
  drawn proxy, badges, motion, arrows, mana-symbol text),
  `art.rs` (Scryfall images/symbols: worker threads + cache, one texture per exact pixel
  size; tokens pictured by Scryfall's token cards; `Art::disabled()` in tests),
  `session.rs` also answers for the human where MTGO would (stops, own spell on top of
  the stack, F4/F6 yields: D-054), tests driven headlessly with
  egui_kittest (D-036, D-037). `card-db/src/images.rs` = image/symbol paths + download.
- `crates/xtask` — check-deps.
- Engine extras: `stack.rs` (StackObject), `targeting.rs` (choose + 608.2b legality in one
  place), `frames/cast.rs` (601.2/602.2, and putting triggers on the stack), `frames/resolve.rs`
  (work list; last known information about targets in `TargetInfo`, D-068), `replacement.rs` + `frames/apply.rs` (614–616 pipeline), `triggers.rs` +
  `frames/put_triggers.rs` (603), `frames/check.rs` (117.5/704.3), `effects.rs` (resolved
  continuous effects, shields), `frames/mana.rs` (mana abilities: what a permanent can make, their costs,
  the payment planner; D-066), `players.rs` (rules about players: hand size, land plays,
  hexproof, the top card of the library, playing from other zones; D-070), `testing.rs` (feature `test-support`: scenario builder, D-022).

## Direction (2026-11-09, D-046): every real card, by compiling Forge's scripts
The user's goal is "every non-silver bordered card ever printed, for the most part,
eventually". Forge scripts 33,248 of Scryfall's 33,499 real cards with ~760 top-level
features (184 effect APIs, 131 trigger modes, 105 static modes, 35 replacement events, 243
keywords, 41 cost kinds, a filter language, an expression language). `card_db::forge`
compiles those scripts into `CardDef`s. **Work loop:** run `harness forge-coverage`, take
the blocker that alone stops the most cards, read what it means in `docs/forge/` (and
Forge's Java under `~/.cache/apnap/forge/repo` — sparse-checkout more of it as needed),
extend the vocabulary/engine *generally* (not for one card), add its test, re-run. Read a
`--sample` of compiled cards against their Oracle text after every step: it is how
mis-compilations were found. The 90 hand-scripted cards are the regression set
(`tests/forge_compile.rs`: the compiler must reproduce each of them).
Status (D-073): 10,914 of 34,463 scripts compile (31.7%). Since 2026-11-10 the order of
work is EDHREC popularity (the user's wish), not raw count; measured after D-073: 90% of
the 100 most played Commander cards compile, 58% of the top 1,000, 35% of the top 5,000
(`forge-coverage --top 0 --ranked`, which joins `edhrec_rank` from the Scryfall bulk file). Precons: `harness coverage --verbose` lists what blocks
the rest. Feature tests live in `crates/harness/tests/forge_features.rs` and, one file per
feature area since D-066, `forge_<area>.rs` next to it: invented cards written
in Forge's format, compiled by `harness::forge_test_catalog` and played (`forge_test_blockers`
for "this must not compile"). Four guards catch compiler holes — keep them: a line that
yields nothing without a named blocker is `unexplained:*`; every compiled card goes through
`card_db::validate` (`invalid:*`); the compiled card is read back against its Oracle text
for words Forge scripts leave out or add (`oracle:*`: "may", "another target", a target
narrowed to "you control", "unless"); and the `forge-Np` soak plays random decks of all of
them. A script found by reading to differ from its card goes into `SCRIPT_DIFFERS` in
`translate.rs` (`script:differs from the card`). A `#` line in a script is a comment, as in
Forge, but only reviewed comment texts are ignored (`forge/comments.rs`, D-078: `HARMLESS`;
`INCOMPLETE` ones block the card; anything else is `comment:unreviewed`). A variable Forge
fills in at run time (`AFLifeLost`) is never read as its placeholder value
(`expr:set by an effect …`, `SET_WHILE_RESOLVING`); `AFLifeLost` right after a life loss is
`Amount::LifeLostThisWay` (D-075).
References written from Forge's sources: `docs/forge/script-format.md` (file format, costs,
filters, `Defined$`, expressions), `effects.md` (every effect API), and
`triggers-statics-replacements-keywords.md`. Read the relevant section before compiling a
feature: Forge's defaults are often not what the key names suggest (flags are tested for
presence, `Defined$ Targeted` means every target of the chain, a trigger that runs an
`AB$` with a cost is "you may pay", triggers without `TriggerZones$` work from any zone).
Not built yet: the full generated-text-vs-Oracle round trip (a word-level guard exists),
flip, meld, prepare and specialize cards, Fuse, Aftermath, Rooms, day/night, disturb,
craft, "enters/cast transformed" and "when this transforms" triggers (modal double-faced,
transforming, Adventure, Omen and split cards are built, D-071), searching another player's library, "X target creatures", changing a spell's targets, delve/convoke/improvise, flashback
given to other cards, Channel cost reductions, draw and mana replacement effects, effects that change life
loss, memory that outlives a resolution (effect objects remembering
cards, linked abilities per CR 607, delayed triggers), counters other than +1/+1 and
-1/-1, "as this enters, choose …" (how a permanent enters is otherwise built, D-067:
`ReplaceWith::EntersTappedUnless…`, the choice asked inside `ApplyFrame` as
`ChoiceKind::AsEnters`), "sacrifice … instead" lands, "lands enter untapped", "exile it
instead. When you do, …", regeneration, abilities given to cards that aren't on
the battlefield, "becomes tapped" and "becomes the target" triggers, protection, Sagas,
and of Forge's "Effect" objects everything but combat rules and "prevent all damage"
(`effects::Lasting`, D-065: no triggers, no characteristic changes, nothing about
players yet). `forge-coverage --families` ranks what is left by kind of feature.

## Current milestone: M4 — IN PROGRESS (started 2026-11-09), on `main` (`m4-wip` merged 2026-11-10)
Decisions: D-034 (precons: Calling All Angels, Wretched Ranks, Reign of Dragons,
Tramplesaurus Rex), D-035 (real-card pipeline; Scryfall data only in the user's cache),
D-037 (card images, mana symbols, proxies, AI pacing), D-038 (subtype selectors; `real-Np`
interim decks), D-040 (combat planned on the table, players in a grid, one main button),
D-041 (card batch 2: mass effects, permanent targets, exile, tokens, attack triggers).
Tokens are named per CR 111.4 ("Zombie Token") and referenced from scripts by `key`.
Order: groundwork (DONE: frozen decklists, `harness fetch-cards`, `harness coverage`,
real-card script format) → egui app (DONE: D-036; mouse-only, save/load/replay; card
images + mana symbols + paced AI turns: D-037; Arena-style combat + table grid: D-040) → cards deck by deck until `harness
coverage` is 100% (IN PROGRESS: 88 of 254 real cards verified; three of the four
commanders (Lathliss, Ghalta, and Giada/Gisa still missing); playable now as the three
`real-Np` decks; batches logged as D-038, D-041..D-045. NEXT,
in order of cards unlocked: costs that need a choice (sacrifice another creature, discard,
pay life) and additional costs on spells, with Treasure/Clue tokens; "may"; damage and combat-damage triggers; hexproof/indestructible/ward; multi-mana
abilities (payment feasibility rework, D-021); graveyard targets; equipment; cycling and
flashback; monarch; the one-off cards. Each card needs a script, a test, and never an
approximation) → precon setups in the app menu, 10,000-game soak.
Real-card scripts hold behaviour only: no Oracle text in them, not even in comments.
Real-card work needs `cargo run -p harness -- fetch-cards` once (network).
Acceptance (spec §13 M4): coverage 100% verified, each card >= 1 test, 10,000 AI 4-player
games clean, the app playable by mouse with a widget per ChoiceKind, save/load/replay in app.

### Future scope (not M4)
The user wants 1v1 Modern, Legacy, Vintage etc. later (D-039): formats are configuration
(GameConfig + per-format deck legality in card-db), never engine forks; don't hard-code
Commander or the precon pool anywhere new.

## Previous milestone: M3 — DONE (2026-11-09).
M3 delivered: combat keywords (first/double strike, trample + deathtouch, lifelink, vigilance,
menace; D-031), Auras + layer-2 control with summoning-sickness tracking (D-029/D-030),
Commander (903: command zone, tax, commander damage, 903.9a SBA choice, 903.9b optional
replacement, color identity + deck validation; D-032), heuristic AI v1 (D-033).
Tests: `rules_m3.rs` (spec's three Commander/800.4 golden cases + keyword/Aura/903 tests),
`ai_m3.rs` (AI never takes avoidable lethal commander damage — property test, mutation-checked;
AI beats random agents), determinism over Commander setups; CI soaks cmd-4p.
M4 starts with: pick four real precons, Scryfall import (card-db, XDG cache), RON scripts for
every card in them, the egui app.

### M2 summary
Layers (613), replacement/prevention (614–616), triggers + LKI (603), state check (704);
decisions D-023–D-028.

### M1 summary
M1 delivered priority/casting/abilities/resolution, 2–8 players with CR 800.4a/j/k, RON
cards via card-db, the scenario builder; decisions D-018–D-022, limits D-021.

### M0 summary
M0 delivered: workspace, CI, ids/zones/GameState, move_object, seeded RNG, step() with
PendingDecision suspension, serde round-trip, harness with random agents; all three M0
acceptance criteria tested (`crates/harness/tests/determinism.rs`).
Known M0 simplifications to remove later: see D-010.
Spec discrepancies already found: see the end of `docs/decisions.md`.
