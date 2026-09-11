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
        assert_eq!(e.columns(), vec!["deals_won".to_string(), "deals_lost".into()]);
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
