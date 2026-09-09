// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::core::types::Ternary;

/// What sort of value a guard sub-expression evaluated to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum GuardValueKind {
    Unknown = 0,
    Boolean = 1,
    Number = 2,
    Text = 3,
}

/// The result of evaluating part of a guard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardValue {
    kind: GuardValueKind,
    boolean: bool,
    number: f64,
    text: String,
}

impl GuardValue {
    pub const fn unknown() -> Self {
        Self {
            kind: GuardValueKind::Unknown,
            boolean: false,
            number: 0.0,
            text: String::new(),
        }
    }

    pub const fn from_boolean(value: bool) -> Self {
        Self {
            kind: GuardValueKind::Boolean,
            boolean: value,
            number: 0.0,
            text: String::new(),
        }
    }

    pub fn from_number(value: f64) -> Self {
        Self {
            kind: GuardValueKind::Number,
            boolean: false,
            number: value,
            text: String::new(),
        }
    }

    pub fn from_text(value: String) -> Self {
        Self {
            kind: GuardValueKind::Text,
            boolean: false,
            number: 0.0,
            text: value,
        }
    }

    pub fn kind(&self) -> GuardValueKind {
        self.kind
    }

    pub fn boolean(&self) -> bool {
        self.boolean
    }

    pub fn number(&self) -> f64 {
        self.number
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Read as a condition, Lua style: anything but false is true.
    pub fn as_condition(&self) -> Ternary {
        match self.kind {
            GuardValueKind::Boolean => {
                if self.boolean {
                    Ternary::True
                } else {
                    Ternary::False
                }
            }
            GuardValueKind::Number | GuardValueKind::Text => Ternary::True,
            GuardValueKind::Unknown => Ternary::Unknown,
        }
    }

    /// Try to get as number for ordering comparisons.
    pub fn try_as_number(&self) -> Option<f64> {
        if self.kind == GuardValueKind::Number {
            Some(self.number)
        } else {
            None
        }
    }

    pub fn equals(&self, other: &Self) -> bool {
        if self.kind != other.kind {
            return false;
        }
        match self.kind {
            GuardValueKind::Boolean => self.boolean == other.boolean,
            GuardValueKind::Number => self.number == other.number,
            GuardValueKind::Text => self.text == other.text,
            GuardValueKind::Unknown => true,
        }
    }
}

impl fmt::Display for GuardValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            GuardValueKind::Boolean => write!(f, "{}", if self.boolean { "true" } else { "false" }),
            GuardValueKind::Number => write!(f, "{}", self.number),
            GuardValueKind::Text => write!(f, "\"{}\"", self.text),
            GuardValueKind::Unknown => write!(f, "unknown"),
        }
    }
}

impl PartialEq for GuardValue {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}
