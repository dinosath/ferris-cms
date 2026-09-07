# Formula / Computed Fields

Ferris CMS lets administrators define business formulas over content types
without writing Rust or SQL. Formulas are stored as **source expressions** on
schema attributes and compiled by the Formula subsystem into safe sea-query
expressions — never into arbitrary user-controlled SQL strings.

This document explains the subsystem for developers.

## 1. Formula DSL

A formula is an expression over fields of the host content type and its
relationships:

```text
SaleLine.net_price  = (quantity * unit_price) - COALESCE(discount, 0)
SaleLine.final_price = net_price + vat_amount
Sale.subtotal        = SUM(lines.net_price)
Sale.total           = SUM(lines.final_price)
Customer.total_revenue = SUM(sales.total)
Customer.outstanding = SUM(sales.balance WHERE sales.status = "OPEN")
Customer.overdue     = SUM(sales.balance WHERE sales.balance > 0 AND sales.due_date < TODAY())
Customer.total_margin = SUM(sales.lines.margin)   # advanced (nested)
```

Operators and functions:

| Area | Syntax |
|---|---|
| Arithmetic | `+ - * / %` |
| Comparison | `= != > >= < <=` |
| Boolean | `AND OR NOT` |
| Conditional | `IF(c,t,f)`, `CASE WHEN c THEN v ELSE e END` |
| Null handling | `COALESCE(a,b,...)`, `NULLIF(a,b)`, `x IS NULL`, `x IS NOT NULL` |
| Aggregates | `SUM() AVG() MIN() MAX() COUNT()` over a collection |
| Filtered aggregates | `SUM(rel.field WHERE rel.cond)` |
| Date | `TODAY() NOW() DATE_ADD() DATE_DIFF() YEAR() MONTH() DAY()` |

Relationships traverse naturally: `customer.name`, `product.cost`,
`lines.total`, `sales.total`. The grammar is extensible: add functions to the
registry in `type_checker.rs` and to the evaluator/SQL compiler.

## 2. AST

Formulas parse to a structural AST (`src/ast.rs`); strings are never kept after
parsing.

```text
SUM(lines.total)          -> Aggregate { Sum, collection: [lines], expr: Field(total) }
quantity * unit_price     -> Binary { Mul, Field(quantity), Field(unit_price) }
```

## 3. Type system

The engine is strongly typed (`src/types.rs`). Supported types:
`Integer Decimal Float Boolean String Date DateTime UUID`. Monetary values use
`Decimal` (via `rust_decimal`) — never binary `f64` — so arithmetic is exact.

The type checker (`src/type_checker.rs`) resolves field paths against the
schema registry and rejects invalid expressions with a location-aware error:

```text
quantity * customer.name   # Decimal × String  -> rejected
quantity + true            # Decimal × Boolean -> rejected
SUM(non_existing_relation.total)  # unknown relation -> rejected
UNKNOWN_FUNCTION(quantity) # unknown function -> rejected
```

## 4. Relationship traversal

Relationship resolution reads the schema registry (`src/registry.rs`); entity
names are never hard-coded. A path like `customer.credit_limit` from `Sale`
walks the `customer` (manyToOne) relation to the `Customer` content type, then
the scalar `credit_limit`. `Customer.sales` is oneToMany, so it may only be used
as the collection of an aggregate.

## 5. Dependency graph

`src/deps.rs` builds the dependency edges between formula fields (both on the
same content type and across relationships) and detects circular definitions:

```text
A = B + 1
B = A + 1   # -> "circular formula dependency detected: ...A -> B -> A"
```

It also produces the `dependencies` list exposed as metadata (e.g.
`["sales.total"]`).

## 6. Typed IR

After type checking, the AST is lowered to a typed IR (`src/ir.rs`) where each
node carries its `ValueType`. This is the representation that different back
ends (SQL, runtime evaluator, future materialized stores) compile from.

## 7. SeaORM / sea-query compiler

`src/sql.rs` compiles the typed IR into safe `sea_query` expressions. Scalars
compile to column references; single-row relationship traversals and
relationship aggregates compile to **correlated scalar subqueries**. A whole
collection is computed in one query (no N+1):

```text
SELECT id,
       (SELECT SUM( (SELECT SUM(net_expr) FROM ct_sale_line ...)
                    FROM ct_sale WHERE ct_sale.customer_id = ct_customer.id)
       ) AS total_revenue
FROM ct_customer
```

Callers embed the expression with `select().expr_as(expr, Alias)`, matching
how `dynamic-store` builds queries.

## 8. Execution strategies

`core_schema::ExecutionStrategy` records how a field is computed:
`generatedColumn`, `sqlExpression`, `sqlAggregateQuery`, `computed`,
`materialized`, `runtime`. The compiler/Metadata heuristic in `registry.rs`
chooses a default; relationship aggregates are never forced into generated
columns.

## 9. Persistence / metadata

A computed field is an `Attribute` with a `formula` block
(`core_schema::FormulaConfig`):

```json
{
  "name": "total_revenue",
  "type": "decimal",
  "computed": true,
  "expression": "SUM(sales.total)",
  "returnType": "decimal",
  "executionStrategy": "sqlAggregateQuery",
  "dependencies": ["sales.total"]
}
```

The source expression is always persisted so the formula can be recompiled when
the schema changes. Computed attributes never receive a physical column and are
read-only (`dynamic-store` skips them).

## 10. Runtime evaluator

`src/evaluator.rs` evaluates scalar formulas against in-memory records with
exact decimal arithmetic. It is used for unit testing, previews and validation,
not as a replacement for SQL.

## 11. Adding a function / operator

1. **Parser**: operators are tokenized in `lexer.rs` and given precedence in
   `parser.rs`. Functions that are not special forms become `Expr::Call`.
2. **Type checking**: add a `FuncSig` entry in `type_checker.rs` (name, arity,
   return-type function).
3. **Evaluator**: add a match arm in `evaluator.rs::eval_function`.
4. **SQL compiler**: add a match arm in `sql.rs` (use `Func::...`/`Expr` when a
   safe expression exists).

## 12. ERP examples

```text
SaleLine.net_price  = (quantity * unit_price) - COALESCE(discount, 0)
SaleLine.vat_amount = net_price * vat_rate / 100
SaleLine.final_price= net_price + vat_amount
Sale.subtotal       = SUM(lines.net_price)
Sale.vat            = SUM(lines.vat_amount)
Sale.total          = subtotal + vat
Customer.total_revenue = SUM(sales.total)
Customer.outstanding = SUM(sales.balance WHERE sales.status = "OPEN")
Customer.overdue     = SUM(sales.balance WHERE sales.balance > 0 AND sales.due_date < TODAY())
Customer.total_margin = SUM(sales.lines.margin)
```

The crate integration suite (`tests/erp.rs`) runs these against a real SQLite
database through the sea-query path.
