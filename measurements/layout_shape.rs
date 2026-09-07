// SPDX-License-Identifier: MIT
//! What the symbolic layout is actually carrying, per slot class, per group.
//!
//! ## The measurement that ranks de-3x76
//!
//! Conversation 14 does not reach a fixed point at six gigabytes and ten minutes, nor at ten
//! and forty; more of both bought it about three hundred steps. So it is not short of
//! allowance, it is carrying too much, and de-3x76 collects the ideas for carrying less.
//!
//! Every one of those ideas claims to remove or narrow some CLASS of slot, and nobody knows
//! how big any class is. This counts them. It needs no search - build the group graph, build
//! the layout, classify - so it is seconds rather than the minutes a search costs, and it is
//! deterministic, which the node-count measurements are not (see the note at the end).
//!
//! ## The classes, and which task each one is evidence for
//!
//! - NEVER WRITTEN: a guard reads it, no action in the group writes it. Its value is fixed
//!   by the save for the whole search. de-3x76.6.
//! - ONCE-ONLY: every increment to it is a one-time action, so it can reach at most the
//!   number of such sites - usually one or two, never the sixteen the cap allows.
//!   de-3x76.2, which is the cheapest task in the epic and needs no graph analysis.
//! - ACYCLIC: incremented, but never inside a cycle, so its maximum is the number of
//!   increment sites on the longest path. de-3x76.4.
//! - CYCLIC: incremented inside a cycle, so it really can reach the cap. These are the only
//!   ones that need the saturating width, and the ones de-3x76.5 would abstract.
//!
//! ## What the slot count already known does NOT say
//!
//! Slots after the read trim: 362 keeps 102, 28 keeps 136,
//! 368 keeps 217, 14 keeps 232, 631 keeps 245. That ranks 631 as the widest - and 631
//! FINISHES the fixed point in 351 seconds while 14 never finishes at all. So the slot count
//! does not predict the thing being optimised. Variables might, and the split by class
//! might; that is what this is for.
//!
//! A RECORD RATHER THAN A CITATION. Those five figures were measured by
//! `tests/slot_width_cost.rs`, which went in 15969c1 along with the state-at-a-time engine
//! and everything that timed it. They are a property of `DataLayout` rather than of that
//! engine, so they should still hold - but nothing re-runs them, and a reader who wants
//! them checked has to measure them again rather than follow a path. Said here because a
//! citation to a file that is not there reads as though the evidence is one file away.
//!
//! Run it with `cargo run --release --example layout_shape`.

use std::collections::{HashMap, HashSet};

use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;

#[path = "../tests/common/mod.rs"]
mod common;

/// TWO MEASUREMENTS IN ONE EXAMPLE, chosen by argument, because they share the classifier.
///
/// An example is one file with one `main`, so the alternative was two files - and the two
/// would have had to duplicate `writes_of`, `on_a_cycle` and `bits_for`, or lift them
/// somewhere both could see. The listing exists precisely to check the classifier the
/// summary is drawn from, so splitting them apart from it is the one arrangement that
/// would defeat the point of having it.
fn main() {
    match std::env::args().nth(1).as_deref() {
        None | Some("groups") => what_each_group_carries(),
        Some("slots") => list_the_slots(),
        Some(other) => {
            eprintln!("unknown argument {other:?}; expected 'groups' (the default) or 'slots'");
            std::process::exit(2);
        }
    }
}

/// How many bits it takes to represent 0..=max, mirroring the layout's own rule.
fn bits_for(max: u32) -> u8 {
    if max == 0 { 1 } else { (u32::BITS - max.leading_zeros()) as u8 }
}

/// Slots that every action writes together, with the same value, at the same entry.
///
/// de-3x76.9. Two such slots carry the same information twice - a task flag and its mirror,
/// a variable renamed with the old one kept in step - and one variable could serve both.
///
/// THE SIGNATURE IS THE WHOLE TEST. A slot's writes, as (entry, kind, value) sorted; two
/// slots with the same signature are written by exactly the same actions to exactly the same
/// values, so nothing a search does can ever tell them apart.
///
/// Only slots the LAYOUT CARRIES and that something actually writes are considered. An
/// unwritten slot has an empty signature and would make every unwritten slot "equivalent",
/// which is true and useless - that class is what `keeping_only_read` and the unwritten
/// column above are for.
fn write_equivalence_classes(
    graph: &LookAheadGraph,
    symbols: &lookahead_engine::core::state::StateSymbols,
    layout: &DataLayout,
) -> Vec<Vec<usize>> {
    let mut signatures: HashMap<usize, Vec<(DialogueNodeId, i32, i32)>> = HashMap::new();
    for node in graph.nodes() {
        for action in &node.actions {
            let slot = action.slot();
            if slot < 0 || slot as usize >= symbols.count() {
                continue;
            }
            signatures.entry(slot as usize).or_default().push((
                node.id,
                action.kind() as i32,
                action.value(),
            ));
        }
    }

    let mut by_signature: HashMap<Vec<(DialogueNodeId, i32, i32)>, Vec<usize>> = HashMap::new();
    for (slot, mut writes) in signatures {
        // Carried by the layout, or collapsing it saves nothing that is being paid for.
        if layout.slot(slot).is_none_or(|(_, bits)| bits == 0) {
            continue;
        }
        writes.sort_by_key(|(id, kind, value)| {
            (id.conversation_id, id.entry_id, *kind, *value)
        });
        by_signature.entry(writes).or_default().push(slot);
    }

    let mut classes: Vec<Vec<usize>> = by_signature
        .into_values()
        .filter(|class| class.len() > 1)
        .map(|mut class| {
            class.sort_unstable();
            class
        })
        .collect();
    classes.sort();
    classes
}

/// Per slot, the largest constant any guard in the group compares it against.
///
/// What makes a narrowed counter SOUND: a value clamped above every constant the guards
/// test answers each of those comparisons the same way the real value would.
///
/// Only the shape `<slot> <op> <literal number>` and its mirror are recognised. A slot
/// compared in ANY other shape is returned in the second set and must keep its full width:
/// the threshold argument only holds if every comparison the slot appears in is one this
/// can see, and a shape it cannot read might distinguish values the clamp would merge.
///
/// REPORTING THAT SET IS THE POINT. Ignoring an unreadable comparison would make the
/// measured saving an UPPER bound that an implementation could not reach, which is exactly
/// the kind of number that gets quoted as if it were the real one.
fn compared_constants(
    graph: &LookAheadGraph,
    symbols: &lookahead_engine::core::state::StateSymbols,
) -> (HashMap<usize, u32>, HashSet<usize>) {
    let mut largest: HashMap<usize, u32> = HashMap::new();
    let mut unreadable: HashSet<usize> = HashSet::new();
    for node in graph.nodes() {
        walk_comparisons(&node.guard, symbols, &mut largest, &mut unreadable);
    }
    (largest, unreadable)
}

/// The slot a guard expression names, if it simply names one.
fn slot_named(
    guard: &lookahead_engine::core::guard::GuardExpression,
    symbols: &lookahead_engine::core::state::StateSymbols,
) -> Option<usize> {
    use lookahead_engine::core::guard::GuardExpression as G;
    let G::Variable(name) = guard else { return None };
    (0..symbols.count()).find(|slot| symbols.name_of(*slot) == Some(name.as_str()))
}

/// The recursion behind [`compared_constants`].
fn walk_comparisons(
    guard: &lookahead_engine::core::guard::GuardExpression,
    symbols: &lookahead_engine::core::state::StateSymbols,
    largest: &mut HashMap<usize, u32>,
    unreadable: &mut HashSet<usize>,
) {
    use lookahead_engine::core::guard::GuardExpression as G;

    match guard {
        G::Comparison(_, a, b) => {
            for (side, other) in [(a.as_ref(), b.as_ref()), (b.as_ref(), a.as_ref())] {
                let Some(slot) = slot_named(side, symbols) else { continue };
                // The slot IS one side of this comparison. Whether it can be narrowed turns
                // on whether the other side is a constant this can read.
                let G::Literal(value) = other else {
                    unreadable.insert(slot);
                    continue;
                };
                let number = value.number();
                if !number.is_finite() || number < 0.0 {
                    unreadable.insert(slot);
                    continue;
                }
                let seen = largest.entry(slot).or_insert(0);
                *seen = (*seen).max(number as u32);
            }
            walk_comparisons(a, symbols, largest, unreadable);
            walk_comparisons(b, symbols, largest, unreadable);
        }
        G::Not(inner) => walk_comparisons(inner, symbols, largest, unreadable),
        G::And(a, b) | G::Or(a, b) => {
            walk_comparisons(a, symbols, largest, unreadable);
            walk_comparisons(b, symbols, largest, unreadable);
        }
        G::Call(_, args) => {
            // A SLOT HANDED TO A QUERY is not a comparison this can reason about at all.
            for arg in args {
                if let Some(slot) = slot_named(arg, symbols) {
                    unreadable.insert(slot);
                }
                walk_comparisons(arg, symbols, largest, unreadable);
            }
        }
        G::Variable(_) | G::Literal(_) => {}
    }
}

/// The groups the matrix measures, so the rows sit beside its numbers.
const GROUPS: [i32; 6] = [362, 28, 368, 14, 631, 1030];

/// The cap every symbolic measurement in this repository uses.
const COUNTER_CAP: i32 = 16;


/// Which entries lie on a cycle.
///
/// Tarjan's, written with an EXPLICIT STACK rather than recursively. A dialogue group is a
/// few thousand entries and the recursion would be as deep as the longest chain; this
/// repository has already lost a day to a recursive walk overflowing (de-fpax), and a
/// measurement that aborts measures nothing.
///
/// An entry is on a cycle if its strongly connected component has more than one member, or
/// if it links to itself.
fn on_a_cycle(graph: &LookAheadGraph) -> HashSet<DialogueNodeId> {
    let ids: Vec<DialogueNodeId> = graph.nodes().map(|node| node.id).collect();
    let mut index_of: HashMap<DialogueNodeId, usize> = HashMap::new();
    let mut low: HashMap<DialogueNodeId, usize> = HashMap::new();
    let mut on_stack: HashSet<DialogueNodeId> = HashSet::new();
    let mut stack: Vec<DialogueNodeId> = Vec::new();
    let mut next_index = 0usize;
    let mut cyclic: HashSet<DialogueNodeId> = HashSet::new();

    // (entry, how many of its links have been dealt with)
    let mut work: Vec<(DialogueNodeId, usize)> = Vec::new();

    for root in ids {
        if index_of.contains_key(&root) {
            continue;
        }
        work.push((root, 0));

        while let Some((id, child)) = work.pop() {
            if child == 0 {
                index_of.insert(id, next_index);
                low.insert(id, next_index);
                next_index += 1;
                stack.push(id);
                on_stack.insert(id);
            }

            let links: &[DialogueNodeId] = match graph.get(id) {
                Some(node) => &node.links,
                None => &[],
            };

            // A link that has been visited since this frame was pushed contributes its
            // lowlink; one never seen is descended into.
            let mut descended = false;
            let mut next = child;
            while next < links.len() {
                let to = links[next];
                next += 1;
                if !index_of.contains_key(&to) {
                    work.push((id, next));
                    work.push((to, 0));
                    descended = true;
                    break;
                }
                if on_stack.contains(&to) {
                    let theirs = index_of[&to];
                    let mine = low[&id];
                    low.insert(id, mine.min(theirs));
                }
                if to == id {
                    cyclic.insert(id);
                }
            }
            if descended {
                continue;
            }

            // Finished with this entry: fold its lowlink into its parent, and close the
            // component if it is a root.
            if low[&id] == index_of[&id] {
                let mut members = Vec::new();
                while let Some(popped) = stack.pop() {
                    on_stack.remove(&popped);
                    members.push(popped);
                    if popped == id {
                        break;
                    }
                }
                if members.len() > 1 {
                    cyclic.extend(members);
                }
            }

            if let Some(&(parent, _)) = work.last() {
                let theirs = low[&id];
                let mine = low[&parent];
                low.insert(parent, mine.min(theirs));
            }
        }
    }

    cyclic
}

/// How each slot is written, gathered in one pass over the graph.
#[derive(Default)]
struct Writes {
    /// Any action writes it at all.
    written: bool,
    /// Every increment to it is a one-time action.
    every_increment_is_once: bool,
    /// Some increment to it sits on a cycle.
    increments_on_a_cycle: bool,
    /// It is incremented anywhere.
    incremented: bool,
    /// How many places increment it.
    increment_sites: usize,
    /// The largest value anything assigns to it.
    max_assigned: u32,
}

impl Writes {
    /// The largest value this slot can actually hold, or None where it is unbounded.
    ///
    /// ## Why the site count is a sound bound
    ///
    /// - A ONCE-GUARDED increment fires at most once ever, whatever the graph looks like,
    ///   so the total is at most the number of such sites.
    /// - AN INCREMENT NOT ON A CYCLE can be passed at most once on any single path, so
    ///   again the total is at most the number of sites.
    /// - AN INCREMENT ON A CYCLE that is not once-guarded can fire without limit. That is
    ///   the only case the saturating cap is for, and no group measured has one.
    ///
    /// An `Assign` writes a value directly, so whatever it writes has to fit too.
    fn ceiling(&self) -> Option<u32> {
        let from_assign = self.max_assigned;
        if !self.incremented {
            return Some(from_assign);
        }
        if self.increments_on_a_cycle && !self.every_increment_is_once {
            return None;
        }
        Some(from_assign.max(self.increment_sites as u32))
    }
}

fn writes_of(graph: &LookAheadGraph, cyclic: &HashSet<DialogueNodeId>, slots: usize) -> Vec<Writes> {
    let mut found: Vec<Writes> = (0..slots)
        .map(|_| Writes { every_increment_is_once: true, ..Writes::default() })
        .collect();

    for node in graph.nodes() {
        // THE ENGINE WRITES SLOTS TOO, and missing that is what made this measurement
        // report a class that does not exist. A rolled check records its own result in
        // `flag_slot` and `failed_flag_slot`, and a `seen_slot` closes a one-time entry;
        // none of those is a parsed action, so a scan of actions alone calls them "never
        // written". Counting them here is what makes the unwritten column mean what it says.
        for slot in [node.flag_slot, node.failed_flag_slot, node.seen_slot, node.once_slot] {
            if let Ok(slot) = usize::try_from(slot) {
                if slot < slots {
                    found[slot].written = true;
                }
            }
        }

        for action in &node.actions {
            let slot = action.slot();
            if slot < 0 || slot as usize >= slots {
                continue;
            }
            let slot = slot as usize;
            match action.kind() {
                DialogueActionKind::Increment => {
                    found[slot].written = true;
                    found[slot].incremented = true;
                    found[slot].increment_sites += 1;
                    if !action.is_once() {
                        found[slot].every_increment_is_once = false;
                    }
                    if cyclic.contains(&node.id) {
                        found[slot].increments_on_a_cycle = true;
                    }
                }
                DialogueActionKind::Assign => {
                    found[slot].written = true;
                    let value = action.value().max(0) as u32;
                    found[slot].max_assigned = found[slot].max_assigned.max(value);
                }
                // Money, clock and unmodelled actions do not write a slot.
                _ => {}
            }
        }
    }

    found
}

fn what_each_group_carries() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");

    println!(
        "{:>5} {:>8} {:>6} {:>6} {:>7} {:>7} {:>7} {:>7} {:>7}",
        "conv", "entries", "slots", "vars", "unwrit", "once", "acyclic", "cyclic", "passes"
    );
    // THE CLOCK IS NOT A COLUMN OF NUMBERS ANY MORE. It costs the shipped layout nothing -
    // the guard compiler folds it in as a constant read from the world - so the only thing
    // worth saying per group is whether folding it in is EXACT or an approximation, which
    // is what "passes" says. What carrying it instead would cost is deliberately not
    // priced here; see the icebox task on the clock.
    println!(
        "  passes is whether the group advances the clock, which is the only case where \
         folding the clock in as a constant approximates rather than answers."
    );

    for conversation in GROUPS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            println!("{conversation:>5}  does not build");
            continue;
        };

        let symbols = graph.symbols().clone();
        let reads = DataLayout::read_by(&graph);
        let passes_time = DataLayout::group_passes_time(&graph);
        // THE LAYOUT THAT SHIPS, which is `DataLayout::for_group` - no clock run at all,
        // because the guard compiler folds the clock in as a constant from the world. This
        // used to pass `passes_time` here, and so reported ELEVEN VARIABLES that the engine
        // does not carry for four of the five groups below (de-3x76.5). Rebuilding the
        // shipped call rather than calling it: `for_group` takes a world, and this
        // measurement has none and wants no money run either.
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
            .keeping_only_read(&symbols, &reads);

        let cyclic = on_a_cycle(&graph);
        let writes = writes_of(&graph, &cyclic, symbols.count());

        // Counted in VARIABLES rather than slots, because a slot is one to five bits and
        // the diagram pays per variable. Both are printed; the second is the one that
        // matters.
        let mut live_slots = 0usize;
        let mut vars = 0usize;
        let mut unwritten = (0usize, 0usize);
        let mut once_only = (0usize, 0usize);
        let mut acyclic = (0usize, 0usize);
        let mut cyclic_slots = (0usize, 0usize);
        let mut widths: HashMap<u8, usize> = HashMap::new();

        for slot in 0..symbols.count() {
            let Some((_, bits)) = layout.slot(slot) else { continue };
            if bits == 0 {
                continue;
            }
            live_slots += 1;
            vars += bits as usize;
            *widths.entry(bits).or_default() += 1;

            let write = &writes[slot];
            if !write.written {
                unwritten.0 += 1;
                unwritten.1 += bits as usize;
            } else if write.incremented && write.every_increment_is_once {
                once_only.0 += 1;
                once_only.1 += bits as usize;
            } else if write.incremented && !write.increments_on_a_cycle {
                acyclic.0 += 1;
                acyclic.1 += bits as usize;
            } else if write.incremented {
                cyclic_slots.0 += 1;
                cyclic_slots.1 += bits as usize;
            }
        }

        // HOW MANY ENTRIES THE SEARCH COULD EVER REACH, by links alone with guards ignored.
        //
        // The upper bound on what any search can find, and the number the fixed point's
        // entries_reached has to be read against: a search that has reached all of these has
        // found every entry there is to find, and anything it does afterwards is refining
        // DATA sets that cannot add an entry. For the question the look-ahead actually asks -
        // is any unseen entry reachable - that later work changes no answer.
        let start = DialogueNodeId::new(conversation, 0);
        let mut seen_from_start = HashSet::new();
        if graph.get(start).is_some() {
            let mut queue = std::collections::VecDeque::new();
            seen_from_start.insert(start);
            queue.push_back(start);
            while let Some(id) = queue.pop_front() {
                let Some(node) = graph.get(id) else { continue };
                for &to in &node.links {
                    if seen_from_start.insert(to) {
                        queue.push_back(to);
                    }
                }
            }
        }
        let real_entries = seen_from_start
            .iter()
            .filter(|id| graph.get(**id).map(|n| !n.is_group).unwrap_or(false))
            .count();
        println!(
            "      {} of {} entries reachable from {}:0 by links ({} non-group)",
            seen_from_start.len(),
            graph.count(),
            conversation,
            real_entries,
        );

        // PRINTED BECAUSE A ZERO IN THE CYCLIC COLUMN IS ALSO WHAT A BROKEN CYCLE FINDER
        // REPORTS. A dialogue group is full of hubs - a menu you return to - so a plausible
        // count here is what says the column above means anything.
        // CHECKS BY KIND, and how many are on a cycle.
        //
        // de-1uy8: a failed check used to leave the state completely unchanged for white,
        // so it could be retried without limit, where the game locks it until a modifier or
        // a skill value changes (FailedWhiteChecks in the game's own code). Both kinds now
        // record the failure - but only where there is a flag to record it in, and a check
        // with no flag at all is still retryable. So the count that matters is the THIRD
        // one: a white check on a cycle with nowhere to write its failure is the loop
        // nothing but the state budget stops.
        let mut white = (0usize, 0usize, 0usize);
        let mut red = (0usize, 0usize, 0usize);
        for node in graph.nodes() {
            let seat = match node.kind {
                lookahead_engine::core::types::DialogueCheckKind::White => &mut white,
                lookahead_engine::core::types::DialogueCheckKind::Red => &mut red,
                _ => continue,
            };
            seat.0 += 1;
            if cyclic.contains(&node.id) {
                seat.1 += 1;
                if node.failed_flag_slot < 0 {
                    seat.2 += 1;
                }
            }
        }
        println!(
            "      checks: {} white ({} on a cycle, {} of those unrecordable), \
             {} red ({} on a cycle, {} of those unrecordable)",
            white.0, white.1, white.2, red.0, red.1, red.2,
        );

        let clock = if passes_time { "yes" } else { "no" };
        println!(
            "      {} of {} entries lie on a cycle",
            cyclic.len(),
            graph.count(),
        );
        println!(
            "{conversation:>5} {:>8} {live_slots:>6} {vars:>6} {:>7} {:>7} {:>7} {:>7} {clock:>7}",
            graph.count(),
            unwritten.0,
            once_only.0,
            acyclic.0,
            cyclic_slots.0,
        );
        println!(
            "      variables: {} unwritten, {} once-only, {} acyclic, {} cyclic",
            unwritten.1, once_only.1, acyclic.1, cyclic_slots.1,
        );
        let mut by_width: Vec<(u8, usize)> = widths.into_iter().collect();
        by_width.sort();
        let shown: Vec<String> =
            by_width.iter().map(|(bits, count)| format!("{bits}b x{count}")).collect();
        println!("      widths: {}", shown.join(", "));

        // WHAT de-3x76.2 WOULD BUY. Each slot re-widened to the largest value it can
        // actually hold rather than to the blanket cap, and the counters listed one by one
        // because there are few enough to read.
        let mut derived = 0usize;
        let mut narrowed: Vec<String> = Vec::new();
        for slot in 0..symbols.count() {
            let Some((_, bits)) = layout.slot(slot) else { continue };
            if bits == 0 {
                continue;
            }
            let wanted = match writes[slot].ceiling() {
                // Unbounded: it keeps the cap, which is what the cap is for.
                None => bits,
                Some(ceiling) => {
                    let needed = if ceiling <= 1 { 1 } else { bits_for(ceiling) };
                    needed.min(bits)
                }
            };
            derived += wanted as usize;
            if wanted < bits {
                narrowed.push(format!(
                    "{} {bits}b->{wanted}b",
                    symbols.name_of(slot).unwrap_or("?"),
                ));
            }
        }
        println!(
            "      derived widths would give {derived} variables against {vars}, saving {} \
             ({:.1}%)",
            vars - derived,
            if vars > 0 { (vars - derived) as f64 / vars as f64 * 100.0 } else { 0.0 },
        );
        // AND WHAT IT WOULD BUY SOUNDLY. The derivation above is UNSAFE on its own:
        // `seed_of` clamps the save's starting value to the slot's ceiling, and the site
        // count bounds what the GROUP adds rather than what the SAVE arrives holding - so
        // narrowing reputation.communist to two bits silently turns a save holding 8 into a
        // 3 and flips a guard testing >= 5.
        //
        // A clamp is sound whenever no guard can tell the difference, so take the ceiling as
        // the larger of the site count and the biggest constant any guard compares the slot
        // against. Above that constant every comparison in the group answers the same for
        // the clamped value and the real one. No world needed, so unlike the world-aware
        // version this costs the workspace key nothing. de-3x76.2.
        let (thresholds, unreadable) = compared_constants(&graph, &symbols);
        let mut sound = 0usize;
        let mut blocked = 0usize;
        for slot in 0..symbols.count() {
            let Some((_, bits)) = layout.slot(slot) else { continue };
            if bits == 0 {
                continue;
            }
            // A slot read by a comparison this cannot see through keeps its full width: the
            // clamp is only sound if EVERY comparison agrees, and one that cannot be read
            // cannot be shown to.
            let ceiling = match writes[slot].ceiling() {
                _ if unreadable.contains(&slot) => {
                    if bits > 1 {
                        blocked += 1;
                    }
                    None
                }
                None => None,
                Some(sites) => Some(sites.max(thresholds.get(&slot).copied().unwrap_or(0))),
            };
            let wanted = match ceiling {
                None => bits,
                Some(ceiling) => {
                    let needed = if ceiling <= 1 { 1 } else { bits_for(ceiling) };
                    needed.min(bits)
                }
            };
            sound += wanted as usize;
        }
        println!(
            "      sound derived widths give {sound} variables against {vars}, saving {} \
             ({:.1}%); {blocked} counter(s) held back by a comparison it cannot read",
            vars - sound,
            if vars > 0 { (vars - sound) as f64 / vars as f64 * 100.0 } else { 0.0 },
        );

        // WHAT de-3x76.9 WOULD BUY, which that task asked be measured before being built:
        // slots that every action writes TOGETHER, with the same value, at the same entry.
        // Two of those carry the same information twice and one variable could do for both.
        let classes = write_equivalence_classes(&graph, &symbols, &layout);
        let shared: usize = classes.iter().map(|class| class.len() - 1).sum();
        println!(
            "      {} non-trivial write-equivalence class(es), {shared} variable(s) they \
             could share",
            classes.len(),
        );
        for class in &classes {
            let names: Vec<&str> =
                class.iter().map(|slot| symbols.name_of(*slot).unwrap_or("?")).collect();
            println!("        {}", names.join(" = "));
        }

        if !narrowed.is_empty() {
            println!("      narrowed: {}", narrowed.join(", "));
        }
    }

    println!(
        "\nDETERMINISTIC, unlike the node-count measurements: this reads the graph and the \
         layout and never runs a search, so two runs agree exactly."
    );
}


/// Every live slot in one group, with how it is written.
///
/// Exists because "conversation 14 has one counter" is a strong claim drawn from a
/// histogram, and a histogram is exactly the shape of summary that hides a mistake in the
/// classifier. This prints the slots themselves so the claim can be read rather than
/// trusted.
///
/// `CONVERSATION=14 cargo run --release --example layout_shape -- slots`
fn list_the_slots() {
    let conversation: i32 = std::env::var("CONVERSATION")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(14);

    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let Ok((graph, _)) = build_group_graph(&index, conversation) else {
        eprintln!("conversation {conversation} does not build; skipping.");
        return;
    };

    let symbols = graph.symbols().clone();
    let reads = DataLayout::read_by(&graph);
    let passes_time = DataLayout::group_passes_time(&graph);
    // The shipped layout, as above: no clock run, because the compiler folds it in.
    let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
        .keeping_only_read(&symbols, &reads);
    let cyclic = on_a_cycle(&graph);
    let writes = writes_of(&graph, &cyclic, symbols.count());

    let mut rows: Vec<(String, u8, String)> = Vec::new();
    for slot in 0..symbols.count() {
        let Some((_, bits)) = layout.slot(slot) else { continue };
        if bits == 0 {
            continue;
        }
        let name = symbols.name_of(slot).unwrap_or("?").to_string();
        let write = &writes[slot];
        let how = if !write.written {
            "never written".to_string()
        } else if write.incremented {
            format!(
                "INCREMENTED at {} site(s){}{}",
                write.increment_sites,
                if write.every_increment_is_once { ", all once-guarded" } else { "" },
                if write.increments_on_a_cycle { ", ON A CYCLE" } else { "" },
            )
        } else {
            format!("assigned, largest {}", write.max_assigned)
        };
        rows.push((name, bits, how));
    }

    rows.sort();
    println!(
        "conversation {conversation}: {} live slots, {} variables\n",
        rows.len(),
        layout.total_vars(),
    );
    for (name, bits, how) in &rows {
        println!("  {bits}b  {name:<44} {how}");
    }

    let counters = rows.iter().filter(|(_, bits, _)| *bits > 1).count();
    println!("\n{counters} slot(s) wider than one bit.");
    if passes_time {
        println!(
            "this group advances the clock, so folding the clock in as a constant is an \
             APPROXIMATION here rather than exact"
        );
    }
}
