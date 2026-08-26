// SPDX-License-Identifier: MIT
using System.Globalization;
using System.Numerics;
using System.Text.Encodings.Web;
using System.Text.Json;
using GlobalConversationTracker.Persistence;

namespace NtwtfDecode;

/// <summary>
/// Renders a decoded Lua value as JSON with <see cref="Utf8JsonWriter"/>.
/// </summary>
/// <remarks>
/// The BCL writer owns every formatting decision - indentation, string
/// escaping, shortest round-tripping numbers - so all this adds is the mapping
/// from Lua's value model onto JSON's.
/// </remarks>
public static class LuaJson
{
    /// <summary>
    /// Writes <paramref name="value"/> as UTF-8 JSON.
    /// </summary>
    /// <param name="indent">
    /// Spaces per level, or null for a single line.
    /// </param>
    public static void Write(Stream stream, object? value, int? indent)
    {
        var options = new JsonWriterOptions
        {
            Indented = indent is not null,
            // Relaxed escaping leaves non-ASCII text and quotes readable; the
            // default encoder is conservative because its output may land in
            // HTML, which this never does.
            Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
            // The writer would otherwise break lines with Environment.NewLine,
            // making a dump differ by the machine it was taken on.
            NewLine = "\n",
        };
        if (indent is int width)
        {
            options.IndentSize = width;
        }

        using var writer = new Utf8JsonWriter(stream, options);
        WriteValue(writer, value);
        writer.Flush();
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
            // Too large for long, and the writer has no overload for it, so the
            // decimal digits go out as-is; they are already valid JSON.
            case BigInteger big:
                writer.WriteRawValue(big.ToString(CultureInfo.InvariantCulture));
                break;
            case double number:
                WriteDouble(writer, number);
                break;
            default:
                throw new InvalidDataException($"Unsupported value type {value.GetType().Name}");
        }
    }

    private static void WriteTable(Utf8JsonWriter writer, LuaTable table)
    {
        writer.WriteStartObject();
        foreach (var entry in table.Entries)
        {
            // Lua keys can be numbers or booleans; JSON names cannot, so they
            // are stringified. A table holding both 1 and "1" would collide, but
            // nothing in a save does.
            writer.WritePropertyName(LuaKey.ToKeyString(entry.Key));
            WriteValue(writer, entry.Value);
        }
        writer.WriteEndObject();
    }

    /// <summary>
    /// JSON has no literal for the non-finite doubles, so they go out as the
    /// strings "NaN", "Infinity" and "-Infinity" - the same spelling
    /// System.Text.Json reads back under AllowNamedFloatingPointLiterals.
    /// </summary>
    private static void WriteDouble(Utf8JsonWriter writer, double value)
    {
        if (double.IsFinite(value))
        {
            writer.WriteNumberValue(value);
        }
        else
        {
            writer.WriteStringValue(value.ToString(CultureInfo.InvariantCulture));
        }
    }
}
