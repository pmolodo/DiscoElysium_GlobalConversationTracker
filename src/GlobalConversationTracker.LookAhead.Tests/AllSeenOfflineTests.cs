// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using GlobalConversationTracker.LookAhead;
using GlobalConversationTracker.LookAheadOffline;
using Xunit;
using Xunit.Abstractions;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// The all-seen claim, without a game: when every entry is already recorded, no
    /// option in the biggest conversations is worth crawling.
    /// </summary>
    /// <remarks>
    /// <para>This is the same claim the in-game "all-seen" suite makes, and it is really
    /// a claim about the crawl algorithm rather than about the game, so it does not need
    /// a game to check. The in-game run costs a launch, five save loads and several
    /// minutes of driving the screen; this costs the time to read the index.</para>
    ///
    /// <para>What the in-game suite still earns, and this cannot: that the Harmony patch
    /// is wired up at all, that a real response menu is composed, and that the marker
    /// reaches the text the game draws. Those need the game and stay there.</para>
    ///
    /// <para>Opt-in the same way <see cref="CorpusTests"/> is. The index is extracted
    /// game content and is not committed, so these pass silently where it has not been
    /// generated. Regenerate it with
    /// <c>dotnet run --project tools/DialogueExtract -- conversation-index</c>.</para>
    /// </remarks>
    public class AllSeenOfflineTests
    {
        /// <summary>
        /// The conversations the in-game all-seen suite opens.
        /// </summary>
        /// <remarks>
        /// Repeated rather than shared: they are declared as
        /// <c>LookAheadSuites.BiggestConversations</c> in the harness, which is
        /// net6.0-windows and cannot be referenced from this net10.0 project. Kept in
        /// step by hand; the list is five numbers that change when a save is added.
        /// </remarks>
        private static readonly int[] BiggestConversations = { 368, 14, 631, 28, 1030 };

        private readonly ITestOutputHelper _output;

        public AllSeenOfflineTests(ITestOutputHelper output)
        {
            _output = output;
        }

        [Fact]
        public void NoOptionIsWorthCrawlingWhenEveryEntryIsRecorded()
        {
            ConversationIndex? index = LoadIndex();
            if (index == null)
            {
                _output.WriteLine("conversation_index.jsonl not generated; skipping.");
                return;
            }

            OfflineWorld world = new OfflineWorld(AllSeenState());
            var crawlable = new List<string>();
            int options = 0;

            foreach (int conversationId in BiggestConversations)
            {
                LookAheadGraph graph = index.BuildGraph(conversationId);
                foreach (DialogueNodeId start in index.EntriesIn(conversationId))
                {
                    if (graph.Get(start).IsGroup)
                    {
                        continue;
                    }

                    options++;
                    if (LookAheadEngine.HasPotentialImprovement(
                        graph, world.GetNovelty(start), world.GetNovelty))
                    {
                        crawlable.Add($"{start.ConversationId}:{start.EntryId}");
                    }
                }
            }

            _output.WriteLine(
                $"{options} options over {BiggestConversations.Length} conversations, "
                + $"{crawlable.Count} worth crawling");

            Assert.True(options > 0, "the index yielded no options, so nothing was checked");
            Assert.Equal(new List<string>(), crawlable);
        }

        /// <summary>
        /// The converse, so that the check above cannot pass by being vacuous: leave one
        /// entry unrecorded and the options that can reach it become worth crawling.
        /// </summary>
        [Fact]
        public void AnUnrecordedEntryIsWorthCrawlingFor()
        {
            ConversationIndex? index = LoadIndex();
            if (index == null)
            {
                _output.WriteLine("conversation_index.jsonl not generated; skipping.");
                return;
            }

            const int conversationId = 631;
            LookAheadGraph graph = index.BuildGraph(conversationId);
            IReadOnlyList<DialogueNodeId> entries = index.EntriesIn(conversationId);

            OfflineState state = AllSeenState();
            DialogueNodeId hidden = entries[entries.Count - 1];
            state.GlobalSeen.Remove($"{hidden.ConversationId}:{hidden.EntryId}");
            OfflineWorld world = new OfflineWorld(state);

            int crawlable = 0;
            foreach (DialogueNodeId start in entries)
            {
                if (!graph.Get(start).IsGroup
                    && LookAheadEngine.HasPotentialImprovement(
                        graph, world.GetNovelty(start), world.GetNovelty))
                {
                    crawlable++;
                }
            }

            _output.WriteLine(
                $"with {hidden.ConversationId}:{hidden.EntryId} unrecorded, "
                + $"{crawlable} option(s) are worth crawling");
            Assert.True(
                crawlable > 0,
                "a single unrecorded entry should make at least one option worth crawling, "
                + "or the all-seen result above proves nothing");
        }

        /// <summary>
        /// Every entry of every conversation recorded, and nothing read in the current
        /// save - the offline equivalent of
        /// <c>testing/scenarios/global-state-worst-case.json</c>.
        /// </summary>
        /// <remarks>
        /// It has to cover the WHOLE index, not just the conversation being crawled. A
        /// crawl follows cross-conversation links, so a state that records only the
        /// starting conversation leaves everything beyond it unseen-anywhere, something
        /// always outranks, and every option looks crawlable. That is why the in-game
        /// fixture records all 112,962 entries rather than one conversation's 1,555.
        /// </remarks>
        private static OfflineState AllSeenState()
        {
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (string path in IndexPaths())
            {
                foreach (string line in File.ReadLines(path))
                {
                    AddEntriesOf(line, seen);
                }
            }

            return new OfflineState { GlobalSeen = seen };
        }

        private static void AddEntriesOf(string line, HashSet<string> seen)
        {
            if (string.IsNullOrWhiteSpace(line))
            {
                return;
            }

            using System.Text.Json.JsonDocument document =
                System.Text.Json.JsonDocument.Parse(line);
            System.Text.Json.JsonElement root = document.RootElement;
            int conversationId = root.GetProperty("id").GetInt32();
            if (!root.TryGetProperty("entries", out System.Text.Json.JsonElement entries))
            {
                return;
            }

            foreach (System.Text.Json.JsonElement entry in entries.EnumerateArray())
            {
                seen.Add($"{conversationId}:{entry.GetProperty("id").GetInt32()}");
            }
        }

        private static ConversationIndex? LoadIndex()
        {
            foreach (string path in IndexPaths())
            {
                return ConversationIndex.Read(path);
            }

            return null;
        }

        /// <summary>The generated index, if it has been generated.</summary>
        private static IEnumerable<string> IndexPaths()
        {
            DirectoryInfo? directory = new DirectoryInfo(AppContext.BaseDirectory);
            while (directory != null)
            {
                string candidate = Path.Combine(
                    directory.FullName,
                    ".game_reference_copies", "derived", "conversation_index.jsonl");
                if (File.Exists(candidate))
                {
                    yield return candidate;
                    yield break;
                }

                directory = directory.Parent;
            }
        }
    }
}
