# Actions

An action is a call in a dialogue entry's userScript, run when the entry is reached. The
look-ahead never runs one against the game: every action a crawl passes through is applied, or
deliberately not applied, to search state in Rust. This is the list of what the scripts call,
what each call writes in the game, and whether the engine applies it.

This document is SELECTIVE, unlike [guards.md](guards.md). An action matters to the look-ahead
only if what it writes can change which branch a later guard takes, so each function is checked
for that first and everything that cannot is listed once as excluded.

## The rule an action is judged by

An action is **in scope** when at least one call site writes state that some guard
**downstream** of it reads.

- Downstream means reachable along dialogue links from the entry carrying the call, including
  links into other conversations, **ignoring guards** - a link counts whether or not its guard
  could pass. That over-approximates what a crawl can walk, so "no downstream reader" is a sound
  exclusion, and "a downstream reader" marks a candidate rather than a proven effect.
- The carrying entry's own guard does not count: it is decided before its script runs.
- What a function writes is taken from the game's own body - the pre-final-cut export (**PFC**),
  or Cpp2IL's dump of the shipped build (**ISIL**) for Final Cut additions - and not from its
  name.

## The corpus

Counted by `tools/survey-action-readers.py`, which reads the extractor's
`conversation_index.jsonl` and the raw dialogue database asset (for the journal's condition
variables and item properties, which the derived index does not carry):

    tools/survey-action-readers.py
    tools/survey-action-readers.py --detail FinishTask

| quantity                           |   count |
| ---------------------------------- | ------: |
| dialogue entries                   | 112,962 |
| distinct scripts                   |   8,339 |
| script functions called            |      81 |
| journal parts (tasks and subtasks) |   1,012 |
| items in the database              |     206 |

`calls` below is the number of call sites across all entries. `live` is how many of those have a
downstream guard reading something the call writes.

## Overview

Every function a script calls, by call sites. `engine` is what the engine does with the call
today; `verdict` is how that compares with the game for the writes a downstream guard reads.

| function                           | family          | calls |  live | engine                                    | verdict                                   |
| ---------------------------------- | --------------- | ----: | ----: | ----------------------------------------- | ----------------------------------------- |
| SetVariableValue                   | variables       | 9,829 | 5,582 | assign or increment a variable slot       | ported; computed values wrong - de-70eo.5 |
| ReputationGrows                    | reputation      | 1,106 |   101 | once-increment `reputation.<name>`        | ported; once ignores seen - de-70eo.4     |
| GainTask                           | journal         |   810 |   222 | `task:<argument>` = 1                     | divergent - de-70eo.3                     |
| FinishTask                         | journal         |   744 |   367 | `task:<argument>` = 0                     | divergent - de-70eo.3                     |
| XPPicoSetBool                      | variables       |   486 |    10 | assign the variable 1                     | ported                                    |
| XPTinySetBool                      | variables       |   390 |    31 | assign the variable 1                     | ported                                    |
| CancelTask                         | journal         |   280 |    39 | `task:<argument>` = 0                     | divergent - de-70eo.3                     |
| GainItem                           | items           |   242 |    92 | `item:<name>` = 1                         | ported for the item; see Items            |
| DamageVolition                     | damage          |   220 |    32 | held by decision                          | not applied - de-70eo.6                   |
| PassTime                           | clock           |   207 |    72 | clock +15 min unless locked; plugin locks | held - de-70eo.8                          |
| ReputationLowers                   | reputation      |   178 |    27 | once-decrement `reputation.<name>`        | ported; once ignores seen - de-70eo.4     |
| LoseItem                           | items           |   177 |   108 | `item:<name>` = 0                         | unequip not applied - de-70eo.7           |
| XPMinorSetBool                     | variables       |   172 |    32 | assign the variable 1                     | ported                                    |
| HealVolition                       | damage          |   106 |     8 | held by decision                          | not applied - de-70eo.6                   |
| GainThought                        | thoughts        |   101 |     9 | `thought:<name>` = 1                      | ported                                    |
| DamageEndurance                    | damage          |    90 |     0 | held by decision                          | excluded                                  |
| SetFlag                            | variables       |    65 |    32 | assign the variable 1                     | ported                                    |
| HealEndurance                      | damage          |    44 |     0 | held by decision                          | excluded                                  |
| XPStandardSetBool                  | variables       |    36 |     8 | assign the variable 1                     | ported                                    |
| GainMoneyOnce                      | money           |    28 |     0 | once-add to the money register            | excluded; ported anyway                   |
| ShowVisCal                         | presentation    |    22 |     0 | declared, no effect                       | excluded                                  |
| Reputation                         | reputation      |    19 |     6 | once-add `reputation.<name>`              | ported; once ignores seen - de-70eo.4     |
| NewspaperEndgame                   | endgame         |    17 |     0 | declared, no effect                       | excluded                                  |
| XPMajorSetBool                     | variables       |    14 |     4 | assign the variable 1                     | ported                                    |
| UseSubstanceInHand                 | substances      |    12 |     0 | held by decision                          | excluded                                  |
| SetAreaState                       | scenery         |    11 |     0 | held by decision                          | excluded                                  |
| GoToDestination                    | movement        |    10 |     0 | held by decision                          | excluded                                  |
| HideVisCal                         | presentation    |    10 |     0 | declared, no effect                       | excluded                                  |
| ReturnKitsuragi                    | party           |    10 |     0 | held by decision                          | excluded                                  |
| ShowDialogueImage                  | presentation    |     9 |     0 | declared, no effect                       | excluded                                  |
| HideVisCalAfterConversation        | presentation    |     7 |     0 | declared, no effect                       | excluded                                  |
| PrimeSpecialEndButton              | endgame         |     7 |     0 | declared, no effect                       | excluded                                  |
| GainMoneyAlways                    | money           |     6 |     6 | add to the money register                 | ported                                    |
| `Variable["x"] = value` statement  | variables       |     5 |     5 | dropped by the parser, unreported         | missing - de-70eo.5                       |
| HideDialogueImage                  | presentation    |     5 |     0 | declared, no effect                       | excluded                                  |
| GoTo                               | movement        |     4 |     0 | held by decision                          | excluded                                  |
| Obsession                          | journal flavour |     4 |     0 | declared, no effect                       | excluded                                  |
| RemoveWhiteCheck                   | checks          |     4 |     0 | declared, no effect                       | excluded; read by checks - de-70eo.2      |
| DestroyObject                      | scenery         |     3 |     0 | held by decision                          | excluded                                  |
| HealAllVolition                    | damage          |     3 |     0 | held by decision                          | excluded                                  |
| LoseMoneyAlways                    | money           |     3 |     3 | subtract from the money register          | ported                                    |
| WhirlingBedWasUsed                 | endday          |     3 |     0 | held by decision                          | excluded                                  |
| AddCunoToParty                     | party           |     2 |     0 | held by decision                          | excluded                                  |
| CloseTequilaDoor                   | scenery         |     2 |     0 | held by decision                          | excluded                                  |
| OpenBookstoreCurtains              | scenery         |     2 |     0 | held by decision                          | excluded                                  |
| RemoveCunoFromParty                | party           |     2 |     0 | held by decision                          | excluded                                  |
| RemoveCunoWaitAtFort               | party           |     2 |     0 | held by decision                          | excluded                                  |
| RemoveKitsuragiWaitAtChurch        | party           |     2 |     2 | held by decision                          | held - de-70eo.8                          |
| TequilaExpressionStopped           | presentation    |     2 |     0 | declared, no effect                       | excluded                                  |
| TequilaFascist                     | presentation    |     2 |     0 | held by decision                          | excluded                                  |
| TequilaShaved                      | presentation    |     2 |     0 | held by decision                          | excluded                                  |
| TurnOffFanLight                    | scenery         |     2 |     0 | held by decision                          | excluded                                  |
| TurnOnFanLight                     | scenery         |     2 |     0 | held by decision                          | excluded                                  |
| WhirlingEngineStart                | scenery         |     2 |     0 | held by decision                          | excluded                                  |
| DamageEnduranceWithNewspaper       | damage          |     1 |     0 | held by decision                          | excluded                                  |
| GraffitoAlight                     | scenery         |     1 |     0 | held by decision                          | excluded                                  |
| GraffitoExtinguish                 | scenery         |     1 |     0 | held by decision                          | excluded                                  |
| LetterSleep                        | endday          |     1 |     0 | held by decision                          | excluded; not fully traced                |
| LoseMoneyOnce                      | money           |     1 |     0 | once-subtract from the money register     | excluded; ported anyway                   |
| NightyNightKitsuragiShack          | party           |     1 |     0 | held by decision                          | excluded                                  |
| PlaySoundGroup                     | presentation    |     1 |     0 | declared, no effect                       | excluded                                  |
| PosseEndgame                       | endgame         |     1 |     0 | declared, no effect                       | excluded                                  |
| RemoveAndHideKitsuragi             | party           |     1 |     0 | held by decision                          | excluded                                  |
| RemoveAndHideKitsuragiUntilMorning | party           |     1 |     0 | held by decision                          | excluded                                  |
| RemoveKitsuragiWaitAtLair          | party           |     1 |     0 | held by decision                          | excluded                                  |
| RemoveKitsuragiWaitAtTent          | party           |     1 |     0 | held by decision                          | excluded                                  |
| ResetCamera                        | presentation    |     1 |     0 | declared, no effect                       | excluded                                  |
| SellItemGroup                      | items           |     1 |     0 | held by decision                          | excluded                                  |
| SellItemGroupWithModifier          | items           |     1 |     0 | held by decision                          | excluded                                  |
| ShackBedWasUsed                    | endday          |     1 |     0 | held by decision                          | excluded                                  |
| ShowInventoryForPawning            | items           |     1 |     0 | held by decision                          | excluded                                  |
| SkipToDebriefLocation              | endday          |     1 |     0 | held by decision                          | excluded; not fully traced                |
| TequilaPutOnBodysuit               | presentation    |     1 |     0 | held by decision                          | excluded; body not recovered              |
| TequilaRemoveBodysuit              | presentation    |     1 |     0 | held by decision                          | excluded; body not recovered              |
| TequilaUnobscured                  | presentation    |     1 |     0 | declared, no effect                       | excluded                                  |
| TequilaWakeUp                      | scenery         |     1 |     0 | held by decision                          | excluded                                  |
| TurnOffCeilingFan                  | scenery         |     1 |     0 | held by decision                          | excluded                                  |

Five more names are called from scripts but are READS used inside an action rather than writers:
`TotalHourCount` (17), `IsHighestCopotype` (5), `NextMorningTime` (2), `DayCount` (1) - all as
the value of a `SetVariableValue` - and `IsTHCPresent` (1), called as a bare statement whose
answer is thrown away.

Both "held by decision" and "declared, no effect" mean a `Decision` in `core::modelling`
names the function and the parser turns it into a stub that does nothing. The difference is the
decision's `readers`: a held decision names guard functions that read what it writes - the
scenery decision names `IsExterior`, for instance - and a declared one names none.

## Variables

**GAME.** `GenericLuaFunctions.SetVariableValue` sets `Variable[name]`. `SetFlag` and `UnsetFlag`
call it with `true` and `false` (ISIL). `TaskLuaFunctions.XPSetBool`, behind all five
`XP*SetBool` functions, sets the variable to `true` only if it is false, and awards experience.

**ENGINE.** `src/parser/action_parser.rs` `translate_call` interns the variable and applies an
assign, or an increment when the value is `Variable["same"] + N` or `+ once(N)`. Counters are
capped (`CounterCaps`) and floored at zero.

Of the 5,582 live `SetVariableValue` call sites, 5,222 assign a literal and 353 increment - both
applied as the game applies them. The rest are the two gaps below, both de-70eo.5.

- **Computed values are read as 1.** `read_assigned_value` falls back to 1 for a value it cannot
  parse. Seven live call sites compute one: `TotalHourCount() + 8` for
  `doomed.dicemaker_order_deadline` (conversation 460, five sites) and `NextMorningTime()` for
  two variables in conversation 965. So a deadline becomes hour 1.
- **Direct assignments are dropped.** Five scripts write `Variable["x"] = true` as a Lua
  statement - for example `canal.tires_concept_red_check` and `tc.electronic_locks`. The parser
  scans only calls, so the statement is skipped without being reported as unmodelled. All five
  have a downstream reader.

`core::modelling` lists `NextMorningTime` as a clock writer; its body
(`DaytimeLuaFunctions.NextMorningTime`) only returns `24 * DayCounter + wakeup hour`.

## Reputation

**GAME.** `KarmaLuaFunctions.ReputationGrows`, `ReputationLowers` and `Reputation` all reach
`ReputationAlterant.ReputationOption(name, value)`, which applies
`Variable["reputation.<name>"] += value` only when `Once(value) != 0`.

**ENGINE.** An increment of `reputation.<name>` by +1, -1 or the parsed amount, marked once. The
guards `IsHighestCopotype` and `IsHighestPolitical` read these slots (see guards.md).

Correct except for what `once` reads - see [Once](#once).

## Once

**GAME.** `GenericLuaFunctions.Once(value)` returns 0 when the entry running it is already seen:
`SunshineNode.IsSeen(ConversationLogger.LastDialogueEntry)`, whose answer `ConversationLogger`
captures before it marks the entry displayed. Reputation, `GainMoneyOnce`, `LoseMoneyOnce`,
`HealVolition` and `HealEndurance` in conversation, and every `+ once(N)` increment go through
it.

**ENGINE.** A node whose actions fire once gets a `once:` slot, set when they fire. The slot
starts at 0 for every crawl: `core::state::seed_state` seeds `seen:` slots from
`ILookAheadWorld::is_seen` but not `once:` slots. So an entry the player saw on an earlier visit
fires its once-actions again during the crawl, where the game fires nothing. de-70eo.4.

## Journal

**GAME.** `TaskLuaFunctions.GainTask`, `FinishTask` and `CancelTask` delegate to `JournalModel`,
which resolves the argument with `GetByConditionVariable`: any of a task's or subtask's three
condition variables names it. Those come from each task conversation's fields -
`display_condition_main`, `done_condition_main`, `cancel_condition_main`, and the same per
subtask (`JournalImporter.Populate`).

| action     | no-op when           | writes                                                       |
| ---------- | -------------------- | ------------------------------------------------------------ |
| GainTask   | visible or cancelled | the show variable (`Completeable.Reveal`)                    |
| FinishTask | done                 | the show variable if not yet visible, then the done variable |
| CancelTask | done                 | the cancel variable                                          |

`JournalModel.IsTaskActive` is: gained, not cancelled, not done - and for a subtask, its parent
neither done nor cancelled.

**ENGINE.** The literal argument is interned as a `task:` slot and set to 1 (`GainTask`) or 0
(`FinishTask`, `CancelTask`). Scripts mostly finish a task by its done variable -
`FinishTask("TASK.x_done")` - so the engine clears `task:TASK.x_done`, which nothing reads, while
`IsTaskActive("TASK.x")` stays active. No condition variable is written, though guards read them
far more than they call `IsTaskActive`, and the no-op orderings are ignored.

| action     | calls | live | live via a variable | live via IsTaskActive |
| ---------- | ----: | ---: | ------------------: | --------------------: |
| GainTask   |   810 |  222 |                 190 |                    82 |
| FinishTask |   744 |  367 |                 425 |                   149 |
| CancelTask |   280 |   39 |                   7 |                    42 |

A call site can be live through several keys, so the last two columns can sum past `live`. This
is the largest gap the audit found. de-70eo.3.

## Items

**GAME.** `InventoryLuaFunctions.GainItem` skips an item already held unless `multipleAllowed`,
then `Inventory.HandlePickedUpItem`:

- an `autoequip` item is equipped (`InventoryViewData.Equip`);
- a consumable never joins `gainedItems` - a valued one adds its `itemValue` to money, a healing
  item goes to the healing pools;
- bullets and `key_ring` stacks go to their special stacks.

`LoseItem` is `Inventory.DeleteItem`, which unequips the item it deletes.

The database's `itemType` is the game's `ItemType` enum plus one, so types 1 to 11 (ARMOR to HELD)
go in an equipment slot; keys and papers are 13.

**ENGINE.** `item:<name>` set to 1 or 0, which `CheckItem` and `CheckItemGroup` read. Equipment is
answered from the slots as they were when the crawl started, and `core::modelling` declares the
equipment questions constant by construction.

| action   | calls | live | live via the item | via equipment | via an item group | via a tab |
| -------- | ----: | ---: | ----------------: | ------------: | ----------------: | --------: |
| GainItem |   242 |   92 |                88 |             0 |                 2 |         4 |
| LoseItem |   177 |  108 |                93 |            26 |                 8 |         2 |

So the item slot is right, and the gap is `LoseItem` of a worn or held item with an equipment
question downstream - 26 call sites, such as `neck_setting_sun_medal` in conversation 280 and
`prybar` in 350. No autoequip gain has a downstream equipment reader today. de-70eo.7.

The pawnables tab is read from the start of the crawl (see guards.md); its 6 live sites are
covered by that note rather than by a port.

## Thoughts

**GAME.** `THCLuaFunctions.GainThought` calls `Inventory.OnPickupThought` when
`Inventory.CanBeGained` - not already gained and not forgotten - which adds the thought to
`gainedThoughts`.

**ENGINE.** `thought:<name>` = 1, read by `IsTHCPresent`. Ported: a forgotten thought cannot be
regained, and no crawl can forget one. 9 live call sites.

## Money

**GAME.** `MoneyLuaFunctions`: `GainMoneyAlways` and `LoseMoneyAlways` move
`PlayerCharacter.Money` by the amount; the `Once` forms move it by `Once(amount)`.

**ENGINE.** The money register, read by `MoneyAmount`. Ported. `GainMoneyAlways` (6) and
`LoseMoneyAlways` (3) have live sites; the once forms have none, but their `once` is subject to
[Once](#once).

## Damage

**GAME.** `CharacterManipulations.DamageVolition(n)` adds `-n` to the skill's single `DAMAGE`
modifier, clamped to its current value (`Modifiable.DamageValue`). `HealVolition(n)` - through
`Once(n)` in conversation - clamps to `maximumValue - value` and removes that much damage
(`Modifiable.HealValue`). `HealAllVolition` heals all of it. Endurance is the same.

**ENGINE.** A `Decision` in `core::modelling` holds `HasVolitionDamage` and `HasEnduranceDamage`
at the world's answer. 32 `DamageVolition` and 8 `HealVolition` call sites have a downstream
`HasVolitionDamage`; no endurance writer has a downstream reader. de-70eo.6.

## Clock

**GAME.** `PassTime` is `SunshineClock.Clang`, `NormalTimeForward(15)` unless time is locked,
which also bakes cooking thoughts (`ThoughtManager.BakeThoughts`) and substances.

**ENGINE.** `PassTime` adds 15 minutes unless the world's clock is locked, and the plugin always
sends it locked, so time stands still for a crawl. 72 call sites are live - 50 through a clock
question and 45 through a cabinet question, since baking can turn a cooking thought fixed. Held
by that approximation. de-70eo.8.

## Party

**GAME.** `KimLuaFunctions` and `CunoLuaFunctions` set `IsInParty` and `IsLeftOutside` on the
party members through `PartyManager` and `PartyMember.Remove`, and record wait locations.

**ENGINE.** Held at the world's answer by a `Decision` in `core::modelling`. Only
`RemoveKitsuragiWaitAtChurch` has live call sites (2). de-70eo.8.

## Excluded

No call site of these writes anything a downstream guard reads.

- **No guard reads what they write.** Presentation (`ShowVisCal`, `HideVisCal`,
  `HideVisCalAfterConversation`, `ShowDialogueImage`, `HideDialogueImage`, `PlaySoundGroup`,
  `ResetCamera`, and the `Tequila*` visual states); scenery (`SetAreaState` and `DestroyObject`
  write the `AreaState` Lua table, which no guard reads, and the fan, curtain, graffito, door and
  engine functions move objects); `Obsession` (the orb manager); the endgame functions;
  `ShackBedWasUsed` and `WhirlingBedWasUsed` (`PartyManager.sleepLocation`).
- **Guards read what they write, but never downstream.** Endurance damage and healing,
  `UseSubstanceInHand` (`HudHeldPanelController.OnSubstanceUse` increments `stats.uses_<group>`,
  which `SubstanceUsedOnce` and `SubstanceUsedMore` read), the party writers other than
  `RemoveKitsuragiWaitAtChurch`, `SellItemGroup`, `SellItemGroupWithModifier` and
  `ShowInventoryForPawning`, and the once forms of money.
- **Movement.** `GoTo` and `GoToDestination` call `ConversationLogger.ForceStopConversation`
  before changing area, so nothing after them in the conversation runs.
- **Not fully traced.** `LetterSleep` and `SkipToDebriefLocation` hand off to `EnddayManager`,
  whose property setters write `auto.daychange_*` variables; which of those these two reach is
  not traced here. `TequilaPutOnBodysuit` and `TequilaRemoveBodysuit` are Final Cut coroutines
  whose bodies were not recovered.

`RemoveWhiteCheck` writes the failed white check cache, which is read by the white check itself
rather than by a guard - see de-70eo.2, which covers what checks read and write.

## Keeping this current

The survey recognises a function by the `WRITES` table at the top of
`tools/survey-action-readers.py`, and marks a function it has no row for as `(no writes
recorded)`. When a script function appears that is not in the overview, read its body, add its
row there, rerun the survey, and add it here. `tools/align-markdown-tables.py docs/actions.md`
realigns the tables.
