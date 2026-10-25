# The shared PSS corpus

Satya is checked against [`psstools/pss-corpus`](https://github.com/psstools/pss-corpus),
a corpus of PSS source shared by several independent projects. It matters
because its expectations are **set outside this repository**: `manifest.toml`
records, per bucket, whether that bucket is expected to parse. Seven buckets
must parse; `pathological/` must not. That is a harder test than any we could
write for ourselves.

Reproduce with:

```bash
git clone --depth 1 https://github.com/psstools/pss-corpus
python3 tools/pss_corpus_check.py pss-corpus ./target/release/satya
```

The tool exits non-zero if any bucket misses its expectation, so it can gate CI.

## Current result

```
  example2        39 file(s)   39 parsed   0 failed   [ok]
  stdlib           5 file(s)    5 parsed   0 failed   [ok]
  peakrdl         25 file(s)   25 parsed   0 failed   [ok]
  language-ref     9 file(s)    9 parsed   0 failed   [ok]
  pss31            3 file(s)    3 parsed   0 failed   [ok]
  lrm31           12 file(s)   12 parsed   0 failed   [ok]
  lexical          4 file(s)    4 parsed   0 failed   [ok]
--------------------------------------------------------------
  buckets that must parse: 97 passed, 0 failed
  pathological     7 file(s)    6 correctly rejected   1 wrongly accepted
        wrongly accepted: unclosed_mustache.pss
```

**97/97** of the buckets that must parse, and 6 of 7 pathological files
correctly rejected.

## What the corpus found

The first run passed 85/97 and accepted 3 of the 7 files that must be rejected.
Nine distinct defects came out of it. They are listed here because the *kinds*
of bug are more instructive than the count.

### A silent correctness bug, not just a parse gap

`sized literals` (`4'b1010`, `8'hFF`, `64'd2`) lexed as **two tokens** — the
size and then the value — and the parser silently discarded the value. So:

| written | evaluated to | should be |
|---|---|---|
| `4'b1010` | 4 | 10 |
| `8'hFF` | 8 | 255 |
| `64'd2` | 64 | 2 |

Every sized literal in every model was silently wrong, and it *parsed*, so no
error was ever raised. The release notes for 0.3.0 had claimed this was fixed
("sized literals `4'b1010` never lexed"); that was wrong — they lexed, and the
value was lost. This is the single most valuable thing the corpus surfaced.

### Lexer gaps

- **Floating-point literals** (`20.14`, `2e6`, `1_0.2_5`) were not lexed at all
  (Syntax 1). A float's value in the engine is its IEEE-754 bit pattern.
- **Signed based literals** (`4'sd12`, `16'sH_FF`) — the `s` sign marker was
  not handled, so these failed once the quote rule was tightened.
- **Escaped identifiers** (`\top-level_c`, `\do-write`) were not lexed
  (Syntax 4).
- **The power operator** `**` was not in the operator table, so `2**3` was
  neither lexed nor evaluated — and the constraint it appeared in was silently
  dropped rather than reported.
- **A lone `'` or `\`** was silently accepted. Both are malformed; they are now
  errors. `pathological/lone_quote.pss` and `lone_backslash.pss` are the corpus
  files that caught this.

### Parser gaps

- **`open_range_list`** — the right-hand side of `in` may mix values and ranges
  (`[1, 3, 5..9]`, PSS 8.5.9). Only a single range or a list of plain values
  was handled. The parser accepted the mixed form after the fix but the
  *solver* then ignored the ranges, so `[1, 3, 5..9]` yielded only {1, 3}.
- **Qualified constants as extents** — `ch_csr[pkg::N]`, `pool [pkg::N] ...`.
  A plain identifier in an expression did not consume a following `::`.
- **Imported function declarations** — `import target C function ...`,
  including the `static` qualifier (PSS 22.2).
- **Nested package names** — `package pkg1::a::b::c { ... }`.
- **`repeat { ... } while (cond);`** in an activity (Syntax 37), which has no
  repeat count.
- **Monitor statements without braces** — `eventually r;` as well as
  `eventually { r; }`.
- **A declaration's inline range** spanning many lines, as in the `addr_reg_pkg`
  claim types.

### A usability defect found on the way

Parse errors carried no line or column: the position was dropped when the file
path was prepended. Every message in this exercise read
`expected ']', found '::'` with no indication of where. Now they say
`(line 86, col 44)`.

## What is still not met

One file: **`pathological/unclosed_mustache.pss`**. It contains an unclosed
`{{` template expression inside target code. Satya treats the contents of a
triple-quoted target-code block as opaque text and does not validate the `{{ }}`
template syntax within it, so it accepts the file. Validating target-code
templates is not implemented.

That is the honest state: 97/97 on what must parse, 6/7 on what must not.

## A note on the `dist` spelling

`lexical/operators.pss` writes a distribution as `<expr> dist { item := w, ... }`
— the SystemVerilog spelling. The standard's form is
`dist <expr> in [ dist_list ]` (16.1.9). Satya accepts both; the alternate
spelling is recorded as an extension in `docs/SEMANTICS.md` §3, since a
conforming PSS model will not contain it.
