// SPDX-License-Identifier: MIT
using System.Text;
using GlobalConversationTracker.Persistence;

namespace NtwtfDecode;

/// <summary>Writes the binary raw-data format stored in a .ntwtf.lua file.</summary>
public static class LuaBinary
{
    private const byte BooleanCode = (byte)'B';
    private const byte NilCode = (byte)'X';
    private const byte NumberCode = (byte)'N';
    private const byte StringCode = (byte)'S';
    private const byte TableMarker = (byte)'T';

    /// <summary>Writes all five tables and the document's preserved trailing bytes.</summary>
    public static void WriteDocument(Stream stream, LuaTable root)
    {
        using var writer = new BinaryWriter(stream, new UTF8Encoding(false), leaveOpen: true);
        foreach (string name in RawDataParser.TableNames)
        {
            if (!root.TryGetValue(name, out object? value) || value is not LuaTable table)
            {
                throw new InvalidDataException($"Document is missing the '{name}' table");
            }
            WriteTable(writer, table);
        }
        writer.Write(root.TrailingBytes);
    }

    private static void WriteTable(BinaryWriter writer, LuaTable table)
    {
        if (table.NumListEntries < 0 || table.NumListEntries > table.Count)
        {
            throw new InvalidDataException(
                $"Table list count {table.NumListEntries} is outside 0..{table.Count}"
            );
        }

        writer.Write(TableMarker);
        writer.Write(table.NumListEntries);
        for (int i = 0; i < table.NumListEntries; i++)
        {
            WriteValue(writer, table.Entries[i].Value);
        }
        writer.Write(table.Count - table.NumListEntries);
        for (int i = table.NumListEntries; i < table.Count; i++)
        {
            WriteValue(writer, table.Entries[i].Key);
            WriteValue(writer, table.Entries[i].Value);
        }
    }

    private static void WriteValue(BinaryWriter writer, object? value)
    {
        switch (value)
        {
            case null:
                writer.Write(NilCode);
                break;
            case LuaTable table:
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
            case int or long or double:
                writer.Write(NumberCode);
                writer.Write(Convert.ToDouble(value));
                break;
            default:
                throw new InvalidDataException(
                    $"Cannot write Lua value of type {value.GetType().Name}"
                );
        }
    }
}
