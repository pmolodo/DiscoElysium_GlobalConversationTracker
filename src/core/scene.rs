// SPDX-License-Identifier: MIT
//! The scene the player is standing in, which is what `IsExterior`, `IsRaining` and
//! `IsSnowing` ask.
//!
//! ## `IsExterior`: read from source
//!
//! From the pre-final-cut export (Assets/Scripts/Assembly-CSharp),
//! `Sunshine.Dialogue.MapLuaFunctions`; Final Cut's body is stripped and taken to be unchanged.
//!
//! ```text
//! public static bool IsExterior()
//! {
//!     return SingletonScriptable<ApplicationManager>.Singleton.CurrentSceneProperties.IsOutside;
//! }
//! ```
//!
//! The plugin reads `CurrentSceneProperties.IsOutside` itself - `DataKind::SceneIsOutside` -
//! and the offline fixture looks the save's area up in `testing/scenes.json`, which is
//! `ApplicationManager.ScenePropertiesList` derived from the game's own asset. Asked through
//! Lua, the call threw a `NullReferenceException` while a save was still loading and read as
//! Unknown; a read that finds no scene properties says so the same way.
//!
//! ## `IsRaining` and `IsSnowing`: MEASURED, not read
//!
//! Both are registered by `FELDLuaFunctions`, which exists only in Final Cut, and whose
//! exported bodies are AssetRipper stubs. What they read was established without the body:
//! `ArcticSwimmerEasterEgg` watches `auto.is_snowing` for the same condition the guards ask
//! about, and evaluating `IsRaining()` in the shipped game over saves that differ in
//! `auto.is_raining` (2026-09-12, see `tests/scene_queries.rs`) answered with the variable.
//! `WeatherController` rewrites both variables from the weather preset whenever the weather
//! changes, including on load, which the offline reader simulates.
//!
//! So both are answered from the dialogue variable, which the plugin already sends.

use crate::core::guard_value::GuardValue;
use crate::core::types::Ternary;

/// The question answered from the plugin's read of the current scene's properties.
pub const IS_EXTERIOR: &str = "IsExterior";

/// Each weather question, with the dialogue variable it reads.
const WEATHER: [(&str, &str); 2] = [
    ("IsRaining", "auto.is_raining"),
    ("IsSnowing", "auto.is_snowing"),
];

/// The dialogue variable a weather question reads, or `None` for anything else.
pub fn variable_read_by(name: &str) -> Option<&'static str> {
    WEATHER
        .iter()
        .find(|(query, _)| *query == name)
        .map(|(_, variable)| *variable)
}

/// The answer to a weather question, reading its variable through `read`.
///
/// `None` where `name` is not a weather question. Unknown where the variable could not be read.
pub fn weather_answer(
    name: &str,
    read: impl FnOnce(&str) -> Option<GuardValue>,
) -> Option<GuardValue> {
    let variable = variable_read_by(name)?;
    Some(match read(variable).map(|value| value.as_condition()) {
        Some(Ternary::True) => GuardValue::from_boolean(true),
        Some(Ternary::False) => GuardValue::from_boolean(false),
        _ => GuardValue::unknown(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::guard_value::GuardValueKind;

    #[test]
    fn the_weather_is_its_variable() {
        let raining = weather_answer("IsRaining", |variable| {
            assert_eq!(variable, "auto.is_raining");
            Some(GuardValue::from_boolean(true))
        })
        .expect("a weather question");
        assert!(raining.boolean());

        let unread = weather_answer("IsSnowing", |_| None).expect("a weather question");
        assert_eq!(unread.kind(), GuardValueKind::Unknown);

        assert!(weather_answer("IsExterior", |_| None).is_none());
    }
}
