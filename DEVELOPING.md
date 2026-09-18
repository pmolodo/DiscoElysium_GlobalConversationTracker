# Developing

Build, deploy and packaging for **GlobalConversationTracker**, the BepInEx plugin for
Disco Elysium - The Final Cut. The plugin's own notes live in
[src/GlobalConversationTracker.Plugin/README.md](src/GlobalConversationTracker.Plugin/README.md).

## TL;DR

```powershell
cargo build --release    # the engine host, and the state library the mod links
.\deploy.ps1             # build + install into your Steam copy
dotnet test              # compile every project, the plugin included, and run every test
```

Then launch the game. That is the whole iterate loop: edit -> `.\deploy.ps1` -> relaunch.

**The Rust build comes first, and is not optional.** `gct_state.dll` is the one reader and
writer of the mod's own state file - there is no C# copy of that format - so a `dotnet
build` without it stops and says which cargo command to run. The engine host beside it is
different: a plugin without one logs the look-ahead as unavailable and plays on.

`dotnet test` alone is enough as the end-of-change check. It builds only the test projects
and what they reference, and no test references the plugin - none can meaningfully test it,
which would need BepInEx, the IL2CPP interop assemblies and a running game. So
`GlobalConversationTracker.Session.Tests` carries a project reference to the plugin marked
`ReferenceOutputAssembly="false"`: a build-order dependency and nothing else, adding no
assembly reference and copying no DLL. It is a build trigger, not coverage, so a compile
error in the plugin fails `dotnet test`.

## Requirements

- Windows PowerShell 5.1 (what ships with Windows) or newer
- the .NET SDK (`dotnet`) - 10.0.400 was used; the plugin targets `net6.0`
- **for the plugin only**, a Disco Elysium install with **BepInEx 6.0.0-be.688 (IL2CPP /
  CoreCLR)** already set up **and run at least once**, so `<game>\BepInEx\interop` holds
  the generated interop assemblies. Nothing else needs a game: `BepInEx\core` is downloaded
  automatically against the pin in `BepInEx.props`, so the libraries, the tests and the
  offline tools build on a machine that has never had Disco Elysium on it

## The solution

`GlobalConversationTracker.slnx` at the repo root is the single entry point for the
libraries, the offline tools and their tests. `dotnet build` and `dotnet test` with no
arguments pick it up, so there is no need to `cd` into one project at a time:

```powershell
dotnet build                 # every project, plugin included (Debug, dotnet's default)
dotnet test                  # ... and run every test project, src\ and tools\ alike
dotnet build -c Release      # what the .ps1 scripts build by default
```

Two things about its contents are deliberate:

- **The plugin is built by `dotnet build` and by `dotnet test`.** It is not the only
  project needing a game install - Persistence and Session carry their own
  `Il2CppInterop.Runtime` reference - and all three resolve the install through
  `Directory.Build.props` (see `provision-refs.ps1` below for the order). `dotnet test`
  reaches the plugin through the build-order reference described in the TL;DR.
  `.\build.ps1` is the way to *package* the plugin - it stamps the commit and prints the
  DLL path - not the only way to compile it.
- **The `tools\` projects are included**, even though none of them is part of the plugin,
  so that a repo-root build keeps the whole repo compiling rather than most of it. Each is
  either a shared library or a standalone console app; run a console app with
  `dotnet run --project tools\<name> -- --help`.
  - `DialogueAsset` - streams the Dialogue System database `.asset` and supplies the
    shared models, readers and writers used by the extraction and look-ahead tools.
  - `DialogueExtract` - regenerates the offline data derived from that database. The
    `.asset` is not in the repo: it comes from an AssetRipper export and defaults to
    `.game_reference_copies\AssetRipperExport\ExportedProject\Assets\Dialogue Databases\Disco Elysium.asset`.
    Its subcommands and default outputs are:
    - `articy-ids` - `articy_ids_final_cut.json` at the repo root.
    - `conversation-index` - `.game_reference_copies\derived\conversation_index.jsonl`.
    - `corpus` - `.game_reference_copies\derived\distinct_guards.txt` and
      `.game_reference_copies\derived\distinct_scripts.txt`.
    - `shipped-index` - `.game_reference_copies\derived\conversation_index.trimmed.jsonl`,
      the conversation index cut down to what a crawl reads. Deployed beside the plugin as
      `GlobalConversationTracker.Index.jsonl`, which is what the native look-ahead opens.
    - `variables` - `.game_reference_copies\derived\variables.jsonl`, what the database
      declares each of its 10,645 variables to be. Deployed as
      `GlobalConversationTracker.Variables.jsonl`; without it the look-ahead cannot tell a
      counter from a flag, and an ordering comparison over one becomes undecidable.
    - `actors` - `.game_reference_copies\derived\actors.jsonl`, every actor's id and name.
      Which skill a passive check tests is decided by its speaker, and an entry names one
      only by id, so this is what lets an offline reader decide a check the way the plugin
      does.
    - `worst-case-state` - `testing\scenarios\global-state-worst-case.json`, derived from
      the conversation index rather than directly from the `.asset`.
  - `gct-state-check` - a Rust binary rather than one of these: verifies
    `global-conversation-state.json` is the union of two or more saves, with no dialogue
    status lower than the highest save that mentions it. Run it by hand after a real
    playthrough, which is the only thing that can answer the question it asks.
  - `GlobalStateBenchmark` - times `GlobalStateStore.Save` broken out by phase, and what
    a caller pays now that the write runs on a background thread, over a sweep of state
    sizes.
Reading and writing a save is the engine host's, by verb rather than by a tool of its own -
the formats live in that crate, so a second program would be a second definition of them.
`dump <save> [<table>]` prints one of a save's five Lua tables, the conversations by
default; `expand <save.zip> <out> [<base>]` writes the archive the game wrote as the
directory this repository commits, and `pack` goes the other way; `rewrite <save.ntwtf>`
writes a committed save again as this build writes one, which is what a format version bump
needs.

Bringing an older file forward is `gct-engine-host convert <file> [<out>]`, a verb rather
than a tool of its own. It works out WHICH format and WHICH version from the file itself,
so neither is an argument: point it at a file and it converts, or says the file is already
current and succeeds. With no `<out>` it converts the file in place and keeps the original
beside it, named for the version it was (`state.json` at version 4 is kept as
`state.v4.json`), so whatever reads the file by its name can read it straight away. It never
overwrites anything: where the kept name, or a named output, is already taken, it refuses.
Every reader in the repository refuses anything but the version it writes and names that
command, so it is the only way to open an older file.

The `.slnx` format, not the classic `.sln`, because it is what `dotnet new sln` emits with
the SDK this repo builds on and it can carry those notes as comments. It needs the .NET SDK
9.0.200 or newer, Visual Studio 17.14+, or Rider 2024.3+.

## The scripts

Five PowerShell scripts at the repo root, each runnable directly, plus one module they
all share:

`build.ps1`, `deploy.ps1`, `make-release.ps1`, `provision-refs.ps1` and `capture-log.ps1`
each `Import-Module .\build-support.psm1`, which holds every shared constant and function
(`Find-SteamGameDir`, `Resolve-TargetGameDir`, `Initialize-BuildReferences`,
`Invoke-PluginBuild`, `Copy-PluginPayload`, ...). No script sources another.

That module is a `.psm1` on purpose. PowerShell runs a dot-sourced script's `param()`
block **in the caller's scope**, so a shared script that declares `-DiscoElysiumDir` or
`-Configuration` resets its caller's copy to the default before the caller ever reads it.
`Import-Module` never executes anything in the importer's scope, so that whole class of
bug cannot arise.

### `provision-refs.ps1`

Locates and verifies the game install the **build reads reference assemblies from**, and
checks that every DLL the csproj lists is actually there - so a missing reference fails
with one clear sentence instead of a wall of MSBuild errors.

Resolution order:

1. `-DiscoElysiumDir <path>`
2. the `DISCO_ELYSIUM_DIR` environment variable
3. the last resolved install, cached per-machine in
   `%LOCALAPPDATA%\GlobalConversationTracker\reference-game-dir.txt`
4. **Steam auto-discovery**: registry `Valve\Steam` + `libraryfolders.vdf`, AppID **632470**
5. a modded copy under `<repo>\.game_reference_copies`

**Step 3 is not a source, it is an optimization over the ones around it.** Steps 1 and 2
cost nothing to read, discovery does, so the cache goes between them: consulted after
discovery it would save nothing, since discovery rewrites it on the way out.

Among the sources, the live Steam install beats the repo copy - it is the one that gets
patched and re-run as the game updates, so its interop assemblies match the game you are
actually playing. The price of reading the cache first is that an install you named by hand
goes on winning after the game updates, with nothing to notice its interop has aged. Name
another, or delete the cache file, to move off it.

**Every candidate must contain BepInEx.** A pristine copy from `depot_download.ps1` is the
game as Steam ships it and carries none of these assemblies, so a folder full of reference
copies can still leave resolution failing - correctly.

Every route that resolves writes step 3's cache, and the cache lives outside the repo on
purpose: a git worktree has no reference copy of its own, so a cache kept in the tree could
never answer the question there. One `.\provision-refs.ps1` anywhere on the machine answers
it for every checkout on it.

`Directory.Build.props` applies steps 1, 2, 4 and 5 itself, so a bare `dotnet build`
resolves the same install without going through PowerShell. Step 3 is the one MSBuild
cannot do, which is why a machine with no cache yet needs one script run. The install is
only ever **read** from.

Normally you do not run this directly - `build.ps1` does it for you.

### `build.ps1`

The everyday build. Verifies references, then runs `dotnet build`, passing the resolved
directory through as `-p:DiscoElysiumDir=<path>` so the csproj's own resolution mechanism
is what actually runs. Prints the path of the built DLL.

```powershell
.\build.ps1
.\build.ps1 -Configuration Debug
.\build.ps1 -DiscoElysiumDir "C:\path\to\Disco Elysium"
```

It also stamps the commit it is building into the assembly, as
`-p:SourceRevisionId=<commit>[.dirty]`. The SDK appends that to the assembly's
informational version, which Windows exposes as the DLL's `ProductVersion`:

```
0.1.0+80da8e807ba78addfd8ffa379292654bec5f1d55.dirty
```

The commit therefore travels *inside* the DLL rather than beside it, so a deployed
plugin - and any log captured from a session that loaded it - can be tied back to the
source it was built from, and cannot be paired with the wrong commit by copying a file
around. `.dirty` means the tree differed from that commit **in a way the build could
see**: any change to a tracked file, or an untracked file under `src\`, `tools\`, or one
of the root build inputs (`*.slnx`, `Directory.Build.*`). An untracked `.cs` under `src\`
is compiled like any other, so a tree holding one is not the commit it claims - but a stray
`debug.log` at the root is not, and must not flag every otherwise clean checkout. The build
prints what made it dirty, and names untracked files it deliberately did not count. A build
with no `git` available is stamped with nothing and says so.

### `deploy.ps1`

Build, then install into a **playable** copy of the game. The headline command.

```powershell
.\deploy.ps1                              # auto-discovered Steam copy
.\deploy.ps1 -GameDir "C:\Apps (x86)\Games\Steam\steamapps\common\Disco Elysium"
$env:DISCO_ELYSIUM_DEPLOY_DIR = "C:\...\Disco Elysium"; .\deploy.ps1
.\deploy.ps1 -DryRun                      # show the target, write nothing
```

What it does:

1. resolves and vets the target (before building, so a bad target fails in a second)
2. builds
3. prints the exact directory it is about to write to
4. replaces the previous build in `<game>\BepInEx\plugins\GlobalConversationTracker`:
   one `GlobalConversationTracker.dll` and nothing else, the mod's own layers being
   compiled into it. The `.pdb` is **not** installed, so a stack trace in a deployed build
   carries no line numbers and the commit stamped inside the DLL is what ties one back to
   its source. Written and deleted are the same set, `GlobalConversationTracker*.dll`,
   which is what makes it safe to delete files rather than the folder; the price is that a
   `.pdb` an older build installed stays where it is. Anything else in that folder - the
   optional hand-placed `articy_ids_final_cut.json` above all - survives a redeploy.
5. prints the log path and the line to look for

**Target resolution:** `-GameDir`, else `DISCO_ELYSIUM_DEPLOY_DIR`, else the
auto-discovered Steam copy. It only stops and asks when all three come up empty, meaning
no override was given *and* no Steam install was found.

What actually keeps a stray deploy from doing damage is the guards, not the absence of a
default: the resolved target is printed before anything is written, any path under
`.game_reference_copies` is refused outright (`-AllowReferenceCopy` overrides), a copy
without BepInEx is rejected because the plugin could never load there, and the only
directory created or deleted is `<game>\BepInEx\plugins\GlobalConversationTracker`.
Uninstalling is deleting that one folder, so there is no uninstaller script to run.

### `make-release.ps1`

Builds and packages `.build\dist\GlobalConversationTracker-v<version>.zip`, laid out to
extract straight into a game folder:

```
BepInEx\plugins\GlobalConversationTracker\GlobalConversationTracker.dll
GlobalConversationTracker-README.md
GlobalConversationTracker-LICENSE.txt
```

One DLL, and only the DLL: the mod's own layers are compiled into the plugin assembly
rather than referenced, and the `.pdb` stays behind in the build output rather than being
packaged. It is the same payload `deploy.ps1` installs, from the same helper. The version
comes from `<Version>` in the csproj.

It also writes a second archive, `GlobalConversationTracker-v<version>-AllInOne.zip`, for
players who do not already run mods:

```
winhttp.dll, doorstop_config.ini, .doorstop_version, dotnet\      <- BepInEx 6.0.0-be.688
BepInEx\core\, BepInEx\patchers\, BepInEx\plugins\               <- ...
BepInEx\plugins\GlobalConversationTracker\GlobalConversationTracker.dll
GlobalConversationTracker-README.md
GlobalConversationTracker-LICENSE.txt              <- ours, MIT
BepInEx-LICENSE.txt                                <- BepInEx's, LGPL-2.1
GlobalConversationTracker-THIRD-PARTY-NOTICES.txt  <- who wrote what
Uninstall-GlobalConversationTracker.ps1
GlobalConversationTracker-install-manifest.json
```

Four things about it are worth knowing:

- **The BepInEx build is pinned**, to the same 6.0.0-be.688 the reference install runs, and
  the archive is downloaded from `builds.bepinex.dev`, checked against a pinned SHA256 and
  cached per machine under `%LOCALAPPDATA%\GlobalConversationTrackerepinex`. The pin, the
  URL and the hash are at the top of `build-support.psm1`; changing them means re-running
  the end-to-end install check, not just editing three lines.
- **BepInEx's archive ships whole**, with one file renamed: `changelog.txt` goes in as
  `BepInEx-changelog.txt`, because at the root of a game folder the bare name says nothing
  about whose it is. Nothing collides with a file the game ships - `changelog.txt`,
  `winhttp.dll`, `doorstop_config.ini`, `.doorstop_version` and `dotnet\` are all BepInEx's,
  and none of them appear in a copy straight from Steam.
- **Both archives carry licences, and the bundle carries four files about licensing.**
  Ours (`GlobalConversationTracker-LICENSE.txt`, MIT) ships in both, because a DLL in
  somebody's game folder is a distribution and MIT asks the notice to travel with it.
  Google.Protobuf's (`Google.Protobuf-LICENSE.txt`, BSD-3-Clause) ships in both too: the
  plugin folder holds `Google.Protobuf.dll`, and that is a binary redistribution.
  BepInEx's (`BepInEx-LICENSE.txt`, LGPL-2.1) ships in the bundle only, as its own file
  rather than quoted inside prose. `GlobalConversationTracker-THIRD-PARTY-NOTICES.txt` is
  the attribution: what BepInEx build is in here, its commit, the URL it came from, that
  SHA256, which protobuf version and where it came from, and which files in the archive are
  ours. Packaging fails rather than shipping without a licence.
- **The protobuf pin lives in `Protobuf.props`**, read by both csprojs and by
  `build-support.psm1`, so the version the wire is generated against, the runtime that
  ships, and the licence that is fetched cannot name three different releases. The licence
  URL carries the version, so it always matches the binary.
- **`BepInEx\interop` cannot be shipped.** Those assemblies are generated from the player's
  own game build on first launch, which is why that launch is slow.
- **The uninstaller is hash-checked.** `GlobalConversationTracker-install-manifest.json`
  lists every shipped file with its SHA256, and `Uninstall-GlobalConversationTracker.ps1`
  deletes a file only if it is still byte-for-byte what was installed. A BepInEx updated in
  place, an edited config, another mod's file at the same path: all left alone and reported.
  It supports `-WhatIf`, keeps the global state file unless `-RemoveGlobalState`, and keeps
  BepInEx's generated data unless `-RemoveBepInExData`.

`-PluginOnly` skips the bundle (and its download); `-BundleOnly` emits only the bundle.

### `capture-log.ps1`

Copies a session's BepInEx log out of the game folder and proves the copy belongs to the
run it is meant to document.

```powershell
.\capture-log.ps1 -Label smoke-test       # while the game is still running
$state = "$env:USERPROFILE\AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames\global-conversation-state.json"
.\capture-log.ps1 -Label session-c -RunArtifact $state
```

BepInEx truncates `LogOutput.log` at process start, so a session's log only survives until
the next launch. "Copy it when the session is over" therefore loses the race whenever
anything relaunches the game in between, and the copy is then a different process's log
while looking exactly like the right one.

So the script copies first - to `.build\logs\<label>-<timestamp>.log` unless `-Destination`
says otherwise - and judges afterwards, using the Harmony banner written while the plugin
patches in `Load()`:

```
### At 2026-08-15 09.34.31
```

That stamp identifies the process that wrote the log, so:

- every `-RunArtifact` - a file that run wrote (the global state file, a save, ...) - must
  have been written at or after it. An artefact *older* than the stamp proves the log is a
  later process's.
- if the game is still running, the stamp must fall inside the running process's lifetime.
- with neither available there is nothing to check against, which is a failure too: an
  artefact that cannot be cross-checked is worse than none.

The copy and a `<copy>.capture.json` manifest (md5, size, the stamp, every check and its
verdict, `verified`) are written even when a check fails, but the script then exits
non-zero unless `-Force` was given. Nothing is ever written into the game folder.

The manifest also records what the run *was*, not just that the log belongs to it:

| Field | What it is |
| --- | --- |
| `pluginCommit`, `pluginTreeDirty`, `pluginBuildVersion` | read out of the installed `GlobalConversationTracker.dll`'s own `ProductVersion` (see `build.ps1` above) |
| `pluginDir`, `pluginFiles` | every file installed in `<game>\BepInEx\plugins\GlobalConversationTracker` with size, write time and md5 - which covers the optional `articy_ids_final_cut.json` without naming it |
| `route` | the source named in the log's `Resynced the global state from the running game (...)` line |
| `envelopeOperation`, `averageEnvelopeMs`, `envelopeCallCount` | the run's final `Average envelope for <op>` figure, so runs can be compared without re-parsing logs |

Each is recorded when present and left `null` when not; a log from a build that stamped
nothing is a fact worth recording rather than a failure. Two warnings come out of this:
an installed DLL with no commit stamp cannot tie the log to a source revision at all, and
an installed DLL written *after* the run's plugin-load stamp means a deploy happened
between the run and the capture, so the folder listed is a later build's.

**Which install it reads:** `-GameDir`, else `DISCO_ELYSIUM_DEPLOY_DIR`, else the
auto-discovered Steam copy - the same playable copy `deploy.ps1` writes to.

**Renaming a capture:** do it through the script, not by hand.

```powershell
.\capture-log.ps1 -Rename .build\logs\capture-20260819-162547.log `
                  -NewName ApplyRawBytes-Hook-06-skip4tables.log
```

A generated name says nothing about the run it documents, and renaming the `.log` by hand
leaves `<old name>.log.capture.json` behind, describing a file that is no longer there.
`-Rename` moves both, rewrites the manifest's `copy` to where the copy now is, and keeps
the old path in `renamedFrom`. It refuses to overwrite an existing name, and needs no game
folder, no log and no running process.

A capture is paired with its manifest **by name** - `<log>.capture.json` beside `<log>` -
and that is the pairing to rely on. `copy` is where the copy was when the manifest was
written, which is history, not a pointer. Each manifest also carries a `runId`
(`<capture timestamp>-<first 8 of the md5>`), so a manifest that has been separated from
its log can still be matched back to it by `md5` and `bytes`.

## Run logs

Every run keeps its whole output under a name that says when it ran, what it ran against,
and what it was:

```
performance/logs/2026-09-04/2026-09-04_07,44,32_1e08319064b7bd9d115f26c3abf35145d3fb7d8e_cargo_full-suite.txt
                  ^ a folder ^ date   ^ time     ^ the commit it ran against              ^ tool ^ verb
                    per date
```

The date is in the folder and in the name both, so a log still says when it ran once it has
been copied somewhere else. `tools/tidy-logs.py` sorts any that arrive loose, taking the date
from the name where there is one and from the file's modification time otherwise.

The wrapper writes to `performance/logs`, so that a measurement's raw output sits beside
the rows it produced and nothing has to say so at the call site; `RUN_LOG_DIR` moves it.
GameHarness keeps its own logs under `testing/logs`, which is a separate mechanism and
reads no environment variable of ours.

The time is `HH,MM,SS` - commas because a file name cannot hold a colon - so a day's runs
sort into the order they happened.

The revision gains `-dirty` when the tree has been changed since that commit - a sha that
does not describe what actually ran would invite a later reader to diff against a commit
that never contained the code under test - and a name already taken gains `_2`, `_3`,
which now takes two runs starting in the same second. The folder is gitignored; the logs
are for reading and diffing locally, not for committing.

**GameHarness logs itself.** Every verb, including the ones the in-game tests reach by
calling `Program.Main`, with nothing to remember at the call site:

```powershell
dotnet run --project tools/GameHarness/GameHarness.csproj -- look-ahead
```

`--no-log`, or `DISCO_ELYSIUM_GCT_NO_RUN_LOG=1`, turns it off.

**Everything else goes through the wrapper**, which cargo and `dotnet test` need because
neither is ours to modify:

```bash
tools/run-logged.sh --kind test cargo corpus -- cargo test --test corpus
tools/run-logged.sh --kind test dotnet unit -- dotnet test
DISCO_ELYSIUM_GCT_INGAME_TESTS=1 \
  tools/run-logged.sh --kind test dotnet in-game -- dotnet test tools/GameAutomation.Tests
```

It tees, so a long run can still be watched, and it exits with the command's own status.

The naming is written out twice - `tools/GameAutomation/RunLog.cs` for the runs that can
call it, `tools/run-logged.sh` for the ones that cannot, since the script has to work
before anything is built and the harness has to work without a shell. `RunLogTests` runs
both and fails if they disagree.

## Safety rules baked into the scripts

- **The repo's reference material is never written to.** `deploy.ps1` refuses any target
  path under `.game_reference_copies` - the folder is the rule, so a copy added tomorrow is
  covered without anyone updating a list. (`-AllowReferenceCopy` overrides it with a
  warning.) Building only ever reads from a game install, never writes.
- **No silent deploy default** - see `deploy.ps1` above.
- **The BepInEx config is never edited.** If `[Logging.Console] Enabled` is not `true`,
  deploy just says so and moves on; likewise `capture-log.ps1` only points out that
  `[Logging.Disk] AppendLog = true` would keep every session in one log instead of
  overwriting it at each launch.

## Verifying a deploy

Only the game itself can confirm the plugin loads.

1. Launch the game (Steam, or `disco.exe` directly).
2. Watch the BepInEx console, or read `<game>\BepInEx\LogOutput.log` afterwards, for:

   ```
   [Info   :   BepInEx] Loading [GlobalConversationTracker 0.1.0]
   [Message:GlobalConversationTracker] GlobalConversationTracker v0.1.0 loaded.
   ```

   The second line is the one that proves the plugin's entry point ran. To keep that log,
   run `.\capture-log.ps1` before anything relaunches the game - see above.
3. For a live console, set `Enabled = true` under `[Logging.Console]` in
   `<game>\BepInEx\config\BepInEx.cfg`.

If the plugin never appears, check `LogOutput.log` for a load error and confirm the DLL was
built against the same BepInEx build as the one installed in that game copy.

## Design notes

### References are read from the game directory, not vendored

The obvious alternative - copy the reference DLLs into a repo-local `.build\lib` and point
the csproj's `HintPath`s there - is what you would do for a Unity **Mono** game, where
BepInEx itself is a downloadable, version-pinnable zip.

That does not fit here. Disco Elysium is **IL2CPP**, so the interesting references are the
interop assemblies under `<game>\BepInEx\interop`. Those are generated on this machine, on
first run, from *this* copy of the game's `GameAssembly.dll`. They cannot be downloaded and
are not interchangeable between game builds, so vendoring them would only be caching a
local artifact - at the cost of a second, parallel reference mechanism next to the one the
csproj already has.

So the csproj keeps resolving `DiscoElysiumDir` itself (see its `PropertyGroup` and the
`ValidateBepInExReferenceDirs` target), and `provision-refs.ps1` drives that instead of
competing with it: it decides *which* install, checks the DLLs are there, and hands the
path to MSBuild.

### `deploy.ps1` installs files, it does not extract an archive

The all-in-one bundle is a release artefact only; `deploy.ps1` copies the payload files
straight in, with no dormant extract-an-archive path. It starts from a known state,
clearing the previous build's `GlobalConversationTracker*` files out of
`<game>\BepInEx\plugins\GlobalConversationTracker\` before copying, and never touches
anything above that folder.

### Build output layout

`Directory.Build.props` redirects `bin` and `obj` to `.build\bin` and `.build\obj`; the
scripts put `cache`, `stage` and `dist` there too. One gitignored `.build\` folder holds
everything generated, so `src\` stays clean.

## Important paths

| What | Where |
| --- | --- |
| Solution | `GlobalConversationTracker.slnx` |
| Plugin project | `src\GlobalConversationTracker.Plugin\` |
| Built DLL | `.build\bin\GlobalConversationTracker.Plugin\<Configuration>\net6.0\GlobalConversationTracker.dll` |
| Release zip | `.build\dist\GlobalConversationTracker-v<version>.zip` |
| Captured logs | `.build\logs\<label>-<timestamp>.log` (+ `.capture.json`) |
| Installed plugin | `<game>\BepInEx\plugins\GlobalConversationTracker\` |
| BepInEx log | `<game>\BepInEx\LogOutput.log` |
| BepInEx config | `<game>\BepInEx\config\BepInEx.cfg` |
| Playable Steam copy | `C:\Apps (x86)\Games\Steam\steamapps\common\Disco Elysium` |
| Reference copies (read-only) | `.game_reference_copies\` - Steam downloads, AssetRipper exports, decompiler output; gitignored |
| BepInEx cache (per machine) | `%LOCALAPPDATA%\GlobalConversationTracker\bepinex\` |
