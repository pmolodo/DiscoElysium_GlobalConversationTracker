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
    private const string ListCountName = "_num_list_entries";
    private const string TrailingBytesName = "_trailing_bytes_base64";

    /// <summary>
    /// Names which representation a file is written in. A split file carries it so
    /// that neither a reader nor a person has to infer the form from its contents.
    /// </summary>
    public const string FormatName = "_format";

    /// <summary>
    /// The table path assumed when a caller does not name one: a lone table dumped
    /// for inspection is not at any known path, so it gets the manifest's default.
    /// </summary>
    private const string DefaultTablePath = "table";

    /// <summary>Writes a Lua value as UTF-8 JSON.</summary>
    /// <param name="format">
    /// When given, the representation to record as a leading <see cref="FormatName"/>
    /// property. Only a whole table can carry one.
    /// </param>
    public static void Write(
        Stream stream,
        object? value,
        int? indent,
        string tablePath = DefaultTablePath,
        string? format = null
    )
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
        else if (format is null)
        {
            WriteValue(writer, value, tablePath);
        }
        else if (value is LuaTable table)
        {
            WriteTable(writer, table, tablePath, format);
        }
        else
        {
            throw new InvalidDataException($"Only a table can record a '{FormatName}'");
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

    /// <summary>Reads one table from its reversible JSON representation.</summary>
    public static LuaTable ReadTable(Stream stream, string tablePath = DefaultTablePath)
    {
        using JsonDocument json = JsonDocument.Parse(stream);
        return ReadTable(json.RootElement, tablePath);
    }

    /// <summary>
    /// The representation a JSON object records for itself, or null when it records
    /// none. Reads only the leading property, so it costs nothing on a large file.
    /// </summary>
    public static string? FormatOf(Stream stream)
    {
        var reader = new Utf8JsonReader(ReadLeadingBytes(stream));
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartObject)
        {
            return null;
        }
        if (!reader.Read() || reader.TokenType != JsonTokenType.PropertyName)
        {
            return null;
        }
        if (reader.GetString() != FormatName || !reader.Read())
        {
            return null;
        }
        return reader.TokenType == JsonTokenType.String ? reader.GetString() : null;
    }

    private static byte[] ReadLeadingBytes(Stream stream)
    {
        // Enough for the opening brace, the property name and a short value.
        var buffer = new byte[128];
        int read = stream.Read(buffer, 0, buffer.Length);
        stream.Position = 0;
        return buffer[..read];
    }

    private static void WriteDocument(Utf8JsonWriter writer, LuaTable root)
    {
        writer.WriteStartObject();
        writer.WriteString(TrailingBytesName, Convert.ToBase64String(root.TrailingBytes));
        foreach (KeyValuePair<object, object?> entry in root.Entries)
        {
            writer.WritePropertyName((string)entry.Key);
            WriteValue(writer, entry.Value, (string)entry.Key);
        }
        writer.WriteEndObject();
    }

    private static void WriteValue(Utf8JsonWriter writer, object? value, string tablePath)
    {
        switch (value)
        {
            case null:
                writer.WriteNullValue();
                break;
            case LuaTable table:
                WriteTable(writer, table, tablePath);
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
                // NaN and the infinities have no JSON literal. Nothing in a save
                // holds one, so saying so beats inventing a spelling for them.
                throw new InvalidDataException(
                    $"Table '{tablePath}' holds {number}, which JSON has no number for"
                );
            default:
                throw new InvalidDataException($"Unsupported value type {value.GetType().Name}");
        }
    }

    private static void WriteTable(
        Utf8JsonWriter writer,
        LuaTable table,
        string path,
        string? format = null
    )
    {
        if (table.NumListEntries < 0 || table.NumListEntries > table.Count)
        {
            throw new InvalidDataException(
                $"Table list count {table.NumListEntries} is outside 0..{table.Count}"
            );
        }

        string[] names = PropertyNames(table, path);
        writer.WriteStartObject();
        if (format is not null)
        {
            writer.WriteString(FormatName, format);
        }
        if (table.NumListEntries > 0)
        {
            // Most tables are pure dictionaries; saying so on every one of them is
            // noise, so an absent boundary means zero.
            writer.WriteNumber(ListCountName, table.NumListEntries);
        }
        for (int i = 0; i < names.Length; i++)
        {
            writer.WritePropertyName(names[i]);
            WriteValue(writer, table.Entries[i].Value, AppendPath(path, names[i]));
        }
        writer.WriteEndObject();
    }

    /// <summary>
    /// The JSON property name of every entry, in order: its 1-based index for the
    /// list part, and its key's text for the dictionary part. Rejects up front
    /// anything the reader could not turn back into this table - a list key that is
    /// not its own index, a dictionary key whose type contradicts
    /// <see cref="LuaKeyTypeManifest"/>, and any name the object cannot carry.
    /// </summary>
    private static string[] PropertyNames(LuaTable table, string path)
    {
        LuaDictionaryKeyType expected = LuaKeyTypeManifest.ExpectedType(path);
        var names = new string[table.Count];
        var used = new HashSet<string>(StringComparer.Ordinal);
        for (int i = 0; i < names.Length; i++)
        {
            object key = table.Entries[i].Key;
            string name = LuaKey.ToKeyString(key);
            if (i < table.NumListEntries)
            {
                string index = (i + 1).ToString(CultureInfo.InvariantCulture);
                if (name != index)
                {
                    throw new InvalidDataException(
                        $"Table '{path}' list entry {index} is keyed '{name}'"
                    );
                }
            }
            else if (KeyType(key) != expected)
            {
                throw new InvalidDataException(
                    $"Table '{path}' expects {Describe(expected)} dictionary keys, "
                        + $"but '{name}' is {Describe(KeyType(key))}"
                );
            }
            ClaimName(used, name, path);
            names[i] = name;
        }
        return names;
    }

    /// <summary>
    /// Takes a property name for one entry of a table, failing if the object cannot
    /// carry it. Both directions use this, so what the writer refuses to produce is
    /// what the reader refuses to accept.
    /// </summary>
    private static void ClaimName(HashSet<string> used, string name, string path)
    {
        if (name == ListCountName || name == FormatName)
        {
            // Either would be read back as this object's own bookkeeping.
            throw new InvalidDataException(
                $"Table '{path}' cannot name an entry '{name}'; that name is reserved"
            );
        }
        if (!used.Add(name))
        {
            throw new InvalidDataException($"Table '{path}' has two '{name}' properties");
        }
    }

    private static LuaDictionaryKeyType KeyType(object key) =>
        key switch
        {
            string => LuaDictionaryKeyType.String,
            int or long or BigInteger or double => LuaDictionaryKeyType.Number,
            bool => LuaDictionaryKeyType.Boolean,
            _ => throw new InvalidDataException(
                $"Unsupported dictionary key type {key.GetType().Name}"
            ),
        };

    private static string Describe(LuaDictionaryKeyType type) =>
        type.ToString().ToLowerInvariant();

    private static LuaTable ReadTable(JsonElement element, string context)
    {
        RequireKind(element, JsonValueKind.Object, context);
        JsonElement.ObjectEnumerator properties = element.EnumerateObject();

        bool more = properties.MoveNext();
        if (more && properties.Current.Name == FormatName)
        {
            more = properties.MoveNext();
        }

        // The list boundary leads the object when there is one; without it the
        // table is all dictionary, which most of them are.
        int listCount = 0;
        if (more && properties.Current.Name == ListCountName)
        {
            listCount = ReadListCount(properties.Current.Value, context);
            more = properties.MoveNext();
        }
        LuaDictionaryKeyType keyType = LuaKeyTypeManifest.ExpectedType(context);

        var table = new LuaTable { NumListEntries = listCount };
        var used = new HashSet<string>(StringComparer.Ordinal);
        int entryIndex = 0;
        for (; more; more = properties.MoveNext())
        {
            JsonProperty property = properties.Current;
            entryIndex++;
            object key;
            if (entryIndex <= listCount)
            {
                string expected = entryIndex.ToString(CultureInfo.InvariantCulture);
                if (property.Name != expected)
                {
                    throw new InvalidDataException(
                        $"{context} list entry {entryIndex} must be named '{expected}', "
                            + $"not '{property.Name}'"
                    );
                }
                key = entryIndex;
            }
            else
            {
                key = ParseDictionaryKey(property.Name, keyType, context);
            }
            ClaimName(used, property.Name, context);
            string childPath = AppendPath(context, property.Name);
            table.Add(key, ReadValue(property.Value, childPath));
        }
        if (entryIndex < listCount)
        {
            throw new InvalidDataException(
                $"{context}.{ListCountName} is {listCount}, but the table has only "
                    + $"{entryIndex} entries"
            );
        }
        return table;
    }

    private static int ReadListCount(JsonElement element, string context)
    {
        if (
            element.ValueKind != JsonValueKind.Number
            || !element.TryGetInt32(out int count)
            || count < 0
        )
        {
            throw new InvalidDataException(
                $"{context}.{ListCountName} must be a non-negative whole number"
            );
        }
        return count;
    }

    /// <summary>
    /// The Lua key a JSON property name stands for, given the type
    /// <see cref="LuaKeyTypeManifest"/> expects there. Shared with
    /// <see cref="LuaSparse"/> so both forms read a key the same way.
    /// </summary>
    public static object ParseDictionaryKey(
        string text,
        LuaDictionaryKeyType keyType,
        string context
    )
    {
        object key = keyType switch
        {
            LuaDictionaryKeyType.String => text,
            LuaDictionaryKeyType.Number when double.TryParse(
                text,
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out double number
            ) => LuaNumber.Normalize(number),
            LuaDictionaryKeyType.Boolean when bool.TryParse(text, out bool flag) => flag,
            _ => throw new InvalidDataException(
                $"{context} dictionary key '{text}' is not a valid {Describe(keyType)}"
            ),
        };

        // Parsing is looser than the writer's rendering - " 10 " and "TRUE" both
        // parse - so insist on the one spelling the writer would produce, or an
        // edited key would come back changed on the next round trip.
        string canonical = LuaKey.ToKeyString(key);
        if (canonical != text)
        {
            throw new InvalidDataException(
                $"{context} dictionary key '{text}' must be spelled '{canonical}'"
            );
        }
        return key;
    }

    private static string AppendPath(string path, string key) => path + "/" + key;

    private static object? ReadValue(JsonElement element, string context) =>
        element.ValueKind switch
        {
            JsonValueKind.Null => null,
            JsonValueKind.String => element.GetString(),
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            JsonValueKind.Number => LuaNumber.Normalize(element.GetDouble()),
            JsonValueKind.Object => ReadTable(element, context),
            _ => throw new InvalidDataException($"Unsupported JSON value at {context}"),
        };

    private static void RequireKind(JsonElement element, JsonValueKind kind, string context)
    {
        if (element.ValueKind != kind)
        {
            throw new InvalidDataException($"{context} must be a JSON {kind}");
        }
    }
}
