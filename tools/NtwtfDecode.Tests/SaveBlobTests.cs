using System.Text;
using Xunit;
using GlobalConversationTracker.Persistence;
using GlobalConversationTracker.Persistence.Tests;

namespace NtwtfDecode.Tests;

/// <summary>
/// The input-resolution matrix: every shape a command-line input can take - a
/// packed save, a loose blob, a save folder - and every way each of them can be
/// wrong. All fixtures are synthesised, so none of this needs a game install or
/// a real save.
/// </summary>
public class SaveBlobTests
{
    /// <summary>A blob that decodes, so a successful read can be checked end to end.</summary>
    private static readonly byte[] Blob = LuaBlob.SerializeSampleSave();

    private const string SaveName = "autosave";
    private const string BlobName = SaveName + SaveBlob.LuaExtension;
    private const string ZipName = SaveName + SaveBlob.ZipExtension;

    /// <summary>Anything else a packed save carries alongside the blob.</summary>
    private static readonly (string EntryName, byte[] Content) Thumbnail = (
        "thumbnail.png",
        Encoding.UTF8.GetBytes("not really a png")
    );

    private static (string EntryName, byte[] Content) BlobEntry(string name) => (name, Blob);

    // -------------------------------------------------------------------
    // Packed saves - what the game actually writes into SaveGames
    // -------------------------------------------------------------------

    [Fact]
    public void Read_PackedSave_ReturnsTheBlobFromInsideTheArchive()
    {
        using var temp = new TempDirectory();
        string path = temp.WriteZip(ZipName, Thumbnail, BlobEntry(BlobName));

        Assert.Equal(Blob, SaveBlob.Read(path));
    }

    [Fact]
    public void Read_PackedSave_FindsABlobNestedInsideTheArchive()
    {
        using var temp = new TempDirectory();
        string path = temp.WriteZip(ZipName, BlobEntry(SaveName + ".ntwtf/" + BlobName));

        Assert.Equal(Blob, SaveBlob.Read(path));
    }

    [Fact]
    public void Read_PackedSaveWithNoBlobEntry_ThrowsAndSaysWhatTheArchiveHolds()
    {
        using var temp = new TempDirectory();
        string path = temp.WriteZip(ZipName, Thumbnail);

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => SaveBlob.Read(path)
        );

        Assert.Contains("found 0", error.Message, StringComparison.Ordinal);
        Assert.Contains(Thumbnail.EntryName, error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Read_PackedSaveWithTwoBlobEntries_ThrowsAndNamesBoth()
    {
        using var temp = new TempDirectory();
        string second = "manualsave" + SaveBlob.LuaExtension;
        string path = temp.WriteZip(ZipName, BlobEntry(BlobName), BlobEntry(second));

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => SaveBlob.Read(path)
        );

        // Never pick one: which save was meant is the caller's to say.
        Assert.Contains("found 2", error.Message, StringComparison.Ordinal);
        Assert.Contains(BlobName, error.Message, StringComparison.Ordinal);
        Assert.Contains(second, error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Read_ArchiveRenamedToHideItsExtension_IsStillReadAsAnArchive()
    {
        // Recognised by content as well as by name, so a save someone renamed is
        // not handed to the decoder as if it were a raw blob.
        using var temp = new TempDirectory();
        string path = temp.WriteZip(SaveName + ".bak", BlobEntry(BlobName));

        Assert.Equal(Blob, SaveBlob.Read(path));
    }

    [Fact]
    public void Read_PackedSaveExtensionInAnyCase_IsReadAsAnArchive()
    {
        using var temp = new TempDirectory();
        string path = temp.WriteZip(
            SaveName.ToUpperInvariant() + ".NTWTF.ZIP",
            BlobEntry(BlobName)
        );

        Assert.Equal(Blob, SaveBlob.Read(path));
    }

    [Fact]
    public void Read_PackedSaveThatIsNotAnArchive_ThrowsSayingSo()
    {
        // A .ntwtf.zip that is not a ZIP: the extension promises an archive, so
        // the failure has to be about the archive, not about the blob decoder.
        using var temp = new TempDirectory();
        string path = temp.WriteFile(ZipName, Encoding.UTF8.GetBytes("this is not a zip"));

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => SaveBlob.Read(path)
        );

        Assert.Contains("ZIP", error.Message, StringComparison.Ordinal);
        Assert.Contains(path, error.Message, StringComparison.Ordinal);
    }

    // -------------------------------------------------------------------
    // Loose blobs
    // -------------------------------------------------------------------

    [Fact]
    public void Read_LooseBlob_ReturnsItsBytesVerbatim()
    {
        using var temp = new TempDirectory();
        string path = temp.WriteFile(BlobName, Blob);

        Assert.Equal(Blob, SaveBlob.Read(path));
    }

    [Fact]
    public void Read_TruncatedBlob_ReturnsWhatIsThereAndLeavesTheDecoderToRejectIt()
    {
        // Resolving the input and decoding it are separate jobs: a short file is
        // still the file the caller named, and the decoder is what notices.
        using var temp = new TempDirectory();
        byte[] truncated = Blob[..(Blob.Length / 2)];
        string path = temp.WriteFile(BlobName, truncated);

        Assert.Equal(truncated, SaveBlob.Read(path));
        Assert.Throws<EndOfStreamException>(
            () => LuaTableVisitor.ReadAllTables(SaveBlob.Read(path), out _)
        );
    }

    // -------------------------------------------------------------------
    // Expanded save folders
    // -------------------------------------------------------------------

    [Fact]
    public void Read_FolderWithOneBlob_ReturnsThatBlob()
    {
        using var temp = new TempDirectory();
        string folder = temp.CreateFolder(SaveName + ".ntwtf");
        File.WriteAllBytes(Path.Combine(folder, BlobName), Blob);
        File.WriteAllBytes(Path.Combine(folder, Thumbnail.EntryName), Thumbnail.Content);

        Assert.Equal(Blob, SaveBlob.Read(folder));
    }

    [Fact]
    public void Read_FolderWithNoBlob_Throws()
    {
        using var temp = new TempDirectory();
        string folder = temp.CreateFolder(SaveName + ".ntwtf");

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => SaveBlob.Read(folder)
        );

        Assert.Contains("found 0", error.Message, StringComparison.Ordinal);
        Assert.Contains(folder, error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Read_FolderWithTwoBlobs_ThrowsAndNamesBoth()
    {
        using var temp = new TempDirectory();
        string folder = temp.CreateFolder(SaveName + ".ntwtf");
        string second = "manualsave" + SaveBlob.LuaExtension;
        File.WriteAllBytes(Path.Combine(folder, BlobName), Blob);
        File.WriteAllBytes(Path.Combine(folder, second), Blob);

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => SaveBlob.Read(folder)
        );

        Assert.Contains("found 2", error.Message, StringComparison.Ordinal);
        Assert.Contains(BlobName, error.Message, StringComparison.Ordinal);
        Assert.Contains(second, error.Message, StringComparison.Ordinal);
    }

    // -------------------------------------------------------------------
    // Nothing there at all
    // -------------------------------------------------------------------

    [Fact]
    public void Read_MissingPath_ThrowsFileNotFoundListingTheAcceptedInputs()
    {
        using var temp = new TempDirectory();
        string missing = temp.Combine("no-such-save" + SaveBlob.ZipExtension);

        FileNotFoundException error = Assert.Throws<FileNotFoundException>(
            () => SaveBlob.Read(missing)
        );

        Assert.Contains(missing, error.Message, StringComparison.Ordinal);
        Assert.Contains(SaveBlob.AcceptedInputs, error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Read_EmptyFile_ReturnsNoBytes()
    {
        // Not an archive by name or by content, so it is a blob - an empty one,
        // which the decoder then rejects.
        using var temp = new TempDirectory();
        string path = temp.WriteFile(BlobName, Array.Empty<byte>());

        Assert.Empty(SaveBlob.Read(path));
    }
}
