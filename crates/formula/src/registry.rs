//! Relationship-aware resolution over Ferris content-type metadata.
//!
//! The formula compiler must understand Ferris relationships (manyToOne,
//! oneToMany, manyToMany, ...) by reading the schema registry — never by
//! hard-coding entity names. This module resolves field paths and relationship
//! hops against the `core_schema::Schema` model.

use crate::errors::{FormulaError, Result};
use crate::types::ValueType;
use core_domain::{FieldType, RelationKind, Uid};
use core_schema::{ExecutionStrategy, FormulaConfig, FormulaType, Schema};

/// Look up a schema by uid.
pub fn by_uid<'a>(all: &'a [Schema], uid: &Uid) -> Option<&'a Schema> {
    all.iter().find(|s| &s.uid == uid)
}

/// Look up a schema index by uid.
pub fn index_of(all: &[Schema], uid: &Uid) -> Option<usize> {
    all.iter().position(|s| &s.uid == uid)
}

/// Map a persisted `FormulaType` to an engine `ValueType`.
pub fn value_type_of_formula(ft: FormulaType) -> ValueType {
    use FormulaType::*;
    match ft {
        Integer => ValueType::Integer,
        Decimal => ValueType::Decimal,
        Float => ValueType::Float,
        Boolean => ValueType::Boolean,
        String => ValueType::String,
        Date => ValueType::Date,
        Datetime => ValueType::DateTime,
        Uuid => ValueType::Uuid,
    }
}

/// The engine `ValueType` of a scalar field attribute (including formula
/// fields, whose declared return type is used). Returns `None` for relation /
/// component / media attributes which do not have a scalar type of their own.
pub fn attribute_value_type(schema: &Schema, name: &str) -> Option<ValueType> {
    let attr = schema.attributes.get(name)?;
    if let Some(cfg) = &attr.formula {
        return Some(value_type_of_formula(cfg.return_type));
    }
    ValueType::from_field_type(attr.attr_type)
}

/// The persisted formula config of a field, if it is a computed field.
pub fn formula_config<'a>(schema: &'a Schema, name: &'a str) -> Option<&'a FormulaConfig> {
    schema.attributes.get(name).and_then(|a| a.formula.as_ref())
}

/// Whether a field is a computed formula field.
pub fn is_formula_field(schema: &Schema, name: &str) -> bool {
    schema.attributes.get(name).is_some_and(|a| a.formula.is_some())
}

/// Resolve a relation attribute on `schema` to its target schema + kind.
pub fn relation<'a>(
    all: &'a [Schema],
    schema: &Schema,
    name: &str,
) -> Option<(&'a Schema, RelationKind)> {
    let attr = schema.attributes.get(name)?;
    if attr.attr_type != FieldType::Relation {
        return None;
    }
    let kind = attr.relation?;
    let uid = attr.target.as_ref()?;
    let target = by_uid(all, uid)?;
    Some((target, kind))
}

/// Whether a relation kind is a "to-many" collection from the owning schema's
/// perspective (oneToMany inverse, manyToMany, manyWay).
pub fn is_to_many(kind: RelationKind) -> bool {
    !kind.owner_has_fk() || kind.uses_join_table()
}

/// The FK column name for a relation field stored on this schema's own table.
pub fn relation_fk_column(attr: &str) -> String {
    core_domain::fk_column(attr)
}

/// The physical column for a scalar field attribute.
pub fn scalar_column(attr: &str) -> String {
    core_domain::column_name(attr)
}

/// Walk a (rebased, scope-relative) dotted path against `scope`, returning the
/// leaf scalar type and whether the path crossed any collection.
///
/// `allow_leading_collection` permits the *first* segment to be a to-many
/// relation that has already been iterated by an enclosing aggregate (so a
/// member expression can still reference fields that keep the collection's
/// name as their first segment). Deeper collection crossings are always
/// rejected.
pub fn walk_path(
    all: &[Schema],
    scope: &Schema,
    path: &[String],
    allow_leading_collection: bool,
) -> Result<Walked> {
    if path.is_empty() {
        return Err(FormulaError::new("empty field reference"));
    }
    let mut current = scope;
    let mut crossed_collection = false;
    let n = path.len();
    for (i, seg) in path.iter().enumerate() {
        let last = i == n - 1;
        // Is `seg` a relation on `current`?
        let attr = current.attributes.get(seg).ok_or_else(|| {
            FormulaError::new(format!(
                "unknown field `{seg}` on {} (of path `{}`)",
                current.uid,
                path.join(".")
            ))
        })?;
        if attr.formula.is_some() {
            if !last {
                return Err(FormulaError::new(format!(
                    "cannot traverse through computed field `{seg}`"
                )));
            }
            return Ok(Walked {
                value_type: value_type_of_formula(attr.formula.as_ref().unwrap().return_type),
                crossed_collection,
            });
        }
        if attr.attr_type == FieldType::Relation {
            let kind = attr.relation.ok_or_else(|| {
                FormulaError::new(format!("relation field `{seg}` has no relation kind"))
            })?;
            let uid = attr.target.clone().ok_or_else(|| {
                FormulaError::new(format!("relation field `{seg}` has no target"))
            })?;
            let target = by_uid(all, &uid).ok_or_else(|| {
                FormulaError::new(format!(
                    "relation `{seg}` points at unknown schema {uid}"
                ))
            })?;
            let to_many = is_to_many(kind);
            if to_many {
                if !crossed_collection {
                    if !allow_leading_collection || i != 0 {
                        // A to-many used where a scalar is required.
                        return Err(FormulaError::new(format!(
                            "field `{}` is a collection ({}), not a scalar",
                            seg,
                            name_of_kind(kind)
                        )));
                    }
                    crossed_collection = true;
                } else {
                    return Err(FormulaError::new(format!(
                        "nested collection traversal through `{seg}` is not supported"
                    )));
                }
            }
            current = target;
            continue;
        }
        // scalar attribute
        if !last {
            return Err(FormulaError::new(format!(
                "cannot traverse into scalar field `{seg}`"
            )));
        }
        let vt = ValueType::from_field_type(attr.attr_type).ok_or_else(|| {
            FormulaError::new(format!("field `{seg}` is not a scalar value in a formula"))
        })?;
        return Ok(Walked {
            value_type: vt,
            crossed_collection,
        });
    }
    Err(FormulaError::new(format!(
        "field path `{}` did not resolve to a scalar",
        path.join(".")
    )))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Walked {
    pub value_type: ValueType,
    pub crossed_collection: bool,
}

/// Determine the execution strategy a compiler would use for a formula tree.
/// Pure heuristic used for metadata; the compiler may refine it.
pub fn execution_strategy(ast: &crate::ast::Expr) -> ExecutionStrategy {
    let mut has_aggregate = false;
    let mut has_relation = false;
    classify(ast, &mut has_aggregate, &mut has_relation);
    if has_aggregate {
        ExecutionStrategy::SqlAggregateQuery
    } else if has_relation {
        ExecutionStrategy::SqlExpression
    } else {
        ExecutionStrategy::GeneratedColumn
    }
}

fn classify(e: &crate::ast::Expr, agg: &mut bool, rel: &mut bool) {
    use crate::ast::Expr::*;
    match e {
        Field(p) => {
            if p.segments.len() > 1 {
                *rel = true;
            }
        }
        Unary { expr, .. } => classify(expr, agg, rel),
        Binary { left, right, .. } => {
            classify(left, agg, rel);
            classify(right, agg, rel);
        }
        Logical { left, right, .. } => {
            classify(left, agg, rel);
            classify(right, agg, rel);
        }
        Not(inner) => classify(inner, agg, rel),
        IsNull { expr, .. } => classify(expr, agg, rel),
        Coalesce(items) => items.iter().for_each(|i| classify(i, agg, rel)),
        NullIf { a, b } => {
            classify(a, agg, rel);
            classify(b, agg, rel);
        }
        Call { args, .. } => args.iter().for_each(|a| classify(a, agg, rel)),
        If {
            cond, then, else_, ..
        } => {
            classify(cond, agg, rel);
            classify(then, agg, rel);
            classify(else_, agg, rel);
        }
        Case { whens, else_ } => {
            for (c, v) in whens {
                classify(c, agg, rel);
                classify(v, agg, rel);
            }
            if let Some(e) = else_ {
                classify(e, agg, rel);
            }
        }
        Aggregate { .. } => *agg = true,
        _ => {}
    }
}

fn name_of_kind(k: RelationKind) -> &'static str {
    match k {
        RelationKind::OneToMany => "oneToMany",
        RelationKind::ManyToMany => "manyToMany",
        RelationKind::ManyWay => "manyWay",
        RelationKind::ManyToOne => "manyToOne",
        RelationKind::OneToOne => "oneToOne",
        RelationKind::OneWay => "oneWay",
    }
}

pub fn _unused(_: &Uid) {}
