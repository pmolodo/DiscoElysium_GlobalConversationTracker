# Actions

An action is a call in a dialogue entry's userScript, run when the entry is reached. The
look-ahead never runs one against the game: every action a crawl passes through is applied, or
deliberately not applied, to search state in Rust. This is the list of what the scripts call,
what each call writes in the game, and whether the engine applies it. Every action, guard and
check the engine does not model exactly is collected in [modelling-gaps.md](modelling-gaps.md).

This document is SELECTIVE, unlike [guards.md](guards.md). An action matters to the look-ahead
only if what it writes can change which branch a later guard takes, so each function is checked
for that first and everything that cannot is listed once as excluded.

## The rule an action is judged by

An action is **in scope** when at least one call site writes state that some reader
**downstream** of it reads. A reader is a guard, or an entry that reads state through its fields:
a priced entry reads money, a rolled or fake check reads its flags, and a white check's modifier
expressions decide whether a failed check reopens - see [Hidden writes](#hidden-writes).

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

| quantity                          |   count |
| --------------------------------- | ------: |
| dialogue entries                  | 112,962 |
| distinct scripts                  |   8,339 |
| script functions called           |      81 |
| journal parts: tasks and subtasks |     337 |
| journal condition variables       |   1,012 |
| items in the database             |     206 |

`calls` below is the number of call sites across all entries. `live` is how many of those have a
downstream reader of something the call writes.

## Overview

Every function a script calls, by call sites. `engine` is what the engine does with the call
today; `verdict` is how that compares with the game for the writes a downstream guard reads.

| function                           | family          | calls |  live | engine                                     | verdict                                |
| ---------------------------------- | --------------- | ----: | ----: | ------------------------------------------ | -------------------------------------- |
| SetVariableValue                   | variables       | 9,829 | 5,673 | assign, increment or clock reading         | ported; 2 shapes unmodelled, none live |
| ReputationGrows                    | reputation      | 1,106 |   102 | once-increment `reputation.<name>`         | ported                                 |
| GainTask                           | journal         |   810 |   222 | show unless cancelled                      | ported                                 |
| FinishTask                         | journal         |   744 |   367 | show unless done, then done                | ported                                 |
| XPPicoSetBool                      | variables       |   486 |    12 | assign the variable 1                      | ported                                 |
| XPTinySetBool                      | variables       |   390 |    37 | assign the variable 1                      | ported                                 |
| CancelTask                         | journal         |   280 |    39 | cancel unless done                         | ported                                 |
| GainItem                           | items           |   242 |    93 | `item:<name>` = 1                          | ported; autoequip unsettles passives   |
| DamageVolition                     | damage          |   220 |    32 | damage amount += n                         | ported                                 |
| PassTime                           | clock           |   207 |    72 | clock +15 min unless locked; plugin locks  | ported; what it bakes is not           |
| ReputationLowers                   | reputation      |   178 |    32 | once-decrement `reputation.<name>`         | ported                                 |
| LoseItem                           | items           |   177 |   108 | `item:<name>` = 0, `unequipped:<name>` = 1 | ported                                 |
| XPMinorSetBool                     | variables       |   172 |    32 | assign the variable 1                      | ported                                 |
| HealVolition                       | damage          |   106 |     8 | damage amount -= n, once                   | ported                                 |
| GainThought                        | thoughts        |   101 |    22 | `thought:<name>` = 1                       | ported; reopening a white check not    |
| DamageEndurance                    | damage          |    90 |     0 | damage amount += n                         | ported; no downstream reader           |
| SetFlag                            | variables       |    65 |    32 | assign the variable 1                      | ported                                 |
| HealEndurance                      | damage          |    44 |     0 | damage amount -= n, once                   | ported; no downstream reader           |
| XPStandardSetBool                  | variables       |    36 |     8 | assign the variable 1                      | ported                                 |
| GainMoneyOnce                      | money           |    28 |     5 | once-add to the money register             | ported                                 |
| ShowVisCal                         | presentation    |    22 |     0 | declared, no effect                        | excluded                               |
| Reputation                         | reputation      |    19 |     6 | once-add `reputation.<name>`               | ported                                 |
| NewspaperEndgame                   | endgame         |    17 |     0 | declared, no effect                        | excluded                               |
| XPMajorSetBool                     | variables       |    14 |     4 | assign the variable 1                      | ported                                 |
| UseSubstanceInHand                 | substances      |    12 |     0 | held by decision                           | excluded                               |
| SetAreaState                       | scenery         |    11 |     0 | held by decision                           | excluded                               |
| GoToDestination                    | movement        |    10 |     0 | held by decision                           | excluded                               |
| HideVisCal                         | presentation    |    10 |     0 | declared, no effect                        | excluded                               |
| ReturnKitsuragi                    | party           |    10 |     0 | held by decision                           | excluded                               |
| ShowDialogueImage                  | presentation    |     9 |     0 | declared, no effect                        | excluded                               |
| HideVisCalAfterConversation        | presentation    |     7 |     0 | declared, no effect                        | excluded                               |
| PrimeSpecialEndButton              | endgame         |     7 |     0 | declared, no effect                        | excluded                               |
| GainMoneyAlways                    | money           |     6 |     6 | add to the money register                  | ported                                 |
| `Variable["x"] = value` statement  | variables       |     5 |     5 | assign, as `SetVariableValue`              | ported                                 |
| HideDialogueImage                  | presentation    |     5 |     0 | declared, no effect                        | excluded                               |
| GoTo                               | movement        |     4 |     0 | held by decision                           | excluded                               |
| Obsession                          | journal flavour |     4 |     0 | declared, no effect                        | excluded                               |
| RemoveWhiteCheck                   | checks          |     4 |     0 | declared, no effect                        | excluded; names no check               |
| DestroyObject                      | scenery         |     3 |     0 | held by decision                           | excluded                               |
| HealAllVolition                    | damage          |     3 |     0 | damage amount := 0                         | ported; no downstream reader           |
| LoseMoneyAlways                    | money           |     3 |     3 | subtract from the money register           | ported                                 |
| WhirlingBedWasUsed                 | endday          |     3 |     0 | held by decision                           | excluded                               |
| AddCunoToParty                     | party           |     2 |     0 | held by decision                           | excluded                               |
| CloseTequilaDoor                   | scenery         |     2 |     0 | held by decision                           | excluded                               |
| OpenBookstoreCurtains              | scenery         |     2 |     0 | held by decision                           | excluded                               |
| RemoveCunoFromParty                | party           |     2 |     0 | held by decision                           | excluded                               |
| RemoveCunoWaitAtFort               | party           |     2 |     0 | held by decision                           | excluded                               |
| RemoveKitsuragiWaitAtChurch        | party           |     2 |     2 | Kim removed := 1                           | ported                                 |
| TequilaExpressionStopped           | presentation    |     2 |     0 | declared, no effect                        | excluded                               |
| TequilaFascist                     | presentation    |     2 |     0 | held by decision                           | excluded                               |
| TequilaShaved                      | presentation    |     2 |     0 | held by decision                           | excluded                               |
| TurnOffFanLight                    | scenery         |     2 |     0 | held by decision                           | excluded                               |
| TurnOnFanLight                     | scenery         |     2 |     0 | held by decision                           | excluded                               |
| WhirlingEngineStart                | scenery         |     2 |     0 | held by decision                           | excluded                               |
| DamageEnduranceWithNewspaper       | damage          |     1 |     0 | damage amount += n                         | ported; no downstream reader           |
| GraffitoAlight                     | scenery         |     1 |     0 | held by decision                           | excluded                               |
| GraffitoExtinguish                 | scenery         |     1 |     0 | held by decision                           | excluded                               |
| LetterSleep                        | endday          |     1 |     0 | held by decision                           | excluded; not fully traced             |
| LoseMoneyOnce                      | money           |     1 |     0 | once-subtract from the money register      | excluded; ported anyway                |
| NightyNightKitsuragiShack          | party           |     1 |     0 | held by decision                           | excluded                               |
| PlaySoundGroup                     | presentation    |     1 |     0 | declared, no effect                        | excluded                               |
| PosseEndgame                       | endgame         |     1 |     0 | declared, no effect                        | excluded                               |
| RemoveAndHideKitsuragi             | party           |     1 |     0 | held by decision                           | excluded                               |
| RemoveAndHideKitsuragiUntilMorning | party           |     1 |     0 | held by decision                           | excluded                               |
| RemoveKitsuragiWaitAtLair          | party           |     1 |     0 | held by decision                           | excluded                               |
| RemoveKitsuragiWaitAtTent          | party           |     1 |     0 | held by decision                           | excluded                               |
| ResetCamera                        | presentation    |     1 |     0 | declared, no effect                        | excluded                               |
| SellItemGroup                      | items           |     1 |     0 | held by decision                           | excluded                               |
| SellItemGroupWithModifier          | items           |     1 |     0 | held by decision                           | excluded                               |
| ShackBedWasUsed                    | endday          |     1 |     0 | held by decision                           | excluded                               |
| ShowInventoryForPawning            | items           |     1 |     0 | held by decision                           | excluded                               |
| SkipToDebriefLocation              | endday          |     1 |     0 | held by decision                           | excluded; not fully traced             |
| TequilaPutOnBodysuit               | presentation    |     1 |     0 | held by decision                           | excluded; body not recovered           |
| TequilaRemoveBodysuit              | presentation    |     1 |     0 | held by decision                           | excluded; body not recovered           |
| TequilaUnobscured                  | presentation    |     1 |     0 | declared, no effect                        | excluded                               |
| TequilaWakeUp                      | scenery         |     1 |     0 | held by decision                           | excluded                               |
| TurnOffCeilingFan                  | scenery         |     1 |     0 | held by decision                           | excluded                               |

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

**ENGINE.** `src/parser/action_parser.rs` reads a script statement by statement.
`translate_value_write` handles `SetVariableValue` and a bare `Variable["x"] = value` statement
alike, and reads the value as one of:

| value                                                     | action                                                        |
| --------------------------------------------------------- | ------------------------------------------------------------- |
| `Variable["same"] + N`, `+ once(N)`                       | increment, capped (`CounterCaps`), floored at zero            |
| `TotalHourCount() + N`, `DayCount()`, `NextMorningTime()` | `AssignClock`: the reading plus N, taken when the action runs |
| `true`, `false`, a number                                 | assign                                                        |
| anything else                                             | unmodelled, and reported as a gap                             |

Of the 5,673 live `SetVariableValue` call sites, 5,313 assign a literal, 353 increment and 7 store
a clock reading - `doomed.dicemaker_order_deadline` in conversation 460 and two meeting times in
965. Three guards read such a deadline back against `TotalHourCount()`.

A clock reading is exact in the reference walk, which reads the state's own clock. The symbolic
searches take it at the world's time (`ActionImage::for_world`), which is the time every state has
while the layout does not carry the clock; where it does, the slot is forgotten instead, which
keeps every value the reading could have. A slot holding a reading is eight bits wide
(`ClockReading::VALUE_CEILING`): readings are absolute hours, no guard names a day past 7 and no
script adds more than 23 hours.

Two value shapes are unmodelled and neither has a downstream reader: `not(Variable[...])` (2
sites) and `IsHighestCopotype(...)` (5 sites, the plaza monument). A call that writes, found
INSIDE a value - one script stores `true and CancelTask(...)` - is applied before the value, as
Lua evaluates it; a call that only reads is part of the value.

## Reputation

**GAME.** `KarmaLuaFunctions.ReputationGrows`, `ReputationLowers` and `Reputation` all reach
`ReputationAlterant.ReputationOption(name, value)`, which applies
`Variable["reputation.<name>"] += value` only when `Once(value) != 0`.

`ReputationOption` then runs `ReputationEffect`, which does more while the reputation's own
copotype thought is fixed: `ultraliberal` adds 100 to money, `revacholian_nationhood` damages
volition by one, and `moralist` and `the_destroyer` heal volition and endurance by one.

**ENGINE.** An increment of `reputation.<name>` by +1, -1 or the parsed amount, marked once. The
guards `IsHighestCopotype` and `IsHighestPolitical` read these slots (see guards.md).

The thought's effect follows as a second once action, conditioned on the thought
(`core::thought_effects`). A graph is fitted to its world before a search
(`LookAheadGraph::fit`), which switches the action on where the world holds the thought fixed; the
group asks for `ThoughtsFixed` over those thoughts, and the kept workspace is rebuilt when the
answer changes. Live: 8 `ultraliberal` sites have a money reader downstream and 4
`revacholian_nationhood` sites a damage reader. Tested by
`a_fixed_copotype_thought_adds_to_a_reputation_action` in `oracle.rs` and `backward.rs`.

Its `once` reads the seen record - see [Once](#once).

## Once

**GAME.** `GenericLuaFunctions.Once(value)` returns 0 when the entry running it is already seen:
`SunshineNode.IsSeen(ConversationLogger.LastDialogueEntry)`, whose answer `ConversationLogger`
captures before it marks the entry displayed. Reputation, `GainMoneyOnce`, `LoseMoneyOnce`,
`HealVolition` and `HealEndurance` in conversation, and every `+ once(N)` increment go through
it.

**ENGINE.** A node whose actions fire once, or whose price is charged once, gets a `once:`
slot, set when they fire. `core::state::seed_state` starts that slot set for an entry
`ILookAheadWorld::is_seen` reports, so an entry the player saw on an earlier visit fires nothing
once-only during the crawl, as in the game. Every search seeds from `seed_state`, so the
reference walk and both symbolic passes agree on it.

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

**ENGINE.** The journal is its variables. `index::journal::Journal` is read from the task
conversations' fields, which the index carries, and resolves an argument by any of a
part's three variables. The parser turns each action into writes to that part's variables:

| action     | writes                                                            |
| ---------- | ----------------------------------------------------------------- |
| GainTask   | show := 1 unless cancel is set                                    |
| FinishTask | show := 1 unless done is set; then done := 1                      |
| CancelTask | cancel := 1 unless done is set; nothing without a cancel variable |

"Unless" is `DialogueActionKind::AssignUnless`, applied by the reference walk and split on the
tested slot in the symbolic image and pre-image. A tested slot the group also writes is kept
however little else reads it, since the action has no world to ask. A tested slot nothing in the
group writes holds the world's value for the whole search, so the write becomes a plain assignment
switched on or off when the graph is fitted (`DialogueAction::settled_by_world`), and the slot is
dropped - conversation 368's menu went from 986 ms to about 650 ms for it. An argument naming no
part writes nothing, as the game logs and returns.

`IsTaskActive("x")` is rewritten when the graph is built into the guard it means over those
variables - `show and not done and not cancel`, and the parent's done and cancel for a subtask -
so it is declared, asked for and decided like any other variable read.

| action     | calls | live | live via a variable | live via IsTaskActive |
| ---------- | ----: | ---: | ------------------: | --------------------: |
| GainTask   |   810 |  222 |                 190 |                    82 |
| FinishTask |   744 |  367 |                 426 |                   149 |
| CancelTask |   280 |   39 |                   7 |                    42 |

A call site can be live through several keys, so the last two columns can sum past `live`.

Two scripts set a task's cancel variable with `SetVariableValue` rather than `CancelTask`
(`TASK.communist_dream_quest_cancelled`, `TASK.find_smokes_cancelled`). The game does not cancel
the task until a load syncs it from the variable; read as variables, it is cancelled at once.

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

**ENGINE.** `item:<name>` set to 1 or 0, which `CheckItem` and `CheckItemGroup` read. `LoseItem`
also sets `unequipped:<name>`. Every equipment question reads a slot holding a lost item as empty,
using what each slot held when the crawl started (`ILookAheadWorld::item_in_slot`). The guard
compiler takes one case per combination of the losable items in the slots a question reads.
Tested by `losing_a_worn_item_takes_it_off` in `oracle.rs` and `backward.rs`.

| action   | calls | live | live via the item | via equipment | via an item group | via a tab |
| -------- | ----: | ---: | ----------------: | ------------: | ----------------: | --------: |
| GainItem |   242 |   93 |                89 |             0 |                 2 |         4 |
| LoseItem |   177 |  108 |                93 |            26 |                 8 |         2 |

The 26 `LoseItem` sites live through equipment, such as `neck_setting_sun_medal` in conversation
280 and `prybar` in 350, are what the unequip covers. `GainItem` writes nothing beyond the item
slot: no autoequip gain has a downstream equipment question, and no gain of a consumable (which
never joins the inventory) has a downstream item reader - `drouamine`, `hypnogamma`, `magnesium`
and `nosaphed` are the consumables gained, at 8 sites, all without one.

Equipment also moves skill values, which a passive check compares. Item bonuses are not in the
dialogue database, so a passive check is answered Unknown in any group that can take off an item
the world has on, or gain one of the seven autoequip items (`core::skill_movers`). The two
autoequip gains in the corpus are the necktie in conversation 9.

The pawnables tab is read from the start of the crawl (see guards.md); its 6 live sites are
covered by that note rather than by a port.

## Thoughts

**GAME.** `THCLuaFunctions.GainThought` calls `Inventory.OnPickupThought` when
`Inventory.CanBeGained` - not already gained and not forgotten - which adds the thought to
`gainedThoughts`.

**ENGINE.** `thought:<name>` = 1, read by `IsTHCPresent`. Ported: a forgotten thought cannot be
regained, and no crawl can forget one. 22 live call sites: 9 with an `IsTHCPresent` guard
downstream, and 13 more whose only downstream reader is a white check modifier, which can reopen a
failed check - not followed, de-vdy9.

## Money

**GAME.** `MoneyLuaFunctions`: `GainMoneyAlways` and `LoseMoneyAlways` move
`PlayerCharacter.Money` by the amount; the `Once` forms move it by `Once(amount)`.

**ENGINE.** The money register, read by `MoneyAmount` and by every priced entry. Ported.
`GainMoneyAlways` (6), `LoseMoneyAlways` (3) and `GainMoneyOnce` (5, all through a priced entry)
have live sites; `LoseMoneyOnce` has none. The once forms' `once` reads the seen record - see
[Once](#once).

A priced entry (`ClickCost`) is not a script call, but it moves the same register.
`CostOptionNode.HandleEntry` charges the price on entry, except for a `CostOnce` entry already
seen. `HandleResponseText` disables the option whenever the price is above the purse, and makes
no exception for one already paid. The engine does the same: a paid once-cost is not charged
again, but still needs the price in hand.

`CostOptionNode.GetCost` doubles the price of healing and drug purchases in hardcore mode. It
finds the item bought by walking from the priced entry to the first `GainItem`, evaluating
conditions on the way. The engine replays that walk when it builds a group, and scales an
entry's price when its world is in hardcore mode (`src/core/price.rs`, `src/index/price.rs`).
Nine purchases are scaled. Where the walk meets a guard, the item found could depend on the
world, so the entry keeps its `ClickCost`. That is the cheaper price, so the engine can only show
a purchase the game refuses, never hide one it allows. Conversation 28's room at 20 real is the
one case in the corpus.

## Damage

**GAME.** `CharacterManipulations.DamageVolition(n)` adds `-n` to the skill's single `DAMAGE`
modifier, clamped to its current value (`Modifiable.DamageValue`). `HealVolition(n)` - through
`Once(n)` in conversation - clamps to `maximumValue - value` and removes that much damage
(`Modifiable.HealValue`). `HealAllVolition` heals all of it. Endurance is the same.

**ENGINE.** A `damage:` slot per skill holds the damage AMOUNT, seeded from the damage value
the plugin reads (`DataKind::SkillDamage`). `DamageVolition(n)` adds n, `HealVolition(n)` subtracts
n once and stops at none, `HealAllVolition` clears it; endurance the same. `HasVolitionDamage` and
`HasEnduranceDamage` read the slot where the group writes one and the world where it does not.
The clamp of damage to the skill's current value is not followed - a blow that large ends the
game. 32 `DamageVolition` and 8 `HealVolition` call sites have a downstream `HasVolitionDamage`.

## Clock

**GAME.** `PassTime` is `SunshineClock.Clang`, `NormalTimeForward(15)` unless time is locked,
which also bakes cooking thoughts (`ThoughtManager.BakeThoughts`) and substances.

**ENGINE.** `PassTime` adds 15 minutes unless the world's clock is locked. Both executors model
it: `core::action` moves `day_minutes`, and a symbolic search carries the clock in eleven
variables and answers every hour question over them - but only where a `PassTime` in the group
can move an unlocked clock, since otherwise the hour is a constant and a register would spend
bits restating it. `DataLayout::clock_can_move` is that decision.

**WHETHER IT IS LOCKED IS THE GAME'S TO SAY, and it usually is not.** The lock is the
must-sleep mechanic - `SunshineClockTime.LockTimeIfNeed` sets it once the player has crossed
midnight without sleeping and the hour reaches two - so during ordinary play the clock runs. No
Lua function exposes the flag, so the plugin takes it from a hook on a method the game calls
with the live clock in hand (`ClockLockPatch`), and a fixture derives it from the save the way
the loader does (`fixtures::clock_locked_in_save`). Until the hook has fired the answer is
locked, which is the conservative one.

72 call sites are live - 50 through a clock question and 45 through a cabinet question, since
baking can turn a cooking thought fixed. THE BAKING IS NOT MODELLED: the clock moves and the
thoughts do not follow it, which is de-m11s.3.3 and de-m11s.3.4.

## Party

**GAME.** `KimLuaFunctions` and `CunoLuaFunctions` set `IsInParty` and `IsLeftOutside` on the
party members through `PartyManager` and `PartyMember.Remove`, and record wait locations.

**ENGINE.** `RemoveKitsuragiWaitAtChurch` is the only party writer with live call sites (2),
and it is ported. `KimKitsuragi.RemoveAndHide` takes Kim out of the party through
`Party.Player.Remove` and leaves `IsLeftOutside` alone, so afterwards `IsKimInParty` and
`IsKimHere` are both false. A search holds that in one `party:kimRemoved` slot, clear at the start
and set by the call. While it is clear the two questions are the world's answer, and an unread
flag stays unknown. The other party writers are held at the world's answer by a `Decision` in
`core::modelling`. Tested by `leaving_kim_at_the_church_moves_the_kim_questions` in `oracle.rs`
and `backward.rs`.

## Excluded

No call site of these writes anything a downstream reader reads.

- **Nothing reads what they write.** Presentation (`ShowVisCal`, `HideVisCal`,
  `HideVisCalAfterConversation`, `ShowDialogueImage`, `HideDialogueImage`, `PlaySoundGroup`,
  `ResetCamera`, and the `Tequila*` visual states); scenery (`SetAreaState` and `DestroyObject`
  write the `AreaState` Lua table, which no guard reads, and the fan, curtain, graffito, door and
  engine functions move objects); `Obsession` (the orb manager); the endgame functions;
  `ShackBedWasUsed` and `WhirlingBedWasUsed` (`PartyManager.sleepLocation`).
- **Something reads what they write, but never downstream.** `UseSubstanceInHand`
  (`HudHeldPanelController.OnSubstanceUse` increments `stats.uses_<group>`, which
  `SubstanceUsedOnce` and `SubstanceUsedMore` read), the party writers other than
  `RemoveKitsuragiWaitAtChurch`, `SellItemGroup`, `SellItemGroupWithModifier`,
  `ShowInventoryForPawning` and `LoseMoneyOnce`.
- **Movement.** `GoTo` and `GoToDestination` call `ConversationLogger.ForceStopConversation`
  before changing area, so nothing after them in the conversation runs.
- **Not fully traced.** `LetterSleep` and `SkipToDebriefLocation` hand off to `EnddayManager`,
  whose property setters write `auto.daychange_*` variables; which of those these two reach is
  not traced here. `TequilaPutOnBodysuit` and `TequilaRemoveBodysuit` are Final Cut coroutines
  whose bodies were not recovered.

`RemoveWhiteCheck` retires a white check from the failed and seen caches, which the check reads
rather than a guard - see [Hidden reads in guards.md](guards.md#hidden-reads). All four call sites
(conversation 627) pass `Variable["yard.hanged_inland_corpse_answered"]`, a variable's value
rather than a check's flag name, so none of them names a check to retire.

## Hidden writes

A hidden write is state a script, a check or a priced entry changes without naming it in its own
text. The reads on the other side are in [Hidden reads in guards.md](guards.md#hidden-reads).
Counts use this document's rule for downstream; the passive-check and check-result counts come
from one-off queries over the same index rather than from the survey.

| writer                                                        | writes without naming it                                                                                                | game source                                                      | engine                                                                                                                                                                                                                                                             |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| entering any entry                                            | its `SimStatus`, `WasDisplayed`                                                                                         | Dialogue System, captured first by `ConversationLogger`          | read only by `Once`, `CostOnce`, fake checks and Kim switches, which are `once:` and `seen:` slots; no guard reads `SimStatus`                                                                                                                                     |
| priced entry                                                  | money, the price `GetCost` computes                                                                                     | PFC `CostOptionNode.HandleEntry`                                 | tracked - see [Money](#money)                                                                                                                                                                                                                                      |
| `GainTask`, `FinishTask`, `CancelTask`                        | the part's show, done or cancel variable                                                                                | PFC `Completeable.Reveal`, `MarkDone`, `CancelTask`              | tracked - see [Journal](#journal)                                                                                                                                                                                                                                  |
| `GainItem`                                                    | equipment for an `autoequip` item; money for a consumable with an `itemValue`; the healing pools; a substance's charges | PFC `Inventory.HandlePickedUpItem`                               | the item is tracked; no autoequip gain has a downstream equipment reader, and no valued consumable is gained anywhere in the corpus                                                                                                                                |
| `GainItem`, `LoseItem` of equipment                           | the skill values its bonuses add to                                                                                     | PFC `Modifiable.Recalc`                                          | a passive check is answered Unknown in a group that can take off an item the world has on or put on an autoequip item, since item bonuses are not in the database (`core::skill_movers`)                                                                           |
| `LoseItem`                                                    | equipment, which it unequips                                                                                            | PFC `Inventory.DeleteItem`                                       | tracked - see [Items](#items)                                                                                                                                                                                                                                      |
| `DamageVolition`, `HealVolition` and the endurance forms      | the skill value itself, since `DAMAGE` is one of its modifiers                                                          | PFC `Modifiable.Recalc`                                          | the damage slot is tracked; a Volition or Endurance passive check is answered Unknown where the group's damage or healing can cross the margin the plugin sends for it (`core::skill_movers`)                                                                      |
| `UseSubstanceInHand`                                          | `stats.uses_<group>`                                                                                                    | PFC `HudHeldPanelController.OnSubstanceUse`                      | excluded: no downstream reader                                                                                                                                                                                                                                     |
| `PassTime`                                                    | cooking thoughts become fixed, substances wear off                                                                      | PFC `SunshineClock.Clang`, `ThoughtManager.BakeThoughts`         | held: the clock moves and these do not follow it - de-m11s.3.3 and de-m11s.3.4. See [Clock](#clock)                                                                                                                                                                |
| `LetterSleep`, `SkipToDebriefLocation`                        | `auto.daychange_*` through `EnddayManager` property setters                                                             | PFC `EnddayManager`                                              | excluded; which variables each reaches is not traced                                                                                                                                                                                                               |
| `SetAreaState`, `DestroyObject`, `GainItem("ledger_damaged")` | the `AreaState` Lua table                                                                                               | PFC `AreaStatePlaceholder.SwitchTo`, `Alterant.HandleItemPickup` | excluded: no guard reads it                                                                                                                                                                                                                                        |
| `XP*SetBool`                                                  | experience                                                                                                              | PFC `TaskLuaFunctions.XPSetBool`                                 | not applied: no guard reads experience                                                                                                                                                                                                                             |
| red and white checks                                          | `FlagName` on success; red `FlagName_failed` on failure; a failed white check joins `FailedWhiteChecks`                 | PFC `RedCheckNode.CheckSuccess`, `WhiteCheckNode.CheckSuccess`   | tracked flag slots; a white failure is recorded as a `FlagName_failed` slot                                                                                                                                                                                        |
| fake checks                                                   | `FlagName_failed` - all three are forced failures (`AlwaysSucceed` false)                                               | PFC `FakeCheckNode.CheckSuccess`                                 | not written; no guard reads any of the three `_failed` variables                                                                                                                                                                                                   |
| a check's result, with certain thoughts fixed                 | money, and volition or endurance damage                                                                                 | PFC `CheckAlterant`                                              | applied while the world holds the thought fixed: on a rolled or fake check's failing branch (`failure_actions`), and as once actions on a Conceptualization or Encyclopedia passive; the check's ability comes from its `SkillType` field, which the index carries |
| a reputation action, with its copotype thought fixed          | money, and volition or endurance damage                                                                                 | PFC `ReputationAlterant.ReputationEffect`                        | applied while the world holds the thought fixed - see [Reputation](#reputation)                                                                                                                                                                                    |
| a failed white check being re-tested                          | the variable, for the five modifier expressions that are `SetVariableValue` calls rather than conditions                | PFC `FailedWhiteChecks.IsFailedWhiteCheckPossible`               | not followed - de-vdy9                                                                                                                                                                                                                                             |

## Keeping this current

The survey recognises a function by the `WRITES` table at the top of
`tools/survey-action-readers.py`, and marks a function it has no row for as `(no writes
recorded)`. When a script function appears that is not in the overview, read its body, add its
row there, rerun the survey, and add it here. `tools/align-markdown-tables.py docs/actions.md`
realigns the tables.
