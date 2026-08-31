// SPDX-License-Identifier: MIT
using System;
using System.Drawing;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The comparison metric, which is what every screenshot-driven wait rests on.
    /// </summary>
    /// <remarks>
    /// Pure functions over images built in memory, so these run anywhere - no game, no
    /// display, no focus. That is the whole reason the arithmetic was pulled out of the
    /// capture code.
    /// </remarks>
    public class GameScreenTests
    {
        private static Bitmap Solid(Color colour, int width = 200, int height = 100)
        {
            var bitmap = new Bitmap(width, height);
            using (Graphics graphics = Graphics.FromImage(bitmap))
            {
                graphics.Clear(colour);
            }

            return bitmap;
        }

        [Fact]
        public void IdenticalImagesDoNotDiffer()
        {
            using Bitmap first = Solid(Color.Black);
            using Bitmap second = Solid(Color.Black);

            Assert.Equal(0, GameScreen.Difference(
                GameScreen.FingerprintOf(first), GameScreen.FingerprintOf(second)), 4);
        }

        /// <summary>
        /// Not exactly 1: the downscale is bilinear, so the outermost samples blend with
        /// the bitmap edge. Asserting equality would be asserting the interpolation away.
        /// </summary>
        [Fact]
        public void OppositeImagesDifferAlmostCompletely()
        {
            using Bitmap black = Solid(Color.Black);
            using Bitmap white = Solid(Color.White);

            double difference = GameScreen.Difference(
                GameScreen.FingerprintOf(black), GameScreen.FingerprintOf(white));

            Assert.True(difference > 0.99, $"expected near 1, got {difference}");
        }

        [Fact]
        public void MidGreyLandsInTheMiddle()
        {
            using Bitmap black = Solid(Color.Black);
            using Bitmap grey = Solid(Color.FromArgb(128, 128, 128));

            double difference = GameScreen.Difference(
                GameScreen.FingerprintOf(black), GameScreen.FingerprintOf(grey));

            Assert.InRange(difference, 0.4, 0.6);
        }

        /// <summary>
        /// The property a threshold depends on: a small change must move the number a
        /// little. A metric that saturated on any change would make every threshold
        /// either 0 or 1.
        /// </summary>
        [Fact]
        public void ASmallRegionChangingMovesTheMetricOnlyALittle()
        {
            using Bitmap plain = Solid(Color.Black);
            using Bitmap speckled = Solid(Color.Black);
            using (Graphics graphics = Graphics.FromImage(speckled))
            {
                graphics.FillRectangle(Brushes.White, 0, 0, 20, 10);
            }

            double difference = GameScreen.Difference(
                GameScreen.FingerprintOf(plain), GameScreen.FingerprintOf(speckled));

            Assert.InRange(difference, 0.001, 0.05);
        }

        [Fact]
        public void FingerprintsAreSquare()
        {
            using Bitmap bitmap = Solid(Color.Black);

            Assert.Equal(64 * 64, GameScreen.FingerprintOf(bitmap).Length);
            Assert.Equal(32 * 32, GameScreen.FingerprintOf(bitmap, 32).Length);
            Assert.Equal(64, GameScreen.DefaultFingerprintSize);
        }

        [Fact]
        public void ComparingDifferentSizesIsRefused()
        {
            using Bitmap bitmap = Solid(Color.Black);

            Assert.Throws<ArgumentException>(() => GameScreen.Difference(
                GameScreen.FingerprintOf(bitmap, 16), GameScreen.FingerprintOf(bitmap, 32)));
        }

        [Theory]
        [InlineData(null, "first")]
        public void ComparingNullIsRefused(double[]? fingerprint, string _)
        {
            using Bitmap bitmap = Solid(Color.Black);
            double[] real = GameScreen.FingerprintOf(bitmap);

            Assert.Throws<ArgumentNullException>(() => GameScreen.Difference(fingerprint!, real));
            Assert.Throws<ArgumentNullException>(() => GameScreen.Difference(real, fingerprint!));
        }

        /// <summary>
        /// Why a stored reference must be size-checked before it is compared.
        /// </summary>
        /// <remarks>
        /// Both images reduce to the same grid whatever their size, so a reference
        /// captured from a 960x480 console compares against a 1280x720 game window
        /// without complaining. It reports a number, the number looks plausible, and it
        /// means nothing. This is not a flaw in the fingerprint - the whole point of it
        /// is to be insensitive to detail - which is why the check belongs at the call
        /// site, in GameSession.WaitUntilMatches.
        /// </remarks>
        [Fact]
        public void FingerprintsHappilyCompareAcrossMismatchedSizes()
        {
            using Bitmap consoleSized = Solid(Color.Black, 960, 480);
            using Bitmap gameSized = Solid(Color.Black, 1280, 720);

            double[] first = GameScreen.FingerprintOf(consoleSized);
            double[] second = GameScreen.FingerprintOf(gameSized);

            Assert.Equal(first.Length, second.Length);

            // A perfect match between two images that share nothing but their fill.
            Assert.Equal(0, GameScreen.Difference(first, second), 4);
        }

        /// <summary>A saved capture keeps its native size; only the fingerprint shrinks.</summary>
        [Fact]
        public void SavedImagesKeepTheirFullResolution()
        {
            string path = Path.Combine(
                Path.GetTempPath(), "gct-fullres-" + Guid.NewGuid().ToString("N") + ".png");

            try
            {
                using (Bitmap frame = Solid(Color.Black, 1280, 720))
                using (Graphics graphics = Graphics.FromImage(frame))
                {
                    graphics.FillRectangle(Brushes.White, 10, 10, 40, 20);
                    frame.Save(path, System.Drawing.Imaging.ImageFormat.Png);
                }

                Size saved = GameScreen.SizeOfFile(path);

                Assert.Equal(1280, saved.Width);
                Assert.Equal(720, saved.Height);

                // And the fingerprint of that same file is still the small grid.
                Assert.Equal(
                    GameScreen.DefaultFingerprintSize * GameScreen.DefaultFingerprintSize,
                    GameScreen.FingerprintFile(path).Length);
            }
            finally
            {
                if (File.Exists(path))
                {
                    File.Delete(path);
                }
            }
        }

        // ---- detail, which is what tells a blank window from a settled screen -------

        /// <summary>
        /// The bug this exists to prevent: an unpainted window is one flat colour, a flat
        /// colour never changes, and a wait for "stops changing" therefore succeeds
        /// immediately - reporting that loading finished before a frame was drawn.
        /// </summary>
        [Fact]
        public void AFlatImageHasNoDetail()
        {
            using Bitmap black = Solid(Color.Black);
            using Bitmap grey = Solid(Color.FromArgb(70, 70, 70));

            double blackDetail = GameScreen.Detail(GameScreen.FingerprintOf(black));
            double greyDetail = GameScreen.Detail(GameScreen.FingerprintOf(grey));

            // Not exactly zero for a non-black fill: the bilinear downscale blends the
            // outermost samples with the bitmap edge, which is a real effect worth about
            // 0.0007. What matters is that it sits far below the floor that decides
            // "this window has not painted yet".
            Assert.True(
                blackDetail < GameSession.BlankDetailFloor,
                $"black should read as blank, got {blackDetail}");
            Assert.True(
                greyDetail < GameSession.BlankDetailFloor,
                $"flat grey should read as blank, got {greyDetail}");
            Assert.True(greyDetail < 0.01, $"and comfortably so, got {greyDetail}");
        }

        [Fact]
        public void AnImageWithContentHasDetail()
        {
            using var bitmap = new Bitmap(200, 100);
            using (Graphics graphics = Graphics.FromImage(bitmap))
            {
                graphics.Clear(Color.Black);
                graphics.FillRectangle(Brushes.White, 0, 0, 100, 100);
            }

            double detail = GameScreen.Detail(GameScreen.FingerprintOf(bitmap));

            Assert.True(
                detail > GameSession.BlankDetailFloor,
                $"a half-white image should clear the blank floor, got {detail}");
        }

        [Fact]
        public void DetailOfNullIsRefused()
        {
            Assert.Throws<ArgumentNullException>(() => GameScreen.Detail(null!));
        }
    }
}
