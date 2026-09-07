// SPDX-License-Identifier: MIT
using FormatConvert;

const int ExitSuccess = 0;
const int ExitError = 2;

const string Usage = """
    FormatConvert - bring a file this repository wrote up to the current version of
    its own format.

    Usage:
      dotnet run --project tools/FormatConvert -- <input> [output]

    It works out WHICH format the file is and WHICH version from the file itself,
    so neither is a flag. With no output path it writes a sibling named after the
    version it produced. The input is never modified and an existing output is
    never overwritten.

    A file already at the current version is left alone and reported as such,
    which is a success rather than an error.
    """;

try
{
    if (args.Length == 1 && args[0] is "-h" or "--help")
    {
        Console.WriteLine(Usage);
        return ExitSuccess;
    }

    if (args.Length is 0 or > 2)
    {
        throw new ArgumentException(Usage);
    }

    string inputPath = Path.GetFullPath(args[0]);
    byte[] input = File.ReadAllBytes(inputPath);
    Detected what = Formats.Detect(input, inputPath);

    // ALREADY CURRENT IS A SUCCESS, and it writes nothing. A tool meant to be pointed at
    // whatever is to hand will be pointed at current files constantly - by a script
    // sweeping a directory, or by somebody who does not know which version they have - and
    // failing there would make "convert everything" a thing nobody can write.
    if (what.IsCurrent)
    {
        Console.WriteLine(
            $"'{inputPath}' is already {what.Name} version {what.Version}, which is what "
            + "this build writes. Nothing to do.");
        return ExitSuccess;
    }

    // REFUSED, NOT CONVERTED, and this is the direction that matters most. A file from a
    // newer build is full of real history and this build cannot understand all of it, so
    // writing anything at all would be inventing the parts it could not read.
    if (what.IsFromTheFuture)
    {
        throw new InvalidDataException(
            $"'{inputPath}' is {what.Name} version {what.Version}, and this build writes "
            + $"version {what.Current} at most. It was written by a newer build; the file "
            + "is not damaged, so do not overwrite it - use a build at least as new as the "
            + "one that wrote it.");
    }

    string outputPath = args.Length == 2
        ? Path.GetFullPath(args[1])
        : DefaultOutput(inputPath, what.Current);

    if (StringComparer.OrdinalIgnoreCase.Equals(inputPath, outputPath))
    {
        throw new ArgumentException("Input and output paths must be different.");
    }

    byte[] converted = Formats.ToCurrent(what, input, inputPath);

    // CreateNew, so an existing output stops the run before a byte is written rather than
    // being replaced. The thing being converted is somebody's history and the output may
    // be an earlier conversion of it.
    using (FileStream output = new(
        outputPath, FileMode.CreateNew, FileAccess.Write, FileShare.None))
    {
        output.Write(converted, 0, converted.Length);
    }

    Console.WriteLine(
        $"Converted '{inputPath}' from {what.Name} version {what.Version} to version "
        + $"{what.Current} at '{outputPath}'.");
    return ExitSuccess;
}
catch (Exception error)
{
    Console.Error.WriteLine(error.Message);
    return ExitError;
}

// A sibling of the input, named for the version produced.
//
// NAMED RATHER THAN IN PLACE, which is what lets the tool need no second argument while
// still never touching its input. Writing over the input would be the obvious way to need
// no output path and is the one thing a converter of somebody's save history must not do.
static string DefaultOutput(string inputPath, int version)
{
    string directory = Path.GetDirectoryName(inputPath) ?? ".";
    string stem = Path.GetFileNameWithoutExtension(inputPath);
    string extension = Path.GetExtension(inputPath);
    return Path.Combine(directory, $"{stem}.v{version}{extension}");
}
