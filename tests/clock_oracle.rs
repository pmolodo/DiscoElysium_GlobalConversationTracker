// SPDX-License-Identifier: MIT
//! The moving clock, judged by the executor that moves it in concrete states.
//!
//! ## What this adds to the tests either side of it
//!
//! Everything else that checks the carried clock compares the symbolic side against what a
//! test expected of it. This puts it beside `oracle`, which walks CONCRETE states through
//! `core::action` - and that advances `day_minutes` itself when the world's clock is unlocked.
//! A disagreement here is the two executors answering the same question differently, which is
//! the one thing "one algorithm by default, everywhere it runs" forbids.
//!
//! ## The shape
//!
//! Two options reaching the same entry, differing only in whether they pass a quarter of an
//! hour on the way, and something past that entry which only opens at seven. From a quarter to
//! seven the time-passing option gets there and the other does not - so the two executors have
//! to disagree about the OPTIONS while agreeing with each other, and the test says both.
//!
//! ## The world is unlocked on purpose
//!
//! That is an arm rather than the default. The plugin sends whatever the game says and the
//! corpus fixtures derive it from the save; a locked clock carries no register at all, so a
//! locked world here would be testing nothing.
//!
//! ## What this sees that `menu_oracle` does not
//!
//! `menu_oracle` carries the same menu and reaches the same verdict, and the two are not
//! redundant: they are sensitive to different halves of the image. The marking's answer comes
//! through the BACKWARD pass, so a defect in the forward image alone leaves it correct - the
//! pre-image still knows what a `PassTime` does. This intersects a FORWARD entry against those
//! backward sets, so a forward-only defect shows up here and nowhere else.
//!
//! Measured, rather than assumed: breaking `ActionImage::pass_time` alone fails this and
//! leaves `menu_oracle` green; breaking the `PassTime` arm of `pre_one` as well fails both.

use lookahead_engine::core::types::{DialogueNodeId, StartBranch};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::oracle;
use lookahead_engine::symbolic::backward::{Backward, SettledPass};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::{Reachability, seed_of};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::test_graph::{Entry, GraphBuilder, node};
use lookahead_engine::world::{GameWorld, ILookAheadWorld};

/// Where an incremented slot saturates; nothing here counts anything.
const CAP: i32 = 16;

/// A quarter to seven, which one `PassTime` turns into seven.
const QUARTER_TO: i32 = 6 * 60 + 45;

/// The entry that only opens once the hour has turned.
const BEHIND_THE_HOUR: i32 = 4;

/// Two options onto the same guarded entry, one of which passes time on the way.
fn menu() -> LookAheadGraph {
    GraphBuilder::new()
        .add(Entry::new(0).links(&[1, 2]))
        .add(Entry::new(1).player().script("PassTime()").links(&[3]))
        .add(Entry::new(2).player().links(&[3]))
        .add(Entry::new(3).guard("IsHour(7)").links(&[BEHIND_THE_HOUR]))
        .add(Entry::new(BEHIND_THE_HOUR))
        .build()
}

/// Whether a symbolic search entering at `option` can reach `target`.
///
/// The same two steps the driver takes: enter the option, then ask a backward pass whether the
/// target is reachable from what entering it left.
fn symbolic_reaches(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    option: DialogueNodeId,
    target: DialogueNodeId,
) -> bool {
    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_group(graph, world, CAP);
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
    let mut compiler = GuardCompiler::new(&vars).with_world(world);
    let seed = seed_of(graph, world, &vars).expect("room for a seed");

    let entered = Reachability::entry_states(
        graph,
        option,
        StartBranch::Either,
        &seed,
        &mut compiler,
        world,
        CAP as u32,
    )
    .expect("room to enter the option");

    let children = graph
        .get(option)
        .map(|node| node.links.clone())
        .unwrap_or_default();
    let backward = Backward::reaching(graph, target, &mut compiler, world, CAP as u32);
    children
        .iter()
        .any(|child| backward.reachable_from(*child, &entered))
}

#[test]
fn both_executors_agree_about_what_the_hour_opens() {
    let graph = menu();
    let world = GameWorld::blank()
        .with_day_minutes(QUARTER_TO)
        .with_clock_locked(false);

    assert!(
        DataLayout::for_group(&graph, &world, CAP).clock().is_some(),
        "this menu is about a clock, so the layout has to be carrying one",
    );

    let target = node(BEHIND_THE_HOUR);
    let mut concrete = Vec::new();
    for option in [node(1), node(2)] {
        let walked = oracle::walk(&graph, option, &world, CAP).reached(target);
        let searched = symbolic_reaches(&graph, &world, option, target);
        assert_eq!(
            searched, walked,
            "the two executors disagree about what entry {} reaches: the concrete walk says \
             {walked} and the symbolic search says {searched}",
            option.entry_id,
        );
        concrete.push(walked);
    }

    // AND THE OPTIONS HAVE TO DIFFER, or the agreement above is agreement about nothing: a
    // clock that never moved would have both of them failing to reach it, and both executors
    // would still agree.
    assert_eq!(
        concrete,
        vec![true, false],
        "the option that passes time should reach what the hour opens, and the one that does \
         not should not - if this fails the fixture stopped testing the clock",
    );
}
