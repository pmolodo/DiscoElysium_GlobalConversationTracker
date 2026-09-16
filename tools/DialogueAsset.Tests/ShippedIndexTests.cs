// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Engine;
using Xunit;

namespace GlobalConversationTracker.DialogueAsset.Tests
{
    /// <summary>
    /// The index as the mod ships it: what it drops, and what it says about itself.
    /// </summary>
    /// <remarks>
    /// The shipped index is a CACHE of the dialogue database rather than ground truth, so
    /// it has to carry two things the full index does not - the format it is in, and what
    /// each conversation's content reduces to. Those are what let the plugin ask whether
    /// the file still describes the database the player's game actually loaded.
    /// </remarks>
    public class ShippedIndexTests
    {
        /// <summary>Every trimmed conversation carries a hash.</summary>
        [Fact]
        public void EveryTrimmedConversationCarriesAHash()
        {
            foreach (ConversationRecord conversation in ShippedIndex.Trim(Fixture()))
            {
                Assert.False(string.IsNullOrEmpty(conversation.Hash));
                // SHA-256 as lower-case hex, which is what the plugin will compare against.
                Assert.Equal(64, conversation.Hash!.Length);
            }
        }

        /// <summary>Trimming does not change what the hash is over.</summary>
        /// <remarks>
        /// The property that makes the whole scheme work. The plugin computes its half from
        /// the LIVE database, which has never been trimmed and never will be, so if
        /// trimming changed the reduction the two sides could never agree. It does not,
        /// because the reduction only ever looks at what the engine reads.
        /// </remarks>
        [Fact]
        public void TrimmingDoesNotChangeTheHash()
        {
            foreach (ConversationRecord full in Fixture())
            {
                ConversationRecord trimmed = ShippedIndex.Trim(full);
                Assert.Equal(ShippedIndex.HashOf(full), trimmed.Hash);
            }
        }

        /// <summary>A hash survives being written to a line and read back.</summary>
        [Fact]
        public void AHashSurvivesTheIndexLine()
        {
            ConversationRecord trimmed = ShippedIndex.Trim(Fixture()[0]);

            using var written = new StringWriter();
            ConversationIndexFile.Write(written, new[] { trimmed });
            using var reading = new StringReader(written.ToString());

            ConversationRecord back = Assert.Single(ConversationIndexFile.Read(reading));
            Assert.Equal(trimmed.Hash, back.Hash);
        }

        /// <summary>
        /// The full index's lines are what they were before there was such a thing as a
        /// hash.
        /// </summary>
        /// <remarks>
        /// It is regenerated from a 170 MB asset and diffed against the last copy, so a key
        /// appearing on every line of it would be 1,501 lines of noise saying nothing. It
        /// is also a build intermediate that nothing validates against anything, so it has
        /// no use for one.
        /// </remarks>
        [Fact]
        public void TheFullIndexCarriesNoHash()
        {
            ConversationRecord full = Fixture()[0];

            Assert.Null(full.Hash);
            Assert.DoesNotContain("\"hash\"", ConversationIndexFile.ToJson(full));
        }

        /// <summary>The header says what version the file is.</summary>
        [Fact]
        public void TheHeaderNamesTheFormat()
        {
            Assert.Equal(
                "{\"" + ShippedIndex.FormatProperty + "\":" + ShippedIndex.FormatVersion + "}",
                ShippedIndex.Header());
        }

        /// <summary>
        /// The header is recognised as one, and a conversation is not.
        /// </summary>
        /// <remarks>
        /// Getting this wrong is silent: a header deserialised as a conversation is a record
        /// with id 0 and no entries, which reads as a real, empty conversation rather than
        /// as a parse failure.
        /// </remarks>
        [Fact]
        public void TheHeaderIsNeverMistakenForAConversation()
        {
            Assert.True(ConversationIndexFile.IsHeader(ShippedIndex.Header()));
            Assert.False(ConversationIndexFile.IsHeader(
                ConversationIndexFile.ToJson(Fixture()[0])));

            using var reading = new StringReader(
                ShippedIndex.Header() + "\n" + ConversationIndexFile.ToJson(Fixture()[0]));
            ConversationRecord back = Assert.Single(ConversationIndexFile.Read(reading));
            Assert.NotEqual(0, back.Id);
        }

        /// <summary>The trim keeps the fields the engine reads and drops the rest.</summary>
        [Fact]
        public void TheTrimKeepsWhatTheEngineReads()
        {
            ConversationRecord trimmed = ShippedIndex.Trim(Fixture()[0]);

            Assert.Null(trimmed.Title);
            foreach (EntryRecord entry in trimmed.Entries)
            {
                Assert.Null(entry.Title);
                foreach (KeyValuePair<string, string> field in entry.Fields)
                {
                    Assert.Contains(field.Key, IndexFields.Read);
                }
            }
        }

        /// <summary>
        /// The extractor's writer and the plugin's write the same line for the same
        /// conversation.
        /// </summary>
        /// <remarks>
        /// <para>There are two writers of this format and there is a reason for it: the
        /// extractor's exists to produce byte-reproducible output for a 50 MB file that is
        /// diffed against the last copy, while the plugin rebuilds an index from the live
        /// database inside the game and cannot reach that assembly at all. What must not
        /// happen is the two drifting, because the second one exists precisely for the case
        /// where the first one's output was wrong.</para>
        ///
        /// <para>So they are compared, over a whole fixture database and byte for byte. A
        /// difference in escaping, in key order, or in when <c>to_conversation</c> is
        /// written would show up here rather than as an index the engine reads differently
        /// from the one it was tested on.</para>
        /// </remarks>
        [Fact]
        public void TheTwoWritersOfThisFormatAgree()
        {
            foreach (ConversationRecord full in Fixture())
            {
                string extractor = ConversationIndexFile.ToJson(ShippedIndex.Trim(full));
                string plugin = ShippedIndexWriter.ToJson(AsIndexConversation(full));

                Assert.Equal(extractor, plugin);
            }
        }

        /// <summary>
        /// A journal task's conditions travel on its line, identically from both writers, and
        /// nothing else of the conversation's own fields does.
        /// </summary>
        [Fact]
        public void ATasksConditionsAreWrittenTheSameByBothWriters()
        {
            var task = new ConversationRecord { Id = 3 };
            task.Fields["Title"] = "Add even more beauty to the wall";
            task.Fields["display_condition_main"] = "Variable[\"TASK.wall\"]";
            task.Fields["done_condition_main"] = "Variable[\"TASK.wall_done\"]";
            task.Fields["display_subtask_01"] = "Variable[\"TASK.oil\"]";
            task.Fields["done_subtask_01"] = "Variable[\"TASK.oil_done\"]";
            task.IndexedFields = new OrderedDictionary<string, string>();
            foreach (KeyValuePair<string, string> field in task.Fields)
            {
                if (Array.IndexOf(IndexFields.ConversationRead, field.Key) >= 0)
                {
                    task.IndexedFields[field.Key] = field.Value;
                }
            }

            string extractor = ConversationIndexFile.ToJson(ShippedIndex.Trim(task));
            string plugin = ShippedIndexWriter.ToJson(AsIndexConversation(task));

            Assert.Equal(extractor, plugin);
            Assert.Contains("\"fields\":{\"display_condition_main\":", plugin);
            Assert.DoesNotContain("beauty", plugin);
        }

        /// <summary>A conversation that is not a task writes no fields key at all.</summary>
        [Fact]
        public void AConversationThatIsNotATaskHasNoFieldsKey()
        {
            foreach (ConversationRecord full in Fixture().Where(c => c.IndexedFields == null))
            {
                string line = ShippedIndexWriter.ToJson(AsIndexConversation(full));
                string beforeEntries = line.Substring(0, line.IndexOf("\"entries\"", StringComparison.Ordinal));
                Assert.DoesNotContain("\"fields\"", beforeEntries);
            }
        }

        /// <summary>The two headers are the same line too.</summary>
        [Fact]
        public void TheTwoWritersOpenAFileTheSameWay()
        {
            Assert.Equal(ShippedIndex.Header(), ShippedIndexWriter.Header());
            Assert.Equal(ShippedIndex.FormatVersion, ShippedIndexWriter.FormatVersion);
        }

        /// <summary>
        /// The same conversation as the plugin would have built it from the live database.
        /// </summary>
        /// <remarks>
        /// The plugin reads links that always name their destination conversation, so the
        /// index's absent-means-this-conversation rule is resolved here, exactly as
        /// <c>LiveDialogueDatabase</c> gets it resolved for free.
        /// </remarks>
        private static IndexConversation AsIndexConversation(ConversationRecord record)
        {
            var conversation = new IndexConversation(record.Id);
            foreach (KeyValuePair<string, string> field in record.Fields)
            {
                conversation.Fields.Add(field);
            }

            foreach (EntryRecord entry in record.Entries)
            {
                var built = new IndexEntry
                {
                    Id = entry.Id,
                    Group = entry.Group,
                    Guard = entry.Guard,
                    Script = entry.Script,
                };

                for (int index = 0; index < entry.To.Count; index++)
                {
                    int destination =
                        entry.ToConversation != null && index < entry.ToConversation.Count
                            ? entry.ToConversation[index]
                            : record.Id;
                    built.Links.Add(new KeyValuePair<int, int>(destination, entry.To[index]));
                }

                foreach (KeyValuePair<string, string> field in entry.Fields)
                {
                    built.Fields.Add(field);
                }

                conversation.Entries.Add(built);
            }

            return conversation;
        }

        private static List<ConversationRecord> Fixture()
        {
            string asset = Path.Combine(
                AppContext.BaseDirectory, "Fixtures", "mini-database.asset");
            return ConversationIndexExtractor.Extract(asset).ToList();
        }
    }
}
