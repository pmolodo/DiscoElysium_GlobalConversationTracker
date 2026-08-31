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

    private static string Render(object? value, int? indent = Indent)
    {
        using var buffer = new MemoryStream();
        LuaJson.Write(buffer, value, indent);
        return Encoding.UTF8.GetString(buffer.ToArray());
    }

    [Fact]
    public void Write_RecordsTheListBoundaryAndTypedDictionaryEntries()
    {
        LuaTable table = LuaBlob.List("a", "b");
        table.Add(0.5, "number key");
        table.Add("0.5", "string key");

        using JsonDocument json = JsonDocument.Parse(Render(table));
        JsonElement root = json.RootElement;

        Assert.Equal(2, root.GetProperty("_num_list_entries").GetInt32());
        Assert.Equal(2, root.GetProperty("_list").GetArrayLength());
        JsonElement dict = root.GetProperty("_dict");
        Assert.Equal(JsonValueKind.Number, dict[0].GetProperty("key").ValueKind);
        Assert.Equal(JsonValueKind.String, dict[1].GetProperty("key").ValueKind);
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
    public void Write_StillSupportsArbitrarilyLargeNumbersForDiagnosticOutput()
    {
        string json = Render(
            LuaBlob.Table(("big", new BigInteger(ulong.MaxValue) * 10)),
            indent: null
        );

        Assert.Contains("184467440737095516150", json, StringComparison.Ordinal);
    }

    [Fact]
    public void ReadDocument_RejectsAMismatchedListCount()
    {
        const string Json = """
            {
              "Actor": {"_num_list_entries": 1, "_list": [], "_dict": []},
              "Item": {"_num_list_entries": 0, "_list": [], "_dict": []},
              "Location": {"_num_list_entries": 0, "_list": [], "_dict": []},
              "Variable": {"_num_list_entries": 0, "_list": [], "_dict": []},
              "Conversation": {"_num_list_entries": 0, "_list": [], "_dict": []}
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
        var edgeCases = new LuaTable();
        edgeCases.Add(1.0, "numeric key");
        edgeCases.Add("1", "string key");
        edgeCases.Add("negative zero", BitConverter.Int64BitsToDouble(unchecked((long)0x8000000000000000)));
        edgeCases.Add("nan payload", BitConverter.Int64BitsToDouble(unchecked((long)0xFFF8000000000042)));
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
}
