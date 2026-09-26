//! Computed-field expression language (design extension).
//!
//! A small, database-portable expression language used by computed/generated
//! columns. It is intentionally limited to what can be emitted as a
//! `GENERATED ALWAYS AS (...)` clause across SQLite/PostgreSQL/MySQL:
//!
//! - column references (`quantity`, `first_name`)
//! - numeric literals (`100`, `2.5`), string literals (`' '`), booleans, `NULL`
//! - arithmetic `+ - * / %` and string concatenation `||`
//! - comparisons `= <> != < <= > >=`
//! - parentheses
//! - a few portable functions: `COALESCE`, `ROUND`, `ABS`, `LOWER`, `UPPER`,
//!   `LENGTH`, `CAST(x AS type)` is intentionally *not* supported.
//!
//! The parser produces a pure AST; `dynamic-store` renders it to a SeaQuery
//! expression. Keeping the AST free of SeaQuery lets `core-schema` stay a
//! dependency-light model crate.

use std::fmt;

/// Binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Concat,
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
}

/// Unary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

/// A parsed computed-field expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// Reference to another field of the same content type.
    Column(String),
    Number(f64),
    Str(String),
    Bool(bool),
    Null,
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    /// Portable function call, e.g. `ROUND(x, 2)`.
    Func {
        name: String,
        args: Vec<Expr>,
    },
}

impl Expr {
    /// Field names referenced by this expression, in first-seen order.
    pub fn columns(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_columns(&mut out);
        out
    }

    fn collect_columns(&self, out: &mut Vec<String>) {
        match self {
            Expr::Column(name) => {
                if !out.iter().any(|c| c == name) {
                    out.push(name.clone());
                }
            }
            Expr::Binary { left, right, .. } => {
                left.collect_columns(out);
                right.collect_columns(out);
            }
            Expr::Unary { expr, .. } => expr.collect_columns(out),
            Expr::Func { args, .. } => {
                for a in args {
                    a.collect_columns(out);
                }
            }
            Expr::Number(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null => {}
        }
    }
}

/// Parse error with a human message and byte offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.position)
    }
}

impl std::error::Error for ParseError {}

/// Parse a computed-field expression.
pub fn parse_expression(input: &str) -> Result<Expr, ParseError> {
    let mut p = Parser::new(input);
    let expr = p.parse_expr()?;
    p.skip_ws();
    if !p.at_end() {
        return Err(p.error("unexpected trailing input"));
    }
    Ok(expr)
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Ident(String),
    Number(f64),
    Str(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

impl Token {
    fn describe(&self) -> String {
        match self {
            Token::Ident(s) => format!("`{s}`"),
            Token::Number(n) => format!("number `{n}`"),
            Token::Str(_) => "string literal".into(),
            Token::Op(o) => format!("`{o}`"),
            Token::LParen => "`(`".into(),
            Token::RParen => "`)`".into(),
            Token::Comma => "`,`".into(),
        }
    }
}

struct Parser<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    /// One-token lookahead.
    lookahead: Option<Token>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            lookahead: None,
        }
    }

    fn at_end(&mut self) -> bool {
        self.peek().is_none()
    }

    fn error(&self, msg: impl Into<String>) -> ParseError {
        ParseError {
            message: msg.into(),
            position: self.pos,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, ParseError> {
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return Ok(None);
        }
        let start = self.pos;
        let c = self.bytes[self.pos] as char;

        if c.is_ascii_alphabetic() || c == '_' {
            while self.pos < self.bytes.len() {
                let ch = self.bytes[self.pos] as char;
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            return Ok(Some(Token::Ident(self.src[start..self.pos].to_string())));
        }

        if c.is_ascii_digit() {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.' {
                self.pos += 1;
                while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            }
            let text = &self.src[start..self.pos];
            let n = text.parse::<f64>().map_err(|_| ParseError {
                message: format!("invalid number `{text}`"),
                position: start,
            })?;
            return Ok(Some(Token::Number(n)));
        }

        if c == '\'' {
            // SQL string literal; '' is an escaped quote.
            self.pos += 1;
            let mut s = String::new();
            loop {
                if self.pos >= self.bytes.len() {
                    return Err(ParseError {
                        message: "unterminated string literal".into(),
                        position: start,
                    });
                }
                let ch = self.bytes[self.pos] as char;
                if ch == '\'' {
                    if self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'\'' {
                        s.push('\'');
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                        break;
                    }
                } else {
                    s.push(ch);
                    self.pos += 1;
                }
            }
            return Ok(Some(Token::Str(s)));
        }

        // Multi-char operators first.
        let two = self.src.get(self.pos..self.pos + 2);
        let op2 = match two {
            Some("||") => Some("||"),
            Some("<>") => Some("<>"),
            Some("!=") => Some("!="),
            Some("<=") => Some("<="),
            Some(">=") => Some(">="),
            _ => None,
        };
        if let Some(op) = op2 {
            self.pos += 2;
            return Ok(Some(Token::Op(op)));
        }

        let one = match c {
            '+' => Some("+"),
            '-' => Some("-"),
            '*' => Some("*"),
            '/' => Some("/"),
            '%' => Some("%"),
            '=' => Some("="),
            '<' => Some("<"),
            '>' => Some(">"),
            _ => None,
        };
        if let Some(op) = one {
            self.pos += 1;
            return Ok(Some(Token::Op(op)));
        }

        match c {
            '(' => {
                self.pos += 1;
                Ok(Some(Token::LParen))
            }
            ')' => {
                self.pos += 1;
                Ok(Some(Token::RParen))
            }
            ',' => {
                self.pos += 1;
                Ok(Some(Token::Comma))
            }
            _ => Err(ParseError {
                message: format!("unexpected character `{c}`"),
                position: start,
            }),
        }
    }

    fn peek(&mut self) -> Option<Token> {
        if self.lookahead.is_none() {
            match self.next_token() {
                Ok(t) => self.lookahead = t,
                Err(_) => self.lookahead = None,
            }
        }
        self.lookahead.clone()
    }

    fn bump(&mut self) -> Result<Token, ParseError> {
        let tok = self.peek();
        match tok {
            Some(t) => {
                self.lookahead = None;
                Ok(t)
            }
            None => Err(self.error("unexpected end of expression")),
        }
    }

    /// Comparison level (lowest precedence).
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive()?;
        while let Some(Token::Op(op)) = self.peek() {
            let op = match op {
                "=" => BinOp::Eq,
                "!=" | "<>" => BinOp::Ne,
                "<" => BinOp::Lt,
                "<=" => BinOp::Lte,
                ">" => BinOp::Gt,
                ">=" => BinOp::Gte,
                _ => break,
            };
            self.bump()?;
            let right = self.parse_additive()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            match self.peek() {
                Some(Token::Op("+")) => {
                    self.bump()?;
                    let right = self.parse_multiplicative()?;
                    left = bin(BinOp::Add, left, right);
                }
                Some(Token::Op("-")) => {
                    self.bump()?;
                    let right = self.parse_multiplicative()?;
                    left = bin(BinOp::Sub, left, right);
                }
                Some(Token::Op("||")) => {
                    self.bump()?;
                    let right = self.parse_multiplicative()?;
                    left = bin(BinOp::Concat, left, right);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Op("*")) => BinOp::Mul,
                Some(Token::Op("/")) => BinOp::Div,
                Some(Token::Op("%")) => BinOp::Mod,
                _ => break,
            };
            self.bump()?;
            let right = self.parse_unary()?;
            left = bin(op, left, right);
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Token::Op("-")) => {
                self.bump()?;
                let e = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(e),
                })
            }
            Some(Token::Op("+")) => {
                self.bump()?;
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self.bump()?;
        match tok {
            Token::Number(n) => Ok(Expr::Number(n)),
            Token::Str(s) => Ok(Expr::Str(s)),
            Token::LParen => {
                let e = self.parse_expr()?;
                match self.bump()? {
                    Token::RParen => Ok(e),
                    other => Err(self.error(format!("expected `)`, found {}", other.describe()))),
                }
            }
            Token::Ident(name) => {
                let lower = name.to_ascii_lowercase();
                match lower.as_str() {
                    "true" => return Ok(Expr::Bool(true)),
                    "false" => return Ok(Expr::Bool(false)),
                    "null" => return Ok(Expr::Null),
                    _ => {}
                }
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.bump()?; // consume '('
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Token::RParen)) {
                        loop {
                            args.push(self.parse_expr()?);
                            match self.peek() {
                                Some(Token::Comma) => {
                                    self.bump()?;
                                }
                                _ => break,
                            }
                        }
                    }
                    match self.bump()? {
                        Token::RParen => {}
                        other => {
                            return Err(self.error(format!(
                                "expected `)` after arguments, found {}",
                                other.describe()
                            )))
                        }
                    }
                    Ok(Expr::Func {
                        name: name.to_uppercase(),
                        args,
                    })
                } else {
                    Ok(Expr::Column(name))
                }
            }
            other => Err(self.error(format!("unexpected token {}", other.describe()))),
        }
    }
}

fn bin(op: BinOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// Token category for syntax highlighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// A field/column reference.
    Column,
    /// A literal keyword (`true`, `false`, `null`).
    Keyword,
    Number,
    Str,
    Operator,
    Punct,
}

/// Tokenize an expression for syntax highlighting.
pub fn tokenize(input: &str) -> Result<Vec<(TokenKind, String)>, ParseError> {
    let mut p = Parser::new(input);
    let mut out = Vec::new();
    while let Some(tok) = p.next_token()? {
        let item = match &tok {
            Token::Ident(s) => {
                let kind = match s.to_ascii_lowercase().as_str() {
                    "true" | "false" | "null" => TokenKind::Keyword,
                    _ => TokenKind::Column,
                };
                (kind, s.clone())
            }
            Token::Number(n) => (TokenKind::Number, n.to_string()),
            Token::Str(s) => (TokenKind::Str, format!("'{s}'")),
            Token::Op(o) => (TokenKind::Operator, o.to_string()),
            Token::LParen => (TokenKind::Punct, "(".to_string()),
            Token::RParen => (TokenKind::Punct, ")".to_string()),
            Token::Comma => (TokenKind::Punct, ",".to_string()),
        };
        out.push(item);
    }
    Ok(out)
}

/// Evaluate a parsed expression against sample field values (used for the
/// Content-Type Builder's realtime preview). Mirrors the DDL semantics for the
/// supported operators/functions.
pub fn evaluate(
    e: &Expr,
    values: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    match e {
        Expr::Column(n) => values
            .get(n)
            .cloned()
            .ok_or_else(|| format!("no sample value for `{n}`")),
        Expr::Number(n) => Ok(num_val(*n)),
        Expr::Str(s) => Ok(Value::String(s.clone())),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Null => Ok(Value::Null),
        Expr::Unary { op, expr } => {
            let v = evaluate(expr, values)?;
            match op {
                UnOp::Neg => Ok(num_val(-to_num(&v)?)),
                UnOp::Not => Ok(Value::Bool(!truthy(&v))),
            }
        }
        Expr::Binary { op, left, right } => {
            let l = evaluate(left, values)?;
            let r = evaluate(right, values)?;
            match op {
                BinOp::Concat => Ok(Value::String(format!("{}{}", to_str(&l), to_str(&r)))),
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                    let a = to_num(&l)?;
                    let b = to_num(&r)?;
                    let out = match op {
                        BinOp::Add => a + b,
                        BinOp::Sub => a - b,
                        BinOp::Mul => a * b,
                        BinOp::Div => {
                            if b == 0.0 {
                                return Err("division by zero".to_string());
                            }
                            a / b
                        }
                        BinOp::Mod => a % b,
                        _ => unreachable!(),
                    };
                    Ok(num_val(out))
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                    let c = compare(&l, &r);
                    let b = match op {
                        BinOp::Eq => c == 0,
                        BinOp::Ne => c != 0,
                        BinOp::Lt => c < 0,
                        BinOp::Lte => c <= 0,
                        BinOp::Gt => c > 0,
                        BinOp::Gte => c >= 0,
                        _ => unreachable!(),
                    };
                    Ok(Value::Bool(b))
                }
            }
        }
        Expr::Func { name, args } => {
            let vals: Vec<serde_json::Value> = args
                .iter()
                .map(|a| evaluate(a, values))
                .collect::<Result<_, _>>()?;
            let arg = |i: usize| vals.get(i).cloned().unwrap_or(serde_json::Value::Null);
            match name.as_str() {
                "COALESCE" => Ok(vals
                    .into_iter()
                    .find(|v| !v.is_null())
                    .unwrap_or(serde_json::Value::Null)),
                "ABS" => Ok(num_val(to_num(&arg(0))?.abs())),
                "ROUND" => {
                    let x = to_num(&arg(0))?;
                    let d = if vals.len() > 1 {
                        to_num(&arg(1))?
                    } else {
                        0.0
                    };
                    let m = 10f64.powf(d);
                    Ok(num_val((x * m).round() / m))
                }
                "LOWER" => Ok(serde_json::Value::String(to_str(&arg(0)).to_lowercase())),
                "UPPER" => Ok(serde_json::Value::String(to_str(&arg(0)).to_uppercase())),
                "LENGTH" => Ok(serde_json::Value::from(
                    to_str(&arg(0)).chars().count() as i64
                )),
                other => Err(format!("function `{other}` is not supported in preview")),
            }
        }
    }
}

fn num_val(f: f64) -> serde_json::Value {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 {
        serde_json::Value::from(f as i64)
    } else {
        serde_json::Value::from(f)
    }
}

fn to_num(v: &serde_json::Value) -> Result<f64, String> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().ok_or_else(|| "number out of range".to_string()),
        serde_json::Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("`{s}` is not numeric")),
        serde_json::Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        other => Err(format!("{other} is not numeric")),
    }
}

fn to_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Null => false,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        serde_json::Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn compare(l: &serde_json::Value, r: &serde_json::Value) -> i32 {
    if let (Ok(a), Ok(b)) = (to_num(l), to_num(r)) {
        return if a < b {
            -1
        } else if a > b {
            1
        } else {
            0
        };
    }
    let (a, b) = (to_str(l), to_str(r));
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arithmetic_and_precedence() {
        let e = parse_expression("subtotal + tax * 2").unwrap();
        // tax * 2 binds tighter than +
        match e {
            Expr::Binary {
                op: BinOp::Add,
                right,
                ..
            } => match *right {
                Expr::Binary { op: BinOp::Mul, .. } => {}
                other => panic!("expected multiply on the right, got {other:?}"),
            },
            other => panic!("expected add at the root, got {other:?}"),
        }
    }

    #[test]
    fn parses_concatenation_and_strings() {
        let e = parse_expression("first_name || ' ' || last_name").unwrap();
        let cols = e.columns();
        assert_eq!(cols, vec!["first_name".to_string(), "last_name".into()]);
    }

    #[test]
    fn parses_functions_and_parens() {
        let e = parse_expression("(deals_won * 100) / (deals_won + deals_lost)").unwrap();
        assert_eq!(
            e.columns(),
            vec!["deals_won".to_string(), "deals_lost".into()]
        );
        let f = parse_expression("ROUND(revenue - expenses, 2)").unwrap();
        assert!(matches!(f, Expr::Func { .. }));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_expression("quantity *").is_err());
        assert!(parse_expression("quantity * unit_price)").is_err());
        assert!(parse_expression("@nope").is_err());
        assert!(parse_expression("'unterminated").is_err());
    }
}

#[cfg(test)]
mod eval_and_tokenize_tests {
    use super::*;
    use serde_json::{json, Map, Value};

    fn vals(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn eval(src: &str, pairs: &[(&str, Value)]) -> Value {
        let e = parse_expression(src).unwrap();
        evaluate(&e, &vals(pairs)).unwrap()
    }

    #[test]
    fn evaluates_erp_and_crm_previews() {
        assert_eq!(
            eval(
                "quantity * unit_price",
                &[("quantity", json!(10)), ("unit_price", json!(100))]
            ),
            json!(1000)
        );
        assert_eq!(
            eval(
                "subtotal * discount / 100",
                &[("subtotal", json!(1000)), ("discount", json!(10))]
            ),
            json!(100)
        );
        assert_eq!(
            eval(
                "subtotal - discount_amount",
                &[("subtotal", json!(1000)), ("discount_amount", json!(100))]
            ),
            json!(900)
        );
        assert_eq!(
            eval(
                "first_name || ' ' || last_name",
                &[("first_name", json!("John")), ("last_name", json!("Smith"))]
            ),
            json!("John Smith")
        );
        assert_eq!(
            eval(
                "(deals_won * 100) / (deals_won + deals_lost)",
                &[("deals_won", json!(8)), ("deals_lost", json!(2))]
            ),
            json!(80)
        );
        assert_eq!(eval("ROUND(10 / 3, 2)", &[]), json!(3.33));
        assert_eq!(
            eval("COALESCE(note, 5)", &[("note", Value::Null)]),
            json!(5)
        );
        assert_eq!(
            eval("UPPER(name)", &[("name", json!("smith"))]),
            json!("SMITH")
        );
    }

    #[test]
    fn preview_reports_errors() {
        let e = parse_expression("a / b").unwrap();
        assert!(evaluate(&e, &vals(&[("a", json!(1)), ("b", json!(0))])).is_err());
        let missing = parse_expression("a + b").unwrap();
        assert!(evaluate(&missing, &vals(&[("a", json!(1))])).is_err());
        let bad_fn = parse_expression("SECRET(a)").unwrap();
        assert!(evaluate(&bad_fn, &vals(&[("a", json!(1))])).is_err());
    }

    #[test]
    fn tokenizes_for_highlighting() {
        let toks = tokenize("quantity * unit_price + 'x'").unwrap();
        assert!(toks
            .iter()
            .any(|(k, t)| *k == TokenKind::Column && t == "quantity"));
        assert!(toks.iter().any(|(k, _)| *k == TokenKind::Operator));
        assert!(toks.iter().any(|(k, t)| *k == TokenKind::Str && t == "'x'"));
        // Keywords vs columns.
        let kw = tokenize("true ANDx").unwrap();
        assert_eq!(kw[0].0, TokenKind::Keyword);
        assert_eq!(kw[1].0, TokenKind::Column);
    }
}
