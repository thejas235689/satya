# Changelog

Notable changes to **Satya**, newest first. Every entry corresponds to a tagged
[release](https://github.com/thejas235689/satya/releases); the linked notes carry
the full detail, including verification figures and the caveats that come with
each change.

Satya is a working engine, not a certified PSS implementation — see
[`docs/FEATURES.md`](docs/FEATURES.md) for a per-construct matrix over all 146
syntax excerpts the standard defines, and [`docs/SEMANTICS.md`](docs/SEMANTICS.md)
for what is implemented per the standard's defined behaviour and what is only
approximated.

---

## 0.41.0 — 2026-10-29

`soft` constraints are a preference now

Found by the conformance examples: `constraint soft lo == 0;` was dropped
entirely. `soft` appeared nowhere in the parser, IR or solver, so the constraint
never reached the atom list — and was not counted in `constraints: n/n` either,
which is what made it hard to notice:

```
seed 1 -> hi=74 lo=73   constraints: 2/2      (three constraints written)
```

The fix is a preference, not a check (16.1.4):

- the parser marks the constraint `is_soft`, and the IR carries the flag;
- the solver collects soft constraints separately and appends their atoms
  *after* the hard ones, recording where they start;
- after the solve, any soft atom left unsatisfied causes one retry with the
  soft set excluded. So a soft constraint is honoured when it can be, and
  dropped when it conflicts with a hard one.

Both directions verified:

```
soft lo == 0, hard lo == 5   ->  lo=5   constraints: 2/2   (hard wins)
soft lo == 0, nothing else   ->  lo=0   constraints: 2/2   (preference honoured)
```

`examples/conformance` is now **all checks passing**, and `cn5_soft_inline.pss`
went from 0/20 to 20/20 seeds.

131 tests (1 new, covering both directions). Corpora unchanged: 97/97 curated,
7/7 pathological, 114/114 compliance, error-suite syntax 86/165, controls
113/113.

## 0.40.0 — 2026-10-29

PSS 3.1 target runtime: what Satya does and does not do

Two models from the PSS 3.1 public-review write-up on the target runtime —
`sync_pkg` channels (21.9) and `export function` (20.4.2) — are now in
`examples/pss31/`, reproduced as published, with a README recording the result.

**Both parse. Neither feature is implemented.**

- `noc_channel.pss` parses and schedules correctly: two steps in `parallel`,
  `strm=69 threshold=79`, constraint satisfied. But `channel_c` appears nowhere
  in the engine, IR or solver — `put`/`get`/`try_put`/`try_get` are parsed as
  ordinary calls and ignored, so the handshake does nothing.
- `dma_export_irq.pss` parses. `export function` has no implementation, because
  it needs something for the environment to call *into*, and Satya has no
  target runtime.

`import target function` declarations are parsed and recorded; the backends emit
the calls rather than executing them.

**Why not fixed here.** A channel FIFO alone would make the example look like it
works while the semantics it depends on are absent: Satya runs `exec` bodies to
completion in one pass, so there is no place for `get()` to block and no other
exec block to keep running while it does — which is precisely the obligation
20.8 puts on the tool. An attempt at the FIFO was made and removed: it compiled,
did not fire, and a version that does not fire is worse than none.

This is the largest remaining gap and the one that matters most for 3.1:
everything else Satya does is settled before the test starts.

130 tests, corpora unchanged: 97/97 curated, 7/7 pathological, 114/114
compliance, error-suite syntax 86/165, controls 113/113.

## 0.39.0 — 2026-10-29

The exec error path, traced rather than guessed

Three releases of guesswork went into this cluster for a net gain of one rule.
This time I traced one case instead of assuming, and the cause was visible in a
single run.

**What was actually wrong.** In the `exec` statement parser's assignment branch:

```rust
Err(er) => {
    // empty
}
```

An unparsable right-hand side was discarded, the statement became
`ExecStmt::Raw`, and `Raw` is accepted. So `x = foo(1, 2,)` — and anything else
with a broken RHS — parsed cleanly. `parse_exec_stmts` also returned
`Vec<ExecStmt>` with no error channel at all.

**What changed.** The parser keeps the first error it meets (`exec_err`) and
`parse_exec_stmts` returns it alongside the statements; `parse_exec` turns it
into a `PssError`. A missing `;` after an `exec` assignment is now an error too,
with a multi-declarator declaration (`int e = 1, f = e + 1, g;`) recognised as
legal rather than a missing semicolon.

pssparser syntax cases Satya rejects: **77 → 86**. Curated corpus stays
**97/97 · 7/7 · 114/114**, error-suite controls **113/113**.

The rule that this new path closes, beyond the trailing comma:
`missing_close_paren_call`, `missing_comma_call_args`,
`missing_semicolon_import`, `dangling_dot`, and several `missing_semicolon`
forms.

130 tests (1 new). The lesson, finally applied: trace the case to the function
that consumes it before writing the check.

## 0.38.0 — 2026-10-29

The punct/expr clusters: a diagnosis, and one rule

I set out to close `syntax.punct` (21 cases) and `syntax.expr` (9). What I found
is more useful than the cases: **they are not where I assumed.**

- **A trailing comma in an argument list** is now rejected. That is the one rule
  that landed, and it works wherever the expression parser handles the call.
- The rest do not live in `exec` bodies at all. `constraint { x + ; }`,
  `== y` and `x = ;` are **constraint expressions**, and the constraint
  expression parser tolerates a dangling operator. `exec` bodies go through a
  separate token-level parser that returns no errors at all.

So the clusters sit behind two parsers that have no error path:
`parse_exec_stmts` (token-level, returns `Vec<ExecStmt>`, no `Result`) and the
constraint expression parser (returns `Expr`, tolerant of a missing operand).

I built a validator over the `exec` token stream and **removed it**: it caught
none of these cases, because the cases are not exec statements. Shipping dead
machinery that could reject valid models is worse than shipping nothing.

The real work is to make those two parsers error-aware. That is a signature
change across seven call sites, not a rule, and it is the honest prerequisite
for `punct` and `expr` rather than something to work around.

Curated corpus **97/97 · 7/7 · 114/114**, error-suite syntax **77/165**,
controls **113/113**. 129 tests (1 new).

## 0.37.0 — 2026-10-29

Reserved words, done per the clause

Last release I reverted the rule that a reserved word cannot name a field
(12.2), because it broke the curated corpus from 97 to 55. That was the wrong
call: the clause is right and the parser was wrong. Implementing it properly is
what this release does.

The rule never actually conflicted with legal PSS. Five constructs put a keyword
where a field name could be, and each is now parsed as its own declaration:

- **`target function`** (22.5) — a target-template implementation
- **`solve function`**, **`pure function`** — function qualifiers
- **`abstract monitor`** (19.1)
- **`extend action`** (20.5)

With those recognised, the reserved-word rule sees only genuine misuse, and:

- curated corpus **stays 97/97 · 7/7 · 114/114**
- pssparser syntax cases Satya rejects: **70 → 77**
- semantic warnings: 0 → 1
- error-suite controls **113/113**

The lesson from 0.36.0 stands but points the other way from how I applied it:
when a rule breaks the corpus, the question is *which legal construct the parser
is misreading*, not whether the rule is wrong.

128 tests (1 new, covering both directions).

## 0.36.0 — 2026-10-29

Leniency: declarations in the wrong scope

Four rules, each closing a cluster rather than a file. pssparser's syntax cases
Satya rejects: **66 → 70**, with the curated corpus unchanged at 97/97 · 7/7 ·
114/114 and the error-suite controls at 113/113.

- **`action` at file scope** is an error (12.3 — actions are declared inside a
  component). This is the rule I tried and reverted last release; it works now
  because the fixtures were conformed first, which was the lesson.
- **A `component` inside an action body** is an error instead of being skipped.
- **`bins` and `coverpoint` outside a covergroup** are errors (18.2). They were
  silently skipped wherever they appeared.

The three fixtures that declared a bare action were wrapped in a component, so
they test target-code templates against a standard-conforming model.

**One rule tried and reverted:** rejecting a reserved word as a field name
(`int action;`) — ten cases, and it looked clean. It broke the curated corpus
from 97 to **55**, because the member parser runs speculatively and returning an
error from there rejects valid models. The check needs to live where a
declaration is certain, not where one is being considered. Recorded rather than
quietly dropped; the corpus caught it in one run, which is what it is for.

127 tests (1 new).

## 0.35.0 — 2026-10-29

Starting on the leniency gap: enum base types

The gap is that Satya accepts code it should refuse — pssparser's error-suite
has 165 deliberately-broken syntax cases and Satya rejects 66 of them. Working
the list turned up something more urgent first: **one *valid* file was being
rejected**. `enum E : int { A, B }` (12.4) did not parse, because the base type
after `:` was not handled at all.

It is now parsed, and the base is validated: `enum E : struct` is an error
rather than something skipped over. That is the corresponding error-suite case,
which the naive fix would have started accepting.

`error-suite` controls (repaired source, must parse) went **112/113 → 113/113**.

Two things I tried and **reverted**, rather than carry half-working:

- rejecting an `action` at file scope (12.3 says actions are declared inside a
  component). It gained one case and broke three tests, because several of my
  own fixtures declare file-scope actions — so the fixtures and the rule need
  changing together, which is a bigger pass than this session had room for.
- an earlier `select` change, already noted in 0.34.0.

The remaining clusters, measured rather than guessed: `syntax.punct` 21,
`semantic.resolve` 10, `syntax.reserved` 10, `syntax.expr` 9,
`syntax.multifile` 9, `syntax.template` 9, `syntax.volume` 9,
`syntax.scope` 8, `syntax.recover` 7, `syntax.keyword` 6, `syntax.names` 5.
That is a dedicated piece of work, not a patch.

126 tests (1 new). Corpora: 97/97 curated, 7/7 pathological, 114/114 compliance.

## 0.34.0 — 2026-10-28

The three behavioral-coverage defects, fixed

All three were found by writing the workshop examples and left recorded as
known-wrong in 0.33.0. They are now correct.

**`select` did not randomise.** `select { do a1; do a2; }` took `a2` on every
seed — the engine kept the last unguarded branch as a "fallback" and never
considered the others. Every branch whose guard holds is now a candidate and the
choice among them is random (12.3.5); an unguarded branch is the default only
when some branch *is* guarded, which is what makes `select { do a1; do a2; }`
two alternatives and `select { (m==1): A; : D }` a guarded choice with a
fallback. The first attempt looked right but the hash left the choice constant
across consecutive seeds; it now mixes the seed properly.

**`sequence` and `concat` were the wrong way round.** `sequence` required
adjacency, so an `idle` before the pair broke the match, while `concat`
tolerated gaps. 19.3.2 gives `sequence` order-without-overlap and permits other
concurrent traversals between; 19.3.3 makes `concat` adjacent. Both now follow
the standard, and the test that pinned the old behaviour was corrected with the
standard as its justification.

**`schedule` did not apply disjointness.** Its sub-scenario realizations must be
pairwise disjoint (19.3.8). Matching the branches independently let example 6's
`m2` match when it must not. A branch whose greedy realization intersects an
earlier one is now retried from later checkpoints — which is also what makes
`m1` match, by pairing the first sub-scenario's `write1` with the second's
`write2`.

All eleven workshop examples now behave as the deck describes.

125 tests. Corpora: 97/97 curated, 7/7 pathological, 114/114 compliance,
workshop 17/17, all randomisation and coverage checks, all 5 reproducers.

## 0.33.0 — 2026-10-28

The "PSS Comes of Age" workshop examples, and what they expose

All eleven behavioral-coverage examples from the DVCon 2025 workshop (Miller /
Gupta / Ballance) are now in `examples/workshop2025/`, one standalone model
each, in deck order, with a README recording what Satya reports for every one.

Writing them found three defects, two of which were fixed:

- **A `cover <monitor>;` with no covergroup reported nothing.** There was no
  covergroup to sample, so a successful match produced no output at all — the
  deck's first example is exactly that form. Behavioral coverage is now
  reported per monitor: `monitor m1   1 hit(s)`.
- **`select` and `with` in a monitor did not parse.** The monitor `select`
  required a `guard :` on every arm, and `do X with core == 0` was rejected
  because the code tested `at_punct("with")` for a keyword. Both parse now.

Three more are **known wrong and recorded as such** in the README, with
reproductions, rather than left to be discovered:

- `select` does not randomise — `select { do a1; do a2; }` takes `a2` on every
  seed. The activity parser produces one branch, not two.
- `sequence` requires adjacency, so an `idle` before the pair breaks the match.
  19.3.2 says other concurrent traversals do not invalidate it.
- `schedule` matches its sub-scenarios independently, so the pairwise-disjoint
  rule of 19.3.8 is not applied.

Each of those is a semantics change with tests pinning the current behaviour, so
they are the next piece of work rather than a patch made blind.

125 tests (1 new). Corpora unchanged: 97/97 curated, 7/7 pathological, 114/114
compliance, workshop 17/17, all randomisation and coverage checks, all 5
reproducers.

## 0.32.0 — 2026-10-28

`typedef`, `function` and `task` bodies

These were listed as accepted-but-not-executed; checking each against the
binary showed three of the four were simply not implemented.

- **`typedef`** (12.2) was not parsed at all, so `typedef bit[8] byte_t;` left
  `byte_t b;` with no type and no width — an over-wide assignment was not
  masked. Aliases now resolve when used: `b = 300` gives 44.
- **`function` / `task`** declarations were skipped outright, so a package-level
  `function` also swallowed the declarations after it. Bodies are now kept and
  run when called: `y = dbl(21)` gives 42, and a `task` runs for its side
  effects. Parameters bind positionally into a scope restored after the call.
- Parsing a parameter list needed three things the corpus caught:
  `const`/`ref`/`input`/`output` qualifiers, a default value (`int v = 3`), and
  a variadic parameter (`int... args`, which the lexer splits into `..` and
  `.`). Each was a regression against the curated corpus until fixed.

**`symbol` is still not right**, and is left as it was rather than claimed:
a `symbol` produces a step but its body's effects do not appear. That is the
one of the four that was genuinely "accepted but not executed".

Matrix: **114 implemented, 32 partial, 0 not**.

124 tests (3 new).

## 0.31.0 — 2026-10-27

Streams are consumed once

`stream` was not even a parsed declaration keyword — the three sites that
recognise `struct`/`resource`/`buffer`/`state` did not include it, so a stream
declaration was skipped and its type never existed.

With the keyword recognised, a stream now has consume-once semantics (PSS 13):
the object goes to the first input that takes it, and a later consumer of the
same stream is reported rather than silently sharing it:

```
- flow-object ordering: input `in2` of type `data_b` has no unconsumed
  preceding output (PSS 13)
```

A **buffer** keeps the old behaviour — several inputs may read the same object,
which is what the standard allows. Both directions are pinned by a test.

Matrix: **112 implemented, 34 partial, 0 not**.

121 tests (1 new).

## 0.30.0 — 2026-10-27

Monitors work: `cover <monitor>;` matches and samples

Last release I recorded that no model produced coverage from `cover <monitor>;`
and marked entries 68–78 partial rather than claim otherwise. Two real bugs
were behind it:

- **`do <handle>` used the handle *name* as the action.** `a w; ... do w;`
  became `Traverse { action: "w" }`, and no step is named `w`, so the pattern
  matched nothing. It now resolves the handle to its declared type.
- **A scoped handle type was never bound.** `c::a w;` is two tokens that are not
  both plain identifiers, so the handle map stayed empty and `do w` fell back to
  the same broken path. The declaration now parses a full type reference.

Together those meant a monitor could never match, so its covergroups were never
sampled and a constraint on it could never be evaluated — which is why the
previous release could not verify entry 78.

**Monitor constraints (19.4) now filter matches.** A realization whose bound
values violate the monitor's `constraint { }` is discarded, its covergroups are
not sampled, and a warning names the monitor:

```
monitor `m` matched but its constraints do not hold; the match is discarded (PSS 19.4)
```

`examples/coverage/c4_monitor.pss` shows both directions across seeds: a `high`
draw samples the covergroup, a `low` draw is discarded.

Still not modelled: `overlap`/`schedule` branches match independently rather
than interleaved (74, 76), and monitor inheritance is not applied (77). A
monitor declared at *package* level is not collected — it must be declared in
the component that uses it; that is recorded in `docs/SEMANTICS.md`.

Matrix: **111 implemented, 35 partial, 0 not** — 68 and 78 restored to `yes`
now that there is a test that shows them working.

120 tests (2 new).

## 0.29.0 — 2026-10-27

`get_tag`, and an honest correction about monitors

**`get_tag()` returns the region's tag string** (PSS 24.10.8). It used to return
a numeric index because the engine had no string values. A region's tag is now
collected from `reg.tag = "name"` (24.7.3.1), the region that satisfied a claim
is recorded when the claim is allocated, and `get_tag(make_handle_from_claim(…))`
yields the tag as a string.

**Monitor constraints are now parsed and checked** (PSS 19.4): a `constraint { }`
block inside a monitor is captured, evaluated against the matched handles, and a
match whose values violate it is discarded with a warning.

**But I could not verify that**, and the reason matters: no model I construct
produces coverage from `cover <monitor>;` at all — with or without a constraint,
inside a component or a package. Either monitor matching does not fire, or every
model I tried is wrong. `cover { … }` (the block form) works.

So `docs/FEATURES.md` was overclaiming: entries 68–78 were marked implemented on
the strength of the parser and the matcher existing, not on observed behaviour.
68 is now `partial` with that stated plainly, and 78 is `partial` because it
cannot be verified while matching does not fire. This is the same class of
overclaim as `foreach`/`unique` earlier — a construct that parses and is
believed to work, recorded as working without a test that shows it.

Also fixed: allocating a claim after trait matching indexed a per-region cursor
that was sized by the *filtered* candidate list, which panicked as soon as a
trait filter excluded a region. Caught by the existing trait test.

Matrix: **109 implemented, 37 partial, 0 not** — one more implemented, three
corrected downward.

118 tests (1 new).

## 0.28.0 — 2026-10-27

The two remaining rv64_core reproducers: sets and wide domains

Both were the same root cause, and both were reported with a reproducer.

**Set membership returned the first member every time.** `x in [0x40, 0x7F,
0x4000, 0x7FFF]` gave 64 on every seed. The set narrowed the domain correctly,
but selection used rejection sampling — 4 allowed values in a 32704-wide hull —
which never hit a member in 64 tries and then fell back to the *first* allowed
value. A set is now sampled uniformly from its members, and a narrow hull with
gaps is enumerated rather than rejected. All four members are now reachable, and
`tools/repro_check.py` asserts that across 60 seeds.

**Wide value sets exhausted memory.** The same constraint on a `bit[64]` filled
the gaps between members into a `forbidden` set — one entry per value in
between, 2^63 of them, the 1.2 GB allocation that failed. A domain can now carry
its allowed set directly (`allowed: Option<Vec<i64>>`) instead of being
described by its gaps. The wide case solves within 400 MB.

**A set member that does not evaluate is now reported, not dropped.** The
report noted this as "a second, quieter hazard": an unresolvable member yielded
no intervals and the constraint vanished. It is now `not evaluated`, and that
line prints even when nothing at all was countable.

`tools/repro_check.py` runs all five reproducers under `tests/repro/` against a
binary, with an address-space cap so an OOM fails fast instead of swapping.

Note on the repository: the reproducers were committed on top of a v0.25.0
checkout, which reverted the v0.26.0 and v0.27.0 source. This release restores
that source (templates, the `bit[64]` domain, enum ranges, the report split, the
UVM enum declarations) on top of the reproducers, so all five fixes are present
together.

117 tests (3 new).

## 0.27.0 — 2026-10-27

Three defects reported from use

**`bit[64]` literals with bit 63 set solved as 0.** The domain for a `bit[N]`
field was clamped to 62 bits, so `constraint x == 0x8000_0000_0000_0000` had no
representable solution and `x` was clamped to 0 while the constraint was
reported unsatisfied. Values are carried in an i64, so a 64-bit pattern with
bit 63 set *is* negative — the domain for `bit[64]` is now the whole i64 range.
All three patterns (`0x7FFF…`, `0x8000…`, `0xFFFF…`) now solve to their bit
pattern with the constraint satisfied.

Two follow-on bugs this exposed:

- the sampler computed `hi - lo + 1` in i64, which overflows to 0 over the full
  range and **panicked** with a divide by zero — the workshop corpus caught it
  (36/36 → 30/36). The span is computed in i128.
- one RNG draw yields only 47 bits, so a full-width domain never reached its
  upper half. Wide domains now combine two draws.

**Enum range constraints were not enforced.** `constraint sel in [A1..A3]` used
enum *items* as bounds; they were resolved against the constant table only, so
they came out `None`, the constraint became unsupported, and `sel` solved
anywhere in `0..5`. Enum items now resolve during extraction, so the range is
enforced and counted.

**The summary contradicted its detail.** The trace printed
`constraints: 2/2 satisfied` and then listed `not satisfied: <expr>`, because
constraints the solver could not evaluate were filed with the ones it evaluated
and failed — while being excluded from the total. They are now reported
separately as `not evaluated:`, which is what they are.

**The UVM backend named PSS enum items without declaring them.** It emitted
`rand int op;` and `op inside {[PADD:PDIV]};`, which cannot compile. It now
emits a `typedef enum` for each enum the model uses and types the field with it:
`typedef enum { PADD, PSUB, PMUL, PDIV } op_e;` and `rand op_e op;`.

Not verified: no SystemVerilog compiler is available here, so the generated
output is checked by inspection and by asserting on its text, not by compiling
it.

114 tests (4 new).

## 0.26.0 — 2026-10-27

Template instantiation, and a width bug it exposed

A templated type is now instantiated at each use (PSS 12.6): a `type` parameter
substitutes the field's type and a value parameter substitutes into a `bit[W]`
width. `pair_s<bit[16], 4>` and `pair_s<bit[8], 16>` are two distinct types,
each with its own field widths.

`examples/language/templates.pss` shows it: both instantiations are assigned
200, and only the 16-bit field can hold it.

Getting there exposed a **pre-existing bug**, independent of templates: an
assignment was not masked to the target field's width. `bit[4] x; x = 200;`
stored 200, not 8 — an over-wide value silently escaped its field. That affects
any model, and it is now masked at the point of assignment.

The first attempt at mangling also broke seven existing tests, because it put
the width into *every* field's type name (`bit[32]` instead of `bit`), which
changed how domains are computed. The width is now part of a template
*argument* only. The regression suite caught it.

The matrix moves to **109 implemented, 37 partial, 0 not**.

110 tests (2 new).

## 0.25.0 — 2026-10-27

A batch of the partial constructs, closed

Eight more entries moved from *partial* to *yes*: **105 implemented, 41 partial,
0 not**.

- **Casts** (18) now apply the target type's width — `(bit[8])300` is 44,
  `(bit[4])300` is 12, `(bit)5` is 1, `(int)x` is a no-op. The parser also
  accepts a width in the cast (`(bit[8])`), which it previously did not, so such
  casts were parsed as a parenthesised expression and silently produced 0.
- **`format()`** (106) builds a string with `%d`, `%x`, `%s` and `%%`.
- **`compile if`** (103) selects a branch from its constant condition; the
  unselected branch is never instantiated, so traversing it is an unresolved
  action. Before this the whole construct was skipped, taking the declarations
  around it with it.
- **`compile assert`** (105) fails the parse when the condition is false.
- **`compile has`** is accepted as an expression inside a condition. It
  evaluates to *true* — a single-file parse cannot see whether a symbol exists
  elsewhere — and that is recorded in `docs/SEMANTICS.md` rather than left
  implied.
- **Empty aggregate literal** (4), **procedural variable declaration** (90),
  **`yield`** (101) and **`export action`** (102) were already working and are
  now recorded as such.

`examples/language/compile_time_and_casts.pss` covers the new behaviour.

The first attempt broke two corpora — `compile if (compile has (X))` is a
construct inside a condition, and the compliance tier and two workshop files use
it. Re-running the regression caught it before release.

108 tests (4 new).

## 0.24.0 — 2026-10-26

Strings are values, and the matrix has no unimplemented construct left

A `string` had no value model at all: `s = "hello";` was silently ignored, and
the sub-string operator was the last construct in the matrix marked **not
implemented**. String-valued variables are now carried in a store alongside the
numeric ones — the solver is untouched, because a string has no numeric value to
solve for.

Working now:

- string literals and assignment, including assignment from another string;
- concatenation with `+`;
- `length()` / `size()`;
- the sub-string operator in **all four forms** the standard gives it
  (8.6.3), with **inclusive** ends:

  | form | meaning | `"hello"` gives |
  |---|---|---|
  | `s[i]` | one character at i | `s[2]` → `"l"` |
  | `s[i..j]` | from i to j, both inclusive | `s[1..3]` → `"ell"` |
  | `s[i..]` | from i to the end | `s[2..]` → `"llo"` |
  | `s[..j]` | from the start to j | `s[..1]` → `"he"` |

String fields are reported in both the trace and the JSON backend.

`examples/strings/string_ops.pss` exercises all of it.

**The feature matrix now reads 97 implemented, 49 partial, and 0 not.** Every
one of the 146 syntax excerpts has an implementation.

Honest scope limits, recorded in `docs/SEMANTICS.md`: strings are not solved
(a `string` field is not randomised), and `join` / `str_from_chars` — which
produce a string from a collection — are still not evaluated.

104 tests (2 new).

## 0.23.0 — 2026-10-26

Address claims match region traits

Claims were allocated first-fit from the first region with room, ignoring the
trait attributes entirely. That was the one remaining place where the tool's
behaviour quietly **differed** from the standard rather than honestly reporting
a gap, which made it the most misleading thing left in the codebase.

PSS 24.7.3: *"Regions with trait values that satisfy the claim's trait
constraints are the candidate matching regions."* Now:

- a region's trait values are collected from `reg.trait.<attr> = v;` in
  `exec init_up` (24.7.3.2);
- a claim's requirements are read from `claim.trait.<attr> == k` on the action;
- allocation only considers regions whose trait values satisfy every attribute
  the claim constrains;
- an unmatched constraint is reported instead of allocating from the wrong
  region.

`examples/address/trait_matching.pss` has two regions differing only in `core`:
a claim asking for `core == 1` lands at 0x2000 and one asking for `core == 0`
lands at 0x1000. Asking for `core == 7` warns
`no region matches the claim's trait constraints {core=7}`.

Still partial, and now recorded as such: a claim does not span several matching
regions, and regions are modelled from their assignments rather than built
through `add_region` calls.

102 tests (2 new).

## 0.22.0 — 2026-10-26

`arr.sum()` now works inside constraints

`pl.sum() <= 20` was counted but never solved: an aggregate over a collection
cannot be seen by bound propagation, so the constraint was reported unsatisfied
on every seed. The repair pass now moves elements one at a time until the
relation holds — downward for `<=`/`<`, upward for `>=`/`>`, either way for
`==` — choosing the largest (or smallest) element so the reduction spreads
instead of emptying a single slot.

All five relations work: `<= 20` gives 6/7/7/0, `== 40` gives 16/16/8/0,
`< 10` gives 3/3/3/0, `>= 100` and `> 200` both pass.

`examples/randomisation/r5_aggregate.pss` covers a budget (`sum() <= 25` with
per-element bounds), an exact total (`== 40`) and a floor (`>= 100`), and the
checker verifies the arithmetic across 60 seeds each.

Two things this exposed:

- A strict bound landed off by one (`sum < 10` produced exactly 10), because the
  repair moved by the whole remaining delta instead of one; a strict operator
  now targets `k-1` or `k+1`.
- The first version added an unguarded `Atom::General` arm to the repair match,
  which **shadowed** the existing ordering and single-unknown arms — the
  compiler said `unreachable pattern`, and a test caught `x + y == 100`
  breaking. The arm is now guarded on `arr.sum()`.

Also corrected in `docs/SEMANTICS.md`: the array-method row claimed
`product()`, `min()` and `max()` were evaluated. They are not — and they are not
PSS array methods either; `size` and `sum` are the only two the standard
defines.

100 tests (1 new).

## 0.21.0 — 2026-10-26

`unique` constraints now work

`constraint unique { ids }` was not even parsed: `unique` was taken for the
*name* of a constraint block, so the directive became a constraint name and its
body was parsed as an ordinary constraint. It is now an atom of its own.

```
constraint unique { ids };        // all elements distinct
constraint unique { x, y, z };    // named variables distinct
```

Duplicates are moved to a free value in the repair pass — a different shape of
repair from the others, which clamp a value rather than choosing one. An
unsatisfiable set (five distinct values from `[1..4]`) is reported as
`5/6 satisfied` naming the constraint, rather than returning a duplicate
assignment:

```
solved: ids.size=5 ids[0]=2 ids[1]=1 ids[2]=3 ids[3]=4 ids[4]=2
constraints: 5/6 satisfied
  not satisfied: unique {ids[0], ids[1], ids[2], ids[3], ids[4]}
```

`examples/randomisation/r4_unique.pss` covers all three cases, and
`tools/rand_cov_check.py` checks both directions: 60 seeds of distinctness, and
that the impossible case is reported rather than faked.

The feature matrix moves to **94 implemented, 51 partial, 1 not** — the string
slice operator is the only construct left with no implementation.

99 tests (2 new).

## 0.20.0 — 2026-10-26

`foreach` constraints now work, in both forms

`constraint foreach (a[i]) { ... }` was parsed into `Expr::Foreach` and then
fell through to `Unsupported`, which is excluded from the constraint count — so
it was neither applied nor reported. It is now expanded into one constraint per
element at atom extraction, using the element count from the model.

Both spellings in the standard's own examples work:

```
constraint foreach (payload[i]) { payload[i] in [0..15]; }   // index form
constraint foreach (e : payload) { e in [0..15]; }           // bound form
```

The bound form binds the variable to the *element* (`e` stands for
`payload[i]`), the index form to the index; `Expr::Foreach` now carries a
`bound` flag so the solver substitutes the right thing. The DVCon 2025 workshop
uses the bound form, which is how the omission surfaced: the first attempt
handled only the index form and took the workshop from 17/17 to 15/17.

`examples/randomisation/r3_arrays.pss` uses the idiomatic form and reports
`9/9 satisfied` across 60 seeds.

`unique` (entry 59) is **still dropped** — `constraint unique { ids }` yields
duplicates in 36 of 40 seeds while reporting `4/4 satisfied`. It stays `no`.

The feature matrix moves to **93 implemented, 51 partial, 2 not** (the string
slice operator and `unique`).

97 tests (1 new).

## 0.19.0 — 2026-10-26

New randomisation and coverage examples, and four defects they found

Added `examples/randomisation/` (three models) and `examples/coverage/` (three
models), plus `tools/rand_cov_check.py`, which measures the *solutions* against
each model's intent rather than trusting the tool's own report. Results and
numbers in [`docs/RANDOMISATION-COVERAGE.md`](docs/RANDOMISATION-COVERAGE.md).

**`dist` weights are honoured.** Over 200 seeds, chi-square **1.55** against the
declared 1:3:2:2:2 proportions, and `:/` really distributes rather than
assigning.

Four defects found while writing the examples:

- **`cover { ... }` dropped its body.** `cover { do b; }` skipped the traversal
  to the next `;`, so the scenario came out empty — zero steps, no error.
- **A modulo constraint was never enforced.** `addr % 4 == 0` was reported
  unsatisfied on every seed. Over a small domain the disallowed values can
  simply be forbidden, which it now does.
- **`if/else` constraints were dropped entirely** — not applied *and* not
  counted, so the report read `1/1 satisfied` while the branch was
  unconstrained. They now lower onto two implications, and the implication
  repair handles `==`, `<=`, `>=`, `<`, `>` and `in` range/set consequents.
- **A cross never sampled unlabelled coverpoints.** `cross len, m` found nothing
  unless the coverpoints carried a `label`. A coverpoint is now keyed by its
  label or its expression.

The checker also caught a fault in my own example — an if/else branch that
contradicted a READ implication, making one draw in sixty unsatisfiable.

**Corrected an overclaim in `docs/FEATURES.md`:** `foreach` and `unique`
constraints were marked *partial* because the parser accepted them. They are
parsed and then **dropped** — not enforced and not counted. Both are now `no`,
which moves the matrix to 92 implemented, 51 partial, 3 not.

96 tests (5 new).

## 0.18.0 — 2026-10-26

andestech/pss-gen: a crash on open ranges

[`andestech/pss-gen`](https://github.com/andestech/pss-gen) has 145 `.pss`
sample inputs. **145/145 now parse**, and it found a crash.

`samples/7_2_Integer_types` uses open-ended ranges inside a comma list — valid
PSS 8.5.9:

```
rand int [64] in [..1 , 8..16 , 4294967296..] long_5;
```

The parser handled a lone open range but not one inside a list, so this failed
to parse. Once parsing was fixed the solver **panicked** with `capacity
overflow`: the span of `[..1]` is `1 - i64::MIN`, which overflows `i64`, and the
guard meant to keep the expansion small computed exactly that difference.

Set membership is now handled with intervals instead of materialised value
lists: the span is computed in `i128`, membership is an interval test, and
propagation narrows to a satisfying interval rather than enumerating a union
that may be unbounded.

Also recorded: `x == -1` yields `0`, because the engine has no negative `int`
values at all (domain `[0, 1023]`, clamped at zero). Pre-existing, independent
of this fix, and surfaced by this repository.

91 tests (2 new).

## 0.17.0 — 2026-10-26

Checked against pssparser's error-suite: Satya is too lenient

`psstools/pssparser` ships `error-suite/`, 190 graded cases of deliberately
broken and deliberately valid PSS, each with a `//!` header saying what a tool
should do with it. `tools/pssparser_errsuite_check.py` runs it.

```
  accept    expect=accept      2 correct    0 wrong
  syntax    expect=error      64 correct  101 wrong
  semantic  expect=error       0 correct   19 wrong
  semantic  expect=warning     1 correct    3 wrong
  TOTAL                       67 correct  123 wrong   (35%)
  controls (repaired source, must parse): 112/113
```

The two directions mean different things. **Satya accepts valid PSS** — 112 of
113 repaired controls parse and both `accept/` cases pass. **But it is too
lenient on malformed syntax**: it rejects only 64 of 165 deliberately-broken
cases. `x + ;`, `== x;`, `x == 5 &&;` and `x = ;` in an `exec` body are all
accepted. That is a larger gap than the shared corpus exposed, and it is now
recorded rather than hidden.

Fixed: a stray `}` at the end of a file was silently ignored; it is now an
error (2 of the 165).

Tried and reverted: making the `exec` statement parser strict. It rejected two
valid compliance files (`|=` compound assignment, `break` inside a nested block)
and still did not catch `x = ;`, because that parser consumes tokens before
giving up. A parser cannot be strict before it can parse the valid language.

The suite's own harness could not be used as-is: `pss_errsuite/cli.py` imports a
`compare` module absent from the published snapshot. The tool here honours the
same conventions, taken from the suite's own `case.py`.

89 tests (1 new).

## 0.16.0 — 2026-10-26

A file of foreign syntax is no longer reported as a success

Running Satya against `kariannekk/Portable-Stimulus` — a master's thesis
appendix built with Questa **inFact**, containing no `.pss` at all but 14
InFact `.rules`/`.rseg` files — turned up a real defect.

Those 14 files all **"parsed successfully"**, into a model with 0 packages, 0
types, 0 components and 0 actions. The top-level parser skipped any construct it
did not recognize up to the next `;`, so a file of entirely foreign syntax
produced a clean success and an empty model. A user who mistyped a top-level
keyword would have got the same thing.

An unrecognized construct at the **file** level is now an error:

```
error: unexpected `rule_graph` at top level; expected a package, component,
       action, struct, enum, import or extend declaration
```

The strictness is scoped to the file level deliberately. Making it global
dropped the curated corpus buckets from 97 to 91 and the LRM strict count from
129 to 90, because `enum`, `monitor`, `covergroup`, `stream` and `annotation`
bodies are reached through the same fallback. Inside a body the lenient skip is
kept: a construct this parser does not yet handle there should not make a whole
model unloadable.

88 tests (2 new).

## 0.15.0 — 2026-10-25

The last of the constructs, and the corpus fully met

The feature matrix closes at **92 implemented, 53 partial, 1 not**: the only
construct with no implementation is the string slice operator, which needs a
string value model the engine does not have.

Finished here:

- **`symbol`** (12.7) — a named activity fragment, expanded in place where it is
  called. It was parsed and then silently produced *no* steps.
- **`override`** (20.5) — `type A with B` and `instance path with B`, applied
  when an action is traversed.
- **Address claims** (24.8) — a claim struct instance is allocated first-fit
  from an address region, so the claimed address is visible to the action.
- **`read_bytes`** (24.10.9.3) and **`read_struct` / `write_struct`**
  (24.10.9.4) — a packed struct is laid out little-endian over memory and read
  back; a byte series fills a list.
- **Target-code `{{ }}` templates** (22.2) — an opening `{{` with no matching
  `}}` in a triple-quoted block is now an error.

That closes the shared corpus completely:

```
  buckets that must parse: 97 passed, 0 failed
  pathological             7 files, 7 correctly rejected, 0 wrongly accepted
  compliance             114 files, 114 parsed, 0 failed
```

86 tests (5 new).

## 0.14.0 — 2026-10-25

The rest of the psstools organization

After the shared corpus, the same check was run against every other
[`psstools`](https://github.com/psstools) repository containing `.pss` files —
356 files in total. **341 parse**, and the 15 that do not are the 6 pathological
files that must fail, 7 Jinja2 generator templates (not PSS), and 4
syntax-highlighter snippets that are deliberately minimal. The corpus's
executable tier — 114 files of instrumented **PSS 3.1** — parses **114/114**,
and the 26 `peakrdl-pss` golden files run through every backend, **156/156
generations**. Details in [`docs/CORPUS.md`](docs/CORPUS.md).

Three more defects, none of which the corpus alone exposed:

- **64-bit literals overflowed.** `0xFFFF_FFFF_FFFF_FFFF` and its decimal form
  `18446744073709551615` are valid 64-bit values that do not fit a signed 64-bit
  integer, and both failed to lex. Found by `pssc`'s bug-repro directory, whose
  README notes other tools accept the input. Literals are now parsed unsigned
  and keep their bit pattern.
- **Nested template arguments.** `nested_s<packed_s<bit, 8>>` failed because the
  constant argument was parsed at a precedence that still admitted `>>` as a
  *shift*, swallowing the closing token. Found by `pssfmt`.
- **Scheduling constraints swallowed a brace.** `constraint parallel { a, b };`
  (PSS 16.2, Syntax 62) was unparsed, and the enclosing `}` went with it. Found
  by `sphinx-pss`'s activity fixtures.

`tools/pss_corpus_check.py` now covers the compliance tier as well as the
manifest-governed buckets.

81 tests (3 new).

## 0.13.0 — 2026-10-25

Checked against the shared PSS corpus

Satya was run against [`psstools/pss-corpus`](https://github.com/psstools/pss-corpus),
whose `manifest.toml` states which buckets must parse and which must not — an
expectation set outside this repository. The first run passed **85/97** and
wrongly accepted **3 of 7** pathological files. Nine defects came out of it; see
[`docs/CORPUS.md`](docs/CORPUS.md).

The most serious was not a parse gap but a **silent correctness bug**: sized
literals lexed as two tokens and the value was discarded, so `4'b1010`
evaluated to 4, `8'hFF` to 8 and `64'd2` to 64. Every sized literal in every
model was wrong, and it parsed cleanly, so nothing ever reported it. The 0.3.0
notes had claimed this was fixed; they were wrong.

Also fixed: floating-point literals, signed based literals (`4'sd12`), escaped
identifiers (`\name`), the `**` operator (which was silently *dropping* the
constraints it appeared in), `open_range_list` values mixed with ranges (the
parser accepted them, the solver ignored the ranges), qualified constants as
array extents and pool counts, imported function declarations with `type` and
language qualifiers, nested package names, activity `repeat { } while`, and
monitor statements without braces. Parse errors now carry a line and column —
they previously did not, which made all of the above harder to find than it
should have been.

**Current: 97/97 of the buckets that must parse, 6/7 pathological correctly
rejected.** The one remaining is an unclosed `{{ }}` template in target code,
which Satya treats as opaque text.

Note on the LRM figures: the raw count falls from 157 to 154 of 160, because
three extracted "examples" are LRM *prose* containing apostrophes that the
looser lexer silently skipped. The stricter lexer is correct; the old number was
inflated by leniency.

78 tests (5 new).

## 0.12.0 — 2026-10-23

Verifying a list of claimed features, and correcting one

Asked whether a specific list of constructs was complete, each was re-tested
against the implementation rather than answered from memory. That turned up two
things worth fixing:

- **`solve … before …` is not a PSS construct.** It is a SystemVerilog
  constraint directive; PSS 3.0 has no such thing, and its 16.1.8 is *forall
  constraints*. Satya still accepts it, but it is now recorded as an extension
  beyond the standard rather than conformance (`docs/SEMANTICS.md` §3), and the
  wrong clause citation in the previous entry is corrected.
- **`exec pre_body` was not executed.** It is a solve-platform block, evaluated
  after `post_solve` and before the body is realized. Now run in that position.
  (`exec body` is deliberately *not* run at solve time — it is target code, and
  is emitted into the generated C and SystemVerilog.)

Everything else on the list checked out, and is now pinned by tests: `replicate`
against `repeat`, the `dist` weights and defaults, flow-object ordering, and
coverage.

## 0.11.0 — 2026-10-22

Address handles and the memory model

- `addr_handle_t` handles (24.10.3), with `make_handle_from_claim` and
  `make_handle_from_handle` offsetting one.
- `addr_value` (24.10.5), `addr_value_solve` (24.10.6) and `addr_value_abs`
  (24.10.7).
- Primitive read/write over a byte-addressable store (24.10.9.1, 24.10.9.2):
  `read8`/`read16`/`read32`/`read64` and their writes, little-endian — the first
  byte written goes into bits [7:0].

Feature matrix: 88 implemented / 43 partial / 15 not.

## 0.10.0 — 2026-10-22

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.10.0) · registers and `sizeof_s`

- `reg_c<R, ACC, SZ>` register declarations (24.11.1), with the register-value
  type, access kind and size taken from the base's template arguments.
- Register access functions over a register store: `read`, `read_val`, `write`,
  `write_val`, `write_masked`, `write_val_masked`, `write_field`, `write_fields`.
  `write_masked` is the standard's formula exactly.
- Register fields pack little-endian from the least-significant bit in
  declaration order (24.10.1).
- `sizeof_s<T>::nbits` / `::nbytes` (24.10.2) for packed structs, bit vectors,
  enums and scalars.

Feature matrix: 80 implemented / 40 partial / 26 not.

## 0.9.0 — 2026-10-22

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.9.0) · monitors

- Monitor declarations, with their action handles and covergroups.
- Monitor activity patterns: action traversal, `sequence`, `concat`,
  `eventually`, `overlap`, `schedule`, `select`, nested monitor traversal and
  constraints.
- `cover <monitor_type>;` matches the pattern against the executed stream and
  samples the monitor's covergroups over the steps that realize it.

The statements are distinguished, not aliased: `sequence` requires contiguity
while `concat` allows gaps, and a `concat` realization excludes the gap steps.

Feature matrix: 78 implemented / 38 partial / 30 not.

## 0.8.0 — 2026-10-22

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.8.0) · procedural `exec`, `float_pkg`, standard library, feature matrix

- `docs/FEATURES.md`: all **146** syntax excerpts mapped to implemented /
  partial / not implemented — the denominator that "implement every feature"
  needs. Checking the statuses against the code caught two constructs marked
  implemented that were not.
- Procedural statements in `exec` (89–101): `repeat`, `repeat…while`, `foreach`,
  `if`, `match`, `break`, `continue`, `return`, `randomize`, `yield`, with real
  control flow and translations for the C and SystemVerilog backends.
- `float_pkg` (112–116): `float32`/`float64` plus `float_mantissa`,
  `float_exponent`, `float_sign`, `to_float` and the math functions, over the
  IEEE-754 bit pattern.
- The standard library (106–111): `print`/`message`/`error`/`fatal` recorded and
  reported; `urandom()`/`urandom_range()` varying by seed.
- Value list literals `[1, 2, 3]` and `[a, b]` (Syntax 5), which did not parse
  before.

## 0.7.0 — 2026-10-22

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.7.0) · closing the semantics list

- **Correction:** the previous gap list named `sort`, `reverse`, `unique`, `and`,
  `or`, `xor` as unimplemented *array* methods. The standard defines no such
  methods for arrays (7.9.2.2). The list is corrected, and the methods Satya had
  invented for arrays (`product`, `min`, `max`) are removed.
- Array operators (7.9.2.1): element-wise `==` / `!=` and `in` membership, solved
  rather than merely parsed.
- Deprecated array properties `arr.size` / `arr.sum` (7.9.2.4).
- Collection mutators in `exec` (7.9.3.2), collection literal assignment, and
  `x in <collection>` as a solved constraint, uniform over the elements.
- `exec init_down` / `init_up` (22.1), run for the component hierarchy before the
  scenario.
- `cross` sampling (18.4) and coverage against declared `bins` (18.3).

First release whose binary was built and attached by CI rather than by hand.

## 0.6.0 — 2026-10-22

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.6.0) · the remaining semantics

- Scheduling (12.3.6): `join_none`, `join_first`, `join_branch`, `join_select`
  decide what the statement after the block waits for; statements carry labels.
- `replicate` (12.5), distinct from `repeat`.
- `dist` (16.1.9): `:=` and `:/` weights — measured 10.2/9.7/10.2/20.7/49.3 %
  against the specified 10/10/10/20/50.
- `solve … before` honoured in assignment order. **Correction (0.12.0):** this
  is a SystemVerilog directive, not a PSS one — see `docs/SEMANTICS.md` §3.
- `exec pre_solve` / `post_solve` (16.4.12); non-`rand` attributes are no longer
  randomised, which is what makes a `pre_solve` assignment stick.
- Flow-object ordering (13) enforced.
- Arrays (8.4): `array<T, N>` and `x[N]` expand into per-element variables, with
  indexing, `size`/`sum` and `foreach`. OpenTitan's AES multireg is modelled
  directly rather than hand-unrolled.
- Coverage (18, 19.2): covergroups, coverpoints and `cover` blocks sampled.

## 0.5.0 — 2026-10-22

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.5.0) · semantic conformance

- Default-constraint rules (16.1.10.1): one default per attribute by precedence,
  `default disable`, contradictions reported.
- Contradictory models report rather than panic.
- Ordering between two random variables (`a < b`) solved by bound propagation.

## 0.4.0 — 2026-10-22

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.4.0) · OpenTitan AES

- `examples/opentitan/ot_aes.pss`, written from the AES IP's register
  specification.
- Defects it exposed: part-selects `identifier[msb:lsb]` (8.6.1) were not parsed;
  bit-wise `~` could not be solved; generated C emitted typedefs alphabetically
  so nested structs did not compile.

## 0.3.1 — 2026-10-21

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.3.1) · OpenTitan UART

- `examples/opentitan/ot_uart.pss` plus `tools/ot_regmap_check.py`, which diffs
  the model against the IP's HJSON: 6/6 registers match.
- Defects it exposed: inline ranges on struct fields were dropped; implications
  were checked but never propagated.

## 0.3.0 — 2026-10-21

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.3.0) · LRM conformance

- The standard's own examples extracted into `examples/lrm/` (197 files) with a
  harness: 157/160 model examples parse.
- 12 defects fixed, including sized literals `4'b1010`, `'{…}` aggregates,
  constant template arguments, `ref` types, open-ended ranges and qualified
  `extend action`.

## 0.2.0 — 2026-10-21

[Notes](https://github.com/thejas235689/satya/releases/tag/v0.2.0) · initial release

- PSS → parse → solve → schedule → six backends (UVM/SystemVerilog, native C,
  firmware reference, post-silicon bench, JSON, trace).
- Dependency-free: the build needs only the Rust standard library.

---

## A note on coverage claims

The project's coverage is stated in two places, both kept honest:

- [`docs/FEATURES.md`](docs/FEATURES.md) — every construct, marked.
- [`docs/SEMANTICS.md`](docs/SEMANTICS.md) §2 — the constructs that are parsed
  but only approximated, listed explicitly rather than left implied.

Where a release states a figure (tests, examples parsed, registers matching), it
was measured rather than estimated. Where something is not implemented, it is
named.
