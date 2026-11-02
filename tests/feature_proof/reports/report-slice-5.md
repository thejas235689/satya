# Slice 5 — PSS constructs 101–125, re-derived by assertion
`proof-slice-5.sh <satya>` on v0.46.0 (sha256 4a40d468…2e421e, verified): 18 pass, 7 fail, ~6 s.
Every verdict comes from observed behaviour — exit status, solved values, messages, warnings, file side-effects — never from the parser's report.

PASS 101 Procedural yield statement
FAIL 102 Export action declaration — no effect: pkg::A1 resolves with no export statement at all
FAIL 103 compile if declaration — declarations lost when nested (inner_a), in an action body (kept) and at package scope (p::B = 0, not 22)
FAIL 104 compile has expression — true for a name declared nowhere, so the else-branch is unreachable
PASS 105 compile assert statement
PASS 106 String formatting and output functions
PASS 107 Runtime messaging function
PASS 108 Text file operations using file handles
PASS 109 Simple text file operations
FAIL 110 Error reporting functions — fatal() does not terminate the run: the next statement still executes and the exit is 0
PASS 111 Randomization functions
FAIL 112 Floating-point storage types — float32 keeps the float64 image (6.0 → 4618441417868443648, not 1086324736)
PASS 113 float_mantissa function
PASS 114 float_exponent function
PASS 115 float_sign function
PASS 116 to_float function
PASS 117 Executor component
PASS 118 Executor group component
FAIL 119 Executor claim struct — an executor claim is routed into address allocation and reported unsatisfiable
PASS 120 Executor query function
FAIL 121 Generic address space component — claim.size == 100 ignored (claim is 64 B, the next 64 B away) yet counted satisfied
PASS 122 Contiguous address space component
PASS 123 Transparent address space component
PASS 124 Base address region type
PASS 125 Contiguous address space region type

## Agreement with docs/FEATURE-AUDIT.md

Slice 5 counts: 9 PASS, 9 PARTIAL-OK, 5 PARTIAL-WORSE, 2 FAIL. Script PASS 18 = 9 + 9, FAIL 7 = 5 + 2 — the split matches exactly. Row reading used: PASS 101,105,106,107,111,113,114,115,116 · PARTIAL-OK 108,109,117,118,120,122,123,124,125 · PARTIAL-WORSE 102,110,112,119,121 · FAIL 103,104. The audit publishes only the aggregate for this slice, so the row-level split is inferred from behaviour, not copied.

## Disagreements

1. **104** — the audit's failure table names only 103 for slice 5, but its counts need two FAILs. I report 104 as the second: a name declared nowhere takes the then-branch, which is silently wrong, not merely "not evaluated" as FEATURES.md says. If the audit's second FAIL is another row, this is where we differ.
2. **121** — FEATURES.md says "traits are not matched"; they are matched (a claim constrained to trait.core == 1 lands in the core == 1 region). My trait assertion passes; the row fails only on claim size. The note is stale — the row is worse than documented, but for a different reason.
3. Not counted in the 25: float_exponent() is correct for 6.0/1.0/2.0 (114 PASS, as the project's model asserts) but wrong for most negative inputs — -6.0 → 1022 (want 1025), -0.25 → 1027 (want 1021), while -1.0 and -2.0 are right.
