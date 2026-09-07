// SPDX-License-Identifier: MIT
using System;
using System.Runtime.InteropServices;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Makes this process measure and capture the screen in real pixels, whatever the
    /// display's scaling is set to.
    /// </summary>
    /// <remarks>
    /// <para>A process that has not said otherwise is DPI UNAWARE, and Windows lies to it
    /// kindly: <c>GetClientRect</c> comes back in scaled units and <c>CopyFromScreen</c>
    /// hands over a downscaled copy of the desktop. On a display at 100% the lie costs
    /// nothing, which is why this was never needed. At 200% every measurement this harness
    /// makes is halved.</para>
    ///
    /// <para>THE FAILURE THAT LOOKS LIKE, measured 2026-09-05 on a 2240x1080 remote display
    /// at 200%: the run asks Unity for a 1280x720 window, the window-size check reports
    /// exactly 640x360 - a factor of two, which is the scale factor rather than a clamp -
    /// and every screen match then fails, because the references are 1280x720 and the
    /// regions compared are fixed pixel rectangles. The game is fine and says so in its own
    /// log; the harness simply cannot see what it is looking at. It reports
    /// <c>unknown (nearest main-menu at 0.2573, needs 0.0313)</c> with plenty of detail in
    /// the frame, which is what separates this from the blank capture
    /// <see cref="DisplayAwake"/> describes.</para>
    ///
    /// <para>Per-monitor v2 rather than system awareness, when the machine has it: the two
    /// are the same thing on a single display, and on two displays at different scalings
    /// only per-monitor tells the truth about the one the game is actually on. The older
    /// calls are kept as fallbacks - each of these entry points arrived in a different
    /// Windows, and a missing one throws at the call rather than at load.</para>
    ///
    /// <para>ONCE, AND FIRST. Awareness cannot be changed after the process has used a
    /// window or a device context, so this belongs at the top of an entry point and nowhere
    /// else. A second call is harmless and reports that it was already set.</para>
    /// </remarks>
    public static class DisplayScaling
    {
        /// <summary>What one call to <see cref="Enable"/> achieved.</summary>
        /// <param name="Aware">
        /// Whether the process now measures in real pixels. False means every screen
        /// measurement is scaled by whatever the display is set to.
        /// </param>
        /// <param name="ScalePercent">
        /// The primary display's scaling, as a percentage, or 0 where it cannot be read.
        /// 100 is the setting everything here was written against.
        /// </param>
        /// <param name="What">One sentence for a log, saying what happened and what it costs.</param>
        public readonly record struct ScalingReport(bool Aware, int ScalePercent, string What);

        /// <summary>DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, which is a handle, not a value.</summary>
        private static readonly IntPtr PerMonitorAwareV2 = new IntPtr(-4);

        /// <summary>PROCESS_PER_MONITOR_DPI_AWARE, for the Windows 8.1 call.</summary>
        private const int ProcessPerMonitorDpiAware = 2;

        /// <summary>The DPI a display reports when it is at 100%.</summary>
        private const int UnscaledDpi = 96;

        /// <summary>
        /// Declares this process DPI aware, by the best call the machine offers.
        /// </summary>
        /// <returns>What it managed, for the caller to report.</returns>
        public static ScalingReport Enable()
        {
            int scale = ScalePercent();

            if (IsAware())
            {
                return new ScalingReport(
                    true,
                    scale,
                    $"the screen is measured in real pixels already, at {Describe(scale)}");
            }

            if (TrySetPerMonitorV2())
            {
                // Read again: the answer before awareness was set is the scaled one.
                scale = ScalePercent();
                return new ScalingReport(
                    true,
                    scale,
                    $"the screen is measured in real pixels, per monitor, at {Describe(scale)}");
            }

            if (TrySetPerMonitor() || TrySetSystemAware())
            {
                scale = ScalePercent();
                return new ScalingReport(
                    true,
                    scale,
                    "the screen is measured in real pixels, at the system's scaling rather "
                        + $"than per monitor, at {Describe(scale)}");
            }

            return new ScalingReport(
                false,
                scale,
                $"the screen is scaled to {Describe(scale)} and this process could not ask "
                    + "for real pixels, so every window size and screenshot here is that "
                    + "fraction of the truth and no reference will match");
        }

        /// <summary>
        /// Whether this process measures the screen in real pixels right now.
        /// </summary>
        /// <remarks>
        /// For code that has to decide whether a measurement of the DESKTOP can be
        /// trusted, rather than one of a window. <see cref="Enable"/> answers the same
        /// question but sets awareness as a side effect, which belongs at an entry point
        /// and nowhere else; this only asks. False does not mean the display is scaled -
        /// it means a reading taken here may be a fraction of the truth and cannot be used
        /// to call anything impossible.
        /// </remarks>
        public static bool MeasuresRealPixels() => IsAware();

        /// <summary>Whether this process already measures in real pixels.</summary>
        private static bool IsAware()
        {
            try
            {
                return IsProcessDPIAware();
            }
            catch (Exception error) when (IsMissingEntryPoint(error))
            {
                return false;
            }
        }

        /// <summary>The primary display's scaling as a percentage, or 0 if unreadable.</summary>
        /// <remarks>
        /// <c>GetDpiForSystem</c> is Windows 10 1607 and later. Before that there is no
        /// answer worth reporting from a process that is not yet aware, so 0 says so rather
        /// than guessing 100 and putting a wrong number in a log.
        /// </remarks>
        private static int ScalePercent()
        {
            try
            {
                return (int)Math.Round(GetDpiForSystem() * 100d / UnscaledDpi);
            }
            catch (Exception error) when (IsMissingEntryPoint(error))
            {
                return 0;
            }
        }

        /// <summary>That scaling as words, for a sentence.</summary>
        private static string Describe(int scalePercent) =>
            scalePercent > 0 ? scalePercent + "%" : "an unreadable scaling";

        /// <summary>Windows 10 1703 and later.</summary>
        private static bool TrySetPerMonitorV2()
        {
            try
            {
                return SetProcessDpiAwarenessContext(PerMonitorAwareV2);
            }
            catch (Exception error) when (IsMissingEntryPoint(error))
            {
                return false;
            }
        }

        /// <summary>Windows 8.1 and later. Returns S_OK, or E_ACCESSDENIED if already set.</summary>
        private static bool TrySetPerMonitor()
        {
            try
            {
                return SetProcessDpiAwareness(ProcessPerMonitorDpiAware) == 0;
            }
            catch (Exception error) when (IsMissingEntryPoint(error))
            {
                return false;
            }
        }

        /// <summary>Vista and later, and the whole desktop rather than one monitor.</summary>
        private static bool TrySetSystemAware()
        {
            try
            {
                return SetProcessDPIAware();
            }
            catch (Exception error) when (IsMissingEntryPoint(error))
            {
                return false;
            }
        }

        /// <summary>
        /// Whether a call failed because this Windows does not have it, which is the only
        /// failure here worth stepping over - anything else is a real one.
        /// </summary>
        private static bool IsMissingEntryPoint(Exception error) =>
            error is EntryPointNotFoundException || error is DllNotFoundException;

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetProcessDpiAwarenessContext(IntPtr context);

        [DllImport("shcore.dll")]
        private static extern int SetProcessDpiAwareness(int awareness);

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetProcessDPIAware();

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool IsProcessDPIAware();

        [DllImport("user32.dll")]
        private static extern uint GetDpiForSystem();
    }
}
