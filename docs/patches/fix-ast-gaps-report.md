# Closing proof rows 43, 80, 100, 103

Measured here: **128 pass / 34 fail** at HEAD c92d8d2 (tree *without* the four staged patches — see Deviations). After this patch: **132 pass / 30 fail**; `cargo test` **178/178** (19 lib + 159 integration), unchanged.

| row | before | after | change |
|---|---|---|---|
| 43 symbol in action body | 1 step, unresolved | **PASS** (A/B/C) | `symbol` branch in `parse_action_body`; fragment on `ActionDecl.symbols`→`ir::Action.symbols`; `dispatch` falls back to a symbol before "unresolved"; `SymbolCall` checks action scopes first, then the component |
| 80 action-scope override | x=58 (want 91) | **PASS** (91) | `ActionDecl.overrides`, `parse_override_list` shared by both forms, `apply_override` reads enclosing action scopes innermost-first |
| 100 `randomize … with` | func3(5)=0 | **PASS** (6) | `ExecStmt::Randomize(RandomizeStmt{targets,constraints})` — still one field, so `backend/*` compiles untouched; both parenthesised and bare forms parsed; attributes re-solve with the clause; locals searched |
| 103 `compile if` in exec body | fixture failed | **PASS** | `try_parse_exec_stmt` selects the branch at parse time; only the selected branch is in the model |

## Ruled out

- **43: the fixture is right, the grammar was narrow.** PSS 3.1 Annex B.2 lists `symbol_declaration` under `action_body_item` (same production in the 1.0 grammar and errata), so a `symbol` in an action body is legal. Verified: component-scope symbols still expand (3 steps); an action-body symbol is invisible to a sibling action (still `unresolved action two`); `two();` works; a nested action sees the outer action's symbol.
- **103 does not reproduce as an exec-body bug at this commit.** Before any change, `compile if (0) {a=1;} else {a=2;}` in `exec post_solve` already gave a=2, and the declaration/nested forms were right — because `compile` was swallowed as an unparsed statement and the `if` was re-read as a *runtime* conditional. Right answers, wrong mechanism (a compile-time condition evaluated at run time). Now selected at parse time; the `Raw("<unparsed>")` is gone and the disabled branch is not in the model. Every earlier probe re-runs identically.
- **103's real failure was a broken assertion.** `m103_body` demanded `solved_val kept` = 0, but `kept` is an unconstrained `rand int` (69/668/469 at seeds 1/2/3) — unsatisfiable at any seed. It now asserts what the construct means: `kept` present, `gone` absent, neighbour `v`=1; I added two exec-body cases to the same fixture. Only fixture edit.
- **100: AST shape confirmed, not parentheses.** `randomize(x) with {x>50;}`→x=55; plain `randomize(x)` still re-solves in its domain (x=20); an unsatisfiable local clause (`res>100000; res<100001;`) now **warns** instead of returning a forbidden value.
- Untouched behaviour re-checked: component-scope overrides, override precedence (inner 33 beats outer 91), the other 158 proof checks, all five backends, `satya demo`.

## Deviations

1. **The four staged patches were absent** — `/scratch/input` did not exist here (no `*.patch`, no PSS 3.1 PDF). This diff is against **c92d8d2 alone**; `git apply --check` passes on that tree, but it is untested on top of the four patches. Rows 79/83/84/102/104 still fail here for that reason (128+5=133/29 matches your expected post-patch baseline, so patched tree + this diff should read 137/25).
2. **`src/frontend/json.rs` touched** — one two-field initialiser, required by the new `ir::Action` fields. Nothing in `solver.rs`, `backend/*`, `bdd/*`, `docs/*` changed.
3. **PSS 3.1 text**: PDF unavailable and `accellera.org` is not on this sandbox's egress allowlist, so B.2 was confirmed from the published 1.0 formal grammar + errata (same production).
4. **Local `randomize` is a bounded search, not a solver**: clause-named values, then a seeded 256-value sample in [0,4095]; if none satisfies, it warns. A clause needing a value outside that space warns where a real solver would succeed.
