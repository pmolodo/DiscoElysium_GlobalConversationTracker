// SPDX-License-Identifier: MIT
use std::fmt;
use serde::{Deserialize, Serialize};

use crate::core::guard_value::GuardValue;
use crate::core::types::{Ternary, ternary_not, ternary_and, ternary_or};

/// Context for evaluating guards - provides variable values and world queries.
pub trait IGuardContext: Send + Sync {
    fn get_variable(&self, name: &str) -> GuardValue;
    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue;
}

/// A parsed guard expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GuardExpression {
    Literal(GuardValue),
    Variable(String),
    Call(String, Vec<GuardExpression>),
    Not(Box<GuardExpression>),
    And(Box<GuardExpression>, Box<GuardExpression>),
    Or(Box<GuardExpression>, Box<GuardExpression>),
    Comparison(String, Box<GuardExpression>, Box<GuardExpression>),
}

impl GuardExpression {
    pub fn always_true() -> Self {
        Self::Literal(GuardValue::from_boolean(true))
    }

    pub fn evaluate(&self, context: &dyn IGuardContext) -> GuardValue {
        match self {
            Self::Literal(v) => v.clone(),
            Self::Variable(name) => context.get_variable(name),
            Self::Call(name, args) => {
                let values: Vec<GuardValue> = args.iter().map(|a| a.evaluate(context)).collect();
                context.query(name, &values)
            }
            Self::Not(inner) => {
                let t = inner.evaluate(context).as_condition();
                match ternary_not(t) {
                    Ternary::Unknown => GuardValue::unknown(),
                    Ternary::True => GuardValue::from_boolean(true),
                    Ternary::False => GuardValue::from_boolean(false),
                }
            }
            Self::And(left, right) => {
                let l = left.evaluate(context).as_condition();
                let r = right.evaluate(context).as_condition();
                match ternary_and(l, r) {
                    Ternary::Unknown => GuardValue::unknown(),
                    Ternary::True => GuardValue::from_boolean(true),
                    Ternary::False => GuardValue::from_boolean(false),
                }
            }
            Self::Or(left, right) => {
                let l = left.evaluate(context).as_condition();
                let r = right.evaluate(context).as_condition();
                match ternary_or(l, r) {
                    Ternary::Unknown => GuardValue::unknown(),
                    Ternary::True => GuardValue::from_boolean(true),
                    Ternary::False => GuardValue::from_boolean(false),
                }
            }
            Self::Comparison(op, left, right) => {
                let l = left.evaluate(context);
                let r = right.evaluate(context);
                if l.kind() == GuardValueKind::Unknown || r.kind() == GuardValueKind::Unknown {
                    return GuardValue::unknown();
                }
                match op.as_str() {
                    "==" => GuardValue::from_boolean(l.equals(&r)),
                    "~=" => GuardValue::from_boolean(!l.equals(&r)),
                    ">=" | "<=" | ">" | "<" => {
                        let Some(a) = l.try_as_number() else { return GuardValue::unknown(); };
                        let Some(b) = r.try_as_number() else { return GuardValue::unknown(); };
                        let result = match op.as_str() {
                            ">=" => a >= b,
                            "<=" => a <= b,
                            ">" => a > b,
                            "<" => a < b,
                            _ => return GuardValue::unknown(),
                        };
                        GuardValue::from_boolean(result)
                    }
                    _ => GuardValue::unknown(),
                }
            }
        }
    }

    pub fn test(&self, context: &dyn IGuardContext) -> Ternary {
        self.evaluate(context).as_condition()
    }
}

impl fmt::Display for GuardExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(v) => write!(f, "{v}"),
            Self::Variable(name) => write!(f, "Variable[\"{name}\"]"),
            Self::Call(name, args) => write!(f, "{name}({})", args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ")),
            Self::Not(inner) => write!(f, "not {inner}"),
            Self::And(l, r) => write!(f, "({l} and {r})"),
            Self::Or(l, r) => write!(f, "({l} or {r})"),
            Self::Comparison(op, l, r) => write!(f, "({l} {op} {r})"),
        }
    }
}

use crate::core::guard_value::GuardValueKind;
