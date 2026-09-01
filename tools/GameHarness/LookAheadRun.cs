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
    /// <summary>
    /// Runs look-ahead suites against the game and reads the marker off the option text
    /// the game was about to draw.
    /// </summary>
    /// <remarks>
    /// <para>What this covers that no unit test can: the crawl runs over the real
    /// dialogue database, from the real world state, and the marker is read from the text
    /// the game composed. The search itself is covered by the LookAhead suite over graphs
    /// handed to it; this covers the wiring.</para>
    ///
    /// <para>One launch per suite, and every save in a suite loaded in place by the test
    /// probe. A cold start is about a minute, so relaunching per scenario would multiply
    /// the run for nothing - but a suite cannot span two global states or two mod
    /// configurations, because the mod reads the first once and BepInEx reads the second
    /// at chainload.</para>
    /// </remarks>
    public static class LookAheadRun
    {
        /// <summary>The colour meaning "leads somewhere no save has reached".</summary>
        public const string OrangeHtml = "#FF8C42";

        /// <summary>The colour meaning "leads somewhere this save has not".</summary>
        public const string RedHtml = "#C4453C";

        /// <summary>What a packed save archive is called.</summary>
        private const string SaveExtension = ".ntwtf.zip";

        /// <summary>
        /// How long to give one Enter before pressing it again. Short enough to walk
        /// through a splash screen or a run of dialogue briskly, long enough that a
        /// loading screen is not hammered.
        /// </summary>
        private static readonly TimeSpan BetweenPresses = TimeSpan.FromSeconds(2);

        /// <summary>Runs the named suites, one launch each.</summary>
        /// <param name="game">Path to disco.exe.</param>
        /// <param name="scenarioRoot">Where the built scenarios are.</param>
        /// <param name="settingsFile">The test settings to stage.</param>
        /// <param name="artifacts">Where packed saves go.</param>
        /// <param name="timeout">How long any single wait may take.</param>
        /// <param name="keepOpen">Leave the last game running.</param>
        /// <param name="suiteName">One suite's name, or null for every suite.</param>
        /// <returns>0 when every check passed.</returns>
        public static int Run(
            string game,
            string scenarioRoot,
            string settingsFile,
            string artifacts,
            TimeSpan timeout,
            bool keepOpen,
            string? suiteName = null)
        {
            IReadOnlyList<LookAheadSuite> suites = LookAheadSuites.Select(suiteName);
            var report = new Report();

            foreach (LookAheadSuite suite in suites)
            {
                Console.WriteLine();
                Console.WriteLine($"=== suite '{suite.Name}': {suite.What} ===");
                RunSuite(
                    suite, game, scenarioRoot, settingsFile, artifacts, timeout,
                    keepOpen && suite == suites[suites.Count - 1], report);
            }

            Console.WriteLine();
            Console.WriteLine($"{report.Passed}/{report.Total} checks passed");
            foreach (string failure in report.Failures)
            {
                Console.WriteLine($"  FAILED: {failure}");
            }

            return report.Failures.Count == 0 ? 0 : 1;
        }

        private static void RunSuite(
            LookAheadSuite suite,
            string game,
            string scenarioRoot,
            string settingsFile,
            string artifacts,
            TimeSpan timeout,
            bool keepOpen,
            Report report)
        {
            string logPath = Path.Combine(
                FilePaths.FolderOf(game, nameof(game)), "BepInEx", "LogOutput.log");
            string saveGames = GameProfile.SavesFolder;
            string globalState = Path.Combine(scenarioRoot, suite.GlobalStateFile);
            if (!File.Exists(globalState))
            {
                throw new FileNotFoundException(
                    $"Suite '{suite.Name}' names a global state at {globalState}.", globalState);
            }

            // Packed in REVERSE order so the first scenario's archive is the newest. The
            // first save is loaded by pressing Continue at the main menu, which takes the
            // newest one, because loading from the menu through the probe dies in
            // HudToggle.FixForDreamScene - the HUD views that path expects are not built
            // yet. Once a save is in and the HUD exists, the probe can load the rest.
            var packed = new List<string>();
            var stagedNames = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (LookAheadScenario scenario in Enumerable.Reverse(suite.Scenarios))
            {
                string expanded = Path.Combine(scenarioRoot, scenario.SaveName + ".ntwtf");
                if (!Directory.Exists(expanded))
                {
                    throw new DirectoryNotFoundException($"No scenario save at {expanded}.");
                }

                string archive = Program.PackSave(expanded, artifacts);
                packed.Add(archive);

                // The packer stamps the time into the archive's name and the game keys a
                // save by exactly that, so the load command has to carry the staged name
                // and not the scenario's. Getting this wrong failed silently.
                string fileName = Path.GetFileName(archive);
                stagedNames[scenario.SaveName] =
                    fileName.EndsWith(SaveExtension, StringComparison.OrdinalIgnoreCase)
                        ? fileName.Substring(0, fileName.Length - SaveExtension.Length)
                        : fileName;
            }

            Process? process = null;

            using StagedGame staged = StagedGame.Stage(
                "disco",
                settingsFile,
                packed,
                globalState,
                progress: message => Console.WriteLine($"staging:   {message}"));

            using StagedPluginConfig? config = suite.PluginSettings.Count == 0
                ? null
                : StagedPluginConfig.Apply(
                    game,
                    suite.PluginSettings,
                    message => Console.WriteLine($"mod cfg:   {message}"));

            using ProbeDeployment probe = ProbeDeployment.Deploy(
                game,
                GameInstall.FindProbeAssembly(),
                message => Console.WriteLine($"probe:     {message}"));

            try
            {
                // Before launching, not after. BepInEx truncates its log when it starts,
                // but the harness begins reading the instant the process exists, and in
                // that gap it would find the PREVIOUS run's events.
                if (File.Exists(logPath))
                {
                    File.Delete(logPath);
                }

                Console.WriteLine("launching...");
                process = Process.Start(new ProcessStartInfo(game) { UseShellExecute = false });

                var watcher = new ProbeWatcher(logPath);
                watcher.WaitForEvent("ready", timeout, Log);
                report.Check(true, $"{suite.Name}: the probe loaded", $"reading {logPath}");

                // Checked, not assumed. Everything downstream is measured against a
                // window of a known size, and a run at the machine's own resolution would
                // still pass every marker check while testing something else.
                GameWindow window = GameSession.WaitForWindow("disco", timeout);
                report.Check(
                    window.Width == staged.Requested.Width
                        && window.Height == staged.Requested.Height,
                    $"{suite.Name}: the window is the requested {staged.Requested}",
                    $"got {window.Width}x{window.Height}");

                for (int i = 0; i < suite.Scenarios.Count; i++)
                {
                    LookAheadScenario scenario = suite.Scenarios[i];
                    Console.WriteLine();
                    Console.WriteLine($"--- {scenario.SaveName}: {scenario.Why} ---");

                    watcher.Mark();
                    if (i == 0)
                    {
                        PressEnterUntil(
                            watcher,
                            e => e.Name == "save-applied",
                            timeout,
                            "a save starts loading",
                            "still on a splash screen");
                    }
                    else
                    {
                        ProbeCommand.SendLoadSave(saveGames, stagedNames[scenario.SaveName]);
                        watcher.WaitForEvent("save-applied", timeout, Log);
                    }

                    RunScenario(scenario, saveGames, watcher, timeout, report);
                }

                // Closed here, not in the finally, and asked rather than killed: the
                // mod writes its look-ahead statistics and flushes its global state from
                // Application.quitting, so a killed game leaves neither. The statistics
                // are otherwise written once every two hundred crawls, which is far more
                // than a suite produces.
                if (!keepOpen)
                {
                    Console.WriteLine();
                    Quit(saveGames, process);
                }

                // Before the finally puts the profile back, which is when these exist.
                CheckArtefacts(suite, saveGames, report);
                CheckLog(suite, logPath, report);
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
                    // Anything still alive after the polite close, and anything alive
                    // because the run threw before reaching it.
                    Close(process);
                    staged.Restore();
                }
            }
        }

        private static void RunScenario(
            LookAheadScenario scenario,
            string saveGames,
            ProbeWatcher watcher,
            TimeSpan timeout,
            Report report)
        {
            watcher.WaitForEvent("load-finished", timeout, Log);

            ProbeCommand.SendStartConversation(saveGames, scenario.ConversationId);

            // The conversation opens on narration, not on a menu: StartConversation puts
            // the first line up and the game waits to be told to go on, exactly as it
            // would for a player. Enter advances it until the options appear.
            //
            // "complete" only. The game composes each menu twice - once per response-UI
            // path - so the recorder reports the first pass as superseded when the second
            // begins. Both carry the same options, but only the completed one is tied to
            // its conversation and its balance.
            ProbeEvent menu = PressEnterUntil(
                watcher,
                e => e.Name == "menu"
                    && e.Number("conversation") == scenario.ConversationId
                    && e.Text("state") == "complete",
                timeout,
                $"a response menu in conversation {scenario.ConversationId}",
                "advancing dialogue");

            // Taken from the menu rather than from load-finished, which is emitted when
            // the game's IsLoading flag falls - a poll boundary earlier than the loaded
            // save's world state reaching Lua. The menu's reading is the one the
            // look-ahead actually crawled from.
            if (scenario.Money is int money)
            {
                report.Check(
                    menu.Number("money") == money,
                    $"{scenario.SaveName}: the look-ahead crawled from {money} centimes",
                    $"the probe reports {menu.Number("money")?.ToString() ?? "nothing"}");
            }

            ProbeOption[] options = menu.Options();
            report.Check(
                options.Length > 0,
                $"{scenario.SaveName}: the response menu was drawn",
                $"{options.Length} option(s)");

            foreach (ProbeOption option in options)
            {
                Console.WriteLine(
                    $"        [{Describe(MarkerOn(option)),-6}] "
                    + $"{option.ConversationId}:{option.EntryId} {Trim(option.Text)}");
            }

            foreach (OptionExpectation expected in scenario.Options)
            {
                ProbeOption? option = options.FirstOrDefault(o => o.EntryId == expected.EntryId);
                if (option == null)
                {
                    report.Check(
                        false,
                        $"{scenario.SaveName}: entry {expected.EntryId} is offered",
                        $"the menu offered {string.Join(", ", options.Select(o => o.EntryId))}");
                    continue;
                }

                Marker actual = MarkerOn(option);
                report.Check(
                    actual == expected.Marker,
                    $"{scenario.SaveName}: entry {expected.EntryId} is "
                        + $"{Describe(expected.Marker)}",
                    $"it is {Describe(actual)} - {expected.Why}");
            }

            // An option the scenario says nothing about must not be marked either.
            // Without this a scan that marked everything would satisfy every expectation
            // a scenario happened to name.
            foreach (ProbeOption option in options)
            {
                if (scenario.Names(option.EntryId))
                {
                    continue;
                }

                report.Check(
                    MarkerOn(option) == Marker.None,
                    $"{scenario.SaveName}: entry {option.EntryId}, which the scenario does "
                        + "not name, is unmarked",
                    $"it is {Describe(MarkerOn(option))}");
            }
        }

        /// <summary>Checks what a suite says the mod should have written to the log.</summary>
        private static void CheckLog(LookAheadSuite suite, string logPath, Report report)
        {
            if (suite.LogExpectations.Count == 0)
            {
                return;
            }

            string log = File.Exists(logPath) ? FilePaths.ReadShared(logPath) : string.Empty;
            foreach (LogExpectation expected in suite.LogExpectations)
            {
                bool present = log.Contains(expected.Substring, StringComparison.Ordinal);
                report.Check(
                    present == expected.ShouldAppear,
                    $"{suite.Name}: {expected.What}",
                    present
                        ? $"the log says '{expected.Substring}'"
                        : $"the log does not say '{expected.Substring}'");
            }
        }

        /// <summary>Checks the files a suite says the run should leave behind.</summary>
        /// <remarks>
        /// Run inside the try, not the finally: the profile is staged, so the mod's
        /// diagnostics are in the staged SaveGames folder and go away with it.
        /// </remarks>
        private static void CheckArtefacts(
            LookAheadSuite suite, string saveGames, Report report)
        {
            foreach (SuiteArtefact artefact in suite.Artefacts)
            {
                string path = Path.Combine(saveGames, artefact.FileName);
                string? contents = File.Exists(path) ? FilePaths.ReadShared(path) : null;

                string? complaint = artefact.Check(contents);
                report.Check(
                    complaint is null,
                    $"{suite.Name}: {artefact.What}",
                    complaint
                        ?? (contents is null
                            ? $"{artefact.FileName} was never written, which is right here"
                            : $"{artefact.FileName} says so"));
            }
        }

        private static Marker MarkerOn(ProbeOption option) =>
            option.HasMarker(OrangeHtml) ? Marker.Orange
            : option.HasMarker(RedHtml) ? Marker.Red
            : Marker.None;

        private static string Describe(Marker marker) => marker switch
        {
            Marker.Orange => "orange",
            Marker.Red => "red",
            _ => "plain",
        };

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

        private static string Trim(string? text)
        {
            if (text == null)
            {
                return "(no text)";
            }

            string oneLine = text.Replace("\r", " ").Replace("\n", " ");
            return oneLine.Length <= 80 ? oneLine : oneLine.Substring(0, 77) + "...";
        }

        /// <summary>
        /// Asks the game to close itself, and waits for it to go.
        /// </summary>
        /// <remarks>
        /// Killing it is the fallback, not the plan. A killed game runs neither
        /// Application.quitting nor AppDomain.ProcessExit, which is where the mod writes
        /// everything it has been holding - so a run that killed would be measuring a
        /// shutdown no player ever performs.
        /// </remarks>
        private static void Quit(string saveGames, Process? process)
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
                if (Process.GetProcessesByName("disco").Length == 0)
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

        /// <summary>How long to wait for the game to close itself before killing it.</summary>
        private static readonly TimeSpan QuitDeadline = TimeSpan.FromSeconds(30);

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
            // The profile is moved back next, and Windows will not move a folder the game
            // still has open.
            Thread.Sleep(2000);
        }

        private static void Log(string message)
        {
            Console.WriteLine($"        {message}");
        }

        /// <summary>Counts what passed and what did not, across every suite.</summary>
        private sealed class Report
        {
            private readonly List<string> _failures = new List<string>();

            /// <summary>How many checks have run.</summary>
            public int Total { get; private set; }

            /// <summary>How many of them passed.</summary>
            public int Passed => Total - _failures.Count;

            /// <summary>What failed, in the order it failed.</summary>
            public IReadOnlyList<string> Failures => _failures;

            /// <summary>Records one check.</summary>
            public void Check(bool ok, string label, string detail)
            {
                Total++;
                Console.WriteLine($"  {(ok ? "PASS" : "FAIL")}  {label}");
                if (detail.Length > 0)
                {
                    Console.WriteLine($"        {detail}");
                }

                if (!ok)
                {
                    _failures.Add(label);
                }
            }
        }
    }
}
