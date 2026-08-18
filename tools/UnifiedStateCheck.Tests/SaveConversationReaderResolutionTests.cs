using NtwtfDecode;
using NtwtfDecode.Tests;
using UnifiedConversationTracker;
using UnifiedConversationTracker.Persistence.Tests;
using Xunit;

namespace UnifiedStateCheck.Tests;

/// <summary>
/// Finding the save to read: the two things a command line can name - a path, or
/// a bare save name resolved against a save directory - and the listing of what
/// a save directory holds.
/// </summary>
/// <remarks>
/// Which file was read is checked through the path the reader reports, so these
/// are about resolution rather than about decoding; the blob every fixture
/// carries is the synthesised sample, so no game install and no real save is
/// needed. Resolution to the byte level - what counts as a packed save, what a
/// save folder must contain - belongs to SaveBlob and is covered by
/// NtwtfDecode.Tests.
/// </remarks>
public class SaveConversationReaderResolutionTests
{
    private const string SaveName = "autosave";

    /// <summary>Entries of <see cref="LuaBlob.SampleSave"/> above Untouched.</summary>
    private const int SampleSaveEntryCount = 2;

    private static readonly byte[] Blob = LuaBlob.SerializeSampleSave();

    // -------------------------------------------------------------------
    // A path, which is used as given
    // -------------------------------------------------------------------

    [Fact]
    public void Load_PathToALooseBlob_ReadsThatFile()
    {
        using var temp = new TempDirectory();
        string path = temp.WriteFile(SaveName + SaveBlob.LuaExtension, Blob);

        AssertReadTheSampleSave(path, temp.Path, path);
    }

    [Fact]
    public void Load_PathToAPackedSave_ReadsThatFile()
    {
        using var temp = new TempDirectory();
        string path = WritePackedSave(temp, SaveName);

        AssertReadTheSampleSave(path, temp.Path, path);
    }

    [Fact]
    public void Load_PathToAnExpandedSaveFolder_ReadsThatFolder()
    {
        using var temp = new TempDirectory();
        string path = WriteExpandedSave(temp, SaveName);

        AssertReadTheSampleSave(path, temp.Path, path);
    }

    [Fact]
    public void Load_SomethingThatExists_IsNotAlsoLookedUpInTheSaveDirectory()
    {
        // A path wins over the bare-name lookup, even when the save directory
        // holds a save of that name: the caller named a file, so read that file.
        using var temp = new TempDirectory();
        WritePackedSave(temp, SaveName);
        string elsewhere = temp.CreateFolder("elsewhere");
        string path = Path.Combine(elsewhere, SaveName);
        File.WriteAllBytes(path, Blob);

        AssertReadTheSampleSave(path, temp.Path, path);
    }

    // -------------------------------------------------------------------
    // A bare save name, resolved against the save directory
    // -------------------------------------------------------------------

    [Fact]
    public void Load_BareSaveName_ResolvesToThePackedSaveInTheSaveDirectory()
    {
        using var temp = new TempDirectory();
        string packed = WritePackedSave(temp, SaveName);

        AssertReadTheSampleSave(SaveName, temp.Path, packed);
    }

    [Fact]
    public void Load_BareSaveNameWithNoPackedSave_FallsBackToTheExpandedFolder()
    {
        using var temp = new TempDirectory();
        string expanded = WriteExpandedSave(temp, SaveName);

        AssertReadTheSampleSave(SaveName, temp.Path, expanded);
    }

    [Fact]
    public void Load_BareSaveNameThatIsBothPackedAndExpanded_PrefersThePackedSave()
    {
        // The packed save is what the game writes; an expanded folder next to it
        // is someone's unzipped copy, and may well be older.
        using var temp = new TempDirectory();
        string packed = WritePackedSave(temp, SaveName);
        WriteExpandedSave(temp, SaveName);

        AssertReadTheSampleSave(SaveName, temp.Path, packed);
    }

    [Fact]
    public void Load_BareSaveNameThatIsNeither_ThrowsNamingEveryPlaceItLooked()
    {
        using var temp = new TempDirectory();

        FileNotFoundException error = Assert.Throws<FileNotFoundException>(
            () => SaveConversationReader.Load(SaveName, temp.Path, out _)
        );

        Assert.Contains(SaveName + SaveBlob.ZipExtension, error.Message, StringComparison.Ordinal);
        Assert.Contains(
            SaveName + SaveBlob.ExpandedExtension,
            error.Message,
            StringComparison.Ordinal
        );
        Assert.Contains(temp.Path, error.Message, StringComparison.Ordinal);
    }

    // -------------------------------------------------------------------
    // Listing a save directory
    // -------------------------------------------------------------------

    [Fact]
    public void ListSaveNames_DirectoryThatDoesNotExist_IsEmpty()
    {
        using var temp = new TempDirectory();

        Assert.Empty(SaveConversationReader.ListSaveNames(temp.Combine("no-such-directory")));
    }

    [Fact]
    public void ListSaveNames_AnythingThatIsNotASave_IsNotListed()
    {
        using var temp = new TempDirectory();
        temp.WriteFile("thumbnail.png", Array.Empty<byte>());
        temp.CreateFolder("screenshots");
        // A loose blob is not a save the game wrote, and a bare name naming one
        // would not resolve, so it has no business in the list either.
        temp.WriteFile(SaveName + SaveBlob.LuaExtension, Blob);

        Assert.Empty(SaveConversationReader.ListSaveNames(temp.Path));
    }

    [Fact]
    public void ListSaveNames_PackedAndExpandedSaves_AreListedWithoutTheirExtensions()
    {
        using var temp = new TempDirectory();
        WritePackedSave(temp, "zulu");
        WritePackedSave(temp, "alpha");
        WriteExpandedSave(temp, "Bravo");

        Assert.Equal(
            new[] { "alpha", "Bravo", "zulu" },
            SaveConversationReader.ListSaveNames(temp.Path)
        );
    }

    [Fact]
    public void ListSaveNames_SaveThatIsBothPackedAndExpanded_IsListedOnce()
    {
        // Both spellings resolve to the same save, and the packed one is the one
        // a bare name would be read from, so that is the spelling reported.
        using var temp = new TempDirectory();
        WritePackedSave(temp, "Autosave");
        WriteExpandedSave(temp, "autosave");

        Assert.Equal(new[] { "Autosave" }, SaveConversationReader.ListSaveNames(temp.Path));
    }

    // -------------------------------------------------------------------
    // Fixtures
    // -------------------------------------------------------------------

    /// <summary>Writes a packed save into the directory and returns its path.</summary>
    private static string WritePackedSave(TempDirectory temp, string name) =>
        temp.WriteZip(name + SaveBlob.ZipExtension, (name + SaveBlob.LuaExtension, Blob));

    /// <summary>Writes an expanded save folder into the directory and returns its path.</summary>
    private static string WriteExpandedSave(TempDirectory temp, string name)
    {
        string folder = temp.CreateFolder(name + SaveBlob.ExpandedExtension);
        File.WriteAllBytes(Path.Combine(folder, name + SaveBlob.LuaExtension), Blob);
        return folder;
    }

    /// <summary>Asserts the reader read the sample save, and read it from where it says.</summary>
    private static void AssertReadTheSampleSave(
        string spec,
        string saveDirectory,
        string expectedPath
    )
    {
        UnifiedConversationState state = SaveConversationReader.Load(
            spec,
            saveDirectory,
            out string resolvedPath
        );

        Assert.Equal(expectedPath, resolvedPath);
        Assert.Equal(SampleSaveEntryCount, state.EntryCount);
    }
}
