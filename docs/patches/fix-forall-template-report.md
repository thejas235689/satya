# Rows 58 (`forall`) and 25 (templated field) — closed

**Measured, same binary.** Baseline on c92d8d2: proof `128 pass, 34 fail, 162 checks`, `cargo test` 178 (19 lib
+ 159 int). After: proof **`130 pass, 32 fail, 162 checks`**, `cargo test` **181 (19+162)**. Rows 25 and 58 pass;
no other row changed verdict.
**Setup gap:** `/scratch/input` did not exist — the four staged patches and the PSS 3.1 PDF were not uploaded. The
diff is against pristine `c92d8d2` (`git apply --check` clean) and my measured baseline (128/34, 178) is exactly
the stated one, so nothing was lost. The §16.1.x text could not be read: the instance domain below comes from the
repo's LRM Examples 128/130/132 and `docs/SEMANTICS.md`, not the PDF — **not stated** vs the standard.
## Row 58 — `forall` (src/ir.rs, src/solver.rs)
The parser recorded `is_forall` + binding; the IR dropped them, so the body reached the solver as `it.size < 5`
with `it` free. `check_atom` returns **true** for an attribute it has no value, so the atom could never fail:
`e.size` = 58, 44, 62, 71, 51 (seeds 1–5), counter `2/2`.
Fix: the IR keeps flag + binding (type mangled like a field's); `collect_constraints` expands the body once per
instance of the type in the action's instance domain (`forall_instances` — the tree `collect_variables` walks:
attributes at any depth, arrays element-wise, plus the component's rand attributes), substituting the binding with
the instance path (`subst_binding`). A body that cannot be placed on an instance is **not** added as a constraint:
it is reported `not evaluated` and counted in neither `total` nor `satisfied`. Evidence: `e.size` = 3, 1, 3, 4, 4
(all `< 5`) while `fa_b` without the forall still solves 58; `it.nosuch < 5` reports `not evaluated: … body does
not bind at 'e'` at `1/1`, not `2/2`. Component-scope forall, two instances and a nested `cc.a` instance also bind.
**Why `docs/proposed/forall-binding.patch` failed:** it binds by *trailing segments* (any var ending `.size`), not
by type, so it cannot tell instances apart; its `Atom::Unsupported` change reclassifies every ungrounded atom, which is what moved row 14; it leaves struct arrays unexpanded. A type-directed walk touches only models that
contain `forall`.
**Row 58's second assertion was unsatisfiable and is amended:** it required the forall model to equal the
unconstrained one on all 20 seeds — impossible once the body binds (`docs/proposed/README.md` records the same
contradiction). It now asserts the body holds on every seed **and** the model differs from the unconstrained one
on ≥10 of 20 seeds, i.e. the body bites. No other assertion touched.
## Row 25 — templated field (src/ir.rs)
1. `category_of("pair_s<bit[8],4>")` matched no raw type → `Unknown`, so `expand_field` produced nothing and
   `small.f` was `__ABSENT__` while the counter read `2/2 satisfied`. A name with `<` now takes its category from
   its base, keyed by the mangled name `instantiate_types` stores. `small.f` = 200, `small.g` = 33.
2. A *type* parameter as a width (`rand bit[W] f`, `W = bit[8]`) went unsubstituted — `substituted_width` parses
   numeric arguments only — so `f` fell back to one-bit `bit` and 200 was out of domain. Same for a value parameter
   as an extent: `int arr[N]` is now 4 elements, so `small.arr[3]` exists and `big.arr[2]` does not.
**Left alone.** `constraint { forall (it : T) { … } }` (block form, LRM Example 128) still passes silently:
`parse_constraint_set` parses the binding and **discards** it — unrecoverable below `src/parser.rs`, out of scope;
the direct `constraint forall (…)` form (row 58's) works. `rand elem_s es[3]` elements are not expanded as structs,
so a forall over them reports `not evaluated`, not a silent pass. `src/engine.rs` needed no change; the two
`is_forall: false,` lines added to `src/frontend/json.rs` struct literals are mechanical, no behaviour change.
