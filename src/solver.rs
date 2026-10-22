//! A pragmatic constraint solver for the PSS constraint subset.
//!
//! Satya's solver is a *domain-propagation* engine with a seeded random
//! assignment step. It handles the constructs that dominate real PSS models:
//!
//! * equality and inequality between fields and constants,
//! * ordering (`<`, `<=`, `>`, `>=`),
//! * membership (`in [lo..hi]`, `in {a, b, c}`),
//! * conjunction (`&&`) and implication (`->`),
//! * negation of a boolean field (`!x`),
//! * enum values (solved as their variant index).
//!
//! It is deliberately explicit about its limits: arithmetic relations between
//! two variables (for example `a + b == 10`) are recognised but not solved;
//! they are reported as unsupported rather than silently ignored.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ast::Expr;
use crate::ir::{Action, Field, Model, TypeCategory};

/// Upper bound applied to a bare `int` field when no width is declared.
const DEFAULT_INT_MAX: i64 = 1023;
/// Recursion limit when expanding nested structs into leaf variables.
const MAX_EXPAND_DEPTH: usize = 6;

#[derive(Debug, Clone)]
pub struct Assignment {
    pub values: BTreeMap<String, i64>,
}

impl Assignment {
    pub fn get(&self, name: &str) -> Option<i64> {
        self.values.get(name).copied()
    }
}

#[derive(Debug, Clone)]
pub struct SolveReport {
    pub assignment: Assignment,
    pub satisfied: usize,
    pub total: usize,
    pub unsupported: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Le,
    Ge,
    Lt,
    Gt,
}

#[derive(Debug, Clone)]
enum Atom {
    Rel(String, Op, Expr),
    InRange(String, i64, i64),
    InSet(String, Vec<Expr>),
    Implies(Expr, Vec<Atom>),
    /// A relation between two expressions, neither of which is a bare
    /// variable (e.g. `a + b == c`). Solved by single-unknown inversion.
    General(Expr, Op, Expr),
    Unsupported(Expr),
}

#[derive(Debug, Clone)]
struct Domain {
    lo: i64,
    hi: i64,
    forbidden: HashSet<i64>,
    #[allow(dead_code)]
    is_enum: bool,
}

impl Domain {
    fn contains(&self, v: i64) -> bool {
        v >= self.lo && v <= self.hi && !self.forbidden.contains(&v)
    }
    fn intersect_interval(&mut self, lo: i64, hi: i64) {
        self.lo = self.lo.max(lo);
        self.hi = self.hi.min(hi);
    }
}

/// Small, dependency-free linear-congruential RNG so runs are reproducible.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 17
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo + 1) as u64;
        lo + (self.next_u64() % span) as i64
    }
}

/// A resolved execution environment: constants and enum-variant indices.
pub struct Env<'a> {
    pub constants: &'a HashMap<String, i64>,
    pub variants: HashMap<String, i64>,
}

impl<'a> Env<'a> {
    pub fn new(model: &'a Model) -> Self {
        let mut variants = HashMap::new();
        for vars in model.enums.values() {
            for (i, v) in vars.iter().enumerate() {
                variants.entry(v.clone()).or_insert(i as i64);
            }
        }
        Env {
            constants: &model.constants,
            variants,
        }
    }

    pub fn lookup(&self, name: &str) -> Option<i64> {
        if let Some(v) = self.constants.get(name) {
            return Some(*v);
        }
        self.variants.get(name).copied()
    }
}

/// Evaluate an expression to an integer under a given assignment.
pub fn eval(expr: &Expr, values: &BTreeMap<String, i64>, env: &Env) -> Option<i64> {
    match expr {
        Expr::Int(v) => Some(*v),
        Expr::Bool(b) => Some(if *b { 1 } else { 0 }),
        Expr::Path(p) => {
            let joined = p.segments.join(".");
            if let Some(v) = values.get(&joined) {
                return Some(*v);
            }
            if p.segments.len() == 1 {
                let n = &p.segments[0];
                if let Some(v) = values.get(n) {
                    return Some(*v);
                }
                return env.lookup(n);
            }
            // Relative reference: match by trailing segment if unambiguous.
            let last = p.segments.last().unwrap();
            let matches: Vec<i64> = values
                .iter()
                .filter(|(k, _)| k == &last || k.ends_with(&format!(".{}", last)))
                .map(|(_, v)| *v)
                .collect();
            if matches.len() == 1 {
                return Some(matches[0]);
            }
            env.lookup(last)
        }
        Expr::Slice(base, msb, lsb) => {
            let b = eval(base, values, env)?;
            let hi = eval(msb, values, env)?;
            let lo = eval(lsb, values, env)?;
            if hi < lo || lo < 0 || hi - lo + 1 > 62 {
                return None;
            }
            let width = (hi - lo + 1) as u32;
            let mask = (1i64 << width) - 1;
            Some((b >> lo) & mask)
        }
        Expr::Unary(op, e) => {
            let v = eval(e, values, env)?;
            match op.as_str() {
                "-" => Some(-v),
                "+" => Some(v),
                "!" => Some(if v == 0 { 1 } else { 0 }),
                "~" => Some(!v),
                _ => None,
            }
        }
        Expr::Cast(_, e) => eval(e, values, env),
        Expr::Binary(op, a, b) => {
            let x = eval(a, values, env)?;
            let y = eval(b, values, env)?;
            match op.as_str() {
                "+" => Some(x + y),
                "-" => Some(x - y),
                "*" => Some(x * y),
                "/" if y != 0 => Some(x / y),
                "%" if y != 0 => Some(x % y),
                "<<" => Some(x << y),
                ">>" => Some(x >> y),
                "&" => Some(x & y),
                "|" => Some(x | y),
                "^" => Some(x ^ y),
                "==" => Some((x == y) as i64),
                "!=" => Some((x != y) as i64),
                "<" => Some((x < y) as i64),
                "<=" => Some((x <= y) as i64),
                ">" => Some((x > y) as i64),
                ">=" => Some((x >= y) as i64),
                "&&" => Some(((x != 0) && (y != 0)) as i64),
                "||" => Some(((x != 0) || (y != 0)) as i64),
                "->" => Some((!(x != 0) || (y != 0)) as i64),
                _ => None,
            }
        }
        Expr::Ternary(c, t, e) => {
            if eval(c, values, env)? != 0 {
                eval(t, values, env)
            } else {
                eval(e, values, env)
            }
        }
        _ => None,
    }
}

/// Solve the constraints of `action` and return a concrete assignment.
pub fn solve(model: &Model, action: &Action, seed: u64) -> SolveReport {
    solve_with(model, action, seed, &[], &BTreeMap::new())
}

/// Solve the constraints of `action`, additionally applying `extra` (inline
/// `do … with { … }` constraints) and treating `bindings` as known values
/// (for example a `repeat (i : n)` index).
pub fn solve_with(
    model: &Model,
    action: &Action,
    seed: u64,
    extra: &[Expr],
    bindings: &BTreeMap<String, i64>,
) -> SolveReport {
    let env = Env::new(model);
    let mut vars = collect_variables(model, action);
    let constraints = collect_constraints(model, action, &vars, extra);
    let atoms = extract_atoms(&constraints);

    // Union-find over variables to merge equality classes.
    let mut uf = UnionFind::new(vars.keys().cloned().collect());

    // Apply relational atoms.
    let mut unsupported = Vec::new();
    for atom in &atoms {
        match atom {
            Atom::Rel(var, Op::Eq, rhs) => {
                // x == const  or  x == y
                if let Some(other) = as_var(rhs) {
                    if vars.contains_key(&other) {
                        uf.union(var, &other);
                    } else if let Some(v) = eval(rhs, bindings, &env) {
                        tighten(&mut vars, var, v, v);
                    }
                } else if let Some(v) = eval(rhs, bindings, &env) {
                    tighten(&mut vars, var, v, v);
                }
            }
            Atom::Rel(var, Op::Ne, rhs) => {
                if let Some(v) = eval(rhs, bindings, &env) {
                    if let Some(d) = vars.get_mut(var) {
                        d.forbidden.insert(v);
                    }
                }
            }
            Atom::Rel(var, op, rhs) => {
                if let Some(v) = eval(rhs, bindings, &env) {
                    let d = vars.get_mut(var);
                    if let Some(d) = d {
                        match op {
                            Op::Le => d.intersect_interval(i64::MIN, v),
                            Op::Lt => d.intersect_interval(i64::MIN, v - 1),
                            Op::Ge => d.intersect_interval(v, i64::MAX),
                            Op::Gt => d.intersect_interval(v + 1, i64::MAX),
                            _ => {}
                        }
                    }
                }
            }
            Atom::InRange(var, lo, hi) => tighten(&mut vars, var, *lo, *hi),
            Atom::InSet(var, items) => {
                let mut allowed = Vec::new();
                for it in items {
                    if let Some(v) = eval(it, bindings, &env) {
                        allowed.push(v);
                    }
                }
                if !allowed.is_empty() {
                    if let Some(d) = vars.get_mut(var) {
                        let lo = *allowed.iter().min().unwrap();
                        let hi = *allowed.iter().max().unwrap();
                        d.intersect_interval(lo, hi);
                        for v in d.lo..=d.hi {
                            if !allowed.contains(&v) {
                                d.forbidden.insert(v);
                            }
                        }
                    }
                }
            }
            Atom::Implies(_, _) => {}
            Atom::General(_, _, _) => {}
            Atom::Unsupported(e) => unsupported.push(render_expr(e)),
        }
    }

    // Bound propagation between related variables (`a < b`, `a <= b`, ...).
    // Single-variable repair alone can get stuck in a local minimum when both
    // sides are random; narrowing the domains first avoids that.
    for _ in 0..8 {
        let mut changed = false;
        for atom in &atoms {
            let (var, op, rhs) = match atom {
                Atom::Rel(v, o, r) => (v, *o, r),
                _ => continue,
            };
            let other = match as_var(rhs) {
                Some(o) => o,
                None => continue,
            };
            let (a, b) = match (vars.get(var).cloned(), vars.get(&other).cloned()) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };
            let (alo, ahi, blo, bhi) = match op {
                Op::Lt => (i64::MIN, b.hi - 1, a.lo + 1, i64::MAX),
                Op::Le => (i64::MIN, b.hi, a.lo, i64::MAX),
                Op::Gt => (b.lo + 1, i64::MAX, i64::MIN, a.hi - 1),
                Op::Ge => (b.lo, i64::MAX, i64::MIN, a.hi),
                _ => continue,
            };
            let before = (a.lo, a.hi, b.lo, b.hi);
            tighten(&mut vars, var, alo, ahi);
            tighten(&mut vars, &other, blo, bhi);
            let now = (
                vars.get(var).map(|d| (d.lo, d.hi)).unwrap_or((before.0, before.1)),
                vars.get(&other).map(|d| (d.lo, d.hi)).unwrap_or((before.2, before.3)),
            );
            if now.0 != (before.0, before.1) || now.1 != (before.2, before.3) {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Merge domains across equality classes.
    let mut class_domain: HashMap<String, Domain> = HashMap::new();
    for (name, d) in &vars {
        let root = uf.find(name);
        class_domain
            .entry(root)
            .and_modify(|cd| {
                cd.intersect_interval(d.lo, d.hi);
                for f in &d.forbidden {
                    cd.forbidden.insert(*f);
                }
            })
            .or_insert_with(|| d.clone());
    }

    // A class whose domain became empty has contradictory constraints; report it
    // rather than silently assigning a value that satisfies nothing.
    for (root, d) in &class_domain {
        if d.lo > d.hi {
            let name = vars.keys().find(|k| uf.find(k) == *root).cloned().unwrap_or_else(|| root.clone());
            unsupported.push(format!("conflicting constraints on `{}`", name));
        }
    }

    // Assign a value per class.
    let mut rng = Rng::new(seed ^ 0x9E37_79B9_7F4A_7C15);
    let mut values: BTreeMap<String, i64> = BTreeMap::new();
    let mut class_value: HashMap<String, i64> = HashMap::new();
    for name in vars.keys() {
        let root = uf.find(name);
        let v = *class_value.entry(root.clone()).or_insert_with(|| {
            let d = class_domain.get(&root).cloned().unwrap_or(Domain {
                lo: 0,
                hi: 0,
                forbidden: HashSet::new(),
                is_enum: false,
            });
            pick(&d, &mut rng)
        });
        values.insert(name.clone(), v);
    }

    // The repair pass adjusts one variable at a time, so give it a name-keyed
    // view of the class domains (class_domain itself is keyed by union-find root).
    let mut var_domain: HashMap<String, Domain> = HashMap::new();
    for name in vars.keys() {
        let root = uf.find(name);
        if let Some(d) = class_domain.get(&root) {
            var_domain.insert(name.clone(), d.clone());
        }
    }

    // Arithmetic repair: try to satisfy relations that domain propagation
    // could not handle (e.g. `a + b == 10`) by adjusting one variable.
    repair(&atoms, &mut values, bindings, &env, &var_domain, &uf);

    // Count satisfied atoms and resolve implications with the assignment.
    let mut check_values = values.clone();
    for (k, v) in bindings {
        check_values.insert(k.clone(), *v);
    }
    let mut satisfied = 0usize;
    let mut total = 0usize;
    for atom in &atoms {
        match atom {
            Atom::Unsupported(_) => {}
            Atom::Implies(ante, cons) => {
                total += 1;
                let a = eval(ante, &check_values, &env).unwrap_or(0) != 0;
                if !a {
                    satisfied += 1; // vacuously true
                } else {
                    let ok = cons.iter().all(|c| check_atom(c, &check_values, &env, &class_domain));
                    if ok {
                        satisfied += 1;
                    } else {
                        unsupported.push(format!(
                            "{} -> {}",
                            render_expr(ante),
                            cons.iter().map(render_atom).collect::<Vec<_>>().join(" && ")
                        ));
                    }
                }
            }
            Atom::General(l, op, r) => {
                match (eval(l, &check_values, &env), eval(r, &check_values, &env)) {
                    (Some(x), Some(y)) => {
                        total += 1;
                        if rel_holds(*op, x, y) {
                            satisfied += 1;
                        }
                    }
                    _ => unsupported.push(render_expr(l)),
                }
            }
            other => {
                total += 1;
                if check_atom(other, &check_values, &env, &class_domain) {
                    satisfied += 1;
                } else {
                    unsupported.push(render_atom(other));
                }
            }
        }
    }

    let mut notes = Vec::new();
    if !unsupported.is_empty() {
        notes.push(format!(
            "{} constraint(s) use constructs outside the solved subset",
            unsupported.len()
        ));
    }

    SolveReport {
        assignment: Assignment { values },
        satisfied,
        total,
        unsupported,
        notes,
    }
}

/// Iteratively repair the assignment against relations that require
/// arithmetic reasoning. Each pass fixes one violated relation by adjusting a
/// single variable; a handful of passes converges for the common cases.
fn repair(
    atoms: &[Atom],
    values: &mut BTreeMap<String, i64>,
    bindings: &BTreeMap<String, i64>,
    env: &Env,
    class_domain: &HashMap<String, Domain>,
    uf: &UnionFind,
) {
    // Fit a computed value into a variable's domain. For an unsigned bit field
    // the value wraps (two's-complement truncation), which is what bit-wise
    // operations such as `~` mean on a `bit[N]` — clamping would turn `~x` into
    // 0 for every 32-bit x.
    let clamp = |name: &str, v: i64| -> i64 {
        match class_domain.get(name) {
            // A contradictory model leaves an empty domain; do not panic.
            Some(d) if d.lo > d.hi => v,
            Some(d) if v >= d.lo && v <= d.hi => v,
            Some(d) if d.lo == 0 && d.hi > 0 => v & d.hi,
            Some(d) => v.clamp(d.lo, d.hi),
            None => v,
        }
    };
    for _ in 0..32 {
        let mut changed = false;
        for atom in atoms {
            match atom {
                Atom::Implies(ante, cons) => {
                    // If the antecedent holds, the consequent must too.
                    if eval(ante, values, env).unwrap_or(0) != 0 {
                        for c in cons {
                            if check_atom(c, values, env, class_domain) {
                                continue;
                            }
                            if let Atom::Rel(v, Op::Eq, rhs) = c {
                                if let Some(t) = eval(rhs, values, env) {
                                    let nv = clamp(v, t);
                                    if values.get(v).copied() != Some(nv) {
                                        values.insert(v.clone(), nv);
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
                Atom::Rel(var, op, rhs) => {
                    let x = match values.get(var) {
                        Some(v) => *v,
                        None => continue,
                    };
                    match eval(rhs, values, env) {
                        Some(y) => {
                            if !rel_holds(*op, x, y) {
                                let fits = |name: &str, v: i64| {
                                    class_domain
                                        .get(name)
                                        .map(|d| v >= d.lo && v <= d.hi)
                                        .unwrap_or(true)
                                };
                                // First try to move the left-hand variable ...
                                // Compare at the field's width when the right
                                // side is bit-wise (`c == ~v`): the moved
                                // value is correct modulo the field, not as a
                                // 64-bit integer.
                                let y_cmp = if contains_bitwise(rhs) {
                                    match class_domain.get(var) {
                                        Some(d) if d.lo == 0 && d.hi > 0 => y & d.hi,
                                        _ => y,
                                    }
                                } else {
                                    y
                                };
                                let moved = adjust(x, *op, y)
                                    .map(|nv| clamp(var, nv))
                                    .filter(|v| {
                                        // Only accept a move that actually
                                        // repairs the relation.
                                        *v != x
                                            && rel_holds(*op, *v, y_cmp)
                                            && fits(var, *v)
                                            && !bindings.contains_key(var)
                                    });
                                match moved {
                                    Some(v) => {
                                        values.insert(var.clone(), v);
                                        changed = true;
                                    }
                                    // ... but if it is pinned by another
                                    // constraint, move the right-hand side.
                                    None => {
                                        if let Some(other) = as_var(rhs) {
                                            if !bindings.contains_key(&other) {
                                                let target = match op {
                                                    Op::Lt => Some(x + 1),
                                                    Op::Le => Some(x),
                                                    Op::Gt => Some(x - 1),
                                                    Op::Ge => Some(x),
                                                    _ => None,
                                                };
                                                if let Some(t) = target {
                                                    let v = clamp(&other, t);
                                                    if fits(&other, v)
                                                        && rel_holds(*op, x, v)
                                                        && values.get(&other).copied() != Some(v)
                                                    {
                                                        values.insert(other.clone(), v);
                                                        changed = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            // Solve for the single unknown inside the rhs.
                            if matches!(op, Op::Eq) {
                                let unks = collect_unknowns(rhs, values, env);
                                let unks: Vec<String> = unks
                                    .into_iter()
                                    .filter(|u| !bindings.contains_key(u))
                                    .collect();
                                if unks.len() == 1 {
                                    if let Some(nv) = invert_eq(rhs, x, &unks[0], values, env) {
                                        values.insert(unks[0].clone(), clamp(&unks[0], nv));
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
                // An ordering between two variables (`a < b`): nudge one of them.
                Atom::General(l, op, r)
                    if matches!(op, Op::Lt | Op::Le | Op::Gt | Op::Ge) =>
                {
                    if let (Some(a), Some(b)) = (as_var(l), as_var(r)) {
                        let (xa, xb) = match (values.get(&a), values.get(&b)) {
                            (Some(x), Some(y)) => (*x, *y),
                            _ => continue,
                        };
                        if rel_holds(*op, xa, xb) {
                            continue;
                        }
                        // Prefer to move the right-hand variable, then the left.
                        let for_right = match op {
                            Op::Lt => Some(xa + 1),
                            Op::Le => Some(xa),
                            Op::Gt => Some(xa - 1),
                            Op::Ge => Some(xa),
                            _ => None,
                        };
                        let for_left = match op {
                            Op::Lt => Some(xb - 1),
                            Op::Le => Some(xb),
                            Op::Gt => Some(xb + 1),
                            Op::Ge => Some(xb),
                            _ => None,
                        };
                        let fixed_right = for_right
                            .filter(|_| !bindings.contains_key(&b))
                            .map(|t| clamp(&b, t))
                            .filter(|v| {
                                class_domain.get(&b).map(|d| *v >= d.lo && *v <= d.hi).unwrap_or(true)
                            });
                        match fixed_right {
                            Some(v) => {
                                values.insert(b.clone(), v);
                                changed = true;
                            }
                            None => {
                                if let Some(v) = for_left
                                    .filter(|_| !bindings.contains_key(&a))
                                    .map(|t| clamp(&a, t))
                                    .filter(|v| {
                                        class_domain.get(&a).map(|d| *v >= d.lo && *v <= d.hi).unwrap_or(true)
                                    })
                                {
                                    values.insert(a.clone(), v);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
                Atom::General(l, Op::Eq, r) => {
                    match (eval(l, values, env), eval(r, values, env)) {
                        (Some(a), Some(b)) if a != b => {
                            // Everything is known but the relation fails:
                            // re-solve one participating variable against the
                            // other side.
                            let mut chosen: Option<(String, i64)> = None;
                            for v in vars_in(l, values).iter().filter(|v| !bindings.contains_key(*v)) {
                                if let Some(raw) = invert_eq(l, b, v, values, env) {
                                    if chosen.is_none() {
                                        chosen = Some((v.clone(), clamp(v, raw)));
                                    }
                                    if domain_ok(v, raw, class_domain) {
                                        chosen = Some((v.clone(), raw));
                                        break;
                                    }
                                }
                            }
                            if chosen.is_none() {
                                for v in vars_in(r, values).iter().filter(|v| !bindings.contains_key(*v)) {
                                    if let Some(raw) = invert_eq(r, a, v, values, env) {
                                        chosen = Some((v.clone(), clamp(v, raw)));
                                        if domain_ok(v, raw, class_domain) {
                                            chosen = Some((v.clone(), raw));
                                            break;
                                        }
                                    }
                                }
                            }
                            if let Some((v, nv)) = chosen {
                                values.insert(v, nv);
                                changed = true;
                            }
                        }
                        (None, Some(b)) => {
                            let lu: Vec<String> = collect_unknowns(l, values, env)
                                .into_iter()
                                .filter(|u| !bindings.contains_key(u))
                                .collect();
                            if lu.len() == 1 {
                                if let Some(nv) = invert_eq(l, b, &lu[0], values, env) {
                                    values.insert(lu[0].clone(), clamp(&lu[0], nv));
                                    changed = true;
                                }
                            }
                        }
                        (Some(a), None) => {
                            let ru: Vec<String> = collect_unknowns(r, values, env)
                                .into_iter()
                                .filter(|u| !bindings.contains_key(u))
                                .collect();
                            if ru.len() == 1 {
                                if let Some(nv) = invert_eq(r, a, &ru[0], values, env) {
                                    values.insert(ru[0].clone(), clamp(&ru[0], nv));
                                    changed = true;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }
    let _ = uf;
}

fn check_atom(
    atom: &Atom,
    values: &BTreeMap<String, i64>,
    env: &Env,
    domains: &HashMap<String, Domain>,
) -> bool {
    match atom {
        Atom::Rel(var, op, rhs) => {
            let x = match values.get(var) {
                Some(v) => *v,
                None => return true,
            };
            let y = match eval(rhs, values, env) {
                Some(v) => v,
                None => return true,
            };
            // A bit-wise expression (`~x`, `x << n`, ...) yields a value at the
            // width of the field it is assigned to, so compare it at that width.
            let y = if contains_bitwise(rhs) {
                match domains.get(var) {
                    Some(d) if d.lo == 0 && d.hi > 0 => y & d.hi,
                    _ => y,
                }
            } else {
                y
            };
            match op {
                Op::Eq => x == y,
                Op::Ne => x != y,
                Op::Le => x <= y,
                Op::Ge => x >= y,
                Op::Lt => x < y,
                Op::Gt => x > y,
            }
        }
        Atom::InRange(var, lo, hi) => values.get(var).map(|v| *v >= *lo && *v <= *hi).unwrap_or(true),
        Atom::InSet(var, items) => {
            let x = match values.get(var) {
                Some(v) => *v,
                None => return true,
            };
            items.iter().any(|it| eval(it, values, env) == Some(x))
        }
        Atom::General(l, op, r) => {
            match (eval(l, values, env), eval(r, values, env)) {
                (Some(x), Some(y)) => rel_holds(*op, x, y),
                _ => true,
            }
        }
        Atom::Implies(_, _) => true,
        Atom::Unsupported(_) => true,
    }
}

fn rel_holds(op: Op, x: i64, y: i64) -> bool {
    match op {
        Op::Eq => x == y,
        Op::Ne => x != y,
        Op::Le => x <= y,
        Op::Ge => x >= y,
        Op::Lt => x < y,
        Op::Gt => x > y,
    }
}

/// Move `x` to the nearest value satisfying `x op y`.
fn adjust(x: i64, op: Op, y: i64) -> Option<i64> {
    let v = match op {
        Op::Eq => y,
        Op::Ne => {
            if x != y {
                return None;
            }
            y.wrapping_add(1)
        }
        Op::Le => x.min(y),
        Op::Ge => x.max(y),
        Op::Lt => x.min(y - 1),
        Op::Gt => x.max(y + 1),
    };
    if v == x {
        None
    } else {
        Some(v)
    }
}

/// Is `raw` inside `var`'s current domain (interval and allowed set)?
fn domain_ok(var: &str, raw: i64, class_domain: &HashMap<String, Domain>) -> bool {
    match class_domain.get(var) {
        Some(d) => d.contains(raw),
        None => true,
    }
}

/// Is `name` referenced by this expression?
fn contains_var(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Path(p) => p.segments.join(".") == name || p.segments.last().map(|s| s == name).unwrap_or(false),
        Expr::Unary(_, x) => contains_var(x, name),
        Expr::Cast(_, x) => contains_var(x, name),
        Expr::Binary(_, a, b) => contains_var(a, name) || contains_var(b, name),
        Expr::Ternary(a, b, c) => contains_var(a, name) || contains_var(b, name) || contains_var(c, name),
        _ => false,
    }
}

/// Names referenced by an expression that are not yet known.
fn collect_unknowns(
    e: &Expr,
    values: &BTreeMap<String, i64>,
    env: &Env,
) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(e: &Expr, values: &BTreeMap<String, i64>, env: &Env, out: &mut Vec<String>) {
        match e {
            Expr::Path(p) => {
                let joined = p.segments.join(".");
                let known = values.contains_key(&joined)
                    || (p.segments.len() == 1
                        && (values.contains_key(&p.segments[0]) || env.lookup(&p.segments[0]).is_some()));
                if !known && !out.contains(&joined) {
                    out.push(joined);
                }
            }
            Expr::Unary(_, x) => walk(x, values, env, out),
            Expr::Cast(_, x) => walk(x, values, env, out),
            Expr::Binary(_, a, b) => {
                walk(a, values, env, out);
                walk(b, values, env, out);
            }
            Expr::Ternary(a, b, c) => {
                walk(a, values, env, out);
                walk(b, values, env, out);
                walk(c, values, env, out);
            }
            _ => {}
        }
    }
    walk(e, values, env, &mut out);
    out
}

/// Does this expression involve a bit-wise operator (whose result is taken at
/// the width of the target field rather than as a 64-bit integer)?
fn contains_bitwise(e: &Expr) -> bool {
    match e {
        Expr::Unary(op, x) => op == "~" || contains_bitwise(x),
        Expr::Binary(op, a, b) => {
            matches!(op.as_str(), "&" | "|" | "^" | "<<" | ">>")
                || contains_bitwise(a)
                || contains_bitwise(b)
        }
        Expr::Cast(_, x) => contains_bitwise(x),
        Expr::Slice(_, _, _) => true,
        _ => false,
    }
}

/// Render an atom as a constraint string, for diagnostics.
fn render_atom(a: &Atom) -> String {
    match a {
        Atom::Rel(v, op, e) => format!("{} {} {}", v, op_str(*op), render_expr(e)),
        Atom::General(l, op, e) => format!("{} {} {}", render_expr(l), op_str(*op), render_expr(e)),
        Atom::InRange(v, lo, hi) => format!("{} in [{}..{}]", v, lo, hi),
        Atom::InSet(v, items) => format!("{} in {{{}}}", v, items.len()),
        Atom::Implies(e, _) => render_expr(e),
        Atom::Unsupported(e) => render_expr(e),
    }
}

fn op_str(op: Op) -> &'static str {
    match op {
        Op::Eq => "==",
        Op::Ne => "!=",
        Op::Le => "<=",
        Op::Ge => ">=",
        Op::Lt => "<",
        Op::Gt => ">",
    }
}

/// Known variables (present in `values`) referenced by an expression, in
/// appearance order.
fn vars_in(e: &Expr, values: &BTreeMap<String, i64>) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(e: &Expr, values: &BTreeMap<String, i64>, out: &mut Vec<String>) {
        match e {
            Expr::Path(p) => {
                let joined = p.segments.join(".");
                let hit = if values.contains_key(&joined) {
                    Some(joined)
                } else if p.segments.len() == 1 && values.contains_key(&p.segments[0]) {
                    Some(p.segments[0].clone())
                } else {
                    None
                };
                if let Some(h) = hit {
                    if !out.contains(&h) {
                        out.push(h);
                    }
                }
            }
            Expr::Unary(_, x) | Expr::Cast(_, x) => walk(x, values, out),
            Expr::Binary(_, a, b) => {
                walk(a, values, out);
                walk(b, values, out);
            }
            Expr::Ternary(a, b, c) => {
                walk(a, values, out);
                walk(b, values, out);
                walk(c, values, out);
            }
            _ => {}
        }
    }
    walk(e, values, &mut out);
    out
}

/// Invert `expr == target` for a single unknown variable.
fn invert_eq(
    e: &Expr,
    target: i64,
    unknown: &str,
    values: &BTreeMap<String, i64>,
    env: &Env,
) -> Option<i64> {
    match e {
        Expr::Path(p) => {
            if p.segments.join(".") == unknown
                || p.segments.last().map(|s| s == unknown).unwrap_or(false)
            {
                Some(target)
            } else {
                None
            }
        }
        Expr::Unary(op, x) if op == "-" => invert_eq(x, -target, unknown, values, env),
        Expr::Unary(op, x) if op == "+" => invert_eq(x, target, unknown, values, env),
        // Bitwise complement: `~x == target` means `x == !target`.
        Expr::Unary(op, x) if op == "~" => invert_eq(x, !target, unknown, values, env),
        Expr::Cast(_, x) => invert_eq(x, target, unknown, values, env),
        Expr::Binary(op, l, r) => {
            let lh = contains_var(l, unknown);
            let rh = contains_var(r, unknown);
            match op.as_str() {
                "+" => {
                    if !lh {
                        invert_eq(r, target - eval(l, values, env)?, unknown, values, env)
                    } else if !rh {
                        invert_eq(l, target - eval(r, values, env)?, unknown, values, env)
                    } else {
                        None
                    }
                }
                "-" => {
                    if !rh {
                        invert_eq(l, target + eval(r, values, env)?, unknown, values, env)
                    } else if !lh {
                        invert_eq(r, eval(l, values, env)? - target, unknown, values, env)
                    } else {
                        None
                    }
                }
                "*" => {
                    if !lh {
                        let c = eval(l, values, env)?;
                        if c != 0 && target % c == 0 {
                            invert_eq(r, target / c, unknown, values, env)
                        } else {
                            None
                        }
                    } else if !rh {
                        let c = eval(r, values, env)?;
                        if c != 0 && target % c == 0 {
                            invert_eq(l, target / c, unknown, values, env)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                "/" => {
                    if !rh {
                        let c = eval(r, values, env)?;
                        if c != 0 {
                            invert_eq(l, target * c, unknown, values, env)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn pick(d: &Domain, rng: &mut Rng) -> i64 {
    if d.hi < d.lo {
        return d.lo;
    }
    for _ in 0..64 {
        let v = rng.range(d.lo, d.hi);
        if d.contains(v) {
            return v;
        }
    }
    // Fall back to the first allowed value.
    for v in d.lo..=d.hi {
        if d.contains(v) {
            return v;
        }
    }
    d.lo
}

fn tighten(vars: &mut BTreeMap<String, Domain>, name: &str, lo: i64, hi: i64) {
    if let Some(d) = vars.get_mut(name) {
        d.intersect_interval(lo, hi);
    }
}

fn as_var(e: &Expr) -> Option<String> {
    match e {
        Expr::Path(p) => Some(p.segments.join(".")),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Variable collection
// ---------------------------------------------------------------------------

fn collect_variables(model: &Model, action: &Action) -> BTreeMap<String, Domain> {
    let mut out = BTreeMap::new();
    let mut visited: Vec<String> = Vec::new();
    for f in &action.fields {
        expand_field(model, f, "", 0, &mut visited, &mut out);
    }
    out
}

fn expand_field(
    model: &Model,
    field: &Field,
    prefix: &str,
    depth: usize,
    visited: &mut Vec<String>,
    out: &mut BTreeMap<String, Domain>,
) {
    if depth > MAX_EXPAND_DEPTH {
        return;
    }
    let path = if prefix.is_empty() {
        field.name.clone()
    } else {
        format!("{}.{}", prefix, field.name)
    };
    match &field.category {
        TypeCategory::Scalar => {
            let hi = field
                .bits
                // Use the declared width (up to 62 bits); a narrower cap would
                // truncate bit-wise results such as `~x` on a `bit[32]`.
                .map(|b| (1i64 << b.clamp(1, 62)) - 1)
                .unwrap_or(DEFAULT_INT_MAX);
            out.insert(
                path,
                Domain {
                    lo: 0,
                    hi,
                    forbidden: HashSet::new(),
                    is_enum: false,
                },
            );
        }
        TypeCategory::Enum(variants) => {
            let hi = (variants.len().saturating_sub(1)) as i64;
            out.insert(
                path,
                Domain {
                    lo: 0,
                    hi,
                    forbidden: HashSet::new(),
                    is_enum: true,
                },
            );
        }
        TypeCategory::Struct(tname) => {
            if visited.contains(tname) {
                return;
            }
            if let Some(t) = model.types.get(tname) {
                visited.push(tname.clone());
                for sub in &t.fields {
                    expand_field(model, sub, &path, depth + 1, visited, out);
                }
                visited.pop();
            }
        }
        _ => {
            // Handles, components and unknown types are opaque to the solver.
        }
    }
}

// ---------------------------------------------------------------------------
// Constraint collection
// ---------------------------------------------------------------------------

/// Gather the constraints that apply to an action instance, applying the LRM's
/// rules for default constraints (PSS 16.1.10.1).
///
///  * a default value constraint behaves as an equality constraint (16.1.10.1a);
///  * `constraint default disable <attr>` removes the defaults on `<attr>`
///    (16.1.10.1b);
///  * when several defaults target the same attribute only one survives, by
///    precedence: a higher-level containing context beats a lower-level one, a
///    derived type beats a base type, and otherwise the later one wins
///    (16.1.10.1d). We collect action constraints before type constraints, so
///    "action beats type" is expressed as a lower level number.
fn collect_constraints(
    model: &Model,
    action: &Action,
    vars: &BTreeMap<String, Domain>,
    extra: &[Expr],
) -> Vec<Expr> {
    let mut regular: Vec<Expr> = Vec::new();
    // (expr, level): level 0 = the action's own context, 1 = a data type's.
    let mut defaults: Vec<(Expr, u8)> = Vec::new();
    let mut disabled: Vec<String> = Vec::new();

    for c in &action.constraints {
        if c.is_disable {
            if let Some(t) = constraint_target(&c.expr) {
                disabled.push(t);
            }
            continue;
        }
        if c.is_default {
            defaults.push((c.expr.clone(), 0));
        } else {
            regular.push(c.expr.clone());
        }
    }

    // Inline constraints from `do … with { … }` are always hard.
    for e in extra {
        regular.push(e.clone());
    }

    // Constraints declared on the types of this action's struct fields.
    let mut visited: Vec<String> = Vec::new();
    for f in &action.fields {
        collect_type_constraints(model, f, "", 0, &mut visited, vars, &mut regular, &mut defaults);
    }

    // Resolve the defaults: drop disabled ones, keep one per attribute.
    let mut chosen: BTreeMap<String, (u8, Expr)> = BTreeMap::new();
    for (e, level) in defaults {
        match constraint_target(&e) {
            Some(t) => {
                let is_disabled = disabled
                    .iter()
                    .any(|d| t == *d || t.starts_with(&format!("{}.", d)));
                if is_disabled {
                    continue;
                }
                match chosen.get(&t) {
                    // An existing constraint with equal or higher precedence wins.
                    Some((lvl, _)) if *lvl <= level => {}
                    _ => {
                        chosen.insert(t, (level, e));
                    }
                }
            }
            // Not an equality on a single attribute: treat it as hard.
            None => regular.push(e),
        }
    }
    for (_, (_, e)) in chosen {
        regular.push(e);
    }
    regular
}

/// The attribute a default constraint targets: the left-hand side of an
/// equality (`constraint default s2.attr1 == 2`), or a bare hierarchical id
/// (`constraint default disable s3.attr1`).
fn constraint_target(e: &Expr) -> Option<String> {
    match e {
        Expr::Path(p) => Some(p.segments.join(".")),
        Expr::Binary(op, l, _) if op == "==" => match &**l {
            Expr::Path(p) => Some(p.segments.join(".")),
            _ => None,
        },
        _ => None,
    }
}

fn collect_type_constraints(
    model: &Model,
    field: &Field,
    prefix: &str,
    depth: usize,
    visited: &mut Vec<String>,
    vars: &BTreeMap<String, Domain>,
    out: &mut Vec<Expr>,
    defaults: &mut Vec<(Expr, u8)>,
) {
    if depth > MAX_EXPAND_DEPTH {
        return;
    }
    let path = if prefix.is_empty() {
        field.name.clone()
    } else {
        format!("{}.{}", prefix, field.name)
    };
    if let TypeCategory::Struct(tname) = &field.category {
        if visited.contains(tname) {
            return;
        }
        if let Some(t) = model.types.get(tname) {
            visited.push(tname.clone());
            let locals: HashSet<String> = t.fields.iter().map(|f| f.name.clone()).collect();
            for c in &t.constraints {
                let e = prefix_paths(&c.expr, &path, &locals, vars);
                if c.is_default {
                    defaults.push((e, 1));
                } else {
                    out.push(e);
                }
            }
            for sub in &t.fields {
                collect_type_constraints(model, sub, &path, depth + 1, visited, vars, out, defaults);
            }
            visited.pop();
        }
    }
}

/// Prefix relative references in a struct's constraints with the instance path.
fn prefix_paths(
    expr: &Expr,
    prefix: &str,
    locals: &HashSet<String>,
    vars: &BTreeMap<String, Domain>,
) -> Expr {
    match expr {
        Expr::Path(p) => {
            let first = p.segments.first().cloned().unwrap_or_default();
            if locals.contains(&first) {
                let mut segs = p.segments.clone();
                let mut full = prefix.to_string();
                for s in segs.drain(..) {
                    full.push('.');
                    full.push_str(&s);
                }
                if vars.contains_key(&full) {
                    return Expr::Path(crate::ast::Path::new(
                        full.split('.').map(|s| s.to_string()).collect(),
                    ));
                }
            }
            expr.clone()
        }
        Expr::Unary(op, e) => Expr::Unary(op.clone(), Box::new(prefix_paths(e, prefix, locals, vars))),
        Expr::Binary(op, a, b) => Expr::Binary(
            op.clone(),
            Box::new(prefix_paths(a, prefix, locals, vars)),
            Box::new(prefix_paths(b, prefix, locals, vars)),
        ),
        Expr::Ternary(c, t, e) => Expr::Ternary(
            Box::new(prefix_paths(c, prefix, locals, vars)),
            Box::new(prefix_paths(t, prefix, locals, vars)),
            Box::new(prefix_paths(e, prefix, locals, vars)),
        ),
        Expr::InRange(v, lo, hi) => Expr::InRange(
            Box::new(prefix_paths(v, prefix, locals, vars)),
            Box::new(prefix_paths(lo, prefix, locals, vars)),
            Box::new(prefix_paths(hi, prefix, locals, vars)),
        ),
        Expr::InSet(v, items) => Expr::InSet(
            Box::new(prefix_paths(v, prefix, locals, vars)),
            items.iter().map(|i| prefix_paths(i, prefix, locals, vars)).collect(),
        ),
        Expr::Cast(t, e) => Expr::Cast(t.clone(), Box::new(prefix_paths(e, prefix, locals, vars))),
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// Atom extraction
// ---------------------------------------------------------------------------

fn extract_atoms(exprs: &[Expr]) -> Vec<Atom> {
    let mut out = Vec::new();
    for e in exprs {
        split_and(e, &mut out);
    }
    out
}

fn split_and(expr: &Expr, out: &mut Vec<Atom>) {
    match expr {
        Expr::Binary(op, a, b) if op == "&&" => {
            split_and(a, out);
            split_and(b, out);
        }
        Expr::Binary(op, a, b) if op == "->" => {
            let mut cons = Vec::new();
            split_and(b, &mut cons);
            out.push(Atom::Implies((**a).clone(), cons));
        }
        Expr::Unary(op, e) if op == "!" => {
            if let Some(v) = as_var(e) {
                out.push(Atom::Rel(v, Op::Eq, Expr::Int(0)));
            } else {
                out.push(Atom::Unsupported(expr.clone()));
            }
        }
        Expr::InRange(v, lo, hi) => {
            if let Some(name) = as_var(v) {
                let l = lo.eval_const(&|_| None);
                let h = hi.eval_const(&|_| None);
                match (l, h) {
                    (Some(l), Some(h)) => out.push(Atom::InRange(name, l, h)),
                    _ => out.push(Atom::Unsupported(expr.clone())),
                }
            } else {
                out.push(Atom::Unsupported(expr.clone()));
            }
        }
        Expr::InSet(v, items) => {
            if let Some(name) = as_var(v) {
                out.push(Atom::InSet(name, items.clone()));
            } else {
                out.push(Atom::Unsupported(expr.clone()));
            }
        }
        Expr::Binary(op, a, b) if matches!(op.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=") => {
            let o = match op.as_str() {
                "==" => Op::Eq,
                "!=" => Op::Ne,
                "<" => Op::Lt,
                "<=" => Op::Le,
                ">" => Op::Gt,
                _ => Op::Ge,
            };
            if let Some(name) = as_var(a) {
                out.push(Atom::Rel(name, o, (**b).clone()));
            } else if let Some(name) = as_var(b) {
                // Normalise `const op var` to `var flipped_op const`.
                let flipped = match o {
                    Op::Lt => Op::Gt,
                    Op::Gt => Op::Lt,
                    Op::Le => Op::Ge,
                    Op::Ge => Op::Le,
                    other => other,
                };
                out.push(Atom::Rel(name, flipped, (**a).clone()));
            } else {
                // Neither side is a bare variable: keep it as a general
                // relation for the arithmetic repair pass.
                out.push(Atom::General((**a).clone(), o, (**b).clone()));
            }
        }
        Expr::Bool(true) => {}
        _ => out.push(Atom::Unsupported(expr.clone())),
    }
}

fn render_expr(e: &Expr) -> String {
    match e {
        Expr::Path(p) => p.segments.join("."),
        Expr::Int(v) => v.to_string(),
        Expr::Unary(op, x) => format!("{}{}", op, render_expr(x)),
        Expr::Binary(op, a, b) => format!("{} {} {}", render_expr(a), op, render_expr(b)),
        Expr::Cast(t, x) => format!("({}){}", t, render_expr(x)),
        Expr::Call(f, args) => {
            let a: Vec<String> = args.iter().map(render_expr).collect();
            format!("{}({})", f, a.join(", "))
        }
        _ => "<expr>".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Union-find
// ---------------------------------------------------------------------------

struct UnionFind {
    parent: HashMap<String, String>,
}

impl UnionFind {
    fn new(names: Vec<String>) -> Self {
        let mut parent = HashMap::new();
        for n in names {
            parent.insert(n.clone(), n);
        }
        UnionFind { parent }
    }
    fn find(&mut self, x: &str) -> String {
        let p = match self.parent.get(x) {
            Some(p) => p.clone(),
            None => return x.to_string(),
        };
        if p == x {
            return p;
        }
        let root = self.find(&p);
        self.parent.insert(x.to_string(), root.clone());
        root
    }
    fn union(&mut self, a: &str, b: &str) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(rb, ra);
        }
    }
}
