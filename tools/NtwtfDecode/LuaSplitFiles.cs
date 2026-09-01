// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Persistence;

namespace NtwtfDecode;

/// <summary>Reads and writes the six-file split representation of a save blob.</summary>
public static class LuaSplitFiles
{
    /// <summary>Name of the file containing bytes after the five tables.</summary>
    public const string TrailingFileName = "trailing.bin";

    /// <summary>The layout-preserving representation, which rebuilds a save exactly.</summary>
    public const string DenseFormat = "dense";

    /// <summary>The smaller representation, which keeps the data but not the layout.</summary>
    public const string SparseFormat = "sparse";

    /// <summary>Writes five table JSON files and one trailing-data binary file.</summary>
    /// <param name="sparse">
    /// Whether to restructure the tables <see cref="LuaSparseManifest"/> names and
    /// leave out the layout detail the reader can choose for itself. Each file says
    /// which form it is in, so reading never has to guess.
    /// </param>
    public static void Write(string directory, LuaTable root, int? indent, bool sparse = false)
    {
        Directory.CreateDirectory(directory);
        Dictionary<string, SparseMap>? sparseTrees = sparse ? EncodeSparse(root) : null;
        foreach (string name in RawDataParser.TableNames)
        {
            LuaTable table = TableOf(root, name);

            using FileStream stream = File.Create(TablePath(directory, name));
            if (sparse)
            {
                SparseJson.Write(stream, sparseTrees![name], indent);
            }
            else
            {
                LuaJson.Write(stream, table, indent, name, DenseFormat);
            }
            stream.WriteByte((byte)'\n');
        }
        File.WriteAllBytes(Path.Combine(directory, TrailingFileName), root.TrailingBytes);
    }

    /// <summary>
    /// Writes only the tables that differ from a base document.
    /// </summary>
    /// <remarks>
    /// No manifest, and no directory at all when nothing differs. The base is named once,
    /// by the expanded save's own _archive.json; a second copy of it down here said the
    /// same thing by a different relative path, and for a diff that changes no table it
    /// was the only reason the directory existed.
    /// </remarks>
    /// <returns>Whether anything was written.</returns>
    public static bool WriteDiff(
        string directory,
        LuaTable root,
        LuaTable baseDocument,
        int? indent
    )
    {
        Dictionary<string, SparseMap> baseTrees = EncodeSparse(baseDocument);
        Dictionary<string, SparseMap> target = EncodeSparse(root);

        var patches = new Dictionary<string, SparseMap>();
        foreach (string name in RawDataParser.TableNames)
        {
            SparseMap? patch = SparseDiff.Create(baseTrees[name], target[name]);
            if (patch is not null)
            {
                patches[name] = patch;
            }
        }

        bool trailingDiffers = !root.TrailingBytes.SequenceEqual(baseDocument.TrailingBytes);
        if (patches.Count == 0 && !trailingDiffers)
        {
            return false;
        }

        Directory.CreateDirectory(directory);
        foreach (KeyValuePair<string, SparseMap> patch in patches)
        {
            using FileStream stream = File.Create(TablePath(directory, patch.Key));
            SparseJson.Write(stream, patch.Value, indent);
            stream.WriteByte((byte)'\n');
        }

        if (trailingDiffers)
        {
            File.WriteAllBytes(Path.Combine(directory, TrailingFileName), root.TrailingBytes);
        }

        return true;
    }

    /// <summary>
    /// Applies whatever table patches a diff carries onto its base document.
    /// </summary>
    /// <remarks>
    /// The directory is allowed not to exist. A save that changes only a pass-through
    /// member - money lives in the 2nd JSON, not in the Lua - has no table patches to
    /// carry, and should not have to carry an empty folder to say so.
    /// </remarks>
    /// <param name="directory">The split directory, which need not exist.</param>
    /// <param name="baseDocument">The document the patches apply to.</param>
    public static LuaTable ReadDiff(string? directory, LuaTable baseDocument)
    {
        Dictionary<string, SparseMap> baseTrees = EncodeSparse(baseDocument);
        var trees = new Dictionary<string, object?>();
        bool present = directory is not null && Directory.Exists(directory);

        foreach (string name in RawDataParser.TableNames)
        {
            SparseMap tree = baseTrees[name];
            string patchPath = present ? TablePath(directory!, name) : string.Empty;
            if (present && File.Exists(patchPath))
            {
                using FileStream stream = File.OpenRead(patchPath);
                tree = SparseJson.Read(stream) is SparseMap patch
                    ? SparseDiff.Apply(tree, patch, patchPath)
                    : throw new InvalidDataException($"'{patchPath}' is not a JSON object");
            }
            trees[name] = tree;
        }

        byte[] trailingBytes = baseDocument.TrailingBytes;
        if (present)
        {
            string trailing = Path.Combine(directory!, TrailingFileName);
            if (File.Exists(trailing))
            {
                trailingBytes = File.ReadAllBytes(trailing);
            }
        }

        return DecodeSparse(trees, trailingBytes);
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
        var dense = new Dictionary<string, LuaTable>();
        foreach (string name in RawDataParser.TableNames)
        {
            using FileStream stream = File.OpenRead(TablePath(directory, name));
            if (LuaJson.FormatOf(stream) == SparseFormat)
            {
                trees[name] = SparseJson.Read(stream);
            }
            else
            {
                // No marker means a file from before the two forms were told apart,
                // which is the dense one.
                dense[name] = LuaJson.ReadTable(stream, name);
            }
        }
        if (dense.Count == RawDataParser.TableNames.Length)
        {
            var denseRoot = new LuaTable { IsDocumentRoot = true };
            foreach (string name in RawDataParser.TableNames)
            {
                denseRoot.Add(name, dense[name]);
            }
            denseRoot.TrailingBytes = File.ReadAllBytes(
                Path.Combine(directory, TrailingFileName)
            );
            return denseRoot;
        }
        if (dense.Count > 0)
        {
            throw new InvalidDataException(
                $"'{directory}' mixes {DenseFormat} and {SparseFormat} table files: "
                    + string.Join(", ", dense.Keys) + " are dense and the rest are not"
            );
        }

        return DecodeSparse(trees, File.ReadAllBytes(Path.Combine(directory, TrailingFileName)));
    }

    /// <summary>Resolves either a six-file split folder or its expanded save parent.</summary>
    public static string ResolveDirectory(string path)
    {
        if (!Directory.Exists(path))
        {
            throw new DirectoryNotFoundException($"No such split directory: '{path}'");
        }
        if (File.Exists(TablePath(path, RawDataParser.TableNames[0])))
        {
            return path;
        }
        string parts = Path.Combine(path, Path.GetFileName(path) + ".lua.parts");
        if (Directory.Exists(parts))
        {
            return parts;
        }
        string[] candidates = Directory.GetDirectories(path, "*.ntwtf.lua.parts");
        if (candidates.Length == 1)
        {
            return candidates[0];
        }
        throw new InvalidDataException(
            $"'{path}' is neither a split directory nor an expanded save with exactly one "
                + $"*.ntwtf.lua.parts directory (found {candidates.Length})"
        );
    }

    private static LuaTable DecodeSparse(Dictionary<string, object?> trees, byte[] trailingBytes)
    {
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
        root.TrailingBytes = trailingBytes;
        return root;
    }

    private static Dictionary<string, SparseMap> EncodeSparse(LuaTable root)
    {
        LuaTable conversations = TableOf(root, LuaSparse.ConversationTableName);
        var trees = new Dictionary<string, SparseMap>();
        foreach (string name in RawDataParser.TableNames)
        {
            SparseMap map = LuaSparse.Encode(TableOf(root, name), name, conversations);
            map.Entries.Insert(
                0,
                new KeyValuePair<string, object?>(LuaJson.FormatName, SparseFormat)
            );
            trees[name] = map;
        }
        return trees;
    }

    private static LuaTable TableOf(LuaTable root, string name) =>
        root.TryGetValue(name, out object? value) && value is LuaTable table
            ? table
            : throw new InvalidDataException($"Document is missing the '{name}' table");

    private static string TablePath(string directory, string tableName) =>
        Path.Combine(directory, tableName + ".json");
}
