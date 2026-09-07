//! Typed intermediate representation.
//!
//! After type checking, an AST is lowered to a [`TypedExpr`] where every node
//! carries the [`ValueType`] it produces. This is the internal representation
//! the SeaORM compiler and the runtime evaluator consume; it is what makes it
//! possible to add further compilers (Postgres, DuckDB, ...) without changing
//! the formula language.

use crate::ast::{AggFunc, BinOp, Logic, Path};
use crate::types::ValueType;
use rust_decimal::Decimal;

/// A typed expression node.
#[derive(Clone, Debug, PartialEq)]
pub struct TypedExpr {
    pub ty: ValueType,
    pub kind: TKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TKind {
    Int(i64),
    Decimal(Decimal),
    Str(String),
    Bool(bool),
    /// A scalar field reference. `path` is kept in the *host* (outer) scope;
    /// the compiler/evaluator re-resolve relationships from it. `scalar` is
    /// true when the path can be evaluated as a scalar in the current scope
    /// (i.e. it does not cross a to-many collection).
    Field {
        path: Path,
        scalar: bool,
    },
    Neg(Box<TypedExpr>),
    Binary {
        op: BinOp,
        left: Box<TypedExpr>,
        right: Box<TypedExpr>,
    },
    Logical {
        op: Logic,
        left: Box<TypedExpr>,
        right: Box<TypedExpr>,
    },
    Not(Box<TypedExpr>),
    IsNull {
        inner: Box<TypedExpr>,
        negated: bool,
    },
    Coalesce(Vec<TypedExpr>),
    NullIf {
        a: Box<TypedExpr>,
        b: Box<TypedExpr>,
    },
    Call {
        name: String,
        args: Vec<TypedExpr>,
    },
    If {
        cond: Box<TypedExpr>,
        then: Box<TypedExpr>,
        else_: Box<TypedExpr>,
    },
    Case {
        whens: Vec<(TypedExpr, TypedExpr)>,
        else_: Option<Box<TypedExpr>>,
    },
    /// Aggregate. `collection` names the to-many relation on the host that is
    /// iterated; `arg` and `filter` are expressions evaluated per member in the
    /// member scope (their field paths are already rebased / member-relative).
    Aggregate {
        func: AggFunc,
        collection: Path,
        arg: Box<TypedExpr>,
        filter: Option<Box<TypedExpr>>,
    },
}

impl TypedExpr {
    pub fn new(ty: ValueType, kind: TKind) -> Self {
        Self { ty, kind }
    }
}
