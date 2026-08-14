# UnifiedConversationTracker plugin

BepInEx plugin skeleton for Disco Elysium - The Final Cut. At this stage it does exactly one
thing: log a line when BepInEx loads it. No hooks, no persistence, no tracking logic yet.

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

Output: `.build\bin\UnifiedConversationTracker.Plugin\Release\net6.0\UnifiedConversationTracker.dll`
(plus a `.pdb`; `Directory.Build.props` redirects `bin`/`obj` under `.build`). That single DLL
is the whole plugin.

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
3. Copy the built `UnifiedConversationTracker.dll` into that folder.
4. Confirm the BepInEx console is on: in
   `<game>\BepInEx\config\BepInEx.cfg`, section `[Logging.Console]`, `Enabled = true`.
5. Launch the game (Steam, or `disco.exe` directly).
6. In the BepInEx console - or afterwards in `...\Disco Elysium\BepInEx\LogOutput.log` - look for
   these two lines:

   ```
   [Info   :   BepInEx] Loading [UnifiedConversationTracker 0.1.0]
   [Message:UnifiedConversationTracker] UnifiedConversationTracker v0.1.0 loaded.
   ```

   The second line is the one that proves the plugin's entry point ran.
7. To uninstall, delete the `UnifiedConversationTracker` folder from `BepInEx\plugins`.

If the plugin does not appear at all, check `BepInEx\LogOutput.log` for a load error and confirm
the DLL was built against the same BepInEx build as the one installed in that game copy.
