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

        Assert.StartsWith("--- a/save.states.lua\n+++ b/save.states.lua\n@@\n-", patch);
        Assert.Equal(target, TextDiff.Apply("save.states.lua", baseline, patch));
        Assert.Throws<InvalidDataException>(() =>
            TextDiff.Apply("save.states.lua", "wrong"u8.ToArray(), patch)
        );
    }
}
