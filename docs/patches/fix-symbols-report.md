# fix-symbols — a declaration set in Satya's parser

**Change.** `src/parser.rs` keeps `decls`: every declared name (struct/state/buffer/resource,
enum + variants, typedef, static const, function/task, action, component, package, `import
class`), as leaf and `scope::leaf`, filled as declarations are parsed. `compile has (X)` answers
from it (§19.3) instead of the constant `true`. Forward references are covered by re-parsing to a
fixed point once the file's set is complete — only when a `compile has` answered "no" *and* the
set grew, so a file without `compile has` is still parsed once. `export` is parsed, not skipped.
`src/ir.rs`: a prototype no longer shadows a later definition, and `Model::exports` gates a
cross-package `do pkg::A`. `src/frontend/json.rs`: one forced line (`exports: Vec::new()`).
Backend/bdd/solver untouched.

## Per row, measured on the base and fixed binaries
| row | asserted | before | after | |
|---|---|---|---|---|
| 104 | `--action else_a m104_no.pss` (`compile has(NO_SUCH_TYPE_ANYWHERE)`) | exit 1 (then) | exit 0 (else) | **flips** |
| 84 | `r = fn_p::sum10(4,2)` **and** unqualified `sum10(4,2)` | 0, 0 | **42, 42** | **flips** |
| 102 | `solve m102_exp.pss` (`export target c::A1`) / `m102_noexp.pss` | 0 / 0 | 0 / **1** | **flips** |
| 86 | `r = twice(21)`, `fp::twice` a prototype only | 0 | 0 | **no flip** |
| 87 | `r = h.get(7)`, `get` a prototype in `import class` | 0 | 0 | **no flip** |

`--action then_a m104_yes.pss` (`compile has(c)`) is exit 0 before and after. 84 needed two fixes:
the qualified call did reach the model, but the prototype above the definition won the `functions`
entry, so *neither* spelling computed. Also verified: definition then prototype → 42; a
forward-referenced type is `true`; a name declared only in a disabled `compile if` branch is
`false`; `compile has (p::S)` resolves.

**86 and 87 cannot pass without inventing a value, so I left them.** Both declare the callee as a
prototype with no body anywhere in the file. The harness asserts the call equals what it yields
with a body present (42), resp. equals its own argument (7); nothing in the model derives either,
so any non-zero answer would be fabricated — worse than the present silent 0. The honest answer is
a "foreign binding missing" diagnostic; I added none, as the suite asserts `s.warnings.is_empty()`.

## Regression
- `run_all.sh`: **128/34 → 131/31**. Failure sets diffed by row id: newly passing 84, 102, 104;
  **newly failing: none** (86, 87, 103 and the rest already failed at baseline).
- `cargo test`: **0 failed before and after**, 178 passing (159 integration + 19 lib). This
  checkout has 19 lib tests, not the brief's 14 — measured on the unmodified tree.
- All 237 `examples/**/*.pss` give identical `check`/`solve` output on both binaries, bar
  `examples/lrm/042_…`, which differs between two runs of the *baseline* binary too.
