// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Persistence;

namespace NtwtfDecode;

/// <summary>Reads and writes the six-file split representation of a save blob.</summary>
public static class LuaSplitFiles
{
    /// <summary>Name of the file containing bytes after the five tables.</summary>
    public const string TrailingFileName = "trailing.bin";

    /// <summary>Writes five table JSON files and one trailing-data binary file.</summary>
    public static void Write(string directory, LuaTable root, int? indent)
    {
        Directory.CreateDirectory(directory);
        foreach (string name in RawDataParser.TableNames)
        {
            if (!root.TryGetValue(name, out object? value) || value is not LuaTable table)
            {
                throw new InvalidDataException($"Document is missing the '{name}' table");
            }

            using FileStream stream = File.Create(TablePath(directory, name));
            LuaJson.Write(stream, table, indent);
            stream.WriteByte((byte)'\n');
        }
        File.WriteAllBytes(Path.Combine(directory, TrailingFileName), root.TrailingBytes);
    }

    /// <summary>Reads five table JSON files and one trailing-data binary file.</summary>
    public static LuaTable Read(string directory)
    {
        if (!Directory.Exists(directory))
        {
            throw new DirectoryNotFoundException($"No such split directory: '{directory}'");
        }

        var root = new LuaTable { IsDocumentRoot = true };
        foreach (string name in RawDataParser.TableNames)
        {
            string path = TablePath(directory, name);
            using FileStream stream = File.OpenRead(path);
            root.Add(name, LuaJson.ReadTable(stream));
        }
        root.TrailingBytes = File.ReadAllBytes(Path.Combine(directory, TrailingFileName));
        return root;
    }

    private static string TablePath(string directory, string tableName) =>
        Path.Combine(directory, tableName + ".json");
}
