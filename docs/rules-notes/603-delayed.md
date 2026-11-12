# CR 603.7 — delayed triggered abilities that remember objects (D-081)

- **Source:** Comprehensive Rules effective **November 6, 2026** (local `docs/rules/MagicCompRules.txt`, not committed).
- Builds on `603-triggered-abilities.md`, D-075 (delayed triggers of a step, remembering a player or an amount) and D-069 ("those cards").

## Verbatim text

> 603.7. An effect may create a delayed triggered ability that can do something at a later time. A delayed triggered ability will contain “when,” “whenever,” or “at,” although that word won’t usually begin the ability.

> 603.7a Delayed triggered abilities are created during the resolution of spells or abilities, as the result of a replacement effect being applied, or as a result of a static ability that allows a player to take an action. A delayed triggered ability won’t trigger until it has actually been created, even if its trigger event occurred just beforehand. Other events that happen earlier may make the trigger event impossible.

> 603.7b A delayed triggered ability will trigger only once—the next time its trigger event occurs—unless it has a stated duration, such as “this turn.” If its trigger event occurs more than once simultaneously and the ability doesn’t have a stated duration, the controller of the delayed triggered ability chooses which event causes the ability to trigger.

> 603.7c A delayed triggered ability that refers to a particular object still affects it even if the object changes characteristics. However, if that object is no longer in the zone it’s expected to be in at the time the delayed triggered ability resolves, the ability won’t affect it. (Note that if that object left that zone and then returned, it’s a new object and thus won’t be affected. See rule 400.7.)

> 603.7d If a spell creates a delayed triggered ability, the source of that delayed triggered ability is that spell. The controller of that delayed triggered ability is the player who controlled that spell as it resolved.

> 603.7e If an activated or triggered ability creates a delayed triggered ability, the source of that delayed triggered ability is the same as the source of that other ability. The controller of that delayed triggered ability is the player who controlled that other ability as it resolved.

> 400.7. An object that moves from one zone to another becomes a new object with no memory of, or relation to, its previous existence. This rule has the following exceptions.

> 400.7j If an effect causes an object to move to a public zone, other parts of that effect can find that object. If the cost of a spell or ability causes an object to move to a public zone, that spell or ability’s effects can find that object.

> 800.4a When a player leaves the game, all objects (see rule 109) owned by that player leave the game and any effects which give that player control of any objects or players end. Then, if that player controlled any objects on the stack not represented by cards, those objects cease to exist. Then, if there are any objects still controlled by that player, those objects are exiled. This is not a state-based action. It happens as soon as the player leaves the game. If the player who left the game had priority at the time they left, priority passes to the next player in turn order who’s still in the game.

> 303.4f If an Aura is entering the battlefield under a player’s control by any means other than by resolving as an Aura spell, and the effect putting it onto the battlefield doesn’t specify the object or player the Aura will enchant, that player chooses what it will enchant as the Aura enters the battlefield. The player must choose a legal object or player according to the Aura’s enchant ability and any other applicable effects.

> 303.4g If an Aura is entering the battlefield and there is no legal object or player for it to enchant, the Aura remains in its current zone, unless that zone is the stack. In that case, the Aura is put into its owner’s graveyard instead of entering the battlefield. If the Aura is a token, it isn’t created.

> 301.5c An Equipment that’s also a creature can’t equip a creature unless that Equipment has reconfigure (see rule 702.151, “Reconfigure”). An Equipment that loses the subtype “Equipment” can’t equip a creature. An Equipment can’t equip itself. An Equipment that equips an illegal or nonexistent permanent becomes unattached from that permanent but remains on the battlefield. (This is a state-based action. See rule 704.) An Equipment can’t equip more than one creature. If a spell or ability would cause an Equipment to equip more than one creature, the Equipment’s controller chooses which creature it equips.

## How the engine reads these

- 603.7c: `effects::Delayed::objects` holds the objects the delayed ability is about, fixed as it is created (from the chain's "those cards", or the card a dying creature became). It lives in `GameState::delayed` (serialized) and travels with the triggered ability (`PendingTrigger::remembered`, `StackObject::remembered`). As it resolves they are its "those cards" — each only while it is still the same object (a zone change makes a new object, CR 400.7, and the remembered one is gone).
- 603.7b: it triggers once and is removed. 603.7d: its controller is the controller of the spell or ability that made it. 800.4a: one whose controller has left the game never triggers.
- 303.4f–g (D-074 follow-up): a permanent entering as a copy of an Aura (or a token copy of one) is attached as it enters to what its controller chooses (not targeted); with nothing it could enchant it stays where it was (from the stack: its owner's graveyard; a token isn't created).
