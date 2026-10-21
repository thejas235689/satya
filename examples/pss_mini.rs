//! A single-file demonstration of Satya's whole pipeline.
//!
//! Run it with:
//!
//! ```text
//! cargo run --example pss_mini
//! ```
//!
//! It parses an inline PSS model, solves the scenario, and prints both a
//! portable trace and the head of the generated UVM sequence.

use satya::backend::{get, BackendOptions};
use satya::engine;
use satya::{compile_pss, PssResult};

const MODEL: &str = r#"
package mini_pkg {
   static const int CACHELINE = 64;
   enum mem_block_e { SYS_MEM, LOCAL };

   buffer data_buf {
      rand bit[16] size;
      rand mem_block_e mem_block;
      constraint size >= 1;
   };
}

component Producer_c {
   action produce {
      output data_buf data_buf_out;
      rand int in [1..1024] size;
      constraint data_buf_out.size == size;
      constraint data_buf_out.size <= CACHELINE * 4;
   };
};

component Consumer_c {
   action consume {
      input data_buf data_buf_in;
      rand int in [1..64] num_trans;
      constraint num_trans <= 8;
   };
};

component mini_top {
   Producer_c P;
   Consumer_c C;

   action run {
      activity {
         do Producer_c::produce;
         parallel {
            do Consumer_c::consume;
            repeat (2) { do Consumer_c::consume; };
         };
      };
   };
};
"#;

fn main() -> PssResult<()> {
    let model = compile_pss(MODEL)?;
    println!(
        "Parsed model: {} components, {} actions, top = {}\n",
        model.components.len(),
        model.actions.len(),
        model.top.as_deref().unwrap_or("<none>")
    );

    let scenario = engine::run_scenario(&model, "run", 42);

    let trace = get("trace").unwrap().emit(&model, &scenario, &BackendOptions::default())?;
    println!("{}", trace);

    let sv = get("uvm").unwrap().emit(&model, &scenario, &BackendOptions::default())?;
    println!("--- first 30 lines of generated UVM ---");
    for line in sv.lines().take(30) {
        println!("{}", line);
    }
    Ok(())
}
