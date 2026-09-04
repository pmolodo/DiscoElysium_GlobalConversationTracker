// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using GlobalConversationTracker.Engine;
using Xunit;
using Xunit.Abstractions;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// Reducing a conversation to something two different readers can compare.
    /// </summary>
    /// <remarks>
    /// <para>The index the mod ships is a cache of the dialogue database, and this is what
    /// says whether it still describes the database a player's game loaded. Two properties
    /// matter and they pull against each other: it must ignore everything that is only a
    /// difference in how the two sources happen to walk their data, and it must miss
    /// nothing that a crawl could observe.</para>
    ///
    /// <para>Both are tested here, and the second one entry at a time - a hash that stopped
    /// noticing one field would be a cache that silently kept answering about a game the
    /// player is not in.</para>
    /// </remarks>
    public class ConversationHasherTests
    {
        private readonly ITestOutputHelper _output;

        public ConversationHasherTests(ITestOutputHelper output)
        {
            _output = output;
        }

        /// <summary>The same conversation, hashed twice, is the same hash.</summary>
        /// <remarks>
        /// Trivial to state and the whole point: <c>string.GetHashCode()</c> is the obvious
        /// thing to reach for here and is randomised per process in .NET Core, so it would
        /// pass this within one run and fail it between the extractor and the plugin -
        /// which are different processes, and where the symptom is a cache miss on every
        /// single launch.
        /// </remarks>
        [Fact]
        public void TheSameConversationHashesTheSameWay()
        {
            Assert.Equal(Example().Finish(), Example().Finish());
        }

        /// <summary>
        /// The ORDER entries, fields and links arrive in does not change the hash.
        /// </summary>
        /// <remarks>
        /// The extractor reads a 170 MB YAML file and the plugin walks live PixelCrushers
        /// objects; there is no reason for the two to enumerate in the same order, and an
        /// order that differs is not a difference in the graph. Without this every launch
        /// would rebuild the index over nothing at all.
        /// </remarks>
        [Fact]
        public void TheOrderThingsArriveInDoesNotMatter()
        {
            var forwards = new ConversationHasher(631);
            forwards.Add(0, false, "a", "b", Links((631, 1), (636, 2)), Fields(
                ("DifficultyPass", "8"), ("kim_watch", "true")));
            forwards.Add(1, true, "c", "d", Links(), Fields());

            var backwards = new ConversationHasher(631);
            backwards.Add(1, true, "c", "d", Links(), Fields());
            backwards.Add(0, false, "a", "b", Links((636, 2), (631, 1)), Fields(
                ("kim_watch", "true"), ("DifficultyPass", "8")));

            Assert.Equal(forwards.Finish(), backwards.Finish());
        }

        /// <summary>
        /// The two ways the statement separator is written hash the same.
        /// </summary>
        /// <remarks>
        /// <para>Found in the game, not reasoned about. The extractor reads the database's
        /// YAML, where a script's statement separator is the two literal characters
        /// backslash and n; the plugin reads the live PixelCrushers objects, where the
        /// Dialogue System has already made it a real newline. Conversation 451's entries
        /// 16 and 80 differ in exactly that and in nothing else, and it was enough to make
        /// the plugin rebuild a 15 MB index on launch.</para>
        ///
        /// <para>The action parser turns the escape into a newline before reading anything,
        /// so the two are the same script to everything downstream - and a hash that
        /// separates what the engine cannot tell apart reports a difference that does not
        /// exist.</para>
        /// </remarks>
        [Fact]
        public void TheTwoSpellingsOfTheStatementSeparatorHashTheSame()
        {
            const string Escaped =
                "GainItem(\"shoes_faln\");\\nSetVariableValue(\"jam.bought\", true)";
            string real = Escaped.Replace("\\n", "\n");
            Assert.NotEqual(Escaped, real);

            var fromTheAsset = new ConversationHasher(451);
            fromTheAsset.Add(16, false, string.Empty, Escaped, Links(), Fields());

            var fromTheGame = new ConversationHasher(451);
            fromTheGame.Add(16, false, string.Empty, real, Links(), Fields());

            Assert.Equal(fromTheAsset.Finish(), fromTheGame.Finish());
        }

        /// <summary>A guard's separator is normalised too, for the same reason.</summary>
        [Fact]
        public void AGuardsSeparatorIsNormalisedAsWell()
        {
            var escaped = new ConversationHasher(1);
            escaped.Add(0, false, "a\\nb", string.Empty, Links(), Fields());

            var real = new ConversationHasher(1);
            real.Add(0, false, "a\nb", string.Empty, Links(), Fields());

            Assert.Equal(escaped.Finish(), real.Finish());
        }

        /// <summary>
        /// Normalising the separator does not make two different scripts look alike.
        /// </summary>
        /// <remarks>
        /// The risk of any normalisation, and worth pinning: it must collapse the two
        /// spellings of one thing and nothing else.
        /// </remarks>
        [Fact]
        public void NormalisingTheSeparatorDoesNotMergeDifferentScripts()
        {
            var one = new ConversationHasher(1);
            one.Add(0, false, string.Empty, "A()\nB()", Links(), Fields());

            var other = new ConversationHasher(1);
            other.Add(0, false, string.Empty, "A()\nC()", Links(), Fields());

            Assert.NotEqual(one.Finish(), other.Finish());
        }

        /// <summary>A field the engine never reads does not change the hash.</summary>
        /// <remarks>
        /// A patch that rewrites dialogue text changes nothing a crawl can observe, and a
        /// cache check that rebuilt the whole index over it would be reporting a difference
        /// that does not exist. So a caller may hand over every field it has.
        /// </remarks>
        [Fact]
        public void AFieldTheEngineNeverReadsDoesNotChangeTheHash()
        {
            var bare = new ConversationHasher(631);
            bare.Add(0, false, "a", "b", Links(), Fields(("DifficultyPass", "8")));

            var wordy = new ConversationHasher(631);
            wordy.Add(0, false, "a", "b", Links(), Fields(
                ("DifficultyPass", "8"),
                ("Dialogue Text", "You feel a great sadness."),
                ("Articy Id", "0x01000000000004E2")));

            Assert.Equal(bare.Finish(), wordy.Finish());
        }

        /// <summary>Every field the engine DOES read changes the hash when it changes.</summary>
        /// <remarks>
        /// One case per field rather than a spot check, because what a missed field costs
        /// is silent: the cache would keep answering about a conversation whose checks,
        /// costs or flags had moved, and the marker would be wrong with nothing to say so.
        /// </remarks>
        [Fact]
        public void EveryFieldTheEngineReadsChangesTheHash()
        {
            string baseline = new Func<string>(() =>
            {
                var hasher = new ConversationHasher(631);
                hasher.Add(0, false, "a", "b", Links(), Fields());
                return hasher.Finish();
            })();

            foreach (string field in IndexFields.Read)
            {
                var changed = new ConversationHasher(631);
                changed.Add(0, false, "a", "b", Links(), Fields((field, "1")));

                _output.WriteLine($"{field}: {changed.Finish()}");
                Assert.True(
                    baseline != changed.Finish(),
                    $"adding {field} did not change the hash");
            }
        }

        /// <summary>Anything a crawl can observe changes the hash.</summary>
        [Theory]
        [InlineData("a different guard")]
        [InlineData("a different script")]
        [InlineData("a different entry id")]
        [InlineData("a different group flag")]
        [InlineData("a link somewhere else")]
        [InlineData("one more link")]
        [InlineData("one more entry")]
        [InlineData("a different conversation id")]
        public void AnythingACrawlCanObserveChangesTheHash(string what)
        {
            var changed = new ConversationHasher(what == "a different conversation id" ? 632 : 631);
            changed.Add(
                what == "a different entry id" ? 9 : 0,
                what == "a different group flag",
                what == "a different guard" ? "changed" : "guard",
                what == "a different script" ? "changed" : "script",
                what == "a link somewhere else" ? Links((999, 1))
                    : what == "one more link" ? Links((631, 1), (631, 2))
                    : Links((631, 1)),
                Fields(("DifficultyPass", "8")));
            if (what == "one more entry")
            {
                changed.Add(1, false, "", "", Links(), Fields());
            }

            Assert.NotEqual(Example().Finish(), changed.Finish());
        }

        /// <summary>
        /// Two entries whose parts run together differently are still told apart.
        /// </summary>
        /// <remarks>
        /// The collision the length prefixes exist to prevent, and it is one by
        /// construction rather than by luck: without them a guard of "ab" with an empty
        /// script and a guard of "a" with a script of "b" reduce to the same characters,
        /// and two different conversations look like the same cached one.
        /// </remarks>
        [Fact]
        public void ValuesThatRunTogetherAreStillToldApart()
        {
            var joined = new ConversationHasher(631);
            joined.Add(0, false, "ab", string.Empty, Links(), Fields());

            var split = new ConversationHasher(631);
            split.Add(0, false, "a", "b", Links(), Fields());

            Assert.NotEqual(joined.Finish(), split.Finish());
        }

        /// <summary>
        /// The same entry added twice is refused rather than quietly keeping one.
        /// </summary>
        [Fact]
        public void AnEntryAddedTwiceIsRefused()
        {
            var hasher = new ConversationHasher(631);
            hasher.Add(0, false, "a", "b", Links(), Fields());

            Assert.Throws<ArgumentException>(
                () => hasher.Add(0, false, "c", "d", Links(), Fields()));
        }

        /// <summary>The canonical form is readable, for when two sides disagree.</summary>
        /// <remarks>
        /// A hash that differs says only that something differs. When the extractor and the
        /// plugin disagree the useful question is WHERE, and the answer has to be a string
        /// somebody can diff.
        /// </remarks>
        [Fact]
        public void TheCanonicalFormShowsWhatWentIntoIt()
        {
            string canonical = Example().Canonical();
            _output.WriteLine(canonical);

            Assert.Contains("guard", canonical);
            Assert.Contains("DifficultyPass", canonical);
        }

        private static ConversationHasher Example()
        {
            var hasher = new ConversationHasher(631);
            hasher.Add(0, false, "guard", "script", Links((631, 1)), Fields(
                ("DifficultyPass", "8")));
            return hasher;
        }

        private static IEnumerable<KeyValuePair<int, int>> Links(
            params (int Conversation, int Entry)[] links)
        {
            foreach ((int conversation, int entry) in links)
            {
                yield return new KeyValuePair<int, int>(conversation, entry);
            }
        }

        private static IEnumerable<KeyValuePair<string, string>> Fields(
            params (string Name, string Value)[] fields)
        {
            foreach ((string name, string value) in fields)
            {
                yield return new KeyValuePair<string, string>(name, value);
            }
        }
    }
}
