//! Trace backend: a human-readable execution trace of the solved scenario.

use super::{Backend, BackendOptions};
use crate::engine::Scenario;
use crate::error::PssResult;
use crate::ir::Model;

pub struct TraceBackend;

impl Backend for TraceBackend {
    fn name(&self) -> &'static str {
        "trace"
    }

    fn description(&self) -> &'static str {
        "Human-readable execution trace"
    }

    fn emit(&self, _model: &Model, scenario: &Scenario, _opts: &BackendOptions) -> PssResult<String> {
        let mut s = String::new();
        s.push_str("Satya scenario trace\n");
        s.push_str("====================\n");
        s.push_str(&format!("root action     : {}\n", scenario.root_action));
        s.push_str(&format!(
            "top component   : {}\n",
            scenario.top_component.as_deref().unwrap_or("<none>")
        ));
        s.push_str(&format!("steps           : {}\n", scenario.steps.len()));
        s.push_str(&format!("concrete steps  : {}\n", scenario.concrete_steps()));
        s.push_str(&format!("warnings        : {}\n", scenario.warnings.len()));
        if !scenario.pools.is_empty() {
            let pools: Vec<String> = scenario
                .pools
                .iter()
                .map(|(n, t, c)| format!("{}:{}={}", n, t, c))
                .collect();
            s.push_str(&format!("pools           : {}\n", pools.join(" ")));
        }
        s.push_str("\n");

        for step in &scenario.steps {
            let indent = "  ".repeat(step.depth);
            let mode = match step.mode {
                crate::engine::StepMode::Sequential => "",
                m => m.label(),
            };
            let group = if step.group > 0 {
                format!(" g{}", step.group)
            } else {
                String::new()
            };
            let tag = if step.unresolved { " [UNRESOLVED]" } else { "" };
            let label = match &step.label {
                Some(l) if step.replicated => format!("  {} (replicated)", l),
                Some(l) => format!("  {}", l),
                None => String::new(),
            };
            s.push_str(&format!(
                "{}{:>3}. {:<24} @ {:<20} [{}{}{}]{}{}\n",
                indent,
                step.index,
                step.action,
                step.component,
                mode,
                group,
                if mode.is_empty() && group.is_empty() { "leaf" } else { "" },
                tag,
                label
            ));
            if !step.fields.is_empty() {
                let fields: Vec<String> = step
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                s.push_str(&format!("{}     solved: {}\n", indent, fields.join(" ")));
            }
            if !step.strings.is_empty() {
                let kv: Vec<String> = step
                    .strings
                    .iter()
                    .map(|(k, v)| format!("{} = \"{}\"", k, v))
                    .collect();
                s.push_str(&format!("{}     strings: {}\n", indent, kv.join(" ")));
            }
            if step.total > 0 {
                s.push_str(&format!(
                    "{}     constraints: {}/{} satisfied\n",
                    indent, step.satisfied, step.total
                ));
                for u in &step.unsatisfied {
                    s.push_str(&format!("{}       not satisfied: {}\n", indent, u));
                }
                // Not counted in the total above, so it must not be labelled
                // "not satisfied".
                for u in &step.unevaluated {
                    s.push_str(&format!("{}       not evaluated: {}\n", indent, u));
                }
            }
            for fr in &step.flow {
                let obj = match fr.obj_id {
                    Some(id) => format!("#{}", id),
                    None => "-".to_string(),
                };
                s.push_str(&format!(
                    "{}     {} {} : {}{}\n",
                    indent,
                    fr.direction,
                    fr.field,
                    fr.type_name,
                    match &fr.pool {
                        Some(p) => format!(" from pool `{}` obj {}", p, obj),
                        None => " (no pool bound)".to_string(),
                    }
                ));
            }
            for c in &step.claims {
                s.push_str(&format!(
                    "{}     {} {} (instance {})\n",
                    indent,
                    if c.exclusive { "lock" } else { "share" },
                    c.resource,
                    c.instance
                ));
            }
        }

        if !scenario.messages.is_empty() {
            s.push_str("\nMessages\n--------\n");
            for m in &scenario.messages {
                s.push_str(&format!("  {}\n", m));
            }
        }

        if !scenario.coverage.is_empty() {
            s.push_str("\nCoverage\n--------\n");
            for cp in &scenario.coverage {
                let total: usize = cp.values.iter().map(|(_, c)| *c).sum();
                let bins = match cp.bins {
                    Some((hit, n)) => format!("   bins {hit}/{n}"),
                    None => String::new(),
                };
                s.push_str(&format!(
                    "  {:<28} {} value(s) observed, {} hit(s){}\n",
                    cp.name,
                    cp.values.len(),
                    total,
                    bins
                ));
            }
        }

        if !scenario.warnings.is_empty() {
            s.push_str("\nWarnings\n--------\n");
            for w in &scenario.warnings {
                s.push_str(&format!("  - {}\n", w));
            }
        }
        Ok(s)
    }
}
