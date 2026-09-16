// SPDX-License-Identifier: MIT
//! Hardcore mode, which is what `IsHardcoreModeActive` and `WasGameBeatenInHardcoreMode` ask.
//!
//! Both are registered by `FELDLuaFunctions`, which exists only in Final Cut, and whose
//! exported bodies are AssetRipper stubs.
//!
//! ## `IsHardcoreModeActive`: recovered from the binary, and measured
//!
//! Cpp2IL's ISIL dump of the shipped `GameAssembly.dll` (de-h0f1.8) reads:
//!
//! ```text
//! IsHardcoreModeActive:
//!     Call GameModeController.IsHardcoreOn
//!     Return rax
//!
//! GameModeController.IsHardcoreOn:
//!     Call GameModeController.get_Singleton
//!     Compare rax, 0            ; no controller -> false
//!     Compare [rax+48], 1       ; currentMode == GameMode.HARDCORE
//! ```
//!
//! and it was measured independently in the shipped build on 2026-09-16 (de-h0f1.30): four
//! saves, diffs of `testing/save_template` varying `gameModeState.gameMode` and
//! `gameModeState.wasSwitched`, each loaded after a control and confirmed to have loaded by
//! differing from it. The answer was true exactly when `gameMode` was HARDCORE, whatever
//! `wasSwitched` said.
//!
//! The plugin reads `GameModeController.Singleton.currentMode`; a save records it as
//! `gameModeState.gameMode`.
//!
//! ## `WasGameBeatenInHardcoreMode`: recovered from the binary
//!
//! The ISIL inlines `GameStatsManager.get_HardcorePlaythroughCompleted` - both read the same
//! static field, `GameStatsManager`'s statics at offset 24 - so it answers that flag. The stat
//! is kept under the key `hardcore_playthrough_completed` beside the substance-use counters,
//! which is profile state rather than save state: no save records it, so an offline world cannot
//! read it and answers a fixed value instead - false, unless a scenario row names
//! `hardcorePlaythroughCompleted`. The plugin reads it directly.
//!
//! Both are constant for a search: nothing in dialogue switches the mode or finishes a game.

/// The question answered from the game mode.
pub const IS_HARDCORE_MODE_ACTIVE: &str = "IsHardcoreModeActive";

/// The `GameModeController.GameMode` name that answers true.
pub const HARDCORE: &str = "HARDCORE";

/// The question answered from the finished-in-hardcore stat.
pub const WAS_GAME_BEATEN_IN_HARDCORE_MODE: &str = "WasGameBeatenInHardcoreMode";
