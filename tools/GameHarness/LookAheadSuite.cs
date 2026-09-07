// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Linq;

namespace GlobalConversationTracker.Harness
{
    /// <summary>What a look-ahead marker on one option should be.</summary>
    public enum Marker
    {
        /// <summary>No marker: nothing reachable outranks the option itself.</summary>
        None = 0,

        /// <summary>Orange: it can still reach a line no save has read.</summary>
        Orange = 1,

        /// <summary>Red: it can still reach a line this save has not read.</summary>
        Red = 2,

        /// <summary>
        /// Grey: the search gave up before it could say, so nothing is claimed either way.
        /// </summary>
        /// <remarks>
        /// Distinct from <see cref="None"/> on purpose. None is an answer - nothing
        /// reachable outranks the option - and this is the absence of one, which the mod
        /// used to draw identically. A suite that could not tell them apart would read a
        /// crawl that ran out of budget as a crawl that found nothing.
        /// </remarks>
        Uncertain = 3,
    }

    /// <summary>
    /// What colour a word of an option's Pass / Fail line should be drawn in.
    /// </summary>
    /// <remarks>
    /// The word's colour is where that OUTCOME lands, by the same three-rung rule an
    /// option's own colour follows - so two of these are the colours <see cref="Marker"/>
    /// already names. The third is not: an option never needs a colour for "already
    /// read", because the game draws a spent option itself, and the line does, because a
    /// word in no colour at all reads as a missing answer rather than a read one.
    /// </remarks>
    public enum BranchColour
    {
        /// <summary>Orange: the outcome lands on a line no save has read.</summary>
        Orange = 0,

        /// <summary>Red: it lands on a line this save has not read.</summary>
        Red = 1,

        /// <summary>Dark red: it lands on a line this save has already read.</summary>
        DarkRed = 2,
    }

    /// <summary>One half of a Pass / Fail line: where it lands, and what lies beyond.</summary>
    public readonly struct BranchHalf : IEquatable<BranchHalf>
    {
        /// <summary>Creates a half.</summary>
        /// <param name="colour">Where the outcome lands.</param>
        /// <param name="marker">What lies beyond it, or None for nothing.</param>
        public BranchHalf(BranchColour colour, Marker marker = Marker.None)
        {
            Colour = colour;
            Marker = marker;
        }

        /// <summary>Where the outcome lands.</summary>
        public BranchColour Colour { get; }

        /// <summary>What lies beyond it.</summary>
        public Marker Marker { get; }

        /// <inheritdoc/>
        public bool Equals(BranchHalf other) =>
            Colour == other.Colour && Marker == other.Marker;

        /// <inheritdoc/>
        public override bool Equals(object? obj) => obj is BranchHalf other && Equals(other);

        /// <inheritdoc/>
        public override int GetHashCode() => ((int)Colour * 4) + (int)Marker;

        /// <inheritdoc/>
        public override string ToString() =>
            Marker == Marker.None ? $"{Colour}" : $"{Colour} with {Marker}";
    }

    /// <summary>What every rolled check in a menu should be drawn with.</summary>
    /// <remarks>
    /// A MENU-WIDE CLAIM RATHER THAN A PER-ENTRY ONE, and that is not a convenience. Which
    /// options a conversation offers is not stable between runs - the same save opened at
    /// the ceiling fan gave four options one day and one the next - so a scenario cannot
    /// name the check it expects to see. What it can do is state the rule the mod is meant
    /// to follow and let it apply to whatever the menu turns out to hold, which is a
    /// stronger claim anyway: every check, not one that was known about in advance.
    /// </remarks>
    public sealed class BranchExpectation
    {
        /// <summary>Creates an expectation.</summary>
        /// <param name="pass">The half naming the outcome where the check succeeds.</param>
        /// <param name="fail">The half naming the outcome where it fails.</param>
        /// <param name="why">Why, in one line, for the report.</param>
        public BranchExpectation(BranchHalf pass, BranchHalf fail, string why)
        {
            Pass = pass;
            Fail = fail;
            Why = why ?? throw new ArgumentNullException(nameof(why));
        }

        /// <summary>The outcome where the check succeeds.</summary>
        public BranchHalf Pass { get; }

        /// <summary>The outcome where it fails.</summary>
        public BranchHalf Fail { get; }

        /// <summary>Why, for the report.</summary>
        public string Why { get; }
    }

    /// <summary>How much a scenario claims about the Pass / Fail lines in its menu.</summary>
    public enum BranchPolicy
    {
        /// <summary>
        /// The lines are not the subject, and nothing about them is asserted.
        /// </summary>
        /// <remarks>
        /// The default, and honest for every scenario written before the line existed: a
        /// menu holding a check the scenario never arranged would otherwise be claimed
        /// about by a suite that is asking a different question entirely.
        /// </remarks>
        Ignored = 0,

        /// <summary>
        /// Nothing in the menu carries a line, whatever the menu turns out to hold.
        /// </summary>
        /// <remarks>
        /// For a conversation with no rolled check in it, where the claim is that the mod
        /// does not invent one - and for the feature switched off, where the claim is that
        /// a check gets nothing either.
        /// </remarks>
        NoneAnywhere = 1,

        /// <summary>
        /// Every rolled check carries the expected line, and nothing else carries one.
        /// </summary>
        /// <remarks>
        /// The negative half is the load-bearing one. A line drawn on an option that rolls
        /// nothing would be inventing two outcomes where the game has one, and would read
        /// perfectly well while doing it.
        /// </remarks>
        EveryCheck = 2,
    }

    /// <summary>How much a scenario claims about the markers in its menu.</summary>
    public enum MarkerPolicy
    {
        /// <summary>
        /// Named options must match, and every other option must be unmarked.
        /// </summary>
        Named = 0,

        /// <summary>
        /// Nothing in the menu should be marked, whatever it offers. For a conversation
        /// whose option ids are not known until it has been opened once.
        /// </summary>
        NoneAnywhere = 1,

        /// <summary>
        /// The markers are not the subject. For a scenario that exists to measure what a
        /// crawl costs, where what gets marked depends on parts of the database the
        /// scenario has not arranged and asserting it would be inventing a claim.
        /// </summary>
        Ignored = 2,
    }

    /// <summary>What one option in a menu should look like.</summary>
    public sealed class OptionExpectation
    {
        /// <summary>Creates an expectation.</summary>
        /// <param name="entryId">The option's destination entry.</param>
        /// <param name="marker">What it should carry.</param>
        /// <param name="why">Why, in one line, for the report.</param>
        public OptionExpectation(int entryId, Marker marker, string why)
        {
            EntryId = entryId;
            Marker = marker;
            Why = why;
        }

        /// <summary>The option's destination entry.</summary>
        public int EntryId { get; }

        /// <summary>What it should carry.</summary>
        public Marker Marker { get; }

        /// <summary>Why, for the report.</summary>
        public string Why { get; }
    }

    /// <summary>One save, and what the menu it opens should look like.</summary>
    /// <remarks>
    /// Expectations are per entry rather than "something is marked somewhere". The
    /// difference matters: a scan that marked the wrong option would satisfy the coarse
    /// form, and the probe already reports every option with its entry id, so naming
    /// them costs nothing and says what the scenario is actually about.
    /// </remarks>
    public sealed class LookAheadScenario
    {
        /// <summary>Creates a scenario.</summary>
        /// <param name="saveName">The staged save's name, without extension.</param>
        /// <param name="conversationId">The conversation to open.</param>
        /// <param name="why">What this scenario is for, in one line.</param>
        /// <param name="options">What each named option should carry.</param>
        /// <param name="money">The balance to assert, or null not to.</param>
        /// <param name="dayMinutes">The clock to assert, or null not to.</param>
        /// <param name="markers">How much the scenario claims about the markers.</param>
        /// <param name="branchPolicy">How much it claims about the Pass / Fail lines.</param>
        /// <param name="branches">What every check's line should be, under EveryCheck.</param>
        /// <param name="advances">Lines to advance before its menu, or null if unmeasured.</param>
        /// <param name="killEngineFirst">
        /// Kill the look-ahead engine before opening this scenario's conversation, so what
        /// the scenario expects is what the mod does WITHOUT one. See de-bnjy.1.2.4: a
        /// running game is the only place the out-of-process arrangement can be tested
        /// under the failure it exists to survive.
        /// </param>
        /// <param name="expectsRecovery">
        /// That the killed engine will be REPLACED rather than mourned, so this scenario
        /// waits for the replacement instead of for the shutdown notice. de-bnjy.1.3: with
        /// the shipped policy a kill produces a new engine and no notice at all, and a
        /// scenario that waited for the window would wait for ever.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ArgumentException">The policy and the expectation disagree.</exception>
        public LookAheadScenario(
            string saveName,
            int conversationId,
            string why,
            IReadOnlyList<OptionExpectation> options,
            int? money = null,
            int? dayMinutes = null,
            MarkerPolicy markers = MarkerPolicy.Named,
            BranchPolicy branchPolicy = BranchPolicy.Ignored,
            BranchExpectation? branches = null,
            int? advances = null,
            bool killEngineFirst = false,
            bool expectsRecovery = false)
        {
            if (expectsRecovery && !killEngineFirst)
            {
                throw new ArgumentException(
                    "A scenario that expects a recovery has to kill something first; "
                    + $"set {nameof(killEngineFirst)} as well.",
                    nameof(expectsRecovery));
            }

            if (advances < 0)
            {
                throw new ArgumentOutOfRangeException(
                    nameof(advances), advances, "A conversation cannot advance backwards.");
            }

            if ((branchPolicy == BranchPolicy.EveryCheck) != (branches != null))
            {
                throw new ArgumentException(
                    $"{nameof(BranchPolicy)}.{BranchPolicy.EveryCheck} needs a line to "
                    + "expect, and every other policy needs none.",
                    nameof(branches));
            }

            SaveName = saveName ?? throw new ArgumentNullException(nameof(saveName));
            ConversationId = conversationId;
            Why = why ?? throw new ArgumentNullException(nameof(why));
            Options = options ?? throw new ArgumentNullException(nameof(options));
            Money = money;
            DayMinutes = dayMinutes;
            Markers = markers;
            Branches = branches;
            BranchPolicy = branchPolicy;
            Advances = advances;
            KillEngineFirst = killEngineFirst;
            ExpectsRecovery = expectsRecovery;
        }

        /// <summary>The staged save's name, without extension.</summary>
        public string SaveName { get; }

        /// <summary>The conversation to open.</summary>
        public int ConversationId { get; }

        /// <summary>What this scenario is for.</summary>
        public string Why { get; }

        /// <summary>What each named option should carry.</summary>
        public IReadOnlyList<OptionExpectation> Options { get; }

        /// <summary>
        /// Whether to kill the look-ahead engine before opening this conversation.
        /// </summary>
        /// <remarks>
        /// Once killed it stays dead for the rest of the run, deliberately - that is what
        /// the mod does about it and the reason the user asked for a game restart rather
        /// than a respawn. So a suite using this puts the scenario that needs a live engine
        /// FIRST, and everything after the kill is about a mod with none.
        /// </remarks>
        public bool KillEngineFirst { get; }

        /// <summary>
        /// Whether the killed engine is expected to be REPLACED rather than mourned.
        /// </summary>
        /// <remarks>
        /// de-bnjy.1.3 made a death be answered with a fresh engine, so the two things a
        /// suite can ask about a kill are now different runs: the shutdown NOTICE (which
        /// needs TestRecoveryLimit = 0 to be reachable at all) and the RECOVERY. This says
        /// which one this scenario is about, and so whether the harness waits for a window
        /// to dismiss or for a new engine to arrive.
        /// </remarks>
        public bool ExpectsRecovery { get; }

        /// <summary>The balance to assert, or null not to.</summary>
        public int? Money { get; }

        /// <summary>The clock to assert, in minutes past midnight, or null not to.</summary>
        public int? DayMinutes { get; }

        /// <summary>How much the scenario claims about the markers.</summary>
        public MarkerPolicy Markers { get; }

        /// <summary>What every rolled check's line should be, or null when nothing is claimed.</summary>
        public BranchExpectation? Branches { get; }

        /// <summary>How much the scenario claims about those lines.</summary>
        public BranchPolicy BranchPolicy { get; }

        /// <summary>
        /// How many lines of narration stand between opening this conversation and its
        /// first response menu, or null where it has not been measured yet.
        /// </summary>
        /// <remarks>
        /// A PROPERTY OF THE SCENARIO, and the thing that makes a run repeatable. A
        /// conversation opens on however much narration its writer put there, and the run
        /// answers one line with one Enter - so this number is fixed for a given save and
        /// conversation, and a run that needs a different one has not arrived where the
        /// scenario says it has. Measure it by running: the report names the count it
        /// actually took.
        /// </remarks>
        public int? Advances { get; }

        /// <summary>Whether the scenario says anything about an entry.</summary>
        /// <param name="entryId">The entry, which may be unreadable.</param>
        public bool Names(int? entryId) =>
            entryId is int id && Options.Any(o => o.EntryId == id);
    }

    /// <summary>A line the mod should, or should not, have written.</summary>
    /// <remarks>
    /// Some of what a setting does is only visible here. Turning the feature off and
    /// starving it of budget both leave every option unmarked, and the only thing that
    /// tells them apart is whether the hook was installed at all - which the mod says
    /// once, at load, and nowhere else.
    /// </remarks>
    public sealed class LogExpectation
    {
        /// <summary>Creates an expectation about the log.</summary>
        /// <param name="substring">What to look for.</param>
        /// <param name="shouldAppear">Whether it should be there.</param>
        /// <param name="what">What its presence or absence proves.</param>
        /// <param name="times">
        /// Exactly how many times it should appear, or null to ask only whether it does.
        /// Meaningless with <paramref name="shouldAppear"/> false, which is already a
        /// count of zero.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ArgumentException">A count is given for an absence.</exception>
        public LogExpectation(
            string substring, bool shouldAppear, string what, int? times = null)
        {
            if (times != null && !shouldAppear)
            {
                throw new ArgumentException(
                    "An expectation that something is absent already says how many times "
                    + "it appears.",
                    nameof(times));
            }

            if (times < 1)
            {
                throw new ArgumentOutOfRangeException(
                    nameof(times), times, "Use shouldAppear: false to expect none.");
            }

            Substring = substring ?? throw new ArgumentNullException(nameof(substring));
            ShouldAppear = shouldAppear;
            What = what ?? throw new ArgumentNullException(nameof(what));
            Times = times;
        }

        /// <summary>What to look for.</summary>
        public string Substring { get; }

        /// <summary>Whether it should be there.</summary>
        public bool ShouldAppear { get; }

        /// <summary>What its presence or absence proves.</summary>
        public string What { get; }

        /// <summary>
        /// Exactly how many times it should appear, or null to ask only whether it does.
        /// </summary>
        /// <remarks>
        /// For the messages whose POINT is that they are said once. A warning that the
        /// look-ahead has stopped is worth nothing if it is repeated on every response
        /// menu afterwards - that is noise where the silence it replaced was merely
        /// unhelpful - so "once" is the claim, and a presence check cannot make it.
        /// </remarks>
        public int? Times { get; }
    }

    /// <summary>A file the run should leave in the profile's SaveGames folder.</summary>
    /// <remarks>
    /// Checked before the profile is put back, because that is when it exists. The mod's
    /// diagnostics are written there and are the only evidence for some of what the
    /// look-ahead does - a budget overflow leaves no other trace, since the feature's
    /// response to running out is to say nothing.
    /// </remarks>
    public sealed class SuiteArtefact
    {
        /// <summary>Creates an artefact check.</summary>
        /// <param name="fileName">Its name inside SaveGames.</param>
        /// <param name="what">What it proves, in one line.</param>
        /// <param name="check">
        /// Given the file's contents, or null when it was never written; returns null
        /// when that is right, else why not. Absence is passed rather than failed
        /// because a file the mod declines to write can be the thing being checked.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public SuiteArtefact(string fileName, string what, Func<string?, string?> check)
        {
            FileName = fileName ?? throw new ArgumentNullException(nameof(fileName));
            What = what ?? throw new ArgumentNullException(nameof(what));
            Check = check ?? throw new ArgumentNullException(nameof(check));
        }

        /// <summary>Its name inside SaveGames.</summary>
        public string FileName { get; }

        /// <summary>What it proves.</summary>
        public string What { get; }

        /// <summary>Given the contents or null; returns null when right, else why not.</summary>
        public Func<string?, string?> Check { get; }
    }

    /// <summary>
    /// A set of scenarios that can share one launch of the game.
    /// </summary>
    /// <remarks>
    /// <para>The unit is a launch, and what fixes it is the global state. The mod reads
    /// global-conversation-state.json once, when it first needs it, and never again - so
    /// scenarios that need to disagree about what other saves have seen cannot share a
    /// session, however much they otherwise have in common. The same goes for the mod's
    /// config: BepInEx reads it at chainload.</para>
    ///
    /// <para>What CAN vary within a suite is the save, which the probe loads in place,
    /// and the conversation, which it opens on command. That is where the minute a cold
    /// start costs is saved.</para>
    /// </remarks>
    public sealed class LookAheadSuite
    {
        /// <summary>Creates a suite.</summary>
        /// <param name="name">What to call it on the command line.</param>
        /// <param name="what">What the suite is for, in one line.</param>
        /// <param name="globalStateFile">
        /// The global state to stage, relative to the scenario root.
        /// </param>
        /// <param name="scenarios">The scenarios, in the order they run.</param>
        /// <param name="pluginSettings">Mod settings to change for the run, or null.</param>
        /// <param name="artefacts">Files the run should leave behind, or null.</param>
        /// <param name="logExpectations">What the mod should have logged, or null.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public LookAheadSuite(
            string name,
            string what,
            string globalStateFile,
            IReadOnlyList<LookAheadScenario> scenarios,
            IReadOnlyDictionary<string, string>? pluginSettings = null,
            IReadOnlyList<SuiteArtefact>? artefacts = null,
            IReadOnlyList<LogExpectation>? logExpectations = null)
        {
            Name = name ?? throw new ArgumentNullException(nameof(name));
            What = what ?? throw new ArgumentNullException(nameof(what));
            GlobalStateFile = globalStateFile
                ?? throw new ArgumentNullException(nameof(globalStateFile));
            Scenarios = scenarios ?? throw new ArgumentNullException(nameof(scenarios));
            PluginSettings = pluginSettings ?? new Dictionary<string, string>();
            Artefacts = artefacts ?? Array.Empty<SuiteArtefact>();
            LogExpectations = logExpectations ?? Array.Empty<LogExpectation>();
        }

        /// <summary>What to call it on the command line.</summary>
        public string Name { get; }

        /// <summary>What the suite is for.</summary>
        public string What { get; }

        /// <summary>The global state to stage, relative to the scenario root.</summary>
        public string GlobalStateFile { get; }

        /// <summary>The scenarios, in the order they run.</summary>
        public IReadOnlyList<LookAheadScenario> Scenarios { get; }

        /// <summary>
        /// Whether running this suite kills a look-ahead engine at all.
        /// </summary>
        /// <remarks>
        /// <para>What <see cref="LookAheadSuites.InRunOrder"/> schedules on: a suite that
        /// kills anything runs after the suites that kill nothing, so that if its killing
        /// or its recovery misbehaves the damage is at the end of the run rather than in
        /// the middle of it.</para>
        ///
        /// <para>NOT THE SAME QUESTION AS <see cref="EndsTheLookAhead"/> since de-bnjy.1.3.
        /// A killed engine is now REPLACED, so killing one is no longer the same as ending
        /// the session's look-ahead - only a suite that also stops the replacement does
        /// that.</para>
        ///
        /// <para>Computed from the scenarios rather than declared beside them, so it cannot
        /// disagree with what the suite actually does.</para>
        /// </remarks>
        public bool KillsTheEngine => Scenarios.Any(scenario => scenario.KillEngineFirst);

        /// <summary>
        /// Whether running this suite leaves the game without a look-ahead engine.
        /// </summary>
        /// <remarks>
        /// <para>THE ONE THAT DECIDES WHETHER A LATER SUITE CAN MEAN ANYTHING. A suite that
        /// ends the look-ahead makes every claim about a marker or a Pass / Fail line fail
        /// for every suite after it in the same launch - measured 2026-09-05, when
        /// engine-death ran seventh and the eight branch-shape suites after it lost all
        /// sixteen of their claims.</para>
        ///
        /// <para>A suite whose kill scenarios all expect a RECOVERY does not end it: it
        /// waits for the replacement and hands the next suite a working engine.</para>
        /// </remarks>
        public bool EndsTheLookAhead =>
            Scenarios.Any(scenario => scenario.KillEngineFirst && !scenario.ExpectsRecovery);

        /// <summary>Mod settings to change for the run.</summary>
        public IReadOnlyDictionary<string, string> PluginSettings { get; }

        /// <summary>Files the run should leave behind, checked before the restore.</summary>
        public IReadOnlyList<SuiteArtefact> Artefacts { get; }

        /// <summary>What the mod should, and should not, have logged.</summary>
        public IReadOnlyList<LogExpectation> LogExpectations { get; }

        /// <summary>Whether this is part of a suite rather than the whole of one.</summary>
        /// <remarks>
        /// Only ever true on a copy made by <see cref="WithScenarios"/>. The run says so
        /// out loud, because a partial suite makes fewer claims than its name implies and
        /// a reader of the output should not have to know which.
        /// </remarks>
        public bool Filtered { get; private set; }

        /// <summary>The same suite over a subset of its scenarios.</summary>
        /// <remarks>
        /// <para>THE WHOLE-SUITE CHECKS ARE DROPPED, deliberately. An artefact check reads
        /// a file the mod wrote over the course of the whole suite - the statistics, the
        /// overflow log - and a log expectation is about everything the run said. Both are
        /// claims about the complete set of scenarios, so keeping them under a filter would
        /// fail a run that is behaving perfectly, and a caller cannot tell that kind of
        /// failure from a real one without knowing which check belongs to which scenario.
        /// Dropping them makes the filtered run a fast way to ask about a menu, and leaves
        /// the whole run as the thing that validates.</para>
        ///
        /// <para>The plugin settings and the global state come along, because those are
        /// what make the scenario mean what it means.</para>
        /// </remarks>
        /// <param name="scenarios">The scenarios to keep, in the order they run.</param>
        /// <returns>A copy carrying only those.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="scenarios"/> is null.</exception>
        public LookAheadSuite WithScenarios(IReadOnlyList<LookAheadScenario> scenarios)
        {
            if (scenarios == null)
            {
                throw new ArgumentNullException(nameof(scenarios));
            }

            return new LookAheadSuite(Name, What, GlobalStateFile, scenarios, PluginSettings)
            {
                Filtered = true,
            };
        }
    }
}
