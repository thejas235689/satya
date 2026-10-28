# Workshop examples — "PSS Comes of Age" (DVCon U.S. 2025)

Hillel Miller (Synopsys), Prabhat Gupta (AMD), Matthew Ballance (AMD).
The workshop's runtime behavioral coverage section builds up a set of examples;
each is reproduced here as a standalone model, in deck order.

Sources: the Accellera video page, and the workshop deck in the DVCon
proceedings (`PSS-Behavioral-Coverage-DVCON-2025.pdf`, `SW_4B_1_PSS_Accellera_WG.pdf`).

## What Satya reports for each

Run `satya solve <file> --seed 1`. "Hit" is the behavioral coverage line.

| # | file | parses | monitor hit | note |
|---|---|---|---|---|
| 1 | `w1_basic_monitor.pss` | yes | `m1` | matches; the deck's realization discussion is not modelled |
| 2 | `w2_three_monitors.pss` | yes | `m3` | `m1` correctly does not match (the `select` chose `send`) |
| 3 | `w3_sequence.pss` | yes | — | **wrong**: `sequence` requires adjacency, so `idle` before the pair breaks it. The deck says other traversals do not invalidate |
| 4 | `w4_concat_vs_sequence.pss` | yes | `m_concat` | `sequence` misses for the same reason as #3 |
| 5 | `w5_inline_constraints.pss` | yes | — | the `with core == 0` condition is parsed but no traversal satisfies it; the model needs `core` constrained |
| 6 | `w6_schedule.pss` | yes | — | **wrong**: `schedule` sub-scenarios are matched independently, so the disjointness rule is not applied. The deck's `m2` must not match, and Satya agrees only by accident |
| 7 | `w7_overlap.pss` | yes | `m` | matches, but without the "simultaneously active" condition |
| 8 | `w8_select.pss` | yes | `c1_mon`, `c2_mon` | |
| 9 | `w9_hierarchical_monitors.pss` | yes | `irw` | nested monitors (`irw1`, `irw2`) are not traversed |
| 10 | `w10_covergroups_in_monitors.pss` | yes | `m` | covergroup sampled through the monitor |
| 11 | `w11_reactive_flow.pss` | yes | — | `select` always resolves to the last branch, so the high path is never taken |

## Known wrong, with the evidence

These three are behaviours Satya gets wrong, found by writing these examples.
They are recorded rather than hidden, because each one silently reports a
result that looks plausible:

- **`select` does not randomise** (affects #11, and any model using it).
  `select { do a1; do a2; }` takes `a2` on every seed:
  ```
  component c { action a1 {} action a2 {} action t { activity { select { do a1; do a2; } } }; };
  seeds 1..6 -> a2, a2, a2, a2, a2, a2
  ```
- **`sequence` requires adjacency** (#3, #4). PSS 19.3.2 says the sub-scenarios
  must be in order and must not overlap, but other concurrent traversals do not
  invalidate the match.
- **`schedule` does not apply disjointness** (#6). Its sub-scenario realizations
  must be pairwise disjoint; matching them independently lets a match through
  that the standard forbids.
