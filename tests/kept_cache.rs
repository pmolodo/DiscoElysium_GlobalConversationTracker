// SPDX-License-Identifier: MIT
//! Does what a measurement keeps agree with what this build derives?
//!
//! ## Why a test and not an argument
//!
//! The matrix driver runs one process per group and every one of them used to read a sixteen
//! megabyte index, build the group's graph and build its world from the save - the same answers
//! every time, in 521 processes per pass. Those answers are now kept under the build output and
//! read back, which takes a whole-game pass from about 150 seconds of work to about 25. See
//! de-9z1u, `performance/kept.rs` and `performance/prepared.rs`.
//!
//! A CACHE NOTHING VERIFIES IS A CACHE NOBODY SHOULD TRUST, and this one sits underneath every
//! performance number the project produces. The failure it guards against is silent by
//! construction: a stale value does not crash, it reports numbers for a graph or a world the
//! current code would not build, and nothing about the run looks unusual.
//!
//! ## What this checks, and in what order
//!
//! FIRST, THAT IT IS KEPT AT ALL. A second `Shipped` asks for the same group and never reads the
//! index - `took` stays at zero - which can only happen if the graph came off the disk. A cache
//! that quietly stopped working would pass every agreement check ever written, because building
//! everything fresh always agrees with itself.
//!
//! THEN, THAT WHAT IS KEPT AGREES. `Caching::verifying` makes each kept value be derived again
//! and compared with what was read back, inside the code that reads it, and a disagreement
//! panics rather than being reported - see `prepared::verified` and `save_world::verified`.

use std::path::PathBuf;

use lookahead_engine::bridge::GameWorld;

mod common;

#[path = "../performance/prepared.rs"]
mod prepared;

#[path = "../performance/save_world.rs"]
mod save_world;

use prepared::Shipped;

/// A handful of groups, small and large: 1494 builds 115 entries and 640 builds 4,064, so a
/// value whose size is what breaks it has somewhere to show.
const GROUPS: [i32; 2] = [1494, 640];

/// The whole thing in one test, because each stage rests on what the one before it left: the
/// cache has to be filled before a second reader can find it there, and read back before a
/// verifying reader can be shown checking it. Three tests would be three orders cargo is free
/// to choose between.
#[test]
fn what_is_kept_agrees_with_what_this_build_derives() {
    let Some(path) = common::shipped_index() else {
        return;
    };

    // FILLING IT, and whether this run derives anything is not the point and not asserted: the
    // cache outlives the process, so a second run of this test finds its own values already
    // there. What the test is about is what a process that has been told nothing gets.
    let filling = Shipped::at(path.clone(), prepared::Caching::default());
    for group in GROUPS {
        let graph = prepared::group_graph(&filling, group)
            .expect("the group builds")
            .graph;
        let world = save_world::of_save(&graph, group, &filling, save_world::TEMPLATE);
        assert!(
            !world.variables.is_empty(),
            "conversation {group}: a world from the template save answers no variables, so \
             there is nothing here to keep or to check"
        );
    }
    // READING IT BACK, in a process that has been told nothing. The index is read when the
    // first thing needs it, so a `took` of zero says nothing did.
    let kept = Shipped::at(path.clone(), prepared::Caching::default());
    for group in GROUPS {
        let held = prepared::group_graph(&kept, group).expect("the group builds");
        assert!(
            held.graph.count() > 0,
            "conversation {group}: a kept graph is empty"
        );
        assert!(
            held.conversations.contains(&group),
            "conversation {group}: a kept group does not hold the conversation it is named for"
        );
        assert!(
            !held.content.is_empty(),
            "conversation {group}: a kept group has no content stamp, so nothing about the \
             dialogue could ever be kept for it"
        );
    }
    assert_eq!(
        kept.took(),
        std::time::Duration::ZERO,
        "a kept graph was not read back: the index was read, which only happens when something \
         had to be derived"
    );

    // CHECKING IT. Everything kept is derived again and compared where it is read - and with
    // verification on, the world's process-local memo is skipped, so every world handed out is
    // one that was checked.
    //
    // ASKED FOR ON THIS `Shipped` ALONE, which is why the two above are unaffected by it. It
    // used to be `std::env::set_var` - a global, process-wide, unsafe mutation, carrying a
    // safety note arguing that no other thread had started and that nothing had read the
    // variable since. See de-3dx9.4.4.
    let checked = Shipped::at(path, prepared::Caching::verifying());
    for group in GROUPS {
        let graph = prepared::group_graph(&checked, group)
            .expect("the group builds")
            .graph;
        let world = save_world::of_save(&graph, group, &checked, save_world::TEMPLATE);
        // A WORLD THAT ANSWERS, so that the comparison inside was over something. The walk a
        // measurement feeds this to refuses rather than guesses wherever it cannot decide.
        assert!(
            !world.variables.is_empty(),
            "conversation {group}: a checked world answers no variables"
        );
        let _ = GameWorld::declaring(world, save_world::declared());
    }
    assert!(
        checked.took() > std::time::Duration::ZERO,
        "a verifying run derives everything it was given, which means reading the index"
    );
}

/// The cache lives under the build output and nowhere else, since it is derived, it is large,
/// and it is invalidated by the very thing `target/` is invalidated by.
#[test]
fn nothing_is_kept_in_the_repository() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for stray in [
        "degct-cache",
        "performance/degct-cache",
        "tests/degct-cache",
    ] {
        assert!(
            !root.join(stray).exists(),
            "{stray} is in the repository; kept values belong under the build output"
        );
    }
}
