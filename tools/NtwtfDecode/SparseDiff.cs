// SPDX-License-Identifier: MIT

namespace NtwtfDecode;

/// <summary>A recursive JSON-object overlay against a sparse split baseline.</summary>
public static class SparseDiff
{
    public const string DiffFormat = "sparse-diff";
    public const string SetFormat = "sparse-diff-set";
    public const string ManifestFileName = "_base.json";

    private const string ChangesName = "_changes";
    private const string RemoveName = "_remove";

    /// <summary>Creates a sparse patch, or null when the two trees are equal.</summary>
    public static SparseMap? Create(SparseMap baseline, SparseMap target)
    {
        var removed = new List<string>();
        SparseMap changes = DiffMap(baseline, target, string.Empty, removed);
        if (removed.Count == 0 && changes.Entries.Count == 0)
        {
            return null;
        }

        var patch = new SparseMap();
        patch.Add(LuaJson.FormatName, DiffFormat);

        // Left out when empty, like the JSON form: a table diff that only changes a
        // value should read as that value.
        if (removed.Count > 0)
        {
            var removalMap = new SparseMap();
            foreach (string path in removed)
            {
                removalMap.Add(path, true);
            }
            patch.Add(RemoveName, removalMap);
        }
        if (changes.Entries.Count > 0)
        {
            patch.Add(ChangesName, changes);
        }
        return patch;
    }

    /// <summary>Recursively merges a patch onto its baseline.</summary>
    public static SparseMap Apply(SparseMap baseline, SparseMap patch, string context)
    {
        if (patch.Find(LuaJson.FormatName) is not string format || format != DiffFormat)
        {
            throw new InvalidDataException($"{context} is not a {DiffFormat} JSON file");
        }
        // Absent means empty for both, so a patch states only what it does.
        SparseMap removals = OptionalMap(patch, RemoveName, context);
        SparseMap changes = OptionalMap(patch, ChangesName, context);
        var removed = new HashSet<string>(
            removals.Entries.Select(entry => entry.Key),
            StringComparer.Ordinal
        );
        return MergeMap(baseline, changes, string.Empty, removed);
    }

    private static SparseMap DiffMap(
        SparseMap baseline,
        SparseMap target,
        string path,
        List<string> removed
    )
    {
        var changes = new SparseMap();
        foreach (KeyValuePair<string, object?> entry in baseline.Entries)
        {
            if (!TryFind(target, entry.Key, out _))
            {
                removed.Add(ChildPath(path, entry.Key));
            }
        }
        foreach (KeyValuePair<string, object?> entry in target.Entries)
        {
            if (!TryFind(baseline, entry.Key, out object? oldValue))
            {
                changes.Add(entry.Key, entry.Value);
            }
            else if (oldValue is SparseMap oldMap && entry.Value is SparseMap newMap)
            {
                int removalsBefore = removed.Count;
                SparseMap child = DiffMap(oldMap, newMap, ChildPath(path, entry.Key), removed);
                if (child.Entries.Count > 0 || removed.Count > removalsBefore)
                {
                    changes.Add(entry.Key, child);
                }
            }
            else if (!Equals(oldValue, entry.Value))
            {
                changes.Add(entry.Key, entry.Value);
            }
        }
        return changes;
    }

    private static SparseMap MergeMap(
        SparseMap baseline,
        SparseMap changes,
        string path,
        HashSet<string> removed
    )
    {
        var merged = new SparseMap();
        foreach (KeyValuePair<string, object?> entry in baseline.Entries)
        {
            string childPath = ChildPath(path, entry.Key);
            if (removed.Contains(childPath))
            {
                continue;
            }
            if (TryFind(changes, entry.Key, out object? changed))
            {
                merged.Add(
                    entry.Key,
                    entry.Value is SparseMap oldMap && changed is SparseMap changeMap
                        ? MergeMap(oldMap, changeMap, childPath, removed)
                        : changed
                );
            }
            else
            {
                merged.Add(entry.Key, entry.Value);
            }
        }
        foreach (KeyValuePair<string, object?> entry in changes.Entries)
        {
            if (!TryFind(baseline, entry.Key, out _))
            {
                merged.Add(entry.Key, entry.Value);
            }
        }
        return merged;
    }

    /// <summary>A member map, or an empty one when the member is absent.</summary>
    private static SparseMap OptionalMap(SparseMap patch, string name, string context) =>
        patch.Find(name) is null ? new SparseMap() : RequireMap(patch, name, context);

    private static SparseMap RequireMap(SparseMap parent, string name, string context) =>
        parent.Find(name) is SparseMap map
            ? map
            : throw new InvalidDataException($"{context} has no object '{name}'");

    private static bool TryFind(SparseMap map, string name, out object? value)
    {
        foreach (KeyValuePair<string, object?> entry in map.Entries)
        {
            if (entry.Key == name)
            {
                value = entry.Value;
                return true;
            }
        }
        value = null;
        return false;
    }

    private static string ChildPath(string parent, string name) =>
        parent + "/" + name.Replace("~", "~0").Replace("/", "~1");
}
