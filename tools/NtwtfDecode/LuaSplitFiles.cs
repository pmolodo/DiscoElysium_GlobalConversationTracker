// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Persistence;

namespace NtwtfDecode;

/// <summary>Reads and writes the six-file split representation of a save blob.</summary>
public static class LuaSplitFiles
{
    /// <summary>Name of the file containing bytes after the five tables.</summary>
    public const string TrailingFileName = "trailing.bin";

    /// <summary>Writes five table JSON files and one trailing-data binary file.</summary>
    /// <param name="sparse">
    /// Whether to restructure the tables <see cref="LuaSparseManifest"/> names.
    /// The result still converts back byte for byte; it just no longer mirrors
    /// the blob's layout entry for entry.
    /// </param>
    public static void Write(string directory, LuaTable root, int? indent, bool sparse = false)
    {
        Directory.CreateDirectory(directory);
        LuaTable? conversations = sparse ? TableOf(root, LuaSparse.ConversationTableName) : null;
        foreach (string name in RawDataParser.TableNames)
        {
            LuaTable table = TableOf(root, name);

            using FileStream stream = File.Create(TablePath(directory, name));
            if (sparse)
            {
                SparseJson.Write(stream, LuaSparse.Encode(table, name, conversations), indent);
            }
            else
            {
                LuaJson.Write(stream, table, indent, name);
            }
            stream.WriteByte((byte)'\n');
        }
        File.WriteAllBytes(Path.Combine(directory, TrailingFileName), root.TrailingBytes);
    }

    /// <summary>Reads five table JSON files and one trailing-data binary file.</summary>
    /// <remarks>
    /// Whether a file is sparse is read off the file itself, not asked for: the
    /// two forms are distinguishable, and a caller that had to remember which one
    /// it wrote would be one wrong flag away from a corrupted save.
    /// </remarks>
    public static LuaTable Read(string directory)
    {
        if (!Directory.Exists(directory))
        {
            throw new DirectoryNotFoundException($"No such split directory: '{directory}'");
        }

        var trees = new Dictionary<string, object?>();
        foreach (string name in RawDataParser.TableNames)
        {
            using FileStream stream = File.OpenRead(TablePath(directory, name));
            trees[name] = SparseJson.Read(stream);
        }

        // Conversation first: the Variable table may have left out the variables
        // that only repeat it, and needs it to put them back.
        var decoded = new Dictionary<string, LuaTable>
        {
            [LuaSparse.ConversationTableName] = LuaSparse.Decode(
                trees[LuaSparse.ConversationTableName],
                LuaSparse.ConversationTableName
            ),
        };
        var root = new LuaTable { IsDocumentRoot = true };
        foreach (string name in RawDataParser.TableNames)
        {
            if (!decoded.TryGetValue(name, out LuaTable? table))
            {
                table = LuaSparse.Decode(
                    trees[name],
                    name,
                    decoded[LuaSparse.ConversationTableName]
                );
            }
            root.Add(name, table);
        }
        root.TrailingBytes = File.ReadAllBytes(Path.Combine(directory, TrailingFileName));
        return root;
    }

    private static LuaTable TableOf(LuaTable root, string name) =>
        root.TryGetValue(name, out object? value) && value is LuaTable table
            ? table
            : throw new InvalidDataException($"Document is missing the '{name}' table");

    private static string TablePath(string directory, string tableName) =>
        Path.Combine(directory, tableName + ".json");
}
