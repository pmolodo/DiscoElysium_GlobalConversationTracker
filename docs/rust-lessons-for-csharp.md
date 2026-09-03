# Improvements the Rust port has that the C# engine does not

A running list, kept while porting the look-ahead engine to Rust (branch `rust_imp`,
epic de-sze). Everything here is a change that would improve
`src/GlobalConversationTracker.LookAhead`, found by writing the same thing twice and
noticing where the second attempt came out better.

**Nothing here has been applied to the C# side, deliberately.** The C# engine is the
working implementation the shipped plugin depends on and the oracle the Rust port is
checked against; changing both at once would leave nothing to check against. This file
is the record so the ideas are not lost, not a to-do list that has been silently
half-done.

Each entry says what C# does now, what Rust does instead, why it is better, and how
much it is likely to be worth. Where an entry has not been measured, it says so.

---

## 1. Once slots are interned during the crawl, for every node, whether or not it needs one

**C# today.** `LookAheadEngine.Charge` ends with:

```csharp
return DialogueAction.Apply(
    node.Actions, paid, symbols.Once(node.Id), _options.CounterCap,
    clockLocked, _options.CounterCapForSlot);
```

`StateSymbols.Once` interns - it creates the slot if it does not exist - and this call
is unconditional. Every node the crawl ever charges therefore gets a once slot, and the
symbol table grows *during* the search. Two more calls at lines 711 and 742 do the same
under `node.CostOnce`, which is the case that genuinely needs one.

**Rust instead.** `LookAheadNode` carries `once_slot: i32`, assigned once by
`LookAheadGraph::new`, and only where the node actually has a one-time effect:

```rust
pub fn needs_once_slot(&self) -> bool {
    self.cost_once || self.actions.iter().any(|action| action.is_once())
}
```

`-1` means "no slot", handled the way `flag_slot` and `seen_slot` already handle it.
The crawl reads slots and never creates one.

**Why it is better.**

- *A narrower state vector.* Every slot is a value in a vector the search copies and
  hashes millions of times. Conversation 631's group is 4,514 nodes and its graph build
  interns 339 slots; if the crawl charges most of those nodes, C# ends up carrying an
  order of magnitude more slots than it needs. Not yet measured directly - the number
  to take is how many nodes actually satisfy `needs_once_slot`.
- *A symbol table that stops growing.* In C# the table is still being extended while
  states are being created, so states made early and late have different widths.
  `LookAheadState.Equals` already copes with that by checking the longer tail is all
  zero, which is a correctness burden that exists only because the table grows.
- *It is a precondition for anything symbolic.* A decision diagram fixes its variable
  order up front and cannot if a new variable can appear halfway through a search. This
  is the reason the Rust change was made at all; the narrower vector was a bonus.

**Cost of adopting.** Small and local: a field on `LookAheadNode`, an assignment loop in
the graph builder, and three call sites in `LookAheadEngine`. `DialogueAction.Apply`
would take the slot as `int` with `-1` for none, matching its neighbours.

---

## 2. `CrawlContext` stores the state it is answering about

**C# today.** `CrawlContext` holds the current `LookAheadState` and is re-bound as the
crawl moves. That is fine in C#, where nothing checks how long a reference lives.

**Rust could not do this**, and the reason is worth reading even if C# never changes.
The stored reference necessarily shares a lifetime with the symbol table and the world,
which are long-lived, so binding one of the crawl's own short-lived states asks that
state to outlive the entire search. The port instead keeps `CrawlContext` stateless -
symbols and world only - and has it hand out a short-lived `BoundContext` for the length
of a single guard evaluation.

**Why it is better even without a borrow checker.** The lifetime error is pointing at a
real design fact: the thing being evaluated has a much shorter life than the thing doing
the evaluating, and combining them into one mutable object means every reader has to
know which state is currently bound. The split makes "which state is this guard being
asked about" an argument rather than a mode, and makes `CrawlContext` shareable rather
than something that must be mutated between uses.

**Cost of adopting.** Moderate: it changes the shape of guard evaluation, which several
call sites go through. Worth doing only alongside other work in that area.

---

## 3. `CounterCapForSlot` supplants the flat cap instead of falling back to it

**C# today.** `LookAheadOptions` has two knobs, `CounterCap` (flat, default 16) and
`CounterCapForSlot` (`Func<int,int>?`). Once the second is supplied it answers for
*every* slot:

```csharp
int cap = counterCapForSlot == null
    ? counterCap : counterCapForSlot(action.Slot);
```

So a caller wanting to special-case one variable must answer for all of them, and
re-state the default itself. `tools/LookAheadOffline/Program.cs` does exactly that, and
duplicates the literal:

```csharp
CounterCapForSlot = slot => state.CounterCaps.TryGetValue(
    graph.Symbols.NameOf(slot), out int cap) ? cap : 16,
```

That `16` is the default from `LookAheadOptions.CounterCap`, written out a second time in
a different file. Change the default and this silently disagrees with it.

**Rust instead.** One type, `CounterCaps`, holding a default and an optional per-slot
function that returns `Option<i32>` - `None` meaning "no opinion, use the default":

```rust
pub fn for_slot(&self, slot: usize) -> i32 {
    self.per_slot.and_then(|f| f(slot)).unwrap_or(self.default)
}
```

The offline caller then says only what it actually knows, and the default lives in one
place.

**Why it is better.** It removes a duplicated constant that nothing keeps in step, and it
makes the common intent - "the usual cap, except for these two variables" - expressible
without writing a total function. It also makes the two knobs one concept rather than two
that interact.

**Cost of adopting.** Small: change the signature to `Func<int,int?>`, use `?? counterCap`
at the single use site, and drop the `: 16` from the offline crawler.

---

## 4. The conversation group is walked in hash-set order, so slot numbering is not reproducible

**C# today.** `ConversationIndex.BuildGraph` collects the group into a `HashSet<int>` and
then builds nodes by iterating it:

```csharp
foreach (int id in group)
{
    foreach (EntryRecord entry in _conversations[id].Entries) { nodes.Add(ToNode(...)); }
}
```

`HashSet<int>` enumeration order is not part of its contract. The order conversations are
visited decides the order their entries intern symbols, which decides every slot number.

**Rust instead.** `discover_group` returns a sorted `Vec<i32>`, and the graph builder walks
that.

**Why it is better.** Slot numbers show up in traces, in the diagnostics writer's overflow
reports, and in anything that compares one run against another. Today two runs over the
same database can label the same variable differently, which makes a diff between two
diagnostic dumps unreadable for reasons that have nothing to do with what changed. Sorting
costs one `Sort` on a list of a handful of integers.

For the Rust side this is not cosmetic at all: a slot number is a decision-diagram
variable number, and a variable order that changes between runs would make any symbolic
measurement unrepeatable. That is why it was noticed.

**Cost of adopting.** Trivial: sort the group before building. The C# plugin's own
`LookAheadGraphBuilder.DiscoverGroup` deserves the same look.

---

## 5. `ConversationStatistics` re-implements a subset of `LookAheadStatistics`

**C# today.** Two types accumulate the same quantities by the same arithmetic.
`LookAheadStatistics.Record` adds to its own crawls, total states, max states, total
milliseconds, max milliseconds and exhaustion counts, then does it again into a
`ConversationStatistics` row a few lines below.

The per-conversation row is the poorer copy: it has no `TotalNodes`, no `MaxNodes` and no
`MinStates`. Not by decision - it was written as "the fields a per-conversation table
needed", and the table has since grown.

**Rust instead.** One `Tally`, used for the whole run and for each conversation:

```rust
pub struct LookAheadStatistics {
    pub overall: Tally,
    pub by_conversation: HashMap<i32, Tally>,
    // buckets and per-novelty counts, which only make sense run-wide
    ...
}
```

**Why it is better.** One place the accumulation can be wrong instead of two that must
agree, and every conversation gets every figure for free. Adding a quantity later means
adding it once.

**Cost of adopting.** Small, and mostly deletion: promote `ConversationStatistics` to
carry the full set, have `LookAheadStatistics` hold one for the run, and call `Record` on
each.

---

## 6. `MinStates` starts at `int.MaxValue` and stays there when nothing was recorded

**C# today.** `public int MinStates { get; private set; } = int.MaxValue;`

A report over a run with no crawls - which is the CORRECT outcome for an all-seen global
state, and the thing the all-seen suite asserts - prints `2147483647` as its minimum.

**Rust instead.** `min_states: Option<usize>`, `None` until something is recorded.

**Why it is better.** An absent minimum is not a very large one, and the sentinel leaks
into anything that prints or serialises the figure. The type says which it is.

**Cost of adopting.** `int?`, and one null check wherever it is reported.

---

## 7. `BudgetExhausted` and `TimeExhausted` overlap

**C# today.**

```csharp
if (result.BudgetExhausted)
{
    BudgetExhausted++;
    if (result.StoppedBy == LookAheadLimit.Time) { TimeExhausted++; }
}
```

`LookAheadResult.BudgetExhausted` is `StoppedBy != None`, so it is true for the time limit
too. `BudgetExhausted` therefore counts crawls stopped by EITHER limit, and `TimeExhausted`
is a subset of it. The count of crawls stopped by the STATE budget - the one the name
suggests - is `BudgetExhausted - TimeExhausted`, a subtraction the reader has to know to
make. Any report adding the two double-counts every timed-out crawl.

**Rust instead.** `stopped_by_states` and `stopped_by_time`, disjoint, with
`stopped_early()` for the sum.

**Why it is better.** The two fields partition the stopped crawls, so they can be added,
compared or reported independently without knowing how they were built. It also removes a
name that means something other than what it says.

**Cost of adopting.** Rename and split the increment; fix any reporter that adds them.

---

## 8. `HasItem` and `IsTaskActive` are named for a question they do not answer

**C# today.** `ILookAheadWorld` declares:

```csharp
bool HasItem(string name);
bool IsTaskActive(string name);
```

Both are used in exactly one place - `Seed`, to initialise the `item:` and `task:` slots
from the world. Neither is how a guard gets answered. A `CheckItem` guard is answered
from crawl STATE, because `GainItem` and `LoseItem` move the slot, and only where no slot
exists does `CrawlContext.Query` fall through to `world.Query("CheckItem", ...)`.

**Why the names are a trap.** They read as the general question - "does the player have
this item" - so they are the obvious thing to reach for when answering a guard. And they
return a plain `bool`, so they are DEFINITE for every name, including one the world has
never heard of, where `Query` would answer unknown. Substituting one for the other decides
"not held" where the engine stays permissive, which prunes a branch the real crawl walks
and can lose a marker.

This is not hypothetical: writing the Rust guard compiler, exactly that substitution was
made, and it was caught only because the user asked how untracked items were handled.

**Rust instead.** `initially_has_item` and `initially_task_active`, with doc comments
saying they are for the seed and are not the answer to a guard.

**Cost of adopting.** A rename and two call sites.

---

## 9. An untracked item or task guard throws away an answer the world already has

**C# today.** When a guard asks `CheckItem("x")` and no action in the conversation group
gains or loses `x`, there is no `item:` slot, and `CrawlContext.Query` falls through to
`world.Query("CheckItem", ...)`. Most worlds answer unknown to that, so the guard is
undecided and the crawl keeps a branch it did not need to.

But the world already knows. `HasItem("x")` is exactly that question and answers
definitely. For an untracked item the information is sitting there and is being discarded.

**Rust instead.** `BoundContext::query` answers an untracked `CheckItem` from
`initially_has_item`, and an untracked `IsTaskActive` from `initially_task_active`.

**Why it is correct, not merely convenient.** For a subject the group does not track, no
action can change it, so its starting value is its ONLY value. Being decisive with a
correct answer is safe; only being decisive with a wrong one is not. The engine was
permissive here out of ignorance rather than principle.

The restriction that makes it correct is the one to hold on to: this must never answer a
guard about a TRACKED item. Once `GainItem` has run, the truth is in the crawl's state and
the starting inventory is stale - a crawl reading it would stop seeing its own purchases.
Both cases now sit behind one deliberately-named pair of methods (see entry 8) with the
distinction spelled out in their doc comments.

**What it was worth.** On conversation 631, 36 of the 74 remaining guard-compiler
fallbacks were exactly this case; removing them took that group from 93.8% to 96.8% of
guard sub-expressions compiling to a real formula. Across the five biggest conversations
`CheckItem` is 140 and `IsTaskActive` 95 of the world queries guards make.

The gain is not confined to the analysis. A permissive guard means the CRAWL keeps a
branch it need not, so this makes real crawls cheaper and their markers more accurate too -
which is the part that matters for the shipped plugin.

**Cost of adopting.** Small: two arms of `CrawlContext.Query`, restructured so the item
name is read once and the slot lookup is tried before falling back to the world.

---

## 10. Dialogue flags are not modelled at all, and they are variables

**A BUG, not a refinement** - the only entry here that costs correct answers rather than
precision.

**C# today.** `ActionParser` has no `SetFlag` case, so it becomes
`DialogueActionKind.Unmodelled` and writes nothing. `CrawlContext.Query` has no `FlagSet`
case, so it falls through to the world and answers unknown.

**What a flag actually is.** The Final Cut adds three functions in
`Sunshine.Dialogue.FELDLuaFunctions`, and the declarations settle it:

```csharp
public static void SetFlag(string variableName)
public static void UnsetFlag(string variableName)
public static bool FlagSet(string variableName)
```

The parameter is called `variableName`. A flag is a dialogue variable, written and read
through different names.

The database bears it out: **62 scripts call `SetFlag`, 9 guards call `FlagSet`, and 56
guard lines read the very names `SetFlag` writes as `Variable[...]`.** Note the reader is
`FlagSet`, not `GetFlag` - searching for the obvious name finds nothing and gives the
false impression that flags are write-only.

**Why it costs answers.** A path that opens only after a flag is set stays closed for the
crawl. The crawl then misses reachable states, reports a lower novelty than the truth, and
the option loses its marker - the failure mode that shows the player nothing rather than
showing something wrong, which is the harder one to notice.

**Rust instead.** `SetFlag` assigns 1 to the variable's slot, `UnsetFlag` assigns 0, and
`FlagSet` reads that slot exactly as `Variable[name]` does. Over the database that moves 63
actions from unmodelled to modelled and interns the 61 distinct flag names.

**Cost of adopting.** One case in `ActionParser` beside `SetVariableValue`, one in
`CrawlContext.Query` beside `CheckItem`. Test it through the ENGINE rather than the parser
- a node sets a flag, a later node is gated on it, the crawl must reach the gated node -
with a companion test that the gate stays shut when nothing sets it, or the first passes
just as well against a guard that is never evaluated.

**Worth checking while you are there.** The other unmodelled action names are listed in
de-p95. Most are scorekeeping - XP, reputation, health - and cost the look-ahead nothing.
`SetFlag` was the one that was not, and nothing had checked which was which.

---

## 11. The action parser reads the statement separator into the next call's name

**This one is a bug, not a refinement, and it is the largest single thing on this list.**
It costs roughly a third of every script's actions.

**C# today.** `ActionParser.Invocations` finds the next call by skipping to the first
character `IsNameStart` accepts:

```csharp
while (index < script.Length && !IsNameStart(script[index]))
{
    index++;
}
```

A userScript is stored as ONE LINE. Its statements are separated by a literal backslash
followed by the letter `n` - two characters, not a newline - and there are **6,760 of them
across the shipped database**. A backslash is not a name start; a letter is. So the scan
stops one character late and reads the separator into the name:

```
FinishTask("TASK.advanced_ballistics_analysis_done");\nGainTask("TASK.locate_the_firearm")
```

yields a second call named `nGainTask`, which matches no case in `TranslateCall` and falls
through to `DialogueAction.Unmodelled`. **Every statement after the first in every script
is lost this way.**

**How much.** In conversation 631's group, 208 of 672 actions were unmodelled and 78 of
those were purely this: `nSetVariableValue` x23, `nFinishTask` x13, `nReputationGrows` x12,
`nGainTask` x11, `nXPPicoSetBool` x7, `nGainMoneyOnce` x6, `nCancelTask` x4,
`nXPTinySetBool` x2. Fixing it moved that group from 464 modelled actions to 521 and
interned 31 more variable slots. The lost calls are not scorekeeping - they are the
`GainTask`, `FinishTask` and `SetVariableValue` calls that later guards are gated on, so
this is the same failure mode as entry 10 and at four times the size.

**Two smaller ones in the same code.** `StripComments` ends a line comment at `'\n'`, a
character no script contains, so a bare `--` eats the entire remainder (2 scripts in the
database, both prose em-dashes, but total where it happens). And the string scanner treats
`\"` as the closing quote, so an argument carrying reported speech - `NewspaperEndgame`
runs to two kilobytes of it - ends mid-sentence and the rest of the prose tokenizes as
code.

**Rust instead.** One pass replaces `StripComments`, tracking strings and comments
together, because Lua's rules interleave: a quote inside a comment opens no string and a
`--` inside a string opens no comment, so neither can be decided without the other. Outside
a string the separator becomes the newline it stands for; inside one, an escape carries its
next character with it.

**Cost of adopting.** One rewritten private method, no signature changes. Test it on a real
multi-statement script from the database and assert the LAST statement took effect -
asserting the count of actions would pass against a parser that produced three unmodelled
ones.

---

*Entries are appended as they are found. Nothing here is applied to the C# side.*

*Nor does anything belong here that the C# does not need. Bugs the Rust port had and the
C# did not are recorded on the issue that found them and in the code that fixed them - a
list of changes to make is useless if it also contains changes not to make.*
