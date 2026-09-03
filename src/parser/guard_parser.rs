// SPDX-License-Identifier: MIT
use std::fmt;
use crate::core::guard::GuardExpression;
use crate::core::guard_value::GuardValue;

#[derive(Debug, Clone)]
pub struct GuardParseError {
    message: String,
    source: String,
}

impl GuardParseError {
    pub fn new(message: String, source: String) -> Self {
        Self { message, source }
    }
}

impl fmt::Display for GuardParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in: {}", self.message, self.source)
    }
}

impl std::error::Error for GuardParseError {}

/// Parse a guard expression from Lua-like syntax.
pub fn parse_guard(text: &str) -> Result<GuardExpression, GuardParseError> {
    let stripped = strip_comments(text);
    if stripped.trim().is_empty() {
        return Ok(GuardExpression::always_true());
    }
    let mut parser = Parser::new(&stripped, text);
    let expr = parser.parse_expression()?;
    parser.expect_end()?;
    Ok(expr)
}

pub fn try_parse_guard(text: &str) -> Result<GuardExpression, GuardParseError> {
    parse_guard(text)
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-' {
            if let Some(&'-') = chars.peek() {
                chars.next();
                if let Some(&'[') = chars.peek() {
                    chars.next();
                    if let Some(&'[') = chars.peek() {
                        chars.next();
                        // Block comment --[[ ... ]]
                        let mut depth = 1;
                        while let Some(c) = chars.next() {
                            if c == ']' && chars.next() == Some(']') {
                                depth -= 1;
                                if depth == 0 { break; }
                            } else if c == '[' && chars.next() == Some('[') {
                                depth += 1;
                            }
                        }
                        out.push(' ');
                        continue;
                    }
                }
                // Line comment -- ...
                while let Some(c) = chars.next() {
                    if c == '\n' { break; }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Variable,
    Number,
    Text,
    Operator,
    And,
    Or,
    Not,
    True,
    False,
    Nil,
    OpenParen,
    CloseParen,
    Comma,
    Name,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    value: String,
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    source: String,
}

impl Parser {
    fn new(text: &str, original: &str) -> Self {
        let tokens = tokenize(text, original);
        Self { tokens, pos: 0, source: original.to_string() }
    }

    fn parse_expression(&mut self) -> Result<GuardExpression, GuardParseError> {
        let mut node = self.parse_conjunction()?;
        while self.peek() == TokenKind::Or {
            self.take();
            let right = self.parse_conjunction()?;
            node = GuardExpression::Or(Box::new(node), Box::new(right));
        }
        Ok(node)
    }

    fn parse_conjunction(&mut self) -> Result<GuardExpression, GuardParseError> {
        let mut node = self.parse_comparison()?;
        while self.peek() == TokenKind::And {
            self.take();
            let right = self.parse_comparison()?;
            node = GuardExpression::And(Box::new(node), Box::new(right));
        }
        Ok(node)
    }

    fn parse_comparison(&mut self) -> Result<GuardExpression, GuardParseError> {
        let left = self.parse_unary()?;
        if self.peek() == TokenKind::Operator {
            let op = self.take().value;
            let right = self.parse_unary()?;
            return Ok(GuardExpression::Comparison(op, Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<GuardExpression, GuardParseError> {
        if self.peek() == TokenKind::Not {
            self.take();
            let inner = self.parse_unary()?;
            return Ok(GuardExpression::Not(Box::new(inner)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<GuardExpression, GuardParseError> {
        match self.peek() {
            TokenKind::OpenParen => {
                self.take();
                let inner = self.parse_expression()?;
                self.expect(TokenKind::CloseParen)?;
                Ok(inner)
            }
            TokenKind::Variable => {
                let name = self.take().value;
                Ok(GuardExpression::Variable(name))
            }
            TokenKind::True => { self.take(); Ok(GuardExpression::Literal(GuardValue::from_boolean(true))) }
            TokenKind::False => { self.take(); Ok(GuardExpression::Literal(GuardValue::from_boolean(false))) }
            TokenKind::Nil => { self.take(); Ok(GuardExpression::Literal(GuardValue::unknown())) }
            TokenKind::Number => {
                let raw = self.take().value;
                let num = raw.parse::<f64>().map_err(|_| GuardParseError::new(format!("bad number '{raw}'"), self.source.clone()))?;
                Ok(GuardExpression::Literal(GuardValue::from_number(num)))
            }
            TokenKind::Text => {
                let text = self.take().value;
                Ok(GuardExpression::Literal(GuardValue::from_text(text)))
            }
            TokenKind::Name => {
                let name = self.take().value;
                let mut args = Vec::new();
                if self.peek() == TokenKind::OpenParen {
                    self.take();
                    while self.peek() != TokenKind::CloseParen {
                        args.push(self.parse_expression()?);
                        if self.peek() == TokenKind::Comma {
                            self.take();
                        } else if self.peek() != TokenKind::CloseParen {
                            return Err(GuardParseError::new("bad argument list".into(), self.source.clone()));
                        }
                    }
                    self.expect(TokenKind::CloseParen)?;
                }
                Ok(GuardExpression::Call(name, args))
            }
            _ => Err(GuardParseError::new("unexpected token".into(), self.source.clone())),
        }
    }

    fn peek(&self) -> TokenKind {
        self.tokens.get(self.pos).map(|t| t.kind.clone()).unwrap_or(TokenKind::Name)
    }

    fn take(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        t
    }

    fn expect(&mut self, kind: TokenKind) -> Result<(), GuardParseError> {
        if self.peek() == kind {
            self.take();
            Ok(())
        } else {
            Err(GuardParseError::new(format!("expected {kind:?}"), self.source.clone()))
        }
    }

    fn expect_end(&mut self) -> Result<(), GuardParseError> {
        if self.pos != self.tokens.len() {
            Err(GuardParseError::new("trailing tokens".into(), self.source.clone()))
        } else {
            Ok(())
        }
    }
}

fn tokenize(text: &str, _original: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() { continue; }

        match c {
            '(' => tokens.push(Token { kind: TokenKind::OpenParen, value: "(".into() }),
            ')' => tokens.push(Token { kind: TokenKind::CloseParen, value: ")".into() }),
            ',' => tokens.push(Token { kind: TokenKind::Comma, value: ",".into() }),
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    if c == '"' { break; }
                    s.push(c);
                }
                tokens.push(Token { kind: TokenKind::Text, value: s });
            }
            c if c.is_ascii_digit() => {
                let mut s = String::new();
                s.push(c);
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' { s.push(chars.next().unwrap()); } else { break; }
                }
                tokens.push(Token { kind: TokenKind::Number, value: s });
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut s = String::new();
                s.push(c);
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '.' { s.push(chars.next().unwrap()); } else { break; }
                }
                if s == "Variable" {
                    // Try to read Variable["name"]
                    let _save_pos = chars.clone();
                    let mut temp = chars.clone();
                    let mut ws = String::new();
                    while let Some(&c) = temp.peek() { if c.is_whitespace() { ws.push(temp.next().unwrap()); } else { break; } }
                    if temp.next() == Some('[') {
                        let mut ws2 = String::new();
                        while let Some(&c) = temp.peek() { if c.is_whitespace() { ws2.push(temp.next().unwrap()); } else { break; } }
                        if temp.next() == Some('"') {
                            let mut name = String::new();
                            while let Some(c) = temp.next() {
                                if c == '"' { break; }
                                name.push(c);
                            }
                            let mut ws3 = String::new();
                            while let Some(&c) = temp.peek() { if c.is_whitespace() { ws3.push(temp.next().unwrap()); } else { break; } }
                            if temp.next() == Some(']') {
                                chars = temp;
                                tokens.push(Token { kind: TokenKind::Variable, value: name });
                                continue;
                            }
                        }
                    }
                    // Not a Variable[...], treat as name
                    tokens.push(Token { kind: TokenKind::Name, value: s });
                } else {
                    let kind = match s.as_str() {
                        "and" => TokenKind::And,
                        "or" => TokenKind::Or,
                        "not" => TokenKind::Not,
                        "true" => TokenKind::True,
                        "false" => TokenKind::False,
                        "nil" => TokenKind::Nil,
                        _ => TokenKind::Name,
                    };
                    tokens.push(Token { kind, value: s });
                }
            }
            c if "+-*/%^#".contains(c) || c == '>' || c == '<' || c == '=' || c == '~' => {
                let mut s = String::new();
                s.push(c);
                if let Some(&c2) = chars.peek() {
                    let two = format!("{c}{c2}");
                    if ["==", "~=", ">=", "<="].contains(&two.as_str()) {
                        s.push(chars.next().unwrap());
                    }
                }
                tokens.push(Token { kind: TokenKind::Operator, value: s });
            }
            _ => return vec![Token { kind: TokenKind::Name, value: format!("unexpected: {c}") }],
        }
    }
    tokens
}
