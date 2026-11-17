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

    /// The width and signedness of this field's *declared* type — the one
    /// thing a renderer needs and cannot get from the value itself. A solved
    /// value is carried in an `i64`, so `0xFFFF_FFFF_FFFF_FFFF` and `-1` are
    /// the same 64 bits: only the declaration says which of the two to print.
    pub fn signature(&self, model: &Model) -> FieldSig {
        FieldSig::of(model, &self.type_name, self.bits)
    }
}

/// Width and signedness of a declared type, carried from the declaration site
/// to the backends so a value is printed as the type it was declared with
/// (PSS 6.1 / 12.2) rather than as whatever the `i64` carrier looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSig {
    /// The declared width in bits, when the type declares one.
    pub bits: Option<i64>,
    /// True when the declared type is unsigned: `bit[N]`, `bool`, `byte`,
    /// `uint` and their aliases. `int`, `shortint`, `longint` and enums are
    /// signed.
    pub unsigned: bool,
}

impl FieldSig {
    /// A signature with no known width, rendered signed — what a value whose
    /// declaration could not be resolved falls back to.
    pub const SIGNED: FieldSig = FieldSig {
        bits: None,
        unsigned: false,
    };

    /// Is this type unsigned at its declared width?
    pub fn is_unsigned(&self) -> bool {
        self.unsigned
    }

    /// Print a solved value the way its declared type reads it: an unsigned
    /// field is the `u64` bit pattern, a signed one is the `i64` it already is.
    pub fn render(&self, v: i64) -> String {
        if self.unsigned {
            (v as u64).to_string()
        } else {
            v.to_string()
        }
    }

    /// Resolve a declared type name to its width and signedness, following a
    /// `typedef` alias to the type it names.
    pub fn of(model: &Model, type_name: &str, bits: Option<i64>) -> FieldSig {
        Self::of_at(model, type_name, bits, 0)
    }

    fn of_at(model: &Model, type_name: &str, bits: Option<i64>, depth: usize) -> FieldSig {
        // A type that (illegally) aliases itself must not recurse forever.
        if depth > 8 {
            return FieldSig { bits, unsigned: false };
        }
        let leaf = type_name.rsplit("::").next().unwrap_or(type_name);
        // An explicit width on the declaration itself (`bit[64]`) keeps the
        // leaf `bit`, so the keyword decides the signedness.
        if let Some(u) = builtin_unsigned(leaf) {
            let bits = bits.or(match leaf {
                "bit" | "bool" => Some(1),
                _ => None,
            });
            return FieldSig { bits, unsigned: u };
        }
        // A `typedef` alias: the width may sit on the alias (`typedef bit[64]
        // u64_t;`), so take it from there when the field has none of its own.
        if let Some(tr) = model
            .typedefs
            .get(type_name)
            .or_else(|| model.typedefs.get(leaf))
        {
            let w = bits.or_else(|| {
                tr.width
                    .as_ref()
                    .and_then(|w| w.eval_const(&|n| model.constants.get(n).copied()))
            });
            return Self::of_at(model, &mangle_type(tr), w, depth + 1);
        }
        // A named type: its keyword carries the signedness (`bit` for an enum's
        // base is still an integer, but an enum's own values are signed).
        if let Some(dt) = model.types.get(&model.resolve(type_name)) {
            if let Some(u) = builtin_unsigned(&dt.keyword) {
                return FieldSig { bits, unsigned: u };
            }
        }
        FieldSig { bits, unsigned: false }
    }
}

/// The signedness of a built-in type keyword (PSS 6.1): `bit`, `bool`, `byte`
/// and `uint` are unsigned; `shortint`, `int`, `longint` and `integer` are not.
fn builtin_unsigned(leaf: &str) -> Option<bool> {
    match leaf {
        "bit" | "bool" | "byte" | "uint" | "uint8" | "uint16" | "uint32" | "uint64"
        | "unsigned" => Some(true),
        "int" | "integer" | "shortint" | "longint" | "int8" | "int16" | "int32" | "int64"
        | "signed" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct Constraint {
    pub name: Option<String>,
    pub expr: Expr,
    pub is_default: bool,
    pub is_disable: bool,
    /// `constraint soft ...` (PSS 16.1.4): honoured when it can be, dropped
    /// when it conflicts with a hard constraint.
    pub is_soft: bool,
    /// `constraint forall (it : T) { ... }` (PSS 16.1.8). The body is kept in
    /// `expr`; without the binding the solver cannot tell which attributes it
    /// means, so the flag and the binding travel with it.
    pub is_forall: bool,
    /// The `(variable, type)` a `forall` binds, as declared. The type is
    /// mangled the way a field's is (`pair_s<bit[8],4>`), so it compares
    /// directly against `Field::type_name`.
    pub forall_binding: Option<(String, String)>,
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
    /// Covergroups instantiated by this action (`cg inst1 (x);`), with the
    /// instance's arguments substituted for the covergroup's formals, so the
    /// instance samples (PSS 18.2, Syntax 64). Each carries the instance name,
    /// so it is reported as `inst1.<coverpoint>`.
    pub covergroup_instances: Vec<Covergroup>,
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
    /// `import pkg::*;` / `import pkg::name;` in this component's body, as
    /// dotted paths (`fpa::*`), which decide what an unqualified name means
    /// (PSS 22.2).
    pub imports: Vec<String>,
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
    /// Trait attribute values for this region, e.g. `core -> 1` from
    /// `reg.trait.core = 1;` in an `exec init_up` block (PSS 24.7.3.2).
    pub traits: std::collections::BTreeMap<String, i64>,
    /// The region's `tag` string (PSS 24.7.3.1), returned by `get_tag()`.
    pub tag: Option<String>,
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
    /// `function` / `task` declarations, keyed by qualified name
    /// (`fpa::value`) when the declaring scope is known, else by leaf name
    /// (PSS 22.1). A qualified key is what makes an `import` able to choose
    /// between two functions of the same name.
    pub functions: std::collections::HashMap<String, crate::ast::Function>,
    /// `import pkg::*;` / `import pkg::name;` at file scope.
    pub imports: Vec<String>,
    /// `typedef` aliases, so `sizeof_s<alias>` resolves (PSS 12.2, 24.10.2).
    pub typedefs: std::collections::HashMap<String, crate::ast::TypeRef>,
    /// Declared enum variant values, `Enum::VARIANT` -> value (PSS 12.4).
    /// Variants without an explicit value take the previous one + 1.
    pub enum_values: std::collections::HashMap<String, i64>,
    /// `export target c::A1;` declarations, as `<pkg>::<leaf>` (PSS 22.4).
    /// An action of a component is nameable from outside its package as
    /// `pkg::A1` only when the package exports it, so this is what decides
    /// whether a cross-package `do pkg::A1` resolves.
    pub exports: Vec<String>,
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

    /// The declared width and signedness of every numeric leaf of `action`,
    /// keyed exactly as the solver names it (`sv.f`, `arr[0]`), so a backend can
    /// print a solved value as its declared type instead of guessing from the
    /// bits it happens to hold. Keys that are absent are not numeric leaves.
    pub fn field_signatures(&self, action: &str) -> HashMap<String, FieldSig> {
        let mut out = HashMap::new();
        if let Some(a) = self.actions.get(&self.resolve(action)) {
            let mut visited = Vec::new();
            self.collect_signatures(&a.fields, "", 0, &mut visited, &mut out);
        }
        out
    }

    fn collect_signatures(
        &self,
        fields: &[Field],
        prefix: &str,
        depth: usize,
        visited: &mut Vec<String>,
        out: &mut HashMap<String, FieldSig>,
    ) {
        // Mirrors the solver's own expansion (see `expand_field`), so a key
        // here is the key the solved value carries.
        if depth > 6 {
            return;
        }
        for f in fields {
            let path = if prefix.is_empty() {
                f.name.clone()
            } else {
                format!("{}.{}", prefix, f.name)
            };
            // A fixed-size array is one variable per element (PSS 8.4).
            if let Some(n) = f.array {
                let elem_ty = f.elem_type.clone().unwrap_or_else(|| f.type_name.clone());
                let sig = FieldSig::of(self, &elem_ty, f.elem_bits.or(f.bits));
                let elem = self.types.get(&self.resolve(&elem_ty));
                for i in 0..n.clamp(0, 4096) {
                    let epath = format!("{}[{}]", path, i);
                    match elem {
                        Some(t) => {
                            if !visited.contains(&elem_ty) {
                                visited.push(elem_ty.clone());
                                self.collect_signatures(&t.fields, &epath, depth + 1, visited, out);
                                visited.pop();
                            }
                        }
                        None => {
                            out.insert(epath, sig);
                        }
                    }
                }
                continue;
            }
            match &f.category {
                // A leaf that carries a value of its own — including one whose
                // type the model never declared (`uint64`, a methodology alias),
                // which is still printed by the signedness its name gives.
                TypeCategory::Component(_) | TypeCategory::Handle => {}
                TypeCategory::Struct(tname) => {
                    if visited.contains(tname) {
                        continue;
                    }
                    if let Some(t) = self.types.get(tname) {
                        visited.push(tname.clone());
                        self.collect_signatures(&t.fields, &path, depth + 1, visited, out);
                        visited.pop();
                    }
                }
                // Scalar, enum, and an unresolved name: one value, whose
                // declared type says how to print it.
                _ => {
                    out.insert(path, f.signature(self));
                }
            }
        }
    }

    pub fn lower(file: &SourceFile) -> PssResult<Model> {
        let mut lw = Lowerer::default();
        // Covergroups are indexed before lowering so an action's instance
        // (`cg inst1 (x);`) can be bound while its members are lowered.
        for cg in &file.covergroups {
            lw.covergroup_decls.insert(cg.name.clone(), cg.clone());
        }
        lw.collect(file.items.iter(), None);
        lw.finish();
        lw.model.typedefs = file.typedefs.clone();
        // Enum variant values: an explicit `= v`, otherwise previous + 1.
        // Enums nested in a package must be reached too — an enum declared
        // inside `package p { }` was skipped, so its variants fell back to
        // their positions.
        let mut all_enums: Vec<&crate::ast::EnumDecl> = Vec::new();
        fn walk<'a>(items: &'a [crate::ast::Item], out: &mut Vec<&'a crate::ast::EnumDecl>) {
            for it in items {
                match it {
                    crate::ast::Item::Enum(e) => out.push(e),
                    crate::ast::Item::Package(p) => walk(&p.items, out),
                    _ => {}
                }
            }
        }
        walk(&file.items, &mut all_enums);
        for e in all_enums {
            {
                let mut next: i64 = 0;
                for (i, v) in e.variants.iter().enumerate() {
                    let declared = e
                        .values
                        .get(i)
                        .and_then(|o| o.as_ref())
                        .and_then(|x| x.eval_const(&|_| None));
                    let val = declared.unwrap_or(next);
                    next = val + 1;
                    lw.model.enum_values.insert(v.clone(), val);
                    lw.model
                        .enum_values
                        .insert(format!("{}::{}", e.name, v), val);
                }
            }
        }
        lw.model.covergroups = file.covergroups.clone();
        lw.model.monitors = file.monitors.clone();
        for f in &file.functions {
            // Qualified by the scope that declares it, so `import fpa::*` can
            // select fpa's `value` rather than whichever was declared last.
            let key = qualify(f.scope.as_deref(), &f.name);
            // A prototype (`function int f(int a);` — no body) only declares the
            // signature; a definition supplies the body and must win.
            // `or_insert` kept whichever came first, so the prototype written
            // above its definition silently shadowed it and every call returned
            // 0 (constructs 84 and 86).
            let take = match lw.model.functions.get(&key) {
                None => true,
                Some(prev) => prev.body.is_empty() && !f.body.is_empty(),
            };
            if take {
                lw.model.functions.insert(key, f.clone());
            }
        }
        // `export` gates cross-package visibility of an action (PSS 22.4).
        check_exported_actions(file, &lw.model)?;
        Ok(lw.model)
    }

    pub fn component(&self, name: &str) -> Option<&Component> {
        self.components.get(name)
    }

    /// Every `import` visible from a component: the file's own, then the
    /// component's, then those of the components above it.
    pub fn imports_for(&self, component: Option<&str>) -> Vec<String> {
        let mut out = self.imports.clone();
        let mut cur = component.map(|c| c.rsplit('.').next().unwrap_or(c).to_string());
        let mut guard = 0;
        while let Some(name) = cur {
            guard += 1;
            if guard > 8 {
                break;
            }
            match self.components.get(&name) {
                Some(c) => {
                    for i in &c.imports {
                        if !out.contains(i) {
                            out.push(i.clone());
                        }
                    }
                    cur = c
                        .base
                        .as_ref()
                        .map(|b| b.rsplit("::").next().unwrap_or(b).to_string());
                }
                None => break,
            }
        }
        out
    }

    /// Resolve a `function` / `task` call (PSS 22.2).
    ///
    /// A qualified name resolves directly. An unqualified one is resolved
    /// through the imports visible from `component`: `import fpa::*` and
    /// `import fpa::value` both select `fpa::value`. With no import that names
    /// it, a name declared in more than one scope is ambiguous and does not
    /// resolve — that is the whole point of an import. A name declared in one
    /// scope only still resolves, so a single-package model keeps working.
    pub fn resolve_function(
        &self,
        component: Option<&str>,
        name: &str,
    ) -> (Option<&crate::ast::Function>, Option<String>) {
        let leaf = name.rsplit("::").next().unwrap_or(name);
        if name.contains("::") {
            if let Some(f) = self.functions.get(name) {
                return (Some(f), None);
            }
        }
        let imports = self.imports_for(component);
        // A scope the imports name explicitly.
        for imp in &imports {
            let (scope, item) = match imp.rsplit_once("::") {
                Some((s, i)) => (s, i),
                None => continue,
            };
            if item == "*" || item == leaf {
                if let Some(f) = self.functions.get(&format!("{}::{}", scope, leaf)) {
                    return (Some(f), None);
                }
            }
        }
        // The component's own declaration wins over a package's.
        if let Some(comp) = component {
            let own = comp.rsplit('.').next().unwrap_or(comp);
            if let Some(f) = self.functions.get(&format!("{}::{}", own, leaf)) {
                return (Some(f), None);
            }
        }
        let mut hits: Vec<&String> = self
            .functions
            .keys()
            .filter(|k| k.as_str() == leaf || k.ends_with(&format!("::{}", leaf)))
            .collect();
        hits.sort();
        match hits.len() {
            0 => (None, None),
            1 => (self.functions.get(hits[0]), None),
            _ => {
                let names: Vec<String> = hits.iter().map(|k| (*k).clone()).collect();
                (
                    None,
                    Some(format!(
                        "`{}` is declared in {} scopes ({}) and no import selects one (PSS 22.2)",
                        leaf,
                        hits.len(),
                        names.join(", ")
                    )),
                )
            }
        }
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
    /// Declared covergroups, by name, so an action's instance of one can be
    /// bound (PSS 18.2).
    covergroup_decls: HashMap<String, Covergroup>,
}

/// The name a field's type is keyed by. A templated use carries its arguments,
/// so `pair_s<bit[16], 4>` is a distinct type from `pair_s` (PSS 12.6).
fn mangle_type(ty: &crate::ast::TypeRef) -> String {
    let leaf = ty.path.leaf();
    if ty.template_args.is_empty() && ty.const_args.is_empty() {
        return leaf;
    }
    let mut parts: Vec<String> = Vec::new();
    for a in &ty.template_args {
        // An argument carries its width: `bit[16]` is not `bit`.
        let mut text = mangle_type(a);
        if let Some(w) = &a.width {
            if let Some(n) = w.eval_const(&|_| None) {
                text = format!("{}[{}]", text, n);
            }
        }
        parts.push(text);
    }
    for c in &ty.const_args {
        parts.push(crate::solver::render_expr(c));
    }
    if parts.is_empty() {
        leaf
    } else {
        format!("{}<{}>", leaf, parts.join(","))
    }
}

/// The width of a rendered type argument such as `bit[16]`.
fn type_arg_bits(a: &str) -> Option<i64> {
    let t = a.trim();
    if let Some(rest) = t.strip_prefix("bit[") {
        return rest.trim_end_matches(']').trim().parse::<i64>().ok();
    }
    match t {
        "bit" => Some(1),
        "int" => Some(32),
        _ => None,
    }
}

/// The template argument bound to `e` when `e` is a bare parameter name, so a
/// width (`bit[W]`) or an extent (`arr[N]`) can be taken from it.
fn param_arg<'a>(e: &crate::ast::Expr, params: &[String], args: &'a [String]) -> Option<&'a String> {
    match e {
        crate::ast::Expr::Path(p) => {
            let name = p.segments.join(".");
            let pos = params.iter().position(|x| *x == name)?;
            args.get(pos)
        }
        _ => None,
    }
}

/// Evaluate a `bit[N]` width with the template's value parameters substituted.
fn substituted_width(
    w: &crate::ast::Expr,
    params: &[String],
    args: &[String],
) -> Option<i64> {
    let subst = |e: &crate::ast::Expr| -> Option<i64> {
        match e {
            crate::ast::Expr::Path(p) => {
                let name = p.segments.join(".");
                let pos = params.iter().position(|x| *x == name)?;
                args.get(pos)?.trim().parse::<i64>().ok()
            }
            other => other.eval_const(&|_| None),
        }
    };
    subst(w)
}

#[derive(Default)]
struct RawType {
    name: String,
    keyword: String,
    base: Option<String>,
    members: Vec<Member>,
    constraints: Vec<ConstraintBlock>,
    /// Template parameter names, in declaration order (PSS 12.6).
    template_params: Vec<String>,
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
    imports: Vec<String>,
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
                Item::Import(p) => {
                    // An `export` declaration reaches the IR as an import-shaped
                    // marker, `__export__::<pkg>::<target>...` (see
                    // `Parser::parse_export`). It is not an import.
                    if p.segments.first().map(|s| s == "__export__").unwrap_or(false) {
                        if p.segments.len() >= 3 {
                            let key = format!(
                                "{}::{}",
                                p.segments[1],
                                p.segments.last().unwrap()
                            );
                            if !self.model.exports.contains(&key) {
                                self.model.exports.push(key);
                            }
                        }
                        continue;
                    }
                    let path = p.segments.join("::");
                    if !self.model.imports.contains(&path) {
                        self.model.imports.push(path);
                    }
                }
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
            template_params: t.template_params.clone(),
            ..Default::default()
        });
        if !t.template_params.is_empty() {
            e.template_params = t.template_params.clone();
        }
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
                CompItem::Import(p) => {
                    let path = p.segments.join("::");
                    let rc = self.raw_components.get_mut(&c.name).unwrap();
                    if !rc.imports.contains(&path) {
                        rc.imports.push(path);
                    }
                }
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
        // Instantiate every templated type the model uses, now that the base
        // types exist (PSS 12.6).
        let mut with_inst = self.model.types.clone();
        self.instantiate_types(&mut with_inst);
        self.model.types = with_inst;
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
        // Trait attribute values, keyed by region then attribute.
        let mut trait_vals: std::collections::BTreeMap<String, std::collections::BTreeMap<String, i64>> =
            std::collections::BTreeMap::new();
        // Region tag strings: `reg.tag = "name";`.
        let mut tag_vals: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let mut consider = |path: &Path, rhs: &Expr| {
            let segs = &path.segments;
            if segs.len() < 2 {
                return;
            }
            let last = segs.last().unwrap().clone();
            // `reg.tag = "name"` names the region (24.7.3.1).
            if last == "tag" {
                if let Expr::Str(t) = rhs {
                    tag_vals.insert(segs[..segs.len() - 1].join("."), t.clone());
                    return;
                }
            }
            // `reg.trait.<attr> = v` describes the region's traits (24.7.3.2).
            if segs.len() >= 3 && segs[segs.len() - 2] == "trait" {
                let Some(v) = rhs.eval_const(&|n| self.model.constants.get(n).copied()) else {
                    return;
                };
                let region = segs[..segs.len() - 2].join(".");
                trait_vals.entry(region).or_default().insert(last, v);
                return;
            }
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
            .filter_map(|(name, base, size)| {
                let traits = trait_vals.get(&name).cloned().unwrap_or_default();
                let tag = tag_vals.get(&name).cloned();
                match (base, size) {
                    (Some(base), Some(size)) => Some(AddrRegion {
                        name,
                        base,
                        size,
                        traits,
                        tag,
                    }),
                    (Some(base), None) => Some(AddrRegion {
                        name,
                        base,
                        size: 0,
                        traits,
                        tag,
                    }),
                    _ => None,
                }
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
        // A template instantiation (`pair_s<bit[8],4>`) is a type of its own,
        // built by `instantiate_types` under its mangled name. Resolving it as
        // written fails — no raw type is called that — so it used to land in
        // `Unknown`, and every member of the instance stayed out of the solve:
        // `constraint small.f == 200;` bound nothing and the counter still read
        // `2/2 satisfied` (audit row 25). The base is what decides the category;
        // the mangled name is the key the instantiated type is stored under.
        if let Some((base, _)) = type_name.split_once('<') {
            let base_key = self.resolve_type_key(base);
            if self.raw_types.contains_key(&base_key) {
                return TypeCategory::Struct(type_name.to_string());
            }
        }
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
        let type_name = mangle_type(&m.ty);
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
                    is_soft: c.is_soft,
                    is_forall: c.is_forall,
                    forall_binding: c
                        .forall_binding
                        .as_ref()
                        .map(|(v, ty)| (v.clone(), mangle_type(ty))),
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
                        is_soft: false,
                        is_forall: false,
                        forall_binding: None,
                    });
                }
                None
            }
            _ => Some(e.clone()),
        }
    }

    /// Build the instantiated form of every templated type the model uses
    /// (PSS 12.6). A type parameter substitutes the field's type; a value
    /// parameter substitutes into a `bit[N]` width.
    fn instantiate_types(&self, out: &mut HashMap<String, DataType>) {
        let mut used: Vec<String> = Vec::new();
        let mut note = |ty: &crate::ast::TypeRef| {
            let n = mangle_type(ty);
            if n.contains('<') && !used.contains(&n) {
                used.push(n);
            }
        };
        for rt in self.raw_types.values() {
            for m in &rt.members {
                note(&m.ty);
            }
        }
        for rc in self.raw_components.values() {
            for m in &rc.members {
                note(&m.ty);
            }
            for a in &rc.actions {
                for m in &a.members {
                    note(&m.ty);
                }
            }
        }
        for mangled in used {
            if out.contains_key(&mangled) {
                continue;
            }
            let (base_name, arg_text) = match mangled.split_once('<') {
                Some((b, a)) => (b.to_string(), a.trim_end_matches('>').to_string()),
                None => continue,
            };
            // `raw_types` is keyed by qualified name (`pkg::pair_s`), so fall
            // back to a leaf match.
            let base = match self
                .raw_types
                .get(&base_name)
                .or_else(|| {
                    self.raw_types
                        .values()
                        .find(|rt| rt.name == base_name)
                })
            {
                Some(b) => b,
                None => continue,
            };
            // Split the argument text at top-level commas.
            let mut args: Vec<String> = Vec::new();
            let mut depth = 0i32;
            let mut cur = String::new();
            for ch in arg_text.chars() {
                match ch {
                    '<' | '[' | '(' => {
                        depth += 1;
                        cur.push(ch);
                    }
                    '>' | ']' | ')' => {
                        depth -= 1;
                        cur.push(ch);
                    }
                    ',' if depth == 0 => {
                        args.push(cur.trim().to_string());
                        cur.clear();
                    }
                    _ => cur.push(ch),
                }
            }
            if !cur.trim().is_empty() {
                args.push(cur.trim().to_string());
            }
            let params = base.template_params.clone();
            let mut fields: Vec<Field> = Vec::new();
            let mut constraints: Vec<Constraint> = Vec::new();
            for m in &base.members {
                let mut f = self.lower_member(m);
                // A type parameter used as the member's type.
                if let Some(pos) = params.iter().position(|p| *p == m.ty.name()) {
                    if let Some(a) = args.get(pos) {
                        f.type_name = a.clone();
                        f.bits = type_arg_bits(a);
                    }
                }
                // A value parameter used as a `bit[N]` width.
                if let Some(w) = &m.ty.width {
                    if let Some(bits) = substituted_width(w, &params, &args) {
                        f.bits = Some(bits);
                    }
                }
                // A *type* parameter used as the width: `rand bit[W] f;` with
                // `W = bit[8]`. `substituted_width` only parses a numeric
                // argument, so the width stayed unsubstituted and the member
                // fell back to a one-bit `bit` — `small.f` could then never
                // hold 200 (audit row 25).
                if f.bits.is_none() {
                    if let Some(w) = &m.ty.width {
                        if let Some(a) = param_arg(w, &params, &args) {
                            f.bits = type_arg_bits(a);
                        }
                    }
                }
                // A value parameter used as an array extent: `int arr[N];`.
                if f.array.is_none() {
                    let extent = m.array.as_ref().or_else(|| m.ty.const_args.first());
                    if let Some(e) = extent {
                        if let Some(a) = param_arg(e, &params, &args) {
                            f.array = a.trim().parse::<i64>().ok();
                        }
                    }
                }
                if let Some(inline) = &f.inline_constraint {
                    constraints.push(Constraint {
                        name: None,
                        expr: inline.clone(),
                        is_default: false,
                        is_disable: false,
                is_soft: false,
                is_forall: false,
                forall_binding: None,
                    });
                }
                fields.push(f);
            }
            out.insert(
                mangled.clone(),
                DataType {
                    name: mangled,
                    keyword: base.keyword.clone(),
                    base: base.base.clone(),
                    fields,
                    constraints,
                },
            );
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
                is_soft: false,
                is_forall: false,
                forall_binding: None,
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
                is_soft: false,
                is_forall: false,
                forall_binding: None,
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
                    imports: rc.imports.clone(),
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
                    // An `extend action` may add `exec` blocks, and they must
                    // run on the merged action (PSS 20.2): the extension's
                    // blocks are appended after the base's, so a `post_solve`
                    // in the extension sees what the base's left behind. This
                    // merge used to carry fields, constraints and activity but
                    // not execs, so the extension's blocks were silently lost.
                    existing.execs.extend(extra.execs);
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
        let mut covergroup_instances: Vec<Covergroup> = Vec::new();
        for m in &a.members {
            // `cg inst1 (x);` — an instance of a declared covergroup (PSS 18.2,
            // Syntax 64). It is not a data member: its coverpoints are bound to
            // the instance's arguments and sampled when the action runs.
            let ty_leaf = m.ty.path.leaf();
            if let Some(decl) = self.covergroup_decls.get(&ty_leaf) {
                let bound = bind_covergroup(decl, &m.name, &m.args);
                covergroup_instances.push(bound);
                continue;
            }
            let f = self.lower_member(m);
            if let Some(inline) = &f.inline_constraint {
                constraints.push(Constraint {
                    name: None,
                    expr: inline.clone(),
                    is_default: false,
                    is_disable: false,
                is_soft: false,
                is_forall: false,
                forall_binding: None,
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
            covergroup_instances,
        }
    }
}

/// Bind a covergroup declaration to one instance: the formals are replaced by
/// the instance's arguments, in declaration order (PSS 18.2), and the result is
/// named after the instance so coverage is reported as `inst1.<coverpoint>`.
fn bind_covergroup(decl: &Covergroup, instance: &str, args: &[Expr]) -> Covergroup {
    let map: HashMap<String, Expr> = decl
        .params
        .iter()
        .enumerate()
        .filter_map(|(i, p)| args.get(i).map(|a| (p.clone(), a.clone())))
        .collect();
    let items = decl
        .items
        .iter()
        .map(|it| match it {
            CoverItem::Coverpoint {
                label,
                expr,
                iff,
                bins,
            } => CoverItem::Coverpoint {
                label: label.clone(),
                expr: subst_expr(expr, &map),
                iff: iff.as_ref().map(|c| subst_expr(c, &map)),
                bins: bins
                    .iter()
                    .map(|(n, e)| (n.clone(), subst_expr(e, &map)))
                    .collect(),
            },
            CoverItem::Cross { label, refs, iff } => CoverItem::Cross {
                label: label.clone(),
                refs: refs.clone(),
                iff: iff.as_ref().map(|c| subst_expr(c, &map)),
            },
        })
        .collect();
    Covergroup {
        name: instance.to_string(),
        params: Vec::new(),
        items,
    }
}

/// Replace each formal parameter with the instance's argument.
fn subst_expr(e: &Expr, map: &HashMap<String, Expr>) -> Expr {
    if map.is_empty() {
        return e.clone();
    }
    match e {
        Expr::Path(p) if p.segments.len() == 1 => map
            .get(&p.segments[0])
            .cloned()
            .unwrap_or_else(|| e.clone()),
        Expr::Unary(op, a) => Expr::Unary(op.clone(), Box::new(subst_expr(a, map))),
        Expr::Binary(op, a, b) => Expr::Binary(
            op.clone(),
            Box::new(subst_expr(a, map)),
            Box::new(subst_expr(b, map)),
        ),
        Expr::Ternary(c, a, b) => Expr::Ternary(
            Box::new(subst_expr(c, map)),
            Box::new(subst_expr(a, map)),
            Box::new(subst_expr(b, map)),
        ),
        Expr::Call(name, args) => {
            Expr::Call(name.clone(), args.iter().map(|a| subst_expr(a, map)).collect())
        }
        Expr::Method(recv, name, args) => Expr::Method(
            Box::new(subst_expr(recv, map)),
            name.clone(),
            args.iter().map(|a| subst_expr(a, map)).collect(),
        ),
        Expr::Index(a, b) => Expr::Index(
            Box::new(subst_expr(a, map)),
            Box::new(subst_expr(b, map)),
        ),
        Expr::Slice(a, b, c) => Expr::Slice(
            Box::new(subst_expr(a, map)),
            Box::new(subst_expr(b, map)),
            Box::new(subst_expr(c, map)),
        ),
        Expr::InRange(a, lo, hi) => Expr::InRange(
            Box::new(subst_expr(a, map)),
            Box::new(subst_expr(lo, map)),
            Box::new(subst_expr(hi, map)),
        ),
        Expr::List(items) => Expr::List(items.iter().map(|i| subst_expr(i, map)).collect()),
        _ => e.clone(),
    }
}

/// Build a declaration key: qualified when inside a package, else the leaf.
fn qualify(pkg: Option<&str>, leaf: &str) -> String {
    match pkg {
        Some(p) if !p.is_empty() => format!("{}::{}", p, leaf),
        _ => leaf.to_string(),
    }
}

// ---------------------------------------------------------------------------
// `export` gating (PSS 22.4)
// ---------------------------------------------------------------------------

/// A cross-package action reference `pkg::A` names an action of another
/// component that `pkg` exports. Without an `export` declaration the name is
/// not visible from outside `pkg`, so it must be reported rather than resolved.
///
/// Only the two-segment form is gated: `pkg::comp::action` names a component
/// *type* declared in the package (a traversal, not an export), and a bare
/// `comp::action` names a component in the same scope. Both keep working.
fn check_exported_actions(file: &SourceFile, model: &Model) -> PssResult<()> {
    let mut acts: Vec<&crate::ast::ActionDecl> = Vec::new();
    fn walk_items<'a>(items: &'a [Item], out: &mut Vec<&'a crate::ast::ActionDecl>) {
        for it in items {
            match it {
                Item::Package(p) => walk_items(&p.items, out),
                Item::Action(a) => out.push(a),
                Item::Component(c) => {
                    for ci in &c.items {
                        if let CompItem::Action(a) = ci {
                            out.push(a);
                        }
                    }
                }
                // `Lowerer::collect` keeps the `then` branch of a `compile if`,
                // so only that branch is checked.
                Item::CompileIf { then_items, .. } => walk_items(then_items, out),
                _ => {}
            }
        }
    }
    walk_items(&file.items, &mut acts);
    for a in acts {
        if let Some(act) = &a.activity {
            check_activity(act, model)?;
        }
    }
    Ok(())
}

/// One `do` target: a two-segment `pkg::A` must be exported by `pkg`.
fn check_action_path(p: &crate::ast::Path, model: &Model) -> PssResult<()> {
    if p.segments.len() != 2 {
        return Ok(());
    }
    let (pkg, leaf) = (&p.segments[0], &p.segments[1]);
    if model.packages.contains(pkg)
        && !model.components.contains_key(pkg)
        && !model.exports.contains(&format!("{}::{}", pkg, leaf))
    {
        return Err(crate::error::PssError::plain(format!(
            "`{}` is not visible: `{}` is a package and does not export `{}` \
             (PSS 22.4 — an `export` declaration is required)",
            p, pkg, leaf
        )));
    }
    Ok(())
}

fn check_activity(act: &crate::ast::Activity, model: &Model) -> PssResult<()> {
    use crate::ast::{Activity, ActivityStmt};
    let Activity::Sequence(stmts) = act;
    for s in stmts {
        match s {
            ActivityStmt::Do(p) | ActivityStmt::DoConstrained { action: p, .. } => {
                check_action_path(p, model)?;
            }
            ActivityStmt::DoWith { action: p, body } => {
                check_action_path(p, model)?;
                if let Some(b) = body {
                    check_activity(b, model)?;
                }
            }
            ActivityStmt::Labeled { stmt, .. } => {
                let one = Activity::Sequence(vec![(**stmt).clone()]);
                check_activity(&one, model)?;
            }
            ActivityStmt::Parallel { branches, .. }
            | ActivityStmt::Schedule { branches, .. } => {
                for b in branches {
                    check_activity(b, model)?;
                }
            }
            ActivityStmt::Select(branches) => {
                for (_, b) in branches {
                    check_activity(b, model)?;
                }
            }
            ActivityStmt::Match { choices, .. } => {
                for (_, b) in choices {
                    check_activity(b, model)?;
                }
            }
            ActivityStmt::Repeat { body, .. }
            | ActivityStmt::Foreach { body, .. }
            | ActivityStmt::Atomic(body) => check_activity(body, model)?,
            ActivityStmt::Cover { body: Some(b), .. } => check_activity(b, model)?,
            _ => {}
        }
    }
    Ok(())
}
