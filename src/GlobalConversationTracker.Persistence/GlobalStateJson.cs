// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

using GlobalConversationTracker.Core;

namespace GlobalConversationTracker.Persistence
{
    /// <summary>
    /// The on-disk JSON format for <see cref="GlobalConversationState"/>: pure
    /// bytes-to-state conversion with no file IO.
    /// </summary>
    /// <remarks>
    /// <para>Shape (format version 5):</para>
    /// <code>
    /// {"_format":"global-state","_formatVersion":5,"conversations":{"WasDisplayed":{"3":"17,19"},"WasOffered":{"3":"18"}},"orbs":[]}
    /// </code>
    /// <para>Grouped by status, then by conversation, then the entry IDs as a RUN-ENCODED
    /// STRING - <c>"3,5,7-25"</c> - written by <see cref="SparseOrder"/>, which is the one
    /// implementation every file in this repository shares.</para>
    ///
    /// <para>WHY IT IS WORTH A FORMAT VERSION. What this file records is what a player has
    /// READ, and a player reads a conversation by walking through it, so the entry IDs it
    /// holds are clustered rather than scattered. Measured on
    /// <c>testing/scenarios/global-state-worst-case.json</c>, which records every entry of
    /// every conversation in the game: 423 KB as arrays, and the run form is a fraction of
    /// it. The worst case for a run encoding is a perfectly alternating set, where it costs
    /// what the array costs; that is not what a playthrough produces.</para>
    ///
    /// <para>THE ONLY SHAPE THIS READS. Three older ones exist on players' disks, and what
    /// each looks like is written down in the engine host's convert verb - the one place that
    /// knows, and the remedy every refusal here names. A reader that still understood an
    /// old shape would be where that shape ROTS, since nothing else would exercise it.</para>
    ///
    /// <para>JSON object keys must be strings, so the integer IDs are written as
    /// invariant decimal strings. Statuses are written as the game's own strings rather
    /// than enum integers, so the file is self-describing and immune to the enum being
    /// renumbered.</para>
    ///
    /// <para><see cref="SimStatus.Untouched"/> is never written: the state never stores
    /// it, and an absent conversation or entry reads back as Untouched. A real save
    /// therefore holds roughly a thousand entries, not the ~113,000 the game tracks.</para>
    ///
    /// <para>Output is UTF-8 with no BOM, unindented, and deterministic: statuses in
    /// SimStatus order, then conversation-ID, then entry-ID order, so two saves of equal
    /// states produce byte-identical files.</para>
    ///
    /// <para>On load, every row goes back through
    /// <see cref="GlobalConversationState.TryMerge"/>. Nothing here assigns a status, so
    /// a hand-edited or partly damaged file cannot lower one, and an unrecognized status
    /// string is skipped and reported rather than taking the load down.</para>
    ///
    /// <para>THE READING AND THE WRITING ARE NOT HERE. They are in
    /// <c>gct_formats::global_state</c>, which is the one implementation of this format, and
    /// this type is how the mod reaches it - see <see cref="GlobalStateNative"/> for why the
    /// state file is linked in where every other format is reached by running the engine
    /// host. What is left here is the shape of the answer: the outcomes a caller acts on and
    /// the load result it acts on them through.</para>
    /// </remarks>
    public static class GlobalStateJson
    {
        /// <summary>
        /// The format version this build writes, and the oldest it will load.
        /// </summary>
        /// <remarks>
        /// Five versions, of which this reads one. The others are described where they are
        /// read - the engine host's convert verb - and what the latest changed is the HEADER: the file
        /// says which format it is in as well as which version of it, as every other
        /// document here does.
        /// </remarks>
        public const int FormatVersion = 5;

        /// <summary>What this document calls itself, in the header every file here carries.</summary>
        /// <remarks>
        /// VERSION 5 IS THE HEADER, and nothing else. The shape of what follows is what
        /// version 4 wrote; what changed is that the file now says WHICH FORMAT it is in
        /// rather than only which version, like every other document this repository
        /// writes. A player's file goes through the engine host's convert verb once, and the mod says
        /// so rather than guessing.
        /// </remarks>
        public const string FormatName = "global-state";

        /// <summary>
        /// The oldest version <see cref="Deserialize(byte[], string)"/> accepts.
        /// </summary>
        /// <remarks>
        /// Equal to <see cref="FormatVersion"/>, and this reader knows no other shape at
        /// all. An older file is refused loudly, as
        /// <see cref="GlobalStateLoadOutcome.UnsupportedVersion"/>, rather than parsed by
        /// a path nothing else exercises - and refused rather than treated as corrupt,
        /// because it is full of real history and the caller must not overwrite it. What an
        /// older shape looks like is written down in the engine host's convert verb, which is the only
        /// thing that reads one and the remedy every refusal names.
        /// </remarks>
        public const int MinimumReadableFormatVersion = FormatVersion;

        /// <summary>Name of the root conversation-map property.</summary>
        public const string ConversationsPropertyName = "conversations";

        /// <summary>
        /// Name of the root orb-list property: the conversation titles whose orb has
        /// been opened, as the game's own <c>ShownOrbs</c> keys.
        /// </summary>
        public const string OrbsPropertyName = "orbs";

        /// <summary>
        /// Upper bound on how many skipped-row descriptions a load result carries.
        /// The count itself is never truncated.
        /// </summary>
        public const int MaxWarnings = 20;

        /// <summary>What the library says this format is, for a line in a log.</summary>
        /// <param name="description">
        /// The format and version it reads, or why it could not be asked.
        /// </param>
        /// <returns>Whether the library answered.</returns>
        /// <remarks>
        /// ASKED RATHER THAN ASSERTED. The point is not the version - this build knows that
        /// - it is that the library is there and answering, said once at startup rather
        /// than discovered at the first save. A mod that cannot reach it cannot read or
        /// write its own state, so that is worth a line either way.
        /// </remarks>
        public static bool TryDescribeFormat(out string description)
        {
            try
            {
                string name = GlobalStateNative.TextAt(
                    GlobalStateNative.FormatName(out nuint length), length);
                description = $"{name} v{GlobalStateNative.FormatVersion()}";
                return true;
            }
            catch (DllNotFoundException missing)
            {
                description =
                    $"the state library is not there ({missing.Message}). Build it with: "
                    + GlobalStateNative.BuildCommand;
                return false;
            }
            catch (EntryPointNotFoundException wrong)
            {
                description =
                    $"the state library is not the one this build expects ({wrong.Message}). "
                    + "Rebuild it with: " + GlobalStateNative.BuildCommand;
                return false;
            }
        }

        /// <summary>Serializes a state to UTF-8 JSON bytes, without a BOM.</summary>
        /// <param name="state">What to write.</param>
        /// <returns>The bytes the file holds.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="state"/> is null.</exception>
        /// <exception cref="InvalidDataException">The library refused to write it.</exception>
        public static byte[] SerializeToUtf8Bytes(GlobalConversationState state)
        {
            if (state == null)
            {
                throw new ArgumentNullException(nameof(state));
            }

            GlobalStateNative.Entry[] entries = EntriesOf(state);
            byte[] orbText = OrbTextOf(state, out nuint[] orbLengths);

            IntPtr written;
            unsafe
            {
                fixed (GlobalStateNative.Entry* first = entries)
                fixed (byte* text = orbText)
                fixed (nuint* lengths = orbLengths)
                {
                    written = GlobalStateNative.Write(
                        first,
                        (nuint)entries.Length,
                        text,
                        (nuint)orbText.Length,
                        lengths,
                        (nuint)orbLengths.Length);
                }
            }

            if (written == IntPtr.Zero)
            {
                // Only a caller that miscounted, or an allocation that failed. Neither is
                // something to write a shorter file over.
                throw new InvalidDataException("The state could not be written.");
            }

            try
            {
                IntPtr first = GlobalStateNative.Bytes(written, out nuint length);
                var bytes = new byte[checked((int)length)];
                if (bytes.Length > 0)
                {
                    Marshal.Copy(first, bytes, 0, bytes.Length);
                }

                return bytes;
            }
            finally
            {
                GlobalStateNative.FreeBytes(written);
            }
        }

        /// <summary>Serializes a state to a JSON string.</summary>
        /// <param name="state">What to write.</param>
        /// <returns>The document, as text.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="state"/> is null.</exception>
        public static string Serialize(GlobalConversationState state)
        {
            return Encoding.UTF8.GetString(SerializeToUtf8Bytes(state));
        }

        /// <summary>Parses UTF-8 JSON bytes back into a state.</summary>
        /// <param name="utf8Json">The file contents.</param>
        /// <param name="sourcePath">
        /// Where the bytes came from, recorded on the result for logging. Pass any
        /// descriptive label when the bytes did not come from a file.
        /// </param>
        /// <returns>
        /// A result whose <see cref="GlobalStateLoadResult.Outcome"/> is
        /// <see cref="GlobalStateLoadOutcome.Loaded"/>,
        /// <see cref="GlobalStateLoadOutcome.Corrupt"/> or
        /// <see cref="GlobalStateLoadOutcome.UnsupportedVersion"/>. This overload
        /// never reports <see cref="GlobalStateLoadOutcome.Missing"/>; only
        /// <see cref="GlobalStateStore"/> knows whether a file exists.
        /// </returns>
        /// <exception cref="ArgumentNullException"><paramref name="utf8Json"/> is null.</exception>
        public static GlobalStateLoadResult Deserialize(byte[] utf8Json, string sourcePath)
        {
            if (utf8Json == null)
            {
                throw new ArgumentNullException(nameof(utf8Json));
            }

            IntPtr read;
            unsafe
            {
                fixed (byte* json = utf8Json)
                {
                    read = GlobalStateNative.Read(json, (nuint)utf8Json.Length);
                }
            }

            if (read == IntPtr.Zero)
            {
                return GlobalStateLoadResult.Corrupt(
                    sourcePath, "The state reader could not be reached.");
            }

            try
            {
                return ResultOf(read, sourcePath);
            }
            finally
            {
                GlobalStateNative.FreeRead(read);
            }
        }

        /// <summary>Parses a JSON string back into a state.</summary>
        /// <param name="json">The document, as text.</param>
        /// <param name="sourcePath">Where it came from, for logging.</param>
        /// <returns>As the other overload.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="json"/> is null.</exception>
        public static GlobalStateLoadResult Deserialize(string json, string sourcePath)
        {
            if (json == null)
            {
                throw new ArgumentNullException(nameof(json));
            }

            return Deserialize(Encoding.UTF8.GetBytes(json), sourcePath);
        }

        /// <summary>What a read said, as the result a caller acts on.</summary>
        private static GlobalStateLoadResult ResultOf(IntPtr read, string sourcePath)
        {
            int outcome = GlobalStateNative.Outcome(read);
            if (outcome != GlobalStateNative.OutcomeLoaded)
            {
                string why = GlobalStateNative.TextAt(
                    GlobalStateNative.Message(read, out nuint length), length);

                // THE TWO REFUSALS MEAN DIFFERENT THINGS TO THE CALLER. A document this
                // build cannot read is full of real history and must be left alone; one
                // that will not parse at all holds nothing to lose.
                return outcome == GlobalStateNative.OutcomeUnsupported
                    ? GlobalStateLoadResult.UnsupportedVersion(sourcePath, why)
                    : GlobalStateLoadResult.Corrupt(sourcePath, why);
            }

            var state = new GlobalConversationState();
            foreach (GlobalStateNative.Entry entry in EntriesIn(read))
            {
                state.TryMerge(
                    entry.Conversation,
                    entry.DialogueEntry,
                    SimStatusNames.ToGameString((SimStatus)entry.Status),
                    out _);
            }

            nuint orbs = GlobalStateNative.OrbCount(read);
            for (nuint at = 0; at < orbs; at++)
            {
                state.MergeOrb(GlobalStateNative.TextAt(
                    GlobalStateNative.Orb(read, at, out nuint length), length));
            }

            var warnings = new List<string>();
            nuint described = GlobalStateNative.WarningCount(read);
            for (nuint at = 0; at < described; at++)
            {
                warnings.Add(GlobalStateNative.TextAt(
                    GlobalStateNative.Warning(read, at, out nuint length), length));
            }

            return GlobalStateLoadResult.Loaded(
                sourcePath,
                state,
                checked((int)GlobalStateNative.Skipped(read)),
                warnings);
        }

        /// <summary>Everything a read holds, copied out in one call.</summary>
        private static GlobalStateNative.Entry[] EntriesIn(IntPtr read)
        {
            int count = checked((int)GlobalStateNative.EntryCount(read));
            var entries = new GlobalStateNative.Entry[count];
            if (count == 0)
            {
                return entries;
            }

            unsafe
            {
                fixed (GlobalStateNative.Entry* into = entries)
                {
                    GlobalStateNative.Entries(read, into, (nuint)count);
                }
            }

            return entries;
        }

        /// <summary>A state's entries, in the shape that crosses.</summary>
        private static GlobalStateNative.Entry[] EntriesOf(GlobalConversationState state)
        {
            var entries = new List<GlobalStateNative.Entry>();
            foreach (GlobalStatusEntry entry in state.EnumerateEntriesInIdOrder())
            {
                entries.Add(new GlobalStateNative.Entry
                {
                    Conversation = entry.ConversationId,
                    DialogueEntry = entry.DialogueEntryId,
                    Status = (int)entry.Status,
                });
            }

            return entries.ToArray();
        }

        /// <summary>
        /// A state's orb titles as one UTF-8 buffer, with the length of each beside it.
        /// </summary>
        /// <remarks>
        /// ONE BUFFER RATHER THAN ONE POINTER PER TITLE, which is one allocation here
        /// instead of one per orb and nothing to pin but two arrays.
        /// </remarks>
        private static byte[] OrbTextOf(GlobalConversationState state, out nuint[] lengths)
        {
            var text = new List<byte>();
            var sizes = new List<nuint>();
            foreach (string title in state.EnumerateOrbs())
            {
                byte[] bytes = Encoding.UTF8.GetBytes(title);
                text.AddRange(bytes);
                sizes.Add((nuint)bytes.Length);
            }

            lengths = sizes.ToArray();
            return text.ToArray();
        }
    }
}
