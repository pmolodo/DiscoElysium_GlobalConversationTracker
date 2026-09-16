// SPDX-License-Identifier: MIT
use crate::core::clock::ClockReading;
use crate::core::state::LookAheadState;
use serde::{Deserialize, Serialize};
use std::fmt;

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
    /// Recognised, and deliberately doing nothing - see [`crate::core::modelling`].
    ///
    /// Applies exactly as `Unmodelled` does, which is the point: the difference between
    /// the two is not what the search does with them but whether anybody has decided.
    /// One is a stub somebody argued for; the other is a gap nobody has looked at.
    Declared = 6,
    /// `slot := reading + value`, a number read off the clock when the action runs - see
    /// [`ClockReading`].
    AssignClock = 7,
    /// `slot := value` unless the slot [`DialogueAction::unless`] names is set, and nothing
    /// otherwise - how the journal refuses to reveal a cancelled task or cancel a done one.
    AssignUnless = 8,
}

/// One state change a dialogue entry's userScript makes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueAction {
    kind: DialogueActionKind,
    slot: i32, // -1 for money/unmodelled
    value: i32,
    once: bool,
    name: String,
    /// What an [`DialogueActionKind::AssignClock`] reads, and `None` for every other kind.
    #[serde(default)]
    reading: Option<ClockReading>,
    /// The slot whose being set stops an [`DialogueActionKind::AssignUnless`], and `None` for
    /// every other kind.
    #[serde(default)]
    unless: Option<i32>,
}

/// How high a counter may climb before it stops moving.
///
/// The cap is what keeps a counter inside a dialogue loop finite: without it a loop that
/// increments something has no repeated state and the search never terminates.
///
/// One knob with an override, rather than the C#'s two. There, `CounterCapForSlot`
/// SUPPLANTS `CounterCap` instead of falling back to it - it is consulted for every slot
/// once supplied - so a caller wanting to special-case one variable has to answer for all
/// of them and re-state the default itself. The offline search duplicates the literal
/// 16 to do it. Here the per-slot function answers `None` for anything it has no opinion
/// about and the default applies, so there is one place the default lives.
pub struct CounterCaps<'a> {
    default: i32,
    per_slot: Option<&'a (dyn Fn(usize) -> Option<i32> + Send + Sync)>,
}

impl<'a> CounterCaps<'a> {
    /// The same cap for every slot.
    pub fn flat(default: i32) -> Self {
        Self {
            default,
            per_slot: None,
        }
    }

    /// A cap that may be overridden per slot; `None` from `per_slot` means the default.
    pub fn with_overrides(
        default: i32,
        per_slot: &'a (dyn Fn(usize) -> Option<i32> + Send + Sync),
    ) -> Self {
        Self {
            default,
            per_slot: Some(per_slot),
        }
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
        Self {
            kind: DialogueActionKind::Assign,
            slot: slot as i32,
            value,
            once: false,
            name,
            reading: None,
            unless: None,
        }
    }

    pub fn increment(slot: usize, amount: i32, once: bool, name: String) -> Self {
        Self {
            kind: DialogueActionKind::Increment,
            slot: slot as i32,
            value: amount,
            once,
            name,
            reading: None,
            unless: None,
        }
    }

    pub fn money(gain: bool, amount: i32, once: bool, name: String) -> Self {
        Self {
            kind: if gain {
                DialogueActionKind::GainMoney
            } else {
                DialogueActionKind::LoseMoney
            },
            slot: -1,
            value: amount,
            once,
            name,
            reading: None,
            unless: None,
        }
    }

    pub fn pass_time(name: String) -> Self {
        Self {
            kind: DialogueActionKind::PassTime,
            slot: -1,
            value: LookAheadState::PASS_TIME_MINUTES,
            once: false,
            name,
            reading: None,
            unless: None,
        }
    }

    /// `slot := reading + offset`, with the reading taken when the action runs.
    pub fn assign_clock(slot: usize, reading: ClockReading, offset: i32, name: String) -> Self {
        Self {
            kind: DialogueActionKind::AssignClock,
            slot: slot as i32,
            value: offset,
            once: false,
            name,
            reading: Some(reading),
            unless: None,
        }
    }

    /// `slot := value`, unless `unless` is set when the action runs.
    pub fn assign_unless(slot: usize, value: i32, unless: usize, name: String) -> Self {
        Self {
            kind: DialogueActionKind::AssignUnless,
            slot: slot as i32,
            value,
            once: false,
            name,
            reading: None,
            unless: Some(unless as i32),
        }
    }

    pub fn unmodelled(name: String) -> Self {
        Self {
            kind: DialogueActionKind::Unmodelled,
            slot: -1,
            value: 0,
            once: false,
            name,
            reading: None,
            unless: None,
        }
    }

    /// An action the model recognises and deliberately does not apply.
    ///
    /// The decision itself is not stored: it is looked up from the name, so there is one
    /// copy of it and a report cannot quote a reason that has since been revised.
    pub fn declared(name: String) -> Self {
        debug_assert!(
            crate::core::modelling::for_action(&name).is_some(),
            "no decision covers {name}",
        );
        Self {
            kind: DialogueActionKind::Declared,
            slot: -1,
            value: 0,
            once: false,
            name,
            reading: None,
            unless: None,
        }
    }

    /// The decision that made this a stub, for an action that is one.
    pub fn decision(&self) -> Option<&'static crate::core::modelling::Decision> {
        match self.kind {
            DialogueActionKind::Declared => crate::core::modelling::for_action(&self.name),
            _ => None,
        }
    }

    /// Whether this action's `slot` names a slot at all.
    ///
    /// Only the four that write one do. For every other kind `slot` is `-1`, which is a
    /// marker rather than an index - see the field.
    pub fn writes_slot(&self) -> bool {
        matches!(
            self.kind,
            DialogueActionKind::Assign
                | DialogueActionKind::Increment
                | DialogueActionKind::AssignClock
                | DialogueActionKind::AssignUnless
        )
    }

    /// The slot an [`DialogueActionKind::AssignUnless`] tests, which it READS rather than writes.
    pub fn unless(&self) -> Option<usize> {
        self.unless.and_then(|slot| usize::try_from(slot).ok())
    }

    /// What an [`DialogueActionKind::AssignClock`] reads off the clock.
    pub fn reading(&self) -> Option<ClockReading> {
        self.reading
    }

    /// The value an [`DialogueActionKind::AssignClock`] writes at this time on this day.
    pub fn clock_value(&self, day_minutes: i32, day_counter: i32) -> Option<i32> {
        self.reading
            .map(|reading| reading.value(day_minutes, day_counter) + self.value)
    }

    /// The same action against a renumbered symbol table, or `None` if its slot has gone.
    ///
    /// `map` is [`crate::core::state::StateSymbols::retaining`]'s old-to-new index map.
    ///
    /// NONE MEANS REMOVE THE ACTION, and the caller must. Writing `-1` into the slot
    /// instead would not disable the write: `-1` is what money and unmodelled actions
    /// carry, so an `Assign` holding it is not "no slot" but a slot index of minus one,
    /// and `apply` would cast it straight to a `usize`. Removal is also what is actually
    /// meant - a dropped slot is one nothing reads, so the write cannot change an answer.
    pub fn renumbered(mut self, map: &[i32]) -> Option<Self> {
        if !self.writes_slot() {
            return Some(self);
        }

        let moved = usize::try_from(self.slot)
            .ok()
            .and_then(|slot| map.get(slot).copied())
            .unwrap_or(-1);
        if moved < 0 {
            return None;
        }

        self.slot = moved;

        // THE SLOT A CONDITIONAL WRITE TESTS IS READ, and the retention that builds `map` keeps
        // every slot a condition reads - so one that has gone is a caller that renumbered
        // against some other rule, and the condition would silently stop holding.
        if let Some(unless) = self.unless {
            let kept = usize::try_from(unless)
                .ok()
                .and_then(|slot| map.get(slot).copied())
                .filter(|slot| *slot >= 0)
                .expect("the slot a conditional write tests was dropped");
            self.unless = Some(kept);
        }
        Some(self)
    }

    pub fn kind(&self) -> DialogueActionKind {
        self.kind
    }
    pub fn slot(&self) -> i32 {
        self.slot
    }
    pub fn value(&self) -> i32 {
        self.value
    }
    pub fn once(&self) -> bool {
        self.once
    }
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Apply a node's actions to a state.
    ///
    /// `day_counter` is the story's day, which no action in a conversation moves; it is what a
    /// clock reading needs beside the state's own time of day.
    pub fn apply(
        actions: &[DialogueAction],
        state: &LookAheadState,
        once_slot: i32,
        counter_cap: &CounterCaps<'_>,
        clock_locked: bool,
        day_counter: i32,
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
                if already_fired {
                    continue;
                }
                fired_something_once = true;
            }

            match action.kind {
                DialogueActionKind::Assign => {
                    changes.push((action.slot as usize, action.value));
                }
                // THE CONDITION AS IT STANDS WHEN THE ACTION RUNS, an earlier write in the same
                // script included: `FinishTask` reveals unless done and then marks done.
                DialogueActionKind::AssignUnless => {
                    let unless = action
                        .unless()
                        .expect("a conditional write carries the slot it tests");
                    let held = changes
                        .iter()
                        .rev()
                        .find(|(i, _)| *i == unless)
                        .map(|(_, v)| *v)
                        .unwrap_or_else(|| state.get(unless));
                    if held == 0 {
                        changes.push((action.slot as usize, action.value));
                    }
                }
                // READ AT THE TIME IT RUNS, a PassTime earlier in the same script included -
                // the game evaluates the call as it reaches the statement.
                DialogueActionKind::AssignClock => {
                    let value = action
                        .clock_value(day_minutes, day_counter)
                        .expect("a clock assignment carries its reading");
                    changes.push((action.slot as usize, value.max(0)));
                }
                DialogueActionKind::Increment => {
                    let idx = action.slot as usize;
                    let current = changes
                        .iter()
                        .rev()
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
                // Both write nothing, and for the same reason from the search's point of
                // view: there is no slot to put anything in. What separates them is
                // whether that was decided or merely not yet looked at.
                DialogueActionKind::Unmodelled | DialogueActionKind::Declared => {}
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
            DialogueActionKind::Assign => {
                write!(f, "{}: slot {} = {}", self.name, self.slot, self.value)
            }
            DialogueActionKind::Increment => write!(
                f,
                "{}: slot {} += {}{}",
                self.name,
                self.slot,
                self.value,
                if self.once { " (once)" } else { "" }
            ),
            DialogueActionKind::GainMoney => write!(
                f,
                "{}: money += {}{}",
                self.name,
                self.value,
                if self.once { " (once)" } else { "" }
            ),
            DialogueActionKind::LoseMoney => write!(
                f,
                "{}: money -= {}{}",
                self.name,
                self.value,
                if self.once { " (once)" } else { "" }
            ),
            DialogueActionKind::AssignClock => write!(
                f,
                "{}: slot {} = {:?} + {}",
                self.name,
                self.slot,
                self.reading
                    .expect("a clock assignment carries its reading"),
                self.value
            ),
            DialogueActionKind::AssignUnless => write!(
                f,
                "{}: slot {} = {} unless slot {} is set",
                self.name,
                self.slot,
                self.value,
                self.unless.unwrap_or(-1)
            ),
            DialogueActionKind::PassTime => write!(f, "{}: clock += {}m", self.name, self.value),
            DialogueActionKind::Unmodelled => write!(f, "{}: not modelled", self.name),
            DialogueActionKind::Declared => match self.decision() {
                Some(decision) => write!(f, "{}: declared, {}", self.name, decision.verdict()),
                None => write!(f, "{}: declared", self.name),
            },
        }
    }
}
