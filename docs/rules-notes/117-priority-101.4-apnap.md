# CR 117 — Timing and priority; CR 101.4 — APNAP

- **Source:** Comprehensive Rules effective **November 6, 2026** (local `docs/rules/MagicCompRules.txt`, not committed).
- **Milestone:** M1

## Verbatim text

> 101.4. If multiple players would make choices and/or take actions at the same time, the active player (the player whose turn it is) makes any choices required, then the next player in turn order (usually the player seated to the active player’s left) makes any choices required, followed by the remaining nonactive players in turn order. Then the actions happen simultaneously. This rule is often referred to as the “Active Player, Nonactive Player (APNAP) order” rule.
>
> Example: A card reads “Each player sacrifices a creature.” First, the active player chooses a creature they control. Then each of the nonactive players, in turn order, chooses a creature they control. Then all creatures chosen this way are sacrificed simultaneously.
>
> 101.4a If an effect has each player choose a card in a hidden zone, such as their hand or library, those cards may remain face down as they’re chosen. However, each player must clearly indicate which face-down card they are choosing.
>
> 101.4b A player knows the choices made by the previous players when making their choice, except as specified in 101.4a.
>
> 101.4c If a player would make more than one choice at the same time, the player makes the choices in the order specified. If no order is specified, the player chooses the order.
>
> 101.4d If a choice made by a nonactive player causes the active player, or a different nonactive player earlier in the turn order, to have to make a choice, APNAP order is restarted for all outstanding choices.
>
> 101.4e If multiple players would make choices or take actions while starting the game, the starting player is considered the active player and each other player is considered a nonactive player.
>
> 102. Players
>
> 117.1. Unless a spell or ability is instructing a player to take an action, which player can take actions at any given time is determined by a system of priority. The player with priority may cast spells, activate abilities, and take special actions.
>
> 117.1a A player may cast an instant spell any time they have priority. A player may cast a noninstant spell during their main phase any time they have priority and the stack is empty.
>
> 117.1b A player may activate an activated ability any time they have priority.
>
> 117.1c A player may take some special actions any time they have priority. A player may take other special actions during their main phase any time they have priority and the stack is empty. See rule 116, “Special Actions.”
>
> 117.1d A player may activate a mana ability whenever they have priority, whenever they are casting a spell or activating an ability that requires a mana payment, or whenever a rule or effect asks for a mana payment (even in the middle of casting or resolving a spell or activating or resolving an ability).
>
> 117.2. Other kinds of abilities and actions are automatically generated or performed by the game rules, or are performed by players without receiving priority.
>
> 117.2a Triggered abilities can trigger at any time, including while a spell is being cast, an ability is being activated, or a spell or ability is resolving. (See rule 603, “Handling Triggered Abilities.”) However, nothing actually happens at the time an ability triggers. Each time a player would receive priority, each ability that has triggered but hasn’t yet been put on the stack is put on the stack. See rule 117.5.
>
> 117.2b Static abilities continuously affect the game. Priority doesn’t apply to them. (See rule 604, “Handling Static Abilities,” and rule 611, “Continuous Effects.”)
>
> 117.2c Turn-based actions happen automatically when certain steps or phases begin. They’re dealt with before a player would receive priority. See rule 117.3a. Turn-based actions also happen automatically when each step and phase ends; no player receives priority afterward. See rule 703, “Turn-Based Actions.”
>
> 117.2d State-based actions happen automatically when certain conditions are met. See rule 704. They’re dealt with before a player would receive priority. See rule 117.5.
>
> 117.2e Resolving spells and abilities may instruct players to make choices or take actions, or may allow players to activate mana abilities. Even if a player is doing so, no player has priority while a spell or ability is resolving. See rule 608, “Resolving Spells and Abilities.”
>
> 117.3. Which player has priority is determined by the following rules:
>
> 117.3a The active player receives priority at the beginning of most steps and phases, after any turn-based actions (such as drawing a card during the draw step; see rule 703) have been dealt with and abilities that trigger at the beginning of that phase or step have been put on the stack. No player receives priority during the untap step. Players usually don’t get priority during the cleanup step (see rule 514.3).
>
> 117.3b The active player receives priority after a spell or ability (other than a mana ability) resolves.
>
> 117.3c If a player has priority when they cast a spell, activate an ability, or take a special action, that player receives priority afterward.
>
> 117.3d If a player has priority and chooses not to take any actions, that player passes. If any mana is in that player’s mana pool, they announce what mana is there. Then the next player in turn order receives priority.
>
> 117.4. If all players pass in succession (that is, if all players pass without taking any actions in between passing), the spell or ability on top of the stack resolves or, if the stack is empty, the phase or step ends.
>
> 117.5. Each time a player would get priority, the game first performs all applicable state-based actions as a single event (see rule 704, “State-Based Actions”), then repeats this process until no state-based actions are performed. Then triggered abilities are put on the stack (see rule 603, “Handling Triggered Abilities”). These steps repeat in order until no further state-based actions are performed and no abilities trigger. Then the player who would have received priority does so.
>
> 117.7. If a player with priority casts a spell or activates an activated ability while another spell or ability is already on the stack, the new spell or ability has been cast or activated “in response to” the earlier spell or ability. The new spell or ability will resolve first. See rule 608, “Resolving Spells and Abilities.”
>
> 118. Costs

## Notes and spec cross-check
- Spec §5–6 quotes 101.4, 117.4 and 117.5 accurately (verified).
- **117.1d / 605.3a:** mana abilities can be activated whenever a player has priority, while casting/activating when a mana payment is required, or when a rule or effect asks for a mana payment. M1 offers them in priority (floating mana) and during cost payment.
- **117.3b:** after a spell or ability *other than a mana ability* resolves, the active player receives priority. With 800.4j, "the active player" may have left: priority goes to the next player in turn order.
- **117.3c:** after casting, activating, or a special action, the same player receives priority again.
- **117.4:** "pass in succession" counts only players still in the game.
- **101.4b–d:** later players know earlier choices; 101.4d (a NAP's choice forcing an earlier player to choose restarts APNAP) has no M1 card that can trigger it.

## Tests
`crates/harness/tests/rules_m1.rs`: `cr_117_3b_*`, `cr_117_3c_*`, `cr_117_4_*`, `cr_117_5_*`, `cr_101_4_*`.
