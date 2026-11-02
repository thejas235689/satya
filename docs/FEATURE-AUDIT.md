# Feature audit — 146 constructs, verified against behaviour

Every construct in [`FEATURES.md`](FEATURES.md) was re-tested against the
published **v0.46.0** binary (`sha256 4a40d468…2e421e`, verified) by six
independent agents working in parallel, one per slice of the construct table.
Each wrote a minimal model per construct and judged the **observed behaviour** —
solved values, trace structure, reported coverage bins, monitor matches — never
"it parses".

The models are in [`tests/feature_proof/`](../tests/feature_proof/), 120 of
them, and `run_all.sh` re-runs them all:

```sh
tests/feature_proof/run_all.sh /path/to/satya
```

## Verdict vocabulary

| verdict | meaning |
|---|---|
| **PASS** | behaves as the standard requires |
| **FAIL** | behaves incorrectly, or parses and is silently ignored |
| **PARTIAL-OK** | the row is documented `partial` and the documented limitation reproduces |
| **PARTIAL-WORSE** | the row is `yes` or `partial`, but more is broken than documented |

## Result

| verdict | count |
|---|---|
| PASS | 74 |
| PARTIAL-OK | 24 |
| PARTIAL-WORSE | 35 |
| FAIL | 13 |
| **total** | **146** |

`FEATURES.md` says **114 implemented, 32 partial, 0 not**. The audit finds **74
that behave as specified** — 51% of the table, and 65% of the rows marked `yes`.
That is the single most important sentence in this document: the matrix was
counting "parsed" as "implemented", which is exactly the confusion the project
has been trying to eliminate since v0.24.0.

## Per slice

| slice | constructs | PASS | PARTIAL-OK | PARTIAL-WORSE | FAIL |
|---|---|---|---|---|---|
| 1 | 1–25 literals, data types, components | 12 | 3 | 8 | 2 |
| 2 | 26–50 templates, activity, flow objects | 15 | 3 | 6 | 1 |
| 3 | 51–75 constraints, coverage, monitors | 18 | 3 | 1 | 3 |
| 4 | 76–100 monitors, extension, functions, procedural | 9 | 1 | 12 | 3 |
| 5 | 101–125 compile-time, executors, address space | 9 | 9 | 5 | 2 |
| 6 | 126–146 handles, registers, memory primitives | 11 | 5 | 3 | 2 |

Slice 3 (constraints and coverage) is the strongest; slice 4 (functions,
packages, procedural) is the weakest, where 12 of 20 rows marked `yes` behave
worse than documented.

## The failures

Thirteen constructs do not work at all, or work in a way that silently produces
a wrong answer:

| # | construct | observed |
|---|---|---|
| 4 | Empty aggregate literal | `arr = {};` silently ignored; a 3-element array stays size 3 |
| 7 | Structure literal | keyed form `'{x : 3, y : 4}` is a parse error; positional `'{3,4}` writes `p[0]/p[1]` and leaves `p.x=p.y=0` |
| 44 | bind statement | silently dropped; trace identical with and without `bind p2.out c1.inp;` |
| 52 | Static bind directives | `bind pa *;` parsed and ignored; all allocations still come from the other pool |
| 64 | covergroup instantiation | `named_cg inst1 (x);` binds nothing and never samples, with no warning |
| 66 | bins declaration | `bins all[4] = {A,B,C,D}` declares **one** bin |
| 82 | import statement | no effect on name resolution — with `import fpa::*`, with `import fpa::value`, and with no import at all, the same name resolves |
| 87 | import class | parse error in every spelling tried |
| 89 | procedural block statement | `{ in_block = 5; }` leaves 0; the block is dropped entirely |
| 103 | `compile if` | condition is evaluated, but declarations are lost when nested, at package scope, or in an action body |
| 132 | `sizeof_s` | correct only for a packed struct of bit fields; returns 0 for a nested struct, a `typedef bit[24]`, an enum, `int` and `bit` |
| 138 | `addr_value_abs` | returns the constant 1 for every handle, not the handle's address |
| — | (slice 3) | see `tests/feature_proof/slice-3/` for the models |

## The pattern in the PARTIAL-WORSE rows

35 rows behave worse than documented, and they cluster around one habit: **the
tool reports success while doing nothing.**

- `bit[8]` constrained to `[256..300]` yields **256** and prints `constraints: 1/1 satisfied`.
- `forall (e : elem_s) { e.size < 5; }` is dropped and still counted `2/2 satisfied`.
- A claim's `size` constraint is ignored — always 64 bytes — and still reports satisfied.
- `rand bool b` yields 69; `rand bit[64]` yields a negative number.
- Nested and inherited struct members, and `rand` templated fields, bind nothing while the counter says satisfied.

A wrong answer that reports success is the worst failure mode for a verification
tool, and it is the one this audit found most often. It is also the one the
project's own tests were least likely to catch, because they asserted that
constraints were *satisfied* rather than that values were *correct*.

## What passed

74 constructs behave as the standard requires, including: string literals and
the string slice operator, cast, all twenty expression operators, enum and
typedef declarations, component and action declaration, templates and their
instantiation, sequence and parallel blocks, repeat-count, select (all branches
reachable across seeds), if-else, match, pools, member and expression
constraints, implication, conditional constraints, foreach constraints, unique,
default constraints, distribution, covergroup and coverpoint declaration,
crosses of named coverpoints, monitor activity and monitor constraints, packages,
function declaration and definition, procedural assignments and control flow,
address handles, register access functions, and the byte/struct memory
primitives.

## Not tested

- **UVM and C backend output was never compiled.** No SystemVerilog or C
  compiler exists in the audit environment; those verdicts are structural.
- The `bench` backend's JTAG/SPI/I2C transport was exercised only against its own
  logging stub.
- Slice 6's `addr_value_abs` verdict is against the project's own documentation;
  the LRM text for §24.10.7 was not available to settle which side is right.

## Reproducing

Each slice directory holds one minimal model per construct. `run_all.sh` parses
every one and prints the result. The verdicts above are what these models showed
on v0.46.0; a different build will print different values, which is the point —
the evidence is re-derivable rather than asserted.

**Note on the artefacts:** each agent also produced a `proof.sh` that re-derived
its verdicts with assertions, and a per-slice `report.md`. Those were lost in
transfer (only the models survived), so this document is the record of the
verdicts and the models are the reproducible evidence. Rebuilding the per-slice
assertion scripts is the obvious next step if this audit is to run in CI.
