// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using GlobalConversationTracker.Core;
using NtwtfDecode;
using Xunit;

namespace GlobalConversationTracker.Persistence.Tests;

/// <summary>
/// Round trip and failure tests for the save-blob decoder, over blobs written by
/// <see cref="LuaBlob"/>.
/// </summary>
public class RawDataParserTests
{
    /// <summary>The five tables, decoded from a freshly written sample blob.</summary>
    private static LuaTable ReadSample(out int trailingBytes) =>
        LuaTableVisitor.ReadAllTables(LuaBlob.SerializeSampleSave(), out trailingBytes);

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

    /// <summary>
    /// Asserts that two decoded values match entry for entry and in order, so a
    /// whole table can be checked at once. <paramref name="path"/> names where a
    /// mismatch is, which a bare "not equal" on nested tables would not.
    /// </summary>
    private static void AssertSameValue(object? expected, object? actual, string path = "$")
    {
        if (expected is not LuaTable expectedTable)
        {
            Assert.True(
                Equals(expected, actual),
                $"{path}: expected {Describe(expected)}, got {Describe(actual)}."
            );
            return;
        }

        Assert.True(actual is LuaTable, $"{path}: expected a table, got {Describe(actual)}.");
        LuaTable actualTable = (LuaTable)actual!;
        Assert.True(
            expectedTable.Count == actualTable.Count,
            $"{path}: expected {expectedTable.Count} entries, got {actualTable.Count}."
        );
        for (int i = 0; i < expectedTable.Count; i++)
        {
            KeyValuePair<object, object?> expectedEntry = expectedTable.Entries[i];
            KeyValuePair<object, object?> actualEntry = actualTable.Entries[i];
            Assert.True(
                Equals(expectedEntry.Key, actualEntry.Key),
                $"{path}: entry {i} is keyed {Describe(actualEntry.Key)}, expected "
                    + $"{Describe(expectedEntry.Key)}."
            );
            AssertSameValue(expectedEntry.Value, actualEntry.Value, $"{path}.{expectedEntry.Key}");
        }
    }

    /// <summary>A value, with its type, for a mismatch message.</summary>
    private static string Describe(object? value) =>
        value switch
        {
            null => "nil",
            LuaTable table => $"a table of {table.Count}",
            _ => $"{value.GetType().Name} {value}",
        };

    [Fact]
    public void ReadAllTables_RoundTripsAWrittenBlob()
    {
        LuaTable expected = LuaBlob.SampleSave();

        LuaTable actual = LuaTableVisitor.ReadAllTables(
            LuaBlob.Serialize(expected),
            out int trailingBytes
        );

        Assert.Equal(0, trailingBytes);
        AssertSameValue(expected, actual);
    }

    [Fact]
    public void ReadAllTables_ReadsTheFiveTablesInFileOrder()
    {
        LuaTable tables = ReadSample(out _);

        Assert.Equal(
            RawDataParser.TableNames,
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

        LuaTableVisitor.ReadAllTables(blob, out int trailingBytes);

        Assert.Equal(extra.Length, trailingBytes);
    }

    [Fact]
    public void ReadAllTables_TruncatedBlobThrowsEndOfStream()
    {
        byte[] blob = LuaBlob.SerializeSampleSave();

        Assert.Throws<EndOfStreamException>(
            () => LuaTableVisitor.ReadAllTables(blob[..(blob.Length / 2)], out _)
        );
    }

    [Fact]
    public void ReadAllTables_EmptyBlobThrowsEndOfStream()
    {
        Assert.Throws<EndOfStreamException>(
            () => LuaTableVisitor.ReadAllTables(Array.Empty<byte>(), out _)
        );
    }

    [Fact]
    public void ReadAllTables_UnknownTypeCodeThrowsInvalidData()
    {
        // A table holding one hash entry whose key carries the type code 'Q'.
        byte[] blob = { (byte)'T', 0, 0, 0, 0, 1, 0, 0, 0, (byte)'Q' };

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => LuaTableVisitor.ReadAllTables(blob, out _)
        );

        Assert.Contains("'Q'", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ReadAllTables_SomethingThatIsNotSaveDataFailsAsBadData()
    {
        // The wrong file entirely - what a user gets for naming, say, a text
        // file. It has to fail as bad input, not as an unhandled crash.
        byte[] blob = Encoding.UTF8.GetBytes(new string('x', 512));

        Exception? error = Record.Exception(() => LuaTableVisitor.ReadAllTables(blob, out _));

        Assert.True(
            error is InvalidDataException or EndOfStreamException or DecoderFallbackException,
            $"Unexpected failure: {error}"
        );
    }

    /// <summary>
    /// A Conversation table keyed the way a real save keys one: integer conversation
    /// and dialogue ids, not the strings the other fixtures here use.
    /// </summary>
    private static LuaTable IntKeyedConversations() =>
        LuaBlob.Table(
            (
                7,
                LuaBlob.Table(
                    ("Title", "Kim Kitsuragi"),
                    (
                        "Dialog",
                        LuaBlob.Table(
                            (10, LuaBlob.Table(("SimStatus", "WasDisplayed"))),
                            (11, LuaBlob.Table(("SimStatus", "WasOffered")))
                        )
                    )
                )
            )
        );

    /// <summary>The sample save's five tables, with its Conversation table replaced.</summary>
    private static byte[] SerializeSampleSaveWith(LuaTable conversations)
    {
        LuaTable sample = LuaBlob.SampleSave();
        return LuaBlob.Serialize(
            LuaBlob.Table(
                ("Actor", Lookup(sample, "Actor")),
                ("Item", Lookup(sample, "Item")),
                ("Location", Lookup(sample, "Location")),
                ("Variable", Lookup(sample, "Variable")),
                (LuaBlob.ConversationTableName, conversations)
            )
        );
    }

    /// <summary>
    /// A visitor that decodes nothing at all. Every callback on
    /// <see cref="IRawDataVisitor"/> has a do-nothing default, so this is what a
    /// visitor stepping over a table it does not care about reduces to.
    /// </summary>
    private sealed class NoOpVisitor : IRawDataVisitor { }

    /// <summary>Walks a blob with the given visitor and returns what it did not consume.</summary>
    private static int Walk(byte[] blob, IRawDataVisitor visitor)
    {
        var parser = new RawDataParser(blob, visitor);
        parser.Parse();
        return parser.Remaining;
    }

    [Fact]
    public void AVisitorThatReadsNothingLandsWhereTheDecodingOneDoes()
    {
        // The parser owns the cursor, so what a visitor decodes cannot move it: a
        // value's length is knowable only from its own encoding either way.
        byte[] blob = LuaBlob.SerializeSampleSave();

        int afterDecoding = Walk(blob, new LuaTableVisitor());
        int afterReadingNothing = Walk(blob, new NoOpVisitor());

        Assert.Equal(0, afterDecoding);
        Assert.Equal(afterDecoding, afterReadingNothing);
    }

    [Fact]
    public void AVisitorThatReadsNothingStillRejectsAnUnknownTypeCode()
    {
        // The same blob ReadAllTables rejects: a visitor gives up decoding the table,
        // not checking that what it steps over is the format it claims to be.
        byte[] blob = { (byte)'T', 0, 0, 0, 0, 1, 0, 0, 0, (byte)'Q' };

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => Walk(blob, new NoOpVisitor())
        );

        Assert.Contains("'Q'", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void AVisitorThatReadsNothingStillRejectsANilKey()
    {
        // No list part, one hash entry, and both its key and its value are nil.
        byte[] blob = { (byte)'T', 0, 0, 0, 0, 1, 0, 0, 0, (byte)'X', (byte)'X' };

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => Walk(blob, new NoOpVisitor())
        );

        Assert.Contains("nil table key", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void GetSimStatuses_ReadsTheConversationTableOverTheOthers()
    {
        List<SimStatusRow> rows = RawDataParser.GetSimStatuses(
            SerializeSampleSaveWith(IntKeyedConversations())
        );

        Assert.Equal(
            new (int, int, string?)[] { (7, 10, "WasDisplayed"), (7, 11, "WasOffered") },
            rows.Select(row => (row.ConversationId, row.DialogueEntryId, row.StatusName))
        );
    }

    [Fact]
    public void GetSimStatuses_RejectsAConversationWithNoDialogTable()
    {
        LuaTable conversations = LuaBlob.Table((7, LuaBlob.Table(("Title", "Kim Kitsuragi"))));

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => RawDataParser.GetSimStatuses(SerializeSampleSaveWith(conversations))
        );

        Assert.Contains("'Dialog'", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void GetSimStatuses_RejectsADialogueEntryWithNoSimStatus()
    {
        LuaTable conversations = LuaBlob.Table(
            (7, LuaBlob.Table(("Dialog", LuaBlob.Table((10, LuaBlob.Table(("Title", "x")))))))
        );

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => RawDataParser.GetSimStatuses(SerializeSampleSaveWith(conversations))
        );

        Assert.Contains("'SimStatus'", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void GetSimStatuses_RejectsANonIntConversationId()
    {
        LuaTable conversations = LuaBlob.Table(
            ("seven", LuaBlob.Table(("Dialog", LuaBlob.Table())))
        );

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => RawDataParser.GetSimStatuses(SerializeSampleSaveWith(conversations))
        );

        Assert.Contains("conversation ID", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void GetSimStatuses_IgnoresConversationFieldsOtherThanDialog()
    {
        // Title sits beside Dialog, and its own nested tables must not be mistaken
        // for dialogue entries.
        LuaTable conversations = LuaBlob.Table(
            (
                7,
                LuaBlob.Table(
                    ("Fields", LuaBlob.Table((1, LuaBlob.Table(("SimStatus", "NotAnEntry"))))),
                    ("Dialog", LuaBlob.Table((10, LuaBlob.Table(("SimStatus", "WasOffered")))))
                )
            )
        );

        List<SimStatusRow> rows = RawDataParser.GetSimStatuses(
            SerializeSampleSaveWith(conversations)
        );

        Assert.Equal(new (int, int, string?)[] { (7, 10, "WasOffered") },
            rows.Select(row => (row.ConversationId, row.DialogueEntryId, row.StatusName)));
    }

    [Fact]
    public void GetSimStatuses_CountsWhatItWalkedOverBesideTheRowsItFound()
    {
        // A parse time is only interpretable against how much there was to parse, so
        // these counts travel with the rows. A second conversation is more of
        // everything: more tables to open and more values to step over.
        RawDataParser.GetSimStatuses(
            SerializeSampleSaveWith(IntKeyedConversations()),
            out SimStatusParseCounts one
        );
        RawDataParser.GetSimStatuses(
            SerializeSampleSaveWith(
                LuaBlob.Table(
                    (7, LuaBlob.Table(("Dialog", LuaBlob.Table((10, SimStatusEntry()))))),
                    (8, LuaBlob.Table(("Dialog", LuaBlob.Table((10, SimStatusEntry())))))
                )
            ),
            out SimStatusParseCounts two
        );

        Assert.Equal(1, one.ConversationCount);
        Assert.Equal(2, two.ConversationCount);
        // Every table is itself a value, and its keys are values too, so there are
        // always strictly more of the latter.
        Assert.True(one.ValueCount > one.TableCount);
        Assert.True(two.TableCount > one.TableCount);
        Assert.True(two.ValueCount > one.ValueCount);
    }

    [Fact]
    public void GetSimStatuses_ReportsTheExtraDataItNeverInterpretsAsTrailingBytes()
    {
        // The five tables can be followed by length-prefixed Lua source this parser
        // does not read. Its size is what says whether a blob is all table or not.
        byte[] blob = SerializeSampleSaveWith(IntKeyedConversations());

        RawDataParser.GetSimStatuses(blob, out SimStatusParseCounts exact);
        List<SimStatusRow> rows = RawDataParser.GetSimStatuses(
            blob.Concat(new byte[] { 1, 2, 3 }).ToArray(),
            out SimStatusParseCounts withExtra
        );

        Assert.Equal(0, exact.TrailingByteCount);
        Assert.Equal(3, withExtra.TrailingByteCount);
        Assert.Equal(2, rows.Count);
    }

    /// <summary>One dialogue entry, carrying the one field this parser reads.</summary>
    private static LuaTable SimStatusEntry() => LuaBlob.Table(("SimStatus", "WasOffered"));
}
