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

Generated from the code rather than maintained by hand, by `tools/env-table.py`: it greps each
tracked file for a variable spelled out in full or asked for by bare name through the helper
for its language, so it says what the code actually asks for. 50 variables.

| `DEGCT_ALL_COMMITTED_SAVES` | `tests/common/mod.rs` |
| `DEGCT_ARMS` | `tools/measure-residue-arms.sh` |
| `DEGCT_BUDGET_MB` | `performance/bidirectional_headroom.rs`, `performance/bound_slack.rs`, `performance/cache_split.rs`, `performance/cache_split_menu.rs`, `performance/manager_reuse.rs`, `performance/menu_matrix.rs`, `performance/onward_or_back.rs`, `performance/repeat_question.rs`, `performance/search_residue.rs`, `tools/measure-residue-arms.sh` |
| `DEGCT_CEILING` | `performance/greedy_playthrough.rs` |
| `DEGCT_COMPARE` | `performance/onward_or_back.rs` |
| `DEGCT_CONVERSATION` | `AGENTS.md`, `CLAUDE.md`, `docs/modelling-gaps.md`, `performance/README.md`, `performance/backward_support.rs`, `performance/bidirectional_headroom.rs`, `performance/bound_slack.rs`, `performance/cache_split.rs`, `performance/cache_split_menu.rs`, `performance/counter_widths.rs`, `performance/greedy_playthrough.rs`, `performance/layout_shape.rs`, `performance/layout_slots.rs`, `performance/manager_reuse.rs`, `performance/menu_matrix.rs`, `performance/menu_wall.rs`, `performance/nodes_repeat.rs`, `performance/onward_or_back.rs`, `performance/per_start_setup.rs`, `performance/redundant_counters.rs`, `performance/repeat_question.rs`, `performance/variable_order.rs`, `performance/workspace_menus.rs`, `src/core/env.rs`, `tests/modelling_gaps.rs`, `tests/reference_oracle.rs`, `tools/degct-env.sh`, `tools/measure-menus.py`, `tools/measure-symbolic.sh`, `tools/measurement_common.py` |
| `DEGCT_EACH_MS` | `performance/onward_or_back.rs` |
| `DEGCT_FRESH` | `performance/workspace_menus.rs` |
| `DEGCT_GIT_INDEX_FILE` | `build.rs` |
| `DEGCT_GROUPS_ONLY` | `performance/README.md`, `performance/menu_matrix.rs`, `tools/measure-menus.py` |
| `DEGCT_GUARD_SNAPSHOT` | `performance/guard_snapshot.rs` |
| `DEGCT_HEADER` | `performance/menu_matrix.rs`, `tools/measure-menus.py`, `tools/measurement_common.py` |
| `DEGCT_LAYERS` | `performance/bidirectional_headroom.rs` |
| `DEGCT_LOG_DIR` | `tools/measure-symbolic.sh`, `tools/run-logged.sh` |
| `DEGCT_MARKING` | `AGENTS.md`, `CLAUDE.md`, `performance/README.md`, `performance/menu_matrix.rs`, `src/bridge.rs`, `src/symbolic/menu.rs`, `tools/measure-menus.py`, `tools/measurement_common.py`, `tools/menu-costs-diff.py` |
| `DEGCT_MEASUREMENT` | `tools/measure-symbolic.sh` |
| `DEGCT_MEMORY_BUDGET_MB` | `performance/menu_residue.rs` |
| `DEGCT_MENUS_OUT` | `performance/README.md`, `tools/DegctEnv.psm1`, `tools/degct-env.sh`, `tools/measure-menus.py`, `tools/measurement_common.py` |
| `DEGCT_MENU_TIME_BUDGET_MS` | `performance/menu_wall.rs` |
| `DEGCT_NOLIMIT` | `performance/menu_matrix.rs`, `src/GlobalConversationTracker.Core/DegctEnvironment.cs`, `src/core/env.rs`, `tools/DegctEnv.psm1`, `tools/degct-env.sh`, `tools/measurement_common.py` |
| `DEGCT_OUT` | `tools/degct-env.sh` |
| `DEGCT_PLAYTHROUGHS_OUT` | `performance/greedy_playthrough.rs` |
| `DEGCT_POOLED_ROUNDS` | `src/symbolic/menu.rs` |
| `DEGCT_PROFILE` | `performance/seen_profile.rs` |
| `DEGCT_REPEATS` | `performance/cache_split.rs`, `performance/per_start_setup.rs`, `performance/repeat_question.rs` |
| `DEGCT_ROUNDS` | `performance/manager_reuse.rs`, `performance/nodes_repeat.rs`, `performance/workspace_menus.rs` |
| `DEGCT_ROW_SECONDS` | `performance/cache_split.rs`, `tools/DegctEnv.psm1`, `tools/degct-env.sh` |
| `DEGCT_RUN_KIND` | `AGENTS.md`, `CLAUDE.md`, `tools/measure-menus.py`, `tools/run-logged.sh` |
| `DEGCT_RUN_LOG` | `tools/measurement_common.py`, `tools/run-logged.sh` |
| `DEGCT_RUN_LOG_DIR` | `tools/measure-symbolic.sh`, `tools/measurement_common.py`, `tools/run-logged.sh` |
| `DEGCT_RUN_NAME` | `tools/measure-symbolic.sh` |
| `DEGCT_SAVE` | `tests/kim_case_offline.rs` |
| `DEGCT_SCATTER` | `performance/onward_or_back.rs` |
| `DEGCT_SEARCHES` | `performance/search_residue.rs`, `tools/measure-residue-arms.sh` |
| `DEGCT_SEARCH_MS` | `performance/repeat_question.rs` |
| `DEGCT_SEEN_WORLD` | `performance/greedy_playthrough.rs`, `performance/menu_matrix.rs` |
| `DEGCT_SETTLE_FACTOR` | `tools/measure-menus.py`, `tools/measurement_common.py` |
| `DEGCT_SETTLE_GROUPS` | `tools/measure-menus.py`, `tools/measurement_common.py` |
| `DEGCT_SETTLE_MS` | `tools/measure-menus.py`, `tools/measurement_common.py` |
| `DEGCT_SPLITS` | `performance/cache_split.rs`, `performance/cache_split_menu.rs` |
| `DEGCT_STARTS` | `performance/bidirectional_headroom.rs`, `performance/bound_slack.rs`, `performance/cache_split_menu.rs`, `performance/manager_reuse.rs`, `performance/menu_matrix.rs`, `performance/menu_residue.rs`, `performance/menu_wall.rs`, `performance/onward_or_back.rs`, `performance/per_start_setup.rs`, `performance/workspace_menus.rs` |
| `DEGCT_TARGET` | `performance/bidirectional_headroom.rs` |
| `DEGCT_THREAD` | `performance/search_residue.rs`, `tools/measure-residue-arms.sh` |
| `DEGCT_TIME_BUDGET_MS` | `performance/menu_residue.rs`, `performance/menu_wall.rs` |
| `DEGCT_UNSEEN` | `performance/bidirectional_headroom.rs`, `performance/bound_slack.rs`, `performance/cache_split_menu.rs`, `performance/manager_reuse.rs`, `performance/menu_matrix.rs`, `performance/menu_residue.rs`, `performance/nodes_repeat.rs`, `performance/onward_or_back.rs` |
| `DEGCT_VAR_ORDER` | `performance/layout_slots.rs`, `src/symbolic/var_order.rs` |
| `DEGCT_VERIFY_REPLAY` | `performance/greedy_playthrough.rs` |
| `DEGCT_WALKED_PROFILE` | `performance/menu_matrix.rs` |
| `DEGCT_WEIGH_FRONTS` | `src/symbolic/backward.rs` |
| `DEGCT_WORKERS` | `performance/README.md`, `tools/measure-menus.py`, `tools/measurement_common.py` |

## Keeping this current

Nothing enforces the table, and a stale one is worse than none: a reader who trusts it will
look for a variable that has been renamed. Re-run `python tools/env-table.py` when a variable
is added or removed, paste its rows over the ones above, and update the count in the paragraph
that introduces them.
