// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.IO;
using System.Threading;
using GlobalConversationTracker.Automation;

namespace GlobalConversationTracker.Harness
{
    /// <summary>Getting the first save of a run into a game that has just launched.</summary>
    /// <remarks>
    /// Shared by every in-game run rather than written out per run. The awkward part is not
    /// the keypress but knowing WHEN to send it, and a second copy of that judgement would
    /// be the one that pressed blind.
    /// </remarks>
    internal static class FirstSave
    {
        /// <summary>
        /// How long to give one Enter before pressing it again. Short enough to walk
        /// through a splash screen or a run of dialogue briskly, long enough that a
        /// loading screen is not hammered.
        /// </summary>
        private static readonly TimeSpan BetweenPresses = TimeSpan.FromSeconds(2);

        /// <summary>
        /// Gets the first save of a run in, by waiting for the main menu and pressing
        /// Continue.
        /// </summary>
        /// <remarks>
        /// <para>Continue rather than a named load, because loading from the menu through
        /// the probe dies in HudToggle.FixForDreamScene - the HUD views that path expects
        /// are not built yet - and Continue takes the newest save, which is what the
        /// caller's staging order arranges. Once a save is in and the HUD exists the probe
        /// can load the rest by name.</para>
        ///
        /// <para>Waiting for the menu by looking at it, rather than pressing Enter every
        /// two seconds and hoping, is what makes the press land somewhere known. The
        /// watcher also skips the logo deliberately, and refuses to send anything at a
        /// window that is not in front - a keypress goes to whatever IS in front, so
        /// pressing blind types into somebody else's window and reports nothing.</para>
        ///
        /// <para>Without a phase file there is nothing to look at, so it falls back to
        /// the old blind pressing. That is a worse way to do it, not a broken one.</para>
        /// </remarks>
        /// <param name="watcher">The probe watcher to wait on.</param>
        /// <param name="timeout">How long any single wait may take.</param>
        /// <param name="report">Where the main-menu check is recorded.</param>
        /// <param name="progress">Called with each step, for verbose output.</param>
        public static void Get(
            ProbeWatcher watcher, TimeSpan timeout, Report report, Action<string> progress)
        {
            StartupWatcher? startup = LoadStartupWatcher(progress);
            if (startup == null)
            {
                PressEnterUntil(
                    watcher,
                    e => e.Name == "save-applied",
                    timeout,
                    "a save starts loading",
                    "still on a splash screen");
                return;
            }

            WaitResult atMenu = startup.WaitForMenu(timeout);
            report.Check(
                atMenu.Succeeded,
                "the main menu is on screen",
                atMenu.ToString());

            if (!atMenu.Succeeded)
            {
                throw new TimeoutException(
                    $"The main menu never appeared: {atMenu}. Nothing can be loaded from a "
                    + "screen the run cannot identify.");
            }

            // One press, at a screen known to be the menu, on a window known to be in
            // front. Retried only if the save does not start, since a single lost
            // keypress should not cost the whole run.
            PressEnterUntil(
                watcher,
                e => e.Name == "save-applied",
                timeout,
                "a save starts loading",
                "waiting at the main menu");
        }

        /// <summary>
        /// The watcher that recognises the main menu, or null when it cannot be built.
        /// </summary>
        private static StartupWatcher? LoadStartupWatcher(Action<string> progress)
        {
            string phasePath = Path.Combine(
                GameInstall.RepoRoot(), "testing", StartupPhases.DefaultFileName);
            StartupPhase[] phases;
            try
            {
                phases = StartupPhases.Load(phasePath);
            }
            catch (Exception error)
            {
                Console.WriteLine(
                    $"        (no startup phases at {phasePath}, so pressing Enter blindly: "
                    + $"{error.Message})");
                return null;
            }

            StartupPhase? menu = Array.Find(
                phases, phase => phase.Name == StartupWatcher.MenuPhaseName);
            if (menu == null)
            {
                Console.WriteLine(
                    $"        (no '{StartupWatcher.MenuPhaseName}' phase, so pressing Enter "
                    + "blindly)");
                return null;
            }

            return new StartupWatcher(
                "disco",
                phases,
                menu.Fingerprint,
                menu.Region,
                menu.Threshold,
                progress: progress);
        }

        /// <summary>
        /// Presses Enter until the probe reports what is being waited for.
        /// </summary>
        /// <remarks>
        /// <para>Two places need this and neither can be timed. Nothing says when the main
        /// menu is actually on screen - both events that sound like it fire about ten
        /// seconds in, while the legal notice and the logo still have twenty-five seconds
        /// to run - and nothing says when a conversation has finished showing the lines
        /// that precede its first response menu.</para>
        ///
        /// <para>An Enter that lands on a splash screen skips it, one that lands on the
        /// menu starts the newest save, and one that lands on a line of dialogue advances
        /// it. So pressing until the awaited thing happens is both the simplest thing that
        /// works and the fastest way through.</para>
        ///
        /// <para>The last press can race the menu it was waiting for and pick an option.
        /// That is harmless: the menu has already been reported by then, with every
        /// option's text, and the next scenario loads a save over whatever it chose.</para>
        /// </remarks>
        private static ProbeEvent PressEnterUntil(
            ProbeWatcher watcher,
            Func<ProbeEvent, bool> matches,
            TimeSpan timeout,
            string what,
            string whileWaiting)
        {
            GameWindow window = GameSession.WaitForWindow("disco", TimeSpan.FromSeconds(60));
            var clock = Stopwatch.StartNew();
            var raiser = new ForegroundRaiser();
            string reported = string.Empty;

            while (true)
            {
                // Not sent unless the game is in front. A keypress goes to the foreground
                // window, so pressing anyway types Enter into whatever that is - and the
                // run then reports that the game never answered, which is true and
                // completely misleading.
                if (!raiser.Ensure(window.Handle))
                {
                    string state = raiser.Describe();
                    if (reported != state)
                    {
                        reported = state;
                        Console.Error.WriteLine(
                            $"        not pressing Enter: the game is {state}");
                    }

                    if (clock.Elapsed >= timeout)
                    {
                        throw new TimeoutException(
                            $"Waited {timeout.TotalSeconds:N0}s for {what} and never got the "
                            + "game in front to ask for it. Something else is holding focus.");
                    }

                    Thread.Sleep(BetweenPresses);
                    continue;
                }

                GameSession.SendKey("Enter");

                try
                {
                    ProbeEvent found = watcher.WaitFor(matches, BetweenPresses, what);
                    Console.WriteLine($"        {what} after {clock.Elapsed.TotalSeconds:N0}s");
                    return found;
                }
                catch (TimeoutException)
                {
                    if (clock.Elapsed >= timeout)
                    {
                        throw new TimeoutException(
                            $"Pressed Enter for {timeout.TotalSeconds:N0}s and {what} never "
                            + "happened. The keypresses may be going to another window.");
                    }

                    Console.WriteLine(
                        $"        {whileWaiting} ({clock.Elapsed.TotalSeconds:N0}s)");
                }
            }
        }
    }
}
