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
use crate::ir::{Action, Field, FieldKind, Model, TypeCategory};

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
    /// `x in collection` where the collection is a named array/list (PSS 8.5.9).
    InArray(String, String),
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
pub struct Rng(u64);

impl Rng {
    /// A generator seeded deterministically, for use outside the solver.
    pub fn seeded(seed: u64) -> Self {
        Rng::new(seed)
    }
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next_u64(&mut self) -> u64 {
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
    /// The run's seed, so `urandom()` varies between seeds.
    pub seed: u64,
    /// `sizeof_s<T>::nbits` for every declared type (PSS 24.10.2).
    pub type_bits: HashMap<String, i64>,
}

impl<'a> Env<'a> {
    pub fn new(model: &'a Model) -> Self {
        let mut variants = HashMap::new();
        for vars in model.enums.values() {
            for (i, v) in vars.iter().enumerate() {
                variants.entry(v.clone()).or_insert(i as i64);
            }
        }
        let mut type_bits = HashMap::new();
        for (name, dt) in &model.types {
            if let Some(n) = type_nbits(model, dt) {
                type_bits.insert(name.clone(), n);
                // Also index by leaf name, so `sizeof_s<ctrl_s>` resolves.
                let leaf = name.rsplit("::").next().unwrap_or(name).to_string();
                type_bits.entry(leaf).or_insert(n);
            }
        }
        Env {
            seed: 0,
            type_bits,
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
        // A floating-point literal's value is its IEEE-754 bit pattern, as for
        // the `float_pkg` functions.
        Expr::Float(f) => Some(f.to_bits() as i64),
        Expr::Bool(b) => Some(if *b { 1 } else { 0 }),
        Expr::Path(p) => {
            let joined = p.segments.join(".");
            if let Some(v) = values.get(&joined) {
                return Some(*v);
            }
            // The deprecated array properties `arr.sum` and `arr.size`
            // (PSS 7.9.2.4) have the same meaning as the methods.
            if let Some((base, last)) = joined.rsplit_once('.') {
                if last == "sum" && values.contains_key(&format!("{}.size", base)) {
                    return array_sum(base, values);
                }
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
            // Array operators whose operands are collections rather than values
            // (PSS 7.9.2.1): set membership and element-wise equality.
            if matches!(op.as_str(), "in" | "inside") {
                let x = eval(a, values, env)?;
                let found = array_contains(b, x, values)?;
                return Some(if found { 1 } else { 0 });
            }
            if matches!(op.as_str(), "==" | "!=") {
                if let (Some(la), Some(lb)) = (array_base(a, values), array_base(b, values)) {
                    let equal = arrays_equal(&la, &lb, values)?;
                    return Some(if (op == "==") == equal { 1 } else { 0 });
                }
            }
            let x = eval(a, values, env)?;
            let y = eval(b, values, env)?;
            match op.as_str() {
                "+" => Some(x + y),
                "-" => Some(x - y),
                "*" => Some(x * y),
                "/" if y != 0 => Some(x / y),
                "%" if y != 0 => Some(x % y),
                // `**` — the power operator (PSS 8.4).
                "**" => {
                    if y >= 0 && y < 64 {
                        Some((x as i128).pow(y as u32) as i64)
                    } else {
                        None
                    }
                }
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
        // Register access: `r.read()` / `r.read_val()` return the register's
        // current value (PSS 24.11.1). The value lives in the instance's
        // values under the register's path.
        Expr::Method(recv, m, args)
            if args.is_empty() && (m == "read" || m == "read_val") =>
        {
            let base = match &**recv {
                Expr::Path(p) => p.segments.join("."),
                _ => return None,
            };
            Some(values.get(&base).copied().unwrap_or(0))
        }
        Expr::Ternary(c, t, e) => {
            if eval(c, values, env)? != 0 {
                eval(t, values, env)
            } else {
                eval(e, values, env)
            }
        }
        // `sizeof_s<T>::nbits` / `::nbytes` (PSS 24.10.2).
        Expr::Sizeof { ty, bits } => {
            let n = *env.type_bits.get(ty)?;
            Some(if *bits { n } else { (n + 7) / 8 })
        }
        // The standard library's `float_pkg` (Annex C.1). A floating-point
        // value is held as its IEEE-754 bit pattern, so the storage functions
        // are bit selections and the math functions reinterpret the bits.
        Expr::Call(name, args) => {
            let arg = |i: usize| -> Option<i64> { args.get(i).and_then(|a| eval(a, values, env)) };
            let unary = |f: fn(f64) -> f64| -> Option<i64> {
                Some(f(f64::from_bits(arg(0)? as u64)).to_bits() as i64)
            };
            match name.as_str() {
                "float_mantissa" => Some(arg(0)? & 0x000F_FFFF_FFFF_FFFF),
                "float_exponent" => Some((arg(0)? >> 52) & 0x7FF),
                "float_sign" => Some((arg(0)? >> 63) & 1),
                "to_float" => {
                    let (m, e, s) = (arg(0)?, arg(1)?, arg(2)?);
                    Some((s << 63) | ((e & 0x7FF) << 52) | (m & 0x000F_FFFF_FFFF_FFFF))
                }
                "log" => unary(f64::ln),
                "log10" => unary(f64::log10),
                "exp" => unary(f64::exp),
                "sqrt" => unary(f64::sqrt),
                "round" => unary(f64::round),
                "floor" => unary(f64::floor),
                "ceil" => unary(f64::ceil),
                "sin" => unary(f64::sin),
                "cos" => unary(f64::cos),
                "tan" => unary(f64::tan),
                "asin" => unary(f64::asin),
                "acos" => unary(f64::acos),
                "atan" => unary(f64::atan),
                "sinh" => unary(f64::sinh),
                "cosh" => unary(f64::cosh),
                "tanh" => unary(f64::tanh),
                "asinh" => unary(f64::asinh),
                "acosh" => unary(f64::acosh),
                "atanh" => unary(f64::atanh),
                "pow" => Some(
                    f64::from_bits(arg(0)? as u64)
                        .powf(f64::from_bits(arg(1)? as u64))
                        .to_bits() as i64,
                ),
                "hypot" => Some(
                    f64::from_bits(arg(0)? as u64)
                        .hypot(f64::from_bits(arg(1)? as u64))
                        .to_bits() as i64,
                ),
                "atan2" => Some(
                    f64::from_bits(arg(0)? as u64)
                        .atan2(f64::from_bits(arg(1)? as u64))
                        .to_bits() as i64,
                ),
                // `urandom()` / `urandom_range()` (PSS 24.4). Deterministic but
                // instance-varying, so a run is reproducible yet different
                // instances draw different numbers.
                "urandom" => Some(((hash_values(values) ^ env.seed) & 0xFFFF_FFFF) as i64),
                // Address handles (PSS 24.10.3-24.10.8). A handle is an index
                // into the instance's handle table, held in `values`.
                "addr_value" | "addr_value_solve" => {
                    let h = arg(0)?;
                    values.get(&format!("__h.{}.addr", h)).copied()
                }
                "addr_value_abs" => {
                    let h = arg(0)?;
                    Some(values.get(&format!("__h.{}.abs", h)).copied().unwrap_or(1))
                }
                "get_tag" => {
                    // The tag is a string and the engine has no string values,
                    // so the region's tag index is returned (see SEMANTICS §2).
                    let h = arg(0)?;
                    Some(values.get(&format!("__h.{}.tag", h)).copied().unwrap_or(0))
                }
                // Primitive reads (PSS 24.10.9.1): little-endian, the first byte
                // going into bits [7:0].
                "read8" | "read16" | "read32" | "read64" => {
                    let h = arg(0)?;
                    let addr = values.get(&format!("__h.{}.addr", h)).copied()?;
                    let n = match name.as_str() {
                        "read8" => 1,
                        "read16" => 2,
                        "read32" => 4,
                        _ => 8,
                    };
                    let mut v = 0i64;
                    for i in 0..n {
                        v |= values.get(&format!("__mem.{}", addr + i)).copied().unwrap_or(0)
                            << (8 * i);
                    }
                    Some(v)
                }
                _ if false => None,
                "urandom_range" => {
                    let (lo, hi) = (arg(0)?, arg(1)?);
                    if hi <= lo {
                        Some(lo)
                    } else {
                        let span = (hi - lo + 1) as u64;
                        Some(lo + ((hash_values(values) ^ env.seed) % span) as i64)
                    }
                }
                _ => None,
            }
        }
        // `arr[i]` — an element of a fixed-size array.
        Expr::Index(base, idx) => {
            let i = eval(idx, values, env)?;
            if let Expr::Path(p) = &**base {
                let key = format!("{}[{}]", p.segments.join("."), i);
                if let Some(v) = values.get(&key) {
                    return Some(*v);
                }
            }
            None
        }
        // Array methods that have a value at solve time.
        Expr::Method(base, name, args) => {
            let path = match &**base {
                Expr::Path(p) => p.segments.join("."),
                _ => return None,
            };
            match name.as_str() {
                // `arr.size()` is the declared extent.
                "size" if args.is_empty() => {
                    values.get(&format!("{}.size", path)).copied()
                }
                // `arr.sum()` folds over the elements (PSS 7.9.2.2).
                "sum" if args.is_empty() => array_sum(&path, values),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The size in bits of a declared type (PSS 24.10.2). A packed struct is the
/// sum of its fields; a bit vector is its width; other scalars use their width.
fn type_nbits(model: &Model, dt: &crate::ir::DataType) -> Option<i64> {
    match dt.keyword.as_str() {
        "struct" => {
            let mut total = 0i64;
            for f in &dt.fields {
                let w = match f.bits {
                    Some(b) => b,
                    None => {
                        let key = model.resolve(&f.type_name);
                        match model.types.get(&key).and_then(|t| type_nbits(model, t)) {
                            Some(n) => n,
                            None => return None,
                        }
                    }
                };
                let n = f.array.unwrap_or(1).max(1);
                total += w * n;
            }
            Some(total)
        }
        "bit" | "int" | "byte" | "shortint" | "longint" | "integer" | "uint" => {
            Some(dt.fields.first().and_then(|f| f.bits).unwrap_or(32))
        }
        "enum" => Some(dt.fields.first().and_then(|f| f.bits).unwrap_or(32)),
        "bool" => Some(1),
        _ => dt.fields.first().and_then(|f| f.bits),
    }
}

/// A deterministic digest of the current assignment, used as a reproducible
/// random source for `urandom()`.
fn hash_values(values: &BTreeMap<String, i64>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (k, v) in values {
        for b in k.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h ^= *v as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// The values a set-membership list item stands for: a range expands to every
/// value in it, anything else to its own value.
fn set_item_intervals(it: &Expr, ctx: &BTreeMap<String, i64>, env: &Env) -> Vec<(i64, i64)> {
    match it {
        Expr::Range(lo, hi) => match (eval(lo, ctx, env), eval(hi, ctx, env)) {
            // The span is computed in i128: an open range such as `[..1]` is
            // `Range(i64::MIN, 1)`, whose span overflows i64 and used to panic.
            (Some(lo), Some(hi)) if hi >= lo => vec![(lo, hi)],
            _ => Vec::new(),
        },
        other => eval(other, ctx, env).map(|v| vec![(v, v)]).unwrap_or_default(),
    }
}

/// The values a set-membership list item stands for, materialised. Only used
/// where the range is small enough to enumerate; prefer `in_intervals`.
fn set_item_values(it: &Expr, ctx: &BTreeMap<String, i64>, env: &Env) -> Vec<i64> {
    let mut out = Vec::new();
    for (lo, hi) in set_item_intervals(it, ctx, env) {
        let span = (hi as i128) - (lo as i128);
        if span < 4096 {
            out.extend(lo..=hi);
        }
    }
    out
}

/// The sum of a fixed-size array's elements, if they are all known.
fn array_sum(base: &str, values: &BTreeMap<String, i64>) -> Option<i64> {
    let n = *values.get(&format!("{}.size", base))?;
    let mut acc = 0i64;
    for i in 0..n.min(MAX_ARRAY) {
        acc += *values.get(&format!("{}[{}]", base, i))?;
    }
    Some(acc)
}

/// The array a path names, if it is a known array (has a recorded `.size`).
fn array_base(e: &Expr, values: &BTreeMap<String, i64>) -> Option<String> {
    let base = match e {
        Expr::Path(p) => p.segments.join("."),
        _ => return None,
    };
    if values.contains_key(&format!("{}.size", base)) {
        Some(base)
    } else {
        None
    }
}

/// Is `v` an element of the array named by `rhs`? (PSS `in`, 8.5.9.)
/// `None` while the array's elements are not all known — otherwise a membership
/// test would be evaluated before assignment and tighten the wrong domain.
fn array_contains(rhs: &Expr, v: i64, values: &BTreeMap<String, i64>) -> Option<bool> {
    let base = match rhs {
        Expr::Path(p) => p.segments.join("."),
        _ => return None,
    };
    let n = *values.get(&format!("{}.size", base))?;
    let mut found = false;
    for i in 0..n.min(MAX_ARRAY) {
        match values.get(&format!("{}[{}]", base, i)) {
            Some(x) => found |= *x == v,
            None => return None,
        }
    }
    Some(found)
}

/// Element-wise equality of two arrays, once both are fully known.
fn arrays_equal(a: &str, b: &str, values: &BTreeMap<String, i64>) -> Option<bool> {
    let n = *values.get(&format!("{}.size", a))?;
    let m = *values.get(&format!("{}.size", b))?;
    if n != m {
        return Some(false);
    }
    for i in 0..n.min(MAX_ARRAY) {
        let x = values.get(&format!("{}[{}]", a, i))?;
        let y = values.get(&format!("{}[{}]", b, i))?;
        if x != y {
            return Some(false);
        }
    }
    Some(true)
}

/// Largest fixed-size array we will expand, to bound a pathological model.
const MAX_ARRAY: i64 = 256;

/// The category of a named type, mirroring the IR's classification.
fn category_for(model: &Model, ty: &str) -> TypeCategory {
    let leaf = ty.rsplit("::").next().unwrap_or(ty);
    if matches!(
        leaf,
        "int" | "bit" | "bool" | "byte" | "shortint" | "longint" | "integer" | "uint" | "float32" | "float64"
    ) {
        return TypeCategory::Scalar;
    }
    if let Some(v) = model.enums.get(leaf) {
        return TypeCategory::Enum(v.clone());
    }
    if model.types.contains_key(ty) {
        return TypeCategory::Struct(ty.to_string());
    }
    TypeCategory::Scalar
}

/// Solve the constraints of `action` and return a concrete assignment.
pub fn solve(model: &Model, action: &Action, seed: u64) -> SolveReport {
    solve_with(model, action, seed, &[], &BTreeMap::new())
}

/// Solve the constraints of `action`, additionally applying `extra` (inline
/// `do … with { … }` constraints) and treating `bindings` as known values
/// (for example a `repeat (i : n)` index).
/// Directives pulled out of the constraint list before atom extraction:
/// `dist` weights (16.1.9) and `solve … before …` ordering (16.1.8).
#[derive(Default, Debug)]
struct Directives {
    /// variable -> [(lo, hi, weight-is-per-value, weight)]
    dist: Vec<(String, Vec<(i64, i64, bool, f64)>)>,
    /// (before, after) variable groups
    order: Vec<(Vec<String>, Vec<String>)>,
}

/// Remove `dist` and `solve … before` markers from `exprs`, returning them.
fn take_directives(exprs: &mut Vec<Expr>, env: &Env) -> Directives {
    let mut out = Directives::default();
    let mut kept = Vec::new();
    let const_eval = |e: &Expr| -> Option<i64> { eval(e, &BTreeMap::new(), env) };
    for e in exprs.drain(..) {
        match &e {
            Expr::Call(f, args) if f == "__dist" => {
                if let Some(Expr::Path(p)) = args.first() {
                    let var = p.segments.join(".");
                    let mut items = Vec::new();
                    for it in &args[1..] {
                        if let Expr::Call(_, ia) = it {
                            let (lo, hi) = match &ia[0] {
                                Expr::InRange(_, lo, hi) => {
                                    (const_eval(lo), const_eval(hi))
                                }
                                single => (const_eval(single), const_eval(single)),
                            };
                            let per_value = matches!(ia.get(1), Some(Expr::Bool(true)));
                            let w = ia.get(2).and_then(|x| const_eval(x)).unwrap_or(1) as f64;
                            if let (Some(lo), Some(hi)) = (lo, hi) {
                                items.push((lo, hi, per_value, w));
                            }
                        }
                    }
                    out.dist.push((var, items));
                }
            }
            Expr::Call(f, args) if f == "__solve_before" => {
                let names = |e: &Expr| -> Vec<String> {
                    match e {
                        Expr::List(items) => items
                            .iter()
                            .filter_map(|x| match x {
                                Expr::Path(p) => Some(p.segments.join(".")),
                                _ => None,
                            })
                            .collect(),
                        _ => Vec::new(),
                    }
                };
                if args.len() == 2 {
                    out.order.push((names(&args[0]), names(&args[1])));
                }
            }
            _ => kept.push(e),
        }
    }
    *exprs = kept;
    out
}

/// Pick a value from a weighted list, in proportion to the weights.
fn pick_weighted(items: &[(i64, f64)], rng: &mut Rng) -> Option<i64> {
    if items.is_empty() {
        return None;
    }
    // Scale the weights to integers so the choice can use the generator's
    // full range (its output is 47 bits, so a float ratio would be skewed).
    let scaled: Vec<(i64, i64)> = items
        .iter()
        .map(|(v, w)| (*v, (w * 1000.0).round().max(0.0) as i64))
        .collect();
    let total: i64 = scaled.iter().map(|(_, w)| *w).sum();
    if total <= 0 {
        return items.first().map(|(v, _)| *v);
    }
    let mut r = rng.range(0, total - 1);
    for (v, w) in &scaled {
        if r < *w {
            return Some(*v);
        }
        r -= *w;
    }
    scaled.last().map(|(v, _)| *v)
}

/// Expand dist items into a per-value weight list restricted to `d`.
fn dist_values(items: &[(i64, i64, bool, f64)], d: &Domain) -> Vec<(i64, f64)> {
    let mut out: Vec<(i64, f64)> = Vec::new();
    for (lo, hi, per_value, w) in items {
        let n = (hi - lo + 1).max(1) as f64;
        let each = if *per_value { *w } else { *w / n };
        for v in *lo..=*hi {
            if v < d.lo || v > d.hi || !d.contains(v) {
                continue;
            }
            match out.iter_mut().find(|(x, _)| *x == v) {
                Some(slot) => slot.1 += each,
                None => out.push((v, each)),
            }
        }
        if out.len() > 4096 {
            break;
        }
    }
    out
}

pub fn solve_with(
    model: &Model,
    action: &Action,
    seed: u64,
    extra: &[Expr],
    bindings: &BTreeMap<String, i64>,
) -> SolveReport {
    let mut env = Env::new(model);
    env.seed = seed;
    let mut vars = collect_variables(model, action);
    // A bound attribute is fixed: `exec pre_solve` assignments to non-random
    // fields, and values passed in, must survive the solve (PSS 16.4.12).
    for (k, v) in bindings {
        if let Some(d) = vars.get_mut(k) {
            d.lo = *v;
            d.hi = *v;
        }
    }
    let mut constraints = collect_constraints(model, action, &vars, extra);
    let directives = take_directives(&mut constraints, &env);
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
            Atom::InArray(var, coll) => {
                // Membership in a named collection (PSS 8.5.9): keep the
                // elements that fit the domain, and forbid the values between
                // them so the assignment is uniform over the elements rather
                // than always the first.
                if let Some(d) = vars.get(var).cloned() {
                    let n = *bindings.get(&format!("{}.size", coll)).unwrap_or(&0);
                    let allowed: Vec<i64> = (0..n.min(MAX_ARRAY))
                        .filter_map(|i| bindings.get(&format!("{}[{}]", coll, i)).copied())
                        .filter(|v| *v >= d.lo && *v <= d.hi)
                        .collect();
                    if !allowed.is_empty() {
                        let lo = *allowed.iter().min().unwrap();
                        let hi = *allowed.iter().max().unwrap();
                        tighten(&mut vars, var, lo, hi);
                        if hi - lo < 4096 {
                            if let Some(dd) = vars.get_mut(var) {
                                for v in lo..=hi {
                                    if !allowed.contains(&v) {
                                        dd.forbidden.insert(v);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Atom::InSet(var, items) => {
                // A list item may be a range (`5..9`), which stands for every
                // value in it (PSS 8.5.9). The union is kept as intervals so an
                // open range (`[..1]`, `[0..]`) does not need materialising.
                let mut intervals: Vec<(i64, i64)> = Vec::new();
                for it in items {
                    intervals.extend(set_item_intervals(it, bindings, &env));
                }
                let hull = if intervals.is_empty() {
                    None
                } else {
                    Some((
                        intervals.iter().map(|(l, _)| *l).min().unwrap(),
                        intervals.iter().map(|(_, h)| *h).max().unwrap(),
                    ))
                };
                let total: i128 = intervals
                    .iter()
                    .map(|(l, h)| (*h as i128) - (*l as i128) + 1)
                    .sum();
                let allowed: Vec<i64> = if total <= 4096 {
                    let mut v = Vec::new();
                    for (l, h) in &intervals {
                        v.extend(*l..=*h);
                    }
                    v
                } else {
                    Vec::new()
                };
                if !allowed.is_empty() {
                    // Small union: narrow to its hull and forbid the gaps.
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
                } else if !intervals.is_empty() {
                    // Large or open union: it cannot be enumerated, so pick one
                    // interval that overlaps the domain and narrow to it. That
                    // is a valid value, and it keeps an open range like
                    // `[..1, 8..16, 4294967296..]` from being vacuous.
                    if let Some(d) = vars.get_mut(var) {
                        let pick = intervals
                            .iter()
                            .find(|(l, h)| *h >= d.lo && *l <= d.hi)
                            .copied()
                            .or_else(|| {
                                // No overlap: move to the nearest interval.
                                intervals
                                    .iter()
                                    .min_by_key(|(l, h)| {
                                        if *h < d.lo {
                                            d.lo as i128 - *h as i128
                                        } else {
                                            *l as i128 - d.hi as i128
                                        }
                                    })
                                    .copied()
                            });
                        if let Some((l, h)) = pick {
                            let (nl, nh) = (d.lo.max(l), d.hi.min(h));
                            if nl <= nh {
                                d.intersect_interval(nl, nh);
                            } else {
                                // The interval lies outside the domain; widen
                                // the domain to it, clamped to a signed 32-bit
                                // range so an open bound (`[..-1]`) does not
                                // become `i64::MIN`.
                                const I32_MIN: i64 = i32::MIN as i64;
                                const I32_MAX: i64 = i32::MAX as i64;
                                d.lo = l.max(I32_MIN);
                                d.hi = h.min(I32_MAX);
                                d.forbidden.clear();
                                if d.lo > d.hi {
                                    d.lo = l.min(h).max(I32_MIN);
                                    d.hi = l.max(h).min(I32_MAX);
                                }
                            }
                        }
                    }
                } else if let Some((hlo, hhi)) = hull {
                    if let Some(d) = vars.get_mut(var) {
                        d.intersect_interval(hlo, hhi);
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

    // `dist` restricts the value set to the listed values and biases the pick
    // by weight (16.1.9). Constraints still win: if the domain does not
    // overlap the dist list, the domain is left alone.
    let mut dist_by_root: HashMap<String, Vec<(i64, f64)>> = HashMap::new();
    for (var, items) in &directives.dist {
        if let Some(d) = class_domain.get(&uf.find(var)) {
            let vs = dist_values(items, d);
            if !vs.is_empty() {
                dist_by_root.insert(uf.find(var), vs);
            }
        }
    }

    // Assign a value per class, honouring `solve … before …` where possible.
    let mut rng = Rng::new(seed ^ 0x9E37_79B9_7F4A_7C15);
    let mut values: BTreeMap<String, i64> = BTreeMap::new();
    let mut class_value: HashMap<String, i64> = HashMap::new();
    let mut order: Vec<String> = vars.keys().cloned().collect();
    if !directives.order.is_empty() {
        // Assign the `before` variables first.
        let rank = |n: &String| -> usize {
            let mut r: usize = 0;
            for (before, after) in &directives.order {
                let is_before = before.iter().any(|b| b == n || n.ends_with(&format!(".{}", b)));
                let is_after = after.iter().any(|a| a == n || n.ends_with(&format!(".{}", a)));
                if is_before {
                    r += 2;
                }
                if is_after {
                    r = r.saturating_sub(1);
                }
            }
            r
        };
        order.sort_by_key(|n| std::cmp::Reverse(rank(n)));
    }
    for name in order.iter() {
        let root = uf.find(name);
        let v = *class_value.entry(root.clone()).or_insert_with(|| {
            let d = class_domain.get(&root).cloned().unwrap_or(Domain {
                lo: 0,
                hi: 0,
                forbidden: HashSet::new(),
                is_enum: false,
            });
            match dist_by_root.get(&root) {
                Some(items) => pick_weighted(items, &mut rng).unwrap_or_else(|| pick(&d, &mut rng)),
                None => pick(&d, &mut rng),
            }
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

    // Bound values (exec-established collection elements, passed-in values) are
    // fixed, so make them visible to the repair pass as well.
    for (k, v) in bindings {
        values.entry(k.clone()).or_insert(*v);
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
                    // `arrA == arrB`: copy the elements of B into A.
                    if matches!(op, Op::Eq) && !bindings.contains_key(var) {
                        let lhs = Expr::Path(crate::ast::Path::simple(var));
                        if let (Some(la), Some(lb)) =
                            (array_base(&lhs, values), array_base(rhs, values))
                        {
                            let n = *values.get(&format!("{}.size", lb)).unwrap_or(&0);
                            for i in 0..n.min(MAX_ARRAY) {
                                if let Some(v) = values.get(&format!("{}[{}]", lb, i)).copied() {
                                    let slot = format!("{}[{}]", la, i);
                                    if values.get(&slot).copied() != Some(v)
                                        && class_domain
                                            .get(&slot)
                                            .map(|d| v >= d.lo && v <= d.hi)
                                            .unwrap_or(true)
                                    {
                                        values.insert(slot, v);
                                        changed = true;
                                    }
                                }
                            }
                            continue;
                        }
                    }
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
            // `arrA == arrB` — both sides are collections, compared element-wise.
            if let Some(la) = array_base(&Expr::Path(crate::ast::Path::simple(var)), values) {
                if let Some(lb) = array_base(rhs, values) {
                    if let Some(equal) = arrays_equal(&la, &lb, values) {
                        return match op {
                            Op::Eq => equal,
                            Op::Ne => !equal,
                            _ => true,
                        };
                    }
                }
            }
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
        Atom::InArray(var, coll) => match values.get(var) {
            Some(v) => array_contains(
                &Expr::Path(crate::ast::Path::simple(coll)),
                *v,
                values,
            )
            .unwrap_or(true),
            None => true,
        },
        Atom::InRange(var, lo, hi) => values.get(var).map(|v| *v >= *lo && *v <= *hi).unwrap_or(true),
        Atom::InSet(var, items) => {
            let x = match values.get(var) {
                Some(v) => *v,
                None => return true,
            };
            items.iter().any(|it| {
                set_item_intervals(it, values, env)
                    .iter()
                    .any(|(lo, hi)| x >= *lo && x <= *hi)
            })
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
        Atom::InArray(v, c) => format!("{} in {}", v, c),
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
        // `arr[i]` names an array element variable (PSS 8.4).
        Expr::Index(base, idx) => {
            let i = idx.eval_const(&|_| None)?;
            match &**base {
                Expr::Path(p) => Some(format!("{}[{}]", p.segments.join("."), i)),
                _ => None,
            }
        }
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
    // A non-`rand` attribute is not randomised (PSS 8.4): it holds its
    // initial value, so model it as a singleton domain. A `rand` attribute
    // keeps its declared range.
    let fixed = if field.kind == FieldKind::Plain {
        Some(
            field
                .init
                .as_ref()
                .and_then(|e| e.eval_const(&|n| model.constants.get(n).copied()))
                .unwrap_or(0),
        )
    } else {
        None
    };
    // A fixed-size array expands into one variable per element (PSS 8.4), so a
    // multireg such as `array<bit[32], 8>` becomes eight modelled registers.
    if let Some(n) = field.array {
        let n = n.clamp(0, MAX_ARRAY);
        let elem_ty = field
            .elem_type
            .clone()
            .unwrap_or_else(|| field.type_name.clone());
        let elem_bits = field.elem_bits.or(field.bits);
        for i in 0..n {
            let sub = Field {
                name: format!("{}[{}]", field.name, i),
                kind: field.kind,
                type_name: elem_ty.clone(),
                category: category_for(model, &elem_ty),
                bits: elem_bits,
                inline_constraint: None,
                init: None,
                elem_type: None,
                elem_bits: None,
                array: None,
            };
            expand_field(model, &sub, prefix, depth + 1, visited, out);
        }
        // `arr.size()` is a constant.
        out.insert(
            format!("{}.size", path),
            Domain {
                lo: n,
                hi: n,
                forbidden: HashSet::new(),
                is_enum: false,
            },
        );
        return;
    }

    match &field.category {
        TypeCategory::Scalar => {
            let hi = field
                .bits
                // Use the declared width (up to 62 bits); a narrower cap would
                // truncate bit-wise results such as `~x` on a `bit[32]`.
                .map(|b| (1i64 << b.clamp(1, 62)) - 1)
                .unwrap_or(DEFAULT_INT_MAX);
            let (lo, hi) = match fixed {
                Some(v) => (v, v),
                None => (0, hi),
            };
            out.insert(
                path,
                Domain {
                    lo,
                    hi,
                    forbidden: HashSet::new(),
                    is_enum: false,
                },
            );
        }
        TypeCategory::Enum(variants) => {
            let hi = (variants.len().saturating_sub(1)) as i64;
            let (lo, hi) = match fixed {
                Some(v) => (v, v),
                None => (0, hi),
            };
            out.insert(
                path,
                Domain {
                    lo,
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
        Expr::Binary(op, a, b) if op == "in" || op == "inside" => {
            match (as_var(a), &**b) {
                (Some(v), Expr::Path(p)) => out.push(Atom::InArray(v, p.segments.join("."))),
                _ => out.push(Atom::Unsupported(expr.clone())),
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

pub fn render_expr(e: &Expr) -> String {
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
