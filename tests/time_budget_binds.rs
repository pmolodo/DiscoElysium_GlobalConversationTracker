// SPDX-License-Identifier: MIT
//! Does the time dial mean what the player is told it means?
//!
//! `LookAheadTimeBudgetMs` is documented as "the longest one option's look-ahead may run
//! for" (src/GlobalConversationTracker.Plugin/Plugin.cs), and until de-cluo it was not. Every
//! ration in the portfolio was an estimate and none of them was a wall:
//!
//! 1. THE FORWARD SLICE WAS OUTSIDE THE CLOCK. `portfolio::best_novelty` runs the slice and
//!    only then enters the backward driver, whose clock starts there - so the slice's fifty
//!    milliseconds were spent before the player's number began counting.
//! 2. A CANDIDATE COULD OVERRUN IT. The driver tested `began.elapsed() >= budget.time` at the
//!    top of its loop and then allowed the candidate a whole `each`, so one beginning a
//!    millisecond under the limit returned a quarter of a second past it.
//!
//! At the shipped default of 1000 the true worst case was about 50 + 1000 + 250 = 1300 ms.
//!
//! ## Why this is a test about ARITHMETIC rather than about wall time
//!
//! A test that ran a real search and asserted it finished inside its budget would be a timing
//! test, and timing tests on a busy machine are how a suite starts failing for reasons that
//! are nobody's fault - this session watched exactly that happen to
//! `narrowed_layout_agreement` (de-x8ms.8). So what is checked here is the SHAPE of the
//! budget the dial produces: that the wall exists, that it is the player's number, and that
//! the rations are inside it rather than added to it.

use lookahead_engine::bridge::LookAheadRequest;
use lookahead_engine::symbolic::portfolio;

/// The dial's number is the wall, and the parts are inside it.
#[test]
fn the_dial_becomes_an_overall_deadline() {
    let request = LookAheadRequest { time_budget_ms: 1000, ..Default::default() };
    let budget = request.search_budget();

    assert_eq!(
        budget.overall,
        std::time::Duration::from_millis(1000),
        "the player's number should be the wall itself",
    );

    // THE PARTS MUST NOT EXCEED THE WHOLE. This is the property that was broken: the slice
    // and one candidate could each be spent outside the number rather than inside it.
    assert!(
        budget.forwards <= budget.overall,
        "the forward slice is spent out of the wall, not before it",
    );
    assert!(
        budget.each <= budget.overall,
        "a candidate cannot be allowed longer than the whole answer",
    );
}

/// A dial smaller than the shipped rations squeezes them rather than being ignored.
///
/// The interesting direction: at 10 ms the 50 ms slice is larger than the whole answer is
/// allowed to be, and a candidate is handed the dial rather than a ration of its own.
#[test]
fn a_small_dial_squeezes_the_rations_it_is_smaller_than() {
    let request = LookAheadRequest { time_budget_ms: 10, ..Default::default() };
    let budget = request.search_budget();

    let ten = std::time::Duration::from_millis(10);
    assert_eq!(budget.overall, ten);
    assert!(budget.forwards <= ten, "a 50ms slice cannot fit in a 10ms answer");
    assert!(budget.each <= ten, "a candidate cannot outlast a 10ms answer");
}

/// The default carries a wall too, so a request that names no budget is bounded.
#[test]
fn the_default_budget_has_a_wall_covering_its_own_rations() {
    let default = portfolio::Budget::default();

    assert!(
        default.overall >= default.forwards + default.backwards,
        "the default wall should cover the work the default rations describe, or the shape \
         of a default search changes the moment the wall is enforced",
    );
}

/// The starve knob stops a candidate finishing, rather than stopping the attempt.
///
/// `state_budget` is test-only and exists so an in-game suite can watch a search give up.
/// It works by allowing no time per candidate while leaving the attempt itself running: a
/// wall of zero would stop the loop before its first candidate, giving up without ever
/// running a pass, which is a different failure from the one the setting provokes.
#[test]
fn the_starve_knob_stops_a_candidate_rather_than_the_attempt() {
    let request = LookAheadRequest { state_budget: 3, ..Default::default() };
    let budget = request.search_budget();

    assert!(!budget.overall.is_zero(), "the attempt still gets a clock");
    assert!(budget.each.is_zero(), "but no candidate may finish");
}
