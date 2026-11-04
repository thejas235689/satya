# Proposed, not landed

Patches that are worked, tested, and **deliberately not merged**. Each one has a
reason recorded; none of them is here because it was forgotten.

## `forall-binding.patch` — enforce a `forall` constraint body

**Status: held, not merged.** Applies to the `src/solver.rs` of **v0.49.0** (the
tree before v0.50.0); it will need rebasing before it applies to current `main`.

### What it fixes

A `forall` body reaches the solver with its bound variable still free, because
the IR drops `is_forall` and the binding (`src/ir.rs` — `Constraint` carries
only `name`, `expr`, `is_default`, `is_disable`, `is_soft`). An atom on an
attribute the solver has no domain for can never fail, because `check_atom`
returns `true` for a missing value. So the body is silently satisfied.

The patch binds ungrounded references by their **trailing segments** — the same
rule `eval` already uses — so `it.size` binds to the attribute in scope ending
in `.size`. Where it finds one candidate (or several under the same root) it
emits one atom per instance; where it finds none, or candidates under different
roots, it emits `Atom::Unsupported`, so the constraint is reported
**not evaluated** rather than satisfied.

### Evidence it works

`tests/feature_proof/slice-3/c58_forall.pss`, `forall (e : elem_s) { e.size < 5; }`,
declared range 0..100:

| | seeds 1–5 `e.size` |
|---|---|
| before | 58, 44, 62, 71, 51 — counter `2/2 satisfied` |
| after | 3, 1, 3, 4, 4 |

`cargo test`: 134 passing. No change to `collect_variables`, `type_nbits`,
`domain_ok`, `Domain`, `repair` or `adjust`.

### Why it is not merged

1. **Combined with the v0.50.0 width and arithmetic patches it enforces
   nothing.** Re-applied on top of both, the body reports
   `not evaluated: e.size < 5` instead of being enforced — better than a silent
   pass, but not the fix the patch achieves on its own.
2. **It changes proof row 14** (`m_flat=6`, expected 5) — a behaviour change in
   struct-member binding, outside the patch's intended scope and not
   investigated.
3. **Total proof movement from merging it: zero** — 105 pass / 57 fail with or
   without.

Landing it would have raised a count without fixing the behaviour, so it stays
here.

### What a merged version needs

- Rebase onto current `main` (the patch predates the v0.50.0 solver changes).
- Resolve the row-14 interaction: find out why binding by trailing segment
  changes `m_flat`, and either scope it more narrowly or fix the underlying
  cause.
- Extend to arrays of a struct type (`rand elem_s es[3]` still reports
  `not evaluated`; per-element expansion is not done).
- Amend `proof-slice-3.sh` row 58, which currently asserts the forall model
  equals the unconstrained one — a test-design contradiction that cannot pass
  once the constraint is enforced.
