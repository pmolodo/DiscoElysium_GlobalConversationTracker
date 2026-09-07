// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using System.Runtime.InteropServices;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Asks, before anything is launched, whether the machine's displays could hold the
    /// window a run is about to ask for.
    /// </summary>
    /// <remarks>
    /// <para>A run that gets the wrong window size already stops - see the window check in
    /// the look-ahead run and in the launch verb - but it can only do that AFTER the game
    /// has started, staged the player's profile aside and drawn a frame. The portrait case
    /// is knowable before any of that: a 1280x720 window cannot fit on a display that is
    /// 1080 wide, whatever the PlayerPrefs say, and Unity clamps it to 1080x720. That
    /// happened on 2026-09-06 (de-qslk), and the display's orientation changed TWICE in
    /// that one session, so this is session state rather than a property of the machine and
    /// is worth re-reading on every run.</para>
    ///
    /// <para>REFUSES ONLY WHAT IS CLEARLY IMPOSSIBLE, which is most of the care here. A
    /// screen larger than the request is fine, and so is one exactly the requested size -
    /// the window's own borders may not fit, but Unity's answer to that is its business and
    /// not something to refuse a run over. Three things are unknowable rather than
    /// impossible, and each returns <see cref="Fit.Unknown"/>:</para>
    ///
    /// <list type="bullet">
    /// <item><description>A FULLSCREEN request is a display MODE, not a window. A display
    /// can be set to a mode smaller than its own bounds, so its size says nothing about
    /// whether the request can be honoured.</description></item>
    /// <item><description>A process that is NOT DPI AWARE is told scaled bounds - a
    /// 2240x1080 display at 200% reads as 1120x540 - and would refuse a request the display
    /// can honour perfectly. See <see cref="DisplayScaling"/>, which is what makes the
    /// ordinary case aware.</description></item>
    /// <item><description>NO SCREEN ENUMERATED at all. There is no evidence either way, and
    /// a run refused for want of evidence is worse than one that fails honestly five
    /// minutes later.</description></item>
    /// </list>
    /// </remarks>
    public static class DisplayBounds
    {
        /// <summary>What the displays have to say about a requested window.</summary>
        public enum Fit
        {
            /// <summary>Nothing here can call the request impossible.</summary>
            Unknown,

            /// <summary>Some screen is at least as large as the request.</summary>
            Fits,

            /// <summary>Every screen is too small in one dimension or both.</summary>
            TooSmall,
        }

        /// <summary>A verdict and the sentence that explains it.</summary>
        /// <param name="Verdict">Whether the request is impossible, possible, or unknowable.</param>
        /// <param name="What">One sentence for a log or a refusal.</param>
        public readonly record struct FitReport(Fit Verdict, string What);

        /// <summary>MONITORINFOF_PRIMARY.</summary>
        private const int MonitorPrimary = 1;

        /// <summary>
        /// Whether the displays attached right now could hold the requested window.
        /// </summary>
        /// <param name="requested">The display settings the run is about to stage.</param>
        /// <exception cref="ArgumentNullException"><paramref name="requested"/> is null.</exception>
        public static FitReport CanHold(DisplaySettings requested)
        {
            if (requested == null)
            {
                throw new ArgumentNullException(nameof(requested));
            }

            return Verdict(requested, Screens(), DisplayScaling.MeasuresRealPixels());
        }

        /// <summary>
        /// The verdict on its own, given what was measured. Separated so it can be tested
        /// against displays this machine does not have.
        /// </summary>
        /// <param name="requested">The display settings the run is about to stage.</param>
        /// <param name="screens">Every attached screen's size, in real pixels.</param>
        /// <param name="measuringRealPixels">
        /// Whether <paramref name="screens"/> is in real pixels rather than scaled units.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static FitReport Verdict(
            DisplaySettings requested, IReadOnlyList<Size> screens, bool measuringRealPixels)
        {
            if (requested == null)
            {
                throw new ArgumentNullException(nameof(requested));
            }

            if (screens == null)
            {
                throw new ArgumentNullException(nameof(screens));
            }

            string wanted = $"{requested.Width}x{requested.Height}";

            if (!requested.IsWindowed)
            {
                return new FitReport(
                    Fit.Unknown,
                    $"{requested} asks for a display mode rather than a window, and a "
                        + "display can be set to a mode smaller than itself, so its size "
                        + "says nothing about whether that can be honoured");
            }

            if (!measuringRealPixels)
            {
                return new FitReport(
                    Fit.Unknown,
                    "this process is not measuring the screen in real pixels, so the "
                        + "bounds it reads are a fraction of the truth and cannot say "
                        + $"whether a {wanted} window fits");
            }

            if (screens.Count == 0)
            {
                return new FitReport(
                    Fit.Unknown,
                    "no display could be enumerated, so there is no evidence either way "
                        + $"about a {wanted} window");
            }

            bool holds = screens.Any(
                screen => screen.Width >= requested.Width && screen.Height >= requested.Height);

            string attached = Describe(screens);
            return holds
                ? new FitReport(
                    Fit.Fits,
                    $"a {wanted} window fits on {attached}")
                : new FitReport(
                    Fit.TooSmall,
                    $"no attached display can hold a {wanted} window: {attached}");
        }

        /// <summary>Every attached screen's size, in the units this process measures in.</summary>
        /// <remarks>
        /// <c>EnumDisplayMonitors</c> rather than Windows Forms' <c>Screen.AllScreens</c>,
        /// which would mean a UI framework reference for two rectangles; the rest of this
        /// assembly reads the desktop through the same interop.
        /// </remarks>
        public static IReadOnlyList<Size> Screens()
        {
            var found = new List<Size>();
            var primary = new List<Size>();

            bool Collect(IntPtr monitor, IntPtr context, ref Rect bounds, IntPtr data)
            {
                var info = new MonitorInfo { Size = Marshal.SizeOf<MonitorInfo>() };
                if (!GetMonitorInfo(monitor, ref info))
                {
                    // Enumeration continues: one unreadable monitor is not a reason to
                    // report none, and reporting fewer only makes this more cautious.
                    return true;
                }

                var size = new Size(
                    info.Monitor.Right - info.Monitor.Left,
                    info.Monitor.Bottom - info.Monitor.Top);

                // The primary first, because it is the one the game opens on and so the
                // one a report should name first.
                if ((info.Flags & MonitorPrimary) != 0)
                {
                    primary.Add(size);
                }
                else
                {
                    found.Add(size);
                }

                return true;
            }

            if (!EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, Collect, IntPtr.Zero))
            {
                return Array.Empty<Size>();
            }

            primary.AddRange(found);
            return primary;
        }

        /// <summary>Those screens as words, for a sentence.</summary>
        private static string Describe(IReadOnlyList<Size> screens) =>
            string.Join(", ", screens.Select(screen => $"{screen.Width}x{screen.Height}"))
                + (screens.Count == 1 ? " is the only display" : " are the displays attached");

        [StructLayout(LayoutKind.Sequential)]
        private struct Rect
        {
            public int Left;
            public int Top;
            public int Right;
            public int Bottom;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct MonitorInfo
        {
            public int Size;
            public Rect Monitor;
            public Rect Work;
            public int Flags;
        }

        private delegate bool MonitorCallback(
            IntPtr monitor, IntPtr context, ref Rect bounds, IntPtr data);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool EnumDisplayMonitors(
            IntPtr deviceContext, IntPtr clip, MonitorCallback callback, IntPtr data);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetMonitorInfo(IntPtr monitor, ref MonitorInfo info);
    }
}
