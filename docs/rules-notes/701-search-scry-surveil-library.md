# Searching, shuffling, scry, surveil; cards put somewhere from hidden zones

Verbatim from the Comprehensive Rules effective 2026-11-06, for decisions.md D-050. Tests:
`crates/harness/tests/forge_features.rs` ("Libraries" section).

## Search (CR 701.23) and shuffle (CR 701.24)
> 701.23a To search for a card in a zone, look at all cards in that zone (even if it’s a
> hidden zone) and find a card that matches the given description.

> 701.23b If a player is searching a hidden zone for cards with a stated quality, such as a
> card with a certain card type or color, that player isn’t required to find some or all of
> those cards even if they’re present in that zone.

> 701.23d If a player is searching a hidden zone simply for a quantity of cards, such as “a
> card” or “three cards,” that player must find that many cards (or as many as possible, if
> the zone doesn’t contain enough cards).

> 701.23e If the effect that contains the search instruction doesn’t also contain
> instructions to reveal the found card(s), then they’re not revealed.

> 701.23i If multiple players search at once, each of those players looks at the appropriate
> cards at the same time, then those players decide in APNAP order (see rule 101.4) which
> card to find.

> 701.24a To shuffle a library or a face-down pile of cards, randomize the cards within it
> so that no player knows their order.

> 701.24b Some effects cause a player to search a library for a card or cards, shuffle that
> library, then put some or all of the found cards into a different zone or in a certain
> position in that library. In such cases, the found cards aren’t included in the shuffle,
> even though they remain in the library at that time. Rather, all the cards in that library
> except those are shuffled. Abilities that trigger when a library is shuffled will still
> trigger. See also rule 401, “Library.”

> 701.24c If an effect would cause a player to shuffle one or more specific objects into a
> library, that library is shuffled even if none of those objects are in the zone they’re
> expected to be in or an effect causes all of those objects to be moved to another zone or
> remain in their current zone.

> 701.24d If an effect would cause a player to shuffle a set of objects into a library, that
> library is shuffled even if there are no objects in that set.

Engine: `Effect::ChooseCards` with `from: Library` is a search. `Work::ChooseCards` asks
each searching player (APNAP) with `ChoiceKind::SearchLibrary`: a `PickN` over the cards in
their library that have the stated quality, offered sorted by name (the order of the
library isn't something a search reveals to the searcher — they then shuffle). `must` is
701.23d ("a card" with no quality must be found); otherwise any number up to the count may
be taken, including none (701.23b). The found cards move in one Apply batch; then
`Work::Shuffle`. When the cards go to the top of that same library the shuffle comes first
(701.24b). While the decision is pending, `PlayerView::revealed` shows the searching player
the whole library (701.23a) and nobody else anything.

## Scry (CR 701.22) and surveil (CR 701.25)
> 701.22a To “scry N” means to look at the top N cards of your library, then put any number
> of them on the bottom of your library in any order and the rest on top of your library in
> any order.

> 701.22b If a player is instructed to scry 0, no scry event occurs. Abilities that trigger
> whenever a player scries won’t trigger.

> 701.25a To “surveil N” means to look at the top N cards of your library, then put any
> number of them into your graveyard and the rest on top of your library in any order.

> 701.25c If a player is instructed to surveil 0, no surveil event occurs. Abilities that
> trigger whenever a player surveils won’t trigger.

> 401.4. If an effect puts two or more cards in a specific position in a library at the same
> time, the owner of those cards may arrange them in any order. That library’s owner doesn’t
> reveal the order in which the cards go into the library.

Engine: `Work::Scry` (one flag for surveil). The player picks which of the cards they
look at go to the bottom (`ChoiceKind::ScryBottom`) or to the graveyard
(`SurveilGraveyard`); two or more cards going to the same end of the library are then put
there in an order the player chooses (`ScryOrder`: the first listed is on top;
`OrderBottom`: the last listed is bottom-most). Cards that stay in the library stay the
same objects (`GameAction::ArrangeLibrary`): scrying isn't a zone change. Scry 0 and
surveil 0 do nothing.

## Cards put onto the battlefield without being cast: Auras (CR 303.4f–g)
> 303.4f If an Aura is entering the battlefield under a player’s control by any means other
> than by resolving as an Aura spell, and the effect putting it onto the battlefield doesn’t
> specify the object or player the Aura will enchant, that player chooses what it will
> enchant as the Aura enters the battlefield. The player must choose a legal object or
> player according to the Aura’s enchant ability and any other applicable effects.

> 303.4g If an Aura is entering the battlefield and there is no legal object or player for
> it to enchant, the Aura remains in its current zone, unless that zone is the stack. In
> that case, the Aura is put into its owner’s graveyard instead of entering the battlefield.
> If the Aura is a token, it isn’t created.

Engine: every effect that moves cards without casting them goes through `Work::Move`,
which, for an Aura card headed to the battlefield, asks its controller-to-be what it will
enchant (`ChoiceKind::AuraHost`; not a target, so hexproof and shroud don't matter) and
leaves the card where it is if there is nothing it could enchant.
