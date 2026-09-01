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

    /// <summary>Writes only recursive sparse-JSON changes from a baseline.</summary>
    public static void WriteDiff(
        string directory,
        LuaTable root,
        string baseline,
        int? indent
    )
    {
        string baselinePath = Path.GetFullPath(baseline);
        string baseDirectory = ResolveDirectory(baselinePath);
        Directory.CreateDirectory(directory);
        Dictionary<string, SparseMap> target = EncodeSparse(root);
        foreach (string name in RawDataParser.TableNames)
        {
            SparseMap baseTree = ReadSparseTable(baseDirectory, name);
            SparseMap? patch = SparseDiff.Create(baseTree, target[name]);
            if (patch is null)
            {
                continue;
            }
            using FileStream stream = File.Create(TablePath(directory, name));
            SparseJson.Write(stream, patch, indent);
            stream.WriteByte((byte)'\n');
        }

        string baseTrailing = Path.Combine(baseDirectory, TrailingFileName);
        if (!root.TrailingBytes.SequenceEqual(File.ReadAllBytes(baseTrailing)))
        {
            File.WriteAllBytes(Path.Combine(directory, TrailingFileName), root.TrailingBytes);
        }

        var manifest = new SparseMap();
        manifest.Add(LuaJson.FormatName, SparseDiff.SetFormat);
        manifest.Add(
            "base",
            Path.GetRelativePath(directory, baselinePath)
                .Replace(Path.DirectorySeparatorChar, '/')
        );
        using FileStream manifestStream = File.Create(
            Path.Combine(directory, SparseDiff.ManifestFileName)
        );
        SparseJson.Write(manifestStream, manifest, indent);
        manifestStream.WriteByte((byte)'\n');
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

        string manifestPath = Path.Combine(directory, SparseDiff.ManifestFileName);
        if (File.Exists(manifestPath))
        {
            return ReadDiff(directory, manifestPath);
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

    private static LuaTable ReadDiff(string directory, string manifestPath)
    {
        using FileStream manifestStream = File.OpenRead(manifestPath);
        if (SparseJson.Read(manifestStream) is not SparseMap manifest
            || manifest.Find(LuaJson.FormatName) is not string format
            || format != SparseDiff.SetFormat
            || manifest.Find("base") is not string relativeBase)
        {
            throw new InvalidDataException($"'{manifestPath}' is not a sparse diff manifest");
        }
        string baseline = ResolveDirectory(
            Path.GetFullPath(Path.Combine(directory, relativeBase))
        );
        var trees = new Dictionary<string, object?>();
        foreach (string name in RawDataParser.TableNames)
        {
            SparseMap tree = ReadSparseTable(baseline, name);
            string patchPath = TablePath(directory, name);
            if (File.Exists(patchPath))
            {
                using FileStream stream = File.OpenRead(patchPath);
                tree = SparseJson.Read(stream) is SparseMap patch
                    ? SparseDiff.Apply(tree, patch, patchPath)
                    : throw new InvalidDataException($"'{patchPath}' is not a JSON object");
            }
            trees[name] = tree;
        }
        string trailing = Path.Combine(directory, TrailingFileName);
        byte[] trailingBytes = File.Exists(trailing)
            ? File.ReadAllBytes(trailing)
            : File.ReadAllBytes(Path.Combine(baseline, TrailingFileName));
        return DecodeSparse(trees, trailingBytes);
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

    private static SparseMap ReadSparseTable(string directory, string name)
    {
        string path = TablePath(directory, name);
        using FileStream stream = File.OpenRead(path);
        if (LuaJson.FormatOf(stream) != SparseFormat)
        {
            throw new InvalidDataException($"Diff baseline table '{path}' is not sparse");
        }
        return SparseJson.Read(stream) as SparseMap
            ?? throw new InvalidDataException($"Sparse table '{path}' is not a JSON object");
    }

    private static LuaTable TableOf(LuaTable root, string name) =>
        root.TryGetValue(name, out object? value) && value is LuaTable table
            ? table
            : throw new InvalidDataException($"Document is missing the '{name}' table");

    private static string TablePath(string directory, string tableName) =>
        Path.Combine(directory, tableName + ".json");
}
