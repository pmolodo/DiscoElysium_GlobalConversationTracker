# Tools that were removed, and where to find them

Each of these answered a question, the answer was written down somewhere that outlives it, and
nothing in the repository referred to it any more. They are removed rather than kept because a
directory of tools nobody runs is a directory nobody reads.

**To get one back**, take it out of the last commit that held it:

```sh
git show 596a427:tools/<name> > tools/<name>
```

`596a427` is the commit before the removal. Everything below existed there, working.

## What went, and what it had answered

| tool | what it did | where the answer lives now |
|---|---|---|
| `conversation-starts.py` | Where every conversation in the game can be started, and what nothing explains. | `performance/group_list.rs` enumerates what is worth measuring, which is the question this fed. |
| `reachable-entries.py` | Which entries no conversation start reaches, walking links and ignoring guards. | The count it produced is quoted in `tools/measure-menus.py`'s help: of 1,422 conversations, 901 reach nothing from their start. |
| `suspect-test-content.py` | A heuristic listing of what looks like test content, evidence kept apart and weighed. | Nothing consumed it. It said on its own face that it was a heuristic. |
| `make-weather-saves.py` | Wrote the weather saves as the smallest change to a save already in the clear, rather than waiting for weather in game. | The saves it wrote are committed. This was the recipe. |
| `measure-probe-waits.py` | Totalled what a harness run spent waiting on the probe, by what it waited for, from a `GameHarness` log. | Nothing referred to it. The harness still writes the lines it read, so it can be brought back against a current log. |
| `measure-residue-arms.sh` | Ran `search_residue`'s four arrangements many times each and counted the deaths. | The arrangement table it settled is in `src/symbolic/isolated.rs`. |
| `read-party-answers.py` | Read the party truth table back, and said which flags `IsKimHere` actually reads - `PartyManager.IsKimHere` has no readable body in Final Cut. | `core::party` holds what it found. |
| `survey-diff-encoding.py` | What the committed sparse files look like byte for byte, for de-xz48.6.1: can a Rust writer reproduce them exactly? | It can; `crates/gct-save-files/src/sparse_diff.rs` is that writer, and its tests pin the format. |
| `survey-scene-entries.py` | What the scene-guarded entries of one conversation are, and who reaches them. | The scenario it was for exists. `tools/survey-scene-guards.py` remains and answers the wider question - which conversations guard on the scene. |

## What was deliberately kept, though nothing points at it

- **`driver-tests.sh`** is the test suite for the measurement drivers in this directory. Nothing
  refers to it, which is a gap in the docs rather than a reason to remove a suite.
- **`stop-measurements.sh`** stops a run and the processes that outlive the shell that started
  it. A whole-game measurement is long enough that wanting to stop one is a recurring need.
