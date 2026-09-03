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
    /// <para>One launch for the whole run. Every suite's global state is staged beside
    /// the saves, then the probe asks the mod to reload that fixture and apply the
    /// suite's look-ahead settings before any scenario in it runs.</para>
    /// </remarks>
    public static class LookAheadRun
    {
        /// <summary>
        /// Limits to run every suite at, whatever it declares; null to respect it.
        /// </summary>
        /// <remarks>
        /// Set once per run from the command line, for asking how cost behaves as a limit
        /// moves. A suite whose markers depend on a crawl completing may fail under one,
        /// which is why the run says loudly that it is in force.
        /// </remarks>
        private static int? _stateBudgetOverride;
        private static int? _timeBudgetOverride;

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

        /// <summary>
        /// How many times a scenario will ask for its conversation before giving up.
        /// </summary>
        /// <remarks>
        /// A conversation asked for too early finds its first node gated and ends at
        /// once, which from outside is indistinguishable from one that never started.
        /// Retrying costs a few seconds and rescues the run; not retrying costs the
        /// whole suite, because the failure lands mid-run with the profile staged.
        /// Three rather than more: if two settles have not made the world ready, the
        /// cause is not a race and a fourth ask will not find it either.
        /// </remarks>
        private const int OpenAttempts = 3;

        /// <summary>
        /// How long to let the world settle before asking for a conversation again.
        /// </summary>
        /// <remarks>
        /// The gap being closed is between <c>load-finished</c> - the game's own loading
        /// flag falling - and the loaded save's world state reaching Lua, which is what
        /// the first node's condition is evaluated against. That is a short gap; this is
        /// several times its size, because the cost of waiting too long is seconds and
        /// the cost of waiting too little is the retry failing the same way.
        /// </remarks>
        private static readonly TimeSpan BetweenOpenAttempts = TimeSpan.FromSeconds(5);

        /// <summary>
        /// How long to wait for the probe to say whether <c>start-conversation</c> took.
        /// </summary>
        /// <remarks>
        /// The probe answers on its next poll, ten frames away, so this is generous by a
        /// wide margin. It is a guard against the command never being picked up at all,
        /// not a real wait.
        /// </remarks>
        private static readonly TimeSpan StartAcknowledgement = TimeSpan.FromSeconds(30);

        /// <summary>Runs the named suites in one game process.</summary>
        /// <param name="game">Path to disco.exe.</param>
        /// <param name="scenarioRoot">Where the built scenarios are.</param>
        /// <param name="settingsFile">The test settings to stage.</param>
        /// <param name="artifacts">Where packed saves go.</param>
        /// <param name="timeout">How long any single wait may take.</param>
        /// <param name="keepOpen">Leave the last game running.</param>
        /// <param name="suiteNames">Suite names, or an empty list for every suite.</param>
        /// <param name="stateBudget">
        /// A state budget to force on every suite, or null to respect what they declare.
        /// </param>
        /// <param name="timeBudgetMs">
        /// A time budget to force on every suite, or null to respect what they declare.
        /// </param>
        /// <returns>0 when every check passed.</returns>
        public static int Run(
            string game,
            string scenarioRoot,
            string settingsFile,
            string artifacts,
            TimeSpan timeout,
            bool keepOpen,
            IReadOnlyList<string> suiteNames,
            int? stateBudget = null,
            int? timeBudgetMs = null)
        {
            IReadOnlyList<LookAheadSuite> suites = LookAheadSuites.SelectMany(suiteNames);
            var report = new Report();
            _stateBudgetOverride = stateBudget;
            _timeBudgetOverride = timeBudgetMs;
            if (stateBudget != null || timeBudgetMs != null)
            {
                Console.WriteLine(
                    "Overriding every suite's limits: "
                    + $"state budget {stateBudget?.ToString() ?? "as declared"}, "
                    + $"time budget {timeBudgetMs?.ToString() ?? "as declared"}ms. "
                    + "Marker expectations may no longer hold.");
            }

            RunSuites(
                suites, game, scenarioRoot, settingsFile, artifacts, timeout, keepOpen, report);

            Console.WriteLine();
            Console.WriteLine($"{report.Passed}/{report.Total} checks passed");
            foreach (string failure in report.Failures)
            {
                Console.WriteLine($"  FAILED: {failure}");
            }

            return report.Failures.Count == 0 ? 0 : 1;
        }

        /// <summary>
        /// The saves a run has to stage, in the order they must be packed.
        /// </summary>
        /// <remarks>
        /// <para>Oldest first, so that the FIRST scenario's save is packed last and is
        /// therefore the newest on disk. That matters because the first save of a run is
        /// loaded by pressing Continue at the main menu, which takes the newest one -
        /// loading from the menu through the probe dies in HudToggle.FixForDreamScene,
        /// whose HUD views are not built yet. Once a save is in and the HUD exists, the
        /// probe can load the rest by name and the order stops mattering.</para>
        ///
        /// <para>Duplicates are dropped by FIRST use, before the reversal, and that
        /// ordering is the whole of this. A save several suites share - afford-both is
        /// used by three - would otherwise be positioned by its LAST use, which is not
        /// where the run needs it: reversing after the deduplication left afford-both
        /// packed fifth of twelve rather than last, so Continue loaded a different save
        /// and the money scenario silently measured the wrong balance.</para>
        /// </remarks>
        /// <param name="suites">The suites about to run, in order.</param>
        /// <returns>Distinct save names, oldest to newest.</returns>
        public static IReadOnlyList<string> StagingOrder(IEnumerable<LookAheadSuite> suites)
        {
            if (suites == null)
            {
                throw new ArgumentNullException(nameof(suites));
            }

            return suites
                .SelectMany(suite => suite.Scenarios)
                .Select(scenario => scenario.SaveName)
                .Distinct(StringComparer.Ordinal)
                .Reverse()
                .ToArray();
        }

        private static void RunSuites(
            IReadOnlyList<LookAheadSuite> suites,
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
            var packed = new List<string>();
            var stagedNames = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (string saveName in StagingOrder(suites))
            {
                string expanded = Path.Combine(scenarioRoot, saveName + ".ntwtf");
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
                stagedNames[saveName] =
                    fileName.EndsWith(SaveExtension, StringComparison.OrdinalIgnoreCase)
                        ? fileName.Substring(0, fileName.Length - SaveExtension.Length)
                        : fileName;
            }

            Process? process = null;

            using StagedGame staged = StagedGame.Stage(
                "disco",
                settingsFile,
                packed,
                null,
                progress: message => Console.WriteLine($"staging:   {message}"));

            var stateFiles = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (LookAheadSuite suite in suites)
            {
                string source = Path.Combine(scenarioRoot, suite.GlobalStateFile);
                if (!File.Exists(source))
                {
                    throw new FileNotFoundException(
                        $"Suite '{suite.Name}' names a global state at {source}.", source);
                }

                string fileName = $"gct-look-ahead-{suite.Name}.json";
                File.Copy(source, Path.Combine(saveGames, fileName), overwrite: true);
                stateFiles[suite.Name] = fileName;
            }

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

                // Every wait from here on gives up the moment the game is gone. Closed by
                // hand, crashed, or killed, it will not report anything again, and the
                // profile is staged until this returns.
                Process? launched = process;
                watcher.AbandonIf(
                    () => launched != null && launched.HasExited,
                    "the game is no longer running");

                watcher.WaitForEvent("ready", timeout, Log);
                report.Check(true, "the probe loaded", $"reading {logPath}");

                // Checked, not assumed. Everything downstream is measured against a
                // window of a known size, and a run at the machine's own resolution would
                // still pass every marker check while testing something else.
                GameWindow window = GameSession.WaitForWindow("disco", timeout);
                report.Check(
                    window.Width == staged.Requested.Width
                        && window.Height == staged.Requested.Height,
                    $"the window is the requested {staged.Requested}",
                    $"got {window.Width}x{window.Height}");

                bool firstScenario = true;
                foreach (LookAheadSuite suite in suites)
                {
                    Console.WriteLine();
                    Console.WriteLine($"=== suite '{suite.Name}': {suite.What} ===");
                    watcher.Mark();
                    SendPrepareSuite(suite, saveGames, stateFiles[suite.Name]);
                    ProbeEvent prepared = watcher.WaitForEvent(
                        "look-ahead-suite-prepared", timeout, Log);
                    bool enabled = Setting(suite, "MarkLookAhead", true);
                    int stateBudget = Setting(suite, "LookAheadStateBudget", 200_000);
                    int timeBudgetMs = Setting(
                        suite, "LookAheadTimeBudgetMs", LookAheadSuites.TestTimeBudgetMs);
                    report.Check(
                        prepared.Text("file") == stateFiles[suite.Name]
                            && prepared.Boolean("enabled") == enabled
                            && prepared.Number("stateBudget") == stateBudget
                            && prepared.Number("timeBudgetMs") == timeBudgetMs,
                        $"{suite.Name}: its global state and settings were prepared",
                        $"the probe loaded {prepared.Text("file") ?? "nothing"} with enabled="
                            + $"{prepared.Boolean("enabled")?.ToString() ?? "missing"}, budget="
                            + $"{prepared.Number("stateBudget")?.ToString() ?? "missing"} and "
                            + $"{prepared.Number("timeBudgetMs")?.ToString() ?? "missing"}ms");
                    // Preparing flushes the preceding diagnostics writer. Clear after
                    // that flush so files from the prior suite cannot satisfy this one.
                    ClearArtefacts(suite, saveGames);

                    foreach (LookAheadScenario scenario in suite.Scenarios)
                    {
                        Console.WriteLine();
                        Console.WriteLine($"--- {scenario.SaveName}: {scenario.Why} ---");

                        watcher.Mark();
                        if (firstScenario)
                        {
                            StartTheNewestSave(watcher, timeout, report);
                            firstScenario = false;
                        }
                        else
                        {
                            ProbeCommand.SendLoadSave(saveGames, stagedNames[scenario.SaveName]);
                            watcher.WaitForEvent("save-applied", timeout, Log);
                        }

                        RunScenario(scenario, saveGames, watcher, timeout, report);
                    }

                    watcher.Mark();
                    ProbeCommand.SendFinishLookAheadSuite(saveGames);
                    watcher.WaitForEvent("look-ahead-suite-finished", timeout, Log);
                    CheckArtefacts(suite, saveGames, report);
                    CheckLog(suite, logPath, report);
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

        /// <summary>
        /// Gets the first save of a run in, by waiting for the main menu and pressing
        /// Continue.
        /// </summary>
        /// <remarks>
        /// <para>Continue rather than a named load, because loading from the menu through
        /// the probe dies in HudToggle.FixForDreamScene - the HUD views that path expects
        /// are not built yet - and Continue takes the newest save, which is what
        /// <see cref="StagingOrder"/> arranges. Once a save is in and the HUD exists the
        /// probe can load the rest by name.</para>
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
        private static void StartTheNewestSave(
            ProbeWatcher watcher, TimeSpan timeout, Report report)
        {
            StartupWatcher? startup = LoadStartupWatcher();
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
        private static StartupWatcher? LoadStartupWatcher()
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
                progress: Log);
        }

        private static void SendPrepareSuite(
            LookAheadSuite suite, string saveGames, string stateFile)
        {
            ProbeCommand.SendPrepareLookAheadSuite(
                saveGames,
                stateFile,
                Setting(suite, "MarkLookAhead", true),
                Setting(suite, "LookAheadStateBudget", 200_000),
                Setting(suite, "LookAheadTimeBudgetMs", LookAheadSuites.TestTimeBudgetMs),
                Setting(suite, "LogLookAheadBudgetExceeded", false),
                Setting(suite, "KeepLookAheadStates", false));
        }

        private static bool Setting(LookAheadSuite suite, string name, bool fallback) =>
            suite.PluginSettings.TryGetValue(name, out string? value)
                ? bool.Parse(value)
                : fallback;

        private static int Setting(LookAheadSuite suite, string name, int fallback)
        {
            int? forced = name switch
            {
                "LookAheadStateBudget" => _stateBudgetOverride,
                "LookAheadTimeBudgetMs" => _timeBudgetOverride,
                _ => null,
            };

            if (forced != null)
            {
                return forced.Value;
            }

            return suite.PluginSettings.TryGetValue(name, out string? value)
                ? int.Parse(value)
                : fallback;
        }

        private static void ClearArtefacts(LookAheadSuite suite, string saveGames)
        {
            foreach (SuiteArtefact artefact in suite.Artefacts)
            {
                string path = Path.Combine(saveGames, artefact.FileName);
                if (File.Exists(path))
                {
                    File.Delete(path);
                }
            }
        }

        /// <summary>
        /// Opens a scenario's conversation and returns the response menu it draws,
        /// asking again if the conversation did not take.
        /// </summary>
        /// <remarks>
        /// <para>Asking once is not enough. A conversation started before the loaded
        /// save's world state has reached Lua evaluates its first node's condition
        /// against the outgoing world, finds it false, and ends immediately - so the
        /// game is healthy, the id was right, and no menu is ever drawn. Waiting out the
        /// menu timeout reports that as a hang, which is both wrong and expensive: it
        /// lands mid-run with the player's profile staged.</para>
        ///
        /// <para>Two things are checked, because they fail differently. The probe says
        /// outright whether <c>StartConversation</c> left a conversation running, which
        /// catches the immediate fall-out; and a conversation that is running but never
        /// reaches a menu is caught by the attempt's own timeout. Both lead to the same
        /// remedy - settle, ask again - so both are retried the same way.</para>
        ///
        /// <para>Each attempt gets an equal share of the scenario's timeout, so retrying
        /// cannot make a genuinely stuck run take three times as long to report.</para>
        /// </remarks>
        /// <summary>The share of a scenario's timeout that one attempt at opening gets.</summary>
        /// <remarks>
        /// Split rather than repeated, so that adding retries cannot multiply how long a
        /// genuinely stuck scenario takes to report. The whole point is to rescue a run
        /// that would have failed, not to spend three times as long failing.
        /// </remarks>
        /// <param name="total">The scenario's whole timeout.</param>
        /// <returns>How long one attempt may wait for its menu.</returns>
        public static TimeSpan AttemptTimeout(TimeSpan total)
        {
            return TimeSpan.FromTicks(total.Ticks / OpenAttempts);
        }

        /// <summary>
        /// Whether an answer to <c>start-conversation</c> says the conversation ended as
        /// soon as it began.
        /// </summary>
        /// <remarks>
        /// Only an explicit false counts. A missing or non-Boolean <c>active</c> means
        /// the probe could not ask the dialogue system, not that the conversation failed,
        /// and treating "do not know" as "failed" would retry - and eventually fail -
        /// scenarios that were about to draw their menu perfectly well.
        /// </remarks>
        /// <param name="started">The probe's answer to the command.</param>
        /// <returns>True only when the probe said outright that nothing is running.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="started"/> is null.</exception>
        public static bool FellStraightOut(ProbeEvent started)
        {
            if (started == null)
            {
                throw new ArgumentNullException(nameof(started));
            }

            return started.Boolean("active") == false;
        }

        private static ProbeEvent OpenConversation(
            LookAheadScenario scenario,
            string saveGames,
            ProbeWatcher watcher,
            TimeSpan timeout)
        {
            string what = $"a response menu in conversation {scenario.ConversationId}";
            TimeSpan perAttempt = AttemptTimeout(timeout);

            for (int attempt = 1; ; attempt++)
            {
                bool last = attempt == OpenAttempts;
                if (attempt > 1)
                {
                    Console.WriteLine(
                        $"        asking for conversation {scenario.ConversationId} again "
                        + $"(attempt {attempt} of {OpenAttempts})");
                }

                ProbeCommand.SendStartConversation(saveGames, scenario.ConversationId);

                // Read before the menu wait, not after: WaitFor consumes as it scans, so
                // pressing Enter first would swallow the acknowledgement this turns on.
                ProbeEvent started = watcher.WaitFor(
                    e => e.Name == "command-finished"
                        && e.Text("command") == "start-conversation",
                    StartAcknowledgement,
                    $"an answer to start-conversation {scenario.ConversationId}",
                    Log);

                if (FellStraightOut(started))
                {
                    if (last)
                    {
                        throw new TimeoutException(
                            $"Asked for conversation {scenario.ConversationId} {OpenAttempts} "
                            + "times and it never stayed open. It ends as soon as it starts, "
                            + "so its first node's condition is false in this save.");
                    }

                    Console.WriteLine(
                        "        it started and ended at once; letting the world settle");
                    Thread.Sleep(BetweenOpenAttempts);
                    continue;
                }

                // The conversation opens on narration, not on a menu: StartConversation
                // puts the first line up and the game waits to be told to go on, exactly
                // as it would for a player. Enter advances it until the options appear.
                //
                // "complete" only. The game composes each menu twice - once per
                // response-UI path - so the recorder reports the first pass as superseded
                // when the second begins. Both carry the same options, but only the
                // completed one is tied to its conversation and its balance.
                try
                {
                    return PressEnterUntil(
                        watcher,
                        e => e.Name == "menu"
                            && e.Number("conversation") == scenario.ConversationId
                            && e.Text("state") == "complete",
                        perAttempt,
                        what,
                        "advancing dialogue");
                }
                catch (TimeoutException) when (!last)
                {
                    Console.WriteLine(
                        "        it is open but drew no menu; letting the world settle");
                    Thread.Sleep(BetweenOpenAttempts);
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

            ProbeEvent menu = OpenConversation(scenario, saveGames, watcher, timeout);

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

            if (scenario.Markers == MarkerPolicy.Ignored)
            {
                return;
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
