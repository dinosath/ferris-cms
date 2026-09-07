//! Value types used by the Formula engine.
//!
//! The engine is strongly typed. A [`ValueType`] describes the *kind* of a
//! value flowing through an expression. Nullability is a runtime concern in
//! this first implementation: every typed value may be present or NULL at
//! evaluation time, and SQL follows three-valued logic. Type *compatibility*
//! (what the type checker rejects) is decided over the [`ValueType`] kind.

use chrono::{DateTime, NaiveDate, Utc};
use core_domain::FieldType;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// The supported value types of the Formula DSL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    /// 64-bit signed integer.
    Integer,
    /// Fixed-point decimal (monetary values). Preferred over Float.
    Decimal,
    /// IEEE-754 double (scientific/ratio inputs).
    Float,
    /// Boolean.
    Boolean,
    /// String / text.
    String,
    /// Calendar date.
    Date,
    /// Timestamp (UTC).
    DateTime,
    /// UUID.
    Uuid,
}

impl ValueType {
    pub const ALL: [ValueType; 8] = [
        Self::Integer,
        Self::Decimal,
        Self::Float,
        Self::Boolean,
        Self::String,
        Self::Date,
        Self::DateTime,
        Self::Uuid,
    ];

    /// Wire name used in schema JSON.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Decimal => "decimal",
            Self::Float => "float",
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Date => "date",
            Self::DateTime => "datetime",
            Self::Uuid => "uuid",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == s)
    }

    /// Map a Ferris content field type onto a formula value type.
    /// Relation / component / media / dynamic-zone are not scalar values.
    pub fn from_field_type(ft: FieldType) -> Option<Self> {
        use FieldType::*;
        match ft {
            Integer => Some(Self::Integer),
            Biginteger => Some(Self::Integer),
            Decimal => Some(Self::Decimal),
            Float => Some(Self::Float),
            Boolean => Some(Self::Boolean),
            String | Text | Richtext | Email | Password | Uid | Enumeration => Some(Self::String),
            Date => Some(Self::Date),
            Datetime => Some(Self::DateTime),
            Time | Json | Blocks | Media | Relation | Component | Dynamiczone => None,
        }
    }

    /// Whether the type is one of the numeric types.
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer | Self::Decimal | Self::Float)
    }

    pub fn is_temporal(&self) -> bool {
        matches!(self, Self::Date | Self::DateTime)
    }

    pub fn is_orderable(&self) -> bool {
        !matches!(self, Self::Boolean)
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Promoted numeric type for a binary arithmetic operation.
/// Integer < Decimal < Float.
pub fn promote_numeric(a: ValueType, b: ValueType) -> Option<ValueType> {
    debug_assert!(a.is_numeric() && b.is_numeric());
    let rank = |t: ValueType| match t {
        ValueType::Integer => 0,
        ValueType::Decimal => 1,
        ValueType::Float => 2,
        _ => unreachable!(),
    };
    let r = rank(a).max(rank(b));
    Some(match r {
        0 => ValueType::Integer,
        1 => ValueType::Decimal,
        _ => ValueType::Float,
    })
}

// ---------------------------------------------------------------------------
// Runtime value
// ---------------------------------------------------------------------------

/// A runtime value flowing through the evaluator. `Null` represents SQL NULL.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Decimal(Decimal),
    Float(f64),
    Boolean(bool),
    String(String),
    Date(NaiveDate),
    DateTime(DateTime<Utc>),
    Uuid(Uuid),
}

impl Value {
    pub fn value_type(&self) -> Option<ValueType> {
        match self {
            Self::Null => None,
            Self::Integer(_) => Some(ValueType::Integer),
            Self::Decimal(_) => Some(ValueType::Decimal),
            Self::Float(_) => Some(ValueType::Float),
            Self::Boolean(_) => Some(ValueType::Boolean),
            Self::String(_) => Some(ValueType::String),
            Self::Date(_) => Some(ValueType::Date),
            Self::DateTime(_) => Some(ValueType::DateTime),
            Self::Uuid(_) => Some(ValueType::Uuid),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Promote an integer/decimal to the requested decimal type if compatible.
    pub fn coerce_numeric(&self, target: ValueType) -> Option<Value> {
        if !target.is_numeric() {
            return None;
        }
        match self {
            Self::Null => Some(Self::Null),
            Self::Integer(i) => Some(match target {
                ValueType::Integer => Self::Integer(*i),
                ValueType::Decimal => Self::Decimal(Decimal::from(*i)),
                ValueType::Float => Self::Float(*i as f64),
                _ => unreachable!(),
            }),
            Self::Decimal(d) => Some(match target {
                ValueType::Decimal => Self::Decimal(*d),
                ValueType::Float => Self::Float(d.to_string().parse().ok()?),
                ValueType::Integer => {
                    // truncate toward zero like SQL integer cast
                    let s = d.trunc().to_string();
                    Self::Integer(s.parse().ok()?)
                }
                _ => unreachable!(),
            }),
            Self::Float(f) => Some(match target {
                ValueType::Float => Self::Float(*f),
                ValueType::Decimal => Self::Decimal(Decimal::from_f64(*f)?),
                ValueType::Integer => {
                    let t = f.trunc() as i64;
                    Self::Integer(t)
                }
                _ => unreachable!(),
            }),
            _ => None,
        }
    }

    pub fn truthy(&self) -> Option<bool> {
        match self {
            Self::Null => None,
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("NULL"),
            Self::Integer(i) => write!(f, "{i}"),
            Self::Decimal(d) => write!(f, "{d}"),
            Self::Float(x) => write!(f, "{x}"),
            Self::Boolean(b) => write!(f, "{b}"),
            Self::String(s) => write!(f, "{s}"),
            Self::Date(d) => write!(f, "{d}"),
            Self::DateTime(d) => write!(f, "{d}"),
            Self::Uuid(u) => write!(f, "{u}"),
        }
    }
}

/// Conversions to help the evaluator round-trip values from JSON fixture rows.
impl Value {
    pub fn from_json(v: &serde_json::Value) -> Option<Value> {
        match v {
            serde_json::Value::Null => Some(Value::Null),
            serde_json::Value::Bool(b) => Some(Value::Boolean(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Some(Value::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Some(Value::Float(f))
                } else {
                    None
                }
            }
            serde_json::Value::String(s) => {
                // strings are left as strings; date coercion is done by the
                // caller based on the typed field
                Some(Value::String(s.clone()))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_type_mapping() {
        assert_eq!(ValueType::from_field_type(FieldType::Decimal), Some(ValueType::Decimal));
        assert_eq!(ValueType::from_field_type(FieldType::Integer), Some(ValueType::Integer));
        assert_eq!(ValueType::from_field_type(FieldType::Boolean), Some(ValueType::Boolean));
        assert_eq!(ValueType::from_field_type(FieldType::Relation), None);
    }

    #[test]
    fn numeric_promotion() {
        assert_eq!(promote_numeric(ValueType::Integer, ValueType::Integer), Some(ValueType::Integer));
        assert_eq!(promote_numeric(ValueType::Integer, ValueType::Decimal), Some(ValueType::Decimal));
        assert_eq!(promote_numeric(ValueType::Decimal, ValueType::Float), Some(ValueType::Float));
    }

    #[test]
    fn roundtrip_wire_names() {
        for t in ValueType::ALL {
            assert_eq!(ValueType::parse(t.as_str()), Some(t));
        }
    }
}
