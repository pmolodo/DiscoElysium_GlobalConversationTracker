// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using NtwtfDecode;

namespace GlobalConversationTracker.Persistence.Tests;

/// <summary>
/// Writes the binary "raw data" blob that <see cref="RawDataParser"/> reads, so
/// the tests can synthesise save data instead of needing a real save.
/// </summary>
/// <remarks>
/// Deliberately written with <see cref="BinaryWriter"/>, mirroring the
/// PixelCrushers side that produces the format: the type markers, the
/// little-endian Int32/Double, and the 7-bit length prefix on strings all come
/// from BinaryWriter rather than from this test's idea of the layout, so a
/// round trip actually pins <see cref="RawDataParser"/> to BinaryReader
/// semantics.
/// </remarks>
internal static class LuaBlob
{
    /// <summary>The one table in a save this repo's tools actually read.</summary>
    public const string ConversationTableName = "Conversation";

    private const byte TableMarker = (byte)'T';
    private const byte StringCode = (byte)'S';
    private const byte NumberCode = (byte)'N';
    private const byte BooleanCode = (byte)'B';
    private const byte NilCode = (byte)'X';

    /// <summary>Builds a table whose entries are written as the hash part, in order.</summary>
    public static LuaTable Table(params (object Key, object? Value)[] entries)
    {
        var table = new LuaTable();
        foreach ((object key, object? value) in entries)
        {
            table.Add(key, value);
        }
        return table;
    }

    /// <summary>Builds a table whose entries are written as the 1-indexed list part.</summary>
    public static LuaTable List(params object?[] items)
    {
        var table = new LuaTable();
        for (int i = 0; i < items.Length; i++)
        {
            table.Add(i + 1, items[i]);
        }
        return table;
    }

    /// <summary>
    /// A save blob's worth of content: the five top-level tables keyed by name,
    /// in file order, shaped the way a real save is - including a Conversation
    /// table with a Dialog map of SimStatus entries.
    /// </summary>
    public static LuaTable SampleSave() =>
        Table(
            ("Actor", Table(("Kim", Table(("Name", "Kim Kitsuragi"), ("IsPlayer", false))))),
            ("Item", Table(("42", Table(("Name", "Necktie"), ("Is_Item", true))))),
            ("Location", new LuaTable()),
            (
                "Variable",
                Table(
                    ("Money", 12),
                    ("Health", 0.5),
                    ("Nickname", "the Bird's Nest"),
                    ("Unset", null),
                    ("Flags", List("a", "b", 3))
                )
            ),
            (
                ConversationTableName,
                Table(
                    (
                        "7",
                        Table(
                            ("Title", "Kim Kitsuragi"),
                            (
                                "Dialog",
                                Table(
                                    ("10", Table(("SimStatus", "WasDisplayed"))),
                                    ("11", Table(("SimStatus", "WasOffered"))),
                                    ("12", Table(("SimStatus", "Untouched")))
                                )
                            )
                        )
                    )
                )
            )
        );

    /// <summary>
    /// Serialises the five top-level tables, keyed by name, in the order
    /// <see cref="RawDataParser.TableNames"/> gives.
    /// </summary>
    public static byte[] Serialize(LuaTable tablesByName)
    {
        using var buffer = new MemoryStream();
        using (var writer = new BinaryWriter(buffer, new UTF8Encoding(false), leaveOpen: true))
        {
            foreach (string name in RawDataParser.TableNames)
            {
                if (
                    !tablesByName.TryGetValue(name, out object? value)
                    || value is not LuaTable table
                )
                {
                    throw new ArgumentException(
                        $"Blob content is missing the '{name}' table.",
                        nameof(tablesByName)
                    );
                }
                WriteTable(writer, table);
            }
        }
        return buffer.ToArray();
    }

    /// <summary>The bytes of <see cref="SampleSave"/>.</summary>
    public static byte[] SerializeSampleSave() => Serialize(SampleSave());

    /// <summary>
    /// The bytes of a save whose Conversation table is the given one and whose
    /// other four top-level tables are empty, so a test that is only about the
    /// Conversation table can vary that table and nothing else.
    /// </summary>
    public static byte[] SerializeConversations(LuaTable conversations)
    {
        var tablesByName = new LuaTable();
        foreach (string name in RawDataParser.TableNames)
        {
            tablesByName.Add(
                name,
                name == ConversationTableName ? conversations : new LuaTable()
            );
        }
        return Serialize(tablesByName);
    }

    private static void WriteTable(BinaryWriter writer, LuaTable table)
    {
        IReadOnlyList<KeyValuePair<object, object?>> entries = table.Entries;

        // The leading run of keys 1, 2, 3, ... is the array part; everything
        // after it is the hash part, which is how the game's writer splits them.
        int listCount = 0;
        while (listCount < entries.Count && IsListKey(entries[listCount].Key, listCount + 1))
        {
            listCount++;
        }

        writer.Write(TableMarker);
        writer.Write(listCount);
        for (int i = 0; i < listCount; i++)
        {
            WriteValue(writer, entries[i].Value);
        }

        writer.Write(entries.Count - listCount);
        for (int i = listCount; i < entries.Count; i++)
        {
            WriteValue(writer, entries[i].Key);
            WriteValue(writer, entries[i].Value);
        }
    }

    /// <summary>
    /// True when a key is the integer <paramref name="expected"/>, as either an int or a
    /// long - see the note on the number cases in <see cref="WriteValue"/>.
    /// </summary>
    private static bool IsListKey(object key, int expected) =>
        key switch
        {
            int i => i == expected,
            long l => l == expected,
            _ => false,
        };

    private static void WriteValue(BinaryWriter writer, object? value)
    {
        switch (value)
        {
            case null:
                writer.Write(NilCode);
                break;
            case LuaTable table:
                // A table is its own marker: there is no separate type code.
                WriteTable(writer, table);
                break;
            case string text:
                writer.Write(StringCode);
                writer.Write(text);
                break;
            case bool flag:
                writer.Write(BooleanCode);
                writer.Write(flag);
                break;
            // Every Lua number goes out as a double, so int and long are the same
            // thing on the wire; a fixture may be written either way.
            case int or long:
                writer.Write(NumberCode);
                writer.Write(Convert.ToDouble(value));
                break;
            case double number:
                writer.Write(NumberCode);
                writer.Write(number);
                break;
            default:
                throw new ArgumentException(
                    $"Cannot write a Lua value of type {value.GetType().Name}.",
                    nameof(value)
                );
        }
    }
}
