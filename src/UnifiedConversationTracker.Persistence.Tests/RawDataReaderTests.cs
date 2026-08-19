using System;
using System.IO;
using System.Linq;
using System.Text;
using Xunit;

namespace UnifiedConversationTracker.Persistence.Tests;

/// <summary>
/// Round trip and failure tests for the save-blob decoder, over blobs written by
/// <see cref="LuaBlob"/>.
/// </summary>
public class RawDataReaderTests
{
    /// <summary>The five tables, decoded from a freshly written sample blob.</summary>
    private static LuaTable ReadSample(out int trailingBytes) =>
        RawDataReader.ReadAllTables(LuaBlob.SerializeSampleSave(), out trailingBytes);

    /// <summary>Follows a chain of keys into nested tables.</summary>
    private static object? Lookup(LuaTable table, params object[] path)
    {
        object? current = table;
        foreach (object key in path)
        {
            LuaTable next = Assert.IsType<LuaTable>(current);
            Assert.True(next.TryGetValue(key, out current), $"No entry '{key}'.");
        }
        return current;
    }

    /// <summary>The tool's own rendering, used to compare whole tables at once.</summary>
    private static string ToJson(object? value)
    {
        var writer = new StringWriter();
        PythonJson.Write(writer, value, indent: 2);
        return writer.ToString();
    }

    [Fact]
    public void ReadAllTables_RoundTripsAWrittenBlob()
    {
        LuaTable expected = LuaBlob.SampleSave();

        LuaTable actual = RawDataReader.ReadAllTables(
            LuaBlob.Serialize(expected),
            out int trailingBytes
        );

        Assert.Equal(0, trailingBytes);
        Assert.Equal(ToJson(expected), ToJson(actual));
    }

    [Fact]
    public void ReadAllTables_ReadsTheFiveTablesInFileOrder()
    {
        LuaTable tables = ReadSample(out _);

        Assert.Equal(
            RawDataReader.TableNames,
            tables.Entries.Select(entry => entry.Key).Cast<string>().ToArray()
        );
    }

    [Fact]
    public void ReadAllTables_ReadsNestedConversationStatuses()
    {
        LuaTable tables = ReadSample(out _);

        Assert.Equal(
            "WasDisplayed",
            Lookup(tables, "Conversation", "7", "Dialog", "10", "SimStatus")
        );
        Assert.Equal("Kim Kitsuragi", Lookup(tables, "Conversation", "7", "Title"));
    }

    [Fact]
    public void ReadAllTables_ListPartKeysAreOneIndexed()
    {
        LuaTable tables = ReadSample(out _);

        LuaTable flags = Assert.IsType<LuaTable>(Lookup(tables, "Variable", "Flags"));
        Assert.Equal(new object[] { 1, 2, 3 }, flags.Entries.Select(entry => entry.Key));
        Assert.Equal("a", flags.Entries[0].Value);
        Assert.Equal(3, flags.Entries[2].Value);
    }

    [Fact]
    public void ReadAllTables_IntegralNumbersComeBackAsIntegers()
    {
        LuaTable tables = ReadSample(out _);

        // Every number on the wire is a double; IDs and counts are only useful
        // as 12 rather than 12.0, but a real fraction has to survive.
        Assert.Equal(12, Lookup(tables, "Variable", "Money"));
        Assert.Equal(0.5, Lookup(tables, "Variable", "Health"));
        Assert.Equal(false, Lookup(tables, "Actor", "Kim", "IsPlayer"));
        Assert.Null(Lookup(tables, "Variable", "Unset"));
    }

    [Fact]
    public void ReadAllTables_EmptyTableIsRead()
    {
        LuaTable tables = ReadSample(out _);

        Assert.Empty(Assert.IsType<LuaTable>(Lookup(tables, "Location")).Entries);
    }

    [Fact]
    public void ReadAllTables_ExtraDataAfterTheTablesIsReportedNotDecoded()
    {
        // A real save can carry length-prefixed Lua source after the five
        // tables; the tool leaves it alone but has to say it is there.
        byte[] extra = Encoding.UTF8.GetBytes("return { extra = true }");
        byte[] blob = LuaBlob.SerializeSampleSave().Concat(extra).ToArray();

        RawDataReader.ReadAllTables(blob, out int trailingBytes);

        Assert.Equal(extra.Length, trailingBytes);
    }

    [Fact]
    public void ReadAllTables_TruncatedBlobThrowsEndOfStream()
    {
        byte[] blob = LuaBlob.SerializeSampleSave();

        Assert.Throws<EndOfStreamException>(
            () => RawDataReader.ReadAllTables(blob[..(blob.Length / 2)], out _)
        );
    }

    [Fact]
    public void ReadAllTables_EmptyBlobThrowsEndOfStream()
    {
        Assert.Throws<EndOfStreamException>(
            () => RawDataReader.ReadAllTables(Array.Empty<byte>(), out _)
        );
    }

    [Fact]
    public void ReadAllTables_UnknownTypeCodeThrowsInvalidData()
    {
        // A table holding one hash entry whose key carries the type code 'Q'.
        byte[] blob = { (byte)'T', 0, 0, 0, 0, 1, 0, 0, 0, (byte)'Q' };

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => RawDataReader.ReadAllTables(blob, out _)
        );

        Assert.Contains("'Q'", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ReadAllTables_SomethingThatIsNotSaveDataFailsAsBadData()
    {
        // The wrong file entirely - what a user gets for naming, say, a text
        // file. It has to fail as bad input, not as an unhandled crash.
        byte[] blob = Encoding.UTF8.GetBytes(new string('x', 512));

        Exception? error = Record.Exception(() => RawDataReader.ReadAllTables(blob, out _));

        Assert.True(
            error is InvalidDataException or EndOfStreamException or DecoderFallbackException,
            $"Unexpected failure: {error}"
        );
    }
}
