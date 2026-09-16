// SPDX-License-Identifier: MIT
//! How often the player has used a substance, which is what `SubstanceUsedOnce` and
//! `SubstanceUsedMore` ask.
//!
//! ## The game's definition
//!
//! From the pre-final-cut export (Assets/Scripts/Assembly-CSharp):
//! `Sunshine.Dialogue.InventoryLuaFunctions` and `HudHeldPanelController`. Final Cut's bodies
//! are stripped and taken to be unchanged.
//!
//! ```text
//! public static bool SubstanceUsedOnce(string substance)
//! {
//!     if (HudHeldPanelController.SubstanceUsedAmount(substance) <= 0)
//!     {
//!         return false;
//!     }
//!     return true;
//! }
//!
//! public static bool SubstanceUsedMore(string substance)
//! {
//!     if (HudHeldPanelController.SubstanceUsedAmount(substance) <= 3)
//!     {
//!         return false;
//!     }
//!     return true;
//! }
//!
//! public static int SubstanceUsedAmount(string substance)
//! {
//!     return Lua.Run("return Variable[\"stats.uses_" + substance + "\"]").AsInt;
//! }
//!
//! private static void SubstanceChargeUsed(string substance)
//! {
//!     Lua.Run(string.Format("Variable[\"stats.uses_{0}\"] = Variable[\"stats.uses_{0}\"] + 1", substance));
//! }
//! ```
//!
//! ## Why this reads a dialogue variable
//!
//! The count IS a dialogue variable - `stats.uses_alcohol` and its four siblings are declared
//! in the database as numbers starting at 0 - so a group that asks declares it, the plugin
//! already sends its value, and nothing has to run. Its only writer is `SubstanceChargeUsed`,
//! called when the player uses a substance from the HUD; no dialogue script writes it, so it
//! is constant for a search.
//!
//! `AsInt` is Pixel Crushers' `Lua.Result.AsInt`, whose body is not in the export: taken to
//! truncate a number to an integer, as a cast does, and to give 0 for anything that is not a
//! number - the database declares every one of these as a number, so the second case does
//! not arise for a readable save.

use crate::core::guard_value::{GuardValue, GuardValueKind};

/// Each question, with the count it must exceed.
const THRESHOLDS: [(&str, i64); 2] = [("SubstanceUsedOnce", 0), ("SubstanceUsedMore", 3)];

/// The count a question must exceed, or `None` where it is not one of these questions.
fn threshold_of(name: &str) -> Option<i64> {
    THRESHOLDS
        .iter()
        .find(|(query, _)| *query == name)
        .map(|(_, threshold)| *threshold)
}

/// The dialogue variable a question with this literal argument reads, if it is one of these.
pub fn variable_read_by(name: &str, substance: &str) -> Option<String> {
    threshold_of(name).map(|_| format!("stats.uses_{substance}"))
}

/// Whether `name` is one of these questions.
pub fn owns(name: &str) -> bool {
    threshold_of(name).is_some()
}

/// The answer to `name`, reading the count through `read`.
///
/// `None` where `name` is not one of these questions. Unknown where the argument is not a
/// literal substance name or the count could not be read.
pub fn answer(
    name: &str,
    arguments: &[GuardValue],
    read: impl FnOnce(&str) -> Option<GuardValue>,
) -> Option<GuardValue> {
    let threshold = threshold_of(name)?;
    let [substance] = arguments else {
        return Some(GuardValue::unknown());
    };
    if substance.kind() != GuardValueKind::Text {
        return Some(GuardValue::unknown());
    }

    let variable = variable_read_by(name, substance.text()).expect("just matched");
    let Some(count) = read(&variable).filter(|value| value.kind() != GuardValueKind::Unknown)
    else {
        return Some(GuardValue::unknown());
    };

    let used = match count.kind() {
        GuardValueKind::Number => count.number().trunc() as i64,
        _ => 0,
    };
    Some(GuardValue::from_boolean(used > threshold))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(name: &str, count: Option<f64>) -> GuardValue {
        answer(
            name,
            &[GuardValue::from_text("alcohol".to_string())],
            |variable| {
                assert_eq!(variable, "stats.uses_alcohol");
                count.map(GuardValue::from_number)
            },
        )
        .expect("one of these questions")
    }

    #[test]
    fn once_is_any_use_at_all() {
        assert!(!asked("SubstanceUsedOnce", Some(0.0)).boolean());
        assert!(asked("SubstanceUsedOnce", Some(1.0)).boolean());
    }

    #[test]
    fn more_is_more_than_three() {
        assert!(!asked("SubstanceUsedMore", Some(3.0)).boolean());
        assert!(asked("SubstanceUsedMore", Some(4.0)).boolean());
    }

    #[test]
    fn an_unread_count_is_unknown() {
        assert_eq!(
            asked("SubstanceUsedOnce", None).kind(),
            GuardValueKind::Unknown
        );
    }

    #[test]
    fn a_question_this_module_does_not_own_is_left_alone() {
        assert!(answer("IsKimHere", &[], |_| None).is_none());
        assert!(!owns("IsKimHere"));
    }
}
