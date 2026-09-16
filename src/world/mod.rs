// SPDX-License-Identifier: MIT

pub mod test_world;

use crate::core::clock::ClockTime;
use crate::core::guard::IGuardContext;
use crate::core::guard_value::GuardValue;
use crate::core::state::LookAheadState;
use crate::core::state::{ITEM_PREFIX, TASK_PREFIX, THOUGHT_PREFIX};
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
pub const FLAG_SET_QUERY: &str = "FlagSet";

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

    /// Whether a journal task is active WHEN THE SEARCH STARTS.
    ///
    /// The same two callers and the same restriction as [`Self::initially_has_item`].
    fn initially_task_active(&self, name: &str) -> bool;

    /// Whether a thought is in the cabinet WHEN THE SEARCH STARTS.
    ///
    /// What `IsTHCPresent` asks, and the same two callers again. GAINED, not
    /// internalised: see [`crate::core::state::THOUGHT_PREFIX`] for why those are
    /// different questions, and why only this one moves.
    fn initially_has_thought(&self, name: &str) -> bool;
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
        // Clock queries answered from search state
        if let Some(state) = self.state
            && ClockTime::owns(name)
        {
            let day_minutes = state.day_minutes();
            let day_counter = self.world.day_counter();
            return ClockTime::answer(name, arguments, day_minutes, day_counter);
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
            FLAG_SET_QUERY => {
                match arguments
                    .first()
                    .filter(|v| v.kind() == GuardValueKind::Text)
                    .map(|v| v.text())
                {
                    Some(flag) if self.symbols.variable_ref(flag).is_some() => {
                        self.get_variable(flag)
                    }
                    _ => self.world.query(name, arguments),
                }
            }
            MONEY_QUERY => {
                if let Some(state) = self.state {
                    GuardValue::from_number(state.money() as f64)
                } else {
                    self.world.query(name, arguments)
                }
            }
            // The three questions the search's own actions can change the answer to:
            // inventory, journal, thought cabinet. Each is answered from a slot where
            // this group moves the subject and from the world where it does not.
            "CheckItem" => {
                self.tracked_or_world(ITEM_PREFIX, name, arguments, &|world, subject| {
                    world.initially_has_item(subject)
                })
            }
            "IsTaskActive" => {
                self.tracked_or_world(TASK_PREFIX, name, arguments, &|world, subject| {
                    world.initially_task_active(subject)
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
            _ => self.world.query(name, arguments),
        }
    }
}

use crate::core::guard_value::GuardValueKind;
