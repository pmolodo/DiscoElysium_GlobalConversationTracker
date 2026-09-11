// SPDX-License-Identifier: MIT
using System.Globalization;
using System.Text.Json;

using GlobalConversationTracker;
using GlobalConversationTracker.Core;
using GlobalConversationTracker.Persistence;

namespace FormatConvert;

/// <summary>
/// The shapes the global state file used to be written in, and how to bring one forward.
/// </summary>
/// <remarks>
/// <para>THE ONLY PLACE THAT KNOWS ANY OF THIS. Every other reader in the repository takes
/// the current version of the format it was written for and refuses anything else, which
/// is what keeps an old shape from ROTTING inside a live reader that nothing exercises.
/// This tool is where an old shape is written down, tested, and used - once, to produce a
/// current file - so the knowledge lives beside the only code that needs it and leaves the
/// assembly the game loads.</para>
///
/// <para>THE THREE SHAPES, and what changed between them:</para>
///
/// <list type="bullet">
/// <item>1 and 2 wrote a STATUS PER ENTRY - <c>conversations: { "10": { "5": "WasDisplayed" } }</c>
/// - which spends a key and a quoted status on every entry recorded.</item>
/// <item>3 grouped the entries under their status and listed the ids -
/// <c>conversations: { "WasDisplayed": { "10": [5, 6, 7] } }</c>.</item>
/// <item>4, the current one, run-encodes each list as a string - <c>"5-7,9"</c> - which took
/// the worst-case fixture from 423 KB to 22.5 KB.</item>
/// </list>
///
/// <para>IT REFUSES RATHER THAN SKIPS, which is the one way it deliberately differs from the
/// reader it replaced. The runtime reader drops a row it cannot read and warns, because a
/// player's session must go on; a conversion that dropped a row would write a file missing
/// history nobody asked it to lose, so anything unreadable stops the whole file.</para>
/// </remarks>
public static class LegacyGlobalState
{
    /// <summary>The last version that wrote a status per entry.</summary>
    public const int PerEntryVersion = 2;

    /// <summary>The version that grouped entries by status and listed their ids.</summary>
    public const int GroupedArrayVersion = 3;

    /// <summary>The last version that ran the entry ids together into a string.</summary>
    /// <remarks>
    /// The shape version 5 keeps. What 5 changed is the HEADER - it names the format as
    /// well as the version, like every other document here - so converting a version 4
    /// file is reading this shape and writing it back with a header on it.
    /// </remarks>
    internal const int RunEncodedVersion = 4;

    /// <summary>The oldest version there has ever been.</summary>
    private const int FirstVersion = 1;

    /// <summary>What a file called its version before it named its format.</summary>
    private const string OldVersionProperty = "version";

    /// <summary>One file of an older version, as the bytes the current one would hold.</summary>
    /// <param name="utf8Json">The file's contents.</param>
    /// <param name="sourcePath">Where they came from, for the message.</param>
    /// <returns>The same history, written in the current format.</returns>
    /// <exception cref="InvalidDataException">
    /// It is not JSON, is not a version this converts, or carries anything this cannot read
    /// exactly.
    /// </exception>
    public static byte[] ConvertToUtf8Bytes(byte[] utf8Json, string sourcePath)
    {
        using JsonDocument document = Parse(utf8Json, sourcePath);
        JsonElement root = document.RootElement;
        if (root.ValueKind != JsonValueKind.Object)
        {
            throw Refused(sourcePath, $"its root is {root.ValueKind} rather than an object");
        }

        int version = VersionOf(root, sourcePath);

        // EVERY VERSION BELOW THE CURRENT ONE, derived rather than listed. A bound that has
        // to be remembered when a version is added is a bound that will be forgotten again -
        // which is how version 3 became a shape this repository had written and nothing
        // could read (de-bnjy.7).
        if (version < FirstVersion || version >= GlobalStateJson.FormatVersion)
        {
            throw Refused(
                sourcePath,
                $"version {version} is not an older version this build converts (expected "
                + $"{FirstVersion} to {GlobalStateJson.FormatVersion - 1})");
        }

        var state = new GlobalConversationState();
        JsonElement conversations = Property(
            root, GlobalStateJson.ConversationsPropertyName, sourcePath);

        if (version > PerEntryVersion)
        {
            ReadGrouped(conversations, state, sourcePath, version);
        }
        else
        {
            ReadPerEntry(conversations, state, sourcePath);
        }

        ReadOrbs(root, state, sourcePath);
        return GlobalStateJson.SerializeToUtf8Bytes(state);
    }

    /// <summary>Reads the shape that groups ids under a status, as versions 3 and 4 wrote it.</summary>
    private static void ReadGrouped(
        JsonElement conversations, GlobalConversationState state, string sourcePath, int version)
    {
        bool runEncoded = version >= RunEncodedVersion;
        foreach (JsonProperty status in Members(conversations, "conversations", sourcePath))
        {
            if (!SimStatusNames.TryParse(status.Name, out _))
            {
                throw Refused(sourcePath, $"'{status.Name}' is not a status a state records");
            }

            foreach (JsonProperty conversation in Members(status.Value, status.Name, sourcePath))
            {
                int id = IdOf(conversation.Name, sourcePath);

                foreach (long entry in Ids(
                    conversation.Value, runEncoded, id, status.Name, sourcePath))
                {
                    Merge(state, id, (int)entry, status.Name, sourcePath);
                }
            }
        }
    }

    /// <summary>One conversation'''s entry ids, in whichever way its version wrote them.</summary>
    /// <remarks>
    /// A LIST UNTIL VERSION 3 AND A RUN FROM 4. The run form is what took the worst-case
    /// fixture from 423 KB to 22.5 KB, and it is read through the same SparseOrder every
    /// other file here shares rather than a second parser for the same grammar.
    /// </remarks>
    private static IEnumerable<long> Ids(
        JsonElement entries, bool runEncoded, int conversation, string status, string sourcePath)
    {
        // WHICH SHAPE, BY THE VERSION AT THE TOP rather than by looking at the value. A
        // version 4 file carrying an array is a DAMAGED version 4 file and not a version 3
        // one, and sniffing the value would quietly accept what the version denies.
        JsonValueKind wanted = runEncoded ? JsonValueKind.String : JsonValueKind.Array;
        if (entries.ValueKind != wanted)
        {
            throw Refused(
                sourcePath,
                $"conversation {conversation} in '{status}' is {entries.ValueKind} where "
                + (runEncoded ? "a run-encoded string" : "a list of ids") + " belongs");
        }

        if (runEncoded)
        {
            return SparseOrder.UnpackRange(
                entries.GetString() ?? string.Empty,
                $"Conversation {conversation} in '{status}'");
        }

        var ids = new List<long>();
        foreach (JsonElement entry in entries.EnumerateArray())
        {
            if (entry.ValueKind != JsonValueKind.Number || !entry.TryGetInt64(out long read))
            {
                throw Refused(
                    sourcePath,
                    $"conversation {conversation} in '{status}' holds {entry} where an entry "
                    + "id belongs");
            }

            ids.Add(read);
        }

        return ids;
    }

    /// <summary>Reads the shape that names a status per entry, as versions 1 and 2 wrote it.</summary>
    private static void ReadPerEntry(
        JsonElement conversations, GlobalConversationState state, string sourcePath)
    {
        foreach (JsonProperty conversation in Members(conversations, "conversations", sourcePath))
        {
            int id = IdOf(conversation.Name, sourcePath);
            foreach (JsonProperty entry in Members(conversation.Value, conversation.Name, sourcePath))
            {
                if (entry.Value.ValueKind != JsonValueKind.String)
                {
                    throw Refused(
                        sourcePath,
                        $"the status of {id}/{entry.Name} is {entry.Value.ValueKind} rather "
                        + "than a name");
                }

                Merge(state, id, IdOf(entry.Name, sourcePath), entry.Value.GetString(), sourcePath);
            }
        }
    }

    /// <summary>Reads the opened orbs, which a version 1 file simply does not have.</summary>
    private static void ReadOrbs(
        JsonElement root, GlobalConversationState state, string sourcePath)
    {
        if (!root.TryGetProperty(GlobalStateJson.OrbsPropertyName, out JsonElement orbs))
        {
            return;
        }

        if (orbs.ValueKind != JsonValueKind.Array)
        {
            throw Refused(
                sourcePath,
                $"'{GlobalStateJson.OrbsPropertyName}' is {orbs.ValueKind} rather than a list");
        }

        foreach (JsonElement orb in orbs.EnumerateArray())
        {
            string? title = orb.ValueKind == JsonValueKind.String ? orb.GetString() : null;
            if (string.IsNullOrEmpty(title))
            {
                throw Refused(sourcePath, $"an orb is {orb} rather than a conversation title");
            }

            state.MergeOrb(title!);
        }
    }

    /// <summary>Records one entry, refusing a status name the state does not know.</summary>
    private static void Merge(
        GlobalConversationState state,
        int conversation,
        int entry,
        string? statusName,
        string sourcePath)
    {
        if (!state.TryMerge(conversation, entry, statusName, out _))
        {
            throw Refused(
                sourcePath, $"'{statusName}' is not a status, on {conversation}/{entry}");
        }
    }

    /// <summary>The version a file records.</summary>
    private static int VersionOf(JsonElement root, string sourcePath)
    {
        if (!root.TryGetProperty(OldVersionProperty, out JsonElement version)
            || version.ValueKind != JsonValueKind.Number
            || !version.TryGetInt32(out int read))
        {
            throw Refused(
                sourcePath, $"it records no {OldVersionProperty} to convert from");
        }

        return read;
    }

    /// <summary>One required property, as an object.</summary>
    private static JsonElement Property(JsonElement root, string name, string sourcePath)
    {
        if (!root.TryGetProperty(name, out JsonElement found))
        {
            throw Refused(sourcePath, $"it has no '{name}'");
        }

        return found;
    }

    /// <summary>The members of an object, refusing anything that is not one.</summary>
    private static IEnumerable<JsonProperty> Members(
        JsonElement value, string what, string sourcePath)
    {
        if (value.ValueKind != JsonValueKind.Object)
        {
            throw Refused(sourcePath, $"'{what}' is {value.ValueKind} rather than an object");
        }

        return value.EnumerateObject();
    }

    /// <summary>One key as the id it has to be.</summary>
    private static int IdOf(string key, string sourcePath)
    {
        if (!int.TryParse(key, NumberStyles.Integer, CultureInfo.InvariantCulture, out int id))
        {
            throw Refused(sourcePath, $"'{key}' is not an id");
        }

        return id;
    }

    private static JsonDocument Parse(byte[] utf8Json, string sourcePath)
    {
        try
        {
            return JsonDocument.Parse(utf8Json);
        }
        catch (JsonException malformed)
        {
            throw Refused(sourcePath, $"it is not valid JSON: {malformed.Message}");
        }
    }

    private static InvalidDataException Refused(string sourcePath, string why) =>
        new($"Could not convert '{sourcePath}': {why}.");
}
