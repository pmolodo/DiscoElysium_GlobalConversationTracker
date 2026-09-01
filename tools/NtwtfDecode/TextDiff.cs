// SPDX-License-Identifier: MIT
using System.Text;
using System.Text.Json;

namespace NtwtfDecode;

/// <summary>An exact, UTF-8 text diff for non-JSON save members.</summary>
public static class TextDiff
{
    private static readonly UTF8Encoding StrictUtf8 = new(
        encoderShouldEmitUTF8Identifier: false,
        throwOnInvalidBytes: true
    );

    /// <summary>Creates a textual old/new patch, or null for identical bytes.</summary>
    public static string? Create(string name, byte[] baseline, byte[] target)
    {
        if (baseline.SequenceEqual(target))
        {
            return null;
        }
        string oldText = Decode(baseline, name);
        string newText = Decode(target, name);
        return $"--- a/{name}\n+++ b/{name}\n@@\n-"
            + JsonSerializer.Serialize(oldText)
            + "\n+"
            + JsonSerializer.Serialize(newText)
            + "\n";
    }

    /// <summary>Applies a textual patch and verifies that its old side matches.</summary>
    public static byte[] Apply(string name, byte[] baseline, string patch)
    {
        string[] lines = patch.Split('\n');
        if (lines.Length != 6
            || lines[0] != $"--- a/{name}"
            || lines[1] != $"+++ b/{name}"
            || lines[2] != "@@"
            || !lines[3].StartsWith('-')
            || !lines[4].StartsWith('+')
            || lines[5].Length != 0)
        {
            throw new InvalidDataException($"Text diff for '{name}' is malformed");
        }
        string oldText = JsonSerializer.Deserialize<string>(lines[3][1..])
            ?? throw new InvalidDataException($"Text diff for '{name}' has no old text");
        string newText = JsonSerializer.Deserialize<string>(lines[4][1..])
            ?? throw new InvalidDataException($"Text diff for '{name}' has no new text");
        if (Decode(baseline, name) != oldText)
        {
            throw new InvalidDataException($"Text diff for '{name}' does not match its baseline");
        }
        return StrictUtf8.GetBytes(newText);
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
}
