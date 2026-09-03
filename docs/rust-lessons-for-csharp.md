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

## 9. The engine is needlessly imprecise about items and tasks it does not track

*An improvement NEITHER implementation has - recorded here because it was found while
comparing them, and it applies to both.*

When a guard asks `CheckItem("x")` and no action in the group gains or loses `x`, there is
no slot, and both engines fall through to `world.Query("CheckItem", ...)`. Most worlds
answer unknown to that, so the guard is undecided and the branch stays open.

But the world already knows: `HasItem("x")` is exactly that question and answers
definitely. The information is there and is being thrown away. Using it would make the
crawl strictly more precise - fewer branches kept for no reason, so less walking and fewer
false reachables - and it is SAFE, because being decisive with a correct answer is fine;
only being decisive with a wrong one is not.

Measured while building the guard compiler: `CheckItem` is 140 and `IsTaskActive` 95 of
the world queries the five biggest conversations' guards make, and after every other
improvement they are among the largest remaining undecidable categories.

Do this in the engine and the compiler follows for free, since the compiler mirrors the
engine deliberately. Doing it in the compiler ALONE would be wrong - it would make the
analysis more decisive than the thing it models.

---

*Entries are appended as they are found. Nothing here is applied to the C# side.*
