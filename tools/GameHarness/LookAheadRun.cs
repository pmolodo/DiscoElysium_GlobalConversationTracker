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
        private static int? _memoryBudgetOverride;
        private static int? _timeBudgetOverride;

        /// <summary>The colour meaning "leads somewhere no save has reached".</summary>
        public const string OrangeHtml = "#FF8C42";

        /// <summary>The colour meaning "leads somewhere this save has not".</summary>
        public const string RedHtml = "#C4453C";

        /// <summary>The colour meaning "the search did not finish".</summary>
        /// <remarks>
        /// Must match <c>ResponseLookAheadPatch.DefaultUncertainColorHtml</c>. The plugin
        /// and this cannot share a constant - one is compiled against the game's runtime -
        /// so the suites are what check they agree.
        /// </remarks>
        public const string UncertainHtml = "#7A7A7A";

        /// <summary>The colour meaning "this save has already read it".</summary>
        /// <remarks>
        /// A dark red, and the Pass / Fail line's alone - an option never needs it, since
        /// the game draws a spent option in its own way. Must match
        /// <c>ResponseLookAheadPatch.DefaultSeenColorHtml</c>, which for the same reason as
        /// the colour above cannot be shared with this.
        /// </remarks>
        public const string SeenHtml = "#7C2F2A";

        /// <summary>What a packed save archive is called.</summary>
        private const string SaveExtension = ".ntwtf.zip";

        /// <summary>
        /// How long to give one Enter before pressing it again. Short enough to walk
        /// through a splash screen or a run of dialogue briskly, long enough that a
        /// loading screen is not hammered.
        /// </summary>
        private static readonly TimeSpan BetweenPresses = TimeSpan.FromSeconds(2);

        /// <summary>How long to spend trying to wake the display before giving up.</summary>
        /// <remarks>
        /// Short. A display that is going to come back does so in a second or two; one that
        /// does not is a locked session, and no amount of waiting changes that.
        /// </remarks>
        private static readonly TimeSpan DisplayWakeTimeout = TimeSpan.FromSeconds(10);

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
        /// <param name="memoryBudgetMb">
        /// A memory budget to force on every suite, or null to respect what they
        /// declare.
        /// </param>
        /// <param name="timeBudgetMs">
        /// A time budget to force on every suite, or null to respect what they declare.
        /// </param>
        /// <param name="scenarioNames">
        /// Scenarios to keep, as save names or save:conversation pairs; empty for all.
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
            int? memoryBudgetMb = null,
            int? timeBudgetMs = null,
            IReadOnlyList<string>? scenarioNames = null)
        {
            // FILTERED BEFORE ANYTHING ELSE READS THE LIST, and StagingOrder above all: it
            // packs the saves so that the FIRST scenario of the run is the newest on disk,
            // which is the one Continue loads at the main menu. Filtering afterwards would
            // leave the run loading a save no remaining scenario asked for.
            IReadOnlyList<LookAheadSuite> suites = LookAheadSuites.Only(
                LookAheadSuites.SelectMany(suiteNames),
                scenarioNames ?? Array.Empty<string>());
            var report = new Report();
            _memoryBudgetOverride = memoryBudgetMb;
            _timeBudgetOverride = timeBudgetMs;
            if (memoryBudgetMb != null || timeBudgetMs != null)
            {
                Console.WriteLine(
                    "Overriding every suite's limits: "
                    + $"memory budget {memoryBudgetMb?.ToString() ?? "as declared"}MB, "
                    + $"time budget {timeBudgetMs?.ToString() ?? "as declared"}ms. "
                    + "Marker expectations may no longer hold.");
            }

            // THE ENGINE THIS RUN WOULD TEST, before a minute is spent testing it. A
            // library built from other sources passes or fails on behalf of code that is
            // not here, and there is no way to tell from the result - see de-8hh2.8, where
            // a run reported 66 of 66 against an engine from the previous day. Refused
            // rather than warned: the whole value of an in-game suite is saying what the
            // current code does in the game, and a run that cannot say that is not worth
            // the display it takes over.
            NativeEngineStamp.Result stamp = NativeEngineStamp.Check(
                ProbeDeployment.PluginsFolder(game) + Path.DirectorySeparatorChar
                    + "GlobalConversationTracker");
            Console.WriteLine($"engine:    {stamp.What}");
            if (stamp.Freshness == NativeEngineStamp.Freshness.Stale)
            {
                Console.Error.WriteLine();
                Console.Error.WriteLine("FAILED: " + stamp.What);
                return 1;
            }

            // Every check this run makes is a screenshot, and a screenshot of a sleeping
            // display is blank - as is the window it wants in front. Woken FIRST and then
            // held: keeping a display awake does nothing for one that is already off, and a
            // run started on an idle machine meets exactly that. Checked rather than
            // assumed, and reported here rather than five minutes later as a timeout on the
            // main-menu match.
            using DisplayAwake awake = DisplayAwake.Keep();
            if (!DisplayAwake.WakeAndCheck(
                message => Console.WriteLine($"display:   {message}"),
                DisplayWakeTimeout))
            {
                Console.Error.WriteLine();
                Console.Error.WriteLine(
                    "FAILED: the screen cannot be read, so nothing this run checks can be "
                    + "checked. Unlock the machine and run it again.");
                return 1;
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
            // A suite that kills the look-ahead engine leaves the launch without one for a
            // while, so it goes last whatever order it was asked for in. Said out loud,
            // because the run is about to report its suites in an order nobody typed.
            IReadOnlyList<LookAheadSuite> asked = suites;
            suites = LookAheadSuites.InRunOrder(suites);
            if (!suites.SequenceEqual(asked))
            {
                Console.WriteLine(
                    "order:     "
                    + string.Join(", ", suites.Where(suite => suite.KillsTheEngine)
                        .Select(suite => $"'{suite.Name}'"))
                    + " moved last: it kills the look-ahead engine, and though the next "
                    + "suite's prepare starts a fresh one, that engine comes up behind the "
                    + "first menus that suite draws");
            }

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

            // THE COLOURS A RUN ASSERTS ARE THE COLOURS IT STAGES. Both of these are
            // player settings, read from the config file at chainload, so until this was
            // here every colour check in every suite was really a check on whatever the
            // config file on THIS machine happened to say. Two ways that lied: a player
            // who had changed a colour failed suites that were passing, and - what
            // actually happened - a changed DEFAULT did not reach a config file BepInEx
            // had already written, so the run went on asserting the old value and failed
            // against a mod that was behaving exactly as asked. The player's file is put
            // back when the run ends.
            using StagedPluginConfig colours = StagedPluginConfig.Apply(
                game,
                new Dictionary<string, string>(StringComparer.Ordinal)
                {
                    ["NovelOptionColor"] = OrangeHtml,
                    ["UncertainLookAheadColor"] = UncertainHtml,
                },
                message => Console.WriteLine($"staging:   {message}"));

            var stateFiles = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (LookAheadSuite suite in suites)
            {
                // RESOLVED RATHER THAN COPIED, because a fixture may be written as a diff of
                // another one. What lands in the profile is a whole state either way, which
                // is the only shape the mod reads - see GlobalStateFixture.
                string state = GlobalStateFixture.Resolve(scenarioRoot, suite.GlobalStateFile);

                string fileName = $"gct-look-ahead-{suite.Name}.json";
                File.WriteAllText(Path.Combine(saveGames, fileName), state);
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
                //
                // AND SAYS HOW IT ENDED, asked at the moment it is noticed. An ordinary
                // close and a crash are opposite diagnoses - one means something asked
                // the game to go, the other means the game's own code took it down and
                // the Player.log is the place to look - and the exit code is the only
                // thing that tells them apart (de-wncd.4.1).
                Process? launched = process;
                watcher.AbandonIf(
                    () => launched != null && launched.HasExited,
                    () => $"the game is no longer running, {GameExit.Describe(launched)}");

                watcher.WaitForEvent("ready", timeout, Log);
                report.Check(true, "the probe loaded", $"reading {logPath}");

                // Checked, not assumed. Everything downstream is measured against a
                // window of a known size, and a run at the machine's own resolution would
                // still pass every marker check while testing something else.
                GameWindow window = GameSession.WaitForWindow("disco", timeout);
                bool rightSize = window.Width == staged.Requested.Width
                    && window.Height == staged.Requested.Height;
                report.Check(
                    rightSize,
                    $"the window is the requested {staged.Requested}",
                    $"got {window.Width}x{window.Height}");

                // AND STOP, because the run is already known to be unmeasurable and every
                // check after this point would be reporting the window rather than the mod.
                //
                // It used to record the failure above and carry on, and what came next was
                // the main-menu wait - which cannot succeed, because the screen references
                // it compares against were all captured at the requested size. That is five
                // minutes of retrying followed by a second, vaguer failure, and it cost two
                // real runs on 2026-09-06: a portrait display at 1080x720 (de-qslk) and a
                // 637x357 window when the display was reconfigured mid-launch.
                //
                // THROWN RATHER THAN RETURNED, so the restore in the finally still runs.
                // The second of those runs had to be stopped by hand, which skips it and
                // leaves the player's profile and PlayerPrefs staged.
                //
                // The diagnosis is spelled out because it is always the same one: nothing
                // about the mod decides this, and the thing to go and look at is the
                // display rather than the run.
                if (!rightSize)
                {
                    throw new InvalidOperationException(
                        $"The game opened at {window.Width}x{window.Height}, not the "
                        + $"{staged.Requested.Width}x{staged.Requested.Height} asked for. "
                        + "Every screen reference this run compares against was captured at "
                        + "the requested size, so nothing downstream can pass and waiting "
                        + "for the main menu would only spend five minutes proving it. "
                        + "Check the display's orientation, resolution and scaling, and run "
                        + "again.");
                }

                bool firstScenario = true;
                // Once per run, not once per scenario. The comparison opens the shipped
                // index - tens of megabytes - and what it checks is whether the two worlds
                // read the game the same way, which does not become a different question
                // for the second save.
                bool snapshotCompared = false;
                foreach (LookAheadSuite suite in suites)
                {
                    Console.WriteLine();
                    Console.WriteLine($"=== suite '{suite.Name}': {suite.What} ===");
                    if (suite.Filtered)
                    {
                        // Said out loud because the suite's name no longer describes what
                        // is being checked: some of its scenarios are not running, and its
                        // artefact and log expectations - which are claims about the whole
                        // suite - are not being made at all.
                        Console.WriteLine(
                            $"        (only {suite.Scenarios.Count} scenario(s) of this "
                            + "suite, and none of its whole-suite checks)");
                    }
                    watcher.Mark();
                    SendPrepareSuite(suite, saveGames, stateFiles[suite.Name]);
                    ProbeEvent prepared = watcher.WaitForEvent(
                        "look-ahead-suite-prepared", timeout, Log);
                    bool enabled = Setting(suite, "MarkLookAhead", true);
                    int memoryBudgetMb = Setting(
                        suite, "LookAheadMemoryBudgetMb", LookAheadSuites.TestMemoryBudgetMb);
                    int timeBudgetMs = Setting(
                        suite, "LookAheadTimeBudgetMs", LookAheadSuites.TestTimeBudgetMs);
                    int stateBudget = Setting(
                        suite, LookAheadSuites.TestStateBudgetSetting, 0);
                    report.Check(
                        prepared.Text("file") == stateFiles[suite.Name]
                            && prepared.Boolean("enabled") == enabled
                            && prepared.Number("memoryBudgetMb") == memoryBudgetMb
                            && prepared.Number("timeBudgetMs") == timeBudgetMs
                            && prepared.Number("stateBudget") == stateBudget,
                        $"{suite.Name}: its global state and settings were prepared",
                        $"the probe loaded {prepared.Text("file") ?? "nothing"} with enabled="
                            + $"{prepared.Boolean("enabled")?.ToString() ?? "missing"}, memory="
                            + $"{prepared.Number("memoryBudgetMb")?.ToString() ?? "missing"}MB, "
                            + $"{prepared.Number("timeBudgetMs")?.ToString() ?? "missing"}ms and "
                            + $"states={prepared.Number("stateBudget")?.ToString() ?? "missing"}");
                    // Preparing flushes the preceding diagnostics writer. Clear after
                    // that flush so files from the prior suite cannot satisfy this one.
                    ClearArtefacts(suite, saveGames);

                    // Where the log had got to, so this suite's expectations are read
                    // against what THIS suite wrote. The game is one process for the
                    // whole run and every suite appends to the same file, so a whole-file
                    // read lets one suite answer another's question - a suite asserting a
                    // line is absent fails on the earlier suite that legitimately wrote
                    // it, and a count adds up every suite's occurrences. Taken after the
                    // prepare, since preparing flushes the preceding diagnostics writer.
                    int logWrittenBefore = LogLength(logPath);

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

                        RunScenario(
                            scenario, saveGames, watcher, timeout, report, window,
                            artifacts, suite.Name);

                        if (!snapshotCompared)
                        {
                            CheckSnapshot(
                                scenario.ConversationId, saveGames, watcher, timeout,
                                logPath, report);
                            snapshotCompared = true;
                        }
                    }

                    watcher.Mark();
                    ProbeCommand.SendFinishLookAheadSuite(saveGames);
                    watcher.WaitForEvent("look-ahead-suite-finished", timeout, Log);
                    CheckArtefacts(suite, saveGames, report);
                    CheckLog(suite, logPath, logWrittenBefore, report);
                    KeepCapturedRequests(saveGames, artifacts);
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
            catch (ProbePendingException pending)
            {
                // ONE CATCH RATHER THAN THIRTEEN. Every command the run sends can be
                // refused this way, and the refusal is written by ProbeCommand, which
                // knows nothing about a process: it can only say the game MAY not be
                // running. This is the one place that launched it and can say whether it
                // is, and how it ended - so the message is completed here instead of at
                // each send (de-wncd.4.1).
                throw new ProbePendingException(
                    $"{pending.Message} As for the game this run launched, "
                    + $"{GameExit.Describe(process)}.",
                    pending);
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
                // Zero unless a suite asks to be starved: this is the test-only knob, and
                // no player setting corresponds to it.
                Setting(suite, LookAheadSuites.TestStateBudgetSetting, 0),
                Setting(suite, "LookAheadTimeBudgetMs", LookAheadSuites.TestTimeBudgetMs),
                Setting(
                    suite, "LookAheadMenuTimeBudgetMs", LookAheadSuites.TestMenuTimeBudgetMs),
                Setting(suite, "LookAheadMemoryBudgetMb", LookAheadSuites.TestMemoryBudgetMb),
                Setting(suite, "LogLookAheadBudgetExceeded", false),
                Setting(suite, "KeepLookAheadStates", false),
                // Negative unless a suite asks the mod not to replace a killed engine.
                // Test-only, like the state budget above, and no player setting
                // corresponds to it either.
                Setting(suite, LookAheadSuites.TestRecoveryLimitSetting, -1),
                // Off unless a suite is comparing the world it was answered from against
                // the one an offline run assembles - see de-v702.
                Setting(suite, "KeepLookAheadRequests", false));
        }

        private static bool Setting(LookAheadSuite suite, string name, bool fallback) =>
            suite.PluginSettings.TryGetValue(name, out string? value)
                ? bool.Parse(value)
                : fallback;

        private static int Setting(LookAheadSuite suite, string name, int fallback)
        {
            int? forced = name switch
            {
                "LookAheadMemoryBudgetMb" => _memoryBudgetOverride,
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

        /// <summary>Takes copies of any captured requests before the profile is put back.</summary>
        /// <remarks>
        /// THE PROFILE IS RESTORED WHEN THE RUN ENDS, which is right and which would take
        /// these with it. What they are for outlives the run: the world the game answered a
        /// group from, to be compared against the one an offline run assembles from the
        /// same save. Nothing is written unless a suite asked for the capture.
        /// </remarks>
        /// <param name="saveGames">The profile's SaveGames folder.</param>
        /// <param name="artifacts">Where the run's own output goes.</param>
        private static void KeepCapturedRequests(string saveGames, string artifacts)
        {
            string[] captured = Directory.GetFiles(saveGames, "look-ahead-request-*.json");
            if (captured.Length == 0)
            {
                return;
            }

            string kept = Path.Combine(artifacts, "requests");
            Directory.CreateDirectory(kept);
            foreach (string path in captured)
            {
                File.Copy(path, Path.Combine(kept, Path.GetFileName(path)), overwrite: true);
            }

            Console.WriteLine(
                $"        kept {captured.Length} captured request(s) in {kept}");
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

        /// <summary>Kills the mod's look-ahead engine and says which process went.</summary>
        /// <remarks>
        /// A process id of zero is not a quiet no-op to be passed over: it means the mod had
        /// no engine to kill, so every scenario after this would pass for the wrong reason -
        /// options drawn without markers because there was never an engine, rather than
        /// because one died. Loud, and it ends the run.
        /// </remarks>
        /// <summary>What the last kill killed, for the wait that comes after the menu.</summary>
        /// <remarks>
        /// A field rather than an argument because the kill happens in
        /// <see cref="OpenConversation"/> - before the conversation - and the wait happens
        /// in <see cref="RunScenario"/> after its menu has been read, with the scenario's
        /// own checks in between. The run is single-threaded and does one scenario at a
        /// time, so there is only ever one of these to remember.
        /// </remarks>
        private static int _killedEngine;

        private static int KillTheEngine(
            string saveGames, ProbeWatcher watcher, TimeSpan timeout)
        {
            // ASKED FOR RATHER THAN ASSUMED, since de-pszk. A suite prepare now revives an
            // engine its predecessor killed, and that engine comes up BEHIND the prepare -
            // a process launch plus an index read - so a kill sent the moment the save is
            // in can find nothing to kill for no better reason than having asked too early.
            // Waiting turns that race into a wait and leaves the throw below for the case
            // that is genuinely wrong. It also promotes a finished warm-up: the mod answers
            // this question through the same call that adopts a new engine, which is what
            // gives the kill something to find.
            int waited = WaitForAnEngine(saveGames, watcher, timeout, excluded: 0);
            if (waited == 0)
            {
                throw new InvalidOperationException(
                    $"No look-ahead engine came up within {EngineWait.TotalSeconds:0} "
                    + "seconds of this suite being prepared, so there is nothing to kill "
                    + "and nothing after this would mean what it claims. Check the run's "
                    + "log for a shutdown notice or a failed engine start.");
            }

            Console.WriteLine("        killing the look-ahead engine on purpose");
            ProbeCommand.SendKillLookAheadEngine(saveGames);
            ProbeEvent killed = watcher.WaitForEvent(
                "look-ahead-engine-killed", timeout, Log);

            int process = killed.Number("process") ?? 0;
            if (process == 0)
            {
                throw new InvalidOperationException(
                    "The mod had no look-ahead engine to kill, so nothing after this would "
                    + "mean what it claims: options drawn without markers because there "
                    + "was never an engine look exactly like ones drawn because it died.");
            }

            Console.WriteLine($"        the engine was process {process}, and is gone");
            _killedEngine = process;
            return process;
        }

        /// <summary>How long to wait for an engine to come up before giving up on it.</summary>
        /// <remarks>
        /// An engine costs a process launch plus a 173-244 ms index read
        /// (measurements/repeat_question.rs), so this is roughly twenty times what it
        /// should need. It is a FAILURE bound, not a guess at the duration - the poll below
        /// returns the moment an id appears.
        /// </remarks>
        private static readonly TimeSpan EngineWait = TimeSpan.FromSeconds(20);

        /// <summary>How often to ask whether an engine has arrived.</summary>
        private static readonly TimeSpan EnginePoll = TimeSpan.FromMilliseconds(250);

        /// <summary>
        /// Asks the mod for its engine's process id until it names one that is not
        /// <paramref name="excluded"/>, or until <see cref="EngineWait"/> is spent.
        /// </summary>
        /// <remarks>
        /// <para>POLLED RATHER THAN SLEPT: an engine comes up fast enough that any fixed
        /// wait is either flaky or wasteful, and the mod can say which process it is
        /// holding, so the harness asks instead of guessing.</para>
        ///
        /// <para>THE EXCLUSION IS WHAT MAKES ONE POLL SERVE TWO CALLERS. Waiting for a
        /// REPLACEMENT has to exclude the id that was just killed, because the mod reports
        /// the engine it is using and a dead id would satisfy "non-zero" if anything were
        /// still holding it. Waiting for ANY engine excludes nothing and passes 0, which the
        /// non-zero test already covers.</para>
        ///
        /// <para>Returns 0 rather than throwing, because what to say about the wait having
        /// been in vain is different for each caller and neither answer belongs here.</para>
        /// </remarks>
        /// <param name="saveGames">Where probe commands are dropped.</param>
        /// <param name="watcher">The probe events to read the answer from.</param>
        /// <param name="timeout">How long one answer may take to arrive.</param>
        /// <param name="excluded">An id that does not count, or 0 to accept any.</param>
        /// <returns>The engine's process id, or 0 if none arrived in time.</returns>
        private static int WaitForAnEngine(
            string saveGames, ProbeWatcher watcher, TimeSpan timeout, int excluded)
        {
            DateTime deadline = DateTime.UtcNow + EngineWait;

            while (true)
            {
                ProbeCommand.SendLookAheadEngineProcess(saveGames);
                ProbeEvent answer = watcher.WaitForEvent(
                    "look-ahead-engine-process", timeout, Log);
                int engine = answer.Number("process") ?? 0;

                if (engine != 0 && engine != excluded)
                {
                    return engine;
                }

                if (DateTime.UtcNow >= deadline)
                {
                    return 0;
                }

                System.Threading.Thread.Sleep(EnginePoll);
            }
        }

        /// <summary>
        /// Waits until the mod is holding an engine that is NOT the one just killed.
        /// </summary>
        /// <remarks>
        /// <para>de-bnjy.1.3. POLLED RATHER THAN SLEPT: the replacement is fast enough that
        /// any fixed wait is either flaky or wasteful, and the mod can say which process it
        /// is holding, so the harness asks instead of guessing.</para>
        ///
        /// <para>A DIFFERENT id rather than merely a non-zero one, because the mod reports
        /// the engine it is using and the dead one's id would satisfy "non-zero" if
        /// anything were still holding it. Different and non-zero is the replacement and
        /// nothing else.</para>
        /// </remarks>
        private static void WaitForTheReplacement(
            LookAheadScenario scenario,
            string saveGames,
            ProbeWatcher watcher,
            TimeSpan timeout)
        {
            if (!scenario.KillEngineFirst || !scenario.ExpectsRecovery)
            {
                return;
            }

            int killed = _killedEngine;
            Console.WriteLine("        waiting for the mod to start a replacement engine");

            int engine = WaitForAnEngine(saveGames, watcher, timeout, excluded: killed);
            if (engine == 0)
            {
                throw new InvalidOperationException(
                    $"No replacement look-ahead engine arrived within {EngineWait
                        .TotalSeconds:0} seconds of killing process {killed}. Either "
                    + "the mod gave up instead of respawning - check the run's log for "
                    + "the shutdown notice - or the replacement could not start.");
            }

            Console.WriteLine($"        the replacement is process {engine}, and is up");
        }

        private static ProbeEvent OpenConversation(
            LookAheadScenario scenario,
            string saveGames,
            ProbeWatcher watcher,
            TimeSpan timeout)
        {
            string what = $"a response menu in conversation {scenario.ConversationId}";
            TimeSpan perAttempt = AttemptTimeout(timeout);

            // BEFORE THE CONVERSATION, so the menu this scenario is about is drawn by a mod
            // whose engine has already gone - which is the arrangement it exists to
            // describe. Outside the retry loop because it happens once: the engine is not
            // restarted, so a second attempt has nothing left to kill.
            if (scenario.KillEngineFirst)
            {
                KillTheEngine(saveGames, watcher, timeout);
            }

            for (int attempt = 1; ; attempt++)
            {
                bool last = attempt == OpenAttempts;
                if (attempt > 1)
                {
                    Console.WriteLine(
                        $"        asking for conversation {scenario.ConversationId} again "
                        + $"(attempt {attempt} of {OpenAttempts})");

                    // A retry is already a recovery, and the attempt that failed may have
                    // left a continue on disk that the probe never took. Sending the next
                    // command on top of it is refused outright, which reports a pending
                    // command where the real story is the attempt before it.
                    ProbeCommand.Clear(saveGames);
                }

                ProbeCommand.SendStartConversation(saveGames, scenario.ConversationId);

                // WITHOUT CONSUMING, because the first lines of the conversation arrive
                // alongside this answer and sometimes before it - the run that found this
                // showed line 451:0 between the command starting and finishing. A wait
                // that scanned past them would swallow them, and the count of lines this
                // conversation needs would depend on how quickly the probe answered.
                ProbeEvent started = WaitWithoutConsuming(
                    watcher,
                    e => e.Name == "command-finished"
                        && e.Text("command") == "start-conversation",
                    StartAcknowledgement,
                    $"an answer to start-conversation {scenario.ConversationId}");

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
                    return AdvanceToMenu(scenario, saveGames, watcher, perAttempt, what);
                }
                catch (TimeoutException stalled) when (!last)
                {
                    // THE REASON, NOT JUST THE FACT. This catch used to discard the
                    // message, and the two failures it hides are completely different
                    // things: a wait that ran out, and a command that finished saying the
                    // conversation reached something other than a menu - which is instant
                    // and names the outcome. de-wncd.4 was diagnosed twice from logs that
                    // could not tell them apart.
                    Console.WriteLine(
                        $"        it is open but drew no menu: {stalled.Message}");
                    Console.WriteLine("        letting the world settle");
                    Thread.Sleep(BetweenOpenAttempts);
                }
            }
        }

        /// <summary>
        /// Advances an open conversation to its first response menu.
        /// </summary>
        /// <remarks>
        /// <para>ASKS THE GAME RATHER THAN WATCHING IT. The probe runs the loop, off the
        /// interface's own state: the mouse UI's continue/options toggle says when a menu
        /// is up, and the continue button says when a line is asking to be advanced. This
        /// sends one command and waits for one answer.</para>
        ///
        /// <para>WHAT THAT REPLACED. The loop used to live here, inferring the same thing
        /// from the ORDER events reached the log - which is half a second late and silent
        /// about what has NOT happened. Deciding no menu was coming meant waiting three
        /// seconds and betting on it, once per line, forty seconds a run.</para>
        ///
        /// <para>A WAIT REMAINS, and it is worth being exact about: the interface does not
        /// announce a menu before it appears - the last line before one reads exactly like
        /// any other - so the probe still lets a line hold still before answering it. What
        /// changed is that the waiting is measured against the game's own state, at frame
        /// granularity, and that a menu ends it the instant it exists rather than a poll
        /// later. de-6vyj carries what would be needed to remove it entirely.</para>
        ///
        /// <para>BEFORE EITHER, this pressed Enter first and looked afterwards, so every
        /// scenario got a keypress whether or not its menu was already up - and an Enter on
        /// an open response menu PICKS THE HIGHLIGHTED OPTION. A suite about what has been
        /// read was quietly reading dialogue of its own choosing.</para>
        ///
        /// <para>The count is returned to the caller, which holds it against what the
        /// scenario says it should be. A scenario that suddenly needs a different number is
        /// not at the menu it thinks it is.</para>
        /// </remarks>
        /// <param name="scenario">The scenario being opened.</param>
        /// <param name="saveGames">The profile's SaveGames folder, for the probe.</param>
        /// <param name="watcher">The probe's events.</param>
        /// <param name="timeout">How long to keep at it.</param>
        /// <param name="what">What is being waited for, for the report.</param>
        /// <returns>The completed menu event.</returns>
        private static ProbeEvent AdvanceToMenu(
            LookAheadScenario scenario,
            string saveGames,
            ProbeWatcher watcher,
            TimeSpan timeout,
            string what)
        {
            var clock = Stopwatch.StartNew();
            ProbeCommand.SendAdvanceToMenu(saveGames);

            // WITHOUT CONSUMING, because the menu this produces is reported by its own
            // event and a scanning wait would swallow it on the way to this answer.
            ProbeEvent done = WaitWithoutConsuming(
                watcher,
                e => e.Name == "command-finished"
                    && e.Text("command") == ProbeCommand.AdvanceToMenu,
                timeout,
                "an answer to advance-to-menu");

            string outcome = done.Text("outcome") ?? "?";
            LastAdvances = done.Number("advances") ?? -1;
            if (outcome != "menu")
            {
                throw new TimeoutException(
                    $"The conversation did not reach a menu: {outcome} "
                    + $"({done.Text("message") ?? "no detail"}), after {LastAdvances} "
                    + $"advance(s) over {done.Number("lines")} line(s).");
            }

            ProbeEvent menu = watcher.WaitFor(
                e => e.Name == "menu"
                    && e.Number("conversation") == scenario.ConversationId
                    && e.Text("state") == "complete",
                timeout,
                what);

            Console.WriteLine(
                $"        {what} after {clock.Elapsed.TotalSeconds:N0}s and "
                + $"{LastAdvances} advance(s)");
            return menu;
        }

        /// <summary>How many advances the last conversation opened with.</summary>
        /// <remarks>
        /// Carried out of band rather than returned beside the menu because every caller
        /// but one wants the menu alone. It is read immediately after
        /// <see cref="OpenConversation"/> and never stored.
        /// </remarks>
        private static int LastAdvances { get; set; }

        /// <summary>
        /// Waits for an event without consuming anything, so a later reader still sees
        /// every event that arrived before it - and every one that arrived beside it.
        /// </summary>
        /// <param name="watcher">The probe's events.</param>
        /// <param name="matches">What is being waited for.</param>
        /// <param name="timeout">How long to wait.</param>
        /// <param name="what">What to call it, for the report.</param>
        /// <exception cref="TimeoutException">It never came.</exception>
        private static ProbeEvent WaitWithoutConsuming(
            ProbeWatcher watcher, Func<ProbeEvent, bool> matches, TimeSpan timeout, string what)
        {
            var clock = Stopwatch.StartNew();
            Console.WriteLine($"        waiting for {what} (0s)");
            while (true)
            {
                foreach (ProbeEvent candidate in watcher.Since())
                {
                    if (matches(candidate))
                    {
                        Console.WriteLine(
                            $"        saw {what} after {clock.Elapsed.TotalSeconds:N1}s");
                        return candidate;
                    }
                }

                if (clock.Elapsed >= timeout)
                {
                    throw new TimeoutException(
                        $"Waited {timeout.TotalSeconds:N0}s for {what} and it never came.");
                }

                Thread.Sleep(ProbeWatcher.DefaultPollInterval);
            }
        }

        private static void RunScenario(
            LookAheadScenario scenario,
            string saveGames,
            ProbeWatcher watcher,
            TimeSpan timeout,
            Report report,
            GameWindow window,
            string artifacts,
            string suiteName)
        {
            watcher.WaitForEvent("load-finished", timeout, Log);

            ProbeEvent menu = OpenConversation(scenario, saveGames, watcher, timeout);
            int advances = LastAdvances;

            // A SCENARIO THAT NEEDS A DIFFERENT NUMBER OF LINES IS NOT AT ITS OWN MENU.
            // The count is a fixed property of a save and a conversation, so a change in
            // it means the run arrived somewhere else - which used to happen silently and
            // is the whole reason this is checked rather than merely reported.
            if (scenario.Advances is int wanted)
            {
                report.Check(
                    advances == wanted,
                    $"{scenario.SaveName}: its conversation opened after {wanted} "
                        + "advance(s), as it always should",
                    advances == wanted
                        ? "it did"
                        : $"it took {advances} this time - the menu it reached is not the "
                            + "one this scenario was written against");
            }
            else
            {
                Console.WriteLine(
                    $"        (this scenario does not say how many advances it needs; it "
                    + $"took {advances})");
            }

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
                string rolls = option.Check == null ? string.Empty : $" ({option.Check} check)";
                Console.WriteLine(
                    $"        [{Describe(MarkerOn(option)),-6}] "
                    + $"{option.ConversationId}:{option.EntryId}{rolls} "
                    + $"{Trim(option.OwnLine())}");

                // ON ITS OWN ROW, because that is where the game draws it and because
                // trimming an option to a line once cut the Fail half off entirely - the
                // only in-game evidence the line existed at all was half of it.
                if (option.Branches() is ProbeBranchLine drawn)
                {
                    Console.WriteLine($"                   {drawn}");
                }
            }

            CaptureMenu(scenario, options, window, artifacts, suiteName);

            // BOTH OF THESE ARE "WHAT TO DO ABOUT A KILLED ENGINE ONCE ITS MENU HAS BEEN
            // DRAWN", and which one applies is the scenario's ExpectsRecovery. They are
            // here rather than beside the kill because THE MOD DOES NOT KNOW ITS ENGINE
            // DIED UNTIL SOMETHING ASKS IT ONE - the death surfaces as a failed request
            // while this menu was being prepared, and nothing before that has either raised
            // a notice or started a replacement.
            DismissTheNotice(scenario, saveGames, watcher, timeout, report);
            WaitForTheReplacement(scenario, saveGames, watcher, timeout);

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

            CheckBranchLines(scenario, options, report);
        }

        /// <summary>
        /// Presses the button on the window the killed engine raised, and checks that it
        /// goes away again.
        /// </summary>
        /// <remarks>
        /// <para>THE OTHER HALF OF THE PICTURE. A photograph says the window was drawn
        /// where the player would see it; it cannot say what happens when the player does
        /// the only thing the window offers. That matters more here than it would for a
        /// notification, because a window that will not close does not merely fail to
        /// inform - it sits over the conversation until the process is killed, which is a
        /// worse outcome than the silence this whole notice replaced.</para>
        ///
        /// <para>AFTER THE PHOTOGRAPH, and after the markers have been read, so nothing
        /// this run reports depends on the window still being up. It runs only for the
        /// scenario that killed the engine, because that is the only scenario that has a
        /// window to press.</para>
        /// </remarks>
        private static void DismissTheNotice(
            LookAheadScenario scenario,
            string saveGames,
            ProbeWatcher watcher,
            TimeSpan timeout,
            Report report)
        {
            if (!scenario.KillEngineFirst || scenario.ExpectsRecovery)
            {
                // A RECOVERY RAISES NO NOTICE, which is the whole claim of de-bnjy.1.3 -
                // the player is not interrupted for a death the mod can answer itself. So
                // there is no window here to press, and waiting for one would hang.
                return;
            }

            Console.WriteLine("        pressing the button on the notice");
            ProbeCommand.SendDismissNotice(saveGames);
            ProbeEvent answer = watcher.WaitForEvent("notice-dismissed", timeout, Log);

            // Defaults chosen so that a field that never arrived fails rather than passes:
            // absent means the probe could not see a window, and could not see it go.
            bool before = answer.Boolean("before") ?? false;
            bool after = answer.Boolean("after") ?? true;

            report.Check(
                before,
                $"{scenario.SaveName}: the notice was a window waiting to be dismissed",
                before
                    ? "it was on screen when the button was pressed"
                    : "there was no window on screen at all - the notice fell back to the "
                        + "passing notification, or was never raised");
            report.Check(
                !after,
                $"{scenario.SaveName}: pressing its button closed it",
                after
                    ? "it is STILL on screen, which leaves the player's game underneath a "
                        + "window they cannot get rid of"
                    : "it is gone, and the conversation is underneath where it was");
        }

        /// <summary>Where a run's menu pictures go, under its artifacts folder.</summary>
        private const string MenuPictures = "menus";

        /// <summary>
        /// Photographs a menu with something on it only a person can judge, while it is
        /// still up.
        /// </summary>
        /// <remarks>
        /// <para>THE ONE CLAIM THE PROBE CANNOT MAKE. Everything else this run checks is
        /// read from the text the game was about to draw, which says the line is composed
        /// and coloured correctly and says nothing at all about whether the player can see
        /// it: the response menu could lay its options out in fixed-height boxes and clip
        /// the second line, and every check here would still pass. A picture is what
        /// separates those two worlds, so one is taken.</para>
        ///
        /// <para>Only menus that actually have something to show, which is a handful per
        /// run - a picture of a menu with nothing on it costs the same and proves nothing.
        /// Two kinds qualify: a menu carrying a Pass / Fail line, and one where the engine
        /// was killed underneath it, whose notice is drawn by the game over the top. Taken
        /// here rather than at the end because the menu is up NOW; the run moves on to the
        /// next save as soon as this scenario returns.</para>
        ///
        /// <para>Not a check. Nothing automatic can look at the picture and say whether
        /// two lines were drawn, so a failed capture is reported and the run carries on -
        /// the markers are what pass or fail a run, and they have already been read.</para>
        /// </remarks>
        private static void CaptureMenu(
            LookAheadScenario scenario,
            ProbeOption[] options,
            GameWindow window,
            string artifacts,
            string suiteName)
        {
            // A killed engine earns a picture too, and for the same reason a check does:
            // the notice it raises is drawn by the game's own confirmation window, over the
            // menu, and nothing the probe reads can say whether it landed somewhere the
            // player would see. The log says it was raised, and in which of the mod's two
            // channels; only this says what it looked like.
            // A recovery has no notice to photograph; the picture is evidence about the
            // window, and this run deliberately produces none.
            bool raisedANotice = scenario.KillEngineFirst && !scenario.ExpectsRecovery;
            if (!raisedANotice && !options.Any(option => option.Branches() != null))
            {
                return;
            }

            string folder = Path.Combine(artifacts, MenuPictures);
            string path = Path.Combine(
                folder,
                $"{suiteName}-{scenario.SaveName}-{scenario.ConversationId}.png");
            try
            {
                Directory.CreateDirectory(folder);
                GameScreen.SaveCapture(window.Handle, path);
                Console.WriteLine($"        a check is on screen; picture in {path}");
            }
            catch (Exception error)
            {
                Console.WriteLine($"        could not photograph the menu: {error.Message}");
            }
        }

        /// <summary>
        /// Checks the Pass / Fail line drawn under each check option in a menu.
        /// </summary>
        /// <remarks>
        /// SEPARATE FROM THE MARKER CHECKS, because they are separate claims about
        /// separate things: the marker says what the option as a whole can still reach,
        /// and the line says which of a rolled check's two outcomes gets there. An option
        /// carries both, and a suite that conflated them could not tell a check whose
        /// failure leads somewhere new from one whose success does.
        /// </remarks>
        private static void CheckBranchLines(
            LookAheadScenario scenario, ProbeOption[] options, Report report)
        {
            if (scenario.BranchPolicy == BranchPolicy.Ignored)
            {
                return;
            }

            BranchExpectation? expected = scenario.Branches;
            foreach (ProbeOption option in options)
            {
                bool wanted = expected != null && option.IsRolledCheck;
                ProbeBranchLine? line = option.Branches();

                if (!wanted)
                {
                    report.Check(
                        line == null,
                        $"{scenario.SaveName}: entry {option.EntryId}, "
                            + (option.Check == null
                                ? "which rolls nothing, has no Pass / Fail line"
                                : $"a {option.Check} check, has no Pass / Fail line either"),
                        line == null ? "it has none" : $"it carries '{line}'");
                    continue;
                }

                if (line == null)
                {
                    report.Check(
                        false,
                        $"{scenario.SaveName}: entry {option.EntryId}, a {option.Check} "
                            + "check, is drawn with a Pass / Fail line",
                        $"it carries none - {expected!.Why}");
                    continue;
                }

                CheckHalf(scenario, option, expected!.Pass, line.Pass, expected.Why, report);
                CheckHalf(scenario, option, expected.Fail, line.Fail, expected.Why, report);
            }
        }

        /// <summary>Checks one half of one line, whole, as the markup it is drawn in.</summary>
        /// <remarks>
        /// <para>ONE STRING COMPARISON, against the markup the mod actually drew -
        /// <c>Pass[#C4453C]*[#FF8C42]</c> - rather than a colour bucket and a marker enum
        /// checked apart. The two used to be compared through <c>MarkerOn</c>, which maps a
        /// glyph and a colour onto <see cref="Marker"/>; anything it mapped the same way it
        /// accepted, and anything it could not map at all it THREW on, which turns a wrong
        /// colour into a crashed run rather than a failed check.</para>
        ///
        /// <para>WHAT THE TEXT CATCHES THAT THE BUCKETS DID NOT: the word itself, so a
        /// "Pass" drawn where "Fail" belongs is a failure rather than an invisible swap;
        /// a colour the mod does not use, reported as the hex it is instead of throwing;
        /// and the exact hex of both the word and its marker, which is the thing a player
        /// sees and the thing a theme change would move.</para>
        ///
        /// <para>The expectation is built from the same constants the mod is configured
        /// with, so the fixture keeps saying "orange" and "gaveUp" - names a person can
        /// read - and only the comparison is in hex. A row spelling the hex itself would
        /// have to be edited whenever the colours moved, which is how sixteen rows come to
        /// disagree with the mod one at a time.</para>
        /// </remarks>
        private static void CheckHalf(
            LookAheadScenario scenario,
            ProbeOption option,
            BranchHalf wanted,
            ProbeBranch drawn,
            string why,
            Report report)
        {
            string expected = MarkupOf(drawn.Word, wanted);
            string actual = drawn.ToString();

            report.Check(
                string.Equals(expected, actual, StringComparison.OrdinalIgnoreCase),
                $"{scenario.SaveName}: entry {option.EntryId} draws {expected}",
                $"it draws {actual} - {why}");
        }

        /// <summary>The markup a half would be drawn in if it were what the row claims.</summary>
        /// <remarks>
        /// The same shape <see cref="ProbeBranch.ToString"/> produces, so the two can be
        /// compared as text: the word, its colour in brackets, and where there is a marker
        /// the glyph and its own colour after it.
        /// </remarks>
        private static string MarkupOf(string word, BranchHalf wanted)
        {
            string colour = HtmlOf(wanted.Colour);
            return wanted.Marker switch
            {
                Marker.None => $"{word}[{colour}]",
                Marker.Uncertain =>
                    $"{word}[{colour}]{ProbeLog.UncertainMarkerGlyph}[{UncertainHtml}]",
                Marker.Orange =>
                    $"{word}[{colour}]{ProbeLog.MarkerGlyph}[{OrangeHtml}]",
                Marker.Red => $"{word}[{colour}]{ProbeLog.MarkerGlyph}[{RedHtml}]",
                _ => throw new InvalidDataException(
                    $"'{wanted.Marker}' is not a marker the mod draws."),
            };
        }

        // WHAT USED TO BE HERE: a Describe(ProbeBranch) that named a drawn half's colour
        // bucket, and a MarkerOn(ProbeBranch) that mapped its glyph and colour onto a
        // Marker. Both are gone with the move to comparing the markup as text - see
        // CheckHalf. MarkerOn THREW on a colour it could not place, which made a wrong
        // colour end the run instead of failing one check; the text comparison reports the
        // hex it found and carries on to the next half.
        //
        // The glyph and its colour still have to agree, which was that method's own reason
        // for existing: a grey '*?' and an orange '*' say opposite things, and either read
        // as the other turns "the search gave up" into "the search found something". A
        // whole-string comparison gets that for nothing.

        /// <summary>The colour the mod draws one branch state in.</summary>
        private static string HtmlOf(BranchColour colour) => colour switch
        {
            BranchColour.Orange => OrangeHtml,
            BranchColour.Red => RedHtml,
            _ => SeenHtml,
        };

        /// <summary>
        /// Asks the mod whether the world it would send the native look-ahead says the
        /// same thing as the world its managed engine reads, and checks the answer.
        /// </summary>
        /// <remarks>
        /// <para>The in-game half of de-i5xj.7, and the only place it can be asked: both
        /// worlds read the running game, so there is nothing to compare outside one. The
        /// comparison happens inside the plugin and comes back through the BepInEx log,
        /// which is the same channel the native library's own report uses and for the same
        /// reason - a probe command would need the library deployed beside the PROBE too.</para>
        ///
        /// <para>SKIPPED, not failed, where the native library was never built. A
        /// contributor who has not run <c>cargo build</c> has a mod that works, and failing
        /// their run would be telling them off for something that is not yet a
        /// requirement. Where the library IS there the agreement is a real check, because
        /// then a disagreement is a bug in the thing being built.</para>
        /// </remarks>
        private static void CheckSnapshot(
            int conversationId,
            string saveGames,
            ProbeWatcher watcher,
            TimeSpan timeout,
            string logPath,
            Report report)
        {
            NativeEngineReport native = NativeEngineReport.FromLog(logPath);
            if (!native.Loaded)
            {
                Console.WriteLine();
                Console.WriteLine($"  NOTE  no snapshot comparison: {native}");
                return;
            }

            Console.WriteLine();
            Console.WriteLine(
                $"comparing the snapshot against the managed world over {conversationId}...");

            watcher.Mark();
            ProbeCommand.SendCheckSnapshot(saveGames, conversationId);
            watcher.WaitForEvent("snapshot-checked", timeout, Log);

            // What the index check cost, and whether it passed. Checked here and not only
            // reported: on an unmodified install a rebuild would mean the extractor's
            // canonicalisation and the plugin's disagree about a database neither of them
            // changed, which is the drift the shared routine exists to prevent.
            IndexCacheReport cache = IndexCacheReport.FromLog(logPath);
            Console.WriteLine($"  NOTE  {cache}");
            report.Check(
                !cache.Rebuilt,
                "the shipped index still describes the game's dialogue database",
                cache.ToString());

            SnapshotAgreementReport agreement = SnapshotAgreementReport.FromLog(logPath);
            report.Check(
                agreement.Agreed,
                $"the snapshot answers what the managed world answers, over {conversationId}",
                agreement.ToString());

            // Separately, because it is the one part the comparison cannot check name for
            // name: the engine hands out a rendered call as each query's key and the plugin
            // runs it as Lua. A disagreement about that rendering answers Unknown for every
            // query in the group, silently, and every guard over one turns permissive.
            if (agreement.QueriesAsked > 0)
            {
                report.Check(
                    agreement.QueriesAnswered > 0,
                    $"the engine's query keys run in the game, over {conversationId}",
                    $"{agreement.QueriesAnswered} of {agreement.QueriesAsked} answered");
            }
        }

        /// <summary>How much of the log has been written, in characters.</summary>
        /// <remarks>
        /// Characters rather than bytes, since that is what the check indexes with, and
        /// the log is UTF-8 where the two do not agree.
        /// </remarks>
        private static int LogLength(string logPath) =>
            File.Exists(logPath) ? FilePaths.ReadShared(logPath).Length : 0;

        /// <summary>Checks what a suite says the mod should have written to the log.</summary>
        /// <remarks>
        /// writtenBefore is how long the log was when this suite started, from
        /// <see cref="LogLength"/>. Only what was written from there on is this suite's
        /// to answer for.
        /// </remarks>
        private static void CheckLog(
            LookAheadSuite suite, string logPath, int writtenBefore, Report report)
        {
            if (suite.LogExpectations.Count == 0)
            {
                return;
            }

            string whole = File.Exists(logPath) ? FilePaths.ReadShared(logPath) : string.Empty;

            // A log that SHRANK was rotated between the mark and here, and the mark means
            // nothing against the new file - read all of it rather than a wrong slice.
            string sinceSuiteBegan = whole.Length >= writtenBefore
                ? whole.Substring(writtenBefore)
                : whole;
            foreach (LogExpectation expected in suite.LogExpectations)
            {
                string log = expected.WholeRun ? whole : sinceSuiteBegan;
                int said = Occurrences(log, expected.Substring);
                bool present = said > 0;
                bool right = expected.Times == null
                    ? present == expected.ShouldAppear
                    : said == expected.Times;

                // WHICH TEXT WAS READ, said out loud, because it is the first thing
                // whoever reads a failure needs and the two answers differ.
                string which = expected.WholeRun ? "the run's log" : "this suite's log";
                report.Check(
                    right,
                    $"{suite.Name}: {expected.What}",
                    expected.Times == null
                        ? present
                            ? $"{which} says '{expected.Substring}'"
                            : $"{which} does not say '{expected.Substring}'"
                        : $"{which} says '{expected.Substring}' {said} time(s), and should "
                            + $"say it {expected.Times}");
            }
        }

        /// <summary>How many times one string appears in another.</summary>
        /// <remarks>
        /// NON-OVERLAPPING, which is the counting a reader means: the search resumes past
        /// the match rather than one character into it. Every substring these expectations
        /// use is a sentence, so it could not overlap itself anyway - but a count that
        /// depended on that would be a trap for whoever first writes a shorter one.
        /// </remarks>
        private static int Occurrences(string text, string sought)
        {
            if (sought.Length == 0)
            {
                return 0;
            }

            int count = 0;
            int at = text.IndexOf(sought, StringComparison.Ordinal);
            while (at >= 0)
            {
                count++;
                at = text.IndexOf(sought, at + sought.Length, StringComparison.Ordinal);
            }

            return count;
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
            : option.HasMarker(UncertainHtml, ProbeLog.UncertainMarkerGlyph) ? Marker.Uncertain
            : Marker.None;

        private static string Describe(Marker marker) => marker switch
        {
            Marker.Orange => "orange",
            Marker.Red => "red",
            Marker.Uncertain => "grey (the search gave up)",
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
