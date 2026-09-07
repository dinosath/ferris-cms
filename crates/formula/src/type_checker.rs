//! Type checker: validates a formula AST against Ferris content-type metadata
//! and lowers it to a typed [`TypedExpr`].
//!
//! The type checker is schema aware: field paths are resolved through the
//! schema registry, so `quantity * customer.name` fails (Decimal × String) and
//! `SUM(non_existing_relation.total)` fails during relationship resolution.

use crate::ast::{AggFunc, BinOp, Expr, Path, UnOp};
use crate::errors::{FormulaError, Result};
use crate::ir::{TKind, TypedExpr};
use crate::registry::{is_to_many, relation, walk_path};
use crate::types::{promote_numeric, ValueType};
use core_schema::Schema;
use rust_decimal::Decimal;

/// A compiled, typed formula plus the execution metadata of its root field.
#[derive(Clone, Debug)]
pub struct CheckedFormula {
    pub typed: TypedExpr,
    /// The host (declaring) schema uid.
    pub host_uid: String,
}

/// Check a formula AST declared on `host`, whose field references resolve
/// against the full registry `all`. `declared` is the intended return type.
pub fn check(ast: &Expr, host: &Schema, all: &[Schema], declared: ValueType) -> Result<TypedExpr> {
    let typed = check_in_scope(ast, host, all)?;
    // Verify the inferred type matches the declared return type.
    if typed.ty != declared {
        return Err(FormulaError::type_mismatch(declared, typed.ty)
            .with_expression(format!("formula returns {}", typed.ty)));
    }
    Ok(typed)
}

/// Type-check an expression evaluated in `scope` (a scalar scope; collections
/// are rejected unless wrapped by an aggregate).
fn check_in_scope(ast: &Expr, scope: &Schema, all: &[Schema]) -> Result<TypedExpr> {
    use Expr::*;
    match ast {
        Int(i) => Ok(TypedExpr::new(ValueType::Integer, TKind::Int(*i))),
        Decimal(d) => Ok(TypedExpr::new(ValueType::Decimal, TKind::Decimal(*d))),
        Str(s) => Ok(TypedExpr::new(ValueType::String, TKind::Str(s.clone()))),
        Bool(b) => Ok(TypedExpr::new(ValueType::Boolean, TKind::Bool(*b))),
        Field(p) => {
            let w = walk_path(all, scope, &p.segments, false)?;
            // Field crossing a collection is invalid outside an aggregate.
            if w.crossed_collection {
                return Err(FormulaError::new(format!(
                    "field `{}` traverses a collection and must be used inside an aggregate",
                    p.join(".")
                )));
            }
            Ok(TypedExpr::new(
                w.value_type,
                TKind::Field {
                    path: p.clone(),
                    scalar: true,
                },
            ))
        }
        Unary { op: UnOp::Not, expr } => {
            let inner = check_in_scope(expr, scope, all)?;
            expect_bool(&inner, "operand of NOT")?;
            Ok(TypedExpr::new(
                ValueType::Boolean,
                TKind::Not(Box::new(inner)),
            ))
        }
        Unary { op: UnOp::Neg, expr } => {
            let inner = check_in_scope(expr, scope, all)?;
            if !inner.ty.is_numeric() {
                return Err(FormulaError::type_mismatch(ValueType::Integer, inner.ty)
                    .with_expression("negation requires a numeric operand"));
            }
            Ok(TypedExpr::new(inner.ty, TKind::Neg(Box::new(inner))))
        }
        Binary { left, op, right } => {
            let l = check_in_scope(left, scope, all)?;
            let r = check_in_scope(right, scope, all)?;
            check_binary(op, l, r)
        }
        Logical { left, op, right } => {
            let l = check_in_scope(left, scope, all)?;
            let r = check_in_scope(right, scope, all)?;
            expect_bool(&l, "left operand of logical")?;
            expect_bool(&r, "right operand of logical")?;
            Ok(TypedExpr::new(
                ValueType::Boolean,
                TKind::Logical {
                    op: *op,
                    left: Box::new(l),
                    right: Box::new(r),
                },
            ))
        }
        Not(e) => {
            let inner = check_in_scope(e, scope, all)?;
            expect_bool(&inner, "operand of NOT")?;
            Ok(TypedExpr::new(ValueType::Boolean, TKind::Not(Box::new(inner))))
        }
        IsNull { expr, negated } => {
            let inner = check_in_scope(expr, scope, all)?;
            Ok(TypedExpr::new(
                ValueType::Boolean,
                TKind::IsNull {
                    inner: Box::new(inner),
                    negated: *negated,
                },
            ))
        }
        Coalesce(items) => {
            if items.is_empty() {
                return Err(FormulaError::new("COALESCE requires at least one argument"));
            }
            let mut typed_items = Vec::with_capacity(items.len());
            let mut ty = None;
            for it in items {
                let t = check_in_scope(it, scope, all)?;
                ty = Some(merge_types(ty, t.ty, "COALESCE")?);
                typed_items.push(t);
            }
            let final_ty = ty.unwrap();
            // re-coerce integer literals to the joined type
            let typed_items = typed_items
                .into_iter()
                .map(|t| coerce_value(t, final_ty))
                .collect();
            Ok(TypedExpr::new(final_ty, TKind::Coalesce(typed_items)))
        }
        NullIf { a, b } => {
            let ta = check_in_scope(a, scope, all)?;
            let tb = check_in_scope(b, scope, all)?;
            if !comparable(ta.ty, tb.ty) {
                return Err(FormulaError::type_mismatch(ta.ty, tb.ty)
                    .with_expression("NULLIF arguments must be comparable"));
            }
            let aty = ta.ty;
            Ok(TypedExpr::new(
                aty,
                TKind::NullIf {
                    a: Box::new(ta),
                    b: Box::new(coerce_value(tb, aty)),
                },
            ))
        }
        Call { name, args } => {
            let f = function_registry(name)?;
            if f.min_args.is_some() && (args.len() < f.min_args.unwrap()) {
                return Err(FormulaError::new(format!(
                    "function {name} expects at least {} arguments",
                    f.min_args.unwrap()
                )));
            }
            if f.max_args.is_some() && (args.len() > f.max_args.unwrap()) {
                return Err(FormulaError::new(format!(
                    "function {name} expects at most {} arguments",
                    f.max_args.unwrap()
                )));
            }
            let mut typed_args = Vec::with_capacity(args.len());
            for a in args {
                typed_args.push(check_in_scope(a, scope, all)?);
            }
            let ret = (f.return_fn)(&typed_args).map_err(|e| e.with_expression(name.clone()))?;
            Ok(TypedExpr::new(ret, TKind::Call {
                name: name.clone(),
                args: typed_args,
            }))
        }
        If {
            cond,
            then,
            else_,
        } => {
            let c = check_in_scope(cond, scope, all)?;
            expect_bool(&c, "IF condition")?;
            let t = check_in_scope(then, scope, all)?;
            let e = check_in_scope(else_, scope, all)?;
            let ty = merge_types(Some(t.ty), e.ty, "IF branches")?;
            Ok(TypedExpr::new(
                ty,
                TKind::If {
                    cond: Box::new(c),
                    then: Box::new(coerce_value(t, ty)),
                    else_: Box::new(coerce_value(e, ty)),
                },
            ))
        }
        Case { whens, else_ } => {
            let mut res_ty = None;
            let mut typed_whens = Vec::with_capacity(whens.len());
            for (cond, val) in whens {
                let c = check_in_scope(cond, scope, all)?;
                expect_bool(&c, "CASE WHEN condition")?;
                let v = check_in_scope(val, scope, all)?;
                res_ty = Some(merge_types(res_ty, v.ty, "CASE")?);
                typed_whens.push((c, v));
            }
            let else_typed = match else_ {
                Some(e) => {
                    let e = check_in_scope(e, scope, all)?;
                    res_ty = Some(merge_types(res_ty, e.ty, "CASE")?);
                    Some(Box::new(e))
                }
                None => None,
            };
            let ty = res_ty.unwrap();
            let typed_whens = typed_whens
                .into_iter()
                .map(|(c, v)| (c, coerce_value(v, ty)))
                .collect();
            let else_typed = else_typed.map(|b| Box::new(coerce_value(*b, ty)));
            Ok(TypedExpr::new(
                ty,
                TKind::Case {
                    whens: typed_whens,
                    else_: else_typed,
                },
            ))
        }
        Aggregate { func, arg, filter } => check_aggregate(*func, arg, filter.as_deref(), scope, all),
    }
}

fn check_aggregate(
    func: AggFunc,
    arg: &Expr,
    filter: Option<&Expr>,
    host: &Schema,
    all: &[Schema],
) -> Result<TypedExpr> {
    // Collect the member-scope expression(s): rebase every field path under the
    // collection relation name.
    let mut paths = Vec::new();
    arg.field_paths(&mut paths);
    if let Some(f) = filter {
        f.field_paths(&mut paths);
    }
    if paths.is_empty() {
        return Err(FormulaError::new(format!(
            "{} requires a relationship field to aggregate over",
            func.name()
        )));
    }
    // The collection is the common first segment of every path.
    let collection_name = paths[0].segments[0].clone();
    for p in &paths {
        if p.segments.len() < 2 || p.segments[0] != collection_name {
            return Err(FormulaError::new(format!(
                "in {}, references must all begin with the collection `{collection_name}`",
                func.name()
            )));
        }
    }
    let (target, kind) = relation(all, host, &collection_name).ok_or_else(|| {
        FormulaError::new(format!(
            "`{collection_name}` is not a relation on {}",
            host.uid
        ))
    })?;
    if !is_to_many(kind) {
        return Err(FormulaError::new(format!(
            "aggregate requires a to-many relation, but `{collection_name}` is not one"
        )));
    }
    let collection = Path::new(vec![collection_name.clone()]);

    let rebase = |e: &Expr| rebase_expr(e, &paths, &collection_name);
    let arg_ast = rebase(arg)?;
    let member_arg = check_in_scope(&arg_ast, target, all)?;
    let filter_typed = match filter {
        Some(f) => {
            let f = rebase(f)?;
            let tf = check_in_scope(&f, target, all)?;
            expect_bool(&tf, "aggregate WHERE")?;
            Some(Box::new(tf))
        }
        None => None,
    };

    let ret = match func {
        AggFunc::Count => ValueType::Integer,
        _ => {
            if !member_arg.ty.is_numeric() {
                return Err(FormulaError::type_mismatch(ValueType::Decimal, member_arg.ty)
                    .with_expression(format!(
                        "{} requires a numeric expression",
                        func.name()
                    )));
            }
            match func {
                AggFunc::Avg => match member_arg.ty {
                    ValueType::Integer => ValueType::Decimal,
                    t => t,
                },
                _ => member_arg.ty,
            }
        }
    };

    Ok(TypedExpr::new(
        ret,
        TKind::Aggregate {
            func,
            collection,
            arg: Box::new(member_arg),
            filter: filter_typed,
        },
    ))
}

fn rebase_expr(e: &Expr, all_paths: &[Path], collection: &str) -> Result<Expr> {
    let _ = (all_paths, collection);
    use Expr::*;
    let strip = |p: &Path| -> Path {
        // drop the first (collection) segment
        Path::new(p.segments[1..].to_vec())
    };
    let rebase_one = |p: &Path| -> Expr { Field(strip(p)) };
    // Literals pass through unchanged.
    match e {
        Int(_) | Decimal(_) | Str(_) | Bool(_) => return Ok(e.clone()),
        _ => {}
    }
    // For a bare Field(arg), strip it.
    if let Field(p) = e {
        return Ok(rebase_one(p));
    }
    // Otherwise recurse into combinators.
    match e {
        Unary { op, expr } => Ok(Unary {
            op: *op,
            expr: Box::new(rebase_expr(expr, all_paths, collection)?),
        }),
        Binary { left, op, right } => Ok(Binary {
            left: Box::new(rebase_expr(left, all_paths, collection)?),
            op: *op,
            right: Box::new(rebase_expr(right, all_paths, collection)?),
        }),
        Logical { left, op, right } => Ok(Logical {
            left: Box::new(rebase_expr(left, all_paths, collection)?),
            op: *op,
            right: Box::new(rebase_expr(right, all_paths, collection)?),
        }),
        Not(inner) => Ok(Not(Box::new(rebase_expr(inner, all_paths, collection)?))),
        IsNull { expr, negated } => Ok(IsNull {
            expr: Box::new(rebase_expr(expr, all_paths, collection)?),
            negated: *negated,
        }),
        Coalesce(items) => Ok(Coalesce(
            items
                .iter()
                .map(|i| rebase_expr(i, all_paths, collection))
                .collect::<Result<Vec<_>>>()?,
        )),
        NullIf { a, b } => Ok(NullIf {
            a: Box::new(rebase_expr(a, all_paths, collection)?),
            b: Box::new(rebase_expr(b, all_paths, collection)?),
        }),
        Call { name, args } => Ok(Call {
            name: name.clone(),
            args: args
                .iter()
                .map(|a| rebase_expr(a, all_paths, collection))
                .collect::<Result<Vec<_>>>()?,
        }),
        If {
            cond, then, else_,
        } => Ok(If {
            cond: Box::new(rebase_expr(cond, all_paths, collection)?),
            then: Box::new(rebase_expr(then, all_paths, collection)?),
            else_: Box::new(rebase_expr(else_, all_paths, collection)?),
        }),
        Case { whens, else_ } => {
            let mut nw = Vec::new();
            for (c, v) in whens {
                nw.push((
                    rebase_expr(c, all_paths, collection)?,
                    rebase_expr(v, all_paths, collection)?,
                ));
            }
            let ne = match else_ {
                Some(e) => Some(Box::new(rebase_expr(e, all_paths, collection)?)),
                None => None,
            };
            Ok(Case { whens: nw, else_: ne })
        }
        // Nested aggregates are not supported inside aggregates.
        Aggregate { func, .. } => Err(FormulaError::new(format!(
            "nested aggregate {} inside an aggregate is not supported yet",
            func.name()
        ))),
        _ => Err(FormulaError::new("unsupported expression inside aggregate")),
    }
}

fn check_binary(op: &BinOp, l: TypedExpr, r: TypedExpr) -> Result<TypedExpr> {
    use BinOp::*;
    if op.is_comparison() {
        if !comparable(l.ty, r.ty) {
            return Err(FormulaError::type_mismatch(l.ty, r.ty).with_expression(format!(
                "cannot compare {} with {} using `{}`",
                l.ty, r.ty, op.as_str()
            )));
        }
        return Ok(TypedExpr::new(ValueType::Boolean, TKind::Binary {
            op: *op,
            left: Box::new(l),
            right: Box::new(r),
        }));
    }
    // arithmetic
    if !l.ty.is_numeric() || !r.ty.is_numeric() {
        return Err(FormulaError::type_mismatch(ValueType::Decimal, l.ty).with_expression(format!(
            "operator `{}` requires numeric operands (got {} and {})",
            op.as_str(),
            l.ty,
            r.ty
        )));
    }
    let ty = promote_numeric(l.ty, r.ty).unwrap();
    // `/` always yields a decimal-scale capable type (avoid int truncation).
    let ty = if *op == Div && ty == ValueType::Integer {
        ValueType::Decimal
    } else {
        ty
    };
    Ok(TypedExpr::new(ty, TKind::Binary {
        op: *op,
        left: Box::new(coerce_value(l, ty)),
        right: Box::new(coerce_value(r, ty)),
    }))
}

/// Merge two value types for COALESCE / IF / CASE branches. Numeric types
/// promote along Integer < Decimal < Float; everything else must be identical.
fn merge_types(a: Option<ValueType>, b: ValueType, what: &str) -> Result<ValueType> {
    match a {
        None => Ok(b),
        Some(a) => {
            if a.is_numeric() && b.is_numeric() {
                return promote_numeric(a, b).ok_or_else(|| FormulaError::new("bad numeric"));
            }
            if a != b {
                return Err(FormulaError::type_mismatch(a, b)
                    .with_expression(format!("{what} branches must share a type")));
            }
            Ok(a)
        }
    }
}

fn comparable(a: ValueType, b: ValueType) -> bool {
    if a.is_numeric() && b.is_numeric() {
        return true;
    }
    a == b
}

fn expect_bool(t: &TypedExpr, what: &str) -> Result<()> {
    if t.ty != ValueType::Boolean {
        return Err(FormulaError::type_mismatch(ValueType::Boolean, t.ty)
            .with_expression(format!("{what} must be boolean")));
    }
    Ok(())
}

/// Coerce a value to a target numeric type when the node is an integer/decimal
/// literal that should participate at the promoted type.
fn coerce_value(t: TypedExpr, target: ValueType) -> TypedExpr {
    if t.ty == target {
        return t;
    }
    // Only safe for numeric literals.
    if !t.ty.is_numeric() || !target.is_numeric() {
        return t;
    }
    if t.ty == ValueType::Integer && target == ValueType::Decimal {
        if let TKind::Int(i) = t.kind {
            return TypedExpr::new(target, TKind::Decimal(Decimal::from(i)));
        }
    }
    t
}

// ---------------------------------------------------------------------------
// function registry
// ---------------------------------------------------------------------------

struct FuncSig {
    name: &'static str,
    min_args: Option<usize>,
    max_args: Option<usize>,
    return_fn: fn(&[TypedExpr]) -> Result<ValueType>,
}

fn function_registry(name: &str) -> Result<FuncSig> {
    let list: Vec<FuncSig> = vec![
        FuncSig {
            name: "TODAY",
            min_args: Some(0),
            max_args: Some(0),
            return_fn: |_| Ok(ValueType::Date),
        },
        FuncSig {
            name: "NOW",
            min_args: Some(0),
            max_args: Some(0),
            return_fn: |_| Ok(ValueType::DateTime),
        },
        FuncSig {
            name: "DATE_ADD",
            min_args: Some(2),
            max_args: Some(2),
            return_fn: |a| {
                if !a[0].ty.is_temporal() || !a[1].ty.is_numeric() {
                    Err(FormulaError::new(
                        "DATE_ADD expects a date/datetime and a number of days",
                    ))
                } else {
                    Ok(a[0].ty)
                }
            },
        },
        FuncSig {
            name: "DATE_DIFF",
            min_args: Some(2),
            max_args: Some(2),
            return_fn: |a| {
                if !a[0].ty.is_temporal() || !a[1].ty.is_temporal() {
                    Err(FormulaError::new("DATE_DIFF expects date/datetime args"))
                } else {
                    Ok(ValueType::Integer)
                }
            },
        },
        FuncSig {
            name: "YEAR",
            min_args: Some(1),
            max_args: Some(1),
            return_fn: |a| {
                if !a[0].ty.is_temporal() {
                    Err(FormulaError::new("YEAR expects a date/datetime"))
                } else {
                    Ok(ValueType::Integer)
                }
            },
        },
        FuncSig {
            name: "MONTH",
            min_args: Some(1),
            max_args: Some(1),
            return_fn: |a| {
                if !a[0].ty.is_temporal() {
                    Err(FormulaError::new("MONTH expects a date/datetime"))
                } else {
                    Ok(ValueType::Integer)
                }
            },
        },
        FuncSig {
            name: "DAY",
            min_args: Some(1),
            max_args: Some(1),
            return_fn: |a| {
                if !a[0].ty.is_temporal() {
                    Err(FormulaError::new("DAY expects a date/datetime"))
                } else {
                    Ok(ValueType::Integer)
                }
            },
        },
        FuncSig {
            name: "ABS",
            min_args: Some(1),
            max_args: Some(1),
            return_fn: |a| {
                if !a[0].ty.is_numeric() {
                    Err(FormulaError::type_mismatch(ValueType::Decimal, a[0].ty))
                } else {
                    Ok(a[0].ty)
                }
            },
        },
        FuncSig {
            name: "ROUND",
            min_args: Some(1),
            max_args: Some(2),
            return_fn: |a| {
                if !a[0].ty.is_numeric() {
                    Err(FormulaError::type_mismatch(ValueType::Decimal, a[0].ty))
                } else {
                    Ok(a[0].ty)
                }
            },
        },
        FuncSig {
            name: "UPPER",
            min_args: Some(1),
            max_args: Some(1),
            return_fn: |a| {
                if a[0].ty != ValueType::String {
                    Err(FormulaError::type_mismatch(ValueType::String, a[0].ty))
                } else {
                    Ok(ValueType::String)
                }
            },
        },
    ];
    list.into_iter().find(|f| f.name == name).ok_or_else(|| {
        FormulaError::new(format!("unknown function `{name}`"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use core_domain::{ContentTypeKind, FieldType, RelationKind, Uid};
    use core_schema::{Attribute, SchemaInfo};
    use indexmap::IndexMap;

    fn schema(uid: &str, singular: &str, plural: &str, attrs: Vec<(&str, Attribute)>) -> Schema {
        Schema {
            uid: Uid::new(uid),
            kind: ContentTypeKind::CollectionType,
            collection_name: None,
            info: SchemaInfo {
                singular_name: singular.into(),
                plural_name: plural.into(),
                display_name: singular.into(),
                description: None,
                icon: None,
            },
            options: Default::default(),
            plugin_options: None,
            attributes: attrs
                .into_iter()
                .map(|(n, a)| (n.to_string(), a))
                .collect::<IndexMap<_, _>>(),
            metadata: None,
        }
    }

    fn decimal_attr() -> Attribute {
        Attribute::new(FieldType::Decimal)
    }
    fn int_attr() -> Attribute {
        Attribute::new(FieldType::Integer)
    }
    fn string_attr() -> Attribute {
        Attribute::new(FieldType::String)
    }
    fn date_attr() -> Attribute {
        Attribute::new(FieldType::Date)
    }
    fn relation_attr(kind: RelationKind, target: &str, mapped: Option<&str>) -> Attribute {
        Attribute {
            attr_type: FieldType::Relation,
            relation: Some(kind),
            target: Some(Uid::new(target)),
            mapped_by: mapped.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    fn erp_registry() -> Vec<Schema> {
        let customer = schema(
            "api::customer.customer",
            "customer",
            "customers",
            vec![
                ("name", string_attr()),
                ("credit_limit", decimal_attr()),
                ("sales", relation_attr(RelationKind::OneToMany, "api::sale.sale", Some("customer"))),
            ],
        );
        let sale_line = schema(
            "api::sale-line.sale_line",
            "saleLine",
            "sale_lines",
            vec![
                ("quantity", int_attr()),
                ("unit_price", decimal_attr()),
                ("discount", decimal_attr()),
                (
                    "product",
                    relation_attr(RelationKind::ManyToOne, "api::product.product", None),
                ),
                (
                    "sale",
                    relation_attr(RelationKind::ManyToOne, "api::sale.sale", None),
                ),
            ],
        );
        let sale = schema(
            "api::sale.sale",
            "sale",
            "sales",
            vec![
                ("balance", decimal_attr()),
                ("status", string_attr()),
                ("due_date", date_attr()),
                ("customer", relation_attr(RelationKind::ManyToOne, "api::customer.customer", None)),
                ("lines", relation_attr(RelationKind::OneToMany, "api::sale-line.sale_line", Some("sale"))),
            ],
        );
        let product = schema(
            "api::product.product",
            "product",
            "products",
            vec![("cost", decimal_attr()), ("name", string_attr())],
        );
        vec![customer, sale_line, sale, product]
    }

    fn check_src(src: &str, host_idx: usize, declared: ValueType, all: &[Schema]) -> Result<TypedExpr> {
        let ast = parse(src).unwrap();
        check(&ast, &all[host_idx], all, declared)
    }

    #[test]
    fn scalar_arithmetic_types_ok() {
        let all = erp_registry();
        let t = check_src("(quantity * unit_price) - COALESCE(discount, 0)", 1, ValueType::Decimal, &all).unwrap();
        assert_eq!(t.ty, ValueType::Decimal);
    }

    #[test]
    fn sum_over_lines_ok() {
        let all = erp_registry();
        // subtotal on sale = SUM(lines.net_price); net_price defined later via a
        // formula on sale_line (referenced by path only).
        let t = check_src("SUM(lines.unit_price)", 2, ValueType::Decimal, &all).unwrap();
        match t.kind {
            TKind::Aggregate { func, .. } => assert_eq!(func, AggFunc::Sum),
            _ => panic!("expected aggregate"),
        }
    }

    #[test]
    fn rejects_mul_string() {
        let all = erp_registry();
        let err = check_src("quantity * product.name", 1, ValueType::Decimal, &all).unwrap_err();
        assert!(err.message.contains("numeric") || err.message.contains("type"));
    }

    #[test]
    fn rejects_quantity_plus_true() {
        let all = erp_registry();
        assert!(check_src("quantity + true", 0, ValueType::Decimal, &all).is_err());
    }

    #[test]
    fn rejects_unknown_function() {
        let all = erp_registry();
        let err = check_src("UNKNOWN_FUNCTION(quantity)", 0, ValueType::Decimal, &all).unwrap_err();
        assert!(err.message.contains("unknown function"), "{err:?}");
    }

    #[test]
    fn rejects_unknown_relation() {
        let all = erp_registry();
        assert!(check_src("SUM(non_existing_relation.total)", 2, ValueType::Decimal, &all).is_err());
    }

    #[test]
    fn rejects_unknown_field() {
        let all = erp_registry();
        assert!(check_src("product.non_existing_field", 1, ValueType::Decimal, &all).is_err());
    }

    #[test]
    fn rejects_arithmetic_on_boolean() {
        let all = erp_registry();
        // force an invalid op using literal true in an aggregate arg position:
        assert!(check_src("SUM(lines.quantity + true)", 2, ValueType::Decimal, &all).is_err());
    }

    #[test]
    fn field_crosses_collection_outside_aggregate_is_rejected() {
        let all = erp_registry();
        // sale.total (not present) -> using `lines.balance` as scalar on sale:
        assert!(check_src("lines.balance", 2, ValueType::Decimal, &all).is_err());
    }
}
