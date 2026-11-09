# CR 506–511 combat and combat keywords (702.2, 702.4, 702.7, 702.15, 702.19, 702.20, 702.111)

- **Source:** Comprehensive Rules effective **November 6, 2026** (local `docs/rules/MagicCompRules.txt`, not committed).
- **Milestone:** M3

## Verbatim text

> 508.1f The active player taps the chosen creatures. Tapping a creature when it’s declared as an attacker isn’t a cost; attacking simply causes creatures to become tapped.
>
> 509.1b The defending player checks each creature they control to see whether it’s affected by any restrictions (effects that say a creature can’t block, or that it can’t block unless some condition is met). If any restrictions are being disobeyed, the declaration of blockers is illegal.
>
>      A restriction may be created by an evasion ability (a static ability an attacking creature has that restricts what can block it). If an attacking creature gains or loses an evasion ability after a legal block has been declared, it doesn’t affect that block. Different evasion abilities are cumulative.
>
> Example: An attacking creature with flying and shadow can’t be blocked by a creature with flying but without shadow.
>
> 510.1. First, the active player announces how each attacking creature assigns its combat damage, then the defending player announces how each blocking creature assigns its combat damage. This turn-based action doesn’t use the stack. A player assigns a creature’s combat damage according to the following rules:
>
> 510.1a Each attacking creature and each blocking creature assigns combat damage equal to its power. Creatures that would assign 0 or less damage this way don’t assign combat damage at all.
>
> 510.1b An unblocked creature assigns its combat damage to the player, planeswalker, or battle it’s attacking. If it isn’t currently attacking anything (if, for example, it was attacking a planeswalker that has left the battlefield), it assigns no combat damage.
>
> 510.1c A blocked creature assigns its combat damage to the creatures blocking it. If no creatures are currently blocking it (if, for example, they were destroyed or removed from combat), it assigns no combat damage. If exactly one creature is blocking it, it assigns all its combat damage to that creature. If two or more creatures are blocking it, it assigns its combat damage to those creatures divided as its controller chooses among them.
>
> Example: An attacking Elvish Regrower (a 4/3 creature) is blocked by Vampire Spawn (a 2/3 creature) and Helpful Hunter (a 1/1 creature). Elvish Regrower’s controller can assign all 4 damage to the Hunter, 1 damage to the Spawn and 3 damage to the Hunter, 2 damage to each creature, 3 damage to the Spawn and 1 damage to the Hunter, or all 4 damage to the Spawn.
>
> 510.1d A blocking creature assigns combat damage to the creatures it’s blocking. If it isn’t currently blocking any creatures (if, for example, they were destroyed or removed from combat), it assigns no combat damage. If it’s blocking exactly one creature, it assigns all its combat damage to that creature. If it’s blocking two or more creatures, it assigns its combat damage divided as its controller chooses among them.
>
> 510.1e Once a player has assigned combat damage from each attacking or blocking creature they control, the total damage assignment (not solely the damage assignment of any individual attacking or blocking creature) is checked to see if it complies with the above rules. If it doesn’t, the combat damage assignment is illegal; the game returns to the moment before that player began to assign combat damage. (See rule 734, “Handling Illegal Actions.”)
>
> 510.2. Second, all combat damage that’s been assigned is dealt simultaneously. This turn-based action doesn’t use the stack. No player has the chance to cast spells or activate abilities between the time combat damage is assigned and the time it’s dealt.
>
> 510.4. If at least one attacking or blocking creature has first strike (see rule 702.7) or double strike (see rule 702.4) as the combat damage step begins, the only creatures that assign combat damage in that step are those with first strike or double strike. After that step, instead of proceeding to the end of combat step, the phase gets a second combat damage step. The only creatures that assign combat damage in that step are the remaining attackers and blockers that had neither first strike nor double strike as the first combat damage step began, as well as the remaining attackers and blockers that currently have double strike. After that step, the phase proceeds to the end of combat step.
>
> 511. End of Combat Step
>
> 702.2b A creature with toughness greater than 0 that’s been dealt damage by a source with deathtouch since the last time state-based actions were checked is destroyed as a state-based action. See rule 704.
>
> 702.2c Any nonzero amount of combat damage assigned to a creature by a source with deathtouch is considered to be lethal damage for the purposes of determining if excess damage is being dealt.
>
> 702.4b If at least one attacking or blocking creature has first strike (see rule 702.7) or double strike as the combat damage step begins, the only creatures that assign combat damage in that step are those with first strike or double strike. After that step, instead of proceeding to the end of combat step, the phase gets a second combat damage step. The only creatures that assign combat damage in that step are the remaining attackers and blockers that had neither first strike nor double strike as the first combat damage step began, as well as the remaining attackers and blockers that currently have double strike. After that step, the phase proceeds to the end of combat step.
>
> 702.4c Removing double strike from a creature during the first combat damage step will stop it from assigning combat damage in the second combat damage step.
>
> 702.7b If at least one attacking or blocking creature has first strike or double strike (see rule 702.4) as the combat damage step begins, the only creatures that assign combat damage in that step are those with first strike or double strike. After that step, instead of proceeding to the end of combat step, the phase gets a second combat damage step. The only creatures that assign combat damage in that step are the remaining attackers and blockers that had neither first strike nor double strike as the first combat damage step began, as well as the remaining attackers and blockers that currently have double strike. After that step, the phase proceeds to the end of combat step.
>
> 702.7c Giving first strike to a creature without it after combat damage has already been dealt in the first combat damage step won’t preclude that creature from assigning combat damage in the second combat damage step. Removing first strike from a creature after it has already dealt combat damage in the first combat damage step won’t allow it to also assign combat damage in the second combat damage step (unless the creature has double strike).
>
> 702.15b Damage dealt by a source with lifelink causes that source’s controller, or its owner if it has no controller, to gain that much life (in addition to any other results that damage causes). See rule 120.3.
>
> 702.19b The controller of an attacking creature with trample first assigns damage to the creature(s) blocking it. Once all those blocking creatures are assigned lethal damage, any excess damage is assigned as its controller chooses among those blocking creatures and the player, planeswalker, or battle the creature is attacking. When checking for assigned lethal damage, take into account damage already marked on the creature and damage from other creatures that’s being assigned during the same combat damage step, but not any abilities or effects that might change the amount of damage that’s actually dealt. The attacking creature’s controller need not assign lethal damage to all those blocking creatures but in that case can’t assign any damage to the player or planeswalker it’s attacking.
>
> Example: A 2/2 creature that can block an additional creature blocks two attackers: a 1/1 with no abilities and a 3/3 with trample. The active player could assign 1 damage from the first attacker and 1 damage from the second to the blocking creature, and 2 damage to the defending player from the creature with trample.
>
> Example: A 6/6 green creature with trample is blocked by a 2/2 creature with protection from green. The attacking creature’s controller must assign at least 2 damage to the blocker, even though that damage will be prevented by the blocker’s protection ability. The attacking creature’s controller can divide the rest of the damage as they choose between the blocking creature and the defending player.
>
> 702.19c Trample over planeswalkers is a variant of trample that modifies the rules for assigning combat damage to planeswalkers. The controller of a creature with trample over planeswalkers assigns that creature’s combat damage as described in rule 702.19b, with one exception. If that creature is attacking a planeswalker, after lethal damage is assigned to all blocking creatures and damage at least equal to the loyalty of the planeswalker the creature is attacking is assigned to that planeswalker, further excess damage may be assigned as the attacking creature’s controller chooses among those blocking creatures, that planeswalker, and that planeswalker’s controller. When checking for assigned damage equal to a planeswalker’s loyalty, take into account damage from other creatures that’s being assigned during the same combat damage step, but not any abilities or effects that might change the amount of damage that’s actually dealt.
>
> Example: A player controls a planeswalker with three loyalty counters that is being attacked by a 1/1 with no abilities and a 7/7 with trample over planeswalkers. The active player could assign 1 damage from the first attacker and 2 damage from the second to the planeswalker and 5 damage to the defending player from the creature with trample over planeswalkers.
>
> 702.19d If an attacking creature with trample or trample over planeswalkers is blocked, but there are no creatures blocking it when damage is assigned, its damage is assigned to the defending player and/or planeswalker as though all blocking creatures have been assigned lethal damage.
>
> 702.20b Attacking doesn’t cause creatures with vigilance to tap. (See rule 508, “Declare Attackers Step.”)
>
> 702.111b A creature with menace can’t be blocked except by two or more creatures. (See rule 509, “Declare Blockers Step.”)
>
> 802.4. If more than one player is being attacked, controls a planeswalker that’s being attacked, or protects a battle that’s being attacked, each defending player in APNAP order declares blockers as the declare blockers step begins. (See rule 101.4 and rule 509, “Declare Blockers Step.”) The first defending player declares all their blocks, then the second defending player, and so on.
>
> 802.4a A defending player can block only with creatures they control. Those creatures can block only creatures attacking that player, a planeswalker that player controls, or a battle that player protects.
>
> 802.4b When determining whether a defending player’s blocks are legal, ignore any creatures attacking other players and any blocking creatures controlled by other players.
>
> 802.5. Combat damage is assigned in APNAP order. Other than that, the combat damage step proceeds just as in a two-player game. See rule 510, “Combat Damage Step.”
>
> 803. Attack Left and Attack Right Options

## Notes and spec cross-check
- Spec §8.5 says to read the current 510.1 text and not rely on "damage assignment order": confirmed — **510.1c**: a creature blocked by two or more creatures divides its damage "as its controller chooses among them" (no ordering), already implemented as `Allocate` (D-016).
- **Trample (702.19b):** lethal damage must be assigned to *all* blockers before any can go to the player; "lethal" accounts for damage already marked and damage assigned by other creatures in the same step; with deathtouch any nonzero amount counts as lethal (702.2c). 702.19d: if all its blockers are gone, a blocked trampler assigns all its damage to the player.
- **First/double strike (510.4):** a second combat damage step exists only if a creature with first or double strike is attacking or blocking as the first combat damage step begins; the second step uses creatures that had neither as the first step began, plus those that *currently* have double strike.
- **Menace (702.111b) and 509.1b:** restrictions are checked on the whole declaration; an illegal declaration returns to the moment before it (CR 734). apnap restarts that defender's declaration (nothing has happened yet: blocks are only recorded at the end).
- **Vigilance (702.20b):** attacking doesn't tap it. **Lifelink (702.15b):** its controller gains life equal to the damage dealt, as part of the damage event.

## Tests
`crates/harness/tests/rules_m3.rs`: `cr_510_4_*`, `cr_702_*`.
