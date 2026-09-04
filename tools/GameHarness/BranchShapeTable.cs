// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Text.Json.Serialization;
using GlobalConversationTracker.Automation;

namespace GlobalConversationTracker.Harness
{
    /// <summary>
    /// The Pass / Fail scenarios, as a definition both harnesses run.
    /// </summary>
    /// <remarks>
    /// <para>ONE DEFINITION, TWO WAYS TO EXECUTE IT. A row names everything a scenario is -
    /// the global state to stage, the save and what it has already read, the budget, the
    /// conversation and entry, and the line that must come out - and says nothing about how
    /// to run it. <see cref="LookAheadSuites.BranchShapes"/> turns each row into an in-game
    /// suite; <c>tests/branch_shapes.rs</c> puts the same row to the engine over the
    /// shipped index. Neither is a mirror of the other, because there is nothing to
    /// mirror: they are two executors of one file.</para>
    ///
    /// <para>WHY IT IS NOT TWO SETS OF TESTS THAT AGREE. It was, briefly, and the trouble
    /// with that shape is that agreement has to be maintained by whoever edits either side.
    /// An offline test that has drifted from the run it stands in for is worse than no
    /// offline test: it is fast, green, and about a scenario nobody runs. Reading one file
    /// removes the possibility rather than guarding against it.</para>
    ///
    /// <para>WHERE A ROW STOPS. It names the fixture and the answer, and nothing about how
    /// either executor works - so what one needs and the other cannot use does not belong
    /// in a row's own fields. If that day comes, it goes under a key naming the executor,
    /// <c>offline</c> or <c>inGame</c>. Nothing needs it yet and no support for it is
    /// built: what the offline executor needs beyond these fields - what the save has
    /// already displayed - it reads out of the same save this one loads.</para>
    ///
    /// <para>WHAT EACH EXECUTOR STILL EARNS ON ITS OWN. The in-game run is the only thing
    /// that can say the patch is installed, that a real response menu was composed, and
    /// that the line reached the text the game drew. The offline run costs the time to read
    /// the index instead of a launch, needs no display, and can say the same thing about
    /// the answer. A disagreement between them is a finding, and there is now only one
    /// place a fixture could have changed to cause one.</para>
    /// </remarks>
    public sealed class BranchShapeTable
    {
        /// <summary>Where the definition lives, under the repository root.</summary>
        public const string FileName = "branch-shapes.json";

        /// <summary>The conversation every row opens.</summary>
        [JsonPropertyName("conversation")]
        public int Conversation { get; set; }

        /// <summary>The rolled check every row is about.</summary>
        [JsonPropertyName("entry")]
        public int Entry { get; set; }

        /// <summary>The scenarios.</summary>
        [JsonPropertyName("rows")]
        public List<BranchShapeRow> Rows { get; set; } = new List<BranchShapeRow>();

        /// <summary>Reads the definition, from the committed scenarios folder.</summary>
        /// <param name="scenarioRoot">Where the scenarios are, or null to find them.</param>
        /// <returns>The table.</returns>
        /// <exception cref="FileNotFoundException">It is not there.</exception>
        /// <exception cref="InvalidDataException">It is there and will not read.</exception>
        public static BranchShapeTable Read(string? scenarioRoot = null)
        {
            string path = Path.Combine(
                scenarioRoot
                    ?? Path.Combine(GameInstall.RepoRoot(), "testing", "scenarios"),
                FileName);

            if (!File.Exists(path))
            {
                throw new FileNotFoundException(
                    $"The branch shape definition is missing from {path}. Both the in-game "
                    + "suites and the offline test are built from it, so neither can run "
                    + "without it.",
                    path);
            }

            BranchShapeTable? table = JsonSerializer.Deserialize<BranchShapeTable>(
                File.ReadAllText(path),
                new JsonSerializerOptions { ReadCommentHandling = JsonCommentHandling.Skip });

            if (table == null || table.Rows.Count == 0)
            {
                throw new InvalidDataException($"{path} names no scenarios.");
            }

            return table;
        }
    }

    /// <summary>One scenario: the fixture that makes it, and the line it must draw.</summary>
    public sealed class BranchShapeRow
    {
        /// <summary>What to call the suite this becomes, and to pass to --suite.</summary>
        [JsonPropertyName("suite")]
        public string Suite { get; set; } = string.Empty;

        /// <summary>What the fixture arranges, in one line, for the report.</summary>
        [JsonPropertyName("what")]
        public string What { get; set; } = string.Empty;

        /// <summary>The global state to stage, naming what other saves have read.</summary>
        [JsonPropertyName("state")]
        public string State { get; set; } = string.Empty;

        /// <summary>The scenario save to load.</summary>
        [JsonPropertyName("save")]
        public string Save { get; set; } = string.Empty;

        /// <summary>The state budget to run at, or 0 for no such limit.</summary>
        /// <remarks>
        /// What a row starves a crawl with, so that a shape only an unfinished search can
        /// produce is reachable at all. TEST-ONLY, and since de-7z0f not a player setting:
        /// the memory budget cannot do this job, because it is checked when a node is
        /// dequeued and this group's whole crawl fits in a thousandth of the smallest
        /// budget a player can express.
        /// </remarks>
        [JsonPropertyName("stateBudget")]
        public int StateBudget { get; set; }

        /// <summary>What the word "Pass" must be drawn as.</summary>
        [JsonPropertyName("pass")]
        public BranchShapeHalf Pass { get; set; } = new BranchShapeHalf();

        /// <summary>What the word "Fail" must be drawn as.</summary>
        [JsonPropertyName("fail")]
        public BranchShapeHalf Fail { get; set; } = new BranchShapeHalf();
    }

    /// <summary>One half of the expected line.</summary>
    public sealed class BranchShapeHalf
    {
        /// <summary>The word's colour, as the definition spells it.</summary>
        [JsonPropertyName("colour")]
        public string Colour { get; set; } = string.Empty;

        /// <summary>Its asterisk's colour, "gaveUp", or null for no asterisk.</summary>
        [JsonPropertyName("marker")]
        public string? Marker { get; set; }

        /// <summary>This half as the suites express it.</summary>
        /// <exception cref="InvalidDataException">It names a colour that is not one.</exception>
        public BranchHalf Expected() =>
            new BranchHalf(ColourOf(Colour), MarkerOf(Marker));

        private static BranchColour ColourOf(string colour) => colour switch
        {
            "orange" => BranchColour.Orange,
            "red" => BranchColour.Red,
            "darkRed" => BranchColour.DarkRed,
            _ => throw new InvalidDataException(
                $"'{colour}' is not a colour a half can be drawn in."),
        };

        private static Marker MarkerOf(string? marker) => marker switch
        {
            null => Harness.Marker.None,
            "orange" => Harness.Marker.Orange,
            "red" => Harness.Marker.Red,
            "gaveUp" => Harness.Marker.Uncertain,
            _ => throw new InvalidDataException(
                $"'{marker}' is not a marker a half can carry."),
        };

        /// <inheritdoc/>
        public override string ToString() =>
            Marker == null ? Colour : $"{Colour} with a {Marker} asterisk";
    }
}
