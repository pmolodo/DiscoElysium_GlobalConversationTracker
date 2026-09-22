// SPDX-License-Identifier: MIT
//! Which save and which clock-carrying group could make a search actually explore.
//!
//! A menu is answered for nothing when every option is itself the top rung - unread anywhere -
//! because nothing can outrank it, and the engine refuses before building a state. So the
//! foothold an in-game scenario over a carried clock needs is a save that HAS displayed some
//! of the group's options, with content past them it has NOT displayed.
//!
//! This ranks the pairs by how much of each is there. It is a shortlist rather than a verdict:
//! the engine's shortcut may still settle a menu without exploring, which only a run can say.
//! See de-086x.

use std::collections::BTreeSet;

use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::world::GameWorld;

use gct_measure::common;
use gct_measure::common::fixtures;

/// The groups that carry a clock, as `clock_groups` found them.
const CARRIERS: [i32; 9] = [14, 17, 368, 381, 566, 631, 827, 1030, 1260];

/// A pair worth trying, and what makes it worth trying.
struct Footing {
    carrier: i32,
    save: String,
    /// Options the save has displayed, so they are not the top rung.
    seen_options: usize,
    /// Entries in the group the save has never displayed, which is what a search hunts for.
    unread: usize,
}

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index, and nothing can build one here");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let unlocked = GameWorld::blank().with_clock_locked(false);
    let saves = fixtures::committed_saves();

    let mut found: Vec<Footing> = Vec::new();
    for carrier in CARRIERS {
        let Ok((graph, conversations)) = build_group_graph(&index, carrier) else {
            continue;
        };
        if !DataLayout::clock_can_move(&graph, &unlocked) {
            continue;
        }

        let mut group: Vec<i32> = conversations.iter().copied().collect();
        group.sort_unstable();

        let options: BTreeSet<(i32, i32)> = graph
            .nodes()
            .filter(|node| node.choice)
            .map(|node| (node.id.conversation_id, node.id.entry_id))
            .collect();
        let entries = graph.nodes().count();

        for save in &saves {
            let displayed: BTreeSet<(i32, i32)> = fixtures::read_in_save_group(save, &group)
                .into_iter()
                .collect();
            if displayed.is_empty() {
                continue;
            }

            let seen_options = options.intersection(&displayed).count();
            let unread = entries - displayed.len().min(entries);
            if seen_options > 0 && unread > 0 {
                found.push(Footing {
                    carrier,
                    save: save.clone(),
                    seen_options,
                    unread,
                });
            }
        }
    }

    found.sort_by_key(|f| (std::cmp::Reverse(f.seen_options), f.carrier, f.save.clone()));

    println!(
        "{:>7} {:<26} {:>13} {:>8}",
        "carrier", "save", "seen options", "unread"
    );
    for footing in found.iter().take(20) {
        println!(
            "{:>7} {:<26} {:>13} {:>8}",
            footing.carrier, footing.save, footing.seen_options, footing.unread
        );
    }

    println!();
    println!(
        "{} pair(s) have a seen option and unread content beyond it",
        found.len()
    );
    if found.is_empty() {
        println!("so nothing here can make a search explore in a group that carries a clock");
    }
}
