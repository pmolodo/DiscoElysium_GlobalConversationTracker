// SPDX-License-Identifier: MIT
using System.IO.Compression;

namespace NtwtfDecode;

/// <summary>Builds a game-ready archive from an expanded sparse save source.</summary>
public static class ExpandedSave
{
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
        foreach (PackedSaveEntry entry in packed.PassThrough)
        {
            File.WriteAllBytes(Path.Combine(directory, entry.Name), entry.Bytes);
        }
        string parts = Path.Combine(directory, packed.LuaName + ".parts");
        if (baseline is null)
        {
            LuaSplitFiles.Write(parts, document, indent, sparse);
        }
        else
        {
            LuaSplitFiles.WriteDiff(parts, document, baseline, indent);
        }
    }

    /// <summary>
    /// Reconstructs the Lua blob and packs it with the expanded save's pass-through files.
    /// </summary>
    public static void Pack(string source, string output)
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
        LuaTable document = LuaSplitFiles.Read(parts);

        string? parent = Path.GetDirectoryName(output);
        if (!string.IsNullOrEmpty(parent))
        {
            Directory.CreateDirectory(parent);
        }
        using FileStream destination = File.Create(output);
        using var archive = new ZipArchive(destination, ZipArchiveMode.Create);
        ZipArchiveEntry lua = archive.CreateEntry(luaName, CompressionLevel.Optimal);
        using (Stream stream = lua.Open())
        {
            LuaBinary.WriteDocument(stream, document);
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
            archive.CreateEntryFromFile(file, name, CompressionLevel.Optimal);
        }
    }
}
