//! # Formula engine for Ferris CMS
//!
//! A first-class **Formula / Computed Field** subsystem. It lets administrators
//! express business formulas over content types without writing Rust or SQL:
//!
//! ```text
//! SaleLine.final_price = (quantity * unit_price - discount) * (1 + vat_rate / 100)
//! Sale.subtotal        = SUM(lines.net_price)
//! Customer.total_revenue = SUM(sales.total)
//! ```
//!
//! The subsystem is designed around a typed pipeline and never treats formulas
//! as arbitrary SQL strings:
//!
//! ```text
//! Formula DSL -> Lexer -> Parser -> AST -> Type checker -> Dependency analyzer
//!              -> Typed IR -> SeaORM/sea-query compiler (or runtime evaluator)
//! ```

pub mod ast;
pub mod deps;
pub mod errors;
pub mod evaluator;
pub mod ir;
pub mod lexer;
pub mod parser;
pub mod registry;
pub mod sql;
pub mod type_checker;
pub mod types;
