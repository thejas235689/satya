# The shared PSS corpus, and the rest of the psstools org

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

## The executable (compliance) tier

`compliance/` is the corpus's second tier: instrumented PSS plus legality
models, written against **PSS 3.1**, with a checker that judges a tool's run.
It has no manifest entry, but it is all valid PSS and should parse.

```
  compliance     114 file(s)  114 parsed   0 failed
```

**114/114.** That is the largest single block of externally-authored PSS Satya
has been run against, and it exercises 3.1 surface beyond the 3.0 standard this
implementation targets.

## The rest of the organization

The same check was run against every other `psstools` repository that contains
`.pss` files:

| Repository | Files | Parse | What it is |
|---|---|---|---|
| `pss-corpus` (`curated/`) | 97 | **97** | the manifest-governed buckets |
| `pss-corpus` (`compliance/`) | 114 | **114** | the executable tier |
| `peakrdl-pss` (`tests/golden/expect/`) | 26 | **26** | PSS generated by a register-description tool |
| `peakrdl-pss` (templates) | 7 | 3 | Jinja2 templates, *not* PSS |
| `pssc` | 46 | **46** | a full DMA operation model, plus a bug repro |
| `pssfmt` | 22 | **22** | formatter golden files |
| `sphinx-pss` | 14 | **14** | activity fixtures |
| `agent-skills-pss` | 9 | **9** | prompt rules |
| `pygments-pss` | 14 | 10 | syntax-highlighter snippets |
| `pssutil_lib` | 0 | — | **empty**: a README and a licence, no PSS |

**341 of 356** parse. Of the 15 that do not: 6 are the pathological files that
must fail, 7 are Jinja2 generator templates (`{{ row.name }}`, `{% elif %}`)
rather than PSS, and 4 are syntax-highlighter snippets (`package p;` with no
body) that are deliberately minimal rather than valid.

The 26 `peakrdl-pss` golden files — real third-party-generated register packages
using `packed_s<LITTLE_ENDIAN>` and `addr_reg_pkg` — were also run through every
backend: **156/156 generations succeeded**.

## What running against the rest of the org found

Three more defects, none of which the corpus alone exposed:

- **64-bit literals overflowed.** `0xFFFF_FFFF_FFFF_FFFF` and its decimal form
  `18446744073709551615` are valid 64-bit values that do not fit a signed 64-bit
  integer. Both failed to lex. Found by `pssc`'s
  `repro/instance-array-offsets` — a bug-repro directory whose README notes that
  other tools accept the input. Fixed by parsing literals unsigned and keeping
  the bit pattern.
- **Nested template arguments.** `nested_s<packed_s<bit, 8>>` failed: the
  constant argument was parsed with a precedence that still admitted `>>` as a
  *shift*, so the closing token was swallowed. Found by `pssfmt`. Fixed, and
  three-level nesting works too.
- **Scheduling constraints swallowed a brace.** `constraint parallel { a, b };`
  (PSS 16.2, Syntax 62) was not parsed, and the parser consumed the enclosing
  `}` with it. Found by `sphinx-pss`'s activity fixtures. Now parsed and
  recorded.

## What is still not met

One file: **`pathological/unclosed_mustache.pss`**. It contains an unclosed
`{{` template expression inside target code. Satya treats the contents of a
triple-quoted target-code block as opaque text and does not validate the `{{ }}`
template syntax within it, so it accepts the file. Validating target-code
templates is not implemented.

That is the honest state: 97/97 on what must parse, 6/7 on what must not, and
114/114 on the compliance tier.

## A note on the `dist` spelling

`lexical/operators.pss` writes a distribution as `<expr> dist { item := w, ... }`
— the SystemVerilog spelling. The standard's form is
`dist <expr> in [ dist_list ]` (16.1.9). Satya accepts both; the alternate
spelling is recorded as an extension in `docs/SEMANTICS.md` §3, since a
conforming PSS model will not contain it.
