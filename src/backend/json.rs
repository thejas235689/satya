//! JSON backend: a portable, tool-independent description of the solved
//! scenario. Any downstream harness — simulation, emulation or silicon —
//! can consume it.

use super::{json_escape, Backend, BackendOptions};
use crate::engine::{Scenario, StepMode};
use crate::error::PssResult;
use crate::ir::Model;

pub struct JsonBackend;

impl Backend for JsonBackend {
    fn name(&self) -> &'static str {
        "json"
    }

    fn description(&self) -> &'static str {
        "Portable JSON scenario description"
    }

    fn emit(&self, model: &Model, scenario: &Scenario, _opts: &BackendOptions) -> PssResult<String> {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!(
            "  \"generator\": \"satya\",\n  \"root_action\": \"{}\",\n",
            json_escape(&scenario.root_action)
        ));
        s.push_str(&format!(
            "  \"top_component\": {},\n",
            match &scenario.top_component {
                Some(t) => format!("\"{}\"", json_escape(t)),
                None => "null".to_string(),
            }
        ));
        s.push_str(&format!("  \"packages\": [{}],\n", model.packages.iter().map(|p| format!("\"{}\"", json_escape(p))).collect::<Vec<_>>().join(", ")));
        s.push_str(&format!("  \"step_count\": {},\n", scenario.steps.len()));
        s.push_str(&format!("  \"concrete_step_count\": {},\n", scenario.concrete_steps()));

        s.push_str("  \"steps\": [\n");
        for (i, step) in scenario.steps.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"index\": {},\n", step.index));
            s.push_str(&format!("      \"action\": \"{}\",\n", json_escape(&step.action)));
            s.push_str(&format!("      \"component\": \"{}\",\n", json_escape(&step.component)));
            s.push_str(&format!("      \"depth\": {},\n", step.depth));
            s.push_str(&format!("      \"mode\": \"{}\",\n", mode_str(step.mode)));
            s.push_str(&format!("      \"group\": {},\n", step.group));
            s.push_str(&format!("      \"unresolved\": {},\n", step.unresolved));
            s.push_str(&format!(
                "      \"constraints\": {{\"satisfied\": {}, \"total\": {}}},\n",
                step.satisfied, step.total
            ));
            s.push_str("      \"flow\": [");
            for (j, fr) in step.flow.iter().enumerate() {
                let comma = if j + 1 < step.flow.len() { "," } else { "" };
                s.push_str(&format!(
                    "{{\"field\": \"{}\", \"direction\": \"{}\", \"type\": \"{}\", \"pool\": {}, \"object\": {}}}{}",
                    json_escape(&fr.field),
                    fr.direction,
                    json_escape(&fr.type_name),
                    match &fr.pool { Some(p) => format!("\"{}\"", json_escape(p)), None => "null".to_string() },
                    match fr.obj_id { Some(i) => i.to_string(), None => "null".to_string() },
                    comma
                ));
            }
            s.push_str("],\n");
            s.push_str("      \"claims\": [");
            for (j, c) in step.claims.iter().enumerate() {
                let comma = if j + 1 < step.claims.len() { "," } else { "" };
                s.push_str(&format!(
                    "{{\"resource\": \"{}\", \"instance\": {}, \"exclusive\": {}}}{}",
                    json_escape(&c.resource), c.instance, c.exclusive, comma
                ));
            }
            s.push_str("],\n");
            s.push_str("      \"fields\": {");
            if step.fields.is_empty() {
                s.push('}');
            } else {
                s.push('\n');
                for (j, (k, v)) in step.fields.iter().enumerate() {
                    let comma = if j + 1 < step.fields.len() { "," } else { "" };
                    s.push_str(&format!("        \"{}\": {}{}\n", json_escape(k), v, comma));
                }
                s.push_str("      }");
            }
            s.push('\n');
            let comma = if i + 1 < scenario.steps.len() { "," } else { "" };
            s.push_str(&format!("    }}{}\n", comma));
        }
        s.push_str("  ],\n");

        s.push_str("  \"warnings\": [");
        if scenario.warnings.is_empty() {
            s.push(']');
        } else {
            s.push('\n');
            for (i, w) in scenario.warnings.iter().enumerate() {
                let comma = if i + 1 < scenario.warnings.len() { "," } else { "" };
                s.push_str(&format!("    \"{}\"{}\n", json_escape(w), comma));
            }
            s.push_str("  ]");
        }
        s.push('\n');
        s.push_str("}\n");
        Ok(s)
    }
}

fn mode_str(m: StepMode) -> &'static str {
    match m {
        StepMode::Sequential => "sequential",
        StepMode::Parallel => "parallel",
        StepMode::Schedule => "schedule",
        StepMode::Atomic => "atomic",
        StepMode::Select => "select",
    }
}
