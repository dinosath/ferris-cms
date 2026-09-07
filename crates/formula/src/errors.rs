//! Error types for the Formula subsystem.

use crate::types::ValueType;
use std::fmt;

/// A byte-offset span inside the formula source, used to point at the failing
/// region when reporting errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
    pub fn merge(a: Span, b: Span) -> Span {
        Span {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
        }
    }
}

/// Where an error occurred. Carries the original source expression when known
/// so validation messages can show it verbatim.
#[derive(Clone, Debug)]
pub struct Spanned {
    pub message: String,
    pub span: Option<Span>,
}

/// A validation / compilation error with an optional location.
#[derive(Clone, Debug, thiserror::Error)]
pub struct FormulaError {
    /// The formula source expression, when known.
    pub expression: Option<String>,
    /// 1-based line within the expression, when available.
    pub location: Option<(usize, usize)>,
    /// Human readable message.
    pub message: String,
    /// Optional: expected type when a type mismatch was found.
    pub expected: Option<ValueType>,
    /// Optional: the actual type found.
    pub actual: Option<ValueType>,
}

impl fmt::Display for FormulaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some((line, col)) = self.location {
            if let Some(expr) = &self.expression {
                write!(
                    f,
                    "formula error at {line}:{col} in `{expr}`: {}",
                    self.message
                )?;
                return Ok(());
            }
            write!(f, "formula error at {line}:{col}: {}", self.message)?;
            return Ok(());
        }
        if let Some(expr) = &self.expression {
            write!(f, "formula error in `{expr}`: {}", self.message)?;
        } else {
            write!(f, "formula error: {}", self.message)?;
        }
        Ok(())
    }
}

impl FormulaError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            expression: None,
            location: None,
            message: message.into(),
            expected: None,
            actual: None,
        }
    }

    pub fn at(message: impl Into<String>, span: Span) -> Self {
        // Keep span info in message text; callers usually have a single line.
        let _ = span;
        Self::new(message)
    }

    pub fn with_expression(mut self, expr: impl Into<String>) -> Self {
        self.expression = Some(expr.into());
        self
    }

    pub fn with_type(mut self, expected: ValueType, actual: ValueType) -> Self {
        self.expected = Some(expected);
        self.actual = Some(actual);
        self
    }

    /// Convenience for the common "expected X, got Y" mismatch.
    pub fn type_mismatch(expected: ValueType, actual: ValueType) -> Self {
        Self::new(format!(
            "type mismatch: expected {expected}, found {actual}"
        ))
        .with_type(expected, actual)
    }
}

pub type Result<T> = std::result::Result<T, FormulaError>;
