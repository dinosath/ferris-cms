//! Dependency analysis over formula fields.
//!
//! Formulas can depend on other formulas (`final_price` depends on
//! `net_price`/`vat_amount`), on scalar inputs, and across relationships
//! (`Customer.total_revenue = SUM(sales.total)`). This module builds the
//! dependency edges between formula fields, produces the human-facing
//! dependency list used for metadata, and detects circular definitions.

use crate::ast::Expr;
use crate::errors::{FormulaError, Result};
use crate::registry::{by_uid, formula_config};
use crate::types::ValueType;
use core_schema::Schema;
use std::collections::{BTreeMap, BTreeSet};

/// A resolved reference from one formula to another formula field.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FormulaDep {
    /// uid of the schema that owns the referenced formula field.
    pub schema_uid: String,
    /// The referenced formula field name.
    pub field: String,
    /// The original dotted path as written in the source (for metadata).
    pub path: String,
}

/// Dependencies discovered for one formula field.
#[derive(Clone, Debug, Default)]
pub struct FieldDeps {
    /// Formula fields this expression depends on.
    pub formulas: BTreeSet<FormulaDep>,
    /// Relationship paths traversed (e.g. `sales`, `sales.lines`).
    pub relations: BTreeSet<String>,
}

/// A formula field definition discovered in the registry.
#[derive(Clone, Debug)]
struct Def {
    schema: String,
    field: String,
    deps: FieldDeps,
}

impl Def {
    fn key(&self) -> (String, String) {
        (self.schema.clone(), self.field.clone())
    }
}

/// Analyze every formula attribute across the registry, building a dependency
/// graph among formula fields and reporting any cycle.
pub fn analyze(all: &[Schema]) -> Result<DependencyReport> {
    let mut defs: Vec<Def> = Vec::new();
    for schema in all {
        for (field, attr) in &schema.attributes {
            if let Some(cfg) = &attr.formula {
                let ast = crate::parser::parse(&cfg.expression).map_err(|e| {
                    e.with_expression(format!(
                        "{}.{} = {}",
                        schema.uid, field, cfg.expression
                    ))
                })?;
                let deps = collect_field_deps(all, schema, &ast)?;
                defs.push(Def {
                    schema: schema.uid.to_string(),
                    field: field.clone(),
                    deps,
                });
            }
        }
    }

    // Build an index key -> def
    let mut by_key: BTreeMap<(String, String), &Def> = BTreeMap::new();
    for d in &defs {
        by_key.insert(d.key(), d);
    }

    // DFS cycle detection only along edges that target an actual formula def.
    let mut visiting: BTreeSet<(String, String)> = BTreeSet::new();
    let mut visited: BTreeSet<(String, String)> = BTreeSet::new();
    let mut stack: Vec<(String, String)> = Vec::new();

    fn visit(
        key: (String, String),
        by_key: &BTreeMap<(String, String), &Def>,
        visiting: &mut BTreeSet<(String, String)>,
        visited: &mut BTreeSet<(String, String)>,
        stack: &mut Vec<(String, String)>,
    ) -> Result<()> {
        if visited.contains(&key) {
            return Ok(());
        }
        if visiting.contains(&key) {
            // found a cycle
            let start = stack.iter().position(|k| k == &key).unwrap_or(0);
            let cycle: Vec<String> = stack[start..]
                .iter()
                .map(|(s, f)| format!("{s}.{f}"))
                .chain(std::iter::once(format!("{}.{}", key.0, key.1)))
                .collect();
            return Err(FormulaError::new(format!(
                "circular formula dependency detected: {}",
                cycle.join(" -> ")
            )));
        }
        visiting.insert(key.clone());
        stack.push(key.clone());
        if let Some(def) = by_key.get(&key) {
            for dep in &def.deps.formulas {
                let dep_key = (dep.schema_uid.clone(), dep.field.clone());
                if by_key.contains_key(&dep_key) {
                    visit(dep_key, by_key, visiting, visited, stack)?;
                }
            }
        }
        stack.pop();
        visiting.remove(&key);
        visited.insert(key);
        Ok(())
    }

    // only iterate over defs sorted for determinism
    let mut keys: Vec<(String, String)> = defs.iter().map(Def::key).collect();
    keys.sort();
    for key in keys {
        visit(key, &by_key, &mut visiting, &mut visited, &mut stack)?;
    }

    let fields: BTreeMap<(String, String), (ValueType, FieldDeps)> = defs
        .iter()
        .map(|d| {
            let vt = formula_config(
                by_uid(all, &core_domain::Uid::new(d.schema.clone())).expect("schema"),
                &d.field,
            )
            .map(|c| crate::registry::value_type_of_formula(c.return_type))
            .unwrap_or(ValueType::Decimal);
            (d.key(), (vt, d.deps.clone()))
        })
        .collect();

    Ok(DependencyReport { fields })
}

/// Collect the formula/relation dependencies of one expression.
pub fn collect_field_deps(all: &[Schema], host: &Schema, ast: &Expr) -> Result<FieldDeps> {
    let mut paths = Vec::new();
    ast.field_paths(&mut paths);
    let mut out = FieldDeps::default();
    for p in &paths {
        let target = resolve_path_target(all, host, &p.segments)?;
        let path_str = p.join(".");
        // Record relation traversal (more than one segment -> relationship).
        if p.segments.len() > 1 {
            out.relations.insert(p.segments[..p.segments.len() - 1].join("."));
        }
        if let Some(schema) = by_uid(all, &core_domain::Uid::new(target.0.clone())) {
            if formula_config(schema, &target.1).is_some() {
                out.formulas.insert(FormulaDep {
                    schema_uid: target.0,
                    field: target.1,
                    path: path_str,
                });
            }
        }
    }
    Ok(out)
}

/// Resolve the owning (schema_uid, field) of a dotted path starting at `host`.
fn resolve_path_target(
    all: &[Schema],
    host: &Schema,
    path: &[String],
) -> Result<(String, String)> {
    if path.is_empty() {
        return Err(FormulaError::new("empty path"));
    }
    let mut current = host;
    let n = path.len();
    for (i, seg) in path.iter().enumerate() {
        let last = i == n - 1;
        let attr = current.attributes.get(seg).ok_or_else(|| {
            FormulaError::new(format!(
                "unknown field `{seg}` on {} while resolving `{}`",
                current.uid,
                path.join(".")
            ))
        })?;
        if !last {
            // must be a relation
            let kind = attr
                .relation
                .ok_or_else(|| FormulaError::new(format!("`{seg}` is not a relation")))?;
            let _ = kind;
            let target_uid = attr
                .target
                .clone()
                .ok_or_else(|| FormulaError::new(format!("`{seg}` relation has no target")))?;
            current = by_uid(all, &target_uid).ok_or_else(|| {
                FormulaError::new(format!("relation `{seg}` points to unknown {target_uid}"))
            })?;
        }
    }
    let leaf = path.last().unwrap();
    Ok((current.uid.to_string(), leaf.clone()))
}

/// The result of analyzing a schema registry's formulas.
#[derive(Clone, Debug, Default)]
pub struct DependencyReport {
    /// `(schema_uid, field)` -> `(return_type, deps)` for every formula field.
    pub fields: BTreeMap<(String, String), (ValueType, FieldDeps)>,
}

impl DependencyReport {
    /// The ordered dependency list strings for a field (e.g. `["sales.total"]`).
    pub fn dependency_strings(&self, schema_uid: &str, field: &str) -> Vec<String> {
        self.fields
            .get(&(schema_uid.to_string(), field.to_string()))
            .map(|(_, d)| d.formulas.iter().map(|f| f.path.clone()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_domain::{ContentTypeKind, FieldType, RelationKind, Uid};
    use core_schema::{Attribute, FormulaConfig, FormulaType, SchemaInfo};
    use indexmap::IndexMap;

    fn formula_attr(expression: &str, ret: FormulaType) -> Attribute {
        Attribute {
            attr_type: FieldType::Decimal,
            formula: Some(FormulaConfig {
                expression: expression.into(),
                return_type: ret,
                execution_strategy: core_schema::ExecutionStrategy::Computed,
                dependencies: vec![],
            }),
            ..Default::default()
        }
    }
    fn num() -> Attribute {
        Attribute::new(FieldType::Decimal)
    }

    fn schema(uid: &str, singular: &str, attrs: Vec<(&str, Attribute)>) -> Schema {
        Schema {
            uid: Uid::new(uid),
            kind: ContentTypeKind::CollectionType,
            collection_name: None,
            info: SchemaInfo {
                singular_name: singular.into(),
                plural_name: format!("{singular}s"),
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

    #[test]
    fn detects_same_schema_cycle() {
        let a = schema(
            "api::thing.thing",
            "thing",
            vec![
                ("a", formula_attr("b + 1", FormulaType::Decimal)),
                ("b", formula_attr("a + 1", FormulaType::Decimal)),
            ],
        );
        let all = vec![a];
        let err = analyze(&all).unwrap_err();
        assert!(err.message.contains("circular"), "{err}");
    }

    #[test]
    fn no_cycle_for_linear_chain() {
        let sale_line = schema(
            "api::sl.sl",
            "saleLine",
            vec![
                ("quantity", num()),
                ("unit_price", num()),
                ("net_price", formula_attr("quantity * unit_price", FormulaType::Decimal)),
                ("final_price", formula_attr("net_price + 1", FormulaType::Decimal)),
            ],
        );
        let report = analyze(&[sale_line]).unwrap();
        // final_price depends on net_price
        let deps = report.dependency_strings("api::sl.sl", "final_price");
        assert_eq!(deps, vec!["net_price"]);
    }

    #[test]
    fn detects_cross_entity_dependency() {
        let sale = schema(
            "api::sale.sale",
            "sale",
            vec![
                ("balance", num()),
                (
                    "lines",
                    Attribute {
                        attr_type: FieldType::Relation,
                        relation: Some(RelationKind::OneToMany),
                        target: Some(Uid::new("api::sl.sl")),
                        mapped_by: Some("sale".into()),
                        ..Default::default()
                    },
                ),
                ("total", formula_attr("SUM(lines.net_price)", FormulaType::Decimal)),
            ],
        );
        let sl = schema(
            "api::sl.sl",
            "saleLine",
            vec![
                ("quantity", num()),
                ("unit_price", num()),
                ("net_price", formula_attr("quantity * unit_price", FormulaType::Decimal)),
            ],
        );
        let report = analyze(&[sale, sl]).unwrap();
        let deps = report.dependency_strings("api::sale.sale", "total");
        assert_eq!(deps, vec!["lines.net_price"]);
    }
}
