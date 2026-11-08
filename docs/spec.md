# Commander Rules Engine + Desktop App: Handoff Spec

Build it as a Rust workspace with a pure, deterministic engine crate. Every mutation passes through one replacement-aware action chokepoint, and every player choice goes through one narrow `Decision` interface shared by the UI and the AI. Card behavior lives in RON data files with a small set of named Rust escape hatches. Ship an egui desktop shell first. Most of this design has already been proven by open-source Rust MTG engines in 2026. The sources disagree on fine details, so the job is mostly to combine the right parts of them and to sequence the hard rules work correctly: replacement effects before triggers, and layers before almost everything else.

## TL;DR

- **Architecture:** a `GameState` that is `Clone + Serialize`, one `step(state, Response) -> (events, Option<PendingDecision>)` function, a central object store with every zone move routed through a single `move_object()`, and a four-primitive decision interface. The AI and the egui UI both implement that interface. MTG-Ichor (Rust, Commander-targeted) and phase-rs (Rust, pure reducers, Tauri) already validate this shape.\[1\]\[2\]
- **Hardest rules, in build order:** (1) priority/stack with APNAP, (2) the replacement/prevention pipeline (CR 614–616), (3) triggers reading *post-replacement* events with last-known information, (4) the layer system (CR 613) with timestamps and dependencies, (5) Commander specifics (command zone, tax, 21 combat damage, players leaving). The milestones gate each one behind golden replay tests.
- **AI:** ship a heuristic AI first that is Commander-aware (commander-damage lethality, threat scores per opponent, never attacking "allies" by accident). That covers the documented Forge failure modes. Then add determinized/ISMCTS search over cloned states with a fixed iteration budget, never a wall-clock budget, so games stay reproducible. Cowling, Ward & Powley (IEEE TCIAIG 2012) found that at a 10,000-simulation budget "around 40 determinizations, each with 250 simulations, provides good play," and binary-decomposed card-play decisions were the strongest MTG search setup in their study.

---

## 1. Goals and Non-Goals

### Goals
1. Play 4-player free-for-all Commander: 1 human vs 3 CPU opponents, on Linux (Arch) desktop.
2. A rules engine that is a **pure deterministic state machine**: actions in, new state plus events out, seeded RNG, serializable state, bit-for-bit replays.
3. **Cloneable state** cheap enough for AI search (thousands of clones per decision).
4. **Data-driven cards**: most cards are RON data composed from a shared vocabulary of costs, effects, targets, conditions, triggers, statics and replacements.
5. Card data (Oracle text, types, costs, color identity) imported from Scryfall bulk data. Behavior is scripted separately and keyed by Oracle ID.
6. Milestone 1: **four fixed Commander precon decks fully playable end to end**, with every card in that pool verified, plus a heuristic AI.
7. Later: search AI (determinized MCTS, then ISMCTS).

### Non-Goals (for v1)
- Networked multiplayer, deckbuilding UI, draft or other formats.
- Full card-pool coverage. The engine must **refuse to load unverified cards** rather than approximate them. kiln-mtg makes this explicit ("the engine never pretends a card works"), and it is the right policy.\[3\]
- Mobile or web builds. Keep the engine `no I/O` so WASM stays possible later, but don't target it now.
- Distributing any Wizards assets in the repo (see §12).

---

## 2. Prior Art and What to Take From It

### Forge (Java)
- **Script format.** One text file per card with `Name:`, `ManaCost:`, `Types:`, `PT:`, `K:` (keywords), `A:` (spell/activated abilities, `SP$`/`AB$`), `T:` (triggers, `T:Mode$ ChangesZone | Destination$ Battlefield | ValidCard$ Card.Self | Execute$ TrigDraw`), `S:` (statics, `S:Mode$ Continuous | Affected$ ... | AddKeyword$ ...`) and `R:` (replacements, `R:Event$ Moved | ... | ReplaceWith$ ...`).\[4\]\[5\]\[6\]\[7\]\[8\]\[9\] `SVar:` lines hold named sub-abilities (`DB$` "drawback" chains linked by `SubAbility$`) and computed values (`SVar:X:Count$...`).\[7\]\[10\] Forge's founder, writing as MTGRares in the "Card Scripting" post on the "Computer Programming and Magic: The Gathering" blog (Jan 2, 2012), estimated "Probably 95% of all cards in Forge are scripted."
- **Lesson: the vocabulary model works, the stringly-typed encoding doesn't.** Forge scripts reference engine internals by string keys, and bugs show up as silent mismatches. For example, a 2026 PR found `DefinedPlayer$ ReplacedPlayer` in a `Moved` replacement resolved to nothing because `ReplaceMoved.setReplacingObjects` never sets a Player.\[11\] Another found that a `StaticEffect$` was left behind when a battlefield move was replaced rather than prevented, affecting "around 55 scripts."\[12\] **Our version: the same compositional vocabulary, but as typed Rust enums deserialized from RON**, so a misspelled parameter fails at load time, not at game time. Use Forge scripts only as a reference when hand-authoring RON; importing them inherits Forge's semantics and string-keyed bugs.
- **AI weaknesses in multiplayer Commander (documented in issues):**
  - It accepts lethal commander damage when its life would stay above 0 (#12269).\[13\] A related report says AIs "let my Commander through very often, getting killed by Commander Damage" and ignore damage redirection and protection-from-creatures on players (#9960).\[14\]
  - It targets allies' permanents with removal in team games (#11701).\[15\]
  - The attacker search can hang at 100% CPU with 16 creatures under Aurification in a 4-player game (#7625), a combinatorial blow-up in attack declaration.\[16\]
  - The Flat Monte Carlo mode could spend "close to a minute" on one decision because nested simulations ran unbounded inside a play (PR #12527).\[17\]
  - A third-party harness found full-simulation games don't replay from a seed because Forge's per-decision AI timeout is wall-clock based (`AI_TIMEOUT` 5 s). The AI silently passes when it expires, "so a busy … night plays differently from a quiet one."\[18\]
  - **Takeaways:** model commander damage in every evaluation, bound combinatorial searches by construction, and budget search by iterations rather than time.

### XMage (Java)
- **Architecture:** Maven multi-module client/server, with a framework module (abilities, effects, filters, targets) separate from the card-sets module. Each card is a Java class (`CardImpl` subclass) that composes reusable `Ability`/`Effect` objects, with bespoke `OneShotEffect` subclasses when needed. Player types (human, AI) and game modes (duel through Commander free-for-all) plug in.\[19\]
- **Testing:** a strong unit-test DSL, `addCard(Zone.LIBRARY, playerA, "Lightning Bolt", 20)`, then scripted actions and assertions.\[20\] **Copy this test style** (see §10).
- **AI:** the default "Mad" AI is `ComputerPlayer7`, a simulation/min-max design. A maintainer called it "pretty convoluted and horribly documented,"\[21\] and a forum developer found a memory leak in its simulated-node graph.\[22\] A 2026 integration describes Mad as an **omniscient** opponent whose simulations "include the pilot's real hand and exact library order," with a 12-second budget on a shared thread pool, which makes it nondeterministic and hardware-dependent.\[23\] **Lesson: our search AI must determinize hidden information honestly, and must be budgeted by iterations.**

### Rust and modern engines (2026)
| Project | Approach | Lesson for us |
|---|---|---|
| **MTG-Ichor** | From-scratch Rust, "correctness-first," targets 4-player Commander and parallel AI self-play; has a CLAUDE.md\[2\] | Central object store; single `move_object()` chokepoint; every mutation is a `GameAction` *proposal* run through the CR 616.1 pipeline; `DecisionProvider` trait with **four primitives** (`pick_n`, `pick_number`, `allocate`, `choose_ordering`) plus a `ChoiceKind` enum; built replacement effects **before** triggers "on purpose" because "triggers fire on events that *did* happen"\[2\] |
| **phase-rs** | Rust engine → native + WASM, Tauri desktop, React UI\[1\] | Pure reducers `apply(state, action) -> ActionResult`; structural sharing via `rpds`; crates `engine` ← `phase-ai` ← adapters; 34,300+ cards parsed from MTGJSON\[1\] |
| **rustmage** | Cards authored in Rust, guided by XMage\[24\] | 517 Foundations cards with rules tests; Python bindings for RL\[24\] |
| **arcana** | Rust engine for RL research\[25\] | "Generated card catalog of over 20,000 cards"; pure `step` function plus a session layer for human play\[25\] |
| **kiln-mtg** | Commander/cEDH-first, rules-exact\[3\] | 278 verified real cards; exposes trigger ordering, replacement ordering, targets, modes, X and mana payment as choices; refuses unverified cards\[3\] |
| **cantrip-rs** | Agent-written ("coding agents write almost all of the project")\[26\] | Keeps per-card context small so a new card takes "under three minutes"\[26\] |

**Hard-won invariants from MTG-Ichor to copy into our CLAUDE.md:**
1. Never read printed characteristics for objects on the battlefield or stack. Always query the layer-computed view.\[2\]
2. A static ability's effect exists only while its source *currently has* that ability, so re-check existence during the layer walk.\[2\]
3. Determinism at the decision boundary: option lists iterate in a stable order (timestamp order), never `HashMap` order, and never random UUIDs (MTG-Ichor notes its `ObjectId` "is a v4 UUID," which is why ordering by it is forbidden).\[2\] **We go further: `ObjectId` is a monotonically increasing `u32`.**

---

## 3. Rules Source

- **Authoritative source:** the Comprehensive Rules at https://magic.wizards.com/en/rules. The current document is **effective November 6, 2026** (the *Magic: The Gathering | Star Trek* update, which added 508.4e, 700.17, 701.72 and 702.196).\[27\]\[28\]\[29\] The previous one was effective September 25, 2026 (added 506.6 "attacks a player alone," renumbering old 506.6–506.7 to 506.7–506.8).\[30\]\[31\]\[32\] Download the TXT (`https://media.wizards.com/2026/downloads/MagicCompRules%2020261106.txt`) into `docs/rules/` locally. **Do not commit it.** The Fan Content Policy excludes "verbatim copying and reposting" of Wizards' content.\[33\]
- Rules numbers move between releases, so **cite rules by number plus version in code comments** and run a CI job that diffs your pinned version against the published one.
- Verbatim anchors used throughout this spec (Aug 7, 2026 text; sections unchanged in later bulletins):
  - 101.4 (APNAP): "If multiple players would make choices and/or take actions at the same time, the active player … makes any choices required, then the next player in turn order … followed by the remaining nonactive players in turn order. Then the actions happen simultaneously."\[34\]
  - 117.4: "If all players pass in succession (that is, if all players pass without taking any actions in between passing), the spell or ability on top of the stack resolves or, if the stack is empty, the phase or step ends."\[34\]
  - 117.5: "Each time a player would get priority, the game first performs all applicable state-based actions as a single event … then repeats this process until no state-based actions are performed. Then triggered abilities are put on the stack … These steps repeat in order until no further state-based actions are performed and no abilities trigger."\[34\]
  - 103.4c / 119.1c: Commander starting life is 40. 104.3j: "a player who's been dealt 21 or more combat damage by the same commander over the course of the game loses the game."\[34\]
  - 903.8: a commander cast from the command zone "costs an additional {2} for each previous time the player casting it has cast it from the command zone that game."\[35\]\[36\]
- **Action for Claude Code:** before implementing each rules area, extract the exact current text of the governing sections (603.3b, 613.1–613.8, 614–616, 704, 800.4, 903) from the local TXT into `docs/rules-notes/<area>.md`, and write the atomic tests from that text. This spec paraphrases them below. The paraphrases are not authoritative.

---

## 4. Cargo Workspace Layout

```
mtg/
├── Cargo.toml                 # [workspace], shared lints: deny(warnings) in CI
├── CLAUDE.md                  # invariants, commands, milestone status (keep < 300 lines)
├── docs/{rules/ (gitignored), rules-notes/, decisions.md}
├── crates/
│   ├── engine-core/           # PURE. no I/O, no threads, no wall clock, no ambient RNG
│   │   ├── ids.rs  state/  zones.rs  objects.rs  mana.rs  costs.rs
│   │   ├── actions/           # GameAction proposals + the single execute chokepoint
│   │   ├── replacement/       # CR 614–616 pipeline
│   │   ├── triggers/          # CR 603: watchers, pending triggers, APNAP stacking
│   │   ├── layers/            # CR 613 characteristic computation + memo
│   │   ├── sba.rs  priority.rs  turn.rs  combat/  stack.rs  targeting.rs
│   │   ├── commander.rs       # CR 903
│   │   ├── decision.rs        # PendingDecision / Response types
│   │   └── view.rs            # PlayerView: redacted, per-player projection
│   ├── card-model/            # serde types for the card DSL (Effect, Cost, Filter, ...)
│   ├── card-db/               # Scryfall import → oracle.bin; loads + validates RON scripts
│   ├── cards/                 # data: cards/<first-letter>/<oracle_slug>.ron + custom.rs
│   ├── ai/                    # heuristic agent, evaluator, later mcts/ismcts
│   ├── harness/               # CLI: run games headless, replay, fuzz, coverage report
│   └── app/                   # eframe/egui desktop UI, image cache, settings
└── tests/{rules/, cards/, golden/}
```

Dependency rule: `engine-core` depends only on `card-model`, `serde`, `rand_chacha` (seeded, portable), `smallvec`, and optionally `im`/`rpds`. **`app` and `ai` depend on `engine-core`, never the reverse.** CI enforces this.

---

## 5. Core Data Structures

```rust
// ids.rs: dense, monotonic, deterministic
pub struct ObjectId(u32);         // new on every zone change (CR 400.7)
pub struct CardInstanceId(u32);   // stable physical-card identity (commander tracking)
pub struct PlayerId(u8);          // seat 0..4
pub struct Timestamp(u64);        // CR 613.7; global counter

pub enum Zone { Library, Hand, Battlefield, Graveyard, Stack, Exile, Command }

pub struct GameState {
    pub rng: ChaCha8Rng,                       // the ONLY randomness source
    pub next_object_id: u32, pub next_timestamp: u64,
    pub objects: BTreeMap<ObjectId, GameObject>,   // or im::OrdMap for cheap clones
    pub players: SmallVec<[PlayerState; 4]>,
    pub zones: ZoneIndex,                      // ordered Vec<ObjectId>s; battlefield by timestamp
    pub stack: Vec<StackItem>,
    pub turn: TurnState,                       // AP, phase, step, priority holder, passes-in-succession
    pub continuous: ContinuousEffectRegistry,  // layer effects with timestamps + durations
    pub replacements: ReplacementRegistry,
    pub delayed_triggers: Vec<DelayedTrigger>,
    pub pending_triggers: Vec<PendingTrigger>,
    pub combat: Option<CombatState>,
    pub lki: LkiCache,                         // last-known information snapshots
    pub resolution: Vec<ResolutionFrame>,      // explicit continuation stack (serializable)
    pub layer_cache: LayerCacheHandle,         // derived; #[serde(skip)]
    pub log_hash: u64,                         // rolling hash of events for golden tests
}

pub struct PlayerState {
    pub id: PlayerId, pub life: i32,            // starts 40 (CR 103.4c)
    pub has_lost: bool, pub has_left: bool,
    pub mana_pool: ManaPool, pub poison: u32, pub lands_played_this_turn: u8,
    pub commander_damage_taken: BTreeMap<CardInstanceId, u32>,
    pub commander_casts: BTreeMap<CardInstanceId, u32>,  // tax (CR 903.8)
    pub color_identity: ColorSet,
}

pub struct GameObject {
    pub id: ObjectId, pub card_instance: CardInstanceId,
    pub card: Arc<CardDef>,          // immutable printed data + compiled abilities
    pub owner: PlayerId, pub controller: PlayerId,
    pub zone: Zone, pub timestamp: Timestamp,
    pub status: Status,              // tapped, face_down, phased_out...
    pub counters: BTreeMap<CounterKind, u32>,
    pub damage_marked: u32, pub attached_to: Option<ObjectId>,
    pub is_commander: bool, pub is_token: bool, pub summoning_sick: bool,
    pub chosen: ChosenValues,        // "as enters, choose…", X, modes
}

pub enum StackItem {
    Spell   { obj: ObjectId, targets: Targets, modes: Vec<u8>, x: Option<u32> },
    Ability { source: ObjectId, source_lki: LkiRef, ability: AbilityRef, controller: PlayerId,
              targets: Targets, x: Option<u32>, trigger_event: Option<EventRef> },
}
```

**Characteristics are never stored mutably.** `fn chars(state, id) -> &Characteristics` returns the layer-computed view (memoized per state generation). Printed data lives in `Arc<CardDef>`.

**Clone cost.** With `Arc<CardDef>`, `SmallVec` and ~300–500 live objects in a 4-player Commander game, a plain `#[derive(Clone)]` is a few tens of KB of memcpy. Measure before reaching for persistent maps. phase-rs uses `rpds` structural sharing, which is the fallback if profiling shows clone cost dominates search.\[1\]

### Actions, events and decisions

```rust
/// What the outside world submits. Always an answer to the CURRENT PendingDecision.
pub enum Response { Pick(Vec<usize>), Number(u32), Allocate(Vec<u32>), Order(Vec<usize>) }

pub struct PendingDecision {
    pub player: PlayerId,
    pub kind: ChoiceKind,        // semantic context for UI/AI
    pub prim: Primitive,         // the shape of the answer
    pub options: Vec<OptionDesc>,// stable order (timestamp/seat order)
}

pub enum Primitive {
    PickN { min: usize, max: usize },
    PickNumber { min: u32, max: u32 },
    Allocate { total: u32, recipients: usize, per_min: Vec<u32>, per_max: Vec<u32> },
    Order { n: usize },
}

pub enum ChoiceKind {
    Priority, Mulligan, ChooseTargets { slot: u8 }, ChooseModes, ChooseX, PayMana,
    DeclareAttackers, DeclareBlockers, AssignCombatDamage, OrderTriggers,
    ChooseReplacement, CommanderToCommandZone, SearchLibrary, Discard, Sacrifice,
    MayAbility, PayUnlessCost, ScryOrder, ChooseColor, ChoosePlayer, // exhaustive match finds every UI site
}

pub enum Event {                  // what DID happen (post-replacement)
    ZoneChange { obj: ObjectId, new_obj: ObjectId, from: Zone, to: Zone, cause: Cause },
    DamageDealt { source: ObjectId, target: DamageTarget, amount: u32, combat: bool, is_commander: bool },
    LifeChanged { player: PlayerId, delta: i32 }, SpellCast { obj: ObjectId, from: Zone },
    PlayerLost { player: PlayerId, reason: LossReason }, PlayerLeft { player: PlayerId },
    // ...
}

/// The engine's single public entry point.
pub fn step(state: &mut GameState, resp: Response) -> Result<StepOutput, IllegalResponse>;
pub struct StepOutput { pub events: Vec<Event>, pub next: Option<PendingDecision> /* None = game over */ }
```

**Why this shape (decision = data, not callback).** MTG-Ichor's `DecisionProvider` is a trait the engine calls.\[2\] We invert it: the engine **suspends** and returns a `PendingDecision`, and the driver feeds back a `Response`. That makes the engine re-entrant without async, makes every game a plain list of `Response`s (a replay), and lets the AI clone the state at any suspension point. Internally, resolution code is written as an explicit continuation/state machine (`ResolutionFrame` stack in `GameState`) so that "pause mid-resolution for a choice" is serializable. **This is the single most important structural decision. Get it right in M0.**

The four primitives follow MTG-Ichor's minimal set. Adding a new decision means adding a `ChoiceKind` variant, and the compiler then points at every UI/AI match arm.\[2\]

**Agent interface (shared by UI and AI):**
```rust
pub trait Agent: Send {
    fn decide(&mut self, view: &PlayerView, d: &PendingDecision) -> Response;
}
```
`PlayerView` is a redacted projection: opponents' hands and libraries are counts only, face-down objects are hidden. **The AI never receives `&GameState`**, which structurally prevents XMage-style omniscience. Search AIs get `PlayerView` plus a `Determinizer` that samples a consistent full `GameState`.

---

## 6. Turn, Priority and Stack Loop

Pseudocode for the driver inside `step` (it runs until it needs a decision):

```
loop {
  match state.turn.cursor {
    TurnBasedActions(step)   => do_tba(step)     // untap, draw, combat damage, cleanup discard...
    CheckSbaAndTriggers      => {
        loop { let any = run_sbas_simultaneously(); if !any { break } }   // CR 117.5 / 704
        if !pending_triggers.is_empty() { return ask OrderTriggers (APNAP: AP first, then each NAP in turn order) }
        if new SBAs became applicable → repeat
        cursor = GivePriority(holder)
    }
    GivePriority(p)          => return PendingDecision{ player: p, kind: Priority, options: legal_plays(p) + Pass }
    AfterPass(p)             => { passes_in_succession += 1;
                                  if passes_in_succession == players_in_game { resolve_top_or_advance_step() }
                                  else { cursor = GivePriority(next_in_turn_order(p)) } }
    AfterAction(p)           => { passes_in_succession = 0; cursor = CheckSbaAndTriggers then GivePriority(p) } // CR 117.3c
  }
}
```

Key rules:
- **Passes in succession** counts only players still in the game. Reset it on any action (CR 117.4).
- **Triggers are stacked in APNAP order** (CR 603.3b; verify the text). The AP puts all of theirs on first, in the order they choose, then each NAP in turn order, so the last NAP's triggers resolve first. This matters for 4-player Commander and is easy to get wrong.
- **Mana abilities** don't use the stack. They are offered inside `PayMana` decisions (and as a priority-window "float mana" option). Auto-payment is a UI/AI convenience that produces the same `Response`s.
- **No player gets priority in untap**, and usually not in cleanup (117.3a).\[34\] Cleanup repeats if SBAs or triggers happen.
- **"Stops" are a UI concern, not an engine concern.** The engine always asks. The `app` layer auto-passes using user-configured stops. This avoids the Forge bug where the human wasn't offered a response window because the engine evaluated "can respond" with AI mana logic.\[37\]

---

## 7. Ability, Effect and Card Script Representation

### 7.1 Vocabulary (card-model crate, serde types)

```rust
pub struct CardScript {
    pub oracle_id: Uuid, pub name: String,           // join key to Scryfall data
    pub abilities: Vec<AbilityDef>,
    pub ai_hints: Option<AiHints>,
    pub verified: bool,                              // only verified cards load in strict mode
    pub tests: Vec<String>,                          // names of tests covering this card
}

pub enum AbilityDef {
    Spell     { effect: Effect, targets: Vec<TargetSpec>, modes: Option<ModeSpec> },
    Activated { cost: Cost, effect: Effect, targets: Vec<TargetSpec>, timing: Timing, is_mana: bool, zone: ZoneFilter },
    Triggered { when: TriggerSpec, intervening_if: Option<Cond>, effect: Effect, targets: Vec<TargetSpec>, optional: bool },
    Static    { layer_effects: Vec<LayerEffect>, affected: Filter, condition: Option<Cond>, zone: ZoneFilter },
    Replacement { event: ReplEventSpec, condition: Option<Cond>, replace_with: ReplaceWith, self_replacement: bool, optional: bool },
    Keyword(Keyword),                                 // expands to the above at compile time
    Custom(String),                                   // name of a registered Rust fn; the escape hatch
}

pub enum Effect {
    Seq(Vec<Effect>), If { cond: Cond, then: Box<Effect>, else_: Option<Box<Effect>> },
    ForEach { players: PlayerSel, effect: Box<Effect> },           // APNAP-ordered
    Draw { who: PlayerSel, n: Num }, GainLife { who: PlayerSel, n: Num }, LoseLife { .. },
    DealDamage { source: ObjSel, to: DamageTargetSel, n: Num },
    Destroy { what: ObjSel, no_regen: bool }, Exile { what: ObjSel }, Sacrifice { who: PlayerSel, filter: Filter, n: Num },
    MoveZone { what: ObjSel, to: Zone, tapped: bool, position: LibPos },
    Search { who: PlayerSel, zone: Zone, filter: Filter, n: Num, then: Box<Effect> },
    CreateToken { token: TokenRef, n: Num, controller: PlayerSel, tapped: bool },
    AddMana { mana: ManaSpec }, PutCounters { what: ObjSel, kind: CounterKind, n: Num },
    CreateContinuous { effects: Vec<LayerEffect>, affected: ObjSel, duration: Duration },
    UnlessPays { who: PlayerSel, cost: Cost, otherwise: Box<Effect> },
    Custom(String),
}
```

**Escape hatch policy:** `Custom("name")` maps to `fn(&mut ResolveCtx) -> ResolveStep` registered in `cards/src/custom.rs`. The rule: if three or more cards would need the same custom code, promote it to a vocabulary variant. Custom fns must be pure functions of `ResolveCtx` (no statics, no RNG except `ctx.rng()`). At load, `card-db` compiles each script plus Scryfall characteristics into an `Arc<CardDef>` and **rejects** scripts with unknown variants, mismatched target slots or missing tests.

### 7.2 Why RON + Rust rather than Rhai or Lua
- **RON** maps 1:1 to serde enums with Rust-like syntax and comments. It's easy for Claude Code to read and write and is validated at load. **Caveat:** RON does not support `#[serde(tag = "type")]` or `#[serde(untagged)]`, and `flatten` is limited, so use **externally tagged enums only** in `card-model`.\[38\]
- **Embedded scripting (Rhai/mlua) is a poor fit for the hot path.** Rhai is a tree-walking interpreter: its README reports "1 million iterations in 0.14 sec on a single-core 2.6 GHz Linux VM" with the standard AST walker, and says its experimental Rhai Grain bytecode VM is "around 1.8-2x" faster. In a Hacker News thread on Rhai (Jan 2025), one user wrote: "I used it for a game and I had to redo it in lua some months later because it was a big bottleneck." More importantly, closures inside a script VM are hard to **clone, serialize and determinize** for AI search. Data-only scripts get those properties for free.

### 7.3 Example card scripts

```ron
// Sol Ring: mana ability, no stack
CardScript(name: "Sol Ring", verified: true, abilities: [
  Activated(cost: Cost(tap_self: true), is_mana: true, timing: Any, zone: Battlefield,
    effect: AddMana(mana: Fixed("CC")), targets: []),
])

// Swords to Plowshares: targeting + "its controller" via LKI
CardScript(name: "Swords to Plowshares", verified: true, abilities: [
  Spell(targets: [TargetSpec(id: 0, filter: Creature, count: Exactly(1))],
    effect: Seq([
      Exile(what: Target(0)),
      GainLife(who: ControllerOfLki(Target(0)), n: PowerOfLki(Target(0))),
    ])),
])

// Glorious Anthem: static, layer 7c
CardScript(name: "Glorious Anthem", verified: true, abilities: [
  Static(zone: Battlefield, affected: And([Creature, ControlledBy(You)]),
    layer_effects: [ModifyPT(power: 1, toughness: 1)]),
])

// Doubling Season: two replacement effects
CardScript(name: "Doubling Season", verified: true, abilities: [
  Replacement(event: CreateTokens(controller: You), replace_with: MultiplyCount(2), self_replacement: false, optional: false),
  Replacement(event: PutCounters(on: And([Permanent, ControlledBy(You)])), replace_with: MultiplyCount(2), self_replacement: false, optional: false),
])
```

Commanders need no special script. The deck file marks the commander, and `commander.rs` supplies command-zone casting, tax and damage tracking.

---

## 8. The Hard Parts and Recommended Designs

### 8.1 Single mutation chokepoint + replacement pipeline (CR 614–616)
Every state change is a `GameAction` proposal (`MoveZone`, `Damage`, `GainLife`, `Draw`, `PutCounters`, `CreateToken`, `Tap`...). `execute(proposal)`:
1. Gathers applicable replacement/prevention effects (from the registry plus "as enters" look-ahead for battlefield entry, CR 614.12), evaluated against the **would-be** state.
2. If more than one applies, asks the **affected object's controller / affected player** to choose one (`ChoiceKind::ChooseReplacement`), applying self-replacement effects first. Apply it, mark it used for this event, and loop until none remain (CR 616.1; verify exact ordering text in the local TXT).
3. Performs the mutated event and emits `Event`s.

The commander "may put it into the command zone" choice is modeled per the current 903.9 text. As of recent rules it is a **state-based-action-style owner choice when the commander is in graveyard or exile** (903.9a).\[36\] Older rules used a replacement effect for hand/library.\[39\] Implement exactly what the current TXT says and add golden tests for "commander dies → owner chooses."

### 8.2 Triggered abilities (CR 603)
- Triggers watch **performed events** (post-replacement). This is why the chokepoint lands first, following MTG-Ichor's ordering.\[2\]
- Leaves-the-battlefield triggers "look back in time." Snapshot **LKI** for every object leaving a zone (characteristics as last computed, controller, counters) and keep it until the triggered ability resolves.
- Pending triggers queue until the next time a player would receive priority, then go on the stack in APNAP order with the controller choosing order (`OrderTriggers`).
- Intervening-"if" clauses are checked twice: on trigger and on resolution.
- Delayed triggers ("at the beginning of the next end step") live in `delayed_triggers` with a lifetime.

### 8.3 Layer system (CR 613)
Layers (verify sublayers in the current TXT): 1 copy (1a copiable, 1b face-down), 2 control, 3 text, 4 type, 5 color, 6 ability add/remove, 7 P/T (7a CDAs, 7b set, 7c modify incl. counters, 7d switch). Within a layer, apply in **timestamp order** (613.7) unless a **dependency** (613.8) exists: effect A depends on B if B's application would change A's existence, what A applies to, or what A does, and both are in the same layer (or sublayer). Dependencies override timestamps, and dependency loops fall back to timestamp order.

Design:
```rust
fn compute_characteristics(state: &GameState) -> LayeredView {
  let mut view = printed_view(state);
  for layer in LAYERS {                         // 1a,1b,2,3,4,5,6,7a,7b,7c,7d
     let mut effs = active_effects_in(layer, &view);   // existence re-checked against *current* view
     order_by_dependency_then_timestamp(&mut effs, &view);   // 613.8: pick an effect depending on no un-applied effect; ties → earliest timestamp
     for e in effs { e.apply(&mut view) }
  }
  view
}
```
- Recompute lazily, memoized on a `state.generation` counter bumped by every performed event.
- **For the precon milestone, many decks only exercise layers 2, 4, 6, 7b, 7c.** Still build all layers, but ship M2 tests for: anthem + counters + "becomes 0/1" (7b vs 7c ordering), Humility/Opalescence-style dependency (golden case), control-change auras, and ability-removing effects that remove an anthem's own source ability.

### 8.4 State-based actions (CR 704)
`run_sbas_simultaneously()` gathers *all* applicable SBAs from one snapshot and performs them as one event batch (zero-life loss, 21 commander damage loss, lethal damage/deathtouch, 0 toughness, legend rule, illegal auras, +1/+1 and −1/−1 counter annihilation, planeswalker 0 loyalty, tokens in non-battlefield zones). Repeat until none apply. **Never** run SBAs during resolution.

### 8.5 Combat with multiple defending players
- `DeclareAttackers` options: for each untapped creature, choose `{no attack} ∪ {each opponent still in game} ∪ {each planeswalker/battle they control}`. Encode as a **per-creature sequence of choices** inside a declaration transaction, validated at the end (requirements/restrictions, attack costs). This avoids the combinatorial option list behind Forge's #7625 hang.
- Blockers are declared by **each defending player in APNAP order** (each defender only blocks creatures attacking them or their permanents).
- Damage assignment uses the current 2025+ rules (MTG-Ichor notes the "2025 damage-assignment rules (CR 506–511)").\[2\] Read the current 510.1 text before implementing. Don't rely on memory of "damage assignment order."
- Commander damage: `DamageDealt{combat: true, is_commander: true}` increments `commander_damage_taken[card_instance]`. The designation follows the card across zone changes, so key by `CardInstanceId`, not the per-zone `ObjectId`.\[40\]

### 8.6 Commander rules (CR 903)
- Deck validation: 100 cards, singleton except basics, every card's color identity ⊆ commander's identity (from Scryfall `color_identity`).
- Start: commander in command zone, 40 life, multiplayer mulligan.
- Casting from command zone: add `{2} × commander_casts[card]` generic to total cost (903.8). Increment on cast from command zone only.\[35\]
- Command-zone return per current 903.9.
- 21 combat damage from the same commander → loss (104.3j/903.10a), checked as an SBA.\[41\]

### 8.7 Player leaving the game (CR 800.4; verify exact text)
When a player loses in multiplayer: all objects they own leave the game, effects giving them control of others' objects end, spells/abilities they control on the stack cease to exist, and turn order skips them. Golden tests: "player loses while their Control Magic-style aura is on another player's creature," "player loses with their trigger on the stack," "player loses during their own turn."

---

## 9. AI Architecture (phased)

### Phase A: Heuristic agent (M3)
- **Board evaluation per player** `V(p)` = life (concave: losses near 0 weigh more) + Σ creature value (P/T, keywords, evasion) + card advantage + mana development + commander presence − **lethal exposure**. Exposure includes *commander damage remaining* (`21 − dmg_taken[c]`) for every opposing commander. That one feature fixes the Forge failures in #12269/#9960.
- **Threat assessment** `T(q)` for each opponent q: board power aimed at me, how close q is to winning (life and commander-damage clocks they present), card/mana advantage, known combo pieces (`ai_hints.threat`), and recent aggression toward me (a grudge term with decay, the simplest "politics").
- **Attack target selection:** for each potential attacker, score each defender by `expected_damage × T(q) × lethal_bonus − crackback_risk`, where crackback is estimated from untapped opposing power that could hit me before my next untap. Attack the player you can kill, or else the highest-threat player whose blocks are worst for them. Add a "don't become the archenemy" penalty when my score is far above the table average.
- **Blocking:** chump only when unblocked damage (life *or* commander damage) is lethal or near-lethal. Otherwise use value blocks.
- **Spell use:** removal targets the highest `T(q)`-weighted permanent. Wraths fire when `ΔΣ V(opponents) − ΔV(me)` is above a threshold. Counterspells are kept for spells scored as game-winning.
- **Determinism:** heuristics use only `PlayerView` plus the agent's own seeded RNG.

### Phase B: Determinized search (M5)
- **Determinization:** sample hidden zones (opponents' hands and libraries, my library order) consistent with public info (revealed cards; precon decklists are public). Cowling, Ward & Powley's results (IEEE TCIAIG 2012, doi:10.1109/TCIAIG.2012.2204883) guide the parameters: **ensemble determinization** (many independent UCT trees with draws fixed per tree, root visit counts summed) beat single-tree chance-node MCTS (which won only 23% vs their expert rules player). With 10,000 simulations, "the best number of determinizations occurs between 20 and 100," and "around 40 determinizations, each with 250 simulations, provides good play." In their tests, "reduced rules rollouts significantly outperform expert rules rollouts (by around 10% in most cases)." Decomposing "which cards to play" into a **binary decision tree** (yes/no per card, by descending mana cost) plus **dominated-move pruning** lifted the win rate vs the expert player from 43.9% at 0.79 s/move (no enhancements) to 50.5% at 0.20 s/move (their Table V).
- **Multiplayer search policy:** use **max^n** values (a vector of per-player rewards) in MCTS. Nijssen & Winands' overview of multiplayer search (tested on Chinese Checkers, Focus, Rolit and Blokus) concludes that "Best-Reply Search is generally the best minimax-based search technique" and that "Monte-Carlo Tree Search performs best with the maxn tree structure." For short budgets, a BRS-flavoured rollout (only the most dangerous opponent responds) is a cheap opponent model.\[42\]
- **Budget = iterations**, never wall clock, so replays reproduce. Each search is seeded from `(game_seed, decision_index, seat)`.
- Rollout policy = the Phase A heuristic with ε-randomness.

### Phase C: ISMCTS (M6)
Information Set MCTS (Cowling, Powley & Whitehouse 2012) searches a single tree of **information sets** and re-determinizes every iteration, which addresses **strategy fusion**: determinized search "effectively makes the erroneous assumption that different decisions can be made from these states."\[43\] Use multiple-observer ISMCTS so opponent nodes don't leak the root's hidden information.\[44\]

---

## 10. Testing Strategy

1. **Atomic rules tests** (`tests/rules/cr_613_layers.rs` ...), each annotated `// CR 613.8 (2026-11-06)`. Use a builder DSL modeled on XMage: `Game::test().player(0).battlefield(["Glorious Anthem","Grizzly Bears"])...; g.cast(0, "Humility"); g.resolve_all(); assert_pt(g, "Grizzly Bears", 1, 1);`.
2. **Card tests**: every `verified: true` card names at least one test. CI fails otherwise.
3. **Golden replay tests**: a replay is `(seed, decklists, engine_version, Vec<Response>)`. Store the expected rolling event hash plus final state hash. Any diff fails CI. Regenerate only via an explicit `harness bless` command, and review diffs.
4. **Property tests (`proptest`)**: random legal `Response`s from random seeds. Invariants: no panic; every object is in exactly one zone; `serde` round-trip yields an identical hash; `clone()` then the same responses yields identical events; priority always reaches a decision or game over within N steps; life totals only change via events.
5. **Fuzz harness**: `harness fuzz --games 1000 --seed 42 --threads 16 --players 4`, all-AI, with panics and stuck-loop detection. MTG-Ichor requires three runs at one seed to agree on every line, a cheap and strong determinism check.\[2\] Adopt it.
6. **Coverage report**: `harness coverage --decks precons/*.dck` lists every card with verified/unverified status and the vocabulary variants it uses.

---

## 11. Desktop UI

**Recommendation: `eframe`/`egui` for M4, with the option to move to Bevy + `bevy_egui` later.**
- egui (0.37) is pure-Rust immediate mode and is the most widely used of the three pure-Rust options (one 2026 write-up counts ~13M downloads, "around ten times as many as both iced and slint").\[45\]\[46\] Immediate mode fits a game whose screen is a pure function of `PlayerView` plus UI state.
- iced (0.14) uses the Elm architecture (a good conceptual fit with `step`), but it has fewer ready-made widgets for freeform card layouts.\[45\]
- Slint (1.18) is polished but brings its own markup language and dual licensing.\[45\]\[46\]
- Tauri (2.12) means a React/TS front end.\[45\] phase-rs shows this works, but on Linux it depends on WebKitGTK. phase-rs documents the AppImage burning "close to a full core at idle on the NVIDIA proprietary driver" and silent audio without GStreamer plugins.\[1\] That's avoidable friction on Arch.

UI architecture:
- The `app` thread owns `GameState`. The AI runs on worker threads with **cloned** state and returns a `Response` over a channel. The UI renders only `PlayerView(human)`.
- Layout: four player areas, stack panel, event log (rendered from `Event`s), command zone with tax shown, commander-damage matrix (4×4), phase bar with configurable stops.
- Every `ChoiceKind` gets a widget. Targeting is click-to-select among highlighted `options` indices, which the engine already validates.
- Images: lazy-download Scryfall `normal` images into `~/.cache/<app>/cards/` on first view. Text-only fallback rendering for offline use.

---

## 12. Card Data, Scryfall and Legal

**Scryfall:**
- Use the **bulk data** files (Oracle Cards for gameplay; Default Cards if you need printings and images), downloaded via `GET /bulk-data` → `download_uri`. Scryfall says gameplay data changes rarely, so "downloading card data once per week or right after set releases would most likely be sufficient." Cache locally at least 24 h.\[47\]
- For the API (only for incidental lookups), Scryfall asks for "50 – 100 milliseconds of delay" between requests ("10 requests per second on average"). Overloading can produce HTTP 429 and "a temporary or permanent ban of your IP address." Files on `*.scryfall.io` (images, bulk files) don't have these limits.\[47\]
- Data terms: no paywall; don't use Scryfall data "to create new games" or imply the cards come from another game; don't "simply repackage, republish, or proxy Scryfall data."\[47\]
- Image terms: don't crop or cover the copyright/artist line, distort, blur, color-shift or watermark. When showing `art_crop`, show the artist name and copyright elsewhere in the same interface.\[47\]
- Import pipeline: `card-db import` → filter to the deck pool (plus tokens referenced by `all_parts`) → `oracle.bin` (bincode) keyed by `oracle_id`. Store the bulk file's `updated_at` in `oracle.bin` so replays record which data version they used.

**Wizards Fan Content Policy (personal, non-commercial):**
- It must be free: no payments, surveys, "downloads, subscriptions, or email registration" to access it, and no selling.\[33\]
- Include the notice: "[Title] is unofficial Fan Content permitted under the Fan Content Policy. Not approved/endorsed by Wizards. Portions of the materials used are property of Wizards of the Coast. ©Wizards of the Coast LLC."\[33\]
- Don't use Wizards' logos and trademarks. The FAQ's list of logos you "may *not* include" pictures the **mana symbols** and guild symbols.\[33\] Render costs as text (`{2}{G}`) or your own neutral glyphs rather than bundling official symbol art.
- Fan Content excludes "verbatim copying and reposting" (e.g., freely distributing rules content, proxies).\[33\] **Don't commit the CR, card images or bulk JSON to a public repo.** Fetch at runtime into the user's cache, as phase-rs does.\[1\]
- Wizards can revoke permission "at any time—for any reason or no reason."\[33\]
- **Net:** for a private hobby app on your own machine, risk is minimal. If you publish the repo, ship code plus your RON scripts, fetch Wizards/Scryfall data at runtime, and include the notice. Note that the policy also says you can't incorporate "game mechanics" without permission,\[33\] a clause no open-source engine (Forge, XMage, phase-rs) has been held to, but it is a real residual risk if you ever distribute.

---

## 13. Milestones and Acceptance Criteria

**M0: Skeleton and determinism (1–2 wks)**
- Workspace, CI (`fmt`, `clippy -D warnings`, tests), CLAUDE.md, `GameState`, IDs, zones, `move_object`, seeded RNG, `step()` with suspension/`PendingDecision`, serde round-trip.
- ✅ Two players with vanilla creatures and basic lands play a full game via `harness` with random agents. ✅ The same seed + responses → identical event hash across 3 runs and across a save/load in the middle of a game. ✅ `clone()` of a mid-game state then replaying the same responses → identical events.

**M1: Priority, stack, turn structure, mana, casting (2–3 wks)**
- CR 117 priority with passes-in-succession, 601.2 casting pipeline (modes, targets, X, cost totals, mana payment), activated and mana abilities, resolution, 4-player turn order.
- ✅ Atomic tests for 117.3–117.5, APNAP choice order (101.4), fizzling on illegal targets. ✅ 4 random agents complete 1,000 games without panic.

**M2: Chokepoint, replacement, triggers, SBAs, layers (4–6 wks; the core)**
- In order: action chokepoint → replacement/prevention pipeline → triggers + LKI + APNAP stacking → SBAs → layers 1–7 with dependency ordering.
- ✅ Golden tests: Doubling Season + token creation; two competing replacement effects with player choice; dies-trigger reading LKI power; Glorious Anthem + −1/−1 counters + "becomes 0/1"; a dependency case; legend rule; simultaneous lethal damage to multiple players.

**M3: Combat + Commander + multiplayer exits; heuristic AI v1 (3–4 wks)**
- Multi-defender attacks, per-defender blocks in APNAP, damage assignment per current CR, first/double strike, trample, deathtouch, lifelink, menace, vigilance, flying/reach; CR 903 (command zone, tax, 21 damage, color identity validation, 40 life); CR 800.4 player leaving.
- ✅ Commander tax test across 3 casts. ✅ 21 commander damage across a death and recast. ✅ Player leaves with their aura on another player's creature and their trigger on the stack. ✅ Heuristic AI never takes lethal commander damage when a legal non-losing block exists (property test over generated boards).

**M4: Four precons fully playable + egui app (4–6 wks)**
- Pick four precons. Freeze the decklists in `decks/`. Choose ones whose mechanics are mostly evergreen keywords, ETB/dies triggers, anthems, removal and ramp, and avoid MDFCs, phasing, copy-heavy or face-down themes for v1.
- ✅ `harness coverage` shows 100% verified for all ~400 cards. ✅ Each card has ≥1 test. ✅ 10,000 AI-vs-AI 4-player games with 0 panics and 0 stuck games (max-turn cap ≤ 1%). ✅ The human can complete a game in the app using only the mouse: every `ChoiceKind` has a widget. ✅ Save/load and replay a game from file in the app.

**M5: Determinized ensemble MCTS (3–4 wks)**
- `Determinizer`, max^n MCTS with iteration budget, binary decomposition of multi-card choices, dominated-move pruning, heuristic rollouts with ε.
- ✅ Over 2,000 seeded 4-player games (seat-rotated), MCTS agent win rate vs 3 heuristic agents ≥ 35% (baseline 25%). ✅ Decision latency p95 ≤ 2 s on the dev machine at the default budget. ✅ Deterministic: the same seed → the same moves.

**M6: ISMCTS + polish**
- ✅ ISMCTS ≥ the determinized ensemble in a head-to-head at equal iterations. ✅ Configurable difficulty (iterations, heuristic noise).

---

## 14. CLAUDE.md (paste this as the repo's CLAUDE.md seed)

```markdown
# CLAUDE.md: MTG Commander engine

## Invariants (never violate)
1. engine-core has NO I/O, threads, wall clock, or ambient RNG. Randomness = state.rng only.
2. Every observable mutation goes through actions::execute(proposal). No direct writes to GameState from effect code.
3. Never read CardDef printed characteristics for objects on battlefield/stack — use layers::chars(state, id).
4. A static ability's effect exists only if its source currently has that ability (re-check during the layer walk).
5. Option lists are emitted in stable order (timestamp, then seat order). Never iterate HashMap. IDs are monotonic u32.
6. The AI never sees GameState — only PlayerView (+ Determinizer for search).
7. AI budgets are iteration counts, never time.
8. Unverified cards do not load in strict mode. Never approximate a card.
9. Cite rules as `// CR 613.8 (2026-11-06)`. Before implementing a rules area, extract the current text into docs/rules-notes/.
10. Externally tagged serde enums only (RON limitation).

## Commands
cargo test --workspace
cargo run -p harness -- fuzz --games 500 --seed 42 --players 4 --threads 8
cargo run -p app

## Definition of done for any PR
- zero warnings; atomic test with CR number; golden hashes unchanged or re-blessed; fuzz 500 games clean.

## Current milestone: M0 (update as you go)
```

---

## 15. Pitfalls to Avoid

1. **Building triggers before the replacement chokepoint.** Triggers must read post-replacement truth. MTG-Ichor reordered its roadmap specifically for this.\[2\]
2. **Callbacks for decisions.** Calling a UI or AI trait from deep in resolution makes state non-serializable mid-resolution and blocks search. Suspend and return `PendingDecision` instead.
3. **Stored characteristics.** Caching "current power" on the object leads to stale-layer bugs. Always compute through layers (memoized).
4. **Nondeterminism leaks:** `HashMap` iteration, UUIDs, `rand::thread_rng()`, wall-clock AI budgets (the Forge `AI_TIMEOUT` lesson), floats in evaluation that differ across platforms.
5. **Combinatorial option lists.** Never enumerate all attack configurations or all mana payments. Decompose into sequential per-object choices (also what makes MCTS tractable, per the binary-decision-tree result).\[48\]
6. **Omniscient AI.** It's tempting because it's easy, and it makes the CPU feel like it's cheating (XMage's Mad).
7. **Commander-damage blind spots** in AI evaluation (Forge #12269, #9960).
8. **Treating ObjectId as card identity.** Objects become new objects on zone change (CR 400.7), but commander designation, commander damage and tax follow the *card*.\[40\] Keep both IDs.
9. **"Each opponent" effects in the wrong order.** Use APNAP (101.4) for simultaneous choices, and restart APNAP when a NAP's choice forces an earlier player to choose (101.4d).\[34\]
10. **Rules drift.** The CR changed three times between August and November 2026.\[28\]\[29\]\[30\]\[31\]\[34\] Pin a version, cite it, and diff it in CI.
11. **Letting card count outrun rules correctness.** Four precons done correctly beat 2,000 cards that half-work.
12. **Committing Wizards assets** (CR text, images, mana symbol art) to a public repo.

---

## Caveats

- CR paraphrases in §8 for 603, 613, 614–616, 704, 800.4 and 903.9 were not verified verbatim against the November 6, 2026 text in this research. Only 101.4, 117.3–117.5, 103.4c/119.1c, 104.3j and 903.8 were quoted from the official text (August 7, 2026 edition, unchanged in those sections per later bulletins). Claude Code must extract current text before implementing each area.
- Acceptance thresholds (35% MCTS win rate, 2 s p95) are engineering targets, not published benchmarks. Recalibrate after M3 once heuristic-agent strength is known.

## Sources

1. [GitHub - phase-rs/phase: A rules engine and game client — Rust + WASM + React · GitHub](https://github.com/phase-rs/phase)
2. [GitHub - lcmaier/MTG-Ichor: MTG Rules Simulator written in Rust, designed for performance and correctness · GitHub](https://github.com/lcmaier/MTG-Ichor)
3. [GitHub - SeanMcKeen/kiln-mtg: Rules-exact Magic: The Gathering engine in Rust, plus a simulation lab your AI can drive (Commander/cEDH-first). · GitHub](https://github.com/SeanMcKeen/kiln-mtg)
4. [Collectible Card Game Headquarters • View topic - Card Scripting Docs](https://slightlymagic.net/forum/viewtopic.php?f=52&t=6667)
5. [Spec 1: Forge card-script parser · Issue #1 · jwilliamson13-tech/mtg-forge-ai · GitHub](https://github.com/jwilliamson13-tech/mtg-forge-ai/issues/1)
6. [Creating a custom Card · Card-Forge/forge Wiki · GitHub](https://github.com/Card-Forge/forge/wiki/Creating-a-custom-Card)
7. [Forge and XMage: The best free and open source rules engines for ‘Magic: the Gathering’ - CGomesu](https://cgomesu.com/blog/forge-xmage-mtg/)
8. [Computer Programming and Magic: The Gathering: Forge Scripting](http://mtgrares.blogspot.com/2011/05/forge-scripting.html)
9. [github.com](https://github.com/Card-Forge/forge/wiki/Triggers)
10. [Collectible Card Game Headquarters • View topic - Custom Card Thread](https://slightlymagic.net/forum/viewtopic.php?f=26&t=16205&start=345)
11. [Fix card scripts: Wilt-Leaf Liege, Volcanic Offering, Urza Academy Headmaster by jczastkiewicz · Pull Request #12471 · Card-Forge/forge · GitHub](https://github.com/Card-Forge/forge/pull/12471)
12. [Clean up StaticEffect\$ when a move to the battlefield is replaced by liamiak · Pull Request #12406 · Card-Forge/forge · GitHub](https://github.com/Card-Forge/forge/pull/12406)
13. [AI willingly accepts lethal commander damage · Issue #12269 · Card-Forge/forge · GitHub](https://github.com/Card-Forge/forge/issues/12269)
14. [AI does not understand certain game mechanics · Issue #9960 · Card-Forge/forge · GitHub](https://github.com/Card-Forge/forge/issues/9960)
15. [During multiplayer matches, the AI will target the cards of it's allies to hurt them rather than help. · Issue #11701 · Card-Forge/forge · GitHub](https://github.com/Card-Forge/forge/issues/11701)
16. [AI can't find any valid attacker configuration with lots of creatures and Aurification, stuck in "waiting for opponent" · Issue #7625 · Card-Forge/forge · GitHub](https://github.com/Card-Forge/forge/issues/7625)
17. [Flat MC: play a human opponent's copy with the heuristic AI in rollouts by MostCromulent · Pull Request #12527 · Card-Forge/forge · GitHub](https://github.com/Card-Forge/forge/pull/12527)
18. [Forge: make full-sim batches replay from their seed · Issue #50 · jwilliamson13-tech/mtg-forge-ai · GitHub](https://github.com/jwilliamson13-tech/mtg-forge-ai/issues/50)
19. [Xmage · Delft Students on Software Architecture: DESOSA 2018](https://delftswa.gitbooks.io/desosa2018/content/xmage/chapter.html)
20. [Lock, Java, and two nulls: XMage edition - DEV Community](https://dev.to/pvsdev/lock-java-and-two-nulls-xmage-edition-57l4)
21. [Collectible Card Game Headquarters • View topic - Bug Reports XMage 1.4.21 SOFTWARE](https://slightlymagic.net/forum/viewtopic.php?f=70&t=20014&start=150)
22. [Collectible Card Game Headquarters • View topic - Bug Reports xmage 1.4.13](https://www.slightlymagic.net/forum/viewtopic.php?f=70&t=18799&start=45)
23. [feat(xmage): add random and native AI opponents by jdblackstar · Pull Request #13 · jdblackstar/lab · GitHub](https://github.com/jdblackstar/lab/pull/13)
24. [GitHub - altf4/rustmage: Rust-based Magic the Gathering rules engine library · GitHub](https://github.com/altf4/rustmage)
25. [GitHub - levineuwirth/arcana: A high-performance Magic: The Gathering rules engine in Rust, designed as a substrate for reinforcement-learning research. · GitHub](https://github.com/levineuwirth/arcana)
26. [GitHub - cantrip-rs/cantrip-rs: A Magic: The Gathering rules engine in Rust, with a C# server, Discord bot and web client · GitHub](https://github.com/cantrip-rs/cantrip-rs)
27. [Comprehensive rules updated, effective 2026-11-06 · Issue #6498 · parun-mtg/parun · GitHub](https://github.com/parun-mtg/parun/issues/6498)
28. [Magic: The Gathering®](https://magic.wizards.com/en/news/announcements/star-trek-update-bulletin)
29. [Release Comprehensive Rules JSON (pre-release) · natewizz/mtg-rules · GitHub](https://github.com/natewizz/mtg-rules/releases/tag/v0.2.0-data.3)
30. [Magic: The Gathering Comprehensive Rules](https://cantrip.ru/en/mtg-judge/magic-comp-rules.shtml)
31. [The rules text is behind the published comprehensive rules · Issue #6160 · parun-mtg/parun · GitHub](https://github.com/parun-mtg/parun/issues/6160)
32. [Reality Fracture Update Bulletin - MTG - Wizards of the Coast](https://magic.wizards.com/en/news/announcements/reality-fracture-update-bulletin)
33. [Fan Content Policy | Wizards of the Coast](https://company.wizards.com/en/legal/fancontentpolicy)
34. <https://media.wizards.com/2026/downloads/MagicCompRules%2020260807.pdf>
35. [Commander Tax MTG: How Much It Costs and How to Avoid It · Lotus](https://lifecounter.app/blog/commander-tax-rules/)
36. [Duel Commander - ❔FAQ](https://www.mtgdc.info/faq)
37. [Comments 62 to 23 of 62 - Neo Forge: MTG Commander vs AI by DokkodoLabs](https://dokkodolabs.itch.io/neo-forge/comments)
38. [github.com](https://github.com/wtfamidoingrn/ron)
39. [What is the command zone replacement rule?](https://www.vintageisthenewold.com/faq/what-is-the-command-zone-replacement-rule)
40. [MTG Commander Damage Explained: The 21 Rule & Every Edge Case](https://krakenthemeta.com/blog/mtg-commander-damage/)
41. [Commander Damage Rule: 21 Damage, Explained](https://themanagrove.com/articles/commander-damage-rule)
42. [(PDF) Best Reply Search for Multiplayer Games](https://www.researchgate.net/publication/224213564_Best_Reply_Search_for_Multiplayer_Games)
43. [This is a repository copy of Information Set Monte Carlo Tree Search.](https://files01.core.ac.uk/download/pdf/13505084.pdf)
44. [Re-determinizing Information Set Monte Carlo Tree Search in Hanabi](https://arxiv.org/html/1902.06075v2)
45. [Rust GUI Apps: Tauri, egui, Slint and Iced](https://rusttutorial.org/gui-apps/)
46. [Thanks for All the Frames: Rust GUI Observations](https://tritium.legal/blog/desktop)
47. <https://scryfall.com/docs/api>
48. <https://eprints.whiterose.ac.uk/id/eprint/75050/1/EnsDetMagic.pdf>
