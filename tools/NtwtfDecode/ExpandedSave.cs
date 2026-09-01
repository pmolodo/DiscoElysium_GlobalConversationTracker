// SPDX-License-Identifier: MIT
using System.IO.Compression;

namespace NtwtfDecode;

/// <summary>Builds a game-ready archive from an expanded sparse save source.</summary>
public static class ExpandedSave
{
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

        string expandedName = Path.GetFileName(source);
        string archiveName = expandedName[..^SaveBlob.ExpandedExtension.Length];
        string parts = Path.Combine(source, expandedName + ".lua.parts");
        LuaTable document = LuaSplitFiles.Read(parts);

        string? parent = Path.GetDirectoryName(output);
        if (!string.IsNullOrEmpty(parent))
        {
            Directory.CreateDirectory(parent);
        }
        using FileStream destination = File.Create(output);
        using var archive = new ZipArchive(destination, ZipArchiveMode.Create);
        ZipArchiveEntry lua = archive.CreateEntry(expandedName + ".lua", CompressionLevel.Optimal);
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
