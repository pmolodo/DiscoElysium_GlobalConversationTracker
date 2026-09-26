// SPDX-License-Identifier: MIT

pub mod node;
pub mod settled;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::OnceLock;

use crate::core::action::ActionCondition;
use crate::core::state::StateSymbols;
use crate::core::types::{DialogueCheckKind, DialogueNodeId, SeenState};
use crate::graph::node::LookAheadNode;

/// What a graph needs to know about its world before a search: the facts a search takes as
/// constant but the graph's prices and actions depend on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fitting {
    /// Whether the world is in hardcore mode, where it matters to a price.
    pub hardcore: bool,
    /// The thoughts the world holds fixed, among those an action is conditioned on.
    pub fixed: BTreeSet<String>,
    /// The passive checks this group can change the outcome of, and so cannot be answered
    /// from the world alone - see [`crate::core::skill_movers`].
    ///
    /// TWO WAYS IN, and both need the world, which is why the set is decided here rather than
    /// in [`LookAheadGraph::fit`]: the group's own damage can cross a check's margin, and the
    /// group can take off or put on a garment that moves the skill the check compares.
    pub unsettled: HashSet<DialogueNodeId>,
    /// The variables the world holds set, among those a settled conditional write tests - see
    /// [`crate::core::action::DialogueAction::settled_by_world`].
    pub set_variables: BTreeSet<String>,
    /// What a slot holds at every read of it, for the slots whose writes settle it - see
    /// [`settled`] for which slots those are and [`Fitting::read`] for how a world settles them.
    pub settled: BTreeMap<usize, i32>,
}

impl Fitting {
    /// What `world` says about everything `graph` depends on, and nothing else - so two worlds
    /// that differ only in what the graph never asks give the same fitting.
    pub fn read(graph: &LookAheadGraph, world: &dyn crate::world::ILookAheadWorld) -> Self {
        let fixed: BTreeSet<String> = graph
            .thoughts_deciding_actions()
            .into_iter()
            .filter(|thought| crate::core::thought_effects::is_fixed(world, thought))
            .map(str::to_string)
            .collect();
        let mut unsettled = graph.checks_damage_can_flip(world, &fixed);
        unsettled.extend(graph.checks_clothing_can_flip(world));

        Self {
            hardcore: graph.prices_by_mode() && crate::core::game_mode::is_hardcore(world),
            unsettled,
            fixed,
            set_variables: graph
                .variables_deciding_actions()
                .into_iter()
                .filter(|name| {
                    graph.symbols().variable_ref(name).is_some_and(|variable| {
                        crate::core::state::slot_value_of(&world.get_variable(variable)) != 0
                    })
                })
                .map(str::to_string)
                .collect(),
            settled: Self::settled_by(graph, world),
        }
    }

    /// What each candidate slot holds at every read of it, for this world.
    ///
    /// A candidate's writes lie on every route to its reads - see [`settled`] - so the only
    /// question left is whether they FIRE, and that is the world's to answer:
    ///
    /// - AN ACTION CAN BE TURNED OFF for a world, and one that is off writes nothing.
    /// - A CHECK'S SUCCESS ACTIONS FIRE ON SUCCESS. `world::passive_outcome` says which way a
    ///   settled check went, and refuses to say for one the group can still move - and a check
    ///   that FAILS leaves the slot holding whatever the world brought, which is not this value.
    ///
    /// EVERY WRITE MUST FIRE AND ALL MUST AGREE. One that does not fire leaves a route to the
    /// read that wrote nothing, and the value there is the world's rather than the literal -
    /// so the slot is not settled and is left alone.
    fn settled_by(
        graph: &LookAheadGraph,
        world: &dyn crate::world::ILookAheadWorld,
    ) -> BTreeMap<usize, i32> {
        let mut settled = BTreeMap::new();
        for candidate in graph.settled_candidates() {
            let mut agreed: Option<i32> = None;
            let fires = candidate.written_at.iter().all(|id| {
                let Some(node) = graph.get(*id) else {
                    return false;
                };
                // A CHECK'S SUCCESS ACTIONS FIRE ON SUCCESS, and the world is what says whether
                // it succeeds. Unknown is the common answer rather than the exotic one - the
                // snapshot carries an outcome only for the checks the plugin evaluated - and
                // Unknown means the slot is not settled.
                //
                // ASKED ONLY OF THE KINDS AN OUTCOME GATES. A `KimSwitch` is entered
                // unconditionally where it is `boolean_only`, and a `Test` is entered and goes
                // nowhere - neither has a roll or a validator, so neither has an outcome for
                // the world to state. Asking anyway got Unknown back and rejected the
                // candidate, which cost a slot that could have been predetermined. One entry
                // in the shipped database reaches this: 29:493, a Kim switch. See de-m11s.2.
                if !matches!(
                    node.kind,
                    DialogueCheckKind::None
                        | DialogueCheckKind::KimSwitch
                        | DialogueCheckKind::Test
                ) && crate::world::passive_outcome(node, world)
                    != crate::core::types::Ternary::True
                {
                    return false;
                }
                node.actions.iter().any(|action| {
                    if !action.is_enabled() || usize::try_from(action.slot()) != Ok(candidate.slot)
                    {
                        return false;
                    }
                    match agreed {
                        Some(value) => value == action.value(),
                        None => {
                            agreed = Some(action.value());
                            true
                        }
                    }
                })
            });
            if let (true, Some(value)) = (fires, agreed) {
                settled.insert(candidate.slot, value);
            }
        }
        settled
    }
}

/// The dialogue entries the look-ahead can walk, indexed by id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadGraph {
    nodes: HashMap<DialogueNodeId, LookAheadNode>,
    /// The order [`Self::nodes`] yields entries in: by conversation, then by entry.
    ///
    /// ## Why a graph carries a list of its own keys
    ///
    /// BECAUSE A HASH MAP'S ORDER IS A FACT ABOUT THE PROCESS, and several things built by
    /// sweeping this graph inherit it - the layout's threshold narrowing, the SCC
    /// decomposition, the order guards are compiled and therefore the order diagram nodes
    /// are created. None of that changes an ANSWER, and measuring says so: three processes
    /// asked the same question of conversation 1030 returned the same verdict, the same
    /// `by` and the same `asked` every time. What moved was the work done to get there -
    /// 126,106, 126,588 and 126,148 diagram nodes - and, once, how deep the recursion went:
    /// the same code overflowed the main thread's stack in one process and not the next.
    ///
    /// So the cost of leaving it was a search whose cost is not reproducible, a `nodes`
    /// column that cannot be compared between runs, and a stack depth that varies for no
    /// reason anybody can see. See de-12wr.3 and `crates/gct-measure/examples/nodes_repeat.rs`, which is
    /// the experiment.
    ///
    /// A SORTED KEY LIST RATHER THAN A `BTreeMap`, because `get` is the hot operation here
    /// and iteration is not: the sweeps above happen once per group, and lookups happen
    /// per link followed. This keeps both at what they were.
    order: Vec<DialogueNodeId>,
    symbols: StateSymbols,
    /// The answer [`Self::inert_slots`] gives, worked out the first time anything asks.
    ///
    /// NOT SERIALISED, because it is derived from the rest of this struct: a graph that
    /// arrives over the wire works it out again on demand rather than carrying it.
    #[serde(skip)]
    inert: OnceLock<HashSet<usize>>,
    /// The answer [`Self::settled_candidates`] gives, worked out the first time anything asks.
    ///
    /// NOT SERIALISED, for the reason `inert` is not: it is derived from the rest of this
    /// struct, and a graph that arrives over the wire works it out again on demand.
    #[serde(skip)]
    candidates: OnceLock<settled::Candidates>,
}

impl LookAheadGraph {
    /// Builds a graph, assigning the once slots and then freezing the symbol table.
    ///
    /// Interning happens HERE and nowhere later. A search reads slots by index and never
    /// creates one, which is what lets the symbol table be shared as `&StateSymbols`
    /// throughout the search, keeps the state vector's width fixed before the first
    /// state exists, and is a precondition for any symbolic encoding: a decision diagram
    /// has to fix its variable order up front, and cannot if a new variable can appear
    /// halfway through.
    pub fn new(nodes: Vec<LookAheadNode>, mut symbols: StateSymbols) -> Result<Self, String> {
        let mut map = HashMap::new();
        for mut node in nodes {
            if map.contains_key(&node.id) {
                return Err(format!("Duplicate dialogue entry {}", node.id));
            }
            if node.needs_once_slot() {
                node.once_slot = symbols.once(node.id) as i32;
            }
            map.insert(node.id, node);
        }

        // SORTED ONCE, HERE, so every sweep of the graph sees the same order in every
        // process. See the field for what a hash map's order was costing.
        let mut order: Vec<DialogueNodeId> = map.keys().copied().collect();
        order.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));

        let mut choices = HashSet::new();
        for parent in map.values() {
            let mut pending = parent.links.clone();
            let mut visited = HashSet::new();
            let mut offered = HashSet::new();
            while let Some(id) = pending.pop() {
                if !visited.insert(id) {
                    continue;
                }
                let Some(node) = map.get(&id) else { continue };
                if node.is_group {
                    pending.extend(node.links.iter().copied());
                } else if node.player {
                    offered.insert(id);
                }
            }
            if offered.len() > 1 {
                choices.extend(offered);
            }
        }
        for id in choices {
            map.get_mut(&id).expect("offered entry exists").choice = true;
        }

        // THE VARIABLES THE GROUP READS, fixed here with the slots and for the same reason: a
        // search reads them and never adds one. Every plain slot, which the seed reads from
        // the world, and every variable a guard names, a flag a `FlagSet` names by a literal
        // included. Nothing else can be read - see `VariableRef`.
        let mut variables: Vec<String> = (0..symbols.count())
            .filter_map(|slot| symbols.name_of(slot))
            .filter(|name| crate::core::state::names_a_variable(name))
            .map(str::to_string)
            .collect();
        for node in map.values() {
            for part in node.guards_read().flat_map(|guard| guard.nodes()) {
                match part.expression() {
                    crate::core::guard::GuardExpression::Variable(name) => {
                        variables.push(name.to_string());
                    }
                    crate::core::guard::GuardExpression::Call(function, arguments)
                        if crate::world::flag_query(function).is_some() =>
                    {
                        if let Some(crate::core::guard::GuardExpression::Literal(value)) =
                            arguments.only().map(|only| only.expression())
                            && value.kind() == crate::core::guard_value::GuardValueKind::Text
                        {
                            variables.push(value.text().to_string());
                        }
                    }
                    // A weather question reads a fixed variable - see `core::scene`.
                    crate::core::guard::GuardExpression::Call(function, _)
                        if crate::core::scene::variable_read_by(function).is_some() =>
                    {
                        variables.extend(
                            crate::core::scene::variable_read_by(function).map(str::to_string),
                        );
                    }
                    // A substance question reads the count variable its literal names - see
                    // `core::substance`.
                    crate::core::guard::GuardExpression::Call(function, arguments)
                        if crate::core::substance::owns(function) =>
                    {
                        if let Some(crate::core::guard::GuardExpression::Literal(value)) =
                            arguments.only().map(|only| only.expression())
                            && value.kind() == crate::core::guard_value::GuardValueKind::Text
                        {
                            variables.extend(crate::core::substance::variable_read_by(
                                function,
                                value.text(),
                            ));
                        }
                    }
                    // A reputation question reads a WHOLE RANGE of reputations rather than
                    // the one it names, so the group declares all of them - see
                    // `core::reputation`. Named by the query rather than by its argument for
                    // that reason: the argument is what the answer is COMPARED to.
                    crate::core::guard::GuardExpression::Call(function, _) => {
                        variables.extend(crate::core::reputation::variables_read_by(function));
                    }
                    _ => {}
                }
            }
            // A variable a settled condition reads, which the world is asked for like any other.
            variables.extend(
                node.all_actions()
                    .filter_map(|action| action.unset_variable())
                    .map(str::to_string),
            );
        }
        symbols.declare_variables(variables);

        Ok(Self {
            nodes: map,
            order,
            symbols,
            inert: OnceLock::new(),
            candidates: OnceLock::new(),
        })
    }

    /// The slots no write of can reach any read of, so nothing a search does to them can
    /// change what it can reach.
    ///
    /// ## What it is for
    ///
    /// A slot like this holds ONE NUMBER for the whole of any search - the one the world
    /// arrived with - so it need not be carried at all: see
    /// [`crate::symbolic::data_layout::DataLayout::dropping_slots_written_too_late`], which
    /// takes the answer and drops them, and `GuardCompiler`, where a variable with no slot
    /// is the world's number everywhere. It takes 10 slots off 640's layout and 10 off
    /// 761's.
    ///
    /// ## A FACT ABOUT THE DIALOGUE, WHICH IS WHY IT LIVES HERE AND IS KEPT
    ///
    /// It reads the links, the actions' KINDS and the guards - never the world. `fit` can
    /// turn an action off for a world, and an action turned off writes nothing, so a fitted
    /// graph can only have FEWER writes reaching reads than this says: the answer stays
    /// sound and is not worth recomputing per world. Nor per layout: `entered_at` narrows
    /// which entries a search can walk, and narrowing the start set can only strand more
    /// writes, never fewer.
    ///
    /// SO IT IS WORKED OUT ONCE AND KEPT, lazily, for the life of the graph - which
    /// `workspace::Workspace` holds across every request sharing its key, so consecutive
    /// menus in a conversation pay for it once between them. Measured before it was kept:
    /// running this per layout build cost 761 an extra 43 ms a menu and 368 an extra 27,
    /// which is several times what dropping the slots gives back.
    pub fn inert_slots(&self) -> &HashSet<usize> {
        self.inert.get_or_init(|| self.slots_no_write_reaches())
    }

    /// Takes an answer somebody else already has, instead of working one out.
    ///
    /// WHAT A KEPT ANSWER IS FOR - see [`crate::index::facts`], which reads one off disk and
    /// hands it over here. Working it out costs 14 ms on a group of four thousand entries and
    /// reading it back costs 0.10, and it is the same answer every time because it depends on
    /// the dialogue alone.
    ///
    /// IGNORED IF THE ANSWER IS ALREADY KNOWN, because it is the same answer: whoever asked
    /// first either read this one or worked out one equal to it. Nothing needs to hear about a
    /// race that cannot change a value.
    pub fn remember_inert_slots(&self, slots: HashSet<usize>) {
        let _ = self.inert.set(slots);
    }

    /// The slots whose writes lie on every route to their reads - see [`settled`].
    ///
    /// A FACT ABOUT THE LINKS, so it is worked out once and kept, like [`Self::inert_slots`]:
    /// the dominator tree it rests on is the expensive half, and which way a world sends a
    /// check is the cheap half, asked where the graph is fitted.
    pub fn settled_candidates(&self) -> &settled::Candidates {
        self.candidates.get_or_init(|| settled::candidates(self))
    }

    /// Takes candidates somebody else already has - see [`Self::remember_inert_slots`].
    pub fn remember_settled_candidates(&self, found: settled::Candidates) {
        let _ = self.candidates.set(found);
    }

    /// Works [`Self::inert_slots`] out.
    ///
    /// ## Reaching definitions, not a search per slot
    ///
    /// What is wanted is the set of slots some path may have written before arriving at an
    /// entry, which is the textbook forward dataflow: what leaves an entry is what arrived
    /// plus what it writes, and what arrives is the union over the entries linking to it. One
    /// fixed point over bitsets answers it for every slot at once, where a walk per slot would
    /// cross the group once per slot.
    ///
    /// ## What it will not touch
    ///
    /// AN ENTRY'S GUARD IS TESTED BEFORE ITS OWN ACTIONS APPLY - see
    /// [`crate::symbolic::reachability`] - so an entry writing what it also reads has not
    /// written it by the time it reads it. That falls out of the dataflow rather than being a
    /// case: what ARRIVES is what predecessors left, and an entry reaches itself only around a
    /// loop, which the fixed point follows like any other path.
    ///
    /// THE ENGINE'S OWN SLOTS, which it writes where no action does: `once:`, `seen:` and a
    /// rolled check's pass and fail flags. Their writes are in no entry's actions, so an
    /// analysis of actions alone would call them unwritten and strand every one.
    ///
    /// A SLOT NO ACTION WRITES, which is left to the rules that already cover it rather than
    /// dropped here on the grounds that nothing wrote it.
    fn slots_no_write_reaches(&self) -> HashSet<usize> {
        use crate::symbolic::data_layout::DataLayout;

        let words = self.symbols.count().div_ceil(64);
        let entries: Vec<&LookAheadNode> = self.nodes().collect();
        if words == 0 || entries.is_empty() {
            return HashSet::new();
        }
        let at_of: HashMap<DialogueNodeId, usize> = entries
            .iter()
            .enumerate()
            .map(|(at, node)| (node.id, at))
            .collect();

        let mut writes = vec![0u64; entries.len() * words];
        let mut the_engine_writes = vec![0u64; words];
        let mut an_action_writes = vec![0u64; words];
        for (at, node) in entries.iter().enumerate() {
            for slot in [
                node.once_slot,
                node.seen_slot,
                node.flag_slot,
                node.failed_flag_slot,
            ] {
                raise(&mut the_engine_writes, slot);
            }
            for action in node.all_actions() {
                if action.writes_slot() {
                    raise(&mut writes[at * words..(at + 1) * words], action.slot());
                    raise(&mut an_action_writes, action.slot());
                }
            }
        }

        let arriving = reaching_writes(&entries, &at_of, &writes, words);

        // ONE SET, REUSED. The reading rules are `DataLayout`'s and are asked for one entry at
        // a time, so the buffer they fill is cleared and handed back rather than allocated
        // 4,000 times - and an entry that can read nothing at all is not asked.
        let mut a_write_arrives = vec![0u64; words];
        let mut names = HashSet::new();
        for (at, node) in entries.iter().enumerate() {
            if DataLayout::reads_no_slot(node) {
                continue;
            }
            names.clear();
            DataLayout::read_by_node_into(node, &self.symbols, &mut names);
            let arrived = &arriving[at * words..(at + 1) * words];
            for name in &names {
                if let Some(slot) = self.symbols.find(name)
                    && holds(arrived, slot)
                {
                    raise(&mut a_write_arrives, slot as i32);
                }
            }
        }

        (0..self.symbols.count())
            .filter(|slot| {
                holds(&an_action_writes, *slot)
                    && !holds(&the_engine_writes, *slot)
                    && !holds(&a_write_arrives, *slot)
            })
            .collect()
    }

    pub fn symbols(&self) -> &StateSymbols {
        &self.symbols
    }

    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    /// Every entry, by conversation and then by entry id.
    ///
    /// THE ORDER IS PART OF THE CONTRACT, not an accident of the storage - see
    /// [`Self::order`]. A caller that sweeps this to build something the search then follows
    /// can rely on two processes building the same thing.
    pub fn nodes(&self) -> impl Iterator<Item = &LookAheadNode> {
        self.order.iter().filter_map(|id| self.nodes.get(id))
    }

    pub fn get(&self, id: DialogueNodeId) -> Option<&LookAheadNode> {
        self.nodes.get(&id)
    }

    /// Whether any entry's price depends on the game mode.
    pub fn prices_by_mode(&self) -> bool {
        self.nodes().any(|node| node.price_scale.is_some())
    }

    /// Every thought whose being fixed decides whether an action fires, sorted.
    pub fn thoughts_deciding_actions(&self) -> BTreeSet<&str> {
        self.nodes()
            .flat_map(|node| node.all_actions())
            .filter_map(|action| action.fixed_thought())
            .collect()
    }

    /// Every item the group takes away, where it also holds a passive check whose skill that
    /// could move - see [`crate::core::skill_movers`]. Empty otherwise.
    pub fn items_lost_near_passive_checks(&self) -> BTreeSet<&str> {
        if !self
            .nodes()
            .any(|node| node.kind == DialogueCheckKind::Passive)
        {
            return BTreeSet::new();
        }
        self.nodes()
            .flat_map(|node| &node.skill_moves.lost_items)
            .map(String::as_str)
            .collect()
    }

    /// The skills the group deals damage or healing to, where it also holds a passive check that
    /// could flip. Empty otherwise.
    pub fn skills_damage_moves_near_passive_checks(&self) -> BTreeSet<&str> {
        if !self
            .nodes()
            .any(|node| node.kind == DialogueCheckKind::Passive)
        {
            return BTreeSet::new();
        }
        self.nodes()
            .flat_map(|node| &node.skill_moves.damage)
            .map(|damage| damage.skill.as_str())
            .collect()
    }

    /// Whether the group both deals damage or healing and holds a passive check it could flip.
    fn damages_near_passive_checks(&self) -> bool {
        !self.skills_damage_moves_near_passive_checks().is_empty()
    }

    /// Every counter the group raises and can raise only a bounded number of times, with what its
    /// raises add up to.
    ///
    /// ## Why it matters
    ///
    /// The counter cap exists to keep a counter finite when a dialogue loop can raise it without
    /// end. A counter that cannot be raised that way needs no cap: it can climb at most by the
    /// sum of its raises. So neither engine saturates one of these - the explicit engine takes
    /// its caps from here ([`crate::core::action::CounterCaps::for_graph`]), and the symbolic
    /// layout holds each as a distance from where the search started
    /// ([`crate::symbolic::data_layout::DataLayout`]). Saturating one clips a value the game
    /// keeps: a reputation at 28 read as 16 once something raises it.
    ///
    /// ## What disqualifies a counter
    ///
    /// - A NEGATIVE RAISE or an ASSIGNMENT. A write that can lower the value or set it outright
    ///   leaves the sum of the raises as no bound on it, and a distance cannot express it.
    /// - A RAISE THAT CAN FIRE TWICE: one on an entry that lies on a dialogue loop, unless it is
    ///   marked `once` - which it fires at most once whatever the links do.
    /// - A SLOT THE SEED FILLS FROM SOMEWHERE ELSE - the inventory, the cabinet, damage, what is
    ///   worn, the seen record - rather than from the variable's own value. None is a counter in
    ///   the content, and a distance needs the start to be the variable's value.
    pub fn counters_that_cannot_loop(&self) -> HashMap<usize, u32> {
        #[derive(Default)]
        struct Raises {
            sum: u32,
            barred: bool,
        }

        let order = crate::symbolic::order::IterationOrder::of(self);
        let cyclic = crate::symbolic::data_layout::DataLayout::entries_on_a_cycle(self, &order);
        let symbols = self.symbols();

        let mut raises: HashMap<usize, Raises> = HashMap::new();
        for node in self.nodes() {
            let repeatable = cyclic.contains(&node.id);
            for action in node.all_actions() {
                let Ok(slot) = usize::try_from(action.slot()) else {
                    continue;
                };
                if slot >= symbols.count() {
                    continue;
                }
                use crate::core::action::DialogueActionKind;
                match action.kind() {
                    DialogueActionKind::Increment => {
                        let found = raises.entry(slot).or_default();
                        let amount = action.value();
                        if amount < 0 || (repeatable && !action.once()) {
                            found.barred = true;
                        }
                        found.sum = found.sum.saturating_add(amount.max(0) as u32);
                    }
                    DialogueActionKind::Assign
                    | DialogueActionKind::AssignClock
                    | DialogueActionKind::AssignUnless => {
                        raises.entry(slot).or_default().barred = true
                    }
                    _ => {}
                }
            }
        }

        raises
            .into_iter()
            .filter(|(slot, found)| {
                !found.barred
                    && found.sum > 0
                    && symbols
                        .name_of(*slot)
                        .is_some_and(crate::core::state::names_a_variable)
            })
            .map(|(slot, found)| (slot, found.sum))
            .collect()
    }

    /// The most minutes a walk over this group can pass, or `None` where it can pass any.
    ///
    /// ## What it is for
    ///
    /// Time passing is what ends a substance - `SunshineClock.ApplyTimeForwardEffects` bakes
    /// every running one down by the minutes passed and strips its buffs at zero - so the
    /// question "can this walk outlast what the player is on" is this number against the
    /// minutes the world says are left. See de-m11s.3.3.
    ///
    /// ## A PassTime on a cycle has no bound
    ///
    /// A walk that can come back round to a `PassTime` can take it again, and again, so such a
    /// group can pass any amount of time and outlast anything. That is `None` rather than a
    /// large number: a bound that is merely big invites a comparison that treats it as small
    /// enough, and there is no honest large number to pick.
    ///
    /// ## And otherwise it is a longest path
    ///
    /// With every `PassTime` off the cycles, the answer is the most of them a walk can string
    /// together, which is a longest path over the condensation - and the components are
    /// already numbered in topological order, since a link that leaves one always climbs (see
    /// [`crate::symbolic::order::IterationOrder`]). So one pass in component order settles it,
    /// and a component that is a real cycle contributes nothing, having no `PassTime` in it.
    ///
    /// EVERY ACTION A NODE RUNS is counted, its failure actions included - though none of
    /// those is a `PassTime` today, since they are thought effects rather than scripts. Asked
    /// of the whole set anyway: an under-estimate here is the one direction this must not be
    /// wrong in, because it models a substance as outlasting a walk that ends it.
    pub fn minutes_passable(&self) -> Option<i32> {
        use crate::core::action::DialogueActionKind;

        let passes = |node: &LookAheadNode| {
            node.all_actions()
                .filter(|action| action.kind() == DialogueActionKind::PassTime)
                .count() as i32
        };

        let order = crate::symbolic::order::IterationOrder::of(self);
        let cyclic = crate::symbolic::data_layout::DataLayout::entries_on_a_cycle(self, &order);

        let mut weight: HashMap<u32, i32> = HashMap::new();
        for node in self.nodes() {
            let taken = passes(node);
            if taken == 0 {
                continue;
            }
            if cyclic.contains(&node.id) {
                return None;
            }
            let Some(component) = order.component_of(node.id) else {
                continue;
            };
            *weight.entry(component).or_default() += taken;
        }

        if weight.is_empty() {
            return Some(0);
        }

        // Every component's own contribution first, so a source's answer is right before
        // anything reads it.
        let mut best: HashMap<u32, i32> = HashMap::new();
        for node in self.nodes() {
            if let Some(component) = order.component_of(node.id) {
                best.entry(component)
                    .or_insert_with(|| weight.get(&component).copied().unwrap_or(0));
            }
        }

        // THE LINKS, GATHERED BY COMPONENT, so the pass below walks the condensation rather
        // than the graph and cannot be led round a cycle.
        let mut onward: HashMap<u32, HashSet<u32>> = HashMap::new();
        for node in self.nodes() {
            let Some(from) = order.component_of(node.id) else {
                continue;
            };
            for link in &node.links {
                if let Some(to) = order.component_of(*link)
                    && to != from
                {
                    onward.entry(from).or_default().insert(to);
                }
            }
        }

        let mut components: Vec<u32> = best.keys().copied().collect();
        components.sort_unstable();
        for from in components {
            let reached = best.get(&from).copied().unwrap_or(0);
            let Some(nexts) = onward.get(&from) else {
                continue;
            };
            for to in nexts {
                let own = weight.get(to).copied().unwrap_or(0);
                let entry = best.entry(*to).or_insert(own);
                *entry = (*entry).max(reached + own);
            }
        }

        Some(
            best.values().copied().max().unwrap_or(0)
                * crate::core::clock::ClockTime::PASS_TIME_MINUTES,
        )
    }

    /// The passive checks a garment this group takes off or puts on can flip.
    ///
    /// ## What changed, and why it was worth changing
    ///
    /// A group that can unclothe the player used to unsettle EVERY passive check in it, because
    /// nothing knew which skill a garment moves. It does now - [`crate::core::garment`] holds
    /// the fifty-five items that move one - so taking off a hat that moves Perception leaves the
    /// Logic checks settled, and their entries keep an answer the world can give.
    ///
    /// ## What is asked of which
    ///
    /// TAKEN OFF COUNTS ONLY WHERE IT IS ON. An item the group deletes that the player is not
    /// wearing has no bonus to take away. PUT ON COUNTS ALWAYS: the group gains the garment, so
    /// what the world is wearing now does not decide whether it can.
    ///
    /// A CHECK IS ONLY UNSETTLED WHERE THE GARMENT CAN REACH ITS MARGIN, which is the same
    /// test damage gets: most garments move a skill by one, so a check passing by three keeps
    /// the world's answer however many hats come off. See [`crate::core::garment::Reach`].
    ///
    /// A CHECK WITH NO STATED SKILL IS UNSETTLED. The world states one for every passive check
    /// it can - see [`crate::world::ILookAheadWorld::check_margin`] - and where it states none,
    /// nothing here can tell whether the garment reaches it. More markers than earned, never
    /// fewer, which is the direction this whole mechanism errs in.
    pub fn checks_clothing_can_flip(
        &self,
        world: &dyn crate::world::ILookAheadWorld,
    ) -> HashSet<DialogueNodeId> {
        use crate::core::garment::Reach;

        // WHICH GARMENTS THE GROUP CAN MOVE, and in which direction. Taken off counts only
        // where the world has it on - an item the group deletes that is not worn has no bonus
        // to take away - and put on counts always, since the group gains it.
        let lost = self.items_lost_near_passive_checks();
        let mut worn_and_lost: Vec<String> = Vec::new();
        if !lost.is_empty() {
            for slot in crate::core::equipment::SLOTS {
                let worn = world.item_in_slot(slot).unwrap_or_default();
                if lost.contains(worn.as_str()) {
                    worn_and_lost.push(worn);
                }
            }
        }
        let gained: Vec<&String> = self
            .nodes()
            .flat_map(|node| node.skill_moves.puts_on.iter())
            .collect();

        if worn_and_lost.is_empty() && gained.is_empty() {
            return HashSet::new();
        }

        // HOW FAR EACH SKILL CAN MOVE, worked out once per skill a check asks about rather
        // than per garment: `Reach` sums both directions, and `can_flip` is the same question
        // `DamageReach` answers for a blow.
        let reach_of = |skill: &str| {
            let mut reach = Reach::default();
            for item in &worn_and_lost {
                reach.taking_off(item, skill);
            }
            for item in &gained {
                reach.putting_on(item, skill);
            }
            reach
        };

        self.nodes()
            .filter(|node| node.kind == DialogueCheckKind::Passive)
            .filter(|node| match world.check_margin(node.id) {
                Some((skill, margin)) => reach_of(&skill).can_flip(margin),
                None => true,
            })
            .map(|node| node.id)
            .collect()
    }

    /// The passive checks whose margin in `world` the group's damage or healing can cross, with
    /// the thoughts in `fixed` held fixed - see [`crate::core::skill_movers`].
    pub fn checks_damage_can_flip(
        &self,
        world: &dyn crate::world::ILookAheadWorld,
        fixed: &BTreeSet<String>,
    ) -> HashSet<DialogueNodeId> {
        if !self.damages_near_passive_checks() {
            return HashSet::new();
        }
        let order = crate::symbolic::order::IterationOrder::of(self);
        let cyclic = crate::symbolic::data_layout::DataLayout::entries_on_a_cycle(self, &order);
        let mut reach: HashMap<&str, crate::core::skill_movers::DamageReach> = HashMap::new();
        for node in self.nodes() {
            let fires = |damage: &&crate::core::skill_movers::DamageMove| {
                damage
                    .fixed_thought
                    .as_ref()
                    .is_none_or(|thought| fixed.contains(thought))
            };
            for damage in node.skill_moves.damage.iter().filter(fires) {
                let repeatable = !damage.once && cyclic.contains(&node.id);
                reach
                    .entry(damage.skill.as_str())
                    .or_default()
                    .add(damage, repeatable);
            }
        }
        self.nodes()
            .filter(|node| node.kind == DialogueCheckKind::Passive)
            .filter(|node| {
                world.check_margin(node.id).is_some_and(|(skill, margin)| {
                    // The damage a skill carries is a negative damage value.
                    let damaged = world
                        .initial_damage(&skill)
                        .map(|value| (-value).max(0.0) as i64);
                    reach
                        .get(skill.as_str())
                        .is_some_and(|reach| reach.can_flip(margin, damaged))
                })
            })
            .map(|node| node.id)
            .collect()
    }

    /// Every variable whose being unset decides whether a settled conditional write fires,
    /// sorted.
    pub fn variables_deciding_actions(&self) -> BTreeSet<&str> {
        self.nodes()
            .flat_map(|node| node.all_actions())
            .filter_map(|action| action.unset_variable())
            .collect()
    }

    /// Whether anything in the graph depends on a [`Fitting`].
    ///
    /// EVERY CLAUSE IS SOMETHING `fit` DECIDES, and a caller that skips fitting on a `false`
    /// here - `workspace::fitting_of` does - skips all of them. So a thing fitting decides and
    /// this does not ask about is a thing silently left undone on that path and done on every
    /// other: the candidate slots are the newest of them, and a group whose prices, thoughts,
    /// variables and checks all need nothing would otherwise carry every settled slot in its
    /// layout for want of a clause here.
    pub fn needs_fitting(&self) -> bool {
        self.prices_by_mode()
            || !self.thoughts_deciding_actions().is_empty()
            || !self.variables_deciding_actions().is_empty()
            || !self.items_lost_near_passive_checks().is_empty()
            || self.damages_near_passive_checks()
            || !self.settled_candidates().is_empty()
    }

    /// Fits the graph to a world: every price to the game mode - see [`crate::core::price`] -
    /// every conditional action on or off by its thought - see
    /// [`crate::core::thought_effects`] - and every passive check settled or not by whether the
    /// group can change what is worn - see [`crate::core::skill_movers`].
    ///
    /// A graph is built as for a normal-mode world holding no thought fixed and wearing nothing
    /// the group takes. Fitting starts from each node's own data every time, so a graph can be
    /// fitted to one world and then another.
    pub fn fit(&mut self, fitting: &Fitting) {
        for node in self.nodes.values_mut() {
            node.check_settled =
                node.kind != DialogueCheckKind::Passive || !fitting.unsettled.contains(&node.id);
            node.cost =
                crate::core::price::price(node.click_cost, node.price_scale, fitting.hardcore);
            for action in node.actions.iter_mut().chain(&mut node.failure_actions) {
                action.fit(|condition| match condition {
                    ActionCondition::ThoughtFixed(thought) => fitting.fixed.contains(thought),
                    ActionCondition::VariableUnset(variable) => {
                        !fitting.set_variables.contains(variable)
                    }
                });
            }
        }
        self.settle_guards(fitting);
    }

    /// Puts a settled slot's value into every guard that reads it - see [`settled`].
    ///
    /// ## Why it happens HERE and not in the compiler
    ///
    /// Because everything downstream then agrees without being told. The layout's `read_by`
    /// and the guard compiler read the same guards, so a variable replaced by its value is
    /// invisible to both in the same way: the slot loses its last reader and
    /// [`crate::symbolic::data_layout::DataLayout::keeping_only_read`] drops it by the rule it
    /// already has. Telling the compiler a slot's value while the layout still carried it -
    /// or worse, the other way round - is two sources of one truth.
    ///
    /// ## What is restored first, and why there is a source guard at all
    ///
    /// A GRAPH CAN BE FITTED TO ONE WORLD AND THEN ANOTHER, which is what the rest of `fit`
    /// rests on: every derived field is recomputed from data that is never overwritten. A
    /// guard is the first derived thing that replaces its own source, so the source is kept
    /// beside it and put back before this runs again.
    fn settle_guards(&mut self, fitting: &Fitting) {
        for node in self.nodes.values_mut() {
            if let Some(source) = node.guard_before_fitting.take() {
                node.guard = source;
            }
        }
        if fitting.settled.is_empty() {
            return;
        }

        let settled: HashMap<&str, i32> = fitting
            .settled
            .iter()
            .filter_map(|(slot, value)| Some((self.symbols.name_of(*slot)?, *value)))
            .collect();
        for node in self.nodes.values_mut() {
            if let Some(put) = node.guard.substituting(|name| {
                settled
                    .get(name)
                    .map(|value| crate::core::guard_value::GuardValue::from_number(*value as f64))
            }) {
                node.guard_before_fitting = Some(std::mem::replace(&mut node.guard, put));
            }
        }
    }

    /// The best seen state class carried by anything LINK-REACHABLE beyond `start`.
    ///
    /// GUARDS ARE IGNORED, which is the whole point: this is a few thousand pointer-follows
    /// against a search that is thousands of diagram operations, and it is run before every
    /// one of them. So it OVER-APPROXIMATES - a class it names may sit behind a guard
    /// nothing can open - and the two answers mean different things. `None`, or a class no
    /// better than a baseline, is DEFINITE: no walk of the links reaches anything better,
    /// so no search can either, and the question is settled without building a state. A
    /// class it does name is a maybe, and what the searches are then sent to establish.
    ///
    /// TWO CALLERS, ONE WALK, and they want the same fact for opposite reasons.
    /// `bridge::class_worth_hunting` asks whether anything outranks a baseline, and refuses to
    /// search when nothing does. `symbolic::answer` asks which class to hunt, because a
    /// search that stops at the best class PRESENT has found the best there is, while one
    /// stopping at the first entry that merely beats "seen" may have walked past a better
    /// one - and it was two different walks, answering these two questions differently,
    /// that made that gap possible.
    ///
    /// THE START IS A RESULT LIKE ANY OTHER, and is scored before a single link is walked.
    /// It reads as a wasted comparison for an ordinary option, where the baseline IS the
    /// start's own class and nothing can outrank itself - but for one outcome of a rolled
    /// check the baseline is where that OUTCOME LANDS, which sits below the check's own
    /// class whenever the outcome opens something already read. There the check entry
    /// genuinely outranks the baseline, and a walk that skipped it would refuse a search
    /// that had its answer in hand before it started. One rule, no special case, and the
    /// cost is one comparison in the case where it cannot fire.
    ///
    /// Groups are skipped, as everywhere else - the game never writes their SimStatus, so
    /// every group reads as never displayed and counting one would make every question
    /// succeed on a lie.
    ///
    /// Stops the moment it meets the top rung, since nothing outranks it.
    pub fn best_linked_class<F>(&self, start: DialogueNodeId, seen_state: F) -> Option<SeenState>
    where
        F: Fn(DialogueNodeId) -> SeenState,
    {
        let mut expanded = HashSet::new();
        let mut pending = VecDeque::new();
        let mut best: Option<SeenState> = None;

        // Every entry this walk meets goes through the same three lines, the start
        // included. SCORED ON ARRIVAL AND EXPANDED ONCE are two different questions, and
        // answering them with one set is what made `reaches_potential_improvement` skip an
        // entry a link led back to.
        let mut consider = |id: DialogueNodeId, node: &LookAheadNode| -> bool {
            if node.is_group {
                return false;
            }
            let class = seen_state(id);
            if class > SeenState::SeenThisGame && Some(class) > best {
                best = Some(class);
            }
            best == Some(SeenState::UnseenAnyGame)
        };

        if let Some(node) = self.get(start)
            && consider(start, node)
        {
            return best;
        }

        expanded.insert(start);
        pending.push_back(start);

        while let Some(id) = pending.pop_front() {
            let Some(node) = self.get(id) else { continue };
            for &child_id in &node.links {
                let Some(child) = self.get(child_id) else {
                    continue;
                };

                if consider(child_id, child) {
                    return best;
                }

                if expanded.insert(child_id) {
                    pending.push_back(child_id);
                }
            }
        }

        best
    }

    pub fn get_mut(&mut self, id: DialogueNodeId) -> Option<&mut LookAheadNode> {
        self.nodes.get_mut(&id)
    }

    pub fn contains(&self, id: DialogueNodeId) -> bool {
        self.nodes.contains_key(&id)
    }
}

impl fmt::Display for LookAheadGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LookAheadGraph({} nodes, {} slots)",
            self.nodes.len(),
            self.symbols.count()
        )
    }
}

#[cfg(test)]
mod minutes_passable_tests {
    use crate::test_graph::{Entry, GraphBuilder};

    /// What one `PassTime` is worth, so a count reads as the arithmetic it is.
    const STEP: i32 = crate::core::clock::ClockTime::PASS_TIME_MINUTES;

    #[test]
    fn a_group_with_no_pass_time_passes_none() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1))
            .build();

        assert_eq!(graph.minutes_passable(), Some(0));
    }

    #[test]
    fn a_chain_passes_a_step_for_each_one_on_it() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).script("PassTime()").links(&[1]))
            .add(Entry::new(1).script("PassTime()").links(&[2]))
            .add(Entry::new(2))
            .build();

        assert_eq!(graph.minutes_passable(), Some(2 * STEP));
    }

    /// The LONGEST branch, not the sum of them: a walk takes one way through a fork.
    #[test]
    fn a_fork_passes_what_its_longest_branch_passes() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 3]))
            .add(Entry::new(1).script("PassTime()").links(&[2]))
            .add(Entry::new(2).script("PassTime()"))
            .add(Entry::new(3).script("PassTime()"))
            .build();

        assert_eq!(graph.minutes_passable(), Some(2 * STEP));
    }

    /// Both halves of a diamond are walked in turn, so the two branches do not add up.
    #[test]
    fn a_diamond_counts_one_side_and_the_tail() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).script("PassTime()").links(&[3]))
            .add(Entry::new(2).script("PassTime()").links(&[3]))
            .add(Entry::new(3).script("PassTime()"))
            .build();

        assert_eq!(graph.minutes_passable(), Some(2 * STEP));
    }

    /// A `PassTime` a walk can come back round to can be taken without end.
    #[test]
    fn a_pass_time_on_a_cycle_has_no_bound() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).script("PassTime()").links(&[0]))
            .build();

        assert_eq!(graph.minutes_passable(), None);
    }

    /// An entry that links to itself is a cycle of one, which is easy to miss.
    #[test]
    fn a_pass_time_that_links_to_itself_has_no_bound() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).script("PassTime()").links(&[0]))
            .build();

        assert_eq!(graph.minutes_passable(), None);
    }

    /// A cycle carrying no `PassTime` bounds nothing; the walk through it still counts.
    #[test]
    fn a_cycle_without_one_leaves_the_bound_alone() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).script("PassTime()").links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2).links(&[1, 3]))
            .add(Entry::new(3).script("PassTime()"))
            .build();

        assert_eq!(graph.minutes_passable(), Some(2 * STEP));
    }

    /// A check's script counts like any other, whichever way the roll goes.
    #[test]
    fn a_pass_time_on_a_check_counts() {
        let graph = GraphBuilder::new()
            .add(
                Entry::new(0)
                    .kind(crate::core::types::DialogueCheckKind::White)
                    .flag("roll")
                    .script("PassTime()")
                    .links(&[1]),
            )
            .add(Entry::new(1))
            .build();

        assert_eq!(graph.minutes_passable(), Some(STEP));
    }
}

#[cfg(test)]
mod best_linked_class_tests {
    use super::*;
    use crate::test_graph::{Entry, GraphBuilder, node};

    /// A seen state function from two lists, so a fixture can say which entry is which class.
    fn classes<'a>(
        unseen_anywhere: &'a [i32],
        unseen_here: &'a [i32],
    ) -> impl Fn(DialogueNodeId) -> SeenState + 'a {
        move |id| {
            if unseen_anywhere.contains(&id.entry_id) {
                SeenState::UnseenAnyGame
            } else if unseen_here.contains(&id.entry_id) {
                SeenState::UnseenThisGame
            } else {
                SeenState::SeenThisGame
            }
        }
    }

    fn chain() -> LookAheadGraph {
        GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build()
    }

    #[test]
    fn everything_read_leaves_no_class_to_hunt() {
        let graph = chain();

        assert_eq!(graph.best_linked_class(node(0), classes(&[], &[])), None);
    }

    /// THE CASE THE WHOLE THING EXISTS FOR. A search that stopped at the first entry
    /// beating "seen" would stop at 1 and report unseen-here, which is a red marker where
    /// orange is right.
    #[test]
    fn the_top_rung_wins_even_when_a_lower_one_is_nearer() {
        let graph = chain();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[2], &[1])),
            Some(SeenState::UnseenAnyGame),
        );
    }

    #[test]
    fn the_rung_below_is_named_only_when_the_top_one_is_absent() {
        let graph = chain();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[], &[1, 2])),
            Some(SeenState::UnseenThisGame),
        );
    }

    /// THE START IS A RESULT LIKE ANY OTHER, scored before a link is walked.
    ///
    /// Whether that can ever fire is the caller's business, not this walk's: for an
    /// ordinary option the baseline is the start's own class, so it cannot outrank itself
    /// and the comparison is wasted. For one outcome of a rolled check the baseline is
    /// where that outcome LANDS, and the check entry can outrank it.
    #[test]
    fn the_start_is_a_candidate_for_itself() {
        let graph = chain();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[0], &[])),
            Some(SeenState::UnseenAnyGame),
        );
    }

    /// And no loop is needed for that, though one changes nothing.
    #[test]
    fn the_start_counts_once_a_link_leads_back_to_it() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[0]))
            .build();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[0], &[])),
            Some(SeenState::UnseenAnyGame),
        );
    }

    /// The game never writes a group's SimStatus, so every group reads as never displayed
    /// and counting one would make every question succeed on a lie.
    #[test]
    fn a_group_is_never_a_candidate() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group())
            .build();

        assert_eq!(graph.best_linked_class(node(0), classes(&[1], &[])), None);
    }

    /// GUARDS ARE IGNORED, deliberately: this walk is what decides whether to spend a
    /// search at all, so it over-approximates and lets the search find out.
    #[test]
    fn a_guard_nothing_can_open_is_still_counted() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
            .add(Entry::new(2))
            .build();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[2], &[])),
            Some(SeenState::UnseenAnyGame),
        );
    }

    /// A loop is walked once, and an entry nothing links to is not reached at all.
    #[test]
    fn it_walks_links_rather_than_the_whole_group() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[0]))
            .add(Entry::new(2))
            .build();

        assert_eq!(graph.best_linked_class(node(0), classes(&[2], &[])), None);
    }
}

#[cfg(test)]
mod iteration_order_tests {
    use super::*;
    use crate::test_graph::{Entry, GraphBuilder};

    /// Entries come back by conversation and then by entry id, whatever order they arrived.
    ///
    /// ## What this is protecting
    ///
    /// A search whose COST depends on the process it runs in. `nodes()` used to hand back a
    /// `HashMap`'s values, and the standard hasher is seeded per process - so the layout's
    /// threshold narrowing, the SCC decomposition and the order guards were compiled all
    /// followed a different order in every run. de-12wr.3 measured what that was worth:
    /// three processes asked the same question of conversation 1030 and all three answered
    /// it identically, having built 126,106, 126,588 and 126,148 diagram nodes to do it -
    /// and one of the three overflowed a stack the other two did not.
    ///
    /// The answers were never wrong, which is exactly why this needs a test rather than
    /// being noticed: nothing failed, the matrix's `nodes` column simply could not be
    /// compared between runs and nobody could say why.
    #[test]
    fn entries_come_back_in_a_stable_order() {
        let shuffled = GraphBuilder::new()
            .add(Entry::new(7))
            .add(Entry::new(1))
            .add(Entry::new(30))
            .add(Entry::new(2))
            .add(Entry::new(4))
            .build();

        let order: Vec<i32> = shuffled.nodes().map(|node| node.id.entry_id).collect();
        assert_eq!(
            order,
            vec![1, 2, 4, 7, 30],
            "entries should come back by id"
        );

        // NUMERIC, NOT LEXICOGRAPHIC, which is what the 30 is there to catch: sorted as text
        // it lands between 2 and 4, and a run ordered that way would be perfectly stable and
        // perfectly confusing to read against an id.
        assert_eq!(shuffled.nodes().count(), shuffled.count());
    }

    /// The same entries in a different arrival order build the same iteration order.
    #[test]
    fn the_order_does_not_depend_on_how_the_graph_was_built() {
        let forwards = GraphBuilder::new()
            .add(Entry::new(1))
            .add(Entry::new(2))
            .add(Entry::new(3))
            .build();
        let backwards = GraphBuilder::new()
            .add(Entry::new(3))
            .add(Entry::new(2))
            .add(Entry::new(1))
            .build();

        let one: Vec<DialogueNodeId> = forwards.nodes().map(|node| node.id).collect();
        let other: Vec<DialogueNodeId> = backwards.nodes().map(|node| node.id).collect();
        assert_eq!(one, other);
    }
}

/// The group with every run of entries a play cannot stop inside folded into one entry, and
/// what each folded entry now stands for.
///
/// ## What a run is here
///
/// A chain where reaching the first entry means showing all of them: one way out of each, one
/// way into each but the first, no choice to make, no guard to fail, no check whose outcome the
/// world decides and nothing to pay. Nothing can divert, stop or arrive part way.
///
/// ## What folding one buys
///
/// FEWER ENTRIES for every sweep of the graph, and fewer SLOTS. A `once` fires while its own
/// entry's slot is clear and raises it as it fires, so a run's onces - which can only ever fire
/// together - need ONE slot between them rather than one each. That is the point of this rather
/// than of any cheaper way to skip an entry: it is a layout change wearing a graph change's
/// clothes, and the layout is where this engine's one large win came from.
///
/// ## What it cannot break, and what a caller must carry
///
/// THE DISTANCE. A layer is charged for LEAVING A CHOICE and a run holds none, so every entry
/// in one is the same distance from everywhere and folding them moves nothing.
///
/// THE NAMES, which is what the map is for. A profile names entries, a witness is an entry and
/// the plugin reads entry ids, so a caller translates the entries it cares about through
/// [`Collapsed::into_head`] and reads answers back the same way. An entry folded away is
/// represented by its run's head, which is a real entry of the real dialogue standing at the
/// same distance.
#[derive(Debug)]
pub struct Collapsed {
    pub graph: LookAheadGraph,
    /// Where each folded entry went; absent for one that stands where it did.
    pub into_head: HashMap<DialogueNodeId, DialogueNodeId>,
}

impl LookAheadGraph {
    /// Folds every run of entries a play cannot stop inside into its head. See [`Collapsed`].
    pub fn collapsing_runs(&self) -> Collapsed {
        let mut arrivals: HashMap<DialogueNodeId, usize> = HashMap::new();
        for node in self.nodes() {
            for &link in &node.links {
                *arrivals.entry(link).or_default() += 1;
            }
        }
        // NOTHING TO DECIDE, FAIL, PAY OR REFUSE, which is what makes an entry one a play
        // passes through rather than one it can be stopped at.
        let foldable = |node: &LookAheadNode| {
            !node.is_group
                && !node.choice
                && node.kind == DialogueCheckKind::None
                && node.cost == 0
                && node.click_cost == 0
                && !node.cost_once
                && !node.hidden_when_unaffordable
                && matches!(
                    node.guard.expression(),
                    crate::core::guard::GuardExpression::Literal(value)
                        if value.as_condition() == crate::core::types::Ternary::True
                )
        };
        let follows = |id: DialogueNodeId| -> Option<DialogueNodeId> {
            let node = self.get(id)?;
            if node.links.len() != 1 || !foldable(node) {
                return None;
            }
            let next = self.get(node.links[0])?;
            let alone = arrivals.get(&next.id).copied().unwrap_or(0) == 1;
            match alone && foldable(next) && next.id != id {
                true => Some(next.id),
                false => None,
            }
        };

        let mut into_head = HashMap::new();
        let mut folded_away = HashSet::new();
        let mut runs: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
        // IN THE GRAPH'S OWN ORDER, so which entry heads a run cannot depend on a hash map.
        for &id in &self.order {
            if folded_away.contains(&id) {
                continue;
            }
            let mut run = vec![id];
            while let Some(next) = follows(*run.last().expect("a run has a head")) {
                if folded_away.contains(&next) || run.contains(&next) {
                    break;
                }
                run.push(next);
            }
            if run.len() < 2 {
                continue;
            }
            for &member in &run[1..] {
                folded_away.insert(member);
                into_head.insert(member, id);
            }
            runs.insert(id, run);
        }

        let mut nodes = Vec::with_capacity(self.order.len() - folded_away.len());
        for &id in &self.order {
            if folded_away.contains(&id) {
                continue;
            }
            let Some(node) = self.get(id) else { continue };
            let mut folded = node.clone();
            if let Some(run) = runs.get(&id) {
                for &member in &run[1..] {
                    let Some(behind) = self.get(member) else {
                        continue;
                    };
                    folded.actions.extend(behind.actions.iter().cloned());
                    folded.holds_the_screen |= behind.holds_the_screen;
                }
                folded.links = self
                    .get(*run.last().expect("a run has a tail"))
                    .map(|tail| tail.links.clone())
                    .unwrap_or_default();
                // RECOMPUTED FROM WHAT THE FOLDED ENTRY NOW DOES, since its actions are every
                // member's rather than the head's.
                folded.skill_moves =
                    crate::core::skill_movers::SkillMoves::of(folded.all_actions(), self.symbols());
                // ONE SLOT FOR THE RUN: cleared so the build interns a single one if the folded
                // actions ask for it. The slots of the entries that are gone stay in the symbol
                // table and are dropped by the layout, which keeps only what something reads.
                folded.once_slot = -1;
            }
            nodes.push(folded);
        }

        // THE SLOTS OF THE ENTRIES THAT ARE GONE GO WITH THEM. Keeping the table whole leaves a
        // once slot per folded entry in it, and every layout built from this graph sweeps and
        // prunes them again - a fixed cost per menu, measured at 8 to 9 ms on groups whose
        // search does nothing at all. `StateSymbols::retaining` drops them and says where the
        // survivors moved to, which the entries' own slot fields are remapped through.
        let mut keep = vec![true; self.symbols().count()];
        for member in into_head.keys() {
            if let Some(gone) = self.get(*member).map(|node| node.once_slot) {
                if gone >= 0 {
                    keep[gone as usize] = false;
                }
            }
        }
        let (symbols, moved) = self.symbols().retaining(&keep);
        let remap = |slot: i32| match slot >= 0 {
            true => moved.get(slot as usize).copied().unwrap_or(-1),
            false => -1,
        };
        for node in &mut nodes {
            node.seen_slot = remap(node.seen_slot);
            node.flag_slot = remap(node.flag_slot);
            node.failed_flag_slot = remap(node.failed_flag_slot);
            node.once_slot = remap(node.once_slot);
        }
        let graph = LookAheadGraph::new(nodes, symbols).expect("folding cannot duplicate an entry");
        Collapsed { graph, into_head }
    }
}
/// Sets the bit for `slot`, where the slot is one.
///
/// A SLOT FIELD SPELLS "NONE" AS A NEGATIVE NUMBER and a node has four of them, so taking the
/// field's own type and ignoring what is not a slot is what keeps the callers free of the same
/// two-line conversion four times over. A slot past the end of the set is ignored for the same
/// reason: a symbol table can be wider than the layout built from it.
fn raise(bits: &mut [u64], slot: i32) {
    let Ok(slot) = usize::try_from(slot) else {
        return;
    };
    if slot < bits.len() * 64 {
        bits[slot / 64] |= 1 << (slot % 64);
    }
}

/// Whether the bit for `slot` is set.
fn holds(bits: &[u64], slot: usize) -> bool {
    slot < bits.len() * 64 && bits[slot / 64] >> (slot % 64) & 1 == 1
}

/// Which slots some path may have written by the time it ARRIVES at each entry.
///
/// Reaching definitions over the links: what leaves an entry is what arrived plus what the
/// entry writes, and what arrives at an entry is the union of what leaves the entries linking
/// to it. Nothing has been written on arrival at an entry nothing links to, which is where the
/// fixed point starts.
///
/// A WORKLIST RATHER THAN SWEEPS, because a dialogue loops: an entry whose arrivals grow has to
/// tell the entries it links to, and a pass in any fixed order would take as many passes as the
/// longest chain. Bits only ever go up, so it terminates.
fn reaching_writes(
    entries: &[&LookAheadNode],
    at_of: &HashMap<DialogueNodeId, usize>,
    writes: &[u64],
    words: usize,
) -> Vec<u64> {
    let mut arriving = vec![0u64; entries.len() * words];
    let mut pending: std::collections::VecDeque<usize> = (0..entries.len()).collect();
    let mut queued = vec![true; entries.len()];
    // ONE SCRATCH SET FOR THE WHOLE FIXED POINT. What leaves an entry is rebuilt on every pop,
    // and a pop happens once per entry per growth of its arrivals - hundreds of thousands of
    // times over a group this size, which is no place to allocate.
    let mut leaving = vec![0u64; words];
    while let Some(at) = pending.pop_front() {
        queued[at] = false;
        for word in 0..words {
            leaving[word] = arriving[at * words + word] | writes[at * words + word];
        }
        for link in &entries[at].links {
            let Some(&onward) = at_of.get(link) else {
                continue;
            };
            let mut grew = false;
            for word in 0..words {
                let joined = arriving[onward * words + word] | leaving[word];
                grew |= joined != arriving[onward * words + word];
                arriving[onward * words + word] = joined;
            }
            if grew && !queued[onward] {
                queued[onward] = true;
                pending.push_back(onward);
            }
        }
    }
    arriving
}
