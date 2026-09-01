// SPDX-License-Identifier: MIT
using System.Text.Json;

namespace NtwtfDecode;

/// <summary>
/// Which dialogue entries belong to a conversation, for rebuilding its
/// <c>Conversation_SimX_*</c> variable.
/// </summary>
/// <remarks>
/// <para>
/// The statuses in those strings are the same data the Conversation table's Dialog
/// maps hold, so the sparse form leaves the strings out and rebuilds them. All it
/// needs to do that is the set of dialogue entries per conversation, which
/// <c>articy_ids_final_cut.json</c> already gives.
/// </para>
/// <para>
/// The order those entries come in is not recorded and not reproduced. A save
/// writes them in the game's own Lua table order, which is not even stable between
/// saves of one playthrough, and it carries no information: the string is an id to
/// status map, and a repeated id always carries the same status. So rebuilding
/// picks one fixed order, and encoding accepts a string whose pairs match
/// regardless of the order they were in.
/// </para>
/// </remarks>
public sealed class SimXOrders
{
    /// <summary>The id map this repo carries.</summary>
    public const string ArticyIdsFileName = "articy_ids_final_cut.json";

    private readonly Dictionary<int, List<(string ArticyId, int DialogueIndex)>> _sequences;
    private readonly Dictionary<string, int> _conversationIndex;

    private SimXOrders(
        Dictionary<int, List<(string, int)>> sequences,
        Dictionary<string, int> conversationIndex
    )
    {
        _sequences = sequences;
        _conversationIndex = conversationIndex;
    }

    /// <summary>
    /// Loads the id map, or returns null when it is not beside the repository.
    /// Sparse conversion then leaves the SimX strings alone rather than failing,
    /// which keeps the tool usable outside a checkout.
    /// </summary>
    public static SimXOrders? TryLoad()
    {
        string? articyPath = FindRepositoryFile(ArticyIdsFileName);
        if (articyPath is null)
        {
            return null;
        }

        using FileStream articyStream = File.OpenRead(articyPath);
        using JsonDocument articy = JsonDocument.Parse(articyStream);
        var conversationIndex = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (
            JsonProperty entry in articy.RootElement.GetProperty("conversations").EnumerateObject()
        )
        {
            conversationIndex[entry.Name] = entry.Value.GetInt32();
        }

        var sequences = new Dictionary<int, List<(string, int)>>();
        foreach (
            JsonProperty entry in articy
                .RootElement.GetProperty("dialogue_entries")
                .EnumerateObject()
        )
        {
            int conversation = entry.Value[0].GetInt32();
            if (!sequences.TryGetValue(conversation, out List<(string, int)>? pairs))
            {
                pairs = new List<(string, int)>();
                sequences[conversation] = pairs;
            }
            foreach (JsonElement index in entry.Value[1].EnumerateArray())
            {
                pairs.Add((entry.Name, index.GetInt32()));
            }
        }
        foreach (List<(string ArticyId, int DialogueIndex)> pairs in sequences.Values)
        {
            // The one order every rebuild uses. Any total order would do; this one
            // reads sensibly, following the dialogue entries as they are numbered.
            pairs.Sort(
                (a, b) =>
                    a.DialogueIndex != b.DialogueIndex
                        ? a.DialogueIndex.CompareTo(b.DialogueIndex)
                        : string.CompareOrdinal(a.ArticyId, b.ArticyId)
            );
        }
        return new SimXOrders(sequences, conversationIndex);
    }

    /// <summary>The conversation index an articy id names, or null when unknown.</summary>
    public int? ConversationIndex(string articyId) =>
        _conversationIndex.TryGetValue(articyId, out int index) ? index : null;

    /// <summary>
    /// The dialogue entries of a conversation, in the order a rebuilt SimX string
    /// lists them, or null when the conversation is not in the map.
    /// </summary>
    public IReadOnlyList<(string ArticyId, int DialogueIndex)>? SequenceFor(int conversation) =>
        _sequences.TryGetValue(conversation, out List<(string, int)>? pairs) ? pairs : null;

    /// <summary>
    /// Looks for a repository file beside the running assembly and beside the
    /// working directory, walking up from each.
    /// </summary>
    private static string? FindRepositoryFile(string name)
    {
        foreach (string start in new[] { AppContext.BaseDirectory, Directory.GetCurrentDirectory() })
        {
            for (
                DirectoryInfo? directory = new(start);
                directory is not null;
                directory = directory.Parent
            )
            {
                string candidate = Path.Combine(directory.FullName, name);
                if (File.Exists(candidate))
                {
                    return candidate;
                }
            }
        }
        return null;
    }
}
