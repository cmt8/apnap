# Defender, hexproof, shroud, indestructible, fear, intimidate, landwalk; counter, tap, discard, sacrifice

Verbatim from the Comprehensive Rules effective 2026-11-06, for the script features the
Forge compiler accepts (decisions.md D-046, D-047). Tests:
`crates/harness/tests/forge_features.rs` (one per feature).

## Defender (CR 702.3)
> 702.3b A creature with defender can’t attack.

Engine: `DeclareAttackersFrame::new` doesn't list it among the creatures that may attack.

## Hexproof (CR 702.11) and shroud (CR 702.18)
> 702.11b “Hexproof” on a permanent means “This permanent can’t be the target of spells or
> abilities your opponents control.”

> 702.18a Shroud is a static ability. “Shroud” means “This permanent or player can’t be the
> target of spells or abilities.”

Engine: `targeting::is_legal`, which both offers targets and re-checks them on resolution
(CR 608.2b) — so a permanent that gains hexproof in response makes the spell's target
illegal. Only permanents: a card with hexproof in a graveyard can be targeted (the ability
works on the battlefield). "Hexproof from [quality]" (702.11d) and players with hexproof
(702.11c) are not compiled.

## Indestructible (CR 702.12)
> 702.12b A permanent with indestructible can’t be destroyed. Such permanents aren’t
> destroyed by lethal damage, and they ignore the state-based action that checks for lethal
> damage (see rule 704.5g).

> 701.8b The only ways a permanent can be destroyed are as a result of an effect that uses
> the word “destroy” or as a result of the state-based actions that check for lethal damage
> (see rule 704.5g) or damage from a source with deathtouch (see rule 704.5h). If a
> permanent is put into its owner’s graveyard for any other reason, it hasn’t been
> “destroyed.”

> 614.17. Some effects state that something can’t happen. These effects aren’t replacement
> effects, but follow similar rules.

Engine: a proposed move whose cause is `MoveCause::Destroy` is dropped when the Apply batch
is built (`ApplyFrame::new`), before any replacement effect can see it; `sba.rs` doesn't
propose it for lethal or deathtouch damage. Sacrifice, toughness 0 (704.5f) and the legend
rule are not destruction and still apply.

## Fear (CR 702.36), intimidate (CR 702.13), landwalk (CR 702.14)
> 702.36b A creature with fear can’t be blocked except by artifact creatures and/or black
> creatures.

> 702.13b A creature with intimidate can’t be blocked except by artifact creatures and/or
> creatures that share a color with it.

> 702.14c A creature with landwalk can’t be blocked as long as the defending player controls
> at least one land with the specified land type (as in “islandwalk”), with the specified
> type or supertype (as in “artifact landwalk”), without the specified type or supertype (as
> in “nonbasic landwalk”), or with both the specified type or supertype and the specified
> subtype (as in “snow swampwalk”).

Engine: `frames::combat::can_block`, one function for every evasion ability that looks at a
single blocker (menace counts blockers and is checked on the whole declaration). Only the
five basic landwalks are compiled.

## Counter (CR 701.6)
> 701.6a To counter a spell or ability means to cancel it, removing it from the stack. It
> doesn’t resolve and none of its effects occur. A countered spell is put into its owner’s
> graveyard.

"Counter target creature spell" is `TargetFilter::SpellMatching`: the spell's card types,
subtypes and colors on the stack.

## Tap and untap (CR 701.26)
> 701.26a To tap a permanent, turn it sideways from an upright position. Only untapped
> permanents can be tapped.

> 701.26b To untap a permanent, rotate it back to the upright position from a sideways
> position. Only tapped permanents can be untapped.

Engine: `Effect::Tap/Untap/TapAll/UntapAll` do nothing to a permanent already in that state
(no event).

## Discard (CR 701.9) and sacrifice (CR 701.21): who chooses, and when
> 701.9a To discard a card, move it from its owner’s hand to that player’s graveyard.

> 701.9b By default, effects that cause a player to discard a card allow the affected player
> to choose which card to discard. Some effects, however, require a random discard or allow
> another player to choose which card is discarded.

> 701.21a To sacrifice a permanent, its controller moves it from the battlefield directly to
> its owner’s graveyard. A player can’t sacrifice something that isn’t a permanent, or
> something that’s a permanent they don’t control. Sacrificing a permanent doesn’t destroy
> it, so regeneration or other effects that replace destruction can’t affect this action.

> 101.4. If multiple players would make choices and/or take actions at the same time, the
> active player (the player whose turn it is) makes any choices required, then the next
> player in turn order (usually the player seated to the active player’s left) makes any
> choices required, followed by the remaining nonactive players in turn order. Then the
> actions happen simultaneously.

Engine: `Work::EachPlayerChoose` — the affected players choose in APNAP order, then one
Apply batch moves everything. (Forge instead has each player choose and sacrifice before the
next player chooses; the rules are followed here.) A player with fewer cards or permanents
than asked discards or sacrifices all of them. "Sacrifice this" (`Effect::SacrificeThis`)
does nothing if the permanent has left the battlefield or is controlled by someone else.

## "Its controller", for a permanent that has left (CR 608.2h, 603.3a)
> 603.3a A triggered ability is controlled by the player who controlled its source at the
> time it triggered, unless it’s a delayed triggered ability.

`PlayerRef::TriggeringController` is the controller of the object whose event triggered the
ability: its current controller while it is still that permanent, otherwise the controller
it last had on the battlefield. Forge scripts write "when this dies, draw a card" with it;
for a permanent's own leaves-the-battlefield trigger that is the same player as "you"
(603.3a).

## Attacking and blocking creatures (CR 506.4)
> 506.4. A permanent is removed from combat if it leaves the battlefield, if its controller
> or protector changes, if it phases out, if an effect specifically removes it from combat,
> if it’s a planeswalker that’s being attacked and stops being a planeswalker, if it’s a
> battle that’s being attacked and stops being a battle, or if it’s an attacking or blocking
> creature that regenerates (see rule 701.19), stops being a creature, or becomes a battle.
> A creature that’s removed from combat stops being an attacking, blocking, blocked, and/or
> unblocked creature. A planeswalker or battle that’s removed from combat stops being
> attacked.

Engine: a creature is attacking while it is in `st.combat.attackers`, blocking while it is
among some attacker's blockers (`layers::combat_role`); leaving the battlefield removes it
(`actions::move_object`), and `st.combat` is cleared at the end of combat. Selectors read
this (`Selector::attacking`, `blocking`); "attacking or blocking creature" is two selectors.
The layer cache is invalidated whenever the roles change. A change of controller removes
the permanent from combat too (`GameAction::ControlChanged`; an attacker it was blocking
stays blocked, CR 509.1h). Not implemented: removal when a creature stops being a creature (nothing
that compiles does that yet).

## Equipment (CR 301.5, 702.6, 704.5n, 701.3)
> 702.6a Equip is an activated ability of Equipment cards. “Equip [cost]” means “[Cost]:
> Attach this permanent to target creature you control. Activate only as a sorcery.”

> 704.5n If an Equipment or Fortification is attached to an illegal permanent or to a
> player, it becomes unattached from that permanent or player. It remains on the
> battlefield.

> 701.3c Attaching an Aura, Equipment, or Fortification on the battlefield to a different
> object or player causes the Aura, Equipment, or Fortification to receive a new timestamp.

From 301.5c (quoted in part): "An Equipment that’s also a creature can’t equip a creature
unless that Equipment has reconfigure … An Equipment can’t equip itself. An Equipment that
equips an illegal or nonexistent permanent becomes unattached from that permanent but
remains on the battlefield."

Engine: `Effect::AttachThis` (does nothing if the source left, is a creature, or the object
isn't a creature or is the one it is already on — 701.3b); `sba.rs` unattaches (704.5n) and
the state check runs again, because a creature may have been kept alive by the bonus.

## Restrictions on attacking and blocking (CR 508.1c, 509.1b)
> 509.1b The defending player checks each creature they control to see whether it’s affected
> by any restrictions (effects that say a creature can’t block, or that it can’t block
> unless some condition is met). If any restrictions are being disobeyed, the declaration of
> blockers is illegal.

Engine: `AbilityDef::CombatRule` abilities are collected from what permanents currently have
(`combat::combat_rules`); a creature that can't attack or can't block is never offered, and
`can_block` refuses a pair a rule forbids — so restrictions that look at one creature or one
pair can't be disobeyed. (Menace looks at the whole declaration and is checked afterwards.)
