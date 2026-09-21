// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using GlobalConversationTracker.Engine;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Does the raw data say what the managed world says, for the same questions?
    /// </summary>
    /// <remarks>
    /// <para>The check de-i5xj.7 exists to pass, and the one that cannot be made anywhere
    /// but inside a running game: <see cref="GameWorld"/> is the world whose
    /// answers reach a player today, and <see cref="LookAheadRequestBuilder"/> is the one that
    /// will. They are asked the same questions here and their answers compared, entry by
    /// entry and name by name.</para>
    ///
    /// <para>Written to the log rather than returned, because the only thing that can run
    /// this is the game and the only thing that can read it afterwards is the harness. That
    /// is the same channel <see cref="NativeEngineCheck"/> uses and for the same reason: a
    /// probe command would need the engine executable deployed beside the PROBE as well as
    /// beside the plugin, which is a second deployment to keep right in order to test the
    /// first.</para>
    ///
    /// <para>NOTHING HERE MAY THROW INTO THE GAME. It is a diagnostic, invoked
    /// deliberately, and a failure to run it is a line in the log rather than a broken
    /// playthrough.</para>
    /// </remarks>
    internal static class WorldRawDataAgreementCheck
    {
        /// <summary>The prefix every line here starts with, so a harness can find them.</summary>
        internal const string LogPrefix = "Snapshot agreement:";

        /// <summary>How many differences are named before the rest are only counted.</summary>
        /// <remarks>
        /// A disagreement is nearly always systematic - one category wrong for every member
        /// of it - so the first few say what the four thousandth would, and a log line per
        /// entry would bury the summary that matters.
        /// </remarks>
        private const int NamedDifferences = 5;

        /// <summary>
        /// Compares the two worlds over one conversation group, and says so in the log.
        /// </summary>
        /// <param name="log">Where the report goes.</param>
        /// <param name="session">The global state, for what other saves have shown.</param>
        /// <param name="modDirectory">Where the mod keeps its own files.</param>
        /// <param name="conversation">Any conversation in the group to compare over.</param>
        internal static void Report(
            IGlobalStateLog log,
            GlobalStateSession session,
            string modDirectory,
            int conversation)
        {
            // Opened per check rather than kept. This runs when a harness asks it to, and
            // the index parse it costs is the price of not holding tens of megabytes for
            // the whole of a playthrough that may never ask again. It stalls the frame it
            // runs in for a second or two, which is a diagnostic being a diagnostic; the
            // shipped path will keep one open (de-i5xj.8).
            using LookAheadIndex? index = LookAheadIndex.Open(
                NativeEngineCheck.PluginDirectory, modDirectory, log);
            if (index == null)
            {
                log.Warning(
                    $"{LogPrefix} no index could be opened; there is nothing to compare "
                    + "against.");
                return;
            }

            try
            {
                // The cache check, which reports its own cost and may rebuild the index
                // from the loaded database. Asked BEFORE the questions are used, because
                // a rebuild replaces the engine and the questions would be about a file
                // that has been superseded.
                index.IsValidFor(index.Engine.QuestionsFor(conversation).Conversations);

                LookAheadQuestions questions = index.Engine.QuestionsFor(conversation);
                LookAheadRequest request =
                    LookAheadRequestBuilder.Build(conversation, questions, session);
                var managed = new GameWorld();

                var differences = new List<string>();
                string report = Compare(questions, request, managed, differences);

                if (differences.Count == 0)
                {
                    log.Info($"{LogPrefix} conversation {conversation}: {report}");
                    return;
                }

                log.Warning(
                    $"{LogPrefix} conversation {conversation}: {report} "
                    + $"FIRST DIFFERENCES: {string.Join("; ", differences)}");
            }
            catch (Exception error)
            {
                log.Warning(
                    $"{LogPrefix} conversation {conversation} could not be compared "
                    + $"({error.GetType().Name}: {error.Message}).");
            }
        }

        /// <summary>Compares every answer, returning the counts and filling in the differences.</summary>
        private static string Compare(
            LookAheadQuestions questions,
            LookAheadRequest request,
            GameWorld managed,
            List<string> differences)
        {
            WorldRawData world = request.World;
            var counts = new List<string>();

            bool situation = managed.Money == world.Money
                && managed.DayMinutes == world.DayMinutes
                && managed.DayCounter == world.DayCounter
                && managed.IsClockLocked == world.ClockLocked;
            Check(
                differences,
                "money and clock",
                situation,
                $"managed {managed.Money}/{managed.DayMinutes}/{managed.DayCounter}/"
                + $"{managed.IsClockLocked}, snapshot {world.Money}/{world.DayMinutes}/"
                + $"{world.DayCounter}/{world.ClockLocked}");
            counts.Add(situation ? "money and clock agree" : "money and clock DIFFER");

            int differing = 0;
            for (int index = 0; index < questions.Variables.Count; index++)
            {
                string name = questions.Variables[index];
                string mine = world.VariableValues[index].ToString();
                string theirs = managed.GetVariable(name).ToString();
                if (mine != theirs)
                {
                    differing++;
                    Check(differences, "variable " + name, false, $"managed {theirs}, snapshot {mine}");
                }
            }

            counts.Add(Count(questions.Variables.Count, differing, "variables"));

            counts.Add(CompareMembership(
                questions.Items, "CheckItem", world.Items, managed, differences));
            counts.Add(CompareMembership(
                questions.Thoughts, "IsTHCPresent", world.Thoughts, managed, differences));

            differing = 0;
            foreach (NodeRef node in questions.Checks)
            {
                Ternary theirs = managed.CheckPasses(
                    new DialogueNodeId(node.Conversation, node.Entry));
                Ternary mine = world.ChecksPass.Contains(node)
                    ? Ternary.True
                    : world.ChecksFail.Contains(node) ? Ternary.False : Ternary.Unknown;
                if (mine != theirs)
                {
                    differing++;
                    Check(differences, "check " + node, false, $"managed {theirs}, snapshot {mine}");
                }
            }

            counts.Add(Count(questions.Checks.Count, differing, "checks"));

            differing = 0;
            foreach (NodeRef node in questions.Entries)
            {
                bool theirs = managed.IsSeen(new DialogueNodeId(node.Conversation, node.Entry));
                if (world.Seen.Contains(node) != theirs)
                {
                    differing++;
                    Check(differences, "seen " + node, false, $"managed {theirs}");
                }
            }

            counts.Add(Count(questions.Entries.Count, differing, "entries"));

            // The queries cannot be compared name for name: the engine hands out a RENDERED
            // call and the managed world takes a name and its arguments, so asking the
            // managed one the same thing would mean parsing the key back apart. What is
            // worth knowing is whether the keys ran at all - a rendering the two sides
            // disagreed about would answer Unknown for every query in the group, silently,
            // and every guard over one would turn permissive.
            int answered = 0;
            foreach (WireValue value in world.QueryValues)
            {
                if (!value.IsUnknown)
                {
                    answered++;
                }
            }

            counts.Add(
                questions.Queries.Count.ToString(CultureInfo.InvariantCulture)
                + " queries (" + answered.ToString(CultureInfo.InvariantCulture) + " answered)");

            return string.Join(", ", counts);
        }

        /// <summary>
        /// Compares one membership set, asking the managed world the same query by name.
        /// </summary>
        /// <remarks>
        /// Through <see cref="GameWorld.Query"/> rather than its <c>HasItem</c>, so that the managed world RENDERS the call itself. That is
        /// the comparison worth making here: both sides turn a name into a Lua call, by
        /// different code, and if the two renderings ever disagreed this is where it would
        /// show.
        /// </remarks>
        private static string CompareMembership(
            IReadOnlyList<string> names,
            string query,
            ISet<string> holding,
            GameWorld managed,
            List<string> differences)
        {
            int differing = 0;
            foreach (string name in names)
            {
                bool theirs = managed
                    .Query(query, new[] { GuardValue.FromText(name) })
                    .AsCondition() == Ternary.True;
                if (holding.Contains(name) != theirs)
                {
                    differing++;
                    Check(
                        differences,
                        query + "(\"" + name + "\")",
                        false,
                        $"managed {theirs}, snapshot {holding.Contains(name)}");
                }
            }

            return Count(names.Count, differing, query);
        }

        private static string Count(int asked, int differing, string what)
        {
            return asked.ToString(CultureInfo.InvariantCulture) + " " + what
                + " (" + differing.ToString(CultureInfo.InvariantCulture) + " differ)";
        }

        /// <summary>Records a difference, up to the first few.</summary>
        private static void Check(
            List<string> differences, string what, bool agreed, string detail)
        {
            if (!agreed && differences.Count < NamedDifferences)
            {
                differences.Add(what + ": " + detail);
            }
        }
    }
}
