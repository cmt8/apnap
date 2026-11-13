# "Becomes the target", ward; revealing until (D-089) — 2026-11-06

Extracted from docs/rules/MagicCompRules.txt (effective 2026-11-06).

## Ward and "becomes the target"

> 702.21a Ward is a triggered ability. Ward [cost] means “Whenever this permanent becomes the
> target of a spell or ability an opponent controls, counter that spell or ability unless
> that player pays [cost].”

> 702.21b Some ward abilities include an X in their cost and state what X is equal to. This
> value is determined at the time the ability resolves, not locked in as the ability
> triggers.

> 601.2c The player announces their choice of an appropriate object or player for each
> target the spell requires. …

> 603.2c An ability triggers only once each time its trigger event occurs. However, it can
> trigger repeatedly if one event contains multiple occurrences.

> 115.7e When changing targets or choosing new targets for a spell or ability, only the final
> set of targets is evaluated to determine whether the change is legal.

> 707.10c … Once the player has decided what the copy’s targets will be, the copy is put onto
> the stack with those targets.

> 701.6a To counter a spell or ability means to cancel it, removing it from the stack. It
> doesn’t resolve and none of its effects occur. A countered spell is put into its owner’s
> graveyard.

> 118.12. Some spells, activated abilities, and triggered abilities read, “[Do something].
> If [a player] [does, doesn’t, or can’t], [effect].” … The action [do something] is a cost,
> paid when the spell or ability resolves. …

The rules don't define "becomes the target" beyond these. The engine (as Forge's
MagicStack.add and ChangeTargetsEffect do) takes the event to happen:
- when a spell is cast or an activated or triggered ability is put on the stack with the
  object among its targets (once all its choices are made, 601.2c–i);
- when a copy of a spell or ability is put on the stack with it as a target (707.10c: the
  copy is put on the stack with its targets);
- when a spell's or ability's targets are changed and the object is a new target (115.7;
  a kept target doesn't become one again).
One event per spell or ability and permanent, however many of its target words name the
permanent (603.2c: one occurrence).

Ward: the triggered ability "counter that spell or ability unless that player pays
[cost]" — a payment asked of the spell's or ability's controller as the ward ability
resolves (118.12); a countered ability is removed from the stack, a countered spell goes to
its owner's graveyard (701.6a). Any cost `Cost` can express: mana, life, discards,
sacrifices.

## Revealing until

> 701.20a To reveal a card, show that card to all players for a brief time. If an effect
> causes a card to be revealed, it remains revealed for as long as necessary to complete the
> parts of the effect that card is relevant to. …

Forge's `DigUntil`: "reveal cards from the top of your library until you reveal a [card].
Put that card [somewhere] and the rest [somewhere else]". The reveal is on the card.

## Tests

crates/harness/tests/forge_ward.rs, crates/harness/tests/forge_dig_until.rs:

| Test | Rule |
|---|---|
| (filled in as the tests are written) | |
