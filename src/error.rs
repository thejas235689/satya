//! Error type shared by the lexer, parser, lowering, solver and engine.

use std::fmt;

/// A single diagnostic produced anywhere in the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PssError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl PssError {
    pub fn new(message: impl Into<String>, line: usize, col: usize) -> Self {
        PssError {
            message: message.into(),
            line,
            col,
        }
    }

    /// An error with no meaningful source location (used by later stages).
    pub fn plain(message: impl Into<String>) -> Self {
        PssError {
            message: message.into(),
            line: 0,
            col: 0,
        }
    }
}

impl fmt::Display for PssError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "error: {}", self.message)
        } else {
            write!(f, "error: {} (line {}, col {})", self.message, self.line, self.col)
        }
    }
}

impl std::error::Error for PssError {}

pub type PssResult<T> = Result<T, PssError>;
