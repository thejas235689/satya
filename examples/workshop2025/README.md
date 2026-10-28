# Workshop examples — "PSS Comes of Age" (DVCon U.S. 2025)

Hillel Miller (Synopsys), Prabhat Gupta (AMD), Matthew Ballance (AMD).
The workshop's runtime behavioral coverage section builds up a set of examples;
each is reproduced here as a standalone model, in deck order.

Sources: the Accellera video page, and the workshop deck in the DVCon
proceedings (`PSS-Behavioral-Coverage-DVCON-2025.pdf`, `SW_4B_1_PSS_Accellera_WG.pdf`).

## What Satya reports for each

Run `satya solve <file> --seed 1`. "Hit" is the behavioral coverage line.

| # | file | monitor hit at seed 1 | note |
|---|---|---|---|
| 1 | `w1_basic_monitor.pss` | `m1` | matches |
| 2 | `w2_three_monitors.pss` | `m1`, `m3` | `select` chose `read`, so both match |
| 3 | `w3_sequence.pss` | `m1` | `idle` before the pair does not invalidate it (19.3.2) |
| 4 | `w4_concat_vs_sequence.pss` | `m_sequence` | `concat` correctly misses: the `idle` is a gap (19.3.3) |
| 5 | `w5_inline_constraints.pss` | — at seed 1 | matches only when the write's `core` is 0 (seeds 3, 4) |
| 6 | `w6_schedule.pss` | `m1` | `m2` correctly misses: its realizations intersect (19.3.8) |
| 7 | `w7_overlap.pss` | `m` | matches |
| 8 | `w8_select.pss` | `c1_mon`, `c2_mon` | both match |
| 9 | `w9_hierarchical_monitors.pss` | `irw` | nested monitors (`irw1`, `irw2`) are not traversed |
| 10 | `w10_covergroups_in_monitors.pss` | `m` | covergroup sampled through the monitor |
| 11 | `w11_reactive_flow.pss` | `high_path_mon` | the `select` is random, so some seeds take the low path and miss |

## Fixed by writing these

Three behaviours were wrong when these examples were first written. All three
are fixed, and the tests that pinned the old behaviour were corrected with the
standard as the justification:

- **`select` did not randomise.** `select { do a1; do a2; }` took `a2` on every
  seed: the engine kept only the last unguarded branch as a "fallback". Every
  branch whose guard holds is now a candidate and the choice is random
  (12.3.5); an unguarded branch is the default only when some branch is
  guarded.
- **`sequence` and `concat` were the wrong way round.** `sequence` required
  adjacency, so `idle` broke `write, read`, while `concat` tolerated gaps.
  19.3.2 gives `sequence` order-without-overlap and permits other concurrent
  traversals between; 19.3.3 makes `concat` adjacent. Both now follow the
  standard.
- **`schedule` did not apply disjointness.** Its sub-scenario realizations must
  be pairwise disjoint (19.3.8); matching branches independently let `m2` match
  in example 6 when it must not. A branch whose greedy realization intersects
  an earlier one is now retried from later checkpoints.

## Still not modelled

Nested monitor traversal (example 9's `irw1` and `irw2`), and `overlap`'s
"simultaneously active" condition (example 7), which in a linear step list
degenerates to schedule's disjointness.
