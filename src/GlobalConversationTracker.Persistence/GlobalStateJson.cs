// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.Json;

namespace GlobalConversationTracker.Persistence
{
    /// <summary>
    /// The on-disk JSON format for <see cref="GlobalConversationState"/>: pure
    /// bytes-to-state conversion with no file IO.
    /// </summary>
    /// <remarks>
    /// <para>Shape (format version 3):</para>
    /// <code>
    /// {"version":3,"conversations":{"WasDisplayed":{"3":[17,19]},"WasOffered":{"3":[18]}},"orbs":[]}
    /// </code>
    /// <para>Grouped by status, then by conversation, then a plain array of entry IDs.
    /// A status string is written once per conversation that has entries in it rather
    /// than once per entry, and an entry costs the digits of its ID: a state recording
    /// every one of the game's ~113,000 entries is about 0.4 MB this way against 2.3 MB
    /// spelled out per entry.</para>
    ///
    /// <para>Shape (format versions 1 and 2), still read:</para>
    /// <code>
    /// {"version":2,"conversations":{"3":{"17":"WasDisplayed","18":"WasOffered"}},"orbs":[]}
    /// </code>
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
    /// </remarks>
    public static class GlobalStateJson
    {
        /// <summary>
        /// The format version this build writes. It reads this version and every older
        /// one; see the version gate in <see cref="Deserialize(string, string)"/>.
        /// </summary>
        /// <remarks>
        /// Version 2 added <see cref="OrbsPropertyName"/>. A version 1 file is a version
        /// 2 file with no orbs, which is why reading one needs no conversion beyond
        /// letting the property be absent. Version 3 regrouped
        /// <see cref="ConversationsPropertyName"/> by status, which is a different shape
        /// rather than another optional property, so it has a reader of its own.
        /// </remarks>
        public const int FormatVersion = 3;

        /// <summary>
        /// The newest version written in the per-entry shape, which this build still
        /// reads.
        /// </summary>
        /// <remarks>
        /// A migration affordance with an expiry, not a feature. Once no profile in use
        /// holds a file this old, the per-entry reader and this constant go, and the
        /// minimum accepted version rises to 3 so an ancient file is refused loudly
        /// instead of being parsed by code nothing exercises. Tracked as de-pc2.
        /// </remarks>
        public const int LegacyPerEntryFormatVersion = 2;

        /// <summary>Name of the root version property.</summary>
        public const string VersionPropertyName = "version";

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

        /// <summary>Serializes a state to UTF-8 JSON bytes, without a BOM.</summary>
        /// <exception cref="ArgumentNullException"><paramref name="state"/> is null.</exception>
        public static byte[] SerializeToUtf8Bytes(GlobalConversationState state)
        {
            if (state == null)
            {
                throw new ArgumentNullException(nameof(state));
            }

            using var buffer = new MemoryStream();
            using (var writer = new Utf8JsonWriter(buffer, new JsonWriterOptions { Indented = false }))
            {
                writer.WriteStartObject();
                writer.WriteNumber(VersionPropertyName, FormatVersion);
                writer.WritePropertyName(ConversationsPropertyName);
                writer.WriteStartObject();

                foreach (KeyValuePair<SimStatus, SortedDictionary<int, List<int>>> status
                    in GroupByStatus(state))
                {
                    writer.WritePropertyName(SimStatusNames.ToGameString(status.Key));
                    writer.WriteStartObject();
                    foreach (KeyValuePair<int, List<int>> conversation in status.Value)
                    {
                        writer.WritePropertyName(ToKey(conversation.Key));
                        writer.WriteStartArray();
                        foreach (int dialogueEntryId in conversation.Value)
                        {
                            writer.WriteNumberValue(dialogueEntryId);
                        }

                        writer.WriteEndArray();
                    }

                    writer.WriteEndObject();
                }

                writer.WriteEndObject();

                // Always written, even when empty, so "no orbs" and "orbs not supported"
                // do not look identical.
                writer.WritePropertyName(OrbsPropertyName);
                writer.WriteStartArray();
                foreach (string title in state.EnumerateOrbs())
                {
                    writer.WriteStringValue(title);
                }

                writer.WriteEndArray();

                writer.WriteEndObject();
            }

            return buffer.ToArray();
        }

        /// <summary>
        /// Buckets a state's entries by status, then by conversation.
        /// </summary>
        /// <remarks>
        /// Ordered throughout, because the file is: statuses by their SimStatus value,
        /// conversations by ID, and entry IDs ascending - the last for free, since
        /// EnumerateEntriesInIdOrder already yields them that way and each list is
        /// appended to in that order.
        ///
        /// Keyed on whatever statuses are actually present rather than on a fixed list
        /// of the two a state is supposed to hold, so a value that should never be
        /// stored is written out and read back rather than silently dropped here.
        /// </remarks>
        private static SortedDictionary<SimStatus, SortedDictionary<int, List<int>>>
            GroupByStatus(GlobalConversationState state)
        {
            var byStatus =
                new SortedDictionary<SimStatus, SortedDictionary<int, List<int>>>();

            foreach (GlobalStatusEntry entry in state.EnumerateEntriesInIdOrder())
            {
                if (!byStatus.TryGetValue(
                        entry.Status, out SortedDictionary<int, List<int>>? conversations))
                {
                    conversations = new SortedDictionary<int, List<int>>();
                    byStatus.Add(entry.Status, conversations);
                }

                if (!conversations.TryGetValue(
                        entry.ConversationId, out List<int>? dialogueEntryIds))
                {
                    dialogueEntryIds = new List<int>();
                    conversations.Add(entry.ConversationId, dialogueEntryIds);
                }

                dialogueEntryIds.Add(entry.DialogueEntryId);
            }

            return byStatus;
        }

        /// <summary>Serializes a state to a JSON string.</summary>
        /// <exception cref="ArgumentNullException"><paramref name="state"/> is null.</exception>
        public static string Serialize(GlobalConversationState state)
        {
            return Encoding.UTF8.GetString(SerializeToUtf8Bytes(state));
        }

        /// <summary>
        /// Converts a format version 1 or 2 document to the current grouped format.
        /// </summary>
        /// <remarks>
        /// Unlike the runtime reader, conversion is strict: a document that would lose
        /// even one unreadable row is rejected instead of producing a partial migration.
        /// </remarks>
        /// <exception cref="ArgumentNullException">
        /// <paramref name="utf8Json"/> or <paramref name="sourcePath"/> is null.
        /// </exception>
        /// <exception cref="InvalidDataException">
        /// The input is corrupt, is not format version 1 or 2, or contains an unreadable
        /// row.
        /// </exception>
        public static byte[] ConvertLegacyToUtf8Bytes(byte[] utf8Json, string sourcePath)
        {
            if (utf8Json == null)
            {
                throw new ArgumentNullException(nameof(utf8Json));
            }

            if (sourcePath == null)
            {
                throw new ArgumentNullException(nameof(sourcePath));
            }

            GlobalStateLoadResult result = DeserializeWithFormatVersion(
                utf8Json, sourcePath, out int? sourceFormatVersion);
            if (!result.IsLoaded)
            {
                throw new InvalidDataException(
                    $"Could not convert '{sourcePath}': {result.ErrorMessage ?? result.Outcome.ToString()}.");
            }

            int version = sourceFormatVersion.GetValueOrDefault();
            if (!sourceFormatVersion.HasValue
                || version < 1
                || version > LegacyPerEntryFormatVersion)
            {
                throw new InvalidDataException(
                    $"Could not convert '{sourcePath}': format version {version} is not "
                    + $"a supported legacy version (expected 1 or {LegacyPerEntryFormatVersion}).");
            }

            if (result.SkippedRowCount > 0)
            {
                string warnings = string.Join(" ", result.Warnings);
                throw new InvalidDataException(
                    $"Could not convert '{sourcePath}': {result.SkippedRowCount} unreadable row(s) "
                    + $"would be lost. {warnings}");
            }

            return SerializeToUtf8Bytes(result.RequireState());
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
        public static GlobalStateLoadResult Deserialize(byte[] utf8Json, string sourcePath)
        {
            if (utf8Json == null)
            {
                throw new ArgumentNullException(nameof(utf8Json));
            }

            return DeserializeWithFormatVersion(utf8Json, sourcePath, out _);
        }

        private static GlobalStateLoadResult DeserializeWithFormatVersion(
            byte[] utf8Json,
            string sourcePath,
            out int? formatVersion)
        {
            JsonDocument document;
            try
            {
                document = JsonDocument.Parse(utf8Json);
            }
            catch (JsonException ex)
            {
                // Covers the whole "torn write" family: zero bytes, half an object,
                // trailing garbage.
                formatVersion = null;
                return GlobalStateLoadResult.Corrupt(sourcePath, $"Not valid JSON: {ex.Message}");
            }

            using (document)
            {
                return ReadRoot(document.RootElement, sourcePath, out formatVersion);
            }
        }

        /// <summary>Parses a JSON string back into a state.</summary>
        public static GlobalStateLoadResult Deserialize(string json, string sourcePath)
        {
            if (json == null)
            {
                throw new ArgumentNullException(nameof(json));
            }

            return Deserialize(Encoding.UTF8.GetBytes(json), sourcePath);
        }

        private static GlobalStateLoadResult ReadRoot(
            JsonElement root,
            string sourcePath,
            out int? formatVersion)
        {
            formatVersion = null;

            if (root.ValueKind != JsonValueKind.Object)
            {
                return GlobalStateLoadResult.Corrupt(
                    sourcePath, $"Root element is {root.ValueKind}, expected an object.");
            }

            if (!root.TryGetProperty(VersionPropertyName, out JsonElement versionElement))
            {
                return GlobalStateLoadResult.Corrupt(
                    sourcePath, $"Missing required '{VersionPropertyName}' property.");
            }

            if (versionElement.ValueKind != JsonValueKind.Number
                || !versionElement.TryGetInt32(out int version))
            {
                return GlobalStateLoadResult.Corrupt(
                    sourcePath, $"'{VersionPropertyName}' is not an integer.");
            }

            formatVersion = version;

            if (version > FormatVersion)
            {
                // Deliberately not Corrupt: a newer version's file is presumably full of
                // real history, so the caller must refuse to overwrite it rather than
                // fall back to a stale backup.
                return GlobalStateLoadResult.UnsupportedVersion(
                    sourcePath,
                    $"File format version {version} is newer than this build, which writes version {FormatVersion}.");
            }

            // Older versions are read, not rejected. Each version so far has only ADDED
            // an optional root property, so an old file is a new file with those absent,
            // and the readers below treat absent as empty. Rejecting them would turn into
            // "refuse to save" at the caller, silently stopping tracking on every
            // existing install the first time this constant was bumped.

            if (!root.TryGetProperty(ConversationsPropertyName, out JsonElement conversations))
            {
                return GlobalStateLoadResult.Corrupt(
                    sourcePath, $"Missing required '{ConversationsPropertyName}' property.");
            }

            if (conversations.ValueKind != JsonValueKind.Object)
            {
                return GlobalStateLoadResult.Corrupt(
                    sourcePath,
                    $"'{ConversationsPropertyName}' is {conversations.ValueKind}, expected an object.");
            }

            var state = new GlobalConversationState();
            var warnings = new List<string>();
            int skippedRowCount = 0;

            if (version > LegacyPerEntryFormatVersion)
            {
                ReadGroupedConversations(conversations, state, warnings, ref skippedRowCount);
            }
            else
            {
                ReadPerEntryConversations(conversations, state, warnings, ref skippedRowCount);
            }

            ReadOrbs(root, state, warnings, ref skippedRowCount);

            return GlobalStateLoadResult.Loaded(sourcePath, state, skippedRowCount, warnings);
        }

        /// <summary>
        /// Reads the grouped shape: status, then conversation, then an array of IDs.
        /// </summary>
        /// <remarks>
        /// An unrecognized status name costs one warning for the whole block rather than
        /// one per row. There are only ever a handful of blocks, and spending the warning
        /// budget on repetitions of the same fact would push out every other complaint in
        /// the file.
        /// </remarks>
        private static void ReadGroupedConversations(
            JsonElement conversations,
            GlobalConversationState state,
            List<string> warnings,
            ref int skippedRowCount)
        {
            foreach (JsonProperty status in conversations.EnumerateObject())
            {
                if (status.Value.ValueKind != JsonValueKind.Object)
                {
                    skippedRowCount++;
                    AddWarning(
                        warnings,
                        $"Status group '{status.Name}' is {status.Value.ValueKind}, expected an object; skipped.");
                    continue;
                }

                if (!SimStatusNames.TryParse(status.Name, out _))
                {
                    int dropped = CountGroupedRows(status.Value);
                    skippedRowCount += dropped;
                    AddWarning(
                        warnings,
                        $"Unrecognized status '{status.Name}'; skipped {dropped} row(s).");
                    continue;
                }

                foreach (JsonProperty conversation in status.Value.EnumerateObject())
                {
                    if (!TryParseId(conversation.Name, out int conversationId))
                    {
                        int dropped = CountIds(conversation.Value);
                        skippedRowCount += dropped;
                        AddWarning(
                            warnings,
                            $"Conversation key '{conversation.Name}' in '{status.Name}' is not an integer; skipped {dropped} row(s).");
                        continue;
                    }

                    if (conversation.Value.ValueKind != JsonValueKind.Array)
                    {
                        skippedRowCount++;
                        AddWarning(
                            warnings,
                            $"Conversation {conversationId} in '{status.Name}' is {conversation.Value.ValueKind}, expected an array; skipped.");
                        continue;
                    }

                    foreach (JsonElement dialogueEntry in conversation.Value.EnumerateArray())
                    {
                        if (dialogueEntry.ValueKind != JsonValueKind.Number
                            || !dialogueEntry.TryGetInt32(out int dialogueEntryId))
                        {
                            skippedRowCount++;
                            AddWarning(
                                warnings,
                                $"Dialogue entry ID in conversation {conversationId} of '{status.Name}' is not an integer; skipped.");
                            continue;
                        }

                        // TryMerge for the same reason the per-entry reader uses it: a
                        // status already in memory cannot be pulled back down by a file.
                        // The name is known good by here, so a false return is impossible
                        // and is not treated as a skipped row.
                        state.TryMerge(conversationId, dialogueEntryId, status.Name, out _);
                    }
                }
            }
        }

        /// <summary>
        /// Reads the per-entry shape written by format versions 1 and 2.
        /// </summary>
        /// <remarks>
        /// Goes when <see cref="LegacyPerEntryFormatVersion"/> does; see de-pc2.
        /// </remarks>
        private static void ReadPerEntryConversations(
            JsonElement conversations,
            GlobalConversationState state,
            List<string> warnings,
            ref int skippedRowCount)
        {
            foreach (JsonProperty conversation in conversations.EnumerateObject())
            {
                if (!TryParseId(conversation.Name, out int conversationId))
                {
                    int dropped = CountRows(conversation.Value);
                    skippedRowCount += dropped;
                    AddWarning(
                        warnings,
                        $"Conversation key '{conversation.Name}' is not an integer; skipped {dropped} row(s).");
                    continue;
                }

                if (conversation.Value.ValueKind != JsonValueKind.Object)
                {
                    skippedRowCount++;
                    AddWarning(
                        warnings,
                        $"Conversation {conversationId} is {conversation.Value.ValueKind}, expected an object; skipped.");
                    continue;
                }

                foreach (JsonProperty entry in conversation.Value.EnumerateObject())
                {
                    if (!TryParseId(entry.Name, out int dialogueEntryId))
                    {
                        skippedRowCount++;
                        AddWarning(
                            warnings,
                            $"Dialogue entry key '{entry.Name}' in conversation {conversationId} is not an integer; skipped.");
                        continue;
                    }

                    if (entry.Value.ValueKind != JsonValueKind.String)
                    {
                        skippedRowCount++;
                        AddWarning(
                            warnings,
                            $"Status of {conversationId}/{dialogueEntryId} is {entry.Value.ValueKind}, expected a string; skipped.");
                        continue;
                    }

                    string? statusName = entry.Value.GetString();

                    // TryMerge, never an assignment: an unknown status string is a skipped
                    // row rather than a failed load, and a status that is lower than what
                    // is already in memory cannot pull anything back down.
                    if (!state.TryMerge(conversationId, dialogueEntryId, statusName, out _))
                    {
                        skippedRowCount++;
                        AddWarning(
                            warnings,
                            $"Unrecognized status '{statusName}' for {conversationId}/{dialogueEntryId}; skipped.");
                    }
                }
            }
        }

        /// <summary>
        /// Reads the orb list into the state. An absent property is not a fault - that
        /// is exactly what a version 1 file looks like - but a present one of the wrong
        /// shape is, and so is a bad element inside a good array.
        /// </summary>
        private static void ReadOrbs(
            JsonElement root,
            GlobalConversationState state,
            List<string> warnings,
            ref int skippedRowCount)
        {
            if (!root.TryGetProperty(OrbsPropertyName, out JsonElement orbs))
            {
                return;
            }

            if (orbs.ValueKind != JsonValueKind.Array)
            {
                skippedRowCount++;
                AddWarning(
                    warnings,
                    $"'{OrbsPropertyName}' is {orbs.ValueKind}, expected an array; skipped.");
                return;
            }

            foreach (JsonElement orb in orbs.EnumerateArray())
            {
                if (orb.ValueKind != JsonValueKind.String)
                {
                    skippedRowCount++;
                    AddWarning(
                        warnings,
                        $"Orb entry is {orb.ValueKind}, expected a string; skipped.");
                    continue;
                }

                string? title = orb.GetString();
                if (string.IsNullOrEmpty(title))
                {
                    skippedRowCount++;
                    AddWarning(warnings, "Orb entry is an empty conversation title; skipped.");
                    continue;
                }

                state.MergeOrb(title);
            }
        }

        private static void AddWarning(List<string> warnings, string warning)
        {
            if (warnings.Count < MaxWarnings)
            {
                warnings.Add(warning);
            }
        }

        private static int CountRows(JsonElement conversationValue)
        {
            if (conversationValue.ValueKind != JsonValueKind.Object)
            {
                return 1;
            }

            int count = 0;
            foreach (JsonProperty _ in conversationValue.EnumerateObject())
            {
                count++;
            }

            return count;
        }

        /// <summary>How many rows a grouped status block holds, for the skipped count.</summary>
        private static int CountGroupedRows(JsonElement statusValue)
        {
            int count = 0;
            foreach (JsonProperty conversation in statusValue.EnumerateObject())
            {
                count += CountIds(conversation.Value);
            }

            return count;
        }

        /// <summary>
        /// How many rows one conversation's ID array holds. Anything that is not an array
        /// is one unusable row rather than none, so a malformed file cannot report that it
        /// skipped nothing.
        /// </summary>
        private static int CountIds(JsonElement conversationValue)
        {
            if (conversationValue.ValueKind != JsonValueKind.Array)
            {
                return 1;
            }

            int count = 0;
            foreach (JsonElement _ in conversationValue.EnumerateArray())
            {
                count++;
            }

            return count;
        }

        private static string ToKey(int id) => id.ToString(CultureInfo.InvariantCulture);

        private static bool TryParseId(string key, out int id)
        {
            return int.TryParse(key, NumberStyles.Integer, CultureInfo.InvariantCulture, out id);
        }
    }
}
