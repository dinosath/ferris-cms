//! Abstract Syntax Tree for the Formula DSL.
//!
//! Formulas are never kept as strings after parsing. A formula parses into an
//! [`Expr`] tree where field references are [`Path`]s (dotted relationship
//! traversals) and aggregates are structural nodes. The AST is schema
//! independent: relationship *resolution* against Ferris content-type metadata
//! happens later, in the type checker / dependency analyzer / compiler.

use crate::types::ValueType;

/// Arithmetic / comparison operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl BinOp {
    pub fn as_str(&self) -> &'static str {
        use BinOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Mod => "%",
            Eq => "=",
            Ne => "!=",
            Gt => ">",
            Gte => ">=",
            Lt => "<",
            Lte => "<=",
        }
    }

    /// Whether this is a comparison operator (result type Boolean).
    pub fn is_comparison(&self) -> bool {
        matches!(
            self,
            Self::Eq | Self::Ne | Self::Gt | Self::Gte | Self::Lt | Self::Lte
        )
    }

    /// Whether this is arithmetic (result is numeric).
    pub fn is_arith(&self) -> bool {
        matches!(self, Self::Add | Self::Sub | Self::Mul | Self::Div | Self::Mod)
    }
}

/// Unary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

/// Aggregate function names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggFunc {
    Sum,
    Avg,
    Min,
    Max,
    Count,
}

impl AggFunc {
    pub fn name(&self) -> &'static str {
        use AggFunc::*;
        match self {
            Sum => "SUM",
            Avg => "AVG",
            Min => "MIN",
            Max => "MAX",
            Count => "COUNT",
        }
    }
}

/// A dotted field reference: `quantity`, `customer.name`, `sales.total`.
/// Segment 0 is an attribute of the *current* entity; each further segment
/// traverses a relationship from the previous entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    pub segments: Vec<String>,
}

impl Path {
    pub fn new(segments: Vec<String>) -> Self {
        Self { segments }
    }
    pub fn first(&self) -> &str {
        &self.segments[0]
    }
    pub fn join(&self, sep: &str) -> String {
        self.segments.join(sep)
    }
}

/// The expression AST.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// Integer literal.
    Int(i64),
    /// Decimal literal (from a source that contains a `.` or scale).
    Decimal(rust_decimal::Decimal),
    /// String literal.
    Str(String),
    /// Boolean literal.
    Bool(bool),
    /// Unqualified scalar field reference (single path segment).
    Field(Path),
    /// Unary operator application.
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    /// Binary arithmetic or comparison.
    Binary {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    /// Logical conjunction/disjunction (operands are Boolean).
    Logical {
        left: Box<Expr>,
        op: crate::ast::Logic,
        right: Box<Expr>,
    },
    /// `NOT expr`.
    Not(Box<Expr>),
    /// `expr IS [NOT] NULL`.
    IsNull {
        expr: Box<Expr>,
        negated: bool,
    },
    /// `COALESCE(a, b, ...)`.
    Coalesce(Vec<Expr>),
    /// `NULLIF(a, b)`.
    NullIf {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    /// A generic function call that is not one of the special forms. Function
    /// names are validated against a registry by the type checker.
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// `IF(cond, then, else)`.
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },
    /// `CASE WHEN c1 THEN v1 ... ELSE e END`.
    Case {
        whens: Vec<(Expr, Expr)>,
        else_: Option<Box<Expr>>,
    },
    /// Aggregate over a relationship collection: `SUM(sales.total)` and the
    /// filtered form `SUM(sales.balance WHERE sales.status = "OPEN")`.
    Aggregate {
        func: AggFunc,
        /// Inner expression evaluated for each member of the source
        /// collection. A bare [`Expr::Field`] path names the collection in its
        /// first segment.
        arg: Box<Expr>,
        /// Optional WHERE filter, also relative to the collection members.
        filter: Option<Box<Expr>>,
    },
    // A `TODAY()`/`NOW()`-style nullary builtin is represented as a Call.
}

/// Logical combinators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logic {
    And,
    Or,
}

/// The type that a whole formula is declared/expected to produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredType {
    pub value_type: ValueType,
    pub nullable: bool,
}

impl Expr {
    /// Human readable shorthand used in diagnostics.
    pub fn describe(&self) -> String {
        match self {
            Expr::Int(i) => i.to_string(),
            Expr::Decimal(d) => d.to_string(),
            Expr::Str(s) => format!("\"{s}\""),
            Expr::Bool(b) => b.to_string(),
            Expr::Field(p) => p.join("."),
            Expr::Unary { op, .. } => format!("unary {op:?}"),
            Expr::Binary { op, .. } => format!("binary {}", op.as_str()),
            Expr::Logical { op, .. } => format!("logical {op:?}"),
            Expr::Not(_) => "NOT".to_string(),
            Expr::IsNull { .. } => "IS NULL".to_string(),
            Expr::Coalesce(_) => "COALESCE".to_string(),
            Expr::NullIf { .. } => "NULLIF".to_string(),
            Expr::Call { name, .. } => name.clone(),
            Expr::If { .. } => "IF".to_string(),
            Expr::Case { .. } => "CASE".to_string(),
            Expr::Aggregate { func, .. } => func.name().to_string(),
        }
    }

    /// All free variable paths appearing in the expression (top-level and
    /// nested, including inside aggregates but not re-entering them further).
    pub fn field_paths(&self, out: &mut Vec<Path>) {
        match self {
            Expr::Field(p) => out.push(p.clone()),
            Expr::Unary { expr, .. } => expr.field_paths(out),
            Expr::Binary { left, right, .. } => {
                left.field_paths(out);
                right.field_paths(out);
            }
            Expr::Logical { left, right, .. } => {
                left.field_paths(out);
                right.field_paths(out);
            }
            Expr::Not(e) => e.field_paths(out),
            Expr::IsNull { expr, .. } => expr.field_paths(out),
            Expr::Coalesce(items) => {
                for i in items {
                    i.field_paths(out);
                }
            }
            Expr::NullIf { a, b } => {
                a.field_paths(out);
                b.field_paths(out);
            }
            Expr::Call { args, .. } => {
                for a in args {
                    a.field_paths(out);
                }
            }
            Expr::If {
                cond, then, else_, ..
            } => {
                cond.field_paths(out);
                then.field_paths(out);
                else_.field_paths(out);
            }
            Expr::Case { whens, else_ } => {
                for (c, v) in whens {
                    c.field_paths(out);
                    v.field_paths(out);
                }
                if let Some(e) = else_ {
                    e.field_paths(out);
                }
            }
            Expr::Aggregate { arg, filter, .. } => {
                arg.field_paths(out);
                if let Some(f) = filter {
                    f.field_paths(out);
                }
            }
            _ => {}
        }
    }
}
