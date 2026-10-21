//! # Satya
//!
//! Satya is a portable test-stimulus engine written in Rust. It implements the
//! two roles that PSS tooling plays in a verification flow:
//!
//! * a **system-level scenario engine** — it lowers a PSS model into a
//!   component tree, solves the model's constraints, and schedules the
//!   scenario described by an activity graph; and
//! * a set of **pluggable backends** — the same solved scenario can be emitted
//!   as UVM/SystemVerilog stimulus (the "generator" paradigm) or as a portable
//!   trace that is simulator-, emulator- and silicon-agnostic (the
//!   "system-coordination" paradigm).
//!
//! The pipeline is:
//!
//! ```text
//!   .pss / .json ──► frontend ──► AST ──► IR (component tree)
//!                                            │
//!                                            ▼
//!                                    constraint solver
//!                                            │
//!                                            ▼
//!                                     scenario engine
//!                                            │
//!                          ┌─────────────────┼─────────────────┐
//!                          ▼                 ▼                 ▼
//!                    uvm backend      json backend      trace backend
//! ```
//!
//! Everything is implemented against the Rust standard library only; there are
//! no third-party dependencies.

pub mod ast;
pub mod backend;
pub mod engine;
pub mod error;
pub mod frontend;
pub mod ir;
pub mod lexer;
pub mod parser;
pub mod solver;

pub use error::{PssError, PssResult};

/// Convenience: parse a PSS source string and lower it into the IR.
pub fn compile_pss(src: &str) -> PssResult<ir::Model> {
    let file = parser::parse_source(src)?;
    ir::Model::lower(&file)
}

/// Compile one or more PSS files together into a single model. Paths may be
/// `.pss` files or `.psf` project files (which list `-pss <file>` entries,
/// resolved relative to the project file).
pub fn compile_files(paths: &[String]) -> PssResult<ir::Model> {
    let mut items: Vec<ast::Item> = Vec::new();
    for path in expand_projects(paths)? {
        let src = std::fs::read_to_string(&path)
            .map_err(|e| PssError::plain(format!("cannot read `{}`: {}", path, e)))?;
        let file = parser::parse_source(&src)
            .map_err(|e| PssError::plain(format!("{}: {}", path, e.message)))?;
        items.extend(file.items);
    }
    ir::Model::lower(&ast::SourceFile { items })
}

/// Expand any `.psf` project files into the list of `.pss` files they name.
pub fn expand_projects(paths: &[String]) -> PssResult<Vec<String>> {
    let mut out = Vec::new();
    for path in paths {
        let is_psf = std::path::Path::new(path)
            .extension()
            .map(|e| e.eq_ignore_ascii_case("psf"))
            .unwrap_or(false);
        if !is_psf {
            out.push(path.clone());
            continue;
        }
        let src = std::fs::read_to_string(path)
            .map_err(|e| PssError::plain(format!("cannot read `{}`: {}", path, e)))?;
        let dir = std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("."));
        for line in src.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            if let Some(rest) = line.strip_prefix("-pss") {
                let name = rest.trim().trim_matches('"');
                if name.is_empty() {
                    continue;
                }
                let p = dir.join(name);
                out.push(p.to_string_lossy().to_string());
            }
        }
    }
    Ok(out)
}
