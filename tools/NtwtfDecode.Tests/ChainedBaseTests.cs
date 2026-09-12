// SPDX-License-Identifier: MIT
using System.Text.Json.Nodes;
using GlobalConversationTracker.Persistence.Tests;
using Xunit;

namespace NtwtfDecode.Tests;

/// <summary>
/// A sparse diff whose base is itself a sparse diff.
/// </summary>
/// <remarks>
/// <para>What this is for: several saves that share a setup should be able to state it
/// once. An intermediate diff carries the shared changes and each save beyond it
/// carries only what makes it different, so the layout says which fields vary instead
/// of leaving a reader to compare the copies and work it out.</para>
///
/// <para>Before this, a base was assumed complete: the expanded reader took a base
/// directory's diff files for the members they describe, and the sparse table reader
/// refused them for not being sparse.</para>
/// </remarks>
public class ChainedBaseTests
{
    /// <summary>The member whose contents each test varies.</summary>
    private const string MetadataSuffix = ".1st.ntwtf.json";

    /// <summary>A save with the given metadata, as the game would have written it.</summary>
    private static (PackedSave Packed, LuaTable Document) Save(string name, JsonObject metadata)
    {
        byte[] original = LuaBlob.SerializeSampleSave();
        LuaTable document = LuaTableVisitor.ReadAllTables(original, out _);
        var packed = new PackedSave(
            name + ".ntwtf.lua",
            original,
            new[]
            {
                new PackedSaveEntry(name + ".states.lua", "state"u8.ToArray()),
                new PackedSaveEntry(
                    name + MetadataSuffix,
                    System.Text.Encoding.UTF8.GetBytes(metadata.ToJsonString())
                ),
            }
        );
        return (packed, document);
    }

    /// <summary>Writes a complete expanded save with the given pass-through members.</summary>
    private static void WriteComplete(string directory, string name, JsonObject metadata)
    {
        (PackedSave packed, LuaTable document) = Save(name, metadata);
        ExpandedSave.Write(directory, packed, document, indent: 2, sparse: true, baseline: null);
    }

    /// <summary>Writes one as a diff of a base, which may itself be a diff.</summary>
    private static void WriteDiff(
        string directory,
        string name,
        JsonObject metadata,
        string baseline
    )
    {
        (PackedSave packed, LuaTable document) = Save(name, metadata);
        ExpandedSave.Write(directory, packed, document, indent: 2, sparse: true, baseline);
    }

    /// <summary>Resolves an expanded save and reads the member each test varies.</summary>
    private static JsonNode ReadMetadata(string source)
    {
        PackedSaveEntry entry = ExpandedSave.MembersBySuffix(source)[MetadataSuffix];
        return JsonNode.Parse(System.Text.Encoding.UTF8.GetString(entry.Bytes))
            ?? throw new InvalidDataException("metadata is not JSON");
    }

    [Fact]
    public void ADiffOfADiffResolvesThroughBothBases()
    {
        using var temp = new TempDirectory();

        // A complete base, then a diff that changes one field, then a diff of THAT which
        // changes a different one. Only the last is resolved, and both changes must show.
        string root = temp.Combine("root.ntwtf");
        WriteComplete(root, "root", new JsonObject
        {
            ["area"] = "Whirling",
            ["money"] = 0,
        });

        string shared = temp.Combine("shared.ntwtf");
        WriteDiff(shared, "shared", new JsonObject
        {
            ["area"] = "Martinaise",
            ["money"] = 0,
        }, root);

        string leaf = temp.Combine("leaf.ntwtf");
        WriteDiff(leaf, "leaf", new JsonObject
        {
            ["area"] = "Martinaise",
            ["money"] = 5100,
        }, shared);

        JsonNode metadata = ReadMetadata(leaf);

        Assert.Equal("Martinaise", metadata["area"]!.GetValue<string>());
        Assert.Equal(5100, metadata["money"]!.GetValue<int>());
    }

    [Fact]
    public void TheLeafOnlyStoresWhatItChanges()
    {
        using var temp = new TempDirectory();

        string root = temp.Combine("root.ntwtf");
        WriteComplete(root, "root", new JsonObject { ["area"] = "Whirling", ["money"] = 0 });

        string shared = temp.Combine("shared.ntwtf");
        WriteDiff(shared, "shared", new JsonObject
        {
            ["area"] = "Martinaise",
            ["money"] = 0,
        }, root);

        string leaf = temp.Combine("leaf.ntwtf");
        WriteDiff(leaf, "leaf", new JsonObject
        {
            ["area"] = "Martinaise",
            ["money"] = 5100,
        }, shared);

        // The point of chaining: the leaf says nothing about the area, because its base
        // already does. That is what makes the layout state which field varies.
        string metadata = File.ReadAllText(
            Directory.GetFiles(leaf, "*" + MetadataSuffix).Single());

        Assert.Contains("money", metadata);
        Assert.DoesNotContain("Martinaise", metadata);
    }

    [Fact]
    public void ABaseChainThatReturnsToItselfIsRefused()
    {
        // A cycle is the only way resolution could fail to terminate, so it is the one
        // thing worth refusing outright. Built by pointing a finished diff at itself,
        // which is what a mis-edited base path would do.
        using var temp = new TempDirectory();

        string root = temp.Combine("root.ntwtf");
        WriteComplete(root, "root", new JsonObject { ["area"] = "Whirling" });

        string leaf = temp.Combine("leaf.ntwtf");
        WriteDiff(leaf, "leaf", new JsonObject { ["area"] = "Martinaise" }, root);

        string manifestPath = Path.Combine(leaf, ExpandedSave.DiffManifestFileName);
        JsonObject manifest = JsonNode.Parse(File.ReadAllText(manifestPath))!.AsObject();
        manifest["base"] = ".";
        File.WriteAllText(manifestPath, manifest.ToJsonString());

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => ExpandedSave.MembersBySuffix(leaf));

        Assert.Contains("returns to", error.Message);
    }

    [Fact]
    public void ACompleteBaseStillWorks()
    {
        // The chain is an addition, not a replacement: one level must behave as before.
        using var temp = new TempDirectory();

        string root = temp.Combine("root.ntwtf");
        WriteComplete(root, "root", new JsonObject { ["area"] = "Whirling", ["money"] = 0 });

        string leaf = temp.Combine("leaf.ntwtf");
        WriteDiff(leaf, "leaf", new JsonObject
        {
            ["area"] = "Whirling",
            ["money"] = 42,
        }, root);

        JsonNode metadata = ReadMetadata(leaf);

        Assert.Equal("Whirling", metadata["area"]!.GetValue<string>());
        Assert.Equal(42, metadata["money"]!.GetValue<int>());
    }
}
