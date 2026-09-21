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

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::str::FromStr;

use crate::core::action::CounterCaps;
use crate::core::state::{LookAheadState, seed_state};
use crate::core::types::{DialogueCheckKind, DialogueNodeId, Ternary};
use crate::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::oracle::{COUNTER_CAP, can_afford, charge, enter, has_been_seen};
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
        caps: CounterCaps::for_graph(COUNTER_CAP, graph),
    }
    .walk(conversation, inputs)
}

/// One step of a leg: what was pressed, and what the game did in answer.
///
/// A step with no `input` is one the game took on its own - a line with a menu behind it does
/// not wait, so nothing is pressed and it still walks entries and displays one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// What was pressed, if anything.
    pub input: Option<Input>,
    /// What was on screen when it was pressed.
    pub at: DialogueNodeId,
    /// The menu on screen, in the order the game draws it, where there was one. An
    /// [`Input::Choose`] indexes this from 1, so a step carries what its number meant.
    pub menu: Vec<DialogueNodeId>,
    /// Where this step entered a red or white check, which way the die was taken to go.
    ///
    /// WHAT A REPLAY HAS TO ARRANGE. The keypresses alone do not reproduce a step through a
    /// rolled check, because the game rolls; a replayer needs to know this leg assumed a 12
    /// here, or a 2. `None` is a step that entered no rolled check.
    pub rolled: Option<bool>,
    /// Every entry the game stepped through in answer, in order.
    pub encountered: Vec<DialogueNodeId>,
    /// The ones it put on screen.
    pub displayed: Vec<DialogueNodeId>,
}

/// One leg of a [`greedy_playthrough`]: the conversation walked from its start to the nearest
/// entry the player had not been shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leg {
    /// What this leg was walked for: the nearest unshown entry, counted in presses.
    pub target: DialogueNodeId,
    /// Whether this leg began at the conversation's start rather than where the last one
    /// ended - the game having ended the conversation, or nothing unshown being reachable from
    /// where the player stood.
    ///
    /// WHAT DELIMITS A SESSION. Consecutive legs up to the next restart are one sitting at the
    /// NPC, and their inputs run together into one keypress sequence from the start entry.
    pub restarted: bool,
    /// The steps, in order. [`Self::inputs`] is the keypress sequence they spell.
    pub steps: Vec<Step>,
}

impl Leg {
    /// The keypress sequence: what the in-game harness presses through the probe.
    ///
    /// NOT AN INPUT LIST [`walk_inputs`] WILL TAKE BACK, which is worth saying because the
    /// shapes match and the assumption is free to make. That function must finish exactly at a
    /// menu; a session finishes wherever its last leg's target was, and a walk driving at the
    /// last entries nobody has seen characteristically ends on a terminal line. Measured over
    /// five real groups, not one session was accepted - see `greedy_playthrough`'s own check.
    pub fn inputs(&self) -> Vec<Input> {
        self.steps.iter().filter_map(|step| step.input).collect()
    }

    /// Every entry stepped through, the conversation's start first - what a request carries
    /// as `encountered`.
    pub fn encountered(&self) -> Vec<DialogueNodeId> {
        self.steps
            .iter()
            .flat_map(|step| step.encountered.iter().copied())
            .collect()
    }

    /// The ones the game put on screen.
    pub fn displayed(&self) -> Vec<DialogueNodeId> {
        self.steps
            .iter()
            .flat_map(|step| step.displayed.iter().copied())
            .collect()
    }
}

/// Why a [`greedy_playthrough`] stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// Nothing unshown can be reached any more, whatever the playthrough does next. This is
    /// the answer that makes the entries left over meaningful.
    Exhausted,
    /// A leg's search passed its ceiling, so what is still unshown is not known to be out of
    /// reach - only that this did not find it.
    OutOfRoom,
    /// The conversation has no start entry.
    NoStart,
}

/// A playthrough that always walks to the nearest thing it has not been shown.
#[derive(Debug, Clone)]
pub struct Playthrough {
    pub legs: Vec<Leg>,
    /// Entries put on screen, in the order they were first shown.
    pub shown: Vec<DialogueNodeId>,
    /// Positions the walk would not step past, because it refuses rather than guesses - see
    /// the module doc. A branch is pruned rather than the leg abandoned, so this is a count of
    /// how much of the conversation this world cannot decide.
    pub refused: usize,
    /// Why, for the first few: the entry the walk stood at and what it would not decide.
    ///
    /// A COUNT ALONE CANNOT BE ACTED ON. A playthrough that stops early stops for a reason, and
    /// the reason is the difference between a world that is missing an answer and a
    /// conversation that genuinely goes no further.
    pub blocked: Vec<(DialogueNodeId, String)>,
    /// Every red or white check the walk was offered, with the fewest presses it stood at one
    /// from a sitting's start.
    ///
    /// WHAT THE NEXT STAGE OF AN ESCALATION PICKS FROM: the closest check not yet being passed
    /// is the one to try a 12 on.
    pub rolled: Vec<(DialogueNodeId, usize)>,
    /// The data state the playthrough ends in: what a save would hold, the counters at the
    /// values the walk drove them to and the one-time effects it fired already fired.
    ///
    /// WHAT MAKES A TRUNCATED WALK USABLE AS A PROFILE. The seen set alone says which entries
    /// were displayed; this says what they DID, and the two together are the world the walk
    /// stopped in rather than a world somebody declared.
    pub ended: LookAheadState,
    /// The entry the player is looking at when the playthrough stops.
    pub ended_at: DialogueNodeId,
    /// Whether that entry is a line waiting for a continue, which decides whether playing on
    /// from here costs a press.
    pub ended_waiting: bool,
    pub stopped: Stop,
}

/// When a playthrough stops short of exhausting the conversation.
///
/// WHY IT IS COUNTED IN ENTRIES SHOWN rather than legs: a leg can show several entries, so
/// stopping after n legs stops at a number nobody chose. A profile wants exactly so many
/// entries still to come.
#[derive(Clone, Copy, Default)]
pub struct Until {
    /// Stop once this many entries have been shown. `None` walks to exhaustion.
    pub shown: Option<usize>,
}

/// How many refusals a playthrough keeps the reason for. Enough to see the shape of what a
/// world could not answer, without a row carrying one per position.
const BLOCKED_KEPT: usize = 8;

/// Plays `conversation` by always walking to the nearest entry it has not been shown, until
/// nothing unshown is reachable.
///
/// ## What it is for
///
/// A state some play demonstrably reaches. The alternative is to ASSERT one - to declare a set
/// of entries seen and hand it to the engine - and an asserted set can contradict itself: a
/// `seen:` slot shuts an entry that shuts once seen, so declaring most of a conversation seen
/// can close the routes to the rest of it and leave content that is link-reachable and
/// symbolically unreachable. What this returns cannot, because the walk that produced it is
/// the witness. See de-l88t.
///
/// ## The rule
///
/// A leg stops at its target rather than playing on, so it walks the shortest route to one new
/// thing and no further. The next leg CONTINUES FROM THERE, which is what a player does: at a
/// hub you take the next topic rather than leaving and walking back to the NPC.
///
/// IT RESTARTS ONLY WHERE THE GAME FORCES IT - the conversation has ended, or nothing unshown
/// can be reached from where the player stands while something still can from the start. Both
/// fall out of the same test: a leg from here finds nothing, so one is tried from the start
/// before the playthrough gives up. [`Leg::restarted`] says which legs began that way, and
/// consecutive legs between restarts are one sitting.
///
/// The state carries across legs and restarts alike - a `once` that has fired stays fired and a
/// counter keeps its value, which is what a save holds.
///
/// Within a leg the nearest unshown entry is the one fewest PRESSES away, since that is the
/// distance a player pays; a line that plays without waiting costs nothing.
///
/// `ceiling` bounds one leg's search in walk positions. [`Stop::OutOfRoom`] says a leg hit it,
/// and only [`Stop::Exhausted`] licenses reading the entries left unshown as unreachable.
pub fn greedy_playthrough(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    conversation: i32,
    ceiling: usize,
    passing: &HashSet<DialogueNodeId>,
    until: Until,
) -> Playthrough {
    let start = DialogueNodeId::new(conversation, 0);
    let mut done = Playthrough {
        legs: Vec::new(),
        shown: Vec::new(),
        refused: 0,
        blocked: Vec::new(),
        rolled: Vec::new(),
        ended: seed_state(graph, world),
        // THE START, until a leg moves it: a playthrough that walks nowhere leaves the player
        // looking at the entry the conversation opened on, waiting for nothing.
        ended_at: start,
        ended_waiting: false,
        stopped: Stop::Exhausted,
    };
    if graph.get(start).is_none() {
        done.stopped = Stop::NoStart;
        return done;
    }

    let walker = Walker {
        graph,
        world,
        context: CrawlContext::new(graph.symbols(), world),
        caps: CounterCaps::for_graph(COUNTER_CAP, graph),
    };
    // WHAT THE WORLD HAS ALREADY SHOWN counts as shown, so a playthrough can be continued from
    // a save rather than only from nothing.
    let mut shown: HashSet<DialogueNodeId> = graph
        .nodes()
        .filter(|node| world.is_seen(node.id))
        .map(|node| node.id)
        .collect();
    let mut state = seed_state(graph, world);
    let mut at = start;
    let mut line_up = false;
    // THE FIRST LEG IS A RESTART, because the player has not sat down yet.
    let mut restarting = true;
    // OPENING THE CONVERSATION SHOWS ITS START, whatever happens next: the game's line hook
    // fires for entry 0 as it opens, which is why `walk` records it. A conversation whose start
    // goes nowhere - one entered only from elsewhere - then reports one entry shown and no
    // legs, rather than nothing shown and an exhausted walk.
    if shown.insert(start) {
        done.shown.push(start);
    }

    loop {
        let found = walker.nearest_unshown(
            Standing {
                at: if restarting { start } else { at },
                state: &state,
                line_up: !restarting && line_up,
                opening: restarting,
            },
            &shown,
            ceiling,
            passing,
        );
        done.refused += found.refused;
        for blocked in found.blocked {
            if done.blocked.len() < BLOCKED_KEPT && !done.blocked.contains(&blocked) {
                done.blocked.push(blocked);
            }
        }
        // THE NEAREST SIGHTING OF EACH, since a check offered again later from further away is
        // the same check and the closest approach is what an escalation orders them by.
        for (check, presses) in found.rolled {
            match done.rolled.iter_mut().find(|(id, _)| *id == check) {
                Some((_, best)) => *best = (*best).min(presses),
                None => done.rolled.push((check, presses)),
            }
        }
        let Some(reached) = found.leg else {
            if found.out_of_room {
                done.stopped = Stop::OutOfRoom;
                return done;
            }
            // NOTHING FROM HERE. A player backs out and comes in again before concluding
            // there is nothing left, so the search is tried once from the start - and only a
            // restart that also finds nothing ends the playthrough.
            if restarting {
                return done;
            }
            restarting = true;
            continue;
        };
        for id in reached.leg.displayed() {
            if shown.insert(id) {
                done.shown.push(id);
            }
        }
        done.legs.push(Leg {
            restarted: restarting,
            ..reached.leg
        });
        state = reached.state;
        at = reached.at;
        line_up = reached.line_up;
        // ALL THREE TOGETHER, and before the early return: a caller that plays on from here
        // needs the position as well as the state, and recording the state alone would leave
        // the position a leg behind whenever the walk was stopped short.
        done.ended = state.clone();
        done.ended_at = at;
        done.ended_waiting = line_up;
        if until.shown.is_some_and(|wanted| done.shown.len() >= wanted) {
            return done;
        }
        restarting = false;
    }
}

/// One stage of a [`roll_escalation`]: the dice it declared, and the playthrough they gave.
pub struct Stage {
    /// The red and white checks this stage rolls a 12 on. Every other rolled check takes a 2.
    ///
    /// It GROWS BY ONE a stage, so a stage's number is how many checks the player had to win.
    pub passing: Vec<DialogueNodeId>,
    /// The check this stage added, and `None` for the first, which passes none.
    pub added: Option<DialogueNodeId>,
    pub walk: Playthrough,
}

/// Every playthrough of one conversation as the dice are conceded one at a time.
///
/// ## The rule
///
/// The first stage rolls a 2 everywhere, so every red and white check fails, and walks to
/// exhaustion. Each stage after it concedes ONE more check - the closest still being failed,
/// by the fewest presses the last stage ever stood at one - and walks to exhaustion again from
/// a fresh start.
///
/// RED CHECKS TOO, although the game never lets a player retry one: conceding a red stands for
/// reloading a save from before the conversation, which is a thing players do and the only way
/// the content behind a failed red is ever seen.
///
/// PASSIVE CHECKS ARE NOT IN THIS. They are not rolled - the character sheet decides them, and
/// the world already answers them from the save. Only a check with a die has two ways to go.
///
/// ## Why it terminates, and why each stage is worth keeping
///
/// The conceded set only grows and it is bounded by the checks the conversation holds, so the
/// schedule is finite. Every stage is a playthrough somebody could have had, and the stages
/// differ in exactly the way that matters: stage n is what a player sees having won n rolls.
///
/// A FAILED CHECK IS NOT A CLOSED DOOR, which is why stage one is not empty. A check that fails
/// is still entered and still says its failure line, and the graph gives some of them failure
/// actions; what a failure closes is the check itself - permanently, for a red or for a white
/// carrying a flag - and `oracle::enter_rolled` holds that rule.
pub fn roll_escalation(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    conversation: i32,
    ceiling: usize,
) -> Vec<Stage> {
    let mut passing: HashSet<DialogueNodeId> = HashSet::new();
    let mut added = None;
    let mut stages = Vec::new();

    loop {
        let walk = greedy_playthrough(
            graph,
            world,
            conversation,
            ceiling,
            &passing,
            Until::default(),
        );
        // THE CLOSEST STILL FAILING, and ties go to the smaller entry so two runs of this
        // concede the same check in the same order.
        let next = walk
            .rolled
            .iter()
            .filter(|(check, _)| !passing.contains(check))
            .min_by_key(|(check, presses)| (*presses, check.conversation_id, check.entry_id))
            .map(|(check, _)| *check);

        stages.push(Stage {
            passing: sorted(&passing),
            added,
            walk,
        });

        let Some(check) = next else {
            return stages;
        };
        passing.insert(check);
        added = Some(check);
    }
}

/// A set of entries in a deterministic order, since `HashSet` has none.
fn sorted(ids: &HashSet<DialogueNodeId>) -> Vec<DialogueNodeId> {
    let mut all: Vec<DialogueNodeId> = ids.iter().copied().collect();
    all.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));
    all
}

/// Where the player is standing when a leg's search begins.
struct Standing<'s> {
    at: DialogueNodeId,
    state: &'s LookAheadState,
    /// Whether `at` is a line waiting for a continue.
    line_up: bool,
    /// Whether this is a fresh sitting, whose first step records the conversation's start the
    /// way [`walk_inputs`] does. A leg that continues has already recorded where it stands.
    opening: bool,
}

/// A menu the player is standing at, and what it took to get there from where a playthrough
/// stopped.
#[derive(Debug, Clone)]
pub struct AtAMenu {
    /// The options, in the order the game draws them: what a request would ask about.
    pub menu: Vec<DialogueNodeId>,
    /// The steps played to reach it, which extend the playthrough's own walk.
    pub steps: Vec<Step>,
    /// The state on arrival - lines said on the way have fired their actions.
    pub state: LookAheadState,
}

impl AtAMenu {
    /// Every entry stepped through on the way, for appending to a walk.
    pub fn encountered(&self) -> Vec<DialogueNodeId> {
        self.steps
            .iter()
            .flat_map(|step| step.encountered.iter().copied())
            .collect()
    }

    /// The ones put on screen on the way.
    pub fn displayed(&self) -> Vec<DialogueNodeId> {
        self.steps
            .iter()
            .flat_map(|step| step.displayed.iter().copied())
            .collect()
    }
}

/// Plays on from where a playthrough stopped until a menu is on screen.
///
/// ## Why a playthrough does not already end at one
///
/// A leg stops at the entry it was walked for, which is a line far more often than a menu. That
/// is right for the walk - it went there to see one new thing - and wrong for anything that
/// wants to ask what the player is being OFFERED, because a menu is what a request is about.
/// This is the few presses between the two.
///
/// ## What it does and does not decide
///
/// Lines are taken as they come and a waiting one costs an [`Input::Enter`], exactly as
/// [`walk_inputs`] presses them. NOTHING IS CHOSEN: the first menu ends it, so no option is
/// taken and no roll is needed. `None` where the conversation ends first, or where the walk
/// refuses a position it cannot decide, or where `most` presses pass without a menu - in each
/// case there is no menu to be standing at, which is an answer rather than a failure.
pub fn on_to_a_menu(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    from: &Playthrough,
    most: usize,
) -> Option<AtAMenu> {
    let walker = Walker {
        graph,
        world,
        context: CrawlContext::new(graph.symbols(), world),
        caps: CounterCaps::for_graph(COUNTER_CAP, graph),
    };
    let mut at = from.ended_at;
    let mut line_up = from.ended_waiting;
    let mut state = from.ended.clone();
    let mut steps = Vec::new();

    for _ in 0..most {
        match walker.next(at, &state).ok()? {
            Next::Menu(options) => {
                // THE MENU'S OWN WAY IN IS ON THE WALK, as `walk` records it: composing the menu
                // expands the groups between here and the options, so the hub it hangs off is
                // passed whether or not anything is chosen from it.
                let drawn: Vec<DialogueNodeId> = options.iter().map(|o| o.id).collect();
                let mut entered = Vec::new();
                for option in &options {
                    for &id in &option.via {
                        if entered.last() != Some(&id) {
                            entered.push(id);
                        }
                    }
                }
                if !entered.is_empty() {
                    steps.push(Step {
                        input: None,
                        at,
                        menu: drawn.clone(),
                        rolled: None,
                        encountered: entered,
                        displayed: Vec::new(),
                    });
                }
                return Some(AtAMenu {
                    menu: drawn,
                    steps,
                    state,
                });
            }
            Next::Line(line) => {
                let mut encountered = line.via.clone();
                encountered.push(line.id);
                steps.push(Step {
                    input: line_up.then_some(Input::Enter),
                    at,
                    menu: Vec::new(),
                    rolled: None,
                    encountered,
                    displayed: vec![line.id],
                });
                state = walker.take(&line, &state);
                at = line.id;
                line_up = true;
            }
            Next::End => return None,
        }
    }
    None
}

/// One walk position a leg's search reached, and how it got there.
struct Reached {
    at: DialogueNodeId,
    state: LookAheadState,
    /// Whether `at` is a line waiting for a continue.
    line_up: bool,
    parent: Option<usize>,
    step: Option<Step>,
    presses: usize,
}

/// A leg, and where it left the player.
struct Reaching {
    leg: Leg,
    state: LookAheadState,
    at: DialogueNodeId,
    line_up: bool,
}

/// What a leg's search found.
struct Found {
    leg: Option<Reaching>,
    refused: usize,
    blocked: Vec<(DialogueNodeId, String)>,
    rolled: Vec<(DialogueNodeId, usize)>,
    out_of_room: bool,
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
    caps: CounterCaps,
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
                    // A HELD LINE IS ANSWERED BEFORE ITS MENU IS ON OFFER: the continue is spent
                    // here, and the same position is evaluated again with nothing held. The rule
                    // is `menu_waits_on`, which the leg search below asks too.
                    if self.menu_waits_on(at, line_up) {
                        match keys.next() {
                            Some((_, Input::Enter)) => {}
                            Some((index, key)) => {
                                return Err(format!(
                                    "input {} is \"{key}\", but {at} holds the screen until it \
                                     is answered with \"{ENTER}\"",
                                    index + 1
                                ));
                            }
                            None if to_first_menu => {}
                            None => {
                                return Err(format!(
                                    "the inputs end at {at}, which holds the screen until it is \
                                     answered with \"{ENTER}\", rather than at a menu"
                                ));
                            }
                        }
                        line_up = false;
                        continue;
                    }

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
                    line_up = self.owes_continue(chosen.id, false);
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

    /// The fewest presses from the conversation's start to something not in `shown`.
    ///
    /// A NOUGHT-ONE SEARCH, because a line that plays without waiting costs no press and a
    /// continue or a choice costs one, so the queue takes a free step at the front and a paid
    /// one at the back. `oracle::choice_distances` counts choices over links the same way.
    ///
    /// A position the walk will not step past is DROPPED RATHER THAN FATAL - it refuses where
    /// it cannot decide what the game would show, and one undecided line does not say the rest
    /// of the conversation is unreachable. The count comes back so a reader can see how much
    /// of it this world could not answer.
    fn nearest_unshown(
        &self,
        standing: Standing<'_>,
        shown: &HashSet<DialogueNodeId>,
        ceiling: usize,
        passing: &HashSet<DialogueNodeId>,
    ) -> Found {
        let Standing {
            at: start,
            state: from,
            line_up: from_line_up,
            opening,
        } = standing;
        // THE START FIRST ON A FRESH SITTING, for the reason `walk` gives: the game reports
        // entry 0 as the conversation opens, and a walk without it loses every hub passed
        // before the first line shown. A leg that continues is already standing there, and
        // recording it again would double it in the walk.
        let opening = opening.then(|| Step {
            input: None,
            at: start,
            menu: Vec::new(),
            rolled: None,
            encountered: vec![start],
            displayed: vec![start],
        });
        let mut reached = vec![Reached {
            at: start,
            state: from.clone(),
            line_up: from_line_up,
            parent: None,
            step: opening,
            presses: 0,
        }];
        let mut best: HashMap<(DialogueNodeId, LookAheadState, bool), usize> =
            HashMap::from([((start, from.clone(), from_line_up), 0)]);
        let mut pending: VecDeque<usize> = VecDeque::from([0]);
        let mut found = Found {
            leg: None,
            refused: 0,
            blocked: Vec::new(),
            rolled: Vec::new(),
            out_of_room: false,
        };
        // THE BEST SO FAR, and the search runs on past it only while the queue can still
        // produce a position of the same cost - which is what makes the winner the nearest
        // rather than the first one the queue happened to hand over.
        let mut winner: Option<(usize, DialogueNodeId, usize)> = None;

        while let Some(here) = pending.pop_front() {
            let (at, state, line_up, presses) = {
                let step = &reached[here];
                (step.at, step.state.clone(), step.line_up, step.presses)
            };
            if best.get(&(at, state.clone(), line_up)) != Some(&presses) {
                continue;
            }
            if winner.as_ref().is_some_and(|(cost, _, _)| presses > *cost) {
                break;
            }
            if reached.len() >= ceiling {
                found.out_of_room = true;
                break;
            }

            let moves = match self.next(at, &state) {
                Ok(next) => next,
                // UNDECIDED, so this position is dropped. See the doc above.
                Err(why) => {
                    found.refused += 1;
                    if found.blocked.len() < BLOCKED_KEPT {
                        found.blocked.push((at, why));
                    }
                    continue;
                }
            };
            // A HELD LINE IS ANSWERED BEFORE ITS MENU IS ON OFFER, and answering it is a move of
            // its own: one continue, spent where the player stands, displaying nothing new. The
            // position it reaches is this one with nothing held, which the search tells apart
            // from this one BY `line_up` - already part of the key, so the two do not collapse
            // and the continue is not spent twice. See `index::sequence_holds_the_screen`.
            if matches!(moves, Next::Menu(_))
                && self.menu_waits_on(at, line_up)
                && self.hold_answered(at, &state, presses, here, &mut best, &mut reached)
            {
                pending.push_back(reached.len() - 1);
                continue;
            }

            let taken: Vec<(Option<Input>, Vec<DialogueNodeId>, &Offered, bool)> = match &moves {
                Next::End => Vec::new(),
                Next::Line(line) => {
                    vec![(line_up.then_some(Input::Enter), Vec::new(), line, true)]
                }
                Next::Menu(options) => {
                    let drawn: Vec<DialogueNodeId> = options.iter().map(|o| o.id).collect();
                    options
                        .iter()
                        .enumerate()
                        .filter(|(_, option)| can_afford(self.node(option.id), &state))
                        .map(|(index, option)| {
                            let input = if options.len() == 1 {
                                Input::Enter
                            } else {
                                Input::Choose(index + 1)
                            };
                            (Some(input), drawn.clone(), option, false)
                        })
                        .collect()
                }
            };

            for (input, menu, offered, is_line) in taken {
                let cost = presses + usize::from(input.is_some());
                let node = self.node(offered.id);
                // THE DIE, WHERE THERE IS ONE. A red or white check is entered either way; which
                // way is not the walk's to decide, so it is declared - `passing` names the checks
                // this playthrough rolls a 12 on and every other rolled check takes a 2.
                let rolled = node.is_rolled().then(|| passing.contains(&offered.id));
                if rolled.is_some() {
                    found.rolled.push((offered.id, cost));
                }
                let Some(next_state) = self.take_rolled(offered, &state, rolled) else {
                    continue;
                };
                let mut encountered = offered.via.clone();
                encountered.push(offered.id);
                let step = Step {
                    input,
                    at,
                    menu,
                    rolled,
                    encountered,
                    displayed: vec![offered.id],
                };
                let waiting = self.owes_continue(offered.id, is_line);
                let key = (offered.id, next_state.clone(), waiting);
                if best.get(&key).is_some_and(|already| *already <= cost) {
                    continue;
                }
                best.insert(key, cost);
                let fresh = !shown.contains(&offered.id);
                reached.push(Reached {
                    at: offered.id,
                    state: next_state,
                    line_up: waiting,
                    parent: Some(here),
                    step: Some(step),
                    presses: cost,
                });
                let index = reached.len() - 1;
                // DETERMINISTIC AMONG TIES, so two runs of this rank the same entries the
                // same way. `DialogueNodeId` is not `Ord`, so the tie-break is spelled out.
                if fresh
                    && winner.as_ref().is_none_or(|(best_cost, best_id, _)| {
                        (cost, offered.id.conversation_id, offered.id.entry_id)
                            < (*best_cost, best_id.conversation_id, best_id.entry_id)
                    })
                {
                    winner = Some((cost, offered.id, index));
                }
                if cost == presses {
                    pending.push_front(index);
                } else {
                    pending.push_back(index);
                }
            }
        }

        if let Some((_, target, index)) = winner {
            let mut steps = Vec::new();
            let mut walk = Some(index);
            while let Some(current) = walk {
                if let Some(step) = reached[current].step.clone() {
                    steps.push(step);
                }
                walk = reached[current].parent;
            }
            steps.reverse();
            found.leg = Some(Reaching {
                leg: Leg {
                    target,
                    // Set by the caller, which is what knows whether the player sat down.
                    restarted: false,
                    steps,
                },
                state: reached[index].state.clone(),
                at: reached[index].at,
                line_up: reached[index].line_up,
            });
        }
        found
    }

    fn node(&self, id: DialogueNodeId) -> &LookAheadNode {
        self.graph
            .get(id)
            .expect("an offered entry is in the graph")
    }

    /// Enters an offered entry, taking a declared branch where it rolls.
    ///
    /// `rolled` is `None` for an entry that does not roll, and otherwise says which way the die
    /// went. `None` COMES BACK where the branch does not exist: a red check the world says can
    /// never pass has no success branch, and a check already resolved has neither, since
    /// `oracle::enter_rolled` shuts one for the rest of the walk.
    ///
    /// THE ORDER IS THE ONE `enter_rolled` BUILDS, not a re-derivation of the roll: success
    /// first where `world::roll_may_succeed` allows one, then failure. Reading it any other way
    /// would take the failing branch for a passing one on a red check a thought has closed.
    fn take_rolled(
        &self,
        offered: &Offered,
        state: &LookAheadState,
        rolled: Option<bool>,
    ) -> Option<LookAheadState> {
        let Some(pass) = rolled else {
            return Some(self.take(offered, state));
        };
        let mut entered = state.clone();
        for &id in &offered.via {
            entered = charge(self.node(id), &entered, &self.caps, self.world);
        }
        let node = self.node(offered.id);
        let ways = enter(node, &entered, &self.context, &self.caps);
        let may_succeed = crate::world::roll_may_succeed(node, self.world);
        match (pass, may_succeed) {
            (true, true) => ways.into_iter().next(),
            (true, false) => None,
            (false, true) => ways.into_iter().nth(1),
            (false, false) => ways.into_iter().next(),
        }
    }

    /// Enters an offered entry: the groups on the way to it, then the entry itself.
    fn take(&self, offered: &Offered, state: &LookAheadState) -> LookAheadState {
        let mut next = state.clone();
        for &id in offered.via.iter().chain(std::iter::once(&offered.id)) {
            next = charge(self.node(id), &next, &self.caps, self.world);
        }
        next
    }

    /// Whether a continue is owed before the MENU after `at` is on offer.
    ///
    /// ## Why both walkers ask this rather than each deciding it
    ///
    /// A menu normally composes beside the line in front of it and costs nothing to reach. An
    /// entry whose sequence RUNS is the exception: it stays up until it is answered, and the
    /// menu arrives after. THE TWO WALKERS BELOW MUST AGREE ABOUT THAT. One walks an explicit
    /// list of presses and answers whether a scenario's inputs fit; the other searches for the
    /// presses that reach an entry. If they count a conversation differently, a scenario and a
    /// playthrough of the same ground disagree - which is the shape of the bug this rule was
    /// added for, arriving from the inside instead. See de-oaaq and de-f739.
    fn menu_waits_on(&self, at: DialogueNodeId, line_up: bool) -> bool {
        line_up && self.node(at).holds_the_screen
    }

    /// Whether arriving at `id` leaves a continue owed.
    ///
    /// A line always owes one. An entry reached by CHOOSING it owes one only when its own
    /// sequence holds the screen, since the conversation otherwise carries straight on into
    /// whatever answers the choice. The other half of [`Self::menu_waits_on`], and shared for
    /// the same reason.
    fn owes_continue(&self, id: DialogueNodeId, is_line: bool) -> bool {
        is_line || self.node(id).holds_the_screen
    }

    /// Records the continue that answers a line holding the screen, as a move of its own.
    ///
    /// It reaches the SAME entry in the SAME state with nothing held, costing one press and
    /// displaying nothing: the line was already on screen, and answering it puts no new entry
    /// there. Answers whether it was worth recording - a position already reached for no more
    /// is left alone, exactly as any other move is.
    #[allow(clippy::too_many_arguments)]
    fn hold_answered(
        &self,
        at: DialogueNodeId,
        state: &LookAheadState,
        presses: usize,
        from: usize,
        best: &mut HashMap<(DialogueNodeId, LookAheadState, bool), usize>,
        reached: &mut Vec<Reached>,
    ) -> bool {
        let cost = presses + 1;
        let key = (at, state.clone(), false);
        if best.get(&key).is_some_and(|already| *already <= cost) {
            return false;
        }
        best.insert(key, cost);
        reached.push(Reached {
            at,
            state: state.clone(),
            line_up: false,
            parent: Some(from),
            step: Some(Step {
                input: Some(Input::Enter),
                at,
                menu: Vec::new(),
                rolled: None,
                encountered: Vec::new(),
                displayed: Vec::new(),
            }),
            presses: cost,
        });
        true
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
        let world = TestWorld::declaring_nothing();
        let walk = walked(two_lines_then_a_menu(), &world, Some(&[Input::Enter])).unwrap();
        assert_eq!(walk.encountered, nodes(&[0, 1, 2]));

        let error = walked(two_lines_then_a_menu(), &world, Some(&[])).unwrap_err();
        assert!(error.contains("line waiting"), "{error}");
    }

    #[test]
    fn no_inputs_presses_enter_to_the_first_menu() {
        let walk = walked(
            two_lines_then_a_menu(),
            &TestWorld::declaring_nothing(),
            None,
        )
        .unwrap();
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
            &TestWorld::declaring_nothing(),
            Some(&[Input::Enter, Input::Choose(2)]),
        )
        .unwrap();
        assert_eq!(walk.encountered, nodes(&[0, 1, 2, 4, 5]));
        assert_eq!(walk.menu, nodes(&[6]));
    }

    #[test]
    fn inputs_that_do_not_fit_the_screen_fail_by_name() {
        let world = TestWorld::declaring_nothing();
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
            &TestWorld::declaring_nothing(),
            Some(&[Input::Enter, Input::Enter]),
        )
        .unwrap();
        assert_eq!(walk.encountered, nodes(&[0, 1, 2, 3]));
        assert_eq!(walk.menu, nodes(&[4, 5]));
    }

    #[test]
    fn a_line_on_offer_wins_over_the_options_and_groups_open_in_place() {
        let world =
            TestWorld::declaring_nothing().set_variable("shut", GuardValue::from_boolean(false));
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

    /// A guard nothing can decide is refused only where it changes what is shown.
    ///
    /// UNDECIDED BY A QUERY, which is the only thing that still can be. A world answers
    /// every VARIABLE - the ones it was told, and the rest from its table - so a variable
    /// cannot be the undecided half of anything. Nobody can say what a query about the world
    /// would have returned, and a world that was not told is honestly unable to say.
    #[test]
    fn an_undecided_guard_is_refused_only_where_it_changes_what_is_shown() {
        let entries = vec![
            Entry::new(0).links(&[1, 2]),
            Entry::new(1).player().guard("IsKimHere()"),
            Entry::new(2).links(&[3]),
            Entry::new(3).player(),
        ];
        let world = TestWorld::declaring_nothing();
        let walk = walked(entries, &world, None).unwrap();
        assert_eq!(walk.encountered, nodes(&[0, 2]));

        let entries = vec![
            Entry::new(0).links(&[1, 2]),
            Entry::new(1).guard("IsKimHere()"),
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
        let error = walked(
            entries,
            &TestWorld::declaring_nothing(),
            Some(&[Input::Choose(1)]),
        )
        .unwrap_err();
        assert!(error.contains("rolled check"), "{error}");
    }

    /// A hub with three topics off it, each one line deep, and a line past the hub that only
    /// opens once a variable is set. The shape a greedy playthrough is about: a player standing
    /// at a hub takes the topics one at a time without leaving.
    fn a_hub_of_three() -> Vec<Entry> {
        vec![
            Entry::new(0).links(&[1]),
            Entry::new(1).group().links(&[2, 4, 6]),
            Entry::new(2).player().links(&[3]),
            Entry::new(3).links(&[1]),
            Entry::new(4).player().links(&[5]),
            Entry::new(5).links(&[1]),
            Entry::new(6).player().links(&[7]),
            Entry::new(7).links(&[1]),
        ]
    }

    fn playthrough(entries: Vec<Entry>, world: &TestWorld) -> Playthrough {
        greedy_playthrough(
            &graph(entries),
            world,
            crate::test_graph::DEFAULT_CONVERSATION,
            10_000,
            &HashSet::new(),
            Until::default(),
        )
    }

    /// Every entry a walk can reach is reached, and the walk says so rather than stopping.
    #[test]
    fn a_playthrough_shows_everything_it_can_reach() {
        let done = playthrough(a_hub_of_three(), &TestWorld::declaring_nothing());

        assert_eq!(done.stopped, Stop::Exhausted);
        assert_eq!(done.refused, 0, "{:?}", done.blocked);
        let shown: HashSet<i32> = done.shown.iter().map(|id| id.entry_id).collect();
        assert_eq!(
            shown,
            HashSet::from([0, 2, 3, 4, 5, 6, 7]),
            "the start, the three topics and what each leads to; 1 is a group and never shown"
        );
    }

    /// A LEG STOPS AT ITS TARGET AND THE NEXT ONE CARRIES ON FROM THERE. Standing at a hub, a
    /// player takes the next topic rather than leaving and walking back to the conversation's
    /// start - so only the first leg is a restart.
    #[test]
    fn legs_continue_from_where_the_last_one_stopped() {
        let done = playthrough(a_hub_of_three(), &TestWorld::declaring_nothing());

        assert!(
            done.legs[0].restarted,
            "the player has not sat down before the first leg"
        );
        assert!(
            done.legs[1..].iter().all(|leg| !leg.restarted),
            "everything else is reachable from the hub without leaving: {:?}",
            done.legs
                .iter()
                .map(|leg| (leg.target.entry_id, leg.restarted))
                .collect::<Vec<_>>()
        );
    }

    /// THE GAME FORCES THE RESTART, not a rule of its own: a conversation that ends leaves
    /// nowhere to continue from, so the next leg begins at the start again.
    #[test]
    fn a_conversation_that_ends_forces_the_next_leg_to_restart() {
        // Two topics off a hub, and the first is a dead end that ends the conversation.
        let entries = vec![
            Entry::new(0).links(&[1]),
            Entry::new(1).group().links(&[2, 3]),
            Entry::new(2).player(),
            Entry::new(3).player().links(&[4]),
            Entry::new(4).links(&[1]),
        ];
        let done = playthrough(entries, &TestWorld::declaring_nothing());

        assert_eq!(done.stopped, Stop::Exhausted);
        let restarts = done.legs.iter().filter(|leg| leg.restarted).count();
        assert!(
            restarts >= 2,
            "the dead end ends the conversation, so what follows it begins again: {:?}",
            done.legs
                .iter()
                .map(|leg| (leg.target.entry_id, leg.restarted))
                .collect::<Vec<_>>()
        );
    }

    /// NEAREST IS COUNTED IN PRESSES, which is what a player pays. A line that plays without
    /// waiting costs nothing, so it is reached before an option that costs a choice.
    #[test]
    fn the_nearest_unshown_entry_is_the_one_fewest_presses_away() {
        // 1 says itself with a menu behind it, so reaching 1 costs no press; 2 and 3 each cost
        // one.
        let entries = vec![
            Entry::new(0).links(&[1]),
            Entry::new(1).links(&[2, 3]),
            Entry::new(2).player(),
            Entry::new(3).player(),
        ];
        let done = playthrough(entries, &TestWorld::declaring_nothing());

        assert_eq!(done.legs[0].target, node(1), "the free line comes first");
        assert!(
            done.legs[0].inputs().is_empty(),
            "and it costs nothing to reach"
        );
        assert_eq!(done.legs[1].inputs().len(), 1, "an option costs one press");
    }

    /// THE KEYPRESSES ARE THE WITNESS, so where they can be fed back they have to walk the same
    /// route: this leg's inputs go through `walk_inputs` and reach what the leg reached.
    ///
    /// A PREFIX RATHER THAN THE SAME LIST, and the difference is each side doing its job. A leg
    /// stops AT ITS TARGET, having gone there to see one new thing; `walk_inputs` must end at a
    /// MENU, so it plays on past the target until one comes up. The leg is the beginning of the
    /// replay, not the whole of it.
    ///
    /// WHICH IS WHY THIS IS A GRAPH WITH A MENU AHEAD rather than a claim about the dataset. On
    /// real conversations a session usually ends on a terminal line, there is no menu to play on
    /// to, and `walk_inputs` refuses the sequence outright - measured over five groups, none was
    /// accepted. What this pins is that the keys are RIGHT, not that the offline runner will
    /// take them.
    #[test]
    fn a_legs_keypresses_walk_the_same_route_where_they_can_be_fed_back() {
        let built = graph(a_hub_of_three());
        let done = greedy_playthrough(
            &built,
            &TestWorld::declaring_nothing(),
            crate::test_graph::DEFAULT_CONVERSATION,
            10_000,
            &HashSet::new(),
            Until::default(),
        );

        let first = &done.legs[0];
        let replay = walk_inputs(
            &built,
            &TestWorld::declaring_nothing(),
            crate::test_graph::DEFAULT_CONVERSATION,
            Some(&first.inputs()),
        )
        .expect("the leg's own keypresses are walkable");
        let walked = first.encountered();
        assert_eq!(
            replay.encountered.get(..walked.len()),
            Some(walked.as_slice()),
            "the replay begins by walking exactly what the leg walked"
        );
        assert!(
            replay.encountered.len() > walked.len(),
            "and carries on to a menu, which the leg had no reason to reach"
        );
    }

    /// A STEP CARRIES WHAT ITS NUMBER MEANT. An `Input::Choose(n)` indexes the menu that was on
    /// screen, so the step records that menu rather than leaving a reader to recover it.
    #[test]
    fn a_step_records_the_menu_its_number_indexed() {
        let done = playthrough(a_hub_of_three(), &TestWorld::declaring_nothing());

        let chose = done
            .legs
            .iter()
            .flat_map(|leg| leg.steps.iter())
            .find(|step| matches!(step.input, Some(Input::Choose(_))))
            .expect("a hub of three is chosen from");
        let Some(Input::Choose(number)) = chose.input else {
            unreachable!()
        };
        assert_eq!(
            chose.menu.get(number - 1),
            chose.displayed.first(),
            "the number indexes the menu the step recorded, from one"
        );
    }

    /// STATE CARRIES ACROSS LEGS, which is what a save holds: an entry the walk has already
    /// been shown is not shown again, and its one-time effects stay fired.
    #[test]
    fn what_the_world_has_shown_is_not_walked_to_again() {
        let already = TestWorld::declaring_nothing().set_seen(node(4), true);
        let done = greedy_playthrough(
            &graph(a_hub_of_three()),
            &already,
            crate::test_graph::DEFAULT_CONVERSATION,
            10_000,
            &HashSet::new(),
            Until::default(),
        );

        assert!(
            !done.legs.iter().any(|leg| leg.target == node(4)),
            "4 is already shown, so no leg is walked to reach it"
        );
    }

    /// STOPPING SHORT IS WHAT A PROFILE USES, and it stops on entries shown rather than legs,
    /// since one leg can show several.
    #[test]
    fn a_walk_can_be_stopped_with_entries_still_to_come() {
        let built = graph(a_hub_of_three());
        let whole = greedy_playthrough(
            &built,
            &TestWorld::declaring_nothing(),
            crate::test_graph::DEFAULT_CONVERSATION,
            10_000,
            &HashSet::new(),
            Until::default(),
        );
        let wanted = whole.shown.len() - 2;

        let stopped = greedy_playthrough(
            &built,
            &TestWorld::declaring_nothing(),
            crate::test_graph::DEFAULT_CONVERSATION,
            10_000,
            &HashSet::new(),
            Until {
                shown: Some(wanted),
            },
        );

        assert!(stopped.shown.len() >= wanted);
        assert!(
            stopped.shown.len() < whole.shown.len(),
            "it stopped short of the whole walk"
        );
        assert_eq!(
            stopped.shown,
            whole.shown[..stopped.shown.len()],
            "and it is a PREFIX of the same walk, which is what makes the rest the deepest"
        );
    }

    /// A rolled check behind a hub, with a line past it that only the pass reaches.
    fn a_check_off_a_hub() -> Vec<Entry> {
        vec![
            Entry::new(0).links(&[1]),
            Entry::new(1).group().links(&[2, 5]),
            Entry::new(2)
                .player()
                .kind(DialogueCheckKind::White)
                .flag("roll")
                .links(&[3, 4]),
            Entry::new(3),
            Entry::new(4),
            Entry::new(5).player().links(&[6]),
            Entry::new(6).links(&[1]),
        ]
    }

    /// STAGE ZERO ROLLS A TWO EVERYWHERE, and a failed check is still ENTERED - it says its
    /// failure line - which is why the first stage is not empty.
    #[test]
    fn the_first_stage_fails_every_roll_and_still_walks_the_check() {
        let stages = roll_escalation(
            &graph(a_check_off_a_hub()),
            &TestWorld::declaring_nothing(),
            crate::test_graph::DEFAULT_CONVERSATION,
            10_000,
        );

        let first = &stages[0];
        assert!(first.passing.is_empty(), "stage zero concedes nothing");
        assert!(
            first.walk.shown.iter().any(|id| *id == node(2)),
            "the check itself is entered and shown even though it fails"
        );
        assert!(
            first
                .walk
                .legs
                .iter()
                .flat_map(|leg| leg.steps.iter())
                .any(|step| step.rolled == Some(false)),
            "and the step records that the die was taken as a two"
        );
    }

    /// EACH LATER STAGE CONCEDES ONE MORE, so the set only grows and the schedule terminates.
    #[test]
    fn each_stage_concedes_one_more_check() {
        let stages = roll_escalation(
            &graph(a_check_off_a_hub()),
            &TestWorld::declaring_nothing(),
            crate::test_graph::DEFAULT_CONVERSATION,
            10_000,
        );

        assert!(stages.len() >= 2, "there is a check to concede");
        for (before, after) in stages.iter().zip(&stages[1..]) {
            assert_eq!(
                after.passing.len(),
                before.passing.len() + 1,
                "one at a time"
            );
            assert!(
                before.passing.iter().all(|id| after.passing.contains(id)),
                "and the set only grows"
            );
            assert!(
                after.added.is_some(),
                "a later stage names what it conceded"
            );
        }
    }

    /// A playthrough stops at the entry it went for, which is usually a line. Playing on reaches
    /// the menu a request would be about, pressing continues and choosing nothing.
    #[test]
    fn playing_on_stops_at_the_first_menu() {
        let entries = vec![
            Entry::new(0).links(&[1]),
            Entry::new(1).links(&[2]),
            Entry::new(2).links(&[3, 4]),
            Entry::new(3).player(),
            Entry::new(4).player(),
        ];
        let world = TestWorld::declaring_nothing();
        let built = graph(entries);
        let stopped = greedy_playthrough(
            &built,
            &world,
            crate::test_graph::DEFAULT_CONVERSATION,
            10_000,
            &HashSet::new(),
            Until { shown: Some(2) },
        );

        let standing = on_to_a_menu(&built, &world, &stopped, 64).expect("a menu is ahead");
        assert_eq!(standing.menu, nodes(&[3, 4]), "in drawn order");
        assert!(
            standing
                .steps
                .iter()
                .all(|step| !matches!(step.input, Some(Input::Choose(_)))),
            "nothing is chosen on the way to a menu"
        );
    }

    /// A conversation with no menu ahead of it has no menu to be standing at, which is an answer
    /// rather than a failure.
    #[test]
    fn playing_on_gives_nothing_where_the_conversation_ends() {
        let entries = vec![Entry::new(0).links(&[1]), Entry::new(1)];
        let world = TestWorld::declaring_nothing();
        let built = graph(entries);
        let done = greedy_playthrough(
            &built,
            &world,
            crate::test_graph::DEFAULT_CONVERSATION,
            10_000,
            &HashSet::new(),
            Until::default(),
        );

        assert!(on_to_a_menu(&built, &world, &done, 64).is_none());
    }
}
