# Rules notes

One file per rules area, written **before** that area is implemented (CLAUDE.md invariant 9):

1. Copy the exact current text of the governing rules from `docs/rules/MagicCompRules.txt`
   (gitignored) into `<area>.md`, quoted, with the CR effective date.
2. Note every place where `docs/spec.md` paraphrases differently. **The CR wins**; log the
   discrepancy in `docs/decisions.md` and tell the user.
3. Write the atomic tests from the quoted text and list them in the file's test table.

| Area | File | Milestone |
|---|---|---|
| 101.4 APNAP, 117 Priority | [117-priority-101.4-apnap.md](117-priority-101.4-apnap.md) | M1 |
| 601/602/605 Casting, abilities, mana (+107.3, 115, 700.2) | [601-casting-602-activating-605-mana.md](601-casting-602-activating-605-mana.md) | M1 |
| 608 Resolution (+701.6) | [608-resolution.md](608-resolution.md) | M1 |
| 704 State-based actions | [704-sba.md](704-sba.md) | M0 (subset), M2 |
| 603 Triggered abilities | [603-triggered-abilities.md](603-triggered-abilities.md) | M2 |
| 613 Layers (+604, 611) | [613-layers.md](613-layers.md) | M2 |
| 614–616 Replacement/prevention | [614-616-replacement-prevention.md](614-616-replacement-prevention.md) | M2 |
| 800.4 Leaving the game | [800.4-leaving-the-game.md](800.4-leaving-the-game.md) | M1 (pulled forward) |
| 903 Commander (incl. 903.9) | [903-commander.md](903-commander.md) | M3 |
| 506–511 Combat + combat keywords | [506-511-combat-keywords.md](506-511-combat-keywords.md) | M3 |
| 303.4 Auras, layer 2 control | [303-auras-613-control.md](303-auras-613-control.md) | M3 |
| 702.29 Cycling, 113.6 ability zones | [702.29-cycling-113.6-ability-zones.md](702.29-cycling-113.6-ability-zones.md) | M4 |
| 113 Abilities an object is given (+305.6, 613.1f, 613.7a) | [113-abilities-given.md](113-abilities-given.md) | M4 |
| 611.2 Effects that last on no object, 615.1 prevention | [611.2-lasting-effects-615-prevention.md](611.2-lasting-effects-615-prevention.md) | M4 |
| Rules about players: 402.2 hand size, 305.2 land plays, 601.3 other zones, 401.5 top card, 702.11c/702.18a | [402-305-401.5-rules-about-players.md](402-305-401.5-rules-about-players.md) | M4 |

Excerpts are committed verbatim by explicit decision (D-012). If the repo is ever
published, revisit that decision first (Fan Content Policy, spec §12).
