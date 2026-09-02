# GlobalStateConvert

`GlobalStateConvert` migrates a version 1 or 2
`global-conversation-state.json` file from the legacy per-entry layout to the
current version 3 grouped layout. It preserves conversation statuses and, for a
version 2 input, shown orbs. A version 1 input has no orb data, so its output has
an empty `orbs` list.

From the repository root, run:

```powershell
dotnet run --project tools/GlobalStateConvert -- <input> <output>
```

Both paths are required. The converter does not modify the input and refuses to
overwrite an existing output. It exits with code 2 and does not produce an
output if the JSON is invalid, its format version is not 1 or 2, or any row
would be skipped by the legacy reader.

## Migration order

1. Exit Disco Elysium so the mod cannot write the global state during migration.
2. Back up the existing `global-conversation-state.json` from the game's
   `SaveGames` directory.
3. Convert the backup to a new output path.
4. Inspect the successful output if desired, then replace
   `global-conversation-state.json` with it.
5. Install or start the mod version that requires format version 3.

Convert before removing or upgrading away from legacy-reader support. Keep the
backup until the converted file has been loaded successfully in game.
