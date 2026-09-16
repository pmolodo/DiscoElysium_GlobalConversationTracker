// SPDX-License-Identifier: MIT

pub mod action;
pub mod clock;
pub mod env;
pub mod equipment;
pub mod guard;
pub mod guard_value;
pub mod inventory_tabs;
pub mod item_group;
pub mod modelling;
pub mod passive_check;
pub mod reputation;
pub mod state;
pub mod substance;
pub mod types;

/// What the MACHINE has left, which is a different question from what a budget allows.
///
/// Here rather than beside the search that reads it, because every search wants it and
/// none of them owns it: a budget says what a caller allows, and this says whether the
/// machine can supply it.
pub mod system_memory;

/// The ported C# clock suite. In a file of its own rather than inline only because of its
/// size; small unit tests stay beside what they test.
#[cfg(test)]
mod clock_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::LookAheadState;

    #[test]
    fn test_dialogue_node_id() {
        let id = types::DialogueNodeId::new(123, 456);
        assert_eq!(id.conversation_id, 123);
        assert_eq!(id.entry_id, 456);
        assert_eq!(format!("{}", id), "123:456");
    }

    #[test]
    fn test_novelty_ordering() {
        use types::Novelty;
        assert!(Novelty::UnseenAnyGame > Novelty::UnseenThisGame);
        assert!(Novelty::UnseenThisGame > Novelty::SeenThisGame);
    }

    #[test]
    fn test_ternary() {
        use types::{Ternary, ternary_and, ternary_not, ternary_or};
        assert!(Ternary::True.can_pass());
        assert!(!Ternary::False.can_pass());
        assert!(Ternary::Unknown.can_pass());
        assert_eq!(ternary_not(Ternary::True), Ternary::False);
        assert_eq!(ternary_and(Ternary::True, Ternary::False), Ternary::False);
        assert_eq!(
            ternary_or(Ternary::False, Ternary::Unknown),
            Ternary::Unknown
        );
    }

    #[test]
    fn test_guard_value() {
        use guard_value::{GuardValue, GuardValueKind};
        let b = GuardValue::from_boolean(true);
        assert_eq!(b.kind(), GuardValueKind::Boolean);
        assert_eq!(b.as_condition(), types::Ternary::True);

        let n = GuardValue::from_number(42.0);
        assert_eq!(n.try_as_number(), Some(42.0));
    }

    #[test]
    fn test_state_symbols() {
        use crate::core::types::DialogueNodeId;
        let mut symbols = state::StateSymbols::new();
        let v = symbols.variable("test_var");
        assert_eq!(v, 0);
        assert_eq!(symbols.variable("test_var"), 0); // same index

        let item = symbols.item("sword");
        assert_eq!(item, 1);

        let task = symbols.task("find_kim");
        assert_eq!(task, 2);

        let node = DialogueNodeId::new(1, 2);
        let once = symbols.once(node);
        assert_eq!(once, 3);

        let seen = symbols.seen(node);
        assert_eq!(seen, 4);

        assert_eq!(symbols.count(), 5);
    }

    #[test]
    fn test_look_ahead_state() {
        let state = LookAheadState::empty(10, 5000, 720);
        assert_eq!(state.money(), 5000);
        assert_eq!(state.day_minutes(), 720);
        assert_eq!(state.slot_count(), 10);

        let s2 = state.with(5, 42);
        assert_eq!(s2.get(5), 42);
        assert_eq!(s2.money(), 5000);

        let s3 = s2.with_money(3000);
        assert_eq!(s3.money(), 3000);
        assert_eq!(s3.get(5), 42);

        let s4 = s3.with_day_minutes(780);
        assert_eq!(s4.day_minutes(), 780);
    }

    #[test]
    fn test_clock_time() {
        use clock::ClockTime;
        assert_eq!(ClockTime::hours_of(720), 12); // noon
        assert_eq!(ClockTime::hours_of(0), 0); // midnight
        assert_eq!(ClockTime::advance(720), 735); // +15 min
        assert_eq!(ClockTime::advance(1439), 14); // wrap at midnight
    }
}
