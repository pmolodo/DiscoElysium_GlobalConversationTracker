// SPDX-License-Identifier: MIT
using System.IO.Compression;
using System.Text.Json.Nodes;
using GlobalConversationTracker.Persistence.Tests;
using Xunit;

namespace NtwtfDecode.Tests;

/// <summary>Tests for writing a save out as a sparse expanded directory.</summary>
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

    /// <summary>
    /// Every member is diffed against the base's, and every one resolves back to itself.
    /// </summary>
    /// <remarks>
    /// The three kinds in one save: a JSON member that changed, one that did not and is
    /// inherited, and a text member that changed. What is under test is the WRITER - that
    /// it picks the right kind for each and writes only what differs - and what proves it
    /// is resolving the result and finding the members it started from.
    /// </remarks>
    [Fact]
    public void WriteDiff_DiffsEveryMemberAndResolvesBackToIt()
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

        IReadOnlyDictionary<string, PackedSaveEntry> resolved =
            ExpandedSave.MembersBySuffix(expanded);
        using var rebuilt = new MemoryStream();
        LuaBinary.WriteDocument(rebuilt, ExpandedSave.ReadBaseDocument(expanded));
        Assert.Equal(lua, rebuilt.ToArray());
        foreach (PackedSaveEntry expected in target.PassThrough)
        {
            string suffix = expected.Name["target".Length..];
            PackedSaveEntry actual = resolved[suffix];

            // A JSON member is compared as a document: a diff is applied by a parser, so
            // what comes back is the same object written out by a writer of its own.
            if (suffix.EndsWith(".json", StringComparison.Ordinal))
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
