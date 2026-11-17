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
                let (v, k) = comparison_sides(a, c, vars)?;
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
                    // `>=` must be the negation of `<`; falling through to the
                    // `<` arm silently turned `x >= 0` into `x < 0`.
                    _ => {
                        let q = lt_const(b, v, k)?;
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
