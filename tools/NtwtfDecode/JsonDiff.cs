// SPDX-License-Identifier: MIT
using System.Text.Json;
using System.Text.Json.Nodes;

using GlobalConversationTracker.Core;

namespace NtwtfDecode;

/// <summary>A recursive overlay for general JSON documents, including arrays and nulls.</summary>
public static class JsonDiff
{
    public const string Format = "json-diff";

    /// <summary>The version of it this build writes.</summary>
    public const int FormatVersion = 1;

    /// <summary>Creates a diff document, or null when the documents are equal.</summary>
    public static JsonObject? Create(JsonNode? baseline, JsonNode? target)
    {
        var removed = new JsonArray();
        JsonNode? changes = DiffNode(baseline, target, string.Empty, removed, out bool changed);
        if (!changed && removed.Count == 0)
        {
            return null;
        }
        // Both members are left out when they have nothing to say. A diff that changes
        // one field should read as that one field; "_remove": [] on every file is noise
        // that a reader has to look past to find the change.
        var patch = new JsonObject
        {
            [FormatStamp.FormatPropertyName] = Format,
            [FormatStamp.VersionPropertyName] = FormatVersion,
        };
        if (removed.Count > 0)
        {
            patch["_remove"] = removed;
        }
        if (changed)
        {
            patch["_changes"] = changes;
        }
        return patch;
    }

    /// <summary>Applies a diff document to a baseline.</summary>
    public static JsonNode? Apply(JsonNode? baseline, JsonObject patch, string context)
    {
        // THE WHOLE HEADER, through the one piece of code that reads one: what the file
        // says it is as well as which version of that, because a document of the wrong
        // KIND read as this one parses and quietly yields whatever happened to line up.
        FormatStamp.EnsureHeader(
            Format,
            patch[FormatStamp.FormatPropertyName]?.GetValue<string>(),
            patch[FormatStamp.VersionPropertyName]?.GetValue<int>() ?? FormatStamp.Unstamped,
            FormatVersion,
            context);

        // Absent means empty, for both. Only the format marker is required, so a diff
        // that removes nothing and a diff that changes nothing each say only what they
        // do rather than carrying an empty half.
        JsonNode? removeNode = patch["_remove"];
        if (removeNode is not null && removeNode is not JsonArray)
        {
            throw new InvalidDataException($"{context} has a '_remove' that is not an array");
        }
        var removals = removeNode as JsonArray ?? new JsonArray();

        var removed = new HashSet<string>(
            removals.Select(node => node?.GetValue<string>()
                ?? throw new InvalidDataException($"{context} has a null removal path")),
            StringComparer.Ordinal
        );
        // Absent and present-but-null are different: a missing '_changes' changes
        // nothing, while an explicit null is a document that became null. Only
        // ContainsKey tells them apart, since the indexer answers null for both.
        JsonNode? changes = patch.ContainsKey("_changes") ? patch["_changes"] : new JsonObject();
        return MergeNode(baseline, changes, string.Empty, removed);
    }

    /// <summary>Writes JSON using the repository's indented UTF-8 style.</summary>
    public static void Write(string path, JsonNode? document, int? indent)
    {
        var options = new JsonSerializerOptions { WriteIndented = indent is not null };
        File.WriteAllText(path, document?.ToJsonString(options) + "\n");
    }

    private static JsonNode? DiffNode(
        JsonNode? baseline,
        JsonNode? target,
        string path,
        JsonArray removed,
        out bool changed
    )
    {
        if (baseline is JsonObject oldObject && target is JsonObject newObject)
        {
            var changes = new JsonObject();
            foreach ((string name, _) in oldObject)
            {
                if (!newObject.ContainsKey(name))
                {
                    removed.Add(ChildPath(path, name));
                }
            }
            foreach ((string name, JsonNode? value) in newObject)
            {
                if (!oldObject.TryGetPropertyValue(name, out JsonNode? oldValue))
                {
                    changes[name] = value?.DeepClone();
                    continue;
                }
                int removalsBefore = removed.Count;
                JsonNode? child = DiffNode(
                    oldValue,
                    value,
                    ChildPath(path, name),
                    removed,
                    out bool childChanged
                );
                if (childChanged || removed.Count > removalsBefore)
                {
                    changes[name] = child;
                }
            }
            changed = changes.Count > 0;
            return changes;
        }

        changed = !JsonNode.DeepEquals(baseline, target);
        return changed ? target?.DeepClone() : null;
    }

    private static JsonNode? MergeNode(
        JsonNode? baseline,
        JsonNode? changes,
        string path,
        HashSet<string> removed
    )
    {
        if (baseline is not JsonObject oldObject || changes is not JsonObject changeObject)
        {
            return changes?.DeepClone();
        }
        var merged = new JsonObject();
        foreach ((string name, JsonNode? value) in oldObject)
        {
            string childPath = ChildPath(path, name);
            if (removed.Contains(childPath))
            {
                continue;
            }
            merged[name] = changeObject.TryGetPropertyValue(name, out JsonNode? changed)
                ? MergeNode(value, changed, childPath, removed)
                : value?.DeepClone();
        }
        foreach ((string name, JsonNode? value) in changeObject)
        {
            if (!oldObject.ContainsKey(name))
            {
                merged[name] = value?.DeepClone();
            }
        }
        return merged;
    }

    private static string ChildPath(string parent, string name) =>
        parent + "/" + name.Replace("~", "~0").Replace("/", "~1");
}
