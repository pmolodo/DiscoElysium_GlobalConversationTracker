// SPDX-License-Identifier: MIT
//! Work in from the target, under a budget, and say plainly when the budget ran out.
//!
//! ## The backward driver is the whole search, and the last word
//!
//! There is no second stage. An unsettled backward driver is the END, and what it found is
//! reported as what it is - a LOWER BOUND, with [`Answered::Partly`] saying so. Every class
//! it refused, it refused completely; an unasked candidate might have carried a better one.
//!
//! A state-at-a-time search could be run here instead, and would answer some groups this one
//! gives up on. Nothing measured says which: the two lower bounds would differ only in the
//! direction the backward driver is already better at, and paying for a second whole search
//! to find that out is what the budgets below exist to avoid.
//!
//! ## Why one search and not a choice between two
//!
//! Searching in from the start instead is a reasonable thing to want, and it was measured at
//! length. Over the whole game, 395 response menus of eight options each - which is the unit
//! a player waits for - it answered 80% of a menu's options and made the menus 2.4 times
//! slower, 22.9 seconds against 9.5. The options it answers are the EASY ones, the ones where
//! something novel is close, which is exactly why it answers them; and an option easy to
//! reach from the start is cheap to reach from the target too. So what it absorbed was never
//! work this driver would have struggled with.
//!
//! It was slower in every bucket, including the 215 menus where it answered every option, and
//! the per-option reading agreed on all 2,605 rows of a whole-game matrix. There is no group
//! where it is worth carrying, and no reading under which it wins.
//!
//! ## What the budget is protecting against
//!
//! The tail, not the median. Conversation 368's median target takes 247ms and its worst
//! takes 78.8 SECONDS. A median-shaped budget would be far too tight for the groups that
//! win and far too loose for the ones that lose; what makes this work is that a group
//! which is going to be slow is usually slow immediately.

use std::time::Duration;

/// How long the search gets, and how that is divided.
pub struct Budget {
    /// THE WALL. Everything below is an estimate; this is the one that binds.
    ///
    /// de-cluo. `LookAheadTimeBudgetMs` is documented to players as "the longest one
    /// option's look-ahead may run for" and it was not: the candidate loop tested its clock
    /// and then allowed a whole `each` past it, so a dial set to 1000 could return at
    /// roughly 1300.
    ///
    /// The rations below are kept as what they are - estimates of what each part should
    /// need - and are narrowed to the time actually left as the answer is assembled. So the
    /// shape of the search is unchanged where it fits, and where it does not the answer
    /// arrives when it said it would.
    pub overall: Duration,
    /// The whole backward attempt, across every candidate.
    pub backwards: Duration,
    /// One candidate's fixed point.
    ///
    /// A CEILING FOR CALLERS THAT WANT ONE rather than a ration the search imposes on
    /// itself: by default it is the whole backward attempt, so a candidate may spend
    /// whatever the wall has left. The driver narrows it to the time actually remaining
    /// before every pass, so [`Self::overall`] binds a candidate whether this does or not.
    ///
    /// SETTING IT BELOW THE ATTEMPT BUYS A FASTER GIVE-UP AND NOTHING ELSE. The obvious
    /// reading is that it protects the candidates after a heavy one - a group whose sets
    /// explode does so on its first, and a cap would cut that one off and leave the rest
    /// their time. The driver does not work that way: an unsettled pass ends the whole
    /// attempt at [`best_novelty`]'s `StoppedBy::Incomplete`, because the answer is
    /// already a lower bound that no later candidate improves. So a cap decides how long
    /// the search takes to give up, not how many candidates it reaches.
    ///
    /// Two callers still want one. [`crate::bridge::LookAheadRequest::state_budget`] sets
    /// it to zero, which is how a test provokes a search that can establish nothing; and
    /// the measurement columns state their own, since a column is only readable against a
    /// ration it names.
    pub each: Duration,
}

impl Budget {
    /// The same rations, held to `left` as well as to whatever they already said.
    ///
    /// ONLY [`Self::overall`] MOVES, and that is enough rather than an oversight: every
    /// other clock here is already narrowed against it where it is spent. The backward
    /// driver takes `backwards.min(overall - elapsed)` and narrows each candidate again to
    /// what is left of that. So one number binds them all, and narrowing the rest as well
    /// would restate the same limit in two places for a reader to keep in step.
    ///
    /// `Duration::MAX` is the identity, which is what a caller with no wall of its own
    /// passes.
    pub fn within(&self, left: Duration) -> Self {
        Self {
            overall: self.overall.min(left),
            backwards: self.backwards,
            each: self.each,
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        // A CANDIDATE GETS THE WHOLE BACKWARD ATTEMPT, so the wall is the only clock that
        // stops one. See [`Budget::each`] for why a smaller ration is not the protection it
        // looks like: the attempt ends at the first pass that fails to settle either way,
        // so cutting that pass short shortens the search without buying anything for the
        // candidates behind it.
        let backwards = Duration::from_secs(2);
        Self {
            // THE BACKWARD ATTEMPT, which is the whole of the search. Stated rather than
            // derived so a reader can see the number the answer is promised in.
            overall: backwards,
            backwards,
            each: backwards,
        }
    }
}
