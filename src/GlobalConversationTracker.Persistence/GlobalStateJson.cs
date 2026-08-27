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
    /// <para>Shape (format version 1):</para>
    /// <code>
    /// {"version":1,"conversations":{"3":{"17":"WasDisplayed","18":"WasOffered"}}}
    /// </code>
    /// <para>
    /// JSON object keys must be strings, so the integer conversation and dialogue
    /// entry IDs are written as invariant decimal strings. Statuses are written as
    /// the game's own strings rather than enum integers, so the file is
    /// self-describing and immune to the enum being renumbered.
    /// </para>
    /// <para>
    /// <see cref="SimStatus.Untouched"/> is never written: the state never stores it,
    /// and an absent conversation or entry already reads back as Untouched. A real
    /// save therefore holds roughly a thousand entries, not the ~113,000 the game
    /// tracks (in one real save: 111,467 Untouched, 142 WasOffered, 1,331
    /// WasDisplayed).
    /// </para>
    /// <para>
    /// Output is UTF-8 with no BOM, unindented, and deterministic: entries are
    /// written in conversation-ID then entry-ID order, so two saves of equal states
    /// produce byte-identical files.
    /// </para>
    /// <para>
    /// On load, every row goes back through
    /// <see cref="GlobalConversationState.TryMerge"/>. Nothing here ever assigns a
    /// status, so a hand-edited or partly damaged file cannot lower a status, and an
    /// unrecognized status string is skipped and reported rather than taking the
    /// whole load down.
    /// </para>
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
        /// letting the property be absent.
        /// </remarks>
        public const int FormatVersion = 2;

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

                // EnumerateEntriesInIdOrder is sorted by conversation then entry, so a
                // single pass can close one conversation object and open the next as the
                // conversation ID changes. Linear in the number of stored entries.
                int currentConversationId = 0;
                bool inConversation = false;
                foreach (GlobalStatusEntry entry in state.EnumerateEntriesInIdOrder())
                {
                    if (!inConversation || entry.ConversationId != currentConversationId)
                    {
                        if (inConversation)
                        {
                            writer.WriteEndObject();
                        }

                        writer.WritePropertyName(ToKey(entry.ConversationId));
                        writer.WriteStartObject();
                        currentConversationId = entry.ConversationId;
                        inConversation = true;
                    }

                    writer.WriteString(ToKey(entry.DialogueEntryId), SimStatusNames.ToGameString(entry.Status));
                }

                if (inConversation)
                {
                    writer.WriteEndObject();
                }

                writer.WriteEndObject();

                // Always written, even when empty, so a file's own shape says which
                // version wrote it rather than leaving "no orbs" and "orbs not supported"
                // looking identical.
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

        /// <summary>Serializes a state to a JSON string.</summary>
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
        public static GlobalStateLoadResult Deserialize(byte[] utf8Json, string sourcePath)
        {
            if (utf8Json == null)
            {
                throw new ArgumentNullException(nameof(utf8Json));
            }

            JsonDocument document;
            try
            {
                document = JsonDocument.Parse(utf8Json);
            }
            catch (JsonException ex)
            {
                // Covers the whole "torn write" family: zero bytes, half an object,
                // trailing garbage.
                return GlobalStateLoadResult.Corrupt(sourcePath, $"Not valid JSON: {ex.Message}");
            }

            using (document)
            {
                return ReadRoot(document.RootElement, sourcePath);
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

        private static GlobalStateLoadResult ReadRoot(JsonElement root, string sourcePath)
        {
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

            if (version > FormatVersion)
            {
                // Deliberately not Corrupt: a newer version's file is presumably full of
                // real history, so the caller must refuse to overwrite it rather than
                // fall back to a stale backup.
                return GlobalStateLoadResult.UnsupportedVersion(
                    sourcePath,
                    $"File format version {version} is newer than this build, which writes version {FormatVersion}.");
            }

            // Older versions are read, not rejected. Every version so far has only ever
            // ADDED an optional root property, so an old file is a new file with those
            // properties absent, and the readers below already treat absent as empty.
            // Rejecting them instead would be the worse failure by far: the caller turns
            // UnsupportedVersion into "refuse to save", so treating a user's own older
            // file as unreadable would silently stop tracking on every existing install
            // the first time this constant was bumped.

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

            ReadOrbs(root, state, warnings, ref skippedRowCount);

            return GlobalStateLoadResult.Loaded(sourcePath, state, skippedRowCount, warnings);
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

        private static string ToKey(int id) => id.ToString(CultureInfo.InvariantCulture);

        private static bool TryParseId(string key, out int id)
        {
            return int.TryParse(key, NumberStyles.Integer, CultureInfo.InvariantCulture, out id);
        }
    }
}
