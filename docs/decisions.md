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
