//! Hand-written lexer for the PSS v3.0 subset understood by this engine.
//!
//! Keywords are *not* distinguished from identifiers here; the parser matches
//! on the identifier text. This keeps the lexer small and makes it easy to
//! treat PSS contextual keywords correctly.

use crate::error::{PssError, PssResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokKind {
    /// Identifier or keyword.
    Ident(String),
    /// Integer literal (decimal, 0x-hex or 0b-binary), already evaluated.
    Int(i64),
    /// String literal (contents, without quotes).
    Str(String),
    /// Operator or punctuation, e.g. `::`, `==`, `{`, `..`.
    Punct(String),
    /// End of input.
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    "::", "->", "==", "!=", "<=", ">=", "&&", "||", "..", "<<", ">>", "+=", "-=", "*=", "/=", "%=",
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
                self.bump();
                let mut s = String::new();
                loop {
                    match self.bump() {
                        None => return Err(PssError::new("unterminated string literal", line, col)),
                        Some('"') => break,
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
            } else if c == '\'' {
                // Sized literal prefix such as 4'b1010 or 8'hFF.
                self.bump();
                let kind = self.lex_number(line, col)?;
                out.push(Token { kind, line, col });
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
            let val = i64::from_str_radix(&digits, radix)
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
        let val: i64 = s
            .parse()
            .map_err(|_| PssError::new(format!("invalid integer literal `{}`", s), line, col))?;
        Ok(TokKind::Int(val))
    }
}

/// Convenience: tokenize a source string.
pub fn tokenize(src: &str) -> PssResult<Vec<Token>> {
    Lexer::new(src).tokenize()
}


