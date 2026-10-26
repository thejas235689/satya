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
    /// Covergroups declared so far (PSS 18).
    covergroups: Vec<Covergroup>,
    /// Monitors declared so far (PSS 19.1).
    monitors: Vec<Monitor>,
}

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser {
            toks,
            pos: 0,
            covergroups: Vec::new(),
            monitors: Vec::new(),
        }
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
        let items = self.parse_items(true)?;
        // `parse_items` stops at a `}`; at file level that means an unmatched
        // one, which used to be ignored silently.
        if !self.at_eof() {
            let (line, col) = (self.peek().line, self.peek().col);
            return Err(PssError::new(
                format!("unmatched {} at end of file", describe(self.peek())),
                line,
                col,
            ));
        }
        // `parse_items` stops at a `}`; at file level that means an unmatched
        // one, which used to be ignored silently.
        if !self.at_eof() {
            let (line, col) = (self.peek().line, self.peek().col);
            return Err(PssError::new(
                format!("unmatched {} at end of file", describe(self.peek())),
                line,
                col,
            ));
        }
        Ok(SourceFile {
            items,
            covergroups: std::mem::take(&mut self.covergroups),
            monitors: std::mem::take(&mut self.monitors),
        })
    }

    /// `covergroup NAME (ports) { items }` or the in-line
    /// `covergroup { items } NAME;` (PSS 18.1, Syntax 63).
    fn parse_covergroup(&mut self) -> PssResult<Covergroup> {
        self.expect_ident_named("covergroup")?;
        let mut name = String::new();
        if matches!(self.peek().kind, TokKind::Ident(_)) && !self.peek_at(1).is_punct("{") {
            name = self.expect_ident()?;
            if self.at_punct("(") {
                self.skip_parens();
            }
        }
        self.expect_punct("{")?;
        let mut items = Vec::new();
        while !self.at_punct("}") && !self.at_eof() {
            if self.eat_punct(";") {
                continue;
            }
            let label = if matches!(self.peek().kind, TokKind::Ident(_)) && self.peek_at(1).is_punct(":")
            {
                let l = self.expect_ident()?;
                self.bump();
                Some(l)
            } else {
                None
            };
            if self.at_ident("coverpoint") {
                self.bump();
                let expr = self.parse_expr(0)?;
                let mut label = label;
                let mut iff = None;
                if self.eat_ident("iff") {
                    self.expect_punct("(")?;
                    iff = Some(self.parse_expr(0)?);
                    self.expect_punct(")")?;
                }
                let mut bins = Vec::new();
                if self.at_punct("{") {
                    self.bump();
                    while !self.at_punct("}") && !self.at_eof() {
                        if self.eat_punct(";") {
                            continue;
                        }
                        // `label <ident>;` names the coverpoint, which is what
                        // a cross refers to (PSS 18.3).
                        if self.at_ident("label") {
                            self.bump();
                            label = Some(self.expect_ident()?);
                            self.eat_punct(";");
                            continue;
                        }
                        let is_bins = self.at_ident("bins");
                        if is_bins || self.at_ident("illegal_bins") || self.at_ident("ignore_bins") {
                            self.bump();
                            let bn = if matches!(self.peek().kind, TokKind::Ident(_)) {
                                self.expect_ident()?
                            } else {
                                String::new()
                            };
                            let bexpr = if self.eat_punct("=") {
                                self.parse_expr(0)?
                            } else {
                                Expr::Bool(true)
                            };
                            self.eat_punct(";");
                            if is_bins {
                                bins.push((bn, bexpr));
                            }
                            continue;
                        }
                        self.skip_to_semicolon();
                    }
                    self.expect_punct("}")?;
                }
                self.eat_punct(";");
                items.push(CoverItem::Coverpoint {
                    label,
                    expr,
                    iff,
                    bins,
                });
                continue;
            }
            if self.at_ident("cross") {
                self.bump();
                let mut refs = Vec::new();
                loop {
                    refs.push(self.expect_ident()?);
                    if !self.eat_punct(",") {
                        break;
                    }
                }
                let mut iff = None;
                if self.eat_ident("iff") {
                    self.expect_punct("(")?;
                    iff = Some(self.parse_expr(0)?);
                    self.expect_punct(")")?;
                }
                let mut label = label;
                if self.at_punct("{") {
                    // `cross a, b { label X; }` — the cross's own label.
                    self.bump();
                    while !self.at_punct("}") && !self.at_eof() {
                        if self.eat_punct(";") {
                            continue;
                        }
                        if self.at_ident("label") {
                            self.bump();
                            label = Some(self.expect_ident()?);
                            self.eat_punct(";");
                            continue;
                        }
                        self.skip_to_semicolon();
                    }
                    self.expect_punct("}")?;
                }
                self.eat_punct(";");
                items.push(CoverItem::Cross { label, refs, iff });
                continue;
            }
            self.skip_to_semicolon();
        }
        self.expect_punct("}")?;
        if name.is_empty() && matches!(self.peek().kind, TokKind::Ident(_)) {
            name = self.expect_ident()?;
        }
        self.eat_punct(";");
        Ok(Covergroup { name, items })
    }

    /// Item list. `top` is true only for the file level, where an
    /// unrecognized construct is an error: a file of foreign syntax must not
    /// turn into a successful parse of an empty model. Inside a package or
    /// component body the older lenient skip is kept, since a construct this
    /// parser does not yet handle there should not make a whole model
    /// unloadable.
    fn parse_items(&mut self, top: bool) -> PssResult<Vec<Item>> {
        let mut items = Vec::new();
        while !self.at_eof() && !self.at_punct("}") {
            if self.eat_punct(";") {
                continue;
            }
            let item = self.parse_item(top)?;
            items.push(item);
        }
        Ok(items)
    }

    fn parse_item(&mut self, top: bool) -> PssResult<Item> {
        // `pure component` / `pure action`.
        if self.at_ident("pure") {
            self.bump();
            return self.parse_item(top);
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
        if !top {
            // Inside a body: skip a chunk defensively and carry on.
            let start = self.bump();
            self.skip_to_semicolon();
            return Ok(Item::Raw(token_text(&start)));
        }
        // At the file level an unrecognized construct is an error. Silently
        // skipping it used to turn a file of foreign syntax (an InFact
        // `.rules` file, say) into a successful parse of an empty model, which
        // is worse than useless: it reports success for input that was never
        // PSS.
        let (line, col) = (self.peek().line, self.peek().col);
        Err(PssError::new(
            format!(
                "unexpected {} at top level; expected a package, component, \
                 action, struct, enum, import or extend declaration",
                describe(self.peek())
            ),
            line,
            col,
        ))
    }

    // ---- imports / packages -------------------------------------------

    fn parse_import(&mut self) -> PssResult<Path> {
        self.expect_ident_named("import")?;
        // `import [solve|target] [C|SV] function ...` — an imported function
        // declaration (PSS 22.2), not a package import. Look ahead so a package
        // legitimately named like a qualifier is not swallowed.
        let mut look = 0usize;
        if self.peek().is_ident("solve") || self.peek().is_ident("target") {
            look += 1;
        }
        if self.peek_at(look).is_ident("C")
            || self.peek_at(look).is_ident("SV")
            || self.peek_at(look).is_ident("SystemVerilog")
        {
            look += 1;
        }
        if self.peek_at(look).is_ident("static") {
            look += 1;
        }
        if self.peek_at(look).is_ident("function") || self.peek_at(look).is_ident("task") {
            for _ in 0..=look {
                self.bump();
            }
            self.skip_to_semicolon();
            self.eat_punct(";");
            return Ok(Path::simple("__imported_function"));
        }
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
        // A package name may be qualified: `package pkg1::a::b::c { ... }`.
        let name = self.parse_scoped_path()?.segments.join("::");
        self.expect_punct("{")?;
        let items = self.parse_items(false)?;
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
            if self.at_ident("covergroup") {
                let cg = self.parse_covergroup()?;
                self.covergroups.push(cg);
                continue;
            }
            if self.at_ident("monitor") {
                let m = self.parse_monitor()?;
                self.monitors.push(m);
                continue;
            }
            if self.at_ident("function") || self.at_ident("task") {
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
                const_args: Vec::new(),
                width,
                open_template: false,
            });
        }

        let path = self.parse_scoped_path()?;
        let mut template_args = Vec::new();
        let mut const_args: Vec<Expr> = Vec::new();
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
                        // Above *every* operator that could swallow the closing
                        // `>`: comparison is 8, but `<<`/`>>` are 9, so a
                        // constant argument in `nested_s<packed_s<bit, 8>>`
                        // would otherwise parse as `8 >> ...`.
                        const_args.push(self.parse_expr(10)?);
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
            const_args,
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
        let mut is_disable = false;
        if self.eat_ident("default") {
            is_default = true;
            // `constraint default disable <hierarchical_id>;`
            if self.eat_ident("disable") {
                is_disable = true;
            }
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
            is_disable,
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
                // `default <attr> == k;` and `default disable <attr>;` may
                // appear as items of a constraint block (LRM Example 134).
                if self.at_ident("default")
                    && !(self.peek_at(1).is_punct("=") || self.peek_at(1).is_punct("."))
                {
                    self.bump();
                    let disable = self.eat_ident("disable");
                    let target = self.parse_constraint_body_item()?;
                    self.eat_punct(";");
                    let name = if disable { "default_disable" } else { "default" };
                    exprs.push(Expr::Call(name.to_string(), vec![target]));
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
        // `foreach (a[i]) { body }` in a constraint (PSS 16.1.6). The solver
        // expands it into one constraint per element; without this branch the
        // whole directive was parsed as junk and dropped.
        if self.at_ident("foreach") {
            self.bump();
            self.expect_punct("(")?;
            // `foreach (x : coll)` binds `x` to each *element*; `foreach (a[i])`
            // binds the variable to the *index*. Both are in use.
            let mut bound = false;
            let mut bound_var = None;
            if matches!(self.peek().kind, TokKind::Ident(_))
                && (self.peek_at(1).is_punct(":") || self.peek_at(1).is_ident("in"))
            {
                bound_var = Some(self.expect_ident()?);
                self.bump(); // `:` or `in`
                bound = true;
            }
            let head = self.parse_expr(0)?;
            self.expect_punct(")")?;
            let (loop_vars, iter) = match (bound_var, head) {
                (Some(v), coll) => (vec![v], coll),
                (None, Expr::Index(b, i)) => match (*b, *i) {
                    (Expr::Path(bp), Expr::Path(ip)) => {
                        (vec![ip.leaf().to_string()], Expr::Path(bp))
                    }
                    (b, i) => (Vec::new(), Expr::Index(Box::new(b), Box::new(i))),
                },
                (None, other) => (Vec::new(), other),
            };
            let body = if self.at_punct("{") {
                self.parse_constraint_set()?
            } else {
                self.parse_constraint_body_item()?
            };
            self.eat_punct(";");
            return Ok(Expr::Foreach {
                vars: loop_vars,
                iter: Box::new(iter),
                body: Box::new(body),
                bound,
            });
        }
        // `dist <expr> in [ item [:= w | :/ w], ... ] ;` (PSS 16.1.9).
        if self.at_ident("dist") {
            self.bump();
            // Above `in`'s precedence, so the target stops before the keyword.
            let target = self.parse_expr(9)?;
            self.expect_ident_named("in")?;
            let close = "[";
            let items = self.parse_dist_items(close)?;
            let mut args = vec![target];
            args.extend(items);
            return Ok(Expr::Call("__dist".into(), args));
        }
        // `solve a, b before c, d ;` (PSS 16.1.8).
        if self.at_ident("solve") && !self.peek_at(1).is_punct("(") {
            self.bump();
            let mut before = Vec::new();
            let mut after = Vec::new();
            let mut cur = &mut before;
            loop {
                let p = self.parse_scoped_path()?;
                cur.push(Expr::Path(p));
                if self.eat_punct(",") {
                    continue;
                }
                if self.eat_ident("before") {
                    cur = &mut after;
                    continue;
                }
                break;
            }
            return Ok(Expr::Call(
                "__solve_before".into(),
                vec![Expr::List(before), Expr::List(after)],
            ));
        }
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
        let e = self.parse_expr(0)?;
        // The SystemVerilog spelling, `<expr> dist { item := w, ... }`, which
        // the shared pss-corpus uses. The standard's form is
        // `dist <expr> in [ ... ]` (16.1.9); this one is accepted as well so
        // models written in either style load. See docs/SEMANTICS.md §3.
        if self.at_ident("dist") && (self.peek_at(1).is_punct("{") || self.peek_at(1).is_punct("[")) {
            self.bump();
            let close = if self.peek().is_punct("{") { "{" } else { "[" };
            let items = self.parse_dist_items(close)?;
            let mut args = vec![e];
            args.extend(items);
            return Ok(Expr::Call("__dist".into(), args));
        }
        Ok(e)
    }

    /// The items of a distribution list, up to the matching `]` or `}`.
    fn parse_dist_items(&mut self, close: &str) -> PssResult<Vec<Expr>> {
        self.expect_punct(close)?;
        let mut items: Vec<Expr> = Vec::new();
        loop {
            if self.at_punct(if close == "[" { "]" } else { "}" }) {
                break;
            }
            let lo = self.parse_expr(0)?;
            // A `dist_item` is a single value or a `lo..hi` range.
            let range = if self.eat_punct("..") {
                let hi = self.parse_expr(0)?;
                Expr::InRange(Box::new(Expr::Int(0)), Box::new(lo), Box::new(hi))
            } else {
                lo
            };
            let mut weight = Expr::Int(1);
            let mut per_value = true;
            // `:=` gives each value in a range the weight; `:/` distributes it.
            if self.at_punct(":") && (self.peek_at(1).is_punct("=") || self.peek_at(1).is_punct("/"))
            {
                self.bump();
                per_value = self.peek().is_punct("=");
                self.bump();
                weight = self.parse_expr(0)?;
            }
            items.push(Expr::Call(
                "__dist_item".into(),
                vec![range, Expr::Bool(per_value), weight],
            ));
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct(if close == "[" { "]" } else { "}" })?;
        Ok(items)
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
        // Procedural statements (PSS Syntax 89-101).
        if self.at_ident("repeat") {
            self.bump();
            if self.at_punct("(") {
                self.bump();
                let count = self.parse_expr(0).ok()?;
                self.expect_punct(")").ok()?;
                let body = self.parse_exec_block();
                self.eat_punct(";");
                return Some(ExecStmt::Repeat { count, body });
            }
            let body = self.parse_exec_block();
            if self.eat_ident("while") {
                self.expect_punct("(").ok()?;
                let cond = self.parse_expr(0).ok()?;
                self.expect_punct(")").ok()?;
                self.eat_punct(";");
                return Some(ExecStmt::RepeatWhile { body, cond });
            }
            return Some(ExecStmt::Repeat {
                count: Expr::Int(1),
                body,
            });
        }
        if self.at_ident("foreach") {
            self.bump();
            self.expect_punct("(").ok()?;
            let mut var = String::new();
            if matches!(self.peek().kind, TokKind::Ident(_))
                && (self.peek_at(1).is_punct(":") || self.peek_at(1).is_ident("in"))
            {
                var = self.expect_ident().ok()?;
                self.bump();
            }
            let mut iter = self.parse_expr(0).ok()?;
            if var.is_empty() {
                if let Expr::Index(base, idx) = &iter {
                    if let (Expr::Path(_), Expr::Path(ip)) = (&**base, &**idx) {
                        var = ip.leaf();
                        iter = (**base).clone();
                    }
                }
            }
            self.expect_punct(")").ok()?;
            let body = self.parse_exec_block();
            self.eat_punct(";");
            return Some(ExecStmt::Foreach { var, iter, body });
        }
        if self.at_ident("match") {
            self.bump();
            self.expect_punct("(").ok()?;
            let subject = self.parse_expr(0).ok()?;
            self.expect_punct(")").ok()?;
            self.expect_punct("{").ok()?;
            let mut arms = Vec::new();
            while !self.at_punct("}") && !self.at_eof() {
                if self.eat_punct(";") {
                    continue;
                }
                let guard = if self.eat_ident("default") {
                    None
                } else {
                    let g = self.parse_expr(0).ok()?;
                    Some(g)
                };
                self.expect_punct(":").ok()?;
                let body = self.parse_exec_block();
                arms.push((guard, body));
            }
            self.expect_punct("}").ok()?;
            self.eat_punct(";");
            return Some(ExecStmt::Match { subject, arms });
        }
        if self.at_ident("break") {
            self.bump();
            self.eat_punct(";");
            return Some(ExecStmt::Break);
        }
        if self.at_ident("continue") {
            self.bump();
            self.eat_punct(";");
            return Some(ExecStmt::Continue);
        }
        if self.at_ident("return") {
            self.bump();
            let e = if self.at_punct(";") {
                None
            } else {
                self.parse_expr(0).ok()
            };
            self.eat_punct(";");
            return Some(ExecStmt::Return(e));
        }
        if self.at_ident("randomize") {
            self.bump();
            self.expect_punct("(").ok()?;
            let mut targets = Vec::new();
            while !self.at_punct(")") && !self.at_eof() {
                if let Ok(Expr::Path(p)) = self.parse_expr(0) {
                    targets.push(p);
                } else {
                    break;
                }
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct(")").ok()?;
            self.eat_punct(";");
            return Some(ExecStmt::Randomize(targets));
        }
        if self.at_ident("yield") {
            self.bump();
            self.eat_punct(";");
            return Some(ExecStmt::Yield);
        }
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
            match self.parse_expr(0) {
                Ok(rhs) => {
                    self.eat_punct(";");
                    if let Expr::Path(path) = e {
                        return Some(ExecStmt::Assign(path, rhs));
                    }
                }
                Err(er) => {
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
        if self.at_ident("covergroup") {
            let cg = self.parse_covergroup()?;
            self.covergroups.push(cg);
            return Ok(CompItem::Raw("covergroup".into()));
        }
        // `override { type A with B; instance p with B; }` (PSS 20.5).
        if self.at_ident("override") && self.peek_at(1).is_punct("{") {
            self.bump();
            self.bump();
            let mut list = Vec::new();
            while !self.at_punct("}") && !self.at_eof() {
                if self.eat_punct(";") {
                    continue;
                }
                let is_instance = self.at_ident("instance");
                if self.at_ident("type") || is_instance {
                    self.bump();
                } else {
                    self.skip_braced_or_semicolon();
                    continue;
                }
                let from = if is_instance {
                    self.parse_scoped_path()?.segments.join(".")
                } else {
                    self.parse_scoped_path()?.leaf().to_string()
                };
                if self.eat_ident("with") {
                    let to = self.parse_scoped_path()?.leaf().to_string();
                    self.eat_punct(";");
                    list.push(OverrideDecl {
                        instance: if is_instance { Some(from.clone()) } else { None },
                        from: if is_instance { String::new() } else { from },
                        to,
                    });
                } else {
                    self.skip_to_semicolon();
                }
            }
            self.expect_punct("}")?;
            self.eat_punct(";");
            return Ok(CompItem::Override(list));
        }
        // `symbol NAME [(params)] { activity_stmts }` (PSS 12.7, Syntax 43).
        if self.at_ident("symbol") {
            self.bump();
            let name = self.expect_ident()?;
            if self.at_punct("(") {
                self.skip_parens();
            }
            let body = self.parse_activity_block()?;
            self.eat_punct(";");
            return Ok(CompItem::Symbol { name, body });
        }
        if self.at_ident("monitor") {
            let m = self.parse_monitor()?;
            self.monitors.push(m);
            return Ok(CompItem::Raw("monitor".into()));
        }
        if self.at_ident("function") || self.at_ident("task") {
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
            if self.at_ident("covergroup") {
                let cg = self.parse_covergroup()?;
                self.covergroups.push(cg);
                continue;
            }
            if self.at_ident("monitor") {
                let m = self.parse_monitor()?;
                self.monitors.push(m);
                continue;
            }
            if self.at_ident("function") || self.at_ident("task") {
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

    /// An optional fine-grained scheduling specifier after `parallel` or
    /// `schedule` (PSS 12.3.6.1, Syntax 34).
    fn parse_join_spec(&mut self) -> PssResult<JoinSpec> {
        if self.at_ident("join_all") {
            self.bump();
            return Ok(JoinSpec::All);
        }
        if self.at_ident("join_none") {
            self.bump();
            return Ok(JoinSpec::None);
        }
        if self.at_ident("join_branch") {
            self.bump();
            self.expect_punct("(")?;
            let mut labels = Vec::new();
            loop {
                labels.push(self.expect_ident()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct(")")?;
            return Ok(JoinSpec::Branch(labels));
        }
        if self.at_ident("join_select") {
            self.bump();
            self.expect_punct("(")?;
            let n = self.parse_expr(0)?;
            self.expect_punct(")")?;
            return Ok(JoinSpec::Select(n));
        }
        if self.at_ident("join_first") {
            self.bump();
            self.expect_punct("(")?;
            let n = self.parse_expr(0)?;
            self.expect_punct(")")?;
            return Ok(JoinSpec::First(n));
        }
        Ok(JoinSpec::All)
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
                let label = self.expect_ident()?;
                self.bump(); // ':'
                return Ok(self
                    .parse_activity_stmt()?
                    .map(|stmt| ActivityStmt::Labeled {
                        label,
                        stmt: Box::new(stmt),
                    }));
            }
            // `name();` / `name(args);` — a call to a `symbol` fragment
            // (PSS 12.7, `symbol_call`).
            if !keyword && self.peek_at(1).is_punct("(") {
                let name = self.expect_ident()?;
                self.expect_punct("(")?;
                let mut args = Vec::new();
                while !self.at_punct(")") && !self.at_eof() {
                    args.push(self.parse_expr(0)?);
                    if !self.eat_punct(",") {
                        break;
                    }
                }
                self.expect_punct(")")?;
                self.eat_punct(";");
                return Ok(Some(ActivityStmt::SymbolCall(name, args)));
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
            return Ok(Some(ActivityStmt::Repeat {
                count: n,
                index,
                body,
                is_replicate: true,
                while_cond: None,
            }));
        }
        if self.at_ident("parallel") {
            self.bump();
            let join = self.parse_join_spec()?;
            let branches = self.parse_activity_branches()?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::Parallel { branches, join }));
        }
        if self.at_ident("schedule") {
            self.bump();
            let join = self.parse_join_spec()?;
            let branches = self.parse_activity_branches()?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::Schedule { branches, join }));
        }
        if self.at_ident("monitor") {
            // A monitor declares a scenario to observe (PSS 19.1). Its
            // covergroups are collected; the scenario match itself is not
            // evaluated (see docs/SEMANTICS.md).
            self.bump();
            if matches!(self.peek().kind, TokKind::Ident(_)) {
                let _ = self.expect_ident()?;
            }
            if self.at_punct(":") {
                self.bump();
                let _ = self.parse_scoped_path()?;
            }
            if self.at_punct("{") {
                self.bump();
                while !self.at_punct("}") && !self.at_eof() {
                    if self.eat_punct(";") {
                        continue;
                    }
                    if self.at_ident("covergroup") {
                        let cg = self.parse_covergroup()?;
                        self.covergroups.push(cg);
                        continue;
                    }
                    self.skip_braced_or_semicolon();
                }
                self.expect_punct("}")?;
            }
            self.eat_punct(";");
            return Ok(None);
        }
        if self.at_ident("cover") {
            self.bump();
            if self.at_punct("{") {
                // `cover { <monitor body> }` — the body may declare an in-line
                // covergroup and/or an activity to observe.
                self.bump();
                let mut stmts: Vec<ActivityStmt> = Vec::new();
                while !self.at_punct("}") && !self.at_eof() {
                    if self.eat_punct(";") {
                        continue;
                    }
                    if self.at_ident("covergroup") {
                        let cg = self.parse_covergroup()?;
                        self.covergroups.push(cg);
                        continue;
                    }
                    if self.at_ident("activity") {
                        self.bump();
                        match self.parse_activity_block()? {
                            Activity::Sequence(v) => stmts.extend(v),
                        }
                        continue;
                    }
                    if self.at_ident("monitor") || self.at_ident("constraint") || self.at_ident("rand")
                    {
                        self.skip_braced_or_semicolon();
                        continue;
                    }
                    // The body of a `cover` block is an activity statement
                    // (PSS 19.2). Anything else here used to be skipped to the
                    // next `;`, which silently dropped `cover { do b; }`.
                    match self.parse_activity_stmt()? {
                        Some(st) => stmts.push(st),
                        None => {
                            if !self.at_punct("}") {
                                self.skip_to_semicolon();
                            }
                        }
                    }
                }
                self.expect_punct("}")?;
                self.eat_punct(";");
                let body = if stmts.is_empty() {
                    None
                } else {
                    Some(Activity::Sequence(stmts))
                };
                return Ok(Some(ActivityStmt::Cover { label: None, body, monitor: None }));
            }
            // `cover <monitor_type>;` — match the named monitor (PSS 19.2).
            let ty = self.parse_scoped_path()?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::Cover {
                monitor: Some(ty.leaf().to_string()),
                label: None,
                body: None,
            }));
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
        // `constraint parallel { a, b };` / `constraint sequence { a, b };`
        // schedules the named action handles (PSS 16.2, Syntax 62).
        if self.at_ident("constraint")
            && (self.peek_at(1).is_ident("parallel") || self.peek_at(1).is_ident("sequence"))
        {
            self.bump();
            let parallel = self.peek().is_ident("parallel");
            self.bump();
            self.expect_punct("{")?;
            let mut refs = Vec::new();
            while !self.at_punct("}") && !self.at_eof() {
                refs.push(self.parse_scoped_path()?.segments.join("."));
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("}")?;
            self.eat_punct(";");
            return Ok(Some(ActivityStmt::SchedConstraint { parallel, refs }));
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
            // `repeat { .. } while (cond);` (Syntax 37) has no count.
            if self.at_punct("{") {
                let body = self.parse_activity_body()?;
                let mut cond = None;
                if self.eat_ident("while") {
                    self.expect_punct("(")?;
                    cond = Some(self.parse_expr(0)?);
                    self.expect_punct(")")?;
                }
                self.eat_punct(";");
                return Ok(Some(ActivityStmt::Repeat {
                    count: Expr::Int(1),
                    index: None,
                    body,
                    is_replicate: false,
                    while_cond: cond,
                }));
            }
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
            // `repeat (n) stmt while (expr);` form.
            let mut while_cond = None;
            if self.eat_ident("while") {
                self.expect_punct("(")?;
                while_cond = Some(self.parse_expr(0)?);
                self.expect_punct(")")?;
                self.eat_punct(";");
            }
            return Ok(Some(ActivityStmt::Repeat {
                count: n,
                index,
                body,
                is_replicate: false,
                while_cond,
            }));
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
            let mut iter = self.parse_expr(0)?;
            if self.at_punct("[") {
                self.bump();
                let _ = self.parse_expr(0)?;
                self.expect_punct("]")?;
            }
            // `foreach (arr[i])`: the collection is `arr` and `i` is the loop
            // variable (PSS 12.2.6).
            if var.is_empty() {
                if let Expr::Index(base, idx) = &iter {
                    if let (Expr::Path(_), Expr::Path(ip)) = (&**base, &**idx) {
                        var = ip.leaf();
                        iter = (**base).clone();
                    }
                }
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
        let items = self.parse_items(false)?;
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
            // An `open_range_list`: a comma-separated list of scalar value
            // expressions *or* ranges, either end of which may be open
            // (`[..1, 8..16, 4294967296..]`, PSS 8.5.9).
            let mut items: Vec<Expr> = Vec::new();
            loop {
                if self.at_punct("]") {
                    break;
                }
                // `..hi` — open at the bottom; the next expression is the
                // *high* bound, not the low one.
                let open_low = self.eat_punct("..");
                let lo = if open_low {
                    Expr::Int(i64::MIN)
                } else {
                    self.parse_expr(0)?
                };
                let item = if open_low || self.eat_punct("..") {
                    // `lo..]` is open at the top; `lo..hi` is a range.
                    let hi = if self.at_punct("]") || self.at_punct(",") {
                        Expr::Int(i64::MAX)
                    } else {
                        self.parse_expr(0)?
                    };
                    Expr::Range(Box::new(lo), Box::new(hi))
                } else {
                    lo
                };
                items.push(item);
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("]")?;
            // A lone range is the plain range form.
            if items.len() == 1 {
                if let Expr::Range(lo, hi) = &items[0] {
                    return Ok(Expr::InRange(
                        Box::new(Expr::Int(0)),
                        lo.clone(),
                        hi.clone(),
                    ));
                }
            }
            return Ok(Expr::List(items));
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
            TokKind::Float(f) => {
                self.bump();
                Ok(Expr::Float(f))
            }
            TokKind::Str(s) => {
                self.bump();
                Ok(Expr::Str(s))
            }
            TokKind::Ident(id) => {
                // `sizeof_s<T>::nbits` / `::nbytes` (PSS 24.10.2).
                if id == "sizeof_s" && self.peek_at(1).is_punct("<") {
                    // `sizeof_s<T>::nbits` / `::nbytes` (PSS 24.10.2). The type
                    // argument is read directly, since it sits inside `<>`.
                    self.bump();
                    self.bump();
                    let mut name = self.expect_ident()?;
                    // A qualified type name may use `::`.
                    loop {
                        if self.eat_punct("::") || self.eat_punct(":") {
                            match self.expect_ident() {
                                Ok(more) => {
                                    name.push_str("::");
                                    name.push_str(&more);
                                }
                                Err(_) => break,
                            }
                        } else {
                            break;
                        }
                    }
                    self.expect_punct(">")?;
                    // `::nbits` — `::` is one token.
                    if !self.eat_punct("::") {
                        self.eat_punct(":");
                        self.eat_punct(":");
                    }
                    let field = self.expect_ident()?;
                    return Ok(Expr::Sizeof {
                        ty: name,
                        bits: field != "nbytes",
                    });
                }
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
                // A qualified name: `pkg::NAME`, `A::B::C`.
                let mut path = Path::simple(id);
                while self.at_punct("::") {
                    self.bump();
                    if self.at_punct("*") {
                        self.bump();
                        path.segments.push("*".to_string());
                        break;
                    }
                    match self.expect_ident() {
                        Ok(seg) => path.segments.push(seg),
                        Err(_) => break,
                    }
                }
                Ok(Expr::Path(path))
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
                // `{ ... }` is either a constraint block (`{ a; b; }`, conjoined)
                // or a literal list (`{ 1, 2, 3 }`, PSS 8.5). The separator
                // decides which.
                self.bump();
                let mut exprs = Vec::new();
                let mut saw_comma = false;
                while !self.at_punct("}") && !self.at_eof() {
                    if self.eat_punct(";") {
                        continue;
                    }
                    exprs.push(self.parse_constraint_body_item()?);
                    if self.eat_punct(",") {
                        saw_comma = true;
                        continue;
                    }
                    self.eat_punct(";");
                }
                self.expect_punct("}")?;
                if saw_comma {
                    return Ok(Expr::List(exprs));
                }
                Ok(combine_and(exprs))
            }
            TokKind::Punct(p) if p == "[" => {
                // A value set or range in operand position: `[1, 2]`, `[lo..hi]`
                // (PSS 8.5.9).
                self.parse_range_or_set()
            }
            _ => {
                let t = self.bump();
                Ok(Expr::Unknown(token_text(&t)))
            }
        }
    }

    /// `monitor NAME { activity { .. } covergroup { .. } }` (PSS 19.1,
    /// Syntax 68).
    fn parse_monitor(&mut self) -> PssResult<Monitor> {
        self.expect_ident_named("monitor")?;
        let name = self.expect_ident()?;
        // Optional base monitor type.
        if self.at_punct(":") {
            self.bump();
            let _ = self.parse_type_ref();
        }
        self.expect_punct("{")?;
        let mut pattern = MonStmt::Empty;
        let mut covergroups = Vec::new();
        let mut handles: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        while !self.at_punct("}") && !self.at_eof() {
            if self.eat_punct(";") {
                continue;
            }
            if self.at_ident("activity") {
                self.bump();
                pattern = self.parse_monitor_list(&mut handles)?;
                continue;
            }
            // `<type> <handle>;` declares an action handle for the activity.
            if matches!(self.peek().kind, TokKind::Ident(_))
                && matches!(self.peek_at(1).kind, TokKind::Ident(_))
            {
                let ty = self.expect_ident()?;
                let h = self.expect_ident()?;
                self.eat_punct(";");
                handles.insert(h, ty);
                continue;
            }
            if self.at_ident("covergroup") {
                covergroups.push(self.parse_covergroup()?);
                continue;
            }
            self.skip_braced_or_semicolon();
        }
        self.expect_punct("}")?;
        self.eat_punct(";");
        Ok(Monitor {
            name,
            component: None,
            pattern,
            covergroups,
        })
    }

    /// `{ stmt; stmt; }` inside a monitor — a sequence of sub-scenarios.
    fn parse_monitor_list(
        &mut self,
        handles: &mut std::collections::HashMap<String, String>,
    ) -> PssResult<MonStmt> {
        self.expect_punct("{")?;
        let mut items = Vec::new();
        while !self.at_punct("}") && !self.at_eof() {
            if self.eat_punct(";") {
                continue;
            }
            items.push(self.parse_monitor_stmt(handles)?);
        }
        self.expect_punct("}")?;
        Ok(match items.len() {
            0 => MonStmt::Empty,
            1 => items.pop().unwrap(),
            _ => MonStmt::Sequence(items),
        })
    }

    fn parse_monitor_stmt(
        &mut self,
        handles: &mut std::collections::HashMap<String, String>,
    ) -> PssResult<MonStmt> {
        // Optional `label :`.
        if matches!(self.peek().kind, TokKind::Ident(_)) && self.peek_at(1).is_punct(":") {
            let _ = self.expect_ident()?;
            self.bump();
        }
        for (kw, make) in [
            ("sequence", 0usize),
            ("concat", 1),
            ("eventually", 2),
            ("overlap", 3),
            ("schedule", 4),
        ] {
            if self.at_ident(kw) {
                self.bump();
                // The sub-scenario may be a braced block or a single statement:
                // `eventually r;` as well as `eventually { r; }`.
                let items = if self.at_punct("{") {
                    self.parse_monitor_list(handles)?
                } else {
                    self.parse_monitor_stmt(handles)?
                };
                let list = match items {
                    MonStmt::Sequence(v) => v,
                    MonStmt::Empty => Vec::new(),
                    other => vec![other],
                };
                return Ok(match make {
                    0 => MonStmt::Sequence(list),
                    1 => MonStmt::Concat(list),
                    2 => MonStmt::Eventually(list),
                    _ => MonStmt::Concurrent(list),
                });
            }
        }
        if self.at_ident("select") {
            self.bump();
            self.expect_punct("{")?;
            let mut arms = Vec::new();
            while !self.at_punct("}") && !self.at_eof() {
                if self.eat_punct(";") {
                    continue;
                }
                let guard = if self.eat_ident("default") {
                    None
                } else {
                    Some(self.parse_expr(0)?)
                };
                self.expect_punct(":")?;
                let body = self.parse_monitor_list(handles)?;
                let body = match body {
                    MonStmt::Sequence(v) => v,
                    MonStmt::Empty => Vec::new(),
                    other => vec![other],
                };
                arms.push((guard, body));
            }
            self.expect_punct("}")?;
            self.eat_punct(";");
            return Ok(MonStmt::Select(arms));
        }
        if self.at_ident("constraint") {
            self.bump();
            let c = self.parse_constraint_set()?;
            self.eat_punct(";");
            return Ok(MonStmt::Constraint(c));
        }
        if self.at_ident("do") {
            self.bump();
            // `do <monitor_type>` — a nested monitor (19.3.9).
            if let Ok(ty) = self.parse_type_ref() {
                let tname = ty.name();
                let leaf = tname.rsplit("::").next().unwrap_or(&tname).to_string();
                if self.at_punct(";") || self.at_punct("with") {
                    let cond = if self.eat_ident("with") {
                        Some(self.parse_constraint_set()?)
                    } else {
                        None
                    };
                    self.eat_punct(";");
                    return Ok(MonStmt::Traverse {
                        action: leaf,
                        handle: None,
                        cond,
                    });
                }
                let _ = leaf;
            }
            self.eat_punct(";");
            return Ok(MonStmt::Empty);
        }
        // `<type> <handle>;` declares a handle; `<handle>;` traverses it.
        let first = self.expect_ident()?;
        if matches!(self.peek().kind, TokKind::Ident(_)) {
            let handle = self.expect_ident()?;
            self.eat_punct(";");
            handles.insert(handle.clone(), first.clone());
            return Ok(MonStmt::Traverse {
                action: first,
                handle: Some(handle),
                cond: None,
            });
        }
        self.eat_punct(";");
        if let Some(ty) = handles.get(&first).cloned() {
            return Ok(MonStmt::Traverse {
                action: ty,
                handle: Some(first),
                cond: None,
            });
        }
        Ok(MonStmt::Traverse {
            action: first,
            handle: None,
            cond: None,
        })
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
            bound: false,
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
        "**" => 12,
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
        TokKind::Float(f) => f.to_string(),
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
        TokKind::Float(f) => format!("`{}`", f),
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
