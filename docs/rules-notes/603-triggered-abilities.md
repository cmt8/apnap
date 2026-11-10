# CR 603 — Handling triggered abilities

- **Source:** Comprehensive Rules effective **November 6, 2026** (local `docs/rules/MagicCompRules.txt`, not committed).
- **Milestone:** M2

## Verbatim text

> 603.1. Triggered abilities have a trigger condition and an effect. They are written as “[When/Whenever/At] [trigger condition or event], [effect]. [Instructions (if any).]”
>
> 603.1a A triggered ability may include instructions after its effects that limit what the ability may target or state that it can’t be countered. This text is not part of the ability’s effect. It functions while the ability is on the stack.
>
> 603.1b A triggered ability may have more than one trigger condition, and an instruction that refers to whether “all” of those conditions have happened during a particular period. This refers to whether or not all of those conditions have occurred during that period, regardless of whether that ability has triggered based on those conditions.
>
> 603.2. Whenever a game event or game state matches a triggered ability’s trigger event, that ability automatically triggers. The ability doesn’t do anything at this point.
>
> 603.2a Because they aren’t cast or activated, triggered abilities can trigger even when it isn’t legal to cast spells and activate abilities. Effects that preclude abilities from being activated don’t affect them.
>
> 603.2b When a phase or step begins, all abilities that trigger “at the beginning of” that phase or step trigger.
>
> 603.2c An ability triggers only once each time its trigger event occurs. However, it can trigger repeatedly if one event contains multiple occurrences.
>
> Example: A permanent has an ability whose trigger condition reads, “Whenever a land is put into a graveyard from the battlefield, . . . .” If someone casts a spell that destroys all lands, the ability will trigger once for each land put into the graveyard during the spell’s resolution.
>
> 603.2d An ability may state that a triggered ability triggers additional times. In this case, rather than simply determining that such an ability has triggered, determine how many times it should trigger, then that ability triggers that many times. An effect that states that an ability triggers additional times doesn’t invoke itself repeatedly and doesn’t apply to other effects that affect how many times an ability triggers. An effect that states a triggered ability of an object triggers additional times refers only to triggered abilities that object has, not to any delayed or reflexive triggered abilities (see rule 603.7 and rule 603.12) that may be created by abilities the object has.
>
> 603.2e Some trigger events use the word “becomes” (for example, “becomes attached” or “becomes blocked”). These trigger only at the time the named event happens—they don’t trigger if that state already exists or retrigger if it persists. An ability that triggers when a permanent “becomes tapped” or “becomes untapped” doesn’t trigger if the permanent enters the battlefield in that state.
>
> Example: An ability that triggers when a permanent “becomes tapped” triggers only when the status of a permanent that’s already on the battlefield changes from untapped to tapped.
>
> 603.2f If a triggered ability’s trigger condition is met, but the object with that triggered ability is at no time visible to all players, the ability does not trigger.
>
> 603.2g An ability triggers only if its trigger event actually occurs. An event that’s prevented or replaced won’t trigger anything.
>
> Example: An ability that triggers on damage being dealt won’t trigger if all the damage is prevented.
>
> 603.2h A triggered ability may have an instruction followed by “Do this only once each turn.” This ability triggers only if its source’s controller has not yet taken the indicated action that turn.
>
> 603.3. Once an ability has triggered, its controller puts it on the stack as an object that’s not a card the next time a player would receive priority. See rule 117, “Timing and Priority.” The ability becomes the topmost object on the stack. It has the text of the ability that created it, and no other characteristics. It remains on the stack until it’s countered, it resolves, a rule causes it to be removed from the stack, or an effect moves it elsewhere.
>
> 603.3a A triggered ability is controlled by the player who controlled its source at the time it triggered, unless it’s a delayed triggered ability. To determine the controller of a delayed triggered ability, see rules 603.7d–g.
>
> 603.3b If multiple abilities have triggered since the last time a player received priority, the abilities are placed on the stack in a two-part process. First, each player, in APNAP order, puts each triggered ability they control with a trigger condition that isn’t another ability triggering on the stack in any order they choose. (See rule 101.4.) Second, each player, in APNAP order, puts all remaining triggered abilities they control on the stack in any order they choose. Then the game once again checks for and performs state-based actions until none are performed, then abilities that triggered during this process go on the stack. This process repeats until no new state-based actions are performed and no abilities trigger. Then the appropriate player gets priority.
>
> 603.3c If a triggered ability is modal, its controller announces the mode choice when putting the ability on the stack. If one of the modes would be illegal (due to an inability to choose legal targets, for example), that mode can’t be chosen. If no mode is chosen, the ability is removed from the stack. (See rule 700.2.)
>
> 603.3d The remainder of the process for putting a triggered ability on the stack is identical to the process for casting a spell listed in rules 601.2c–d. If a choice is required when the triggered ability goes on the stack but no legal choices can be made for it, or if a rule or a continuous effect otherwise makes the ability illegal, the ability is simply removed from the stack.
>
> 603.4. A triggered ability may read “When/Whenever/At [trigger event], if [condition], [effect].” When the trigger event occurs, the ability checks whether the stated condition is true. The ability triggers only if it is; otherwise it does nothing. If the ability triggers, it checks the stated condition again as it resolves. If the condition isn’t true at that time, the ability is removed from the stack and does nothing. Note that this mirrors the check for legal targets. This rule is referred to as the “intervening ‘if’ clause” rule. (The word “if” has only its normal English meaning anywhere else in the text of a card; this rule only applies to an “if” that immediately follows a trigger condition.)
>
> Example: Felidar Sovereign reads, “At the beginning of your upkeep, if you have 40 or more life, you win the game.” Its controller’s life total is checked as that player’s upkeep begins. If that player has 39 or less life, the ability doesn’t trigger at all. If that player has 40 or more life, the ability triggers and goes on the stack. As the ability resolves, that player’s life total is checked again. If that player has 39 or less life at this time, the ability is removed from the stack and has no effect. If that player has 40 or more life at this time, the ability resolves and that player wins the game.
>
> 603.5. Some triggered abilities’ effects are optional (they contain “may,” as in “At the beginning of your upkeep, you may draw a card”). These abilities go on the stack when they trigger, regardless of whether their controller intends to exercise the ability’s option or not. The choice is made when the ability resolves. Likewise, triggered abilities that have an effect “unless” something is true or a player chooses to do something will go on the stack normally; the “unless” part of the ability is dealt with when the ability resolves.
>
> 603.6. Trigger events that involve objects changing zones are called “zone-change triggers.” Many abilities with zone-change triggers attempt to do something to that object after it changes zones. During resolution, these abilities look for the object in the zone that it moved to. If the object is unable to be found in the zone it went to, the part of the ability attempting to do something to the object will fail to do anything. The ability could be unable to find the object because the object never entered the specified zone, because it left the zone before the ability resolved, or because it is in a zone that is hidden from a player, such as a library or an opponent’s hand. (This rule applies even if the object leaves the zone and returns again before the ability resolves.) The most common zone-change triggers are enters-the-battlefield triggers and leaves-the-battlefield triggers.
>
> 603.6a Enters-the-battlefield abilities trigger when a permanent enters the battlefield. These are written, “When [this object] enters, . . . “ or “Whenever a [type] enters, . . .” Each time an event puts one or more permanents onto the battlefield, all permanents on the battlefield (including the newcomers) are checked for any enters-the-battlefield triggers that match the event.
>
> 603.6b Continuous effects that modify characteristics of a permanent do so the moment the permanent is on the battlefield (and not before then). The permanent is never on the battlefield with its unmodified characteristics. Continuous effects don’t apply before the permanent is on the battlefield, however (see rule 603.6d).
>
> Example: If an effect reads “All lands are creatures” and a land card is played, the effect makes the land card into a creature the moment it enters the battlefield, so it would trigger abilities that trigger when a creature enters the battlefield. Conversely, if an effect reads “All creatures lose all abilities” and a creature card with an enters-the-battlefield triggered ability enters the battlefield, that effect will cause it to lose its abilities the moment it enters the battlefield, so the enters-the-battlefield ability won’t trigger.
>
> 603.6c Leaves-the-battlefield abilities trigger when a permanent moves from the battlefield to another zone, or when a phased-in permanent leaves the game because its owner leaves the game. These are written as, but aren’t limited to, “When [this object] leaves the battlefield, . . .” or “Whenever [something] is put into a graveyard from the battlefield, . . . .” (See also rule 603.10.) An ability that attempts to do something to the card that left the battlefield checks for it only in the first zone that it went to. An ability that triggers when a card is put into a certain zone “from anywhere” is never treated as a leaves-the-battlefield ability, even if an object is put into that zone from the battlefield.
>
> 603.6d Some permanents have text that reads “[This permanent] enters with . . . ,” “As [this permanent] enters . . . ,” “[This permanent] enters as . . . ,” or “[This permanent] enters tapped.” Such text is a static ability—not a triggered ability—whose effect occurs as part of the event that puts the permanent onto the battlefield.
>
> 603.6e Some Auras have triggered abilities that trigger on the enchanted permanent leaving the battlefield. These triggered abilities can find the new object that permanent card became in the zone it moved to; they can also find the new object the Aura card became in its owner’s graveyard after state-based actions have been checked. See rule 400.7.
>
> 603.7. An effect may create a delayed triggered ability that can do something at a later time. A delayed triggered ability will contain “when,” “whenever,” or “at,” although that word won’t usually begin the ability.
>
> 603.7a Delayed triggered abilities are created during the resolution of spells or abilities, as the result of a replacement effect being applied, or as a result of a static ability that allows a player to take an action. A delayed triggered ability won’t trigger until it has actually been created, even if its trigger event occurred just beforehand. Other events that happen earlier may make the trigger event impossible.
>
> Example: Part of an effect reads “When this creature leaves the battlefield,” but the creature in question leaves the battlefield before the spell or ability creating the effect resolves. In this case, the delayed ability never triggers.
>
> Example: If an effect reads “When this creature becomes untapped” and the named creature becomes untapped before the effect resolves, the ability waits for the next time that creature untaps.
>
> 603.7b A delayed triggered ability will trigger only once—the next time its trigger event occurs—unless it has a stated duration, such as “this turn.” If its trigger event occurs more than once simultaneously and the ability doesn’t have a stated duration, the controller of the delayed triggered ability chooses which event causes the ability to trigger.
>
> 603.7c A delayed triggered ability that refers to a particular object still affects it even if the object changes characteristics. However, if that object is no longer in the zone it’s expected to be in at the time the delayed triggered ability resolves, the ability won’t affect it. (Note that if that object left that zone and then returned, it’s a new object and thus won’t be affected. See rule 400.7.)
>
> Example: An ability that reads “Exile this creature at the beginning of the next end step” will exile the permanent even if it’s no longer a creature during the next end step. However, it won’t do anything if the permanent left the battlefield before then.
>
> 603.7d If a spell creates a delayed triggered ability, the source of that delayed triggered ability is that spell. The controller of that delayed triggered ability is the player who controlled that spell as it resolved.
>
> 603.7e If an activated or triggered ability creates a delayed triggered ability, the source of that delayed triggered ability is the same as the source of that other ability. The controller of that delayed triggered ability is the player who controlled that other ability as it resolved.
>
> 603.7f If a static ability generates a replacement effect which causes a delayed triggered ability to be created, the source of that delayed triggered ability is the object with that static ability. The controller of that delayed triggered ability is the same as the controller of that object at the time the replacement effect was applied.
>
> 603.7g If a static ability allows a player to take an action and creates a delayed triggered ability if that player does so, the source of that delayed triggered ability is the object with that static ability. The controller of that delayed triggered ability is the same as the controller of that object at the time the action was taken.
>
> 603.7h An activated or triggered ability may create a delayed triggered ability that triggers when the ability that created it has resolved a certain number of times in a turn. In that case, that delayed triggered ability is created only once, during the appropriate resolution of that ability.
>
> 603.10. Normally, objects that exist immediately after an event are checked to see if the event matched any trigger conditions, and continuous effects that exist at that time are used to determine what the trigger conditions are and what the objects involved in the event look like. However, some triggered abilities are exceptions to this rule; the game “looks back in time” to determine if those abilities trigger, using the existence of those abilities and the appearance of objects immediately prior to the event. The list of exceptions is as follows:
>
> 603.10a Some zone-change triggers look back in time. These are leaves-the-battlefield abilities, abilities that trigger when a player sacrifices a permanent, abilities that trigger when a card leaves a graveyard, and abilities that trigger when an object that all players can see is put into a hand or library.
>
> Example: Two creatures are on the battlefield along with an artifact that has the ability “Whenever a creature dies, you gain 1 life.” Someone casts a spell that destroys all artifacts, creatures, and enchantments. The artifact’s ability triggers twice, even though the artifact goes to its owner’s graveyard at the same time as the creatures.
>
> 603.10b Abilities that trigger when a permanent phases out look back in time.
>
> 603.10c Abilities that trigger specifically when an object becomes unattached look back in time.
>
> 603.10d Abilities that trigger when a player loses control of an object or when a player’s opponent gains control of an object from that player look back in time.
>
> 603.10e Abilities that trigger when a spell is countered look back in time.
>
> 603.10f Abilities that trigger when a player loses the game look back in time.
>
> 603.10g Abilities that trigger when a player planeswalks away from a plane look back in time.

## Notes and spec cross-check
- **603.3b is a two-part APNAP process** (flagged in M0; spec §6 omits it): first triggers whose condition isn't "another ability triggering", then the rest; each player in APNAP order orders their own.
- **603.2g:** only events that actually happen trigger anything (post-replacement) → triggers are detected from performed events, after the replacement pipeline (spec §8.2 agrees).
- **603.3c–d:** modes and targets are chosen as the ability is put on the stack; with no legal choice it's simply removed.
- **603.4:** intervening "if" is checked when the trigger event happens AND on resolution.
- **603.6a:** for an event putting permanents onto the battlefield, all permanents on the battlefield *including the newcomers* are checked → ETB triggers are detected after the whole simultaneous batch.
- **603.10a:** leaves-the-battlefield triggers look back in time: the leaving object and its abilities as they existed just before the event.

## Tests
`crates/harness/tests/rules_m2.rs`: `cr_603_*`.

## Steps abilities trigger at; counted conditions (D-057)

> 505.1. There are two main phases in a turn. In each turn, the first main phase (also known as the precombat main phase) and the second main phase (also known as the postcombat main phase) are separated by the combat phase (see rule 506, “Combat Phase”). The precombat and postcombat main phases are individually and collectively known as the main phase.
>
> 507.2. Second, the active player gets priority. (See rule 117, “Timing and Priority.”)
>
> 508.8. If no creatures are declared as attackers or put onto the battlefield attacking, skip the declare blockers and combat damage steps.
>
> 511.1. The end of combat step has no turn-based actions. Once it begins, the active player gets priority. (See rule 117, “Timing and Priority.”)
>
> 511.2. Abilities that trigger “at end of combat” trigger as the end of combat step begins. Effects that last “until end of combat” expire at the end of the combat phase.
>
> 513.1. The end step has no turn-based actions. Once it begins, the active player gets priority. (See rule 117, “Timing and Priority.”)
>
> 513.1a Previously, abilities that triggered at the beginning of the end step were printed with the trigger condition “at end of turn.” Cards that were printed with that text have received errata in the Oracle card reference to say “at the beginning of the end step” or “at the beginning of the next end step.”
>
> 601.2i Once the steps described in 601.2a–h are completed, effects that modify the characteristics of the spell as it’s cast are applied, then the spell becomes cast. Any abilities that trigger when a spell is cast or put onto the stack trigger at this time. If the spell’s controller had priority before casting it, they get priority.

- `TriggerStep` has the beginning of combat step, the end of combat step ("at end of
  combat", 511.2), the precombat and postcombat main phases, besides upkeep, draw step and
  end step. Forge writes the end step `End of Turn` (513.1a).
- "Leaves the battlefield" (`Trigger::Leaves`, 603.6c) is found the way "dies" is: among
  the permanents there now and everything that left in the same batch, as it last
  existed (603.10a).
- "Whenever [a player] casts a [noncreature / Spirit / blue / mana value 4 or greater]
  spell" compares the spell as it is on the stack when it becomes cast (601.2i).
- `Condition::Compare` — "if [something counted] is N or more / N or less / exactly N" —
  is an intervening "if" on a triggered ability (603.4: asked when the event happens and
  again on resolution) and "as long as" on a static ability, where it is counted inside
  the layer walk and so may only count what the walk can (card-db validation).

| Rule | Test (`crates/harness/tests/forge_features.rs`) |
|---|---|
| 603.2b, 505.1, 507.2, 511.2, 513.1a, 508.8 | `step_triggers_at_main_phase_combat_and_end_step` |
| 603.6c, 603.10a | `leaves_the_battlefield_triggers_for_any_zone` |
| 601.2i | `cast_triggers_ask_what_the_spell_is` |
| 603.4 | `conditions_that_compare_a_count` |

## Damage triggers; what an event carries (D-058)

> 120.1. Objects can deal damage to battles, creatures, planeswalkers, and players. This is generally detrimental to the object or player that receives that damage. An object that deals damage is the source of that damage.
>
> 120.2a Damage may be dealt as a result of combat. Each attacking and blocking creature deals combat damage equal to its power during the combat damage step.
>
> 120.2b Damage may be dealt as an effect of a spell or ability. The spell or ability will specify which object deals that damage.
>
> 120.9. If an ability triggers on damage being dealt by a specific source or sources, and the effect refers to the “damage dealt,” it refers only to the damage dealt by the specified sources and not to any damage dealt at the same time by other sources.
>
> 510.2. Second, all combat damage that’s been assigned is dealt simultaneously. This turn-based action doesn’t use the stack. No player has the chance to cast spells or activate abilities between the time combat damage is assigned and the time it’s dealt.
>
> 510.3a Any abilities that triggered on damage being dealt or while state-based actions are performed afterward are put onto the stack before the active player gets priority; the order in which they triggered doesn’t matter. (See rule 603, “Handling Triggered Abilities.”)
>
> 603.2c An ability triggers only once each time its trigger event occurs. However, it can trigger repeatedly if one event contains multiple occurrences.
>
> 603.2g An ability triggers only if its trigger event actually occurs. An event that’s prevented or replaced won’t trigger anything.

- `Trigger::Damage { by, to, combat, per }` is found from the `DamageDealt` events of one
  batch — damage dealt at the same time (510.2). `per` says how that counts: once for each
  source and recipient ("whenever this creature deals damage to a player"), once for each
  recipient ("whenever this creature is dealt damage", "whenever one or more creatures you
  control deal combat damage to a player"), or once for each source ("whenever this
  creature deals damage"). The amount an ability refers to is the damage of the sources
  it asks for (120.9). Prevented damage is no event (603.2g).
- The ability carries `EventFacts { player, amount }`: "that player" is the player dealt
  damage (`PlayerRef::TriggeringPlayer`), "that much" the amount
  (`Amount::TriggeringAmount`). "It" (`ObjRef::Triggering`) is the permanent dealt damage;
  for damage to a player, the source of the damage.
- `ObjFilter::Host`: "enchanted creature"/"equipped creature" as what a trigger is about.
  When the creature dies the Aura is still attached to the object that left (state-based
  actions come later), which is how "when enchanted creature dies" finds it (603.10a).

| Rule | Test (`crates/harness/tests/forge_features.rs`) |
|---|---|
| 120.2, 510.3a | `deals_damage_triggers_by_recipient_and_kind_of_damage` |
| 510.2, 120.9, 603.2c | `simultaneous_damage_triggers_once_where_the_card_says_so` |
| 303.4, 603.10a | `triggers_about_the_enchanted_creature` |

## Attacking, blocking, life, draws, discards, sacrifices as events (D-061)

> 508.1m Any abilities that trigger on attackers being declared trigger.
>
> 509.1h An attacking creature with one or more creatures declared as blockers for it becomes a blocked creature; one with no creatures declared as blockers for it becomes an unblocked creature. This remains unchanged until the creature is removed from combat, an effect says that it becomes blocked or unblocked, or the combat phase ends, whichever comes first. A creature remains blocked even if all the creatures blocking it are removed from combat.
>
> 509.3a An ability that reads “Whenever [a creature] blocks, . . .” generally triggers only once each combat for that creature, even if it blocks multiple creatures. It triggers if the creature is declared as a blocker. It will also trigger if that creature becomes a blocker as the result of an effect, but only if it wasn’t a blocking creature at that time. (See rule 509.1g.) It won’t trigger if the creature is put onto the battlefield blocking.
>
> 509.3b An ability that reads “Whenever [a creature] blocks a creature, . . .” triggers once for each attacking creature the creature with the ability blocks. It triggers if the creature is declared as a blocker. It will also trigger if an effect causes that creature to block an attacking creature, but only if it wasn’t already blocking that attacking creature at that time. It won’t trigger if the creature is put onto the battlefield blocking.
>
> 509.3c An ability that reads “Whenever [a creature] becomes blocked, . . .” generally triggers only once each combat for that creature, even if it’s blocked by multiple creatures. It will trigger if that creature becomes blocked by at least one creature declared as a blocker. It will also trigger if that creature becomes blocked by an effect or by a creature that’s put onto the battlefield as a blocker, but only if the attacking creature was an unblocked creature at that time. (See rule 509.1h.)
>
> 509.3d An ability that reads “Whenever [a creature] becomes blocked by a creature, . . .” triggers once for each creature that blocks the specified creature. It triggers if a creature is declared as a blocker for the attacking creature. It will also trigger if an effect causes a creature to block the attacking creature, but only if it wasn’t already blocking that attacking creature at that time. In addition, it will trigger if a creature is put onto the battlefield blocking that creature. It won’t trigger if the creature becomes blocked by an effect rather than a creature.
>
> 509.3g An ability that reads “Whenever [a creature] attacks and isn’t blocked, . . .” triggers if no creatures are declared as blockers for that creature. It will trigger even if the creature was never declared as an attacker (for example, if it entered the battlefield attacking). It won’t trigger if the attacking creature is blocked and then all its blockers are removed from combat.
>
> 119.9. Some triggered abilities are written, “Whenever [a player] gains life, . . . .” Such abilities are treated as though they are written, “Whenever a source causes [a player] to gain life, . . . .” If a player gains 0 life, no life gain event has occurred, and these abilities won’t trigger.
>
> 121.1. A player draws a card by putting the top card of their library into their hand. This is done as a turn-based action during each player’s draw step. It may also be done as part of a cost or effect of a spell or ability.
>
> 701.21a To sacrifice a permanent, its controller moves it from the battlefield directly to its owner’s graveyard. A player can’t sacrifice something that isn’t a permanent, or something that’s a permanent they don’t control. Sacrificing a permanent doesn’t destroy it, so regeneration or other effects that replace destruction can’t affect this action.
>
> 702.45a Bushido is a triggered ability. “Bushido N” means “Whenever this creature blocks or becomes blocked, it gets +N/+N until end of turn.” (See rule 509, “Declare Blockers Step.”)
>
> 702.25a Flanking is a triggered ability that triggers during the declare blockers step. (See rule 509, “Declare Blockers Step.”) “Flanking” means “Whenever this creature becomes blocked by a creature without flanking, the blocking creature gets -1/-1 until end of turn.”

- `Trigger::PlayerAttacks` is found once for a declaration of attackers (508.1m);
  `Blocks`, `BecomesBlocked`, `Unblocked` and `BlockedBy` when the last defending player
  has declared blockers (509.1h): once for a creature that blocks (509.3a) or becomes
  blocked (509.3c), once for each pair for "blocks a creature" / "becomes blocked by a
  creature" (509.3b, 509.3d), and for each attacker left without a blocker (509.3g).
  Creatures put onto the battlefield blocking, and blockers added by effects, are not
  events here: the engine has no such effects yet.
- `GainsLife`/`LosesLife`, `Draws`, `Discards`, `Sacrifices` are found from the events of
  an Apply batch. A draw knows which card of the turn it was
  (`PlayerState::drawn_this_turn`). A sacrificed permanent is matched as it last existed.
- Bushido and flanking compile to their triggered abilities (702.45a, 702.25a). Flanking
  is also a keyword on the card — "a creature without flanking" needs to know who has it —
  but no script effect can grant that keyword, because the ability would not come with it.

| Rule | Test (`crates/harness/tests/forge_features.rs`) |
|---|---|
| 508.1m | `whenever_you_attack` |
| 119.9, 121.1, 701.9, 701.21 | `life_draw_discard_and_sacrifice_triggers` |
| 509.3a–d, 509.3g, 702.45a, 702.25a | `blocking_triggers` |
