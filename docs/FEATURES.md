# Feature matrix

The standard defines **146 syntax excerpts** — one per language construct. This
document maps every one of them to its status in Satya, so the project's
coverage can be read honestly rather than inferred from the test count.

> **This matrix is not a conformance record, and parts of it are stale.**
> Read [`FEATURE-AUDIT.md`](FEATURE-AUDIT.md) before trusting any row. The
> verdicts below were measured on the published **v0.46.0** binary
> (`sha256 4a40d468…2e421e`) by `tests/feature_proof/proof-slice-1.sh` …
> `proof-slice-6.sh`, which print one `PASS`/`FAIL` line per construct with the
> observed value on failure.
>
> The code has been fixed many times since and this table did not follow it.
> Running the harness against the **v0.75.0 source build** gives **128 pass /
> 34 fail of 162 checks**; re-testing the rows the harness now passes against
> that binary corrected **21 rows here** (8, 9, 10, 11, 13, 21, 34, 49, 50, 84,
> 87, 90, 91, 94, 97, 98, 110, 112, 114, 132, 138 — each says "re-verified on
> 0.75.0" in Notes; 18 of them change bucket). Where a row here disagrees with
> the harness, **the harness is right**. Every other row still carries its
> v0.46.0 verdict and has **not** been re-verified against the current build;
> treat it as unverified. Nothing below is inferred from the parser accepting a
> construct.

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

**The v0.46.0 audit said 114 implemented / 32 partial / 0 not implemented; its
behavioural re-test replaced that with 74 / 21 / 38 / 13.** Both are historical.
What is measured today is the harness, not this table:

```
tests/feature_proof/run_all.sh ./target/release/satya     # v0.75.0 source build
slice 1: 19 pass  6 fail      slice 4: 15 pass 10 fail
slice 2: 19 pass  6 fail      slice 5: 20 pass  5 fail
slice 3: 21 pass  4 fail      slice 6: 34 pass  3 fail
TOTAL: 128 pass, 34 fail, 162 checks
```

The 34 assertions that fail name these constructs: 6, 9, 14, 15, 16, 25, 30, 34,
37, 43, 47, 48, 54, 58, 62, 74, 76, 77, 79, 80, 83, 84, 86, 87, 88, 100, 102,
103, 104, 119, 121, 126, 127, 128. Every other construct's assertion passes on
this build.

Applying the 21 row corrections made in this revision to the audit's tally gives
the table below. **It is arithmetic on the v0.46.0 verdicts, not a fresh
measurement** — the rows not listed above as re-verified were not re-tested:

| Status | Count |
|---|---|
| yes | 88 |
| partial | 23 |
| worse | 25 |
| no | 10 |
| **total** | **146** |

Fourteen rows moved **into** `yes` (8, 10, 11, 13, 21, 49, 50, 90, 91, 94, 97,
98, 112, 138) — each had been fixed without this table following. Four moved
between the other buckets, and each Notes cell says why:

- **34 activity join specification** — `worse` → `partial`. The four forms are
  distinguishable in emission order; what is still wrong is the trace metadata.
- **87 import class declaration** — `no` → `worse`. All four spellings parse; the
  imported method binds nothing.
- **110 error reporting functions** — `worse` → `partial`. `fatal()` now stops
  its `exec` block; the run still exits 0.
- **132 sizeof_s struct** — `no` → `partial`. It now sizes a nested struct, a
  typedef'd bit vector, an enum, `int` and `bit`; the inline `bit[24]` spelling
  does not parse.

## Every construct

| # | Syntax excerpt | Implemented | Notes |
|---|---|---|---|
| 1 | Numeric constants | yes |  |
| 2 | String literals | yes | string-valued variables are carried alongside the numeric ones; literals, assignment, concatenation and `length()` work. |
| 3 | Aggregate literals | yes |  |
| 4 | Empty aggregate literal | yes | `arr = {};` now yields an empty collection (was stored as 1, array stayed size 3) — verified fixed in 0.51.0 |
| 5 | Value list literal | yes |  |
| 6 | Map literal | partial | maps are not solved (documented limitation reproduces) — `{1:10,2:20}` is stored as a flat six-element list (`m.size` is 6, and the members read back at indices 0, 2, 3 and 5), so a keyed read `m[1]` returns 0. Re-verified on 0.75.0. |
| 7 | Structure literal | yes | verified fixed in 0.51.0
| 8 | Data types and data declarations | yes | `bool` is confined to its two values — a random `bool` reads 0 or 1, and `constraint bo == 2` leaves the field at 0 and is counted unsatisfied (0/3), rather than being accepted. Re-verified on 0.75.0; the previous note's `flag = 69` no longer reproduces. |
| 9 | Integer type declaration | worse | the declared width **is** now enforced on solved values — `bit[8]` stays ≤ 255, `bit[4]` ≤ 15, `bit` ≤ 1 (the previous note's 256 / 16 / 69 no longer reproduce). What still fails is (a) **rendering**: values are `i64` and the JSON and trace backends format them signed, so a `bit[64]` holding 2^64−1 prints as `-1` (`bad64=-1`), which the harness rejects; and (b) a constraint no value of the width can satisfy (`bit[8] in [256..300]`) leaves the field at an in-range value and is only counted unsatisfied (1/2), not reported. Re-verified on 0.75.0. |
| 10 | Floating-point type declaration | yes | `float32` stores its IEEE-754 **binary32** word — `f = 6.0` gives 1086324736 (0x40C00000) and `f = 20.14` gives 1101078200 (0x41A11EB8) — while `float64` keeps the binary64 image (6.0 → 4618441417868443648). Fixed in 0.68.0; re-verified on 0.75.0. |
| 11 | enum declaration | yes | declared enum values are honoured — `enum e { W0 = 200, W1 = 201 }` solves `w` to 201, not to its declaration position. Fixed in 0.66.0 (packaged enums); re-verified on 0.75.0. |
| 12 | string declaration | yes | a `string` field holds a value in the string store; the trace and the JSON backend report it. |
| 13 | chandle declaration | yes | a chandle carries an opaque id, distinct per attribute and seed-stable: `h == h` is 1 and two different chandles compare 0. No other chandle operation is modelled. Fixed in 0.68.0; re-verified on 0.75.0 — the previous note's `h == h` → 0 no longer reproduces. |
| 14 | struct declaration | worse | the inherited member binds nothing — `d.n` constrained to 6 reads back 0 (nested members do bind). |
| 15 | Collection data types | worse | collection size is wrong (`sz_st=3` for a 2-element list) and `set<int>` keeps a duplicate (7 pushed twice). |
| 16 | ref declaration | partial | reference semantics are not modelled (documented limitation reproduces) — a write through the alias does not propagate (`alias_r.v=99`, `real_obj.v=42`). |
| 17 | User-defined type declaration | yes |  |
| 18 | cast operation | yes | the cast applies the target type's width: `(bit[8])300` is 44, `(bit)5` is 1, `(int)x` is a no-op. |
| 19 | Expressions and operators | yes |  |
| 20 | Conditional operator | yes |  |
| 21 | Set membership operator | yes | membership over a literal or range list is evaluated — `2 in [1,2,3]` is 1, `9 in [1,2,3]` is 0, `5 in [4..7]` is 1, `3 in [4..7]` is 0. Fixed in 0.66.0; re-verified on 0.75.0. Membership in a **named** collection is still not solved — see `SEMANTICS.md` §1, `x in <collection>`. |
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
| 34 | Activity join specification | partial | the forms **are** distinguishable in emission order, which the previous note denied: `join_branch(L2)` (with `L2: do A`) → `A, C, B` — the named branch, then the joined statement, then the rest — and `join_select(1)` → `B, C, A`, `join_none` and `join_first(1)` → `C` first (byte-identical to each other), default `join_all` → `A, B` then `C`. What is still wrong is the trace metadata: for all four non-default forms the joined statement is tagged with the concurrent group's id, so the trace does not show it as outside the group (the default form does, and the harness fails on exactly that tag). There is no run time, so "first *n* to complete" is not simulated. Re-verified on 0.75.0. |
| 35 | Atomic block | yes |  |
| 36 | repeat-count statement | yes |  |
| 37 | repeat-while statement | worse | the `repeat … while` condition is never re-evaluated — `while (k<2)` with k pinned to 0 still yields 1 step. |
| 38 | foreach statement | yes |  |
| 39 | select statement | yes |  |
| 40 | if-else statement | yes |  |
| 41 | match statement | yes |  |
| 42 | replicate statement | yes |  |
| 43 | symbol declaration | worse | `two;` expands to nothing — 1 step instead of 3, and no warning. |
| 44 | bind statement | yes | verified fixed in 0.51.0
| 45 | buffer declaration | yes |  |
| 46 | stream declaration | yes | a stream is consumed once: the object goes to the first input that takes it, and a later consumer is reported. A buffer is still shareable. |
| 47 | state declaration | partial | state objects are not persisted (documented limitation reproduces) — the writer sets mode=5 and the reader of the same object solves 269. |
| 48 | Flow object reference | worse | a bound input carries no data — the object holds size=7 while the consumer's input solves 269. |
| 49 | resource declaration | yes | two parallel lockers on a 4-instance pool get **distinct** instances (0 and 1) and are not reported as contending. Fixed in 0.69.0; re-verified on 0.75.0. |
| 50 | Resource object reference | yes | parallel lock references get distinct instances (0 and 1), and each carries its own object. Fixed in 0.69.0; re-verified on 0.75.0. |
| 51 | Pool instantiation | yes |  |
| 52 | Static bind directives | yes | verified fixed in 0.51.0
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
| 64 | covergroup instantiation | yes | verified fixed in 0.51.0
| 65 | coverpoint declaration | yes |  |
| 66 | bins declaration | yes | verified fixed in 0.51.0
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
| 82 | import statement | yes | verified fixed in 0.51.0
| 83 | exec block declaration | worse | a composite action's `exec post_solve` never runs. |
| 84 | Function declaration | worse | a package-qualified call **does** reach the definition when the function is only defined: `fn_p::sum10(4,2)` is 42, as is the unqualified call. The failure is with a **prototype**: when `function int sum10(int a, int b);` precedes the definition, the bodyless prototype wins and *both* call forms silently yield 0, with no diagnostic. Re-verified on 0.75.0 — the previous note's "unqualified returns 42" held only for the definition-only model. |
| 85 | Function definition | yes | parameters are bound positionally into a scope restored after the call; `task` bodies run for their side effects. |
| 86 | Imported function qualifiers | worse | a bodyless imported prototype silently yields 0, with no diagnostic. |
| 87 | Import class declaration | worse | all four spellings now parse (the previous note's `expected ';', found 'foreign_c'` no longer reproduces), but the imported class method binds nothing — `h.get(7)` is 0, with no diagnostic. Re-verified on 0.75.0. |
| 88 | Target-template function implementation | worse | the target-template body is neither evaluated nor reported — `compute(21)` is 0. |
| 89 | Procedural block statement | yes | verified fixed in 0.51.0
| 90 | Procedural variable declaration | yes | a block-local `int t = 7;` is scoped to its block: the local reads 9 where it shadows a field, and the field keeps its own value when the block ends. Fixed in 0.67.0; re-verified on 0.75.0. |
| 91 | Procedural assignment statement | yes | `+=` and indexed assignment both take effect — `s = 1 + 1; s += 2;` leaves `s = 4`, and `arr[1] = 9;` leaves `arr[1] = 9`. Re-verified on 0.75.0. |
| 92 | Void function call | yes |  |
| 93 | Procedural return statement | yes |  |
| 94 | Procedural repeat-count statement | yes | a single-statement body iterates — `repeat (4) b = b + 3;` leaves `b = 12`. Fixed in 0.59.0; re-verified on 0.75.0. |
| 95 | Procedural repeat-while statement | yes |  |
| 96 | Procedural foreach statement | yes |  |
| 97 | Procedural if-else statement | yes | the single-statement form takes the right branch — `if (5 > 3) r = 1; else r = 2;` yields `r = 1`. Fixed in 0.59.0; re-verified on 0.75.0. |
| 98 | Procedural match statement | yes | range arms match and unbraced arm bodies run — `match (5)` with `[4..7]: { res = 2; }` gives 2, and the unbraced LRM Example 275 form does too. Fixed in 0.69.0; re-verified on 0.75.0. Known approximation: when several arms match, the first is taken where 11.4.6.d chooses one at random. |
| 99 | Procedural break/continue statement | yes |  |
| 100 | Procedural randomize statement | worse | `randomize res with { res > a; }` ignores the with-clause and returns 0. |
| 101 | Procedural yield statement | yes | accepted, and there is no coroutine to yield from — but **not** for the reason this note used to give. It said "the engine runs `exec` bodies to completion"; an `exec body` is never executed at all (verified on 0.75.0: `exec body { x = 99; }` leaves the field at its solved value, and a `message()` inside it is never printed). The `exec` blocks that do run are `pre_solve`, `post_solve`, `pre_body`, `init_up` and `init_down`. Corrected here — see `SEMANTICS.md` §2.1. |
| 102 | Export action declaration | worse | the export statement has no effect — `pkg::A1` resolves with no export declaration at all. |
| 103 | compile if declaration | no | the condition is evaluated, but declarations are lost when the `compile if` is nested, at package scope, or in an action body. |
| 104 | compile has expression | no | `compile has()` is true for a name declared nowhere, so the else-branch is unreachable. |
| 105 | compile assert statement | yes | a false condition is an error naming the expression. |
| 106 | String formatting and output functions | yes | `format()` builds a string with `%d`, `%x`, `%s` and `%%`. |
| 107 | Runtime messaging function | yes |  |
| 108 | Text file operations using file handles | partial | accepted and have no effect (documented limitation reproduces) — `fopen`/`fwrite`/`fclose` write no file. |
| 109 | Simple text file operations | partial | accepted and has no effect (documented limitation reproduces) — `fprintf` writes no file. |
| 110 | Error reporting functions | partial | `error()` reports and execution continues (verified: the next statement runs); `fatal()` records a `FATAL:` message and the rest of its `exec` block does **not** run — the previous note's "the next statement executes" no longer reproduces. What remains: the process still exits 0 and no error reaches the caller, so a harness cannot see the failure from the exit status. Re-verified on 0.75.0. |
| 111 | Randomization functions | yes |  |
| 112 | Floating-point storage types | yes | `float32` stores the binary32 word — 6.0 stores 1086324736 (0x40C00000), 20.14 stores 1101078200 — and `float64` keeps 4618441417868443648. Fixed in 0.68.0; re-verified on 0.75.0. |
| 113 | float_mantissa function | yes |  |
| 114 | float_exponent function | yes | the negative values now agree with their positive counterparts (1.0 and −1.0 → 1023, 2.0 and −2.0 → 1024, 6.0 and −6.0 → 1025, ±0.25 → 1021), so the previous note's "wrong for most negative inputs" no longer reproduces after the 0.68.0 sign fix. The value returned is the raw biased exponent field (1.0 → 1023), not the unbiased exponent; whether that is what the standard's `float_exponent` requires was **not verified**. Re-verified on 0.75.0. |
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
| 132 | sizeof_s struct | partial | computed for a packed struct of bit fields (`bit[1]`+`bit[3]`+`bit[4]` → `nbits` 8, `nbytes` 1), a struct nesting one (16), a `typedef bit[24]` (24), an enum (32), `int` (32) and `bit` (1) — the previous note's zeros no longer reproduce. Still a parse error in the inline spelling `sizeof_s<bit[24]>`. Re-verified on 0.75.0. |
| 133 | Address space handle | yes |  |
| 134 | make_handle_from_claim function | yes |  |
| 135 | make_handle_from_handle function | yes |  |
| 136 | addr_value function | yes |  |
| 137 | addr_value_solve function | yes |  |
| 138 | addr_value_abs function | yes | a **boolean**, per PSS 3.1 §21.13.7: 1 when the handle's absolute address is available (a handle from a region added to an address space — `addr_value` 1060/0x424 with `addr_value_abs` 1; the harness's two handles give 1 and 1), 0 for a handle made from a generic claim whose region is not transparent. Restored in 0.61.0; re-verified on 0.75.0. The 0 case coincides with the `addr_value` gap in `SEMANTICS.md` §2.1, so which of the two produces it could not be separated. |
| 139 | get_tag function | yes | returns the region's `tag` string, which is set by `reg.tag = \"name\"`. |
| 140 | Primitive read operations for byte addressable spaces | yes |  |
| 141 | Primitive write operations for byte addressable spaces | yes |  |
| 142 | Read and write series of bytes | yes | `write_bytes` takes a literal list; `read_bytes` fills one. |
| 143 | Read and write packed structs | yes |  |
| 144 | Primitive operation implementation functions | partial | a user-supplied `read32` is used for the primitive call, but the primitives are still implemented in the engine rather than delegated to an executor instance (documented limitation reproduces). |
| 145 | PSS register definition | yes | `reg_c<R, ACC, SZ>` declarations plus the register access functions over a register store. |
| 146 | PSS register group definition | partial | `reg_group_c` is accepted and register access works; the group's own `get_offset_of_*` functions are not evaluated (documented limitation reproduces). |
