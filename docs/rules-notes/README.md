# Rules notes

One file per rules area, written **before** that area is implemented (CLAUDE.md invariant 9):

1. Copy the exact current text of the governing rules from `docs/rules/MagicCompRules.txt`
   (gitignored) into `<area>.md`, quoted, with the CR effective date.
2. Note every place where `docs/spec.md` paraphrases differently. **The CR wins**; log the
   discrepancy in `docs/decisions.md` and tell the user.
3. Write the atomic tests from the quoted text and list them in the file's test table.

| Area | File | Milestone |
|---|---|---|
| 704 State-based actions | [704-sba.md](704-sba.md) | M0 (subset), M2 |
| 603 Triggered abilities | — | M2 |
| 613 Layers | — | M2 |
| 614–616 Replacement/prevention | — | M2 |
| 800.4 Leaving the game | — | M3 |
| 903 Commander (incl. 903.9) | — | M3 |

Excerpts are committed verbatim by explicit decision (D-012). If the repo is ever
published, revisit that decision first (Fan Content Policy, spec §12).
