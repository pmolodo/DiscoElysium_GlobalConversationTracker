// SPDX-License-Identifier: MIT
//! What the database says an item moves, in the engine's own words.
//!
//! ## Why a translation is needed at all
//!
//! An item's bonus is in the dialogue database as the prose a player reads - `+1 Rhetoric: The
//! heroic deeds (of others)` - and `item_names.jsonl` carries it verbatim, spelling and all.
//! Verbatim because normalising it in the extractor would put a copy of the engine's skill list
//! in C#, which is a second thing to keep in step with the game. So the extractor reads the
//! shape and this reads the meaning.
//!
//! ## What it has to cope with
//!
//! Thirty-seven distinct names for about twenty-four skills, because the database is not
//! consistent with itself:
//!
//! - MISSPELLINGS, both real and both shipped: `Electrochemisty` beside `Electrochemistry`,
//!   `Half-Light` beside `Half Light`.
//! - VARIANTS: `Reaction` for what [`crate::core::thought_effects`] calls `REACTION` and the
//!   game's sheet calls Reaction Speed; `H/E Coordination` for `HE_COORDINATION`.
//! - PARENTHETICALS: `Perception (Sight)`, and one condition - `Suggestion (unless wearing full
//!   armor)`.
//! - THINGS THAT ARE NOT SKILLS: Health and Morale, the attribute abbreviations `FYS`, `INT`,
//!   `MOT` and `PSY`, and two thoughts. Every one of them is on a substance rather than on
//!   anything worn, so none reaches the question this exists to answer - but they are named
//!   here so that a name nobody has classified can be told from one deliberately left out.
//!
//! ## Loud rather than silent
//!
//! [`moved_by`] answers `None` for a name it has never been told about, and
//! `tests/corpus.rs` holds the database to it. A name that quietly mapped to nothing would
//! mean a garment stopped unsettling the checks it moves, and the symptom - a marker that is
//! wrong on one entry - is not one anybody would trace back here.

/// What one stated bonus moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moves {
    /// Skills, by the names [`crate::core::thought_effects`] uses.
    ///
    /// More than one where the database's name does not pick out a single skill - see
    /// `Perception (Sight)`, whose two readings are both kept.
    Skills(&'static [&'static str]),
    /// Something a passive check never compares: Health, Morale, an attribute, a thought.
    NotASkill,
}

/// `Perception (Sight)`, which does not say which of the two it means.
///
/// `SKILLS` holds PERCEPTION and SIGHT separately, and the text picks neither cleanly. BOTH
/// are taken, which is the conservative reading: it can only unsettle a check that did not
/// need unsettling, where choosing wrongly would settle one the garment moves.
const PERCEPTION_OR_SIGHT: &[&str] = &["PERCEPTION", "SIGHT"];

/// Every name the shipped database states, and what the engine calls it.
///
/// SORTED BY THE DATABASE'S SPELLING, so a reader can find a name they saw in the data
/// without knowing what it was meant to be.
const STATED: [(&str, Moves); 37] = [
    ("Authority", Moves::Skills(&["AUTHORITY"])),
    ("Composure", Moves::Skills(&["COMPOSURE"])),
    ("Conceptualization", Moves::Skills(&["CONCEPTUALIZATION"])),
    ("Drama", Moves::Skills(&["DRAMA"])),
    ("Electrochemistry", Moves::Skills(&["ELECTROCHEMISTRY"])),
    // Shipped misspelt, on two items.
    ("Electrochemisty", Moves::Skills(&["ELECTROCHEMISTRY"])),
    ("Empathy", Moves::Skills(&["EMPATHY"])),
    ("Encyclopedia", Moves::Skills(&["ENCYCLOPEDIA"])),
    ("Endurance", Moves::Skills(&["ENDURANCE"])),
    ("Esprit de Corps", Moves::Skills(&["ESPRIT_DE_CORPS"])),
    // An ATTRIBUTE, on substances. It moves all four of its skills, which is a different
    // question from the one this answers, and nothing worn carries one.
    ("FYS", Moves::NotASkill),
    ("H/E Coordination", Moves::Skills(&["HE_COORDINATION"])),
    ("Half Light", Moves::Skills(&["HALF_LIGHT"])),
    // Shipped hyphenated, on one item.
    ("Half-Light", Moves::Skills(&["HALF_LIGHT"])),
    ("Health", Moves::NotASkill),
    ("Health (when consumed in dialogue)", Moves::NotASkill),
    ("INT", Moves::NotASkill),
    ("Inland Empire", Moves::Skills(&["INLAND_EMPIRE"])),
    ("Interfacing", Moves::Skills(&["INTERFACING"])),
    // A THOUGHT rather than a skill.
    ("Kingdom of Conscience", Moves::NotASkill),
    ("Logic", Moves::Skills(&["LOGIC"])),
    ("MOT", Moves::NotASkill),
    ("Mazovian Socio-Economics", Moves::NotASkill),
    ("Morale", Moves::NotASkill),
    ("PSY", Moves::NotASkill),
    ("Pain Threshold", Moves::Skills(&["PAIN_THRESHOLD"])),
    ("Perception", Moves::Skills(&["PERCEPTION"])),
    ("Perception (Sight)", Moves::Skills(PERCEPTION_OR_SIGHT)),
    (
        "Physical Instrument",
        Moves::Skills(&["PHYSICAL_INSTRUMENT"]),
    ),
    ("Reaction", Moves::Skills(&["REACTION"])),
    ("Reaction Speed", Moves::Skills(&["REACTION"])),
    ("Rhetoric", Moves::Skills(&["RHETORIC"])),
    ("Savoir Faire", Moves::Skills(&["SAVOIR_FAIRE"])),
    ("Shivers", Moves::Skills(&["SHIVERS"])),
    ("Suggestion", Moves::Skills(&["SUGGESTION"])),
    // THE CONDITION IS NOT READ, and the bonus is taken as always applying. This decides which
    // checks a garment CAN unsettle, and one that applies sometimes can still move one.
    (
        "Suggestion (unless wearing full armor)",
        Moves::Skills(&["SUGGESTION"]),
    ),
    ("Visual Calculus", Moves::Skills(&["VISUAL_CALCULUS"])),
];

/// What `stated` moves, or `None` for a name nothing here has classified.
///
/// `None` is the loud answer and is what `tests/corpus.rs` holds the database to. It means the
/// dialogue has started saying something new, which wants a person rather than a default.
pub fn moved_by(stated: &str) -> Option<Moves> {
    STATED
        .iter()
        .find(|(name, _)| *name == stated)
        .map(|(_, moves)| *moves)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_misspelling_and_its_correct_form_reach_the_same_skill() {
        assert_eq!(moved_by("Electrochemisty"), moved_by("Electrochemistry"));
        assert_eq!(moved_by("Half-Light"), moved_by("Half Light"));
    }

    #[test]
    fn a_variant_reaches_the_name_the_engine_uses() {
        assert_eq!(
            moved_by("Reaction Speed"),
            Some(Moves::Skills(&["REACTION"]))
        );
        assert_eq!(
            moved_by("H/E Coordination"),
            Some(Moves::Skills(&["HE_COORDINATION"]))
        );
    }

    /// The condition does not change which skill it is about.
    #[test]
    fn a_conditional_bonus_still_names_its_skill() {
        assert_eq!(
            moved_by("Suggestion (unless wearing full armor)"),
            moved_by("Suggestion")
        );
    }

    #[test]
    fn an_ambiguous_name_keeps_both_readings() {
        assert_eq!(
            moved_by("Perception (Sight)"),
            Some(Moves::Skills(PERCEPTION_OR_SIGHT))
        );
    }

    #[test]
    fn what_a_passive_check_never_compares_is_not_a_skill() {
        for name in ["Health", "Morale", "FYS", "INT", "MOT", "PSY"] {
            assert_eq!(moved_by(name), Some(Moves::NotASkill), "{name}");
        }
    }

    #[test]
    fn a_name_nobody_has_classified_is_refused_rather_than_guessed_at() {
        assert_eq!(moved_by("Sharpshooting"), None);
    }

    /// Every skill named here is one the engine holds - see `thought_effects::SKILLS`.
    #[test]
    fn every_skill_named_is_one_the_engine_has() {
        for (stated, moves) in STATED {
            let Moves::Skills(skills) = moves else {
                continue;
            };
            for skill in skills {
                assert!(
                    crate::core::thought_effects::names_a_skill(skill),
                    "{stated} maps to {skill}, which is not a skill the engine holds"
                );
            }
        }
    }
}
