// SPDX-License-Identifier: MIT
//! What the model has DECIDED about the things it does not simulate.
//!
//! ## Why a decision needs writing down
//!
//! An action the parser does not recognise becomes [`DialogueActionKind::Unmodelled`] and
//! is silently skipped, and a guard the compiler cannot read becomes undecided
//! everywhere. Both are reported as gaps, and both are the same shape whether nobody has
//! looked at them yet or somebody looked, thought about it, and decided the model is
//! better off without them.
//!
//! Those are not the same thing at all. The first is work outstanding; the second is
//! work finished. A report that cannot tell them apart makes the finished work look
//! unfinished forever, and - worse - buries the one gap somebody should be looking at
//! under forty that have already been settled.
//!
//! So a decision is declared here, with the evidence that justifies it, and the parser
//! turns a declared action into a stub that does nothing rather than into an unknown.
//! What is left in the unknown bucket is then exactly what nobody has decided yet.
//!
//! ## What a decision may say, and what it costs
//!
//! Two shapes, and the difference is whether anything can notice.
//!
//! - NO READER. No guard in the database asks about what the action writes, so applying
//!   it could not change which branch a compiled guard takes. [`Decision::readers`] is
//!   empty. Read that as "no GUARD reads it" rather than "nothing does" - the reason is
//!   in the decision's own text, and `RemoveWhiteCheck` is the one whose reader is the
//!   check machinery instead.
//!
//! - HELD CONSTANT. Guards DO read what it writes, and the model answers them from the
//!   world instead - the same rule that lets an untracked variable compile to a literal.
//!   That is exact only while the crawl does not write it, and a group that both writes
//!   and reads the same subject is walking a branch the model has judged against a stale
//!   answer. [`Decision::readers`] names the guard functions to watch, so a report can
//!   ask whether the group in front of it actually does both - see the modelling-gaps
//!   report, which does exactly that rather than assuming.
//!
//! Counts quoted below are over the extracted corpus - `distinct_scripts.txt` for
//! writers, `distinct_guards.txt` for readers - and were measured, not estimated.

/// One settled question about something the model does not simulate.
pub struct Decision {
    /// The script functions this covers, as they are written in a userScript.
    pub writers: &'static [&'static str],
    /// The guard functions that read back what those write - empty when none do.
    pub readers: &'static [&'static str],
    /// What was decided, and the evidence for it.
    pub why: &'static str,
}

impl Decision {
    /// Whether anything can notice this being skipped.
    ///
    /// True when guards read what the writers write, so the model is answering them from
    /// the world and the answer goes stale the moment the crawl writes it.
    pub fn is_held_constant(&self) -> bool {
        !self.readers.is_empty()
    }

    /// A one-line summary for a report.
    pub fn verdict(&self) -> &'static str {
        if self.is_held_constant() {
            "held constant"
        } else {
            "no reader"
        }
    }
}

/// Every decision taken so far.
pub const DECISIONS: &[Decision] = &[
    Decision {
        writers: &[
            "DamageVolition",
            "HealVolition",
            "HealAllVolition",
            "DamageEndurance",
            "HealEndurance",
            "DamageEnduranceWithNewspaper",
        ],
        readers: &["HasVolitionDamage", "HasEnduranceDamage"],
        why: "Morale and health are the character sheet, not dialogue state, and modelling \
              them means two more counters plus the healing items and skill checks that \
              move them - which is a second simulation, not a slot. 150 conversations \
              write them; 6 guards in the whole database read them back.",
    },
    Decision {
        writers: &["GainThought"],
        // IsTHCPresent ALONE, and the omissions are the point. A thought has three states
        // the guards ask about separately, and conversation 636 forks on all three in a
        // row - Fixed, then Cooking, then Present. Only the last is something a script
        // can reach.
        //
        // `Sunshine.Dialogue.THCLuaFunctions` in the decompiled game settles it. It
        // registers exactly five Lua functions - GainThought and the four readers - so
        // the dialogue's entire vocabulary for thoughts is one writer. That writer runs
        // `CharacterThoughts.GainThought`, which is `gainedThoughts.Add(project)`, and
        // `IsTHCPresent` is `gainedThoughts.Contains(project)`: the same set, written and
        // read synchronously. COOKING and FIXED live in `cookingEffects`/`fixedEffects`,
        // which only `ThoughtSlotsTree` - the cabinet screen - and save loading write. So
        // `IsTHCCooking` and `IsTHCFixed` cannot move during a crawl at any price, and
        // naming them here would report an exposure that cannot happen.
        readers: &["IsTHCPresent"],
        why: "A thought should be tracked the way an item is - a `thought:` slot written by \
              GainThought and read by IsTHCPresent - and the game makes that a fair model: \
              the gain is synchronous and idempotent (`Inventory.CanBeGained` refuses a \
              thought already gained), so it is an assign of 1 and nothing subtler. It is \
              not modelled yet, so the cabinet is whatever the save says. 48 conversations \
              gain a thought and 183 read one, and 28 do both.",
    },
    Decision {
        writers: &["UseSubstanceInHand"],
        readers: &["SubstanceUsedOnce", "SubstanceUsedMore"],
        why: "Substance use is inventory plus a per-substance counter the crawl has no \
              reader for in practice: 4 conversations use one, 21 ask about one, and no \
              conversation does both.",
    },
    Decision {
        writers: &[
            "ReturnKitsuragi",
            "RemoveAndHideKitsuragi",
            "RemoveAndHideKitsuragiUntilMorning",
            "RemoveKitsuragiWaitAtChurch",
            "RemoveKitsuragiWaitAtLair",
            "RemoveKitsuragiWaitAtTent",
            "NightyNightKitsuragiShack",
            "AddCunoToParty",
            "RemoveCunoFromParty",
            "RemoveCunoWaitAtFort",
        ],
        readers: &["IsKimHere", "IsKimInParty", "IsCunoInParty"],
        why: "Who is standing next to you is world state the crawl reads constantly - \
              IsKimHere alone is 323 guards, the largest single world query - and these \
              move it. Held at the save's answer because a party model is a model of \
              where everybody is, and the writers are rare: 21 scripts across the \
              database.",
    },
    Decision {
        writers: &["NextMorningTime"],
        readers: &[
            "DayCount", "HourCount", "TotalHourCount", "IsHour", "IsHourBetween",
            "IsMorning", "IsAfternoon", "IsEvening", "IsNight", "IsNighttime",
            "IsDaytime", "IsNoon", "IsDusk", "IsMidnight", "IsDayFrom", "IsDayUntil",
        ],
        why: "Sleeping to the next morning is the one thing that moves the clock by more \
              than a PassTime, and it ends the day - which is past where a look-ahead is \
              answering. Two scripts in the database, and the constant-clock \
              approximation already covers the readers.",
    },
    Decision {
        writers: &["SellItemGroup", "SellItemGroupWithModifier", "ShowInventoryForPawning"],
        readers: &["MoneyAmount", "CheckItem", "CheckItemGroup", "HasPawnablesInInventory"],
        why: "Pawning turns items into money, and the crawl models both - but how much \
              money depends on what is in the inventory, which the crawl only knows about \
              for items this group itself moved. Three scripts in the database.",
    },
    Decision {
        writers: &["RemoveWhiteCheck"],
        readers: &[],
        why: "Retiring a white check is read by the CHECK, not by a guard - so no query \
              here goes stale, and `readers` is empty for that reason rather than because \
              nothing notices. What notices is `ILookAheadWorld::check_passes`, which the \
              crawl already takes at the world's word for every check; a check retired \
              mid-conversation is one the world would now refuse and the crawl still \
              offers. Two scripts in the database.",
    },
    Decision {
        writers: &["Obsession"],
        readers: &[],
        why: "An obsession is a journal flavour entry. Nothing in the guard corpus asks \
              about one.",
    },
    Decision {
        writers: &[
            "ShowVisCal",
            "HideVisCal",
            "HideVisCalAfterConversation",
            "ShowDialogueImage",
            "HideDialogueImage",
            "PlaySoundGroup",
            "ResetCamera",
            "TequilaExpressionStopped",
            "TequilaUnobscured",
        ],
        readers: &[],
        why: "Presentation: pictures, sound and camera. Nothing in the guard corpus asks \
              about any of it.",
    },
    Decision {
        writers: &[
            "GoTo",
            "GoToDestination",
            "SetAreaState",
            "SkipToDebriefLocation",
            "DestroyObject",
            "CloseTequilaDoor",
            "OpenBookstoreCurtains",
            "TurnOnFanLight",
            "TurnOffFanLight",
            "TurnOffCeilingFan",
            "GraffitoAlight",
            "GraffitoExtinguish",
            "WhirlingEngineStart",
            "ShackBedWasUsed",
            "WhirlingBedWasUsed",
            "LetterSleep",
            "TequilaWakeUp",
            "TequilaShaved",
            "TequilaFascist",
            "TequilaPutOnBodysuit",
            "TequilaRemoveBodysuit",
        ],
        readers: &[],
        why: "Moving the player and rearranging the scenery. The crawl holds no location \
              and no object state, and the only spatial guard in the database, \
              IsExterior, is answered from the world like any other constant.",
    },
    Decision {
        writers: &["IsTHCPresent"],
        readers: &[],
        why: "A QUESTION, used as a statement, with its answer thrown away - one script in \
              the database is nothing but `IsTHCPresent(\"revacholian_nationhood\")`. It \
              writes nothing in the game either, so skipping it is not an approximation; \
              it is the same thing the game does.",
    },
    Decision {
        writers: &["NewspaperEndgame", "PosseEndgame", "PrimeSpecialEndButton"],
        readers: &[],
        why: "The endgame, which is past where a look-ahead has anything to say. Nothing \
              in the guard corpus asks about it.",
    },
];

/// The decision covering a script function, if one has been taken.
pub fn for_action(name: &str) -> Option<&'static Decision> {
    DECISIONS.iter().find(|d| d.writers.contains(&name))
}

/// The decision whose writers a guard function reads back, if any.
///
/// The reverse lookup, for the compiler: answering this query from the world is exact
/// only while nothing writes it, and this says who would.
pub fn for_query(name: &str) -> Option<&'static Decision> {
    DECISIONS.iter().find(|d| d.readers.contains(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No function may be covered twice: the first match would win and the second would
    /// be a decision nobody could see taking effect.
    #[test]
    fn every_declared_function_is_declared_once() {
        let mut seen: Vec<&str> = DECISIONS.iter().flat_map(|d| d.writers.iter().copied()).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(count, seen.len(), "a script function is declared by two decisions");
    }

    /// A decision with no reason is not a decision.
    #[test]
    fn every_decision_says_why() {
        for decision in DECISIONS {
            assert!(!decision.writers.is_empty(), "a decision covers nothing");
            assert!(decision.why.len() > 40, "a decision without a reason: {:?}", decision.writers);
        }
    }
}
