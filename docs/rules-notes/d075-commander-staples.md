# D-075: the most played Commander cards that no feature family covers

Comprehensive Rules effective 2026-11-06, quoted verbatim. One section per feature of the
batch; tests in `crates/harness/tests/forge_staples.rs`.

## Life lost this way (119.3, 608.2c)

> 119.3. If an effect causes a player to gain life or lose life, that player’s life total is
> adjusted accordingly.

> 608.2c The controller of the spell or ability follows its instructions in the order
> written. However, replacement effects may modify these actions. …

"You gain life equal to the life lost this way" (Exsanguinate, Gray Merchant of Asphodel,
Kokusho) is what the players actually lost to the instruction before it: after replacement
effects, and not more than their life totals went down.

| Test | Rule |
|---|---|
| `life_lost_this_way_is_what_the_opponents_lost` | 119.3, 608.2c |
| `life_lost_this_way_is_zero_when_nothing_was_lost` | 119.3 |
| `a_value_forge_sets_while_resolving_is_never_read_as_written` | compiler (invariant 15) |

## "In any order" (401.4)

> 401.4. If an effect puts two or more cards in a specific position in a library at the
> same time, the owner of those cards may arrange them in any order. That library’s owner
> doesn’t reveal the order in which the cards go into the library.

Forge's `Reorder$` lets the player who chose the cards order them; the engine always has
them ordered by their owner — the same player for cards from their own hand or library.

| Test | Rule |
|---|---|
| `cards_put_on_top_from_the_hand_are_ordered_by_their_owner` | 401.4 |

## Untap up to N (701.26b)

> 701.26b To untap a permanent, rotate it back to the upright position from a sideways
> position. Only tapped permanents can be untapped.

"Untap up to two lands" (Snap, Frantic Search, Peregrine Drake): any lands, chosen as the
spell or ability resolves — not targets. Only tapped ones are offered (choosing an untapped
one would do nothing).

| Test | Rule |
|---|---|
| `untap_up_to_n_lands_chooses_among_tapped_lands_of_anyone` | 701.26b |

## Proliferate (701.34)

> 701.34a To proliferate means to choose any number of permanents and/or players that have a
> counter, then give each one additional counter of each kind that permanent or player
> already has.

No player can have a counter in this engine yet (poison, energy and experience counters
block every card that gives them), so only permanents are offered; the counters are put on
all at once (one event, replaceable). `Amount$ 2` is "proliferate twice": two choices.

| Test | Rule |
|---|---|
| `proliferate_gives_each_chosen_permanent_another_counter_of_each_kind` | 701.34a |

## Cards that share a land type (205.3i)

> 205.3i Lands have their own unique set of subtypes; these subtypes are called land types.
> The land types are Cave, Desert, Forest, Gate, Island, Lair, Locus, Mine, Mountain, Plains,
> Planet, Power-Plant, Sphere, Swamp, Tower, Town, and Urza’s. Of that list, Forest, Island,
> Mountain, Plains, and Swamp are the basic land types. See rule 305.6.

> 701.23b If a player is searching a hidden zone for cards with a stated quality, such as a
> card with a certain card type or color, that player isn’t required to find some or all of
> those cards even if they’re present in that zone.

"Search for up to two basic land cards that share a land type" (Myriad Landscape): the
cards are chosen one at a time, each further card sharing a land type with each card chosen
before it (Forge's rule; for two cards the same as having one in common).

| Test | Rule |
|---|---|
| `search_for_lands_that_share_a_land_type` | 205.3i, 701.23b |

## Delayed triggered abilities (603.7)

> 603.7a Delayed triggered abilities are created during the resolution of spells or
> abilities, as the result of a replacement effect being applied, or as a result of a static
> ability that allows a player to take an action. A delayed triggered ability won’t trigger
> until it has actually been created, even if its trigger event occurred just beforehand.

> 603.7b A delayed triggered ability will trigger only once—the next time its trigger event
> occurs—unless it has a stated duration, such as “this turn.” …

> 603.7c A delayed triggered ability that refers to a particular object still affects it even
> if the object changes characteristics. However, if that object is no longer in the zone
> it’s expected to be in at the time the delayed triggered ability resolves, the ability
> won’t affect it. …

> 603.7d If a spell creates a delayed triggered ability, the source of that delayed triggered
> ability is that spell. The controller of that delayed triggered ability is the player who
> controlled that spell as it resolved.

> 603.7e If an activated or triggered ability creates a delayed triggered ability, the source
> of that delayed triggered ability is the same as the source of that other ability. …

So far: "at the beginning of [step]" events only, triggering once; "this turn" ones end at
cleanup (514.2). What one remembers is fixed as it is created: a player ("its controller"),
an amount ("that spell's mana value"). Remembering objects is not compiled yet.

| Test | Rule |
|---|---|
| `a_delayed_trigger_adds_mana_equal_to_the_countered_spells_mana_value` | 603.7a–c, 202.3e |
| `delayed_triggers_of_the_next_turn_remember_a_player` | 603.7c–d |
| `a_delayed_trigger_for_this_turn_ends_at_cleanup` | 603.7b, 514.2 |

## Text only: `CostDesc$`

How a cost reads in the ability's description (Forge's `SpellAbility.getDescription`); the
cost is `Cost$`. No rule involved.

| Test | Rule |
|---|---|
| `a_cost_description_is_text_only` | 601.2b (the additional cost is still compiled from `Cost$`) |
