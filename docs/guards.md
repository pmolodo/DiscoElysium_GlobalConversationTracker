# Guards

A guard is the Lua condition on a dialogue entry that decides whether the entry can be taken.
The look-ahead evaluates guards speculatively, thousands of times per menu, for entries the
player may never reach. This is the list of what those guards ask, how each question is
answered, and what checks the answer. Every guard, action and check the engine does not model
exactly is collected in [modelling-gaps.md](modelling-gaps.md).

The rule every row below follows: **no guard is answered by running Lua against the game.**
Each is computed in Rust, from dialogue variables or from data the plugin READS, and the
offline fixture supplies the same data from a save. A guard that could only be answered by
execution is also a guard that could write to the player's save when a menu opens, and two in
the corpus do (see [Actions called from a guard](#actions-called-from-a-guard)).

## The corpus

|                                           |  count |
| ----------------------------------------- | -----: |
| distinct guards                           | 13,059 |
| distinct guards that read `Variable[...]` | 12,234 |
| distinct guard functions called           |     49 |

Counted from the extractor's output under `.game_reference_copies/derived/`, which is local
and gitignored:

    tools/survey-guard-functions.py
    grep -c "Variable\[" .game_reference_copies/derived/distinct_guards.txt

`guards` below is the number of DISTINCT guard texts calling a function
(`distinct_guards.txt`); `entries` is the number of dialogue entries carrying one
(`conversation_index.jsonl`). The first is how many questions there are to get right; the
second is how often they are asked.

## Overview

Every guard function, by `entries`. The mechanisms are defined under
[Mechanisms](#mechanisms). `during a search` says whether the answer can change as a crawl
walks - see [What the search can move](#what-the-search-can-move):

- `tracked` - read from the search's own state, which the group's actions write where any do;
- `constant` - nothing a crawl can do changes it;
- `held` - a dialogue action can change it, and the engine answers from the crawl's starting
  value anyway, by a recorded decision or an approximation named in the family's section.

| function                    | family       | guards | entries | mechanism  | during a search                    |
| --------------------------- | ------------ | -----: | ------: | ---------- | ---------------------------------- |
| IsKimHere                   | party        |    323 |   4,687 | data, slot | tracked where removed              |
| CheckItem                   | inventory    |    493 |     835 | set        | tracked                            |
| IsTHCPresent                | cabinet      |    213 |     731 | set        | tracked                            |
| CheckEquipped               | equipment    |    199 |     514 | data, slot | tracked where lost                 |
| IsCunoInParty               | party        |     22 |     358 | data       | held by decision                   |
| IsTaskActive                | journal      |    204 |     341 | slot       | tracked                            |
| DayCount                    | clock        |     66 |     111 | port       | constant                           |
| IsHourBetween               | clock        |     63 |     108 | port       | held - clock locked                |
| IsHighestPolitical          | reputation   |     17 |      75 | slot       | tracked                            |
| IsEvening                   | clock        |      6 |      69 | port       | held - clock locked                |
| IsTHCFixed                  | cabinet      |     47 |      65 | data       | constant while the clock is locked |
| IsMorning                   | clock        |      7 |      60 | port       | held - clock locked                |
| IsNight                     | clock        |      4 |      54 | port       | held - clock locked                |
| IsAfternoon                 | clock        |      5 |      44 | port       | held - clock locked                |
| IsHighestCopotype           | reputation   |     12 |      43 | slot       | tracked                            |
| IsTHCCookingOrFixed         | cabinet      |     32 |      42 | data       | constant while the clock is locked |
| SubstanceUsedOnce           | substances   |     24 |      41 | slot       | tracked                            |
| MoneyAmount                 | money        |     31 |      37 | slot       | tracked                            |
| IsDayFrom                   | clock        |     16 |      36 | port       | constant                           |
| IsDaytime                   | clock        |      3 |      33 | port       | held - clock locked                |
| IsNighttime                 | clock        |      3 |      29 | port       | held - clock locked                |
| CheckItemGroup              | inventory    |     23 |      28 | set        | tracked                            |
| CheckEquippedGroup          | equipment    |     13 |      25 | data, slot | tracked where lost                 |
| SubstanceUsedMore           | substances   |     11 |      19 | slot       | tracked                            |
| IsExterior                  | scene        |      4 |      14 | data       | held by decision                   |
| HasJacket                   | equipment    |     10 |      12 | data, slot | tracked where lost                 |
| HasVolitionDamage           | damage       |      4 |      12 | data, slot | tracked                            |
| CheckHeldRightGroup         | equipment    |      8 |      12 | data, slot | tracked where lost                 |
| HasShirt                    | equipment    |      9 |      11 | data, slot | tracked where lost                 |
| IsDayUntil                  | clock        |      4 |      11 | port       | constant                           |
| FlagSet                     | flags        |      9 |      10 | slot       | tracked                            |
| HasEnduranceDamage          | damage       |      2 |       6 | data, slot | tracked                            |
| HasShoes                    | equipment    |      4 |       6 | data, slot | tracked where lost                 |
| IsHardcoreModeActive        | game mode    |      2 |       6 | data       | constant                           |
| WasGameBeatenInHardcoreMode | game mode    |      2 |       6 | data       | constant                           |
| HasHat                      | equipment    |      2 |       5 | data, slot | tracked where lost                 |
| TotalHourCount              | clock        |      5 |       5 | port       | held - clock locked                |
| FlagNotSet                  | flags        |      3 |       3 | slot       | tracked                            |
| IsTHCCooking                | cabinet      |      3 |       3 | data       | constant while the clock is locked |
| IsKimInParty                | party        |      2 |       3 | data, slot | tracked where removed              |
| HourCount                   | clock        |      2 |       2 | port       | held - clock locked                |
| IsDusk                      | clock        |      2 |       2 | port       | held - clock locked                |
| IsNoon                      | clock        |      2 |       2 | port       | held - clock locked                |
| IsRaining                   | scene        |      2 |       2 | slot       | tracked                            |
| IsSnowing                   | scene        |      2 |       2 | slot       | tracked                            |
| WeirdClothing               | equipment    |      2 |       2 | data, slot | tracked where lost                 |
| XPStandardSetBool           | guard action |      2 |       2 | answered   | not applicable                     |
| FinishTask                  | guard action |      1 |       1 | answered   | not applicable                     |
| HasPawnablesInInventory     | inventory    |      1 |       1 | data       | held - tab read at the start       |

## How a question is answered

Every guard is compiled or evaluated against a `BoundContext` (`src/world/mod.rs`). Its
`query` either answers a function itself or passes it to the world:

1. **Answered in `BoundContext::query`**, from search state where the search can move the
   answer and from the world's starting value where it cannot: the clock, flags, money, the
   two slot-backed sets, reputation, item groups, weather and substances. The journal's
   `IsTaskActive` never reaches it: the index rewrites it into variables first.
2. **Answered in `GameWorld::query`** (`src/world/game_world.rs`), from a `DataKind` the plugin
   read: party, equipment and clothing, cabinet states, damage, game mode, scene, inventory
   tabs.

`bridge::collect` decides what the plugin is asked for. It hands out a `DataRequest` per data
kind, a name per slot-backed subject, and the group's variables - and, today, **no query
keys**: nothing reaches the plugin as a call to evaluate. Two tests hold that in place:

- `tests/scenario_suites.rs` `every_question_the_suites_ask_is_answered_offline` fails if a
  suite group asks anything the offline world cannot answer.
- `tests/bridge_contract.rs` `the_engine_names_questions_the_snapshot_can_answer` puts a
  world through JSON and requires the same answers without it, skipping only calls the engine
  never handed out as keys.

The plugin services each data kind in `LookAheadRequestBuilder.Serviced`
(`src/GlobalConversationTracker.Plugin/LookAheadRequestBuilder.cs`); the offline fixture services the
same kinds from a save in `Holdings::data_for` (`tests/common/fixtures.rs`).

The GUARD COMPILER (`src/symbolic/guard_formula.rs`) decides guards on its own path, so a
function answered from search state also has an arm there - see
[Adding a guard function](#adding-a-guard-function).

### Mechanisms

How an answer is produced, which is a separate question from where its data comes from:

| mechanism  | meaning                                                                                                     |
| ---------- | ----------------------------------------------------------------------------------------------------------- |
| `port`     | computed in Rust from a number the plugin already sends (the clock, the day)                                |
| `slot`     | a dialogue variable or the money register, which the search's own actions write                             |
| `set`      | membership of a named subject: a slot where the group's actions move it, the starting set where they do not |
| `data`     | computed in Rust from a `DataKind` the plugin reads and the fixture reads from a save                       |
| `answered` | an action used as a guard, answered with the game's return value and never run                              |

### Unknown is permissive

Where data could not be read, the answer is Unknown, and Unknown lets a guard pass. The engine
is allowed to be wrong only in that direction: showing a route the game closes, never closing
one the game opens. Every data kind carries an explicit `read` flag for this reason - an empty
set that was read and a set nobody could read must not look alike.

## The 49 functions

`game source` says where the game's definition was taken from. **PFC** is the pre-final-cut
AssetRipper export, which has method bodies; Final Cut's export has them stripped, and a PFC
body is taken to be unchanged in Final Cut. **ISIL** is Cpp2IL's dump of the shipped
`GameAssembly.dll` (de-h0f1.8). **Measured** means the shipped game was asked over saves that
vary the state the function reads (de-h0f1.33). Each Rust module quotes the definition it
emulates beside the emulation.

### Clock and day

`ClockTime` in `src/core/clock.rs`, tested in `src/core/clock_tests.rs`. PFC:
`DaytimeLuaFunctions`, `SunshineClockTime`, `SunshineClock` - every function, including the
bucket boundaries and the wrapping `IsHourBetween`, was checked against those bodies (de-n5b8).

The plugin reads the clock to the hour (`GameFacts.ReadClock`, from `HourCount()` and
`DayCount()`) and always sends it locked, so `PassTime` does not move it during a search.
`owns` covers the hour questions, answered from the state's `day_minutes`; `owns_day` covers the
day questions, which no action within a conversation changes.

| function       | guards | entries | mechanism |
| -------------- | -----: | ------: | --------- |
| DayCount       |     66 |     111 | port      |
| IsHourBetween  |     63 |     108 | port      |
| IsDayFrom      |     16 |      36 | port      |
| IsMorning      |      7 |      60 | port      |
| IsEvening      |      6 |      69 | port      |
| IsAfternoon    |      5 |      44 | port      |
| TotalHourCount |      5 |       5 | port      |
| IsDayUntil     |      4 |      11 | port      |
| IsNight        |      4 |      54 | port      |
| IsDaytime      |      3 |      33 | port      |
| IsNighttime    |      3 |      29 | port      |
| HourCount      |      2 |       2 | port      |
| IsDusk         |      2 |       2 | port      |
| IsNoon         |      2 |       2 | port      |

#### Tests

- `DayCount`: `the_day_questions_answer_from_the_day_counter`, `hour_count_and_total_hour_count`
- `IsHourBetween`: `is_hour_between_is_inclusive_at_both_ends`, `is_hour_between_wraps_when_first_exceeds_second`
- `IsDayFrom`: `the_day_questions_answer_from_the_day_counter`, `a_day_question_without_its_argument_is_unknown`
- `IsMorning`: `is_morning_includes_dawn`
- `IsEvening`: `is_evening_includes_dusk`
- `IsAfternoon`: `is_afternoon_includes_noon`
- `TotalHourCount`: `hour_count_and_total_hour_count`
- `IsDayUntil`: `the_day_questions_answer_from_the_day_counter`
- `IsNight`: `night_predicates_are_different_sets`, `advance_wraps_past_midnight`
- `IsDaytime`: `midnight_is_neither_day_nor_night`
- `IsNighttime`: `night_predicates_are_different_sets`, `midnight_is_neither_day_nor_night`
- `HourCount`: `hour_count_and_total_hour_count`
- `IsDusk`: `daytime_of_matches_the_games_buckets` (the bucket, not the name)
- `IsNoon`: `daytime_of_matches_the_games_buckets` (the bucket, not the name)

The compiler holds the clock at the world's time too (`GuardCompiler::with_constant_clock`), for
the numbers as well as the conditions: `TotalHourCount() >= 30` is decided against the world's
hour, and so is a stored deadline read back against the clock - `TotalHourCount() >=
Variable["plaza.alice_serial_next_meeting_time"]` becomes a comparison of the slot against that
hour (`fixed_value`, `variable_against_query`). Tested by
`a_clock_question_is_answered_at_the_worlds_time`, `a_clock_number_is_compared_at_the_worlds_time`
and `a_clock_question_is_undecided_without_the_approximation` in `guard_formula.rs`, and by
`a_deadline_set_from_the_clock_is_read_back_exactly` in `backward.rs`.

`IsHour` and `IsMidnight` are ported too and appear in no guard.

### Inventory and cabinet: the slot-backed sets

Answered in `BoundContext::query` through `tracked_or_world`: a subject the group's actions
move has an `item:` or `thought:` slot and is read from search state; any other subject is
answered from the world's starting set (`initially_has_item`, `initially_has_thought`).

The plugin fills the starting sets in `LookAheadRequestBuilder.FillMembers` by calling the game's own
predicate once per named subject - a pure read, kept as Lua because re-implementing
`CharacterItems.IsItemGained` in C# would be a second copy that can drift (de-m7t2). The
fixture reads them from the save.

| function     | guards | entries | mechanism | game source                                                       | offline, from the save                                                           |
| ------------ | -----: | ------: | --------- | ----------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| CheckItem    |    493 |     835 | set       | PFC `CharacterItems.IsItemGained`                                 | bag and equipment, the key pocket for `key_ring` stacks, the count for `bullets` |
| IsTHCPresent |    213 |     731 | set       | PFC `CharacterThoughts.ThoughtGained` (`gainedThoughts.Contains`) | every cabinet state except `UNKNOWN` and `FORGOTTEN`                             |

#### Tests

- `CheckItem`: `a_tracked_item_compiles_against_its_slot`, `an_untracked_item_is_answered_from_the_worlds_inventory`, `a_tracked_item_ignores_the_worlds_starting_inventory`
- `IsTHCPresent`: `a_gained_thought_compiles_against_its_slot`, `an_ungained_thought_is_answered_from_the_save`

`bridge.rs` `the_slot_backed_queries_are_asked_for_by_subject` checks both are asked for by
name rather than as calls.

### Journal

| function     | guards | entries | mechanism | game source                     |
| ------------ | -----: | ------: | --------- | ------------------------------- |
| IsTaskActive |    204 |     341 | slot      | PFC `JournalModel.IsTaskActive` |

#### Tests

- `IsTaskActive`: `a_task_question_is_rewritten_over_its_variables` (`index::journal`)

A task's state is its three condition variables - show, done and cancel - which the journal
actions write and `JournalWatchman` keeps the game's own flags in step with on load. So
`IsTaskActive("x")` is REWRITTEN when the graph is built, by `Journal::with_tasks_as_variables`,
into `show and not done and not cancel` over the part `x` names, and for a subtask also its
parent's done and cancel. From there it is a guard over dialogue variables like any other: the
graph declares them, the plugin sends their values, the search reads its own slots for the ones
the group writes, and the compiler decides it. A name the journal does not know is `false`, as
the game answers. See `docs/actions.md` for the writes.

`FORGOTTEN` is not present because `CharacterThoughts.ForgetThought` removes the thought from
`gainedThoughts` as it sets that state.

### Cabinet states

`IsTHCCooking` and `IsTHCFixed` read collections that only the cabinet screen fills
(`cookingEffects`, `fixedEffects`), so no dialogue action moves them. Answered in
`GameWorld::query` from `DataKind::ThoughtsCooking` and `ThoughtsFixed`, each a set over the
group's named thoughts. The plugin builds each set by asking the game's predicate per thought
(`ThoughtsWhere`), because those dictionaries do not project through Il2CppInterop; the fixture
reads the save's cabinet states.

| function            | guards | entries | mechanism | game source                             |
| ------------------- | -----: | ------: | --------- | --------------------------------------- |
| IsTHCFixed          |     47 |      65 | data      | PFC `CharacterThoughts.ThoughtFixed`    |
| IsTHCCookingOrFixed |     32 |      42 | data      | PFC: `ThoughtCooking` or `ThoughtFixed` |
| IsTHCCooking        |      3 |       3 | data      | PFC `CharacterThoughts.ThoughtCooking`  |

#### Tests

- `IsTHCFixed`: `a_cabinet_state_question_names_its_thought`
- `IsTHCCookingOrFixed`: `a_cabinet_state_question_names_its_thought`
- `IsTHCCooking`: `a_cabinet_state_question_names_its_thought`

### Equipment and clothing

`src/core/equipment.rs`. Every question is answered from what each equipment slot holds:
`DataKind::EquippedInSlot`, which the plugin reads with `InventoryViewData.GetEquipped` and the
fixture reads from the save's `inventoryViewState.equipment`. The held-group and equipped-group
questions also read `DataKind::ItemsInGroup`. PFC: `InventoryLuaFunctions`,
`ClothingLuaFunctions`, `TequilaClothing`, `InventoryViewData`.

`CheckEquipped` reads all twelve slots and counts an item in any of them as equipped, since
`Equip` files an item under its own type's slot.

Where the group loses an item (`LoseItem`), a slot holding it at the start reads as empty once
its `unequipped:` slot is set - see `docs/actions.md`. Tested by
`losing_a_worn_item_takes_it_off`.

| function            | guards | entries | mechanism |
| ------------------- | -----: | ------: | --------- |
| CheckEquipped       |    199 |     514 | data      |
| CheckEquippedGroup  |     13 |      25 | data      |
| HasJacket           |     10 |      12 | data      |
| HasShirt            |      9 |      11 | data      |
| CheckHeldRightGroup |      8 |      12 | data      |
| HasShoes            |      4 |       6 | data      |
| HasHat              |      2 |       5 | data      |
| WeirdClothing       |      2 |       2 | data      |

#### Tests

- `CheckEquipped`: `an_item_in_any_slot_is_equipped`, `an_item_in_no_slot_is_not_equipped`, `an_unread_slot_leaves_a_missing_item_unknowable`, `an_unread_slot_does_not_unmake_an_item_found_elsewhere`; `bridge.rs` `check_equipped_is_answered_from_the_slots`, `check_equipped_reads_every_slot_rather_than_running_a_call`
- `CheckEquippedGroup`: `equipped_group_takes_a_slot_name_as_well_as_a_group`
- `HasJacket`: `a_clothing_question_asks_whether_its_slot_is_filled`
- `HasShirt`: none by name; the `WeirdClothing` tests read its slot
- `CheckHeldRightGroup`: `a_hand_holding_a_member_holds_the_group`, `a_held_item_of_unknown_group_membership_is_unknowable`; `bridge.rs` `a_held_group_question_reads_its_hand_and_the_group`
- `HasShoes`: `a_clothing_question_asks_whether_its_slot_is_filled`
- `HasHat`: `a_clothing_question_asks_whether_its_slot_is_filled`; `bridge.rs` `a_clothing_question_reads_its_slots_rather_than_running_a_call`
- `WeirdClothing`: `weird_clothing_is_true_without_a_shirt_whatever_the_shoes`, `weird_clothing_with_shirt_and_trousers_is_whether_barefoot`, `weird_clothing_is_settled_by_a_missing_shirt_even_with_other_slots_unread`

Also ported, and in no guard: `HasNecktie`, `HasPants`, `CheckHeldLeftGroup`.

### Item groups and inventory tabs

| function                | guards | entries | mechanism | Rust                                                  | data                               | game source                                             |
| ----------------------- | -----: | ------: | --------- | ----------------------------------------------------- | ---------------------------------- | ------------------------------------------------------- |
| CheckItemGroup          |     23 |      28 | set       | `core::item_group`, answered in `BoundContext::query` | `ItemsInGroup`, `HeldItemsInGroup` | PFC `Inventory.CheckItemGroup`, `ItemUtil.GetItemGroup` |
| HasPawnablesInInventory |      1 |       1 | data      | `core::inventory_tabs`                                | `TabHoldsItems`                    | PFC `InventoryViewData.IsTabEmpty(PAWNABLES)`           |

#### Tests

- `CheckItemGroup`: `a_member_held_from_the_start_holds_the_group`, `a_member_the_search_gained_holds_the_group`, `a_member_the_search_lost_does_not_hold_it`, `unread_members_leave_it_unknowable`, `a_group_with_no_members_is_never_held`; `bridge.rs` `an_item_group_and_a_tab_are_read_rather_than_run`
- `HasPawnablesInInventory`: `pawnables_is_the_pawnables_tab`; `bridge.rs` `an_item_group_and_a_tab_are_read_rather_than_run`

`CheckItemGroup` is turned round: the game walks what is held and asks each item its group, but
what is held changes during a search, so the engine reads the group's MEMBERS and answers each
one the way `CheckItem` is answered - from its slot if the group moves it, from the starting
inventory if not. The plugin reads membership from the live database's `itemGroup` field.

`HasPawnablesInInventory` reads the tab as it was when the crawl started: the engine does not
know which tab an item goes in, so a pawnable gained during the search is not seen.

An inventory TAB (`ItemTabGroup`: TOOLS, CLOTHES, PAWNABLES, READING) is not an item GROUP
(`ItemGroup`: alcohol, smokes and the other substance categories).

### Party

`src/core/party.rs`, from `DataKind::PartyFlag`. The plugin reads `IsInParty` and
`IsLeftOutside` off the party members, found by type because the generic singleton answers null
through the interop layer; the fixture reads the save's `partyState`.

| function      | guards | entries | mechanism | game source                                                                |
| ------------- | -----: | ------: | --------- | -------------------------------------------------------------------------- |
| IsKimHere     |    323 |   4,687 | data      | PFC `PartyManager.IsKimHere`, confirmed by ISIL and measured over 24 saves |
| IsCunoInParty |     22 |     358 | data      | PFC `PartyManager.IsCunoInParty`                                           |
| IsKimInParty  |      2 |       3 | data      | PFC `PartyManager.IsKimInParty`                                            |

#### Tests

- `IsKimHere`: `kim_is_here_when_in_the_party_and_not_left_outside`, `a_kim_out_of_the_party_is_not_here_whatever_else_is_unread`, `an_unread_flag_leaves_the_answer_unknowable`
- `IsCunoInParty`: `an_unread_flag_leaves_the_answer_unknowable`
- `IsKimInParty`: `a_kim_out_of_the_party_is_not_here_whatever_else_is_unread`

`IsKimHere` is `isKimInParty && !isKimLeftOutside`. The measurement found it reads no other
party flag: not `isKimAbandoned`, `isKimAwayUpToMorning` or `isKimSleepingInHisRoom`.

Where a group leaves Kim at the church (`RemoveKitsuragiWaitAtChurch`), `IsKimHere` and
`IsKimInParty` are false once its `party:kimRemoved` slot is set, and the world's answer until
then - see `docs/actions.md`. Tested by `leaving_kim_at_the_church_moves_the_kim_questions`.
Otherwise the party is held at the world's answer for the length of a search - a recorded
decision in `core::modelling`, since the other party actions (`ReturnKitsuragi`,
`AddCunoToParty` and the rest) have no downstream reader and a party model is a model of where
everyone is.

### Reputation

`src/core/reputation.rs`. PFC `ReputationAlterant.GetHighestReputationString`. The amounts are
the dialogue variables `reputation.<name>`, which `ReputationGrows`, `ReputationLowers` and
`Reputation` write, so the group declares the whole range compared and each amount is read
through `get_variable` - from the search's slot where the group moves it. The layout counts a
reputation question as reading every amount in its range, so those slots are kept and never
narrowed to a threshold: the amounts are compared with each other, not with a constant.

The symbolic compiler (`GuardCompiler::highest_reputation`) runs the same loop over sets of
states. It carries each possible pair of (reputation ahead, amount it is ahead by) together
with the states where the loop reaches that pair, and splits each pair by the amounts the next
reputation can hold in the state. Each amount is read the way the search reads it.

The compiler skips that work where no search from the request's starts can change the winner
(`GuardCompiler::settle_reputation`), and answers the question from the world. The game's loop
has a winner exactly where one amount is above zero and strictly above every other amount in
the range, so raises cannot change the winner if no other reputation can reach the winner's
amount. A raise is left out of the sum when it sits behind a guard, on every link path to it,
that requires its own reputation to be winning.

The raises counted are the ones in the menu's trimmed group (`bridge::walkable_menu`), so a
raise behind a guard that holds in no state is already gone.

Evrart's folder (785) is the case: every copotype raise is behind its own "is winning" guard.
The at-evart save also has raises elsewhere in Evrart's group. Those either can't reach
apocalypse_cop's lead, like 789:262's boring_cop, or are trimmed away behind a guard the save
keeps shut, like 605:169's superstar_cop. Any other write to the range - a lowering, or a raise
that could close the gap - leaves the question to the per-state comparison.

| function           | guards | entries | mechanism | range               |
| ------------------ | -----: | ------: | --------- | ------------------- |
| IsHighestPolitical |     17 |      75 | slot      | enum indices 4 to 8 |
| IsHighestCopotype  |     12 |      43 | slot      | enum indices 0 to 4 |

#### Tests

- `IsHighestPolitical`: `the_political_range_is_its_own_four`; `bridge.rs` `a_reputation_question_declares_the_range_it_compares`
- `IsHighestCopotype`: `nothing_is_winning_when_everything_is_zero`, `the_only_one_above_zero_wins`, `a_leading_zero_does_not_prevent_a_winner`, `a_tie_leaves_nothing_winning`, `a_later_higher_one_wins_back_a_cleared_tie`, `one_unreadable_reputation_makes_the_answer_unknown`, `a_query_names_every_variable_its_range_reads`
- Raised during a search, over conversation 767: `tests/reputation_writes.rs` `raising_a_tied_reputation_makes_it_win`, `a_tie_left_alone_wins_nothing`, `a_reputation_already_winning_stays_winning`
- The `reputation-branch` suite over `at-evart`: Evrart's 785:24 and its two replies are unmarked where apocalypse_cop is winning
- Answered from the world: `tests/reputation_writes.rs` `a_raise_to_the_winner_is_answered_from_the_world`, `a_raise_that_can_change_the_winner_is_left_to_the_search`; `tests/scenario_suites.rs` `evarts_copotype_split_is_answered_from_the_world`

The game's loop is not a maximum: a tie clears the winner, the running best starts at zero, and
a later higher entry wins back a cleared tie. The module's doc works through both.

### Money

| function    | guards | entries | mechanism | Rust                                                                                  |
| ----------- | -----: | ------: | --------- | ------------------------------------------------------------------------------------- |
| MoneyAmount |     31 |      37 | slot      | `world::MONEY_QUERY`, from the state's money register or the balance the plugin sends |

#### Tests

- `MoneyAmount`: `a_money_comparison_is_undecided_and_says_it_is_about_money`, `a_money_comparison_is_decided_once_money_has_a_register`

PFC `MoneyLuaFunctions.MoneyAmount` is `PlayerCharacter.Money`. The `GainMoney*` and
`LoseMoney*` actions move the register.

### Flags

| function   | guards | entries | mechanism | game source                            |
| ---------- | -----: | ------: | --------- | -------------------------------------- |
| FlagSet    |      9 |      10 | slot      | ISIL: `LuaHelper.GetVariable`          |
| FlagNotSet |      3 |       3 | slot      | ISIL: `LuaHelper.GetVariable`, negated |

#### Tests

- `FlagSet`: `a_flag_is_decided_against_its_slot_either_way_round`; `bridge.rs` `a_flag_is_asked_for_as_a_variable`
- `FlagNotSet`: `a_flag_is_decided_against_its_slot_either_way_round`; `bridge.rs` `a_flag_asked_about_negatively_is_also_asked_for_as_a_variable`

A flag is a dialogue variable, and `world::flag_query` is the one place that says so. `SetFlag`
and `UnsetFlag` write the same variable.

### Substances

`src/core/substance.rs`. PFC `InventoryLuaFunctions` and `HudHeldPanelController`: the count is
the dialogue variable `stats.uses_<substance>`, read like any other variable.

| function          | guards | entries | mechanism | threshold |
| ----------------- | -----: | ------: | --------- | --------- |
| SubstanceUsedOnce |     24 |      41 | slot      | count > 0 |
| SubstanceUsedMore |     11 |      19 | slot      | count > 3 |

#### Tests

- `SubstanceUsedOnce`: `once_is_any_use_at_all`, `an_unread_count_is_unknown`; `bridge.rs` `a_substance_question_asks_for_its_count_variable`
- `SubstanceUsedMore`: `more_is_more_than_three`; `bridge.rs` `a_substance_question_asks_for_its_count_variable`

### Damage

`src/core/damage.rs`, from `DataKind::SkillDamage`. PFC `CharacterLuaFunctions` and `Modifiable`:
damaged means the skill's `damageValue` is below zero. The plugin reads `damageValue`; the
fixture sums the skill's `DAMAGE` modifiers from the save's character sheet. Where the group
damages or heals the skill, the answer comes from its `damage:` slot instead - see
`docs/actions.md`.

| function           | guards | entries | mechanism |
| ------------------ | -----: | ------: | --------- |
| HasVolitionDamage  |      4 |      12 | data      |
| HasEnduranceDamage |      2 |       6 | data      |

#### Tests

- `HasVolitionDamage`: `damage_is_a_negative_value`; `bridge.rs` `a_damage_question_reads_the_skill_and_compares_below_zero`
- `HasEnduranceDamage`: `damage_is_a_negative_value`

Tested as moved by the search in `damage_and_healing_move_the_damage_question`, in both
`oracle.rs` and `backward.rs`.

### Scene and weather

`src/core/scene.rs`, tested in `tests/scene_queries.rs` against the committed `scene-*` saves.

| function   | guards | entries | mechanism | data                                                                          | game source                      |
| ---------- | -----: | ------: | --------- | ----------------------------------------------------------------------------- | -------------------------------- |
| IsExterior |      4 |      14 | data      | `SceneIsOutside`; offline, the save's area looked up in `testing/scenes.json` | PFC `MapLuaFunctions.IsExterior` |
| IsRaining  |      2 |       2 | slot      | the variable `auto.is_raining`                                                | ISIL, and measured               |
| IsSnowing  |      2 |       2 | slot      | the variable `auto.is_snowing`                                                | ISIL                             |

#### Tests

- `IsExterior`: `the_engine_asks_about_the_scene_where_the_guards_do`, `every_conversation_that_guards_on_the_scene_asks_about_it`
- `IsRaining`: `the_weather_is_its_variable`, `the_weather_saves_answer_the_weather_they_were_made_with`
- `IsSnowing`: `the_weather_is_its_variable`, `the_engine_asks_about_the_scene_where_the_guards_do`

`WeatherController` rewrites both weather variables from the preset on load, and the offline
reader simulates that step rather than editing the committed saves.

### Game mode

`src/core/game_mode.rs`. Both functions exist only in Final Cut.

| function                    | guards | entries | mechanism | data                                                                     | game source                                                          |
| --------------------------- | -----: | ------: | --------- | ------------------------------------------------------------------------ | -------------------------------------------------------------------- |
| IsHardcoreModeActive        |      2 |       6 | data      | `GameMode`; offline, the save's `gameModeState.gameMode`                 | ISIL `GameModeController.IsHardcoreOn`, and measured over four saves |
| WasGameBeatenInHardcoreMode |      2 |       6 | data      | `HardcorePlaythroughCompleted`; offline, a fixed value, false by default | ISIL `GameStatsManager.HardcorePlaythroughCompleted`                 |

#### Tests

- `IsHardcoreModeActive`: `bridge.rs` `the_hardcore_question_reads_the_game_mode`
- `WasGameBeatenInHardcoreMode`: `bridge.rs` `the_hardcore_question_reads_the_game_mode`

`WasGameBeatenInHardcoreMode` is profile state, not save state, so no save records it. The
offline worlds answer `fixtures::HARDCORE_PLAYTHROUGH_COMPLETED`, which is false, and a scenario
row fixes it otherwise with `hardcorePlaythroughCompleted`.

### Actions called from a guard

| function          | guards | entries | mechanism | game source                                                               |
| ----------------- | -----: | ------: | --------- | ------------------------------------------------------------------------- |
| XPStandardSetBool |      2 |       2 | answered  | PFC `TaskLuaFunctions.XPSetBool`: sets the variable and awards experience |
| FinishTask        |      1 |       1 | answered  | PFC `JournalModel.FinishTask`: closes the task                            |

#### Tests

- `XPStandardSetBool`: `bridge.rs` `an_action_called_from_a_guard_is_never_asked_for`
- `FinishTask`: `bridge.rs` `an_action_called_from_a_guard_is_never_asked_for`

Two functions that return nothing and exist for their effect appear where a condition is
expected: conversation 369 entry 94 (`FinishTask`), and conversation 850 entries 109 and 110
(`XPStandardSetBool`). Evaluated as Lua, they would finish a task or award experience in the
player's save on every crawl of those groups. `core::modelling::ACTIONS_USED_AS_GUARDS` lists
them; `bridge::collect` never asks for them, `BoundContext::query` answers them, and the answer
is `false` - the game's own answer, since a void function returns nil and all three guards
compare it against `true`.

The search does not follow their effect. The game really does finish that task while deciding
whether to show the line; a write on the guard path would be a different feature.

## Dialogue variables

12,234 of the 13,059 distinct guards read `Variable["name"]` directly. A variable a group's
actions write has a slot and is read from search state; any other is read from the values the
plugin sends, positionally, for exactly the variables the group declares (`Questions::variables`),
falling back to the database's declared initial value where the plugin could not read one.

## Check kinds

Not guard functions, but they decide whether an entry can be taken the same way.
`ReturnDialogueOptionValidator.IsEntryValid` dispatches on seven cases:

| validator branch                    | engine                               | plugin                             | offline                                                  |
| ----------------------------------- | ------------------------------------ | ---------------------------------- | -------------------------------------------------------- |
| `PassiveNode.CheckSuccess`          | `Passive`                            | `PassiveCheckRule`                 | `checks_in_save`, through `core::passive_check::outcome` |
| `RedCheckNode.IsRedCheckDecided`    | `Red`                                | `ThoughtAlterant.RedChecksFail`    | `passive_thoughts_in_save`                               |
| `WhiteCheckNode.IsWhiteCheckPassed` | `White`                              | `FailedWhiteChecks.ChecksBySkill`  | `failed_white_checks_in_save`                            |
| `FakeCheckNode.IsFakeCheckDone`     | `Fake`                               | the seen set                       | `closes_once_seen`                                       |
| `TestOptionNode.IsHidden`           | `Test`                               | not applicable - goes nowhere      | not applicable                                           |
| `KimSwitchNode.IsAvailable`         | `KimSwitch`                          | the seen set unless `boolean_only` | the same                                                 |
| `CostOptionNode.IsHidden`           | not a kind; `ClickCost` / `CostOnce` | money, and `GameMode`              | money, and the save's `gameModeState.gameMode`           |

The passive row shares its arithmetic as well as its shape: the offline side calls the plugin's
own comparison rather than repeating it.

The cost row reads the game mode as well as money: in hardcore mode `CostOptionNode.GetCost`
doubles the price of healing and drug purchases. A group with a scaled purchase asks for
`GameMode` even where no guard calls `IsHardcoreModeActive` - see
[Money in actions.md](actions.md#money).

## Hidden reads

A hidden read is state a guard or a check kind reads without naming it in its own text. Each one
has to be declared, asked for and tracked like a named read, or the engine answers it from a
value nothing tells it about. The writes on the other side are in
[Hidden writes in actions.md](actions.md#hidden-writes).

No guard in the corpus reads an entry's `SimStatus` directly.

| reader                                    | reads without naming it                                                                                                                                                                            | game source                                                                         | engine                                                                                                                                                                                                                                                                                    |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `IsTaskActive`                            | the part's show, done and cancel variables, and its parent's done and cancel                                                                                                                       | PFC `JournalModel.IsTaskActive`                                                     | tracked: rewritten into those variables when the graph is built                                                                                                                                                                                                                           |
| `IsRaining`, `IsSnowing`                  | `auto.is_raining`, `auto.is_snowing`                                                                                                                                                               | ISIL                                                                                | tracked variables                                                                                                                                                                                                                                                                         |
| `SubstanceUsedOnce`, `SubstanceUsedMore`  | `stats.uses_<substance>`                                                                                                                                                                           | PFC `InventoryLuaFunctions`                                                         | tracked variable                                                                                                                                                                                                                                                                          |
| `IsHighestCopotype`, `IsHighestPolitical` | every `reputation.<name>` in the range compared                                                                                                                                                    | PFC `ReputationAlterant.GetHighestReputationString`                                 | tracked variables, declared as the whole range                                                                                                                                                                                                                                            |
| `FlagSet`, `FlagNotSet`                   | the variable named                                                                                                                                                                                 | ISIL `LuaHelper.GetVariable`                                                        | tracked variable                                                                                                                                                                                                                                                                          |
| passive check                             | the skill's value, which sums every modifier - equipment bonuses and the `DAMAGE` modifier included - and the thoughts that force or shift a passive (`PassiveSuccessList`, `PassiveModifiedList`) | PFC `PassiveNode.CheckSuccess`, `Modifiable.Recalc`, `ThoughtAlterant`              | held: the plugin evaluates each check once per request (`PassiveCheckRule`), and where the group can change what is worn - a worn item lost, an autoequip item gained - or where its damage or healing can cross the check's margin - the check is Unknown instead (`core::skill_movers`) |
| red check                                 | `FlagName` and `FlagName_failed`; `ThoughtAlterant.RedChecksFail`                                                                                                                                  | PFC `RedCheckNode.IsRedCheckDecided`                                                | flags tracked (`flag_slot`, `failed_flag_slot`); `RedChecksFail` constant                                                                                                                                                                                                                 |
| white check                               | `FlagName`; the failed-check cache, which reopens a check when its skill rank rises or a modifier expression (`variable1` to `variable10`) lowers the target                                       | PFC `WhiteCheckNode`, `FailedWhiteChecks.IsFailedWhiteCheckPossible`                | flag tracked; a failure is a `FlagName_failed` slot that nothing clears, so a reopened check stays closed - de-vdy9                                                                                                                                                                       |
| white check difficulty                    | `GameModeController.WhiteCheckModifier`, +1 in hardcore                                                                                                                                            | PFC `CheckNodeUtil.GetCheckDifficulty`                                              | not read: difficulty moves the odds, and a search takes both outcomes of a roll whatever they are                                                                                                                                                                                         |
| fake check                                | whether it is seen; `HandleResponseText` also hides it once `FlagName` or `FlagName_failed` is set                                                                                                 | PFC `FakeCheckNode`                                                                 | seen tracked; the flags are not read, which can only show an option the game hides                                                                                                                                                                                                        |
| Kim switch                                | its own condition, and whether it is seen unless `boolean_only`                                                                                                                                    | PFC `KimSwitchNode.IsAvailable`                                                     | tracked                                                                                                                                                                                                                                                                                   |
| priced entry                              | money; the game mode; the conditions on the walk to the item bought                                                                                                                                | PFC `CostOptionNode`                                                                | money tracked, mode read as `GameMode`; a walk with a condition on it is priced unscaled - see [Money in actions.md](actions.md#money)                                                                                                                                                    |
| `Once()` in a script, `CostOnce`          | whether the entry running it is seen                                                                                                                                                               | PFC `GenericLuaFunctions.Once`, `SunshineNode.IsSeen`, `CostOptionNode.HandleEntry` | a `once:` slot, seeded from the seen set - see [Once in actions.md](actions.md#once)                                                                                                                                                                                                      |

## Thought effects

A thought changes what dialogue sees in two ways: an effect it applies while cooking or fixed
(`CharacterEffect.Apply`), and game code that branches on whether it is fixed. Only `PassTime`
bakes a thought, and the plugin sends the clock locked, so which thoughts are cooking or fixed is
constant for a search. What matters is whether a thought changes an answer, or a write, while a
search runs. Taken from the pre-final-cut bodies, since Final Cut's export has them stripped.

| effect                                                                                                                                                                      | what it changes                                                                                                                           | engine                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `PASSIVE_TARGET_MODIFIER`, `PASSIVES_SUCCEED`                                                                                                                               | a passive check's threshold, or forces it through                                                                                         | read: `PassiveCheckRule` in game, `testing/thought-effects.json` offline; measured (de-2jlj) |
| `THC_RED_CHECK_FAILURE`                                                                                                                                                     | every red check fails                                                                                                                     | read: `RedChecksFail` closes a red check's success                                           |
| `SKILL_BONUS`, `STAT_BONUS`                                                                                                                                                 | skill values, and so passive checks                                                                                                       | read through the skill value the plugin evaluates checks with                                |
| `MALE_TARGET`, `FEMALE_TARGET`, `KIM_TARGET`, `SKILL_BONUS_WHEN_UNARMED`, `SKILL_BONUS_WITHOUT_SHIRT`, `THC_CRIT_RANGE_EXPAND`                                              | a white or red check's odds (`SituationalCheckModifiers`)                                                                                 | not read: a search takes both outcomes of a roll                                             |
| `REPUTATION_BONUS`, `DAMAGE`, `HEAL`, `XP_REWARD`, `LUA_COMMAND`, `REOPEN_WHITE`                                                                                            | applied once when the thought is researched or fixed                                                                                      | not needed: the result is in the state the plugin reads                                      |
| `THC_ORB_MONEY`, `THC_ORB_XP`, `COMMUNISM_XP_BONUS`, `DRUGS_ARE_BAD_MKAY`, `FIND_BETTER_ITEMS`, `MAX_LEARNING_CAP`, `MOUTH_SLOT`, `MODIFY_CAMERA_MAX_ZOOM_LIMIT`, `TOOLTIP` | orbs, experience, containers, substances, the camera                                                                                      | not needed: nothing a dialogue guard, check or price reads                                   |
| `CheckAlterant` after a check result                                                                                                                                        | money (`return_on_investment`, `trant_heidelstam`), volition and endurance damage (`superstar_cop`, `kras_mazov`, `sorry_cop`, `art_cop`) | applied while the world holds the thought fixed (`core::thought_effects`)                    |
| `ReputationAlterant.ReputationEffect` after a reputation action                                                                                                             | money (`ultraliberal`), volition and endurance damage (`moralist`, `the_destroyer`, `revacholian_nationhood`)                             | applied while the world holds the thought fixed (`core::thought_effects`)                    |
| `Alterant` substance and item branches                                                                                                                                      | what a substance or a tare item does when used                                                                                            | not needed: dialogue does not use items                                                      |

## What the search can move

A question is only exact from a starting value while nothing along the search path writes what
it reads. Three positions, per function above:

- **Tracked.** The answer is read from search state, which the search's own actions write: the
  two slot-backed sets, the journal, `CheckItemGroup`'s members, reputation, flags, money,
  substance counts, weather, damage, equipment a group takes away, Kim left at the church, and
  every `Variable[...]` read.
- **Constant.** No dialogue action can write the data: game mode and the day - and the cabinet
  states while the clock is locked, since only passing time bakes a thought.
- **Held.** Dialogue actions CAN write the data, and the engine answers from the starting value
  anyway, by a decision recorded in `core::modelling::DECISIONS`, a note in the module, or the
  locked clock: the party writers other than leaving Kim at the church, the clock, the pawnables
  tab, the scene.

Which actions write what, and whether any of them reaches a guard that reads it, is in
[actions.md](actions.md). Equipment is tracked rather than constant: `LoseItem` unequips what it
deletes (`Inventory.DeleteItem`), and 26 call sites have an equipment question downstream.

`UseSubstanceInHand` writes `stats.uses_<group>` (`HudHeldPanelController.OnSubstanceUse`), and no
call site has a substance question downstream, so the count is read from the world.

## Adding a guard function

A function answered from search state is honoured in FIVE places, and they have to agree. A
function honoured in some of them is worse than one honoured in none: it gets declared and
slotted and then read from a snapshot that was never told about it, which is the starting value
forever.

1. `src/graph/mod.rs` declares the subject as one of the group's variables, which makes it
   readable at all.
2. `src/symbolic/data_layout.rs` spends a slot on it.
3. `src/bridge.rs` `collect` keeps it out of what the plugin is asked, or asks for its data.
4. `src/world/mod.rs` `BoundContext::query` (or `GameWorld::query`) answers it.
5. `src/symbolic/guard_formula.rs` - both the compile arm and `search_can_change` - decides it,
   because the compiler runs on its own path, and a compiler less decisive than the engine
   leaves branches open that the search closes.

Name the query once, as a constant or a small predicate in its `core` module, and use that name
at every site - that is what makes the five findable with one search. Quote the game's
definition beside the emulation and name where it came from. Then:

- give the data a `DataKind` if it is not a variable, serviced by a READ in
  `LookAheadRequestBuilder.Serviced` and by the save in `Holdings::data_for`;
- run `tools/survey-guard-functions.py` and update the tables here.
