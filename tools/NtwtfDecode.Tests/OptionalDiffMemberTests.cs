// SPDX-License-Identifier: MIT
using System.Text.Json.Nodes;
using Xunit;

namespace NtwtfDecode.Tests;

/// <summary>
/// A diff carries only the halves it uses.
/// </summary>
/// <remarks>
/// Both formats used to require <c>_remove</c> and <c>_changes</c> whether or not they
/// held anything, so a diff that changed a single value still carried an empty removal
/// list. These files are read by people - a scenario save's diff is how a reader learns
/// what that scenario changes - and an empty half is one more thing to look past before
/// finding the one line that matters.
/// </remarks>
public class OptionalDiffMemberTests
{
    [Fact]
    public void AJsonDiffWithNoRemovalsOmitsTheRemoveMember()
    {
        JsonNode baseline = JsonNode.Parse("""{"money":0,"area":"Whirling"}""")!;
        JsonNode target = JsonNode.Parse("""{"money":5100,"area":"Whirling"}""")!;

        JsonObject patch = JsonDiff.Create(baseline, target)!;

        Assert.False(patch.ContainsKey("_remove"));
        Assert.Contains("money", patch["_changes"]!.ToJsonString());
    }

    [Fact]
    public void AJsonDiffWithNoChangesOmitsTheChangesMember()
    {
        JsonNode baseline = JsonNode.Parse("""{"keep":1,"drop":2}""")!;
        JsonNode target = JsonNode.Parse("""{"keep":1}""")!;

        JsonObject patch = JsonDiff.Create(baseline, target)!;

        Assert.Contains("/drop", patch["_remove"]!.ToJsonString());
        Assert.False(patch.ContainsKey("_changes"));
    }

    [Fact]
    public void AJsonDiffAppliesWithNeitherMemberPresent()
    {
        JsonNode baseline = JsonNode.Parse("""{"money":0}""")!;
        var patch = new JsonObject { [LuaJson.FormatName] = JsonDiff.Format };

        JsonNode? rebuilt = JsonDiff.Apply(baseline, patch, "empty patch");

        Assert.True(JsonNode.DeepEquals(baseline, rebuilt));
    }

    [Fact]
    public void AJsonDiffStillAppliesWithBothMembersPresent()
    {
        // Files written before this change carry both, and must keep working.
        JsonNode baseline = JsonNode.Parse("""{"money":0,"drop":1}""")!;
        var patch = new JsonObject
        {
            [LuaJson.FormatName] = JsonDiff.Format,
            ["_remove"] = new JsonArray("/drop"),
            ["_changes"] = new JsonObject { ["money"] = 5100 },
        };

        JsonNode? rebuilt = JsonDiff.Apply(baseline, patch, "old patch");

        Assert.Equal(5100, rebuilt!["money"]!.GetValue<int>());
        Assert.Null(rebuilt["drop"]);
    }

    [Fact]
    public void AJsonDiffWithTheWrongShapeIsStillRefused()
    {
        JsonNode baseline = JsonNode.Parse("""{"money":0}""")!;
        var patch = new JsonObject
        {
            [LuaJson.FormatName] = JsonDiff.Format,
            ["_remove"] = "not an array",
        };

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => JsonDiff.Apply(baseline, patch, "bad patch"));

        Assert.Contains("_remove", error.Message);
    }

    [Fact]
    public void ASparseDiffWithNoRemovalsOmitsTheRemoveMember()
    {
        var baseline = new SparseMap();
        baseline.Add("kept", "same");
        baseline.Add("moved", "before");
        var target = new SparseMap();
        target.Add("kept", "same");
        target.Add("moved", "after");

        SparseMap patch = SparseDiff.Create(baseline, target)!;

        Assert.Null(patch.Find("_remove"));
        Assert.NotNull(patch.Find("_changes"));
    }

    [Fact]
    public void ASparseDiffAppliesWithNeitherMemberPresent()
    {
        var baseline = new SparseMap();
        baseline.Add("kept", "same");
        var patch = new SparseMap();
        patch.Add(LuaJson.FormatName, SparseDiff.DiffFormat);

        SparseMap rebuilt = SparseDiff.Apply(baseline, patch, "empty patch");

        Assert.Equal("same", rebuilt.Find("kept"));
    }

    [Fact]
    public void ASparseDiffRoundTripsWhenOnlyOneValueMoves()
    {
        // The shape every scenario save's table diff takes.
        var baseline = new SparseMap();
        baseline.Add("flag", false);
        baseline.Add("other", "untouched");
        var target = new SparseMap();
        target.Add("flag", true);
        target.Add("other", "untouched");

        SparseMap patch = SparseDiff.Create(baseline, target)!;
        SparseMap rebuilt = SparseDiff.Apply(baseline, patch, "one value");

        Assert.Equal(true, rebuilt.Find("flag"));
        Assert.Equal("untouched", rebuilt.Find("other"));
    }
}
