// SPDX-License-Identifier: MIT
//! Generated shapes, for the two places a hand-written test is worst.
//!
//! ## What this adds that the corpus does not
//!
//! The corpus tests run the parsers over all 13,059 guards and 8,339 scripts in the shipped
//! database, which is the more valuable half and is already done: it is real content, and
//! it is what the mod will actually meet. What it cannot cover is shapes nobody wrote -
//! guards nested past any depth a writer would type, a variable name that is also a
//! keyword, a slot whose actions between them demand more bits than any one of them does.
//! Those are where a recursive-descent parser and a bit-blasted layout tend to break, and a
//! generator finds them without anybody having to imagine them first.
//!
//! ## Why these two and not everything
//!
//! A property test earns its place where there is a LAW rather than an example. Here there
//! are two:
//!
//! - A guard that is printed and read back is the guard it started as.
//! - A slot is wide enough for everything written to it.
//!
//! Both are total statements about all inputs, both are cheap to check, and both are the
//! kind of thing that stays true for a thousand cases and then does not.

use std::collections::HashSet;

use lookahead_engine::core::action::DialogueAction;
use lookahead_engine::core::guard::Guard;
use lookahead_engine::core::guard_value::GuardValue;
use lookahead_engine::core::state::StateSymbols;
use lookahead_engine::core::types::{DialogueCheckKind, DialogueNodeId};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::graph::node::LookAheadNode;
use lookahead_engine::oracle;
use lookahead_engine::parser::guard_parser::parse_guard;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated::on_its_own_thread;
use lookahead_engine::symbolic::reachability::{seed_of, Reachability};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::test_graph::{node, Entry, GraphBuilder};
use lookahead_engine::world::test_world::TestWorld;
use proptest::prelude::*;

/// The counter cap the rest of the repository measures with.
const COUNTER_CAP: i32 = 16;

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// A name a guard can carry, in the shape the database uses.
///
/// Restricted to what the language can WRITE, not to what it can parse. A variable called
/// `and` renders as `Variable["and"]` and reads back fine, but generating names with
/// characters the printer does not escape would be testing the generator rather than the
/// parser.
fn variable_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,6}(\\.[a-z][a-z0-9_]{0,6}){0,2}".prop_map(|s| s)
}

/// A function name, which the tokeniser reads as a bare word.
fn call_name() -> impl Strategy<Value = String> {
    "[A-Za-z][A-Za-z0-9]{0,10}".prop_filter(
        "a keyword is a token rather than a name",
        |name| !matches!(name.as_str(), "and" | "or" | "not" | "true" | "false" | "nil"),
    )
}

/// A literal that survives being printed.
///
/// Whole numbers only: a float prints as many digits as it needs and reads back as the same
/// double, but `0.1 + 0.2` shaped values make a failure look like a parser bug when it is
/// arithmetic. Text without quotes or backslashes, because the printer escapes neither -
/// which is a real limitation of `Display` and not something to hide behind a generator.
fn literal() -> impl Strategy<Value = GuardValue> {
    prop_oneof![
        any::<bool>().prop_map(GuardValue::from_boolean),
        (-1000i32..1000).prop_map(|n| GuardValue::from_number(n as f64)),
        "[a-z_]{1,8}".prop_map(GuardValue::from_text),
    ]
}

/// An operator the tokeniser reads as one token.
fn comparison_operator() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("==".to_string()),
        Just("~=".to_string()),
        Just("<".to_string()),
        Just("<=".to_string()),
        Just(">".to_string()),
        Just(">=".to_string()),
    ]
}

/// A guard, up to `depth` levels of nesting.
fn guard(depth: u32) -> impl Strategy<Value = Guard> {
    let leaf = prop_oneof![
        literal().prop_map(Guard::literal),
        variable_name().prop_map(Guard::variable),
    ];

    leaf.prop_recursive(depth, 64, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(Guard::not),
            (inner.clone(), inner.clone())
                .prop_map(|(l, r)| Guard::and(l, r)),
            (inner.clone(), inner.clone())
                .prop_map(|(l, r)| Guard::or(l, r)),
            (comparison_operator(), inner.clone(), inner.clone())
                .prop_map(|(op, l, r)| Guard::comparison(op, l, r)),
            (call_name(), prop::collection::vec(inner, 0..3))
                .prop_map(|(name, args)| Guard::call(name, args)),
        ]
    })
}

// ---------------------------------------------------------------------------
// The guard parser
// ---------------------------------------------------------------------------

proptest! {
    /// A guard printed and read back is the guard it started as.
    ///
    /// The law the parser and the printer have to keep between them, and neither can be
    /// checked properly without the other. What it catches that examples do not: an
    /// operator whose precedence the printer parenthesises one way and the parser reads
    /// another, which is invisible on any expression shallow enough to write by hand.
    #[test]
    fn a_printed_guard_reads_back_as_itself(expression in guard(4)) {
        let printed = expression.to_string();
        let parsed = parse_guard(&printed)
            .map_err(|e| TestCaseError::fail(format!("{printed:?} would not parse: {e}")))?;

        prop_assert_eq!(
            &parsed,
            &expression,
            "printed as {:?}, read back as {:?}",
            printed,
            parsed.to_string(),
        );
    }

    /// And it keeps reading back the same way however many times it goes round.
    ///
    /// A printer that loses a distinction on the first pass and is stable afterwards would
    /// satisfy the test above only if the loss happened before it; this one says the pair
    /// has reached a fixed point rather than merely agreeing once.
    #[test]
    fn printing_a_parsed_guard_is_stable(expression in guard(3)) {
        let once = expression.to_string();
        let parsed = parse_guard(&once)
            .map_err(|e| TestCaseError::fail(format!("{once:?} would not parse: {e}")))?;
        prop_assert_eq!(parsed.to_string(), once);
    }

    /// The parser answers on any input at all, rather than panicking on some of them.
    ///
    /// A guard comes out of a dialogue database that a game patch or another mod can
    /// change, so "this string is not a guard" has to be an error and never a crash. The
    /// generator is deliberately ARBITRARY TEXT here rather than a guard: the interesting
    /// inputs are the ones no printer would produce.
    #[test]
    fn any_text_is_answered_rather_than_crashed_on(text in "\\PC{0,40}") {
        let _ = parse_guard(&text);
    }

    /// Nesting deeper than anybody writes is answered rather than overflowing the stack.
    ///
    /// A recursive-descent parser's failure mode, and one no corpus can find: the deepest
    /// guard in the shipped database is nothing like this. Either answer is acceptable -
    /// what is not is going down with the process.
    #[test]
    fn deep_nesting_is_answered_rather_than_overflowing(depth in 1usize..300) {
        let text = format!(
            "{}Variable[\"x\"]{}",
            "not (".repeat(depth),
            ")".repeat(depth),
        );
        let _ = parse_guard(&text);
    }
}

// ---------------------------------------------------------------------------
// The state vector's layout
// ---------------------------------------------------------------------------

/// One action against a named slot, as a generated pair.
#[derive(Debug, Clone)]
struct Written {
    slot: String,
    /// True to increment by `amount`, false to assign it.
    increment: bool,
    amount: i32,
}

fn written() -> impl Strategy<Value = Written> {
    ("[a-e]", any::<bool>(), 0i32..64).prop_map(|(slot, increment, amount)| Written {
        slot,
        increment,
        amount,
    })
}

/// A graph of one node carrying `writes`, and the symbols it interned.
fn graph_of(writes: &[Written]) -> (LookAheadGraph, StateSymbols) {
    let mut symbols = StateSymbols::new();
    let mut actions = Vec::new();
    for write in writes {
        let slot = symbols.variable(&write.slot);
        actions.push(if write.increment {
            DialogueAction::increment(slot, write.amount, false, "s".to_string())
        } else {
            DialogueAction::assign(slot, write.amount, "s".to_string())
        });
    }

    let node = LookAheadNode::new(
        DialogueNodeId::new(1, 0),
        false,
        DialogueCheckKind::None,
        Guard::always_true(),
        actions,
        vec![],
        0,
        false,
        false,
        -1,
        -1,
        false,
        -1,
    );

    let snapshot = symbols.clone();
    (LookAheadGraph::new(vec![node], symbols).expect("one node is a graph"), snapshot)
}

proptest! {
    /// Every slot is wide enough for everything written to it.
    ///
    /// The law `DataLayout` exists to keep. It reads widths off the graph's actions rather
    /// than giving every slot the same number of bits, which is what keeps conversation
    /// 631's group near 350 variables instead of 1,700 - and the risk that buys is a slot
    /// one bit too narrow for a value some action assigns, which does not fail. It stores a
    /// DIFFERENT NUMBER, silently, and the search carries on with a state that does not
    /// exist.
    ///
    /// A generated action set is the natural way to ask, because the case that breaks is
    /// several writes to one slot where the widest is not the last.
    #[test]
    fn a_slot_is_wide_enough_for_everything_written_to_it(
        writes in prop::collection::vec(written(), 1..8),
    ) {
        let (graph, symbols) = graph_of(&writes);
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);

        for write in &writes {
            let slot = symbols.find(&write.slot).expect("the slot was interned");
            let (_, bits) = layout.slot(slot).expect("a slot in the layout");
            let ceiling: u32 = if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 };

            // An assignment must be representable outright. An increment saturates at the
            // counter cap, so what has to fit is the cap rather than the step - a step
            // larger than the cap simply arrives at the cap.
            let needed = if write.increment {
                COUNTER_CAP.max(0) as u32
            } else {
                write.amount.max(0) as u32
            };

            prop_assert!(
                ceiling >= needed,
                "slot {:?} got {} bits, holding at most {}, but {} is written to it",
                write.slot,
                bits,
                ceiling,
                needed,
            );
        }
    }

    /// The layout is exactly as wide as the sum of its parts, and its runs do not overlap.
    ///
    /// Two slots sharing a variable would make one slot's value change when the other was
    /// written, which is the kind of thing that produces a wrong answer rather than a
    /// crash.
    #[test]
    fn the_slots_tile_the_variables_without_overlapping(
        writes in prop::collection::vec(written(), 1..8),
    ) {
        let (graph, _) = graph_of(&writes);
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);

        let mut covered = vec![false; layout.total_vars() as usize];
        for slot in 0..layout.slot_count() {
            let (base, bits) = layout.slot(slot).expect("a slot in the layout");
            for bit in 0..bits as u32 {
                let number = (base + bit) as usize;
                prop_assert!(number < covered.len(), "slot {slot} runs past the layout");
                prop_assert!(!covered[number], "variable {number} belongs to two slots");
                covered[number] = true;
            }
        }

        prop_assert!(
            covered.iter().all(|seen| *seen),
            "the layout has variables no slot owns",
        );
    }
}

// ---------------------------------------------------------------------------
// Generated graphs, against the reference walk
// ---------------------------------------------------------------------------

/// The guards a generated entry can carry.
///
/// One of each KIND of answer rather than a variety of expressions: a guard the search can
/// decide false, one it can decide true only after something has been written, a threshold
/// on a counter, and one nothing can decide. What is being generated is the SHAPE of the
/// graph, and richer guards are already covered by the corpus and by the round trip above.
const GUARDS: [&str; 5] = [
    "",
    r#"Variable["open"]"#,
    r#"not Variable["open"]"#,
    r#"Variable["count"] >= 2"#,
    "IsKimHere()",
];

/// And what it can write: nothing, the flag a guard reads, a counter, and a counter that
/// only ever climbs one step.
const SCRIPTS: [&str; 4] = [
    "",
    r#"SetVariableValue("open", true)"#,
    r#"SetVariableValue("count", Variable["count"] + 1)"#,
    r#"SetVariableValue("count", Variable["count"] +once(1))"#,
];

/// What the generator says about one entry.
#[derive(Debug, Clone)]
struct Shape {
    guard: usize,
    script: usize,
    /// 0 an ordinary entry, 1 a white check, 2 a passive check.
    kind: usize,
    /// 0 free, 1 priced every time, 2 priced once - the three ways a purse is read.
    price: usize,
    links: Vec<usize>,
}

fn shape(entries: usize) -> impl Strategy<Value = Shape> {
    (
        0..GUARDS.len(),
        0..SCRIPTS.len(),
        0usize..3,
        0usize..3,
        prop::collection::vec(0..entries, 0..3),
    )
        .prop_map(|(guard, script, kind, price, links)| Shape {
            guard,
            script,
            kind,
            price,
            links,
        })
}

/// A graph of generated entries, built by the same builder every other fixture uses.
fn graph_from(shapes: &[Shape]) -> LookAheadGraph {
    let mut builder = GraphBuilder::new();
    for (index, shape) in shapes.iter().enumerate() {
        let links: Vec<i32> = shape.links.iter().map(|to| *to as i32).collect();
        let mut entry = Entry::new(index as i32)
            .guard(GUARDS[shape.guard])
            .script(SCRIPTS[shape.script])
            .links(&links);
        entry = match shape.kind {
            // A ROLL THAT CLOSES. Every check gets a flag, so a failure is recorded and the
            // check cannot be retried: a white check with nowhere to record its failure is
            // retryable for ever, and a generated graph full of them would spend the walk's
            // whole ceiling on one shape.
            1 => entry.kind(DialogueCheckKind::White).flag(&format!("roll{index}")),
            2 => entry.kind(DialogueCheckKind::Passive),
            _ => entry,
        };
        // FOUR CENTIMES OUT OF TEN, so a path can afford two prices and not three - which is
        // the only interesting size. A price nobody can meet closes an entry for a reason
        // the guards already cover, and one everybody can meet is not a price.
        entry = match shape.price {
            1 => entry.cost(4),
            2 => entry.cost(4).cost_once(),
            _ => entry,
        };
        builder = builder.add(entry);
    }
    builder.build()
}

proptest! {
    // SIXTY-FOUR CASES, not proptest's default. Each one builds a diagram manager on a
    // thread of its own and runs a fixed point in it, which is thousands of times the cost
    // of parsing a guard - and the shapes here are small enough that the interesting ones
    // turn up early.
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The symbolic search reaches every entry the reference walk reaches.
    ///
    /// THE LAW THE WHOLE ENGINE RESTS ON, and the one that cannot be checked by comparing
    /// the two symbolic searches: they share a layout, a guard compiler and their
    /// approximations, so a fault in any of those is invisible to their agreement.
    /// `oracle` walks one state at a time and shares none of it.
    ///
    /// Containment and not equality. The symbolic side may reach MORE - an undecided guard
    /// goes through, and a counter saturating at the cap holds together values the walk
    /// tells apart - and a surplus costs precision where a shortfall costs a marker.
    ///
    /// The generated shapes carry PRICES, in the two forms that read a purse differently:
    /// paid every time, and paid once. Money is in the layout here, as it is in the product,
    /// so a price the walk refuses is one the search has to refuse too.
    ///
    /// Generated rather than written out, because the shapes that break a symbolic search
    /// are the ones nobody thinks to write: a link back into the middle of a cycle, a guard
    /// read before anything writes it, a check whose failure branch is the only way on.
    #[test]
    fn the_symbolic_search_reaches_what_the_reference_walk_reaches(
        shapes in (3usize..8).prop_flat_map(|n| prop::collection::vec(shape(n), n..=n)),
    ) {
        let graph = graph_from(&shapes);
        let world = TestWorld::new()
            .with_money(10)
            .set_variable("open", GuardValue::from_boolean(false))
            .set_variable("count", GuardValue::from_number(0.0));

        let walk = oracle::walk(&graph, node(0), &world, COUNTER_CAP);
        // A walk that ran out of room reaches fewer entries than the graph allows, so it is
        // not an oracle for this shape and the case proves nothing either way.
        prop_assume!(!walk.exhausted());

        let layout = DataLayout::for_group(&graph, &world, COUNTER_CAP);
        let symbols = graph.symbols().clone();

        // A thread of its own, with the manager built inside it - de-fpax.
        let symbolic: HashSet<DialogueNodeId> = on_its_own_thread(|| {
            let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
            let mut compiler = GuardCompiler::new(&vars).with_world(&world);
            let seed = seed_of(&graph, &world, &vars).expect("room for a seed");
            let found = Reachability::explore(
                &graph, node(0), &seed, &mut compiler, &world, COUNTER_CAP as u32,
            );
            found.entries().collect()
        });

        let missed: Vec<&DialogueNodeId> = walk.entries().difference(&symbolic).collect();
        prop_assert!(
            missed.is_empty(),
            "the symbolic search missed {:?}, which the walk reached, on {:?}",
            missed,
            shapes,
        );
    }
}
