// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.Threading;
using GlobalConversationTracker.Automation;

namespace GlobalConversationTracker.Harness
{
    /// <summary>Getting the first save of a run into a game that has just launched.</summary>
    /// <remarks>
    /// Shared by every in-game run rather than written out per run. The awkward part is
    /// knowing WHEN the menu can be pressed, and a second copy of that judgement would be
    /// the one that pressed too early.
    /// </remarks>
    internal static class FirstSave
    {
        /// <summary>How long to wait after a refusal before asking again.</summary>
        private static readonly TimeSpan BetweenAsks = TimeSpan.FromSeconds(2);

        /// <summary>
        /// Gets the first save of a run in, by asking the probe to press the main menu's
        /// Continue from inside the game.
        /// </summary>
        /// <remarks>
        /// <para>NO WINDOW IN FRONT. A keypress goes to whatever window has the focus, so a
        /// run that pressed Enter at the menu had to raise the game first and failed whenever
        /// something else held on to the foreground. The probe calls the menu's own
        /// <c>GameLevelCommand.ContinueGame</c> instead, which is what the button runs.</para>
        ///
        /// <para>CONTINUE RATHER THAN A LOAD BY NAME. Loading by name while the startup
        /// screens are up applies the save's data without taking the player out of the menu
        /// scene - the HUD comes up over the menu's world and no conversation can open. The
        /// menu's command does what the menu does around the load, and Continue takes the
        /// newest save, which is what the caller's staging order arranges. Once a save is in,
        /// the probe loads the rest by name.</para>
        ///
        /// <para>The probe refuses until the main menu is showing and offers Continue, so
        /// this asks again after each refusal - and only then. A press it accepted starts a
        /// load that is under way before it is applied, and a second press would start
        /// another on top of it.</para>
        /// </remarks>
        /// <param name="watcher">The probe watcher to wait on.</param>
        /// <param name="timeout">How long to keep asking.</param>
        /// <param name="saveGames">The profile's SaveGames folder.</param>
        public static void Get(ProbeWatcher watcher, TimeSpan timeout, string saveGames)
        {
            var clock = Stopwatch.StartNew();
            string lastRefusal = string.Empty;
            while (true)
            {
                ProbeCommand.SendContinueGame(saveGames);
                ProbeEvent found = watcher.WaitFor(
                    e => e.Name == "save-applied"
                        || (e.Name == "command-failed"
                            && e.Text("command") == ProbeCommand.ContinueGame),
                    timeout,
                    "the probe to press Continue or say why it cannot");
                if (found.Name == "save-applied")
                {
                    Console.WriteLine(
                        $"        a save starts loading after {clock.Elapsed.TotalSeconds:N0}s");
                    return;
                }

                // Said once per reason rather than once per ask: the menu takes half a
                // minute to come up, and the same refusal every two seconds says nothing.
                string refusal = found.Text("message") ?? string.Empty;
                if (refusal != lastRefusal)
                {
                    lastRefusal = refusal;
                    Console.WriteLine(
                        $"        not yet at {clock.Elapsed.TotalSeconds:N0}s: {refusal}");
                }

                if (clock.Elapsed >= timeout)
                {
                    throw new TimeoutException(
                        $"Asked the probe to press Continue for {timeout.TotalSeconds:N0}s and "
                        + $"no save was applied. The last refusal was: {refusal}");
                }

                Thread.Sleep(BetweenAsks);
            }
        }
    }
}
