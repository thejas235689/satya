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
