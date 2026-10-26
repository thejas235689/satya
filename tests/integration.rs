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

// ---------------------------------------------------------------------------
// Default-constraint semantics (PSS 16.1.10.1) and variable-variable ordering.
// ---------------------------------------------------------------------------

/// The LRM's Example 133, with its stated expected results.
#[test]
fn default_constraint_precedence_matches_the_lrm() {
    let src = r#"
package p {
  struct my_struct {
    rand int in [0..3] attr1;
    constraint default attr1 == 0;        // (1)
    rand int in [0..3] attr2;
    constraint attr1 < attr2;             // (2)
  };
  component C {
    action my_action {
      rand my_struct s1;
      rand my_struct s2;
      constraint default s2.attr1 == 2;      // (3) overrides (1) for s2
      rand my_struct s3;
      constraint default disable s3.attr1;   // (4)
      constraint s3.attr1 > 0;               // (5)
    };
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in [1u64, 2, 3, 4, 5, 6, 7, 8] {
        let s = engine::run_scenario(&m, "my_action", seed);
        let step = &s.steps[0];
        assert_eq!(step.satisfied, step.total, "seed {seed}: {:?}", step.unsatisfied);
        let get = |n: &str| step.fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
        // s1 takes the type-level default.
        assert_eq!(get("s1.attr1"), 0, "seed {seed}");
        // s2 takes the action-level default, which overrides the type's.
        assert_eq!(get("s2.attr1"), 2, "seed {seed}");
        // s3's default was disabled, so only `> 0` applies: domain 1..2.
        let s3 = get("s3.attr1");
        assert!((1..=2).contains(&s3), "seed {seed}: s3.attr1 = {s3}");
        // `attr1 < attr2` holds everywhere.
        for p in ["s1", "s2", "s3"] {
            assert!(get(&format!("{p}.attr1")) < get(&format!("{p}.attr2")), "seed {seed}: {p}");
        }
    }
}

#[test]
fn default_disable_removes_the_default() {
    let src = r#"
package p {
  struct S { rand int in [0..3] a; constraint default a == 0; };
  component C {
    action with_disable { rand S s; constraint default disable s.a; constraint s.a > 0; };
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in [1u64, 2, 3] {
        let s = engine::run_scenario(&m, "with_disable", seed);
        let a = s.steps[0].fields.iter().find(|(k, _)| k == "s.a").map(|(_, v)| *v).unwrap();
        assert!(a >= 1, "default was not disabled (seed {seed}, a = {a})");
        assert_eq!(s.steps[0].satisfied, s.steps[0].total);
    }
}

#[test]
fn a_default_conflicting_with_a_hard_constraint_is_reported() {
    // The LRM's Example 134: `default attr2 == -1` and `attr2 == 0` cannot both
    // hold, and the tool must say so rather than silently pick one.
    let src = r#"
package p {
  component C {
    action my_action {
      rand int attr1;
      constraint default attr1 == -1;
      rand int attr2;
      constraint default attr2 == -1;
    };
    action my_derived_action : my_action {
      constraint {
        default disable attr1;
        attr1 == 0;
      }
      constraint attr2 == 0;
    };
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "my_derived_action", 1);
    let step = &s.steps[0];
    let attr1 = step.fields.iter().find(|(k, _)| k == "attr1").map(|(_, v)| *v).unwrap();
    assert_eq!(attr1, 0, "the disabled default must not force attr1 to -1");
    assert!(
        step.satisfied < step.total,
        "the attr2 contradiction must be reported"
    );
}

#[test]
fn ordering_between_two_random_variables_is_solved() {
    let src = r#"
component C {
  action a {
    rand int in [0..3] lo;
    rand int in [0..3] hi;
    constraint lo < hi;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in 1u64..=12 {
        let s = engine::run_scenario(&m, "a", seed);
        let step = &s.steps[0];
        let lo = step.fields.iter().find(|(k, _)| k == "lo").map(|(_, v)| *v).unwrap();
        let hi = step.fields.iter().find(|(k, _)| k == "hi").map(|(_, v)| *v).unwrap();
        assert!(lo < hi, "seed {seed}: lo = {lo}, hi = {hi}");
        assert_eq!(step.satisfied, step.total);
    }
}

// ---------------------------------------------------------------------------
// Semantics implemented in 0.6.0: join specs, replicate, dist, exec ordering,
// flow-object ordering, arrays and coverage.
// ---------------------------------------------------------------------------

fn step_names(s: &engine::Scenario) -> Vec<String> {
    s.steps.iter().map(|x| x.action.clone()).collect()
}

#[test]
fn join_none_removes_the_scheduling_dependency() {
    let src = r#"
component C {
  action A {} action B {} action Cc {};
  action t { activity { L1: parallel join_none { L2: do A; L3: do B; } L4: do Cc; } };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    // No dependency: the following statement is scheduled first.
    assert_eq!(step_names(&s)[0], "Cc", "{:?}", step_names(&s));
}

#[test]
fn join_branch_waits_only_for_the_named_branch() {
    let src = r#"
component C {
  action A {} action B {} action Cc {} action D {};
  action t {
    activity {
      L1: parallel join_branch(L2) { L2: do A; L3: do B; }
      L4: do Cc;
      L5: do D;
    }
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    let names = step_names(&s);
    let pos = |n: &str| names.iter().position(|x| x == n).unwrap();
    // Cc waits for L2 (A); B does not gate it.
    assert!(pos("A") < pos("Cc"), "{names:?}");
    assert!(pos("Cc") < pos("D"), "{names:?}");
}

#[test]
fn join_select_waits_for_n_branches() {
    let src = r#"
component C {
  action A {} action B {} action Cc {};
  action t { activity { L1: parallel join_select(1) { L2: do A; L3: do B; } L4: do Cc; } };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in 1u64..=8 {
        let s = engine::run_scenario(&m, "t", seed);
        let names = step_names(&s);
        let pos = |n: &str| names.iter().position(|x| x == n).unwrap();
        // Exactly one branch precedes the following statement.
        let before = ["A", "B"].iter().filter(|n| pos(n) < pos("Cc")).count();
        assert_eq!(before, 1, "seed {seed}: {names:?}");
    }
}

#[test]
fn replicate_produces_labelled_copies() {
    let src = r#"
component C {
  action A {};
  action t { activity { replicate (3) R: do A; } };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    assert_eq!(s.steps.len(), 3);
    assert!(s.steps.iter().all(|x| x.replicated));
    assert!(s.steps.iter().all(|x| x.label.as_deref() == Some("R")));
}

#[test]
fn dist_weights_bias_the_value_distribution() {
    let src = r#"
component C {
  action a { rand int x; constraint dist x in [100..102 := 1, 200 := 2, 300 := 5]; };
};
"#;
    let m = compile_pss(src).unwrap();
    let mut counts = std::collections::HashMap::new();
    for seed in 1u64..=400 {
        let s = engine::run_scenario(&m, "a", seed);
        let x = s.steps[0].fields.iter().find(|(k, _)| k == "x").map(|(_, v)| *v).unwrap();
        *counts.entry(x).or_insert(0usize) += 1;
    }
    // `:=` gives each value in the range the full weight: 1,1,1,2,5 over 10.
    let n = |v: i64| *counts.get(&v).unwrap_or(&0) as f64 / 400.0;
    assert!(n(300) > n(200) && n(200) > n(100), "{counts:?}");
    assert!((n(300) - 0.5).abs() < 0.12, "300 should be ~50%: {}", n(300));
    assert!((n(100) - 0.1).abs() < 0.08, "100 should be ~10%: {}", n(100));
}

#[test]
fn exec_pre_and_post_solve_run_around_the_solver() {
    let src = r#"
component C {
  action a {
    rand int r;
    int limit;
    int derived;
    exec pre_solve  { limit = 12; }
    exec post_solve { derived = r + limit; }
    constraint r in [0..30];
    constraint r > limit;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in 1u64..=5 {
        let s = engine::run_scenario(&m, "a", seed);
        let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
        assert_eq!(get("limit"), 12, "pre_solve value must survive");
        assert_eq!(get("derived"), get("r") + 12, "post_solve must use the solved value");
    }
}

#[test]
fn flow_object_ordering_is_checked() {
    let src = r#"
package p { buffer buf { rand int d; }; };
component C {
  action prod { output buf b; };
  action cons { input buf b; };
  action ok   { activity { do prod; do cons; } };
  action bad  { activity { do cons; do prod; } };
};
"#;
    let m = compile_pss(src).unwrap();
    assert!(engine::run_scenario(&m, "ok", 1).warnings.is_empty());
    assert!(engine::run_scenario(&m, "bad", 1)
        .warnings
        .iter()
        .any(|w| w.contains("flow-object ordering")));
}

#[test]
fn fixed_size_arrays_expand_into_elements() {
    let src = r#"
component C {
  action a {
    rand bit[8] arr[4];
    rand array<bit[8], 3> av;
    rand int total;
    constraint arr[0] == 11;
    constraint av[2] == 99;
    constraint total == arr.sum();
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("arr.size"), 4);
    assert_eq!(get("av.size"), 3);
    assert_eq!(get("arr[0]"), 11);
    assert_eq!(get("av[2]"), 99);
    assert_eq!(get("total"), get("arr[0]") + get("arr[1]") + get("arr[2]") + get("arr[3]"));
}

#[test]
fn foreach_iterates_array_elements() {
    let src = r#"
component C {
  action A { rand int in [1..9] v; };
  action a {
    rand array<int, 3> items;
    activity { foreach (items[i]) { do A; } }
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    assert_eq!(s.steps.len(), 3, "foreach must iterate the three elements");
}

#[test]
fn coverpoints_are_sampled_inside_a_cover_block() {
    let src = r#"
package p { enum color_e {red, green, blue}; };
component C {
  action gen {
    rand color_e color;
    covergroup { c: coverpoint color; } cs;
  };
  action t { activity { L: cover { activity { do gen; do gen; do gen; } } }; };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    assert!(!s.coverage.is_empty(), "coverage must be collected");
    let cp = &s.coverage[0];
    assert!(cp.name.ends_with(".c"), "{}", cp.name);
    let total: usize = cp.values.iter().map(|(_, c)| *c).sum();
    assert_eq!(total, 3, "three observed instances");
}

#[test]
fn array_operators_are_solved() {
    let src = r#"
component C {
  action a {
    rand bit[8] x[4];
    rand bit[8] y[4];
    rand bit[1] in_it;
    rand int s;
    constraint x[0] == 5; constraint x[1] == 6;
    constraint x[2] == 7; constraint x[3] == 8;
    constraint y == x;                 // element-wise equality
    constraint in_it == (7 in x);      // set membership
    constraint s == x.sum;             // deprecated property form
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let step = &s.steps[0];
    let get = |n: &str| step.fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("in_it"), 1);
    assert_eq!(get("s"), 26);
    for i in 0..4 {
        assert_eq!(get(&format!("y[{i}]")), get(&format!("x[{i}]")), "element {i}");
    }
    assert_eq!(step.satisfied, step.total);
}

#[test]
fn list_mutators_in_exec_build_a_collection() {
    let src = r#"
component C {
  action a {
    list<int> q;
    exec pre_solve {
      q.push_back(10);
      q.push_back(20);
      q.push_back(30);
    }
    rand int v;
    constraint v in q;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("q.size"), 3);
    assert_eq!(get("q[0]"), 10);
    assert_eq!(get("q[2]"), 30);
    assert!(get("v") == 10 || get("v") == 20 || get("v") == 30, "v must be an element");
}

#[test]
fn aggregate_literal_assignment_in_init_up() {
    // The LRM's own form (Example 269): a component attribute initialised by
    // `exec init_up`, then read by an action's constraint.
    let src = r#"
package p { enum op_mode_e {rx, tx, copy}; };
component C {
  list<op_mode_e> modes;
  exec init_up { modes = {rx, tx}; }
  action a {
    rand op_mode_e m;
    constraint m in modes;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("modes.size"), 2);
    assert!(get("m") == 0 || get("m") == 1, "m must be rx or tx");
}

#[test]
fn crosses_and_bins_are_sampled() {
    let src = r#"
package p { enum color_e {red, green, blue}; };
component C {
  action gen {
    rand color_e color;
    rand bit[4] pixel_offset;
    covergroup {
      c: coverpoint color;
      Offset: coverpoint pixel_offset {
        bins low  = {[0..3]};
        bins high = {[12..15]};
      }
      AxC: cross c, Offset;
    } cs;
  };
  action t { activity { L: cover { activity { do gen; do gen; do gen; do gen; } } }; };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 3);
    let find = |n: &str| s.coverage.iter().find(|c| c.name.ends_with(n));
    let cross = find(".AxC").expect("cross sampled");
    // Cross values are tuples, one per observed pair.
    assert!(cross.values.iter().all(|(v, _)| v.contains(',')), "{:?}", cross.values);
    let off = find(".Offset").expect("coverpoint sampled");
    let (hit, total) = off.bins.expect("bins declared");
    assert_eq!(total, 2, "two bins declared");
    assert!(hit <= total);
}

#[test]
fn cover_type_observes_the_enclosing_activity() {
    let src = r#"
package p { enum color_e {red, green, blue}; };
component C {
  action gen {
    rand color_e color;
    covergroup { c: coverpoint color; } cs;
  };
  action t {
    activity {
      L1: cover { activity { do gen; } };
      L2: cover color_mon;
      do gen;
      do gen;
    }
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    let cp = s.coverage.iter().find(|c| c.name.ends_with(".c")).expect("sampled");
    let total: usize = cp.values.iter().map(|(_, c)| *c).sum();
    // One from the cover block, two from the statements after `cover <type>`.
    assert_eq!(total, 3, "{:?}", cp.values);
}

// ---------------------------------------------------------------------------
// Procedural exec statements (Syntax 89-101) and the standard library
// (Syntax 106-116).
// ---------------------------------------------------------------------------

#[test]
fn exec_procedural_statements_run() {
    let src = r#"
component C {
  action a {
    list<int> q;
    int total;
    int first_big;
    exec pre_solve {
      repeat (4) { q.push_back(5); }
      total = 0;
      foreach (q[i]) {
        total = total + q[i];
        if (total > 10) { first_big = total; break; }
      }
      match (total) {
        20: { first_big = 99; }
        default: { first_big = total; }
      }
    }
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("q.size"), 4, "repeat must push four elements");
    assert_eq!(get("total"), 15, "foreach accumulates then breaks at 15");
    assert_eq!(get("first_big"), 15, "match default takes total");
}

#[test]
fn exec_repeat_while_runs() {
    let src = r#"
component C {
  action a {
    int n;
    exec pre_solve {
      n = 0;
      repeat { n = n + 1; } while (n < 5);
    }
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let n = s.steps[0].fields.iter().find(|(k, _)| k == "n").map(|(_, v)| *v).unwrap();
    assert_eq!(n, 5, "repeat/while must loop until the condition fails");
}

#[test]
fn float_pkg_functions_are_evaluated() {
    let src = r#"
component C {
  action a {
    int fv;
    int mant;
    int expo;
    int sign;
    int root;
    exec pre_solve {
      fv = to_float(0, 1023, 0);       // 1.0 in IEEE-754
      mant = float_mantissa(fv);
      expo = float_exponent(fv);
      sign = float_sign(fv);
      root = sqrt(fv);                 // sqrt(1.0) == 1.0
    }
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("fv"), 0x3FF0_0000_0000_0000, "1.0 in IEEE-754");
    assert_eq!(get("mant"), 0);
    assert_eq!(get("expo"), 1023);
    assert_eq!(get("sign"), 0);
    assert_eq!(get("root"), 0x3FF0_0000_0000_0000, "sqrt(1.0) == 1.0");
}

#[test]
fn messaging_functions_are_recorded() {
    let src = r#"
component C {
  action a {
    rand int in [4..9] n;
    exec post_solve {
      print("n is", n);
      error("always");
    }
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    assert!(s.messages.iter().any(|m| m.starts_with("INFO: n is ")), "{:?}", s.messages);
    assert!(s.messages.iter().any(|m| m.starts_with("ERROR:")), "{:?}", s.messages);
}

#[test]
fn urandom_range_varies_by_seed_and_stays_in_range() {
    let src = r#"
component C {
  action a {
    int r;
    exec pre_solve { r = urandom_range(1, 6); }
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let mut seen = std::collections::HashSet::new();
    for seed in 1u64..=40 {
        let s = engine::run_scenario(&m, "a", seed);
        let r = s.steps[0].fields.iter().find(|(k, _)| k == "r").map(|(_, v)| *v).unwrap();
        assert!((1..=6).contains(&r), "seed {seed}: r = {r}");
        seen.insert(r);
    }
    assert!(seen.len() > 1, "urandom_range must vary across seeds");
}

#[test]
fn value_list_literal_and_enum_list_membership() {
    // PSS Syntax 5: `[1, 2, 3]`, and a list of enum items.
    let src = r#"
package p { enum e_t {a, b, c}; };
component C {
  action a {
    rand int sel;
    rand e_t v;
    rand int cond;
    constraint sel in [1, 2, 3];
    constraint v in [a, b];
    constraint cond == sel ? v == a : v == b;   // Syntax 20
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in 1u64..=12 {
        let s = engine::run_scenario(&m, "a", seed);
        let step = &s.steps[0];
        let get = |n: &str| step.fields.iter().find(|(k, _)| k == n).map(|(_, x)| *x).unwrap();
        assert!((1..=3).contains(&get("sel")), "seed {seed}: sel = {}", get("sel"));
        assert!(get("v") == 0 || get("v") == 1, "seed {seed}: v must be a or b");
        assert_eq!(step.satisfied, step.total, "seed {seed}: {:?}", step.unsatisfied);
    }
}

// ---------------------------------------------------------------------------
// Monitors (Syntax 68-78).
// ---------------------------------------------------------------------------

const MONITORS: &str = r#"
package p {
  enum core_e {c0, c1};
  component C {
    action start {};
    action nop {};
    action write { rand core_e core; };
    action read  { rand core_e core; };
    monitor m_concat {
      write w;
      activity { concat { do start; w; do read; } }
      covergroup { c: coverpoint w.core; } cc;
    };
    monitor m_sequence {
      write w;
      activity { sequence { do start; w; do read; } }
      covergroup { c: coverpoint w.core; } cs;
    };
    monitor m_eventually {
      write w;
      activity { eventually { w; } }
      covergroup { c: coverpoint w.core; } ce;
    };
    monitor m_select {
      activity { select { 1: { do start; } default: { do read; } } }
      covergroup { c: coverpoint 1; } csel;
    };
    action t {
      activity {
        do start; do nop; do write; do read;
        cover m_concat;
        cover m_sequence;
        cover m_eventually;
        cover m_select;
      }
    };
  };
};
"#;

fn coverage_total(s: &engine::Scenario, name: &str) -> Option<usize> {
    s.coverage
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.values.iter().map(|(_, n)| *n).sum())
}

#[test]
fn concat_matches_across_a_gap_but_sequence_does_not() {
    let m = compile_pss(MONITORS).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    // `concat { start; write; read }` tolerates the `nop` between them, and the
    // realization is the three matched steps - the gap is not part of it.
    assert_eq!(coverage_total(&s, "cc.c"), Some(3), "{:?}", s.coverage);
    // `sequence` requires contiguity, so the same pattern must not match.
    assert_eq!(coverage_total(&s, "cs.c"), None, "sequence must not match across a gap");
}

#[test]
fn eventually_finds_a_later_match() {
    let m = compile_pss(MONITORS).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    // `eventually { w; }` matches the single write step.
    assert_eq!(coverage_total(&s, "ce.c"), Some(1), "{:?}", s.coverage);
}

#[test]
fn monitor_select_takes_the_first_matching_arm() {
    let m = compile_pss(MONITORS).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    // Guard `1` is true, so the `start` arm is taken and sampled once.
    assert_eq!(coverage_total(&s, "csel.c"), Some(1), "{:?}", s.coverage);
}

#[test]
fn cover_of_an_unknown_monitor_falls_back_to_the_enclosing_activity() {
    let src = r#"
component C {
  action gen { rand int in [0..3] v; covergroup { c: coverpoint v; } cs; };
  action t { activity { cover no_such_monitor; do gen; do gen; } };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    assert_eq!(coverage_total(&s, "cs.c"), Some(2), "{:?}", s.coverage);
}

// ---------------------------------------------------------------------------
// Registers and address-space sizing (Syntax 132, 145).
// ---------------------------------------------------------------------------

#[test]
fn sizeof_s_reports_the_type_width() {
    let src = r#"
package p {
  struct ctrl_s { bit[1] en; bit[3] mode; bit[4] rsvd; };
  component C {
    action a {
      int bits;
      int bytes;
      exec pre_solve {
        bits  = sizeof_s<ctrl_s>::nbits;    // 1 + 3 + 4
        bytes = sizeof_s<ctrl_s>::nbytes;
      }
      rand int v;
    };
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("bits"), 8);
    assert_eq!(get("bytes"), 1);
}

#[test]
fn register_access_functions_follow_the_standard() {
    // `reg_c<ctrl_s, READWRITE, 8>` — fields pack little-endian from the LSB
    // (PSS 24.10.1), and write_masked is (cur & ~mask) | (val & mask).
    let src = r#"
package p {
  struct ctrl_s { bit[1] en; bit[3] mode; bit[4] rsvd; };
  pure component ctrl_reg_c : reg_c<ctrl_s, READWRITE, 8> {};
  component C {
    ctrl_reg_c ctrl;
    action a {
      int v0; int v1; int v2; int v3; int v4;
      exec pre_solve {
        ctrl.write(0x00);
        ctrl.write_field("mode", 5);      // mode is bits [1..3]
        v0 = ctrl.read();
        ctrl.write_masked(0x0F, 0xF0);    // low nibble cleared by the mask
        v1 = ctrl.read();
        ctrl.write_field("en", 1);        // en is bit 0
        v2 = ctrl.read();
        ctrl.write_val(0xFF);
        v3 = ctrl.read_val();
        ctrl.write_field("mode", 9);      // 9 is masked into the 3-bit field
        v4 = ctrl.read();
      }
      rand int x;
    };
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("v0"), 0b1010, "mode=5 at offset 1");
    assert_eq!(get("v1"), 0, "write_masked(0x0F, 0xF0) on 0x0A clears the low nibble");
    assert_eq!(get("v2"), 1, "en is bit 0");
    assert_eq!(get("v3"), 0xFF, "write_val/read_val round-trip");
    // 0xFF with mode replaced by (9 & 7) << 1 == 0b010
    assert_eq!(get("v4"), 0xF3, "field write is masked to the field width");
}

// ---------------------------------------------------------------------------
// Address handles and the memory model (Syntax 133-142).
// ---------------------------------------------------------------------------

const ADDR_MODEL: &str = r#"
package p {
  component C {
    exec init_up {
      mem_region.addr = 0x400;
      mem_region.size = 4096;
    }
    action a {
      addr_handle_t h0; addr_handle_t h1; addr_handle_t h2;
      int addr0; int addr1; int addr2; int abs0;
      int r32; int r8; int r16; int r64;
      int tag0;
      exec pre_solve {
        h0 = make_handle_from_claim(mem_region, 0x10);
        h1 = make_handle_from_handle(h0, 4);
        h2 = make_handle_from_claim(mem_region, 0x20);
        addr0 = addr_value(h0);
        addr1 = addr_value_solve(h1);
        addr2 = addr_value(h2);
        abs0  = addr_value_abs(h0);
        tag0  = get_tag(h0);
        write32(h0, 0xDEADBEEF);
        r32 = read32(h0);
        r8  = read8(h0);                 // the low byte, bits [7:0]
        write16(h1, 0x1234);
        r16 = read16(h1);
        write64(h2, 0x0102030405060708);
        r64 = read64(h2);
      }
      rand int x;
    };
  };
};
"#;

#[test]
fn address_handles_carry_absolute_addresses() {
    let m = compile_pss(ADDR_MODEL).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("addr0"), 0x410, "0x400 + 0x10");
    assert_eq!(get("addr1"), 0x414, "handle plus an offset");
    assert_eq!(get("addr2"), 0x420);
    assert_eq!(get("abs0"), 1, "a region of an address space is transparent");
}

#[test]
fn memory_primitives_are_little_endian() {
    let m = compile_pss(ADDR_MODEL).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("r32"), 0xDEADBEEF);
    // The first byte written goes into bits [7:0] (PSS 24.10.9.1).
    assert_eq!(get("r8"), 0xEF);
    assert_eq!(get("r16"), 0x1234);
    assert_eq!(get("r64"), 0x0102030405060708);
    // Each primitive round-trips through the byte-addressable store.
    let mem: Vec<(String, i64)> = s.steps[0]
        .fields
        .iter()
        .filter(|(k, _)| k.starts_with("__mem."))
        .cloned()
        .collect();
    let at = |a: i64| mem.iter().find(|(k, _)| k == &format!("__mem.{}", a)).map(|(_, v)| *v);
    assert_eq!(at(0x410), Some(0xEF), "low byte at the starting address");
    assert_eq!(at(0x411), Some(0xBE));
}

// ---------------------------------------------------------------------------
// Statistical and ordering checks for the distribution directive and the exec
// block sequence.
// ---------------------------------------------------------------------------

#[test]
fn dist_is_uniform_without_weights_and_weighted_with_them() {
    // The standard: an item with no weight has the default weight `:= 1`, a
    // range with `:=` gives each value the full weight, and `:/` spreads it.
    let src = r#"
component C {
  action plain  { rand int x; constraint dist x in [10, 20, 30]; };
  action ranged { rand int y; constraint dist y in [1..4 := 1, 100 := 1]; };
  action weight { rand int z; constraint dist z in [100..102 := 1, 200 := 2, 300 := 5]; };
};
"#;
    let m = compile_pss(src).unwrap();
    let n = 20000u64;

    let counts = |act: &str| {
        let mut c: std::collections::BTreeMap<i64, usize> = std::collections::BTreeMap::new();
        for seed in 1..=n {
            let s = engine::run_scenario(&m, act, seed);
            *c.entry(s.steps[0].fields[0].1).or_insert(0) += 1;
        }
        c
    };
    let frac = |c: &std::collections::BTreeMap<i64, usize>, v: i64| {
        *c.get(&v).unwrap_or(&0) as f64 / n as f64
    };

    // Unweighted: three values, `:= 1` each, so uniform.
    let c = counts("plain");
    for v in [10, 20, 30] {
        assert!((frac(&c, v) - 1.0 / 3.0).abs() < 0.02, "plain {v}: {}", frac(&c, v));
    }

    // `[1..4 := 1, 100 := 1]` — each of five values carries weight 1.
    let c = counts("ranged");
    for v in [1, 2, 3, 4, 100] {
        assert!((frac(&c, v) - 0.2).abs() < 0.02, "ranged {v}: {}", frac(&c, v));
    }

    // `:=` gives every value in the range the full weight: 1,1,1,2,5 over 10.
    let c = counts("weight");
    for v in [100, 101, 102] {
        assert!((frac(&c, v) - 0.1).abs() < 0.02, "weight {v}: {}", frac(&c, v));
    }
    assert!((frac(&c, 200) - 0.2).abs() < 0.02, "weight 200: {}", frac(&c, 200));
    assert!((frac(&c, 300) - 0.5).abs() < 0.02, "weight 300: {}", frac(&c, 300));
}

#[test]
fn exec_blocks_run_in_the_order_the_standard_specifies() {
    // init_down / init_up are called before the root action's pre_solve (22.1);
    // pre_solve precedes solving, post_solve follows it, and pre_body is a
    // solve-platform block after post_solve and before the body (22.1).
    let src = r#"
component C {
  int log0; int log1; int log2; int log3;
  exec init_down { log0 = 1; }
  exec init_up   { log1 = 2; }
  action a {
    int x;
    exec pre_solve  { log2 = 3; }
    exec post_solve { log3 = 4; }
    exec pre_body   { x = 5; }
    exec body       { x = 6; }   // target code, not run at solve time
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
    assert_eq!(get("log0"), Some(1), "init_down");
    assert_eq!(get("log1"), Some(2), "init_up");
    assert_eq!(get("log2"), Some(3), "pre_solve");
    assert_eq!(get("log3"), Some(4), "post_solve");
    // pre_body runs on the solve platform; the body does not.
    assert_eq!(get("x"), Some(5), "pre_body, not body");
}

// ---------------------------------------------------------------------------
// Lexical and expression fixes found by the shared PSS corpus
// (github.com/psstools/pss-corpus).
// ---------------------------------------------------------------------------

#[test]
fn sized_literals_carry_their_value_not_their_width() {
    // A regression guard: `4'b1010` is 10, not 4. The size is a width hint;
    // before this was fixed the token carried the width and the digits were
    // silently dropped, so every sized literal in a model was wrong.
    let src = r#"
component C {
  action a {
    int b; int h; int d; int sd; int o;
    exec pre_solve {
      b  = 4'b1010;
      h  = 8'hFF;
      d  = 64'd2;
      sd = 4'sd12;      // signed
      o  = 3'o7;
    }
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("b"), 0b1010);
    assert_eq!(get("h"), 0xFF);
    assert_eq!(get("d"), 2);
    assert_eq!(get("sd"), 12);
    assert_eq!(get("o"), 0o7);
}

#[test]
fn an_open_range_list_mixes_values_and_ranges() {
    // PSS 8.5.9: the right-hand side of `in` is "a comma-separated list of
    // scalar value expressions or ranges".
    let src = r#"
component C {
  action a { rand int x; constraint x in [1, 3, 5..9]; };
};
"#;
    let m = compile_pss(src).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for seed in 1u64..=400 {
        let s = engine::run_scenario(&m, "a", seed);
        seen.insert(s.steps[0].fields[0].1);
    }
    let want: std::collections::BTreeSet<i64> = [1, 3, 5, 6, 7, 8, 9].into_iter().collect();
    assert_eq!(seen, want);
}

#[test]
fn the_power_operator_is_evaluated() {
    let src = r#"
component C {
  action a {
    int p; int q; int r;
    exec pre_solve {
      p = 2**3;
      q = 64'd2**5;
      r = 2**0;
    }
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("p"), 8);
    assert_eq!(get("q"), 32);
    assert_eq!(get("r"), 1);
}

#[test]
fn escaped_identifiers_and_qualified_extents_parse() {
    // Escaped identifiers (Syntax 4) and a qualified constant as an extent.
    let src = r#"
package p { static const int N = 4; };
component C {
  action \do-write { rand int v; };
  action t { activity { repeat (p::N) { do \do-write; } } };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    assert_eq!(s.steps.len(), 4, "p::N is 4");
}

#[test]
fn a_lone_quote_or_backslash_is_rejected() {
    // The corpus marks these as pathological: they must not parse.
    for src in ["component c { int x = 'ff; }", "component c { int x = 1; \\ }"] {
        assert!(compile_pss(src).is_err(), "should reject: {src}");
    }
}

// ---------------------------------------------------------------------------
// Defects found by running against other psstools projects: peakrdl-pss,
// pssc, pssfmt, sphinx-pss (see docs/CORPUS.md).
// ---------------------------------------------------------------------------

#[test]
fn sixty_four_bit_literals_keep_their_bit_pattern() {
    // A 64-bit value such as 0xFFFF_FFFF_FFFF_FFFF does not fit a signed
    // 64-bit integer. It is a bit pattern, so it is parsed unsigned and kept
    // as-is. Found by pssc's repro/instance-array-offsets.
    let src = r#"
component C {
  action a {
    int hex; int dec; int high;
    exec pre_solve {
      hex  = 0xFFFF_FFFF_FFFF_FFFF;
      dec  = 18446744073709551615;      // the same value in decimal
      high = 0x8000_0000_0000_0000;
    }
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("hex"), -1, "all ones");
    assert_eq!(get("dec"), -1, "the same value written in decimal");
    assert_eq!(get("high"), i64::MIN);
}

#[test]
fn nested_template_arguments_close_correctly() {
    // `nested_s<packed_s<bit, 8>>` — the `>>` ends two template lists. A
    // constant argument must not be parsed as a shift. Found by pssfmt.
    let src = r#"
package p {
  struct packed_s<type T, int W = 32> { T value; bit[W] raw; };
  struct outer_s {
    packed_s<bit[8], 4> small;
    packed_s<bit, 8> plain;
    nested_s<packed_s<bit, 8>> nested;
  };
  struct nested_s<type T> { T inner; };
};
"#;
    assert!(compile_pss(src).is_ok(), "nested template arguments must parse");
}

#[test]
fn a_scheduling_constraint_parses_and_is_recorded() {
    // `constraint parallel { a, b };` (PSS 16.2, Syntax 62). It used to swallow
    // the enclosing `}`. Found by sphinx-pss's activity fixtures.
    let src = r#"
component C {
  action x {};
  action a {
    activity {
      parallel { do x; }
      constraint parallel { s1, s2 };
      constraint sequence { s3, s4 };
    }
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    assert!(
        s.messages.iter().any(|m| m.contains("parallel {s1, s2}")),
        "{:?}",
        s.messages
    );
    assert!(s.messages.iter().any(|m| m.contains("sequence {s3, s4}")), "{:?}", s.messages);
}

// ---------------------------------------------------------------------------
// The constructs finished last: symbol, override, claims, struct access and
// target-code template validation.
// ---------------------------------------------------------------------------

#[test]
fn a_symbol_expands_in_place() {
    // PSS 12.7: `symbol s { ... }` is referenced as `s();` and has the same
    // effect as writing the fragment inline.
    let src = r#"
component C {
  action A {};
  symbol twice { do A; do A; };
  action t { activity { twice(); } };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    assert_eq!(s.steps.len(), 2, "the symbol's two traversals");
}

#[test]
fn an_override_replaces_the_traversed_type() {
    // PSS 20.5: `override { type A with B; }` makes every A a B.
    let src = r#"
component C {
  action base_a { rand int v; };
  action x_a : base_a { rand int extra; };
  action t { activity { do base_a; } };
  override { type base_a with x_a; }
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "t", 1);
    assert_eq!(s.steps[0].action, "x_a");
}

#[test]
fn an_address_claim_is_allocated_from_a_region() {
    // PSS 24.8: a claim struct instance is satisfied by allocation from a
    // region, so its address is visible to the action.
    let src = r#"
package p { struct my_trait_s : addr_trait_s { rand int core; }; };
component C {
  exec init_up { mem_region.addr = 0x400; mem_region.size = 4096; }
  action a {
    transparent_addr_claim_s<my_trait_s> claim;
    constraint claim.size == 64;
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
    assert_eq!(get("claim.size"), Some(64));
    assert_eq!(get("claim.addr"), Some(0x400), "allocated at the region base");
}

#[test]
fn packed_structs_round_trip_through_memory() {
    // PSS 24.10.9.4: write_struct lays the packed struct out little-endian, and
    // read_struct brings it back.
    let src = r#"
package p { struct ctl_s : packed_s<> { bit[1] en; bit[3] mode; bit[4] rsvd; }; };
component C {
  exec init_up { mem_region.addr = 0x400; mem_region.size = 4096; }
  action a {
    ctl_s s; ctl_s t;
    list<bit[8]> bytes;
    addr_handle_t h;
    int mode_back; int n;
    exec pre_solve {
      h = make_handle_from_claim(mem_region, 0);
      s.en = 1; s.mode = 5; s.rsvd = 10;
      write_struct(h, s);
      read_struct(h, t);
      mode_back = t.mode;
      read_bytes(h, bytes, 2);
      n = bytes.size;
    }
    rand int v;
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let get = |n: &str| s.steps[0].fields.iter().find(|(k, _)| k == n).map(|(_, v)| *v).unwrap();
    assert_eq!(get("mode_back"), 5, "the struct round-trips");
    assert_eq!(get("n"), 2, "read_bytes filled the list");
    // 1 | (5 << 1) | (10 << 4) == 171, stored at the handle's address.
    assert_eq!(get("bytes[0]"), 171);
}

#[test]
fn an_unclosed_template_expression_in_target_code_is_rejected() {
    // The corpus marks this pathological. Balanced braces and real template
    // expressions must still be accepted.
    assert!(compile_pss("action a { exec body C = \"\"\"printf(\"{{ x);\"\"\"; }").is_err());
    assert!(compile_pss("action a { exec body C = \"\"\"{{ x + 1 }}\"\"\"; }").is_ok());
    assert!(compile_pss("action a { exec body { int m[2][2] = {{1,2},{3,4}}; } }").is_ok());
}

#[test]
fn a_file_of_foreign_syntax_is_rejected_not_silently_emptied() {
    // An InFact `.rules` file (Siemens' PSS tool, a PSS-like notation that is
    // not the Accellera standard) used to parse into an *empty* model and
    // report success, because an unrecognized top-level construct was skipped
    // to the next `;`. That is worse than an error: it reports success for
    // input that was never PSS.
    let src = "rule_graph g {\n  attributes g { call_pre_post_randomize = true; }\n  action init;\n}\n";
    let err = compile_pss(src).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("unexpected"), "{msg}");
    assert!(msg.contains("rule_graph"), "{msg}");
}

#[test]
fn an_unrecognized_construct_inside_a_body_still_loads() {
    // Inside a package or component body the older lenient skip is kept: a
    // construct this parser does not handle there must not make a whole model
    // unloadable.
    let src = r#"
package p {
  struct S { rand int x; };
};
component C {
  action a { rand int v; };
};
"#;
    assert!(compile_pss(src).is_ok());
}

#[test]
fn an_unmatched_closing_brace_at_end_of_file_is_an_error() {
    // Found by pssparser's error-suite (syntax.braces). A stray trailing `}`
    // used to be ignored, because the item loop simply stopped at it.
    let src = "component C {\n    action A { }\n}\n}\n";
    let err = compile_pss(src).unwrap_err();
    assert!(format!("{}", err).contains("unmatched"), "{err}");
    // The balanced version still loads.
    assert!(compile_pss("component C { action A { } }\n").is_ok());
}

#[test]
fn an_open_range_inside_a_list_does_not_panic() {
    // Found by andestech/pss-gen (samples/7_2_Integer_types). An open-ended
    // range inside a comma list is valid PSS 8.5.9:
    //     rand int [64] in [..1 , 8..16 , 4294967296..] long_5;
    // The span of `[..1]` is `1 - i64::MIN`, which overflows i64 — computing it
    // in i64 panicked with "capacity overflow".
    let src = r#"
component C {
  action a {
    rand int x;
    constraint x in [..1 , 8..16 , 4294967296..];
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in 1u64..=60 {
        let s = engine::run_scenario(&m, "a", seed);
        let v = s.steps[0].fields[0].1;
        assert!(
            v <= 1 || (8..=16).contains(&v) || v >= 4_294_967_296,
            "seed {seed} produced {v}, outside the range list"
        );
    }
}

#[test]
fn each_open_range_form_is_accepted() {
    // `[..hi]`, `[lo..]`, `[lo..hi]` and a plain value, alone and in a list.
    for c in [
        "x in [..1]",
        "x in [0..]",
        "x in [8..16]",
        "x in [..1, 8..16]",
        "x in [0, 1]",
        "x in [1, 3, 5..9]",
    ] {
        let src = format!(
            "component C {{ action a {{ rand int x; constraint {c}; }}; }};"
        );
        assert!(compile_pss(&src).is_ok(), "should parse: {c}");
    }
}

// ---------------------------------------------------------------------------
// Randomisation and coverage, found by writing the examples in
// examples/randomisation and examples/coverage and measuring them.
// ---------------------------------------------------------------------------

#[test]
fn a_dist_roughly_follows_its_weights() {
    // `dist pick in [0 := 1, 1 := 3, 10..12 :/ 6]` — `:/` splits 6 across the
    // three values, so the weights are 1:3:2:2:2 of 10.
    let src = r#"
component C {
  action a { rand int pick; constraint dist pick in [0 := 1, 1 := 3, 10..12 :/ 6]; };
};
"#;
    let m = compile_pss(src).unwrap();
    let mut counts = std::collections::BTreeMap::new();
    let n = 2000;
    for seed in 1..=n {
        let s = engine::run_scenario(&m, "a", seed);
        *counts.entry(s.steps[0].fields[0].1).or_insert(0usize) += 1;
    }
    assert_eq!(counts.len(), 5, "only the five weighted values appear");
    let share = |v: i64| *counts.get(&v).unwrap_or(&0) as f64 / n as f64;
    assert!((share(0) - 0.10).abs() < 0.04, "0: {}", share(0));
    assert!((share(1) - 0.30).abs() < 0.05, "1: {}", share(1));
    for v in [10, 11, 12] {
        assert!((share(v) - 0.20).abs() < 0.05, "{v}: {}", share(v));
    }
}

#[test]
fn a_modulo_constraint_is_enforced() {
    // Found while writing the randomisation examples: `addr % 4 == 0` was
    // reported unsatisfied on every seed, because bound propagation cannot
    // invert a modulo.
    let src = r#"
component C {
  action a { rand int addr; constraint addr in [0..1023]; constraint addr % 4 == 0; };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in 1..=60 {
        let s = engine::run_scenario(&m, "a", seed);
        let v = s.steps[0].fields[0].1;
        assert_eq!(v % 4, 0, "seed {seed} gave {v}");
    }
}

#[test]
fn an_if_else_constraint_is_applied() {
    // `if (c) a else b` is lowered onto two implications. It used to be dropped
    // entirely — not applied *and* not counted.
    let src = r#"
component C {
  action a {
    rand int len; rand int burst;
    constraint len in [33..64];
    constraint { if (len > 32) { burst <= 4; } else { burst >= 100; } }
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in 1..=40 {
        let s = engine::run_scenario(&m, "a", seed);
        let f: std::collections::BTreeMap<_, _> = s.steps[0].fields.iter().cloned().collect();
        assert!(f["len"] > 32);
        assert!(f["burst"] <= 4, "seed {seed}: burst={}", f["burst"]);
    }
}

#[test]
fn a_cover_block_observes_its_body() {
    // `cover { do b; }` used to swallow the traversal: the body was skipped to
    // the next `;`, so the scenario came out empty.
    let src = r#"
component C {
  action a { activity { cover { do b; } } };
  action b { };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    assert_eq!(s.steps.len(), 1, "the covered traversal is executed");
}

#[test]
fn coverage_samples_coverpoints_and_crosses() {
    let src = r#"
package p { enum mode_e { FAST, SLOW }; };
component C {
  covergroup cg (int len, mode_e m) {
    coverpoint len { label len_bin; bins small = [0..3]; bins large = [4..7]; }
    coverpoint m { label mode_bin; }
    cross len_bin, mode_bin { label len_x_mode; }
  }
  action a { activity { cover { do b; } } };
  action b { rand int len; rand mode_e m; constraint len in [0..7]; };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let names: Vec<&str> = s.coverage.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"cg.len_bin"), "{names:?}");
    assert!(names.contains(&"cg.mode_bin"), "{names:?}");
    assert!(names.contains(&"cg.len_x_mode"), "the cross is sampled: {names:?}");
}

#[test]
fn a_foreach_constraint_is_expanded_per_element() {
    // `constraint foreach (a[i]) { ... }` used to fall through to Unsupported,
    // which is excluded from the constraint count — so it was neither applied
    // nor reported. It is now one constraint per element.
    let src = r#"
component C {
  action a {
    rand bit[8] payload[8];
    constraint foreach (payload[i]) { payload[i] in [0..15]; }
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in 1..=40 {
        let s = engine::run_scenario(&m, "a", seed);
        let f: std::collections::BTreeMap<_, _> = s.steps[0].fields.iter().cloned().collect();
        for i in 0..8 {
            let v = f[&format!("payload[{i}]")];
            assert!(v <= 15, "seed {seed}: payload[{i}] = {v}");
        }
    }
}

#[test]
fn a_unique_constraint_makes_values_distinct() {
    // `constraint unique { arr }` was not even parsed: it was taken for a named
    // constraint block called `unique`.
    let src = r#"
component C {
  action a {
    rand int ids[4];
    constraint foreach (ids[i]) { ids[i] in [1..4]; }
    constraint unique { ids };
  };
};
"#;
    let m = compile_pss(src).unwrap();
    for seed in 1..=40 {
        let s = engine::run_scenario(&m, "a", seed);
        let vals: Vec<i64> = (0..4)
            .map(|i| {
                s.steps[0]
                    .fields
                    .iter()
                    .find(|(k, _)| k == &format!("ids[{i}]"))
                    .map(|(_, v)| *v)
                    .unwrap()
            })
            .collect();
        let distinct: std::collections::BTreeSet<_> = vals.iter().collect();
        assert_eq!(distinct.len(), 4, "seed {seed}: {vals:?} not distinct");
    }
}

#[test]
fn an_impossible_unique_is_reported_not_faked() {
    // Five distinct values cannot come from [1..4]. The tool must say so.
    let src = r#"
component C {
  action a {
    rand int five[5];
    constraint foreach (five[i]) { five[i] in [1..4]; }
    constraint unique { five };
  };
};
"#;
    let m = compile_pss(src).unwrap();
    let s = engine::run_scenario(&m, "a", 1);
    let st = &s.steps[0];
    assert!(
        st.satisfied < st.total,
        "the unsatisfiable constraint must be reported: {}/{}",
        st.satisfied,
        st.total
    );
}

#[test]
fn a_sum_constraint_is_solved() {
    // `arr.sum()` folds the elements; an aggregate cannot be seen by bound
    // propagation, so the repair moves elements until the relation holds.
    let cases: [(&str, fn(i64) -> bool); 4] = [
        ("<= 25", |s| s <= 25),
        ("== 40", |s| s == 40),
        (">= 100", |s| s >= 100),
        ("< 10", |s| s < 10),
    ];
    for (rel, ok) in cases {
        let src = format!(
            "component C {{ action a {{ rand bit[8] v[4]; constraint v.sum() {rel}; }}; }};"
        );
        let m = compile_pss(&src).unwrap();
        for seed in 1..=20 {
            let s = engine::run_scenario(&m, "a", seed);
            let total: i64 = (0..4)
                .map(|i| {
                    s.steps[0]
                        .fields
                        .iter()
                        .find(|(k, _)| k == &format!("v[{i}]"))
                        .map(|(_, v)| *v)
                        .unwrap()
                })
                .sum();
            assert!(ok(total), "seed {seed}: sum {total} fails `sum {rel}`");
        }
    }
}
