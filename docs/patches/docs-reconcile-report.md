# Docs reconciliation — Satya v0.75.0 (build: rust 1.98.1 + zig cc, `cargo build --release`)

Method: every claim below re-run against `./target/release/satya`; harness `tests/feature_proof/run_all.sh` = **128 pass / 34 fail / 162**. Patch: `docs-reconcile.patch` (docs/*.md + README.md only; no source file touched).

| Claim (document, location) | Was | Now — as verified | Evidence |
|---|---|---|---|
| FEATURES 10, 112 float32 | "float64 image; 20.14 → 4626362123713081508" | `float32` = binary32 word: 6.0 → 1086324736 (0x40C00000), 20.14 → 1101078200; `float64` keeps 4618441417868443648 → status `yes` | json backend, 0.75.0 build; fixed in 0.68.0 |
| FEATURES 13 chandle | "carries no value; `h == h` → 0" | opaque id per attribute: `h == h` → 1, two handles → 0 → `yes` | same model, `exec post_solve` |
| FEATURES 138 `addr_value_abs` | "no — constant 1, not the address" | boolean per PSS 3.1 §21.13.7: 1 for a handle in a region on an address space, 0 for a generic claim → `yes` | harness A138 (1, 1); own probe (av 1060 / abs 1; generic claim av 0 / abs 0) |
| SEMANTICS §floating-point, row 136 | "float32 does not narrow; field 8 bytes; float_exponent wrong for negatives" | both fixed (0.68.0 sign fix): −6.0 → 1025 as +6.0, ±0.25 → 1021 | `float_exponent` over ±1.0/2.0/6.0/0.25; 20.14 → 1101078200 |
| SEMANTICS `addr_value_abs` row 115 | clause "21.13.7"; "opaque region not modelled" | re-cited **PSS 3.1 §21.13.7**; the 0 case *is* produced (generic claim); caveat added that `addr_value` is 0 there too | own probe (see above) |
| CORPUS `compliance/` 114/114 · 97/97 · 7/7 | presented as the tier's result | **parse-level only**; tier is executable (trace = `@@PSS-TRACE` via `message()` inside `exec body`, oracle = `test.json`). Satya emits **0** trace lines for all 113 oracle tests; 108 have only `outcome: ok` → uncheckable; the 5 `compile_error` cases are all accepted (exit 0) → **0 of 113 checked, 0 passed** | cloned psstools/pss-corpus; 113 `test.json` (108 `{outcome}`, 5 `{diagnostics}`); 113 solves, grep `@@PSS-TRACE` = 0; `satya check` OK on all 5 |
| FEATURES 8, 11, 21, 49, 50, 90, 91, 94, 97, 98 (stale `worse`) | bool→69; enum→position; `in` unevaluated; both lockers instance 0; block-local clobbers field; `+=`/`arr[1]=` dropped; single-stmt `repeat`/`if`; `match` ranges | all reproduce as fixed: bool 0/1, `{W0=200,W1=201}`→201, `2 in [1,2,3]`→1 / `9`→0, instances 0 and 1, scoped local, `s=4` & `arr[1]=9`, `b=12`, `r=1`, `res=2` → `yes` | own probes + harness passes those rows |
| FEATURES 132 `sizeof_s` | "no — 0 for nested/typedef/enum/int/bit" | 8/1 (packed), 16 (nested), 24 (typedef bit[24]), 32 (enum), 32 (int), 1 (bit); `sizeof_s<bit[24]>` still a parse error → `partial` | harness A132 values + own probe |
| FEATURES 84 functions | "qualified call 0, unqualified 42" | qualified **works** (42) when only defined; with a bodyless prototype before the definition **both** forms give 0, no diagnostic → stays `worse`, note rewritten | two models: definition-only vs prototype+definition |
| FEATURES 87 `import class` | "no — parse error in all four spellings" | all four parse; imported method binds nothing (`h.get(7)`=0) → `worse` | `satya check` OK; harness row 87 |
| FEATURES 34 join spec | "all four indistinguishable, nothing gated" | forms differ in emission order (`join_branch` → A,C,B; `join_select(1)` → B,C,A; `join_none`≡`join_first(1)` → C first; default → C outside group); remaining defect is the group tag → `partial` | five traces; agrees with FEATURE-AUDIT's own reconciliation note |
| FEATURES 110 `fatal()` | "next statement executes, exit 0" | next statement does **not** execute, `FATAL:` recorded; run still exits 0 → `partial` | own probe (`r` stays 0, `FATAL: boom`, exit 0) |
| FEATURES 101 | "the engine runs `exec` bodies to completion" | **`exec body` is never executed** (only pre/post_solve, pre_body, init_up/init_down run) | `exec body { x = 99; }` leaves x = 69; `message()` inside it never printed |
| Conformance figures (FEATURES, SEMANTICS §4, VERIFICATION §7, README, CORPUS) | 104/58, "74 of 146", 134 tests | **128/34 of 162**; 159 `#[test]` in tests/integration.rs; per-slice pass/fail replaced with measured values (19/6, 19/6, 21/4, 15/10, 20/5, 34/3) | harness run; `grep -c '#\[test\]'`; FEATURES' malformed per-slice row (`\| 4 \| 76–100 \| 8 \| 17 \|`) removed |

## Code behaviour no document mentions (found here, recorded in the patch)

- **`exec body` is never executed** — SEMANTICS said the engine "runs `exec` bodies to completion"; VERIFICATION said bodies are "captured (body not interpreted)". No doc stated the consequence: a model whose meaning is in `exec body` is modelled and never run.
- **A component-scope attribute initializer is dropped** — `int t = 1;` at component scope reads 0 inside an action's `exec` (`r = t + 1` → 1; `message(…, t)` prints the argument unresolved), while `exec init_up { t = 1; }` and an action-scope `int t = 1;` both read 1.
- **The C realization truncates wide values** — every field is emitted `int32_t` (chandles `uint64_t`), so `s0.g = 4618441417868443648` warns "changes value … to 0" and the compiled harness prints `g = 0`. JSON/trace are correct; the native C backend is not.
- **chandles now expose an opaque id in the JSON** (`h1: 0`, `h2: 1`) — not documented anywhere.
- **The corpus compliance tier's 5 diagnostic cases are all accepted** (undeclared type/call inside `exec` is not diagnosed) — the docs called this a general "no semantic analysis" boundary, never as 5 concrete failing tests.

Not verified, and marked as such in the patch: every FEATURES row not in the 21 re-tested (still v0.46.0); `float_exponent`'s value convention (raw biased field, 1.0 → 1023) against the standard; the 341/356 org-wide parse table in CORPUS.md (repos not cloned).
