// SPDX-License-Identifier: MIT

namespace NtwtfDecode;

/// <summary>The wire type shared by every dictionary key in one Lua table.</summary>
public enum LuaDictionaryKeyType
{
    /// <summary>UTF-8 string keys.</summary>
    String,

    /// <summary>IEEE-754 double keys.</summary>
    Number,

    /// <summary>Boolean keys.</summary>
    Boolean,
}

/// <summary>Known table paths whose dictionary keys are not strings.</summary>
public static class LuaKeyTypeManifest
{
    /// <summary>
    /// Path patterns and their exceptional key types. An asterisk matches one
    /// table-path segment. Every path not listed here has string keys.
    /// </summary>
    public static readonly IReadOnlyList<KeyValuePair<string, LuaDictionaryKeyType>> Entries =
        new[]
        {
            new KeyValuePair<string, LuaDictionaryKeyType>(
                "Conversation/*/Dialog",
                LuaDictionaryKeyType.Number
            ),
        };

    /// <summary>Returns the expected dictionary-key type for a table path.</summary>
    public static LuaDictionaryKeyType ExpectedType(string path)
    {
        foreach (KeyValuePair<string, LuaDictionaryKeyType> entry in Entries)
        {
            if (Matches(entry.Key, path))
            {
                return entry.Value;
            }
        }
        return LuaDictionaryKeyType.String;
    }

    private static bool Matches(string pattern, string path)
    {
        string[] expected = pattern.Split('/');
        string[] actual = path.Split('/');
        if (expected.Length != actual.Length)
        {
            return false;
        }
        for (int i = 0; i < expected.Length; i++)
        {
            if (expected[i] != "*" && expected[i] != actual[i])
            {
                return false;
            }
        }
        return true;
    }
}
