// SPDX-License-Identifier: MIT
//! What a FIXED thought adds to an action, where the game branches on the thought in code rather
//! than applying one of its `CharacterEffect`s.
//!
//! ## The game
//!
//! `ReputationGrows`, `ReputationLowers` and `Reputation` reach
//! `ReputationAlterant.ReputationOption` (pre-final-cut export; Final Cut's body is stripped):
//!
//! ```text
//! if ((int)GenericLuaFunctions.Once(value) != 0)
//!     ModifyIfDifferentFromZero(reputationName, value);
//!
//! ModifyIfDifferentFromZero:
//!     ModifyReputation(reputationName, value);
//!     Reputation? reputation = GetReputation(reputationName);
//!     if (reputation.HasValue) { ReputationEffect(reputation.Value); ObsessionGiver(...); }
//!
//! ReputationEffect(Reputation rep):
//!     if (CharacterThoughts.IsThoughtFixed(copotypeThought[(int)rep]))
//!         switch (rep)
//!             moralist:               CharacterManipulations.HealVolition(1)
//!             the_destroyer:          CharacterManipulations.HealEndurance(1)
//!             revacholian_nationhood: CharacterManipulations.DamageVolition(1)
//!             ultraliberal:           PlayerCharacter.Money += LIBERAL_MONEY_AMOUNT   // 100
//!             communist:              XpAmount += COMMUNIST_XP_AMOUNT
//!             apocalypse_cop:         CheckAlterant.ResetWhiteChecksByAbility(INT, PSY) // empty bodies
//! ```
//!
//! `copotypeThought` lists the thoughts in `Reputation` enum order, and for each of the four
//! reputations above the thought has the reputation's own name. The effect fires on a lowering
//! as well as a raising - only a zero amount skips it - and inside the same `Once` as the
//! reputation itself.
//!
//! ## What the engine does
//!
//! The parser follows a reputation action with the effect, marked once like the reputation and
//! conditioned on the thought being fixed (`DialogueAction::when_thought_fixed`). A graph is
//! fitted to its world before a search (`crate::graph::Fitting`), which switches each such action
//! on or off by whether the world holds the thought fixed. Only `PassTime` bakes a thought, and
//! the plugin sends the clock locked, so the answer holds for the whole search. A thought whose
//! state could not be read counts as not fixed.
//!
//! Experience and the empty white-check resets are left out: nothing a guard, check or price
//! reads.

use crate::core::action::DialogueAction;
use crate::core::guard_value::{GuardValue, GuardValueKind};
use crate::core::state::StateSymbols;
use crate::world::ILookAheadWorld;

/// The question a fixed thought is asked with.
pub const IS_FIXED: &str = "IsTHCFixed";

/// What a thought adds to an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThoughtEffect {
    /// `PlayerCharacter.Money` moves by this much.
    Money(i32),
    /// The skill's damage moves by `amount`: positive is a blow, negative a heal - the same
    /// convention as a `damage:` slot, see `core::damage`.
    Damage { skill: &'static str, amount: i32 },
}

/// `ReputationAlterant.LIBERAL_MONEY_AMOUNT`.
const LIBERAL_MONEY_AMOUNT: i32 = 100;

/// The reputations whose thought adds something a search can observe, and what it adds.
const REPUTATION_EFFECTS: [(&str, ThoughtEffect); 4] = [
    (
        "moralist",
        ThoughtEffect::Damage {
            skill: "VOLITION",
            amount: -1,
        },
    ),
    (
        "the_destroyer",
        ThoughtEffect::Damage {
            skill: "ENDURANCE",
            amount: -1,
        },
    ),
    (
        "revacholian_nationhood",
        ThoughtEffect::Damage {
            skill: "VOLITION",
            amount: 1,
        },
    ),
    ("ultraliberal", ThoughtEffect::Money(LIBERAL_MONEY_AMOUNT)),
];

/// The thought a reputation action consults and what it adds, or `None` for a reputation whose
/// thought adds nothing a search observes.
pub fn of_reputation(reputation: &str) -> Option<(&'static str, ThoughtEffect)> {
    REPUTATION_EFFECTS
        .iter()
        .find(|(name, _)| *name == reputation)
        .map(|(name, effect)| (*name, *effect))
}

impl ThoughtEffect {
    /// The effect as a once action named `name`, applying only while `thought` is fixed.
    pub fn action(self, thought: &str, symbols: &mut StateSymbols, name: String) -> DialogueAction {
        let action = match self {
            ThoughtEffect::Money(amount) => {
                DialogueAction::money(amount >= 0, amount.abs(), true, name)
            }
            ThoughtEffect::Damage { skill, amount } => {
                DialogueAction::increment(symbols.damage(skill), amount, true, name)
            }
        };
        action.when_thought_fixed(thought)
    }
}

/// Whether `world` holds `thought` fixed; Unknown reads as not.
pub fn is_fixed(world: &dyn ILookAheadWorld, thought: &str) -> bool {
    let answer = world.query(IS_FIXED, &[GuardValue::from_text(thought.to_string())]);
    answer.kind() == GuardValueKind::Boolean && answer.boolean()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_reputations_with_an_observable_effect() {
        assert_eq!(
            of_reputation("ultraliberal"),
            Some(("ultraliberal", ThoughtEffect::Money(100)))
        );
        assert_eq!(
            of_reputation("revacholian_nationhood"),
            Some((
                "revacholian_nationhood",
                ThoughtEffect::Damage {
                    skill: "VOLITION",
                    amount: 1
                }
            ))
        );
        assert!(of_reputation("communist").is_none());
        assert!(of_reputation("art_cop").is_none());
    }
}
