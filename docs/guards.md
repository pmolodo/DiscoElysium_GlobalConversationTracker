# Guards

A guard is the Lua condition on a dialogue entry that decides whether the entry can be taken.
The look-ahead evaluates guards speculatively, thousands of times per menu, for entries the
player may never reach. This is the list of what those guards ask, how each question is
answered, and what checks the answer.

The rule every row below follows: **no guard is answered by running Lua against the game.**
Each is computed in Rust, from dialogue variables or from data the plugin READS, and the
offline fixture supplies the same data from a save. A guard that could only be answered by
execution is also a guard that could write to the player's save when a menu opens, and two in
the corpus do (see [Actions called from a guard](#actions-called-from-a-guard)).

## The corpus

| | count |
| --- | ---: |
| distinct guards | 13,059 |
| distinct guards that read `Variable[...]` | 12,234 |
| distinct guard functions called | 49 |

Counted from the extractor's output under `.game_reference_copies/derived/`, which is local
and gitignored:

    tools/survey-guard-functions.py
    grep -c "Variable\[" .game_reference_copies/derived/distinct_guards.txt

`guards` below is the number of DISTINCT guard texts calling a function
(`distinct_guards.txt`); `entries` is the number of dialogue entries carrying one
(`conversation_index.jsonl`). The first is how many questions there are to get right; the
second is how often they are asked.

## How a question is answered

Every guard is compiled or evaluated against a `BoundContext` (`src/world/mod.rs`). Its
`query` either answers a function itself or passes it to the world:

1. **Answered in `BoundContext::query`**, from search state where the search can move the
   answer and from the world's starting value where it cannot: the clock, flags, money, the
   three slot-backed sets, reputation, item groups, weather and substances.
2. **Answered in `SnapshotWorld::query`** (`src/bridge.rs`), from a `DataKind` the plugin
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

The plugin services each data kind in `GameWorldSnapshot.Serviced`
(`src/GlobalConversationTracker.Plugin/GameWorldSnapshot.cs`); the offline fixture services the
same kinds from a save in `Holdings::data_for` (`tests/common/fixtures.rs`).

The GUARD COMPILER (`src/symbolic/guard_formula.rs`) decides guards on its own path, so a
function answered from search state also has an arm there - see
[Adding a guard function](#adding-a-guard-function).

### Mechanisms

How an answer is produced, which is a separate question from where its data comes from:

| mechanism | meaning |
| --- | --- |
| `port` | computed in Rust from a number the plugin already sends (the clock, the day) |
| `slot` | a dialogue variable or the money register, which the search's own actions write |
| `set` | membership of a named subject: a slot where the group's actions move it, the starting set where they do not |
| `data` | computed in Rust from a `DataKind` the plugin reads and the fixture reads from a save |
| `answered` | an action used as a guard, answered with the game's return value and never run |

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

| function | guards | entries | mechanism | tests |
| --- | ---: | ---: | --- | --- |
| DayCount | 66 | 111 | port | `the_day_questions_answer_from_the_day_counter`, `hour_count_and_total_hour_count` |
| IsHourBetween | 63 | 108 | port | `is_hour_between_is_inclusive_at_both_ends`, `is_hour_between_wraps_when_first_exceeds_second` |
| IsDayFrom | 16 | 36 | port | `the_day_questions_answer_from_the_day_counter`, `a_day_question_without_its_argument_is_unknown` |
| IsMorning | 7 | 60 | port | `is_morning_includes_dawn` |
| IsEvening | 6 | 69 | port | `is_evening_includes_dusk` |
| IsAfternoon | 5 | 44 | port | `is_afternoon_includes_noon` |
| TotalHourCount | 5 | 5 | port | `hour_count_and_total_hour_count` |
| IsDayUntil | 4 | 11 | port | `the_day_questions_answer_from_the_day_counter` |
| IsNight | 4 | 54 | port | `night_predicates_are_different_sets`, `advance_wraps_past_midnight` |
| IsDaytime | 3 | 33 | port | `midnight_is_neither_day_nor_night` |
| IsNighttime | 3 | 29 | port | `night_predicates_are_different_sets`, `midnight_is_neither_day_nor_night` |
| HourCount | 2 | 2 | port | `hour_count_and_total_hour_count` |
| IsDusk | 2 | 2 | port | `daytime_of_matches_the_games_buckets` (the bucket, not the name) |
| IsNoon | 2 | 2 | port | `daytime_of_matches_the_games_buckets` (the bucket, not the name) |

The compiler's clock arms are tested by `a_clock_question_is_answered_at_the_worlds_time` and
`a_clock_question_is_undecided_without_the_approximation` in `guard_formula.rs`.

`IsHour` and `IsMidnight` are ported too and appear in no guard.

### Inventory, journal and cabinet: the slot-backed sets

Answered in `BoundContext::query` through `tracked_or_world`: a subject the group's actions
move has an `item:`, `task:` or `thought:` slot and is read from search state; any other
subject is answered from the world's starting set (`initially_has_item`,
`initially_task_active`, `initially_has_thought`).

The plugin fills the starting sets in `GameWorldSnapshot.FillMembers` by calling the game's own
predicate once per named subject - a pure read, kept as Lua because re-implementing
`CharacterItems.IsItemGained` in C# would be a second copy that can drift (de-m7t2). The
fixture reads them from the save.

| function | guards | entries | mechanism | game source | offline, from the save | tests |
| --- | ---: | ---: | --- | --- | --- | --- |
| CheckItem | 493 | 835 | set | PFC `CharacterItems.IsItemGained` | bag and equipment, the key pocket for `key_ring` stacks, the count for `bullets` | `a_tracked_item_compiles_against_its_slot`, `an_untracked_item_is_answered_from_the_worlds_inventory`, `a_tracked_item_ignores_the_worlds_starting_inventory` |
| IsTHCPresent | 213 | 731 | set | PFC `CharacterThoughts.ThoughtGained` (`gainedThoughts.Contains`) | every cabinet state except `UNKNOWN` and `FORGOTTEN` | `a_gained_thought_compiles_against_its_slot`, `an_ungained_thought_is_answered_from_the_save` |
| IsTaskActive | 204 | 341 | set | PFC `JournalModel.IsTaskActive` | acquired and not resolved | `a_task_question_reads_the_task_slot` |

`bridge.rs` `the_slot_backed_queries_are_asked_for_by_subject` checks all three are asked for
by name rather than as calls.

`FORGOTTEN` is not present because `CharacterThoughts.ForgetThought` removes the thought from
`gainedThoughts` as it sets that state.

### Cabinet states

`IsTHCCooking` and `IsTHCFixed` read collections that only the cabinet screen fills
(`cookingEffects`, `fixedEffects`), so no dialogue action moves them. Answered in
`SnapshotWorld::query` from `DataKind::ThoughtsCooking` and `ThoughtsFixed`, each a set over the
group's named thoughts. The plugin builds each set by asking the game's predicate per thought
(`ThoughtsWhere`), because those dictionaries do not project through Il2CppInterop; the fixture
reads the save's cabinet states.

| function | guards | entries | mechanism | game source | tests |
| --- | ---: | ---: | --- | --- | --- |
| IsTHCFixed | 47 | 65 | data | PFC `CharacterThoughts.ThoughtFixed` | `a_cabinet_state_question_names_its_thought` |
| IsTHCCookingOrFixed | 32 | 42 | data | PFC: `ThoughtCooking` or `ThoughtFixed` | `a_cabinet_state_question_names_its_thought` |
| IsTHCCooking | 3 | 3 | data | PFC `CharacterThoughts.ThoughtCooking` | `a_cabinet_state_question_names_its_thought` |

### Equipment and clothing

`src/core/equipment.rs`. Every question is answered from what each equipment slot holds:
`DataKind::EquippedInSlot`, which the plugin reads with `InventoryViewData.GetEquipped` and the
fixture reads from the save's `inventoryViewState.equipment`. The held-group and equipped-group
questions also read `DataKind::ItemsInGroup`. PFC: `InventoryLuaFunctions`,
`ClothingLuaFunctions`, `TequilaClothing`, `InventoryViewData`.

`CheckEquipped` reads all twelve slots and counts an item in any of them as equipped, since
`Equip` files an item under its own type's slot.

| function | guards | entries | mechanism | tests |
| --- | ---: | ---: | --- | --- |
| CheckEquipped | 199 | 514 | data | `an_item_in_any_slot_is_equipped`, `an_item_in_no_slot_is_not_equipped`, `an_unread_slot_leaves_a_missing_item_unknowable`, `an_unread_slot_does_not_unmake_an_item_found_elsewhere`; `bridge.rs` `check_equipped_is_answered_from_the_slots`, `check_equipped_reads_every_slot_rather_than_running_a_call` |
| CheckEquippedGroup | 13 | 25 | data | `equipped_group_takes_a_slot_name_as_well_as_a_group` |
| HasJacket | 10 | 12 | data | `a_clothing_question_asks_whether_its_slot_is_filled` |
| HasShirt | 9 | 11 | data | none by name; the `WeirdClothing` tests read its slot |
| CheckHeldRightGroup | 8 | 12 | data | `a_hand_holding_a_member_holds_the_group`, `a_held_item_of_unknown_group_membership_is_unknowable`; `bridge.rs` `a_held_group_question_reads_its_hand_and_the_group` |
| HasShoes | 4 | 6 | data | `a_clothing_question_asks_whether_its_slot_is_filled` |
| HasHat | 2 | 5 | data | `a_clothing_question_asks_whether_its_slot_is_filled`; `bridge.rs` `a_clothing_question_reads_its_slots_rather_than_running_a_call` |
| WeirdClothing | 2 | 2 | data | `weird_clothing_is_true_without_a_shirt_whatever_the_shoes`, `weird_clothing_with_shirt_and_trousers_is_whether_barefoot`, `weird_clothing_is_settled_by_a_missing_shirt_even_with_other_slots_unread` |

Also ported, and in no guard: `HasNecktie`, `HasPants`, `CheckHeldLeftGroup`.

### Item groups and inventory tabs

| function | guards | entries | mechanism | Rust | data | game source | tests |
| --- | ---: | ---: | --- | --- | --- | --- | --- |
| CheckItemGroup | 23 | 28 | set | `core::item_group`, answered in `BoundContext::query` | `ItemsInGroup`, `HeldItemsInGroup` | PFC `Inventory.CheckItemGroup`, `ItemUtil.GetItemGroup` | `a_member_held_from_the_start_holds_the_group`, `a_member_the_search_gained_holds_the_group`, `a_member_the_search_lost_does_not_hold_it`, `unread_members_leave_it_unknowable`, `a_group_with_no_members_is_never_held`; `bridge.rs` `an_item_group_and_a_tab_are_read_rather_than_run` |
| HasPawnablesInInventory | 1 | 1 | data | `core::inventory_tabs` | `TabHoldsItems` | PFC `InventoryViewData.IsTabEmpty(PAWNABLES)` | `pawnables_is_the_pawnables_tab`; `bridge.rs` `an_item_group_and_a_tab_are_read_rather_than_run` |

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

| function | guards | entries | mechanism | game source | tests |
| --- | ---: | ---: | --- | --- | --- |
| IsKimHere | 323 | 4,687 | data | PFC `PartyManager.IsKimHere`, confirmed by ISIL and measured over 24 saves | `kim_is_here_when_in_the_party_and_not_left_outside`, `a_kim_out_of_the_party_is_not_here_whatever_else_is_unread`, `an_unread_flag_leaves_the_answer_unknowable` |
| IsCunoInParty | 22 | 358 | data | PFC `PartyManager.IsCunoInParty` | `an_unread_flag_leaves_the_answer_unknowable` |
| IsKimInParty | 2 | 3 | data | PFC `PartyManager.IsKimInParty` | `a_kim_out_of_the_party_is_not_here_whatever_else_is_unread` |

`IsKimHere` is `isKimInParty && !isKimLeftOutside`. The measurement found it reads no other
party flag: not `isKimAbandoned`, `isKimAwayUpToMorning` or `isKimSleepingInHisRoom`.

Party is held at the world's answer for the length of a search - a recorded decision in
`core::modelling`, since the party actions (`ReturnKitsuragi`, `AddCunoToParty` and the rest)
are rare and a party model is a model of where everyone is.

### Reputation

`src/core/reputation.rs`. PFC `ReputationAlterant.GetHighestReputationString`. The amounts are
the dialogue variables `reputation.<name>`, which `ReputationGrows`, `ReputationLowers` and
`Reputation` write, so the group declares the whole range compared and each amount is read
through `get_variable` - from the search's slot where the group moves it.

| function | guards | entries | mechanism | range | tests |
| --- | ---: | ---: | --- | --- | --- |
| IsHighestPolitical | 17 | 75 | slot | enum indices 4 to 8 | `the_political_range_is_its_own_four`; `bridge.rs` `a_reputation_question_declares_the_range_it_compares` |
| IsHighestCopotype | 12 | 43 | slot | enum indices 0 to 4 | `nothing_is_winning_when_everything_is_zero`, `the_only_one_above_zero_wins`, `a_leading_zero_does_not_prevent_a_winner`, `a_tie_leaves_nothing_winning`, `a_later_higher_one_wins_back_a_cleared_tie`, `one_unreadable_reputation_makes_the_answer_unknown`, `a_query_names_every_variable_its_range_reads` |

The game's loop is not a maximum: a tie clears the winner, the running best starts at zero, and
a later higher entry wins back a cleared tie. The module's doc works through both.

### Money

| function | guards | entries | mechanism | Rust | tests |
| --- | ---: | ---: | --- | --- | --- |
| MoneyAmount | 31 | 37 | slot | `world::MONEY_QUERY`, from the state's money register or the balance the plugin sends | `a_money_comparison_is_undecided_and_says_it_is_about_money`, `a_money_comparison_is_decided_once_money_has_a_register` |

PFC `MoneyLuaFunctions.MoneyAmount` is `PlayerCharacter.Money`. The `GainMoney*` and
`LoseMoney*` actions move the register.

### Flags

| function | guards | entries | mechanism | game source | tests |
| --- | ---: | ---: | --- | --- | --- |
| FlagSet | 9 | 10 | slot | ISIL: `LuaHelper.GetVariable` | `a_flag_is_decided_against_its_slot_either_way_round`; `bridge.rs` `a_flag_is_asked_for_as_a_variable` |
| FlagNotSet | 3 | 3 | slot | ISIL: `LuaHelper.GetVariable`, negated | `a_flag_is_decided_against_its_slot_either_way_round`; `bridge.rs` `a_flag_asked_about_negatively_is_also_asked_for_as_a_variable` |

A flag is a dialogue variable, and `world::flag_query` is the one place that says so. `SetFlag`
and `UnsetFlag` write the same variable.

### Substances

`src/core/substance.rs`. PFC `InventoryLuaFunctions` and `HudHeldPanelController`: the count is
the dialogue variable `stats.uses_<substance>`, read like any other variable.

| function | guards | entries | mechanism | threshold | tests |
| --- | ---: | ---: | --- | --- | --- |
| SubstanceUsedOnce | 24 | 41 | slot | count > 0 | `once_is_any_use_at_all`, `an_unread_count_is_unknown`; `bridge.rs` `a_substance_question_asks_for_its_count_variable` |
| SubstanceUsedMore | 11 | 19 | slot | count > 3 | `more_is_more_than_three`; `bridge.rs` `a_substance_question_asks_for_its_count_variable` |

### Damage

`src/core/damage.rs`, from `DataKind::SkillDamage`. PFC `CharacterLuaFunctions` and `Modifiable`:
damaged means the skill's `damageValue` is below zero. The plugin reads `damageValue`; the
fixture sums the skill's `DAMAGE` modifiers from the save's character sheet.

| function | guards | entries | mechanism | tests |
| --- | ---: | ---: | --- | --- |
| HasVolitionDamage | 4 | 12 | data | `damage_is_a_negative_value`; `bridge.rs` `a_damage_question_reads_the_skill_and_compares_below_zero` |
| HasEnduranceDamage | 2 | 6 | data | `damage_is_a_negative_value` |

Held at the world's answer for a search - a recorded decision in `core::modelling`.

### Scene and weather

`src/core/scene.rs`, tested in `tests/scene_queries.rs` against the committed `scene-*` saves.

| function | guards | entries | mechanism | data | game source | tests |
| --- | ---: | ---: | --- | --- | --- | --- |
| IsExterior | 4 | 14 | data | `SceneIsOutside`; offline, the save's area looked up in `testing/scenes.json` | PFC `MapLuaFunctions.IsExterior` | `the_engine_asks_about_the_scene_where_the_guards_do`, `every_conversation_that_guards_on_the_scene_asks_about_it` |
| IsRaining | 2 | 2 | slot | the variable `auto.is_raining` | ISIL, and measured | `the_weather_is_its_variable`, `the_weather_saves_answer_the_weather_they_were_made_with` |
| IsSnowing | 2 | 2 | slot | the variable `auto.is_snowing` | ISIL | `the_weather_is_its_variable`, `the_engine_asks_about_the_scene_where_the_guards_do` |

`WeatherController` rewrites both weather variables from the preset on load, and the offline
reader simulates that step rather than editing the committed saves.

### Game mode

`src/core/game_mode.rs`. Both functions exist only in Final Cut.

| function | guards | entries | mechanism | data | game source | tests |
| --- | ---: | ---: | --- | --- | --- | --- |
| IsHardcoreModeActive | 2 | 6 | data | `GameMode`; offline, the save's `gameModeState.gameMode` | ISIL `GameModeController.IsHardcoreOn`, and measured over four saves | `bridge.rs` `the_hardcore_question_reads_the_game_mode` |
| WasGameBeatenInHardcoreMode | 2 | 6 | data | `HardcorePlaythroughCompleted`; offline, unread | ISIL `GameStatsManager.HardcorePlaythroughCompleted` | `bridge.rs` `the_hardcore_question_reads_the_game_mode` |

`WasGameBeatenInHardcoreMode` is profile state, not save state, so no save records it and the
offline world leaves it Unknown.

### Actions called from a guard

| function | guards | entries | mechanism | game source | tests |
| --- | ---: | ---: | --- | --- | --- |
| XPStandardSetBool | 2 | 2 | answered | PFC `TaskLuaFunctions.XPSetBool`: sets the variable and awards experience | `bridge.rs` `an_action_called_from_a_guard_is_never_asked_for` |
| FinishTask | 1 | 1 | answered | PFC `JournalModel.FinishTask`: closes the task | `bridge.rs` `an_action_called_from_a_guard_is_never_asked_for` |

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

| validator branch | engine | plugin | offline |
| --- | --- | --- | --- |
| `PassiveNode.CheckSuccess` | `Passive` | `PassiveCheckRule` | `checks_in_save`, through `core::passive_check::outcome` |
| `RedCheckNode.IsRedCheckDecided` | `Red` | `ThoughtAlterant.RedChecksFail` | `passive_thoughts_in_save` |
| `WhiteCheckNode.IsWhiteCheckPassed` | `White` | `FailedWhiteChecks.ChecksBySkill` | `failed_white_checks_in_save` |
| `FakeCheckNode.IsFakeCheckDone` | `Fake` | the seen set | `closes_once_seen` |
| `TestOptionNode.IsHidden` | `Test` | not applicable - goes nowhere | not applicable |
| `KimSwitchNode.IsAvailable` | `KimSwitch` | the seen set unless `boolean_only` | the same |
| `CostOptionNode.IsHidden` | not a kind; `ClickCost` / `CostOnce` | money | money |

The passive row shares its arithmetic as well as its shape: the offline side calls the plugin's
own comparison rather than repeating it.

## What the search can move

A question is only exact from a starting value while nothing along the search path writes what
it reads. Three positions, per function above:

- **Moved and tracked.** The search's own actions write the data and the answer is read from
  search state: the three slot-backed sets, `CheckItemGroup`'s members, reputation, flags,
  money, and every `Variable[...]` read.
- **Constant.** No dialogue action can write the data: the cabinet states, game mode, the day.
- **Held.** Dialogue actions CAN write the data, and the engine answers from the starting value
  anyway, by a decision recorded in `core::modelling::DECISIONS` or a note in the module:
  party, damage, the clock (locked), the pawnables tab, the scene.

Which actions write what, and whether any of them reaches a guard that reads it, is the subject
of the action audit (de-70eo), which will record its findings in `docs/actions.md`. Findings
from reading the game's action bodies already put two functions in the wrong position above,
and they will move when that audit settles them: `GainItem` equips an item flagged
`autoEquip` and `LoseItem` unequips (`Inventory.HandlePickedUpItem`, `Inventory.DeleteItem`),
so equipment can be written by dialogue; and `UseSubstanceInHand` increments
`stats.uses_<group>` (`HudHeldPanelController.OnSubstanceUse`), which `core::substance`
describes as having no dialogue writer.

## Adding a guard function

A function answered from search state is honoured in FIVE places, and they have to agree. A
function honoured in some of them is worse than one honoured in none: it gets declared and
slotted and then read from a snapshot that was never told about it, which is the starting value
forever.

1. `src/graph/mod.rs` declares the subject as one of the group's variables, which makes it
   readable at all.
2. `src/symbolic/data_layout.rs` spends a slot on it.
3. `src/bridge.rs` `collect` keeps it out of what the plugin is asked, or asks for its data.
4. `src/world/mod.rs` `BoundContext::query` (or `SnapshotWorld::query`) answers it.
5. `src/symbolic/guard_formula.rs` - both the compile arm and `search_can_change` - decides it,
   because the compiler runs on its own path, and a compiler less decisive than the engine
   leaves branches open that the search closes.

Name the query once, as a constant or a small predicate in its `core` module, and use that name
at every site - that is what makes the five findable with one search. Quote the game's
definition beside the emulation and name where it came from. Then:

- give the data a `DataKind` if it is not a variable, serviced by a READ in
  `GameWorldSnapshot.Serviced` and by the save in `Holdings::data_for`;
- run `tools/survey-guard-functions.py` and update the tables here.
