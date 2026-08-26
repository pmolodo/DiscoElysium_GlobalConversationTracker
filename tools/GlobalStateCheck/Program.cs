using GlobalConversationTracker;
using GlobalConversationTracker.Persistence;
using GlobalStateCheck;

const int ExitPass = 0;
const int ExitFail = 1;
const int ExitError = 2;

const string Usage = """
    GlobalStateCheck - verify global-conversation-state.json is the union of some saves.

    Usage:
      dotnet run --project tools/GlobalStateCheck -- <save> <save> [<save>...] [options]
      dotnet run --project tools/GlobalStateCheck -- --list

    Each <save> names a save: a path to a '<name>.ntwtf.zip', an expanded
    '<name>.ntwtf' folder or a '<name>.ntwtf.lua' file, or a bare save name
    resolved inside the SaveGames directory. Two or more are required.

    Passes when, for every dialogue entry, the global state is at least as high
    as the highest of the saves, ordering Untouched < WasOffered < WasDisplayed.
    A global status strictly higher than every save is legal and is reported as
    information, not as a failure.

    Options:
      -d, --dir PATH      SaveGames directory. Default: the game's own.
      -s, --state PATH    The global state file.
                          Default: <dir>/global-conversation-state.json
      -n, --examples N    How many entries to name per reported category.
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
    var saveArgs = new List<string>();
    string? directory = null;
    string? statePath = null;
    bool list = false;
    int maxExamples = UnionReport.DefaultMaxExamples;

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
            case "-n" or "--examples":
                string raw = NextArg(argv, ref i, arg);
                if (!int.TryParse(raw, out maxExamples) || maxExamples < 0)
                {
                    throw new ArgumentException(
                        $"Option '{arg}' needs a non-negative whole number, not '{raw}'"
                    );
                }
                break;
            case "--list":
                list = true;
                break;
            default:
                if (arg.StartsWith('-'))
                {
                    throw new ArgumentException($"Unknown option '{arg}'\n\n{Usage}");
                }
                saveArgs.Add(arg);
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

    if (saveArgs.Count < 2)
    {
        throw new ArgumentException($"At least two saves are required\n\n{Usage}");
    }

    statePath ??= Path.Combine(directory, GlobalStateStore.FileName);

    Console.WriteLine($"SaveGames directory : {directory}");

    var saves = new List<NamedSave>(saveArgs.Count);
    foreach (string saveArg in saveArgs)
    {
        GlobalConversationState state = SaveConversationReader.Load(
            saveArg,
            directory,
            out string savePath
        );
        string label = Path.GetFileName(savePath);
        Console.WriteLine($"Save : {savePath}");
        Console.WriteLine($"       {Describe(state)}");
        saves.Add(new NamedSave(label, state));
    }

    if (!File.Exists(statePath))
    {
        // Not an assertion failure but not a pass either: with no file there is
        // nothing to have merged, so the run proves nothing.
        Console.Error.WriteLine();
        Console.Error.WriteLine($"No global state file at {statePath}.");
        Console.Error.WriteLine(
            "The mod writes it on the first dialogue entry of a session, so this means no "
                + "tracked playthrough has happened yet."
        );
        return ExitError;
    }

    GlobalStateLoadResult load = GlobalStateJson.Deserialize(
        File.ReadAllBytes(statePath),
        statePath
    );
    if (!load.IsLoaded)
    {
        Console.Error.WriteLine();
        Console.Error.WriteLine($"Could not read {statePath}: {load.Outcome} - {load.ErrorMessage}");
        return ExitError;
    }

    GlobalConversationState global = load.RequireState();
    Console.WriteLine($"Global : {statePath}");
    Console.WriteLine($"          {Describe(global)}");
    if (load.SkippedRowCount > 0)
    {
        Console.WriteLine($"          {load.SkippedRowCount} unreadable row(s) skipped:");
        foreach (string warning in load.Warnings)
        {
            Console.WriteLine($"            {warning}");
        }
    }
    Console.WriteLine();

    UnionReport report = UnionReport.Compare(global, saves);
    report.Write(Console.Out, maxExamples);
    return report.Passed ? ExitPass : ExitFail;
}

static string Describe(GlobalConversationState state) =>
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
