//! The scenario engine: walks an action's activity graph, resolves each `do`
//! into a concrete action instance, solves its constraints, allocates flow
//! objects from pools, arbitrates resource claims, and records an executable
//! schedule.
//!
//! This is the "system-level coordination" half of Satya: the schedule it
//! produces is backend-agnostic and can be replayed on a simulator, an
//! emulator, a host processor (native C), or post-silicon hardware.

use std::collections::{BTreeMap, HashMap};

use crate::ast::{Activity, ActivityStmt, ExecStmt, Expr, JoinSpec, Path};
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

/// One sampled coverpoint or cross (PSS 18.3, 18.4).
#[derive(Debug, Clone)]
pub struct CoveragePoint {
    /// `covergroup.label`, or `covergroup.<expr>` when unlabelled.
    pub name: String,
    /// Observed value (or value tuple, for a cross) -> hit count.
    pub values: Vec<(String, usize)>,
    /// `(bins hit, bins declared)` when the coverpoint declared explicit bins.
    pub bins: Option<(usize, usize)>,
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
    /// Label of the enclosing labelled activity statement, if any.
    pub label: Option<String>,
    /// True for the copies produced by `replicate` (PSS 12.5).
    pub replicated: bool,
    /// Solved values for the action's numeric leaf fields.
    pub fields: Vec<(String, i64)>,
    pub satisfied: usize,
    pub total: usize,
    /// Constraints the solver could not evaluate or satisfy (for diagnosis).
    pub unsatisfied: Vec<String>,
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
    /// Sampled coverpoints and crosses.
    pub coverage: Vec<CoveragePoint>,
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
    /// Label of the labelled activity statement being walked, if any.
    current_label: Option<String>,
    /// Set while walking the copies of a `replicate` statement.
    current_replicated: bool,
    /// Flow objects produced so far: type -> (producing step, id, consumed).
    flow_produced: HashMap<String, Vec<(usize, usize, bool)>>,
    /// Identity counter for flow objects that are not bound to a pool.
    flow_seq: usize,
    /// True while walking the body of a `cover` statement.
    in_cover: bool,
    /// Values established by component `init_down`/`init_up` blocks, visible to
    /// the whole scenario (PSS 22.1).
    globals: BTreeMap<String, i64>,
    /// Coverpoint name -> observed value -> hit count.
    coverage: BTreeMap<String, BTreeMap<String, usize>>,
    /// Declared bins per coverpoint, for coverage against the declaration.
    coverage_bins: BTreeMap<String, Vec<Vec<i64>>>,
}

/// Peel any `Labeled` wrappers off a statement, returning the inner statement
/// and the outermost label.
fn peel_label(stmt: &ActivityStmt) -> (&ActivityStmt, Option<&str>) {
    match stmt {
        ActivityStmt::Labeled { label, stmt } => (stmt, Some(label.as_str())),
        other => (other, None),
    }
}

/// The values a `bins` declaration covers: a single value, a `lo..hi` range, or
/// a `{ ... }` list of those (PSS 18.3).
fn bin_values(e: &Expr, ctx: &BTreeMap<String, i64>, model: &Model) -> Vec<i64> {
    match e {
        Expr::List(items) => items
            .iter()
            .flat_map(|i| bin_values(i, ctx, model))
            .collect(),
        Expr::InRange(_, lo, hi) => {
            let lo = eval_value(lo, ctx, model);
            let hi = eval_value(hi, ctx, model);
            match (lo, hi) {
                (Some(lo), Some(hi)) if hi >= lo && hi - lo < 4096 => (lo..=hi).collect(),
                _ => Vec::new(),
            }
        }
        other => eval_value(other, ctx, model).into_iter().collect(),
    }
}

/// Largest collection an `exec` block may build, to bound a pathological model.
const MAX_LIST: i64 = 256;

/// The label of a top-level branch, if the branch is a labelled statement.
fn branch_label(branch: &Activity) -> Option<&str> {
    let Activity::Sequence(stmts) = branch;
    match stmts.as_slice() {
        [ActivityStmt::Labeled { label, .. }] => Some(label.as_str()),
        _ => None,
    }
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
        current_label: None,
        current_replicated: false,
        flow_produced: HashMap::new(),
        flow_seq: 0,
        in_cover: false,
        globals: BTreeMap::new(),
        coverage: BTreeMap::new(),
        coverage_bins: BTreeMap::new(),
    };
    if let Some(t) = &top {
        eng.pools = collect_pools(model, t);
        eng.resources = collect_resources(model, t);
    }
    // Component `init_down`/`init_up` blocks run before the root action's
    // `pre_solve`, `init_down` top-down and `init_up` bottom-up (PSS 22.1).
    if let Some(t) = &top {
        let mut visited = Vec::new();
        eng.run_component_inits(t, &mut visited);
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
    let coverage = eng
        .coverage
        .iter()
        .map(|(point, hits)| {
            let bins = eng.coverage_bins.get(point).map(|decl| {
                let hit = decl
                    .iter()
                    .filter(|allowed| {
                        hits.keys().any(|k| {
                            k.parse::<i64>()
                                .map(|v| allowed.contains(&v))
                                .unwrap_or(false)
                        })
                    })
                    .count();
                (hit, decl.len())
            });
            CoveragePoint {
                name: point.clone(),
                values: hits.iter().map(|(v, c)| (v.clone(), *c)).collect(),
                bins,
            }
        })
        .collect();
    Scenario {
        coverage,
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
        let mut i = 0;
        // `cover <type>;` has no body: it observes from that point to the end of
        // the enclosing activity (PSS 19.2).
        let mut covered_from_here = false;
        while i < stmts.len() {
            let stmt = &stmts[i];
            if let ActivityStmt::Cover { body: None, .. } = peel_label(stmt).0 {
                covered_from_here = true;
                i += 1;
                continue;
            }
            // A fine-grained scheduling specifier changes what the statement
            // *after* the block waits for (PSS 12.3.6), so handle the pair here.
            // A labelled block (`L1: parallel join_branch(..) { .. }`) is the
            // common form, so peel the label before looking for the spec.
            let (inner, _) = peel_label(stmt);
            let join_block = match inner {
                ActivityStmt::Parallel { branches, join } | ActivityStmt::Schedule { branches, join } => {
                    Some((branches, join, matches!(inner, ActivityStmt::Schedule { .. })))
                }
                _ => None,
            };
            if let Some((branches, join, is_schedule)) = join_block {
                if !matches!(join, JoinSpec::All) {
                    let continuation = stmts.get(i + 1);
                    let m = if is_schedule { StepMode::Schedule } else { StepMode::Parallel };
                    self.walk_joined(branches, join, continuation, &component, depth, m, ctx);
                    i += if continuation.is_some() { 2 } else { 1 };
                    continue;
                }
            }
            let saved_cover = self.in_cover;
            if covered_from_here {
                self.in_cover = true;
            }
            self.walk_stmt(stmt, &component, depth, mode, group, ctx);
            self.in_cover = saved_cover;
            i += 1;
        }
    }

    /// Walk a `parallel`/`schedule` block that carries a fine-grained
    /// scheduling specifier, emitting the branches the following statement
    /// depends on first, then that statement, then the rest (PSS 12.3.6).
    #[allow(clippy::too_many_arguments)]
    fn walk_joined(
        &mut self,
        branches: &[Activity],
        join: &JoinSpec,
        continuation: Option<&ActivityStmt>,
        component: &str,
        depth: usize,
        mode: StepMode,
        ctx: &BTreeMap<String, i64>,
    ) {
        self.group_counter += 1;
        let g = self.group_counter;
        let mark = self.steps.len();

        // Which top-level branches does the following statement wait for?
        let gating: Vec<bool> = match join {
            JoinSpec::All => vec![true; branches.len()],
            // `join_none` has no scheduling dependency, and `join_first` has a
            // run-time dependency only, so neither gates the schedule.
            JoinSpec::None | JoinSpec::First(_) => vec![false; branches.len()],
            JoinSpec::Branch(labels) => branches
                .iter()
                .map(|b| {
                    branch_label(b)
                        .map(|l| labels.iter().any(|x| x == l))
                        .unwrap_or(false)
                })
                .collect(),
            JoinSpec::Select(e) => {
                let n = eval_value(e, ctx, self.model).unwrap_or(branches.len() as i64);
                let n = n.max(0) as usize;
                let mut flags = vec![false; branches.len()];
                // The specific statements are selected randomly (12.3.6.1b).
                let mut order: Vec<usize> = (0..branches.len()).collect();
                let mut rng = crate::solver::Rng::seeded(self.seed ^ (g as u64) ^ 0x5bf0_3635);
                for k in (1..order.len()).rev() {
                    let j = (rng.next_u64() % (k as u64 + 1)) as usize;
                    order.swap(k, j);
                }
                for &idx in order.iter().take(n.min(order.len())) {
                    flags[idx] = true;
                }
                flags
            }
        };

        for (k, b) in branches.iter().enumerate() {
            if gating[k] {
                self.walk_activity(b, component.to_string(), depth, mode, g, ctx);
            }
        }
        if let Some(cont) = continuation {
            self.walk_stmt(cont, component, depth, mode, g, ctx);
        }
        for (k, b) in branches.iter().enumerate() {
            if !gating[k] {
                self.walk_activity(b, component.to_string(), depth, mode, g, ctx);
            }
        }
        self.enforce_contention(mark, g);
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
            ActivityStmt::Parallel { branches, join } => {
                self.walk_joined(branches, join, None, component, depth, StepMode::Parallel, ctx);
            }
            ActivityStmt::Schedule { branches, join } => {
                self.walk_joined(branches, join, None, component, depth, StepMode::Schedule, ctx);
            }
            ActivityStmt::Cover { body, .. } => {
                // Everything scheduled inside a `cover` block is observed for
                // the model's covergroups (PSS 19.2).
                let saved = self.in_cover;
                self.in_cover = true;
                if let Some(b) = body {
                    self.walk_activity(b, component.to_string(), depth, mode, group, ctx);
                }
                self.in_cover = saved;
            }
            ActivityStmt::Labeled { label, stmt } => {
                let saved = self.current_label.replace(label.clone());
                self.walk_stmt(stmt, component, depth, mode, group, ctx);
                self.current_label = saved;
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
            ActivityStmt::Repeat {
                count,
                index,
                body,
                is_replicate,
            } => {
                let n = eval_count(count, self.model, ctx).unwrap_or(1);
                let kind = if *is_replicate { "replicate" } else { "repeat" };
                if n > MAX_REPEAT {
                    self.warnings
                        .push(format!("{kind} count {} clamped to {}", n, MAX_REPEAT));
                }
                // `replicate` expands in place into n labelled copies (12.5),
                // each of which is a distinct activity statement.
                let saved_rep = self.current_replicated;
                self.current_replicated = *is_replicate;
                for k in 0..n.min(MAX_REPEAT) {
                    let mut local = ctx.clone();
                    if let Some(name) = index {
                        local.insert(name.clone(), k);
                    }
                    if *is_replicate {
                        // Copies are labelled, and an array-style label carries
                        // an index (12.3.6.1a2).
                        local.insert("__replicate_index".to_string(), k);
                    }
                    self.walk_activity(body, component.to_string(), depth, mode, group, &local);
                }
                self.current_replicated = saved_rep;
            }
            ActivityStmt::Foreach { var, iter, body } => {
                // `foreach (arr[i])` over a fixed-size array iterates its
                // elements (PSS 8.4); the extent was recorded as `arr.size`.
                let iter_path = match iter {
                    Expr::Path(p) => Some(p.segments.join(".")),
                    _ => None,
                };
                if let Some(path) = iter_path.as_ref() {
                    if let Some(n) = ctx.get(&format!("{}.size", path)).copied() {
                        for i in 0..n.max(0) {
                            let mut local = ctx.clone();
                            if !var.is_empty() {
                                local.insert(var.clone(), i);
                            }
                            self.walk_activity(
                                body,
                                component.to_string(),
                                depth,
                                mode,
                                group,
                                &local,
                            );
                        }
                        return;
                    }
                }
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
            label: self.current_label.clone(),
            replicated: self.current_replicated,
            satisfied: 0,
            total: 0,
            unsatisfied: Vec::new(),
            flow: Vec::new(),
            claims: Vec::new(),
            unresolved: true,
        });
    }

    /// Evaluate a component hierarchy's `init_down`/`init_up` blocks into the
    /// scenario's globals (PSS 22.1).
    fn run_component_inits(&mut self, comp_name: &str, visited: &mut Vec<String>) {
        if visited.iter().any(|v| v == comp_name) {
            return;
        }
        visited.push(comp_name.to_string());
        let (subs, downs, ups) = match self.model.components.get(comp_name) {
            Some(c) => (
                c.subcomponents.clone(),
                c.execs.iter().filter(|e| e.kind == "init_down").cloned().collect::<Vec<_>>(),
                c.execs.iter().filter(|e| e.kind == "init_up").cloned().collect::<Vec<_>>(),
            ),
            None => return,
        };
        // init_down: this component first, then its children.
        for e in &downs {
            let mut g = std::mem::take(&mut self.globals);
            self.exec_stmts(&e.stmts, &mut g);
            self.globals = g;
        }
        for (_, sub_ty) in &subs {
            self.run_component_inits(sub_ty, visited);
        }
        // init_up: children first, then this component.
        for e in &ups {
            let mut g = std::mem::take(&mut self.globals);
            self.exec_stmts(&e.stmts, &mut g);
            self.globals = g;
        }
    }

    /// Execute `exec` statements against an instance's values (PSS 10.4).
    fn exec_stmts(&self, stmts: &[ExecStmt], values: &mut BTreeMap<String, i64>) {
        for st in stmts {
            match st {
                ExecStmt::Assign(path, rhs) => {
                    let base = path.segments.join(".");
                    // `L = {a, b, c};` gives a collection its elements (PSS 8.5).
                    if let Expr::List(items) = rhs {
                        for i in 0..MAX_LIST {
                            values.remove(&format!("{}[{}]", base, i));
                        }
                        for (i, it) in items.iter().enumerate() {
                            if let Some(v) = eval_value(it, values, self.model) {
                                values.insert(format!("{}[{}]", base, i), v);
                            }
                        }
                        values.insert(format!("{}.size", base), items.len() as i64);
                        continue;
                    }
                    if let Some(v) = eval_value(rhs, values, self.model) {
                        values.insert(base, v);
                    }
                }
                ExecStmt::If(cond, then, els) => match eval_condition_opt(cond, values, self.model) {
                    Some(true) => self.exec_stmts(then, values),
                    Some(false) => self.exec_stmts(els, values),
                    None => {}
                },
                // Collection mutators (PSS 7.9.3.2): the receiver's elements are
                // kept as `recv[i]` with a `recv.size`, so a list can be built
                // by an `exec` block and then read by constraints.
                ExecStmt::Call(name, args) => self.exec_call(name, args, values),
                ExecStmt::Raw(_) => {}
            }
        }
    }

    /// One `exec` method call, e.g. `q.push_back(v)`.
    fn exec_call(&self, name: &str, args: &[Expr], values: &mut BTreeMap<String, i64>) {
        let (recv, method) = match name.rsplit_once('.') {
            Some(parts) => parts,
            None => return,
        };
        let size_key = format!("{}.size", recv);
        let n = *values.get(&size_key).unwrap_or(&0);
        let elem = |i: i64| format!("{}[{}]", recv, i);
        let arg = |k: usize| args.get(k).and_then(|a| eval_value(a, values, self.model));
        match method {
            "push_back" => {
                if let Some(v) = arg(0) {
                    values.insert(elem(n), v);
                    values.insert(size_key, n + 1);
                }
            }
            "push_front" => {
                if let Some(v) = arg(0) {
                    for i in (0..n).rev() {
                        if let Some(x) = values.get(&elem(i)).copied() {
                            values.insert(elem(i + 1), x);
                        }
                    }
                    values.insert(elem(0), v);
                    values.insert(size_key, n + 1);
                }
            }
            "pop_back" => {
                if n > 0 {
                    values.remove(&elem(n - 1));
                    values.insert(size_key, n - 1);
                }
            }
            "pop_front" => {
                if n > 0 {
                    for i in 1..n {
                        if let Some(x) = values.get(&elem(i)).copied() {
                            values.insert(elem(i - 1), x);
                        }
                    }
                    values.remove(&elem(n - 1));
                    values.insert(size_key, n - 1);
                }
            }
            "insert" => {
                if let (Some(i), Some(v)) = (arg(0), arg(1)) {
                    if (0..=n).contains(&i) {
                        for j in (i..n).rev() {
                            if let Some(x) = values.get(&elem(j)).copied() {
                                values.insert(elem(j + 1), x);
                            }
                        }
                        values.insert(elem(i), v);
                        values.insert(size_key, n + 1);
                    }
                }
            }
            "delete" => {
                if let Some(i) = arg(0) {
                    if (0..n).contains(&i) {
                        for j in i..n - 1 {
                            if let Some(x) = values.get(&elem(j + 1)).copied() {
                                values.insert(elem(j), x);
                            }
                        }
                        values.remove(&elem(n - 1));
                        values.insert(size_key, n - 1);
                    }
                }
            }
            "clear" => {
                for i in 0..n {
                    values.remove(&elem(i));
                }
                values.insert(size_key, 0);
            }
            "shuffle" => {
                // Deterministic permutation from the engine's seed.
                let mut rng = crate::solver::Rng::seeded(self.seed ^ (n as u64) ^ 0x5eed_5eed);
                let mut order: Vec<i64> = (0..n).collect();
                for k in (1..order.len()).rev() {
                    let j = (rng.next_u64() % (k as u64 + 1)) as usize;
                    order.swap(k, j);
                }
                let vals: Vec<i64> = order
                    .iter()
                    .filter_map(|&i| values.get(&elem(i)).copied())
                    .collect();
                for (i, v) in vals.into_iter().enumerate() {
                    values.insert(elem(i as i64), v);
                }
            }
            _ => {}
        }
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
        // `exec pre_solve` runs before the solver and its assignments to
        // non-random fields feed the solve (PSS 16.4.12).
        let mut pre: BTreeMap<String, i64> = self.globals.clone();
        for (k, v) in bindings {
            pre.insert(k.clone(), *v);
        }
        for e in action.execs.iter().filter(|e| e.kind == "pre_solve") {
            self.exec_stmts(&e.stmts, &mut pre);
        }
        let report = solver::solve_with(
            self.model,
            action,
            self.seed ^ (self.steps.len() as u64),
            extra,
            &pre,
        );

        // `exec post_solve` runs after the solver and sets non-random fields
        // from the solved values.
        let mut solved = report.assignment.values.clone();
        // Values an `exec` block established for things the solver does not own
        // (collection elements and lengths) must survive into the instance.
        for (k, v) in &pre {
            solved.entry(k.clone()).or_insert(*v);
        }
        for e in action.execs.iter().filter(|e| e.kind == "post_solve") {
            self.exec_stmts(&e.stmts, &mut solved);
        }

        let mut fields: Vec<(String, i64)> = Vec::new();
        for (k, v) in &solved {
            if k.matches('.').count() <= 1 {
                fields.push((k.clone(), *v));
            }
        }

        // Sample the model's coverpoints and crosses against this instance
        // (PSS 18.3, 18.4).
        if self.in_cover {
            for cg in &self.model.covergroups {
                // Values sampled for each labelled coverpoint, so a cross can
                // combine them.
                let mut sampled: BTreeMap<String, i64> = BTreeMap::new();
                for item in &cg.items {
                    if let crate::ast::CoverItem::Coverpoint { label, expr, iff, bins } = item {
                        if let Some(cond) = iff {
                            if eval_condition_opt(cond, &solved, self.model) != Some(true) {
                                continue;
                            }
                        }
                        if let Some(v) = eval_value(expr, &solved, self.model) {
                            let point = match label {
                                Some(l) => format!("{}.{}", cg.name, l),
                                None => format!("{}.{}", cg.name, crate::solver::render_expr(expr)),
                            };
                            if let Some(l) = label {
                                sampled.insert(l.clone(), v);
                            }
                            *self
                                .coverage
                                .entry(point.clone())
                                .or_default()
                                .entry(v.to_string())
                                .or_insert(0) += 1;
                            // Record the declared bins so coverage can be
                            // reported against them (18.3).
                            if !bins.is_empty() {
                                let decl = self.coverage_bins.entry(point).or_default();
                                if decl.len() != bins.len() {
                                    decl.clear();
                                    for (_, bexpr) in bins {
                                        decl.push(bin_values(bexpr, &solved, self.model));
                                    }
                                }
                            }
                        }
                    }
                }
                for item in &cg.items {
                    if let crate::ast::CoverItem::Cross { label, refs, iff } = item {
                        if let Some(cond) = iff {
                            if eval_condition_opt(cond, &solved, self.model) != Some(true) {
                                continue;
                            }
                        }
                        // A cross needs every referenced coverpoint to have been
                        // sampled for this instance.
                        let mut parts = Vec::new();
                        for r in refs {
                            match sampled.get(r) {
                                Some(v) => parts.push(v.to_string()),
                                None => {
                                    parts.clear();
                                    break;
                                }
                            }
                        }
                        if parts.is_empty() {
                            continue;
                        }
                        let point = match label {
                            Some(l) => format!("{}.{}", cg.name, l),
                            None => format!("{}.cross({})", cg.name, refs.join(",")),
                        };
                        *self
                            .coverage
                            .entry(point)
                            .or_default()
                            .entry(parts.join(","))
                            .or_insert(0) += 1;
                    }
                }
            }
        }

        // Allocate / consume flow objects and claim resources. A flow object
        // may only be consumed after it has been produced (PSS 13), so inputs
        // are bound to the earliest unconsumed object of that type produced by
        // an earlier step.
        let mut flow = Vec::new();
        let mut claims = Vec::new();
        let step_no = self.steps.len();
        for f in &action.fields {
            match f.kind {
                FieldKind::Output => {
                    let mut rec = self.allocate(f, &solved, "output");
                    // With no pool bound, give the object an identity so a later
                    // input can bind to it. A pool that refused the allocation
                    // keeps `obj_id: None`.
                    if rec.obj_id.is_none() && rec.pool.is_none() {
                        rec.obj_id = Some(self.flow_seq);
                        self.flow_seq += 1;
                    }
                    // Record it as produced only if the allocation succeeded;
                    // a refused allocation produces nothing to bind to.
                    if let Some(id) = rec.obj_id {
                        self.flow_produced
                            .entry(f.type_name.clone())
                            .or_default()
                            .push((step_no, id, false));
                    }
                    flow.push(rec);
                }
                FieldKind::Input => {
                    let mut rec = self.consume(f, "input");
                    // Several inputs may reference the same flow object (a
                    // buffer is shared), so the object is not marked used up;
                    // it only has to have been produced by an earlier step.
                    let avail = self
                        .flow_produced
                        .get(&f.type_name)
                        .and_then(|v| v.iter().find(|(at, _, _)| *at < step_no));
                    match avail {
                        Some((_, id, _)) => {
                            if rec.obj_id.is_none() {
                                rec.obj_id = Some(*id);
                            }
                        }
                        None => {
                            self.warnings.push(format!(
                                "flow-object ordering: input `{}` of type `{}` has no \
                                 preceding output (PSS 13)",
                                f.name, f.type_name
                            ));
                        }
                    }
                    flow.push(rec);
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
            label: self.current_label.clone(),
            replicated: self.current_replicated,
            satisfied: report.satisfied,
            total: report.total,
            unsatisfied: report.unsupported.clone(),
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
