// SPDX-License-Identifier: MIT
//! Who is with the player, which is what `IsKimHere`, `IsKimInParty` and `IsCunoInParty` ask.
//!
//! ## The game's definition
//!
//! From the pre-final-cut export (Assets/Scripts/Assembly-CSharp), `PartyManager`:
//!
//! ```text
//! public static bool IsKimInParty()
//! {
//!     return SingletonComponent<KimKitsuragi>.Singleton.IsInParty;
//! }
//!
//! public static bool IsKimHere()
//! {
//!     if (SingletonComponent<KimKitsuragi>.Singleton.IsInParty)
//!     {
//!         return !SingletonComponent<KimKitsuragi>.Singleton.IsLeftOutside;
//!     }
//!     return false;
//! }
//!
//! public static bool IsCunoInParty()
//! {
//!     return SingletonComponent<Cuno>.Singleton.IsInParty;
//! }
//! ```
//!
//! and the same shape in Final Cut, where the bodies are stripped in the export but Cpp2IL's
//! ISIL dump of the shipped `GameAssembly.dll` shows `IsKimHere` calling the singleton's
//! `IsInParty` getter, returning when it is false, and otherwise comparing `IsLeftOutside`
//! against zero. It was also measured in the shipped build over 24 control-verified party
//! saves (de-h0f1.33), which found `IsKimHere` reading those two flags and no others.
//!
//! So the three are answered from three flags. The plugin reads `IsInParty` and
//! `IsLeftOutside` off the party members themselves; a save records them in its
//! `partyState`, by the names used here as the data request's subject.
//!
//! ## What dialogue does to it
//!
//! `KimLuaFunctions`, `KimKitsuragi` and `PartyMember`, from the same export, and the same
//! calls in Final Cut's ISIL dump:
//!
//! ```text
//! RemoveKitsuragiWaitAtChurch() -> PartyManager.SetKimWaitLocation(OUTSIDE_CHURCH);
//!                                  KimKitsuragi.RemoveAndHide()
//! RemoveAndHide():  if (IsInParty) Remove();  else hide the object
//! Remove():         RemoveAndStay() -> Party.Player.Remove(character, hide: false)
//! ```
//!
//! So the call takes Kim out of the party and leaves `IsLeftOutside` alone: afterwards
//! `IsKimInParty` and `IsKimHere` are both false, whatever they were. A search tracks that in
//! one slot, [`KIM_REMOVED_SLOT`], which starts clear and is set by the call. While it is
//! clear the two questions are the world's answer; once set, both are false. The slot says
//! what the search DID rather than what the flag IS, so an unread flag stays unknown until
//! the call rather than being guessed at.
//!
//! Only this call is ported: it is the one party writer with a downstream reader in the
//! shipped database. The rest are held by a decision in `core::modelling`.

/// The flag that says Kim is in the party.
pub const KIM_IN_PARTY: &str = "isKimInParty";

/// The flag that says Kim has been left outside.
pub const KIM_LEFT_OUTSIDE: &str = "isKimLeftOutside";

/// The flag that says Cuno is in the party.
pub const CUNO_IN_PARTY: &str = "isCunoInParty";

/// The slot set once dialogue has taken Kim out of the party.
pub const KIM_REMOVED_SLOT: &str = "party:kimRemoved";

/// The script calls that take Kim out of the party and are modelled as doing so.
pub const KIM_REMOVERS: [&str; 1] = ["RemoveKitsuragiWaitAtChurch"];

/// The questions a removal answers false.
const ANSWERED_BY_REMOVAL: [&str; 2] = ["IsKimInParty", "IsKimHere"];

/// Whether a script call takes Kim out of the party.
pub fn removes_kim(call: &str) -> bool {
    KIM_REMOVERS.contains(&call)
}

/// Whether a question reads [`KIM_REMOVED_SLOT`] where the group has one.
pub fn reads_kim_removal(name: &str) -> bool {
    ANSWERED_BY_REMOVAL.contains(&name)
}

/// The flags `name` reads, or nothing where it is not a party question.
pub fn flags_read_by(name: &str) -> &'static [&'static str] {
    match name {
        "IsKimInParty" => &[KIM_IN_PARTY],
        "IsKimHere" => &[KIM_IN_PARTY, KIM_LEFT_OUTSIDE],
        "IsCunoInParty" => &[CUNO_IN_PARTY],
        _ => &[],
    }
}

/// The answer to a party question, reading each flag through `flag` - `None` where it was not
/// read. The outer `None` means `name` is not a party question.
pub fn answer(name: &str, flag: impl Fn(&str) -> Option<bool>) -> Option<Option<bool>> {
    match name {
        "IsKimInParty" => Some(flag(KIM_IN_PARTY)),
        "IsCunoInParty" => Some(flag(CUNO_IN_PARTY)),
        // The `if` stops at a Kim who is not in the party, whatever the other flag says.
        "IsKimHere" => Some(match flag(KIM_IN_PARTY) {
            Some(false) => Some(false),
            Some(true) => flag(KIM_LEFT_OUTSIDE).map(|outside| !outside),
            None => None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags<'a>(set: &'a [(&str, bool)]) -> impl Fn(&str) -> Option<bool> + 'a {
        move |name| set.iter().find(|(flag, _)| *flag == name).map(|(_, v)| *v)
    }

    #[test]
    fn kim_is_here_when_in_the_party_and_not_left_outside() {
        let with = flags(&[(KIM_IN_PARTY, true), (KIM_LEFT_OUTSIDE, false)]);
        assert_eq!(answer("IsKimHere", &with), Some(Some(true)));

        let outside = flags(&[(KIM_IN_PARTY, true), (KIM_LEFT_OUTSIDE, true)]);
        assert_eq!(answer("IsKimHere", &outside), Some(Some(false)));
    }

    #[test]
    fn a_kim_out_of_the_party_is_not_here_whatever_else_is_unread() {
        let away = flags(&[(KIM_IN_PARTY, false)]);
        assert_eq!(answer("IsKimHere", &away), Some(Some(false)));
        assert_eq!(answer("IsKimInParty", &away), Some(Some(false)));
    }

    #[test]
    fn the_removal_slot_is_not_a_variable() {
        assert!(KIM_REMOVED_SLOT.starts_with(crate::core::state::PARTY_PREFIX));
        assert!(!crate::core::state::names_a_variable(KIM_REMOVED_SLOT));
    }

    #[test]
    fn an_unread_flag_leaves_the_answer_unknowable() {
        let unread = flags(&[]);
        assert_eq!(answer("IsCunoInParty", &unread), Some(None));
        assert_eq!(answer("IsKimHere", &unread), Some(None));
        assert_eq!(answer("IsDaytime", &unread), None);
    }
}
