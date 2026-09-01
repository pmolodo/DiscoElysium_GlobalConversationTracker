// SPDX-License-Identifier: MIT
using Xunit;

namespace NtwtfDecode.Tests;

public class TextDiffTests
{
    [Fact]
    public void CreateAndApply_PreservesExactTextBytes()
    {
        byte[] baseline = "first\r\nold \"text\""u8.ToArray();
        byte[] target = "first\nnew text\n"u8.ToArray();

        string patch = TextDiff.Create("save.states.lua", baseline, target)!;

        Assert.Contains("-old \"text\"", patch);
        Assert.Contains("+new text", patch);
        Assert.DoesNotContain("\\u0022", patch);
        Assert.Equal(target, TextDiff.Apply("save.states.lua", baseline, patch));
        Assert.Throws<InvalidDataException>(() =>
            TextDiff.Apply("save.states.lua", "wrong"u8.ToArray(), patch)
        );
    }

    [Fact]
    public void Create_UsesSeparateHunksWithOneContextLine()
    {
        byte[] baseline = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\n"u8.ToArray();
        byte[] target = "one\nTWO\nthree\nfour\nfive\nsix\nseven\nEIGHT\nnine\n"u8.ToArray();

        string patch = TextDiff.Create("save.states.lua", baseline, target)!;

        Assert.Equal(2, patch.Split('\n').Count(line => line.StartsWith("@@ ")));
        Assert.Contains(" one\n-two\n+TWO\n three", patch);
        Assert.Contains(" seven\n-eight\n+EIGHT\n nine", patch);
        Assert.DoesNotContain(" four", patch);
        Assert.Equal(target, TextDiff.Apply("save.states.lua", baseline, patch));
    }
}
