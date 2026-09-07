//! SeaORM 2 / sea-query compiler.
//!
//! Compiles a typed formula IR into safe `sea_query` expressions (no arbitrary
//! user SQL). Scalars compile to column expressions; single-row relationship
//! traversals and single-hop relationship aggregates compile to correlated
//! scalar subqueries, so a whole collection (e.g. every customer) can be
//! computed in ONE query — never N+1 per row.
//!
//! The output is a [`sea_query::SimpleExpr`] that callers embed via
//! `select().expr_as(expr, Alias)` or use in ORDER BY / WHERE, matching how
//! Ferris's dynamic-store builds queries.

use crate::errors::{FormulaError, Result};
use crate::ir::{TKind, TypedExpr};
use crate::registry::{
    formula_config, relation, relation_fk_column, scalar_column, value_type_of_formula,
};
use crate::type_checker::check as typecheck;
use core_schema::Schema;
use sea_query::{Alias, Expr, ExprTrait, Func, Query, SimpleExpr, SubQueryStatement, Value};
use std::collections::HashMap;

/// A compiled SQL expression plus its scalar value type.
#[derive(Clone, Debug)]
pub struct CompiledExpr {
    pub sql: SimpleExpr,
    pub ty: crate::types::ValueType,
}

/// Relationship compiler over a schema registry.
pub struct SqlCompiler {
    all: Vec<Schema>,
    /// (schema_uid, field) -> typed IR of formula fields (memoized).
    formulas: HashMap<(String, String), TypedExpr>,
}

impl SqlCompiler {
    pub fn new(all: Vec<Schema>) -> Result<Self> {
        let mut formulas = HashMap::new();
        for schema in &all {
            for (field, attr) in &schema.attributes {
                if let Some(cfg) = &attr.formula {
                    let ast = crate::parser::parse(&cfg.expression).map_err(|e| {
                        e.with_expression(format!("{}.{}", schema.uid, field))
                    })?;
                    let ret = value_type_of_formula(cfg.return_type);
                    let ir = typecheck(&ast, schema, &all, ret).map_err(|e| {
                        e.with_expression(format!("{}.{} = {}", schema.uid, field, cfg.expression))
                    })?;
                    formulas.insert((schema.uid.to_string(), field.clone()), ir);
                }
            }
        }
        Ok(Self { all, formulas })
    }

    fn formula_ir(&self, schema_uid: &str, field: &str) -> Result<TypedExpr> {
        self.formulas
            .get(&(schema_uid.to_string(), field.to_string()))
            .cloned()
            .ok_or_else(|| FormulaError::new(format!("{schema_uid}.{field} is not a formula")))
    }

    /// Compile a formula for `host_schema` (whose rows are being queried).
    pub fn compile_host(&self, host: &Schema, ir: &TypedExpr) -> Result<CompiledExpr> {
        let table = host.table_name();
        let sql = self.compile_scalar(host, &table, ir)?;
        Ok(CompiledExpr {
            sql,
            ty: ir.ty,
        })
    }

    /// Compile a scalar expression in `scope` (whose table is `table`).
    fn compile_scalar(
        &self,
        scope: &Schema,
        table: &str,
        ir: &TypedExpr,
    ) -> Result<SimpleExpr> {
        use TKind::*;
        match &ir.kind {
            Int(i) => Ok(Expr::value(Value::BigInt(Some(*i)))),
            Decimal(d) => {
                let f = d.to_string().parse::<f64>().map_err(|_| {
                    FormulaError::new(format!("cannot render decimal literal {d}"))
                })?;
                Ok(Expr::value(Value::Double(Some(f))))
            }
            Str(s) => Ok(Expr::value(Value::String(Some(s.clone())))),
            Bool(b) => Ok(Expr::value(Value::Bool(Some(*b)))),
            Field { path, .. } => self.compile_path(scope, table, &path.segments),
            Neg(inner) => {
                let e = self.compile_scalar(scope, table, inner)?;
                // 0 - x negates without raw SQL.
                Ok(Expr::value(Value::Int(Some(0))).sub(e))
            }
            Binary { op, left, right } => {
                use crate::ast::BinOp::*;
                let l = self.compile_scalar(scope, table, left)?;
                let r = self.compile_scalar(scope, table, right)?;
                Ok(match op {
                    Add => l.add(r),
                    Sub => l.sub(r),
                    Mul => l.mul(r),
                    Div => l.div(r),
                    Mod => l.modulo(r),
                    Eq => l.eq(r),
                    Ne => l.ne(r),
                    Gt => l.gt(r),
                    Gte => l.gte(r),
                    Lt => l.lt(r),
                    Lte => l.lte(r),
                })
            }
            Logical { op, left, right } => {
                use crate::ast::Logic::*;
                let l = self.compile_scalar(scope, table, left)?;
                let r = self.compile_scalar(scope, table, right)?;
                Ok(match op {
                    And => l.and(r),
                    Or => l.or(r),
                })
            }
            Not(inner) => Ok(self.compile_scalar(scope, table, inner)?.not()),
            IsNull { inner, negated } => {
                let e = self.compile_scalar(scope, table, inner)?;
                if *negated {
                    Ok(e.is_not_null())
                } else {
                    Ok(e.is_null())
                }
            }
            Coalesce(items) => {
                if items.len() == 1 {
                    return self.compile_scalar(scope, table, &items[0]);
                }
                let mut args = Vec::with_capacity(items.len());
                for it in items {
                    args.push(self.compile_scalar(scope, table, it)?);
                }
                Ok(Expr::from(Func::coalesce(args)))
            }
            NullIf { .. } => Err(FormulaError::new(
                "NULLIF is supported in the evaluator; not yet emitted to SQL",
            )),
            Call { .. } | If { .. } | Case { .. } => Err(FormulaError::new(format!(
                "this expression form (function/IF/CASE) is not yet emitted to SQL (supported in the evaluator)",
            ))),
            Aggregate { func, collection, arg, filter } => {
                self.compile_aggregate(scope, table, *func, &collection.segments[0], arg, filter)
            }
        }
    }

    /// Compile a member-expression path within `scope`. Handles scalar columns,
    /// formula-field inlining, and single-row relationship traversal via
    /// correlated subqueries.
    fn compile_path(&self, scope: &Schema, table: &str, path: &[String]) -> Result<SimpleExpr> {
        if path.is_empty() {
            return Err(FormulaError::new("empty field path"));
        }
        let seg0 = &path[0];
        let attr = scope.attributes.get(seg0).ok_or_else(|| {
            FormulaError::new(format!("unknown field `{seg0}` on {}", scope.uid))
        })?;

        if path.len() == 1 {
            // Leaf: scalar column or an inlined scalar/formula field.
            if attr.formula.is_some() {
                let ir = self.formula_ir(&scope.uid.to_string(), seg0)?;
                return self.compile_scalar(scope, table, &ir);
            }
            if attr.attr_type.is_scalar_column() {
                return Ok(Expr::col((
                    Alias::new(table.to_string()),
                    Alias::new(scalar_column(seg0)),
                )));
            }
            return Err(FormulaError::new(format!(
                "`{seg0}` is not a scalar field on {}",
                scope.uid
            )));
        }

        // Traverse a relationship.
        if attr.attr_type != core_domain::FieldType::Relation {
            return Err(FormulaError::new(format!(
                "cannot traverse through scalar field `{seg0}`"
            )));
        }
        let (target, kind) = relation(&self.all, scope, seg0).ok_or_else(|| {
            FormulaError::new(format!("cannot resolve relation `{seg0}` on {}", scope.uid))
        })?;
        if crate::registry::is_to_many(kind) {
            return Err(FormulaError::new(format!(
                "collection `{seg0}` used as a scalar (must be aggregated)"
            )));
        }
        let target_table = target.table_name();
        let fk = relation_fk_column(seg0);
        let inner = self.compile_path(target, &target_table, &path[1..])?;
        // (SELECT <inner> FROM <target_table> WHERE <target_table>.id = <table>.<fk>)
        let mut sel = Query::select();
        sel.expr_as(
            inner,
            Alias::new("f"),
        )
        .from(Alias::new(&target_table))
        .and_where(Expr::col((Alias::new(&target_table), Alias::new("id"))).eq(Expr::col((
            Alias::new(table.to_string()),
            Alias::new(fk),
        ))))
        .limit(1);
        Ok(Expr::from(SubQueryStatement::from(sel)))
    }

    /// Compile a single-hop relationship aggregate as a correlated scalar
    /// subquery over `collection` (a to-many relation of `scope`).
    #[allow(clippy::too_many_arguments)]
    fn compile_aggregate(
        &self,
        scope: &Schema,
        table: &str,
        func: crate::ast::AggFunc,
        collection: &str,
        arg: &TypedExpr,
        filter: &Option<Box<TypedExpr>>,
    ) -> Result<SimpleExpr> {
        let (target, kind) = relation(&self.all, scope, collection).ok_or_else(|| {
            FormulaError::new(format!(
                "`{collection}` is not a relation on {}",
                scope.uid
            ))
        })?;
        if !crate::registry::is_to_many(kind) {
            return Err(FormulaError::new(format!(
                "`{collection}` is not a to-many relation"
            )));
        }
        // Determine the correlation FK column on the target table. For
        // oneToMany the inverse FK is `fk_column(mapped_by)`.
        let target_attr = scope
            .attributes
            .get(collection)
            .expect("collection attr exists");
        let mapped = target_attr
            .mapped_by
            .clone()
            .unwrap_or_else(|| collection.to_string());
        let fk = relation_fk_column(&mapped);

        let target_table = target.table_name();
        let member_expr = self.compile_scalar(target, &target_table, arg)?;

        // Build aggregate function call over the member expression.
        let agg: SimpleExpr = match func {
            crate::ast::AggFunc::Sum => Expr::from(Func::sum(member_expr.clone())),
            crate::ast::AggFunc::Avg => Expr::from(Func::avg(member_expr.clone())),
            crate::ast::AggFunc::Min => Expr::from(Func::min(member_expr.clone())),
            crate::ast::AggFunc::Max => Expr::from(Func::max(member_expr.clone())),
            crate::ast::AggFunc::Count => {
                // COUNT(*) if no arg or arg is unit; else COUNT(expr)
                Expr::from(Func::count(member_expr.clone()))
            }
        };

        let mut sel = Query::select();
        sel.expr_as(agg, Alias::new("v"))
            .from(Alias::new(&target_table))
            .and_where(
                Expr::col((Alias::new(&target_table), Alias::new(&fk))).eq(Expr::col((
                    Alias::new(table.to_string()),
                    Alias::new("id"),
                ))),
            );
        if let Some(f) = filter {
            let cond = self.compile_scalar(target, &target_table, f)?;
            sel.and_where(cond);
        }
        Ok(Expr::from(SubQueryStatement::from(sel)))
    }
}

/// Convenience wrapper used by integration tests to add computed columns to a
/// base `select` built by dynamic-store. Returns the expression for `field`.
pub fn build_expr(
    all: &[Schema],
    host: &Schema,
    formula_field: &str,
) -> Result<CompiledExpr> {
    if formula_config(host, formula_field).is_none() {
        return Err(FormulaError::new(format!(
            "{formula_field} is not a formula field on {}",
            host.uid
        )));
    }
    let compiler = SqlCompiler::new(all.to_vec())?;
    let ir = compiler.formula_ir(&host.uid.to_string(), formula_field)?;
    compiler.compile_host(host, &ir)
}
