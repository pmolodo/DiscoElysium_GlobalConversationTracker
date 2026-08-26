// SPDX-License-Identifier: MIT
using System;
using GlobalConversationTracker;
using Xunit;

namespace GlobalConversationTracker.Tests
{
    public class DialogueScoreTests
    {
        [Theory]
        [InlineData(SimStatus.Untouched, 0d)]
        [InlineData(SimStatus.WasOffered, 0.5d)]
        [InlineData(SimStatus.WasDisplayed, 1d)]
        public void Of_ScoresEachStatus(SimStatus status, double expected)
        {
            Assert.Equal(expected, DialogueScore.Of(status));
        }

        [Fact]
        public void Of_UndefinedStatus_Throws()
        {
            Assert.Throws<ArgumentOutOfRangeException>(() => DialogueScore.Of((SimStatus)99));
        }

        [Theory]
        [InlineData(0, 0, 0d)]
        [InlineData(1, 0, 0.5d)]
        [InlineData(0, 1, 1d)]
        [InlineData(3, 2, 3.5d)]
        [InlineData(112_940, 0, 56_470d)]
        public void Total_WeighsOfferedAtAHalf(int offered, int displayed, double expected)
        {
            Assert.Equal(expected, DialogueScore.Total(offered, displayed));
        }

        [Theory]
        [InlineData(0d, false)]
        [InlineData(1d, false)]
        [InlineData(0.5d, true)]
        [InlineData(12_345.5d, true)]
        public void HasHalf_SpotsTheOnlyFractionThereIs(double score, bool expected)
        {
            Assert.Equal(expected, DialogueScore.HasHalf(score));
        }

        [Theory]
        [InlineData(0d, "0")]
        [InlineData(7d, "7")]
        [InlineData(7.5d, "7.5")]
        [InlineData(1_234d, "1,234")]
        [InlineData(12_345.5d, "12,345.5")]
        public void Format_ShowsTheDecimalPlaceOnlyForAHalf(double score, string expected)
        {
            Assert.Equal(expected, DialogueScore.Format(score));
        }

        [Fact]
        public void Format_IsInvariantOfTheAmbientCulture()
        {
            // The plugin runs inside a game that has been through a locale the mod
            // does not choose; the display must not turn into "1.234,5".
            System.Globalization.CultureInfo previous =
                System.Globalization.CultureInfo.CurrentCulture;
            try
            {
                System.Globalization.CultureInfo.CurrentCulture =
                    new System.Globalization.CultureInfo("de-DE");
                Assert.Equal("1,234.5", DialogueScore.Format(1234.5d));
            }
            finally
            {
                System.Globalization.CultureInfo.CurrentCulture = previous;
            }
        }
    }
}
