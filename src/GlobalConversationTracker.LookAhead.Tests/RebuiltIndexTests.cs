// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.IO;
using GlobalConversationTracker.Engine;
using Xunit;
using Xunit.Abstractions;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// An index written the way the plugin rebuilds one, opened by the engine that reads it.
    /// </summary>
    /// <remarks>
    /// <para>The shipped index is a cache, and when it turns out to describe a different
    /// game the plugin writes a new one from the database in front of it. That writer has
    /// one job - produce a file the Rust engine reads the same way it reads the extractor's
    /// - and the only thing that can confirm it is the Rust engine.</para>
    ///
    /// <para>So this writes an index, opens it with the real library, and asks it questions
    /// whose answers are decided by the content that went in. What it does NOT need is the
    /// game: the writer takes plain data, which is why it takes plain data.</para>
    /// </remarks>
    public class RebuiltIndexTests
    {
        private readonly ITestOutputHelper _output;

        public RebuiltIndexTests(ITestOutputHelper output)
        {
            _output = output;
            NativeLookAhead.Install();
        }

        /// <summary>
        /// An index this writer produced opens, and the engine finds in it what was put in.
        /// </summary>
        [Fact]
        public void AnIndexWrittenHereOpensAndAnswers()
        {
            if (NativeLookAhead.Engine == null)
            {
                _output.WriteLine("the engine is not built; skipping. Run: cargo build");
                return;
            }

            string path = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName() + ".jsonl");
            try
            {
                int written = ShippedIndexWriter.Write(path, Built());
                _output.WriteLine(File.ReadAllText(path));
                Assert.Equal(2, written);

                using LookAheadLibrary engine = LookAheadLibrary.Open(path);

                Assert.Equal(ShippedIndexWriter.FormatVersion, engine.IndexFormat);
                Assert.Equal(2, engine.ConversationCount);
                Assert.Equal(3, engine.EntryCount(900));
                Assert.Equal(-1, engine.EntryCount(-1));

                // The link out of 900 pulls 901 into the group, which is the thing a
                // rebuilt index has to get right for a crawl to reach anything at all.
                LookAheadQuestions questions = engine.QuestionsFor(900);
                Assert.Contains(901, questions.Conversations);
                Assert.Contains("jam.asked", questions.Variables);
                Assert.Contains(new NodeRef(900, 2), questions.Checks);
            }
            finally
            {
                File.Delete(path);
            }
        }

        /// <summary>
        /// The hash the writer stored is the hash the engine hands back.
        /// </summary>
        /// <remarks>
        /// The round trip the whole cache check rests on. The plugin computes a hash from
        /// the live database and compares it against what the library reports; if writing
        /// and reading did not preserve it, every launch would rebuild.
        /// </remarks>
        [Fact]
        public void TheStoredHashSurvivesTheRoundTrip()
        {
            if (NativeLookAhead.Engine == null)
            {
                _output.WriteLine("the engine is not built; skipping. Run: cargo build");
                return;
            }

            List<IndexConversation> built = Built();
            string path = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName() + ".jsonl");
            try
            {
                ShippedIndexWriter.Write(path, built);
                using LookAheadLibrary engine = LookAheadLibrary.Open(path);

                foreach (IndexConversation conversation in built)
                {
                    _output.WriteLine($"{conversation.Id}: {conversation.Hash()}");
                    Assert.Equal(conversation.Hash(), engine.HashOf(conversation.Id));
                }
            }
            finally
            {
                File.Delete(path);
            }
        }

        /// <summary>An index with no header reports no format, and is still readable.</summary>
        /// <remarks>
        /// What the FULL index looks like. It carries no hashes either, so a mod shipping
        /// one simply cannot validate itself - which is what the mod did before there was
        /// any such thing, and is not a failure.
        /// </remarks>
        [Fact]
        public void AnIndexWithNoHeaderReportsNoFormatAndNoHash()
        {
            if (NativeLookAhead.Engine == null)
            {
                _output.WriteLine("the engine is not built; skipping. Run: cargo build");
                return;
            }

            string path = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName() + ".jsonl");
            try
            {
                File.WriteAllText(path, "{\"id\":900,\"entries\":[]}\n");
                using LookAheadLibrary engine = LookAheadLibrary.Open(path);

                Assert.Equal(0, engine.IndexFormat);
                Assert.Equal(string.Empty, engine.HashOf(900));
                Assert.Equal(1, engine.ConversationCount);
            }
            finally
            {
                File.Delete(path);
            }
        }

        /// <summary>Two conversations, one linking into the other.</summary>
        private static List<IndexConversation> Built()
        {
            var first = new IndexConversation(900);

            var root = new IndexEntry { Id = 0, Group = true };
            root.Links.Add(new KeyValuePair<int, int>(900, 1));
            root.Links.Add(new KeyValuePair<int, int>(900, 2));
            first.Entries.Add(root);

            var guarded = new IndexEntry
            {
                Id = 1,
                Guard = "Variable[\"jam.asked\"] == false",
                Script = "Variable[\"jam.asked\"] = true",
            };
            guarded.Links.Add(new KeyValuePair<int, int>(901, 0));
            first.Entries.Add(guarded);

            var check = new IndexEntry { Id = 2 };
            check.Fields.Add(new KeyValuePair<string, string>("DifficultyPass", "8"));
            // Dropped by the writer, because the engine never reads it.
            check.Fields.Add(new KeyValuePair<string, string>("Dialogue Text", "A great sadness."));
            first.Entries.Add(check);

            var second = new IndexConversation(901);
            second.Entries.Add(new IndexEntry { Id = 0 });

            return new List<IndexConversation> { first, second };
        }
    }
}
