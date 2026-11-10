# CR 601 / 602 / 605 — Casting spells, activating abilities, mana abilities (with 107.3 X, 700.2 modes, 115 targets)

- **Source:** Comprehensive Rules effective **November 6, 2026** (local `docs/rules/MagicCompRules.txt`, not committed).
- **Milestone:** M1

## Verbatim text

> 601.2. To cast a spell is to take it from where it is (usually the hand), put it on the stack, and pay its costs, so that it will eventually resolve and have its effect. Casting a spell includes proposal of the spell (rules 601.2a–d) and determination and payment of costs (rules 601.2f–h). To cast a spell, a player follows the steps listed below, in order. A player must be legally allowed to cast the spell to begin this process (see rule 601.3). If a player is unable to comply with the requirements of a step listed below while performing that step, the casting of the spell is illegal; the game returns to the moment before the casting of that spell was proposed (see rule 734, “Handling Illegal Actions”).
>
> 601.2a To propose the casting of a spell, a player first moves that card (or that copy of a card) from where it is to the stack. It becomes the topmost object on the stack. It has all the characteristics of the card (or the copy of a card) associated with it, and that player becomes its controller. Any continuous effects that modify the characteristics of the spell as you start casting it begin as it is put on the stack (see rule 611.2f). Any one-shot effects that cause the spell to gain abilities as you cast it apply as it is put on the stack (see rule 610.5). The spell remains on the stack until it resolves, it’s countered, or a rule or effect moves it elsewhere.
>
> 601.2b If the spell is modal, the player announces the mode choice (see rule 700.2). If the player wishes to splice any cards onto the spell (see rule 702.47), they reveal those cards in their hand. If the spell has alternative or additional costs that will be paid as it’s being cast such as buyback or kicker costs (see rules 118.8 and 118.9), the player announces their intentions to pay any or all of those costs (see rule 601.2f). A player can’t apply two alternative methods of casting or two alternative costs to a single spell. If the spell has a variable cost that will be paid as it’s being cast (such as an {X} in its mana cost; see rule 107.3), the player announces the value of that variable. If the value of that variable is defined in the text of the spell by a choice that player would make later in the announcement or resolution of the spell, that player makes that choice at this time instead of that later time. If a cost that will be paid as the spell is being cast includes hybrid mana symbols, the player announces the nonhybrid equivalent cost they intend to pay. If a cost that will be paid as the spell is being cast includes Phyrexian mana symbols, the player announces whether they intend to pay 2 life or a corresponding colored mana cost for each of those symbols. Previously made choices (such as choosing to cast a spell with flashback from a graveyard or choosing to cast a creature with morph face down) may restrict the player’s options when making these choices.
>
> 601.2c The player announces their choice of an appropriate object or player for each target the spell requires. A spell may require some targets only if an alternative or additional cost (such as a kicker cost) or a particular mode was chosen for it; otherwise, the spell is cast as though it did not require those targets. Similarly, a spell may require alternative targets only if an alternative or additional cost was chosen for it. If the spell has a variable number of targets, the player announces how many targets they will choose before they announce those targets. In some cases, the number of targets will be defined by the spell’s text. Once the number of targets the spell has is determined, that number doesn’t change, even if the information used to determine the number of targets does. The same target can’t be chosen multiple times for any one instance of the word “target” on the spell. However, if the spell uses the word “target” in multiple places, the same object or player can be chosen once for each instance of the word “target” (as long as it fits the targeting criteria). If any effects say that an object or player must be chosen as a target, the player chooses targets so that they obey the maximum possible number of such effects without violating any rules or effects that say that an object or player can’t be chosen as a target. The chosen objects and/or players each become a target of that spell. (Any abilities that trigger when those objects and/or players become the target of a spell trigger at this point; they’ll wait to be put on the stack until the spell has finished being cast.)
>
> Example: If a spell says “Tap two target creatures,” then the same creature can’t be chosen twice; the spell requires two different legal targets. A spell that says “Destroy target artifact and target land,” however, can target the same artifact land twice because it uses the word “target” in multiple places.
>
> 601.2d If the spell requires the player to divide or distribute an effect (such as damage or counters) among one or more targets, the player announces the division. Each of these targets must receive at least one of whatever is being divided.
>
> 601.2e The game checks to see if the proposed spell can legally be cast. If the proposed spell is illegal, the game returns to the moment before the casting of that spell was proposed (see rule 734, “Handling Illegal Actions”).
>
> 601.2f The player determines the total cost of the spell. Usually this is just the mana cost. Some spells have additional or alternative costs. Some effects may increase or reduce the cost to pay, or may provide other alternative costs. Costs may include paying mana, tapping permanents, sacrificing permanents, discarding cards, and so on. The total cost is the mana cost or alternative cost (as determined in rule 601.2b), plus all additional costs and cost increases, and minus all cost reductions. If multiple cost reductions apply, the player may apply them in any order. If the mana component of the total cost is reduced to nothing by cost reduction effects, it is considered to be {0}. It can’t be reduced to less than {0}. Once the total cost is determined, any effects that directly affect the total cost are applied. Then the resulting total cost becomes “locked in.” If effects would change the total cost after this time, they have no effect.
>
> 601.2g If the total cost includes a mana payment, the player then has a chance to activate mana abilities (see rule 605, “Mana Abilities”). Mana abilities must be activated before costs are paid.
>
> 601.2h The player pays the total cost. First, they pay all costs that don’t involve random elements or moving objects from the library to a public zone, in any order. Then they pay all remaining costs in any order. Partial payments are not allowed. Unpayable costs can’t be paid.
>
> Example: You cast Altar’s Reap, which costs {1}{B} and has an additional cost of sacrificing a creature. You sacrifice Thunderscape Familiar, whose effect makes your black spells cost {1} less to cast. Because a spell’s total cost is “locked in” before payments are actually made, you pay {B}, not {1}{B}, even though you’re sacrificing the Familiar.
>
> 601.2i Once the steps described in 601.2a–h are completed, effects that modify the characteristics of the spell as it’s cast are applied, then the spell becomes cast. Any abilities that trigger when a spell is cast or put onto the stack trigger at this time. If the spell’s controller had priority before casting it, they get priority.
>
> 602.2. To activate an ability is to put it onto the stack and pay its costs, so that it will eventually resolve and have its effect. Only an object’s controller (or its owner, if it doesn’t have a controller) can activate its activated ability unless the object specifically says otherwise. Activating an ability follows the steps listed below, in order. If, at any point during the activation of an ability, a player is unable to comply with any of those steps, the activation is illegal; the game returns to the moment before that ability started to be activated (see rule 734, “Handling Illegal Actions”). Announcements and payments can’t be altered after they’ve been made.
>
> 602.2a The player announces that they are activating the ability. If an activated ability is being activated from a hidden zone, the card that has that ability is revealed (see rule 701.20a). That ability is created on the stack as an object that’s not a card. It becomes the topmost object on the stack. It has the text of the ability that created it, and no other characteristics. Its controller is the player who activated the ability. The ability remains on the stack until it’s countered, it resolves, or an effect moves it elsewhere.
>
> 602.2b The remainder of the process for activating an ability is identical to the process for casting a spell listed in rules 601.2b–i. Those rules apply to activating an ability just as they apply to casting a spell. An activated ability’s analog to a spell’s mana cost (as referenced in rule 601.2f) is its activation cost.
>
> 602.5. A player can’t begin to activate an ability that’s prohibited from being activated.
>
> 602.5a A creature’s activated ability with the tap symbol ({T}) or the untap symbol ({Q}) in its activation cost can’t be activated unless the creature has been under its controller’s control since the start of their most recent turn. Ignore this rule for creatures with haste (see rule 702.10).
>
> 602.5b If an activated ability has a restriction on its use (for example, “Activate only once each turn”), the restriction continues to apply to that object even if its controller changes.
>
> 602.5c If an object acquires an activated ability with a restriction on its use from another object, that restriction applies only to that ability as acquired from that object. It doesn’t apply to other, identically worded abilities.
>
> 602.5d Activated abilities that read “Activate only as a sorcery” mean the player must follow the timing rules for casting a sorcery spell, though the ability isn’t actually a sorcery. The player doesn’t actually need to have a sorcery card that they could cast.
>
> 602.5e Activated abilities that read “Activate only as an instant” mean the player must follow the timing rules for casting an instant spell, though the ability isn’t actually an instant. The player doesn’t actually need to have an instant card that they could cast.
>
> 603. Handling Triggered Abilities
>
> 605.1. Some activated abilities and some triggered abilities are mana abilities, which are subject to special rules. Only abilities that meet either of the following two sets of criteria are mana abilities, regardless of what other effects they may generate or what timing restrictions (such as “Activate only as an instant”) they may have.
>
> 605.1a An activated ability is a mana ability if it meets all of the following criteria: it doesn’t require a target (see rule 115.6), it could add mana to a player’s mana pool when it resolves, it’s not a loyalty ability (see rule 606, “Loyalty Abilities”), and its cost and effect don’t move any card to or from a library. Do not take into account replacement effects that may apply, other than self-replacement effects, when evaluating these criteria.
>
> 605.1b A triggered ability is a mana ability if it meets all of the following criteria: it doesn’t require a target (see rule 115.6), it triggers from the activation or resolution of an activated mana ability (see rule 605.1a) or from mana being added to a player’s mana pool, and it could add mana to a player’s mana pool when it resolves.
>
> 605.3. Activating an activated mana ability follows the rules for activating any other activated ability (see rule 602.2), with the following exceptions:
>
> 605.3a A player may activate an activated mana ability whenever they have priority, whenever they are casting a spell or activating an ability that requires a mana payment, or whenever a rule or effect asks for a mana payment, even if it’s in the middle of casting or resolving a spell or activating or resolving an ability.
>
> 605.3b An activated mana ability doesn’t go on the stack, so it can’t be targeted, countered, or otherwise responded to. Rather, it resolves immediately after it is activated. (See rule 405.6c.)
>
> 605.3c Once a player begins to activate a mana ability, that ability can’t be activated again until it has resolved.
>
> 107.3a If a spell or activated ability has a mana cost, alternative cost, additional cost, and/or activation cost with an {X}, [-X], or X in it, and the value of X isn’t defined by the text of that spell or ability, the controller of that spell or ability chooses and announces the value of X as part of casting the spell or activating the ability. (See rule 601, “Casting Spells.”) While a spell is on the stack, any X in its mana cost or in any alternative cost or additional cost it has equals the announced value. While an activated ability is on the stack, any X in its activation cost equals the announced value.
>
> 700.2. A spell or ability is modal if it has two or more options in a bulleted list preceded by instructions for a player to choose a number of those options, such as “Choose one —.” Each of those options is a mode. Modal cards printed prior to the Khans of Tarkir™ set didn’t use bulleted lists for the modes; these cards have received errata in the Oracle card reference so the modes do appear in a bulleted list.
>
> 700.2a The controller of a modal spell or activated ability chooses the mode(s) as part of casting that spell or activating that ability. If one of the modes would be illegal (due to an inability to choose legal targets, for example), that mode can’t be chosen. (See rule 601.2b.)
>
> 700.2b The controller of a modal triggered ability chooses the mode(s) as part of putting that ability on the stack. If one of the modes would be illegal (due to an inability to choose legal targets, for example), that mode can’t be chosen. If no mode is chosen, the ability is removed from the stack. (See rule 603.3c.)
>
> 700.2c If a spell or ability targets one or more targets only if a particular mode is chosen for it, its controller will need to choose those targets only if they chose that mode. Otherwise, the spell or ability is treated as though it did not have those targets. (See rule 601.2c.)
>
> 700.2d If a player is allowed to choose more than one mode for a modal spell or ability, that player normally can’t choose the same mode more than once. However, some modal spells include the instruction “You may choose the same mode more than once.” If a particular mode is chosen multiple times, the spell is treated as if that mode appeared that many times in sequence. If that mode requires a target, the same player or object may be chosen as the target for each of those modes, or different targets may be chosen.
>
> 700.2e Some spells and abilities specify that a player other than their controller chooses a mode for it. In that case, the other player does so when the spell or ability’s controller normally would do so. If there is more than one other player who could make such a choice, the spell or ability’s controller decides which of those players will make the choice.
>
> 700.2f Modal spells and abilities may have different targeting requirements for each mode. Changing a spell or ability’s target can’t change its mode.
>
> 700.2g A copy of a modal spell or ability copies the mode(s) chosen for it. The controller of the copy can’t choose a different mode. (See rule 707.10.)
>
> 700.2h Some modal spells have one or more modes with a cost listed before the effect of that mode. This indicates that the mode has an additional cost that must be paid as the spell is cast if that mode is chosen. If more than one such mode is chosen, all additional costs must be paid to cast that spell. Paying these costs follows the rules for paying additional costs in rules 601.2b and 601.2f–h.
>
> 700.2i Some modal spells have one or more pawprint symbols ({P}) rather than bullet points, as well as an instruction to choose up to a specified number of {P} “worth of modes.” While casting such a spell, its controller can choose any number of modes such that the total number of pawprint symbols listed for the chosen modes is not greater than the specified number.
>
> 700.2a The controller of a modal spell or activated ability chooses the mode(s) as part of casting that spell or activating that ability. If one of the modes would be illegal (due to an inability to choose legal targets, for example), that mode can’t be chosen. (See rule 601.2b.)
>
> 115.1. Some spells and abilities require their controller to choose one or more targets for them. The targets are object(s) and/or player(s) the spell or ability will affect. These targets are declared as part of the process of putting the spell or ability on the stack. The targets can’t be changed except by another spell or ability that explicitly says it can do so.
>
> 115.1a An instant or sorcery spell is targeted if its spell ability identifies something it will affect by using the phrase “target [something],” where the “something” is a phrase that describes an object and/or player. The target(s) are chosen as the spell is cast; see rule 601.2c. (If an activated or triggered ability of an instant or sorcery uses the word target, that ability is targeted, but the spell is not.)
>
> Example: A sorcery card has the ability “When you cycle this card, target creature gets -1/-1 until end of turn.” This triggered ability is targeted, but that doesn’t make the card it’s on targeted.
>
> 115.1b Aura spells are always targeted. An Aura’s target is specified by its enchant keyword ability (see rule 702.5, “Enchant”). The target is chosen as the spell is cast; see rule 601.2c. An Aura permanent doesn’t target anything; only the spell is targeted. (An activated or triggered ability of an Aura permanent can also be targeted.)
>
> 115.1c An activated ability is targeted if it identifies something it will affect by using the phrase “target [something],” where the “something” is a phrase that describes an object and/or player. The target(s) are chosen as the ability is activated; see rule 602.2b.
>
> 115.1d A triggered ability is targeted if it identifies something it will affect by using the phrase “target [something],” where the “something” is a phrase that describes an object and/or player. The target(s) are chosen as the ability is put on the stack; see rule 603.3d.
>
> 115.1e Some keyword abilities, such as equip and modular, represent targeted activated or triggered abilities, and some keyword abilities, such as mutate, cause spells to have targets. In those cases, the phrase “target [something]” appears in the rule for that keyword ability rather than in the ability itself. (The keyword’s reminder text will often contain the word “target.”) See rule 702, “Keyword Abilities.”
>
> 115.3. The same target can’t be chosen multiple times for any one instance of the word “target” on a spell or ability. If the spell or ability uses the word “target” in multiple places, the same object or player can be chosen once for each instance of the word “target” (as long as it fits the targeting criteria). This rule applies both when choosing targets for a spell or ability and when changing targets or choosing new targets for a spell or ability (see rule 115.7).
>
> 115.4. Some spells and abilities that refer to damage require “any target,” “another target,” “two targets,” or similar rather than “target [something].” These targets may be creatures, players, planeswalkers, or battles. Other game objects, such as noncreature artifacts or spells, can’t be chosen.
>
> 115.5. A spell or ability on the stack is an illegal target for itself.

## Notes and design consequences
- **601.2 / 602.2 + 734 rewind:** if the player can't complete a step, the game "returns to the moment before". apnap never needs to rewind (D-019): the engine only offers a cast/activation if at least one complete legal sequence exists, and at each later step it only offers choices that keep completion possible. Modes are offered only if their targets exist (700.2a); X is bounded by what's payable; mana abilities are offered only if the cost stays payable afterward (exact bipartite feasibility, `mana.rs`).
- **601.2b order:** modes, then X (and other announcements), then **601.2c** targets, **601.2d** division, **601.2f** total cost locked in, **601.2g** mana abilities, **601.2h** pay, **601.2i** becomes cast.
- **601.2h "in any order":** which floating mana pays a generic cost is a player choice when it matters; the engine asks (`Allocate` over mana types) only when the choice is ambiguous.
- **602.2a:** an activated ability on the stack is an object that is *not a card* (it matters for 800.4a).
- **602.5a / 302.6:** `{T}` abilities of a creature need it to have been controlled since the controller's most recent turn began, unless it has haste (702.10c). This applies to creature mana abilities too.
- **605.3b:** activated mana abilities don't use the stack; they resolve immediately.
- **115.5:** a spell or ability on the stack is an illegal target for itself.

## Tests
`crates/harness/tests/rules_m1.rs`: `cr_601_*`, `cr_602_*`, `cr_605_*`, `cr_107_3a_*`, `cr_700_2a_*`, `cr_115_5_*`.

## Costs that aren't mana: sacrifice, discard, life; additional costs (D-048)
> 118.3. A player can’t pay a cost without having the necessary resources to pay it fully.
> For example, a player with only 1 life can’t pay a cost of 2 life, and a permanent that’s
> already tapped can’t be tapped to pay a cost.

> 118.8. Some spells and abilities have additional costs. An additional cost is a cost
> listed in a spell’s rules text, or applied to a spell or ability from another effect, that
> its controller must pay at the same time they pay the spell’s mana cost or the ability’s
> activation cost.

> 119.4. If a cost or effect allows a player to pay an amount of life greater than 0, the
> player may do so only if their life total is greater than or equal to the amount of the
> payment. If a player pays life, the payment is subtracted from their life total; in other
> words, the player loses that much life.

> 701.21a To sacrifice a permanent, its controller moves it from the battlefield directly to
> its owner’s graveyard. A player can’t sacrifice something that isn’t a permanent, or
> something that’s a permanent they don’t control.

Engine (`frames/cast.rs`):
- `can_begin` offers a spell or ability only if every part of its cost can be paid
  (`extra_payable`): enough permanents to sacrifice, enough cards to discard — the card
  being cast doesn't count, it is on the stack by then (601.2a) — and life total ≥ the life
  to pay.
- Which permanents and cards pay is asked after the mana abilities (601.2g, `CastStage::Pay`)
  and before the payment itself (`CastStage::ChooseSacrifice`, `ChooseDiscard`; D-052 — until
  then it was asked before the mana). They leave with the rest of the payment in `finish`
  (601.2h, one Apply batch: sacrifices, discards, the life loss), so a land can be tapped
  for mana and then sacrificed, while a Treasure sacrificed for mana is no longer there to
  be chosen. They are events like any other: replaceable, and they trigger ("dies",
  "whenever you lose life").
- A spell's additional cost is `AbilityDef::AdditionalCost` on its card. Forge restates the
  whole cost on the `SP$` line (`Cost$ 1 B Sac<1/Creature>`); the compiler requires the mana
  in it to be the card's mana cost and takes the rest as the additional cost.
- Costs are paid whether or not the spell or ability later resolves (601.2h happens before
  it can be responded to).

## Mana abilities that make a choice of mana, or several mana (D-049)
> 106.4. When an effect instructs a player to add mana, that mana goes into a player’s mana pool. From there, it can be used to pay costs immediately, or it can stay in the player’s mana pool as unspent mana. Each player’s mana pool empties at the end of each step and phase, and the player is said to lose this mana. Cards with abilities that produce mana or refer to unspent mana have received errata in the Oracle™ card reference to no longer explicitly refer to the mana pool.

> 605.1a An activated ability is a mana ability if it meets all of the following criteria: it doesn’t require a target (see rule 115.6), it could add mana to a player’s mana pool when it resolves, it’s not a loyalty ability (see rule 606, “Loyalty Abilities”), and its cost and effect don’t move any card to or from a library. Do not take into account replacement effects that may apply, other than self-replacement effects, when evaluating these criteria.

> 605.3b An activated mana ability doesn’t go on the stack, so it can’t be targeted, countered, or otherwise responded to. Rather, it resolves immediately after it is activated. (See rule 405.6c.)

Engine: a permanent that can be tapped for mana is a `cast::ManaSource { id, mask, count }`:
one activation adds `count` mana, all of one type the player picks from `mask`
(`OptionDesc::TapForMana { source, mana }` names the type; `tap_for_mana` adds all of it).
`cast::payable` decides whether a cost can be paid from the pool and the sources, trying
each type for a source that makes several mana of a chosen type, so that no payment step is
offered that can't be completed (D-019). Mana that isn't spent stays in the pool until the
step or phase ends (106.4; `frames/turn.rs`).

## Treasure, commander identity, restricted mana, mana from spells (D-052)
> 111.10a A Treasure token is a colorless Treasure artifact token with “{T}, Sacrifice this token: Add one mana of any color.”

> 106.6. Some spells or abilities that produce mana restrict how that mana can be spent, have an additional effect that affects the spell or ability that mana is spent on, or create a delayed triggered ability (see rule 603.7a) that triggers when that mana is spent. This doesn’t affect the mana’s type.

> 106.12. To “tap [a permanent] for mana” is to activate a mana ability of that permanent that includes the {T} symbol in its activation cost. See rule 605, “Mana Abilities.”

> 605.5a An ability with a target is not a mana ability, even if it could put mana into a player’s mana pool when it resolves. The same is true for a triggered ability that could produce mana but triggers from an event other than activating a mana ability, or a triggered ability that triggers from activating a mana ability but couldn’t produce mana. These follow the normal rules for activated or triggered abilities, as appropriate.

> 605.5b A spell can never be a mana ability, even if it could put mana into a player’s mana pool when it resolves. It’s cast and resolves just like any other spell. Some older cards were printed with the card type “mana source”; these cards have received errata in the Oracle card reference and are now instants.

> 602.5b If an activated ability has a restriction on its use (for example, “Activate only once each turn”), the restriction continues to apply to that object even if its controller changes.

> 903.4. The Commander variant uses color identity to determine what cards can be in a deck with a certain commander. The color identity of a card is the color or colors of any mana symbols in that card’s mana cost or rules text, plus any colors defined by its characteristic-defining abilities (see rule 604.3) or color indicator (see rule 204).

> 903.4f If an ability refers to the colors or number of colors in a commander’s color identity, that quality is undefined if that player doesn’t have a commander. That part of the ability won’t do anything. Costs that refer to that quality are unpayable.

> 608.2h If an effect requires information from the game (such as the number of creatures on the battlefield), the answer is determined only once, when the effect is applied. If the effect requires information from a specific object, including the source of the ability itself, the effect uses the current information of that object if it’s in the public zone it was expected to be in; if it’s no longer in that zone, or if the effect has moved it from a public zone to a hidden zone, the effect uses the object’s last known information. See rule 113.7a. If an ability states that an object does something, it’s the object as it exists—or as it most recently existed—that does it, not the ability.

Engine (`frames/cast.rs`):
- `ManaSource { id, mask, count, sacrifice, ability }` is one *way* to tap a permanent for
  mana. A permanent's plain abilities ("{T}: Add one mana", and what its land types give
  it) are merged into one entry; any other mana ability (several mana, "sacrifice this",
  a spending restriction) is its own entry, and entries of one permanent exclude each
  other in `payable`. The player still picks by permanent and mana type
  (`OptionDesc::TapForMana`), so card-db rejects a card with two mana abilities that
  differ in more than the type and make the same type.
- **Treasure (111.10a, 605.3b):** `tap_for_mana` taps, adds the mana and returns the
  sacrifice, which the caller applies at once as a `Frame::Apply` (a replaceable event);
  the cast frame then carries on. The mana ability's cost is paid when it is activated,
  i.e. before the spell's own costs (601.2g before 601.2h).
- **One permanent can't pay twice:** a source that sacrifices itself and could also be
  chosen for the cost's own "sacrifice N" is limited (`SacLimit`): only so many of them may
  be used for mana that N candidates remain. `can_begin`, the X bound and every tap option
  use the same check (`payable_within`), so no offered step can dead-end (D-019).
- **"Activate only if" (602.5b)** on a mana ability: `AbilityDef::Mana::only_if`, checked
  each time the sources are listed.
- **Commander identity (903.4, 903.4f):** `ManaProduce::CommanderIdentity` makes one mana of
  a colour in the identity of the controller's commander(s), taken from the commander
  card's definition (903.4a); with no commander the ability adds nothing and isn't offered.
  `color_identity` moved from card-db to card-model so the engine can use it.
- **Restricted mana (106.6):** `AbilityDef::Mana::spend` (`ManaRestriction { spells,
  abilities }`). Such mana is kept apart in `PlayerState::restricted_mana` (which ability
  made it); what a player can spend on a given spell or ability is `usable_pool`. When
  the cost is paid, matching restricted mana is spent first. While paying for something
  the mana can't be spent on, its source isn't offered. It empties with the pool (106.4).
  A player's view shows it as part of the pool, with the restricted part counted.
- **Mana from a spell or a non-mana ability (605.5):** `Effect::AddMana` — on the stack like
  any other, the mana arrives on resolution.
- **"The sacrificed creature's power" (608.2h):** the total power of the permanents chosen
  for a cost's sacrifice, read in `finish` just before they leave, kept on the stack object
  (`StackObject::sacrificed_power`) and used as `Amount::SacrificedPower`.
