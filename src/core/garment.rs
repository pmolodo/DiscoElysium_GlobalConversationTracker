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

/// Which skills each item moves, by the engine's names for them.
///
/// ## Why a const rather than a file
///
/// `item_names.jsonl` holds this, and the engine cannot read it: that file is game data a
/// TEST regenerates, and nothing ships it to a player. The alternative would be a third
/// deployed file beside the index and the variable table, for fifty-five rows that change only
/// when the game does - so it is a const, as `skill_movers::AUTOEQUIP_ITEMS` beside it already
/// is for the same reason.
///
/// ## How to rebuild it, and what keeps it honest
///
/// It is derived from `item_names.jsonl` through [`moved_by`], so regenerating it is reading
/// that file and mapping each stated name. `tests/corpus.rs` holds the two to each other: an
/// item the database gives a skill bonus that is missing here, or one here the database no
/// longer moves, fails there rather than quietly changing which checks a garment unsettles.
///
/// SORTED BY ITEM NAME, and the skills within a row sorted too, so a regeneration that changes
/// nothing produces no diff.
const MOVED_BY_ITEM: [(&str, &[(&str, i32)]); 55] = [
    (
        "glasses_flipup",
        &[("AUTHORITY", -1), ("VISUAL_CALCULUS", 1)],
    ),
    (
        "glasses_megabinos",
        &[("ENCYCLOPEDIA", 2), ("PERCEPTION", -4), ("SIGHT", -4)],
    ),
    (
        "glasses_self_destruction",
        &[("ELECTROCHEMISTRY", 1), ("ENDURANCE", -1)],
    ),
    (
        "glasses_sub_insulindics",
        &[("INLAND_EMPIRE", 1), ("PERCEPTION", 1), ("SIGHT", 1)],
    ),
    ("gloves_bum", &[("ELECTROCHEMISTRY", 1)]),
    ("gloves_faln", &[("INTERFACING", 1)]),
    ("gloves_garden", &[("INTERFACING", 1)]),
    ("gloves_t500", &[("INTERFACING", 2)]),
    (
        "hat_amphibian_sports_visor",
        &[("PERCEPTION", 1), ("SIGHT", 1)],
    ),
    (
        "hat_faln",
        &[("LOGIC", 1), ("PERCEPTION", -1), ("SIGHT", -1)],
    ),
    ("hat_headset", &[("INLAND_EMPIRE", 2), ("REACTION", -1)]),
    ("hat_mullen", &[("ENCYCLOPEDIA", 1)]),
    ("hat_rcm", &[("AUTHORITY", 1)]),
    ("hat_samaran", &[("LOGIC", 1), ("SUGGESTION", -1)]),
    ("hat_t500", &[("HALF_LIGHT", 1), ("SUGGESTION", -1)]),
    (
        "jacket_faln",
        &[("HALF_LIGHT", 1), ("PAIN_THRESHOLD", 1), ("SUGGESTION", -2)],
    ),
    ("jacket_fritte_raincoat", &[("ENDURANCE", 1)]),
    ("jacket_fucktheworld", &[("SAVOIR_FAIRE", 1)]),
    ("jacket_interisolar", &[("SUGGESTION", 1)]),
    (
        "jacket_kimono_robe",
        &[("DRAMA", 1), ("ELECTROCHEMISTRY", 1)],
    ),
    ("jacket_korovjev", &[("CONCEPTUALIZATION", 1)]),
    ("jacket_mullen", &[("DRAMA", 1)]),
    ("jacket_navalcoat", &[("HALF_LIGHT", -1)]),
    (
        "jacket_patrol_cloak",
        &[("ESPRIT_DE_CORPS", 1), ("SHIVERS", 1)],
    ),
    ("jacket_pissflaubert", &[("AUTHORITY", -1), ("DRAMA", 1)]),
    (
        "jacket_rcm",
        &[
            ("AUTHORITY", 1),
            ("ESPRIT_DE_CORPS", 1),
            ("VISUAL_CALCULUS", 1),
        ],
    ),
    (
        "jacket_reflective_vest",
        &[("ENDURANCE", 2), ("REACTION", -1)],
    ),
    ("jacket_suede", &[("ESPRIT_DE_CORPS", 1)]),
    (
        "jacket_windbreaker_surf",
        &[("COMPOSURE", 1), ("SHIVERS", -1)],
    ),
    ("neck_bowtie", &[("DRAMA", 2)]),
    (
        "neck_scented_scarf",
        &[("PHYSICAL_INSTRUMENT", -2), ("SHIVERS", 1)],
    ),
    ("neck_setting_sun_medal", &[("RHETORIC", 1)]),
    ("neck_teratorn_tie", &[("INLAND_EMPIRE", 1)]),
    ("neck_tie", &[("INLAND_EMPIRE", 1)]),
    ("neck_winter_scarf", &[("EMPATHY", 1)]),
    ("neck_winter_scarf_red", &[("PAIN_THRESHOLD", 1)]),
    (
        "pants_bellbottom",
        &[("ELECTROCHEMISTRY", 1), ("SAVOIR_FAIRE", -1)],
    ),
    ("pants_carabineer", &[("REACTION", 1)]),
    (
        "pants_faln",
        &[("PHYSICAL_INSTRUMENT", 3), ("SAVOIR_FAIRE", 2)],
    ),
    (
        "pants_itchy_angry",
        &[("COMPOSURE", -1), ("HALF_LIGHT", 2), ("SAVOIR_FAIRE", -1)],
    ),
    ("pants_jeans", &[("ELECTROCHEMISTRY", 1), ("REACTION", -1)]),
    ("pants_jeans_black", &[("LOGIC", 1)]),
    ("pants_jeans_red", &[("PHYSICAL_INSTRUMENT", 1)]),
    ("pants_rcm", &[("AUTHORITY", 1), ("SUGGESTION", 1)]),
    (
        "shirt_dress_disco",
        &[("CONCEPTUALIZATION", 1), ("SUGGESTION", -1)],
    ),
    ("shirt_faln", &[("HE_COORDINATION", 1)]),
    (
        "shirt_hjelmdall",
        &[
            ("AUTHORITY", -2),
            ("PHYSICAL_INSTRUMENT", 1),
            ("SHIVERS", 1),
        ],
    ),
    ("shirt_interisolar", &[("LOGIC", 1)]),
    ("shirt_mesh", &[("DRAMA", 1)]),
    ("shirt_polo", &[("EMPATHY", -1), ("RHETORIC", 1)]),
    (
        "shirt_t500",
        &[("AUTHORITY", 1), ("EMPATHY", -1), ("PAIN_THRESHOLD", 1)],
    ),
    ("shirt_tank_top", &[("PHYSICAL_INSTRUMENT", 1)]),
    ("shoes_faln", &[("HE_COORDINATION", 1), ("REACTION", 1)]),
    ("shoes_fancy_loafer_brown", &[("PERCEPTION", 1)]),
    ("shoes_snakeskin", &[("COMPOSURE", 1), ("SAVOIR_FAIRE", -1)]),
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

/// What wearing `item` does to each skill it moves, signed, or an empty slice for an item that
/// moves none.
///
/// POSITIVE IS WHAT WEARING IT ADDS, so taking it off subtracts the same. EMPTY IS AN ANSWER,
/// not an absence: nearly every item in the game moves no skill, so a garment that is not here
/// is one whose coming off cannot flip a check.
pub fn skills_moved_by_item(item: &str) -> &'static [(&'static str, i32)] {
    MOVED_BY_ITEM
        .iter()
        .find(|(name, _)| *name == item)
        .map(|(_, moved)| *moved)
        .unwrap_or(&[])
}

/// How far a group can move one skill, each way, by what it takes off and puts on.
///
/// ## Why both ways, and why a sum
///
/// A check flips when its margin crosses zero, so which direction matters: a check that passes
/// is flipped by the skill FALLING and one that fails by it RISING. A group can do both - take
/// off a garment that helped and put on one that helps more - so each direction is accumulated
/// separately.
///
/// THE SUMS ARE UPPER BOUNDS, deliberately. A group that can remove two hats worth one apiece
/// is treated as able to lower the skill by two, whether or not any route through it does both.
/// That can unsettle a check nothing would really flip; the other way would settle one that
/// flips, which is the answer a player sees as a marker that should not be there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reach {
    /// The most the skill can fall.
    pub down: i32,
    /// The most it can rise.
    pub up: i32,
}

impl Reach {
    /// Adds what taking `item` off would do.
    pub fn taking_off(&mut self, item: &str, skill: &str) {
        self.moves(item, skill, -1);
    }

    /// Adds what putting `item` on would do.
    pub fn putting_on(&mut self, item: &str, skill: &str) {
        self.moves(item, skill, 1);
    }

    /// `sign` is +1 where the garment's bonus is applied and -1 where it is taken away.
    fn moves(&mut self, item: &str, skill: &str, sign: i32) {
        for (moved, amount) in skills_moved_by_item(item) {
            if *moved != skill {
                continue;
            }
            let change = amount * sign;
            if change < 0 {
                self.down += -change;
            } else {
                self.up += change;
            }
        }
    }

    /// Whether a check at `margin` can be flipped by this reach.
    ///
    /// The margin is the skill value plus the check's bonus, minus its threshold: zero or more
    /// clears it. So a passing check flips where the skill can fall past zero, and a failing
    /// one where it can rise to meet it.
    pub fn can_flip(&self, margin: i32) -> bool {
        if margin >= 0 {
            self.down > margin
        } else {
            self.up >= -margin
        }
    }
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

    #[test]
    fn taking_off_what_helped_can_flip_a_check_that_was_passing() {
        let mut reach = Reach::default();
        reach.taking_off("hat_mullen", "ENCYCLOPEDIA");

        assert_eq!(reach, Reach { down: 1, up: 0 });
        // The hat is worth one, so a check passing by nothing flips and one passing by one does
        // not - it would sit exactly on the threshold, which still clears.
        assert!(reach.can_flip(0));
        assert!(!reach.can_flip(1));
    }

    #[test]
    fn putting_on_what_helps_can_flip_a_check_that_was_failing() {
        let mut reach = Reach::default();
        reach.putting_on("hat_mullen", "ENCYCLOPEDIA");

        assert_eq!(reach, Reach { down: 0, up: 1 });
        assert!(reach.can_flip(-1));
        assert!(!reach.can_flip(-2));
    }

    /// A garment that HURT a skill raises it by coming off.
    #[test]
    fn taking_off_a_penalty_raises_the_skill() {
        let mut reach = Reach::default();
        reach.taking_off("glasses_flipup", "AUTHORITY");

        assert_eq!(
            reach,
            Reach { down: 0, up: 1 },
            "the glasses cost one Authority"
        );
        assert!(reach.can_flip(-1));
    }

    #[test]
    fn a_skill_the_garment_does_not_move_reaches_nothing() {
        let mut reach = Reach::default();
        reach.taking_off("hat_mullen", "LOGIC");

        assert_eq!(reach, Reach::default());
        assert!(!reach.can_flip(0));
        assert!(!reach.can_flip(-1));
    }

    #[test]
    fn an_item_that_moves_nothing_answers_an_empty_slice() {
        assert!(skills_moved_by_item("key_trash_container").is_empty());
    }

    #[test]
    fn an_items_skills_are_the_ones_the_engine_holds() {
        for (item, moved) in MOVED_BY_ITEM {
            assert!(!moved.is_empty(), "{item} is listed but moves nothing");
            for (skill, amount) in moved {
                assert!(
                    crate::core::thought_effects::names_a_skill(skill),
                    "{item} moves {skill}, which is not a skill the engine holds"
                );
                assert_ne!(*amount, 0, "{item} moves {skill} by nothing");
            }
        }
    }

    /// Sorted, so a regeneration that changes nothing produces no diff.
    #[test]
    fn the_table_is_sorted_by_item_and_by_skill() {
        let mut previous = "";
        for (item, moved) in MOVED_BY_ITEM {
            assert!(previous <= item, "{item} is out of order, after {previous}");
            previous = item;

            let named: Vec<&str> = moved.iter().map(|(skill, _)| *skill).collect();
            let mut sorted = named.clone();
            sorted.sort_unstable();
            assert_eq!(named, sorted, "{item}'s skills are out of order");
        }
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
