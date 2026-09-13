# Environment variables

Every environment variable this project defines is prefixed **`DEGCT_`**, and a throwaway
script in a session scratchpad uses **`DEGCTT_`**. The rule and the reasoning are in
`CLAUDE.md`; this is the list.

## Read them through the helper for your language, not by name

The prefix is applied by a function so that a new variable is named right because there is no
other way to ask for one. Each language has the same four operations - read one of ours, ask
whether it is set, set one, and read a FOREIGN one under its own name.

| language | helper |
|---|---|
| Rust | `src/core/env.rs` - `env::var`, `env::is_set`, `env::number`, `env::pass`, `env::foreign` |
| Python | `tools/measurement_common.py` - `env`, `env_is_set`, `env_int`, `env_list`, `env_for_child` |
| bash | `tools/degct-env.sh` - `degct_env`, `degct_env_is_set`, `degct_env_set`, `degct_env_foreign` |
| PowerShell | `tools/DegctEnv.psm1` - `Get-DegctEnv`, `Test-DegctEnv`, `Set-DegctEnv` |
| C# | `src/GlobalConversationTracker.Core/DegctEnvironment.cs` - `Get`, `IsSet`, `Number`, `Set`, `Foreign` |

Every one of them takes the **bare** name: `env("CONVERSATION")` reads `DEGCT_CONVERSATION`.
They are all idempotent about an already-qualified name, because callers build names from both
halves and `DEGCT_DEGCT_CONVERSATION` would be unset, silently, and read as a default.

## What we do NOT rename

Variables somebody else owns keep their own spelling, and are read through the `foreign` door
so a call site says which of the two it means:

`PATH`, `CARGO_TARGET_DIR`, `NUMBER_OF_PROCESSORS`, and cargo's own `OUT_DIR` and `PROFILE`
inside `build.rs`.

## Why the prefix exists at all

`GROUPS` is a bash **built-in array** holding the current user's numeric group ids. Assigning
to it looks like it works - `set -x` shows the assignment with the right value - and every
later `"$GROUPS"` expands to `${GROUPS[0]}`, a gid. In this repository that arrives at a
measurement as a conversation id that does not exist, and the run refuses with `conversation
197609: no group builds from it`, pointing nowhere near the variable.

It cost time three separate times. The shell owns a long list of short generic names -
`GROUPS`, `IFS`, `HOME`, `LINES`, `COLUMNS`, `SECONDS`, `RANDOM`, `PWD`, `REPLY`,
`PIPESTATUS`, `UID`, `HOSTNAME` - drawn from exactly the vocabulary a measurement wants. A
prefix takes the whole class off the table rather than dodging them one at a time.

**In bash the rule covers locals too**, not only exported variables, because the collision
that started this was a local.

## The list

Generated from the code rather than maintained by hand: it greps the syntax that reads a
variable, in each language, so it says what the code actually asks for. Regenerate it when a
variable is added or removed.

| `DEGCT_ARMS` | `tools/measure-residue-arms.sh` |
| `DEGCT_BUDGET_MB` | `measurements/cache_split.rs`, `measurements/cache_split_menu.rs`, `measurements/cacheable_asks.rs`, `measurements/dead_quantify.rs`, `measurements/manager_reuse.rs`, `measurements/prune_on_menus.rs`, `measurements/relaxed_settle.rs`, `measurements/repeat_question.rs`, `measurements/search_residue.rs`, `measurements/settles_within.rs`, `tools/measure-residue-arms.sh` |
| `DEGCT_CENSUS` | `measurements/performance_matrix.rs`, `src/GlobalConversationTracker.Core/DegctEnvironment.cs`, `src/core/env.rs`, `tools/DegctEnv.psm1`, `tools/degct-env.sh`, `tools/measurement_common.py` |
| `DEGCT_CENSUS_ALL` | `measurements/performance_matrix.rs`, `tools/census-coverage.py` |
| `DEGCT_CENSUS_FILE` | `measurements/performance_matrix.rs`, `tools/measure-matrix.py` |
| `DEGCT_CENSUS_JOURNAL` | `measurements/symbolic_answers.rs` |
| `DEGCT_CENSUS_OUT` | `measurements/performance_matrix.rs`, `tools/measure-census.py` |
| `DEGCT_CENSUS_REUSE` | `tools/measure-matrix.py` |
| `DEGCT_CONSTANTS_ONLY` | `measurements/performance_matrix.rs` |
| `DEGCT_CONVERSATION` | `AGENTS.md`, `CLAUDE.md`, `measurements/README.md`, `measurements/backward_support.rs`, `measurements/candidate_recurrence.rs`, `measurements/dominance_share.rs`, `measurements/layout_shape.rs`, `measurements/manager_reuse.rs`, `measurements/menu_wall.rs`, `measurements/nodes_repeat.rs`, `measurements/performance_matrix.rs`, `measurements/row_overhead.rs`, `measurements/shared_symbolic.rs`, `measurements/symbolic_answers.rs`, `tests/modelling_gaps.rs`, `tests/reference_oracle.rs`, `tests/slice_and_cone.rs`, `tools/degct-env.sh`, `tools/measure-symbolic.sh` |
| `DEGCT_EACH_MS` | `measurements/cacheable_asks.rs` |
| `DEGCT_ENGINES` | `measurements/performance_matrix.rs`, `tools/measure-matrix.py` |
| `DEGCT_ESTIMATE_EVERY` | `tools/measure-matrix.py` |
| `DEGCT_FORWARD_MS` | `measurements/settles_within.rs` |
| `DEGCT_FRESH` | `measurements/workspace_menus.rs` |
| `DEGCT_GROUPS_ONLY` | `measurements/README.md`, `measurements/performance_matrix.rs`, `tools/measure-matrix.py` |
| `DEGCT_GUARD_SNAPSHOT` | `measurements/guard_snapshot.rs` |
| `DEGCT_HEADER_ONLY` | `measurements/performance_matrix.rs` |
| `DEGCT_LOG_DIR` | `tools/measure-symbolic.sh`, `tools/run-logged.sh` |
| `DEGCT_LONE` | `measurements/settles_within.rs` |
| `DEGCT_MATRIX_OUT` | `measurements/README.md`, `tools/degct-env.sh`, `tools/measure-matrix.py`, `tools/measurement_common.py` |
| `DEGCT_MEASUREMENT` | `tools/measure-symbolic.sh` |
| `DEGCT_MEMORY_BUDGET_MB` | `measurements/menu_residue.rs` |
| `DEGCT_MEMORY_HEADROOM` | `tools/measure-matrix.py` |
| `DEGCT_MENUS` | `measurements/cacheable_asks.rs`, `measurements/candidate_recurrence.rs` |
| `DEGCT_MENU_TIME_BUDGET_MS` | `measurements/menu_wall.rs` |
| `DEGCT_NOLIMIT` | `measurements/menu_matrix.rs` |
| `DEGCT_NO_HEADER` | `measurements/performance_matrix.rs` |
| `DEGCT_OUT` | `tools/degct-env.sh` |
| `DEGCT_PAST_RUNS` | `tools/measure-matrix.py` |
| `DEGCT_PER_GROUP` | `tools/measure-matrix.py` |
| `DEGCT_PROFILE` | `measurements/performance_matrix.rs`, `measurements/seen_profile.rs` |
| `DEGCT_PROFILES` | `measurements/cacheable_asks.rs`, `measurements/candidate_recurrence.rs`, `measurements/dominance_share.rs`, `measurements/performance_matrix.rs`, `tools/measure-matrix.py` |
| `DEGCT_PROGRESS_SECONDS` | `measurements/performance_matrix.rs`, `measurements/symbolic_answers.rs` |
| `DEGCT_PRUNING` | `measurements/shared_symbolic.rs` |
| `DEGCT_REPEATS` | `measurements/cache_split.rs`, `measurements/per_start_setup.rs`, `measurements/repeat_question.rs` |
| `DEGCT_ROUNDS` | `measurements/manager_reuse.rs`, `measurements/nodes_repeat.rs`, `measurements/row_overhead.rs`, `measurements/workspace_menus.rs` |
| `DEGCT_ROW_MEMORY_MB` | `measurements/performance_matrix.rs`, `tools/measure-matrix.py` |
| `DEGCT_ROW_OVERHEAD` | `tools/measure-matrix.py` |
| `DEGCT_ROW_SECONDS` | `measurements/cache_split.rs`, `measurements/performance_matrix.rs`, `tools/degct-env.sh`, `tools/measure-matrix.py` |
| `DEGCT_RUN_LOG_DIR` | `AGENTS.md`, `CLAUDE.md`, `DEVELOPING.md`, `tools/measure-symbolic.sh`, `tools/run-logged.sh` |
| `DEGCT_RUN_NAME` | `tools/measure-symbolic.sh` |
| `DEGCT_SAVE` | `tests/kim_case_offline.rs` |
| `DEGCT_SEARCHES` | `measurements/search_residue.rs`, `tools/measure-residue-arms.sh` |
| `DEGCT_SEARCH_MS` | `measurements/repeat_question.rs` |
| `DEGCT_SERIAL_GROUPS` | `measurements/README.md`, `tools/measure-matrix.py` |
| `DEGCT_SETTLE_FACTOR` | `tools/measure-matrix.py` |
| `DEGCT_SETTLE_GROUPS` | `tools/measure-matrix.py` |
| `DEGCT_SETTLE_MS` | `measurements/relaxed_settle.rs`, `tools/measure-matrix.py` |
| `DEGCT_SPANNING` | `measurements/settles_within.rs` |
| `DEGCT_STARTS` | `measurements/cache_split_menu.rs`, `measurements/dead_quantify.rs`, `measurements/dominance_share.rs`, `measurements/manager_reuse.rs`, `measurements/menu_residue.rs`, `measurements/menu_wall.rs`, `measurements/per_start_setup.rs`, `measurements/prune_on_menus.rs`, `measurements/workspace_menus.rs` |
| `DEGCT_SWEEP_MS` | `measurements/settles_within.rs` |
| `DEGCT_THREAD` | `measurements/search_residue.rs`, `tools/measure-residue-arms.sh` |
| `DEGCT_TIME_BUDGET_MS` | `measurements/menu_residue.rs`, `measurements/menu_wall.rs` |
| `DEGCT_UNSEEN` | `measurements/cache_split_menu.rs`, `measurements/candidate_recurrence.rs`, `measurements/dead_quantify.rs`, `measurements/dominance_share.rs`, `measurements/manager_reuse.rs`, `measurements/menu_residue.rs`, `measurements/nodes_repeat.rs`, `measurements/prune_on_menus.rs` |
| `DEGCT_WIDTHS` | `measurements/candidate_recurrence.rs` |
| `DEGCT_WORKERS` | `measurements/README.md`, `tools/measure-census.py`, `tools/measurement_common.py` |

## Keeping this current

Nothing enforces the table, and a stale one is worse than none: a reader who trusts it will
look for a variable that has been renamed. Re-run the generator when a variable is added or
removed, and check the count at the top of the table against the count here.
