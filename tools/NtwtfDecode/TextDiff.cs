// SPDX-License-Identifier: MIT
using System.Text;

namespace NtwtfDecode;

/// <summary>A UTF-8, line-oriented diff for non-JSON save members.</summary>
public static class TextDiff
{
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    /// <summary>Creates a readable unified diff, or null for equivalent text.</summary>
    public static string? Create(string name, byte[] baseline, byte[] target)
    {
        List<string> oldLines = ReadLines(baseline, name);
        List<string> newLines = ReadLines(target, name);
        if (oldLines.SequenceEqual(newLines))
        {
            return null;
        }
        List<Operation> operations = Diff(oldLines, newLines);
        var patch = new StringBuilder();
        patch.Append("--- a/").Append(name).Append('\n');
        patch.Append("+++ b/").Append(name).Append('\n');
        patch.Append("@@ -1,").Append(oldLines.Count);
        patch.Append(" +1,").Append(newLines.Count).Append(" @@\n");
        foreach (Operation operation in operations)
        {
            patch.Append(operation.Prefix).Append(operation.Line).Append('\n');
        }
        return patch.ToString();
    }

    /// <summary>Applies a line-oriented patch and verifies its complete old side.</summary>
    public static byte[] Apply(string name, byte[] baseline, string patch)
    {
        string[] lines = Normalize(patch).Split('\n');
        if (lines.Length < 5
            || lines[0] != $"--- a/{name}"
            || lines[1] != $"+++ b/{name}"
            || !lines[2].StartsWith("@@ -1,", StringComparison.Ordinal)
            || !lines[2].EndsWith(" @@", StringComparison.Ordinal)
            || lines[^1].Length != 0)
        {
            throw new InvalidDataException($"Text diff for '{name}' is malformed");
        }
        var oldLines = new List<string>();
        var newLines = new List<string>();
        foreach (string line in lines[3..^1])
        {
            if (line.Length == 0 || line[0] is not (' ' or '-' or '+'))
            {
                throw new InvalidDataException($"Text diff for '{name}' has a malformed line");
            }
            string content = line[1..];
            if (line[0] != '+')
            {
                oldLines.Add(content);
            }
            if (line[0] != '-')
            {
                newLines.Add(content);
            }
        }
        if (!ReadLines(baseline, name).SequenceEqual(oldLines))
        {
            throw new InvalidDataException($"Text diff for '{name}' does not match its baseline");
        }
        return StrictUtf8.GetBytes(string.Join('\n', newLines) + "\n");
    }

    private static List<Operation> Diff(IReadOnlyList<string> oldLines, IReadOnlyList<string> newLines)
    {
        var lengths = new int[oldLines.Count + 1, newLines.Count + 1];
        for (int oldIndex = oldLines.Count - 1; oldIndex >= 0; oldIndex--)
        {
            for (int newIndex = newLines.Count - 1; newIndex >= 0; newIndex--)
            {
                lengths[oldIndex, newIndex] = oldLines[oldIndex] == newLines[newIndex]
                    ? lengths[oldIndex + 1, newIndex + 1] + 1
                    : Math.Max(lengths[oldIndex + 1, newIndex], lengths[oldIndex, newIndex + 1]);
            }
        }
        var result = new List<Operation>();
        int oldPosition = 0;
        int newPosition = 0;
        while (oldPosition < oldLines.Count || newPosition < newLines.Count)
        {
            if (oldPosition < oldLines.Count
                && newPosition < newLines.Count
                && oldLines[oldPosition] == newLines[newPosition])
            {
                result.Add(new Operation(' ', oldLines[oldPosition++]));
                newPosition++;
            }
            else if (newPosition < newLines.Count
                && (oldPosition == oldLines.Count
                    || lengths[oldPosition, newPosition + 1]
                        > lengths[oldPosition + 1, newPosition]))
            {
                result.Add(new Operation('+', newLines[newPosition++]));
            }
            else
            {
                result.Add(new Operation('-', oldLines[oldPosition++]));
            }
        }
        return result;
    }

    private static List<string> ReadLines(byte[] bytes, string name)
    {
        string text;
        try
        {
            text = StrictUtf8.GetString(bytes);
        }
        catch (DecoderFallbackException error)
        {
            throw new InvalidDataException(
                $"Non-JSON save member '{name}' is not UTF-8 text and cannot be diffed",
                error
            );
        }
        string[] lines = Normalize(text).Split('\n');
        int count = lines[^1].Length == 0 ? lines.Length - 1 : lines.Length;
        return lines.Take(count).ToList();
    }

    private static string Normalize(string text) =>
        text.Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n');

    private sealed record Operation(char Prefix, string Line);
}
