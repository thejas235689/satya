# Feature matrix

The standard defines **146 syntax excerpts** — one per language construct. This
document maps every one of them to its status in Satya, so the project's
coverage can be read honestly rather than inferred from the test count.

Legend: **yes** — parsed *and* implemented with its defined semantics;
**partial** — parsed, and some semantics implemented, but not all;
**no** — not implemented (in most cases not yet parsed either).

Every "yes" is exercised by a test or by the corpora in `docs/LRM-EXAMPLES.md`
and `docs/OPENTITAN.md`. The statuses were checked against the implementation
rather than assumed — which has twice caught constructs marked implemented that
were not.

## Summary

| Status | Count |
|---|---|
| yes | 88 |
| partial | 43 |
| no | 15 |
| **total** | **146** |

## Every construct

| # | Syntax excerpt | Implemented | Notes |
|---|---|---|---|
| 1 | Numeric constants | yes |  |
| 2 | String literals | partial | lexed; strings are not values in the engine |
| 3 | Aggregate literals | yes |  |
| 4 | Empty aggregate literal | partial |  |
| 5 | Value list literal | yes |  |
| 6 | Map literal | partial | parsed; maps are not solved |
| 7 | Structure literal | yes |  |
| 8 | Data types and data declarations | yes |  |
| 9 | Integer type declaration | yes |  |
| 10 | Floating-point type declaration | yes |  |
| 11 | enum declaration | yes |  |
| 12 | string declaration | partial | parsed; no string value model |
| 13 | chandle declaration | partial | parsed; opaque handle |
| 14 | struct declaration | yes |  |
| 15 | Collection data types | yes |  |
| 16 | ref declaration | partial | parsed; reference semantics not modelled |
| 17 | User-defined type declaration | yes |  |
| 18 | cast operation | partial | parsed; casts are not applied to values |
| 19 | Expressions and operators | yes |  |
| 20 | Conditional operator | yes |  |
| 21 | Set membership operator | yes |  |
| 22 | String slice operator | no | needs a string value model |
| 23 | component declaration | yes |  |
| 24 | action declaration | yes |  |
| 25 | Template type declaration | partial | parsed; template parameters are not instantiated |
| 26 | Template value parameter declaration | partial |  |
| 27 | Template type parameter declaration | partial |  |
| 28 | Template type instantiation | partial |  |
| 29 | activity statement | yes |  |
| 30 | Action traversal statement | yes |  |
| 31 | Activity sequence block | yes |  |
| 32 | Parallel statement | yes |  |
| 33 | Schedule statement | yes |  |
| 34 | Activity join specification | yes |  |
| 35 | Atomic block | yes |  |
| 36 | repeat-count statement | yes |  |
| 37 | repeat-while statement | yes |  |
| 38 | foreach statement | yes |  |
| 39 | select statement | yes |  |
| 40 | if-else statement | yes |  |
| 41 | match statement | yes |  |
| 42 | replicate statement | yes |  |
| 43 | symbol declaration | no |  |
| 44 | bind statement | yes |  |
| 45 | buffer declaration | yes |  |
| 46 | stream declaration | partial | parsed; stream consume-once is not enforced |
| 47 | state declaration | partial | parsed; state objects are not persisted |
| 48 | Flow object reference | yes |  |
| 49 | resource declaration | yes |  |
| 50 | Resource object reference | yes |  |
| 51 | Pool instantiation | yes |  |
| 52 | Static bind directives | yes |  |
| 53 | Member constraint declaration | yes |  |
| 54 | Expression constraint | yes |  |
| 55 | Implication constraint | yes |  |
| 56 | Conditional constraint | yes |  |
| 57 | foreach constraint | partial | foreach over a collection is expanded; not over a random index |
| 58 | forall constraint | partial | parsed; forall is not expanded |
| 59 | unique constraint | partial | parsed; the `unique` constraint is not enforced |
| 60 | Default constraints | yes |  |
| 61 | Distribution directive | yes |  |
| 62 | scheduling constraint statement | no |  |
| 63 | covergroup declaration | yes |  |
| 64 | covergroup instantiation | yes |  |
| 65 | coverpoint declaration | yes |  |
| 66 | bins declaration | yes |  |
| 67 | cross declaration | yes |  |
| 68 | Cover statement and monitor declaration | yes | monitors are declared and matched; see 69-78 |
| 69 | Monitor activity | yes |  |
| 70 | Action traversal statement | yes |  |
| 71 | Sequence monitor activity statement | yes |  |
| 72 | Concat monitor activity statement | yes |  |
| 73 | Eventually monitor activity statement | yes |  |
| 74 | Overlap monitor activity statement | partial | branches are matched independently; true interleaving is not modelled |
| 75 | Select monitor activity statement | yes |  |
| 76 | Schedule monitor activity statement | partial | as overlap: branches are matched independently |
| 77 | Monitor traversal statement | partial | nested monitors are matched by name; monitor inheritance is not applied |
| 78 | Monitor constraints | partial | parsed; a monitor constraint does not filter the matched steps |
| 79 | type extension | yes |  |
| 80 | override declaration | no | parsed; overrides are not applied |
| 81 | package declaration | yes |  |
| 82 | import statement | yes |  |
| 83 | exec block declaration | yes |  |
| 84 | Function declaration | partial |  |
| 85 | Function definition | partial |  |
| 86 | Imported function qualifiers | partial |  |
| 87 | Import class declaration | partial |  |
| 88 | Target-template function implementation | partial |  |
| 89 | Procedural block statement | yes |  |
| 90 | Procedural variable declaration | partial | assignments and declarations are handled; scoping is flat |
| 91 | Procedural assignment statement | yes |  |
| 92 | Void function call | yes |  |
| 93 | Procedural return statement | yes |  |
| 94 | Procedural repeat-count statement | yes |  |
| 95 | Procedural repeat-while statement | yes |  |
| 96 | Procedural foreach statement | yes |  |
| 97 | Procedural if-else statement | yes |  |
| 98 | Procedural match statement | yes |  |
| 99 | Procedural break/continue statement | yes |  |
| 100 | Procedural randomize statement | yes |  |
| 101 | Procedural yield statement | partial | accepted; there is no coroutine to yield from |
| 102 | Export action declaration | partial | parsed; not exported |
| 103 | compile if declaration | partial | parsed; the branch is not selected by the compile-time condition |
| 104 | compile has expression | partial | parsed; not evaluated |
| 105 | compile assert statement | partial | parsed and recorded, not evaluated |
| 106 | String formatting and output functions | partial | `print`/`message` are recorded; `format` returns no string |
| 107 | Runtime messaging function | yes |  |
| 108 | Text file operations using file handles | no |  |
| 109 | Simple text file operations | no |  |
| 110 | Error reporting functions | yes |  |
| 111 | Randomization functions | yes |  |
| 112 | Floating-point storage types | yes |  |
| 113 | float_mantissa function | yes |  |
| 114 | float_exponent function | yes |  |
| 115 | float_sign function | yes |  |
| 116 | to_float function | yes |  |
| 117 | Executor component | no |  |
| 118 | Executor group component | no |  |
| 119 | Executor claim struct | no |  |
| 120 | Executor query function | no |  |
| 121 | Generic address space component | partial | address regions are modelled; traits and claims are not |
| 122 | Contiguous address space component | partial |  |
| 123 | Transparent address space component | partial |  |
| 124 | Base address region type | partial |  |
| 125 | Contiguous address space region type | partial |  |
| 126 | Transparent region type | partial |  |
| 127 | Base address space claim type | no |  |
| 128 | Contiguous address space claim type | no |  |
| 129 | Transparent contiguous address space claim type | no |  |
| 130 | Address space group | no |  |
| 131 | packed_s base struct | partial | parsed; the packed little-endian layout drives register field packing (24.10.1) |
| 132 | sizeof_s struct | yes | `sizeof_s<T>::nbits` / `::nbytes` for packed structs, bit vectors and scalars |
| 133 | Address space handle | yes | a handle is an index into the instance's handle table, holding its absolute address |
| 134 | make_handle_from_claim function | yes | a handle at the claim's address plus an offset |
| 135 | make_handle_from_handle function | yes | a handle at another handle's address plus an offset |
| 136 | addr_value function | yes | the handle's absolute address |
| 137 | addr_value_solve function | yes | as addr_value: regions of an address space are transparent, so the absolute address is known |
| 138 | addr_value_abs function | yes | true for a transparent region, which every region added to an address space is |
| 139 | get_tag function | partial | the region's tag *index* is returned; the engine has no string values, so the name itself is not comparable |
| 140 | Primitive read operations for byte addressable spaces | yes | `read8` / `read16` / `read32` / `read64`, little-endian over a byte-addressable store |
| 141 | Primitive write operations for byte addressable spaces | yes | `write8` / `write16` / `write32` / `write64`, little-endian over a byte-addressable store |
| 142 | Read and write series of bytes | partial | `write_bytes` takes a literal list; `read_bytes` into a list is not supported |
| 143 | Read and write packed structs | no |  |
| 144 | Primitive operation implementation functions | partial | the primitives are implemented in the engine rather than delegated to an executor instance |
| 145 | PSS register definition | yes | `reg_c<R, ACC, SZ>` declarations plus read / read_val / write / write_val / write_masked / write_field, over a register store |
| 146 | PSS register group definition | partial | `reg_group_c` is parsed; offsets come from the group's own get_offset_of_* functions, which are not evaluated |
