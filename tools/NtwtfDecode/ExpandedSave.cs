// SPDX-License-Identifier: MIT
using System.IO.Compression;
using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

namespace NtwtfDecode;

/// <summary>Builds a game-ready archive from an expanded sparse save source.</summary>
public static class ExpandedSave
{
    public const string DiffManifestFileName = "_archive.json";

    private static readonly Regex TimestampPattern = new(
        @"\(\d{1,2}_\d{1,2}_\d{4} \d{1,2}-\d{2}-\d{2} (?:AM|PM)\)$",
        RegexOptions.CultureInvariant
    );

    /// <summary>Writes a complete expanded save from a packed input.</summary>
    public static void Write(
        string directory,
        PackedSave packed,
        LuaTable document,
        int? indent,
        bool sparse,
        string? baseline
    )
    {
        Directory.CreateDirectory(directory);
        string parts = Path.Combine(directory, packed.LuaName + ".parts");
        if (baseline is null)
        {
            foreach (PackedSaveEntry entry in packed.PassThrough)
            {
                File.WriteAllBytes(Path.Combine(directory, entry.Name), entry.Bytes);
            }
            LuaSplitFiles.Write(parts, document, indent, sparse);
        }
        else
        {
            WriteMemberDiffs(directory, packed, baseline, indent);
            LuaSplitFiles.WriteDiff(parts, document, baseline, indent);
        }
    }

    /// <summary>
    /// Reconstructs the Lua blob and packs it with the expanded save's pass-through files.
    /// </summary>
    public static string Pack(string source, string output, DateTime? now = null)
    {
        if (!Directory.Exists(source))
        {
            throw new DirectoryNotFoundException($"No expanded save directory at '{source}'.");
        }
        if (!source.EndsWith(SaveBlob.ExpandedExtension, StringComparison.OrdinalIgnoreCase))
        {
            throw new ArgumentException(
                $"An expanded save directory must end in '{SaveBlob.ExpandedExtension}': '{source}'.",
                nameof(source)
            );
        }
        if (!output.EndsWith(SaveBlob.ZipExtension, StringComparison.OrdinalIgnoreCase))
        {
            throw new ArgumentException(
                $"A packed save must end in '{SaveBlob.ZipExtension}': '{output}'.",
                nameof(output)
            );
        }

        string parts = LuaSplitFiles.ResolveDirectory(source);
        string partsName = Path.GetFileName(parts);
        const string PartsSuffix = ".parts";
        if (!partsName.EndsWith(SaveBlob.LuaExtension + PartsSuffix, StringComparison.Ordinal))
        {
            throw new InvalidDataException($"Split directory '{parts}' is not named for a Lua blob");
        }
        string luaName = partsName[..^PartsSuffix.Length];
        string archiveName = luaName[..^SaveBlob.LuaExtension.Length];
        string outputArchiveName = archiveName;
        if (!TimestampPattern.IsMatch(archiveName))
        {
            string timestamp = (now ?? DateTime.Now).ToString(
                "(M_d_yyyy h-mm-ss tt)",
                CultureInfo.InvariantCulture
            );
            outputArchiveName += timestamp;
            output = AppendTimestamp(output, timestamp);
        }
        string outputLuaName = outputArchiveName + SaveBlob.LuaExtension;
        LuaTable document = LuaSplitFiles.Read(parts);

        string? parent = Path.GetDirectoryName(output);
        if (!string.IsNullOrEmpty(parent))
        {
            Directory.CreateDirectory(parent);
        }
        using FileStream destination = File.Create(output);
        using var archive = new ZipArchive(destination, ZipArchiveMode.Create);
        ZipArchiveEntry lua = archive.CreateEntry(outputLuaName, CompressionLevel.Optimal);
        using (Stream stream = lua.Open())
        {
            LuaBinary.WriteDocument(stream, document);
        }

        string manifestPath = Path.Combine(source, DiffManifestFileName);
        if (File.Exists(manifestPath))
        {
            WriteDiffMembers(archive, source, manifestPath, archiveName, outputArchiveName);
            return output;
        }

        foreach (string file in Directory.GetFiles(source))
        {
            string name = Path.GetFileName(file);
            if (!name.StartsWith(archiveName + ".", StringComparison.Ordinal))
            {
                throw new InvalidDataException(
                    $"Expanded save member '{name}' does not match save name '{archiveName}'."
                );
            }
            string outputName = outputArchiveName + name[archiveName.Length..];
            archive.CreateEntryFromFile(file, outputName, CompressionLevel.Optimal);
        }
        return output;
    }

    private static void WriteMemberDiffs(
        string directory,
        PackedSave target,
        string baselinePath,
        int? indent
    )
    {
        string fullBaseline = Path.GetFullPath(baselinePath);
        SaveMembers baseline = ReadMembers(fullBaseline);
        string targetStem = StemOf(target.LuaName);
        var members = new JsonArray();
        foreach (PackedSaveEntry entry in target.PassThrough)
        {
            string suffix = SuffixOf(entry.Name, targetStem);
            baseline.BySuffix.TryGetValue(suffix, out PackedSaveEntry? oldEntry);
            byte[] oldBytes = oldEntry?.Bytes ?? Array.Empty<byte>();
            string kind;
            string? diffName = null;
            if (entry.Name.EndsWith(".json", StringComparison.OrdinalIgnoreCase))
            {
                JsonNode? oldJson = oldEntry is null ? null : ParseJson(oldBytes, oldEntry.Name);
                JsonNode? targetJson = ParseJson(entry.Bytes, entry.Name);
                JsonObject? patch = JsonDiff.Create(oldJson, targetJson);
                kind = patch is null ? "inherit" : "json";
                if (patch is not null)
                {
                    diffName = entry.Name;
                    JsonDiff.Write(Path.Combine(directory, diffName), patch, indent);
                }
            }
            else
            {
                string? patch = TextDiff.Create(entry.Name, oldBytes, entry.Bytes);
                kind = patch is null ? "inherit" : "text";
                if (patch is not null)
                {
                    diffName = entry.Name + ".diff";
                    File.WriteAllText(Path.Combine(directory, diffName), patch);
                }
            }
            members.Add(
                new JsonObject
                {
                    ["diff"] = diffName,
                    ["kind"] = kind,
                    ["name"] = entry.Name,
                    ["suffix"] = suffix,
                }
            );
        }
        var manifest = new JsonObject
        {
            [LuaJson.FormatName] = "expanded-save-diff",
            ["base"] = Path.GetRelativePath(directory, fullBaseline)
                .Replace(Path.DirectorySeparatorChar, '/'),
            ["members"] = members,
        };
        JsonDiff.Write(Path.Combine(directory, DiffManifestFileName), manifest, indent);
    }

    private static void WriteDiffMembers(
        ZipArchive archive,
        string source,
        string manifestPath,
        string archiveName,
        string outputArchiveName
    )
    {
        foreach (PackedSaveEntry entry in ApplyDiff(source, manifestPath, chain: null))
        {
            string outputName = outputArchiveName + entry.Name[archiveName.Length..];
            ZipArchiveEntry output = archive.CreateEntry(outputName, CompressionLevel.Optimal);
            using Stream stream = output.Open();
            stream.Write(entry.Bytes);
        }
    }

    /// <summary>
    /// Materialises the members a diff describes, resolving its base first.
    /// </summary>
    /// <remarks>
    /// The base may itself be a diff, which is what lets several saves that share a
    /// setup state it once: an intermediate diff names the shared changes, and each save
    /// beyond it carries only what makes it different. Resolution is therefore
    /// recursive, and <paramref name="chain"/> is what stops a base that eventually
    /// points back at itself from recursing forever.
    /// </remarks>
    private static List<PackedSaveEntry> ApplyDiff(
        string source,
        string manifestPath,
        HashSet<string>? chain
    )
    {
        JsonObject manifest = JsonNode.Parse(File.ReadAllText(manifestPath)) as JsonObject
            ?? throw new InvalidDataException($"'{manifestPath}' is not a JSON object");
        if (manifest[LuaJson.FormatName]?.GetValue<string>() != "expanded-save-diff"
            || manifest["base"]?.GetValue<string>() is not string relativeBase
            || manifest["members"] is not JsonArray members)
        {
            throw new InvalidDataException($"'{manifestPath}' is not an expanded save diff");
        }

        chain ??= new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        Descend(chain, source, manifestPath);

        string baselinePath = Path.GetFullPath(Path.Combine(source, relativeBase));
        SaveMembers baseline = ReadMembers(baselinePath, chain);
        var applied = new List<PackedSaveEntry>();
        foreach (JsonNode? node in members)
        {
            if (node is not JsonObject member
                || member["name"]?.GetValue<string>() is not string name
                || member["suffix"]?.GetValue<string>() is not string suffix
                || member["kind"]?.GetValue<string>() is not string kind)
            {
                throw new InvalidDataException($"'{manifestPath}' has a malformed member");
            }
            baseline.BySuffix.TryGetValue(suffix, out PackedSaveEntry? oldEntry);
            byte[] oldBytes = oldEntry?.Bytes ?? Array.Empty<byte>();
            byte[] bytes = kind switch
            {
                "inherit" when oldEntry is not null => oldBytes,
                "json" => ApplyJson(source, member, oldEntry),
                "text" => ApplyText(source, member, name, oldBytes),
                "inherit" => throw new InvalidDataException(
                    $"Diff member '{name}' has no matching baseline member"
                ),
                _ => throw new InvalidDataException($"Diff member '{name}' has unknown kind '{kind}'"),
            };
            applied.Add(new PackedSaveEntry(name, bytes));
        }
        return applied;
    }

    /// <summary>
    /// Records one step of a base chain, refusing one that returns to itself.
    /// </summary>
    /// <remarks>
    /// A cycle is the only way resolution could fail to terminate: every other chain
    /// ends at a complete save, because the filesystem is finite and each step moves to
    /// a different directory. So there is no depth limit - a long chain is unusual but
    /// not wrong, and a cap would only turn a working save into a refused one.
    /// </remarks>
    internal static void Descend(HashSet<string> chain, string source, string context)
    {
        if (!chain.Add(Path.GetFullPath(source)))
        {
            throw new InvalidDataException(
                $"'{context}' is part of a base chain that returns to '{source}'."
            );
        }
    }

    private static byte[] ApplyJson(string source, JsonObject member, PackedSaveEntry? baseline)
    {
        string diffPath = DiffPath(source, member);
        JsonObject patch = JsonNode.Parse(File.ReadAllText(diffPath)) as JsonObject
            ?? throw new InvalidDataException($"'{diffPath}' is not a JSON object");
        JsonNode? oldJson = baseline is null ? null : ParseJson(baseline.Bytes, baseline.Name);
        JsonNode? merged = JsonDiff.Apply(oldJson, patch, diffPath);
        return JsonSerializer.SerializeToUtf8Bytes(merged, new JsonSerializerOptions { WriteIndented = true });
    }

    private static byte[] ApplyText(
        string source,
        JsonObject member,
        string name,
        byte[] baseline
    ) => TextDiff.Apply(name, baseline, File.ReadAllText(DiffPath(source, member)));

    private static string DiffPath(string source, JsonObject member)
    {
        string diff = member["diff"]?.GetValue<string>()
            ?? throw new InvalidDataException("Changed diff member has no diff filename");
        return Path.Combine(source, diff);
    }

    private static SaveMembers ReadMembers(string path, HashSet<string>? chain = null)
    {
        if (SaveBlob.IsArchive(path))
        {
            PackedSave packed = SaveBlob.ReadArchive(path);
            return BuildMembers(packed.LuaName, packed.PassThrough, path);
        }
        string parts = LuaSplitFiles.ResolveDirectory(path);
        string luaName = Path.GetFileName(parts)[..^".parts".Length];

        // A base that is itself a diff has to be applied before it can be read as one,
        // or its diff files would be taken for the members they describe.
        string manifestPath = Path.Combine(path, DiffManifestFileName);
        if (File.Exists(manifestPath))
        {
            return BuildMembers(luaName, ApplyDiff(path, manifestPath, chain), path);
        }

        var entries = Directory.GetFiles(path)
            .Where(file => Path.GetFileName(file) != DiffManifestFileName)
            .Select(file => new PackedSaveEntry(Path.GetFileName(file), File.ReadAllBytes(file)))
            .ToList();
        return BuildMembers(luaName, entries, path);
    }

    private static SaveMembers BuildMembers(
        string luaName,
        IReadOnlyList<PackedSaveEntry> entries,
        string context
    )
    {
        string stem = StemOf(luaName);
        var bySuffix = new Dictionary<string, PackedSaveEntry>(StringComparer.Ordinal);
        foreach (PackedSaveEntry entry in entries)
        {
            string suffix = SuffixOf(entry.Name, stem);
            if (!bySuffix.TryAdd(suffix, entry))
            {
                throw new InvalidDataException($"'{context}' has duplicate save suffix '{suffix}'");
            }
        }
        return new SaveMembers(bySuffix);
    }

    private static JsonNode? ParseJson(byte[] bytes, string context)
    {
        try
        {
            return JsonNode.Parse(bytes);
        }
        catch (JsonException error)
        {
            throw new InvalidDataException($"Save member '{context}' is not valid JSON", error);
        }
    }

    private static string StemOf(string luaName) =>
        luaName.EndsWith(SaveBlob.LuaExtension, StringComparison.OrdinalIgnoreCase)
            ? luaName[..^SaveBlob.LuaExtension.Length]
            : throw new InvalidDataException($"Lua member '{luaName}' has an invalid name");

    private static string SuffixOf(string name, string stem) =>
        name.StartsWith(stem + ".", StringComparison.Ordinal)
            ? name[stem.Length..]
            : throw new InvalidDataException(
                $"Save member '{name}' does not match save name '{stem}'"
            );

    private static string AppendTimestamp(string output, string timestamp)
    {
        string stem = output[..^SaveBlob.ZipExtension.Length];
        return stem + timestamp + SaveBlob.ZipExtension;
    }

    private sealed record SaveMembers(Dictionary<string, PackedSaveEntry> BySuffix);
}
