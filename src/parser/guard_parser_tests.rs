// SPDX-License-Identifier: MIT
//! The C# `GuardParserTests`, ported.
//!
//! Cases chosen for what they pin rather than for coverage: Lua's refusal to coerce
//! across types, three-valued conjunction where one side is definite, the exporter's
//! negation form, and operator precedence. Several are copied from real conversations.
//!
//! In a file of their own rather than at the foot of `guard_parser.rs`, because the
//! parser is only half the subject - every case here parses AND evaluates, and the
//! evaluation lives in `core::guard`.

use crate::core::guard::IGuardContext;
use crate::core::guard_value::GuardValue;
use crate::core::types::Ternary;
use crate::parser::guard_parser::parse_guard;
use crate::world::test_world::TestWorld;
use crate::world::world::ILookAheadWorld;

/// Adapts a world to the guard-evaluation interface, without a search state.
struct WorldContext<'a>(&'a TestWorld);

impl IGuardContext for WorldContext<'_> {
    fn get_variable(&self, name: &str) -> GuardValue {
        self.0.get_variable(name)
    }

    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue {
        self.0.query(name, arguments)
    }
}

/// Parses and evaluates, the way a guard is actually used.
fn test(guard: &str, world: &TestWorld) -> Ternary {
    parse_guard(guard)
        .unwrap_or_else(|e| panic!("{guard} should parse: {e:?}"))
        .test(&WorldContext(world))
}

fn with_bool(name: &str, value: bool) -> TestWorld {
    TestWorld::new().set_variable(name, GuardValue::from_boolean(value))
}

fn with_number(name: &str, value: f64) -> TestWorld {
    TestWorld::new().set_variable(name, GuardValue::from_number(value))
}

#[test]
fn no_condition_is_true() {
    for guard in ["", "   "] {
        assert_eq!(test(guard, &TestWorld::new()), Ternary::True, "{guard:?}");
    }
}

#[test]
fn a_bare_variable_reads_as_truthiness() {
    let world = with_bool("a.b", true);
    assert_eq!(test(r#"Variable["a.b"]"#, &world), Ternary::True);
}

/// The exporter's negation form, and by a wide margin the most common negative shape in
/// the database - thousands of guards are exactly this.
#[test]
fn parenthesised_equals_false_is_negation() {
    assert_eq!(
        test(r#"(Variable["a.b"]) == false"#, &with_bool("a.b", true)),
        Ternary::False
    );
    assert_eq!(
        test(r#"(Variable["a.b"]) == false"#, &with_bool("a.b", false)),
        Ternary::True
    );
}

#[test]
fn block_comments_are_stripped() {
    // The comment holds an unbalanced bracket, so a parser that did not strip it first
    // would try to read it.
    let world = TestWorld::new().set_query_bool("IsTaskActive", true);
    assert_eq!(
        test(r#"IsTaskActive("TASK.x")--[[ Variable[ ]]"#, &world),
        Ternary::True
    );
}

#[test]
fn an_unknown_query_is_unknown_not_false() {
    assert_eq!(test("IsKimHere()", &TestWorld::new()), Ternary::Unknown);
}

/// False beats Unknown: one definitely-false conjunct settles it.
#[test]
fn and_with_a_definite_false_is_false_even_when_the_other_is_unknown() {
    let world = with_bool("a.b", false);
    assert_eq!(
        test(r#"IsKimHere() and Variable["a.b"]"#, &world),
        Ternary::False
    );
}

#[test]
fn or_with_a_definite_true_is_true_even_when_the_other_is_unknown() {
    let world = with_bool("a.b", true);
    assert_eq!(
        test(r#"IsKimHere() or Variable["a.b"]"#, &world),
        Ternary::True
    );
}

#[test]
fn numeric_comparison() {
    assert_eq!(
        test(r#"Variable["q.count"] < 4"#, &with_number("q.count", 3.0)),
        Ternary::True
    );
    assert_eq!(
        test(r#"Variable["q.count"] < 4"#, &with_number("q.count", 4.0)),
        Ternary::False
    );
}

/// The negated counter guard that sits beside it in conversation 825.
#[test]
fn negated_numeric_comparison() {
    assert_eq!(
        test(
            r#"(Variable["q.count"] < 4) == false"#,
            &with_number("q.count", 3.0)
        ),
        Ternary::False
    );
    assert_eq!(
        test(
            r#"(Variable["q.count"] < 4) == false"#,
            &with_number("q.count", 4.0)
        ),
        Ternary::True
    );
}

/// Lua's equality does not coerce across types, so a numeric variable is not equal to
/// true however non-zero it is.
#[test]
fn equality_does_not_coerce_across_types() {
    let world = with_number("q.count", 1.0);
    assert_eq!(
        test(r#"Variable["q.count"] == true"#, &world),
        Ternary::False
    );
}

/// A real multi-clause guard, copied from conversation 451.
#[test]
fn the_siileng_speakers_guard_parses_and_evaluates() {
    const GUARD: &str = concat!(
        r#"Variable["jam.siileng_bought_faln_sneakers"] == true"#,
        r#"  and  Variable["jam.siileng_learned_when_you_can_buy_speakers"] == true"#,
        r#"  and  CheckItem("samaran_speakers") == false"#
    );

    let ready = TestWorld::new()
        .set_variable(
            "jam.siileng_bought_faln_sneakers",
            GuardValue::from_boolean(true),
        )
        .set_variable(
            "jam.siileng_learned_when_you_can_buy_speakers",
            GuardValue::from_boolean(true),
        )
        .set_query_bool("CheckItem", false);
    assert_eq!(test(GUARD, &ready), Ternary::True);

    let no_sneakers = TestWorld::new()
        .set_variable(
            "jam.siileng_bought_faln_sneakers",
            GuardValue::from_boolean(false),
        )
        .set_variable(
            "jam.siileng_learned_when_you_can_buy_speakers",
            GuardValue::from_boolean(true),
        )
        .set_query_bool("CheckItem", false);
    assert_eq!(test(GUARD, &no_sneakers), Ternary::False);
}

#[test]
fn operator_precedence_and_binds_tighter_than_or() {
    // false and false or true  ==  (false and false) or true  ==  true
    let world = TestWorld::new()
        .set_variable("a", GuardValue::from_boolean(false))
        .set_variable("b", GuardValue::from_boolean(false))
        .set_variable("c", GuardValue::from_boolean(true));
    assert_eq!(
        test(
            r#"Variable["a"] and Variable["b"] or Variable["c"]"#,
            &world
        ),
        Ternary::True
    );
}

#[test]
fn garbage_is_an_error() {
    assert!(parse_guard(r#"Variable["a"] $$ 3"#).is_err());
}

/// A failed parse must be reportable without unwinding, and the caller's fallback -
/// treating the guard as absent - must then be true rather than false.
///
/// The engine relies on that: `build_group_graph` turns a parse failure into
/// `always_true`, so a guard nobody can read leaves the branch open instead of silently
/// closing it.
#[test]
fn a_failed_parse_falls_back_to_always_true() {
    let fallback = parse_guard(r#"Variable["a"] $$ 3"#)
        .unwrap_or_else(|_| crate::core::guard::Guard::always_true());
    assert_eq!(
        fallback.test(&WorldContext(&TestWorld::new())),
        Ternary::True
    );
}

/// A negative number is a number, not an operator followed by one.
///
/// Found by a generated guard rather than by the corpus, which never produced one: no guard
/// in the shipped database has a negative literal, so nothing had ever asked. What it cost
/// was the whole guard - a refused parse becomes `always_true`, which leaves a branch open
/// that the comparison was there to close.
#[test]
fn a_negative_number_is_read_as_one() {
    let parsed = parse_guard(r#"Variable["a"] > -1"#).expect("a negative literal parses");
    assert_eq!(parsed.to_string(), r#"(Variable["a"] > -1)"#);

    // And on its own, and in a call argument.
    assert!(parse_guard("-42").is_ok());
    assert!(parse_guard("Thing(-1)").is_ok());
}

/// Running out of tokens mid-expression is an error, not a panic.
///
/// The parser used to report end-of-input as a Name token and then index past the end of
/// the token list. Guards come out of a dialogue database that a game patch or another mod
/// can change, so unparseable input has to be answered - it becomes `always_true` and the
/// branch stays open - and must never take the process with it.
#[test]
fn input_that_stops_mid_expression_is_refused_rather_than_crashing() {
    for truncated in [
        "not",
        r#"Variable["a"] and"#,
        r#"Variable["a"] or"#,
        r#"Variable["a"] =="#,
        "Thing(",
        "(",
        r#"Thing(Variable["a"],"#,
        "-",
    ] {
        assert!(
            parse_guard(truncated).is_err(),
            "{truncated:?} should be refused, not accepted",
        );
    }
}

/// Nesting far past anything real is REFUSED, and refusing is not crashing.
///
/// ## The failure this replaces
///
/// A stack overflow. The parser used to be recursive descent, so nesting depth was stack
/// depth, and until de-fpax there was no bound on it:
/// `deep_nesting_is_answered_rather_than_overflowing` in tests/properties.rs - a test
/// written to prove exactly this - brought the whole test binary down with
/// STATUS_STACK_OVERFLOW on a clean tree. An overflow is not a panic. The guard page is hit,
/// Rust prints, the process ABORTS, and nothing can catch it; inside the game that is the
/// player's session.
///
/// The parser is iterative now (de-bnjy.4) and cannot overflow at all, but everything that
/// USES what it returns still walks the tree by recursion - evaluate, Display, and the Drop
/// that frees it - so the bound is still what keeps the abort out of reach.
///
/// ## The two ends it is pinned between
///
/// Both measured, both worth keeping honest:
///
/// - ELEVEN is the deepest guard in the shipped database, of 26,210, measured by
///   measurements/guard_depth.rs - so everything real is accepted with room to spare. The
///   whole database is re-parsed by tests/guard_depth.rs, which is what keeps saying so.
/// - 800 is where walking a tree overflows a one-megabyte stack, the Windows main-thread
///   default, in a debug build; 2,875 in a release one (measurements/guard_stack.rs). The limit is
///   under a third of the pessimistic figure.
#[test]
fn nesting_deeper_than_anything_real_is_refused_rather_than_fatal() {
    // Comfortably inside, and about ten times the deepest guard the game ships. The count
    // is now the depth a reader sees: the limit is on the TREE, so `not (` is one level and
    // not the two recursion steps the old bound charged for it.
    let real = format!("{}Variable[\"x\"]{}", "not (".repeat(100), ")".repeat(100));
    assert!(
        parse_guard(&real).is_ok(),
        "100 levels of not( should still parse"
    );

    // And past the limit, an ERROR - which is the whole point. The number is not asserted
    // here; what matters is that there is one and that it answers.
    let absurd = format!("{}Variable[\"x\"]{}", "not (".repeat(500), ")".repeat(500));
    let refused = parse_guard(&absurd);
    assert!(refused.is_err(), "500 levels should be refused");
    assert!(
        refused.unwrap_err().to_string().contains("nested"),
        "the message should say what was wrong with it",
    );
}

/// The same, without a parenthesis in sight.
///
/// `not not not x` reaches the depth through a different part of the parser than
/// `not (not (...))` does - a run of prefix operators waiting on the operator stack, rather
/// than a run of open brackets waiting on the frame stack. Both end up as depth in the same
/// tree, and both are bounded, so both are tested.
#[test]
fn unparenthesised_nesting_is_bounded_too() {
    let absurd = format!("{}Variable[\"x\"]", "not ".repeat(500));
    assert!(parse_guard(&absurd).is_err(), "500 nots should be refused");
}

/// A CHAIN is bounded too, which is the hole the old limit left open.
///
/// `a and b and c` never recursed in the recursive-descent parser - conjunctions were
/// gathered by a `while` loop - so a guard with ten thousand `and`s cost nothing to parse
/// and sailed past a limit that counted re-entries. What it produced was a tree ten thousand
/// levels deep down its left side, and evaluating, printing or freeing that walks every one
/// of them. The bound is on the tree now, so the shape that used to slip through does not.
#[test]
fn a_long_chain_is_as_bounded_as_a_deep_nest() {
    let chain = "true and ".repeat(500) + "true";
    let refused = parse_guard(&chain);
    assert!(refused.is_err(), "a 500-long and-chain should be refused");
    assert!(
        refused.unwrap_err().to_string().contains("nested"),
        "the message should say what was wrong with it",
    );

    // And a chain of ordinary length is untouched: real guards are full of these.
    let short = "true and ".repeat(20) + "true";
    assert!(
        parse_guard(&short).is_ok(),
        "a 20-long and-chain is ordinary content"
    );
}
