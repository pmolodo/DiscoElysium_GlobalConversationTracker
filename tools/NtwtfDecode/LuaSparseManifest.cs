// SPDX-License-Identifier: MIT

namespace NtwtfDecode;

/// <summary>
/// A table whose children all hold one value under the same key, and so can be
/// written as one list of keys per distinct value instead of one object per key.
/// </summary>
/// <param name="Path">
/// The table's path pattern, matched the way <see cref="LuaKeyTypeManifest"/>
/// matches: an asterisk stands for one path segment.
/// </param>
/// <param name="ChildKey">The single key every child table holds.</param>
/// <param name="DefaultValue">
/// The value carried by the great majority of children, which the grouped form
/// leaves out entirely and the reader puts back.
/// </param>
public sealed record LuaValueGrouping(string Path, string ChildKey, object? DefaultValue);

/// <summary>Where the sparse form knows enough about the data to restructure it.</summary>
/// <remarks>
/// Every rule here is a claim about the shape of real save data, and every one is
/// checked before it is used: a table that does not match its rule is written
/// densely instead. So a rule that goes stale costs output size, never accuracy.
/// </remarks>
public static class LuaSparseManifest
{
    /// <summary>
    /// The dialogue-status map of a conversation. In the template save this is
    /// 112,940 tables of exactly <c>{"SimStatus": ...}</c>, 94% of them
    /// "Untouched" - about 6 MB of the 10.8 MB the dense form takes.
    /// </summary>
    public const string DialogPath = "Conversation/*/Dialog";

    /// <summary>The key every dialogue entry holds.</summary>
    public const string SimStatusKey = "SimStatus";

    /// <summary>The status an untouched dialogue entry carries.</summary>
    public const string UntouchedStatus = "Untouched";

    /// <summary>Tables written as one key list per distinct value.</summary>
    public static readonly IReadOnlyList<LuaValueGrouping> Groupings =
        new[] { new LuaValueGrouping(DialogPath, SimStatusKey, UntouchedStatus) };

    /// <summary>The grouping rule for a table path, or null when there is none.</summary>
    public static LuaValueGrouping? GroupingFor(string path)
    {
        foreach (LuaValueGrouping grouping in Groupings)
        {
            if (LuaKeyTypeManifest.Matches(grouping.Path, path))
            {
                return grouping;
            }
        }
        return null;
    }
}
