# Randomisation and coverage

Two things are easy to get wrong in a PSS engine and hard to notice by eye: a
`dist` whose weights are not honoured, and a constraint that is *reported*
satisfied but never enforced. Coverage has the same shape — a bin that is
declared but never sampled looks exactly like one that is simply rare.

This document records what was measured, with the numbers, rather than what was
intended.

Reproduce everything here with:

```bash
python3 tools/rand_cov_check.py ./target/release/satya --seeds=200
```

The examples live in `examples/randomisation/` and `examples/coverage/`. The
checker restates each model's intent in Python and verifies the *solutions*
against it — it does not trust the tool's own report.

## Randomisation

### `dist` weights are honoured

`examples/randomisation/r1_dist_weights.pss`:

```
constraint dist pick in [0 := 1, 1 := 3, 10..12 :/ 6];
```

`:=` gives each value the weight; `:/` divides it across a range, so 10, 11 and
12 share weight 6 as 2 each. Over 200 seeds:

| value | expected | observed | count |
|---|---|---|---|
| 0 | 10.0% | 10.0% | 20 |
| 1 | 30.0% | 33.0% | 66 |
| 10 | 20.0% | 20.5% | 41 |
| 11 | 20.0% | 17.0% | 34 |
| 12 | 20.0% | 19.5% | 39 |

Pearson chi-square **1.55** against 5 categories — well inside the noise. The
weights are honoured, and `:/` really does distribute rather than assign.

### Constraints are enforced, and reported honestly when they are not

The checker verifies every solution against the model's intent across 60 seeds
per action: ranges, alignment, implication, if/else, relations between two
random variables, and per-element array bounds. All four actions pass.

The important property is the second half of the sentence above: when the
solver *cannot* satisfy something it says so, per constraint:

```
solved: addr=388 burst=1 kind=0 len=26
constraints: 6/7 satisfied
  not satisfied: kind == READ -> burst == 0
```

That matters more than a high success rate. A solver that quietly returns a
violating assignment is worse than one that admits it.

### What writing these examples found

Four defects, none of which the parse-level corpora could reach:

- **`cover { ... }` dropped its body.** `cover { do b; }` skipped the traversal
  to the next `;`, so the scenario came out *empty* — zero steps, no error.
- **A modulo constraint was never enforced.** `addr % 4 == 0` was reported
  unsatisfied on every seed: bound propagation cannot invert a modulo. Over a
  small domain the disallowed values can simply be forbidden, which is what it
  does now.
- **`if/else` constraints were dropped entirely** — not applied *and* not
  counted, so the report read `1/1 satisfied` while the branch was
  unconstrained. They are now lowered onto two implications (`c -> then`,
  `!c -> else`), and the implication repair handles `==`, `<=`, `>=`, `<`, `>`
  and `in` range/set consequents.
- **A cross never sampled unlabelled coverpoints.** `cross len, m` found
  nothing when the coverpoints had no `label`, because only labelled ones were
  keyed for the cross. A coverpoint is now keyed by its label *or* its
  expression.

The checker also caught a fault in my own example: `burst >= 1` for short
transfers contradicted the READ implication that forces `burst == 0`, making
one draw in sixty unsatisfiable. The model was wrong, not the solver — which is
exactly what an independent check is for.

## Coverage

Coverage is sampled inside a `cover { ... }` activity block (PSS 19.2), which is
how the engine models "observe these steps". Cumulative over 60 seeds:

| example | point | hits |
|---|---|---|
| `c1_bins.pss` | `len_cg.len` bin `short_len` [0..3] | 16 |
| | bin `long_len` [4..7] | 15 |
| | bin `exact_15` = 15 | 4 |
| `c2_cross.pss` | `xfer_cg.len` bin `small` | 29 |
| | bin `large` | 31 |
| `c3_labelled.pss` | `mode_cg.len_bin` bin `small` | 27 |
| | bin `large` | 33 |

Every declared bin is reachable, and every declared coverage point — including
the crosses `xfer_cg.cross(len,kind)` and the labelled `mode_cg.len_x_mode` —
is reported.

A single run samples one value, so the tool's per-run `bins 1/3` is **not** a
coverage figure. Real coverage accumulates across a test; the checker computes
it from the solved values and the declared bins, independently of the tool's
report, so the two have to agree.

## What is still not applied

These parse but are **dropped** — not enforced and not counted, which is the
worst of the three possible behaviours and is recorded as such in
`docs/FEATURES.md`:

- **`unique` constraints** (entry 59). `constraint unique { ids }` is dropped:
  the four draws come out with duplicates in 36 of 40 seeds, while the report
  reads `4/4 satisfied`.
- **`a.sum()`** and the other array fold methods inside constraints: counted,
  but not solved, so they are honestly reported unsatisfied rather than
  silently ignored.

### `foreach` was fixed, `unique` was not

`foreach` constraints (entry 57) now work. They were parsed into
`Expr::Foreach` and then fell through to `Unsupported`, which is excluded from
the constraint count — so `constraint foreach (a[i]) { a[i] in [0..15]; }` was
neither applied nor reported. The constraint body is now expanded into one
constraint per element at atom extraction, using the element count from the
model. `examples/randomisation/r3_arrays.pss` uses the idiomatic form and
reports `9/9 satisfied` across 60 seeds.

`unique` (entry 59) is still dropped, and is now recorded as `no` rather than
`partial`. Both were previously marked *partial* on the strength of the parser
accepting them, which was an overclaim.
