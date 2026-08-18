using System.Globalization;
using NtwtfDecode;
using UnifiedConversationTracker;
using UnifiedConversationTracker.Persistence;

namespace UnifiedStateCheck;

/// <summary>
/// Reads the Conversation table out of a Disco Elysium save and projects it into
/// a <see cref="UnifiedConversationState"/>, so both halves of the comparison are
/// the same type.
/// </summary>
/// <remarks>
/// <para>
/// The projection is lossless for this tool's purposes: the state drops
/// <see cref="SimStatus.Untouched"/> on merge, which is exactly what is wanted -
/// a real save holds ~113,000 entries of which only ~1,500 are above Untouched,
/// and the unified file never records Untouched either.
/// </para>
/// <para>
/// Save-side shape, confirmed against the de-omm.9 reference decode:
/// <code>
/// Conversation = { "&lt;convId&gt;": { Title = ..., Dialog = { "&lt;entryId&gt;": { SimStatus = "WasDisplayed" } } } }
/// </code>
/// </para>
/// </remarks>
public static class SaveConversationReader
{
    /// <summary>The table inside the save blob that carries dialogue statuses.</summary>
    public const string ConversationTableName = "Conversation";

    /// <summary>The per-conversation field holding the dialogue entry map.</summary>
    public const string DialogFieldName = "Dialog";

    /// <summary>The per-entry field holding the status string.</summary>
    public const string SimStatusFieldName = "SimStatus";

    /// <summary>
    /// Loads a save's dialogue statuses.
    /// </summary>
    /// <param name="spec">
    /// A path to a <c>*.ntwtf.zip</c>, a <c>*.ntwtf.lua</c>, or an expanded
    /// <c>*.ntwtf</c> folder; or a bare save name to resolve inside
    /// <paramref name="saveDirectory"/>.
    /// </param>
    /// <param name="saveDirectory">Directory bare names are resolved against.</param>
    /// <param name="resolvedPath">The save file actually read, for reporting.</param>
    public static UnifiedConversationState Load(
        string spec,
        string saveDirectory,
        out string resolvedPath
    )
    {
        resolvedPath = ResolveSave(spec, saveDirectory);
        byte[] blob = SaveBlob.Read(resolvedPath);
        LuaTable allTables = RawDataReader.ReadAllTables(blob, out _);

        if (
            !allTables.TryGetValue(ConversationTableName, out object? conversationValue)
            || conversationValue is not LuaTable conversations
        )
        {
            throw new InvalidDataException(
                $"'{resolvedPath}' has no '{ConversationTableName}' table."
            );
        }

        var state = new UnifiedConversationState();
        foreach ((object conversationKey, object? conversationRow) in conversations.Entries)
        {
            if (
                conversationRow is not LuaTable conversation
                || !TryParseId(conversationKey, out int conversationId)
            )
            {
                continue;
            }

            if (
                !conversation.TryGetValue(DialogFieldName, out object? dialogValue)
                || dialogValue is not LuaTable dialog
            )
            {
                continue;
            }

            foreach ((object entryKey, object? entryValue) in dialog.Entries)
            {
                if (entryValue is not LuaTable entry || !TryParseId(entryKey, out int entryId))
                {
                    continue;
                }

                if (
                    !entry.TryGetValue(SimStatusFieldName, out object? statusValue)
                    || statusValue is not string statusName
                )
                {
                    continue;
                }

                // Parse, not TryParse: an unknown status string means an assumption
                // about the save format is wrong, and silently dropping it would
                // quietly weaken the check.
                state.Merge(conversationId, entryId, SimStatusNames.Parse(statusName));
            }
        }

        return state;
    }

    /// <summary>Names of the saves in a directory, packed or expanded, without extensions.</summary>
    public static IEnumerable<string> ListSaveNames(string saveDirectory)
    {
        if (!Directory.Exists(saveDirectory))
        {
            return Array.Empty<string>();
        }

        return Directory
            .EnumerateFiles(saveDirectory, "*" + SaveBlob.ZipExtension)
            .Select(path => Path.GetFileName(path)[..^SaveBlob.ZipExtension.Length])
            .Concat(
                Directory
                    .EnumerateDirectories(saveDirectory, "*" + SaveBlob.ExpandedExtension)
                    .Select(path => Path.GetFileName(path)[..^SaveBlob.ExpandedExtension.Length])
            )
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .OrderBy(name => name, StringComparer.OrdinalIgnoreCase);
    }

    private static string ResolveSave(string spec, string saveDirectory)
    {
        if (File.Exists(spec) || Directory.Exists(spec))
        {
            return spec;
        }

        // A bare save name: prefer the packed archive, fall back to an expanded folder.
        string[] candidates = { spec + SaveBlob.ZipExtension, spec + SaveBlob.ExpandedExtension };
        foreach (string candidate in candidates)
        {
            string path = Path.Combine(saveDirectory, candidate);
            if (File.Exists(path) || Directory.Exists(path))
            {
                return path;
            }
        }

        throw new FileNotFoundException(
            $"No save matching '{spec}' as a path, or as '{spec}{SaveBlob.ZipExtension}' / "
                + $"'{spec}{SaveBlob.ExpandedExtension}' under '{saveDirectory}'.",
            spec
        );
    }

    private static bool TryParseId(object key, out int id) =>
        int.TryParse(
            LuaKey.ToKeyString(key),
            NumberStyles.Integer,
            CultureInfo.InvariantCulture,
            out id
        );
}
