//! Abstract syntax tree for the PSS v3.0 subset understood by Satya.
//!
//! The AST is intentionally faithful to the surface syntax (names, not resolved
//! symbols). Name resolution and semantic checks happen during IR lowering.

/// A dotted/scoped path such as `pssm_pkg::data_buf_h` or `data_buf_out.mem_seg`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    pub segments: Vec<String>,
}

impl Path {
    pub fn new(segments: Vec<String>) -> Self {
        Path { segments }
    }

    pub fn simple(name: impl Into<String>) -> Self {
        Path {
            segments: vec![name.into()],
        }
    }

    pub fn last(&self) -> &str {
        self.segments.last().map(|s| s.as_str()).unwrap_or("")
    }

    /// The name with any package qualifier removed (last segment).
    pub fn leaf(&self) -> String {
        self.last().to_string()
    }
}

impl std::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.segments.join("::"))
    }
}

/// A reference to a type, possibly templated and possibly bit-sized.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeRef {
    pub path: Path,
    pub template_args: Vec<TypeRef>,
    /// Constant template arguments, e.g. the `8` in `array<bit[32], 8>`.
    pub const_args: Vec<Expr>,
    /// For `bit[N]` / `int in [..]` style widths.
    pub width: Option<Box<Expr>>,
    /// True for `T<>` (template with default args elided).
    pub open_template: bool,
}

impl TypeRef {
    pub fn simple(path: Path) -> Self {
        TypeRef {
            path,
            template_args: Vec::new(),
            const_args: Vec::new(),
            width: None,
            open_template: false,
        }
    }

    pub fn name(&self) -> String {
        self.path.leaf()
    }
}

/// How a member participates in the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    /// A plain (non-random) attribute.
    Plain,
    /// A `rand` attribute.
    Rand,
    /// An `input` flow object.
    Input,
    /// An `output` flow object.
    Output,
    /// A `lock` resource claim.
    Lock,
    /// A `share` resource claim.
    Share,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub kind: MemberKind,
    pub ty: TypeRef,
    pub name: String,
    /// An inline constraint, e.g. `rand int in [1..1024] num_trans`.
    pub inline_constraint: Option<Expr>,
    /// An explicit array extent, e.g. `pool [NUM_PROC] ...`.
    pub array: Option<Expr>,
    pub init: Option<Expr>,
    /// `rand bit[..]` sized-array style, captured for completeness.
    pub is_static: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<String>,
    pub is_extend: bool,
    pub base: Option<Path>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    /// `struct`, `resource`, `buffer` or `state`.
    pub keyword: String,
    pub name: String,
    pub base: Option<TypeRef>,
    pub template_params: Vec<String>,
    pub members: Vec<Member>,
    pub constraints: Vec<ConstraintBlock>,
    pub is_extend: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintBlock {
    pub name: Option<String>,
    pub is_default: bool,
    /// `constraint default disable <attribute>;` (PSS 16.1.10.1b).
    pub is_disable: bool,
    pub is_forall: bool,
    pub forall_binding: Option<(String, TypeRef)>,
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecBlock {
    /// e.g. `init_up`, `init_down`, `pre_solve`, `post_solve`, `body`, `pre_body`.
    pub kind: String,
    /// Raw body text, kept verbatim for backends that pass it through.
    pub body: String,
    /// The body parsed into simple statements where possible.
    pub stmts: Vec<ExecStmt>,
}

/// A statement inside an `exec` body that Satya understands structurally.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecStmt {
    /// `lhs = rhs;`
    Assign(Path, Expr),
    /// `f(a, b);`
    Call(String, Vec<Expr>),
    /// `if (cond) { .. } else { .. }`
    If(Expr, Vec<ExecStmt>, Vec<ExecStmt>),
    /// `repeat (n) { .. }` (Syntax 94)
    Repeat { count: Expr, body: Vec<ExecStmt> },
    /// `repeat { .. } while (cond);` (Syntax 95)
    RepeatWhile { body: Vec<ExecStmt>, cond: Expr },
    /// `foreach (c[i]) { .. }` (Syntax 96)
    Foreach {
        var: String,
        iter: Expr,
        body: Vec<ExecStmt>,
    },
    /// `match (e) { ... }` (Syntax 98)
    Match {
        subject: Expr,
        arms: Vec<(Option<Expr>, Vec<ExecStmt>)>,
    },
    /// `break;` / `continue;` (Syntax 99)
    Break,
    Continue,
    /// `return [e];` (Syntax 93)
    Return(Option<Expr>),
    /// `randomize(x, y);` (Syntax 100)
    Randomize(Vec<Path>),
    /// `yield;` (Syntax 101) — no-op outside a coroutine.
    Yield,
    /// Anything else, kept as text.
    Raw(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionDecl {
    pub name: String,
    pub base: Option<TypeRef>,
    pub template_params: Vec<String>,
    pub is_abstract: bool,
    pub members: Vec<Member>,
    pub constraints: Vec<ConstraintBlock>,
    pub execs: Vec<ExecBlock>,
    pub activity: Option<Activity>,
    pub is_extend: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolDecl {
    pub ty: TypeRef,
    pub name: String,
    pub count: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BindDecl {
    pub pool: Path,
    /// `*` binds to every instance of the pool's element type.
    pub wildcard: bool,
    pub target: Option<Path>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDecl {
    pub name: String,
    pub base: Option<TypeRef>,
    pub template_params: Vec<String>,
    pub is_abstract: bool,
    pub is_extend: bool,
    pub items: Vec<CompItem>,
}

/// One statement of an `override` block (PSS 20.5.1).
#[derive(Debug, Clone, PartialEq)]
pub struct OverrideDecl {
    /// `Some(path)` for an instance override, `None` for a type override.
    pub instance: Option<String>,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompItem {
    Import(Path),
    Member(Member),
    Action(ActionDecl),
    Pool(PoolDecl),
    Bind(BindDecl),
    Exec(ExecBlock),
    Constraint(ConstraintBlock),
    Enum(EnumDecl),
    Struct(TypeDecl),
    StaticConst { name: String, value: Expr },
    /// `symbol NAME { activity_stmts }` — a named activity fragment
    /// (PSS 12.7, Syntax 43).
    Symbol { name: String, body: Activity },
    /// `override { type A with B; instance p with B; }` (PSS 20.5, Syntax 80).
    Override(Vec<OverrideDecl>),
    Raw(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Import(Path),
    Package(Package),
    Enum(EnumDecl),
    Struct(TypeDecl),
    Component(ComponentDecl),
    Action(ActionDecl),
    StaticConst { name: String, value: Expr },
    CompileIf {
        cond: String,
        then_items: Vec<Item>,
        else_items: Vec<Item>,
    },
    /// Anything the parser recognised but does not model (kept verbatim).
    Raw(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    pub items: Vec<Item>,
    /// Covergroups declared anywhere in the file (PSS 18).
    pub covergroups: Vec<Covergroup>,
    /// Monitor declarations anywhere in the file (PSS 19.1).
    pub monitors: Vec<Monitor>,
}

// ---------------------------------------------------------------------------
// Activities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Activity {
    Sequence(Vec<ActivityStmt>),
}

/// Fine-grained scheduling specifier on a `parallel`/`schedule` block
/// (PSS 12.3.6).
#[derive(Debug, Clone, PartialEq)]
pub enum JoinSpec {
    /// `join_all` — the default: wait for every branch.
    All,
    /// `join_none` — no scheduling dependency on the branches.
    None,
    /// `join_first(n)` — a runtime dependency on the first n to finish.
    First(Expr),
    /// `join_branch(L1, L2)` — wait for the named branches.
    Branch(Vec<String>),
    /// `join_select(n)` — wait for n randomly selected branches.
    Select(Expr),
}

/// A coverage point or cross declared in a covergroup (PSS 18.3, 18.4).
#[derive(Debug, Clone, PartialEq)]
pub enum CoverItem {
    Coverpoint {
        label: Option<String>,
        expr: Expr,
        iff: Option<Expr>,
        /// `bins <name> = <expr>;` — an explicit bin.
        bins: Vec<(String, Expr)>,
    },
    Cross {
        label: Option<String>,
        refs: Vec<String>,
        iff: Option<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Covergroup {
    pub name: String,
    pub items: Vec<CoverItem>,
}

/// A monitor's activity pattern (PSS 19.3, Syntax 69-78).
#[derive(Debug, Clone, PartialEq)]
pub enum MonStmt {
    /// `do type [with constraint];` or a named action handle.
    Traverse {
        action: String,
        handle: Option<String>,
        cond: Option<Expr>,
    },
    /// `sequence { .. }` — consecutive in the trace (19.3.2).
    Sequence(Vec<MonStmt>),
    /// `concat { .. }` — ordered, but gaps are allowed (19.3.3).
    Concat(Vec<MonStmt>),
    /// `eventually { .. }` — matches at any checkpoint at or after the start (19.3.4).
    Eventually(Vec<MonStmt>),
    /// `overlap { .. }` / `schedule { .. }` — concurrent branches (19.3.5, 19.3.8).
    Concurrent(Vec<MonStmt>),
    /// `select { guard: { .. } .. }` (19.3.6).
    Select(Vec<(Option<Expr>, Vec<MonStmt>)>),
    /// `do <monitor_type>` — a nested monitor (19.3.9).
    MonitorRef(String),
    /// `constraint <set>` inside a monitor (19.4).
    Constraint(Expr),
    /// The empty scenario, which matches anything (19.3.7).
    Empty,
}

/// A monitor declaration (PSS 19.1, Syntax 68).
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub name: String,
    /// A monitor declared inside a component is scoped to it.
    pub component: Option<String>,
    pub pattern: MonStmt,
    pub covergroups: Vec<crate::ast::Covergroup>,
}



#[derive(Debug, Clone, PartialEq)]
pub enum ActivityStmt {
    /// `do SomeAction;` — optionally component-qualified.
    Do(Path),
    /// `L: <stmt>` — a labelled activity statement (PSS `labeled_activity_stmt`).
    Labeled { label: String, stmt: Box<ActivityStmt> },
    /// `do SomeAction { ... }` — an inline activity body.
    DoWith { action: Path, body: Option<Activity> },
    /// `do SomeAction with { constraints }` — inline constraints (LRM
    /// `inline_constraints_or_empty`).
    DoConstrained { action: Path, constraints: Vec<Expr> },
    Parallel {
        branches: Vec<Activity>,
        join: JoinSpec,
    },
    Schedule {
        branches: Vec<Activity>,
        join: JoinSpec,
    },
    /// `select { [(guard)][[index]] : branch ... }`
    Select(Vec<(Option<Expr>, Activity)>),
    /// `match (expr) { [range] : branch | default : branch }`
    Match {
        expr: Expr,
        choices: Vec<(Option<Expr>, Activity)>,
    },
    /// `repeat ( [index :] count ) body` and `replicate ( [index :] count ) body`.
    /// The two differ: `replicate` expands in place into n labelled copies
    /// (PSS 12.5) and requires a label; `repeat` is a loop.
    Repeat {
        count: Expr,
        index: Option<String>,
        body: Activity,
        is_replicate: bool,
        /// `repeat { .. } while (cond);` (Syntax 37) — the loop condition.
        while_cond: Option<Expr>,
    },
    Foreach {
        var: String,
        iter: Expr,
        body: Activity,
    },
    Atomic(Activity),
    /// `constraint <set>` inside an activity (LRM `activity_constraint_stmt`).
    Constraint(Expr),
    /// `constraint parallel { a, b };` / `constraint sequence { a, b };` —
    /// a scheduling constraint between action handles (PSS 16.2, Syntax 62).
    SchedConstraint { parallel: bool, refs: Vec<String> },
    /// `name(args);` — a call to a symbol declared with `symbol` (PSS 12.7).
    SymbolCall(String, Vec<Expr>),
    /// `[label:] cover { <monitor body> }` — monitor the executed stream for a
    /// scenario (PSS 19.2).
    Cover {
        label: Option<String>,
        body: Option<Activity>,
        /// `cover <monitor_type>;` — the monitor to match against the stream.
        monitor: Option<String>,
    },
    If {
        cond: Expr,
        then: Activity,
        els: Option<Activity>,
    },
    Raw(String),
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    /// A floating-point literal (PSS Syntax 1). Its value in the engine is the
    /// IEEE-754 bit pattern, as for `float_pkg`.
    Float(f64),
    Str(String),
    Bool(bool),
    /// A reference such as `data_buf_out.mem_seg.trait.row`.
    Path(Path),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `f(a, b)` — free function call.
    Call(String, Vec<Expr>),
    /// `recv.m(a, b)` — method-style call (e.g. `addr_value_abs(...)` treated as call).
    Method(Box<Expr>, String, Vec<Expr>),
    /// `x in [lo..hi]`
    InRange(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `x in {a, b, c}`
    InSet(Box<Expr>, Vec<Expr>),
    /// `(int)expr`
    Cast(String, Box<Expr>),
    /// `a..b`
    Range(Box<Expr>, Box<Expr>),
    /// `foreach (it : T) { body }` used inside constraints.
    Foreach {
        vars: Vec<String>,
        iter: Box<Expr>,
        body: Box<Expr>,
    },
    /// `a[i]`
    Index(Box<Expr>, Box<Expr>),
    /// A bit part-select: `base[msb:lsb]` (PSS 8.6.1).
    Slice(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `{a, b, c}`
    List(Vec<Expr>),
    /// `sizeof_s<T>::nbits` / `::nbytes` (PSS 24.10.2).
    Sizeof { ty: String, bits: bool },
    /// `x inside { ... }` / `x dist { ... }` — captured, not solved.
    Unknown(String),
}

impl Expr {
    /// Best-effort constant evaluation (used for widths, counts, constants).
    pub fn eval_const(&self, consts: &dyn Fn(&str) -> Option<i64>) -> Option<i64> {
        match self {
            Expr::Int(v) => Some(*v),
            Expr::Bool(b) => Some(if *b { 1 } else { 0 }),
            Expr::Path(p) if p.segments.len() == 1 => consts(&p.segments[0]),
            Expr::Unary(op, e) => {
                let v = e.eval_const(consts)?;
                match op.as_str() {
                    "-" => Some(-v),
                    "+" => Some(v),
                    "!" => Some(if v == 0 { 1 } else { 0 }),
                    _ => None,
                }
            }
            Expr::Binary(op, a, b) => {
                let x = a.eval_const(consts)?;
                let y = b.eval_const(consts)?;
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
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
