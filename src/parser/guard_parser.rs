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
    let mut parser = Parser::new(&stripped, text)?;
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
    /// Past the last token.
    ///
    /// A kind of its own rather than `Name`, which is what it used to report. A parser that
    /// runs out of input in the middle of an expression - `not`, `x and`, `f(` - then took
    /// the Name branch and indexed past the end of the token list, which PANICKED. A guard
    /// comes out of a dialogue database a game patch or another mod can change, so an
    /// unparseable one has to be an error and never a crash.
    End,
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
    fn new(text: &str, original: &str) -> Result<Self, GuardParseError> {
        let tokens = tokenize(text, original)?;
        Ok(Self { tokens, pos: 0, source: original.to_string() })
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

        // A NEGATIVE NUMBER. The tokeniser reads `-` as an operator, so `x > -1` arrived
        // here as an operator where a value was wanted and the whole guard was refused -
        // which for a guard means Unknown, which means permissive, which means a marker
        // that is wrong with nothing to say so.
        //
        // No guard in the shipped database has one, which is why the corpus never found
        // this; a generated guard found it immediately. Folded into the literal rather than
        // given a Negate node, because the language has no arithmetic and the only thing a
        // minus can be here is part of a number.
        if self.peek() == TokenKind::Operator && self.peek_value() == "-" {
            if self.peek_at(1) == TokenKind::Number {
                self.take();
                let raw = self.take().value;
                let number = raw.parse::<f64>().map_err(|_| {
                    GuardParseError::new(format!("bad number '-{raw}'"), self.source.clone())
                })?;
                return Ok(GuardExpression::Literal(GuardValue::from_number(-number)));
            }
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

    /// The kind `offset` tokens ahead, for the one decision that needs to look past the
    /// next token: whether a minus begins a negative number or is something else.
    fn peek_at(&self, offset: usize) -> TokenKind {
        self.tokens.get(self.pos + offset).map(|t| t.kind.clone()).unwrap_or(TokenKind::End)
    }

    /// The next token's text, or empty at the end of the input.
    fn peek_value(&self) -> &str {
        self.tokens.get(self.pos).map(|t| t.value.as_str()).unwrap_or("")
    }

    fn peek(&self) -> TokenKind {
        self.tokens.get(self.pos).map(|t| t.kind.clone()).unwrap_or(TokenKind::End)
    }

    /// The next token, consumed.
    ///
    /// Only ever called where [`Self::peek`] has already said what is there, so the end is
    /// unreachable - but it returns an End token rather than indexing, because "unreachable"
    /// and "indexes a vector" together is how this panicked on `not` with nothing after it.
    fn take(&mut self) -> Token {
        match self.tokens.get(self.pos) {
            Some(token) => {
                let token = token.clone();
                self.pos += 1;
                token
            }
            None => Token { kind: TokenKind::End, value: String::new() },
        }
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

/// Splits guard text into tokens, or fails on a character it cannot read.
///
/// Failing is the point. This previously returned, for any unexpected character, a single
/// Name token holding the text "unexpected: X" - DISCARDING EVERY TOKEN READ SO FAR. That
/// parsed cleanly as a reference to a variable of that name, satisfied the end-of-input
/// check, and evaluated to Unknown because no world has heard of it. So an unreadable
/// guard became a permissive one and nothing reported it, and a corpus test asserting
/// that every guard parses could not have failed whatever it was given.
fn tokenize(text: &str, _original: &str) -> Result<Vec<Token>, GuardParseError> {
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
            _ => {
                return Err(GuardParseError::new(
                    format!("unexpected character {c:?}"),
                    text.to_string(),
                ))
            }
        }
    }
    Ok(tokens)
}
