// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Engine;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// The Pass / Fail line below a white or red check.
    /// </summary>
    /// <remarks>
    /// What the player actually reads, so the assertions are about the markup rather than
    /// about the numbers behind it. The colours are given distinct fake values, so a test
    /// that passed because two of them happened to be equal cannot.
    /// </remarks>
    public class BranchLineTests
    {
        private const string Any = "#AAAAAA";
        private const string This = "#BBBBBB";
        private const string Seen = "#CCCCCC";
        private const string Gave = "#DDDDDD";

        /// <summary>
        /// The option's uncertain colour, which the line must NOT use.
        /// </summary>
        /// <remarks>
        /// Distinct from <see cref="Gave"/> so that a line reaching for the wrong one of
        /// the two is a failure rather than a coincidence. They are different colours in
        /// the shipped mod for a real reason - the line is drawn on the check's background
        /// and an option on black, see de-8hh2.4 - so a test that let them be equal would
        /// not be testing the thing that broke.
        /// </remarks>
        private const string GaveOnAnOption = "#EEEEEE";

        private static MarkerPalette Palette(bool markUncertain = true) =>
            new MarkerPalette(Any, This, Seen, GaveOnAnOption, Gave, markUncertain);

        /// <summary>The line for a pair of outcomes.</summary>
        private static string Line(Outcomes both, MarkerPalette palette) =>
            BranchLine.For(both.Pass, both.Fail, palette);

        /// <summary>One outcome, as the engine now answers for it.</summary>
        /// <remarks>
        /// AN ORDINARY ANSWER WITH AN OUTCOME NAMED, which is the whole of de-8hh2.6: a
        /// check is two options wearing one line of text, and each half is answered like
        /// any other start rather than as a field inside one answer for the option.
        /// </remarks>
        private static LookAheadAnswer Outcome(
            string branch, int destination, int best, bool complete = true) =>
            new LookAheadAnswer(
                new NodeRef(451, 12), best, complete, 0, 0, 0, "none", branch, destination);

        /// <summary>Both outcomes of one check.</summary>
        private static Outcomes Both(
            int passDestination, int passBest, int failDestination, int failBest,
            bool passComplete = true, bool failComplete = true) =>
            new Outcomes(
                Outcome(LookAheadAnswer.Pass, passDestination, passBest, passComplete),
                Outcome(LookAheadAnswer.Fail, failDestination, failBest, failComplete));

        /// <summary>
        /// An option with one outcome gets no line.
        /// </summary>
        /// <remarks>
        /// THE ABSENCE IS LOAD BEARING, and it has moved one level out. It used to be a
        /// null pair inside the option's answer; a check now comes back as two answers and
        /// an ordinary option as one that names no outcome, so "is this a roll" is a
        /// question about the RESPONSE. `BranchLine.For` is handed the two halves and
        /// composes a line unconditionally, which is why this asks `OutcomesOf` instead.
        /// </remarks>
        [Fact]
        public void AnOptionThatDoesNotRollGetsNoLine()
        {
            var plain = new NodeRef(451, 12);
            LookAheadResponse response = LookAheadResponse.Parse(
                "{\"answers\":[{\"start\":{\"conversation\":451,\"entry\":12},"
                + "\"best\":0,\"complete\":true,\"elapsed_ms\":0}]}");

            Assert.Null(response.OutcomesOf(plain));
            Assert.NotNull(response.Find(plain, null));
        }

        /// <summary>And a rolled check's two answers are found as a pair.</summary>
        [Fact]
        public void ARolledCheckIsFoundAsItsTwoOutcomes()
        {
            var check = new NodeRef(451, 12);
            LookAheadResponse response = LookAheadResponse.Parse(
                "{\"answers\":["
                + "{\"start\":{\"conversation\":451,\"entry\":12},\"branch\":\"pass\","
                + "\"destination\":0,\"best\":2,\"complete\":true,\"elapsed_ms\":0},"
                + "{\"start\":{\"conversation\":451,\"entry\":12},\"branch\":\"fail\","
                + "\"destination\":1,\"best\":1,\"complete\":true,\"elapsed_ms\":0}]}");

            Outcomes both = Assert.NotNull(response.OutcomesOf(check));
            Assert.Equal(2, both.Pass.Best);
            Assert.Equal(1, both.Fail.Destination);

            // Asking for the option itself finds nothing, which is the point: an entry no
            // longer names one answer, so a lookup that forgot the outcome would otherwise
            // get whichever half happened to be first.
            Assert.Null(response.Find(check, null));
        }

        /// <summary>A rolled check names both outcomes, in that order.</summary>
        [Fact]
        public void ARolledCheckNamesBothOutcomes()
        {
            string line = Assert.IsType<string>(
                Line(Both(0, 0, 0, 0), Palette()));

            Assert.StartsWith("\n", line);
            Assert.Contains(BranchLine.PassWord, line);
            Assert.Contains(BranchLine.FailWord, line);
            Assert.True(
                line.IndexOf(BranchLine.PassWord) < line.IndexOf(BranchLine.FailWord),
                "Fail was drawn before Pass");
        }

        /// <summary>
        /// Each word is coloured by where its own outcome leads.
        /// </summary>
        /// <remarks>
        /// The half of the design that says the two outcomes are told APART: a check whose
        /// passing branch leads somewhere unread and whose failing branch leads somewhere
        /// read has to look different from its opposite, and this is where that shows.
        /// </remarks>
        [Theory]
        [InlineData(0, Seen)]
        [InlineData(1, This)]
        [InlineData(2, Any)]
        public void EachWordTakesTheColourOfItsOwnDestination(int novelty, string expected)
        {
            string line = Assert.IsType<string>(
                Line(Both(novelty, novelty, 0, 0), Palette()));

            Assert.Contains($"<color={expected}>{BranchLine.PassWord}</color>", line);
            Assert.Contains($"<color={Seen}>{BranchLine.FailWord}</color>", line);
        }

        /// <summary>
        /// An outcome earns an asterisk when something beyond it outranks it.
        /// </summary>
        /// <remarks>
        /// The same rule the option's own marker follows, applied to one branch: the word
        /// says what the outcome IS, and the asterisk says there is more past it.
        /// </remarks>
        [Fact]
        public void AnOutcomeThatReachesFurtherEarnsAnAsterisk()
        {
            // Passing leads to something already read, but something unread lies beyond it.
            string line = Assert.IsType<string>(
                Line(Both(0, 2, 0, 0), Palette()));

            Assert.Contains($"<color={Seen}>{BranchLine.PassWord}</color>", line);
            Assert.Contains($"<color={Any}>{BranchLine.FoundMarker}</color>", line);
        }

        /// <summary>An outcome that reaches nothing past itself earns nothing.</summary>
        [Fact]
        public void AnOutcomeThatReachesNoFurtherEarnsNoAsterisk()
        {
            string line = Assert.IsType<string>(
                Line(Both(2, 2, 2, 2), Palette()));

            Assert.DoesNotContain(BranchLine.FoundMarker, line);
        }

        /// <summary>
        /// An outcome whose search gave up says so, rather than saying nothing.
        /// </summary>
        /// <remarks>
        /// de-pvq's rule, per branch. Not finding something is provisional where the search
        /// was cut short, and drawing nothing would claim the stronger thing - that there is
        /// nothing down there - on the strength of a search that did not establish it.
        /// </remarks>
        [Fact]
        public void AnOutcomeWhoseSearchGaveUpSaysSo()
        {
            string line = Assert.IsType<string>(
                Line(Both(0, 0, 0, 0, failComplete: false), Palette()));

            Assert.Contains($"<color={Gave}>{BranchLine.UncertainMarker}</color>", line);
        }

        /// <summary>...unless the player has asked not to be told.</summary>
        [Fact]
        public void AnUncertainOutcomeIsSilentWhenTheSwitchIsOff()
        {
            string line = Assert.IsType<string>(
                Line(Both(0, 0, 0, 0, failComplete: false), Palette(markUncertain: false)));

            Assert.DoesNotContain(BranchLine.UncertainMarker, line);
            Assert.DoesNotContain(Gave, line);
        }

        /// <summary>
        /// A found asterisk beats an uncertain one on the same outcome.
        /// </summary>
        /// <remarks>
        /// A witness is a witness: a search that ran out of budget AFTER finding something
        /// has still found it, so the outcome draws the ordinary marker rather than the
        /// grey one. Only a search that found nothing is uncertain about anything.
        /// </remarks>
        [Fact]
        public void AnIncompleteSearchThatFoundSomethingStillSaysSo()
        {
            string line = Assert.IsType<string>(
                Line(Both(0, 2, 0, 0, passComplete: false), Palette()));

            Assert.Contains($"<color={Any}>{BranchLine.FoundMarker}</color>", line);
            Assert.DoesNotContain(BranchLine.UncertainMarker, line);
        }

        /// <summary>
        /// Every shape one half of the line can take, named.
        /// </summary>
        /// <remarks>
        /// <para>EIGHT, AND NO MORE THAN EIGHT. A half is a word in the colour of where
        /// its outcome lands, and an asterisk in the colour of the best beyond it, drawn
        /// only when that best outranks the destination. Three destinations times what can
        /// outrank each gives eight - and the ninth, an orange word with an asterisk, is
        /// impossible by construction: nothing outranks the top rung, so there is never
        /// anything for an asterisk to report. <see cref="AnOrangeHalfNeverCarriesAnything"/>
        /// holds that.</para>
        ///
        /// <para>Named because the in-game suites arrange these same eight through
        /// fixtures, at a minute a launch, and a shape that has never been composed here
        /// has no business being asked of the game.</para>
        /// </remarks>
        public enum Shape
        {
            /// <summary>Lands on text no save has read. Nothing can outrank it.</summary>
            OrangeAlone,

            /// <summary>Lands on text this save has not read; nothing beyond beats it.</summary>
            RedAlone,

            /// <summary>...and beyond it is text no save has read.</summary>
            RedThenOrange,

            /// <summary>...and the search gave up before it could say.</summary>
            RedThenGaveUp,

            /// <summary>Lands on text this save has read; nothing beyond beats it.</summary>
            DarkRedAlone,

            /// <summary>...and beyond it is text this save has not read.</summary>
            DarkRedThenRed,

            /// <summary>...and beyond it is text no save has read.</summary>
            DarkRedThenOrange,

            /// <summary>...and the search gave up.</summary>
            DarkRedThenGaveUp,
        }

        /// <summary>The answer that produces one shape.</summary>
        private static LookAheadAnswer AnswerFor(Shape shape, string branch) => shape switch
        {
            Shape.OrangeAlone => Outcome(branch, 2, 2),
            Shape.RedAlone => Outcome(branch, 1, 1),
            Shape.RedThenOrange => Outcome(branch, 1, 2),
            Shape.RedThenGaveUp => Outcome(branch, 1, 1, false),
            Shape.DarkRedAlone => Outcome(branch, 0, 0),
            Shape.DarkRedThenRed => Outcome(branch, 0, 1),
            Shape.DarkRedThenOrange => Outcome(branch, 0, 2),
            _ => Outcome(branch, 0, 0, false),
        };

        /// <summary>The markup one shape must be drawn as, for a given word.</summary>
        private static string MarkupFor(Shape shape, string word) => shape switch
        {
            Shape.OrangeAlone => Coloured(Any, word),
            Shape.RedAlone => Coloured(This, word),
            Shape.RedThenOrange => Coloured(This, word) + Coloured(Any, BranchLine.FoundMarker),
            Shape.RedThenGaveUp =>
                Coloured(This, word) + Coloured(Gave, BranchLine.UncertainMarker),
            Shape.DarkRedAlone => Coloured(Seen, word),
            Shape.DarkRedThenRed =>
                Coloured(Seen, word) + Coloured(This, BranchLine.FoundMarker),
            Shape.DarkRedThenOrange =>
                Coloured(Seen, word) + Coloured(Any, BranchLine.FoundMarker),
            _ => Coloured(Seen, word) + Coloured(Gave, BranchLine.UncertainMarker),
        };

        private static string Coloured(string colour, string text) =>
            $"<color={colour}>{text}</color>";

        /// <summary>
        /// Every shape, on the Pass half and on the Fail half, drawn exactly.
        /// </summary>
        /// <remarks>
        /// BOTH HALVES FOR EACH, because the two are separate code paths in everything but
        /// name and the bug worth catching is one that draws the Pass answer twice. Each
        /// case pairs a shape with a DIFFERENT one on the other half for the same reason -
        /// a line whose halves are identical cannot show that they are read separately -
        /// and the two same-shape cases at the end cover the other half of the user's ask.
        /// </remarks>
        [Theory]
        [InlineData(Shape.OrangeAlone, Shape.RedAlone)]
        [InlineData(Shape.RedAlone, Shape.OrangeAlone)]
        [InlineData(Shape.RedThenOrange, Shape.DarkRedAlone)]
        [InlineData(Shape.DarkRedAlone, Shape.RedThenOrange)]
        [InlineData(Shape.RedThenGaveUp, Shape.DarkRedThenRed)]
        [InlineData(Shape.DarkRedThenRed, Shape.RedThenGaveUp)]
        [InlineData(Shape.DarkRedThenOrange, Shape.DarkRedThenGaveUp)]
        [InlineData(Shape.DarkRedThenGaveUp, Shape.DarkRedThenOrange)]
        [InlineData(Shape.OrangeAlone, Shape.OrangeAlone)]
        [InlineData(Shape.DarkRedThenOrange, Shape.DarkRedThenOrange)]
        public void EveryShapeIsDrawnOnEitherHalf(Shape pass, Shape fail)
        {
            string line = Assert.IsType<string>(
                BranchLine.For(
                    AnswerFor(pass, LookAheadAnswer.Pass),
                    AnswerFor(fail, LookAheadAnswer.Fail),
                    Palette()));

            Assert.Contains(MarkupFor(pass, BranchLine.PassWord), line);
            Assert.Contains(MarkupFor(fail, BranchLine.FailWord), line);
        }

        /// <summary>
        /// A half on the top rung carries nothing, whatever the search did.
        /// </summary>
        /// <remarks>
        /// The ninth shape, which must not exist. An asterisk answers "is there something
        /// beyond this that outranks it", and text no save has read cannot be outranked -
        /// so the answer is no, settled, and a search that ran out of budget has not made
        /// it doubtful. It used to draw '*?' here, which claimed a doubt about a question
        /// that has none.
        /// </remarks>
        [Theory]
        [InlineData(true)]
        [InlineData(false)]
        public void AnOrangeHalfNeverCarriesAnything(bool complete)
        {
            string line = Assert.IsType<string>(
                BranchLine.For(
                    Outcome(LookAheadAnswer.Pass, 2, 2, complete),
                    Outcome(LookAheadAnswer.Fail, 2, 2, complete),
                    Palette()));

            Assert.DoesNotContain(BranchLine.FoundMarker, line);
            Assert.DoesNotContain(BranchLine.UncertainMarker, line);
        }

        /// <summary>The two words are separated, so they read as two columns.</summary>
        [Fact]
        public void TheTwoWordsAreSetApart()
        {
            string line = Assert.IsType<string>(
                Line(Both(0, 0, 0, 0), Palette()));

            int between = line.IndexOf(BranchLine.FailWord)
                - (line.IndexOf(BranchLine.PassWord) + BranchLine.PassWord.Length);
            Assert.True(between > 1, $"the words are {between} characters apart");
        }
    }
}
