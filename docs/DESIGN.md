# Satya — Design and Architecture

*Version 0.1.0*

## 1. Purpose

Satya is a portable test-stimulus engine written in Rust. It takes a PSS
(Portable Test and Stimulus) model and turns it into concrete, executable
stimulus for a design-under-test. It is designed to occupy, in a single
codebase, the two roles that PSS tooling plays in a modern verification flow:

1. **A system-level scenario engine.** It lowers a model into a component tree,
   solves the model's constraints, and schedules the scenario described by an
   activity graph, coordinating stimulus across multiple components and
   executors. This is the role a tool such as *Perspec* plays.

2. **A stimulus generator.** The scheduled scenario can be emitted as
   UVM/SystemVerilog sequence classes that drop into an existing testbench.
   This is the role a tool such as *InFact* plays.

The distinction between the two roles is, in Satya, an architectural one: the
*engine* is shared and the *output* is a pluggable backend. The UVM generator is
therefore one backend among several rather than the model's reason for
existing, and the same solved scenario can equally drive emulation or
post-silicon bring-up through the portable JSON backend.

## 2. Design principles

* **Backend-agnostic core.** Nothing below the backend layer knows about UVM,
  SystemVerilog, or any other target. The scenario engine produces a
  language-neutral schedule; backends translate it.
* **Lenient lowering.** Real PSS models import methodology packages Satya may
  not have. Unknown types become opaque references rather than errors, so a
  partial or single-file model still yields a usable component tree,
  constraint set and activity graph.
* **Honest limits.** Constructs outside the solved subset are *reported*
  (counted as unsupported, surfaced as warnings) rather than silently dropped.
* **No dependencies.** The engine is built on the Rust standard library alone,
  which keeps the tool auditable and trivially portable.

## 3. Pipeline

```
                ┌──────────────┐        ┌──────────────┐
   .pss  ──────►│   lexer      │───────►│   parser     │───────┐
                └──────────────┘ tokens └──────────────┘  AST  │
                                                               ▼
   .json ──────►┌──────────────┐                       ┌──────────────┐
                │ json frontend│──────────────────────►│  IR lowering │
                └──────────────┘                       └──────────────┘
                                                               │ Model
                                                               ▼
                                                       ┌──────────────┐
                                                       │   solver     │
                                                       └──────────────┘
                                                               │ assignment
                                                               ▼
                                                       ┌──────────────┐
                                                       │   engine     │
                                                       └──────────────┘
                                                               │ Scenario
                                        ┌──────────────────────┼──────────────────────┐
                                        ▼                      ▼                      ▼
                                  ┌──────────┐          ┌──────────┐          ┌──────────┐
                                  │   uvm    │          │   json   │          │  trace   │
                                  └──────────┘          └──────────┘          └──────────┘
```

Each stage has a single responsibility and a narrow interface to the next.

## 4. Module map

| Module | Key types | Responsibility |
|--------|-----------|----------------|
| `error` | `PssError`, `PssResult` | One diagnostic type with an optional source position. |
| `lexer` | `Token`, `TokKind`, `Lexer` | Char-based tokeniser; keywords are identifiers matched by the parser. |
| `ast` | `SourceFile`, `Item`, `ComponentDecl`, `ActionDecl`, `Activity`, `Expr` | Faithful surface syntax. |
| `parser` | `Parser`, `parse_source`, `parse_expression` | Recursive-descent parser; forgiving recovery for out-of-subset constructs. |
| `ir` | `Model`, `Component`, `Action`, `Field`, `Constraint`, `Pool`, `Bind` | The semantic model. `Model::lower` performs lowering. |
| `solver` | `solve`, `SolveReport`, `Env`, `eval`, `Atom`, `Domain` | Domain-propagation constraint solver. |
| `engine` | `run_scenario`, `Scenario`, `Step`, `StepMode` | Activity walker and scheduler. |
| `backend` | `Backend` trait, `all`, `get`, `render_sv` | Backend registry and shared rendering. |
| `backend::uvm` | `UvmBackend` | UVM/SystemVerilog emitter. |
| `backend::c` | `CBackend` | Native C harness emitter. |
| `backend::fw` | `FwBackend` | Firmware reference (interface/address map + header). |
| `backend::bench` | `BenchBackend` | Post-silicon bench sequence (Python transport). |
| `backend::json` | `JsonBackend` | Portable JSON emitter. |
| `backend::trace` | `TraceBackend` | Human-readable emitter. |
| `frontend::json` | `model_from_json`, `Json` | JSON frontend (with its own minimal JSON parser). |
| `bin/satya.rs` | — | Command-line interface (single- or multi-file, `.psf` projects). |
| `lib` | `compile_pss`, `compile_files`, `expand_projects` | Entry points; multi-file and `.psf` project compilation. |

## 5. The intermediate representation

`Model` is the centre of gravity. It is a set of maps keyed by leaf name:

```rust
pub struct Model {
    pub constants: HashMap<String, i64>,
    pub enums: HashMap<String, Vec<String>>,
    pub types: HashMap<String, DataType>,       // struct/resource/buffer/state
    pub components: HashMap<String, Component>,
    pub actions: HashMap<String, Action>,
    pub packages: Vec<String>,
    pub top: Option<String>,
}
```

A `Component` holds sub-component instances, data fields, the actions it
declares, pools, binds, exec blocks and component-level constraints. An
`Action` holds its fields (each classified by `FieldKind` and `TypeCategory`),
constraints, exec blocks, activity and owner component. Inheritance is resolved
during lowering: a component or action merges the members and constraints of
its base chain.

`Field.category` is what lets later stages treat a field appropriately:

```rust
pub enum TypeCategory {
    Scalar,            // int, bit[N], bool
    Enum(Vec<String>), // variant list, solved by index
    Struct(String),    // a type defined in the model — expanded recursively
    Handle,            // opaque address/handle
    Component(String), // a sub-component
    Unknown,           // not resolvable in this model
}
```

## 6. Constraint solver

The solver (`src/solver.rs`) is a domain-propagation engine with a seeded
random assignment step.

**Variable collection.** Starting from an action's fields, numeric leaves are
collected as variables keyed by dotted path (`data_buf_out.size`). Struct-typed
fields are expanded recursively (bounded by depth and a visited set), so a flow
object's fields become variables too. Handles, components and unknown types are
opaque and contribute no variables.

**Constraint collection.** The action's own constraints are taken, plus the
constraints declared on the *types* of its struct fields, with relative
references prefixed to match the variable paths (so a `buffer`'s
`size == mem_seg.size` becomes `data_buf_out.size == data_buf_out.mem_seg.size`).

**Atom extraction.** Each constraint expression is split on `&&` and reduced to
atoms:

```rust
enum Atom {
    Rel(String, Op, Expr),        // var op expr
    InRange(String, i64, i64),    // var in [lo..hi]
    InSet(String, Vec<Expr>),     // var in {..}
    Implies(Expr, Vec<Atom>),     // antecedent -> consequent
    Unsupported(Expr),
}
```

**Propagation.** Relational atoms tighten variable domains; equality between
two variables unions their equality classes (a small union-find); interval and
set membership intersect domains; `!=` adds a forbidden value.

**Assignment.** One value is chosen per equality class, seeded by an LCG so
runs are reproducible. `pick` samples within the domain and falls back to the
first allowed value.

**Arithmetic repair.** Relations that domain propagation cannot handle — where
neither side is a bare variable (`x + y == 100`) — are extracted as
`Atom::General` and resolved by a bounded repair pass. Each pass picks a
violated relation, inverts it for a single participating variable
(`invert_eq` handles `+`, `-`, `*`, `/` by constants and unary minus), and
accepts the inversion only if the result lies inside that variable's domain;
otherwise it tries another operand. This is a heuristic, not a linear-programming
solver, but it covers the arithmetic that appears in practice.

**Verification.** Every atom is re-checked against the repaired assignment; the
report carries `satisfied`/`total` counts and the list of unsupported atoms,
which the CLI and backends surface.

## 7. Scenario engine

`run_scenario(model, root_action, seed)` walks the root action's activity and
records a `Scenario` — an ordered list of `Step`s:

```rust
pub struct Step {
    pub index: usize,
    pub action: String,
    pub component: String,   // e.g. "pss_top.IP_A_c"
    pub depth: usize,
    pub mode: StepMode,      // Sequential | Parallel | Schedule | Atomic | Select
    pub group: u32,          // parallel/schedule/select group id
    pub fields: Vec<(String, i64)>, // solved values
    pub satisfied: usize,
    pub total: usize,
    pub unresolved: bool,
}
```

The walker's rules:

* A `do X` resolves `X` (optionally component-qualified) to an action. If the
  action has its own `activity`, that activity is expanded; otherwise the action
  is a leaf and a step is recorded after solving its constraints.
* `parallel` / `schedule` assign a group id; the backend uses it to emit
  `fork … join`.
* `select` takes the first branch (and warns when there are alternatives).
* `repeat (n)` unrolls the body `n` times.
* `foreach` executes its body once (pool sizes are not modelled) and warns.
* `atomic` marks the enclosed steps.
* Component-qualified `do Comp::action` is resolved to an instance of `Comp`
  under the current or top component, producing a dotted component path.

**Flow objects and pools.** Pools are gathered from the top component and its
sub-components. An `output` field of a type served by a bound pool allocates an
object (carrying that field's solved values); an `input` consumes the next
object of that type. `foreach` over a pool iterates the allocated objects,
binding both the index and each object's fields. Over-allocation beyond a pool's
declared count is reported.

**Resource arbitration.** `lock`/`share` fields claim instances of a resource
type. The number of available instances is estimated from the model's component
instances (a resource `pssm_executor_r` is served by `pssm_executor_c`
instances). Inside a parallel or schedule group, the engine counts exclusive
claims per resource and reports contention when they exceed the instances
available.

**Address space.** Address regions declared in `exec` blocks
(`region.addr` / `region.size`) are collected during lowering. Each allocated
flow object is given a 64-byte-aligned base address from the first region with
room; the address flows into the firmware and bench backends.

## 8. Backends

A backend is a small trait:

```rust
pub trait Backend {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn emit(&self, model: &Model, scenario: &Scenario, opts: &BackendOptions) -> PssResult<String>;
}
```

* **`uvm`** emits one `uvm_sequence` class per distinct action in the schedule
  (with `rand` fields, a translated `constraint` block, and a reference
  solution in a comment), plus a top-level virtual sequence that replays the
  schedule, using `fork … join` for parallel groups. Constraints are rendered
  with `render_sv`, which maps PSS `x in [lo..hi]` to SV `x inside {[lo:hi]}`.
* **`c`** emits a self-contained native C harness: type definitions, one
  function per action, and a `main()` that replays the schedule. Translated
  `exec` statements appear inside the action functions; foreign calls become
  comments so the file always compiles.
* **`fw`** emits the firmware/driver reference as Markdown: interface map
  (types, fields, widths), address map (regions and allocated objects), the
  scenario table, and an embedded C header.
* **`bench`** emits a runnable Python driver with the scenario as data and a
  pluggable `Transport` (`execute`/`write`/`read`) so a bench adapter for
  JTAG/SPI/I2C/UART can be dropped in. Flow objects with addresses become
  `write` operations.
* **`json`** emits a portable description of the scenario: root action, top
  component, per-step action/component/mode/group/fields, flow objects, claims
  and constraint counts, plus warnings.
* **`trace`** emits a human-readable execution trace, including flow objects,
  addresses and resource claims.

Adding a backend means implementing `Backend` and registering it in
`backend::all`.

## 9. Relationship to InFact and Perspec

The comparison that motivated this project frames two tools by their
relationship to UVM. Satya's architecture maps onto that comparison as follows:

| Aspect | Satya |
|--------|-------|
| Relationship to UVM | UVM is *one backend*. The engine is independent of it. |
| Abstraction level | Above UVM: a system-level component tree, activity graph and scheduler. |
| What you change in your TB | Add a generated sequence *or* consume the portable JSON scenario. |
| Multi-agent coordination | Core: `parallel`/`schedule`/`select` groups, component-qualified `do`, per-component solve. |
| Portability scope | One model → simulation, emulation and post-silicon, via backend selection. |

## 10. Limitations and roadmap

**Current limitations** (also listed in the README): qualified name resolution
without import-based scope checking; opaque unknown types; resource contention
resolved by serialising (no queuing); `compile if` takes the primary branch; `function`/`task` bodies, `covergroups`, `typedef`s and
`symbol` declarations are accepted but not executed; `exec` bodies are
translated only for assignments and `if`/`else`, and foreign calls are not
implemented; arithmetic solving is single-unknown heuristic inversion rather
than a linear-programming solver.

**Roadmap**

1. Import-based scope checking, so an unqualified name resolves through the
   model's `import` statements instead of first-wins.
2. Queuing/priority for resource claims (today contention is resolved by
   serialising the conflicting steps).
3. A full linear-arithmetic solver (interval/linear programming) to replace the
   single-unknown inversion heuristic.
4. Additional backends: SystemC/C++, Python, and a cycle-accurate waveform
   schedule.
5. `exec` body translation for the UVM backend.
6. Evaluating `select` guards and `match` choices from the solved assignment.

## 11. Testing

`tests/integration.rs` drives the whole pipeline: parsing (packages, enums,
types, components, component-qualified activity), lowering, scheduling (leaf
count and parallel grouping), constraint satisfaction and equality linking, and
all three backends plus the JSON frontend. Run with:

```bash
cargo test
```
