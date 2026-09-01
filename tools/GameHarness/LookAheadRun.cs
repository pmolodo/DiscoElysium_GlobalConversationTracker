// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using GlobalConversationTracker.Automation;

namespace GlobalConversationTracker.Harness
{
    /// <summary>One money scenario and what its menu should look like.</summary>
    public sealed class LookAheadScenario
    {
        /// <summary>Creates a scenario.</summary>
        /// <param name="saveName">The staged save's name.</param>
        /// <param name="money">The balance it sets, in centimes.</param>
        /// <param name="expectOrange">Whether an option should carry the orange marker.</param>
        /// <param name="why">Why, in one line, for the report.</param>
        public LookAheadScenario(
            string saveName, int money, bool expectOrange, string why)
        {
            SaveName = saveName;
            Money = money;
            ExpectOrange = expectOrange;
            Why = why;
        }

        /// <summary>The staged save's name.</summary>
        public string SaveName { get; }

        /// <summary>The balance it sets, in centimes.</summary>
        public int Money { get; }

        /// <summary>Whether an option should carry the orange marker.</summary>
        public bool ExpectOrange { get; }

        /// <summary>Why, for the report.</summary>
        public string Why { get; }
    }

    /// <summary>
    /// Drives the look-ahead money scenarios: one launch, three saves, and the marker
    /// read off the option text the game was about to draw.
    /// </summary>
    /// <remarks>
    /// <para>Conversation 451 gates a 0.50 real purchase behind a 50.00 one, and the
    /// staged global state leaves exactly one entry unseen - the one only a speaker
    /// buyer reaches. An option therefore carries the orange marker precisely when the
    /// crawl spent 50.00 and still had 0.50, which is what the three balances separate.
    /// The middle one is the point: a scan that checked an option's price without
    /// subtracting what the path already spent would mark it.</para>
    ///
    /// <para>One launch for all three. A cold start is about a minute, and the probe can
    /// load a save in place, so relaunching per scenario would triple the run for
    /// nothing.</para>
    /// </remarks>
    public static class LookAheadRun
    {
        /// <summary>The conversation the scenarios open.</summary>
        public const int ConversationId = 451;

        /// <summary>The colour meaning "leads somewhere no save has reached".</summary>
        public const string OrangeHtml = "#FF8C42";

        /// <summary>The colour meaning "leads somewhere this save has not".</summary>
        public const string RedHtml = "#C4453C";

        /// <summary>What a packed save archive is called.</summary>
        private const string SaveExtension = ".ntwtf.zip";

        /// <summary>The scenarios, in the order they are run.</summary>
        public static readonly LookAheadScenario[] Scenarios =
        {
            new LookAheadScenario(
                "afford-both", 5100, true,
                "100 centimes left after the sneakers, so the speakers are still affordable"),
            new LookAheadScenario(
                "afford-only-sneakers", 5025, false,
                "25 centimes left after the sneakers, so the speakers are not"),
            new LookAheadScenario(
                "afford-neither", 4900, false,
                "the sneakers cannot be bought at all"),
        };

        /// <summary>Runs every scenario in one session.</summary>
        /// <param name="game">Path to disco.exe.</param>
        /// <param name="scenarioRoot">Where the built scenarios are.</param>
        /// <param name="settingsFile">The test settings to stage.</param>
        /// <param name="artifacts">Where to write screenshots.</param>
        /// <param name="timeout">How long any single wait may take.</param>
        /// <param name="keepOpen">Leave the game running at the end.</param>
        /// <returns>0 when every check passed.</returns>
        public static int Run(
            string game,
            string scenarioRoot,
            string settingsFile,
            string artifacts,
            TimeSpan timeout,
            bool keepOpen)
        {
            var checks = new List<string>();
            var failures = new List<string>();

            void Check(bool ok, string label, string detail)
            {
                checks.Add(label);
                Console.WriteLine($"  {(ok ? "PASS" : "FAIL")}  {label}");
                if (detail.Length > 0)
                {
                    Console.WriteLine($"        {detail}");
                }

                if (!ok)
                {
                    failures.Add(label);
                }
            }

            string logPath = Path.Combine(
                FilePaths.FolderOf(game, nameof(game)), "BepInEx", "LogOutput.log");
            string saveGames = GameProfile.SavesFolder;
            string globalState = Path.Combine(scenarioRoot, "global-conversation-state.json");

            var packed = new List<string>();

            // The packer stamps the time into the archive's name, and the game keys a
            // save by exactly that name, so what gets staged is "afford-both(9_1_2026
            // 8-14-05 AM)" and not "afford-both". Asking the game to load the scenario's
            // own name found nothing and failed silently - the load simply did not
            // happen - so the staged name is what the load command has to carry.
            var stagedNames = new Dictionary<string, string>(StringComparer.Ordinal);

            // Packed in REVERSE order so the first scenario's archive is the newest.
            // The first save is loaded by pressing Continue at the main menu, which takes
            // the newest one, and that path is used because loading from the menu through
            // the probe does not work: SunshinePersistence.Load called with no menu
            // interaction dies in HudToggle.FixForDreamScene, since the HUD views the
            // load path expects have not been built. Once a save is in and the HUD
            // exists, the probe can load the rest.
            foreach (LookAheadScenario scenario in Enumerable.Reverse(Scenarios))
            {
                string expanded = Path.Combine(scenarioRoot, scenario.SaveName + ".ntwtf");
                if (!Directory.Exists(expanded))
                {
                    throw new DirectoryNotFoundException(
                        $"No scenario save at {expanded}.");
                }

                string archive = Program.PackSave(expanded, artifacts);
                packed.Add(archive);

                string fileName = Path.GetFileName(archive);
                stagedNames[scenario.SaveName] =
                    fileName.EndsWith(SaveExtension, StringComparison.OrdinalIgnoreCase)
                        ? fileName.Substring(0, fileName.Length - SaveExtension.Length)
                        : fileName;
                Console.WriteLine(
                    $"           {scenario.SaveName} staged as \"{stagedNames[scenario.SaveName]}\"");
            }

            if (!File.Exists(globalState))
            {
                throw new FileNotFoundException(
                    $"No staged global state at {globalState}.",
                    globalState);
            }

            Process? process = null;

            // The same staging every in-game run uses, so they all measure the game
            // under the same conditions. It carries the display too: the settings file
            // does not size the window, Unity's PlayerPrefs do, and staging only the
            // file left this run at whatever resolution the machine happened to be at.
            using StagedGame staged = StagedGame.Stage(
                "disco",
                settingsFile,
                packed,
                globalState,
                progress: message => Console.WriteLine($"staging:   {message}"));

            // The probe is this verb's own dependency: without it there is nothing to
            // drive the game with and nothing to read the markers off. Installed here
            // rather than by the caller so the verb is runnable on its own, and inside
            // the staging so a failure anywhere below still takes it out again.
            using ProbeDeployment probe = ProbeDeployment.Deploy(
                game,
                GameInstall.FindProbeAssembly(),
                message => Console.WriteLine($"probe:     {message}"));

            try
            {
                Console.WriteLine(
                    $"staged:    {packed.Count} saves and a global state into {saveGames}");

                // Before launching, not after. BepInEx truncates its log when it starts,
                // but the harness begins reading the instant the process exists, and in
                // that gap it would find the PREVIOUS run's events - see a stale
                // world-ready, believe the menu was up, and fire the first command at
                // chainload time, when nothing can load a save yet.
                if (File.Exists(logPath))
                {
                    File.Delete(logPath);
                    Console.WriteLine("log:       cleared the previous run's BepInEx log");
                }

                Console.WriteLine();
                Console.WriteLine("launching...");
                process = Process.Start(new ProcessStartInfo(game) { UseShellExecute = false });

                var watcher = new ProbeWatcher(logPath);
                watcher.WaitForEvent("ready", timeout, Log);
                Check(true, "the probe loaded", $"reading {logPath}");

                // Checked, not assumed. Everything downstream is measured against a
                // window of a known size, and a run at the machine's own resolution
                // would still pass every marker check while testing something else.
                GameWindow window = GameSession.WaitForWindow("disco", timeout);
                Check(
                    window.Width == staged.Requested.Width
                        && window.Height == staged.Requested.Height,
                    $"the window is the requested {staged.Requested}",
                    $"got {window.Width}x{window.Height}");

                // The probe says "ready" the moment BepInEx chainloads it, which is long
                // before the game can load anything - SunshinePersistence has no instance
                // yet, and a command sent then fails on nothing being there. world-ready
                // fires when the HUD is built, which at startup means the menu is up.
                watcher.WaitForEvent("main-menu", timeout, Log);
                Check(true, "the game reached its main menu", "Continue can be pressed");

                for (int i = 0; i < Scenarios.Length; i++)
                {
                    LookAheadScenario scenario = Scenarios[i];
                    Console.WriteLine();
                    Console.WriteLine($"--- {scenario.SaveName} ({scenario.Money} centimes) ---");

                    watcher.Mark();
                    if (i == 0)
                    {
                        ContinueFromMenu(watcher, timeout);
                    }
                    else
                    {
                        ProbeCommand.SendLoadSave(saveGames, stagedNames[scenario.SaveName]);
                        watcher.WaitForEvent("save-applied", timeout, Log);
                    }

                    RunScenario(scenario, saveGames, watcher, timeout, Check);
                }
            }
            finally
            {
                if (keepOpen)
                {
                    staged.Abandon();
                    Console.Error.WriteLine();
                    Console.Error.WriteLine(
                        "Left the game running, so NOTHING was restored. The player's profile "
                        + $"is at {staged.ProfileMovedTo} and their PlayerPrefs at "
                        + $"{staged.RegistryBackupPath}.");
                }
                else
                {
                    Console.WriteLine();
                    Console.WriteLine("closing the game...");
                    Close(process);
                    staged.Restore();
                }
            }

            Console.WriteLine();
            Console.WriteLine(
                $"{checks.Count - failures.Count}/{checks.Count} checks passed");
            foreach (string failure in failures)
            {
                Console.WriteLine($"  FAILED: {failure}");
            }

            return failures.Count == 0 ? 0 : 1;
        }

        private static void RunScenario(
            LookAheadScenario scenario,
            string saveGames,
            ProbeWatcher watcher,
            TimeSpan timeout,
            Action<bool, string, string> check)
        {
            watcher.WaitForEvent("load-finished", timeout, Log);

            ProbeCommand.SendStartConversation(saveGames, ConversationId);

            // The conversation opens on narration, not on a menu: StartConversation puts
            // the first line up and the game waits to be told to go on, exactly as it
            // would for a player. Enter advances it until the options appear.
            //
            // "complete" only. The game composes each menu twice - once per response-UI
            // path - so the recorder reports the first pass as superseded when the second
            // begins. Both carry the same options, but only the completed one is tied to
            // its conversation and its balance, and matching either would make the run
            // depend on which arrived first.
            ProbeEvent menu = PressEnterUntil(
                watcher,
                e => e.Name == "menu"
                    && e.Number("conversation") == ConversationId
                    && e.Text("state") == "complete",
                timeout,
                $"a response menu in conversation {ConversationId}",
                "advancing dialogue");

            // Taken from the menu rather than from load-finished. load-finished is
            // emitted when the game's IsLoading flag falls, which is a poll boundary
            // earlier than the loaded save's money reaching Lua - it reported the
            // previous scenario's balance. The menu's reading is the one the look-ahead
            // actually crawled from, which is the number this check is about.
            int? money = menu.Number("money");
            check(
                money == scenario.Money,
                $"{scenario.SaveName}: the look-ahead crawled from {scenario.Money} centimes",
                $"the probe reports {money?.ToString() ?? "nothing"}");

            ProbeOption[] options = menu.Options();
            check(
                options.Length > 0,
                $"{scenario.SaveName}: the response menu was drawn",
                $"{options.Length} option(s), state {menu.Text("state")}");

            ProbeOption[] orange = options.Where(o => o.HasMarker(OrangeHtml)).ToArray();
            ProbeOption[] red = options.Where(o => o.HasMarker(RedHtml)).ToArray();

            foreach (ProbeOption option in options)
            {
                Console.WriteLine(
                    $"        [{(option.HasMarker(OrangeHtml) ? "orange" : option.HasMarker(RedHtml) ? "red   " : "      ")}] "
                    + $"{option.ConversationId}:{option.EntryId} {Trim(option.Text)}");
            }

            check(
                (orange.Length > 0) == scenario.ExpectOrange,
                $"{scenario.SaveName}: "
                    + (scenario.ExpectOrange
                        ? "an option leads to the unseen line"
                        : "no option leads to the unseen line"),
                $"{orange.Length} orange, {red.Length} red - {scenario.Why}");
        }

        /// <summary>
        /// How long to give one Enter before pressing it again. Short enough to walk
        /// through a splash screen or a run of dialogue briskly, long enough that a
        /// loading screen is not hammered.
        /// </summary>
        private static readonly TimeSpan BetweenPresses = TimeSpan.FromSeconds(2);

        /// <summary>
        /// Presses Enter until a save actually starts loading.
        /// </summary>
        /// <remarks>
        /// <para>The first save has to come in through the menu. Asking the probe to
        /// load one straight from the main menu throws inside the game, in
        /// HudToggle.FixForDreamScene, because the HUD views the load path expects have
        /// not been built - so the run does what a player does once, and drives the rest
        /// from inside a session that has a HUD.</para>
        ///
        /// <para>Repeated rather than timed, because nothing says when the menu is
        /// actually on screen. Both the events that sound like it fire about ten seconds
        /// in - MainMenuList.Start builds the menu object, HudMoneyController.Start
        /// builds the HUD - while the legal notice and the logo still have twenty-five
        /// seconds to run, and an Enter sent then is swallowed by a splash screen. An
        /// Enter that lands on one of those skips it, and an Enter that lands on the menu
        /// starts the newest save, so pressing until something loads is both the simplest
        /// thing that works and the fastest way through the splash screens.</para>
        /// </remarks>
        private static ProbeEvent ContinueFromMenu(ProbeWatcher watcher, TimeSpan timeout)
        {
            return PressEnterUntil(
                watcher,
                e => e.Name == "save-applied",
                timeout,
                "a save starts loading",
                "still on a splash screen");
        }

        /// <summary>
        /// Presses Enter until the probe reports what is being waited for.
        /// </summary>
        /// <remarks>
        /// <para>Two places need this and neither can be timed. Nothing says when the
        /// main menu is actually on screen - both events that sound like it fire about
        /// ten seconds in, while the legal notice and the logo still have twenty-five
        /// seconds to run - and nothing says when a conversation has finished showing
        /// the lines that precede its first response menu.</para>
        ///
        /// <para>An Enter that lands on a splash screen skips it, one that lands on the
        /// menu starts the newest save, and one that lands on a line of dialogue advances
        /// it. So pressing until the awaited thing happens is both the simplest thing
        /// that works and the fastest way through.</para>
        ///
        /// <para>The last press can race the menu it was waiting for and pick an option.
        /// That is harmless here: the menu has already been reported by then, with every
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

            while (true)
            {
                if (!GameWindows.BringToFront(window.Handle))
                {
                    Console.Error.WriteLine(
                        "        could not bring the game to the front; is something "
                        + "stealing focus?");
                }

                GameSession.SendKey("Enter");

                try
                {
                    ProbeEvent found = watcher.WaitFor(matches, BetweenPresses, what);
                    Console.WriteLine(
                        $"        {what} after {clock.Elapsed.TotalSeconds:N0}s");
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

        private static string Trim(string? text)
        {
            if (text == null)
            {
                return "(no text)";
            }

            string oneLine = text.Replace("\r", " ").Replace("\n", " ");
            return oneLine.Length <= 90 ? oneLine : oneLine.Substring(0, 87) + "...";
        }

        private static void Close(Process? process)
        {
            foreach (Process running in Process.GetProcessesByName("disco"))
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
            // The profile is moved back next, and Windows will not move a folder the
            // game still has open.
            Thread.Sleep(2000);
        }

        private static void Log(string message)
        {
            Console.WriteLine($"        {message}");
        }
    }
}
