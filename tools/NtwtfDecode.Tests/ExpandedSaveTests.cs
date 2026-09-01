// SPDX-License-Identifier: MIT
using System.IO.Compression;
using System.Text.Json.Nodes;
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

        string actualOutput = ExpandedSave.Pack(
            source,
            output,
            new DateTime(2026, 8, 31, 20, 13, 30)
        );

        Assert.Equal(
            temp.Combine("chosen(8_31_2026 8-13-30 PM).ntwtf.zip"),
            actualOutput
        );
        Assert.False(File.Exists(output));
        Assert.Equal(original, SaveBlob.Read(actualOutput));
        using ZipArchive archive = ZipFile.OpenRead(actualOutput);
        Assert.Equal(
            new[]
            {
                "chosen(8_31_2026 8-13-30 PM).ntwtf.lua",
                "chosen(8_31_2026 8-13-30 PM).states.lua",
            },
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

    [Fact]
    public void DiffAndPack_ProcessesEveryArchiveMemberAgainstPackedBaseline()
    {
        using var temp = new TempDirectory();
        byte[] lua = LuaBlob.SerializeSampleSave();
        string baselinePath = temp.Combine("base.ntwtf.zip");
        WriteArchive(
            baselinePath,
            "base",
            lua,
            new Dictionary<string, byte[]>
            {
                [".1st.ntwtf.json"] = "{\"nested\":{\"old\":1,\"keep\":2}}"u8.ToArray(),
                [".2nd.ntwtf.json"] = "{\"same\":true}"u8.ToArray(),
                [".states.lua"] = "old\r\nstate"u8.ToArray(),
            }
        );
        var target = new PackedSave(
            "target.ntwtf.lua",
            lua,
            new[]
            {
                new PackedSaveEntry(
                    "target.1st.ntwtf.json",
                    "{\"nested\":{\"keep\":2,\"new\":3}}"u8.ToArray()
                ),
                new PackedSaveEntry("target.2nd.ntwtf.json", "{\"same\":true}"u8.ToArray()),
                new PackedSaveEntry("target.states.lua", "new\nstate\n"u8.ToArray()),
            }
        );
        string expanded = temp.Combine("target.ntwtf");
        LuaTable document = LuaTableVisitor.ReadAllTables(lua, out _);

        ExpandedSave.Write(expanded, target, document, 2, sparse: true, baselinePath);

        Assert.True(File.Exists(Path.Combine(expanded, ExpandedSave.DiffManifestFileName)));
        Assert.True(File.Exists(Path.Combine(expanded, "target.1st.ntwtf.json")));
        Assert.False(File.Exists(Path.Combine(expanded, "target.2nd.ntwtf.json")));
        Assert.StartsWith(
            "--- a/target.states.lua",
            File.ReadAllText(Path.Combine(expanded, "target.states.lua.diff"))
        );

        string rebuilt = temp.Combine("target.ntwtf.zip");
        string actualRebuilt = ExpandedSave.Pack(
            expanded,
            rebuilt,
            new DateTime(2026, 8, 31, 20, 13, 30)
        );
        PackedSave packed = SaveBlob.ReadArchive(actualRebuilt);
        Assert.Equal(lua, packed.LuaBytes);
        Assert.Equal(
            target.PassThrough.Select(entry =>
                entry.Name.Replace("target", "target(8_31_2026 8-13-30 PM)")
            ),
            packed.PassThrough.Select(entry => entry.Name)
        );
        foreach (PackedSaveEntry expected in target.PassThrough)
        {
            string expectedName = expected.Name.Replace(
                "target",
                "target(8_31_2026 8-13-30 PM)"
            );
            PackedSaveEntry actual = packed.PassThrough.Single(entry => entry.Name == expectedName);
            if (expected.Name.EndsWith(".json", StringComparison.Ordinal))
            {
                Assert.True(
                    JsonNode.DeepEquals(JsonNode.Parse(expected.Bytes), JsonNode.Parse(actual.Bytes))
                );
            }
            else
            {
                Assert.Equal(expected.Bytes, actual.Bytes);
            }
        }
    }

    private static void WriteArchive(
        string path,
        string stem,
        byte[] lua,
        IReadOnlyDictionary<string, byte[]> companions
    )
    {
        using ZipArchive archive = ZipFile.Open(path, ZipArchiveMode.Create);
        WriteEntry(archive, stem + SaveBlob.LuaExtension, lua);
        foreach ((string suffix, byte[] bytes) in companions)
        {
            WriteEntry(archive, stem + suffix, bytes);
        }
    }

    private static void WriteEntry(ZipArchive archive, string name, byte[] bytes)
    {
        using Stream stream = archive.CreateEntry(name).Open();
        stream.Write(bytes);
    }
}
