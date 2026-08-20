# Overall Goal

The goal is to develop a mod for Disco Elysium that tracks seen conversations
across all games/saves - a "Unified Conversation Tracker".  The system's save
game system already tracks seen conversation per save - I want to extend that
so that we track it across all games played / all saves.

# Game Editions

We desire to make a mod for the "Disco Elysium - Final Cut" version / edition,
which is a unity game that uses IL2CPP.

However, because IL2CPP makes recovering original source much more difficult,
I have also installed a copy of the pre-Final-Cut version, which used Unity +
Mono, which makes it easy to decompile it's associated scripts.  These are used
for reference only, as the assumption is that the vast majority of game logic is
the same, and most changes were additional content, tweaked dialogue, more audio
voice-overs, etc.

# File / Directory Layout

Notes about the file/directory layout of this project, and other relevant disk
locations:

## This repo

- Steam Install - Unaltered
  A point-in-time copy of my Steam install of the "Final Cut" game (the game
  files themselves are one level down, in a nested "Disco Elysium"
  subdirectory).  "Unaltered" means unaltered *since the copy was taken* - it
  is NOT a vendor-fresh install, and was never intended to be one.  By the
  time the copy was made I had already installed BepInEx 6.0.0-be.688 and the
  FuriousTare plugin via the Vortex mod manager, so the copy carries that
  whole BepInEx tree along with it, plus the usual companions - winhttp.dll,
  doorstop_config.ini, vortex.deployment.bepinex-injector.json, and some
  preloader_*.log files.  This is expected, and is not a problem: no game code
  differs anywhere (GameAssembly.dll is md5-identical to the playable Steam
  copy below), and BepInEx\interop is precisely what the mod builds against,
  so a modded copy is an asset here rather than a liability.
  There is no pristine/unmodded copy of the game on disk, and none is wanted.
  Should be left as-is; nothing in this project writes to it.
- AssetRipperExport
  The destination of the AssetRipper export of "Steam Install - Unaltered".
  The export was actually run against a second, throwaway copy of that install,
  in case AssetRipper altered its source; that copy was afterwards verified to
  be binary identical to "Steam Install - Unaltered" and deleted, so
  "Steam Install - Unaltered" is now the only reference copy in this repo.
  Contains exported assets, but since this is an export of an IL2CPP game,
  the scripts are mostly either missing or placeholders.
- Cpp2IL
  Outputs of running Cpp2IL tool, with various options, on the Final Cut version.
  Largely unsuccessful in terms of recreating C# source.
- pre-final-cut
  A copy of Disco Elysium, before the Final Cut version.  Uses Mono, not IL2CPP.
  Obtained by using the "DepotDownloader" tool - invoked via "depot_download_pre-final-cut.bat",
  then the resulting folder moved / renamed.
  Should be unaltered, but used as an AssetRipper export source.
- pre-final-cut-assetripper-export
  The destination of the AssetRipper export of "pre-final-cut".  Since the source
  is a Mono unity game, most script sources are recovered.  Though the possibility
  exists that the scripts in the Final Cut version may differ, I'm guessing
  most functionality that we care about is largely the same.
  - ExportedProject\Assets\Plugins
    Subdirectory holding Unity Plugins. Also used as the destianton for a run of
    ILSpy on DialogSystem.dll, resulting in the various PixelCrushers.DialogueSystem*
    subfolders, containing the decompiled source for DialogueSystem.dll

## Playable Steam install

- C:\Apps (x86)\Games\Steam\steamapps\common\Disco Elysium
  My actual, playable Steam install of "Final Cut" - the copy Steam updates,
  and the one the game is launched from.  It is modded in the same way as
  "Steam Install - Unaltered" above (BepInEx 6.0.0-be.688 + FuriousTare,
  deployed by Vortex), which is unsurprising, since that copy was taken from
  this install.  It has two roles in this project:
  - Build reference assemblies: provision-refs.ps1 sources BepInEx\core and
    the generated BepInEx\interop assemblies from here.  The interop
    assemblies cannot be downloaded - BepInEx generates them, which is why a
    modded install is required to build at all.
  - Deploy target: deploy.ps1 installs the mod into
    BepInEx\plugins\UnifiedConversationTracker here.
  Both scripts locate it automatically via Steam AppID 632470, so the path
  above is not hardcoded anywhere - it is recorded here for orientation only.

## Save file location

-  %UserProfile%\AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames

# Game Saves

For background - the game stores it's saves in %UserProfile%\AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames\{save_name}.ntwtf.zip files.
Each zip contains 5 files:
   {save_name}.1st.ntwtf.json
   {save_name}.2nd.ntwtf.json
   {save_name}.FOW.json
   {save_name}.ntwtf.lua
   {save_name}.states.lua

The conversation state data we are interested in is saved in {save_name}.ntwtf.lua.
The file consists of binary data representing 5 lua tables, and is read by:

    PixelCrushers.DialogueSystem.PersistentDataManager.ApplyRawData

The table we want is the 5th, "Conversation". It's relevant structure is
(pseudo-json):

```json
  "Conversation": {
    "<conversation-id-integer>": {
      ...
      "Dialog": {
        "<dialogue-entry-id-integer": {
          "SimStatus": "<one of Untouched|WasOffered|WasDisplayed>"
        },
      }
    },
```

An example expanded .ntwtf.zip file can be found at:

    %USERPROFILE%\AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames\MARTINAISE, DAY 1, 12-33(7_31_2026 3-55-56 AM).ntwtf

In that folder, the .ntwtf.json is a "decoded" version of the binary content in the .ntwtf.lua file.

# Existing Tooling / Python vs C#

In my initial exploration, I wrote some initial tooling using python - ie, read_ntwtf_lua_data.py.  This was written
in python because that is the language I am more familiar with - going forward, code + tools should be written in
C# (making use of `dotnet run somescript.cs` where necessary).

# Implementation - Rough Initial Plan

Details here may change, but my inital thoughts on implementation are:

- We will make use of BepinEx v6.0, which allows modding Unity games using
  IL2CPP
- We will need to override key functions that record changes to the "SimStatus"
  state of dialogue entries - a likely candidate is
  `PixelCrushers.DialogueSystem.DialogueLua.MarkDialogueEntry`, in
  `pre-final-cut-assetripper-export\ExportedProject\Assets\Plugins\PixelCrushers.DialogueSystem\DialogueLua.cs`.
  We modify it to keep track of a seperate "unified" / "global" dialogue state.
- This "unified" dialogue state is persisted on disk... somewhere. Ideally in
  the same directory in which save files are normally saved - on windows,
  %UserProfile%\AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames, but
  ideally, we use unity (or Disco Elysium) functionality to query the
  appropriate directory.  For initial implementation, the name of the save file
  should be constant / unalterable.
- If we have never populated the global dialogue state, we attempt to read it
  from this serialized file location. If it does not exist, we initialize by
  copying the current game's dialogue's simstatuses.
- Anytime we make a change to a dialgue simstatus, in addition to updating the
  "current-game" dialogue state, we update the "unified" dialogue state, using
  logic that a given dialgue entry's state can only be increased, where
  "Untouched" < "WasOffered" < "WasDisplayed". Any time we alter the unified
  state, we alter the in-memory copy, as well as overwriting the on-disk
  serialized copy.  We assume that this disk rewrite is reasonably fast - if
  not, we can revisit the design.
- We are not concerning ourselves with the possibility of concurrent writes - we
  assume there is only a single copy of Disco Elysium running at a time.  For
  speed, we never bother reading the copy on disk before overwriting.  The only
  time it is read is on the first access, when the unified dialog state has not
  been initialized.

# Related Tools / Repos

- BepinEx: https://github.com/bepinex/bepinex
- Disco Explorer Remastered: https://github.com/tparker48/Disco-Explorer-Remastered
  - A Disco Explorer mod that also uses BepinEx, and targets the Final Cut edition
  - Can be used as a reference on modding Disco Explorer / using BepinEx /
    modding an IL2CPP Unity game
- C:\Projects\Games\Marble World\Mods\ViewSelected
  - Location of a mod for another unity game (Marble World) that I made - some
    of it utility scripts may be useful:
    - **`provision-refs.ps1`**
    - **`build.ps1`**
    - **`make-release.ps1`**
    - **`deploy.ps1`**
  - See it's `DEVLEOPING.md` scripts for details on their usage.
