# Forge reference: triggers, static abilities, replacement effects, keyword lines

Implementation-grade notes on how Forge (GPL v3; sources read at
`~/.cache/apnap/forge/repo`) interprets the `T:`, `S:`, `R:` and `K:` lines of its card
scripts. Written for the author of the Forge-script -> apnap translator. Behaviour is
described in our own words; only key names and one-line script strings are quoted.

Conventions used below

- Paths are relative to `forge-game/src/main/java/forge/game/` unless they start with
  `forge-gui/` or `docs/`.
- "Valid filter" = a comma-separated list of Forge selectors (`Creature.YouCtrl+Other`,
  `Card.Self`, `Player.Opponent`, …) evaluated relative to the host card and its controller.
  The selector grammar itself is out of scope here (see `card/CardProperty.java`,
  `player/PlayerProperty.java`, `spellability/SpellAbilityProperty.java`).
- "Comparator" = two-letter operator + operand: `GE1`, `EQ0`, `LT3`, `GTX` (operand may be a
  number or an SVar name; operators `LT LE EQ NE GE GT`).
- **UNVERIFIED** marks anything not confirmed by reading the Java.
- Card counts ("7193 cards") come from the corpus frequency table
  (`forge-gui/res/cardsfolder`, 34,465 scripts).

General line syntax: every `T:`/`S:`/`R:` line (and every SVar holding one) is a list of
`Key$ Value` pairs separated by ` | ` (parsed by `FileSection.parseToMap` with the `$`
separator). Unknown keys are silently ignored - a key that no Java class reads has no effect
(the corpus contains a few such dead keys, e.g. `ValidBlocked$` on `AttackerBlockedByCreature`).

Contents: [Part 1 Triggers](#part-1--triggers) · [Part 2 Static abilities](#part-2--static-abilities)
· [Part 3 Replacement effects](#part-3--replacement-effects) · [Part 4 Keyword lines](#part-4--keyword-lines-k)
· [Appendix: translator checklist](#appendix--the-non-obvious-behaviours-a-translator-must-know)

---

# Part 1 — Triggers

Sources: `trigger/Trigger.java`, `trigger/TriggerHandler.java`, `trigger/TriggerWaiting.java`,
`trigger/WrappedAbility.java`, `trigger/TriggerType.java`, `CardTraitBase.java`,
`TriggerReplacementBase.java`, `ability/AbilityUtils.java`, one `trigger/Trigger<Mode>.java`
per mode. Forge's own (incomplete) notes: `docs/Card-scripting-API/Triggers.md`.

Shape: `T:Mode$ <Mode> | <mode keys> | <generic keys> | Execute$ <SVar> | TriggerDescription$ <text>`

## 1.1 Life cycle of a trigger (what the generic keys hook into)

1. **Registration.** `TriggerHandler.resetActiveTriggers` walks every card in the game and
   keeps a trigger "active" when `isTriggerActive` passes: the `Phase$` restriction holds
   (`phasesCheck`), the trigger is not suppressed, and the host card is in one of the
   `TriggerZones` (`zonesCheck`; a phased-out host never passes). Leaves-the-battlefield style
   triggers ("looks back in time", see 1.2 `TriggerZones`) of a card that was not on the
   battlefield at the last state check are skipped.
2. **Event.** Engine code calls `runTrigger(TriggerType, runParams, holdTrigger)`. `runParams`
   is a map keyed by `AbilityKey` (`ability/AbilityKey.java`). While the stack is "frozen"
   (a spell/ability is being cast or resolved) or `holdTrigger` is true the event is queued as
   a `TriggerWaiting`; `TapsForMana` and `ManaAdded` are never queued; `Always` (state
   triggers) is evaluated immediately.
3. **Match.** `canRunTrigger`: same mode; not suppressed; `ActivationLimit`/`GameActivationLimit`
   not exhausted; `requirementsCheck` (generic conditions, 1.3); `meetsRequirementsOnTriggeredObjects`
   (only Evolve and Increment); the mode's `performTest(runParams)`; not disabled by a
   `Mode$ DisableTriggers` static (Torpor Orb). During the match the event's triggering
   objects are visible to the condition keys (so `CheckSVar$` may reference `TriggeredPlayer$…`).
4. **Fire.** `runSingleTriggerInternal` builds the effect from `Execute$` (the SVar is parsed
   as an ability; if `Execute` is absent the effect is an empty ability), copies it, attaches
   the triggering objects (`setTriggeringObjects` of the mode, 1.5), decides optional/mandatory,
   wraps it in a `WrappedAbility` and
   - `Static$ True`: resolves it immediately, without the stack (`playTrigger`);
   - otherwise: puts it into the simultaneous-stack-entry list (the stack later orders these
     APNAP and asks the controller to order their own) and fires an `AbilityTriggered` event.
   Extra copies are fired for each applicable `Mode$ Panharmonicon` static (counted when the
   event is collected, capped by the activation limits).
5. **Resolve.** `WrappedAbility.resolve`: unless the mode is `Always` or `NoResolvingCheck$`
   is set, `requirementsCheck` and the Evolve/Increment check are run **again** (this is the
   "intervening if" recheck - see 1.3); `ResolvedLimit` is checked; then the optional decider is
   asked (`confirmTrigger`), and the effect resolves.

Delayed triggers (created by the `DelayedTrigger` effect) live in a separate list, are
matched by the same `canRunTrigger`, fire once and are removed; their controller is the
player who created them (`getSpawningAbility().getActivatingPlayer()`), and zone checks are
skipped for them. They are excluded from Panharmonicon (CR 603.2d comment in the source).

## 1.2 Generic keys (all modes)

| Key | Meaning (source) |
|---|---|
| `Mode$` | Trigger type; looked up case-insensitively in `TriggerType`. Several script modes share one class (see 1.6). |
| `Execute$ <SVar>` | Name of the SVar holding the effect (`DB$ …` / `AB$ …` chain). Resolved against the card state that owns the trigger. For granted triggers (`AddTrigger$`) the SVar is looked up on the granting static/keyword first, then the card. If the SVar is missing on the current state the trigger is skipped with a warning. |
| `TriggerDescription$` | Display text only. `CARDNAME`, `NICKNAME`, `ORIGINALHOST`, `EFFECTSOURCE` are substituted; the token `ABILITY` is replaced by the description of the executed ability (used with `Charm`). A trigger without it shows no text. Never parse rules from it. |
| `TriggerZones$ <zones>` | Comma list of `ZoneType` names; the **host** must be in one of them for the trigger to be active. Absent = works from every zone (but see the automatic defaults for `ChangesZone` in 1.7.1). Zone names: `Battlefield, Hand, Library, Graveyard, Exile, Stack, Command, Sideboard, Ante, Flashback, …` (`zone/ZoneType.java`). |
| `Phase$ <phases>` | Parsed for **every** trigger in the `Trigger` constructor (`PhaseType.parseRange`): the trigger is only active while the current phase/step is in the set. For `Mode$ Phase` this is the step selector (1.7.2). Values: `Untap, Upkeep, Draw, Main1, BeginCombat, Declare Attackers, Declare Blockers, First Strike Damage, Combat Damage, EndCombat, Main2, End of Turn, Cleanup` (also the enum names `UPKEEP`, `COMBAT_BEGIN`, …), `Main` (= Main1+Main2), ranges `A->B` (`A->` = through Cleanup), comma lists. |
| `PhaseCount$` | With `Phase$`: only in the second main-phase-count (`getNumMain()+1 == 2`). Rare. |
| `PlayerTurn$ True` / `NotPlayerTurn$ True` / `OpponentTurn$ True` | Only during the host controller's turn / not during it / during an opponent's turn. Checked in `phasesCheck`, i.e. at registration time, not against the event. |
| `FirstUpkeep$`, `FirstUpkeepThisGame$`, `FirstCombat$`, `TurnCount$ N` | Phase-handler state checks (first upkeep of the turn's player, first upkeep of the game, first combat phase of the turn, exact turn number). |
| `OptionalDecider$ <DefinedPlayer>` | "You may": the named player (`You`, `TriggeredCardController`, `TriggeredPlayer`, …) is asked **on resolution** whether the whole effect happens. If the decider has left the game another chooser is picked. |
| *(implicit optionality)* | If there is no `OptionalDecider`, the trigger is mandatory **unless** the executed ability is a root ability (`AB$`) with a `Cost$` other than `0` that is not flagged mandatory - then it is optional with the controller as decider (paying the cost is the choice). `DB$` effects are always mandatory. |
| `Secondary$ True` | Text-box suppression only: the description is not shown (used when one printed ability needs several script triggers, or for keyword-generated triggers). No rules effect. |
| `Static$ True` | Not a triggered ability in the rules sense: the effect is applied at once, without the stack, before normal triggers of the same event; not affected by `DisableTriggers`; not doubled by Panharmonicon except `TapsForMana`/`ManaAdded`; does not auto-restrict `ChangesZone` leave triggers to the battlefield. Used for mana-doubling "triggered mana abilities", bookkeeping, "as … happens" helpers, and state-watchers such as Ascend/Daybound. |
| `TriggerController$ <DefinedPlayer>` | The player who controls (activates) the triggered ability instead of the host's controller (e.g. `TriggeredCardController` for "its controller …" death triggers granted by effects). |
| `ActivationLimit$ N` | "This ability triggers only N times each turn": compared with the number of times this trigger's ability was put on the stack this turn (`limitByActivations`); also caps Panharmonicon copies. |
| `GameActivationLimit$ N` | Same, per game. |
| `ResolvedLimit$ N` | CR 603.2i style "do this only once each turn": counts **resolutions** of the ability this turn by the same player; checked both when matching and when resolving. |
| `NoResolvingCheck$ True` | Skip the resolution-time recheck of the conditions (the condition is then a plain trigger condition, not an intervening if). |
| `ResolvingCheck$ <KeyName>` | For `Mode$ Always` (which is never rechecked by default): recheck just the named condition key (e.g. `IsPresent2`) on resolution. |
| `OneOff$ True` | When hosted on an effect card in the command zone: remove that effect card after the trigger fires. |
| `BoonAmount$` | Boon (Alchemy) bookkeeping: removes the boon after N triggers. |
| `Chapter$ N` | Marks a Saga chapter trigger (generated by `K:Chapter`). |
| `ValidXxx$`, `InvertValidXxx$` | Mode-specific filters (1.7). For every filter key `P` read through `matchesValidParam`, a companion key `InvertP$ True` negates the test. A filter that is absent always passes. When the event value is a collection, the filter passes if **any** element matches. |

Keys that look generic but are not read by `Trigger`: `Description$` (that is the static/replacement
key), `Optional$` (belongs to effects and replacement effects).

## 1.3 Condition keys and the "intervening if"

`Trigger.requirementsCheck` -> `CardTraitBase.meetsCommonRequirements`. All of these are
evaluated relative to the **host's controller** (or, while the trigger is resolving, the
player who controls the resolving ability) and are checked **twice**: when the event is
matched and again when the ability resolves (unless `Mode$ Always`/`NoResolvingCheck`). So
in Forge *every* condition key behaves as an intervening-"if" (CR 603.4). A condition that
is only a trigger condition ("whenever X, if it was a Y" about the event object) is normally
written into the mode's `ValidXxx` filter instead, which is not rechecked.

| Key | Test |
|---|---|
| `IsPresent$ <valid>` (+ `PresentZone$` default `Battlefield`, `PresentPlayer$` `You`/`Opponent`/`Any` default `Any`, `PresentCompare$` default `GE1`, `PresentDefined$ <Defined>`) | Count cards matching the filter in the zone(s) of the chosen players (or among the `Defined` cards) and compare. `IsPresent$ Card.Self` is the standard "if this is still on the battlefield" idiom; `IsPresent$ Card.Self+counters_GE1_P1P1`-style filters encode "if it has …". |
| `IsPresent2$` (+ `PresentZone2$`, `PresentPlayer2$`, `PresentCompare2$`) | A second, independent presence test. |
| `CheckSVar$ <expr>` (+ `SVarCompare$` default `GE1`), `CheckSecondSVar$` (+ `SecondSVarCompare$`) | Evaluate an SVar/`Count$` expression on the host and compare. May mention triggering objects (`TriggeredCard$…`, `TriggerCount$…`); when checked without an event (e.g. AI look-ahead) such conditions are dropped. |
| `CheckDefinedPlayer$ <Defined>` (+ `DefinedPlayerCompare$` default `GE1`) | Number of players the `Defined` expression yields. |
| `LifeTotal$ You|OpponentSmallest|OpponentGreatest` + `LifeAmount$ <cmp>` | Life total comparison. |
| `Metalcraft$`, `Delirium$`, `Threshold$`, `Hellbent$`, `Bloodthirst$`, `FatefulHour$`, `Monarch$`, `Revolt$`, `Blessing$`, `EnduringStory$` = `True`/`False` | Named ability-word states of the controller must equal the value (`Revolt$ None`: no player has revolt). |
| `DayTime$ Day|Night|Neither` | Day/night state. |
| `Adamant$ <Color|Any>` | At least three mana of that colour were spent on the host's cast. |
| `ManaSpent$ <colors>` / `ManaNotSpent$ <colors>` | Colours of mana spent to cast the host. |
| `ClassLevel$ N` | Host Class is at least level N. |
| `WerewolfTransformCondition$` / `WerewolfUntransformCondition$` | No spells cast last turn / some player cast two or more. |
| `APlayerHasMoreLifeThanEachOther$`, `APlayerHasMostCardsInHand$` | Exactly one player strictly leads (in `Trigger.requirementsCheck`). |
| `Condition$ Evolve` | Special: the entering creature must exceed the host in power or toughness (also applied automatically to the Evolve keyword trigger); rechecked on resolution. |

Not rechecked on resolution: everything in the mode's `performTest` (the `ValidXxx` filters,
`Origin`, `Destination`, `CombatDamage`, …) and the `Phase$`/turn keys of 1.2.

## 1.4 State triggers (`Mode$ Always`)

`GameAction.checkStaticAbilities` fires an `Always` event after every static-ability
recalculation (i.e. effectively on every state-based-action check). `TriggerAlways.performTest`
is unconditionally true, so the trigger is driven only by its condition keys
(`IsPresent`, `CheckSVar`, …). It will not trigger again while an instance from the same
trigger id is already on the stack (`hasStateTrigger`, CR 603.8). It is not rechecked on
resolution unless `ResolvingCheck$` names a key. With `Static$ True` it is Forge's idiom
for "as soon as <state>, do X without the stack" (Ascend, day/night, transform helpers).

`Mode$ Immediate` is internal: the `ImmediateTrigger` effect creates a reflexive trigger
("when you do, …") and fires it right away; `AfterReplacement$` delays it until no replacement
is being applied.

## 1.5 How the event's objects reach the effect (`Triggered…`)

Each mode's `setTriggeringObjects` copies selected `runParams` entries into the effect's
triggering-object map (keyed by `AbilityKey`). Scripts read them by name; the lookup is
`AbilityKey.fromString` (case-insensitive) on whatever follows the prefix
(`ability/AbilityUtils.java`):

- `Defined$ Triggered<Key>` as a **card**: the *current game object* for the stored card
  (`game.getCardState`), i.e. the card wherever it is now. If the stored value is a collection,
  all cards in it.
  - `Triggered<Key>LKICopy`: the stored object itself (usually a last-known-information copy),
    not re-resolved. Used when the effect must act on / read the object as it was
    (`TriggeredCardLKICopy`, `TriggeredAttackerLKICopy`, `TriggeredNewCardLKICopy`).
  - `Triggered<Key>HostCard`: when the stored value is a spell/ability, its source card
    (`TriggeredSourceSAHostCard`, `TriggeredSpellAbilityHostCard`).
- `Defined$ Triggered<Key>` as a **player**: the stored player(s); or with a suffix
  - `…Controller`: controller of the stored card / activator of the stored spell-ability
    (`TriggeredCardController`, `TriggeredSpellAbilityController`, `TriggeredSourceSAController`),
  - `…OrController`: the value itself if it is a player, else its controller
    (form `Triggered<Key>OrController`),
  - `…Owner`, `…Opponent` (opponents of the controller),
  - `…AndYou`: additionally the host's controller.
  The key `OriginalController` is special-cased so its trailing "Controller" is not stripped.
- `Defined$ Triggered<Key>` as a **spell/ability** (`TriggeredSpellAbility`, `TriggeredSourceSA`,
  `TriggeredStackInstance`-style uses): the stored `SpellAbility`.
- In numeric expressions:
  - `TriggerCount$<Key>[/op]`: an integer triggering object (`TriggerCount$DamageAmount`,
    `TriggerCount$LifeAmount`, `TriggerCount$Amount`, `TriggerCount$CurrentStormCount/Minus.1`);
    if the object is a list of integers they are summed (`TriggerCountMax$…` takes the maximum).
  - `Triggered<Key>$<CardProperty>`: a property of the stored card
    (`TriggeredCard$CardPower`, `TriggeredCard$CardCounters.P1P1`). `CardTriggered<Key>$…` forces
    the card reading for keys that could also be players.
  - `TriggeredPlayer…$`, `TriggeredTarget…$`, `TriggeredDefendingPlayer…$`, `TriggeredActivator…$`:
    player properties of the stored player. `TriggeredPlayers<Key>$…` / `TriggeredCardController$…`
    go through the player resolution above.
  - `TriggerObjects<Key>$…`: property over a stored card collection; `TriggerRemembered$…`: over
    the objects a delayed trigger remembered.
  - `Count$TriggeredManaCostGeneric`, `Count$TriggeredManaCostDevotion.<Color>`,
    `Count$TriggeredPayingMana.<c1>.<c2>` (spell-cast triggers).
- In selectors: `Card.TriggeredCard`, `Creature.blockingTriggeredAttacker`,
  `Creature.ControlledBy TriggeredDefendingPlayer`, `Player.TriggeredPlayer`-style properties
  resolve through the same map; in `ChangeType`/`Valid` strings the words `TriggeredCard`,
  `TriggeredObject`, `TriggeredAttacker`, `TriggeredBlocker` are rewritten to compare against that
  object (`AbilityUtils.filterListByType`).
- Delayed triggers: `DelayTriggerRemembered` / `DelayTriggerRememberedLKI` give the objects
  stored with `RememberObjects$` when the delayed trigger was created.

Triggering objects belong to the **root** ability of the chain; sub-abilities see the same map.

### Triggering objects per mode

Names are given as scripts write them (prefix `Triggered`). "LKI" = a snapshot taken before
the event. (From each class's `setTriggeringObjects`.)

| Mode | Objects |
|---|---|
| `ChangesZone` | `Card` (if `Origin$` is exactly `Battlefield`: the LKI copy of the permanent as it last existed; otherwise the moved card in its new zone), `CardLKI` (when `Origin$` is not `Battlefield`), `NewCard` (only when `Origin$` is `Battlefield`: the new object in the destination zone - used by undying/persist as `TriggeredNewCardLKICopy`) |
| `ChangesZoneAll` | `Cards` (the filtered set), `Amount`, `Cause` |
| `Phase`, `TurnBegin` | `Player` (the active player) |
| `Attacks` | `Attacker`, `Defender` (the attacked player/planeswalker/battle), `Defenders` (everything attacked this combat), `DefendingPlayer` |
| `AttackersDeclared`, `AttackersDeclaredOneTarget` | `Attackers` (filtered by `ValidAttackers`), `AttackedTarget` (collection), `AttackingPlayer` |
| `AttackerBlocked` | `Attacker`, `Blockers`, `Defender`, `DefendingPlayer` |
| `AttackerBlockedByCreature` | `Attacker`, `Blocker` |
| `AttackerBlockedOnce` | `Attackers` |
| `AttackerUnblocked` | `Attacker`, `Defender`, `DefendingPlayer` |
| `AttackerUnblockedOnce` | `AttackingPlayer`, `Defenders` |
| `Blocks` | `Blocker`, `Attackers` (the creatures it blocks) |
| `BlockersDeclared` | `Blockers`, `Attackers` |
| `SpellCast`, `SpellCopy`, `SpellCastOrCopy`, `AbilityCast`, `SpellAbilityCast`, `SpellAbilityCopy` | `SpellAbility`, `Card` (its source), `SpellAbilityTargets` (if it has targets), `LifeAmount` (life paid), `CardLKI`, `Activator`, `CurrentStormCount`, `CurrentCastSpells` |
| `DamageDone` | `Source` (LKI copy), `Target` (card or player), `Cause`, `DamageAmount`, `DefendingPlayer` |
| `DamageDoneOnce` | `Target` (LKI copy if a card), `Sources` (matching `ValidSource`), `DamageAmount` (sum from matching sources), `AttackingPlayer` (controller of an arbitrary first source) |
| `DamageDealtOnce` | `Source`, `Targets` (matching `ValidTarget`), `DamageAmount` (sum to matching targets) |
| `DamageDoneOnceByController` | `Target`, `Source` (here: the controlling *player*) |
| `DamageAll` | `DamageAmount` (total), `Sources`, `Targets` |
| `DamagePreventedOnce`, `ExcessDamage` | `Target`, `DamageAmount` |
| `ExcessDamageAll` | `Targets` |
| `Drawn` | `Card`, `Player` |
| `Discarded` | `Card`, `Cause` |
| `DiscardedAll`, `SacrificedOnce` | `Cards`, `Amount`, `Player`, `Cause` |
| `Sacrificed`, `Exiled`, `Taps`, `Untaps`, `Transformed`, `Mutates`, `PhaseIn`, `PhaseOut`, `Specializes`, `Trains`, `Evolved`, `Adapt`, `Connives`, `BecomeRenowned`, `BecomesPlotted` | `Card` |
| `Destroyed` | `Card`, `Causer` |
| `Cycled`, `TurnFaceUp` | `Card`, `Cause` |
| `LifeGained`, `LifeLost`, `PayLife` | `LifeAmount`, `Player` |
| `LifeLostAll` | `Map` (player -> amount), `Player` (the matching players) |
| `CounterAdded` | `Card`, `Player` (whichever received the counter) |
| `CounterAddedOnce` | `Card`, `Player`, `Amount` |
| `CounterAddedAll` | `Objects`, `Amount` |
| `CounterPlayerAddedAll` | `Source` (the player who put them), `Object`, `CounterMap`, `Amount` |
| `CounterTypeAddedAll` | `Object` |
| `CounterRemoved` | `Card` |
| `CounterRemovedOnce` | `Card`, `Amount` |
| `BecomesTarget` | `Source` (source card of the targeting spell/ability), `SourceSA`, `Target` |
| `BecomesTargetOnce` | `SourceSA`, `Targets`, `Source` |
| `TapsForMana` | `Card`, `Produced` (mana string), `Activator` |
| `ManaAdded` | `Card`, `Player`, `Produced` |
| `ManaExpend` | `Amount`, `Player` |
| `LandPlayed` | `Card` |
| `Attached` | `Source` (the Aura/Equipment), `Target` (what it became attached to) |
| `Unattached` | `Object`, `AttachSource` |
| `ChangesController` | `Card`, `OriginalController` |
| `Countered` | `Card`, `Cause`, `SpellAbility` |
| `Exploited` | `Exploited` (the sacrificed creature), `Card` (the exploiter) |
| `Championed` | `Championed`, `Card`; `Devoured`: `Devoured`; `Enlisted`: `Card`, `Enlisted`; `Mentored`: `Card`, `Source` |
| `Explores` | `Explorer`, `Explored` |
| `BecomeMonstrous` | `Card`, `Amount` |
| `BecomesCrewed`, `BecomesSaddled`, `Crewed`, `Saddled`, `Stationed` | `Card`, `Crew` |
| `TokenCreated` | `Player`, `Card`; `TokenCreatedOnce`: `Cards` |
| `Milled` | `Player`, `Card`; `MilledOnce`: `Cards`, `Amount`, `Player`; `MilledAll`: `Cards`, `Amount` |
| `Scry` | `Player`, `ScryNum`, `ScryBottom` |
| `AbilityTriggered` | `SpellAbility` (the triggered ability), `Source` (its host), `Cause` (cards that caused it) |
| `AbilityResolves` | `Source`, `SpellAbility` |
| `LosesGame` | `Player`, `LastStateBattlefield` |
| `RolledDie`, `RolledDieOnce` | `Result`, `Player`, `SourceSA` |
| `Vote` | `OpponentVotedDiff`, `OpponentVotedSame` (player collections) |
| `UntapAll` | `Map`, `Player`, `Cards`, `Amount`; `TapAll`, `PhaseOutAll`, `ManifestDread`: `Cards` |
| `ClassLevelGained` | `ClassLevel` |
| `UnlockDoor`, `FullyUnlock`, `CaseSolved`, `Exerted`, `RingTemptsYou`, `ClaimPrize`, `CrankContraption`, `VisitAttraction`, `StickerPlaced` (+`StickerKind`) | `Card`, `Player` |
| `ConjureAll` | `Cards`, `Amount`, `Player`, `Cause`; `SeekAll`: `Player`, `Cards` |
| `Discover` | `Player`, `Amount` |
| `Fight` | `Fighter`; `FightOnce`: `Fighters` |
| `PayCumulativeUpkeep` | `Card`, `PayingMana`; `PayEcho`: `Card` |
| Player-only modes (`BecomeMonarch`, `CommitCrime`, `Surveil`, `Shuffled`, `SearchedLibrary`, `Proliferate`, `Investigated`, `FlippedCoin`, `FlippedCoinOnce`, `Foretell`, `Forage`, `GiveGift`, `CollectEvidence`, `DungeonCompleted`, `FacesDilemma`, `ChaosEnsues`, `TakesInitiative`, `PlanarDice`, the four `…bend` modes) | `Player` |
| `Always`, `Immediate`, `DayTimeChanges`, `NewGame`, `Clashed` | none |
| `Abandoned`, `SetInMotion` | `Scheme`; `PlaneswalkedFrom`/`PlaneswalkedTo`: `Cards`; `RoomEntered`: `RoomName` |

## 1.6 Mode -> class aliases

`TriggerType.java` maps several modes onto one class, so they share parameters and differ
only in which engine event fires them:

- `SpellCast`, `SpellCopy`, `SpellCastOrCopy`, `AbilityCast`, `SpellAbilityCast`, `SpellAbilityCopy`
  -> `TriggerSpellAbilityCastOrCopy`.
- `AttackersDeclared`, `AttackersDeclaredOneTarget` -> `TriggerAttackersDeclared`.
- `Crewed`, `Saddled`, `Stationed` -> `TriggerCrewedSaddled`.
- `Airbend`, `Earthbend`, `Firebend`, `Waterbend`, `ElementalBend` -> `TriggerElementalbend`.

## 1.7 Trigger modes, by frequency

### 1.7.1 `ChangesZone` (7193 cards) — a card moves between zones
`trigger/TriggerChangesZone.java`; fired from `GameAction.changeZone` for every single card
move (held until the whole action finishes), with `Card` (new object), `CardLKI` (snapshot
before the move), `Cause` (the spell/ability that moved it, may be null), `Origin`,
`Destination` (zone names), plus `Fizzle`, `MergedCards`, cost-payment context.

| Key | Meaning |
|---|---|
| `Origin$` | Comma list of zone names, or `Any`. Absent = any. A move from "nowhere" (token creation) has a null origin and only matches when `Origin` is absent/`Any`. |
| `Destination$` | Same for the destination. |
| `ExcludedOrigins$`, `ExcludedDestinations$` | Zones that must **not** be the origin/destination. |
| `ValidCard$` | Filter on the moved card. **Which object is tested depends on the zones:** `Origin$` exactly `Battlefield`, or `Origin$` exactly `Graveyard` with a destination other than `Battlefield` -> the LKI snapshot (CR 603.10a "looks back in time"); destination `Battlefield` -> the entered permanent as it exists on the battlefield (taken from the this-turn-entered list); otherwise the moved card. |
| `ValidCause$` | Filter on the causing spell/ability (`SpellAbility.YouCtrl`, `Card.Self` …). |
| `CheckOnTriggeredCard$ <SVar> [cmp]` | Evaluate an SVar **on the moved card** and compare with a value computed on the host. |
| `Fizzle$ True/False` | For spells leaving the stack: whether it was a fizzle (only matches when the event carries that flag). |
| `NotThisAbility$` | Does not trigger if the move was caused by this trigger's own ability. |
| `ConditionYouCastThisTurn$ <cmp>` | Host was the n-th spell its controller cast this turn. |

How common wordings are written:

- **"When CARDNAME enters"**: `Destination$ Battlefield | ValidCard$ Card.Self` (no `Origin`).
- **"Whenever another creature you control enters"**: `Destination$ Battlefield | ValidCard$ Creature.YouCtrl+Other | TriggerZones$ Battlefield`.
- **"When CARDNAME dies"**: `Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self`.
- **"Whenever a creature dies"**: same with `ValidCard$ Creature` (+ `TriggerZones$ Battlefield`).
- **"When CARDNAME leaves the battlefield"**: `Origin$ Battlefield | Destination$ Any | ValidCard$ Card.Self`.
- **"When CARDNAME is put into a graveyard from anywhere"**: `Destination$ Graveyard | ValidCard$ Card.Self` (origin absent).
- **"When this is put into your graveyard from your library"** etc.: explicit `Origin$ Library | Destination$ Graveyard`.
- "Dies or is put into exile": `Destination$ Graveyard,Exile`.

Automatic `TriggerZones` when none is given (`correctZones`, runs only if `ValidCard` is present):

- If `Origin$` contains `Battlefield` and the trigger is not `Static`, the trigger zone is set
  to `Battlefield` (the host must have been on the battlefield; together with "looks back in
  time" registration this is what makes "when this dies" and "whenever another creature dies"
  work for simultaneous deaths).
- If `ValidCard$` contains `Self` and `Origin` is absent or `Any`, the trigger zone is set to
  the `Destination$` zone(s) (an ETB trigger of the card itself functions from the battlefield).

Other details: for `Origin$ Battlefield` with `TriggerZones$ Graveyard` (recursion from the
graveyard watching other deaths) the host must already have been in the graveyard before the
event. For merged (mutated) permanents a leaves-battlefield trigger fires once for the pile,
other triggers once per component. `ChangesZone` events with destination `Battlefield` are
collected only after everything that enters simultaneously has entered (CR 603.6a).

Examples:
`T:Mode$ ChangesZone | Destination$ Battlefield | ValidCard$ Artifact.YouCtrl | TriggerZones$ Battlefield | Execute$ TrigLoot | TriggerDescription$ Whenever an artifact you control enters, …`
`T:Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self | Execute$ TrigDraw | TriggerDescription$ When CARDNAME dies, draw a card.`

### 1.7.2 `Phase` (2651) — beginning of a step/phase
`trigger/TriggerPhase.java`; fired by `PhaseHandler` at the start of every non-skipped step,
once per active player, with `Player` = that player.

- `Phase$` (generic key, 1.2) selects the step: `Upkeep`, `Draw`, `Main1`, `Main2`,
  `BeginCombat`, `Declare Attackers`, `Declare Blockers`, `EndCombat`, `End of Turn`, `Cleanup`, …
- `ValidPlayer$` - whose step: `You` ("your upkeep"), `Opponent`/`Player.Opponent`,
  `Player` (each player's), `Player.EnchantedBy`/`Player.EnchantedController` etc. Absent = every turn.
- Plus the generic turn keys (`PlayerTurn`, `FirstUpkeep`, `FirstCombat`, `TurnCount`, …).

"At the beginning of your end step" = `Phase$ End of Turn | ValidPlayer$ You`;
"at the beginning of combat on your turn" = `Phase$ BeginCombat | ValidPlayer$ You`;
"at the beginning of each upkeep" = `Phase$ Upkeep`. Triggering object: `TriggeredPlayer`.

Example: `T:Mode$ Phase | Phase$ BeginCombat | ValidPlayer$ You | Execute$ TrigCharm | TriggerZones$ Battlefield | TriggerDescription$ At the beginning of combat on your turn, ABILITY`

### 1.7.3 `Attacks` (1632) — a creature is declared as an attacker
`trigger/TriggerAttacks.java`; fired once per attacking creature by
`CombatUtil.checkDeclaredAttacker` after attackers are declared (also for creatures put onto
the battlefield attacking **UNVERIFIED** - only the declare path was read).

| Key | Meaning |
|---|---|
| `ValidCard$` | The attacker (`Card.Self`, `Creature.YouCtrl`, `Card.EquippedBy`, `Card.AttachedBy` …). |
| `Attacked$` | What it attacks (`Player`, `Player.Opponent`, `Planeswalker`, `Battle`, `Player.withMostLife`, `You` …). |
| `Alone$ True/False` | Exactly no other attackers / at least one other attacker. |
| `FirstAttack$` | The creature's first attack this turn. |
| `DefendingPlayerPoisoned$` | Defending player has a poison counter. |
| `AttackDifferentPlayers$` | The attacked entity is a player and another player is also being attacked this combat. |

Objects: `TriggeredAttacker`, `TriggeredDefender` (attacked entity), `TriggeredDefendingPlayer`, `TriggeredDefenders`.
Example: `T:Mode$ Attacks | ValidCard$ Card.EquippedBy | Alone$ True | Execute$ TrigLoseLife | TriggerDescription$ Whenever equipped creature attacks alone, …`

### 1.7.4 `SpellCast` (1499) and the cast/copy/activate family
`trigger/TriggerSpellAbilityCastOrCopy.java`; fired from `zone/MagicStack.java` when an
object is put on the stack:

- `SpellAbilityCast`: any non-copied spell or ability (incl. mana abilities); `SpellCast`: non-copied spells;
  `AbilityCast` (47): non-copied **activated** abilities ("whenever you activate an ability");
- `SpellCopy` / `SpellAbilityCopy`: copies; `SpellCastOrCopy` (30): any spell, cast or copied.

| Key | Meaning |
|---|---|
| `ValidCard$` | The spell's card (`Instant,Sorcery`, `Creature`, `Card.Self` for cast triggers, `Card.nonCreature`…). |
| `ValidActivatingPlayer$` | Who cast/activated it (`You`, `Opponent`, `Player`). |
| `ValidSA$` | Filter on the spell-ability itself (`Spell.Kicked`, `Activated.Exhaust`, `Spell.MayPlaySource`, …). |
| `ValidSAonCard$` | Same filter, but evaluated relative to the cast card instead of the trigger host. |
| `TargetsValid$` | At least one target of the spell (or its sub-abilities) matches (heroic: `TargetsValid$ Card.Self`). |
| `IsSingleTarget$` | The spell has exactly one target in total. |
| `CanTargetOtherCondition$ <valid>` | Some *other* legal target matching the filter exists. |
| `HasXManaCost$` | The spell/ability has {X} in its cost. |
| `NoColoredMana$` | Only colourless mana was spent. |
| `SnowSpentForCardsColor$` | Snow mana of one of the spell's colours was spent. |
| `ActivatorThisTurnCast$ <cmp>` | (needs `ValidActivatingPlayer`) number of spells matching `ValidCard` that player has cast this turn, including this one - "second spell each turn" = `EQ2`. |
| `ActivatorThisTurnCastEach$ <cmp>` | Same, but per alternative in `ValidCard`. |

"When you cast this spell": `ValidCard$ Card.Self` with `TriggerZones$ Stack` (keyword-made
ones) or no trigger zone. Objects: `TriggeredSpellAbility`, `TriggeredCard`, `TriggeredActivator`,
`TriggeredSpellAbilityTargets`, `TriggerCount$CurrentStormCount`.
Example: `T:Mode$ SpellCast | ValidActivatingPlayer$ You | TargetsValid$ Card.Self | TriggerZones$ Battlefield | Execute$ DBGainLife | TriggerDescription$ Heroic — Whenever you cast a spell that targets CARDNAME, you gain 2 life.`

### 1.7.5 `DamageDone` (940) — one source deals damage to one object/player
`trigger/TriggerDamageDone.java`; fired per (source, target) damage event from
`Card.addDamage…`/`Player.addDamage…` after replacement/prevention, with the final amount.

| Key | Meaning |
|---|---|
| `ValidSource$` | The damage source. |
| `ValidTarget$` | The damaged permanent or player (`Player`, `Opponent`, `You`, `Creature`, `Card.Self`, `Planeswalker`…). |
| `ValidCause$` | The spell/ability that caused the damage. |
| `CombatDamage$ True/False` | Combat damage only / noncombat only. Absent = both. |
| `TargetRelativeToSource$ <valid>` | Filter on the target evaluated with the *source* as reference card (e.g. the source's controller's opponent). |
| `TargetRelativeToCause$ <valid>` | Same relative to the causing ability (must exist). |
| `DamageAmount$ <cmp>` | Amount comparison; the operand may be the literal `TargetToughness`. |

Objects: `TriggeredSource` (LKI), `TriggeredTarget`, `TriggerCount$DamageAmount`, `TriggeredCause`, `TriggeredDefendingPlayer`.
"Whenever CARDNAME deals combat damage to a player" = `ValidSource$ Card.Self | ValidTarget$ Player | CombatDamage$ True`.
"Whenever CARDNAME is dealt damage" = `ValidTarget$ Card.Self`.

### 1.7.6 `AttackersDeclared` (242) / `AttackersDeclaredOneTarget` (34) — "whenever you attack"
`trigger/TriggerAttackersDeclared.java`; fired once per declare-attackers step that has
attackers (`AttackersDeclared`), and once per attacked defender (`…OneTarget`, where
`Attackers` holds only that defender's attackers).

- `AttackingPlayer$` - filter on the attacking player (`You`, `Player.Opponent`).
- `AttackedTarget$` - filter on the attacked entities ("whenever enchanted player is attacked",
  `Player`, `You`); the object `AttackedTarget` is filtered to the matching ones.
- `ValidAttackers$` + `ValidAttackersAmount$ <cmp>` (default `GE1`) - count attackers that match
  ("whenever you attack with three or more creatures" = `ValidAttackers$ Creature.YouCtrl | ValidAttackersAmount$ GE3`);
  the object `Attackers` is filtered to the matching ones.

With several attacking players (shared team turns) an event is produced per attacking player
for triggers that mention the attacking player and once for the whole attack otherwise.

### 1.7.7 `DamageDoneOnce` (218) — "one or more … deal damage to X" (grouped by target)
`trigger/TriggerDamageDoneOnce.java`; fired from `CardDamageTable.triggerDamageDoneOnce`
once per damaged entity per damage batch (one combat damage step, or one resolving effect).

- `ValidTarget$` - the damaged entity; `ValidSource$` - filter on sources (at least one
  matching source must have dealt > 0); `CombatDamage$`; `DamageAmount$ <cmp>` against the
  summed damage from matching sources; `FirstTime$` - first time that entity is dealt (that
  kind of) damage this turn.

Also the standard way to write "whenever CARDNAME is dealt damage" when the total matters
(`ValidTarget$ Card.Self`, amount via `TriggerCount$DamageAmount`).

### 1.7.8 `Drawn` (162) — a player draws a card
`trigger/TriggerDrawn.java`; fired per card drawn; never during mulligans.

- `ValidCard$` - the drawn card; in practice used as the player filter too (`Card.YouCtrl`,
  `Card.OppOwn`); `ValidPlayer$` - the drawing player.
- `Number$ N` - it is the N-th card that player has drawn this turn ("second card each turn" = `2`).
- `FirstCardInDrawStep$ True/False` - is / is not the first card drawn in that player's own draw step.
- `ForReveal$` - (miracle-style) only when the card may be revealed as drawn.

### 1.7.9 `ChangesZoneAll` (131) — "one or more cards move" (batched)
`trigger/TriggerChangesZoneAll.java`; fired once per zone-change batch
(`CardZoneTable.triggerChangesZoneAll`).

- `ValidCards$` (plural!), `Origin$`, `Destination$` (lists or `Any`) select the cards counted;
  the trigger fires if at least one matches.
- `ValidCause$`; `ValidAmount$ <cmp>` on the number of matching cards; `FirstTime$` (no matching
  card entered that destination earlier this turn).
- Objects: `TriggeredCards`, `TriggerCount$Amount`.

### 1.7.10 `Blocks` (131) — a creature blocks
`trigger/TriggerBlocks.java`; once per blocking creature (first block of the combat).
`ValidCard$` = the blocker; `ValidBlocked$` = filter passes if **any** of the creatures it blocks matches.
Objects: `TriggeredBlocker`, `TriggeredAttackers`.
"Whenever CARDNAME attacks or blocks" is two triggers (`Attacks` + `Blocks`, the second `Secondary$ True`).

### 1.7.11 `AttackerBlocked` (126) — an attacker becomes blocked
`trigger/TriggerAttackerBlocked.java`; once per blocked attacker.
`ValidCard$` = the attacker; `ValidBlocker$` + `ValidBlockerAmount$ <cmp>` (default `GE1`) = number of its blockers matching.
Note the idiom `AttackerBlocked | ValidBlocker$ Card.Self` for "whenever CARDNAME blocks a creature"
(the object of interest is then `TriggeredAttacker`).
Objects: `TriggeredAttacker`, `TriggeredBlockers`, `TriggeredDefender`, `TriggeredDefendingPlayer`.

### 1.7.12 `TurnFaceUp` (126)
`ValidCard$` = the permanent turned face up; `ValidCause$` = the ability that did it. Objects `Card`, `Cause`.

### 1.7.13 `Taps` (124) — a permanent becomes tapped
`trigger/TriggerTaps.java`. `ValidCard$`; `ValidCause$` (ability that tapped it); `ValidPlayer$`
(the player who tapped it); `Attacker$ True/False` (tapped by attacking / not);
`FirstTime$` (first time this turn); `Teamwork$` (tapped to pay a teamwork cost).

### 1.7.14 `BecomesTarget` (123) / `BecomesTargetOnce` (4)
`trigger/TriggerBecomesTarget.java`; fired when a spell/ability is put on the stack (or its
targets changed), once per distinct target. `ValidSource$` filters the **spell/ability**
(`SpellAbility.OppCtrl`, `Spell`, `Activated`…, not the card); `ValidTarget$` the target;
`FirstTime$` (first time the permanent became a target this turn); `Valiant$` (first time
this turn by a spell/ability its controller controls).
Objects: `TriggeredSourceSA` (the spell/ability - what Ward counters), `TriggeredSource`
(its card), `TriggeredTarget`. `…Once`: `ValidTarget` passes if any target matches;
extra `ValidCause$` (source card) and `Random$`.

### 1.7.15 `Sacrificed` (114)
`ValidCard$` (the sacrificed permanent, LKI), `ValidPlayer$` (who sacrificed), `ValidCause$`,
`WhileKeyword$ <valid SA>` (sacrificed while paying a cost of a matching ability). `SacrificedOnce` (2)
is the batched form (`ValidCard` any-match; objects `Cards`, `Amount`, `Player`).

### 1.7.16 `LifeGained` (106)
`ValidPlayer$`; `ValidSource$` (source card of the gain); `FirstTime$` (first gain this turn);
`Spell$` (caused by a spell). Objects `TriggeredPlayer`, `TriggerCount$LifeAmount`. One event per
life-gain event (not per point).

### 1.7.17 `Discarded` (78) / `DiscardedAll` (21)
`Discarded`: per card - `ValidCard$` (often carries the player: `Card.YouCtrl`), `ValidPlayer$`, `ValidCause$` (`SpellAbility.OppCtrl` for "an opponent causes you to discard").
`DiscardedAll`: per batch - `ValidPlayer$`, `ValidCause$`, `ValidCard$` (any-match; `Cards` is filtered), `Random$`, `FirstTime$`.

### 1.7.18 `Cycled` (77)
`ValidCard$`, `ValidPlayer$`, `FirstTime$`. Fired when a cycling ability is activated (cost paid).

### 1.7.19 `AttackerBlockedByCreature` (69) — per (attacker, blocker) pair
`ValidCard$` = attacker, `ValidBlocker$` = blocker; special literal values
`ValidCard$ LessPowerThanBlocker` / `ValidBlocker$ LessPowerThanAttacker`. Objects `Attacker`, `Blocker`.
Used for "whenever CARDNAME blocks or becomes blocked by a creature, <that creature> …" (two triggers).

### 1.7.20 `TapsForMana` (65)
`trigger/TriggerTapsForMana.java`; fired from `AbilityManaPart` when a mana ability with a
tap cost produces mana, never queued. `ValidCard$` (the tapped permanent), `Activator$`
(player filter), `Produced$ <Color|ChosenColor>` (the produced mana string must contain that
colour). Almost always `Static$ True` with a `DB$ Mana` effect (mana doubling is a mana
ability); `Trigger.isManaAbility` reports true when the executed ability is one.
Objects: `TriggeredCard`, `TriggeredActivator`, `TriggeredProduced`.

### 1.7.21 `Always` (64) — state trigger; see 1.4.

### 1.7.22 `DamageDealtOnce` (62) — grouped by **source**
One event per source per damage batch. `ValidSource$`, `ValidTarget$` (at least one matching
target took damage; `Targets`/`DamageAmount` are restricted to matching targets),
`CombatDamage$`, `AtLeastOneInstance$ <cmp>` (some single target received an amount satisfying it).
"Whenever CARDNAME deals combat damage" (once, however many targets) uses this mode.

### 1.7.23 `CounterAddedOnce` (47) vs `CounterAdded` (20)
Both from `Card.addCounter…`/`Player.addCounter…`. `CounterAdded` fires **once per single
counter** (with `CounterAmount` = the running total on the object) - it is what Saga chapters
use (`CounterType$ LORE | CounterAmount$ EQ<n>`). `CounterAddedOnce` fires once per
put-counters event with `Amount`.

- Shared: `ValidCard$`, `ValidPlayer$` (the *recipient* when it is a player), `ValidSource$`
  (the player who put the counters), `CounterType$ <TYPE>` (enum name: `P1P1`, `M1M1`, `LORE`,
  `LOYALTY`, `TIME`, …; exact string match).
- `CounterAdded` only: `CounterAmount$ <cmp>` against the new total.
- `CounterAddedOnce` only: `ValidEntity$` (card or player), `FirstTime$` (first time this turn).
- Batched variants: `CounterAddedAll` (3; `CounterType`, `Valid`, `ValidSource`),
  `CounterPlayerAddedAll` (8; `ValidSource`, `ValidObject`, `ValidObjectToSource`),
  `CounterTypeAddedAll` (3; `ValidObject`, `FirstTime`).

### 1.7.24 `AbilityCast` (47) — see 1.7.4.

### 1.7.25 `LandPlayed` (44)
`ValidCard$` (the land; `Land.YouCtrl`), `Origin$` (zone it was played from), `ValidSA$`,
`NotFirstLand$`. Only *playing* a land, not putting one onto the battlefield (landfall is a
`ChangesZone` trigger with `ValidCard$ Land.YouCtrl`).

### 1.7.26 `AttackerUnblocked` (41)
Per unblocked attacker after blockers are declared. `ValidCard$` (attacker), `ValidDefender$`.
`AttackerUnblockedOnce` (1): `ValidDefenders$`, `ValidAttackingPlayer$`.

### 1.7.27 `Untaps` (34)
`ValidCard$` only (inspired). Fired on every untap including the untap step.

### 1.7.28 `Mutates` (32), `Transformed` (28), `UnlockDoor` (26), `Exploited` (23), `BecomeMonstrous` (19), `FullyUnlock` (17), `Specializes` (11)
All take `ValidCard$` (the permanent the event happened to). Extras: `UnlockDoor`:
`ValidPlayer$`, `ThisDoor$` (the door of the face the trigger is on); `Exploited`: `ValidCard$`
is the **sacrificed** creature and `ValidSource$` the exploiting creature; `FullyUnlock`: `ValidPlayer$`.

### 1.7.29 `Scry` (24), `Surveil` (16)
`ValidPlayer$`; `Scry`: `ToBottom$` (at least one card went to the bottom); `Surveil`: `FirstTime$`.

### 1.7.30 `LifeLost` (21)
`ValidPlayer$`; `ValidCause$` (spell/ability that caused the loss - damage is not a cause);
`FirstTime$`; `LifeAmount$ <cmp>`. `LifeLostAll` (2): `ValidPlayer$`, `ValidAmountEach$ <cmp>`.

### 1.7.31 `CommitCrime` (20), `BecomeMonarch` (5), `Proliferate` (6), `Investigated` (2) and other player events
`ValidPlayer$` only (`BecomeMonarch` also `BeginTurn$`; `Investigated` `FirstTime$`).

### 1.7.32 `CounterRemoved` (13) / `CounterRemovedOnce` (2)
`CounterRemoved` fires per single counter: `ValidCard$`, `ValidPlayer$` (the remover),
`CounterType$`, `NewCounterAmount$ N` (exact number left - `0` = "last counter removed";
used by Vanishing/Suspend). `…Once`: `ValidCard$`, `CounterType$`, `Remaining$ N`.

### 1.7.33 `ChangesController` (12)
`ValidCard$`, `ValidOriginalController$`. Fired when a permanent's controller changes (also
from the zone-change code when a permanent leaves the battlefield to another player's zone).

### 1.7.34 `ManaExpend` (12)
`Player$` (filter), `Amount$ N` - fires when the player's total mana spent this turn reaches exactly N ("expend 4").

### 1.7.35 `ClassLevelGained` (11)
`ValidCard$`, `ClassLevel$ N` (exact level reached).

### 1.7.36 `SpellAbilityCast` (11), `SpellCopy` (6), `SpellAbilityCopy` (1) — see 1.7.4.

### 1.7.37 `DamageAll` (10)
One event per damage batch for the whole table: `ValidSource$`, `ValidTarget$`, `CombatDamage$`;
fires if any (source,target) pair matches; objects `DamageAmount` (total over matching pairs),
`Sources`, `Targets`. "Whenever one or more creatures you control deal combat damage to one or more players."

### 1.7.38 `DayTimeChanges` (10) — no parameters (combine with `DayTime$` condition).

### 1.7.39 `Attached` (10) / `Unattached` (4)
`Attached`: `ValidSource$` (the attachment), `ValidTarget$` (what it attaches to),
`TargetRelativeToSource$`. `Unattached`: `ValidObject$`, `ValidAttachment$`.

### 1.7.40 Remaining modes (1–3 lines each)

| Mode (cards) | Event and keys |
|---|---|
| `RingTemptsYou` (9) | `ValidPlayer$`, `ValidCard$` (the chosen Ring-bearer). |
| `LosesGame` (8) | `ValidPlayer$`. |
| `FacesDilemma` (8) | `ValidPlayer$`, `Random$`. |
| `RolledDieOnce` (8) / `RolledDie` (3) | `ValidPlayer$`, `ValidSA$`, `RolledToVisitAttractions$`; `RolledDie` also `ValidResult$` (numbers, `Highest`, comparators), `ValidSides$`, `Number$` (n-th roll), `Natural$`. |
| `Crewed` (7), `Saddled` (2), `Stationed` (1) | Per crewing creature: `ValidCard$` (vehicle/mount), `ValidCrew$`. |
| `BecomesCrewed` (5) | `ValidCard$`, `ValidCrew$`, `FirstTimeCrewed$`, `ValidCrewAmount$`. `BecomesSaddled` (1): `ValidSaddled$`, `FirstTimeSaddled$`. |
| `TurnBegin` (7) | Start of a turn (before untap); `ValidPlayer$`. Script helper, not a real rules trigger. |
| `Exerted` (5), `Explores` (5; `ValidExplored$`), `Connives` (3), `Evolved` (2), `Trains` (1), `Adapt` (1), `BecomeRenowned` (2), `BecomesPlotted` (2), `PhaseIn` (4), `PhaseOut` (2) | `ValidCard$`. |
| `Shuffled` (5) | `ValidPlayer$`, `ShuffleFromEffect$`, `ShuffleBySelfControlled$`. |
| `SearchedLibrary` (4) | `ValidPlayer$`, `SearchOwnLibrary$`. |
| `Clashed` (4) | `ValidPlayer$`, `Won$ True/False`. |
| `PayCumulativeUpkeep` (4), `PayEcho` (1) | `ValidCard$`, `Paid$ True/False`. |
| `DungeonCompleted` (4), `Discover` (2), `CollectEvidence` (2), `Forage`, `GiveGift`, `ManifestDread`, `ElementalBend`, `Foretell` (`OnlyFirst$`), `FlippedCoin` (`ValidResult$ Win/Lose`), `FlippedCoinOnce`, `PayLife`, `CaseSolved` (+`ValidCard$`) | `ValidPlayer$`. |
| `Countered` (4) | `ValidCard$` (the countered spell's card), `ValidCause$` (card that countered), `ValidSA$`. |
| `BecomesTargetOnce` (4) | See 1.7.14. |
| `AbilityTriggered` (4) | A triggered ability was put on the stack: `ValidMode$` (list of trigger modes), `ValidDestination$`, `ValidSpellAbility$`, `ValidSource$`, `ValidCause$` (cards that caused it), `TriggeredOwnAbility$`. |
| `AbilityResolves` (3) | `ValidSpellAbility$`, `ValidSource$`; fired when a `Static$` trigger's ability resolved (and by other internal paths, UNVERIFIED which). |
| `TokenCreated` (3) / `TokenCreatedOnce` (2) | `ValidPlayer$`, `ValidToken$`, `OnlyFirst$`. |
| `MilledAll` (3), `Milled` (2), `MilledOnce` (2) | `ValidCard$`, `ValidPlayer$`. |
| `Destroyed` (3) | `ValidCard$`, `ValidCauser$` (player/ability that destroyed it). |
| `Exiled` (3) | `ValidCard$`, `Origin$`, `ValidCause$`, `WhileKeyword$`; also the carrier of Madness's cast trigger. |
| `Enlisted` (3) (`ValidCard$`, `ValidEnlisted$`), `Championed` (1) (`ValidCard$` = exiled card, `ValidSource$` = champion), `Mentored` (1) (`ValidCard$`, `ValidSource$`) | |
| `ExcessDamage` (3) / `ExcessDamageAll` (1) | `ValidTarget$`, `CombatDamage$`; amount = damage beyond lethal. |
| `ConjureAll` (3), `SeekAll` (3) | Alchemy; `ValidPlayer$`, `ValidCard$`, `ValidCause$`. |
| `AttackerBlockedOnce` (3) | "one or more creatures you control become blocked": `ValidCard$` any-match over blocked attackers. |
| `Vote` (3) | No test keys (`List$` only appears in the class's param scan); objects give opponents who voted differently/the same. |
| `TapAll` (2), `UntapAll` (1), `PhaseOutAll` (1) | Batched: `ValidCards$` (`UntapAll` also `ValidPlayer$`). |
| `ManaAdded` (1) | Mana added by any ability: `ValidSource$`, `ValidSA$`, `Player$`, `Produced$`. |
| `Fight` (1) / `FightOnce` (1) | `ValidCard$`. |
| `DamagePreventedOnce` (1) | `ValidTarget$`, `CombatDamage$`, `DamageAmount$`. |
| `DamageDoneOnceByController` (0 in `T:` lines) | Per (damaged entity, controller of sources): `ValidTarget$`, `ValidSource$` (a player filter), `CombatDamage$`. |
| `BlockersDeclared` (1), `NewGame` (1) | No test keys. |
| `CantAttackUnless` (1 in the table) | Not a trigger mode - it is a static mode that appears in an SVar; ignore. |
| Planechase/Archenemy/Un-/Alchemy modes (`PlanarDice`, `PlaneswalkedTo/From`, `ChaosEnsues`, `SetInMotion`, `Abandoned`, `RoomEntered`, `VisitAttraction`, `ClaimPrize`, `CrankContraption`, `StickerPlaced`, `TakesInitiative`, `Devoured`) | `ValidPlayer$`/`ValidCard$` as listed in 1.5; not needed for Commander precons except `TakesInitiative`/`RoomEntered` (dungeons). |


---

# Part 2 — Static abilities

Sources: `staticability/StaticAbility.java`, `StaticAbilityMode.java`, `StaticAbilityLayer.java`,
`StaticAbilityContinuous.java`, one `StaticAbility<Mode>.java` per rules-modifying mode,
`cost/CostAdjustment.java` (ReduceCost/RaiseCost/SetCost), `GameAction.checkStaticAbilities`
(the layer loop), `GameActionUtil.java` (OptionalCost). Forge's notes:
`docs/Card-scripting-API/Statics.md`.

Shape: `S:Mode$ <Mode[,Mode…]> | <mode keys> | <generic keys> | Description$ <text>`

Statics also appear inside SVars (no `S:` prefix) when they are granted by another ability:
`AddStaticAbility$ <SVar>` on a Continuous static, `StaticAbilities$ <SVar>` on an `Effect`,
keyword expansions. The frequency table's "static" section counted every `Mode$` it found,
so it also lists values that are **not** static modes: `TgtChoose`, `RevealYouChoose`, `Hand`,
`Random`, `Defined`, `LookYouChoose`, `RevealDiscardAll`, `RevealTgtChoose` are `Discard`
effect modes; `Transform`, `TurnFaceUp`, `TurnFaceDown`, `Flip`, `Unlock`, `LockOrUnlock`
are `SetState`/door modes; `Gain`, `Toughness`, `Power` belong to other effects. The real
list is the `StaticAbilityMode` enum.

## 2.1 Generic keys (all modes)

| Key | Meaning |
|---|---|
| `Mode$` | One or more `StaticAbilityMode` names separated by `,` or space (`setValueOf`). A static with several modes (e.g. `CantAttack,CantBlock`, `CantAttack,CantBlock,CantBeActivated` for Pacifism/Arrest effects) answers each of those queries with the same parameter set. |
| `EffectZone$ <zones>` | Zones in which the **host** must be for the ability to function. Default: battlefield only (`zonesCheck`: `isInPlay`). `All` = every zone. Typical: `Command` (emblems/effects), `Graveyard`, `Stack`, `Hand`, `All` (cost modifiers of the card itself, CDAs). Ignored for characteristic-defining abilities (they always pass the zone check). A phased-out or suppressed host never applies. |
| `Description$` | Display text only (`CARDNAME`, `NICKNAME` substituted). |
| `Secondary$ True` | Hide the description (rules-irrelevant). |
| `Condition$ <word>` | One of the fixed words (checked against the host's controller): `Threshold`, `Hellbent`, `Metalcraft`, `Delirium`, `Ferocious`, `Blessing`, `EnduringStory`, `Monarch`, `Night`, `MaxSpeed`, `PlayerTurn` (it is the controller's turn), `NotPlayerTurn`, `ExtraTurn`, `FatefulHour`. |
| `Phases$ <range>` | Only during these phases/steps (same syntax as trigger `Phase$`). |
| `PlayerTurn$ <DefinedPlayer>` | Only during a turn of one of the defined players. |
| `TopCardOfLibraryIs$ <valid>` | Top card of the controller's library matches. |
| `IsPresent$ <valid>` (+ `PresentZone$` default `Battlefield`, `PresentCompare$` default `GE1`) | Count matching cards in the zone(s) **of all players** and compare. (No `PresentPlayer` here, unlike triggers - restrict with `.YouCtrl` etc. in the filter.) |
| `CheckSVar$` (+ `SVarCompare$`), `CheckSecondSVar$`, `CheckThirdSVar$`, `CheckFourthSVar$` (+ `Second…/Third…/FourthSVarCompare$`) | Numeric SVar tests; default comparator `GE1`. Note the code only looks at the 2nd/3rd/4th when the previous one exists. |
| `GameStage$ <stages>` | Only in the listed `GameStage`s (e.g. `Mulligan`). If present it short-circuits the later checks. |
| `ClassLevel$ N` | Host Class level is at least N (used by the `K:Class` expansion). |
| `Timestamp$` | Overrides the layer timestamp (internal). |
| `Invert<Filter>$ True` | As for triggers: negates the corresponding `matchesValidParam` filter. |

All conditions are re-evaluated whenever the ability is consulted; there is no memory.

Where statics are looked for: Continuous statics are collected from every card in the game
whose `zonesCheck` passes. The other modes are queried by scanning cards in
`ZoneType.STATIC_ABILITIES_SOURCE_ZONES` = Battlefield, Graveyard, Exile, Command, Stack
(**not** Hand or Library), usually plus the card being asked about itself (so a card in hand
can restrict its own casting). Each scanned static must still pass `checkConditions`.

## 2.2 `Mode$ Continuous` (4906 cards)

### 2.2.1 Who is affected

| Key | Meaning |
|---|---|
| `Affected$ <valid>` | Filter for affected **cards and players**. Players: every player matching the filter is affected (for `AddKeyword` on players, hand size, land plays, votes…). Cards: applied as a filter to the candidate set below. If `Affected` is present and matches nothing, the static does nothing. |
| `AffectedZone$ <zones>` | Zone(s) the candidate cards are taken from. Default `Battlefield`. `All`, `Stack` ("spells you control have …"), `Graveyard`, `Exile`, `Hand`, `Library`, `Command`. |
| `AffectedDefined$ <Defined>` | Start from a `Defined` set instead of a zone (`Self`, `Enchanted`, `Equipped`, `AttachedBy`, `EffectSource`, `Remembered`…), phased-in cards only; `Affected$` then filters it further (`AffectedDefined$ Enchanted | Affected$ Creature` = "as long as enchanted permanent is a creature"). Not zone-restricted. |
| `CharacteristicDefining$ True` | CDA: affects only the host, in every zone (optional `ExcludeZone$ <zones>`), ignores `EffectZone`/`Affected`; applied before non-CDA effects within its layer; `SetPower/SetToughness` go to layer 7a. |

Both spellings occur for the common cases: `Affected$ Card.Self` / `AffectedDefined$ Self`;
`Affected$ Creature.EnchantedBy` / `AffectedDefined$ Enchanted`; `Affected$ Creature.EquippedBy`
/ `AffectedDefined$ Equipped`.

### 2.2.2 Layer assignment and application order

`StaticAbility.generateLayer` derives the set of layers a static participates in **purely
from which keys are present**; `GameAction.checkStaticAbilities` then walks the layers in
order `COPY(1), CONTROL(2), TEXT(3), TYPE(4), COLOR(5), ABILITIES(6), CHARACTERISTIC(7a),
SETPT(7b), MODIFYPT(7c), RULES("8")` and applies, per layer, only the keys of that layer.

- Within a layer: CDAs first, then by timestamp (`getTimestamp` = host's layer timestamp);
  for non-CDA effects a dependency search (`findStaticAbilityToApply`) may reorder (CR 613.8).
- The set of affected objects is computed the **first** time the static applies in any layer
  and reused in later layers (CR 613.6), even if the static's source has lost the ability in
  between. A static only *starts* applying if its host still has it at that point.
- If an affected card gains a new Continuous static during the walk (via `AddStaticAbility`
  etc.), that static is added and applied from the current layer on.
- No script-visible "Layer$" key exists for statics. There is no layer-1 static (copy effects
  are resolving effects in Forge) and no 7d (P/T switching is an effect/keyword).
- +1/+1 and -1/-1 counters are not statics; they are folded in by `Card`'s P/T computation
  (where exactly relative to 7c was not read - UNVERIFIED).
- If none of the layer keys is present the static lands in RULES.

Gotcha (from the code): keys handled only in the RULES pass (`SetMaxHandSize`,
`RaiseMaxHandSize`, `MayLookAt`, `ControlOpponentsSearchingLibrary`, `AdditionalVillainousChoice`,
`LethalDamageByPower`) are not in the layer-detection list, so they only take effect when the
static has no other-layer key (then it defaults to RULES) or also carries one of the keys that
do select RULES (`MayPlay`, `AddHiddenKeyword`, `Goad`, `AdjustLandPlays`, …).

### 2.2.3 Keys by layer

**Layer 2 - control** (`CONTROL`)

| Key | Effect |
|---|---|
| `GainControl$ <DefinedPlayer>` | The defined player (first one) controls each affected card (`You`, `Player.isMonarch`, …) with the static's timestamp. Control-changing Auras: `AffectedDefined$ Enchanted | GainControl$ You`. |

**Layer 3 - text** (`TEXT`)

| Key | Effect |
|---|---|
| `GainTextOf$ <Defined>` (+ `GainTextAbilities$ <SVars>`) | Affected card takes name, mana cost, colour, types, abilities, keywords and base P/T of the first defined card (Volrath's Shapeshifter). |
| `SetName$ <name|ChosenName>` | Changes the name. `AddNames$` adds all nonlegendary creature names (one card). |
| `ManaCost$ <cost>` | Replaces the mana cost. `Incorporate$ <cost>` adds to the mana cost (and adds the cost's colours). |
| `ChangeColorWordsTo$ <color|ChosenColor>` (+ `ChangeColorWordsFrom$`, default `Any`) | Text-changing of colour words. |

**Layer 4 - type** (`TYPE`)

| Key | Effect |
|---|---|
| `AddType$ A & B` | Adds card types / subtypes / supertypes (` & `-separated). Placeholders: `ChosenType`, `ChosenType2`, `ImprintedCreatureType`, `AllBasicLandType`, `AllNonBasicLandType`. |
| `RemoveType$ A & B` | Removes the listed types (`ChosenType` allowed). |
| `RemoveCardTypes$`, `RemoveSubTypes$`, `RemoveSuperTypes$`, `RemoveLandTypes$`, `RemoveCreatureTypes$`, `RemoveArtifactTypes$`, `RemoveEnchantmentTypes$` = `True` | Remove whole categories - combined with `AddType` this is "becomes X (and is no longer …)": e.g. `AddType$ Forest | RemoveLandTypes$ True` = "is a Forest" (set land type), `AddType$ Creature & Elk | RemoveCardTypes$ True | RemoveCreatureTypes$ True`. Skipped when `AddType` was given but resolved to nothing (choice not made yet). |
| `AddAllCreatureTypes$ True` | Every creature type (Changeling). |

Setting a basic land type in layer 4 and the resulting loss/gain of mana abilities is done by
Forge's type-change code, not by extra script keys (UNVERIFIED in detail; see
`Card.addChangedCardTypes`).

**Layer 5 - colour** (`COLOR`)

| Key | Effect |
|---|---|
| `AddColor$ <colors>` | Adds colours (`White & Blue`, `ChosenColor`, `All`). |
| `SetColor$ <colors>` | Replaces colours (`Colorless` for Devoid, `All`, `ChosenColor`). |
| `RemoveColor$` | Named in the layer-detection list but not read by `StaticAbilityContinuous` (no effect - UNVERIFIED whether read elsewhere). |

**Layer 6 - abilities** (`ABILITIES`)

| Key | Effect |
|---|---|
| `AddKeyword$ K1 & K2` | Grants keywords, written exactly like `K:` lines (`Flying`, `Protection:Card.Red:red`, `Ward:2`, `Equip:2`, `Hexproof`…); they are expanded the same way (Part 4). When the affected object is a player, grants player keywords. Placeholders resolved from the host: `ChosenColor`/`chosenColor`, `ChosenType`, `ChosenNumber`, `ChosenPlayerUID`/`ChosenPlayerName`, `ChosenName`, `ChosenEvenOdd` (the keyword is dropped if the choice is missing); expanding lists: `AllColors`, `CommanderColorID`/`NotCommanderColorID`, `ColorsYouCtrl`, `YourBasic`, `EachCMCAmongDefined` (with `KeywordDefined$`); per affected card: `CardColors`, `CardManaCost`, `ConvertedManaCost`; `HostCardUID`, `HostCardControllerUID`; `N` with `CalcKeywordN$ <expr>` (e.g. `Annihilator:N`). |
| `KeywordMultiplier$ N` | Grant each keyword N times. |
| `SharedKeywordsZone$ <zones>` + `SharedRestrictions$ <valid>` | Only those listed keywords that some card matching the restriction in the zone has (Odric, Majestic Myriarch). |
| `ShareRememberedKeywords$` | Grants the keywords remembered on the host. `FromDraftNotes$` (draft-matters). |
| `RemoveKeyword$ K1 & K2` | Removes those keywords. |
| `CantHaveKeyword$ K1 & K2` | "Can't have or gain" those keywords. |
| `RemoveAllAbilities$ True` | Affected cards lose all abilities (keywords, activated, triggered, static, replacement). |
| `RemoveNonManaAbilities$ True` | Lose all abilities except mana abilities. |
| `AddAbility$ SVar1 & SVar2` | Grants activated abilities/spells; each SVar holds an `AB$ …` string, looked up on the static (then card). `CardManaCost` / `ConvertedManaCost` in the text are replaced per affected card. |
| `AddTrigger$ SVar1 & SVar2` | Grants triggered abilities; each SVar holds a `Mode$ …` trigger line whose `Execute$` SVar is again resolved via the granting static. |
| `AddStaticAbility$ SVar1 & SVar2` | Grants static abilities (SVar holds a `Mode$ …` static line; `ConvertedManaCost` substituted). |
| `AddReplacementEffect$ SVar1 & SVar2` | Grants replacement effects (SVar holds an `Event$ …` line). |
| `AddSVar$ SVar1 & SVar2` | Copies SVars onto the affected card (needed when granted abilities reference SVars by name at run time, e.g. `X`). A value of the form `SVar:Name:Value` renames it. Applied in every layer pass the static takes part in. |
| `GainsAbilitiesOf$ <valid>` (+ `GainsAbilitiesOfZones$`, default battlefield) / `GainsAbilitiesOfDefined$ <Defined>` | Affected cards have all **activated** abilities of the matching cards (`GainsValidAbilities$ <SA filter>` restricts which; `GainsAbilitiesLimitPerTurn$` adds a per-turn limit). |
| `GainsTriggerAbsOf$ <valid>` | Affected cards have the triggered abilities of the matching cards. |
| `GainsStickerAbilitiesOf$` / `…Defined$` | Un-set stickers. |

Granted traits are tied to the static (`…ForStaticAbility`) and cached per (static, affected
card); they are "extrinsic" (`intrinsic=false`), which matters for copy effects.

**Layer 7a / 7b - set P/T** (`CHARACTERISTIC` if `CharacteristicDefining`, else `SETPT`)

| Key | Effect |
|---|---|
| `SetPower$ <n|SVar>` / `SetToughness$ <n|SVar>` | Sets base power/toughness. The value is computed on the host; if the SVar *name* contains `Affected` it is computed per affected card instead. CDA example: `Mode$ Continuous | EffectZone$ All | CharacteristicDefining$ True | SetPower$ X | SetToughness$ X`. |

**Layer 7c - modify P/T** (`MODIFYPT`)

| Key | Effect |
|---|---|
| `AddPower$ <n|SVar>` / `AddToughness$ <n|SVar>` | +N/+N (negative allowed; `AddPower$ -1`). Same `Affected…` SVar-name convention for per-card values. |

**Rules "layer"** (`RULES`; CR 613.11 game-rule effects - not a real layer)

| Key | Effect |
|---|---|
| `AddHiddenKeyword$ K1 & K2` | Grants *hidden* keywords: rule strings checked by the engine but not shown as abilities and not removable by "loses all abilities". Corpus values: `CARDNAME can't block.`, `All creatures able to block CARDNAME do so.`, `CARDNAME must be blocked if able.`, `This card doesn't untap during your next untap step.`, `CARDNAME can't attack or block.`, `CARDNAME can't attack alone.`, `CARDNAME can only attack alone.` |
| `MayPlay$ True` | The affected cards may be played (lands) / cast (spells) by `MayPlayPlayer$ <Defined>` (default: the static's controller) from wherever they are - this is how "you may cast spells from your graveyard / the top of your library / exile" is written (`Affected$ … | AffectedZone$ Graveyard`). Modifiers: `MayPlayWithoutManaCost$ True`; `MayPlayAltManaCost$ <cost>` (`ConvertedManaCost` substituted); `MayPlayWithFlash$ True`; `MayPlayLimit$ N` (N uses per turn, counted on the static); `MayPlayDontGrantZonePermissions$ True` (only modifies cost/timing, does not itself permit playing from that zone); `MayPlayIgnoreColor$` / `MayPlayIgnoreType$` / `MayPlaySnowIgnoreColor$` (mana may be spent as though any colour/type); `MayPlayNotSorcerySpeed$`; `MayPlayText$` (label). The spell option generated remembers the static (`Spell.MayPlaySource` in SA filters). |
| `MayLookAt$ <DefinedPlayer|True>` | The players may look at the affected (hidden/face-down) cards; `True` = the same players as `MayPlay`. |
| `SetMaxHandSize$ <n|Unlimited>`, `RaiseMaxHandSize$ <n>` | Player effect. |
| `AdjustLandPlays$ <n|Unlimited>` | Additional land plays for affected players. |
| `Goad$ True` | Affected creatures are goaded by the host's controller. |
| `CanBlockAny$ True`, `CanBlockAmount$ <n>` | May block any number / n additional creatures. |
| `IgnoreEffectCost$ <cost>` | Adds an activated ability "<cost>: ignore this effect until end of turn" (old "pay {N}: this doesn't affect you" cards). |
| `ControlVote$`, `AdditionalVote$ n`, `AdditionalOptionalVote$ n`, `AdditionalVillainousChoice$ n` | Voting/choice modifiers for affected players. |
| `DeclaresAttackers$ <Defined>`, `DeclaresBlockers$ <Defined>` | That player declares attackers/blockers for the affected player. |
| `ControlOpponentsSearchingLibrary$ <Defined>` | Opposition Agent. |
| `LethalDamageByPower$ True` | Lethal damage is determined by power instead of toughness. |

Examples:
`S:Mode$ Continuous | Affected$ Creature.YouCtrl | AddPower$ 1 | AddToughness$ 1 | Description$ Creatures you control get +1/+1.`
`S:Mode$ Continuous | AffectedDefined$ Equipped | Affected$ Creature | AddPower$ 2 | AddToughness$ 2 | AddStaticAbility$ STFirstStrike | Description$ Equipped creature gets +2/+2 and has "…"`
`S:Mode$ Continuous | AddKeyword$ Lifelink | Affected$ Instant.YouCtrl,Sorcery.YouCtrl | AffectedZone$ Stack | Description$ Instant and sorcery spells you control have lifelink.`
`S:Mode$ Continuous | Affected$ Creature.OppCtrl | RemoveKeyword$ Flying | CantHaveKeyword$ Flying | Description$ …lose flying and can't have or gain flying.`
`S:Mode$ Continuous | Affected$ You | AdjustLandPlays$ 1 | Description$ You may play an additional land on each of your turns.`

## 2.3 Cost-modifying modes (`cost/CostAdjustment.java`)

Scanned on cards on the **battlefield, the stack and the command zone, plus the spell's own
card**; each must pass its conditions (so a card's own cost modifier needs `EffectZone$ All`).
Never applied to triggered abilities or to costs paid during resolution.

Common filter keys (`checkRequirement`): `ValidCard$` (the card whose spell/ability is being
paid for), `ValidSpell$` (filter on the spell-ability), `Activator$` (payer filter: `You`,
`Opponent`, `Player`), `Type$ Spell | Ability | Foretell` (`Spell`: only spells, with
`OnlyFirstSpell$ True` = only the first matching spell that player casts each turn; `Ability`:
activated abilities only; `Foretell` with `FirstForetell$`), `AffectedZone$ <zones>` (the zone
the spell is cast from / the ability's source is in), `ValidTarget$ <valid>` (some target of
the spell matches; with `UnlessValidTarget$` the sense is inverted).

### `ReduceCost` (562)
- `Amount$ <n|SVar>`: how much. Evaluated on the static's host; `Relative$ True` evaluates the
  SVar in the context of the spell being cast (for "for each target", "X less where X is its …");
  the SVar name `AffectedX` is evaluated on the affected card; the literal `Undaunted` = number
  of opponents.
- Default: reduces **generic** mana only, never below `MinMana$ <n>` (default 0) total mana value.
- `Color$ <shards>`: reduce these specific shards instead (`Color$ U` + `Amount$ 2` = `{U}{U}`
  less; a numeric shard inside reduces generic; with `IgnoreGeneric$` coloured reductions cannot
  spill into generic).
- `UpTo$ True`: the caster chooses 0..Amount.
- When several reductions apply the caster orders them (`chooseSingleStaticAbility`).
- A cost of exactly `{0}` is never reduced.

Example: `S:Mode$ ReduceCost | ValidCard$ Creature | Type$ Spell | Activator$ You | Amount$ 1 | Description$ Creature spells you cast cost {1} less to cast.`
Self: `S:Mode$ ReduceCost | ValidCard$ Card.Self | Type$ Spell | Amount$ X | EffectZone$ All | Description$ This spell costs {X} less to cast, where …`

### `RaiseCost` (106)
- `Cost$ <cost>` (default `1`): the additional cost; may be mana or any Forge cost
  (`PayLife<2>`, `Sac<1/Creature>`).
- `Amount$ <n|SVar>` (default 1): how many times `Cost` is added. Special literals:
  `Escalate` (number of chosen modes - 1), `Strive` (targets - 1), `Spree`/`Tiered` (adds each
  chosen mode's `ModeCost$`). `Relative$ True` as above.
- `ForEachShard$ <color>`: once per mana symbol of that colour in the spell's cost.
- Applied before reductions (raise in `adjust(Cost)`, reduce later on the mana being paid).

### `SetCost` (1; Trinisphere)
`Amount$ n` adds generic; with `RaiseTo$ True` only the difference up to n. Applied last.

### `AlternativeCost` (145) — `staticability/StaticAbilityAlternativeCost.java`
Offers an extra way to cast/activate: `ValidSA$` (e.g. `Spell.Self`, `Activated.Equip`),
`ValidCard$`, `ValidPlayer$`, `Cost$ <cost>` (replaces the mana cost for spells / the whole cost
for abilities; `ConvertedManaCost` substituted), optional `XAlternative$`, `Announce$`,
`ManaRestriction$`, `StackDescription$`, `CostDesc$`, `Named$`. Only "basic" (unmodified) spells
get alternatives. If `EffectZone$` names exactly one zone the alternative can only be used from
that zone. Self alt-costs ("you may discard a Swamp card rather than pay this spell's mana
cost") are `ValidSA$ Spell.Self | EffectZone$ All | Cost$ …`.

### `OptionalCost` (40) — `GameActionUtil.getOptionalCostValues`
"As an additional cost you may …": `ValidCard$`, `ValidSA$`, `Activator$`, `Cost$`. Paid-ness is
then tested by effects through the generic optional-cost flags (`OptionalCost.Generic`;
`ReduceColor$ W/U/B/R/G` makes it one of the colour-reduce flags used by a few cards).

### `CantAttackUnless` (31), `CantBlockUnless` (10), `OptionalAttackCost` (28) — `StaticAbilityCantAttackBlock`
- `CantAttackUnless`: `ValidCard$` (attacker), `Target$` (the attacked entity filter), `Cost$`
  (a cost string, or an SVar name evaluated to a generic amount; a leading `X` keeps X shards),
  `RememberingAttacker$` (the attacker is remembered on the host while the SVar is evaluated),
  `Trigger$ <SVar>` (an ability executed when the cost is paid).
- `CantBlockUnless`: `ValidCard$` (blocker), `Attacker$`, `Cost$`.
- `OptionalAttackCost`: same keys as `CantAttackUnless`, but the controller may choose to pay
  (exert: `Cost$ Exert<1/CARDNAME>`, enlist) and `Trigger$` runs if they do.

## 2.4 Combat-restriction modes (`StaticAbilityCantAttackBlock`, `…MustAttack`, `…MustBlock`, …)

| Mode (cards) | Keys and behaviour |
|---|---|
| `CantBlockBy` (585) | "X can't be blocked (by Y)". `ValidAttacker$`; `ValidBlocker$` (comma list; the block is forbidden if the blocker matches any; absent = no creature can block it - plain unblockable is `ValidAttacker$ Card.Self`); `ValidAttackerRelative$` (attacker filter evaluated relative to the blocker), `ValidBlockerRelative$` (blocker filter relative to the attacker, e.g. power comparisons), `ValidDefender$` (the blocker's controller). Evasion keywords are generated in this mode (flying = `ValidBlocker$ Creature.withoutFlying+withoutReach`). Special-casing: a `withoutReach`/`withoutShadow` alternative is waived by `CanBlockIfReach`/`CanBlockIfShadow` statics; landwalk can be turned off by `IgnoreLandwalk`. Statics on the attacker and blocker themselves are consulted first (LKI-safe). |
| `CantAttack` (231) | `ValidCard$` (attacker), `Target$` (attacked entity; absent = can't attack at all), `UnlessDefender$ <player property>`, `DefenderNotNearestToYouInChosenDirection$`. Defender's own "can't attack" is this mode and is waived by `CanAttackDefender`. |
| `CantBlock` (152) | `ValidCard$` (blocker). |
| `CanAttackDefender` (57) | "can attack as though it didn't have defender": `ValidCard$`, `ValidAttacked$`. |
| `MinMaxBlocker` (43) | `ValidCard$` (attacker), `Min$ <n|All>`, `Max$ <n>` ("can't be blocked except by two or more" = `Min$ 2`; "can't be blocked by more than one" = `Max$ 1`). Menace sets min 2 in code. |
| `MustAttack` (156) | `ValidCreature$` attacks each combat if able; `MustAttack$ <Defined entity>` = must attack that player/planeswalker. `PlayerMustAttack` (2): `ValidPlayer$`, `MustAttack$ <valid>`. `AttackRequirement` (4): `ValidCard$`, `ValidAttacker$` (attacks if another matching creature attacks). |
| `MustBlock` (24) | `ValidCreature$` blocks each combat if able. |
| `CanAttackIfHaste` (3) | `ValidCard$`, `ValidTarget$`: may attack despite summoning sickness. |
| `CanBlockIfReach` (1), `CanBlockIfShadow` (4) | `ValidAttacker$`, `ValidBlocker$`. |
| `BlockTapped` (1) | `ValidCard$` may block while tapped. `AttackVigilance` (1 + every Vigilance keyword): `ValidCard$` doesn't tap to attack. |
| `AttackRestrict` (8) | `MaxAttackers$ n` (global, or per `ValidDefender$`). `BlockRestrict` (4): `MaxBlockers$ n`, `ValidDefender$`. |
| `AssignNoCombatDamage` (26) | `ValidCard$` assigns no combat damage. |
| `AssignCombatDamageAsUnblocked` (19) | `ValidCard$`; `Optional$ True` = "you may have it assign … as though it weren't blocked". |
| `CombatDamageToughness` (22) | `ValidCard$` assigns combat damage equal to toughness. `CombatDamageNegatePower` (2). |
| `IgnoreLandwalk` (10) | `ValidAttacker$`, `ValidBlocker$`, `ValidKeyword$`. |
| `NoCleanupDamage` (7) | `ValidCard$`: damage isn't removed during cleanup. |

## 2.5 Casting / activating / targeting restrictions

| Mode (cards) | Keys and behaviour |
|---|---|
| `CantBeCast` (123) | `ValidCard$` (the spell's card), `Caster$` (player filter), `OnlySorcerySpeed$` (can't cast except when a sorcery could be), `Origin$ <zones>` (only spells cast from there), `cmcGT$ <Turns|Type>` (mana value greater than the caster's turn count / number of permanents of a type), `NumLimitEachTurn$ n` (+`ValidCard`: can't cast more than n matching spells per turn). The card being cast is included in the scan, so "can't be cast unless …" on the card itself works from any zone via `EffectZone$ All`. |
| `CantBeActivated` (42) | `ValidCard$` (source), `ValidSA$` (`Activated`, `Activated.!ManaAbility`, …), `Activator$`, `AffectedZone$` (single zone the source must be in). Triggers are unaffected. |
| `CantPlayLand` (17) | `ValidCard$`, `Origin$`, `Player$`. |
| `CastWithFlash` (66) | `ValidCard$`, `ValidSA$`, `Caster$`: matching spells may be cast as though they had flash. |
| `CantTarget` (33) | `ValidTarget$` (the would-be target: card or player), `AffectedZone$` (zone a card target must be in; default battlefield - a static without it never protects cards elsewhere), `ValidSA$` (the targeting spell/ability), `ValidSource$` (its source card), `Activator$` (its controller), `SourceCanOnlyTarget$`. Hexproof/Shroud/Protection are generated in this mode (Part 4). |
| `IgnoreHexproof` (4), `IgnoreShroud` (1) | `Activator$`, `ValidEntity$`: turns off the Hexproof/Shroud keyword-generated `CantTarget`. |
| `MustTarget` (3) | Flagbearer: `ValidSA$`, `ValidTarget$` (a type name), `ValidZone$`. |
| `CantAttach` (7) | `ValidCard$` (the attachment), `Target$` (what it would attach to), `ValidCardToTarget$`, `Exceptions$`, `ExceptionSBA$`. Protection generates one. |
| `CantBeCopied` (3), `CantGainControl` (1), `CantTransform` (1; `ExceptCause$`), `CantPhaseIn` (4), `CantPhaseOut` (2), `CantRegenerate` (18), `CantCrew`, `CantBeSuspected` (1), `CantBeBeamedUp` (2), `CountersRemain` (2) | `ValidCard$` only. |

## 2.6 Player / game-rule modes

| Mode (cards) | Keys and behaviour |
|---|---|
| `CantGainLife` (26), `CantLoseLife` (2), `CantChangeLife` (5), `CantPayLife` (3) | `ValidPlayer$`. `CantChangeLife` implies both; paying life is also blocked by `CantLoseLife`. `ForCost$`, `ValidCause$` on the pay-life check. |
| `CantSacrifice` (16), `CantExile` (1), `CantDiscard` (1) | `ValidCard$` / `ValidPlayer$`, `ValidCause$`, `ForCost$ True/False` (restrict to cost payments / to effects). |
| `CantDraw` (6) | `ValidPlayer$`, `DrawLimit$ n` (can't draw more than n per turn; default 0). |
| `CantPutCounter` (9) | `ValidCard$` and/or `ValidPlayer$`, `CounterType$`. `MaxCounter` (1): `ValidCard$`, `CounterType$`, `MaxNum$`. |
| `CantPreventDamage` (23) | `ValidSource$`, `IsCombat$`. |
| `Panharmonicon` (40) | An ability triggers an additional time. `ValidCard$` (the **source of the trigger**), `ValidMode$` (trigger modes), `ValidZone$` (zone of the trigger's source; default battlefield). Event-specific filters: for `ChangesZone`/`ChangesZoneAll` `ValidCause$` (the moved card), `Origin$`, `Destination$`; for `Attacks`/`AttackersDeclared` `ValidCause$` (attackers); for spell-cast modes `ValidCause$` (the spell's card), `ValidActivator$`; `ValidTarget$` (becomes-target; damage modes), `ValidSource$`, `CombatDamage$` (damage modes); `ValidTurned$` (TurnFaceUp); `ValidPlayer$` (LifeGained). Never applies to delayed triggers or `Static$` helper triggers. |
| `DisableTriggers` (8) | Torpor Orb / Hushbringer: `ValidCard$` (trigger source; not delayed triggers), `ValidTrigger$` (filter on the trigger's ability), `ValidMode$`, and for zone-change modes `ValidCause$` (the moving card), `Origin$`, `Destination$`. A dying creature's own "put into a graveyard from anywhere" trigger is exempted. |
| `IgnoreLegendRule` (11), `IgnorePlaneswalkerZeroLoyaltyRule` (1) | `ValidCard$`. |
| `UntapOtherPlayer` (15) | `ValidCard$` untaps during `ValidPlayer$`'s untap step ("untap during each other player's untap step"). |
| `ManaConvert` (17) | `ValidPlayer$`, `ValidCard$`, `ValidSA$`, `ManaConversion$ <from->to>` (`AnyType->AnyColor`, `AnyColor->AnyColor`…), `Optional$`. |
| `UnspentMana` (7) | `ValidPlayer$`, `ManaType$` (that type, or all, doesn't empty). `ManaBurn` (1). `ManaRestriction` (3): `ValidSource$`. |
| `TapPowerValue` (11) | `ValidCard$`, `ValidSA$`, `Value$ <n|Toughness>`: crews/stations as though its power were greater / using toughness. |
| `NumLoyaltyAct` (6) | `ValidCard$`, `Twice$`, `Additional$`, `OnlySourceAbs$`. `Activations` (3): `ValidCard$`, `ValidSA$`, `ValidPlayer$`, `MinLimit$`, `Additional$` (activation-limit changes). |
| `ActivateAbilityAsIfHaste` (4) | `ValidCard$`. |
| `ColorlessDamageSource`, `InfectDamage` (`ValidTarget$`), `WitherDamage`, `Devotion` (`ValidPlayer$`, `Value$`), `DrawFromBottom`, `ForetellAnyTurn`, `PlotZone`, `SurveilNum`, `GainLifeRadiation`, `FlipCoinMod`/`FlipCoinDoubler`, `CantVenture`, `CantBecomeMonarch`, `CantChangeDayTime`, `TurnReversed`/`PhaseReversed`, `CanAdapt` | One card each; `ValidCard$`/`ValidPlayer$` as listed in the class of the same name. |

---

# Part 3 — Replacement effects

Sources: `replacement/ReplacementEffect.java`, `ReplacementHandler.java`, `ReplacementType.java`,
`ReplacementLayer.java`, `ReplacementResult.java`, one `replacement/Replace<Event>.java` per
event, the helper effects `ability/effects/ReplaceEffect.java`, `ReplaceDamageEffect.java`,
`ReplaceSplitDamageEffect.java`, `ReplaceTokenEffect.java`, `ReplaceCounterEffect.java`,
`ReplaceManaEffect.java`, and the call sites (`GameAction.changeZone`, `Player`, `Card`,
`GameEntityCounterTable`, `TokenEffectBase`, `AbilityManaPart`, `PhaseHandler`). Forge's
notes: `docs/Card-scripting-API/Replacements.md`.

Shape: `R:Event$ <Event> | <event keys> | <generic keys> | ReplaceWith$ <SVar> | Description$ <text>`

## 3.1 How a replaceable event is processed

1. Engine code builds `runParams` for the event (`AbilityKey.Affected` = the card or player
   the event happens to, plus event data) and calls `ReplacementHandler.run(event, runParams)`.
2. For each `ReplacementLayer` in the order `CantHappen, Control, Copy, Transform, Other`
   (CR 614.17, 616.1b–e) the handler collects the applicable effects of that layer from **every
   card in the game** (`getReplacementList`): right event (`modeCheck`), not already applied to
   this event (`hasRun`), host in `ActiveZones` and not phased out (`zonesCheck`),
   `requirementsCheck` (generic conditions) and the event class's `canReplace(runParams)`.
3. If there are several, the **affected player / controller of the affected object** chooses
   one (`chooseSingleReplacementEffect`, CR 616.1); `CantHappen` effects are never a choice.
4. `executeReplacement`:
   - `Optional$` -> ask the decider (default: the affected player; `OptionalDecider$` overrides);
     "no" = not replaced, try the others.
   - `Prevent$ True` -> result `Prevented` (nothing happens), unless damage can't be prevented.
   - `Skip$ True` -> result `Skipped` (phase/turn skipping).
   - else, if there is a `ReplaceWith` ability: fill its replaced objects (3.3), run it
     immediately (not on the stack) with the **host's controller** as activator.
   - Result: `ReplacementResult$ <value>` if given; else whatever the ability stored in
     `runParams` (`ReplaceEffect`, `ReplaceDamage`, `ReplaceToken`, `ReplaceCounter`,
     `ReplaceMana` set `Updated`/`Replaced`); else `Replaced`.
5. `Replaced` / `Prevented` / `Skipped`: the original event does **not** happen (the
   `ReplaceWith` ability was the substitute). `Updated`: the event's parameters were modified
   and the handler runs again on the modified event for the remaining effects (each effect
   applies at most once to one event - CR 614.5), then the caller performs the modified event.
   `NotReplaced`: unchanged.

Consequence for translation: **the same script shape means "instead" or "also/modified"
depending on `ReplacementResult$`**. `R:Event$ Moved | … | ReplaceWith$ Exile` (no result key)
= the move is cancelled and the `Exile` ability moves the card somewhere else instead;
`R:Event$ Moved | … | ReplacementResult$ Updated | ReplaceWith$ ETBTapped` = the move happens,
after the ability has modified the entering object.

For zone changes to the battlefield, the candidate effects are evaluated against a
last-known-information copy of the entering card *as it would exist on the battlefield*
(with its entering counters and with the continuous effects that already exist applied -
CR 614.12), and effects of other permanents only apply if their source was already on the
battlefield before the event (`canReplaceETB`: an effect that is not `ValidCard$ Card.Self…`
never applies to its own host entering, nor from a host entering simultaneously).

Damage is special: all damage of one batch is gathered first and replacement/prevention
effects are applied per damaged player in APNAP order (`runReplaceDamage`), with the damaged
player/controller choosing the order; redirection/splitting re-queues the modified event.

## 3.2 Generic keys

| Key | Meaning |
|---|---|
| `Event$` | `ReplacementType` name (case-insensitive). |
| `ActiveZones$ <zones>` | Zones the **host** must be in. **Absent = every zone** (`zonesCheck` passes when no zones are set) - this is why self-replacements like "enters tapped", "can't be countered", "if this would be put into a graveyard from anywhere" carry no `ActiveZones`, while effects of permanents say `ActiveZones$ Battlefield` (and emblems/effects `Command`). |
| `ReplaceWith$ <SVar>` | The ability executed in place of / on top of the event. Looked up on the card state (or the granting static). |
| `ReplacementResult$ Updated|Replaced|Prevented|Skipped|NotReplaced` | Forces the outcome (see 3.1). `Updated` is the standard marker for "as/if it enters, also …" modifications. |
| `Prevent$ True` | Pure prevention: the event simply doesn't happen. For damage it is a prevention effect (ignored when damage can't be prevented). |
| `PreventionEffect$ True` | The effect is a damage-*prevention* effect even though it has a `ReplaceWith` (e.g. "prevent 1 of that damage" via `DB$ ReplaceDamage`); skipped when damage can't be prevented (`AlwaysReplace$` still records the amount). |
| `Skip$ True` | For `BeginPhase`/`BeginTurn`: skip it. |
| `Layer$ CantHappen|Control|Copy|Transform|Other` | Replacement category, default `Other` (default `Control` for `BeginPhase`). `CantHappen` = "can't" (614.17): checked first, also queried in advance through `cantHappenCheck` (e.g. is this spell counterable, can this untap). `Control` = effects that change under whose control something enters; `Copy` = "enters as a copy" / face-up choices; `Transform` = enters transformed. |
| `Optional$ True` (+ `OptionalDecider$ <Defined>`) | "You may … instead". |
| `Description$` | Text only (also the text of the confirmation prompt and the log entry). |
| `Secondary$ True` | Hide text. |
| `PlayerTurn$ True|<Defined>` | Only during the host controller's turn / a defined player's turn. |
| `ActivePhases$ <range>` | Only during those phases. |
| Condition keys | The same `meetsCommonRequirements` set as triggers (1.3): `IsPresent…`, `CheckSVar`/`SVarCompare`, `CheckDefinedPlayer`, `LifeTotal`, `Metalcraft`…, `DayTime`, `ClassLevel`, `ManaSpent`… |
| `Valid…$` / `Invert…$` | Event-specific filters, as for triggers. |
| `AILogic$` | AI hint; ignore. |

## 3.3 Replaced objects visible to the `ReplaceWith` ability

Each event class's `setReplacingObjects` stores values on every ability of the chain; the
whole original `runParams` map is also stored (as `OriginalParams`) so helper effects can
modify the event in place. Access from scripts (prefix `Replaced`, lookup by `AbilityKey` name):

- `Defined$ Replaced<Key>` as a card: the stored object **as is** (for `Moved` this is the
  moving card - no LKI re-resolution, unlike `Triggered…`). Collections give all their cards.
- `Defined$ Replaced<Key>` as a player; suffixes `Controller`, `Owner` etc. work like the
  `Triggered` ones (`ReplacedCardController`, `ReplacedSourceController`, `ReplacedCardLKIController`).
- `ReplaceCount$<Key>[/op]` in an SVar: an integer replaced object with optional arithmetic -
  `ReplaceCount$DamageAmount/Twice`, `…/Plus.1`, `…/Minus.1`, `…/Thrice`, `ReplaceCount$LifeGained/Twice`,
  `ReplaceCount$CounterNum/Plus.1`, `ReplaceCount$Amount/…`, `ReplaceCount$Number/Plus.1`,
  `ReplaceCount$TokenNum`.
- `Replaced<Key>$<CardProperty>`: a property of the replaced card.

| Event | Replaced objects |
|---|---|
| `Moved` | `Card` (the moving card), `NewCard` (the would-be battlefield object, ETB case), `CardLKI`, `Cause`, plus internals (`LastStateBattlefield`, `LastStateGraveyard`, `CounterTable`, `CounterMap`) |
| `DamageDone` | `DamageAmount`, `Target` (the would-be damaged card/player), `Source` |
| `Draw` | `Player`, and if caused by an ability `Cause`, `Source` (its card) |
| `DrawCards` | `Player`, `Number` |
| `GainLife` | `LifeGained`, `Player` |
| `LifeReduced` | `Amount`, `Player` |
| `PayLife` | `Player`, `Amount` |
| `CreateToken` | `TokenNum` (number of matching tokens), `Token` (the table), `Cause`, `Player` |
| `AddCounter`, `RemoveCounter` | `CounterMap`, `Card` or `Player` (whichever it is), `Object`; `ReplaceCounter` sets `CounterNum` |
| `Destroy` | `Card`, `Cause` |
| `Counter` | `Card` (the spell's card) |
| `Untap`, `Tap`, `Transform`, `Explore`, `Connive`, `Attached`, `DealtDamage`, `AssignDealDamage` | `Card` |
| `TurnFaceUp` | `Card`, `Cause` |
| `ProduceMana` | `Mana` (string of produced mana, e.g. `"G G"`) |
| `LoseMana` | `Player`, `Mana` |
| `BeginPhase`, `BeginTurn`, `Learn`, `DeclareBlocker` (`DefendingPlayer`) | `Player` |
| `Mill` | `Player`, `Number`; `Scry`, `Proliferate`: `Player`, `Num` |
| `Cascade` | `Player`, `Cards`; `CopySpell`: `Amount`; `RollDice`: `Number`, `Ignore`, `IgnoreChosen`, `DicePTExchanges`; `RollPlanarDice`: `Number`, `Ignore`; `PlanarDiceResult`: `Result` |
| `GameLoss`, `GameWin`, `SetInMotion` | none |

### Helper effects used inside `ReplaceWith`

| Effect | What it does (file) |
|---|---|
| `DB$ ReplaceEffect | VarName$ <Key> | VarValue$ <expr> [| VarType$ amount|Card|Player|GameEntity|Map|CardSet|PlanarDice]` | Overwrites one parameter of the original event and marks it `Updated`. Default type `amount` (integer expression). `VarName$ DamageAmount | VarValue$ X` with `SVar:X:ReplaceCount$DamageAmount/Twice` = damage doubling; `VarName$ LifeGained`, `VarName$ Amount`, `VarName$ Number`; `VarName$ Affected | VarType$ Card|Player|GameEntity` = redirect the event to another object. (`ReplaceEffect.java`) |
| `DB$ ReplaceDamage | Amount$ <n|SVar>` | Prevents/reduces the next N damage (default 1): event `Replaced` if nothing is left, else `Updated` with the smaller amount; when `Amount` is an SVar of the form `Number$n` on the host it is decremented (shield bookkeeping) and an exhausted effect card is exiled. Sets SVar `PreventedDamage`. (`ReplaceDamageEffect.java`) |
| `DB$ ReplaceSplitDamage | DamageTarget$ <Defined> | VarName$ <n>` | Redirects part of the damage to another object (shield amount in `VarName`, default 1). (`ReplaceSplitDamageEffect.java` - parameters beyond these UNVERIFIED) |
| `DB$ ReplaceToken | Type$ Amount|AddToken|ReplaceToken|ReplaceController` | Edits the table of tokens about to be created: `Amount` (+`Amount$ Twice`/`Plus.1`… default `Twice`) scales counts; `AddToken` (+`TokenScript$`, optional `Amount$`) adds further tokens; `ReplaceToken` (+`TokenScript$ <script|Chosen>`, `ValidChoices$`) substitutes them; `ReplaceController` (+`NewController$`) changes who gets them. Only tokens matching the replacement's `ValidToken$` are touched. (`ReplaceTokenEffect.java`) |
| `DB$ ReplaceCounter | Amount$ <expr> [| ValidCounterType$ T] [| ValidSource$ …] [| ChooseCounter$ True]` | Rewrites the number of counters about to be put; the old number is available as `ReplaceCount$CounterNum` (`Amount$ X`, `SVar:X:ReplaceCount$CounterNum/Twice`). `ChooseCounter` is for "+1 counter" effects when several kinds/sources are involved. (`ReplaceCounterEffect.java`) |
| `DB$ ReplaceMana | ReplaceMana$ <mana|Any>` / `ReplaceType$ <type|Any>` / `ReplaceColor$ <color|Chosen> [| ReplaceOnly$ <c>]` / `ReplaceAmount$ <n>` | Rewrites the produced-mana string: fixed replacement, change every symbol's type, change colours only, or multiply. (`ReplaceManaEffect.java`) |
| any ordinary effect with `ETB$ True` (`DB$ Tap`, `DB$ Untap`, `DB$ PutCounter`, `DB$ SetState`, `DB$ ChooseColor`/`ChooseType`, `DB$ Clone`, …) | Applied to the entering card *before* it is on the battlefield; combined with `ReplacementResult$ Updated`. |
| `DB$ ChangeZone | Defined$ ReplacedCard | Origin$ … | Destination$ …` (often `Hidden$ True | Origin$ All`) | The substitute move for "instead" replacements. |

## 3.4 Events

### `Moved` (1072 cards) — a card would change zones
`replacement/ReplaceMoved.java`; run from `GameAction.changeZone` before the move, with
`Affected` = the card, `CardLKI`, `Cause`, `Origin`, `Destination` (zone types),
`EffectOnly` (true for moves to the battlefield and for moves done by effects, false for
cost payments/state-based actions), and flags `Fizzle`, `Cycling`, `Discard`,
`FoundSearchingLibrary`, `StackSa` when applicable.

| Key | Meaning |
|---|---|
| `Origin$`, `Destination$` | Zone lists the origin/destination must be in. `ExcludeDestination$` = must not be. |
| `ValidCard$` | Filter on the moving card (for moves to the battlefield: evaluated on the would-be battlefield object). `Card.Self` = this card. |
| `ValidLKI$` | Filter on the last-known-information snapshot from the origin zone - used for "if a creature would die" (`Origin$ Battlefield | Destination$ Graveyard | ValidLKI$ Creature…`) and for spells/cards "that would be put into a graveyard" where current characteristics would be wrong. |
| `ValidStackSa$` | Filter on the spell-ability that is on the stack for the moving card (`Spell.Flashback+castKeyword`, `Spell.Buyback`). |
| `ValidCause$` | Filter on the ability causing the move. |
| `Fizzle$ True/False` | Spell leaving the stack because it fizzled / resolved normally. |
| `Cycling$ True/False`, `Discard$ True/False` | Moved by cycling / by being discarded. |
| `EffectOnly$ True` | Only moves caused by effects (not costs or state-based actions). |
| `FoundSearchingLibrary$ True` | The card was found by a library search. |

Standard idioms:

- **Enters tapped**: `R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplacementResult$ Updated | ReplaceWith$ ETBTapped | Description$ CARDNAME enters tapped.` with `SVar:ETBTapped:DB$ Tap | Defined$ Self | ETB$ True`. "Enters tapped unless …": same, with the condition on the effect (`ConditionPresent…`/`UnlessCost$`) or on the replacement (`IsPresent`, `CheckSVar`). Other permanents: `ActiveZones$ Battlefield | ValidCard$ Creature.OppCtrl | … | ReplaceWith$ … DB$ Tap | ETB$ True | Defined$ ReplacedCard`.
- **Enters with counters**: `K:etbCounter:…` (Part 4) expands to `Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplacementResult$ Updated` + `DB$ PutCounter | Defined$ Self | CounterType$ … | CounterNum$ … | ETB$ True`. For other permanents: `Event$ Moved | ActiveZones$ Battlefield | ValidCard$ Creature.YouCtrl+Other | Destination$ Battlefield | ReplacementResult$ Updated | ReplaceWith$ …` with `DB$ PutCounter | ETB$ True | Defined$ ReplacedCard | …`. (Note: the `AddCounter` event also sees counters put on an entering permanent, see below.)
- **As this enters, choose …**: `K:ETBReplacement:Other:<SVar>` or `R:Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplacementResult$ Updated | ReplaceWith$ <choice effect>`.
- **Enters as a copy**: `K:ETBReplacement:Copy:<SVar>:Optional` (layer `Copy`, effect `DB$ Clone`).
- **If it would die, exile it instead**: `R:Event$ Moved | ActiveZones$ Battlefield | Origin$ Battlefield | Destination$ Graveyard | ValidLKI$ Creature.OppCtrl | ReplaceWith$ Exile` with `SVar:Exile:DB$ ChangeZone | Hidden$ True | Origin$ All | Destination$ Exile | Defined$ ReplacedCard` (58 scripts use exactly this ability). No `ReplacementResult` -> the original move is cancelled.
- **If this would be put into a graveyard from anywhere, exile/shuffle it instead**: `R:Event$ Moved | ValidCard$ Card.Self | Destination$ Graveyard | ReplaceWith$ Exile` (no `ActiveZones`, no `Origin`).
- **Leaves-the-battlefield exile riders** ("if it would leave the battlefield, exile it instead"): `Origin$ Battlefield | ExcludeDestination$ Exile`.
- **Spell-to-graveyard riders** (flashback, buyback, rebound): `Origin$ Stack | Destination$ Graveyard | Fizzle$ False | ValidStackSa$ …`.
- **Enters under another player's control**: `Layer$ Control`.

### `DamageDone` (446) — damage would be dealt
`replacement/ReplaceDamage.java`. Never applies to 0 damage.

| Key | Meaning |
|---|---|
| `ValidSource$` | Damage source (`Card.Self`, `Creature.EnchantedBy`, `Card,Emblem` for "a source", `Card.RedSource`…). |
| `ValidTarget$` | Would-be damaged object or player (`You`, `Creature.YouCtrl`, `Card.Self`, `Permanent`, `Player`). |
| `ValidCause$`, `CauseIsSource$` | The dealing ability; `CauseIsSource`: the cause's host is the source. |
| `RelativeToSource$ <valid>` | Filter on the target evaluated relative to the source. |
| `IsCombat$ True/False` | Combat / noncombat only. |
| `DamageAmount$ <cmp>` | Amount comparison. |
| `DamageTarget$ <Defined>` | Marks a redirection effect and names where the damage goes; the effect is inapplicable if redirection is impossible (target gone, not a creature/planeswalker/battle, `NoRedirection`). |
| `Prevent$ True` | Prevent all of it. With `PreventionEffect$`/`ReplaceWith$ DB$ ReplaceDamage` for partial prevention. |

Idioms: prevent all = `Prevent$ True`; "prevent all combat damage that would be dealt to you"
= `ValidTarget$ You | IsCombat$ True | Prevent$ True`; double = `ReplaceWith$ Dbl` /
`DB$ ReplaceEffect | VarName$ DamageAmount | VarValue$ X` / `SVar:X:ReplaceCount$DamageAmount/Twice`;
"deals that much damage plus 1" = `/Plus.1`; redirect = `DB$ ReplaceEffect | VarName$ Affected | VarValue$ … | VarType$ Card`.
Protection's damage prevention is generated as `Event$ DamageDone | Prevent$ True | ActiveZones$ Battlefield | ValidTarget$ Card.Self | ValidSource$ …`.
Note: an ability that is neither `ReplaceDamage`/`ReplaceSplitDamage`/`ReplaceEffect` on a
`DamageDone` replacement is not run immediately; it is buffered and the event counts as
`Replaced` ("if damage would be dealt, do X instead").

### `Untap` (209) — a permanent would untap
`ValidCard$`; `ValidStepTurnToController$ <player filter>` = whose untap step it is,
*relative to the permanent's controller* (`You` = "during its controller's untap step").
Nearly always `Layer$ CantHappen` ("doesn't untap during your untap step"). Without
`ValidStepTurnToController` it also blocks untapping by effects ("can't become untapped").

### `Counter` (115) — a spell/ability would be countered
`ValidCard$` (the spell's card), `ValidSA$` (`Spell`, `Activated`…), `ValidCause$` (the
countering ability). "Can't be countered" = `R:Event$ Counter | ValidCard$ Card.Self | ValidSA$ Spell | Layer$ CantHappen`
(no `ActiveZones`); granted by permanents with `ActiveZones$ Battlefield | ValidCard$ <filter>`.

### `Draw` (50) / `DrawCards` (2)
`Draw` is per single card: `ValidPlayer$`, `ValidCause$`, `NotFirstCardInDrawStep$ True`
("except the first one you draw in each of your draw steps"), `FirstExtraCardDrawnThisTurn$`.
The `ReplaceWith` ability is the substitute ("exile the top card instead", "draw two instead").
`DrawCards` is per draw instruction: `ValidPlayer$`, `Number$ <cmp>`; modify with
`ReplaceEffect | VarName$ Number`. Dredge is an optional `Draw` replacement active in the graveyard.

### `AddCounter` (36) — counters would be put on a card/player
`replacement/ReplaceAddCounter.java`. `ValidCard$` / `ValidPlayer$` / `ValidObject$` (all test
the recipient), `ValidSource$` (the player putting them), `ValidCause$`, `ValidCounterType$`,
`EffectOnly$`. Also consulted for counters a permanent **enters with** (the handler treats a
`Moved`-to-battlefield event carrying a counter map as an `AddCounter` event too; scripts
therefore often write `ValidCard$ Creature.inZoneBattlefield` to exclude that, or omit it to
include it - Doubling Season / Hardened Scales behaviour). Modify with `DB$ ReplaceCounter`.

### `CreateToken` (35)
`replacement/ReplaceToken.java`. `ValidPlayer$` (the player the tokens are created for),
`ValidToken$` (filter on the token prototypes - `Card.YouCtrl`, `Creature`, `Treasure`…),
`EffectOnly$`. Applies only if at least one token matches. Modify with `DB$ ReplaceToken`.

### `GameLoss` (24) / `GameWin` (10)
`ValidPlayer$`; `GameLoss` also `ValidLoseReason$` (`LifeReachedZero`, `Milled`, `Poisoned`,
`CommanderDamage`, … enum `GameLossReason`). Always `Layer$ CantHappen` ("can't lose/win").

### `GainLife` (22)
`ValidPlayer$`, `ValidSource$` (filter on the causing spell-ability), `SourceController$ True`
(the gain is caused by something the gaining player controls). Never applies to 0.
`Prevent$ True` = "can't gain life" style (there is also the static `CantGainLife`);
modify with `ReplaceEffect | VarName$ LifeGained`.

### `BeginPhase` (19) / `BeginTurn` (5)
`BeginPhase`: `ValidPlayer$`, `Phase$ <range>`, default layer `Control`; `Skip$ True` skips
the step ("skip your draw step"); with `Optional$` + `ReplaceWith` = "you may skip … if you do".
`BeginTurn`: `ValidPlayer$`, `ExtraTurn$ True`.

### `ProduceMana` (17)
`ValidCard$` (the permanent whose ability makes mana), `ValidPlayer$`, `ValidActivator$`,
`ValidSA$` (default `Activated.hasTapCost+ManaAbility`, i.e. "tapped for mana"),
`ManaAmount$ <cmp>`. Modify with `DB$ ReplaceMana`.

### `LifeReduced` (11) / `PayLife` (1)
`LifeReduced`: `ValidPlayer$`, `IsDamage$ True/False`, `Result$ <cmp>` on the resulting life
total ("would reduce your life total to less than 1"); modify with `ReplaceEffect | VarName$ Amount`.
`PayLife`: `ValidPlayer$`, `Amount$ <cmp>`.

### `TurnFaceUp` (8), `Transform` (4)
`ValidCard$` (`TurnFaceUp` also `ValidCause$`). "As this is turned face up / transforms, …".

### `Destroy` (7)
`ValidCard$`, `ValidCause$`, `Regeneration$ True` (only destruction that allows regeneration,
and only if the permanent can be regenerated). Regeneration shields and umbra armor
(`Event$ Destroy | ValidCard$ Card.EnchantedBy` -> remove damage, destroy the Aura) use it.
Indestructible is **not** a replacement (it is a keyword check in `Card.canBeDestroyed`).

### Remaining events (brief)

| Event (cards) | Keys |
|---|---|
| `LoseMana` (5) | `ValidPlayer$` - unspent mana would empty. |
| `Tap` (0 in `R:` lines) | `ValidCard$`, `Attacker$ True/False`. |
| `Attached` (3) | `ValidCard$` (attachment), `ValidTarget$`. |
| `RollDice` (3), `RollPlanarDice` (1), `PlanarDiceResult` | `ValidPlayer$`, `ValidSides$`; `ValidRoll$`. |
| `Mill` (2), `Scry` (2), `Proliferate` (1), `Learn` (1), `Cascade` (1), `Connive` (1), `Explore` (2; `ValidExplorer$`) | `ValidPlayer$` / `ValidCard$`. |
| `RemoveCounter` (2) | `ValidCard$`, `ValidCounterType$`, `IsDamage$`, `Result$ <cmp>`. |
| `CopySpell` (1) | `ValidPlayer$`, `ValidSpell$`. |
| `DeclareBlocker` (1) | `ValidPlayer$`. |
| `Planeswalk` (1), `AssembleContraption` (1), `SetInMotion` | `ValidPlayer$`, `ValidCause$`. |
| `AssignDealDamage`, `DealtDamage` | `ValidCard$` (internal combat-damage helpers). |

---

# Part 4 — Keyword lines (`K:`)

Sources: `keyword/Keyword.java` (enum: display name, instance class, "multiple instances
redundant" flag, reminder text), `keyword/KeywordInstance.java` and the parameter classes
(`KeywordWithAmount`, `KeywordWithCost`, `KeywordWithCostAndAmount`, `KeywordWithCostAndType`,
`KeywordWithType`, `Affinity`, `Equip`, `Hexproof`, `Kicker`, `Landwalk`, `Protection`,
`Suspend`, `Trample`, `Vanishing`, `Ward`, …), `card/CardFactoryUtil.java`
(`addTriggerAbility`, `addReplacementEffect`, `addSpellAbility`, `addStaticAbility`,
`makeEtbCounter`, `createETBReplacement`), `GameActionUtil.java` (alternative / optional /
additional keyword costs), `cost/CostAdjustment.java` (convoke, delve, improvise, …).

## 4.1 How a `K:` line is interpreted

- `Keyword.getInstance(line)` (`getKeywordDetails`):
  1. if the line contains `:` -> name = text before the first `:`, details = the rest
     (a trailing `:Flavor <title>` is stripped: flavour-word ability titles);
  2. else if it contains a space -> try the whole line as a display name (`First Strike`,
     `Double Strike`, `Split second`, `Battle cry`); if that is not a keyword, try the first
     word as the name with the rest as details (`Protection from red` -> `Protection`, details
     `from red`);
  3. else the line is the name.
  Names are matched case-insensitively against `Keyword.displayName`.
- A line that matches no enum entry becomes `Keyword.UNDEFINED` (a `SimpleKeyword`): it has
  no parsed parameters and exists only as its literal string. The engine finds these with
  `card.hasKeyword("<exact string>")` / `startsWith` checks. This is how script directives
  (`ETBReplacement`, `etbCounter`, `MayEffectFromOpeningHand`, …) and sentence-keywords
  (`CARDNAME can't block.`) work.
- `KeywordInstance.createTraits` then calls, for **every** keyword instance, the four
  generators in order: `addTriggerAbility`, `addReplacementEffect`, `addSpellAbility`,
  `addStaticAbility`. Each is a long `if (keyword.startsWith(…))` chain keyed on the original
  string; the generated triggers / replacements / abilities / statics are stored on the
  keyword instance, so removing the keyword ("loses flying", `RemoveKeyword$`) removes them.
  All generated script strings below are quoted from those methods; `Secondary$ True` and
  description text are omitted for brevity.
- Keywords granted by effects (`AddKeyword$`, `KW$`, `Keywords$`) use exactly the same
  strings and the same expansion.
- Costs inside keyword arguments use Forge's cost syntax: mana shards separated by spaces
  (`2 G`, `1 U U`, `X 3 W`, `XMin1 X 3 U`) optionally followed by non-mana parts
  (`PayLife<3>`, `Discard<1/Card>`, `Sac<1/Creature>`, `ExileFromGrave<3/Card.Other/other>`,
  `tapXType<2/Creature>`, `AddCounter<1/M1M1>`…). The literal cost `ManaCost` means "its mana cost".
- Simple evergreen keywords have **no** generated trait or only a helper static; their rules
  live in engine code that asks `hasKeyword(Keyword.X)` (combat, damage, timing).

Parameter-class shapes (what follows the keyword name):

| Class | Details syntax |
|---|---|
| `SimpleKeyword` | none |
| `KeywordWithAmount` | `<n>` or `X[:extra]` |
| `KeywordWithCost` | `<cost>[:…]` (only the part before a `\|` of the first field is the cost) |
| `KeywordWithCostAndAmount` | `<n|X>:<cost>` |
| `KeywordWithCostAndType` | `<type>:<cost>[:<description>]` |
| `KeywordWithType` | `<valid>[:<description>]`, or a bare colour word (-> `Card.<Color>`) |

## 4.2 Keywords used by 20 or more cards

Counts are cards in the corpus. "Engine" = implemented by `hasKeyword` checks rather than by
generated traits.

### Evergreen / simple (engine-checked)

| Keyword (cards) | Syntax | Meaning and implementation |
|---|---|---|
| `Flying` (3249) | `K:Flying` | CR 702.9. Generates `Mode$ CantBlockBy | ValidAttacker$ Creature.Self | ValidBlocker$ Creature.withoutFlying+withoutReach` (`addStaticAbility`). |
| `Reach` (429) | `K:Reach` | No trait; it is the `withoutReach` test inside Flying's static. |
| `Trample` (1020) | `K:Trample`, `K:Trample:Planeswalker` | Engine (`combat/Combat.java` damage assignment). The `:Planeswalker` form is "trample over planeswalkers". |
| `Vigilance` (754) | `K:Vigilance` | Generates `Mode$ AttackVigilance | ValidCard$ Card.Self` (attacking doesn't tap). |
| `Haste` (665) | `K:Haste` | Engine: `Card.isSick` (`sickness && !hasKeyword(HASTE)`). |
| `Flash` (634) | `K:Flash` | Engine: timing check in `SpellAbility`/`SpellAbilityProperty` (instant, has flash, or a `CastWithFlash` static). |
| `Menace` (415) | `K:Menace` | Engine: `StaticAbilityCantAttackBlock.getMinMaxBlocker` sets minimum blockers 2. |
| `Lifelink` (386) | `K:Lifelink` | Engine: `GameAction` grants life equal to damage dealt after the damage batch (not a trigger). |
| `First Strike` (382), `Double Strike` (125) | `K:First Strike`, `K:Double Strike` (note capital S) | Engine: combat damage steps (`Card.hasFirstStrike/hasDoubleStrike`). |
| `Deathtouch` (342) | `K:Deathtouch` | Engine: damage marking / lethal-damage computation in `Card`. |
| `Defender` (311) | `K:Defender` | Generates `Mode$ CantAttack | ValidCard$ Card.Self`; waived by `CanAttackDefender` statics. |
| `Indestructible` (111) | `K:Indestructible` | Engine: `Card.canBeDestroyed`; state-based lethal damage check skips it. |
| `Hexproof` (93) | `K:Hexproof`, `K:Hexproof:<valid>:<description>` | Generates `Mode$ CantTarget | ValidTarget$ Card.Self | Activator$ Opponent`; with a type it adds `ValidSource$ <valid>` (or `ValidSA$ <valid>` when the description contains "abilities") = "hexproof from X". Turned off by `IgnoreHexproof` statics. Player hexproof is the same keyword on a player. |
| `Shroud` (34) | `K:Shroud` | Generates `Mode$ CantTarget | ValidTarget$ Card.Self` (any controller). |
| `Protection` (57 + colour forms) | `K:Protection from <white|blue|black|red|green|colorless|each color|everything>` or `K:Protection:<valid>:<description>[:<…>:<attach exceptions>[:<SBA flag>]]` (`Protection:Card.MultiColor:multicolored`, `Protection:Artifact`, `Protection:Player.Opponent:…`) | Four generated traits (DEBT): damage - `Event$ DamageDone | Prevent$ True | ActiveZones$ Battlefield | ValidTarget$ Card.Self | ValidSource$ <valid>` (colour qualities get the `…Source` suffix so the *source's* colour is tested); blocking - `Mode$ CantBlockBy | ValidAttacker$ Creature.Self | ValidBlocker$ <valid>`; targeting - `Mode$ CantTarget | ValidTarget$ Card.Self | ValidSource$ <valid>`; attaching - `Mode$ CantAttach | Target$ Card.Self | ValidCard$ <valid>` (+ `Exceptions$` from the 4th field, `ExceptionSBA$` if a 5th exists). The valid string is wrapped as `Card.<v>,Emblem.<v>` for colour forms; `Player…` becomes `ControlledBy Player…`; `everything` = no filter. (`keyword/Protection.java`) |
| `Landwalk` (129) | `K:Landwalk:<LandType>` or `K:Landwalk:<valid>:<description>` (`Landwalk:Island`, `Landwalk:Land.Legendary:Legendary land`) | Generates `Mode$ CantBlockBy | ValidAttacker$ Creature.Self | ValidDefender$ Player.controls<valid>`; can be disabled by `IgnoreLandwalk`. |
| `Fear` (40) | `K:Fear` | `Mode$ CantBlockBy | ValidAttacker$ Creature.Self | ValidBlocker$ Creature.nonArtifact+nonBlack`. |
| `Intimidate` (23) | `K:Intimidate` | `… ValidBlocker$ Creature.nonArtifact+!SharesColorWith`. |
| `Shadow` (39) | `K:Shadow` | Two statics: can't be blocked by `Creature.withoutShadow`; and creatures without shadow can't be blocked by it. |
| `Horsemanship` (28) | `K:Horsemanship` | `… ValidBlocker$ Creature.withoutHorsemanship`. |
| `Infect` (45), `Wither` (27) | `K:Infect`, `K:Wither` | Engine: damage dealing in `Card`/`Player` (-1/-1 counters to creatures; infect also poison to players). |
| `Toxic` (43) | `K:Toxic:<n>` | Engine: `Player` adds n poison counters when dealt combat damage by the source. |
| `Changeling` (63) | `K:Changeling` | `Mode$ Continuous | EffectZone$ All | CharacteristicDefining$ True | AddAllCreatureTypes$ True`. |
| `Devoid` (132) | `K:Devoid` | `Mode$ Continuous | EffectZone$ All | CharacteristicDefining$ True | SetColor$ Colorless`. |
| `Split second` (21) | `K:Split second` | Engine: `MagicStack.isSplitSecondOnStack`. |
| `Banding` (24), `Phasing` (12) | bare | Engine (`combat/AttackingBand.java`, `phase/Untap.java`). |
| `Convoke` (106), `Delve` (28), `Improvise` (24) | bare | Engine: `CostAdjustment.adjust` lets the caster tap creatures / exile graveyard cards / tap artifacts to reduce the cost while paying. |
| `Ascend` (28) | `K:Ascend` | On permanents: `Mode$ Always | TriggerZones$ Battlefield | Static$ True | Blessing$ False | IsPresent$ Permanent.YouCtrl | PresentCompare$ GE10` -> sets the city's blessing (hard-coded ability). On spells: checked on resolution (UNVERIFIED where). |

### Auras, Equipment, vehicles

| Keyword | Syntax | Expansion |
|---|---|---|
| `Enchant` (1249) | `K:Enchant:<valid>[:<description>]` (`Enchant:Creature`, `Enchant:Creature.YouCtrl:creature you control`, `Enchant:Land`, `Enchant:Player`) | No generated ability. The `<valid>` (relative to the Aura and its controller) is the attach-legality test (`GameEntity.cantBeEnchantedByMsg`, `Card.canBeAttached`, state-based check in `GameAction`), and the Aura's cast spell is the card's own `A:SP$ Attach | ValidTgts$ …` line. A card without the keyword can't be attached as an Aura. |
| `Equip` (613) | `K:Equip:<cost>[:<valid>[:<description>[:<extra params>[:<extra description>]]]]` (`Equip:3`, `Equip:1:Creature.Legendary+YouCtrl:legendary creature`) | `AB$ Attach | Cost$ <cost> | ValidTgts$ <valid, default Creature.YouCtrl> | SorcerySpeed$ True` (+ the extra params, e.g. `AlternateCost$ …`). The Equipment's bonus is a separate `S:Mode$ Continuous | Affected$ Creature.EquippedBy` / `AffectedDefined$ Equipped` line. |
| `Crew` (184) | `K:Crew:<n>[:<extra params>]` | `AB$ Animate | Cost$ tapXType<Any/Creature.Other+withTotalPowerGE<n>> | Defined$ Self | Types$ Artifact,Creature` (until end of turn). |
| `Reconfigure` (21) | `K:Reconfigure:<cost>[:<alt cost>]` | Two sorcery-speed abilities: `AB$ Attach | ValidTgts$ Creature.YouCtrl+Other` and `AB$ Unattach | Defined$ Self | IsPresent$ Card.Self+AttachedTo Creature`. (The "isn't a creature while attached" part must be in the script as a static.) |
| `Bestow` (43) | `K:Bestow:<cost>[:<AI logic>]` | Alternative spell `SP$ Attach | ValidTgts$ Creature | Cost$ <cost>` flagged `AlternativeCost.Bestow`; becoming an Aura / reverting is engine code. |
| `Saddle` (32) | `K:Saddle:<n>` | `AB$ AlterAttribute | Cost$ tapXType<Any/Creature.Other+withTotalPowerGE<n>> | Attributes$ Saddle | Defined$ Self | SorcerySpeed$ True`. |
| `Station` (69) | `K:Station:<n>` | `AB$ PutCounter | Cost$ tapXType<1/Creature.Other> | Defined$ Self | CounterType$ CHARGE | CounterNum$ StationX | SorcerySpeed$ True` with `StationX = TappedCards$TapPowerValue`. The number's use (threshold display) is UNVERIFIED. |

### Enters-the-battlefield directives (pseudo-keywords)

| Keyword | Syntax | Expansion |
|---|---|---|
| `etbCounter` (469) | `K:etbCounter:<CounterType>:<amount>[:<condition|no Condition>[:<description|no desc>]]` | `makeEtbCounter`: replacement `Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplacementResult$ Updated [| <condition params>]` executing `DB$ PutCounter | Defined$ Self | CounterType$ <type> | ETB$ True | CounterNum$ <amount>`. `<amount>` is a number or an SVar name (`X`). The 3rd field is raw replacement params appended verbatim (`CheckSVar$ WasKicked`, `IsPresent$ …`), or the literal `no Condition`. A type written `EACH A & B` becomes `CounterTypes$`. Counter type names are the `CounterEnumType` names (`P1P1`, `M1M1`, `LOYALTY`, `CHARGE`, `TIME`, `LORE`, …). |
| `ETBReplacement` (405) | `K:ETBReplacement:<Layer>:<SVar>[:<Optional|Mandatory>[:<ActiveZones>[:<ValidCard>]]]` | `createETBReplacement`: `Event$ Moved | ValidCard$ <ValidCard, default Card.Self> | Destination$ Battlefield | ReplacementResult$ Updated [| Optional$ True] [| ActiveZones$ <zones>]` with layer `<Layer>` (`Other`, `Copy`, `Control`, `Transform`) executing the named SVar; the description is the SVar's `SpellDescription`. Used for "as CARDNAME enters, choose a colour / creature type / card name", "enters as a copy" (`Copy:…:Optional`), "you may have it enter with …", and kicker-dependent entering (`ETBReplacement:Other:<SVar>:Mandatory::Card.Self+kicked 1`). |

### Costs: alternative, additional, optional

| Keyword | Syntax | Expansion |
|---|---|---|
| `Kicker` (236) | `K:Kicker:<cost>[:<cost2>]` | No generated trait. `GameActionUtil.getOptionalCostValues` offers `OptionalCost.Kicker1` (and `Kicker2`) when casting; chosen costs are added to the spell. Scripts test it with `Condition$ Kicked` on effects, `Count$Kicked.<a>.<b>` (a if kicked else b), `Card.Self+kicked` / `kicked 1` / `kicked 2` in filters, `Spell.Kicked`, and `CheckSVar$ WasKicked` style SVars. |
| `Multikicker` (18) | `K:Multikicker:<cost>` | `GameActionUtil.addExtraKeywordCost`: the caster chooses a number, the cost is added that many times, stored as the keyword's optional amount; scripts read `Count$TimesKicked`. |
| `Buyback` (39) | `K:Buyback:<cost>` | Optional cost (`OptionalCost.Buyback`) + replacement `Event$ Moved | ValidCard$ Card.Self | Origin$ Stack | Destination$ Graveyard | Fizzle$ False | ValidStackSa$ Spell.Buyback` -> `DB$ ChangeZone | Defined$ Self | Origin$ Stack | Destination$ Hand`. |
| `Entwine` (32) | `K:Entwine:<cost>` | Optional cost (`OptionalCost.Entwine`); the card's `Charm` then takes all modes (in the Charm effect). |
| `Bargain` (22) | `K:Bargain` | Optional cost `Sac<1/Artifact;Enchantment;Card.token/…>`; tested by scripts with `Condition$ Bargain`, `Count$Bargained`, `Card.Self+bargained`, `Spell.Bargain` (corpus). |
| `AlternateAdditionalCost` (44) | `K:AlternateAdditionalCost:<costA>:<costB>` | `getAdditionalCostSpell`: "as an additional cost, do A or B" - one castable copy of the spell per alternative. |
| `Flashback` (216) | `K:Flashback[:<cost>[:<extra params>[:<extra description>]]]` (`Flashback:3 R`, `Flashback:1 G PayLife<3>`; bare `Flashback` = its mana cost) | `GameActionUtil.getGraveyardSpellByKeyword`: while in the graveyard, a copy of the spell with the mana cost replaced, flagged `AlternativeCost.Flashback`, castable only from the graveyard. Plus replacement `Event$ Moved | ValidCard$ Card.Self | Origin$ Stack | ExcludeDestination$ Exile | ValidStackSa$ Spell.Flashback+castKeyword` -> `DB$ ChangeZone | Defined$ Self | Origin$ Stack | Destination$ Exile`. (`Harmonize`, `Beam me up` share the code.) |
| `Escape` (33) | `K:Escape:<cost incl. ExileFromGrave<n/Card.Other/other>>` | Alternative cast from the graveyard (`AlternativeCost.Escape`). "Escapes with …" riders are separate script lines keyed on the `escaped` property. |
| `Madness` (61) | `K:Madness:<cost>` | Replacement `Event$ Moved | ActiveZones$ Hand | ValidCard$ Card.Self | Discard$ True` -> `DB$ ChangeZone | Hidden$ True | Origin$ All | Destination$ Exile | Defined$ ReplacedCard`; trigger `Mode$ Exiled | ValidCard$ Card.Self` (only for that madness exile) -> `DB$ Play | Defined$ Self | ValidSA$ Spell | PlayCost$ <cost> | Optional$ True | RememberPlayed$ True`, then if not cast `DB$ ChangeZone … Origin$ Exile | Destination$ Graveyard`. |
| `Foretell` (52) | `K:Foretell:<cost>` | Special action from hand (`{2}`, own turn unless `ForetellAnyTurn`): exile face down, mark foretold. Later-turn alternative cast from exile for `<cost>` (`AlternativeCost.Foretold`, `GameActionUtil.getAlternativeCosts`). |
| `Suspend` (71) | `K:Suspend:<n|X>:<cost>` (bare `K:Suspend` = "has suspend" for cards given time counters by effects) | (a) special action from hand when the card could be cast: pay cost, exile with n TIME counters; (b) `Mode$ Phase | Phase$ Upkeep | ValidPlayer$ You | TriggerZones$ Exile | IsPresent$ Card.Self+suspended | PresentZone$ Exile` -> `DB$ RemoveCounter | Defined$ Self | CounterType$ TIME | CounterNum$ 1`; (c) `Mode$ CounterRemoved | TriggerZones$ Exile | ValidCard$ Card.Self | CounterType$ TIME | NewCounterAmount$ 0` -> `DB$ Play | Defined$ Self | WithoutManaCost$ True | Optional$ True | RememberPlayed$ True` then haste (`DB$ Pump | Defined$ Remembered | KW$ Haste | PumpZone$ Stack | Duration$ UntilLoseControlOfHost`). |
| `Evoke` (36) | `K:Evoke:<cost>` | Alternative cost copy of the creature spell (`AlternativeCost.Evoke`) + trigger `Mode$ ChangesZone | Destination$ Battlefield | ValidCard$ Card.Self+evoked` -> `DB$ Sacrifice`. |
| `Overload` (28) | `K:Overload:<cost>` | Alternative-cost copy of the first spell ability in which the targeting is removed and `Defined$ Valid <the former ValidTgts>` is set ("each" instead of "target"). |
| `Dash` (22) | `K:Dash:<cost>` | Alternative cost (`AlternativeCost.Dash`); static `Mode$ Continuous | Affected$ Card.Self+dashed+castKeyword | AddKeyword$ Haste`; on resolution a delayed "return to hand at the next end step" trigger (`PermanentEffect`). |
| `Warp` (37) | `K:Warp:<cost>` | Alternative cost from hand (`AlternativeCost.Warp`); on resolution a delayed "exile at the next end step" trigger; the exiled card may be cast later (engine, `Card.isWarped`). |
| `Plot` (33) | `K:Plot:<cost>` | Sorcery-speed special action from hand: pay, exile plotted; on a later turn cast it without paying its mana cost at sorcery speed (`AlternativeCost.Plotted`). |
| `Disturb` (32) | `K:Disturb:<cost>` | Alternative cast of the **back face's** spell from the graveyard (`AlternativeCost.Disturb`, zone Graveyard). The "if it would be put into a graveyard, exile it instead" rider is a script line on the back face. |
| `Sneak` (27) | `K:Sneak:<cost>[:…]` | Alternative cost `<cost> Return<1/Creature.attacking+unblocked>`; a creature cast this way enters attacking in the returned creature's place. |
| `Affinity` (76) | `K:Affinity:<Type>` or `K:Affinity:<valid>:<singular description>` (specials `Affinity`, `Outlaw`, `Historic`) | `Mode$ ReduceCost | ValidCard$ Card.Self | Type$ Spell | Amount$ AffinityX | EffectZone$ All`, `AffinityX = Count$Valid <type>.YouCtrl`. |
| `Strive` (20) | `K:Strive:<cost>` | `Mode$ RaiseCost | ValidCard$ Card.Self | Type$ Spell | Amount$ Strive | Cost$ <cost> | EffectZone$ All` (once per target beyond the first). |
| `Spree` (21) | `K:Spree` | `Mode$ RaiseCost | ValidCard$ Card.Self | Type$ Spell | Amount$ Spree | EffectZone$ All`; each mode's price is its `ModeCost$`. |
| `Replicate` (20) | `K:Replicate:<cost>` | Extra cost chosen N times (`addExtraKeywordCost`) + trigger `Mode$ SpellCast | ValidCard$ Card.Self | CheckSVar$ ReplicateAmount` -> `DB$ CopySpellAbility | Defined$ TriggeredSpellAbility | Amount$ ReplicateAmount | MayChooseTarget$ True` (`Count$OptionalKeywordAmount`). |
| `Offspring` (21) | `K:Offspring:<cost>` | Optional extra cost + `Mode$ ChangesZone | Destination$ Battlefield | ValidCard$ Card.Self | CheckSVar$ Offspring` -> `DB$ CopyPermanent | Defined$ TriggeredCardLKICopy | SetPower$ 1 | SetToughness$ 1`. |
| `Prototype` (21) | `K:Prototype:<cost>:<power>:<toughness>` | Alternative spell copy with `SetManaCost`, `SetColorByManaCost`, `SetPower`, `SetToughness`, `Prototype$ True`. |
| `Splice` (30) | `K:Splice:<type>:<cost>` (`Splice:Arcane:1 W`) | Engine: `AbilityUtils` adds the card's effect to a spell of that type on cast (not read in detail - UNVERIFIED). |

### Activated abilities from hand / graveyard / battlefield

| Keyword | Syntax | Expansion |
|---|---|---|
| `Cycling` (304) | `K:Cycling:<cost>` | `AB$ Draw | Cost$ <cost> Discard<1/CARDNAME> | ActivationZone$ Hand`. Fires the `Cycled` trigger event; the discard is flagged `Cycling` for replacements. |
| `TypeCycling` (100) | `K:TypeCycling:<type>:<cost>[:<description>]` (`TypeCycling:Basic:2`, `TypeCycling:Plains:2`) | `AB$ ChangeZone | Cost$ <cost> Discard<1/CARDNAME> | ActivationZone$ Hand | Origin$ Library | Destination$ Hand | ChangeType$ <type>`. |
| `Unearth` (58) | `K:Unearth:<cost>` | `AB$ ChangeZone | Cost$ <cost> | Defined$ Self | Origin$ Graveyard | Destination$ Battlefield | SorcerySpeed$ True | ActivationZone$ Graveyard`; on resolution `ChangeZoneEffect` adds an effect giving haste, "if it would leave the battlefield, exile it instead", and a delayed exile at the next end step. |
| `Ninjutsu` (38) | `K:Ninjutsu:<cost>[:Commander]` | `AB$ ChangeZone | Cost$ <cost> Return<1/Creature.attacking+unblocked/unblocked attacker> | ActivationZone$ Hand | Origin$ Hand | Destination$ Battlefield | Defined$ Self` (enters tapped and attacking - handled in the effect via the returned creature); `:Commander` adds the same from the command zone. |
| `Morph` (153), `Megamorph` (31), `Disguise` (47) | `K:Morph:<cost>`, `K:Megamorph:<cost>`, `K:Disguise:<cost>[:<reduce SVar>]` | (a) a face-down spell for `{3}` (`abilityCastFaceDown`: 2/2 creature; Disguise adds `Ward:2` while face down); (b) `ST$ SetState | Cost$ <cost> | Mode$ TurnFaceUp | IsPresent$ Card.Self+faceDown` (a special action, not using the stack as an ability - `ST$` static ability). Megamorph adds `Event$ TurnFaceUp | ValidCard$ Card.Self | ValidCause$ SpellAbility.SameKeyword` -> `DB$ PutCounter | CounterType$ P1P1 | CounterNum$ 1`. |
| `Mutate` (34) | `K:Mutate:<cost>` | `SP$ Mutate | Cost$ <cost> | ValidTgts$ Creature.sharesOwnerWith+nonHuman` (`AlternativeCost.Mutate`). |
| `Level up` (26) | `K:Level up:<cost>` | `AB$ PutCounter | Cost$ <cost> | CounterType$ LEVEL | SorcerySpeed$ True` (level bands are separate conditional statics in the script). |
| `Encore` (26) | `K:Encore:<cost>[:<extra params>]` | `AB$ CopyPermanent | Cost$ <cost> ExileFromGrave<1/CARDNAME> | ActivationZone$ Graveyard | SorcerySpeed$ True | Defined$ Self | PumpKeywords$ Haste | ForEach$ Opponent | AtEOT$ Sacrifice` + a `MustAttack` effect per token. |
| `Craft` (24) | `K:Craft:<cost incl. ExileCtrlOrGrave<…>>[:<desc>:<reminder>]` | `AB$ ChangeZone | Cost$ Exile<1/CARDNAME> <cost> | Origin$ Exile | Destination$ Battlefield | Transformed$ True | Defined$ CorrectedSelf | SorcerySpeed$ True`. |

### Triggered-ability keywords

| Keyword | Syntax | Generated trigger -> effect |
|---|---|---|
| `Prowess` (98) | `K:Prowess` | `Mode$ SpellCast | ValidCard$ Card.nonCreature | ValidActivatingPlayer$ You | TriggerZones$ Battlefield` -> `DB$ Pump | Defined$ Self | NumAtt$ +1 | NumDef$ +1`. |
| `Ward` (209) | `K:Ward:<cost>` (`Ward:2`, `Ward:PayLife<3>`, `Ward:Discard<1/Random>`), several costs `K:Ward:<c1>:<c2>` = payer chooses one | `Mode$ BecomesTarget | ValidSource$ SpellAbility.OppCtrl | ValidTarget$ Card.Self | TriggerZones$ Battlefield` -> `DB$ Counter | Defined$ TriggeredSourceSA | UnlessCost$ <cost> | UnlessPayer$ TriggeredSourceSAController` (multi-cost: a `GenericChoice` among such counters with a plain counter as fallback). |
| `Chapter` (241) | `K:Chapter:<N>:<SVar1>,<SVar2>,…` (exactly N SVar names; repeat a name for "I, II -") | For chapter i: `Mode$ CounterAdded | ValidCard$ Card.Self | TriggerZones$ Battlefield | Chapter$ i | CounterType$ LORE | CounterAmount$ EQi` executing the i-th SVar. Lore counters on entering / after the draw step and the Saga sacrifice are engine code (`PhaseHandler`, `GameAction` state-based actions). `Read ahead` (10) adds a choose-starting-chapter ETB replacement and a `DisableTriggers` static. |
| `Cumulative upkeep` (79) | `K:Cumulative upkeep:<cost>[:<description>]` | `Mode$ Phase | Phase$ Upkeep | ValidPlayer$ You | TriggerZones$ Battlefield | IsPresent$ Card.Self` -> `DB$ Sacrifice | SacValid$ Self | CumulativeUpkeep$ <cost>` (the Sacrifice effect adds an age counter and asks for payment). |
| `Echo` (51) | `K:Echo:<cost>` | `Mode$ Phase | Phase$ Upkeep | ValidPlayer$ You | TriggerZones$ Battlefield | IsPresent$ Card.Self+cameUnderControlSinceLastUpkeep` -> `DB$ Sacrifice | SacValid$ Self | Echo$ <cost>`. |
| `Partner with` (52) | `K:Partner with:<Name>[:<short name>]` | ETB trigger -> `DB$ ChangeZone | ValidTgts$ Player | Origin$ Library | Destination$ Hand | ChangeType$ Card.named<Name> | Optional$ True` (target player may search). Also a commander deck-building rule. |
| `Storm` (34) | `K:Storm` | `Mode$ SpellCast | ValidCard$ Card.Self | TriggerZones$ Stack` -> `DB$ CopySpellAbility | Defined$ TriggeredSpellAbility | Amount$ StormCount | MayChooseTarget$ True`, `StormCount = TriggerCount$CurrentStormCount/Minus.1`. |
| `Cascade` (37) | `K:Cascade` | `Mode$ SpellCast | ValidCard$ Card.Self | TriggerZones$ Stack` -> `DB$ DigUntil | Valid$ Card.nonLand+cmcLTCascadeX | FoundDestination$ Exile | RevealedDestination$ Exile …` -> `DB$ Play | Defined$ Imprinted | WithoutManaCost$ True | Optional$ True | ValidSA$ Spell.cmcLTCascadeX` -> rest to library bottom in random order (`CascadeX = Count$CardManaCost`). |
| `Bushido` (36) | `K:Bushido:<n>` | Two triggers (`Mode$ Blocks | ValidCard$ Card.Self` and `Mode$ AttackerBlocked | ValidCard$ Card.Self`) -> `DB$ Pump | Defined$ Self | NumAtt$ n | NumDef$ n`. |
| `Exalted` (35) | `K:Exalted` | `Mode$ Attacks | ValidCard$ Creature.YouCtrl | Alone$ True | TriggerZones$ Battlefield` -> `DB$ Pump | Defined$ TriggeredAttackerLKICopy | NumAtt$ +1 | NumDef$ +1`. |
| `Flanking` (29) | `K:Flanking` | `Mode$ AttackerBlockedByCreature | ValidCard$ Card.Self | ValidBlocker$ Creature.withoutFlanking` -> `DB$ Pump | Defined$ TriggeredBlockerLKICopy | NumAtt$ -1 | NumDef$ -1`. |
| `Rebound` (35) | `K:Rebound` | Replacement `Event$ Moved | ValidLKI$ Card.Self+wasCastFromHand+YouOwn+YouCtrl | Origin$ Stack | Destination$ Graveyard | Fizzle$ False` -> exile + `DelayedTrigger | Mode$ Phase | Phase$ Upkeep | ValidPlayer$ You | OptionalDecider$ You` -> `DB$ Play | Defined$ DelayTriggerRememberedLKI | WithoutManaCost$ True`. |
| `Daybound` (36) / `Nightbound` (36) | bare | Static-trigger pairs (`Mode$ Always | Static$ True | DayTime$ …`) to set day/night and transform, an ETB replacement (`Layer$ Transform`, enters transformed if night), and `Mode$ CantTransform … ExceptCause$ SpellAbility.Daybound`. |
| `Persist` (24) / `Undying` (22) | bare | `Mode$ ChangesZone | Origin$ Battlefield | Destination$ Graveyard | ValidCard$ Card.Self+counters_EQ0_M1M1` (`…_P1P1` for undying) `| TriggerZones$ Battlefield` -> `DB$ ChangeZone | Defined$ TriggeredNewCardLKICopy | Origin$ Graveyard | Destination$ Battlefield | WithCountersType$ M1M1` (`P1P1`). |
| `Modular` (24) | `K:Modular:<n>` (or `Modular:Sunburst`) | ETB counters via `etbCounter:P1P1:<n>` + `Mode$ ChangesZone | ValidCard$ Card.Self | Origin$ Battlefield | Destination$ Graveyard | OptionalDecider$ TriggeredCardController` -> `DB$ PutCounter | ValidTgts$ Artifact.Creature | CounterType$ P1P1 | CounterNum$ ModularX` (`TriggeredCard$CardCounters.P1P1`). |
| `Soulshift` (26) | `K:Soulshift:<n>` | Dies trigger, `OptionalDecider$ You` -> `DB$ ChangeZone | Origin$ Graveyard | Destination$ Hand | ValidTgts$ Spirit.YouOwn+cmcLE<n>`. |
| `Soulbond` (25) | bare | Two ETB triggers (self / another creature) with `IsPresent$ …+!Paired` -> `DB$ Bond`. |
| `Exploit` (25) | bare | ETB -> `DB$ Sacrifice | SacValid$ Creature | Optional$ True` (fires `Exploited`). |
| `Evolve` (23) | bare | `Mode$ ChangesZone | Destination$ Battlefield | ValidCard$ Creature.YouCtrl+Other | TriggerZones$ Battlefield` -> `DB$ PutCounter | Defined$ Self | CounterType$ P1P1 | CounterNum$ 1`; the P/T comparison is hard-coded in `Trigger.meetsRequirementsOnTriggeredObjects` (checked on trigger and on resolution). |
| `Myriad` (23) | bare | `Mode$ Attacks | ValidCard$ Card.Self` -> `DB$ CopyPermanent | Defined$ Self | TokenTapped$ True | OptionalForEach$ True | TokenAttacking$ … | ForEach$ OppNonDefendingPlayer | AtEOT$ ExileCombat`. |
| `Renown` (20) | `K:Renown:<n>` | `Mode$ DamageDone | ValidSource$ Card.Self | ValidTarget$ Player | CombatDamage$ True | IsPresent$ Card.Self+!IsRenowned` -> `DB$ PutCounter | Defined$ Self | CounterType$ P1P1 | CounterNum$ n` (becomes renowned). |
| `Vanishing` (21) | `K:Vanishing:<n>` (bare = no counters) | `etbCounter:TIME:<n>` + upkeep trigger removing a TIME counter (`IsPresent$ Card.Self+counters_GE1_TIME`) + `Mode$ CounterRemoved | ValidCard$ Card.Self | NewCounterAmount$ 0 | CounterType$ TIME` -> `DB$ Sacrifice | SacValid$ Self`. |
| `Backup` (25) | `K:Backup:<n>:<SVar>` | ETB -> `DB$ PutCounter | ValidTgts$ Creature | CounterNum$ n | CounterType$ P1P1` then the SVar's effect with `ConditionDefined$ Targeted | ConditionPresent$ Card.Other | Defined$ Targeted`. |
| `Firebending` (26) | `K:Firebending:<n|X>[:<text>]` | `Mode$ Attacks | ValidCard$ Card.Self` -> `DB$ Mana | Defined$ You | CombatMana$ True | Produced$ R | Amount$ n`. |
| `Gift` (27) | `K:Gift` | Optional cost `PromiseGift` + (permanents) `Mode$ ChangesZone | Destination$ Battlefield | ValidCard$ Card.Self+PromisedGift` trigger. The gift itself is the script's `SVar:GiftAbility:DB$ …` (attached as the additional ability `GiftAbility`, run for the promised opponent - `AbilityUtils`/`SpellAbilityEffect`); scripts test `Card.PromisedGift` / `ConditionPresent$ Card.PromisedGift`. |

### Enters-with / as-enters keywords (replacement based)

| Keyword | Syntax | Expansion |
|---|---|---|
| `Bloodthirst` (23) | `K:Bloodthirst:<n|X>` | `etbCounter:P1P1:<n>:Bloodthirst$ True:<desc>` (condition key `Bloodthirst`; X = `Count$BloodthirstAmount`). |
| `Devour` (23) | `K:Devour:<n>[:<Type>[:extra]]` | `Event$ Moved | ValidCard$ Card.Self | Destination$ Battlefield | ReplacementResult$ Updated` -> `DB$ Sacrifice | Defined$ You | Amount$ DevourSacX | Optional$ True | SacValid$ <Type>.Other | RememberSacrificed$ True` -> `DB$ PutCounter | ETB$ True | Defined$ Self | CounterType$ P1P1 | CounterNum$ DevourX` (`Count$RememberedSize/Times.<n>`). |
| `Aftermath` (27) | bare (on the right half of a split card) | The half's spell is restricted to the graveyard + `Event$ Moved | ValidCard$ Card.Self | Origin$ Stack | ExcludeDestination$ Exile | ValidStackSa$ Spell.Aftermath` -> exile. |

### Deck-building, commander and miscellaneous directives

| Keyword | Notes |
|---|---|
| `Partner` (70; `K:Partner`, `K:Partner:<group>`), `Choose a Background` (32), `Doctor's companion` (27) | Commander pairing rules (deck validation; `keyword/Partner.java`). No in-game traits. |
| `Start your engines` (46) | Engine: `GameAction` state check sets the controller's speed to 1 if it is 0. |
| `Starting intensity` (20; `K:Starting intensity:<n>`) | Alchemy: initial intensity value read by `Card.getIntensity`. |
| `Class` (35; `K:Class:<level>:<cost>:<params>`) | For each higher level: `AB$ ClassLevelUp | Cost$ <cost> | ClassLevel$ EQ<level-1> | SorcerySpeed$ True` and `Mode$ Continuous | Affected$ Card.Self | ClassLevel$ <level> | <params>` (the params are `AddTrigger$`/`AddStaticAbility$`/`AddReplacementEffect$` SVar lists). |
| `MayEffectFromOpeningHand` (27; `K:MayEffectFromOpeningHand:<SVar>[:!PlayFirst]`) | Leyline/Chancellor: before the first turn each player may run the named SVar for such cards in hand (`GameAction`); `!PlayFirst` excludes the starting player. `MayEffectFromOpeningDeck:<SVar>` (3) is the conspiracy/companion-like variant (`Player`). |
| `You may choose not to untap CARDNAME during your untap step.` (46) | Sentence keyword checked in `phase/Untap.java` (`optionalUntap`). |

## 4.3 Keywords used by fewer than 20 cards (brief)

Trigger-based (`addTriggerAbility`): `Afflict:<n>` (becomes blocked -> defending player loses n);
`Afterlife:<n>` (dies -> n Spirit tokens); `Annihilator:<n>` (`Mode$ Attacks` -> `DB$ Sacrifice | Defined$ TriggeredDefendingPlayer | SacValid$ Permanent | Amount$ n`);
`Battle cry`; `Casualty:<n>` (extra cost `Sac<1/Creature.powerGE n>` + copy trigger);
`Champion:<Types>` (ETB exile-or-sacrifice + leaves-battlefield return); `Conspire`;
`Decayed` (can't block + sacrifice at end of combat); `Demonstrate`; `Dethrone`; `Double team`;
`Enlist` (`Mode$ OptionalAttackCost | Cost$ Enlist<…> | Trigger$ …`); `Extort`
(`Mode$ SpellCast | ValidActivatingPlayer$ You` -> `AB$ LoseLife | Cost$ WB | Defined$ Player.Opponent | LifeAmount$ 1` + gain);
`Fabricate:<n>`; `Fading:<n>` (`etbCounter:FADE:<n>` + upkeep remove-or-sacrifice);
`For Mirrodin`, `Living Weapon`, `Job select` (ETB token + attach); `Graft:<n>`
(`etbCounter:P1P1:<n>` + optional move-counter trigger); `Gravestorm`; `Haunt:<SVar>`;
`Hideaway:<n>`; `Increment`; `Ingest`; `Impending:<n>:<cost>`; `Melee`; `Mentor`;
`Miracle:<cost>` (`Mode$ Drawn | ValidCard$ Card.Self | Number$ 1 | Static$ True` reveal -> immediate trigger to cast);
`Mobilize:<n>`; `Poisonous:<n>`; `Provoke`; `Rampage:<n>`; `Ravenous`; `Recover:<cost>`;
`Ripple:<n>`; `Squad:<cost>`; `Storied`; `Training`; `Tribute:<n>`; `MayFlashSac`;
`Visit:<SVar>`, `Prize:<SVar>`, `Dungeon…` (Un-set / dungeon rooms).

Replacement-based (`addReplacementEffect`): `Absorb:<n>` (`Event$ DamageDone | ValidTarget$ Card.Self | PreventionEffect$ True` -> `DB$ ReplaceDamage | Amount$ n`);
`Amplify:<n>:<types>`; `Compleated`; `Dredge:<n>` (`Event$ Draw | ActiveZones$ Graveyard | ValidPlayer$ You | Optional$ True | CheckSVar$ DredgeCheckLib | SVarCompare$ GE<n>` -> mill n, return to hand);
`Jump-start` (cast from graveyard with `Discard<1/Card>` + exile replacement); `Read ahead`;
`Reflect:<cost>`; `Riot` (ETB replacement: haste unless a +1/+1 counter); `Sunburst`;
`Umbra armor` (`Event$ Destroy | ValidCard$ Card.EnchantedBy` -> heal + destroy the Aura);
`Unleash` (optional ETB +1/+1 counter + `CantBlock` while it has one);
sentence keywords `Prevent all [combat] damage that would be dealt [to and dealt by | by | to] CARDNAME.`
(expand to `Event$ DamageDone | Prevent$ True [| IsCombat$ True]` with `ValidSource$ Card.Self` and/or `ValidTarget$ Card.Self`).

Spell/ability-based (`addSpellAbility`, `GameActionUtil`): `Aura swap:<cost>`; `Awaken:<n>:<cost>`;
`Blitz:<cost>` (alt cost; haste + "when it dies draw" static; sacrifice at end step);
`Embalm:<cost>` / `Eternalize:<cost>` (`AB$ CopyPermanent | Cost$ <cost> ExileFromGrave<1/CARDNAME> | ActivationZone$ Graveyard | SorcerySpeed$ True | …`);
`Emerge:<cost>[:<type>]`; `Epic`; `Fortify:<cost>`; `Freerunning:<cost>`; `Fuse`;
`Harmonize:<cost>`; `Beam me up:<cost>`; `Mayhem:<cost>`; `More Than Meets the Eye:<cost>`;
`Outlast:<cost>` (`AB$ PutCounter | Cost$ <cost> T | CounterType$ P1P1 | SorcerySpeed$ True`);
`Paradigm`; `Prowl:<cost>`; `Reinforce:<n>:<cost>`; `Retrace` (cast from graveyard with `Discard<1/Land>`);
`Scavenge:<cost>`; `Specialize:<cost>`; `Spectacle:<cost>`; `Surge:<cost>`; `Transfigure:<cost>`;
`Transmute:<cost>` (`AB$ ChangeZone | Cost$ <cost> Discard<1/CARDNAME> | ActivationZone$ Hand | Origin$ Library | Destination$ Hand | ChangeType$ Card.cmcEQTransmuteX`);
`Web-slinging:<cost>`; `Teamwork:<n>`; `MayFlashCost:<cost>` (optional cost to cast as though it had flash);
`Offering:<type>`; `Assist`; `Escalate:<cost>` / `Tiered` / `Undaunted` (RaiseCost/ReduceCost statics);
`Cipher`; `Skulk` (`CantBlockBy … ValidBlocker$ Creature.powerGTX`); `Living metal`; `Prismatic`.

Engine-only or deck-building: `Companion:<restriction>:<text>`, `Hidden agenda`, `Double agenda`,
`DeckLimit:<n>:<text>`, `A deck can have any number of cards named CARDNAME.`,
`CARDNAME can be your commander.`, ante/Conspiracy sentences, `StickerSheet:…`, `Space sculptor`.

## 4.4 Sentence keywords and other script directives

These are literal strings (not in the `Keyword` enum) tested with `hasKeyword("…")`; they are
usually granted with `AddHiddenKeyword$`/`KW$` but also appear as `K:` lines.

| String | Where checked / meaning |
|---|---|
| `CARDNAME can't attack.`, `CARDNAME can't attack or block.` | `StaticAbilityCantAttackBlock.cantAttack` (block side: `combat/CombatUtil.java`). |
| `CARDNAME can't block.` | Block legality (`combat/CombatUtil.java`). The scripted equivalent is `S:Mode$ CantBlock | ValidCard$ Card.Self`. |
| `CARDNAME can't attack alone.`, `CARDNAME can't block alone.`, `CARDNAME can't attack or block alone.`, `CARDNAME can only attack alone.` | Attack/block validation (`combat/AttackRestriction.java`, `combat/CombatUtil.java`). |
| `CARDNAME can't attack unless …` / `can't block unless …` (several fixed sentences) | Hard-coded attack/block requirements, one card each. |
| `CARDNAME must be blocked if able.`, `All creatures able to block CARDNAME do so.`, `CARDNAME must be blocked by exactly one creature if able.`, `… by two or more creatures if able.`, `MustBeBlockedByAll:<valid>:<text>` | Block requirements (`CombatUtil`/AI block logic). |
| `Prevent all (combat) damage that would be dealt to/by CARDNAME.` | Expanded to replacements (4.3). |
| `You may choose not to untap CARDNAME during your untap step.`, `This card doesn't untap during your next untap step.` | `phase/Untap.java`. |
| `CARDNAME's power and toughness are switched` | P/T computation in `Card` (layer 7d substitute). |
| `If CARDNAME is unblocked, you may have it assign its combat damage to a creature defending player controls.`, `You may assign CARDNAME's combat damage divided as you choose among …` | Combat damage assignment. |
| `Damage that would be dealt to CARDNAME can't be redirected.` | `ReplaceDamage.canReplace`. |
| `CARDNAME can't be destroyed by lethal damage unless lethal damage dealt by a single source is marked on it.` | State-based lethal damage check. |
| Player keywords: `Shaman's Trance`, `Piracy`, `CantSearchLibrary`, `LimitSearchLibrary`, `You assign combat damage of each creature attacking you.`, … | Player-level rule switches. |
| `ETBReplacement:…`, `etbCounter:…`, `AlternateAdditionalCost:…`, `MayEffectFromOpeningHand:…`, `MayEffectFromOpeningDeck:…`, `MayFlashCost:…`, `MayFlashSac`, `Chapter:…`, `Class:…`, `Visit:…`, `Prize:…`, `StickerSheet:…`, `DeckLimit:…` | Script directives documented above. |

---

# Appendix — the non-obvious behaviours a translator must know

1. **`TriggerZones` / `ActiveZones` absent means "any zone"; `EffectZone` absent means
   "battlefield only".** Triggers and replacement effects default to working everywhere
   (except the automatic battlefield/destination defaults `ChangesZone` gives itself),
   statics default to the battlefield. Self cost modifiers and CDAs need `EffectZone$ All`.
2. **Every generic condition key on a trigger is an intervening "if"**: it is checked when
   the event happens and again on resolution (`WrappedAbility.resolve`), unless
   `NoResolvingCheck$`; `ValidXxx` filters in the mode are not rechecked. `Mode$ Always`
   (state triggers) is the opposite: not rechecked unless `ResolvingCheck$`.
3. **`ChangesZone` chooses which object `ValidCard` sees from the zones**: LKI for
   `Origin$ Battlefield` (and graveyard-leaving), the battlefield object for
   `Destination$ Battlefield`. `TriggeredCard` is likewise the LKI copy for leaves-battlefield
   triggers and `TriggeredNewCard` the new object; plain `Defined$ TriggeredCard` re-resolves
   to the current game object while `…LKICopy` does not.
4. **`Static$ True` triggers are not triggered abilities**: no stack, no Panharmonicon/Torpor
   Orb, immediate. They implement mana-ability triggers, "as … happens" bookkeeping and
   state watchers.
5. **Optionality has three spellings**: `OptionalDecider$` on the trigger (whole ability),
   an executed `AB$` with a real `Cost$` (implicitly optional - "you may pay"), and
   `Optional$ True` on individual effects / on replacement effects.
6. **Replacement outcome is decided by `ReplacementResult$` and the helper effect, not by the
   event**: without `ReplacementResult$ Updated` the original event is dropped ("instead");
   with it the event proceeds after modification ("enters tapped / with counters / as a copy").
   `ReplaceEffect`, `ReplaceDamage`, `ReplaceToken`, `ReplaceCounter`, `ReplaceMana` set
   `Updated` themselves.
7. **"Can't" is modelled three ways**: `Layer$ CantHappen` replacement effects (untap,
   counter, lose/win the game), dedicated static modes (`CantAttack`, `CantBlock`, `CantTarget`,
   `CantBeCast`, `CantGainLife`, …), and hidden sentence keywords (`CARDNAME can't block.`).
   The same rules text can appear in any of the three.
8. **Keywords are macros.** A `K:` line (or `AddKeyword$`) silently expands into triggers,
   replacements, statics and abilities with fixed script text (Part 4); evasion and
   protection are `CantBlockBy`/`CantTarget`/`CantAttach` statics plus a damage-prevention
   replacement, ward is a `BecomesTarget` trigger, sagas are `CounterAdded` triggers.
   Unknown `K:` strings are legal and inert unless some engine code tests that exact string.
9. **Continuous statics get their layer from the keys present**, a multi-key static applies
   in several layers with one locked-in affected set, and a handful of rules keys
   (`SetMaxHandSize`, `MayLookAt`, …) only work when the static has no other-layer key.
   Per-object values are signalled by an SVar *name* containing `Affected`.
10. **Costs are modified only by statics found on the battlefield, stack, command zone and
    the spell itself**, raise before reduce, `SetCost` last; `Relative$ True` switches the
    amount's evaluation context to the spell being cast. Convoke/delve/improvise/affinity etc.
    are keyword-driven parts of the same routine, not scripted effects.
11. **Batched vs single events**: many modes exist in per-object and "one or more" forms
    (`DamageDone` / `DamageDoneOnce` / `DamageDealtOnce` / `DamageAll`; `ChangesZone` /
    `ChangesZoneAll`; `CounterAdded` (per counter!) / `CounterAddedOnce`; `Discarded` /
    `DiscardedAll`; `Sacrificed` / `SacrificedOnce`; `Attacks` / `AttackersDeclared`). The
    form chosen encodes how many times the ability triggers.
12. **`Mode$` may list several static modes**, the filter keys differ per mode family
    (`ValidCard` vs `ValidCreature` vs `ValidAttacker`/`ValidBlocker` vs `ValidTarget`), and
    unknown keys are ignored without error - a mistyped key silently widens the effect.

## Verification status

Read in full: `Trigger.java`, `TriggerHandler.java`, `TriggerWaiting.java`, `WrappedAbility.resolve`,
`CardTraitBase.meetsCommonRequirements`, all trigger classes cited in 1.7.1–1.7.39 and most of
1.7.40, `StaticAbility.java`, `StaticAbilityContinuous.java`, the layer loop in
`GameAction.checkStaticAbilities`, `CostAdjustment.java`, the static classes cited in 2.3–2.6,
`ReplacementEffect.java`, the first half of `ReplacementHandler.java`, the replacement classes
cited in 3.4 and the `Replace*Effect` helpers, `Keyword.java`, the keyword parameter classes,
and `CardFactoryUtil.addStaticAbility`. `addTriggerAbility`, `addReplacementEffect` and
`addSpellAbility` were read through an extraction of their script strings (full text for
the first third of `addTriggerAbility` and for the blocks quoted in 4.2).

Not verified (marked UNVERIFIED in place or simply not read): the selector grammar; effect
(`DB$`/`AB$`) parameters other than those quoted; the damage batch algorithm in the second
half of `ReplacementHandler`; whether `Attacks` fires for creatures put onto the battlefield
attacking; `RemoveColor$`; land-type mana-ability bookkeeping; Splice, Station's number,
Ascend on spells; Planechase/Archenemy/Un-set/Alchemy
modes beyond their parameter lists; all AI code.
