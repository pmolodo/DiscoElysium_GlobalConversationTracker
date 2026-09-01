// SPDX-License-Identifier: MIT
using System.Text.Json;
using System.Text.Json.Nodes;

namespace NtwtfDecode;

/// <summary>A recursive overlay for general JSON documents, including arrays and nulls.</summary>
public static class JsonDiff
{
    public const string Format = "json-diff";

    /// <summary>Creates a diff document, or null when the documents are equal.</summary>
    public static JsonObject? Create(JsonNode? baseline, JsonNode? target)
    {
        var removed = new JsonArray();
        JsonNode? changes = DiffNode(baseline, target, string.Empty, removed, out bool changed);
        if (!changed && removed.Count == 0)
        {
            return null;
        }
        return new JsonObject
        {
            [LuaJson.FormatName] = Format,
            ["_remove"] = removed,
            ["_changes"] = changes,
        };
    }

    /// <summary>Applies a diff document to a baseline.</summary>
    public static JsonNode? Apply(JsonNode? baseline, JsonObject patch, string context)
    {
        if (patch[LuaJson.FormatName]?.GetValue<string>() != Format
            || patch["_remove"] is not JsonArray removals
            || !patch.ContainsKey("_changes"))
        {
            throw new InvalidDataException($"{context} is not a {Format} JSON file");
        }
        var removed = new HashSet<string>(
            removals.Select(node => node?.GetValue<string>()
                ?? throw new InvalidDataException($"{context} has a null removal path")),
            StringComparer.Ordinal
        );
        return MergeNode(baseline, patch["_changes"], string.Empty, removed);
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
