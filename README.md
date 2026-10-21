# Satya

**A portable test-stimulus engine, written in Rust.**

Satya reads a PSS (Portable Test and Stimulus) model, solves its constraints,
schedules the scenario described by its activity graph, and emits that scenario
through one of several backends — UVM/SystemVerilog stimulus, a portable JSON
description, or a human-readable trace.

It implements, in one engine, the two roles PSS tooling plays in a verification
flow:

* a **system-level scenario engine** that coordinates stimulus across a
  component tree (the "Perspec-style" role), and
* a **sequence generator** whose UVM output is just one of several backends
  (the "InFact-style" role).

Because the solved scenario is backend-agnostic, the same model can drive
simulation, emulation or post-silicon bring-up without changing the model.

---

## Quick start

```bash
# Run the built-in demo end-to-end (no files needed)
cargo run -- demo

# Parse a model and print a summary
cargo run -- parse examples/demo.pss

# Solve the scenario and print an execution trace
cargo run -- solve examples/demo.pss --action test

# Generate UVM/SystemVerilog sequences
cargo run -- gen examples/demo.pss --backend uvm -o demo_sequences.sv

# Generate the other realizations of the same model
cargo run -- gen examples/demo.pss --backend c     -o scenario.c      # native C
cargo run -- gen examples/demo.pss --backend fw    -o fw_reference.md # firmware docs + header
cargo run -- gen examples/demo.pss --backend bench -o bench_seq.py   # post-silicon sequence
cargo run -- gen examples/demo.pss --backend json  -o scenario.json  # portable JSON

# List the available backends
cargo run -- backends

# The single-file walkthrough
cargo run --example pss_mini
```

Satya has **no third-party dependencies** — it builds with the Rust standard
library alone (Rust 1.70+; developed against 1.98).

```bash
cargo build --release
cargo test
```

---

## Command-line interface

```
satya <command> [options]

  parse <model>            Parse a model and print a summary
  check <model>            Parse a model and report diagnostics
  solve <model>            Solve the scenario and print an execution trace
  gen <model>              Generate output with a backend
  backends                 List available backends
  demo                     Run the built-in demo model end-to-end

Options
  --action <name>          Root action to execute (default: `test`, else first)
  --seed <n>               Solver seed (default: 1)
  --backend <name>         Backend for `gen` (uvm | json | trace)
  -o, --out <file>         Write output to a file instead of stdout
  --top-sequence <name>    Class name for the UVM top sequence
```

A model is either a `.pss` file (PSS v3.0 subset), a `.json` file using the
schema below, or a `.psf` project file listing `-pss` entries. Several model
files may be given at once and are compiled together:

```bash
satya parse examples/3_ip.psf
satya solve file_a.pss file_b.pss --action test
```

---

## How it works

```
 .pss  ─┐
        ├─► frontend ─► AST ─► IR (component tree) ─► constraint solver
 .json ─┘                                              │
                                                       ▼
                                              scenario engine (scheduler)
                                                       │
                              ┌────────────────────────┼────────────────────────┐
                              ▼                        ▼                        ▼
                        uvm backend             json backend            trace backend
                    (SystemVerilog/UVM)      (portable scenario)      (human-readable)
```

| Stage | Module | Responsibility |
|-------|--------|----------------|
| Lexer | `src/lexer.rs` | Tokenise PSS source (comments, literals, operators) |
| Parser | `src/parser.rs` | Recursive-descent parser for the PSS subset → AST |
| AST | `src/ast.rs` | Faithful surface syntax (names, not resolved symbols) |
| Lowering | `src/ir.rs` | Build the semantic model: components, actions, fields, constraints, pools, binds |
| Solver | `src/solver.rs` | Domain-propagation constraint solver with a seeded assignment step |
| Engine | `src/engine.rs` | Walk the activity graph, resolve `do`, solve, record a schedule |
| Backends | `src/backend/` | Emit one of six realizations from the schedule |
| JSON frontend | `src/frontend/json.rs` | Build a model from JSON |

### One model, six realizations

PSS is most valuable as a *single source of truth*: one model, several test
realizations that stay in sync. Satya makes that concrete — every backend
consumes the same solved scenario:

| Backend | Realization | Typical use |
|---------|-------------|-------------|
| `uvm` | UVM/SystemVerilog sequence classes | pre-silicon simulation |
| `c` | self-contained native C harness | host-processor / software-driven tests |
| `fw` | interface map, address map, C header | driver and firmware reference |
| `bench` | Python sequence over a pluggable transport | post-silicon (JTAG/SPI/I2C/UART) |
| `json` | portable scenario description | any other harness |
| `trace` | human-readable execution trace | debugging the model |

The C, firmware and bench backends correspond to the three realizations a
PSS-as-single-source-of-truth flow needs — native mode, UVM mode and firmware
reference mode — plus the pre-/post-silicon bridge.

See [`docs/DESIGN.md`](docs/DESIGN.md) for the detailed design,
[`docs/VERIFICATION.md`](docs/VERIFICATION.md) for the verification report
(checked against the PSS v3.0 standard and the DVCon 2025 workshop examples) and
[`docs/LRM-EXAMPLES.md`](docs/LRM-EXAMPLES.md) for the conformance run over
every code example in the standard itself.

---

## The PSS subset

Satya parses a substantial, pragmatic subset of PSS v3.0. Constructs it
understands:

* `package` / `import` (`pkg::*`)
* `enum` declarations and `extend enum`
* `struct` / `resource` / `buffer` / `state` with inheritance, template
  parameters and `extend`
* `component` with sub-component instances, `extend component`
* `action` (including `abstract action`) with inheritance and templates
* members: `rand`, `input`, `output`, `lock`, `share`, plain, `bit[N]`,
  template-typed (`data_buf<>`, `executor_c<T>`)
* inline ranges: `rand int in [1..1024] n;` (and the lenient
  `rand int n in [1..1024];`)
* `constraint` blocks: named, `default`, `forall`, and expression-form
  (`constraint x <= 10;`)
* `activity` blocks: `do`, `do … with { … }`, `do … { }`, `parallel`
  (with `join_none` / `join_branch` / `join_select` / `join_first`),
  `schedule`, `select` (with branch guards), `repeat`, `foreach`,
  `replicate`, `if/else`, `atomic`, `sequence`, nested blocks, and
  single-statement bodies (`repeat (n) do X;`)
* `bit[N]` and ranged `bit[msb:lsb]` widths
* repeat/replicate **index binding**: `repeat (i: 4) do B with { size == i*10; }`
* activity-level `constraint { … }`
* `pool`, `pool [N] T p;`, `bind p *;`
* `static const`, `compile if (…) { … } else { … }`
* multi-file models and `.psf` project files
* `exec` bodies parsed into statements (assignments, calls, `if`/`else`) and
  translated into the C and UVM backends
* address regions declared in `exec` blocks (`region.addr = …; region.size = …;`)

Constructs outside the modelled subset are *accepted but skipped*, so a real
`.pss` file still parses and its modelled parts remain usable: raw `exec`
bodies (SystemVerilog), `function`/`task`/`covergroup`, `typedef`, `symbol`
declarations, `match` statements and `super`. Skipped constructs are noted in
the verification report and the design document.

### Constraints solved

The solver handles the constraints that dominate real models:

| Construct | Example |
|-----------|---------|
| Equality / inequality | `data_buf_out.size == size`, `a != b` |
| Ordering | `size <= 512`, `num_trans >= 1` |
| Membership | `x in [1..1024]`, `mode in {SYS_MEM, LOCAL}` |
| Conjunction | `a == b && c <= 10` |
| Implication | `a == 1 -> b == 2` |
| Boolean negation | `!state.initial` |
| Enum values | solved as their variant index |
| Arithmetic on one unknown | `x + y == 100`, `y * 2 == z` (single-unknown inversion) |

Arithmetic relations are solved by a repair pass that inverts a single unknown
(`x + y == 100` re-solves one operand); a relation that stays unsatisfiable
within the declared domains is reported in the satisfied/total count rather
than hidden.

---

## JSON model schema

```json
{
  "packages": ["demo_pkg"],
  "constants": { "CACHELINE": 64 },
  "enums": { "mem_block_e": ["SYS_MEM", "LOCAL"] },
  "types": {
    "data_buf": {
      "fields": [
        { "name": "size", "kind": "rand", "type": "bit", "bits": 16 },
        { "name": "mem_block", "kind": "rand", "type": "mem_block_e" }
      ],
      "constraints": ["size >= 1"]
    }
  },
  "components": {
    "pss_top": {
      "subcomponents": { "P": "Producer_c" },
      "actions": {
        "test": {
          "activity": [
            { "do": "Producer_c::produce" },
            { "parallel": [ { "do": "consume" }, { "repeat": [ { "do": "consume" } ], "count": 2 } ] }
          ]
        },
        "produce": {
          "fields": [
            { "name": "data_buf_out", "kind": "output", "type": "data_buf" },
            { "name": "size", "kind": "rand", "type": "int", "in": [1, 1024] }
          ],
          "constraints": ["data_buf_out.size == size", "data_buf_out.size <= 512"]
        }
      }
    }
  },
  "top": "pss_top"
}
```

* `kind` is one of `rand`, `plain`, `input`, `output`, `lock`, `share`.
* `type` is a built-in (`int`, `bit`, `bool`), an enum, a type, or a component.
* `bits` sets the width of a `bit` field.
* `in` is an inline `[lo, hi]` range.
* `constraints` is a list of expression strings, parsed by the same expression
  parser used for PSS.

See [`examples/demo.json`](examples/demo.json) for a complete example.

---

## Library use

```rust
use satya::{compile_pss, engine};
use satya::backend::{get, BackendOptions};

let model = compile_pss(MODEL_SRC)?;
let scenario = engine::run_scenario(&model, "test", /* seed */ 1);
let sv = get("uvm").unwrap().emit(&model, &scenario, &BackendOptions::default())?;
println!("{}", sv);
```

---

## Limitations

Satya is a working engine, not a certified PSS implementation. Known limits:

* **Name resolution is qualified, but not scope-checked.** `pkg::name` and
  `Component::action` references resolve, and types that collide across packages
  are kept distinct, but imports are not used to disambiguate an unqualified
  leaf reference — such a name is flagged as ambiguous and the first
  declaration wins.
* **Unknown types are opaque.** A type not defined in the model (typically from
  a methodology package such as `addr_reg_pkg`) is treated as an opaque handle;
  constraints referring to its fields are not solved.
* **Resource arbitration is enforced by serialisation.** When a parallel group
  makes more exclusive `lock` claims than there are instances, the conflicting
  steps are moved out of the group (serialised) and the decision reported; there
  is no queuing or priority.
* **Pools are allocated and consumed**, `foreach` over a pool iterates its
  objects, and a pool's declared count is enforced — an allocation beyond it is
  refused and reported.
* **`compile if` takes the primary branch.**
* **`function`/`task` bodies, `covergroup`s, `typedef`s and `symbol`
  declarations are accepted but not executed.**
* **`exec` bodies are translated for simple assignments and `if`/`else`;**
  foreign calls (`make_handle_from_claim`, `add_region`, …) are emitted as
  comments in C and passed through in SystemVerilog — Satya does not implement
  the foreign functions.
* **`match` is executed** with `[lo..hi]`, `[value, lo..hi, …]` lists and
  `default`; `select` index expressions are parsed but ignored.

---

## Repository layout

```
satya/
  Cargo.toml
  README.md
  docs/DESIGN.md
  src/
    lib.rs           crate root and pipeline entry point
    error.rs         diagnostics
    lexer.rs         tokeniser
    ast.rs           abstract syntax tree
    parser.rs        recursive-descent parser
    ir.rs            semantic model and lowering
    solver.rs        constraint solver
    engine.rs        scenario engine / scheduler
    backend/
      mod.rs         Backend trait, registry, shared rendering
      uvm.rs         UVM/SystemVerilog emitter
      json.rs        portable JSON emitter
      trace.rs       human-readable trace emitter
    frontend/
      mod.rs
      json.rs        JSON model frontend
    bin/satya.rs     command-line interface
  examples/
    demo.pss         example PSS model
    demo.json        the same model in JSON
    pss_mini.rs      single-file library walkthrough
    lrm/             197 code examples extracted from the PSS v3.0 standard
  tools/
    lrm_extract.py   extract examples from the LRM PDF text
    lrm_examples.py  run Satya over every extracted example
  tests/
    integration.rs   end-to-end tests
```

## License

MIT.
