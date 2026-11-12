# Forge card script format — implementation reference

This document describes the *syntax and core sub-languages* of the card scripts of the
open-source engine Forge (GPL v3), precisely enough to write a lossless parser and a
translator. It is a description in our own words of what Forge's parser and evaluator do;
it contains no Forge code. Forge is a design reference only (CLAUDE.md, rule 13).

Sources read (checkout at commit `3d5c95bcaa592f6df72538b63d5d2d890e46dbfd`):

- `forge-core/src/main/java/forge/card/CardRules.java` (inner class `Reader`), `CardFace.java`,
  `CardSplitType.java`, `CardStateName.java`, `CardType.java`, `mana/ManaCost*.java`
- `forge-game/src/main/java/forge/game/` — `ability/AbilityFactory.java`,
  `ability/AbilityUtils.java`, `ability/SpellAbilityEffect.java`, `ability/AbilityKey.java`,
  `cost/*.java`, `card/CardProperty.java`, `card/CardStateProperty.java`, `card/Card.java`
  (`isValid`), `card/CardFactory.java`, `card/CardState.java`, `player/PlayerProperty.java`,
  `player/Player.java` (`isValid`), `spellability/SpellAbility*.java`,
  `spellability/TargetRestrictions.java`, `CardTraitBase.java`, `GameObject.java`
- `forge.util.FileSection`, `forge.util.TextUtil`, `forge.util.Expressions` are **not** in
  the local checkout (it is sparse); they were read from GitHub at the same commit.
- The corpus: `forge-gui/res/cardsfolder/<letter>/*.txt` (all counts below are from it).

Conventions used here: `UNVERIFIED` marks a meaning that was not confirmed in the Java.
"Host" is the card a script line belongs to. "Activator" is the player who cast / activated
/ controls the ability being evaluated.

Contents

1. [File structure](#1-file-structure)
2. [Records: `Key$ Value | Key$ Value`](#2-records-key-value--key-value)
3. [The `Cost$` sub-language](#3-the-cost-sub-language)
4. [The "Valid" filter sub-language](#4-the-valid-filter-sub-language)
5. [The `Defined$` sub-language](#5-the-defined-sub-language)
6. [Numeric expressions](#6-numeric-expressions)
7. [Durations, remembering, Cleanup, descriptions](#7-durations-remembering-cleanup-descriptions)
8. [Keys a translator can ignore](#8-keys-a-translator-can-ignore)
9. [Things a translator author must know](#9-things-a-translator-author-must-know)

---

## 1. File structure

Source: `CardRules.Reader.readCard` / `parseLine`, `CardFace`, `CardFactory.readCard` /
`readCardFace`, `CardState` (implicit abilities).

### 1.1 Lines

A script is a list of lines. For each line:

- An empty line, or a line whose first character is `#`, is skipped. (211 comment lines exist
  in the corpus.) Blank lines have no meaning of their own; by convention they surround the
  face separators.
- Otherwise the line is split at its **first** `:`. The part before is the *key*; the part
  after, trimmed of surrounding whitespace, is the *value*. A line without `:` is all key
  (this is how `ALTERNATE` is written).
- The key is matched **case-sensitively** against a fixed set. A line whose key is not
  recognised is silently ignored.
- Lines may come in any order, except that `Name` must come first in each face (it creates
  the face that the following lines fill in), and face separators switch the current face.

**Comments (D-078).** The test is on the raw line, `line.isEmpty() || line.charAt(0) == '#'`
(`CardRules.Reader.readCard`): only a `#` in column 0 makes a comment, and the line never
reaches `parseLine`. A `#` anywhere else (indented, or inside a value) is ordinary text. The
corpus has 135 distinct comments on 210 lines in 198 cards: AI notes ("TODO: the AI won't
..."), notes on how SVars are used, `# --- VARIANTS ---` markers, and a handful that say a
script is wrong or incomplete ("Doesn't follow the strict wording of the Oracle text",
"TODO: need EnchantedLKI ..."). Forge ignores all of them; the compiler ignores the ones in
`forge/comments.rs`'s `HARMLESS` list, blocks the ones in `INCOMPLETE`
(`script:differs from the card`), and blocks any comment in neither (`comment:unreviewed`).
(`mkzip.sh` in the cards folder is not a card and starts with `#!/bin/sh`.)

Every line type the reader recognises:

| Key | Count | Value | Meaning |
|---|---|---|---|
| `Name` | 35,429 | text | Starts a face; the face's name. Must be non-blank. |
| `ManaCost` | 35,428 | mana cost (§1.3) or `no cost` | Mana cost of the face. |
| `Types` | 35,429 | words separated by single spaces (§1.4) | Supertypes, card types, subtypes. |
| `PT` | 19,660 | `P/T` | Power/toughness. Exactly one `/`. Each side is an integer or contains `*` (`*`, `1+*`, `*+1`, `7-*`). The text is kept; the integer is computed by deleting `+*`, `-*`, `*+` and then turning a remaining `*` into `0` (so `1+*` → 1, `*` → 0). The real value of a `*` comes from a characteristic-defining static ability elsewhere in the script. |
| `Loyalty` | 360 | text | Starting loyalty; a number, or `X` (evaluated as a numeric expression, §6). |
| `Defense` | 37 | text | Starting defense of a battle. |
| `Colors` | 379 | colour names separated by `,` (`white`, `blue`, `black`, `red`, `green`) | Colour indicator / explicit colour. When absent, colour is derived from the mana cost. |
| `K` | 18,616 | keyword text | One keyword ability per line; see §1.5. |
| `A` | 18,794 | record (§2) with `SP$`/`AB$`/`ST$` | A spell or activated ability. |
| `T` | 17,500 | record with `Mode$` | A triggered ability. |
| `S` | 7,280 | record with `Mode$` | A static ability. |
| `R` | 1,751 | record with `Event$` | A replacement effect. |
| `SVar` | 61,743 | `<name>:<text>` | A named string variable, see §2.4. The name is everything up to the next `:`; the rest is the value (not trimmed again). An `SVar` line with no value at all is an error. |
| `Oracle` | 35,429 | text, `\n` written literally as backslash-n | Printed rules text. Not interpreted, **except** that a few places search it (deck-building colours for lands; "was spent to cast" enables tracking of mana colours spent, `SpellAbility.tracksManaSpent`). |
| `Text` | 74 | text | Extra display text that no ability generates (e.g. `CARDNAME can be your commander.`). Display only in the reader; the *rules* for such text live elsewhere (a `K:` line or hard-coded). |
| `AlternateMode` | 923 | layout name (§1.2) | Declares that the card has more than one face, and how they relate. Written in the first face. |
| `ALTERNATE` | (no colon) | — | Face separator: following lines belong to face 1 (the "other part"). |
| `SPECIALIZE` | 95 | `WHITE`/`BLUE`/`BLACK`/`RED`/`GREEN` | Face separator for Specialize cards: following lines belong to face 2..6. |
| `MeldPair` | 14 | card name | Name of the card this one melds with (§1.2). |
| `CopyFaceFrom` | 25 | card name | The current face is a placeholder copied from another card's face (used for cards that reuse an existing half). |
| `Variant` | 305 | `<variantName>:<line>` | The inner `<line>` is parsed as a normal line but applied to a *functional variant* of the current face (same name, different printing-specific content). Almost all uses are `Variant:UniversesWithin:FlavorName:…`. Fields the variant does not set are inherited from the face. |
| `FlavorName` | (inside `Variant`) | text | Alternative display name. |
| `AI` | 4,929 | `RemoveDeck:All` / `RemoveDeck:Random` / `RemoveDeck:NonCommander` | Deck-building hint for the AI. Not rules. |
| `DeckHints`, `DeckNeeds`, `DeckHas` | 4,174 / 1,223 / 7,635 | hint expression | Deck-building hints. Not rules. |
| `DeckRule` | 13 | text | Hard-coded deck-construction rule strings (e.g. companion-like restrictions). |
| `Draft` | 48 | text | Hard-coded draft-time action strings (Conspiracy draft matters cards). |
| `HandLifeModifier` | 106 | `+h/+l` | Vanguard hand-size / life modifiers. |
| `Lights` | 1 | integers separated by spaces | Attraction lights. |
| `SETCOLORID` | 1 | integer | Internal colour-identity override. |

Independently of the key, any line whose value contains `TokenScript$` (not at position 0)
has the following comma-separated names (up to the next `|`) recorded as the tokens the card
can make. That is bookkeeping for the UI, not rules.

`SVar` names are stored in a **case-insensitive** map (`CardFace.addSVar`), and a later
definition of the same name replaces an earlier one. SVars are per face.

Everything that looks like a record in `A:`, `T:`, `S:`, `R:` and in SVar values is kept as
raw text by the reader and only parsed when a game card is built.

### 1.2 Multi-face cards

The reader keeps up to seven faces: index 0 = main part, 1 = other part, 2–6 = the five
Specialize faces (W, U, B, R, G). The layout is named by `AlternateMode:` in the first face.
`AlternateMode:DoubleFaced` is read as `Transform`. Any other value must be an exact enum
name or loading fails.

| `AlternateMode` | Count | Second face becomes game state | How the faces relate |
|---|---|---|---|
| (absent) | — | — | Single-faced card. |
| `DoubleFaced` (= `Transform`) | 399 | `Backside` | Transforming double-faced card. The back face normally has `ManaCost:no cost` and a `Colors:` line. |
| `Modal` | 101 | `Backside` | Modal double-faced card: either face can be played from hand; the back has its own mana cost. |
| `Meld` | 14 | `Meld` | The second face is the *melded* permanent. Both front cards say `MeldPair:<other front>` and `AlternateMode:Meld`, but only **one** of the two files contains the `ALTERNATE` melded face; the other finds it through its `MeldPair` partner (`CardFactory.readCard`). |
| `Split` | 128 | `RightSplit` (and the first face is `LeftSplit`) | Split card. The card's own "Original" state is the combination of both halves (combined name, cost, colour, types). Fuse / Aftermath / Room behaviour comes from `K:` lines on the halves. |
| `Flip` | 20 | `Flipped` | Kamigawa flip card. The flipped face repeats the mana cost and has a `Colors:` line. |
| `Adventure` | 151 | `Secondary` | Creature (first face) with an Adventure spell (second face, type line includes `Adventure`). |
| `Omen` | 16 | `Secondary` | Same shape as Adventure (type line includes `Omen`). |
| `Vengeance` | 1 | `Secondary` | Same shape; the secondary spell is cast from the graveyard (the factory restricts its first spell ability to the graveyard). |
| `Prepare` | 74 | `PreparedSpell` | A permanent (first face) with a spell (second face) that may be cast as a copy while the permanent is "prepared". |
| `Specialize` | 19 | `SpecializeW/U/B/R/G` | Six faces: the base, then five blocks introduced by `SPECIALIZE:WHITE` … `SPECIALIZE:GREEN`. |

Layout of a two-face file:

```
<lines of face 0, including AlternateMode:…>
(blank)
ALTERNATE
(blank)
<lines of face 1, starting with Name:>
```

Real example (split card, `f/fire_ice.txt`):

```
Name:Fire
ManaCost:1 R
Types:Instant
A:SP$ DealDamage | ValidTgts$ Any | … | NumDmg$ 2 | TargetMin$ 1 | TargetMax$ 2 | DividedAsYouChoose$ 2 | SpellDescription$ …
AlternateMode:Split
Oracle:Fire deals 2 damage divided as you choose among one or two targets.

ALTERNATE

Name:Ice
ManaCost:1 U
Types:Instant
A:SP$ Tap | ValidTgts$ Permanent | SubAbility$ DBDraw | SpellDescription$ Tap target permanent. Draw a card.
SVar:DBDraw:DB$ Draw
Oracle:Tap target permanent.\nDraw a card.
```

Real example (transforming card, `d/delver_of_secrets_insectile_aberration.txt`): the front
face ends with `AlternateMode:DoubleFaced`; after `ALTERNATE` the back face is
`Name:Insectile Aberration`, `ManaCost:no cost`, `Colors:blue`, `Types:Creature Human Insect`,
`PT:3/2`, `K:Flying`.

Notes for a translator:

- The deck-hint and `AI` lines are per *card*, not per face; all the other lines are per face.
- A face never sees the SVars of another face.
- The transition between faces is always scripted explicitly (e.g. `DB$ SetState | Mode$
  Transform`, `Mode$ Flip`, `DB$ Meld`), or comes from a keyword (`K:Specialize:2`,
  `K:Daybound`, `K:Disturb:…`, `K:Craft:…`).

### 1.3 Mana cost syntax

Source: `ManaCostParser`, `ManaCostShard.parseNonGeneric`, `ManaCost`.

- The literal value `no cost` means "has no mana cost" (cannot be cast for its cost; mana
  value 0). 2,204 faces.
- Otherwise the value is split on single spaces into tokens.
  - A token that parses as an integer adds to the **generic** amount (`2`, `10`, `0`).
    Several integer tokens add up. (A total of `-1` with no `X` also means "no cost".)
  - Any other token is one mana symbol ("shard"). It is read **character by character**,
    each character switching on a property; the order of characters inside a token does not
    matter, and unknown characters such as `/` are ignored:

    | Char | Property |
    |---|---|
    | `W` `U` `B` `R` `G` | that colour |
    | `C` | colourless (must be paid with colourless mana) |
    | `P` | Phyrexian (may pay 2 life instead) |
    | `S` | snow |
    | `X` | X |
    | `2` | "or two generic" (monocoloured hybrid `{2/W}`) |
    | other digit | generic |

    The resulting set of properties must be one of the known symbols:

    | Written | Symbol |
    |---|---|
    | `W` `U` `B` `R` `G` | coloured mana |
    | `C` | `{C}` |
    | `WU` `WB` `UB` `UR` `BR` `BG` `RW` `RG` `GW` `GU` (either order) | two-colour hybrid |
    | `2W` `2U` `2B` `2R` `2G` (also written `2/R`) | `{2/W}` etc. |
    | `CW` `CU` `CB` `CR` `CG` | colourless-or-colour hybrid |
    | `WP` `UP` `BP` `RP` `GP` | Phyrexian |
    | `WUP`, `RWP`, `GUP`, … (two colours + `P`) | hybrid Phyrexian |
    | `S` | snow |
    | `X` | X (write `X X` for `{X}{X}`) |

- The mana value of a symbol is 2 for the `2/…` hybrids, 0 for `X`, otherwise 1.

Examples from the corpus: `ManaCost:2 G`, `ManaCost:X R R`, `ManaCost:1 WB` (hybrid),
`ManaCost:1 BP` (Phyrexian), `ManaCost:2R 2R 2R`, `ManaCost:0`, `ManaCost:no cost`.

The same parser is used for the mana part of `Cost$` strings (§3), and for mana written in
other parameters.

### 1.4 Types

Source: `CardType.parse`, `CardType.add`.

The value is a list of words separated by single spaces, in printed order. Each word is
classified on its own, not by position:

- one of the **card types** `Artifact Battle Conspiracy Creature Dungeon Enchantment Instant
  Kindred Land Phenomenon Plane Planeswalker Scheme Sorcery Stickers Vanguard` → card type;
- one of the **supertypes** `Basic Elite Host Legendary Ongoing Snow World` → supertype;
- anything else → subtype.

There is no dash; `Types:Legendary Creature Goblin Shaman` is "Legendary Creature — Goblin
Shaman". A subtype that consists of more than one word is recognised by prefix match against a
list of multiword types before the split on spaces (corpus: `Time Lord`, `Bolas's Meditation
Realm`). That list is loaded from a data file that is not part of the checkout — its exact
contents are UNVERIFIED.

### 1.5 `K:` lines

Each `K:` line is one keyword. Three shapes exist:

1. A bare keyword: `K:Flying`, `K:First Strike`, `K:Split second`.
2. A keyword with `:`-separated arguments: `K:Kicker:1 R`, `K:Cycling:2`, `K:Equip:2`,
   `K:Enchant:Creature`, `K:Suspend:4:U`, `K:TypeCycling:Basic:2`,
   `K:Protection:Card.MultiColor:multicolored`, `K:etbCounter:P1P1:2`,
   `K:ETBReplacement:Copy:DBCopy:Optional`, `K:Ward:2`. Cost arguments are §3 cost
   strings; type arguments are §4 filters; numeric arguments are §6 expressions.
3. A whole sentence that the engine matches literally: `K:CARDNAME can't attack or block
   alone.`, `K:You may choose not to untap CARDNAME during your untap step.`

The per-keyword argument meanings are outside the scope of this document (they are expanded
into ordinary triggers / statics / replacements / abilities in
`CardFactoryUtil.addTriggerAbility`, `addReplacementEffect`, `addSpellAbility`,
`addStaticAbility`). Two special cases matter to the reader itself: `K:Partner with:<name>`
and `K:Partner:<type>` are also recorded on the card rules.

### 1.6 Abilities that are *not* written in the script

Source: `CardState` (spell-ability list construction), `CardFactory.buildAbilities`.

- A permanent face with no explicit spell gets an implicit "cast this permanent spell"
  ability whose cost is the face's `ManaCost`. So a vanilla creature has no `A:` line.
- A land face gets an implicit "play this land" ability.
- An Aura face gets an implicit Aura spell built from its `K:Enchant:…` keyword.
- A land that has a basic land subtype (`Plains Island Swamp Mountain Forest`) gets the
  matching `{T}: Add <mana>` ability implicitly. That is why `f/forest.txt` is just
  `Name`, `ManaCost:no cost`, `Types:Basic Land Forest`, `Oracle`.
- A planeswalker gets "enters with loyalty counters" and a battle gets "enters with defense
  counters" implicitly, from `Loyalty:` / `Defense:`.
- Instants and sorceries always have an explicit `A:SP$ …` line.

---

## 2. Records: `Key$ Value | Key$ Value`

Source: `FileSection.parseToMap`, `AbilityFactory.getAbility` / `getMapParams`,
`TriggerHandler.parseTrigger`, `ReplacementEffect`, `StaticAbility`,
`SpellAbilityRestriction.setRestrictions`, `SpellAbilityCondition.setConditions`,
`CardTraitBase.meetsCommonRequirements`, `AbilityUtils.resolveApiAbility` /
`handleUnlessCost`.

### 2.1 Lexical rules

The value of an `A:`, `T:`, `S:`, `R:` line, and the value of any SVar that holds an ability,
trigger, static or replacement, is a *record*:

```
record := pair ( "|" pair )*
pair   := key "$" value
```

- The text is split on **every** `|`. There is no escaping, so a value can never contain `|`.
- Each piece is split at its **first** `$`. Key and value are both trimmed. Later `$` signs
  belong to the value (`SVarCompare$ GE1`, `NumCards$ Count$xPaid`,
  `DeckHints:Type$Human` are all fine).
- A piece with no `$` becomes a key with the empty string as value.
- The result is a map with **case-insensitive keys**; if a key occurs twice, the **last**
  occurrence wins.
- Values are otherwise opaque text. Each key's value has its own little grammar (a filter, a
  cost, a number, a comma list…) defined by whoever reads that key.

A very important consequence of how the Java reads the map: a great many flag keys are tested
for **presence only** (`hasParam` / `containsKey`), not for the value `True`. E.g.
`SorcerySpeed`, `InstantSpeed`, `PlayerTurn`, `OpponentTurn`, `Static`, `UnlessSwitched`,
`RememberTargets`, `ClearRemembered`, `DividedAsYouChoose`, `TargetUnique`, `NoResolvingCheck`.
For those, `Key$ False` would still switch the flag on. A translator should treat presence as
"on" for such keys and only read the value where this document says the value is compared
(e.g. `Secondary$ True`, `Prevent$ True`, `ConditionPlayerTurn$ False`).

### 2.2 The four record kinds

| Line | Kind is recognised by | The "what it is" key |
|---|---|---|
| `A:` (and ability SVars) | one of `AB$`, `SP$`, `ST$`, `DB$` — tested in that order | the value of that key names the *API* (effect type): `SP$ DealDamage` |
| `T:` (and trigger SVars) | `Mode$` | trigger type: `Mode$ ChangesZone` |
| `S:` (and static SVars) | `Mode$` | static ability mode: `Mode$ Continuous` |
| `R:` (and replacement SVars) | `Event$` | replaced event: `Event$ Moved` |

Ability prefixes:

| Prefix | Meaning | Cost |
|---|---|---|
| `SP$` | A spell: the effect of casting this face. | `Cost$` if present, otherwise the face's `ManaCost`. |
| `AB$` | An activated ability (also used for abilities with a cost that are run from a trigger, e.g. "you may pay {2}. If you do, …"). | `Cost$` is **required**; a missing `Cost$` is a load error. |
| `ST$` | A "static" ability that resolves without using the stack (special actions). Rare: one `A:` line and 7 SVars in the corpus. | `Cost$` required, as for `AB$`. |
| `DB$` | A sub-ability ("drawback"): never activated on its own; it is reached through `SubAbility$`, `Execute$`, `ReplaceWith$`, a `Choices$` list, etc. | Never has a cost; a `Cost$` key on a `DB$` is ignored by the factory. |

184 distinct API names occur in the corpus (`ChangeZone`, `Pump`, `Draw`, `Token`,
`PutCounter`, …); each has its own parameters, which are outside the scope of this document.

### 2.3 Chaining: `SubAbility$` and the other ability-valued keys

The value of these keys is the **name of an SVar** of the same face whose value is itself an
ability record (normally a `DB$` record):

- `SubAbility$ <SVar>` — the ability to resolve *after* this one. A chain
  `A → SubAbility → SubAbility → …` is the unit Forge calls one spell/ability; the first
  element is the *root*. If the SVar does not exist, Forge prints a warning and the chain ends.
- Keys that hold one extra ability each (`AbilityFactory.additionalAbilityKeys`):
  `WinSubAbility`, `OtherwiseSubAbility` (Clash); `BidSubAbility`; `ChooseNumberSubAbility`,
  `Lowest`, `Highest`, `NotLowest`, `GuessCorrect`, `GuessWrong`, `MatchedAbility`,
  `UnmatchedAbility` (ChooseNumber); `HeadsSubAbility`, `TailsSubAbility`, `LoseSubAbility`
  (FlipCoin); `TrueSubAbility`, `FalseSubAbility` (Branch); `ChosenPile`, `UnchosenPile`
  (TwoPiles / MultiplePiles); `RepeatSubAbility` (Repeat, RepeatEach); `Execute`
  (DelayedTrigger, ImmediateTrigger); `FallbackAbility`; `ChooseSubAbility`,
  `CantChooseSubAbility` (ChoosePlayer); `RegenerationAbility`; `ReturnAbility`;
  `GiftAbility`; `VoteSubAbility`, `VoteTiedAbility`.
- `Choices$ A,B,C` — for the APIs `Charm`, `GenericChoice`, `AssignGroup`,
  `VillainousChoice`, `Vote` only: a comma-separated list of SVar names, each a `DB$`
  record (the modes). For every other API `Choices$` is a *filter* (§4).
- `ResultSubAbilities$ k1:SVarA,k2:SVarB` — `RollDice` only: `key:SVarName` pairs.
- `PreventionSubAbility$ <SVar>`.
- Triggers: `Execute$ <SVar>` names the ability the trigger puts on the stack.
- Replacements: `ReplaceWith$ <SVar>` names the ability that runs instead of the event.
- Effects that create an object with abilities (`Effect`, `Animate`, `Token` modifiers …)
  take comma-separated SVar-name lists in `Abilities$`, `Triggers$`, `ReplacementEffects$` /
  `Replacements$`, `StaticAbilities$` / `staticAbilities$`, `SVars$` / `sVars$`.

Real example (trigger → ability → sub-ability, `d/delver_of_secrets…`):

```
T:Mode$ Phase | Phase$ Upkeep | ValidPlayer$ You | TriggerZones$ Battlefield | Execute$ TrigPeek | TriggerDescription$ …
SVar:TrigPeek:DB$ PeekAndReveal | PeekAmount$ 1 | RevealOptional$ True | RememberRevealed$ True | SubAbility$ DBTransform
SVar:DBTransform:DB$ SetState | Defined$ Self | Mode$ Transform | ConditionDefined$ Remembered | ConditionPresent$ Card.Instant,Card.Sorcery | ConditionCompare$ EQ1 | SubAbility$ DBCleanup
SVar:DBCleanup:DB$ Cleanup | ClearRemembered$ True
```

**How a chain resolves** (`AbilityUtils.resolveApiAbility`): for each element in order — if
its conditions (§2.6) hold, its effect happens (or its `UnlessCost` is offered, §2.7); then,
**whether or not the conditions held**, the next `SubAbility` is processed the same way. A
failed condition skips only that one link, never the rest of the chain. (Effects that branch,
such as `Branch`, `FlipCoin`, `Charm`, decide for themselves which of their extra abilities to
run.)

### 2.4 SVars and how they are referenced

`SVar:<Name>:<Value>` defines a named string on the face. The same mechanism is used for four
unrelated things; what an SVar "is" depends only on who reads it:

| Use | Value looks like | Referenced from |
|---|---|---|
| An ability / trigger / static / replacement record | `DB$ Draw \| NumCards$ 2`, `Mode$ …`, `Event$ …` | the ability-valued keys of §2.3 |
| A numeric expression (§6) | `Count$Valid Creature.YouCtrl`, `Number$3`, `TriggerCount$DamageAmount`, a bare integer | any numeric parameter, by bare name (`NumDmg$ X`); also inside filters (`cmcLEX`) and comparisons (`SVarCompare$ GEX`) |
| An AI / UI hint | `True`, `Curse`, a filter … | read by name by the AI: `AIPreference`, `PlayMain1`, `NeedsToPlayVar`, `SacMe`, `AttachAILogic`, `HasAttackEffect`, … (§8) |
| A flag read by rules code | e.g. `SVar:X:Count$xPaid` marks "X is the announced X" (§3.5, §6) | by fixed name |

Lookup (`AbilityUtils.calculateAmount`, `CardTraitBase.getSVar`): a name is looked up first on
the ability itself (abilities can carry their own SVars, e.g. values set while announcing or
paying), then on the keyword that granted the ability, then on the card state (face), then on
the host card. Names are case-insensitive. Conventional names are just conventions: `X`, `Y`,
`Z` for numbers; `Trig…`, `DB…` for abilities. Nothing but the context distinguishes
`SVar:X:Count$xPaid` from `SVar:X:DB$ Draw`.

A value that contains `$` is never treated as a name: wherever a number is expected,
`NumCards$ Count$xPaid` is evaluated directly as an expression.

### 2.5 Activation restrictions (checked when an ability would be cast / activated)

Source: `SpellAbilityRestriction`. These apply to `SP$`, `AB$`, `ST$` records (and to the
modes of a `GenericChoice`, where they filter the legal choices). Defaults in parentheses.

| Key | Value | Meaning |
|---|---|---|
| `ActivationZone` | zone name (Battlefield; for spells effectively Hand) | Zone the host must be in to activate. `Forecast$` forces `Hand`. |
| `AdditionalActivationZone` | zone | A second zone from which it may also be activated. |
| `Activator` | player filter (§4.4) (`You`) | Who may activate, relative to the host's controller: e.g. `Player` (anyone), `Opponent`. |
| `SorcerySpeed` | present | Only when the activator could cast a sorcery. |
| `InstantSpeed` | present | May be activated any time the player has priority (marks an ability as instant-speed for timing checks). |
| `PlayerTurn` | present | Only during the activator's own turn. |
| `OpponentTurn` | present | Only during a turn of an opponent of the activator. |
| `ActivationPhases` | phase list/range (below) | Only during these phases/steps. |
| `ActivationFirstCombat` | present | Only before/during the first combat phase of the turn. |
| `ActivationAfterBlockers` | present | Only if the declare-blockers step was not skipped (CR 506.7f wording). |
| `ActivationLimit` | number (§6) | Maximum activations **per turn**. |
| `GameActivationLimit` | number | Maximum activations per game. |
| `ActivationGameTypes` | game-variant list | Only in those game variants. |
| `Activation` | `Threshold` / `Metalcraft` / `Delirium` / `Hellbent` / `Blessing` / `Solved` | Named condition on the activator (or, for `Solved`, on the host Case). |
| `IsPresent` | card filter (§4) | Count the objects matching the filter (in `PresentZone`, or among `PresentDefined`) and compare with `PresentCompare`. |
| `PresentCompare` | `<op><number>` (`GE1`) | The comparison for `IsPresent`. `<op>` is two letters (§4.3); the rest is a numeric expression. |
| `PresentZone` | zone list (`Battlefield`) | Zones (of all players) searched for `IsPresent`. Only read when `IsPresent` is given. |
| `PresentDefined` | Defined (§5) | Check the filter against these objects instead of a zone. |
| `ActivationLifeTotal` / `ActivationLifeAmount` | `You` / `<op><number>` (`GE1`) | Compare the activator's life total. (0 uses in the corpus.) |
| `CheckSVar` | SVar name or expression | Evaluate it as a number and compare with `SVarCompare`. |
| `SVarCompare` | `<op><number-or-SVar>` (`GE1`) | The comparison for `CheckSVar`. |
| `ClassLevel` | `<op><n>` (`EQ…`) | Compare the host Class's level. |
| `Planeswalker` | present | It is a loyalty ability (one per turn per permanent, sorcery timing). `Ultimate$ True` is an AI/achievement tag only. |

Phase names (`PhaseType`, case-insensitive): `Untap`, `Upkeep`, `Draw`, `Main1`,
`BeginCombat`, `Declare Attackers`, `Declare Blockers`, `First Strike Damage`,
`Combat Damage`, `EndCombat`, `Main2`, `End of Turn`, `Cleanup`. A list is comma-separated;
`A->B` is an inclusive range, `A->` runs to `Cleanup`, `Main` means both main phases.

Zone names (`ZoneType`, case-insensitive): `Hand`, `Library`, `Graveyard`, `Battlefield`,
`Exile`, `Command`, `Stack`, `Sideboard`, `Ante`, `Flashback`, `Merged`, `SchemeDeck`,
`PlanarDeck`, `AttractionDeck`, `Junkyard`, `ContraptionDeck`, `StickerSheets`, `Subgame`,
`ExtraHand`, `None`. A zone list is split on commas and/or spaces. The single word `All`
means `Battlefield, Hand, Graveyard, Exile, Stack, Library, Command`.

### 2.6 Resolution conditions (checked when each link of a chain resolves)

Source: `SpellAbilityCondition`. These apply to **any** ability record, including `DB$`
links. They are evaluated when that link is about to resolve; a link whose condition fails is
skipped (§2.3).

| Key | Value | Meaning |
|---|---|---|
| `Condition` | one of `Threshold`, `Metalcraft`, `Delirium`, `Hellbent`, `Revolt`, `Blessing`, `EnduringStory`, `Kicked`, `Kicked 1`, `Kicked 2`, `Surge`, `Bargain`, `Teamwork`, `AltCost`, `OptionalCost`, `Foretold` | A named condition on the activator or on how this spell was cast. `OptionalCost` = "the generic optional additional cost was paid". |
| `ConditionOptionalPaid` | `True`/`False` (`True`) | Only read together with `Condition$ OptionalCost`; `False` inverts it ("if it was *not* paid"). |
| `ConditionPresent` | card filter | Count matching objects and compare with `ConditionCompare`. |
| `ConditionCompare` | `<op><number>` (`GE1`) | Only read when `ConditionPresent` is given. |
| `ConditionNotPresent` | card filter | Shorthand for `ConditionPresent` with `EQ0`. |
| `ConditionZone` | zone list (`Battlefield`) | Zones searched. |
| `ConditionDefined` | Defined (§5) | Check the filter against these objects (cards, players or spells) instead of a zone. The idiom `ConditionDefined$ Remembered \| ConditionPresent$ Card` means "if something was remembered". |
| `ConditionPresent2`, `ConditionCompare2`, `ConditionDefined2` | as above | A second, independent present-check (both must hold). `ConditionPresent2` is only read when `ConditionPresent` is also given. |
| `ConditionCheckSVar` | SVar name or expression | Evaluate as a number and compare with `ConditionSVarCompare`. |
| `ConditionSVarCompare` | `<op><number-or-SVar>` (`GE1`) | |
| `OrOtherConditionSVarCompare` (+ optional `OrConditionCheckSVar`) | `<op><number>` | Alternative comparison: the condition holds if either comparison holds. Without `OrConditionCheckSVar` it re-tests the same SVar. |
| `ConditionPlayerTurn` | `True`/`False` | `False` means "not the activator's turn"; anything else means "is the activator's turn". |
| `ConditionOpponentTurn` | present | An opponent's turn. |
| `ConditionSorcerySpeed` | present | The activator could cast a sorcery now. |
| `ConditionPhases` | phase list/range | Current phase is in the list. |
| `ConditionFirstCombat` | present | It is the first combat of the turn. |
| `ConditionGameTypes` | game-variant list | |
| `ConditionActivationLimit` | `<op><integer>` | Compare the number of times this ability was activated this turn. Literal integer only. |
| `ConditionChosenColor` | colour name | The host's chosen colour includes this colour. |
| `ConditionLifeTotal` / `ConditionLifeAmount` | Defined player / `<op><n or X>` (`GE1`) | Compare the first defined player's life. |
| `ConditionPlayerDefined` + `ConditionPlayerContains` | Defined players | Holds if every player of `…Contains` is among `…Defined` (and `…Contains` is non-empty). |
| `ConditionNoDifferentColors` | Defined cards | Holds if the defined cards all have exactly the same colours (and there is at least one). |
| `ConditionManaSpent` / `ConditionManaNotSpent` | colour names separated by spaces | The spell that became the host was (not) paid with all of these colours. |
| `ConditionTargetValidTargeting` | filter | The first targeted spell/ability (or one of its sub-abilities) has a target matching the filter. |
| `ConditionTargetsSingleTarget` | present | The targeted spell/ability has exactly one target in total. |

Inside a replacement's ability, present-checks on Battlefield / Graveyard use the
*last-known* contents of those zones from before the event, not the current ones.

### 2.7 "Unless" costs (any ability link)

Source: `AbilityUtils.handleUnlessCost`, `calculateUnlessCost`.

| Key | Meaning |
|---|---|
| `UnlessCost` | A cost (§3) that one or more players may pay to stop this link's effect. Special values: `ChosenNumber` (generic mana equal to the host's chosen number); `DefinedCost_<Defined>[_Minus<n>\|_Plus<n>]` (the mana cost of the first defined card, adjusted); `DefinedSACost_<Defined>`; `LifeTotalHalfUp` (pay half your life, rounded up); the name of an SVar other than `X` (generic mana equal to that number, or of colour `UnlessColor`); otherwise an ordinary cost string. |
| `UnlessPayer` | Defined players (default `TargetedController`). Each in turn is offered the cost until one pays. |
| `UnlessSwitched` | present → inverted: the effect happens **only if** the cost is paid ("you may pay X. If you do, …"). |
| `UnlessResolveSubs` | `WhenPaid` / `WhenNotPaid`: run the following `SubAbility` only in that case. Absent: sub-abilities always run. Refers to payment, independent of `UnlessSwitched`. |
| `UnlessUpTo`, `UnlessColor`, `UnlessAI` | modifiers / AI hint. |

Real examples:
`A:SP$ Counter | TargetType$ Spell | ValidTgts$ Card | UnlessCost$ 1 | …` ("counter target
spell unless its controller pays {1}"), and
`A:SP$ Draw | NumCards$ 2 | UnlessCost$ Discard<1/Card> | UnlessPayer$ You | UnlessSwitched$ True`
("you may discard a card. If you do, draw two cards").

### 2.8 Keys common to every trigger, static and replacement

Source: `CardTraitBase.meetsCommonRequirements` (shared by triggers, replacements, and — in
part — statics), `Trigger`, `WrappedAbility.resolve`, `StaticAbility.checkConditions`,
`ReplacementHandler`.

Shared "intervening if" keys (triggers and replacements; checked when the event happens, and
for triggers again on resolution — see below):

| Key | Value | Meaning |
|---|---|---|
| `IsPresent`, `PresentCompare`, `PresentZone` | as §2.5 | Present-check. |
| `PresentPlayer` | `You` / `Opponent` / `Any` (`Any`) | Whose zones are searched. |
| `PresentDefined` | Defined cards | Check these objects instead. |
| `IsPresent2`, `PresentCompare2`, `PresentZone2`, `PresentPlayer2` | | Second independent present-check. |
| `CheckSVar`, `SVarCompare` | as §2.5 | Numeric check. |
| `CheckSecondSVar`, `SecondSVarCompare` | | Second numeric check (only read when `CheckSVar` is present). |
| `CheckDefinedPlayer`, `DefinedPlayerCompare` | Defined players, `<op><n>` (`GE1`) | Compare the *number* of defined players. |
| `LifeTotal`, `LifeAmount` | `You` / `OpponentSmallest` / `OpponentGreatest`, `<op><n>` (`GE1`) | Life-total check. |
| `Metalcraft`, `Delirium`, `Threshold`, `Hellbent`, `Bloodthirst`, `FatefulHour`, `Monarch`, `Revolt`, `Blessing`, `EnduringStory` | `True`/`False` | The host's controller does / does not have that state. |
| `DayTime` | `Day` / `Night` / `Neither` | |
| `Adamant` | colour name or `Any` | At least three mana of that colour were spent to cast the host. |
| `ManaSpent`, `ManaNotSpent` | colour | Mana of that colour was (not) spent to cast the host. |
| `ClassLevel` | integer | Host's Class level is at least this. |
| `WerewolfTransformCondition`, `WerewolfUntransformCondition` | present | "No spells were cast last turn" / "a player cast two or more spells last turn". |

Trigger-only generic keys:

| Key | Meaning |
|---|---|
| `Mode` | Trigger type (131 types in the corpus). |
| `Execute` | SVar of the ability to put on the stack. |
| `TriggerZones` | Zone list the host must be in for the trigger to function. Absent: the trigger works from any zone (most self-referential triggers rely on the event itself to pin the zone). 9,711 uses. |
| `Phase` | Phase list/range (same syntax as `ActivationPhases`); used by `Mode$ Phase` but read for every trigger. |
| `PlayerTurn`, `NotPlayerTurn`, `OpponentTurn` | present → only on the host controller's turn / not on it / on an opponent's turn. |
| `FirstUpkeep`, `FirstUpkeepThisGame`, `FirstCombat`, `TurnCount` | present / integer → timing restrictions. |
| `OptionalDecider` | Defined player who may decline the whole trigger ("you may"). The first defined player decides. |
| `Static` | present → the triggered ability resolves immediately, without using the stack (mana-ability triggers and script helpers). |
| `Secondary` | `True` → hidden from the text box; no rules meaning. |
| `ResolvedLimit` | integer → triggers only this many times each turn (CR 603.2i style "this ability triggers only once each turn"). |
| `ActivationLimit`, `GameActivationLimit` | integer → limit how many times it can trigger per turn / game. |
| `NoResolvingCheck` | present → do **not** re-check the conditions on resolution. |
| `ResolvingCheck` | name of one of the shared keys above → re-check just that key on resolution. |
| `APlayerHasMoreLifeThanEachOther`, `APlayerHasMostCardsInHand` | present → special-purpose conditions. |
| `Condition$ Evolve` | special-purpose. |
| `TriggerDescription` | text only. |

Re-check on resolution (`WrappedAbility.resolve`): every trigger **except** `Mode$ Always`
(state triggers) re-evaluates its shared conditions when it resolves, unless
`NoResolvingCheck` is present. So in Forge any `IsPresent` / `CheckSVar` on a trigger behaves
as an intervening-if (CR 603.4) by default.

Static-only generic keys: `Mode`; `EffectZone` (zone list where the host must be for the
static to work; default `Battlefield`; `All` for everywhere); `Affected` (filter),
`AffectedZone`; `Condition` (`Threshold`, `Hellbent`, `Metalcraft`, `Delirium`, `Ferocious`,
`Blessing`, `EnduringStory`, `Monarch`, `Night`, `MaxSpeed`, `PlayerTurn`, `NotPlayerTurn`,
`ExtraTurn`, `FatefulHour`); `Phases`; `PlayerTurn` (Defined players — note: different meaning
from the trigger key); `TopCardOfLibraryIs` (filter); `IsPresent` / `PresentCompare` /
`PresentZone`; `CheckSVar` / `SVarCompare`; `GameStage`; `ClassLevel`; `CharacteristicDefining`;
`Description` (text).

Replacement-only generic keys: `Event`; `ReplaceWith` (SVar); `ActiveZones` (zone list where
the host must be; 800 uses); `Layer` (`CantHappen` / `Control` / `Copy` / `Transform` /
`Other`, default `Other`); `Optional` (present → a player may decline to apply it; `OptionalDecider$` names who);
`Prevent$ True` (the event simply does not happen); `ReplacementResult$ Updated` (the
replacement modified the event, which then continues — e.g. "enters tapped"); `Description`.

### 2.9 Targeting keys (any ability link)

Source: `TargetRestrictions`. A link targets if and only if it has `ValidTgts$`.

| Key | Value (default) | Meaning |
|---|---|---|
| `ValidTgts` | filter list (§4) | What may be targeted. `Any` = "any target" (creature, planeswalker, battle or player). |
| `TargetMin`, `TargetMax` | numbers (`1`, `1`) | Number of targets. "Up to one" is `TargetMin$ 0`. |
| `TgtZone` | zone list (`Battlefield`) | Zone(s) of the targets (`Graveyard`, `Stack`, …). For `ChangeZone`, the factory also sets it from `Origin$` when the target is not a player. |
| `TargetType` | SA filter | Restricts targeted stack objects (e.g. `Spell`, `Activated`, `Triggered`). |
| `TargetValidTargeting` | filter | For "target spell that targets X". |
| `TargetUnique` | present | The same object may not be targeted by this spell's other links. |
| `TargetsWithSameCreatureType`, `TargetsWithoutSameCreatureType`, `TargetsWithSameCardType`, `TargetsWithSameController`, `TargetsWithDifferentControllers`, `TargetsWithDifferentCMC`, `TargetsWithDifferentNames`, `TargetsWithEqualToughness`, `TargetsForEachPlayer` | present | Constraints among the chosen targets. |
| `MaxTotalTargetCMC`, `MaxTotalTargetPower` | number | Upper bound on the sum over the targets. |
| `TargetsAtRandom`, `RandomNumTargets` | present | Targets / their number are chosen at random. |
| `TargetingPlayer` | Defined player | Another player chooses the targets (which also makes targeting mandatory). |
| `TargetingPlayerControls` | present | Targets must be controlled by the targeting player. |
| `DividedAsYouChoose` | number | The amount is divided among the targets as the controller chooses. |
| `TgtPrompt`, `ValidTgtsDesc` | text | UI only. |

Which objects an effect acts on (`SpellAbilityEffect.getCards` / `getPlayers`): if the link
targets, it acts on **its own targets**; otherwise it acts on `Defined$` (§5), which defaults
to `Self` for card-affecting effects and `You` for player-affecting ones. Some effects prefer
`Defined$` over their targets when both are present ("defined first"); and
`ThisDefinedAndTgts$ <Defined>` unions a Defined with the targets.

---

## 3. The `Cost$` sub-language

Source: `Cost.java` (constructor `Cost(String, boolean, boolean)`, `parseCostPart`,
`abCostParse`), `CostPart.java`, one `Cost*.java` class per part, `TextUtil.splitWithParenthesis`,
`PlaySpellAbility` (announcing X), `TriggerHandler` (costs on triggered abilities).

The same language is used by `Cost$`, `UnlessCost$`, the cost arguments of keywords
(`K:Kicker:…`, `K:Equip:…`, `K:Cumulative upkeep:…`) and cost-valued parameters of statics
(`Mode$ AlternativeCost | Cost$ …`, `RaiseCost`, …).

### 3.1 Grammar

```
cost      := token ( " " token )*
token     := "T" | "Q" | "Untap" | "Forage" | "Mandatory" | "XMin" digits
           | name "<" arg ( "/" arg )* ">"
           | mana-token                      -- anything else
```

- The string is split on spaces, except that spaces between `<` and `>` do not split
  (so `Sac<1/Creature.Other/another creature>` is one token). Empty tokens are dropped.
- Each token is tried against the list of part names in §3.3 by `startsWith("Name<")`.
- **Any token that is not recognised is treated as mana**: all such tokens are joined with
  spaces and parsed as one mana cost (§1.3). There is no error for an unknown word; it just
  contributes nothing (the corpus contains one `Cost$ True`, which is a free cost).
- Inside `<…>` the text up to the first `>` is split on `/` into at most *n* pieces, where *n*
  is fixed per part (given below). Because of the limit, the **last** piece may itself contain
  `/`. Empty pieces are dropped.
- The parts are then **sorted by a fixed payment order**, not kept in script order (tap first,
  mana early, life/counters next, zone changes later, random/hidden-information parts last).
  A translator must not assume script order is payment order.
- `Cost$ 0` is "free". A cost with no mana token has no mana part.

Flags:

| Token | Meaning |
|---|---|
| `Mandatory` | The cost is not optional. Used on `UnlessCost$` and on the cost of a triggered `AB$` ability (§3.4). 29 uses. |
| `XMin<n>` | Minimum value for an announced X in the mana part (`XMin1`). 17 uses. (The ability-level key `XMin$ n` does the same.) |

### 3.2 Common argument conventions

Most parts are `Name<Amount/Type/Description>`.

**Amount** — evaluated with `AbilityUtils.calculateAmount` (§6):

- a literal integer;
- `X` (or another SVar name such as `Y`): the value of that SVar. If the SVar is
  `Count$xPaid` (or absent), X is *announced* by the player when paying (§3.5); otherwise it is
  computed (e.g. `SVar:X:Count$Valid Creature.YouCtrl`);
- `All` — recognised by `Sac` ("sacrifice all …", always payable) and `SubCounter` (remove all
  counters of that type from the source);
- `X1+` — `SubCounter` and `RemoveAnyCounter` only: X, announced, at least one ("remove one
  or more");
- `Any` — used internally by `Teamwork`; `tapXType<Any/…>` occurs twice. Other spellings found
  in the corpus — `1+` (`Exile<1+/…>` "one or more"), `Tapped` (`tapXType<Tapped/Creature>`) —
  are handled by the payment UI code, which is not in the checkout: UNVERIFIED.

**Type** — which objects can pay:

- `CARDNAME` or `NICKNAME` — the host itself ("sacrifice this creature").
- `OriginalHost` — the object that granted the ability (e.g. the Equipment that gives the
  equipped creature an ability): accepted by `Sac`, `tapXType`, `SubCounter`, `Exile`,
  `Unattach`.
- otherwise a **card filter** (§4), evaluated relative to the paying player and the host,
  over that player's cards in the part's zone. **Alternatives are separated by `;`, not `,`**
  (`ExileFromGrave<1/Instant;Sorcery/instant or sorcery>`), because `/` and `,` are taken.
- A few parts accept pseudo-types or pseudo-property suffixes that are *not* filters; they are
  listed per part below and must be stripped before the remainder is parsed as a filter.

**Description** — optional display text for the type (`another creature`). No rules meaning.

**Recorded payment.** Parts that choose cards remember them on the ability under a fixed list
name. Those lists can be read later in the same chain through `Defined$ <ListName>` (§5) and
`<ListName>$<expr>` numeric expressions (§6), e.g. `SVar:X:Sacrificed$CardPower`. The values
recorded are last-known-information copies taken when the cost was paid.

### 3.3 Every cost part

Counts are occurrences inside `Cost$` values in the corpus.

**Tap / untap**

| Token | Count | Meaning |
|---|---|---|
| `T` | 5,653 | Tap the host ({T}). Unpayable if the host is tapped or is a creature its controller has not controlled continuously since the turn began ("summoning sick"). |
| `Q` or `Untap` | 18 | Untap the host ({Q}). Same summoning-sickness rule; needs the host tapped (and removable stun counters). |
| `tapXType<Amount/Type/Desc>` (3) | 245 | Tap *Amount* untapped permanents you control matching Type. If the cost also contains `T`, the host cannot be one of them. List: `Tapped`. Pseudo-suffixes: `.sharesCreatureTypeWith` (the tapped permanents must share a creature type), `+withTotalPowerGE<n>` (tap any number with total power ≥ n; used by Crew-like costs). |
| `untapYType<Amount/Type/Desc>` (3) | 5 | Untap *Amount* tapped permanents matching Type — searched on the whole battlefield, so add `YouCtrl`/`OppCtrl` in the filter. List: `Untapped`. |

**Mana**

| Token | Meaning |
|---|---|
| mana tokens | §1.3. `2 W W`, `X R`, `WB`, `S`, `C`. |
| `Mana<cost\restriction>` (10) | An explicit mana part; note the **backslash**. `cost` is a §1.3 mana string (may contain spaces). `restriction` is one of: `NumTimes` ("pay this any number of times" — the count is announced into the SVar named by `Announce$`), `Exiled` (add the mana cost of the card exiled by this cost's `Exile…` part), `EnchantedCost` (add the mana cost of the enchanted creature), `XMin<n>`. If an explicit `Mana<…>` is present, loose mana tokens in the same cost are ignored. The form `Mana<2\Creature>` shown in Forge's own docs (mana that must come from a source of a given type) is **not** implemented by this parser: UNVERIFIED / stale. |
| `Waterbend<mana>` (1) | 27 uses. A mana part flagged as a "waterbend" cost (`CostWaterbend` is a mana part that records the waterbend amount; the extra ways of paying it are implemented outside the cost class — UNVERIFIED here). `Waterbend<X>` occurs. |

**Sacrifice / discard / reveal**

| Token (max pieces) | Count | Meaning |
|---|---|---|
| `Sac<Amount/Type/Desc>` (3) | 2,375 | Sacrifice *Amount* permanents you control matching Type. `Sac<1/CARDNAME>` = sacrifice the host. `Sac<X/…>` = any number, `Sac<All/…>` = all of them. Pseudo-suffix `+WithDifferentNames`. List: `Sacrificed`. |
| `Discard<Amount/Type/Desc>` (3) | 654 | Discard *Amount* cards from hand matching Type. Pseudo-types: `Hand` (discard your whole hand; amount ignored; payable with an empty hand), `Random` (at random), `LastDrawn` (the last card you drew this turn), `CARDNAME` (discard the host from hand, e.g. cycling-like costs). Pseudo-suffixes `+WithSameName`, `+WithDifferentNames`. If the host is in hand it cannot be discarded to pay its own cost (unless Type is `CARDNAME`). List: `Discarded`. |
| `Reveal<Amount/Type/Desc>` (3) | 55 | Reveal *Amount* cards from your hand matching Type. Pseudo-types: `Hand` (reveal your hand), `SameColor` (cards that share a colour), `CARDNAME`. A spell cannot reveal itself. List: `Revealed`. |
| `RevealFromExile<Amount/Type/Desc>` | 1 | As `Reveal`, from exile. |
| `RevealOrChoose<Amount/Type/Desc>` | 2 | As `Reveal`, choosing from hand **or** battlefield. |
| `ChooseCard<Amount/Type/Desc>` | 1 | As `Reveal`, choosing from all zones (`All`). |
| `Behold<Amount/Type/Desc>` | 12 | "Behold": choose a matching permanent you control or reveal a matching card from hand. Type ending in `ChosenType` means "sharing the chosen creature type". |
| `BeholdExile<Amount/Type/Desc>` | 5 | Behold, then exile the beheld card. |
| `RevealChosen<What/Desc>` (2) | 4 | Reveal a secretly chosen `Player` or `Type`. |

**Zone changes**

| Token (max pieces) | Count | Meaning |
|---|---|---|
| `Exile<Amount/Type/Desc>` (3) | 82 | Exile from the battlefield, among permanents you control. `Exile<1/CARDNAME>` exiles the host. |
| `ExileFromHand<…>` | 55 | Exile cards from your hand. |
| `ExileFromGrave<…>` | 251 | Exile cards from **your** graveyard. `ExileFromGrave<1/CARDNAME>` = exile the host from the graveyard. |
| `ExileFromTop<…>` | 8 | Exile cards from the top of your library (Type is normally `Card`). |
| `ExileAnyGrave<…>` | 18 | Exile cards from **any** graveyard. |
| `ExileSameGrave<…>` | 1 | Exile cards that are all in a single graveyard (any player's). |
| `ExileCtrlOrGrave<…>` | 1 (plus keyword uses) | Exile from among permanents you control and/or cards in your graveyard (Craft). |
| `ExileFromStack<Amount/Type/Desc>` | 4 | Exile spells from the stack (Type is a filter over stack objects, e.g. `Spell.Instant+YouCtrl`). |
| `ExiledMoveToGrave<Amount/Type/Desc>` | 16 | Put *Amount* cards matching Type from exile into their owners' graveyards. List: `MovedToGrave`. |
| `Return<Amount/Type/Desc>` (3) | 88 | Return permanents you control to their owner's hand. `Return<1/CARDNAME>` returns the host. List: `Returned`. |
| `PutCardToLibFromHand<Amount/LibPos/Type/Desc>` (4) | 4 | Put cards from your hand into your library at position LibPos (`0` = top, `-1` = bottom). List: `CardPutToLib`. |
| `PutCardToLibFromGrave<…>` | 6 | Same, from your graveyard. |
| `PutCardToLibFromSameGrave<…>` | 0 | Same, all from one graveyard of any player. |
| `PutCardToLibFromBattlefield<…>` | 1 | Same, from the battlefield. |
| `Mill<Amount>` (1) | 9 | Mill that many cards. List: `Milled`. |
| `Draw<Amount/PlayerFilter>` (2) | 52 | Chosen player(s) matching the player filter (§4.4) draw *Amount* cards. `Draw<1/You>` = "draw a card" as a cost. |
| `CollectEvidence<N>` (1) | 24 | Exile cards from your graveyard with total mana value ≥ N. List: `Collected`. |
| `Forage` (bare word) | 8 | Exile three cards from your graveyard or sacrifice a Food. List: `Foraged`. |

All the `Exile…` forms record into the list `Exiled`. Pseudo-types / suffixes accepted by
`Exile…`: `All` (exile all cards of that zone; always payable), `FromTopGrave`,
`+withTotalCMCEQ<n>`, `+withTotalCMCGE<n>` (the chosen cards' total mana value), 
`+withSharedCardType`, `+withTypesGE<n>` (number of card types among them).

**Counters**

| Token (max pieces) | Count | Meaning |
|---|---|---|
| `SubCounter<Amount/CounterType/Type/Desc/Zone>` (5) | 954 | Remove *Amount* counters of that type. Type defaults to `CARDNAME` (from the host); otherwise the counters come from **one** permanent matching Type. Zone defaults to `Battlefield`. `CounterType` `Any` means counters of any type. `SubCounter<3/LOYALTY>` is a planeswalker "−3". The number actually removed is stored on the ability as SVar `CostCountersRemoved`. |
| `AddCounter<Amount/CounterType/Type/Desc>` (4) | 438 | Put *Amount* counters of that type on the host (default) or on one permanent matching Type. `AddCounter<1/LOYALTY>` is "+1", `AddCounter<0/LOYALTY>` is "0". `AddCounter<1/M1M1>` is "put a −1/−1 counter on this". List: `CounterPut`. |
| `AddCounterYou<Amount/CounterType>` (2) | 0 | The paying *player* gets counters. |
| `RemoveAnyCounter<Amount/CounterType/Type/Desc>` (4) | 43 | Remove *Amount* counters (of that type, or `Any`) **spread over** any permanents you control matching Type (or the host if Type is `CARDNAME`). |
| `Blight<N>` (1) | 21 | Put N −1/−1 counters on a creature you control (shorthand for `AddCounter<N/M1M1/Creature.YouCtrl/…>`). |
| `PayEnergy<Amount>` (1) | 126 | Pay that many energy counters. |
| `PayShards<Amount>` | 0 | Forge "Adventure mode" currency; not a Magic cost. |

Counter type names are written in capitals with `+`→`P`, `-`→`M` and no slash: `P1P1`,
`M1M1`, `LOYALTY`, `CHARGE`, `TIME`, `LORE`, … (`CounterEnumType.getType` normalises
`+1/+1` to `P1P1`). A name that is a keyword (`Flying`) is a keyword counter; an unknown name
becomes a custom counter type.

**Life / damage / other**

| Token (max pieces) | Count | Meaning |
|---|---|---|
| `PayLife<Amount/Desc>` (2) | 327 | Pay that much life. |
| `GainLife<Amount/PlayerFilter/Count>` (3) | 3 | *Count* players matching the filter each gain *Amount* life (`Count` default 1; `*` = every matching player). "An opponent gains 3 life" as a cost. |
| `DamageYou<Amount>` (1) | 18 | The host deals that much damage to you. |
| `Exert<Amount/Type/Desc>` (3) | 36 | Exert the host (`CARDNAME`) or matching creatures. List: `Exerted`. |
| `Enlist<Amount/Type/Desc>` | 0 (keyword) | Tap an untapped non-attacking creature without summoning sickness. |
| `Unattach<Type/Desc>` (2) | 9 | Unattach an Equipment: `CARDNAME` (the host), `OriginalHost`, or a filter over Equipment attached to the host. |
| `GainControl<Amount/Type/Desc>` | 0 | Gain control of permanents matching Type. |
| `ChooseColor<N>`, `ChooseCreatureType<N>` (1) | 0 / 2 | Choose a colour / creature type as part of the cost; stored as the host's chosen colour / type. |
| `FlipCoin<N>`, `RollDice<N/Sides/ResultSVar/Desc>` (4) | 0 / 1 | Flip coins / roll dice; the dice total is stored in the named SVar. |
| `AddMana<Amount/Type/Desc>` | 0 | Add mana as a "cost". |
| `PromiseGift` | 0 (keyword) | Choose an opponent to promise a gift to. |
| `Teamwork<N>` | 0 (keyword) | Tap any number of creatures with total power ≥ N. |

Real examples:

```
A:AB$ DealDamage | Cost$ 1 T Sac<1/CARDNAME> | ValidTgts$ Any | NumDmg$ 2 | SpellDescription$ It deals 2 damage to any target.
A:AB$ DealDamage | Cost$ SubCounter<2/LOYALTY> | Planeswalker$ True | ValidTgts$ Creature.tapped | NumDmg$ 4 | …      (a/ajani_outland_chaperone.txt)
A:SP$ Draw | Cost$ 1 B Sac<1/Creature> | NumCards$ 2 | SpellDescription$ Draw two cards.                              (a/altars_reap.txt)
A:AB$ ChangeZone | Cost$ 2 W T Exile<1+/Artifact.Other+withTotalCMCEQX/other artifacts you control with total mana value X> | Announce$ X | …   (f/fabrication_foundry.txt)
S:Mode$ AlternativeCost | ValidSA$ Spell.Self | EffectZone$ All | Cost$ ExileFromHand<1/Card.Red+Other+cmcEQX/red card> | Announce$ X | …
```

### 3.4 Where costs appear, and optional costs on triggers

- `A:SP$ …` without `Cost$`: the cost is the face's mana cost. With `Cost$`, that string is
  the **whole** cost, so additional costs are written together with the mana:
  `A:SP$ Destroy | Cost$ 1 B Sac<1/Creature> | …`.
- `A:AB$ … | Cost$ …`: activation cost.
- An `AB$` record reached from a trigger's `Execute$` (an SVar) is a triggered ability with a
  cost paid **on resolution**. If the trigger has no `OptionalDecider$` and the cost is not
  `0` and not flagged `Mandatory`, the trigger is automatically treated as optional
  ("you may pay … If you do, …"): the controller is asked, and declining skips the ability
  (`TriggerHandler`, "triggers with a cost can't be mandatory").
- `UnlessCost$` (§2.7).

### 3.5 X and announced values

Source: `PlaySpellAbility.announceValuesLikeX`, `AbilityUtils.getAnnouncementBounds`,
`AbilityUtils.xCount` (`xPaid`).

- A cost "has X" if its mana part contains an `X` symbol or any other part's Amount is exactly
  `X`.
- If the cost has X and the ability's SVar `X` is exactly `Count$xPaid` (or is empty), the
  player **announces** X. Otherwise the SVar `X` is evaluated and that is X (so
  `SVar:X:Count$Valid Creature.YouCtrl` makes "X" a computed number, not a choice).
- `Announce$ A,B` forces announcement of each listed variable. `X` is stored as the ability's
  paid-X; any other name is stored as an SVar on the ability and on the card, readable by name
  afterwards (`Announce$ NumTimes` with `Mana<2 R\NumTimes>`).
- Bounds: `XMin$ n` / the `XMin<n>` cost token (minimum), `XMax$ <expr>` and
  `AnnounceMax$ <expr>` (maximum).
- Elsewhere in the script the announced value is read with `Count$xPaid` (§6.3), hence the
  ubiquitous `SVar:X:Count$xPaid`.

---

## 4. The "Valid" filter sub-language

Source: `GameObject.isValid(String[] …)`, `Card.isValid` / `Card.ParsedRestriction` /
`Card.hasProperty`, `CardProperty.cardHasProperty`, `CardStateProperty.hasProperty`,
`CardType.hasStringType`, `Player.isValid`, `PlayerProperty.playerHasProperty`,
`SpellAbility.isValid`, `SpellAbilityProperty.hasProperty`, `Expressions.compare`,
`CardTraitBase.matchesValid`.

Used by `ValidTgts$`, `ValidCard$`, `ValidCards$`, `ValidSource$`, `ValidTarget$`,
`ValidPlayer$`, `ValidActivatingPlayer$`, `Affected$`, `ChangeType$`, `ChangeValid$`,
`SacValid$`, `IsPresent$`, `ConditionPresent$`, `Choices$` (when not a mode list), the Type
slot of costs, `Count$Valid …`, `Defined$ Valid …`, and many more keys.

### 4.1 Shape

```
filter       := alternative ( "," alternative )*          -- OR
alternative  := [ "!" ] typeword [ "." property ( "+" property )* ]   -- AND
property     := [ "!" ] token
```

- The caller splits on `,` (in cost Type slots: on `;`). An object matches the filter if it
  matches **any** alternative.
- An alternative is split at its **first** `.` into the *type word* and the property list; the
  property list is split on **every** `+`. All properties must hold.
- `!` in front of the type word negates the **whole alternative** (cards and spell abilities
  only): `!Creature.Blue` would be "not (a creature that is blue)". (Illustrative; whole-alternative negation is rare — the corpus only has bare `!Creature`, in `AITgts$`.)
- `!` in front of a property negates that property: `Creature.YouCtrl+!token`. (For types and colours the `non…` prefix is the more common spelling: `Creature.nonBlack+nonArtifact`.)
- Property tokens are otherwise free text and **may contain spaces, dots, colons and
  underscores**: `ControlledBy TriggeredPlayer`, `EnchantedBy Aura.Other`,
  `sharesCreatureTypeWith Valid Creature`, `counters_GE1_P1P1`. Only `,` and `+` (and the
  first `.`) are structural. A property therefore cannot contain a literal `,` or `+`; where a
  nested list is needed the script writes `;` for `,` (`namedBruna; the Fading Light`,
  `DamagedByGiant.YouCtrl;Wizard.YouCtrl`), and one property uses `~` for `+`
  (`IsTargeting`).
- Matching is **case-sensitive**.
- A filter is always evaluated relative to three things: a **reference player** ("You" —
  normally the controller/activator of the ability that owns the filter), a **source card**
  (the host; this is what `Self`, `Other`, `IsRemembered`, `EnchantedBy` … refer to) and the
  **ability context** (needed by `Triggered…`, `targetedBy`, and for SVar look-ups inside
  comparisons).
- The same filter list may be applied to a mix of cards, players and spells (e.g.
  `ValidTgts$ Creature,Player`): each object is tested with its own rules, and a card never
  matches a player alternative or vice versa.

**How a property token is recognised.** `CardProperty.cardHasProperty` is one long
`if / else if` chain over the token, most branches testing `startsWith(prefix)` and a few
testing `equals`. The **first** branch whose test matches decides; later branches are never
consulted. Consequences:

- Prefix matches accept any suffix: `YouCtrl` and `YouCtrlXyz` are the same test. Do not
  assume an unknown-looking suffix is an error or has meaning unless listed below.
- Order matters where one name is a prefix of another (`EnchantedBy…` is tested before
  `Enchanted…`; `ThisTurnEnteredFrom…` before `ThisTurnEntered…`; `StrictlyOther` before
  `Other`). The tables below already describe the effective behaviour.
- A token that matches no branch falls through to the card-state properties (§4.6), whose
  last resort is "the card has this type/supertype/subtype". That is how `Creature.Elf`,
  `Permanent.Artifact`, `Land.Basic`, `Card.Legendary` work.

A **phased-out** card matches nothing, unless the property token starts with `phasedOut`, in
which case that prefix is removed and the rest is tested (`phasedOutSelf`,
`phasedOutIsRemembered`, `phasedOutCreature`).

### 4.2 The type word

For **cards** (`Card.isValid`):

| Type word | Matches |
|---|---|
| `Card` (or `card`) | Any real card or token, in any zone. Excludes Forge's internal "effect" objects (emblems, effect cards in the command zone). |
| `Permanent` | Not an effect object, and either on the battlefield or having a permanent card type (`Card.isPermanent`). So it also matches permanent *cards* in other zones. |
| `Spell` | **An instant or sorcery card, or an Aura that is not on the battlefield** (`Card.isSpell`). It does *not* test that the card is on the stack and does not match creature spells; "a spell" in the rules sense is expressed with an SA filter (below) or with `Card` plus a stack zone. |
| `Effect` | A Forge effect object (the pseudo-card created by `DB$ Effect`). |
| `Emblem`, `Boon` | Emblems / boons. |
| `Any` | Creature, planeswalker or battle ("any target" minus players). Not negatable. |
| anything else | The card **has that type**: tried first as a subtype exactly as written (creature types respect "is every creature type"), then, with the first letter capitalised, as a card type or supertype. So `Creature`, `Artifact`, `Legendary`, `Basic`, `Snow`, `Elf`, `Forest`, `Aura`, `Equipment` are all valid type words. `Goblin` alone also matches a Goblin *Kindred* enchantment; `Creature.Goblin` is "Goblin creature". |

For **players** (`Player.isValid`):

| Type word | Matches |
|---|---|
| `Player` | Any player. |
| `You` | The reference player. |
| `Opponent` | An opponent of the reference player. |
| `Any` | Any player. |
| anything else | No player. (So `!Player` never matches; negation is only available on properties.) |

For **spells and abilities** on the stack or being cast (`SpellAbility.isValid`, used by
`ValidSA$`, `TargetType$`, `CastSa …`, `hasAbility …`): `Spell`, `Ability` (any non-spell
ability), `Instant`, `Sorcery`, `Activated`, `Triggered`, `Static`, a word containing
`LandAbility`, or `SpellAbility` (anything). All are judged on the *root* of the chain.

Note the two different senses of `Spell`: as a **card** type word it means "instant, sorcery or
not-on-battlefield Aura card"; in an **SA** filter it means "this stack object is a spell, not an ability".
Which one applies depends on whether the parameter is matched against a card or against a
spell/ability object (e.g. `ValidSource$ Spell` on a `BecomesTarget` trigger vs.
`DamagedBySpell.YouCtrl`). Trigger parameters
such as `ValidCard$` on `SpellCast` take card filters; `ValidSA$` takes SA filters.

### 4.3 Comparisons

Several properties embed a comparison `<OP><RHS>`:

- `<OP>` is two capital letters: `LT` `LE` `EQ` `GE` `GT` `NE`, plus `M2` ("same parity").
- `<RHS>` is a numeric expression (§6) evaluated **relative to the source card and the
  ability context**, not the object being tested: a literal (`cmcLE3`), an SVar name
  (`cmcLEX`, with `SVar:X:…` on the host), or — for the P/T/CMC family only — the word
  `Chosen` (the source's chosen number).

Caveat for a parser: for the P/T/CMC family Forge extracts `<RHS>` by fixed offset but picks
the operator by searching the **whole token** for `LT`, `LE`, `EQ`, `GE`, `GT`, `NE`, `M2` in
that order. This equals "the two letters after the attribute name" unless the SVar name
itself contains an earlier operator in capitals (e.g. an SVar called `LEFT` after `GE`).
Parse the two letters positionally and flag such names.

### 4.4 Player properties

Source: `PlayerProperty.playerHasProperty`. An unknown token makes the alternative fail.
Reference player = "you"; source = host.

Relationship

| Token | Meaning |
|---|---|
| `You` | Is the reference player. |
| `Opponent` | Is an opponent of the reference player. |
| `OpponentOf <DefinedPlayers>` | Is an opponent of every defined player. |
| `Other` | Is not the reference player. |
| `Allies` | Is a teammate of, but not, the reference player. |
| `YourTeam` | Is on the reference player's team (including them). |
| `Active` | It is this player's turn. |
| `Activator` | Is the controller of the ability context's host. |
| `CardOwner` | Owns the source card. |
| `Chosen` | Is the source's chosen player. |
| `IsRemembered` | Is remembered by the source. |
| `IsRememberedOrController` | Is remembered by the source, or controls a card remembered by it. |
| `IsTriggerRemembered` | Is among the trigger-remembered objects of the ability context. |
| `Triggered<Key>…` | Is one of the players that `Defined$ Triggered<Key>…` (§5.2) would return, e.g. `TriggeredTarget`, `TriggeredCardController`. |
| `OriginalHostRemembered` | Same, via `Defined$ OriginalHostRemembered`. |
| `EnchantedBy` | Is enchanted by the source. |
| `EnchantedController` | Controls the permanent the source is attached to. |
| `CanBeEnchantedBy` | The source could legally be attached to this player. |
| `targetedBy` | Is a target of the current spell/ability (root). |
| `PlayerUID_<n>` | Internal id match. |

State

| Token | Meaning |
|---|---|
| `life<OP><RHS>` | Life total comparison (`Player.lifeEQ13`, `Player.Opponent+lifeEQX`). |
| `counters_<OP><RHS>_<TYPE>` | Number of counters of that type on the player (corpus: `Player.counters_EQ0_C`). |
| `IsPoisoned` / `IsCorrupted` | Has ≥ 1 / ≥ 3 poison counters. |
| `isMonarch`, `hasInitiative`, `hasBlessing`, `hasEnduringStory` | Designations. |
| `NoSpeed`, `MaxSpeed` | Speed (Aetherdrift). |
| `controls<filter>[_<OP><RHS>]` | Controls permanents matching the card filter; default "at least one". `_` separates the comparison; a literal `_` inside the filter is written `\_`. Example `controlsCreature.attacking`. |
| `HasCardsIn<Zone>_<filter>_<OP><RHS>` | Count of the player's cards in that zone matching the filter (`HasCardsInHand_Card_EQ0`). |
| `withMore<Type>sThan<Who>` | Controls more permanents of that type than `<Who>` (`You` or a Defined player): `withMoreLandsThanYou`. |
| `withAtLeast<N>More<Type>sThan<Who>` | …at least N more (single digit N). |
| `hasMoreLife[Than<Who>]`, `hasMoreCardsInHand[Than<Who>]` | Strictly more life / cards in hand than `<Who>` (default: you). |
| `hasFewer<Type>sInPlayThan<Who>`, `hasFewerCreaturesInYardThan<Who>` | Strictly fewer permanents of that type / creature cards in graveyard. |
| `withMostLife`, `withMostCardsInHand`, `withMostPermanentInPlay`, `withMostType<Type>[Only]` | Has the most (ties allowed for life and for `Type` without `Only`; strict for the others). |
| `withLowestLife` | Has the lowest life total (ties allowed). |
| `Condition[<OP><RHS>] <playerExpr>` | Evaluates a per-player numeric expression (§6.5) and compares (default `GE1`). |

History this turn / combat

| Token | Meaning |
|---|---|
| `LostLifeThisTurn[ <OP><RHS>]` | Lost life this turn (default: any). |
| `descended`, `committedCrimeThisTurn`, `VenturedThisTurn`, `castSpellThisTurn`, `attackedWithCreaturesThisTurn`, `TappedLandForManaThisTurn` | Did that this turn. |
| `CardsInHandAtBeginningOfTurn` | Had at least one card in hand as the turn began. |
| `wasDealtDamageThisTurn`, `wasDealtCombatDamageThisTurn`, `wasDealtNonCombatDamageThisTurn` | Was dealt (combat / non-combat) damage this turn. |
| `wasDealt…DamageThisTurnBy <filter>[ <OP><RHS>]` | …by sources matching the card filter; optional count of damage instances. |
| `wasDealt…DamageThisTurnBySource[Times][ <OP><RHS>]` | …by the source card (amount, or number of times). |
| `wasDealt…DamageLastTurn` | UNVERIFIED wording; reads the damage assigned in the previous turn. |
| `wasDealtCombatDamageThisCombatBy <DefinedCards>` | Was dealt combat damage this combat by one of them. |
| `wasDealtDamageThisGameBy <DefinedCards>` | Was dealt damage this game by one of them. |
| `damageDoneSingleSource <OP><RHS>` | The largest damage dealt this turn by a single source you control compares so. |
| `Attacking` / `Defending` | Is the attacking player / is being attacked this combat. |
| `attackedBySourceThisCombat`, `attackedBySourceThisTurn` | The source card attacked this player. |
| `wasAttackedThisTurnBy <filter>` | Was attacked this turn by a creature of yours matching the filter. |
| `attackedYouTheirCurrentTurn`, `attackedYouTheirLastTurn` | Attacked you during their current / last turn. |
| `attackedYouCtrlTheirCurrentTurn_<Type>` | Attacked a permanent of that type you control. |
| `BeenAttackedThisCombat` | Has been attacked this combat. |
| `NotedFor<key>`, `NotedDefender` | Draft-matters notes. |

### 4.5 Card properties

Source: `CardProperty.cardHasProperty`. "Source" = the host card of the filter; "you" = the
reference player; `(p)` marks a prefix match, `(=)` an exact match where it matters.

#### Identity and references

| Token | Meaning |
|---|---|
| `Self` (p) | Is the source card. |
| `Other` (p) | Is not the source card. |
| `StrictlySelf`, `StrictlyOther` | Same, but comparing the exact object incarnation (same game timestamp), so a card that left and came back does not count as "self". |
| `IsRemembered` | Is remembered by the source (§7.2). |
| `IsImprinted` | Is imprinted on the source. |
| `IsTriggerRemembered` | Is among the trigger-remembered objects of the ability context. |
| `ChosenCard` (p), `ChosenCardStrict` | Is one of the source's chosen cards (`Strict`: same incarnation). |
| `nonChosenCard` | Is not one of the source's chosen cards. |
| `Triggered<Key>` | Is (or is in) the triggering object of that name for the current ability: `TriggeredCard`, `TriggeredAttacker`, `TriggeredTarget`, `TriggeredCards`, `TriggeredNewCard` … |
| `NotDefined<Defined>` | Is **not** among `Defined$ <Defined>` (§5): `NotDefinedTargeted`, `NotDefinedTriggeredCard`. |
| `targetedBy` | Is a target of the current spell/ability (root). |
| `EffectSource` | The source is an effect object and this card is the card that created it. |
| `Cloned` | Was cloned from the source. |
| `CardUID_<n>` | Internal id match. |
| `named<Name>` (p) | Has that name. Spaces may be written directly or as `_`; a comma in the name is written `;`. |
| `NamedCard` | Its name is one of the names chosen for the source (`NameCard` effect). |
| `NamedByRememberedPlayer` | Its name is the card named by each player remembered by the source. |
| `noName` | Has no name (face-down). |
| `sameName` (p), `sharesNameWith` | Has the same name as the source. |
| `sharesNameWith <X>` | Same name as some card in X, where X is `YourGraveyard`, `Graveyard`, `Battlefield`, `ThisTurnCast`, `MovedToGrave` (cards moved by an `ExiledMoveToGrave` cost), `NonToken` (a nontoken permanent), `TriggeredCard`, or any Defined (§5). |
| `doesNotShareNameWith[ <X>]` | No shared name with the source / with X, where X is `Remembered…`, `Imprinted…`, `YourGraveyard`, `OtherYourBattlefield`, or a card **filter** over the battlefield. |
| `set<COD>` | First printing is from the set with that three-letter code. |
| `BorderColor<Colour>` | Border colour (`BorderColorSilver`). |
| `IsCommander` | Is a commander. |
| `IsRingbearer` | Is its controller's Ring-bearer. |
| `ChosenMode<Mode>`, `ChosenSector`, `DifferentSector` | Mode chosen for the card (`ChosenModeKhans`); sector mechanics. |
| `NotedFor<key>`, `NotedColor`, `NotedTypes`, `NotedName…`, `NotedGuessPhantasm` | Conspiracy draft-matters notes. |

#### Controller and owner

| Token | Meaning |
|---|---|
| `YouCtrl` (p) | You control it. |
| `YouDontCtrl` (p) | You do not control it. |
| `OppCtrl` (p) | An opponent of yours controls it. |
| `YourTeamCtrl` (p) | A player on your team controls it. |
| `YouOwn` (p), `YouDontOwn` (p), `OppOwn` (p) | Ownership, likewise. |
| `OwnerDoesntControl` | Its owner is not its controller. |
| `ControlledBy <P>` | Its controller matches `<P>`, tried first as a player filter (§4.4: `Player.Opponent`, `You`…) and, failing that, as Defined players (§5.2: `TriggeredPlayer`, `TargetedController`, `Remembered` …). |
| `OwnedBy <P>` | Its owner matches `<P>`, same two-step interpretation. |
| `EnteredUnder <P>` | It entered under the control of a player matching `<P>`. |
| `ChosenCtrl` | Controlled by the source's chosen player. |
| `TargetedPlayerCtrl`, `TargetedPlayerOwn` | Controlled / owned by a player targeted by the current ability. |
| `ActivePlayerCtrl` | Controlled by the player whose turn it is. |
| `RememberedPlayerCtrl`, `RememberedPlayerOwn` | Controlled / owned by a player remembered by the source. |
| `EnchantedPlayerCtrl`, `EnchantedPlayerOwn` | Controlled / owned by the player the source is attached to. |
| `EnchantedControllerCtrl`, `EnchantedControllerOwn` (written `EnchantedController…`) | Controlled / owned by the controller of the permanent the source is attached to. A suffix of `Ctrl` selects "controlled", anything else "owned". |
| `DefenderCtrl`, `DefenderCtrlForRemembered` | Controlled by the defending player of the combat the source (or the source's first remembered card) is attacking in. |
| `DefendingPlayerCtrl`, `DefendingPlayerOwn` | Controlled / owned by a player who is being attacked. |
| `OppProtect`, `ProtectedBy <DefinedPlayers>` | (Battles) protected by an opponent / by one of those players. |
| `ControllerControls<Type>` | Its controller controls a permanent of that type (`ControllerControlsIsland`). Variants: `…More<Type>` (more of them than you), `…AtLeastAsMany<Type>`, `…<Type>_<OP><RHS>`. |
| `sharesControllerWith[ <Defined>]`, `sharesOwnerWith[ <Defined>]` | Same controller / owner as the source or as the defined cards. |
| `ExiledByYou` | You exiled it. |

#### Types, colours, mana cost

Handled by the card-state fallback (§4.6) unless noted.

| Token | Meaning |
|---|---|
| `<Type>` | Has that card type, supertype or subtype (`Creature.Elf`, `Card.Legendary`). |
| `non<Type>` | Does not have it (`nonLand`, `nonCreature`, `nonBasic`, `nonHuman`). The rest is passed to the type test as written. |
| `Permanent` | Is a permanent card. |
| `Historic` | Legendary, artifact or Saga. |
| `Outlaw`, `Party` | Has an outlaw / party creature type. |
| `hasABasicLandType`, `hasANonBasicLandType` | |
| `ChosenType`, `ChosenType2`, `IsNotChosenType` | Has (not) the source's chosen type. If the chosen type itself starts with `Non`, `ChosenType` means "lacks that type". |
| `NotedType` | Has one of the source's noted types. |
| `White` `Blue` `Black` `Red` `Green` | Is that colour (among others). |
| `nonWhite` … `nonGreen` | Is not that colour. |
| `<Colour>Source`, `ColorlessSource` | Same test, used for damage sources: honours "is a colourless damage source" effects. |
| `Colorless`, `nonColorless` | |
| `MultiColor`, `MonoColor`, `AllColors` | Two or more colours / exactly one / all five. |
| `EnemyColor` | Exactly two colours forming an enemy pair. |
| `ChosenColor`, `ChosenColorSource` | Has the source's chosen colour. |
| `AnyChosenColor` | Has any of the source's chosen colours. |
| `AssociatedWithChosenColor` | Has the basic land type of the chosen colour. |
| `SharesColorWith[ <X>]` | Shares a colour with the source / with X: a Defined, or `MostProminentColor`, `LastCastThisTurn`, `ActivationColor`, `TriggeredProduced`. Colourless never shares. |
| `SharesColorWithOther <Defined>` | Shares a colour with another card of that set. |
| `MostProminentColor <colour>` | That colour is (one of) the most common among permanents. Independent of the tested card. |
| `numColors<OP><RHS>` | Number of colours. |
| `sharesCreatureTypeWith[ <X>]` | Shares a creature type with the source / with X (a Defined, or `Commander`). `sharesCreatureTypeWithAll <Defined>` requires sharing with every one. |
| `sharesCardTypeWith[ <X>]` | Shares a card type with the source / X (a Defined, `Imprinted`, `EachTopLibrary`). `sharesCardTypeWithOther <Defined>`, `sharesAllCardTypesWithOther <Defined>` exclude the card itself. |
| `sharesLandTypeWith <Defined>`, `sharesPermanentTypeWith` | |
| `MostProminentCreatureTypeInLibrary` | Has the most common creature type(s) in your library. |
| `numTypes<OP><RHS>`, `numCreatureTypes<OP><RHS>` | Number of card types / creature types. |
| `cmc<OP><RHS>` | Mana value comparison (`cmcLE3`, `cmcEQX`, `cmcEQChosen`). |
| `cmcEven`, `cmcOdd`, `cmcChosenEvenOdd`, `cmcNotChosenEvenOdd` | Parity of mana value (vs. the source's chosen parity). |
| `SharesCMCWith[ <Defined>]` | Same mana value as the source / a defined card. |
| `hasXCost` | Mana cost contains {X}. |
| `ManaCost<short>`, `ManaCostPartial<colour>` | Mana cost equals the given symbol string / contains that colour's symbol. |
| `CostsPhyrexianMana`, `CostsHybridMana` | |
| `greatestCMC_<Type>[ControlledBy<DefinedPlayers>]` | Has the greatest mana value among permanents of that type (`NonLandPermanent` allowed). |
| `lowestCMC` | No nonland permanent has a smaller mana value. |
| `greatestRememberedCMC`, `lowestRememberedCMC` | Extreme mana value among the source's remembered cards. |
| `CanPayManaCost` | Its mana cost could be paid with exactly the mana spent on the current ability. |

#### Power, toughness

| Token | Meaning |
|---|---|
| `power<OP><RHS>`, `toughness<OP><RHS>` | Current power / toughness. |
| `basePower<OP><RHS>`, `baseToughness<OP><RHS>` | Base P/T. |
| `totalPT_<OP><RHS>` | Power + toughness (note the underscore). |
| `baseLoyalty<OP><RHS>` | Printed loyalty. |
| `equalPT`, `powerGTtoughness`, `powerLTtoughness` | Power vs. toughness. |
| `powerGTbasePower`, `powerNOTbasePower` | Current vs. base power. |
| `powerEven`, `powerOdd` | |
| `greatestPower[ControlledBy<DefinedPlayers>]`, `leastPower[…]`, `leastToughness[…]` | Extreme among creatures on the battlefield (optionally only those controlled by the defined players; no space before the Defined: `greatestPowerControlledByYou`). Ties all match. |
| `yardGreatestPower` | Greatest power among creature cards in your graveyard. |

#### Counters

| Token | Meaning |
|---|---|
| `HasCounters` | Has any counter. |
| `counters_<OP><RHS>_<TYPE>` | Number of counters of that type (`counters_GE1_P1P1`, `counters_EQ0_TIME`). `<TYPE>` as in §3.3. |
| `countersReceivedThisTurn_<OP><RHS>_<TYPE>_<playerFilter>` | Number of counters of that type (`Any` = any type) put on it this turn by players matching the player filter: `countersReceivedThisTurn_GE1_P1P1_You`. |
| `canReceiveCounters <TYPE>` | Could have such a counter put on it. |

#### Keywords and abilities

| Token | Meaning |
|---|---|
| `with<Keyword>` | Has a keyword whose text **starts with** that string: `withFlying`, `withFirst Strike`, `withCycling` (also matches `Cycling:2`). |
| `without<Keyword>` | Has none starting with it (`withoutFlying`). |
| `hasKeyword<Keyword>` | Has exactly that keyword (`hasKeywordFlash`, which unlike `withFlash` does not match Flashback). |
| `NoAbilities` | Has no abilities. |
| `hasManaAbility`, `canProduceMana` | Has a mana ability. |
| `canProduceManaColor <colour>`, `canProduceSameManaTypeWith` | Can produce that colour / a type of mana the source can produce. |
| `hasNonManaActivatedAbility` | |
| `hasAbility <SAfilter>` | Has an ability matching the SA filter (§4.7): `hasAbility Activated.hasTapCost`. |
| `HasSVar <name>` | The card defines that SVar. |
| `activated` | One of its abilities was activated this turn. |

#### Status on the battlefield

| Token | Meaning |
|---|---|
| `tapped`, `untapped` | |
| `faceDown`, `faceUp`, `turnedFaceUpThisTurn`, `manifested`, `cloaked`, `canBeTurnedFaceUp` | |
| `phasedOut…` (prefix modifier, see §4.1), `phasedIn` | |
| `token` (p) | Is a token. `tokenCreated` (8 uses) additionally excludes tokens that were cast as copies of spells (it requires that the object was not cast). |
| `copiedSpell` | Is a copy of a spell. |
| `enchanted`, `equipped` | Has an Aura / Equipment attached. |
| `enchanting`, `equipping` | Is attached as an Aura / Equipment. |
| `modified` | Equipped, enchanted by an Aura its controller controls, or has counters. |
| `stickered`, `stickeredWith <kind>` | |
| `isDamaged` | Has damage marked. |
| `suspended` | Has suspend (in exile with time counters). |
| `IsMonstrous`, `IsRenowned`, `IsSolved`, `IsSaddled`, `IsSuspected`, `IsGoaded`, `IsPrepared`, `IsUnearthed`, `harnessed`, `FullyUnlocked`, `notTributed`, `HasDevoured` | Designations / one-shot states. |
| `Paired`, `PairedWith` | Soulbond: is paired / paired with the source. |
| `DoubleFaced`, `FrontSide`, `BackSide`, `CanTransform`, `Transformed`, `Flip`, `Split`, `AdventureCard`, `prepared`, `Worthy` | Layout / face state. |
| `firstTurnControlled` | Came under its controller's control this turn ("summoning sick" sense). |
| `startedTheTurnUntapped`, `cameUnderControlSinceLastUpkeep` | |

#### Attachment

| Token | Meaning |
|---|---|
| `EnchantedBy` (=) | The card is enchanted by the source, i.e. it is what the source Aura is attached to. The idiom for "enchanted creature": `Creature.EnchantedBy`. |
| `EnchantedBy <X>` | `Imprinted` / `Targeted`: enchanted by those cards. Otherwise X is a card filter: has an Aura attached that matches it (`EnchantedBy Aura.Other`). |
| `EquippedBy`, `AttachedBy` (same branch) | The source is attached to the card ("equipped creature": `Creature.EquippedBy`). With a Defined immediately after (no space): attached by one of those (`EquippedByTargeted`). |
| `FortifiedBy` | Likewise for Fortifications. |
| `Attached` (=) | The card is attached **to** the source ("Auras attached to this"). |
| `Enchanted…` (other suffixes) | The card is attached to the source (same test as `Attached`). |
| `AttachedTo <X>` | The card is attached to something matching X — tried as a filter, then as a Defined (players if X contains `Player`). |
| `CanEnchantSource`, `CanEnchantRemembered`, `CanEnchantEquippedBy` | The card (an Aura) could be attached to the source / each remembered card / the creature the source equips. |
| `CanBeEnchantedBy`, `CanBeEnchantedByTargeted`, `CanBeAttachedBy` | The source (or the targeted cards) could be attached to the card. |
| `NameNotEnchantingEnchantedPlayer` | No card with this name is attached to the player the source enchants. |
| `HauntedBy`, `EncodedWithSource` | Haunt / cipher links. |

#### Zones and zone history

| Token | Meaning |
|---|---|
| `inZone<Zone>` | The card's (last known) zone is that zone: `inZoneBattlefield`, `inZoneStack`. |
| `inRealZone<Zone>` | Its actual current zone. |
| `ThisTurnEntered` (=) | Entered its current zone this turn. |
| `ThisTurnEntered<Zone>`, `ThisTurnEnteredYour<Zone>` | …and is among the cards put into your `<Zone>` this turn (`Your`: and you own it). |
| `ThisTurnEnteredFrom_<Zone>` | Entered its current zone this turn **from** that zone (`ThisTurnEnteredFrom_Battlefield` in a graveyard = "died this turn"). |
| `EnteredSinceYourLastTurn` | |
| `DiscardedThisTurn`, `milledThisTurn`, `surveilledThisTurn` | Entered this turn by being discarded / milled / surveilled. |
| `DrawnThisTurn` | Was drawn this turn. |
| `TopLibrary` (=) | Is the top card of its owner's library. |
| `TopLibrary_<filter>`, `BottomLibrary[_<filter>]` | Is the first card from the top / bottom of its owner's library among those matching the filter. |
| `TopGraveyard`, `TopGraveyard<N>` | Is the top card (one of the top N cards) of its owner's graveyard. |
| `TopGraveyardCreature`, `BottomGraveyard` | Top creature card / bottom card of its owner's graveyard. |
| `Above`, `DirectlyAbove` | Is above / directly above the source in its owner's graveyard. |
| `ExiledWithSource` | Was exiled by the source (linked exile, "the exiled card"). |
| `ExiledWithSourceLKI` | Same, using last-known information of the exile link. |
| `ExiledWithEffectSource` | Exiled by the card that created the source effect object. |

#### Combat

All "current combat" tests are false when there is no combat.

| Token | Meaning |
|---|---|
| `attacking` (=) | Is an attacking creature. |
| `attackingLKI` | Was attacking according to last-known information. |
| `attackingAlone` | Is the only attacker. |
| `attackingYou` | Is attacking you. |
| `attackingYouOrYourPW` | Is attacking you or a planeswalker you control. |
| `attackingSame` | Is attacking the same player/permanent as the source. |
| `attackingBattle` | Is attacking a battle. |
| `attacking <DefinedEntity>` | Is attacking one of those players/permanents (`attacking Opponent`, `attacking TriggeredAttackedTarget`). |
| `blocking` | Is a blocking creature. |
| `blockingAlone`, `blockingSource`, `blockingCreatureYouCtrl`, `blocking<DefinedCards>` | …alone / blocking the source / a creature you control / one of the defined cards (`blockingRemembered`, `blockingTriggeredAttacker`, `blockingValid Creature`). |
| `blocked` (=) | Is an attacker that has been blocked. |
| `unblocked` | Is an unblocked attacker. |
| `blockedBySource`, `blockedBySourceLKI`, `blockedBySourceThisTurn` | The source is/was blocking it. |
| `isBlockedByRemembered`, `blockedByRemembered`, `blockedRemembered` | Blocked by / has blocked a remembered card. |
| `blockedValidThisTurn <X>`, `blockedByValidThisTurn <X>` | Blocked / was blocked by something matching X (filter or Defined) this turn. |
| `sharesBlockingAssignmentWith` | Blocks an attacker the source also blocks. |
| `attackersBandedWith` | Is in the source's attacking band. |
| `Defending` | Is a permanent being attacked. |
| `attackedBySourceThisCombat` | The source is attacking it. |
| `attackedThisCombat[ <OP><RHS>]` | Attacked this combat (optionally compare the count). |
| `blockedThisCombat`, `enlistedThisCombat` | |
| `hadToAttackThisCombat`, `couldAttackButNotAttacking` | Attack-requirement tests. |
| `attackedThisTurn`, `attackedLastTurn`, `attackedYouThisTurn`, `attackedBattleThisTurn` | |
| `blockedThisTurn`, `gotBlockedThisTurn`, `blockedByThisTurn` | |
| `attackedOrBlockedSinceYourLastUpkeep`, `blockedOrBeenBlockedSinceYourLastUpkeep` | |
| `notExertedThisTurn`, `FoughtThisTurn` | |
| `CrewedThisTurn`, `SaddledThisTurn` | Crewed / saddled the source this turn. |
| `CrewedBySourceThisTurn` | The source crewed it this turn. |
| `VisitedThisTurn` | Attraction visited this turn. |

#### Damage history

| Token | Meaning |
|---|---|
| `DamagedBy` (=) | Was dealt damage this turn by the source. |
| `DamagedBy <Defined>` | …by one of the defined cards (`DamagedBy Remembered`). |
| `DamagedBy<filter>[;<filter>…]` | …by a source matching the filter (no space; `;` separates alternatives). |
| `Damaged` (p) | Dealt damage to the source this turn. |
| `wasDealtDamageThisTurn`, `wasDealtNonCombatDamageThisTurn`, `wasDealtExcessDamageThisTurn` | |
| `wasDealtDamageByThisGame` | Was dealt damage by the source this game. |
| `dealtDamageThisTurn`, `dealtDamageToYouThisTurn`, `dealtDamageToOppThisTurn` | |
| `dealtCombatDamageThisTurn <validTarget>`, `dealtCombatDamageThisCombat` | |
| `dealtDamagetoAny`, `dealtCombatDamagetoAny` | Has ever dealt (combat) damage. |

#### How it was cast / put onto the stack

"Linked" means the test is false if the ability asking was not part of the object when it was
cast (CR 607.2i).

| Token | Meaning |
|---|---|
| `wasCast`, `wasCastByYou` | Was cast (by you), as opposed to put onto the battlefield. |
| `wasCastFrom[Your\|Their]<Zone>[ByYou]` | Was cast from that zone (`Your`: your zone; `Their`: its controller's): `wasCastFromYourHand`, `wasCastFromExile`, `wasCastFromYourGraveyardByYou`. |
| `ThisTurnCast` | Was cast this turn. |
| `SecondSpellCastThisTurn` | Is the second spell cast this turn. |
| `kicked`, `kicked 1`, `kicked 2` | Was kicked (with the first / second kicker). |
| `bargained`, `surged`, `blitzed`, `dashed`, `escaped`, `evoked`, `impended`, `prowled`, `spectacle`, `sneaked`, `foretold`, `warped`, `webSlinged`, `madness`, `PromisedGift`, `Teamwork` | Cast with that alternative/additional cost. |
| `linkedCastSA` | Was cast, and the asking ability is linked to that cast. |
| `castKeyword` | Was cast using the keyword ability that is asking. |
| `CastSaSource` | Was cast with the same cast-ability as the source. |
| `CastSa <SAfilter>` | The ability it was cast with matches the SA filter (`CastSa Spell.MayPlaySource`). |
| `delved`, `convoked`, `exploited` | Was exiled by delve for / tapped to convoke / exploited by the source. |
| `CanBeSacrificedBy` | Could be sacrificed to pay the current cost. |
| `CanBeTargetedBy<DefinedSA>` | The defined spell/ability could legally target it. |
| `canBeBeamedUp` | |

### 4.6 Card-state fallback

`CardStateProperty.hasProperty`, reached for any token `CardProperty` did not recognise, in
this order: a token **containing** `White`/`Blue`/`Black`/`Red`/`Green` is a colour test
(optionally prefixed `non`, optionally suffixed `Source`); a token containing `Colorless`;
`MultiColor…`, `EnemyColor…`, `AllColors…`, `MonoColor…`, `ChosenColor…`, `AnyChosenColor…`;
`AssociatedWithChosenColor`; `prepared`; `Worthy`; `Outlaw`; `Party`; `non<Type>`;
`CostsPhyrexianMana`; `CostsHybridMana`; `HasSVar <n>`; `ChosenType`; `IsNotChosenType`;
`ChosenType2`; `NotedType`; `hasAbility <f>`; `hasManaAbility`; `hasNonManaActivatedAbility`;
and finally **"has this type"**.

Trap: because the colour test is a substring test that comes first, a subtype whose name
contains a colour word cannot be tested as a *property* (it would be read as a colour and
fail); it can still be used as the type word.

### 4.7 Spell-ability properties

`SpellAbilityProperty.hasProperty`, for SA filters (`ValidSA$ Spell.…`, `Activated.…`).
Anything not listed is delegated to the **host card's** card properties (so
`Spell.Creature+YouCtrl` works).

| Token | Meaning |
|---|---|
| `ManaAbility` | Is a mana ability. |
| `YouCtrl`, `OppCtrl` | Controlled by you / an opponent (activating player). |
| `cmc<OP><RHS>` | Mana value of the spell (or of the ability's mana cost). |
| `XCost<OP><RHS>`, `withoutXCost` | Value paid for X / has no X in its cost. |
| `ManaSpent <OP><RHS>` | Total mana spent on it. |
| `ManaSpentBy <DefinedPlayers>` | Some mana was spent by one of them. |
| `ManaFrom<filter>[_<n>]` | (At least n) mana came from sources matching the card filter (`ManaFromTreasure`). |
| `paidPhyrexianMana` | Life was paid for a Phyrexian symbol. |
| `CountersRemovedToPay<OP><RHS>`, `SacrificedToPay` | About its paid cost. |
| `hasTapCost` | Its cost includes {T}. |
| `singleTarget`, `numTargets <OP><RHS>` | Number of targets. |
| `IsTargeting <Defined>` | Targets one of the defined objects (`~` stands for `+` inside). |
| `Modal`, `Kicked`, `Bargain`, `Buyback`, `Flashback`, `Foretold`, `Foretelling`, `Plotting`, `Warp`, `Mayhem`, `Sneak`, `Dash`, `Blitz`, `Disturb`, `Bestow`, `Mutate`, `Aftermath`, `Harmonize`, `Jumpstart`, `Teamwork` | Cast in that way. |
| `Cycling`, `Equip`, `Crew`, `Saddle`, `Station`, `Craft`, `Embalm`, `Eternalize`, `Unearth`, `Outlast`, `Monstrosity`, `Boast`, `Exhaust`, `PowerUp`, `Ninjutsu`, `MorphUp`, `ManifestUp`, `Unlock`, `ClassLevelUp`, `Loyalty`, `BeamMeUp`, `Backup`, `Modular`, `Ward`, `Daybound`, `Nightbound`, `CumulativeUpkeep` | Is that kind of (keyword) ability. |
| `isTurnFaceUp`, `isCastFaceDown` | |
| `ChapterNotLore`, `LastChapter` | Saga chapter abilities. |
| `SameKeyword`, `KeywordSource`, `MayPlaySource`, `EffectSourceAbility`, `otherAbility` | Relations to the asking ability / the source. |
| `NamedSpell`, `NamedAbility<name>` | Named-card / named-ability tests. |
| `CouldCastTiming`, `ManaAbilityCantPaidFor` | Timing helpers. |

---

## 5. The `Defined$` sub-language

Source: `AbilityUtils.getDefinedCards`, `getDefinedPlayers`, `getDefinedSpellAbilities`,
`getDefinedObjects` / `getDefinedEntities`, `addPlayer`, `adjustTriggerContext`,
`getPaidCards`; `SpellAbilityEffect.getCards` / `getPlayers`; `AbilityKey`.

A *Defined* names objects **without targeting**. It is the value of `Defined$`, and of every
other key documented as "Defined" (`ConditionDefined$`, `UnlessPayer$`, `RememberObjects$`,
`NewController$`, `Chooser$`, `TokenOwner$`, …), and appears inside filters (`ControlledBy
<Defined>`, `NotDefined<Defined>`, `sharesNameWith <Defined>`).

### 5.1 General shape

```
defined-list := defined ( " & " defined )*            -- union; only where the caller splits it
defined      := [ context ">" ] name [ "." filter-tail ]
context      := "Spawner" | "TriggeredSpellAbility" | "CastSA"
filter-tail  := property-list ( "," property-list )*   -- §4 properties, without a type word
```

- **Union**: `Defined$ Self & Targeted`, `Defined$ You & TriggeredTarget`,
  `Defined$ Valid Creature & Player`. The separator is exactly space-ampersand-space. It is
  split by the effect helpers (`SpellAbilityEffect.getCards/getPlayers/getEntities`), so it
  works for the main `Defined$`-style parameters of effects, but **not** inside a filter
  property or other places that call the resolver directly.
- **Filter tail**: after the first `.`, the remainder is a comma list of property lists; each
  is applied as `Card.<tail>` (cards) or `Player.<tail>` (players) to the objects found.
  `Defined$ Remembered.Creature` = "the remembered cards that are creatures". (Not available
  with the `Valid…` forms, whose `.` belongs to their own filter.)
- **Context switch** (`adjustTriggerContext`): `Spawner>X` evaluates X in the ability that
  created the current (delayed/immediate) trigger; `TriggeredSpellAbility>X` in the
  spell/ability that caused the trigger; `CastSA>X` in the spell that the host was cast as.
  If that context does not exist, X is evaluated in the current one.
- **Defaults**: an effect acting on cards uses `Self` when `Defined$` is absent; an effect
  acting on players uses `You`.
- One and the same string is given to all three resolvers (cards, players, spell abilities)
  when the caller wants "objects"; each returns what it understands. E.g. `Targeted` yields
  the targeted cards **and** the targeted players **and** the targeted spells.
- Unknown names: the card resolver returns nothing. The **player resolver returns all
  players** (in turn order) for any name it does not recognise — that is how `Player` works,
  and it means a misspelt player Defined silently becomes "each player".
- Text-changing effects are applied to the string before it is interpreted.

Timing note: a Defined is resolved when the effect resolves, against the current game state.
Forms ending in `LKI` / `LKICopy` deliberately return the remembered snapshot of an object
that may have changed zones; the plain forms return the object's *current* incarnation (which
for a card that died is the card in the graveyard).

### 5.2 Cards (`getDefinedCards`)

| Form | Meaning |
|---|---|
| `Self` | The host card. |
| `CorrectedSelf` | The host card's current incarnation in the game (after it has changed zones). |
| `OriginalHost` | The card that granted this ability (root ability's original host). |
| `EffectSource` | If the host is an effect object: the real card that created it (following nested effects). |
| `Enchanted` | The card the host (an Aura) is attached to. |
| `Equipped` | The creature the host Equipment is attached to. |
| `AttachedTo <Defined>` | All cards attached to the defined cards/players. |
| `AttachedBy <Defined>` | What each defined card is attached to (`AttachedBy Self` = what this is attached to; `AttachedBy Sacrificed`). |
| `Targeted`, `TargetedCard` | All cards targeted by **any link of the whole chain** — the root ability and every `SubAbility` after it, including links later than the current one (`SpellAbility.getAllTargetChoices`). Last-known information is used for targets that have moved. |
| `ThisTargetedCard` | Only this link's own card targets. |
| `ParentTarget` | The card targets of the nearest parent link that targets a card. |
| `TargetedSource` | The host cards of targeted spells/abilities. |
| `Triggered<Key>` | The triggering object of that name, as the card currently is (e.g. `TriggeredCard`, `TriggeredAttacker`, `TriggeredBlocker`, `TriggeredTarget`, `TriggeredSource`, `TriggeredNewCard`, `TriggeredCards`, `TriggeredAttackers`). `<Key>` is one of the `AbilityKey` names, matched case-insensitively; which keys exist depends on the trigger type. Collections yield all their cards. |
| `Triggered<Key>LKICopy` | The same object as a last-known-information snapshot from the event (e.g. the creature as it was when it died). 230 uses of `TriggeredCardLKICopy`. |
| `Triggered<Key>HostCard` | The host card of a triggering spell/ability (`TriggeredSpellAbilityHostCard`). |
| `Replaced<Key>` | The object of that name in the event being replaced (`ReplacedCard`, `ReplacedTarget`, `ReplacedSource`, `ReplacedNewCard`). Only inside a replacement's ability. |
| `Remembered`, `RememberedCard` | The cards remembered by the host (current incarnations; a melded card also brings its partner). If the host object has nothing remembered, the host's current incarnation is consulted too. |
| `RememberedLKI` | The remembered cards exactly as remembered (snapshots). |
| `DirectRemembered` | The remembered card objects without re-resolving them. |
| `RememberedFirst`, `RememberedLast` | First / last remembered card. |
| `DelayTriggerRemembered`, `DelayTriggerRememberedLKI` | The cards a delayed trigger remembered when it was created (`RememberObjects$` on `DelayedTrigger`). |
| `Imprinted`, `ImprintedLKI` | The cards imprinted on the host. |
| `ChosenCard` | The host's chosen cards (`ChooseCard`). |
| `ExiledWith…` | The cards exiled by/with the host. |
| `Convoked` | The creatures that convoked the host. |
| `TopOfLibrary`, `BottomOfLibrary` | Top / bottom card of the activator's library (nothing if empty). |
| `Top_<N>_OfLibrary` | The top N cards (N is a numeric expression). |
| `TopThirdOfLibrary`, `TopThirdRoundedDownOfLibrary` | The top third, rounded up / down. |
| `TopOfGraveyard` | Top card of the activator's graveyard. |
| `Valid <filter>` | All cards **on the battlefield** matching the §4 filter (comma alternatives allowed). |
| `Valid<Zone> <filter>` | All cards in that zone (every player's) matching: `ValidGraveyard Creature.YouOwn`, `ValidHand Card`, `ValidExile …`, `ValidLibrary …`, `ValidCommand Effect`, `ValidStack …` (cards on the stack). |
| `ValidAll <filter>` | All cards in the game matching. |
| `CardUID_<n>` | Internal id. |
| `Sacrificed`, `Discarded`, `Exiled`, `Tapped`, `Untapped`, `Returned`, `Revealed`, `Collected`, `Foraged`, `MovedToGrave`, `CardPutToLib`, `CounterPut`, `Exerted`, `Milled`, … | The cards used to pay the cost part of that name (§3.2) of the **root** ability. Also the `…Cards` variants (`SacrificedCards`), which hold the moved cards rather than their snapshots. |
| anything else | Nothing. |

Real examples:
`SVar:TrigReturn:DB$ ChangeZone | Defined$ DelayTriggerRememberedLKI | Origin$ Battlefield | Destination$ Hand`;
`DB$ ChangeZone | Origin$ Graveyard | Destination$ Hand | Defined$ ValidGraveyard Creature.YouOwn+ThisTurnEnteredFrom_Battlefield`.

### 5.3 Players (`getDefinedPlayers`)

| Form | Meaning |
|---|---|
| `You` | The activator (the ability's controller; for a static, the host's controller). |
| `Opponent` | Each opponent of the activator. |
| `Player` (and any unrecognised name) | Each player, in turn order. `Player.<props>` filters them (`Player.Other`, `Player.Opponent`). |
| `Registered` | Every player who started the game, including those who have left. |
| `ActivePlayer` | The player whose turn it is. |
| `Targeted`, `TargetedPlayer` | All players targeted by any link of the whole chain (as for cards). |
| `ThisTargetedPlayer` | Only this link's own player targets. |
| `ParentTarget` | Player targets of the nearest parent link that targets a player. |
| `TargetedController`, `TargetedOwner` | Controllers / owners of all targeted cards, plus the activators of (owners of the host of) targeted spells. |
| `ThisTargetedController`, `ThisTargetedOwner` | Same, for this link's own targets only. |
| `ParentTargetedController` | Controllers of the parent link's targets. |
| `TargetedOrController` | Targeted players plus controllers of targeted cards ("target creature or player… that player or that creature's controller"). |
| `TargetedAndYou` | The targeted players and the activator. |
| `Triggered<Key>` | A triggering object that is a player (or a collection of players): `TriggeredPlayer`, `TriggeredTarget`, `TriggeredActivator`, `TriggeredDefendingPlayer`, `TriggeredAttackingPlayer`, `TriggeredCauser`… |
| `Triggered<Key>Controller` | Controller of that triggering card (activator, if it is a spell/ability): `TriggeredCardController`, `TriggeredSourceController`. |
| `Triggered<Key>Owner` | Its owner. |
| `Triggered<Key>Opponent` | The opponents of its controller. |
| `Triggered<Key>OrController` | The object itself if it is a player, otherwise its controller. |
| `Triggered…AndYou` | Any of the above, plus the activator. |
| `Replaced<Key>`, `Replaced<Key>Controller`, `Replaced<Key>Owner` | Same idea for the replaced event (`ReplacedPlayer`, `ReplacedCardController`, `ReplacedSourceController`). |
| `Remembered` | The players remembered by the host, plus players remembered by cards the host remembers. |
| `RememberedController`, `RememberedOwner` | Controllers / owners of the remembered cards. |
| `RememberedOpponents` | Opponents of the remembered players. |
| `Imprinted…`, `ChosenCard…`, `EffectSource…`, `OriginalHost…`, `DelayTriggerRemembered…`, `Enchanted…`, `Equipped…` | Same suffix scheme (`Controller`, `Owner`, `Remembered`, `Opponents`) applied to those objects: `EnchantedController` (controller of the enchanted permanent), `EnchantedOwner`, `ImprintedController`, `ChosenCardController`, `OriginalHostController`, `EffectSourceController`. |
| `EnchantedPlayer` | The player the host is attached to. |
| `ChosenPlayer` | The host's chosen player. |
| `ChoosingPlayer` | The player who is making the current choice (root ability). |
| `Promised` | The player promised a gift. |
| `SourceController` | Controller of the ability's host. |
| `CardController`, `CardOwner` | Controller / owner of the card passed as context (normally the host). |
| `Caster` | The player who cast the host. |
| `Exiler` | The player who exiled the host. |
| `AttackingPlayer` | The attacking player (for a blocker: the controller of what it blocks). |
| `DefendingPlayer` | The defending player related to the host in the current combat. |
| `ManaSpender` | The players whose mana paid for this ability. |
| `NextPlayerToYourLeft` / `…Right`, `NextOpponentToYourLeft` / `…Right` | Seat-order neighbours. |
| `Flipped<Result>` | Players whose coin flip for the host had that result (`FlippedTails`). |
| `PlayerNamed_<name>`, `PlayerUID_<n>` | By name / id. |
| `Non<Defined>` | All players except those of the inner Defined (`NonTriggeredCardController`, `NonRememberedOwner`). |
| `OppNon<Defined>` | All opponents except those of the inner Defined. |
| `Self`, `TargetedCard`, `ThisTargetedCard`, `TargetedSource`, `Convoked`, `Valid…`, `CardUID_…`, a paid-list name | **No players** (these are card-only forms). |

Players returned for an effect are sorted in APNAP order starting with the active player
(or with `StartingWith$ <Defined>`).

### 5.4 Spells and abilities (`getDefinedSpellAbilities`)

| Form | Meaning |
|---|---|
| `Self` | This ability link. |
| `Parent` | The root of this chain. |
| `Targeted` | The spells/abilities targeted by any link of the whole chain (as they are on the stack). |
| `ThisTargeted` | Only this link's own targets. |
| `Triggered<Key>` | A triggering object that is a spell/ability: `TriggeredSpellAbility`, `TriggeredSourceSA`, `TriggeredStackSa`. |
| `Remembered` | Remembered spells/abilities, and all spell abilities of remembered cards. |
| `Imprinted` | All spell abilities of imprinted cards. |
| `EffectSource` | The ability that created the host effect object. |
| `SourceFirstSpell` | The spell on the stack whose host is this card. |
| `ValidStack <SAfilter>` | Every stack object matching the SA filter (§4.2): `ValidStack Spell`, `ValidStack Activated,Triggered`. |

### 5.5 Triggering-object names

The `<Key>` after `Triggered` / `Replaced` / in `TriggerCount$` / `ReplaceCount$` is one of
(`AbilityKey`): `Activator`, `Affected`, `Amount`, `Attacked`, `Attacker`, `Attackers`,
`AttackingPlayer`, `AttackedTarget`, `Blocker`, `Blockers`, `Card`, `Cards`, `CardLKI`,
`Cause`, `Causer`, `Championed`, `CounterAmount`, `CounterNum`, `CounterType`, `Crew`,
`DamageAmount`, `DamageSource`, `DamageTarget`, `Defender`, `Defenders`, `DefendingPlayer`,
`Destination`, `Devoured`, `Enlisted`, `Exploited`, `Explored`, `Explorer`, `Fighter`,
`Fighters`, `LifeAmount`, `LifeGained`, `NewCard`, `NewCounterAmount`, `Num`, `Number`,
`Object`, `Objects`, `Origin`, `OriginalController`, `OriginalDefender`, `OtherAttackers`,
`PayingMana`, `Phase`, `Player`, `PreventedAmount`, `Produced`, `Result`, `ScryNum`,
`Source`, `Sources`, `SourceSA`, `SpellAbility`, `SpellAbilityTargets`, `StackSa`,
`SurveilNum`, `Target`, `Targets`, `Token`, `TokenNum`, `Won`, and a few dozen rarer ones.
Which of them a given trigger or replacement provides is defined per trigger / replacement
type (see Forge's `Triggers.md`; e.g. `ChangesZone` provides `Card`, `CardLKI`, `NewCard`;
`DamageDone` provides `Source`, `Target`, `DamageAmount`; `Attacks` provides `Attacker`,
`Attacked`, `DefendingPlayer`).

---

## 6. Numeric expressions

Source: `AbilityUtils.calculateAmount`, `xCount`, `doXMath`, `handlePaid`, `playerXCount`,
`playerXProperty`, `objectXCount`, `getCardListForXCount`; `CardFactoryUtil.extractOperators`.

### 6.1 How a numeric parameter is evaluated

Given the text of a numeric parameter (`NumDmg$ X`, `NumCards$ 2`, `LifeAmount$ Count$xPaid`,
the RHS of a comparison, a cost amount…):

1. Blank → `0`.
2. A leading `+` or `-` is removed; `-` negates the final result. (`NumAtt$ +2`,
   `NumDef$ -X`.)
3. All digits → that integer.
4. If the text contains `$` it **is** the expression. Otherwise it is an **SVar name**: looked
   up on the ability, then on the card (§2.4). A missing SVar evaluates to `0` (with a console
   warning), it is not an error.
5. If the SVar's value is all digits → that integer.
6. Otherwise the expression is split at its first `$` into a **head** and a **body**. No `$`,
   or body `none` → `0`.
7. A context prefix on the head (`Spawner>`, `TriggeredSpellAbility>`, `CastSA>`; §5.1)
   switches the ability context.
8. The head selects an evaluator (§6.2). An unknown head, or an evaluator that finds nothing,
   yields `0`.

Negative results are possible; some callers clamp at 0.

Every evaluator finishes with at most **one** arithmetic suffix (§6.6), written after a `/`:
`Count$Valid Creature.YouCtrl/Times.2`.

### 6.2 Heads

`<body>` types: *count-expr* = §6.3; *list-expr* = §6.4; *player-expr* = §6.5.

| Head | Body | Value |
|---|---|---|
| `Count` | count-expr | The general evaluator, about the host card and the activator. 5,288 cards. |
| `Number` | integer | A literal: `Number$3`, `Number$0`. Useful to attach a suffix or to give a name to a constant. |
| `SVar` | SVar name | The value of another SVar, so a second suffix can be applied: `SVar:Y:SVar$X/Plus.1`. |
| `PlayerCount<Who>` | player-expr | Evaluate per player over a set of players and combine (§6.5). `<Who>`: `Players` or empty (all players), `Opponents`, `YourTeam`, `Other` (all but you), `RegisteredOpponents`, `HasLost`, `PropertyYou` (just you), `Property<playerProperty>` (players with that §4.4 property), `Defined<DefinedPlayers>`, `Remembered…` (remembered players; suffixes as §5.3). |
| `TriggeredPlayers<Key>…`, `TriggeredCardController` | player-expr | Over the players of `Defined$ Triggered<Key>…`. |
| `TriggeredPlayer`, `TriggeredTarget`, `TriggeredDefendingPlayer`, `TriggeredActivator` | player-expr | About that single triggering **player** (0 if the object is not a player). |
| `TargetedPlayer`, `ThisTargetedPlayer` | player-expr | Over the players targeted by this link or, failing that, the nearest parent that targets a player / by this link only. |
| `TargetedController` | player-expr | Over the controllers of the targets. |
| `TriggerCount` | `<Key>` | The integer triggering object of that name: `TriggerCount$DamageAmount`, `TriggerCount$LifeAmount`, `TriggerCount$Amount`. A collection of integers is summed; `TriggerCountMax` takes the maximum. |
| `ReplaceCount` | `<Key>` | The integer of that name in the replaced event: `ReplaceCount$DamageAmount`, `ReplaceCount$Amount`. |
| `Triggered<Key>`, `CardTriggered<Key>` | list-expr | About the single triggering **card** of that name: `TriggeredCard$CardPower`, `TriggeredAttacker$CardToughness`. Use the `CardTriggered…` spelling for keys that the player rule above would catch (`CardTriggeredTarget$CardPower`). |
| `TriggerObjects<Key>` | list-expr | About a triggering *collection* of cards: `TriggerObjectsAttackers$Amount`. |
| `Replaced<Key>` | list-expr | About a card in the replaced event: `ReplacedCardLKI$CardPower`. |
| `Targeted` | list-expr | About the targeted cards, found by `SpellAbility.findTargetedCards`: this link's own card targets; else the host cards of this link's targeted spells; else those of the nearest *parent* link that targets. (Note this is a different search from `Defined$ Targeted`, §5.2.) |
| `AllTargeted` | list-expr | About every card targeted by any link of the chain. |
| `ParentTargeted` | list-expr | About the parent link's targeted cards. |
| `TargetedObjects`, `TargetedObjectsDistinct` | `Amount` or `Valid …` | About all targets of the chain (cards, players, spells). |
| `TargetedByTarget` | list-expr | About the cards targeted by the targeted spell/ability. |
| `TriggeredSpellAbility`, `SpellTargeted` | count-expr | Evaluate a `Count`-style expression in the context of that spell/ability and its host. |
| `Remembered`, `RememberedLKI` | list-expr | About the remembered cards (current / as remembered). `Remembered$Amount` is "how many cards are remembered". |
| `Imprinted`, `ImprintedLKI` | list-expr | About the imprinted cards. |
| `ChosenCard` | list-expr | About the chosen cards. |
| `Enchanted`, `Equipped` | list-expr | About the card the host is attached to. |
| `ExiledWith` | list-expr | About the cards exiled with the host. |
| `Convoked`, `Crewed`, `Emerged` | list-expr | About the creatures that convoked / crewed this turn / were sacrificed to emerge the host. |
| `TriggerRemembered` | list-expr | About cards remembered by the trigger. |
| `DungeonsCompleted` | list-expr | About the activator's completed dungeons. |
| `OriginalHost` | count-expr | Evaluate with the ability's original host as the card. |
| `Sacrificed`, `Discarded`, `Exiled`, `Tapped`, `Revealed`, `Returned`, … (any paid-list name, §3.2) | list-expr | About the cards used to pay that cost part of the root ability. `Sacrificed$CardPower`, `Discarded$CardManaCost`, `Tapped$CardPower`. |

The heads are tested with `startsWith` in roughly the order above; in particular
`TargetedPlayer…`/`TargetedController…`/`TargetedObjects…`/`TargetedByTarget…` win over
`Targeted`, and `TriggeredPlayer…`/`TriggeredTarget…` win over `Triggered<Key>`.

### 6.3 `Count$` expressions

`xCount(card, body, ability)`. "The card" is the host (or, inside a list-expr, each card of
the list in turn); "you" is the activator. Syntax inside the body:

- `/` introduces the arithmetic suffix (so a body cannot otherwise contain `/`);
- `.` separates the head word from positional arguments (`Kicked.3.1`), **except** in forms
  that take a filter, where the text after the first space is the filter and keeps its dots;
- `$` after a card-set form introduces a list-expr applied to that set
  (`Count$Valid Creature.YouCtrl$GreatestCardPower`).

Conditional forms are written `<Cond>.<A>.<B>`: the value is A if the condition holds, else B.
A and B are numeric parameters in the sense of §6.1 (literal, SVar name).

**Control and literals**

| Form | Value |
|---|---|
| `Compare <L> <OP><R>.<A>.<B>` | A if `L OP R` else B. L, R, A, B are numbers or SVar names: `Count$Compare Y GE1.2.1`. |
| `IsPrime <L>.<A>.<B>` | A if L is prime. |
| `Random.<Min>.<Max>` | Uniform random integer in the range. |
| `SVar$<name>` (as body) | Value of that SVar. |

**X, costs and how the spell was cast** (need an ability context)

| Form | Value |
|---|---|
| `xPaid` | The announced X. In order: the X paid for the root ability; else 0 for a copied ability on another card; for a delayed/immediate trigger, the X of the ability that created it; for an enters-the-battlefield trigger (`ChangesZone` to Battlefield) or an `ETB$` replacement, the X the permanent spell was cast with (CR 107.3k); for `SpellCast` / `Cycled` / `TurnFaceUp` triggers, the X of that cast / cycling / turn-up ability; otherwise 0. Outside an ability: the card's own paid X. |
| `Kicked.<A>.<B>` | A if this spell was kicked (or the permanent was cast kicked). |
| `TimesKicked` | How many times it was kicked (multikicker). |
| `Bargain.<A>.<B>`, `Madness.<A>.<B>`, `Freerunning.<A>.<B>`, `Teamwork.<A>.<B>`, `OptionalGenericCostPaid.<A>.<B>`, `AltCost.<A>.<B>`, `Foretold.<A>.<B>`, `Escaped.<A>.<B>`, `Emerged.<A>.<B>`, `PromisedGift.<A>.<B>` | A if cast that way. |
| `wasCastFrom[Your]<Zone>[ByYou].<A>.<B>` | A if the card was cast from that zone. |
| `FromNamedAbility<name>.<A>.<B>` | A if the card was cast with the ability of that name. |
| `Presence_<Type>.<A>.<B>` | A if, as you cast it, you revealed or controlled a card of that type. |
| `IfCastInOwnMainPhase.<n>.<m>`, `InOwnMainPhase.<n>.<m>` | n if it is your main phase (and, for `IfCast…`, the card was cast); literal integers only. |
| `Converge` | Number of colours of mana spent to cast the card. |
| `ManaColorsPaid` | Number of colours of mana spent on this ability. |
| `Adamant[_N].<Colour>.<A>.<B>` | A if at least N (default 3) mana of that colour was spent. |
| `CastTotalManaSpent`, `CastTotalManaSpent <filter>` | Mana spent to cast the card (from sources matching). |
| `TotalManaSpent <filter>` | Mana spent on this ability from matching sources. |
| `EachSpentToCast.<manaSymbol>` | Mana of exactly that type spent to cast it. |
| `EachPhyrexianPaidWithLife` | Phyrexian symbols paid with life. |
| `xColorPaid <colours>` | Amount of X paid with those colours. |
| `AmountManaProduced` | Mana produced by this ability. |
| `NumTimesChoseMode` | How many modes were chosen. |
| `OptionalKeywordAmount`, `hasOptionalKeywordAmount` | Keyword-with-amount helpers. |
| `ActivatedThisGame`, `ResolvedThisTurn` | Times this ability was activated this game / resolved this turn. |
| `TriggerRememberAmount` | Sum of the integers remembered by the trigger. |
| `TriggeredManaCostGeneric`, `TriggeredManaCostDevotion.<Colour>`, `TriggeredPayingMana.<c1>.<c2>…` | About the triggering card's mana cost / the mana paid. |
| `HasNumChosenColors.<DefinedCards>` | Sum over those cards of how many of the chosen colours each has. |

**Counting cards**

| Form | Value |
|---|---|
| `Valid <filter>` | Number of permanents on the battlefield matching the §4 filter. The single most common form (2,047 uses): `Count$Valid Creature.YouCtrl`. |
| `Valid<Zones> <filter>` | Same over the named zone(s) of all players; zones comma-separated: `Count$ValidGraveyard Creature.YouOwn`, `Count$ValidGraveyard,Exile Instant.YouOwn,Sorcery.YouOwn`, `Count$ValidHand Card.YouOwn`, `ValidLibrary`, `ValidExile`, `ValidStack`, `ValidCommand`, `ValidSideboard`. |
| `ValidAll <filter>` | Over all cards in the game. |
| `ValidSelf <filter>` | 1 if the card itself matches, else 0. |
| `… $<list-expr>` | Any of the four forms above followed by `$` and a list-expr (§6.4): `Count$Valid Creature.YouCtrl$GreatestCardPower`, `Count$ValidGraveyard Card.YouOwn$CardTypes`. |
| `LastStateBattlefield <filter>`, `LastStateGraveyard <filter>` | Count in the snapshot of that zone taken when the ability was put on the stack / the event happened. `…WithFallback` uses the snapshot of the spell the host was cast as, or the live zone. |
| `RememberedSize`, `ImprintedSize`, `ChosenSize` | Number of remembered objects / imprinted cards / chosen cards. |
| `RememberedNumber` | Sum of the integers remembered by the host. |
| `ThisTurnCast_<filter>`, `LastTurnCast_<filter>`, `CastSinceBeginningOfYourLastTurn_<filter>` | Spells cast this turn / last turn / since then matching the filter (`Count$ThisTurnCast_Card.YouCtrl`). Accept `$<list-expr>`. |
| `ThisTurnActivated_<SAfilter>` | Abilities activated this turn. |
| `ThisTurnEntered_<Zone>_<filter>`, `ThisTurnEntered_<Zone>_from_<Origin>_<filter>` | Cards put into that zone this turn (from that origin). `Count$ThisTurnEntered_Graveyard_from_Battlefield_Creature` = creatures that died. `LastTurnEntered_…` likewise. Accept `$<list-expr>`. |
| `LeftBattlefieldThisTurn <filter>`, `LeftGraveyardThisTurn <filter>` | |
| `CreaturesAttackedThisTurn <filter>` | Your creatures that attacked this turn and match. |
| `AttackersDeclared` | Number of creatures you attacked with this turn. |
| `Devoured <filter>` | Creatures the card devoured that match. |
| `AttachedTo <filter>` | Cards attached to the card that match (corpus: `TriggeredCard$AttachedTo Aura.YouCtrl`, i.e. used as a per-card form). |
| `TypesSharedWith <DefinedCards>` | Number of card types the card shares with them. |
| `MostCardName[<Zones>] <filter>` | Size of the largest group of same-named cards. |
| `MostProminentCreatureType <filter>` | Size of the largest creature-type group. |
| `SecondMostProminentColor <filter>` | Count of the second most common colour. |
| `DifferentCounterKinds_<filter>` | Number of different kinds of counters among matching permanents. |
| `ExactManaCost[<Zones>] <filter>` | Number of distinct mana costs. |
| `CardsInYourHand` (any head containing `InYourHand`) | Cards in your hand. Legacy substring scheme: a head containing `YouCtrl`, `OppCtrl`, `OnBattlefield`, `InYourYard`, `InYourLibrary`, `InYourHand`, `InYourSideboard`, `InAllHands`, `SpellsOnStack` selects those cards (unioned), then `Type….<Type>`, `Named….<Name>`, `Multicolor`, `Monocolor` filter them (so `Count$TypeInYourYard.Creature` and `Count$TypeYouCtrl.Forest` are valid old-style spellings). Only `CardsInYourHand` and `NumInAllHands` remain in the corpus. **Any unrecognised `Count$` body ends up here and evaluates to 0.** |

**About the card**

| Form | Value |
|---|---|
| `CardPower`, `CardToughness`, `CardSumPT` | Current power / toughness / their sum. |
| `CardBasePower`, `CardBaseToughness` | Base values. |
| `CardManaCost`, `CardManaCostLKI` | Mana value (`LKI`: including X as paid, when not on the stack). |
| `CardCounters.<TYPE>`, `CardCounters.ALL` | Counters of that type / of all types on it. |
| `CardNumColors` | Number of colours. |
| `CardMulticolor.<n>.<m>` | n if multicoloured else m (literal integers). |
| `CardNumAttacksThisTurn`, `CardNumAttacksThisGame` | |
| `CardNumNotedTypes`, `CardStickers[.…]`, `Intensity`, `CrewSize`, `TimesMutated`, `RegeneratedThisTurn`, `FinalChapterNr`, `MaxSameStoredRolls` | Mechanic-specific values. |
| `TotalValue <Keyword>` | Sum of the magnitudes of that keyword on the card (e.g. total Bushido). |
| `ChosenNumber` | The number chosen for the card (`ChooseNumber`). |
| `TotalDamageDoneByThisTurn` | Damage dealt by it this turn. |
| `TotalDamageReceivedThisTurn`, `ExcessDamageReceivedThisTurn` | Damage / excess damage dealt to it this turn. |
| `TotalDamageThisTurn <sourceFilter>` | Damage dealt to it this turn by matching sources. |
| `Chroma[.<Colour>]`, `ChromaSource[.<Colour>]`, `ChromaInGrave[.<Colour>]` | Number of mana symbols of that colour (all colours if omitted) in the mana costs of your permanents / this card / your graveyard. |
| `EnchantedControllerCreatures` | Creatures controlled by the enchanted permanent's controller. |

**About you (the activator)**

| Form | Value |
|---|---|
| `YourLifeTotal`, `YourStartingLife`, `OppGreatestLifeTotal` | |
| `LifeYouGainedThisTurn`, `LifeYouLostThisTurn`, `LifeOppsLostThisTurn`, `LifeYourTeamGainedThisTurn`, `LifeYouGainedTimesThisTurn` | |
| `YouDrewThisTurn`, `YouDrewLastTurn`, `YouCastThisGame`, `YouScryThisTurn`, `YouSurveilThisTurn`, `YouDescendedThisTurn`, `YouFlipThisTurn`, `YouRollThisTurn`, `YouRolledThisTurn<N>`, `YouFaceDilemmaThisTurn` | Per-turn / per-game tallies. |
| `YourCounters<Type>` | Counters of that type you have: `YourCountersEnergy`, `YourCountersExperience`, `YourCountersPoison`. |
| `TotalOppPoisonCounters` | |
| `Devotion.<colour>`, `DevotionDual.<c1>.<c2>` | Your devotion (`Chosen` = the chosen colour). |
| `Domain` | Number of basic land types among lands you control. |
| `Party` | Size of your party (0–4). |
| `AbilityYouCtrl<kw1,kw2,…>` | How many of the listed keywords appear among creatures you control. |
| `UniqueManaColorsProduced.ByUntappedSources` (or any other argument) | Colours your permanents could produce. |
| `ManaPool:<colour>`, `ManaPool:All` | Mana in your pool. |
| `TopOfLibraryCMC` | Mana value of your top card. |
| `YourTurns`, `YourExtraTurns`, `YourSpeed`, `YourStartingLibrarySize`, `ColorsColorIdentity`, `BloodthirstAmount`, `UnlockedDoors`, `DistinctUnlockedDoors`, `NotedNumber`, `DraftNotesHighest.<k>`, `DraftNotesCount.<k>` | |
| `CommanderCastFromCommandZone`, `TotalCommanderCastFromCommandZone` | Times this commander / your commanders were cast from the command zone. |
| `SearchedLibrary.<DefinedPlayers>` | Times those players searched a library. |
| `MaxOppDamageThisTurn`, `MaxCombatDamageThisTurn` | |
| Conditionals `.<A>.<B>`: `Hellbent`, `Metalcraft`, `Delirium`, `Threshold`, `FatefulHour`, `Revolt`, `Landfall`, `Morbid`, `Monarch`, `Initiative`, `Blessing`, `StartingPlayer`, `CommittedCrimeThisTurn`, `ExtraTurn`, `MaxSpeed`, `AllFourBend`, `UrzaLands`, `Night`, `Void` | A if the named condition holds. |

**About the game**

| Form | Value |
|---|---|
| `StormCount` | Spells cast this turn, minus one. |
| `TotalTurns` | Turn number. |
| `[Num\|Max][Non][Combat]DamageThisTurn <sourceFilter> <targetFilter>` | Damage dealt this turn by matching sources to matching recipients: total; `Num…` = number of damage events; `Max…` = largest. `Count$NumDamageThisTurn Card.YouCtrl,Emblem.YouCtrl Player,Permanent`. |
| `SingleMaxDamageThisTurn` | Largest single damage event this turn. |
| `CountersAddedThisTurn <TYPE> <playerFilter> <cardFilter>` | Counters of that type put this turn by matching players on matching cards. |
| `CountersRemovedThisTurn <TYPE> <filter>` | |
| `FinishedUpkeepsThisTurn`, `FinishedEndOfTurnsThisTurn`, `MaxDistinctOnStack`, `PlanarDiceSpecialActionThisTurn`, `NumPiledGuessedSA` | |

### 6.4 List expressions

`handlePaid(cards, expr)`, used after a card-set head (`Remembered$…`, `Targeted$…`,
`Sacrificed$…`, `TriggeredCard$…`, `Count$Valid …$…`). An empty set gives 0 (then the suffix).

| Expr | Value |
|---|---|
| `Amount` | Number of cards in the set. |
| `Valid <filter>` | Number of cards in the set matching the filter. May be followed by `$<list-expr>` to apply another list-expr to the filtered set, or by `/suffix`. |
| `Greatest<count-expr>` | Maximum over the set of a per-card `Count` form: `GreatestCardPower`, `GreatestCardManaCost`, `GreatestCardToughness`. |
| `Least<count-expr>` | Minimum: `LeastCardPower`. |
| `Different<count-expr>` | Number of distinct values: `DifferentCardManaCost`, `DifferentCardPower`. |
| `DifferentCardNames` | Number of distinct names. |
| `Colors` | Number of colours among the cards. |
| `DifferentColorPair` | Number of distinct two-colour pairs. |
| `CardTypes`, `CardTypesPermanent` | Number of card types (permanent types) among them. |
| `AllTypes` | Card types + supertypes + subtypes among them. |
| `CreatureType`, `PlaneswalkerType`, `LandType` | Number of distinct such subtypes among them. |
| `SharesCardTypes` | Size of the largest group sharing a card type. |
| `TapPowerValue` | Total power. |
| `StickerPower`, `StickerToughness` | |
| any other count-expr | **Sum** over the set of that `Count` form evaluated on each card: `CardPower`, `CardManaCost`, `CardToughness`, `CardCounters.P1P1`, `CardNumColors`… So `Targeted$CardPower` is the target's power (or the total, with several targets). |

### 6.5 Player expressions

`playerXCount(players, expr)` then `playerXProperty(player, expr)`.

Aggregators over the player set:

| Expr | Value |
|---|---|
| `Amount` | Number of players in the set. |
| `Highest<expr>`, `Lowest<expr>` | Max / min of the per-player expr: `PlayerCountOpponents$HighestLifeTotal`. |
| `TiedForHighestLife`, `TiedForLowestLife` | Number of players tied. |
| `HasProperty<playerProperty>` | Number of players having that §4.4 property. |
| `Condition[<OP><n>] <expr>` | Number of players whose per-player expr compares true (default `GE1`). |
| `DamageThisTurn` | Total damage dealt to them this turn. |
| `TotalCommanderCastFromCommandZone` | Sum. |
| anything else | **Sum** of the per-player expr below. (The suffix is applied per player before summing.) |

Per-player expressions:

| Expr | Value |
|---|---|
| `Valid <filter>` | Battlefield cards matching, with *that player* as "You" in the filter. |
| `Valid<Zones> <filter>` | Same over the zones. So `YouCtrl` / `YouOwn` inside it refer to the player being counted: `PlayerCountOpponents$HighestValid Creature.YouCtrl` is "the greatest number of creatures controlled by an opponent". |
| `CardsInHand`, `CardsInLibrary`, `CardsInGraveyard`, `CardsInPlay`, `CreaturesInPlay`, `LandsInGraveyard` | Sizes. |
| `LifeTotal`, `StartingLife`, `StartingLibrarySize` | |
| `LifeLostThisTurn`, `LifeLostLastTurn`, `LifeGainedThisTurn`, `LifeGainedByTeamThisTurn`, `LifeStartedThisTurnWith` | |
| `Counters.<TYPE>`, `Counters.ALL` | Counters on the player. |
| `SpellsCastThisTurn`, `CardsDrawn`, `CardsDiscardedThisTurn`, `LandsPlayed`, `LandsPlayed <filter>`, `ExploredThisTurn`, `AttackersDeclared` | This-turn tallies. |
| `SacrificedThisTurn[ <filter>]`, `SacrificedPermanentTypesThisTurn` | |
| `ThisTurnEntered_<Zone>[_from_<Origin>]_<filter>` | As §6.3, for that player. |
| `DamageToOppsThisTurn`, `NonCombatDamageDealtThisTurn` | |
| `OpponentsAttackedThisTurn`, `OpponentsAttackedThisCombat`, `BeenDealtCombatDamageSinceLastTurn` | |
| `DomainPlayer`, `TopOfLibraryCMC`, `Speed`, `RingTemptedYou`, `NumPowerSurgeLands`, `AttractionsVisitedThisTurn`, `PlaneswalkedToThisTurn <name>`, `SVarAmount` | |
| anything else | 0. |

### 6.6 Arithmetic suffixes

`doXMath`. Written `/<Op>` or `/<Op>.<N>`; `<N>` is an integer or, failing that, any numeric
parameter (SVar name or expression). **Only the first `/…` is used**; to chain, go through
another SVar with `SVar$`.

| Suffix | Result (v = value so far) |
|---|---|
| `/Plus.N` | v + N |
| `/Minus.N` | v − N |
| `/NMinus.N` | N − v |
| `/Times.N` | v × N |
| `/Twice`, `/Thrice` | 2v, 3v |
| `/Negative` | −v |
| `/Abs` | \|v\| |
| `/HalfUp`, `/HalfDown` | v/2 rounded up / down |
| `/ThirdUp`, `/ThirdDown` | v/3 rounded up / down |
| `/DivideEvenlyUp.N`, `/DivideEvenlyDown.N` | v/N rounded up / truncated (0 if N = 0) |
| `/Mod.N` | v mod N (0 if N = 0) |
| `/Pow.N` | v to the power N |
| `/LimitMax.N` | min(v, N) |
| `/LimitMin.N` | max(v, N) |
| anything else | v |

The operator is recognised by substring search in the order `Plus`, `NMinus`, `Minus`,
`Twice`, `Thrice`, `HalfUp`, `HalfDown`, `ThirdUp`, `ThirdDown`, `Negative`, `Times`, `Pow`,
`DivideEvenlyUp`, `DivideEvenlyDown`, `Mod`, `Abs`, `LimitMax`, `LimitMin`.

Real examples:

```
SVar:X:Count$xPaid
SVar:X:Count$Valid Creature.YouCtrl
SVar:X:Count$ValidGraveyard,Exile Instant.YouOwn,Sorcery.YouOwn
SVar:Y:Count$RememberedSize/Plus.1                      (v/valakut_awakening…)
SVar:X:ReplaceCount$DamageAmount/Plus.1                 (a/akki_lavarunner…)
SVar:X:ReplaceCount$Amount/LimitMax.Difference
SVar:X:TriggeredCard$CardPower
SVar:X:PlayerCount$HasPropertyHasCardsInHand_Card_LE1
SVar:X:Count$CardsInYourHand/Times.2
```

---

## 7. Durations, remembering, Cleanup, descriptions

### 7.1 `Duration$`

Source: `SpellAbilityEffect.addUntilCommand`, `checkValidDuration`; `PumpEffect`,
`AnimateEffectBase`, `EffectEffect` (handling of `Permanent` / `Perpetual`); `GoadEffect`.

`Duration$` says when a continuous effect created by a resolving ability ends. It is read by
the effects that create such effects (`Pump`, `PumpAll`, `Animate`, `AnimateAll`, `Effect`,
`Debuff`, `Protection`, `GainControl`-style effects, `ChangeZone` "exile until…", `Clone`, …).
"You" below is the ability's activator; "host" is the ability's host card.

| Value | Corpus | The effect lasts… |
|---|---|---|
| *(absent)*, or any value not listed below — the corpus writes `EndOfTurn`, `UntilEndOfTurn`, `EOT`, `End` | 100+ explicit | …until end of turn. This is the helper's fall-through branch, and it is the **default for these effects** (contrary to CR 611.2a's "until end of game" default — Forge chose the common case). |
| `Permanent` | 910 | …indefinitely (the effect is never scheduled for removal). |
| `Perpetual` | 268 | …"perpetually" (digital-only Alchemy wording): it stays with the card across zone changes. |
| `UntilEndOfCombat` | 52 | …until end of combat. |
| `UntilEndOfCombatYourNextTurn` | 1 | …until the end of combat on your next turn. |
| `UntilYourNextTurn` | 157 | …until your next turn begins. |
| `UntilTheEndOfYourNextTurn` | 111 | …until the end of your next turn (the current turn, if it is yours, does not count). |
| `UntilTheEndOfTargetedNextTurn` | 2 | …until the end of the first targeted player's next turn. |
| `ThisTurnAndNextTurn` | 1 | …until the end of the next turn. |
| `UntilYourNextUpkeep` | 9 | …until your next upkeep. |
| `UntilTheEndOfYourNextUpkeep` | 1 | …until the end of your next upkeep step. |
| `UntilTheEndOfYourNextUntap` | 22 | …until the end of your next untap step ("doesn't untap during its controller's next untap step"-style effects). |
| `UntilNextEndStep` | 1 | …until the beginning of the next end step. |
| `UntilYourNextEndStep` | 15 | …until your next end step. |
| `UntilStateBasedActionChecked` | 1 | …until state-based actions are next checked. |
| `UntilHostLeavesPlay` | 130 | …until the host leaves the battlefield ("for as long as this remains on the battlefield"; the O-Ring pattern with `ChangeZone`). |
| `UntilHostLeavesPlayOrEOT` | 73 | …until the host leaves the battlefield or end of turn, whichever is first. |
| `UntilHostLeavesPlayOrEndOfCombat` | 1 | …likewise with end of combat. |
| `UntilLoseControlOfHost` | — | …until the host leaves the battlefield or changes controller. |
| `AsLongAsControl` | 28 | …for as long as you control the host (ends when it leaves, changes controller, or phases out). |
| `AsLongAsInPlay` | 16 | …for as long as the host is on the battlefield (ends when it leaves or phases out). |
| `UntilUntaps` | 30 | …for as long as the host remains tapped (ends when it untaps, leaves or phases out). |
| `UntilTargetedUntaps` | 2 | …for as long as the targeted permanent remains tapped. |
| `UntilUnattached` | 5 | …for as long as the host stays attached. |
| `UntilFacedown` | 1 | …until the host is turned face down. |
| `UntilAnimatedFaceup` | 1 | (`Animate` only) …until the animated permanent is turned face up. |

Pre-check (`checkValidDuration`): if, when the ability resolves, the duration's premise is
already false, **the effect does nothing at all**. That covers: host not on the battlefield
(nor on the stack) for `UntilHostLeavesPlay…`, `UntilLoseControlOfHost`, `UntilUntaps`,
`AsLongAsControl`, `AsLongAsInPlay`; host phased out for the two `AsLongAs…`; the activator no
longer controlling the host for `UntilLoseControlOfHost` / `AsLongAsControl`; host untapped for
`UntilUntaps`; target untapped or phased out for `UntilTargetedUntaps`.

Other APIs have their own defaults — e.g. `Goad` defaults to `UntilYourNextTurn`. Check the
effect before assuming end of turn.

### 7.2 Remembered, imprinted and chosen objects

Source: `Card` (`rememberedObjects`, `imprintedCards`, `chosen…`),
`AbilityUtils.handleRemembering`, `MagicStack` (call site), `ChangeZoneEffect`,
`EffectEffect`, `SpellAbilityEffect.addForgetOnMovedTrigger`.

Forge has no local variables. To pass an object from one link of a chain to a later link (or
to a delayed trigger, or to a later activation), scripts store it **on the host card**:

- **Remembered**: an insertion-ordered set of arbitrary objects — cards (current objects or
  last-known snapshots), players, spells/abilities, integers, strings.
- **Imprinted**: a second, independent list, of cards only. Used both for the real "imprint"
  mechanic and simply as a second register.
- **Chosen…**: separate slots filled by the `Choose*` / `NameCard` effects — chosen cards,
  chosen player, chosen type (and a second type), chosen colours, chosen number, named cards,
  even/odd, direction, mode.

These live on the card object, not on the ability. They **persist after the ability has
finished**, are shared by all abilities of that card, and can survive the card changing zones.
That is why almost every chain that remembers something ends with
`DB$ Cleanup | ClearRemembered$ True` (2,764 uses): it is the scope exit. A translator should
treat *remember … use … Cleanup* as a let-binding and can usually drop the Cleanup.

Readers:

| Mechanism | Reads |
|---|---|
| `Defined$ Remembered` / `RememberedLKI` / `RememberedController` / … (§5) | the remembered cards / players |
| filter property `IsRemembered`, `IsImprinted`, `ChosenCard`, `ChosenType`, `ChosenColor`, `NamedCard`, `RememberedPlayerCtrl` … (§4) | membership |
| `Remembered$<list-expr>`, `Count$RememberedSize`, `Count$RememberedNumber`, `Count$ChosenNumber` (§6) | numbers |
| `ConditionDefined$ Remembered \| ConditionPresent$ Card \| ConditionCompare$ GE1` | "if something was remembered" — the standard encoding of "if you do" |

Writers — generic (`handleRemembering`; processed **once, for the root ability of a spell or
ability, just before it resolves**, not for later links):

| Key | Meaning |
|---|---|
| `RememberTargets` | present → remember this ability's targets. |
| `ForgetOtherTargets` | present (with the above) → clear the remembered set first. |
| `IncludeAllComponentCards` | also remember the components of merged/melded targets. |
| `RememberCostMana` | clear, then remember (as a string) the mana spent on the activation cost. |

Writers — effect-specific. Each effect has its own flags named `Remember<What>$ True`; they
are implemented one by one in the effect classes. Confirmed for `ChangeZone` /
`ChangeZoneAll`: `RememberChanged` (remember each card that actually moved; 1,278 uses),
`RememberLKI` (remember a last-known snapshot of each card before it moves), `ForgetChanged`
(remove the moved cards from the remembered set), `ForgetOtherRemembered` (clear the set
before starting), `Imprint` (imprint the moved cards), `Unimprint` (clear the imprinted list
first). The others follow the same naming scheme and are **UNVERIFIED individually** here; by
corpus frequency: `RememberRevealed`, `RememberSacrificed`, `RememberDiscarded`,
`RememberTokens`, `RememberMilled`, `RememberFound`, `RememberChosen`, `RememberMade`,
`RememberDestroyed`, `RememberDamaged`, `RememberPeeked`, `RememberDrawn`, `RememberRemoved`,
`RememberAmount`, `RememberPlayed`, `RememberDrafted`, `RememberTapped`, `RememberCountered`,
`RememberControlled`, `RememberManifested`, `RememberPut`, `RememberPumped`, …

`RememberObjects$ <Defined & Defined…>` is different: it does not write to the host. It
initialises the memory of a **new object** the ability creates:

- on `DB$ Effect`: the created effect object remembers those cards/players. Inside the
  effect's own statics, triggers and replacements, `IsRemembered` / `Remembered` then refer to
  *the effect's* memory, and `EffectSource` refers to the card that created it. Related keys on
  `Effect`: `ImprintCards$ <Defined>`; `ForgetOnMoved$ <Zone>` (the effect forgets a remembered
  card when it leaves that zone — implemented as hidden triggers on the effect — and, unless
  `ForgetOnCast$ False`, when it is cast); `ExileOnMoved$ <Zone>` (the effect is removed when a
  remembered card leaves that zone); `ForgetCounter`, `ForgetOnPhasedIn`.
- on `DB$ DelayedTrigger` / `ImmediateTrigger`: the created trigger remembers them; its
  `Execute$` ability reads them back with `Defined$ DelayTriggerRemembered[LKI]` or
  `TriggerRemembered$…`. `RememberSVarAmount$ <SVar>` stores a number instead, read back with
  `Count$TriggerRememberAmount`.
- also accepted by `Pump`, `Animate`, `Regenerate`.

### 7.3 The `Cleanup` API

Source: `CleanUpEffect`. `DB$ Cleanup` has no game-visible effect; it resets host-card
registers. Keys (all presence-tested): `ClearRemembered`, `ClearImprinted`, `ClearChosenCard`,
`ClearChosenPlayer`, `ClearChosenType` (both type slots), `ClearChosenColor`,
`ClearNamedCard`, `ClearCoinFlips`, `ClearTriggered` (removes the host's pending delayed
triggers — this one *does* change behaviour), `ForgetDefined$ <Defined>` (remove just those
objects from the remembered set), `Log` (write the chosen things to the game log), and
`Defined$ <card>` to clean a card other than the host.

### 7.4 Descriptions are not rules

Source: `CardTraitBase.descriptiveKeys`, `AbilityFactory` (description set-up),
`SpellAbilityEffect.getStackDescription`.

`SpellDescription$` (abilities), `StackDescription$` (text shown while on the stack),
`TriggerDescription$` (triggers) and `Description$` (statics, replacements) are display text.
The engine never derives behaviour from them; a script whose description disagrees with its
parameters behaves according to the parameters. Conventions inside them: `CARDNAME` and
`NICKNAME` are replaced by the card's name / short name; `StackDescription$ None` hides the
text and `StackDescription$ SpellDescription` reuses the spell description; `{c:<Defined>}`,
`{p:<Defined>}` etc. interpolate object names. A sub-ability's `SpellDescription` is used as a
prompt. These keys are exempt from text-changing effects, while every *other* parameter of an
intrinsic ability is subject to them (a Mind Bend–style effect rewrites colour words and
basic land types inside filters and Defined strings) unless the record has `LockInText$`.

---

## 8. Keys a translator can ignore

Listed only where the Java was checked. "Not read in `forge-game`" means a search of the rules
engine sources for the key found no reader; the AI lives in a separate module (`forge-ai`)
that is not part of the checkout, so these keys are consumed there or by the UI.

**Whole lines**

| Line | Why |
|---|---|
| `AI:RemoveDeck:…` | AI deck-building flags (`CardRules.Reader`). |
| `DeckHints:`, `DeckNeeds:`, `DeckHas:` | Deck-building hints. |
| `Oracle:` | Display text. (Two non-display uses exist: deck-colour detection for lands, and enabling "mana spent" tracking when the text contains `was spent to cast` / `mana spent to pay` — a translator that always tracks colours spent can ignore both.) |
| `Variant:…:FlavorName:…` | Alternative display name. |
| `SETCOLORID`, `Lights`, `HandLifeModifier`, `Draft`, `DeckRule` | Not relevant to Commander / 1v1 constructed rules (Vanguard, Attractions, Conspiracy draft, deck construction). `DeckRule` *is* a rule, but a deck-construction one. |
| `Text:` | Display only. Caution: the sentence it shows may describe a real rule that Forge implements elsewhere (a `K:` line or hard-coded), so check that the rule has another source before dropping it. |

**Record keys — descriptive** (Forge's own `descriptiveKeys` list plus prompt keys):
`SpellDescription`, `StackDescription`, `TriggerDescription`, `Description`, `ChangeTypeDesc`,
`ValidTgtsDesc`, `TgtPrompt`, `CostDesc`, `PrecostDesc`.

**Record keys — presentation only**: `Secondary$ True` (every reader in `forge-game` is
text-building code: it hides the ability from the rules text).

**Record keys — AI only** (not read in `forge-game`): `AITgts`, `AITgtsStrict`, `AICheckSVar`,
`AISVarCompare`, `AIXMax`, `AIManaPref`, `AINoRecursiveCheck`, `AILifeThreshold`,
`AITgtOwnCards`, `AIPhyrexianPayment`, `AIExpectAmount`, `AIMinTgts`, `AIBidMax`,
`AIRespondsToOwnAbility`, `AIActivateLast`, `UnlessAI`, `Stackable`, `IsCurse` (defined in
`SpellAbility` but only consulted by the AI), `Ultimate` (only the achievement tracker reads
it). `AILogic` is AI-only in practice, with one exception found: `ChooseSector` passes it to
the player controller as a hint — still not a rules input.

**SVars — AI hints** (never read by name in `forge-game`; some are only *written* there for
the AI's benefit): `AIPreference`, `AIEvaluationModifier`, `AIPriorityModifier`,
`AICurseEffect`, `PlayMain1`, `NeedsToPlay`, `NeedsToPlayVar`, `NonStackingEffect`,
`NoZeroToughnessAI`, `BuffedBy`, `AntiBuffedBy`, `EnchantMe`, `EquipMe`, `DonateMe`,
`DiscardMe`, `DiscardMeByOpp`, `DoNotDiscardIfAble`, `SacMe`, `UntapMe`, `HasCombatEffect`,
`HasAttackEffect`, `HasBlockEffect`, `MustBeBlocked`, `ManaNeededToAvoidNegativeEffect`,
`EndOfTurnLeavePlay`, and `AttachAILogic` / `AttachAITgts` / `AttachAIValid` (copied into the
implicit Aura spell as AI parameters). Two cautions: (1) an SVar is ignorable only as an *AI
hint*; the same line format also holds abilities and numbers, so decide by name; (2) the
filter property `HasSVar <name>` can observe any SVar (one corpus use, on `SacMe`).

**Not ignorable although they look like hints**: `SVar:NeedsOrderedGraveyard` (tells the
engine graveyard order matters for this card), `SVar:X:Count$xPaid` (declares an announced X),
`Named$` (names an ability so `FromNamedAbility…` / `NamedAbility…` can find it),
`ReplacementResult$`, `Layer$`, `Static$`, `LockInText$`, `NonBasicSpell$`, `Planeswalker$`.

Everything else should be assumed to be rules-relevant until checked.

---

## 9. Things a translator author must know

1. **A failed condition skips one link, not the chain.** `Condition…` keys on a link gate only
   that link's effect; the `SubAbility` after it still runs (`AbilityUtils.resolveApiAbility`).
   "If you do" is therefore encoded by remembering something and testing it on *each* later
   link, or with `UnlessCost … UnlessSwitched … UnlessResolveSubs`.
2. **Flags are presence-tested.** `SorcerySpeed$ False` still means sorcery speed. Read the
   value only for the few keys that compare it (§2.1).
3. **Filters are matched by an ordered chain of prefix tests**, not by a table of names
   (§4.1). Unknown-looking suffixes are accepted silently; unknown tokens fall through to
   "has this type"; a token containing a colour word is a colour test. A faithful translator
   needs the branch order, which is why §4.5 is written the way it is.
4. **Everything silently defaults to zero / empty.** A missing SVar is 0; an unknown
   `Count$` body is 0; an unknown head is 0; an unknown card Defined is the empty set — but an
   unknown *player* Defined is **all players**. Misspellings in the corpus therefore "work".
   A translator should reject what it does not recognise rather than reproduce the default.
5. **State lives on the card, not in the ability.** Remembered / imprinted / chosen values
   outlive the ability and are shared across the card's abilities (§7.2). `DB$ Cleanup` is
   scope management, and a script that forgets it leaks state into the next activation.
   Inside a `DB$ Effect`, "remembered" means the effect object's own memory.
6. **Much of a card is not in its script.** Permanent spells, land plays, Aura spells, basic
   land mana abilities, planeswalker loyalty and battle defense counters are implicit (§1.6);
   keywords (`K:`) expand into triggers/statics/replacements in Java; `Loyalty`/`PT` with `*`
   rely on a separate characteristic-defining static.
7. **Costs: order, alternatives and X.** Cost parts are re-sorted into a fixed payment order;
   alternatives inside a cost Type use `;` not `,`; unknown tokens become (zero) mana; "X" is
   an announced value only when `SVar:X` is `Count$xPaid`, otherwise it is computed. An `AB$`
   with a non-zero cost reached from a trigger makes the trigger an optional "you may pay"
   (§3.4).
8. **One arithmetic suffix per expression**, evaluated after the whole body; chaining requires
   an intermediate `SVar$`. Comparison operands and filter RHS values are evaluated relative
   to the *host*, not the object being tested.
9. **Triggers re-check their conditions on resolution by default** (every mode but `Always`),
   i.e. Forge treats trigger conditions as intervening-ifs unless `NoResolvingCheck` is set
   (§2.8). And `Triggered<Key>` vs `Triggered<Key>LKICopy` is the difference between "the
   object as it is now (possibly in a new zone)" and "the object as it last existed".
10. **The default duration is end of turn**, not "indefinitely"; `Duration$ Permanent` is the
    explicit way to say no expiry (§7.1). And an effect whose duration premise is already false
    on resolution does nothing.
11. **Targets are shared along the chain.** `Defined$ Targeted` on any link means the targets
    of *every* link of the chain (root to tail); `ThisTargetedCard` / `ParentTarget` narrow it;
    the numeric head `Targeted$…` instead looks at this link, then at parents. Several
    targeted links in one chain each have their own `ValidTgts`. `Pump` with no
    parameters is the idiom for "a link that only exists to declare a target".
12. **`Spell` means two things**: as a card type word, "instant/sorcery (or unattached Aura)
    card" regardless of zone; in an SA filter, "a spell as opposed to an ability" (§4.2).
13. **Text-changing effects rewrite script parameters.** Filters, Defined strings and
    expressions of intrinsic abilities pass through `applyAbilityTextChangeEffects` before
    use. A translated card needs an equivalent hook only if such effects are in scope.

## Could not be verified

- The contents of the multiword-type list (`CardType.Constant.MultiwordTypes`), loaded from a
  data file outside the checkout (§1.4).
- Cost amounts `1+` and `Tapped`, and how the interactive payment code interprets them
  (`forge-gui` / `forge-ai` are not in the checkout) (§3.2).
- The extra payment options of `Waterbend<…>` (§3.3).
- `Mana<2\Creature>` from Forge's `Costs.md` — the parser only recognises the restrictions
  `NumTimes`, `Exiled`, `EnchantedCost`, `XMin<n>` (§3.3).
- The individual meanings of the effect-specific `Remember<What>$` flags other than those of
  `ChangeZone` (§7.2).
- Per-keyword argument grammars (`K:` lines) and per-API / per-trigger-mode / per-static-mode
  parameters: out of scope for this document.
- The player property family `wasDealt…DamageLastTurn` (§4.4) was read but its exact wording
  in scripts was not found in the corpus.

