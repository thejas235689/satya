# Slice 2 (PSS constructs 26–50) — verdicts re-derived by assertion

`proof-slice-2.sh <binary>` embeds 25 models and derives each verdict from observed behaviour (step counts, order, group ids, solved values, flow-object ids). Against Satya v0.46.0 x86_64-linux (sha256 4a40d468…2e421e verified): 39 s, exit 1.

## Verdicts
PASS 26 Template value parameter declaration
PASS 27 Template type parameter declaration
PASS 28 Template type instantiation
PASS 29 activity statement
FAIL 30 Action traversal statement — handle traversal `h1;` is [UNRESOLVED]; `do sub.s;` truncated to `sub`, [UNRESOLVED]
PASS 31 Activity sequence block
PASS 32 Parallel statement
PASS 33 Schedule statement
FAIL 34 Activity join specification — the statement after the block lands inside the concurrent group in all four forms; join_none and join_first traces byte-identical
PASS 35 Atomic block
PASS 36 repeat-count statement
FAIL 37 repeat-while statement — condition never re-evaluated: `while (k<2)` with k pinned to 0 still gives 1 step
PASS 38 foreach statement
PASS 39 select statement
PASS 40 if-else statement
PASS 41 match statement
PASS 42 replicate statement
FAIL 43 symbol declaration — `two;` expands to nothing: 1 step (`test`), no A/B/C, no warning
FAIL 44 bind statement — `bind p2.out c.inp` ignored: consumer takes object #0 (first producer); trace identical without it
PASS 45 buffer declaration
PASS 46 stream declaration
FAIL 47 state declaration — not persisted: writer sets mode=5, reader of the same object solves mode=269
FAIL 48 Flow object reference — bound input carries no data: object holds size=7, consumer's input solves 269
FAIL 49 resource declaration — two parallel lockers on a 4-instance pool both get instance 0; contention reported
FAIL 50 Resource object reference — both parallel lock references resolve to instance 0; distinct instances never assigned

## Against docs/FEATURE-AUDIT.md, slice 2
The audit records only a slice-2 aggregate (15 PASS, 3 PARTIAL-OK, 6 PARTIAL-WORSE, 1 FAIL) and names #44 as its single FAIL; it holds no per-construct verdicts for 26–50, so only #44 and the tally can be compared.
**Agrees:** #44 bind — reproduced exactly (identical traces with and without the statement).
**My FAILs in the audit's vocabulary:** FAIL {34, 44}; PARTIAL-WORSE {30, 37, 43, 48, 49, 50}; PARTIAL-OK {46, 47} (documented limitations reproduce); PASS 16.
**Disagreement 1 — #34 join specification.** I FAIL it: every join form leaves the following statement inside the concurrent group, so nothing is gated and join_branch, join_first, join_none and join_select are indistinguishable. The audit's tally admits one FAIL only, so it must have counted #34 PASS or PARTIAL. Expected verdict not adjusted.
**Disagreement 2 — counts.** Mine 16/2/6/2 (PASS/PARTIAL-OK/PARTIAL-WORSE/FAIL) against the audit's 15/3/6/1: one construct I count PASS is one the audit counted PARTIAL-OK; its aggregate-only record cannot say which.
**Notes, no verdict change.** #26 passes on the struct-template form, but a component-scope value parameter never reaches `bit[W]` (200 in `c26<4>` stays 200). #33 schedule differs from parallel only by its mode label — its branches still share one concurrent group id. #35 atomic is behaviourally identical to the sequence twin, which a sequential walker makes expected. #45 passes (object allocated and bound to both actions) while the data never reaches the consumer — that defect is reported under #48.
