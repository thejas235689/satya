# BDD prover: variable-to-variable relations

Scope: `src/bdd/prove.rs` only (+391/-33). `parser.rs`, `engine.rs`, `solver.rs`, `backend/*` untouched.

**What changed.** `translate` now recognises three comparison shapes: `var OP const` (unchanged fast path), `arith ==/!= const` (unchanged), and a new general comparator between any two arithmetic operands — `x < y`, `x <= y`, `x == y`, `x != y`, `x > y`, `x >= y`, `x + y == z`, `x - y == z`. New circuits: `eq_bits`, `ult_bits`, and `lt_bits` (signed `<` = unsigned compare with both sign bits flipped, so there is no overflow case to guard). `bits_of` now returns the bits of a variable's *value*, not its encoding — the domain offset `lo` is folded in with the adder, which also fixes a latent bug where `x + y == k` compared an encoded sum against an unencoded `k` whenever `lo != 0`. A literal that does not fit the comparison width makes translation abstain (`fits_width`) instead of wrapping.

**Abstention intact.** `x * y == 6`, `x / y == 2`, `x * 2 == 4`, `x < y * y` all still return NOT PROVED (asserted by `relations_outside_the_translator_still_abstain`).

**Enumeration evidence** — BDD satisfying set vs direct evaluation, every assignment in the domain, 0 mismatches out of 1600:

| expression | cases | agree |
|---|---|---|
| `x < y`, `<=`, `==`, `!=`, `>`, `>=` (x,y in 0..7) | 6 x 64 | 384/384 |
| `x < y`, `==`, `>=` (x in 2..9, y in -3..4) | 3 x 64 | 192/192 |
| `x + y == z` (0..7 each; 36 satisfying) | 512 | 512/512 |
| `x + y == z && x < y` (0..7 each) | 512 | 512/512 |

The three-variable order test (`a_three_variable_model_shows_an_order_difference`) additionally enumerates all 512 assignments under *both* orderings, so the smaller diagram is proved to be the same function, not a wrong one. The cross-domain row is the one that catches an encoding-based comparator.

**`--order` now differs.** All of these printed `NOT PROVED` before; all now print real counts:
- three variables, `x + y == z` (0..7 each): **196 node(s) in declaration order, 164 in reverse order (0.8x)**
- three variables, `x + y == z && x < y`: 621 / 527 (0.8x)
- `x + y == z`, x,y in 0..15, z in 0..31: 607 / 487 (0.8x)
- four-variable `<` chain (0..15 each): 948 / 654 (0.7x) — largest spread observed
- reverse can be *larger*: `x == y + z && x < 8` (0..15): 441 / 536 (1.2x)

Honest nuance: `--order` was never stuck at 1.0x on *every* model — models it could already translate that contain a third variable did differ (e.g. `x + y == 7 && z < 4` was 117/97 before and is byte-identical after). What it could not do was express relations *between* variables; those models abstained. Purely single-variable models are order-insensitive and still print 1.0x, correctly.

**`cargo test`**: before 178 passing (19 lib + 159 integration), after **183 passing** (24 + 159), 0 failures — 5 new tests, no regressions. Note the brief's baseline of 173 does not match the repo: HEAD c92d8d2 measures 178 (173 + the 5 arithmetic tests added by the preceding commit). The patch applies cleanly with `git apply` to a fresh clone and the suite passes there.
