// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Persistence
{
    /// <summary>
    /// How an attempt to load the global state from one file ended.
    /// </summary>
    /// <remarks>
    /// The whole point of separating these is so first-access initialization can
    /// tell "there is no history yet, start empty" apart from "there is history
    /// but this copy is damaged, try the backup and do not overwrite anything
    /// until you know". Never collapse them into a nullable state.
    /// </remarks>
    public enum GlobalStateLoadOutcome
    {
        /// <summary>
        /// The file was read and parsed. <see cref="GlobalStateLoadResult.State"/>
        /// is non-null. Individual unreadable rows may still have been skipped; see
        /// <see cref="GlobalStateLoadResult.SkippedRowCount"/>.
        /// </summary>
        Loaded = 0,

        /// <summary>
        /// The file does not exist. Nothing is wrong; this is what a first run looks
        /// like. Safe to start empty and save over.
        /// </summary>
        Missing = 1,

        /// <summary>
        /// The file exists but could not be parsed: truncated, not JSON, or missing
        /// the required structure. The previous generation in the backup slot is the
        /// thing to try next.
        /// </summary>
        Corrupt = 2,

        /// <summary>
        /// The file is well-formed JSON but carries a format version this build does
        /// not understand, most likely written by a newer version of the mod.
        /// Distinct from <see cref="Corrupt"/> on purpose: the file is probably
        /// intact and full of real history, so overwriting it would destroy data.
        /// Callers should refuse to save rather than recover.
        /// </summary>
        UnsupportedVersion = 3,
    }
}
