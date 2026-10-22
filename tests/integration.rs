//! Integration tests exercising the full Satya pipeline.

use satya::backend::{get, BackendOptions};
use satya::engine;
use satya::frontend::json::model_from_json;
use satya::{compile_pss, parser};

const MODEL: &str = r#"
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

#[test]
fn parses_packages_enums_types_and_components() {
    let m = compile_pss(MODEL).expect("model should parse");
    assert_eq!(m.packages, vec!["demo_pkg".to_string()]);
    assert_eq!(m.enums.get("mem_block_e").map(|v| v.len()), Some(2));
    assert!(m.types.keys().any(|k| k.ends_with("data_buf")));
    assert_eq!(m.components.len(), 3);
    assert_eq!(m.top.as_deref(), Some("pss_top"));

    let top = m.components.get("pss_top").unwrap();
    assert_eq!(top.subcomponents.len(), 2);
    assert_eq!(top.subcomponents[0], ("A".to_string(), "IP_A_c".to_string()));
}

#[test]
fn lowers_component_qualified_activity() {
    let m = compile_pss(MODEL).unwrap();
    let test = m.actions.get("test").unwrap();
    assert!(test.activity.is_some());
}

#[test]
fn scenario_engine_schedules_every_leaf_action() {
    let m = compile_pss(MODEL).unwrap();
    let s = engine::run_scenario(&m, "test", 1);
    // A_produce once, B_process three times (1 + repeat 2).
    assert_eq!(s.concrete_steps(), 4);
    // The three B_process steps share a parallel group.
    let parallel: Vec<_> = s.steps.iter().filter(|st| st.group > 0).collect();
    assert_eq!(parallel.len(), 3);
    assert!(parallel.iter().all(|st| st.action == "B_process"));
}

#[test]
fn solver_respects_ranges_and_equalities() {
    let m = compile_pss(MODEL).unwrap();
    let s = engine::run_scenario(&m, "test", 7);
    for step in &s.steps {
        assert_eq!(
            step.satisfied, step.total,
            "all constraints should be satisfied for {}",
            step.action
        );
        for (k, v) in &step.fields {
            match k.as_str() {
                "num_trans" => assert!((1..=16).contains(v), "num_trans out of range: {}", v),
                "size" | "data_buf_out.size" => {
                    assert!((1..=512).contains(v), "size out of range: {}", v)
                }
                "data_buf_out.mem_block" => assert!((0..=1).contains(v)),
                _ => {}
            }
        }
    }
}

#[test]
fn solver_links_equality_between_fields() {
    let m = compile_pss(MODEL).unwrap();
    let s = engine::run_scenario(&m, "test", 3);
    let step = &s.steps[0];
    let size = step.fields.iter().find(|(k, _)| k == "size").map(|(_, v)| *v);
    let out = step
        .fields
        .iter()
        .find(|(k, _)| k == "data_buf_out.size")
        .map(|(_, v)| *v);
    assert_eq!(size, out, "`data_buf_out.size == size` must hold");
}

#[test]
fn uvm_backend_emits_sequence_classes() {
    let m = compile_pss(MODEL).unwrap();
    let s = engine::run_scenario(&m, "test", 1);
    let sv = get("uvm").unwrap().emit(&m, &s, &BackendOptions::default()).unwrap();
    assert!(sv.contains("class A_produce_seq extends uvm_sequence"));
    assert!(sv.contains("class B_process_seq extends uvm_sequence"));
    assert!(sv.contains("class satya_top_seq extends uvm_sequence"));
    assert!(sv.contains("fork"));
    assert!(sv.contains("join"));
}

#[test]
fn json_backend_emits_valid_shape() {
    let m = compile_pss(MODEL).unwrap();
    let s = engine::run_scenario(&m, "test", 1);
    let js = get("json").unwrap().emit(&m, &s, &BackendOptions::default()).unwrap();
    assert!(js.contains("\"root_action\": \"test\""));
    assert!(js.contains("\"steps\": ["));
    assert!(js.contains("\"action\": \"B_process\""));
    // Braces should balance.
    let opens = js.matches('{').count();
    let closes = js.matches('}').count();
    assert_eq!(opens, closes, "JSON braces must balance");
}

#[test]
fn json_frontend_builds_a_solvable_model() {
    let src = r#"{
      "constants": { "LIMIT": 100 },
      "enums": { "mode_e": ["A", "B"] },
      "types": { "buf": { "fields": [ {"name":"size","kind":"rand","type":"bit","bits":16} ] } },
      "components": {
        "pss_top": {
          "subcomponents": { "A": "IP_c" },
          "actions": {
            "test": { "activity": [ {"do": "produce"} ] },
            "produce": {
              "fields": [
                {"name":"size","kind":"rand","type":"int","in":[1,50]},
                {"name":"buf_out","kind":"output","type":"buf"}
              ],
              "constraints": ["buf_out.size == size", "size <= 25"]
            }
          }
        },
        "IP_c": { "actions": { "idle": {} } }
      },
      "top": "pss_top"
    }"#;
    let m = model_from_json(src).expect("json model should build");
    assert!(m.components.contains_key("pss_top"));
    let s = engine::run_scenario(&m, "test", 5);
    assert_eq!(s.concrete_steps(), 1);
    let step = &s.steps[0];
    let size = step.fields.iter().find(|(k, _)| k == "size").map(|(_, v)| *v).unwrap();
    assert!((1..=25).contains(&size), "size out of range: {}", size);
}

#[test]
fn malformed_input_reports_an_error_with_position() {
    let err = parser::parse_source("component C { action x { activity { do ;").unwrap_err();
    assert!(err.line > 0);
}

#[test]
fn unknown_backend_is_rejected() {
    assert!(get("nonsense").is_none());
    let names = satya::backend::names();
    assert!(names.contains(&"uvm") && names.contains(&"json") && names.contains(&"trace"));
}

// ---------------------------------------------------------------------------
// LRM-conformance tests (PSS v3.0 grammar, Annex B)
// ---------------------------------------------------------------------------

const LRM_GRAMMAR: &str = r#"
component C {
   action A { rand bit[3:0] f1; };
   action B { rand int in [0..9] size; };
   action t {
      rand int in [1..4] count;
      activity {
         repeat (i: 4) do B with { size == i*10; };
         if (count > 2) do A; else do B;
         foreach (e: count) { do A; };
         select { (count==1): do A; default: do B; };
         parallel join_none { do A; do B; };
         sequence { do A; do B; };
         constraint { count <= 4; };
         replicate (i: 2) do A;
      };
   };
};
"#;

#[test]
fn parses_lrm_activity_grammar() {
    let m = compile_pss(LRM_GRAMMAR).expect("LRM activity grammar should parse");
    assert!(m.actions.contains_key("t"));
}

#[test]
fn ranged_bit_width_is_parsed() {
    let m = compile_pss(LRM_GRAMMAR).unwrap();
    let a = m.actions.get("A").unwrap();
    let f = a.field("f1").unwrap();
    // bit[3:0] => 4 bits
    assert_eq!(f.bits, Some(4));
}

#[test]
fn repeat_index_binds_into_inline_constraints() {
    let m = compile_pss(LRM_GRAMMAR).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    // The first four steps come from `repeat (i: 4) do B with { size == i*10; }`.
    let sizes: Vec<i64> = s
        .steps
        .iter()
        .filter(|st| st.action == "B")
        .filter_map(|st| st.fields.iter().find(|(k, _)| k == "size").map(|(_, v)| *v))
        .take(4)
        .collect();
    assert_eq!(sizes, vec![0, 10, 20, 30], "index must be bound in the inline constraint");
}

#[test]
fn nested_template_defaults_parse() {
    let src = r#"
      struct mem_segment_s<struct T = mem_trait_s> : base_claim_s<T> {};
      buffer data_buf <struct TRAIT = mem_trait_s, struct CLAIM = mem_segment_s<TRAIT>> {
         rand CLAIM mem_seg;
         rand bit[64] size;
      };
    "#;
    let m = compile_pss(src).expect("nested template defaults should parse");
    assert!(m.types.keys().any(|k| k.ends_with("data_buf")));
}

// ---------------------------------------------------------------------------
// Completion tests: pools, arbitration, match/select, arithmetic, addresses,
// and the realization backends.
// ---------------------------------------------------------------------------

const FLOW_MODEL: &str = r#"
package p { buffer data_buf { rand bit[16] size; constraint size >= 1; }; };
component P { action produce { output data_buf out; rand int in [1..64] size; constraint out.size == size; }; };
component Q { action consume { input data_buf in; rand int in [1..9] n; }; };
component top {
  P p; Q q;
  pool data_buf buf_p;
  bind buf_p *;
  exec init_up { mem_region.addr = 0x400; mem_region.size = 4096; };
  action t { activity { do P::produce; do Q::consume; do Q::consume; }; };
};
"#;

#[test]
fn pools_allocate_and_consume_flow_objects() {
    let m = compile_pss(FLOW_MODEL).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    let produced = s.steps[0].flow.iter().find(|f| f.direction == "output").unwrap();
    assert_eq!(produced.pool.as_deref(), Some("buf_p"));
    assert_eq!(produced.obj_id, Some(0));
    // Both consumers read the same object.
    for st in &s.steps[1..] {
        let c = st.flow.iter().find(|f| f.direction == "input").unwrap();
        assert_eq!(c.obj_id, Some(0));
    }
    assert!(s.pools.iter().any(|(n, _, c)| n == "buf_p" && *c == 1));
}

#[test]
fn address_regions_are_discovered_and_allocated() {
    let m = compile_pss(FLOW_MODEL).unwrap();
    assert_eq!(m.addr_regions.len(), 1);
    assert_eq!(m.addr_regions[0].base, 0x400);
    let s = engine::run_scenario(&m, "t", 1);
    let addr = s.steps[0].flow[0].addr.unwrap();
    assert_eq!(addr, 0x400, "first object sits at the region base");
}

const ARB_MODEL: &str = r#"
resource exec_r {};
component top {
  action a { lock exec_r e; rand int in [1..9] x; };
  action t { activity { parallel { do a; do a; }; }; };
};
"#;

#[test]
fn lock_claims_are_recorded_and_contention_reported() {
    let m = compile_pss(ARB_MODEL).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    assert!(s.steps.iter().all(|st| !st.claims.is_empty()));
    assert!(s.steps.iter().all(|st| st.claims[0].exclusive));
    // Two exclusive claims on a single-instance resource in one parallel
    // group is a contention.
    assert!(
        s.warnings.iter().any(|w| w.contains("contention")),
        "expected a contention warning, got {:?}",
        s.warnings
    );
}

#[test]
fn match_and_select_guards_choose_consistently() {
    let src = r#"
component C {
  action A { rand int in [0..9] q; };
  action B { rand int in [0..9] q; };
  action D { rand int in [0..9] q; };
  action t {
    rand int in [1..3] mode;
    activity {
      match (mode) { [1..1]: do A; [2..2]: do B; default: do D; };
      select { (mode == 1): do A; (mode == 2): do B; : do D; };
    };
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in [1u64, 2, 5, 9] {
        let s = engine::run_scenario(&m, "t", seed);
        assert_eq!(s.steps.len(), 2);
        assert_eq!(
            s.steps[0].action, s.steps[1].action,
            "match and select must agree (seed {})",
            seed
        );
    }
}

#[test]
fn arithmetic_constraints_are_solved() {
    let src = r#"
component C {
  action a {
    rand int in [0..100] x;
    rand int in [0..100] y;
    constraint x + y == 100;
    constraint x >= 20;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in [1u64, 4, 7] {
        let s = engine::run_scenario(&m, "a", seed);
        let x = s.steps[0].fields.iter().find(|(k, _)| k == "x").unwrap().1;
        let y = s.steps[0].fields.iter().find(|(k, _)| k == "y").unwrap().1;
        assert_eq!(x + y, 100, "x + y == 100 must hold (seed {})", seed);
        assert!(x >= 20);
    }
}

#[test]
fn realization_backends_are_registered_and_emit() {
    let names = satya::backend::names();
    for want in ["uvm", "c", "fw", "bench", "json", "trace"] {
        assert!(names.contains(&want), "backend {} missing", want);
    }
    let m = compile_pss(FLOW_MODEL).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    let opts = BackendOptions::default();

    let c = get("c").unwrap().emit(&m, &s, &opts).unwrap();
    assert!(c.contains("int main(void)"));
    assert!(c.contains("produce_run"));
    assert!(c.contains("0x400") || c.contains("address map"));

    let fw = get("fw").unwrap().emit(&m, &s, &opts).unwrap();
    assert!(fw.contains("## Interface map"));
    assert!(fw.contains("## Address map"));
    assert!(fw.contains("#define"));

    let bench = get("bench").unwrap().emit(&m, &s, &opts).unwrap();
    assert!(bench.contains("class Transport"));
    assert!(bench.contains("\"op\": \"write\""));
}

#[test]
fn uvm_backend_translates_exec_body() {
    let src = r#"
component C {
  action a {
    rand int in [1..9] x;
    exec body { x = x + 1; };
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let sv = get("uvm").unwrap().emit(&m, &s, &BackendOptions::default()).unwrap();
    assert!(sv.contains("exec body (translated"));
    assert!(sv.contains("x = (x + 1);"));
}

// ---------------------------------------------------------------------------
// Name resolution, enforcement and remaining grammar.
// ---------------------------------------------------------------------------

#[test]
fn qualified_names_resolve_to_distinct_types() {
    let src = r#"
package A { buffer data_buf { rand int in [1..9] a; }; };
package B { buffer data_buf { rand int in [10..20] b; }; };
component C { action t { rand int in [1..9] q; }; };
component top {
  C c;
  pool A::data_buf pa;
  pool B::data_buf pb;
  bind pa *;
  bind pb *;
  action test { activity { do C::t; }; };
};
"#;
    let m = compile_pss(src).unwrap();
    assert_eq!(m.types.len(), 2, "colliding leaf types must stay distinct");
    assert!(m.qual_index.contains_key("A::data_buf"));
    assert!(m.qual_index.contains_key("B::data_buf"));
    assert!(m.is_ambiguous("data_buf"));
    assert!(m.find_action(Some("C"), "t").is_some());
    assert_eq!(m.resolve("C::t"), "t");
}

#[test]
fn extend_struct_in_another_package_merges() {
    let src = r#"
package A { struct trait_s { rand int a; }; };
package B { extend struct trait_s { rand int b; }; };
"#;
    let m = compile_pss(src).unwrap();
    assert_eq!(m.types.len(), 1, "extend must merge, not duplicate");
    let t = m.types.values().next().unwrap();
    assert_eq!(t.fields.len(), 2, "the extension's field must be merged in");
    assert!(!m.is_ambiguous("trait_s"));
}

#[test]
fn contention_is_enforced_by_serialising() {
    let src = r#"
resource exec_r {};
component top {
  action a { lock exec_r e; rand int in [1..9] x; };
  action t { activity { parallel { do a; do a; }; }; };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    assert!(s.steps.iter().all(|st| st.group == 0));
    assert!(s.steps.iter().all(|st| st.mode == satya::engine::StepMode::Sequential));
    assert!(s.warnings.iter().any(|w| w.contains("serialised")));
}

#[test]
fn pool_capacity_is_enforced() {
    let src = r#"
package p { buffer data_buf { rand bit[16] size; constraint size >= 1; }; };
component P { action produce { output data_buf out; rand int in [1..64] size; constraint out.size == size; }; };
component top {
  P p;
  pool [1] data_buf buf_p;
  bind buf_p *;
  action t { activity { do P::produce; do P::produce; }; };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    assert!(s.pools.iter().any(|(n, _, c)| n == "buf_p" && *c == 1));
    assert_eq!(s.steps[1].flow[0].obj_id, None, "over-capacity allocation must be refused");
    assert!(s.warnings.iter().any(|w| w.contains("exhausted")));
}

#[test]
fn match_open_range_list_matches_values_and_ranges() {
    // Force `mode` so the chosen branch is deterministic.
    let model_for = |mode: i64| {
        format!(
            r#"
component C {{
  action A {{ rand int in [0..9] q; }};
  action B {{ rand int in [0..9] q; }};
  action D {{ rand int in [0..9] q; }};
  action t {{
    rand int in [1..5] mode;
    constraint mode == {mode};
    activity {{ match (mode) {{ [1, 3..5]: do A; 2: do B; default: do D; }}; }};
  }};
}};
"#,
            mode = mode
        )
    };
    // mode 2 -> B; 1, 3, 4, 5 -> A (values and range in one list).
    for (mode, want) in [(2i64, "B"), (1, "A"), (3, "A"), (4, "A"), (5, "A")] {
        let m = compile_pss(&model_for(mode)).unwrap();
        let s = engine::run_scenario(&m, "t", 1);
        assert_eq!(s.steps[0].action, want, "mode {} should select {}", mode, want);
    }
}

// ---------------------------------------------------------------------------
// Constructs found by running the PSS v3.0 LRM's own examples.
// ---------------------------------------------------------------------------

#[test]
fn sized_literals_and_aggregate_literals_both_lex() {
    // `4'b1010` is a sized literal; `'{...}` is an aggregate literal.
    let m = compile_pss("action a { rand bit[4] x = 4'b1010; exec body { x = 1; }; };").unwrap();
    assert!(m.actions.contains_key("a"));
    let m = compile_pss("struct s { int a; }; action a { s v; exec body { v = '{1}; }; };").unwrap();
    assert!(m.actions.contains_key("a"));
}

#[test]
fn constant_template_arguments_parse() {
    let src = "component C { array<bit[15:0], 5> foo_arr; sized_addr_handle_s<32> h; action a { rand int in [1..9] x; }; };";
    let m = compile_pss(src).unwrap();
    assert!(m.components.contains_key("C"));
}

#[test]
fn ref_types_and_import_aliases_parse() {
    let src = r#"
package P1 { package P2 { struct s { rand int a; }; }; };
component C {
  import P1::P2 as P2;
  ref C close_sibling;
  list<ref C> all_siblings;
  action a { rand int in [1..9] x; };
};
"#;
    let m = compile_pss(src).unwrap();
    assert!(m.components.contains_key("C"));
}

#[test]
fn pure_component_and_compile_assert_parse() {
    let src = r#"
pure component reg_c { action a { rand int in [1..9] x; }; };
compile assert (1 == 1);
"#;
    let m = compile_pss(src).unwrap();
    assert!(m.components.contains_key("reg_c"));
}

#[test]
fn open_ended_ranges_are_solved() {
    // `in [1..]` means "at least 1".
    let m = compile_pss("component C { action a { rand int in [1..] y; }; };").unwrap();
    for seed in [1u64, 5, 9] {
        let s = engine::run_scenario(&m, "a", seed);
        let y = s.steps[0].fields.iter().find(|(k, _)| k == "y").map(|(_, v)| *v).unwrap();
        assert!(y >= 1, "y must be >= 1, got {}", y);
    }
}

#[test]
fn triple_quoted_strings_lex() {
    let src = "action a { exec body { x = \"\"\"line one\nline two\"\"\"; }; };";
    let m = compile_pss(src).unwrap();
    assert!(m.actions.contains_key("a"));
}

// ---------------------------------------------------------------------------
// Found by modelling a real IP (OpenTitan UART).
// ---------------------------------------------------------------------------

#[test]
fn inline_range_on_a_struct_field_is_a_constraint() {
    // `rand int in [0..7] stop_bits;` inside a buffer must bound the field.
    let src = r#"
package p {
  buffer frame_b { rand bit[8] data; rand int in [0..7] stop_bits; };
};
component C { action write { output frame_b f; }; };
"#;
    let m = compile_pss(src).unwrap();
    for seed in [1u64, 4, 9, 17] {
        let s = engine::run_scenario(&m, "write", seed);
        let sb = s.steps[0].fields.iter().find(|(k, _)| k == "f.stop_bits").map(|(_, v)| *v).unwrap();
        assert!((0..=7).contains(&sb), "stop_bits must be in [0..7], got {}", sb);
    }
}

#[test]
fn implications_are_enforced_not_just_checked() {
    let src = r#"
component C {
  action a {
    rand bit[1] p_enable;
    rand bit[1] p_odd;
    constraint p_odd == 1;
    constraint p_odd == 1 -> p_enable == 1;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in [1u64, 3, 8, 21] {
        let s = engine::run_scenario(&m, "a", seed);
        let en = s.steps[0].fields.iter().find(|(k, _)| k == "p_enable").map(|(_, v)| *v).unwrap();
        assert_eq!(en, 1, "the implication must force p_enable to 1");
        assert_eq!(s.steps[0].satisfied, s.steps[0].total);
    }
}

#[test]
fn generated_c_orders_dependent_types() {
    // A struct containing another struct must emit the inner typedef first,
    // or the generated C will not compile.
    let src = r#"
package p {
  struct inner_s { rand bit[8] x; };
  buffer outer_b { rand inner_s i; rand int in [0..3] n; };
};
component C { action a { output outer_b o; }; };
"#;
    let m = compile_pss(src).unwrap();
    let sc = engine::run_scenario(&m, "a", 1);
    let out = get("c").unwrap().emit(&m, &sc, &BackendOptions::default()).unwrap();
    let inner = out.find("} inner_s_t;").expect("inner typedef emitted");
    let outer = out.find("} outer_b_t;").expect("outer typedef emitted");
    assert!(inner < outer, "inner_s_t must be defined before outer_b_t");
}

// ---------------------------------------------------------------------------
// Found by modelling OpenTitan's AES (multiregs, shadowed writes, wide fields).
// ---------------------------------------------------------------------------

#[test]
fn part_select_parses_and_evaluates() {
    // PSS 8.6.1: `identifier[msb:lsb]`.
    let src = r#"
component C {
  action a {
    rand bit[32] v;
    rand bit[32] c;
    constraint v == 0x12348d01;
    constraint v[15:0] == 0x8d01;
    constraint c == ~v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    assert_eq!(s.steps[0].satisfied, s.steps[0].total, "all constraints must hold");
    let v = s.steps[0].fields.iter().find(|(k, _)| k == "v").map(|(_, x)| *x).unwrap();
    assert_eq!(v, 0x12348d01);
}

#[test]
fn bitwise_complement_solves_at_field_width() {
    // `~x` on a bit[32] is a 32-bit value, not a 64-bit one.
    let src = r#"
component C {
  action a {
    rand bit[32] v;
    rand bit[32] c;
    constraint v == 0x0000000f;
    constraint c == ~v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let c = s.steps[0].fields.iter().find(|(k, _)| k == "c").map(|(_, x)| *x).unwrap();
    assert_eq!(c, 0xfffffff0, "32-bit complement expected, got {:#x}", c);
    assert_eq!(s.steps[0].satisfied, s.steps[0].total);
}

#[test]
fn bit_width_domains_are_not_truncated() {
    // A bit[32] field must be able to hold values above 2^20.
    let src = "component C { action a { rand bit[32] v; constraint v >= 3000000000; }; };";
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let v = s.steps[0].fields.iter().find(|(k, _)| k == "v").map(|(_, x)| *x).unwrap();
    assert!(v >= 3_000_000_000, "bit[32] must not be capped at 20 bits, got {}", v);
}
