# Measurements that were removed, and where to find them

Each of these answered a question once. The number it produced is in its beads issue, in the
comment that states it, and in its run log under `performance/logs`, which is why those logs
are kept. What went is the recipe, and it is in git.

**To get one back**, take it out of the commit named in its row and put it under
`crates/gct-measure/examples/`, where Cargo finds an example unasked:

```sh
git show <commit>:<path> > crates/gct-measure/examples/<name>.rs
```

Every path below existed and worked at the commit beside it.

## What went, and what it had answered

| measurement | what it asked | take it from |
|---|---|---|
| `bidirectional_headroom` | What a forward front costs per layer, against the backward one the search already walks - the argument for meeting in the middle. | `58bb39f:crates/gct-measure/examples/bidirectional_headroom.rs` |
| `bound_slack` | Where the structural bound's slack is, and what refusals and dominance can take off it. | `596a427:performance/bound_slack.rs` |
| `clock_saves` | Which committed saves have been through a group that carries a clock, which is what decides whether an in-game scenario exercising one can be built from what is here. 16 of the 39 have displayed entries in group 631 - `at-evart` 731 of them - and most have a few in 1260; `before-the-deserter` has 295 in 1030 and 687 in 14. So it can be built. See de-086x for why it has not been. | `22cc546:crates/gct-measure/examples/clock_saves.rs` |
| `clock_worth` | What a carried clock BUYS, against what it costs. A save's clock is read to the hour so a walk starts at `:00`, and a `PassTime` is fifteen minutes - so four steps are needed before any hour question moves. Of the 9 groups that carry a clock, 8 can change an answer and most at every starting hour; the one that cannot is 566, which chains two steps and so never leaves the hour it started in. | `903a3ca:crates/gct-measure/examples/clock_worth.rs` |
| `clock_groups` | How many of the ENGINE's own groups carry a clock, asked of `build_group_graph` rather than of a partition it does not use. Of 1,422 groups, 38 can move the clock and 9 can read it, so 29 carry none; the nine pay 55 variables - one at two bits, one at four, seven at seven - where a minute of the day would have been 99. | `849bb5e:crates/gct-measure/examples/clock_groups.rs` |
| `clock_cost` | What carrying the clock costs the GUARDS, over the eight groups that pass time and ask the hour. Held as a minute of the day it was eleven variables and roughly TWICE the diagram nodes; held as `PassTime` steps it is one to seven variables and about a tenth more nodes - 631 goes 609 to 721 where it went 609 to 1,498, and 566 goes 23 to 25 where it went 23 to 856. No verdict moved under either. It does not price a search. | `6344c47:crates/gct-measure/examples/clock_cost.rs` |
| `group_census` | How many DISTINCT groups the game has: 1,372 of 1,422 are one conversation, and the other 50 carry fifty-five per cent of the entries. | `58bb39f:crates/gct-measure/examples/group_census.rs` |
| `guard_depth` | How deeply nested the deepest guard in the shipped database is. Eleven levels, of 26,210. | `58bb39f:crates/gct-measure/examples/guard_depth.rs` |
| `live_ranges` | How much of the state is dead at the average entry. | `596a427:performance/live_ranges.rs` |
| `manager_memory` | What a diagram node really costs once the unique table has grown - the figure `DiagramBudget::BYTES_PER_NODE` is derived from. About 15.7 bytes, flat from six million nodes to twenty-three. | `58bb39f:crates/gct-measure/examples/manager_memory.rs` |
| `onward_or_back` | Whether an option reaches unread content WITHOUT coming back through the menu - a binary marking, proposed against the exact-distance one. | `58bb39f:crates/gct-measure/examples/onward_or_back.rs` |
| `per_start_setup` | What a menu pays PER OPTION for work that depends on the graph alone. 246 ms a menu of 24 starts on conversation 631. | `58bb39f:crates/gct-measure/examples/per_start_setup.rs` |
| `profile_closure` | Whether a profile's globally-unseen set is a state any number of playthroughs could leave - de-5sdm. | `596a427:performance/profile_closure.rs` |
| `repeat_question` | What a SECOND question about the same group costs, split three ways: the graph, the diagram side, the search. Eighteen to twenty-seven milliseconds of setup a request. | `58bb39f:crates/gct-measure/examples/repeat_question.rs` |
| `start_relative_layout` | How much smaller a layout would be if built from where a query starts, at all three granularities. | `58bb39f:crates/gct-measure/examples/start_relative_layout.rs` |
| `target_cost` | What it costs to prove ONE target unreachable, a target at a time. | `58bb39f:crates/gct-measure/examples/target_cost.rs` |
| `unread_slots` | How many of the slots a search carries anything ever READS. Between a quarter and nearly a half are never read. | `58bb39f:crates/gct-measure/examples/unread_slots.rs` |

## What was kept, and why, though only a comment names it

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
- **`guard_stack.rs`** is not a measurement at all: it asks whether anything that walks a guard
  still costs stack in proportion to depth, on a one-megabyte stack. A guard comes out of a
  database a patch or another mod can change, and a stack overflow ends the process rather than
  panicking. The suite cannot ask it - a test thread's stack is generous - so nothing else can
  perform this check, and it comes back with every change to code that walks a guard.
- **`permissive_census.rs`** is a live fixture of a settled design rather than an orphan.
  `world::IVariableTable` keeps Unknown representable BECAUSE this tool needs a world that
  constrains nothing, and de-m11s.5 closed on exactly that: the variant stays, and what holds
  instead is that no NORMAL path produces one, which `tests/nothing_answers_unknown.rs` pins.

This record says what was considered and kept, so the next audit does not make the same
judgement from scratch.
