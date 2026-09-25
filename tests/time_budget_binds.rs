// SPDX-License-Identifier: MIT
//! Does the time dial mean what the player is told it means?
//!
//! `LookAheadTimeBudgetMs` is documented as "the longest one option's look-ahead may run
//! for" (src/GlobalConversationTracker.Plugin/Plugin.cs), and until de-cluo it was not: every
//! ration was an estimate and none of them was a wall. Work done before the driver's own
//! clock started was spent outside the player's number entirely, and a candidate beginning a
//! millisecond under the limit was still allowed a whole `each`, so it returned a quarter of
//! a second past it. At the shipped default of 1000 the true worst case was about 1300 ms.
//!
//! ## Why this is a test about ARITHMETIC rather than about wall time
//!
//! A test that ran a real search and asserted it finished inside its budget would be a timing
//! test, and timing tests on a busy machine are how a suite starts failing for reasons that
//! are nobody's fault - this session watched exactly that happen to
//! `narrowed_layout_agreement` (de-x8ms.8). So what is checked here is the SHAPE of the
//! budget the dial produces: that the wall exists, that it is the player's number, and that
//! the rations are inside it rather than added to it.
//!
//! ## The MENU's dial, which is the same claim one level up
//!
//! `LookAheadMenuTimeBudgetMs` is documented as the longest a whole menu may take, and the
//! per-option dial bounds one turn of the loop that draws it. de-dt75.3: a menu's worst case
//! was the per-option number times twice the option count, because a rolled check is two
//! searches, and nothing anywhere bounded the sum. The tests for it are the same shape as
//! the ones above and for the same reason - the arithmetic that narrows each option's ration
//! to what is left of the menu, rather than a stopwatch on a real menu.

use lookahead_engine::bridge::LookAheadRequest;

/// The dial's number is the wall, and the parts are inside it.
#[test]
fn the_dial_becomes_an_overall_deadline() {
    let request = LookAheadRequest {
        time_budget_ms: 1000,
        ..Default::default()
    };
    let budget = request.search_budget();

    assert_eq!(
        budget.overall,
        std::time::Duration::from_millis(1000),
        "the player's number should be the wall itself",
    );

    // THE PARTS MUST NOT EXCEED THE WHOLE. This is the property that was broken: a ration
    // could be spent outside the number rather than inside it.
    assert!(
        budget.backwards <= budget.overall,
        "the driver is spent out of the wall, not before it",
    );
    assert!(
        budget.each <= budget.overall,
        "a candidate cannot be allowed longer than the whole answer",
    );
}

/// A dial smaller than the shipped rations squeezes them rather than being ignored.
///
/// The interesting direction: at 10 ms a ration sized for a whole second is larger than the
/// answer is allowed to be, and a candidate is handed the dial rather than a ration of its
/// own.
#[test]
fn a_small_dial_squeezes_the_rations_it_is_smaller_than() {
    let request = LookAheadRequest {
        time_budget_ms: 10,
        ..Default::default()
    };
    let budget = request.search_budget();

    let ten = std::time::Duration::from_millis(10);
    assert_eq!(budget.overall, ten);
    assert!(
        budget.backwards <= ten,
        "a ration sized for a second cannot fit in a 10ms answer"
    );
    assert!(
        budget.each <= ten,
        "a candidate cannot outlast a 10ms answer"
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
    let request = LookAheadRequest {
        state_budget: 3,
        ..Default::default()
    };
    let budget = request.search_budget();

    assert!(!budget.overall.is_zero(), "the attempt still gets a clock");
    assert!(budget.each.is_zero(), "but no candidate may finish");
}

/// An unset menu dial is no wall at all, which is what zero means on both time settings.
///
/// `Duration::MAX` rather than an `Option`, because it is the identity of the `min` the wall
/// is spent through - so "no wall" takes the same line as a wall of three seconds.
#[test]
fn an_unset_menu_dial_bounds_nothing() {
    let none = LookAheadRequest::default();
    assert_eq!(
        none.menu_budget(),
        std::time::Duration::MAX,
        "zero is no menu wall"
    );

    let set = LookAheadRequest {
        menu_time_budget_ms: 3000,
        ..Default::default()
    };
    assert_eq!(set.menu_budget(), std::time::Duration::from_secs(3));
}

/// An option's ration is narrowed to what is left of the menu, not added to it.
///
/// The property the whole wall rests on: at the last option of a menu that has nearly run
/// out, the ration handed to the search is the REMAINDER rather than the player's per-option
/// number, so the menu ends at its wall rather than one option's budget past it.
#[test]
fn what_is_left_of_the_menu_narrows_an_options_ration() {
    let request = LookAheadRequest {
        time_budget_ms: 1000,
        ..Default::default()
    };
    let budget = request.search_budget();

    let nearly_spent = budget.within(std::time::Duration::from_millis(20));
    assert_eq!(
        nearly_spent.overall,
        std::time::Duration::from_millis(20),
        "an option cannot be allowed longer than the menu has left",
    );

    // AND A MENU WITH ROOM TO SPARE CHANGES NOTHING. The wall binds where it is the smaller
    // number and nowhere else, so an ordinary menu runs exactly as it did before there was
    // one - which is what makes the default safe to ship.
    let roomy = budget.within(std::time::Duration::from_secs(30));
    assert_eq!(
        roomy.overall, budget.overall,
        "a wall further out than the ration binds nothing"
    );
    assert_eq!(
        budget.within(std::time::Duration::MAX).overall,
        budget.overall,
        "no wall at all binds nothing",
    );
}

/// Only the wall moves, because every other ration is already narrowed against it.
///
/// Stated as a test rather than only as a comment: a later hand adding a clock to `Budget`
/// and not narrowing it where it is spent would make this pass while the wall leaked, so
/// what this pins is the SHAPE the narrowing relies on - one number binds the rest.
#[test]
fn narrowing_a_budget_moves_the_wall_and_leaves_the_estimates_alone() {
    let budget = LookAheadRequest {
        time_budget_ms: 1000,
        ..Default::default()
    }
    .search_budget();
    let narrowed = budget.within(std::time::Duration::from_millis(20));

    assert_eq!(
        narrowed.backwards, budget.backwards,
        "the driver keeps its estimate"
    );
    assert_eq!(narrowed.each, budget.each, "and so does a candidate");
    assert!(
        narrowed.overall < narrowed.each,
        "which is only safe because the wall is below them and they are spent against it",
    );
}

/// A marking's wall is the lesser of what the menu and the option each have left, and the
/// per-option dial is never multiplied into a menu total.
///
/// THE MENU MEASUREMENT BUILDS ITS WALL THROUGH THIS CALL, so a row is cut where a player's
/// menu is cut, where a copy of the rule would be free to give a menu the per-option dial
/// times its width.
#[test]
fn a_markings_wall_is_the_lesser_dial_left() {
    let ms = std::time::Duration::from_millis;
    let request = LookAheadRequest {
        time_budget_ms: 1000,
        menu_time_budget_ms: 3000,
        ..Default::default()
    };

    let fresh = request.marking_budget(ms(0), ms(0));
    assert_eq!(
        fresh.wall,
        ms(1000),
        "one option's dial, not one per option"
    );
    assert_eq!(fresh.each, request.search_budget().each);

    assert_eq!(
        request.marking_budget(ms(2500), ms(0)).wall,
        ms(500),
        "a menu nearly spent binds the option",
    );
    assert_eq!(
        request.marking_budget(ms(100), ms(400)).wall,
        ms(600),
        "and what the option spent comes off its own dial",
    );
    assert_eq!(
        LookAheadRequest::default()
            .marking_budget(ms(0), ms(0))
            .wall,
        std::time::Duration::MAX,
        "no dials, no wall",
    );
}
