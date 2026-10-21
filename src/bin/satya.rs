//! Satya command-line interface.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

use satya::backend::{self, BackendOptions};
use satya::engine;
use satya::frontend::json::model_from_json;
use satya::ir::Model;
use satya::{compile_pss, PssResult};

const USAGE: &str = "\
Satya — portable test-stimulus engine (PSS → solve → schedule → backends)

USAGE:
    satya <command> [options]

REALIZATIONS (one model, many targets):
    uvm      UVM/SystemVerilog stimulus sequences
    c        native C test harness (host processor)
    fw       firmware/driver reference (interface + address map + headers)
    bench    post-silicon bench sequence (JTAG/SPI/I2C transport)
    json     portable JSON scenario description
    trace    human-readable execution trace

COMMANDS:
    parse <model>            Parse a model and print a summary
    check <model>            Parse a model and report diagnostics
    solve <model>            Solve the scenario and print an execution trace
    gen <model>              Generate output with a backend
    backends                 List available backends
    demo                     Run the built-in demo model end-to-end
    help                     Show this message

OPTIONS:
    --action <name>          Root action to execute (default: `test`, else first)
    --seed <n>               Solver seed (default: 1)
    --backend <name>         Backend for `gen` (see `satya backends`)
    -o, --out <file>         Write output to a file instead of stdout
    --top-sequence <name>    Class name for the UVM top sequence

A model is a `.pss` file, or a `.json` file using Satya's JSON schema.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> PssResult<()> {
    let cmd = match args.first() {
        Some(c) => c.as_str(),
        None => {
            print!("{}", USAGE);
            return Ok(());
        }
    };
    let rest = &args[1..];

    match cmd {
        "help" | "--help" | "-h" => {
            print!("{}", USAGE);
            Ok(())
        }
        "backends" => {
            for b in backend::all() {
                println!("{:<8} {}", b.name(), b.description());
            }
            Ok(())
        }
        "demo" => cmd_demo(rest),
        "parse" => {
            let files = positional_files(rest);
            let model = load_models(&files)?;
            print_summary(&model);
            Ok(())
        }
        "check" => {
            let files = positional_files(rest);
            let model = load_models(&files)?;
            println!("OK: {} parsed and lowered successfully", files.join(", "));
            print_summary(&model);
            Ok(())
        }
        "solve" => {
            let files = positional_files(rest);
            let model = load_models(&files)?;
            let action = pick_action(&model, flag(rest, "--action"))?;
            let seed = flag(rest, "--seed").and_then(|s| s.parse().ok()).unwrap_or(1);
            let scenario = engine::run_scenario(&model, &action, seed);
            let text = backend::get("trace")
                .unwrap()
                .emit(&model, &scenario, &BackendOptions::default())?;
            write_out(flag(rest, "-o").or_else(|| flag(rest, "--out")), &text)?;
            Ok(())
        }
        "gen" => {
            let files = positional_files(rest);
            let model = load_models(&files)?;
            let action = pick_action(&model, flag(rest, "--action"))?;
            let seed = flag(rest, "--seed").and_then(|s| s.parse().ok()).unwrap_or(1);
            let backend_name = flag(rest, "--backend").unwrap_or("uvm");
            let be = backend::get(backend_name)
                .ok_or_else(|| satya::PssError::plain(format!(
                    "unknown backend `{}` (available: {})",
                    backend_name,
                    backend::names().join(", ")
                )))?;
            let scenario = engine::run_scenario(&model, &action, seed);
            let opts = BackendOptions {
                top_sequence_name: flag(rest, "--top-sequence").map(|s| s.to_string()),
                uvm_factory: true,
            };
            let text = be.emit(&model, &scenario, &opts)?;
            write_out(flag(rest, "-o").or_else(|| flag(rest, "--out")), &text)?;
            Ok(())
        }
        other => Err(satya::PssError::plain(format!(
            "unknown command `{}` — run `satya help`",
            other
        ))),
    }
}

fn cmd_demo(rest: &[String]) -> PssResult<()> {
    let src = DEMO_MODEL;
    let model = compile_pss(src)?;
    let action = pick_action(&model, flag(rest, "--action"))?;
    let seed = flag(rest, "--seed").and_then(|s| s.parse().ok()).unwrap_or(1);
    let scenario = engine::run_scenario(&model, &action, seed);
    println!("== Model summary ==");
    print_summary(&model);
    println!("\n== Scenario trace ==");
    print!(
        "{}",
        backend::get("trace").unwrap().emit(&model, &scenario, &BackendOptions::default())?
    );
    let backend_name = flag(rest, "--backend").unwrap_or("uvm");
    if let Some(be) = backend::get(backend_name) {
        println!("\n== Backend: {} ==", backend_name);
        println!(
            "{}",
            be.emit(&model, &scenario, &BackendOptions::default())?
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------

/// Positional arguments, skipping flags and the values of value-taking flags.
fn positional_files(args: &[String]) -> Vec<String> {
    const VALUE_FLAGS: &[&str] = &[
        "--action",
        "--seed",
        "--backend",
        "-o",
        "--out",
        "--top-sequence",
    ];
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if VALUE_FLAGS.contains(&a.as_str()) {
            i += 2;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    out
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().map(|s| s.as_str());
        }
    }
    None
}

/// Load a model from one or more files. A single `.json` file uses the JSON
/// frontend; anything else is treated as PSS (including `.psf` projects).
fn load_models(files: &[String]) -> PssResult<Model> {
    if files.is_empty() {
        return Err(satya::PssError::plain(
            "missing model file — run `satya help`",
        ));
    }
    if files.len() == 1 {
        let path = &files[0];
        if Path::new(path)
            .extension()
            .map(|e| e.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
        {
            let src = std::fs::read_to_string(path)
                .map_err(|e| satya::PssError::plain(format!("cannot read `{}`: {}", path, e)))?;
            return model_from_json(&src);
        }
    }
    satya::compile_files(files)
}

/// Choose the root action: an explicit request must exist; otherwise pick
/// deterministically (prefer `test`, then an activity of the top component,
/// then the first activity in name order).
fn pick_action(model: &Model, requested: Option<&str>) -> PssResult<String> {
    if let Some(a) = requested {
        let leaf = a.rsplit("::").next().unwrap_or(a);
        if !model.actions.contains_key(leaf) {
            let mut available: Vec<String> = model.actions.keys().cloned().collect();
            available.sort();
            return Err(satya::PssError::plain(format!(
                "action `{}` not found in the model (available: {})",
                a,
                available.join(", ")
            )));
        }
        return Ok(leaf.to_string());
    }
    for candidate in ["test", "soc_test"] {
        if model.actions.contains_key(candidate) {
            return Ok(candidate.to_string());
        }
    }
    if let Some(top) = &model.top {
        if let Some(c) = model.components.get(top) {
            let mut owned: Vec<&String> = c.actions.iter().collect();
            owned.sort();
            if let Some(name) = owned
                .into_iter()
                .find(|n| model.actions.get(*n).map(|x| x.activity.is_some()).unwrap_or(false))
            {
                return Ok(name.clone());
            }
        }
    }
    let mut names: Vec<String> = model
        .actions
        .iter()
        .filter(|(_, a)| a.activity.is_some())
        .map(|(n, _)| n.clone())
        .collect();
    names.sort();
    Ok(names
        .into_iter()
        .next()
        .unwrap_or_else(|| "test".to_string()))
}

fn print_summary(model: &Model) {
    println!("packages   : {}", model.packages.len());
    println!("enums      : {}", model.enums.len());
    println!("types      : {}", model.types.len());
    println!("components : {}", model.components.len());
    println!("actions    : {}", model.actions.len());
    println!("top        : {}", model.top.as_deref().unwrap_or("<none>"));
    if !model.duplicates.is_empty() {
        println!(
            "ambiguous  : {} (defined in more than one scope)",
            model.duplicates.join(", ")
        );
    }
    if !model.addr_regions.is_empty() {
        let r: Vec<String> = model
            .addr_regions
            .iter()
            .map(|x| format!("{}@0x{:x}+0x{:x}", x.name, x.base, x.size))
            .collect();
        println!("addr regions: {}", r.join(" "));
    }

    if !model.components.is_empty() {
        println!("\ncomponents:");
        for name in sorted(model.components.keys()) {
            let c = &model.components[&name];
            let subs = if c.subcomponents.is_empty() {
                String::new()
            } else {
                format!(
                    "  [{}]",
                    c.subcomponents
                        .iter()
                        .map(|(i, t)| format!("{}:{}", i, t))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            println!("  {:<16} actions={}{}", name, c.actions.len(), subs);
        }
    }
    if !model.actions.is_empty() {
        println!("\nactions:");
        for name in sorted(model.actions.keys()) {
            let a = &model.actions[&name];
            println!(
                "  {:<20} fields={} constraints={} activity={}{}",
                name,
                a.fields.len(),
                a.constraints.len(),
                a.activity.is_some(),
                if a.is_abstract { "  (abstract)" } else { "" }
            );
        }
    }
}

fn sorted<'a>(keys: impl Iterator<Item = &'a String>) -> Vec<String> {
    let set: BTreeSet<String> = keys.cloned().collect();
    set.into_iter().collect()
}

fn write_out(path: Option<&str>, text: &str) -> PssResult<()> {
    match path {
        Some(p) => {
            std::fs::write(p, text)
                .map_err(|e| satya::PssError::plain(format!("cannot write `{}`: {}", p, e)))?;
            eprintln!("wrote {}", p);
            Ok(())
        }
        None => {
            print!("{}", text);
            Ok(())
        }
    }
}

/// A compact, self-contained demo model used by `satya demo`.
const DEMO_MODEL: &str = r#"
// Satya built-in demo: a producer/consumer interoperability scenario with a
// parallel burst, mirroring the shape of the DVCon workshop examples.
package demo_pkg {
   static const int CACHELINE = 64;
   enum mem_block_e { SYS_MEM, LOCAL };

   buffer data_buf {
      rand bit[16] size;
      rand mem_block_e mem_block;
      constraint size >= 1;
   };
}

component IP_A_c {
   action A_produce {
      output data_buf data_buf_out;
      rand int in [1..1024] size;
      constraint data_buf_out.size == size;
      constraint data_buf_out.size <= 512;
   };
};

component IP_B_c {
   action B_process {
      input data_buf data_buf_in;
      rand int in [1..64] num_trans;
      constraint num_trans <= 16;
   };
};

component pss_top {
   IP_A_c A;
   IP_B_c B;

   pool data_buf data_buf_p;
   bind data_buf_p *;

   exec init_up {
      mem_region.addr = 0x400;
      mem_region.size = 4096;
   };

   action test {
      activity {
         do IP_A_c::A_produce;
         parallel {
            do IP_B_c::B_process;
            repeat (2) { do IP_B_c::B_process; };
         };
      };
   };
};
"#;
