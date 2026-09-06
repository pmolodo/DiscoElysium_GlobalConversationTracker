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
    /// The look-ahead scenarios that are not about a check's two outcomes, as a definition
    /// both harnesses run.
    /// </summary>
    /// <remarks>
    /// <para>ONE DEFINITION, TWO WAYS TO EXECUTE IT, which is the same argument
    /// <see cref="BranchShapeTable"/> makes for the Pass / Fail shapes and the same file
    /// layout. A suite names everything a fixture is - the global state to stage, the mod
    /// settings to change, and per scenario a save, a conversation, the balance and clock
    /// to hold it at, and what each option must carry - and says nothing about how to run
    /// it. <see cref="LookAheadSuites.FromDefinition"/> turns each suite into an in-game
    /// run; <c>tests/scenario_suites.rs</c> puts the same scenarios to the engine over the
    /// shipped index.</para>
    ///
    /// <para>WHY THIS IS A SEPARATE FILE FROM THE BRANCH SHAPES, and why that is not two
    /// formats by accident. A branch shape varies along ONE axis - what the two outcomes
    /// of one check land on - so its definition is a table, with the conversation and the
    /// entry named once above eight rows, and the argument that the eight cover every
    /// shape a half can take is only legible in that form. These vary along all their axes
    /// at once and have to be written out. Both files build the same
    /// <see cref="LookAheadSuite"/>, and neither executor declares a scenario of its
    /// own.</para>
    ///
    /// <para>WHAT A SUITE STILL CANNOT SAY. An artefact check reads a file the mod wrote
    /// over the course of a launch and is a C# predicate; there is nothing offline to run
    /// it against. A row therefore NAMES one - see <see cref="InGameExtras"/> - and
    /// <see cref="LookAheadSuites"/> resolves the name, so the half only the game can
    /// execute sits beside the shared row rather than inside it. A definition that reads
    /// as shared and is not would be worse than two honest declarations.</para>
    /// </remarks>
    public sealed class ScenarioTable
    {
        /// <summary>Where the definition lives, under the scenario root.</summary>
        public const string FileName = "suites.json";

        /// <summary>The suites, in the order a full run would do them.</summary>
        [JsonPropertyName("suites")]
        public List<ScenarioSuiteDefinition> Suites { get; set; } =
            new List<ScenarioSuiteDefinition>();

        /// <summary>Reads the definition, from the committed scenarios folder.</summary>
        /// <param name="scenarioRoot">Where the scenarios are, or null to find them.</param>
        /// <returns>The table.</returns>
        /// <exception cref="FileNotFoundException">It is not there.</exception>
        /// <exception cref="InvalidDataException">It is there and will not read.</exception>
        public static ScenarioTable Read(string? scenarioRoot = null)
        {
            string path = Path.Combine(
                scenarioRoot
                    ?? Path.Combine(GameInstall.RepoRoot(), "testing", "scenarios"),
                FileName);

            if (!File.Exists(path))
            {
                throw new FileNotFoundException(
                    $"The scenario definition is missing from {path}. Both the in-game "
                    + "suites and the offline tests are built from it, so neither can run "
                    + "without it.",
                    path);
            }

            ScenarioTable? table = JsonSerializer.Deserialize<ScenarioTable>(
                File.ReadAllText(path),
                new JsonSerializerOptions { ReadCommentHandling = JsonCommentHandling.Skip });

            if (table == null || table.Suites.Count == 0)
            {
                throw new InvalidDataException($"{path} names no suites.");
            }

            return table;
        }
    }

    /// <summary>One suite: what to stage once, and the scenarios that share the staging.</summary>
    public sealed class ScenarioSuiteDefinition
    {
        /// <summary>What to call it on the command line.</summary>
        [JsonPropertyName("suite")]
        public string Suite { get; set; } = string.Empty;

        /// <summary>Why this suite is not being run, or empty when it is.</summary>
        /// <remarks>
        /// A SENTENCE RATHER THAN A FLAG, and it is not optional for a disabled suite: a
        /// definition that is switched off without saying why is one nobody can decide to
        /// switch back on. Both executors skip it and both say the sentence, so a run
        /// missing a claim reports which claim and on whose authority.
        ///
        /// Meant to be temporary, and paired with a task. A suite that stays off is a claim
        /// nobody is making any more, which wants deleting rather than disabling.
        /// </remarks>
        [JsonPropertyName("disabled")]
        public string Disabled { get; set; } = string.Empty;

        /// <summary>What the suite is for, in one line.</summary>
        [JsonPropertyName("what")]
        public string What { get; set; } = string.Empty;

        /// <summary>The longer argument, for a reader deciding whether it still holds.</summary>
        /// <remarks>
        /// NOT READ BY EITHER EXECUTOR, and kept for exactly that reason: it is the part of
        /// a suite that used to live in a C# doc comment, and losing it in the move would
        /// have been losing the only record of why a fixture is arranged the way it is.
        /// </remarks>
        [JsonPropertyName("why")]
        public string Why { get; set; } = string.Empty;

        /// <summary>The global state to stage, relative to the scenario root.</summary>
        [JsonPropertyName("state")]
        public string State { get; set; } = string.Empty;

        /// <summary>Mod settings to change for the run.</summary>
        [JsonPropertyName("settings")]
        public Dictionary<string, string>? Settings { get; set; }

        /// <summary>Why those settings, where it is not obvious. For a reader.</summary>
        [JsonPropertyName("settingsWhy")]
        public string SettingsWhy { get; set; } = string.Empty;

        /// <summary>What only the offline executor can check.</summary>
        /// <remarks>
        /// THE MIRROR OF <see cref="InGame"/>, and read by nobody here. Some claims are
        /// about every entry in a group rather than about the options a menu turned out to
        /// offer, and only the executor that needs no menu can make them - so the suite
        /// names one and <c>tests/scenario_suites.rs</c> runs it. Declared on this side so
        /// that a reader of the definition, and this parser, both know the key is meant.
        /// </remarks>
        [JsonPropertyName("offline")]
        public OfflineClaim? Offline { get; set; }

        /// <summary>The state budget to run at, or 0 for no such limit.</summary>
        /// <remarks>
        /// <para>What a suite starves a crawl with, so that a shape only an unfinished
        /// search can produce is reachable at all. TEST-ONLY, and since de-7z0f not a
        /// player setting; see <see cref="LookAheadSuites.TestStateBudgetSetting"/> for why
        /// the memory budget cannot take the job over.</para>
        ///
        /// <para>ON THE SUITE AND NOT ON A SCENARIO, because that is where it actually
        /// lands: it reaches the mod through the probe's prepare-suite command, alongside
        /// the settings, and those are read once per launch. A scenario carrying its own
        /// would read as though the harness could change it between two saves in one
        /// game, which it cannot.</para>
        ///
        /// <para>Spelled as its own field rather than left in <see cref="Settings"/> so
        /// that the offline executor - which has no settings and no mod, and takes the
        /// number straight to the request - does not have to know the mod's spelling for
        /// it.</para>
        /// </remarks>
        [JsonPropertyName("stateBudget")]
        public int StateBudget { get; set; }

        /// <summary>What only the in-game executor can check.</summary>
        [JsonPropertyName("inGame")]
        public InGameExtras? InGame { get; set; }

        /// <summary>The scenarios, in the order they run.</summary>
        [JsonPropertyName("scenarios")]
        public List<ScenarioDefinition> Scenarios { get; set; } =
            new List<ScenarioDefinition>();

        /// <summary>This suite as the harness runs it.</summary>
        /// <param name="checks">The artefact predicates, by the name a row gives.</param>
        /// <returns>The suite.</returns>
        /// <exception cref="InvalidDataException">A row names something that is not there.</exception>
        public LookAheadSuite Build(IReadOnlyDictionary<string, Func<string?, string?>> checks)
        {
            IReadOnlyList<SuiteArtefact> artefacts =
                (InGame?.Artefacts ?? new List<ArtefactDefinition>())
                    .Select(artefact => artefact.Build(Suite, checks))
                    .ToArray();

            IReadOnlyList<LogExpectation> logs =
                (InGame?.Log ?? new List<LogDefinition>())
                    .Select(line => line.Build())
                    .ToArray();

            var settings = new Dictionary<string, string>(
                Settings ?? new Dictionary<string, string>());
            if (StateBudget > 0)
            {
                settings[LookAheadSuites.TestStateBudgetSetting] =
                    StateBudget.ToString(CultureInfo.InvariantCulture);
            }

            return new LookAheadSuite(
                Suite,
                What,
                State,
                Scenarios.Select(scenario => scenario.Build(Suite)).ToArray(),
                pluginSettings: settings.Count > 0 ? settings : null,
                artefacts: artefacts.Count > 0 ? artefacts : null,
                logExpectations: logs.Count > 0 ? logs : null);
        }
    }

    /// <summary>The half of a suite that needs a game to check.</summary>
    /// <remarks>
    /// Under a key naming the executor, which is the shape agreed for anything one side
    /// can run and the other cannot. Everything here is a claim about what the MOD did -
    /// which files it left behind, what it said in the log - rather than about what the
    /// answer was, and there is no offline run for it to be true of.
    /// </remarks>
    public sealed class InGameExtras
    {
        /// <summary>Files the run should leave in the profile's SaveGames folder.</summary>
        [JsonPropertyName("artefacts")]
        public List<ArtefactDefinition>? Artefacts { get; set; }

        /// <summary>What the mod should, and should not, have logged.</summary>
        [JsonPropertyName("log")]
        public List<LogDefinition>? Log { get; set; }
    }

    /// <summary>A claim only a run without a game can make.</summary>
    /// <remarks>
    /// Under a key naming the executor, exactly as <see cref="InGameExtras"/> is. What
    /// goes here is a claim about EVERY entry in a group - that no search worth running
    /// exists anywhere in it - which the in-game run cannot make, because it can only see
    /// the handful of options a menu composed.
    /// </remarks>
    public sealed class OfflineClaim
    {
        /// <summary>Which claim, by name; the offline executor knows the names.</summary>
        [JsonPropertyName("claim")]
        public string Claim { get; set; } = string.Empty;

        /// <summary>Why it holds, and why it is worth asking that way.</summary>
        [JsonPropertyName("why")]
        public string Why { get; set; } = string.Empty;
    }

    /// <summary>A file the run should leave behind, and the predicate that reads it.</summary>
    public sealed class ArtefactDefinition
    {
        /// <summary>Its name inside SaveGames.</summary>
        [JsonPropertyName("file")]
        public string File { get; set; } = string.Empty;

        /// <summary>What it proves, in one line.</summary>
        [JsonPropertyName("what")]
        public string What { get; set; } = string.Empty;

        /// <summary>Which predicate reads it, by name.</summary>
        /// <remarks>
        /// A NAME AND NOT THE CHECK ITSELF. What these do - parse the statistics and prove
        /// the counts account for each other, look for an overflow block - is code, and
        /// code does not go in a definition file. Naming it keeps the row readable and
        /// leaves the predicate where it can be compiled; a name with nothing behind it is
        /// refused at load rather than at run.
        /// </remarks>
        [JsonPropertyName("check")]
        public string Check { get; set; } = string.Empty;

        /// <summary>This artefact as the harness checks it.</summary>
        /// <param name="suite">Which suite asked, for the message.</param>
        /// <param name="checks">The predicates, by name.</param>
        /// <returns>The check.</returns>
        /// <exception cref="InvalidDataException">The name has nothing behind it.</exception>
        public SuiteArtefact Build(
            string suite, IReadOnlyDictionary<string, Func<string?, string?>> checks)
        {
            if (!checks.TryGetValue(Check, out Func<string?, string?>? check))
            {
                throw new InvalidDataException(
                    $"{suite} asks for the artefact check '{Check}', and there is no such "
                    + "check. The ones there are: "
                    + string.Join(", ", checks.Keys.OrderBy(k => k, StringComparer.Ordinal))
                    + ".");
            }

            return new SuiteArtefact(File, What, check);
        }
    }

    /// <summary>A line the mod should, or should not, have written.</summary>
    public sealed class LogDefinition
    {
        /// <summary>What to look for.</summary>
        [JsonPropertyName("contains")]
        public string Contains { get; set; } = string.Empty;

        /// <summary>Whether it should be there. Absent means it should.</summary>
        [JsonPropertyName("present")]
        public bool Present { get; set; } = true;

        /// <summary>What its presence or absence proves.</summary>
        [JsonPropertyName("what")]
        public string What { get; set; } = string.Empty;

        /// <summary>This line as the harness checks it.</summary>
        public LogExpectation Build() => new LogExpectation(Contains, Present, What);
    }

    /// <summary>One save, and what the menu it opens must look like.</summary>
    public sealed class ScenarioDefinition
    {
        /// <summary>The staged save's name, without extension.</summary>
        [JsonPropertyName("save")]
        public string Save { get; set; } = string.Empty;

        /// <summary>The conversation to open.</summary>
        [JsonPropertyName("conversation")]
        public int Conversation { get; set; }

        /// <summary>What this scenario is for, in one line.</summary>
        [JsonPropertyName("what")]
        public string What { get; set; } = string.Empty;

        /// <summary>The balance to assert, or null not to.</summary>
        [JsonPropertyName("money")]
        public int? Money { get; set; }

        /// <summary>The clock to assert, in minutes past midnight, or null not to.</summary>
        [JsonPropertyName("dayMinutes")]
        public int? DayMinutes { get; set; }

        /// <summary>Lines of narration before the menu, or null where unmeasured.</summary>
        [JsonPropertyName("advances")]
        public int? Advances { get; set; }

        /// <summary>How much the scenario claims about the markers. Absent means named.</summary>
        [JsonPropertyName("markers")]
        public string Markers { get; set; } = "named";

        /// <summary>What each named option should carry.</summary>
        [JsonPropertyName("options")]
        public List<OptionDefinition>? Options { get; set; }

        /// <summary>How much it claims about the Pass / Fail lines. Absent means ignored.</summary>
        [JsonPropertyName("branches")]
        public string Branches { get; set; } = "ignored";

        /// <summary>What the word "Pass" must be drawn as, under everyCheck.</summary>
        [JsonPropertyName("pass")]
        public BranchShapeHalf? Pass { get; set; }

        /// <summary>What the word "Fail" must be drawn as, under everyCheck.</summary>
        [JsonPropertyName("fail")]
        public BranchShapeHalf? Fail { get; set; }

        /// <summary>Why the lines are what they are, for the report.</summary>
        [JsonPropertyName("branchesWhy")]
        public string BranchesWhy { get; set; } = string.Empty;

        /// <summary>This scenario as the harness runs it.</summary>
        /// <param name="suite">Which suite it belongs to, for the message.</param>
        /// <returns>The scenario.</returns>
        /// <exception cref="InvalidDataException">A policy is not one, or disagrees.</exception>
        public LookAheadScenario Build(string suite)
        {
            BranchPolicy branches = BranchPolicyOf(suite);
            BranchExpectation? expectation = null;

            if (branches == BranchPolicy.EveryCheck)
            {
                if (Pass == null || Fail == null)
                {
                    throw new InvalidDataException(
                        $"{suite}/{Save}: a branch policy of everyCheck needs both a pass "
                        + "and a fail half, and this names "
                        + (Pass == null ? "neither" : "only the pass one") + ".");
                }

                expectation = new BranchExpectation(
                    Pass.Expected(),
                    Fail.Expected(),
                    BranchesWhy.Length > 0
                        ? BranchesWhy
                        : $"{What} - so Pass is {Pass} and Fail is {Fail}");
            }
            else if (Pass != null || Fail != null)
            {
                throw new InvalidDataException(
                    $"{suite}/{Save}: a half is named under a branch policy of "
                    + $"'{Branches}', which asserts nothing about the line - so the half "
                    + "would be read by nobody.");
            }

            return new LookAheadScenario(
                Save,
                Conversation,
                What,
                (Options ?? new List<OptionDefinition>())
                    .Select(option => option.Build(suite, Save))
                    .ToArray(),
                money: Money,
                dayMinutes: DayMinutes,
                markers: MarkerPolicyOf(suite),
                branchPolicy: branches,
                branches: expectation,
                advances: Advances);
        }

        private MarkerPolicy MarkerPolicyOf(string suite) => Markers switch
        {
            "named" => MarkerPolicy.Named,
            "noneAnywhere" => MarkerPolicy.NoneAnywhere,
            "ignored" => MarkerPolicy.Ignored,
            _ => throw new InvalidDataException(
                $"{suite}/{Save}: '{Markers}' is not how much a scenario can claim about "
                + "its markers."),
        };

        private BranchPolicy BranchPolicyOf(string suite) => Branches switch
        {
            "ignored" => BranchPolicy.Ignored,
            "noneAnywhere" => BranchPolicy.NoneAnywhere,
            "everyCheck" => BranchPolicy.EveryCheck,
            _ => throw new InvalidDataException(
                $"{suite}/{Save}: '{Branches}' is not how much a scenario can claim about "
                + "its Pass / Fail lines."),
        };
    }

    /// <summary>What one option in a menu should look like.</summary>
    public sealed class OptionDefinition
    {
        /// <summary>The option's destination entry.</summary>
        [JsonPropertyName("entry")]
        public int Entry { get; set; }

        /// <summary>What it should carry, as the definition spells it.</summary>
        [JsonPropertyName("marker")]
        public string MarkerName { get; set; } = string.Empty;

        /// <summary>Why, in one line, for the report.</summary>
        [JsonPropertyName("why")]
        public string Why { get; set; } = string.Empty;

        /// <summary>This option as the suites express it.</summary>
        /// <param name="suite">Which suite it belongs to, for the message.</param>
        /// <param name="save">And which scenario.</param>
        /// <returns>The expectation.</returns>
        /// <exception cref="InvalidDataException">It names a marker that is not one.</exception>
        public OptionExpectation Build(string suite, string save) =>
            new OptionExpectation(Entry, MarkerOf(suite, save), Why);

        private Marker MarkerOf(string suite, string save) => MarkerName switch
        {
            "none" => Marker.None,
            "orange" => Marker.Orange,
            "red" => Marker.Red,
            "gaveUp" => Marker.Uncertain,
            _ => throw new InvalidDataException(
                $"{suite}/{save}: '{MarkerName}' is not a marker an option can carry."),
        };
    }
}
