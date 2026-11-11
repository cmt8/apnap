# CR 605 / 106 — Mana abilities that cost more than {T}, make several mana, or do more (D-066)

- **Source:** Comprehensive Rules effective **November 6, 2026** (local `docs/rules/MagicCompRules.txt`, not committed).
- **Milestone:** M4 (D-066). Complements `601-casting-602-activating-605-mana.md` (D-049, D-052).

## Verbatim text

> 106.1b There are six types of mana: white, blue, black, red, green, and colorless.
>
> 106.3. Mana is produced by the effects of mana abilities (see rule 605). It may also be produced by the effects of spells, as well as by the effects of abilities that aren’t mana abilities. A spell or ability that produces mana instructs a player to add that mana. If mana is produced by a spell, the source of that mana is that spell. If mana is produced by an ability, the source of that mana is the source of that ability (see rule 113.7).
>
> 106.4. When an effect instructs a player to add mana, that mana goes into a player’s mana pool. From there, it can be used to pay costs immediately, or it can stay in the player’s mana pool as unspent mana. Each player’s mana pool empties at the end of each step and phase, and the player is said to lose this mana. Cards with abilities that produce mana or refer to unspent mana have received errata in the Oracle™ card reference to no longer explicitly refer to the mana pool.
>
> 106.5. If an ability would produce one or more mana of an undefined type, it produces no mana instead.
>
> 106.6. Some spells or abilities that produce mana restrict how that mana can be spent, have an additional effect that affects the spell or ability that mana is spent on, or create a delayed triggered ability (see rule 603.7a) that triggers when that mana is spent. This doesn’t affect the mana’s type.
>
> 106.7. Some abilities produce mana based on the type of mana another permanent or permanents “could produce.” The type of mana a permanent could produce at any time includes any type of mana that an ability of that permanent would produce if the ability were to resolve at that time, taking into account any applicable replacement effects in any possible order. Ignore whether any costs of the ability could or could not be paid. If that permanent wouldn’t produce any mana under these conditions, or no type of mana can be defined this way, there’s no type of mana it could produce.
>
> 106.12. To “tap [a permanent] for mana” is to activate a mana ability of that permanent that includes the {T} symbol in its activation cost. See rule 605, “Mana Abilities.”
>
> 106.12a An ability that triggers whenever a permanent “is tapped for mana” or is tapped for mana of a specified type triggers whenever such a mana ability resolves and produces mana or the specified type of mana.
>
> 107.4e A hybrid mana symbol is also a colored mana symbol, even if one of its components is colorless. Each one represents a cost that can be paid in one of two ways, as represented by the two halves of the symbol. A hybrid symbol such as {W/U} can be paid with either white or blue mana, and a monocolored hybrid symbol such as {2/B} can be paid with either one black mana or two mana of any type. A hybrid mana symbol is all of its component colors.
>
> 118.3. A player can’t pay a cost without having the necessary resources to pay it fully. For example, a player with only 1 life can’t pay a cost of 2 life, and a permanent that’s already tapped can’t be tapped to pay a cost. See rule 202, “Mana Cost and Color,” and rule 602, “Activating Activated Abilities.”
>
> 118.3a Paying mana is done by removing the indicated mana from a player’s mana pool. (Players can always pay 0 mana.) If excess mana remains in that player’s mana pool after making that payment, the player announces what mana is still there.
>
> 118.3b Paying life is done by subtracting the indicated amount of life from a player’s life total. (Players can always pay 0 life.)
>
> 118.3c Activating mana abilities is not mandatory, even if paying a cost is.
>
> 119.4. If a cost or effect allows a player to pay an amount of life greater than 0, the player may do so only if their life total is greater than or equal to the amount of the payment. If a player pays life, the payment is subtracted from their life total; in other words, the player loses that much life.
>
> 601.2g If the total cost includes a mana payment, the player then has a chance to activate mana abilities (see rule 605, “Mana Abilities”). Mana abilities must be activated before costs are paid.
>
> 601.2h The player pays the total cost. First, they pay all costs that don’t involve random elements or moving objects from the library to a public zone, in any order. Then they pay all remaining costs in any order. Partial payments are not allowed. Unpayable costs can’t be paid.
>
> 602.2b The remainder of the process for activating an ability is identical to the process for casting a spell listed in rules 601.2b–i. Those rules apply to activating an ability just as they apply to casting a spell. An activated ability’s analog to a spell’s mana cost (as referenced in rule 601.2f) is its activation cost.
>
> 605.1. Some activated abilities and some triggered abilities are mana abilities, which are subject to special rules. Only abilities that meet either of the following two sets of criteria are mana abilities, regardless of what other effects they may generate or what timing restrictions (such as “Activate only as an instant”) they may have.
>
> 605.1a An activated ability is a mana ability if it meets all of the following criteria: it doesn’t require a target (see rule 115.6), it could add mana to a player’s mana pool when it resolves, it’s not a loyalty ability (see rule 606, “Loyalty Abilities”), and its cost and effect don’t move any card to or from a library. Do not take into account replacement effects that may apply, other than self-replacement effects, when evaluating these criteria.
>
> 605.1b A triggered ability is a mana ability if it meets all of the following criteria: it doesn’t require a target (see rule 115.6), it triggers from the activation or resolution of an activated mana ability (see rule 605.1a) or from mana being added to a player’s mana pool, and it could add mana to a player’s mana pool when it resolves.
>
> 605.2. A mana ability remains a mana ability even if the game state doesn’t allow it to produce mana.
>
> 605.3. Activating an activated mana ability follows the rules for activating any other activated ability (see rule 602.2), with the following exceptions:
>
> 605.3a A player may activate an activated mana ability whenever they have priority, whenever they are casting a spell or activating an ability that requires a mana payment, or whenever a rule or effect asks for a mana payment, even if it’s in the middle of casting or resolving a spell or activating or resolving an ability.
>
> 605.3b An activated mana ability doesn’t go on the stack, so it can’t be targeted, countered, or otherwise responded to. Rather, it resolves immediately after it is activated. (See rule 405.6c.)
>
> 605.3c Once a player begins to activate a mana ability, that ability can’t be activated again until it has resolved.
>
> 605.4. Triggered mana abilities follow all the rules for other triggered abilities (see rule 603, “Handling Triggered Abilities”), with the following exception:
>
> 605.4a A triggered mana ability doesn’t go on the stack, so it can’t be targeted, countered, or otherwise responded to. Rather, it resolves immediately after the mana ability that triggered it, without waiting for priority.
>
> 605.5. Abilities that don’t meet the criteria specified in rules 605.1a–b and spells aren’t mana abilities.
>
> 605.5a An ability with a target is not a mana ability, even if it could put mana into a player’s mana pool when it resolves. The same is true for a triggered ability that could produce mana but triggers from an event other than activating a mana ability, or a triggered ability that triggers from activating a mana ability but couldn’t produce mana. These follow the normal rules for activated or triggered abilities, as appropriate.
>
> 701.21a To sacrifice a permanent, its controller moves it from the battlefield directly to its owner’s graveyard. A player can’t sacrifice something that isn’t a permanent, or something that’s a permanent they don’t control. Sacrificing a permanent doesn’t destroy it, so regeneration or other effects that replace destruction can’t affect this action.
>
