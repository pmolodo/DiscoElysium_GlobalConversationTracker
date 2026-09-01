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
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public LookAheadScenario(
            string saveName,
            int conversationId,
            string why,
            IReadOnlyList<OptionExpectation> options,
            int? money = null,
            int? dayMinutes = null)
        {
            SaveName = saveName ?? throw new ArgumentNullException(nameof(saveName));
            ConversationId = conversationId;
            Why = why ?? throw new ArgumentNullException(nameof(why));
            Options = options ?? throw new ArgumentNullException(nameof(options));
            Money = money;
            DayMinutes = dayMinutes;
        }

        /// <summary>The staged save's name, without extension.</summary>
        public string SaveName { get; }

        /// <summary>The conversation to open.</summary>
        public int ConversationId { get; }

        /// <summary>What this scenario is for.</summary>
        public string Why { get; }

        /// <summary>What each named option should carry.</summary>
        public IReadOnlyList<OptionExpectation> Options { get; }

        /// <summary>The balance to assert, or null not to.</summary>
        public int? Money { get; }

        /// <summary>The clock to assert, in minutes past midnight, or null not to.</summary>
        public int? DayMinutes { get; }

        /// <summary>Whether the scenario says anything about an entry.</summary>
        /// <param name="entryId">The entry, which may be unreadable.</param>
        public bool Names(int? entryId) =>
            entryId is int id && Options.Any(o => o.EntryId == id);
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
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public LookAheadSuite(
            string name,
            string what,
            string globalStateFile,
            IReadOnlyList<LookAheadScenario> scenarios,
            IReadOnlyDictionary<string, string>? pluginSettings = null)
        {
            Name = name ?? throw new ArgumentNullException(nameof(name));
            What = what ?? throw new ArgumentNullException(nameof(what));
            GlobalStateFile = globalStateFile
                ?? throw new ArgumentNullException(nameof(globalStateFile));
            Scenarios = scenarios ?? throw new ArgumentNullException(nameof(scenarios));
            PluginSettings = pluginSettings ?? new Dictionary<string, string>();
        }

        /// <summary>What to call it on the command line.</summary>
        public string Name { get; }

        /// <summary>What the suite is for.</summary>
        public string What { get; }

        /// <summary>The global state to stage, relative to the scenario root.</summary>
        public string GlobalStateFile { get; }

        /// <summary>The scenarios, in the order they run.</summary>
        public IReadOnlyList<LookAheadScenario> Scenarios { get; }

        /// <summary>Mod settings to change for the run.</summary>
        public IReadOnlyDictionary<string, string> PluginSettings { get; }
    }
}
