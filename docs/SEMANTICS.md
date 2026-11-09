# Semantic conformance

Parsing a construct is not the same as implementing it. A tool can accept
`join_branch` and still ignore what it means; it can accept `constraint default`
and apply it as though it were hard. This document tracks what Satya does
*according to the LRM's defined behaviour*, and — just as importantly — where it
only approximates it.

For coverage of **every** construct the standard defines — all 146 syntax
excerpts, each marked implemented / partial / not implemented — see
[`FEATURES.md`](FEATURES.md).

The test of semantic conformance used here is simple: **feed the LRM's own
examples, with their stated expected results, and check the tool produces them.**

## How this revision was verified

Every claim below was re-checked against the published **v0.46.0** release binary
(`satya-v0.46.0-x86_64-linux.tar.gz`, `sha256 4a40d468…2e421e`, checksum
verified before use — nothing here is built from source). The evidence is:

* the six assertion scripts in [`tests/feature_proof/`](../tests/feature_proof/)
  — `./proof-slice-N.sh <binary>` — which re-derive one verdict per construct
  from observed behaviour (104 pass / 58 fail on this binary);
* the per-slice reports in `tests/feature_proof/reports/`;
* purpose-written models for the claims those two did not settle.

Markers used in the tables:

| marker | meaning |
|---|---|
| ✔ | the claim was re-tested against the binary in this revision and still holds |
| ✎ | the claim was **wrong or stale** and is corrected here; the previous wording is quoted so the change is visible |
| + | a defect the audit found and this document did not mention before; verified here |
| *(none)* | carried over from the previous revision, not re-tested in this pass — treat as unverified |

---

## 1. Implemented per the standard

### Scheduling (Clause 12)

| Construct | Clause | Behaviour |
|---|---|---|
| `join_all` (default) | 12.3.6.1 | ✔ the following statement is scheduled after every branch, and this is the only join form that leaves it **outside** the concurrent group in the trace (`A`, `B`, then `C` with no group id) |
| `join_none` | 12.3.6.1c | ✎ no scheduling dependency, and the following statement is emitted first. **Previously:** "the following statement is scheduled independently (and is emitted first, to make the absence of a dependency visible)". The emission-first part is true; what the row did not say is that the statement is still tagged with the concurrent group's id, and that its trace is byte-identical to `join_first(1)`'s |
| `join_branch(L1, …)` | 12.3.6.1a | ✎ the following statement's emission order does wait for the named branches: `join_branch(L1)` → `A, C, B`; `join_branch(L2)` → `B, C, A`; `join_branch(L1, L2)` → `A, B, C`, stable over seeds. **Previously** the row claimed this without qualification; the caveat is that the statement is still tagged with the concurrent group id, so the trace metadata does not show it as outside the group. The audit's row-34 verdict ("all four join forms are indistinguishable, nothing is gated") did **not** reproduce — the forms differ in where the following statement lands |
| `join_select(n)` | 12.3.6.1b | ✔ the following statement waits for *n* selected branches; the selection varies by seed (three branches, `n = 1`: all three chosen within 12 seeds); `n = 2` waits for both; `n = 0` behaves as `join_none`. Same group-id caveat as above |
| `join_first(n)` | 12.3.6.1d | ✔ a run-time dependency only, so it imposes no scheduling order (the following statement is emitted first) |
| `replicate (n) L: stmt` | 12.5 | ✔ expands in place into *n* labelled copies, distinct from `repeat` |
| labelled statements | 12.1 | ✔ labels are kept and drive `join_branch` |

Checked against the LRM's Examples 68, 71 and 72, which state the expected
ordering. For Example 71 (`parallel join_select(1) { L2: do A; L3: do B; } L4: do C;`)
the selected branch, then `C`, then the other branch — reproduced exactly on
seeds 1 and 2 (seeds 3 and 4 select `A` instead, as the example requires):

```
0. B   [parallel g1]  L3
1. Cc  [parallel g1]  L4
2. A   [parallel g1]  L2
```

### Constraints (Clause 16)

| Construct | Clause | Behaviour |
|---|---|---|
| default constraints | 16.1.10.1 | ✔ one default per attribute survives, by precedence; conflicts are flagged |
| `default disable <attr>` | 16.1.10.1b | ✔ removes the defaults on an attribute |
| `dist` / `:=` / `:/` | 16.1.9 | ✔ weighted selection; `:=` gives each value in a range the full weight, `:/` spreads it |
| `exec pre_body` | 22.1 | ✎ a solve-platform block that runs **after** `post_solve` — verified for a leaf action: `pre_solve` wrote `x = 42`, the solve kept it, `post_solve` read 42, and `pre_body` read what `post_solve` had written. **Previously** the row said only "run after `post_solve` and before the body is realized"; the qualification that matters is that an `exec` block declared in a **composite** action never runs at all (see §2.1) |
| implications | 8.5 | ✔ propagated, not merely checked (`a == 1 -> b == 5`: every seed with `a == 1` had `b == 5`) |
| ordering between two random variables | — | ✔ both domains narrowed before assignment (`a < b` over `[0..10]²` holds on every seed) |

**Default constraints.** The LRM's Example 133 states its expected results —
`s1.attr1` = 0, `s2.attr1` = 2 (the action-level default overriding the type's),
`s3.attr1` in 1..2 because its default was disabled, `attr1 < attr2` throughout.
✔ Satya produces exactly that on every seed tried. Example 134 reports its
contradiction rather than silently picking a side.

**`dist`.** ✔ Re-measured here over 200 seeds: `dist x in [100..102 := 1, 200 := 2, 300 := 5]`
gives 10.0 / 11.0 / 11.0 / 21.0 / 47.0 % against the specified 10 / 10 / 10 / 20 / 50
(the previous revision's 600-seed figures — 10.2 / 9.7 / 10.2 / 20.7 / 49.3 — are
consistent with these). With `:/`, `[100..102 :/ 6, 200 := 2]` gives
23.5 / 27.5 / 23.5 / 25.5 % against 25 / 25 / 25 / 25.

### Data and execution

| Construct | Clause | Behaviour |
|---|---|---|
| fixed-size arrays `array<T, N>`, `x[N]` | 8.4 | ✔ expand into one variable per element, with per-element domains |
| `arr[i]` indexing | 8.6.2 | ✔ resolves to the element variable |
| `arr.size()`, `arr.sum()` | 8.6, 7.9.2.2 | ✔ evaluated, and both work inside a constraint; `size` is the declared extent, `sum` folds the elements (`arr.sum() == 10` is honoured). These are the only two array folds the standard defines |
| `foreach (arr[i])` | 12.2.6 | ✔ iterates the elements |
| `exec pre_solve` | 16.4.12 | ✔ executed **before** the solver; its assignments to non-random fields feed the solve |
| `exec post_solve` | 16.4.12 | ✎ executed **after** the solver, from the solved values — for a leaf action. **Previously** the row was unconditional; a `post_solve` declared on a composite action never runs (§2.1) |
| non-`rand` attributes | 8.4 | ✔ not randomised — they hold their initial value (`int keepme = 42;` stays 42 while `rand` neighbours move) |
| flow-object ordering | 13 | ✎ an input must be bound to an object produced by an earlier step, and a violation is reported — verified. **Previously** the row stopped there; what it did not say is that the bound input does **not** carry the producer's data: the object holds `size = 7` and the consumer's input solves `size = 269` (construct 48, §2.1) |
| `stream` / `buffer` consumption | 13 | ✎ **moved here from §2, where it was stale.** §2 used to say "consume-once semantics for streams are not enforced". They **are** enforced: a `stream` goes to the first input that takes it and a second consumer is reported (`flow-object ordering: input 'inp' of type 'data_s' has no unconsumed preceding output (PSS 13)`), while the same model written with `buffer` produces no warning |
| covergroups, coverpoints, `cover { … }` | 18, 19.2 | ✔ sampled while the `cover` block's activity executes; a coverage summary is reported. A covergroup declared with parameters and covered inline samples correctly. Note that a covergroup **instantiation** (`named_cg inst1 (x);`) binds nothing (§2.1) |
| `cross` items | 18.4 | ✔ sampled as a value tuple, once every referenced coverpoint is known. Verified for crossed labelled coverpoints, including coverpoints declared on bit-selects (`coverpoint v[3:0]` / `v[7:4]`, crossed): four distinct `v` values produced four distinct tuples and three distinct values on each coverpoint |
| explicit `bins` | 18.3 | ✎ coverage is reported against the declared bins (`bins hit/declared`) — verified for ordinary `bins` and `bins x[] = [ … ]` lists. **Previously** the row implied this held for every bins form; `bins all[N] = {…}` declares **one** bin (§2.1) |
| `cover <type>;` | 19.2 | ✔ matches the named monitor's scenario against the executed stream and samples its covergroups over the matched steps |
| monitor declarations | 19.1 | ✔ parsed, with their action handles and covergroups |
| monitor activity: action traversal | 19.3.1 | ✔ matches the *earliest* step of that action at or after the checkpoint, subject to the inline constraint |
| `sequence` / `concat` | 19.3.2, 19.3.3 | ✎ **the binary and this document disagreed, and the binary is what ships.** The row used to say "`sequence` requires the sub-scenarios to be contiguous; `concat` allows gaps". The binary does the **opposite**: `sequence { do x; do y; }` matches a trace `a, g, b` (a gap is accepted), while `concat { do x; do y; }` matches `a, b` but **not** `a, g, b` (adjacency is required). The second half of the old sentence does hold: the realization is the matched steps only — with `sequence` over `a, g, b` the gap step is not part of it (a covergroup sampled over the match reports the matched step's value only, `bins 1/3`, not the gap's). Which side matches the LRM could not be settled here: the LRM text was not available in the audit environment, and the source comment on the matcher claims the document's reading while the code implements the opposite |
| `eventually` | 19.3.4 | ✔ matches from any checkpoint at or after the start |
| `select` | 19.3.6 | ✔ takes the first arm whose guard holds |
| nested monitors | 19.3.9 | ✎ **moved here from §1, where it claimed "matched by name".** They are not matched at all: with a flat control monitor matching once on the same trace, traversing a nested monitor by type (`do rw;`) or through a handle (`m;`) produces **0 hits** (construct 77) |
| `sizeof_s<T>::nbits` / `::nbytes` | 24.10.2 | ✎ **moved here from §1, where it claimed the size is "computed for packed structs, bit vectors, enums and scalars".** It is computed only for a packed struct of bit fields: a `packed_s` of `bit[4]`+`bit[12]` gives `nbits = 16`, `nbytes = 2`, and a struct holding a packed-struct member plus a `bit[8]` gives 24. It returns **0** for a `typedef bit[24]`, for `int`, and for an enum, and `sizeof_s<bit[24]>` is a parse error (construct 132) |
| `reg_c<R, ACC, SZ>` registers | 24.11.1 | ✔ declared, with `read`, `read_val`, `write`, `write_val`, `write_masked`, `write_val_masked`, `write_field` and `write_fields` over a register store |
| packed little-endian layout | 24.10.1 | ✔ register fields pack from the least-significant bit in declaration order |
| address handles | 24.10.3 | ✔ a handle holds an address; `make_handle_from_claim` and `make_handle_from_handle` offset it |
| `addr_value` / `addr_value_solve` | 24.10.5, 24.10.6 | ✔ the handle's address (`0x410` and `0xC00` for claims at `0x10` and `0x800` in a region based at `0x400`) |
| `addr_value_abs` | 24.10.7 | ✔ **fixed in 0.51.0.** Returns the handle's absolute address, derived from `addr_value` — two handles at 0x410 and 0xC00 give 1040 and 3072. It previously returned the constant 1 for every handle, because it read a `__h.<n>.abs` flag the engine pinned to 1; that flag is removed. **This document is the specification for this builtin** — a unit test asserted the 1 as an "absolute-known" flag and was corrected to match, since the two could not both be right |
| primitive read/write | 24.10.9.1, 24.10.9.2 | ✔ `read8`/`read16`/`read32`/`read64` and their writes, little-endian over a byte-addressable store |
| array operators `==`, `!=` | 7.9.2.1 | ✔ element-wise equality and inequality are solved (`a == b` holds for two equal arrays, `a != b` evaluates to 0) |
| array operator `in` | 7.9.2.1 | ✎ **split out of the row above, which claimed "element-wise equality and set membership are solved".** Membership is not evaluated: `v in [1, 3, 5..9]` as an expression yields 0 even when `v = 6` lies in `5..9`, and `v in allowed` over a named array leaves `v = 69` outside `{4, 7, 9}` (the counter reports 3/4 rather than claiming success). See also `x in <collection>` below |
| array properties `arr.size`, `arr.sum` | 7.9.2.4 | ✔ the deprecated property forms evaluate like the methods |
| `exec init_down` / `init_up` | 22.1 | ✔ run for the component hierarchy before the scenario, `init_down` top-down and `init_up` bottom-up (`cfg` ends at the `init_up` value) |
| collection mutators in `exec` | 7.9.3.2 | ✎ `push_back`, `push_front`, `pop_back`, `pop_front`, `insert`, `delete`, `clear`, `shuffle` build a list that constraints can then read — **in method form** (`base.push_back(4)` grows the list to 5 and makes `sum` 9). **Previously** the row did not say the form matters: the free-function spelling `push_back(base, 4)` is silently ignored |
| `symbol` declarations | 12.7 | ✎ **moved here from §1, where it claimed "a named activity fragment, expanded in place where it is called".** It is not expanded: `symbol two { do A; do B; }` traversed by `two;` produces one step, `two`, reported `[UNRESOLVED]` with `unresolved action 'two' (no definition in the model)`, and `A`/`B` never run (construct 43) |
| `override` | 20.5 | ✎ **moved here from §1, where it claimed "`type A with B` and `instance path with B` are applied when an action is traversed".** Component-scope overrides are applied (type override `x = 91`, instance override `x = 55`); the **action-scope** form of LRM Example 226 is parsed and ignored (`x = 58` where 91 is required) (construct 80) |
| address claims | 24.8 | ✎ a claim struct instance is allocated first-fit from an address region, so its address is visible to the action — verified. **Previously** the row said this without qualification; the claim's `size` constraint is ignored and the claim is always 64 bytes (§2.1) |
| `read_bytes` | 24.10.9.3 | ✔ fills a list with consecutive bytes from an address |
| `read_struct` / `write_struct` | 24.10.9.4 | ✔ a packed struct is laid out little-endian over memory and read back |
| target-code `{{ }}` templates | 22.2 | an opening `{{` with no matching `}}` in a triple-quoted block is an error |
| collection literal assignment | 8.5 | ✔ `L = {a, b, c};` in `exec` gives a collection its elements |
| floating-point literals | 1 | ✔ `20.14`, `2e6`, `1_0.2_5`; the value is the IEEE-754 bit pattern, as for `float_pkg` |
| based literals | 1 | ✔ `4'b1010` (10), `8'hFF` (255), `64'd2` and the signed forms `4'sd12`, `16'sH_FF` (255) — the token carries the *value*, the size being a width hint |
| escaped identifiers | 4 | `\top-level_c`, `\do-write` |
| the power operator | 8.4 | ✔ `**`, evaluated (`2 ** 10` = 1024) |
| an `open_range_list` mixing values and ranges | 8.5.9 | ✎ **moved here from §1, where the row simply gave `[1, 3, 5..9]` as accepted.** Accepted is not enough: a mixed list is honoured member by member (`[1, 4, 9..12]` and `[1, 3, 5..9]` reach every member), but if the list contains an **open-ended** range the open range is never chosen and the solve collapses onto the first single value — `constraint z in [1, 4, 9..12, 20..]` gave `z = 1` on all 60 seeds tried, and `[9..12, 20..]` only ever produced 9..12 |
| value list literal | 5 | ✔ `[1, 2, 3]` and `[a, b]` as a constraint's value set |
| procedural statements | 89-101 | ✎ **moved here from §1, where the row claimed `repeat`/`repeat…while`/`foreach`/`if`/`match`/`break`/`continue`/`return`/`randomize` in `exec` work "with control flow".** The braced forms work; the single-statement forms are mis-parsed (§2.1), `+=`/`\|=`/indexed assignment are dropped (§2.1), `match` range arms never match (§2.1), `randomize x with { … }` ignores its `with` clause (construct 100), a bare `{ … }` block statement is dropped entirely and a block-scoped declaration leaks into the component (constructs 89, 90) |
| floating-point (`float_pkg`) | 112-116 | ✎ **moved here from §1.** `float_mantissa`, `float_sign` and `to_float` behave; but `float32` does **not** narrow — `6.0` is stored as the 64-bit image `4618441417868443648`, and the field is 8 bytes (construct 112), and `float_exponent` is wrong for most negative inputs (`-6.0` → 1022 where 1025 is wanted, `-0.25` → 1027 where 1021 is wanted; `-1.0` and `-2.0` are right) |
| messaging | 106-110 | ✎ `print`/`message`/`error` are recorded and reported, and file operations are accepted and have no effect — verified. **Previously** the row included `fatal` in the same breath; `fatal()` does **not** terminate the run: the next statement still executes and the process exits 0 (construct 110) |
| randomization functions | 111 | ✔ `urandom()` / `urandom_range()` — deterministic and varying by run seed |
| `x in <collection>` | 8.5.9 | ✎ **moved here from §1, where it claimed "membership in a named array/list is a solved constraint, uniform over its elements".** It is not solved: with `allowed = {4, 7, 9}` and `constraint v in allowed`, `v` solved to 69, 668 and 469 on three seeds, the constraint left unsatisfied (3/4), and the same membership used as an expression evaluated to 0 |

The array work is what lets a **multireg** be modelled directly: OpenTitan's
`KEY_SHARE0` is `array<bit[32], 8>`, and it yields eight modelled registers
rather than needing to be hand-unrolled (see `docs/OPENTITAN.md` §5).

---

## 2. Parsed but still approximate

Listed so the coverage numbers are not read as more than they are. The previous
revision of this list named `sort`, `reverse`, `unique`, `and`, `or` and `xor`
as unimplemented array methods — **that was wrong**: the standard defines no such
methods for arrays (7.9.2.2). The real list is below.

| Construct | Clause | What Satya does today |
|---|---|---|
| array methods `join`, `str_from_chars` | 7.9.2.2 | ✔ not evaluated. String *variables* are modelled (7.2), but these produce a string from a collection and are not implemented — `s = arr.join();` leaves `s` unset, with no diagnostic |
| `to_list`, `to_set` conversions | 7.9.2.2, 7.9.3.2 | not evaluated |
| collection mutators inside a *constraint* | 16.1 | only `exec` mutates a collection; a constraint can read one but not change it |
| `overlap` / `schedule` monitor statements | 19.3.5, 19.3.8 | ✔ each branch is matched independently from the checkpoint; true interleaving of concurrent branches is not modelled — `overlap { do x; do y; }` matches a strictly sequential `a, b` trace, where no instant has both branches active |
| monitor constraints | 19.4 | ✎ **stale, corrected.** The row said "parsed, but a monitor constraint does not filter which steps match". It **does** filter: with a monitor constrained by `x.av == 2`, a match was reported on exactly the 10 of 30 seeds whose step solved `av == 2`, and never otherwise |
| monitor inheritance | 19.6 | ✔ a monitor's base type is parsed but not applied — a derived monitor covered by `cover der_m;` does not sample the base monitor's covergroup |
| `exec` statements other than assignment, `if` and method calls | 10.4 | ✔ `for`/`while` loops and target-code templates are passed through to the backends, not executed. The breakage is wider than this row used to imply — see §2.1 |
| `repeat { … } while (cond);` in an activity | 12.2.5 | ✔ parsed and the condition recorded, but the engine has no state the condition could observe, so the body is traversed once (`while (k < 2)` with `k` pinned to 0 still gives one step) |
| scheduling constraint | 16.2 | ✎ **moved here from §1, where it claimed "parsed and recorded; the engine walks the activity in one deterministic order, so `sequence` holds by construction".** It does not hold. `constraint sequence { a1, a2 };` with the activity written `do a2; do a1;` runs `a2, a1` on every seed — the declared order is ignored, and `constraint parallel { … }` is recorded without effect (construct 62). This is the Clause 16.2 activity-scheduling constraint, not the monitor `sequence { }` statement in §1 |
| `join_first(n)`'s run-time dependency | 12.3.6.1d | ✔ imposes no *scheduling* order (correct), but there is no runtime, so the "first n to complete" part is not simulated |
| multiple `dist` directives on one attribute | 16.1.9 | weights are summed; the LRM leaves the resulting distribution undefined |
| `import`-based scope checking | 9 | ✎ **worse than this row said.** It said "name resolution uses `pkg::name` and `Component::action`, not `import`". `import` has **no effect at all**: with `import fpa::*`, with `import fpa::value`, and with no import statement, the same name resolves to the same function (`value()` returns 22 in all three, and `fpa`'s 11 is never selected) (construct 82). `import class` is a parse error in every spelling tried (construct 87) |
| `exec body` | 22.1 | ✔ not run at solve time, which is correct: the body is *target* code and is emitted into the generated C and SystemVerilog (`// exec body (translated from the PSS model)` in the UVM output, `self->y = 1;` in the C). A *declaration* inside the body is not translated — UVM marks it `// (untranslated) <unparsed>` and the C backend drops the declarator — so only assignments and calls survive. `pre_body` is the solve-time hook |
| register group offsets | 24.11.2 | `reg_group_c` is parsed, but the offsets come from the group's own `get_offset_of_instance` / `get_offset_of_instance_array` / `get_offset_of_path` functions, which are not evaluated |
| monitors declared at package level | 19.1 | ✔ a monitor is collected from a component or action body; one declared directly in a package is not registered, so `cover <name>;` cannot find it. Declare it in the component that uses it |
| `compile has (X)` | 22.3 | ✎ **the row was too kind to it.** It said the expression "evaluates to **true**: a single-file parse cannot see whether a symbol exists elsewhere in the model, and a referenced symbol is normally one that does". It is unconditionally true: `compile if (compile has (NO_SUCH_TYPE_ANYWHERE)) { … } else { … }` takes the then-branch, so the **else branch is unreachable** and a model that guards on a missing symbol silently gets the wrong one (construct 104). `compile if` itself selects the branch correctly from a constant condition, but declarations in the selected branch are lost when the `compile if` is nested, at package scope, or in an action body (construct 103) |
| address trait matching | 24.8.6 | ✎ **stale, corrected — and it was in the wrong direction.** `FEATURES.md` said "traits are not matched"; they **are** matched. A claim constrained to `claim.trait.core == 1` is allocated in the `core == 1` region at `0x9000`, not the `core == 0` region at `0x1000`, and an unmatched trait constraint is reported (`no region matches the claim's trait constraints {core=1}`). What does fail in this area is the claim's **size**, not its traits (§2.1) |
| executor behaviour | 24.10.9.5 | executor components are parsed, but read/write/`addr_value` are implemented in the engine rather than delegated to an executor instance |
| register read/write primitives | 24.10.9 | register access is modelled over a register store, not translated into the primitive byte-level read/write operations on an address |
| `get_tag` | 24.10.8 | ✎ **moved here from §1 and corrected.** The row said it "returns the region's tag *index*; region tags are not mapped to the string names they are declared with, so two tags can be compared but not read". It returns the tag **string**: with regions tagged `"low"` and `"high"`, `where = get_tag(make_handle_from_claim(claim, 0))` yields `"high"` — the string, and the one belonging to the region the claim actually landed in (construct 139) |
| executor-based customisation | 24.10.9.5 | the primitives are implemented in the engine rather than delegated to an executor instance (this row duplicates "executor behaviour" above; kept so the numbering matches `FEATURES.md`) |
| target-code templates (`exec declaration` bodies) | 22.2 | carried through to the backends verbatim, not interpreted |

### 2.1 Defects the v0.46.0 audit found that this document did not mention

Each entry below was reproduced here against the release binary; the numbers in
the Clause column are the `FEATURES.md` row numbers. Several of these are the worst
failure mode a verification tool has — **a wrong answer reported as success** —
so they are called out even where the construct still parses.

| Construct | Clause | Observed |
|---|---|---|
| + `+=`, `\|=` and array-element assignment | 91 | **silently dropped.** Only the plain `=` form takes effect: after `s = r + 1; s += 2;` the field is 2, not 4, and `arr[1] = 9;` leaves `arr[1] = 0`. No diagnostic |
| + single-statement bodies | 94, 97 | **mis-parsed.** `repeat (4) b = b + 3;` iterates once (`b = 3`, not 12) and `if (5 > 3) r = 1; else r = 2;` takes the **wrong** branch (`r = 2`). The braced forms are correct, so the same logic gives different answers depending on whether it is written with braces |
| + `match` range arms | 98 | **never match.** `match (e)` with `e = 5` and a `[4..7] : { m = 2; }` arm falls through to `default`; only an exact-value arm with a braced body works, and unbraced arm bodies are dropped as well (LRM Example 275 form gives 4 where 2 is expected) |
| + resource pools | 49, 50 | **never create instances.** `pool [4] core_r core_pool;` reports `core_pool:core_r=0`, and two parallel `lock`s both resolve to instance 0 and are serialised on contention — a four-instance pool never hands out distinct instances |
| + extension `exec` blocks | 79 | **never run.** `extend action c::A { … }` adds its field and its constraint (the added constraint binds: `x = 77`), but the `exec post_solve` in the extension does not execute (`base_exec` stays 1, the added `ext_field` stays 0), so an attribute the extension introduces stays unusable |
| + composite action `exec` blocks | 83 | **never run.** A `post_solve` declared on an action whose body is an activity is skipped — the leaf's `post_solve` reports its mark, the composite's does not (construct 83). `init_down`/`init_up` and `pre_solve` on the same model are correct |
| + claim `size` ignored | 121, 127, 128 | a claim's `size` constraint has no effect — the claim is always 64 bytes (`cl.size = 64` under `constraint cl.size == 16`) and the model still reports `2/2 satisfied`, so two 100-byte claims sit 64 bytes apart |
| + `add_nonallocatable_region` | 126 | yields no usable handle: `sys_mem.add_nonallocatable_region(r)` returns a handle whose `addr_value` is 0, where the region's address (`0xA0001000`) is required |
| + `addr_value` on a trait-parameterised claim | 139 | returns 0 for a handle built from a transparent claim whose region was chosen by trait, while `claim.addr` is `0x9000` |
| + `bind` statement | 44 | ignored: `bind p2.out c.inp;` leaves the consumer bound to object #0 (the first producer's), and the trace is identical with and without the statement |
| + static `bind <pool> *` | 52 | ignored: with two pools declared `pb` then `pa` and `bind pa *;`, the allocation still comes from `pb` |
| + `bins all[N]` | 66 | declares **one** bin: `bins all[4] = {A, B, C, D}` reports `bins 1/1`, not 4 |
| + covergroup instantiation | 64 | `named_cg inst1 (x);` binds nothing and never samples — no coverage row appears at all, with no warning |
| + `forall` constraints | 58 | not expanded: `forall (it : elem_s) { it.size < 5; }` leaves `e.size = 69` and the counter still reports `1/1 satisfied` |
| + flow-object data, and `state` | 47, 48 | a bound input does not carry the producer's data, and a `state` object is not persisted: the writer sets `mode = 5`, the reader of the same object solves `mode = 269` |
| + `fatal()` | 110 | does not terminate: the following statement executes and the exit status is 0 |
| + action-handle traversal | 30 | `h1;` for a declared action handle is `[UNRESOLVED]` (`unresolved action 'h1'`), and `do sub.s;` is truncated to its first segment |
| + `symbol` | 43 | expands to nothing (see §1) |
| + `export` action declaration | 102 | no effect: `pkg::A1` resolves with no export statement at all |
| + executor claim struct | 119 | an executor claim is routed into address allocation and reported unsatisfiable |
| + value widths and enum values | 8, 9, 11 | declared widths are not enforced (`bit[8]` yields 256, `bit[64]` yields −1, `bool` yields 69) and an enum's declared values are ignored — `{ W0 = 200, W1 = 201 }` gives 1, and `{ A = 5, B, C }` gives 2 for `C` |

---

## 3. Accepted beyond the standard

Satya accepts two constructs that **the standard does not define**, kept because
they are natural things to reach for and are harmless when unused:

| Construct | Status |
|---|---|
| `constraint solve a before b;` | **Not PSS.** ✔ accepted and used to order assignment. `solve … before …` is a SystemVerilog constraint directive; PSS 3.0 has no such construct (its 16.1.8 is *forall constraints*). A conforming PSS model will never contain it. Earlier revisions of this document cited it as "16.1.8", which was wrong on both counts. |
| `<expr> dist { item := w, … }` | **Not PSS.** ✔ accepted, and the weights are honoured (a 1:2 split produced 2:4 over six seeds). The standard's distribution directive is `dist <expr> in [ dist_list ]` (16.1.9); the brace spelling is SystemVerilog's. Satya accepts both because the shared `pss-corpus` uses the brace form — see [`CORPUS.md`](CORPUS.md). |

## 4. How this is checked

* `tests/integration.rs` — ✎ **134 tests**, not 57 as this section previously
  said (the file grew; the count is `grep -c '#\[test\]' tests/integration.rs`
  on the v0.46.0 tree). They include the LRM's Examples 133 and 134 with their
  stated values, the join orderings of Examples 68/71/72 (including
  `join_branch_waits_only_for_the_named_branch` and
  `join_select_waits_for_n_branches`), the `dist` distribution, exec ordering,
  flow-object ordering, arrays, `foreach` and coverage sampling.
* `tests/feature_proof/run_all.sh <binary>` — six self-contained assertion
  scripts, one per slice of the construct table, that re-derive each verdict from
  observed behaviour (solved values, trace structure, reported bins, monitor
  matches). On v0.46.0 they report **104 pass / 58 fail** over 162 checks. They
  are the reproducible evidence for §2.1; `tests/feature_proof/reports/` holds
  the per-slice write-ups, including where a script disagreed with the audit
  (the audit's "all four join forms are indistinguishable" is the clearest case —
  §1 above records what the binary actually does).
* `tools/lrm_examples.py` — runs every extracted standard example and reports a
  lenient and a strict parse bound plus the engine result.
* `tools/ot_regmap_check.py` — diffs the OpenTitan UART model against the IP's
  register specification.

Section 1 is covered by tests and by the assertion scripts. Section 2 and §2.1
are **not** covered by `tests/integration.rs` — those constructs have no
behaviour to test yet, which is precisely why they are listed separately; the
assertion scripts are what pin them down, and they are what should be wired into
CI so a regression in any of them fails a build.
