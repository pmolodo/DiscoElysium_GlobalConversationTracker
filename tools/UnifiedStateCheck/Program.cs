using UnifiedConversationTracker;
using UnifiedConversationTracker.Persistence;
using UnifiedStateCheck;

const int ExitPass = 0;
const int ExitFail = 1;
const int ExitError = 2;

const string Usage = """
    UnifiedStateCheck - verify unified-conversation-state.json is the union of two saves.

    Usage:
      dotnet run --project tools/UnifiedStateCheck -- <saveA> <saveB> [options]
      dotnet run --project tools/UnifiedStateCheck -- --list

    <saveA> and <saveB> each name a save: a path to a '<name>.ntwtf.zip', an
    expanded '<name>.ntwtf' folder or a '<name>.ntwtf.lua' file, or a bare save
    name resolved inside the SaveGames directory.

    Passes when, for every dialogue entry, the unified state is at least as high
    as the higher of the two saves, ordering Untouched < WasOffered < WasDisplayed.
    A unified status strictly higher than both saves is legal and is reported as
    information, not as a failure.

    Options:
      -d, --dir PATH      SaveGames directory. Default: the game's own.
      -s, --state PATH    The unified state file.
                          Default: <dir>/unified-conversation-state.json
          --list          List the saves in <dir> and exit.
      -h, --help          Show this message.

    Exit codes: 0 pass, 1 fail, 2 usage or read error.
    """;

try
{
    return Run(args);
}
catch (Exception ex)
{
    Console.Error.WriteLine(ex.Message);
    return ExitError;
}

int Run(string[] argv)
{
    string? saveA = null;
    string? saveB = null;
    string? directory = null;
    string? statePath = null;
    bool list = false;

    for (int i = 0; i < argv.Length; i++)
    {
        string arg = argv[i];
        switch (arg)
        {
            case "-h" or "--help":
                Console.WriteLine(Usage);
                return ExitPass;
            case "-d" or "--dir":
                directory = NextArg(argv, ref i, arg);
                break;
            case "-s" or "--state":
                statePath = NextArg(argv, ref i, arg);
                break;
            case "--list":
                list = true;
                break;
            default:
                if (arg.StartsWith('-'))
                {
                    throw new ArgumentException($"Unknown option '{arg}'\n\n{Usage}");
                }
                if (saveA is null)
                {
                    saveA = arg;
                }
                else if (saveB is null)
                {
                    saveB = arg;
                }
                else
                {
                    throw new ArgumentException($"Unexpected extra argument '{arg}'\n\n{Usage}");
                }
                break;
        }
    }

    directory ??= DefaultSaveDirectory();

    if (list)
    {
        Console.WriteLine($"Saves in {directory}:");
        foreach (string name in SaveConversationReader.ListSaveNames(directory))
        {
            Console.WriteLine($"  {name}");
        }
        return ExitPass;
    }

    if (saveA is null || saveB is null)
    {
        throw new ArgumentException($"Two saves are required\n\n{Usage}");
    }

    statePath ??= Path.Combine(directory, UnifiedStateStore.FileName);

    Console.WriteLine($"SaveGames directory : {directory}");

    UnifiedConversationState stateA = SaveConversationReader.Load(
        saveA,
        directory,
        out string pathA
    );
    Console.WriteLine($"Save A : {pathA}");
    Console.WriteLine($"         {Describe(stateA)}");

    UnifiedConversationState stateB = SaveConversationReader.Load(
        saveB,
        directory,
        out string pathB
    );
    Console.WriteLine($"Save B : {pathB}");
    Console.WriteLine($"         {Describe(stateB)}");

    if (!File.Exists(statePath))
    {
        // Not an assertion failure but not a pass either: with no file there is
        // nothing to have merged, so the run proves nothing.
        Console.Error.WriteLine();
        Console.Error.WriteLine($"No unified state file at {statePath}.");
        Console.Error.WriteLine(
            "The mod writes it on the first dialogue entry of a session, so this means no "
                + "tracked playthrough has happened yet."
        );
        return ExitError;
    }

    UnifiedStateLoadResult load = UnifiedStateJson.Deserialize(
        File.ReadAllBytes(statePath),
        statePath
    );
    if (!load.IsLoaded)
    {
        Console.Error.WriteLine();
        Console.Error.WriteLine($"Could not read {statePath}: {load.Outcome} - {load.ErrorMessage}");
        return ExitError;
    }

    UnifiedConversationState unified = load.RequireState();
    Console.WriteLine($"Unified : {statePath}");
    Console.WriteLine($"          {Describe(unified)}");
    if (load.SkippedRowCount > 0)
    {
        Console.WriteLine($"          {load.SkippedRowCount} unreadable row(s) skipped:");
        foreach (string warning in load.Warnings)
        {
            Console.WriteLine($"            {warning}");
        }
    }
    Console.WriteLine();

    UnionReport report = UnionReport.Compare(unified, stateA, stateB);
    report.Write(Console.Out);
    return report.Passed ? ExitPass : ExitFail;
}

static string Describe(UnifiedConversationState state) =>
    $"{state.EntryCount} entries above Untouched in {state.ConversationCount} conversations";

static string DefaultSaveDirectory() =>
    Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        "AppData",
        "LocalLow",
        "ZAUM Studio",
        "Disco Elysium",
        "SaveGames"
    );

static string NextArg(string[] argv, ref int i, string option)
{
    i++;
    if (i >= argv.Length)
    {
        throw new ArgumentException($"Option '{option}' requires a value");
    }
    return argv[i];
}
