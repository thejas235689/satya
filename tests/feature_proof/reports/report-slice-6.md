# Slice 6 proof — Satya v0.46.0

Binary: release tarball, sha256 `4a40d468…2e421e` verified before any run. Script:
`proof-slice-6.sh <binary>` — 21 Part A items (126–146), 16 Part B items. Result:
**32 PASS / 5 FAIL**, script exits 1, runtime 4.7 s.

FAIL verdict lines:
    FAIL A126 transparent-region — va2=0(want 2684358656)
    FAIL A127 claim-base — b.size=64(want 8)
    FAIL A128 claim-contiguous — c.size=64(want 16)
    FAIL A132 sizeof-s — nested_bits=0(want 16) typedef_bits=0(want 24) int_bits=0(want 32) bit_bits=0(want 1) enum_bits=0(want non-zero)
    FAIL A138 addr-value-abs — abs0=1(want 1040) abs1=1(want 3072)

PASS verdict lines:
    A129 A130 A131 A133 A134 A135 A136 A137 A139 A140 A141 A142 A143 A144 A145 A146
    B1 B2 B3 B4 B5 B6 B7 B8 B9 B10 B11 B12 B13 B14 B15 B16

## Against docs/FEATURE-AUDIT.md
The slice-6 aggregate (11 PASS, 5 PARTIAL-OK, 3 PARTIAL-WORSE, 2 FAIL over 126–146)
reproduces exactly: FAIL = 132, 138, as the audit's failure table states; the three
extra script FAILs are the audit's PARTIAL-WORSE rows; the documented-partial rows
whose documented limitation reproduces are script PASS.

## Disagreements (reported, not silently adjusted)
1. Per-row verdicts are **not published** in FEATURE-AUDIT.md — only the aggregate and the
   two named FAILs. The 5/3 PARTIAL-OK vs PARTIAL-WORSE split is inferred from FEATURES.md
   + SEMANTICS.md + observation: consistent with the aggregate, but an inference, not a quote.
2. A126/A127/A128 are PARTIAL-WORSE but FAIL here — PASS/FAIL has no partial class.
   The breaks are the audit's own pattern: claim `size` silently ignored (always 64)
   while `constraints: N/N satisfied`, and `add_nonallocatable_region` yields no handle.
3. A139: `get_tag` returns "high", matching FEATURES.md — but SEMANTICS.md §2 says it
   returns the tag *index*, so that row is stale. Separately, `addr_value` on a handle
   from a trait-parameterised transparent claim returns 0 while `claim.addr = 0x9000`;
   undocumented and absent from the audit's failure list.
4. Constructs 145 and 146 have **no models** in `tests/feature_proof/slice-6/` (the
   directory ends at 144); the models here are new, so those verdicts rest on this script.
5. `sizeof_s<bit[24]>` (`132b`) is a parse error — consistent with 132's FAIL note. 129's
   claim size is ignored too (`t.size=64` vs 32), the defect of 127/128; kept PASS because
   129's documented evidence (allocation, address known only after solve) does reproduce.
