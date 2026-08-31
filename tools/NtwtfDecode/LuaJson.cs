// SPDX-License-Identifier: MIT
using System.Globalization;
using System.Numerics;
using System.Text.Encodings.Web;
using System.Text.Json;
using GlobalConversationTracker.Persistence;

namespace NtwtfDecode;

/// <summary>Reads and writes the reversible JSON representation of a save blob.</summary>
public static class LuaJson
{
    private const string DictName = "_dict";
    private const string KeyName = "key";
    private const string ListName = "_list";
    private const string ListCountName = "_num_list_entries";
    private const string NumberBitsName = "_number_bits";
    private const string TrailingBytesName = "_trailing_bytes_base64";
    private const string ValueName = "value";

    /// <summary>Writes a Lua value as UTF-8 JSON.</summary>
    public static void Write(Stream stream, object? value, int? indent)
    {
        var options = new JsonWriterOptions
        {
            Indented = indent is not null,
            Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
            NewLine = "\n",
        };
        if (indent is int width)
        {
            options.IndentSize = width;
        }

        using var writer = new Utf8JsonWriter(stream, options);
        if (value is LuaTable { IsDocumentRoot: true } root)
        {
            WriteDocument(writer, root);
        }
        else
        {
            WriteValue(writer, value);
        }
        writer.Flush();
    }

    /// <summary>Reads a complete five-table document from reversible JSON.</summary>
    public static LuaTable ReadDocument(Stream stream)
    {
        using JsonDocument json = JsonDocument.Parse(stream);
        JsonElement rootElement = json.RootElement;
        RequireKind(rootElement, JsonValueKind.Object, "document root");

        var root = new LuaTable { IsDocumentRoot = true };
        foreach (string name in RawDataParser.TableNames)
        {
            if (!rootElement.TryGetProperty(name, out JsonElement table))
            {
                throw new InvalidDataException($"JSON document is missing the '{name}' table");
            }
            root.Add(name, ReadTable(table, name));
        }

        if (rootElement.TryGetProperty(TrailingBytesName, out JsonElement trailing))
        {
            try
            {
                root.TrailingBytes = Convert.FromBase64String(trailing.GetString() ?? string.Empty);
            }
            catch (FormatException ex)
            {
                throw new InvalidDataException($"{TrailingBytesName} is not valid base64", ex);
            }
        }
        return root;
    }

    private static void WriteDocument(Utf8JsonWriter writer, LuaTable root)
    {
        writer.WriteStartObject();
        writer.WriteString(TrailingBytesName, Convert.ToBase64String(root.TrailingBytes));
        foreach (KeyValuePair<object, object?> entry in root.Entries)
        {
            writer.WritePropertyName((string)entry.Key);
            WriteValue(writer, entry.Value);
        }
        writer.WriteEndObject();
    }

    private static void WriteValue(Utf8JsonWriter writer, object? value)
    {
        switch (value)
        {
            case null:
                writer.WriteNullValue();
                break;
            case LuaTable table:
                WriteTable(writer, table);
                break;
            case string text:
                writer.WriteStringValue(text);
                break;
            case bool flag:
                writer.WriteBooleanValue(flag);
                break;
            case int i:
                writer.WriteNumberValue(i);
                break;
            case long l:
                writer.WriteNumberValue(l);
                break;
            case BigInteger big:
                writer.WriteRawValue(big.ToString(CultureInfo.InvariantCulture));
                break;
            case double number when double.IsFinite(number):
                writer.WriteNumberValue(number);
                break;
            case double number:
                writer.WriteStartObject();
                writer.WriteString(
                    NumberBitsName,
                    BitConverter.DoubleToInt64Bits(number).ToString("X16", CultureInfo.InvariantCulture)
                );
                writer.WriteEndObject();
                break;
            default:
                throw new InvalidDataException($"Unsupported value type {value.GetType().Name}");
        }
    }

    private static void WriteTable(Utf8JsonWriter writer, LuaTable table)
    {
        if (table.NumListEntries < 0 || table.NumListEntries > table.Count)
        {
            throw new InvalidDataException(
                $"Table list count {table.NumListEntries} is outside 0..{table.Count}"
            );
        }

        writer.WriteStartObject();
        writer.WriteNumber(ListCountName, table.NumListEntries);
        writer.WriteStartArray(ListName);
        for (int i = 0; i < table.NumListEntries; i++)
        {
            WriteValue(writer, table.Entries[i].Value);
        }
        writer.WriteEndArray();
        writer.WriteStartArray(DictName);
        for (int i = table.NumListEntries; i < table.Count; i++)
        {
            writer.WriteStartObject();
            writer.WritePropertyName(KeyName);
            WriteValue(writer, table.Entries[i].Key);
            writer.WritePropertyName(ValueName);
            WriteValue(writer, table.Entries[i].Value);
            writer.WriteEndObject();
        }
        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    private static LuaTable ReadTable(JsonElement element, string context)
    {
        RequireKind(element, JsonValueKind.Object, context);
        int listCount = RequiredProperty(element, ListCountName, context).GetInt32();
        JsonElement list = RequiredProperty(element, ListName, context);
        JsonElement dict = RequiredProperty(element, DictName, context);
        RequireKind(list, JsonValueKind.Array, $"{context}.{ListName}");
        RequireKind(dict, JsonValueKind.Array, $"{context}.{DictName}");
        if (list.GetArrayLength() != listCount)
        {
            throw new InvalidDataException(
                $"{context}.{ListCountName} is {listCount}, but {ListName} has "
                    + $"{list.GetArrayLength()} entries"
            );
        }

        var table = new LuaTable { NumListEntries = listCount };
        int index = 1;
        foreach (JsonElement item in list.EnumerateArray())
        {
            table.Add(index++, ReadValue(item, $"{context}.{ListName}"));
        }
        foreach (JsonElement pair in dict.EnumerateArray())
        {
            RequireKind(pair, JsonValueKind.Object, $"{context}.{DictName} entry");
            object? key = ReadValue(RequiredProperty(pair, KeyName, context), $"{context}.key");
            if (key is null or LuaTable)
            {
                throw new InvalidDataException($"{context} has an invalid table key");
            }
            object? value = ReadValue(
                RequiredProperty(pair, ValueName, context),
                $"{context}.value"
            );
            table.Add(key, value);
        }
        return table;
    }

    private static object? ReadValue(JsonElement element, string context) =>
        element.ValueKind switch
        {
            JsonValueKind.Null => null,
            JsonValueKind.String => element.GetString(),
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            JsonValueKind.Number => element.GetDouble(),
            JsonValueKind.Object when element.TryGetProperty(NumberBitsName, out JsonElement bits) =>
                ReadNumberBits(bits, context),
            JsonValueKind.Object => ReadTable(element, context),
            _ => throw new InvalidDataException($"Unsupported JSON value at {context}"),
        };

    private static double ReadNumberBits(JsonElement element, string context)
    {
        string text = element.GetString() ?? string.Empty;
        if (!long.TryParse(text, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out long bits))
        {
            throw new InvalidDataException($"Invalid {NumberBitsName} at {context}: '{text}'");
        }
        return BitConverter.Int64BitsToDouble(bits);
    }

    private static JsonElement RequiredProperty(JsonElement element, string name, string context)
    {
        if (!element.TryGetProperty(name, out JsonElement value))
        {
            throw new InvalidDataException($"{context} is missing '{name}'");
        }
        return value;
    }

    private static void RequireKind(JsonElement element, JsonValueKind kind, string context)
    {
        if (element.ValueKind != kind)
        {
            throw new InvalidDataException($"{context} must be a JSON {kind}");
        }
    }
}
