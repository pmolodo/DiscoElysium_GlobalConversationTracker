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

**C# has no helper, because no C# file reads one of ours.** The rule still covers it: a C# file
that needs one gets a helper of its own, and `tests/environment_table.rs` gets the call shape to
find it in the same change. Until then there is nothing for a door to open.

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

Checked against the code rather than trusted, by `tests/environment_table.rs`: it reads each
tracked file for a variable spelled out in full or asked for by bare name through the helper for
its language, compares what it finds with the rows below and with the count in this sentence,
and fails with the block to paste. So a stale table is a failing test rather than a reader
looking for a variable that has been renamed. 21 variables.

A TEST RATHER THAN A GENERATOR because what is wanted is enforcement: a build script that
rewrote this file would dirty the working tree on every build, which every measurement's log
name would then carry as `-dirty`. Pasting by hand is fine as long as something fails when it is
forgotten.

| `DEGCT_ALL_COMMITTED_SAVES` | `tests/common/mod.rs` |
| `DEGCT_ARMS` | `tools/measure-residue-arms.sh` |
| `DEGCT_ASKED_LOG_DIR` | `tools/measure-symbolic.sh` |
| `DEGCT_BUDGET_MB` | `tools/measure-residue-arms.sh` |
| `DEGCT_CONVERSATION` | `AGENTS.md`, `CLAUDE.md`, `docs/modelling-gaps.md`, `src/core/env.rs` |
| `DEGCT_GIT_INDEX_FILE` | `build.rs` |
| `DEGCT_LOG_DIR` | `tools/measure-symbolic.sh`, `tools/run-logged.sh` |
| `DEGCT_MARKING` | `AGENTS.md`, `CLAUDE.md`, `performance/README.md`, `src/bridge.rs`, `tools/measurement_common.py`, `tools/menu-costs-diff.py` |
| `DEGCT_MEASUREMENT` | `tools/measure-symbolic.sh` |
| `DEGCT_NOLIMIT` | `src/core/env.rs`, `tools/measurement_common.py` |
| `DEGCT_OUT` | `tools/measure-residue-arms.sh` |
| `DEGCT_OVERRIDE` | `tools/measure-residue-arms.sh` |
| `DEGCT_POOLED_ROUNDS` | `src/symbolic/menu.rs` |
| `DEGCT_REST` | `tools/measure-symbolic.sh` |
| `DEGCT_RUN_KIND` | `AGENTS.md`, `CLAUDE.md`, `tools/measure-menus.py`, `tools/measurement_common.py`, `tools/run-logged.sh` |
| `DEGCT_RUN_LOG` | `tools/measurement_common.py`, `tools/run-logged.sh` |
| `DEGCT_RUN_LOG_DIR` | `tools/measure-symbolic.sh`, `tools/measurement_common.py`, `tools/run-logged.sh` |
| `DEGCT_RUN_NAME` | `tools/measure-symbolic.sh` |
| `DEGCT_SEARCHES` | `tools/measure-residue-arms.sh` |
| `DEGCT_VAR_ORDER` | `src/symbolic/var_order.rs` |
| `DEGCT_WEIGH_FRONTS` | `src/symbolic/backward.rs` |

## Keeping this current

Add or remove a variable and `tests/environment_table.rs` fails, naming what is missing from the
table, what the table has that the code does not, and the whole block to paste over the rows
above. The count in the paragraph that introduces them is checked by the same test, since it
drifted too.

```bash
tools/smoke.sh                                  # does not run this - it is an integration test
cargo test --release --test environment_table   # does
```
