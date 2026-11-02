# Slice 1 — assertion proof re-run (Satya v0.46.0, sha256 4a40d468…2e421e)

`./proof-slice-1.sh <path-to-satya>` prints these 25 lines and exits 1 (13 assertions fail).

PASS 1 Numeric constants
PASS 2 String literals
PASS 3 Aggregate literals
FAIL 4 Empty aggregate literal — sz_after=3 (expected 0)
PASS 5 Value list literal
FAIL 6 Map literal — m={1:10,2:20} gives size 6 (expected 2); keyed read m[1] returned 0 (expected 10); after insert size 7 (expected 3); audit: PARTIAL-OK — 'maps are not solved' reproduces
FAIL 7 Structure literal — keyed form '{x : 3, y : 4} rejected: error: m07a.pss: expected `}`, found `:` (line 6, col 38); positional '{3,4} leaves p.x=0 p.y=0 (wrote p[0]=3 p[1]=4); audit: FAIL
FAIL 8 Data types and data declarations — flag=69 not in {0,1} at seed 1; audit: PARTIAL-WORSE — bool is not confined to its two values
FAIL 9 Integer type declaration — bad8=256 out of [0..255] at seed 1; bad64=-1 out of [0..18446744073709551615] at seed 1; bad4=16 out of [0..15] at seed 1; b1=69 not in {0,1} at seed 1; declared width not enforced; audit: PARTIAL-WORSE
FAIL 10 Floating-point type declaration — f32=20.14 stored as 64-bit pattern 4626362123713081508 (binary32 is 1101078200): float32 is 8 bytes; audit: PARTIAL-WORSE — float32 does not narrow
FAIL 11 enum declaration — enum wide_e : bit[8] { W0 = 200, W1 = 201 } gives w=1 (declared value ignored; position used); enum seq_e { A = 5, B, C } gives s=2 under 'in { A, C }' (A=5, C=7); audit: PARTIAL-WORSE
PASS 12 string declaration
FAIL 13 chandle declaration — h == h evaluates to 0 (expected 1); handle carries no value; audit: PARTIAL-OK — 'opaque handle' reproduces
FAIL 14 struct declaration — inherited member d.n constrained to 6 binds nothing (read back 0); audit: PARTIAL-WORSE
FAIL 15 Collection data types — sz_st=3 (expected 2); st.size=3 (expected 2); set<int> kept a duplicate: 7 pushed twice, 8 once; audit: PARTIAL-WORSE
FAIL 16 ref declaration — write through ref alias_r.v=99 left real_obj.v=42 (write did not propagate); audit: PARTIAL-OK — documented limitation reproduces
PASS 17 User-defined type declaration
PASS 18 cast operation
PASS 19 Expressions and operators
PASS 20 Conditional operator
FAIL 21 Set membership operator — lit_hit=0 (expected 1); audit: PARTIAL-WORSE — membership over a literal list is not evaluated
PASS 22 String slice operator
PASS 23 component declaration
PASS 24 action declaration
FAIL 25 Template type declaration — rand templated field small.f=__ABSENT__ under 'small.f == 200' (no field of the instantiation is reported); rand templated field small.g=__ABSENT__ under 'small.g == 33' (bound nothing); audit: PARTIAL-WORSE — rand templated fields bind nothing while the counter reports 2/2 satisfied

## Reconciliation with docs/FEATURE-AUDIT.md
Audit slice 1: 12 PASS / 3 PARTIAL-OK / 8 PARTIAL-WORSE / 2 FAIL. This script: 12 PASS / 13 FAIL, where FAIL means "not as the standard requires" — the audit's other three verdicts all land there, and each such line names the audit verdict it corresponds to. The tags line up construct by construct: 6/13/16 are the documented-partial rows, 8/9/10/11/14/15/21/25 the eight PARTIAL-WORSE rows, 4/7 the two FAILs the audit names. Both counts match exactly.

## Where an assertion disagreed with the audit
14 — the audit says nested *and* inherited struct members "bind nothing". Nested DO bind: `o.in.a == 77` reads back as 77 through a mirror field. Only the inherited member (`d.n == 6`, reads back 0) binds nothing. Verdict stands; the audit's stated reason is half wrong.
11 — the audit's model (`RD=0, WR=1, IDLE=2`) cannot tell declared values from declaration positions. They are ignored: `{W0 = 200, W1 = 201}` gives 1, and `{A = 5, B, C}` gives 2 for C. Verdict unchanged, defect newly demonstrated.
23 — the audit has no model for 20-25 at all (slice-1 models stop at c19); those six were written here. With two top-level components and no explicit top, the step's component label is seed-dependent, so each component is probed in its own model and the label is not asserted.
Note — the json `constraints` counter reports "satisfied" while the values are wrong (`u8 = 256` prints 2/2; a constrained templated field binds nothing and prints 2/2). Every assertion reads solved values, never the counter.
