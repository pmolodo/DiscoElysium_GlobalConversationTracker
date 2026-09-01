// SPDX-License-Identifier: MIT
using System.Text.Json.Nodes;
using Xunit;

namespace NtwtfDecode.Tests;

public class JsonDiffTests
{
    [Fact]
    public void CreateAndApply_RecursivelyChangesObjectsAndReplacesArrays()
    {
        JsonNode baseline = JsonNode.Parse(
            """{"keep":1,"nested":{"change":"old","remove":2},"array":[1,2]}"""
        )!;
        JsonNode target = JsonNode.Parse(
            """{"keep":1,"nested":{"change":null,"add":3},"array":[4]}"""
        )!;

        JsonObject patch = JsonDiff.Create(baseline, target)!;
        JsonNode? rebuilt = JsonDiff.Apply(baseline, patch, "test patch");

        Assert.True(JsonNode.DeepEquals(target, rebuilt));
        Assert.DoesNotContain("keep", patch["_changes"]!.ToJsonString());
        Assert.Contains("/nested/remove", patch["_remove"]!.ToJsonString());
    }
}
