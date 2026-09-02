# LookAheadOffline

Runs the plugin's look-ahead engine against an extracted dialogue index without launching Disco Elysium.

Generate a current index from an AssetRipper export first:

```powershell
dotnet run --project tools/DialogueExtract -- conversation-index
```

That reads the exported `Disco Elysium.asset` and writes
`.game_reference_copies/derived/conversation_index.jsonl`; pass `--asset` and `--out` to
use other paths. The generated file is ignored by Git. Existing indexes with the former
always-null entry `actor` key remain readable, but rerun this command to produce the current
schema, where an entry's actor is available only in `fields.Actor`.

Then crawl every selectable entry in one conversation:

```powershell
dotnet run --project tools/LookAheadOffline -- --index .game_reference_copies/derived/conversation_index.jsonl --state state.json --conversation 451
```

Use `--entry 12` to crawl just entry 12 and `--state-budget 500000` to change the deterministic crawl limit.

The state file is JSON. `variables` and `queries` map names to booleans, numbers, or strings; `items`, `tasks`, `localSeen`, and `globalSeen` are arrays. Seen entries use `conversation:entry` strings. `localSeen` is the current save, and `globalSeen` is the union from all saves, so the output reports the same three novelty levels as the in-game plugin. `counterCaps` can set a proven per-variable saturation cap for an experimental crawl.

```json
{
    "money": 1000,
    "dayMinutes": 600,
    "dayCounter": 2,
    "clockLocked": false,
    "variables": { "met_kim": true, "counter": 3 },
    "items": ["shoes_faln"],
    "tasks": ["TASK.find_ruby"],
    "localSeen": ["451:12"],
    "globalSeen": ["451:12", "451:19"],
    "queries": { "IsKimHere": true },
    "counterCaps": { "pier.joyce_loopcounter": 5 }
}
```

Unknown world queries and passive checks intentionally remain permissive, matching the plugin's conservative behavior. The generated index preserves every entry field in `fields`, including the field-presence markers that determine special check behavior.
