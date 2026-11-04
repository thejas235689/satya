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

> **Verified coverage — read this before trusting the feature list.**
> Every construct Satya claims was re-tested against the published **v0.46.0**
> binary and judged on **observed behaviour**, never on whether it parsed. The
> audit covered **146 constructs and found 74 that behave as the standard
> specifies** — 24 more whose documented limitation reproduces, 35 that behave
> worse than documented, and 13 that do not work at all. That is 51% of the
> construct table, against the 114 rows the feature matrix marks `yes`: the
> matrix was counting *parsed* as *implemented*. The verdicts, the 120
> reproducing models and `tests/feature_proof/run_all.sh` are in
> [`docs/FEATURE-AUDIT.md`](docs/FEATURE-AUDIT.md). Where this README and the
> audit disagree, the audit is the accurate description of the released binary.

---

> **Note — Satya does not replace any commercial tool.**
> It is an independent, open-source implementation, not a substitute for a
> commercial PSS product. Tools such as Cadence Perspec and Siemens InFact are
> mature, vendor-supported, production-validated environments with capabilities
> Satya does not have: full language coverage, coverage closure, integration with
> established verification flows, and commercial support. Satya implements a
> **documented subset** of the standard (see [`docs/SEMANTICS.md`](docs/SEMANTICS.md))
> and exists to explore the space and to serve as a small, readable reference.
> For production stimulus generation, use a commercial tool.
>
> Satya is not affiliated with, endorsed by, sponsored by, or derived from
> Cadence, Siemens, or any other EDA vendor. Product names are the trademarks of
> their respective owners and are used here only to describe the category of
> tooling. See [`NOTICE.md`](NOTICE.md).

---

## Getting it

Prebuilt binaries are attached to each [release](https://github.com/thejas235689/satya/releases/latest).
The alias below always resolves to the newest build:

```bash
curl -LO https://github.com/thejas235689/satya/releases/latest/download/satya-x86_64-linux.tar.gz
tar -xzf satya-x86_64-linux.tar.gz && cd satya-x86_64-linux
./satya demo
```

Each release also carries a version-named archive (for example
`satya-v0.12.0-x86_64-linux.tar.gz`) with a matching `.sha256`, if you need to
pin a specific version.

Or build from source (no dependencies beyond Rust):

```bash
cargo build --release && cargo test
```

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
  --backend <name>         Backend for `gen` (uvm | c | fw | bench | json | trace)
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
every code example in the standard itself, and
[`docs/OPENTITAN.md`](docs/OPENTITAN.md) for modelling a real IP (OpenTitan's
UART) with Satya, and [`docs/SEMANTICS.md`](docs/SEMANTICS.md) for what is
implemented per the standard's defined behaviour and what is only approximated,
and [`docs/FEATURES.md`](docs/FEATURES.md) for a per-construct feature matrix
covering all 146 syntax excerpts the standard defines, and
[`docs/FEATURE-AUDIT.md`](docs/FEATURE-AUDIT.md) for the behavioural re-test of
all 146 of them against v0.46.0, and
[`docs/CORPUS.md`](docs/CORPUS.md) for the result of running Satya against the
shared community [PSS corpus](https://github.com/psstools/pss-corpus).
[`CHANGELOG.md`](CHANGELOG.md) records what each release changed.

---

## The PSS subset

Satya parses a substantial, pragmatic subset of PSS v3.0. Constructs it
understands:

* `package` / `import` (`pkg::*`) — parsed, but `import` does not currently
  affect name resolution (audit row 82)
* `enum` declarations and `extend enum` — an explicit variant value is not
  honoured; a variant solves to its position (audit row 11)
* `struct` / `resource` / `buffer` / `state` with inheritance, template
  parameters and `extend`
* `component` with sub-component instances, `extend component`
* `action` (including `abstract action`) with inheritance and templates
* members: `rand`, `input`, `output`, `lock`, `share`, plain, `bit[N]`,
  template-typed (`data_buf<>`, `executor_c<T>`)
* inline ranges: `rand int in [1..1024] n;` (and the lenient
  `rand int n in [1..1024];`)
* `constraint` blocks: named, `default`, `forall`, and expression-form
  (`constraint x <= 10;`) — `forall` is parsed but not expanded (audit row 58)
* `activity` blocks: `do`, `do … with { … }`, `do … { }`, `parallel`
  (with `join_none` / `join_branch` / `join_select` / `join_first`),
  `schedule`, `select` (with branch guards), `repeat`, `foreach`,
  `replicate`, `if/else`, `atomic`, `sequence`, nested blocks, and
  single-statement bodies (`repeat (n) do X;`) — the four join forms behave
  identically and gate nothing (audit row 34), the `sequence` scheduling
  constraint is not honoured (row 62), and `repeat … while` never re-evaluates
  its condition (row 37)
* `bit[N]` and ranged `bit[msb:lsb]` widths — parsed and sized, but the declared
  width is not enforced on solved values (audit row 9)
* repeat/replicate **index binding**: `repeat (i: 4) do B with { size == i*10; }`
* activity-level `constraint { … }`
* `pool`, `pool [N] T p;`, `bind p *;` — `bind p *;` is parsed and ignored
  (audit row 52)
* `static const`, `compile if (…) { … } else { … }`
* multi-file models and `.psf` project files
* `exec` bodies parsed into statements (assignments, calls, `if`/`else`) and
  translated into the C and UVM backends — only plain `=` and braced block
  bodies take effect; `{ }` blocks, `+=`/`|=`, array-element assignment and
  single-statement `repeat`/`if` bodies are dropped or mis-executed (audit
  rows 89, 91, 94, 97)
* address regions declared in `exec` blocks (`region.addr = …; region.size = …;`)

A bullet above means the syntax is parsed and lowered. It does **not** mean the
construct behaves as the standard requires — of the 146 constructs the audit
judged, 74 do. The parenthetical audit references name the rows the proof shows
are parse-only or wrong.

Constructs outside the modelled subset are *accepted but skipped*, so a real
`.pss` file still parses and its modelled parts remain usable: raw `exec`
bodies (SystemVerilog), `task`/`covergroup` bodies, `symbol` declarations,
`match` arms outside the exact-value braced form, and `super`. Skipped
constructs are noted in the verification report and the design document.

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

The audit qualifies this table: a solved value is not always a *correct* one.
Multi-unknown relations (`a*b <= 20`, `a < b`) are violated on a substantial
fraction of seeds, and membership over a literal list
(`mode in {SYS_MEM, LOCAL}`) is not evaluated at all — the constraint is still
counted satisfied (audit rows 21, 54). The satisfied/total counter reflects the
constraints *seen*, not the constraints *met*.

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

Satya is a working engine, not a certified PSS implementation, and it does not
replace any commercial tool — see [`NOTICE.md`](NOTICE.md). Known limits:

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
  refused and reported. **Resource pools do not currently create instances**, so
  concurrent lockers all resolve to instance 0 and always contend (audit rows
  49, 50).
* **`compile if` takes the primary branch**, but declarations from the enabled
  branch are lost when the statement is nested, at package scope, or inside an
  action body (audit row 103).
* **`task` bodies, `covergroup`s and `symbol` declarations are accepted but not
  executed** — a `symbol` expands to nothing (audit row 43). `typedef` *is*
  implemented (row 17), and `function`s resolve when called unqualified, but a
  package-qualified call does not reach its definition (row 84).
* **`exec` bodies are translated for plain assignments and braced `if`/`else`;**
  `{ }` blocks, `+=`/`|=`, array-element assignment and single-statement
  `repeat`/`if` bodies are dropped or mis-executed (audit rows 89, 91, 94, 97).
  Foreign calls (`make_handle_from_claim`, `add_region`, …) are emitted as
  comments in C and passed through in SystemVerilog — Satya does not implement
  the foreign functions.
* **`match` is only partly executed** — exact-value arms with a braced body
  work, but range arms `[lo..hi]` never match and unbraced arms are dropped
  (audit row 98). `select` index expressions are parsed but ignored.

---

## Repository layout

```
satya/
  Cargo.toml
  README.md
  CHANGELOG.md       what each release changed
  NOTICE.md          relationship to commercial tools; trademarks
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
    opentitan/       PSS models of OpenTitan's UART and AES IPs
  tools/
    pss_corpus_check.py   runs Satya against the shared PSS corpus
    lrm_extract.py   extract examples from the LRM PDF text
    lrm_examples.py  run Satya over every extracted example
  tests/
    integration.rs   end-to-end tests
```

## License

MIT.
