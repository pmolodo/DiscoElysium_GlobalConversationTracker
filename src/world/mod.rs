// SPDX-License-Identifier: MIT

pub mod test_world;

use crate::core::clock::ClockTime;
use crate::core::guard::IGuardContext;
use crate::core::guard_value::GuardValue;
use crate::core::state::LookAheadState;
use crate::core::state::{ITEM_PREFIX, THOUGHT_PREFIX};
use crate::core::state::{StateSymbols, VariableRef};
use crate::core::types::{DialogueNodeId, Ternary};

/// The guard-language call that asks what the player is carrying, in centimes.
///
/// Named once because three places have to agree about it: this file intercepts it so a
/// guard is answered from the search's own balance rather than the world's, the guard
/// compiler decides it against the money register, and the layout only spends variables on
/// money where something asks. A fourth spelling would be a slot silently untracked.
pub const MONEY_QUERY: &str = "MoneyAmount";

/// The guard-language call that asks whether a flag is set, which is `Variable[name]` written
/// another way and answered from the same place.
///
/// `FELDLuaFunctions` exists only in Final Cut and its exported bodies are stubs; Cpp2IL's ISIL
/// dump of the shipped `GameAssembly.dll` (de-h0f1.8) recovers both:
///
/// ```text
/// FlagSet(string variableName):
///     Call LuaHelper.GetVariable, rcx, rdx
///     Return rax
///
/// FlagNotSet(string variableName):
///     Call LuaHelper.GetVariable, rcx, rdx
///     Xor rax, rax, 1
///     Return rax
/// ```
pub const FLAG_SET_QUERY: &str = "FlagSet";

/// The same question asked the other way round. Nine guards use [`FLAG_SET_QUERY`] and three
/// use this one.
pub const FLAG_NOT_SET_QUERY: &str = "FlagNotSet";

/// Whether `name` asks about a flag, and whether it wants the answer negated.
///
/// `Some(false)` for `FlagSet`, `Some(true)` for `FlagNotSet`, `None` for anything else.
///
/// FOUR PLACES HAVE TO AGREE, which is why this is a function rather than two constants
/// compared at each of them: the graph declares the flag as one of the group's variables so
/// it can be read at all, the layout spends a slot on it, `bridge::collect` keeps it out of
/// what the plugin is asked because it is asked for as a variable instead, and
/// [`BoundContext::query`] answers it from that variable. A name honoured in three of the
/// four is worse than one honoured in none: it is declared and slotted and then read from a
/// snapshot that was never told about it, which is the starting value forever.
pub fn flag_query(name: &str) -> Option<bool> {
    match name {
        FLAG_SET_QUERY => Some(false),
        FLAG_NOT_SET_QUERY => Some(true),
        _ => None,
    }
}

/// Everything outside the dialogue graph that the look-ahead needs to know.
pub trait ILookAheadWorld: Send + Sync {
    fn money(&self) -> i32;
    fn day_minutes(&self) -> i32;
    fn day_counter(&self) -> i32;
    fn is_clock_locked(&self) -> bool;
    /// A dialogue variable's value.
    ///
    /// Taken as a [`VariableRef`] rather than a name, so only a variable the group declared
    /// can be asked for - and those are exactly what the plugin is asked to answer.
    fn get_variable(&self, variable: VariableRef<'_>) -> GuardValue;
    /// Whether the player holds an item WHEN THE SEARCH STARTS.
    ///
    /// Two callers, and the difference between them is the whole point of the name.
    ///
    /// - Seeding an `item:` slot, for an item this conversation group's actions DO move.
    /// - Answering a `CheckItem` guard about an item the group does NOT move. There is no
    ///   slot for such an item, nothing can change it, so its starting value is its only
    ///   value and this is simply the answer.
    ///
    /// What it must never do is answer a guard about a TRACKED item. Once `GainItem` or
    /// `LoseItem` has run, the truth is in the search's state and this is stale. The old
    /// name, `has_item`, read as the general question and invited exactly that: it
    /// returns a plain `bool`, so it looks authoritative everywhere. Using it for a
    /// tracked item reports the starting inventory forever and the search stops seeing its
    /// own purchases.
    fn initially_has_item(&self, name: &str) -> bool;

    /// Whether a thought is in the cabinet WHEN THE SEARCH STARTS.
    ///
    /// What `IsTHCPresent` asks, and the same two callers again. GAINED, not
    /// internalised: see [`crate::core::state::THOUGHT_PREFIX`] for why those are
    /// different questions, and why only this one moves.
    fn initially_has_thought(&self, name: &str) -> bool;

    /// A skill's damage value WHEN THE SEARCH STARTS, by `SkillType` name - negative where
    /// damaged - or `None` where it is not known.
    ///
    /// Seeds a `damage:` slot for a skill this group's actions damage or heal; see
    /// `core::damage`. For a skill nothing in the group moves, the question is answered by
    /// [`Self::query`] instead.
    fn initial_damage(&self, _skill: &str) -> Option<f64> {
        None
    }

    /// The item an equipment slot holds WHEN THE SEARCH STARTS, by `EquipmentSlotType` name -
    /// empty for an empty slot - or `None` where it is not known.
    ///
    /// Read only where the group takes an item away, to empty the slot that held it; see
    /// `core::equipment`. Otherwise an equipment question is answered by [`Self::query`].
    fn item_in_slot(&self, _slot: &str) -> Option<String> {
        None
    }

    /// Every item the database files under an item group, or `None` where it is not known.
    ///
    /// What `CheckItemGroup` is answered over - see `core::item_group`. A world that cannot
    /// say leaves the question Unknown, which is permissive.
    fn items_in_group(&self, _group: &str) -> Option<Vec<String>> {
        None
    }

    /// The items of a group the player holds WHEN THE SEARCH STARTS, or `None` where it is
    /// not known.
    ///
    /// The same restriction as [`Self::initially_has_item`]: for a member the search moves,
    /// the slot is the truth and this is stale.
    fn initially_held_in_group(&self, _group: &str) -> Option<Vec<String>> {
        None
    }

    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue;
    fn check_passes(&self, node: DialogueNodeId) -> Ternary;
    fn is_seen(&self, node: DialogueNodeId) -> bool;

    /// Whether a red check's roll may succeed at this entry.
    ///
    /// False while a thought forces every red check to fail - the game's
    /// `ThoughtAlterant.RedChecksFail` - which closes the success branch to every crawl.
    /// Asked per entry rather than once, because a locked option's own Pass half is answered
    /// as if its roll could succeed while every red check deeper in the walk still fails -
    /// see `bridge::answer_starts`. Read through [`roll_may_succeed`].
    fn red_check_may_pass(&self, node: DialogueNodeId) -> bool;
}

/// Whether a passive check fires: the world's answer, or Unknown where the group can move the
/// skill it compares - see [`crate::core::skill_movers`]. One place for the reference walk and
/// both symbolic passes.
pub fn passive_outcome(
    node: &crate::graph::node::LookAheadNode,
    world: &dyn ILookAheadWorld,
) -> Ternary {
    if node.check_settled {
        world.check_passes(node.id)
    } else {
        Ternary::Unknown
    }
}

/// Whether entering `node` can take its roll's success branch.
///
/// Only a red check can be refused: a thought can force every red check to fail, and nothing
/// forces a white one. One place decides it for the reference walk and both symbolic passes,
/// which have to agree case for case.
pub fn roll_may_succeed(
    node: &crate::graph::node::LookAheadNode,
    world: &dyn ILookAheadWorld,
) -> bool {
    node.kind != crate::core::types::DialogueCheckKind::Red || world.red_check_may_pass(node.id)
}

/// What a search consults that outlives any one state: the symbol table and the world.
///
/// Deliberately holds NO state. An earlier version stored the state being evaluated and
/// had a `bind` method, which cannot be made to typecheck: the stored reference took the
/// same lifetime as the symbols and the world, so binding one of the search's own
/// short-lived states required it to outlive the whole search. Handing out a short-lived
/// [`BoundContext`] instead lets each state be borrowed for exactly the guard evaluation
/// that reads it, and leaves this shareable as `&CrawlContext`.
pub struct CrawlContext<'w> {
    pub symbols: &'w StateSymbols,
    pub world: &'w dyn ILookAheadWorld,
}

impl<'w> CrawlContext<'w> {
    pub fn new(symbols: &'w StateSymbols, world: &'w dyn ILookAheadWorld) -> Self {
        Self { symbols, world }
    }

    /// A view that answers guards from `state` where the search tracks a slot, and from
    /// the world otherwise.
    pub fn bound<'s>(&self, state: &'s LookAheadState) -> BoundContext<'s>
    where
        'w: 's,
    {
        BoundContext {
            symbols: self.symbols,
            world: self.world,
            state: Some(state),
        }
    }

    /// A view with no state behind it, for the seeding pass that runs before the first
    /// state exists.
    pub fn unbound(&self) -> BoundContext<'_> {
        BoundContext {
            symbols: self.symbols,
            world: self.world,
            state: None,
        }
    }
}

/// A [`CrawlContext`] looking at one particular state, for the length of one evaluation.
pub struct BoundContext<'s> {
    pub symbols: &'s StateSymbols,
    pub world: &'s dyn ILookAheadWorld,
    state: Option<&'s LookAheadState>,
}

impl BoundContext<'_> {
    /// A query about a named subject, answered from the slot that tracks it if there is
    /// one and from the world otherwise.
    ///
    /// Shared by the three queries shaped this way. TRACKED means the search's own actions
    /// have been moving it, so the state is the truth and the world is stale. UNTRACKED
    /// means no action in this group touches it, so its starting value is its only value
    /// and the world can simply be asked - falling through to `query` instead, which most
    /// worlds answer unknown, would throw away an answer already in hand and leave the
    /// branch open for no reason.
    fn tracked_or_world(
        &self,
        prefix: &str,
        name: &str,
        arguments: &[GuardValue],
        from_world: &dyn Fn(&dyn ILookAheadWorld, &str) -> bool,
    ) -> GuardValue {
        let Some(subject) = arguments
            .first()
            .filter(|v| v.kind() == GuardValueKind::Text)
            .map(|v| v.text())
        else {
            return self.world.query(name, arguments);
        };

        if let Some(state) = self.state
            && let Some(slot) = self.symbols.find(&format!("{prefix}{subject}"))
        {
            return GuardValue::from_boolean(state.is_set(slot));
        }

        GuardValue::from_boolean(from_world(self.world, subject))
    }
}

impl BoundContext<'_> {
    /// A query's single text argument.
    fn text_of(arguments: &[GuardValue]) -> Option<&str> {
        match arguments {
            [value] if value.kind() == crate::core::guard_value::GuardValueKind::Text => {
                Some(value.text())
            }
            _ => None,
        }
    }

    /// Whether this state says dialogue has taken `item` away.
    fn is_unequipped(&self, state: &LookAheadState, item: &str) -> bool {
        self.symbols
            .find(&format!("{}{item}", crate::core::state::UNEQUIPPED_PREFIX))
            .is_some_and(|slot| state.is_set(slot))
    }

    /// Whether some slot an equipment question reads holds an item this state has taken away.
    fn lost_worn_item(&self, name: &str, arguments: &[GuardValue]) -> bool {
        let Some(state) = self.state else {
            return false;
        };
        crate::core::equipment::slots_read_by(name, Self::text_of(arguments))
            .into_iter()
            .filter_map(|slot| self.world.item_in_slot(slot))
            .any(|item| !item.is_empty() && self.is_unequipped(state, &item))
    }

    /// The slot tracking the damage a damage question asks about, where this group has one.
    fn damage_slot(&self, question: &str) -> Option<usize> {
        let skill = crate::core::damage::skill_read_by(question)?;
        self.symbols
            .find(&format!("{}{skill}", crate::core::state::DAMAGE_PREFIX))
    }
}

impl IGuardContext for BoundContext<'_> {
    fn get_variable(&self, name: &str) -> GuardValue {
        let variable = self.symbols.variable_ref(name).unwrap_or_else(|| {
            panic!("a guard reads '{name}', which the group does not declare as a variable")
        });

        if let Some(slot) = self.symbols.find(name)
            && let Some(state) = self.state
        {
            let value = state.get(slot);
            // Check if the world has this as a number
            let world_val = self.world.get_variable(variable);
            if world_val.kind() == GuardValueKind::Number {
                return GuardValue::from_number(value as f64);
            }
            return GuardValue::from_boolean(value != 0);
        }
        self.world.get_variable(variable)
    }

    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue {
        // Clock queries answered from search state, or from the world's clock where there is
        // no state yet - which is the time the search starts at. Never asked of the plugin:
        // `ClockTime` is the game's own table, and the plugin already sends the clock.
        if ClockTime::owns(name) {
            let day_minutes = self
                .state
                .map_or_else(|| self.world.day_minutes(), |state| state.day_minutes());
            return ClockTime::answer(name, arguments, day_minutes, self.world.day_counter());
        }

        // The day, which is not the clock. A search's `PassTime` moves the time of day and
        // never the day counter, so these need no state and are exact with or without
        // one. Answered here rather than by each world because they are a comparison
        // against `day_counter`, which the world already supplies - see
        // `ClockTime::owns_day`.
        if ClockTime::owns_day(name) {
            return ClockTime::day_answer(name, arguments, self.world.day_counter());
        }

        match name {
            // `FlagSet(name)` asks whether a flag is set, and a flag is a dialogue
            // variable - so this is `Variable[name]` written another way, and is answered
            // from the same place. Nine guards in the database use it.
            //
            // A FLAG THE GROUP DOES NOT DECLARE is one named by something other than a literal,
            // which nothing asked the plugin for - so the world cannot have been told it, and
            // it is asked as the query it is, which answers Unknown.
            _ if flag_query(name).is_some() => {
                let negated = flag_query(name).expect("just matched");
                match arguments
                    .first()
                    .filter(|v| v.kind() == GuardValueKind::Text)
                    .map(|v| v.text())
                {
                    Some(flag) if self.symbols.variable_ref(flag).is_some() => {
                        let held = self.get_variable(flag);
                        if negated {
                            // `FlagNotSet(f)` is `not FlagSet(f)`, and the negation happens
                            // HERE rather than at the call site so it cannot be forgotten by
                            // one caller. A flag the search has raised reads raised, which
                            // is the whole point: SetFlag and UnsetFlag are modelled writes,
                            // and answering from the snapshot instead reports the value the
                            // crawl started with.
                            GuardValue::from_boolean(held.as_condition() == Ternary::False)
                        } else {
                            held
                        }
                    }
                    _ => self.world.query(name, arguments),
                }
            }
            // The balance, from search state or - before there is one - the balance the world
            // starts the search with, which the plugin already sends. Never asked as a call.
            MONEY_QUERY => GuardValue::from_number(
                self.state
                    .map_or_else(|| self.world.money(), |state| state.money())
                    as f64,
            ),
            // Two questions the search's own actions can change the answer to: inventory
            // and thought cabinet. Each is answered from a slot where this group moves the
            // subject and from the world where it does not. The journal is not here: the
            // index rewrites `IsTaskActive` into the variables it reads - see
            // `index::journal`.
            "CheckItem" => {
                self.tracked_or_world(ITEM_PREFIX, name, arguments, &|world, subject| {
                    world.initially_has_item(subject)
                })
            }
            "IsTHCPresent" => {
                self.tracked_or_world(THOUGHT_PREFIX, name, arguments, &|world, subject| {
                    world.initially_has_thought(subject)
                })
            }
            // An ACTION the database calls from a guard. The plugin was never asked for it,
            // because asking means running it, so there is nothing in the world to fall
            // through to. Answered as the game's own return would be: a function returning
            // nothing answers nil, the guards compare it against `true`, and `nil == true`
            // is false. See `modelling::ACTIONS_USED_AS_GUARDS`.
            other if crate::core::modelling::is_action_used_as_guard(other) => {
                GuardValue::from_boolean(false)
            }
            // WHICH REPUTATION IS WINNING, decided here rather than asked of the world.
            //
            // The amounts are dialogue variables, so each is read through `get_variable` -
            // which takes the search's own slot where the group moves one and the world's
            // value where it does not. That is the whole fix: `ReputationGrows` is a
            // modelled write, and answering this from the world reported whoever was ahead
            // when the crawl STARTED.
            //
            // The comparison itself is the game's, tie rule and all, and lives in
            // `core::reputation` rather than here because it is nothing like a maximum.
            other if crate::core::reputation::range_of(other).is_some() => {
                let range = crate::core::reputation::range_of(other).expect("just matched");
                let Some(wanted) = arguments
                    .first()
                    .filter(|v| v.kind() == GuardValueKind::Text)
                    .map(|v| v.text())
                else {
                    return GuardValue::unknown();
                };

                let winner = crate::core::reputation::highest(range, |name| {
                    let variable = crate::core::reputation::variable_of(name);
                    self.symbols.variable_ref(&variable)?;
                    self.get_variable(&variable)
                        .try_as_number()
                        .map(|n| n as i32)
                });

                match winner {
                    // NOTHING WINNING IS AN ANSWER, not an absence: the game returns the
                    // empty string, which equals no reputation's name.
                    Some(winner) => GuardValue::from_boolean(winner == Some(wanted)),
                    None => GuardValue::unknown(),
                }
            }
            // WHETHER A SKILL IS DAMAGED, from its `damage:` slot where this group damages or
            // heals it, and from the world where nothing does. See `core::damage`.
            other
                if crate::core::damage::skill_read_by(other).is_some()
                    && self.damage_slot(other).is_some() =>
            {
                let slot = self.damage_slot(other).expect("just matched");
                match self.state {
                    Some(state) => GuardValue::from_boolean(state.is_set(slot)),
                    None => self.world.query(name, arguments),
                }
            }
            // WHAT IS WORN, where this group has taken away an item a slot the question reads
            // holds: that slot reads empty. Anything else is the world's answer. See
            // `core::equipment`.
            other
                if crate::core::equipment::reads_equipment(other)
                    && self.lost_worn_item(other, arguments) =>
            {
                let state = self.state.expect("just matched");
                crate::core::equipment::answer_after_losses(
                    name,
                    Self::text_of(arguments),
                    |slot| self.world.item_in_slot(slot),
                    |item| self.is_unequipped(state, item),
                    |group| self.world.items_in_group(group),
                )
                .expect("just matched")
                .map_or_else(GuardValue::unknown, GuardValue::from_boolean)
            }
            // WHETHER KIM IS HERE OR IN THE PARTY: false once this group has taken Kim out of
            // the party, and the world's answer until then. See `core::party`.
            other
                if crate::core::party::reads_kim_removal(other)
                    && self.state.is_some_and(|state| {
                        self.symbols
                            .find(crate::core::party::KIM_REMOVED_SLOT)
                            .is_some_and(|slot| state.is_set(slot))
                    }) =>
            {
                GuardValue::from_boolean(false)
            }
            // WHETHER ANYTHING IN AN ITEM GROUP IS HELD, each member answered the way
            // `CheckItem` is - from its slot where the group moves it, from the starting
            // inventory where it does not. See `core::item_group`.
            crate::core::item_group::CHECK_ITEM_GROUP => {
                let Some(group) = arguments
                    .first()
                    .filter(|v| v.kind() == GuardValueKind::Text)
                    .map(|v| v.text())
                else {
                    return GuardValue::unknown();
                };

                let members = self.world.items_in_group(group);
                let held = self.world.initially_held_in_group(group);
                crate::core::item_group::any_held(
                    members.as_deref(),
                    |item| {
                        let state = self.state?;
                        let slot = self.symbols.find(&format!("{ITEM_PREFIX}{item}"))?;
                        Some(state.is_set(slot))
                    },
                    |item| held.as_ref().map(|held| held.iter().any(|h| h == item)),
                )
                .map_or_else(GuardValue::unknown, GuardValue::from_boolean)
            }
            // THE WEATHER, from the variable the group declares for it. See `core::scene`.
            other if crate::core::scene::variable_read_by(other).is_some() => {
                crate::core::scene::weather_answer(other, |variable| {
                    self.symbols.variable_ref(variable)?;
                    Some(self.get_variable(variable))
                })
                .expect("just matched")
            }
            // HOW OFTEN A SUBSTANCE WAS USED, from the count variable the group declares for
            // it. A computed argument names no declared variable and reads Unknown.
            other if crate::core::substance::owns(other) => {
                crate::core::substance::answer(other, arguments, |variable| {
                    self.symbols.variable_ref(variable)?;
                    Some(self.get_variable(variable))
                })
                .expect("just matched")
            }
            _ => self.world.query(name, arguments),
        }
    }
}

use crate::core::guard_value::GuardValueKind;
