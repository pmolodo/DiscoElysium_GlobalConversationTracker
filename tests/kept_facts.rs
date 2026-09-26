// SPDX-License-Identifier: MIT
//! The facts `index::facts` keeps on disk change only with their derivation number.
//!
//! A kept group's facts are keyed on its dialogue, so they outlive every rebuild of the engine -
//! which is right while the engine only reads them, and wrong the moment the code changes what
//! they come out as. The number that says so is bumped by hand, and a number bumped by hand is a
//! number that gets forgotten. What a forgotten one costs is quiet: a change to which variables
//! a node counts as reading renumbers the slots under files that still match their dialogue, a
//! player's game takes a live slot for an inert one and answers menus wrongly, and every offline
//! check - which works its facts out fresh - still passes.
//!
//! So this pins a fingerprint of the facts beside the number. Change what the facts hold and it
//! fails; bump `DERIVATION` in `src/index/facts.rs` and put the new fingerprint here, and every
//! file an older engine wrote is refused and worked out again.

use gct_measure::common;
use lookahead_engine::index::facts::{DERIVATION, GroupFacts};
use lookahead_engine::index::{build_group_graph, read_index};

/// Groups with facts worth having, Joyce's among them: one conversation from each.
const GROUPS: [i32; 9] = [13, 271, 346, 354, 631, 639, 1118, 1133, 1467];

/// The derivation these facts belong to, and what they fingerprint as.
const PINNED: (u32, u64) = (2, 0x9c7e_12ee_19a0_d2e5);

/// FNV-1a, which is stable across builds and toolchains where the standard hasher is not.
fn fnv1a(bytes: &[u8], mut hash: u64) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[test]
fn the_kept_facts_change_only_with_their_derivation() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let mut fingerprint = 0xcbf2_9ce4_8422_2325;
    for conversation in GROUPS {
        let (graph, _) = build_group_graph(&index, conversation).expect("the group builds");
        let mut inert_slots: Vec<usize> = graph.inert_slots().iter().copied().collect();
        inert_slots.sort_unstable();
        let facts = GroupFacts {
            inert_slots,
            settled: graph.settled_candidates().clone(),
        };
        let bytes = bincode::serialize(&facts).expect("the facts serialise");
        fingerprint = fnv1a(&bytes, fingerprint);
    }

    assert_eq!(
        (DERIVATION, fingerprint),
        PINNED,
        "the facts a group implies have changed, to {fingerprint:#x}. Bump DERIVATION in \
         src/index/facts.rs so that files an older engine wrote are refused, and pin the new \
         number beside {fingerprint:#x} here",
    );
}
