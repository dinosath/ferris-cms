//! Lexer for the Formula DSL.

use crate::errors::{FormulaError, Result, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Number(String),
    Str(String),
    Ident(String),
    // keywords
    And,
    Or,
    Not,
    Case,
    When,
    Then,
    Else,
    End,
    Is,
    Null,
    True,
    False,
    Where,
    // operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    // punctuation
    LParen,
    RParen,
    Comma,
    Dot,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: Tok,
    pub span: Span,
    pub lexeme: String,
}

impl Token {
    fn new(kind: Tok, start: usize, end: usize, lexeme: String) -> Self {
        Self {
            kind,
            span: Span::new(start, end),
            lexeme,
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_ident_cont(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

pub fn lex(src: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    let n = chars.len();

    macro_rules! push {
        ($kind:expr, $start:expr, $lex:expr) => {{
            out.push(Token::new($kind, $start, i, $lex));
        }};
    }

    while i < n {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                i += 1;
            }
            '(' => {
                push!(Tok::LParen, i, "(".into());
                i += 1;
            }
            ')' => {
                push!(Tok::RParen, i, ")".into());
                i += 1;
            }
            ',' => {
                push!(Tok::Comma, i, ",".into());
                i += 1;
            }
            '.' => {
                push!(Tok::Dot, i, ".".into());
                i += 1;
            }
            '+' => {
                push!(Tok::Plus, i, "+".into());
                i += 1;
            }
            '-' => {
                push!(Tok::Minus, i, "-".into());
                i += 1;
            }
            '*' => {
                push!(Tok::Star, i, "*".into());
                i += 1;
            }
            '/' => {
                push!(Tok::Slash, i, "/".into());
                i += 1;
            }
            '%' => {
                push!(Tok::Percent, i, "%".into());
                i += 1;
            }
            '=' => {
                push!(Tok::Eq, i, "=".into());
                i += 1;
            }
            '!' => {
                if i + 1 < n && chars[i + 1] == '=' {
                    push!(Tok::Neq, i, "!=".into());
                    i += 2;
                } else {
                    return Err(err_at(src, i, "unexpected `!`"));
                }
            }
            '>' => {
                if i + 1 < n && chars[i + 1] == '=' {
                    push!(Tok::Gte, i, ">=".into());
                    i += 2;
                } else {
                    push!(Tok::Gt, i, ">".into());
                    i += 1;
                }
            }
            '<' => {
                if i + 1 < n && chars[i + 1] == '=' {
                    push!(Tok::Lte, i, "<=".into());
                    i += 2;
                } else if i + 1 < n && chars[i + 1] == '>' {
                    push!(Tok::Neq, i, "<>".into());
                    i += 2;
                } else {
                    push!(Tok::Lt, i, "<".into());
                    i += 1;
                }
            }
            '"' | '\'' => {
                let quote = c;
                let start = i;
                i += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < n {
                    let ch = chars[i];
                    if ch == quote {
                        // handle doubled quotes as escape
                        if i + 1 < n && chars[i + 1] == quote {
                            s.push(quote);
                            i += 2;
                            continue;
                        }
                        closed = true;
                        i += 1;
                        break;
                    }
                    s.push(ch);
                    i += 1;
                }
                if !closed {
                    return Err(err_at(src, start, "unterminated string literal"));
                }
                out.push(Token::new(Tok::Str(s.clone()), start, i, s));
            }
            c if c.is_ascii_digit() => {
                let start = i;
                let mut seen_dot = false;
                let mut lex = String::new();
                while i < n && chars[i].is_ascii_digit() {
                    lex.push(chars[i]);
                    i += 1;
                }
                if i < n && chars[i] == '.' {
                    seen_dot = true;
                    lex.push('.');
                    i += 1;
                    while i < n && chars[i].is_ascii_digit() {
                        lex.push(chars[i]);
                        i += 1;
                    }
                }
                let _ = seen_dot;
                out.push(Token::new(Tok::Number(lex.clone()), start, i, lex));
            }
            c if is_ident_start(c) => {
                let start = i;
                let mut lex = String::new();
                while i < n && is_ident_cont(chars[i]) {
                    lex.push(chars[i]);
                    i += 1;
                }
                let kind = match lex.to_ascii_uppercase().as_str() {
                    "AND" => Tok::And,
                    "OR" => Tok::Or,
                    "NOT" => Tok::Not,
                    "CASE" => Tok::Case,
                    "WHEN" => Tok::When,
                    "THEN" => Tok::Then,
                    "ELSE" => Tok::Else,
                    "END" => Tok::End,
                    "IS" => Tok::Is,
                    "NULL" => Tok::Null,
                    "TRUE" => Tok::True,
                    "FALSE" => Tok::False,
                    "WHERE" => Tok::Where,
                    _ => Tok::Ident(lex.clone()),
                };
                out.push(Token::new(kind, start, i, lex));
            }
            _ => {
                return Err(err_at(src, i, &format!("unexpected character `{c}`")));
            }
        }
    }
    out.push(Token::new(Tok::Eof, n, n, String::new()));
    Ok(out)
}

fn err_at(src: &str, pos: usize, msg: &str) -> FormulaError {
    let (line, col) = line_col(src, pos);
    FormulaError {
        expression: Some(src.to_string()),
        location: Some((line, col)),
        message: msg.to_string(),
        expected: None,
        actual: None,
    }
}

/// Compute 1-based (line, col) for a byte offset (approximate for the lexer,
/// which handles ASCII identifiers/operators).
pub fn line_col(src: &str, pos: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (idx, ch) in src.char_indices() {
        if idx >= pos {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_basic() {
        let toks = lex("quantity * unit_price").unwrap();
        let kinds: Vec<_> = toks
            .iter()
            .map(|t| t.kind.clone())
            .filter(|t| *t != Tok::Eof)
            .collect();
        assert_eq!(
            kinds,
            vec![
                Tok::Ident("quantity".into()),
                Tok::Star,
                Tok::Ident("unit_price".into())
            ]
        );
    }

    #[test]
    fn number_and_string() {
        let toks = lex("SUM(sales.balance WHERE sales.status = \"OPEN\")").unwrap();
        let kinds: Vec<_> = toks
            .iter()
            .map(|t| t.kind.clone())
            .filter(|t| *t != Tok::Eof)
            .collect();
        assert_eq!(
            kinds,
            vec![
                Tok::Ident("SUM".into()),
                Tok::LParen,
                Tok::Ident("sales".into()),
                Tok::Dot,
                Tok::Ident("balance".into()),
                Tok::Where,
                Tok::Ident("sales".into()),
                Tok::Dot,
                Tok::Ident("status".into()),
                Tok::Eq,
                Tok::Str("OPEN".into()),
                Tok::RParen
            ]
        );
    }

    #[test]
    fn operators_and_precedence_tokens() {
        let toks = lex("quantity + unit_price * 2 >= 3.5").unwrap();
        let kinds: Vec<_> = toks
            .iter()
            .map(|t| t.kind.clone())
            .filter(|t| *t != Tok::Eof)
            .collect();
        assert_eq!(
            kinds,
            vec![
                Tok::Ident("quantity".into()),
                Tok::Plus,
                Tok::Ident("unit_price".into()),
                Tok::Star,
                Tok::Number("2".into()),
                Tok::Gte,
                Tok::Number("3.5".into())
            ]
        );
    }

    #[test]
    fn keywords_are_uppercased_tokens() {
        let toks = lex("COALESCE(discount, 0)").unwrap();
        assert!(matches!(toks[0].kind, Tok::Ident(_)));
        let toks = lex("CASE WHEN a THEN b ELSE c END").unwrap();
        assert_eq!(toks[0].kind, Tok::Case);
        assert_eq!(toks[1].kind, Tok::When);
        assert_eq!(toks[3].kind, Tok::Then);
        assert_eq!(toks[5].kind, Tok::Else);
        assert_eq!(toks[7].kind, Tok::End);
    }

    #[test]
    fn unterminated_string_errors() {
        assert!(lex("a = \"oops").is_err());
    }
}
