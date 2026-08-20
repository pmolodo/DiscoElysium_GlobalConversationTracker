# UnifiedConversationTracker plugin

BepInEx plugin for Disco Elysium - The Final Cut. It Harmony-patches
`DialogueLua.MarkDialogueEntry`, the game's single write funnel for dialogue SimStatus, and
records every status it sees into a unified across-all-saves state file in the SaveGames
directory (`unified-conversation-state.json`).

The patch is a postfix, so the game's own per-save behavior runs first and unmodified, and
nothing is ever read back into the game: the unified state is write-only. The state file is
read from disk once per session, on the first mark, and rewritten whenever a mark raises a
status. A status can only ever go up, so nothing the game does - including resetting a save
to Untouched - can lose recorded history.

## Target environment

Values observed in the BepInEx installation under
`Steam Install - Unaltered/Disco Elysium/BepInEx`:

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
3. default: `<repo root>\Steam Install - Unaltered\Disco Elysium`

The default is read-only reference use; nothing is ever written into that directory.

Normally you do not run `dotnet` by hand: `build.ps1` at the repo root resolves the game
directory for you and passes it in. See [DEVELOPING.md](../../DEVELOPING.md). The direct
equivalent is:

```
dotnet build src\UnifiedConversationTracker.Plugin -c Release
```

Output: `.build\bin\UnifiedConversationTracker.Plugin\Release\net6.0\`, holding
`UnifiedConversationTracker.dll` and the mod's own libraries next to it
(`UnifiedConversationTracker.Core/.Persistence/.Session.dll`), each with a `.pdb`;
`Directory.Build.props` redirects `bin`/`obj` under `.build`. An installed plugin is all four
DLLs: BepInEx resolves a plugin's dependencies out of the plugin's own folder, so the entry
point DLL alone would load and then fail on the first line of dialogue.

## Installing and verifying

`deploy.ps1` at the repo root does steps 1-3 below in one command; see
[DEVELOPING.md](../../DEVELOPING.md). The manual equivalent:

No BepInEx installation is needed: a working 6.0.0-be.688 loader is already present in
`Steam Install - Unaltered\Disco Elysium\BepInEx`, and its `LogOutput.log` shows a third-party
IL2CPP plugin (`plugins\FuriousTare`) loading and Harmony-patching the game successfully. So
plugin loading in this build is already proven; only this plugin is unproven.

Pick the game copy to install into (`<game>` below):

- `D:\Downloads\Apps\Games\Disco Elysium\Decompilation\Steam Install - Unaltered\Disco Elysium`
  - has the working BepInEx loader, so nothing else to set up
  - despite the name it is not actually pristine (it already carries BepInEx and a third-party
    plugin); which copy counts as clean is tracked separately in de-omm.13
  - installing here means writing into that directory, which is otherwise treated as read-only
- `C:\Apps (x86)\Games\Steam\steamapps\common\Disco Elysium` - the playable Steam copy the
  existing `LogOutput.log` was produced from; use this if the directory above must stay untouched

Steps:

1. Build as above.
2. Create the plugin folder:
   `<game>\BepInEx\plugins\UnifiedConversationTracker\`
3. Copy every built `UnifiedConversationTracker*.dll` into that folder (see Building above).
4. Confirm the BepInEx console is on: in
   `<game>\BepInEx\config\BepInEx.cfg`, section `[Logging.Console]`, `Enabled = true`.
5. Launch the game (Steam, or `disco.exe` directly).
6. In the BepInEx console - or afterwards in `...\Disco Elysium\BepInEx\LogOutput.log` - look for
   these two lines:

   ```
   [Info   :   BepInEx] Loading [UnifiedConversationTracker 0.1.0]
   [Message:UnifiedConversationTracker] UnifiedConversationTracker v0.1.0 loaded.
   [Message:UnifiedConversationTracker] Unified state file: ...\SaveGames\unified-conversation-state.json
   [Message:UnifiedConversationTracker] Hooked DialogueLua.MarkDialogueEntry; dialogue statuses are being tracked.
   ```

   The second line proves the plugin's entry point ran; the fourth proves the hook is on. With
   HarmonyX logging enabled there is also a
   `[Info :HarmonyX] Patching void PixelCrushers.DialogueSystem.DialogueLua::MarkDialogueEntry(...)`
   line. Nothing touches the state file until the first line of dialogue is marked, or a
   savegame is loaded, whichever comes first: that is when the file is read and the first
   write happens.
7. To uninstall, delete the `UnifiedConversationTracker` folder from `BepInEx\plugins`.

If the plugin does not appear at all, check `BepInEx\LogOutput.log` for a load error and confirm
the DLL was built against the same BepInEx build as the one installed in that game copy.
