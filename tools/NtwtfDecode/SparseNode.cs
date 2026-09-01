// SPDX-License-Identifier: MIT
using System.Globalization;
using System.Numerics;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace NtwtfDecode;

/// <summary>An ordered JSON object in the sparse representation.</summary>
/// <remarks>
/// The sparse form is not a Lua table and cannot be one: its property names are
/// as often bookkeeping - a key range, a status name - as they are Lua keys. So it
/// gets its own tree rather than bending <see cref="LuaTable"/>, whose invariants
/// are what the dense form means.
/// </remarks>
public sealed class SparseMap
{
    /// <summary>Properties, in the order they will be written.</summary>
    public List<KeyValuePair<string, object?>> Entries { get; } = new();

    /// <summary>Appends a property.</summary>
    public void Add(string name, object? value) =>
        Entries.Add(new KeyValuePair<string, object?>(name, value));

    /// <summary>The value of a property, or null when it is absent.</summary>
    public object? Find(string name)
    {
        foreach (KeyValuePair<string, object?> entry in Entries)
        {
            if (entry.Key == name)
            {
                return entry.Value;
            }
        }
        return null;
    }

    /// <summary>Whether a property is present.</summary>
    public bool Has(string name) => Find(name) is not null;
}

/// <summary>Reads and writes the sparse tree as UTF-8 JSON.</summary>
public static class SparseJson
{
    /// <summary>Writes a sparse tree, with the same escaping the dense form uses.</summary>
    public static void Write(Stream stream, object? root, int? indent)
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
        WriteValue(writer, root);
        writer.Flush();
    }

    /// <summary>Reads a sparse tree.</summary>
    public static object? Read(Stream stream)
    {
        using JsonDocument json = JsonDocument.Parse(stream);
        return ReadValue(json.RootElement, "root");
    }

    private static void WriteValue(Utf8JsonWriter writer, object? value)
    {
        switch (value)
        {
            case null:
                writer.WriteNullValue();
                break;
            case SparseMap map:
                writer.WriteStartObject();
                foreach (KeyValuePair<string, object?> entry in map.Entries)
                {
                    writer.WritePropertyName(entry.Key);
                    WriteValue(writer, entry.Value);
                }
                writer.WriteEndObject();
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
                throw new InvalidDataException(
                    $"The sparse form holds {number}, which JSON has no number for"
                );
            default:
                throw new InvalidDataException($"Unsupported value type {value.GetType().Name}");
        }
    }

    private static object? ReadValue(JsonElement element, string context) =>
        element.ValueKind switch
        {
            JsonValueKind.Null => null,
            JsonValueKind.String => element.GetString(),
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            JsonValueKind.Number => LuaNumber.Normalize(element.GetDouble()),
            JsonValueKind.Object => ReadMap(element, context),
            _ => throw new InvalidDataException($"Unsupported JSON value at {context}"),
        };

    private static SparseMap ReadMap(JsonElement element, string context)
    {
        var map = new SparseMap();
        foreach (JsonProperty property in element.EnumerateObject())
        {
            map.Add(property.Name, ReadValue(property.Value, $"{context}/{property.Name}"));
        }
        return map;
    }
}
