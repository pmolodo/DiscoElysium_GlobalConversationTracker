// SPDX-License-Identifier: MIT
//
// The look-ahead budgets a player gets without opening the config file. THE ONE PLACE THEY ARE
// WRITTEN DOWN: the engine reads them from here, and `build.rs` includes this file and writes
// them out as C# constants for the plugin's `Config.Bind` defaults and for the harness - see
// `generate_shipped_budgets` there. A number typed anywhere else is a copy that can drift.
//
// NO INNER DOC COMMENTS in this file, because `build.rs` includes it inside a module and an
// included file may not carry inner attributes. The module's own documentation is on its `mod`
// line in `lib.rs`.

/// `LookAheadTimeBudgetMs`: the longest one option's look-ahead may run for, in milliseconds.
///
/// A BACKSTOP RATHER THAN THE LIMIT THAT NORMALLY DECIDES. How much a search gets through in a
/// second depends on the machine, so this cannot promise a reproducible give-up; the memory
/// budget is what usually stops a crawl. The worst crawl measured over the largest
/// conversations in the game took about three quarters of a second.
pub const TIME_BUDGET_MS: u64 = 1000;

/// `LookAheadMenuTimeBudgetMs`: the longest a whole response menu's look-ahead may run for, in
/// milliseconds.
///
/// THE WALL AROUND THE WHOLE MENU, which the per-option budget cannot be: a menu is the sum of
/// its options and a rolled check counts twice, so twelve options at [`TIME_BUDGET_MS`] arrive
/// at a worst case near twenty-four seconds - under a host read deadline of thirty whose answer
/// to being crossed is to kill the engine. de-dt75.3.
///
/// THREE SECONDS BECAUSE THE MEASUREMENT SAYS SO. `crates/gct-measure/examples/menu_wall.rs`
/// asks the six heaviest groups a deliberately adversarial menu - twenty-four starts, every one
/// with unread text beyond it, and a cold engine - and the worst, conversation 368, came back in
/// 2.05 seconds. So this sits above the worst menu anyone has measured and an order of
/// magnitude below the deadline that kills, which is the gap it exists to hold open.
pub const MENU_TIME_BUDGET_MS: u64 = 3000;

/// `LookAheadMemoryBudgetMb`: the most memory one option's search may hold, in megabytes.
///
/// STATED IN MEGABYTES because that is the unit a player can reason about, and the unit it is
/// spent in; the diagram manager buys nodes with it - see `DiagramBudget`.
///
/// ## Why 300 and not 256
///
/// 256 was never measured as a threshold, only as a round number that was enough for
/// everything the engine then did. Conversation 761 cannot be marked EXACTLY below about
/// 288 MB at any clock, and answers above it (de-0jsf.16). A ceiling eleven per cent under the
/// one group that needs it is an arbitrary number doing harm, so this clears it with headroom.
///
/// THE SHIPPED MARKING DOES NOT NEED IT, which is what makes it cheap. Measured 2026-09-17 at
/// 256 MB, `menu::mark_menu_hybrid` answers every one of the 389 menus a walked profile puts
/// up with no option left unsettled, and under sixty thousand diagram nodes for the worst of
/// them - conversation 16, at 56,644. The headroom is for the exact search a fallback reaches
/// for, and for the groups nobody has profiled.
///
/// ONE MENU IN THE GAME DOES NOT SETTLE, AND MORE MEMORY IS NOT WHAT IT WANTS. Asked on the
/// adversarial profile `MenuProfile::of` builds, 761 fails to settle at 256 and at 300 alike -
/// 2,534 ms against 2,561, four and a half million nodes either way. That profile asks for a
/// state no save holds; the same menu in a state a playthrough walked to costs 118 ms at 256 MB
/// and settles every option. See de-zbsb.
///
/// ## What 256 bought, which still holds
///
/// NOTHING IN THE GAME RUNS OUT. A whole-game matrix, 2,334 measured rows across both engine
/// arms, records no manager that filled (de-dt75.2).
///
/// AND A SESSION DOES NOT ACCUMULATE INTO IT. `crates/gct-measure/examples/workspace_menus.rs`
/// walks forty menus through the shipped call: conversation 761, the heaviest group in the
/// game, leaves the store holding 17.9% of it, and the next highest of the forty heaviest is
/// 1.5%. The store is filled by the FIRST request and the other thirty-nine add two tenths of a
/// per cent, so what a group holds is a constant of the group rather than a curve heading for
/// the ceiling.
///
/// So there is over six times the headroom on the worst group, and every megabyte here is
/// memory a player gives up whether their conversations need it or not - the node store is one
/// preallocation. That is the argument against raising it further.
///
/// A CHANGE HERE WANTS A MATRIX RUN EITHER SIDE OF IT, since a regression traced to a budget
/// that moved in the same commit is not traced at all.
pub const MEMORY_BUDGET_MB: usize = 300;
