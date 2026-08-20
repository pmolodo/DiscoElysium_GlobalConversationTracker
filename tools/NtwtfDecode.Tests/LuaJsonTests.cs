using System.Numerics;
using System.Text;
using System.Text.Json;
using UnifiedConversationTracker.Persistence;
using UnifiedConversationTracker.Persistence.Tests;
using Xunit;

namespace NtwtfDecode.Tests;

/// <summary>
/// What the tool's output looks like: the mapping from Lua's value model onto
/// JSON's, which is the part the BCL writer does not decide.
/// </summary>
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
    public void Write_IndentsNestedTables()
    {
        string json = Render(LuaBlob.Table(("Dialog", LuaBlob.Table(("10", "WasOffered")))));

        Assert.Equal(
            "{\n  \"Dialog\": {\n    \"10\": \"WasOffered\"\n  }\n}",
            json,
            ignoreLineEndingDifferences: false
        );
    }

    [Fact]
    public void Write_WithoutAnIndentIsOneLine()
    {
        string json = Render(LuaBlob.Table(("a", 1), ("b", 2)), indent: null);

        Assert.Equal("{\"a\":1,\"b\":2}", json);
    }

    [Fact]
    public void Write_KeysThatAreNotStringsBecomeNames()
    {
        // Lua keys can be any scalar; JSON names can only be strings.
        string json = Render(LuaBlob.Table((7, "a"), (0.5, "b"), (true, "c")), indent: null);

        Assert.Equal("{\"7\":\"a\",\"0.5\":\"b\",\"true\":\"c\"}", json);
    }

    [Fact]
    public void Write_RendersEveryLuaValueType()
    {
        string json = Render(
            LuaBlob.Table(
                ("nil", null),
                ("flag", false),
                ("count", 12),
                ("big", new BigInteger(ulong.MaxValue) * 10),
                ("fraction", 0.5),
                ("text", "quote \" and newline \n")
            ),
            indent: null
        );

        Assert.Equal(
            "{\"nil\":null,\"flag\":false,\"count\":12,\"big\":184467440737095516150,"
                + "\"fraction\":0.5,\"text\":\"quote \\\" and newline \\n\"}",
            json
        );
    }

    [Fact]
    public void Write_NonFiniteNumbersBecomeTheirNames()
    {
        // JSON has no literal for these. The names are the ones System.Text.Json
        // itself reads back under AllowNamedFloatingPointLiterals.
        string json = Render(
            LuaBlob.Table(
                ("nan", double.NaN),
                ("inf", double.PositiveInfinity),
                ("ninf", double.NegativeInfinity)
            ),
            indent: null
        );

        Assert.Equal("{\"nan\":\"NaN\",\"inf\":\"Infinity\",\"ninf\":\"-Infinity\"}", json);
    }

    [Fact]
    public void Write_LeavesNonAsciiTextAlone()
    {
        // A save's text is full of it, and escaping it would only make the dump
        // harder to read. Written as escapes here to keep this file ASCII.
        const string Name = "Ren\u00e9 Arnoux, \u00fcbermensch";

        string json = Render(LuaBlob.Table(("Name", Name)), indent: null);

        Assert.Equal($"{{\"Name\":\"{Name}\"}}", json);
    }

    [Fact]
    public void Write_RejectsAValueTypeTheDecoderNeverProduces()
    {
        Assert.Throws<InvalidDataException>(() => Render(LuaBlob.Table(("when", DateTime.Now))));
    }

    [Fact]
    public void Write_ProducesJsonThatParsesBack()
    {
        LuaTable tables = RawDataParser.ReadAllTables(LuaBlob.SerializeSampleSave(), out _);

        using JsonDocument parsed = JsonDocument.Parse(Render(tables));

        Assert.Equal(
            "WasDisplayed",
            parsed
                .RootElement.GetProperty("Conversation")
                .GetProperty("7")
                .GetProperty("Dialog")
                .GetProperty("10")
                .GetProperty("SimStatus")
                .GetString()
        );
    }
}
