// SPDX-License-Identifier: MIT
using System.IO.Compression;
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
    /// <summary>Writes a complete expanded save with the given pass-through members.</summary>
    private static void WriteComplete(string directory, string name, JsonObject metadata)
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
                    name + ".1st.ntwtf.json",
                    System.Text.Encoding.UTF8.GetBytes(metadata.ToJsonString())
                ),
            }
        );
        ExpandedSave.Write(directory, packed, document, indent: 2, sparse: true, baseline: null);
    }

    /// <summary>Packs an expanded source and reads one member back out.</summary>
    private static JsonNode ReadMetadata(string source, string output)
    {
        string actual = ExpandedSave.Pack(source, output, new DateTime(2026, 9, 1, 12, 0, 0));
        using ZipArchive archive = ZipFile.OpenRead(actual);
        ZipArchiveEntry entry = archive.Entries.Single(e =>
            e.Name.EndsWith(".1st.ntwtf.json", StringComparison.Ordinal));
        using Stream stream = entry.Open();
        using var reader = new StreamReader(stream);
        return JsonNode.Parse(reader.ReadToEnd())
            ?? throw new InvalidDataException("metadata is not JSON");
    }

    /// <summary>Diffs a complete save against a base, which may itself be a diff.</summary>
    private static void WriteDiff(string source, string target, string baseline)
    {
        string packed = ExpandedSave.Pack(
            source, Path.Combine(Path.GetDirectoryName(target)!, "seed.ntwtf.zip"));
        PackedSave read = SaveBlob.ReadArchive(packed);
        LuaTable document = LuaTableVisitor.ReadAllTables(read.LuaBytes, out _);
        ExpandedSave.Write(target, read, document, indent: 2, sparse: true, baseline: baseline);
    }

    [Fact]
    public void ADiffOfADiffResolvesThroughBothBases()
    {
        using var temp = new TempDirectory();

        // A complete base, then a diff that changes one field, then a diff of THAT which
        // changes a different one. Only the last is packed, and both changes must show.
        string root = temp.Combine("root.ntwtf");
        WriteComplete(root, "root", new JsonObject
        {
            ["area"] = "Whirling",
            ["money"] = 0,
        });

        string shared = temp.Combine("shared.ntwtf");
        string sharedSource = temp.Combine("shared-source.ntwtf");
        WriteComplete(sharedSource, "shared", new JsonObject
        {
            ["area"] = "Martinaise",
            ["money"] = 0,
        });
        WriteDiff(sharedSource, shared, root);

        string leafSource = temp.Combine("leaf-source.ntwtf");
        WriteComplete(leafSource, "leaf", new JsonObject
        {
            ["area"] = "Martinaise",
            ["money"] = 5100,
        });
        string leaf = temp.Combine("leaf.ntwtf");
        WriteDiff(leafSource, leaf, shared);

        JsonNode metadata = ReadMetadata(leaf, temp.Combine("out.ntwtf.zip"));

        Assert.Equal("Martinaise", metadata["area"]!.GetValue<string>());
        Assert.Equal(5100, metadata["money"]!.GetValue<int>());
    }

    [Fact]
    public void TheLeafOnlyStoresWhatItChanges()
    {
        using var temp = new TempDirectory();

        string root = temp.Combine("root.ntwtf");
        WriteComplete(root, "root", new JsonObject { ["area"] = "Whirling", ["money"] = 0 });

        string sharedSource = temp.Combine("shared-source.ntwtf");
        WriteComplete(sharedSource, "shared", new JsonObject
        {
            ["area"] = "Martinaise",
            ["money"] = 0,
        });
        string shared = temp.Combine("shared.ntwtf");
        WriteDiff(sharedSource, shared, root);

        string leafSource = temp.Combine("leaf-source.ntwtf");
        WriteComplete(leafSource, "leaf", new JsonObject
        {
            ["area"] = "Martinaise",
            ["money"] = 5100,
        });
        string leaf = temp.Combine("leaf.ntwtf");
        WriteDiff(leafSource, leaf, shared);

        // The point of chaining: the leaf says nothing about the area, because its base
        // already does. That is what makes the layout state which field varies.
        // The packer stamps the time into member names, so the file is found by suffix.
        string metadata = File.ReadAllText(
            Directory.GetFiles(leaf, "*.1st.ntwtf.json").Single());

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

        string leafSource = temp.Combine("leaf-source.ntwtf");
        WriteComplete(leafSource, "leaf", new JsonObject { ["area"] = "Martinaise" });
        string leaf = temp.Combine("leaf.ntwtf");
        WriteDiff(leafSource, leaf, root);

        string manifestPath = Path.Combine(leaf, ExpandedSave.DiffManifestFileName);
        JsonObject manifest = JsonNode.Parse(File.ReadAllText(manifestPath))!.AsObject();
        manifest["base"] = ".";
        File.WriteAllText(manifestPath, manifest.ToJsonString());

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => ExpandedSave.Pack(leaf, temp.Combine("out.ntwtf.zip")));

        Assert.Contains("returns to", error.Message);
    }

    [Fact]
    public void ACompleteBaseStillWorks()
    {
        // The chain is an addition, not a replacement: one level must behave as before.
        using var temp = new TempDirectory();

        string root = temp.Combine("root.ntwtf");
        WriteComplete(root, "root", new JsonObject { ["area"] = "Whirling", ["money"] = 0 });

        string leafSource = temp.Combine("leaf-source.ntwtf");
        WriteComplete(leafSource, "leaf", new JsonObject
        {
            ["area"] = "Whirling",
            ["money"] = 42,
        });
        string leaf = temp.Combine("leaf.ntwtf");
        WriteDiff(leafSource, leaf, root);

        JsonNode metadata = ReadMetadata(leaf, temp.Combine("out.ntwtf.zip"));

        Assert.Equal("Whirling", metadata["area"]!.GetValue<string>());
        Assert.Equal(42, metadata["money"]!.GetValue<int>());
    }
}
