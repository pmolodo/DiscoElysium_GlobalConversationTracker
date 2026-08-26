// SPDX-License-Identifier: MIT
using System.IO.Compression;

namespace NtwtfDecode;

/// <summary>
/// Resolves a command-line input to the raw bytes of a {save}.ntwtf.lua blob.
/// </summary>
/// <remarks>
/// <para>
/// Three input shapes are accepted, all of which occur in practice: a packed
/// <c>*.ntwtf.zip</c> save - which is what the game actually writes into the
/// SaveGames directory - a loose <c>*.ntwtf.lua</c> blob, and an expanded
/// <c>*.ntwtf</c> save folder containing exactly one such blob.
/// </para>
/// <para>
/// An archive is read in memory rather than expanded to a temp directory that
/// would then need cleaning up.
/// </para>
/// </remarks>
public static class SaveBlob
{
    /// <summary>Extension of an already-expanded save folder.</summary>
    public const string ExpandedExtension = ".ntwtf";

    /// <summary>Extension of a packed save.</summary>
    public const string ZipExtension = ExpandedExtension + ".zip";

    /// <summary>Extension of the decodable blob inside a save.</summary>
    public const string LuaExtension = ExpandedExtension + ".lua";

    /// <summary>One sentence naming every accepted input shape, for error messages.</summary>
    public const string AcceptedInputs =
        $"Expected a packed *{ZipExtension} save, a loose *{LuaExtension} blob, or an "
        + "expanded save folder containing exactly one such blob.";

    /// <summary>Cap on how many names an error message spells out.</summary>
    private const int MaxListedNames = 10;

    /// <summary>Local file header magic every ZIP archive starts with ("PK\x03\x04").</summary>
    private static readonly byte[] ZipMagic = { 0x50, 0x4B, 0x03, 0x04 };

    /// <summary>Reads the save blob named by a command-line input.</summary>
    /// <param name="input">
    /// A <c>*.ntwtf.zip</c> archive, a <c>*.ntwtf.lua</c> file, or a folder
    /// containing exactly one <c>*.ntwtf.lua</c> file.
    /// </param>
    public static byte[] Read(string input)
    {
        if (Directory.Exists(input))
        {
            string[] candidates = Directory.GetFiles(input, "*" + LuaExtension);
            if (candidates.Length != 1)
            {
                string names = FormatNames(candidates.Select(Path.GetFileName));
                throw new InvalidDataException(
                    $"Expected exactly one *{LuaExtension} in folder '{input}', found "
                        + $"{candidates.Length} ({names})."
                );
            }
            return File.ReadAllBytes(candidates[0]);
        }

        if (!File.Exists(input))
        {
            throw new FileNotFoundException(
                $"No such file or folder: '{input}'. {AcceptedInputs}",
                input
            );
        }

        // Recognised by extension, and also by content, so a save that has been
        // renamed is still handled rather than fed to the decoder as a raw blob.
        if (input.EndsWith(ZipExtension, StringComparison.OrdinalIgnoreCase) || LooksLikeZip(input))
        {
            return ReadFromZip(input);
        }

        return File.ReadAllBytes(input);
    }

    private static byte[] ReadFromZip(string path)
    {
        using ZipArchive archive = OpenZip(path);
        List<ZipArchiveEntry> blobs = archive
            .Entries.Where(entry =>
                entry.Name.EndsWith(LuaExtension, StringComparison.OrdinalIgnoreCase)
            )
            .ToList();

        if (blobs.Count != 1)
        {
            // With no match at all the useful thing to show is what the archive
            // does hold; with several, which ones collided. Never pick one.
            bool empty = blobs.Count == 0;
            IEnumerable<ZipArchiveEntry> listed = empty ? archive.Entries : blobs;
            string label = empty ? "archive contains" : "matching entries";
            string names = FormatNames(listed.Select(entry => entry.FullName));
            throw new InvalidDataException(
                $"Expected exactly one *{LuaExtension} entry inside archive '{path}', found "
                    + $"{blobs.Count} ({label}: {names})."
            );
        }

        using Stream entryStream = blobs[0].Open();
        using var buffer = new MemoryStream();
        entryStream.CopyTo(buffer);
        return buffer.ToArray();
    }

    private static ZipArchive OpenZip(string path)
    {
        try
        {
            return ZipFile.OpenRead(path);
        }
        catch (InvalidDataException ex)
        {
            throw new InvalidDataException(
                $"'{path}' is not a readable ZIP archive: {ex.Message}",
                ex
            );
        }
    }

    private static bool LooksLikeZip(string path)
    {
        using FileStream stream = File.OpenRead(path);
        Span<byte> header = stackalloc byte[ZipMagic.Length];
        int read = stream.ReadAtLeast(header, header.Length, throwOnEndOfStream: false);
        return read == header.Length && header.SequenceEqual(ZipMagic);
    }

    private static string FormatNames(IEnumerable<string?> names)
    {
        List<string?> shown = names.Take(MaxListedNames + 1).ToList();
        if (shown.Count == 0)
        {
            return "none";
        }
        bool truncated = shown.Count > MaxListedNames;
        if (truncated)
        {
            shown.RemoveAt(MaxListedNames);
        }
        return string.Join(", ", shown.Select(name => $"'{name}'")) + (truncated ? ", ..." : "");
    }
}
