# Overall Goal

The goal is to develop a mod for Disco Elysium that tracks seen conversations
across all games/saves - a "Global Conversation Tracker".  The system's save
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

Everything the project reads but does not author - copies of the game, exports of
its assets, decompiler output - lives in one gitignored folder:

- .game_reference_copies
  Read-only reference material, tens of gigabytes of it, none of it ours to
  redistribute and all of it reproducible. Gitignored as a folder rather than by
  listing each copy, so a copy added tomorrow is out of git and refused as a
  deploy target without anyone updating a list.

  - steam_<build date>_<edition>_<manifest id>
    A copy of the game straight from Steam's content servers, fetched by
    depot_download.ps1 and named after the version it is. PRISTINE: no BepInEx,
    no Vortex, none of the files the Steam client writes into an install. That
    is what makes it useful - a modded install cannot tell you which files the
    game actually ships, and this one has already settled that question twice
    (changelog.txt, winhttp.dll, doorstop_config.ini and dotnet\ are BepInEx's,
    not the game's). Each carries a steam-download.json saying what it is and
    how to fetch it again.
    - "latest" is the current public build.
    - "pre-final-cut" is manifest 3499130543868275315, built 2021-02-11, the
      last content before The Final Cut. It uses Mono rather than IL2CPP, which
      is why its AssetRipper export recovers real script sources.

  - AssetRipperExport
    An AssetRipper export of the Final Cut game. Contains exported assets, but
    since this is an export of an IL2CPP game, the scripts are mostly either
    missing or placeholders.

  - pre-final-cut-assetripper-export
    An AssetRipper export of the pre-Final-Cut game. Since that source is a Mono
    Unity game, most script sources are recovered. The Final Cut version's
    scripts may differ, but most functionality we care about is likely the same.
    - ExportedProject\Assets\Plugins
      Unity Plugins, and the destination of a run of ILSpy on DialogueSystem.dll -
      the PixelCrushers.DialogueSystem* subfolders hold that decompiled source.

  - Cpp2IL
    Output of running the Cpp2IL tool, with various options, on the Final Cut
    version. Largely unsuccessful at recreating C# source.

Two copies that used to sit at the repo root were removed on 2026-08-26, and
neither is missed:

- "Steam Install - Unaltered", a point-in-time copy of the modded Steam install.
  Its build role is covered by the playable Steam install below, which is the
  copy that actually gets patched and re-run; its reference role is covered by a
  pristine steam_* download, which is the better answer for "what does the game
  ship" precisely because it is not modded.
- "pre-final-cut", fetched by a depot_download_pre-final-cut.bat that has since
  been folded into depot_download.ps1. Re-fetch it with
  `.\depot_download.ps1 pre-final-cut -Username <steam account>`.

## Playable Steam install

- C:\Apps (x86)\Games\Steam\steamapps\common\Disco Elysium
  My actual, playable Steam install of "Final Cut" - the copy Steam updates,
  and the one the game is launched from.  It is modded with BepInEx
  6.0.0-be.688 + FuriousTare, deployed by Vortex.  It has two roles in this
  project:
  - Build reference assemblies: it is where the generated BepInEx\interop
    assemblies come from.  Those cannot be downloaded - BepInEx generates them
    from the game's own GameAssembly.dll - which is why a modded install is
    required to build THE PLUGIN.  BepInEx\core is a different matter: it is a
    published archive, so the build fetches the pinned one itself (see
    BepInEx.props), and every other project builds with no game install at all.
  - Deploy target: deploy.ps1 installs the mod into
    BepInEx\plugins\GlobalConversationTracker here.
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
  We modify it to keep track of a seperate "global" / "global" dialogue state.
- This "global" dialogue state is persisted on disk... somewhere. Ideally in
  the same directory in which save files are normally saved - on windows,
  %UserProfile%\AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames, but
  ideally, we use unity (or Disco Elysium) functionality to query the
  appropriate directory.  For initial implementation, the name of the save file
  should be constant / unalterable.
- If we have never populated the global dialogue state, we attempt to read it
  from this serialized file location. If it does not exist, we initialize by
  copying the current game's dialogue's simstatuses.
- Anytime we make a change to a dialgue simstatus, in addition to updating the
  "current-game" dialogue state, we update the "global" dialogue state, using
  logic that a given dialgue entry's state can only be increased, where
  "Untouched" < "WasOffered" < "WasDisplayed". Any time we alter the global
  state, we alter the in-memory copy, as well as overwriting the on-disk
  serialized copy.  We assume that this disk rewrite is reasonably fast - if
  not, we can revisit the design.
- We are not concerning ourselves with the possibility of concurrent writes - we
  assume there is only a single copy of Disco Elysium running at a time.  For
  speed, we never bother reading the copy on disk before overwriting.  The only
  time it is read is on the first access, when the global dialog state has not
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
