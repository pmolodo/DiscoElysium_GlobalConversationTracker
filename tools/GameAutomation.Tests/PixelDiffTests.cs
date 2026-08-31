// SPDX-License-Identifier: MIT
using System;
using System.Drawing;
using Xunit;
using Xunit.Abstractions;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Detecting a change the size of one asterisk on a dialogue option.
    /// </summary>
    /// <remarks>
    /// The fixtures are the real shape of the problem: a 1280x720 frame with a line of
    /// text, and the same frame with a small glyph appended. That is a change of about a
    /// hundred pixels in nine hundred thousand.
    /// </remarks>
    public class PixelDiffTests
    {
        private const int ScreenWidth = 1280;
        private const int ScreenHeight = 720;

        private readonly ITestOutputHelper _output;

        public PixelDiffTests(ITestOutputHelper output)
        {
            _output = output;
        }

        /// <summary>A dark frame with a row of "text" blocks, like a dialogue line.</summary>
        private static Bitmap DialogueFrame(bool withAsterisk)
        {
            var bitmap = new Bitmap(ScreenWidth, ScreenHeight);
            using (Graphics graphics = Graphics.FromImage(bitmap))
            {
                graphics.Clear(Color.FromArgb(20, 18, 24));

                // A line of text: blocks along one row, as a rendered option would be.
                for (int i = 0; i < 40; i++)
                {
                    graphics.FillRectangle(
                        Brushes.Gainsboro, 80 + (i * 14), 400, 9, 14);
                }

                if (withAsterisk)
                {
                    // The marker: a glyph at the end of the line, about 10x10.
                    graphics.FillRectangle(Brushes.Orange, 80 + (40 * 14) + 4, 400, 10, 10);
                }
            }

            return bitmap;
        }

        /// <summary>
        /// The reason PixelDiff exists. A mean absolute difference cannot see an
        /// asterisk at any fingerprint size, because it divides by the pixel count.
        /// </summary>
        [Theory]
        [InlineData(64)]
        [InlineData(256)]
        [InlineData(512)]
        public void TheMeanCannotSeeAnAsteriskAtAnyResolution(int fingerprintSize)
        {
            using Bitmap without = DialogueFrame(withAsterisk: false);
            using Bitmap with = DialogueFrame(withAsterisk: true);

            double mean = GameScreen.Difference(
                GameScreen.FingerprintOf(without, fingerprintSize),
                GameScreen.FingerprintOf(with, fingerprintSize));

            _output.WriteLine($"{fingerprintSize}x{fingerprintSize}: mean difference {mean:N6}");

            // Well under GameSession.StillThreshold, which is what decides a screen is
            // not moving at all. Raising the resolution does not rescue it.
            Assert.True(
                mean < GameSession.StillThreshold,
                $"expected the mean to miss it, but got {mean}");
        }

        /// <summary>And the reason a count does work.</summary>
        [Fact]
        public void CountingPixelsSeesTheAsterisk()
        {
            using Bitmap without = DialogueFrame(withAsterisk: false);
            using Bitmap with = DialogueFrame(withAsterisk: true);

            PixelDifference difference = PixelDiff.Compare(without, with);

            _output.WriteLine(difference.ToString());

            Assert.True(difference.Any, "a 10x10 glyph must register");
            Assert.InRange(difference.ChangedPixels, 50, 200);
        }

        /// <summary>
        /// The bounding box is what separates "an asterisk appeared" from "the scene
        /// changed". A count alone cannot tell those apart.
        /// </summary>
        [Fact]
        public void TheBoundingBoxLocatesTheAsterisk()
        {
            using Bitmap without = DialogueFrame(withAsterisk: false);
            using Bitmap with = DialogueFrame(withAsterisk: true);

            PixelDifference difference = PixelDiff.Compare(without, with);

            Assert.InRange(difference.Bounds.Width, 8, 14);
            Assert.InRange(difference.Bounds.Height, 8, 14);
            Assert.InRange(difference.Bounds.Y, 395, 405);

            // At the END of the line of text, which is where the marker belongs.
            Assert.True(
                difference.Bounds.X > 600,
                $"expected the change at the end of the line, found it at x={difference.Bounds.X}");
        }

        [Fact]
        public void IdenticalFramesShowNoChange()
        {
            using Bitmap first = DialogueFrame(withAsterisk: false);
            using Bitmap second = DialogueFrame(withAsterisk: false);

            PixelDifference difference = PixelDiff.Compare(first, second);

            Assert.False(difference.Any);
            Assert.Equal(Rectangle.Empty, difference.Bounds);
            Assert.Equal(ScreenWidth * ScreenHeight, difference.TotalPixels);
        }

        /// <summary>
        /// Narrowing to the region the marker belongs in is what keeps an animation
        /// elsewhere on screen from burying it.
        /// </summary>
        [Fact]
        public void ARegionIgnoresChangesOutsideIt()
        {
            using Bitmap without = DialogueFrame(withAsterisk: false);
            using Bitmap with = DialogueFrame(withAsterisk: true);
            using (Graphics graphics = Graphics.FromImage(with))
            {
                // Something big and irrelevant moving in the corner.
                graphics.FillRectangle(Brushes.Red, 0, 0, 300, 200);
            }

            PixelDifference everything = PixelDiff.Compare(without, with);
            PixelDifference justTheLine = PixelDiff.Compare(
                without, with, new Rectangle(0, 380, ScreenWidth, 60));

            _output.WriteLine($"whole frame: {everything}");
            _output.WriteLine($"text row:    {justTheLine}");

            Assert.True(everything.ChangedPixels > 50_000, "the red block should dominate");
            Assert.InRange(justTheLine.ChangedPixels, 50, 200);
        }

        /// <summary>
        /// The tolerance has to be above zero. Video output is not bit-exact frame to
        /// frame, and a tolerance of zero reports a still screen as changing everywhere.
        /// </summary>
        [Fact]
        public void ATolerantComparisonIgnoresTinyWobble()
        {
            using Bitmap first = DialogueFrame(withAsterisk: false);
            using var second = new Bitmap(first);

            // Nudge every pixel by less than the tolerance, as dithering would.
            using (Graphics graphics = Graphics.FromImage(second))
            {
                graphics.FillRectangle(
                    new SolidBrush(Color.FromArgb(8, 255, 255, 255)), 0, 0, ScreenWidth, ScreenHeight);
            }

            PixelDifference tolerant = PixelDiff.Compare(first, second);
            PixelDifference exact = PixelDiff.Compare(first, second, region: null, tolerance: 0);

            _output.WriteLine($"tolerance {PixelDiff.DefaultTolerance}: {tolerant}");
            _output.WriteLine($"tolerance 0: {exact}");

            Assert.False(tolerant.Any, "a sub-tolerance nudge must not register");
            Assert.True(exact.ChangedPixels > 0, "and a zero tolerance must be why that matters");
        }

        [Fact]
        public void MismatchedSizesAreRefused()
        {
            using var small = new Bitmap(100, 100);
            using var large = new Bitmap(200, 200);

            Assert.Throws<ArgumentException>(() => PixelDiff.Compare(small, large));
        }

        [Fact]
        public void NullImagesAreRefused()
        {
            using var bitmap = new Bitmap(10, 10);

            Assert.Throws<ArgumentNullException>(() => PixelDiff.Compare(null!, bitmap));
            Assert.Throws<ArgumentNullException>(() => PixelDiff.Compare(bitmap, null!));
        }

        [Fact]
        public void ARegionOutsideTheImageComparesNothing()
        {
            using Bitmap first = DialogueFrame(withAsterisk: false);
            using Bitmap second = DialogueFrame(withAsterisk: true);

            PixelDifference difference = PixelDiff.Compare(
                first, second, new Rectangle(5000, 5000, 100, 100));

            Assert.False(difference.Any);
            Assert.Equal(0, difference.TotalPixels);
        }
    }
}
