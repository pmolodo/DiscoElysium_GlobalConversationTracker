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
//! ## What dialogue does to it
//!
//! `CharacterLuaFunctions`, from the same export:
//!
//! ```text
//! DamageVolition(amount)  -> CharacterManipulations.DamageVolition((int)amount)
//! HealVolition(amount)    -> CharacterManipulations.HealVolition((int)GenericLuaFunctions.Once(amount))  // in conversation
//! HealAllVolition()       -> CharacterManipulations.HealVolition(-you.volition.damageValue)
//! DamageEndurance, HealEndurance, DamageEnduranceWithNewspaper(amount, newspaper): the same for endurance
//!
//! // Modifiable
//! DamageValue(amount): if (amount > value) amount = value;  DAMAGE modifier.Amount -= amount
//! HealValue(amount):   removes up to amount from the DAMAGE modifiers
//! // CharacterManipulations.HealX clamps the heal to maximumValue - value first
//! ```
//!
//! So a search tracks the damage AMOUNT per skill, in a `damage:` slot seeded from what the
//! world reads: damage adds, a heal subtracts and stops at none, healing everything clears
//! it. A heal is once-only in conversation, which is the only place a search runs. The clamp
//! of damage to the skill's current value is not followed: a blow that large ends the game,
//! which is past where a look-ahead answers.

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

/// A damage value as the positive amount a `damage:` slot holds.
pub fn amount_of(damage: f64) -> i32 {
    (-damage).max(0.0).round() as i32
}

/// The script calls that move damage, with the skill each moves, as `(call, skill)`.
pub const DAMAGE_WRITERS: [(&str, &str); 6] = [
    ("DamageVolition", "VOLITION"),
    ("HealVolition", "VOLITION"),
    ("HealAllVolition", "VOLITION"),
    ("DamageEndurance", "ENDURANCE"),
    ("HealEndurance", "ENDURANCE"),
    ("DamageEnduranceWithNewspaper", "ENDURANCE"),
];

/// The skill a damage or heal call moves, or `None` for anything else.
pub fn skill_written_by(call: &str) -> Option<&'static str> {
    DAMAGE_WRITERS
        .iter()
        .find(|(writer, _)| *writer == call)
        .map(|(_, skill)| *skill)
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
