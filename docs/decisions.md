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
