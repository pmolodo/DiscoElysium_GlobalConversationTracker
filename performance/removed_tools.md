# Measurements that were removed, and where to find them

Each of these answered a question once. The number it produced is in its beads issue and in
its run log under `performance/logs`, which is why those logs are kept; the driver that
produced it was referred to by nothing but `Cargo.toml`.

**To get one back**, take it out of the last commit that held it, and put its `[[example]]`
entry back in `Cargo.toml`:

```sh
git show 596a427:performance/<name>.rs > performance/<name>.rs
```

`596a427` is the commit before the removal. Everything below existed there, working.

## What went, and what it had answered

| measurement | what it asked |
|---|---|
| `bound_slack.rs` | Where the structural bound's slack is, and what refusals and dominance can take off it. |
| `live_ranges.rs` | How much of the state is dead at the average entry. |
| `profile_closure.rs` | Whether a profile's globally-unseen set is a state any number of playthroughs could leave - de-5sdm, which asked whether a row taken on a fresh-save profile measures a world or a fiction. |

## What was kept, and why, though only `Cargo.toml` names it

A measurement is worth keeping where the question comes back. These do:

- **`counter_widths.rs`** says what each counter slot would cost under each of three encodings,
  and its own note says all three stay candidates - the writer encoding wins nothing in THIS
  dialogue set and is the one that pays where bonus amounts vary wildly. It answered de-qkng in
  September 2026, which is exactly the recurrence it was kept for.
- **`layout_slots.rs`** lists every slot a group's layout carries, by name. It is the tool for
  "is this thing a slot, and what is it called", which is asked whenever a slot-level
  optimisation is in question.
- **`graph_dot.rs`** draws a group as graphviz with nothing elided, for the questions whose
  answer is a shape rather than a number. `tools/render-dot.py` turns what it writes into a
  picture.
