// SPDX-License-Identifier: MIT
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Capturing the game's window and reducing it to something comparable.
    /// </summary>
    /// <remarks>
    /// <para>Compiled rather than written in PowerShell because Defender matches
    /// <c>Graphics.CopyFromScreen</c> in a script against
    /// <c>HackTool:PowerShell/EmpireGetScreenshot</c> and blocks the whole file. The
    /// signature is real - this is what a screen-scraping tool looks like - so the
    /// answer is to keep the primitive in a compiled assembly a reviewer can read once,
    /// rather than to fight the scanner.</para>
    ///
    /// <para><c>CopyFromScreen</c> and not <c>PrintWindow</c>: PrintWindow asks a window
    /// to paint itself, and a GPU-rendered Unity window answers with black. The price is
    /// that the window must be visible and in front, which the caller is responsible for
    /// arranging.</para>
    ///
    /// <para>A fingerprint is a small greyscale grid rather than the image. Downscaling
    /// first is what makes a difference threshold mean anything: a mouse cursor or one
    /// flickering pixel barely moves it, while a scene change moves it a lot.</para>
    ///
    /// <para>Saved captures are always written at the window's native size. Only the
    /// fingerprint is reduced, and only on a copy - a PNG that has been downscaled is no
    /// use for looking at afterwards, which is the whole reason for keeping it.</para>
    ///
    /// <para>That downscale also hides a mismatch: two images of different sizes, even
    /// different aspect ratios, both reduce to the same grid and compare without
    /// complaint. A reference captured from the wrong window compares perfectly happily
    /// against the right one. Callers comparing against a stored reference should check
    /// its size first - see <see cref="SizeOfFile"/>.</para>
    /// </remarks>
    public static class GameScreen
    {
        /// <summary>The default fingerprint edge, in samples.</summary>
        /// <remarks>
        /// 64 rather than 32. A loading spinner is a small feature of a 1280x720 frame,
        /// and at 32 it averages away to nothing - which makes a spinning loading screen
        /// read as perfectly still, and a "wait until it stops moving" return at once.
        /// </remarks>
        public const int DefaultFingerprintSize = 64;

        /// <summary>Captures a window's client area.</summary>
        /// <param name="window">The window to capture.</param>
        /// <returns>The captured image; the caller disposes it.</returns>
        /// <exception cref="InvalidOperationException">The window has no client area.</exception>
        public static Bitmap Capture(IntPtr window)
        {
            if (!GameWindows.TryGetClientBounds(window, out int x, out int y, out int width, out int height))
            {
                throw new InvalidOperationException(
                    "Windows would not report the client bounds; has the window closed?");
            }

            if (width <= 0 || height <= 0)
            {
                throw new InvalidOperationException(
                    $"The window has no client area ({width}x{height}); is it minimised?");
            }

            var bitmap = new Bitmap(width, height);
            using (Graphics graphics = Graphics.FromImage(bitmap))
            {
                graphics.CopyFromScreen(x, y, 0, 0, new Size(width, height));
            }

            return bitmap;
        }

        /// <summary>Captures a window's client area straight to a PNG.</summary>
        /// <param name="window">The window to capture.</param>
        /// <param name="path">Where to write it.</param>
        public static void SaveCapture(IntPtr window, string path)
        {
            using (Bitmap bitmap = Capture(window))
            {
                bitmap.Save(path, ImageFormat.Png);
            }
        }

        /// <summary>Captures a window and reduces it to a fingerprint.</summary>
        /// <param name="window">The window to capture.</param>
        /// <param name="size">The fingerprint edge, in samples.</param>
        public static double[] Fingerprint(IntPtr window, int size = DefaultFingerprintSize)
        {
            using (Bitmap bitmap = Capture(window))
            {
                return FingerprintOf(bitmap, size);
            }
        }

        /// <summary>The pixel size of an image on disk, without decoding all of it.</summary>
        /// <param name="path">The image to measure.</param>
        public static Size SizeOfFile(string path)
        {
            using (var image = Image.FromFile(path))
            {
                return new Size(image.Width, image.Height);
            }
        }

        /// <summary>Reads an image from disk and reduces it to a fingerprint.</summary>
        /// <param name="path">The image to read.</param>
        /// <param name="size">The fingerprint edge, in samples.</param>
        public static double[] FingerprintFile(string path, int size = DefaultFingerprintSize)
        {
            using (var bitmap = new Bitmap(path))
            {
                return FingerprintOf(bitmap, size);
            }
        }

        /// <summary>
        /// Reads an image from disk and fingerprints one region of it.
        /// </summary>
        /// <remarks>
        /// For a screen that is partly animated. Disco Elysium's main menu is a static
        /// list of options beside a moving painting, and fingerprinting the whole frame
        /// measures mostly the painting - so the threshold has to be loose enough to
        /// tolerate the animation, which is the same looseness that lets another screen
        /// match. Fingerprinting only the still part removes the problem rather than
        /// budgeting for it.
        /// </remarks>
        /// <param name="path">The image to read.</param>
        /// <param name="region">The area to fingerprint.</param>
        /// <param name="size">The fingerprint edge, in samples.</param>
        public static double[] FingerprintFileRegion(
            string path, Rectangle region, int size = DefaultFingerprintSize)
        {
            using (var bitmap = new Bitmap(path))
            {
                return FingerprintRegion(bitmap, region, size);
            }
        }

        /// <summary>Fingerprints one region of an image.</summary>
        /// <param name="bitmap">The image.</param>
        /// <param name="region">The area to fingerprint.</param>
        /// <param name="size">The fingerprint edge, in samples.</param>
        /// <exception cref="ArgumentNullException"><paramref name="bitmap"/> is null.</exception>
        /// <exception cref="ArgumentException">The region is outside the image.</exception>
        public static double[] FingerprintRegion(
            Bitmap bitmap, Rectangle region, int size = DefaultFingerprintSize)
        {
            if (bitmap == null)
            {
                throw new ArgumentNullException(nameof(bitmap));
            }

            Rectangle area = Rectangle.Intersect(
                region, new Rectangle(0, 0, bitmap.Width, bitmap.Height));
            if (area.Width <= 0 || area.Height <= 0)
            {
                throw new ArgumentException(
                    $"Region {region} lies outside the {bitmap.Width}x{bitmap.Height} image.",
                    nameof(region));
            }

            using (var cropped = bitmap.Clone(area, bitmap.PixelFormat))
            {
                return FingerprintOf(cropped, size);
            }
        }

        /// <summary>Captures a window and fingerprints one region of it.</summary>
        /// <param name="window">The window to capture.</param>
        /// <param name="region">The area to fingerprint.</param>
        /// <param name="size">The fingerprint edge, in samples.</param>
        public static double[] FingerprintRegion(
            IntPtr window, Rectangle region, int size = DefaultFingerprintSize)
        {
            using (Bitmap bitmap = Capture(window))
            {
                return FingerprintRegion(bitmap, region, size);
            }
        }

        /// <summary>Reduces an image to a greyscale fingerprint.</summary>
        /// <param name="bitmap">The image.</param>
        /// <param name="size">The fingerprint edge, in samples.</param>
        /// <exception cref="ArgumentNullException"><paramref name="bitmap"/> is null.</exception>
        public static double[] FingerprintOf(Bitmap bitmap, int size = DefaultFingerprintSize)
        {
            if (bitmap == null)
            {
                throw new ArgumentNullException(nameof(bitmap));
            }

            var values = new double[size * size];
            using (var small = new Bitmap(size, size))
            {
                using (Graphics graphics = Graphics.FromImage(small))
                {
                    graphics.InterpolationMode = InterpolationMode.HighQualityBilinear;
                    graphics.DrawImage(bitmap, 0, 0, size, size);
                }

                for (int y = 0; y < size; y++)
                {
                    for (int x = 0; x < size; x++)
                    {
                        Color pixel = small.GetPixel(x, y);
                        values[(y * size) + x] =
                            ((0.299 * pixel.R) + (0.587 * pixel.G) + (0.114 * pixel.B)) / 255.0;
                    }
                }
            }

            return values;
        }

        /// <summary>
        /// How different two fingerprints are, from 0 (identical) to 1 (opposite).
        /// </summary>
        /// <remarks>
        /// A mean rather than a maximum, so one changed region cannot saturate the
        /// number. That is what lets a caller say "less than 0.005 counts as still".
        /// </remarks>
        /// <param name="first">One fingerprint.</param>
        /// <param name="second">The other, of the same size.</param>
        /// <exception cref="ArgumentNullException">Either is null.</exception>
        /// <exception cref="ArgumentException">They are different sizes.</exception>
        public static double Difference(double[] first, double[] second)
        {
            if (first == null)
            {
                throw new ArgumentNullException(nameof(first));
            }

            if (second == null)
            {
                throw new ArgumentNullException(nameof(second));
            }

            if (first.Length != second.Length)
            {
                throw new ArgumentException(
                    $"Fingerprints are different sizes ({first.Length} vs {second.Length}).",
                    nameof(second));
            }

            double total = 0;
            for (int i = 0; i < first.Length; i++)
            {
                total += Math.Abs(first[i] - second[i]);
            }

            return total / first.Length;
        }

        /// <summary>
        /// How much variation a fingerprint holds, as a standard deviation 0..1.
        /// </summary>
        /// <remarks>
        /// A window that exists but has not painted is one flat colour, and a flat colour
        /// is perfectly stable. Without this, a "wait until the screen stops changing"
        /// returns success before the game has drawn a single frame.
        /// </remarks>
        /// <param name="fingerprint">The fingerprint to measure.</param>
        /// <exception cref="ArgumentNullException"><paramref name="fingerprint"/> is null.</exception>
        public static double Detail(double[] fingerprint)
        {
            if (fingerprint == null)
            {
                throw new ArgumentNullException(nameof(fingerprint));
            }

            double mean = 0;
            foreach (double value in fingerprint)
            {
                mean += value;
            }

            mean /= fingerprint.Length;

            double variance = 0;
            foreach (double value in fingerprint)
            {
                variance += (value - mean) * (value - mean);
            }

            return Math.Sqrt(variance / fingerprint.Length);
        }
    }
}
