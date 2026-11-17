//! A reduced ordered binary decision diagram (ROBDD), used to *prove* things
//! about a PSS constraint set rather than sample it.
//!
//! Why a BDD at all: it is the only structure here that gives a canonical form,
//! so equivalence is a pointer comparison, `sat_count` counts every satisfying
//! assignment rather than the ones a seed happened to find, and "this constraint
//! restricts nothing" becomes a decision instead of an impression.
//!
//! Why it is deliberately narrow: a BDD over a 32-bit PSS integer is a memory
//! leak, not a data structure. Only small domains are eligible — `bool`, an
//! enum, `bit[N]` for small N, and a declared range of at most
//! [`MAX_DOMAIN`] values. Anything else makes the proof abstain, and the caller
//! reports **NOT PROVED** rather than guessing.

use std::collections::HashMap;

/// The largest domain a variable may have and still be BDD-eligible.
pub const MAX_DOMAIN: i64 = 256;

const FALSE: u32 = 0;
const TRUE: u32 = 1;

#[derive(Clone, Copy)]
struct Node {
    var: u32,
    low: u32,
    high: u32,
}

/// A hash-consed ROBDD manager.
///
/// Nodes are only ever created through [`Bdd::mk`], which is what makes the
/// representation canonical: two equal subgraphs are the same index.
pub struct Bdd {
    nodes: Vec<Node>,
    unique: HashMap<(u32, u32, u32), u32>,
    /// Cache for `apply`, keyed by (op, a, b).
    cache: HashMap<(u8, u32, u32), u32>,
    order: HashMap<u32, u32>,
}

const OP_AND: u8 = 1;
const OP_OR: u8 = 2;
const OP_XOR: u8 = 3;

impl Default for Bdd {
    fn default() -> Self {
        Self::new()
    }
}

impl Bdd {
    pub fn new() -> Self {
        Bdd {
            // Index 0 is FALSE, 1 is TRUE; they carry no variable.
            nodes: vec![
                Node { var: u32::MAX, low: FALSE, high: FALSE },
                Node { var: u32::MAX, low: TRUE, high: TRUE },
            ],
            unique: HashMap::new(),
            cache: HashMap::new(),
            order: HashMap::new(),
        }
    }

    /// Declare the relative position of a BDD variable. Lower is nearer the
    /// root. Ordering is what decides the size of the diagram, so it is set
    /// explicitly rather than left to insertion order.
    pub fn set_order(&mut self, var: u32, rank: u32) {
        self.order.insert(var, rank);
        self.cache.clear();
    }

    fn rank(&self, var: u32) -> u32 {
        self.order.get(&var).copied().unwrap_or(var)
    }

    fn var_of(&self, n: u32) -> Option<u32> {
        let node = self.nodes[n as usize];
        if n <= TRUE {
            None
        } else {
            Some(node.var)
        }
    }

    /// The only constructor. Returns FALSE/TRUE for the constants.
    pub fn mk(&mut self, var: u32, low: u32, high: u32) -> u32 {
        if low == high {
            return low;
        }
        let key = (var, low, high);
        if let Some(&n) = self.unique.get(&key) {
            return n;
        }
        let n = self.nodes.len() as u32;
        self.nodes.push(Node { var, low, high });
        self.unique.insert(key, n);
        n
    }

    pub fn is_true(&self, n: u32) -> bool {
        n == TRUE
    }

    pub fn is_false(&self, n: u32) -> bool {
        n == FALSE
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len().saturating_sub(2)
    }

    fn top_rank(&self, a: u32, b: u32) -> u32 {
        let ra = self.var_of(a).map(|v| self.rank(v));
        let rb = self.var_of(b).map(|v| self.rank(v));
        match (ra, rb) {
            (Some(x), Some(y)) => x.min(y),
            (Some(x), None) => x,
            (None, Some(y)) => y,
            (None, None) => u32::MAX,
        }
    }

    fn cofactors(&self, n: u32, rank: u32) -> (u32, u32) {
        match self.var_of(n) {
            Some(v) if self.rank(v) == rank => {
                let node = self.nodes[n as usize];
                (node.low, node.high)
            }
            _ => (n, n),
        }
    }

    fn apply_rec(&mut self, op: u8, a: u32, b: u32) -> u32 {
        // Terminal cases.
        match op {
            OP_AND => {
                if a == FALSE || b == FALSE {
                    return FALSE;
                }
                if a == TRUE {
                    return b;
                }
                if b == TRUE || a == b {
                    return a;
                }
            }
            OP_OR => {
                if a == TRUE || b == TRUE {
                    return TRUE;
                }
                if a == FALSE {
                    return b;
                }
                if b == FALSE || a == b {
                    return a;
                }
            }
            OP_XOR => {
                if a == FALSE {
                    return b;
                }
                if b == FALSE {
                    return a;
                }
                if a == b {
                    return FALSE;
                }
            }
            _ => unreachable!(),
        }
        if let Some(&r) = self.cache.get(&(op, a, b)) {
            return r;
        }
        let rank = self.top_rank(a, b);
        let (a0, a1) = self.cofactors(a, rank);
        let (b0, b1) = self.cofactors(b, rank);
        let low = self.apply_rec(op, a0, b0);
        let high = self.apply_rec(op, a1, b1);
        let var = self.var_of(a).filter(|v| self.rank(*v) == rank)
            .or_else(|| self.var_of(b).filter(|v| self.rank(*v) == rank))
            .unwrap_or(0);
        let r = self.mk(var, low, high);
        self.cache.insert((op, a, b), r);
        r
    }

    pub fn and(&mut self, a: u32, b: u32) -> u32 {
        self.apply_rec(OP_AND, a, b)
    }

    pub fn or(&mut self, a: u32, b: u32) -> u32 {
        self.apply_rec(OP_OR, a, b)
    }

    pub fn xor(&mut self, a: u32, b: u32) -> u32 {
        self.apply_rec(OP_XOR, a, b)
    }

    pub fn not(&mut self, a: u32) -> u32 {
        self.xor(a, TRUE)
    }

    /// The number of assignments that satisfy `n`, given `n_vars` variables.
    pub fn sat_count(&self, n: u32, n_vars: u32) -> u128 {
        fn go(b: &Bdd, n: u32, level: u32, n_vars: u32, memo: &mut HashMap<(u32, u32), u128>) -> u128 {
            if n == FALSE {
                return 0;
            }
            if level >= n_vars {
                return if n == TRUE { 1 } else { 0 };
            }
            if let Some(&v) = memo.get(&(n, level)) {
                return v;
            }
            let var = match b.var_of(n) {
                Some(v) => v,
                None => {
                    // Constant: every remaining variable is free.
                    return 1u128 << (n_vars - level);
                }
            };
            let r = b.rank(var);
            let skip = if r > level { (r - level) as u32 } else { 0 };
            let node = b.nodes[n as usize];
            let v = (go(b, node.low, r + 1, n_vars, memo)
                + go(b, node.high, r + 1, n_vars, memo))
                << skip;
            memo.insert((n, level), v);
            v
        }
        let mut memo = HashMap::new();
        go(self, n, 0, n_vars, &mut memo)
    }

    /// A satisfying assignment, or `None` when `n` is unsatisfiable.
    pub fn witness(&self, n: u32) -> Option<Vec<(u32, bool)>> {
        if n == FALSE {
            return None;
        }
        let mut out = Vec::new();
        let mut cur = n;
        while cur != TRUE {
            let node = self.nodes[cur as usize];
            // Prefer the low branch, which keeps the witness deterministic.
            out.push((node.var, false));
            if node.low == FALSE {
                out.pop();
                out.push((node.var, true));
                cur = node.high;
            } else {
                cur = node.low;
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(b: &mut Bdd, v: u32) -> u32 {
        let f = FALSE;
        let t = TRUE;
        b.mk(v, f, t)
    }

    #[test]
    fn constants_and_identity() {
        let mut b = Bdd::new();
        let x = var(&mut b, 0);
        assert_eq!(b.and(x, TRUE), x);
        assert_eq!(b.and(x, FALSE), FALSE);
        assert_eq!(b.or(x, FALSE), x);
        assert_eq!(b.or(x, TRUE), TRUE);
        assert_eq!(b.xor(x, x), FALSE);
        assert_eq!(b.not(TRUE), FALSE);
        assert_eq!(b.not(FALSE), TRUE);
    }

    #[test]
    fn canonical_form_makes_equivalence_structural() {
        // `x & y` and `y & x` must be the *same node*, which is the whole point.
        let mut b = Bdd::new();
        b.set_order(0, 0);
        b.set_order(1, 1);
        let x = var(&mut b, 0);
        let y = var(&mut b, 1);
        let a = b.and(x, y);
        let c = b.and(y, x);
        assert_eq!(a, c, "canonical form: the same function is the same node");
        // And a rewrite that means the same thing.
        let nx = b.not(x);
        let ny = b.not(y);
        let o = b.or(nx, ny);
        let d = b.not(o);
        assert_eq!(a, d, "De Morgan rewrite must collapse to the same node");
    }

    #[test]
    fn apply_matches_brute_force() {
        // For every pair of a few functions over 3 variables, compare the BDD
        // result against evaluation on all 8 assignments. A wrong comparator in
        // translation is the classic BDD bug, so this is the test that matters.
        for mask_a in 0u32..8 {
            for mask_b in 0u32..8 {
                for op in [OP_AND, OP_OR, OP_XOR] {
                    let mut b = Bdd::new();
                    for v in 0..3 {
                        b.set_order(v, v);
                    }
                    let fa = build_from_mask(&mut b, mask_a, 3);
                    let fb = build_from_mask(&mut b, mask_b, 3);
                    let r = b.apply_rec(op, fa, fb);
                    for assign in 0..8u32 {
                        let want = match op {
                            OP_AND => (mask_a >> assign & 1) & (mask_b >> assign & 1),
                            OP_OR => (mask_a >> assign & 1) | (mask_b >> assign & 1),
                            _ => (mask_a >> assign & 1) ^ (mask_b >> assign & 1),
                        } == 1;
                        let got = eval_bdd(&b, r, assign);
                        assert_eq!(got, want, "op {} mask {:03b} {:03b} assign {:03b}",
                                   op, mask_a, mask_b, assign);
                    }
                }
            }
        }
    }

    /// Build a BDD whose truth table is `mask`: bit `i` is the value at
    /// assignment `i`, where bit `v` of the assignment is variable `v`.
    fn build_from_mask(b: &mut Bdd, mask: u32, n: u32) -> u32 {
        fn rec(b: &mut Bdd, mask: u32, n: u32, v: u32, prefix: u32) -> u32 {
            if v == n {
                return if (mask >> prefix) & 1 == 1 { TRUE } else { FALSE };
            }
            let lo = rec(b, mask, n, v + 1, prefix);
            let hi = rec(b, mask, n, v + 1, prefix | (1 << v));
            b.mk(v, lo, hi)
        }
        rec(b, mask, n, 0, 0)
    }

    fn eval_bdd(b: &Bdd, mut n: u32, assign: u32) -> bool {
        loop {
            if n == TRUE {
                return true;
            }
            if n == FALSE {
                return false;
            }
            let node = b.nodes[n as usize];
            let bit = (assign >> node.var) & 1 == 1;
            n = if bit { node.high } else { node.low };
        }
    }

    #[test]
    fn sat_count_matches_enumeration() {
        for mask in 0u32..16 {
            let mut b = Bdd::new();
            for v in 0..4 {
                b.set_order(v, v);
            }
            let f = build_from_mask(&mut b, mask, 4);
            let got = b.sat_count(f, 4);
            let want = mask.count_ones() as u128;
            assert_eq!(got, want, "mask {:04b}", mask);
        }
    }

    #[test]
    fn witness_satisfies_the_function() {
        let mut b = Bdd::new();
        b.set_order(0, 0);
        b.set_order(1, 1);
        let x = var(&mut b, 0);
        let y = var(&mut b, 1);
        let ny = b.not(y);
        let f = b.and(x, ny);
        let w = b.witness(f).expect("satisfiable");
        let mut assign = 0u32;
        for (v, val) in &w {
            if *val {
                assign |= 1 << v;
            }
        }
        assert!(eval_bdd(&b, f, assign), "witness must satisfy the function");
    }
}
