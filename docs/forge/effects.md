# Forge effect APIs — implementation reference

Reference for translating Forge card scripts into apnap's card format. It describes what each
Forge *effect API* (`SP$` / `AB$` / `DB$` / `ST$ <Api>`) does and every parameter its Java class reads.

- Source: Forge checkout at commit `3d5c95bcaa` (2026-11-10), `forge-game/src/main/java/forge/game/ability/`.
  Every parameter listed for a fully covered API was confirmed by reading the effect class (and its
  base classes). Anything not confirmed is marked **UNVERIFIED**.
- Forge is GPL v3. This file describes behaviour in our own words; it contains parameter names and
  short script lines only. Do not copy Forge code into apnap (CLAUDE.md, invariant 13).
- Forge's behaviour is *not* authoritative for the rules. Where Forge and the Comprehensive Rules
  differ, the CR wins; this document tells us what a script *means*, not what the engine must do.
- **Counts.** "cards" is the number from `forge_feature_frequency.md` (real, non-funny cards).
  "lines" and the per-parameter counts are ability lines (`A:` and `SVar:` lines) over all 34,465
  scripts, measured with `scratchpad/agent-effects/corpus.py`; a count next to a parameter is the
  number of lines of that API that set the key. Keys used by fewer than ~3 lines are listed without
  a count or as "rare".
- The generic grammar — `Defined$` values, `Valid…` filters, targeting keys, costs, conditions /
  restrictions, SVar expressions — is documented elsewhere and only referenced here.

## 0. Framework behaviour shared by every API

These are implemented in `AbilityUtils.resolve*`, `SpellAbilityEffect` and `AbilityFactory`, not in
the individual effect classes. A translator must handle them once, for all APIs.

### 0.1 Ability chain and resolution order

An ability is a linked list: the root (`SP$`/`AB$`, or the `DB$` named by a trigger's `Execute$` or
a replacement's `ReplaceWith$`) followed by `SubAbility$ <SVar>` links. Resolution of one link:

1. If the link's **conditions** are met (`Condition…` keys, `sa.metConditions()`):
   - no `UnlessCost$` → run the effect;
   - `UnlessCost$` present → the unless-cost procedure (0.3), which may or may not run the effect.
2. Regardless of whether the conditions were met, continue with the `SubAbility$` link (static
   abilities are re-evaluated in between — Forge calls `checkStaticAbilities` before each sub-ability).
   A failed condition therefore skips **only that link**, never the rest of the chain. The only ways a
   link suppresses its sub-abilities are `UnlessResolveSubs$` and APIs that run sub-abilities
   themselves (Branch, Charm, GenericChoice, Repeat, RepeatEach, FlipCoin, Clash …).
3. A sub-ability without its own `ValidTgts$` does not "use targeting"; it reaches the parent's
   targets through `Defined$ Targeted` / `ParentTarget` etc.

`Cost$` is mandatory for `AB$`; for `SP$` it defaults to the card's mana cost; `DB$` has no cost.
`ST$` is an effect resolved without using the stack. The API name after `SP$`/`AB$`/`DB$`/`ST$` is
matched case-insensitively against `ApiType`; an unknown name is a load error.

Extra sub-ability slots (parsed for every API when the key is present; the value is an SVar name):
`WinSubAbility`, `OtherwiseSubAbility` (Clash), `BidSubAbility`, `ChooseNumberSubAbility`,
`Lowest`, `Highest`, `NotLowest`, `GuessCorrect`, `GuessWrong`, `MatchedAbility`,
`UnmatchedAbility` (ChooseNumber), `HeadsSubAbility`, `TailsSubAbility`, `LoseSubAbility`
(FlipCoin), `TrueSubAbility`, `FalseSubAbility` (Branch), `ChosenPile`, `UnchosenPile` (TwoPiles /
MultiplePiles), `RepeatSubAbility` (Repeat, RepeatEach), `Execute` (DelayedTrigger /
ImmediateTrigger), `FallbackAbility`, `ChooseSubAbility`, `CantChooseSubAbility` (ChoosePlayer),
`RegenerationAbility`, `ReturnAbility`, `GiftAbility`, `VoteSubAbility`, `VoteTiedAbility`.
`Choices$` is parsed as a comma list of sub-abilities for Charm, GenericChoice, AssignGroup,
VillainousChoice and Vote (for other APIs `Choices$` is a card filter). RollDice parses
`ResultSubAbilities$ <range>:<SVar>,…`.

### 0.2 Who is affected: `Defined$` versus targets

Effect classes obtain their objects through four helper families; the difference matters:

| Helper | Rule |
|---|---|
| `getTargetCards / getTargetPlayers / getTargetEntities(sa)` | If *this* link has `ValidTgts$`: its targets, and `Defined$` is ignored. Otherwise `Defined$`. |
| `getDefinedCardsOrTargeted / getDefinedPlayersOrTargeted(sa, key)` | If `key` is present: that Defined. Otherwise this link's targets if it targets. Otherwise the default. |

- Default when the key is absent and the link does not target: **`Self`** for cards / entities,
  **`You`** for players.
- The value may be several Defined expressions joined with ` & `.
- `ThisDefinedAndTgts$ <Defined>` (card helpers only): the defined cards **plus** the targets.
- `IncludeAllComponentCards$ True`: also include the other components of merged/melded cards.
- Player lists are sorted in APNAP order starting with the active player, or with
  `StartingWith$ <DefinedPlayer>`.
- Most card effects re-check each card before acting on it: the object must still be the same game
  object it was when chosen (`equalsWithGameTimestamp` — same card id and same zone-change
  timestamp). A card that changed zones since it was targeted/remembered is silently skipped. This is
  Forge's implementation of "new object" (CR 400.7) and is the reason `…LKICopy` Defined values exist.

### 0.3 `UnlessCost$` ("… unless <player> pays")

| Key | Default | Meaning |
|---|---|---|
| `UnlessCost$` | — | A cost string, or one of the special forms: `ChosenNumber` (generic mana equal to the host's chosen number), `DefinedCost_<DefinedCard>[_Minus<n>\|_Plus<n>]` (that card's mana cost, modified), `DefinedSACost_<DefinedSA>` (mana cost of that spell with X filled in), the name of an SVar (generic amount; coloured with `UnlessColor$`), `LifeTotalHalfUp` (pay half the payer's life, rounded up). |
| `UnlessPayer$` | `TargetedController` | Defined players who may pay. Each is asked in turn; players who left the game are skipped. |
| `UnlessSwitched$` | false | Invert: the effect happens **only if** someone paid ("you may pay X. If you do, …"). |
| `UnlessResolveSubs$` | (always) | `WhenPaid` / `WhenNotPaid`: run the `SubAbility$` chain only in that case. Not affected by `UnlessSwitched`. |
| `UnlessColor$`, `UnlessUpTo$`, `UnlessAI$` | | colour of an SVar-amount cost; Flash-style chosen reduction; AI hint. |

Order: conditions → payers asked (the first who pays ends it; later payers are told it is already
paid) → effect runs iff `paid == UnlessSwitched` → sub-abilities per `UnlessResolveSubs`. If the cost
cannot be computed (`DefinedCost_` with no card) the effect simply resolves.

### 0.4 Remembered / imprinted state

Forge has no local variables. An ability passes objects to later links (or to the ability's own later
resolutions!) by storing them on the **host card**: `Remembered` (cards, players, numbers, strings),
`Imprinted` cards, `ChosenCard`, `ChosenPlayer`, `ChosenType`, `ChosenColor`, `ChosenNumber`,
`NamedCard`. These survive until explicitly cleared — hence the ubiquitous trailing
`DB$ Cleanup | ClearRemembered$ True`. A translator should treat "remember in link A → `Defined$
Remembered` in link B → Cleanup" as a local binding, and must notice the cases where there is *no*
cleanup because a later trigger or static ability on the same card reads the value.

Framework-level keys (handled when the ability is put on the stack, `MagicStack`):
- `RememberTargets$ True` — add this link's targets to the host's remembered list;
  `ForgetOtherTargets$ True` clears the list first.
- `RememberCostMana$ True` — clear remembered, then remember the mana spent on the activation cost.

Per-effect keys are named `Remember<Something>$`, `Imprint…$`, `Forget…$` and are listed per API.

### 0.5 `Duration$`

Read by every effect that creates a continuous effect (Pump, PumpAll, Animate, Effect, GainControl,
Debuff, Protection, ChangeZone "until", …). Values (from `addUntilCommand`):

| Value | Ends |
|---|---|
| *(absent)*, or any unrecognised value such as `EndOfTurn` / `EOT` | at cleanup of the current turn ("until end of turn"). **Note**: this default contradicts CR 611.2a; scripts rely on it. |
| `Permanent` | never (each effect class checks this itself and registers nothing). |
| `Perpetual` | never, and the change follows the card across zones (Arena-only "perpetually"); handled by Pump/Animate/etc. individually. |
| `UntilEndOfCombat` | end of this combat. |
| `UntilEndOfCombatYourNextTurn` | end of combat on the controller's next turn. |
| `UntilYourNextUpkeep` | beginning of the controller's next upkeep. |
| `UntilTheEndOfYourNextUpkeep` | end of the controller's next upkeep step (this one, if registered during an upkeep, counts as "next"). |
| `UntilTheEndOfYourNextUntap` | end of the controller's next untap step. |
| `UntilNextEndStep` | the next end step (any player's). |
| `UntilYourNextEndStep` | the controller's next end step. |
| `UntilYourNextTurn` | when the controller's next turn begins (implemented at the cleanup step preceding it). |
| `UntilTheEndOfYourNextTurn` | end of the controller's next turn (if it is currently their turn, the one after this). |
| `UntilTheEndOfTargetedNextTurn` | same, for the first targeted player. |
| `ThisTurnAndNextTurn` | end of the turn after the current one. |
| `UntilStateBasedActionChecked` | the next time state-based actions are checked. |
| `UntilHostLeavesPlay` | when the host leaves the battlefield. |
| `UntilHostLeavesPlayOrEOT`, `UntilHostLeavesPlayOrEndOfCombat` | whichever comes first. |
| `UntilLoseControlOfHost` | host leaves play or changes controller. |
| `AsLongAsControl` | host leaves play, changes controller, or phases out. |
| `AsLongAsInPlay` | host leaves play or phases out. |
| `UntilUntaps` | host leaves play, untaps, or phases out. |
| `UntilTargetedUntaps` | the targeted card leaves play or untaps. |
| `UntilUnattached` | host leaves play, becomes unattached, or phases out. |
| `UntilFacedown` | host is turned face down. |

`checkValidDuration` is evaluated first by most duration-aware effects: for the host-dependent
durations the effect **does nothing at all** if, at resolution, the host is no longer on the
battlefield (or stack), is phased out (`AsLongAs…`), is no longer controlled by the activator
(`UntilLoseControlOfHost`, `AsLongAsControl`), or is already untapped (`UntilUntaps`; for
`UntilTargetedUntaps`, if the target is untapped or phased out). This is CR 611.2b.

### 0.6 `AtEOT$` — built-in delayed trigger

Several APIs (ChangeZone, Token, CopyPermanent, Pump, PumpAll, Animate…) accept `AtEOT$ <What>` and
register a delayed trigger for the affected cards. `<What>` is an action with optional affixes:

- action: `Sacrifice` (controller of the *effect* sacrifices them), `SacrificeCtrl` (each card's
  controller sacrifices it), `Exile`, `Destroy`, `Hand` (return to hand), `Library` (shuffle into library);
- prefix `Your…` → only at *your* next end step / upkeep (`ValidPlayer$ You`);
- suffix `…Combat` → at end of combat instead of the beginning of the next end step;
- suffix `…Upkeep` → at the beginning of the next upkeep.

Examples: `Exile`, `Sacrifice`, `ExileCombat`, `SacrificeCombat`, `YourSacrifice`, `Hand`.
The trigger acts on the remembered objects only if they are still the same objects (LKI-checked
`DelayTriggerRememberedLKI`). `AtEOTDesc$` overrides the text; `AtEOTCondition$ <SVar>` adds a
`ConditionCheckSVar` to the trigger's effect. `AtEOTTrig$ [You_]Sacrifice|Exile` (Token /
CopyPermanent) instead grants the *token itself* a real "at the beginning of the end step" trigger.

### 0.7 Other keys read by the framework for any API

- Descriptions: `SpellDescription$` (rules text), `StackDescription$` (`None`, `SpellDescription`,
  `REP a_b & c_d` replacements, or a template with `{c:Defined}`, `{p:Defined}`, `{s:Defined}`,
  `{n:SVar noun}`), `ConditionDescription$`, `AfterDescription$`, `PrecostDesc$`, `CostDesc$`,
  `Named$` (ability name). None affect game behaviour.
- `Forecast$ True` rewrites the ability to: activation zone Hand, once per turn, only during your
  upkeep. Boast / Exhaust / Power-Up abilities get their restrictions the same way.
- `NonBasicSpell$ True`: the `SP$` is not the card's basic "cast this card" spell.
- `Announce$ X` (announced value), `DividedAsYouChoose$ <amount>` (targets share an amount chosen on
  announcement; effects read each target's share), `ChangeZoneTable$ True` (see ChangeZoneResolve).
- `ReplaceDyingDefined$` / `ReplaceDyingValid$` (+ `ReplaceDyingCondition$ Kicked`,
  `ReplaceDyingZone$` default `Exile`, `ReplaceDyingExiledWith$`): after the effect, create an
  until-end-of-turn effect "if that permanent would die this turn, exile it instead" (read by
  DealDamage, Pump, Fight and others via `replaceDying`).
- `ExiledWithEffectSource$`, `DefinedExiler$`: adjust who/what an exiled card is recorded as "exiled with/by".
- AI-only (ignore): `AILogic`, `AITgts`, `AITgtsStrict`, `IsCurse`, `AICheckSVar`, `AIPreference`, …

## Index of Part I (rank by number of cards)

1. ChangeZone (5398) · 2. Pump (4436) · 3. Draw (3566) · 4. Token (3277) · 5. PutCounter (3106) · 6. Cleanup (2783) · 7. DealDamage (2575) · 8. Mana (2181) · 9. GainLife (1728) · 10. Effect (1724) · 11. Tap (1472) · 12. Destroy (1439) · 13. LoseLife (1117) · 14. Discard (1043) · 15. PumpAll (970) · 16. Dig (928) · 17. Animate (920) · 18. Sacrifice (809) · 19. Charm (806) · 20. ChangeZoneAll (572) · 21. Mill (570) · 22. Counter (514) · 23. Untap (470) · 24. DelayedTrigger (449) · 25. Scry (424) · 26. DamageAll (396) · 27. ChooseCard (347) · 28. CopyPermanent (334) · 29. DestroyAll (328) · 30. GainControl (308) · 31. ImmediateTrigger (307) · 32. Play (302) · 33. PutCounterAll (293) · 34. RepeatEach (286) · 35. SetState (276) · 36. Regenerate (266) · 37. Surveil (245) · 38. Attach (236) · 39. CopySpellAbility (235) · 40. RemoveCounter (190) · 41. MakeCard (182) · 42. ChooseType (170) · 43. Clone (168) · 44. DigUntil (167) · 45. PeekAndReveal (152) · 46. Fight (148) · 47. ReplaceEffect (147) · 48. Investigate (139) · 49. GenericChoice (134) · 50. ChooseColor (128) · 51. SacrificeAll (126) · 52. AlterAttribute (123) · 53. AnimateAll (122) · 54. UntapAll (122) · 55. PreventDamage (113) · 56. ChoosePlayer (113) · 57. Seek (110) · 58. Branch (108) · 59. Proliferate (94) · 60. NameCard (83)

Part II lists the remaining APIs of the frequency table (ranks 61–184 plus engine-internal ones) with short entries; Appendix A summarises the cross-cutting rules, Appendix B the verification status.

---

# Part I — the 60 most-used APIs

## 1. ChangeZone — 5398 cards, 6777 lines — `ChangeZoneEffect`

Moves specific cards from one zone to another: bounce, reanimate, tutor, exile, flicker, "put a land
from your hand onto the battlefield", "exile until this leaves the battlefield". Not used for draw,
discard, mill, sacrifice or destroy (those have their own APIs because triggers/replacements
distinguish them). Declares itself as "moves a card to/from a library" (so the ability is not a mana
ability) when `Origin` or `Destination` includes Library.

There are **two completely different resolution modes**:

- **Hidden-origin mode** ("search / choose at resolution"): selected when `Hidden$ True` is set, or
  when any zone in `Origin$` is a hidden zone (`Hand`, `Library`, `Sideboard`, the variant decks).
  The cards are chosen *while resolving*, by a player, out of a filtered list.
- **Known-origin mode** (everything else, and always for Ninjutsu): the cards are already fixed —
  they are this link's targets or its `Defined$`.

At build time, if the ability targets cards (not players) the target zone is set to the `Origin$`
list — so `ValidTgts$ Creature | Origin$ Graveyard` targets creature cards in graveyards without a
`TgtZone$`.

### Parameters

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Origin` | 6760 | zone list (`,`-separated): Battlefield, Graveyard, Hand, Library, Exile, Stack, Command, Sideboard, `All` … | none | Zones the card may come from. Known mode: a card not currently in one of them is skipped (no restriction when absent). Hidden mode: the zones searched. |
| `Destination` | 6775 | one zone | required | Where the card goes. |
| `Hidden` | 675 | `True` | — | Force hidden-origin mode for a public origin ("return a creature card from your graveyard" chosen on resolution, non-targeted). |
| `Defined` | 1801 | Defined cards | `Self` (known mode) | Known mode: the cards to move (ignored if this link has targets). Hidden mode: the candidate list is exactly these cards and **all** of them move, without a choice (unless `ChangeNum` limits it — then the first N). |
| `ValidTgts`, `TgtPrompt`, `TargetMin`, `TargetMax`, `TgtZone`, `TargetsWithDefinedController` … | 2596 | targeting | — | Known mode: targets are the cards (or spells on the stack) moved. Hidden mode: a targeted **player** whose zone is searched (see below). |
| `ChangeType` | 2018 | Valid filter; or `EACH f1 & f2`; may start with `Triggered…`, `Targeted…`, `Remembered…`, `Imprinted…` (filter evaluated relative to that card); comparisons may name an SVar (`cmcLEX`) | no filter | Hidden mode: which cards may be chosen. `EACH a & b` = choose `ChangeNum` cards of *each* listed filter. |
| `ChangeNum` | 1450 | amount | 1 (all candidates when `Defined` is used) | Hidden mode: maximum number of cards chosen. |
| `ChangeTypeDesc`, `ChangeNumDesc`, `DefinedDesc`, `SelectPrompt`, `OptionalPrompt` | 375/3/4/164/3 | text | — | Description / prompt text only. |
| `Mandatory` | 511 | `True` | false | Hidden mode: the chooser may not decline while candidates remain (CR 701.23d: searching for an unrestricted card, or choosing from a public zone). Without it every pick is optional ("fail to find"). |
| `Optional` | 235 | `True` (any value) | false | "You may". Known mode: the chooser confirms **per card**. Hidden mode: the decider confirms once per fetching player before anything is looked at; declining skips that player (no search, no shuffle). |
| `Chooser` | 256 | Defined player | known: activator; hidden: the fetching player | Who makes the choices (which cards; ordering). If it defines several players, the activator picks one of them. |
| `DefinedPlayer` | 332 | Defined players | `You` | Hidden mode: the players whose zones are searched ("each player searches…"), processed in APNAP order. Its presence also forces the candidate list to that player's own cards. |
| `ChooseFromDefined` | 16 | Defined cards | — | Hidden mode: candidates are these cards, but (unlike `Defined`) the player still **chooses** `ChangeNum` of them. |
| `LibraryPosition` | 299 | amount; `0` top, `-1` bottom, `n` = n cards below the top | `0` (top) | Position when `Destination$ Library`. |
| `Shuffle` | 254 | `True` / `False` | see rules | `True`: shuffle after moving to a library (known mode: owners of the moved cards shuffle). `False`: suppress the automatic shuffle after a library search. |
| `NoShuffle` | 32 | `True` | — | Same as `Shuffle$ False` (hidden mode). |
| `ShuffleNonMandatory` | 217 | `True` | — | Marks "if you do, shuffle" wording. Hidden mode: if the player cannot search at all, no shuffle happens. Known mode: the activator is asked whether to shuffle and answering "no" **cancels the whole effect**. |
| `Tapped` | 486 | `True` | — | Enters the battlefield tapped. |
| `GainControl` | 270 | `True` or Defined player | — | Enters under the control of the activator (`True`) or of that player instead of its owner. |
| `Attacking` | 35 | `True` or Defined entities (` & `) | — | Enters attacking; `True` = the activator chooses among the current defenders, otherwise among the defined players/planeswalkers/battles. Only if a combat is in progress, the card is a creature and its controller is on the attacking side. |
| `Blocking` | rare | Defined attacker | — | Enters blocking that attacker (fires the blocked triggers if the attacker was unblocked). |
| `Transformed` | 75 | `True` | — | Enters transformed / converted. A card that cannot transform does **not move at all**. |
| `FaceDown` | 10 | `True` | — | Enters the battlefield face down (see `FaceDownPower/Toughness/SetType` — read by `CardFactoryUtil.setFaceDownState`, UNVERIFIED details); for exile it means exiled face down. |
| `ExileFaceDown` | 25 | `True` | — | Exiled face down. |
| `WithMayLook` | rare | `True` | — | The activator may look at the face-down exiled card. |
| `Foretold`, `ForetoldCost` | rare | `True` | — | Marks the exiled card foretold (and that its foretell cost came from an effect); implies may-look. |
| `WithCountersType` | 145 | counter type(s) | — | Enters the battlefield with counters (applied as part of the move, so replacement effects see them); for Exile/other destinations counters are put on the card after it moved. Known mode → battlefield accepts a `,` list of types. |
| `WithCountersAmount` | 57 | amount | 1 | Number of each counter. |
| `WithCountersPlacer` | rare | Defined player | activator | Who puts the counters (stack→zone case only). |
| `WithNotedCounters` | rare | `True` | — | Enters with the counters previously noted on the host. |
| `AttachedTo` | 42 | Defined card, else a Valid filter over the battlefield as it was before the effect | — | An Aura/Equipment enters attached to one of these (activator/decider chooses if several; only legal hosts qualify). An **Aura with no legal host stays where it is**. |
| `AttachedToPlayer` | 5 | Defined player | — | Same, attached to a player. No legal player → the card does not move. |
| `AttachAfter` | rare | Defined / Valid | — | Attach after the card has entered (for Equipment etc.) instead of entering attached. |
| `LeaveBattlefield` | rare | zone | — | Creates a replacement effect: if the moved permanent would leave the battlefield, it goes to that zone instead. |
| `Duration` | 112 | usually `UntilHostLeavesPlay` | — | "Exile … until CARDNAME leaves the battlefield". See rules. |
| `ReturnAbility`, `ReturnValid` | rare | SVar; Valid | — | With `Duration`: an ability to run when a card returns (delayed trigger on that return). |
| `AtEOT` | 10 | see §0.6 | — | Delayed trigger for the moved cards. |
| `RememberChanged` | 744 | `True` | — | Remember each card that actually moved (the new object; plus meld/merge components). |
| `RememberLKI` | 50 | `True` | — | Remember a last-known-information copy of each card as it was *before* moving. |
| `ForgetChanged` | 47 | `True` | — | Remove each moved card from the remembered list. |
| `ForgetOtherRemembered` | 53 | `True` | — | Clear the remembered list before doing anything. |
| `Imprint` | 80 | `True` | — | Imprint each moved card on the host. `ImprintLast` (Duplicant) keeps only the last. |
| `Unimprint` | 6 | `True` | — | Clear imprinted cards first. |
| `RememberSearcher` | rare | `True` | — | Remember the player whose library was searched. |
| `Reveal` | 63 | `True` | auto | Hidden mode: reveal the moved cards. Automatic when the destination is not the battlefield and `ChangeType` is a real restriction (anything but empty / `Card`) — "search for a creature card, **reveal it**". |
| `NoReveal` | 10 | `True` | — | Suppress that automatic reveal. |
| `NoLooking` | 53 | `True` | — | Library origin without *searching*: no search event, no "look at the whole library" (random or top/bottom based picks). |
| `AlreadyRevealed` | 13 | `True` | — | Do not show the zone to the decider again. |
| `AtRandom` | 52 | `True` | — | Hidden mode: cards are picked at random from the candidates. |
| `DifferentNames`, `DifferentCMC`, `DifferentPower` | 21/rare | `True` | — | Hidden mode: the chosen cards must all differ in name / mana value / power. |
| `ShareLandType` | rare | `True` | — | Later picks must share a land type with earlier picks. |
| `SameNameAsEach` | 3 | Defined cards | — | For each of those cards, pick one candidate with the same name. |
| `WithTotalCMC`, `WithTotalPower` | 5/rare | amount | — | The chosen cards' total mana value / power may not exceed this (budget shrinks with each pick). |
| `WithTotalCardTypes` | rare | amount | — | Must reach at least this many card types in total, otherwise **nothing** is chosen. |
| `Exactly` | 3 | `True` | — | If fewer than `ChangeNum` cards were chosen, that player moves nothing. |
| `Reorder` | 16 | `True` | — | Decider orders the chosen cards before they move. |
| `RandomOrder` | rare | `True` | — | Known mode into a library: random order. |
| `ShuffleChangedPile` | rare | `True` | — | Randomise the order of the chosen cards. |
| `SkipCancelPrompt` | rare | `True` | — | UI only. |
| `OriginAlternative` | 62 | zone list | — | Hidden mode: additional zones the player may choose to look in ("search your library and/or graveyard"). If a matching card exists in a chosen *public* zone the pick becomes mandatory. |
| `DestinationAlternative` | 49 | zone | — | An alternative destination; see `DestAltSVar`. |
| `LibraryPositionAlternative` | 39 | integer | 0 | Library position for the alternative destination. |
| `DestAltSVar`, `DestAltSVarCompare` | 6 | SVar (optionally prefixed `MANDATORY `); comparison (default `GE1`) | — | The alternative is available only if the comparison holds; with `MANDATORY` it is then forced. |
| `AlternativeDecider` | 36 | Defined player | the chooser | Who picks between the two destinations ("its owner's choice of top or bottom"). |
| `TrackDiscarded` | rare | `True` | — | Mark the moved card as discarded (CR 400.7k-style tracking). |
| `Champion` | rare | `True` | — | Fire the "championed" trigger for each moved card. |
| `StaticEffect` (50), `FaceDownSetType` … | | | | Read by the zone-change machinery, not this class: `StaticEffect$ Animate` applies an accompanying Animate to the card as it enters. UNVERIFIED details. |

### Known-origin mode — order of operations

1. Compute destination, `LibraryPosition`; apply the destination alternative (step "Alternatives" below).
2. Targeted **spells on the stack** are handled first: removed from the stack and put into
   Graveyard / Exile / Hand / Library (any other destination is an error). (This is how "return
   target spell to its owner's hand" and "exile target spell" are scripted.)
3. `Unimprint`, `ForgetOtherRemembered`.
4. `ShuffleNonMandatory` prompt (declining aborts everything).
5. If several cards go to a library (and `Shuffle` is not True) their order is chosen: `RandomOrder`
   → random; `Chooser` given → that player; otherwise **each owner orders their own cards** (CR 401.4).
6. For each card, in order:
   - skip if it is no longer the same object or is phased out;
   - `RememberLKI`; `Optional` confirmation; skip if not in an `Origin` zone;
   - **to the battlefield**: apply `Tapped`, `Transformed`, counters, `GainControl`, `AttachedTo`,
     `AttachedToPlayer`, `FaceDown` to the card *before* it moves, then put it onto the battlefield
     under its (possibly new) controller with all the moving cards marked as entering
     simultaneously; then `LeaveBattlefield`, `Attacking`/`Blocking`, `AttachAfter`;
   - **elsewhere**: for Exile check the card can be exiled by this ability and record "exiled with
     <host>" / "exiled by <activator>"; move (using the library position); cards moving from the
     command zone to a hand are revealed; then counters, face-down, foretold, may-look;
   - if the card really changed zones: `RememberChanged`, `ForgetChanged`, `Imprint`.
7. After the loop: zone-change triggers are collected for **all** moved cards as one simultaneous
   event; counters-put replacement/trigger processing; `AtEOT`; `Duration` handling; `Shuffle$ True`
   into library → the owners of the moved cards shuffle (or the `TargetsWithDefinedController`
   players; the activator if there were no cards at all).

### Hidden-origin mode — order of operations

Phase A (choices), for each fetching player in APNAP order:

1. Fetching players = `DefinedPlayer` (default: you). If the link **targets a player** and has no
   `DefinedPlayer`, the zone owner is the first targeted player while the *decider* is still the
   activator (or `Chooser`) — this is "target opponent reveals their hand; you choose a card from it".
2. `OriginAlternative` questions; `Optional` confirmation.
3. Candidate list:
   - `Defined` / `ChooseFromDefined`: those cards (still the same objects, not phased out);
   - else if `Origin` contains neither Library nor Hand and there is no `DefinedPlayer`: cards in
     the origin zones of **all** players;
   - else the fetching player's cards in the origin zones. For a library (without `NoLooking`) this
     is a *search*: only the searchable part of the library counts (effects may restrict it, or
     forbid searching — then nothing can be found and the shuffle depends on `ShuffleNonMandatory`),
     the "searched library" trigger fires and "cast while searching" cards are offered.
4. Apply `ChangeType`.
5. Pick up to `ChangeNum` cards: at random (`AtRandom`), automatically (`Defined`), or by the
   decider — optional per pick unless `Mandatory`; subject to the `Different…` / `WithTotal…` /
   `ShareLandType` / `SameNameAsEach` constraints.
6. `DestinationAlternative` decision. **If the search is library → library and a shuffle is due, the
   library is shuffled now, before the card is placed** (tutor to top: "search, shuffle, put on top").
7. `Reorder`; `Exactly` check.

Phase B (moves), again per player, after *all* players have chosen:

8. Each chosen card moves: Library → at `LibraryPosition`; Battlefield → `Tapped`, `GainControl`,
   counters, `Transformed`, `AttachedTo(Player)`, `Attacking`/`Blocking`, `FaceDown`, then enters
   under its controller, then `AttachAfter`; Exile → can-be-exiled check, exiled-with bookkeeping,
   `ExileFaceDown`, `Foretold`, `WithMayLook`; otherwise a plain move.
9. `Champion`, `RememberChanged`, `RememberLKI`, `ForgetChanged`, `Imprint`, exile counters.
10. Reveal (see `Reveal`). Shuffle: if the origin included the library, the destination is not the
    library and the shuffle is still mandatory (`NoShuffle` / `Shuffle$ False` / `Defined` turn it
    off), or `Shuffle$ True` — the fetching player shuffles. `AtEOT`.
11. After all players: zone-change triggers as one batch; `Duration` handling.

### Subtle points

- **Alternatives**: without `DestAltSVar` the decider is simply asked "destination 1 or 2" (for
  Library/Library: "top or bottom"). With `DestAltSVar` the alternative exists only if the SVar
  comparison holds, and `MANDATORY` removes the choice.
- **`Duration$ UntilHostLeavesPlay`**: if the host has already left the battlefield when the ability
  resolves, nothing is moved (§0.5). Otherwise the moved non-token cards are recorded on the host;
  when the host leaves play each one that is still in the destination zone as the same object returns
  to the zone it came from (the battlefield, under its owner's control), all in one batch. Tokens
  never return. This is one ability with a duration (CR 610.3), not two linked abilities.
- `Defined$` in hidden mode is the usual way to move "that card" out of a hand/library where no
  choice is involved (`Defined$ TopOfLibrary`, `Defined$ Remembered`).
- A card moved to the battlefield from a zone where it has no controller enters under its **owner's**
  control unless `GainControl` is given.
- An unearth-keyword ChangeZone additionally gives haste, the exile-instead replacement and the
  end-step exile trigger; a Ninjutsu ChangeZone enters tapped and attacking the returned creature's
  defender. These are driven by the keyword, not by parameters.

### Examples

```
A:SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Library | LibraryPosition$ 0            (Excommunicate)
SVar:TrigChangeZone:DB$ ChangeZone | Origin$ Library | Destination$ Hand | ChangeType$ Land.Basic | ChangeTypeDesc$ basic land | ChangeNum$ X | DifferentNames$ True   (Fblthp Knows the Way)
SVar:TrigBounce:DB$ ChangeZone | Origin$ Battlefield | Destination$ Hand | Hidden$ True | Mandatory$ True | ChangeType$ Creature.YouCtrl | ChangeNum$ 1   (Roaring Primadox)
SVar:TrigExile:DB$ ChangeZone | Origin$ Battlefield | Destination$ Exile | ValidTgts$ Creature.DefenderCtrl | Duration$ UntilHostLeavesPlay   (Colossal Whale)
```

## 2. Pump — 4436 cards, 5082 lines — `PumpEffect`

Gives specific cards +X/+Y and/or keywords for a duration; can also give keywords to players.
Because it does nothing when given no numbers and no keywords, it doubles as Forge's **no-op carrier**
for targeting ("choose a target here, use it in a sub-ability") and for remember/imprint bookkeeping.

### Parameters

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` | 2057 | Defined cards / players | card `Self` **and** player `You` | Affected objects when not targeting. The same key is evaluated once as cards and once as players. |
| `ValidTgts` … | 2584 | targeting | — | Targets; targeted *spells* count as their cards (with `PumpZone$ Stack`). |
| `NumAtt` | 2680 | signed amount (`+2`, `-X`, `X`), or `Double` / `Triple` | 0 | Power change. `Double` = +current power, `Triple` = +2× current power, computed per card at resolution. |
| `NumDef` | 1812 | same | 0 | Toughness change. |
| `KW` | 2140 | keywords separated by ` & ` | — | Keywords granted. Any Forge keyword string, including parameterised ones (`Protection:…`, `Landwalk:Island`) and long rules sentences Forge treats as keywords (`Prevent all combat damage that would be dealt by CARDNAME.`). A keyword prefixed `HIDDEN ` is granted invisibly (an internal marker such as `HIDDEN CARDNAME can't block.`). Placeholders `CardManaCost` / `ConvertedManaCost` are replaced per affected card. |
| `KWChoice` | 32 | `,` list of keywords | — | Used only when `KW` is absent: the activator chooses one keyword from the list at resolution. |
| `DefinedKW` | 13 | `ChosenType`, `ChosenColor`, `ChosenPlayer`, or Defined players | — | Substitute into the keyword text: the host's chosen type / colour (`ChosenColor`, `chosenColor`) / player (`ChosenPlayerUID`, `ChosenPlayerName`). If the host has no such choice the effect does nothing. |
| `DefinedLandwalk` | rare | Defined cards | — | Add `Landwalk:<type>` for each land type of those cards. |
| `SharedKeywordsZone`, `SharedRestrictions` | rare | zones; `,` Valid list (default `Card`) | — | Keep only those `KW` keywords that some matching card in those zones has ("gains flying if a creature card in your graveyard has flying"). |
| `RandomKeyword`, `RandomKWNum`, `NoRepetition` | rare | `True`; amount (1); `True` | — | Grant N keywords picked at random from `KW` (optionally excluding ones the first target already has). |
| `Duration` | 334 | §0.5; `Permanent`, `Perpetual` | until end of turn | How long. `Permanent` registers no expiry. `Perpetual` additionally records the boost/keywords as perpetual. For players only `Permanent` is special. |
| `PumpZone` | 121 | zone list | `Battlefield` | Only cards currently in these zones are affected (use `Hand`, `Graveyard`, `Stack`, `Exile` … to pump cards elsewhere). |
| `Optional`, `OptionQuestion` | 4 | `True`; text with `TARGETS` | — | Activator may decline the whole effect. |
| `CanBlockAny` | rare | `True` | — | Affected creature can block any number of creatures. |
| `CanBlockAmount` | 6 | amount | — | Can block that many additional creatures. |
| `LeaveBattlefield` | rare | zone | — | Replacement: if the pumped permanent would leave the battlefield, put it there instead. |
| `AtEOT` | 35 | §0.6 | — | Delayed trigger on the target cards (`Sacrifice` = "sacrifice it at the beginning of the next end step"). |
| `Radiance` | rare | `True` | — | Also affects each other creature sharing a colour with the target. |
| `RememberPumped` | 4 | `True` | — | Remember each card actually pumped. |
| `RememberObjects` | 67 | Defined objects | — | Add to remembered (works with no pump at all). |
| `ForgetObjects` | 20 | Defined objects | — | Remove from remembered. |
| `ImprintCards` | 22 | Defined cards | — | Imprint. |
| `ForgetImprinted` | 20 | Defined cards | — | Remove from imprinted. |
| `NoteCardsFor` + `NoteCards` | 29 | name; Defined cards | — | For each target player, record a note under that name containing those cards' ids. |
| `ClearNotedCardsFor` | 14 | `,` names | — | Clear such notes. |
| `NoteNumber` | rare | amount | — | Target players note a number under the host's name. |
| `ReplaceDyingDefined` etc. | 4 | §0.7 | — | "If it would die this turn, exile it instead." |
| `IfDesc`, `SpellDescription` | | | | Description only. |

### Behaviour

1. `checkValidDuration` (§0.5) — may abort.
2. One timestamp is taken for the whole effect.
3. `Optional` prompt.
4. Keyword list is built (`KW` or `KWChoice`, then `SharedKeywords…`, `DefinedKW`,
   `DefinedLandwalk`, `RandomKeyword`); amounts are computed **once at resolution** (locked in, CR
   608.2h) except `Double`/`Triple`, which are per card.
5. Remember / note / forget / imprint bookkeeping (this happens even when nothing is pumped).
6. Each target card: skipped if phased out, not in a `PumpZone`, or no longer the same object.
   Otherwise it gets a P/T boost and keyword grant tagged with the timestamp (layer 7c / layer 6
   effects from a resolved ability: they affect only the objects chosen at resolution).
7. `AtEOT` trigger; Radiance cards; then each affected **player** gets the keywords.
8. Unless `Permanent`/`Perpetual`, an expiry is registered per card per §0.5.

Quirks: a Pump with neither `Defined` nor targets affects the host card *and* gives the keywords to
its controller as a player — scripts avoid this by always writing `Defined$ Self`. Power/toughness
"switching" is expressed as the hidden keyword `HIDDEN CARDNAME's power and toughness are switched`.

### Examples

```
A:AB$ Pump | Cost$ R | Defined$ Self | NumAtt$ +1 | SpellDescription$ CARDNAME gets +1/+0 until end of turn.
SVar:TrigPump:DB$ Pump | Defined$ Self | KW$ HIDDEN CARDNAME can't block. | Duration$ UntilEndOfCombat   (Flummoxed Cyclops)
A:SP$ Pump | ValidTgts$ Land.YouCtrl | SubAbility$ DBExchange | …   (Political Trickery: Pump as a pure target carrier)
```

## 3. Draw — 3566 cards, 3869 lines — `DrawEffect`

Players draw cards. Always counts as moving cards from a library (never a mana ability).

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` | 1379 | Defined players | `You` | Who draws. `Defined` takes precedence over this link's targets. A player listed twice draws twice the amount. |
| `ValidTgts` … | 150 | player targeting | — | Target players draw. |
| `NumCards` | 2268 | amount | 1 | Cards drawn by each player. |
| `Upto` | 9 | `True` | — | "Draw up to N": the player chooses a number from 0 to the number they are able to draw. Implies optional. |
| `OptionalDecider` | 35 | any value | — | "May draw": each drawing player is asked yes/no (the value is not used to pick the decider — the drawing player always decides). If the player cannot draw that many, they are skipped without being asked. |
| `Reveal` | 5 | `True` / `All` | — | Reveal the drawn cards (`All`: to everyone including the drawer's own log; `True`: to the other players). |
| `RememberDrawn` | 33 | `True` / `AllReplaced` | — | Remember the cards drawn. `AllReplaced` (read in `Player`) also remembers what happened to draws that were replaced. |
| `NumCardsDesc`, `IfDesc` | | text | | Description only. |

Behaviour: players are processed in APNAP order; each draw is an individual draw event performed by
the player's draw routine (so replacement effects such as dredge apply per card, and drawing from an
empty library flags the player). `Draw` with `UnlessCost$ Discard<1/Card> | UnlessSwitched$ True` is
the idiom for "you may discard a card. If you do, draw a card" (52 lines).

```
A:SP$ Draw | NumCards$ 2 | SpellDescription$ Draw two cards.
SVar:TrigDraw:DB$ Draw | Defined$ Player.Opponent | Upto$ True | NumCards$ 3   (Shah of Naar Isle)
```

## 4. Token — 3277 cards, 3698 lines — `TokenEffect` (base `TokenEffectBase`)

Creates tokens defined by token scripts.

### Token scripts

`TokenScript$ name[,name…]` names files `forge-gui/res/tokenscripts/<name>.txt` (864 files). Each is
a miniature card script: `Name:`, `ManaCost:no cost`, `Colors:`, `Types:`, `PT:`, `K:` keywords,
`A:`/`T:`/`S:`/`R:` abilities, `SVar:`s, `Oracle:`. The file name is a convention, not parsed:
`<colours>_<power>_<toughness>_[a_|e_]<subtypes…>[_<keywords/abilities>]` — colours are WUBRG letters
(`c` colourless, `all`), `a`/`e` mark artifact/enchantment, `x` marks a `*` P/T that the creating
ability supplies with `TokenPower`/`TokenToughness`. Examples: `w_1_1_soldier`,
`r_4_4_dragon_flying`, `c_a_treasure_sac`, `c_a_clue_draw`, `g_x_x_wurm` (PT `*/*`),
`c_0_0_a_construct_total_artifacts` (carries its own static ability), `role_cursed` (an Aura Role).
The most used are `c_a_treasure_sac` (388 lines), `c_a_food_sac` (160), `b_2_2_zombie` (114).
The translator needs the token scripts as a second input corpus; the same script name always means
the same token definition (the edition only selects artwork).

### Parameters

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `TokenScript` | 3698 | `,` list of script names | required | One token *kind* per name; `TokenAmount` of **each** kind is created for each owner. |
| `TokenAmount` | 1067 | amount | 1 | Number of tokens per kind per owner. Less than 1 → nothing happens. |
| `TokenOwner` | 2541 | Defined players | `You`, or this link's targeted players | Who creates (owns and controls) the tokens. |
| `ValidTgts` … | 80 | targeting | — | Target players (as owners) or target objects referenced by `AttachedTo$ Targeted` etc. |
| `TokenTapped` | 246 | `True` | — | Created tapped. |
| `TokenAttacking` | 66 | `True` or Defined entities | — | Created attacking (see ChangeZone `Attacking`). With `True` the *activator* chooses the defender. |
| `TokenBlocking` | 3 | Defined attacker | — | Created blocking that attacker. |
| `TokenPower`, `TokenToughness` | 77/75 | amount / SVar | from script | Set the token's base power / toughness (for `x` scripts). Evaluated once, at creation. |
| `TokenTypes` | 2 | types, may contain `ChosenType` | — | Add types and rename the token to that string. If it needs `ChosenType` and the host has none, nothing is created. |
| `TokenColors` | 2 | `,` colours, may contain `ChosenColor` | — | Set the colours. Same abort rule for `ChosenColor`. |
| `LockTokenScript` | 13 | `True` | — | Do not apply the creating ability's text-change effects (changed colour/creature-type words) to the token. |
| `PumpKeywords` | 29 | ` & ` keywords | — | Keywords granted to the tokens by the effect (not part of the token's copiable values), e.g. haste. |
| `PumpDuration` | 25 | `UntilYourNextTurn` or anything else (`EOT`) | **permanent if absent** | When the `PumpKeywords` wear off: `UntilYourNextTurn`, otherwise end of turn. Without this key they never wear off. |
| `AttachedTo` | 50 | Defined entity (first is used) | — | The token (Aura/Equipment/Role) is created attached to it. An Aura token that cannot legally be attached is **not created**. |
| `AttachAfter` | rare | `True` | — | Attach after entering instead of entering attached. |
| `WithCountersType`, `WithCountersAmount` | 2 | type; amount (1) | — | Enters with counters. |
| `AddTriggersFrom` | 1 | Defined cards | — | Copy those cards' triggered abilities onto the token. |
| `AtEOT` | 48 | §0.6 | — | Delayed trigger for all tokens created (`Exile`, `Sacrifice`, `ExileCombat`, …). |
| `AtEOTTrig` | rare | `[You_]Sacrifice` / `[You_]Exile` | — | Give each token its own end-step trigger. |
| `RememberTokens` | 118 | `True` | — | Remember each token created. |
| `RememberOriginalTokens` | 8 | `True` | — | Remember only tokens of the originally requested kinds (not ones substituted by replacement effects). |
| `ImprintTokens` | 12 | `True` | — | Imprint each token on the host. |
| `RememberSource` | 2 | `True` | — | Each token remembers the host. |
| `TokenRemembered` | 1 | Defined objects | — | Each token remembers these. |
| `CleanupForEach` | rare | `True` | — | Token forgets what its prototype remembered. |
| `ChangeZoneTable` | — | `True` | — | Do not fire enter-the-battlefield triggers yet; a later `ChangeZoneResolve` does (several creations entering simultaneously). |

### Behaviour

1. Abort checks (`TokenTypes`/`TokenColors` needing a missing choice; amount < 1).
2. A table *(creating player, token prototype) → amount* is built for every owner still in the game.
3. For each player the **create-token replacement event** is run on the whole table (doublers,
   "instead create…", "can't create" all operate here and may change kinds and amounts).
4. For every token: fresh copy of the prototype with a common timestamp; owner = creator; `Tapped`,
   `AttachedTo`, counters, extra triggers are applied *before* it enters; it enters the battlefield;
   then keywords, end-step trigger, combat placement, attach-after, remember/imprint.
5. `AtEOT` trigger; ETB triggers for all tokens fire together.

The token's controller is the prototype's controller, which is the owner unless a replacement effect
changed it.

```
A:SP$ Token | TokenAmount$ 3 | TokenScript$ w_1_1_soldier | TokenOwner$ You | SpellDescription$ Create three 1/1 white Soldier creature tokens.
SVar:TrigToken:DB$ Token | TokenScript$ w_4_4_angel_flying | TokenTapped$ True | TokenAttacking$ True | AtEOT$ ExileCombat   (Geist of Saint Traft)
```

## 5. PutCounter — 3106 cards, 3436 lines — `CountersPutEffect`

Puts counters on cards and/or players. Also the implementation of the keyword actions bolster,
support, adapt and monstrosity, and of "enters with counters" replacement bodies.

### Parameters

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `CounterType` | 3305 | counter name in capitals (`P1P1`, `M1M1`, `LOYALTY`, `CHARGE`, `ENERGY`, `POISON`, keyword counters such as `Flying`); a `,` list (the placer chooses one type when there are several); `ExistingCounter` | required unless a shorthand sets it | Kind of counter. `ExistingCounter` = a kind already on the object (chosen by the activator if more than one). |
| `CounterNum` | 3066 | amount | 1 | Number of counters per object. |
| `Defined` | 1892 | Defined cards and/or players | `Self` | Objects receiving counters (`Defined` wins over targets). Players get player counters (energy, poison, experience …). |
| `ValidTgts` … | 982 | targeting | — | Target objects. |
| `DividedAsYouChoose` | 54 | amount | — | Framework key: the counters are distributed among the targets as chosen on announcement; each target receives its share. Without targets (with `Choices`) the distribution is asked at resolution, at least 1 each. |
| `Choices` | 51 | Valid cards | — | Non-targeted choice at resolution: choose card(s) matching this filter and put the counters on them. Cards that cannot receive the counter are not offered (CR 608.2d) unless `SkipReceiveCounters`. |
| `ChoiceZone` | rare | zone | Battlefield | Zone the choices are in. |
| `ChoiceAmount`, `MinChoiceAmount` | 10/8 | amount | 1; = `ChoiceAmount` | How many cards to choose (max / min). |
| `Chooser` | 6 | Defined player | activator | Who chooses among `Choices`. No such player → nothing happens. |
| `ChoiceTitle`, `ChoicesDesc`, `SpecifyCounter` | | text | | Prompt text. |
| `ChoiceOptional` | 1 | `True` | — | The choice may be declined. |
| `Placer` | 11 | Defined player | activator | The player who *puts* the counters (matters for "whenever you put counters" and doubling effects). |
| `ETB` | 161 | `True` | — | Used inside a replacement of an enters-the-battlefield event: the counters are added to the entering event's counter table, so the permanent **enters with** them rather than receiving them afterwards. |
| `Optional` | 7 | `True` | — | Activator may decline. |
| `UpTo`, `UpToMin` | 6 | `True`; amount (0) | — | Activator chooses how many (min..`CounterNum`) per object. |
| `CounterNumPerDefined` | 3 | SVar | — | Amount evaluated relative to each receiving card. |
| `CounterTypePerDefined` | 2 | `True` | — | Choose the type (from the `CounterType` list) separately for each object. |
| `UniqueType` | 1 | `True` | — | Per object, only types it does not have yet may be chosen. |
| `RandomType` | 1 | `True` | — | Pick the type at random from the list. |
| `CounterTypes` | 19 | `,` list of types; may include `ChosenFromList` (+ `TypeList`), `EachType_<Valid>` | — | Put `CounterNum` of **each** listed type ("a +1/+1 counter and a lifelink counter"). `EachType_<Valid>` = each kind of counter found on matching permanents. |
| `SplitAmount` | 1 | `True` | — | With `CounterTypes`: split `CounterNum` among the types instead of that many of each. |
| `AltChoiceForEach` | 1 | type | — | For each `EachType` kind, may choose this type instead. |
| `ChooseDifferent` | 1 | integer | — | Choose that many different types from the `CounterType` list. |
| `EachExistingCounter` | 3 | `True` | — | One `CounterNum` of every kind already on the object. |
| `EachFromSource` | 24 | Defined cards | — | For each kind of counter on those cards, put as many of that kind (or `CounterNum` if given) on the object — "put its counters on …" (typically `TriggeredCardLKICopy`). |
| `PutOnEachOther` | 1 | Valid | — | With `ExistingCounter`: put the chosen kind on each *other* matching permanent. |
| `PutOnDefined`, `OnlyNewKind` | 1 | Defined cards; `True` | — | With `ExistingCounter`: put the chosen kind on those cards (only if they lack that kind). |
| `TriggeredCounterMap`, `CounterMapValues` | 3 | `True`; integer | — | Use the counter kinds and amounts of the triggering event (optionally a fixed number of each). |
| `SharedKeywords` + `SharedKeywordsDefined` / `SharedKeywordsZone` + `SharedRestrictions` | 2 | | — | Put a keyword counter for each listed keyword that the defined cards / matching cards in the zones have. |
| `DividedRandomly` | 2 | `True` | — | Counters are distributed one at a time at random among the objects. |
| `ForColor` | 1 | `True` | — | Repeat the whole effect once per colour with the host's chosen colour set to it. |
| `MaxFromEffect` | 4 | integer | — | (counter table) cap: the object ends with at most this many of that kind from this effect. |
| `RemovePhase` | 2 | phase name (`Cleanup`, `End of Turn`) | — | For each counter put, a delayed trigger removes one at the beginning of that step. |
| `RememberCards` | 13 | `True` | — | Remember every object that was to receive counters. |
| `RememberPut` | 10 | `True` | — | (counter table) remember the objects that actually received counters after replacement effects. |
| `RememberAmount` | 8 | `True` | — | (counter table) remember the total number actually put. |
| `Bolster` | 26 | amount | — | Shorthand: `CounterType$ P1P1`, `CounterNum$ n`, `Choices$ Creature.leastToughnessControlledByYou`, chosen even if it cannot receive counters. |
| `Support` | 19 | amount | — | Shorthand: up to *n* target creatures (other creatures if the host is a permanent), one +1/+1 counter each. |
| `Adapt` | 25 | amount | — | Shorthand P1P1 × n; skipped if the creature already has a +1/+1 counter (unless an effect says otherwise); fires the "adapted" trigger. |
| `Monstrosity` | 37 | amount | — | Shorthand P1P1 × n; skipped if already monstrous; marks it monstrous and fires "becomes monstrous". |

### Behaviour

1. Amount, placer, and (in the simple case) the counter type are fixed first — a multi-type
   `CounterType` list prompts the *placer* once.
2. Recipients: `Choices` (chosen now) or defined/targeted entities. A card that is no longer the same
   object is skipped.
3. Per recipient the variant keys are applied in this precedence: `ChooseDifferent` → `CounterTypes`
   → `ExistingCounter` family → `EachFromSource` → per-object type choice → plain put (with
   `CounterNumPerDefined`, divided shares, `UpTo`, adapt / monstrosity / tribute gates).
4. All puts are collected in one counter table and applied **together at the end**
   (`replaceCounterEffect`): replacement effects (doubling, "one fewer") apply per object, and
   "counters put" triggers see one event per object. `RemovePhase`, `RememberCards` follow.
5. Renown, mentor and tribute keyword abilities piggy-back on this class (flags set from the keyword).

```
SVar:TrigPutCounter:DB$ PutCounter | Defined$ Self | CounterType$ P1P1 | CounterNum$ 1
SVar:DBPutLoyalty:DB$ PutCounter | Choices$ Planeswalker.YouCtrl | CounterType$ LOYALTY | CounterNum$ X | SubAbility$ DBCleanup   (The Elderspell)
SVar:TrigCounter:DB$ PutCounter | ValidTgts$ Creature | CounterType$ P1P1 | CounterNum$ 3 | TargetMin$ 1 | TargetMax$ 3 | DividedAsYouChoose$ 3   (Glint Weaver)
```

## 6. Cleanup — 2783 cards, 3083 lines — `CleanUpEffect`

Bookkeeping only; no game-visible effect. Clears the scratch state stored on the host card (§0.4).
In apnap this corresponds to "end of the local variable's scope" and normally translates to nothing.

| Key | Uses | Meaning |
|---|---|---|
| `Defined` | rare | The card whose state is cleared (first defined card). Default: the host. |
| `ClearRemembered$ True` | 2758 | Clear the remembered list (also on the card's current game-state object, in case the host is an LKI copy). |
| `ForgetDefined$ <Defined>` | 6 | Remove just these entities from the remembered list. |
| `ClearImprinted$ True` | 217 | Clear imprinted cards. |
| `ClearChosenCard$ True` | 244 | Clear chosen cards. |
| `ClearChosenPlayer$ True` | 39 | Clear the chosen player. |
| `ClearChosenType$ True` | 4 | Clear chosen type (both slots). |
| `ClearChosenColor$ True` | 11 | Clear chosen colours. |
| `ClearNamedCard$ True` | 52 | Clear named cards. |
| `ClearTriggered$ True` | 2 | Remove the **delayed triggers** this card has registered. (This one is a real game effect.) |
| `ClearCoinFlips$ True` | 2 | Clear stored coin-flip results. |
| `Log$ True` | 2 | Write the values being cleared to the game log. |

Not cleared by any key: the chosen number, SVars set by StoreSVar, noted counters.

```
SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True
SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True | ClearChosenCard$ True
```


## 7. DealDamage — 2575 cards, 2911 lines — `DamageDealEffect` (base `DamageBaseEffect`, empty)

A source deals non-combat damage to chosen permanents and/or players.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `NumDmg` | 2911 | amount | required | Damage dealt to **each** recipient (or the amount divided, see below). Zero or less → that recipient is skipped. |
| `ValidTgts` … | 2081 | targeting (`Any`, `Creature`, `Player,Planeswalker`, …) | — | Recipients. |
| `Defined` | 825 | Defined entities (cards and players) | `Self` | Recipients when not targeting. |
| `DamageSource` | 192 | Defined cards | `Self` (the host) | The source(s) of the damage. Each defined source deals the damage ("target creature you control deals damage equal to its power to …": `DamageSource$ ParentTarget`). No source found → nothing happens. The source's last-known information is used if it has left its zone. |
| `DividedAsYouChoose` | 74 | amount | — | Framework: damage divided among targets at announcement; each target receives its share. |
| `DividerOnResolution` | 1 | Defined player | — | That player divides `NumDmg` among the recipient *cards* at resolution (uses the combat-damage assignment dialog). |
| `DivideEvenly` | 1 | `RoundedDown` | — | Each recipient gets `NumDmg / count`, rounded down. |
| `CardChoices`, `PlayerChoices` | 4/4 | Valid cards on the battlefield; Defined players | — | Non-targeted recipients chosen by the activator at resolution, from the union of both lists. |
| `ChoiceAmount` | rare | amount | 1 | Exactly this many are chosen. |
| `Random` | 3 | `True` | — | With choices: picked at random (and each pick is remembered on the host). |
| `ChoicePrompt` | | text | | Prompt. |
| `OptionalDecider` | 6 | Defined player | — | That player may decline the whole damage ("you may have it deal …"). |
| `RelativeTarget` | 2 | `True` | — | Evaluate `Defined` relative to each damage source instead of the host. |
| `Radiance` | 3 | `True` | — | Also each other creature sharing a colour with the target. |
| `ExcessDamage` | 6 | Defined entity | — | Deal only lethal damage to the card (taking deathtouch on the source into account) and the **excess to that entity instead** (first defined). |
| `ExcessDamageCondition` | 1 | Valid (tested on the source) | — | The redirection of excess applies only if the source matches. |
| `RememberRedirectedExcess` | 1 | `True` | — | Remember the excess amount (a number). |
| `ExcessSVar` | 22 | SVar name | — | (damage table) after the damage is dealt, store the excess damage dealt to the recipients in that SVar of the ability. |
| `ExcessSVarCondition` | 7 | Valid (tested on the damaged object) | — | Only count excess on matching recipients. |
| `DamageMap` | 70 | `True` | — | Do not deal the damage yet: accumulate it in a table shared with the following sub-abilities; a later `DB$ DamageResolve` deals everything simultaneously. |
| `NoPrevention` | 12 | `True` | — | (replacement handler) the damage can't be prevented. |
| `NoRedirection` | 1 | `True` | — | (replacement) the damage can't be redirected. |
| `RememberDamaged` | 34 | `True` | — | (game action) remember each entity actually dealt damage by the host as source. |
| `RememberAmount` | 1 | `True` | — | (game action) remember the total damage dealt. |
| `ReplaceDyingDefined` … | 45 | §0.7 | — | "If that creature would die this turn, exile it instead." |
| `CombatDamage` | 1 | | | UNVERIFIED — not read by this class. |

Behaviour:

1. Sources resolved first; an "assign deal damage" replacement event is run for each (lets other
   effects change who is treated as dealing the damage — UNVERIFIED which cards use it).
2. Amount computed once. Recipients determined (choices or targets/defined). `OptionalDecider`.
3. For each source and recipient: cards must still be the same object, **on the battlefield** and
   not phased out; players always qualify. Entries go into a damage table.
4. Unless `DamageMap` is in use, the whole table is dealt **simultaneously** as one damage event
   (prevention/replacement per entry, then lifelink, infect, "dealt damage" triggers in one batch).
5. `replaceDying`.

`DealDamage | DamageMap$ True | SubAbility$ …DealDamage… | SubAbility$ DamageResolve` is how "deals
4 damage to any target and 2 damage to you" is made simultaneous.

```
A:SP$ DealDamage | ValidTgts$ Any | NumDmg$ 4 | DamageMap$ True | SubAbility$ DBDealDamage   (Psionic Blast)
SVar:DBDamage:DB$ DealDamage | ValidTgts$ Creature.YouDontCtrl | NumDmg$ X | DamageSource$ ParentTarget   (Finishing Move)
```

## 8. Mana — 2181 cards, 2574 lines — `ManaEffect` (+ `AbilityManaPart`)

Adds mana to players' mana pools. The parameters are parsed into an `AbilityManaPart` when the
ability is built. Whether the ability is a *mana ability* (no stack) is decided by the framework
(activated, no targets, could add mana, not a loyalty ability, does not move cards to/from a
library — CR 605.1a); a `DB$ Mana` inside a triggered ability is a mana ability only when the
trigger is "whenever … tapped for mana" (CR 605.1b). UNVERIFIED: the exact Forge predicate.

### `Produced$` forms

| Form | Uses | Meaning |
|---|---|---|
| mana letters separated by spaces: `C`, `G`, `R R`, `C R`, `W U B R G` | ~1500 | Exactly that mana. With `Amount$ n` the whole group is produced *n* times (`G` × 3 = GGG). |
| a number | rare (default `1`) | That much generic/colourless mana, multiplied by `Amount`. |
| `Any` | 515 | The chooser picks **one** colour; `Amount` mana of that single colour ("add three mana of any one colour"). |
| `Combo <colours>` e.g. `Combo R G` | ~470 | `Amount` mana, each of which may be any of the listed colours ("add {R} or {G}"; with `Amount$ 2` "two mana in any combination of {R} and/or {G}"). |
| `Combo Any` | 40 | `Amount` mana in any combination of colours. |
| `Combo AnyDifferent` | 5 | `Amount` mana, each of a different colour. |
| `Combo ColorIdentity` | 6 | Any combination of colours in the controller's commander colour identity; **no commander / colourless identity → produces nothing**. |
| `Combo ColorID`, `Combo NotedColors` | 1 each | Colours marked on the host / noted during a draft. |
| `Chosen` (alone, or inside a Combo: `Combo W Chosen`) | 32 | The colour chosen for the host (`ChooseColor`). |
| `Special EnchantedManaCost` | 1 | Mana equal to the enchanted permanent's mana cost (hybrid symbols chosen). |
| `Special EachColoredManaSymbol_<Defined>` | 1 | One mana for each coloured mana symbol in those cards' costs. |
| `Special EachColorAmong_<Defined or Valid …>` | 4 | One mana of each colour among those cards. |
| `Special DoubleManaInPool` | 1 | For each mana in the chooser's pool, one more of the same type. |
| `Special LastNotedType` | 2 | The mana string remembered on the host. |

### Parameters

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Produced` | 2574 | see above | `1` | What is produced. |
| `Amount` | 674 | amount | 1 | Multiplier / number of mana. If the ability's conditions are not met the amount is 0. A non-positive amount produces nothing. |
| `Each` | 2 | amount | 1 | Combo only: each colour choice yields this many mana. |
| `Defined` / `ValidTgts` | 52/6 | Defined players | `You` | Who gets the mana (`Defined` wins over targets). |
| `Chooser` | 1 | Defined player | the recipient | Who chooses colours. |
| `Optional` | 1 | `True` | — | Activator may decline. |
| `RestrictValid` | 197 | `,` list of spell/ability filters (`Spell.Creature`, `Activated.Artifact+inZoneBattlefield`, `Spell.Kicked` …) and the special words `nonSpell`, `CumulativeUpkeep`, `CostContainsX`, `CostContainsC`, `CantPayGenericCosts`, `CantCastSpellFrom<Zone>`, `CantCastNonArtifactSpells` | unrestricted | The mana may be spent only on a matching spell/ability/cost ("spend this mana only to cast creature spells"). |
| `AddsNoCounter` | 3 | `True` or a filter | — | A spell this mana is spent on can't be countered. |
| `AddsKeywords` + `AddsKeywordsValid` + `AddsKeywordsUntil` | 6 | keywords; spell filter; `UntilEOT` | — | A matching spell this mana is spent on gains the keywords (e.g. haste). |
| `AddsCounters` | 4 | `<Valid>_<CounterType>_<n>` | — | A matching permanent spell paid with this mana enters with additional counters. |
| `TriggersWhenSpent` | 14 | SVar (a trigger) | — | That trigger fires when the mana is spent. |
| `PersistentMana` | 25 | `True` | — | The mana does not empty from the pool as steps and phases end. |
| `CombatMana` | 2 | `True` | — | Mana that exists only during combat (firebending-style). |
| `AmountDesc` | 1 | text | | Description. |

Behaviour: for each recipient in the game, colour choices are made (the cost-payment code may have
pre-selected them so that the player is not asked while paying), the mana string is generated and
added to the pool with its restrictions attached; then the "tapped for mana" event fires once for
the **root** ability with everything produced (this is what `TapsForMana` triggers see).
A root `AB$ Mana` is undoable (can be rolled back while paying) unless it has sub-abilities, a
non-numeric `Amount`, several recipients or an `ActivationLimit`.

```
A:AB$ Mana | Cost$ T | Produced$ G | SpellDescription$ Add {G}.
A:AB$ Mana | Cost$ 5 T | Produced$ Combo Any | Amount$ 5   (Cascading Cataracts)
A:AB$ Mana | Cost$ T | Produced$ C | RestrictValid$ Spell.Instant,Spell.Sorcery   (Vodalian Arcanist)
```

## 9. GainLife — 1728 cards, 1838 lines — `LifeGainEffect`

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `LifeAmount` | 1838 | amount | required | Life gained by each player. |
| `Defined` | 1025 | Defined players | `You` | Who gains (wins over targets). A player listed twice gains twice. |
| `ValidTgts` … | 52 | player targeting | — | Target players gain. |

Each player still in the game gains the amount (through the player's gain-life routine, where
"can't gain life" and replacement effects apply). No remember keys.

```
SVar:DBGainLife:DB$ GainLife | Defined$ You | LifeAmount$ 3   (Poison Arrow)
```

## 10. Effect — 1724 cards, 1970 lines — `EffectEffect`

Creates an **effect object**: an invisible pseudo-card in the command zone that carries static
abilities, triggered abilities, replacement effects and/or activated abilities for a duration. This
is how Forge scripts every continuous or delayed effect that is *not* a simple P/T/keyword change:
"until end of turn, you may play that card", "prevent the next 3 damage", "target creature can't be
blocked this turn", "that permanent doesn't untap during its controller's next untap step",
emblems, boons, "the next spell you cast this turn …".

### Parameters

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `StaticAbilities` | 1267 | `,` SVar names (each an `S:`-style `Mode$ …` line) | — | Static abilities of the effect object; active while it is in the command zone. |
| `ReplacementEffects` | 449 | `,` SVar names (`Event$ …`) | — | Replacement effects of the effect object. |
| `Triggers` | 354 | `,` SVar names (`Mode$ …`) | — | Triggered abilities of the effect object (function from the command zone). |
| `Abilities` | 12 | `,` SVar names (`AB$ …`) | — | Activated abilities the effect's controller can activate. |
| `RememberObjects` | 1197 | Defined entities (` & `) | — | Stored as the effect object's *Remembered* list. The carried abilities refer to them as `Card.IsRemembered` / `Player.IsRemembered` / `Remembered`. This is the binding from "target creature" to the effect. |
| `RememberSpell` | 1 | `,` Defined spell abilities | — | Remember spells on the stack. |
| `RememberLKI` | 2 | `,` Defined cards | — | Remember LKI copies. |
| `RememberKeywords` (+ `SharedKeywordsZone`, `SharedRestrictions`) | rare | `,` keywords | — | Remember keyword strings (optionally only shared ones). |
| `ImprintCards` | 47 | Defined cards | — | Stored as the effect's imprinted cards. |
| `NoteCounterDefined` | 1 | Defined cards | — | Note those cards' counters on the effect. |
| `SetChosenNumber` | 22 | amount | host's chosen number | The effect's chosen number (a way to lock in X). |
| `Duration` | 758 | §0.5 | until end of turn | When the effect object is removed. `Permanent` = never by time (emblems; effects that are removed by the triggers below). |
| `EffectOwner` | 54 | Defined players | activator | One effect object is created **per owner**; the owner is "you" for its abilities. |
| `ForgetOnMoved` | 447 | zone | — | When a remembered card leaves that zone, forget it; when nothing remains remembered, remove the effect. Unless the zone is `Stack` (or `ForgetOnCast$ False`), a remembered card being *cast* also counts. |
| `ExileOnMoved` | 505 | zone | — | When a remembered card leaves that zone, remove the whole effect. |
| `ForgetOnPhasedIn` | 6 | `True` | — | Forget a remembered card when it phases in. |
| `ForgetCounter` | 18 | counter type | — | Only remember cards that have this counter; forget a card when its last such counter is removed (or it phases out). |
| `ForgetOnCast` | 34 | Valid card (or `False`, see above) | — | With no remembered objects: remove the effect when a matching spell is cast ("the next instant or sorcery spell you cast …"). |
| `ExileOnLost` | 1 | `True` | — | Remove the effect when its owner loses the game. |
| `ExileOnCounter` | rare | counter type | — | Remove the effect when the last such counter is removed from the source. |
| `Unique` | 1 | `True` | — | Skip owners who already have an effect object with the same name. |
| `Name` | 191 | text | `<host>'s Effect` | Name. A name starting with `Emblem` makes it an **emblem** (colourless, CR 114). |
| `Boon` | 24 | `True` | — | Marks a boon: its trigger fires once and then the effect is removed (Arena mechanic). |
| `ImprintOnHost` | 7 | `True` | — | The host imprints the effect object (so the host can find/remove it later). |
| `AtEOT` | 2 | §0.6 | — | Delayed trigger on the *host*. |
| `Image`, `Adventure`, `Stackable` | | | | Cosmetic / AI. |
| `ValidTgts` / `Defined` | 355/8 | | | Not read by the class itself; exist so `RememberObjects$ Targeted` has something to refer to. |

### Behaviour

1. `checkValidDuration` — may abort.
2. If `RememberObjects` / `RememberLKI` yields nothing and the effect's lifetime depends on
   remembered objects (`ForgetOnMoved`, `ExileOnMoved`, `ForgetCounter`, `ForgetOnPhasedIn`), **no
   effect is created**.
3. For each owner: create the object (timestamp = now; colours of the host; SVars = the creating
   ability's SVars, so the carried abilities can use `X` etc. — evaluated later, in the effect's
   context, unless locked with `SetChosenNumber`), attach abilities, remember/imprint, add the
   lifetime triggers, **copy the host's chosen colour / card / player / direction / type / named
   card / number** onto it, register the expiry, move it to the command zone.
4. The effect's "source" is the creating ability/host (`EffectSource` in filters and Defined).

The carried lines use the ordinary static/trigger/replacement grammar; `EffectZone$ Command` is
implied. Typical shapes:

- "can't be blocked this turn": `StaticAbilities$ Unblockable` with
  `Mode$ CantBlockBy | ValidAttacker$ Card.IsRemembered`;
- "you may play it this turn": `StaticAbilities$ STPlay` with `Mode$ Continuous | MayPlay$ True |
  Affected$ Card.IsRemembered | AffectedZone$ Exile` plus `ForgetOnMoved$ Exile`;
- "doesn't untap during its controller's next untap step": `ReplacementEffects$ DontUntap`.

```
SVar:DBEffect:DB$ Effect | RememberObjects$ Targeted | ForgetOnMoved$ Battlefield | ReplacementEffects$ DontUntap | Duration$ AsLongAsInPlay   (Somnophore)
A:SP$ Effect | Triggers$ IslandTrigger | SpellDescription$ Until end of turn, whenever a player taps an Island for mana, that player adds an additional {U}.   (High Tide)
```

## 11. Tap — 1472 cards, 1522 lines — `TapEffect`

Taps permanents. More than half of all uses (840 lines) are the body of the "enters tapped"
replacement effect.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` | 991 | Defined cards | `Self` | Cards to tap (targets take precedence). |
| `ValidTgts` … | 511 | targeting | — | Targets to tap. |
| `ETB` | 840 | `True` | — | The card is entering the battlefield (used with `Defined$ Self` / `ReplacedCard` in a `Moved` replacement): set its tapped flag directly so it **enters tapped** — no "becomes tapped" event. |
| `CardChoices` | 13 | Valid cards on the battlefield | — | Non-targeted: the activator chooses which to tap. |
| `ChoiceAmount` | 13 | amount | 1 | Maximum (and, without `AnyNumber`, exact) number chosen. |
| `AnyNumber` | 13 | `True` | — | Minimum 0. |
| `ChoicePrompt` | 1 | text | | Prompt. |
| `Tapper` | 2 | Defined player | activator | The player who taps them (for "whenever you tap …"). |
| `RememberTapped` | 13 | `True` | — | Remember the cards that were untapped and get tapped by this. |

Behaviour: skip phased-out cards and cards that are no longer the same object; a card on the
battlefield is tapped (already-tapped cards produce no event); one "tapped (all)" trigger event is
fired with the set of cards actually tapped.

```
SVar:ETBTapped:DB$ Tap | Defined$ Self | ETB$ True   (every tapped land)
SVar:TrigTap:DB$ Tap | CardChoices$ Creature.YouCtrl+untapped | AnyNumber$ True | ChoiceAmount$ Count$Valid Creature.YouCtrl+untapped | RememberTapped$ True | SubAbility$ DBPump   (Ledev Champion)
```

## 12. Destroy — 1439 cards, 1572 lines — `DestroyEffect`

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `ValidTgts` … | 1363 | targeting | — | Permanents to destroy. |
| `Defined` | 210 | Defined cards | `Self` | Permanents to destroy when not targeting. |
| `NoRegen` | 107 | present | — | "It can't be regenerated." Presence test — `NoRegen$ False` still disables regeneration. |
| `RememberDestroyed` | 20 | `True` | — | **Clears the remembered list first**, then remembers each permanent actually destroyed. |
| `RememberLKI` | 12 | `True` | — | Remember the destroyed permanent's last known information (as it was on the battlefield). |
| `Radiance` | 1 | `True` | — | Also each other permanent sharing a colour. |

Behaviour: cards are ordered per owner (graveyard order choice); each must be on the battlefield and
still the same object; destruction fails for indestructible permanents and when a replacement
(regeneration shield, totem armor, …) applies — "destroyed" is true only if it actually happened.
All resulting zone changes are reported as one batch, so "dies" triggers see them as simultaneous.
In a replacement-effect body the cause is the original cause.

```
A:SP$ Destroy | ValidTgts$ Creature | SpellDescription$ Destroy target creature.
SVar:TrigDestroy:DB$ Destroy | ValidTgts$ Creature.White | NoRegen$ True   (Phyrexian Bloodstock)
```

## 13. LoseLife — 1117 cards, 1238 lines — `LifeLoseEffect`

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `LifeAmount` | 1238 | amount | required | Life lost by each player. |
| `ValidTgts` … | 194 | player targeting | — | Target players (take precedence over `Defined`). |
| `Defined` | 852 | Defined players | `You` | Players losing life. |

Behaviour: each player still in the game loses the amount; the life **actually lost** is summed and
stored in the ability's SVar **`AFLifeLost`** (as `Number$n`), which a following sub-ability can
read ("you gain life equal to the life lost this way"). One "life lost (all)" trigger event is fired
for the batch.

```
SVar:TrigDrain:DB$ LoseLife | ValidTgts$ Opponent | LifeAmount$ 2 | SubAbility$ DBGainLife   (Vraska's Conquistador)
```

## 14. Discard — 1043 cards, 1124 lines — `DiscardEffect`

Players discard cards. `Mode$` is **required** and selects who chooses and what is discarded.

| `Mode` | Uses | Meaning |
|---|---|---|
| `TgtChoose` | 822 | Each affected player chooses the cards to discard from their own hand. |
| `Hand` | 110 | Each affected player discards their whole hand. |
| `RevealYouChoose` | 83 | The player reveals their hand; the **activator** chooses. |
| `Random` | ~60 | `NumCards` cards at random among those matching `DiscardValid`. |
| `Defined` | 27 | The specific cards given by `DefinedCards$` are discarded. |
| `LookYouChoose` | rare | The activator looks at the hand (not revealed to others) and chooses. |
| `RevealTgtChoose` | rare | The hand is revealed and the first *targeted* player chooses; the discarders are `Defined` (or the targets). |
| `RevealDiscardAll` | rare | Reveal the hand, then discard every card matching `DiscardValid`. |

Other modes are composed from the prefixes `Reveal` (hand revealed to all) / `Look` (shown to the
chooser) and the suffixes `YouChoose` (activator chooses) / `TgtChoose` (discarding player chooses).

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` | 694 | Defined players | `You` | Discarding players. |
| `ValidTgts` … | 276 | player targeting | — | Target players discard. |
| `NumCards` | 767 | amount | 1 | Number to discard; capped at the hand size. |
| `DiscardValid` | 99 | Valid | `Card` | Which cards may be chosen (`Card.nonLand` for "a nonland card"). |
| `DiscardValidDesc` | 21 | text | | Description. |
| `Optional` | 58 | `True` | — | `Hand` / `Random` / `Defined`: the player may refuse entirely. Choice modes: the minimum becomes 0. |
| `AnyNumber` | 22 | `True` | — | Choose any number (0 … all valid cards). |
| `UnlessType` | 15 | `,` types | — | With `TgtChoose`: "discard `NumCards` cards unless you discard a <type> card" — one card of the type satisfies the whole discard. |
| `RevealNumber` | 11 | amount | — | The player first chooses that many cards from their hand to show; the chooser picks only among those. |
| `DefinedCards` | 27 | Defined cards | — | For `Mode$ Defined`. |
| `DiscardMessage` | 1 | text | | Prompt for an optional `Defined` discard. |
| `RememberDiscarded` | 147 | `True` | — | (in `Player.discard`) remember each card discarded — the new object in its destination zone. |
| `RememberDiscardingPlayers` | 7 | `True` | — | Remember each player who was processed (even if they discarded nothing). |

Behaviour:

1. For every player (APNAP order) the set to discard is determined; a player who "can't discard by
   this ability" is skipped; empty hands are skipped in the reveal/choose modes.
2. Only after **all** players have chosen are the cards discarded (CR 101.4): per player, each card
   is discarded individually (replacement effects such as madness apply per card), then one
   "discarded all" event per player, then the zone-change batch for everything.
3. Minimum for the choice modes is `min(valid cards, NumCards)` — a player must discard as many as
   they can.

```
A:SP$ Discard | ValidTgts$ Player | NumCards$ 2 | Mode$ TgtChoose | SpellDescription$ Target player discards two cards.
SVar:DBDiscard:DB$ Discard | Defined$ You | NumCards$ 2 | Mode$ TgtChoose | UnlessType$ Creature   (Thirst for Identity)
SVar:TrigDiscard:DB$ Discard | ValidTgts$ Player | Mode$ RevealYouChoose | Optional$ True | DiscardValid$ Card.nonLand   (Cracked Skull)
```

## 15. PumpAll — 970 cards, 1074 lines — `PumpAllEffect`

Pump for every card matching a filter ("creatures you control get +1/+1 and gain trample until end
of turn").

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `ValidCards` | 1074 | Valid filter (may start with `Remembered…`, `Triggered…`, `Targeted…` like ChangeType) | all cards | Which cards are affected. |
| `NumAtt`, `NumDef` | 715/520 | signed amount | 0 | P/T change (no `Double`/`Triple` here). |
| `KW` | 479 | ` & ` keywords; `HIDDEN ` prefix allowed | — | Keywords granted. |
| `Duration` | 53 | §0.5, `Permanent`, `Perpetual` | until end of turn | Duration. |
| `PumpZone` | 35 | zone list | `Battlefield` | Zones whose cards are considered. |
| `ValidTgts` / `Defined` | 18/3 | players | — | If present, only cards in those **players'** zones are considered; otherwise all players'. |
| `SharedKeywordsZone`, `SharedRestrictions` | 4 | as Pump | — | Restrict `KW` to keywords shared with matching cards. |
| `RememberPumped` | 9 | `True` | — | Remember each affected card. |
| `AtEOT` | 1 | §0.6 | — | Delayed trigger on the affected cards. |
| `ReplaceDyingDefined/Valid` | | §0.7 | | |

Behaviour: the affected set is determined **once, at resolution** (CR 611.2c — creatures arriving
later are not affected, creatures that stop matching keep the bonus); amounts are locked in; one
timestamp for the whole effect; each card gets its own expiry registration.

```
SVar:TrigPump:DB$ PumpAll | ValidCards$ Creature.YouCtrl | NumAtt$ +2 | NumDef$ +2 | KW$ Trample   (Decimator of the Provinces)
```


## 16. Dig — 928 cards, 1008 lines — `DigEffect`

"Look at / reveal the top N cards of a library, put some of them in zone 1 and the rest in zone 2."
Also the standard script for **"exile the top card of your library"** (`DigNum$ 1 | ChangeNum$ All |
DestinationZone$ Exile`, about half of all uses), impulse draw, "put the top N cards into your
graveyard / hand" without the word *mill* or *draw*. Always counts as moving library cards.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `DigNum` | 1008 | amount | required | How many cards from the top (fewer if the zone is smaller). |
| `Defined` / `ValidTgts` | 346/58 | Defined players | `You` | Whose library (`Defined` wins over targets). The effect runs once per player. |
| `SourceZone` | 2 | zone | `Library` | Zone dug into. |
| `FromBottom` | 2 | `True` | — | Take from the bottom instead. |
| `ChangeNum` | 929 | amount, `All`, `Any` | 1 | How many of the dug cards go to zone 1: exactly that many (as many as possible), `All` = every valid card (no choice), `Any` = any number. |
| `ChangeValid` | 396 | Valid (may contain `ChosenType`) | any card | Which of the dug cards may go to zone 1. |
| `Optional` | 269 | `True` | — | The minimum number chosen becomes 0 ("you may"). |
| `DestinationZone` | 709 | zone | `Hand` | Zone 1. |
| `DestinationZone2` | 211 | zone | `Library` | Zone 2 (the rest). |
| `LibraryPosition` | 97 | integer (`0` top, `-1` bottom) | `-1` | Position for zone 1 if it is a library. Note the default differs from ChangeZone. Parsed as a literal integer. |
| `LibraryPosition2` | 94 | integer | `-1` (bottom) | Position for the rest. |
| `RestRandomOrder` | 209 | `True` | — | The rest goes in a random order. |
| `SkipReorder` | 17 | `True` | — | The rest keeps its order (no ordering prompt). |
| `DestZone2Optional` | 2 | `True` | — | The player may leave the rest where it is. |
| `Reveal` | 169 | `True` | — | The dug cards are revealed to all players (otherwise only the chooser looks at them). |
| `RevealOptional` | 3 | `True` | — | The player may reveal them. |
| `NoLooking` | 5 | `True` | — | Nobody looks (blind move). |
| `NoReveal` | 84 | `True` | — | Cards picked with a `ChangeValid` restriction are normally revealed; this suppresses it. |
| `ForceRevealToController` / `ForceReveal` | 9/2 | `True` / Defined player | — | Show the cards to the activator / that player. |
| `Choser` (sic) | 9 | Defined players | activator | Who picks the zone-1 cards (activator chooses among several). `SetChosenPlayer$ True` stores that player as the host's chosen player. |
| `RandomChange` | 2 | `True` | — | Zone-1 cards are picked at random. |
| `WithTotalCMC` | 3 | amount | — | Picks are limited by total mana value. |
| `ForEachColorPair`, `WithDifferentPowers` | 1/1 | `True` | — | One card of each two-colour pair / cards with pairwise different powers. |
| `Tapped` | 33 | `True` | — | Zone-1 cards enter tapped. |
| `GainControl` | 1 | `True` | — | Zone-1 cards enter under the activator's control. (The code path reorders using the *whole* dug list; treat as UNVERIFIED/buggy.) |
| `Attacking`, `Blocking` | 7 | as ChangeZone | — | Enter attacking / blocking. |
| `WithCounters`, `WithCountersAmount` | 2 | type; amount (1) | — | Enter the battlefield with counters. |
| `ExileFaceDown` | 55 | `True` | — | Zone-1 cards are turned face down (no auto-reveal). |
| `WithMayLook` | 30 | `True` | — | Activator may look at them (also shows the dug cards to the activator). |
| `ExileWithCounters` | 5 | counter type | — | One counter of that type on each exiled card (zone 1 or 2). |
| `FaceDown` (+ `FaceDownPower` …) | 1 | | — | Enter face down as a creature. |
| `RememberChanged` | 427 | `True` | — | Remember the cards moved to zone 1. |
| `RememberMovedToZone` | 2 | string containing `1` and/or `2` | — | Remember cards moved to zone 1 / zone 2. |
| `RememberRevealed`, `ImprintRevealed` | 9/1 | `True` | — | Remember / imprint **all** dug cards. |
| `Imprint` | 19 | `True` | — | Imprint zone-1 cards. |
| `ForgetOtherRemembered` | 2 | `True` | — | Clear remembered before remembering. |
| `PromptToSkipOptionalAbility`, `OptionalAbilityPrompt`, `PrimaryPrompt` | 9/5/5 | | | With `Optional`: first ask whether to use the ability at all (declining ends the effect — the rest is **not** moved). Prompts. |
| `DefinedExiler` | 3 | §0.7 | | |

Behaviour, per player:

1. Take the top `DigNum` cards. Reveal / look as configured; remember/imprint "revealed".
2. Candidates for zone 1 = dug cards matching `ChangeValid` (and the mana-value budget).
3. Choose the zone-1 cards (all / random / chooser picks between min and max, where
   `max = min(candidates, ChangeNum)` and `min = max` unless `Optional` or `Any`).
   Cards chosen under a `ChangeValid` restriction are revealed unless `NoReveal`/`ExileFaceDown`.
4. Move zone-1 cards (ordered by their owners for battlefield/library), with the enter modifiers.
5. Move the rest to zone 2: for a library or graveyard destination in an order chosen by the chooser
   (library) or owner (graveyard), or random / unchanged.
6. After all players: one zone-change batch; exile counters.

If the zone holds no cards nothing happens. Because zone 2 defaults to **bottom of library**,
"look at the top card; you may put it on the bottom" style scripts set `DestinationZone$ Library |
LibraryPosition$ -1 | Optional$ True | DestinationZone2$ Library | LibraryPosition2$ 0`.

```
SVar:DBExile:DB$ Dig | Defined$ You | DigNum$ 1 | ChangeNum$ All | DestinationZone$ Exile | RememberChanged$ True | SubAbility$ DBEffect   (Urza, Lord High Artificer)
SVar:DBDig:DB$ Dig | DigNum$ 4 | Reveal$ True | ChangeNum$ All | ChangeValid$ Card.NamedCard | DestinationZone2$ Graveyard | SubAbility$ DBCleanup   (Wood Sage)
```

## 17. Animate — 920 cards, 1053 lines — `AnimateEffect` (base `AnimateEffectBase`)

Changes characteristics of specific cards for a duration: types, colours, base P/T, keywords and
granted abilities. Used for man-lands ("becomes a 3/3 Elemental creature"), "loses all abilities",
"becomes an artifact in addition to its other types", "gains '{T}: …'", perpetual changes.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` / `ValidTgts` | 648/320 | cards | `Self` | Affected cards (targets win; targeted spells count as their cards). |
| `Power`, `Toughness` | 439/429 | amount | unchanged | **Set** base power / toughness (layer 7b), computed once. |
| `Types` | 530 | `,` list of super/card/sub types; `ChosenType` / `ChosenType2` | — | Types added. If the list contains `ChosenType` it is replaced *entirely* by the host's chosen type. |
| `RemoveTypes` | 6 | `,` types | — | Specific types removed. |
| `RemoveSuperTypes`, `RemoveCardTypes`, `RemoveSubTypes`, `RemoveLandTypes`, `RemoveCreatureTypes`, `RemoveArtifactTypes`, `RemoveEnchantmentTypes` | 173 (creature) / 56 (card) / 29 (land) / 14 | `True` | — | Remove that whole category first, so the added types are "instead of" rather than "in addition to". |
| `AddAllCreatureTypes` | 11 | `True` | — | Becomes every creature type. |
| `Colors` | 174 | `,` colour names, `ChosenColor`, `All` | — | Colours added … |
| `OverwriteColors` | 155 | `True` | — | … or, with this, colours **replaced**. |
| `Keywords` | 321 | ` & ` keywords (an entry naming an SVar is replaced by its value) | — | Keywords granted. |
| `RemoveKeywords` | 10 | ` & ` keywords | — | Keywords removed. |
| `HiddenKeywords` | 6 | ` & ` | — | Hidden keywords (internal rule sentences). |
| `CantHaveKeyword` | rare | keyword | — | Can't have that keyword. |
| `Abilities` | 46 | `,` SVars (`AB$ …`) | — | Activated abilities granted. |
| `Triggers` | 149 | `,` SVars (`Mode$ …`) | — | Triggered abilities granted. |
| `Replacements` | 17 | `,` SVars (`Event$ …`) | — | Replacement effects granted. |
| `staticAbilities` (lower-case s) | 67 | `,` SVars | — | Static abilities granted. |
| `sVars` | 10 | `,` SVar names | — | SVars copied onto the card (needed by granted abilities). |
| `RemoveAllAbilities` | 35 | `True` | — | Loses all abilities (and all keywords). |
| `RemoveNonManaAbilities` | 1 | `True` | — | Loses everything except mana abilities. |
| `RemoveThisAbility` | 14 | `True` | — | Loses the ability that is resolving. |
| `Duration` | 499 | §0.5, `Permanent`, `Perpetual`, `UntilAnimatedFaceup` | until end of turn | Duration. `Permanent` (352) is needed for anything lasting indefinitely. |
| `RevertCost` | 13 | mana cost | — | Adds an ability "pay cost: end this effect". |
| `Name` | 4 | text | — | Changes the card's name. |
| `ManaCost`, `Incorporate` | 1/6 | mana cost | — | Perpetual mana-cost change / incorporate (Arena). |
| `LeaveBattlefield` | 4 | zone | — | Leaves-the-battlefield replacement (to that zone). |
| `Optional`, `OptionQuestion` | 6 | | — | Activator may decline. |
| `AtEOT` | 35 | §0.6 | — | Delayed trigger on the affected cards. |
| `RememberAnimated` | 1 | `True` | — | Host remembers each animated card. |
| `RememberObjects`, `ImprintCards` | 4/3 | Defined | — | Stored on the **animated card** (not the host). |
| `TransferActivator` | 1 | `True` | — | Granted abilities may be activated only by the current activator. |

Behaviour: one timestamp; each card must be the same object and not phased out; all changes are
registered under that timestamp in their own layers (type 4, colour 5, abilities 6, P/T set 7b) and
removed together when the duration ends. A crew ability is an Animate; resolving it marks the
vehicle "crewed". No zone restriction: Animate works on cards in any zone (stack, graveyard …).

```
A:AB$ Animate | Cost$ 1 G | Defined$ Self | Power$ 3 | Toughness$ 3 | Types$ Creature,Ape | Colors$ Green | OverwriteColors$ True | Keywords$ Trample   (Treetop Village)
SVar:DBPump:DB$ Animate | Defined$ Targeted | HiddenKeywords$ CARDNAME can't attack or block. | RemoveAllAbilities$ True | Duration$ UntilHostLeavesPlay   (Opportunistic Dragon)
```

## 18. Sacrifice — 809 cards, 906 lines — `SacrificeEffect`

Players sacrifice permanents of their choice; also "sacrifice CARDNAME" and the bodies of echo and
cumulative upkeep.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `SacValid` | 578 | Valid filter, or `Self` | `Self` | What must be sacrificed. `Self` = the host itself (special path). With `SacEachValid`, several filters joined by ` & `. |
| `Defined` / `ValidTgts` | 378/80 | players | `You` | The players who sacrifice (targets win). Not used for `Self`. |
| `Amount` | 149 | amount | 1 | How many each player sacrifices. |
| `Optional` | 61 | `True` | — | "May sacrifice": the player is asked; minimum 0. |
| `StrictAmount` | 4 | `True` | — | All-or-nothing: with fewer than `Amount` available nothing is sacrificed; with `Optional` the minimum stays `Amount`. |
| `SacEachValid` | 4 | `True` | — | Sacrifice `Amount` permanents for **each** of the ` & `-separated filters (the choices are constrained so later categories stay satisfiable). |
| `Random` | 1 | `True` | — | Chosen at random. |
| `Destroy` | 4 | `True` | — | The chosen permanents are destroyed instead of sacrificed ("destroys a creature of their choice"); "can't be sacrificed" is then not checked. |
| `RememberSacrificed` | 160 | `True` | — | Remember the LKI of each permanent actually sacrificed (for `Self`: the host). |
| `SacMessage` | 63 | text | | Prompt text. |
| `Echo` | — | cost | — | Echo body: the activator may pay the cost; if paid (or the host changed controller) nothing is sacrificed. Fires the pay-echo trigger. |
| `CumulativeUpkeep` | — | cost | — | Cumulative-upkeep body: add an age counter, then pay the cost once per age counter or sacrifice. |
| `UnlessCost` | 154 | §0.3 | | "Sacrifice CARDNAME unless you pay …" — the commonest use of `UnlessCost`. |

Behaviour:

- `Self`: only if the host is on the battlefield **and controlled by the activator**; optional
  confirmation; sacrificed.
- Otherwise, for each player: candidates = that player's permanents that were on the battlefield
  **when the ability started resolving** (last-state snapshot), matching the filter and able to be
  sacrificed; the player chooses exactly `Amount` (or all if fewer). All of a player's choices are
  sacrificed together; players are processed in APNAP order, each choosing and sacrificing before
  the next (note: not "all choose, then all sacrifice").
- Devour and exploit keyword abilities run through this class and record what was sacrificed.
- One zone-change batch at the end.

```
SVar:TrigSac:DB$ Sacrifice | SacValid$ Creature.nonElf | Defined$ TriggeredPlayer   (Ruthless Winnower)
SVar:TrigUpkeep:DB$ Sacrifice | UnlessPayer$ You | UnlessCost$ W W   (Haazda Shield Mate)
```

## 19. Charm — 806 cards, 844 lines — `CharmEffect`

Modal spells and abilities ("Choose one —"). The effect class does nothing on resolution: the modes
are chosen **when the spell/ability is put on the stack** (`PlaySpellAbility` calls `makeChoices`,
CR 601.2b / 603.3c), and the chosen modes are spliced into the ability as its sub-ability chain.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Choices` | 844 | `,` list of SVar names, each a `DB$` ability | required | The modes, in printed order. Each mode has its own targets, description, and may have its own `SubAbility` chain. |
| `CharmNum` | 217 | amount | 1 | Maximum number of modes. |
| `MinCharmNum` | 155 | amount | = `CharmNum` | Minimum number of modes ("choose one or both" = `MinCharmNum$ 1 \| CharmNum$ 2`; "up to" = `0`). |
| `CanRepeatModes` | 24 | `True` | — | The same mode may be chosen more than once. |
| `ChoiceRestriction` | 28 | `ThisGame`, `ThisTurn`, `YourLastCombat` | — | Modes already chosen in that period (tracked on the host) are not available. |
| `Random` | 5 | `True` / `Compare` (+ `RandomCompareSVar`, `RandomCompare`) | — | Modes are chosen at random (or only when the comparison holds). |
| `Chooser` | 3 | (any value) | activator | An **opponent** chosen by the activator picks the modes. |
| `Optional` | 1 | `True` | — | Activator may decline to choose at all (the ability is then not put on the stack). |
| `Pawprint` | 6 | integer | — | Budget for modes that carry their own `Pawprint$ n` weight. Enforced by the mode-choice dialog — UNVERIFIED in engine code. |
| `AdditionalDescription` | 59 | text | | Description only. |

Rules behaviour:

- A mode is **not offered** if it requires targets and has no legal target candidate.
- If fewer modes are available than `MinCharmNum` (no repeats) the ability cannot be cast/activated
  at all; `CharmNum` is capped at the number of available modes.
- The chosen modes are copied and appended in **printed order** (not the order chosen), so they
  resolve in printed order (CR 608.2c). Each copy defaults its stack text to its `SpellDescription`.
- For a triggered ability, choosing no mode removes the trigger from the stack.
- A copy of a modal spell keeps the original's modes (no new choice).
- Entwine: all modes are chained without choice. Mode sub-abilities may carry `ModeCost$ <cost>`
  (Spree / tiered), added to the total cost for each chosen mode by the cost-adjustment code.
- The pseudo-APIs `CompanionChoose`, `InternalLegendaryRule`, `InternalIgnoreEffect` reuse this class.

```
A:SP$ Charm | Choices$ DBExile,DBDraw,DBCounters   (Abzan Charm; each DB… SVar is one mode with its own ValidTgts)
A:SP$ Charm | MinCharmNum$ 1 | CharmNum$ 2 | Choices$ DBDamage,TrigPumpAll   (Deafening Clarion)
```

## 20. ChangeZoneAll — 572 cards, 676 lines — `ChangeZoneAllEffect`

Moves **every** card matching a filter from the origin zones to a destination, without choices
("exile all graveyards", "return all creatures to their owners' hands", "shuffle your graveyard
into your library"). Also the usual way to move "all remembered cards".

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Origin` | 676 | zone list | required | Zones scanned. |
| `Destination` | 676 | zone | required | Destination. |
| `ChangeType` | 663 | Valid (prefix forms as in ChangeZone) | all cards | Filter. |
| `ValidTgts` / `Defined` | 76/47 | players | — | If present, only those players' zones are scanned; with neither (or with `UseAllOriginZones$ True`, 32) every player's. |
| `TypeLimit` | 1 | amount | — | Only the first N matching cards. |
| `LibraryPosition` | 68 | integer | 0 (top) | Position in the library. |
| `Shuffle` | 84 | present | — | The affected players shuffle afterwards (presence test). |
| `RandomOrder` | 46 | `True` | — | Random order into a library. |
| `DefinedPlayer` | 3 | Defined player | `You` | Who orders cards going to a library when neither random nor shuffled. |
| `GainControl` | 22 | `True` | — | Cards go **onto the battlefield** under the activator's control (this branch always moves to the battlefield). |
| `Tapped`, `FaceDown`, `WithCountersType`, `WithCountersAmount` | 17/1/2 | | — | Battlefield entry modifiers (the counter amount is evaluated relative to each card). |
| `ExileFaceDown` | 12 | `True` | — | Moved cards turned face down. |
| `Optional`, `OptionQuestion` | 3 | | — | Activator may decline everything. |
| `RememberChanged` | 92 | `True` or a Valid filter | — | Remember moved cards (all, or those matching the filter, e.g. `Card.!token`). |
| `RememberLKI` | 5 | `True` | — | Remember LKI copies before moving. |
| `ForgetChanged`, `ForgetOtherRemembered`, `Imprint` | 14/11/5 | `True` | — | As ChangeZone. |
| `Duration` | 7 | `UntilHostLeavesPlay` | — | As ChangeZone ("until … leaves the battlefield"). |
| `AtEOT` | 2 | §0.6 | — | Delayed trigger for the moved cards. |

Behaviour: collect, filter, order (owners order their own cards, CR 401.4 / graveyard order), move
each (Exile checks "can be exiled" and records exiled-with), one simultaneous zone-change batch,
then AtEOT / Duration / shuffle. The set is determined at resolution.

```
SVar:DBReturn:DB$ ChangeZoneAll | Origin$ Graveyard | Destination$ Battlefield | ChangeType$ Card.IsRemembered | SubAbility$ DBCleanup   (Gruesome Menagerie)
```

## 21. Mill — 570 cards, 601 lines — `MillEffect`

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `NumCards` | 588 | amount | 1 | Cards milled per player; ≤ 0 → nothing. |
| `Defined` / `ValidTgts` | 355/162 | players | `You` | Who mills (targets win). |
| `Destination` | 0 | zone | `Graveyard` | Where the cards go (e.g. `Exile` for "exile the top N cards" worded as mill-like). |
| `Optional` | 27 | `True` | — | Each player may decline; a player with fewer than `NumCards` cards in library is **skipped** when optional. |
| `RememberMilled` | 112 | `True` | — | Remember the milled cards (as they are in the destination). |
| `Imprint` | 6 | `True` | — | Imprint them. |
| `ForgetOtherRemembered` | rare | `True` | — | Clear remembered first. |

All players' cards are milled in one action and one zone-change batch (mill replacement effects and
"milled" triggers apply in the game action). A library with fewer cards mills what it has.

```
SVar:DBMill:DB$ Mill | Defined$ You | NumCards$ 2 | RememberMilled$ True | SubAbility$ DBPutCounter   (Grist, the Plague Swarm)
```

## 22. Counter — 514 cards, 540 lines — `CounterEffect`

Counters spells and abilities on the stack. When the ability targets, the target zone is forced to
the stack.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `TargetType` | 468 | `Spell`, `Activated`, `Triggered` (`,` list) | — | Targeting grammar: which kinds of stack objects may be targeted. |
| `ValidTgts` | 468 | Valid tested on the **source card** of the stack object (`Card`, `Card.nonCreature`, `Card,Emblem`) | — | Targets. |
| `TargetValidTargeting` | 21 | Valid | — | Targeting grammar: only spells/abilities that themselves target something matching. |
| `Defined` | 72 | Defined spell abilities (`TriggeredSpellAbility`, `Targeted`, `TriggeredSourceSA`) | — | Non-targeted: the spell(s)/abilities to counter. |
| `Destination` | 45 | `Graveyard`, `Exile`, `Hand`, `Battlefield`, `TopOfLibrary`, `BottomOfLibrary` | `Graveyard` | Where a countered **spell** goes. `Battlefield` = under the countering player's control. Ignored for abilities. |
| `DestinationChoice` | 1 | `,` list of the above | — | The activator chooses the destination. |
| `UnlessCost` | 157 | §0.3 | | "Counter target spell unless its controller pays {N}" (default payer: the target's controller). |
| `Optional` | 1 | `True` | — | Activator may decline (ends the whole effect). |
| `ConditionWouldDestroy` | 1 | Valid | — | Counter only if the targeted ability would destroy a matching permanent. |
| `DestroyPermanent` | 2 | `True` | — | If an *ability* is countered, also destroy its source. |
| `RememberCountered` | 12 | `True` | — | Remember the source card of each countered object (only if actually countered). |
| `RememberCounteredCMC` | 14 | `True` | — | Remember the mana value of the targeted spell (even if countering then fails). |
| `RememberForCounter` | 4 | `True` | — | Remember the targeted spell's card before attempting. |
| `RememberCounteredSA` | 1 | `True` | — | Remember the spell ability object. |
| `RememberSplicedOntoCounteredSpell` | 1 | `True` | — | Remember cards spliced onto it. |

Behaviour per stack object: skip if it is a spell that can't be countered or is no longer on the
stack; run the *counter* replacement event (if replaced, nothing else happens); remove from the
stack; a spell's card moves to the destination (an exile destination is checked against "can't be
exiled"); the "countered" trigger fires. Countering an ability just removes it.

```
A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | SpellDescription$ Counter target spell.
A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | UnlessCost$ 3   (Geistlight Snare)
```

## 23. Untap — 470 cards, 496 lines — `UntapEffect`

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` / `ValidTgts` | 231/200 | cards | `Self` | Permanents to untap (targets win). |
| `UntapUpTo` | 16 | `True` | — | Choice mode: each `Defined` **player** untaps up to `Amount` permanents matching `UntapType`. |
| `UntapExactly` | 2 | `True` | — | Same, but exactly `Amount`. |
| `UntapType` | 18 | Valid | — | Filter for the choice modes (evaluated over the whole battlefield, so include a controller restriction if needed). |
| `Amount` | 18 | amount | — | Number for the choice modes. |
| `ETB` | 7 | `True` | — | Entering-the-battlefield variant: clear the tapped flag without an untap event ("enters untapped"). |
| `Radiance` | 1 | `True` | — | Also cards sharing a colour. |

Behaviour: phased-out and changed objects are skipped; only permanents on the battlefield untap;
"can't untap" is respected by the card's untap routine; one "untapped (all)" trigger event for the
batch. Choice modes offer only permanents that can untap.

```
SVar:TrigUntap:DB$ Untap | UntapUpTo$ True | UntapType$ Land | Amount$ 7   (Great Whale)
```

## 24. DelayedTrigger — 449 cards, 492 lines — `DelayedTriggerEffect`

Creates a delayed triggered ability (CR 603.7). The ability line itself **is** the trigger
definition: every parameter of the line is handed to the trigger parser, so it uses the trigger
grammar (`Mode$`, `Phase$`, `ValidPlayer$`, `ValidCard$`, `Origin$`, `Destination$`, … — see the
trigger reference) plus the keys below. `Cost` and `SpellDescription` are stripped first.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Mode` | 492 | trigger mode (`Phase` 395, `ChangesZone` 47, `SpellCast` 40, …) | required | The triggering event. |
| `Execute` | 492 | SVar name (`DB$ …`) | — | The effect of the delayed trigger. Copied at creation and detached from the creating chain. |
| `TriggerDescription` | 390 | text | `SpellDescription` | Text. |
| `RememberObjects` | 317 | Defined entities (` & `) | — | Objects stored **on the trigger**; the trigger's condition refers to them as `Card.IsTriggerRemembered` / `Player.IsTriggerRemembered`, its effect as `DelayTriggerRemembered` / `DelayTriggerRememberedLKI`. |
| `RememberNumber` | 4 | `True` | — | Copy the numbers remembered on the host onto the trigger. |
| `RememberSVarAmount` | rare | SVar name | — | Evaluate that SVar now and store the number on the trigger (locks in X). |
| `ThisTurn` | 91 | `True` | — | The trigger exists only until end of turn ("the next time … this turn", "at end of combat this turn"). |
| `NextTurn` | 58 | `True` | — | The trigger starts existing at the next turn's beginning and lasts for that turn. |
| `UpcomingTurn` | 2 | `True` | — | Registered at the next cleanup, without an end. |
| `DelayedTriggerDefinedPlayer` | 9 | Defined player | — | The trigger is held back and only registered when that player next takes a turn (`PhaseHandler` hands it over as the turn starts); with `ThisTurn` it then lasts for that turn. Used for "at the beginning of *that player's* next upkeep / end step". |
| `Static` | 12 | `True` | — | Trigger grammar: the effect happens immediately when the event occurs, without using the stack. |
| `OptionalDecider` | 2 | player | — | Trigger grammar: "you may". |

Behaviour:

- The trigger is created from a copy of the creating ability's context (controller = activator,
  source = the host as it is now) and is **not tied to any zone**: it fires even if the source has
  left the battlefield.
- It fires **once** and is then removed (the first time its event matches), unless removed earlier
  by `ThisTurn` expiry or `Cleanup | ClearTriggered$ True`. If its controller leaves the game it is
  removed.
- Its `Execute` effect resolves as a normal triggered ability on the stack, with the delayed
  trigger's remembered objects available.
- If the executed effect is SetState, the host's transformation timestamp is stored so a later
  "transform it" does nothing if it transformed meanwhile.

The shorthand `AtEOT$` (§0.6) generates the same thing for the common cases.

```
SVar:DelTrig:DB$ DelayedTrigger | Mode$ Phase | Phase$ End of Turn | Execute$ TrigBounce | TriggerDescription$ Return exiled permanent to the battlefield. | RememberObjects$ RememberedLKI | SubAbility$ DBCleanup   (Glimmerpoint Stag)
SVar:DelTrigSlowtrip:DB$ DelayedTrigger | NextTurn$ True | Mode$ Phase | Phase$ Upkeep | ValidPlayer$ Player | Execute$ DrawSlowtrip | TriggerDescription$ Draw a card.   (the "slowtrip" cantrips)
```

## 25. Scry — 424 cards, 455 lines — `ScryEffect`

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `ScryNum` | 442 | amount | 1 | N. |
| `Defined` / `ValidTgts` | 8/2 | players | `You` | Who scries (targets win). |
| `Optional` | 4 | `True` | — | Each player may decline. |

All the players scry "simultaneously" through one game action (APNAP order of decisions; each looks
at the top N of their library, puts any number on the bottom in any order and the rest on top in any
order; the scry replacement/trigger events are in the game action). Scry 0 produces no event —
UNVERIFIED in the action code.

```
SVar:DBScry:DB$ Scry | ScryNum$ 2
```

## 26. DamageAll — 396 cards, 435 lines — `DamageAllEffect`

One source deals the same damage to every matching permanent and/or player, simultaneously.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `NumDmg` | 435 | amount | required | Damage to each. |
| `ValidCards` | 377 | Valid | **no permanents** | Permanents on the battlefield that are damaged. Absent → no permanents at all. |
| `ValidPlayers` | 168 | Defined players | none | Players damaged. |
| `ValidTgts` | 13 | player targeting | — | Restricts `ValidCards` to permanents **controlled by the first targeted player** (it does not damage that player; add `ValidPlayers$ Targeted` for that). |
| `DamageSource` | 19 | Defined card | `Self` | The source (first defined card; its LKI). |
| `ValidDescription` | 217 | text | | Description. |
| `RememberDamaged` | 10 | `True` | — | (game action) remember everything dealt damage. |
| `ReplaceDyingDefined/Valid` | 6 | §0.7 | | |
| `DamageMap` | — | | | Participates in an enclosing damage map if the parent set one (cannot start one itself). |

```
A:AB$ DamageAll | Cost$ B | NumDmg$ 1 | ValidCards$ Creature | ValidPlayers$ Player | ValidDescription$ each creature and each player.   (Pestilence)
A:SP$ DamageAll | ValidCards$ Creature.withoutFlying | ValidPlayers$ Player | NumDmg$ X   (Earthquake)
```


## 27. ChooseCard — 347 cards, 440 lines — `ChooseCardEffect`

A player chooses cards **without targeting**; the result is stored as the host's `ChosenCard` list
(read later with `Defined$ ChosenCard`, `Card.ChosenCard`) and optionally remembered. No game
action by itself — it is the first half of "choose a creature you control … then do X to it".

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` / `ValidTgts` | 212/27 | players | `You` | The choosing player(s); each chooses in turn (`Defined` wins). A chooser who has left the game is replaced by a player picked by the activator (CR 800.4g). |
| `Choices` | 407 | Valid | all cards in the zone | Which cards may be chosen (filter evaluated for the activator). |
| `ChoiceZone` | 209 | zone list | `Battlefield` | Where the candidates are (all players' zones). |
| `AllCards` | 2 | `True` | — | Candidates = every card in the game. |
| `DefinedCards` | 19 | Defined cards | — | Candidates = exactly these (overrides zone and `Choices`). |
| `IncludeSpellsOnStack` | 5 | `True` | — | Add cards on the stack to the candidates. |
| `ControlledByPlayer` | 49 | `Chooser`, `Left`, `Right`, Defined players | — | Restrict candidates to those controlled by the choosing player / the neighbour / those players. ("Each player chooses a creature they control" = `Defined$ Player \| ControlledByPlayer$ Chooser`.) |
| `Amount` | 170 | amount, or `Random` | 1 | Maximum number chosen (`Random` = a random number 0…count). ≤ 0 → the chosen list is set to empty. |
| `MinAmount` | 42 | integer | = `Amount` | Minimum number. |
| `Mandatory` | 267 | `True` | — | The player cannot decline. Without it the chooser may choose fewer / none. |
| `AtRandom` | 66 | `True` | — | Chosen at random. |
| `ChooseEach` | 11 | types joined by ` & `, or `Party` | — | One card of each listed type (optional unless `Mandatory`). |
| `EachBasicType` | 3 | `True` | — | One card of each basic land type present. |
| `WithTotalPower` | 2 | amount | — | Any creatures the player controls with total power ≤ N. |
| `WithDifferentPowers`, `WithDifferentManaValues` / `WithDifferentCMC`, `EachDifferentPower` | rare | `True` | — | Pairwise-different power / mana value; one per distinct power. |
| `ControlAndNot` | 1 | `True` | — | One selection among cards the player controls and one among cards they don't. |
| `Reveal` | 48 | `True` | — | Reveal the chosen cards. |
| `Secretly` | 4 | `True` | — | With `Reveal`: all players' choices are revealed together after everyone has chosen. |
| `ChosenMap` | 3 | `True` | — | Also record *who chose what* (read by `Defined$ ChosenMap` in CopyPermanent etc.). |
| `RememberChosen` | 82 | `True` | — | Add the chosen cards to remembered. |
| `ForgetChosen` | 13 | `True` | — | Remove them from remembered. |
| `ImprintChosen` | 19 | `True` | — | Imprint them. |
| `ForgetOtherRemembered` | 3 | `True` | — | Clear remembered first. |
| `ChoiceTitle`, `ChoiceTitleAppend`, `RevealTitle`, `ChoiceDesc` | | text | | Prompts. |

The chosen list **replaces** any previous `ChosenCard` value. Choosing is not targeting: hexproof,
shroud and protection do not matter, and the choice is made on resolution.

```
SVar:TrigChoose:DB$ ChooseCard | ChoiceZone$ Hand | Choices$ Creature.YouOwn | ChoiceTitle$ You may choose a creature card in your hand | SubAbility$ DBAnimate   (Guildpact Greenwalker)
SVar:ChooseYou:DB$ ChooseCard | Choices$ Permanent.YouCtrl | Amount$ 1 | AtRandom$ True | RememberChosen$ True | SubAbility$ DBExchange   (Karona, False God Avatar)
```

## 28. CopyPermanent — 334 cards, 392 lines — `CopyPermanentEffect` (base `TokenEffectBase`)

Creates token copies of permanents/cards (CR 707). Shares the token pipeline of §4 — all of
`TokenTapped`, `TokenAttacking`, `TokenBlocking`, `AttachedTo`, `AttachAfter`, `WithCountersType`,
`WithCountersAmount`, `PumpKeywords`, `PumpDuration`, `AtEOT`, `AtEOTTrig`, `RememberTokens`,
`RememberOriginalTokens`, `ImprintTokens`, `RememberSource`, `TokenRemembered`, `AddTriggersFrom`,
`ChangeZoneTable` work as described there.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` / `ValidTgts` | 211/132 | cards | `Self` | What to copy (`Defined` wins). `Defined$ ChosenMap` copies each player's chosen cards for that player. Instants and sorceries are skipped. |
| `NumCopies` | 32 | amount | 1 | Copies of each card. |
| `Controller` | 27 | Defined players | activator | Who creates the tokens; one set per controller. |
| `Choices` (+ `Chooser`, `ChoiceTitle`, `WithDifferentNames`) | 21 | Valid | — | Instead of copying all defined cards: choose one (or any number with different names) among the battlefield / the `Defined` cards matching the filter. |
| `Populate` | 27 | `True` | — | Shorthand: `Choices$ Creature.token+YouCtrl`. |
| `DefinedName` | 19 | card name, or `NamedCard` | — | Create a token that is a copy of the card with that name (from the card database). |
| `ValidSupportedCopy`, `RandomCopied`, `RandomNum` | 4 | | — | Copy random cards from the whole card pool matching a filter (digital-only designs). |
| `ForEach`, `OptionalForEach` | rare | Defined players; `True` | — | One copy per listed player; each token remembers its player. |
| `Optional` | 3 | `True` | — | Activator may decline. |

**Copy modifiers** (read by `CardFactory.getCloneStates`; also used by Clone). These become part of
the token's copiable values (CR 707.9b):

| Key | Uses | Meaning |
|---|---|---|
| `NonLegendary` | 32 | "except it isn't legendary". |
| `AddTypes` | 56 | types (joined by ` & `) added. |
| `SetCreatureTypes` | 11 | creature types replaced (space-separated). |
| `RemoveCardTypes`, `RemoveSubTypes` | 6/4 | loses its card types (and subtypes). |
| `SetPower`, `SetToughness` | 46/46 | base P/T set. |
| `SetColor`, `AddColors`, `SetColorByManaCost` | 15/3 | colours replaced / added. |
| `AddKeywords`, `RemoveKeywords` | 35 | keywords (` & `) added / removed. |
| `AddTriggers`, `AddAbilities`, `AddStaticAbilities`, `AddSVars` | 7/2/2/7 | SVar names of abilities the copy has in addition. |
| `NewName`, `KeepName` | 2 | name changed / kept. |
| `SetManaCost`, `RemoveCost`, `SetLoyalty` | rare | mana cost / loyalty overrides. |
| `GainThisAbility`, `GainTextOf`, `GainTextAbilities`, `Embalm` | rare | "has this ability" (Clone-style), text gain, embalm/eternalize modifications. |

Behaviour: a prototype is built for each copied card from its **current copiable values** (for a
`…LKICopy` defined card, as it last existed), in the same face it currently shows (a transformed
permanent is copied transformed). The tokens then go through the create-token replacement and enter
exactly as in §4. Counters, damage, attachments and non-copy effects are not copied.

```
SVar:DBCopyPerm:DB$ CopyPermanent | Defined$ Remembered | PumpKeywords$ Haste | AtEOT$ Exile | SubAbility$ DBClearNamed   (Gallery of Legends)
SVar:DBCopy:DB$ CopyPermanent | Populate$ True   (Scion of Vitu-Ghazi)
```

## 29. DestroyAll — 328 cards, 371 lines — `DestroyAllEffect`

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `ValidCards` | 371 | Valid (prefix forms allowed; a literal `X` in the filter is replaced by the value of SVar X) | all permanents | What is destroyed. |
| `ValidTgts` | 11 | player targeting | — | Only permanents controlled by the first targeted player. |
| `NoRegen` | 37 | present | — | Can't be regenerated. |
| `NoRegenValid` | 1 | Valid | — | Only matching permanents can't be regenerated. |
| `Optional` | 1 | `True` | — | Activator may decline (not asked if nothing matches). |
| `RememberDestroyed` | 36 | `True` | — | Clear remembered, then remember the LKI of each permanent actually destroyed. |
| `RememberAllObjects` | 1 | `True` | — | Remember everything that matched (before destruction). |
| `ValidDescription` | 6 | text | | Description. |

Behaviour: the set is taken at resolution; indestructible permanents are filtered out; owners order
their cards; each is destroyed (regeneration / replacement may save it); one simultaneous
zone-change batch.

```
A:SP$ DestroyAll | ValidCards$ Creature | RememberDestroyed$ True | SubAbility$ DBTreasure | SpellDescription$ Destroy all creatures.
```

## 30. GainControl — 308 cards, 334 lines — `ControlGainEffect`

Changes control of permanents (a layer-2 effect from a resolved ability).

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `ValidTgts` / `Defined` | 223/94 | cards | `Self` | Permanents whose control changes (`Defined` wins). |
| `AllValid` | 34 | Valid | — | Instead: all permanents on the battlefield matching the filter. |
| `Choices` (+ `Chooser`, `ChoiceTitle`) | 5 | Valid | — | Instead: a player (default activator) chooses exactly one matching permanent. |
| `NewController` | 92 | Defined player | activator (or the targeted player, if the ability targets a player and the key is absent) | Who gains control. |
| `LoseControl` | 178 | `,` list of: `EOT`, `EndOfCombat`, `LeavesPlay`, `LoseControl`, `Untap`, `UntilTheEndOfYourNextTurn`, `UntilSourceUnattached`, `StaticCommandCheck` | **never** (permanent) | When the control effect ends: end of turn / end of combat / when the host leaves play / when the activator loses control of the host / when the host untaps / … Any one of the listed events ends it. |
| `StaticCommandCheckSVar`, `StaticCommandSVarCompare` | 2 | SVar; comparison | — | With `StaticCommandCheck`: ends when the comparison becomes true ("for as long as …"). |
| `Untap` | 114 | `True` | — | Untap the permanent. |
| `AddKWs` | 106 | ` & ` keywords | — | It gains these keywords **until end of turn** (always end of turn, independent of `LoseControl`). |
| `Optional` | 3 | `True` | — | Activator may decline (per permanent). |
| `RememberControlled` | 15 | `True` | — | Remember each permanent taken. |
| `ForgetControlled` | rare | `True` | — | Remove from remembered. |

Behaviour:

1. Pre-checks mirror CR 611.2b: with `LeavesPlay` the host must be on the battlefield, with
   `LoseControl` the activator must still control the host, with `Untap` the host must be tapped —
   otherwise nothing happens.
2. Each permanent must be on the battlefield, not phased out, and controllable by the new
   controller. It receives a timestamped "temporary controller" entry; ending the effect removes that
   entry (control reverts to whatever other effects say). A permanent that "can't change control"
   keeps its controller when the end fires.
3. Note that **`Duration$` is not used** by this API — `LoseControl$` is its own vocabulary.

Threaten effects are `GainControl | LoseControl$ EOT | Untap$ True | AddKWs$ Haste`.

```
A:AB$ GainControl | Cost$ T | ValidTgts$ Creature | LoseControl$ LeavesPlay   (Infernal Denizen)
SVar:DBGainControl:DB$ GainControl | Defined$ Targeted | LoseControl$ EOT | Untap$ True | AddKWs$ Haste | Condition$ Kicked | SubAbility$ DBToken   (Jet's Brainwashing)
```

## 31. ImmediateTrigger — 307 cards, 336 lines — `ImmediateTriggerEffect`

Creates a **reflexive triggered ability** (CR 603.12: "When you do, …"). It is registered as a
delayed trigger with the internal mode `Immediate`, which triggers at the very next opportunity, so
its effect goes on the stack (with its own targets chosen then) after the current ability finishes
resolving.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Execute` | 336 | SVar (`DB$ …`, may have `ValidTgts`) | — | The reflexive ability's effect. |
| `TriggerDescription` | 268 | text | `SpellDescription` | Text. |
| `TriggerAmount` | 18 | amount | 1 | Number of separate reflexive triggers created; ≤ 0 → none. |
| `RememberObjects` | 62 | Defined entities (` & `) | — | Stored on the trigger (`…IsTriggerRemembered`, `DelayTriggerRemembered…`). |
| `RememberEach` | 3 | `True` | — | With `TriggerAmount`: the i-th trigger remembers only the i-th object. |
| `RememberSVarAmount` | 18 | SVar name | — | Evaluate now and store the number on the trigger. |
| `UnlessCost` + `UnlessSwitched$ True` | 22 | §0.3 | | The usual form of "you may pay/sacrifice X. When you do, …". |
| `AfterReplacement` | 1 | `True` | — | Do not trigger while a replacement effect is being applied. |
| other trigger-grammar keys (`OptionalDecider`, `Static` …) | | | | Passed to the trigger parser. |

```
SVar:Pay2:DB$ ImmediateTrigger | UnlessCost$ 2 | UnlessPayer$ You | UnlessSwitched$ True | Execute$ TrigDestroy   (Anthropede)
```

## 32. Play — 302 cards, 341 lines — `PlayEffect`

Lets a player cast spells / play lands **during the resolution** of this ability ("you may cast it
without paying its mana cost", cascade-like effects, "copy that card and cast the copy").

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` / `ValidTgts` | 135/35 | cards | `Self` | The candidate cards (targets win). Cards that changed zones are dropped unless `ZoneRegardless$ True`. |
| `Valid` + `ValidZone` | 161/162 | Valid; zone list (default `Hand`) | — | Instead: candidates are all cards in those zones (all players) matching the filter. |
| `ValidSA` | 275 | `,` list of spell filters (`Spell`, `Instant,Sorcery`, `Spell.cmcLEX`) | any playable ability incl. land play | Restricts which of a card's abilities may be used. `Spell…` = "cast" (no land plays). Cards with no matching ability are removed from the candidates. |
| `Amount` | 145 | amount or `All` | 1 | How many cards may be played. |
| `Optional` | 321 | `True` | — | "May". Without it the cast is mandatory if possible (costs and targets are forced), except that some non-mana costs still make it optional. |
| `Controller` | 147 | Defined player | activator | The player who plays the cards. |
| `WithoutManaCost` | 293 | `True` | — | Cast without paying the mana cost (additional costs still apply; a spell with X whose minimum X > 0 cannot be chosen; X = 0). |
| `PlayCost` | 8 | cost string (may contain `ConvertedManaCost`), `ManaCost`, `SuspendCost` | — | Cast by paying this cost instead of the mana cost (`ManaCost` = its normal mana cost, e.g. for timing-free casting). |
| `PlayReduceCost`, `PlayRaiseCost` | 2 | amount / SVar | — | Cost reduction / increase for the cast. |
| `WithTotalCMC` | 4 | amount | — | Total mana value of the spells cast may not exceed this. |
| `CopyCard` | 58 | `True` | — | Create a copy of the card (in the same zone) and cast the copy; the original stays. The copy ceases to exist when it leaves the stack. |
| `CopyFromChosenName` | 8 | `True` | — | Cast a copy of the card with the named card's name. |
| `AnySupportedCard` (+ `RandomCopied`, `RandomNum`, `ChoiceNum`) | 5 | `Instant`, `Sorcery`, `Names:a,b,…` | — | Cast copies of random / listed cards from the card pool. |
| `CastTransformed` | rare | `True` | — | Cast the back face. A card without one is skipped. |
| `CastFaceDown` | rare | `True` | — | Cast it face down (as a morph). |
| `ReplaceGraveyard` | 35 | zone (always `Exile`) | — | "If that spell would be put into a graveyard, exile it instead" — an until-end-of-turn replacement tied to the cast spell. `ReplaceGraveyardValid` limits it to matching spells. |
| `ReplaceIlluMask` | rare | `True` | — | Illusionary Mask's turn-face-up replacement. |
| `ManaConversion` | 4 | e.g. `AnyType->AnyColor` | — | Mana may be spent as though it were of other types. |
| `ControlledByPlayer` | rare | Defined player | — | That player makes the decisions for the casting player during the cast. |
| `AllowRepeats` | 3 | `True` | — | The same card may be chosen again. |
| `ShowCards`, `ShowCardToActivator` | rare | | — | Extra cards shown / reveal the card to the player. |
| `RememberPlayed`, `ImprintPlayed` | 20/5 | `True` | — | Remember / imprint each card played (the object on the stack / battlefield). |
| `ForgetPlayed` | 21 | `True` | — | Remove the played card from remembered. |
| `ForgetRemembered` | 4 | `True` | — | Clear remembered after a successful play. |

Behaviour:

1. Build the candidate list; apply `ValidSA`; nothing to do if empty.
2. Loop while candidates remain and the amount / mana-value budget allows:
   the controller picks a card (may decline if `Optional`); a face-down card is turned face up to
   look at it; the card's castable abilities are collected (alternative costs such as flashback are
   offered only when no cost override is in force); the player picks which ability/face/mode.
   - A **land** is played immediately by resolving its land ability (no timing check). Whether
     this counts against the player's land plays for the turn is UNVERIFIED (decided inside the
     land ability, not in this class).
   - A **spell** is put on the stack right now, following the normal casting steps (modes, targets,
     costs) while the outer ability is still resolving; it will resolve after it.
     Timing restrictions on the card type are ignored.
3. Successful plays are remembered/imprinted; zone-change triggers for each play fire.

```
SVar:DBPlay:DB$ Play | Defined$ Remembered | ValidSA$ Spell | WithoutManaCost$ True | Optional$ True | ReplaceGraveyard$ Exile | SubAbility$ DBCleanup   (Storm of Memories)
SVar:DBPlay:DB$ Play | Defined$ Remembered | CopyCard$ True | Controller$ You | WithoutManaCost$ True | Optional$ True | SubAbility$ DBCleanup   (Narset, Enlightened Exile)
```


> From here on (ranks 33–60) the tables give usage counts where they were cheap to obtain; value
> distributions were not analysed in depth.

## 33. PutCounterAll — 293 cards, 312 lines — `CountersPutAllEffect`

Puts counters on every card matching a filter.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `ValidCards` | 312 | Valid | required | Cards receiving counters. |
| `ValidZone` | 2 | zone | `Battlefield` | Where to look. |
| `CounterType` | 312 | counter type | required | Kind. |
| `CounterNum` | 302 | amount | 1 | Number on each; ≤ 0 → nothing. |
| `ValidTgts` | 9 | player targeting | — | Only cards controlled by the first targeted player. |
| `Placer` | 7 | `Controller`, `Owner`, or Defined player | activator | Who puts the counters (per card for the first two). |
| `ValidCards2`, `ValidZone2`, `CounterType2`, `CounterNum2` | 7/–/13/6 | | — | A second group handled in the same simultaneous action; each defaults to the first group's value (so `CounterType2` alone = "and a second kind on the same cards"). |
| `AmountByChosenMap` | 1 | `<expr>[ INDEX n]` | — | Per-card amount computed from the placer's chosen card. |
| `RememberPut`, `RememberAmount` | 1/1 | `True` | — | Counter-table keys, see PutCounter. |

All counters are applied as one batch.

```
SVar:TrigCounters:DB$ PutCounterAll | ValidCards$ Creature.counters_GE1_FUSE | CounterType$ FUSE | CounterNum$ 1   (Bomb Squad)
```

## 34. RepeatEach — 286 cards, 363 lines — `RepeatEachEffect`

Loop: resolves the `RepeatSubAbility` chain once for each element of a collection, with the current
element temporarily **remembered on the host** (so the body says `Defined$ Remembered`,
`Player.IsRemembered`, `Card.IsRemembered`). This is how "each player …" / "for each creature …"
effects with per-element choices are written.

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `RepeatSubAbility` | 363 | SVar | The loop body (a full ability chain, resolved with its own conditions). |
| `RepeatPlayers` | 263 | Defined players | Loop over players, APNAP order (`StartingWith$` changes the first player). During an iteration *all previously remembered players are removed* and restored afterwards, so exactly one player is remembered. |
| `RepeatCards` + `Zone` | 54/27 | Valid; zone list (default `Battlefield`) | Loop over matching cards. |
| `DefinedCards` | 36 | Defined cards | Loop over these cards. |
| `RepeatTargeted` | 4 | `True` | Loop over this link's targets. |
| `RepeatSpellAbilities` | 1 | spell filter | Loop over matching stack objects. |
| `RepeatTypesFrom` | 5 | Defined cards or `ThisTurnCast_<Valid>` | Loop over the card types among those cards; the current type is the host's chosen type; order chosen by the player. |
| `UseImprinted` | 43 | `True` | Card loops store the current card as *imprinted* instead of remembered. |
| `ChooseOrder` | 9 | `True` or Defined player | That player chooses the order of the card loop. |
| `RepeatOptionalForEachPlayer`, `RepeatOptionalMessage` | 8 | `True`; text | Each player may decline their iteration. |
| `NextTurnForEachPlayer` | 2 | `True` | Each player's iteration is deferred to the start of that player's next turn. |
| `ClearRemembered` / `ClearRememberedBeforeLoop` | 6/11 | `True` | Clear remembered before starting / before the player loop. |
| `AmountFromVotes` | 5 | `True` | Sets SVar `Votes` for the element from a preceding Vote. |
| `ChangeZoneTable` | 47 | `True` | Zone changes made by the iterations (tokens, copies, …) are collected and their triggers fire once after the loop — the permanents enter **simultaneously**. |
| `DamageMap` | 22 | `True` | Damage from all iterations is collected and dealt simultaneously after the loop. |
| `LoseLifeMap` | 1 | `True` | One "life lost" batch event after the loop. |
| `Optional` + `OptionPrompt` | 1 | | Activator may skip the whole loop. |

Without the three "map" keys each iteration is a complete, separate resolution (state-based actions
are **not** checked in between, but static abilities are refreshed and triggers accumulate).
The several source keys are not exclusive: card loop, then stack loop, then targets, types, players
all run if given.

```
SVar:DBRepeatChoice:DB$ RepeatEach | StartingWith$ You | RepeatSubAbility$ DBChoose | RepeatPlayers$ Player   (Eureka)
```

## 35. SetState — 276 cards, 349 lines — `SetStateEffect`

Changes which face/state a card shows: transform, flip, turn face up / face down, meld back, etc.

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Mode` | 348 | `Transform` (290), `TurnFaceUp` (22), `Flip` (20), `TurnFaceDown` (11), `Unspecialize`, `Specialize`, … | The state change. |
| `Defined` / `ValidTgts` | 289/19 | cards (default `Self`; targets win) | Affected cards. |
| `Choices` (+ `Amount`, `MinAmount`, `Mandatory`, `ChoiceTitle`) | 5 | Valid | Instead: the activator chooses battlefield cards. |
| `NewState` | 1 | state name | Explicit target state. |
| `Optional` | 7 | `True` | Activator may decline (ends the effect). |
| `ETB` | — | `True` | Allow acting on a card that is not yet on the battlefield. |
| `RevealFirst`, `ValidNewFace` | 1 | `True`; Valid | Check the hidden face first; only proceed if it matches. |
| `FaceDownPower`, `FaceDownToughness`, `FaceDownSetType` | 3 | | Characteristics of the face-down state. |
| `RememberChanged` | 14 | `True` | Remember each card whose state changed. |

Rules behaviour: the card must be the same object; except for face-up/down and specialize modes it
must be on the battlefield. `Transform` does nothing if the permanent can't transform (not
double-faced, a token copy without a back, etc.). **A "transform it" executed by a DelayedTrigger is skipped if the host has transformed since
the delayed trigger was created** (DelayedTrigger stores the transform timestamp). This is Forge's
take on CR 701.27g — note that the CR text applies the check to non-delayed abilities and counts
from when the ability went on the stack, so apnap must follow the CR, not Forge, here. Turning face up a face-down card that is not a
permanent card just reveals it and it stays face down. Turning face up through this API runs the
"turned face up" machinery (triggers).

```
SVar:TrigTransform:DB$ SetState | Defined$ Self | Mode$ Transform   (Huntmaster of the Fells)
```

## 36. Regenerate — 266 cards, 272 lines — `RegenerateEffect`

Creates a regeneration shield (CR 701.19) for each affected permanent.

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Defined` / `ValidTgts` | 33/62 | cards, default `Self` (`Defined` wins) | Permanents to shield. Must be on the battlefield and the same object. |
| `RegenerationAbility` | 4 | SVar | Extra effect that happens when the shield is used ("when it regenerates this way, …"). |
| `RememberObjects` | 1 | Defined | Extra objects stored on the shield effect. |

Implementation: one effect object named "<host>'s Regeneration" remembering the permanents, with a
replacement `Event$ Destroy | ValidCard$ Card.IsRemembered | Regeneration$ True` whose body is
`DB$ Regeneration | Defined$ ReplacedCard` (tap, remove from combat, remove damage — see
Regeneration in Part II) and then removes that permanent from the shield. The shield lasts until end
of turn or until used; it is forgotten if the permanent leaves the battlefield. "Can't be
regenerated" (`NoRegen`) destruction skips the replacement.

```
A:AB$ Regenerate | Cost$ 1 G | SpellDescription$ Regenerate CARDNAME.   (Child of Gaea)
```

## 37. Surveil — 245 cards, 249 lines — `SurveilEffect`

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Amount` | 245 | amount, default 1 | N. `0` → nothing happens (no surveil event). |
| `Defined` / `ValidTgts` | 1/– | players, default `You` | Who surveils (targets win). |
| `Optional` | – | `True` | Each player may decline. |
| `RememberMoved`, `RememberKept` | 2/1 | `True` | (read in `Player.surveil`) remember the cards put into the graveyard / the cards kept on top. |

Each player looks at the top N cards, puts any number into the graveyard and the rest back on top in
any order; the cards put into graveyards form one zone-change batch.

## 38. Attach — 236 cards, 247 lines — `AttachEffect`

Attaches an Aura / Equipment / Fortification to a permanent or player. It is also the **spell
effect of every Aura card** (`A:SP$ Attach | ValidTgts$ …`): when the host is an Aura and the
ability is a spell, resolution first attaches and then moves the Aura onto the battlefield.

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `ValidTgts` / `Defined` | 130/113 | card or player | What to attach **to** (`Defined` wins). If several entities are defined the chooser picks one. None → nothing happens. |
| `Object` | 88 | Defined cards | The attachment(s). Default: the host itself. |
| `Choices` (+ `ChoiceZone`, `ChoiceTitle`) | 29 | Valid | Without `Object`: the chooser picks **which attachment** (among matching cards) to attach to the defined entity. With `Object`: the chooser picks **what to attach to** among matching cards the attachment can legally be attached to. |
| `PlayerChoices` | 3 | Defined players | With `Object`: choose a player to attach to. |
| `Move` | 6 | `True` | The current host is excluded from the choices ("attach to another creature"). |
| `Chooser` | 2 | Defined player | Who chooses (default activator). |
| `Optional` | 26 | `True` | The chooser may decline (per attachment). |
| `RememberAttached` | 7 | `True` | Remember attachments that ended up attached. |

Attaching uses the normal legality check (enchant/equip restrictions, protection); an illegal
attach does nothing. Equip abilities are `AB$ Attach | Cost$ … | ValidTgts$ Creature.YouCtrl |
SorcerySpeed$ True` generated from the keyword.

```
SVar:ShadowReattach:DB$ Attach | Defined$ Remembered | Object$ OriginalHost | SubAbility$ DBCleanup   (Shifting Shadow)
```

## 39. CopySpellAbility — 235 cards, 259 lines — `CopySpellAbilityEffect`

Copies spells or abilities on the stack (CR 707.10). Targeting is forced to the stack zone.

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Defined` / `ValidTgts` | 192/67 | Defined spell abilities (`TriggeredSpellAbility`, `Parent`, `Targeted`) | What to copy (targets win). Objects that "can't be copied" are dropped. |
| `Amount` | 34 | amount, default 1 | Number of copies. |
| `Controller` | 35 | Defined players, default `You` | Each listed player gets the copies and controls them. |
| `MayChooseTarget` | 209 | `True` | "You may choose new targets for the copy." Without it the copy keeps the original targets. |
| `Optional` | 11 | `True` | The controller may decline. |
| `SingleChoice` | – | `True` | If several spells are defined, copy only one of them (controller's choice). |
| `CopyForEachCanTarget` | 11 | Valid | One copy for each *other* legal target matching the filter, each copy targeting that object (Radiate / Zada). `ChooseOnlyOne$ True`: a single such copy with a chosen target. |
| `DefinedTarget` | 3 | Defined entities | One copy per defined entity, retargeted to it (a defined player may be replaced by one of their permanents by choice). |
| `IgnoreFreeze` | 3 | `True` | UNVERIFIED (stack-freeze bookkeeping). |
| `Epic` | – | `True` | The copy lacks epic. |
| `RememberCopies` | 3 | `True` | Remember the copies. |
| copy modifiers (`NonLegendary`, `SetPower`, `SetToughness`, `AddTypes`, `SetColor`, `RememberNewCard`, `UseOriginalHost`) | few | see §28 | Applied to copies of permanent spells (the resulting token). |

Behaviour: copies are created (not cast — no "cast" triggers), the *copy-spell replacement event*
may change the number (extra copies may choose new targets), then the controller orders them and
puts them on the stack. A copy of a modal spell keeps its modes; a copy has the same X, kicker
status and other choices.

```
SVar:TrigCopySpell:DB$ CopySpellAbility | Defined$ TriggeredSpellAbility | MayChooseTarget$ True   (Chancellor of Tales)
```

## 40. RemoveCounter — 190 cards, 206 lines — `CountersRemoveEffect`

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `CounterType` | 206 | type, `Any` (activator chooses kinds), `All` (every kind) | Kind. |
| `CounterNum` | 197 | amount (default 1), `All` (all of that kind), `Any` (any number) | How many. |
| `Defined` / `ValidTgts` | 113/39 | cards and/or players (default `Self` for cards, `You` for players; targets win) | Where to remove from. Both the card list and the player list are processed. |
| `Choices` (+ `ChoiceZone`, `ChoiceOptional`, `ChoiceNum`) | 10 | Valid | Instead: the activator chooses the card(s) (exactly 1; 0..all if optional; exactly `ChoiceNum`). |
| `UpTo` | 7 | `True` | The activator chooses how many (0..N) per card. |
| `CounterNumShared` | 1 | `True` | `CounterNum` is a total across all the cards. |
| `Optional` | 2 | `True` | Activator may decline. |
| `RememberRemoved` | 29 | `True` | Remember one entry per counter removed (type + index pairs; counted with `Count$RememberedSize`). |
| `RememberAmount` | 14 | `True` | Remember the total number removed. |

Removes at most as many as are present; skips cards whose counters can't be removed. In a
`Moved`-replacement body only cards that were on the battlefield beforehand are considered.

```
SVar:TrigRemoveCounter:DB$ RemoveCounter | Defined$ Self | CounterType$ M1M1 | CounterNum$ 1   (Magmaroth)
```

## 41. MakeCard — 182 cards, 213 lines — `MakeCardEffect`

Creates brand-new **cards** (not tokens) from the card database and puts them into a zone — the
Arena-only *conjure* / spellbook mechanics (191 of 213 lines), plus a few paper cards.
Keys: `Defined` (players, default You); card source — one of `Name$ <card>|ChosenName`,
`Names$ a,b` (`;` stands for a comma inside a name), `DefinedName$ <Defined cards|ChosenMap>`,
`Spellbook$ a,b,…` / `Choices$ …` (choose `SpellbookAmount`, or `AtRandom`; `Filter$ Valid`;
`SpellbookName`), `Booster$ True`; `Amount` (copies of each, default 1); `Zone` (default
`Library`; no `LibraryPosition` → shuffled in afterwards); `LibraryPosition`; `Tapped`;
`WithCounter` + `WithCounterNum`; `AttachedTo`; `FaceDown`; `TokenCard$ True`; `Reveal`;
`Conjure$ True` (fires the conjure trigger); `RememberMade`, `ImprintMade`; `Optional` +
`OptionPrompt`. Not needed for paper-Magic Commander precons.

## 42. ChooseType — 170 cards, 183 lines — `ChooseTypeEffect`

A player chooses a card type / subtype; stored as the host's `ChosenType` (read by filters such as
`Creature.ChosenType`, by `Types$ ChosenType`, `DefinedKW$ ChosenType` …).

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Type` | 183 | `Creature` (127), `Basic Land` (27), `Card` (18), `Land`, `Nonbasic Land`, `Planeswalker`, `CreatureInTargetedDeck`, `Shared` | Which family of types is offered. |
| `Defined` / `ValidTgts` | 142/1 | players, default `You` | Who chooses. If several players choose, the last choice wins (single slot). |
| `ValidTypes` | 11 | `,` list | Explicit list instead of the family. |
| `InvalidTypes` | 8 | `,` list | Removed from the options. |
| `TypesFromDefined` | 1 | Defined cards | Only the creature types of those cards (or with `Shared`: card types shared by all). |
| `MostPrevalentInDefinedZone` | 2 | `<DefinedPlayer>[_Zone]` | Only the most common creature type(s) there. |
| `AtRandom` | 3 | `True` | Random choice. |
| `ChooseType2` | 2 | `True` | Store in the second slot (`ChosenType2`). |
| `Note` / `ChooseNoted` | 3/1 | `True` | Record the type in the host's noted-types list (types already noted are not offered); only also set the chosen type with `ChooseNoted`. |
| `Secretly` | 1 | `True` | Hidden choice. |

No options → error (script bug) except with `TypesFromDefined`, where nothing happens.

```
SVar:DBChooseType:DB$ ChooseType | Defined$ Player.IsRemembered | Type$ Creature | SubAbility$ DBRemember   (Harsh Mercy)
```

## 43. Clone — 168 cards, 185 lines — `CloneEffect`

A permanent **becomes a copy** of another object (CR 707.2), as opposed to CopyPermanent which
creates a token. Used for Clone-style "enter as a copy" replacement bodies and for "becomes a copy
until end of turn".

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Choices` + `ChoiceZone`, `ChoiceTitle`, `ChoiceOptional` | 78/16 | Valid; zone (default `Battlefield`) | The activator chooses the object to copy. Inside a replacement effect the candidates are restricted to what was in the zone *before* the event (so a Clone cannot copy something entering at the same time). |
| `Defined` | 63 | Defined card (first) | The object to copy. |
| `ValidTgts` | 58 | targeting | Target = the object to copy (or, with `Choices`, the target is the card that *becomes* the copy). |
| `CopyFromChosenName` | 1 | `True` | Copy the card with the named name. |
| `CloneTarget` | 48 | Defined cards | The cards that become copies. Default: the host. |
| `ExcludeChosen` | 2 | `True` | Don't turn the copied object into a copy of itself. |
| `CloneZone` | 2 | zone | Only clone-targets currently in that zone. |
| `Optional` | 13 | `True` | Activator may decline. |
| `Duration` | 64 | §0.5 | **Absent = permanent** (unlike most effects). Present = the copy effect ends then. |
| `IntoPlayTapped` | 3 | `True` | The cloning card is set tapped. |
| `PumpKeywords`, `PumpDuration` | 2 | as Token | Extra non-copiable keywords. |
| `RememberCloneOrigin` | 4 | `True` | The clone remembers what it copied. |
| `ImprintRememberedNoCleanup` | 1 | `True` | Keep the host's remembered/imprinted lists. |
| copy modifiers: `AddTypes` 46, `AddKeywords` 33, `SetPower`/`SetToughness` 25, `GainThisAbility` 24, `AddSVars` 22, `NewName` 16, `NonLegendary` 10, `AddTriggers` 10, `AddAbilities`, `AddStaticAbilities`, `SetColor`, `AddColors`, `RemoveCardTypes`, `RemoveSubTypes`, `KeepName`, `Embalm`, `RemoveCost` | | see §28 | "except it's …" — become part of the copiable values. `GainThisAbility` = "and it has this ability". |

Behaviour: the copy is a timestamped clone state layered on the card (layer 1); the card's
remembered/imprinted lists are cleared (they belonged to its old identity); ending the duration
removes the clone state and restores them. Phased-out cards are skipped.

```
SVar:DBCopy:DB$ Clone | Choices$ Creature.Other | ChoiceZone$ Graveyard   (Body Double)
```

## 44. DigUntil — 167 cards, 188 lines — `DigUntilEffect`

"Reveal cards from the top of your library until you reveal a <X> card. Put that card <somewhere>
and the rest <somewhere else>." Cascade and discover-like effects use it.

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` / `ValidTgts` | 60/20 | players | `You` | Whose library (targets win). |
| `Valid` | 187 | `,` Valid list | `Card` | What counts as "found". |
| `Amount` | 34 | amount | 1 | Stop after this many found cards. 0 → nothing happens. |
| `MaxRevealed` | 1 | amount | whole library | Reveal at most this many cards. |
| `MinTotalCMC` | 3 | amount | — | Instead of `Valid`: reveal until the total mana value revealed reaches N (nothing is "found"). |
| `DigZone` | 4 | zone | `Library` | Zone revealed from. |
| `FoundDestination` | 162 | zone | none (found cards stay with the revealed pile) | Where found cards go. |
| `FoundLibraryPosition` | 3 | amount | 0 | Position if that is a library. |
| `RevealedDestination` | 180 | zone | — | Where the other revealed cards go. |
| `RevealedLibraryPosition` | 84 | amount | 0 | Usually `-1` (bottom). |
| `RevealRandomOrder` | 64 | `True` | — | The rest goes in random order (otherwise the player orders ≥ 2 cards unless shuffling). |
| `NoMoveRevealed` | 8 | `True` | — | The non-found revealed cards are **not moved** (they stay on top of the library in order; a later link deals with them). |
| `NoMoveFound` | 1 | `True` | — | Found cards are not moved (unless the found destination is the battlefield). |
| `NoneFoundDestination`, `NoneFoundLibraryPosition` | 1 | zone; pos | — | Alternative destination for the pile when fewer than `Amount` were found. |
| `Shuffle` | 20 | `True` | — | Shuffle the library afterwards; `ShuffleCondition$ NoneFound` only if nothing was found. |
| `Optional` | 2 | `True` | — | The player may decline to dig. |
| `OptionalFoundMove` + `OptionalNoDestination` | 4/1 | `True`; zone | — | The player may decline to move a found card (it then goes to the alternative zone, or stays with the pile). |
| `GainControl`, `Tapped`, `Attacking`, `Blocking`, `AttachedTo` | 10/10/2/–/1 | | — | Battlefield entry modifiers for found cards (as ChangeZone; control goes to the activator). |
| `RememberFound`, `ImprintFound` | 67/17 | `True` | — | Remember / imprint each found card (as found in the library, before moving). |
| `RememberRevealed`, `ImprintRevealed` | 24/7 | `True` | — | Remember / imprint the non-found revealed cards. |
| `ForgetOtherRemembered` | 3 | `True` | — | Clear remembered when a card is found. |

Behaviour: reveal one by one from the top until satisfied or the library runs out; all revealed
cards are revealed to everyone; found cards move first (all "found" entering the battlefield count
as simultaneous), then the rest. When both destinations are the same zone the cards are moved one
at a time in library order ("put them into your graveyard" while digging). A cascade DigUntil also
runs the cascade replacement event.

```
SVar:TrigExile:DB$ DigUntil | Valid$ Card.nonLand | ValidDescription$ nonland | FoundDestination$ Exile | RevealedDestination$ Exile | RememberFound$ True | SubAbility$ DBPutCounter   (The Tenth Doctor)
```

## 45. PeekAndReveal — 152 cards, 170 lines — `PeekAndRevealEffect`

Look at (and maybe reveal) the top cards of a library **without moving them**; the result is
remembered so a following link can branch on it ("reveal the top card; if it's a land, …").

| Key | Uses | Values | Default | Meaning |
|---|---|---|---|---|
| `Defined` / `ValidTgts` | 48/14 | players | `You` | Whose library (`Defined` wins). The *looking* player is always the activator. |
| `PeekAmount` | 107 | amount | 1 | Number of cards. |
| `SourceZone` | 1 | zone | `Library` | Zone. |
| `NoPeek` | 60 | `True` | — | Skip the private look: the cards are just revealed ("reveal the top card of your library"). |
| `NoReveal` | 49 | `True` | — | Only look, never reveal. |
| `RevealValid` | 37 | Valid | `Card` | Only matching cards can be revealed / remembered. |
| `RevealOptional` | 37 | `True` | — | The activator may choose whether to reveal ("you may reveal it"). |
| `RememberRevealed` | 106 | `True` | — | If revealed: remember LKI copies of the revealable cards. |
| `ImprintRevealed` | 8 | `True` | — | Imprint instead. |
| `RememberPeeked` | 33 | `True` | — | If **not** revealed: remember the matching peeked cards anyway. |

The remembered objects are LKI copies (the card as it is in the library), so later links address the
real card with `Defined$ TopOfLibrary` or `Remembered` filters depending on the script.

## 46. Fight — 148 cards, 154 lines — `FightEffect` (base `DamageBaseEffect`)

Two creatures fight (CR 701.14).

| Key | Uses | Meaning |
|---|---|---|
| `ValidTgts` | 146 | One or two target creatures. |
| `Defined` | 150 | The other fighter(s). Combination rule: with a target and one defined card, **fighter 1 = the defined card, fighter 2 = the target**; with two defined cards and no target they are the fighters; with two targets and no `Defined`, those two. Defined cards must be creatures on the battlefield, same object, not phased out. |
| `Optional` | 2 | The host's controller may decline. |
| `ExcessSVar`, `ExcessSVarCondition`, `ReplaceDyingDefined…`, `DamageMap` | few | As DealDamage. |

Behaviour: fewer than two fighters → nothing (a fight needs both). Each deals damage equal to its
power to the other, simultaneously; a creature fighting itself deals twice its power to itself. Both
are flagged "fought this turn"; a "fights" trigger event per fighter and one for the pair.
The usual script is `Pump (target your creature) → SubAbility Fight | Defined$ ParentTarget |
ValidTgts$ Creature.YouDontCtrl`.

```
SVar:DBFight:DB$ Fight | Defined$ ParentTarget | ValidTgts$ Creature.YouDontCtrl   (Ruthless Predation)
```

## 47. ReplaceEffect — 147 cards, 165 lines — `ReplaceEffect`

Only meaningful as the body (`ReplaceWith$`) of a **replacement effect**: it modifies one parameter
of the event being replaced and lets the modified event continue ("if a source would deal damage,
it deals double that damage instead").

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `VarName` | 164 | name of an event parameter: `DamageAmount` (80), `Affected` (40), `LifeGained` (15), `Amount`, `Number`, `Num`, `Result`, `Ignore`, `IgnoreChosen`, … | Which field of the event to overwrite. |
| `VarValue` | 164 | expression | New value. |
| `VarType` | 44 | `amount` (default), `Card`, `Player`, `GameEntity`, `PlanarDice`, `Map`, `CardSet` | How `VarValue` is interpreted: a number, or the first defined card/player/entity; `Map` adds the amount to a per-player map entry for the `VarKey$` players; `CardSet` adds a card to a set. |
| `VarKey` | 2 | Defined players | Keys for `Map`. |

The result of the replacement is marked *updated*: the event proceeds with the new value and other
replacement effects may still apply (each only once, CR 614.5). Typical uses: damage doubling /
halving / `+1` (`VarName$ DamageAmount | VarValue$ X` with `X:ReplaceCount$DamageAmount/Twice`),
redirecting who is affected (`VarName$ Affected | VarType$ Card|Player`), life-gain modification.
Related bodies: ReplaceDamage (prevention), ReplaceToken, ReplaceCounter, ReplaceMana,
ReplaceSplitDamage (Part II).

```
SVar:DmgTwice:DB$ ReplaceEffect | VarName$ DamageAmount | VarValue$ X   (Dual Doomsuits)
```

## 48. Investigate — 139 cards, 151 lines — `InvestigateEffect` (base `TokenEffectBase`)

Creates Clue tokens (token script `c_a_clue_draw`). Keys: `Num` (32; amount, default 1),
`Defined` / `ValidTgts` (players, default You; targets win), `Optional`,
`RememberInvestigatingPlayers`; plus the token keys of §4 (`RememberTokens` etc.). Investigating N
times is N separate token creations (N "investigate" events, each with its own enter batch); the
player's "investigated this turn" counter is incremented each time.

## 49. GenericChoice — 134 cards, 167 lines — `ChooseGenericEffect`

A player chooses one (or more) of several **effects at resolution** — the non-modal "choose one"
(e.g. "that player sacrifices a creature or discards a card", villainous-style punisher choices,
"an opponent chooses …"). Unlike Charm the choice is made while resolving, so the chosen branches
cannot have targets chosen at cast time.

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Choices` | 167 | `,` list of SVars (`DB$` abilities) | The options; each option's `SpellDescription` is its label. |
| `Defined` / `ValidTgts` | 111/25 | players, default `You` (`Defined` wins) | The choosing player(s); each chooses and their choice resolves before the next player chooses. A player who left the game is replaced (CR 800.4g). |
| `ChoiceAmount` | – | amount, default 1 | Number of options chosen. |
| `FallbackAbility` | 13 | SVar | Resolved if no option is available / chosen. |
| `AtRandom` | 12 | `True` / `Urza` | Random option(s). |
| `NumRandomChoices` | 2 | amount | First reduce the option list to N random options. |
| `TempRemember` | 25 | `True` | While the chosen option resolves, the choosing player is the only remembered player on the host. |
| `ShowChoice` | 48 | `True` / `ExceptSelf` | Announce the choice. |
| `Secretly` | 3 | `True` | Choices are revealed together at the end. |
| `SetChosenMode` | 15 | `True` | Store the chosen option's description as the host's chosen mode. |
| `DamageMap`, `ChangeZoneTable` | 2/1 | `True` | Batch damage / zone changes across all players' choices. |
| `Guess` | 2 | `True` | Bookkeeping for guessing effects. |
| `ChoicePrompt` | 4 | text | Prompt. |

An option is **not offered** if its own restriction keys (`IsPresent`, `CheckSVar`, … on the option
SVar) fail, or if it has an `UnlessCost` the chooser cannot pay — this is how "sacrifice a creature
*or* …" hides impossible options. The chosen option is resolved as a full chain.

```
SVar:DBChoose:DB$ GenericChoice | ValidTgts$ Opponent | Choices$ Self,Others   (May Civilization Collapse)
```

## 50. ChooseColor — 128 cards, 136 lines — `ChooseColorEffect`

A player chooses colour(s); stored as the host's `ChosenColor` list.

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Defined` / `ValidTgts` | 124/– | players, default `You` (targets win) | Who chooses (last chooser wins). |
| `Choices` | 1 | `,` colour names | Restrict the options. |
| `Exclude` | 10 | `,` colour names | Remove options. |
| `ColorsFrom` / `ExcludeColorsFrom` | 1/1 | Defined cards | Options = the colours of those cards (nothing happens if colourless) / remove those colours. |
| `OrColors` | 8 | `True` | Choose one **or more** colours. |
| `TwoColors` | 3 | `True` | Choose exactly two. |
| `UpTo` | 1 | `True` | Minimum 0. |
| `Random` | 5 | `True` | Random. |

Options default to the five colours (never colourless). No colour chosen → the previous value is
kept and the effect ends.

## 51. SacrificeAll — 126 cards, 148 lines — `SacrificeAllEffect`

Sacrifices **every** matching permanent, no choice. Keys: `Defined` (73; exact cards) *or*
`ValidCards` (75; filter over the whole battlefield — **all** permanents if neither is given);
`Controller` (25; Defined players — only permanents they control); `RememberSacrificed` (13;
clears remembered, then remembers LKI of each sacrificed card); `ImprintSacrificed`.
Permanents that can't be sacrificed by this ability are skipped. Note that a permanent is
sacrificed by **its controller**; with `Defined$` pointing at a permanent someone else now controls
it is still sacrificed (the built-in `AtEOT$ Sacrifice` uses `Controller$ You` to prevent that).

```
SVar:SacAllOthers:DB$ SacrificeAll | ValidCards$ Creature.Other+YouCtrl   (Plague Reaver)
```

## 52. AlterAttribute — 123 cards, 135 lines — `AlterAttributeEffect`

Sets or clears a named designation on cards. Keys: `Attributes` (`,` list): `Prepared` (81),
`Suspected`/`Suspect` (26), `Solved`/`Solve` (15; fires the case-solved trigger), `Plotted` (5),
`Saddled`/`Saddle` (3; records the saddling creatures, fires the trigger), `Commander` (3),
`Harnessed` (2); `Activate$ False` (8) clears instead of sets (default true); `Defined` /
`ValidTgts` (cards, default Self; `Defined` wins); `Optional` + `OptionQuestion`;
`RememberAltered` (remember cards whose state actually changed). Skips phased-out / changed objects.

## 53. AnimateAll — 122 cards, 140 lines — `AnimateAllEffect` (base `AnimateEffectBase`)

Animate (§17) for every card matching a filter. Keys: `ValidCards` (140; filter, prefix forms
allowed), `Zone` (25; zone list, default Battlefield), `ValidTgts` / `Defined` (players: restrict to
their zones); and the Animate keys `Power`, `Toughness`, `Types`, `RemoveTypes`, `Remove…Types`,
`AddAllCreatureTypes`, `Colors` (no `All` form here), `OverwriteColors`, `Keywords`,
`RemoveKeywords`, `HiddenKeywords`, `CantHaveKeyword`, `Abilities`, `Triggers`, `Replacements`,
`staticAbilities`, `sVars`, `RemoveAllAbilities`, `RemoveNonManaAbilities`, `Duration` (default
until end of turn), `RevertCost`, `LeaveBattlefield`, `RememberAnimated`. The affected set is fixed
at resolution; there is no `checkValidDuration` pre-check in this class.

## 54. UntapAll — 122 cards, 129 lines — `UntapAllEffect`

Untaps every matching permanent. Keys: `ValidCards` (129; filter, default all), `Defined` /
`ValidTgts` (players: only their permanents; `Defined` wins), `ControllerUntaps` (each permanent's
controller is recorded as the one untapping it; default the activator), `RememberUntapped`
(remember those that actually untapped). One "untapped (all)" event.

## 55. PreventDamage — 113 cards, 122 lines — `DamagePreventEffect`

"Prevent the next N damage that would be dealt to <target> this turn" (a prevention shield,
CR 615.7).

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Amount` | 122 | amount | Size of the shield. |
| `ValidTgts` / `Defined` | 99/22 | cards / players (default `Self`; targets win) | What is protected. Cards must be on the battlefield. |
| `DividedAsYouChoose` | 5 | amount | Shield divided among targets. |
| `CardChoices`, `PlayerChoices`, `Random` | 1 | | Non-targeted / random recipient (as DealDamage). |
| `Radiance` | 1 | `True` | Also cards sharing a colour. |
| `PreventionSubAbility` | 7 | SVar | Effect that happens when damage is prevented (may use the prevented amount). |
| `ShieldEffectTarget` | 7 | Defined entity | Substituted into the sub-ability text as its target ("… deals that much damage to X"). |

Implementation: one effect object per protected entity, remembering it, with SVar `ShieldAmount`
and a damage replacement whose body is `DB$ ReplaceDamage | Amount$ ShieldAmount` (reduces the
shield as it is used). It expires at end of turn and is forgotten when a protected card leaves the
battlefield. "Prevent **all** damage …" is not this API — those are `Effect` with a replacement, or
`Fog`.

```
A:AB$ PreventDamage | Cost$ T | ValidTgts$ Any | Amount$ 1   (Kitsune Healer)
```

## 56. ChoosePlayer — 113 cards, 134 lines — `ChoosePlayerEffect`

A player chooses a player; stored as the host's `ChosenPlayer`.

| Key | Uses | Values | Meaning |
|---|---|---|---|
| `Defined` / `ValidTgts` | 120/4 | players, default `You` (targets win) | Who chooses. |
| `Choices` | 132 | Defined players | Candidates (default: all players in turn order). |
| `Random` | 18 | `True` | Random. |
| `Optional` | 1 | `True` | May decline. |
| `Secretly` | 3 | `True` | Hidden choice (separate slot). |
| `Protect` | – | `True` | Store as "protecting player" instead. |
| `RememberChosen` | 7 | `True` | Also remember the chosen player (`ForgetOtherRemembered` clears first). |
| `ChooseSubAbility` | 1 | SVar | Resolved right after a player was chosen. |
| `CantChooseSubAbility` | 1 | SVar | Resolved instead when no player was chosen (no candidates, or declined). |
| `ChoiceTitle`, `DontNotify` | | | UI. |

## 57. Seek — 110 cards, 121 lines — `SeekEffect`

Arena-only: put a **random** card of a given type from the library into the hand without revealing
or shuffling. Keys: `Type` (116; a Valid filter, default `Card`) or `Types` (`,` list — one seek per
entry), `Num` (29; default 1), `Defined` / `ValidTgts` (players, default You), `DefinedCards`
(pool instead of the library), `RememberFound`, `ImprintFound`. Fires a seek trigger event.

## 58. Branch — 108 cards, 121 lines — `BranchEffect`

If/else. Evaluates `BranchConditionSVar$ <expr>` against `BranchConditionSVarCompare$ <op><value>`
(default `GE1`; operators as in conditions, the operand may be an SVar) and resolves
`TrueSubAbility$` or `FalseSubAbility$` (either may be absent → nothing). The chosen branch is a
full chain resolved in place; afterwards the Branch's own `SubAbility$` continues as usual.
`ValidTgts` on a Branch (12 lines) is only a target carrier for the branches.

```
SVar:TrigBranch:DB$ Branch | BranchConditionSVar$ X | BranchConditionSVarCompare$ GE1 | TrueSubAbility$ DBToken | FalseSubAbility$ DBDraw   (Faramir, Prince of Ithilien)
```

## 59. Proliferate — 94 cards, 100 lines — `CountersProliferateEffect`

Keys: `Amount` (9; number of times to proliferate, default 1). `RememberPut` works via the counter
table. Behaviour: the *proliferate replacement event* may change the count or cancel; then, for each
time: the activator chooses any number of players and permanents that have counters; each chosen
object gets **one more of each kind** of counter it already has (for poison, only one player per
team); the puts are one batch; a proliferate trigger event fires each time — even when nothing was
chosen.

## 60. NameCard — 83 cards, 105 lines — `ChooseCardNameEffect`

A player names a card; the name is **added** to the host's `NamedCard` list (and set as the
player's named card unless random). Keys: `Defined` / `ValidTgts` (players, default You; targets
win); `ValidCards` (55; filter the name must satisfy, evaluated on card *faces*; `cmcEQ<SVar>` and
`ManaCost=Equipped|Imprinted` are substituted) with `ValidDescription`; `AtRandom` (25; random
matching name from the whole card pool); `ChooseFromList$ a,b,c` (14; `;` = comma inside a name);
`ChooseFromDefinedCards$ <Defined>` (names of those cards, both halves of split cards);
`ExcludeChosen` (names already named are excluded); `SelectPrompt`. Requires a database of all
card names — for apnap, the set of names available at runtime.


---

# Part II — every remaining API (short entries)

Format: **Api** — cards / lines — `Class` — what it does — keys the class reads (corpus line counts
in parentheses where non-zero). All of them also accept the framework keys of §0 and
`Defined$`/`ValidTgts$` unless stated otherwise.

**How far these entries are verified.** The key lists were extracted mechanically from each class
(`scratchpad/agent-effects/keys2.py`) and are complete for literal `hasParam`/`getParam` reads in
the class itself (not in base classes or helpers). The descriptions are weaker than in Part I:

- For Reveal, RevealHand, Amass, TapAll, ChooseSource, Goad, Shuffle, AddTurn, Phases,
  MultiplyCounter, Protection, AddPhase, Repeat, StoreSVar, Connive, Debuff, TapOrUntap,
  ChangeTargets, Explore, SetLife, ReplaceCounter, ReplaceToken, ReplaceDamage, MoveCounter,
  ExchangeControl, EachDamage, RemoveCounterAll and Regeneration the parameter-handling lines of
  the class were read (defaults and value vocabularies are confirmed; full control flow is not).
- For **all other entries** the one-line description is inferred from the API/class name, the key
  names, Forge's own docs and the rules meaning of the mechanic. Treat their parameter semantics as
  **UNVERIFIED** until the class is read; entries tagged "(UNVERIFIED)" are those where even the
  general meaning is a guess.

## 61–100

- **Reveal** — 81 / 87 — `RevealEffect` — players reveal cards from their hand (no zone change).
  `NumCards` (6; default 1), `AnyNumber` (19), `Optional` (5), `Random` (21; random cards),
  `RevealValid` (27; which hand cards qualify), `RevealDefined` (23; reveal exactly these cards, any
  zone), `RevealAllValid` (11; reveal every matching hand card), `RevealToAll`, `RevealTitle`,
  `RememberRevealed` (59).
- **RevealHand** — 73 / 77 — `RevealHandEffect` — a player's whole hand is revealed, or with
  `Look$ True` (24) only looked at by the activator. `Optional`, `RevealType` (only cards of a
  type), `RememberRevealed` (22), `ImprintRevealed`, `RememberRevealedPlayer` (3).
- **Amass** — 71 / 73 — `AmassEffect` (TokenEffectBase) — amass N: if you control no Army, create a
  0/0 black Army token; put N +1/+1 counters on an Army you control; it becomes the given type.
  `Num` (73), `Type` (73; e.g. `Zombie`, `Orc`), `RememberAmass` (5); token keys of §4;
  `ChangeZoneTable`.
- **FlipCoin** — 71 / 84 — `FlipCoinEffect` — flip coins, with or without a call.
  `Amount` (6), `NoCall` (11; plain heads/tails, uses `HeadsSubAbility` 8 / `TailsSubAbility` 5),
  otherwise win/lose: `WinSubAbility` (50), `LoseSubAbility` (51); `Flipper` (6), `Caller`,
  `ForEachPlayer`, `FlipUntilYouLose` (5), `SaveNumFlipsToSVar`, `RememberResult`,
  `RememberWinner`, `RememberLoser`, `RememberNumber`. (semantics of the less common keys UNVERIFIED)
- **TapAll** — 71 / 77 — `TapAllEffect` — tap every permanent matching `ValidCards` (77); with
  targets / `Defined` (players) only theirs; `TapperController` (3; each controller is the tapper),
  `RememberTapped` (5). One "tapped (all)" event.
- **ChooseSource** — 66 / 67 — `ChooseSourceEffect` — choose a *source of damage* (a permanent, a
  spell on the stack, an object referred to by something on the stack, or a command-zone object) →
  host's `ChosenCard`. `Choices` (67; Valid), `Amount`, `ChoiceTitle`, `RememberChosen` (3). First
  half of "prevent all damage a source of your choice would deal".
- **RollDice** — 66 / 138 — `RollDiceEffect` — roll dice and branch on the result. `Sides` (79;
  default 6), `Amount` (23), `Modifier` (7), `ResultSVar` (57; store the result),
  `ResultSubAbilities` (65; `lo-hi:SVar,…,Else:SVar`, parsed in AbilityFactory), `ChosenSVar`,
  `OtherSVar`, `IgnoreLower`, `UseHighestRoll`, `UseDifferenceBetweenRolls`, `EvenOddResults`,
  `DifferentResults`, `MaxRollsResults`, `StoreResults`, `SubsForEach`, `NoteDoubles`,
  `RerollResults`, `RememberHighestPlayer`, `ToVisitYourAttractions`. (semantics UNVERIFIED beyond
  the first five.)
- **Goad** — 65 / 69 — `GoadEffect` — goad creatures (CR 701.15: must attack each combat and
  attack a player other than the goader if able). `Defined` (38) / targets, `Duration` (8; default
  **`UntilYourNextTurn`**, `Permanent` allowed), `NoLonger` (1; un-goad), `RememberGoaded` (2).
- **DamageResolve** — 62 / 67 — `DamageResolveEffect` — no parameters. Deals all damage collected in
  the enclosing damage map (`DamageMap$ True` on an earlier link) as one simultaneous event.
- **BecomeMonarch** — 61 / 64 — `BecomeMonarchEffect` — the defined/target player (default You)
  becomes the monarch. No own keys.
- **Shuffle** — 59 / 66 — `ShuffleEffect` — defined/target players shuffle their libraries.
  `Optional` (1; the activator decides for each).
- **AddTurn** — 58 / 67 — `AddTurnEffect` — players take extra turns after this one (one per team).
  `NumTurns` (67), `ExtraTurnDelayedTrigger` + `ExtraTurnDelayedTriggerExecute` (7; a trigger that
  exists during the extra turn, e.g. "at the beginning of that turn's end step, you lose the
  game"), `SkipUntap` (2), `NoSchemes`, `ShowMessage`.
- **Phases** — 55 / 73 — `PhasesEffect` — permanents phase out (or in). Targets / `Defined`, or
  `AllValid` (11; all matching), `AnyNumber` (2), `PhaseInOrOut` (8; toggle),
  `WontPhaseInNormal` (5; stays phased out until an effect says otherwise), `Tapped` / `Untapped`,
  `RememberAffected` (4), `RememberValids`.
- **MultiplyCounter** — 54 / 56 — `CountersMultiplyEffect` — double (or ×N) the counters on
  permanents/players by putting (N−1)× as many more. `Multiplier` (6; default 2),
  `CounterType` (43; only that kind — default every kind).
- **ChooseNumber** — 53 / 59 — `ChooseNumberEffect` — players choose a number → host's
  `ChosenNumber`. `Min` (22), `Max` (34), `ChooseAnyNumber` (12), `Random` (5), `Secretly` (7),
  `ListTitle`, `RemoveChoices`, `KeepSecret`, `Notify`, `Guesser`, `RememberChosen`,
  `RememberHighest`; result branches `ChooseNumberSubAbility`, `Lowest`, `NotLowest`, `Highest`,
  `GuessCorrect`, `GuessWrong`, `MatchedAbility`, `UnmatchedAbility`.
- **Protection** — 53 / 53 — `ProtectEffect` — targets gain protection until end of turn.
  `Gains` (53): a colour / quality list, `Choice` (choose one of `Choices$ a,b,…`, 39),
  `ChosenColor`, or `Defined <cards>` (protection from those cards' colours); `Choser$ Controller`
  (the target's controller chooses), `Radiance`, `Duration` (`Permanent` allowed).
- **AddPhase** — 53 / 59 — `AddPhaseEffect` — additional phases/steps. `ExtraPhase` (59;
  `Combat`, `Beginning`, a phase name), `AfterPhase` (36), `FollowedBy` (14; e.g. an extra main
  phase after the extra combat), `NumPhases` (2), `BeforeFirstPostCombatMainEnd`,
  `ExtraPhaseDelayedTrigger` + `ExtraPhaseDelayedTriggerExcute` (sic, 4).
- **Repeat** — 53 / 60 — `RepeatEffect` — do-while loop over `RepeatSubAbility` (60): the body runs
  at least once and repeats while the condition holds: `RepeatCheckSVar` (28) +
  `RepeatSVarCompare` (30), or `RepeatPresent` (11) + `RepeatCompare` (default `GE1`) +
  `RepeatDefined` (10); `RepeatOptional` (7; a player — `RepeatOptionalDecider`, default activator —
  is asked whether to repeat); `MaxRepeat` (22; hard cap).
- **StoreSVar** — 51 / 131 — `StoreSVarEffect` — writes an SVar on the host card (a persistent
  integer variable). `SVar` (name; `EachPlayer` special form), `Type` (`Count`, `Number`,
  `CountSVar`, `Targeted`, `Triggered`, `Calculate`), `Expression`. The stored value replaces the
  SVar's definition, so later reads see a constant. Translates to a card-local counter/variable.
- **Connive** — 49 / 52 — `ConniveEffect` — each defined/target creature connives: its controller
  draws N then discards N; for each nonland card discarded this way put a +1/+1 counter on it.
  `ConniveNum` (3; default 1).
- **RingTemptsYou** — 49 / 49 — `RingTemptsYouEffect` (extends EffectEffect) — "the Ring tempts
  you": advance the player's Ring emblem and choose a Ring-bearer. No own keys.
- **Debuff** — 48 / 53 — `DebuffEffect` — targets/defined cards **lose** keywords. `Keywords` (52;
  ` & ` list), `AllSuffixKeywords$ walk` (all landwalk), `Duration` (default until end of turn;
  `Permanent`).
- **TapOrUntap** — 45 / 50 — `TapOrUntapEffect` — "you may tap or untap target permanent": the
  activator chooses per card; `Toggle` (tap if untapped, untap if tapped, no choice), `Tapper`.
- **LosesGame** — 44 / 51 — `GameLossEffect` — defined/target players (default You) lose the game.
- **ChangeTargets** — 43 / 44 — `ChangeTargetsEffect` — change the target(s) of a spell/ability on
  the stack. Targets a stack object (`TargetType$`, 31). `Optional` (16), `ChangeSingleTarget` (3;
  only one of its targets), `DefinedMagnet` (5; new target must be this object — redirect to self) +
  `TargetRestriction` (5), `RandomTarget` (2) + `RandomTargetRestriction`, `Chooser` (2). Fires the
  becomes-target events for new targets.
- **Draft** — 42 / 42 — `DraftEffect` — Arena "draft a card from a spellbook". `Spellbook` (42),
  `DraftNum`, `RememberDrafted`, `Defined`. Digital-only.
- **Explore** — 42 / 44 — `ExploreEffect` — defined/target creatures explore `Num` (5; default 1)
  times: reveal the top card of the controller's library; land → hand, otherwise +1/+1 counter and
  the controller may put the card into the graveyard. Fires "explores".
- **ManaReflected** — 42 / 47 — `ManaReflectedEffect` — "add one mana of any colour/type that a
  land an opponent controls could produce". `ReflectProperty` (47; `Produce`, `Is`, `Produced`),
  `ColorOrType` (47; `Color` / `Type`), `Valid` (25; which permanents to look at), `Amount` (2).
  (Read partly in the mana-part code; details UNVERIFIED.)
- **WinsGame** — 40 / 45 — `GameWinEffect` — defined/target players (default You) win the game.
- **SetLife** — 39 / 49 — `LifeSetEffect` — a player's life total becomes N (counts as gaining or
  losing the difference). `LifeAmount` (46), `Redistribute` (3; players' life totals are
  redistributed by choice), `PlayerChoices` + `ChoiceAmount` (`Any`) + `ChoicePrompt` (3).
- **Venture** — 37 / 46 — `VentureEffect` — venture into the dungeon. `Dungeon`, `RoomName`,
  `NextRoomName` (internal).
- **Empower** — 36 / 36 — `EmpowerEffect` (TokenEffectBase) — `Num`, `Type`. (semantics UNVERIFIED;
  Universes Beyond mechanic.)
- **ReplaceCounter** — 36 / 38 — `ReplaceCounterEffect` — replacement body for "counters would be
  put": sets the number to `Amount` (38; expression, usually using `ReplaceCount$…`) for
  `ValidCounterType` (25) / all kinds, optionally `ChooseCounter` (24; choose which kind when
  several), `ValidSource`. Result: event updated.
- **Earthbend** — 36 / 38 — `EarthbendEffect` — `Num` (38). (Avatar mechanic: a land becomes a
  creature with counters; semantics UNVERIFIED.)
- **ManifestDread** — 36 / 38 — `ManifestDreadEffect` (ManifestEffect) — look at the top two cards
  of the library, manifest one, put the other into the graveyard. Script keys: `Amount`,
  `DefinedPlayer`, `RememberManifested`.
- **ReplaceToken** — 35 / 39 — `ReplaceTokenEffect` — replacement body for token creation. `Type`
  (39): `Amount` (multiply — `Amount$` default `Twice`), `AddToken` (also create `TokenScript`
  tokens, `Amount`), `ReplaceToken` (create `TokenScript` / a `ValidChoices`-chosen permanent's copy
  instead), `ReplaceController` (`NewController`). Only tokens matching the replacement's
  `ValidToken` are affected.
- **Discover** — 35 / 37 — `DiscoverEffect` — discover N: exile from the top until a nonland card
  with mana value ≤ N; cast it free or put it into hand; rest on the bottom in random order.
  `Num` (37), `RememberDiscovered`.
- **ReplaceDamage** — 35 / 39 — `ReplaceDamageEffect` — replacement body that **prevents** up to
  `Amount` (39; default 1) of the damage event; if the damage drops to 0 the event is fully
  replaced, otherwise updated with the reduced amount. Used by prevention shields.
- **Poison** — 34 / 37 — `PoisonEffect` — defined/target players get `Num` (37) poison counters.
- **Vote** — 33 / 34 — `VoteEffect` — council voting. `Choices` (27; options as SVars) or
  `VotePlayer` / `VoteCard` (+ `Zone`), `VoteSubAbility`, `VoteTiedAbility` (14), `StoreVoteNum`
  (13; per-option counts into SVars), `Secretly`, `UpTo`, `EachVote`, `VoteMessage`,
  `RememberVotedObjects`.
- **Incubate** — 33 / 34 — `IncubateEffect` (TokenEffectBase) — create an Incubator token with
  `Amount` (34) +1/+1 counters; `Times` (4).
- **RearrangeTopOfLibrary** — 32 / 35 — `RearrangeTopOfLibraryEffect` — look at the top `NumCards`
  (35) cards of a library and put them back in any order; `MayShuffle` (5), `RearrangePlayer`.
- **MoveCounter** — 32 / 33 — `CountersMoveEffect` — move counters from `Source` (18; Defined card,
  default the host) onto the defined/target cards. `CounterType` (33; or `Any`), `CounterNum` (28;
  amount, `All`, `Any`), `ValidSource` (3; choose among matching sources), `ValidDefined`,
  `NonZero`.
- **Fog** — 32 / 34 — `FogEffect` — prevent all combat damage that would be dealt this turn. No own
  keys.
- **Manifest** — 31 / 33 — `ManifestEffect` (ManifestBaseEffect) — manifest the top card(s) of a
  library (or defined cards) as face-down 2/2 creatures. `Amount`, `DefinedPlayer`,
  `RememberManifested`; base-class keys not listed (UNVERIFIED).
- **ExchangeControl** — 30 / 39 — `ControlExchangeEffect` — exchange control of two permanents
  (two targets, or one target + `Defined`). `Optional`, `RememberExchanged`. Nothing happens unless
  both are still on the battlefield.
- **Clash** — 29 / 29 — `ClashEffect` — clash with an opponent; `WinSubAbility` (28),
  `OtherwiseSubAbility` (3), `RememberClasher`.
- **TwoPiles** — 29 / 34 — `TwoPilesEffect` — separate cards into two piles; another player chooses
  one. `Zone` (17), `ValidCards` / `DefinedCards` (21), `Separator` (30), `Chooser` (19),
  `ChosenPile` (32) / `UnchosenPile` (25) sub-abilities (the pile is the remembered set while each
  runs), `FaceDown`, `LeftRightPile`, `DefinedPiles`, `RememberChosen`, `KeepRemembered`.
- **EachDamage** — 27 / 29 — `DamageEachEffect` — each of several sources deals damage:
  `DefinedDamagers` (20) or battlefield filtered by `ValidCards` (7); `NumDmg` (29; default `X`,
  evaluated **per source**); recipients = targets/defined, or `EachToItself`, or
  `ToEachOther$ <Defined>` (each deals damage to each other one). Simultaneous.
- **RemoveFromCombat** — 27 / 29 — `RemoveFromCombatEffect` — remove creatures from combat.
  `UnblockCreaturesBlockedOnlyBy` (4), `RememberRemovedFromCombat`.
- **MustBlock** — 26 / 27 — `MustBlockEffect` — target creature must block `DefinedAttacker` (10;
  default the host) this turn if able; `BlockAllDefined`, `Choices` + `Chooser` + `ChoiceTitle`,
  `Duration`.
- **PermanentCreature** — 26 / 27 — `PermanentCreatureEffect` — the implicit spell ability of a
  creature card (put it onto the battlefield on resolution). Written explicitly only for special
  cases (`SetPower`, `SetToughness`, `AdditionalDesc`). **PermanentNoncreature** (1) is the same for
  other permanent cards.
- **TakeInitiative** — 23 / 23 — the defined player takes the initiative. No keys.
- **Radiation** — 22 / 22 — `RadiationEffect` — players get `Num` rad counters (Fallout).
- **Learn** — 22 / 23 — `LearnEffect` — learn (Lesson from outside the game, or rummage). No keys.
- **ReplaceMana** — 22 / 30 — `ReplaceManaEffect` — replacement body for mana production:
  `ReplaceMana`, `ReplaceType` (21), `ReplaceColor`, `ReplaceOnly`, `ReplaceAmount`.
- **Intensify** — 20 / 21 — `IntensifyEffect` — Arena: raise a card's "intensity". `Amount`,
  `AllDefined`, `DefinedDesc`.
- **ReplaceSplitDamage** — 19 / 20 — `ReplaceSplitDamageEffect` — replacement body redirecting part
  of the damage to `DamageTarget` (20); `VarName` (amount redirected).
- **SkipPhase** — 16 / 18 — `SkipPhaseEffect` — a player skips a `Phase` (8) or `Step` (10);
  `Start`, `Duration`.
- **DayTime** — 14 / 15 — `DayTimeEffect` — `Value` (15): `Day`, `Night`, `Switch`.
- **RemoveCounterAll** — 14 / 20 — `CountersRemoveAllEffect` — remove counters from every card
  matching `ValidCards` (20) in `ValidZone`: `CounterType` + `CounterNum`, or `AllCounters` (12; all
  of that kind), `AllCounterTypes`; `RememberAmount`.
- **Airbend** — 13 / 13 — `AirbendEffect` — (Avatar mechanic; semantics UNVERIFIED). `Defined`.
- **VillainousChoice** — 13 / 13 — `VillainousChoiceEffect` — an opponent faces a villainous
  choice between the `Choices` (13) sub-abilities.
- **ChangeText** — 12 / 17 — `ChangeTextEffect` — text-changing effects: `ChangeColorWord` (9),
  `ChangeTypeWord` (8), `ForbiddenNewTypes`, `Duration` (11).
- **Endure** — 12 / 12 — `EndureEffect` (TokenEffectBase) — endure N: +1/+1 counters or a Spirit
  token. `Num`.
- **Detain** — 12 / 13 — `DetainEffect` — detain targets (can't attack/block/activate until your
  next turn). No keys.

## 101 and below

- **DigMultiple** (11) — `DigMultipleEffect` — Dig variant choosing one card **per filter**:
  `DigNum`, `ChangeValid` (list), `SourceZone`, `DestinationZone`, `DestinationZone2`,
  `LibraryPosition(2)`, `Optional`, `Reveal`, `ChooseAmount`, `ChosenZone`, `ChangeLater`, `Tapped`,
  `ExileFaceDown`, `Imprint`, `RememberChanged`, `ForgetOtherRemembered`, `RestRandomOrder`,
  `ImprintRest`.
- **SkipTurn** (11) — `SkipTurnEffect` — players skip their next `NumTurns` turns.
- **ProtectionAll** (11) — `ProtectAllEffect` — Protection for all `ValidCards` / `ValidPlayers`;
  `Gains`, `Choices`, `Duration`.
- **Cloak** (11) — `CloakEffect` — cloak cards (face-down 2/2 with ward {2}). `Tapped`,
  `RememberCloaked`.
- **ChangeCombatants** (10) — `ChangeCombatantsEffect` — reselect which player/planeswalker an
  attacker is attacking. `Attacking`, `Optional`.
- **AddOrRemoveCounter** (10) — `CountersPutOrRemoveEffect` — "put a counter on or remove a counter
  from": `CounterType`, `CounterNum`, `EachExistingCounter`, `Optional`, `DefinedPlayer`,
  `AddConditionSVar`, `RemoveConditionSVar` (+`Compare`), `RememberRemovedCards`.
- **GainControlVariant** (10) — `ControlGainVariantEffect` — mass control changes by rule:
  `AllValid` (which permanents), `ChangeController` (`CardOwner` — each returns to its owner's
  control, 6 of 10; `Random`; direction-based forms).
- **Recruit** (10) — `RecruitEffect` — (Universes Beyond mechanic; UNVERIFIED).
- **TimeTravel** (9) — `TimeTravelEffect` — add/remove time counters on your suspended cards and
  permanents; `Amount`.
- **Planeswalk** (8), **ChaosEnsues** (6), **RollPlanarDice** (1), **SetInMotion**, **Abandon**,
  **RunChaos** — Planechase / Archenemy. Not relevant to Commander precons.
- **Heist** (8), **Draft**, **Seek**, **Intensify**, **LosePerpetual** (2), **MakeCard** — Arena-only.
- **Unattach** (8) — `UnattachEffect` — unattach the defined/target attachment. No keys.
- **Assimilate** (8) — `AssimilateEffect` — see Forge's own docs; Universes Beyond.
- **EndTurn** (8) — `EndTurnEffect` — end the turn. `Optional`, `Defined`.
- **ControlPlayer** (8) — `ControlPlayerEffect` — control target player during their next turn /
  combat. `Controller`, `Combat`.
- **Meld** (7) — `MeldEffect` — exile `Primary` and `Secondary` (names) and meld them; `Name`,
  `SecondaryType`, `Tapped`.
- **ExchangeLife** (7) — `LifeExchangeEffect` — two players exchange life totals.
  `RememberOwnLoss`, `RememberDifference`.
- **ChooseEvenOdd** (6) — choose odd or even (`Notify`). **ChooseDirection** (5) — left or right.
  **ChooseSector** — Arena.
- **Blight** (6) — `BlightEffect` — `Num`. (UNVERIFIED.)
- **ControlSpell** (6) — `ControlSpellEffect` — gain control of a spell on the stack. `Mode` (6;
  `Gain` / `Exchange`), `NewController`, `Remember`.
- **AssignGroup** (5) — `AssignGroupEffect` — assign defined objects to groups, one sub-ability per
  group (`Choices`, `Chooser`).
- **DrainMana** (5) — `DrainManaEffect` — empty a player's mana pool; `DrainMana` (take it),
  `RememberDrainedMana`.
- **BecomesBlocked** (5) — the defined attacker becomes blocked (no blocker). No keys.
- **BlankLine** (5) — does nothing (text-only filler).
- **Balance** (4) — `BalanceEffect` — Balance-style equalisation: `Valid`, `Zone`.
- **GainOwnership** (4) — `OwnershipGainEffect` — ante / ownership change; `DefinedPlayer`.
- **LookAt** (4) — look at defined face-down cards. **UnlockDoor** (4) — Rooms: `Mode`, `Choices`.
- **ExchangeLifeVariant** (3) — `Mode`: life ↔ power/toughness exchanges. **BidLife** (3) —
  `StartBidding`, `OtherBidder`, `BidSubAbility`. **Block** (3) — `DefinedAttacker`,
  `DefinedBlocker`: make a creature block. **HealDamage** (3) — remove all damage from a permanent.
- **Regeneration** (3 explicit, but used by every regeneration shield) — `RegenerationEffect` — the
  act of regenerating: remove all damage, tap, remove from combat (tapped-event batch). No keys.
- **GameDrawn** (2), **RestartGame** (1; `RestrictFromZone`, `RestrictFromValid`),
  **ReverseTurnOrder** (1), **Subgame** (1), **RemoveFromGame**, **RemoveFromMatch** — game-level.
- **ReorderZone** (2) — reorder a `Zone` (`Random`). **ActivateAbility** (2) — force a player to
  activate abilities (`Type`, `ManaAbility`). **ExchangePower** (2) — exchange two creatures'
  power (`BasePower`, `Duration`). **EndCombatPhase** (2) — `Optional`. **ChangeX** (2) — change
  the X of a spell (`Value`). **SwitchBlock** (2) — `DefinedAttacker`, `DefinedBlocker`,
  `RemoveFromCombat`. **FlipOntoBattlefield** (2) — dexterity cards (`AllowRandom`).
- **ChangeSpeed** (1; `Mode`), **ExchangeTextBox** (1), **AssembleContraption** (1),
  **TapOrUntapAll** (1; `ValidCards`; one script spells it **TaporUntapAll** — API names are
  matched case-insensitively), **ExchangeZone** (1; `Zone1`, `Zone2`, `Type`,
  `ValidExchange`, `Object`, `Mandatory`), **MultiplePiles** (1; `Piles`, `ValidCards`, `Zone`,
  `RandomChosen`, `ChosenPile`), **Camouflage** (1), **ExchangeControlVariant** (1; `Zone`,
  `Type`), **PutSticker**, **OpenAttraction**, **AdvanceCrank**, **ClaimThePrize**,
  **PlayLandVariant** — one-off or Un-set support.
- Engine-internal, never or rarely written in scripts: **ChangeZoneResolve** (fires the batched
  zone-change triggers collected with `ChangeZoneTable$ True`), **Bond** (soulbond; `ValidCards`),
  **Encode** (cipher), **Haunt**, **Mutate**, **ClassLevelUp**, **InternalRadiation**,
  **CompanionChoose** / **InternalLegendaryRule** / **InternalIgnoreEffect** (Charm reused for
  engine choices).

---

# Appendix A — things a translator must get right

1. **Two `ChangeZone`s.** `Hidden$ True` or a hidden origin zone selects "choose at resolution"
   (search/choose, `ChangeType`, `ChangeNum`, optional unless `Mandatory`); otherwise the cards are
   the targets/`Defined` and `ChangeType`/`ChangeNum` are ignored. The same key (`Defined`,
   `Optional`, `Shuffle`) means different things in the two modes.
2. **Booleans are presence tests.** Almost every flag is read with `hasParam`; `NoRegen$ False`
   still means "can't be regenerated". Exceptions compare the value explicitly (ChangeZone
   `Shuffle$ True/False`, Dig `Reveal$ True`, AlterAttribute `Activate$ False`,
   ChangeZoneAll `RememberChanged$ <filter>`).
3. **Default duration is "until end of turn"**, including for Pump/Animate/Effect/Debuff; lasting
   effects say `Duration$ Permanent`. But `Clone` and `GainControl` are permanent by default and
   GainControl uses its own `LoseControl$` vocabulary, and `Token.PumpKeywords` are permanent unless
   `PumpDuration$` is given.
4. **`Defined$` versus targets precedence differs by API.** "Targets win" (ChangeZone known mode,
   Pump, Destroy, Tap, Untap, DealDamage, Sacrifice, Discard, LoseLife, Mill, Scry) versus
   "`Defined` wins" (Draw, GainLife, PutCounter, Token owner, Mana, Dig, CopyPermanent, GainControl,
   Regenerate …). It only matters when a link has both, which is rare but real
   (`ValidTgts$ … | Defined$ …` on Fight, Attach, PutCounter).
5. **Defaults for the absent `Defined$`**: `Self` for card effects, `You` for player effects — and
   Pump evaluates both.
6. **Remember/Imprint/Chosen* are host-card state**, i.e. global variables scoped to the card
   object, not to the resolution. `RepeatEach` iterates by temporarily remembering each element;
   `Effect` snapshots the host's chosen values; `Cleanup` ends the scope. Several keys *clear*
   remembered implicitly (`RememberDestroyed`, SacrificeAll `RememberSacrificed`,
   `ForgetOtherRemembered`, `RememberCostMana`).
7. **Conditions gate one link only.** A failed `Condition…` skips that effect but the `SubAbility`
   chain continues. Real branching is `Branch`, `GenericChoice`, `UnlessCost` +
   `UnlessResolveSubs`, or matching/inverse conditions on sibling links.
8. **`UnlessCost` + `UnlessSwitched$ True`** is the standard encoding of "you may pay X. If you do,
   <effect>"; without `UnlessSwitched` it is "<effect> unless <payer> pays X", and the default payer
   is the *target's controller*, not you.
9. **Simultaneity is explicit.** Within one effect everything is batched (one damage event, one
   zone-change batch, one counter batch). Across links it is sequential unless the script uses
   `DamageMap$ True … DamageResolve` or `ChangeZoneTable$ True … ChangeZoneResolve` (or the
   `RepeatEach`/`GenericChoice` versions of those keys).
10. **Object identity checks.** Effects silently skip a card that changed zones since it was
    targeted/remembered (same id but newer timestamp). Scripts that need the *old* object's values
    use `…LKICopy` Defined forms or `RememberLKI$`; scripts that need the *new* object use
    `RememberChanged$` and then `Defined$ Remembered`.
11. **Charm modes are chosen on announcement** and become the sub-ability chain in printed order;
    `GenericChoice` chooses on resolution. Charm mode SVars carry their own `ValidTgts`.
12. **`Effect` is the general continuous/replacement/delayed-effect carrier.** Its
    `StaticAbilities` / `Triggers` / `ReplacementEffects` are ordinary static/trigger/replacement
    lines bound to objects through `RememberObjects$` → `Card.IsRemembered`; its lifetime is
    `Duration$` plus the `ForgetOnMoved` / `ExileOnMoved` clean-up triggers.
13. **"Enters with/tapped" replacement bodies** are ordinary effects with `ETB$ True`
    (`Tap | ETB$ True`, `PutCounter | ETB$ True`): they modify the entering event instead of acting
    afterwards.
14. **Amounts are computed once at resolution** (`calculateAmount`) — except where the class
    evaluates per object (`Pump NumAtt$ Double`, `PutCounter CounterNumPerDefined`,
    `EachDamage NumDmg`, `ChangeZoneAll WithCountersAmount`).
15. **Library positions**: `0` = top, `-1` = bottom; ChangeZone defaults to top, Dig defaults both
    destinations to bottom. Forge's library list has index 0 = top; apnap's has last = top.

# Appendix B — verification status

- Read in full (resolve logic): the 60 APIs of Part I, `SpellAbilityEffect`, `AbilityUtils.resolve*`
  / `handleUnlessCost` / `handleRemembering`, `AbilityFactory.getAbility`, `TokenEffectBase`,
  `AnimateEffectBase`, `TokenInfo.getProtoType`, `AbilityManaPart` (restrictions, combo colours).
- Caveat for ranks 43–60 (Clone … NameCard): these classes were read through a condensing filter
  that hid prompt lines and bare `else` lines. Two misreadings this caused (DigUntil
  `NoMoveRevealed`/`NoMoveFound`, ChoosePlayer `CantChooseSubAbility`) were caught and corrected
  against the source; the branch structure of the others was re-checked from the condensed view
  only, so re-read the class before relying on an if/else detail there.
- **Not read**: the game-action layer the effects call into (`GameAction.moveTo` / `destroy` /
  `sacrifice` / `mill` / `scry` / `dealDamage`, `Player.drawCards` / `discard` / `surveil`, the
  replacement and trigger handlers) except for the specific lines cited. Statements about what
  happens *inside* those calls (per-card replacement, events fired, "can't" checks) describe the
  rules meaning and Forge's evident intent, not line-by-line verified behaviour.
- Marked UNVERIFIED in the text: the exact mana-ability predicate; `FaceDown…` state keys
  (`CardFactoryUtil.setFaceDownState`); `StaticEffect$` on zone changes; the purpose of the
  `AssignDealDamage` replacement run before damage; land-drop accounting in `Play`; Charm
  `Pawprint` enforcement (in the mode-choice UI); Dig `GainControl` (looks buggy);
  `CombatDamage$` on DealDamage/PreventDamage; scry 0; CopySpellAbility `IgnoreFreeze`; several
  Part II entries explicitly tagged.
- Keys that occur in scripts but are consumed outside the effect class (verified location):
  `RememberDiscarded` (`Player.discard`), `RememberDrawn$ AllReplaced` (`Player`),
  `RememberMoved` / `RememberKept` (`Player.surveil`), `RememberDamaged` / `RememberAmount`
  (`GameAction.dealDamage`), `RememberPut` / `MaxFromEffect` / `RememberAmount`
  (`GameEntityCounterTable`), `ExcessSVar` (`CardDamageTable`), `NoPrevention` / `NoRedirection`
  (replacement code), `DividedAsYouChoose` (`SpellAbility`), `ModeCost` (`CostAdjustment`),
  copy modifiers (`CardFactory.getCloneStates`), `ResultSubAbilities` (`AbilityFactory`).
- Scripts used for the statistics: `corpus.py`, `show.py`, `show2.py`, `show3.py`, `ex.py`,
  `javakeys.py`, `keys2.py`, `jview.py` under
  `/tmp/claude-1000/-run-host-home-cmt-Code-apnap/1d7f8b6f-0a4a-457f-af68-5331e6f679f2/scratchpad/agent-effects/`.
