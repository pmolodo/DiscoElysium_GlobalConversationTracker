// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using GlobalConversationTracker.LookAhead;
using Xunit;
using Xunit.Abstractions;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// Runs the parsers over every distinct guard and action in the shipped dialogue
    /// database, rather than over cases someone thought to write down.
    /// </summary>
    /// <remarks>
    /// <para>Opt-in. The corpus is extracted game content, so it is not committed - the
    /// repo keeps everything under <c>.game_reference_copies/</c> out of git, and no
    /// tracked file here exceeds half a megabyte. These tests find the corpus if it has
    /// been generated and pass silently if it has not, so a checkout without a game
    /// install still runs green.</para>
    ///
    /// <para>Regenerate with <c>extract_dialogue_corpus.py</c> at the repo root. The
    /// parser was developed against this corpus and handled 100% of it; a failure here
    /// means either a regression or a database this build has not seen.</para>
    /// </remarks>
    public class CorpusTests
    {
        private const string GuardCorpus = "distinct_guards.txt";
        private const string ActionCorpus = "distinct_scripts.txt";

        private readonly ITestOutputHelper _output;

        public CorpusTests(ITestOutputHelper output)
        {
            _output = output;
        }

        [Fact]
        public void EveryGuardInTheDatabaseParses()
        {
            IReadOnlyList<string>? corpus = Load(GuardCorpus);
            if (corpus == null)
            {
                _output.WriteLine($"{GuardCorpus} not generated; skipping.");
                return;
            }

            var failures = new List<string>();
            foreach (string line in corpus)
            {
                string guard = Unescape(line);
                if (!GuardParser.TryParse(guard, out _))
                {
                    failures.Add(guard);
                }
            }

            _output.WriteLine($"parsed {corpus.Count - failures.Count} of {corpus.Count} guards");
            Assert.Equal(new List<string>(), Sample(failures));
        }

        /// <summary>
        /// Every guard must also evaluate without throwing, against a world that knows
        /// nothing. Everything should come back Unknown or a definite value - never an
        /// exception, because the engine runs this inside a UI callback.
        /// </summary>
        [Fact]
        public void EveryGuardEvaluatesWithoutThrowing()
        {
            IReadOnlyList<string>? corpus = Load(GuardCorpus);
            if (corpus == null)
            {
                _output.WriteLine($"{GuardCorpus} not generated; skipping.");
                return;
            }

            var world = new FakeWorld();
            var symbols = new StateSymbols();
            var graph = new LookAheadGraph(Array.Empty<LookAheadNode>(), symbols);
            var context = new EmptyContext(world);

            var counts = new Dictionary<Ternary, int>
            {
                [Ternary.True] = 0,
                [Ternary.False] = 0,
                [Ternary.Unknown] = 0,
            };

            foreach (string line in corpus)
            {
                if (GuardParser.TryParse(Unescape(line), out GuardExpression expression))
                {
                    counts[expression.Test(context)]++;
                }
            }

            _output.WriteLine(
                $"true {counts[Ternary.True]}, false {counts[Ternary.False]}, "
                + $"unknown {counts[Ternary.Unknown]}");
            Assert.True(graph.Count == 0);
        }

        [Fact]
        public void EveryActionInTheDatabaseParses()
        {
            IReadOnlyList<string>? corpus = Load(ActionCorpus);
            if (corpus == null)
            {
                _output.WriteLine($"{ActionCorpus} not generated; skipping.");
                return;
            }

            var symbols = new StateSymbols();
            int modelled = 0;
            int unmodelled = 0;
            var failures = new List<string>();

            foreach (string line in corpus)
            {
                string script = Unescape(line);
                try
                {
                    foreach (DialogueAction action in ActionParser.Parse(script, symbols))
                    {
                        if (action.Kind == DialogueActionKind.Unmodelled)
                        {
                            unmodelled++;
                        }
                        else
                        {
                            modelled++;
                        }
                    }
                }
                catch (Exception error)
                {
                    failures.Add(script + " -> " + error.Message);
                }
            }

            _output.WriteLine(
                $"{corpus.Count} scripts: {modelled} modelled actions, "
                + $"{unmodelled} unmodelled, {symbols.Count} slots");
            Assert.Equal(new List<string>(), Sample(failures));
        }

        /// <summary>
        /// Applying every action in the database must leave money non-negative. This is
        /// the invariant the search's termination argument rests on.
        /// </summary>
        [Fact]
        public void NoActionDrivesMoneyNegative()
        {
            IReadOnlyList<string>? corpus = Load(ActionCorpus);
            if (corpus == null)
            {
                _output.WriteLine($"{ActionCorpus} not generated; skipping.");
                return;
            }

            var symbols = new StateSymbols();
            int once = symbols.Once(new DialogueNodeId(0, 0));
            LookAheadState state = LookAheadState.Empty(symbols.Count, 0);

            foreach (string line in corpus)
            {
                IReadOnlyList<DialogueAction> actions =
                    ActionParser.Parse(Unescape(line), symbols);
                LookAheadState after = DialogueAction.Apply(actions, state, once, 16);
                Assert.True(after.Money >= 0, "money went negative on: " + Unescape(line));
            }
        }

        /// <summary>Keeps a failure message readable when a whole corpus regresses.</summary>
        private static List<string> Sample(List<string> failures)
        {
            const int Limit = 10;
            if (failures.Count <= Limit)
            {
                return failures;
            }

            var sample = failures.GetRange(0, Limit);
            sample.Add($"... and {failures.Count - Limit} more");
            return sample;
        }

        /// <summary>
        /// Reverses the escaping <c>extract_dialogue_corpus.py</c> applies. Done as a
        /// scan rather than chained Replace calls, because a script containing a literal
        /// backslash followed by 'n' must not turn into a newline.
        /// </summary>
        private static string Unescape(string line)
        {
            if (line.IndexOf('\\') < 0)
            {
                return line;
            }

            var builder = new System.Text.StringBuilder(line.Length);
            for (int i = 0; i < line.Length; i++)
            {
                if (line[i] != '\\' || i + 1 >= line.Length)
                {
                    builder.Append(line[i]);
                    continue;
                }

                char next = line[++i];
                switch (next)
                {
                    case 'n':
                        builder.Append('\n');
                        break;
                    case 'r':
                        builder.Append('\r');
                        break;
                    case '\\':
                        builder.Append('\\');
                        break;
                    default:
                        builder.Append('\\').Append(next);
                        break;
                }
            }

            return builder.ToString();
        }

        /// <summary>
        /// Finds a corpus file by walking up from the test assembly to the repo root.
        /// </summary>
        private static IReadOnlyList<string>? Load(string fileName)
        {
            var directory = new DirectoryInfo(AppContext.BaseDirectory);
            while (directory != null)
            {
                string candidate = Path.Combine(
                    directory.FullName, ".game_reference_copies", "derived", fileName);
                if (File.Exists(candidate))
                {
                    return File.ReadAllLines(candidate);
                }

                directory = directory.Parent;
            }

            return null;
        }

        /// <summary>A world that knows nothing, so every query comes back Unknown.</summary>
        private sealed class EmptyContext : IGuardContext
        {
            private readonly FakeWorld _world;

            public EmptyContext(FakeWorld world)
            {
                _world = world;
            }

            public GuardValue GetVariable(string name)
            {
                return _world.GetVariable(name);
            }

            public GuardValue Query(string name, IReadOnlyList<GuardValue> arguments)
            {
                return _world.Query(name, arguments);
            }
        }
    }
}
