// SPDX-License-Identifier: MIT
//! Whether the player's health or morale is damaged, which is what `HasEnduranceDamage` and
//! `HasVolitionDamage` ask.
//!
//! ## The game's definition
//!
//! From the pre-final-cut export (Assets/Scripts/Assembly-CSharp):
//! `Sunshine.Dialogue.CharacterLuaFunctions` and `Sunshine.Metric.Modifiable`. Final Cut's
//! bodies are stripped and taken to be unchanged.
//!
//! ```text
//! public static bool HasEnduranceDamage()
//! {
//!     return (double)SingletonComponent<World>.Singleton.you.endurance.damageValue < 0.0;
//! }
//!
//! public static bool HasVolitionDamage()
//! {
//!     return (double)SingletonComponent<World>.Singleton.you.volition.damageValue < 0.0;
//! }
//!
//! // Modifiable, recalculating from its modifiers:
//! if (modifier.type == ModifierType.DAMAGE)
//! {
//!     damageValue += amount;
//! }
//! ```
//!
//! So the damage is the sum of the skill's `DAMAGE` modifiers. The plugin reads `damageValue`
//! itself - `DataKind::SkillDamage` - and a save records the modifiers, in the character
//! sheet's `SkillModifierCauseMap`, which the offline fixture sums.
//!
//! Constant for a search: damage and healing are the character sheet rather than dialogue
//! state, held at what the world says by a recorded modelling decision.

/// Each question, with the skill whose damage it asks about, by `SkillType` name.
const DAMAGE_QUERIES: [(&str, &str); 2] = [
    ("HasEnduranceDamage", "ENDURANCE"),
    ("HasVolitionDamage", "VOLITION"),
];

/// The skill a damage question asks about, or `None` for anything else.
pub fn skill_read_by(name: &str) -> Option<&'static str> {
    DAMAGE_QUERIES
        .iter()
        .find(|(query, _)| *query == name)
        .map(|(_, skill)| *skill)
}

/// Whether a skill with this damage value is damaged.
pub fn is_damaged(damage: f64) -> bool {
    damage < 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_is_a_negative_value() {
        assert_eq!(skill_read_by("HasVolitionDamage"), Some("VOLITION"));
        assert_eq!(skill_read_by("IsKimHere"), None);
        assert!(is_damaged(-1.0));
        assert!(!is_damaged(0.0));
    }
}
