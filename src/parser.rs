//! Recursive-descent parser for the PSS v3.0 subset understood by Satya.
//!
//! The parser is deliberately forgiving: constructs outside the modelled
//! subset (raw `exec` bodies, `compile if`, covergroups, functions) are
//! captured verbatim or skipped so that a real-world `.pss` file still parses
//! and the modelled parts remain usable.

use crate::ast::*;
use crate::error::{PssError, PssResult};
use crate::lexer::{tokenize, TokKind, Token};

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser { toks, pos: 0 }
    }

    // ---- token helpers -------------------------------------------------

    fn peek(&self) -> &Token {
        self.toks.get(self.pos).unwrap_or_else(|| self.toks.last().unwrap())
    }

    fn peek_at(&self, n: usize) -> &Token {
        self.toks
            .get(self.pos + n)
            .unwrap_or_else(|| self.toks.last().unwrap())
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokKind::Eof)
    }

    fn at_punct(&self, p: &str) -> bool {
        self.peek().is_punct(p)
    }

    fn at_ident(&self, name: &str) -> bool {
        self.peek().is_ident(name)
    }

    fn bump(&mut self) -> Token {
        let t = self.peek().clone();
        if !self.at_eof() {
            self.pos += 1;
        }
        t
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        if self.at_punct(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_ident(&mut self, name: &str) -> bool {
        if self.at_ident(name) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// True when the next token closes a template (`>` or the `>>` that ends
    /// two nested templates).
    fn at_gt(&self) -> bool {
        self.at_punct(">") || self.at_punct(">>")
    }

    /// Consume one `>`, splitting a `>>` token so the second `>` remains for
    /// the enclosing template. This is how `a<b<c>>` is disambiguated.
    fn eat_gt(&mut self) -> bool {
        if self.at_punct(">") {
            self.bump();
            return true;
        }
        if self.at_punct(">>") {
            if let TokKind::Punct(ref mut s) = self.toks[self.pos].kind {
                *s = ">".to_string();
            }
            return true;
        }
        false
    }

    fn err<T>(&self, msg: impl Into<String>) -> PssResult<T> {
        let t = self.peek();
        Err(PssError::new(msg, t.line, t.col))
    }

    fn expect_punct(&mut self, p: &str) -> PssResult<()> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            self.err(format!("expected `{}`, found {}", p, describe(self.peek())))
        }
    }

    fn expect_ident(&mut self) -> PssResult<String> {
        match &self.peek().kind {
            TokKind::Ident(s) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            other => self.err(format!("expected identifier, found {}", describe_kind(other))),
        }
    }

    fn expect_ident_named(&mut self, name: &str) -> PssResult<()> {
        if self.eat_ident(name) {
            Ok(())
        } else {
            self.err(format!("expected `{}`, found {}", name, describe(self.peek())))
        }
    }

    // ---- skip helpers --------------------------------------------------

    fn skip_to_semicolon(&mut self) {
        let mut depth = 0i32;
        while !self.at_eof() {
            if self.at_punct("{") {
                depth += 1;
            } else if self.at_punct("}") {
                if depth == 0 {
                    return;
                }
                depth -= 1;
            } else if self.at_punct(";") && depth == 0 {
                self.bump();
                return;
            }
            self.bump();
        }
    }

    /// Consume a `{ ... }` block (assumes the opening brace is next) and
    /// return the inner tokens.
    fn capture_braced_tokens(&mut self) -> PssResult<Vec<Token>> {
        self.expect_punct("{")?;
        let mut depth = 1i32;
        let mut out: Vec<Token> = Vec::new();
        while !self.at_eof() {
            if self.at_punct("{") {
                depth += 1;
            } else if self.at_punct("}") {
                depth -= 1;
                if depth == 0 {
                    self.bump();
                    break;
                }
            }
            out.push(self.bump());
        }
        Ok(out)
    }

    /// Consume a `{ ... }` block and return the inner tokens joined as text.
    #[allow(dead_code)]
    fn capture_braced_body(&mut self) -> PssResult<String> {
        let toks = self.capture_braced_tokens()?;
        Ok(toks.iter().map(token_text).collect::<Vec<_>>().join(" "))
    }

    fn skip_braced(&mut self) -> PssResult<()> {
        if !self.at_punct("{") {
            return Ok(());
        }
        self.expect_punct("{")?;
        let mut depth = 1i32;
        while !self.at_eof() {
            if self.at_punct("{") {
                depth += 1;
            } else if self.at_punct("}") {
                depth -= 1;
                if depth == 0 {
                    self.bump();
                    return Ok(());
                }
            }
            self.bump();
        }
        Ok(())
    }

    // ---- entry point ---------------------------------------------------

    pub fn parse_file(&mut self) -> PssResult<SourceFile> {
        let items = self.parse_items()?;
        Ok(SourceFile { items })
    }

    fn parse_items(&mut self) -> PssResult<Vec<Item>> {
        let mut items = Vec::new();
        while !self.at_eof() && !self.at_punct("}") {
            if self.eat_punct(";") {
                continue;
            }
            let item = self.parse_item()?;
            items.push(item);
        }
        Ok(items)
    }

    fn parse_item(&mut self) -> PssResult<Item> {
        // `pure component` / `pure action`.
        if self.at_ident("pure") {
            self.bump();
            return self.parse_item();
        }
        if self.at_ident("import") {
            return Ok(Item::Import(self.parse_import()?));
        }
        if self.at_ident("package") {
            return Ok(Item::Package(self.parse_package()?));
        }
        if self.at_ident("enum") {
            return Ok(Item::Enum(self.parse_enum(false, None)?));
        }
        if self.at_ident("struct") || self.at_ident("resource") || self.at_ident("buffer") || self.at_ident("state") {
            return Ok(Item::Struct(self.parse_type_decl(false)?));
        }
        if self.at_ident("component") {
            return Ok(Item::Component(self.parse_component(false)?));
        }
        if self.at_ident("action") || (self.at_ident("abstract") && self.peek_at(1).is_ident("action")) {
            return Ok(Item::Action(self.parse_action(false)?));
        }
        if self.at_ident("static") || self.at_ident("const") {
            if let Some(sc) = self.try_parse_static_const()? {
                return Ok(sc);
            }
        }
        if self.at_ident("compile") {
            return self.parse_compile_if();
        }
        if self.at_ident("extend") {
            return self.parse_extend();
        }
        if self.at_ident("function") || self.at_ident("task") {
            self.bump();
            self.skip_to_semicolon();
            return Ok(Item::Raw("function".into()));
        }
        // Unknown top-level construct: skip a balanced chunk defensively.
        let start = self.bump();
        self.skip_to_semicolon();
        Ok(Item::Raw(token_text(&start)))
    }

    // ---- imports / packages -------------------------------------------

    fn parse_import(&mut self) -> PssResult<Path> {
        self.expect_ident_named("import")?;
        let path = self.parse_scoped_path()?;
        // `import P3::P4 as P2;`
        if self.eat_ident("as") {
            let _ = self.expect_ident()?;
        }
        self.expect_punct(";")?;
        Ok(path)
    }

    fn parse_package(&mut self) -> PssResult<Package> {
        self.expect_ident_named("package")?;
        let name = self.expect_ident()?;
        self.expect_punct("{")?;
        let items = self.parse_items()?;
        self.expect_punct("}")?;
        self.eat_punct(";");
        Ok(Package { name, items })
    }

    /// Parse `a::b::c` or `a::*`.
    fn parse_scoped_path(&mut self) -> PssResult<Path> {
        let mut segments = Vec::new();
        loop {
            if self.at_punct("*") {
                segments.push("*".to_string());
                self.bump();
            } else {
                segments.push(self.expect_ident()?);
            }
            if self.at_punct("::") {
                self.bump();
                continue;
            }
            break;
        }
        Ok(Path::new(segments))
    }

    // ---- enums ---------------------------------------------------------

    fn parse_enum(&mut self, is_extend: bool, base: Option<Path>) -> PssResult<EnumDecl> {
        self.expect_ident_named("enum")?;
        let name = self.parse_scoped_path()?;
        let mut variants = Vec::new();
        if self.eat_punct("{") {
            while !self.at_punct("}") && !self.at_eof() {
                if let Some(id) = self.peek().ident() {
                    variants.push(id.to_string());
                    self.bump();
                    // Optional `= value`
                    if self.eat_punct("=") {
                        let _ = self.parse_expr(0)?;
                    }
                } else {
                    self.bump();
                }
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("}")?;
        }
        self.eat_punct(";");
        Ok(EnumDecl {
            name: name.leaf(),
            variants,
            is_extend,
            base,
        })
    }

    // ---- type declarations (struct/resource/buffer/state) --------------

    fn parse_type_decl(&mut self, is_extend: bool) -> PssResult<TypeDecl> {
        let keyword = self.expect_ident()?; // struct/resource/buffer/state
        let name = self.parse_scoped_path()?.leaf();
        let template_params = self.parse_template_params()?;
        let base = if self.eat_punct(":") {
            Some(self.parse_type_ref()?)
        } else {
            None
        };
        let (members, constraints) = self.parse_data_body()?;
        Ok(TypeDecl {
            keyword,
            name,
            base,
            template_params,
            members,
            constraints,
            is_extend,
        })
    }

    fn parse_data_body(&mut self) -> PssResult<(Vec<Member>, Vec<ConstraintBlock>)> {
        let mut members = Vec::new();
        let mut constraints = Vec::new();
        self.expect_punct("{")?;
        while !self.at_punct("}") && !self.at_eof() {
            if self.eat_punct(";") {
                continue;
            }
            if self.at_ident("constraint") {
                constraints.push(self.parse_constraint()?);
                continue;
            }
            if self.at_ident("exec") {
                let _ = self.parse_exec()?;
                continue;
            }
            if self.at_ident("function") || self.at_ident("task") || self.at_ident("covergroup") {
                self.bump();
                self.skip_to_semicolon();
                continue;
            }
            if self.at_ident("compile") {
                self.skip_braced_or_semicolon();
                continue;
            }
            // Otherwise: a member declaration.
            if let Some(m) = self.try_parse_member()? {
                members.push(m);
            } else {
                self.skip_to_semicolon();
            }
        }
        self.expect_punct("}")?;
        self.eat_punct(";");
        Ok((members, constraints))
    }

    fn skip_braced_or_semicolon(&mut self) {
        // Skip a construct that may be brace-delimited or end in `;`.
        let mut guard = 0;
        while !self.at_eof() && guard < 100_000 {
            guard += 1;
            if self.at_punct("{") {
                let _ = self.skip_braced();
                return;
            }
            if self.at_punct(";") {
                self.bump();
                return;
            }
            self.bump();
        }
    }

    // ---- members -------------------------------------------------------

    fn try_parse_member(&mut self) -> PssResult<Option<Member>> {
        let mut kind = MemberKind::Plain;
        let mut is_static = false;
        loop {
            if self.at_ident("rand") {
                kind = MemberKind::Rand;
                self.bump();
            } else if self.at_ident("input") {
                kind = MemberKind::Input;
                self.bump();
            } else if self.at_ident("output") {
                kind = MemberKind::Output;
                self.bump();
            } else if self.at_ident("lock") {
                kind = MemberKind::Lock;
                self.bump();
            } else if self.at_ident("share") {
                kind = MemberKind::Share;
                self.bump();
            } else if self.at_ident("static") {
                is_static = true;
                self.bump();
            } else if self.at_ident("const") {
                self.bump();
            } else {
                break;
            }
        }

        // The next token must begin a type. Guard against stray tokens.
        if !matches!(self.peek().kind, TokKind::Ident(_)) {
            return Ok(None);
        }

        let ty = self.parse_type_ref()?;

        // Inline range: `in [lo..hi]` before the name. Require `[` or `{` to
        // follow so a field literally named `in` is not mistaken for a range.
        let mut inline = None;
        if self.at_ident("in") && (self.peek_at(1).is_punct("[") || self.peek_at(1).is_punct("{")) {
            self.bump();
            let rng = self.parse_range_or_set()?;
            inline = Some(rng);
        }

        let name = match &self.peek().kind {
            TokKind::Ident(_) => self.expect_ident()?,
            _ => return Ok(None),
        };

        // Also accept the range after the name (`rand int v in [1..64]`), which
        // some authors write even though the LRM places it before the name.
        // Require `[` or `{` to follow, so a field literally named `in`
        // (`input data_buf in;`) is not mistaken for a range.
        if self.at_ident("in") && (self.peek_at(1).is_punct("[") || self.peek_at(1).is_punct("{")) {
            self.bump();
            let rng = self.parse_range_or_set()?;
            inline = Some(rng);
        }

        let array = if self.at_punct("[") {
            self.bump();
            let e = self.parse_expr(0)?;
            self.expect_punct("]")?;
            Some(e)
        } else {
            None
        };

        let init = if self.eat_punct("=") {
            Some(self.parse_expr(0)?)
        } else {
            None
        };

        if !self.eat_punct(";") {
            // Declaration without a semicolon (e.g. an instance list): recover.
            self.skip_to_semicolon();
        }

        // Fold an inline range into a proper `name in [lo..hi]` constraint.
        let inline_constraint = inline.map(|rng| match rng {
            Expr::InRange(_, lo, hi) => Expr::InRange(
                Box::new(Expr::Path(Path::simple(name.clone()))),
                lo,
                hi,
            ),
            other => other,
        });

        Ok(Some(Member {
            kind,
            ty,
            name,
            inline_constraint,
            array,
            init,
            is_static,
        }))
    }

    /// Parse a bit-range suffix `[N]` or `[msb:lsb]`; returns a width
    /// expression (the latter becomes `msb - lsb + 1`).
    fn parse_bit_range(&mut self) -> PssResult<Expr> {
        self.expect_punct("[")?;
        let hi = self.parse_expr(0)?;
        let width = if self.eat_punct(":") {
            let lo = self.parse_expr(0)?;
            Expr::Binary(
                "+".into(),
                Box::new(Expr::Binary("-".into(), Box::new(hi), Box::new(lo))),
                Box::new(Expr::Int(1)),
            )
        } else {
            hi
        };
        self.expect_punct("]")?;
        Ok(width)
    }

    fn parse_type_ref(&mut self) -> PssResult<TypeRef> {
        // `ref T` reference type.
        if self.at_ident("ref") {
            self.bump();
            return self.parse_type_ref();
        }
        // `bit[N]` or `bit[msb:lsb]` sized type.
        if self.at_ident("bit") {
            self.bump();
            let mut width = None;
            if self.at_punct("[") {
                let e = self.parse_bit_range()?;
                width = Some(Box::new(e));
            }
            return Ok(TypeRef {
                path: Path::simple("bit"),
                template_args: Vec::new(),
                width,
                open_template: false,
            });
        }

        let path = self.parse_scoped_path()?;
        let mut template_args = Vec::new();
        let mut open_template = false;
        if self.at_punct("<") {
            self.bump();
            if self.at_gt() {
                open_template = true;
                self.eat_gt();
            } else {
                loop {
                    if self.at_gt() {
                        break;
                    }
                    // A template argument may be a constant expression
                    // (`array<bit[15:0], 5>`, `sized_addr_handle_s<32>`).
                    if matches!(self.peek().kind, TokKind::Int(_)) {
                        // Precedence above comparison so the closing `>` is not
                        // swallowed as a `>` operator.
                        let _ = self.parse_expr(9)?;
                    } else {
                        template_args.push(self.parse_type_ref()?);
                    }
                    if self.eat_punct(",") {
                        continue;
                    }
                    break;
                }
                if !self.eat_gt() {
                    return self.err("expected `>` to close template arguments");
                }
            }
        }
        // Sizing suffix on an arbitrary type, e.g. `int[4]` or `int[7:0]`.
        let mut width = None;
        if self.at_punct("[") {
            let e = self.parse_bit_range()?;
            width = Some(Box::new(e));
        }
        Ok(TypeRef {
            path,
            template_args,
            width,
            open_template,
        })
    }

    /// Parse declaration-site template parameters such as
    /// `<struct TRAIT = mem_trait_s, struct CLAIM = mem_segment_s<TRAIT>>`.
    fn parse_template_params(&mut self) -> PssResult<Vec<String>> {
        let mut names = Vec::new();
        if !self.at_punct("<") {
            return Ok(names);
        }
        self.bump();
        let mut depth = 1i32;
        let mut pending_name: Option<String> = None;
        while !self.at_eof() && depth > 0 {
            if self.at_punct("<") {
                depth += 1;
                self.bump();
                continue;
            }
            if self.at_gt() {
                // A `>>` may close two nested template lists at once.
                depth -= 1;
                self.eat_gt();
                continue;
            }
            if depth == 1 {
                if let Some(id) = self.peek().ident() {
                    pending_name = Some(id.to_string());
                }
                if self.at_punct(",") {
                    if let Some(n) = pending_name.take() {
                        names.push(n);
                    }
                }
                if self.at_punct("=") {
                    // Skip the default value, which may itself be templated.
                    self.bump();
                    let mut d = 0i32;
                    while !self.at_eof() {
                        if self.at_punct("<") {
                            d += 1;
                            self.bump();
                            continue;
                        }
                        if self.at_gt() {
                            if d == 0 {
                                break;
                            }
                            d -= 1;
                            self.eat_gt();
                            continue;
                        }
                        if self.at_punct(",") && d == 0 {
                            break;
                        }
                        self.bump();
                    }
                    if let Some(n) = pending_name.take() {
                        names.push(n);
                    }
                    continue;
                }
            }
            self.bump();
        }
        if let Some(n) = pending_name.take() {
            names.push(n);
        }
        Ok(names)
    }

    // ---- constraints ---------------------------------------------------

    fn parse_constraint(&mut self) -> PssResult<ConstraintBlock> {
        self.expect_ident_named("constraint")?;
        let mut is_default = false;
        if self.eat_ident("default") {
            is_default = true;
        }
        let mut is_forall = false;
        let mut forall_binding = None;
        if self.eat_ident("forall") {
            is_forall = true;
            self.expect_punct("(")?;
            let var = self.expect_ident()?;
            self.expect_punct(":")?;
            let ty = self.parse_type_ref()?;
            self.expect_punct(")")?;
            forall_binding = Some((var, ty));
        }

        // Named block: `constraint name { ... }`
        let name = if matches!(self.peek().kind, TokKind::Ident(_)) && self.peek_at(1).is_punct("{") {
            Some(self.expect_ident()?)
        } else {
            None
        };

        let expr = if name.is_some() || self.at_punct("{") {
            self.parse_constraint_set()?
        } else {
            let e = self.parse_constraint_body_item()?;
            // The LRM requires a `;`, but real models often omit it.
            self.eat_punct(";");
            e
        };

        Ok(ConstraintBlock {
            name,
            is_default,
            is_forall,
            forall_binding,
            expr,
        })
    }

    /// Parse a constraint set: either a `{ item* }` block or a single item.
    fn parse_constraint_set(&mut self) -> PssResult<Expr> {
        if self.at_punct("{") {
            self.bump();
            let mut exprs = Vec::new();
            while !self.at_punct("}") && !self.at_eof() {
                if self.eat_punct(";") {
                    continue;
                }
                exprs.push(self.parse_constraint_body_item()?);
                self.eat_punct(";");
            }
            self.expect_punct("}")?;
            Ok(combine_and(exprs))
        } else {
            self.parse_constraint_body_item()
        }
    }

    /// One constraint body item: an `if`/`else` constraint, or an expression.
    fn parse_constraint_body_item(&mut self) -> PssResult<Expr> {
        if self.at_ident("if") {
            self.bump();
            self.expect_punct("(")?;
            let cond = self.parse_expr(0)?;
            self.expect_punct(")")?;
            let then = self.parse_constraint_set()?;
            let els = if self.eat_ident("else") {
                Some(self.parse_constraint_set()?)
            } else {
                None
            };
            return Ok(Expr::Ternary(
                Box::new(cond),
                Box::new(then),
                Box::new(els.unwrap_or(Expr::Bool(true))),
            ));
        }
        self.parse_expr(0)
    }

    // ---- exec ----------------------------------------------------------

    fn parse_exec(&mut self) -> PssResult<ExecBlock> {
        self.expect_ident_named("exec")?;
        let kind = self.expect_ident()?;
        let (body, stmts) = if self.at_punct("{") {
            let toks = self.capture_braced_tokens()?;
            let text = toks.iter().map(token_text).collect::<Vec<_>>().join(" ");
            (text, parse_exec_stmts(&toks))
        } else {
            self.eat_punct(";");
            (String::new(), Vec::new())
        };
        Ok(ExecBlock { kind, body, stmts })
    }

    /// Parse the body of an `if` inside an `exec` block.
    fn parse_exec_block(&mut self) -> Vec<ExecStmt> {
        if !self.at_punct("{") {
            return Vec::new();
        }
        match self.capture_braced_tokens() {
            Ok(toks) => parse_exec_stmts(&toks),
            Err(_) => Vec::new(),
        }
    }

    /// Parse one statement inside an `exec` body.
    fn try_parse_exec_stmt(&mut self) -> Option<ExecStmt> {
        if self.at_ident("if") {
            self.bump();
            self.expect_punct("(").ok()?;
            let cond = self.parse_expr(0).ok()?;
            self.expect_punct(")").ok()?;
            let then = self.parse_exec_block();
            let els = if self.eat_ident("else") {
                self.parse_exec_block()
            } else {
                Vec::new()
            };
            return Some(ExecStmt::If(cond, then, els));
        }
        let save = self.pos;
        let e = match self.parse_expr(0) {
            Ok(e) => e,
            Err(_) => {
                self.pos = save;
                return None;
            }
        };
        if self.at_punct("=") {
            self.bump();
            if let Ok(rhs) = self.parse_expr(0) {
                self.eat_punct(";");
                if let Expr::Path(path) = e {
                    return Some(ExecStmt::Assign(path, rhs));
                }
            }
            self.pos = save;
            return None;
        }
        self.eat_punct(";");
        match e {
            Expr::Call(name, args) => Some(ExecStmt::Call(name, args)),
            Expr::Method(recv, m, args) => {
                let r = match *recv {
                    Expr::Path(p) => p.segments.join("."),
                    other => format!("{:?}", other),
                };
                Some(ExecStmt::Call(format!("{}.{}", r, m), args))
            }
            _ => None,
        }
    }

    // ---- components ----------------------------------------------------

    fn parse_component(&mut self, is_extend: bool) -> PssResult<ComponentDecl> {
        self.expect_ident_named("component")?;
        let name = self.expect_ident()?;
        let template_params = self.parse_template_params()?;
        let base = if self.eat_punct(":") {
            Some(self.parse_type_ref()?)
        } else {
            None
        };
        let mut is_abstract = false;
        if self.eat_ident("abstract") {
            is_abstract = true;
        }
        self.expect_punct("{")?;
        let mut items = Vec::new();
        while !self.at_punct("}") && !self.at_eof() {
            if self.eat_punct(";") {
                continue;
            }
            let item = self.parse_comp_item()?;
            items.push(item);
        }
        self.expect_punct("}")?;
        self.eat_punct(";");
        Ok(ComponentDecl {
            name,
            base,
            template_params,
            is_abstract,
            is_extend,
            items,
        })
    }

    fn parse_comp_item(&mut self) -> PssResult<CompItem> {
        if self.at_ident("import") {
            return Ok(CompItem::Import(self.parse_import()?));
        }
        if self.at_ident("action") || (self.at_ident("abstract") && self.peek_at(1).is_ident("action")) {
            return Ok(CompItem::Action(self.parse_action(false)?));
        }
        if self.at_ident("pool") {
            return Ok(CompItem::Pool(self.parse_pool()?));
        }
        if self.at_ident("bind") {
            return Ok(CompItem::Bind(self.parse_bind()?));
        }
        if self.at_ident("exec") {
            return Ok(CompItem::Exec(self.parse_exec()?));
        }
        if self.at_ident("constraint") {
            return Ok(CompItem::Constraint(self.parse_constraint()?));
        }
        if self.at_ident("enum") {
            return Ok(CompItem::Enum(self.parse_enum(false, None)?));
        }
        if self.at_ident("struct") || self.at_ident("resource") || self.at_ident("buffer") || self.at_ident("state") {
            return Ok(CompItem::Struct(self.parse_type_decl(false)?));
        }
        if self.at_ident("static") || self.at_ident("const") {
            if let Some(Item::StaticConst { name, value }) = self.try_parse_static_const()? {
                return Ok(CompItem::StaticConst { name, value });
            }
        }
        if self.at_ident("function") || self.at_ident("task") || self.at_ident("covergroup") {
            self.bump();
            self.skip_to_semicolon();
            return Ok(CompItem::Raw("function".into()));
        }
        if self.at_ident("compile") {
            let cond = self.capture_compile_cond();
            let _ = cond;
            self.skip_braced_or_semicolon();
            return Ok(CompItem::Raw("compile".into()));
        }
        // A data member or a sub-component instance.
        if let Some(m) = self.try_parse_member()? {
            return Ok(CompItem::Member(m));
        }
        self.skip_to_semicolon();
        Ok(CompItem::Raw("member".into()))
    }

    fn parse_pool(&mut self) -> PssResult<PoolDecl> {
        self.expect_ident_named("pool")?;
        let count = if self.at_punct("[") {
            self.bump();
            let e = self.parse_expr(0)?;
            self.expect_punct("]")?;
            Some(e)
        } else {
            None
        };
        let ty = self.parse_type_ref()?;
        let name = self.expect_ident()?;
        self.expect_punct(";")?;
        Ok(PoolDecl { ty, name, count })
    }

    fn parse_bind(&mut self) -> PssResult<BindDecl> {
        self.expect_ident_named("bind")?;
        let pool = self.parse_scoped_path()?;
        let mut wildcard = false;
        let mut target = None;
        if self.eat_punct("*") {
            wildcard = true;
        } else if !self.at_punct(";") {
            target = Some(self.parse_scoped_path()?);
        }
        self.expect_punct(";")?;
        Ok(BindDecl {
            pool,
            wildcard,
            target,
        })
    }

    // ---- actions -------------------------------------------------------

    fn parse_action(&mut self, is_extend: bool) -> PssResult<ActionDecl> {
        let mut is_abstract = false;
        if self.eat_ident("abstract") {
            is_abstract = true;
        }
        self.expect_ident_named("action")?;
        // The name may be qualified when extending (`extend action C::A`).
        let name = self.parse_scoped_path()?.leaf();
        let template_params = self.parse_template_params()?;
        let base = if self.eat_punct(":") {
            Some(self.parse_type_ref()?)
        } else {
            None
        };
        let mut members = Vec::new();
        let mut constraints = Vec::new();
        let mut execs = Vec::new();
        let mut activity = None;
        self.expect_punct("{")?;
        while !self.at_punct("}") && !self.at_eof() {
            if self.eat_punct(";") {
                continue;
            }
            if self.at_ident("constraint") {
                constraints.push(self.parse_constraint()?);
                continue;
            }
            if self.at_ident("exec") {
                execs.push(self.parse_exec()?);
                continue;
            }
            if self.at_ident("activity") {
                activity = Some(self.parse_activity()?);
                continue;
            }
            if self.at_ident("function") || self.at_ident("task") || self.at_ident("covergroup") {
                self.bump();
                self.skip_to_semicolon();
                continue;
            }
            if self.at_ident("compile") {
                self.skip_braced_or_semicolon();
                continue;
            }
            if let Some(m) = self.try_parse_member()? {
                members.push(m);
            } else {
                self.skip_to_semicolon();
            }
        }
        self.expect_punct("}")?;
        self.eat_punct(";");
        Ok(ActionDecl {
            name,
            base,
            template_params,
            is_abstract,
            members,
            constraints,
            execs,
            activity,
            is_extend,
        })
    }

    // ---- activities ----------------------------------------------------

    fn parse_activity(&mut self) -> PssResult<Activity> {
        self.expect_ident_named("activity")?;
        self.parse_activity_block()
    }

    /// Parse a brace-delimited activity body: `{ stmt* }` with an optional
    /// trailing `;`.
    fn parse_activity_block(&mut self) -> PssResult<Activity> {
        self.expect_punct("{")?;
        let act = self.parse_activity_sequence()?;
        self.expect_punct("}")?;
        self.eat_punct(";");
        Ok(act)
    }

    /// Parse an activity body: a `{ … }` block or a single statement
    /// (the LRM allows `repeat (n) do X;`, `if (c) do X;`, and so on).
    fn parse_activity_body(&mut self) -> PssResult<Activity> {
        if self.at_punct("{") {
            self.parse_activity_block()
        } else if let Some(s) = self.parse_activity_stmt()? {
            Ok(Activity::Sequence(vec![s]))
        } else {
            Ok(Activity::Sequence(Vec::new()))
        }
    }

    /// Skip an optional `join_branch(..)` / `join_select(..)` / `join_none` /
    /// `join_first(..)` after `parallel` or `schedule`.
    fn skip_join_spec(&mut self) {
        if self.at_ident("join_branch")
            || self.at_ident("join_select")
            || self.at_ident("join_none")
            || self.at_ident("join_first")
        {
            self.bump();
            if self.at_punct("(") {
                self.skip_parens();
            }
        }
    }

    fn skip_parens(&mut self) {
        if !self.at_punct("(") {
            return;
        }
        let mut depth = 0i32;
        while !self.at_eof() {
            if self.at_punct("(") {
                depth += 1;
            } else if self.at_punct(")") {
                depth -= 1;
                if depth == 0 {
                    self.bump();
                    return;
                }
            }
            self.bump();
        }
    }

    fn parse_activity_sequence(&mut self) -> PssResult<Activity> {
        let mut stmts = Vec::new();
        while !self.at_punct("}") && !self.at_eof() {
            if self.eat_punct(";") {
                continue;
            }
            if let Some(s) = self.parse_activity_stmt()? {
                stmts.push(s);
            }
        }
        Ok(Activity::Sequence(stmts))
    }

    fn parse_activity_branches(&mut self) -> PssResult<Vec<Activity>> {
        self.expect_punct("{")?;
        let mut branches = Vec::new();
        while !self.at_punct("}") && !self.at_eof() {
            if self.eat_punct(";") {
                continue;
            }
            if self.at_punct("{") {
                branches.push(self.parse_activity_block()?);
                continue;
            }
            if let Some(s) = self.parse_activity_stmt()? {
                branches.push(Activity::Sequence(vec![s]));
            }
        }
        self.expect_punct("}")?;
        Ok(branches)
    }

    fn parse_activity_stmt(&mut self) -> PssResult<Option<ActivityStmt>> {
        // Optional label: `label : stmt`
        if let TokKind::Ident(id) = &self.peek().kind {
            let keyword = is_activity_keyword(id);
            if !keyword && self.peek_at(1).is_punct(":") {
                self.bump(); // label
                self.bump(); // ':'
                return self.parse_activity_stmt();
            }
            // A bare action traversal: `a1;`, `a1 with {..};`, `comp::a1;`.
            if !keyword
                && (self.peek_at(1).is_punct(";")
                    || self.peek_at(1).is_ident("with")
                    || self.peek_at(1).is_punct("::"))
            {
                let action = self.parse_action_ref()?;
                return Ok(Some(self.parse_traversal_tail(action)?));
            }
        }
        if self.at_ident("do") {
            self.bump();
            let action = self.parse_action_ref()?;
            return Ok(Some(self.parse_traversal_tail(action)?));
        }
        if self.at_ident("sequence") {
            self.bump();
            let body = self.parse_activity_block()?;
            return Ok(Some(ActivityStmt::Atomic(body)));
        }
        if self.at_ident("replicate") {
            self.bump();
            self.expect_punct("(")?;
            let mut index = None;
            if matches!(self.peek().kind, TokKind::Ident(_)) && self.peek_at(1).is_punct(":") {
                index = Some(self.expect_ident()?);
                self.bump();
            }
            let n = self.parse_expr(0)?;
            self.expect_punct(")")?;
            let body = self.parse_activity_body()?;
            return Ok(Some(ActivityStmt::Repeat { count: n, index, body }));
        }
        if self.at_ident("parallel") {
            self.bump();
            self.skip_join_spec();
            let b = self.parse_activity_branches()?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::Parallel(b)));
        }
        if self.at_ident("schedule") {
            self.bump();
            self.skip_join_spec();
            let b = self.parse_activity_branches()?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::Schedule(b)));
        }
        if self.at_ident("select") {
            self.bump();
            self.expect_punct("{")?;
            let mut branches: Vec<(Option<Expr>, Activity)> = Vec::new();
            while !self.at_punct("}") && !self.at_eof() {
                if self.eat_punct(";") {
                    continue;
                }
                let mut guard = None;
                if self.at_punct("(") {
                    self.bump();
                    guard = Some(self.parse_expr(0)?);
                    self.expect_punct(")")?;
                }
                if self.at_punct("[") {
                    self.bump();
                    let _ = self.parse_expr(0)?;
                    self.expect_punct("]")?;
                }
                self.eat_punct(":");
                let body = self.parse_activity_body()?;
                branches.push((guard, body));
            }
            self.expect_punct("}")?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::Select(branches)));
        }
        if self.at_ident("match") {
            self.bump();
            self.expect_punct("(")?;
            let subject = self.parse_expr(0)?;
            self.expect_punct(")")?;
            self.expect_punct("{")?;
            let mut choices: Vec<(Option<Expr>, Activity)> = Vec::new();
            while !self.at_punct("}") && !self.at_eof() {
                if self.eat_punct(";") {
                    continue;
                }
                if self.eat_ident("default") {
                    self.expect_punct(":")?;
                    let body = self.parse_activity_body()?;
                    choices.push((None, body));
                    continue;
                }
                let range;
                if self.at_punct("[") {
                    // `[ open_range_list ]`: values and/or `lo..hi` ranges.
                    self.bump();
                    let mut items = Vec::new();
                    loop {
                        let lo = self.parse_expr(0)?;
                        if self.eat_punct("..") {
                            let hi = if self.at_punct("]") || self.at_punct(",") {
                                Expr::Int(i64::MAX)
                            } else {
                                self.parse_expr(0)?
                            };
                            items.push(Expr::InRange(
                                Box::new(Expr::Int(0)),
                                Box::new(lo),
                                Box::new(hi),
                            ));
                        } else {
                            items.push(lo);
                        }
                        if !self.eat_punct(",") {
                            break;
                        }
                    }
                    self.expect_punct("]")?;
                    range = Some(if items.len() == 1 {
                        items.into_iter().next().unwrap()
                    } else {
                        Expr::List(items)
                    });
                } else {
                    range = Some(self.parse_expr(0)?);
                }
                self.expect_punct(":")?;
                let body = self.parse_activity_body()?;
                choices.push((range, body));
            }
            self.expect_punct("}")?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::Match {
                expr: subject,
                choices,
            }));
        }
        if self.at_ident("constraint") {
            self.bump();
            let e = self.parse_constraint_set()?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::Constraint(e)));
        }
        if self.at_ident("super") {
            self.bump();
            self.eat_punct(";");
            return Ok(None);
        }
        if self.at_ident("atomic") {
            self.bump();
            let body = self.parse_activity_block()?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::Atomic(body)));
        }
        if self.at_ident("repeat") {
            self.bump();
            self.expect_punct("(")?;
            // Optional `index_identifier :`
            let mut index = None;
            if matches!(self.peek().kind, TokKind::Ident(_)) && self.peek_at(1).is_punct(":") {
                index = Some(self.expect_ident()?);
                self.bump();
            }
            let n = self.parse_expr(0)?;
            self.expect_punct(")")?;
            let body = self.parse_activity_body()?;
            // `repeat stmt while (expr);` form.
            if self.eat_ident("while") {
                self.skip_parens();
                self.eat_punct(";");
            }
            return Ok(Some(ActivityStmt::Repeat { count: n, index, body }));
        }
        if self.at_ident("foreach") {
            self.bump();
            self.expect_punct("(")?;
            let mut var = String::new();
            if matches!(self.peek().kind, TokKind::Ident(_))
                && (self.peek_at(1).is_punct(":") || self.peek_at(1).is_ident("in"))
            {
                var = self.expect_ident()?;
                self.bump(); // ':' or 'in'
            }
            let iter = self.parse_expr(0)?;
            if self.at_punct("[") {
                self.bump();
                let _ = self.parse_expr(0)?;
                self.expect_punct("]")?;
            }
            self.expect_punct(")")?;
            let body = self.parse_activity_body()?;
            return Ok(Some(ActivityStmt::Foreach { var, iter, body }));
        }
        if self.at_ident("if") {
            self.bump();
            self.expect_punct("(")?;
            let cond = self.parse_expr(0)?;
            self.expect_punct(")")?;
            let then = self.parse_activity_body()?;
            let els = if self.eat_ident("else") {
                Some(self.parse_activity_body()?)
            } else {
                None
            };
            return Ok(Some(ActivityStmt::If { cond, then, els }));
        }
        if self.at_punct("{") {
            let a = self.parse_activity_block()?;
            self.eat_punct(";");
            // Flatten a nested sequence into the parent.
            let Activity::Sequence(s) = a;
            if s.len() == 1 {
                return Ok(Some(s.into_iter().next().unwrap()));
            }
            return Ok(Some(ActivityStmt::Atomic(Activity::Sequence(s))));
        }
        if self.at_ident("while") || self.at_ident("foreach") || self.at_ident("async") {
            self.bump();
            self.skip_braced_or_semicolon();
            return Ok(None);
        }
        // Unknown statement: skip to the next `;` or block.
        self.skip_braced_or_semicolon();
        Ok(None)
    }

    /// Parse what follows an action reference in a traversal: an inline
    /// constraint set (`with { .. }`), an inline activity body, or `;`.
    fn parse_traversal_tail(&mut self, action: Path) -> PssResult<ActivityStmt> {
        if self.eat_ident("with") {
            self.expect_punct("{")?;
            let mut constraints = Vec::new();
            while !self.at_punct("}") && !self.at_eof() {
                if self.eat_punct(";") {
                    continue;
                }
                constraints.push(self.parse_constraint_body_item()?);
                self.eat_punct(";");
            }
            self.expect_punct("}")?;
            self.eat_punct(";");
            return Ok(ActivityStmt::DoConstrained { action, constraints });
        }
        if self.at_punct("{") {
            let body = self.parse_activity_block()?;
            return Ok(ActivityStmt::DoWith {
                action,
                body: Some(body),
            });
        }
        self.eat_punct(";");
        Ok(ActivityStmt::Do(action))
    }

    /// Parse an action reference, allowing `Component::action` qualification.
    fn parse_action_ref(&mut self) -> PssResult<Path> {
        let mut segments = vec![self.expect_ident()?];
        while self.at_punct("::") {
            self.bump();
            segments.push(self.expect_ident()?);
        }
        Ok(Path::new(segments))
    }

    // ---- static const --------------------------------------------------

    fn try_parse_static_const(&mut self) -> PssResult<Option<Item>> {
        // `static const int NAME = value;`
        let save = self.pos;
        self.eat_ident("static");
        if !self.eat_ident("const") {
            self.pos = save;
            return Ok(None);
        }
        let _ty = self.parse_type_ref()?;
        let name = match &self.peek().kind {
            TokKind::Ident(_) => self.expect_ident()?,
            _ => {
                self.pos = save;
                return Ok(None);
            }
        };
        let value = if self.eat_punct("=") {
            self.parse_expr(0)?
        } else {
            Expr::Int(0)
        };
        self.expect_punct(";")?;
        Ok(Some(Item::StaticConst { name, value }))
    }

    // ---- compile if ----------------------------------------------------

    fn capture_compile_cond(&mut self) -> String {
        // Consume up to the `{` that opens the branch body.
        let mut parts = Vec::new();
        while !self.at_eof() && !self.at_punct("{") {
            parts.push(token_text(&self.bump()));
        }
        parts.join(" ")
    }

    fn parse_compile_if(&mut self) -> PssResult<Item> {
        self.expect_ident_named("compile")?;
        if self.eat_ident("assert") {
            // `compile assert ( expression ) ;`
            if self.at_punct("(") {
                let _ = self.capture_paren_cond();
            }
            self.eat_punct(";");
            return Ok(Item::Raw("compile assert".into()));
        }
        self.expect_ident_named("if")?;
        let cond = if self.at_punct("(") {
            self.capture_paren_cond()
        } else {
            String::new()
        };
        let then_items = self.parse_braced_items()?;
        let else_items = if self.eat_ident("else") {
            self.parse_braced_items()?
        } else {
            Vec::new()
        };
        self.eat_punct(";");
        Ok(Item::CompileIf {
            cond,
            then_items,
            else_items,
        })
    }

    fn capture_paren_cond(&mut self) -> String {
        self.expect_punct("(").ok();
        let mut depth = 1i32;
        let mut parts = Vec::new();
        while !self.at_eof() && depth > 0 {
            if self.at_punct("(") {
                depth += 1;
            } else if self.at_punct(")") {
                depth -= 1;
                if depth == 0 {
                    self.bump();
                    break;
                }
            }
            parts.push(token_text(&self.bump()));
        }
        parts.join(" ")
    }

    fn parse_braced_items(&mut self) -> PssResult<Vec<Item>> {
        self.expect_punct("{")?;
        let items = self.parse_items()?;
        self.expect_punct("}")?;
        Ok(items)
    }

    // ---- extend --------------------------------------------------------

    fn parse_extend(&mut self) -> PssResult<Item> {
        self.expect_ident_named("extend")?;
        if self.at_ident("component") {
            return Ok(Item::Component(self.parse_component(true)?));
        }
        if self.at_ident("enum") {
            return Ok(Item::Enum(self.parse_enum(true, None)?));
        }
        if self.at_ident("struct") || self.at_ident("resource") || self.at_ident("buffer") || self.at_ident("state") {
            return Ok(Item::Struct(self.parse_type_decl(true)?));
        }
        if self.at_ident("action") || self.at_ident("abstract") {
            return Ok(Item::Action(self.parse_action(true)?));
        }
        self.skip_to_semicolon();
        Ok(Item::Raw("extend".into()))
    }

    // ---- expressions ---------------------------------------------------

    pub fn parse_expr(&mut self, min_prec: u8) -> PssResult<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match &self.peek().kind {
                TokKind::Punct(p) => p.clone(),
                TokKind::Ident(id) if id == "in" || id == "inside" => id.clone(),
                _ => break,
            };
            let prec = match bin_prec(&op) {
                Some(p) => p,
                None => break,
            };
            if prec < min_prec {
                break;
            }
            if op == "in" || op == "inside" {
                self.bump();
                let rhs = self.parse_range_or_set()?;
                lhs = rhs_into_in(lhs, rhs);
                continue;
            }
            self.bump();
            let rhs = self.parse_expr(prec + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        // Ternary (lowest precedence, right-associative).
        if min_prec == 0 && self.at_punct("?") {
            self.bump();
            let t = self.parse_expr(0)?;
            self.expect_punct(":")?;
            let e = self.parse_expr(0)?;
            lhs = Expr::Ternary(Box::new(lhs), Box::new(t), Box::new(e));
        }
        Ok(lhs)
    }

    fn parse_range_or_set(&mut self) -> PssResult<Expr> {
        if self.at_punct("[") {
            self.bump();
            // `[..hi]` — open at the bottom.
            if self.eat_punct("..") {
                let hi = self.parse_expr(0)?;
                self.expect_punct("]")?;
                return Ok(Expr::InRange(
                    Box::new(Expr::Int(0)),
                    Box::new(Expr::Int(i64::MIN)),
                    Box::new(hi),
                ));
            }
            let lo = self.parse_expr(0)?;
            if self.eat_punct("..") {
                // `[lo..]` — open at the top.
                let hi = if self.at_punct("]") {
                    Expr::Int(i64::MAX)
                } else {
                    self.parse_expr(0)?
                };
                self.expect_punct("]")?;
                return Ok(Expr::InRange(Box::new(Expr::Int(0)), Box::new(lo), Box::new(hi)));
            }
            self.expect_punct("]")?;
            return Ok(Expr::List(vec![lo]));
        }
        if self.at_punct("{") {
            self.bump();
            let mut items = Vec::new();
            while !self.at_punct("}") && !self.at_eof() {
                items.push(self.parse_expr(0)?);
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("}")?;
            return Ok(Expr::List(items));
        }
        Ok(self.parse_expr(0)?)
    }

    fn parse_unary(&mut self) -> PssResult<Expr> {
        if let TokKind::Punct(p) = &self.peek().kind {
            if p == "!" || p == "-" || p == "+" || p == "~" {
                let op = p.clone();
                self.bump();
                let e = self.parse_unary()?;
                return Ok(Expr::Unary(op, Box::new(e)));
            }
        }
        // Cast: `( type ) expr`
        if self.at_punct("(")
            && matches!(self.peek_at(1).kind, TokKind::Ident(_))
            && self.peek_at(2).is_punct(")")
            && starts_expr(self.peek_at(3))
        {
            self.bump(); // (
            let ty = self.expect_ident()?;
            self.expect_punct(")")?;
            let e = self.parse_unary()?;
            return Ok(Expr::Cast(ty, Box::new(e)));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> PssResult<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            if self.at_punct(".") {
                self.bump();
                let name = self.expect_ident()?;
                if self.at_punct("(") {
                    let args = self.parse_call_args()?;
                    e = Expr::Method(Box::new(e), name, args);
                } else if let Expr::Path(p) = e {
                    let mut segs = p.segments;
                    segs.push(name);
                    e = Expr::Path(Path::new(segs));
                } else {
                    e = Expr::Method(Box::new(e), name, Vec::new());
                }
                continue;
            }
            if self.at_punct("[") {
                self.bump();
                let first = self.parse_expr(0)?;
                if self.eat_punct(":") {
                    // Part-select: `identifier[msb:lsb]` (PSS 8.6.1).
                    let lsb = self.parse_expr(0)?;
                    self.expect_punct("]")?;
                    e = Expr::Slice(Box::new(e), Box::new(first), Box::new(lsb));
                    continue;
                }
                self.expect_punct("]")?;
                e = Expr::Index(Box::new(e), Box::new(first));
                continue;
            }
            if self.at_punct("(") {
                let args = self.parse_call_args()?;
                if let Expr::Path(p) = &e {
                    e = Expr::Call(p.segments.join("::"), args);
                } else {
                    e = Expr::Call("<expr>".into(), args);
                }
                continue;
            }
            break;
        }
        Ok(e)
    }

    fn parse_call_args(&mut self) -> PssResult<Vec<Expr>> {
        self.expect_punct("(")?;
        let mut args = Vec::new();
        while !self.at_punct(")") && !self.at_eof() {
            args.push(self.parse_expr(0)?);
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct(")")?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> PssResult<Expr> {
        match self.peek().kind.clone() {
            TokKind::Int(v) => {
                self.bump();
                Ok(Expr::Int(v))
            }
            TokKind::Str(s) => {
                self.bump();
                Ok(Expr::Str(s))
            }
            TokKind::Ident(id) => {
                if id == "true" {
                    self.bump();
                    return Ok(Expr::Bool(true));
                }
                if id == "false" {
                    self.bump();
                    return Ok(Expr::Bool(false));
                }
                if id == "foreach" {
                    self.bump();
                    return self.parse_foreach_expr();
                }
                self.bump();
                Ok(Expr::Path(Path::simple(id)))
            }
            TokKind::Punct(p) if p == "'" => {
                // `'{ ... }` aggregate (assignment-pattern) literal.
                self.bump();
                if self.at_punct("{") {
                    self.bump();
                    let mut items = Vec::new();
                    while !self.at_punct("}") && !self.at_eof() {
                        items.push(self.parse_expr(0)?);
                        if !self.eat_punct(",") {
                            break;
                        }
                    }
                    self.expect_punct("}")?;
                    return Ok(Expr::List(items));
                }
                Ok(Expr::Unknown("'".into()))
            }
            TokKind::Punct(p) if p == "(" => {
                self.bump();
                let e = self.parse_expr(0)?;
                self.expect_punct(")")?;
                Ok(e)
            }
            TokKind::Punct(p) if p == "{" => {
                // A constraint block: `{ item; item; }` — conjoined.
                self.bump();
                let mut exprs = Vec::new();
                while !self.at_punct("}") && !self.at_eof() {
                    if self.eat_punct(";") {
                        continue;
                    }
                    exprs.push(self.parse_constraint_body_item()?);
                    self.eat_punct(";");
                }
                self.expect_punct("}")?;
                Ok(combine_and(exprs))
            }
            _ => {
                let t = self.bump();
                Ok(Expr::Unknown(token_text(&t)))
            }
        }
    }

    fn parse_foreach_expr(&mut self) -> PssResult<Expr> {
        self.expect_punct("(")?;
        let mut vars = Vec::new();
        // Optional `iterator_identifier :`
        if matches!(self.peek().kind, TokKind::Ident(_))
            && (self.peek_at(1).is_punct(":") || self.peek_at(1).is_ident("in"))
        {
            vars.push(self.expect_ident()?);
            self.bump(); // ':' or 'in'
        }
        let iter = self.parse_expr(0)?;
        // Optional `[index_identifier]`
        if self.at_punct("[") {
            self.bump();
            let _ = self.parse_expr(0)?;
            self.expect_punct("]")?;
        }
        self.expect_punct(")")?;
        let body = self.parse_constraint_set()?;
        Ok(Expr::Foreach {
            vars,
            iter: Box::new(iter),
            body: Box::new(body),
        })
    }
}

// ---------------------------------------------------------------------------
// free helpers
// ---------------------------------------------------------------------------

fn bin_prec(op: &str) -> Option<u8> {
    Some(match op {
        "->" => 1,
        "||" => 2,
        "&&" => 3,
        "|" => 4,
        "^" => 5,
        "&" => 6,
        "==" | "!=" => 7,
        "<" | "<=" | ">" | ">=" | "in" | "inside" => 8,
        "<<" | ">>" => 9,
        "+" | "-" => 10,
        "*" | "/" | "%" => 11,
        _ => return None,
    })
}

fn rhs_into_in(lhs: Expr, rhs: Expr) -> Expr {
    match rhs {
        Expr::InRange(_, lo, hi) => Expr::InRange(Box::new(lhs), lo, hi),
        Expr::List(items) => Expr::InSet(Box::new(lhs), items),
        other => Expr::Binary("in".into(), Box::new(lhs), Box::new(other)),
    }
}

fn starts_expr(t: &Token) -> bool {
    match &t.kind {
        TokKind::Ident(_) | TokKind::Int(_) | TokKind::Str(_) => true,
        TokKind::Punct(p) => p == "(" || p == "!" || p == "-",
        _ => false,
    }
}

fn combine_and(exprs: Vec<Expr>) -> Expr {
    let mut it = exprs.into_iter();
    match it.next() {
        None => Expr::Bool(true),
        Some(first) => it.fold(first, |acc, e| Expr::Binary("&&".into(), Box::new(acc), Box::new(e))),
    }
}

fn token_text(t: &Token) -> String {
    match &t.kind {
        TokKind::Ident(s) => s.clone(),
        TokKind::Int(v) => v.to_string(),
        TokKind::Str(s) => format!("\"{}\"", s),
        TokKind::Punct(p) => p.clone(),
        TokKind::Eof => String::new(),
    }
}

fn describe(t: &Token) -> String {
    describe_kind(&t.kind)
}

fn describe_kind(k: &TokKind) -> String {
    match k {
        TokKind::Ident(s) => format!("`{}`", s),
        TokKind::Int(v) => format!("`{}`", v),
        TokKind::Str(s) => format!("\"{}\"", s),
        TokKind::Punct(p) => format!("`{}`", p),
        TokKind::Eof => "end of input".to_string(),
    }
}

/// Parse a complete source string into an AST.
pub fn parse_source(src: &str) -> PssResult<SourceFile> {
    let toks = tokenize(src)?;
    let mut p = Parser::new(toks);
    p.parse_file()
}

/// Parse a single expression (used by the JSON frontend for constraint
/// strings). The whole input must be consumed.
pub fn parse_expression(src: &str) -> PssResult<Expr> {
    let toks = tokenize(src)?;
    let mut p = Parser::new(toks);
    let e = p.parse_expr(0)?;
    if !p.at_eof() {
        return Err(PssError::new(
            format!("trailing tokens after expression in `{}`", src),
            p.peek().line,
            p.peek().col,
        ));
    }
    Ok(e)
}

/// Parse a sequence of `exec`-body statements from captured tokens.
fn parse_exec_stmts(toks: &[Token]) -> Vec<ExecStmt> {
    let mut t: Vec<Token> = toks.to_vec();
    t.push(Token {
        kind: TokKind::Eof,
        line: 0,
        col: 0,
    });
    let mut p = Parser::new(t);
    let mut out = Vec::new();
    let mut guard = 0;
    while !p.at_eof() && guard < 10_000 {
        guard += 1;
        if p.eat_punct(";") {
            continue;
        }
        let before = p.pos;
        match p.try_parse_exec_stmt() {
            Some(st) => out.push(st),
            None => {
                if p.pos == before {
                    // Nothing consumed: skip this statement to its `;`.
                    let start = p.bump();
                    p.skip_to_semicolon();
                    out.push(ExecStmt::Raw(token_text(&start)));
                } else {
                    // The statement was consumed but not understood.
                    out.push(ExecStmt::Raw("<unparsed>".into()));
                }
            }
        }
    }
    out
}

/// Activity keywords that can begin a statement (used to tell a labelled
/// statement or a bare action traversal from a keyword).
fn is_activity_keyword(id: &str) -> bool {
    matches!(
        id,
        "do"
            | "parallel"
            | "schedule"
            | "select"
            | "repeat"
            | "foreach"
            | "if"
            | "else"
            | "atomic"
            | "sequence"
            | "replicate"
            | "match"
            | "while"
            | "async"
            | "random"
    )
}
