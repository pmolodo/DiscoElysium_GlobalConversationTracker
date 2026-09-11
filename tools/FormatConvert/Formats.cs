// SPDX-License-Identifier: MIT
using System.Text.Json;
using GlobalConversationTracker.Core;
using GlobalConversationTracker.Persistence;
using NtwtfDecode;

namespace FormatConvert;

/// <summary>
/// Which format a file is in, which version of it, and what this build writes.
/// </summary>
/// <remarks>
/// <para>WORKED OUT FROM THE FILE, never from the caller. Both facts are already in every
/// file this repository writes - that is the whole point of the <c>_format</c>
/// discriminator and of the version stamps de-8hh2.11.2 added - so asking a caller to
/// repeat them would be asking them to be able to get it wrong.</para>
///
/// <para>TWO PLACES A VERSION CAN LIVE, and it is not tidiness that they differ. The
/// Lua-side formats carry <c>_format</c> naming their representation, so their version sits
/// beside it as <see cref="FormatStamp.VersionPropertyName"/>. The global state file is not a Lua
/// table and has carried a plain <c>version</c> at its root through four versions of real
/// player history; renaming that would be a format break bought with nothing. See
/// <see cref="FormatStamp"/>, which says the same thing from the reading end.</para>
/// </remarks>
public sealed record Detected(string Name, int Version, int Current)
{
    /// <summary>Whether the file is already what this build writes.</summary>
    public bool IsCurrent => Version == Current;

    /// <summary>Whether the file was written by a build newer than this one.</summary>
    public bool IsFromTheFuture => Version > Current;
}

/// <summary>What this build knows how to recognise, and how to convert it.</summary>
public static class Formats
{

    /// <summary>What a file that records no version is taken to be.</summary>
    /// <remarks>
    /// THE ONE PLACE THIS MAY BE ASSUMED, and the reason is what this tool is for: every
    /// format was stamped without changing its shape, so a file written before the stamp
    /// existed IS version 1 - and bringing such a file forward is precisely the job here.
    /// Every reader outside this tool refuses an unstamped file instead; see
    /// FormatStamp.EnsureStamped.
    /// </remarks>
    private const int Unstamped = 1;
    /// <summary>What the global state file is called, for messages.</summary>
    public const string GlobalState = "global-conversation-state";

    /// <summary>The root property the global state file carries its version in.</summary>
    private const string GlobalStateVersion = "version";

    /// <summary>A property only the global state file has, so it can be told apart.</summary>
    private const string GlobalStateConversations = "conversations";

    /// <summary>
    /// What version this build writes, for each Lua-side format that names itself.
    /// </summary>
    /// <remarks>
    /// KEYED BY WHAT THE FILE SAYS IT IS. A format missing from here is one this build has
    /// never heard of, which is a different thing from one it cannot convert - and the two
    /// want different messages, so they are not collapsed into a lookup failure.
    ///
    /// EVERY ONE OF THESE IS AT VERSION 1 TODAY, so nothing here has an older shape to
    /// convert yet. That is not a reason to leave the table out: the point of de-bnjy.3 is
    /// that the readers refuse anything but the current version, and a refusal without a
    /// converter that recognises the file is a wall.
    /// </remarks>
    private static readonly IReadOnlyDictionary<string, int> LuaSideVersions =
        new Dictionary<string, int>(StringComparer.Ordinal)
        {
            [SparseDiff.DiffFormat] = SparseDiff.FormatVersion,
            [JsonDiff.Format] = JsonDiff.FormatVersion,
            [ExpandedSave.DiffFormat] = ExpandedSave.FormatVersion,
            [LuaSplitFiles.DenseFormat] = LuaSplitFiles.FormatVersion,
            [LuaSplitFiles.SparseFormat] = LuaSplitFiles.FormatVersion,
        };

    /// <summary>Every format name this build recognises, for a message that lists them.</summary>
    public static IEnumerable<string> Known =>
        LuaSideVersions.Keys.Append(GlobalState).OrderBy(name => name, StringComparer.Ordinal);

    /// <summary>Works out what a file is, from the file.</summary>
    /// <param name="utf8Json">The file's bytes.</param>
    /// <param name="path">Where it came from, for the message.</param>
    /// <returns>What it is.</returns>
    /// <exception cref="InvalidDataException">
    /// It is not JSON, or it is JSON that names no format this build knows.
    /// </exception>
    public static Detected Detect(byte[] utf8Json, string path)
    {
        JsonDocument document;
        try
        {
            document = JsonDocument.Parse(utf8Json);
        }
        catch (JsonException malformed)
        {
            throw new InvalidDataException($"'{path}' is not valid JSON: {malformed.Message}");
        }

        using (document)
        {
            JsonElement root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object)
            {
                throw new InvalidDataException(
                    $"'{path}' has a {root.ValueKind} at its root, and every format this "
                    + "converts is a JSON object.");
            }

            // THE GLOBAL STATE AS IT WAS BEFORE IT NAMED ITSELF, and on TWO properties
            // rather than one: a bare version is a plausible thing for some other file to
            // carry, so requiring the conversations beside it is what stops this claiming a
            // file it cannot read. From version 5 the file says what it is like everything
            // else does, and is recognised by the header branch below.
            if (root.TryGetProperty(GlobalStateVersion, out JsonElement version)
                && root.TryGetProperty(GlobalStateConversations, out _))
            {
                if (version.ValueKind != JsonValueKind.Number
                    || !version.TryGetInt32(out int found))
                {
                    throw new InvalidDataException(
                        $"'{path}' looks like a {GlobalState} file, but its "
                        + $"'{GlobalStateVersion}' is not an integer.");
                }

                return new Detected(GlobalState, found, GlobalStateJson.FormatVersion);
            }

            if (root.TryGetProperty(FormatStamp.FormatPropertyName, out JsonElement format)
                && format.ValueKind == JsonValueKind.String)
            {
                string name = format.GetString() ?? string.Empty;
                if (name == GlobalStateJson.FormatName)
                {
                    return new Detected(
                        GlobalState, StampOf(root), GlobalStateJson.FormatVersion);
                }

                if (!LuaSideVersions.TryGetValue(name, out int current))
                {
                    throw new InvalidDataException(
                        $"'{path}' says it is '{name}', which this build does not know. "
                        + "The formats it knows are: "
                        + string.Join(", ", Known)
                        + ".");
                }

                return new Detected(name, StampOf(root), current);
            }
        }

        throw new InvalidDataException(
            $"'{path}' names no format this converter knows. A file it can read carries "
            + $"either a '{FormatStamp.FormatPropertyName}' naming its representation, or a "
            + $"'{GlobalStateVersion}' beside a '{GlobalStateConversations}'. The formats "
            + "it knows are: "
            + string.Join(", ", Known)
            + ".");
    }

    /// <summary>The current version of `what`, as bytes.</summary>
    /// <param name="what">What the file is, per <see cref="Detect"/>.</param>
    /// <param name="utf8Json">The file's bytes.</param>
    /// <param name="path">Where it came from, for the message.</param>
    /// <returns>The converted file.</returns>
    /// <exception cref="InvalidDataException">
    /// This build has no conversion for that format from that version. Thrown rather than
    /// half-written: a converter that produces a partial file is worse than one that
    /// refuses, because the partial file looks like a result.
    /// </exception>
    public static byte[] ToCurrent(Detected what, byte[] utf8Json, string path)
    {
        if (what.Name == GlobalState)
        {
            return LegacyGlobalState.ConvertToUtf8Bytes(utf8Json, path);
        }

        // NO LUA-SIDE FORMAT HAS EVER HAD AN OLDER SHAPE - every one of them is at version
        // 1 - so reaching here means a file claims a version below 1, which is not a
        // version this repository ever wrote.
        throw new InvalidDataException(
            $"'{path}' is {what.Name} version {what.Version}, and this build knows no "
            + $"conversion from it. Every version of {what.Name} this repository has "
            + $"written is version {what.Current}.");
    }

    /// <summary>The version a document records, or what a document without one is.</summary>
    /// <remarks>
    /// ABSENT MEANS VERSION 1, everywhere, because every format was stamped while its shape
    /// was unchanged - so a file written before the stamp is version 1 in fact rather than
    /// by convention. This tool is the only thing that may assume it; see Unstamped.
    /// </remarks>
    private static int StampOf(JsonElement root) =>
        root.TryGetProperty(FormatStamp.VersionPropertyName, out JsonElement stamped)
            && stamped.ValueKind == JsonValueKind.Number
            && stamped.TryGetInt32(out int read)
                ? read
                : Unstamped;
}
