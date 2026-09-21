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
//! A check's result runs `CheckAlterant` (pre-final-cut export):
//!
//! ```text
//! WhiteCheckResult(result):                       // WhiteCheckNode.CheckSuccess
//!     if (result.IsSuccess) return;
//!     INT: if (IsThoughtFixed("return_on_investment")) Money += 100
//!     MOT: if (IsThoughtFixed("superstar_cop"))        DamageVolition(1)
//!     if (IsThoughtFixed("kras_mazov"))                HealVolition(99)
//!
//! RedCheckResult(result):                         // RedCheckNode and FakeCheckNode.CheckSuccess
//!     if (result.IsSuccess) return;
//!     FYS, MOT: if (IsThoughtFixed("sorry_cop")) HealEndurance(1)
//!     INT, PSY: if (IsThoughtFixed("sorry_cop")) HealVolition(1)
//!
//! PassiveCheckSuccessPrice(skillType):            // PassiveNode.HandleEntry, applyPassiveSuccessBonus
//!     CONCEPTUALIZATION: if (IsThoughtFixed("art_cop"))          HealVolition(1)
//!     ENCYCLOPEDIA:      if (IsThoughtFixed("trant_heidelstam")) Money += 200
//! ```
//!
//! The ability is `Skill.GetAbility`, by `SkillType` range. A rolled or fake check's skill is its
//! `SkillType` field, an articy id `ArticyBridge.SkillIdToSkillType` looks up; a passive check's
//! is its speaker's (`ActorIdToSkillType`). `PassiveNode.HandleEntry` pays the passive price only
//! for a success - an antipassive never is one - on an entry not yet seen.
//!
//! ## What the engine does
//!
//! The parser follows a reputation action with the effect, marked once like the reputation and
//! conditioned on the thought being fixed (`DialogueAction::when_thought_fixed`). The graph
//! builder gives a failing rolled or fake check its failure effects as
//! `LookAheadNode::failure_actions`, applied on the failing branch after the failure flag, and a
//! passive check its success effects as once actions on the entry, since a passive's success is
//! the entry being charged. A graph is
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

/// A skill's ability, `Skill.GetAbility`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ability {
    Int,
    Psy,
    Fys,
    Mot,
}

/// `ArticyBridge.ARTICY_ID_TO_SKILL_TYPE`, with each skill's ability from its `SkillType` range.
const SKILLS: [(&str, &str, Ability); 28] = [
    ("0x0100000400000767", "LOGIC", Ability::Int),
    ("0x010000040000076B", "ENCYCLOPEDIA", Ability::Int),
    ("0x0100000A00000016", "RHETORIC", Ability::Int),
    ("0x0100000A0000001A", "DRAMA", Ability::Int),
    ("0x0100000400000918", "CONCEPTUALIZATION", Ability::Int),
    ("0x0100000A0000001E", "VISUAL_CALCULUS", Ability::Int),
    ("0x0100000A0000003E", "VOLITION", Ability::Psy),
    ("0x010000040000076F", "INLAND_EMPIRE", Ability::Psy),
    ("0x0100000400000773", "EMPATHY", Ability::Psy),
    ("0x0100000A00000042", "AUTHORITY", Ability::Psy),
    ("0x0100000A00000046", "SUGGESTION", Ability::Psy),
    ("0x0100000A0000004A", "ESPRIT_DE_CORPS", Ability::Psy),
    ("0x0100000400000B11", "PHYSICAL_INSTRUMENT", Ability::Fys),
    ("0x0100000A00000026", "ELECTROCHEMISTRY", Ability::Fys),
    ("0x01000004000009A7", "ENDURANCE", Ability::Fys),
    ("0x01000011000010D8", "HALF_LIGHT", Ability::Fys),
    ("0x0100000A00000022", "PAIN_THRESHOLD", Ability::Fys),
    ("0x0100000400000BC7", "SHIVERS", Ability::Fys),
    ("0x0100000A0000002A", "HE_COORDINATION", Ability::Mot),
    ("0x0100000400000BC3", "PERCEPTION", Ability::Mot),
    ("0x0100000800000BB0", "HEARING", Ability::Mot),
    ("0x0100000800000BBC", "SIGHT", Ability::Mot),
    ("0x0100000800000BAC", "SMELL", Ability::Mot),
    ("0x0100000800000BB8", "TASTE", Ability::Mot),
    ("0x0100000A0000002E", "REACTION", Ability::Mot),
    ("0x0100000A00000032", "SAVOIR_FAIRE", Ability::Mot),
    ("0x0100000A00000036", "INTERFACING", Ability::Mot),
    ("0x0100000A0000003A", "COMPOSURE", Ability::Mot),
];

/// Whether `name` is one of the skills this engine holds.
///
/// For a caller translating some other vocabulary into this one - see [`crate::core::garment`],
/// which maps what the dialogue database calls a skill onto what the engine does. A name that
/// is not here is a name nothing else in the engine will match either.
pub fn names_a_skill(name: &str) -> bool {
    SKILLS.iter().any(|(_, skill, _)| *skill == name)
}

/// The ability of the skill an articy id names, or `None` for an id that names no skill.
pub fn ability_of_skill_id(articy_id: &str) -> Option<Ability> {
    SKILLS
        .iter()
        .find(|(id, _, _)| *id == articy_id)
        .map(|(_, _, ability)| *ability)
}

/// The skills a passive check's price is paid for, by the speaker's actor id in the shipped
/// database - Conceptualization is actor 397 and Encyclopedia 399, which
/// `tests/shipped_index.rs` pins against the actor table.
pub const PASSIVE_PRICE_ACTORS: [(&str, &str); 2] =
    [("397", "CONCEPTUALIZATION"), ("399", "ENCYCLOPEDIA")];

/// The literal amounts `CheckAlterant` pays out.
const RETURN_ON_INVESTMENT_MONEY: i32 = 100;
const TRANT_HEIDELSTAM_MONEY: i32 = 200;

/// A heal of one, the size every `CheckAlterant` heal but `kras_mazov`'s is.
const HEAL_ONE: i32 = -1;

/// What a failed white check adds, by the check's ability: (ability or any, thought, effect).
const WHITE_FAILURE_EFFECTS: [(Option<Ability>, &str, ThoughtEffect); 3] = [
    (
        Some(Ability::Int),
        "return_on_investment",
        ThoughtEffect::Money(RETURN_ON_INVESTMENT_MONEY),
    ),
    (
        Some(Ability::Mot),
        "superstar_cop",
        ThoughtEffect::Damage {
            skill: "VOLITION",
            amount: 1,
        },
    ),
    (
        None,
        "kras_mazov",
        ThoughtEffect::Damage {
            skill: "VOLITION",
            amount: -99,
        },
    ),
];

/// What a failed red or fake check adds, by the check's ability.
const RED_FAILURE_EFFECTS: [(Option<Ability>, &str, ThoughtEffect); 4] = [
    (
        Some(Ability::Fys),
        "sorry_cop",
        ThoughtEffect::Damage {
            skill: "ENDURANCE",
            amount: HEAL_ONE,
        },
    ),
    (
        Some(Ability::Mot),
        "sorry_cop",
        ThoughtEffect::Damage {
            skill: "ENDURANCE",
            amount: HEAL_ONE,
        },
    ),
    (
        Some(Ability::Int),
        "sorry_cop",
        ThoughtEffect::Damage {
            skill: "VOLITION",
            amount: HEAL_ONE,
        },
    ),
    (
        Some(Ability::Psy),
        "sorry_cop",
        ThoughtEffect::Damage {
            skill: "VOLITION",
            amount: HEAL_ONE,
        },
    ),
];

/// What a passive check's success adds, by the skill it tests.
const PASSIVE_SUCCESS_EFFECTS: [(&str, &str, ThoughtEffect); 2] = [
    (
        "CONCEPTUALIZATION",
        "art_cop",
        ThoughtEffect::Damage {
            skill: "VOLITION",
            amount: HEAL_ONE,
        },
    ),
    (
        "ENCYCLOPEDIA",
        "trant_heidelstam",
        ThoughtEffect::Money(TRANT_HEIDELSTAM_MONEY),
    ),
];

/// Which rolled check failed, for [`failure_effects`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolledKind {
    White,
    /// A red check, or a fake one - both report through `RedCheckResult`.
    Red,
}

/// The (thought, effect) pairs a failed check of this kind and ability adds, in the game's order.
pub fn failure_effects(kind: RolledKind, ability: Ability) -> Vec<(&'static str, ThoughtEffect)> {
    let table: &[(Option<Ability>, &str, ThoughtEffect)] = match kind {
        RolledKind::White => &WHITE_FAILURE_EFFECTS,
        RolledKind::Red => &RED_FAILURE_EFFECTS,
    };
    table
        .iter()
        .filter(|(wanted, _, _)| wanted.is_none_or(|wanted| wanted == ability))
        .map(|(_, thought, effect)| (*thought, *effect))
        .collect()
}

/// The (thought, effect) a passive check spoken by `actor` adds on success, if any.
pub fn passive_success_effect(actor: &str) -> Option<(&'static str, ThoughtEffect)> {
    let skill = PASSIVE_PRICE_ACTORS
        .iter()
        .find(|(id, _)| *id == actor)
        .map(|(_, skill)| *skill)?;
    PASSIVE_SUCCESS_EFFECTS
        .iter()
        .find(|(name, _, _)| *name == skill)
        .map(|(_, thought, effect)| (*thought, *effect))
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

/// Every thought this module asks about, for a fixture stating that all of them are fixed.
///
/// A WORLD STATES SETS, not answers by name - see `GameWorld::set_fixed` - so a test meaning
/// "whatever thought is asked about, it is fixed" has to name them. Listed here rather than in
/// each test, since the list belongs to the effects table beside it and would otherwise be
/// copied into every fixture that wants it.
pub const EVERY_THOUGHT: [&str; 10] = [
    "art_cop",
    "kras_mazov",
    "moralist",
    "return_on_investment",
    "revacholian_nationhood",
    "sorry_cop",
    "superstar_cop",
    "the_destroyer",
    "trant_heidelstam",
    "ultraliberal",
];

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
