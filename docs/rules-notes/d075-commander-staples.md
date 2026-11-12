# D-075: the most played Commander cards that no feature family covers

Comprehensive Rules effective 2026-11-06, quoted verbatim. One section per feature of the
batch; tests in `crates/harness/tests/forge_staples.rs`.

## Life lost this way (119.3, 608.2c)

> 119.3. If an effect causes a player to gain life or lose life, that player’s life total is
> adjusted accordingly.

> 608.2c The controller of the spell or ability follows its instructions in the order
> written. However, replacement effects may modify these actions. …

"You gain life equal to the life lost this way" (Exsanguinate, Gray Merchant of Asphodel,
Kokusho) is what the players actually lost to the instruction before it: after replacement
effects, and not more than their life totals went down.

| Test | Rule |
|---|---|
| `life_lost_this_way_is_what_the_opponents_lost` | 119.3, 608.2c |
| `life_lost_this_way_counts_a_replaced_loss_as_none` | 608.2c (a replacement modifies the action) |
| `a_value_forge_sets_while_resolving_is_never_read_as_written` | compiler (invariant 15) |
| `comments_that_were_read_are_ignored_others_block` | compiler (invariant 15) |
