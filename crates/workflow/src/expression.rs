//! n8n-style expression engine (expression layer).
//!
//! Expressions are written `{{ ... }}` and reference trigger data, previous
//! node output, the current item, workflow/execution metadata and environment
//! variables. Examples:
//!
//! ```text
//! {{$json.email}}
//! {{$node["Get Product"].json.price}}
//! {{$workflow.id}}
//! {{$execution.id}}
//! {{$env.API_URL}}
//! ```
//!
//! Evaluation is **safe**: there is no general-purpose code execution, only a
//! small arithmetic/logic/string language plus property access and a curated
//! set of built-in functions. Values never leak credentials into logs because
//! the context is populated by `services` from non-sensitive execution data.

use std::collections::HashMap;
use std::fmt;

use serde_json::{Map, Value};

/// Error produced while parsing/evaluating an expression.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionError {
    pub message: String,
}

impl fmt::Display for ExpressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExpressionError {}

impl ExpressionError {
    fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

/// Context available to expressions.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// Current item's JSON object (what `$json` refers to).
    pub json: Value,
    /// Current item index (0-based).
    pub item_index: i64,
    /// Previous node outputs, keyed by node name. Each value mirrors the shape
    /// `$node["Name"]` exposes (e.g. `.json`, `.data`).
    pub nodes: HashMap<String, Value>,
    /// Workflow metadata (`id`, `name`, `variables`).
    pub workflow: Value,
    /// Execution metadata (`id`, `mode`, `trigger`).
    pub execution: Value,
    /// Environment variables.
    pub env: HashMap<String, String>,
    /// `$now` timestamp (RFC3339) and `$today` (date).
    pub now: String,
    pub today: String,
}

impl Context {
    /// A minimal context for unit tests.
    pub fn minimal() -> Self {
        Self {
            now: "2026-01-01T00:00:00.000Z".into(),
            today: "2026-01-01".into(),
            ..Default::default()
        }
    }
}

/// Whether a template contains any `{{ }}` expression.
pub fn contains_expression(s: &str) -> bool {
    s.contains("{{")
}

/// Evaluate a full template. When the entire template is a single `{{ ... }}`
/// expression, the raw value is returned (typed). Otherwise `{{ ... }}` blocks
/// are interpolated as strings.
pub fn evaluate(template: &str, ctx: &Context) -> Result<Value, ExpressionError> {
    let inner = single_expression(template);
    if let Some(inner) = inner {
        return eval_expression(inner.trim(), ctx);
    }
    interpolate(template, ctx)
}

/// Extract the inner text if `template` is exactly one `{{ ... }}` expression
/// (allowing surrounding whitespace), otherwise `None`.
fn single_expression(template: &str) -> Option<&str> {
    let t = template.trim();
    if t.len() < 4 {
        return None;
    }
    if let (Some(stripped)) = t.strip_prefix("{{") {
        if let Some(rest) = stripped.strip_suffix("}}") {
            if !rest.contains("{{") && !rest.contains("}}") {
                return Some(rest);
            }
        }
    }
    None
}

/// Replace each `{{ ... }}` block with its stringified value.
fn interpolate(template: &str, ctx: &Context) -> Result<Value, ExpressionError> {
    let mut out = String::new();
    let bytes: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '{' && i + 1 < bytes.len() && bytes[i + 1] == '{' {
            // Find the closing `}}`.
            let mut j = i + 2;
            let mut depth = 1;
            while j + 1 < bytes.len() {
                if bytes[j] == '{' && bytes[j + 1] == '{' {
                    depth += 1;
                    j += 2;
                } else if bytes[j] == '}' && bytes[j + 1] == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    j += 2;
                } else {
                    j += 1;
                }
            }
            if depth != 0 || j + 1 >= bytes.len() {
                return Err(ExpressionError::new("unbalanced expression braces"));
            }
            let inner: String = bytes[i + 2..j].iter().collect();
            let value = eval_expression(inner.trim(), ctx)?;
            out.push_str(&stringify(&value));
            i = j + 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(Value::String(out))
}

/// Stringify a value for interpolation.
fn stringify(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Tokenizer + recursive-descent parser
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Ref(String),
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    AndAnd,
    OrOr,
    Bang,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
    Comma,
    Question,
    Colon,
}

fn tokenize(input: &str) -> Result<Vec<Tok>, ExpressionError> {
    let chars: Vec<char> = input.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '(' => {
                toks.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                toks.push(Tok::RParen);
                i += 1;
            }
            '[' => {
                toks.push(Tok::LBracket);
                i += 1;
            }
            ']' => {
                toks.push(Tok::RBracket);
                i += 1;
            }
            '.' => {
                toks.push(Tok::Dot);
                i += 1;
            }
            ',' => {
                toks.push(Tok::Comma);
                i += 1;
            }
            ':' => {
                toks.push(Tok::Colon);
                i += 1;
            }
            '?' => {
                toks.push(Tok::Question);
                i += 1;
            }
            '+' => {
                toks.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                toks.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                toks.push(Tok::Star);
                i += 1;
            }
            '/' => {
                toks.push(Tok::Slash);
                i += 1;
            }
            '&' if i + 1 < chars.len() && chars[i + 1] == '&' => {
                toks.push(Tok::AndAnd);
                i += 2;
            }
            '|' if i + 1 < chars.len() && chars[i + 1] == '|' => {
                toks.push(Tok::OrOr);
                i += 2;
            }
            '=' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                toks.push(Tok::Eq);
                i += 2;
            }
            '!' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                toks.push(Tok::Ne);
                i += 2;
            }
            '!' => {
                toks.push(Tok::Bang);
                i += 1;
            }
            '>' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                toks.push(Tok::Ge);
                i += 2;
            }
            '>' => {
                toks.push(Tok::Gt);
                i += 1;
            }
            '<' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                toks.push(Tok::Le);
                i += 2;
            }
            '<' => {
                toks.push(Tok::Lt);
                i += 1;
            }
            '$' => {
                // Reference: $json, $node, $workflow, ...
                let mut name = String::new();
                i += 1;
                while i < chars.len() && is_ident_char(chars[i]) {
                    name.push(chars[i]);
                    i += 1;
                }
                if name.is_empty() {
                    return Err(ExpressionError::new("empty reference after '$'"));
                }
                toks.push(Tok::Ref(name));
            }
            '"' | '\'' => {
                let quote = c;
                i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != quote {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                        match chars[i] {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            other => s.push(other),
                        }
                        i += 1;
                    } else {
                        s.push(chars[i]);
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err(ExpressionError::new("unterminated string literal"));
                }
                i += 1;
                toks.push(Tok::Str(s));
            }
            c if c.is_ascii_digit() || (c == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) => {
                let mut num = String::new();
                num.push(c);
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == 'e' || chars[i] == 'E')
                {
                    num.push(chars[i]);
                    i += 1;
                }
                let n: f64 = num
                    .parse()
                    .map_err(|_| ExpressionError::new(format!("invalid number '{num}'")))?;
                toks.push(Tok::Num(n));
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut ident = String::new();
                ident.push(c);
                i += 1;
                while i < chars.len() && is_ident_char(chars[i]) {
                    ident.push(chars[i]);
                    i += 1;
                }
                toks.push(Tok::Ident(ident));
            }
            other => {
                return Err(ExpressionError::new(format!(
                    "unexpected character '{other}' in expression"
                )))
            }
        }
    }
    Ok(toks)
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, t: &Tok, what: &str) -> Result<(), ExpressionError> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(ExpressionError::new(format!(
                "expected {what} in expression"
            )))
        }
    }

    fn parse(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        let v = self.or(ctx)?;
        if self.peek().is_some() {
            return Err(ExpressionError::new("unexpected trailing tokens in expression"));
        }
        Ok(v)
    }

    fn or(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        let mut v = self.and(ctx)?;
        while self.eat(&Tok::OrOr) {
            let r = self.and(ctx)?;
            v = Value::Bool(truthy(&v) || truthy(&r));
        }
        Ok(v)
    }
    fn and(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        let mut v = self.equality(ctx)?;
        while self.eat(&Tok::AndAnd) {
            let r = self.equality(ctx)?;
            v = Value::Bool(truthy(&v) && truthy(&r));
        }
        Ok(v)
    }
    fn equality(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        let mut v = self.comparison(ctx)?;
        loop {
            if self.eat(&Tok::Eq) {
                let r = self.comparison(ctx)?;
                v = Value::Bool(loose_eq(&v, &r));
            } else if self.eat(&Tok::Ne) {
                let r = self.comparison(ctx)?;
                v = Value::Bool(!loose_eq(&v, &r));
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn comparison(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        let mut v = self.additive(ctx)?;
        loop {
            if self.eat(&Tok::Gt) {
                let r = self.additive(ctx)?;
                v = Value::Bool(cmp(&v, &r) == Some(std::cmp::Ordering::Greater));
            } else if self.eat(&Tok::Ge) {
                let r = self.additive(ctx)?;
                v = Value::Bool(cmp(&v, &r) != Some(std::cmp::Ordering::Less));
            } else if self.eat(&Tok::Lt) {
                let r = self.additive(ctx)?;
                v = Value::Bool(cmp(&v, &r) == Some(std::cmp::Ordering::Less));
            } else if self.eat(&Tok::Le) {
                let r = self.additive(ctx)?;
                v = Value::Bool(cmp(&v, &r) != Some(std::cmp::Ordering::Greater));
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn additive(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        let mut v = self.term(ctx)?;
        loop {
            if self.eat(&Tok::Plus) {
                let r = self.term(ctx)?;
                v = add_values(&v, &r);
            } else if self.eat(&Tok::Minus) {
                let r = self.term(ctx)?;
                v = num_bin(&v, &r, |a, b| a - b);
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn term(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        let mut v = self.unary(ctx)?;
        loop {
            if self.eat(&Tok::Star) {
                let r = self.unary(ctx)?;
                v = num_bin(&v, &r, |a, b| a * b);
            } else if self.eat(&Tok::Slash) {
                let r = self.unary(ctx)?;
                v = num_bin(&v, &r, |a, b| a / b);
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn unary(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        if self.eat(&Tok::Bang) {
            let v = self.unary(ctx)?;
            return Ok(Value::Bool(!truthy(&v)));
        }
        if self.eat(&Tok::Minus) {
            let v = self.unary(ctx)?;
            return Ok(num_bin(&Value::Null, &v, |_, b| -b));
        }
        self.postfix(ctx)
    }
    fn postfix(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        let mut v = self.primary(ctx)?;
        loop {
            if self.eat(&Tok::Dot) {
                let name = match self.next() {
                    Some(Tok::Ident(n)) => n,
                    _ => return Err(ExpressionError::new("expected field name after '.'")),
                };
                v = get_field(&v, &name);
            } else if self.eat(&Tok::LBracket) {
                if self.peek() == Some(&Tok::RBracket) {
                    // slice `[0]` handled below; empty bracket is invalid
                    return Err(ExpressionError::new("empty index"));
                }
                let idx = self.expression_in_brackets(ctx)?;
                self.expect(&Tok::RBracket, "']'")?;
                v = index_value(&v, &idx);
            } else if self.peek() == Some(&Tok::LParen) {
                // Function call.
                let fname = match v {
                    Value::String(ref s) => s.clone(),
                    _ => return Err(ExpressionError::new("expected function name")),
                };
                self.next(); // consume '('
                let mut args = Vec::new();
                if self.peek() != Some(&Tok::RParen) {
                    loop {
                        args.push(self.or(ctx)?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&Tok::RParen, "')'")?;
                v = call_function(&fname, args)?;
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn expression_in_brackets(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        self.or(ctx)
    }
    fn primary(&mut self, ctx: &Context) -> Result<Value, ExpressionError> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(number_value(n)),
            Some(Tok::Str(s)) => Ok(Value::String(s)),
            Some(Tok::Ident(id)) => {
                // Function call like `upper(...)` when followed by `(`.
                if self.peek() == Some(&Tok::LParen) {
                    self.next(); // consume '('
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.or(ctx)?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&Tok::RParen, "')'")?;
                    return call_function(&id, args);
                }
                match id.as_str() {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    "null" => Ok(Value::Null),
                    other => Err(ExpressionError::new(format!(
                        "unknown identifier '{other}' (did you mean a $-reference?)"
                    ))),
                }
            }
            Some(Tok::Ref(name)) => self.resolve_reference(&name, ctx),
            Some(Tok::LParen) => {
                let v = self.or(ctx)?;
                self.expect(&Tok::RParen, "')'")?;
                Ok(v)
            }
            other => Err(ExpressionError::new(format!(
                "unexpected token in expression: {other:?}"
            ))),
        }
    }

    /// Resolve a `$reference` (which is followed by postfix access handled by
    /// `postfix`).
    fn resolve_reference(&mut self, name: &str, ctx: &Context) -> Result<Value, ExpressionError> {
        match name {
            "json" => Ok(ctx.json.clone()),
            "itemIndex" => Ok(Value::from(ctx.item_index)),
            "workflow" => Ok(ctx.workflow.clone()),
            "execution" => Ok(ctx.execution.clone()),
            "now" => Ok(Value::String(ctx.now.clone())),
            "today" => Ok(Value::String(ctx.today.clone())),
            "env" => {
                // `$env.X` is a postfix access; represent env as a map.
                let mut map = Map::new();
                for (k, v) in &ctx.env {
                    map.insert(k.clone(), Value::String(v.clone()));
                }
                Ok(Value::Object(map))
            }
            "node" => {
                // `$node["Name"]` is handled by bracket access in postfix.
                // Represent the node map itself as a JSON object so that
                // `$node["Name"].json` resolves.
                let mut map = Map::new();
                for (k, v) in &ctx.nodes {
                    map.insert(k.clone(), v.clone());
                }
                Ok(Value::Object(map))
            }
            "items" => Ok(Value::Null),
            other => Err(ExpressionError::new(format!(
                "unknown expression reference '${other}'"
            ))),
        }
    }
}

fn number_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        Value::from(n as i64)
    } else {
        Value::from(n)
    }
}

pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Null, Value::Null) => true,
        (Value::Number(_), Value::String(_)) | (Value::String(_), Value::Number(_)) => {
            a.to_string() == b.to_string()
        }
        (Value::Array(x), Value::Array(y)) => x == y,
        (Value::Object(x), Value::Object(y)) => x == y,
        _ => false,
    }
}

fn cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64()),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

fn add_values(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::String(x), y) => Value::String(format!("{x}{}", stringify(y))),
        (x, Value::String(y)) => Value::String(format!("{}{y}", stringify(x))),
        _ => num_bin(a, b, |x, y| x + y),
    }
}

fn num_bin(a: &Value, b: &Value, f: impl Fn(f64, f64) -> f64) -> Value {
    let an = to_f64(a);
    let bn = to_f64(b);
    match (an, bn) {
        (Some(x), Some(y)) => {
            let r = f(x, y);
            number_value(r)
        }
        _ => Value::Null,
    }
}

fn to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        Value::Null => Some(0.0),
        _ => None,
    }
}

fn get_field(v: &Value, name: &str) -> Value {
    match v {
        Value::Object(map) => map.get(name).cloned().unwrap_or(Value::Null),
        Value::String(_) => Value::Null,
        other => {
            let _ = other;
            Value::Null
        }
    }
}

fn index_value(v: &Value, idx: &Value) -> Value {
    match v {
        Value::Array(arr) => match idx {
            Value::Number(n) => n
                .as_i64()
                .and_then(|i| {
                    if i < 0 {
                        let len = arr.len() as i64;
                        arr.get((len + i) as usize)
                    } else {
                        arr.get(i as usize)
                    }
                })
                .cloned()
                .unwrap_or(Value::Null),
            Value::String(s) => arr
                .iter()
                .find(|item| item.get(s) == Some(&Value::Bool(true)))
                .cloned()
                .unwrap_or(Value::Null),
            _ => Value::Null,
        },
        Value::Object(map) => match idx {
            Value::String(s) => map.get(s).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        },
        _ => Value::Null,
    }
}

/// Built-in functions available in expressions.
fn call_function(name: &str, args: Vec<Value>) -> Result<Value, ExpressionError> {
    let first = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    match name {
        "string" => Ok(Value::String(stringify(&first(0)))),
        "number" => Ok(Value::from(to_f64(&first(0)).unwrap_or(0.0))),
        "bool" => Ok(Value::Bool(truthy(&first(0)))),
        "length" => Ok(Value::from(match &first(0) {
            Value::String(s) => s.chars().count() as i64,
            Value::Array(a) => a.len() as i64,
            Value::Object(o) => o.len() as i64,
            _ => 0,
        })),
        "lower" | "lowerCase" => Ok(Value::String(stringify(&first(0)).to_lowercase())),
        "upper" | "upperCase" => Ok(Value::String(stringify(&first(0)).to_uppercase())),
        "trim" => Ok(Value::String(stringify(&first(0)).trim().to_string())),
        "floor" => Ok(Value::from(to_f64(&first(0)).unwrap_or(0.0).floor() as i64)),
        "ceil" => Ok(Value::from(to_f64(&first(0)).unwrap_or(0.0).ceil() as i64)),
        "round" => Ok(Value::from(to_f64(&first(0)).unwrap_or(0.0).round() as i64)),
        "contains" => {
            let hay = stringify(&first(0));
            let needle = stringify(&first(1));
            Ok(Value::Bool(hay.contains(&needle)))
        }
        "startsWith" => Ok(Value::Bool(stringify(&first(0)).starts_with(&stringify(&first(1))))),
        "endsWith" => Ok(Value::Bool(stringify(&first(0)).ends_with(&stringify(&first(1))))),
        "date" => Ok(Value::String(stringify(&first(0)))),
        "now" => Ok(Value::String("".to_string())),
        "json" => Ok(first(0)),
        "not" => Ok(Value::Bool(!truthy(&first(0)))),
        _ => Err(ExpressionError::new(format!(
            "unknown function '{name}' in expression"
        ))),
    }
}

/// Evaluate a bare expression body (without `{{ }}`).
pub fn eval_expression(expr: &str, ctx: &Context) -> Result<Value, ExpressionError> {
    if expr.trim().is_empty() {
        return Ok(Value::String(String::new()));
    }
    let toks = tokenize(expr)?;
    let mut parser = Parser { toks, pos: 0 };
    parser.parse(ctx)
}

/// Convenience: evaluate a template and return it as a String.
pub fn evaluate_string(template: &str, ctx: &Context) -> Result<String, ExpressionError> {
    let v = evaluate(template, ctx)?;
    Ok(stringify(&v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx_with(json: Value) -> Context {
        let mut ctx = Context::minimal();
        ctx.json = json;
        let mut nodes = HashMap::new();
        nodes.insert(
            "Get Product".to_string(),
            json!({ "json": { "price": 199.5, "name": "Widget" } }),
        );
        ctx.nodes = nodes;
        ctx.workflow = json!({ "id": 42, "name": "Demo", "variables": { "rate": 0.2 } });
        ctx.execution = json!({ "id": 7, "mode": "manual" });
        ctx.env.insert("API_URL".into(), "https://api.example.com".into());
        ctx
    }

    #[test]
    fn evaluates_references() {
        let ctx = ctx_with(json!({"email":"a@b.dev","age":30}));
        assert_eq!(
            evaluate_string("{{$json.email}}", &ctx).unwrap(),
            "a@b.dev"
        );
        assert_eq!(
            evaluate_string("{{$node[\"Get Product\"].json.price}}", &ctx).unwrap(),
            "199.5"
        );
        assert_eq!(evaluate_string("{{$workflow.id}}", &ctx).unwrap(), "42");
        assert_eq!(evaluate_string("{{$execution.id}}", &ctx).unwrap(), "7");
        assert_eq!(
            evaluate_string("{{$env.API_URL}}", &ctx).unwrap(),
            "https://api.example.com"
        );
        assert_eq!(
            evaluate_string("{{$workflow.variables.rate}}", &ctx).unwrap(),
            "0.2"
        );
    }

    #[test]
    fn evaluates_typed_single_expression() {
        let ctx = ctx_with(json!({"age":30}));
        // Single {{ }} returns the typed value.
        let v = evaluate("{{$json.age}}", &ctx).unwrap();
        assert_eq!(v, json!(30));
        // Arithmetic
        let v = evaluate("{{$json.age + 10}}", &ctx).unwrap();
        assert_eq!(v, json!(40));
    }

    #[test]
    fn interpolates_mixed_strings() {
        let ctx = ctx_with(json!({"name":"Ferris","age":30}));
        assert_eq!(
            evaluate_string("Hello {{$json.name}}, you are {{$json.age}}", &ctx).unwrap(),
            "Hello Ferris, you are 30"
        );
        assert_eq!(
            evaluate_string("Plain text with no expressions", &ctx).unwrap(),
            "Plain text with no expressions"
        );
    }

    #[test]
    fn evaluates_logic_and_comparisons() {
        let ctx = ctx_with(json!({"price":199.5,"stock":5}));
        assert_eq!(
            evaluate("{{$json.price > 100}}", &ctx).unwrap(),
            json!(true)
        );
        assert_eq!(evaluate("{{$json.stock > 0}}", &ctx).unwrap(), json!(true));
        assert_eq!(
            evaluate("{{$json.price > 100 && $json.stock > 0}}", &ctx).unwrap(),
            json!(true)
        );
        assert_eq!(
            evaluate("{{$json.price > 100 || $json.stock == 0}}", &ctx).unwrap(),
            json!(true)
        );
        assert_eq!(
            evaluate("{{!($json.price > 100)}}", &ctx).unwrap(),
            json!(false)
        );
        assert_eq!(
            evaluate("{{$json.stock != 0}}", &ctx).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn evaluates_functions_and_strings() {
        let ctx = ctx_with(json!({"name":"Ferris"}));
        assert_eq!(
            evaluate_string("{{upper($json.name)}}", &ctx).unwrap(),
            "FERRIS"
        );
        assert_eq!(
            evaluate_string("{{lower(\"ABC\")}}", &ctx).unwrap(),
            "abc"
        );
        assert_eq!(evaluate("{{length(\"hello\")}}", &ctx).unwrap(), json!(5));
        assert_eq!(
            evaluate_string("{{$json.name + \" the crab\"}}", &ctx).unwrap(),
            "Ferris the crab"
        );
        // $node access via bracket with dot navigation
        assert_eq!(
            evaluate_string("{{upper($node[\"Get Product\"].json.name)}}", &ctx).unwrap(),
            "WIDGET"
        );
    }

    #[test]
    fn safe_errors() {
        let ctx = ctx_with(json!({}));
        assert!(eval_expression("unknown", &ctx).is_err());
        assert!(eval_expression("$nonexistent", &ctx).is_err());
        assert!(evaluate("{{", &ctx).is_err());
        assert!(eval_expression("foo(", &ctx).is_err());
        // Unknown function fails.
        assert!(eval_expression("hack()", &ctx).is_err());
    }
}
