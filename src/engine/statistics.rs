// SPDX-License-Identifier: MIT
//! What a run of crawls cost, accumulated.
//!
//! Folds together three things the C# `LookAheadStatistics` kept apart - the tally, the
//! buckets and the rendering - because nothing ever wanted one without the others.

use std::collections::HashMap;
use std::fmt;

use crate::core::types::{DialogueNodeId, LookAheadLimit, Novelty};
use crate::engine::engine::LookAheadResult;

/// The upper bound of each histogram bucket, in states. The last bucket catches
/// everything above the highest bound.
pub const BUCKET_BOUNDS: [usize; 5] = [10, 100, 1_000, 10_000, 100_000];

/// Which histogram bucket a state count falls in.
pub fn bucket_of(states: usize) -> usize {
    BUCKET_BOUNDS.iter().position(|&bound| states <= bound).unwrap_or(BUCKET_BOUNDS.len())
}

/// A readable label for a bucket, such as `101-1000`.
pub fn bucket_label(bucket: usize) -> String {
    let low = if bucket == 0 { 0 } else { BUCKET_BOUNDS[bucket - 1] + 1 };
    match BUCKET_BOUNDS.get(bucket) {
        Some(high) => format!("{low}-{high}"),
        None => format!("{low}+"),
    }
}

/// Running totals over some set of crawls.
///
/// ONE type, used both for the whole run and for each conversation in it. The C# has two
/// - `LookAheadStatistics` and `ConversationStatistics` - which accumulate the same six
/// quantities by the same arithmetic in two places, and the per-conversation one is the
/// poorer for it: it has no node counts and no minimum. Here every conversation gets
/// everything, and there is one place the accumulation can be wrong.
#[derive(Debug, Clone, Default)]
pub struct Tally {
    pub crawls: u64,
    pub total_states: u64,
    pub max_states: usize,
    /// `None` until something is recorded.
    ///
    /// The C# seeds its equivalent with `int.MaxValue` and leaves it there when no crawl
    /// happens, so a report over an empty run prints 2147483647 as its minimum. An
    /// absent minimum is not a very large one.
    pub min_states: Option<usize>,
    pub total_nodes: u64,
    pub max_nodes: usize,
    pub total_milliseconds: f64,
    pub max_milliseconds: f64,
    /// Crawls stopped by the state budget. Disjoint from [`Self::stopped_by_time`].
    ///
    /// The C# instead counts `BudgetExhausted` for EITHER limit and `TimeExhausted` as a
    /// subset of it, so the two overlap and the states-only figure is a subtraction a
    /// reader has to know to make. These two partition the stopped crawls.
    pub stopped_by_states: u64,
    pub stopped_by_time: u64,
}

impl Tally {
    /// Folds in one crawl.
    pub fn record(&mut self, result: &LookAheadResult, milliseconds: f64) {
        self.crawls += 1;
        self.total_states += result.states_explored as u64;
        self.total_nodes += result.nodes_reached as u64;
        self.total_milliseconds += milliseconds;
        self.max_states = self.max_states.max(result.states_explored);
        self.max_nodes = self.max_nodes.max(result.nodes_reached);
        self.max_milliseconds = self.max_milliseconds.max(milliseconds);
        self.min_states = Some(match self.min_states {
            Some(least) => least.min(result.states_explored),
            None => result.states_explored,
        });

        match result.stopped_by {
            // MEMORY COUNTS AS A SIZE LIMIT rather than getting a third counter. The
            // report's question is "did this crawl run out of room or out of time", and
            // both of these are room - one measured in states and one in bytes, which is
            // the same limit expressed in a better unit (de-e23q). A reader who needs to
            // tell them apart has the stopped_by on the result itself.
            LookAheadLimit::States | LookAheadLimit::Memory => self.stopped_by_states += 1,
            LookAheadLimit::Time => self.stopped_by_time += 1,
            LookAheadLimit::None => {}
        }
    }

    /// How many crawls stopped early, by either limit.
    pub fn stopped_early(&self) -> u64 {
        self.stopped_by_states + self.stopped_by_time
    }

    pub fn mean_states(&self) -> f64 {
        if self.crawls == 0 { 0.0 } else { self.total_states as f64 / self.crawls as f64 }
    }

    pub fn mean_milliseconds(&self) -> f64 {
        if self.crawls == 0 { 0.0 } else { self.total_milliseconds / self.crawls as f64 }
    }
}

/// What a whole run of crawls cost.
#[derive(Debug, Clone, Default)]
pub struct LookAheadStatistics {
    pub overall: Tally,
    /// How many crawls fell in each state-count bucket, one longer than
    /// [`BUCKET_BOUNDS`] - the extra entry is everything above the top bound.
    pub buckets: Vec<u64>,
    pub found_nothing: u64,
    pub found_unseen_this_game: u64,
    pub found_unseen_any_game: u64,
    /// Per-conversation totals, keyed by the conversation a crawl BEGAN in.
    pub by_conversation: HashMap<i32, Tally>,
}

impl LookAheadStatistics {
    pub fn new() -> Self {
        Self { buckets: vec![0; BUCKET_BOUNDS.len() + 1], ..Default::default() }
    }

    /// Records one crawl.
    pub fn record(&mut self, start: DialogueNodeId, result: &LookAheadResult, milliseconds: f64) {
        if self.buckets.is_empty() {
            self.buckets = vec![0; BUCKET_BOUNDS.len() + 1];
        }

        self.overall.record(result, milliseconds);
        self.by_conversation
            .entry(start.conversation_id)
            .or_default()
            .record(result, milliseconds);

        self.buckets[bucket_of(result.states_explored)] += 1;
        match result.best {
            Novelty::UnseenAnyGame => self.found_unseen_any_game += 1,
            Novelty::UnseenThisGame => self.found_unseen_this_game += 1,
            Novelty::SeenThisGame => self.found_nothing += 1,
        }
    }

    /// The conversations that were crawled, in a stable order.
    ///
    /// Sorted, because a report is read against a previous one and a hash map's order is
    /// not the same twice.
    pub fn conversations(&self) -> Vec<i32> {
        let mut ids: Vec<i32> = self.by_conversation.keys().copied().collect();
        ids.sort_unstable();
        ids
    }
}

/// The cost report, for a measurement to print.
///
/// REPORTING IS NOT ASSERTING, which is the point this was split off from. The in-game
/// harness had the only cost report and it FAILED when no crawl had run, because the suite
/// that owned it demanded a cost. That is wrong for a general measurement: "no crawls" is a
/// legitimate result to print, it is exactly what a fully-read profile produces, and it is
/// the correct answer rather than a fault. This renders whatever it was given, including
/// nothing.
impl fmt::Display for LookAheadStatistics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.overall.crawls == 0 {
            return writeln!(f, "no crawls ran");
        }

        writeln!(
            f,
            "{} crawls: {} states worst, {:.0} mean; {:.0}ms worst, {:.1}ms mean",
            self.overall.crawls,
            self.overall.max_states,
            self.overall.mean_states(),
            self.overall.max_milliseconds,
            self.overall.mean_milliseconds(),
        )?;

        writeln!(
            f,
            "  stopped early: {} on states, {} on time ({} finished)",
            self.overall.stopped_by_states,
            self.overall.stopped_by_time,
            self.overall.crawls - self.overall.stopped_early(),
        )?;

        writeln!(
            f,
            "  found: {} nothing, {} unseen this save, {} unseen anywhere",
            self.found_nothing, self.found_unseen_this_game, self.found_unseen_any_game,
        )?;

        // THE HISTOGRAM IS WHAT MAKES THE TAIL VISIBLE. A mean is useless for spotting the
        // one menu in a thousand that costs a hundred times the rest, and that menu is the
        // whole reason a budget exists.
        writeln!(f, "  states:")?;
        for (bucket, &count) in self.buckets.iter().enumerate() {
            if count > 0 {
                writeln!(f, "    {:>12}  {count}", bucket_label(bucket))?;
            }
        }

        let conversations = self.conversations();
        if conversations.len() > 1 {
            writeln!(f, "  by conversation:")?;
            writeln!(
                f,
                "    {:>6} {:>7} {:>10} {:>10} {:>9} {:>7}",
                "conv", "crawls", "max states", "mean", "max ms", "stopped",
            )?;
            for id in conversations {
                let tally = &self.by_conversation[&id];
                writeln!(
                    f,
                    "    {id:>6} {:>7} {:>10} {:>10.0} {:>9.0} {:>7}",
                    tally.crawls,
                    tally.max_states,
                    tally.mean_states(),
                    tally.max_milliseconds,
                    tally.stopped_early(),
                )?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(states: usize, nodes: usize, stopped_by: LookAheadLimit, best: Novelty)
        -> LookAheadResult
    {
        LookAheadResult { best, states_explored: states, nodes_reached: nodes, stopped_by, trace: None }
    }

    #[test]
    fn buckets_cover_their_bounds_inclusively() {
        assert_eq!(bucket_of(0), 0);
        assert_eq!(bucket_of(10), 0);
        assert_eq!(bucket_of(11), 1);
        assert_eq!(bucket_of(100), 1);
        assert_eq!(bucket_of(100_000), 4);
        assert_eq!(bucket_of(100_001), 5);
    }

    #[test]
    fn bucket_labels_read_as_ranges() {
        assert_eq!(bucket_label(0), "0-10");
        assert_eq!(bucket_label(1), "11-100");
        assert_eq!(bucket_label(5), "100001+");
    }

    #[test]
    fn an_empty_run_has_no_minimum_rather_than_a_huge_one() {
        let stats = LookAheadStatistics::new();
        assert_eq!(stats.overall.min_states, None);
        assert_eq!(stats.overall.mean_states(), 0.0);
        assert_eq!(stats.overall.mean_milliseconds(), 0.0);
    }

    #[test]
    fn the_two_limits_partition_the_stopped_crawls() {
        let mut stats = LookAheadStatistics::new();
        let start = DialogueNodeId::new(1, 0);
        stats.record(start, &result(5, 2, LookAheadLimit::States, Novelty::SeenThisGame), 1.0);
        stats.record(start, &result(5, 2, LookAheadLimit::Time, Novelty::SeenThisGame), 1.0);
        stats.record(start, &result(5, 2, LookAheadLimit::None, Novelty::SeenThisGame), 1.0);

        assert_eq!(stats.overall.stopped_by_states, 1);
        assert_eq!(stats.overall.stopped_by_time, 1);
        // No double counting: the two are disjoint and sum to the stopped crawls.
        assert_eq!(stats.overall.stopped_early(), 2);
        assert_eq!(stats.overall.crawls, 3);
    }

    #[test]
    fn totals_and_extremes_accumulate() {
        let mut stats = LookAheadStatistics::new();
        let start = DialogueNodeId::new(7, 0);
        stats.record(start, &result(10, 3, LookAheadLimit::None, Novelty::UnseenAnyGame), 2.0);
        stats.record(start, &result(90, 8, LookAheadLimit::None, Novelty::UnseenThisGame), 4.0);

        let t = &stats.overall;
        assert_eq!(t.crawls, 2);
        assert_eq!(t.total_states, 100);
        assert_eq!(t.max_states, 90);
        assert_eq!(t.min_states, Some(10));
        assert_eq!(t.total_nodes, 11);
        assert_eq!(t.max_nodes, 8);
        assert_eq!(t.max_milliseconds, 4.0);
        assert_eq!(t.mean_states(), 50.0);
        assert_eq!(t.mean_milliseconds(), 3.0);
        assert_eq!(stats.found_unseen_any_game, 1);
        assert_eq!(stats.found_unseen_this_game, 1);
        assert_eq!(stats.found_nothing, 0);
        assert_eq!(stats.buckets[0], 1); // 10 states
        assert_eq!(stats.buckets[1], 1); // 90 states
    }

    /// The per-conversation rows are the same type as the overall one, so they carry
    /// everything the overall one does - the C# rows have neither node counts nor a
    /// minimum.
    #[test]
    fn each_conversation_gets_a_full_tally_of_its_own() {
        let mut stats = LookAheadStatistics::new();
        stats.record(
            DialogueNodeId::new(1, 0),
            &result(10, 4, LookAheadLimit::None, Novelty::SeenThisGame),
            1.0,
        );
        stats.record(
            DialogueNodeId::new(2, 0),
            &result(20, 6, LookAheadLimit::Time, Novelty::SeenThisGame),
            5.0,
        );

        assert_eq!(stats.conversations(), vec![1, 2]);
        let one = &stats.by_conversation[&1];
        assert_eq!(one.crawls, 1);
        assert_eq!(one.min_states, Some(10));
        assert_eq!(one.max_nodes, 4);
        assert_eq!(one.stopped_early(), 0);

        let two = &stats.by_conversation[&2];
        assert_eq!(two.max_states, 20);
        assert_eq!(two.stopped_by_time, 1);
        assert_eq!(two.max_milliseconds, 5.0);
    }

    /// Every crawl lands in exactly one of the three tallies, so they sum to the count.
    #[test]
    fn what_was_found_is_tallied_by_novelty() {
        let mut stats = LookAheadStatistics::new();
        let start = DialogueNodeId::new(1, 1);

        for best in [
            Novelty::UnseenAnyGame,
            Novelty::UnseenAnyGame,
            Novelty::UnseenThisGame,
            Novelty::SeenThisGame,
        ] {
            stats.record(start, &result(1, 1, LookAheadLimit::None, best), 1.0);
        }

        assert_eq!(stats.found_unseen_any_game, 2);
        assert_eq!(stats.found_unseen_this_game, 1);
        assert_eq!(stats.found_nothing, 1);

        let tallied = stats.found_unseen_any_game + stats.found_unseen_this_game
            + stats.found_nothing;
        assert_eq!(tallied, stats.overall.crawls);
    }

    /// The histogram is the point of keeping statistics at all: a mean hides the one menu
    /// in a thousand that costs a hundred times the rest.
    #[test]
    fn buckets_separate_the_tail_from_the_bulk() {
        let mut stats = LookAheadStatistics::new();
        let start = DialogueNodeId::new(1, 1);

        for _ in 0..99 {
            stats.record(start, &result(5, 1, LookAheadLimit::None, Novelty::SeenThisGame), 0.1);
        }
        stats.record(
            start,
            &result(50_000, 1, LookAheadLimit::None, Novelty::SeenThisGame),
            500.0,
        );

        assert_eq!(stats.buckets[0], 99);
        assert_eq!(stats.buckets[4], 1);
        assert_eq!(stats.overall.max_states, 50_000);

        // And the mean is the thing the histogram exists to contradict: it lands near 505,
        // a number no single crawl came close to.
        let mean = stats.overall.mean_states();
        assert!(mean > 500.0 && mean < 510.0, "mean was {mean}");
    }
}
