// SPDX-License-Identifier: MIT
//! Does a layout narrowed to where the query starts answer what the whole group answers?
//!
//! ## What is at risk
//!
//! de-3x76.8 narrows the layout to what the request's conversations can STRUCTURALLY reach,
//! which drops the slots read only by guards further out - 241 variables to 124 on
//! conversation 368. That is a performance change and must not be an ANSWER change.
//!
//! The safety argument is that structural reachability - links followed, guards ignored -
//! over-approximates what any search can walk, so a slot the search could read is never
//! dropped. THIS IS THE TEST OF THAT ARGUMENT, and it is worth having as a test rather than
//! as a paragraph because the failure mode is silent: a dropped slot does not crash, it
//! makes a guard undecidable and a marker quietly wrong.
//!
//! ## Why the existing tests do not cover it
//!
//! `workspace_agreement` compares the workspace against `bridge::answer`, and since this
//! change BOTH narrow - so the two agree while both being wrong together. `reference_oracle`
//! is the real guard but runs over small groups, where there is nothing beyond the starting
//! conversation to drop. This asks the question on the big groups, where the narrowing
//! actually removes something.

use std::collections::HashSet;

use lookahead_engine::bridge::{
    COUNTER_CAP, LookAheadAnswer, LookAheadRequest, NodeRef, SnapshotWorld, WireValue,
    WorldSnapshot, answer_starts, entered_at_of, questions_for,
};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

mod common;

/// The groups where the narrowing actually removes something.
///
/// 368 loses half its variables to it and 14 loses a fifth
/// (`performance/start_relative_layout.rs`), so these are where a dropped slot would show.
/// A group the narrowing does not shrink would pass this test without exercising it.
const GROUPS: [i32; 3] = [368, 14, 631];

/// How many starts to ask about per group.
const STARTS: usize = 6;

#[test]
fn a_narrowed_layout_answers_what_the_whole_group_answers() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let mut compared = 0;
    let mut narrowed_something = false;
    // Pairs where one side or the other did not finish, so nothing could be compared. Kept
    // as a number rather than ignored, because a run that skips most of its pairs proves
    // much less than its passing status suggests - and there is no other sign of that.
    let mut unfinished = 0;

    for conversation in GROUPS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let Ok(questions) = questions_for(&index, conversation) else {
            continue;
        };

        // STARTS FROM THE NAMED CONVERSATION ONLY, which is the shape the plugin sends and
        // the shape the narrowing is for. Starts scattered across the group would widen
        // `entered_at` back to the whole thing and test nothing.
        let mut starts: Vec<NodeRef> = graph
            .nodes()
            .map(|node| NodeRef::from(node.id))
            .filter(|node| node.conversation == conversation)
            .collect();
        starts.sort_by_key(|node| (node.conversation, node.entry));
        starts.truncate(STARTS);
        if starts.is_empty() {
            continue;
        }

        // Something unread to look for, or every answer is the same and proves nothing.
        // FORTY ENTRIES OUTSIDE THE ASKED CONVERSATION ARE UNREAD ANYWHERE, so what some
        // playthrough showed is everything else. Stated as the set the request carries.
        let unread: HashSet<NodeRef> = graph
            .nodes()
            .map(|node| NodeRef::from(node.id))
            .filter(|node| node.conversation != conversation)
            .take(40)
            .collect();
        let seen_anywhere: HashSet<NodeRef> = graph
            .nodes()
            .map(|node| NodeRef::from(node.id))
            .filter(|node| !unread.contains(node))
            .collect();

        let world = WorldSnapshot {
            day_minutes: 720,
            day_counter: 1,
            variables: questions
                .variables
                .iter()
                .map(|name| (name.clone(), WireValue::Unknown))
                .collect(),
            ..Default::default()
        };

        let request = LookAheadRequest {
            conversation,
            starts: starts.clone(),
            seen_any_game: seen_anywhere.iter().copied().collect(),
            world,
            ..Default::default()
        };

        let entered_at = entered_at_of(&request);
        let whole = DataLayout::for_group(&graph, &declaring(&request), COUNTER_CAP);
        let narrow = DataLayout::for_group_entered_at(
            &graph,
            &declaring(&request),
            COUNTER_CAP,
            Some(&entered_at),
        );

        // The premise. If these are equal the group is not exercising the narrowing, and a
        // passing comparison below would be worth nothing.
        if narrow.total_vars() < whole.total_vars() {
            narrowed_something = true;
        }

        let with_whole = answers_using(&graph, &request, whole);
        let with_narrow = answers_using(&graph, &request, narrow);

        let (Some(with_whole), Some(with_narrow)) = (with_whole, with_narrow) else {
            // The machine could not supply a manager for one of them; nothing to compare.
            continue;
        };

        assert_eq!(
            with_whole.len(),
            with_narrow.len(),
            "conversation {conversation}: a different number of answers",
        );

        for (wide, narrow) in with_whole.iter().zip(with_narrow.iter()) {
            // AN INCOMPLETE PASS IS NOT AN ANSWER, so there is nothing here to compare.
            //
            // What this test is about is whether narrowing changes the ANSWER. A search
            // that ran out of budget did not produce one - `best` is a lower bound and
            // `stopped_by` says which ration ran out - so asserting equality would be
            // comparing an answer with a non-answer, and it would fail with a message
            // showing two different-looking results, which reads exactly like a genuine
            // disagreement. de-x8ms.8: that is what it did, on conversation 14, when the
            // suite ran while something else on the machine was holding memory.
            //
            // AND IT WOULD FORBID THE IMPROVEMENT THE NARROWING EXISTS FOR. The narrowed
            // layout carries fewer variables, so it is CHEAPER, so it is expected to finish
            // where the whole-group one cannot. "Whole incomplete, narrow complete" is the
            // narrowing working, not a defect - and the old comparison called it a failure
            // because `same` requires `complete` and `stopped_by` to match.
            if !wide.complete || !narrow.complete {
                unfinished += 1;
                continue;
            }

            assert!(
                same(wide, narrow),
                "conversation {conversation}: the whole-group layout answered {wide:?} \
                 where the narrowed one answered {narrow:?}",
            );
            compared += 1;
        }
    }

    // SAID OUT LOUD WHEN IT HAPPENS. Skipped pairs are invisible in a passing run otherwise,
    // and the number is how a reader tells "this proved a lot" from "this proved one thing
    // and skipped the rest because the machine was busy".
    if unfinished > 0 {
        println!(
            "{unfinished} pair(s) skipped: one side or the other ran out of budget, so there \
             was no answer to compare. {compared} pair(s) were compared.",
        );
    }

    assert!(
        compared > 0,
        "nothing was compared, so this test proves nothing"
    );
    assert!(
        narrowed_something,
        "no group's layout actually got smaller, so the comparison above never exercised \
         the narrowing - check the groups named at the top still span more than one \
         conversation",
    );
}

/// The request's world, as a world.
fn declaring(request: &LookAheadRequest) -> SnapshotWorld {
    SnapshotWorld::declaring(request.world.clone(), common::declared())
}

/// The answers one layout produces, or `None` where no manager could be had for it.
///
/// ON A THREAD OF ITS OWN, and that is not optional here. This test builds TWO managers to
/// compare two layouts, and de-fpax is that a second manager on a thread which has already
/// held one overflows the stack while releasing the first - which is exactly what happened
/// when both were built inline (it passed alone and overflowed under `cargo test`, where
/// the binary had already done other work). `symbolic::isolated` is the repository's answer
/// to that: one manager per thread, and a large stack because releasing a diagram recurses.
fn answers_using(
    graph: &LookAheadGraph,
    request: &LookAheadRequest,
    layout: DataLayout,
) -> Option<Vec<LookAheadAnswer>> {
    isolated::on_its_own_thread(|| answers_on_this_thread(graph, request, layout))
}

/// The body of [`answers_using`], which must run on the thread that built its manager.
fn answers_on_this_thread(
    graph: &LookAheadGraph,
    request: &LookAheadRequest,
    layout: DataLayout,
) -> Option<Vec<LookAheadAnswer>> {
    let symbols = graph.symbols().clone();
    let world = declaring(request);
    let vars = DataVars::try_new(&layout, &symbols, request.diagram_budget())?;
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, &world, &vars).expect("room for a seed");

    // THE ONE RULE, as `bridge::answer` asks it - see `world::seen_state`. A copy here would be
    // a second way of deciding it, and this test exists to agree with the bridge.
    let seen_any_game = |id: DialogueNodeId| request.seen_any_game.contains(&NodeRef::from(id));
    let seen_state = lookahead_engine::world::seen_states(&world, seen_any_game);

    Some(answer_starts(
        graph,
        &world,
        request,
        &seen_state,
        &mut compiler,
        &seed,
    ))
}

/// Every field of an answer except the elapsed time, which is a stopwatch and not a claim.
fn same(a: &LookAheadAnswer, b: &LookAheadAnswer) -> bool {
    a.start == b.start
        && a.branch == b.branch
        && a.best == b.best
        && a.complete == b.complete
        && a.stopped_by == b.stopped_by
}
