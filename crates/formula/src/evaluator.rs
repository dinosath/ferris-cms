//! Runtime (in-memory) evaluator.
//!
//! Intended for unit testing, previews, and validation — not as a replacement
//! for SQL when an expression can be evaluated efficiently in the database. It
//! evaluates scalar formulas against an in-memory record using exact `Decimal`
//! arithmetic so monetary calculations do not introduce binary floating point
//! errors.

use crate::ast::{BinOp, Expr, Logic, UnOp};
use crate::errors::{FormulaError, Result};
use crate::types::{promote_numeric, Value, ValueType};
use chrono::{Datelike, Duration, Utc};
use indexmap::IndexMap;
use rust_decimal::Decimal;

/// A flat record whose scalar fields are keyed by schema attribute name.
pub type Row = IndexMap<String, Value>;

/// Evaluate a scalar formula AST against a record. Referenced computed fields
/// must already be materialized into `row` (they usually are when the caller
/// walks the formula chain bottom-up).
pub fn eval_scalar(ast: &Expr, row: &Row) -> Result<Value> {
    eval(ast, row)
}

fn eval(ast: &Expr, row: &Row) -> Result<Value> {
    use Expr::*;
    match ast {
        Int(i) => Ok(Value::Integer(*i)),
        Decimal(d) => Ok(Value::Decimal(*d)),
        Str(s) => Ok(Value::String(s.clone())),
        Bool(b) => Ok(Value::Boolean(*b)),
        Field(p) => field_value(p, row),
        Unary { op: UnOp::Not, expr } => {
            let t = truthy(&eval(expr, row)?)?;
            Ok(Value::Boolean(!t))
        }
        Unary { op: UnOp::Neg, expr } => {
            let v = eval(expr, row)?;
            neg(v)
        }
        Binary { left, op, right } => {
            let l = eval(left, row)?;
            let r = eval(right, row)?;
            binop(*op, &l, &r)
        }
        Logical { left, op, right } => {
            // short-circuit
            match op {
                Logic::And => {
                    let l = truthy(&eval(left, row)?)?;
                    if !l {
                        return Ok(Value::Boolean(false));
                    }
                    let r = truthy(&eval(right, row)?)?;
                    Ok(Value::Boolean(r))
                }
                Logic::Or => {
                    let l = truthy(&eval(left, row)?)?;
                    if l {
                        return Ok(Value::Boolean(true));
                    }
                    let r = truthy(&eval(right, row)?)?;
                    Ok(Value::Boolean(r))
                }
            }
        }
        Not(e) => {
            let t = truthy(&eval(e, row)?)?;
            Ok(Value::Boolean(!t))
        }
        IsNull { expr, negated } => {
            let v = eval(expr, row)?;
            let is_null = v.is_null();
            Ok(Value::Boolean(if *negated { !is_null } else { is_null }))
        }
        Coalesce(items) => {
            for it in items {
                let v = eval(it, row)?;
                if !v.is_null() {
                    return Ok(v);
                }
            }
            Ok(Value::Null)
        }
        NullIf { a, b } => {
            let a = eval(a, row)?;
            let b = eval(b, row)?;
            if values_equal(&a, &b)? {
                Ok(Value::Null)
            } else {
                Ok(a)
            }
        }
        Call { name, args } => eval_function(name, args, row),
        If {
            cond, then, else_,
        } => {
            if truthy(&eval(cond, row)?)? {
                eval(then, row)
            } else {
                eval(else_, row)
            }
        }
        Case { whens, else_ } => {
            for (c, v) in whens {
                if truthy(&eval(c, row)?)? {
                    return eval(v, row);
                }
            }
            match else_ {
                Some(e) => eval(e, row),
                None => Ok(Value::Null),
            }
        }
        // Aggregates require collections; the SQL path / caller materializes.
        Aggregate { func, .. } => Err(FormulaError::new(format!(
            "aggregate {} cannot be evaluated against a single record; use the SQL path",
            func.name()
        ))),
    }
}

fn field_value(p: &crate::ast::Path, row: &Row) -> Result<Value> {
    if p.segments.len() == 1 {
        return Ok(row.get(&p.segments[0]).cloned().unwrap_or(Value::Null));
    }
    Err(FormulaError::new(format!(
        "relationship path `{}` cannot be evaluated against a flat record",
        p.join(".")
    )))
}

fn truthy(v: &Value) -> Result<bool> {
    match v {
        Value::Null => Ok(false),
        Value::Boolean(b) => Ok(*b),
        _ => Err(FormulaError::new("expected a boolean value")),
    }
}

fn neg(v: Value) -> Result<Value> {
    match v {
        Value::Null => Ok(Value::Null),
        Value::Integer(i) => Ok(Value::Integer(-i)),
        Value::Decimal(d) => Ok(Value::Decimal(-d)),
        Value::Float(f) => Ok(Value::Float(-f)),
        other => Err(FormulaError::new(format!(
            "cannot negate non-numeric value {other}"
        ))),
    }
}

fn numeric(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Decimal(d) => d.to_string().parse().ok(),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn binop(op: BinOp, l: &Value, r: &Value) -> Result<Value> {
    if l.is_null() || r.is_null() {
        return Ok(Value::Null); // three-valued: arithmetic with NULL is NULL
    }
    use BinOp::*;
    if op.is_arith() {
        return arith(op, l, r);
    }
    // comparisons
    let res = match (l, r) {
        (Value::Boolean(a), Value::Boolean(b)) => match op {
            Eq => a == b,
            Ne => a != b,
            _ => {
                return Err(FormulaError::new("boolean only supports = / !="));
            }
        },
        _ => {
            // numeric promotion
            if numeric(l).is_some() && numeric(r).is_some() {
                let a = numeric(l).unwrap();
                let b = numeric(r).unwrap();
                cmp_f64(op, a, b)
            } else {
                // string / date equality & ordering
                let s = cmp_other(op, l, r)?;
                s
            }
        }
    };
    Ok(Value::Boolean(res))
}

fn cmp_f64(op: BinOp, a: f64, b: f64) -> bool {
    use BinOp::*;
    match op {
        Eq => a == b,
        Ne => a != b,
        Gt => a > b,
        Gte => a >= b,
        Lt => a < b,
        Lte => a <= b,
        _ => unreachable!(),
    }
}

fn cmp_other(op: BinOp, l: &Value, r: &Value) -> Result<bool> {
    use BinOp::*;
    // Compare by display for date/datetime/string lexicographic semantics.
    let ord = |v: &Value| match v {
        Value::String(s) => format!("0{s}"),
        Value::Date(d) => format!("1{d}"),
        Value::DateTime(d) => format!("2{d}"),
        other => format!("3{other}"),
    };
    if matches!(op, Eq | Ne) {
        let eq = values_equal(l, r)?;
        return Ok(if op == Eq { eq } else { !eq });
    }
    let a = ord(l);
    let b = ord(r);
    Ok(match op {
        Gt => a > b,
        Gte => a >= b,
        Lt => a < b,
        Lte => a <= b,
        _ => unreachable!(),
    })
}

fn values_equal(a: &Value, b: &Value) -> Result<bool> {
    // Compare numerically when both numeric (Decimal vs Int).
    if numeric(a).is_some() && numeric(b).is_some() {
        return Ok(numeric(a).unwrap() == numeric(b).unwrap());
    }
    Ok(a == b)
}

fn arith(op: BinOp, l: &Value, r: &Value) -> Result<Value> {
    use BinOp::*;
    let lt = l.value_type().unwrap();
    let rt = r.value_type().unwrap();
    let target = promote_numeric(lt, rt).unwrap();
    let a = l.coerce_numeric(target).ok_or_else(|| type_err())?;
    let b = r.coerce_numeric(target).ok_or_else(|| type_err())?;

    // Division always yields Decimal to preserve precision for money.
    let decimal_div = op == Div && target != ValueType::Float;

    match (target, op) {
        (ValueType::Integer, Add) => {
            if let (Value::Integer(x), Value::Integer(y)) = (a, b) {
                return Ok(Value::Integer(x.wrapping_add(y)));
            }
        }
        (ValueType::Integer, Sub) => {
            if let (Value::Integer(x), Value::Integer(y)) = (a, b) {
                return Ok(Value::Integer(x.wrapping_sub(y)));
            }
        }
        (ValueType::Integer, Mul) => {
            if let (Value::Integer(x), Value::Integer(y)) = (a, b) {
                return Ok(Value::Integer(x.wrapping_mul(y)));
            }
        }
        (ValueType::Integer, Mod) => {
            if let (Value::Integer(x), Value::Integer(y)) = (a, b) {
                if y == 0 {
                    return Ok(Value::Null);
                }
                return Ok(Value::Integer(x % y));
            }
        }
        (ValueType::Integer, Div) => {
            // fallthrough to decimal path via decimal_div
        }
        (ValueType::Decimal, _) if matches!(op, Add | Sub | Mul | Div | Mod) => {
            let dx = as_dec(&a);
            let dy = as_dec(&b);
            if decimal_div {
                if dy.is_zero() {
                    return Ok(Value::Null);
                }
                return Ok(Value::Decimal(dx / dy));
            }
            let v = match op {
                Add => dx + dy,
                Sub => dx - dy,
                Mul => dx * dy,
                Mod => {
                    if dy.is_zero() {
                        return Ok(Value::Null);
                    }
                    dx % dy
                }
                _ => unreachable!(),
            };
            return Ok(Value::Decimal(v));
        }
        (ValueType::Float, _) => {
            let x = numeric(&a).unwrap();
            let y = numeric(&b).unwrap();
            let v = match op {
                Add => x + y,
                Sub => x - y,
                Mul => x * y,
                Div => {
                    if y == 0.0 {
                        return Ok(Value::Null);
                    }
                    x / y
                }
                Mod => x % y,
                _ => unreachable!(),
            };
            return Ok(Value::Float(v));
        }
        _ => {}
    }
    Err(type_err())
}

fn as_dec(v: &Value) -> Decimal {
    match v {
        Value::Decimal(d) => *d,
        Value::Integer(i) => Decimal::from(*i),
        _ => Decimal::ZERO,
    }
}

fn type_err() -> FormulaError {
    FormulaError::new("type error in arithmetic")
}

fn eval_function(name: &str, args: &[Expr], row: &Row) -> Result<Value> {
    match name {
        "TODAY" => Ok(Value::Date(Utc::now().date_naive())),
        "NOW" => Ok(Value::DateTime(Utc::now())),
        "DATE_ADD" => {
            let d = eval(&args[0], row)?;
            let n = eval(&args[1], row)?;
            let days = match n {
                Value::Integer(i) => i,
                Value::Decimal(d) => d.trunc().to_string().parse().unwrap_or(0),
                Value::Float(f) => f.trunc() as i64,
                _ => return Err(FormulaError::new("DATE_ADD needs a day count")),
            };
            match d {
                Value::Date(date) => Ok(Value::Date(date + Duration::days(days))),
                Value::DateTime(dt) => Ok(Value::DateTime(dt + Duration::days(days))),
                _ => Err(FormulaError::new("DATE_ADD needs a date/datetime")),
            }
        }
        "DATE_DIFF" => {
            let a = eval(&args[0], row)?;
            let b = eval(&args[1], row)?;
            let days = match (a, b) {
                (Value::Date(x), Value::Date(y)) => (x - y).num_days(),
                (Value::DateTime(x), Value::DateTime(y)) => {
                    (x.date_naive() - y.date_naive()).num_days()
                }
                _ => return Err(FormulaError::new("DATE_DIFF needs date/datetime args")),
            };
            Ok(Value::Integer(days))
        }
        "YEAR" | "MONTH" | "DAY" => {
            let v = eval(&args[0], row)?;
            let d = match v {
                Value::Date(date) => date,
                Value::DateTime(dt) => dt.date_naive(),
                _ => return Err(FormulaError::new("needs a date/datetime")),
            };
            Ok(Value::Integer(match name {
                "YEAR" => d.year() as i64,
                "MONTH" => d.month() as i64,
                _ => d.day() as i64,
            }))
        }
        "ABS" => {
            let v = eval(&args[0], row)?;
            match v {
                Value::Integer(i) => Ok(Value::Integer(i.abs())),
                Value::Decimal(d) => Ok(Value::Decimal(d.abs())),
                Value::Float(f) => Ok(Value::Float(f.abs())),
                Value::Null => Ok(Value::Null),
                _ => Err(FormulaError::new("ABS needs a number")),
            }
        }
        "ROUND" => {
            let v = eval(&args[0], row)?;
            let scale = if args.len() > 1 {
                match eval(&args[1], row)? {
                    Value::Integer(i) => i as u32,
                    _ => 0,
                }
            } else {
                0
            };
            match v {
                Value::Decimal(d) => Ok(Value::Decimal(d.round_dp(scale))),
                Value::Float(f) => {
                    let factor = 10f64.powi(scale as i32);
                    Ok(Value::Float((f * factor).round() / factor))
                }
                Value::Integer(i) => Ok(Value::Integer(i)),
                Value::Null => Ok(Value::Null),
                _ => Err(FormulaError::new("ROUND needs a number")),
            }
        }
        "UPPER" => {
            let v = eval(&args[0], row)?;
            match v {
                Value::String(s) => Ok(Value::String(s.to_uppercase())),
                Value::Null => Ok(Value::Null),
                _ => Err(FormulaError::new("UPPER needs a string")),
            }
        }
        _ => Err(FormulaError::new(format!("unknown function {name}"))),
    }
}

/// Convenience: run a parser + scalar evaluation.
pub fn evaluate(src: &str, row: &Row) -> Result<Value> {
    let ast = crate::parser::parse(src)?;
    eval(&ast, row)
}

/// A small helper to run a bottom-up formula chain over scalar rows.
pub fn evaluate_chain<'a>(
    expressions: impl IntoIterator<Item = (&'a str, &'a str)>,
    base: Row,
) -> Result<Row> {
    let mut row = base;
    for (field, expr) in expressions {
        let ast = crate::parser::parse(expr)?;
        let v = eval(&ast, &row)?;
        row.insert(field.to_string(), v);
    }
    Ok(row)
}

/// Exact decimal arithmetic helper used by tests (avoids f64).
pub fn dec(s: &str) -> Result<Decimal> {
    s.parse().map_err(|e| FormulaError::new(format!("bad decimal: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: Vec<(&str, i64)>) -> Row {
        pairs.into_iter().map(|(k, v)| (k.to_string(), Value::Integer(v))).collect()
    }

    #[test]
    fn decimal_arithmetic_is_exact() {
        // 19.99 * 1.24 must not introduce binary floating point error.
        let r = row(vec![("net_price", 19)]);
        let _ = r;
        let mut base = Row::new();
        base.insert("net_price".into(), Value::Decimal(dec("19.99").unwrap()));
        base.insert("vat_rate".into(), Value::Integer(24));
        // vat = net_price * 24 / 100
        let v = evaluate("net_price * vat_rate / 100", &base).unwrap();
        match v {
            Value::Decimal(d) => assert_eq!(d.to_string(), "4.7976"),
            other => panic!("expected decimal, got {other}"),
        }
        // 7.125 * 100 == 712.5 exactly
        base.insert("a".into(), Value::Decimal(dec("7.125").unwrap()));
        base.insert("b".into(), Value::Decimal(dec("100").unwrap()));
        let v = evaluate("a * b", &base).unwrap();
        match v {
            Value::Decimal(d) => assert_eq!(d.to_string(), "712.500"),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn net_price_exact_scenario() {
        // (quantity * unit_price) - discount with quantity 10 unit 8.00 discount 5.00
        let mut base = Row::new();
        base.insert("quantity".into(), Value::Integer(10));
        base.insert("unit_price".into(), Value::Decimal(dec("8.00").unwrap()));
        base.insert("discount".into(), Value::Decimal(dec("5.00").unwrap()));
        let v = evaluate("(quantity * unit_price) - COALESCE(discount, 0)", &base).unwrap();
        match v {
            Value::Decimal(d) => assert_eq!(d.to_string(), "75.00"),
            other => panic!("expected decimal got {other}"),
        }
    }

    #[test]
    fn coalesce_null_handling() {
        let mut base = Row::new();
        base.insert("discount".into(), Value::Null);
        base.insert("quantity".into(), Value::Integer(2));
        base.insert("price".into(), Value::Decimal(dec("10").unwrap()));
        // COALESCE(NULL, 0) => 0
        let v = evaluate("COALESCE(discount, 0)", &base).unwrap();
        assert_eq!(v, Value::Integer(0));
        // 2 * 10 - 0
        let v = evaluate("quantity * price - COALESCE(discount, 0)", &base).unwrap();
        match v {
            Value::Decimal(d) => assert_eq!(d.to_string(), "20"),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn scalar_chain_bottom_up() {
        // net_price -> vat_amount -> final_price
        let mut base = Row::new();
        base.insert("quantity".into(), Value::Integer(10));
        base.insert("unit_price".into(), Value::Decimal(dec("8.00").unwrap()));
        base.insert("discount".into(), Value::Decimal(dec("5.00").unwrap()));
        base.insert("vat_rate".into(), Value::Integer(24));
        let net = evaluate("(quantity * unit_price) - COALESCE(discount, 0)", &base).unwrap();
        base.insert("net_price".into(), net);
        let vat = evaluate("net_price * vat_rate / 100", &base).unwrap();
        base.insert("vat_amount".into(), vat);
        let final_price_val = evaluate("net_price + vat_amount", &base).unwrap();
        assert_eq!(format!("{final_price_val}"), "93.00");
    }

    #[test]
    fn comparison_and_if() {
        let mut base = Row::new();
        base.insert("quantity".into(), Value::Integer(5));
        base.insert("unit_price".into(), Value::Decimal(dec("2").unwrap()));
        let v = evaluate("IF(quantity > 0, quantity * unit_price, 0)", &base).unwrap();
        assert_eq!(format!("{v}"), "10");
        let v = evaluate("IF(quantity < 0, 1, 0)", &base).unwrap();
        assert_eq!(v, Value::Integer(0));
    }

    #[test]
    fn case_expression() {
        let mut base = Row::new();
        base.insert("quantity".into(), Value::Integer(3));
        let v = evaluate("CASE WHEN quantity > 10 THEN 1 ELSE 0 END", &base).unwrap();
        assert_eq!(v, Value::Integer(0));
    }

    #[test]
    fn string_equals_filter_semantics() {
        let mut base = Row::new();
        base.insert("status".into(), Value::String("OPEN".into()));
        let v = evaluate("status = \"OPEN\"", &base).unwrap();
        assert_eq!(v, Value::Boolean(true));
    }

    #[test]
    fn date_functions() {
        let mut base = Row::new();
        base.insert("due_date".into(), Value::Date(chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()));
        base.insert("today".into(), Value::Date(chrono::Utc::now().date_naive()));
        let v = evaluate("due_date < TODAY()", &base).unwrap();
        assert_eq!(v, Value::Boolean(true));
        let v = evaluate("DATE_DIFF(TODAY(), due_date) > 0", &base).unwrap();
        assert_eq!(v, Value::Boolean(true));
        let y = evaluate("YEAR(due_date)", &base).unwrap();
        assert_eq!(y, Value::Integer(2020));
    }

    #[test]
    fn is_null_predicate() {
        let mut base = Row::new();
        base.insert("discount".into(), Value::Null);
        assert_eq!(evaluate("discount IS NULL", &base).unwrap(), Value::Boolean(true));
        assert_eq!(evaluate("discount IS NOT NULL", &base).unwrap(), Value::Boolean(false));
    }
}
