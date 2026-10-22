# Semantic conformance

Parsing a construct is not the same as implementing it. A tool can accept
`join_branch` and still ignore what it means; it can accept `constraint default`
and apply it as though it were hard. This document tracks what Satya does
*according to the LRM's defined behaviour*, and — just as importantly — where it
only approximates it.

The test of semantic conformance used here is simple: **feed the LRM's own
examples, with their stated expected results, and check the tool produces them.**

---

## 1. Implemented per the standard

### Default constraints (PSS 16.1.10.1)

| Rule | Clause | Status |
|---|---|---|
| A default behaves as an equality constraint unless disabled | 16.1.10.1a | implemented |
| Conflict between a default and a hard constraint is a contradiction, flagged | 16.1.10.1a | implemented |
| `constraint default disable <attr>` removes the defaults on `<attr>` | 16.1.10.1b | implemented |
| Only one default per attribute survives; higher-level context beats lower, derived beats base, otherwise later wins | 16.1.10.1d | implemented |
| Defaults may target an aggregate's attributes | 16.1.10.1e | approximated — treated as a constraint on the whole attribute |

Verified against the LRM's own examples:

**Example 133** — the standard states the expected outcome: `s1.attr1` resolves
to 0, `s2.attr1` to 2 (the action-level default overriding the type-level one),
`s3.attr1` randomised in 1..2 because its default was disabled, and
`attr1 < attr2` throughout. Satya produces exactly that, on every seed tried.

```
solved: s1.attr1=0 s1.attr2=1 s2.attr1=2 s2.attr2=3 s3.attr1=1 s3.attr2=3
constraints: 12/12 satisfied
```

**Example 134** — `default attr1 == -1` is disabled and `attr1 == 0` applies
(→ 0); `default attr2 == -1` conflicts with the hard `attr2 == 0`, which the
standard says has no consistent assignment. Satya reports the contradiction
rather than silently picking a side:

```
solved: attr1=0 attr2=0
constraints: 2/3 satisfied
  not satisfied: conflicting constraints on `attr2`
```

Before this work Satya treated *every* default as a hard constraint. It happened
to get Example 133's `s1` and `s2` right, but would have produced a spurious
contradiction whenever two defaults targeted one attribute, and it ignored
`default disable` entirely.

### Other semantics

* **Implications are propagated, not merely checked** — `p_odd == 1 -> p_enable == 1`
  forces `p_enable`, rather than being reported unsatisfied.
* **Ordering between two random variables is solved** — `a < b` narrows both
  domains (`a` to `[lo, hi_b-1]`, `b` to `[lo_a+1, hi]`) before assignment, so
  it does not get stuck when one side is pinned by another constraint.
* **Bit-wise operations take the field's width** — `~x` on a `bit[32]` is a
  32-bit value; part-selects (`x[msb:lsb]`, PSS 8.6.1) evaluate correctly.
* **A contradictory model is reported, not fatal** — an empty domain yields
  `conflicting constraints on \`x\`` and the step reports it. (It previously
  panicked.)

---

## 2. Parsed but not implemented semantically

These are accepted by the parser and **do not** affect the scheduled scenario.
They are listed so the coverage numbers are not read as more than they are.

| Construct | Clause | What Satya does today |
|---|---|---|
| `join_none`, `join_first`, `join_branch`, `join_select` | 12.3.4 | parsed and ignored — a `parallel` block always completes as if `join_all` |
| `replicate (n) stmt` | 12.5 | treated as `repeat (n) stmt`; the LRM's in-place expansion into *n labelled* statements is not modelled |
| `solve … before …` | 16.1.8 | not parsed; ordering hints are irrelevant to a deterministic assignment anyway |
| `dist` weights, `dist_list` | 16.1.9 | not parsed; randomisation is uniform over the domain |
| `array<T, N>`, `x[4]` (multiregs) | 8.4 | parses, but the engine gives arrays **no semantics** — a multireg collapses to one scalar and must be hand-unrolled |
| `foreach (c[i])` over an array | 12.2.6 | handled for pools; not over array-typed fields |
| flow-object producer/consumer ordering | 13 | allocation and pool capacity are modelled, but the rule that a consumer runs after its producer is not enforced |
| `exec` block ordering (`pre_solve`, `post_solve`, …) | 10.4 | bodies are parsed and passed through to backends; they are not executed at their defined points |
| `monitor`, `cover`, `covergroup` | 18, 19 | out of scope — Satya is a stimulus engine |

The array limitation is the most consequential: it is what stops AES's 20
multireg words being modelled directly (see `docs/OPENTITAN.md` §5).

---

## 3. How this is checked

* `tests/integration.rs` — 42 tests, including the LRM's Example 133 and 134
  with their stated expected values, and ordering constraints across 12 seeds.
* `tools/lrm_examples.py` — runs every extracted standard example and reports
  both a lenient and a strict parse bound plus the engine result.
* `tools/ot_regmap_check.py` — diffs the OpenTitan UART model against the IP's
  register specification.

Section 1 is covered by tests. Section 2 is **not** — those constructs have no
behaviour to test yet, which is precisely why they are listed separately.
