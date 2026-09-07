// SPDX-License-Identifier: MIT
using System;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Reading the Pass / Fail line off an option the game drew.</summary>
    /// <remarks>
    /// The mod composes the line in <c>BranchLine</c>, which has its own tests. These are
    /// the other half: that what a suite reads back out of the game's text is what was
    /// put in, including the two ways a half can carry no marker and the one way the whole
    /// line can be absent.
    /// </remarks>
    public class ProbeBranchLineTests
    {
        private const string Orange = "#FF8C42";
        private const string Red = "#C4453C";
        private const string DarkRed = "#7C2F2A";
        private const string Grey = "#7A7A7A";

        /// <summary>An option's own line, of the kind a check is drawn on.</summary>
        private const string Option = "Grab the tie.";

        /// <summary>One coloured run, as the game's markup spells it.</summary>
        private static string Draw(string colour, string text) =>
            "<color=" + colour + ">" + text + "</color>";

        /// <summary>An option with a line under it, spaced as the mod spaces it.</summary>
        private static string WithLine(string pass, string fail) =>
            Option + "\n           " + pass + "           " + fail;

        [Fact]
        public void AnOptionWithNoLineHasNone()
        {
            Assert.Null(ProbeLog.BranchLine(Option));
            Assert.Null(ProbeLog.BranchLine(Option + Draw(Orange, "*")));
            Assert.Null(ProbeLog.BranchLine(null));
        }

        [Fact]
        public void BothHalvesComeBackWithTheirColours()
        {
            ProbeBranchLine? line = ProbeLog.BranchLine(
                WithLine(Draw(DarkRed, "Pass"), Draw(Orange, "Fail")));

            Assert.NotNull(line);
            Assert.Equal("Pass", line!.Pass.Word);
            Assert.Equal(DarkRed, line.Pass.ColourHtml);
            Assert.Null(line.Pass.Marker);

            Assert.Equal("Fail", line.Fail.Word);
            Assert.Equal(Orange, line.Fail.ColourHtml);
            Assert.Null(line.Fail.Marker);
        }

        [Fact]
        public void AHalfKeepsItsOwnMarkerAndItsOwnMarkersColour()
        {
            ProbeBranchLine? line = ProbeLog.BranchLine(
                WithLine(
                    Draw(Red, "Pass") + Draw(Orange, "*"),
                    Draw(DarkRed, "Fail") + Draw(Grey, "*?")));

            Assert.Equal(Red, line!.Pass.ColourHtml);
            Assert.Equal("*", line.Pass.Marker);
            Assert.Equal(Orange, line.Pass.MarkerColourHtml);

            Assert.Equal(DarkRed, line.Fail.ColourHtml);
            Assert.Equal("*?", line.Fail.Marker);
            Assert.Equal(Grey, line.Fail.MarkerColourHtml);
        }

        [Fact]
        public void OneHalfMayCarryAMarkerWhileTheOtherDoesNot()
        {
            ProbeBranchLine? line = ProbeLog.BranchLine(
                WithLine(Draw(Orange, "Pass"), Draw(Orange, "Fail") + Draw(Orange, "*")));

            Assert.Null(line!.Pass.Marker);
            Assert.Equal("*", line.Fail.Marker);
        }

        /// <summary>
        /// The whole point of reading the line rather than the option's text: an option
        /// carries its own marker too, and that one is not part of the line.
        /// </summary>
        [Fact]
        public void TheOptionsOwnMarkerIsNotReadAsPartOfTheLine()
        {
            string text = Option + Draw(Orange, "*") + "\n   "
                + Draw(Red, "Pass") + "   " + Draw(DarkRed, "Fail");

            ProbeBranchLine? line = ProbeLog.BranchLine(text);

            Assert.Equal(Red, line!.Pass.ColourHtml);
            Assert.Equal(DarkRed, line.Fail.ColourHtml);
            Assert.Null(line.Pass.Marker);
            Assert.Equal(Option + Draw(Orange, "*"), ProbeLog.WithoutBranchLine(text));
        }

        [Fact]
        public void AnOptionWithoutALineKeepsAllOfItsText()
        {
            Assert.Equal(Option, ProbeLog.WithoutBranchLine(Option));
        }

        /// <summary>
        /// A marker on a half of the line is not a marker on the option.
        /// </summary>
        /// <remarks>
        /// The shape a rolled check is drawn in since its own marker was dropped: no
        /// marker of its own, and a Pass half carrying one. Asking the whole text would
        /// find the Pass half's asterisk and report the option as marked - which passed
        /// every suite in the run that first drew it this way, because the suites were
        /// expecting a marker that was no longer there.
        /// </remarks>
        [Fact]
        public void AMarkerOnAHalfIsNotAMarkerOnTheOption()
        {
            var option = new ProbeOption(
                9,
                50,
                WithLine(Draw(Red, "Pass") + Draw(Orange, "*"), Draw(Orange, "Fail")),
                "White");

            Assert.False(option.HasMarker(Orange));
            Assert.False(option.HasMarker(Red));
            Assert.False(option.HasMarker(Grey, "*?"));
            Assert.NotNull(option.Branches());
        }

        /// <summary>
        /// A line that will not parse is a failure, not an absence. It means the mod drew
        /// something no suite can read, and a suite asserting on it must not pass anyway.
        /// </summary>
        [Theory]
        [InlineData("Pass without its other half")]
        [InlineData("Fail before Pass")]
        [InlineData("something after the second half")]
        public void AMalformedLineIsRefused(string shape)
        {
            string tail = shape switch
            {
                "Pass without its other half" => Draw(Orange, "Pass"),
                "Fail before Pass" => Draw(Orange, "Fail") + " " + Draw(Orange, "Pass"),
                _ => Draw(Orange, "Pass") + " " + Draw(Orange, "Fail")
                    + " " + Draw(Orange, "and more"),
            };

            Assert.Throws<FormatException>(
                () => ProbeLog.BranchLine(Option + "\n" + tail));
        }
    }
}
