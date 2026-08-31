// SPDX-License-Identifier: MIT
using System.Text;
using NtwtfDecode;
using GlobalConversationTracker.Persistence;

const string AllTables = "all";
const int DefaultIndent = 2;
const int ExitFailure = 1;

const string Usage = """
    NtwtfDecode - convert Disco Elysium .ntwtf.lua data to and from JSON.

    Usage:
      dotnet run --project tools/NtwtfDecode -- <input> [options]
      dotnet run --project tools/NtwtfDecode -- --to-lua <input.json> -o <output.ntwtf.lua>

    <input> is any of:
      - a packed save archive, {save}.ntwtf.zip, as written to SaveGames
      - a {save}.ntwtf.lua file
      - an expanded .ntwtf save folder containing exactly one such file

    Options:
      -o, --output PATH   Write JSON here instead of stdout.
      -t, --table NAME    Table to dump: Actor, Item, Location, Variable,
                          Conversation, or all. Default: Conversation.
          --indent N      JSON indent width. Default: 2.
          --compact       Single-line JSON (overrides --indent).
          --to-lua        Convert reversible JSON back to a .ntwtf.lua blob.
      -h, --help          Show this message.
    """;

try
{
    return Run(args);
}
catch (Exception ex) when (IsUserError(ex))
{
    // Bad input is the caller's problem, not a defect: say what is wrong and
    // stop. A stack trace here would bury the one line that helps.
    Console.Error.WriteLine($"error: {ex.Message}");
    return ExitFailure;
}
catch (Exception ex)
{
    Console.Error.WriteLine(ex);
    return ExitFailure;
}

int Run(string[] argv)
{
    string? input = null;
    string? output = null;
    string table = "Conversation";
    int? indent = DefaultIndent;
    bool toLua = false;

    for (int i = 0; i < argv.Length; i++)
    {
        string arg = argv[i];
        switch (arg)
        {
            case "-h" or "--help":
                Console.WriteLine(Usage);
                return 0;
            case "-o" or "--output":
                output = NextArg(argv, ref i, arg);
                break;
            case "-t" or "--table":
                table = NextArg(argv, ref i, arg);
                break;
            case "--indent":
                indent = int.Parse(NextArg(argv, ref i, arg));
                break;
            case "--compact":
                indent = null;
                break;
            case "--to-lua":
                toLua = true;
                break;
            default:
                if (arg.StartsWith('-'))
                {
                    throw new ArgumentException($"Unknown option '{arg}'\n\n{Usage}");
                }
                if (input is not null)
                {
                    throw new ArgumentException($"Unexpected extra argument '{arg}'\n\n{Usage}");
                }
                input = arg;
                break;
        }
    }

    if (input is null)
    {
        throw new ArgumentException($"No input file given\n\n{Usage}");
    }

    if (toLua)
    {
        using FileStream json = File.OpenRead(input);
        LuaTable document = LuaJson.ReadDocument(json);
        using Stream lua = output is null ? Console.OpenStandardOutput() : File.Create(output);
        LuaBinary.WriteDocument(lua, document);
        lua.Flush();
        return 0;
    }

    LuaTable allTables = ReadTables(SaveBlob.Read(input), input, out int trailing);
    if (trailing > 0)
    {
        // PersistentDataManager.ApplyExtraData reads length-prefixed Lua source
        // after the five tables; this tool does not interpret it.
        Console.Error.WriteLine(
            $"note: {trailing} trailing byte(s) of extra data after the five tables were not decoded"
        );
    }

    object? selected;
    if (string.Equals(table, AllTables, StringComparison.OrdinalIgnoreCase))
    {
        selected = allTables;
    }
    else if (allTables.TryGetValue(table, out object? value))
    {
        selected = value;
    }
    else
    {
        throw new ArgumentException(
            $"Unknown table '{table}'; expected one of "
                + $"{string.Join(", ", RawDataParser.TableNames)}, {AllTables}"
        );
    }

    using Stream stream = output is null ? Console.OpenStandardOutput() : File.Create(output);
    LuaJson.Write(stream, selected, indent);
    stream.WriteByte((byte)'\n');
    stream.Flush();
    return 0;
}

static string NextArg(string[] argv, ref int i, string option)
{
    i++;
    if (i >= argv.Length)
    {
        throw new ArgumentException($"Option '{option}' requires a value");
    }
    return argv[i];
}

/// <summary>
/// Decodes the five tables, restating a format failure in terms of the input the
/// caller actually named. Something that is not save data at all - the wrong
/// file, a truncated copy - otherwise surfaces as a bare offset deep inside the
/// reader, which says nothing about which argument was wrong.
/// </summary>
static LuaTable ReadTables(byte[] blob, string input, out int trailing)
{
    try
    {
        return LuaTableVisitor.ReadAllTables(blob, out trailing);
    }
    catch (Exception ex)
        when (ex is InvalidDataException or EndOfStreamException or DecoderFallbackException)
    {
        throw new InvalidDataException(
            $"'{input}' does not hold decodable {SaveBlob.LuaExtension} save data: "
                + $"{ex.Message}. {SaveBlob.AcceptedInputs}",
            ex
        );
    }
}

/// <summary>
/// Whether an exception describes bad input rather than a bug in this tool.
/// </summary>
static bool IsUserError(Exception ex) =>
    ex is ArgumentException or IOException or InvalidDataException or UnauthorizedAccessException;
