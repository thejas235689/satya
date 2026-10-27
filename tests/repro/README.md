# Reproducers for four issues found from rv64_core

Found by running `satya v0.25.0` (prebuilt x86_64 release) against a real model
(the numeric-ABI PSS in `thejas235689/rv64_core`, `pss/rv64_numeric.pss`).

Each file below is standalone and needs no build step:

```sh
satya solve tests/repro/bug1_bit63_literal.pss --action test
```

## bug1_bit63_literal.pss — bit[64] literals with bit 63 set solve as 0

```
constraint x == 0x7FFFFFFFFFFFFFFF;   -> x=9223372036854775807   correct
constraint x == 0x8000000000000000;   -> x=0                     violates the constraint
constraint x == 0xFFFFFFFFFFFFFFFE;   -> x=0                     violates the constraint
```

The boundary is exactly bit 63. `src/lexer.rs` stores literals as `Int(i64)` and
casts with `u64 as i64` (lines 332, 367, 450), so any literal >= 2^63 arrives
negative; the domain then rejects it and the assignment falls back to 0. Either
the token needs to carry `u64`/`u128`, or unsigned `bit[N]` variables need their
domain built from the raw bit pattern.

## bug2_enum_range.pss — enum range constraints are not enforced

```
rand e_t sel;  constraint sel in [A1..A3];   (i.e. 1..3)
seeds 1..5 -> sel = 3, 0, 5, 5, 0
```

Three of five seeds return a value outside the constraint. Note the reporting
half of this too: two steps in a larger model printed `not satisfied` while
holding exactly the pinned values, so the solution was right and the report was
wrong -- the summary and the detail disagree in both directions. The trace makes this
visible but also contradicts itself: it prints `constraints: 2/2 satisfied`
while listing `not satisfied: <expr>`.

I could not pin the cause without a compiler. What I ruled out by reading:
`set_item_intervals` builds the interval, `Atom::InRange` reaches `tighten`,
`tighten` calls `intersect_interval`, and `Env::lookup` does resolve enum
variants (so `A1` evaluates to 1). The loss is therefore after atom extraction --
in how the tightened domain reaches the selection step, or in the satisfaction
reporting -- which needs a debugger rather than reading.

Note also that `set_item_intervals` returns an *empty* vector when either bound
fails to evaluate, which silently drops a constraint. That is a second, quieter
hazard: an unresolvable bound should be an error, not a no-op.

## bug5_set_membership.pss — set membership always returns the first element

```
rand int x;  constraint x in [0x40, 0x7F, 0x4000, 0x7FFF];
seeds 1..5 -> x = 64 every time
```

The same holds for `bit[64]`. A **range** (`x in [64..32767]`) randomises
correctly across seeds, so a value list is not randomisation at all -- it is a
constant. Anything relying on a set for coverage is silently getting one value.

## bug3_wide_domains_oom.pss — wide operand domains exhaust memory

Three actions from the real model fail with
`memory allocation of 1207959568 bytes failed` while narrower ones solve:

| action | operand domain | result |
| --- | --- | --- |
| `quire_op` | none (enum only) | solves |
| `posit_op` | values <= 0x7FFF | solves |
| `rns_op` | values <= 0xFFF | solves |
| `fixed_q_op` | values up to 2^63-1 | 1.2 GB allocation, fails |
| `lns_op` | values up to 0x7FFFFFFF | fails |
| `fp32_op` | values up to 0x7F7FFFFF | fails |

The 1.2 GB figure is identical in all three failures, which suggests a fixed
allocation sized by something other than the actual domain width.

## bug4_uvm_enum_names.pss — the UVM backend emits PSS enum names as bare SV

`satya gen --backend uvm` turns

```
constraint op in [PADD..PDIV];
```

into

```systemverilog
constraint posit_op_c {
  op inside {[PADD:PDIV]};
```

`PADD`/`PDIV` are PSS enum variants; in SystemVerilog they are undeclared
identifiers. The generated sequence needs an enum declaration emitted alongside
it, or the bounds inlined as integers.
