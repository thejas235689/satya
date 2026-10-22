# Semantic conformance

Parsing a construct is not the same as implementing it. A tool can accept
`join_branch` and still ignore what it means; it can accept `constraint default`
and apply it as though it were hard. This document tracks what Satya does
*according to the LRM's defined behaviour*, and — just as importantly — where it
only approximates it.

For coverage of **every** construct the standard defines — all 146 syntax
excerpts, each marked implemented / partial / not implemented — see
[`FEATURES.md`](FEATURES.md).

The test of semantic conformance used here is simple: **feed the LRM's own
examples, with their stated expected results, and check the tool produces them.**

---

## 1. Implemented per the standard

### Scheduling (Clause 12)

| Construct | Clause | Behaviour |
|---|---|---|
| `join_all` (default) | 12.3.6.1 | the following statement is scheduled after every branch |
| `join_none` | 12.3.6.1c | no scheduling dependency — the following statement is scheduled independently (and is emitted first, to make the absence of a dependency visible) |
| `join_branch(L1, …)` | 12.3.6.1a | the following statement waits for the named top-level branches |
| `join_select(n)` | 12.3.6.1b | the following statement waits for *n* randomly selected branches; `n = 0` behaves as `join_none` |
| `join_first(n)` | 12.3.6.1d | a run-time dependency only, so it imposes no scheduling order |
| `replicate (n) L: stmt` | 12.5 | expands in place into *n* labelled copies, distinct from `repeat` |
| labelled statements | 12.1 | labels are kept and drive `join_branch` |

Checked against the LRM's Examples 68, 71 and 72, which state the expected
ordering. For Example 71 (`parallel join_select(1) { L2: do A; L3: do B; } L4: do C;`)
the selected branch, then `C`, then the other branch:

```
0. B   [parallel g1]  L3
1. Cc  [parallel g1]  L4
2. A   [parallel g1]  L2
```

### Constraints (Clause 16)

| Construct | Clause | Behaviour |
|---|---|---|
| default constraints | 16.1.10.1 | one default per attribute survives, by precedence; conflicts are flagged |
| `default disable <attr>` | 16.1.10.1b | removes the defaults on an attribute |
| `dist` / `:=` / `:/` | 16.1.9 | weighted selection; `:=` gives each value in a range the full weight, `:/` spreads it |
| `solve … before …` | 16.1.8 | assignment order follows the directive |
| implications | 8.5 | propagated, not merely checked |
| ordering between two random variables | — | both domains narrowed before assignment |

**Default constraints.** The LRM's Example 133 states its expected results —
`s1.attr1` = 0, `s2.attr1` = 2 (the action-level default overriding the type's),
`s3.attr1` in 1..2 because its default was disabled, `attr1 < attr2` throughout.
Satya produces exactly that on every seed. Example 134 reports its contradiction
rather than silently picking a side.

**`dist`.** Measured over 600 seeds, `dist x in [100..102 := 1, 200 := 2, 300 := 5]`
gives 10.2 / 9.7 / 10.2 / 20.7 / 49.3 % against the specified 10 / 10 / 10 / 20 / 50.
With `:/`, `[100..102 :/ 6, 200 := 2]` gives 25.3 / 25.5 / 24.8 / 24.3 % against
25 / 25 / 25 / 25.

### Data and execution

| Construct | Clause | Behaviour |
|---|---|---|
| fixed-size arrays `array<T, N>`, `x[N]` | 8.4 | expand into one variable per element, with per-element domains |
| `arr[i]` indexing | 8.6.2 | resolves to the element variable |
| `arr.size()`, `sum()`, `product()`, `min()`, `max()` | 8.6 | evaluated |
| `foreach (arr[i])` | 12.2.6 | iterates the elements |
| `exec pre_solve` | 16.4.12 | executed **before** the solver; its assignments to non-random fields feed the solve |
| `exec post_solve` | 16.4.12 | executed **after** the solver, from the solved values |
| non-`rand` attributes | 8.4 | not randomised — they hold their initial value |
| flow-object ordering | 13 | an input must be bound to an object produced by an earlier step; a violation is reported |
| covergroups, coverpoints, `cover { … }` | 18, 19.2 | sampled while the `cover` block's activity executes; a coverage summary is reported |
| `cross` items | 18.4 | sampled as a value tuple, once every referenced coverpoint is known |
| explicit `bins` | 18.3 | coverage is reported against the declared bins (`bins hit/declared`) |
| `cover <type>;` | 19.2 | matches the named monitor's scenario against the executed stream and samples its covergroups over the matched steps |
| monitor declarations | 19.1 | parsed, with their action handles and covergroups |
| monitor activity: action traversal | 19.3.1 | matches the *earliest* step of that action at or after the checkpoint, subject to the inline constraint |
| `sequence` / `concat` | 19.3.2, 19.3.3 | `sequence` requires the sub-scenarios to be contiguous; `concat` allows gaps, and the realization is the matched steps only — a gap is not part of it |
| `eventually` | 19.3.4 | matches from any checkpoint at or after the start |
| `select` | 19.3.6 | takes the first arm whose guard holds |
| nested monitors | 19.3.9 | matched by name |
| array operators `==`, `!=`, `in` | 7.9.2.1 | element-wise equality and set membership are solved |
| array properties `arr.size`, `arr.sum` | 7.9.2.4 | the deprecated property forms evaluate like the methods |
| `exec init_down` / `init_up` | 22.1 | run for the component hierarchy before the scenario, `init_down` top-down and `init_up` bottom-up |
| collection mutators in `exec` | 7.9.3.2 | `push_back`, `push_front`, `pop_back`, `pop_front`, `insert`, `delete`, `clear`, `shuffle` build a list that constraints can then read |
| collection literal assignment | 8.5 | `L = {a, b, c};` in `exec` gives a collection its elements |
| value list literal | 5 | `[1, 2, 3]` and `[a, b]` as a constraint's value set |
| procedural statements | 89-101 | `repeat`/`repeat…while`/`foreach`/`if`/`match`/`break`/`continue`/`return`/`randomize` in `exec`, with control flow |
| floating-point (`float_pkg`) | 112-116 | `float32`/`float64` storage plus `float_mantissa`/`float_exponent`/`float_sign`/`to_float` and the math functions, over the IEEE-754 bit pattern |
| messaging | 106-110 | `print`/`message`/`error`/`fatal` are recorded and reported; file operations are accepted and have no effect |
| randomization functions | 111 | `urandom()` / `urandom_range()` — deterministic and varying by run seed |
| `x in <collection>` | 8.5.9 | membership in a named array/list is a solved constraint, uniform over its elements |

The array work is what lets a **multireg** be modelled directly: OpenTitan's
`KEY_SHARE0` is `array<bit[32], 8>`, and it now yields eight modelled registers
rather than needing to be hand-unrolled (see `docs/OPENTITAN.md` §5).

---

## 2. Parsed but still approximate

Listed so the coverage numbers are not read as more than they are. The previous
revision of this list named `sort`, `reverse`, `unique`, `and`, `or` and `xor`
as unimplemented array methods — **that was wrong**: the standard defines no such
methods for arrays (7.9.2.2). The real list is below.

| Construct | Clause | What Satya does today |
|---|---|---|
| array methods `join`, `str_from_chars` | 7.9.2.2 | not evaluated — they produce strings, which the engine does not model as values |
| `to_list`, `to_set` conversions | 7.9.2.2, 7.9.3.2 | not evaluated |
| collection mutators inside a *constraint* | 16.1 | only `exec` mutates a collection; a constraint can read one but not change it |
| `overlap` / `schedule` monitor statements | 19.3.5, 19.3.8 | each branch is matched independently from the checkpoint; true interleaving of concurrent branches is not modelled |
| monitor constraints | 19.4 | parsed, but a monitor constraint does not filter which steps match |
| monitor inheritance | 19.6 | a monitor's base type is parsed but not applied |
| `exec` statements other than assignment, `if` and method calls | 10.4 | `for`/`while` loops and target-code templates are passed through to the backends, not executed |
| `join_first(n)`'s run-time dependency | 12.3.6.1d | imposes no *scheduling* order (correct), but there is no runtime, so the "first n to complete" part is not simulated |
| multiple `dist` directives on one attribute | 16.1.9 | weights are summed; the LRM leaves the resulting distribution undefined |
| stream vs buffer consumption | 13 | a flow object may be referenced by several inputs; consume-once semantics for streams are not enforced |
| `import`-based scope checking | 9 | name resolution uses `pkg::name` and `Component::action`, not `import` |
| target-code templates (`exec declaration` bodies) | 22.2 | carried through to the backends verbatim, not interpreted |

## 3. How this is checked

* `tests/integration.rs` — 57 tests, including the LRM's Examples 133 and 134
  with their stated values, the join orderings of Examples 68/71/72, the `dist`
  distribution, exec ordering, flow-object ordering, arrays, `foreach` and
  coverage sampling.
* `tools/lrm_examples.py` — runs every extracted standard example and reports a
  lenient and a strict parse bound plus the engine result.
* `tools/ot_regmap_check.py` — diffs the OpenTitan UART model against the IP's
  register specification.

Section 1 is covered by tests. Section 2 is **not** — those constructs have no
behaviour to test yet, which is precisely why they are listed separately.
