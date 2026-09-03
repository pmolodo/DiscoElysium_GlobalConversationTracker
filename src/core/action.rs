// SPDX-License-Identifier: MIT
use std::fmt;
use serde::{Deserialize, Serialize};
use crate::core::state::LookAheadState;

/// What kind of change an action makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum DialogueActionKind {
    Unmodelled = 0,
    Assign = 1,
    Increment = 2,
    GainMoney = 3,
    LoseMoney = 4,
    PassTime = 5,
}

/// One state change a dialogue entry's userScript makes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueAction {
    kind: DialogueActionKind,
    slot: i32,           // -1 for money/unmodelled
    value: i32,
    once: bool,
    name: String,
}

/// How high a counter may climb before it stops moving.
///
/// The cap is what keeps a counter inside a dialogue loop finite: without it a loop that
/// increments something has no repeated state and the search never terminates.
///
/// One knob with an override, rather than the C#'s two. There, `CounterCapForSlot`
/// SUPPLANTS `CounterCap` instead of falling back to it - it is consulted for every slot
/// once supplied - so a caller wanting to special-case one variable has to answer for all
/// of them and re-state the default itself. The offline crawler duplicates the literal
/// 16 to do it. Here the per-slot function answers `None` for anything it has no opinion
/// about and the default applies, so there is one place the default lives.
pub struct CounterCaps<'a> {
    default: i32,
    per_slot: Option<&'a (dyn Fn(usize) -> Option<i32> + Send + Sync)>,
}

impl<'a> CounterCaps<'a> {
    /// The same cap for every slot.
    pub fn flat(default: i32) -> Self {
        Self { default, per_slot: None }
    }

    /// A cap that may be overridden per slot; `None` from `per_slot` means the default.
    pub fn with_overrides(
        default: i32,
        per_slot: &'a (dyn Fn(usize) -> Option<i32> + Send + Sync),
    ) -> Self {
        Self { default, per_slot: Some(per_slot) }
    }

    /// The cap that applies to one slot.
    pub fn for_slot(&self, slot: usize) -> i32 {
        self.per_slot.and_then(|f| f(slot)).unwrap_or(self.default)
    }
}

impl DialogueAction {
    /// Whether this action fires only the first time its entry is reached.
    ///
    /// Read when the graph decides which entries need a once slot; the fields stay
    /// private so an action is still built only through the constructors below.
    pub fn is_once(&self) -> bool {
        self.once
    }

    pub fn assign(slot: usize, value: i32, name: String) -> Self {
        Self { kind: DialogueActionKind::Assign, slot: slot as i32, value, once: false, name }
    }

    pub fn increment(slot: usize, amount: i32, once: bool, name: String) -> Self {
        Self { kind: DialogueActionKind::Increment, slot: slot as i32, value: amount, once, name }
    }

    pub fn money(gain: bool, amount: i32, once: bool, name: String) -> Self {
        Self {
            kind: if gain { DialogueActionKind::GainMoney } else { DialogueActionKind::LoseMoney },
            slot: -1,
            value: amount,
            once,
            name,
        }
    }

    pub fn pass_time(name: String) -> Self {
        Self { kind: DialogueActionKind::PassTime, slot: -1, value: LookAheadState::PASS_TIME_MINUTES, once: false, name }
    }

    pub fn unmodelled(name: String) -> Self {
        Self { kind: DialogueActionKind::Unmodelled, slot: -1, value: 0, once: false, name }
    }

    pub fn kind(&self) -> DialogueActionKind { self.kind }
    pub fn slot(&self) -> i32 { self.slot }
    pub fn value(&self) -> i32 { self.value }
    pub fn once(&self) -> bool { self.once }
    pub fn name(&self) -> &str { &self.name }

    /// Apply a node's actions to a state.
    pub fn apply(
        actions: &[DialogueAction],
        state: &LookAheadState,
        once_slot: i32,
        counter_cap: &CounterCaps<'_>,
        clock_locked: bool,
    ) -> LookAheadState {
        if actions.is_empty() {
            return state.clone();
        }

        // -1 means the node has no once slot, which the graph assigns only where
        // something actually fires once. Nothing has fired if there is nowhere to
        // record that it did.
        let already_fired = once_slot >= 0 && state.is_set(once_slot as usize);
        let mut fired_something_once = false;
        let mut changes = Vec::with_capacity(actions.len() + 1);
        let mut money = state.money();
        let mut day_minutes = state.day_minutes();

        for action in actions {
            if action.once {
                if already_fired { continue; }
                fired_something_once = true;
            }

            match action.kind {
                DialogueActionKind::Assign => {
                    changes.push((action.slot as usize, action.value));
                }
                DialogueActionKind::Increment => {
                    let idx = action.slot as usize;
                    let current = changes.iter().rev()
                        .find(|(i, _)| *i == idx)
                        .map(|(_, v)| *v)
                        .unwrap_or_else(|| state.get(idx));
                    let raised = current + action.value;
                    // Floored as well as capped. The cap is what makes a counter in a
                    // loop finite; the floor is what keeps a slot inside what a state can
                    // represent, now that an increment can be negative -
                    // `ReputationLowers` subtracts one. A slot's decision-diagram
                    // encoding is an unsigned run of bits, so a negative value has
                    // nowhere to go and the symbolic image floors it at zero; the two
                    // have to agree or the oracle comparison measures the disagreement
                    // rather than the diagrams.
                    changes.push((idx, raised.clamp(0, counter_cap.for_slot(idx))));
                }
                DialogueActionKind::GainMoney => money += action.value,
                DialogueActionKind::LoseMoney => money -= action.value,
                DialogueActionKind::PassTime => {
                    if !clock_locked {
                        day_minutes = LookAheadState::wrap_minutes(day_minutes + action.value);
                    }
                }
                DialogueActionKind::Unmodelled => {}
            }
        }

        if fired_something_once && once_slot >= 0 {
            changes.push((once_slot as usize, 1));
        }

        if changes.is_empty() && money == state.money() && day_minutes == state.day_minutes() {
            state.clone()
        } else {
            state.with_changes(&changes, money, day_minutes)
        }
    }
}

impl fmt::Display for DialogueAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            DialogueActionKind::Assign => write!(f, "{}: slot {} = {}", self.name, self.slot, self.value),
            DialogueActionKind::Increment => write!(f, "{}: slot {} += {}{}", self.name, self.slot, self.value, if self.once { " (once)" } else { "" }),
            DialogueActionKind::GainMoney => write!(f, "{}: money += {}{}", self.name, self.value, if self.once { " (once)" } else { "" }),
            DialogueActionKind::LoseMoney => write!(f, "{}: money -= {}{}", self.name, self.value, if self.once { " (once)" } else { "" }),
            DialogueActionKind::PassTime => write!(f, "{}: clock += {}m", self.name, self.value),
            DialogueActionKind::Unmodelled => write!(f, "{}: not modelled", self.name),
        }
    }
}
