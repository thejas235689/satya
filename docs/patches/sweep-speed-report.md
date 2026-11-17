# sweep-speed — one `--seeds N` per sweep instead of one process per seed
## Wall clock — `./tests/feature_proof/run_all.sh`, same binary, back to back
run 1: 207.9 s → 61.1 s (3.4x).  run 2: 158.4 s → 50.3 s (3.1x).
satya invocations per slice: 1: 291→43, 2: 151→34, 3: 996→48, 5: 41→34 (slices 4 and 6 have
no seed sweep). Process creation costs ~21 ms here for *any* exec, so the win comes from
removing the per-seed process entirely, not just the per-seed satya startup.

## Counts unchanged
`TOTAL: 128 pass, 34 fail, 162 checks` on both sides, identical per-slice split
(19/6, 19/6, 21/4, 15/10, 20/5, 34/3). run_all exits 1 both times — the 34 known failures.

## Diff evidence — output byte-identical
md5 of captured stdout+stderr, before vs after, equal in every case:
slice-1 `1c263db0`, slice-2 `107806ec`, slice-3 `fe8558c3`, slice-4 `5241874a`,
slice-5 `58a607e9`, slice-6 `5c677542`, conformance `c264d394`, rand_cov `670e477d`,
repro `f84d1c65`. Whole-run `run_all.sh` output: `5c6d1e06` both sides. The patch applied to
a clean checkout of `c92d8d2` reproduces that same output (50.3 s).

Tools: conformance_check.py 11.6→0.67 s, rand_cov_check.py 22.4→0.68 s, repro_check.py
3.4→0.47 s — all three print byte-identical reports.

## What changed (7 files, none under `src/`)
Each seed sweep is now one `--seeds N` invocation. That separates runs with
`--- seed N ---` (trace) / `/* --- seed N --- */` (generated code), so the marker is split
**once** by a single awk into a `<seed> <v1> <v2> …` table the loop then walks with shell
builtins — no parser assumes one run per invocation, and no per-seed process remains. A
model the tool refuses falls back to one run per seed, so error paths (exit status, `err1`)
are untouched. Out of scope by inspection: `pss_corpus_check.py` has no seed loop (it loops
over corpus files via `parse`), and `examples/` holds only `.pss` models and READMEs.
