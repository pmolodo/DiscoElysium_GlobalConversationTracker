// SPDX-License-Identifier: MIT
using System.Text.Json;
using GlobalConversationTracker.Core;
using Xunit;

namespace NtwtfDecode.Tests;

/// <summary>
/// Every committed fixture says what format it is in, and is at the current version of it.
/// </summary>
/// <remarks>
/// <para>ONLY THE LATEST VERSION IS EVER COMMITTED. A version bump regenerates every file of
/// that format in the repository, so no reader here has to understand an older one and the
/// code that would have read one goes with the bump. This is what keeps that true: without
/// it the rule is a sentence in an issue, and the first half-finished migration leaves files
/// nobody notices until a reader refuses them.</para>
///
/// <para>IT IS ALSO WHAT LETS THE READERS BE STRICT. An absent stamp used to be read as
/// version 1 - a reader that cannot tell version 1 from silence - and 62 committed fixtures
/// were written one so that silence could be refused instead.</para>
/// </remarks>
public class FixtureStampTests
{
    /// <summary>The version each format this build writes is at.</summary>
    /// <remarks>
    /// Named from the writers rather than read off the files, which is the point: this is
    /// the build's own answer, and the test is that every committed file agrees with it.
    /// </remarks>
    private static readonly Dictionary<string, int> Current = new(StringComparer.Ordinal)
    {
        [LuaSplitFiles.DenseFormat] = LuaSplitFiles.FormatVersion,
        [LuaSplitFiles.SparseFormat] = LuaSplitFiles.FormatVersion,
        [SparseDiff.DiffFormat] = SparseDiff.FormatVersion,
        [JsonDiff.Format] = JsonDiff.FormatVersion,
        [ExpandedSave.DiffFormat] = ExpandedSave.FormatVersion,
        [FixtureFormats.ScenarioSuites] = FixtureFormats.FormatVersion,
        [FixtureFormats.BranchShapes] = FixtureFormats.FormatVersion,
    };

    [Fact]
    public void EveryCommittedFixtureCarriesTheCurrentVersionOfWhatItSaysItIs()
    {
        string testing = Path.Combine(RepoRoot(), "testing");
        var wrong = new List<string>();
        int seen = 0;

        foreach (string path in Directory.GetFiles(testing, "*.json", SearchOption.AllDirectories))
        {
            JsonElement root;
            try
            {
                using JsonDocument document = JsonDocument.Parse(File.ReadAllText(path));
                root = document.RootElement.Clone();
            }
            catch (JsonException)
            {
                continue;
            }

            if (root.ValueKind != JsonValueKind.Object
                || !root.TryGetProperty(FormatStamp.FormatPropertyName, out JsonElement named))
            {
                continue;
            }

            seen++;
            string format = named.GetString() ?? string.Empty;
            string shown = Path.GetRelativePath(RepoRoot(), path);

            if (!Current.TryGetValue(format, out int current))
            {
                wrong.Add($"{shown}: '{format}' is not a format this build writes");
            }
            else if (!root.TryGetProperty(
                FormatStamp.VersionPropertyName, out JsonElement stamped))
            {
                wrong.Add(
                    $"{shown}: a {format} carrying no {FormatStamp.VersionPropertyName}");
            }
            else if (!stamped.TryGetInt32(out int version) || version != current)
            {
                wrong.Add($"{shown}: {format} version {stamped}, and this build writes {current}");
            }
        }

        Assert.True(wrong.Count == 0, string.Join("\n", wrong));
        Assert.True(seen > 0, $"nothing under {testing} names a format");
    }

    /// <summary>The repository, found by walking up from the test assembly.</summary>
    private static string RepoRoot()
    {
        var here = new DirectoryInfo(AppContext.BaseDirectory);
        while (here is not null)
        {
            if (Directory.Exists(Path.Combine(here.FullName, ".git")))
            {
                return here.FullName;
            }

            here = here.Parent;
        }

        throw new DirectoryNotFoundException(
            $"No repository above {AppContext.BaseDirectory}.");
    }
}
