// SPDX-License-Identifier: MIT
//! A world built the way the game builds one answers everything its guards ask.
//!
//! ## The rule this keeps
//!
//! de-m11s: Unknown is for things going wrong, not for things we did not model. A guard, an
//! action, a world query, a variable query - anything the engine asks about the world - answers
//! definitely during normal functioning. Unknown is left for a plugin that threw, and for a tool
//! that asks a deliberately permissive question.
//!
//! ## Why a test rather than the measurements that established it
//!
//! Each source was closed by instrumenting the engine and measuring all 429 groups: variables
//! (de-m11s.1), the checks a group can move (de-m11s.2), computed call arguments (de-m11s.4),
//! malformed calls (de-m11s.6), unanswered questions (de-m11s.7), reputations (de-m11s.8). Every
//! one of those runs was taken once, by hand, against a build that no longer exists.
//!
//! What made them true is not obvious from any one place in the code: it is `bridge::collect`
//! placing every call, the variable table answering every name, and the plugin servicing every
//! `DataRequest`. A change to any of the three could reopen a source, and the symptom in the game
//! would be a marker no play can clear - which nothing else reports.
//!
//! ## What it does NOT claim
//!
//! That Unknown is unreachable. `performance/permissive_census.rs` produces one deliberately, to
//! ask what is unreachable in EVERY save rather than in one; and a plugin that throws still
//! answers Unknown, which is the case the rule allows. This says only that a world built from the
//! shipped table and a committed save, asked what a group's guards ask, always answers.

mod common;

#[path = "../performance/prepared.rs"]
mod prepared;

#[path = "../performance/save_world.rs"]
mod save_world;

use lookahead_engine::core::guard_value::GuardValueKind;
use lookahead_engine::core::types::Ternary;
use lookahead_engine::world::{GameWorld, ILookAheadWorld};

use prepared::Shipped;

/// Groups chosen for the questions they ask rather than for size.
///
/// 631 is the group every measurement uses; 761 is the heaviest in the game and the one whose
/// guards reach furthest; 29 holds the Kim switch that de-m11s.2 was about; 767 asks the
/// reputation questions de-m11s.8 was about.
const GROUPS: [i32; 4] = [631, 761, 29, 767];

#[test]
fn a_world_from_a_save_answers_everything_its_group_asks() {
    let Some(path) = common::shipped_index() else {
        return;
    };
    let Some(declared) = common::variable_table() else {
        return;
    };

    let shipped = Shipped::at(path, prepared::Caching::default());
    let mut checked = 0;
    let mut unanswered: Vec<String> = Vec::new();

    for group in GROUPS {
        let Ok(built) = prepared::group_graph(&shipped, group) else {
            continue;
        };
        let mut graph = built.graph;

        // THE WORLD A MEASUREMENT USES, which is the one built the way the game builds one:
        // the committed template save, resolved against the questions this group asks.
        let raw = save_world::of_save(&graph, group, &shipped, save_world::TEMPLATE);
        let world = GameWorld::declaring(raw, declared.clone());
        // FITTED FIRST, as a request fits it - `check_settled` is what `passive_outcome`
        // reads, and it is decided here rather than when the graph is built.
        graph.fit(&lookahead_engine::graph::Fitting::read(&graph, &world));

        let questions = lookahead_engine::bridge::questions_of(&graph, built.conversations.clone());

        // EVERY QUERY ITS GUARDS ASK. The list is what the engine would hand the plugin, so
        // this asks exactly what a request asks.
        for query in &questions.queries {
            let answer = world.query(query, &[]);
            checked += 1;
            if answer.kind() == GuardValueKind::Unknown {
                unanswered.push(format!("{group}: query {query}"));
            }
        }

        // AND EVERY VARIABLE, which the table answers whether or not the save holds it.
        for name in &questions.variables {
            let answer = world.variable(name);
            checked += 1;
            if answer.kind() == GuardValueKind::Unknown {
                unanswered.push(format!("{group}: variable {name}"));
            }
        }

        // AND EVERY PASSIVE CHECK, through the one function that asks - including the ones
        // whose skill the group can move, which answer from the state the crawl started in
        // rather than Unknown. See `world::passive_outcome`.
        //
        // PASSIVE ONLY, because a ROLL has no single outcome to state: a red or white check is
        // two branches, and `questions.checks` carries it so the plugin can price it rather
        // than so the world can decide it.
        for node in graph.nodes() {
            if node.kind != lookahead_engine::core::types::DialogueCheckKind::Passive {
                continue;
            }
            checked += 1;
            if lookahead_engine::world::passive_outcome(node, &world) == Ternary::Unknown {
                unanswered.push(format!("{group}: passive check {}", node.id));
            }
        }
    }

    assert!(checked > 0, "no group was reachable, so nothing was asked");
    println!(
        "{checked} question(s) asked across {} group(s)",
        GROUPS.len()
    );
    assert_eq!(unanswered, Vec::<String>::new());
}
