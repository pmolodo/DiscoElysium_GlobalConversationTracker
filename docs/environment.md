# Environment variables

Every environment variable this project defines is prefixed **`DEGCT_`**, and a throwaway
script in a session scratchpad uses **`DEGCTT_`**. The rule and the reasoning are in
`CLAUDE.md`; this is the list.

**There are six, and each one says at its own definition why it is not a CLI argument.** An
option belongs on a command line, where it carries its own name, its own help and its own
default, and where `--help` is never stale - so a driver's options are flags and a variable is
what is left when a flag genuinely cannot do the job. See de-3dx9, which moved forty-five of
them.

**A shell script's own working variables carry the prefix too**, and are not in this table. The
rule covers them because the collision that prompted it was a local - `GROUPS` is a built-in
array whatever a script meant by it - but a variable a script assigns before it reads, never
exports, and nothing else mentions is not an option anybody can set. `tests/environment_table.rs`
tells the two apart, and the note there says why a table that listed both was mostly noise.

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
| C# | `tools/GameAutomation/DegctEnv.cs` - `DegctEnv.Get`, `DegctEnv.IsSet`, `DegctEnv.Qualified`, `DegctEnv.Foreign` |

**The C# shapes carry their type's name and the others do not**, because `Get(` and `IsSet(` on
their own are words a C# file uses for a hundred other things, and a pattern matching them would
put a row here for every one.

Every one of them takes the **bare** name: `env("RUN_KIND")` reads `DEGCT_RUN_KIND`. They are all
idempotent about an already-qualified name, because callers build names from both halves and a
doubled prefix would be unset, silently, and read as a default.

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
looking for a variable that has been renamed. 6 variables.

**What each one is, and why it is not a flag.** The full argument lives at each definition; this
is the shape of it.

- **RUN_KIND, RUN_LOG and RUN_LOG_DIR** are one case. `tools/run-logged.sh` wraps `cargo`,
  `dotnet`, a Python driver - anything - and adding a flag to somebody else's command line is
  not a thing a wrapper may do. A tool that asked the wrapper for the name instead would derive
  a SECOND one, since a run is named for the instant it started; the pairing between a
  transcript and its rows has to be the one already decided, which means handing it over.
- **ALL_COMMITTED_SAVES** forces the slow exhaustive pass over every committed save. Cargo owns
  a test binary's command line, and this WIDENS three tests rather than selecting any, so
  `#[ignore]` and `--ignored` do not fit either.
- **CHECK_DEPLOY** turns on the check that refuses an in-game run against a look-ahead engine
  older than the one this tree has built. Not a flag because the thing that has to carry it is
  not a command line anybody types: a run is started by the harness, by `dotnet test`, and by
  whatever a session reaches for that afternoon, and the check has to be on for all of them or
  it protects only the paths somebody remembered.
- **MARKING** is not an option at all any more. Nothing sets it and no run can be asked for it:
  it is the name a value was written under in the run records already in `performance/logs`,
  read so that a later measurement can still be compared against them.

The rows below are generated and pasted; the list above is the one to read.

| `DEGCT_ALL_COMMITTED_SAVES` | `crates/gct-measure/src/common/mod.rs` |
| `DEGCT_CHECK_DEPLOY` | `DEVELOPING.md`, `tools/GameAutomation/DeployedEngine.cs` |
| `DEGCT_MARKING` | `tools/measurement_common.py` |
| `DEGCT_RUN_KIND` | `AGENTS.md`, `CLAUDE.md`, `tools/measure-menus.py`, `tools/measurement_common.py`, `tools/run-logged.sh` |
| `DEGCT_RUN_LOG` | `tools/measurement_common.py`, `tools/run-logged.sh` |
| `DEGCT_RUN_LOG_DIR` | `tools/measure-symbolic.sh`, `tools/measurement_common.py`, `tools/run-logged.sh` |

## Keeping this current

Add or remove a variable and `tests/environment_table.rs` fails, naming what is missing from the
table, what the table has that the code does not, and the whole block to paste over the rows
above. The count in the paragraph that introduces them is checked by the same test, since it
drifted too.

```bash
tools/smoke.sh                                  # does not run this - it is an integration test
cargo test --profile release-incremental --test suite environment_table::  # does
```
