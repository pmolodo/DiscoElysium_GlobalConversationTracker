// SPDX-License-Identifier: MIT
using System.IO.Compression;
using GlobalConversationTracker.Persistence.Tests;
using Xunit;

namespace NtwtfDecode.Tests;

/// <summary>Tests for rebuilding a game archive from a sparse expanded source.</summary>
public class ExpandedSaveTests
{
    [Fact]
    public void Write_PreservesCompanionsAndNestsSplitLuaParts()
    {
        using var temp = new TempDirectory();
        string source = temp.Combine("chosen.ntwtf");
        byte[] original = LuaBlob.SerializeSampleSave();
        LuaTable document = LuaTableVisitor.ReadAllTables(original, out _);
        byte[] state = "state"u8.ToArray();
        byte[] metadata = "{}"u8.ToArray();
        var packed = new PackedSave(
            "chosen.ntwtf.lua",
            original,
            new[]
            {
                new PackedSaveEntry("chosen.states.lua", state),
                new PackedSaveEntry("chosen.1st.ntwtf.json", metadata),
            }
        );

        ExpandedSave.Write(source, packed, document, indent: 2, sparse: true, baseline: null);

        Assert.Equal(state, File.ReadAllBytes(Path.Combine(source, "chosen.states.lua")));
        Assert.Equal(
            metadata,
            File.ReadAllBytes(Path.Combine(source, "chosen.1st.ntwtf.json"))
        );
        string parts = Path.Combine(source, "chosen.ntwtf.lua.parts");
        Assert.True(Directory.Exists(parts));
        Assert.Equal(6, Directory.GetFiles(parts).Length);
        Assert.False(File.Exists(Path.Combine(source, "chosen.ntwtf.lua")));
    }

    [Fact]
    public void Pack_ReconstructsLuaAndIncludesPassThroughFiles()
    {
        using var temp = new TempDirectory();
        string source = temp.Combine("chosen.ntwtf");
        string parts = Path.Combine(source, "chosen.ntwtf.lua.parts");
        string output = temp.Combine("chosen.ntwtf.zip");
        byte[] original = LuaBlob.SerializeSampleSave();
        LuaTable document = LuaTableVisitor.ReadAllTables(original, out _);
        LuaSplitFiles.Write(parts, document, indent: 2, sparse: true);
        File.WriteAllText(Path.Combine(source, "chosen.states.lua"), "state");

        ExpandedSave.Pack(source, output);

        Assert.Equal(original, SaveBlob.Read(output));
        using ZipArchive archive = ZipFile.OpenRead(output);
        Assert.Equal(
            new[] { "chosen.ntwtf.lua", "chosen.states.lua" },
            archive.Entries.Select(entry => entry.FullName).ToArray()
        );
    }

    [Fact]
    public void Pack_RejectsPassThroughFilesForAnotherSave()
    {
        using var temp = new TempDirectory();
        string source = temp.Combine("chosen.ntwtf");
        string parts = Path.Combine(source, "chosen.ntwtf.lua.parts");
        LuaTable document = LuaTableVisitor.ReadAllTables(
            LuaBlob.SerializeSampleSave(),
            out _
        );
        LuaSplitFiles.Write(parts, document, indent: 2, sparse: true);
        File.WriteAllText(Path.Combine(source, "other.states.lua"), "state");

        Assert.Throws<InvalidDataException>(() =>
            ExpandedSave.Pack(source, temp.Combine("chosen.ntwtf.zip"))
        );
    }
}
