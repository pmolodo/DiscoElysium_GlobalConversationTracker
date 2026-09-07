// SPDX-License-Identifier: MIT
using System.Text;
using GlobalConversationTracker.Persistence;
using NtwtfDecode;
using Xunit;

namespace FormatConvert.Tests;

/// <summary>
/// Working out what a file is, from the file.
/// </summary>
/// <remarks>
/// THE HALF THAT IS NEW. What to DO with an old global state file was already tested by
/// <c>LegacyGlobalStateConversionTests</c>; what this adds is recognising which of six
/// formats a file is and which version of it, since the tool takes neither as an argument.
/// A converter that guesses wrong writes a file in the wrong shape and says it succeeded.
/// </remarks>
public class FormatDetectionTests
{
    private const string Source = "test.json";

    [Fact]
    public void AGlobalStateIsRecognisedByItsVersionAndConversations()
    {
        Detected what = Detect(
            "{\"version\":4,\"conversations\":{},\"orbs\":[]}");

        Assert.Equal(Formats.GlobalState, what.Name);
        Assert.Equal(4, what.Version);
        Assert.Equal(GlobalStateJson.FormatVersion, what.Current);
        Assert.True(what.IsCurrent);
    }

    [Fact]
    public void AVersionWithoutConversationsIsNotClaimedAsAGlobalState()
    {
        // TWO PROPERTIES RATHER THAN ONE, deliberately: a bare `version` is a plausible
        // thing for some other file to carry, and claiming it would mean reading a file
        // this cannot read.
        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => Detect("{\"version\":4,\"something\":1}"));

        Assert.Contains("names no format", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ALuaSideFileIsRecognisedByItsFormatName()
    {
        Detected what = Detect(
            $"{{\"_format\":\"{SparseDiff.DiffFormat}\",\"_changes\":{{}}}}");

        Assert.Equal(SparseDiff.DiffFormat, what.Name);
        Assert.Equal(SparseDiff.FormatVersion, what.Current);
    }

    [Fact]
    public void AnUnstampedLuaSideFileIsVersionOne()
    {
        // Every format was stamped while its shape was unchanged, so the files written
        // before the stamp are version 1 in fact rather than by convention - and every
        // committed fixture in this repository is one of them.
        Detected what = Detect(
            $"{{\"_format\":\"{SparseDiff.DiffFormat}\",\"_changes\":{{}}}}");

        Assert.Equal(1, what.Version);
        Assert.True(what.IsCurrent);
    }

    [Fact]
    public void AStampedLuaSideFileTakesTheStampedVersion()
    {
        Detected what = Detect(
            $"{{\"_format\":\"{SparseDiff.DiffFormat}\",\"_formatVersion\":7,\"_changes\":{{}}}}");

        Assert.Equal(7, what.Version);
        Assert.True(what.IsFromTheFuture);
        Assert.False(what.IsCurrent);
    }

    [Theory]
    [InlineData("dense")]
    [InlineData("expanded-save-diff")]
    [InlineData("json-diff")]
    [InlineData("sparse")]
    [InlineData("sparse-diff")]
    public void EveryLuaSideFormatThisRepositoryWritesIsRecognised(string format)
    {
        // SPELLED OUT RATHER THAN TAKEN FROM THE TABLE, which is the point: a test that
        // read the same dictionary the code reads would pass however many formats were
        // missing from it. These five are the ones de-bnjy.3 names.
        Detected what = Detect($"{{\"_format\":\"{format}\"}}");

        Assert.Equal(format, what.Name);
        Assert.True(what.IsCurrent);
    }

    [Fact]
    public void AFormatNobodyKnowsIsRefusedWithTheListOfOnesThatAre()
    {
        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => Detect("{\"_format\":\"invented\"}"));

        Assert.Contains("'invented'", error.Message, StringComparison.Ordinal);
        Assert.Contains(SparseDiff.DiffFormat, error.Message, StringComparison.Ordinal);
        Assert.Contains(Formats.GlobalState, error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void SomethingThatIsNotJsonSaysSoRatherThanNamingAFormat()
    {
        InvalidDataException error = Assert.Throws<InvalidDataException>(() => Detect("{"));

        Assert.Contains("not valid JSON", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void JsonThatIsNotAnObjectIsRefusedByWhatItIs()
    {
        InvalidDataException error = Assert.Throws<InvalidDataException>(() => Detect("[1,2]"));

        Assert.Contains("Array", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void AGlobalStateWhoseVersionIsNotANumberSaysThatRatherThanFallingThrough()
    {
        // It IS a global state - both properties are there - so reporting it as an
        // unrecognised format would send the reader looking for the wrong problem.
        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => Detect("{\"version\":\"four\",\"conversations\":{}}"));

        Assert.Contains(Formats.GlobalState, error.Message, StringComparison.Ordinal);
        Assert.Contains("not an integer", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ALuaSideFormatHasNoOlderShapeToConvertFrom()
    {
        // Every one of them is at version 1, so there is nothing to convert and the
        // message says which version the file would have to be. If this ever fails it is
        // because a format was bumped, and its conversion belongs in Formats.ToCurrent.
        var what = new Detected(SparseDiff.DiffFormat, 0, SparseDiff.FormatVersion);

        InvalidDataException error = Assert.Throws<InvalidDataException>(
            () => Formats.ToCurrent(what, Encoding.UTF8.GetBytes("{}"), Source));

        Assert.Contains("knows no conversion", error.Message, StringComparison.Ordinal);
    }

    private static Detected Detect(string json) =>
        Formats.Detect(Encoding.UTF8.GetBytes(json), Source);
}
