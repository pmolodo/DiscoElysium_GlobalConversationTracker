// SPDX-License-Identifier: MIT
using System.IO.Compression;

namespace NtwtfDecode.Tests;

/// <summary>
/// A scratch directory that stands in for the game's SaveGames directory,
/// deleted when the test finishes. Every fixture the tests need - a loose blob,
/// a packed save, a save folder - is built here, so no game install and no real
/// save is required to run them.
/// </summary>
internal sealed class TempDirectory : IDisposable
{
    public TempDirectory()
    {
        Path = System.IO.Path.Combine(
            System.IO.Path.GetTempPath(),
            "ntwtf-decode-tests",
            Guid.NewGuid().ToString("N")
        );
        Directory.CreateDirectory(Path);
    }

    /// <summary>The directory, with no trailing separator.</summary>
    public string Path { get; }

    /// <summary>A path inside the directory, which need not exist.</summary>
    public string Combine(string name) => System.IO.Path.Combine(Path, name);

    /// <summary>Writes a file and returns its path.</summary>
    public string WriteFile(string name, byte[] content)
    {
        string path = Combine(name);
        File.WriteAllBytes(path, content);
        return path;
    }

    /// <summary>Creates a subdirectory and returns its path.</summary>
    public string CreateFolder(string name)
    {
        string path = Combine(name);
        Directory.CreateDirectory(path);
        return path;
    }

    /// <summary>Writes a ZIP archive holding the given entries, and returns its path.</summary>
    public string WriteZip(string name, params (string EntryName, byte[] Content)[] entries)
    {
        string path = Combine(name);
        using (FileStream file = File.Create(path))
        using (var archive = new ZipArchive(file, ZipArchiveMode.Create))
        {
            foreach ((string entryName, byte[] content) in entries)
            {
                using Stream entryStream = archive.CreateEntry(entryName).Open();
                entryStream.Write(content);
            }
        }
        return path;
    }

    public void Dispose()
    {
        try
        {
            Directory.Delete(Path, recursive: true);
        }
        catch (DirectoryNotFoundException)
        {
            // Nothing to clean up.
        }
    }
}
