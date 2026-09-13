# GlobalConversationTracker plugin

BepInEx plugin for Disco Elysium - The Final Cut. It Harmony-patches the game's two SimStatus
writers and its orb writer, and records everything it sees into a global across-all-saves state
file in the SaveGames directory (`global-conversation-state.json`):

- `DialogueLua.MarkDialogueEntry` - the game's single write funnel for dialogue SimStatus
  while playing. The patch is a postfix, so the game's own per-save behavior runs first and
  unmodified.
- `PersistentDataManager.ApplyRawData` - loading a savegame rebuilds the whole SimStatus table
  without going through `MarkDialogueEntry`. The patch is a prefix that reads the raw save
  bytes the game is about to apply and resyncs the global state from them.

- `SenseOrb.SetShown` - the single funnel through which the game records an opened orb, by
  writing `ShownOrbs[conversation] = {OrbSeen=1}`. Every click path reaches it, and it is not
  virtual, so one patch covers `SenseOrb`, `ConditionalSenseOrb` and `VisCalOrb` alike. The
  patch is a postfix. An orb with no conversation is skipped rather than reported: `SetShown`
  itself declines to write one, which is why a thought orb - instantiated from the orb template
  by `GlobalOrbManager.AddThought`, which never sets its conversation - leaves no trace in
  `ShownOrbs` and is not counted here either.

- `SunshinePersistence.SaveGameLoadedData.GetString` - loading a savegame restores its orbs
  by executing the save's `{save}.states.lua`, a Lua script rather than data, so there is no
  parsed form to intercept the way `ApplyRawData` offers one for dialogue. The postfix reads
  that file's text as it is pulled out of the in-memory save, parses the
  `ShownOrbs["title"]={OrbSeen=1};` rows, and resyncs from them - replacing the current save's
  orbs and merging into the across-all-saves state. Calls for the save's other files are
  ignored by name. Reading the text rather than the live `ShownOrbs` table keeps the parsing
  testable against real save files and avoids depending on whether the game has executed the
  script yet.

A further hook covers the one event none of the writers above can see:

- `World.ResetStates` - a new game rebuilds the whole SimStatus table at once rather than
  marking entries, so nothing else tells the mod that the save being played has started over.
  The postfix resets the current-save tally only. The across-all-saves state is never reset by
  anything; a new game is what it exists to survive.

It also patches the main HUD so the tracked totals are visible in game from the first frame of
a playthrough:

- `HudMoneyController.Start` - the money display's own startup, in the live HUD the `Init`
  scene builds. The postfix adds two rows under that display: how many dialogue entries have
  been reached in the save being played, and how many across all saves. They sit directly
  under the money, right aligned with its digits and in its own font, and are placed off the
  money display's rect as measured at runtime. That rect lives in a panel pinned to the
  bottom-right corner, which keeps its shape at any screen width, so they land in the same
  place at any aspect ratio. Hanging them off the money display is what makes them fade with the
  HUD: each HUD element fades itself, and the panel they share never does. `HudCountOffsetX`
  and `HudCountOffsetY` in the plugin's config file nudge the pair from there.

Both numbers are scores rather than plain counts. An entry the game only ever marked
`WasOffered` - listed as a response option, never actually shown - is worth 0.5, and one marked
`WasDisplayed` is worth 1.0; reaching a line that was already offered is the other half of the
same entry and not a second one. An opened orb is worth 1.0, for the same reason a displayed
entry is: it is text actually reached. There is no half for an orb, because the game records a
bare `OrbSeen=1` and nothing between. The decimal place is shown only when the total ends in .5,
which is the only fraction a total can end in, so a whole score reads as the plain count it is.

The layout is one number per line, this save above and all saves below, the two numbers right
aligned with each other and the two icons in a column left of whichever number is wider - a
speech bubble for this save, a globe for all saves. Both are white PNGs embedded in the plugin
(`Resources\current-save-count-icon.png` and `Resources\all-saves-count-icon.png`), tinted to
the counts' colour at runtime, and neither is a character: every font asset the game ships is a
static atlas whose highest codepoint is U+FF70, emoji start at U+1F300, and the project's TMP
sprite asset is TextMesh Pro's fourteen-smiley EmojiOne sample. An emoji character would draw
as nothing. If an icon cannot be decoded its count is shown without it.

Nothing polls: the hooks above tell the display when a count has moved, which is the
only time one can have.

Each hook is installed independently, so one failing to patch costs only what that hook
covered. Nothing is fed back into the game's own state: the global state is read only to draw
the numbers, never to change what the game records. The state file is read from disk once per
session, on the first mark or savegame load, and rewritten whenever a mark raises a status. A
status can only ever go up, so nothing the game does - including resetting a save to Untouched
- can lose recorded history.

## Target environment

The BepInEx 6 installation this plugin targets:

| Item | Value |
| --- | --- |
| BepInEx | 6.0.0-be.688 (IL2CPP / CoreCLR) |
| Unity | 2020.3.12f1 |
| Plugin runtime | .NET 6.0 (`net6.0`) |
| Interop assemblies | `<game>\BepInEx\interop` (already generated) |

## Building

The project references assemblies from an existing BepInEx installation, so it needs to know
where the game lives. Resolution order:

1. `-p:DiscoElysiumDir=<path>` on the command line
2. the `DISCO_ELYSIUM_DIR` environment variable
3. the machine-level cache `.\provision-refs.ps1` writes, which is also the only step that
   can find your Steam copy

Every candidate must actually contain `BepInEx\core` and `BepInEx\interop`; a game install
without BepInEx cannot supply the assemblies this project references. Whatever resolves is
read from and never written to.

Normally you do not run `dotnet` by hand: `build.ps1` at the repo root resolves the game
directory for you and passes it in. See [DEVELOPING.md](../../DEVELOPING.md). The direct
equivalent is:

```
dotnet build src\GlobalConversationTracker.Plugin -c Release
```

Output: `.build\bin\GlobalConversationTracker.Plugin\Release\net6.0\`, holding
`GlobalConversationTracker.dll` and its `.pdb`; `Directory.Build.props` redirects `bin`/`obj`
under `.build`. The `.pdb` is not installed or packaged - it stays here, for debugging the
build you just made. That DLL is all of the mod's own code: the layers (Core, Persistence,
Session, Engine) are compiled into it rather than referenced as projects. The projects still
build and are tested on their own; the plugin csproj says why it takes them as source.

Beside it travel the look-ahead engine, the conversation index, the variable table, the
engine's build stamp, and `Google.Protobuf.dll`. The first four are named
`GlobalConversationTracker`-something so that deploy, packaging and the uninstaller handle
them without being told they exist; protobuf keeps its own name, because .NET resolves an
assembly by its identity rather than by its filename, so `Get-PluginPayloadFile` names it
outright.

## Installing

Two archives are published per release, and which one you want depends on whether you
already run mods in this game.

**`GlobalConversationTracker-v<version>-AllInOne.zip`** - if you do not. It carries
BepInEx 6.0.0-be.688 (IL2CPP) along with the plugin, so it is the whole install:

1. Extract it into the game folder, the one with `disco.exe` in it. On Steam that is
   usually `steamapps\common\Disco Elysium`; right-click the game, Manage, Browse local
   files.
2. Launch the game. **The first launch is slow** - a minute or more on a black screen -
   because BepInEx generates the IL2CPP interop assemblies from your copy of the game
   before anything loads. That happens once. Later launches are normal.
3. The dialogue counts appear at the bottom of the screen, to the left of the money,
   once you are in the game world.

To remove it again, run `Uninstall-GlobalConversationTracker.ps1` from that same folder
(right-click, Run with PowerShell). It deletes only the files the archive wrote, and only
while they still hold the bytes the archive wrote, so a BepInEx that has since been updated
or a file you edited is left alone and reported rather than removed. Your dialogue history -
`global-conversation-state.json`, in the SaveGames folder - is kept unless you pass
`-RemoveGlobalState`. `-WhatIf` shows you the whole run without changing anything.

Neither archive can ship the `BepInEx\interop` assemblies: those are generated from your
own game build on first launch, which is the slow start-up above.

**`GlobalConversationTracker-v<version>.zip`** - if you already have BepInEx 6 IL2CPP
working. It is the plugin alone: extract it into the game folder and the DLL lands in
`BepInEx\plugins\GlobalConversationTracker\`.

## Installing from a build, and verifying

`deploy.ps1` at the repo root does steps 1-3 below in one command; see
[DEVELOPING.md](../../DEVELOPING.md). The manual equivalent:

Pick the game copy to install into (`<game>` below). It needs BepInEx 6 installed and run
once already, so that both `BepInEx\core` and `BepInEx\interop` exist: the plugin cannot load
without a loader, and the all-in-one archive above is how a copy that has none gets one.

The everyday target is the playable Steam copy, usually
`steamapps\common\Disco Elysium`, which is what `deploy.ps1` resolves to on its own when
neither `-GameDir` nor `DISCO_ELYSIUM_DEPLOY_DIR` names one.

The read-only reference copies under `.game_reference_copies` are not deploy targets.
`deploy.ps1` refuses any path under that folder unless `-AllowReferenceCopy` is given. A
copy to build *against* is a separate question from a copy to install *into* - see
`-DiscoElysiumDir` versus `-GameDir` in [DEVELOPING.md](../../DEVELOPING.md).

Steps:

1. Build as above.
2. Create the plugin folder:
   `<game>\BepInEx\plugins\GlobalConversationTracker\`
3. Copy the built `GlobalConversationTracker.dll` into that folder (see Building above).
   The payload is that one DLL; delete any other `GlobalConversationTracker*.dll` in there.
4. Confirm the BepInEx console is on: in
   `<game>\BepInEx\config\BepInEx.cfg`, section `[Logging.Console]`, `Enabled = true`.
5. Launch the game (Steam, or `disco.exe` directly).
6. In the BepInEx console - or afterwards in `...\Disco Elysium\BepInEx\LogOutput.log` - look for
   these lines:

   ```
   [Info   :   BepInEx] Loading [GlobalConversationTracker 0.1.0]
   [Message:GlobalConversationTracker] GlobalConversationTracker v0.1.0 loaded.
   [Message:GlobalConversationTracker] Global state file: ...\SaveGames\global-conversation-state.json
   [Message:GlobalConversationTracker] Hooked DialogueLua.MarkDialogueEntry; dialogue statuses are being tracked.
   [Message:GlobalConversationTracker] Hooked PersistentDataManager.ApplyRawData; the global state is resynced whenever a savegame is loaded (using raw file bytes).
   [Message:GlobalConversationTracker] Hooked World.ResetStates; the current save's dialogue count is reset when a new game starts.
   [Message:GlobalConversationTracker] Hooked HudMoneyController.Start; the main HUD shows how many dialogue entries have been reached, in this save and across all saves.
   [Message:GlobalConversationTracker] Shutdown flush registered on Application.quitting and AppDomain.ProcessExit. ...
   ```

   The `v0.1.0 loaded` line proves the plugin's entry point ran; the `Hooked` lines prove
   the hooks are on. A hook that could not be installed logs `Failed to hook <method>` instead,
   and the others carry on without it. The counts themselves show up once a game is in play, to
   the left of the money at the bottom of the screen, and log a `Dialogue counts added to the
   main HUD` line. With HarmonyX logging enabled there is also an `[Info :HarmonyX] Patching ...`
   line per patched method. Nothing touches the state file until the first line of dialogue is
   marked, or a savegame is loaded, whichever comes first.
7. To uninstall, delete the `GlobalConversationTracker` folder from `BepInEx\plugins`.

If the plugin does not appear at all, check `BepInEx\LogOutput.log` for a load error and confirm
the DLL was built against the same BepInEx build as the one installed in that game copy.
