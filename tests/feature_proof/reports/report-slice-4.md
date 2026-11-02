# Slice 4 proof — constructs 76–100 (Satya v0.46.0)

`proof-slice-4.sh <satya>` on the verified release binary (sha256 4a40d468…2e421e OK): **8 PASS / 17 FAIL**, 25 constructs, 5.4 s, assertions on observed effects.

PASS 78 monitor constraints
PASS 81 package declaration
PASS 85 function definition
PASS 92 void function call
PASS 93 procedural return statement
PASS 95 procedural repeat-while statement
PASS 96 procedural foreach statement
PASS 99 procedural break/continue statement
FAIL 76 schedule monitor activity — overlap matches a sequential trace (1 hit, expected 0)
FAIL 77 monitor traversal — by type and by handle both 0 hits; flat control 1 hit
FAIL 79 type extension — extension exec never runs: base_exec=1, ext_field=0
FAIL 80 override declaration — action-scope override (LRM Ex.226) ignored: x=58, expected 91
FAIL 82 import statement — `import fpa::*` and `import fpa::value` both give 22, as with no import
FAIL 83 exec block declaration — composite action's exec post_solve never runs
FAIL 84 function declaration — qualified `fn_p::sum10(4,2)`=0; unqualified returns 42
FAIL 86 imported function qualifiers — bodyless prototype yields 0, no diagnostic
FAIL 87 import class declaration — parse error in 4/4 spellings
FAIL 88 target-template function — template neither evaluated nor reported: compute(21)=0
FAIL 89 procedural block statement — `{ in_block = 5; }` leaves 0; block dropped
FAIL 90 procedural variable declaration — block-local `int t = 7;` overwrites component field t
FAIL 91 procedural assignment statement — `s += 2` and `arr[1] = 9` silently dropped
FAIL 94 procedural repeat-count — `repeat (4) b = b + 3;` runs once (b=3, expected 12)
FAIL 97 procedural if-else — `if (5 > 3) r = 1; else r = 2;` yields 2
FAIL 98 procedural match — range arms never selected; unbraced arm bodies dropped (Ex.275 → 4)
FAIL 100 procedural randomize — `randomize res with { res > a; }` ignores the with-clause (0)

## Disagreement with docs/FEATURE-AUDIT.md
The audit publishes slice-4 **totals only** (9 PASS / 1 PARTIAL-OK / 12 PARTIAL-WORSE / 3 FAIL), no
per-row verdicts, so rows cannot be matched one by one. I derive **8 PASS / 1 / 13 / 3**: the three
audit FAILs (82, 87, 89) and the one PARTIAL-OK (76, the documented "branches matched independently"
limitation) reproduce exactly, but PASS is one short and PARTIAL-WORSE one over.
Likely source rows, where I assert more than the published models exercise: **80** (its model omits the
`;` after the override block, so its activity never runs; with the `;` the override is still ignored),
**91** (`+=`, indexed lvalues; docs/CORPUS.md: `|=` is valid PSS the grammar lacks) and **94/97** (bare
single-statement bodies). No expected verdict was changed to fit — this is the evidence.
