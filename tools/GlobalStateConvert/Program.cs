// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Persistence;

const int ExitSuccess = 0;
const int ExitError = 2;

const string Usage = """
    GlobalStateConvert - convert a version 1 or 2 global state to version 3.

    Usage:
      dotnet run --project tools/GlobalStateConvert -- <input> <output>

    The output path must not exist. The input is never modified.
    """;

try
{
    if (args.Length == 1 && args[0] is "-h" or "--help")
    {
        Console.WriteLine(Usage);
        return ExitSuccess;
    }

    if (args.Length != 2)
    {
        throw new ArgumentException(Usage);
    }

    string inputPath = Path.GetFullPath(args[0]);
    string outputPath = Path.GetFullPath(args[1]);
    if (StringComparer.OrdinalIgnoreCase.Equals(inputPath, outputPath))
    {
        throw new ArgumentException("Input and output paths must be different.");
    }

    byte[] converted = GlobalStateJson.ConvertLegacyToUtf8Bytes(
        File.ReadAllBytes(inputPath), inputPath);
    using (FileStream output = new(
        outputPath, FileMode.CreateNew, FileAccess.Write, FileShare.None))
    {
        output.Write(converted, 0, converted.Length);
    }

    Console.WriteLine($"Converted '{inputPath}' to version {GlobalStateJson.FormatVersion} at '{outputPath}'.");
    return ExitSuccess;
}
catch (Exception ex)
{
    Console.Error.WriteLine(ex.Message);
    return ExitError;
}
