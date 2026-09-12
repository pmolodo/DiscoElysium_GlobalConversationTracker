// SPDX-License-Identifier: MIT
//! Does the projection read a real save's dialogue statuses?
//!
//! The unit tests beside the report build their states by hand, which says nothing about
//! whether a SAVE can be read into one. These run over the committed scenarios, which are
//! the only saves a fresh clone has.
//!
//! ## What is checked, and what only a playthrough could check
//!
//! That a save projects into statuses at all, that the two shapes of save - expanded here,
//! packed in a real SaveGames folder - agree, and that a save which recorded nothing reads
//! as nothing rather than as a failure. WHETHER THE MOD'S OWN FILE IS THE UNION of some real
//! playthroughs is the question the tool exists for and it needs saves nobody can commit.

use std::path::{Path, PathBuf};

use gct_save_files::global_state::Status;
use gct_state_check::{NamedSave, compare, resolve_save, saves_in, statuses_in_save, written};

/// The committed save that has read the most, which is the one worth projecting.
const READ_A_LOT: &str = "at-trashcan";

/// And one that has read nothing, which is the empty case rather than an error.
const READ_NOTHING: &str = "at-garte";

/// How many entries `at-trashcan` is known to have displayed.
///
/// PINNED rather than merely non-zero. A projection that quietly stopped finding statuses -
/// a renamed field, a table half never walked - would still read as "some" against a
/// tolerant check, and this is the save every other test's expectations lean on.
const READ_IN_TRASHCAN: usize = 6826;

fn scenarios() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("testing")
        .join("scenarios")
}

#[test]
fn a_committed_save_projects_into_the_statuses_it_recorded() {
    let path = resolve_save(READ_A_LOT, &scenarios()).expect("the save is there");

    let state = statuses_in_save(&path).expect("it reads");

    assert_eq!(state.len(), READ_IN_TRASHCAN);
    assert!(
        state.conversations().count() > 1,
        "one save reads as {} conversations",
        state.conversations().count(),
    );
}

/// A save that has read nothing is empty, which is not the same as unreadable.
#[test]
fn a_save_that_recorded_nothing_reads_as_nothing() {
    let path = resolve_save(READ_NOTHING, &scenarios()).expect("the save is there");

    let state = statuses_in_save(&path).expect("it reads");

    assert_eq!(state.len(), 0);
}

/// A bare name is resolved against the directory; a path is taken as it stands.
#[test]
fn a_save_is_found_by_name_or_by_path() {
    let by_name = resolve_save(READ_A_LOT, &scenarios()).expect("by name");
    let by_path =
        resolve_save(&by_name.display().to_string(), Path::new("nowhere")).expect("by path");

    assert_eq!(by_name, by_path);
}

#[test]
fn a_name_nothing_matches_says_where_it_looked() {
    let why = resolve_save("no-such-save", &scenarios()).expect_err("refused");

    assert!(why.to_string().contains("no-such-save"), "{why}");
    assert!(why.to_string().contains("scenarios"), "{why}");
}

#[test]
fn the_committed_scenarios_are_listed_by_the_name_a_caller_would_type() {
    let found = saves_in(&scenarios());

    assert!(found.iter().any(|name| name == READ_A_LOT), "{found:?}");
    assert!(found.windows(2).all(|pair| pair[0] < pair[1]), "{found:?}");
}

/// And the whole run works end to end over saves a fresh clone has.
///
/// A save is trivially the union of itself and an empty one, which is the smallest true
/// comparison this can make without a playthrough.
#[test]
fn a_state_holding_one_saves_statuses_is_the_union_of_it_and_an_empty_one() {
    let read = statuses_in_save(&resolve_save(READ_A_LOT, &scenarios()).expect("there"))
        .expect("it reads");
    let empty = statuses_in_save(&resolve_save(READ_NOTHING, &scenarios()).expect("there"))
        .expect("it reads");

    let global = read.clone();
    let report = compare(
        &global,
        &[
            NamedSave {
                label: READ_A_LOT.to_string(),
                state: read,
            },
            NamedSave {
                label: READ_NOTHING.to_string(),
                state: empty,
            },
        ],
    )
    .expect("two saves are enough");

    assert!(report.passed(), "{}", written(&report, 10));
    assert_eq!(report.union_entries, READ_IN_TRASHCAN);
    assert!(report.in_global_only.is_empty());
    assert_eq!(report.exclusives[0].total, READ_IN_TRASHCAN);
    assert_eq!(report.exclusives[0].preserved, READ_IN_TRASHCAN);
    assert_eq!(report.exclusives[1].total, 0);
}

/// A status the save records is one the projection can name.
#[test]
fn what_a_save_recorded_reads_back_as_a_status_and_not_as_untouched() {
    let state = statuses_in_save(&resolve_save(READ_A_LOT, &scenarios()).expect("there"))
        .expect("it reads");

    let displayed: usize = state
        .conversations()
        .collect::<Vec<_>>()
        .iter()
        .map(|&conversation| state.entries_at(conversation, Status::WasDisplayed).len())
        .sum();

    assert!(
        displayed > 0,
        "nothing was displayed in a save that read 6826 entries"
    );
}
