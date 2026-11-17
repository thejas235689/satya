//! Translate a PSS constraint set into a BDD and answer questions exactly.
//!
//! Only small domains are eligible: `bool`, an enum, `bit[N]` with a small N,
//! and a declared range of at most [`crate::bdd::MAX_DOMAIN`] values. Anything
//! else makes the whole proof **abstain** — the caller reports NOT PROVED, never
//! "satisfiable". A proof tool that guesses is worse than one that declines.

use crate::ast::{Expr, Path};
use crate::bdd::{Bdd, MAX_DOMAIN};
use crate::ir::{Action, Model};
use crate::solver;
use std::collections::BTreeMap;

/// One eligible PSS variable, encoded as `bits` consecutive BDD variables.
struct Var {
    name: String,
    lo: i64,
    hi: i64,
    bits: u32,
    /// Index of the least significant BDD variable.
    base: u32,
}

impl Var {
    fn width(&self) -> i64 {
        self.hi - self.lo + 1
    }
}

/// The outcome of a proof attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum Proof {
    /// The constraint set cannot be satisfied.
    Unsat,
    /// Satisfiable, with a witness assignment.
    Sat(BTreeMap<String, i64>),
    /// These constraints restrict nothing: every assignment satisfies them.
    Vacuous(Vec<String>),
    /// Some constraints restrict only part of the space.
    Restricting(Vec<String>),
    /// The model is outside what a BDD can represent here.
    NotProved(String),
}

fn bits_for(width: i64) -> u32 {
    let mut b = 0u32;
    while (1i64 << b) < width && b < 32 {
        b += 1;
    }
    b.max(1)
}

/// The variables of `action` whose domains are small enough to encode.
///
/// Returns `Err` with a reason when *any* random variable is too wide: a proof
/// over a subset of the variables would be a lie about the whole constraint set.
fn eligible(model: &Model, action: &Action) -> Result<Vec<Var>, String> {
    let mut out: Vec<Var> = Vec::new();
    let mut base = 0u32;
    // `collect_variables` already knows every declared domain, including the
    // declared-width ceiling added in 0.50.0.
    let vars = solver::variables_for(model, action);
    // A field's declared range (`rand int x in [0..7]`) is an inline constraint,
    // not part of the type domain, so the domain alone is the default width.
    // Narrow it here or every declared range looks like a 1024-value span.
    let mut narrowed: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for f in &action.fields {
        if let Some(ic) = &f.inline_constraint {
            let leaf = f.name.rsplit('.').next().unwrap_or(&f.name).to_string();
            if let Expr::InRange(_, lo, hi) = ic {
                if let (Some(l), Some(h)) = (const_of(lo), const_of(hi)) {
                    narrowed.insert(leaf, (l, h));
                }
            }
        }
    }
    for (name, d) in vars {
        let leaf = name.rsplit('.').next().unwrap_or(&name).to_string();
        let (lo, hi) = narrowed.get(&leaf).copied().unwrap_or((d.lo, d.hi));
        let width = hi.saturating_sub(lo).saturating_add(1);
        if width <= 0 || width > MAX_DOMAIN {
            return Err(format!(
                "`{}` spans {} values, more than the {} a BDD can carry",
                name, width, MAX_DOMAIN
            ));
        }
        let bits = bits_for(width);
        out.push(Var { name, lo, hi, bits, base });
        base += bits;
    }
    Ok(out)
}

fn find<'a>(vars: &'a [Var], path: &Path) -> Option<&'a Var> {
    let full = path.segments.join(".");
    vars.iter().find(|v| v.name == full)
        .or_else(|| vars.iter().find(|v| v.name.rsplit('.').next() == Some(full.as_str())))
}

/// The BDD for `var == value`, given the value is inside the domain.
fn eq_const(b: &mut Bdd, v: &Var, value: i64) -> Option<u32> {
    if value < v.lo || value > v.hi {
        return Some(crate::bdd::FALSE_NODE);
    }
    let enc = value - v.lo;
    let mut out = crate::bdd::TRUE_NODE;
    for i in 0..v.bits {
        let bit = (enc >> i) & 1 == 1;
        let node = {
            let f = crate::bdd::FALSE_NODE;
            let t = crate::bdd::TRUE_NODE;
            b.mk(v.base + i, f, t)
        };
        let lit = if bit { node } else { b.not(node) };
        out = b.and(out, lit);
    }
    Some(out)
}

/// The BDD for `var < value` over the encoded (unsigned) representation.
fn lt_const(b: &mut Bdd, v: &Var, value: i64) -> Option<u32> {
    let enc = value - v.lo;
    if enc <= 0 {
        return Some(crate::bdd::FALSE_NODE);
    }
    // Every value is below `width`, so `x < width` is a tautology. Using
    // `>` here missed the equality case and built a comparator for a value
    // that does not fit the encoding, which read as "unsatisfiable".
    if enc >= v.width() {
        return Some(crate::bdd::TRUE_NODE);
    }
    // Standard unsigned "less than": scan from the MSB, accumulating
    // (prefix equal) AND (this bit is 0 while the constant's bit is 1).
    let mut eq_prefix = crate::bdd::TRUE_NODE;
    let mut acc = crate::bdd::FALSE_NODE;
    for i in (0..v.bits).rev() {
        let bit = (enc >> i) & 1 == 1;
        let node = {
            let f = crate::bdd::FALSE_NODE;
            let t = crate::bdd::TRUE_NODE;
            b.mk(v.base + i, f, t)
        };
        if bit {
            // constant bit 1: a 0 here makes it smaller
            let lit = b.not(node);
            let term = b.and(eq_prefix, lit);
            acc = b.or(acc, term);
            eq_prefix = b.and(eq_prefix, node);
        } else {
            let nn = b.not(node);
            eq_prefix = b.and(eq_prefix, nn);
        }
    }
    Some(acc)
}

fn translate(b: &mut Bdd, e: &Expr, vars: &[Var]) -> Option<u32> {
    match e {
        Expr::Bool(true) => Some(crate::bdd::TRUE_NODE),
        Expr::Bool(false) => Some(crate::bdd::FALSE_NODE),
        Expr::Binary(op, a, c) => match op.as_str() {
            "&&" => {
                let x = translate(b, a, vars)?;
                let y = translate(b, c, vars)?;
                Some(b.and(x, y))
            }
            "||" => {
                let x = translate(b, a, vars)?;
                let y = translate(b, c, vars)?;
                Some(b.or(x, y))
            }
            "->" => {
                let x = translate(b, a, vars)?;
                let nx = b.not(x);
                let y = translate(b, c, vars)?;
                Some(b.or(nx, y))
            }
            "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                // Three shapes are recognised, in order of specificity.
                //
                // 1. `var OP const` (or `const OP var`). This is the cheapest
                //    circuit, and the only one that can answer for a constant
                //    that falls outside the encoded domain (`x < 100` on a
                //    [0..7] variable is a tautology, not an overflow), so it is
                //    tried first and keeps its exact previous encoding.
                if let Some((v, k)) = comparison_sides(a, c, vars) {
                    let node = match op.as_str() {
                        "==" => eq_const(b, v, k)?,
                        "!=" => {
                            let q = eq_const(b, v, k)?;
                            b.not(q)
                        }
                        "<" => lt_const(b, v, k)?,
                        "<=" => {
                            let q = lt_const(b, v, k + 1)?;
                            q
                        }
                        ">" => {
                            let q = lt_const(b, v, k + 1)?;
                            b.not(q)
                        }
                        // `>=` must be the negation of `<`; falling through to
                        // the `<` arm silently turned `x >= 0` into `x < 0`.
                        _ => {
                            let q = lt_const(b, v, k)?;
                            b.not(q)
                        }
                    };
                    return Some(node);
                }
                // 2. `arith == const` / `arith != const`, e.g. `x + y == 7`.
                //    `const_of` folds a constant expression on either side, so
                //    `x + y == 2 + 5` lands here too.
                if op == "==" || op == "!=" {
                    let width = width_for(vars);
                    if let Some(k) = const_of(c) {
                        let q = eq_arith(b, a, k, vars, width)?;
                        return Some(if op == "==" { q } else { b.not(q) });
                    }
                    if let Some(k) = const_of(a) {
                        let q = eq_arith(b, c, k, vars, width)?;
                        return Some(if op == "==" { q } else { b.not(q) });
                    }
                }
                // 3. A general comparison between two arithmetic expressions:
                //    `x < y`, `x == y`, `x + y == z`, and the same against a
                //    folded constant. Both sides become two's-complement bit
                //    vectors of a common width and are compared MSB-first.
                //    Anything `bits_of` cannot build (`*`, `/`, a call, a
                //    string) returns `None` here, so the proof still abstains.
                let width = width_for(vars);
                if !fits_width(a, width) || !fits_width(c, width) {
                    return None;
                }
                let xb = bits_of(b, a, vars, width)?;
                let yb = bits_of(b, c, vars, width)?;
                let node = match op.as_str() {
                    "==" => eq_bits(b, &xb, &yb),
                    "!=" => {
                        let q = eq_bits(b, &xb, &yb);
                        b.not(q)
                    }
                    "<" => lt_bits(b, &xb, &yb),
                    // `x <= y` is `!(y < x)`; `>=` is `!(x < y)`.
                    "<=" => {
                        let q = lt_bits(b, &yb, &xb);
                        b.not(q)
                    }
                    ">" => lt_bits(b, &yb, &xb),
                    _ => {
                        let q = lt_bits(b, &xb, &yb);
                        b.not(q)
                    }
                };
                Some(node)
            }
            _ => None,
        },
        Expr::Unary(op, a) if op == "!" => {
            let x = translate(b, a, vars)?;
            Some(b.not(x))
        }
        Expr::InSet(subject, items) => {
            let v = match subject.as_ref() {
                Expr::Path(p) => find(vars, p)?,
                _ => return None,
            };
            let mut acc = crate::bdd::FALSE_NODE;
            for it in items {
                let k = const_of(it)?;
                let q = eq_const(b, v, k)?;
                acc = b.or(acc, q);
            }
            Some(acc)
        }
        Expr::InRange(subject, lo, hi) => {
            let v = match subject.as_ref() {
                Expr::Path(p) => find(vars, p)?,
                _ => return None,
            };
            let l = const_of(lo)?;
            let h = const_of(hi)?;
            let below = lt_const(b, v, l)?;
            let ge = b.not(below);
            let lt = lt_const(b, v, h + 1)?;
            Some(b.and(ge, lt))
        }
        _ => None,
    }
}

/// The BDD for each bit of an arithmetic expression over eligible variables.
///
/// `+` and `-` between a variable and a constant, or two variables, become a
/// ripple-carry adder over the encoded bit vectors. This is what lets
/// `x + y == 7` be proved rather than abstained on — and it is what gives
/// `--order` something order-sensitive to measure.
fn bits_of(b: &mut Bdd, e: &Expr, vars: &[Var], width: u32) -> Option<Vec<u32>> {
    match e {
        Expr::Int(n) => {
            let mut out = Vec::new();
            for i in 0..width {
                out.push(if (*n >> i) & 1 == 1 {
                    crate::bdd::TRUE_NODE
                } else {
                    crate::bdd::FALSE_NODE
                });
            }
            Some(out)
        }
        Expr::Path(p) => {
            let v = find(vars, p)?;
            // The bits are of the variable's *value*, not of its encoding:
            // two variables with different lower bounds (`x in [2..9]` and
            // `y in [0..7]`) must compare as values, so the offset `lo` is
            // folded in here with the same ripple adder. Leaving it out made
            // `x + y == k` compare encoded sums against an unencoded k, which
            // is only correct when every `lo` is zero.
            Some(var_value_bits(b, v, width))
        }
        Expr::Binary(op, a, c) if op == "+" || op == "-" => {
            let x = bits_of(b, a, vars, width)?;
            let y = bits_of(b, c, vars, width)?;
            // Two's complement subtract: a + ~b + 1.
            let y = if op == "-" {
                y.into_iter().map(|n| b.not(n)).collect::<Vec<_>>()
            } else {
                y
            };
            let mut carry = if op == "-" {
                crate::bdd::TRUE_NODE
            } else {
                crate::bdd::FALSE_NODE
            };
            let mut out = Vec::new();
            for i in 0..width as usize {
                let (xi, yi) = (x[i], y[i]);
                // sum = x XOR y XOR carry
                let xy = b.xor(xi, yi);
                let sum = b.xor(xy, carry);
                // carry = majority(x, y, carry)
                let c1 = b.and(xi, yi);
                let c2 = b.and(carry, xy);
                let next = b.or(c1, c2);
                out.push(sum);
                carry = next;
            }
            Some(out)
        }
        _ => None,
    }
}

/// Two's-complement bits of a variable's actual value, `width` bits wide.
///
/// The variable is stored as `enc = value - lo`, which is unsigned and fits in
/// `v.bits`; adding `lo` back with the adder gives the value modulo `2^width`,
/// which is exactly its two's-complement encoding.
fn var_value_bits(b: &mut Bdd, v: &Var, width: u32) -> Vec<u32> {
    let mut enc = Vec::with_capacity(width as usize);
    for i in 0..width {
        if i < v.bits {
            let f = crate::bdd::FALSE_NODE;
            let t = crate::bdd::TRUE_NODE;
            enc.push(b.mk(v.base + i, f, t));
        } else {
            enc.push(crate::bdd::FALSE_NODE);
        }
    }
    add_const(b, &enc, v.lo, width)
}

/// `bits + k`, two's complement, modulo `2^width`.
fn add_const(b: &mut Bdd, bits: &[u32], k: i64, width: u32) -> Vec<u32> {
    if k == 0 {
        return bits.to_vec();
    }
    let mut carry = crate::bdd::FALSE_NODE;
    let mut out = Vec::with_capacity(width as usize);
    for i in 0..width as usize {
        let kb = if (k >> i) & 1 == 1 {
            crate::bdd::TRUE_NODE
        } else {
            crate::bdd::FALSE_NODE
        };
        let partial = b.xor(bits[i], kb);
        let sum = b.xor(partial, carry);
        let c1 = b.and(bits[i], kb);
        let c2 = b.and(carry, partial);
        out.push(sum);
        carry = b.or(c1, c2);
    }
    out
}

/// The two's-complement width used for a general comparison: enough to hold a
/// sum or a difference of any two values in range, plus a sign bit. It is
/// derived from the domains, not from the expression, so both sides of a
/// comparison always share one width.
fn width_for(vars: &[Var]) -> u32 {
    let m = vars
        .iter()
        .map(|v| v.lo.abs().max(v.hi.abs()))
        .max()
        .unwrap_or(1)
        .max(1);
    let span = m.saturating_mul(2).saturating_add(1);
    let mut bits = 2u32;
    // `bits` is signed, so the representable span is 2^(bits-1).
    while bits < 62 && (1i64 << (bits - 1)) <= span {
        bits += 1;
    }
    bits
}

/// Can every literal in `e` be represented in a signed `width`-bit vector?
///
/// A literal that does not fit would silently wrap in [`bits_of`], which is
/// how a comparator turns into a lie, so the translation abstains instead.
fn fits_width(e: &Expr, width: u32) -> bool {
    let lim = 1i64 << (width - 1);
    match e {
        Expr::Int(n) => *n > -lim && *n < lim,
        Expr::Unary(op, a) if op == "-" => fits_width(a, width),
        Expr::Binary(_, a, c) => fits_width(a, width) && fits_width(c, width),
        _ => true,
    }
}

/// The BDD for `x == y` between two bit vectors.
fn eq_bits(b: &mut Bdd, x: &[u32], y: &[u32]) -> u32 {
    let mut acc = crate::bdd::TRUE_NODE;
    for i in 0..x.len().min(y.len()) {
        let differ = b.xor(x[i], y[i]);
        let same = b.not(differ);
        acc = b.and(acc, same);
    }
    acc
}

/// Unsigned `x < y`, MSB first: `x < y` iff at the highest bit where they
/// differ, `x` has a 0 and `y` a 1.
fn ult_bits(b: &mut Bdd, x: &[u32], y: &[u32]) -> u32 {
    let mut eq_prefix = crate::bdd::TRUE_NODE;
    let mut acc = crate::bdd::FALSE_NODE;
    for i in (0..x.len()).rev() {
        let nx = b.not(x[i]);
        let smaller_here = b.and(nx, y[i]);
        let term = b.and(eq_prefix, smaller_here);
        acc = b.or(acc, term);
        let differ = b.xor(x[i], y[i]);
        let same = b.not(differ);
        eq_prefix = b.and(eq_prefix, same);
    }
    acc
}

/// Signed `x < y` in two's complement.
///
/// Flipping the sign bit of both operands maps the signed order onto the
/// unsigned one, which avoids the overflow a `x - y` sign test would need to
/// guard against.
fn lt_bits(b: &mut Bdd, x: &[u32], y: &[u32]) -> u32 {
    let w = x.len().min(y.len());
    if w == 0 {
        return crate::bdd::FALSE_NODE;
    }
    let top = w - 1;
    let mut xf = x[..w].to_vec();
    let mut yf = y[..w].to_vec();
    let nx = b.not(xf[top]);
    xf[top] = nx;
    let ny = b.not(yf[top]);
    yf[top] = ny;
    ult_bits(b, &xf, &yf)
}

/// `expr == const` where `expr` may be arithmetic.
fn eq_arith(b: &mut Bdd, e: &Expr, k: i64, vars: &[Var], width: u32) -> Option<u32> {
    let bits = bits_of(b, e, vars, width)?;
    let mut acc = crate::bdd::TRUE_NODE;
    for (i, node) in bits.iter().enumerate() {
        let want = if (k >> i) & 1 == 1 {
            crate::bdd::TRUE_NODE
        } else {
            crate::bdd::FALSE_NODE
        };
        let lit = if want == crate::bdd::TRUE_NODE {
            *node
        } else {
            b.not(*node)
        };
        acc = b.and(acc, lit);
    }
    Some(acc)
}

fn const_of(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(n) => Some(*n),
        Expr::Unary(op, a) if op == "-" => Some(-const_of(a)?),
        Expr::Binary(op, a, c) => {
            let x = const_of(a)?;
            let y = const_of(c)?;
            match op.as_str() {
                "+" => Some(x + y),
                "-" => Some(x - y),
                "*" => Some(x * y),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Identify `var OP const` (or `const OP var`, flipped) for a comparison.
fn comparison_sides<'a>(a: &'a Expr, c: &'a Expr, vars: &'a [Var]) -> Option<(&'a Var, i64)> {
    if let (Expr::Path(p), _) = (a, c) {
        if let Some(v) = find(vars, p) {
            if let Some(k) = const_of(c) {
                return Some((v, k));
            }
        }
    }
    if let (_, Expr::Path(p)) = (a, c) {
        if let Some(v) = find(vars, p) {
            if let Some(k) = const_of(a) {
                return Some((v, k));
            }
        }
    }
    None
}

/// Is the action's constraint set satisfiable? Exact, or NOT PROVED.
pub fn prove_sat(model: &Model, action: &Action) -> Proof {
    let vars = match eligible(model, action) {
        Ok(v) => v,
        Err(why) => return Proof::NotProved(why),
    };
    if vars.is_empty() {
        return Proof::NotProved("no random variables with a small domain".into());
    }
    let mut b = Bdd::new();
    for (i, v) in vars.iter().enumerate() {
        for k in 0..v.bits {
            b.set_order(v.base + k, (i as u32) * 32 + k);
        }
    }
    let mut acc = crate::bdd::TRUE_NODE;
    for c in &action.constraints {
        let node = match translate(&mut b, &c.expr, &vars) {
            Some(n) => n,
            None => {
                return Proof::NotProved(format!(
                    "a constraint could not be expressed as a Boolean circuit: {}",
                    crate::solver::render_expr(&c.expr)
                ))
            }
        };
        acc = b.and(acc, node);
        if b.is_false(acc) {
            return Proof::Unsat;
        }
    }
    if b.is_false(acc) {
        return Proof::Unsat;
    }
    let mut out = BTreeMap::new();
    for (bv, val) in b.witness(acc).unwrap_or_default() {
        for v in &vars {
            if bv >= v.base && bv < v.base + v.bits {
                let e = out.entry(v.name.clone()).or_insert(0i64);
                if val {
                    *e |= 1 << (bv - v.base);
                }
            }
        }
    }
    for v in &vars {
        let e = out.entry(v.name.clone()).or_insert(0);
        *e += v.lo;
    }
    Proof::Sat(out)
}

/// Are two actions' constraint sets equivalent?
///
/// Canonical form makes this a pointer comparison: two constraints that mean the
/// same thing collapse to the same BDD node, so this is exact rather than
/// statistical.
pub fn prove_equivalent(model: &Model, left: &Action, right: &Action) -> Proof {
    let lv = match eligible(model, left) {
        Ok(v) => v,
        Err(why) => return Proof::NotProved(why),
    };
    let rv = match eligible(model, right) {
        Ok(v) => v,
        Err(why) => return Proof::NotProved(why),
    };
    if lv.len() != rv.len() {
        return Proof::NotProved(format!(
            "`{}` has {} random variable(s), `{}` has {}",
            left.name, lv.len(), right.name, rv.len()
        ));
    }
    // Both sides share one manager, so the two functions are comparable.
    let mut b = Bdd::new();
    for (i, v) in lv.iter().enumerate() {
        for k in 0..v.bits {
            b.set_order(v.base + k, (i as u32) * 32 + k);
        }
    }
    let build = |b: &mut Bdd, vars: &[Var], act: &Action| -> Option<u32> {
        let mut acc = crate::bdd::TRUE_NODE;
        for c in &act.constraints {
            let node = translate(b, &c.expr, vars)?;
            acc = b.and(acc, node);
        }
        Some(acc)
    };
    let ln = match build(&mut b, &lv, left) {
        Some(n) => n,
        None => return Proof::NotProved("a constraint could not be expressed as a Boolean circuit".into()),
    };
    let rn = match build(&mut b, &rv, right) {
        Some(n) => n,
        None => return Proof::NotProved("a constraint could not be expressed as a Boolean circuit".into()),
    };
    if ln == rn {
        return Proof::Vacuous(Vec::new());
    }
    // Not the same node: show that they differ by finding a separating case.
    let diff = b.xor(ln, rn);
    if b.is_false(diff) {
        Proof::Vacuous(Vec::new())
    } else {
        Proof::Restricting(vec![
            "the two constraint sets admit different assignments".to_string()
        ])
    }
}

/// Build the constraint set under a given variable ordering and report the node
/// count. This is the demonstration that ordering decides the size of the
/// diagram — the same function, two orders, very different graphs.
pub fn prove_order(model: &Model, action: &Action) -> Proof {
    let vars = match eligible(model, action) {
        Ok(v) => v,
        Err(why) => return Proof::NotProved(why),
    };
    if vars.is_empty() {
        return Proof::NotProved("no random variables with a small domain".into());
    }
    let build = |reverse: bool| -> Option<usize> {
        let mut b = Bdd::new();
        let n = vars.len() as u32;
        for (i, v) in vars.iter().enumerate() {
            let rank_base = if reverse { (n - 1 - i as u32) * 32 } else { i as u32 * 32 };
            for k in 0..v.bits {
                b.set_order(v.base + k, rank_base + k);
            }
        }
        let mut acc = crate::bdd::TRUE_NODE;
        for c in &action.constraints {
            let node = translate(&mut b, &c.expr, &vars)?;
            acc = b.and(acc, node);
        }
        Some(b.node_count())
    };
    let decl = match build(false) {
        Some(n) => n,
        None => return Proof::NotProved("a constraint could not be expressed as a Boolean circuit".into()),
    };
    let rev = match build(true) {
        Some(n) => n,
        None => return Proof::NotProved("a constraint could not be expressed as a Boolean circuit".into()),
    };
    let ratio = if decl == 0 { 0.0 } else { rev as f64 / decl as f64 };
    Proof::Restricting(vec![format!(
        "{} node(s) in declaration order, {} in reverse order ({:.1}x)",
        decl, rev, ratio
    )])
}

/// Which constraints restrict nothing? A vacuous constraint is a silent no-op.
pub fn prove_vacuity(model: &Model, action: &Action) -> Proof {
    let vars = match eligible(model, action) {
        Ok(v) => v,
        Err(why) => return Proof::NotProved(why),
    };
    if vars.is_empty() {
        return Proof::NotProved("no random variables with a small domain".into());
    }
    let mut b = Bdd::new();
    for (i, v) in vars.iter().enumerate() {
        for k in 0..v.bits {
            b.set_order(v.base + k, (i as u32) * 32 + k);
        }
    }
    let mut vacuous = Vec::new();
    let mut restricting = Vec::new();
    // A field's declared range is lowered into the constraint list, but it is
    // the domain declaration, not something the user wrote — reporting it as
    // "vacuous" is a false positive.
    let declared: Vec<String> = action
        .fields
        .iter()
        .filter(|f| f.inline_constraint.is_some())
        .map(|f| f.name.rsplit('.').next().unwrap_or(&f.name).to_string())
        .collect();
    for c in &action.constraints {
        let is_declaration = match &c.expr {
            Expr::InRange(subj, _, _) | Expr::InSet(subj, _) => match subj.as_ref() {
                Expr::Path(p) => declared.contains(&p.segments.join(".")),
                _ => false,
            },
            _ => false,
        };
        if is_declaration {
            continue;
        }
        let node = match translate(&mut b, &c.expr, &vars) {
            Some(n) => n,
            None => {
                return Proof::NotProved(format!(
                    "a constraint could not be expressed as a Boolean circuit: {}",
                    crate::solver::render_expr(&c.expr)
                ))
            }
        };
        if b.is_true(node) {
            vacuous.push(crate::solver::render_expr(&c.expr));
        } else {
            restricting.push(crate::solver::render_expr(&c.expr));
        }
    }
    if vacuous.is_empty() {
        Proof::Restricting(restricting)
    } else {
        Proof::Vacuous(vacuous)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn model_of(src: &str) -> Model {
        let f = parser::parse_source(src).unwrap();
        Model::lower(&f).unwrap()
    }

    #[test]
    fn an_unsatisfiable_pair_is_proved_unsat() {
        let m = model_of("component c { action a { rand int x in [1..5]; constraint x in [10..20]; } };");
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        assert_eq!(prove_sat(&m, &a), Proof::Unsat);
    }

    #[test]
    fn a_vacuous_constraint_is_flagged() {
        // `x >= 0` on a domain that starts at 0 restricts nothing.
        let m = model_of("component c { action a { rand int x in [0..7]; constraint x >= 0; } };");
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        // The declared range `x in [0..7]` is the domain, not a user
        // constraint, so only `x >= 0` may be reported.
        match prove_vacuity(&m, &a) {
            Proof::Vacuous(v) => {
                assert_eq!(v.len(), 1, "{:?}", v);
                assert!(v[0].contains(">="), "{:?}", v);
            }
            other => panic!("expected Vacuous, got {:?}", other),
        }
    }

    #[test]
    fn a_real_constraint_is_not_vacuous() {
        let m = model_of("component c { action a { rand int x in [0..7]; constraint x >= 3; } };");
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        assert!(matches!(prove_vacuity(&m, &a), Proof::Restricting(_)));
    }

    #[test]
    fn equivalent_rewrites_are_the_same_node() {
        // The showcase: `x >= 3` and `x > 2` mean the same thing, and canonical
        // form makes that a pointer comparison rather than a sample.
        let m = model_of(r#"
component c {
  action a { rand int x in [0..7]; constraint x >= 3; }
  action b { rand int x in [0..7]; constraint x > 2; }
  action d { rand int x in [0..7]; constraint x >= 4; }
};
"#);
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        let b = m.find_action(Some("c"), "b").unwrap().clone();
        let d = m.find_action(Some("c"), "d").unwrap().clone();
        assert!(
            matches!(prove_equivalent(&m, &a, &b), Proof::Vacuous(_)),
            "x >= 3 and x > 2 must prove equivalent"
        );
        assert!(
            matches!(prove_equivalent(&m, &a, &d), Proof::Restricting(_)),
            "x >= 3 and x >= 4 must NOT prove equivalent"
        );
    }

    #[test]
    fn arithmetic_agrees_with_direct_evaluation() {
        // The adder is the newest and most error-prone circuit: verify it by
        // enumeration, the same way `apply` was verified.
        for k in 0i64..=14 {
            let m = model_of(&format!(
                "component c {{ action a {{ rand int x in [0..7]; rand int y in [0..7]; constraint x + y == {}; }} }};",
                k
            ));
            let a = m.find_action(Some("c"), "a").unwrap().clone();
            let vars = eligible(&m, &a).unwrap();
            let mut b = Bdd::new();
            for (i, v) in vars.iter().enumerate() {
                for q in 0..v.bits {
                    b.set_order(v.base + q, (i as u32) * 32 + q);
                }
            }
            let node = translate(&mut b, &a.constraints[0].expr, &vars).unwrap();
            for x in 0i64..=7 {
                for y in 0i64..=7 {
                    let wx = eq_const(&mut b, &vars[0], x).unwrap();
                    let wy = eq_const(&mut b, &vars[1], y).unwrap();
                    let both = b.and(wx, wy);
                    let conj = b.and(node, both);
                    let hit = !b.is_false(conj);
                    assert_eq!(hit, x + y == k, "x={} y={} k={}", x, y, k);
                }
            }
        }
    }

    /// Build an action's constraint set into one BDD, under declaration order or
    /// the reverse variable order.
    ///
    /// Ranks are contiguous (`0..n_bits`) rather than spaced, so the result can
    /// also be handed to `sat_count`, which walks ranks level by level.
    fn circuit(m: &Model, a: &Action, reverse: bool) -> (Bdd, Vec<Var>, u32) {
        let vars = eligible(m, a).expect("eligible");
        let total: u32 = vars.iter().map(|v| v.bits).sum();
        let mut b = Bdd::new();
        for v in vars.iter() {
            let rank_base = if reverse {
                total - v.base - v.bits
            } else {
                v.base
            };
            for k in 0..v.bits {
                b.set_order(v.base + k, rank_base + k);
            }
        }
        let mut acc = crate::bdd::TRUE_NODE;
        for c in &a.constraints {
            let node = translate(&mut b, &c.expr, &vars)
                .unwrap_or_else(|| panic!("constraint must translate: {}", crate::solver::render_expr(&c.expr)));
            acc = b.and(acc, node);
        }
        (b, vars, acc)
    }

    /// Does the circuit accept this assignment? Conjunction with the equality
    /// circuit for each named value, so the answer is exact.
    fn accepts(b: &mut Bdd, vars: &[Var], acc: u32, assign: &[(&str, i64)]) -> bool {
        let mut conj = acc;
        for (name, val) in assign {
            let v = vars
                .iter()
                .find(|v| v.name.rsplit('.').next() == Some(*name))
                .expect("variable is eligible");
            let w = eq_const(b, v, *val).expect("eq_const");
            conj = b.and(conj, w);
        }
        !b.is_false(conj)
    }

    #[test]
    fn variable_comparisons_agree_with_direct_evaluation() {
        // The acceptance criterion for variable-to-variable comparison: for every
        // assignment in the domain, the BDD's satisfying set must equal direct
        // evaluation of the PSS expression. Verified the same way `apply` and
        // `sat_count` are.
        for op in ["<", "<=", "==", "!=", ">", ">="] {
            let m = model_of(&format!(
                "component c {{ action a {{ rand int x in [0..7]; rand int y in [0..7]; constraint x {} y; }} }};",
                op
            ));
            let a = m.find_action(Some("c"), "a").unwrap().clone();
            let (mut b, vars, acc) = circuit(&m, &a, false);
            for x in 0i64..=7 {
                for y in 0i64..=7 {
                    let want = match op {
                        "<" => x < y,
                        "<=" => x <= y,
                        "==" => x == y,
                        "!=" => x != y,
                        ">" => x > y,
                        _ => x >= y,
                    };
                    assert_eq!(
                        accepts(&mut b, &vars, acc, &[("x", x), ("y", y)]),
                        want,
                        "`x {} y` with x={} y={}",
                        op, x, y
                    );
                }
            }
        }
    }

    #[test]
    fn variable_comparisons_agree_across_different_domains() {
        // `x` and `y` are encoded as offsets from their own lower bounds, so a
        // comparator over the raw encodings would be wrong here. This is the
        // test that catches it.
        for op in ["<", "<=", "==", "!=", ">", ">="] {
            let m = model_of(&format!(
                "component c {{ action a {{ rand int x in [2..9]; rand int y in [-3..4]; constraint x {} y; }} }};",
                op
            ));
            let a = m.find_action(Some("c"), "a").unwrap().clone();
            let (mut b, vars, acc) = circuit(&m, &a, false);
            for x in 2i64..=9 {
                for y in -3i64..=4 {
                    let want = match op {
                        "<" => x < y,
                        "<=" => x <= y,
                        "==" => x == y,
                        "!=" => x != y,
                        ">" => x > y,
                        _ => x >= y,
                    };
                    assert_eq!(
                        accepts(&mut b, &vars, acc, &[("x", x), ("y", y)]),
                        want,
                        "`x {} y` with x={} y={}",
                        op, x, y
                    );
                }
            }
        }
    }

    #[test]
    fn sum_of_two_variables_equals_a_third_agrees_with_direct_evaluation() {
        let m = model_of(
            "component c { action a { rand int x in [0..7]; rand int y in [0..7]; rand int z in [0..7]; constraint x + y == z; } };",
        );
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        let (mut b, vars, acc) = circuit(&m, &a, false);
        let mut hits = 0;
        for x in 0i64..=7 {
            for y in 0i64..=7 {
                for z in 0i64..=7 {
                    let want = x + y == z;
                    let got = accepts(&mut b, &vars, acc, &[("x", x), ("y", y), ("z", z)]);
                    assert_eq!(got, want, "x + y == z with x={} y={} z={}", x, y, z);
                    if got {
                        hits += 1;
                    }
                }
            }
        }
        // 36 pairs satisfy x + y <= 7, and z is determined by each.
        assert_eq!(hits, 36, "x + y == z has 36 satisfying assignments on 0..7");
        // And the count agrees with `sat_count`, which is the other way the
        // engine is checked.
        assert_eq!(b.sat_count(acc, vars.iter().map(|v| v.bits).sum()), 36);
    }

    #[test]
    fn a_three_variable_model_shows_an_order_difference() {
        // `x + y == z` relates the three variables, which is what variable
        // ordering can act on: the same function, two orders, different graphs.
        let m = model_of(
            "component c { action a { rand int x in [0..7]; rand int y in [0..7]; rand int z in [0..7]; constraint x + y == z; } };",
        );
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        let (mut bd, vd, ad) = circuit(&m, &a, false);
        let (mut br, vr, ar) = circuit(&m, &a, true);
        assert_ne!(
            bd.node_count(),
            br.node_count(),
            "the two orderings must give different node counts"
        );
        // Both orders describe the same function, checked by enumeration — a
        // smaller diagram must not be a different (wrong) one.
        for x in 0i64..=7 {
            for y in 0i64..=7 {
                for z in 0i64..=7 {
                    let want = x + y == z;
                    let got = accepts(&mut bd, &vd, ad, &[("x", x), ("y", y), ("z", z)]);
                    assert_eq!(got, want, "declaration order x={} y={} z={}", x, y, z);
                    let got = accepts(&mut br, &vr, ar, &[("x", x), ("y", y), ("z", z)]);
                    assert_eq!(got, want, "reverse order x={} y={} z={}", x, y, z);
                }
            }
        }
        match prove_order(&m, &a) {
            Proof::Restricting(v) => assert!(v[0].contains("node(s)"), "{:?}", v),
            other => panic!("expected Restricting, got {:?}", other),
        }
    }

    #[test]
    fn relations_outside_the_translator_still_abstain() {
        // Multiplication of two variables, division and a product on one side are
        // outside the circuit, so the prover must decline rather than guess.
        for c in ["x * y == 6", "x / y == 2", "x * 2 == 4"] {
            let m = model_of(&format!(
                "component c {{ action a {{ rand int x in [0..7]; rand int y in [0..7]; constraint {}; }} }};",
                c
            ));
            let a = m.find_action(Some("c"), "a").unwrap().clone();
            assert!(
                matches!(prove_sat(&m, &a), Proof::NotProved(_)),
                "`{}` must abstain, not guess",
                c
            );
            assert!(matches!(prove_order(&m, &a), Proof::NotProved(_)));
        }
        // A variable compared to something that is not a variable or a constant
        // is outside the encoding too.
        let m = model_of(
            "component c { action a { rand int x in [0..7]; rand int y in [0..7]; constraint x < y * y; } };",
        );
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        assert!(matches!(prove_sat(&m, &a), Proof::NotProved(_)));
    }

    #[test]
    fn ordering_reports_node_counts_and_abstains_on_arithmetic() {
        let m = model_of("component c { action a { rand int x in [0..7]; constraint x >= 3; } };");
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        match prove_order(&m, &a) {
            Proof::Restricting(v) => assert!(v[0].contains("node(s)"), "{:?}", v),
            other => panic!("expected Restricting, got {:?}", other),
        }
        // Arithmetic is now translated, so this reports node counts rather
        // than abstaining. (This assertion used to expect NotProved; the
        // arithmetic support added beside it made that obsolete.)
        let m = model_of(
            "component c { action a { rand int x in [0..7]; rand int y in [0..7]; constraint x + y == 7; } };",
        );
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        assert!(matches!(prove_order(&m, &a), Proof::Restricting(_)));
        // An expression outside the translator must still abstain.
        let m = model_of(
            "component c { action a { rand int x in [0..7]; rand int y in [0..7]; constraint x * y == 7; } };",
        );
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        assert!(matches!(prove_order(&m, &a), Proof::NotProved(_)));
    }

    #[test]
    fn a_wide_domain_abstains_rather_than_guessing() {
        let m = model_of("component c { action a { rand int x; } };");
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        assert!(matches!(prove_sat(&m, &a), Proof::NotProved(_)));
    }

    #[test]
    fn the_witness_satisfies_every_constraint() {
        let m = model_of(
            "component c { action a { rand int x in [0..7]; constraint x >= 3; constraint x <= 5; } };",
        );
        let a = m.find_action(Some("c"), "a").unwrap().clone();
        match prove_sat(&m, &a) {
            Proof::Sat(w) => {
                let x = w["x"];
                assert!((3..=5).contains(&x), "witness {} violates the constraints", x);
            }
            other => panic!("expected Sat, got {:?}", other),
        }
    }

    #[test]
    fn the_bdd_agrees_with_direct_evaluation() {
        // The test that matters: for every value in the domain, the BDD's answer
        // must equal evaluating the PSS expression directly. A wrong comparator
        // is the classic BDD bug.
        for (lo, hi, k) in [(0i64, 7i64, 3i64), (0, 15, 0), (0, 15, 15), (2, 9, 5)] {
            let m = model_of(&format!(
                "component c {{ action a {{ rand int x in [{}..{}]; constraint x < {}; }} }};",
                lo, hi, k
            ));
            let a = m.find_action(Some("c"), "a").unwrap().clone();
            let vars = eligible(&m, &a).unwrap();
            let mut b = Bdd::new();
            for (i, v) in vars.iter().enumerate() {
                for q in 0..v.bits {
                    b.set_order(v.base + q, (i as u32) * 32 + q);
                }
            }
            let node = translate(&mut b, &a.constraints[0].expr, &vars).unwrap();
            for value in lo..=hi {
                let w = eq_const(&mut b, &vars[0], value).unwrap();
                let conj = b.and(node, w);
                let hit = !b.is_false(conj);
                assert_eq!(
                    hit,
                    value < k,
                    "x in [{}..{}], x < {}: x={} gave {}",
                    lo, hi, k, value, hit
                );
            }
        }
    }
}
