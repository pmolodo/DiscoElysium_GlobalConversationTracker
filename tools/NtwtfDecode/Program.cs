using System.Text;
using NtwtfDecode;

const string AllTables = "all";
const int DefaultIndent = 2;
const int ExitFailure = 1;

const string Usage = """
    NtwtfDecode - dump Disco Elysium {save}.ntwtf.lua data as JSON.

    Usage:
      dotnet run --project tools/NtwtfDecode -- <input> [options]

    <input> is a {save}.ntwtf.lua file, or an expanded .ntwtf save folder
    containing exactly one such file.

    Options:
      -o, --output PATH   Write JSON here instead of stdout.
      -t, --table NAME    Table to dump: Actor, Item, Location, Variable,
                          Conversation, or all. Default: Conversation.
          --indent N      JSON indent width. Default: 2.
          --compact       Single-line JSON (overrides --indent).
      -h, --help          Show this message.
    """;

try
{
    return Run(args);
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

    string inputPath = ResolveInputPath(input);
    LuaTable allTables = RawDataReader.ReadAllTables(File.ReadAllBytes(inputPath), out int trailing);
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
                + $"{string.Join(", ", RawDataReader.TableNames)}, {AllTables}"
        );
    }

    if (output is null)
    {
        var stdout = new StreamWriter(Console.OpenStandardOutput(), new UTF8Encoding(false));
        PythonJson.Write(stdout, selected, indent);
        stdout.Write('\n');
        stdout.Flush();
    }
    else
    {
        using var writer = new StreamWriter(output, false, new UTF8Encoding(false));
        PythonJson.Write(writer, selected, indent);
    }
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

static string ResolveInputPath(string input)
{
    if (Directory.Exists(input))
    {
        string[] candidates = Directory.GetFiles(input, "*.ntwtf.lua");
        if (candidates.Length != 1)
        {
            throw new ArgumentException(
                $"Expected exactly one *.ntwtf.lua in '{input}', found {candidates.Length}"
            );
        }
        return candidates[0];
    }
    if (!File.Exists(input))
    {
        throw new FileNotFoundException($"No such file or directory: {input}", input);
    }
    return input;
}
