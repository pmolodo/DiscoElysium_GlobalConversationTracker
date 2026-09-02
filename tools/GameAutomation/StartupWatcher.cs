// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.Drawing;
using System.Threading;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Waiting for the game to reach its main menu, by looking at the screen.
    /// </summary>
    /// <remarks>
    /// <para>The one implementation both harness paths use. Startup is a sequence of
    /// distinct screens rather than a fade to a still image, and the only thing that can
    /// say which one is up is the screen itself: no hook, event or log line marks the
    /// moment the menu is actually on display, and the two probe events that sound like
    /// it fire about ten seconds in, while the legal notice and logo still have
    /// twenty-five seconds to run.</para>
    ///
    /// <para>Knowing costs less than not knowing. The logo can be skipped with a single
    /// keypress at the right moment instead of Enter pressed blindly every two seconds
    /// and hoping; a silent fifty-second wait becomes a report of where startup has got
    /// to; and a screen that is not the game - because something else is in front - is
    /// skipped rather than measured, which is the difference between waiting and
    /// photographing somebody's terminal.</para>
    ///
    /// <para>An unrecognised screen is not an error. Startup has brief screens that are
    /// not worth naming, and a game update could add more; the wait carries on, because
    /// the menu is what it is really looking for.</para>
    /// </remarks>
    public sealed class StartupWatcher
    {
        /// <summary>The phase name that means the main menu is up.</summary>
        public const string MenuPhaseName = "main-menu";

        /// <summary>How often the screen is looked at.</summary>
        private static readonly TimeSpan PollInterval = TimeSpan.FromMilliseconds(500);

        private readonly string _processName;
        private readonly StartupPhase[] _phases;
        private readonly double[] _menu;
        private readonly Rectangle? _menuRegion;
        private readonly double _menuThreshold;
        private readonly Action<string>? _progress;
        private readonly Action<Bitmap, TimeSpan>? _onUnknownScreen;

        /// <summary>Creates a watcher.</summary>
        /// <param name="processName">The game's process name, for re-finding its window.</param>
        /// <param name="phases">The startup screens to recognise; may be empty.</param>
        /// <param name="menuFingerprint">What the main menu looks like.</param>
        /// <param name="menuRegion">The part of the screen to compare, or null for all.</param>
        /// <param name="menuThreshold">How close a screen must be to count as the menu.</param>
        /// <param name="progress">Called with each change of screen, for output.</param>
        /// <param name="onUnknownScreen">
        /// Given a screen matching no phase, and how far into the wait it appeared. For
        /// saving a picture: a distance says how far off it was, only the image says why.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public StartupWatcher(
            string processName,
            StartupPhase[] phases,
            double[] menuFingerprint,
            Rectangle? menuRegion,
            double menuThreshold,
            Action<string>? progress = null,
            Action<Bitmap, TimeSpan>? onUnknownScreen = null)
        {
            _processName = processName ?? throw new ArgumentNullException(nameof(processName));
            _phases = phases ?? throw new ArgumentNullException(nameof(phases));
            _menu = menuFingerprint ?? throw new ArgumentNullException(nameof(menuFingerprint));
            _menuRegion = menuRegion;
            _menuThreshold = menuThreshold;
            _progress = progress;
            _onUnknownScreen = onUnknownScreen;
        }

        /// <summary>
        /// Waits until the main menu is on screen, or the time runs out.
        /// </summary>
        /// <param name="timeout">How long to wait.</param>
        /// <returns>
        /// What was seen. <see cref="WaitResult.Succeeded"/> is false both when the menu
        /// never came and when the game exited on the way.
        /// </returns>
        public WaitResult WaitForMenu(TimeSpan timeout)
        {
            var clock = Stopwatch.StartNew();
            var raiser = new ForegroundRaiser();

            string reported = string.Empty;
            bool skippedLogo = false;
            double closest = 1.0;
            double detail = 0;
            IntPtr lastHandle = IntPtr.Zero;

            while (clock.Elapsed < timeout)
            {
                // Re-found every poll. Unity destroys and recreates its window when the
                // display mode changes, so a handle captured at launch dies partway
                // through startup - and until it does it reports the old window's size,
                // which is how a switch to windowed 1280x720 once looked like a game
                // stuck at 3840x1200.
                GameWindow? window = GameSession.FindGameWindow(_processName);
                if (window == null)
                {
                    if (Process.GetProcessesByName(_processName).Length == 0)
                    {
                        Report($"{clock.Elapsed.TotalSeconds,5:N1}s  the game has exited");
                        return new WaitResult(false, closest, detail, true, clock.Elapsed);
                    }

                    // Expected briefly, between the old window going and the new arriving.
                    Thread.Sleep(PollInterval);
                    continue;
                }

                if (window.Handle != lastHandle)
                {
                    // A new window is a new screen, and its own chance to be raised.
                    lastHandle = window.Handle;
                    raiser.Reset();
                }

                if (!raiser.Ensure(window.Handle))
                {
                    string state = raiser.Describe();
                    if (reported != state)
                    {
                        reported = state;
                        Report(
                            $"{clock.Elapsed.TotalSeconds,5:N1}s  nothing can be read from the "
                            + $"screen: the game is {state}");
                    }

                    Thread.Sleep(PollInterval);
                    continue;
                }

                using (Bitmap screen = GameScreen.Capture(window.Handle))
                {
                    double[] current = _menuRegion == null
                        ? GameScreen.FingerprintOf(screen)
                        : GameScreen.FingerprintRegion(screen, _menuRegion.Value);

                    detail = GameScreen.Detail(current);
                    double difference = GameScreen.Difference(_menu, current);
                    if (difference < closest)
                    {
                        closest = difference;
                    }

                    if (difference <= _menuThreshold)
                    {
                        Report(
                            $"{clock.Elapsed.TotalSeconds,5:N1}s  main menu ({difference:N4})");
                        return new WaitResult(true, difference, detail, true, clock.Elapsed);
                    }

                    StartupPhase? phase = StartupPhases.Identify(
                        screen, _phases, out double distance);
                    string name = phase?.Name ?? "unknown";

                    if (name != reported)
                    {
                        reported = name;
                        Report(
                            $"{clock.Elapsed.TotalSeconds,5:N1}s  {name}"
                            + DescribeMatch(screen, phase, distance));

                        // A new screen is a fresh chance to raise the window.
                        raiser.Reset();

                        if (phase == null)
                        {
                            _onUnknownScreen?.Invoke(screen, clock.Elapsed);
                        }
                    }

                    // The logo is the one screen worth doing something about: a keypress
                    // skips it. Sent once - pressing again at the menu would choose
                    // whatever is highlighted.
                    if (!skippedLogo && phase != null && phase.Name == "logo")
                    {
                        Report("        skipping the logo with Enter");
                        GameSession.SendKey("Enter");
                        skippedLogo = true;
                    }
                }

                Thread.Sleep(PollInterval);
            }

            return new WaitResult(false, closest, detail, true, clock.Elapsed);
        }

        /// <summary>
        /// What to add after a phase name: how well it matched, or how badly.
        /// </summary>
        /// <remarks>
        /// "unknown" alone never says whether a threshold is slightly too tight or the
        /// screen is something else entirely, and those need opposite fixes.
        /// </remarks>
        private string DescribeMatch(Bitmap screen, StartupPhase? phase, double distance)
        {
            if (phase != null)
            {
                return $" ({distance:N4})";
            }

            StartupPhase? near = StartupPhases.Nearest(screen, _phases, out double nearDistance);
            return near == null
                ? string.Empty
                : $" (nearest {near.Name} at {nearDistance:N4}, needs {near.Threshold:N4})";
        }

        private void Report(string message)
        {
            _progress?.Invoke(message);
        }
    }
}
