// SPDX-License-Identifier: MIT
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::core::guard_value::{GuardValue, GuardValueKind};
use crate::core::types::{Ternary, ternary_and, ternary_not, ternary_or};

/// Context for evaluating guards - provides variable values and world queries.
pub trait IGuardContext: Send + Sync {
    fn get_variable(&self, name: &str) -> GuardValue;
    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue;
}

/// Where one node sits in a guard's table.
type NodeId = u32;

/// One node of a guard: plain data, with its children addressed by index.
///
/// Nothing here owns another node, which is the whole point - see [`Guard`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Node {
    Literal(GuardValue),
    Variable(String),
    /// `name(...)`, whose arguments are a run in [`Guard::arguments`].
    Call {
        name: String,
        first: u32,
        count: u32,
    },
    Not(NodeId),
    And(NodeId, NodeId),
    Or(NodeId, NodeId),
    Comparison {
        operator: String,
        left: NodeId,
        right: NodeId,
    },
}

/// A parsed guard expression, as a flat table.
///
/// ## THIS CRATE NOW DEFINES NO RECURSIVE TYPE AT ALL
///
/// Swept for rather than claimed: `tools/recursive-types.py` builds the type-reference graph
/// out of rustdoc's own JSON and looks for cycles, and across 107 type definitions it finds
/// none. A guard was the last one. Re-run it rather than trusting this sentence - a list of
/// recursive types is exactly the thing that goes stale silently:
///
/// ```text
/// RUSTDOCFLAGS="-Z unstable-options --output-format json --document-private-items" \
///   cargo +nightly rustdoc --lib
/// python tools/recursive-types.py
/// ```
///
/// What the sweep still reports is oxidd's `BDDFunction`, which is the dependency's own
/// node store and not something this crate can flatten.
///
/// ## WHY A TABLE RATHER THAN A TREE OF BOXES
///
/// Because a tree of boxes is a recursive type, and a recursive type is recursive in the
/// one place no error can be returned from: `Drop`. Freeing a nest of `Box`es walks it, so
/// the deeper the guard the deeper the stack at the moment it is thrown away - and an
/// overflow there is not a panic a host can catch. The process aborts. Inside the game that
/// is the player's session.
///
/// A `Vec` of nodes owns no node, so dropping a guard is ONE DEALLOCATION whatever its
/// shape. Cloning is one allocation and a memcpy of plain data rather than a walk. And the
/// nodes of one guard sit together in memory, where a boxed tree scatters them.
///
/// This is the shape oxidd already uses for its own node store, and it is exactly why
/// `drop_edge` there does not cascade. The precedent is in the dependency.
///
/// ## THE TWO INVARIANTS, and both are maintained by construction
///
/// 1. THE ROOT IS THE LAST NODE. Every constructor appends a node's children before the
///    node itself, so `nodes` is never empty and `nodes.len() - 1` is the whole guard.
/// 2. EVERY CHILD IS EARLIER THAN ITS PARENT. That follows from the first, and it is what
///    makes a single forward sweep of `nodes` a complete bottom-up evaluation - see
///    [`Guard::evaluate`], which needs no stack at all because of it.
///
/// Both are checked in the tests rather than only asserted here.
///
/// ## WHAT STILL WALKS A GUARD TOP-DOWN, and why that costs no stack
///
/// `GuardCompiler::compile_node` does, over [`GuardRef`] handles rather than over the data. It
/// is top-down DELIBERATELY: a comparison compiles neither operand - it reads their shape and
/// answers from the register - so a bottom-up sweep would build a decision diagram for every
/// operand that comparisons never look at, which is work the demand-driven walk does not do.
/// It walks on a work stack of its own rather than the thread's, so no guard is too deep for
/// it, and the parser accepts a guard of any depth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Guard {
    /// Every node, children before parents, the root last.
    nodes: Vec<Node>,
    /// The arguments of every [`Node::Call`], as runs indexed by `first` and `count`.
    arguments: Vec<NodeId>,
}

/// One node of a guard, and the guard it belongs to.
///
/// A handle rather than a borrow of a node, because a node's children are indices and mean
/// nothing without the table they index. `Copy`, so passing one costs what passing a
/// reference costs.
#[derive(Debug, Clone, Copy)]
pub struct GuardRef<'a> {
    guard: &'a Guard,
    node: NodeId,
}

/// What a [`GuardRef`] IS: the shape to match on, children as handles.
///
/// Borrowed rather than owned, so matching one costs nothing and the arms read the way
/// they read over a tree.
#[derive(Debug, Clone, Copy)]
pub enum GuardExpression<'a> {
    Literal(&'a GuardValue),
    Variable(&'a str),
    Call(&'a str, Arguments<'a>),
    Not(GuardRef<'a>),
    And(GuardRef<'a>, GuardRef<'a>),
    Or(GuardRef<'a>, GuardRef<'a>),
    Comparison(&'a str, GuardRef<'a>, GuardRef<'a>),
}

/// The arguments of one call, as handles.
///
/// Not a slice, because the arguments are stored as indices and a slice of those is not a
/// slice of expressions. [`Arguments::only`] covers the shape nearly every caller wants -
/// a query of exactly one literal.
#[derive(Debug, Clone, Copy)]
pub struct Arguments<'a> {
    guard: &'a Guard,
    ids: &'a [NodeId],
}

impl Guard {
    /// The guard that lets everything through, which is what an entry with none has.
    pub fn always_true() -> Self {
        Self::literal(GuardValue::from_boolean(true))
    }

    pub fn literal(value: GuardValue) -> Self {
        Self::leaf(Node::Literal(value))
    }

    pub fn variable(name: impl Into<String>) -> Self {
        Self::leaf(Node::Variable(name.into()))
    }

    /// Named for the guard language's own operator, beside [`Self::and`] and [`Self::or`],
    /// which is worth more than avoiding the resemblance to `std::ops::Not`. This takes a
    /// guard and returns one; that trait takes `self`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(inner: Self) -> Self {
        let mut built = inner;
        let child = built.root();
        built.nodes.push(Node::Not(child));
        built
    }

    pub fn and(left: Self, right: Self) -> Self {
        Self::binary(left, right, Node::And)
    }

    pub fn or(left: Self, right: Self) -> Self {
        Self::binary(left, right, Node::Or)
    }

    pub fn comparison(operator: impl Into<String>, left: Self, right: Self) -> Self {
        let operator = operator.into();
        Self::binary(left, right, move |a, b| Node::Comparison {
            operator,
            left: a,
            right: b,
        })
    }

    pub fn call(name: impl Into<String>, arguments: Vec<Self>) -> Self {
        let mut built = Self {
            nodes: Vec::new(),
            arguments: Vec::new(),
        };
        let mut roots = Vec::with_capacity(arguments.len());
        for argument in arguments {
            roots.push(built.absorb(argument));
        }

        let first = built.arguments.len() as u32;
        let count = roots.len() as u32;
        built.arguments.extend(roots);
        built.nodes.push(Node::Call {
            name: name.into(),
            first,
            count,
        });
        built
    }

    /// The whole guard, as the handle everything reads it through.
    pub fn as_ref(&self) -> GuardRef<'_> {
        GuardRef {
            guard: self,
            node: self.root(),
        }
    }

    /// What the guard is, ready to match on.
    pub fn expression(&self) -> GuardExpression<'_> {
        self.as_ref().expression()
    }

    /// How deeply the guard nests: one for a leaf, one more for each level above it.
    ///
    /// A forward sweep rather than a walk, for the reason the type doc gives: a node's
    /// children are always earlier than it is, so one pass in index order has every child's
    /// answer before it needs it.
    pub fn depth(&self) -> usize {
        let mut depths: Vec<usize> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let below = match node {
                Node::Literal(_) | Node::Variable(_) => 0,
                Node::Not(inner) => depths[*inner as usize],
                Node::And(a, b) | Node::Or(a, b) => depths[*a as usize].max(depths[*b as usize]),
                Node::Comparison { left, right, .. } => {
                    depths[*left as usize].max(depths[*right as usize])
                }
                Node::Call { first, count, .. } => self
                    .run(*first, *count)
                    .iter()
                    .map(|id| depths[*id as usize])
                    .max()
                    .unwrap_or(0),
            };
            depths.push(below + 1);
        }

        depths[self.root() as usize]
    }

    /// Every node of the guard, children before parents.
    ///
    /// WHAT A CALLER THAT ONLY COLLECTS WANTS - the variables a guard reads, the queries it
    /// makes, whether it names money anywhere. Gathering from every node needs no structure
    /// at all, so a sweep answers those without a walk and without a stack, and the shape of
    /// the guard never enters into it.
    ///
    /// A caller that needs the structure - which side of a comparison is the literal, what
    /// a conjunction's halves are - starts at [`Self::as_ref`] instead.
    /// The same guard with every variable `settled` answers for replaced by that value, or
    /// `None` where it answers for none of them.
    ///
    /// ## Why this can be a map rather than a rebuild
    ///
    /// The table is FLAT and a node's children are indices into it, so replacing a leaf with
    /// another leaf leaves every index exactly where it was. Nothing is restructured, nothing
    /// is renumbered, and a guard of any shape substitutes in one pass. A rebuild that walked
    /// the tree would have to reproduce the numbering to stay equivalent, which is work and a
    /// chance to be wrong for no gain.
    ///
    /// `None` RATHER THAN A COPY THAT CHANGED NOTHING, so a caller can tell whether it is worth
    /// keeping a second guard at all - most guards mention no settled variable.
    pub fn substituting(&self, settled: impl Fn(&str) -> Option<GuardValue>) -> Option<Self> {
        let mut replaced = false;
        let nodes: Vec<Node> = self
            .nodes
            .iter()
            .map(|node| match node {
                Node::Variable(name) => match settled(name) {
                    Some(value) => {
                        replaced = true;
                        Node::Literal(value)
                    }
                    None => node.clone(),
                },
                other => other.clone(),
            })
            .collect();

        replaced.then(|| Self {
            nodes,
            arguments: self.arguments.clone(),
        })
    }

    pub fn nodes(&self) -> impl Iterator<Item = GuardRef<'_>> + '_ {
        (0..self.nodes.len() as NodeId).map(move |node| GuardRef { guard: self, node })
    }

    /// The value of the whole guard under `context`.
    ///
    /// ONE FORWARD SWEEP, NO STACK. Children are always earlier in the table than their
    /// parent, so walking the nodes in index order computes every operand before the
    /// operator that reads it, and the last answer is the guard's.
    ///
    /// It evaluates every node rather than short-circuiting, which is what a walk over this
    /// has always done: a conjunction here takes the ternary AND of both sides, and an
    /// unknown operand is a value rather than a reason to stop.
    pub fn evaluate(&self, context: &dyn IGuardContext) -> GuardValue {
        let mut values: Vec<GuardValue> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let value = match node {
                Node::Literal(value) => value.clone(),
                Node::Variable(name) => context.get_variable(name),
                Node::Call { name, first, count } => {
                    let arguments: Vec<GuardValue> = self
                        .run(*first, *count)
                        .iter()
                        .map(|id| values[*id as usize].clone())
                        .collect();
                    context.query(name, &arguments)
                }
                Node::Not(inner) => {
                    from_ternary(ternary_not(values[*inner as usize].as_condition()))
                }
                Node::And(a, b) => from_ternary(ternary_and(
                    values[*a as usize].as_condition(),
                    values[*b as usize].as_condition(),
                )),
                Node::Or(a, b) => from_ternary(ternary_or(
                    values[*a as usize].as_condition(),
                    values[*b as usize].as_condition(),
                )),
                Node::Comparison {
                    operator,
                    left,
                    right,
                } => compare(operator, &values[*left as usize], &values[*right as usize]),
            };
            values.push(value);
        }

        values.swap_remove(self.root() as usize)
    }

    pub fn test(&self, context: &dyn IGuardContext) -> Ternary {
        self.evaluate(context).as_condition()
    }

    /// The last node, which the invariant makes the root.
    fn root(&self) -> NodeId {
        debug_assert!(!self.nodes.is_empty(), "a guard always has a root");
        self.nodes.len() as NodeId - 1
    }

    /// One call's run of argument ids.
    fn run(&self, first: u32, count: u32) -> &[NodeId] {
        &self.arguments[first as usize..(first + count) as usize]
    }

    fn leaf(node: Node) -> Self {
        Self {
            nodes: vec![node],
            arguments: Vec::new(),
        }
    }

    /// Two sub-guards under one operator, in the order that keeps the invariants.
    fn binary(left: Self, right: Self, node: impl FnOnce(NodeId, NodeId) -> Node) -> Self {
        let mut built = left;
        let a = built.root();
        let b = built.absorb(right);
        built.nodes.push(node(a, b));
        built
    }

    /// Copies `other`'s table onto the end of this one, and says where its root landed.
    ///
    /// The only place indices are rewritten, and the only place they can go wrong: every
    /// id inside `other` is relative to its own table, so it moves by however many nodes
    /// are already here.
    fn absorb(&mut self, other: Self) -> NodeId {
        let shift = self.nodes.len() as NodeId;
        let arguments_shift = self.arguments.len() as u32;
        self.arguments
            .extend(other.arguments.iter().map(|id| id + shift));
        self.nodes
            .extend(other.nodes.into_iter().map(|node| match node {
                Node::Literal(value) => Node::Literal(value),
                Node::Variable(name) => Node::Variable(name),
                Node::Call { name, first, count } => Node::Call {
                    name,
                    first: first + arguments_shift,
                    count,
                },
                Node::Not(inner) => Node::Not(inner + shift),
                Node::And(a, b) => Node::And(a + shift, b + shift),
                Node::Or(a, b) => Node::Or(a + shift, b + shift),
                Node::Comparison {
                    operator,
                    left,
                    right,
                } => Node::Comparison {
                    operator,
                    left: left + shift,
                    right: right + shift,
                },
            }));

        self.nodes.len() as NodeId - 1
    }
}

impl<'a> GuardRef<'a> {
    /// What this node is, ready to match on.
    pub fn expression(self) -> GuardExpression<'a> {
        match &self.guard.nodes[self.node as usize] {
            Node::Literal(value) => GuardExpression::Literal(value),
            Node::Variable(name) => GuardExpression::Variable(name),
            Node::Call { name, first, count } => GuardExpression::Call(
                name,
                Arguments {
                    guard: self.guard,
                    ids: self.guard.run(*first, *count),
                },
            ),
            Node::Not(inner) => GuardExpression::Not(self.to(*inner)),
            Node::And(a, b) => GuardExpression::And(self.to(*a), self.to(*b)),
            Node::Or(a, b) => GuardExpression::Or(self.to(*a), self.to(*b)),
            Node::Comparison {
                operator,
                left,
                right,
            } => GuardExpression::Comparison(operator, self.to(*left), self.to(*right)),
        }
    }

    /// This node and everything under it, on its own.
    ///
    /// Copies rather than borrows, because a sub-guard is a `Guard` and a `Guard` owns its
    /// table. Wanted where something has to be kept or handed on rather than read in place.
    pub fn to_guard(self) -> Guard {
        match self.expression() {
            GuardExpression::Literal(value) => Guard::literal(value.clone()),
            GuardExpression::Variable(name) => Guard::variable(name),
            GuardExpression::Call(name, arguments) => {
                Guard::call(name, arguments.iter().map(|a| a.to_guard()).collect())
            }
            GuardExpression::Not(inner) => Guard::not(inner.to_guard()),
            GuardExpression::And(a, b) => Guard::and(a.to_guard(), b.to_guard()),
            GuardExpression::Or(a, b) => Guard::or(a.to_guard(), b.to_guard()),
            GuardExpression::Comparison(operator, left, right) => {
                Guard::comparison(operator, left.to_guard(), right.to_guard())
            }
        }
    }

    fn to(self, node: NodeId) -> Self {
        Self {
            guard: self.guard,
            node,
        }
    }
}

impl<'a> Arguments<'a> {
    pub fn len(self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(self) -> bool {
        self.ids.is_empty()
    }

    pub fn get(self, index: usize) -> Option<GuardRef<'a>> {
        self.ids.get(index).map(|node| GuardRef {
            guard: self.guard,
            node: *node,
        })
    }

    /// The single argument, where there is exactly one.
    ///
    /// The shape nearly every reader wants: `CheckItem("ledger")` and `IsHour(3)` are one
    /// literal, and a call with any other number of arguments is one this model does not
    /// answer.
    pub fn only(self) -> Option<GuardRef<'a>> {
        match self.ids {
            [node] => Some(GuardRef {
                guard: self.guard,
                node: *node,
            }),
            _ => None,
        }
    }

    pub fn iter(self) -> impl Iterator<Item = GuardRef<'a>> + 'a {
        let guard = self.guard;
        self.ids
            .iter()
            .map(move |node| GuardRef { guard, node: *node })
    }
}

impl fmt::Display for Guard {
    /// ONE FORWARD SWEEP, like [`Guard::evaluate`] and for the same reason: a node's
    /// children are rendered before it, so nothing here recurses.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut rendered: Vec<String> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let text = match node {
                Node::Literal(value) => format!("{value}"),
                Node::Variable(name) => format!("Variable[\"{name}\"]"),
                Node::Call { name, first, count } => {
                    let arguments: Vec<&str> = self
                        .run(*first, *count)
                        .iter()
                        .map(|id| rendered[*id as usize].as_str())
                        .collect();
                    format!("{name}({})", arguments.join(", "))
                }
                Node::Not(inner) => format!("not {}", rendered[*inner as usize]),
                Node::And(a, b) => {
                    format!("({} and {})", rendered[*a as usize], rendered[*b as usize])
                }
                Node::Or(a, b) => {
                    format!("({} or {})", rendered[*a as usize], rendered[*b as usize])
                }
                Node::Comparison {
                    operator,
                    left,
                    right,
                } => format!(
                    "({} {operator} {})",
                    rendered[*left as usize], rendered[*right as usize]
                ),
            };
            rendered.push(text);
        }

        f.write_str(&rendered[self.root() as usize])
    }
}

impl fmt::Display for GuardRef<'_> {
    /// One node and what is under it, which is what a diagnostic naming a sub-expression
    /// wants. Built through [`GuardRef::to_guard`] so there is one renderer and not two.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_guard())
    }
}

/// A ternary as the value a guard carries.
fn from_ternary(answer: Ternary) -> GuardValue {
    match answer {
        Ternary::Unknown => GuardValue::unknown(),
        Ternary::True => GuardValue::from_boolean(true),
        Ternary::False => GuardValue::from_boolean(false),
    }
}

/// Two values under a comparison operator, the way the guard language spells it.
fn compare(operator: &str, left: &GuardValue, right: &GuardValue) -> GuardValue {
    if left.kind() == GuardValueKind::Unknown || right.kind() == GuardValueKind::Unknown {
        return GuardValue::unknown();
    }

    match operator {
        "==" => GuardValue::from_boolean(left.equals(right)),
        "~=" => GuardValue::from_boolean(!left.equals(right)),
        ">=" | "<=" | ">" | "<" => {
            let Some(a) = left.try_as_number() else {
                return GuardValue::unknown();
            };
            let Some(b) = right.try_as_number() else {
                return GuardValue::unknown();
            };
            let holds = match operator {
                ">=" => a >= b,
                "<=" => a <= b,
                ">" => a > b,
                "<" => a < b,
                _ => return GuardValue::unknown(),
            };
            GuardValue::from_boolean(holds)
        }
        _ => GuardValue::unknown(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A guard reading nothing, for the shapes that do not ask.
    struct Nothing;

    impl IGuardContext for Nothing {
        fn get_variable(&self, _name: &str) -> GuardValue {
            GuardValue::unknown()
        }

        fn query(&self, _name: &str, _arguments: &[GuardValue]) -> GuardValue {
            GuardValue::unknown()
        }
    }

    fn truth(value: bool) -> Guard {
        Guard::literal(GuardValue::from_boolean(value))
    }

    /// Every node's children sit earlier in the table than it does.
    ///
    /// THE INVARIANT THE SWEEPS REST ON. `evaluate` and `Display` walk the nodes in index
    /// order and index straight into what they have already computed, which is only correct
    /// while this holds - so it is checked on a guard built every way there is to build one
    /// rather than asserted in a comment.
    fn children_come_first(guard: &Guard) {
        for (index, node) in guard.nodes.iter().enumerate() {
            let index = index as NodeId;
            let check = |child: NodeId| {
                assert!(child < index, "node {index} points forward at {child}");
            };
            match node {
                Node::Literal(_) | Node::Variable(_) => {}
                Node::Not(inner) => check(*inner),
                Node::And(a, b) | Node::Or(a, b) => {
                    check(*a);
                    check(*b);
                }
                Node::Comparison { left, right, .. } => {
                    check(*left);
                    check(*right);
                }
                Node::Call { first, count, .. } => {
                    for id in guard.run(*first, *count) {
                        check(*id);
                    }
                }
            }
        }
    }

    #[test]
    fn every_shape_puts_its_children_before_itself() {
        let guard = Guard::and(
            Guard::or(
                Guard::not(Guard::variable("a")),
                Guard::comparison("==", Guard::variable("b"), truth(true)),
            ),
            Guard::call(
                "CheckItem",
                vec![Guard::literal(GuardValue::from_text("ledger".to_string()))],
            ),
        );

        children_come_first(&guard);
        assert_eq!(guard.root() as usize, guard.nodes().count() - 1);
    }

    /// A call's arguments survive being absorbed into a larger guard.
    ///
    /// The one index rewrite that has two tables to keep in step - the nodes AND the run of
    /// argument ids - so it is the one most able to be subtly wrong. Reading the arguments
    /// back after the call has been nested twice is what checks it.
    #[test]
    fn arguments_still_point_at_their_own_nodes_after_nesting() {
        let call = Guard::call(
            "IsHour",
            vec![
                Guard::literal(GuardValue::from_number(3.0)),
                Guard::literal(GuardValue::from_number(4.0)),
            ],
        );
        let guard = Guard::and(
            Guard::not(Guard::variable("a")),
            Guard::or(truth(false), call),
        );

        children_come_first(&guard);

        let GuardExpression::And(_, right) = guard.expression() else {
            panic!("an and")
        };
        let GuardExpression::Or(_, call) = right.expression() else {
            panic!("an or")
        };
        let GuardExpression::Call(name, arguments) = call.expression() else {
            panic!("a call")
        };
        assert_eq!(name, "IsHour");
        assert_eq!(arguments.len(), 2);

        let numbers: Vec<f64> = arguments
            .iter()
            .map(|argument| match argument.expression() {
                GuardExpression::Literal(value) => value.try_as_number().expect("a number"),
                _ => panic!("a literal"),
            })
            .collect();
        assert_eq!(numbers, vec![3.0, 4.0]);
    }

    #[test]
    fn a_conjunction_is_the_ternary_and_of_its_sides() {
        assert_eq!(
            Guard::and(truth(true), truth(true)).test(&Nothing),
            Ternary::True
        );
        assert_eq!(
            Guard::and(truth(true), truth(false)).test(&Nothing),
            Ternary::False
        );
        assert_eq!(
            Guard::or(truth(false), truth(true)).test(&Nothing),
            Ternary::True
        );
        assert_eq!(Guard::not(truth(false)).test(&Nothing), Ternary::True);
    }

    /// An unknown operand makes the comparison unknown rather than false.
    #[test]
    fn a_comparison_against_an_unknown_is_unknown() {
        let guard = Guard::comparison("==", Guard::variable("never_set"), truth(true));
        assert_eq!(guard.test(&Nothing), Ternary::Unknown);
    }

    #[test]
    fn depth_counts_the_levels_a_reader_would_count() {
        assert_eq!(truth(true).depth(), 1);
        assert_eq!(Guard::not(truth(true)).depth(), 2);
        assert_eq!(Guard::and(Guard::not(truth(true)), truth(false)).depth(), 3);
    }

    /// A sub-expression taken out on its own says what it said in place.
    #[test]
    fn a_subguard_renders_and_evaluates_as_it_did_nested() {
        let guard = Guard::and(Guard::not(Guard::variable("a")), truth(true));
        let GuardExpression::And(left, _) = guard.expression() else {
            panic!("an and")
        };

        let alone = left.to_guard();
        children_come_first(&alone);
        assert_eq!(alone.to_string(), "not Variable[\"a\"]");
        assert_eq!(alone.test(&Nothing), Ternary::Unknown);
    }

    #[test]
    fn display_brackets_the_way_the_language_spells_it() {
        let guard = Guard::or(
            Guard::and(Guard::variable("a"), Guard::variable("b")),
            Guard::call("IsKimHere", vec![]),
        );
        assert_eq!(
            guard.to_string(),
            "((Variable[\"a\"] and Variable[\"b\"]) or IsKimHere())",
        );
    }

    /// A settled variable becomes the value it settled at, and the rest of the guard stands.
    #[test]
    fn substituting_replaces_only_what_is_settled() {
        let guard = Guard::or(
            Guard::and(Guard::variable("a"), Guard::variable("b")),
            Guard::call("IsKimHere", vec![]),
        );

        let settled = guard
            .substituting(|name| (name == "a").then(|| GuardValue::from_number(1.0)))
            .expect("a was settled");

        assert_eq!(
            settled.to_string(),
            "((1 and Variable[\"b\"]) or IsKimHere())"
        );
    }

    /// A guard mentioning nothing settled is left alone, and says so.
    #[test]
    fn substituting_nothing_answers_nothing() {
        let guard = Guard::and(Guard::variable("a"), Guard::variable("b"));
        assert!(guard.substituting(|_| None).is_none());
    }

    /// Substituting keeps a guard's ANSWER, which is the whole point: the value put in is the
    /// value the variable was going to read as.
    #[test]
    fn a_substituted_guard_answers_as_the_world_would_have() {
        struct Holds;
        impl IGuardContext for Holds {
            fn get_variable(&self, name: &str) -> GuardValue {
                match name {
                    "a" => GuardValue::from_number(3.0),
                    _ => GuardValue::from_boolean(true),
                }
            }
            fn query(&self, _: &str, _: &[GuardValue]) -> GuardValue {
                GuardValue::from_boolean(true)
            }
        }

        let guard = Guard::comparison(
            "<",
            Guard::variable("a"),
            Guard::literal(GuardValue::from_number(5.0)),
        );
        let settled = guard
            .substituting(|name| (name == "a").then(|| GuardValue::from_number(3.0)))
            .expect("a was settled");

        assert_eq!(guard.test(&Holds), settled.test(&Holds));
        assert_eq!(settled.test(&Holds), Ternary::True);
    }
}
