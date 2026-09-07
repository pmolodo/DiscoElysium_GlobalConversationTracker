# FormatConvert

One tool for every format this repository writes. Point it at a file and it works
out **which format** the file is and **which version**, and writes the current
version of the same format. Neither is a flag, because both are already in the
file - that is what the `_format` discriminator and the version stamps are for,
and asking a caller to repeat them would be asking them to be able to get it
wrong.

```powershell
dotnet run --project tools/FormatConvert -- <input> [output]
```

With no output path it writes a sibling named for the version it produced -
`global-conversation-state.json` becomes `global-conversation-state.v4.json`.
**The input is never modified and an existing output is never overwritten.** It
exits with code 2 and writes nothing at all if the file cannot be read, is from a
newer build, or has a row the reader would have to drop.

A file **already at the current version** is left alone and reported as such, and
that is a success. Pointing this at a whole directory should not be a thing only
somebody who already knows the versions can do.

## What it recognises

| format | where it comes from |
|---|---|
| `global-conversation-state` | the mod's own state file, beside the saves |
| `sparse-diff` | a scenario save's diff over its base |
| `json-diff` | a diff between two JSON documents |
| `expanded-save-diff` | an expanded save folder's manifest |
| `dense`, `sparse` | the two shapes a split-out Lua table is written in |

Every one of those except the global state is at version 1, so there is nothing
older of them to convert yet. They are recognised anyway: the readers refuse
anything but the current version, and a refusal that names a converter which does
not recognise the file is a wall rather than an instruction.

## Why a converter rather than a legacy branch in the reader

A player's history should not depend on a code path nothing else exercises. A
converter is a place where an old shape is written down and **tested**; a legacy
branch inside a live reader is a place where one rots. So every reader accepts
only the version this build writes, and everything older comes through here.

The cost of that discipline showed up the first time it was tested: version 3 of
the global state was a shape this repository had written that **nothing could
read**. The runtime refused it and named the converter, and the converter refused
it as an unknown legacy version, because its bound was a constant that had not
been updated when version 4 arrived. It is now derived from the current version,
so there is nothing left to forget. See de-bnjy.7.

## Migrating the mod's global state

1. Exit Disco Elysium so the mod cannot write the global state during migration.
2. Back up the existing `global-conversation-state.json` from the game's
   `SaveGames` directory.
3. Convert the backup:

   ```powershell
   dotnet run --project tools/FormatConvert -- <backup>
   ```

4. Check the output opens and looks right, then put it in place of the original
   under its original name.
5. Start the game and confirm the mod loads the state without complaint.

If the tool exits 2, **do not delete the original**. Every refusal it makes is one
where the file is intact and something else is wrong - it is from a newer build,
or a row would have been lost - and in both cases the file is worth more than the
conversion.

## History

This replaces `GlobalStateConvert`, which did the same job for one format and took
both paths as required arguments. Its behaviours are the ones kept: never touch
the input, refuse to overwrite the output, exit 2 rather than write half a file.
