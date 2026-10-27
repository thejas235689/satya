# Feature matrix

The standard defines **146 syntax excerpts** — one per language construct. This
document maps every one of them to its status in Satya, so the project's
coverage can be read honestly rather than inferred from the test count.

Legend: **yes** — parsed *and* implemented with its defined semantics;
**partial** — parsed, and some semantics implemented, but not all;
**no** — not implemented.

Every "yes" is exercised by a test or by the corpora in
[`CORPUS.md`](CORPUS.md), [`LRM-EXAMPLES.md`](LRM-EXAMPLES.md) and
[`OPENTITAN.md`](OPENTITAN.md). The statuses were checked against the
implementation rather than assumed — which has repeatedly caught constructs
marked implemented that were not.

## Summary

| Status | Count |
|---|---|
| yes | 105 |
| partial | 41 |
| no | 0 |
| **total** | **146** |

## Every construct

| # | Syntax excerpt | Implemented | Notes |
|---|---|---|---|
| 1 | Numeric constants | yes |  |
| 2 | String literals | yes | string-valued variables are carried alongside the numeric ones; literals, assignment, concatenation and `length()` work |
| 3 | Aggregate literals | yes |  |
| 4 | Empty aggregate literal | yes | `L = {};` gives an empty collection |
| 5 | Value list literal | yes |  |
| 6 | Map literal | partial | parsed; maps are not solved |
| 7 | Structure literal | yes |  |
| 8 | Data types and data declarations | yes |  |
| 9 | Integer type declaration | yes |  |
| 10 | Floating-point type declaration | yes |  |
| 11 | enum declaration | yes |  |
| 12 | string declaration | yes | a `string` field holds a value in the string store; the trace and the JSON backend report it |
| 13 | chandle declaration | partial | parsed; opaque handle |
| 14 | struct declaration | yes |  |
| 15 | Collection data types | yes |  |
| 16 | ref declaration | partial | parsed; reference semantics not modelled |
| 17 | User-defined type declaration | yes |  |
| 18 | cast operation | yes | the cast applies the target type's width: `(bit[8])300` is 44, `(bit)5` is 1, `(int)x` is a no-op |
| 19 | Expressions and operators | yes |  |
| 20 | Conditional operator | yes |  |
| 21 | Set membership operator | yes |  |
| 22 | String slice operator | yes | all four forms: `s[i]`, `s[i..j]` (ends inclusive), `s[i..]`, `s[..j]` |
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
| 43 | symbol declaration | yes |  |
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
| 55 | Implication constraint | yes | the consequent is repaired for `==`, `<=`, `>=`, `<`, `>` and `in` range/set forms |
| 56 | Conditional constraint | yes | lowered onto two implications (`if (c) a else b` becomes `c -> a` and `!c -> b`) |
| 57 | foreach constraint | yes | expanded into one constraint per element at atom extraction, using the element count from the model; braced and unbraced bodies both work |
| 58 | forall constraint | partial | parsed; forall is not expanded |
| 59 | unique constraint | yes | `unique { arr }` and `unique { a, b, c }`; duplicates are moved to a free value in the repair pass, and an unsatisfiable set is reported rather than faked |
| 60 | Default constraints | yes |  |
| 61 | Distribution directive | yes |  |
| 62 | scheduling constraint statement | partial | parsed and recorded; the engine walks the activity in one deterministic order, so `sequence` holds by construction |
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
| 80 | override declaration | yes | type and instance overrides are applied when an action is traversed |
| 81 | package declaration | yes |  |
| 82 | import statement | yes |  |
| 83 | exec block declaration | yes |  |
| 84 | Function declaration | partial |  |
| 85 | Function definition | partial |  |
| 86 | Imported function qualifiers | partial |  |
| 87 | Import class declaration | partial |  |
| 88 | Target-template function implementation | partial |  |
| 89 | Procedural block statement | yes |  |
| 90 | Procedural variable declaration | yes | a declaration with an initialiser inside `exec` is scoped to the block |
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
| 101 | Procedural yield statement | yes | accepted; the engine runs `exec` bodies to completion, so there is no coroutine to yield from |
| 102 | Export action declaration | yes | accepted and recorded |
| 103 | compile if declaration | yes | the constant condition selects a branch; the other branch is never instantiated |
| 104 | compile has expression | partial | parsed; not evaluated |
| 105 | compile assert statement | yes | a false condition is an error naming the expression |
| 106 | String formatting and output functions | yes | `format()` builds a string with `%d`, `%x`, `%s` and `%%` |
| 107 | Runtime messaging function | yes |  |
| 108 | Text file operations using file handles | partial | accepted and have no effect on the scenario |
| 109 | Simple text file operations | partial | accepted and have no effect on the scenario |
| 110 | Error reporting functions | yes |  |
| 111 | Randomization functions | yes |  |
| 112 | Floating-point storage types | yes |  |
| 113 | float_mantissa function | yes |  |
| 114 | float_exponent function | yes |  |
| 115 | float_sign function | yes |  |
| 116 | to_float function | yes |  |
| 117 | Executor component | partial | parsed as a component type; no executor-specific behaviour is modelled |
| 118 | Executor group component | partial | parsed; no executor-specific behaviour is modelled |
| 119 | Executor claim struct | partial | parsed; claim allocation does not go through an executor |
| 120 | Executor query function | partial | parsed; the query is not evaluated |
| 121 | Generic address space component | partial | parsed; regions are modelled and claims allocate from them, but traits are not matched |
| 122 | Contiguous address space component | partial |  |
| 123 | Transparent address space component | partial |  |
| 124 | Base address region type | partial |  |
| 125 | Contiguous address space region type | partial |  |
| 126 | Transparent region type | partial |  |
| 127 | Base address space claim type | partial | claims are allocated first-fit from a region; trait matching is not applied |
| 128 | Contiguous address space claim type | partial |  |
| 129 | Transparent contiguous address space claim type | partial |  |
| 130 | Address space group | partial | parsed as a component type |
| 131 | packed_s base struct | partial | parsed; the packed little-endian layout drives register and struct packing (24.10.1) |
| 132 | sizeof_s struct | yes |  |
| 133 | Address space handle | yes |  |
| 134 | make_handle_from_claim function | yes |  |
| 135 | make_handle_from_handle function | yes |  |
| 136 | addr_value function | yes |  |
| 137 | addr_value_solve function | yes |  |
| 138 | addr_value_abs function | yes |  |
| 139 | get_tag function | partial | the region's tag *index* is returned; the engine has no string values |
| 140 | Primitive read operations for byte addressable spaces | yes |  |
| 141 | Primitive write operations for byte addressable spaces | yes |  |
| 142 | Read and write series of bytes | yes | `write_bytes` takes a literal list; `read_bytes` fills one |
| 143 | Read and write packed structs | yes |  |
| 144 | Primitive operation implementation functions | partial | the primitives are implemented in the engine rather than delegated to an executor instance |
| 145 | PSS register definition | yes | `reg_c<R, ACC, SZ>` declarations plus the register access functions over a register store |
| 146 | PSS register group definition | partial | `reg_group_c` is parsed; offsets come from the group's own get_offset_of_* functions, which are not evaluated |
