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

        private static LookAheadAnswer Answer(BranchAnswers? branches) =>
            new LookAheadAnswer(
                new NodeRef(451, 12), 0, true, 0, 0, 0, "none", branches);

        private static BranchAnswers Both(
            int passDestination, int passBest, int failDestination, int failBest,
            bool passComplete = true, bool failComplete = true) =>
            new BranchAnswers(
                new BranchAnswer(passDestination, passBest, passComplete),
                new BranchAnswer(failDestination, failBest, failComplete));

        /// <summary>
        /// An option with one outcome gets no line.
        /// </summary>
        /// <remarks>
        /// The absence is load bearing: the engine fills the branches only for a white or
        /// red check, and that is exactly how the mod decides which options earn a line.
        /// </remarks>
        [Fact]
        public void AnOptionThatDoesNotRollGetsNoLine()
        {
            Assert.Null(BranchLine.For(Answer(null), Palette()));
        }

        /// <summary>A rolled check names both outcomes, in that order.</summary>
        [Fact]
        public void ARolledCheckNamesBothOutcomes()
        {
            string line = Assert.IsType<string>(
                BranchLine.For(Answer(Both(0, 0, 0, 0)), Palette()));

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
                BranchLine.For(Answer(Both(novelty, novelty, 0, 0)), Palette()));

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
                BranchLine.For(Answer(Both(0, 2, 0, 0)), Palette()));

            Assert.Contains($"<color={Seen}>{BranchLine.PassWord}</color>", line);
            Assert.Contains($"<color={Any}>{BranchLine.FoundMarker}</color>", line);
        }

        /// <summary>An outcome that reaches nothing past itself earns nothing.</summary>
        [Fact]
        public void AnOutcomeThatReachesNoFurtherEarnsNoAsterisk()
        {
            string line = Assert.IsType<string>(
                BranchLine.For(Answer(Both(2, 2, 2, 2)), Palette()));

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
                BranchLine.For(Answer(Both(0, 0, 0, 0, failComplete: false)), Palette()));

            Assert.Contains($"<color={Gave}>{BranchLine.UncertainMarker}</color>", line);
        }

        /// <summary>...unless the player has asked not to be told.</summary>
        [Fact]
        public void AnUncertainOutcomeIsSilentWhenTheSwitchIsOff()
        {
            string line = Assert.IsType<string>(
                BranchLine.For(
                    Answer(Both(0, 0, 0, 0, failComplete: false)),
                    Palette(markUncertain: false)));

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
                BranchLine.For(Answer(Both(0, 2, 0, 0, passComplete: false)), Palette()));

            Assert.Contains($"<color={Any}>{BranchLine.FoundMarker}</color>", line);
            Assert.DoesNotContain(BranchLine.UncertainMarker, line);
        }

        /// <summary>The two words are separated, so they read as two columns.</summary>
        [Fact]
        public void TheTwoWordsAreSetApart()
        {
            string line = Assert.IsType<string>(
                BranchLine.For(Answer(Both(0, 0, 0, 0)), Palette()));

            int between = line.IndexOf(BranchLine.FailWord)
                - (line.IndexOf(BranchLine.PassWord) + BranchLine.PassWord.Length);
            Assert.True(between > 1, $"the words are {between} characters apart");
        }
    }
}
