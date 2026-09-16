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
//!   That is exact only while the search does not write it, and a group that both writes
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
    /// the world and the answer goes stale the moment the search writes it.
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

/// Questions the search treats as constant because NOTHING CAN WRITE THEM.
///
/// Not decisions at all, which is why they are here rather than in [`DECISIONS`]: there is
/// no approximation to justify. The game registers 171 Lua functions for dialogue to call,
/// and among them these have readers and no writer whatsoever - so their answer at the
/// start of a search is their answer at the end of it, and asking the world once is exact.
///
/// Read what the game lets dialogue call, rather than reasoning about what it might.
///
/// EQUIPMENT IS NOT HERE, though dialogue has no equip function: `LoseItem` takes a worn item
/// off, so the equipment questions move where a group loses one - see `core::equipment`.
///
/// THE CABINET'S THREE NARROW QUESTIONS ARE THE WHOLE LIST, and the argument is
/// worth stating because it does not look true either. `GainThought` is a modelled write, so
/// `IsTHCCooking`, `IsTHCFixed` and `IsTHCCookingOrFixed` plainly ask about something a
/// conversation moves. They do not. `CharacterThoughts.GainThought` adds to `gainedThoughts`
/// and sets the thought's state to `KNOWN`, while `ThoughtCooking` reads `cookingEffects` and
/// `ThoughtFixed` reads `fixedEffects` - different collections, which only the cabinet screen
/// fills. `THCLuaFunctions` registers exactly five functions and `GainThought` is the only
/// writer among them, so nothing dialogue can call moves these three.
///
/// Time passing is the exception the game has: `PassTime` bakes cooking thoughts
/// (`ThoughtManager.BakeThoughts`), but only while the clock runs, and the plugin always sends
/// it locked - see `docs/actions.md`.
///
/// `IsTHCPresent` is the one that DOES move, and it is not here: it is `gainedThoughts`
/// itself, which is exactly what `GainThought` adds to.
pub const CONSTANT_BY_CONSTRUCTION: &[&str] =
    &["IsTHCCooking", "IsTHCCookingOrFixed", "IsTHCFixed"];

/// Actions the database calls FROM A GUARD, which must never be run to answer one.
///
/// Two functions in the shipped corpus return nothing and exist only for their effect, yet
/// appear where a condition is expected. `FinishTask` closes a journal task;
/// `XPStandardSetBool` sets a dialogue variable and awards experience. Three guards in all -
/// conversation 369 entry 94, and conversation 850 entries 109 and 110.
///
/// WHY THIS LIST HAS TO EXIST. The plugin answers a query by running it as Lua in the live
/// game, so a name that reaches the query list is a name the mod EXECUTES - against the
/// player's save, on every crawl of that group, for a node the player may never reach. The
/// game runs them too when it evaluates the link itself, so the effect is the writers' doing
/// rather than ours; what a look-ahead adds is doing it early, often, and for branches
/// nobody takes.
///
/// So they are kept out of the query list and answered here instead. The answer is FALSE,
/// and that is the game's answer rather than a convenient one: a function returning nothing
/// answers Lua nil, all three guards compare the call against `true`, and `nil == true` is
/// false.
///
/// NOT A MODEL OF THE EFFECT. The game really does finish that task while deciding whether
/// to show the line, and the search does not follow it. That is a deliberate omission - a
/// guard is evaluated speculatively and thousands of times per menu, and a write on that
/// path would be a different feature.
pub const ACTIONS_USED_AS_GUARDS: &[&str] = &["FinishTask", "XPStandardSetBool"];

/// Whether `name` is an action a guard calls, which must be answered rather than run.
///
/// Three places agree about this and each would be wrong alone: `bridge::collect` keeps the
/// name out of what the plugin is asked, `BoundContext::query` answers it, and the guard
/// compiler decides it. A name dropped from the questions but left unanswered would read
/// Unknown, which is permissive - safe for the save and wrong for the marker.
pub fn is_action_used_as_guard(name: &str) -> bool {
    ACTIONS_USED_AS_GUARDS.contains(&name)
}

/// Every decision taken so far.
pub const DECISIONS: &[Decision] = &[
    Decision {
        writers: &["UseSubstanceInHand"],
        readers: &["SubstanceUsedOnce", "SubstanceUsedMore"],
        why: "Substance use is inventory plus a per-substance counter the search has no \
              reader for in practice: 4 conversations use one, 21 ask about one, and no \
              conversation does both.",
    },
    Decision {
        writers: &[
            "ReturnKitsuragi",
            "RemoveAndHideKitsuragi",
            "RemoveAndHideKitsuragiUntilMorning",
            "RemoveKitsuragiWaitAtLair",
            "RemoveKitsuragiWaitAtTent",
            "NightyNightKitsuragiShack",
            "AddCunoToParty",
            "RemoveCunoFromParty",
            "RemoveCunoWaitAtFort",
        ],
        readers: &["IsKimHere", "IsKimInParty", "IsCunoInParty"],
        why: "Who is standing next to you is world state the search reads constantly - \
              IsKimHere alone is 323 guards, the largest single world query - and these \
              move it. Held at the save's answer because a party model is a model of \
              where everybody is, and the writers are rare: 21 scripts across the \
              database. The one with a downstream reader, \
              RemoveKitsuragiWaitAtChurch, is modelled - see core::party.",
    },
    Decision {
        writers: &[
            "SellItemGroup",
            "SellItemGroupWithModifier",
            "ShowInventoryForPawning",
        ],
        readers: &[
            "MoneyAmount",
            "CheckItem",
            "CheckItemGroup",
            "HasPawnablesInInventory",
        ],
        why: "Pawning turns items into money, and the search models both - but how much \
              money depends on what is in the inventory, which the search only knows about \
              for items this group itself moved. Three scripts in the database.",
    },
    Decision {
        writers: &["RemoveWhiteCheck"],
        readers: &[],
        why: "Retiring a white check is read by the CHECK, not by a guard - so no query \
              here goes stale, and `readers` is empty for that reason rather than because \
              nothing notices. What notices is `ILookAheadWorld::check_passes`, which the \
              search already takes at the world's word for every check; a check retired \
              mid-conversation is one the world would now refuse and the search still \
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
        readers: &["IsExterior"],
        why: "Moving the player and rearranging the scenery. The search holds no location \
              and no object state, so both are held at what the world says. IsExterior is \
              the one reader, and it pairs with the MOVEMENT writers rather than the \
              scenery ones: GoTo, GoToDestination, SetAreaState and SkipToDebriefLocation \
              change where the player stands, while nothing in the database reads back \
              whether a fan is on or a door is shut. Measured rather than assumed - no \
              conversation both moves the player and asks IsExterior, so the staleness \
              this admits to is theoretical in the shipped content.",
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
        let mut seen: Vec<&str> = DECISIONS
            .iter()
            .flat_map(|d| d.writers.iter().copied())
            .collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            count,
            seen.len(),
            "a script function is declared by two decisions"
        );
    }

    /// A decision with no reason is not a decision.
    #[test]
    fn every_decision_says_why() {
        for decision in DECISIONS {
            assert!(!decision.writers.is_empty(), "a decision covers nothing");
            assert!(
                decision.why.len() > 40,
                "a decision without a reason: {:?}",
                decision.writers
            );
        }
    }
}
