# Copies of spells and abilities; storm (CR 707.10–707.12, 702.40, 111.13, 608.3f) — 2026-11-06

Extracted from docs/rules/MagicCompRules.txt (effective 2026-11-06) for D-085. Copies of
permanents are in 707-copies.md.

## The rules

> 707.10. To copy a spell, activated ability, or triggered ability means to put a copy of it
> onto the stack; a copy of a spell isn’t cast and a copy of an activated ability isn’t
> activated. A copy of a spell or ability copies both the characteristics of the spell or
> ability and all decisions made for it, including modes, targets, the value of X, and
> additional or alternative costs. (See rule 601, “Casting Spells.”) Choices that are
> normally made on resolution are not copied. If an effect of the copy refers to objects
> used to pay its costs, it uses the objects used to pay the costs of the original spell or
> ability. A copy of a spell is owned by the player under whose control it was put on the
> stack. A copy of a spell or ability is controlled by the player under whose control it
> was put on the stack. A copy of a spell is itself a spell, even though it has no spell
> card associated with it. A copy of an ability is itself an ability.

> 707.10a If a copy of a spell is in a zone other than the stack, it ceases to exist. If a
> copy of a card is in any zone other than the stack or the battlefield, it ceases to exist.
> These are state-based actions. See rule 704.

> 707.10b A copy of an ability has the same source as the original ability. If the ability
> refers to its source by name, the copy refers to that same object and not to any other
> object with the same name. The copy is considered to be the same ability by effects that
> count how many times that ability has resolved during the turn.

> 707.10c Some effects copy a spell or ability and state that its controller may choose new
> targets for the copy. The player may leave any number of the targets unchanged, even if
> those targets would be illegal. If the player chooses to change some or all of the
> targets, the new targets must be legal. Once the player has decided what the copy’s
> targets will be, the copy is put onto the stack with those targets.

> 707.10f Some effects copy a permanent spell. As that copy resolves, it ceases being a copy
> of a spell and becomes a token permanent. (See rule 608.3f.)

> 707.12. An effect that instructs a player to cast a copy of an object (and not just copy a
> spell) follows the rules for casting spells, except that the copy is created in the same
> zone the object is in and then cast while another spell or ability is resolving. …

> 111.13. A copy of a permanent spell becomes a token as it resolves. The token has the
> characteristics of the spell that became that token. The token is not “created” for the
> purposes of any replacement effects or triggered abilities that refer to creating a token.

> 608.3f If the object that’s resolving is a copy of a permanent spell, it will become a
> token permanent as it is put onto the battlefield in any of the steps above. A token put
> onto the battlefield this way is no longer a copy of a spell and is not “created” for the
> purposes of any rules or effects that refer to creating a token.

> 608.2h … If the effect requires information from a specific object, including the source
> of the ability itself, the effect uses the current information of that object if it’s in
> the public zone it was expected to be in; if it’s no longer in that zone, … the effect uses
> the object’s last known information. …

> 601.2i Once the steps described in 601.2a–h are completed, effects that modify the
> characteristics of the spell as it’s cast are applied, then the spell becomes cast. Any
> abilities that trigger when a spell is cast or put onto the stack trigger at this time. …

> 702.40a Storm is a triggered ability that functions on the stack. “Storm” means “When you
> cast this spell, copy it for each other spell that was cast before it this turn. If the
> spell has any targets, you may choose new targets for any of the copies.”

> 702.40b If a spell has multiple instances of storm, each triggers separately.

> 704.5e If a copy of a spell is in a zone other than the stack, it ceases to exist. …

## What the engine does

- A copy is a new stack object with everything decided for the original: modes, X, targets,
  the alternative cost, what was paid by sacrifice (707.10). A copy of a spell is a
  `GameObject` with `spell_copy`, the original's card and face, its copiable values plus
  any exceptions ("except that the copy is red"), a new card instance (it is no commander),
  owned and controlled by the player who made it. A copy of an ability has the same source
  (707.10b). No `SpellCast` event: a copy is not cast, so "whenever you cast" doesn't see
  it — "whenever you cast or copy" does.
- "You may choose new targets for the copy" is D-082's "choose new targets" (115.7d), asked
  right after the copy is made and before the next copy is made (707.10c: a copy isn't on
  the stack while its targets are chosen, so no copy can target a later one; a spell never
  targets itself, 115.5). Keeping a target is allowed even if it has become illegal.
- The spell or ability copied is read as it is on the stack, or as it last was there if it
  has left (608.2h): a storm copy, or "copy that spell", is made even if the original was
  countered in response. The last known stack objects are kept while the stack isn't empty.
- A copy of a permanent spell resolves into a token (111.13, 608.3f, 707.10f), and is not
  "created". A copy of a spell anywhere but on the stack ceases to exist (704.5e).
- Storm: "When you cast this spell" triggers from the spell on the stack (601.2i); the
  number of spells cast before it this turn, by every player, is fixed as it triggers
  (copies were never cast and don't count).

## Tests (crates/harness/tests/forge_spell_copies.rs)

| Test | Rule |
|---|---|
| `a_copy_is_not_cast` ("whenever you cast or copy" too) | 707.10, 601.2i |
| `a_copy_keeps_modes_x_and_targets` | 707.10 |
| `new_targets_for_the_copy_are_optional` | 707.10c, 115.5 |
| `countering_the_original_leaves_the_copy` | 707.10, 704.5e |
| `a_copied_permanent_spell_becomes_a_token` | 707.10f, 111.13, 608.3f |
| `copy_target_triggered_or_activated_ability` | 707.10, 707.10b |
| `whenever_you_cast_copy_that_spell` | 707.10, 608.2h |
| `storm_counts_every_players_spells` | 702.40a, 608.2h |
| `copy_it_for_each_other_spell_of_its_kind` | 707.10, 608.2h |
| `save_and_load_with_a_copy_on_the_stack` | 707.10 |
| `copies_not_built_are_named` | — |
