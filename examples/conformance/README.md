# Conformance examples

Eleven focused models, each targeting one construct, plus a checker that asserts
the *behaviour* the clause requires rather than that the file parses.

Run them with:

```sh
python3 tools/conformance_check.py ./satya
```

## Data types

| file | what it exercises |
|---|---|
| `dt1_scalars.pss` | `bit[8]`/`[16]`/`[32]`/`[64]`, `int`, `bit`, `bool`, width-bounded constraints |
| `dt2_aggregates.pss` | `typedef`, an enum with a base type, a packed struct, a fixed array of 4, a struct field referencing an enum |
| `dt3_strings_templates.pss` | a `string` field and a two-parameter templated struct used at two widths |

## Constraints

| file | what it exercises |
|---|---|
| `cn1_ranges_sets.pss` | a closed range, a value set, and an `open_range_list` mixing values and ranges including an open-ended one |
| `cn2_dist.pss` | `dist` with weights 1:3:2 and two implications on the same condition |
| `cn3_foreach_unique.pss` | `foreach` over an array together with `unique` |
| `cn4_aggregate.pss` | `arr.sum()` and an `if`/`else` constraint |
| `cn5_soft_inline.pss` | a `soft` constraint a hard one may override |

## Coverage

| file | what it exercises |
|---|---|
| `co1_bins.pss` | value, range and set bins, and a labelled coverpoint |
| `co2_cross.pss` | a cross of two coverpoints |
| `co3_monitor.pss` | a monitor sampling its own covergroup over a matched sequence |

## Results

14 checks, **13 pass, 1 fails**:

```
dt1  u8/u16/b1 stay inside their widths                     ok
dt2  f.kind in {RD,WR} and f.n in [0..4]                    ok
dt3  string field holds its value                           ok
dt3  pair_s<_,4> masks to 4 bits (200 -> 8)                 ok
dt3  pair_s<_,16> holds 200                                 ok
cn1  x/y/z stay inside their constraints                    ok
cn2  all three dist values reachable                        ok
cn2  dist weights roughly 1:3:2                             ok
cn2  implication holds on every seed                        ok
cn3  four distinct values, each in [1..4]                   ok
cn4  sum() and if/else both hold                            ok
cn5  lo < hi on every seed                                  ok
cn5  soft lo == 0 honoured when it can be                   FAIL  0/20
co1  bins are reported                                      ok
co2  all eight size x dir combinations reached              ok
co3  the monitor matches and reports a hit                  ok
co3  its covergroup is sampled through the match            ok
```

## The one failure

**`soft` constraints are not implemented.** `constraint soft lo == 0;` is
dropped: `soft` appears nowhere in the parser, the IR or the solver, so the
constraint never reaches the atom list and is not counted in
`constraints: n/n` either. On 20 seeds `lo` is never 0:

```
seed 1 -> hi=74 lo=73   constraints: 2/2
seed 2 -> hi=77 lo=52   constraints: 2/2
seed 3 -> hi=34 lo=33   constraints: 2/2
```

Two of three constraints are reported satisfied because only two were counted.
That is the same failure mode as `foreach` and `unique` earlier in this project:
a construct that parses, is silently dropped, and leaves a summary that looks
healthy.

A correct implementation needs a preference, not a check: solve with the soft
constraints, and if the result does not satisfy them, solve again without. That
is a change to the solve entry point rather than a rule, which is why it is
recorded here rather than patched in passing.
