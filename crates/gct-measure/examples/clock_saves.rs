// SPDX-License-Identifier: MIT
//! Whether any committed save has been through a menu in a group that carries a clock.
//!
//! A crawl only runs where an option is NOT itself the top rung - so the save has to have
//! displayed the menu's options already, and what a save has displayed is in the save. This
//! asks every committed save about every group that carries a clock, which is what decides
//! whether an in-game scenario exercising a carried clock can be built from what is here.

use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::world::GameWorld;

use gct_measure::common;
use gct_measure::common::fixtures;

/// The groups that carry a clock, as `clock_groups` found them.
const CARRIERS: [i32; 9] = [14, 17, 368, 381, 566, 631, 827, 1030, 1260];

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index, and nothing can build one here");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let unlocked = GameWorld::blank().with_clock_locked(false);
    let saves = fixtures::committed_saves();

    println!(
        "{} committed saves against {} carriers",
        saves.len(),
        CARRIERS.len()
    );
    println!();

    let mut any = false;
    for carrier in CARRIERS {
        let Ok((graph, conversations)) = build_group_graph(&index, carrier) else {
            eprintln!("{carrier}: no group builds from it");
            continue;
        };
        if !DataLayout::clock_can_move(&graph, &unlocked) {
            eprintln!("{carrier}: carries no clock after all");
            continue;
        }
        let mut group: Vec<i32> = conversations.iter().copied().collect();
        group.sort_unstable();

        for save in &saves {
            let read = fixtures::read_in_save_group(save, &group);
            if read.is_empty() {
                continue;
            }
            any = true;
            println!(
                "  group {carrier}: {save} has displayed {} entr(y/ies) in it",
                read.len()
            );
        }
    }

    println!();
    if any {
        println!("so an in-game scenario over a carried clock can be built from what is here");
    } else {
        println!(
            "NO committed save has displayed anything in any group that carries a clock, so no \
             crawl can be made to run in one without a save from a real playthrough"
        );
    }
}
