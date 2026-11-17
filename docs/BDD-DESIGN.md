# Satya — BDD proving: design note

Status: design, not implemented. Target: `satya prove` (slice 1) and `satya reach` (slice 2).

## Why

Satya's solver is inversion + bound propagation + hill-climbing. It is fast and
approximate, and it cannot answer three questions a verification engineer
actually asks:

1. **Is this constraint set satisfiable at all?** Today: run seeds, see if any
   fail. Absence of failure is not proof.
2. **Is this constraint vacuous?** A constraint that restricts nothing is a
   silent no-op — the failure class this project has spent twenty releases
   removing. There is no way to detect one today.
3. **Is this coverage bin reachable?** Sampling shows a bin *is* reachable; it
   can never show one *isn't*. An unreachable bin is either a constraint bug or
   dead coverage, and both matter.

BDDs answer all three exactly, and give the canonical form that makes
equivalence a pointer comparison.

## Where BDDs are the wrong tool

PSS integers are 32-bit. A BDD over `[0..2^32)` is not a data structure, it is a
memory leak. General PSS solving stays with sampling/SAT. This design is
deliberately narrow:

**A variable is BDD-eligible only if its declared domain is small.** Concretely:
- `bool` — 1 bit
- an enum — ceil(log2(variants)) bits
- `bit[N]` with N <= 8 — N bits
- `x in [lo..hi]` with `hi - lo + 1 <= 256` — 8 bits

Everything else is reported as **NOT PROVED**, never as "satisfiable". That
distinction is the entire value: a tool that says "I could not prove this" is
usable; one that guesses is not.

## Slice 1 — `satya prove`

```
satya prove model.pss --action A --sat          # satisfiable?
satya prove model.pss --action A --vacuity      # which constraints restrict nothing
satya prove model.pss --action A --equivalent 'B'   # vs another constraint block
satya prove model.pss --action A --order        # node count under two variable orders
```

Exit codes: `0` proved, `1` disproved, `2` not proved (out of scope). Scriptable,
and honest in the exit code as well as the text.

### Translation

Each eligible variable becomes a vector of Boolean BDD variables, encoded in its
declared domain's order. Expressions translate structurally:

| PSS | BDD |
|---|---|
| `x == k`, `x != k` | equality / inequality over the bit vector |
| `x < k`, `x <= k`, `x > k`, `x >= k` | unsigned comparator circuit |
| `x in [lo..hi]` | comparator pair, ANDed |
| `x in {a, b, c}` | OR of equalities |
| `a && b`, `a \|\| b`, `!a`, `a -> b` | `and`, `or`, `not`, `or(not a, b)` |
| `a + b == k` etc. | ripple-carry adder, then compare |
| anything else | **abstain** — the whole proof returns NOT PROVED |

Abstention is per-proof, not per-constraint: if any term cannot be translated,
the result is NOT PROVED rather than a partial answer that looks complete.

### Core data structure

Hash-consed ROBDD with complemented edges:

```rust
struct Bdd {
    nodes: Vec<Node>,                    // index 0 = FALSE, 1 = TRUE
    unique: HashMap<(u32, u32, u32), u32>, // (var, low, high) -> node
}
struct Node { var: u32, low: u32, high: u32 }
```

Operations, in dependency order:
- `mk(var, low, high)` — hash-consing, the only way nodes are created
- `apply(op, a, b)` — memoised, the workhorse
- `restrict(a, var, value)` — for `sat_count` and for reporting a witness
- `sat_count(a)` — number of satisfying assignments, for "how much does this
  constraint actually restrict" (a vacuous one counts the full domain)
- `is_tautology(a)`, `is_unsat(a)` — `a == TRUE`, `a == FALSE`

### The three proofs

- **sat**: build the conjunction of the action's constraints; unsat iff the
  result is FALSE.
- **vacuity**: for each constraint `c`, test `is_tautology(c)`. A tautology
  restricts nothing — report it. Also report `sat_count(c) / domain_size` so a
  near-vacuous constraint is visible.
- **equivalence**: build both blocks, compare node identity. Canonical form
  makes this exact, not statistical.
- **order**: build under declaration order and under reverse order; report node
  counts. This is the "why variable order matters" demonstration, on the user's
  own model rather than a textbook one.

### Reporting a witness

When a constraint set is satisfiable, `restrict` down a satisfying path and print
the concrete assignment. A proof tool that says "satisfiable" without showing a
model is half a tool.

## Slice 2 — `satya reach`

```
satya reach model.pss --cover cg --unreachable
```

Fixed-point over the activity graph:

```
R = { initial states }
repeat:
    R' = R ∪ { successors(s, a) : s ∈ R, a enabled in s }
until R' == R          -- BDD equality, exact termination
```

A coverage bin is unreachable iff `R ∧ bin = FALSE`. This is where the
fixed-point lesson lives, and it is the strongest argument for BDDs in this
domain: the answer is a set, not a sample.

Scope for slice 2: finite-state models only — enum and small-range variables,
bounded `repeat`. Unbounded state space → NOT PROVED.

## Integration points in the existing code

| concern | file |
|---|---|
| constraint extraction for an action | `src/solver.rs` `collect_constraints` |
| variable domains | `src/solver.rs` `collect_variables` / `Domain` |
| new module | `src/bdd/` (new): `mod.rs`, `translate.rs`, `prove.rs` |
| CLI | `src/bin/satya.rs`, new `prove` subcommand |
| coverage bins | `src/engine.rs` `coverage_bins` |

The BDD engine must not touch the existing solver path: `satya solve` and
`satya gen` keep their current behaviour, and a model that proves NOT PROVED is
not an error.

## Test plan

1. **Unit, on the BDD core**: `apply` truth tables against brute force for all
   16 binary ops on 3 variables; hash-consing produces identical indices for
   identical subgraphs; `sat_count` equals enumeration for every BDD over 4
   variables.
2. **Translation**: for each supported form, enumerate the domain and compare
   the BDD's satisfying set against direct evaluation of the PSS expression.
   This is the test that catches a wrong comparator — the classic BDD bug.
3. **Proofs**: a known-unsat model (`x in [1..5]; x in [10..20]`) proves unsat;
   a known-vacuous one (`x >= 0` on an unsigned) is flagged; two rewrites of the
   same constraint (`x != 0` vs `x > 0 || x < 0`) prove equivalent.
4. **Abstention**: a 32-bit unconstrained integer makes every proof NOT PROVED,
   and the exit code is 2.
5. **Mutation**: break `apply`'s memoisation, or invert a comparator, and
   confirm a test fails. A proof engine that passes with a broken `apply` is
   testing nothing.

## What this does not claim

It does not replace the solver, does not make `satya solve` exact, and does not
handle 32-bit state spaces. It adds a narrow, exact reasoning path for the small
domains where coverage and constraint questions actually live — and it says
"not proved" everywhere else.
