//! Intermediate representation: the semantic model Satya solves and executes.
//!
//! Lowering is intentionally *lenient*. A real PSS model imports packages that
//! Satya may not have (methodology libraries, address/executor packages), so
//! unknown types are kept as opaque references rather than rejected. This lets
//! a partial or single-file model still yield a usable component tree,
//! constraints and activity graph.

use std::collections::HashMap;

use crate::ast::*;
use crate::error::PssResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Plain,
    Rand,
    Input,
    Output,
    Lock,
    Share,
}

impl FieldKind {
    pub fn is_flow(self) -> bool {
        matches!(self, FieldKind::Input | FieldKind::Output)
    }
    pub fn is_rand(self) -> bool {
        matches!(self, FieldKind::Rand)
    }
}

/// The broad category a field's type falls into, after name resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeCategory {
    /// `int`, `bit[N]`, `bool`, … — an integer value.
    Scalar,
    /// A user enum, with its ordered variants.
    Enum(Vec<String>),
    /// A struct/resource/buffer/state defined in the model.
    Struct(String),
    /// A handle / address type (opaque to the solver).
    Handle,
    /// A sub-component type.
    Component(String),
    /// Not resolvable within this model.
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
    pub type_name: String,
    pub category: TypeCategory,
    pub bits: Option<i64>,
    /// Constraint attached at the declaration site (`rand int in [1..10] x`).
    pub inline_constraint: Option<Expr>,
    /// Explicit array extent (pool-like fields).
    pub array: Option<i64>,
    /// Initialiser at the declaration site, e.g. `int a = 17;`.
    pub init: Option<Expr>,
    /// For `array<T, N>` / `list<T>`: the element type's name.
    pub elem_type: Option<String>,
    /// For `array<bit[N], M>`: the element's width.
    pub elem_bits: Option<i64>,
}

impl Field {
    pub fn is_numeric(&self) -> bool {
        matches!(self.category, TypeCategory::Scalar | TypeCategory::Enum(_))
    }
}

#[derive(Debug, Clone)]
pub struct Constraint {
    pub name: Option<String>,
    pub expr: Expr,
    pub is_default: bool,
    pub is_disable: bool,
}

#[derive(Debug, Clone)]
pub struct Action {
    pub name: String,
    pub base: Option<String>,
    pub is_abstract: bool,
    pub fields: Vec<Field>,
    pub constraints: Vec<Constraint>,
    pub execs: Vec<ExecBlock>,
    pub activity: Option<Activity>,
    /// Leaf name of the component that declared this action, if any.
    pub owner: Option<String>,
}

impl Action {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
    pub fn flow_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(|f| f.kind.is_flow())
    }
    pub fn rand_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(|f| f.kind.is_rand())
    }
}

#[derive(Debug, Clone)]
pub struct DataType {
    pub name: String,
    pub keyword: String,
    pub base: Option<String>,
    pub fields: Vec<Field>,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone)]
pub struct Pool {
    pub name: String,
    pub type_name: String,
    pub count: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Bind {
    pub pool: String,
    pub wildcard: bool,
    pub target: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Component {
    pub name: String,
    pub base: Option<String>,
    /// Set when the component is a register: `reg_c<R, ACC, SZ>` (PSS 24.11.1).
    pub reg_value_type: Option<String>,
    pub reg_access: String,
    pub reg_size: Option<i64>,
    /// (instance name, component type leaf)
    pub subcomponents: Vec<(String, String)>,
    pub fields: Vec<Field>,
    pub actions: Vec<String>,
    pub pools: Vec<Pool>,
    pub binds: Vec<Bind>,
    /// Named activity fragments declared with `symbol` (PSS 12.7).
    pub symbols: HashMap<String, crate::ast::Activity>,
    /// `override` statements in this component (PSS 20.5).
    pub overrides: Vec<crate::ast::OverrideDecl>,
    pub execs: Vec<ExecBlock>,
    pub constraints: Vec<Constraint>,
}

/// An address region declared by the model (typically set up in an `exec
/// init_up` block, e.g. `mem_region.addr = 0x400; mem_region.size = 4096;`).
#[derive(Debug, Clone, Default)]
pub struct AddrRegion {
    pub name: String,
    pub base: i64,
    pub size: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub constants: HashMap<String, i64>,
    pub enums: HashMap<String, Vec<String>>,
    pub types: HashMap<String, DataType>,
    pub components: HashMap<String, Component>,
    pub actions: HashMap<String, Action>,
    pub packages: Vec<String>,
    pub top: Option<String>,
    /// Address regions discovered in the model, in declaration order.
    pub addr_regions: Vec<AddrRegion>,
    /// Qualified name -> canonical (leaf) key, e.g. `pssm_pkg::data_buf` ->
    /// `data_buf`, and `IP_B_c::B_process` -> `B_process`.
    pub qual_index: HashMap<String, String>,
    /// Leaf names declared in more than one scope (a real ambiguity).
    pub duplicates: Vec<String>,
    /// Covergroups declared in the model (PSS 18).
    pub covergroups: Vec<crate::ast::Covergroup>,
    /// Monitor declarations in the model (PSS 19.1).
    pub monitors: Vec<crate::ast::Monitor>,
}

impl Model {
    /// Resolve a possibly package- or component-qualified name to its canonical
    /// key: exact qualified match first, then the leaf name.
    pub fn resolve(&self, name: &str) -> String {
        if name.contains("::") {
            if let Some(k) = self.qual_index.get(name) {
                return k.clone();
            }
            return name.rsplit("::").next().unwrap_or(name).to_string();
        }
        name.to_string()
    }

    /// The leaf (display) name of a type key.
    pub fn type_leaf(&self, key: &str) -> String {
        self.types
            .get(key)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| key.rsplit("::").next().unwrap_or(key).to_string())
    }

    /// True when `leaf` is defined in more than one scope.
    pub fn is_ambiguous(&self, leaf: &str) -> bool {
        self.duplicates.iter().any(|d| d == leaf)
    }

    pub fn lower(file: &SourceFile) -> PssResult<Model> {
        let mut lw = Lowerer::default();
        lw.collect(file.items.iter(), None);
        lw.finish();
        lw.model.covergroups = file.covergroups.clone();
        lw.model.monitors = file.monitors.clone();
        Ok(lw.model)
    }

    pub fn component(&self, name: &str) -> Option<&Component> {
        self.components.get(name)
    }

    pub fn action(&self, name: &str) -> Option<&Action> {
        let leaf = name.rsplit("::").next().unwrap_or(name);
        self.actions.get(leaf)
    }

    /// Find an action, preferring one declared by `owner`, then any action.
    pub fn find_action(&self, owner: Option<&str>, name: &str) -> Option<&Action> {
        let resolved = self.resolve(name);
        let leaf = resolved.as_str();
        if let Some(owner) = owner {
            if let Some(c) = self.components.get(owner) {
                if c.actions.iter().any(|a| a == leaf) {
                    if let Some(a) = self.actions.get(leaf) {
                        return Some(a);
                    }
                }
            }
        }
        self.actions.get(leaf)
    }
}

// ---------------------------------------------------------------------------
// Lowering
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Lowerer {
    model: Model,
    /// Leaf name -> number of scopes that declare it.
    leaf_counts: HashMap<String, usize>,
    // Raw declarations collected before classification.
    raw_types: HashMap<String, RawType>,
    raw_components: HashMap<String, RawComponent>,
    raw_actions: HashMap<String, RawAction>,
    component_names: Vec<String>,
}

#[derive(Default)]
struct RawType {
    name: String,
    keyword: String,
    base: Option<String>,
    members: Vec<Member>,
    constraints: Vec<ConstraintBlock>,
}

#[derive(Default)]
struct RawComponent {
    /// `symbol` declarations, as (name, activity) (PSS 12.7).
    symbols: Vec<(String, crate::ast::Activity)>,
    overrides: Vec<crate::ast::OverrideDecl>,
    /// `reg_c<R, ACC, SZ>` template arguments, when this is a register.
    reg_value_type: Option<String>,
    reg_access: String,
    reg_size: Option<i64>,
    name: String,
    base: Option<String>,
    members: Vec<Member>,
    actions: Vec<ActionDecl>,
    pools: Vec<PoolDecl>,
    binds: Vec<BindDecl>,
    execs: Vec<ExecBlock>,
    constraints: Vec<ConstraintBlock>,
}

struct RawAction {
    decl: ActionDecl,
    owner: Option<String>,
}

impl Lowerer {
    /// Record a declaration's qualified name and count its leaf.
    fn record_qual(&mut self, scope: Option<&str>, leaf: &str) {
        *self.leaf_counts.entry(leaf.to_string()).or_insert(0) += 1;
        if let Some(scope) = scope {
            if !scope.is_empty() {
                self.model
                    .qual_index
                    .insert(format!("{}::{}", scope, leaf), leaf.to_string());
            }
        }
    }

    fn collect<'a>(&mut self, items: impl Iterator<Item = &'a Item>, pkg: Option<String>) {
        for item in items {
            match item {
                Item::Import(_) => {}
                Item::Package(p) => {
                    self.model.packages.push(p.name.clone());
                    self.collect(p.items.iter(), Some(p.name.clone()));
                }
                Item::Enum(e) => {
                    self.record_qual(pkg.as_deref(), &e.name);
                    let entry = self.model.enums.entry(e.name.clone()).or_default();
                    for v in &e.variants {
                        if !entry.contains(v) {
                            entry.push(v.clone());
                        }
                    }
                }
                Item::Struct(t) => {
                    // An `extend struct X` merges into the existing declaration
                    // rather than introducing a new, package-qualified type.
                    let key = if t.is_extend {
                        self.resolve_type_key(&t.name)
                    } else {
                        self.record_qual(pkg.as_deref(), &t.name);
                        qualify(pkg.as_deref(), &t.name)
                    };
                    self.collect_type(t, key)
                }
                Item::Component(c) => {
                    if !c.is_extend {
                        self.record_qual(pkg.as_deref(), &c.name);
                    }
                    self.collect_component(c)
                }
                Item::Action(a) => {
                    if !a.is_extend {
                        self.record_qual(pkg.as_deref(), &a.name);
                    }
                    self.collect_action(a, None)
                }
                Item::StaticConst { name, value } => {
                    self.record_qual(pkg.as_deref(), name);
                    if let Some(v) = value.eval_const(&|n| self.model.constants.get(n).copied()) {
                        self.model.constants.insert(name.clone(), v);
                    } else {
                        self.model.constants.entry(name.clone()).or_insert(0);
                    }
                }
                Item::CompileIf { then_items, .. } => {
                    // Take the primary (then) branch of a `compile if`.
                    self.collect(then_items.iter(), pkg.clone());
                }
                Item::Raw(_) => {}
            }
        }
    }

    fn collect_type(&mut self, t: &TypeDecl, key: String) {
        let e = self.raw_types.entry(key).or_insert_with(|| RawType {
            name: t.name.clone(),
            keyword: t.keyword.clone(),
            base: t.base.as_ref().map(|b| b.name()),
            ..Default::default()
        });
        if t.base.is_some() {
            e.base = t.base.as_ref().map(|b| b.name());
        }
        e.members.extend(t.members.iter().cloned());
        e.constraints.extend(t.constraints.iter().cloned());
    }

    fn collect_component(&mut self, c: &ComponentDecl) {
        if !self.component_names.contains(&c.name) {
            self.component_names.push(c.name.clone());
        }
        for a in &c.items {
            if let CompItem::Action(act) = a {
                self.record_qual(Some(&c.name), &act.name);
            }
        }
        {
            let e = self
                .raw_components
                .entry(c.name.clone())
                .or_insert_with(|| RawComponent {
                    name: c.name.clone(),
                    base: c.base.as_ref().map(|b| b.name()),
                    reg_access: "READWRITE".to_string(),
                    ..Default::default()
                });
            if c.base.is_some() {
                e.base = c.base.as_ref().map(|b| b.name());
            }
            // `reg_c<R, ACC, SZ>` — a register declaration (PSS 24.11.1).
            if let Some(b) = &c.base {
                let leaf = b.name();
                let leaf = leaf.rsplit("::").next().unwrap_or(&leaf).to_string();
                if leaf == "reg_c" || leaf == "reg_group_c" {
                    if let Some(r) = b.template_args.first() {
                        e.reg_value_type = Some(r.name());
                    }
                    for a in &b.template_args {
                        let n = a.name();
                        if n == "READONLY" || n == "WRITEONLY" || n == "READWRITE" {
                            e.reg_access = n;
                        }
                    }
                    if let Some(ca) = b.const_args.first() {
                        if let Some(v) = ca.eval_const(&|_| None) {
                            e.reg_size = Some(v);
                        }
                    }
                }
            }
        }
        for it in &c.items {
            match it {
                CompItem::Member(m) => self.raw_components.get_mut(&c.name).unwrap().members.push(m.clone()),
                CompItem::Action(a) => self.raw_components.get_mut(&c.name).unwrap().actions.push(a.clone()),
                CompItem::Pool(p) => self.raw_components.get_mut(&c.name).unwrap().pools.push(p.clone()),
                CompItem::Bind(b) => self.raw_components.get_mut(&c.name).unwrap().binds.push(b.clone()),
                CompItem::Exec(x) => self.raw_components.get_mut(&c.name).unwrap().execs.push(x.clone()),
                CompItem::Override(list) => self
                    .raw_components
                    .get_mut(&c.name)
                    .unwrap()
                    .overrides
                    .extend(list.clone()),
                CompItem::Symbol { name, body } => self
                    .raw_components
                    .get_mut(&c.name)
                    .unwrap()
                    .symbols
                    .push((name.clone(), body.clone())),
                CompItem::Constraint(cn) => self.raw_components.get_mut(&c.name).unwrap().constraints.push(cn.clone()),
                CompItem::Enum(en) => {
                    let entry = self.model.enums.entry(en.name.clone()).or_default();
                    for v in &en.variants {
                        if !entry.contains(v) {
                            entry.push(v.clone());
                        }
                    }
                }
                CompItem::Struct(t) => self.collect_type(t, t.name.clone()),
                CompItem::StaticConst { name, value } => {
                    if let Some(v) = value.eval_const(&|n| self.model.constants.get(n).copied()) {
                        self.model.constants.insert(name.clone(), v);
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_action(&mut self, a: &ActionDecl, owner: Option<String>) {
        let e = self
            .raw_actions
            .entry(a.name.clone())
            .or_insert_with(|| RawAction {
                decl: a.clone(),
                owner: owner.clone(),
            });
        // A re-declaration (e.g. `extend component` adding an action) merges.
        if e.decl.name == a.name {
            if a.activity.is_some() {
                e.decl.activity = a.activity.clone();
            }
            e.decl.members.extend(a.members.iter().cloned());
            e.decl.constraints.extend(a.constraints.iter().cloned());
            e.decl.execs.extend(a.execs.iter().cloned());
            if a.base.is_some() {
                e.decl.base = a.base.clone();
            }
        }
    }

    fn finish(&mut self) {
        // Enums are already populated; classify types and components.
        self.model.types = self.build_types();
        self.model.components = self.build_components();
        self.model.actions = self.build_actions();
        self.model.top = self.pick_top();
        self.model.addr_regions = self.build_addr_regions();
        let mut dupes: Vec<String> = self
            .leaf_counts
            .iter()
            .filter(|(_, n)| **n > 1)
            .map(|(k, _)| k.clone())
            .collect();
        dupes.sort();
        self.model.duplicates = dupes;
    }

    /// Find address regions from `exec` assignments to `<region>.addr` and
    /// `<region>.size`.
    fn build_addr_regions(&self) -> Vec<AddrRegion> {
        let mut acc: Vec<(String, Option<i64>, Option<i64>)> = Vec::new();
        let mut consider = |path: &Path, rhs: &Expr| {
            let segs = &path.segments;
            if segs.len() < 2 {
                return;
            }
            let last = segs.last().unwrap().clone();
            let key = segs[..segs.len() - 1].join(".");
            let Some(v) = rhs.eval_const(&|n| self.model.constants.get(n).copied()) else {
                return;
            };
            let entry = match acc.iter_mut().find(|(k, _, _)| *k == key) {
                Some(e) => e,
                None => {
                    acc.push((key.clone(), None, None));
                    acc.last_mut().unwrap()
                }
            };
            match last.as_str() {
                "addr" | "base" => entry.1 = Some(v),
                "size" => entry.2 = Some(v),
                _ => {}
            }
        };
        for c in self.raw_components.values() {
            for e in &c.execs {
                for st in &e.stmts {
                    if let ExecStmt::Assign(path, rhs) = st {
                        consider(path, rhs);
                    }
                }
            }
        }
        acc.into_iter()
            .filter_map(|(name, base, size)| match (base, size) {
                (Some(base), Some(size)) => Some(AddrRegion { name, base, size }),
                (Some(base), None) => Some(AddrRegion {
                    name,
                    base,
                    size: 0,
                }),
                _ => None,
            })
            .collect()
    }

    fn pick_top(&self) -> Option<String> {
        if self.model.components.contains_key("pss_top") {
            return Some("pss_top".to_string());
        }
        // Otherwise the component with the most sub-components.
        self.model
            .components
            .values()
            .max_by_key(|c| c.subcomponents.len())
            .map(|c| c.name.clone())
    }

    /// Canonical key for a type reference: exact key, else leaf, else a unique
    /// qualified match, else the leaf.
    fn resolve_type_key(&self, name: &str) -> String {
        if self.raw_types.contains_key(name) {
            return name.to_string();
        }
        let leaf = name.rsplit("::").next().unwrap_or(name);
        if self.raw_types.contains_key(leaf) {
            return leaf.to_string();
        }
        let suffix = format!("::{}", leaf);
        let matches: Vec<&String> = self.raw_types.keys().filter(|k| k.ends_with(&suffix)).collect();
        if matches.len() == 1 {
            return matches[0].clone();
        }
        leaf.to_string()
    }

    fn category_of(&self, type_name: &str) -> TypeCategory {
        let key = self.resolve_type_key(type_name);
        let leaf = key.rsplit("::").next().unwrap_or(&key).to_string();
        let leaf = leaf.as_str();
        if matches!(
            leaf,
            "int" | "bit" | "bool" | "byte" | "shortint" | "longint" | "integer" | "uint" | "float32" | "float64"
        ) {
            return TypeCategory::Scalar;
        }
        if let Some(vars) = self.model.enums.get(leaf) {
            return TypeCategory::Enum(vars.clone());
        }
        if self.raw_types.contains_key(&key) {
            return TypeCategory::Struct(key.clone());
        }
        if self.component_names.iter().any(|n| n == leaf) {
            return TypeCategory::Component(leaf.to_string());
        }
        if leaf.ends_with("_h") || leaf.contains("handle") || leaf.ends_with("_t") {
            return TypeCategory::Handle;
        }
        TypeCategory::Unknown
    }

    fn lower_member(&self, m: &Member) -> Field {
        let type_name = m.ty.name();
        let bits = m
            .ty
            .width
            .as_ref()
            .and_then(|w| w.eval_const(&|n| self.model.constants.get(n).copied()));
        let mut array = m
            .array
            .as_ref()
            .and_then(|a| a.eval_const(&|n| self.model.constants.get(n).copied()));
        // `array<T, N>` / `list<T>`: keep the element type, and take N from the
        // constant template argument if present.
        let mut elem_type = None;
        let mut elem_bits = None;
        let leaf = m.ty.name();
        if matches!(leaf.as_str(), "array" | "list") {
            if let Some(t) = m.ty.template_args.first() {
                elem_type = Some(t.name());
                elem_bits = t
                    .width
                    .as_ref()
                    .and_then(|w| w.eval_const(&|n| self.model.constants.get(n).copied()));
            }
            if array.is_none() {
                array = m
                    .ty
                    .const_args
                    .first()
                    .and_then(|c| c.eval_const(&|n| self.model.constants.get(n).copied()));
            }
        }
        Field {
            name: m.name.clone(),
            kind: match m.kind {
                MemberKind::Plain => FieldKind::Plain,
                MemberKind::Rand => FieldKind::Rand,
                MemberKind::Input => FieldKind::Input,
                MemberKind::Output => FieldKind::Output,
                MemberKind::Lock => FieldKind::Lock,
                MemberKind::Share => FieldKind::Share,
            },
            type_name: type_name.clone(),
            category: self.category_of(&type_name),
            bits,
            inline_constraint: m.inline_constraint.clone(),
            init: m.init.clone(),
            elem_type,
            elem_bits,
            array,
        }
    }

    fn lower_constraints(&self, cs: &[ConstraintBlock]) -> Vec<Constraint> {
        let mut out = Vec::new();
        for c in cs {
            // A constraint block may itself contain `default ...` /
            // `default disable ...` items; they become their own constraints.
            let mut inner = Vec::new();
            let rest = self.split_default_items(&c.expr, &mut inner);
            if let Some(e) = rest {
                out.push(Constraint {
                    name: c.name.clone(),
                    expr: e,
                    is_default: c.is_default,
                    is_disable: c.is_disable,
                });
            }
            out.extend(inner);
        }
        out
    }

    /// Pull `default <attr> == k` / `default disable <attr>` markers out of a
    /// conjunction, returning the remaining expression (if any).
    fn split_default_items(&self, e: &Expr, out: &mut Vec<Constraint>) -> Option<Expr> {
        match e {
            Expr::Binary(op, l, r) if op == "&&" => {
                let a = self.split_default_items(l, out);
                let b = self.split_default_items(r, out);
                match (a, b) {
                    (Some(a), Some(b)) => Some(Expr::Binary("&&".into(), Box::new(a), Box::new(b))),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                }
            }
            Expr::Call(f, args) if f == "default" || f == "default_disable" => {
                if let Some(t) = args.first() {
                    out.push(Constraint {
                        name: None,
                        expr: t.clone(),
                        is_default: true,
                        is_disable: f == "default_disable",
                    });
                }
                None
            }
            _ => Some(e.clone()),
        }
    }

    fn build_types(&self) -> HashMap<String, DataType> {
        let mut out = HashMap::new();
        for (name, rt) in &self.raw_types {
            // Inherit fields/constraints from the base type.
            let mut fields: Vec<Field> = Vec::new();
            let mut constraints: Vec<Constraint> = Vec::new();
            if let Some(base) = &rt.base {
                if let Some(b) = self.raw_types.get(base) {
                    for m in &b.members {
                        let f = self.lower_member(m);
                        // An inline range on a field (`rand int in [0..7] x;`)
                        // is a constraint on the type, like any other.
                        if let Some(inline) = &f.inline_constraint {
                            constraints.push(Constraint {
                                name: None,
                                expr: inline.clone(),
                                is_default: false,
                                is_disable: false,
                            });
                        }
                        fields.push(f);
                    }
                    constraints.extend(self.lower_constraints(&b.constraints));
                }
            }
            for m in &rt.members {
                let f = self.lower_member(m);
                if let Some(inline) = &f.inline_constraint {
                    constraints.push(Constraint {
                        name: None,
                        expr: inline.clone(),
                        is_default: false,
                        is_disable: false,
                    });
                }
                if let Some(existing) = fields.iter_mut().find(|x| x.name == f.name) {
                    *existing = f;
                } else {
                    fields.push(f);
                }
            }
            constraints.extend(self.lower_constraints(&rt.constraints));
            out.insert(
                name.clone(),
                DataType {
                    name: rt.name.clone(),
                    keyword: rt.keyword.clone(),
                    base: rt.base.clone(),
                    fields,
                    constraints,
                },
            );
        }
        out
    }

    fn build_components(&self) -> HashMap<String, Component> {
        let mut out = HashMap::new();
        for (name, rc) in &self.raw_components {
            let mut subcomponents = Vec::new();
            let mut fields = Vec::new();
            let mut members = Vec::new();
            if let Some(base) = &rc.base {
                if let Some(b) = self.raw_components.get(base) {
                    members.extend(b.members.iter().cloned());
                }
            }
            members.extend(rc.members.iter().cloned());
            for m in &members {
                let leaf = self.model.resolve(&m.ty.name());
                if self.component_names.iter().any(|n| n == &leaf) {
                    subcomponents.push((m.name.clone(), leaf));
                } else {
                    fields.push(self.lower_member(m));
                }
            }
            let actions: Vec<String> = rc.actions.iter().map(|a| a.name.clone()).collect();
            let pools = rc
                .pools
                .iter()
                .map(|p| Pool {
                    name: p.name.clone(),
                    type_name: p.ty.name(),
                    count: p
                        .count
                        .as_ref()
                        .and_then(|c| c.eval_const(&|n| self.model.constants.get(n).copied())),
                })
                .collect();
            let symbols: HashMap<String, crate::ast::Activity> = rc.symbols.iter().cloned().collect();
            let overrides = rc.overrides.clone();
            let binds = rc
                .binds
                .iter()
                .map(|b| Bind {
                    pool: b.pool.leaf(),
                    wildcard: b.wildcard,
                    target: b.target.as_ref().map(|t| t.leaf()),
                })
                .collect();
            out.insert(
                name.clone(),
                Component {
                    name: rc.name.clone(),
                    base: rc.base.clone(),
                    reg_value_type: rc.reg_value_type.clone(),
                    reg_access: rc.reg_access.clone(),
                    reg_size: rc.reg_size,
                    subcomponents,
                    fields,
                    actions,
                    pools,
                    binds,
                    symbols,
                    overrides,
                    execs: rc.execs.clone(),
                    constraints: self.lower_constraints(&rc.constraints),
                },
            );
        }
        out
    }

    fn build_actions(&self) -> HashMap<String, Action> {
        let mut out: HashMap<String, Action> = HashMap::new();
        // First pass: components' actions.
        for rc in self.raw_components.values() {
            for a in &rc.actions {
                let act = self.lower_action(a, Some(rc.name.clone()));
                out.insert(a.name.clone(), act);
            }
        }
        // Top-level actions (owner None). An `extend action X` whose target is
        // already declared merges into it.
        for ra in self.raw_actions.values() {
            let extra = self.lower_action(&ra.decl, ra.owner.clone());
            match out.get_mut(&ra.decl.name) {
                Some(existing) => {
                    for f in extra.fields {
                        if !existing.fields.iter().any(|x| x.name == f.name) {
                            existing.fields.push(f);
                        }
                    }
                    existing.constraints.extend(extra.constraints);
                    if extra.activity.is_some() {
                        existing.activity = extra.activity;
                    }
                }
                None => {
                    out.insert(ra.decl.name.clone(), extra);
                }
            }
        }
        // Resolve inherited fields/constraints from base actions.
        let names: Vec<String> = out.keys().cloned().collect();
        for name in &names {
            let mut inherited_fields: Vec<Field> = Vec::new();
            let mut inherited_constraints: Vec<Constraint> = Vec::new();
            let mut base = out.get(name).and_then(|a| a.base.clone());
            let mut depth = 0;
            while let Some(b) = base {
                depth += 1;
                if depth > 16 {
                    break;
                }
                if let Some(ba) = out.get(&b) {
                    inherited_fields.extend(ba.fields.iter().cloned());
                    inherited_constraints.extend(ba.constraints.iter().cloned());
                    base = ba.base.clone();
                } else {
                    break;
                }
            }
            if let Some(a) = out.get_mut(name) {
                let mut fields = inherited_fields;
                for f in a.fields.drain(..) {
                    if let Some(existing) = fields.iter_mut().find(|x| x.name == f.name) {
                        *existing = f;
                    } else {
                        fields.push(f);
                    }
                }
                a.fields = fields;
                let mut cs = inherited_constraints;
                cs.extend(a.constraints.drain(..));
                a.constraints = cs;
            }
        }
        out
    }

    fn lower_action(&self, a: &ActionDecl, owner: Option<String>) -> Action {
        let mut fields: Vec<Field> = Vec::new();
        let mut constraints = self.lower_constraints(&a.constraints);
        for m in &a.members {
            let f = self.lower_member(m);
            if let Some(inline) = &f.inline_constraint {
                constraints.push(Constraint {
                    name: None,
                    expr: inline.clone(),
                    is_default: false,
                    is_disable: false,
                });
            }
            fields.push(f);
        }
        Action {
            name: a.name.clone(),
            base: a.base.as_ref().map(|b| b.name()),
            is_abstract: a.is_abstract,
            fields,
            constraints,
            execs: a.execs.clone(),
            activity: a.activity.clone(),
            owner,
        }
    }
}

/// Build a declaration key: qualified when inside a package, else the leaf.
fn qualify(pkg: Option<&str>, leaf: &str) -> String {
    match pkg {
        Some(p) if !p.is_empty() => format!("{}::{}", p, leaf),
        _ => leaf.to_string(),
    }
}
