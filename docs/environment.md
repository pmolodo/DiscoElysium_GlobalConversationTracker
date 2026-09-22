# Environment variables

Every environment variable this project defines is prefixed **`DEGCT_`**, and a throwaway
script in a session scratchpad uses **`DEGCTT_`**. The rule and the reasoning are in
`CLAUDE.md`; this is the list.

**There are twelve, and each one says at its own definition why it is not a CLI argument.** An
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

**MSBuild has no door, and the plugin's own assemblies cannot reach one.** A property file reads
`$(DEGCT_GAME_DIR)` with the prefix typed out, because MSBuild has no function to put it there;
`GlobalStatePath` does the same, because the C# helper lives beside the automation tools and
that assembly ships inside the game. Both spell the name once, where it is defined.

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

**A name read without a door has to be one or the other**, and
`every_name_the_code_reads_is_ours_or_somebody_elses` in `tests/environment_table.rs` fails
where it is neither. What counts as reading without a door is `$env:NAME`,
`Environment.GetEnvironmentVariable("NAME")`, `std::env::var("NAME")`, `env!("NAME")`,
`os.environ["NAME"]` and the rest of that family - everything that takes the name exactly as
written. The foreign ones it allows are `PATH`, `HOME`, `USERPROFILE`, `LOCALAPPDATA`,
`SystemRoot`, `OUT_DIR`, `DOTNET_HOST_PATH` and anything cargo sets under `CARGO_`, whose
membership cargo decides - there is a `CARGO_BIN_EXE_` per binary a test asks for.

**A C# constant may hold a BARE name**, which is the shape to prefer: `"NO_RUN_LOG"` in the
file, `DEGCT_NO_RUN_LOG` in the environment, because the door puts the prefix on. The same test
challenges a `...Variable` constant that nothing passes to a door, since a bare name nothing
prefixes is read bare - and that is indistinguishable, from the outside, from a full name under
some other prefix, which is exactly what six of these were.

It sees a name where the name is WRITTEN DOWN. A name computed at runtime it cannot judge, and
MSBuild is out of reach altogether: `$(NAME)` is a property reference and an environment read at
once, with nothing to tell them apart.

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
looking for a variable that has been renamed. 12 variables.

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
- **GAME_DIR and DEPLOY_DIR** name a copy of the game: the one to BUILD against, which needs
  BepInEx's interop assemblies in it, and the one to deploy INTO, which needs to be playable.
  Both are one link in a resolution order that ends in Steam auto-discovery, and the scripts
  that read them take a `-DiscoElysiumDir` or `-GameDir` parameter for the same thing - so what
  a variable adds is a machine that answers without either, including for a bare `dotnet build`,
  which has no command line of ours at all.
- **NO_RUN_LOG** turns off the run log. `--no-log` does the same for the harness; the variable
  covers what has no flag to take, since `dotnet test` and cargo own their own command lines.
- **INGAME_TESTS** opts into the tests that need the game and a desktop. Not a flag because
  cargo and `dotnet test` own the command line those run on, and the point is a default that
  SKIPS: a suite that fails because somebody alt-tabbed teaches nobody anything.
- **PROFILE_DIR** redirects the game's profile folder, which is how the harness's most
  destructive path - moving that folder aside and putting it back - is tested against a scratch
  copy rather than against gigabytes of somebody's playthroughs.
- **GLOBAL_STATE_PATH** redirects the mod's own state file. It is read INSIDE the game, where
  there is no command line to put a flag on, and it is what lets a test or a staged run keep its
  state somewhere other than beside the player's saves.

The rows below are generated and pasted; the list above is the one to read.

| `DEGCT_ALL_COMMITTED_SAVES` | `crates/gct-measure/src/common/mod.rs` |
| `DEGCT_CHECK_DEPLOY` | `DEVELOPING.md`, `tools/GameAutomation/DeployedEngine.cs` |
| `DEGCT_DEPLOY_DIR` | `DEVELOPING.md`, `build-support.psm1`, `capture-log.ps1`, `deploy.ps1`, `src/GlobalConversationTracker.Plugin/README.md` |
| `DEGCT_GAME_DIR` | `DEVELOPING.md`, `Directory.Build.props`, `Directory.Build.targets`, `build-support.psm1`, `build.ps1`, `make-release.ps1`, `provision-refs.ps1`, `src/GlobalConversationTracker.Plugin/README.md` |
| `DEGCT_GLOBAL_STATE_PATH` | `src/GlobalConversationTracker.Persistence/GlobalStatePath.cs` |
| `DEGCT_INGAME_TESTS` | `DEVELOPING.md`, `tools/GameAutomation.Tests/InGameFactAttribute.cs`, `tools/run-logged.sh` |
| `DEGCT_MARKING` | `tools/measurement_common.py` |
| `DEGCT_NO_RUN_LOG` | `DEVELOPING.md`, `tools/GameAutomation/RunLog.cs`, `tools/GameHarness/Program.cs` |
| `DEGCT_PROFILE_DIR` | `tools/GameAutomation/GameProfile.cs` |
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
