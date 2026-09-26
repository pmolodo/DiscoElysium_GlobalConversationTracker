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
| Until the clock hook has fired for the first time, a crawl is told the clock is locked                                                                                                 | restrictive | plugin `ClockLockPatch`                                                     | a crawl in the seconds before the game's first clock tick or save load                |           |
| The game's own clock is understood to advance about a minute per unseen line, which nothing models                                                                                     | either | not modelled                                                                      | every conversation; unmeasured                                                        |           |
| A thought part-way through internalising never finishes during a conversation, so `IsTHCFixed`, `IsTHCCooking` and `IsTHCCookingOrFixed` hold their starting answer however late it gets - and the passive checks a newly fixed thought would move keep the world's answer. DECIDED, not pending: see below | either | `core::thought_effects`; cabinet states in `world::GameWorld`                | the same 72 live `PassTime` sites; cabinet questions: 110 entries                     |           |
| Passing time wears a running substance off, stripping the buffs it was giving - which move an ATTRIBUTE, so six skills at once - and that can flip a passive check either way. DEFERRED: see below | either | not modelled                                                                      | 3,566 passive checks sit in a group that can pass time, but only while something is running - no committed save has one | de-m11s.3.3 |
| The clock is read to the hour, not the minute                                                                                                                                          | either | plugin `GameFacts.ReadClock`                                                      | not reached: no guard asks about minutes                                              |           |
| `AssignClock` writes the reading at the WORLD's hour, so a deadline stored after a `PassTime` is stored a quarter-hour early                                                           | either | `symbolic::action_image::clock_write`                                             | not reached: of 1,315 groups, 35 pass time and 2 write a reading, and no group does both (2026-09-21) |           |
| A clock reading compared against a variable is undecided where the clock moves, since nothing here compares two registers                                                              | permissive | `GuardCompiler::variable_against_query`                                       | not reached: no group that passes time holds a guard naming `HourCount` or `TotalHourCount` at all (2026-09-21) |           |

### A thought never finishes internalising mid-conversation, and that is a decision

**Not a gap waiting to be filled.** A thought part-way through the cabinet finishes after so
many hours, and passing time in a conversation could in principle cross that line - but
modelling it means every passive check a newly fixed thought moves becomes unsettled, and the
thought's effects are what `core::thought_effects` reads to decide those checks in the first
place. That is a large cost in markers and in diagram work for a circumstance that has to line
up three ways at once: a thought part-way through, a conversation that passes enough time to
finish it, and a check close enough to its threshold for the thought's effect to flip it.

It is treated the way the game's own per-entry minute is treated - ignored deliberately, and
written down here so the next person meets the decision rather than the surprise. The
[Time](#time) row above is the entry.

### A substance wearing off is deferred rather than decided against

Same shape, stronger case, so it keeps its issue. Two things make it likelier to matter than a
thought finishing:

- **A substance lasts sixty minutes** and a crawl passes fifteen at a time, so four calls end a
  fresh one and fewer end one taken a while ago. Internalising a thought takes hours, so the
  window a crawl can cross is much smaller here.
- **A substance buff moves an ATTRIBUTE.** `CharacterEffect` carries an `abilityType` as well
  as a `skillType` and applies through `GetAbility(...).Add(modifier)`, so one wearing off
  moves every skill under that attribute - six margins to cross instead of one.

Still small in absolute terms: it needs a player who is on something, in one of the
time-passing groups holding a passive check - the row above counts them - with a check inside
the buff's swing. What it would take to build is written out on de-m11s.3.3, including the one
thing missing: a committed save with a substance actually running, and none of them has one.

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
| A white check failed DURING the search is reopened by a modifier the group can move, without asking what the modifier was worth when it failed. One the save had already failed is held to its remembered target exactly | permissive            | `graph::node::Reopening`; `GuardCompiler::reopening_for`                     | a check the crawl fails and then comes back to; see below                                                          |         |
| A fake check's `FlagName` and `FlagName_failed` are not read, though the game hides the option once either is set                                                                                                       | permissive            | fake check handling in the graph builder                                     | three fake checks; nothing writes their `_failed` flags                                                            |         |
| `RemoveWhiteCheck` is not applied, so a check it retires mid-conversation stays offered                                                                                                                                 | permissive            | `core::modelling`                                                            | not reached: all four call sites pass a variable's value rather than a check's flag name                           |         |
| A rolled check's odds (hardcore difficulty, situational modifiers, crit range) are not read. Both outcomes are taken whatever they are                                                                                  | none for reachability | by design                                                                    | every rolled check. Exact for "can this be reached"; says nothing about how likely it is                           |         |

### What reopens a failed white check, and what is left of the gap

**The skill-rank half is dead inside one conversation** - nothing in a dialogue levels a
skill - so what reopens a check during a crawl is the modifier half:
`difficulty + the bonuses of whichever variable1..variable10 expressions are true` falling
below what it was when the check failed.

**Every one of the game's 126 white checks carries at least one such expression**, and what
they read is items and equipment (`CheckItem`, `CheckEquipped`, 83 of them), thoughts
(`IsTHCPresent` and the fixed forms, 38), Kim, the clock, and named variables. All of those
are things a dialogue action writes.

**A check the save has already failed is asked exactly.** The game remembers each failed
check's `difficulty` and the target it failed against in `FailedWhiteChecks.WhiteCheckCache`;
the plugin sends both, and the engine offers the check again where `difficulty` plus the
bonuses of every modifier that holds falls below that target - modifiers of either sign, the
ones the group cannot move counting as the world's constants.

**What is left is a check the crawl fails itself.** The search state does not carry the target
a check was failed at, so such a check is offered again where a modifier worth a negative bonus
holds that this group can MOVE - one nothing here writes held at the failure if it holds now,
and lowered nothing. A movable modifier that ALREADY held at that failure reopens it too, which
the game would not: that residue errs permissive, the direction this engine is built to be
wrong in. Closing it needs the target at the failure carried in the state, a register per
check.

The game also reopens a check the moment its target falls and keeps it open, where the engine
asks at the moment the check is offered, so a modifier that holds and then stops holding
before the crawl returns is missed. And whether the check's own precondition still holds is
the check's guard, which the search asks anyway.

## Actions not applied, or applied in part

| gap                                                                                                                           | errs       | where                            | in content                                        | tracked |
| ----------------------------------------------------------------------------------------------------------------------------- | ---------- | -------------------------------- | ------------------------------------------------- | ------- |
| `SetVariableValue` with a value of `not(Variable[...])`, or one other computed shape, is not applied                          | either     | action parser; reported as a gap | not reached: no call site has a downstream reader |         |
| `TequilaPutOnBodysuit` and `TequilaRemoveBodysuit` are Final Cut coroutines whose bodies were not recovered                   | unknown    | `core::modelling`                | one call site each                                |         |
| A purchase whose walk to its `GainItem` passes a guard keeps its unscaled `ClickCost` in hardcore mode                        | permissive | `core::price`, `index::price`    | one: conversation 28's room at 20 real            |         |
| Experience (`XP*SetBool`'s experience, `COMMUNIST_XP_AMOUNT`) is not applied                                                  | none       | action parser                    | not reached: no guard reads experience            |         |

## The symbolic compiler's undecided answers

Where the guard compiler cannot build a formula, it leaves the guard undecided: both outcomes
are open, which errs permissive. `GuardCompiler::fallback_reasons` counts them per group, and
`tests/modelling_gaps.rs` names them for the group it is about. The reasons are:

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
| Trimming runs one round: a slot whose writers were all trimmed away is still tracked rather than read as a constant. Measured over the scenario menus, a second round would remove 17 entries in one menu of 32            | less tight, never wrong                                      | `symbolic::trim`                                         |                  |
| A search that runs out of time or diagram memory draws the uncertain marker rather than an answer                                                                                                                          | no answer, reported as such                                  | `bridge::LookAheadRequest::search_budget`, `menu_budget` |                  |

## Keeping this current

A row belongs here when an answer can differ from the game's, whatever the reason. When a
gap is closed, delete its row and say how it was closed in the commit. When one is found, add
it with its direction and its reach in content, and link the issue that tracks it.
