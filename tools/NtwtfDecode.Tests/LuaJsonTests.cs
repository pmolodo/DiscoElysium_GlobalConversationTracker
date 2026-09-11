// SPDX-License-Identifier: MIT
using System.Numerics;
using System.Text;
using System.Text.Json;
using GlobalConversationTracker.Persistence.Tests;
using Xunit;

namespace NtwtfDecode.Tests;

/// <summary>Tests for the reversible Lua/JSON representation.</summary>
public class LuaJsonTests
{
    private const int Indent = 2;

    /// <summary>A manifest path whose dictionary keys are numbers, and one that is not.</summary>
    private const string NumberKeyedPath = "Conversation/7/Dialog";
    private const string StringKeyedPath = "Conversation/7";

    private static string Render(object? value, int? indent = Indent, string path = "table")
    {
        using var buffer = new MemoryStream();
        LuaJson.Write(buffer, value, indent, path);
        return Encoding.UTF8.GetString(buffer.ToArray());
    }

    private static LuaTable ReadTable(string json, string path = "table")
    {
        using var stream = new MemoryStream(Encoding.UTF8.GetBytes(json));
        return LuaJson.ReadTable(stream, path);
    }

    private static InvalidDataException RejectsTable(string json, string path = "table") =>
        Assert.Throws<InvalidDataException>(() => ReadTable(json, path));

    [Fact]
    public void Write_RecordsOnlyTheListBoundaryAlongsideOrdinaryProperties()
    {
        LuaTable table = LuaBlob.List("a", "b");
        table.Add("0.5", "string key");

        using JsonDocument json = JsonDocument.Parse(Render(table));
        JsonElement root = json.RootElement;

        Assert.Equal(2, root.GetProperty("_num_list_entries").GetInt32());
        Assert.Equal("a", root.GetProperty("1").GetString());
        Assert.Equal("b", root.GetProperty("2").GetString());
        Assert.Equal("string key", root.GetProperty("0.5").GetString());
        Assert.Equal(4, root.EnumerateObject().Count());
    }

    [Fact]
    public void Write_NamesNumberKeysBareWhereTheManifestExpectsThem()
    {
        LuaTable dialog = LuaBlob.Table((10, "a"), (11, "b"));

        string json = Render(dialog, indent: null, path: NumberKeyedPath);

        Assert.Equal("""{"10":"a","11":"b"}""", json);
    }

    [Fact]
    public void Write_RejectsAKeyThatViolatesTheManifest()
    {
        LuaTable table = LuaBlob.Table((5, "number key"), ("name", "string key"));

        InvalidDataException error = Assert.Throws<InvalidDataException>(() => Render(table));

        Assert.Contains("expects string", error.Message, StringComparison.Ordinal);
        Assert.Contains("'5' is number", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Write_RejectsAStringKeyWhereTheManifestExpectsNumbers()
    {
        LuaTable dialog = LuaBlob.Table(("10", "string key"));

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => Render(dialog, path: NumberKeyedPath)
        );

        Assert.Contains("expects number", error.Message, StringComparison.Ordinal);
        Assert.Contains("'10' is string", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Write_RejectsADictionaryKeyThatWouldRepeatAListIndex()
    {
        LuaTable table = LuaBlob.List("list value");
        table.Add("1", "string key");

        InvalidDataException error = Assert.Throws<InvalidDataException>(() => Render(table));

        Assert.Contains("has two '1' properties", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Write_RejectsADictionaryKeyNamedForTheListBoundary()
    {
        LuaTable table = LuaBlob.Table(("_num_list_entries", "shadow"));

        InvalidDataException error = Assert.Throws<InvalidDataException>(() => Render(table));

        Assert.Contains(
            "cannot name an entry '_num_list_entries'",
            error.Message,
            StringComparison.Ordinal
        );
    }

    [Fact]
    public void Write_OmitsTheListBoundaryFromATableThatHasNoListPart()
    {
        Assert.Equal("{}", Render(new LuaTable(), indent: null));
        Assert.Equal("""{"a":1}""", Render(LuaBlob.Table(("a", 1)), indent: null));
        Assert.Equal(
            """{"_num_list_entries":1,"1":"a","b":2}""",
            Render(WithDictionaryEntry(LuaBlob.List("a"), "b", 2), indent: null)
        );
    }

    private static LuaTable WithDictionaryEntry(LuaTable table, object key, object? value)
    {
        table.Add(key, value);
        return table;
    }

    [Fact]
    public void Write_RejectsAListEntryThatIsNotKeyedByItsOwnIndex()
    {
        // A hand-built table claiming a list part its keys do not match.
        LuaTable table = LuaBlob.Table((5, "a"));
        table.NumListEntries = 1;

        InvalidDataException error = Assert.Throws<InvalidDataException>(() => Render(table));

        Assert.Contains("list entry 1 is keyed '5'", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Write_RejectsAListCountBeyondTheEntriesPresent()
    {
        LuaTable table = LuaBlob.Table(("a", 1));
        table.NumListEntries = 2;

        InvalidDataException error = Assert.Throws<InvalidDataException>(() => Render(table));

        Assert.Contains("outside 0..1", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void KeyTypeManifest_UsesNumbersOnlyForConversationDialogs()
    {
        Assert.Equal(LuaDictionaryKeyType.Number, LuaKeyTypeManifest.ExpectedType(NumberKeyedPath));
        Assert.Equal(LuaDictionaryKeyType.String, LuaKeyTypeManifest.ExpectedType(StringKeyedPath));
        Assert.Equal(
            LuaDictionaryKeyType.String,
            LuaKeyTypeManifest.ExpectedType("Conversation/7/Dialog/10")
        );
        Assert.Equal(LuaDictionaryKeyType.String, LuaKeyTypeManifest.ExpectedType("Variable/Dialog"));
    }

    [Fact]
    public void Write_WithoutAnIndentIsOneLine()
    {
        string json = Render(LuaBlob.Table(("a", 1)), indent: null);

        Assert.DoesNotContain('\n', json);
    }

    [Fact]
    public void Write_RejectsAValueTypeTheDecoderNeverProduces()
    {
        Assert.Throws<InvalidDataException>(() => Render(LuaBlob.Table(("when", DateTime.Now))));
    }

    [Fact]
    public void Write_KeepsWhatIsNotAnIntegerAsANumber()
    {
        // Narrowing an integral double to an int is only to keep ids readable; a
        // value that is not one is written out as it is.
        LuaTable table = LuaBlob.Table(("Health", 0.5), ("negative zero", -0.0));

        Assert.Equal(
            """{"Health":0.5,"negative zero":-0}""",
            Render(table, indent: null)
        );
    }

    [Fact]
    public void Write_RejectsANumberJsonHasNoLiteralFor()
    {
        LuaTable table = LuaBlob.Table(("broken", double.NaN));

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => Render(table, indent: null)
        );

        Assert.Contains("NaN, which JSON has no number for", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Write_KeepsANumberKeyThatIsNotAnInteger()
    {
        LuaTable dialog = LuaBlob.Table((10.5, "a"), (11, "b"));

        Assert.Equal(
            """{"10.5":"a","11":"b"}""",
            Render(dialog, indent: null, path: NumberKeyedPath)
        );
    }

    [Fact]
    public void Write_StillSupportsArbitrarilyLargeNumbersForDiagnosticOutput()
    {
        string json = Render(
            LuaBlob.Table(("big", new BigInteger(ulong.MaxValue) * 10)),
            indent: null
        );

        Assert.Contains("184467440737095516150", json, StringComparison.Ordinal);
    }

    [Fact]
    public void ReadTable_TypesKeysFromTheManifestRatherThanTheirSpelling()
    {
        const string Json = """{"_num_list_entries": 0, "10": "a"}""";

        Assert.Equal(10, Assert.Single(ReadTable(Json, NumberKeyedPath).Entries).Key);
        Assert.Equal("10", Assert.Single(ReadTable(Json, StringKeyedPath).Entries).Key);
    }

    [Fact]
    public void ReadTable_KeepsListEntriesOutOfTheDictionaryPart()
    {
        LuaTable table = ReadTable("""{"_num_list_entries": 2, "1": "a", "2": "b", "x": "c"}""");

        Assert.Equal(2, table.NumListEntries);
        Assert.Equal(new object[] { 1, 2, "x" }, table.Entries.Select(entry => entry.Key));
    }

    [Fact]
    public void ReadTable_TakesTheListCountOnlyAsTheLeadingProperty()
    {
        // Anywhere else it is an entry, and entries cannot carry that name.
        InvalidDataException error = RejectsTable("""{"x": "a", "_num_list_entries": 1}""");

        Assert.Contains("cannot name an entry", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ReadTable_RejectsAListCountThatIsNotAWholeNumber()
    {
        Assert.Contains(
            "non-negative whole number",
            RejectsTable("""{"_num_list_entries": -1}""").Message,
            StringComparison.Ordinal
        );
        Assert.Contains(
            "non-negative whole number",
            RejectsTable("""{"_num_list_entries": "0"}""").Message,
            StringComparison.Ordinal
        );
    }

    [Fact]
    public void ReadTable_RequiresListEntriesToBeNamedByTheirIndex()
    {
        InvalidDataException error = RejectsTable("""{"_num_list_entries": 2, "1": "a", "3": "b"}""");

        Assert.Contains("list entry 2 must be named '2'", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ReadTable_RejectsAKeyThatIsNotTheTypeTheManifestExpects()
    {
        InvalidDataException error = RejectsTable(
            """{"_num_list_entries": 0, "Title": "a"}""",
            NumberKeyedPath
        );

        Assert.Contains("'Title' is not a valid number", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ReadTable_RejectsANumberKeyThatIsNotSpelledTheWayItIsWritten()
    {
        InvalidDataException error = RejectsTable(
            """{"_num_list_entries": 0, " 10 ": "a"}""",
            NumberKeyedPath
        );

        Assert.Contains("must be spelled '10'", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ReadTable_RejectsARepeatedPropertyName()
    {
        InvalidDataException error = RejectsTable("""{"_num_list_entries": 0, "x": "a", "x": "b"}""");

        Assert.Contains("has two 'x' properties", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ReadTable_NarrowsANumberTheSameWayTheDecoderDoes()
    {
        LuaTable table = ReadTable(
            """
            {"small": 12, "past int32": 5000000000,
             "fraction": 0.5, "negative zero": -0}
            """
        );

        Assert.Equal(12, table.Entries[0].Value);
        Assert.Equal(5_000_000_000.0, table.Entries[1].Value);
        Assert.Equal(0.5, table.Entries[2].Value);
        // Integral and it fits, but int 0 would not write back out as the same bits.
        Assert.Equal(
            unchecked((long)0x8000000000000000),
            BitConverter.DoubleToInt64Bits(Assert.IsType<double>(table.Entries[3].Value))
        );
    }

    [Fact]
    public void ReadTable_ReadsATableWithNoListBoundary()
    {
        Assert.Empty(ReadTable("{}").Entries);

        LuaTable table = ReadTable("""{"a": 1, "b": 2}""");

        Assert.Equal(0, table.NumListEntries);
        Assert.Equal(new object[] { "a", "b" }, table.Entries.Select(entry => entry.Key));
    }

    [Fact]
    public void ReadTable_RejectsAnEntryNamedForTheListBoundary()
    {
        Assert.Contains(
            "cannot name an entry '_num_list_entries'",
            RejectsTable("""{"a": 1, "_num_list_entries": 0}""").Message,
            StringComparison.Ordinal
        );
    }

    [Fact]
    public void ReadDocument_RejectsAMismatchedListCount()
    {
        const string Json = """
            {
              "Actor": {"_num_list_entries": 1},
              "Item": {"_num_list_entries": 0},
              "Location": {"_num_list_entries": 0},
              "Variable": {"_num_list_entries": 0},
              "Conversation": {"_num_list_entries": 0}
            }
            """;
        using var stream = new MemoryStream(Encoding.UTF8.GetBytes(Json));

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => LuaJson.ReadDocument(stream)
        );

        Assert.Contains("_num_list_entries", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void LuaToJsonToLua_IsBitForBitIdentical()
    {
        LuaTable tables = LuaBlob.SampleSave();
        LuaTable edgeCases = LuaBlob.List("list value");
        // A string key that looks like a number, past the end of the list part: the
        // reader has to take its type from the manifest, not from its spelling.
        edgeCases.Add("3", "string key");
        edgeCases.Add("quote\" and\nnewline", "awkward key");
        edgeCases.Add("past int32", 5_000_000_000L);
        edgeCases.Add("most negative int32", int.MinValue);
        edgeCases.Add("negative zero", BitConverter.Int64BitsToDouble(unchecked((long)0x8000000000000000)));
        Assert.True(tables.TryGetValue("Variable", out object? variables));
        ((LuaTable)variables!).Add("EdgeCases", edgeCases);

        byte[] tablesBytes = LuaBlob.Serialize(tables);
        byte[] original = tablesBytes.Concat(new byte[] { 0x03, 0x66, 0x6F, 0x6F }).ToArray();
        LuaTable decoded = LuaTableVisitor.ReadAllTables(original, out int trailing);
        Assert.Equal(4, trailing);

        using var json = new MemoryStream();
        LuaJson.Write(json, decoded, Indent);
        json.Position = 0;
        LuaTable reconstructed = LuaJson.ReadDocument(json);
        using var lua = new MemoryStream();
        LuaBinary.WriteDocument(lua, reconstructed);

        Assert.Equal(original, lua.ToArray());
    }

    /// <summary>
    /// The version a file records is the version that is read back, whatever it is.
    /// </summary>
    /// <remarks>
    /// WHAT THIS WOULD HAVE CAUGHT: the reader stopped one token short and answered "no
    /// version" for every file ever written, which nobody saw because an absent version was
    /// read as 1 and every file was version 1. It surfaced the moment an absent version
    /// started being refused - a reader whose two mistakes cancelled out until one of them
    /// was fixed.
    /// </remarks>
    [Theory]
    [InlineData(1)]
    [InlineData(7)]
    public void TheVersionInAFileIsTheVersionReadBack(int version)
    {
        var table = new LuaTable();
        table.Add("Name", "Fysique");

        using var json = new MemoryStream();
        LuaJson.Write(json, table, Indent, "Actor", "sparse", version);
        json.Position = 0;

        Assert.Equal("sparse", LuaJson.FormatOf(json));
        Assert.Equal(version, LuaJson.VersionOf(json));
    }

    [Fact]
    public void AFileThatRecordsNoVersionAnswersNone()
    {
        var table = new LuaTable();
        table.Add("Name", "Fysique");

        using var json = new MemoryStream();
        LuaJson.Write(json, table, Indent, "Actor");
        json.Position = 0;

        Assert.Null(LuaJson.VersionOf(json));
    }
}
