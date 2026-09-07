//! Formula / computed-field configuration carried on a schema `Attribute`.
//!
//! A computed field is a normal attribute plus a `formula` block. The source
//! expression and return type are always persisted (never only compiled SQL) so
//! the formula can be recompiled when the schema or its dependencies change.
//! `dependencies` is derived by the formula subsystem and cached here for
//! metadata/API exposure; it is not the source of truth.

use serde::{Deserialize, Serialize};

/// Numeric/return types expressible by a formula. Mirrors the Formula engine's
/// value-type set but lives in `core-schema` so the persisted schema model does
/// not depend on the engine crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormulaType {
    #[default]
    Integer,
    Decimal,
    Float,
    Boolean,
    String,
    Date,
    Datetime,
    Uuid,
}

impl FormulaType {
    pub const ALL: [FormulaType; 8] = [
        Self::Integer,
        Self::Decimal,
        Self::Float,
        Self::Boolean,
        Self::String,
        Self::Date,
        Self::Datetime,
        Self::Uuid,
    ];
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Decimal => "decimal",
            Self::Float => "float",
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Date => "date",
            Self::Datetime => "datetime",
            Self::Uuid => "uuid",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == s)
    }
}

/// How a computed field is materialized / evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionStrategy {
    /// A database generated column (Postgres GENERATED ALWAYS AS ...).
    #[default]
    GeneratedColumn,
    /// A plain (non-aggregate) SQL expression.
    SqlExpression,
    /// A relational aggregate query (SUM over a relationship).
    SqlAggregateQuery,
    /// Computed at query time by the service.
    Computed,
    /// Materialized into storage (future).
    Materialized,
    /// Evaluated in the runtime engine (preview / validation).
    Runtime,
}

impl ExecutionStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GeneratedColumn => "generatedColumn",
            Self::SqlExpression => "sqlExpression",
            Self::SqlAggregateQuery => "sqlAggregateQuery",
            Self::Computed => "computed",
            Self::Materialized => "materialized",
            Self::Runtime => "runtime",
        }
    }
}

/// The persisted configuration of a computed field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulaConfig {
    /// The formula source (DSL). Always persisted.
    #[serde(default)]
    pub expression: String,
    /// Declared return type.
    #[serde(default)]
    pub return_type: FormulaType,
    /// Execution strategy chosen for this field.
    #[serde(default)]
    pub execution_strategy: ExecutionStrategy,
    /// Derived dependency paths (metadata only; re-derived on schema change).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_type_roundtrip() {
        for t in FormulaType::ALL {
            assert_eq!(FormulaType::parse(t.as_str()), Some(t));
        }
    }

    #[test]
    fn config_json_roundtrip() {
        let c = FormulaConfig {
            expression: "SUM(sales.total)".into(),
            return_type: FormulaType::Decimal,
            execution_strategy: ExecutionStrategy::SqlAggregateQuery,
            dependencies: vec!["sales.total".into()],
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["expression"], "SUM(sales.total)");
        assert_eq!(v["returnType"], "decimal");
        assert_eq!(v["executionStrategy"], "sqlAggregateQuery");
        let back: FormulaConfig = serde_json::from_value(v).unwrap();
        assert_eq!(back, c);
        // dependencies omitted when empty
        let empty = FormulaConfig::default();
        let v = serde_json::to_value(&empty).unwrap();
        assert!(v.get("dependencies").is_none());
    }
}
