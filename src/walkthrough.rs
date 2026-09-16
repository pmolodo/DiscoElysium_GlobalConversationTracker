// SPDX-License-Identifier: MIT
//! The walk a player's inputs make through a conversation, from its start.
//!
//! ## What it is for
//!
//! A scenario reaches the menu it is about the way a player does: by what is pressed from
//! the moment the conversation opens. Both executors follow the same inputs - the in-game
//! harness through the probe, the offline runner through this - and what this returns is
//! what the game would have shown on the way, which is what `LookAheadRequest::encountered`
//! carries, and the menu the inputs end at, which is what the request asks about.
//!
//! ## The rules it applies
//!
//! The dialogue system's, as the game plays them:
//!
//! - From where the conversation stands, links are evaluated in order and a group whose
//!   guard passes is expanded in place. The first NPC line on offer is said next; player
//!   lines are offered as a menu only when no NPC line is.
//! - A passive check that does not fire is stepped over rather than blocking the way: the
//!   game writes Passthrough onto such an entry as it decides it, so its links are evaluated
//!   in its place and nothing is displayed for it. The entries gone over this way are on the
//!   walk, because a hub can sit behind one.
//! - A line with another line behind it waits for a continue, which is one [`Input::Enter`].
//!   A line with a menu behind it does not wait: the menu comes up beside it. A chosen
//!   option never waits. Measured in game: Siileng's stall puts up two lines and needs one
//!   continue, and choosing 29:582 reaches Kim's case menu with none although his line
//!   29:393 stands between them.
//! - A menu is drawn top to bottom in link order, and [`Input::Choose`] counts in that
//!   order. Read off the screen at Garte's hub, whose offered links run 130, 528, 718, 892
//!   and whose menu reads the same way down, leaving last. The probe's menu event lists the
//!   same options bottom up - it hears them composed in that order - so its 892, 718, 528,
//!   130 is this menu reversed, and the in-game harness counts from its end.
//!
//! Link priorities are not in the index, so every link is taken at the same priority.
//!
//! ## What it refuses rather than guesses
//!
//! A guard or passive check this world cannot decide, where the answer changes what is
//! shown; a rolled check chosen on the way, whose outcome is the roll's; an option the purse
//! cannot pay for; and any input that does not fit what is on screen. Each is an error naming
//! the step, because a walk that guessed would reach a menu the game does not.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use crate::core::action::CounterCaps;
use crate::core::state::{LookAheadState, seed_state};
use crate::core::types::{DialogueCheckKind, DialogueNodeId, Ternary};
use crate::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::oracle::{COUNTER_CAP, can_afford, charge, has_been_seen};
use crate::world::{CrawlContext, ILookAheadWorld};

/// How an [`Input::Enter`] is spelled in a scenario.
pub const ENTER: &str = "enter";

/// How many steps a walk may take before it is called a loop. Far above any scenario.
const MOST_STEPS: usize = 1_000;

/// One thing a player presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// Advances a waiting line, or takes the only option of a menu of one.
    Enter,
    /// Chooses the N-th option, counting from 1 in drawn order, of a menu of several.
    Choose(usize),
}

impl FromStr for Input {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text == ENTER {
            return Ok(Input::Enter);
        }
        match text.parse::<usize>() {
            Ok(number) if number >= 1 => Ok(Input::Choose(number)),
            _ => Err(format!(
                "\"{text}\" is not an input: an input is \"{ENTER}\" or an option's number, \
                 counting from 1"
            )),
        }
    }
}

impl fmt::Display for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Input::Enter => f.write_str(ENTER),
            Input::Choose(number) => write!(f, "{number}"),
        }
    }
}

/// Where a walk ended, and what it stepped through on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walkthrough {
    /// Every entry the walk stepped through, in order: the conversation's start, the lines
    /// said and the options chosen, and the entries the game goes over without displaying -
    /// the group entries it expands in place and the passive checks that do not fire.
    ///
    /// WHAT THE PLUGIN RECORDS AND A REQUEST CARRIES, so a hub is on the walk however the
    /// player reached it, and nothing downstream has to recover one from the links.
    pub encountered: Vec<DialogueNodeId>,
    /// The ones the game put on screen: the lines said and the options chosen.
    ///
    /// What the walk's own rules are about - a line waits for a continue, a menu is chosen
    /// from - and what a reader comparing a walk against the game's own log should read.
    pub displayed: Vec<DialogueNodeId>,
    /// The menu the walk ended at, in the order the game draws it.
    pub menu: Vec<DialogueNodeId>,
}

/// Walks `conversation` from its start by `inputs`.
///
/// `None` presses [`Input::Enter`] at every waiting line and stops at the first menu, which
/// is what a scenario that names no inputs is about. `Some` must end exactly at a menu: an
/// input left over, or a line still waiting when they run out, is an error.
pub fn walk_inputs(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    conversation: i32,
    inputs: Option<&[Input]>,
) -> Result<Walkthrough, String> {
    Walker {
        graph,
        world,
        context: CrawlContext::new(graph.symbols(), world),
        caps: CounterCaps::flat(COUNTER_CAP),
    }
    .walk(conversation, inputs)
}

/// An entry on offer, with the groups the evaluation went through to reach it.
#[derive(Debug, Clone)]
struct Offered {
    id: DialogueNodeId,
    via: Vec<DialogueNodeId>,
}

/// One thing a link evaluation found.
enum Candidate {
    Line(Offered),
    Option(Offered),
    /// An entry whose showing this world cannot decide, and why.
    Undecided {
        id: DialogueNodeId,
        why: &'static str,
        /// A player line, which can change the menu but never which line is said.
        player: bool,
    },
}

/// What follows an entry.
enum Next {
    Line(Offered),
    /// In drawn order.
    Menu(Vec<Offered>),
    End,
}

struct Walker<'w> {
    graph: &'w LookAheadGraph,
    world: &'w dyn ILookAheadWorld,
    context: CrawlContext<'w>,
    caps: CounterCaps<'static>,
}

impl Walker<'_> {
    fn walk(&self, conversation: i32, inputs: Option<&[Input]>) -> Result<Walkthrough, String> {
        let start = DialogueNodeId::new(conversation, 0);
        if self.graph.get(start).is_none() {
            return Err(format!("{start}: the conversation has no start entry"));
        }

        let to_first_menu = inputs.is_none();
        let mut keys = inputs.unwrap_or_default().iter().copied().enumerate();
        let mut state = seed_state(self.graph, self.world);
        let mut at = start;
        // Whether `at` is a line, which waits for a continue unless a menu follows it.
        let mut line_up = false;
        // THE START FIRST, because the game reports it: its line hook fires for entry 0 as a
        // conversation opens - measured in game, 656:0 and 29:0 each ahead of the first line
        // with text - so the plugin's walk begins there, and the engine follows the hubs from
        // there. A walk without it loses every hub passed before the first line shown.
        let mut encountered = vec![start];
        let mut displayed = vec![start];

        for _ in 0..MOST_STEPS {
            match self.next(at, &state)? {
                Next::Line(line) => {
                    if line_up {
                        match keys.next() {
                            Some((_, Input::Enter)) => {}
                            Some((index, key)) => {
                                return Err(format!(
                                    "input {} is \"{key}\", but {at} is a line waiting for \
                                     \"{ENTER}\"",
                                    index + 1
                                ));
                            }
                            None if to_first_menu => {}
                            None => {
                                return Err(format!(
                                    "the inputs end at {at}, a line waiting for \"{ENTER}\", \
                                     rather than at a menu"
                                ));
                            }
                        }
                    }
                    state = self.take(&line, &state);
                    // THE ENTRIES GONE OVER FIRST, then the line itself: the game walks the
                    // groups and passed-over checks on the way to a line before it says it.
                    encountered.extend(line.via.iter().copied());
                    encountered.push(line.id);
                    displayed.push(line.id);
                    at = line.id;
                    line_up = true;
                }
                Next::Menu(options) => {
                    let drawn: Vec<DialogueNodeId> = options.iter().map(|o| o.id).collect();
                    let Some((index, key)) = keys.next() else {
                        // THE MENU'S OWN HUBS ARE ON THE WALK. Composing it expands the groups
                        // between here and the options, so the hub a menu hangs off is passed
                        // whether or not anything is chosen from it - which is what the game
                        // records, and what tells the cut where the player is standing. The
                        // options share their way in, so a repeat of the entry just added is
                        // the same step rather than a second one.
                        for option in &options {
                            for &id in &option.via {
                                if encountered.last() != Some(&id) {
                                    encountered.push(id);
                                }
                            }
                        }
                        return Ok(Walkthrough {
                            encountered,
                            displayed,
                            menu: drawn,
                        });
                    };
                    let chosen = match key {
                        Input::Enter if options.len() == 1 => &options[0],
                        Input::Enter => {
                            return Err(format!(
                                "input {} is \"{ENTER}\", but the menu after {at} offers {} \
                                 options ({}), and one of several is chosen by its number",
                                index + 1,
                                options.len(),
                                listed(&drawn)
                            ));
                        }
                        Input::Choose(_) if options.len() == 1 => {
                            return Err(format!(
                                "input {} is \"{key}\", but the menu after {at} offers one \
                                 option ({}), and a single option is taken with \"{ENTER}\"",
                                index + 1,
                                drawn[0]
                            ));
                        }
                        Input::Choose(number) if number <= options.len() => &options[number - 1],
                        Input::Choose(_) => {
                            return Err(format!(
                                "input {} is \"{key}\", but the menu after {at} offers only {} \
                                 options ({})",
                                index + 1,
                                options.len(),
                                listed(&drawn)
                            ));
                        }
                    };

                    let node = self.node(chosen.id);
                    if node.is_rolled() {
                        return Err(format!(
                            "input {} chooses {}, a rolled check, and which way it goes is the \
                             roll's",
                            index + 1,
                            chosen.id
                        ));
                    }
                    if !can_afford(node, &state) {
                        return Err(format!(
                            "input {} chooses {}, which the purse cannot pay for",
                            index + 1,
                            chosen.id
                        ));
                    }
                    state = self.take(chosen, &state);
                    encountered.extend(chosen.via.iter().copied());
                    encountered.push(chosen.id);
                    displayed.push(chosen.id);
                    at = chosen.id;
                    line_up = false;
                }
                Next::End => {
                    return Err(format!(
                        "the conversation ends after {at}, before the inputs reach a menu"
                    ));
                }
            }
        }

        Err(format!(
            "{MOST_STEPS} steps without reaching the menu, so the walk is going round a loop"
        ))
    }

    fn node(&self, id: DialogueNodeId) -> &LookAheadNode {
        self.graph
            .get(id)
            .expect("an offered entry is in the graph")
    }

    /// Enters an offered entry: the groups on the way to it, then the entry itself.
    fn take(&self, offered: &Offered, state: &LookAheadState) -> LookAheadState {
        let mut next = state.clone();
        for &id in offered.via.iter().chain(std::iter::once(&offered.id)) {
            next = charge(self.node(id), &next, &self.caps, self.world);
        }
        next
    }

    /// What follows `from`, by the dialogue system's rules.
    fn next(&self, from: DialogueNodeId, state: &LookAheadState) -> Result<Next, String> {
        let mut candidates = Vec::new();
        self.offer(from, state, &[], &mut HashSet::new(), &mut candidates);

        for candidate in &candidates {
            match candidate {
                Candidate::Line(line) => return Ok(Next::Line(line.clone())),
                Candidate::Undecided {
                    id,
                    why,
                    player: false,
                } => {
                    return Err(format!(
                        "after {from}, which line is said depends on {id}, and {why}"
                    ));
                }
                _ => {}
            }
        }

        let mut options: Vec<Offered> = Vec::new();
        for candidate in candidates {
            match candidate {
                Candidate::Option(option) => {
                    if !options.iter().any(|o| o.id == option.id) {
                        options.push(option);
                    }
                }
                Candidate::Undecided { id, why, .. } => {
                    return Err(format!(
                        "the menu after {from} depends on whether {id} is offered, and {why}"
                    ));
                }
                Candidate::Line(_) => unreachable!("a line was returned above"),
            }
        }

        if options.is_empty() {
            return Ok(Next::End);
        }
        Ok(Next::Menu(options))
    }

    /// Evaluates `from`'s links in order, expanding the groups that are shown.
    fn offer(
        &self,
        from: DialogueNodeId,
        state: &LookAheadState,
        via: &[DialogueNodeId],
        visited: &mut HashSet<DialogueNodeId>,
        candidates: &mut Vec<Candidate>,
    ) {
        if !visited.insert(from) {
            return;
        }

        for &id in &self.node(from).links {
            let Some(child) = self.graph.get(id) else {
                continue;
            };
            // A PASSIVE CHECK THAT DOES NOT FIRE IS STEPPED OVER RATHER THAN BLOCKING THE WAY.
            // The game's PassiveNode.CheckSuccess writes Passthrough onto the entry before it
            // answers, so a failed check leaves the link evaluated in the entry's place and
            // nothing displayed for it. That is how a player walks past a skill line their
            // character never says - and, in conversation 379, into the hub behind it.
            //
            // THE GUARD DECIDES FIRST, as it does in the game: the validator is only consulted
            // where the Lua condition passed, so an entry whose guard is false is blocked
            // whatever its check would have said.
            if child.kind == DialogueCheckKind::Passive
                && child.guard.test(&self.context.bound(state)) == Ternary::True
                && self.world.check_passes(child.id) == Ternary::False
            {
                let deeper: Vec<DialogueNodeId> =
                    via.iter().copied().chain(std::iter::once(id)).collect();
                self.offer(id, state, &deeper, visited, candidates);
                continue;
            }

            match self.shows(child, state) {
                Err(why) => candidates.push(Candidate::Undecided {
                    id,
                    why,
                    player: child.player && !child.is_group,
                }),
                Ok(false) => {}
                Ok(true) if child.is_group => {
                    let deeper: Vec<DialogueNodeId> =
                        via.iter().copied().chain(std::iter::once(id)).collect();
                    self.offer(id, state, &deeper, visited, candidates);
                }
                Ok(true) => {
                    let offered = Offered {
                        id,
                        via: via.to_vec(),
                    };
                    candidates.push(if child.player {
                        Candidate::Option(offered)
                    } else {
                        Candidate::Line(offered)
                    });
                }
            }
        }
    }

    /// Whether the game shows `node` from `state`, or why this world cannot say.
    fn shows(&self, node: &LookAheadNode, state: &LookAheadState) -> Result<bool, &'static str> {
        match node.guard.test(&self.context.bound(state)) {
            Ternary::False => return Ok(false),
            Ternary::Unknown => return Err("its guard cannot be decided in this world"),
            Ternary::True => {}
        }
        if node.kind == DialogueCheckKind::Passive {
            match self.world.check_passes(node.id) {
                Ternary::False => return Ok(false),
                Ternary::Unknown => return Err("its passive check is not decided in this world"),
                Ternary::True => {}
            }
        }
        if node.closes_once_seen() && has_been_seen(node, state) {
            return Ok(false);
        }
        if node.hidden_when_unaffordable && !can_afford(node, state) {
            return Ok(false);
        }
        Ok(true)
    }
}

fn listed(ids: &[DialogueNodeId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::guard_value::GuardValue;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::test_world::TestWorld;

    fn graph(entries: Vec<Entry>) -> LookAheadGraph {
        entries
            .into_iter()
            .fold(GraphBuilder::new(), GraphBuilder::add)
            .build()
    }

    fn walked(
        entries: Vec<Entry>,
        world: &TestWorld,
        inputs: Option<&[Input]>,
    ) -> Result<Walkthrough, String> {
        walk_inputs(
            &graph(entries),
            world,
            crate::test_graph::DEFAULT_CONVERSATION,
            inputs,
        )
    }

    fn nodes(ids: &[i32]) -> Vec<DialogueNodeId> {
        ids.iter().map(|id| node(*id)).collect()
    }

    /// Two lines and a menu of two: the first line waits, the second has the menu beside it.
    fn two_lines_then_a_menu() -> Vec<Entry> {
        vec![
            Entry::new(0).links(&[1]),
            Entry::new(1).links(&[2]),
            Entry::new(2).links(&[3, 4]),
            Entry::new(3).player(),
            Entry::new(4).player(),
        ]
    }

    #[test]
    fn inputs_parse_as_enter_or_a_number_from_one() {
        assert_eq!("enter".parse::<Input>(), Ok(Input::Enter));
        assert_eq!("2".parse::<Input>(), Ok(Input::Choose(2)));
        assert!("0".parse::<Input>().is_err());
        assert!("Enter".parse::<Input>().is_err());
    }

    #[test]
    fn only_a_line_with_a_line_behind_it_waits_for_enter() {
        let world = TestWorld::new();
        let walk = walked(two_lines_then_a_menu(), &world, Some(&[Input::Enter])).unwrap();
        assert_eq!(walk.encountered, nodes(&[0, 1, 2]));

        let error = walked(two_lines_then_a_menu(), &world, Some(&[])).unwrap_err();
        assert!(error.contains("line waiting"), "{error}");
    }

    #[test]
    fn no_inputs_presses_enter_to_the_first_menu() {
        let walk = walked(two_lines_then_a_menu(), &TestWorld::new(), None).unwrap();
        assert_eq!(walk.encountered, nodes(&[0, 1, 2]));
        assert_eq!(walk.menu, nodes(&[3, 4]));
    }

    #[test]
    fn a_menu_is_drawn_and_numbered_in_link_order() {
        let mut entries = two_lines_then_a_menu();
        entries[4] = Entry::new(4).player().links(&[5]);
        entries.push(Entry::new(5).links(&[6]));
        entries.push(Entry::new(6).player());
        let walk = walked(
            entries,
            &TestWorld::new(),
            Some(&[Input::Enter, Input::Choose(2)]),
        )
        .unwrap();
        assert_eq!(walk.encountered, nodes(&[0, 1, 2, 4, 5]));
        assert_eq!(walk.menu, nodes(&[6]));
    }

    #[test]
    fn inputs_that_do_not_fit_the_screen_fail_by_name() {
        let world = TestWorld::new();
        let enter_at_several = walked(
            two_lines_then_a_menu(),
            &world,
            Some(&[Input::Enter, Input::Enter]),
        )
        .unwrap_err();
        assert!(enter_at_several.contains("input 2"), "{enter_at_several}");

        let number_at_a_line =
            walked(two_lines_then_a_menu(), &world, Some(&[Input::Choose(1)])).unwrap_err();
        assert!(number_at_a_line.contains("waiting"), "{number_at_a_line}");

        let beyond = walked(
            two_lines_then_a_menu(),
            &world,
            Some(&[Input::Enter, Input::Choose(3)]),
        )
        .unwrap_err();
        assert!(beyond.contains("only 2"), "{beyond}");

        let single = vec![
            Entry::new(0).links(&[1]),
            Entry::new(1).player().links(&[2]),
            Entry::new(2).links(&[3, 4]),
            Entry::new(3).player(),
            Entry::new(4).player(),
        ];
        let number_at_one = walked(single, &world, Some(&[Input::Choose(1)])).unwrap_err();
        assert!(number_at_one.contains("single option"), "{number_at_one}");
    }

    #[test]
    fn a_chosen_option_does_not_wait() {
        let entries = vec![
            Entry::new(0).links(&[1]),
            Entry::new(1).player().links(&[2]),
            Entry::new(2).links(&[3]),
            Entry::new(3).links(&[4, 5]),
            Entry::new(4).player(),
            Entry::new(5).player(),
        ];
        let walk = walked(
            entries,
            &TestWorld::new(),
            Some(&[Input::Enter, Input::Enter]),
        )
        .unwrap();
        assert_eq!(walk.encountered, nodes(&[0, 1, 2, 3]));
        assert_eq!(walk.menu, nodes(&[4, 5]));
    }

    #[test]
    fn a_line_on_offer_wins_over_the_options_and_groups_open_in_place() {
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));
        let entries = vec![
            Entry::new(0).links(&[1, 2, 3]),
            Entry::new(1).player(),
            Entry::new(2)
                .group()
                .guard(r#"Variable["shut"]"#)
                .links(&[4]),
            Entry::new(3).group().links(&[5]),
            Entry::new(4),
            Entry::new(5).links(&[1, 6]),
            Entry::new(6).player(),
        ];
        let walk = walked(entries, &world, None).unwrap();
        // THE GROUP IT OPENED IN PLACE IS ON THE WALK, and the line it led to is what was
        // displayed: 3 is never put on screen, and a cut that never heard of it would let a
        // route back through it count as leading onward.
        assert_eq!(walk.encountered, nodes(&[0, 3, 5]));
        assert_eq!(walk.displayed, nodes(&[0, 5]));
        assert_eq!(walk.menu, nodes(&[1, 6]));
    }

    #[test]
    fn an_undecided_guard_is_refused_only_where_it_changes_what_is_shown() {
        let entries = vec![
            Entry::new(0).links(&[1, 2]),
            Entry::new(1).player().guard(r#"Variable["unknown"]"#),
            Entry::new(2).links(&[3]),
            Entry::new(3).player(),
        ];
        let world = TestWorld::new();
        let walk = walked(entries, &world, None).unwrap();
        assert_eq!(walk.encountered, nodes(&[0, 2]));

        let entries = vec![
            Entry::new(0).links(&[1, 2]),
            Entry::new(1).guard(r#"Variable["unknown"]"#),
            Entry::new(2).links(&[3]),
            Entry::new(3).player(),
        ];
        let error = walked(entries, &world, None).unwrap_err();
        assert!(error.contains("cannot be decided"), "{error}");
    }

    #[test]
    fn a_rolled_check_is_not_walked_through() {
        let entries = vec![
            Entry::new(0).links(&[1, 2]),
            Entry::new(1)
                .player()
                .kind(DialogueCheckKind::White)
                .flag("roll")
                .links(&[3]),
            Entry::new(2).player(),
            Entry::new(3),
        ];
        let error = walked(entries, &TestWorld::new(), Some(&[Input::Choose(1)])).unwrap_err();
        assert!(error.contains("rolled check"), "{error}");
    }
}
