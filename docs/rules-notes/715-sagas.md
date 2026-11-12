# CR 715 — Saga cards (D-084)

- **Source:** Comprehensive Rules effective **November 6, 2026** (local `docs/rules/MagicCompRules.txt`, not committed).
- **Numbering:** in this edition Sagas are rule **715** ("Saga Cards"; 714 is "Substitute
  Cards"). The D-084 brief said 714; the CR wins.

## Verbatim text

> 715. Saga Cards

> 715.1. Each Saga card has a striated text box containing a number of chapter symbols. Its illustration is vertically oriented on the right side of the card, and its type line is along the bottom of the card.

> 715.1a Saga enchantments that also have the type creature are printed with both a power and toughness box and an additional text box below the type line. Any abilities in that text box are independent of its chapter symbols.

> 715.2. A chapter symbol is a keyword ability that represents a triggered ability referred to as a chapter ability.

> 715.2a A chapter symbol includes a Roman numeral, indicated here as “{rN}.” The numeral I represents 1, II represents 2, III represents 3, and so on.

> 715.2b “{rN}—[Effect]” means “When one or more lore counters are put onto this Saga, if the number of lore counters on it was less than N and became at least N, [effect].”

> 715.2c “{rN1}, {rN2}—[Effect]” means the same as “{rN1}—[Effect]” and “{rN2}—[Effect].”

> 715.2d A Saga’s final chapter number is the greatest value among chapter abilities it has. If a Saga somehow has no chapter abilities, its final chapter number is 0.

> 715.2e A Saga’s final chapter ability is the chapter ability which has its final chapter number in its chapter symbol.

> 715.3. Sagas use lore counters to track their progress.

> 715.3a Each Saga without read ahead has the intrinsic ability “This Saga enters with a lore counter on it.” This ability creates a replacement effect (see rule 614.1c).

> 715.3b Each Saga with read ahead has the intrinsic abilities “As this Saga enters, choose a number between one and this Saga’s final chapter number” and “This Saga enters with the chosen number of lore counters on it.” (See rule 702.155, “Read Ahead.”) These abilities create replacement effects (see rule 614.1c).

> 715.3c As a player’s precombat main phase begins, that player puts a lore counter on each Saga they control with one or more chapter abilities. This turn-based action doesn’t use the stack.

> 715.4. If the number of lore counters on a Saga permanent with one or more chapter abilities is greater than or equal to its final chapter number, and it isn’t the source of a chapter ability that has triggered but not yet left the stack, that Saga’s controller sacrifices it. This state-based action doesn’t use the stack.

## Related rules (verbatim)

> 107.15a “{rN}—[Effect]” means “When one or more lore counters are put onto this Saga, if the number of lore counters on it was less than N and became at least N, [effect].”

> 505.4. Second, if the active player controls one or more Saga enchantments and it’s the active player’s precombat main phase, the active player puts a lore counter on each Saga they control with one or more chapter abilities. (See rule 715, “Saga Cards.”) This turn-based action doesn’t use the stack.

> 703.4f Immediately after a player’s precombat main phase begins, that player puts a lore counter on each Saga enchantment they control with one or more chapter abilities. In an Archenemy game, this happens after the archenemy’s scheme action. See rule 715, “Saga Cards.”

> 704.5s If the number of lore counters on a Saga permanent with one or more chapter abilities is greater than or equal to its final chapter number and it isn’t the source of a chapter ability that has triggered but not yet left the stack, that Saga’s controller sacrifices it. See rule 715, “Saga Cards.”

> 702.155a Read ahead is a keyword found on some Saga cards. “Read ahead” means “Chapter abilities of this Saga can’t trigger the turn it entered the battlefield unless it has exactly the number of lore counters on it specified in the chapter symbol of that ability.” See rule 715, “Saga Cards.”

> 702.155b Each Saga with read ahead has the intrinsic abilities “As this Saga enters, choose a number between one and this Saga’s final chapter number” and “This Saga enters with the chosen number of lore counters on it.” See rule 715.3b.

> 702.155c Multiple instances of read ahead on the same object are redundant.

> 122.6. Some spells and abilities refer to counters being put on an object. This refers to putting counters on that object while it’s on the battlefield and also to an object that’s given counters as it enters the battlefield.

## How the engine reads these

- A chapter ability is `AbilityDef::Triggered { when: Trigger::Chapter(n) }`, one per chapter
  number (715.2c: "I, II — X" is two abilities). It triggers from a batch of performed events
  in which lore counters were put on its source — by an effect, a cost, the turn-based action,
  or as the Saga entered (122.6) — when the count before the batch was below `n` and after it
  is at least `n` (715.2b). Several chapters can trigger at once (a doubled lore counter).
- 715.3a, the intrinsic "enters with a lore counter", is compiled onto the card as the
  ordinary `Replaces::ThisEnters` / `EntersWithCounters { lore, 1 }` ability (so it is a
  replacement effect other effects can modify, 614.1c, and a copy of a Saga has it). An
  object that becomes a Saga only through a type-changing effect would lack it (LIMIT).
- 715.3c / 505.4: a turn-based action as the active player's precombat main phase begins —
  one `Proposed::PutCounters` per Saga they control with a chapter ability, performed as one
  batch through the replacement pipeline (no stack).
- 715.4 / 704.5s: a state-based action; a chapter ability "that has triggered but not yet left
  the stack" is a pending trigger or an ability on the stack whose source is the Saga.
- Read ahead (702.155) is not built: blocked by name.
