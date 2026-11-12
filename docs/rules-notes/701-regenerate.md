# 701.19 Regenerate, 614.8 (D-079)

Comprehensive Rules effective 2026-11-06, quoted verbatim. Tests:
`crates/harness/tests/forge_regen.rs`.

## What regeneration is

> 701.19a If the effect of a resolving spell or ability regenerates a permanent, it creates a
> replacement effect that protects the permanent the next time it would be destroyed this
> turn. In this case, “Regenerate [permanent]” means “The next time [permanent] would be
> destroyed this turn, instead remove all damage marked on it and its controller taps it.
> If it’s an attacking or blocking creature, remove it from combat.”

> 701.19b If the effect of a static ability regenerates a permanent, it replaces
> destruction with an alternate effect each time that permanent would be destroyed. In
> this case, “Regenerate [permanent]” means “Instead remove all damage marked on
> [permanent] and its controller taps it. If it’s an attacking or blocking creature, remove
> it from combat.”

> 701.19c Neither activating an ability that creates a regeneration shield nor casting a
> spell that creates a regeneration shield is the same as regenerating a permanent. Effects
> that say that a permanent can’t be regenerated don’t preclude such abilities from being
> activated or such spells from being cast; rather, they cause regeneration shields to not
> be applied.

> 614.8. Regeneration is a destruction-replacement effect. The word “instead” doesn’t
> appear on the card but is implicit in the definition of regeneration. “Regenerate
> [permanent]” means “The next time [permanent] would be destroyed this turn, instead
> remove all damage marked on it and its controller taps it. If it’s an attacking or
> blocking creature, remove it from combat.” Abilities that trigger from damage being dealt
> still trigger even if the permanent regenerates. See rule 701.19.

> 614.4. Replacement effects must exist before the appropriate event occurs—they can’t “go
> back in time” and change something that’s already happened. …
> Example: A player can activate an ability to regenerate a creature in response to a spell
> that would destroy it. Once the spell resolves, though, it’s too late to regenerate the
> creature.

> 506.4. A permanent is removed from combat if … it’s an attacking or blocking creature
> that regenerates (see rule 701.19) …

## What is destruction (and what isn't)

> 701.8a To destroy a permanent, move it from the battlefield to its owner’s graveyard.

> 701.8b The only ways a permanent can be destroyed are as a result of an effect that uses
> the word “destroy” or as a result of the state-based actions that check for lethal damage
> (see rule 704.5g) or damage from a source with deathtouch (see rule 704.5h). If a
> permanent is put into its owner’s graveyard for any other reason, it hasn’t been
> “destroyed.”

> 701.8c A regeneration effect replaces a destruction event. See rule 701.19, “Regenerate.”

> 704.5f If a creature has toughness 0 or less, it’s put into its owner’s graveyard.
> Regeneration can’t replace this event.

> 704.5g If a creature has toughness greater than 0, it has damage marked on it, and the
> total damage marked on it is greater than or equal to its toughness, that creature has
> been dealt lethal damage and is destroyed. Regeneration can replace this event.

> 704.5h If a creature has toughness greater than 0, and it’s been dealt damage by a source
> with deathtouch since the last time state-based actions were checked, that creature is
> destroyed. Regeneration can replace this event.

> 702.2b A creature with toughness greater than 0 that’s been dealt damage by a source with
> deathtouch since the last time state-based actions were checked is destroyed as a
> state-based action. See rule 704.

> 701.21a To sacrifice a permanent, its controller moves it from the battlefield directly to
> its owner’s graveyard. A player can’t sacrifice something that isn’t a permanent, or
> something that’s a permanent they don’t control. Sacrificing a permanent doesn’t destroy
> it, so regeneration or other effects that replace destruction can’t affect this action.

> 702.12b A permanent with indestructible can’t be destroyed. …

## In the engine

- A regeneration shield is an `effects::Shield` with `regenerate` set, on one permanent,
  created by `Effect::Regenerate` as the spell or ability resolves (701.19a), and gone at
  cleanup ("this turn", 514.2) or once used.
- It is a replacement effect in the `replacement::Proposed` pipeline: applicable to a
  `Proposed::Move` to the graveyard with `MoveCause::Destroy` (destroy effects, 704.5g,
  704.5h) of a permanent on the battlefield; it turns the move into
  `Proposed::Regenerate` (damage removed, tapped, removed from combat). Several shields, or a
  shield and another replacement effect, are chosen among by the affected player (616.1).
- "It can't be regenerated" on a destroy instruction makes its destruction
  `MoveCause::DestroyNoRegen`; "[these] can't be regenerated" for a while is
  `AbilityDef::CantBeRegenerated` (a static ability, or a rule a lasting effect carries):
  shields aren't applied (701.19c), and may still be made.
- Not built: static regeneration (701.19b, Forge `R:Event$ Destroy | Regeneration$`),
  "when it regenerates this way" (`RegenerationAbility$`).

| Test | Rule |
|---|---|
| `a_shield_replaces_destruction_once` | 701.19a, 614.8, 701.8c |
| `a_shield_replaces_lethal_damage_and_deathtouch` | 704.5g, 704.5h, 702.2b |
| `a_shield_doesnt_replace_sacrifice_or_toughness_zero` | 701.21a, 704.5f |
| `a_regenerating_attacker_is_removed_from_combat` | 701.19a, 506.4 |
| `a_shield_ends_at_cleanup` | 701.19a ("this turn"), 514.2 |
| `cant_be_regenerated_on_a_destroy_instruction` | 701.19c |
| `cant_be_regenerated_this_turn_is_a_lasting_rule` | 701.19c, 611.2 |
