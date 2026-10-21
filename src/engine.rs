//! The scenario engine: walks an action's activity graph, resolves each `do`
//! into a concrete action instance, solves its constraints, allocates flow
//! objects from pools, arbitrates resource claims, and records an executable
//! schedule.
//!
//! This is the "system-level coordination" half of Satya: the schedule it
//! produces is backend-agnostic and can be replayed on a simulator, an
//! emulator, a host processor (native C), or post-silicon hardware.

use std::collections::{BTreeMap, HashMap};

use crate::ast::{Activity, ActivityStmt, Expr, Path};
use crate::ir::{Action, Field, FieldKind, Model};
use crate::solver::{self, Env};

const MAX_NESTING: usize = 64;
const MAX_REPEAT: i64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMode {
    Sequential,
    Parallel,
    Schedule,
    Atomic,
    Select,
}

impl StepMode {
    pub fn label(self) -> &'static str {
        match self {
            StepMode::Sequential => "seq",
            StepMode::Parallel => "parallel",
            StepMode::Schedule => "schedule",
            StepMode::Atomic => "atomic",
            StepMode::Select => "select",
        }
    }
}

/// A flow object allocated to (or consumed by) a step.
#[derive(Debug, Clone)]
pub struct FlowRecord {
    pub field: String,
    /// `"output"` or `"input"`.
    pub direction: &'static str,
    pub type_name: String,
    pub pool: Option<String>,
    pub obj_id: Option<usize>,
    /// Base address assigned from the model's address space, if any.
    pub addr: Option<i64>,
}

/// A resource claim made by a step (`lock` or `share`).
#[derive(Debug, Clone)]
pub struct Claim {
    pub resource: String,
    pub instance: usize,
    pub exclusive: bool,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub index: usize,
    pub action: String,
    /// Dotted path of the component that owns this step, e.g. `pss_top.IP_A_c`.
    pub component: String,
    pub depth: usize,
    pub mode: StepMode,
    /// Parallel/schedule/select group id (0 when not grouped).
    pub group: u32,
    /// Solved values for the action's numeric leaf fields.
    pub fields: Vec<(String, i64)>,
    pub satisfied: usize,
    pub total: usize,
    /// Flow objects produced or consumed by this step.
    pub flow: Vec<FlowRecord>,
    /// Resource claims held by this step.
    pub claims: Vec<Claim>,
    /// True when the action could not be resolved in the model.
    pub unresolved: bool,
}

#[derive(Debug, Clone)]
pub struct Scenario {
    pub root_action: String,
    pub top_component: Option<String>,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
    /// Pools after execution: name, element type, and allocated object count.
    pub pools: Vec<(String, String, usize)>,
    /// Address regions the model declared.
    pub addr_regions: Vec<(String, i64, i64)>,
}

impl Scenario {
    pub fn concrete_steps(&self) -> usize {
        self.steps.iter().filter(|s| !s.unresolved).count()
    }
}

#[derive(Debug, Clone)]
struct FlowObj {
    values: BTreeMap<String, i64>,
    addr: Option<i64>,
}

#[derive(Debug, Clone)]
struct PoolState {
    name: String,
    type_name: String,
    capacity: Option<i64>,
    instances: Vec<FlowObj>,
    /// Index of the next object to hand out for an `input`.
    read_cursor: usize,
}

#[derive(Debug, Clone)]
struct ResourceState {
    capacity: usize,
    next: usize,
}

struct Engine<'a> {
    model: &'a Model,
    seed: u64,
    steps: Vec<Step>,
    warnings: Vec<String>,
    group_counter: u32,
    pools: Vec<PoolState>,
    resources: HashMap<String, ResourceState>,
    /// Bump cursors over the model's address regions.
    addr_cursors: Vec<(i64, i64)>,
}

/// Run a scenario rooted at `root_action`.
pub fn run_scenario(model: &Model, root_action: &str, seed: u64) -> Scenario {
    let top = model.top.clone();
    let mut eng = Engine {
        model,
        seed,
        steps: Vec::new(),
        warnings: Vec::new(),
        group_counter: 0,
        pools: Vec::new(),
        resources: HashMap::new(),
        addr_cursors: model
            .addr_regions
            .iter()
            .map(|r| (r.base, r.base + r.size))
            .collect(),
    };
    if let Some(t) = &top {
        eng.pools = collect_pools(model, t);
        eng.resources = collect_resources(model, t);
    }
    let action = model.find_action(top.as_deref(), root_action).cloned();
    match action {
        Some(act) => {
            eng.walk_action(
                &act,
                top.clone().unwrap_or_default(),
                0,
                StepMode::Sequential,
                0,
                false,
            );
        }
        None => {
            eng.warnings
                .push(format!("root action `{}` not found in model", root_action));
        }
    }
    let pools = eng
        .pools
        .iter()
        .map(|p| (p.name.clone(), p.type_name.clone(), p.instances.len()))
        .collect();
    let addr_regions = model
        .addr_regions
        .iter()
        .map(|r| (r.name.clone(), r.base, r.size))
        .collect();
    Scenario {
        root_action: root_action.to_string(),
        top_component: top,
        steps: eng.steps,
        warnings: eng.warnings,
        pools,
        addr_regions,
    }
}

impl<'a> Engine<'a> {
    fn walk_action(
        &mut self,
        action: &Action,
        component: String,
        depth: usize,
        mode: StepMode,
        group: u32,
        record_self: bool,
    ) {
        if depth > MAX_NESTING {
            self.warnings.push("activity nesting limit reached".into());
            return;
        }
        let has_activity = action.activity.is_some();
        if !has_activity {
            self.record_step(action, component.clone(), depth, mode, group, &[], &BTreeMap::new());
            return;
        }
        if record_self {
            self.record_step(action, component.clone(), depth, mode, group, &[], &BTreeMap::new());
        }
        if let Some(act) = &action.activity {
            // Solve the container action first so its random fields — a repeat
            // count, for instance — are available while walking the activity.
            let report = solver::solve(self.model, action, self.seed ^ (depth as u64) ^ 0x5EED);
            let ctx = report.assignment.values;
            self.walk_activity(act, component, depth + 1, mode, group, &ctx);
        }
    }

    fn walk_activity(
        &mut self,
        activity: &Activity,
        component: String,
        depth: usize,
        mode: StepMode,
        group: u32,
        ctx: &BTreeMap<String, i64>,
    ) {
        let Activity::Sequence(stmts) = activity;
        for stmt in stmts {
            self.walk_stmt(stmt, &component, depth, mode, group, ctx);
        }
    }

    fn walk_stmt(
        &mut self,
        stmt: &ActivityStmt,
        component: &str,
        depth: usize,
        mode: StepMode,
        group: u32,
        ctx: &BTreeMap<String, i64>,
    ) {
        match stmt {
            ActivityStmt::Do(path) => {
                self.dispatch(path, component, depth, mode, group);
            }
            ActivityStmt::DoWith { action, body } => {
                self.dispatch(action, component, depth, mode, group);
                if let Some(b) = body {
                    self.walk_activity(b, component.to_string(), depth + 1, mode, group, ctx);
                }
            }
            ActivityStmt::DoConstrained { action, constraints } => {
                self.dispatch_constrained(action, component, depth, mode, group, constraints, ctx);
            }
            ActivityStmt::Parallel(branches) => {
                self.group_counter += 1;
                let g = self.group_counter;
                let mark = self.steps.len();
                for b in branches {
                    self.walk_activity(b, component.to_string(), depth, StepMode::Parallel, g, ctx);
                }
                self.enforce_contention(mark, g);
            }
            ActivityStmt::Schedule(branches) => {
                self.group_counter += 1;
                let g = self.group_counter;
                let mark = self.steps.len();
                for b in branches {
                    self.walk_activity(b, component.to_string(), depth, StepMode::Schedule, g, ctx);
                }
                self.enforce_contention(mark, g);
            }
            ActivityStmt::Select(branches) => {
                self.group_counter += 1;
                let g = self.group_counter;
                let mut chosen: Option<&Activity> = None;
                let mut fallback: Option<&Activity> = None;
                for (guard, body) in branches {
                    match guard {
                        None => fallback = Some(body),
                        Some(e) => {
                            if eval_condition_opt(e, ctx, self.model) == Some(true) {
                                chosen = Some(body);
                                break;
                            }
                        }
                    }
                }
                match chosen.or(fallback) {
                    Some(body) => {
                        self.walk_activity(body, component.to_string(), depth, StepMode::Select, g, ctx)
                    }
                    None => self
                        .warnings
                        .push("select: no branch guard was satisfiable".into()),
                }
            }
            ActivityStmt::Match { expr, choices } => {
                self.group_counter += 1;
                let g = self.group_counter;
                let subject = eval_value(expr, ctx, self.model);
                let mut chosen: Option<&Activity> = None;
                let mut default_body: Option<&Activity> = None;
                for (range, body) in choices {
                    match range {
                        None => default_body = Some(body),
                        Some(r) => {
                            if let Some(s) = subject {
                                if range_matches(r, s, ctx, self.model) {
                                    chosen = Some(body);
                                    break;
                                }
                            }
                        }
                    }
                }
                match chosen.or(default_body) {
                    Some(body) => {
                        self.walk_activity(body, component.to_string(), depth, StepMode::Select, g, ctx)
                    }
                    None => self.warnings.push(format!(
                        "match: no choice matched the subject ({:?})",
                        subject
                    )),
                }
            }
            ActivityStmt::Repeat { count, index, body } => {
                let n = eval_count(count, self.model, ctx).unwrap_or(1);
                if n > MAX_REPEAT {
                    self.warnings
                        .push(format!("repeat count {} clamped to {}", n, MAX_REPEAT));
                }
                for k in 0..n.min(MAX_REPEAT) {
                    if let Some(name) = index {
                        let mut local = ctx.clone();
                        local.insert(name.clone(), k);
                        self.walk_activity(body, component.to_string(), depth, mode, group, &local);
                    } else {
                        self.walk_activity(body, component.to_string(), depth, mode, group, ctx);
                    }
                }
            }
            ActivityStmt::Foreach { var, iter, body } => {
                // If the collection names a pool, iterate its allocated
                // objects; otherwise run the body once.
                if let Some(pool_idx) = self.pool_index_for(iter) {
                    let count = self.pools[pool_idx].instances.len();
                    for i in 0..count {
                        let mut local = ctx.clone();
                        if !var.is_empty() {
                            local.insert(var.clone(), i as i64);
                            for (k, v) in &self.pools[pool_idx].instances[i].values {
                                local.insert(format!("{}.{}", var, k), *v);
                            }
                        }
                        self.walk_activity(body, component.to_string(), depth, mode, group, &local);
                    }
                    if count == 0 {
                        self.warnings.push(format!(
                            "foreach over pool `{}` ran zero iterations (nothing allocated yet)",
                            self.pools[pool_idx].name
                        ));
                    }
                } else {
                    if let Expr::Path(p) = iter {
                        self.warnings.push(format!(
                            "foreach over `{}` executed once (not a modelled pool)",
                            p.segments.join(".")
                        ));
                    }
                    self.walk_activity(body, component.to_string(), depth, mode, group, ctx);
                }
            }
            ActivityStmt::Atomic(body) => {
                self.walk_activity(body, component.to_string(), depth, StepMode::Atomic, group, ctx);
            }
            ActivityStmt::Constraint(_) => {
                // Activity-level constraints refine the enclosing action; the
                // enclosing action is already solved, so this is recorded but
                // not re-solved.
            }
            ActivityStmt::If { cond, then, els } => {
                if let Some(v) = eval_condition_opt(cond, ctx, self.model) {
                    if v {
                        self.walk_activity(then, component.to_string(), depth, mode, group, ctx);
                    } else if let Some(e) = els {
                        self.walk_activity(e, component.to_string(), depth, mode, group, ctx);
                    }
                    return;
                }
                if els.is_some() {
                    self.warnings
                        .push("conditional activity: taking the `then` branch".into());
                }
                self.walk_activity(then, component.to_string(), depth, mode, group, ctx);
            }
            ActivityStmt::Raw(_) => {}
        }
    }

    fn dispatch(&mut self, path: &Path, component: &str, depth: usize, mode: StepMode, group: u32) {
        let (target_component, action_name) = split_action_ref(path);
        let comp = if let Some(c) = target_component {
            resolve_component(self.model, component, &c).unwrap_or(c)
        } else {
            component.to_string()
        };
        match self.model.find_action(Some(&comp), &action_name).cloned() {
            Some(act) => self.walk_action(&act, comp, depth, mode, group, false),
            None => self.push_unresolved(&action_name, comp, path, depth, mode, group),
        }
    }

    fn dispatch_constrained(
        &mut self,
        path: &Path,
        component: &str,
        depth: usize,
        mode: StepMode,
        group: u32,
        constraints: &[Expr],
        bindings: &BTreeMap<String, i64>,
    ) {
        let (target_component, action_name) = split_action_ref(path);
        let comp = if let Some(c) = target_component {
            resolve_component(self.model, component, &c).unwrap_or(c)
        } else {
            component.to_string()
        };
        match self.model.find_action(Some(&comp), &action_name).cloned() {
            Some(act) => {
                if act.activity.is_some() {
                    self.walk_action(&act, comp, depth, mode, group, false);
                } else {
                    self.record_step(&act, comp, depth, mode, group, constraints, bindings);
                }
            }
            None => self.push_unresolved(&action_name, comp, path, depth, mode, group),
        }
    }

    fn push_unresolved(
        &mut self,
        action_name: &str,
        comp: String,
        path: &Path,
        depth: usize,
        mode: StepMode,
        group: u32,
    ) {
        self.warnings.push(format!(
            "unresolved action `{}` (no definition in the model)",
            path.segments.join("::")
        ));
        let idx = self.steps.len();
        self.steps.push(Step {
            index: idx,
            action: action_name.to_string(),
            component: comp,
            depth,
            mode,
            group,
            fields: Vec::new(),
            satisfied: 0,
            total: 0,
            flow: Vec::new(),
            claims: Vec::new(),
            unresolved: true,
        });
    }

    fn record_step(
        &mut self,
        action: &Action,
        component: String,
        depth: usize,
        mode: StepMode,
        group: u32,
        extra: &[Expr],
        bindings: &BTreeMap<String, i64>,
    ) {
        let report = solver::solve_with(
            self.model,
            action,
            self.seed ^ (self.steps.len() as u64),
            extra,
            bindings,
        );
        let mut fields: Vec<(String, i64)> = Vec::new();
        for (k, v) in &report.assignment.values {
            if k.matches('.').count() <= 1 {
                fields.push((k.clone(), *v));
            }
        }

        // Allocate / consume flow objects and claim resources.
        let mut flow = Vec::new();
        let mut claims = Vec::new();
        for f in &action.fields {
            match f.kind {
                FieldKind::Output => {
                    flow.push(self.allocate(f, &report.assignment.values, "output"));
                }
                FieldKind::Input => {
                    flow.push(self.consume(f, "input"));
                }
                FieldKind::Lock | FieldKind::Share => {
                    let exclusive = f.kind == FieldKind::Lock;
                    let inst = self.claim(&f.type_name, exclusive);
                    claims.push(Claim {
                        resource: f.type_name.clone(),
                        instance: inst,
                        exclusive,
                    });
                }
                _ => {}
            }
        }

        let idx = self.steps.len();
        self.steps.push(Step {
            index: idx,
            action: action.name.clone(),
            component,
            depth,
            mode,
            group,
            fields,
            satisfied: report.satisfied,
            total: report.total,
            flow,
            claims,
            unresolved: false,
        });
    }

    /// Allocate an object from the pool bound to the field's type.
    fn allocate(
        &mut self,
        f: &Field,
        values: &BTreeMap<String, i64>,
        direction: &'static str,
    ) -> FlowRecord {
        let ty = f.type_name.clone();
        let mut obj_values = BTreeMap::new();
        let prefix = format!("{}.", f.name);
        for (k, v) in values {
            if let Some(rest) = k.strip_prefix(&prefix) {
                obj_values.insert(rest.to_string(), *v);
            }
        }
        let size = obj_values.get("size").copied().unwrap_or(64).max(1);
        let addr = self.alloc_addr(size);
        match self.pools.iter().position(|p| p.type_name == ty) {
            Some(i) => {
                if let Some(cap) = self.pools[i].capacity {
                    if self.pools[i].instances.len() as i64 >= cap {
                        // Pool exhausted: refuse the allocation rather than
                        // silently over-allocating.
                        self.warnings.push(format!(
                            "pool `{}` exhausted ({} objects); allocation of `{}` refused",
                            self.pools[i].name, cap, f.name
                        ));
                        return FlowRecord {
                            field: f.name.clone(),
                            direction,
                            type_name: ty,
                            pool: Some(self.pools[i].name.clone()),
                            obj_id: None,
                            addr: None,
                        };
                    }
                }
                let id = self.pools[i].instances.len();
                self.pools[i]
                    .instances
                    .push(FlowObj { values: obj_values, addr });
                FlowRecord {
                    field: f.name.clone(),
                    direction,
                    type_name: ty,
                    pool: Some(self.pools[i].name.clone()),
                    obj_id: Some(id),
                    addr,
                }
            }
            None => FlowRecord {
                field: f.name.clone(),
                direction,
                type_name: ty,
                pool: None,
                obj_id: None,
                addr,
            },
        }
    }

    /// Allocate `size` bytes (64-byte aligned) from the first region with room.
    fn alloc_addr(&mut self, size: i64) -> Option<i64> {
        for (cur, end) in self.addr_cursors.iter_mut() {
            let need = ((size + 63) / 64) * 64;
            if *cur + need <= *end {
                let a = *cur;
                *cur += need;
                return Some(a);
            }
        }
        None
    }

    /// Consume an object of the field's type from its pool.
    fn consume(&mut self, f: &Field, direction: &'static str) -> FlowRecord {
        let ty = f.type_name.clone();
        match self.pools.iter().position(|p| p.type_name == ty) {
            Some(i) => {
                let avail = self.pools[i].instances.len();
                if avail == 0 {
                    self.warnings.push(format!(
                        "input `{}` of type `{}`: no object available in pool `{}`",
                        f.name, ty, self.pools[i].name
                    ));
                    return FlowRecord {
                        field: f.name.clone(),
                        direction,
                        type_name: ty,
                        pool: Some(self.pools[i].name.clone()),
                        obj_id: None,
                        addr: None,
                    };
                }
                let cursor = self.pools[i].read_cursor;
                let id = cursor % avail;
                self.pools[i].read_cursor = cursor + 1;
                let addr = self.pools[i].instances[id].addr;
                FlowRecord {
                    field: f.name.clone(),
                    direction,
                    type_name: ty,
                    pool: Some(self.pools[i].name.clone()),
                    obj_id: Some(id),
                    addr,
                }
            }
            None => FlowRecord {
                field: f.name.clone(),
                direction,
                type_name: ty,
                pool: None,
                obj_id: None,
                addr: None,
            },
        }
    }

    fn claim(&mut self, resource: &str, _exclusive: bool) -> usize {
        let st = self
            .resources
            .entry(resource.to_string())
            .or_insert(ResourceState {
                capacity: 1,
                next: 0,
            });
        let inst = st.next % st.capacity.max(1);
        st.next = st.next.wrapping_add(1);
        inst
    }

    /// Enforce resource exclusivity inside a parallel group: when exclusive
    /// claims exceed the instances available, the conflicting steps are
    /// serialised (moved out of the parallel group) and the decision reported.
    fn enforce_contention(&mut self, from: usize, group: u32) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for st in &self.steps[from..] {
            if st.group != group {
                continue;
            }
            for c in &st.claims {
                if c.exclusive {
                    *counts.entry(c.resource.clone()).or_insert(0) += 1;
                }
            }
        }
        let mut over: Vec<String> = Vec::new();
        for (res, n) in &counts {
            let cap = self.resources.get(res).map(|r| r.capacity).unwrap_or(1);
            if *n > cap {
                over.push(res.clone());
            }
        }
        if over.is_empty() {
            return;
        }
        let mut serialised = 0usize;
        for st in self.steps[from..].iter_mut() {
            if st.group != group {
                continue;
            }
            if st.claims.iter().any(|c| c.exclusive && over.contains(&c.resource)) {
                st.mode = StepMode::Sequential;
                st.group = 0;
                serialised += 1;
            }
        }
        self.warnings.push(format!(
            "resource contention on {}: serialised {} step(s) in parallel group {}",
            over.join(", "),
            serialised,
            group
        ));
    }

    fn pool_index_for(&self, iter: &Expr) -> Option<usize> {
        if let Expr::Path(p) = iter {
            let leaf = p.segments.last()?;
            return self.pools.iter().position(|pool| &pool.name == leaf);
        }
        None
    }
}

fn split_action_ref(path: &Path) -> (Option<String>, String) {
    let segs = &path.segments;
    if segs.len() >= 2 {
        let action = segs.last().unwrap().clone();
        let comp = segs[segs.len() - 2].clone();
        (Some(comp), action)
    } else {
        (None, segs.first().cloned().unwrap_or_default())
    }
}

/// Resolve a possibly-short component name against a dotted component path.
fn resolve_component(model: &Model, current: &str, want: &str) -> Option<String> {
    let want_leaf = want.rsplit("::").next().unwrap_or(want);
    let candidates = [current.to_string(), model.top.clone().unwrap_or_default()];
    for base in candidates.iter().filter(|b| !b.is_empty()) {
        if let Some(comp) = model.component(base.rsplit('.').next().unwrap_or(base)) {
            for (inst, ty) in &comp.subcomponents {
                if ty == want_leaf {
                    return Some(format!("{}.{}", base, inst));
                }
            }
        }
    }
    None
}

/// Gather the pools of a component and its sub-components.
fn collect_pools(model: &Model, top: &str) -> Vec<PoolState> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    collect_pools_into(model, top, &mut out, &mut seen, 0);
    out
}

fn collect_pools_into(
    model: &Model,
    name: &str,
    out: &mut Vec<PoolState>,
    seen: &mut Vec<String>,
    depth: usize,
) {
    if depth > 8 || seen.contains(&name.to_string()) {
        return;
    }
    seen.push(name.to_string());
    if let Some(c) = model.component(name) {
        for p in &c.pools {
            out.push(PoolState {
                name: p.name.clone(),
                type_name: p.type_name.clone(),
                capacity: p.count,
                instances: Vec::new(),
                read_cursor: 0,
            });
        }
        for (_, ty) in &c.subcomponents {
            collect_pools_into(model, ty, out, seen, depth + 1);
        }
    }
}

/// Estimate the number of exclusive instances available per resource type.
///
/// A resource type such as `pssm_executor_r` is served by component instances
/// of a matching family (`pssm_executor_c`); we count sub-components whose type
/// name shares the resource's stem. Types with no matching component default to
/// a single instance.
fn collect_resources(model: &Model, top: &str) -> HashMap<String, ResourceState> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut subcomponent_types: Vec<String> = Vec::new();
    let mut seen = Vec::new();
    gather_subcomponent_types(model, top, &mut subcomponent_types, &mut seen, 0);
    for ty in &subcomponent_types {
        *counts.entry(ty.clone()).or_insert(0) += 1;
    }
    // Map resource types seen anywhere in the model onto a capacity.
    let mut out = HashMap::new();
    for action in model.actions.values() {
        for f in &action.fields {
            if matches!(f.kind, FieldKind::Lock | FieldKind::Share) {
                let stem = resource_stem(&f.type_name);
                let cap = subcomponent_types
                    .iter()
                    .filter(|t| resource_stem(t) == stem)
                    .count()
                    .max(1);
                out.insert(
                    f.type_name.clone(),
                    ResourceState {
                        capacity: cap,
                        next: 0,
                    },
                );
            }
        }
    }
    out
}

fn gather_subcomponent_types(
    model: &Model,
    name: &str,
    out: &mut Vec<String>,
    seen: &mut Vec<String>,
    depth: usize,
) {
    if depth > 8 || seen.contains(&name.to_string()) {
        return;
    }
    seen.push(name.to_string());
    if let Some(c) = model.component(name) {
        for (_, ty) in &c.subcomponents {
            out.push(ty.clone());
            gather_subcomponent_types(model, ty, out, seen, depth + 1);
        }
    }
}

/// `pssm_executor_r` / `pssm_executor_c` -> `pssm_executor`.
fn resource_stem(name: &str) -> String {
    let leaf = name.rsplit("::").next().unwrap_or(name);
    leaf.trim_end_matches("_r")
        .trim_end_matches("_c")
        .trim_end_matches("_s")
        .to_string()
}

fn eval_count(e: &Expr, model: &Model, ctx: &BTreeMap<String, i64>) -> Option<i64> {
    let env = Env::new(model);
    if let Some(v) = solver::eval(e, ctx, &env) {
        return Some(v);
    }
    e.eval_const(&|n| model.constants.get(n).copied())
}

/// Evaluate a condition against the container action's solved values, if all
/// its references are known.
fn eval_condition_opt(expr: &Expr, ctx: &BTreeMap<String, i64>, model: &Model) -> Option<bool> {
    let env = Env::new(model);
    solver::eval(expr, ctx, &env).map(|v| v != 0)
}

/// Evaluate an expression to a value against the container action's values.
fn eval_value(expr: &Expr, ctx: &BTreeMap<String, i64>, model: &Model) -> Option<i64> {
    let env = Env::new(model);
    solver::eval(expr, ctx, &env)
}

/// Does a `match` choice's range contain `value`?
fn range_matches(range: &Expr, value: i64, ctx: &BTreeMap<String, i64>, model: &Model) -> bool {
    let env = Env::new(model);
    match range {
        Expr::InRange(_, lo, hi) => {
            let l = solver::eval(lo, ctx, &env).unwrap_or(i64::MIN);
            let h = solver::eval(hi, ctx, &env).unwrap_or(i64::MAX);
            value >= l && value <= h
        }
        Expr::InSet(_, items) => items.iter().any(|i| solver::eval(i, ctx, &env) == Some(value)),
        Expr::List(items) => items.iter().any(|i| range_matches(i, value, ctx, model)),
        other => solver::eval(other, ctx, &env) == Some(value),
    }
}

/// Evaluate a boolean expression against a solved assignment (used by
/// backends when they need to re-check an activity condition).
pub fn eval_condition(
    expr: &Expr,
    values: &std::collections::BTreeMap<String, i64>,
    model: &Model,
) -> bool {
    let env = Env::new(model);
    solver::eval(expr, values, &env).unwrap_or(0) != 0
}
