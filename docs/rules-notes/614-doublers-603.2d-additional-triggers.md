# CR 614 / 616 doublers, 603.2d "triggers an additional time", 101.2 "can't be countered" (D-073)

- **Source:** Comprehensive Rules effective **November 6, 2026** (local
  `docs/rules/MagicCompRules.txt`, not committed). Text below is verbatim.
- **Milestone:** M4 (Forge compiler work, D-073). The general 614–616 text is in
  `614-616-replacement-prevention.md`; this file adds what replacement effects that change
  *how much* rely on.

## Verbatim text

### Tokens and counters

> 111.1. Some effects put tokens onto the battlefield. A token is a marker used to represent any permanent that isn’t represented by a card.
>
> 111.2. The player who creates a token is its owner. The token enters the battlefield under that player’s control.
>
> 111.3. The spell or ability that creates a token may define the values of any number of characteristics for the token. This becomes the token’s “text.” The characteristic values defined this way are functionally equivalent to the characteristic values that are printed on a card; for example, they define the token’s copiable values. A token doesn’t have any characteristics not defined by the spell or ability that created it.
>
> 122.6. Some spells and abilities refer to counters being put on an object. This refers to putting counters on that object while it’s on the battlefield and also to an object that’s given counters as it enters the battlefield.
>
> 122.6a If an object enters the battlefield with counters on it, the effect causing the object to be given counters may specify which player puts those counters on it. If the effect doesn’t specify a player, the object’s controller puts those counters on it.
>
> 614.16. Some replacement effects apply “if an effect would create one or more tokens” or “if an effect would put one or more counters on a permanent.” These replacement effects apply if the effect of a resolving spell or ability creates a token or puts a counter on a permanent, and they also apply if another replacement or prevention effect does so, even if the original event being modified wasn’t itself an effect.

### A created token enters the battlefield

> 111.6. A token is subject to anything that affects permanents in general or that affects the token’s card type or subtype. A token isn’t a card (even if represented by a card that has a Magic back or that came from a Magic booster pack).
>
> 614.1c Effects that read “[This permanent] enters with . . . ,” “As [this permanent] enters . . . ,” or “[This permanent] enters as . . . “ are replacement effects.
>
> 614.1d Continuous effects that read “[This permanent] enters . . .” or “[Objects] enter [the battlefield] . . .” are replacement effects.
>
> 614.12. Some replacement effects modify how a permanent enters the battlefield. (See rules 614.1c–d.) Such effects may come from the permanent itself if they affect only that permanent (as opposed to a general subset of permanents that includes it). They may also come from other sources. To determine which replacement effects apply and how they apply, check the characteristics of the permanent as it would exist on the battlefield, taking into account replacement effects that have already modified how it enters the battlefield (see rule 616.1), continuous effects from the permanent’s own static abilities that would apply to it once it’s on the battlefield, and continuous effects that already exist and would apply to the permanent.
>
> 616.1g While following the steps in 616.1a–f, one replacement or prevention effect may apply to an event, and another may apply to an event contained within the first event. In this case, the second effect can’t be chosen until after the first effect has been chosen.
>
> Example: A player is instructed to create a token that’s a copy of Voice of All, which has the ability “As this creature enters, choose a color.” Doubling Season has an ability that reads “If an effect would create one or more tokens under your control, it creates twice that many of those tokens instead.” Because entering the battlefield is an event contained within the event of creating a token, the effect of Doubling Season must be applied first, and then the effects of the two Voice of All tokens may be applied in either order.
>
> 800.4b If an object would change to the control of a player who has left the game, it doesn’t. If a token would be created under the control of a player who has left the game, no token is created. If an object would be put onto the battlefield or onto the stack under the control of a player who has left the game, that object remains in its current zone. If a player would be controlled by a player who has left the game, they aren’t.

### Once per event, order chosen by the affected player

> 614.5. A replacement effect doesn’t invoke itself repeatedly; it gets only one opportunity to affect an event or any modified events that may replace that event.
>
> Example: A player controls two permanents, each with an ability that reads “If a creature you control would deal damage to a permanent or player, it deals double that damage to that permanent or player instead.” A creature that normally deals 2 damage will deal 8 damage—not just 4, and not an infinite amount.
>
> 616.1. If two or more replacement and/or prevention effects are attempting to modify the way an event affects an object or player, the affected object’s controller (or its owner if it has no controller) or the affected player chooses one to apply, following the steps listed below. If two or more players have to make these choices at the same time, choices are made in APNAP order (see rule 101.4).
>
> 616.1e Any of the applicable replacement and/or prevention effects may be chosen.
>
> 616.1f Once the chosen effect has been applied, this process is repeated (taking into account only replacement or prevention effects that would now be applicable) until there are no more left to apply.
>
> 616.2. A replacement or prevention effect can become applicable to an event as the result of another replacement or prevention effect that modifies the event.

### Triggering an additional time

> 603.2. Whenever a game event or game state matches a triggered ability’s trigger event, that ability automatically triggers. The ability doesn’t do anything at this point.
>
> 603.2d An ability may state that a triggered ability triggers additional times. In this case, rather than simply determining that such an ability has triggered, determine how many times it should trigger, then that ability triggers that many times. An effect that states that an ability triggers additional times doesn’t invoke itself repeatedly and doesn’t apply to other effects that affect how many times an ability triggers. An effect that states a triggered ability of an object triggers additional times refers only to triggered abilities that object has, not to any delayed or reflexive triggered abilities (see rule 603.7 and rule 603.12) that may be created by abilities the object has.

### "Can't be countered"

> 101.2. When a rule or effect allows or directs something to happen, and another effect states that it can’t happen, the “can’t” effect takes precedence.
>
> 614.17. Some effects state that something can’t happen. These effects aren’t replacement effects, but follow similar rules.
>
> 614.17a “Can’t” effects must exist before the appropriate event occurs—they can’t “go back in time” and change something that’s already happened.
>
> 701.6a To counter a spell or ability means to cancel it, removing it from the stack. It doesn’t resolve and none of its effects occur. A countered spell is put into its owner’s graveyard.

## Notes

- **"Can't be countered" (101.2, 614.17).** A spell that can't be countered can still be
  the target of a spell that counters; that spell resolves and does everything else it
  does, and the countering instruction does nothing to it. "This spell can't be
  countered" is an ability of the spell that works on the stack; "creature spells you
  control can't be countered" is an ability of a permanent, judged when the countering
  would happen.

- **One event, several kinds of tokens.** An effect in this engine creates one kind of
  token per event (`Proposed::CreateTokens`). A replacement effect can turn it into
  several kinds ("those tokens plus a Food token", "instead create a Clue, a Food and a
  Treasure"); those stay one event (`more`), so a later effect that adds "an additional
  token" adds it once (614.5, 616.1f), and one that applies to "Clue, Food or Treasure"
  tokens sees all of them.
- **Which tokens.** "If one or more creature tokens would be created" is judged on each
  kind of token as it would be created (111.3: its characteristics are those the effect
  defines), controlled by the player creating it (111.2). Only the kinds it applies to are
  doubled or replaced.
- **Counters as a permanent enters** count (122.6). As for other entering effects, another
  permanent's effect applies only if that permanent is already on the battlefield; the
  entering permanent is judged as it would exist there (614.12).
- **Tokens enter (fixed in D-073; before it, created tokens skipped every "enters"
  effect).** A fully modified `CreateTokens` event becomes one `TokenEnters` event per
  token (616.1g: entering is contained in creating, so the creation's effects are
  chosen first). Each token's entering is then modified like a card moved onto the
  battlefield (111.6): "each Dragon you control enters with an additional counter",
  "creatures your opponents control enter tapped", counter doublers on the counters it
  would enter with (122.6, 616.2). Each token is its own event, so when several such
  effects apply its controller orders them per token (616.1). A token definition can't
  have its own "this enters …" effect (card-db rejects it: no Forge token script has one).
- **"As it would exist on the battlefield" (614.12)** is worked out on a copy of the game
  in which the event has happened — the permanent with its counters, its own static
  abilities and every continuous effect that would apply ("creatures you control are
  Dragons") — for tokens and for cards moved there alike.
- **Order matters.** "Twice that many" and "that many plus one" give 4 or 3 from 1
  depending on which applies first; the affected player (616.1) chooses each time (616.1f).
- **603.2d.** An effect "that ability triggers an additional time" counts when the ability
  triggers: two such effects that apply give three instances; each instance is put on the
  stack separately, with its own targets and choices. Effects that don't apply to the
  trigger event (a creature entering vs a land entering) don't count.
