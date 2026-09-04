// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>What a wait saw, whether or not it got what it wanted.</summary>
    /// <remarks>
    /// Every field here exists because a bare true/false hid something. "Never settled"
    /// and "settled instantly on a blank window" are the same boolean and completely
    /// different problems.
    /// </remarks>
    public sealed class WaitResult
    {
        /// <summary>Creates a result.</summary>
        /// <param name="succeeded">Whether the wait got what it was waiting for.</param>
        /// <param name="difference">The last difference measured.</param>
        /// <param name="detail">The last detail measured.</param>
        /// <param name="sawMotion">Whether the screen was ever seen to change.</param>
        /// <param name="elapsed">How long the wait took.</param>
        public WaitResult(
            bool succeeded, double difference, double detail, bool sawMotion, TimeSpan elapsed)
        {
            Succeeded = succeeded;
            Difference = difference;
            Detail = detail;
            SawMotion = sawMotion;
            Elapsed = elapsed;
        }

        /// <summary>Whether the wait got what it was waiting for.</summary>
        public bool Succeeded { get; }

        /// <summary>The last difference measured, 0..1.</summary>
        public double Difference { get; }

        /// <summary>The last detail measured, 0..1. Near zero means nothing was drawn.</summary>
        public double Detail { get; }

        /// <summary>Whether the screen was ever seen to change.</summary>
        public bool SawMotion { get; }

        /// <summary>How long the wait took.</summary>
        public TimeSpan Elapsed { get; }

        /// <summary>
        /// Whether the screen this waited on had essentially nothing on it.
        /// </summary>
        /// <remarks>
        /// The difference between "the run is looking at the wrong screen" and "the run is
        /// not looking at a screen at all". A wrong screen has plenty of detail and simply
        /// does not match; a blank capture is what a display that has gone to sleep gives,
        /// and it is a fault of the machine rather than of the game.
        /// </remarks>
        public bool LooksBlank => !Succeeded && Detail < GameSession.BlankDetailFloor;

        /// <inheritdoc/>
        public override string ToString()
        {
            string reading =
                $"{(Succeeded ? "ok" : "TIMED OUT")} after {Elapsed.TotalSeconds:N0}s "
                + $"(difference {Difference:N5}, detail {Detail:N3}, saw motion: {SawMotion})";

            if (!LooksBlank)
            {
                return reading;
            }

            // Named rather than left to be worked out from a number nobody looks up. A run
            // that failed this way has almost certainly been reading a sleeping display,
            // and the remedies - wake it, or unlock the session - are nothing like the ones
            // for a run that landed on the wrong screen.
            return reading
                + ". Almost nothing was on screen, which is what a display that has gone to "
                + "sleep looks like rather than what a wrong screen looks like. The run holds "
                + "the display awake while it works, so if this persists the session is "
                + "probably LOCKED - which nothing running inside it can undo.";
        }
    }

    /// <summary>Driving a running game: waiting on its screen, pressing its keys.</summary>
    public static class GameSession
    {
        /// <summary>
        /// A difference small enough that nothing meaningful changed on screen.
        /// </summary>
        /// <remarks>
        /// No longer used to decide anything - waiting for the screen to stop changing is
        /// gone, because every screen in this game animates and the one that does hold
        /// still is the legal notice, motionless for 25 seconds in the middle of startup.
        /// It survives as a yardstick: a change this small is not something a person would
        /// see, which is what makes it the right scale to say a look-ahead asterisk is
        /// invisible to a mean.
        /// </remarks>
        public const double StillThreshold = 0.005;

        /// <summary>
        /// A fingerprint flatter than this holds nothing worth looking at.
        /// </summary>
        /// <remarks>
        /// Reporting only. It was a gate once, on the theory that a flat screen is a
        /// window that has not painted yet - but the legal notice measures 0.008, well
        /// under this, while being a screen the game deliberately shows for 25 seconds.
        /// So it distinguishes "almost nothing on screen" from "something", which is worth
        /// saying in a timeline, and decides nothing.
        /// </remarks>
        public const double BlankDetailFloor = 0.02;

        /// <summary>Waits for the game's rendering window to appear.</summary>
        /// <param name="processName">The process name, without .exe.</param>
        /// <param name="timeout">How long to wait.</param>
        /// <exception cref="TimeoutException">It never appeared.</exception>
        public static GameWindow WaitForWindow(string processName, TimeSpan timeout)
        {
            DateTime deadline = DateTime.UtcNow + timeout;
            while (DateTime.UtcNow < deadline)
            {
                GameWindow? window = FindGameWindow(processName);
                if (window != null)
                {
                    return window;
                }

                Thread.Sleep(500);
            }

            var seen = new List<string>();
            foreach (GameWindow candidate in AllWindows(processName))
            {
                seen.Add($"{candidate.ClassName} '{candidate.Title}'");
            }

            string detail = seen.Count > 0 ? $" Windows seen: {string.Join("; ", seen)}." : string.Empty;
            throw new TimeoutException(
                $"No '{processName}' game window appeared within {timeout.TotalSeconds:N0}s.{detail}");
        }

        /// <summary>Every visible top-level window the game's processes own.</summary>
        /// <param name="processName">The process name, without .exe.</param>
        public static GameWindow[] AllWindows(string processName)
        {
            var windows = new List<GameWindow>();
            foreach (Process process in Process.GetProcessesByName(processName))
            {
                windows.AddRange(GameWindows.OfProcess(process.Id));
            }

            return windows.ToArray();
        }

        /// <summary>
        /// The game's rendering window, never its console.
        /// </summary>
        /// <remarks>
        /// A BepInEx-modded game owns a console as well as its own window, and
        /// Process.MainWindowHandle usually hands back the console. A console never
        /// changes and is whatever size it was left at, so a screenshot-driven wait
        /// pointed at one reports that loading finished instantly, at a resolution the
        /// game never used - a failure that reads exactly like success.
        /// </remarks>
        /// <param name="processName">The process name, without .exe.</param>
        /// <returns>The window, or null if it is not up yet.</returns>
        public static GameWindow? FindGameWindow(string processName)
        {
            GameWindow[] candidates = AllWindows(processName);
            if (candidates.Length == 0)
            {
                return null;
            }

            var unity = new List<GameWindow>();
            var others = new List<GameWindow>();
            foreach (GameWindow candidate in candidates)
            {
                if (candidate.ClassName == GameWindows.UnityWindowClass)
                {
                    unity.Add(candidate);
                }
                else if (candidate.ClassName != GameWindows.ConsoleWindowClass)
                {
                    others.Add(candidate);
                }
            }

            if (unity.Count == 1)
            {
                return unity[0];
            }

            if (unity.Count > 1)
            {
                throw new InvalidOperationException(
                    $"{unity.Count} Unity windows belong to '{processName}'; cannot tell which is the game.");
            }

            // Never fall back to a console.
            return others.Count == 1 ? others[0] : null;
        }

        /// <summary>Waits until the screen STOPS matching a reference image.</summary>
        /// <remarks>
        /// The counterpart to WaitUntilMatches, and what stands in for "the game did
        /// something". There is no wait for the screen to settle, because nothing in this
        /// game settles: every screen animates, and the one that does hold still - the
        /// legal notice, motionless for 25 seconds at a difference of 0.0002 - is the one
        /// a settle would wrongly stop on.
        ///
        /// Leaving a known screen is observable without any of that. It says a keypress
        /// was received, not that whatever followed has finished.
        /// </remarks>
        /// <param name="window">The window to watch.</param>
        /// <param name="referencePath">The reference image being left behind.</param>
        /// <param name="threshold">How close still counts as matching.</param>
        /// <param name="timeout">How long to wait.</param>
        /// <param name="progress">Called with each sample, for verbose output.</param>
        /// <param name="region">The part of the screen to compare, or null for all of it.</param>
        public static WaitResult WaitUntilStopsMatching(
            GameWindow window,
            string referencePath,
            double threshold,
            TimeSpan timeout,
            Action<string>? progress = null,
            Rectangle? region = null)
        {
            Size referenceSize = GameScreen.SizeOfFile(referencePath);
            if (referenceSize.Width != window.Width || referenceSize.Height != window.Height)
            {
                throw new InvalidOperationException(
                    $"The reference image is {referenceSize.Width}x{referenceSize.Height} but the "
                    + $"window is {window.Width}x{window.Height}.");
            }

            double[] reference = region == null
                ? GameScreen.FingerprintFile(referencePath)
                : GameScreen.FingerprintFileRegion(referencePath, region.Value);

            var clock = Stopwatch.StartNew();
            DateTime deadline = DateTime.UtcNow + timeout;
            double difference = 0;
            double detail = 0;

            while (DateTime.UtcNow < deadline)
            {
                double[] current = region == null
                    ? GameScreen.Fingerprint(window.Handle)
                    : GameScreen.FingerprintRegion(window.Handle, region.Value);
                detail = GameScreen.Detail(current);
                difference = GameScreen.Difference(reference, current);

                progress?.Invoke($"difference from the old screen {difference:N4}");

                if (difference > threshold)
                {
                    return new WaitResult(true, difference, detail, true, clock.Elapsed);
                }

                Thread.Sleep(500);
            }

            return new WaitResult(false, difference, detail, false, clock.Elapsed);
        }

        /// <summary>Waits until the screen matches a reference image.</summary>
        /// <remarks>
        /// For "we are at the main menu", which stillness cannot establish: it cannot
        /// tell one still screen from another.
        /// </remarks>
        /// <param name="window">The window to watch.</param>
        /// <param name="referencePath">The reference image.</param>
        /// <param name="threshold">How close counts as a match.</param>
        /// <param name="timeout">How long to wait.</param>
        /// <param name="progress">Called with each sample, for verbose output.</param>
        /// <param name="region">
        /// The part of the screen to compare, or null for all of it. Worth setting when
        /// the screen is partly animated: matching only the still part is what makes a
        /// tight threshold possible.
        /// </param>
        public static WaitResult WaitUntilMatches(
            GameWindow window,
            string referencePath,
            double threshold,
            TimeSpan timeout,
            Action<string>? progress = null,
            Rectangle? region = null)
        {
            // Before anything else: a reference of a different size still reduces to the
            // same fingerprint grid and compares without complaint, so a reference
            // captured from the wrong window - a console, say - would silently pass for
            // the game. The downscale that makes comparison robust is exactly what hides
            // this, so it has to be checked separately.
            Size referenceSize = GameScreen.SizeOfFile(referencePath);
            if (referenceSize.Width != window.Width || referenceSize.Height != window.Height)
            {
                throw new InvalidOperationException(
                    $"The reference image is {referenceSize.Width}x{referenceSize.Height} but the "
                    + $"window is {window.Width}x{window.Height}. Comparing them would produce a "
                    + "number that means nothing. Recapture the reference.");
            }

            double[] reference = region == null
                ? GameScreen.FingerprintFile(referencePath)
                : GameScreen.FingerprintFileRegion(referencePath, region.Value);
            var clock = Stopwatch.StartNew();
            DateTime deadline = DateTime.UtcNow + timeout;

            double best = 1.0;
            double difference = 1.0;
            double detail = 0.0;

            while (DateTime.UtcNow < deadline)
            {
                double[] current = region == null
                    ? GameScreen.Fingerprint(window.Handle)
                    : GameScreen.FingerprintRegion(window.Handle, region.Value);
                detail = GameScreen.Detail(current);
                difference = GameScreen.Difference(reference, current);
                if (difference < best)
                {
                    best = difference;
                }

                progress?.Invoke($"difference from reference {difference:N4} (best {best:N4})");

                if (difference <= threshold)
                {
                    return new WaitResult(true, difference, detail, true, clock.Elapsed);
                }

                Thread.Sleep(500);
            }

            return new WaitResult(false, best, detail, true, clock.Elapsed);
        }

        /// <summary>Presses and releases a key.</summary>
        /// <param name="key">The key name.</param>
        /// <param name="hold">How long to hold it down.</param>
        /// <param name="after">How long to pause afterwards.</param>
        public static void SendKey(string key, TimeSpan? hold = null, TimeSpan? after = null)
        {
            GameKeyboard.Press(key);
            Thread.Sleep(hold ?? TimeSpan.FromMilliseconds(40));
            GameKeyboard.Release(key);
            Thread.Sleep(after ?? TimeSpan.FromMilliseconds(120));
        }

        /// <summary>Presses several keys in order.</summary>
        /// <param name="keys">The key names.</param>
        /// <param name="progress">Called with each key, for verbose output.</param>
        /// <exception cref="ArgumentNullException"><paramref name="keys"/> is null.</exception>
        public static void SendKeys(IEnumerable<string> keys, Action<string>? progress = null)
        {
            if (keys == null)
            {
                throw new ArgumentNullException(nameof(keys));
            }

            foreach (string key in keys)
            {
                progress?.Invoke($"key: {key}");
                SendKey(key);
            }
        }
    }
}
