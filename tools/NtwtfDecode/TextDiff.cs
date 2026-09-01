// SPDX-License-Identifier: MIT
using System.Diagnostics;
using System.Text;

namespace NtwtfDecode;

/// <summary>Creates and applies UTF-8 text diffs using Git.</summary>
public static class TextDiff
{
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    /// <summary>Creates a Git unified diff with one context line.</summary>
    public static string? Create(string name, byte[] baseline, byte[] target)
    {
        string oldText = Decode(baseline, name);
        string newText = Decode(target, name);
        if (Normalize(oldText) == Normalize(newText))
        {
            return null;
        }
        string directory = CreateTemporaryDirectory("diff");
        try
        {
            string oldPath = Path.Combine(directory, "old.txt");
            string newPath = Path.Combine(directory, "new.txt");
            File.WriteAllText(oldPath, Normalize(oldText), StrictUtf8);
            File.WriteAllText(newPath, Normalize(newText), StrictUtf8);
            ProcessResult result = RunGit(
                directory,
                "diff",
                "--no-index",
                "--no-ext-diff",
                "--text",
                "--unified=1",
                "--",
                oldPath,
                newPath
            );
            if (result.ExitCode != 1)
            {
                throw new InvalidOperationException(
                    $"git diff exited with code {result.ExitCode}: {result.Error.Trim()}"
                );
            }
            string[] lines = Normalize(result.Output).Split('\n');
            int firstHunk = Array.FindIndex(lines, line => line.StartsWith("@@ "));
            if (firstHunk < 0)
            {
                throw new InvalidDataException("git diff produced no unified hunk");
            }
            var patch = new StringBuilder();
            patch.Append("--- a/").Append(name).Append('\n');
            patch.Append("+++ b/").Append(name).Append('\n');
            foreach (string line in lines[firstHunk..])
            {
                if (line.Length > 0)
                {
                    patch.Append(line).Append('\n');
                }
            }
            return patch.ToString();
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }

    /// <summary>Applies a unified diff using Git.</summary>
    public static byte[] Apply(string name, byte[] baseline, string patch)
    {
        if (Path.GetFileName(name) != name)
        {
            throw new InvalidDataException($"Text diff member name '{name}' is not flat");
        }
        string directory = CreateTemporaryDirectory("apply");
        try
        {
            string memberPath = Path.Combine(directory, name);
            string patchPath = Path.Combine(directory, "member.diff");
            File.WriteAllText(memberPath, Normalize(Decode(baseline, name)), StrictUtf8);
            File.WriteAllText(patchPath, Normalize(patch), StrictUtf8);
            ProcessResult result = RunGit(
                directory,
                "apply",
                "--no-index",
                "--whitespace=nowarn",
                patchPath
            );
            if (result.ExitCode != 0)
            {
                throw new InvalidDataException(
                    $"git apply failed for '{name}': {(result.Error + result.Output).Trim()}"
                );
            }
            return File.ReadAllBytes(memberPath);
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }

    private static ProcessResult RunGit(string directory, params string[] arguments)
    {
        var start = new ProcessStartInfo("git")
        {
            RedirectStandardError = true,
            RedirectStandardOutput = true,
            UseShellExecute = false,
            WorkingDirectory = directory,
        };
        foreach (string argument in arguments)
        {
            start.ArgumentList.Add(argument);
        }
        using Process process = Process.Start(start)
            ?? throw new InvalidOperationException("Could not start Git");
        string output = process.StandardOutput.ReadToEnd();
        string error = process.StandardError.ReadToEnd();
        process.WaitForExit();
        return new ProcessResult(process.ExitCode, output, error);
    }

    private static string Decode(byte[] bytes, string name)
    {
        try
        {
            return StrictUtf8.GetString(bytes);
        }
        catch (DecoderFallbackException error)
        {
            throw new InvalidDataException(
                $"Non-JSON save member '{name}' is not UTF-8 text and cannot be diffed",
                error
            );
        }
    }

    private static string CreateTemporaryDirectory(string operation)
    {
        string path = Path.Combine(
            Path.GetTempPath(),
            $"ntwtf-{operation}-{Guid.NewGuid()}"
        );
        Directory.CreateDirectory(path);
        return path;
    }

    private static string Normalize(string text) =>
        text.Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n');

    private sealed record ProcessResult(int ExitCode, string Output, string Error);
}
