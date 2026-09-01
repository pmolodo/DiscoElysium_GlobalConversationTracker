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
      dotnet run --project tools/NtwtfDecode -- <save.ntwtf.zip> --split -o <expanded.ntwtf>
      dotnet run --project tools/NtwtfDecode -- <input> --split --sparse --base <expanded.ntwtf> -o <directory>
      dotnet run --project tools/NtwtfDecode -- --to-lua <directory> --split -o <output.ntwtf.lua>
      dotnet run --project tools/NtwtfDecode -- --pack <expanded.ntwtf> -o <save.ntwtf.zip>

    <input> is any of:
      - a packed save archive, {save}.ntwtf.zip, as written to SaveGames
      - a {save}.ntwtf.lua file
      - an expanded .ntwtf save folder containing exactly one such file

    Packed input with --split writes a complete expanded save: companion archive
    members stay at top level and the Lua split goes in <name>.ntwtf.lua.parts.

    Options:
      -o, --output PATH   Write output here instead of stdout. With --split
                          conversion to JSON, this is the output directory.
      -t, --table NAME    Table to dump: Actor, Item, Location, Variable,
                          Conversation, or all. Default: Conversation.
          --indent N      JSON indent width. Default: 2.
          --compact       Single-line JSON (overrides --indent).
          --to-lua        Convert reversible JSON back to a .ntwtf.lua blob.
          --pack          Rebuild and pack an expanded sparse save for the game.
          --base PATH     With --split --sparse, diff every save member against this
                          packed save, sparse split directory, or expanded save.
                          JSON uses recursive overlays; other members use text diffs.
          --split         Use five table JSON files (Actor.json through
                          Conversation.json) plus trailing.bin in one directory.
          --sparse        With --split, restructure the tables whose shape is
                          known - dialogue statuses become one key range per
                          status - and leave out what can be derived. Keeps
                          every key and value, but not the entry order or the
                          Lua list/dictionary split, so the blob it converts
                          back to is not byte for byte the one it read.
                          Reading detects the form, so --to-lua needs no flag.
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
    bool split = false;
    bool sparse = false;
    bool pack = false;
    string? baseline = null;

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
            case "--pack":
                pack = true;
                break;
            case "--base":
                baseline = NextArg(argv, ref i, arg);
                break;
            case "--split":
                split = true;
                break;
            case "--sparse":
                sparse = true;
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

    if (sparse && !split)
    {
        throw new ArgumentException($"--sparse only applies to --split output\n\n{Usage}");
    }
    if (baseline is not null && (!split || !sparse || toLua || pack))
    {
        throw new ArgumentException($"--base requires --split --sparse output\n\n{Usage}");
    }

    if (pack)
    {
        if (toLua || split || sparse || output is null)
        {
            throw new ArgumentException($"--pack requires only --output <save.ntwtf.zip>\n\n{Usage}");
        }
        ExpandedSave.Pack(input, output);
        return 0;
    }

    if (toLua)
    {
        LuaTable document;
        if (split)
        {
            document = LuaSplitFiles.Read(input);
        }
        else
        {
            using FileStream json = File.OpenRead(input);
            document = LuaJson.ReadDocument(json);
        }
        using Stream lua = output is null ? Console.OpenStandardOutput() : File.Create(output);
        LuaBinary.WriteDocument(lua, document);
        lua.Flush();
        return 0;
    }

    PackedSave? packed = SaveBlob.IsArchive(input) ? SaveBlob.ReadArchive(input) : null;
    LuaTable allTables = ReadTables(
        packed?.LuaBytes ?? SaveBlob.Read(input),
        input,
        out int trailing
    );
    if (trailing > 0)
    {
        // PersistentDataManager.ApplyExtraData reads length-prefixed Lua source
        // after the five tables; this tool does not interpret it.
        Console.Error.WriteLine(
            $"note: {trailing} trailing byte(s) of extra data after the five tables were not decoded"
        );
    }

    if (split)
    {
        if (output is null)
        {
            throw new ArgumentException("--split conversion to JSON requires --output <directory>");
        }
        if (packed is not null)
        {
            ExpandedSave.Write(output, packed, allTables, indent, sparse, baseline);
            return 0;
        }
        if (baseline is null)
        {
            LuaSplitFiles.Write(output, allTables, indent, sparse);
        }
        else
        {
            LuaSplitFiles.WriteDiff(output, allTables, baseline, indent);
        }
        return 0;
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
    LuaJson.Write(stream, selected, indent, table);
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
