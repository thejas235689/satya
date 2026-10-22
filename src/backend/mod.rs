//! Pluggable output backends.
//!
//! A backend turns a solved [`Scenario`] into a concrete artefact. Satya ships
//! three:
//!
//! * [`uvm::UvmBackend`] — emits UVM/SystemVerilog stimulus classes and a
//!   top-level virtual sequence (the "sequence generator" paradigm).
//! * [`c::CBackend`] — emits a native C test harness (host-processor
//!   realization).
//! * [`fw::FwBackend`] — emits the firmware/driver reference: interface map,
//!   address map and headers.
//! * [`bench::BenchBackend`] — emits a post-silicon bench sequence driven over
//!   an external transport (JTAG/SPI/I2C/…).
//! * [`json::JsonBackend`] — emits a portable JSON scenario description.
//! * [`trace::TraceBackend`] — emits a human-readable execution trace.
//!
//! The same scenario feeds all three; adding a backend means implementing
//! [`Backend`] and registering it in [`get`].

pub mod bench;
pub mod c;
pub mod fw;
pub mod json;
pub mod trace;
pub mod uvm;

use crate::ast::Expr;
use crate::engine::Scenario;
use crate::error::PssResult;
use crate::ir::Model;

/// Options shared by all backends.
#[derive(Debug, Clone, Default)]
pub struct BackendOptions {
    /// Name of the top-level virtual sequence class (UVM backend).
    pub top_sequence_name: Option<String>,
    /// Emit a UVM factory registration macro.
    pub uvm_factory: bool,
}

pub trait Backend {
    /// Stable identifier used on the command line.
    fn name(&self) -> &'static str;
    /// One-line description.
    fn description(&self) -> &'static str;
    /// Render the scenario.
    fn emit(&self, model: &Model, scenario: &Scenario, opts: &BackendOptions) -> PssResult<String>;
}

/// All registered backends.
pub fn all() -> Vec<Box<dyn Backend>> {
    vec![
        Box::new(uvm::UvmBackend),
        Box::new(c::CBackend),
        Box::new(fw::FwBackend),
        Box::new(bench::BenchBackend),
        Box::new(json::JsonBackend),
        Box::new(trace::TraceBackend),
    ]
}

/// Look up a backend by its CLI name.
pub fn get(name: &str) -> Option<Box<dyn Backend>> {
    all().into_iter().find(|b| b.name() == name)
}

/// Names of the registered backends, for help text.
pub fn names() -> Vec<&'static str> {
    all().iter().map(|b| b.name()).collect()
}

// ---------------------------------------------------------------------------
// Shared expression rendering
// ---------------------------------------------------------------------------

/// Render an expression using SystemVerilog-style syntax (used by the UVM
/// backend for constraint blocks).
pub fn render_sv(expr: &Expr) -> String {
    match expr {
        Expr::Int(v) => v.to_string(),
        Expr::Bool(b) => b.to_string(),
        Expr::Str(s) => format!("\"{}\"", s),
        Expr::Path(p) => p.segments.join("."),
        Expr::Unary(op, e) => format!("{}{}", op, render_sv(e)),
        Expr::Binary(op, a, b) => {
            if op == "->" {
                format!("{} -> {}", render_sv(a), render_sv(b))
            } else {
                format!("({} {} {})", render_sv(a), op, render_sv(b))
            }
        }
        Expr::Ternary(c, t, e) => format!("({} ? {} : {})", render_sv(c), render_sv(t), render_sv(e)),
        Expr::Slice(base, msb, lsb) => {
            format!("{}[{}:{}]", render_sv(base), render_sv(msb), render_sv(lsb))
        }
        Expr::Call(f, args) => {
            let a: Vec<String> = args.iter().map(render_sv).collect();
            format!("{}({})", f, a.join(", "))
        }
        Expr::Method(r, m, args) => {
            let a: Vec<String> = args.iter().map(render_sv).collect();
            format!("{}.{}({})", render_sv(r), m, a.join(", "))
        }
        Expr::InRange(v, lo, hi) => format!("{} inside {{[{}:{}]}}", render_sv(v), render_sv(lo), render_sv(hi)),
        Expr::InSet(v, items) => {
            let a: Vec<String> = items.iter().map(render_sv).collect();
            format!("{} inside {{{}}}", render_sv(v), a.join(", "))
        }
        Expr::Cast(t, e) => format!("{}({})", t, render_sv(e)),
        Expr::Range(a, b) => format!("{}..{}", render_sv(a), render_sv(b)),
        Expr::Index(a, b) => format!("{}[{}]", render_sv(a), render_sv(b)),
        Expr::List(items) => {
            let a: Vec<String> = items.iter().map(render_sv).collect();
            format!("{{{}}}", a.join(", "))
        }
        Expr::Foreach { vars, iter, body } => format!(
            "foreach ({} in {}) {{ {} }}",
            vars.join(","),
            render_sv(iter),
            render_sv(body)
        ),
        Expr::Unknown(s) => s.clone(),
    }
}

/// Escape a string for embedding in JSON.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Type keys ordered so that a struct is emitted only after the structs it
/// contains (C and the firmware header need this; C has no forward decls here).
pub fn ordered_type_keys(model: &crate::ir::Model) -> Vec<String> {
    use crate::ir::TypeCategory;
    use std::collections::HashSet;
    let mut keys: Vec<String> = model.types.keys().cloned().collect();
    keys.sort();
    let mut done: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    while out.len() < keys.len() {
        let mut progressed = false;
        for k in &keys {
            if done.contains(k) {
                continue;
            }
            let ready = model.types[k].fields.iter().all(|f| match &f.category {
                TypeCategory::Struct(dep) => done.contains(dep) || dep == k,
                _ => true,
            });
            if ready {
                out.push(k.clone());
                done.insert(k.clone());
                progressed = true;
            }
        }
        if !progressed {
            for k in &keys {
                if !done.contains(k) {
                    out.push(k.clone());
                    done.insert(k.clone());
                }
            }
        }
    }
    out
}
