# Design decisions

Newest last. Each entry: decision, why, and where it lives. "Spec" = `docs/spec.md`.
CR = Comprehensive Rules effective 2026-11-06.

## D-001 Workspace layout (M0, 2026-11-08)
Crate names follow spec §4 (`engine-core`, `card-model`, `card-db`, `cards`, `ai`, `harness`,
`app`) at the repo root (spec draws `mtg/`). Added `xtask` (repo automation, `cargo xtask …`
alias in `.cargo/config.toml`). Toolchain in `rust-toolchain.toml` (see D-017), edition 2024, MSRV 1.99.
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

## D-009 The stack is `GameState::stack: Vec<StackObject>` (M0, revised M1)
M0 kept `zones.stack: Vec<ObjectId>`. **M1:** the stack is `GameState::stack` (top = last),
the single source of truth for stack order. A `StackObject` records controller, kind
(`Spell` = the card object with the same id, or `Ability { source, source_card, index }`),
chosen modes, X and targets. Activated abilities on the stack are objects that are *not
cards* (CR 602.2a), so they live only here, not in `GameState::objects`, which is exactly
the distinction CR 800.4a needs. The spec's `source_lki`/`trigger_event` arrive with M2.

## D-010 Temporary M0 simplifications — **all removed in M1** except where noted
*M1 status:* mana is now exact for multi-type sources and the player chooses which floating
mana pays generic (D-021); 2–8 players are supported with CR 800.4a (D-018); instants,
flash, activated and mana abilities exist. "Blockers block exactly one attacker" remains
true because no card yet lets a creature block more.

Original M0 list:
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

## D-017 Track the `stable` toolchain instead of pinning 1.99.1 (2026-11-08)
Pinning an exact version broke rust-analyzer in the user's editor: it runs cargo with
`RUSTUP_AUTO_INSTALL=0`, and only `stable` was installed there. Determinism doesn't depend
on the compiler (D-006, D-007, and the golden test would catch it if it did), so the pin was
only protecting against new clippy lints failing `-D warnings`. Accepted cost: an occasional
small lint-fix commit when a new stable comes out. `rust-version = "1.99"` remains as the MSRV.

## D-018 CR 800.4 (leaving the game) pulled forward from M3 into M1 (user decision 2026-11-08)
M1's acceptance needs 1,000 four-player games, which eliminate players. Implemented as the
`GameAction::LeaveGame` action in the order 800.4a gives: owned objects leave the game, then
abilities the player controls on the stack cease to exist, then anything still controlled
is exiled. A multiplayer loss triggers it right after the SBA batch; a two-player loss just
ends the game (104.2a). **800.4j** (missed by the spec): a turn whose active player left
continues without one; "the active player would receive priority" goes to the next player
(`GameState::first_priority_player`, and the priority frame redirects any departed player).
800.4k: turn order skips them. Control-change cases (800.4a "control effects end", 800.4b–c)
have no M1 cards; M3 adds the spec's aura/control golden tests.

## D-019 No rewinds: the engine only offers completable choices (M1)
CR 601.2 / 602.2 say an uncompletable cast "returns to the moment before" (CR 734). apnap
never needs that: `can_begin` offers a cast/activation only if a full legal sequence
exists; each later step offers only choices that keep completion possible (legal modes,
X bounded by what's payable, existing targets, mana abilities that keep the cost payable).
So every shape-valid response completes the cast, and there is no snapshot/rollback
machinery in the engine. A UI "cancel" is a UI concern (the app can clone the state before a
cast). Revisit if a future card makes completability expensive to decide.

## D-020 Resolution is a resumable work list (M1; user decision 2026-11-08)
`ResolveFrame` holds the remaining instructions as `Vec<Work>` (CR 608.2c "in the order
written"). Each item performs one action or suspends (today: APNAP each-player choices,
CR 101.4, which collect every player's choice first and then act simultaneously). M2's
replacement pipeline will add a "pending action" item so a choice *inside* one action (e.g.
two replacement effects on a draw) suspends and resumes the same way — no callbacks.

## D-021 M1 mana-ability and casting limits
- Every mana ability costs exactly `{T}` and makes one mana (card-db rejects others), so one
  untapped source = one mana and payment feasibility is an exact bipartite matching
  (`mana.rs`). Abilities like Sol Ring ({T}: add {C}{C}) or filters need a generalized check.
- The engine asks which floating mana pays generic (`Allocate` over mana types) only when
  that is ambiguous; payment completes automatically as soon as the pool covers the cost
  (players can still float extra mana with priority).
- Targets: exactly one object or player per target slot; no "up to N", no divided effects
  (601.2d), no targeted permanent spells (Auras). X appears only in mana costs.
- No LKI cache yet: an ability whose source left the battlefield uses the source's id as
  the damage source (enough for M1, where nothing reads the source's characteristics).
  M2 adds LKI with triggers (spec §8.2).

## D-022 Test-only scenario builder (`engine-core` feature `test-support`) (M1)
Atomic rules tests (spec §10.1) build positions directly with `testing::Scenario` (place
named cards in zones, set life/turn/step) and then play normally through `step`. Building
writes `GameState` without `actions::execute`: the one sanctioned exception to invariant 2,
because it constructs a position rather than playing to it. The feature is enabled only by
harness dev-dependencies; CI still builds engine-core `no_std` with and without it.

## D-023 M2 scope: the layer walk has every slot; effect vocabulary for layers 4–7 (user decision 2026-11-08)
`layers.rs` walks 1a, 1b, 2, 3, 4, 5, 6, 7a, 7b, 7c, 7d with timestamps (613.7), the 613.8
dependency algorithm and 613.6 locking. Effect kinds exist for layers 4 (add types), 5 (set
colors), 6 (add/lose keywords, lose all abilities), 7b (set P/T), 7c (modify P/T + counters)
and 7d (switch). Copy (1a), face-down (1b), text-changing (3) and control-changing (2) effects
are added to the same walk when a card needs them (control with Auras in M3). Likewise SBAs:
M2 adds 704.5d, h, j, q; 704.5i (planeswalkers) and 704.5m–p (attachments) come with those
card types (user decision). No characteristic-defining abilities exist yet, so 613.3 / 613.8a(c)
have nothing to order.

## D-024 Replaceable events are proposals run through an `Apply` frame (M2)
Every replaceable game event — zone moves (destroy, sacrifice, discard, dying, resolving,
countering, land plays, entering the battlefield), damage, draws, token creation, counters,
life gain/loss, and SBA batches — is built as a `replacement::Proposed` and pushed as
`Frame::Apply(ApplyFrame)` above the frame that caused it. The frame finds applicable
replacement/prevention effects (static abilities the source *currently* has, the entering
object's own "enters …" effects, prevention shields), applies a single one directly, asks
the affected player (`ChooseReplacement`) when several apply, records each applied effect
so it can't apply twice (614.5), re-checks after every application (616.1f/616.2), and then
performs the whole batch simultaneously. Events are grouped by affected player in APNAP
order (616.1). The caller resumes afterwards (D-020), so choices inside an action need no
callbacks. Not replaceable (direct `execute`): casting's move to the stack, mulligan/library
manipulation, untapping, mana, leaving the game.

## D-025 Triggers: detected from performed events; put on the stack by the state check (M2)
`triggers.rs` detects triggers only from events the `Apply` batch actually performed
(603.2g): enters-the-battlefield after the whole batch, against every permanent including
newcomers (603.6a); "dies" looking back in time with last-known information for the dying
object *and* any other object that left in the same batch (603.10a); "at the beginning of"
when a step begins (603.2b); "whenever you cast" at 601.2i. Intervening-if is checked on
detection and again on resolution (603.4). `StateCheckFrame` implements 117.5/704.3: SBAs as
one batch, repeat; then `PutTriggersFrame` (603.3b: each player in APNAP order orders their
own with `OrderTriggers`; each trigger then goes through `CastFrame`'s modes/targets steps
and is removed if it has no legal choice, 603.3c–d); then check again. The 603.3b second part
(triggers on abilities triggering) has no card yet. The trigger queue is engine bookkeeping;
each queued trigger emits `Event::Triggered`, so it is part of the hashed log.

## D-026 Last known information is captured per batch and stored where it's needed (M2)
Before an `Apply` batch performs, it snapshots (`triggers::Lki`) every object about to leave
the battlefield, from the pre-batch state (704.8). Those snapshots feed look-back trigger
detection, are copied into any pending trigger whose source left, and are recorded on any
ability on the stack whose source is leaving (`StackObject::source_lki`, 113.7a). There is no
global LKI cache to prune. "Its power" (`Amount::SourcePower`) reads the source's current
characteristics if it's still on the battlefield, otherwise its LKI (608.2h).

## D-027 Flying and reach blocking pulled into M2 (2026-11-08)
M2's layer-6 tests need a real keyword, and a test card with flying that ignored blocking
would be an approximated card (invariant 8). 702.9b/702.17 blocking restrictions are in
`DeclareBlockersFrame`. The rest of combat keywords (first/double strike, trample, lifelink,
menace, vigilance, and 702.2c deathtouch-as-lethal in assignment) stay in M3.

## D-028 M2 vocabulary limits (to lift when a card needs them)
- Replacement effects are mandatory; no optional ("you may … instead") ones, no 614.15
  self-replacement effects, no 614.17 "can't" effects, so the 616.1a–d tiers are trivially
  satisfied. Regeneration (614.8) and redirection (614.9) aren't in the vocabulary.
- 614.12 "as it would exist on the battlefield" uses the entering object's *own* printed
  replacement abilities plus permanents' current abilities; continuous effects that would
  start applying to the entering object (e.g. "creatures you control enter tapped" defined
  by type) aren't consulted yet.
- Selectors match card types, supertypes, controller, "other" and token status (no colors,
  names or subtypes yet).

## D-029 Auras (M3): "Enchant creature" only
An Aura card is an Enchantment with subtype "Aura" and an `Enchant(filter)` ability (CR 702.5);
card-db accepts only `Enchant(Creature)` for now. Casting targets per the enchant ability
(303.4a); on resolution an illegal target means it goes to the graveyard (608.3b), otherwise
it enters attached (`Proposed::Move::attach_to`). `Affected::Enchanted` (303.4m) scopes its
static abilities. SBA 704.5m removes Auras attached to nothing or to an illegal object.
Equipment, Auras entering other than by resolving (303.4f), "enchant player" and 704.5n–p
wait for cards that need them.

## D-030 Layer 2 control and summoning sickness (M3)
Control is computed in the layer walk (`Characteristics::controller`, `ContinuousEffect::
GainControl`); `GameObject::controller` is the *base* controller (who it entered/was cast
under). All rules reads use `layers::controller_of`. The driver calls `layers::sync_control`
after every frame step: when a permanent's layered controller differs from the last one seen,
a `ControlChanged` action records it and marks it summoning sick (CR 302.6). So control
effects end automatically when their source leaves (800.4a), and leaving players are
evaluated after their objects' control effects end.

## D-031 Combat keywords (M3)
- First/double strike: `CombatState` records whether a second damage step is needed and which
  creatures had first/double strike as the first step began (CR 510.4).
- Trample: two decisions — `PickNumber` for the damage to the player (0..=power − Σ lethal),
  then `Allocate` among blockers with per-blocker minimum = lethal when anything goes to the
  player. "Lethal" counts marked damage, damage assigned earlier in the step, and deathtouch
  (702.19b, 702.2c). The decomposition is exact and keeps D-019 (no dead ends).
- Menace: an illegal declaration is detected after a defender's last block decision and that
  defender declares again (CR 734 return-to-before; nothing has happened yet). A
  deterministic agent that repeats an illegal declaration would loop; the heuristic agent's
  plan is always menace-legal and the harness caps decisions.
- Lifelink: the life gain is performed in the same damage event (702.15b).

## D-032 Commander (M3)
- Commanders are designated per card instance (903.3) at `new_game_with_commanders` and start
  in the command zone (903.6). Commander tax is locked in when the cast is announced (601.2f),
  so it's included in every feasibility check (D-019). Casts from the command zone are
  counted per card.
- Commander damage is tallied in the `Damage` action (post-replacement), keyed by card;
  704.6c is an SBA.
- 903.9a: commanders that moved to graveyard/exile are remembered until the next SBA check;
  the owner gets an Accept/Decline `CommanderToCommandZone` choice inside the state check.
- 903.9b: a rule-based *optional* replacement (`RepKey::CommanderReturn`): the affected player
  is always asked (Accept/Decline, `ChooseReplacement`), even when it's the only applicable
  effect; declining records it so it isn't asked again for that event. (Accepting moves the
  card to the command zone, so the "may apply more than once" clause never needs a repeat.)
- Deck rules (903.4 color identity, 903.5a–d) are validated in card-db, not the engine.
  The four M3 commander decks were generated from the card pool once and committed; the two
  mono-colored decks are land-heavy because the pool has few mono-colored cards.

## D-033 Heuristic AI v1 (M3, spec §9 Phase A)
`ai::HeuristicAgent` sees only its `PlayerView` plus the card catalog (card text is public
information, not `GameState`). Integer scoring only. Plans blocks once per combat: first
prevent lethal commander damage, then lethal life loss (trample-aware chumps), then value
blocks; plans are menace- and flying-legal. Attacks are "safe" attacks toward the
highest-threat opponent, with big bonuses for lethal life or commander damage; keeps a blocker
home under crackback threat. Spells are classified harmful/helpful from their effects and
targeted accordingly (killable creatures first; face when lethal). Not yet: politics/grudges,
combat tricks, holding up counters, mana planning. It wins 199/200 four-player Commander
games against three random agents; the property test over generated boards shows it never
takes avoidable lethal commander damage.

## D-034 M4 precons: the four mono-colored FDC decks (user decision 2026-11-09)
Calling All Angels (W, Giada, Font of Hope), Wretched Ranks (B, Ghoulcaller Gisa), Reign of
Dragons (R, Lathliss, Dragon Queen), Tramplesaurus Rex (G, Ghalta, Primal Hunger); released
2026-10-02. Chosen from a measured comparison of 25 precons (MTGJSON decklists + rules
text): 254 unique nonbasic cards, no planeswalkers, no vehicles, one Saga, one library
search. Decklists (names and counts only) are frozen in `crates/cards/decks/precon_*.ron`.
M4 order (user decision): groundwork → egui app → cards deck by deck.

## D-035 Real cards: behaviour-only scripts + printed data fetched at runtime (user decision 2026-11-09)
- `crates/cards/scripts/<letter>/<slug>.ron` holds a `RealCardScript`: oracle id, name,
  verified/tests, and *our encoding* of keywords and abilities. No rules text, no printed
  characteristics. `card-db::real::compile` joins it with the card's Scryfall data (mana
  cost, type line, P/T, colors) into a `CardDef`, and refuses anything it can't represent
  exactly (hybrid mana, `*` P/T, multi-faced layouts) — never approximate (invariant 8). A
  script keyword that Scryfall doesn't list for the card is an error.
- Printed data is fetched only at runtime into `$APNAP_CACHE_DIR` or the platform cache dir
  (`~/.cache/apnap/scryfall/cards.json`), via Scryfall's batched `/cards/collection` endpoint
  (75 names/request, paused, identified User-Agent), only for the cards the deck pool needs,
  and never re-fetched once cached. Spec §12 suggested the bulk file + a bincode `oracle.bin`;
  for ~254 cards four small requests and a JSON cache are simpler and kinder to Scryfall.
  Revisit when the pool grows.
- Real-card tests and CI need the cache: `cargo run -p harness -- fetch-cards`. Without it
  they fail with that hint. Tests of invented cards stay fully offline.
- The self-contained catalog (invented cards, tokens, basic lands) keeps its ids; real
  cards are appended after it (`load_full_catalog`), so golden hashes of non-precon games
  don't depend on Scryfall data.
- `crates/cards` embeds its files through `build.rs` (sorted paths), replacing the
  hand-maintained list.
- `harness coverage` is the spec §10.6 report.

## D-036 The desktop app (M4): session layer + egui view, tested headlessly
- `crates/app` is a library plus a thin binary. `session.rs` owns the `GameState`, the AI
  seats, the response log and the *stops*; it has no graphics. `ui.rs` is one function
  `ui(&mut App, &mut egui::Ui)` drawing from the human's `PlayerView` (spec §11). The AI
  runs synchronously in the session for now (the heuristic agent is instant); worker
  threads come with search AIs in M5.
- **Stops are a session/UI concern** (spec §6): the engine always asks; the session answers
  for the human when a decision has only one possible answer, when priority offers nothing
  but passing or mana, when the stack is empty outside the configured stop steps, or (with
  auto-pay on) for mana payment. "Pass until end of turn" is a session flag.
- **Every decision is mouse-only**: each option is a button in the decision panel (single
  pick, multi-select + Confirm, number with -/+, Allocate with per-recipient -/+, Order with
  Up/Down), and options attached to a card or player can be chosen by clicking it on the
  board (an object with several options focuses the panel on them). `prompt(ChoiceKind)` is
  an exhaustive match, so a new choice kind can't be forgotten.
- **Save = replay file** (setup, seed, every response). Loading replays it, asking each AI
  agent its recorded decisions again so agent state matches; "Watch replay" steps through
  the same file. Saves live in the platform data dir (`~/.local/share/apnap/saves`).
- **Card text is generated** from ability definitions (`card_model::describe`), so the text
  a player reads is what the engine does. Cards are text frames; images come with the real
  cards (lazy Scryfall download into the cache).
- **Tested without a window** via `egui_kittest`: a robot plays whole games by clicking
  widgets only (`tests/robot.rs`), targeted widget tests cover Allocate/Order/multi-select
  and board clicks (`tests/widgets.rs`), and an ignored test renders PNG screenshots. The
  harness's accessibility and missing-glyph checks stay on.
- `app` depends on `harness` (setups, agents, narrator, replay format): added to the allowed
  crate edges. The spec's layout had no such edge; the alternative was duplicating them.

---

## D-037 Card images, mana symbols and pacing in the app (user request 2026-11-09)
The user found the text-only board "very hard to read" and asked for real card art, mana
symbols and the normal Scryfall card image.
- **Where the files come from.** The printed-data cache (`cards.json`, D-035) now also
  stores each printing's Scryfall id, `normal` and `art_crop` image addresses, artist, set
  and collector number; entries cached before that are fetched again automatically. Images
  are downloaded on demand from `cards.scryfall.io` into
  `<cache>/scryfall/images/normal/<printing id>.jpg`, symbols from `svgs.scryfall.io` into
  `<cache>/scryfall/symbols/<CODE>.svg` (`card_db::images`). Nothing is committed
  (invariant 13); each file is downloaded once; only those two hosts are ever contacted for
  files (the addresses come out of the cache file, so they are checked, not trusted).
- **Who downloads.** The app (`app::art`), on three worker threads, never the UI thread and
  never the engine. It also fetches the printed data itself if the cache is missing, so
  `harness fetch-cards` is no longer a prerequisite for *running the app* (it still is for
  tests). The menu has a checkbox to turn images (and with them all network use) off.
  Tests construct the app with `Art::disabled()`: no threads, no cache, no network.
- **What is drawn.** A real card is its whole Scryfall image, scaled, never cropped or
  recoloured, turned a quarter when tapped (Scryfall's image guidelines). Pointing at a
  card shows it enlarged **right next to the pointer** (beside the card, above it for cards
  in the hand; never over the card itself), at the image's own resolution, with the artist
  credit and the object's current state — the user asked for this instead of a preview in
  the side panel (2026-11-09). What the printed card can't show — power/toughness
  **only when it differs from the printed numbers** (user request: the card already shows
  the rest), damage, counters, attacking/blocking, summoning sickness, commander,
  attachment, piles of identical lands — is drawn as small corner badges that stay clear of
  the artist/copyright line. A card without an image (the invented test cards, tokens,
  anything still loading, offline) is drawn from its `CardDef` as a plain "proxy" in the
  same shape, with rules text from `card_model::describe` — so the fallback is still what
  the engine does.
- **Sharpness.** A texture is made for the exact number of screen pixels a card (or
  symbol) is drawn at: card sizes are whole pixels, the worker scales the image once with a
  Lanczos filter, and it is painted one texture pixel per screen pixel. (The first version
  used one 320 px texture for every size and let the GPU rescale it, which the user found
  unreadable on a 1440p monitor.) Sizes above 488 px use Scryfall's `large` image.
  Textures nothing has drawn for a while are dropped. Board cards are as tall as the
  window allows without making a row wrap (up to 380 points), so a big monitor gets big,
  readable cards.
- **New dependencies (app only):** `image` (JPEG/PNG decode) and `resvg` (SVG symbols).
- **Pacing.** The session used to run every AI decision in one frame, so a human with
  nothing to do saw their own turns back to back with no sign of the opponents' turns in
  between (reported as "it let me play three lands on turn 1" — the engine did enforce one
  land per turn, CR 305.2; the turns were just invisible). `Session::pump_visible` now stops
  after each AI decision that adds to the log, and the UI waits (Instant / Fast / Normal /
  Slow, default Normal = 450 ms) before continuing. This is presentation only: the same
  responses are applied in the same order (`tests/pacing.rs` checks paced and unpaced games
  are identical). The wall clock lives in `ui.rs`, not in the session or the engine.

## D-038 Subtype selectors, and interim decks of verified real cards (M4)
- `Selector.subtypes` ("Dinosaurs", "Dragon creatures", "another Angel you control"): the
  object must have at least one of the listed subtypes, read from its current
  characteristics (so type-changing effects in layer 4 are seen by layer 7 effects and by
  triggers through last known information). Non-land subtypes only for now.
- 24 more precon cards are scripted and verified — the ones today's vocabulary expresses
  **exactly** (invariant 8). Cards that would need an approximation were left out, e.g.
  Secret Rendezvous ("you and target opponent each draw": CR 121.2 orders those draws by
  turn order, which `Seq` can't say) and Vengeful Dead ("this creature or another Zombie").
- `Setup::Real { players }` (`real-Np`): two 60-card decks built only from verified real
  cards (`decks/real_stompy.ron`, `decks/real_angels.ron`), alternating by seat. They exist
  so the app can be played with real cards and art before a precon reaches 100% coverage;
  they are not a format and will be dropped from the menu's top spot when the precons are
  playable. They need the Scryfall cache, so they are not in the golden-hash file (printed
  data can change); determinism, save/load/clone, property tests and CI soaks cover them.
- `Agents::make` takes the game's catalog (the heuristic agent reads card text from it; the
  real-card catalog is larger than the self-contained one).
- Golden re-bless: only `state_hash` changed (the catalog is part of the state: `Selector`
  gained a field and `Test Wyrmling` was added); every `log_hash` is unchanged.

## D-040 Playing on the table: combat planning, the grid, one main button (user request 2026-11-09)
The user asked for play that is "more dynamic so everything isn't just clicking on a menu",
named Arena's combat as the model, and asked for the four players in a grid.
- **Combat is planned on the table and confirmed once.** The engine still asks about one
  creature at a time (its protocol since M0; CR 508.1/509.1 declare as a whole). The app
  finds out the whole declaration up front — `Session::declaration` answers "no" for each
  creature on a *copy* of the game and records who is asked and what they may do — and the
  human plans it in `combat::Plan`: click a creature to attack (click it again to stand it
  down), click a player's name to choose who new attackers go at, click a blocker and then
  its attacker, or drag a creature onto a player or an attacker; "All attack", "Clear", and
  one button to confirm. `Session::declare` then gives the engine exactly those answers,
  creature by creature. Nothing is sent before the confirmation, a plan can only hold
  options the engine offered, and if the engine sends a block back as illegal (menace) the
  app says why and the block is planned again. The look-ahead reveals nothing hidden (the
  human's own creatures and public combat state) and never touches the real game.
- **What it looks like.** Attackers step towards the middle of the table and blockers half
  as far (declared or still planned); arrows show who blocks what and, when there is more
  than one player to attack, who is attacked; a dragged creature trails an arrow. Cards
  turn smoothly when they tap. Animation is off in tests (`App::animate`).
- **The table.** Players sit clockwise in turn order: the viewer bottom left, the others
  along the top and back along the bottom (`table_layout`; four players = a 2 x 2 grid, two
  face each other, three put both opponents on top). Every seat has a row of lands at the
  far edge and a row of everything else towards the middle. Card size is the largest at
  which the whole table fits, letting rows wrap when that allows bigger cards
  (`fit_card_height`); a player who is out keeps only a narrow seat.
- **Options live where the thing is.** An option that belongs to a card or player drawn on
  the table is chosen by clicking it (gold outline) and no longer also listed as a button;
  the decision bar shows only what has no place on the table (pass, keep/mulligan, modes,
  yes/no…). "List every option" in the top bar brings the full list back. The main action
  — pass priority, confirm — is one large button, and the space bar presses it.
- **Player plates and piles** (user request, same day). Each seat's header is a plate
  with the player's name and a large life total (red at 5 or less), then icons with counts
  for hand, library and graveyard; exile appears only when that player has exiled cards.
  Clicking the graveyard or exile counter opens that pile in a window beside the counter
  (most recent first; cards enlarge on hover and can be clicked when they are a choice;
  click the counter again or press Escape to close). A pile holding a choice is outlined.
- **Tests.** `tests/combat.rs` drives all of this with real pointer presses and drags at
  the places the cards are drawn; the robot (`tests/robot.rs`) declares every attack and
  block of its games that way too.

## D-041 M4 card batch 2: mass effects, permanent targets, exile, tokens, attack triggers
Rules text: `docs/rules-notes/508-701-attack-triggers-keyword-actions.md`. 34 more precon
cards verified (61 of 254 unique; 65 of 275 deck slots), each with a test.
- **Selectors** also filter by `not_subtypes`, `colors`, `power_min` / `toughness_min` and
  `tapped`. Power and toughness are read from final characteristics, so those two are for
  targets, triggers, conditions and one-shot effects; card-db rejects them in the affected
  set of a static ability (which is worked out inside the layer walk, CR 613).
- **Targets:** `TargetFilter::Permanent([selectors])` — a permanent matching any one
  ("target artifact or enchantment", "target nonartifact creature", "artifact,
  enchantment, or creature with power 4 or greater"). Legality on resolution uses the same
  function as choosing, so a creature whose toughness changes can become an illegal target.
- **Effects:** `DestroyAll`, `DamageEach`, `Exile`, `ExileGraveyard`, `Mill`, `ApplyAll`
  (set locked in on resolution, CR 611.2c), `SetX` ("where X is …", worked out once, CR
  608.2h), `Amount::Count(selector)`, `PlayerRef::EachOpponent`.
- **Triggers:** `Attacks(filter)` (CR 508.1m/508.3a: only creatures *declared* as
  attackers), `Any([..])` for one ability with several trigger events ("enters or
  attacks" is one ability, not two), `ObjFilter::ThisOr` ("this creature or another
  Zombie": the source whatever it is, or another object that matches), and
  `Condition::Control` for intervening ifs.
- **Tokens for real cards** are self-contained definitions in `crates/cards/data/tokens/`.
  **CR finding:** by CR 111.4 (2026-11-06) a token whose name isn't specified is named
  for its subtype *plus the word "Token"* — a Zombie token is "Zombie Token". The
  definitions carry that name; scripts refer to them by `CardDef::key`, because different
  tokens share a name (a 5/5 and a 6/6 "Dragon Token"). Told to the user.
- **Left for later, deliberately:** mana abilities that make more than one mana (Sol Ring,
  Llanowar Tribe) — payment feasibility assumes one mana per source (D-021) and needs
  rework first; effects that refer to the triggering object ("it deals damage equal to its
  power"); anything needing a cost other than mana and {T}.
- A third interim deck, `real_zombies.ron`; the other two were refreshed with new cards.
- Golden re-bless: only `state_hash` changed (catalog: new tokens, new fields).

## D-042 M4 card batch 3: sacrifice as a cost, "activate only if"
7 more cards (68 of 254 unique; 75 of 275 deck slots), including the commander Lathliss.
- `Cost::sacrifice_self` ("Sacrifice this permanent"): paid with the rest of the cost as
  the ability is activated (CR 602.2b), as a proposed move to the graveyard through the
  replacement pipeline (invariant 14) — so it can be replaced and it triggers "dies"
  (CR 700.4). The ability keeps its source's last known information.
- `AbilityDef::Activated::only_if` ("Activate only if you control a creature with power 4
  or greater", CR 602.5b), using the same `Condition`s as intervening ifs.
- Still not expressible, on purpose: mana abilities with a sacrifice cost (Treasure) and
  costs that need a choice (sacrifice *another* creature, discard a card).

## D-043 M4 card batch 4: cost reductions
9 more cards (77 of 254 unique), including the commander Ghalta, Primal Hunger. Rules text
appended to `docs/rules-notes/508-701-attack-triggers-keyword-actions.md`.
- `AbilityDef::CostReduction { spells, amount }` ("Dragon spells you cast cost {1} less")
  and `AbilityDef::SelfCostReduction(amount)` ("this spell costs {1} less for each creature
  on the battlefield"), with `Amount::TotalPower(selector)` for Ghalta.
- CR 118.7a: only the generic part is reduced, after commander tax is added, never below
  nothing; locked in when the cast is announced (CR 601.2f).

## D-044 M4 card batch 5: graveyard targets and "you may"
6 more cards (83 of 254 unique).
- `TargetFilter::CardInGraveyard { cards, whose }`, `Effect::ReturnToBattlefield`,
  `Condition::CardsInGraveyard`. Cards are matched by card type and subtype only.
- `Effect::May(effect)`: a yes/no decision (`ChoiceKind::MayAbility`) for the controller as
  it resolves; targets are still chosen when it goes on the stack.
- In the app a graveyard card that is a choice is listed as a button and can be clicked in
  the pile viewer (its counter is outlined).
- Not yet: effects that depend on what the targeted card is ("if it's a Zombie card, draw
  a card"), which need a conditional on the target.

## D-045 M4 card batch 6: the object that triggered the ability
5 more cards (88 of 254 unique). A triggered ability carries its triggering object from
detection to the stack to resolution (with last known information once that object leaves,
CR 608.2h): `ObjRef::Triggering`, `Amount::TriggeringPower` / `TriggeringToughness`,
`DealDamage { by: Triggering }`. Selectors can also require keywords ("with flying").

## D-039 Future scope: other 1v1 constructed formats (user note 2026-11-09)
The user wants 1v1 **Modern, Legacy, Vintage, etc.** supported in future. Nothing is built
for this yet; it is recorded so current work doesn't close the door:
- Formats stay *configuration* (D-004): `GameConfig` for the game rules (life, deck
  size, mulligan/draw rules are already there), plus deck-legality rules per format — a
  card pool, a banned list, a restricted list (Vintage) — which belong in `card-db` next to
  the Commander validation. Scryfall's `legalities` field is the data source; it is not
  cached yet.
- Matches: constructed play is normally best-of-three with a 15-card sideboard; the
  engine has no notion of a match or sideboard yet (CR 100.4, 100.6).
- The card pool is the real cost: those formats span tens of thousands of cards. The
  behaviour-only script + printed-data pipeline (D-035) and the vocabulary are meant to
  scale to it, but only cards with a verified script load (invariant 8), so a format is
  playable deck by deck, not all at once.
- Not in M4. To be scheduled with the user after the precons (spec §13).

## D-046 Change of course: compile Forge's card scripts instead of scripting by hand (user decision 2026-11-09)
**Why.** The user wants "every non-silver bordered card ever printed, for the most part,
eventually". Six hand-scripting batches reached 88 of the 254 precon cards, with yields
falling from 34 cards a batch to 5. Asked whether there is a better way, I proposed moving
the per-card work out of our hands; the user agreed, allowed GPL data to be used, and said
the project will be GPL (the repository is already public).

**Measurement (2026-11-09).** Scryfall's Oracle file lists 33,499 real cards (not tokens,
not silver-bordered or "funny" sets). Forge (github.com/Card-Forge/forge, GPL v3, commit
3d5c95bc) has a structured script for 33,248 of them (99.3%). At the top level those
scripts use 763 distinct features: 184 effect APIs, 131 trigger modes, 105 static modes, 35
replacement events, 243 keywords, 41 cost kinds, plus a filter language and an expression
language. Choosing features greedily, 100 complete 64% of all cards, 200 complete 80%, 300
complete 89%, 500 complete 96%. At parameter level there are about 7,800 distinct details,
3,051 of them used by exactly one card: the long tail is real.
(`scratchpad/measure/forge_stats.py`; the numbers are of Forge's vocabulary, not ours.)

**Decisions.**
- **Licence:** the project is GPL-3.0-or-later (`LICENSE`, workspace `Cargo.toml`). Forge
  is GPL v3 "or later"; using its card scripts and reading its sources to learn what each
  script feature means is now allowed. This replaces the earlier rule that prior-art
  engines are design references only (CLAUDE.md invariant 13, second half) — for
  GPL-compatible sources. Code is still written for this engine, not transplanted.
- **Cards come from Forge's scripts**, compiled into our `CardDef` by `card_db::forge`.
  The scripts are fetched at runtime into the cache, pinned to a Forge commit (they contain
  card names and Oracle text, so they stay out of the repository like Scryfall's data).
- **A card loads only if every line of its script is understood** — every API, parameter,
  filter property and expression. Nothing is dropped or guessed ("never approximate"
  becomes "never ignore a parameter").
- **Verification moves from one test per card to:** (1) a test for each script feature
  the compiler understands (its semantics, written from the rules), (2) the compiled card's
  printed characteristics agreeing with Scryfall's independent data (name, cost, types,
  P/T), (3) the 88 hand-scripted, individually tested cards as a regression set — the
  compiler must produce the same definition for each, (4) fuzzing over games with compiled
  cards. This replaces spec §13's "each card has at least one test" and CLAUDE.md's rule
  that every verified card names tests.
- **Vocabulary is chosen by measurement:** `harness forge-coverage` reports, over all
  cards, which missing feature blocks the most cards; that one is built next.
- **M4** keeps its goal (the four precons playable in the app), reached through the
  compiler rather than by hand.
- Sub-agents may be used for research and parallel work (user, same message).

## D-047 Compiler batch 1: evasion and protection keywords, counterspells, tap/untap, any-player discard and sacrifice (2026-11-09)
First batch of the D-046 work loop after compiled cards went into games. 3,459 → 4,371 of
34,463 scripts compile. Rules text: `docs/rules-notes/702-evasion-protection-keywords-701-actions.md`;
tests: `crates/harness/tests/forge_features.rs`, one per feature, played with invented cards
written in Forge's format (`harness::forge_test_catalog`).

- **Feature tests replace card tests** (as D-046 said they would): a test card is a Forge
  script in the test file, compiled by the same code as real cards. Only subtypes that
  appear on the test scripts' own type lines are known to that compile.
- **Keywords:** Defender, Fear, Hexproof, Indestructible, Intimidate, Shroud,
  `Landwalk(BasicLandType)`. Blocking restrictions that look at one blocker live in
  `combat::can_block`. Indestructible is a "can't" (CR 614.17): a destroy event for such a
  permanent is dropped when the Apply batch is built, and the lethal-damage check skips it.
  Not compiled: hexproof from a quality, landwalk for anything but the five basic types,
  Forge's "hidden keywords" (`HIDDEN CARDNAME can't block.` — an effect on the creature,
  not an ability, so "loses all abilities" must not remove it; it needs its own vocabulary).
- **References:** `TargetRef` gained `Players(PlayerRef)` and `Object(ObjRef)` (damage to
  "you", "each opponent", "itself", "that creature"); `PlayerRef` gained `Each` and
  `TriggeringController` ("its controller", last-known for a permanent that left).
- **Effects:** `Tap`, `Untap`, `TapAll`, `UntapAll`, `Discard { who, amount }`,
  `DiscardHand`, `Sacrifice { who, what, amount }`, `SacrificeThis`;
  `TargetFilter::SpellMatching` ("counter target creature spell");
  `ContinuousEffect::ModifyPTBy` (+X/+X with X worked out on resolution, CR 608.2h — never
  stored: the resolve frame turns it into `ModifyPT`; card-db rejects it in static
  abilities). `Work::EachPlayerChoose` now carries a `PlayerChoice` and the list of players,
  so "target player", "each opponent" and "each player" share one path (CR 101.4).
- **Where Forge and the rules differ, the rules win:** Forge has each player choose *and*
  sacrifice before the next chooses; here everyone chooses (APNAP), then all sacrifice at
  once. Forge's `TriggeredCardController` for a card now in a graveyard is whoever Forge
  finds as its controller there; here it is the last controller on the battlefield.
- **Still blocked on purpose:** `Destroy | NoRegen$` ("can't be regenerated") until
  regeneration exists; a spell whose `Cost$` isn't just its mana cost
  (`spell:additional-cost`); `Defined$ TargetedController` (needs the target's last known
  controller after the target is gone); `.attacking`/`.blocking` filters.
- **`forge-Np` soak setup:** random two-colour 60-card decks drawn by seed from every
  compiled card, so the fuzzers reach cards no hand-made deck contains.

## D-048 Compiler batch 2: costs that need a choice, additional costs; what the all-cards soak found (2026-11-09)
4,368 → 4,858 of 34,463 scripts compile (14.1%). Rules text: the "Costs that aren't mana"
section of `docs/rules-notes/601-casting-602-activating-605-mana.md`.

- **`Cost` gained `sacrifice`, `discard` (a `CostChoice`: N of those matching any of the
  selectors) and `life`.** Chosen after targets (`CastStage::ChooseSacrifice/ChooseDiscard`),
  paid with the mana in one Apply batch (CR 601.2h); `can_begin` refuses a spell or ability
  whose cost can't be paid in full (CR 118.3, 119.4). The golden state hashes changed
  because every activated ability's `Cost` has three more fields; the event-log hashes of
  all 15 golden games are identical.
- **`AbilityDef::AdditionalCost(Cost)`** for "as an additional cost to cast this spell".
  Forge restates the whole cost on the `SP$` line; mana that differs from the card's mana
  cost is not an additional cost and stays blocked (`spell:other-cost`).
- **Not compiled:** `Sac<X/…>`, `Discard<…/Hand|Random|CARDNAME>`, two sacrifice or two
  discard parts in one cost, `tapXType` (tapping other permanents interacts with paying
  mana from them: needs the payment-feasibility rework of D-021), mana abilities with any
  cost but `{T}` (Treasure).
- **Three guards against compiler holes** (the first two found real mis-compilations):
  a line that yields nothing without naming a blocker is `unexplained:*` (12 cards had lost
  their only ability: `DamageAll` with `ValidPlayers$` and no `ValidCards$`); every compiled
  card goes through `card_db::validate`, problems are `invalid:*` (2 cards used the X of a
  permanent spell in its enters trigger, CR 107.3m — now `expr:Count$xPaid in a trigger`);
  the `forge-Np` soak.
- **Free abilities and the agents.** "{0}: this gets -1/-0 and gains swampwalk" compiles
  correctly, and a player who chooses at random between passing and activating it makes
  the stack a random walk: games ran for minutes. `RandomAgent` now passes when the stack
  is 8 deep (`ai::DEEP_STACK`), and `HeuristicAgent` activates one ability at most three
  times a turn — without that the app's CPU players could keep a turn going forever.
- **Runaway boards in the soak.** Horn of Gondor ("{3}, {T}: create X Human Soldier tokens,
  where X is the number of Humans you control") doubles its controller's board every turn;
  with 6,000 creatures each asked whether it attacks, a legal game doesn't end in practice.
  A `forge-Np` game whose battlefield passes 1,000 permanents is abandoned
  (`HarnessError::Runaway`, naming what the board consists of) and counted apart from
  failures. Only the soak does this; the engine has no such limit.
- **"Attacking" and "blocking" in selectors** (`Selector::attacking/blocking`, Forge's
  `.attacking` / `.blocking`): read from `st.combat`, so `layers::matches` takes the state;
  the last-known information of a permanent that left records the role it had ("whenever
  an attacking creature dies"). Static abilities can use it ("attacking creatures you
  control get +1/+0"), so declaring attackers or blockers and the end of combat now
  invalidate the layer cache. Selectors gained two fields: state hashes re-blessed, event
  logs of all 15 golden games identical. With this, 5,001 scripts compile (14.5%).
- **Auras compile:** `K:Enchant:Creature` is `AbilityDef::Enchant(Creature)`, a static with
  `AffectedDefined$ Enchanted` is `Affected::Enchanted` (`Self` is `Affected::This`), and
  `GainControl$ You` is layer 2. Forge filters the enchanted permanent again with
  `Affected$`; that is accepted only when it repeats the enchant filter (anything narrower
  is "as long as enchanted creature is …", not compiled). Other enchant filters (land,
  player, permanent) are `enchant:*` blockers until the engine's Enchant takes them.
- **Hexproof/shroud and Auras:** `targeting::is_legal` (may it be targeted?) is now
  `describes` (is it what the filter says?) plus the hexproof/shroud check. The state-based
  check for an Aura's attachment (CR 704.5m, 303.4c) uses `describes`: an Aura doesn't fall
  off a creature that has or gains shroud.
- **Reading the blocker list:** `svar:unused-ability` is now reported only for a card with
  no other blocker (an ability is unused *because* something else was blocked), which
  raised "one feature away" from 8,500 to 10,400 cards and reordered the list; and
  `forge-coverage` prints the sets of two or three blockers that together are all that
  stops the most cards (an Aura needed both "enchant" and "enchanted creature gets").
- **Layer walk:** with hundreds of resolved effects in one layer, CR 613.8 dependency
  checks copied the whole view for every pair of effects. Effects that can't depend on
  anything (resolved ones, statics already started: CR 611.2c, 613.6) are no longer
  compared, and a cheap pre-check (`Walk::could_matter`) skips the comparison when the other
  effect can't change what this one looks at. Results are unchanged.

## D-049 Mana abilities, step 1: a choice of types, and several mana of one type (2026-11-09)
5,468 → 5,648 of 34,463 scripts compile (16.4%); precons 109 → 114 of 275 deck slots.
Rules text: `docs/rules-notes/601-casting-602-activating-605-mana.md`.

- `ManaProduce::OneOf(types)` — "{T}: Add {W} or {U}" (Forge `Produced$ Combo W U`): still
  one mana per tap, so the payment code only sees a wider choice.
- `ManaProduce::Times { of, count }` — `count` mana of ONE type chosen from `of`: "Add
  {C}{C}" (`Produced$ C C`), "{G}{G}{G}" (`G` with `Amount$ 3`), "three mana of any one
  color" (`Any` with `Amount$ 3`). `cast::ManaSource { id, mask, count }` replaces the
  `(id, mask)` pair; `cast::payable` is exact: a fixed-type source counts as `count`
  units, and a source with a real choice of type is tried with each type (up to four such
  sources exhaustively; beyond that the extras count as one mana each, which can only
  under-estimate). Mana beyond the cost stays in the pool (CR 106.4) until the step ends.
  This lifts the "one mana per source" limit of D-021 for these forms.
- Validation: a multi-mana ability must be the permanent's only way to make mana (the
  planner models one yield per permanent).
- Colour identity (CR 903.4) counts the symbols a mana ability names: both of "{W} or
  {U}", none of "any color".
- **Still blocked:** different mana from one activation ("{W}{U}"), several mana "in any
  combination", `Combo ColorIdentity` (Arcane Signet), mana abilities that cost more than
  {T} (Treasure, signets, pain lands' damage), restricted mana, `DB$ Mana` inside other
  abilities. When sacrificing sources arrive, two things must be handled with them: a
  permanent chosen as a sacrifice *cost* must not also be sacrificed for mana, and
  `can_begin` must not count one Treasure for both.

## D-050 Libraries: searching, scry, surveil, cards put somewhere without being cast (2026-11-09)
5,648 → 6,212 of 34,463 scripts compile (18.0%). Rules text:
`docs/rules-notes/701-search-scry-surveil-library.md`. The vocabulary and the resolve-frame
code were written by a parallel agent in a worktree (which could not finish: its tool calls
kept being declined); its uncommitted work was merged into the main tree, and the compiler,
validation, AI answers, app prompts and all tests were written on top of it.

- **Effects:** `ChooseCards { who, from, cards, count, must, to, reveal, shuffle }` (a
  library search, or cards chosen from a hand), `Scry`, `Surveil`, `PutInto` ("put target
  creature on top of its owner's library", "shuffle it into its owner's library"),
  `MoveAll` ("return all creatures to their owners' hands"), `Dig` (engine only — the
  compiler doesn't produce it yet). `CardZone`, `CardDest`, `Take`.
- **Decisions** reuse `PickN`/`Order` over `OptionDesc::Card`: `SearchLibrary`,
  `ChooseCards`, `ScryBottom`, `SurveilGraveyard`, `ScryOrder` (first listed on top),
  `OrderBottom` (last listed bottom-most), `AuraHost`.
- **Hidden information:** `PlayerView::revealed` holds the hidden cards the pending
  decision shows its player — the cards offered, and the rest of what the effect lets them
  see (the whole library during a search, CR 701.23a) — and is empty for everyone else. The
  AI and the app find card names there. The app shows these choices as a list of named
  buttons for now (no card images in a picker yet).
- **Auras entering without being cast** (CR 303.4f–g): every such move goes through
  `Work::Move`; the controller-to-be chooses what the Aura enchants, and with nothing to
  enchant the card stays where it is. Before, `ReturnToBattlefield` put it onto the
  battlefield unattached and the state check sent it to the graveyard.
- **Selectors:** `mana_value_min/max`, `names` (not compiled from Forge yet), supertypes in
  filters (`Land.Basic`, `.Legendary`, `.Snow`).
- **Compiler** (Forge's hidden-origin `ChangeZone`, docs/forge/effects.md §1): untargeted
  `Origin$ Library | Hand` with `ChangeType$`, `ChangeNum$`, `Mandatory$` (otherwise every
  pick may be declined), `Tapped$`, `LibraryPosition$ 0 | -1`, `Optional$` (one yes/no
  first = `May`), `DefinedPlayer$`. Found cards are revealed when a quality was asked for
  and they don't go to the battlefield (Forge's rule; `Reveal$`/`NoReveal$` override). A
  searched library is shuffled unless `NoShuffle$`/`Shuffle$ False`. `ShuffleNonMandatory$`
  is accepted and means nothing here (it only matters when a player can't search at all).
- **Blocked:** searching another player's library (a targeted player with the activator
  choosing), `Chooser$`, `AtRandom$`, `DifferentNames$`, `EACH` filters, `GainControl$`,
  counters on entering, `Hidden$` choices from public zones, `Dig`, `named` and `cmc`
  filter properties, graveyard-origin untargeted moves.

## D-051 Compiler batch 3: conditional statics, combat restrictions, Equipment, Auras on anything (2026-11-09)
5,001 → 5,468 of 34,463 scripts compile (15.9%); precons 103 → 109 of 275 deck slots.
(D-049 and D-050 are reserved for the mana-ability and library-search work started in
parallel worktrees.) Rules text: `docs/rules-notes/702-evasion-protection-keywords-701-actions.md`
(Equipment, combat restrictions) and `613-layers.md` for the layer questions.

- **`AbilityDef::Static { only_if }`**: "as long as …" / "during your turn". The condition is
  judged inside the layer walk against the view computed so far (`Walk::holds`), never
  through `chars` (which would be the computation calling itself), and it takes part in
  CR 613.8 dependency: an effect that would make another's condition false is applied
  first. New conditions `YourTurn`, `NotYourTurn`, `ControlAtMost`, `HandAtMost`; Forge's
  `IsPresent$` (+ `EQ0`/`LE<n>`) and `Condition$ PlayerTurn | NotPlayerTurn | Threshold |
  Metalcraft | Hellbent`. `CheckSVar$` conditions are a long tail and stay blocked.
- **Static "+N/+N for each [permanent]"** is `ModifyPTBy` counted continuously in layer 7c.
  Only counts of permanents that don't look at power or toughness are allowed there (the
  validator enforces it): anything else would depend on the order of other 7c effects.
- **`AbilityDef::CombatRule { affected, rule }`** — `CantAttack`, `CantBlock`,
  `CantBeBlocked { by }`, `CantBlockAttackers(sel)`: restrictions (CR 508.1c, 509.1b), not
  characteristics, so they are read by the combat frames from the abilities permanents
  currently have, not by the layer walk. One Forge line with several modes ("can't attack
  or block") becomes several abilities. Not compiled: `CantAttack` with `UnlessDefender$`
  or `Target$`, `CantBeActivated`.
- **Selectors:** `power_max` ("with power N or less"), `not_keywords` ("without flying").
- **Equipment:** `K:Equip:<cost>` is an ordinary activated ability (sorcery timing, target
  creature you control) whose effect is `Effect::AttachThis`; "equipped creature" is the
  existing `Affected::Enchanted` (what the source is attached to — the generated text says
  "equipped" for an Equipment). CR 704.5n: an Equipment whose host is gone or not a
  creature becomes unattached (`GameAction::Unattach`, done on the spot by the state
  check, which then runs again). A new attachment is a new timestamp (CR 701.3c).
  Restricted equips ("equip legendary creature") are blocked.
- **Auras enchant any permanent filter** ("enchant land", "enchant creature you control"):
  `Enchant(TargetFilter::Permanent(..))`; the engine needed no change (targets, attachment
  and the CR 704.5m check all go through the filter). Players can't be enchanted yet.
- **A mis-compilation fixed:** off the battlefield Forge's filter word `Permanent` means a
  permanent *card* (CR 110.4a); it compiled as "any card" for graveyard targets and counts
  and for counterspell and cost-reduction filters (`Tx::card_selector`).
- **Heuristic AI and illegal block declarations:** its block plan doesn't know every
  restriction the engine applies; when a planned double-block of a creature with menace
  lost one blocker the engine rejected the declaration, and the agent repeated it forever
  (two soak games hit the decision cap). Asked about the same blocker twice in one combat,
  it now declares no blocks.
- **Parallel agents:** two worktree agents (mana abilities, library search) were started on
  the user's suggestion; both were stopped by declined tool calls inside their worktrees
  and left only partial, uncommitted work there.

## D-052 Mana for the precons: Treasure, commander identity, restrictions; cost steps in rules order (2026-11-09)
The user asked for the precons to be playable ("so I have something to play and test");
their commanders and mana rocks needed these.
- **Mana abilities are per ability, not per permanent** (`cast::ManaSource`): plain "{T}:
  Add one mana" abilities of a permanent are merged, every other one (several mana,
  "sacrifice this", a spending restriction) is an alternative way to tap it. This replaces
  D-049's "a multi-mana ability must be the permanent's only mana ability". What remains
  unmodelled, and is rejected by `card_db::validate`: two abilities of one permanent that
  differ in more than the mana's type and make the same type (the player picks an ability
  by permanent + type), and mana abilities whose cost is more than `{T}` and "sacrifice
  this" (signets' `{1}, {T}`, "sacrifice a creature: add …").
- **Treasure:** the sacrifice is applied the moment the ability is activated, in the middle
  of paying for something else (`Flow::Continue([cast frame, Apply])`).
- **The order of cost steps changed to the rules' order** (CR 601.2g then 601.2h): mana
  abilities first, then the choice of what to sacrifice and discard, then everything is
  paid. Before, the sacrifice was chosen before the mana, which with Treasure would have
  let one permanent pay two costs. `SacLimit` keeps enough sacrifice candidates out of the
  mana payment that the cost can always be completed (D-019). Replays recorded before this
  change that pay such costs no longer replay.
- **Commander identity mana**, **restricted mana** (`PlayerState::restricted_mana`, new
  `GameAction`s `AddRestrictedMana` / `ReleaseRestrictedMana`), **"activate only if" on mana
  abilities**, **`Effect::AddMana`**, **`Amount::SacrificedPower`**: see
  `docs/rules-notes/601-casting-602-activating-605-mana.md`.
- **Entering with counters:** `K:etbCounter` (fixed, counted, or the spell's own X) and
  `K:ETBReplacement` when it puts counters on other entering permanents
  (`Replaces::OtherEnters`, `ReplaceWith::EntersWithCounted`). `Replaces`/`ReplaceWith` are
  no longer `Copy`. Counters other than +1/+1 and -1/-1 are still not modelled.
- State hashes changed (new fields on `PlayerState`, `StackObject`); event-log hashes did
  not. Golden file re-blessed.
- The heuristic AI taps permanents that stay before sacrificing Treasures.

## D-053 The precons in the app before they are complete (2026-11-09)
`Setup::Precon { players }` (`precon-Np`, first in the app's menu) is a Commander game with
the four precons in seat order, **as far as their cards compile**: a card that doesn't
compile yet is left out of the deck — never approximated — and basic lands are trimmed in
proportion (`harness::precon_deck`), so that a deck missing a third of its spells isn't
two-thirds land. The decks are therefore smaller than 100 cards for now and grow as
`harness coverage` does; Commander deck validation (CR 903.5a) is not applied to this
setup. All four commanders compile. Chosen over waiting for 100% because the user wants to
play and test now; chosen over filling with basics because that plays badly.

## D-054 App: the user's first list of issues from playing the precons (2026-11-10)
- **"Commander tax doesn't exist":** the engine charges it (checked over eight AI precon
  games: every recast cost {2} more, except where Starnheim Aspirant's "Angel spells you
  cast cost {2} less" cancelled Giada's first tax). The app never showed it: the commander
  in the command zone now carries a "tax +N" badge, and the enlarged card says so.
- **Token pictures:** tokens are pictured by Scryfall's token cards
  (`scryfall::load_bulk_tokens`, `token_printing`): the printed token with the same name,
  types, subtypes, colors, power/toughness and keywords, and rules text exactly when the
  token has abilities; none when several with different text fit. 545 of the 659 tokens
  that compile have one. They come from the bulk file already in the cache; images are
  downloaded on demand like card images.
- **Automatic passing (after MTGO):** while the human's own spell or ability is on top of
  the stack, priority is passed for them ("Hold priority" in the stops panel turns that
  off); "Pass unless an opponent acts" (F4) passes for the rest of the turn until an
  opponent's spell or ability is on the stack; "Pass until end of turn" (F6) as before.
  Both yields also decline the attack; blocks are always asked.
- **Deck choice:** `Setup::Precon { players, decks }` — the deck of each seat, chosen in
  the menu (label `precon-4p:2301`; old saves load with the default order).
- **Slower opponents:** "Slower" (2 s) and "Slowest" (4 s); the speed is a drop-down.
- **Cards that aren't on the table as choices** (graveyard targets, library searches) are
  offered as the cards themselves with where they are, and named in option text ("target
  X (in P0's graveyard)", was "target #123").
- **The table resizing during actions:** the prompt area of the bottom bar changed height
  as decisions came and went (three different table heights in a test game), and the top
  bar could re-wrap when the turn label or the "loading images" note changed. The prompt
  area now always has room for its title and a row of buttons, the turn label has a fixed
  width, and image loading is reported in the side panel. `tests/layout.rs` holds this.
- The log has room below its last line.

## D-055 Compiler batch 4 (breadth): "look at the top N cards" and what followed (2026-11-10)
The user's direction from here: the biggest blockers across all cards, not the precons.
- **Dig** (`Effect::Dig` existed; now compiled): how many are taken (`ChangeNum`,
  `Optional`), which (`ChangeValid`), where they and the rest go, reveal or look.
  - *Engine fix found by reading the sample:* the effect's **controller** looks, picks and
    orders, whoever's library it is ("look at the top two cards of target player's
    library" had the library's owner doing it). `Work::Dig::chooser`, `Work::Place::by`.
    What a player looks at is logged for that player only (`Event::LookedAt`, appended).
  - Forge has the activator choose for every library; for "each player …" that is only
    right when nobody has anything to choose, so anything else is a blocker.
  - Three kinds of script-versus-card differences are checked against the card's Oracle
    text and blocked: revealing where the card looks, looking where it reveals, and the
    order of the rest (any order / random). Found in the first sample read (Forging the
    Anchor, Sarkhan's Dragonfire).
- `harness forge-coverage --sample N --with TEXT` reads over just the cards whose compiled
  text contains TEXT.
- **One "target" word with several targets** (`TargetMin$`/`TargetMax$`: "two target
  creatures", "up to two target creatures", "one or two"): `Mode::groups` (`TargetGroup
  { first, count, min }`) — the group's slots share a filter, take different targets (CR
  115.3), and those past `min` may be left empty. An "up to" group must be the mode's
  last slots, so targets that weren't chosen are simply missing from the end of the list
  and nothing else in the engine changes shape (`CastFrame::targets_closed` remembers that
  the player stopped). A spell with no targets chosen still resolves (CR 608.2b is about
  spells that *have* targets). The instruction is copied per slot and wrapped in
  `Effect::Together`: one event, so two creatures destroyed this way die together.
  Compiled only for instructions that are one event each (damage, destroy, exile, return
  to hand, tap/untap, counters, pump, life, draw, mill); "X target creatures" and an "up
  to" group followed by other targets are blockers. +131 cards.
  State hashes changed (`Mode` gained a field, and modes are part of the catalog in the
  state); event-log hashes did not. Golden file re-blessed.
- **Amounts** (Forge's expression language, `Tx::expr_text`): cards in graveyards, hands
  and libraries (`Amount::CardsIn`), one arithmetic suffix (`Amount::Calc`: plus, minus,
  times, halves, limits), `Count$Compare` (`Amount::IfAtLeast`), chaining through `SVar$`,
  `Number$`, domain, devotion, the greatest power / toughness / mana value, hand size,
  life total, the source's toughness. `Amount::stands_alone` says which a replacement
  effect may use, `Amount::countable_in_layers` which a static "+N/+N for each" may.
  +181 cards.
  - *Mis-compilation found by reading the sample and fixed:* "where X is …" was worked out
    before the first instruction of the chain. CR 608.2h has it worked out when the
    instruction that needs it is applied, so "draw a card, then deal damage equal to the
    number of cards in your hand" was off by the draw. `SetX` is now placed in front of
    the first instruction that uses X (`Chain::x_at`).
- **Characteristic-defining power/toughness** ("*/*, equal to the number of …"; CR 604.3,
  613.4a): `ContinuousEffect::SetPTBy` in a static ability on its own card, applied in
  layer 7a against the view in progress, and — because such an ability functions in every
  zone — by `layers::chars` for a card anywhere else. A printed "*" needs such an ability
  (else `pt:variable`), and the cross-check against Scryfall's printed P/T accepts "*"
  exactly then. +76 cards.
  - *Found by the soak (four of 250 random-deck games overflowed the stack):* a static
    ability's graveyard condition looked at a graveyard card whose "*" counts permanents,
    which asked for the layered view while it was being built. The walk now marks itself
    as running (`LayerCache`), and a card that isn't on the battlefield is looked at as
    printed from inside it; card-db already keeps anything evaluated there from looking
    at power or toughness. Regression test, mutation-checked.
- **Cards chosen on resolution from a public zone** (`Hidden$ True` with `Origin$
  Graveyard` or `Battlefield`: "return a creature card from your graveyard to your hand",
  "return a creature you control to its owner's hand") compile onto `Effect::ChooseCards`.
  Forge offers every player's matching cards; the engine's choice is among the chooser's
  own, so the filter must say "your" (`YouOwn`/`YouCtrl`), else a blocker. And a
  `ChangeZone` from the battlefield with neither target nor `Defined$` moves its own card
  (Forge's default): "return this to its owner's hand". +93 cards.
- **Filter properties:** mana value (`cmcLE/GE/EQ/LT/GT`), "non-" colors and supertypes
  (`nonBlack`, `nonBasic`, `nonLegendary`), toughness at most, power less/greater than.
  `Selector` gained `not_colors`, `not_supertypes`, `toughness_max` (state hashes changed,
  event-log hashes did not; golden file re-blessed). +121 cards.
  - *Two silent mis-compilations found while doing it, both fixed:*
    (1) a card in a graveyard, hand or library was matched on its types and subtypes
    only (`targeting::card_matches`), so "return target green card from your graveyard"
    (Revive) accepted any card and "target legendary card" any card. It now compares
    everything the selector says (color, supertypes, mana value, name, power…).
    (2) a spell with {X} in its cost whose script defines X as something else (Stir the
    Grave: "X is the target's mana value") compiled with a free X — castable for X = 0 on
    any target. The numeric variable was simply never read. `cost:X defined by the
    script` is now a blocker.
- `PutCounterAll` ("put a +1/+1 counter on each creature you control"):
  `Effect::PutCountersAll`, one event. +33 cards.
- **Where this batch ended (2026-11-10):** 7,125 of 34,463 scripts (20.7%), from 6,367.
  Largest blockers left, alone / at all: double-faced cards 399; Forge's "Effect" objects
  353 / 1,546; statics that grant abilities or triggers 337 / 1,064 (needs abilities that
  come from another card: `(CardDefId, index)` instead of an index into the permanent's
  own card); Animate 236 / 848; "you may pay" triggers 185 / 796 and the other costs paid
  during resolution (`UnlessCost`); Sagas 185; Adventure 151; Regenerate 146; Cycling 140
  (+65 typecycling; needs abilities activated from the hand); `repl:Moved` 132.

## D-056 Compiler batch 5 (breadth): abilities off the battlefield, cycling (2026-11-10)
Rules text: docs/rules-notes/702.29-cycling-113.6-ability-zones.md.
- **Where an activated ability works from** (CR 113.6): `AbilityDef::Activated::from`
  (`ActivationZone`: battlefield, hand, graveyard; Forge's `ActivationZone$`). Off the
  battlefield the card's owner activates it. Costs "discard this card"
  (`Cost::discard_self`) and "exile this card from your graveyard" (`Cost::exile_self`);
  a cost that uses the card in a zone the ability doesn't work from is rejected by
  validation and is a compiler blocker. Activating from the hand reveals the card
  (CR 602.2a).
- **Cycling and typecycling** (CR 702.29): the keyword compiles to that kind of ability
  with `cycling: true` ("basic landcycling" is Forge's `TypeCycling:Basic`). The discard
  that pays for it is `MoveCause::Cycle`: `Event::Discarded` then `Event::Cycled` (appended
  to the enum). `Trigger::Cycles` — "when you cycle this card" is found on the card where
  the discard put it; "whenever [a player] cycles a card" on permanents.
- **"This card"** (`ObjRef::ThisCard`): the source of such an ability while it is still the
  same object (CR 400.7) — "{2}{B}: Return this card from your graveyard to your hand".
- **Bug found by reading the sample, older than this batch: "another target …" could
  target the ability's own source.** The target filter kept `other`, but targeting matched
  it against no object at all. `targeting::{is_legal, describes, candidates}` now take the
  ability's source. Every compiled card with "another target" was affected (an ability
  could put its counters on its own source).
- `forge-coverage --families`: ranks kinds of feature, counting a trigger mode, effect or
  keyword together with its own parameters. The plain ranking shows a new trigger mode as
  blocking nothing alone, because its parameters are separate blockers.
- The app labels an ability option with the ability's text (casting a card and cycling it
  were "cast X" and "activate X (ability 0)").
- Golden state hashes re-blessed (`Cost` and `Activated` gained fields); event-log hashes
  unchanged.
- 7,125 → 7,467 of 34,463 scripts (21.7%).

## D-057 Compiler batch 6 (breadth): more trigger events, counted conditions (2026-11-10)
Rules text: docs/rules-notes/603-triggered-abilities.md (last section).
- **Steps**: `TriggerStep` gains the beginning of combat, end of combat, and the two main
  phases; Forge's `End of Turn` (the end step, CR 513.1a) and `Draw` were simply not mapped
  (925 scripts use the former). `ValidPlayer$ Player` is "each player's".
- **`Trigger::Leaves`** ("leaves the battlefield", any destination): found like "dies",
  looking back (CR 603.10a).
- **`Trigger::Cast::spells`**: what the spell has to be, as selectors ("noncreature",
  "Spirit or Arcane", "multicolored", "mana value 4 or greater"), matched against the spell
  on the stack. Card types alone still compile to `types` (the hand-scripted cards are
  unchanged).
- **`Condition::Compare { amount, cmp, n }`**: Forge's `CheckSVar$`/`SVarCompare$` and
  every `PresentCompare$` operator (only `GE` compiled before). The amount must stand
  alone; on a static ability it must be countable inside the layer walk. A condition that
  counts permanents takes part in the dependency check like `Control` does.
- Text: a single optional target reads "up to one target …".
- Golden state hashes re-blessed (`Trigger::Cast` gained a field); event-log hashes
  unchanged.
- 7,467 → 7,716 of 34,463 scripts (22.4%).

## D-058 Compiler batch 7 (breadth): damage triggers, what an event carries, an Oracle guard (2026-11-10)
Rules text: docs/rules-notes/603-triggered-abilities.md (last section).
- **`Trigger::Damage { by, to, combat, per }`** for Forge's `DamageDone` (once for each
  source and recipient), `DamageDoneOnce` (once for each recipient) and `DamageDealtOnce`
  (once for each source), found from the `DamageDealt` events of one Apply batch.
- **`EventFacts { player, amount }`** travel with a triggered ability from the event to
  its resolution (pending trigger → stack object → resolve frame): "that player"
  (`PlayerRef::TriggeringPlayer`) and "that much" (`Amount::TriggeringAmount`). There is
  still one triggering object: the permanent dealt damage, or for damage to a player the
  source. The compiler tracks which of Forge's `Triggered…` names the event of the trigger
  being compiled provides (`TrigCtx`); a name it doesn't provide is a blocker as before.
- **`ObjFilter::Host`**: "enchanted creature", "equipped creature" in any trigger.
- **Reading the compiled card back against its Oracle text** (`oracle:*` blockers). A scan
  of all 7,929 compiled cards for words of the Oracle text missing from the engine's text
  found Forge scripts that leave things out where its AI would never choose otherwise:
  "you may put a counter" compiled as mandatory (3 cards), "another target creature" on an
  ability that sacrifices its source (1), and "you may search … then shuffle" compiled as
  a search that always shuffles (3). The compiler now blocks a card that says "may" where
  the compiled card has no choice, or "another target" where it has no "another"; a plain
  library search is made optional when the card has one search and says "may search" once.
  This is the first piece of the generated-text-versus-Oracle round trip.
- 7,716 → 7,924 of 34,463 scripts (23.0%). Golden hashes unchanged.

## D-059 Compiler batch 8 (breadth): the card a dead permanent became, self-moving spells, ownership (2026-11-10)
Rules text: docs/rules-notes/702.29-cycling-113.6-ability-zones.md (last section).
- **`ObjRef::TriggeringCard`** + `EventFacts::moved`: "when this dies, return it to its
  owner's hand" — the object the permanent became in the graveyard, while it is still
  there (CR 400.7). Forge's `Defined$ TriggeredNewCardLKICopy`; also `TriggeredCard` and
  `Self` where the effect takes the card from the graveyard in a "dies" trigger.
- **`Effect::ThisSpellTo { to, shuffle }`**: "exile this spell", "shuffle it into its
  owner's library" — where the spell goes as it finishes resolving (CR 608.2n). Recorded
  on the resolve frame and used when the spell leaves the stack by resolving.
- A targeted card from a graveyard to a library (`PutInto`); `ValidTgtsDesc$` is text.
- **Bug found by reading the sample, older than this batch: "you own" was compiled as "you
  control"** (`Creature.YouOwn` in "is put into your graveyard from the battlefield").
  `Selector::owner` now says whose card a permanent is; selector facts and last known
  information carry the owner. Six compiled cards changed, all to their Oracle meaning.
  An Oracle guard blocks a card that says "put into your graveyard from the battlefield"
  where the script asks who controlled it.
- A before/after diff of the engine's text of every compiled card
  (`forge-coverage --sample 1`) shows exactly which cards a compiler change touches.
- Golden state hashes re-blessed (`Selector` gained a field); event-log hashes unchanged.
- 7,924 → 8,060 of 34,463 scripts (23.4%).

## D-060 Compiler batch 9 (breadth): keywords that are shorthand (2026-11-10)
- **Shadow** (CR 702.28b) and **horsemanship** (702.31b): `Keyword` variants, checked
  where the other evasion abilities are (`combat::can_block`).
- **Prowess** (702.108a) and **exalted** (702.83a) compile to the triggered abilities the
  rules define them as; `Trigger::AttacksAlone` (CR 506.5: the only creature declared as
  an attacker), also for Forge's `Mode$ Attacks | Alone$`. They are not `Keyword`s, so
  "gains prowess" stays a blocker.
- **Affinity** (702.41a) is `SelfCostReduction` counting what the player controls.
- **Devoid** (702.114a): the card is compiled with no colors. That is what the
  characteristic-defining ability amounts to in every zone; its color identity still
  comes from its mana cost (CR 903.4).
- 8,060 → 8,237 of 34,463 scripts (23.9%). Golden hashes unchanged.

## D-061 Compiler batch 10 (breadth): attacking, blocking, life, draws, discards, sacrifices as trigger events (2026-11-10)
Rules text: docs/rules-notes/603-triggered-abilities.md (last section).
- `Trigger::{PlayerAttacks, Blocks, BecomesBlocked, Unblocked, BlockedBy, GainsLife,
  LosesLife, Draws, Discards, Sacrifices}`; `triggers::detect_with` is the one loop the new
  kinds share. Blocks are detected in `DeclareBlockersFrame` once every defending player
  has declared (`triggers::detect_blocks`).
- `PlayerState::drawn_this_turn` for "your second card each turn".
- The compiler's trigger context is now a table of Forge's names per event (`TrigCtx`:
  which `Triggered…` names are "it", which are the ability's own source, which is "that
  player", which expression is "that much") instead of one flag per damage case.
- Bushido and flanking. **Caught by reading the sample before committing:** flanking had
  first been added as a bare keyword, with no trigger. It is now the keyword (for "a
  creature without flanking") plus the triggered ability, and only `K:Flanking` on the
  card itself produces it.
- Not done here: "becomes tapped/untapped" (taps happen outside Apply batches, where
  triggers are detected: D-025), "becomes the target" (ward), "when you sacrifice this".
- Golden state hashes re-blessed (`PlayerState` gained a field); event-log hashes unchanged.
- 8,237 → 8,528 of 34,463 scripts (24.7%).

## D-062 Compiler batch 11 (breadth): Animate; statics that change types, colors, base P/T (2026-11-10)
Rules notes: docs/rules-notes/613-layers.md (last section).
- **`Animate` / `AnimateAll`** compile to `Apply` / `ApplyAll` with the continuous effects
  the engine already had (types added, colors set, keywords, "loses all abilities", base
  power and toughness), for a turn or indefinitely. Man-lands, "becomes an artifact in
  addition to its other types", Humble. Anything that removes types, adds colors, sets
  only one of power and toughness, or lasts "for as long as …" stays a blocker.
- **`S:Mode$ Continuous`** reads `AddType$`, `SetColor$`, `RemoveKeyword$`,
  `RemoveAllAbilities$`, `SetPower$`/`SetToughness$` (Humility, Living Lands, Ensoul
  Artifact).
- Validation let only Auras carry a combat rule about "the enchanted creature"; Equipment
  may too (Whispersilk Cloak).
- 8,528 → 8,668 of 34,463 scripts (25.2%). Golden hashes unchanged.
- **Where this run ended.** The largest family left is still static abilities that grant
  abilities (`AddAbility$`/`AddTrigger$`, ~170 cards alone; the same is needed by the rest
  of Animate and by many Pump and Effect scripts). It needs an ability to be addressable
  as (card definition, index) everywhere an index into the permanent's own card is used
  now: `Characteristics::abilities`, `OptionDesc::ActivateAbility`, `CastSource::Ability`,
  mana sources, trigger sources, replacement and static keys. `StackKind::Ability` and
  `PendingTrigger` already carry the definition. The granted definitions would live in the
  granting card (a `granted` list beside `abilities`). After that, by family (cards blocked
  by it alone): `Mana` 345 (mana abilities that cost mana, two abilities making the same
  type), costs paid during resolution — "you may pay", "unless you pay" — 281,
  double-faced cards 399, Forge's `Effect` objects 376, Sagas 185, protection 141,
  flashback 103.

## D-063 Compiler batch 12 (breadth): costs paid while something resolves; "its controller" (2026-11-10)
Rules text: docs/rules-notes/601-casting-602-activating-605-mana.md (last section).
- **`Effect::Pay { who, cost, paid, unpaid }`** (CR 118.12, 118.12a): "you may pay {1}. If
  you do, …" — Forge's trigger that runs an `AB$` with a `Cost$` — and "… unless [a
  player] pays …" — `UnlessCost$`/`UnlessPayer$`/`UnlessSwitched$` on any effect.
  The rest of a chain after an "unless" instruction happens either way (Forge's default).
- **Paying reuses the cast frame**: `CastSource::Payment` starts a `CastFrame` at its
  payment stage, so mana abilities, generic-mana allocation and chosen sacrifices/discards
  work as for a spell. The resolve frame asks (`ChoiceKind::PayUnlessCost`) only when
  `cast::can_pay` says the cost can be paid in full (CR 118.3), then carries on.
- **`PlayerRef::ControllerOf(slot)`** ("its controller", Forge's `TargetedController`):
  who controlled the target as resolution began (`ResolveFrame::target_controllers`,
  CR 608.2h) — "destroy target land. Its controller loses 2 life".
- **Found by the soak (1 game in 750): the engine asked a player to pay who then
  couldn't.** A source of restricted mana ("spend this mana only to cast Angel spells")
  counted towards a cost it can't pay. `Payee::Cost` now marks such payments; restricted
  mana and its sources are left out. Regression test
  `restricted_mana_cant_pay_a_cost_on_resolution`. The dead-end assertion now says who was
  paying what for which card.
- A trigger that is both "you may" and priced asks once (the payment is the choice).
- The heuristic agent pays when asked.
- Known and not fixed: two cards found whose Forge script restricts a target more than
  the card does (Spirit Bonds, Konstrari Charm: "you control" in the script only). A probe
  for it is too noisy to be a compiler guard yet.
- 8,668 → 8,933 of 34,463 scripts (25.9%). Golden hashes unchanged.

## D-064 Abilities given to other objects; an ability is named by its card (2026-11-10)
Rules text: docs/rules-notes/113-abilities-given.md.
- **`AbilityId { card, index, granted }`** replaces the bare index into an object's own
  card everywhere an ability is named: `Characteristics::abilities`, the options
  `ActivateAbility` / `Trigger` / `Replacement`, `CastSource::Ability`, `StackKind::Ability`,
  `PendingTrigger`, `RestrictedMana`, replacement keys, `Event::Triggered`, the views. The
  text of an ability is looked up in the catalog (`CardCatalog::ability`), so it is found
  after its source has left and whoever's card it is written on.
- **`CardDef::granted`**: the abilities a card gives away (the quoted text in "enchanted
  creature has "…""), beside the ones it has. **`ContinuousEffect::GrantAbilities`**
  (layer 6) names them by index — in a static ability, or in a resolving effect
  ("until end of turn, target creature gains "…"").
- The refactor was checked to change nothing before anything was added: all 15 golden
  games kept their event logs, and 650 soak games over four setups ended exactly as before.
- **Static abilities that are given** join the layer walk when they are given (layer 6),
  with the timestamp of CR 613.7a. card-db refuses a given static ability whose effect
  belongs to an earlier layer (control, types, colors, a characteristic-defining ability):
  by the rules it would never apply, and nothing printed needs it.
- **The same ability twice is had twice** (CR 113.2c): it triggers twice, its static effect
  applies twice, each replacement effect is its own (`nth` in the key). As something to
  activate it is offered once.
- **`OptionDesc::TapForMana` names the ability** unless it is a plain "{T}: Add one mana"
  (those stay one way to tap the permanent). This lifts D-052's limit — two mana abilities
  of one permanent may now make the same type in different amounts, at different costs or
  with different restrictions — which a land given "{T}: Add two mana of any one color"
  needs anyway. The app says which ability only where two make the same mana.
- **Rules correction, changes existing games:** a land that loses all abilities loses the
  "{T}: Add [mana]" of its basic land types too (CR 305.6: an ability it has; ruled so for
  Imprisoned in the Moon, 2016-07-13). Before, an animated Forest under "creatures lose
  all abilities" still tapped for {G}. With this one rule switched off, every soak game of
  the old cards matched the baseline; with it, games in which that happens differ.
- **Compiler:** `AddAbility$`, `AddTrigger$`, `AddStaticAbility$`, `AddReplacementEffect$`
  on `S:Mode$ Continuous`; `Abilities$`, `Triggers$`, `staticAbilities$`, `Replacements$`
  on Animate. The script variables are compiled as abilities of whatever gets them (a
  trigger needs no `TriggerZones$`: it works where the object is given it). `AddSVar$` is
  read and means nothing here — the variables are found on the giving card anyway —
  except a variable copied under another name, which is accepted only for the names
  Forge's AI reads (`AI_HINTS`). An ability that gives itself is refused.
- One static ability that takes abilities away and gives some ("loses all other abilities
  and has flying") now takes away first, as Forge does; before, the keywords it gave
  were lost with the rest. No compiled card had both.
- The Oracle guard compares what a card says in quotes with the abilities the compiled
  card gives, and what it says outright with the rest.
- The app shows the abilities a permanent has been given under its enlarged card, and
  labels a given ability's button with its text.
- `Event::Triggered` carries the `AbilityId`: the event logs of games with triggers hash
  differently (7 of the 15 golden games; the 8 without triggers kept theirs). State
  hashes changed for all (the types above are part of the state). Re-blessed.
- 8,933 → 9,115 of 34,463 scripts (26.4%); precons 148 of 275 deck slots. The family
  "static abilities" fell from 426 cards blocked by it alone to 166. All 182 newly
  compiled cards were read against their Oracle text.
- **Next by family** (cards blocked by it alone): Forge's `Effect` objects 419,
  double-faced cards 399, `Mana` 356 (different mana at once, mana abilities that cost
  mana — the payment feasibility rework of D-021), `ChangeZone` rest 350, tokens 218,
  Sagas 185, regeneration 182.

## D-065 Effects that last on no object (Forge's effect objects); "prevent all damage" (2026-11-10)
Rules text: docs/rules-notes/611.2-lasting-effects-615-prevention.md.
- **`effects::Lasting`** / **`Effect::Lasting`**: abilities that work for a while without
  being on any permanent — "target creature can't be blocked this turn", "prevent all
  combat damage that would be dealt this turn". This is the engine's form of Forge's
  `Effect` API (an invisible card in the command zone carrying static abilities,
  replacement effects and triggers, with "remembered" objects). The carried abilities are
  entries of the card's `granted` list (D-064); in them "this" is the source of the spell
  or ability that made the effect and `Affected::Remembered` / `ObjFilter::Remembered` the
  object it is about.
- **Why not a granted ability:** CR 113.12 — "can't be blocked" on a creature "is neither
  granting an ability nor setting a characteristic". As a granted ability it would be
  lost with "loses all abilities"; as a lasting effect it isn't.
- **What a lasting effect can carry so far:** combat rules (`CombatRule`, plus the new
  `CantBlockSource`) and prevention (`Replaces::Damage` with `ReplaceWith::Prevent`).
  card-db refuses anything else; the compiler names each missing kind (`Effect:Triggers`,
  `Effect:static <mode>`, `Effect:repl <event>`) so they can be ranked.
- **Rules, not characteristics, aren't locked in** (CR 611.2c): "creatures without flying
  can't block this turn" also holds for a creature that arrives later. Forge's `PumpAll`
  with a hidden keyword locks the set in; here it is compiled as a lasting rule about
  whatever matches.
- **"Prevent all damage"** also as a permanent's own ability (`R:Event$ DamageDone |
  Prevent$ True`), and as Forge's keyword-shaped rules: `K:Prevent all damage that would
  be dealt to CARDNAME.`, `KW$ HIDDEN CARDNAME can't block.`, `AddHiddenKeyword$ …`.
- **Mis-compilation found by reading the sample (older than this batch):** "You have
  hexproof" / "You have shroud" (five cards) compiled to an ability that did nothing —
  a joke card's type line ("Planeswalker — You") had made "You" a known subtype, so
  `Affected$ You` read as "permanents of subtype You". Forge's filter words for players
  and kinds of object are never subtypes now; those cards are blocked (`filter:You`).
- **New Oracle guard:** `oracle:target you control` — the compiled card says "target …
  you control" and the card never says "you control" or "your". Two cards whose script
  narrows the target for Forge's AI (Konstrari Charm, Firefright Mage) are blocked by it.
  Equip is exempt (its restriction is in reminder text).
- The views list the lasting effects in force (`PlayerView::lasting`); the app shows the
  ones about a permanent under its enlarged card and labels the choice between
  prevention effects. Effects about nothing in particular ("prevent all combat damage
  this turn") are not shown anywhere yet.
- The heuristic agent has no preference where to point a lasting effect (it doesn't see
  what the effect carries).
- State hashes changed (`GameState::lasting`); all 15 event logs unchanged. Re-blessed.
- 9,115 → 9,305 of 34,463 scripts (27.0%); precons 149 of 275 deck slots. The `Effect`
  family fell from 419 cards blocked by it alone to 207. All 197 newly compiled cards
  were read against their Oracle text.
- **Next for effect objects:** triggers they carry — "whenever … this turn", "at the
  beginning of the next end step" (46 alone, 169 in all); then must-attack and
  can-attack-as-though rules, and effects about players.

## D-067 How a permanent enters: "enters tapped unless …", "as this enters, you may …" (2026-11-10)
Rules text: docs/rules-notes/614.12-enters-tapped-unless.md.
- **Why first:** by EDHREC rank `repl:Moved` alone blocked more of the most played
  Commander cards than any other blocker (the dual lands). 75 of the 1,000 and 114 of the
  5,000 most played cards compile now that didn't: the ten shock lands, check lands,
  battle lands, bond lands, slow lands, fast lands, reveal lands and Snarls, the Castles
  with an "unless", Rivendell, Mox Diamond, Authority of the Consuls.
- **Vocabulary** (all appended at the end of their enums):
  `ReplaceWith::EntersTappedUnless(Condition)`, `ReplaceWith::EntersTappedUnlessYou(AsEnters)`
  ("as this enters, you may [pay N life / reveal a … card from your hand]. If you don't,
  it enters tapped"), `ReplaceWith::EntersOnlyIfYou(AsEnters)` ("if this would enter, you
  may discard a land card instead. If you do, put it onto the battlefield. If you don't,
  put it into its owner's graveyard"), `AsEnters { PayLife, Reveal, Discard }`,
  `ReplaceWith::RevealShuffleIntoLibrary`, `Replaces::Dies(Selector)` ("if a creature an
  opponent controls would die"), `Replaces::ThisToGraveyard` ("… from anywhere"),
  `Amount::Players(Who)` ("the number of opponents you have") and
  `Amount::LifeAmong { players, highest }`.
- **The choice is made inside the replacement pipeline** (CR 614.12a, 603.6d: no stack,
  no priority). `ApplyFrame` gets a `choosing` key (serialized only when set, so golden
  hashes are unchanged) and suspends with the new `ChoiceKind::AsEnters { object,
  ability, tapped }`: `Accept` (pay) or the cards that may be revealed / discarded, then
  `Decline`. `tapped` says it enters tapped anyway, for the agents and the app. When
  nothing can be done (less life than the payment, CR 119.4 / 118.3; no such card) there
  is no question and the effect simply applies.
- **What the answer causes is an event of its own, performed first.** Paid life is a
  `Proposed::LoseLife` and a discard a `Proposed::Move(Discard)`, each through a nested
  `Apply` frame pushed above the batch being modified: replaceable, they trigger
  ("whenever you lose life"), and they happen before the permanent enters. So for
  several permanents entering at once the second question sees the life already paid
  (614.12b).
- **The condition is judged as the event is modified**, for the player the permanent will
  enter under the control of (109.5, 110.2): on the battlefield as it was before the
  event, so the land never counts itself ("two or more other lands" is "two or more
  lands") and lands entering together don't see each other — the batch is performed only
  after all its events are modified. An "unless" whose condition holds is not an
  applicable effect at all (no pointless choice between replacement effects); it is
  checked again as it is applied. Entering tapped is not becoming tapped (110.5b: no
  `Tapped` event).
- **CR 614.13a:** a card being discarded for an effect that modifies how a permanent
  enters can't be the entering card or any card moving in the same batch. The same
  exclusion is applied to revealing — no rule says so, and no compiled card can tell the
  difference (no reveal land asks for a card of its own types).
- **Other permanents' entering** uses the existing `Replaces::OtherEnters` (the source must
  already be on the battlefield, so "permanents enter tapped" never affects itself, 614.12
  example). Only a permanent's own entering may have a condition or a choice; for others
  "you" would be a different player, and those forms are blocked.
- **Compiler:** `R:Event$ Moved | Destination$ Battlefield | ReplacementResult$ Updated`
  with `DB$ Tap | ETB$ True` (`ConditionPresent$`/`ConditionCompare$`,
  `ConditionCheckSVar$`/`ConditionSVarCompare$` — the conditions under which it is tapped,
  negated into "unless"; `UnlessCost$ PayLife<N>` / `Reveal<1/A;B>` with
  `UnlessPayer$ You`) or `DB$ PutCounter | ETB$ True`, on `Self` (or no `Defined$`,
  Forge's default) or `ReplacedCard` (with `ActiveZones$ Battlefield`). Mox Diamond's
  chain (discard, remembered; to the graveyard if none; to the battlefield if one;
  cleanup) is recognized only exactly. `Destination$ Graveyard` with a `ChangeZone` of the
  replaced card to exile or (shuffled, optionally revealed) into the library: "this
  creature would die" (`ThisDies`), "a creature an opponent controls would die"
  (`Dies`), "this card … from anywhere" (`ThisToGraveyard`, works in every zone,
  CR 113.6b). Expressions `PlayerCount[Players|Opponents]$Amount`,
  `…$HighestLifeTotal`, `…$LowestLifeTotal`.
- **Filters (general, not only here):** alternatives that differ in exactly one
  "at least one of" list — card types, subtypes or colors — and nothing else are one
  selector ("Island.YouCtrl,Mountain.YouCtrl"; before, only bare card types merged). This
  alone compiled about 120 more cards (Commander staples among them: Dockside
  Extortionist, Archaeomancer, the Verges). The engine's text now joins several colors or
  subtypes of one selector with "or". **Found while doing it:** Forge's `+` is "and", so
  `Card.Red+White` ("a spell that's both red and white", the Mimics) would have read as
  "red or white" — no compiled card had it yet; it is blocked now (`filter:two colors`).
- **Oracle guard:** a card that says "unless" must say it in the engine's text too
  (`oracle:says unless`). It blocks no current card.
- **Read against the Oracle text:** all 256 newly compiled cards. No mis-compilation; texts
  fixed: "the number of Islands you control is not 0" → "you control an Island", "an
  opponent controls 8 or more lands" → "your opponents control …" (the engine counts all
  of them together, as the cards mean), "Island Swamp card" → "Island or Swamp card".
  Swarmyard Massacre counts "Insect, Rat, Spider, or Squirrel" permanents where the card
  says creatures — Forge's script, compiled as written (a noncreature permanent of those
  types would count).
- **Heuristic agent:** reveals whenever it can; discards for a permanent that otherwise
  goes to the graveyard; pays life only on its own turn, with at least 10 life left after
  paying, not for a permanent that enters tapped anyway, and only if a card in its hand or
  command zone costs exactly one more than its untapped lands (the mana is wanted now).
  The app titles the question with the ability's text and labels the buttons "pay 2 life"
  / "No: it enters tapped" / "No: it goes to the graveyard".
- Golden hashes unchanged (state and event logs).
- 9,305 → 9,561 of 34,463 scripts (27.7%); precons 149 → 150 of 275 deck slots.
  `repl:Moved` fell from 135 cards blocked by it alone (353 in all) to none under that
  name; what is left is named by form.
- **Still blocked, by name:** "if this would enter, sacrifice … instead" (Lotus Vale and
  the other sacrifice lands, `repl:Moved enters instead`); "enters tapped with N
  [charge/depletion] counters" (`repl:Moved enters:Tap+SubAbility`, needs other counter
  kinds); "lands you control enter untapped" (`repl:Moved enters:Untap`); "as this
  enters, choose …" (`repl:Moved enters:ChooseType`, `…:ChoosePlayer`, `…:GenericChoice`);
  enters as a copy (`param:repl Moved.Layer`); day/night (`repl:Moved enters:DayTime`);
  "exile it instead. When you do, …" (Kalitas, Nemata: `repl:Moved to graveyard:
  ChangeZone+SubAbility`); "if a card would be put into an opponent's graveyard from
  anywhere" (`repl:Moved to graveyard:shape`: no vocabulary for other cards from anywhere);
  leaving the battlefield to anywhere but a graveyard, from the stack, discards caused by
  opponents (`repl:Moved`).
- **Next here:** the sacrifice-instead lands (an `AsEnters::Sacrifice` with a count), other
  counter kinds for the storage and Vivid lands, "lands you control enter untapped".

## D-068 Players other than "you": "its controller", "its owner", "that player" (2026-11-10)
Rules text: docs/rules-notes/608.2h-its-controller-that-player.md. Tests:
`crates/harness/tests/forge_their.rs` (three- and four-player games).
- **What a target was (CR 608.2h, 608.2b).** As resolution begins, the resolve frame
  records each legal target (`resolve::TargetInfo`: controller, owner, power, toughness,
  mana value with X on the stack, deathtouch, lifelink) and brings the record up to date
  before every step of the resolution while the target is still the object that was
  targeted, where it was targeted. Once it has gone, the record is how it last existed
  there — changed by an earlier instruction of the same spell, if one did ("target creature
  gets +3/+0, exile it, its controller gains life equal to its power" gains 8 from a 5/4).
  A target that was illegal as resolution began tells nothing: "its controller gains life"
  doesn't happen (608.2b's last sentence). This replaces D-063's `target_controllers`
  (who controlled the target as resolution began), which a control change or a pump
  earlier in the same resolution would have read wrong.
- **Vocabulary:** `PlayerRef::OwnerOf(slot)` ("its owner"; differs from "its controller"
  under a control change); `Amount::OfTarget { slot, what }` ("its power / toughness / mana
  value"); `Amount::HandOf(player)` ("the number of cards in that player's hand");
  `DamageBy::Target(slot)` ("target creature deals damage to itself equal to its power":
  the target is the source, with its lifelink and deathtouch); `Effect::PlayerMay { who,
  effect }` ("that creature's controller may draw a card": that player is asked, CR 608.2d;
  with `who` = you it is `May`); `ChooseCards::optional` ("its controller may search their
  library", "each player may search": each searching player decides for themselves,
  before looking; one who declines neither searches nor shuffles). `CreateToken` may name
  several players ("each player creates"): each owns and controls their tokens (CR 111.2).
- **Triggers carry "that player"** (`EventFacts::player`): who cast the spell (Forge's
  `TriggeredActivator`, `TriggeredCardController` of `SpellCast`), whose step it is
  (`TriggeredPlayer` of `Phase`), the defending player of an attacking, blocked or
  unblocked creature (`TriggeredDefendingPlayer`; CR 506.2 — creatures here attack
  players only). The player who drew or discarded is also `TriggeredCardController` /
  `TriggeredCardOwner` of `Drawn` and `Discarded` (the card is in their hand/graveyard).
- **"You" where the event can only be yours.** "That player" of an event only the
  controller's own events trigger ("at the beginning of your upkeep", "whenever you draw")
  is compiled as "you"; so is "its controller" of the ability's own source ("when this
  dies, its controller draws a card", CR 603.3a). Both are exactly the same player; the
  text now matches the Oracle's. A second (`Secondary$`) event added to such an ability
  must have the same property, else it is blocked (`trig:Secondary about another object`).
- **The compiler** resolves every player name of a script in one place
  (`Tx::named_player`): `Defined$`, `TokenOwner$`, `UnlessPayer$`, `DefinedPlayer$`, a
  trigger's `OptionalDecider$` and a draw's. Forge's draw asks the player who would draw
  whatever `OptionalDecider$` says; it is compiled only where it names that player
  (`param:Draw.OptionalDecider not the drawer`). A trigger whose decider is someone else
  and that also costs something is blocked (`trig:OptionalDecider with a cost`).
  `TargetedController`/`TargetedOwner` of a player or of a card in a graveyard is blocked
  (Forge finds no controller there). `AITgts$`/`AITgtsStrict$` are accepted as AI hints
  (never read by Forge's rules code); a test shows the card is identical without them.
- **Mis-compilations found (older than this batch), fixed:**
  - Opponents' cast triggers that hit "its controller" did nothing at all: the cast
    trigger's "it" was taken for an object the event doesn't carry (Ishi-Ishi, Isolation
    Cell, Spelltithe Enforcer). The spell is no "it" of a cast trigger now; "that player"
    is the caster.
  - "Each player / each opponent may search their library" was decided by the caster for
    everyone (Boldwyr Heavyweights, Jungle Wayfinder, New Frontiers, Noble Benefactor,
    Old-Growth Dryads, Veteran Explorer, Weird Harvest). Now each searcher decides.
  - Found reading the new cards: "that player may search" where the script has no
    `Optional$` but the Oracle says "may" (Volatile Fault, White Orchid Phantom) was being
    made the caster's choice; the searcher decides now. Goblin Tinkerer ("that artifact
    deals damage … to this creature", scripted as the creature damaging itself) and
    Commander Mustard ("it deals 1 damage to defending player", scripted as life loss) are
    now blocked by two new Oracle guards: `oracle:that one deals damage` and
    `oracle:says damage`. Selectors' "non[color]" was missing from the text (Soot Imp's
    "nonblack spell" read "spell"; the engine had it right).
- **New Oracle guards:** `oracle:another player` — the card says "its controller", "its
  owner", "that player", "defending player"… and the compiled card names nobody but you
  (blocks no compiled card today: a regression guard); the two damage guards above; "may
  pay" counts as answered by "unless … pays" (CR 118.12a: Smothering Tithe).
- **Hidden information:** a library being searched is shown only to the searcher, and only
  while searching — not while they decide whether to (view `shown`).
- **The heuristic agent** treats an effect given to someone else (its controller's life,
  token, card) as neither help nor harm when choosing a target; accepts "may" choices;
  pays an opponent's "unless that player pays" (a tested case). The app titles a "may"
  question with the card it is for (it can be an opponent's).
- Descriptions: an ability with several trigger events reads each in full ("enters or
  deals combat damage to a player", not "enters or player"); "you may have this deal …".
- Golden hashes unchanged.
- 9,305 → 9,520 of 34,463 scripts (27.6%) before D-067; precons 149 → 152 of 275 deck
  slots. All 215 newly compiled cards were read against their Oracle text. Rebased onto
  D-067 (9,560 alone): 9,776 (28.4%), precons 153 of 275. Two cards compile only in the
  combination (Cavalier of Dawn, A-Elderfang Ritualist), both read; nothing main compiles
  is blocked by this batch's guards. Among the most played: Swords to Plowshares, Path to Exile, Beast
  Within, An Offer You Can't Refuse, Rhystic Study, Solemn Simulacrum, Generous Gift, Swan
  Song, Smothering Tithe, Feed the Swarm, Assassin's Trophy, Stroke of Midnight, Chaos
  Warp, Sheoldred the Apocalypse, Consecrated Sphinx, Fecundity, Dictate of Kruphix.
- **Still blocked, by design or for later:** "creature defending player controls" and
  other selectors relative to a triggering player (`filter:.ControlledBy
  TriggeredDefendingPlayer/TriggeredTarget/TriggeredPlayer`, ~40 cards alone); "unless any
  player pays" (`unless:payer Player`); lasting effects about a player
  (`Effect:remembers TargetedController`); `TriggeredCardOwner` of a cast spell; `Targeted$`
  amounts with several targets; `TargetedAndYou`, `ParentTarget`.
- **Follow-ups (2026-11-11), found reading the rebased cards; fixed:**
  - **"Another target … card" in a dies trigger** could target the dying card itself (the
    ability's source is the old permanent; its card in the graveyard is a new object,
    CR 400.7). "Another" is now other than the card the source became when the ability
    triggered on its leaving (`triggers::this_card`, CR 400.7e/603.10a), for targets
    (when chosen and on resolution) and for cards chosen as it resolves (`Hidden$`).
    With no other card, the trigger has no target and is removed (CR 603.3d). Affected:
    Elderfang Ritualist, A-Elderfang Ritualist. Text: "another target Elf card" (was
    "target another …": the wording of 122 cards).
  - **"You may [X]. If you do, [Y]"** was "you may [X, then Y]": Y happened even when X
    had nothing to act on. `Effect::IfDone { first, then }` (Y only if carrying out X made
    an event happen, CR 608.2c/614.6), compiled for a trigger whose `OptionalDecider$`
    makes the "may" and whose text says "If you do" (or "… this way") once. Cards: Ob
    Nixilis the Fallen, Stockpiling Celebrant, Turntimber Ranger, Zealot il-Vec, Unlucky
    Cabbage Merchant, A-Elderfang Ritualist. Cards whose "if you do" follows a single
    instruction that always happens once chosen (Cream of the Crop, Fertile Thicket,
    Planar Atlas, Munda, Precognition: "you may look at …") or a target that makes the
    spell fizzle anyway (Energy Tap) were right before and are unchanged; "you may X, then
    Y" keeps doing both. Limit: a replacement that turns X into another event (614.6)
    still counts as X having happened.
  - **Who reveals and who puts a card onto the battlefield (CR 110.2a).** The engine was
    right — a card another player searches for or reveals from their own library enters
    under their control (no controller given = its owner, the player told to put it
    there) — but the text said "you reveal". It now says "its owner reveals the top card
    of their library. Then its owner puts …" (Chaos Warp; Goblin Guide, Clear the Land).
    Every compiled card that puts another player's card onto the battlefield (Path to
    Exile, Assassin's Trophy, Chaos Warp, the "each player searches" cards, Metamorphose…)
    gives it to the player who searched or revealed, as the cards say. A dig of another
    player's library onto the battlefield in which you choose is not compiled
    (`Dig:another player's cards onto the battlefield, chosen by you`; none compiled).

## D-070 Rules about players; playing cards from other zones; Theros gods (2026-11-10)
Rules text: docs/rules-notes/402-305-401.5-rules-about-players.md.
- **`AbilityDef::PlayerRule { who, rule, only_if }`**: a static ability that changes a rule
  for players rather than an object (CR 613.10, 613.11). `PlayerRule`: no maximum hand size,
  a maximum hand size / ±N (402.2), additional land plays / any number (305.2), hexproof and
  shroud on a player (702.11c, 702.18a), the top card of the library revealed / looked at
  (401.5), "you may play lands and cast [cards] from [your graveyard / the top of your
  library / exile]" (`PlayFrom`, 601.3, 116.2a), and "you may cast this card from your
  graveyard / from exile" (`PlayThisFrom`, the card's own ability, 113.6f).
- **Nothing is stored.** `engine_core::players` reads the rules in force off the abilities
  permanents have now (`layers::abilities`, CLAUDE.md invariant 4) and off lasting effects
  (`effects::lasting_abilities`) on every question, in timestamp order. A lasting effect may
  now carry a player rule: "you may play an additional land this turn" (Explore, Urban
  Evolution, Summer Bloom, Scale the Heights) is Forge's `Effect` with an `AdjustLandPlays`
  static, i.e. a rule until end of turn (611.2c).
- **Where the engine asked the hard-coded rule, it asks `players`:** the cleanup discard
  (`max_hand_size`), the land play (`land_play_left`: played < 1 + additional; a land from the
  graveyard or the top of the library is a land play), the zones the priority frame offers
  (`playable_elsewhere`) and `cast::can_begin` (`may_play_from`), targeting a player
  (`targeting::is_legal`: shroud; hexproof against an opponent's spell or ability — also on
  resolution, 608.2b). The cast itself is unchanged: timing, costs, commander tax.
- **What players see:** `PlayerPublic::library_top` — the top card, for every viewer when it
  is revealed, for its owner when they may look at it (the AI sees it through its
  `PlayerView`). A card a decision offers to play from a hidden zone is in the decider's
  `revealed`. `PlayerPublic::rules` lists the rules about each player. While a spell is being
  cast or an ability activated no top card is shown at all: CR 401.5 hides a *new* top card
  until the cast is complete, and the engine doesn't remember which card was on top when it
  began (deliberately less than the rules show for that moment, never more).
- **The app** shows a revealed / looked-at top card beside the library count (hover to
  enlarge, click to play it), labels "play X from your graveyard / from the top of your
  library", and lists the rules about a player behind an ⓘ next to their counts. The
  heuristic agent plays every land it is offered, so it uses extra land plays and lands
  from the graveyard or the top of its library.
- **Compiler:** a `Mode$ Continuous` line is a player rule when it has `SetMaxHandSize`,
  `RaiseMaxHandSize`, `AdjustLandPlays`, `MayPlay` or `MayLookAt`, or names a player in
  `Affected$` (`You`, `Player`, `Opponent`, `Player.Opponent`). Card alternatives of the same
  line given keywords ("you, planeswalkers you control and other creatures you control have
  hexproof", Shalai) become one static ability per alternative (the keywords are redundant
  where they overlap). `MayPlay` needs the card to be its controller's own (`YouOwn` /
  `YouCtrl`), and `TopLibrary` exactly when the zone is the library. **Choice made:** a
  `MayPlay` filter of a kind of card that isn't a land (`Creature`, `Merfolk`, `Instant`)
  is read as Oracle's "cast [kind] spells" — such a card is never played as a land, not even
  a land creature (Forge would let Dryad Arbor be played by Emperor Mihail's "Merfolk"
  if it were one); `Card`, `Permanent` and land types include lands ("play lands and cast
  spells"). `Colorless` is now a filter property (none of the five colors).
- **Theros gods:** `ContinuousEffect::RemoveTypes` (layer 4, "isn't a creature"; subtypes
  stay: a God that isn't a creature is still a God), `Count$DevotionDual`. CR 208.3: a
  permanent that isn't a creature has no power or toughness — now true of every layered
  view. CR 506.4: an attacking or blocking creature that stops being a creature is removed
  from combat (`GameAction::RemoveFromCombat`, `Event::RemovedFromCombat`, checked with
  the control sync after every step).
- **Static abilities that work from the graveyard** (CR 113.6b): `AbilityDef::FromZone
  { zone, ability }` — "as long as this card is in your graveyard [and you control a
  Mountain], creatures you control have haste" (Anger, Wonder, Brawn, Filth, Valor,
  Riftstone Portal). The layer walk takes them from the graveyards (`Key::FromZone`; "you"
  is the card's owner; off the battlefield a card has only its printed abilities, so no
  "has the ability" check is needed, only the condition). `EffectZone$ Battlefield` (the
  default, written out) is accepted on any `Mode$ Continuous` line; any other zone is named
  (`static:works from Stack`, `…Command`).
- `AddType$ AllBasicLandType` ("every basic land type in addition", Dryad of the Ilysian
  Grove, Prismatic Omen, Nylea's Presence).
- **A `Mode$ Continuous` line that only copies Forge AI hints** (`AddSVar$` of `AITapDown`,
  `HasAttackEffect`…) is no ability at all; such a line no longer blocks a card.
- **Blocker names:** `static:Continuous-nothing` is gone — a line from which nothing was
  compiled now names its unread parameters (`param:static Continuous.CanBlockAny`,
  `…GainsAbilitiesOf`, `…AddAllCreatureTypes`, `…RemoveCardTypes`, `…EffectZone`).
- **Deliberately still blocked:** alternative costs (`MayPlayWithoutManaCost` — Omniscience,
  One with the Multiverse, Dracogenesis; `MayPlayAltManaCost` — Bolas's Citadel) because
  paying them touches the cast frame's payment code another branch is reworking;
  `MayPlayLimit` (Muldrotha: once each turn per permanent type), `MayPlayDontGrantZone
  Permissions`, `MayPlayIgnoreType/Color`, `MayPlayPlayer`, `MayPlayWithFlash`; cards an
  effect remembers (`static:MayPlay remembered cards` — impulse draw); another player's
  zones (`static:MayPlay from every player's zone`); hand sizes and land plays that are
  counted (`static:SetMaxHandSize variable`, `static:AdjustLandPlays variable`); player
  keywords other than hexproof and shroud (`player keyword:…`: protection, `UntapAdjust`,
  `CantSearchLibrary`…); players chosen by a property (`filter:player …`); "has all
  activated abilities of …" (`…GainsAbilitiesOf`: Conspicuous Snoop, Necrotic Ooze);
  "every creature type" off the battlefield (`…AddAllCreatureTypes`, Maskwood Nexus);
  "can block any number / an additional creature" (`…CanBlockAny`, `…CanBlockAmount`);
  "loses all other card types and creature types" (`…RemoveCardTypes`,
  `…RemoveCreatureTypes`: Kenrith's Transformation, Darksteel Mutation).
- Golden hashes: one game changed (cmd-4p seed 1) — by CR 506.4: an animated land was
  attacking when the player whose permanent animated it left the game; it now leaves
  combat and deals no combat damage (one new event). The other 14 games: unchanged event
  logs and states. Re-blessed.
- 9,305 → 9,412 of 34,463 scripts (27.3%); precons 149 → 152 of 275 deck slots. All 107
  newly compiled cards were read against their Oracle text; famous ones now compiling:
  Reliquary Tower, Thought Vessel, Decanter of Endless Water, Exploration, Explore, Azusa,
  Oracle of Mul Daya, Crucible of Worlds, Ramunap Excavator, Mystic Forge, Courser of
  Kruphix, Gravecrawler, Shalai, Purphoros, Heliod Sun-Crowned, Anger, Wonder, Dryad of the
  Ilysian Grove. Text fixes found by reading them (no behaviour change): "colorless" was
  missing from spell kinds ("Spend this mana only to cast  spells"), and cost reductions
  read "Creature with power 4 or greater spells you cast" (now "Creature spells you cast
  with power 4 or greater").
- **Merged onto D-067/D-068** (main at 3e37f8d): 9,776 → 9,891 of 34,463 scripts (28.7%);
  precons 156 of 275 deck slots. No card of main's stopped compiling. Nine cards compile
  only in the combination, all read against their Oracle text (Anvil of Bogardan, Price of
  Knowledge, Rites of Flourishing, Mogis, Marang River Prowler, Infernal Reckoning,
  Leeching Sliver, Stoneshaker Shaman); the ninth, Clash of Realities, gives its trigger
  to Spirit *creatures* where the card says "all Spirits" and went into `SCRIPT_DIFFERS`.
  Tests of the combination: a land played from the graveyard or the top of the library
  is asked "as this enters" (D-067); additional land plays with lands that enter tapped;
  "its controller loses 2 life" reaches a player with hexproof or shroud (CR 115.10a: the
  player isn't targeted). The golden re-bless is unchanged on top of main: only cmd-4p
  seed 1, one `RemovedFromCombat` event more.
- **Next:** alternative costs on a permission (Omniscience, One with the Multiverse,
  Bolas's Citadel) once the cast frame's payment rework lands; `MayPlayLimit` (Muldrotha,
  a once-per-turn permission counted per static ability); impulse draw ("you may play
  that card this turn", effects that remember the cards they exiled); protection on
  players.

## D-066 Mana abilities that cost more than {T}, make several mana, count, reflect, or do more; the payment planner (2026-11-10)
Rules text: docs/rules-notes/605-mana-abilities-costs-and-yields.md. Tests:
`crates/harness/tests/forge_mana.rs` (one file per feature area from now on).
- **Vocabulary.** `ManaProduce` gained `Fixed(types)` ("{U}{B}", "{C}{C}{R}"),
  `Combination { of, count }` ("two mana in any combination of …", each mana chosen),
  `Counted { of, amount }` ("{B} for each Swamp you control", "X mana of any one color";
  counted as it resolves) and `Reflected { from, types }` ("any color that a land an
  opponent controls could produce", CR 106.7). `AbilityDef::Mana` gained `then`: what the
  ability also does as it resolves (CR 605.3b) — damage to, life loss and life gain of its
  controller only (the pain lands, Talismans, Ancient Tomb, The Great Henge). A mana
  ability's `Cost` may now hold mana, life, "sacrifice this" and one chosen sacrifice,
  with or without `{T}`; it must tap or sacrifice something, so nothing can be activated
  for ever. `ManaSymbol::Hybrid` (`{R/W}`, CR 107.4e) exists for the costs of mana
  abilities only (the filter lands); card-db refuses it anywhere else, and Scryfall
  printed costs with hybrid are refused as before. `ManaRestriction.sources`: "spend this
  mana only to activate abilities of artifact sources / an ability" — wherever the source
  is; `abilities` (Forge's `+inZoneBattlefield`) now really means permanents (CR 109.2:
  before, Dragon-restricted mana could pay a Dragon card's cycling from the hand).
  `Effect::AddManaOf { who, of, amount }`: a spell or non-mana ability adding mana of a
  type chosen as it resolves (Lotus Cobra's landfall, which is an ordinary trigger, CR
  605.5a; "add one mana of any color" when something enters); several types from one
  `DB$ Mana` ("R R R G G G") are a `Seq` of `AddMana`.
- **Engine: `frames/mana.rs`.** `mana_ways` lists every mana ability a player could
  activate now (`ManaWay`: tap, sacrifice-self, mana cost, life, chosen sacrifice and its
  candidates, what it makes, restricted, depends on the battlefield). Plain "{T}: Add one
  mana" abilities of a permanent are still merged into one way. A way that is nothing but
  a tap (and maybe "sacrifice this") is still done in one step (`tap_for_mana`, same
  events in the same order as before); anything else runs a `ManaFrame` (new
  `Frame::Mana`, boxed): choose what to sacrifice → choose which floating mana pays its
  own cost (hybrid colors first, then generic, asked only when it is a real choice) → pay
  (tap, `PayMana`, sacrifices and life through `ApplyFrame`) → choose the type of each
  further mana (`ChoiceKind::ChooseColor` over `OptionDesc::Mana`) → add the mana → its
  `then` runs in a `ResolveFrame::mana_ability` (no stack object, no targets). The same
  path serves priority (CR 605.3a) and paying (CR 601.2g), including payments while
  something resolves (D-063).
- **Payment feasibility (replaces D-021's / D-049's / D-052's matching).** `plan` decides
  whether a cost can be paid: one bipartite matching between units of mana wanted (each
  symbol of the cost — specific, hybrid, generic — and of the costs of the mana-costing
  abilities chosen) and units of mana that could be had (the pool, each permanent's
  merged free ways, what the chosen mana-costing or sacrificing abilities make). Around
  it: which mana-costing/sacrificing abilities to use (and how often: an altar as many
  times as there are creatures) is enumerated (at most 4,096 combinations), their
  sacrifices are a second matching together with the cost's own sacrifice; Treasures that
  the cost could also sacrifice are counted by kind as before (D-052); several mana of
  one chosen type are tried with each type for the first four such sources (D-049).
  **Conservative rules (D-019: never offer what can't be completed; under-estimating is
  allowed):** a mana ability's mana cost is paid only from the pool or from abilities
  that cost no mana (a chain of Signets each paying the next is not found); mana that
  can only be spent in certain ways never pays a mana ability's cost; an altar doesn't
  plan to sacrifice a Treasure, a permanent with a mana-costing ability, or (when
  Treasures are among them) what the cost itself could sacrifice; when life paid plus all
  self-damage of mana abilities would exceed the player's life, life-paying abilities
  aren't planned with. The planner assumes the best order (tap a mana creature before
  sacrificing it, count Cradle before sacrificing creatures); each option offered while
  paying is checked on the state it leads to — for a plain tap by updating the planner's
  snapshot, for anything that sacrifices, costs mana or life, or makes mana of open types
  by playing it out on a copy of the game (`simulate`) and planning again from there.
  Choices inside an activation (which creature, which floating mana, which type) are
  filtered the same way.
- **Deviation from the rules' procedure (not their outcome):** a mana-costing mana
  ability is offered once its cost is in the pool; the player taps the land first rather
  than in the middle of activating the Signet (CR 605.3a allows both).
- **Compiler.** `AB$ Mana` / `AB$ ManaReflected` go through `Tx::mana_ability`:
  `Produced$` letters with different types, `Combo` with `Amount$ n`, a counted
  `Amount$` (`Count$Valid …` only, no tapped/untapped filter, no sacrifice in the cost),
  `SubAbility$` of DealDamage/LoseLife/GainLife (no target), costs with mana, `PayLife`,
  `Sac<1/…>`, and hybrid tokens (`RW`). Blocked with names: `Mana:sub Draw` (drawing
  moves a card from a library: not a mana ability, CR 605.1a), `Mana:sub <api>`,
  `Mana:sub with a target`, `Mana:cost` (a mana ability that neither taps nor sacrifices:
  "{R}: Add {B}", "{1}{G}: Add one mana of any color"), `Mana:cost sacrifice N`,
  `Mana:counted` (other counts), `Mana:counted with sacrifice`, `ManaReflected:Is`,
  `ManaReflected:Produced`, `Mana:produced` (`Combo AnyDifferent`, `Chosen`, `Special …`,
  counted `Combo`), and the parameters `TriggersWhenSpent`, `AddsNoCounter`,
  `AddsCounters`, `AddsKeywords`, `PersistentMana`, `ActivationLimit`. Hybrid mana in
  anything but a mana ability's cost stays `cost:RW` etc.
- **New Oracle guard `oracle:{T}`:** the card says `{T}` more often than the compiled card
  does. Found two scripts that leave it out: Lotus Ring's given ability (newly compiled
  without it — would have been a repeatable Black Lotus) and Letter of Acceptance's
  "{2}, {T}, Sacrifice: Draw a card" (compiled wrongly before this batch). Both blocked.
- **Found by the soak (precon-4p, heuristic, 3 games in 40): a payment dead-ended.** Orb of
  Dragonkind ({1},{T}: two mana in any combination, Dragon-only) paid Lathliss's ability:
  after the Mountain paid the Orb's {1}, the copy-of-the-game check dropped the Orb's
  second (still untyped, restricted) mana, so the Orb was no longer offered. Untyped mana
  now keeps its restriction in the plan. Regression test
  `restricted_mana_of_open_types_still_pays`.
- **Found by the soak after merging D-070 (forge-4p, 2 games in 500):** an altar could
  plan to sacrifice the source of the ability being paid for while mana restricted to
  abilities of such sources was in the pool; once the source is gone that mana's use
  depends on last known information the payment doesn't keep (the engine panicked, then
  dead-ended). Mana abilities no longer sacrifice the source of what is being paid for
  (conservative), and restricted mana for something that is gone isn't counted on.
  Regression test `an_altar_doesnt_sacrifice_the_source_of_the_ability_being_paid_for`.
- **AI and app.** The heuristic agent pays with the cheapest way first (free taps, then
  Treasure, mana-for-mana, damage, life, sacrificing another permanent last), and when it
  has a "sacrifice a creature: add mana" ability it doesn't cast what it couldn't pay for
  without one. The app's auto-pay only taps for mana that costs nothing else; it labels a
  mana ability that does more than tap for one mana with its text.
- **Old cards unchanged:** with the new code, 60 games each of real-4p, cmd-4p, m2-4p
  (random) and 40 of real-4p (heuristic) ended exactly as with the old build; all 15
  golden event logs are unchanged. State hashes changed (the catalog serializes the new
  `then` field); re-blessed.
- 9,305 → 9,571 of 34,463 scripts (27.8%); real cards 9,225 → 9,489; precons 149 → 150
  of 275 deck slots. The `api:Mana` family fell from 356 cards blocked by it alone (708
  at all) to about 120. All 266 newly compiled cards were read against their Oracle text.
- **Next for mana:** "two mana of different colors" (`Combo AnyDifferent`), counted
  combinations (Axebane Guardian), mana-for-mana abilities without `{T}` (they need a
  loop guard), triggered mana abilities ("whenever a land is tapped for mana"), mana
  whose spending does something (`TriggersWhenSpent`, `AddsNoCounter`, `AddsCounters`:
  Path of Ancestry, Cavern of Souls, Opal Palace — mana that remembers its source),
  Bloom Tender (`EachColorAmong`), hybrid mana in spells.

## D-071 Cards with more than one face: double-faced cards, adventurers, omens, split cards (2026-11-10)
Rules text: docs/rules-notes/712-double-faced-cards.md (712, 202.3, 305.9, 903.4d–e),
701.27-transform.md, 716-adventurers-721-omens-709-split.md. In this edition casting a
face is CR 712.8, playing a land face 712.9, entering 712.10–712.11, "the same object"
712.15; adventurers are 716 and omens 721.
- **Vocabulary.** A `CardDef` is the card's front face (or, for a split card, its two
  halves combined); `CardDef::faces` holds its other faces, each a whole face of its own
  (name, cost, types, abilities, `granted`), and `CardDef::layout` says how they relate:
  `Layout::{Normal, Modal, Transform, Adventure, Omen, Split}`. The new fields are skipped
  when empty, so one-faced cards serialise as before.
- **Catalog.** `CardCatalog::add` gives every other face an entry of its own right after
  the card — face `n` of the card at `id` is `catalog.face(id, n)` = `id + n` — marked
  `face_of` (never a card: `find` skips it, decks can't hold it, `new_game` refuses it). So
  an ability written on a back face is named like any other,
  `AbilityId { card: <the face's entry>, index, granted }`, and `catalog.ability`,
  `layers::abilities`, triggers, replacement keys, mana sources and the views needed no
  special case. The face definitions stay nested in the card too, so `describe`, the
  validator and the Oracle guard work on a `CardDef` alone.
- **Objects.** `GameObject::face` (0 = front) says which face is up. It is non-zero only on
  the stack and the battlefield (CR 712.6a: anywhere else only the front face), which
  `perform_move` guarantees: a moved card is front face up unless the move says otherwise
  (`MoveZone::face`, `Proposed::Move::face`), except that a resolving spell becomes a
  permanent with the face it had on the stack (CR 712.10). `GameState::face_card(id)` is the
  entry of the face that is up and `card_def(id)` its definition; `layers::chars` starts from
  it (CLAUDE.md invariant 3), and `layers::chars_as(st, id, face)` gives the characteristics
  a card would have with another face up — what is evaluated before a face is cast or
  played (CR 712.8c, 716.3a, 721.3a, 709.3a) and what a permanent about to enter is checked
  against (CR 614.12: a land face's own "enters tapped").
- **Mana value** is `Characteristics::mana_value()`; the back face of a transforming card
  carries its front face's cost for it (`mana_value_from`, CR 202.3b / 712.6d).
- **Casting and playing.** `OptionDesc::CastFace { card, face }` and
  `OptionDesc::PlayLandFace { card, face }` (new variants at the end; `CastSpell` and
  `PlayLand` stay the front face). `CastSource::Face` evaluates that face — its cost,
  timing, targets, cost reductions and restricted mana (`Payee::SpellFace`) — and once
  announced puts the card on the stack with that face up, after which the frame's source is
  a plain `Card`. A land face uses the land drop. Which faces may be cast:
  `cast::castable_faces` — either face of a modal card (CR 712.8b, 712.9; from a hand or
  the command zone), the Adventure or Omen (not from exile, CR 716.3d), either half of a
  split card (CR 709.3; the whole split card is never cast). A transforming card's back
  face is never cast or played.
- **Transform** (CR 701.27): `Effect::Transform { what }`, performed as
  `GameAction::Transform`: the same object turns over (counters, damage, attachments stay;
  it doesn't enter, so nothing triggers on entering), gets a new timestamp (CR 613.7g) and
  emits `Event::Transformed`. Only a double-faced permanent transforms, never into an
  instant or sorcery face (CR 701.27d–e). CR 701.27g: an ability of the permanent that
  would transform it does nothing if the permanent transformed since the ability was put
  on the stack (`GameObject::transforms`, recorded on the stack object as
  `source_transforms` for double-faced sources only).
- **Adventure** (CR 716.3d): a spell cast as an Adventure is exiled as it resolves (not
  when countered) and marked `GameObject::adventure` = its controller, who may then cast
  the card — or play it, if it is a land — from exile, but not as an Adventure again. The
  mark moves with the card from the stack to exile only; it ends when the card leaves
  exile. **Omen** (CR 721.3d): shuffled into its owner's library as it resolves.
  "That has an Adventure" (CR 716.2a, Forge's `AdventureCard`) is
  `Selector::has_adventure`, read from `Characteristics::has_adventure` — true for an
  adventurer card whichever of its characteristics it uses now (Edgewall Innkeeper,
  Frantic Firebolt, Mysterious Pathlighter).
- **Split cards** (CR 709.4): the compiler makes the card's own definition the two halves
  combined — both names ("Fire // Ice", which is also the card's name in the library and
  decks, as at Scryfall), the combined mana cost (mana value and colors from it), every
  type — and the halves faces 1 and 2. Only halves that are instants or sorceries with
  nothing but their spell ability (and an additional cost) compile: an ability that works
  from a hand would be the whole card's (`split:half with other abilities`); Rooms
  (`split:Room`, CR 709.5), Fuse and Aftermath (`kw:Fuse`, `kw:Aftermath`) stay blocked.
  Known limit: the combined card has one name string, so "a card named Fire" would not
  match it (CR 709.4a); no compiled card names a split card's half.
- **Commander.** Color identity counts every face (CR 903.4d–e); deck validation refuses a
  face entry; the commander tax and identity are per card, and a modal commander may be
  cast with either face from the command zone.
- **Compiler.** `AlternateMode:` `Modal`, `DoubleFaced`, `Adventure`, `Omen`, `Split`
  compile when every face compiles, each face as a card of its own (its own SVars, and its
  own Oracle text for the Oracle guard; `unexplained:` and `invalid:` cover every face). A
  blocker on any face blocks the card. `SetState | Mode$ Transform` with `Defined$ Self`
  (Forge's default) is "transform this permanent"; other modes (`Flip`, turning face up or
  down, specializing), a target, or another object stay blocked (`SetState:mode …`,
  `SetState:target`, `SetState:not itself`). card-db checks that an Adventure/Omen card's
  other face is an instant or sorcery with that spell type.
- **Scryfall.** `ScryfallCard::card_faces`. `ScryfallData::get` also finds a card by its
  front face's name (a real card wins over art cards that share face names);
  `ScryfallData::face(name)` is one face as a card of its own, with its own picture for a
  two-sided card (cached as `<id>-<n>.jpg`). The printed-data cross-check now runs for
  cards with several faces, face by face, and compares the layout (`printed:faces`) — it
  never ran for them before (they weren't found by name). Cards cached before the faces
  were stored count as stale, so `fetch-cards` fetches them again. "Real cards per
  Scryfall" counts a card compiled by its full or its front face's name.
- **App and AI.** Option labels name the face ("cast Malakir Rebirth", "play Malakir
  Mire"); the board, stack and card pickers draw the face that is up; the enlarged card
  shows the other face of a double-faced card beside it (CR 712.4). The heuristic agent
  plays a land face when it has no other land to play and fewer than five lands (or the
  spell face can't be cast now), and judges a spell face as the spell it would be. Random
  deck building (`forge-Np`) checks every face's cost against the deck's colors.
- **Mis-rendering found by reading the sample:** `describe` of a trigger with several
  events dropped the subject of the later ones ("Whenever this permanent enters or dies"
  for Jerren's "enters or another nontoken Human you control dies"); the card was
  compiled right. Fixed: a later event about another subject is written out.
- Golden state hashes re-blessed: the stable hasher writes a tag for every skipped serde
  field, so the new fields change every state hash. All 15 event-log hashes unchanged.
- On its own (on D-065) this was 9,305 → 9,426 of 34,463 scripts (real cards per Scryfall
  9,225 → 9,342). All newly compiled cards were read against both faces' Oracle text.
- **With D-067 and D-068** (rebased onto them): 9,776 → 9,906 of 34,463 (real 9,691 →
  9,817). No card that compiled on either side stops compiling. By layout: modal 32 of
  101, transforming 19 of 399, adventurers 63 of 151, omens 5 of 16, split 6 of 128.
  Nine cards compile only in the combination, all read on both faces: the MDFC lands
  with "as this land enters, you may pay 3 life" (Fell the Profane, Witch Enchanter,
  Boggart Trawler, Pinnacle Monk, Razorgrass Ambush), adventurers and a transforming
  card whose other text needed those batches (Scalding Viper's "that player", Brightcap
  Badger, Curious Homunculus), and
  Ishgard, the Holy See (a land with an Adventure). A land face's own "as it enters"
  choice and "enters tapped unless" are read from the entering face (CR 712.9, 614.12);
  "its mana value" of a target (`TargetInfo`) uses `Characteristics::mana_value`, so a
  transformed permanent has its front face's (CR 202.3b) and a spell cast as an Adventure
  or a split half has that face's (CR 716.3b, 709.3b).
- **With D-070 and D-066** (merged): 10,163 → 10,300 of 34,463 (real 10,076 → 10,209).
  By layout: modal 33 of 101, transforming 22 of 399, adventurers 66 of 151, omens 5 of
  16, split 6 of 128. Seven cards compile only in the combination, all read on both
  faces: Growing Rites of Itlimoc, Storm the Vault, Mystic Skull (mana abilities that
  count, cost mana, or are given), Sea Gate Restoration ("no maximum hand size" and the
  pay-3-life land), Beanstalk Wurm and Beorn ("play an additional land"), Rosethorn
  Acolyte (an Adventure that adds mana). Letter of Acceptance, which compiled on this
  branch alone, is blocked by D-066's `oracle:{T}` guard, as on main. **A rule that lets
  a player play or cast cards from another zone judges its cards by the face that would
  be played or cast** (CR 601.3e, 712.8c, 712.9): "you may play lands from your
  graveyard" lets a modal card be played as its land face, not cast as its spell face;
  `players::may_play_face`, `targeting::card_matches_as`. A card sent on an Adventure is
  one more such permission (only as itself, CR 716.3d). Restricted mana is judged by the
  face being cast (`Payee::SpellFace`).
- **What is left, by kind** (cards blocked by it alone): transforming cards — disturb
  (`kw:Disturb`, 14), Sagas (`kw:Chapter`, 10), battles (`line:Defense`, 10), craft, and in
  all 36 day/night werewolves (`kw:Daybound`), 32 old werewolf conditions ("if no spells
  were cast last turn"), "enters transformed" (`param:ChangeZone.Transformed`),
  `trig:Transformed`; split cards — Aftermath 13, Fuse 8, Rooms. Still blocked as
  layouts: `Flip`, `Meld`, `Prepare`, `Specialize`, `Vengeance`.
## D-069 "Those cards": Forge's remembered cards within one resolution; conditions on one instruction (2026-11-10)
Rules text: docs/rules-notes/608.2c-400.7j-those-cards.md.
- **Vocabulary.** `Effect::Note(instruction)`: the cards the instruction moved — as the
  new objects they became (CR 400.7) — and the tokens it created are "those cards" for the
  rest of the resolution (of a Dig, only the cards put in its first place).
  `ObjRef::Noted` ("those cards", "that card", "it"), `Selector::noted` (`Some(true)`: one
  of them, `Some(false)`: not one of them), `Amount::Noted(selectors)` ("for each creature
  destroyed this way", "if it's a Spirit"), `Amount::NotedTotal(measure)` ("its mana
  value", "that creature's power"). `CardDest::Stay`: a search that leaves what it found
  where it is, for the instructions after it (Cultivate). `Effect::If { test, then }`: a
  condition on one instruction (Forge's `Condition…$` on a link: a failed condition skips
  only that link). `PutInto::controller` ("… under your control", Forge's
  `GainControl$ True`). Named apart from D-065's `Affected::Remembered` /
  `ObjFilter::Remembered`, which are what a *lasting effect* is about (an effect object's
  memory, which outlives the resolution); this is resolution-local and never stored on a
  card or effect.
- **Engine** (`frames/resolve.rs`, self-contained; `apply.rs` untouched): the resolve
  frame keeps `noted` (object, the zone this resolution last put it in, whether later
  instructions can find it, its characteristics just before it moved), `noting`, and
  `moving` (proposed zone changes, identified by card instance). Every `ApplyFrame` the
  frame starts records the moves it proposes; when the frame resumes it looks at what
  became of them: a card that moved is noted (if an instruction is being noted) or
  followed (if it already was). **Findable** (CR 400.7j): still the same object, in the
  zone this resolution put it in, and that zone is public — or it was found by a search
  and stayed. A card a replacement effect sent elsewhere isn't found (Ghostly Flicker
  doesn't return it from a hand) but is **counted** ("destroyed this way" counts a
  creature exiled instead by a Rest-in-Peace effect, as Forge's `destroy()` does; an
  indestructible one isn't destroyed and isn't noted). Amounts read a card as it is now if
  it is on the battlefield, else as it was just before it moved (CR 608.2h: the sacrificed
  creature's power is its last known power). Effects that take an `ObjRef` act on every
  noted card at once (Exile, Destroy, ReturnToHand, PutInto, PutCounters, Tap/Untap,
  Apply, damage). Each chosen mode starts with nothing noted (`Work::ForgetNoted`). A
  choice among noted cards in a library is not a new search (no `SearchedLibrary`, the
  library isn't shown).
- **When Forge's memory compiles as resolution-local** (`Tx::local_memory`): the chain
  (an ability and its `SubAbility$` links) has a `Remember…$` writer the compiler knows
  (`RememberChanged`, `RememberTargets` on the root move, `RememberLKI`, `RememberDestroyed`,
  `RememberSacrificed`, `RememberTokens`, `RememberMilled`, `RememberDiscarded`), its last
  link is its only `Cleanup`, with `ClearRemembered$ True` and no condition. Only inside
  such a chain (and not inside what an effect object carries) do `Defined$ Remembered`,
  `IsRemembered` / `!IsRemembered`, `Remembered$…`, `RememberedLKI$…`,
  `Count$RememberedSize` and `ConditionDefined$ Remembered` mean "those cards"; anywhere
  else they stay the blockers they were. Since every writer of a compiled card forgets at
  the end of its own chain, nothing else on the card can see the memory (Forge's memory
  lives on the card). A writer whose chain doesn't forget at its end blocks the card
  (`remember:not forgotten when the chain ends`), as do: a `Cleanup` that isn't last, a
  reader before anything is remembered, remembering twice in one chain (`DestroyAll`
  clears first in Forge), a search that puts found cards back in the library or never
  shuffles them away, a Dig that leaves what it takes in the library, a move of "those
  cards" from where they aren't (`Origin$`) or from a hand or library.
- **`Defined$ Targeted` after the target moved** (CR 400.7: the target slot is a card
  that is gone; Forge follows the card): the move is noted and the reference is
  `ObjRef::Noted` if the target went to a public zone; otherwise blocked
  (`defined:Targeted after it moved`). **Mis-compilation found, older than this batch:**
  Miraculous Recovery ("return target creature card … to the battlefield. Put a +1/+1
  counter on it") compiled before and put the counter on nothing. Grave Upheaval's haste
  likewise (newly compilable through `GainControl$`).
- **Conditions on links** (`ConditionPresent$`/`ConditionCompare$` on the battlefield,
  `ConditionDefined$ Remembered`, `ConditionCheckSVar$`/`ConditionSVarCompare$`) are
  compiled for every chain, not only remembering ones: "if you control a Wizard, draw a
  card", "if X is 5 or more, destroy all other creatures".
- **New Oracle guard** `oracle:under your control`: the compiled card puts something onto
  the battlefield "under your control" exactly when the card returns or puts something "to
  the battlefield under your control" ("enters the battlefield under your control" is old
  wording for "you control" and doesn't count). It caught Photon, Lady of Light
  (`GainControl$` where the card returns it under its owner's control) and blocks Meneldor
  (harmless there: "a creature you own").
- **Players of noted cards** (after D-068's `Tx::named_player`): `PlayerRef::NotedControllers`
  (Forge `RememberedController`) and `PlayerRef::NotNotedControllers`
  (`NonRememberedController`), each card's controller as it is now on the battlefield or
  as it last was before it moved (CR 608.2h). Plaguecrafter: "each player who can't
  [sacrifice] discards a card". card-db refuses them where one player is meant.
- card-db refuses "those cards" anywhere but a resolving effect, a selector asking about
  them except where the resolving effect filters with it, `Stay` outside a noted search,
  a noted instruction that isn't a single one, noted cards as what an Equipment attaches
  to or a lasting effect is about.
- AI: an effect that returns noted cards to the battlefield is help (blink your own).
- With D-067 (how a permanent enters): the moves the resolve frame proposes are looked at
  when it resumes, after the whole Apply — including its "as this enters" choice and any
  nested Apply for the price. A noted card entering through such a choice is found where
  it entered; one a replacement put elsewhere instead (Mox Diamond's graveyard) is not.
  Tests `a_noted_card_entering_with_a_choice_is_found`,
  `a_noted_card_put_elsewhere_instead_is_not_found`. With D-068: "its power" of a moved
  target still comes from `TargetInfo` (`Amount::OfTarget`); only object references
  (`Defined$ Targeted` as the thing acted on) follow the moved card.
- Save/load and clone of a game suspended in the middle of a noted resolution continue
  identically (`a_noted_resolution_saves_loads_and_clones`).
- **Engine bug found by the soak (older than this batch, newly reachable):** when a player
  leaves the game (CR 800.4a), the permanents they control but don't own are exiled after
  the layered view was worked out, without a new generation; the stale view still held
  them and the next control check panicked ("no object"). It takes a permanent of another
  player's under your control — reanimated with `GainControl$`. `actions::leave_game` now
  drops the derived layer cache after those moves (the serialized state, and its hash, are
  unchanged). Test `a_reanimated_permanent_is_exiled_when_its_controller_leaves`
  (mutation-checked: it panics without the fix).
- **With D-068's `Effect::IfDone` (merged 2026-11-11).** The two mean different things.
  `IfDone { first, then }` is "[first]. If you do, [then]": `then` happens if `first`
  changed anything at all (any event). `If { test, then }` is a condition counted as the
  instruction is reached — here, from Forge's `ConditionDefined$ Remembered |
  ConditionPresent$ … | ConditionCompare$ …`, how many of "those cards" (of a kind) there
  are. One Forge shape reaches both: an optional trigger whose description says "If you
  do" / "this way", whose chain notes cards and whose later link asks about them (Carrion
  Imp, Geothermal Kami, Lagonna-Band Storyteller, Mardu Woe-Reaper). It compiles to
  `IfDone { first: Note(…), then: If { noted …, … } }` — the two checks together, never one
  without the other, so no card can reach that meaning two ways. They differ only where the
  script's test is stricter than "something happened": "you may search … If you do, gain
  2 life" with nothing found has searched and shuffled (IfDone holds) but noted nothing
  (the script's `GE1` fails): no life, as the script says (test
  `you_may_search_if_you_do_with_noted_cards`).
- **With D-070 and D-066:** a rule about players whose source is exiled as its controller
  leaves stops at once (`a_rule_about_players_ends_when_its_controller_leaves`); a noted
  card later played from a graveyard under a "play lands from your graveyard" rule is a
  new object and the memory is gone with the resolution
  (`a_noted_card_played_later_is_no_longer_that_card`); counts of noted lands are of cards,
  not of the mana they make (`counting_noted_lands_not_their_mana`); a noted creature
  sacrificed to an altar's mana ability while paying "you may pay {2}. If you do, put two
  +1/+1 counters on it" is a new object this resolution didn't move and gets nothing
  (`a_noted_creature_sacrificed_for_mana_while_paying`).
- State hashes changed (`Selector` gained a field: the catalog is part of the state); all
  15 event logs unchanged. Re-blessed.
- Merged with main `4f75185` (D-066, D-068 follow-ups, D-070): 10,163 → 10,363 of 34,463
  (30.1%). None of main's cards stops compiling; one changes text (Miraculous Recovery).
  Letter of Acceptance, compiled on this branch alone, is blocked by D-066's `oracle:{T}`
  (it had lost its `{T}`). Two cards compile on neither parent and were read: Klothys, God
  of Destiny and Thassa, Deep-Dwelling.
- Merged with main `bbca45d` (D-071, faces): 10,300 → 10,508 of 34,463 (30.5%). None of
  main's cards stops compiling; one changes text (Miraculous Recovery). Nine faced cards
  compile on neither parent (Blex's "Search for Blex", Bloomvine Regent's "Claim
  Territory", Colossal Badger's "Dig Deep", Docent of Perfection, Dowsing Device, Hit //
  Run, Ravenous Demon, Stormshriek Feral's "Flush Out", Vincent Valentine); all were read.
  Vincent Valentine's Galian Beast ("return it to the battlefield tapped") had
  `GainControl$` where the card returns it under its owner's control: the
  `oracle:under your control` guard now reads "onto the battlefield tapped under your
  control" as well, and blocks it (`under_your_control_after_tapped_is_read_back`).
  "Those cards" are the same objects whichever face is up (transforming is no zone
  change, CR 701.27c), and a noted card in a library or hand is read by its front face
  (CR 712.8a): a search for a land card finds a pathway, not an instant whose back is a
  land (`a_noted_double_faced_card_is_found_by_its_front_face`).
- On `04ef403`: 9,305 → 9,491 of 34,463 scripts; precons 149 → 151 of 275 deck slots.
  Rebased onto D-067/D-068 (`3e37f8d`): 9,776 → 9,974 (28.9%). Of the 9,776 cards main
  compiled none stops compiling, and the engine text of one changes: Miraculous Recovery
  (the fix above). Of the cards named for this batch, Cultivate, Kodama's Reach, Fabled
  Passage, Ghostly Flicker, Cloudshift, Essence Flux, Displacer Kitten, Teleportation
  Circle, Flicker of Fate, Fumigate, Plaguecrafter, Bane of Progress, Dark Confidant,
  Martial Coup, Disciple of Bolas and Reanimate compile. All 198 cards that compile only
  in the combination were read against their Oracle text.
- **Still blocked, deliberately:** memory that outlives the resolution (handed to an effect
  object, `Effect:remembers Remembered…`: Professional Face-Breaker; linked abilities,
  CR 607; delayed triggers), `ForgetChanged$` (Nissa's Pilgrimage), `RememberedOwner`,
  Command Beacon (`ChangeZone:no-target` from the command zone, not memory), other
  `Remember…$` writers (`RememberRevealed`, `RememberDrawn`, `RememberDamaged`, …).
- **Next:** `ForgetChanged$`; `ChangeZoneAll` with `RememberChanged$` ("exile all …, then
  return them"); `Effect:remembers Remembered` (an effect object about noted cards: "exile
  the top card; you may play it this turn", 188 cards).

## D-078 Comment lines in Forge scripts: ignored as Forge ignores them, but each text is reviewed (2026-11-12)

Until now every `# …` line made its card uncompilable (`line:# TODO` was the single blocker
of 6 of the 1,000 most played Commander cards, among them Demonic Tutor). Two things were
established before changing that.

- **Forge ignores them.** `forge-core/.../card/CardRules.java`, `Reader.readCard`:
  `for (String line : script) { if (line.isEmpty() || line.charAt(0) == '#') { continue; } this.parseLine(line, this.faces[curFace]); }`.
  The test is on the raw line's first character, so only a `#` in column 0 is a comment;
  an indented `#` goes to `parseLine`, and a `#` inside a value is part of the value.
  `parse.rs` follows that: a column-0 `#` line becomes `Face::comments` (text kept, lossless);
  indented ones stay `unknown` lines (still blocked).
- **Comments are where authors write that a script is wrong.** 135 distinct texts (210
  lines, 198 cards). Most are about Forge's AI ("TODO: the AI will ..."), its internals ("X
  Will get overwritten by Announce") or are section markers ("--- VARIANTS ---"). 16 say or
  suggest that the script does not do what the card says ("Doesn't follow the strict wording
  of the Oracle text currently", "TODO: need EnchantedLKI because it isn't enchanted
  anymore if this is sacrificed", "This calculation isn't considering the remembered
  player", a bare "TODO" ...).

**Rule** (`crates/card-db/src/forge/comments.rs`, next to the other reviewed lists; kept in its
own file so the 130 entries don't sit in `translate.rs`): a comment is compared after dropping
the `#` and collapsing white space. It matches an entry exactly, or, if the entry ends in `*`,
by prefix (long comments are listed by their first 60 characters: a typo fixed later in a
sentence does not block the card, while a changed beginning does). In `HARMLESS` -> ignored.
In `INCOMPLETE` -> `script:differs from the card` (the same blocker as `SCRIPT_DIFFERS`,
raised by the comment instead of the card name, so a card gaining the same comment later is
caught too; the entry holds the comment's own words as the reason). In neither ->
`comment:unreviewed`. A Forge update that adds a comment therefore blocks that card until
someone reads the comment and classifies it. Cards with an `INCOMPLETE` comment, 21 of
them (`forge-coverage --why script:differs`): Dark Deal, Sycorax Commander, Hellfire, Cocoon,
Frozen Solid, Liege of the Hollows, Quillmane Baku, The Madcap Jester, Transmute Artifact,
Silent Gravestone, Helm of Obedience, Banish into Fable, Circus of the Sun, Elvish
Impersonators, Robot Chicken, Bolshack Dragon, the three EDH Silver errata cards (1996
World Champion, Celestine Cave Witch, Shichifukujin Dragon), plus the two found earlier by
reading. None of them compiled before this change (every comment blocked); only Robot Chicken
would compile now but for its comment, the others have further blockers, and the
entries keep them blocked when those go away.

**A hole this opened and closed.** `SVar:AFLifeLost:Number$0` + "# AFLifeLost will be set by
LoseLife": Forge's effects write results into SVars named `AF…`; the script holds only the
starting value. The compiler read `LifeAmount$ AFLifeLost` as the constant 0 ("Then X is
0. Then you gain X life"), so Blood Tithe, Exsanguinate, Kokusho, Agent of Masks, Subversion
-- and Scholar of Athreos, which already compiled this way -- would have gained no life.
`expr` now blocks any name `AF` + capital letter (`expr:set by an effect AFLifeLost`);
"life lost this way" is not built yet. Scholar of Athreos stops compiling (it was a wrong
card).

**Counts.** 10,300 -> 10,314 of 34,463 scripts (+15, -1). Top 100 by EDHREC 74 -> 75, top
1,000 516 -> 522, top 5,000 1,571 -> 1,580. New: Demonic, Diabolic, Grim, Mystical and
Personal Tutor, Imperial Seal, Natural Order, Aether Mutation, Aura Mutation, Consult the
Necrosages, Culling the Weak, Embodiment of Spring, Kuro Pitlord, Sawtooth Loon, Utopia Vow --
each read against its Oracle text. Most of the 57 `# TODO` cards stay blocked by something
else (Proliferate, Flashback, copy effects ...).

## D-072 Other ways to pay for a spell: alternative costs, flashback, evoke, overload, "pay X life" (2026-11-12)
Rules text: docs/rules-notes/118.9-alternative-costs.md. Tests:
`crates/harness/tests/forge_altcost.rs` (26).
- **Vocabulary** (appended at the end of their enums/structs): `AbilityDef::AlternativeCost
  (AltCost { kind, cost, only_if })`, `AltKind { Rather, Flashback, Evoke, Overload(Body) }`
  — `Rather` is "you may [cost] rather than pay this spell's mana cost" / "… cast this spell
  without paying its mana cost" (`cost.mana` none), `Overload` carries the spell's text with
  "target" changed to "each" (CR 702.96c). `Cost::exile_from_hand` ("exile N [cards] from
  your hand"), `Cost::return_to_hand` ("return N [permanents] you control to their owner's
  hand"), `Cost::life_x` ("as an additional cost, pay X life"). `Condition::ControlCommander
  { own }` ("if you control a commander" / "… your commander", CR 903.3d — also usable in
  static abilities: the Lieutenants), `Condition::CastFor(AltKindName::Evoke)` ("if its evoke
  cost was paid"), `Effect::ControllerSacrificesThis` (evoke's "its controller sacrifices
  it", whoever controls it by then; `SacrificeThis` needs the ability's controller).
- **Where it hooks into casting.** The choice of an alternative cost is its own priority
  option, `OptionDesc::CastWith { card, face, ability }`, offered right after the
  `CastSpell`/`CastFace` of the same card and face, one per alternative cost of that face in
  printed order (invariant 5; CR 601.2b, 118.9a: one alternative cost per spell). It is only
  offered when the cast can be completed with it (D-019): its zone (flashback: the owner's
  graveyard, CR 702.34a; everything else: wherever the card may be cast — hand, command zone,
  a `PlayFrom` rule), its condition (checked as casting begins), and the payability of the
  resulting cost. `CastFrame::alt` remembers it; `cast::proposal_with` replaces the mana
  cost with the alternative cost's and merges its other parts with the additional cost
  (`Cost::with`, CR 118.9d) **before** payment: the payment code and the planner (D-066) see
  an ordinary cost. Commander tax and cost reductions still apply (CR 903.8, 118.9d,
  tested); with no mana cost there is no X to announce, so X is 0 (CR 107.3b, tested).
  New logic is in `frames/altcost.rs`; `cast.rs` got two stages (`ChooseExile`,
  `ChooseReturn`, after `ChooseDiscard`) and the paying of those parts in `finish`.
- **What the spell remembers.** `StackObject::alt` (set by `GameAction::SetCastAlt` as the
  spell is announced) and, for a permanent spell, `GameObject::cast_with` on the permanent
  it becomes. Flashback (CR 702.34a) is a replacement effect in the pipeline: any move of
  that spell off the stack except to exile goes to exile instead (resolving, countered,
  bounced) — `replacement::applicable` names it by the flashback ability (`RepKey::Static`),
  so it competes with other replacement effects like any other. Overload's text is read from
  the stack object (`altcost::spell_body`, in the resolve frame). Evoke's sacrifice is an
  ordinary triggered ability the compiler adds to the card, with `CastFor(Evoke)` as its
  intervening "if" (CR 603.4) — it is lost with "loses all abilities", as the rules say.
- **"Those cards" (D-069) was not needed:** nothing here acts on a card the same resolution
  moved. Flashback's exile is a rule about the spell itself (a replacement, CR 702.34a), not
  a remembered card. "Exile the top card … you may cast it without paying its mana cost"
  (cascade, `Play` with `WithoutManaCost$`) is not built; when it is, it should name the
  card with `ObjRef::Noted` and cast it with an alternative cost of no mana.
- **Costs.** "Return an Island you control" may return a land tapped for mana for this same
  cost (CR 601.2g–h, tested); the planner keeps enough permanents for it as for a sacrifice,
  and card-db refuses a cost that both sacrifices and returns. Cards exiled from the hand
  are never the card whose cost it is; a script of an ability activated from the hand that
  doesn't say `Other` is blocked (`cost:ExileFromHand-maybe-itself`). "Pay X life": X is
  announced at the X stage, at most the caster's life (CR 119.4); only a spell's own
  `Cost$` may have it (`cost:PayLife-amount` elsewhere).
- **Restricted mana** looks at the spell, not at which cost is paid (CR 106.6, tested: mana
  for creature spells pays a creature's evoke cost and no instant's alternative cost).
- **Faces (D-071):** an alternative cost belongs to the face being cast (CR 601.2b, 712.8c):
  `altcost::alternatives(st, card, face)`, tested with an invented modal double-faced card.
  No compiled real card has that shape (the one multi-face script with an alternative cost,
  a transforming Saga, is blocked otherwise).
- **Compiler.** `S:Mode$ AlternativeCost` with `ValidSA$ Spell.Self` (or `Spell` +
  `ValidCard$ Card.Self`), `EffectZone$ All`, `ValidPlayer$ You`, `Cost$` (`0` = no mana),
  `Condition$ PlayerTurn/NotPlayerTurn`, `IsPresent$` (incl. `Card.IsCommander+YouCtrl`,
  `…+YouOwn`), `CheckSVar$`/`SVarCompare$`. `K:Flashback:<cost>`, `K:Evoke:<cost>`,
  `K:Overload:<cost>` (printed last). Overload is compiled the way Forge builds it
  (`CardFactoryUtil`): the spell's one instruction in its mass form, `ValidTgts$` becoming
  `ValidCards$`/`ChangeType$` (`Destroy`→`DestroyAll`, `ChangeZone` from the battlefield→
  `ChangeZoneAll`, `DealDamage`→`DamageAll`, `Tap`/`Untap`/`Pump`/`PutCounter`→`…All`).
  Costs `ExileFromHand<N/filter>` (types, subtypes, colors), `Return<N/filter>`,
  `PayLife<X>` in a spell's `Cost$`. Filter `YouDontCtrl` = `Who::Opponent`: in a game
  without teams every other player is an opponent (CR 102.3); the text says "an opponent
  controls". `Counter` with `Destination$ Graveyard` (Forge's default written out:
  Force of Will, Force of Negation and other counterspells).
- **The heuristic agent** takes a free alternative cost (+30 over the spell's score), a
  cheaper mana one (+5), overload (+15), flashback (−2); evokes only a card it can't cast;
  never an alternative cost that sacrifices, discards, exiles or returns cards (pitching for
  nothing), nor one paying life below 10 + the life; for "pay X life" it pays at most a
  third of its life. Exile-from-hand choices like discards, return choices like sacrifices.
  The app labels `CastWith` with the face and the ability's text ("cast Fierce
  Guardianship — If you control a commander, …") and titles the two new choices.
- **Texts fixed on the way (no behaviour change):** "unless you sacrifice a mountain" →
  "… a Mountain" (the unless-cost text lower-cased whole words; 11 cards), "creature card"
  → "creature card with power 2 or less" (card filters in a library or graveyard dropped
  `power_max`/`toughness_max` from the text; 3 cards, and the new Vesperlark).
- **Golden hashes:** re-blessed; every event log unchanged, all state hashes moved — the
  stable hasher writes a marker for each skipped field (`hash.rs` `skip_field`), so the new
  skip-if-empty fields change state hashes without changing a game.
- **Numbers:** on main `cda0566` 10,508 → 10,742 of 34,463 scripts; EDHREC tiers there,
  main → D-072: top 100 78 → 83, top 300 191 → 199, top 1,000 527 → 539, top 2,000
  869 → 887, top 5,000 1,605 → 1,646. Merged onto `ef30f04` (D-078): 10,524 → 10,758
  (31.2%); top 100 79 → 84 of 95 (88.4%), top 300 201 (68.1%), top 1,000 534 → 546 of 992
  (55.0%), top 2,000 895 (45.0%), top 5,000 1,657 (33.2%); no card compiles only in the
  combination. Among them: Fierce Guardianship,
  Deadly Rollick, Flawless Maneuver, Cyclonic Rift, Vandalblast, Toxic Deluge, Force of Will,
  Force of Vigor, Flare of Denial, Daze, Mulldrifter, Shriekmaw, Solitude, Faithless Looting,
  Dread Return, Deep Analysis, Think Twice, Strike It Rich, Mizzium Mortars, Electrickery,
  Momentary Blink, Unburial Rites, Lingering Souls. All newly compiled cards were read
  against their Oracle text; nothing that compiled before stopped compiling or changed
  meaning.
- **Still blocked, by name:** `api:ChangeTargets` (Deflecting Swat #74, Misdirection, Bolt
  Bend; CR 115.7: targeting abilities on the stack is not modelled, and 115.7a/b/d differ —
  next); `kw:Convoke`, `kw:Improvise`, `kw:Delve` (pay mana by tapping/exiling, after the
  total cost is locked in — they belong in the planner, D-066); `kw:AlternateAdditionalCost`
  (Demand Answers: one of two additional costs); `kw:Madness`, `kw:Foretell`, `kw:Escape`,
  `kw:Miracle`; reductions (`param:static ReduceCost.*`, `param:*.ReduceCost`: the channel
  lands Otawara, Boseiju); `static:AlternativeCost of other spells or abilities` (Dream Halls,
  equip/cycling costs), `param:Continuous.MayPlayAltManaCost` (Bolas's Citadel),
  `MayPlayWithoutManaCost` (Omniscience); `param:static AlternativeCost.CheckSecondSVar`,
  `.Announce`, `.XAlternative`, `.ManaRestriction`; `kw:Flashback with parameters`, flashback
  given to other cards (Snapcaster Mage, Past in Flames: `kw:Flashback`); overload of more
  than one instruction or of a player target.
- **Next here:** ChangeTargets (a target filter for spells *and abilities* on the stack;
  115.7d as a re-pick of each target slot with "keep" offered, groups as one pick of a set of
  the same size), then delve/convoke/improvise in the planner, then the channel lands'
  `ReduceCost`.

## D-075 The most played Commander cards no feature family covers, part 1 (2026-11-12)
Rules text: docs/rules-notes/d075-commander-staples.md. Tests:
`crates/harness/tests/forge_staples.rs`. Each feature is general (every card using it), in
EDHREC order.
- **"The life lost this way"** (CR 119.3, 608.2c): `Amount::LifeLostThisWay` (appended) —
  what the players' life totals actually went down by in the "lose life" instruction just
  before it (`Work::LifeLost` snapshots the totals; `ResolveFrame::life_lost`, serialized
  only when set). Forge's `AFLifeLost`, compiled only right after a `LoseLife` in the same
  chain (`expr:AFLifeLost not after a life loss` otherwise). D-078's guard on `AF…`
  variables stays for every other use (its test now uses another `AF` name); D-075 adds
  `SET_WHILE_RESOLVING`, the other variables Forge's code writes as it resolves
  (`PreventedDamage`, `AffectedX` — counted per affected card in a static —, `Votes`,
  die-roll results…): never read by their written value (`expr:set while resolving …`).
  Scholar of Athreos, which D-078 found and blocked, compiles again, now right; with it
  Exsanguinate, Gray Merchant of Asphodel, Kokusho, Blood Tithe, Agent of Masks,
  Subversion, Debt to the Deathless.
- **"In any order"** (CR 401.4): `Reorder$` on a `ChangeZone` to a library (Brainstorm,
  Brainsurge, Brainstone, Riverwise Augur, Congregation at Dawn, Dwarven Recruiter, Scouting
  Trek). The engine already has cards put on a library ordered by their owner; elsewhere
  `param:ChangeZone.Reorder not to a library`.
- **"Untap up to N lands"** (CR 701.26b): `Effect::UntapChosen { what, count }` and
  `ChoiceKind::Untap` (both appended) — anyone's permanents matching (Forge's `UntapType$`
  is not limited to yours), chosen as it resolves, not targets; only tapped ones are
  offered. Chosen by another player: `param:Untap.UntapUpTo by another player`. Frantic
  Search, Snap, Peregrine Drake, Palinchron, Great Whale, Treachery, Rewind, Unwind, Cloud
  of Faeries, Pore Over the Pages. The heuristic agent untaps its own permanents; the app
  titles the choice "Choose what to untap".
- **Proliferate** (CR 701.34a): `Effect::Proliferate`, `ChoiceKind::Proliferate`
  (appended): any number of permanents that have a counter, each gets one more of each
  kind, all in one replaceable event. Players have no counters in this engine (every card
  that gives poison, energy or experience is blocked), so only permanents are offered —
  when player counters arrive, proliferate must offer players too. `Amount$ N` is N
  proliferates in a row (a non-number is `param:Proliferate.Amount not a number`). 40
  cards: Atraxa, Karn's Bastion, Evolution Sage, Flux Channeler, Inexorable Tide… The
  heuristic agent picks its own permanents with +1/+1 counters and others' with -1/-1.
- Golden hashes unchanged (state and event logs).
- 9,891 → 9,968 of 34,463 scripts on base `0365ace`; merged with main `b2647ec` (D-072):
  10,827 (main alone: 10,758; +69, none lost, no engine text changed — a card-by-card diff
  of `forge-coverage --sample 1` against main's build). Top 100 by EDHREC rank 86 of 95
  (main: 84), top 1,000 56.1% (main: 55.0%), top 5,000 33.9% (33.2%). Every newly compiled
  card was read against its Oracle text.
- With D-072: a flashback spell's "you gain life equal to the life lost this way" counts a
  loss that takes an opponent below 0 in full (CR 119.3: an opponent at 1 life who loses 3
  has lost 3 — the Gray Merchant ruling); a land returned to hand to pay an alternative
  cost isn't offered to "untap up to N lands".
- **Next (in the coordinator's order):** Path of Ancestry, Myriad Landscape, delayed
  triggers (Arcane Denial, Mana Drain), pay X life, Ponder / Sensei's Divining Top
  (`api:RearrangeTopOfLibrary`), Channel lands, Esper Sentinel, City of Brass (`trig:Taps`),
  cumulative upkeep, variable charm counts, attack costs (Propaganda), Big Score, Mana
  Vault, regeneration / "can't be regenerated" (CR 701.19).

## D-073 Replacement effects that change how much; tokens enter the battlefield (2026-11-12)
Rules text: docs/rules-notes/614-doublers-603.2d-additional-triggers.md (with
614-616-replacement-prevention.md).
- **Tokens.** `Replaces::Tokens { token, effect_only }`: "if [an effect would create] one
  or more [creature] tokens [under your control]" — each kind of token of the event is
  matched as it would be created (its definition's characteristics, CR 111.3; controlled
  by its creator, 111.2). With `ReplaceWith::Scale(Calc)` ("twice that many"),
  `AddTokens { tokens, n }` ("those tokens plus a Food" / "plus that many Squirrels") and
  `ReplaceTokens(tokens)` ("that many 4/4 Angels instead", "instead create one of each").
  Only the kinds the effect applies to are scaled or replaced. A replacement can make one
  event create several kinds of token: `Proposed::CreateTokens.more` (serde default, not
  serialized when empty) keeps them one event, so a later "plus an additional Food" adds
  one Food (CR 614.5, 616.1f). `effect_only` only changes the text: every token in this
  engine is created by an effect.
- **Counters.** `Replaces::Counters { on, kind, effect_only }`: "if one or more [+1/+1]
  counters would be put on [a creature you control]" with `ReplaceWith::Scale` ("twice
  that many", "that many plus one", "minus one"); counters a permanent enters with count
  (CR 122.6, 614.16), matched as it would exist on the battlefield. Counters on players
  are blocked (`repl:AddCounter:counters on players`: the engine has none).
- **Order (CR 616.1):** unchanged pipeline — each effect applies once per event (614.5)
  and the affected player chooses among those that apply (`ChooseReplacement`), so "plus
  one" then "twice" gives 4 and the other order 3. The heuristic AI orders them by what
  they do: for what is good for it (its tokens, +1/+1 counters) add, then multiply, then
  shrink; for what is bad (damage, -1/-1 counters) prevent, shrink, multiply, then add.
- **Tokens enter the battlefield (a rules error on main, fixed here).** A created token
  skipped every effect that modifies how a permanent enters ("each Dragon you control
  enters with an additional +1/+1 counter", "creatures your opponents control enter
  tapped"). Now a `CreateTokens` event, once its own replacement effects are applied,
  becomes one `Proposed::TokenEnters { token, controller, tapped, counters }` per token
  (CR 616.1g: entering is contained in creating, so doublers are chosen first); those are
  modified like a card moved onto the battlefield (CR 111.6, 614.1c–d) — other
  permanents' `OtherEnters` and `Counters`/`PutCountersOnYourPermanents` effects, and the
  counters a token would enter with are doubled after an effect gives them (616.2). Each
  token is its own event, so its controller orders effects per token. `GameAction::
  CreateToken` carries `tapped` and `counters` (entering tapped is not becoming tapped).
  A token definition can't carry its own "this enters …" replacement (card-db rejects it;
  no Forge token script has one) — copy tokens of permanents that have one are the copy
  branch's to handle.
- **"As it would exist on the battlefield" (CR 614.12)** is now worked out on a copy of the
  game in which the entering has happened (`replacement::as_it_would_exist`): the permanent
  with the counters and tapped state the event has so far, its own static abilities and
  every continuous effect that would apply ("creatures you control are Dragons"). Used for
  tokens and for cards moved onto the battlefield (`OtherEnters`, `Counters`); before, a
  moving card was judged by its printed characteristics.
- **Numbers saturate:** `Calc` was already saturating; counts on objects, damage marked,
  commander damage and life changes now saturate (`actions.rs`: `life_amount` caps a
  damage/life amount at `i32::MAX`, `change_life` adds saturating; `layers.rs`: counters
  to P/T saturate), and one event creates at most `MAX_TOKENS_AT_ONCE` = 10,000 tokens of
  a kind (`frames/apply.rs`) — a deliberate limit far above any board a game is played to
  (the harness abandons a game at 1,000 permanents).
- **Triggering an additional time (CR 603.2d)** is not a replacement effect:
  `AbilityDef::AdditionalTrigger { sources, cause }` with `TriggerCause::{Enters, Dies,
  Attacks}` (Forge's `Mode$ Panharmonicon`). Counted where a trigger is queued
  (`triggers::queue_caused`): each detector says what made the ability trigger
  (`Cause::{Entered, Died, Attacked, Other}`); every such ability of a permanent on the
  battlefield whose `sources` matches the trigger's source and whose cause matches adds one
  instance — two effects give three instances, each put on the stack on its own (own
  targets, own order). For a look-back trigger of a dying permanent, the effects of
  permanents that left in the same batch count as they last existed (Teysa dying with the
  others). Compiled: no `ValidMode` (any trigger of matching sources: Annie Joins Up,
  Delney, Katara, Twinflame Travelers); `ChangesZone[,ChangesZoneAll]` + `ValidCause` +
  `Destination$ Battlefield` (Panharmonicon, Yarok, Ancient Greenwarden, Naban) or `Origin$
  Battlefield | Destination$ Graveyard` (Teysa); `Attacks[,AttackersDeclared…]` + `ValidCause`
  (Isshin). Blocked: other modes (`static:Panharmonicon mode …`: life gained, damage,
  cast, becomes target, turned face up, rooms), `ValidZone`, `ValidPlayer`, conditions.
- **"Can't be countered" (CR 101.2, 614.17)** is not a replacement effect either:
  `AbilityDef::CantBeCountered { spells }` — `None`: the spell's own ability (Forge's
  `R:Event$ Counter | ValidCard$ Card.Self | ValidSA$ Spell | Layer$ CantHappen`); `Some`:
  a permanent's "[creature] spells you control can't be countered" (`ValidSA$
  Spell.<filter>` or `ValidCard$ <filter>`). `Effect::Counter` does nothing to such a
  spell (`replacement::cant_be_countered`); the countering spell resolves and does the rest
  of what it does. Blocked: `ValidCause` (only by some spells), abilities that can't be
  countered (`repl:Counter:ValidSA …`), conditions (`CheckSVar`, `IsPresent`), effects
  (`repl:Counter:shape`).
- **Damage that is doubled or increased:** `Replaces::Damage { by, to, combat }` (D-065)
  now also takes `ReplaceWith::Scale` — Forge's `DB$ ReplaceEffect | VarName$ DamageAmount |
  VarValue$ ReplaceCount$DamageAmount/<math>` (Twice, Thrice, Plus.N, Minus.N, HalfDown):
  Dictate of the Twin Gods, Furnace of Rath, Angrath's Marauders, Fiery Emancipation,
  Twinflame Tyrant, Thor, Tomik, Calamity Bearer, Goldnight Castigator, Inquisitor's
  Flail, Lashknife Barrier, Ghosts of the Innocent. Each damage event is one source to one
  recipient, so doubling applies per recipient; combat damage is assigned (trample)
  before; lifelink gains what is dealt; the damaged player (or the damaged permanent's
  controller) orders several effects (CR 616.1). `DamageTo::Either { player, permanent }`
  is "an opponent or a permanent an opponent controls" (also in triggers). Sources:
  `Card,Emblem` is any source and `Card.X,Emblem.X` a source that is X — **an emblem
  alternative is read only where it mirrors a card alternative**, because the engine has
  no emblems (when emblems arrive, `Replaces::Damage.by` must include them). The same
  reading of sources and recipients now serves "prevent all damage" too (four more
  prevention cards compile: Blessed Sanctuary, Endure, Light of Sanction, Safe Passage).
  Blocked: `RedSource` and other `…Source` properties (`filter:.RedSource`, Torbran),
  prevention with a `ReplaceWith` (`PreventionEffect$`, shields that count), redirection
  (`DamageTarget$`), `inZoneBattlefield` on a source (Gratuitous Violence).
- **Life gain:** `Replaces::GainLife(Who)` + `Scale` (Rhox Faithmender, Boon Reflection,
  The Wind Crystal, Angel of Vitality, Heron of Hope, Honor Troll, Knight of Dawn's Light,
  Pest Rescuer). CR 120.4c: lifelink's gain is a result of the damage that replacement
  effects modify — `apply::perform` returns it, when an effect would modify it, as a batch
  of its own that runs right after the damage batch (before anything else; triggers of
  both are put on the stack together); without such an effect it is gained with the
  damage as before, so no game without one changes. A doubler with "whenever you gain
  life, you gain 1 life" is a real loop.
- **Oracle guards:** a card whose Oracle text says "can't be countered" or "an additional
  time" and whose compiled text doesn't is blocked (`oracle:says can't be countered`,
  `oracle:says an additional time`). No compiled card is caught by them now.
- **Deliberately still blocked:** "you may" token replacements (`param:repl
  CreateToken.Optional`: Flitwing, Jinnie Fay), tokens of a chosen permanent (`repl:
  CreateToken:chosen token`), tokens under another player's control (`…Type
  ReplaceController`), conditions on these effects (`…CheckSVar`, `…IsPresent`,
  `…PlayerTurn`), counters on players (`repl:AddCounter:counters on players`: Winding
  Constrictor), "if you would put counters" (`param:repl AddCounter.ValidSource`: Halving
  Season), draw replacements (`repl:Draw`: one draw replaced by several, or by winning —
  needs an event that becomes several; Alhammarret's Archive, Notion Thief, Laboratory
  Maniac), mana replacements (`repl:ProduceMana`: for D-066's `frames/mana.rs`), "lose
  life" replacements, prevention that counts or redirects (`PreventionEffect$`,
  `DamageTarget$`). A token that is a copy (D-074) or a double-faced token is out of
  scope here: its own "enters" effects would need the copy's characteristics.
- **"Those cards" (D-069):** tokens are created by `GameAction::CreateToken` and reported
  as `TokenCreated`; the moves an Apply performs don't include them, so Forge's
  `RememberTokens$` stays blocked.
- Golden hashes: unchanged (no golden setup has any of these effects; lifelink's gain only
  moves to a batch of its own when an effect would modify it).
- 10,524 → 10,610 of 34,463 scripts on main ef30f04 (86 cards, every one read against its
  Oracle text, none lost); merged onto main 12504fd (D-072): 10,758 → 10,845. EDHREC tiers
  on 12504fd → this branch: top 100 88.4% (84 of 95, unchanged), top 300 69.8%, top 1,000
  55.0% → 57.2% (546 → 567), top 5,000 34.1%. Soaks: forge-4p 300 random-agent games and
  200 heuristic games (one abandoned at 1,000 permanents: Horn of Gondor's Human Soldiers),
  precon-4p 100 games, no failures.
- **Merged onto main 3fc33f1 (D-075 part 1):** 10,827 → 10,914 scripts (87; no card lost,
  no compiled text changed; Pearl Lake Ancient compiles only in the combination). Tiers
  3fc33f1 → this branch: top 100 90.5% (unchanged), top 300 70.2% → 71.9%, top 1,000
  56.1% → 58.3% (557 → 578), top 5,000 33.9% → 34.7%. Proliferate puts its counters as
  `Proposed::PutCounters` through `Frame::Apply`, so counter doublers apply to it (tested);
  "you gain life equal to the life lost this way" is doubled by a life-gain doubler, the
  loss isn't (tested; effects on losing life aren't built).
- **Order with flashback (D-072):** main's "exiled instead" for a flashback spell is about
  a spell leaving the stack; the effects here are about tokens being created, a permanent
  entering, counters, damage and life — never the same event, so they never compete under
  CR 616.1. "Can't be countered" holds for a spell cast with flashback too (tested).
- **Next:** draw replacements (an event that becomes several: "draw two instead",
  "exile the top card instead", "you win instead"; "except the first one you draw in each
  of your draw steps"); mana doubling in `frames/mana.rs` (Nyxbloom Ancient, Mana
  Reflection); Torbran (`RedSource`); "you may" token replacements; Winding Constrictor
  once players have counters.
## D-076 "As this enters, choose a creature type / a color"; "the chosen type"; changeling (2026-11-12)
Rules text: docs/rules-notes/614.12a-607.2d-chosen-type-color.md. Tests:
`crates/harness/tests/forge_chosen.rs`, `crates/app/tests/chosen.rs`.
- **Vocabulary** (appended): `ReplaceWith::Choose(ChoiceOf)` ("as this enters, choose …",
  with `Replaces::ThisEnters`), `Effect::Choose(ChoiceOf)` ("choose a creature type." while a
  spell resolves), `Selector::chosen: Option<OfChosen>` ("of the chosen type" / "that
  aren't of the chosen type" / "of the chosen color"), `ContinuousEffect::AddChosenType`
  (layer 4), `Keyword::Changeling`, `AbilityDef::CostReduction::anyone` ("creature spells of
  the chosen type cost {2} less": anyone's spells). `card_model::chosen` holds CR 205.3m's
  creature types (`CREATURE_TYPES`, sorted; `CreatureType` is an index serialized as its
  name), `ChoiceOf { CreatureType, Color }`, `Chosen { CreatureType, Color }`.
- **The choice is made in D-067's place:** `replacement::entry_choice` asks it for a
  `ReplaceWith::Choose` (always — there is always something to choose), as the new
  `ChoiceKind::Choose { what, object, ability }` over `OptionDesc::CreatureType` /
  `OptionDesc::Color`, of the player the permanent will enter under the control of (also
  when an effect puts it there). `ApplyFrame::chosen` keeps the answer, by the entering
  object, until the batch is performed; `perform` then gives it to the new object
  (`GameAction::SetChosen`, `Event::Chose`) — only if it did enter the battlefield. No
  stack, no priority (CR 603.6d); enters-triggers see the choice (it is set before triggers
  are looked for).
- **Where a choice lives:** a permanent's on the object (`GameObject::chosen`, a new object
  has none, CR 400.7; public: `ObjectView::chosen`, the app's enlarged card shows "Chosen
  type: Elf", the log "P0 chooses Elf for X"); an ability's on its stack object
  (`StackObject::chosen`, copied from its source when it is put on the stack — the same value
  once the source has left, CR 608.2h); a resolving spell's own choice in its resolve frame
  (`ResolveFrame::chosen`, CR 608.2c: no object keeps it). Mana restricted to "a creature
  spell of the chosen type" keeps the value it was made with (`RestrictedMana::chosen`; two
  units are alike only if that is too).
- **One way to read it:** a selector that refers to a choice is *bound* before it is matched
  (`chosen::bind`): the reference becomes the chosen subtype or color itself, so layers,
  triggers, replacement effects, costs, targets, player rules and resolution all compare the
  same way. `layers::matches` binds with the choice of the ability's source; the places that
  match without it bind first (trigger filters and cast triggers, damage/dies/enters
  replacement filters, cost reductions, graveyard and spell targets, "may cast from the top
  of your library" rules, conditions, mana restrictions with the mana's own value); a
  resolving instruction is bound from the frame (its own choice first, else its source's).
  A reference left unbound means nothing was chosen (CR 607.5a): "of the chosen type"
  matches nothing, "not of the chosen type" everything (as Forge).
- **Linked abilities (CR 607.2d):** an object keeps one choice of each kind, read by the
  abilities of the same object. card-db refuses a card that refers to a choice none of its
  abilities makes (`invalid:`), and the compiler names what isn't read: a dies/leaves trigger
  of the source that refers to it (`chosen:in a trigger on its source leaving`), a static
  ability that works from another zone, abilities given to other objects, discard costs,
  mana restrictions of "not the chosen type", of several kinds at once or on an ability that
  sacrifices its source (`chosen:…`), a choosing ability of a permanent (Forge keeps the
  last choice on the card for its other abilities: `ChooseType:in an ability of a
  permanent`), `Animate:Types ChosenType`.
- **The options** (CR 205.3m: any creature type may be chosen): every type, those among the
  chooser's own cards first — counted over all their cards, library included (their own
  deck is known to them) — most common first, then the rest alphabetically. Colors: the five,
  WUBRG (CR 105.4). The app shows a search field and the first 24 matches in that order
  (Enter takes the first); the heuristic agent picks the type (or color) most common among
  its own creatures and creature cards it can see, ties and nothing seen to the engine's
  order. The random agent picks any of the 367.
- **"Is the chosen type in addition"** (`AddChosenType`, layer 4): only a creature or kindred
  gains it (CR 205.3d).
- **Changeling** (CR 702.73a): `Characteristics::all_creature_types`, set from the printed
  keyword (so in every zone, and before anything else in layer 4, CR 613.3: losing all
  abilities later doesn't undo it); `has_subtype` answers yes for every creature type of
  205.3m. Only the printed keyword is compiled: changeling given by another effect wouldn't
  be a characteristic-defining ability (CR 604.3a).
- **Compiler:** `K:ETBReplacement:Other:<SVar>` with `DB$ ChooseType | Type$ Creature` or
  `DB$ ChooseColor` (`Defined$ You` or none; `ETB$ True` accepted); `SP$`/`DB$ ChooseType` and
  `ChooseColor` in a spell; filter properties `ChosenType`, `IsNotChosenType`, `ChosenColor`
  (alone: with a subtype or color beside them → `filter:.ChosenType with another`);
  `AddType$ ChosenType`; `K:Changeling`; `ReduceCost` with no `Activator$` (Forge: any
  player); `DestroyAll | Zone$ Battlefield` (Forge reads nothing else); a `ChangesZone`
  trigger without `TriggerZones$` whose filter needs the chosen type (only a permanent has a
  choice, so it can only trigger there). `ETBReplacement` forms are now named by their API:
  `kw:ETBReplacement:Copy` (D-074's), `kw:ETBReplacement:ChoosePlayer`, `…:ChooseNumber`,
  `…:GenericChoice`, `…:<api>+SubAbility`; `ChooseType:Card`, `ChooseType:Basic Land`,
  `param:ETBReplacement ChooseColor.Exclude` (Thriving lands), `…ChooseType.ValidTypes`.
- **New Oracle guard** `oracle:says chosen`: the card says "chosen type", "chosen color" or
  "choose a creature type" ("of that type" read as "of the chosen type") where the compiled
  card doesn't, or the reverse.
- **Text:** "As this permanent enters, choose a creature type.", "… you control of the
  chosen type", "creatures that aren't of the chosen type", "spells of the chosen type cost
  {2} less" (anyone's). Fixed while reading: "Each another creature you control enters
  with …" → "Each other …" (four cards, text only).
- **Read against the Oracle text:** all newly compiled cards. `For the Ancestors` takes
  *creature* cards of the chosen type where the card says "cards of the chosen type" (a
  Kindred card of that type): `SCRIPT_DIFFERS`.
- **Golden hashes:** all 15 event logs unchanged; every state hash moved, because the stable
  hasher marks each field skipped by `skip_serializing_if` (`hash.rs`, `skip_field`) and the
  catalog's selectors now have one more such field (`Selector::chosen`), as do objects, stack
  objects and frames. Re-blessed.
- **Hidden information:** the order of the type options comes from the chooser's own cards,
  hand and library included; only the chooser sees the decision (`PlayerView::decision`),
  and the events tell everyone only the value chosen (tested).
- **A delayed triggered ability** that refers to the chosen type would read its source's
  choice when it triggers, not the one fixed as it was created (CR 603.7c); no card that
  otherwise compiles needs it: `chosen:in a delayed trigger`.
- **Numbers** (on main 6cc8595: 10,991 of 34,463): 11,071 compile (32.1%), +80, none lost.
  By EDHREC rank: top 300 219 → 224, top 1,000 586 → 601, top 2,000 957 → 977, top 5,000
  1,747 → 1,782. Among them: Doom Cannon, Plague Engineer, Patchwork Banner, Unclaimed Territory,
  Secluded Courtyard, Three Tree City, Vanquisher's Banner, Kindred Discovery, Urza's
  Incubator, Metallic Mimic, Adaptive Automaton, Icon of Ancestry, Pillar of Origins,
  Realmwalker, Kindred Dominance, Kindred Judgment, Distant Melody, Crippling Fear, Raise the
  Palisade, Cover of Darkness, Chronicle of Victory, Etchings of the Chosen, Bloodline
  Pretender, Species Specialist, Belbe's Portal, Engineered Plague, Xenograft, Hall of
  Triumph, the changelings (Changeling Outcast, Taurean Mauler, Universal Automaton, Crib
  Swap…), cards that make changeling tokens (Black Market Connections, Irregular Cohort,
  Personify), Helm of Awakening and Arcane Melee (anyone's cost reduction). Precon deck
  slots: not measured.
- **Tokens and copies (D-073, D-074):** `ApplyFrame::chosen` keeps a choice by the place of
  its event in the batch, and `perform` gives it to the permanent a `Move` *or* a
  `TokenEnters` puts onto the battlefield. But `replacement::applicable` offers a
  permanent's own "as this enters" abilities only for a `Move` (from the face the card
  enters with): a token entering (`TokenEnters`, no object yet) is not asked today, so a
  token copy of a permanent with an as-enters choice would enter with no choice (its "of the
  chosen type" then means nothing, CR 607.5a). For copies, `applicable` would have to read
  the entering object's copiable abilities (layer 1) rather than its card.
- **Still blocked, by name:** Cavern of Souls (`param:Mana.AddsNoCounter`: "that spell can't
  be countered" as a property of the mana; the restriction half is built and tested), Roaming
  Throne (only `kw:Ward` left: its additional trigger of the chosen type is built and
  tested), Utopia Sprawl / Caged Sun / Gauntlet of Power / Heraldic Banner (`Mana:produced`
  "of the chosen color" and mana triggers), Herald's Horn (`api:PeekAndReveal`), Door of
  Destinies (charge counters), Path of Ancestry (`param:Mana.TriggersWhenSpent`), Kindred
  Summons (`api:DigUntil`), Kindred Charge (`api:CopyPermanent`), Coat of Arms
  (`expr:set while resolving AffectedX`), Arcane Adaptation (`…AffectedZone`), Stuffy Doll
  (choose a player), Nykthos / Nyx Lotus (a permanent's ability choosing a color, devotion to
  it), Thriving lands (a color other than one), Cloud Key / Serra's Emissary (a card type),
  Multiversal Passage / Banner of Kinship (`R:Event$ Moved` with a choice and more).

## D-075 part 2: share a land type, delayed triggers, "draw up to", look and rearrange (2026-11-12)
Rules text: docs/rules-notes/d075-commander-staples.md (sections 205.3i, 603.7, 401.4 /
701.24a). Tests: `crates/harness/tests/forge_staples.rs`.
- **Cards that share a land type** (CR 205.3i): `Effect::ChooseCards::share_land_type`
  (serialized only when set; `Work::ChooseCards::partial` likewise). The cards are chosen one
  at a time, each further one sharing a land type with each chosen before it — Forge's rule
  (for two cards the same as having one in common). Myriad Landscape.
- **Delayed triggered abilities** (CR 603.7): `Effect::DelayedTrigger { ability, window,
  player, amount }` (the ability is an entry of `CardDef::granted`; `DelayWindow::{Next,
  ThisTurn, NextTurn}`), `GameState::delayed` (`effects::Delayed`, serialized only when not
  empty), `GameAction::{AddDelayed, RemoveDelayed}`. Detected with the step triggers
  (`triggers::detect_step`); each triggers once and is removed (603.7b); "this turn" ones
  that didn't trigger end at cleanup (514.2); "next turn" ones can't trigger before it. Its
  source is the spell's or ability's source (603.7d–e; a spell's is gone by then, so it
  carries the source as it was). What it remembers is fixed as it is created (603.7c): a
  player — "its controller" of a countered spell (`RememberObjects$ RememberedController`
  after `RememberTargets$`) — or an amount — that spell's mana value on the stack, X
  included (`RememberNumber$` after `RememberCounteredCMC$`). The delayed trigger is created
  whether or not the spell was countered — Forge's CounterEffect remembers the mana value
  before trying to counter ("should remember even that spell cannot be countered"), and a
  Mana Drain aimed at a spell that can't be countered still gives the mana. (Corrected after
  the merge: it was first compiled as "if you do", from a claim about Forge not checked
  against its Java. Mana Drain's text now says "that spell's mana value".) Compiled:
  `Mode$ Phase` only (`DelayedTrigger:mode …` otherwise); remembering objects is not
  (`DelayedTrigger:remembers objects`: 195 cards in all — "exile it, return it at the beginning of the
  next end step" needs D-069's memory to outlive the resolution); "this turn" over two steps
  unless the card says "next" (603.7b would make it trigger at each:
  `DelayedTrigger:this turn, more than once`).
- **"Draw up to N cards"** (Forge's `Draw | Upto$`): `Effect::DrawUpTo`, `ChoiceKind::DrawUpTo`
  (PickNumber; the drawer chooses). The heuristic agent draws all; the app asks "How many cards
  do you draw?".
- **Look at the top N cards and put them back in any order; shuffle** (CR 401.4, 701.24a):
  Forge's `RearrangeTopOfLibrary` compiles to a "look at the top N" that takes none and puts
  the rest on top in the controller's order; `MayShuffle$` to `May(Effect::Shuffle)`
  (appended; "you may have that player shuffle"). `AIRespondsToOwnAbility$` is a Forge AI
  hint. Ponder, Sensei's Divining Top, Index, Natural Selection, Portent, Halimar Depths.
- **`CostDesc$` is text only** (how the cost reads in Forge's description; the cost is
  `Cost$`). Big Score, Unexpected Windfall, Tormenting Voice, Cathartic Reunion.
- **Golden state hashes re-blessed; all 15 event logs unchanged.** The stable hasher writes a
  tag for a field that `skip_serializing_if` skips (`hash.rs`, `skip_field`), so a new
  top-level `GameState` field changes every state hash even while it is empty
  (`GameState::delayed`). Fields skipped inside frames (`ResolveFrame::life_lost`,
  `ChooseSpec::share_land_type`) only show while a resolution is suspended.
- Against main `0b956d9` (D-073 merged), by a card-by-card diff of `forge-coverage --sample 1`:
  77 cards newly compile, none stops compiling, no engine text changes. All 77 were read
  against their Oracle text. Among the most played: Arcane Denial, Mana Drain, Ponder,
  Sensei's Divining Top, Myriad Landscape, Big Score, Unexpected Windfall.
- **Deliberately not done here:** Path of Ancestry (`Mana.TriggersWhenSpent`: mana that
  remembers a trigger needs the player to choose which mana pays — CR 601.2h — inside the
  payment planner, and "shares a creature type with your commander"; left until the cast
  frame is quiet); War Room (pay life equal to a count, `ActivationGameTypes$`,
  `Count$ColorsColorIdentity`).
- **Next:** regeneration with "can't be regenerated" (`api:Regenerate` alone blocks 185
  cards, `NoRegen` 61), the Channel lands, Esper Sentinel, City of Brass, cumulative upkeep.

## D-079 Regeneration and "can't be regenerated" (2026-11-12)
Rules text: docs/rules-notes/701-regenerate.md (CR 701.19, 614.8, 701.8, 704.5f–h, 702.2b,
701.21a, 506.4). Tests: `crates/harness/tests/forge_regen.rs`.
- **A regeneration shield** (CR 701.19a): `Effect::Regenerate { what }` makes one as the
  spell or ability resolves (`GameAction::AddRegenerationShield`), on a permanent still
  there. It is an `effects::Shield` with `regenerate` set (serialized only when set),
  used up once, and gone at cleanup with the prevention shields ("this turn", 514.2).
- **A destruction-replacement effect** (CR 614.8, invariant 14): in `replacement::applicable`
  a regeneration shield applies to a `Proposed::Move` to the graveyard with
  `MoveCause::Destroy` of a permanent on the battlefield — destroy effects and the
  state-based actions for lethal damage and deathtouch (701.8b, 704.5g–h) — and turns it
  into `Proposed::Regenerate` (appended), performed as `GameAction::Regenerate`: all damage
  removed, tapped by its controller, removed from combat if attacking or blocking (506.4),
  then `Event::Regenerated` (appended; narrated). Sacrifice (701.21a), toughness 0 (704.5f)
  and other moves to the graveyard are not destruction and are never replaced. Prevention
  shields ignore regeneration shields and vice versa. Several applicable shields are chosen
  among by the affected player like any replacement effects (616.1).
- **"It can't be regenerated"** (CR 701.19c), as a rider: `Effect::CantBeRegenerated(Box<Effect>)`
  around a `Destroy` / `DestroyAll` (Forge's `NoRegen$`, tested for presence by
  DestroyEffect/DestroyAllEffect) makes its destruction `MoveCause::DestroyNoRegen`
  (appended; `MoveCause::destroys()` covers both for indestructible). As a rule:
  `AbilityDef::CantBeRegenerated(Affected)` (appended; Forge's `Mode$ CantRegenerate`),
  a static ability of a permanent or carried by a lasting effect ("target creature can't be
  regenerated this turn"); read by `replacement::can_be_regenerated`. Shields can still be
  made; they just aren't applied.
- **Blocked, by name:** "when it regenerates this way" (`param:Regenerate.RegenerationAbility`,
  4 cards), static regeneration (CR 701.19b, Forge `R:Event$ Destroy | Regeneration$ True`:
  `repl:Destroy`, 3), "can't be regenerated" about creatures dealt damage this turn
  (`static:CantRegenerate-who`: `Creature.DamagedBy`), regenerating the enchanted creature
  (`defined:card Enchanted`, a blocker of many effects, 17 Regenerate cards among them).
- The heuristic agent counts regenerating as help (aimed at its own permanents).
- **Golden: one state hash re-blessed (cmd-2p seed 1), all 15 event logs unchanged.** That
  game ends with a prevention shield in force, and the stable hasher writes a tag for the
  skipped `Shield::regenerate` field (`hash.rs`, `skip_field`).
- Against main `6410af8`/`6252f08` by a card-by-card diff of `forge-coverage --sample 1`:
  230 cards newly compile, none stops compiling, no engine text changes. All 230 were read
  against their Oracle text. Among them Pongify, Rapid Hybridization, Terminate, Putrefy,
  Damnation, Terror, Vendetta, Jokulhaups, Obliterate, Plague Wind, Thrun, Lotleth Troll,
  Masticore, Sedge Sliver, the Slivers that give regeneration. Text left as it is: Knight of
  the Mists reads "destroy target Knight creature. It can't be regenerated unless you pay
  {U}" (the meaning is right — unless you pay, it is destroyed and can't be regenerated —
  the word order isn't).
- 11,071 → 11,301 of 34,463 scripts (32.8%); top 1,000 60.6% → 61.4%, top 5,000 35.7% →
  36.1%.
## D-074 Copies of permanents: enter as a copy, become a copy, token copies (2026-11-12)
Rules text: docs/rules-notes/707-copies.md. Tests: `crates/harness/tests/forge_copies.rs`.
- **Copiable values** (CR 707.2) are `copy::Copiable { card, except }`: one catalog entry
  (a card, a token definition or a face — CR 707.8: a double-faced permanent is copied as
  the face that is up) plus the exceptions of the copy effects that made them, in order,
  each with the card it is written on (`copy::Exception`). An object that is a copy carries
  them in `GameObject::copy` (serialized only when set). **Layer 1a is one hook:**
  `copy::base(st, id, face, controller)` is where `layers::chars_as` and the layer walk
  start; it is the printed face for anything else. Everything downstream follows from it:
  abilities (named by the copied card's `AbilityId`s; the walk's `Key::Static` now names the
  `AbilityId`, not a printed index), triggers, replacement effects, mana value (a copy of a
  transformed back face has no mana cost: 0, CR 202.3b), the name for the legend rule.
- **Fixed when the copy effect begins** (CR 707.2b): changing or removing the original
  changes nothing. A copy of a copy copies its values, exceptions included (CR 707.3,
  707.9a–b). Not copied because they aren't copiable values: counters, status, Auras,
  other continuous effects, being a commander (CR 903.3), what was chosen as it entered.
- **Exceptions** (`CopyExcept`): not legendary, has keywords, types in addition, base P/T
  (a characteristic-defining ability of the original that sets P/T is not copied, CR
  707.9d), colors, creature types, "has [abilities of the `granted` list]".
- **Enters as a copy** (CR 707.5, 614.1c): `ReplaceWith::EntersAsCopy { of, except,
  tapped }`, a choice inside the replacement pipeline (D-067's `ChoiceKind::AsEnters`):
  the permanents matching `of` on the battlefield before it enters, or `Decline` (it enters
  as itself). The answer sets `Proposed::Move::copy`; then the copied text's own "enters"
  effects apply — "enters with counters", "enters tapped", "as this enters, you may pay 2
  life" and D-076's "choose a creature type" (CR 707.6: the copy makes its own choice) —
  because `replacement::entering_abilities` reads the event's copy. A copied
  enters-as-a-copy ability applies too (CR 707.9e example) unless it is the very ability
  that applied already (CR 614.5: a Clone copying a Clone that copied nothing stays a 0/0
  Clone; noted as a limit).
- **Becomes a copy** (CR 707.4): `Effect::BecomeCopy { what, except }` — for good; the
  same object, no enter/leave, its other effects stay. "Until end of turn" is not compiled.
- **Token copies** (CR 707, 111.12): `Effect::CopyPermanent { what: CopyWhat, count,
  except, gains, tapped }`; `CopyWhat::Obj(ObjRef)` (a target, this, "equipped/enchanted
  creature" = new `ObjRef::Host`, "it", D-069's "those cards") or `Each(Selector)` ("for
  each token you control"). They are created through D-073's path: `Proposed::CreateTokens`
  carries `copy: Option<TokenCopy>` for its first kind, so doublers double them (and a
  substituting replacement drops the copy), and each `TokenEnters` is modified like any
  permanent entering — with the copied "enters" replacement effects and choices (source
  `copy::ENTERING_TOKEN`, the token not being an object yet). `gains` ("that token gains
  haste") is a noncopiable continuous effect begun as the token is created. Copying
  something that is gone makes nothing (CR 111.12); the "copy of what it last was" forms
  (`…LKICopy`) are blocked.
- **Views and app:** `ObjectView::card` of a copy is the copied card (its picture and text);
  `ObjectView::printed` is the card or token it is itself. The app's enlarged card says
  "Test Clone, a copy of …" / "A token copy of …". The heuristic agent copies the most
  valuable permanent offered. The narrator says "X becomes a copy of Y" (`Event::BecameCopy`).
- **Compiler:** `K:ETBReplacement:Copy:<SVar>:Optional` + `DB$ Clone | Choices$ …` (with
  `IntoPlayTapped$`, the copy modifiers); `AB$/DB$ Clone` with a target or `Defined$` and no
  `Duration$`/`CloneTarget$`/`Choices$`; `CopyPermanent` with `ValidTgts$`/`Defined$`,
  `NumCopies$`, `NonLegendary$`, `AddKeywords$`, `AddTypes$`, `SetPower$`+`SetToughness$`,
  `SetColor$`, `SetCreatureTypes$`, `PumpKeywords$` (no `PumpDuration$`), `TokenTapped$`.
  **New Oracle guards:** `oracle:says except` (a sentence with "copy" and "except" whose
  compiled card has no exception) and `oracle:says gains until end of turn` (a token copy
  that "gains haste until end of turn" where the script makes it lasting: Devastating
  Onslaught, Molten Duplication). Pinned in `guard_phrases_are_still_written`.
- **Deliberately blocked, by name:** "sacrifice / exile it at the beginning of the next end
  step" (`CopyPermanent:AtEOT (a delayed trigger about the token)`: D-075's delayed
  triggers don't remember objects yet — 45 scripts, 22 by it alone; Kiki-Jiki, The Fire
  Crystal, Twinflame, Feldon, Molten Duplication, Splinter Twin…); Spark Double and Altered
  Ego (`Clone:SubAbility`: an additional effect, CR 707.9e); "becomes a copy until end of
  turn" (`param:Clone.Duration`: Cursed Mirror, Mirage Mirror, Shifting Woodland); "except
  it has this ability" (`param:Clone.GainThisAbility`: Thespian's Stage) and added triggers
  (`param:Clone.AddTriggers`: Phantasmal Image); Populate; LKI copies (Mirrorworks,
  Miirym); Myriad, Encore. **Copies of spells and abilities** (`api:CopySpellAbility`,
  `kw:Storm`, Replicate, Casualty, Conspire) are not built: the engine has the groundwork
  (`GameObject::spell_copy`, a copy of a spell ceases to exist off the stack, CR 704.5e;
  a permanent spell copy becomes a token, CR 608.3f) but no effect creates one.
- **Double-faced permanents** (CR 707.8, 707.8a — the text covers every double-faced
  permanent, modal ones included: the token copy has both faces): a token copy of one is
  double-faced (`Copiable::faces`; the token is the card with the original's face up, its
  values follow its own face, exceptions applied to each), so it transforms; a copy of one
  with its back face up enters back face up. A permanent that enters or becomes a copy is
  not double-faced: it copies the face that is up (mana value 0 for a back face, CR 202.3b).
- **Regeneration (D-079):** the shield is the object's, not a copiable value; a copy has the
  copied "{W}: Regenerate this creature" and regenerates itself (tested).
- **Blocker bookkeeping:** `Tx::blockers` counts every raise (`Raised`), so a second ability
  blocked by a name an earlier one raised is no longer reported `unexplained:` (Orthion).
- **Limits:** (1) "if this would enter, you may [discard a land] instead; if you don't, put
  it into its owner's graveyard" is not applied to a token copy: it enters with no question
  (comment in `replacement::applicable`). (2) A Clone copying a Clone that copied nothing:
  CR 707.5 says the copied "enters" replacement abilities "will take effect", and the 707.9e
  example has Altered Ego choose again through the Clone ability it copied. Here a copied
  enter-as-a-copy ability that is written on *another* card is offered (as in that
  example), but the very ability that already applied — Clone copying Clone, the same
  `AbilityId` — is not offered a second time (CR 614.5 read as "the same effect"): the
  second Clone enters as a 0/0 Clone and dies, where a reading of 707.5 by analogy would let
  it choose again. (3) A copied "creature types are …" replaces all non-land subtypes.
- Golden hashes: all 15 event logs unchanged; state hashes moved (the stable hasher writes
  a tag for each skipped `GameObject` field). Re-blessed.
- 11,071 → 11,123 (on main 31fa7b2: 11,301 → 11,353) of 34,463 scripts (52 newly compiled, none lost, no existing text
  changed); top 1,000 by EDHREC rank 601 → 606 (on 31fa7b2: 609 → 614), top 5,000 1,782 → 1,809 (1,800 → 1,827). All 52 read against
  their Oracle text. Among the most played: Helm of the Host, Second Harvest, Sculpting
  Steel, Vesuva, Clone, Irenicus's Vile Duplication, Pack Rat, Relm's Sketching,
  Mirrormade, Copy Enchantment, Glasspool Mimic, Stunt Double, Quicksilver Gargantuan.
- **Next:** delayed triggers that remember objects (unblocks the 22 "next end step"
  copiers), copies of spells and abilities with "you may choose new targets" (CR 707.10c)
  and storm, "except it has this ability".

## D-080 The last gaps in the EDHREC top 100: Channel lands, "becomes tapped", "first … each turn" (2026-11-12)
Rules text: docs/rules-notes/d080-top100.md. Tests: `crates/harness/tests/forge_top100.rs`.
- **D-079 follow-ups:** regeneration from combat damage is tested (a regenerated blocker or
  attacker leaves combat in the first-strike step and deals no damage in the regular step;
  the attacker stays blocked: CR 506.4, 509.1h, 510.4) — nothing needed fixing. Knight of the
  Mists is in `SCRIPT_DIFFERS` (its script destroys target Knight *creature*; a Kindred
  Knight is a Knight too, CR 308.2). "You may pay {U}. If you don't, destroy … It can't be
  regenerated." now reads that way, not as "… can't be regenerated unless you pay".
- **Abilities that cost less to activate** (CR 602.2b, 601.2f, 118.7a):
  `AbilityDef::Activated::reduce` (serialized only when set) — this much less generic mana,
  counted for the controller as the total cost is determined (`cast::ability_reduction`;
  offered only when the reduced cost can be paid). Forge's `ReduceCost$` (CostAdjustment);
  with `ReduceAmount$` (other mana) it blocks. The Channel lands (Boseiju, Otawara, Eiganjo,
  Takenuma-style), Voldaren Estate, Mirror of Galadriel, Jodah's Codex. A discarded source
  deals its damage as it last existed (113.7a, tested). **Filter:** `hasABasicLandType` is
  any of the five basic land types (CR 205.3i); text "land card with a basic land type".
- **"Whenever this becomes tapped"** (CR 603.2e, 701.26a): `Trigger::BecomesTapped(ObjFilter)`
  (appended). Tapping is no replaceable event, so the tap action itself detects it
  (`triggers::detect_tapped`, called from `GameAction::Tap`) — a mana ability's cost included
  (the trigger waits for the spell being cast, 603.3), an effect, attacking. Only untapped
  permanents are tapped, and entering tapped isn't becoming tapped (tested). Forge's
  `Mode$ Taps` (`ValidCard$`; `Attacker$`, `ValidCause$`, `FirstTime$`, `Teamwork$` block).
  City of Brass and 55 more (Auras that punish tapping, Pilots, Merfolk).
- **"Their first noncreature spell each turn"** (CR 601.2i): `PlayerState::cast_this_turn`
  (each spell as it was when cast, `GameAction::NoteCast`; serialized only when not empty;
  cleared as a turn begins) and `Trigger::Cast::nth` (Forge's `ActivatorThisTurnCast$ EQn`;
  other comparisons block). **"Unless that player pays {X}, where X is …"** (CR 118.12,
  107.3): an unless-cost `{X}` is paid as the X the resolution worked out (`UnlessCost$ X`),
  also the announced X of "counter target spell unless its controller pays {X}". Esper
  Sentinel, Monologue Tax, Tuvasa, Rodeo Pyromancers, Valeria Richards, Clash of Wills,
  Spell Rupture, Thassa's Rebuff, Mindswipe, Overrule.
- Not in this batch, and what each needs: **Deflecting Swat** — changing a spell's or
  ability's targets (`api:ChangeTargets`: re-choosing each target slot with "keep" offered,
  CR 115.7) and targeting abilities on the stack. **Path of Ancestry** — mana that carries a
  trigger ("when that mana is spent to cast …", `Mana.TriggersWhenSpent`): the payment
  planner must let the player choose which mana pays (CR 601.2h) and remember which unit
  was spent on what, plus "shares a creature type with your commander". **The One Ring** —
  protection from everything (CR 702.16), burden counters (counter kinds beyond +1/+1 and
  -1/-1, CR 122), "if you cast it" (`wasCastByYou`), and a trigger with no stated zone.
- Golden: state hashes re-blessed — the new `reduce` field of every activated ability in
  the catalog and `PlayerState::cast_this_turn` are written as skipped fields by the stable
  hasher; event logs unchanged (see the commit).
- Against main `d718b5b` by a card-by-card diff of `forge-coverage --sample 1`: 89 cards
  newly compile, one stops compiling (Knight of the Mists, on purpose), no existing text
  changed. All 89 were read against their Oracle text.

## D-081 Delayed triggered abilities that remember objects; copies of Auras (2026-11-12)
Rules text: docs/rules-notes/603-delayed.md. Tests: `crates/harness/tests/forge_delayed.rs`
(and the Aura/Equipment cases in `forge_copies.rs`).
- **One mechanism, extended** (D-075's `Effect::DelayedTrigger` / `GameState::delayed`):
  `Effect::DelayedTrigger::objects: Option<ObjRef>` names what the delayed ability is about;
  as it is created the objects are fixed (CR 603.7c) into `effects::Delayed::objects`. When
  it triggers they travel as `PendingTrigger::remembered` → `StackObject::remembered`, and as
  it resolves they are its "those cards" (D-069's `ObjRef::Noted`) — each only while it is
  still that object: a card that changed zones is a new object (CR 400.7) and is not
  affected. Serialized in the game state (save/load tested); nothing lives in a card or a
  Rust local. A delayed ability about objects of which none is left isn't created.
  Controller: the spell's or ability's (CR 603.7d); one whose controller left the game never
  triggers (CR 800.4a, tested).
- **Compiler:** `RememberObjects$ Remembered | RememberedLKI` in a chain that notes what it
  moved (D-069) → the chain's "those cards"; `RememberObjects$ TriggeredNewCardLKICopy` in a
  dies trigger → the card the creature became. In the delayed ability
  `Defined$ DelayTriggerRemembered[LKI]` is "those cards". `CopyPermanent | AtEOT$
  Sacrifice|Exile` → `Seq[Note(CopyPermanent), DelayedTrigger { objects: Noted }]` with the
  delayed ability "sacrifice / exile those cards" on the card's `granted` list (not in a
  chain that notes other things). New `Effect::SacrificeIt { what }` (CR 701.21a: only what
  its controller controls).
- **D-074 follow-up — copies of Auras (CR 303.4f–g):** a permanent entering as a copy of an
  Aura, or a token copy of one, is attached as it enters to what its controller chooses
  (`ChoiceKind::AuraHost`, not targeted; one choice is made without asking); with nothing
  it could enchant it doesn't enter (from the stack: to its owner's graveyard; a token isn't
  created). Before, it entered attached to nothing and died to the state check. The state
  check now reads a copy's "enchant" (`copy::text_def`). An Equipment copy enters
  unattached (tested). `Proposed::TokenEnters::attach_to` is new.
- **Newly compiled (31, all read against their Oracle text):** Kiki-Jiki, Splinter Twin, The
  Fire Crystal, Saheeli the Sun's Brilliance, Tempestra, Orthion, Reg Barclay, Stormsplitter,
  Kindred Charge, Cogwork Assembler; flicker until the next end step: Liberate, Ghost of
  Illyria, Guardian of Ghirapur, Skyskipper Duo, Blizzard Strix, Chief Kyle, Kykar, Skybind,
  Wiccan, Voyager Staff, Temporal Tribulation, Test of Priorities, Explosive Getaway,
  Mystifying Maze, Astral Slide, Astral Drift, Nahiri's Resolve; "return that card":
  The Scarab God, The Locust God, Liesa, Avacyn Angel of Horror. None lost; no existing
  text changed. The delayed text reads "Then at the beginning of the next end step, put
  those cards onto the battlefield" (the engine's wording for "return it").
- **Still blocked, by name:** a delayed ability about a target the effect didn't move
  (`DelayedTrigger:remembers Targeted`, 17; `ParentTarget`, 7), about attackers or blockers
  (`…TriggeredAttackerLKICopy` 17, `…TriggeredBlockerLKICopy` 16), about remembered cards
  outside a chain that notes them (`…remembers Remembered` 15), imprinted cards; token
  copies "at end of combat" or "your next end step" (`CopyPermanent:AtEOT ExileCombat` …)
  and `AtEOT$` in a chain that already notes (Jaxis).
- Golden: see the commit message.

## D-082 The most played gaps of the EDHREC top 1,000: changing targets, mass moves from graveyards, "mana value X or less" (2026-11-12)
Rules text: docs/rules-notes/d082-top1000.md. Tests: `crates/harness/tests/forge_top1000.rs`.
- Text first (from the D-080 review): "X is this creature's power" for the source's power or
  toughness in "where X is …" (was "its power", which could read as the spell's); 31 cards'
  text, no meaning.
- **Changing targets** (CR 115.7, 115.3, 115.5): `TargetFilter::OnStack { abilities,
  single_target }` (a spell, or a spell or activated/triggered ability, on the stack;
  "with a single target" = exactly one target in all), `Effect::ChangeTargets { what,
  single }`, `ChoiceKind::NewTarget`, `GameAction::SetTargets`, `Event::TargetsChanged`
  (all appended). The changing spell's controller goes through the targets slot by slot:
  "you may choose new targets" (115.7d) offers the current target first (keep) and every
  legal one; "change the target of target spell with a single target" (115.7a) only other
  legal targets, and with none it stays. Legal is as for the changed spell or ability and its
  controller (`targeting::candidates`, 115.5 included); a slot doesn't offer what another
  slot of the same "target" has (115.3); the new set is written at once (115.7e). Modes, X
  and everything else of the spell stay. Forge: `ChangeTargets` with `TargetType$ Spell` /
  `Spell,Activated,Triggered` + `Optional$` (choose new), `Spell.singleTarget` /
  `SpellAbility.singleTarget` without it (change the target); `UnlessCost$` works (Divert).
  **When "becomes the target" triggers are built, they must fire here for the new targets**
  (Forge's ChangeTargetsEffect runs BecomesTarget for each changed target). Blocked:
  115.7b/c, a fixed new target (Spellskite, `DefinedMagnet$`), random targets, a restriction
  on the new target (Meddle), another chooser, a target type of only activated abilities.
  Deflecting Swat, Redirect, Misdirection, Divert, Deflection, Shunt, Swerve, Imp's Mischief.
- **Mass moves from graveyards and hands** (CR 400.3, 110.2a, 701.24a): Forge's
  `ChangeZoneAll` from graveyards or hands is `Effect::MoveAll` (new field `controller`, "under
  your control", serialized only when set): every player's zone unless `Defined$` or a target
  player says whose; a filter of "your" cards is your zone only — and then only you shuffle
  (Forge would shuffle every library, which the cards don't say). Onto the battlefield a card
  enters under its owner's control (the engine's default) or yours with `GainControl$`;
  other players' cards are compiled only where the card says whose control ("under their
  owners' control", "each player returns …"), since by 110.2a "put onto the battlefield" is
  the instructing player's (`ChangeZoneAll:onto the battlefield under whose control`). Into a
  library only when shuffled (`ChangeZoneAll:Graveyard->Library`). Manabond is in
  `SCRIPT_DIFFERS` (its script doesn't reveal the hand, and discards only if a land moved).
  40 cards: Farewell, Scavenger Grounds, Splendid Reclamation, Rise of the Dark Realms,
  Replenish, Aftermath Analyst, Morningtide, Mnemonic Nexus, Decree of Annihilation.
  `ChangeZone:no-target` is now named by its shape; in the top 1,000 the shapes are triggers
  that return "this card" from a graveyard (they need triggers that work in a graveyard,
  `trig:zone Graveyard`), "return it … as an enchantment" (`Defined
  TriggeredNewCardLKICopy` with `StaticEffect$`), the commander from the command zone, and
  cards remembered across a cost.
- **"With mana value X or less"** (CR 107.3a, 608.2h): `Selector::mana_value_at_most`
  (serialized only when set; Forge's `cmcLEX` with X a script variable) — only in the cards a
  search or a look at the top cards chooses among, worked out as that instruction is reached
  (`ResolveFrame::fix_mana_values`); card-db refuses it anywhere else. `AIXMax$` is a Forge AI
  hint. Green Sun's Zenith, Wargate, Genesis Wave, Mind into Matter, Grim Servant, Loot,
  Mausoleum Secrets. Still blocked: Finale of Devastation (`PumpAll` with a condition), Eldritch
  Evolution (`Sacrificed$CardManaCost`), Chord of Calling (convoke).
- Golden: see the commit.

## D-083 Delayed abilities about targets, attackers and blockers; "at end of combat"; their wording (2026-11-12)
Rules text: docs/rules-notes/603-delayed.md. Tests: `crates/harness/tests/forge_delayed.rs`.
- **Wording:** a delayed ability about one object says "it" (`Effect::DelayedTrigger::one`,
  set by the compiler where the object is one: a single target moved, one token made, the
  card a creature became, the event's creature, the chain's one target); about several,
  "those cards". A card put onto the battlefield with no controller named is "return it to
  the battlefield under its owner's control" (CR 110.2a: that is what happens); "you
  sacrifice those cards" is "sacrifice it". Text only: 30 compiled texts changed, nothing
  else.
- **About the chain's one target** (`RememberObjects$ Targeted | ParentTarget` where no link
  moved it): "destroy it at the beginning of the next end step", "return it to its owner's
  hand at end of combat" — affected while it is still that object (CR 603.7c; tested: a
  target that left and came back is a new object and stays). Targets that may be players
  or spells are named (`…remembers a target that may be a player`, `…a spell`).
- **About the creature of the event** (`TriggeredAttackerLKICopy`, `TriggeredBlockerLKICopy`,
  `TriggeredTargetLKICopy` — the creature it blocked, that blocked it, that it damaged):
  Forge's LKI copy is the creature as it was when the trigger happened; the delayed ability
  is about that permanent and touches it only while it is still that permanent. "At end of
  combat" is the existing step trigger (CR 511.2); one created outside combat triggers at
  the next end of combat. `TriggeredCardLKICopy` (an object that has left, e.g. a
  sacrificed artifact copied later) stays blocked.
- **Token copies at other times:** Forge's `AtEOT$ Your…` (your next end step) and `…Combat`
  (the next end of combat) with `Sacrifice` / `Exile`; `Hand`, `Destroy`, `SacrificeCtrl`,
  `…Upkeep` are named (`CopyPermanent:AtEOT <value>`).
- **Named precisely now:** `DelayedTrigger:remembers Remembered (not noted by this chain)` /
  `(in an effect)`, `…remembers Targeted (not the chain's one target)` / `(a target the chain
  moved)`.
- Newly compiled (8, all read against their Oracle text): Scrappy Bruiser, Aether Membrane,
  Kaijin of the Vanishing Touch, Wall of Tears, Infernal Medusa, Lowland Basilisk, Ohran
  Viper, Sosuke, Son of Seshiro. Most other cards of these shapes wait on other blockers
  (`trig:Secondary-alone` for "blocks or becomes blocked by", Banding, Madness).

## D-084 Counters of any kind; Sagas (2026-11-12)
Rules text: docs/rules-notes/122-counters.md, docs/rules-notes/715-sagas.md (Sagas are CR
**715** in this edition, not 714). Tests: `crates/harness/tests/forge_counters.rs`,
`forge_sagas.rs`.
- **One representation:** `CounterKind::Named(CounterName)` next to `PlusOne`/`MinusOne` — a
  name (lower case, at most 24 bytes, so the kind stays `Copy` and a `BTreeMap` key). Every
  path that knew ±1 counters takes any kind: `Proposed::PutCounters`/`RemoveCounters`, the
  D-073 doublers, proliferate, "enters with N [kind] counters", Forge's
  `Count$CardCounters.<KIND>` (`Amount::CountersOnThis(kind)`, read from the source — or as
  it last existed: `Characteristics::counters` carries an object's counters, so last known
  information has them, CR 122.2), and costs `SubCounter<n/KIND>` / `AddCounter<n/KIND>`
  on the object itself (`Cost::remove_counters` / `add_counters`; the cost's counters are
  put through the replacement pipeline like any others). Only +1/+1 and -1/-1 do anything
  by themselves; only they annihilate (704.5q).
- **Blocked by name:** kinds with rules of their own (`counter:<KIND> (rules of its own)`:
  shield, stun, loyalty, defense, finality, hone), keyword counters (CR 122.1b,
  `(keyword counter)`), +X/+Y other than ±1 (`(+X/+Y)`), counters on players (`(on
  players)`: poison, energy, experience, rad, ticket), Forge's words for several kinds
  (`(not one kind)`: ALL, Any, EachFromSource, ExistingCounter); filters "with a [kind]
  counter on it" (`filter:.counters_…`) stay blocked (a filter's `Facts` have no counters).
  Counter costs on mana abilities and as additional costs of spells are not built (card-db
  rejects them).
- **Sagas (CR 715):** a chapter symbol is `Trigger::Chapter(n)` on a triggered ability
  (`K:Chapter:N:<SVar>,…`; "I, II — X" is two abilities, 715.2c). It triggers from a batch
  in which lore counters were put on the permanent — by an effect, a cost, the turn-based
  action or as it entered (122.6) — when the count went from below n to at least n
  (715.2b): two counters at once trigger two chapters. The intrinsic "this Saga enters with
  a lore counter" (715.3a) is compiled onto each Saga as its `ThisEnters` replacement, so
  doublers modify it and copies have it (LIMIT: a permanent that becomes a Saga only by a
  type-changing effect lacks it; card-db rejects a chapter ability on anything but a Saga
  that has it). CR 505.4/715.3c: as the active player's precombat main phase begins, one
  batch puts a lore counter on each Saga they control with a chapter ability (no stack).
  CR 704.5s/715.4: a state-based action sacrifices a Saga whose lore count is at least its
  final chapter number (715.2d, from the abilities it has now) unless a chapter ability of
  it is waiting to be put on the stack or is on it. Read ahead (702.155) is blocked by name
  (`kw:Read ahead`); Sagas that transform (exile, return transformed) are blocked by
  `ChangeZone.Transformed`; creature Sagas (715.1a) need nothing more and compile.
- **App:** counters of named kinds are badges "lore x2" (`describe::counter`).
- **Script differs:** Origin of the Avengers (chapter II puts any Hero card; the card says a
  Hero creature card).
- **Coverage:** 11,535 → 11,632 scripts (+97 against main, every one read against its Oracle
  text: 63 with counters of named kinds — Walking Ballista, Devoted Druid, the Spikes,
  Triskelion, Karlov of the Ghost Council, … — and 34 Sagas, 7 of them creature Sagas).
  EDHREC top 1,000: 629 → 631; top 5,000: 1,863 → 1,878. Most other Sagas wait on other
  features (transforming, `Effect:Triggers`, read ahead, fight, goad, …).
- **Not built — what loyalty needs beyond a counter kind:** starting loyalty as the
  planeswalker enters (306.5b, an intrinsic "enters with" like 715.3a); loyalty abilities
  (606): a cost of putting or removing loyalty counters (the counter costs above, plus −X),
  sorcery timing and one loyalty ability per permanent per turn (606.3, a per-object
  record of this turn); damage dealt to a planeswalker removes that many loyalty counters
  (120.3c); attacking planeswalkers (506.3, 508.1b: the engine's creatures attack only
  players); the SBA at 0 loyalty (704.5i); "loyalty" as a number read by effects.

## D-085 Copies of spells and abilities; storm; "whenever you cast or copy" (2026-11-12)
Rules text: docs/rules-notes/707-spell-copies.md. Tests: `crates/harness/tests/forge_spell_copies.rs`.
- **A copy** (CR 707.10): `GameAction::CopyStackObject` puts a new stack object on the stack
  with everything decided for the original — modes, X, targets, the alternative cost, what was
  paid by sacrifice — controlled (and, for a spell, owned) by the player who made it. A copy of
  a spell is a `GameObject` with `spell_copy` (D-081's groundwork): the original's card and
  face, its copiable values plus the copy effect's exceptions ("except that the copy is red",
  "except it isn't legendary": `CopyExcept`, D-074), a new card instance (never a commander).
  A copy of an ability has the original's source (707.10b). `Event::CopiedOnStack`; no
  `SpellCast` — a copy isn't cast, so "whenever you cast" and `cast_this_turn` don't see it.
  It ceases to exist off the stack (704.5e, already on main); a copy of a permanent spell
  resolves into a token that wasn't "created" (111.13, 608.3f, already on main).
- **What is copied** is read as it is on the stack, or as it last was there (CR 608.2h): a
  spell countered in response to "copy that spell" or to its storm trigger is still copied.
  `GameState::left_stack` keeps the stack objects that left while the stack wasn't empty
  (serialized only when not empty); it is cleared when the stack empties.
- **New targets** (707.10c): "you may choose new targets for the copy" is D-082's
  `Work::ChangeTargets` (keep first, then every legal target), asked for each copy right after
  it is made and before the next is made — so no copy can target a later one, and none
  targets itself (115.5).
- `Effect::CopySpell { what, count, new_targets, except }` (appended): `what` a target on
  the stack, `ObjRef::Triggering` ("copy that spell" of a cast trigger, whose `subject` is
  now the spell), or `ObjRef::This` (storm). `TargetFilter::AbilityOnStack { activated,
  triggered, yours }` (appended): "target triggered ability you control"; `SpellMatching`
  now also asks "you control". Forge: `CopySpellAbility` with `ValidTgts$`/`TargetType$`
  (Spell; `Activated`/`Triggered`[`.YouCtrl`] with `ValidTgts$ Card,Emblem`), `Defined$
  TriggeredSpellAbility` in a `SpellCast` trigger, `Amount$`, `MayChooseTarget$`, the copy
  modifiers of `CopyPermanent` (`NonLegendary$`, `SetColor$`, `AddTypes$`).
- **Storm** (CR 702.40a): `K:Storm` is a triggered ability `Trigger::CastThis` (appended:
  "when you cast this spell", triggering from the spell on the stack at 601.2i) whose "that
  much" is the number of spells every player cast this turn before it. **"Copy it for each
  other instant and sorcery spell you've cast before it this turn"** (Thousand-Year Storm):
  a "whenever you cast" trigger's "that much" is now the number of spells of its kind the
  caster cast this turn before this one, fixed as it triggers; Forge's
  `TriggerObjectsCurrentCastSpells$Valid <the trigger's types>.YouCtrl/Minus.1` (read in
  MagicStack.add: the list includes this spell) compiles to it only when its types are the
  trigger's own.
- **"Whenever you cast or copy"** (magecraft, Forge's `SpellCastOrCopy`, run by
  MagicStack.add for a copy with the copy's controller as the activator): `Trigger::Cast`
  gets `copies` (serialized only when set); a copy of a spell triggers it for the player who
  made it. 32 scripts use it; 9 of them now compile.
- Text: "permanent card" / "permanent spell" (CR 110.4a–b) for "noninstant nonsorcery";
  71 cards' text, no meaning. New Oracle guard `oracle:says new targets`: a script that lets
  you choose new targets where the card doesn't (Reflections of Littjara) is not compiled;
  pinned in `guard_phrases_are_still_written`.
- **Newly compiled (57, all read against their Oracle text):** Reverberate, Twincast, Fork,
  Dualcaster Mage, Flare of Duplication, Force of Rowan, Naru Meha, Nivix Guildmage,
  Geistblast, Insidious Will, Strionic Resonator, Lithoform Engine, Mirrorpool, Drafna,
  Double Major, Swarm Intelligence, Double Vision, Thousand-Year Storm, Mirari, Lucky Clover,
  Chancellor of Tales, Gorion, Tawnos, Thurid, Virtue of Knowledge; storm: Grapeshot,
  Flusterstorm, Brain Freeze, Empty the Warrens, Chatterstorm, Tendrils of Agony, Dragonstorm,
  Stormscale Scion, Krosan Adaptation, Temporal Fissure, Hindering Touch and 12 more;
  magecraft: Archmage Emeritus, Storm-Kiln Artist, Witherbloom Apprentice, Clever Lumimancer,
  Eager First-Year, Karok Wrangler, Lorehold/Quandrix/Silverquill Apprentice. None lost but
  Reflections of Littjara (the guard above; it compiled nowhere before either).
- **Still blocked, by name:** `param:CopySpellAbility.<key>` for another player controlling
  the copies (`Controller$`: Chain of Vapor), one copy per legal target
  (`CopyForEachCanTarget$`: Radiate, Zada), `Optional$`, `SingleChoice$`, `DefinedTarget$`;
  `CopySpellAbility:Defined <name>` for a copy of anything but a target or the cast spell
  (`Parent`, `Self`, remembered); `CopySpellAbility:TargetType …` for kinds mixing spells and
  abilities (Return the Favor). Not built: replicate, casualty, conspire, cascade, "cast a
  copy" (707.12 — a cast, `api:Play`), copy replacement effects (Twinning Staff), "whenever
  you activate" triggers (Rings of Brighthearth, Illusionist's Bracers).
- Golden: see the commit.

## D-086 Planeswalkers: loyalty, loyalty abilities, attacking them (2026-11-12)
Rules text: docs/rules-notes/306-planeswalkers.md (306, 606, 115.4, 120.3c, 704.5i, 506.3–506.4,
508.1b, 509.1a, 510.1b, 702.19, 903.3). Tests: `crates/harness/tests/forge_planeswalkers.rs`,
`crates/app/tests/planeswalkers.rs`, `app::combat` unit tests.
- **Loyalty** is the `loyalty` counter kind (`CounterKind::LOYALTY`, no longer blocked). A
  planeswalker card has `CardDef::loyalty` (306.5a) and its intrinsic "enters with N loyalty
  counters" compiled onto it as the ordinary `ThisEnters` replacement (306.5b), so doublers
  and copies treat it like Sagas' lore counter; card-db requires it of every planeswalker.
  `Loyalty:X` is blocked (`line:Loyalty not a number`).
- **Loyalty abilities:** `Timing::Loyalty` (new, last) on an activated ability whose cost is
  exactly one loyalty symbol: `add_counters` (+N; "[0]" is zero counters, also written
  `SubCounter<0/…>`), `remove_counters` (−N) or the new `Cost::remove_x_counters` (−X: X
  announced, at most the counters it has, 606.6). Offered only as a sorcery and only if no
  loyalty ability of that permanent was activated this turn (606.3:
  `GameState::loyalty_activated`, cleared as each turn begins; recorded as the cost is
  paid). Putting the counters is a replaceable event (606.4: doublers make "+1" put two).
  Text: "[+1]: …", "[−2]: …", "[0]: …", "[−X]: …". `Ultimate$` is read and ignored (Forge:
  AI/achievement tag). A loyalty ability that adds mana is blocked (`loyalty:adds mana (not
  a mana ability, CR 605.1a)`).
- **Damage** to a planeswalker removes that many loyalty counters (120.3c); a creature
  planeswalker also has it marked (120.3e). **704.5i** is a state-based action. **"Any
  target"** includes planeswalkers (115.4).
- **Attacking planeswalkers:** the declare-attackers option list is, per opponent in seat
  order, that player (`OptionDesc::Attack`) then the planeswalkers they control in timestamp
  order (`OptionDesc::AttackPlaneswalker`, new, last). `AttackInfo::planeswalker` (new) is
  the planeswalker attacked; `defender` stays the defending player (508.5), so blocks
  (509.1a) are unchanged. `Event::AttacksPlaneswalker` (new, last) follows the attacker's
  `AttackerDeclared`. Unblocked, the attacker deals its damage to the planeswalker while it is
  still in combat (on the battlefield, a planeswalker, controlled by the defending player:
  506.4), else to nothing (506.4c, 510.1b); trample's excess goes to the planeswalker, never
  its controller (702.19b, 702.19f). Trample over planeswalkers isn't built.
- **Golden:** event logs unchanged (no planeswalker in the golden setups, so no option list
  changed); state hashes moved (the new fields), re-blessed.
- **Also fixed:** `PutCounter` of any kind (D-084 had compiled counters of any kind
  everywhere but the "put N [kind] counters on …" effect); an activated ability without a
  target that adds mana and isn't a loyalty ability is a mana ability (CR 605.1a), which
  card-db now rejects (it took Empowered Autogenerator, which had just compiled as a
  stack ability).
- **AI (heuristic):** attacks an opponent's planeswalker when its attacker can take much of
  its loyalty (above all kill it) and the player isn't the better target; uses a "+"/"0"
  loyalty ability each turn unless it only hurts, a "−" one when it kills or removes
  something and leaves the planeswalker alive (or is worth losing it); "−X" keeps one
  counter. Targets of spells and abilities are now judged by card type (a planeswalker is
  no "target creature"). PlayerView only, no budgets changed.
- **App:** loyalty is the named-counter badge ("loyalty x4"); loyalty abilities are offered
  by clicking the planeswalker, like other activated abilities ("Test Sage Walker — [+1]: …");
  in the attack plan an attackable planeswalker is outlined, a click on it aims new
  attackers at it (again, or a click on a player, aims them back), a creature can be
  dropped onto it; arrows and the "ATK <name>" badge show it. Attacks on players work as
  before. The glyph test now also covers the app's own string and character literals
  (the "ⓘ" rules marker the fonts can't draw is "(i)").
- **Not built:** emblems (CR 114: `emblem (CR 114)` blocks 99 scripts — 95 planeswalkers,
  21 of them by it alone), loyalty abilities that add mana (21 planeswalkers), "can be your
  commander" (903.3a), trample over planeswalkers, battles. Most of Forge's 354 planeswalker
  scripts still wait on other features (`api:Cleanup`, `line:Text`, "until your next turn"
  durations, gain control, …).
- **Coverage:** 11,689 → 11,803 scripts (+114 against main, every one read against its
  Oracle text: 25 planeswalkers, Oath of Gideon, two "remove X counters" costs (Cruel
  Sadist, The Astonishing Ant-Man), and 86 cards that put counters of named kinds on things
  — Thallids, Trigons, Quests and Expeditions, Smokestack, Black Market, Assemble the
  Legion …). EDHREC top 1,000: 638 → 640; top 5,000:
  1,899 → 1,915; top 10,000: 3,263 → 3,303.

## D-087 Tapping permanents as a cost; convoke and improvise; crew; gaining control (2026-11-12)
Rules text: docs/rules-notes/d087-tap-costs.md. Tests: `crates/harness/tests/forge_tap_costs.rs`.
- **Follow-ups first (D-085):** Thousand-Year Storm's count tested against an opponent's
  instant ("you've cast" in its text); a counted bonus reads "+1/+0 for each artifact you
  control"; CR 303.4f is one function, `replacement::aura_hosts`, called by `Work::Move`
  (D-050) and the replacement pipeline (D-081), same behaviour (golden unchanged).
- **"Tap N untapped [permanents] you control"** (Forge's `tapXType`): `Cost::tap_others`,
  chosen in `CastStage::ChooseTap` after the mana abilities (CR 601.2g–h; `ChoiceKind::
  TapForCost`); never the permanent `{T}` taps; not `{T}`, so CR 302.6 doesn't stop a
  creature that arrived this turn. The payment planner keeps that many candidates untapped
  (`PlanInput::tap`: a candidate used to make mana counts against them, the "limited"
  mechanism D-066 has for sacrifices), so an ability is offered only if a payment exists.
  Also in alternative costs (flashback "tap three untapped white creatures"), "unless"
  costs and "you may tap … If you do". Not yet in mana abilities' costs (`Mana:cost
  tapXType`: Springleaf Drum, Relic of Legends) nor with a chosen sacrifice in the same
  cost (card-db). Oracle guard `oracle:says other untapped` (Tidal Terror's script lets it
  tap itself).
- **Convoke and improvise** (CR 702.51, 702.126; printed keywords only): while the total
  cost is paid, `OptionDesc::TapToPay { source, mana }` taps an untapped creature for a mana
  of one of its colors or one generic mana (convoke), or an artifact for one generic mana
  (improvise); never `{C}`. The planner counts each as a unit of mana that pays only the
  spell (a `GENERIC` mask bit every generic unit accepts), next to the permanent's mana
  abilities (it is one or the other), so a spell is offered only if a payment exists and a
  `TapToPay` only if the rest stays payable. `CastFrame::convoked` reduces the requirement.
- **Crew** (CR 702.122a): `Cost::crew`, an activated ability whose effect is "is also an
  artifact creature until end of turn" (layer 4); text "Crew N". Paid one creature at a time
  (other untapped creatures; summoning sickness doesn't matter), each offered only if total
  power N can still be reached; once it is, `Decline` stops (or tap more). Vehicles keep
  their printed power and toughness (card-db allows it, CR 301.7a); `layers::chars` already
  drops it while they aren't creatures (CR 208.3), and a noncreature can't attack.
- **Gaining control** (Forge's `GainControl`): `Effect::Apply`/`ApplyAll` with
  `ContinuousEffect::GainControl` (layer 2, D-029/D-030), until end of turn
  (`LoseControl$ EOT`) or for good; `Untap$`; `AddKWs$` until end of turn. Text "gain
  control of …". Not built, by name: `GainControl:LoseControl <kind>` (for as long as you
  control this, end of combat, until it untaps …), `GainControl:NewController`,
  `param:GainControl.Choices`, `GainControl:AllValid with Untap`.
- Text: a delayed "at the beginning of the next combat this turn" no longer reads "each
  combat" (it triggers once, CR 603.7c).
- New variants appended: `Keyword::Convoke`, `Keyword::Improvise`, `OptionDesc::TapToPay`,
  `ChoiceKind::TapForCost`, `CastStage::ChooseTap`; new skipped fields `Cost::tap_others`,
  `Cost::crew`, `PayGoal::tap_cost`, `PayGoal::tap_pays`, `CastFrame::pay_tap`,
  `CastFrame::convoked`.
- Golden: see the commit.

## D-088 Emblems; badges off the card's name (2026-11-12)
Rules text: docs/rules-notes/114-emblems.md. Tests: `crates/harness/tests/forge_emblems.rs`
(and a view-level test in `forge_planeswalkers.rs`).
- **An emblem is an object** (CR 114): a `GameObject` in its owner's command zone with
  `emblem: Some(abilities)` — indices into the granted list of `card`, the card whose effect
  made it (`Effect::Emblem`, new last; `GameAction::CreateEmblem`; `Event::EmblemCreated`,
  new last, public). `copy::base` gives it no characteristics but those abilities (114.3);
  it is owned and controlled by the player who got it (114.2), no permanent (114.5: nothing
  targets or moves it), and leaves the game with its owner (800.4a: `leave_game`, which
  emits `ObjectLeftGame`).
- **One representation for its abilities:** they are read like any object's
  (`layers::abilities`, `AbilityId` with `granted`), and work from the command zone (114.4):
  static abilities take part in the layer walk (`Key::Emblem`, "you" its owner); triggered
  abilities are detected with the permanents' (`battlefield_sources` takes emblems too);
  replacement/prevention effects, rules about players and combat rules are gathered with
  the permanents'. Two emblems are two objects and both work (tested: +2/+2). "This emblem
  deals damage": the emblem is the source (colorless, no characteristics).
- **Compiler:** Forge's `DB$ Effect | Name$ Emblem — … | Duration$ Permanent` with
  `StaticAbilities$` / `Triggers$` / `ReplacementEffects$` (docs/forge/effects.md §10: a name
  starting with "Emblem" makes the effect object an emblem); `TriggerZones$ Command` is
  accepted there, and `AffectedZone$ Battlefield` on any continuous static (it is where a
  permanent is anyway — this alone compiled several cards). Text: `You get an emblem with
  "…"`; the Oracle guard "says emblem" is pinned in `guard_phrases_are_still_written`.
  Named: `emblem:activated abilities` (Forge `Abilities$`), `emblem:duration`; card-db
  rejects an emblem ability about one object or anything remembered.
- **`effects::Lasting` and emblems:** Lasting (D-065) stays what a resolving spell or
  ability leaves behind for a while on no object (611.2, 603.7: "creatures can't block this
  turn") — no object, a duration, its source the spell. An emblem is an object for the whole
  game. Forge writes both as `Effect`; the engine keeps them apart by the name.
  **Not built:** `Effect:Triggers` without an emblem — "until end of turn, whenever …",
  delayed triggered abilities that trigger every time during their duration (CR 603.7b);
  the blocker is now named `Effect:Triggers (delayed abilities for a while, CR 603.7b)`
  (139 scripts). It would need `effects::Delayed` to trigger repeatedly until its window ends
  and to be detected by every trigger kind (today: one trigger, at steps, attacks and
  blocks), with its source as it last existed (`source_lki`) — not an emblem object.
- **Clients (the engine is the product):** `PlayerView::players[p].emblems` lists each
  player's emblems with the card that made them and each ability's text (public to every
  seat); `EmblemCreated` says who got which, `ObjectLeftGame` when it leaves with its owner;
  a triggered ability of an emblem is on the stack with the emblem as `ability_of`, and its
  decisions are ordinary ones. Tested from another seat's view and the events. For
  planeswalkers (D-086) a view-level test reads loyalty, the loyalty abilities offered and
  what each attacker attacks.
- **App (reference client, minimal):** an emblem is drawn as itself (named after its card,
  its abilities as text; enlarged on hover), the narrator says "P0 gets an emblem (…) with
  "…"". Counter and combat badges now sit over the art below the title bar, and a badge is
  shortened to the card's width with an ellipsis ("ATK Test S…").
- **Coverage:** 12,129 → 12,149 scripts (+20 against main, every one read against its Oracle
  text): 6 cards with emblems (Ajani Resolute, Elspeth, Knight-Errant, Elspeth, Sun's
  Champion, Gideon, Ally of Zendikar, Huatli, Radiant Champion, Professor Dellian Fel) and 14
  through `AffectedZone$ Battlefield` (Falter, Seismic Stomp, Gruul Charm, Goblin
  Trashmaster, Leaf-Crowned Visionary, …). EDHREC top 1,000: 642 → 644; top 5,000: 1,960 →
  1,964. Of Forge's 104 scripts with emblems, 98 still wait on other features
  (`api:Cleanup`, `line:Text`, "until your next turn", mana loyalty abilities, …);
  `emblem:activated abilities` is in 3.

## D-089 Reveals the card makes; "becomes the target" and ward; revealing until (2026-11-12)
Rules text: docs/rules-notes/d089-ward-dig-until.md. Tests: `crates/harness/tests/forge_ward.rs`,
`forge_dig_until.rs`, narration.rs `no_reveal_the_card_doesnt_make`.
- **Reveals (CR 701.20):** Forge reveals what a search or a look finds when a quality is
  asked; the engine now reveals a card only on its way to a hidden zone (hand, library) and
  only where the card says "reveal". Onto the battlefield, into a graveyard or exile a card
  is simply moved (public by arriving). 19 cards changed text and behaviour (Collected
  Company, Buried Alive, Goblin Engineer, Genesis Ultimatum, Deploy the Gatewatch, Mana
  Severance, N'Yami-Class Mother Ship, Loot, …). New Oracle guard `oracle:reveals` (the
  compiled text reveals, the card — reminder text included — never says "reveal"), pinned;
  it blocks none of the real cards after the fix. Golden unchanged.
- **Text:** "you may have this Vehicle become an artifact creature in addition to its other
  types" (The Fantasticar; base verbs after "have"), "becomes an artifact creature in
  addition to its other types" for every type-adding effect of card types only, "fourth",
  "nonlegendary", "nonland permanent card"; 66 cards' text, no meaning.
- **"Becomes the target"** (`Trigger::BecomesTarget { by, only_spells }`, on the permanent
  itself): when a spell is cast or an activated or triggered ability is put on the stack with
  it as a target, when a copy is put on the stack targeting it (707.10c), and when it becomes
  a new target of a changed spell or ability (115.7; a kept target doesn't); once per spell
  or ability and permanent (603.2c). "It" is the spell or ability, "that player" its
  controller (Forge's MagicStack.add and ChangeTargetsEffect run `BecomesTarget` the same
  way). Forge's `Mode$ BecomesTarget | ValidTarget$ Card.Self` compiles; other targets
  (`trig:BecomesTarget target …`), `FirstTime$`, `Valiant$` are named.
- **Ward** (CR 702.21a): `K:Ward:<cost>` with any cost `Cost` expresses (mana, life,
  discard, sacrifice) is that trigger with "counter that spell or ability unless that player
  pays [cost]" (`Effect::CounterIt`: a spell to its owner's graveyard, an ability removed;
  "can't be countered" respected). Text "Ward {2}", "Ward—Pay 3 life". Several costs to
  choose from (`kw:Ward several costs`) aren't built.
- **What a client sees** (the engine is the product): `StackView::about` names what a
  triggered ability is about when it is public (the spell or ability a ward trigger would
  counter); the payment is `ChoiceKind::PayToSave { at_stake }` with `OptionDesc::PayCost
  { cost }` stating the cost (other "unless" payments keep `PayUnlessCost`);
  `Event::PaidToSave { player, at_stake, paid }` and `Event::Countered` say what happened.
- **Revealing until** (Forge's `DigUntil`, CR 701.20): `Effect::DigUntil`: each player
  reveals from the top until N matching cards (all revealed to every player, `Event::
  Revealed`), the found cards go first, then the rest (a random order, or ordered by the
  owner), then an optional shuffle. Not built, by name: `MinTotalCMC$` (Tasha's Hideous
  Laughter), `OptionalFoundMove$`, `NoMoveRevealed$`, `MaxRevealed$`, `Remember…$`,
  `GainControl$` of another player's cards, a targeted player.
- Not built: heroic ("whenever you cast a spell that targets this") — a cast trigger with a
  condition on the targets, not this event.
- **With D-090's client view:** `PaidToSave` is public (`client::map_ids`/`object_ids`
  know it). CR 701.20d ("shuffled or otherwise reordered … stop being revealed"): a
  library put in order on its top or bottom (`Event::LibraryArranged`: the rest of a
  reveal-until in a random order, a scry, a look) now ends what was revealed of that
  library, in `EventView` and in client_view's knowledge model alike — both had kept the
  revealed cards known, so a seat could follow one later drawn from the bottom (a leak on
  both sides, which the property test couldn't see; tested in `forge_dig_until.rs`). And
  `EventView` names revealed and looked-at cards (`Identified`) before the `Revealed`
  /`LookedAt` that shows them, so a narrator can say what was revealed.
- A "becomes the target" trigger that only spells can make reads "counter that spell" (ward
  itself keeps CR 702.21a's "that spell or ability").
- New variants appended: `Trigger::BecomesTarget`, `Effect::CounterIt`, `Effect::DigUntil`,
  `ChoiceKind::PayToSave`, `OptionDesc::PayCost`, `Event::PaidToSave`; `StackView::about`,
  `Work::ChangeTargets::fresh` (skipped when unset).
- Golden: see the commit.

## D-090 What a client sees: one seat's event stream, and who the game waits for (2026-11-12)
Rules text: docs/rules-notes/400-hidden-information.md (CR 400.2, 400.7, 401, 402, 406.3,
701.20, 708). Tests: `crates/harness/tests/client_view.rs`.
- **Shape:** `engine_core::client::EventView` — one seat's view of the stream, built from the
  game as it is (`EventView::new(&st, seat)`, seat = `Seat::Player(p)`, `Spectator` or
  `Everyone` for a referee/replay) and fed every batch in order (`redact(&truth)`). It is
  stateful because whether an id may be shown depends on where the object was when the event
  happened, which only the stream itself says. Plain data (serialisable), pure, no_std.
  `StepOutput::events` is now `StepOutput::truth`, documented as engine truth with hidden
  information, for replays and hashes: a client that reaches for "the events" meets the word
  and the pointer to `EventView`. (Not done: making the raw stream private — the harness
  records and hashes it everywhere.)
- **What a seat gets:** everything about public zones; its own hand; what is revealed to
  all or looked at by the seat (until it is shuffled away, 701.20d). Every other object of a
  hidden zone is `client::HIDDEN`. A move the seat can't follow is `Event::MovedUnseen
  { owner, old, new, from, to }` (whose, from where to where, the unseen side's id hidden, no
  card instance — which would link the two objects across a hidden zone, CR 400.7); each
  object the seat newly gets to see is named once by `Event::Identified { object, card, face,
  owner }`. Both variants are added last and never emitted by the engine (event logs and
  hashes don't move). Identities that become public later don't leak earlier ones: the
  hidden ids were never handed out. `client::object_ids(&event)` lists the ids an event
  names.
- **Decisions:** `PlayerView::decision` (and `revealed`) were already the viewer's only;
  `PlayerView::waiting` now says to every seat who the game waits for and the kind of
  decision (`ChoiceKind`), never the options.
- **Consumers:** the narrator (`harness::text::Narrator::new(&st, seat)`, `narrate(start,
  truth, seat)`) is fed the seat's redacted stream and names only what it is told; its own
  hiding (viewer checks on draws, looks, hidden moves) is gone. `watch` narrates as
  `Everyone`, `play` and the app's log as the human's seat (the app spectating: seat 0, the
  seat whose view it shows). One narration test changed meaning: a landcycling search
  *reveals* the land, so the opponent's log now names it (CR 701.20a).
- **Rustdoc** on `start`, `step`, `player_view` and `client` says what an embedder may rely
  on: a pure, deterministic state machine that stops at every decision; illegal responses
  rejected before any mutation; `truth` vs a seat's stream.
- **Tested:** a draw, a non-revealing vs a revealing tutor, looking at the top three, mill,
  a mulligan, determinism and serialisation of the stream, a spectator; and a property test
  over random forge-4p and m2-4p games checking every seat's (and a spectator's) stream event
  by event against an independent knowledge model (public zones, own hand, revealed, looked
  at), and every seat's `PlayerView` at every decision. It found no leak in the
  implementation; with the redaction weakened (library cards visible) it fails at once.
- **Not built here (part 2, below):** the events clients were missing.

### D-090 part 2: the events say what a client had to guess (2026-11-12)
Tests: `crates/harness/tests/events.rs`, two direct per-seat tests in `client_view.rs`.
Events only report: every golden game makes the same decisions and reaches the same state at
each of them (checked with `log_hash`/`event_count` zeroed, 48,432 decisions); event logs and
hashes moved once (re-bless commit). New variants are last (after `PaidToSave`); new fields
are `#[serde(default)]`.
- `SpellCast` + `face` (which face was cast, CR 601.3e), `alt: Option<AbilityId>` (the
  alternative cost it was cast for — flashback, evoke, "without paying its mana cost" …:
  read the `AlternativeCost` it names), `tax` (commander tax paid, CR 903.8). Tax is on the
  cast, not on `ManaPaid`, because it is part of the total cost, not a payment of its own.
- `AbilityActivated` + `text: AbilityId` (which ability: where its text is written) and
  `triggered` (a triggered ability put on the stack). That is the trigger's link to its stack
  object: `Triggered { source }` then `AbilityActivated { ability, source, text, triggered:
  true }`; no extra id was needed.
- `AbilityRemoved` + `reason: StackExit` (`Resolved`, `NoLegalTargets`, `Countered`,
  `ControllerLeft` (800.4a), `Abandoned` (a trigger removed while being put on the stack),
  `ConditionFalse` (an intervening "if", 603.4)).
- `ResolutionBegan { object }` as a spell or ability starts to resolve (608.2): a client no
  longer counts passes. `Resolved`/`Fizzled`/`AbilityRemoved` still end it.
- `ContinuousEffectBegan` + `card`, `effects: Vec<ContinuousEffect>`, `affected` (the
  objects, when locked in, 611.2c), `abilities` (abilities granted, as `AbilityId`s),
  `about` (the object "that permanent" refers to) and `until_end_of_turn`: enough to say
  what it does, to what, until when (`card_model::describe::continuous_text`).
- `ManaPaid` + `mana: Vec<ManaType>` (what was spent, by diffing the pool); `ManaAdded` +
  `source: Option<ObjectId>` (the permanent or spell that made it) and `restricted`.
- `MayChosen { player, object, yes }` ("you may", 608.2d); `UnlessPaid { player, object,
  paid }` (every "pay or not" a resolution asks, 118.12 — ward keeps `PaidToSave`);
  `SearchFound { player, cards }` after a library search (701.23a), possibly empty. Redacted:
  the searcher (and `Everyone`) gets the ids and `Identified`; other seats get `HIDDEN` for
  each card — a count, which the rules let them know; a reveal that follows names them.
- `ZoneChange` + `creature` (it was a creature as it last existed: "dies", 700.4), `face`
  (the face it showed), `position: Option<LibraryPosition>` (top/bottom/Nth, public — also
  on `MovedUnseen`).
- `Tapped`/`Untapped` + `cause: TapCause` (`Cost`, `Mana`, `Attack`, `Effect`,
  `UntapStep`).
- `TokensCapped { controller, token, wanted, made }` when `MAX_TOKENS_AT_ONCE` cuts a count:
  the user-approved deviation is no longer silent.
- Not built: a face-down flag on exile moves (face-down objects aren't built yet; the field
  belongs with them).
- Found on the way: `perform_move` reads the object's characteristics before the move (for
  `creature`), which cached a pre-move layer view at an unchanged generation; the layer cache
  is now invalidated after each move (derived data only).
- Consumers: the narrator uses `ResolutionBegan` (no pass counting), names alternative costs,
  the mana spent, what a continuous effect does, declined "may"s, unless payments, search
  results and the token cap, and tap causes instead of guessing from the step.

## D-092 The client boundary: what a client gets, enforced and proved by wire (2026-11-12)
Tests: `crates/harness/tests/client_boundary.rs`, `client_view.rs` (property test and
`each_seat_sees_its_own_view_of_a_search`), `tests/knowledge/mod.rs` (the shared knowledge
model); `cargo xtask check-deps` (the source check, with its own unit test).
- **A client's whole input**, stated once in engine-core's crate docs ("Host and client"): the
  card catalog (shared, read-only), its view (`seat_view`), the `PendingDecision` when it
  decides, and its `EventView` of each step's events. It answers with a `Response`. The host
  owns the `GameState` and makes these.
- **One view for every seat:** `view::seat_view(st, Seat)`; `player_view(st, p)` is
  `seat_view(st, Seat::Player(p))`, so the three can't drift apart. A spectator gets public
  information only: no hand, no decision, no `revealed`, a library's top card only when it is
  revealed to everyone (CR 401.5, `players::library_top_seen_by(st, None, owner)`), and — like
  every seat — `waiting`. `Seat::Everyone` (replays, a referee) gets every hand and library
  (`PlayerPublic::hand` / `library`, `None` for every other seat) and the pending decision
  whoever's it is. `PlayerView::seat` says whose view it is; `viewer` stays (player 0 for the
  two seats that are no player) so the AI and the app keep reading it (superseded by D-093:
  `viewer()` is derived from the seat, `None` for those two).
- **Moved into the view** (what the reference clients read from the state): the decision
  counter (`PlayerView::decision_count`: answers so far — public, since `waiting` tells every
  seat who decides each time), the result (`PlayerView::result`), whether an object is a token
  (`ObjectView::token`, CR 111.1). Turn, step and active player were already there. No state
  field was added: golden state hashes don't move.
- **Clients rewired:** the app's UI reads `session.view()`, `session.catalog()` and the
  human's decision — no `session.st`. The narrator is built from what a client has
  (`Narrator::new(catalog, &PlayerView)`); `harness::narrate(start, truth, seat)` is the
  host-side shortcut that makes the seat's view and redacted events from a state. The
  session's automatic answers (stops, auto-pay; D-054, D-066) are worked out from the
  player's view, as a client would.
- **Enforced:** `cargo xtask check-deps` fails if client code — the app's `ui.rs`,
  `cardview.rs`, `combat.rs`, `art.rs`, `harness/src/text.rs`, the `ai` crate — names
  `GameState`, a session's `.st`, or what only the host makes from a state (`player_view`,
  `seat_view`, `EventView::new`), outside the file's `#[cfg(test)]` module. Host side: the
  app's `session.rs`, the harness driver, tests; later the search AI's Determinizer (spec, M5:
  a host-made sample consistent with the agent's view).
- **Proved by wire:** `client_boundary.rs` plays whole m2-4p (random), cmd-4p (heuristic vs
  random) and forge-4p (random) games where every seat's agent and narrator get only JSON text
  (its view, its decision, its redacted events) and answer with JSON; each game ends in the
  same state, after the same number of decisions, as the same seed played directly. What
  crosses the wire to a seat is checked against the knowledge model: the ids of its view and
  events, and no JSON string that is the name only a card it may not know has (the
  decision's own cards — a scry, a search — and a rule-revealed top card excepted, the latter
  checked by the model). Both checks were seen to fail with a library's top card leaked.
- **Property test:** the knowledge model now checks every seat's view at every decision
  (a spectator's and everyone's too) against what its stream made known, and plays 24
  forge-4p and 6 m2-4p whole games on four threads (about 15 s); an `#[ignore]`d long
  variant (`APNAP_LEAK_GAMES`, default 200) is for the soak.
- **Left on the host side:** `Session::declaration` — the whole attack or block, found by
  answering "no" for each creature on a copy of the game, because the engine asks one
  creature at a time; a remote client can't do that and would want the candidates and their
  options in the decision. The app's spectating still shows seat 0's player view (an app
  choice; `Seat::Spectator` is there for it).

## D-093 Boundary leftovers: a preview of the whole declaration, a viewer only for players, no invented event fields (2026-11-12)
Rules text: docs/rules-notes/508.1-509.1-declarations.md (CR 508.1–508.1d, 509.1–509.1c,
702.111b, 734.1). Tests: `client_boundary.rs` (`whole_declarations_planned_from_the_first_question`),
`client_view.rs` (`only_a_players_view_has_a_viewer`, `an_agent_handed_a_spectators_view_fails_loudly`,
and the knowledge model's view checks).
- **The declaration preview.** Declarations stay one decision per creature (spec §8.5, pitfall
  5). `PlayerView::declaration: Vec<DeclarationAsk { creature, options }>` lists, while the
  seat's decision is a declaration of attackers or blockers, every creature still to be asked —
  the one asked now first — in the order they will be asked, each with the options it will be
  offered (built by the same `attack_options`/`block_options` the frames ask with). Empty, and
  not serialised, for every other decision and every other seat. It is on the view, not on
  `PendingDecision`: views aren't state, so **golden state hashes, decisions and events did not
  move at all** (no re-bless); the golden test passes unchanged.
- **Can a later creature's options depend on an earlier answer?** In the rules, yes: restrictions
  and requirements are checked on the declaration as a whole (508.1c–d, 509.1b–c; "can't attack
  alone", menace). In this engine, no: each creature is offered what is legal for it alone on the
  game as the declaration began, and nothing changes the game between the questions (answers go
  into the frame only). The one whole-declaration check built is menace (702.111b); when it
  fails the declaration is undone (509.1, 734.1), `Event::BlockDeclarationIllegal { player }` is
  emitted (it already existed), and that player is asked again from their first creature with a
  new preview — that is how a client tells a restart from a new declaration. Not built:
  "can't attack/block alone", "attacks/blocks if able", attack/block costs. When one is, the
  preview's promise ("these are the questions that follow; the whole is checked at the end and
  an illegal one is asked again") must still hold or the field's docs change.
- **Consumers:** `Session::declaration` reads the preview instead of answering "no" on a clone
  of the game. The heuristic AI keeps its own attack and block planning (it doesn't need the
  preview); h-vs-r on cmd-4p, 200 games, the same seeds before and after: seat 0 (heuristic)
  won 195 of 200 both times, identical games (avg 56.5 turns, 2,436 decisions).
- **`viewer` is derived from the seat.** The field is gone; `PlayerView::viewer()` is
  `Some(p)` for `Seat::Player(p)` and `None` for a spectator or everyone. The heuristic agent
  panics if handed a view that is no player's (`"the heuristic agent was handed a Spectator
  view"`); the app's table rotation draws a non-player view from player 0's seat.
- **No invented event fields.** The `#[serde(default)]`s D-090 part 2 put on event fields —
  including `no_ability()` (ability 0 of card 0) and `no_card()`, and the `Default`s of
  `TapCause` (`Effect`) and `StackExit` (`Resolved`) — are gone: every field is required (an
  `Option` still reads a missing field as `None`). Nothing depended on them: replays store
  responses, not events; no event log is checked in; the state stores no events.
- The app watching with no human seat still shows seat 0's player view (unchanged, out of
  scope); `Seat::Spectator` exists for it.

## D-095 A portable copy: one folder to zip and send (2026-11-12)
The user asked for something to hand to another person. (D-094 is reserved for attack and
block requirements, briefed but not started.)
- **Why a directly run binary found no cards:** nothing is embedded in the programs; they
  read Forge's scripts, Scryfall's Oracle file and the printed data from the cache
  directory, which depends on the environment (`$XDG_CACHE_HOME`/`$HOME`). The app itself
  downloads only printed data and images; the scripts and the Oracle file come from
  `harness fetch-forge` / `fetch-bulk`. Run from another home directory (outside the dev
  container), the cache is empty and stays without them.
- **Portable folder:** `card_db::scryfall::cache_dir` is now `$APNAP_CACHE_DIR`, else an
  `apnap-data` directory next to the executable if one exists (`PORTABLE_DIR`), else the
  platform cache. Saved games go there too (`ui::saves_dir`). Without that directory
  nothing changes.
- **`cargo xtask bundle [--out DIR] [--target TRIPLE] [--with-images]`** (default
  `dist/apnap`, git-ignored): builds the app and the harness, copies them as `apnap` and
  `apnap-harness`, fills `apnap-data` from this machine's cache through
  `harness bundle-data` (cards and token scripts with Forge's LICENSE, the Oracle file,
  the precons' printed data; not Forge's `.git`), adds LICENSE, a README and
  `apnap-source.tar.gz` (`git archive HEAD`: the bundle hands on GPL binaries and scripts,
  so the source goes with them), then proves the folder stands alone: the bundled harness
  plays a precon game with an empty user cache. It replaces only a folder it made.
- **Images** are not bundled unless `--with-images`: they are Wizards of the Coast's art,
  under no licence we can pass on; the app downloads what it shows into `apnap-data`.
  Invariant 13 is about the repository and still holds (`/dist/` is ignored).
- **Not a single executable:** embedding 35,000 scripts and the Oracle file would tie every
  build to a filled cache; a folder needs no build change. Not done: a Windows or macOS
  bundle (the command takes `--target`, the toolchains aren't installed here), a build
  against an older glibc (a bundle made on a new Linux may not start on an older one).
- Tests: `scryfall::tests::the_cache_is_the_portable_folder_when_there_is_one`,
  `a_portable_folder_is_a_directory_next_to_the_executable`, `harness::bundle::tests`,
  `xtask::bundle::tests`.

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
7. **CR 800.4j** (found in M1): if a player leaves during their own turn, the turn continues
   without an active player, and priority that would go to them goes to the next player.
   Spec §8.7 doesn't mention it (D-018). Fixed in the spec 2026-11-08.
8. **M2 (CR 613/614–616/603 verified, 2026-11-08):** the spec omits 616.1b–d (more
   must-choose-first tiers) and 616.1g (contained events); 613.3/613.8a(c) (CDAs first;
   dependency only between CDA/CDA or non-CDA/non-CDA); **613.6** (an effect keeps applying to
   the same objects in later layers even if its source ability is removed meanwhile — this
   qualifies the spec's MTG-Ichor invariant 2 / CLAUDE.md invariant 4: existence is checked
   when an effect would *begin* to apply); 613.8a(b) (text changes count as dependency);
   613.7a/c timestamps. Details in docs/rules-notes/613-layers.md and 614-616-*.md.
