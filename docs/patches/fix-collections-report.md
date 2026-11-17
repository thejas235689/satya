# Rows 6 (map literal) and 15 (set dedup) — closed

**Baseline measured (clean c92d8d2):** proof **128 pass / 34 fail / 162 checks**; `cargo test` **178** (19 lib + 159 integration) — matches the stated v0.75.0 figures.
**After:** proof **130 pass / 32 fail**; `cargo test` **178/178**. A verdict diff of all 162 checks shows **exactly two lines moved: `FAIL 6 → PASS 6`, `FAIL 15 → PASS 15`.**

## What was wrong
`{1:10, 2:20}` never reached the engine as pairs: the `{...}` operand form parsed `1:10` as `Int(1), Unknown(":")`, so the "map" was six junk list items → `size 6`, and `m[1]` had no key to read (`0`). And every collection method shared one positional implementation, so `insert` shifted like a list and `push_back` ignored set uniqueness.

## The fix (src/parser.rs, src/engine.rs only)
- **Parser:** a `key : value` item in a brace literal becomes one `Expr::Binary(":", k, v)` node — the shape the existing `'{x:3}` keyed form already emits (PSS Syntax 6, §4.8.3). A lone `{1:10}` stays a one-item list rather than collapsing to a bare `":"`.
- **`CollKind`:** a classifier resolves a variable's declared type (through typedefs) to Array/List/Set/Map; the family — not the flat `BTreeMap<String,i64>` — picks the storage discipline.
- **Map assignment:** entries keyed at `<name>[<key>]`; size = **distinct keys**; a repeated key keeps the **last** value; prior entries cleared by prefix (a map's keys are not `0..size`).
- **Map `insert(k,v)`** associates and replaces in place (size grows only for a new key); **`clear`** on a map clears by prefix.
- **Set dedup:** `push_back`/`push_front` and **value-list literals** (`s = {7,7,8}`) store each value once (PSS §4.8.2: "each value is counted only once").

## Observed (row assertions, seed 1)
Row 6: `sz_lit=2`, `got=10`, `sz_ins=3`, `got3=30` — all four pass. Row 15: `sz_st=2`, `st.size=2`, `st[0]=7`, `st[1]=8`, with `arr.size=3`, `a1=5`, `li.size=2`, `l1=20` unchanged. Extra probes: duplicate key → last wins; non-contiguous `{1:10,100:20}` → size 2, `n[100]=20`; single pair → size 1; `insert` on an existing key → replaced, size 2.

## Ruled out
- **No new `Expr::Map` AST variant.** `backend::render_sv` matches `Expr` exhaustively and `src/backend/*` is out of scope, so a new variant cannot compile without editing a forbidden file. The pair encoding reuses the codebase's keyed-literal idiom, and the **declared type** — not the literal's shape — decides map vs struct vs array.
- **No `src/ir.rs` change:** `Field` is constructed in `src/frontend/json.rs` (outside scope), so the family is derived in the engine. Zero ir.rs/backend/bdd/docs edits.
- **Still wrong, left alone:** `m.erase(k)`, map `foreach` over keys, `.size()` on an uninitialised map (absent, not 0). Neither row exercises them.

## Caveats
- **The four staging patches were absent** — `/scratch/input` did not exist in this sandbox. This diff is against **clean c92d8d2**, not the patched tree. It touches only `src/parser.rs` and `src/engine.rs`, which none of those four patches name, so it should still apply on top.
- **The PSS PDF was absent too.** §4.8.2/§4.8.3 were read from the published v3.1 text; the §7.9.4/7.9.5 body was not retrievable, so method semantics rest on those literal rules plus the rows' own assertions.
