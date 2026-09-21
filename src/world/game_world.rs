// SPDX-License-Identifier: MIT
//! The world the engine answers questions about, and the only one there is.
//!
//! ## One type, three ways of stating it
//!
//! A request carries a [`WorldRawData`] and the engine wraps it here. A test states one with
//! the builders below. A measurement takes a preset built from a committed save. They are the
//! same world - what differs is who filled the snapshot in.
//!
//! There used to be three types doing this, and the cost was that they disagreed: one of them
//! answered every variable it had not been told about as Unknown, which is what a symbolic
//! search cannot prune on, so a test world quietly measured a harder problem than the game
//! ever poses.
//!
//! ## It cannot be built without a table
//!
//! Every world carries an [`IVariableTable`], and that table answers EVERY name - see its own
//! documentation for why there is no "not declared" case for a world to handle. So a variable
//! lookup here always has an answer, and none of the constructors below can be reached without
//! saying where that answer comes from.

use std::sync::Arc;

use crate::bridge::{
    DataAnswer, DataKind, DataRequest, NodeRef, WireValue, WorldRawData, query_key,
    thought_state_kinds,
};
use crate::core::guard_value::{GuardValue, GuardValueKind};
use crate::core::state::VariableRef;
use crate::core::types::{DialogueNodeId, Ternary};
use crate::core::{equipment, inventory_tabs};
use crate::world::ILookAheadWorld;

/// A [`WorldRawData`], answering as a world.
///
/// ## The variable table behind it
///
/// A snapshot answers what the plugin could read. What it could not read reads Unknown,
/// which is permissive and correct for a genuinely unanswerable question - but a dialogue
/// variable nobody has written is not unanswerable, it is at the value the database
/// declares. Answering Unknown for it makes every guard over it undecidable; answering
/// boolean false for it makes every ordering comparison over a COUNTER undecidable, which
/// is the bug de-sze.5.4 exists about.
///
/// So a variable the snapshot could not answer falls back to its declared initial value,
/// which carries the right KIND as well as the right value. THERE IS ALWAYS A TABLE -
/// `variables.jsonl`, deployed beside the index - because an engine opened without one is
/// refused; see [`crate::service::Service::open`].
pub struct GameWorld {
    snapshot: WorldRawData,
    /// What a variable reads where the snapshot could not answer it.
    declared: Arc<dyn crate::world::IVariableTable>,
}

impl GameWorld {
    /// A world whose table declares nothing at all.
    ///
    /// NAMED FOR WHAT IT LACKS, so that a test about something else says out loud that it
    /// is measuring a world where no variable has a declared kind - a variable the snapshot
    /// cannot answer reads as the game's false here, where a declared one would answer with
    /// what the database says it starts as. The table itself is never absent: see
    /// [`crate::index::VariableTable::empty`].
    pub fn declaring_nothing(snapshot: WorldRawData) -> Self {
        Self {
            snapshot,
            declared: Arc::new(crate::index::VariableTable::empty()),
        }
    }

    /// The same, falling back to the database's declared variables.
    pub fn declaring(
        snapshot: WorldRawData,
        declared: Arc<dyn crate::world::IVariableTable>,
    ) -> Self {
        Self { snapshot, declared }
    }

    /// Puts the plugin's positional answers onto the names the engine asked under.
    ///
    /// ON THE WORLD RATHER THAN ON THE DATA, so that everything asking anything asks a world.
    /// The raw data is a bag of facts and answers nothing on its own; letting it be prepared
    /// separately meant a caller could hold a half-resolved bag and forget which it had.
    ///
    /// MUST RUN BEFORE THE WORLD IS ASKED. A positional list that is present and the wrong
    /// length is REFUSED rather than zipped as far as it goes: a caller answering a stale
    /// questions list would otherwise have every answer after the first difference land on the
    /// wrong variable, and the marker would be wrong with nothing to report it.
    pub fn resolve(&mut self, questions: &crate::bridge::Questions) -> Result<(), String> {
        self.snapshot.resolve(questions)
    }

    /// The data this world answers from, for a caller that KEEPS it rather than asks it.
    ///
    /// A disk cache and a fixture file hold raw data, not worlds - a world is that data plus a
    /// table, and the table is read once at startup rather than stored per group. So the way to
    /// get prepared data is to prepare a world and take it back out, which keeps
    /// [`Self::resolve`] the only thing that ever does the preparing.
    pub fn into_raw(self) -> WorldRawData {
        self.snapshot
    }

    /// A world told nothing and declaring nothing, to be stated with the builders below.
    ///
    /// WHAT A UNIT TEST WANTS: every variable reads false, every set is empty, and nothing is
    /// undecided except the queries and checks nobody can answer. It is a world a play could
    /// be in, rather than an absence of information.
    pub fn blank() -> Self {
        Self::declaring_nothing(WorldRawData::default())
    }

    pub fn with_money(mut self, money: i32) -> Self {
        self.snapshot.money = money;
        self
    }

    pub fn with_day_minutes(mut self, minutes: i32) -> Self {
        self.snapshot.day_minutes = minutes;
        self
    }

    pub fn with_day_counter(mut self, day: i32) -> Self {
        self.snapshot.day_counter = day;
        self
    }

    pub fn with_clock_locked(mut self, locked: bool) -> Self {
        self.snapshot.clock_locked = locked;
        self
    }

    pub fn with_red_checks_failing(mut self, failing: bool) -> Self {
        self.snapshot.red_checks_fail = failing;
        self
    }

    pub fn set_variable(mut self, name: &str, value: GuardValue) -> Self {
        self.snapshot
            .variables
            .insert(name.to_string(), WireValue::from(&value));
        self
    }

    /// A variable's value by name, for a caller with no symbol table to ask through.
    pub fn variable(&self, name: &str) -> GuardValue {
        let answered = self.snapshot.variables.get(name).map(GuardValue::from);
        match answered {
            Some(value) if value.kind() != GuardValueKind::Unknown => value,
            _ => self.declared.unset(name),
        }
    }

    /// Whether the player is carrying `name` when the search starts.
    ///
    /// A SET RATHER THAN A FLAG, so `false` removes the name rather than recording a denial -
    /// which is the same thing, since anything not in the set is not carried.
    pub fn set_item(mut self, name: &str, has: bool) -> Self {
        if has {
            self.snapshot.items.insert(name.to_string());
        } else {
            self.snapshot.items.remove(name);
        }
        self
    }

    /// The same for the thought cabinet, which `IsTHCPresent` asks about.
    pub fn set_thought(mut self, name: &str, gained: bool) -> Self {
        if gained {
            self.snapshot.thoughts.insert(name.to_string());
        } else {
            self.snapshot.thoughts.remove(name);
        }
        self
    }

    /// Which thoughts are being internalised, as the plugin enumerates them.
    ///
    /// STATING THE SET IS STATING THAT IT WAS READ, which is the difference between "none are
    /// cooking" and "nobody could say". An empty slice is the first, and is what a world that
    /// has opened the cabinet and found nothing looks like; leaving it out entirely is the
    /// second. `IsTHCCooking` is answered from here and from nowhere else.
    pub fn set_cooking<'a>(self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.set_thought_state(DataKind::ThoughtsCooking, names)
    }

    /// The same for thoughts already internalised, which `IsTHCFixed` asks about.
    pub fn set_fixed<'a>(self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.set_thought_state(DataKind::ThoughtsFixed, names)
    }

    fn set_thought_state<'a>(
        mut self,
        kind: DataKind,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let names: Vec<String> = names.into_iter().map(str::to_string).collect();
        // COOKING OR FIXED IS ALSO PRESENT. The game keeps `gainedThoughts` apart from the
        // cooking and fixed effects, and internalising a thought never takes it out of that
        // set - so a world that says a thought is being internalised and does not say it is
        // in the cabinet is one no play can be in, and `IsTHCPresent` would answer wrongly.
        self.snapshot.thoughts.extend(names.iter().cloned());
        self.snapshot
            .data
            .insert(DataRequest::set(kind), DataAnswer::of_names(names));
        self
    }

    /// One skill's damage, negative where damaged.
    pub fn set_damage(mut self, skill: &str, value: f64) -> Self {
        self.snapshot.data.insert(
            DataRequest::about(DataKind::SkillDamage, skill),
            DataAnswer::of_value(WireValue::Number { value }),
        );
        self
    }

    /// What a passive check's outcome is, where the world can state it.
    ///
    /// TWO SETS RATHER THAN THREE OUTCOMES, which is how the snapshot carries it: an entry in
    /// neither set is one nobody could state, and [`Ternary::Unknown`] puts it in neither.
    pub fn set_check_result(mut self, node: DialogueNodeId, result: Ternary) -> Self {
        let node = NodeRef::from(node);
        self.snapshot.checks_pass.remove(&node);
        self.snapshot.checks_fail.remove(&node);
        match result {
            Ternary::True => {
                self.snapshot.checks_pass.insert(node);
            }
            Ternary::False => {
                self.snapshot.checks_fail.insert(node);
            }
            Ternary::Unknown => {}
        }
        self
    }

    /// A passive check's skill and margin - see [`ILookAheadWorld::check_margin`].
    pub fn set_check_margin(mut self, node: DialogueNodeId, skill: &str, margin: i32) -> Self {
        let node = NodeRef::from(node);
        self.snapshot.check_margins.retain(|held| held.node != node);
        self.snapshot
            .check_margins
            .push(crate::bridge::CheckMargin {
                node,
                skill: skill.to_string(),
                margin,
            });
        self
    }

    pub fn set_seen(mut self, node: DialogueNodeId, seen: bool) -> Self {
        let node = NodeRef::from(node);
        if seen {
            self.snapshot.seen.insert(node);
        } else {
            self.snapshot.seen.remove(&node);
        }
        self
    }

    /// Puts `item` in an equipment slot.
    pub fn set_equipped(mut self, slot: &str, item: &str) -> Self {
        self.snapshot.data.insert(
            DataRequest::about(DataKind::EquippedInSlot, slot),
            DataAnswer::of_value(WireValue::Text {
                value: item.to_string(),
            }),
        );
        self
    }

    /// Answers a named world query, such as `IsKimHere`.
    pub fn set_query(mut self, name: &str, value: GuardValue) -> Self {
        self.snapshot
            .queries
            .insert(query_key(name, &[]), WireValue::from(&value));
        self
    }

    /// Answers a named world query with a boolean.
    pub fn set_query_bool(self, name: &str, value: bool) -> Self {
        self.set_query(name, GuardValue::from_boolean(value))
    }

    /// Answers a world query ASKED ABOUT SOMETHING, such as `IsTaskActive("TASK.x")`.
    ///
    /// A query is keyed by its call, arguments included, because that is what it is: the same
    /// function asked about two subjects is two questions, and answering both from one entry
    /// would state a world in which every task is active at once.
    pub fn set_query_about(mut self, name: &str, subject: &str, value: GuardValue) -> Self {
        let arguments = [GuardValue::from_text(subject.to_string())];
        self.snapshot
            .queries
            .insert(query_key(name, &arguments), WireValue::from(&value));
        self
    }

    /// Whether `subject` is in the set `kind` answered with.
    ///
    /// `None` where nothing answered that request at all, which stays Unknown rather than
    /// reading as "not in the set": a set nobody sent is not an empty set.
    fn in_set(&self, kind: DataKind, subject: &str) -> Option<bool> {
        let answer = self.snapshot.data.get(&DataRequest::set(kind))?;
        // NOT READ IS NOT EMPTY. A plugin that could not reach the cabinet sends an answer
        // saying so, and treating that as an empty set would answer "not in it" for
        // everything - which closes routes rather than opening them.
        if !answer.read {
            return None;
        }

        Some(answer.names.iter().any(|name| name == subject))
    }

    /// The item one equipment slot holds - empty for an empty slot - or `None` where the
    /// slot was not read.
    /// What one equipment slot holds - EMPTY WHERE NOTHING SAYS, rather than undecided.
    ///
    /// A slot nobody answered used to read as "not known", and a caller had to decide what to
    /// make of that - `Fitting::read` reads it as "might hold the item about to be lost", which
    /// unsettles a price nothing was ever going to move. Empty is a definite answer a play can
    /// be in, and it is the one an unanswered slot is overwhelmingly likely to be: the plugin
    /// is asked about the slots a group's guards name, so a slot it did not answer is one the
    /// game had nothing in.
    fn in_slot(&self, slot: &str) -> &str {
        let Some(answer) = self
            .snapshot
            .data
            .get(&DataRequest::about(DataKind::EquippedInSlot, slot))
        else {
            return "";
        };

        match &answer.value {
            WireValue::Text { value } if answer.read => value,
            _ => "",
        }
    }

    /// The names a per-subject, set-valued request answered with, or `None` where it was
    /// not read.
    fn names_about(&self, kind: DataKind, subject: &str) -> Option<Vec<String>> {
        let answer = self.snapshot.data.get(&DataRequest::about(kind, subject))?;
        answer.read.then(|| answer.names.clone())
    }
}

impl ILookAheadWorld for GameWorld {
    fn money(&self) -> i32 {
        self.snapshot.money
    }

    fn day_minutes(&self) -> i32 {
        self.snapshot.day_minutes
    }

    fn day_counter(&self) -> i32 {
        self.snapshot.day_counter
    }

    fn is_clock_locked(&self) -> bool {
        self.snapshot.clock_locked
    }

    fn get_variable(&self, variable: VariableRef<'_>) -> GuardValue {
        let name = variable.name();
        let answered = self.snapshot.variables.get(name).map(GuardValue::from);
        if let Some(value) = answered
            && value.kind() != GuardValueKind::Unknown
        {
            return value;
        }

        // The plugin could not read it, so the table answers - which it does for EVERY name,
        // so there is no case left here to get wrong. What it says an unwritten variable
        // starts as is a better answer than "no idea", and is the only one that gets a
        // counter's KIND right. See `IVariableTable`.
        self.declared.unset(name)
    }

    fn initially_has_item(&self, name: &str) -> bool {
        self.snapshot.items.contains(name)
    }

    fn initially_has_thought(&self, name: &str) -> bool {
        self.snapshot.thoughts.contains(name)
    }

    fn initial_damage(&self, skill: &str) -> Option<f64> {
        let answer = self
            .snapshot
            .data
            .get(&DataRequest::about(DataKind::SkillDamage, skill))?;
        if !answer.read {
            return None;
        }
        GuardValue::from(&answer.value).try_as_number()
    }

    fn item_in_slot(&self, slot: &str) -> Option<String> {
        Some(self.in_slot(slot).to_string())
    }

    fn items_in_group(&self, group: &str) -> Option<Vec<String>> {
        self.names_about(DataKind::ItemsInGroup, group)
    }

    fn initially_held_in_group(&self, group: &str) -> Option<Vec<String>> {
        self.names_about(DataKind::HeldItemsInGroup, group)
    }

    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue {
        let answer = (|| {
            // WHETHER A TAB HOLDS ANYTHING, and whether the scene is outdoors, as the plugin read
            // them. No query key behind either.
            let read_value = |request: DataRequest| match self.snapshot.data.get(&request) {
                Some(answer) if answer.read => GuardValue::from(&answer.value),
                _ => GuardValue::unknown(),
            };
            if let Some(tab) = inventory_tabs::tab_read_by(name) {
                return read_value(DataRequest::about(DataKind::TabHoldsItems, tab));
            }
            if name == crate::core::scene::IS_EXTERIOR {
                return read_value(DataRequest::set(DataKind::SceneIsOutside));
            }
            if name == crate::core::game_mode::WAS_GAME_BEATEN_IN_HARDCORE_MODE {
                return read_value(DataRequest::set(DataKind::HardcorePlaythroughCompleted));
            }
            // WHO IS WITH THE PLAYER, from the party flags the plugin read.
            let party_flag = |flag: &str| {
                let value = read_value(DataRequest::about(DataKind::PartyFlag, flag));
                (value.kind() == GuardValueKind::Boolean).then(|| value.boolean())
            };
            if let Some(here) = crate::core::party::answer(name, party_flag) {
                return here.map_or_else(GuardValue::unknown, GuardValue::from_boolean);
            }
            if name == crate::core::game_mode::IS_HARDCORE_MODE_ACTIVE {
                let mode = read_value(DataRequest::set(DataKind::GameMode));
                return if mode.kind() == GuardValueKind::Text {
                    GuardValue::from_boolean(mode.text() == crate::core::game_mode::HARDCORE)
                } else {
                    GuardValue::unknown()
                };
            }
            if let Some(skill) = crate::core::damage::skill_read_by(name) {
                let damage = read_value(DataRequest::about(DataKind::SkillDamage, skill));
                return damage
                    .try_as_number()
                    .map_or_else(GuardValue::unknown, |value| {
                        GuardValue::from_boolean(crate::core::damage::is_damaged(value))
                    });
            }

            // THE CABINET'S NARROW QUESTIONS, answered from the sets the plugin enumerated.
            // Nothing asks it to evaluate these any more - see `collect` - so there is no query
            // key to fall back to, and a set nobody sent leaves the question Unknown.
            if let Some(kinds) = thought_state_kinds(name) {
                let Some(subject) = arguments
                    .first()
                    .filter(|value| value.kind() == GuardValueKind::Text)
                    .map(|value| value.text())
                else {
                    return GuardValue::unknown();
                };

                let mut answered = false;
                for kind in kinds.iter().copied() {
                    match self.in_set(kind, subject) {
                        // IN ANY OF THEM IS ENOUGH, which is what cooking-or-fixed means and is
                        // the only case for the other two.
                        Some(true) => return GuardValue::from_boolean(true),
                        Some(false) => answered = true,
                        None => {}
                    }
                }

                return if answered {
                    GuardValue::from_boolean(false)
                } else {
                    GuardValue::unknown()
                };
            }

            // WHAT IS WORN, answered from the slots the plugin read. Like the cabinet, there is
            // no query key behind it to fall back to.
            let argument = match arguments {
                [value] if value.kind() == GuardValueKind::Text => Some(value.text()),
                _ => None,
            };
            if let Some(worn) = equipment::answer(
                name,
                argument,
                |slot| Some(self.in_slot(slot)),
                |group| self.items_in_group(group),
            ) {
                return worn.map_or_else(GuardValue::unknown, GuardValue::from_boolean);
            }

            self.snapshot
                .queries
                .get(&query_key(name, arguments))
                .map(GuardValue::from)
                .unwrap_or_else(GuardValue::unknown)
        })();
        if answer.kind() == GuardValueKind::Unknown {
            unanswered_warning(&query_key(name, arguments));
        }
        answer
    }

    fn check_passes(&self, node: DialogueNodeId) -> Ternary {
        let node = NodeRef::from(node);
        if self.snapshot.checks_pass.contains(&node) {
            Ternary::True
        } else if self.snapshot.checks_fail.contains(&node) {
            Ternary::False
        } else {
            unanswered_warning(&format!(
                "whether the check on {}:{} passes",
                node.conversation, node.entry
            ));
            Ternary::Unknown
        }
    }

    fn is_seen(&self, node: DialogueNodeId) -> bool {
        self.snapshot.seen.contains(&NodeRef::from(node))
    }

    fn red_check_may_pass(&self, _node: DialogueNodeId) -> bool {
        !self.snapshot.red_checks_fail
    }

    fn check_margin(&self, node: DialogueNodeId) -> Option<(String, i32)> {
        let node = NodeRef::from(node);
        self.snapshot
            .check_margins
            .iter()
            .find(|margin| margin.node == node)
            .map(|margin| (margin.skill.clone(), margin.margin))
    }
}

/// Says, once per question, that the world was never told the answer.
///
/// ## What reaching this means
///
/// Everything a group's guards ask is asked of the plugin - `bridge::collect` walks the
/// guards and puts every call in `Questions`, either as a query by key or as a `DataRequest`
/// for a kind the plugin services. So a question with no answer here is one the plugin was
/// asked and could not give: it threw, or could not reach what the question needs.
///
/// THAT IS THE ONE UNKNOWN THE RULE ALLOWS - see de-m11s - because nobody can say what a
/// query about the world would have returned. What was missing is that it said nothing: the
/// Unknown became an ordinary value and travelled inward, indistinguishable downstream from a
/// question nobody thought to ask, and its cost is the usual one - a guard that cannot be
/// pruned on, and an entry behind it that can never be marked.
///
/// Measured over all 429 groups on 2026-09-21: never reached. So a line here is a thing to
/// investigate rather than noise, and its absence is the expectation.
fn unanswered_warning(question: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static ASKED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

    let Ok(mut asked) = ASKED.lock() else {
        return;
    };
    if !asked
        .get_or_insert_with(HashSet::new)
        .insert(question.to_string())
    {
        return;
    }

    eprintln!(
        "look-ahead: the world was not told {question}, so it cannot be decided and the guard          reading it is left undecided. The entry behind that guard can never be shown. The          plugin was asked and could not answer - see de-m11s.7."
    );
}
