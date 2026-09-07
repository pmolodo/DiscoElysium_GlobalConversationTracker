// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using GlobalConversationTracker.Automation;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The pre-flight that refuses a run whose display cannot give it the window.
    /// </summary>
    /// <remarks>
    /// The verdict is tested against displays this machine does not have, which is the
    /// whole reason it takes the screens as an argument. What is checked on the real
    /// machine is only that enumeration answers at all; what it answers depends on whoever
    /// is running the tests.
    /// </remarks>
    public class DisplayBoundsTests
    {
        /// <summary>The window every in-game run asks for.</summary>
        private static readonly DisplaySettings Windowed720p =
            new DisplaySettings(1280, 720, DisplaySettings.WindowedMode);

        private static IReadOnlyList<Size> Displays(params (int Width, int Height)[] sizes) =>
            sizes.Select(size => new Size(size.Width, size.Height)).ToArray();

        [Fact]
        public void ALargerDisplayHoldsIt()
        {
            DisplayBounds.FitReport fit = DisplayBounds.Verdict(
                Windowed720p, Displays((2240, 1080)), measuringRealPixels: true);

            Assert.Equal(DisplayBounds.Fit.Fits, fit.Verdict);
        }

        /// <summary>
        /// A screen exactly the requested size is allowed, deliberately.
        /// </summary>
        /// <remarks>
        /// The window's borders and caption do not fit in it, and what Unity does about
        /// that is its own business - it may position the window partly off-screen and
        /// still give the client area asked for. Not clearly impossible, so not refused.
        /// </remarks>
        [Fact]
        public void ADisplayExactlyTheRequestedSizeIsNotRefused()
        {
            DisplayBounds.FitReport fit = DisplayBounds.Verdict(
                Windowed720p, Displays((1280, 720)), measuringRealPixels: true);

            Assert.Equal(DisplayBounds.Fit.Fits, fit.Verdict);
        }

        /// <summary>
        /// The portrait case of 2026-09-06: tall enough, and 200 pixels too narrow.
        /// </summary>
        [Fact]
        public void APortraitDisplayCannotHoldIt()
        {
            DisplayBounds.FitReport fit = DisplayBounds.Verdict(
                Windowed720p, Displays((1080, 2240)), measuringRealPixels: true);

            Assert.Equal(DisplayBounds.Fit.TooSmall, fit.Verdict);
            Assert.Contains("1280x720", fit.What, StringComparison.Ordinal);
            Assert.Contains("1080x2240", fit.What, StringComparison.Ordinal);
        }

        /// <summary>
        /// One screen out of several is enough, since the game opens on one of them.
        /// </summary>
        [Fact]
        public void OneBigEnoughDisplayAmongSeveralIsEnough()
        {
            DisplayBounds.FitReport fit = DisplayBounds.Verdict(
                Windowed720p,
                Displays((1080, 2240), (800, 600), (1920, 1080)),
                measuringRealPixels: true);

            Assert.Equal(DisplayBounds.Fit.Fits, fit.Verdict);
        }

        /// <summary>
        /// Every screen too small in some dimension is the refusal, even when between them
        /// they cover both.
        /// </summary>
        [Fact]
        public void SeveralDisplaysThatEachMissAreStillTooSmall()
        {
            DisplayBounds.FitReport fit = DisplayBounds.Verdict(
                Windowed720p,
                Displays((1080, 2240), (1920, 600)),
                measuringRealPixels: true);

            Assert.Equal(DisplayBounds.Fit.TooSmall, fit.Verdict);
        }

        /// <summary>
        /// A scaled process reads a 2240x1080 display as 1120x540, which would refuse a
        /// request that display can honour perfectly.
        /// </summary>
        [Fact]
        public void AProcessThatIsNotMeasuringRealPixelsRefusesNothing()
        {
            DisplayBounds.FitReport fit = DisplayBounds.Verdict(
                Windowed720p, Displays((1120, 540)), measuringRealPixels: false);

            Assert.Equal(DisplayBounds.Fit.Unknown, fit.Verdict);
            Assert.Contains("real pixels", fit.What, StringComparison.Ordinal);
        }

        /// <summary>
        /// A fullscreen request is a display mode, and a display can be set to a mode
        /// smaller than itself.
        /// </summary>
        [Fact]
        public void AFullscreenRequestIsNotJudgedByTheDisplaySize()
        {
            var fullscreen = new DisplaySettings(1280, 720, DisplaySettings.WindowedMode + 1);

            DisplayBounds.FitReport fit = DisplayBounds.Verdict(
                fullscreen, Displays((1080, 2240)), measuringRealPixels: true);

            Assert.Equal(DisplayBounds.Fit.Unknown, fit.Verdict);
        }

        /// <summary>Nothing enumerated is no evidence, not a refusal.</summary>
        [Fact]
        public void NoDisplaysAtAllIsNotEvidence()
        {
            DisplayBounds.FitReport fit = DisplayBounds.Verdict(
                Windowed720p, Array.Empty<Size>(), measuringRealPixels: true);

            Assert.Equal(DisplayBounds.Fit.Unknown, fit.Verdict);
        }

        /// <summary>Every verdict carries a sentence worth printing.</summary>
        [Theory]
        [InlineData(2240, 1080, true)]
        [InlineData(1080, 2240, true)]
        [InlineData(1120, 540, false)]
        public void EveryVerdictSaysWhy(int width, int height, bool measuringRealPixels)
        {
            DisplayBounds.FitReport fit = DisplayBounds.Verdict(
                Windowed720p, Displays((width, height)), measuringRealPixels);

            Assert.False(string.IsNullOrWhiteSpace(fit.What));
        }

        /// <summary>
        /// The machine running the tests has at least one screen, and it has a size.
        /// </summary>
        /// <remarks>
        /// What that size IS cannot be asserted - it is whoever's machine this is - so this
        /// checks only that the interop answers rather than returning nothing, which is the
        /// failure that would silently turn the pre-flight into a no-op.
        /// </remarks>
        [Fact]
        public void ThisMachineEnumeratesItsScreens()
        {
            IReadOnlyList<Size> screens = DisplayBounds.Screens();

            Assert.NotEmpty(screens);
            Assert.All(screens, screen =>
            {
                Assert.True(screen.Width > 0);
                Assert.True(screen.Height > 0);
            });
        }

        /// <summary>Asking about nothing is a mistake rather than an Unknown.</summary>
        [Fact]
        public void ANullRequestIsRejected()
        {
            Assert.Throws<ArgumentNullException>(
                () => DisplayBounds.Verdict(null!, Array.Empty<Size>(), true));
            Assert.Throws<ArgumentNullException>(
                () => DisplayBounds.Verdict(Windowed720p, null!, true));
            Assert.Throws<ArgumentNullException>(() => DisplayBounds.CanHold(null!));
        }
    }
}
