# Satya — Verification Report

**Subject:** Satya 0.1.0, portable test-stimulus engine (Rust)
**Verified against:**
- *Portable Test and Stimulus Standard 3.0*, Accellera, August 2024 (485 pp.)
- *PSS Workshop Examples, DVCon 2025* — 11 `.pss` files and 6 `.psf` project files

**Date:** 21 October 2026
**Revision:** 2 — refreshed for Satya 0.2.0. The verification below was performed
on 0.1.0; the findings are unchanged, but several limitations recorded at the
time have since been closed (name resolution, arbitration, pool capacity,
`match`, `select` guards, arithmetic, `exec` translation). Those rows are
updated here and marked *closed in 0.2.0*.
**Revision:** 3 — adds §7, the feature-proof harness, recording the behavioural
audit of **v0.46.0**. Sections 1–6 are unchanged from the 0.2.0 pass and are a
**grammar and parse** verification; they do not establish behavioural
conformance, which §7 measures and which is materially lower.

---

## 1. Verdict

Satya was checked against both artefacts and **defects were found and fixed**. The
initial build parsed 9 of 11 workshop `.pss` files; the current build parses
**all 11** and compiles **all 6** `.psf` projects, and every project now solves
end-to-end with a deterministic default action.

Six defects were found: four parser defects that rejected valid PSS, one engine
defect that silently produced an incomplete scenario, and one usability defect
where a missing action name produced an empty result instead of an error. All
six are fixed and covered by tests.

The claims in the README and design document have been corrected where they
over- or under-stated what the implementation does.

**What this pass does not establish.** Everything above is a parse-level
result: it proves that valid PSS is accepted and that the models compile and
solve. It does not prove that a construct which parses behaves as the standard
requires. A behavioural re-test of all 146 constructs on v0.46.0 (§7 and
[`FEATURE-AUDIT.md`](FEATURE-AUDIT.md)) found **35 rows behaving worse than
documented and 13 not working at all**, including constructs that report
`constraints: 1/1 satisfied` while returning a wrong value. The statement that
defects were "found and fixed" is true of the defects a *parse* pass can find;
it is not a claim that the implementation is behaviourally correct.

| Check | Before | After |
|-------|--------|-------|
| Workshop `.pss` files parsing | 9 / 11 | **11 / 11** |
| Workshop `.psf` projects compiling | not supported | **6 / 6** |
| Projects solving end-to-end | 3 partial | **6 / 6** |
| Integration tests | 10 | **14** |
| LRM grammar constructs accepted | — | 29 / 29 probe snippets |

---

## 2. Method

1. The standard was converted to text and its BNF (Annex B, p441–464) and clause
   grammar were read directly, so every construct Satya claims to support could
   be checked against the normative production.
2. Every `.pss` file and every `.psf` project in the workshop archive was
   compiled with the release build; failures were bisected to a minimal
   reproducer and traced to the responsible code.
3. A 29-item probe of LRM grammar productions was compiled, and each probe was
   then classified as *modelled* (produces model content) or *accepted but not
   executed* (parsed and skipped) by inspecting the resulting IR.
4. The solver and engine were exercised on the real models, checking constraint
   satisfaction counts, value ranges, seed reproducibility and step counts.

---

## 3. Results against the workshop examples

### 3.1 Source files (`.pss`)

All 11 parse and lower successfully.

| File | Result |
|------|--------|
| `1_interoperability.pss` | ok |
| `2_ddr.pss` | ok (was a hard failure) |
| `3_1_executor_ip_pkg.pss` | ok |
| `3_2_executor_ip_top.pss` | ok |
| `3_3_executor_soc_top.pss` | ok |
| `4_local_mem.pss` | ok (was a hard failure) |
| `5_hypervisor/proj_pkg.pss` | ok |
| `5_hypervisor/hypervisor.pss` | ok |
| `5_hypervisor/hyp_top.pss` | ok |
| `src/pssm.pss` | ok (was a hard failure) |
| `src/pssm_hyp.pss` | ok (was a hard failure) |

### 3.2 Projects (`.psf`)

The `.psf` files list several `.pss` files to compile together (`-pss <file>`).
This is a tool convention rather than a standard construct, so Satya gained
multi-file and `.psf` support during this verification. All six projects compile:

| Project | Files | Components | Actions | Default root | Steps |
|---------|-------|-----------|---------|--------------|-------|
| `1.psf` | 1 | 3 | 5 | `test` | 1 |
| `2.psf` | 1 | 4 | 8 | `complex_test` | 8 |
| `3_ip.psf` | 2 | 2 | 3 | `test` | 2 |
| `3_soc.psf` | 2 | 3 | 5 | `soc_test` | 2 |
| `4.psf` | 1 | 3 | 12 | `test` | 3 |
| `5.psf` | 4 | 4 | 12 | `test_0` | 3 |

`3_ip.psf` is the important one: it compiles `3_1_executor_ip_pkg.pss` together
with `3_2_executor_ip_top.pss`, whose `extend component pss_top { … }` must be
merged into the component declared in the first file. The merge is correct.

---

## 4. Defects found and fixed

Each defect is stated with the normative grammar that proves it was a defect.

### D1 — `do <action> with { … }` rejected

**LRM:** Clause 12.3, and Annex B.9 (p447):
`activity_action_traversal_stmt ::= … do type_identifier inline_constraints_or_empty`
with `inline_constraints_or_empty ::= with constraint_set | ;` (p131, p447).

**Symptom:** `error: expected ';', found 'with'` at `2_ddr.pss:165` and
`4_local_mem.pss:247`.

**Root cause:** the parser only accepted `do X;` and `do X { … }`; the
`with` form was not handled.

**Fix:** inline constraint sets are parsed into a dedicated AST node; the solver
applies them to the action instance (`solve_with`), and the engine dispatches
them via `dispatch_constrained`. Inline constraints take precedence over the
action's own constraints, and a genuine conflict is reported in the
satisfied/total count rather than hidden.

### D2 — nested template closing `>>` lost

**LRM:** Clause 11, *Template types* (p121–128).

**Symptom:** `error: expected '{', found end of input` for `pssm.pss`, which
declares
`buffer data_buf <struct TRAIT = mem_trait_s, struct CLAIM = mem_segment_s<TRAIT>> { … }`.

**Root cause:** the lexer emits `>>` as a single token (the shift-right
operator). When a template's default value itself ended in a template, the two
closing `>` characters arrived as one token and the second was swallowed, so the
declaration never terminated.

**Fix:** template parsing now splits a `>>` token into two `>` as needed
(`eat_gt`), in both the parameter list and the default-value scanner. This is
the standard ambiguity resolution for `a<b<c>>`.

### D3 — constraint implication-to-block and `if/else` constraints rejected

**LRM:** Clause 16.1, and Annex B (p453):
`implication_constraint_item ::= expression -> constraint_set` (p199) and
`if_constraint_item ::= if ( expression ) constraint_set [ else constraint_set ]` (p200).
A `constraint_set` may be a `{ … }` block.

**Symptom:** `error: expected ')', found …` at `pssm_hyp.pss:64`, in
`constraint initial -> { foreach (page_table_done : mc_state.vm_page_table_done[i]) { … } }`.

**Root cause:** two related gaps. A `{ … }` in an expression was parsed as a
comma-separated list rather than a constraint block, so `expr -> { … }` failed;
and the constraint `foreach` parsed its collection as a *type* rather than an
*expression*.

**Fix:** a `{ … }` in constraint position now parses a constraint block
(conjoined); `if/else` constraint items are parsed; and the constraint `foreach`
takes an expression, per `foreach ( [iterator_identifier :] expression
[[index_identifier]] ) constraint_set` (p447). The trailing `;` after a
constraint item is now accepted as optional, since real models omit it.

### D4 — ranged bit widths `bit[msb:lsb]` rejected

**LRM:** Clause 7, *Data types*; the LRM's own examples use `rand bit[3:0] f1;`
(e.g. the action-traversal example at p132).

**Symptom:** `error: expected ']', found ':'`.

**Root cause:** only the single-width form `bit[N]` was parsed.

**Fix:** a bit-range parser accepts `[N]` or `[msb:lsb]`, computing the width of
the latter as `msb - lsb + 1`. (The workshop examples happen to use only `bit[N]`,
so this was found by checking the LRM examples, not the workshop set.)

### D5 — `repeat` / `replicate` counts not solved before use

**LRM:** Clause 12.4 and 12.5, and Annex B.9:
`activity_repeat_stmt ::= repeat ( [index_identifier :] expression ) activity_stmt` (p155, p447)
`activity_replicate_stmt ::= replicate ( [index_identifier :] expression ) [label :] labeled_activity_stmt` (p162, p448).

**Symptom:** `5.psf` action `test_0` schedules 1 step instead of `count` steps,
where `count` is a random field of the action
(`parallel { replicate (count) { do app_c::write_data; }; }`).

**Root cause:** the engine evaluated the repeat count against model constants
only, so a random field resolved to "unknown" and defaulted to 1.

**Fix:** the engine now solves a container action before walking its activity
and threads the solved values through the walk. `test_0` now yields a
seed-dependent count within `[1..12]` (3 steps at seed 1, 10 at seed 7).
The `repeat (i: n)` index is additionally bound while walking each iteration,
so inline constraints such as `do B with { size == i*10; }` solve to
0, 10, 20, 30 across the four iterations, as intended.

### D6 — missing action name produced a silent empty scenario

**Symptom:** `satya solve 2.psf --action test` printed a 0-step scenario,
because `2_ddr.pss` has no action named `test`.

**Fix:** an explicitly requested action that does not exist is now a hard error
listing the available actions; and the default action choice (previously
dependent on hash-map iteration order, and therefore non-reproducible between
runs) is now deterministic: `test`, then `soc_test`, then a sorted activity of
the top component. Verified identical across repeated runs.

---

## 5. Conformance against the standard

Probe snippets were compiled for 29 grammar productions. All are *accepted*
(no parse error). They divide into constructs that are **modelled** (they
produce model content that the solver and engine use) and constructs that are
**accepted but not executed** (parsed and skipped so that a real file still
compiles). The distinction matters and is stated plainly.

### 5.1 Modelled

| Construct | Clause | Evidence |
|-----------|--------|----------|
| `struct` / `resource` / `buffer` / `state` declarations | 7, 13, 14 | fields and constraints lowered |
| `enum`, `extend enum` | 7 | variants recorded |
| type inheritance and `extend` | 20 | base members merged |
| `component`, component inheritance, `extend component` | 9 | sub-components resolved; `3_ip.psf` merge verified |
| `action`, `abstract action`, action inheritance | 10 | fields and constraints inherited |
| `lock` / `share` claims | 14 | lowered as action fields |
| `exec` blocks (`init_up`, `init_down`, `pre_solve`, `post_solve`, `body`, `pre_body`) | 10 | captured (body not interpreted) |
| `constraint` — named, `default`, `forall`, expression form | 16.1 | solved |
| `constraint` implication, `if/else`, blocks | 16.1 | solved / reported |
| inline `in [lo..hi]` on a member | 16.1 | solved |
| `pool`, `pool [N] T p`, `bind p *` | 15 | recorded in the IR |
| `compile if ( … ) { … } else { … }` | 5 | primary branch taken |
| activity: `do`, `do … with { … }`, `parallel` (+ join specs), `schedule`, `select`, `repeat`, `replicate`, `foreach`, `if/else`, `atomic`, `sequence`, labels, single-statement bodies | 12, Annex B.9 | scheduled |
| `bit[N]`, `bit[msb:lsb]`, templated types | 7, 11 | typed and sized |

*Modelled* here means the construct produces model content that the solver and
engine consume — it does not mean that content is correct. The v0.46.0 audit
re-tested these same rows on behaviour and found 74 of 146 passing: `bind p *`
is recorded in the IR but ignored, `import` does not affect name resolution, and
`compile if` loses declarations in nested scope (§7,
[`FEATURE-AUDIT.md`](FEATURE-AUDIT.md)).

### 5.2 Accepted but not executed

These parse (so a real file still compiles) but Satya does not act on them.
Each is stated in the README and the design document.

Closed in 0.2.0 (previously in this list):

| Construct | Clause | Behaviour now |
|-----------|--------|---------------|
| `match` statement | 12, Annex B.9 | **executed** — `[lo..hi]`, `[v, lo..hi, …]` lists and `default` |
| pool allocation and `lock`/`share` arbitration | 14, 15 | **executed** — objects allocated/consumed; contention enforced by serialising |
| `select` branch guards `(expr) : …` | 12.4 | **guards evaluated**; first satisfiable branch taken |
| `foreach` over a pool | 12.4 | **iterates the pool's allocated objects** |
| raw `exec` bodies | 10 | **parsed into statements** and translated into the C and UVM backends |

Still accepted but not executed:

| Construct | Clause | Behaviour |
|-----------|--------|-----------|
| `function` / `task` bodies | — | skipped |
| `covergroup` / `coverpoint` | 18, 19 | skipped |
| `typedef` | 7.11 | skipped |
| `symbol` declarations | 12.7 | skipped |
| `super` in an activity | Annex B.9 | accepted, no-op |
| `solve x before y` | 16.2 | accepted inside a constraint block but not honoured |
| foreign calls inside `exec` (`make_handle_from_claim`, …) | 10 | translated as comments in C, passed through in SV — not implemented |

---

## 6. Solver and engine verification

* **Constraint satisfaction.** On `2.psf` (`complex_test`), the eight scheduled
  `hammer_row` steps report 6/6 constraints satisfied. On `1.psf` and `4.psf`
  the reported counts match the constraints declared.
* **Value ranges.** Solved values were checked to lie inside their declared
  domains, including equalities that link a flow object's field to a local field
  (`data_buf_out.size == size` yields identical values).
* **Seed reproducibility.** The same seed yields the same scenario; different
  seeds yield different, still-legal scenarios (verified on `test_0`).
* **Inline-constraint precedence.** Where an inline `with` constraint conflicts
  with an action constraint, the inline constraint wins and the conflict is
  reported in the satisfied/total count rather than silently dropped.
* **Determinism of the default action** was verified across repeated runs.
* **Flow objects and pools (0.2.0).** On `2.psf` the eight `hammer_row` steps
  each allocate a `data_buf` object from `data_buf_p` (objects #0–#7), and the
  pool summary reports seven allocated. `input` fields consume the next object.
* **Address allocation (0.2.0).** Regions declared in `exec init_up` are
  discovered (`mem_region@0x400+0x1000` in `1.psf`) and allocated objects
  receive 64-byte-aligned addresses from the region base.
* **Arbitration (0.2.0).** Two exclusive claims on a single-instance resource in
  one parallel group are detected and the conflicting steps are serialised.
* **Arithmetic (0.2.0).** `x + y == 100` with `x >= 20` solves for all seeds
  tested; where the required value falls outside the declared domain the
  relation is reported unsatisfied rather than silently accepted.
* **Realizations (0.2.0).** The generated C was compiled with a C compiler and
  executed; the generated bench driver was executed under Python. Both replay
  the scenario and print the solved values.

---

## 7. The feature-proof harness

The verification above is a **grammar and parse** pass: it proves that valid
PSS is accepted, that the six defects it found were fixed, and that the workshop
models compile and solve. It does **not** prove that a construct which parses
behaves as the standard requires — and the behavioural audit of v0.46.0 showed
that many do not.

That audit is now a first-class verification method in the repository:

```sh
tests/feature_proof/run_all.sh /path/to/satya
```

`run_all.sh` runs six self-contained slice scripts (`proof-slice-1.sh` …
`proof-slice-6.sh`), one per slice of the 146-row construct table in
[`FEATURES.md`](FEATURES.md), over the 120 minimal models in
`tests/feature_proof/slice-*/` (20 per slice). Each script takes the binary as
`$1`, fails closed with exit 2 if it is missing or is not Satya, and **asserts
on observed behaviour** rather than on parsing: it reads solved values back out
of the `json` backend — never the trace text, never "it parsed" — and compares
them against what PSS requires. A field constrained `in [10..20]` is checked in
range over 20 seeds, a `bit[8]` is checked never to exceed 255, a `bool` is
checked to be 0 or 1, a `float32` is checked to hold a binary32 bit pattern
rather than a binary64 one, and an aggregate is checked for the elements and
cardinality it must have. One `PASS`/`FAIL` line is printed per item, with the
observed value on failure. Slice 6 also carries Part B, sixteen CLI and backend
checks (exit statuses, line-numbered diagnostics, and every one of the six
backends).

Result on the checksum-verified v0.46.0 binary:

| slice | constructs | pass | fail |
|---|---|---|---|
| 1 | 1–25 literals, data types, components | 12 | 13 |
| 2 | 26–50 templates, activity, flow objects | 16 | 9 |
| 3 | 51–75 constraints, coverage, monitors | 18 | 7 |
| 4 | 76–100 monitors, extension, functions, procedural | 8 | 17 |
| 5 | 101–125 compile-time, executors, address space | 18 | 7 |
| 6 | 126–146 handles, registers, memory primitives | 16 | 5 |
| CLI / backends | 16 items | 16 | 0 |
| **total** | | **104** | **58** |

That is 162 assertions, 104 pass and 58 fail. Judged four ways over the 146
constructs, the audit found **74 PASS, 24 PARTIAL-OK, 35 PARTIAL-WORSE, 13
FAIL** — 35 rows behave worse than this document describes, and 13 do not work
at all. The per-row verdicts and observed values are in
[`FEATURE-AUDIT.md`](FEATURE-AUDIT.md).

The scripts use a two-way vocabulary, so a `PARTIAL-OK` row counts as PASS in
some slices and FAIL in others depending on whether the assertion pins the
documented limitation or the standard. That inconsistency is real; the
four-way table in the audit is the more informative view, and the scripts are
the reproducible evidence rather than a single score.

This is the correction to the earlier framing. The verification described in
§§1–6 found the defects a parse pass can find; it did not catch the behavioural
ones — a constraint that reports `1/1 satisfied` while yielding 256 for
`bit[8] in [256..300]`, `bind` and `import` that parse and do nothing, a
`bins all[4]` that declares one bin — because its assertions tested that
constraints were *satisfied*, not that values were *correct*. The audit also
corrected this document's own record in six places (rows 62, 121, 14, 34, 80,
104) and found new defects the audit itself had missed; both lists are in
[`FEATURE-AUDIT.md`](FEATURE-AUDIT.md).

---

## 8. What was not verified, and residual risk

* **Semantic equivalence with a commercial tool.** Satya's schedule is checked
  for internal consistency and against the LRM grammar; it has not been compared
  against the output of InFact, Perspec or another PSS tool. Whether a given
  scenario is the *same* scenario a commercial tool would produce is not
  established.
* **`exec` bodies.** Because Satya does not interpret SystemVerilog, any model
  whose meaning lives in `exec` (address translation, handle creation, executor
  setup — as in the workshop's methodology package) is only partly understood.
* **Unresolved external packages.** The workshop models import packages not
  present in the archive (`addr_reg_pkg`, `executor_pkg`, `std_pkg`). Satya
  treats their types as opaque, so constraints over their fields are not solved.
* **Resource arbitration is resolved by serialising.** Contention is detected and
  the conflicting steps are moved out of the parallel group, but there is no
  queuing, priority or blocking model.
* **Arithmetic is heuristic.** Relations are solved by single-unknown inversion;
  there is no interval-consistency or linear-programming solver, so relations
  coupling several unknowns are still reported.
* **Name resolution is not scope-checked.** `pkg::name` and `Component::action`
  resolve, and colliding leaf types stay distinct, but `import` statements are
  not used to disambiguate an unqualified leaf reference — such a name is
  flagged and the first declaration wins.

---

## 9. Reproducing this verification

```bash
cargo build --release
cargo test                                  # 134 tests in tests/integration.rs

# The behavioural feature proof — six slices, 162 assertions
tests/feature_proof/run_all.sh ./target/release/satya

WS=<path-to>/pss_workshop_examples_dvcon_2025
for f in $(find $WS -name '*.pss'); do satya parse "$f"; done      # 11 / 11
for p in $WS/examples/*.psf;        do satya parse "$p"; done      # 6 / 6
satya solve $WS/examples/2.psf                                     # 8 steps
satya gen   $WS/examples/1.psf --backend uvm                       # UVM sequences
```

New regression tests added for this verification:

* `parses_lrm_activity_grammar` — the B.9 activity grammar in one model
* `ranged_bit_width_is_parsed` — `bit[3:0]` yields 4 bits
* `repeat_index_binds_into_inline_constraints` — index binding yields 0,10,20,30
* `nested_template_defaults_parse` — the `…<TRAIT>>` closing case

Added for 0.2.0: `qualified_names_resolve_to_distinct_types`,
`extend_struct_in_another_package_merges`, `contention_is_enforced_by_serialising`,
`pool_capacity_is_enforced`, `match_open_range_list_matches_values_and_ranges`,
plus the pool/address/arbitration/arithmetic/backend/`exec` tests (26 total).

---

## 10. Changes to the deliverable

The Cargo project has been updated in place (same filename, new version):

* four parser fixes and two engine/CLI fixes as above;
* multi-file and `.psf` project compilation (`compile_files`, `expand_projects`);
* `README.md` and `docs/DESIGN.md` corrected to match the implementation, with
  the accepted-but-not-executed set stated explicitly;
* four new integration tests (14 total).

The public API is unchanged apart from additions; `satya::solve_with` and
`satya::compile_files` are new.
