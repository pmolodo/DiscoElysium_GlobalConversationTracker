# Developing

Build, deploy and packaging for **GlobalConversationTracker**, the BepInEx plugin for
Disco Elysium - The Final Cut. The plugin's own notes live in
[src/GlobalConversationTracker.Plugin/README.md](src/GlobalConversationTracker.Plugin/README.md).

## TL;DR

```powershell
.\deploy.ps1     # build + install into your Steam copy
dotnet test      # compile every project, the plugin included, and run every test
```

Then launch the game. That is the whole iterate loop: edit -> `.\deploy.ps1` -> relaunch.

`dotnet test` alone is enough as the end-of-change check, and that took arranging.
`dotnet test` builds only the test projects and what they reference, so the plugin - which
no test references, and none can meaningfully test - used to sit outside it, and a compile
error in it passed. `GlobalConversationTracker.Session.Tests` therefore carries a project
reference to the plugin marked `ReferenceOutputAssembly="false"`: a build-order dependency
and nothing else, adding no assembly reference and copying no DLL. It is a build trigger,
not coverage, and the comment at the reference says so at length. Nothing tests the
plugin, which would need BepInEx, the IL2CPP interop assemblies and a running game.

## Requirements

- Windows PowerShell 5.1 (what ships with Windows) or newer
- the .NET SDK (`dotnet`) - 10.0.400 was used; the plugin targets `net6.0`
- a Disco Elysium install with **BepInEx 6.0.0-be.688 (IL2CPP / CoreCLR)** already set up
  **and run at least once**, so `<game>\BepInEx\interop` holds the generated interop
  assemblies

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

- **The plugin is built by `dotnet build` and by `dotnet test`**, as of 2026-08-26. It used to be listed with
  `<Build Project="false" />`, on the grounds that it alone needed a game install to
  compile against; that stopped being true when Persistence and Session gained their own
  `Il2CppInterop.Runtime` reference. The exclusion then protected nothing - a root build
  in a checkout without an install failed on those two projects first - while quietly
  costing coverage: a compile error in the plugin passed `dotnet build` and `dotnet test`
  and only appeared when someone built the csproj by hand. All three projects resolve the
  install through `Directory.Build.props` (see `provision-refs.ps1` below for the order),
  and `dotnet test` reaches the plugin through the deliberate build-order reference
  described in the TL;DR. `.\build.ps1` remains the way to *package* the plugin - it stamps
  the commit and prints the DLL path - but it is no longer the only way to compile it.
- **The `tools\` projects are included**, even though none of them is part of the plugin,
  so that a repo-root build keeps the whole repo compiling rather than most of it. Each is
  a standalone console app; run one with `dotnet run --project tools\<name> -- --help`.
  - `NtwtfDecode` - dumps the Lua tables inside a `{save}.ntwtf` save (zip, folder or
    `.lua` file) as JSON.
  - `GlobalStateCheck` - verifies `global-conversation-state.json` is the union of two or
    more saves, with no dialogue status lower than the highest save that mentions it.
  - `GlobalStateBenchmark` - times `GlobalStateStore.Save` broken out by phase, and what
    a caller pays now that the write runs on a background thread, over a sweep of state
    sizes.

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
3. `<repo>\Steam Install - Unaltered\Disco Elysium` - untracked, so present in a clone
   and absent in a git worktree
4. the last resolved install, cached per-machine in
   `%LOCALAPPDATA%\GlobalConversationTracker\reference-game-dir.txt`
5. Steam auto-discovery: registry `Valve\Steam` + `libraryfolders.vdf`, AppID **632470**

Every route that resolves writes step 4's cache, not just discovery, and the cache lives
outside the repo on purpose: a worktree has neither the untracked copy from step 3 nor a
`.build\` of its own, so a cache kept in the repo could never answer the question there.
One `.\provision-refs.ps1` anywhere on the machine answers it for every checkout on it.

`Directory.Build.props` applies steps 1 to 4 itself, so a bare `dotnet build` resolves the
same install without going through PowerShell; only step 5 needs these scripts. This
install is only ever **read** from.

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
is compiled like any other, so a tree holding one is not the commit it claims - but a
stray `debug.log` at the root is not, and used to set the flag on every build of an
otherwise clean checkout, which is worth nothing to read. The build prints what made it
dirty, and names untracked files it deliberately did not count. A build with no `git`
available is stamped with nothing and says so.

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
   the plugin DLL plus the mod's own `Core`, `Persistence` and `Session` assemblies, each
   with its `.pdb`. BepInEx resolves a plugin's dependencies out of the plugin's own
   folder, so the DLL alone would load and then fail. Only `GlobalConversationTracker*`
   `.dll`/`.pdb` are deleted first, so anything else in that folder - the optional
   hand-placed `articy_ids_final_cut.json` above all - survives a redeploy.
5. prints the log path and the line to look for

**Target resolution:** `-GameDir`, else `DISCO_ELYSIUM_DEPLOY_DIR`, else the
auto-discovered Steam copy. It only stops and asks when all three come up empty, meaning
no override was given *and* no Steam install was found.

What actually keeps a stray deploy from doing damage is the guards, not the absence of a
default: the resolved target is printed before anything is written, the repo's
`Steam Install - *` reference copy is refused outright (`-AllowReferenceCopy` overrides),
a copy without BepInEx is rejected because the plugin could never load there, and the only
directory created or deleted is `<game>\BepInEx\plugins\GlobalConversationTracker`.
Uninstalling is deleting that one folder, so there is no uninstaller script to run.

### `make-release.ps1`

Builds and packages `.build\dist\GlobalConversationTracker-v<version>.zip`, laid out to
extract straight into a game folder:

```
BepInEx\plugins\GlobalConversationTracker\GlobalConversationTracker.dll
GlobalConversationTracker-README.md
```

One DLL, shipping with its `.pdb`: the mod's own layers are compiled into the plugin
assembly rather than referenced, so nothing installs beside it. It is the same payload
`deploy.ps1` installs, from the same helper. The version comes from `<Version>` in the
csproj.

It also writes a second archive, `GlobalConversationTracker-v<version>-AllInOne.zip`, for
players who do not already run mods:

```
winhttp.dll, doorstop_config.ini, .doorstop_version, dotnet\      <- BepInEx 6.0.0-be.688
BepInEx\core\, BepInEx\patchers\, BepInEx\plugins\               <- ...
BepInEx\plugins\GlobalConversationTracker\GlobalConversationTracker.dll
GlobalConversationTracker-README.md
GlobalConversationTracker-THIRD-PARTY.txt
Uninstall-GlobalConversationTracker.ps1
GlobalConversationTracker-install-manifest.json
```

Four things about it are worth knowing:

- **The BepInEx build is pinned**, to the same 6.0.0-be.688 the reference install runs, and
  the archive is downloaded from `builds.bepinex.dev`, checked against a pinned SHA256 and
  cached per machine under `%LOCALAPPDATA%\GlobalConversationTrackerepinex`. The pin, the
  URL and the hash are at the top of `build-support.psm1`; changing them means re-running
  the end-to-end install check, not just editing three lines.
- **BepInEx's own `changelog.txt` is left out**, because it sits at the root of its archive
  under a name the *game* already uses at the root of its folder. Everything else ships.
- **`BepInEx\interop` cannot be shipped.** Those assemblies are generated from the player's
  own game build on first launch, which is why that launch is slow.
- **The uninstaller is hash-checked.** `GlobalConversationTracker-install-manifest.json`
  lists every shipped file with its SHA256, and `Uninstall-GlobalConversationTracker.ps1`
  deletes a file only if it is still byte-for-byte what was installed. A BepInEx updated in
  place, an edited config, another mod's file at the same path: all left alone and reported.
  It supports `-WhatIf`, keeps the global state file unless `-RemoveGlobalState`, and keeps
  BepInEx's generated data unless `-RemoveBepInExData`.

`-PluginOnly` skips the bundle (and its download); `-BundleOnly` emits only the bundle.
Redistributing BepInEx is what `GlobalConversationTracker-THIRD-PARTY.txt` covers: it names
the build, its commit, the URL and the hash, and carries the LGPL-2.1 text fetched from that
same commit.

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

A generated name says nothing about the run it documents, so renaming one is the natural
thing to do - and renaming the `.log` by hand leaves `<old name>.log.capture.json` behind,
describing a file that is no longer there. `-Rename` moves both, rewrites the manifest's
`copy` to where the copy now is, and keeps the old path in `renamedFrom`. It refuses to
overwrite an existing name, and it needs no game folder, no log and no running process.

A capture is paired with its manifest **by name** - `<log>.capture.json` beside `<log>` -
and that is the pairing to rely on. `copy` is where the copy was when the manifest was
written, which is history, not a pointer. Each manifest also carries a `runId`
(`<capture timestamp>-<first 8 of the md5>`), so a manifest that has been separated from
its log can still be matched back to it by `md5` and `bytes`.

## Safety rules baked into the scripts

- **The repo's reference copy of the game is never written to.** `deploy.ps1` refuses any
  target path containing a `Steam Install - Unaltered` segment. (`-AllowReferenceCopy`
  overrides it with a warning.) Building only ever reads from a game install, never
  writes.
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

### No all-in-one archive bundling BepInEx

For a Mono game it is friendly to ship "BepInEx + plugin + uninstaller, extract and go".
For BepInEx 6 IL2CPP it is not: the archive would be far heavier, architecture-specific,
and still useless until the player runs the game once to generate their own interop
assemblies. Both game copies on this machine already have a working BepInEx (deployed by
Vortex). So the release is a plugin-only zip, and the README points at BepInEx's own
install instructions.

The knock-on effect on `deploy.ps1`: with no bundle to extract, it installs the payload
files directly, and there is deliberately no dormant extract-an-archive path waiting for
one. It still starts from a known state, clearing the previous build's
`GlobalConversationTracker*` files out of `<game>\BepInEx\plugins\GlobalConversationTracker\`
before copying, and it never touches anything above that folder.

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
| Reference copy (read-only) | `Steam Install - Unaltered\` |
