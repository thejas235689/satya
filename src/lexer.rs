//! Hand-written lexer for the PSS v3.0 subset understood by this engine.
//!
//! Keywords are *not* distinguished from identifiers here; the parser matches
//! on the identifier text. This keeps the lexer small and makes it easy to
//! treat PSS contextual keywords correctly.

use crate::error::{PssError, PssResult};

#[derive(Debug, Clone, PartialEq)]
pub enum TokKind {
    /// Identifier or keyword.
    Ident(String),
    /// Integer literal (decimal, 0x-hex or 0b-binary), already evaluated.
    Int(i64),
    /// Floating-point literal (PSS Syntax 1), e.g. `20.14`, `2e6`, `1_0.2_5`.
    Float(f64),
    /// String literal (contents, without quotes).
    Str(String),
    /// Operator or punctuation, e.g. `::`, `==`, `{`, `..`.
    Punct(String),
    /// End of input.
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokKind,
    pub line: usize,
    pub col: usize,
}

impl Token {
    pub fn is_punct(&self, p: &str) -> bool {
        matches!(&self.kind, TokKind::Punct(s) if s == p)
    }

    pub fn is_ident(&self, name: &str) -> bool {
        matches!(&self.kind, TokKind::Ident(s) if s == name)
    }

    pub fn ident(&self) -> Option<&str> {
        match &self.kind {
            TokKind::Ident(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// Multi-character operators, longest first so that greedy matching is correct.
const OPERATORS: &[&str] = &[
    "::", "->", "==", "!=", "<=", ">=", "&&", "||", "..", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "**",
    "|=", "&=", "^=",
];

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Lexer {
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if let Some(ch) = c {
            self.pos += 1;
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        c
    }

    fn skip_ws_and_comments(&mut self) -> PssResult<()> {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some('/') if self.peek_at(1) == Some('*') => {
                    let (l, c0) = (self.line, self.col);
                    self.bump();
                    self.bump();
                    loop {
                        match self.peek() {
                            None => {
                                return Err(PssError::new("unterminated block comment", l, c0))
                            }
                            Some('*') if self.peek_at(1) == Some('/') => {
                                self.bump();
                                self.bump();
                                break;
                            }
                            _ => {
                                self.bump();
                            }
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    /// Tokenize the whole input.
    pub fn tokenize(mut self) -> PssResult<Vec<Token>> {
        let mut out = Vec::new();
        loop {
            self.skip_ws_and_comments()?;
            let (line, col) = (self.line, self.col);
            let c = match self.peek() {
                None => {
                    out.push(Token {
                        kind: TokKind::Eof,
                        line,
                        col,
                    });
                    return Ok(out);
                }
                Some(c) => c,
            };

            if c.is_ascii_alphabetic() || c == '_' {
                let mut s = String::new();
                while let Some(ch) = self.peek() {
                    if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                        s.push(ch);
                        self.bump();
                    } else {
                        break;
                    }
                }
                out.push(Token {
                    kind: TokKind::Ident(s),
                    line,
                    col,
                });
            } else if c.is_ascii_digit() {
                let kind = self.lex_number(line, col)?;
                out.push(Token { kind, line, col });
            } else if c == '"' {
                // Triple-quoted string (`"""..."""`) or an ordinary string.
                let triple = self.peek_at(1) == Some('"') && self.peek_at(2) == Some('"');
                self.bump();
                if triple {
                    self.bump();
                    self.bump();
                }
                let mut s = String::new();
                loop {
                    if triple
                        && self.peek() == Some('"')
                        && self.peek_at(1) == Some('"')
                        && self.peek_at(2) == Some('"')
                    {
                        self.bump();
                        self.bump();
                        self.bump();
                        break;
                    }
                    match self.bump() {
                        None => return Err(PssError::new("unterminated string literal", line, col)),
                        Some('"') if !triple => break,
                        Some('\\') => {
                            if let Some(e) = self.bump() {
                                s.push(match e {
                                    'n' => '\n',
                                    't' => '\t',
                                    '\\' => '\\',
                                    '"' => '"',
                                    other => other,
                                });
                            }
                        }
                        Some(ch) => s.push(ch),
                    }
                }
                out.push(Token {
                    kind: TokKind::Str(s),
                    line,
                    col,
                });
            } else if c == '\\' {
                // An escaped identifier: `\` then any characters up to
                // whitespace (PSS Syntax 4). The name excludes the backslash.
                self.bump();
                let mut name = String::new();
                while let Some(ch) = self.peek() {
                    if ch.is_whitespace() {
                        break;
                    }
                    name.push(ch);
                    self.bump();
                }
                if name.is_empty() {
                    return Err(PssError::new(
                        "a backslash must begin an escaped identifier".to_string(),
                        line,
                        col,
                    ));
                }
                out.push(Token {
                    kind: TokKind::Ident(name),
                    line,
                    col,
                });
            } else if c == '\'' {
                // A sized literal prefix (4'b1010, 8'hFF) — but a lone quote,
                // as in `'{ ... }` aggregate literals or quoted text, is just
                // punctuation.
                let nxt = self.peek_at(1);
                if matches!(nxt, Some(ch) if ch.is_ascii_digit() || "bBoOdDhHsS".contains(ch)) {
                    self.bump();
                    let kind = self.lex_sized_number(line, col)?;
                    out.push(Token { kind, line, col });
                } else if nxt == Some('{') {
                    // `'{ ... }` — an aggregate literal.
                    self.bump();
                    out.push(Token {
                        kind: TokKind::Punct("'".to_string()),
                        line,
                        col,
                    });
                } else {
                    return Err(PssError::new(
                        "a quote must begin a sized literal (`4'b1010`) or an \
                         aggregate literal (`'{...}`)"
                            .to_string(),
                        line,
                        col,
                    ));
                }
            } else {
                // Operator / punctuation, greedy longest match.
                let rest: String = self.chars[self.pos..].iter().take(3).collect();
                let mut matched = None;
                for op in OPERATORS {
                    if rest.starts_with(op) {
                        matched = Some(*op);
                        break;
                    }
                }
                let op = matched
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| c.to_string());
                for _ in 0..op.chars().count() {
                    self.bump();
                }
                out.push(Token {
                    kind: TokKind::Punct(op),
                    line,
                    col,
                });
            }
        }
    }

    /// A sized literal after `'`: `4'b1010`, `8'hFF`, `16'd42`.
    fn lex_sized_number(&mut self, line: usize, col: usize) -> PssResult<TokKind> {
        // An optional `s`/`S` marks the literal as signed (PSS Syntax 1):
        // `4'sd12`, `16'sH_FF`.
        if matches!(self.peek(), Some('s') | Some('S')) {
            self.bump();
        }
        let radix = match self.peek() {
            Some('b') | Some('B') => 2,
            Some('o') | Some('O') => 8,
            Some('d') | Some('D') => 10,
            Some('h') | Some('H') => 16,
            _ => 10,
        };
        if matches!(
            self.peek(),
            Some('b') | Some('B') | Some('o') | Some('O') | Some('d') | Some('D') | Some('h') | Some('H')
        ) {
            self.bump();
        }
        let mut digits = String::new();
        while let Some(ch) = self.peek() {
            if ch == '_' {
                self.bump();
            } else if ch.is_ascii_alphanumeric() {
                digits.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        u64::from_str_radix(&digits, radix)
            .map(|v| TokKind::Int(v as i64))
            .map_err(|_| PssError::new(format!("invalid sized literal `'{}`", digits), line, col))
    }

    fn lex_number(&mut self, line: usize, col: usize) -> PssResult<TokKind> {
        let mut s = String::new();
        // Support 0x / 0b / 0o prefixes, and underscores as digit separators.
        if self.peek() == Some('0')
            && matches!(self.peek_at(1), Some('x') | Some('X') | Some('b') | Some('B') | Some('o') | Some('O'))
        {
            let base_char = self.peek_at(1).unwrap();
            s.push(self.bump().unwrap());
            s.push(self.bump().unwrap());
            let radix = match base_char {
                'x' | 'X' => 16,
                'b' | 'B' => 2,
                _ => 8,
            };
            let mut digits = String::new();
            while let Some(ch) = self.peek() {
                if ch == '_' {
                    self.bump();
                } else if ch.is_ascii_alphanumeric() {
                    digits.push(ch);
                    self.bump();
                } else {
                    break;
                }
            }
            // A 64-bit pattern such as `0xFFFF_FFFF_FFFF_FFFF` does not fit a
            // signed 64-bit integer, so parse unsigned and keep the bit
            // pattern (PSS 7.2: the value is a bit vector).
            let val = u64::from_str_radix(&digits, radix)
                .map(|v| v as i64)
                .map_err(|_| PssError::new(format!("invalid integer literal `{}{}`", s, digits), line, col))?;
            return Ok(TokKind::Int(val));
        }
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.bump();
            } else if ch == '_' {
                self.bump();
            } else {
                break;
            }
        }
        // A fractional part or an exponent makes this a floating-point
        // literal (PSS Syntax 1). The digits may carry `_` separators.
        let mut is_float = false;
        if self.peek() == Some('.')
            && matches!(self.peek_at(1), Some(ch) if ch.is_ascii_digit())
        {
            is_float = true;
            s.push(self.bump().unwrap()); // the dot
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() {
                    s.push(ch);
                    self.bump();
                } else if ch == '_' {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            // An exponent only continues the number if a digit (or a sign and a
            // digit) follows.
            let after = self.peek_at(1);
            let ok = match after {
                Some(ch) if ch.is_ascii_digit() => true,
                Some('+') | Some('-') => matches!(self.peek_at(2), Some(ch) if ch.is_ascii_digit()),
                _ => false,
            };
            if ok {
                is_float = true;
                s.push(self.bump().unwrap());
                if matches!(self.peek(), Some('+') | Some('-')) {
                    s.push(self.bump().unwrap());
                }
                while let Some(ch) = self.peek() {
                    if ch.is_ascii_digit() {
                        s.push(ch);
                        self.bump();
                    } else if ch == '_' {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
        }
        if is_float {
            let cleaned: String = s.chars().filter(|c| *c != '_').collect();
            return cleaned
                .parse::<f64>()
                .map(TokKind::Float)
                .map_err(|_| PssError::new(format!("invalid floating-point literal `{}`", s), line, col));
        }

        // A based literal may follow the size: `64'd2`, `8'hFF`, `4'b1010`
        // (PSS Syntax 1). The size is a width hint; the *value* is what the
        // token carries, so the digits after the base character are the value.
        if self.peek() == Some('\'') {
            if let Some(nxt) = self.peek_at(1) {
                if "bBoOdDhHsS".contains(nxt) {
                    self.bump(); // the apostrophe
                    return self.lex_sized_number(line, col);
                }
            }
        }
        // Parse unsigned so a full 64-bit value such as
        // `18446744073709551615` keeps its bit pattern rather than failing.
        let val: i64 = s
            .parse::<u64>()
            .map(|v| v as i64)
            .map_err(|_| PssError::new(format!("invalid integer literal `{}`", s), line, col))?;
        Ok(TokKind::Int(val))
    }
}

/// Convenience: tokenize a source string.
pub fn tokenize(src: &str) -> PssResult<Vec<Token>> {
    Lexer::new(src).tokenize()
}


