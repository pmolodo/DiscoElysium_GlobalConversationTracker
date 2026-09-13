// SPDX-License-Identifier: MIT
use crate::core::guard::Guard;
use crate::core::guard_value::GuardValue;
use std::fmt;

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
pub fn parse_guard(text: &str) -> Result<Guard, GuardParseError> {
    let stripped = strip_comments(text);
    if stripped.trim().is_empty() {
        return Ok(Guard::always_true());
    }
    let mut parser = Parser::new(&stripped, text)?;
    let expr = parser.parse()?;
    parser.expect_end()?;
    Ok(expr)
}

pub fn try_parse_guard(text: &str) -> Result<Guard, GuardParseError> {
    parse_guard(text)
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-'
            && let Some(&'-') = chars.peek()
        {
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
                            if depth == 0 {
                                break;
                            }
                        } else if c == '[' && chars.next() == Some('[') {
                            depth += 1;
                        }
                    }
                    out.push(' ');
                    continue;
                }
            }
            // Line comment -- ...
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
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

/// How deep a guard's expression tree may go before it is refused.
///
/// ## Why there is still a limit, when nothing about a guard recurses any more
///
/// The parser below is ITERATIVE (de-bnjy.4): nesting costs entries in a `Vec` and nothing
/// on the stack, so no guard, however deep, can overflow while being read. What it produces
/// is a FLAT TABLE (de-eyk8.2), so freeing one is a deallocation rather than a walk, and
/// evaluating or rendering one is a sweep in index order rather than a descent.
///
/// One consumer still descends by choice: `GuardCompiler::compile` is demand-driven,
/// because a comparison answers from its operands' SHAPE without compiling either, and a
/// bottom-up sweep would build a decision diagram for every operand it never looks at. That
/// recursion is what this limit bounds. An overflow is not a panic: the guard page is hit,
/// Rust prints, the process ABORTS, and nothing can catch it. Inside the game that is the
/// player's session.
///
/// ## It counts the TREE, which is what the old limit did not
///
/// The limit this replaces counted the parser's re-entries. That was twice the depth a
/// reader counts - `not (` was two steps for one level - and needed the factor of two
/// explained every time. It also missed the shape that matters most: `a and b and c` chained
/// in a `while` loop, costing NO recursion to parse and producing a left-leaning tree as
/// deep as the chain is long. Ten thousand `and`s were accepted by the old bound and would
/// have overflowed whatever walked the result.
///
/// This is measured on the tree as it is built, so a long chain is bounded exactly as a deep
/// nest is.
///
/// ## Why this number
///
/// Far above real content, and bounding nothing but what is plainly not content.
///
/// - THE DEEPEST GUARD IN THE SHIPPED DATABASE IS ELEVEN LEVELS, of 26,210 non-empty ones,
///   and `tests/guard_depth.rs` re-parses all of them rather than arguing from the figure.
///   This is twenty-three times that, so the limit is nowhere near real content.
/// - IT IS NOT A STACK LIMIT. Nothing that walks a guard costs stack in proportion to its
///   depth - `measurements/guard_stack.rs` builds, evaluates, renders and compiles forty
///   thousand levels on a one-megabyte stack - so a guard past this is refused as a
///   database that is not what the game ships, rather than as one that would crash.
const MAX_DEPTH: usize = 256;

/// A value the parser has read, carrying how deep the tree under it goes.
///
/// The depth travels WITH the value because [`MAX_DEPTH`] bounds the tree, and the parser's
/// own stacks do not measure it: an operator is reduced as soon as one of equal or lower
/// binding power arrives, so a chain of ten thousand `and`s never has more than one entry
/// waiting while the tree under it grows ten thousand deep.
struct Operand {
    node: Guard,
    depth: usize,
}

/// An operator that has been read but not yet built, because its operands are not all in.
#[derive(Debug)]
enum Pending {
    Not,
    And,
    Or,
    Compare(String),
}

/// How tightly each operator holds its operands.
///
/// The order is the grammar this parser replaced: `or` loosest, then `and`, then a
/// comparison, then a prefix `not` tightest - which is why `not a == b` is `(not a) == b`
/// and `not a and b` is `(not a) and b`.
impl Pending {
    fn binding_power(&self) -> u8 {
        match self {
            Pending::Or => 1,
            Pending::And => 2,
            Pending::Compare(_) => 3,
            Pending::Not => 4,
        }
    }
}

/// A bracket that is open, and what closing it will mean.
enum FrameKind {
    /// The guard as a whole. Never popped, so the stacks always have a floor.
    Whole,
    /// `( ... )`, whose value is simply what is inside it.
    Group,
    /// `name( ... )`, gathering arguments until the closing parenthesis.
    Call {
        name: String,
        args: Vec<Guard>,
        deepest: usize,
    },
}

/// One open bracket, and where the work inside it starts.
struct Frame {
    kind: FrameKind,
    /// The first operand and the first operator that belong to this frame. Reductions stop
    /// here, so nothing inside a bracket can consume anything outside it.
    operands: usize,
    ops: usize,
    /// Whether a comparison has already been made at this level.
    ///
    /// Comparison does not chain: `a == b == c` was refused by the grammar this replaces -
    /// its comparison rule took at most one operator - and is refused here by declining to
    /// take a second, which leaves the token where it is for [`Parser::expect_end`] to
    /// report as trailing.
    compared: bool,
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    source: String,
    /// Values read but not yet built into anything, innermost last.
    operands: Vec<Operand>,
    /// Operators waiting for their operands, innermost last.
    ops: Vec<Pending>,
    /// Brackets currently open, outermost first.
    frames: Vec<Frame>,
}

impl Parser {
    fn new(text: &str, original: &str) -> Result<Self, GuardParseError> {
        let tokens = tokenize(text, original)?;
        Ok(Self {
            tokens,
            pos: 0,
            source: original.to_string(),
            operands: Vec::new(),
            ops: Vec::new(),
            frames: vec![Frame {
                kind: FrameKind::Whole,
                operands: 0,
                ops: 0,
                compared: false,
            }],
        })
    }

    /// Reads the whole expression, alternating between the two positions a parser can be in.
    ///
    /// Where the recursive version had five functions calling each other - expression,
    /// conjunction, comparison, unary, primary - there is one loop and three `Vec`s. A value
    /// is wanted, or an operator is wanted; each says whether the other is wanted next.
    fn parse(&mut self) -> Result<Guard, GuardParseError> {
        loop {
            if self.read_value()? {
                continue;
            }
            if self.read_operator()? {
                continue;
            }
            break;
        }
        Ok(self.pop_operand()?.node)
    }

    /// The prefix position: any `not`s, then a value or an opening bracket.
    ///
    /// Returns true when a bracket was opened, because then a value is wanted again rather
    /// than an operator.
    fn read_value(&mut self) -> Result<bool, GuardParseError> {
        // An argument list that ends where an argument would start: `f()`, and `f(a,)`,
        // which the grammar this replaces also accepted.
        if self.peek() == TokenKind::CloseParen && self.inside_a_call() {
            self.take();
            self.close_call(None)?;
            return Ok(false);
        }

        while self.peek() == TokenKind::Not {
            self.take();
            // A prefix operator has nothing to its left, so it is pushed rather than
            // reduced against what is already there - which is also what makes `not not x`
            // right-associative without saying so.
            self.ops.push(Pending::Not);
        }

        match self.peek() {
            TokenKind::OpenParen => {
                self.take();
                self.open(FrameKind::Group);
                return Ok(true);
            }
            TokenKind::Name if self.peek_at(1) == TokenKind::OpenParen => {
                let name = self.take().value;
                self.take();
                self.open(FrameKind::Call {
                    name,
                    args: Vec::new(),
                    deepest: 0,
                });
                return Ok(true);
            }
            _ => {}
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
        if self.peek() == TokenKind::Operator
            && self.peek_value() == "-"
            && self.peek_at(1) == TokenKind::Number
        {
            self.take();
            let raw = self.take().value;
            let number = raw.parse::<f64>().map_err(|_| {
                GuardParseError::new(format!("bad number '-{raw}'"), self.source.clone())
            })?;
            self.push_leaf(Guard::literal(GuardValue::from_number(-number)))?;
            return Ok(false);
        }

        let leaf = match self.peek() {
            TokenKind::Variable => Guard::variable(self.take().value),
            TokenKind::True => {
                self.take();
                Guard::literal(GuardValue::from_boolean(true))
            }
            TokenKind::False => {
                self.take();
                Guard::literal(GuardValue::from_boolean(false))
            }
            TokenKind::Nil => {
                self.take();
                Guard::literal(GuardValue::unknown())
            }
            TokenKind::Number => {
                let raw = self.take().value;
                let number = raw.parse::<f64>().map_err(|_| {
                    GuardParseError::new(format!("bad number '{raw}'"), self.source.clone())
                })?;
                Guard::literal(GuardValue::from_number(number))
            }
            TokenKind::Text => Guard::literal(GuardValue::from_text(self.take().value)),
            // A name with no argument list. Still a call, as it always was: the world is
            // what decides whether it answers.
            TokenKind::Name => Guard::call(self.take().value, Vec::new()),
            _ => {
                return Err(GuardParseError::new(
                    "unexpected token".into(),
                    self.source.clone(),
                ));
            }
        };
        self.push_leaf(leaf)?;
        Ok(false)
    }

    /// The infix position: an operator, or whatever closes the brackets around it.
    ///
    /// Returns true when a value is wanted next - an operator or a comma was taken - and
    /// false when the guard is finished.
    fn read_operator(&mut self) -> Result<bool, GuardParseError> {
        loop {
            match self.peek() {
                TokenKind::Or => {
                    self.take();
                    self.push_op(Pending::Or)?;
                    return Ok(true);
                }
                TokenKind::And => {
                    self.take();
                    self.push_op(Pending::And)?;
                    return Ok(true);
                }
                TokenKind::Operator if !self.compared() => {
                    let op = self.take().value;
                    self.push_op(Pending::Compare(op))?;
                    return Ok(true);
                }
                _ => {}
            }

            // Nothing this frame can take, so it is finished: fold everything in it into
            // one value and see what closes it.
            self.reduce_frame()?;
            let closing = self.peek();
            if closing == TokenKind::Comma && self.inside_a_call() {
                self.take();
                let argument = self.pop_operand()?;
                self.add_argument(argument);
                return Ok(true);
            }
            if closing == TokenKind::CloseParen && self.inside_a_call() {
                self.take();
                let argument = self.pop_operand()?;
                self.close_call(Some(argument))?;
                continue;
            }
            if closing == TokenKind::CloseParen && self.inside_a_group() {
                // A group's value is what is inside it, which is already the top operand.
                self.take();
                self.frames.pop();
                continue;
            }
            if self.frames.len() == 1 {
                return Ok(false);
            }
            // An argument list says so, because it is the more useful half of "expected a
            // comma or a close" and because it is the message this parser has always given.
            let message = if self.inside_a_call() {
                "bad argument list"
            } else {
                "expected CloseParen"
            };
            return Err(GuardParseError::new(message.into(), self.source.clone()));
        }
    }

    fn open(&mut self, kind: FrameKind) {
        self.frames.push(Frame {
            kind,
            operands: self.operands.len(),
            ops: self.ops.len(),
            compared: false,
        });
    }

    /// Pops the innermost frame, which must be a call, and pushes what it built.
    fn close_call(&mut self, last: Option<Operand>) -> Result<(), GuardParseError> {
        if let Some(argument) = last {
            self.add_argument(argument);
        }
        match self.frames.pop().map(|frame| frame.kind) {
            Some(FrameKind::Call {
                name,
                args,
                deepest,
            }) => self.push_operand(Guard::call(name, args), deepest + 1),
            _ => Err(self.confused()),
        }
    }

    fn add_argument(&mut self, argument: Operand) {
        if let Some(frame) = self.frames.last_mut() {
            frame.compared = false;
            if let FrameKind::Call { args, deepest, .. } = &mut frame.kind {
                *deepest = (*deepest).max(argument.depth);
                args.push(argument.node);
            }
        }
    }

    /// Takes an infix operator, building everything that binds at least as tightly first.
    ///
    /// This is where precedence happens: `a or b and c` leaves the `or` waiting because
    /// `and` holds tighter, while `a and b or c` builds the `and` before the `or` goes on.
    fn push_op(&mut self, op: Pending) -> Result<(), GuardParseError> {
        let power = op.binding_power();
        while self.ops.len() > self.floor().1
            && self
                .ops
                .last()
                .is_some_and(|top| top.binding_power() >= power)
        {
            self.reduce()?;
        }

        if let Some(frame) = self.frames.last_mut() {
            // A comparison is what blocks a second one; `and` and `or` open a fresh level
            // where a comparison is allowed again.
            frame.compared = match op {
                Pending::Compare(_) => true,
                Pending::And | Pending::Or => false,
                Pending::Not => frame.compared,
            };
        }
        self.ops.push(op);
        Ok(())
    }

    /// Builds everything the innermost frame is holding, leaving it one value.
    fn reduce_frame(&mut self) -> Result<(), GuardParseError> {
        while self.ops.len() > self.floor().1 {
            self.reduce()?;
        }
        Ok(())
    }

    /// Builds one operator from the operands waiting under it.
    fn reduce(&mut self) -> Result<(), GuardParseError> {
        let op = self.ops.pop().ok_or_else(|| self.confused())?;
        let right = self.pop_operand()?;
        if let Pending::Not = op {
            let depth = right.depth;
            return self.push_operand(Guard::not(right.node), depth + 1);
        }

        let left = self.pop_operand()?;
        let depth = left.depth.max(right.depth);
        let (left, right) = (left.node, right.node);
        let node = match op {
            Pending::And => Guard::and(left, right),
            Pending::Or => Guard::or(left, right),
            Pending::Compare(name) => Guard::comparison(name, left, right),
            Pending::Not => return Err(self.confused()),
        };
        self.push_operand(node, depth + 1)
    }

    fn push_leaf(&mut self, node: Guard) -> Result<(), GuardParseError> {
        self.push_operand(node, 1)
    }

    fn push_operand(&mut self, node: Guard, depth: usize) -> Result<(), GuardParseError> {
        if depth > MAX_DEPTH {
            return Err(GuardParseError::new(
                format!("nested more than {MAX_DEPTH} deep"),
                self.source.clone(),
            ));
        }
        self.operands.push(Operand { node, depth });
        Ok(())
    }

    fn pop_operand(&mut self) -> Result<Operand, GuardParseError> {
        self.operands.pop().ok_or_else(|| self.confused())
    }

    /// Where the innermost frame's operands and operators begin.
    fn floor(&self) -> (usize, usize) {
        self.frames
            .last()
            .map_or((0, 0), |frame| (frame.operands, frame.ops))
    }

    fn compared(&self) -> bool {
        self.frames.last().is_some_and(|frame| frame.compared)
    }

    fn inside_a_call(&self) -> bool {
        matches!(
            self.frames.last().map(|frame| &frame.kind),
            Some(FrameKind::Call { .. })
        )
    }

    fn inside_a_group(&self) -> bool {
        matches!(
            self.frames.last().map(|frame| &frame.kind),
            Some(FrameKind::Group)
        )
    }

    /// The error for a state the grammar cannot reach.
    ///
    /// Only a bug in this file gets here, and it is an error rather than a panic for the
    /// same reason everything else in this file is: a guard is game content, and no string
    /// may bring the process down.
    fn confused(&self) -> GuardParseError {
        GuardParseError::new("the parser lost its place".into(), self.source.clone())
    }

    /// The kind `offset` tokens ahead, for the two decisions that need to look past the
    /// next token: whether a minus begins a negative number, and whether a name is a call.
    fn peek_at(&self, offset: usize) -> TokenKind {
        self.tokens
            .get(self.pos + offset)
            .map(|t| t.kind.clone())
            .unwrap_or(TokenKind::End)
    }

    /// The next token's text, or empty at the end of the input.
    fn peek_value(&self) -> &str {
        self.tokens
            .get(self.pos)
            .map(|t| t.value.as_str())
            .unwrap_or("")
    }

    fn peek(&self) -> TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| t.kind.clone())
            .unwrap_or(TokenKind::End)
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
            None => Token {
                kind: TokenKind::End,
                value: String::new(),
            },
        }
    }

    fn expect_end(&mut self) -> Result<(), GuardParseError> {
        if self.pos != self.tokens.len() {
            Err(GuardParseError::new(
                "trailing tokens".into(),
                self.source.clone(),
            ))
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
        if c.is_whitespace() {
            continue;
        }

        match c {
            '(' => tokens.push(Token {
                kind: TokenKind::OpenParen,
                value: "(".into(),
            }),
            ')' => tokens.push(Token {
                kind: TokenKind::CloseParen,
                value: ")".into(),
            }),
            ',' => tokens.push(Token {
                kind: TokenKind::Comma,
                value: ",".into(),
            }),
            '"' => {
                let mut s = String::new();
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                    s.push(c);
                }
                tokens.push(Token {
                    kind: TokenKind::Text,
                    value: s,
                });
            }
            c if c.is_ascii_digit() => {
                let mut s = String::new();
                s.push(c);
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' {
                        s.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::Number,
                    value: s,
                });
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut s = String::new();
                s.push(c);
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                        s.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                if s == "Variable" {
                    // Try to read Variable["name"]
                    let _save_pos = chars.clone();
                    let mut temp = chars.clone();
                    let mut ws = String::new();
                    while let Some(&c) = temp.peek() {
                        if c.is_whitespace() {
                            ws.push(temp.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    if temp.next() == Some('[') {
                        let mut ws2 = String::new();
                        while let Some(&c) = temp.peek() {
                            if c.is_whitespace() {
                                ws2.push(temp.next().unwrap());
                            } else {
                                break;
                            }
                        }
                        if temp.next() == Some('"') {
                            let mut name = String::new();
                            for c in temp.by_ref() {
                                if c == '"' {
                                    break;
                                }
                                name.push(c);
                            }
                            let mut ws3 = String::new();
                            while let Some(&c) = temp.peek() {
                                if c.is_whitespace() {
                                    ws3.push(temp.next().unwrap());
                                } else {
                                    break;
                                }
                            }
                            if temp.next() == Some(']') {
                                chars = temp;
                                tokens.push(Token {
                                    kind: TokenKind::Variable,
                                    value: name,
                                });
                                continue;
                            }
                        }
                    }
                    // Not a Variable[...], treat as name
                    tokens.push(Token {
                        kind: TokenKind::Name,
                        value: s,
                    });
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
                tokens.push(Token {
                    kind: TokenKind::Operator,
                    value: s,
                });
            }
            _ => {
                return Err(GuardParseError::new(
                    format!("unexpected character {c:?}"),
                    text.to_string(),
                ));
            }
        }
    }
    Ok(tokens)
}
