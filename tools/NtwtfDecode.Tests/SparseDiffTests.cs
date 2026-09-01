// SPDX-License-Identifier: MIT
using Xunit;

namespace NtwtfDecode.Tests;

/// <summary>Tests for recursive sparse JSON overlays.</summary>
public class SparseDiffTests
{
    [Fact]
    public void CreateAndApply_ChangeAddRemoveAndPreserveNull()
    {
        var oldNested = new SparseMap();
        oldNested.Add("same", 1);
        oldNested.Add("remove", 2);
        var baseline = new SparseMap();
        baseline.Add("nested", oldNested);
        baseline.Add("becomes-null", "value");

        var newNested = new SparseMap();
        newNested.Add("same", 1);
        newNested.Add("add", 3);
        var target = new SparseMap();
        target.Add("nested", newNested);
        target.Add("becomes-null", null);

        SparseMap patch = SparseDiff.Create(baseline, target)!;
        SparseMap merged = SparseDiff.Apply(baseline, patch, "test");

        Assert.Contains(merged.Entries, entry => entry.Key == "becomes-null");
        Assert.Null(merged.Find("becomes-null"));
        SparseMap nested = Assert.IsType<SparseMap>(merged.Find("nested"));
        Assert.Equal(1, nested.Find("same"));
        Assert.Equal(3, nested.Find("add"));
        Assert.False(nested.Has("remove"));
    }

    [Fact]
    public void Create_ReturnsNullForEqualTrees()
    {
        var baseline = new SparseMap();
        baseline.Add("same", 1);
        var target = new SparseMap();
        target.Add("same", 1);

        Assert.Null(SparseDiff.Create(baseline, target));
    }
}
