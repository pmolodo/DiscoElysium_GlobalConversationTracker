// SPDX-License-Identifier: MIT
//! EVERY INTEGRATION TEST THAT CAN SHARE A PROCESS, in one binary.
//!
//! Cargo makes a separate test binary out of every `tests/*.rs`, and each one links the whole
//! engine again. At 45 of them that is 45 link steps against a thirty-megabyte library for any
//! change at all, and it dominated what the suite cost: building from nothing took 2m40s, of
//! which the tests themselves ran in about a minute.
//!
//! So `autotests` is off in `Cargo.toml` and the files are named here instead, by `#[path]`
//! rather than by moving them: a test that reaches for a fixture beside it still finds it, and
//! `tests/properties.proptest-regressions` still sits next to the test it records.
//!
//! ## What this changes for a test, and it is not nothing
//!
//! Tests in one binary SHARE A PROCESS, and Cargo runs binaries one after another while a
//! binary runs its own tests in parallel. Two tests that were previously in different files
//! could not overlap; now they can. Anything a test leaves in a fixed place - a scratch folder,
//! an environment variable, a file under `.build` - has to be named so that two tests running
//! at once cannot collide. `convert_verb` is the pattern to copy: its scratch folder carries
//! both the process id and the test's own name.
//!
//! ## What cannot come in here
//!
//! A `#[global_allocator]` is one per binary, so `out_of_memory` and `manager_memory` keep
//! their own targets and are declared beside this one in `Cargo.toml`. They are the whole
//! exception list; anything else added to `tests/` belongs in the roll below, and a file that
//! is in neither place is built by nothing and run by nobody.
//!
//! ## Naming a single file's tests
//!
//! The file name is a module, so a filter carries it: `cargo test --test suite corpus::` runs
//! the whole of `tests/corpus.rs` and nothing else.

#[path = "all_seen.rs"]
mod all_seen;

#[path = "antipassive_checks.rs"]
mod antipassive_checks;

#[path = "branch_shapes.rs"]
mod branch_shapes;

#[path = "bridge_contract.rs"]
mod bridge_contract;

#[path = "check_branches.rs"]
mod check_branches;

#[path = "clock_lock.rs"]
mod clock_lock;

#[path = "clock_oracle.rs"]
mod clock_oracle;

#[path = "committed_diffs.rs"]
mod committed_diffs;

#[path = "committed_saves_expand.rs"]
mod committed_saves_expand;

#[path = "committed_saves_pack.rs"]
mod committed_saves_pack;

#[path = "committed_sparse_tables.rs"]
mod committed_sparse_tables;

#[path = "committed_stamps.rs"]
mod committed_stamps;

#[path = "committed_states.rs"]
mod committed_states;

#[path = "convert_verb.rs"]
mod convert_verb;

#[path = "counter_saturation.rs"]
mod counter_saturation;

#[path = "corpus.rs"]
mod corpus;

#[path = "engine_host.rs"]
mod engine_host;

#[path = "environment_table.rs"]
mod environment_table;

#[path = "every_test_file_is_run.rs"]
mod every_test_file_is_run;

#[path = "failing_passive_checks.rs"]
mod failing_passive_checks;

#[path = "folded_menu.rs"]
mod folded_menu;

#[path = "guard_coverage.rs"]
mod guard_coverage;

#[path = "guard_depth.rs"]
mod guard_depth;

#[path = "hardcore_prices.rs"]
mod hardcore_prices;

#[path = "iteration_order.rs"]
mod iteration_order;

#[path = "kept_cache.rs"]
mod kept_cache;

#[path = "kept_facts.rs"]
mod kept_facts;

#[path = "kim_case_offline.rs"]
mod kim_case_offline;

#[path = "main_hub.rs"]
mod main_hub;

#[path = "menu_oracle.rs"]
mod menu_oracle;

#[path = "modelling_gaps.rs"]
mod modelling_gaps;

#[path = "narrowed_layout_agreement.rs"]
mod narrowed_layout_agreement;

#[path = "nothing_answers_unknown.rs"]
mod nothing_answers_unknown;

#[path = "oxidd_smoke.rs"]
mod oxidd_smoke;

#[path = "packed_save_blob.rs"]
mod packed_save_blob;

#[path = "properties.rs"]
mod properties;

#[path = "reference_oracle.rs"]
mod reference_oracle;

#[path = "reputation_writes.rs"]
mod reputation_writes;

#[path = "request_agreement.rs"]
mod request_agreement;

#[path = "request_size.rs"]
mod request_size;

#[path = "scenario_suites.rs"]
mod scenario_suites;

#[path = "scene_queries.rs"]
mod scene_queries;

#[path = "shipped_index.rs"]
mod shipped_index;

#[path = "thought_effects.rs"]
mod thought_effects;

#[path = "time_budget_binds.rs"]
mod time_budget_binds;

#[path = "wire_schema.rs"]
mod wire_schema;

#[path = "workspace_agreement.rs"]
mod workspace_agreement;

#[path = "xp_read_write.rs"]
mod xp_read_write;
