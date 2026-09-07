//! Recursive-descent parser for the Formula DSL.
//!
//! Precedence (lowest to highest): OR, AND, NOT, comparison, additive,
//! multiplicative, unary minus, primary. Arithmetic `*` binds tighter than `+`
//! so `quantity + unit_price * 2` parses as `quantity + (unit_price * 2)`.

use crate::ast::{AggFunc, BinOp, Expr, Logic, Path, UnOp};
use crate::errors::{FormulaError, Result};
use crate::lexer::{lex, Tok, Token};
use crate::types::ValueType;
use rust_decimal::Decimal;
use std::str::FromStr;

pub struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn new(src: &'a str) -> Result<Self> {
        Ok(Self {
            src,
            toks: lex(src)?,
            pos: 0,
        })
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].kind
    }

    fn advance(&mut self) -> Tok {
        let t = self.toks[self.pos].kind.clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn check(&mut self, k: &Tok) -> bool {
        self.peek() == k
    }

    fn expect(&mut self, k: &Tok, what: &str) -> Result<()> {
        if self.check(k) {
            self.advance();
            Ok(())
        } else {
            Err(self.err_here(&format!("expected {what}")))
        }
    }

    fn err_here(&self, msg: &str) -> FormulaError {
        let t = &self.toks[self.pos.min(self.toks.len() - 1)];
        let (line, col) = crate::lexer::line_col(self.src, t.span.start);
        FormulaError {
            expression: Some(self.src.to_string()),
            location: Some((line, col)),
            message: msg.to_string(),
            expected: None,
            actual: None,
        }
    }

    pub fn parse_expression(mut self) -> Result<Expr> {
        let e = self.parse_or()?;
        // allow trailing `AS type`? not part of grammar.
        if !matches!(self.peek(), Tok::Eof) {
            return Err(self.err_here("unexpected trailing input"));
        }
        Ok(e)
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.check(&Tok::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Logical {
                left: Box::new(left),
                op: Logic::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_not()?;
        while self.check(&Tok::And) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::Logical {
                left: Box::new(left),
                op: Logic::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.check(&Tok::Not) {
            self.advance();
            let inner = self.parse_not()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Tok::Eq => BinOp::Eq,
                Tok::Neq => BinOp::Ne,
                Tok::Gt => BinOp::Gt,
                Tok::Gte => BinOp::Gte,
                Tok::Lt => BinOp::Lt,
                Tok::Lte => BinOp::Lte,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        // postfix IS [NOT] NULL handled here because it is a comparison-ish.
        if self.check(&Tok::Is) {
            self.advance();
            let negated = if self.check(&Tok::Not) {
                self.advance();
                true
            } else {
                false
            };
            self.expect(&Tok::Null, "NULL")?;
            left = Expr::IsNull {
                expr: Box::new(left),
                negated,
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.check(&Tok::Minus) {
            self.advance();
            let inner = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnOp::Neg,
                expr: Box::new(inner),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Tok::Number(raw) => {
                self.advance();
                if raw.contains('.') {
                    let d = Decimal::from_str(&raw).map_err(|_| {
                        self.err_here(&format!("invalid decimal literal `{raw}`"))
                    })?;
                    Ok(Expr::Decimal(d))
                } else {
                    let i = raw.parse::<i64>().map_err(|_| {
                        self.err_here(&format!("integer literal out of range `{raw}`"))
                    })?;
                    Ok(Expr::Int(i))
                }
            }
            Tok::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Tok::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Tok::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Tok::Ident(name) => {
                self.advance();
                if self.check(&Tok::LParen) {
                    self.parse_function_call(&name)
                } else {
                    // Possibly a dotted path.
                    let mut segments = vec![name];
                    while self.check(&Tok::Dot) {
                        self.advance();
                        match self.peek().clone() {
                            Tok::Ident(next) => {
                                segments.push(next);
                                self.advance();
                            }
                            other => {
                                return Err(self.err_here(&format!(
                                    "expected identifier after `.`, found {other:?}"
                                )))
                            }
                        }
                    }
                    Ok(Expr::Field(Path::new(segments)))
                }
            }
            Tok::Case => {
                self.advance();
                self.parse_case()
            }
            Tok::LParen => {
                self.advance();
                let e = self.parse_or()?;
                self.expect(&Tok::RParen, "`)`")?;
                Ok(e)
            }
            Tok::Not => {
                // handled in parse_not normally, but guard just in case
                self.advance();
                let inner = self.parse_not()?;
                Ok(Expr::Not(Box::new(inner)))
            }
            other => Err(self.err_here(&format!("unexpected token {other:?}"))),
        }
    }

    fn parse_function_call(&mut self, name: &str) -> Result<Expr> {
        // '(' has already been advanced? No: parse_primary advanced Ident, and
        // sees LParen still. Consume it here.
        self.expect(&Tok::LParen, "`(`")?;

        let upper = name.to_ascii_uppercase();
        if let Some(func) = aggregate_of(&upper) {
            return self.parse_aggregate(func);
        }
        if upper == "IF" {
            let cond = self.parse_or()?;
            self.expect(&Tok::Comma, "`,`")?;
            let then = self.parse_or()?;
            self.expect(&Tok::Comma, "`,`")?;
            let else_ = self.parse_or()?;
            self.expect(&Tok::RParen, "`)`")?;
            return Ok(Expr::If {
                cond: Box::new(cond),
                then: Box::new(then),
                else_: Box::new(else_),
            });
        }
        if upper == "COALESCE" {
            let mut args = Vec::new();
            if !self.check(&Tok::RParen) {
                loop {
                    args.push(self.parse_or()?);
                    if !self.check(&Tok::Comma) {
                        break;
                    }
                    self.advance();
                }
            }
            self.expect(&Tok::RParen, "`)`")?;
            return Ok(Expr::Coalesce(args));
        }
        if upper == "NULLIF" {
            let a = self.parse_or()?;
            self.expect(&Tok::Comma, "`,`")?;
            let b = self.parse_or()?;
            self.expect(&Tok::RParen, "`)`")?;
            return Ok(Expr::NullIf {
                a: Box::new(a),
                b: Box::new(b),
            });
        }

        // Generic call with zero or more arguments.
        let mut args = Vec::new();
        if !self.check(&Tok::RParen) {
            loop {
                args.push(self.parse_or()?);
                if !self.check(&Tok::Comma) {
                    break;
                }
                self.advance();
            }
        }
        self.expect(&Tok::RParen, "`)`")?;
        Ok(Expr::Call {
            name: upper,
            args,
        })
    }

    fn parse_aggregate(&mut self, func: AggFunc) -> Result<Expr> {
        let arg = self.parse_or()?;
        let mut filter = None;
        if self.check(&Tok::Where) {
            self.advance();
            let f = self.parse_or()?;
            filter = Some(Box::new(f));
        }
        self.expect(&Tok::RParen, "`)`")?;
        Ok(Expr::Aggregate {
            func,
            arg: Box::new(arg),
            filter,
        })
    }

    fn parse_case(&mut self) -> Result<Expr> {
        // CASE [expr] WHEN val THEN ... — simple CASE is not supported; only
        // searched CASE: WHEN cond THEN val ...
        let mut whens = Vec::new();
        while self.check(&Tok::When) {
            self.advance();
            let cond = self.parse_or()?;
            self.expect(&Tok::Then, "THEN")?;
            let val = self.parse_or()?;
            whens.push((cond, val));
        }
        if whens.is_empty() {
            return Err(self.err_here("CASE requires at least one WHEN"));
        }
        let else_ = if self.check(&Tok::Else) {
            self.advance();
            Some(Box::new(self.parse_or()?))
        } else {
            None
        };
        self.expect(&Tok::End, "END")?;
        Ok(Expr::Case { whens, else_ })
    }
}

/// Parse an expression from a source string.
pub fn parse(src: &str) -> Result<Expr> {
    Parser::new(src)?.parse_expression()
}

fn aggregate_of(name: &str) -> Option<AggFunc> {
    match name {
        "SUM" => Some(AggFunc::Sum),
        "AVG" => Some(AggFunc::Avg),
        "MIN" => Some(AggFunc::Min),
        "MAX" => Some(AggFunc::Max),
        "COUNT" => Some(AggFunc::Count),
        _ => None,
    }
}

/// Return type of aggregate functions.
pub fn aggregate_return_type(func: AggFunc, arg: ValueType) -> ValueType {
    match func {
        AggFunc::Count => ValueType::Integer,
        AggFunc::Avg => match arg {
            ValueType::Integer => ValueType::Decimal,
            t => t,
        },
        _ => arg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Expr::*;

    #[test]
    fn parses_binary_with_precedence() {
        let e = parse("quantity + unit_price * 2").unwrap();
        match e {
            Binary { left, op, right } => {
                assert_eq!(op, BinOp::Add);
                assert!(matches!(*left, Field(p) if p.join(".") == "quantity"));
                match *right {
                    Binary { op, .. } => assert_eq!(op, BinOp::Mul),
                    other => panic!("expected multiplication on RHS, got {other:?}"),
                }
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_subtraction() {
        let e = parse("quantity * unit_price - discount").unwrap();
        match e {
            Binary { op: BinOp::Sub, .. } => {}
            other => panic!("expected subtraction, got {other:?}"),
        }
    }

    #[test]
    fn parses_field_paths() {
        let e = parse("customer.name").unwrap();
        assert!(matches!(e, Field(p) if p.segments == vec!["customer".to_string(), "name".to_string()]));
    }

    #[test]
    fn parses_sum_path() {
        let e = parse("SUM(lines.total)").unwrap();
        match e {
            Aggregate { func, filter, .. } => {
                assert_eq!(func, AggFunc::Sum);
                assert!(filter.is_none());
            }
            other => panic!("expected aggregate, got {other:?}"),
        }
    }

    #[test]
    fn parses_filtered_sum() {
        let e = parse("SUM(sales.balance WHERE sales.status = \"OPEN\")").unwrap();
        match e {
            Aggregate { func, filter, .. } => {
                assert_eq!(func, AggFunc::Sum);
                assert!(filter.is_some());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_if() {
        let e = parse("IF(quantity > 0, quantity * unit_price, 0)").unwrap();
        assert!(matches!(e, If { .. }));
    }

    #[test]
    fn parses_coalesce() {
        let e = parse("COALESCE(discount, 0)").unwrap();
        assert!(matches!(e, Coalesce(_)));
    }

    #[test]
    fn parses_case() {
        let e = parse("CASE WHEN quantity > 0 THEN net ELSE 0 END").unwrap();
        assert!(matches!(e, Case { .. }));
    }

    #[test]
    fn parses_is_null() {
        let e = parse("discount IS NULL").unwrap();
        assert!(matches!(e, IsNull { negated: false, .. }));
        let e = parse("discount IS NOT NULL").unwrap();
        assert!(matches!(e, IsNull { negated: true, .. }));
    }

    #[test]
    fn decimal_literals() {
        let e = parse("1.5").unwrap();
        assert!(matches!(e, Decimal(d) if d.to_string() == "1.5"));
        let e = parse("42").unwrap();
        assert!(matches!(e, Int(42)));
    }

    #[test]
    fn nested_functions_and_precedence() {
        let e = parse("COALESCE(SUM(sales.total), 0) + 1").unwrap();
        match e {
            Binary { op: BinOp::Add, right, .. } => {
                assert!(matches!(*right, Int(1)));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_syntax() {
        assert!(parse("quantity *").is_err());
        assert!(parse("(").is_err());
        assert!(parse("SUM()").is_err());
    }
}
