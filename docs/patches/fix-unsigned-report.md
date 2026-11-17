# Fix: unsigned rendering of 64-bit values (proof row 9)
Satya c92d8d2 (v0.75.0). Values are carried as `i64`; the declared type now decides how they
print and how constraints compare. Touches `src/ir.rs`, `src/solver.rs`, `src/backend/json.rs`,
`src/backend/trace.rs` only — parser/engine untouched.

## Row 9 before / after
- before: `FAIL 9 Integer type declaration — bad64=-1 out of [0..18446744073709551615] at seed 1`
- after: `PASS 9 Integer type declaration`; json `"bad64": -1` → `18446744073709551615`, trace
  `bad64=-1` → `bad64=18446744073709551615` (value was already `0xFFFF_FFFF_FFFF_FFFF`, 1/1 — rendering only)

## Mechanism
`Field::signature` (ir.rs) derives width+signedness from the declared type (`bit[N]`, `bool`, `byte`,
`uint`, typedefs followed; `int`/`longint`/enums signed); `Model::field_signatures` maps each action's
leaf paths to it and both backends print `as u64` when unsigned. The solver carries the flag in `Domain`
and compares unsigned (`within`, `cmp_vals`, `intersect_unsigned`), so `in [0..18446744073709551615]` is
the whole range rather than `[0..-1]` — now 1/1 satisfied.

## Signed rendering still correct
`int i32=-5`, `shortint i16v=-32768`, `longint i64v=-9000000000` in json and trace; slice-1 row 8
(`int i32; i32=-5`) still passes. Unsigned side: `bit[64]`=18446744073709551615,
`typedef bit[64] u64_t`=18446744073709551614, `uint` unchanged (4294967295); `uint64` renders
18446744073709551615 but is an undeclared type for the solver, so it is not randomised.

## Regressions
- `run_all.sh`: 128 pass / 34 fail → **129 pass / 33 fail**, row 9 the only change.
- `cargo test`: **178 passed / 0 failed** before and after (19 unit + 159 integration; the stated 173
  baseline does not reproduce on this commit).
- Diffed against the pre-fix binary: unconstrained `bit[64]`, `bit[8] in [256..300]`, `uint in [-100..-10]`,
  contradictory uint ranges and packed-struct `cfg.mode == 5` are all identical.
