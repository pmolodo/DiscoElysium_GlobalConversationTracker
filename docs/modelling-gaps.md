# Modelling gaps

Every place the look-ahead's answer about a guard, an action or a check can differ from the
game's, or is left unknown. [guards.md](guards.md) and [actions.md](actions.md) describe each
function in full; this page collects the ones that are not exact, so they can be read, and
worked down, in one place.

## How to read a row

**Errs** says which way the answer can be wrong:

- **permissive** - a route the game closes can be counted as open. At worst an option is
  starred that leads nowhere new. The engine is built to be wrong in this direction when it
  has to be; see [Unknown is permissive](guards.md#unknown-is-permissive).
- **restrictive** - a route the game opens can be counted as closed. At worst a star is
  missing.
- **either** - a value is held or clipped, so the answer can be wrong both ways.

**In content** says whether the shipped dialogue can actually reach the gap. "Not reached"
means the gap exists by construction, but a survey found no call site whose write a downstream
reader reads (the rule in [actions.md](actions.md#the-rule-an-action-is-judged-by)). Such a
row stays exact only while the content stays as it is.

## Time

| gap                                                                                                                                                                                    | errs   | where                                                                             | in content                                                                            | tracked   |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | --------- |
| The clock is sent locked, so `PassTime` does not move it during a search, and every hour question is answered at the world's hour                                                      | either | plugin `GameFacts.ReadClock`; `core::clock`; `GuardCompiler::with_constant_clock` | `PassTime`: 207 calls, 72 with a downstream reader. Hour questions: about 400 entries |           |
| The game's own clock is understood to advance about a minute per unseen line, which nothing models                                                                                     | either | not modelled                                                                      | every conversation; unmeasured                                                        | de-sze.10 |
| Passing time bakes cooking thoughts into fixed ones and wears substances off; with the clock locked, `IsTHCFixed`, `IsTHCCooking` and `IsTHCCookingOrFixed` hold their starting answer | either | `core::thought_effects`; cabinet states in `bridge::SnapshotWorld`                | the same 72 live `PassTime` sites; cabinet questions: 110 entries                     |           |
| The clock is read to the hour, not the minute                                                                                                                                          | either | plugin `GameFacts.ReadClock`                                                      | not reached: no guard asks about minutes                                              |           |

## Counters and amounts

| gap                                                                                                                                                                                                     | errs       | where                                                          | in content                                                                                                                                  | tracked |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| A counter a dialogue loop can raise without end saturates at the counter cap, 16, which keeps the search finite. A counter that cannot loop is not capped (`LookAheadGraph::counters_that_cannot_loop`) | either     | `bridge::COUNTER_CAP`; `CounterCaps`; `ActionImage` saturation | a counter on a loop that a guard compares above 16, or that is lowered or assigned as well as raised - a reputation something lowers is one |         |
| A loop counter held as its value clamps the value the save arrives with into its slot's width                                                                                                           | either     | `seed_of`                                                      | a save value above 31 on such a counter                                                                                                     |         |
| A slot's starting value below zero is read as zero by the symbolic encoding; the explicit engine keeps it until an increment floors it                                                                  | either     | `seed_of`; `GuardCompiler::rebase_start`                       | not surveyed                                                                                                                                |         |
| A reputation question whose argument is not a literal name, or whose amounts the world cannot give, is left undecided by the symbolic compiler                                                          | permissive | `GuardCompiler::highest_reputation`, `amounts_of`              | not reached: every reputation question names its reputation                                                                                 |         |

## Party, place and inventory held at the save's answer

Decisions recorded in `core::modelling::DECISIONS`: the engine answers these from the starting
world although a dialogue action can change them.

| gap                                                                                                        | errs   | where                                                          | in content                                                                                                        | tracked |
| ---------------------------------------------------------------------------------------------------------- | ------ | -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------- |
| `IsKimHere`, `IsKimInParty`, `IsCunoInParty` held, except where a group leaves Kim at the church           | either | `core::modelling`, `core::party`                               | not reached: of the party writers, only `RemoveKitsuragiWaitAtChurch` has a downstream reader, and it is modelled |         |
| `IsExterior` held while movement actions change where the player stands                                    | either | `core::modelling`                                              | not reached: no conversation both moves the player and asks                                                       |         |
| `HasPawnablesInInventory`, and the money and items pawning moves, held at the starting inventory           | either | `core::modelling` (`SellItemGroup`, `ShowInventoryForPawning`) | not reached: three scripts, no downstream reader                                                                  |         |
| `SubstanceUsedOnce`, `SubstanceUsedMore` read the starting count while `UseSubstanceInHand` is not applied | either | `core::modelling`                                              | not reached: no conversation both uses a substance and asks                                                       |         |
| An `autoequip` item gained is not put on for equipment questions                                           | either | `core::equipment`                                              | not reached: no autoequip gain has a downstream equipment reader                                                  |         |

## Checks

| gap                                                                                                                                                                                                                     | errs                  | where                                                                        | in content                                                                                                         | tracked |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ------- |
| A passive check is decided once per request, by the plugin. Where the group can change what is worn, or its damage or healing can cross the check's margin, the check is Unknown - item bonuses are not in the database | permissive            | plugin `PassiveCheckRule`; `core::skill_movers`; `Fitting::damage_unsettled` | groups that lose a worn item, gain an autoequip item, or damage or heal Volition or Endurance near a passive check |         |
| A failed white check stays closed. The game reopens one when its skill rank rises or a modifier expression lowers the target                                                                                            | restrictive           | `FlagName_failed` slot, never cleared                                        | any failed white check with modifiers or a raisable skill                                                          | de-vdy9 |
| A fake check's `FlagName` and `FlagName_failed` are not read, though the game hides the option once either is set                                                                                                       | permissive            | fake check handling in the graph builder                                     | three fake checks; nothing writes their `_failed` flags                                                            |         |
| `RemoveWhiteCheck` is not applied, so a check it retires mid-conversation stays offered                                                                                                                                 | permissive            | `core::modelling`                                                            | not reached: all four call sites pass a variable's value rather than a check's flag name                           |         |
| A rolled check's odds (hardcore difficulty, situational modifiers, crit range) are not read. Both outcomes are taken whatever they are                                                                                  | none for reachability | by design                                                                    | every rolled check. Exact for "can this be reached"; says nothing about how likely it is                           |         |

## Actions not applied, or applied in part

| gap                                                                                                                           | errs       | where                            | in content                                        | tracked |
| ----------------------------------------------------------------------------------------------------------------------------- | ---------- | -------------------------------- | ------------------------------------------------- | ------- |
| `SetVariableValue` with a value of `not(Variable[...])`, or one other computed shape, is not applied                          | either     | action parser; reported as a gap | not reached: no call site has a downstream reader |         |
| `LetterSleep` and `SkipToDebriefLocation` write `auto.daychange_*` through `EnddayManager`, and which variables is not traced | unknown    | `core::modelling`                | two call sites                                    |         |
| `TequilaPutOnBodysuit` and `TequilaRemoveBodysuit` are Final Cut coroutines whose bodies were not recovered                   | unknown    | `core::modelling`                | one call site each                                |         |
| A purchase whose walk to its `GainItem` passes a guard keeps its unscaled `ClickCost` in hardcore mode                        | permissive | `core::price`, `index::price`    | one: conversation 28's room at 20 real            |         |
| Experience (`XP*SetBool`'s experience, `COMMUNIST_XP_AMOUNT`) is not applied                                                  | none       | action parser                    | not reached: no guard reads experience            |         |

## The symbolic compiler's undecided answers

Where the guard compiler cannot build a formula, it leaves the guard undecided: both outcomes
are open, which errs permissive. `GuardCompiler::fallback_reasons` counts them per group, and
`tests/modelling_gaps.rs` names them for one group (`DEGCT_CONVERSATION`). The reasons are:

| reason                                                                        | when                                                                                            |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `call: world cannot say`, `... world cannot say`                              | the plugin could not read what the question needs (clock, damage, equipment, party, a variable) |
| `call: subject is not a literal`                                              | a set question whose subject is computed                                                        |
| `call: item group not answerable`, `... members unknown`                      | an item group the database does not describe                                                    |
| `call: flag has no slot`                                                      | `FlagSet` with a computed name                                                                  |
| `call: clock, not a question of the hour`                                     | a clock question outside `ClockTime`                                                            |
| `comparison: ordering on a non-numeric query`, `comparison: unknown operator` | a comparison the guard language does not define                                                 |
| `literal is unknown`                                                          | a literal that reads as neither true nor false                                                  |
| `reputation: ...`                                                             | see [Counters and amounts](#counters-and-amounts)                                               |
| `no room to build the formula`                                                | the diagram manager is full. The answer stays sound, and coarser than the content warrants      |

## Approximations in how a menu is marked

These do not misread a guard or an action. They are choices about which options get a star,
and they can differ from the exact marking.

| approximation                                                                                                                                                                                                              | effect                                                       | where                                                    | tracked          |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ | -------------------------------------------------------- | ---------------- |
| The onward question stars an option that reaches unread content without returning through the menu or the hubs just passed. It is not a superset of the exact marking: over the whole game 18 menus mark more and 12 fewer | marks can differ from the exact marking, by design           | `menu::mark_menu_hybrid` step 1                          |                  |
| The hub stack decides "can the player get back" over the link graph. On a trimmed menu (`bridge::walkable_menu`) guards that hold in no state are respected; others are ignored                                            | a way back that a variable closes still counts as a way back | `symbolic::hub`, `symbolic::trim`                        |                  |
| Trimming runs one round: a slot whose writers were all trimmed away is still tracked rather than read as a constant. Measured over the scenario menus, a second round would remove 17 entries in one menu of 32            | less tight, never wrong                                      | `symbolic::trim`                                         | de-fw84 (closed) |
| A search that runs out of time or diagram memory draws the uncertain marker rather than an answer                                                                                                                          | no answer, reported as such                                  | `bridge::LookAheadRequest::search_budget`, `menu_budget` |                  |

## Keeping this current

A row belongs here when an answer can differ from the game's, whatever the reason. When a
gap is closed, delete its row and say how it was closed in the commit. When one is found, add
it with its direction and its reach in content, and link the issue that tracks it.
