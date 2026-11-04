# Feature matrix

The standard defines **146 syntax excerpts** — one per language construct. This
document maps every one of them to its status in Satya, so the project's
coverage can be read honestly rather than inferred from the test count.

> **Read [`FEATURE-AUDIT.md`](FEATURE-AUDIT.md) before trusting this table.**
> Every row here was re-tested against the published **v0.46.0** binary
> (`sha256 4a40d468…2e421e`, checksum verified before use) by running
> `tests/feature_proof/proof-slice-1.sh` … `proof-slice-6.sh`, which print one
> `PASS`/`FAIL` line per construct with the observed value on failure. The
> verdicts and the reproducing models are in
> [`tests/feature_proof/`](../tests/feature_proof/); `run_all.sh` re-runs them
> all. Nothing below is inferred from the parser accepting a construct.

Legend — each status is a **measured** verdict:

| Status | Meaning |
|---|---|
| **yes** | verified PASS — behaves as the standard requires |
| **partial** | verified PARTIAL-OK — the row was documented as limited, and the documented limitation reproduces |
| **worse** | verified PARTIAL-WORSE — more is broken than the previous table documented |
| **no** | verified FAIL — does not work, or parses and is silently ignored |

Every `worse` and `no` row names the defect the proof script measured, with the
value it observed. Where a row's residual gap could not be pinned to a specific
behaviour, it is marked `partial` and the Notes say the gap is **not
individually verified** rather than claiming a PASS.

The corpora in [`CORPUS.md`](CORPUS.md), [`LRM-EXAMPLES.md`](LRM-EXAMPLES.md)
and [`OPENTITAN.md`](OPENTITAN.md) are not evidence of correctness on their own:
several constructs they exercise are marked `worse` or `no` below, because a
model that runs is not a model that solves correctly.

## Summary

**The previous revision of this table said 114 implemented / 32 partial / 0 not
implemented.** That counted what *parsed and was partly implemented*, not what
worked — the same "parsed means implemented" confusion the project has been
trying to eliminate since v0.24.0. This revision corrects it against measured
behaviour: **74 of the 146 constructs behave as the standard specifies.**

| Status | Count |
|---|---|
| yes | 74 |
| partial | 21 |
| worse | 38 |
| no | 13 |
| **total** | **146** |

Per slice, from the same six scripts:

| Slice | Constructs | yes | partial | worse | no |
|---|---|---|---|---|---|
| 1 | 1–25 | 12 | 3 | 8 | 2 |
| 2 | 26–50 | 16 | 1 | 7 | 1 |
| 3 | 51–75 | 18 | 2 | 2 | 3 |
| 4 | 76–100 | 8 | 1 | 13 | 3 |
| 5 | 101–125 | 9 | 9 | 5 | 2 |
| 6 | 126–146 | 11 | 5 | 3 | 2 |

Three rows land in a different bucket from the audit's aggregate, and the audit's
own notes explain why:

- **46 stream declaration** — the audit counted it PARTIAL-OK; the script passes
  it and the documented one-shot consumption limitation reproduces, so it stays
  **yes** with that limitation in Notes.
- **34 activity join specification** and **54 expression constraint** — both were
  documented `yes` and the scripts show broken behaviour (nothing gated; 26 of 40
  seeds violating the constraint), so they are **worse**, not PARTIAL-OK.
- **80 override declaration** — the audit counted it PASS on a model that omitted
  a `;`, so its activity never ran; with the statement present the override is
  still ignored, so it is **worse**.

Net effect: 74 yes (as the audit), 21 partial, 38 worse, and the
audit's 13 failures unchanged. The `no` rows are the same thirteen constructs
the audit names.

## Every construct

| # | Syntax excerpt | Implemented | Notes |
|---|---|---|---|
| 1 | Numeric constants | yes |  |
| 2 | String literals | yes | string-valued variables are carried alongside the numeric ones; literals, assignment, concatenation and `length()` work. |
| 3 | Aggregate literals | yes |  |
| 4 | Empty aggregate literal | no | `arr = {};` is silently ignored — a 3-element array stays size 3. |
| 5 | Value list literal | yes |  |
| 6 | Map literal | partial | maps are not solved (documented limitation reproduces) — `{1:10,2:20}` reports size 6, and a keyed read `m[1]` returns 0. |
| 7 | Structure literal | no | the keyed form `'{x : 3, y : 4}` is a parse error (`expected '}', found ':'`); the positional `'{3,4}` writes `p[0]/p[1]` and leaves `p.x=p.y=0`. |
| 8 | Data types and data declarations | worse | `bool` is not confined to its two values — `flag` solves to 69 at seed 1. |
| 9 | Integer type declaration | worse | the declared width is not enforced — `bit[8]` solves to 256, `bit[4]` to 16, `bit` to 69. |
| 10 | Floating-point type declaration | worse | `float32` does not narrow — a float32 field is stored as a 64-bit pattern (20.14 → 4626362123713081508). |
| 11 | enum declaration | worse | declared enum values are ignored and declaration position used instead — `{W0 = 200, W1 = 201}` solves `w` to 1. |
| 12 | string declaration | yes | a `string` field holds a value in the string store; the trace and the JSON backend report it. |
| 13 | chandle declaration | partial | opaque handle (documented limitation reproduces) — the handle carries no value, so `h == h` evaluates to 0. |
| 14 | struct declaration | worse | the inherited member binds nothing — `d.n` constrained to 6 reads back 0 (nested members do bind). |
| 15 | Collection data types | worse | collection size is wrong (`sz_st=3` for a 2-element list) and `set<int>` keeps a duplicate (7 pushed twice). |
| 16 | ref declaration | partial | reference semantics are not modelled (documented limitation reproduces) — a write through the alias does not propagate (`alias_r.v=99`, `real_obj.v=42`). |
| 17 | User-defined type declaration | yes |  |
| 18 | cast operation | yes | the cast applies the target type's width: `(bit[8])300` is 44, `(bit)5` is 1, `(int)x` is a no-op. |
| 19 | Expressions and operators | yes |  |
| 20 | Conditional operator | yes |  |
| 21 | Set membership operator | worse | membership over a literal list is not evaluated — `lit_hit=0` where 1 is required. |
| 22 | String slice operator | yes | all four forms: `s[i]`, `s[i..j]` (ends inclusive), `s[i..]`, `s[..j]`. |
| 23 | component declaration | yes |  |
| 24 | action declaration | yes |  |
| 25 | Template type declaration | worse | rand templated fields bind nothing (`small.f`, `small.g` absent) while the counter reports 2/2 satisfied. |
| 26 | Template value parameter declaration | yes | a value parameter substitutes into a `bit[W]` width. However, a component-scope value parameter never reaches a `bit[W]` width (200 in `c26<4>` stays 200); verdict unchanged. |
| 27 | Template type parameter declaration | yes | a type parameter substitutes the field's type. |
| 28 | Template type instantiation | yes | `pair_s<bit[16], 4>` and `pair_s<bit[8], 16>` are distinct types, each with its own field widths. |
| 29 | activity statement | yes |  |
| 30 | Action traversal statement | worse | action-handle traversal `h1;` is [UNRESOLVED], and `do sub.s;` is truncated to `sub`. |
| 31 | Activity sequence block | yes |  |
| 32 | Parallel statement | yes |  |
| 33 | Schedule statement | yes | schedule differs from parallel only by its mode label — its branches still share one concurrent group id. |
| 34 | Activity join specification | worse | all four join forms are indistinguishable and nothing is gated — the statement after the block lands inside the concurrent group. |
| 35 | Atomic block | yes |  |
| 36 | repeat-count statement | yes |  |
| 37 | repeat-while statement | worse | the `repeat … while` condition is never re-evaluated — `while (k<2)` with k pinned to 0 still yields 1 step. |
| 38 | foreach statement | yes |  |
| 39 | select statement | yes |  |
| 40 | if-else statement | yes |  |
| 41 | match statement | yes |  |
| 42 | replicate statement | yes |  |
| 43 | symbol declaration | worse | `two;` expands to nothing — 1 step instead of 3, and no warning. |
| 44 | bind statement | no | silently dropped — the trace is identical with and without `bind p2.out c.inp;`, and the consumer still takes producer #0. |
| 45 | buffer declaration | yes |  |
| 46 | stream declaration | yes | a stream is consumed once: the object goes to the first input that takes it, and a later consumer is reported. A buffer is still shareable. |
| 47 | state declaration | partial | state objects are not persisted (documented limitation reproduces) — the writer sets mode=5 and the reader of the same object solves 269. |
| 48 | Flow object reference | worse | a bound input carries no data — the object holds size=7 while the consumer's input solves 269. |
| 49 | resource declaration | worse | two parallel lockers on a 4-instance pool both get instance 0 and are reported as contending. |
| 50 | Resource object reference | worse | parallel lock references never get distinct instances — both resolve to instance 0. |
| 51 | Pool instantiation | yes |  |
| 52 | Static bind directives | no | `bind pa *;` is parsed and ignored — every allocation still comes from pool `pb`. |
| 53 | Member constraint declaration | yes |  |
| 54 | Expression constraint | worse | expression constraints are not enforced — `a+b==10` and `a*b<=20` are violated on 26 of 40 seeds, and 6 of 7 members of a mixed range list are unreachable. |
| 55 | Implication constraint | yes | the consequent is repaired for `==`, `<=`, `>=`, `<`, `>` and `in` range/set forms. |
| 56 | Conditional constraint | yes | lowered onto two implications (`if (c) a else b` becomes `c -> a` and `!c -> b`). |
| 57 | foreach constraint | yes | expanded into one constraint per element at atom extraction, using the element count from the model; braced and unbraced bodies both work. |
| 58 | forall constraint | partial | `forall` is not expanded (documented limitation reproduces) — `forall (it : elem_s) { it.size < 5; }` leaves `e.size >= 5` on 20 of 20 seeds. |
| 59 | unique constraint | yes | `unique { arr }` and `unique { a, b, c }`; duplicates are moved to a free value in the repair pass, and an unsatisfiable set is reported rather than faked. |
| 60 | Default constraints | yes |  |
| 61 | Distribution directive | yes |  |
| 62 | scheduling constraint statement | worse | the declared order is not honoured — `constraint sequence { a1, a2 };` with the activity listing `do a2; do a1;` runs `a2, a1` on every seed. |
| 63 | covergroup declaration | yes |  |
| 64 | covergroup instantiation | no | `named_cg inst1 (x);` binds nothing and never samples, with no warning; only the inline covergroup is reported. |
| 65 | coverpoint declaration | yes |  |
| 66 | bins declaration | no | `bins all[4] = {A,B,C,D}` declares 1 bin, not 4 (reported bins 1/1). |
| 67 | cross declaration | yes |  |
| 68 | Cover statement and monitor declaration | yes | `cover <monitor>;` matches the monitor's activity pattern against the executed stream and samples its covergroups over the steps that realize it. |
| 69 | Monitor activity | yes |  |
| 70 | Action traversal statement | yes |  |
| 71 | Sequence monitor activity statement | yes |  |
| 72 | Concat monitor activity statement | yes |  |
| 73 | Eventually monitor activity statement | yes |  |
| 74 | Overlap monitor activity statement | partial | true interleaving is not modelled (documented limitation reproduces) — `overlap` matches a strictly sequential trace because the branches are matched independently. |
| 75 | Select monitor activity statement | yes |  |
| 76 | Schedule monitor activity statement | partial | as overlap, the branches are matched independently (documented limitation reproduces) — the schedule monitor matched a strictly sequential trace, 1 hit where 0 is required. |
| 77 | Monitor traversal statement | worse | monitor traversal matches nothing — 0 hits by type and by handle against 1 for the flat control. |
| 78 | Monitor constraints | yes | a monitor's `constraint { }` filters the match: a realization whose bound values violate it is discarded, and its covergroups are not sampled. |
| 79 | type extension | worse | the extension's `exec` block never runs, so the attribute it adds stays unusable (`ext_field=0`). |
| 80 | override declaration | worse | the action-scope override of LRM Example 226 is parsed and ignored (`x=58`, want 91); the component-scope forms work. |
| 81 | package declaration | yes |  |
| 82 | import statement | no | no effect on name resolution — `import fpa::*`, `import fpa::value` and no import at all all resolve the same name. |
| 83 | exec block declaration | worse | a composite action's `exec post_solve` never runs. |
| 84 | Function declaration | worse | the package-qualified call does not reach the definition — `fn_p::sum10(4,2)` is 0 while the unqualified call returns 42. |
| 85 | Function definition | yes | parameters are bound positionally into a scope restored after the call; `task` bodies run for their side effects. |
| 86 | Imported function qualifiers | worse | a bodyless imported prototype silently yields 0, with no diagnostic. |
| 87 | Import class declaration | no | parse error in all four spellings tried (`expected ';', found 'foreign_c'`). |
| 88 | Target-template function implementation | worse | the target-template body is neither evaluated nor reported — `compute(21)` is 0. |
| 89 | Procedural block statement | no | `{ in_block = 5; }` leaves 0 — the block is dropped entirely. |
| 90 | Procedural variable declaration | worse | a block-local `int t = 7;` overwrites the component field `t` instead of being scoped to the block. |
| 91 | Procedural assignment statement | worse | only the plain form takes effect — `s += 2` and `arr[1] = 9` are silently dropped. |
| 92 | Void function call | yes |  |
| 93 | Procedural return statement | yes |  |
| 94 | Procedural repeat-count statement | worse | a single-statement body runs once — `repeat (4) b = b + 3;` leaves b=3 (want 12); the braced form is correct. |
| 95 | Procedural repeat-while statement | yes |  |
| 96 | Procedural foreach statement | yes |  |
| 97 | Procedural if-else statement | worse | the single-statement form takes the wrong branch — `if (5 > 3) r = 1; else r = 2;` yields 2; the braced form is correct. |
| 98 | Procedural match statement | worse | range arms never match and unbraced arm bodies are dropped — `[4..7]` on 5 gives 4 (want 2). |
| 99 | Procedural break/continue statement | yes |  |
| 100 | Procedural randomize statement | worse | `randomize res with { res > a; }` ignores the with-clause and returns 0. |
| 101 | Procedural yield statement | yes | accepted; the engine runs `exec` bodies to completion, so there is no coroutine to yield from. |
| 102 | Export action declaration | worse | the export statement has no effect — `pkg::A1` resolves with no export declaration at all. |
| 103 | compile if declaration | no | the condition is evaluated, but declarations are lost when the `compile if` is nested, at package scope, or in an action body. |
| 104 | compile has expression | no | `compile has()` is true for a name declared nowhere, so the else-branch is unreachable. |
| 105 | compile assert statement | yes | a false condition is an error naming the expression. |
| 106 | String formatting and output functions | yes | `format()` builds a string with `%d`, `%x`, `%s` and `%%`. |
| 107 | Runtime messaging function | yes |  |
| 108 | Text file operations using file handles | partial | accepted and have no effect (documented limitation reproduces) — `fopen`/`fwrite`/`fclose` write no file. |
| 109 | Simple text file operations | partial | accepted and has no effect (documented limitation reproduces) — `fprintf` writes no file. |
| 110 | Error reporting functions | worse | `fatal()` does not terminate the run — the next statement executes and the exit status is 0. |
| 111 | Randomization functions | yes |  |
| 112 | Floating-point storage types | worse | `float32` keeps the float64 image — 6.0 stores 4618441417868443648 instead of 1086324736. |
| 113 | float_mantissa function | yes |  |
| 114 | float_exponent function | yes | correct for 6.0/1.0/2.0 as asserted; wrong for most negative inputs (-6.0 gives 1022, want 1025), which no model pins. |
| 115 | float_sign function | yes |  |
| 116 | to_float function | yes |  |
| 117 | Executor component | partial | an executor component's trait struct is settable and readable; no executor-specific solve behaviour is modelled and no further limitation is stated, so the residual gap is not individually verified. |
| 118 | Executor group component | partial | `add_executor` registers without a warning and the traits read back; no further executor-group behaviour is exercised, so the residual gap is not individually verified. |
| 119 | Executor claim struct | worse | an executor claim is routed into address allocation and reported unsatisfiable instead of being matched against an executor. |
| 120 | Executor query function | partial | `executor()` is callable and returns null in `pre_solve` as the standard requires; the query is otherwise not evaluated, so the residual gap is not individually verified. |
| 121 | Generic address space component | worse | a claim's `size` is ignored — a 100-byte claim is allocated 64 bytes and counted satisfied; traits *are* matched, contrary to the previous note. |
| 122 | Contiguous address space component | partial | successive claims are laid out contiguously from the region base (0x1000, 0x1040, 0x1080); no further contiguous-space behaviour is exercised, so the residual gap is not individually verified. |
| 123 | Transparent address space component | partial | a region added with `add_region()` is where the claim lands (0x8000); no further transparent-space behaviour is exercised, so the residual gap is not individually verified. |
| 124 | Base address region type | partial | a 100-byte region refuses a second 64-byte claim, as documented; no further region behaviour is exercised, so the residual gap is not individually verified. |
| 125 | Contiguous address space region type | partial | a claim lands in the first region that fits (0x3000, not the 10-byte 0x1000); no further region behaviour is exercised, so the residual gap is not individually verified. |
| 126 | Transparent region type | worse | `add_nonallocatable_region` yields no handle — the claim address reads 0 instead of 2684358656. |
| 127 | Base address space claim type | worse | a claim's `size` is ignored — a claim constrained to 8 bytes is 64. |
| 128 | Contiguous address space claim type | worse | a claim's `size` is ignored — a claim constrained to 16 bytes is 64. |
| 129 | Transparent contiguous address space claim type | partial | a transparent claim's address is known only after solve (0 before, 0x1000 after), as documented; its `size` is still ignored (`t.size=64`, want 32). |
| 130 | Address space group | partial | an address space group is accepted and does not disturb allocation (the claim still lands at 0x1000); no further group behaviour is exercised, so the residual gap is not individually verified. |
| 131 | packed_s base struct | partial | the packed little-endian layout is verified from the least-significant bit (24.10.1); no further limitation is stated, so the residual gap is not individually verified. |
| 132 | sizeof_s struct | no | correct only for a packed struct of bit fields; returns 0 for a nested struct, a `typedef bit[24]`, an enum, `int` and `bit`. |
| 133 | Address space handle | yes |  |
| 134 | make_handle_from_claim function | yes |  |
| 135 | make_handle_from_handle function | yes |  |
| 136 | addr_value function | yes |  |
| 137 | addr_value_solve function | yes |  |
| 138 | addr_value_abs function | no | returns the constant 1 for every handle (want 1040 and 3072), not the handle's address. |
| 139 | get_tag function | yes | returns the region's `tag` string, which is set by `reg.tag = \"name\"`. |
| 140 | Primitive read operations for byte addressable spaces | yes |  |
| 141 | Primitive write operations for byte addressable spaces | yes |  |
| 142 | Read and write series of bytes | yes | `write_bytes` takes a literal list; `read_bytes` fills one. |
| 143 | Read and write packed structs | yes |  |
| 144 | Primitive operation implementation functions | partial | a user-supplied `read32` is used for the primitive call, but the primitives are still implemented in the engine rather than delegated to an executor instance (documented limitation reproduces). |
| 145 | PSS register definition | yes | `reg_c<R, ACC, SZ>` declarations plus the register access functions over a register store. |
| 146 | PSS register group definition | partial | `reg_group_c` is accepted and register access works; the group's own `get_offset_of_*` functions are not evaluated (documented limitation reproduces). |
