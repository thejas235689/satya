# Engine-level construct gaps — fix report

Only `src/engine.rs` and `src/ir.rs` changed (`fix-engine-constructs.patch`, +72/−4).
Proof: **130 pass / 32 fail** (was 128/34). `cargo test`: **178 → 178** (19 unit + 159 integration;
the brief's "173" is a stale count — the pristine tree measures 178 too, so nothing regressed).

| Row | Before | After | Flip |
|-----|--------|-------|------|
| 79 type extension | `base_exec=1`, `ext_field=0` | `base_exec=2`, `ext_field=99`, `x=77` | **yes** |
| 83 composite exec | `MARK composite_post` absent | present; `cfg=21 seedv=40 x=42` | **yes** |
| 43 symbol | 1 step, `two` unresolved | 3 steps for a *component-scope* symbol; row still 1 step | no |
| 80 action override | `x=58` | `x=58` | no |
| 100 `randomize … with` | `func3(5)=0` | `func3(5)=0` | no |

**79.** The two earlier fixes hold (parser yields leaf `A`; merge guard matches). Instrumenting the
merged IR found the third: `ir::build_actions` merges an `extend action` carrying fields, constraints
and activity **but not `execs`** — the merged `A` had `execs=[post_solve(1 stmts)]`, the base's only.
Fix: `existing.execs.extend(extra.execs)`, appended after the base's so `post_solve` runs base→extension.

**83.** `record_step` was the only place execs ran, and a composite action is walked without being
recorded, so its execs were dead. Fix in `walk_action`: container `pre_solve` before its solve (fed to
`solve_with`), `post_solve`/`pre_body` after the activity, `exec_action`/`exec_component` set to it.

**43.** Real engine gap, fixed: `dispatch` resolved a traversal only to an *action*, so `two;` became
`[UNRESOLVED]`. It now falls back to the component's `symbols` and walks the body in place — a
component-scope `symbol two { do A; do B; }` now gives 3 steps A/B/C. The row still fails for a cause
outside scope: the fixture declares the symbol **inside** the action body, where `parse_action_body`
has no `symbol` branch. The AST shows a member of type `symbol` named `two` (body discarded) and
`activity: None` — even `activity { two; do C; }` is swallowed, so nothing reaches the IR.

**80.** `apply_override` only sees `Component.overrides`; the action-scope `override { … }` is eaten by
`skip_to_semicolon` in `parse_action_body`, and `ast::ActionDecl` has no field to hold it (AST of `t`:
`members=[]`, `constraints=[]`, no override anywhere). Needs `ast.rs` + `parser.rs`.

**100.** `ExecStmt::Randomize(Vec<Path>)` has no constraint slot and the parser accepts only
`randomize (...)`: `func3` lowers to `Raw("<unparsed>")`×3 + `If(1)`, text gone. Ruled out the narrower
case too — `randomize(x) with { x > 50; }` ignores its clause (`x=20`) — so the loss is the AST shape. `src/parser.rs`, `src/ast.rs`, `src/backend/*`, `src/solver.rs`, `src/bdd/*` left untouched as
instructed; rows 80, 100 and the rest of 43 need the parser/AST work owned by others.
