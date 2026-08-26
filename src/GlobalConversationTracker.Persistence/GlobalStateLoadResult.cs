// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.Persistence
{
    /// <summary>
    /// The outcome of loading the global state from one file.
    /// </summary>
    public sealed class GlobalStateLoadResult
    {
        private static readonly string[] NoWarnings = new string[0];

        private GlobalStateLoadResult(
            GlobalStateLoadOutcome outcome,
            string sourcePath,
            GlobalConversationState? state,
            string? errorMessage,
            int skippedRowCount,
            IReadOnlyList<string> warnings)
        {
            Outcome = outcome;
            SourcePath = sourcePath;
            State = state;
            ErrorMessage = errorMessage;
            SkippedRowCount = skippedRowCount;
            Warnings = warnings;
        }

        /// <summary>How the load ended.</summary>
        public GlobalStateLoadOutcome Outcome { get; }

        /// <summary>The file this result describes, whether or not it existed.</summary>
        public string SourcePath { get; }

        /// <summary>
        /// The loaded state, non-null if and only if <see cref="Outcome"/> is
        /// <see cref="GlobalStateLoadOutcome.Loaded"/>.
        /// </summary>
        public GlobalConversationState? State { get; }

        /// <summary>
        /// Why the load failed, or null when it did not. Never used as a control
        /// signal; switch on <see cref="Outcome"/> for that.
        /// </summary>
        public string? ErrorMessage { get; }

        /// <summary>
        /// How many individual rows were dropped while parsing an otherwise readable
        /// file: unrecognized status strings, non-integer IDs, wrong value types. A
        /// non-zero count with <see cref="GlobalStateLoadOutcome.Loaded"/> means the
        /// load succeeded but the file was partly nonsense and is worth logging.
        /// </summary>
        public int SkippedRowCount { get; }

        /// <summary>
        /// Human-readable descriptions of the first few skipped rows, capped at
        /// <see cref="GlobalStateJson.MaxWarnings"/> so a pathological file cannot
        /// balloon memory. <see cref="SkippedRowCount"/> is the untruncated count.
        /// </summary>
        public IReadOnlyList<string> Warnings { get; }

        /// <summary>True when <see cref="State"/> is usable.</summary>
        public bool IsLoaded => Outcome == GlobalStateLoadOutcome.Loaded;

        /// <summary>
        /// The loaded state, throwing if the load did not succeed. For call sites
        /// that have already checked <see cref="IsLoaded"/>.
        /// </summary>
        /// <exception cref="InvalidOperationException">The load did not succeed.</exception>
        public GlobalConversationState RequireState()
        {
            if (State == null)
            {
                throw new InvalidOperationException(
                    $"No state was loaded from '{SourcePath}': {Outcome} ({ErrorMessage ?? "no detail"}).");
            }

            return State;
        }

        internal static GlobalStateLoadResult Loaded(
            string sourcePath,
            GlobalConversationState state,
            int skippedRowCount,
            IReadOnlyList<string> warnings)
        {
            return new GlobalStateLoadResult(
                GlobalStateLoadOutcome.Loaded, sourcePath, state, null, skippedRowCount, warnings);
        }

        internal static GlobalStateLoadResult Missing(string sourcePath)
        {
            return new GlobalStateLoadResult(
                GlobalStateLoadOutcome.Missing, sourcePath, null, null, 0, NoWarnings);
        }

        internal static GlobalStateLoadResult Corrupt(string sourcePath, string errorMessage)
        {
            return new GlobalStateLoadResult(
                GlobalStateLoadOutcome.Corrupt, sourcePath, null, errorMessage, 0, NoWarnings);
        }

        internal static GlobalStateLoadResult UnsupportedVersion(string sourcePath, string errorMessage)
        {
            return new GlobalStateLoadResult(
                GlobalStateLoadOutcome.UnsupportedVersion, sourcePath, null, errorMessage, 0, NoWarnings);
        }

        /// <inheritdoc />
        public override string ToString()
        {
            string detail = State != null
                ? $"{State.EntryCount} entries"
                : ErrorMessage ?? "no detail";
            string skipped = SkippedRowCount > 0 ? $", {SkippedRowCount} rows skipped" : string.Empty;
            return $"GlobalStateLoadResult({Outcome}, '{SourcePath}', {detail}{skipped})";
        }
    }
}
