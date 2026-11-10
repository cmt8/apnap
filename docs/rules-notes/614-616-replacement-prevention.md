# CR 614–616 — Replacement and prevention effects

- **Source:** Comprehensive Rules effective **November 6, 2026** (local `docs/rules/MagicCompRules.txt`, not committed).
- **Milestone:** M2

## Verbatim text

> 614.1. Some continuous effects are replacement effects. Like prevention effects (see rule 615), replacement effects apply continuously as events happen—they aren’t locked in ahead of time. Such effects watch for a particular event that would happen and completely or partially replace that event with a different event. They act like “shields” around whatever they’re affecting.
>
> 614.1a Effects that use the word “instead” are replacement effects. Most replacement effects use the word “instead” to indicate what events will be replaced with other events.
>
> 614.1b Effects that use the word “skip” are replacement effects. These replacement effects use the word “skip” to indicate what events, steps, phases, or turns will be replaced with nothing.
>
> 614.1c Effects that read “[This permanent] enters with . . . ,” “As [this permanent] enters . . . ,” or “[This permanent] enters as . . . “ are replacement effects.
>
> 614.1d Continuous effects that read “[This permanent] enters . . .” or “[Objects] enter [the battlefield] . . .” are replacement effects.
>
> 614.1e Effects that read “As [this permanent] is turned face up . . . ,” are replacement effects.
>
> 614.2. Some replacement effects apply to damage from a source. See rule 609.7.
>
> 614.3. There are no special restrictions on casting a spell or activating an ability that generates a replacement effect. Such effects last until they’re used up or their duration has expired.
>
> 614.4. Replacement effects must exist before the appropriate event occurs—they can’t “go back in time” and change something that’s already happened. Spells or abilities that generate these effects are often cast or activated in response to whatever would produce the event and thus resolve before that event would occur.
>
> Example: A player can activate an ability to regenerate a creature in response to a spell that would destroy it. Once the spell resolves, though, it’s too late to regenerate the creature.
>
> 614.5. A replacement effect doesn’t invoke itself repeatedly; it gets only one opportunity to affect an event or any modified events that may replace that event.
>
> Example: A player controls two permanents, each with an ability that reads “If a creature you control would deal damage to a permanent or player, it deals double that damage to that permanent or player instead.” A creature that normally deals 2 damage will deal 8 damage—not just 4, and not an infinite amount.
>
> 614.6. If an event is replaced, it never happens. A modified event occurs instead, which may in turn trigger abilities. Note that the modified event may contain instructions that can’t be carried out, in which case the impossible instruction is simply ignored.
>
> 614.7. If a replacement effect would replace an event, but that event never happens, the replacement effect simply doesn’t do anything.
>
> 614.7a If a source would deal 0 damage, it does not deal damage at all. Replacement effects that would increase the damage dealt by that source, or would have that source deal that damage to a different object or player, have no event to replace, so they have no effect.
>
> 614.10. An effect that causes a player to skip an event, step, phase, or turn is a replacement effect. “Skip [something]” is the same as “Instead of doing [something], do nothing.” Once a step, phase, or turn has started, it can no longer be skipped—any skip effects will wait until the next occurrence.
>
> 614.10a Anything scheduled for a skipped step, phase, or turn won’t happen. Anything scheduled for the “next” occurrence of something waits for the first occurrence that isn’t skipped. If two effects each cause a player to skip their next occurrence, that player must skip the next two; one effect will be satisfied in skipping the first occurrence, while the other will remain until another occurrence can be skipped.
>
> 614.10b Some effects cause a player to skip a step, phase, or turn, then take another action. That action is considered to be the first thing that happens during the next step, phase, or turn to actually occur.
>
> 614.11. Some effects replace card draws. These effects are applied even if no cards could be drawn because there are no cards in the affected player’s library.
>
> 614.11a If an effect replaces a draw within a sequence of card draws, all actions required by the replacement are completed, if possible, before resuming the sequence.
>
> 614.11b If an effect would have a player both draw a card and perform an additional action on that card, and the draw is replaced, the additional action is not performed on any cards that are drawn as a result of that replacement effect.
>
> 614.12. Some replacement effects modify how a permanent enters the battlefield. (See rules 614.1c–d.) Such effects may come from the permanent itself if they affect only that permanent (as opposed to a general subset of permanents that includes it). They may also come from other sources. To determine which replacement effects apply and how they apply, check the characteristics of the permanent as it would exist on the battlefield, taking into account replacement effects that have already modified how it enters the battlefield (see rule 616.1), continuous effects from the permanent’s own static abilities that would apply to it once it’s on the battlefield, and continuous effects that already exist and would apply to the permanent.
>
> Example: Voice of All says “As this creature enters, choose a color” and “This creature has protection from the chosen color.” An effect creates a token that’s a copy of Voice of All. As that token is created, the token’s controller chooses a color for it.
>
> Example: Yixlid Jailer says “Cards in graveyards lose all abilities.” Scarwood Treefolk says “This creature enters tapped.” A Scarwood Treefolk that’s put onto the battlefield from a graveyard enters the battlefield tapped.
>
> Example: Orb of Dreams is an artifact that says “Permanents enter tapped.” It won’t affect itself, so Orb of Dreams enters the battlefield untapped.
>
> 614.12a If a replacement effect that modifies how a permanent enters the battlefield requires a choice, that choice is made before the permanent enters the battlefield.
>
> 614.12b If multiple replacement effects that require choices from a player would modify how multiple permanents enter the battlefield simultaneously, that player may not make choices for those effects that would cause the combined costs of those effects to not be payable.
>
> 614.12c Some replacement effects cause a permanent to enter the battlefield with its controller’s choice of one of two abilities, each marked with an anchor word and preceded by a bullet point. “[Anchor word] — [ability]” means “As long as [anchor word] was chosen as this permanent entered the battlefield, this permanent has [ability].” The abilities preceded by anchor words are each linked to the ability that causes a player to choose between them. See rule 607, “Linked Abilities.”
>
> 614.15. Some replacement effects are not continuous effects. Rather, they are an effect of a resolving spell or ability that replace part or all of that spell or ability’s own effect(s). Such effects are called self-replacement effects. The text creating a self-replacement effect is usually part of the ability whose effect is being replaced, but the text can be a separate ability, particularly when preceded by an ability word. When applying replacement effects to an event, self-replacement effects are applied before other replacement effects.
>
> 614.16. Some replacement effects apply “if an effect would create one or more tokens” or “if an effect would put one or more counters on a permanent.” These replacement effects apply if the effect of a resolving spell or ability creates a token or puts a counter on a permanent, and they also apply if another replacement or prevention effect does so, even if the original event being modified wasn’t itself an effect. 
>
> 614.17. Some effects state that something can’t happen. These effects aren’t replacement effects, but follow similar rules.
>
> 614.17a “Can’t” effects must exist before the appropriate event occurs—they can’t “go back in time” and change something that’s already happened.
>
> 614.17b If an event can’t happen, a player can’t choose to pay a cost that includes that event.
>
> 614.17c If an event can’t happen, it can only be replaced by a self-replacement effect (see rule 614.15). Other replacement and/or prevention effects can’t modify or replace it.
>
> 614.17d Some “can’t” effects modify how a permanent enters the battlefield or whether it can enter the battlefield. Such effects may come from the permanent itself if they affect only that permanent (as opposed to a general subset of permanents that includes it). They may also come from other sources. To determine which “can’t” effects apply, check the characteristics of the permanent as it would exist on the battlefield, taking into account replacement effects that have already modified how it enters the battlefield (see rule 616.1), continuous effects from the permanent’s own static abilities that would apply to it once it’s on the battlefield, and continuous effects that already exist and would apply to the permanent.
>
> 615. Prevention Effects
>
> 615.1. Some continuous effects are prevention effects. Like replacement effects (see rule 614), prevention effects apply continuously as events happen—they aren’t locked in ahead of time. Such effects watch for a damage event that would happen and completely or partially prevent the damage that would be dealt. They act like “shields” around whatever they’re affecting.
>
> 615.1a Effects that use the word “prevent” are prevention effects. Prevention effects use “prevent” to indicate what damage will not be dealt.
>
> 615.6. If damage that would be dealt is prevented, it never happens. A modified event may occur instead, which may in turn trigger abilities. Note that the modified event may contain instructions that can’t be carried out, in which case the impossible instruction is simply ignored.
>
> 615.7. Some prevention effects generated by the resolution of a spell or ability refer to a specific amount of damage—for example, “Prevent the next 3 damage that would be dealt to any target this turn.” These work like shields. Each 1 damage that would be dealt to the “shielded” permanent or player is prevented. Preventing 1 damage reduces the remaining shield by 1. If damage would be dealt to the shielded permanent or player by two or more applicable sources at the same time, the player or the controller of the permanent chooses which damage the shield prevents. Once the shield has been reduced to 0, any remaining damage is dealt normally. Such effects count only the amount of damage; the number of events or sources dealing it doesn’t matter.
>
> 616.1. If two or more replacement and/or prevention effects are attempting to modify the way an event affects an object or player, the affected object’s controller (or its owner if it has no controller) or the affected player chooses one to apply, following the steps listed below. If two or more players have to make these choices at the same time, choices are made in APNAP order (see rule 101.4).
>
> 616.1a If any of the replacement and/or prevention effects are self-replacement effects (see rule 614.15), one of them must be chosen. If not, proceed to rule 616.1b.
>
> 616.1b If any of the replacement and/or prevention effects would modify under whose control an object would enter the battlefield, one of them must be chosen. If not, proceed to rule 616.1c.
>
> 616.1c If any of the replacement and/or prevention effects would cause an object to become a copy of another object as it enters the battlefield, one of them must be chosen. If not, proceed to rule 616.1d.
>
> 616.1d If any of the replacement and/or prevention effects would cause a card to enter the battlefield with its back face up, one of them must be chosen (See rule 701.27, “Transform,” and rule 701.28, “Convert.”). If not, proceed to 616.1e.
>
> 616.1e Any of the applicable replacement and/or prevention effects may be chosen.
>
> 616.1f Once the chosen effect has been applied, this process is repeated (taking into account only replacement or prevention effects that would now be applicable) until there are no more left to apply.
>
> Example: Two permanents are on the battlefield. One is an enchantment that reads “If a card would be put into a graveyard from anywhere, instead exile it,” and the other is a creature that reads “If this creature would die, instead shuffle it into its owner’s library.” If the creature is destroyed, its controller decides which replacement to apply first; the other does nothing.
>
> Example: Essence of the Wild reads “Creatures you control enter as a copy of this creature.” A player who controls Essence of the Wild casts Rusted Sentinel, which normally enters the battlefield tapped. As it enters the battlefield, the copy effect from Essence of the Wild is applied first. As a result, it no longer has the ability that causes it to enter the battlefield tapped. Rusted Sentinel will enter the battlefield as an untapped copy of Essence of the Wild.
>
> 616.1g While following the steps in 616.1a–f, one replacement or prevention effect may apply to an event, and another may apply to an event contained within the first event. In this case, the second effect can’t be chosen until after the first effect has been chosen.
>
> Example: A player is instructed to create a token that’s a copy of Voice of All, which has the ability “As this creature enters, choose a color.” Doubling Season has an ability that reads “If an effect would create one or more tokens under your control, it creates twice that many of those tokens instead.” Because entering the battlefield is an event contained within the event of creating a token, the effect of Doubling Season must be applied first, and then the effects of the two Voice of All tokens may be applied in either order.
>
> 616.2. A replacement or prevention effect can become applicable to an event as the result of another replacement or prevention effect that modifies the event.
>
> Example: One effect reads “If you would gain life, draw that many cards instead,” and another reads “If you would draw a card, return a card from your graveyard to your hand instead.” Both effects combine (regardless of the order they came into existence): Instead of gaining 1 life, the player puts a card from their graveyard into their hand.
>
> 7. Additional Rules
>
> 700. General

## Notes and spec cross-check
- Spec §8.1 matches 616.1 on *who* chooses (affected object's controller / affected player; APNAP when several choose at once) and on self-replacement effects first (616.1a). **Missing from the spec:** 616.1b–d are three more must-choose-first tiers (control on entry, copy on entry, back face up), and **616.1g**: an effect on a *contained* event (e.g. entering the battlefield inside "create a token") can't be chosen until the containing event's effect has been.
- **614.5:** each replacement effect gets one opportunity per event (and its modified events) → apnap records the effects already applied to each proposed event.
- **614.7 / 614.7a:** an event that never happens isn't replaced; 0 damage is no event.
- **614.12:** effects that modify how a permanent enters are evaluated against the permanent *as it would exist on the battlefield*, including its own static abilities.
- **614.16 / 122.6:** "would put counters" effects also apply to counters a permanent enters with.
- **616.2:** applying one effect can make another applicable (re-evaluate after each application).

## Tests
`crates/harness/tests/rules_m2.rs`: `cr_614_*`, `cr_615_*`, `cr_616_*`.

## Entering with counters: "for each", another permanent's ability, X (D-052)
> 614.1c Effects that read “[This permanent] enters with . . . ,” “As [this permanent] enters . . . ,” or “[This permanent] enters as . . . “ are replacement effects.

> 614.12. Some replacement effects modify how a permanent enters the battlefield. (See rules 614.1c–d.) Such effects may come from the permanent itself if they affect only that permanent (as opposed to a general subset of permanents that includes it). They may also come from other sources. To determine which replacement effects apply and how they apply, check the characteristics of the permanent as it would exist on the battlefield, taking into account replacement effects that have already modified how it enters the battlefield (see rule 616.1), continuous effects from the permanent’s own static abilities that would apply to it once it’s on the battlefield, and continuous effects that already exist and would apply to the permanent.

> 122.6. Some spells and abilities refer to counters being put on an object. This refers to putting counters on that object while it’s on the battlefield and also to an object that’s given counters as it enters the battlefield.

> 107.3m If an object’s enters-the-battlefield triggered ability or replacement effect refers to X, and the spell that became that object as it resolved had a value of X chosen for any of its costs, the value of X for that ability is the same as the value of X for that spell, although the value of X for that permanent is 0. This is an exception to rule 107.3i.

Engine: `Replaces::OtherEnters(selector)` applies to another permanent that would enter
the battlefield and matches the selector *as it would exist there* (614.12), relative to
the source's controller; the source must be on the battlefield, so a permanent never
counts for itself as it enters. `ReplaceWith::EntersWithCounted` is evaluated when the
effect is applied to the event — "for each Angel you already control" doesn't count the
one entering. For the permanent's own "enters with X counters", `Amount::X` is the X of
the spell it is entering as (107.3m), read from its stack object.
