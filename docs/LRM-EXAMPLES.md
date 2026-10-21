# LRM Example Conformance

Satya was run against **every code example in the PSS v3.0 standard**, extracted
automatically from the PDF, to check that the features the standard demonstrates
are actually implemented.

Source: *Portable Test and Stimulus Standard 3.0*, Accellera, August 2024
(485 pp.). Extracted examples live in `examples/lrm/`.

---

## 1. Method

1. **Extract.** `tools/lrm_extract.py` walks the LRM text, finds every
   `Example N—Title` marker (skipping the table of contents), and captures the
   code body that follows — across page breaks, dropping running headers,
   footers and page numbers, stripping elision markers (`...`), a leading
   prose/BNF prologue, and any trailing BNF listing. Each example is written to
   `examples/lrm/NNN_slug.pss`.
2. **Classify.** Each snippet is labelled `component`, `package`, `action`,
   `declaration`, `expression` or `coverage`.
3. **Run.** `tools/lrm_examples.py` compiles each snippet. Because most LRM
   examples are *fragments* (a bare action, a struct, a single field), each is
   tried with four minimal wrappings (`as-is`, `component`, `package`,
   `struct-in-package`) and the first that works is taken — so a pass means the
   **construct** is supported, not that the framing happened to work.
4. **Execute.** Every snippet that parses is run through the engine
   (`satya solve`) to check that it schedules.

Reproduce:

```bash
python3 tools/lrm_extract.py                 # needs the LRM text (see the script)
python3 tools/lrm_examples.py ./target/release/satya
```

---

## 2. Results

| Metric | Count |
|--------|-------|
| Example markers in the LRM | 321 (317 distinct examples) |
| Examples yielding extractable code | **197** |
| Dropped — prose-only, grammar listing, or fully elided | 124 |
| …of the 197, `expression` snippets (literals/operators, not models) | 37 |
| **Model snippets** | **160** |
| Models parsed without error | **157 / 160 (98.1%)** |
| Models parsed with real content beyond the wrapper | **121 / 160 (75.6%)** |
| …of the parsed models, those containing an `activity {` | 73 |
| …and those that schedule ≥ 1 step | **55** |

**Reading the two parse figures.** Satya's parser is deliberately lenient — it
skips constructs it does not model rather than failing — so "parses without
error" is an **upper bound** and "parses with real content" is a **lower bound**.
The engine figure is the strong signal: a silently-skipped activity schedules
nothing.

By kind (no-error parse):

| Kind | Parsed | Failed |
|------|--------|--------|
| `component` | 78 | 2 |
| `action` | 45 | 1 |
| `declaration` | 25 | 0 |
| `package` | 9 | 0 |

---

## 3. Defects found and fixed

Running the LRM's own examples exposed **eleven** genuine gaps. All are fixed
and covered by tests.

| # | Construct | Found via | Fix |
|---|-----------|-----------|-----|
| 1 | Sized literals never lexed — `4'b1010`, `8'hFF` produced *"invalid integer literal"* | Ex30, Ex135 | dedicated sized-literal scanner (base letter + radix) |
| 2 | `'` was always treated as a sized-literal prefix, breaking `'{ … }` aggregate literals and quoted text in comments | Ex40, Ex213, Ex245 | `'` is punctuation unless followed by a digit or base letter |
| 3 | `'{ … }` aggregate (assignment-pattern) literals | Ex30, Ex135 | parsed as an aggregate literal in expressions |
| 4 | Constant (non-type) template arguments — `array<bit[15:0], 5>`, `sized_addr_handle_s<32>` | Ex14, Ex312 | template arguments accept constant expressions |
| 5 | …and the bug behind it: the constant was parsed at full precedence, so the closing `>` was swallowed as a comparison operator | Ex135, Ex312 | constants parsed above comparison precedence |
| 6 | `ref` reference types — `ref my_comp x;`, `list<ref my_comp>` | Ex24 | `ref` accepted as a type qualifier |
| 7 | `import P3::P4 as P2;` — package aliases | Ex245 | import aliases parsed |
| 8 | `pure component` / `pure action` | Ex317 | `pure` modifier accepted |
| 9 | `compile assert ( … )` | Ex285 | parsed (recorded, not evaluated) |
| 10 | Open-ended ranges — `rand int in [1..] y;` | Ex14 | `[lo..]` and `[..hi]` supported |
| 11 | Triple-quoted strings — `""" … """` | Ex269 | lexed as a single string |
| 12 | Qualified extension — `extend action C::A { … }`, and merging the extension into the existing action | Ex27 | scoped names accepted; extensions merge |

Defects 1 and 5 are the instructive ones: in both cases a construct that had
*never* worked silently broke a later one, and neither was reachable from the
DVCon workshop corpus alone.

---

## 4. Residual failures

Three snippets still fail to parse. All are artefacts of extracting code from a
paginated PDF, not parser defects — evidence is included so this is checkable.

| Example | Cause |
|---------|-------|
| Ex85 `match statement` | truncated at a page break — braces unbalanced (`{`×3, `}`×2) |
| Ex269 `comments` | the `"""` block runs past the page; snippet ends mid-string (`{`×8, `}`×5) |
| Ex312 `complex data structures` | component body truncated at a page break (`{`×6, `}`×5) |

A further 36 model snippets parse leniently but produce no content. Sampling
them shows they are fragments and non-model lines rather than parser gaps — for
example `int c1[4] = {1, 2, 3, 4};` at top level, or a line of prose with a
brace in it. They are counted in the lower bound, not the upper.

---

## 5. Out of scope

**Coverage** constructs (`monitor`, `cover`, `covergroup`, Clause 18/19) are
parsed and skipped, not executed. Satya is a stimulus engine; it does not model
coverage collection. Examples in this category are labelled `coverage` and
excluded from the pass counts rather than counted as failures.

---

## 6. Regression check

The parser changed, so the previously verified corpora were re-run:

* DVCon 2025 workshop examples — **17/17** files parse, **36/36**
  project × backend generation combinations succeed
* Tests — **32/32** pass (6 new tests cover the constructs above)
