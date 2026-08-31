// SPDX-License-Identifier: MIT
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

namespace GlobalConversationTracker.Automation
{
    /// <summary>What changed between two captures, and where.</summary>
    public sealed class PixelDifference
    {
        /// <summary>Creates a result.</summary>
        /// <param name="changedPixels">How many pixels differ beyond the tolerance.</param>
        /// <param name="totalPixels">How many were compared.</param>
        /// <param name="bounds">The box containing every change, or Empty for none.</param>
        public PixelDifference(int changedPixels, int totalPixels, Rectangle bounds)
        {
            ChangedPixels = changedPixels;
            TotalPixels = totalPixels;
            Bounds = bounds;
        }

        /// <summary>How many pixels differ beyond the tolerance.</summary>
        public int ChangedPixels { get; }

        /// <summary>How many pixels were compared.</summary>
        public int TotalPixels { get; }

        /// <summary>
        /// The smallest box containing every changed pixel, in the compared region's
        /// coordinates. <see cref="Rectangle.Empty"/> when nothing changed.
        /// </summary>
        /// <remarks>
        /// The shape is as diagnostic as the count. An asterisk appended to a line of
        /// dialogue is a small box at the end of a text row; a scene change is the whole
        /// frame. A count alone cannot tell those apart.
        /// </remarks>
        public Rectangle Bounds { get; }

        /// <summary>Whether anything changed at all.</summary>
        public bool Any => ChangedPixels > 0;

        /// <inheritdoc/>
        public override string ToString()
        {
            return Any
                ? $"{ChangedPixels} of {TotalPixels} pixels changed, within "
                    + $"{Bounds.Width}x{Bounds.Height} at {Bounds.X},{Bounds.Y}"
                : $"no change across {TotalPixels} pixels";
        }
    }

    /// <summary>
    /// Comparing captures pixel by pixel, for changes too small to average.
    /// </summary>
    /// <remarks>
    /// <para>A separate tool from <see cref="GameScreen.Difference"/>, because the two
    /// questions want opposite statistics.</para>
    ///
    /// <para>"Has the screen stopped changing" wants a MEAN over a downscaled copy: it
    /// should ignore a cursor and a little noise, and a mean does.</para>
    ///
    /// <para>"Did an asterisk appear at the end of that dialogue option" wants a COUNT at
    /// full resolution. An asterisk is on the order of 100 pixels out of 921,600, so its
    /// mean absolute difference is about 0.0001 - under the threshold that decides a
    /// screen is not moving at all, and indistinguishable from noise. Raising the
    /// fingerprint resolution does not help, because the mean divides by the pixel count
    /// either way. Counting does not divide by anything: a hundred changed pixels is a
    /// hundred, whatever the screen size.</para>
    /// </remarks>
    public static class PixelDiff
    {
        /// <summary>
        /// How far a channel may move before a pixel counts as changed.
        /// </summary>
        /// <remarks>
        /// Not zero. Video output is not bit-exact frame to frame - dithering, subpixel
        /// text rendering and compression all wobble the bottom bits - and a tolerance of
        /// zero reports thousands of "changes" between two captures of a still screen.
        /// </remarks>
        public const int DefaultTolerance = 24;

        /// <summary>Compares two images pixel by pixel.</summary>
        /// <param name="first">The earlier image.</param>
        /// <param name="second">The later image, of the same size.</param>
        /// <param name="region">
        /// The area to compare, or null for all of it. Narrowing to the region a change
        /// is expected in is what keeps an unrelated animation elsewhere from drowning it.
        /// </param>
        /// <param name="tolerance">Per-channel tolerance; see <see cref="DefaultTolerance"/>.</param>
        /// <exception cref="ArgumentNullException">Either image is null.</exception>
        /// <exception cref="ArgumentException">The images are different sizes.</exception>
        public static PixelDifference Compare(
            Bitmap first, Bitmap second, Rectangle? region = null, int tolerance = DefaultTolerance)
        {
            if (first == null)
            {
                throw new ArgumentNullException(nameof(first));
            }

            if (second == null)
            {
                throw new ArgumentNullException(nameof(second));
            }

            if (first.Width != second.Width || first.Height != second.Height)
            {
                throw new ArgumentException(
                    $"Images are different sizes ({first.Width}x{first.Height} vs "
                    + $"{second.Width}x{second.Height}).",
                    nameof(second));
            }

            Rectangle area = region ?? new Rectangle(0, 0, first.Width, first.Height);
            area = Rectangle.Intersect(area, new Rectangle(0, 0, first.Width, first.Height));
            if (area.Width <= 0 || area.Height <= 0)
            {
                return new PixelDifference(0, 0, Rectangle.Empty);
            }

            BitmapData firstData = first.LockBits(
                area, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            BitmapData secondData = second.LockBits(
                area, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);

            try
            {
                return CompareLocked(firstData, secondData, area, tolerance);
            }
            finally
            {
                first.UnlockBits(firstData);
                second.UnlockBits(secondData);
            }
        }

        private static unsafe PixelDifference CompareLocked(
            BitmapData first, BitmapData second, Rectangle area, int tolerance)
        {
            var firstScan = (byte*)first.Scan0;
            var secondScan = (byte*)second.Scan0;

            int changed = 0;
            int minX = int.MaxValue;
            int minY = int.MaxValue;
            int maxX = int.MinValue;
            int maxY = int.MinValue;

            for (int y = 0; y < area.Height; y++)
            {
                byte* firstRow = firstScan + (y * first.Stride);
                byte* secondRow = secondScan + (y * second.Stride);

                for (int x = 0; x < area.Width; x++)
                {
                    int offset = x * 4;

                    // Blue, green, red. Alpha is ignored: a screen capture is opaque, and
                    // comparing it would only add noise.
                    int deltaB = Math.Abs(firstRow[offset] - secondRow[offset]);
                    int deltaG = Math.Abs(firstRow[offset + 1] - secondRow[offset + 1]);
                    int deltaR = Math.Abs(firstRow[offset + 2] - secondRow[offset + 2]);

                    if (deltaB > tolerance || deltaG > tolerance || deltaR > tolerance)
                    {
                        changed++;
                        if (x < minX) { minX = x; }
                        if (x > maxX) { maxX = x; }
                        if (y < minY) { minY = y; }
                        if (y > maxY) { maxY = y; }
                    }
                }
            }

            Rectangle bounds = changed == 0
                ? Rectangle.Empty
                : new Rectangle(
                    area.X + minX, area.Y + minY, maxX - minX + 1, maxY - minY + 1);

            return new PixelDifference(changed, area.Width * area.Height, bounds);
        }

        /// <summary>Compares two images already on disk.</summary>
        /// <param name="firstPath">The earlier image.</param>
        /// <param name="secondPath">The later image.</param>
        /// <param name="region">The area to compare, or null for all of it.</param>
        /// <param name="tolerance">Per-channel tolerance.</param>
        public static PixelDifference CompareFiles(
            string firstPath,
            string secondPath,
            Rectangle? region = null,
            int tolerance = DefaultTolerance)
        {
            using (var first = new Bitmap(firstPath))
            using (var second = new Bitmap(secondPath))
            {
                return Compare(first, second, region, tolerance);
            }
        }

        /// <summary>
        /// Writes an image highlighting what changed, for looking at a result that
        /// surprises you.
        /// </summary>
        /// <param name="first">The earlier image.</param>
        /// <param name="second">The later image.</param>
        /// <param name="path">Where to write the PNG.</param>
        /// <param name="region">The area compared, or null for all of it.</param>
        /// <param name="tolerance">Per-channel tolerance.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static PixelDifference SaveComparison(
            Bitmap first,
            Bitmap second,
            string path,
            Rectangle? region = null,
            int tolerance = DefaultTolerance)
        {
            PixelDifference difference = Compare(first, second, region, tolerance);

            using (var annotated = new Bitmap(second))
            {
                using (Graphics graphics = Graphics.FromImage(annotated))
                {
                    if (region.HasValue)
                    {
                        graphics.DrawRectangle(Pens.DeepSkyBlue, region.Value);
                    }

                    if (difference.Any)
                    {
                        // Inflated by a pixel so a one-pixel change is still visible as a
                        // box rather than hidden under the line drawn around it.
                        Rectangle box = difference.Bounds;
                        box.Inflate(2, 2);
                        graphics.DrawRectangle(Pens.Magenta, box);
                    }
                }

                annotated.Save(path, ImageFormat.Png);
            }

            return difference;
        }
    }
}
