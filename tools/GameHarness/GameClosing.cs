// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.Threading;
using GlobalConversationTracker.Automation;

namespace GlobalConversationTracker.Harness
{
    /// <summary>Ending a run's game, politely first and forcibly second.</summary>
    /// <remarks>
    /// Shared by every in-game run rather than written out per run. What is easy to get
    /// subtly wrong here is not the killing but the two things around it - asking first,
    /// and waiting afterwards - and a second copy would be the one that forgot.
    /// </remarks>
    internal static class GameClosing
    {
        /// <summary>The game's process name.</summary>
        private const string ProcessName = "disco";

        /// <summary>How long to wait for the game to close itself before killing it.</summary>
        private static readonly TimeSpan QuitDeadline = TimeSpan.FromSeconds(30);

        /// <summary>
        /// How long to let go of the profile folder before it is moved back.
        /// </summary>
        /// <remarks>
        /// Windows will not move a folder the game still has open, and a process that has
        /// exited has not necessarily released its handles by the time it disappears from
        /// the process list.
        /// </remarks>
        private static readonly TimeSpan ProfileRelease = TimeSpan.FromSeconds(2);

        /// <summary>
        /// Asks the game to close itself, and waits for it to go.
        /// </summary>
        /// <remarks>
        /// Killing it is the fallback, not the plan. A killed game runs neither
        /// Application.quitting nor AppDomain.ProcessExit, which is where the mod writes
        /// everything it has been holding - so a run that killed would be measuring a
        /// shutdown no player ever performs.
        /// </remarks>
        /// <param name="saveGames">The profile's SaveGames folder, where commands go.</param>
        /// <param name="process">The launched process, for reporting.</param>
        public static void Quit(string saveGames, Process? process)
        {
            Console.WriteLine("asking the game to close...");
            try
            {
                ProbeCommand.SendQuit(saveGames);
            }
            catch (Exception error)
            {
                Console.Error.WriteLine($"  could not ask: {error.Message}");
                return;
            }

            var clock = Stopwatch.StartNew();
            while (clock.Elapsed < QuitDeadline)
            {
                if (Process.GetProcessesByName(ProcessName).Length == 0)
                {
                    Console.WriteLine($"  it closed after {clock.Elapsed.TotalSeconds:N0}s");
                    return;
                }

                Thread.Sleep(500);
            }

            Console.Error.WriteLine(
                $"  still running after {QuitDeadline.TotalSeconds:N0}s; it will be closed "
                + "the hard way, and anything the mod writes on the way out will be lost.");
        }

        /// <summary>Kills whatever is still running, and waits for the folder to go free.</summary>
        /// <param name="process">The launched process, disposed here.</param>
        public static void Close(Process? process)
        {
            foreach (Process running in Process.GetProcessesByName(ProcessName))
            {
                try
                {
                    running.Kill();
                    running.WaitForExit(10_000);
                }
                catch (Exception error)
                {
                    Console.Error.WriteLine($"  could not close the game: {error.Message}");
                }
            }

            process?.Dispose();

            // The profile is moved back next, and Windows will not move a folder the game
            // still has open.
            Thread.Sleep(ProfileRelease);
        }
    }
}
