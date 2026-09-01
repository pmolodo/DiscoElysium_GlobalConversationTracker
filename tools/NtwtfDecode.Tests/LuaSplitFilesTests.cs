// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Persistence.Tests;
using Xunit;

namespace NtwtfDecode.Tests;

/// <summary>Tests for the six-file split representation.</summary>
public class LuaSplitFilesTests
{
    [Fact]
    public void Write_CreatesFiveJsonFilesAndOneBinaryFile()
    {
        using var temp = new TempDirectory();
        string split = temp.Combine("split");
        LuaTable document = LuaTableVisitor.ReadAllTables(
            LuaBlob.SerializeSampleSave(),
            out _
        );

        LuaSplitFiles.Write(split, document, indent: 2);

        string[] names = Directory
            .GetFiles(split)
            .Select(Path.GetFileName)
            .OrderBy(name => name, StringComparer.Ordinal)
            .ToArray()!;
        Assert.Equal(
            new[]
            {
                "Actor.json",
                "Conversation.json",
                "Item.json",
                "Location.json",
                "Variable.json",
                "trailing.bin",
            },
            names
        );
    }

    [Fact]
    public void SplitFilesToLua_IsBitForBitIdentical()
    {
        using var temp = new TempDirectory();
        string split = temp.Combine("split");
        byte[] tables = LuaBlob.SerializeSampleSave();
        byte[] original = tables.Concat(new byte[] { 0x03, 0x66, 0x6F, 0x6F }).ToArray();
        LuaTable document = LuaTableVisitor.ReadAllTables(original, out _);

        LuaSplitFiles.Write(split, document, indent: 2);
        LuaTable reconstructed = LuaSplitFiles.Read(split);
        using var output = new MemoryStream();
        LuaBinary.WriteDocument(output, reconstructed);

        Assert.Equal(original, output.ToArray());
        Assert.Equal(
            new byte[] { 0x03, 0x66, 0x6F, 0x6F },
            File.ReadAllBytes(Path.Combine(split, LuaSplitFiles.TrailingFileName))
        );
    }

    [Fact]
    public void Read_RequiresTheTrailingBinaryFile()
    {
        using var temp = new TempDirectory();
        string split = temp.Combine("split");
        LuaTable document = LuaTableVisitor.ReadAllTables(
            LuaBlob.SerializeSampleSave(),
            out _
        );
        LuaSplitFiles.Write(split, document, indent: 2);
        File.Delete(Path.Combine(split, LuaSplitFiles.TrailingFileName));

        Assert.Throws<FileNotFoundException>(() => LuaSplitFiles.Read(split));
    }

    [Fact]
    public void DiffOfUnchangedSaveContainsOnlyManifestAndRebuilds()
    {
        using var temp = new TempDirectory();
        string baseline = temp.Combine("baseline");
        string diff = temp.Combine("diff");
        byte[] original = LuaBlob.SerializeSampleSave();
        LuaTable document = LuaTableVisitor.ReadAllTables(original, out _);
        LuaSplitFiles.Write(baseline, document, indent: 2, sparse: true);

        LuaSplitFiles.WriteDiff(diff, document, baseline, indent: 2);
        LuaTable rebuilt = LuaSplitFiles.Read(diff);
        using var output = new MemoryStream();
        LuaBinary.WriteDocument(output, rebuilt);

        Assert.Equal(
            new[] { SparseDiff.ManifestFileName },
            Directory.GetFiles(diff).Select(Path.GetFileName).ToArray()
        );
        Assert.Equal(original, output.ToArray());
    }

    [Fact]
    public void DiffCarriesChangedTrailingBytes()
    {
        using var temp = new TempDirectory();
        string baseline = temp.Combine("baseline");
        string diff = temp.Combine("diff");
        byte[] original = LuaBlob.SerializeSampleSave();
        LuaTable baseDocument = LuaTableVisitor.ReadAllTables(original, out _);
        LuaTable changed = LuaTableVisitor.ReadAllTables(original, out _);
        changed.TrailingBytes = new byte[] { 1, 2, 3 };
        LuaSplitFiles.Write(baseline, baseDocument, indent: 2, sparse: true);

        LuaSplitFiles.WriteDiff(diff, changed, baseline, indent: 2);
        LuaTable rebuilt = LuaSplitFiles.Read(diff);

        Assert.Equal(new byte[] { 1, 2, 3 }, rebuilt.TrailingBytes);
        Assert.True(File.Exists(Path.Combine(diff, LuaSplitFiles.TrailingFileName)));
    }
}
