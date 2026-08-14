# Developing

Build, deploy and packaging for **UnifiedConversationTracker**, the BepInEx plugin for
Disco Elysium - The Final Cut. The plugin's own notes live in
[src/UnifiedConversationTracker.Plugin/README.md](src/UnifiedConversationTracker.Plugin/README.md).

## TL;DR

```powershell
.\deploy.ps1     # build + install into your Steam copy
```

Then launch the game. That is the whole iterate loop: edit -> `.\deploy.ps1` -> relaunch.

## Requirements

- Windows PowerShell 5.1 (what ships with Windows) or newer
- the .NET SDK (`dotnet`) - 10.0.400 was used; the plugin targets `net6.0`
- a Disco Elysium install with **BepInEx 6.0.0-be.688 (IL2CPP / CoreCLR)** already set up
  **and run at least once**, so `<game>\BepInEx\interop` holds the generated interop
  assemblies

## The scripts

Four PowerShell scripts at the repo root, each runnable directly. They dot-source each
other, so each reuses the previous one's constants and functions:

`deploy.ps1` / `make-release.ps1` -> `build.ps1` -> `provision-refs.ps1`

### `provision-refs.ps1`

Locates and verifies the game install the **build reads reference assemblies from**, and
checks that every DLL the csproj lists is actually there - so a missing reference fails
with one clear sentence instead of a wall of MSBuild errors.

Resolution order:

1. `-DiscoElysiumDir <path>`
2. the `DISCO_ELYSIUM_DIR` environment variable
3. `<repo>\Steam Install - Unaltered\Disco Elysium` (the csproj's own default)
4. Steam auto-discovery: registry `Valve\Steam` + `libraryfolders.vdf`, AppID **632470**,
   cached to `.build\cache\reference-game-dir.txt`

Step 4 is why the build works from a git worktree, where the repo-local reference copy in
step 3 is not checked out. This install is only ever **read** from.

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
4. deletes any previous `<game>\BepInEx\plugins\UnifiedConversationTracker` and copies the
   fresh `.dll` + `.pdb` in
5. prints the log path and the line to look for

**Target resolution:** `-GameDir`, else `DISCO_ELYSIUM_DEPLOY_DIR`, else the
auto-discovered Steam copy. It only stops and asks when all three come up empty, meaning
no override was given *and* no Steam install was found.

What actually keeps a stray deploy from doing damage is the guards, not the absence of a
default: the resolved target is printed before anything is written, the repo's
`Steam Install - *` reference copies are refused outright (`-AllowReferenceCopy` overrides),
a copy without BepInEx is rejected because the plugin could never load there, and the only
directory created or deleted is `<game>\BepInEx\plugins\UnifiedConversationTracker`.
Uninstalling is deleting that one folder, so there is no uninstaller script to run.

### `make-release.ps1`

Builds and packages `.build\dist\UnifiedConversationTracker-v<version>.zip`, laid out to
extract straight into a game folder:

```
BepInEx\plugins\UnifiedConversationTracker\UnifiedConversationTracker.dll
BepInEx\plugins\UnifiedConversationTracker\UnifiedConversationTracker.pdb
UnifiedConversationTracker-README.md
```

The version comes from `<Version>` in the csproj.

## Safety rules baked into the scripts

- **The repo's reference copies of the game are never written to.** `deploy.ps1` refuses
  any target path containing a `Steam Install - Unaltered` or
  `Steam Install - AssetRipperSource` segment. (`-AllowReferenceCopy` overrides it with a
  warning, but the state of those copies is under question - see de-omm.13.) Building only
  ever reads from a game install, never writes.
- **No silent deploy default** - see `deploy.ps1` above.
- **The BepInEx config is never edited.** If `[Logging.Console] Enabled` is not `true`,
  deploy just says so and moves on.

## Verifying a deploy

Only the game itself can confirm the plugin loads.

1. Launch the game (Steam, or `disco.exe` directly).
2. Watch the BepInEx console, or read `<game>\BepInEx\LogOutput.log` afterwards, for:

   ```
   [Info   :   BepInEx] Loading [UnifiedConversationTracker 0.1.0]
   [Message:UnifiedConversationTracker] UnifiedConversationTracker v0.1.0 loaded.
   ```

   The second line is the one that proves the plugin's entry point ran.
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

### No all-in-one archive bundling BepInEx (deferred to phase 3, de-4pp.1)

For a Mono game it is friendly to ship "BepInEx + plugin + uninstaller, extract and go".
For BepInEx 6 IL2CPP it is not: the archive would be far heavier, architecture-specific,
and still useless until the player runs the game once to generate their own interop
assemblies. Both game copies on this machine already have a working BepInEx (deployed by
Vortex). So phase 1 ships a plugin-only zip, and the README points at BepInEx's own
install instructions.

The knock-on effect on `deploy.ps1`: the prior-art script it was adapted from installs *by
extracting its all-in-one zip* into the game folder. With no bundle, deploy installs the
DLL directly instead, and there is deliberately no dormant extract-an-archive path waiting
for one. It keeps that design's useful half - start from a known state - by removing
`<game>\BepInEx\plugins\UnifiedConversationTracker\` before copying, and it never touches
anything above that folder.

### Build output layout

`Directory.Build.props` redirects `bin` and `obj` to `.build\bin` and `.build\obj`; the
scripts put `cache`, `stage` and `dist` there too. One gitignored `.build\` folder holds
everything generated, so `src\` stays clean.

## Important paths

| What | Where |
| --- | --- |
| Plugin project | `src\UnifiedConversationTracker.Plugin\` |
| Built DLL | `.build\bin\UnifiedConversationTracker.Plugin\<Configuration>\net6.0\UnifiedConversationTracker.dll` |
| Release zip | `.build\dist\UnifiedConversationTracker-v<version>.zip` |
| Installed plugin | `<game>\BepInEx\plugins\UnifiedConversationTracker\` |
| BepInEx log | `<game>\BepInEx\LogOutput.log` |
| BepInEx config | `<game>\BepInEx\config\BepInEx.cfg` |
| Playable Steam copy | `C:\Apps (x86)\Games\Steam\steamapps\common\Disco Elysium` |
| Reference copies (read-only) | `Steam Install - Unaltered\`, `Steam Install - AssetRipperSource\` |
