# Satya — capability roadmap

Consolidates everything identified as worth adding, ordered by what it buys.
Effort is measured in focused sessions, not hours, and every item names the
evidence that would prove it done.

## The gap that outranks all of this

**Satya never executes `exec body`.** It parses the procedural body and
translates it to target code, but nothing evaluates it. Consequence: of the
corpus's 113 runtime conformance tests, **108 cannot be checked at all**.

Every feature below is worth less than this one until it is fixed, because a
verification engineer hits it in the first ten minutes of evaluating the tool.

---

## Tier 1 — credibility (do first)

### 1.1 Procedural interpreter for `exec body`
**Buys:** 108 corpus tests move from "parses" to "verified"; `message()` and
`%n` formatting start working; `@@PSS-TRACE` output appears, which is the corpus's
own oracle format.
**Needs:** a value environment, block scoping (partly built — `ExecStmt::Decl`
landed in 0.67.0), control flow (already parsed), function calls, string
formatting.
**Proves it:** corpus compliance tier passes at the rate a reference tool does.
**Effort:** 3–4 sessions.

### 1.2 Close the 34 failing proof rows
**Buys:** the tool stops producing wrong answers without complaint.
**Proves it:** `run_all.sh` at 162/162.
**Effort:** 1 session per 2–3 rows; ~15 sessions for the structural remainder.

### 1.3 Corpus runtime conformance in CI
**Buys:** the claim in `docs/CORPUS.md` matches the evidence. Today it reports
parse-level results that read like conformance.
**Proves it:** a per-release conformance figure with the tier named.
**Effort:** 1 session after 1.1.

---

## Tier 2 — exactness

### 2.1 SMT backend for solving
**Buys:** `satya solve` becomes **exact**. Today it is inversion + propagation +
hill-climbing: it can return a value that violates a constraint and report "not
satisfied" rather than searching harder. PSS constraints are bit-vector and
integer arithmetic — `QF_BV`/`QF_LIA`, SMT's home turf.
**Design:** `--solver smt` behind a feature flag linking z3, so the default
build keeps **zero runtime dependencies**. A tool that currently has none should
not acquire a heavyweight one by accident.
**Proves it:** a constraint set with no solution reports unsat instead of
returning a violating value; solved models agree with a reference SMT run.
**Effort:** 2–3 sessions.

### 2.2 BDD prover — `satya prove`
See `docs/BDD-DESIGN.md` for the full design. Scope: `bool`, enum,
`bit[N]` N<=8, ranges <=256 values. Everything else reports **NOT PROVED**.
**Buys:**
- **vacuity** — detects constraints that restrict nothing (a silent no-op class
  nothing in the commercial flow catches)
- **equivalence** — canonical form makes it a pointer comparison
- **sat** — exact satisfiability on small domains
- **order** — the variable-ordering demonstration, on the user's own model
**Proves it:** known-unsat and known-vacuous models detected; two rewrites of the
same constraint proved equivalent; a broken `apply` makes a test fail.
**Effort:** 2–3 sessions (ROBDD core is ~500 lines, dependency-free).

### 2.3 BDD reachability — `satya reach`
Fixed-point over the activity graph; an unreachable coverage bin is
`R ∧ bin = FALSE`.
**Buys:** coverage closure becomes a proof instead of an argument — the strongest
differentiator, because no commercial PSS tool offers it.
**Proves it:** a bin known unreachable from the constraints is reported
unreachable; a reachable one is not.
**Effort:** 2 sessions after 2.2.

---

## Tier 3 — coverage of the standard

Each of these is a construct group from the 34 rows, needing machinery rather
than a patch.

| item | buys | needs |
|---|---|---|
| `forall` expansion | rows 58 and the constraint families built on it | an instance tree |
| `overlap` / monitor semantics | rows 74, 76, 77 | schedule carrying time spans, not a flat step list |
| Claims and executors | rows 119, 121, A126–A128 | a component tree with address-space scoping |
| `ref` / `state` / flow identity | rows 16, 47, 48 | object identity in the value model |
| Map literals, `set` dedup | rows 6, 15 | keyed storage; values are a flat `i64` map today |
| Symbol table | rows 104, 87, 84, 86, 102 | the parser tracking declared names |

The symbol table is the highest-leverage single item here: one prerequisite
unlocks five rows.

---

## Tier 4 — engineering quality

- **Unsigned rendering** (row 9): values are `i64`; the JSON and trace backends
  format signed, so 2^64−1 prints as −1. Needs a width/unsignedness flag on the
  field rather than a formatter patch. **1 session.**
- **`--seeds` everywhere**: the sweep tools still launch one process per seed;
  `--seeds N` exists but the proof harness and vector generators don't use it.
  A full proof run drops from ~2 min to seconds. **1 session.**
- **Template substitution with per-step values**: a patch exists
  (`fix-substitution.patch`) but bakes the *first* step's values into a per-action
  function, so `repeat (3)` emits the same operation three times. Needs the
  target code emitted inside the per-step block. **1–2 sessions.**

---

## What I would not add

- **A full formal property checker.** SAT/SMT-based RTL formal is a mature
  market with decades of engine work behind it. Satya should reason about *PSS
  models*, not replace JasperGold.
- **A BDD-based general solver.** A BDD over 32-bit PSS integers is a memory
  leak. The eligibility rule in the BDD design is what keeps that honest.
- **Anything that makes a construct "parse and look handled" without behaviour.**
  That is the failure class this project has spent twenty releases removing:
  `foreach`, `unique`, `soft`, `bins all[N]`, `import`, `bind`, `addr_value_abs`,
  `exec body <lang>`, `int t = 7;` — each once reported as supported while doing
  nothing.

---

## Order of work

```
1.1  exec-body interpreter        <- unblocks everything
1.2  the 34 rows                  <- removes wrong answers
1.3  corpus conformance in CI     <- makes the claim true
2.1  SMT backend (exact solve)    <- makes solving correct at scale
2.2  BDD prover                   <- the differentiator
2.3  BDD reachability             <- coverage as proof
3    structural rows              <- standard coverage
4    engineering quality
```

Tier 1 is not optional. A tool that proves an unreachable bin while silently
not running the test body is worse than useless in a verification flow — it is
misleading, which is the one thing a verification tool must never be.
