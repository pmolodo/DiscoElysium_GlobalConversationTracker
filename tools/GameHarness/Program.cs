// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.Threading;
using System.Drawing.Imaging;
using System.Drawing;
using System.IO;
using System.Linq;
using GlobalConversationTracker.Automation;

namespace GlobalConversationTracker.Harness
{
    /// <summary>
    /// Drives the game from outside: swaps in fixed settings, launches it, waits by
    /// screenshot, presses keys, and puts the settings back.
    /// </summary>
    /// <remarks>
    /// <para>It learns nothing from inside the game. Loading is detected by watching the
    /// screen, not by a hook, which is the constraint this harness was asked for.</para>
    ///
    /// <para>A console app rather than a PowerShell script because every primitive it
    /// needs - window enumeration, SendInput, screen capture - had to be compiled
    /// anyway: Defender blocks a PowerShell file containing them, matching
    /// HackTool:PowerShell/EmpireGetScreenshot. The signature is fair. This is a
    /// screen-scraping input-injecting tool, and it is better read as one compiled
    /// assembly than as a script that looks like malware.</para>
    /// </remarks>
    public static class Program
    {
        private const string DefaultProcessName = "disco";

        /// <summary>What this names itself as in a log file's name.</summary>
        private const string ToolName = "GameHarness";

        /// <summary>The save staged when none is named.</summary>
        /// <remarks>
        /// The archive's entries are named to match it. Every entry in a save is prefixed
        /// with the save's own name, and an archive whose entries disagree is ignored:
        /// renaming the file alone produced a main menu with no Continue and Load Game
        /// greyed out, the game having found no saves at all. A test checks they agree.
        /// </remarks>
        private const string TemplateSave = "save_template.ntwtf";

        private const string SaveLoadedLogText =
            "Resynced the global state after a savegame load:";

        /// <summary>Entry point.</summary>
        /// <param name="args">The verb and its options.</param>
        /// <returns>0 when every check passed.</returns>
        public static int Main(string[] args)
        {
            var options = Options.Parse(args);
            if (options == null)
            {
                PrintUsage();
                return 2;
            }

            try
            {
                // HERE RATHER THAN IN A WRAPPER SCRIPT, so that a verb typed by hand is
                // logged too - a wrapper that has to be remembered is the thing this
                // replaces. It also catches the in-game tests, which reach these verbs by
                // calling Main rather than by running the executable.
                //
                // Inside the try so that a run which cannot be logged - no repository
                // around it, nowhere to write - says so in one sentence like every other
                // failure here, and so that what the catch reports lands in the log.
                using IDisposable? log = options.NoLog
                    ? null
                    : RunLog.Begin(
                        ToolName, options.Verb, ToolName + " " + string.Join(" ", args));

                switch (options.Verb)
                {
                    case "analyse-timeline":
                        return AnalyseTimeline(options);
                    case "locks":
                    {
                        string target = options.Artifacts
                            ?? GlobalConversationTracker.Automation.GameProfile.ProfilePath;
                        Console.WriteLine($"asking about {target}");
                        Console.WriteLine();
                        Console.WriteLine(FileLocks.Describe(target));
                        return 0;
                    }

                    case "unlock":
                    {
                        string target = options.Artifacts
                            ?? GlobalConversationTracker.Automation.GameProfile.ProfilePath;
                        string[] showing = ExplorerWindows.Showing(target);
                        Console.WriteLine(
                            showing.Length == 0
                                ? $"No Explorer window is showing {target}"
                                : $"{showing.Length} Explorer window(s) showing it:");
                        foreach (string location in showing)
                        {
                            Console.WriteLine($"  {location}");
                        }

                        int moved = ExplorerWindows.CloseShowing(
                            target, message => Console.WriteLine($"  {message}"));
                        Console.WriteLine($"closed {moved} Explorer window(s)");

                        // Explorer first because it is the cheapest to be rid of: the
                        // window is closed outright, with nothing to save and nothing to
                        // ask. Only what is still holding the folder after that is worth
                        // asking to close.
                        Console.WriteLine();
                        LockHolder[]? stillHolding = SysinternalsHandle.WhoIsHolding(
                            target, executable: null,
                            message => Console.WriteLine($"  {message}"));

                        if (stillHolding == null)
                        {
                            Console.WriteLine(SysinternalsHandle.InstallHint);
                            return 0;
                        }

                        if (stillHolding.Length == 0)
                        {
                            Console.WriteLine("Nothing is holding it now.");
                            return 0;
                        }

                        foreach (LockHolder holder in stillHolding)
                        {
                            Console.WriteLine($"  still held by {holder}");
                        }

                        Console.WriteLine();
                        CloseAttempt[] attempts = PoliteClose.AskToClose(
                            stillHolding,
                            options.Askable,
                            TimeSpan.FromSeconds(options.CloseDeadlineSeconds),
                            message => Console.WriteLine($"  {message}"));

                        foreach (CloseAttempt attempt in attempts)
                        {
                            Console.WriteLine($"  {attempt}");
                        }

                        if (attempts.Length == 0)
                        {
                            Console.WriteLine(
                                "  Nothing holding it is on the list this may ask to close "
                                + $"({string.Join(", ", options.Askable)}); close it yourself.");
                        }

                        return 0;
                    }

                    case "capture-phases":
                        return CapturePhases(options);

                    case "windows":
                        return ListWindows(options);
                    case "keys":
                        Console.WriteLine(string.Join(", ", GameKeyboard.KeyNames));
                        return 0;
                    case "capture-reference":
                        return RunSession(options, captureReference: true);
                    case "load-save":
                        return RunSession(options, captureReference: false);
                    case "look-ahead":
                        return LookAheadRun.Run(
                            ResolveGame(options),
                            options.Artifacts is null
                                ? Path.Combine(RepoRoot(), "testing", "scenarios")
                                : options.Artifacts,
                            options.TestSettings
                                ?? Path.Combine(RepoRoot(), "testing", "Settings.json"),
                            Path.Combine(RepoRoot(), ".build", "automation"),
                            TimeSpan.FromSeconds(options.TimeoutSeconds),
                            options.KeepOpen,
                            options.SuiteNames,
                            options.MemoryBudgetMb,
                            options.TimeBudgetMs,
                            options.ScenarioNames);
                    default:
                        PrintUsage();
                        return 2;
                }
            }
            catch (Exception error)
            {
                Console.Error.WriteLine();
                Console.Error.WriteLine($"FAILED: {error.Message}");
                return 1;
            }
        }

        private static void PrintUsage()
        {
            Console.WriteLine(@"Usage: GameHarness <verb> [options]

Verbs:
  capture-reference   Launch, wait for the screen to settle, and save it as the
                      main-menu reference. Look at the PNG before trusting it.
  load-save           Launch, confirm the main menu, send the load-save keys, and
                      check the screen changed to something else.
  look-ahead          Launch once, then for each money scenario ask the test
                      probe to load its save and open Siileng's conversation,
                      and check which options carry a look-ahead marker. The
                      scenarios are committed under testing/scenarios as sparse
                      diffs of save_template; the probe is installed and removed
                      by this verb. --artifacts names the scenario folder, and
                      --suite runs one or more named suites instead of all of them.
                      Repeat --suite or separate names with commas.
  --scenario <names>  Run only these scenarios of the selected suites, instead of
                      every scenario they hold. A name is a save - at-the-fan -
                      which takes every scenario loading it, or a save and a
                      conversation - at-the-fan:9 - which takes exactly one.
                      Repeatable and comma-separated, like --suite; a name that
                      matches nothing is an error rather than an empty run.
                      A filtered suite makes NONE of its whole-suite checks: its
                      artefacts and log expectations are claims about all of its
                      scenarios. For iterating; run the whole thing to validate.
  analyse-timeline    Read a recorded timeline back and report which frames
                      identify which screens, with a measured threshold. Add
                      --save-reference to write the chosen frame as main-menu.png.
  locks               Report what is holding the game's profile folder open,
                      which is what blocks staging. --artifacts asks about
                      another path instead.
  unlock              Close any Explorer window showing the profile folder, then
                      ask whatever still holds it to close. Nothing is ever
                      killed: a process that answers with a save prompt is left
                      running and reported.
  windows             List every window the game's process owns, with its class.
                      What to run when the wrong window is being captured.
  keys                List the key names the harness accepts.

Options:
  --game <path>       disco.exe. Found in the usual Steam libraries if omitted.
  --process <name>    Process name without .exe (default: disco).
  --artifacts <dir>   Where screenshots go (default: .build/automation).
  --settings <file>   Test settings to install (default: testing/Settings.json).
  --save <path>       The .ntwtf.zip or expanded .ntwtf source to stage as the ONLY save. Defaults to
                      the save in testing/, so Continue has exactly
                      one thing it can load.
  --keys a,b,c        The key sequence for load-save (default: Enter, which is
                      Continue when a single save has been staged).
  --threshold <n>     How close the menu match must be (default: 0.05).
  --whole-frame       Match the whole screen instead of just the menu options.
                      The default region excludes the animated painting, which
                      is 96% of the variation when the whole frame is used.
  --timeout <n>       Seconds to wait for loading, the slow part (default: 300).
  --window-timeout    Seconds to wait for the game window to appear (default: 30).
                      A window either shows up quickly or something is wrong.
  --dry-run           Do everything except press keys.
  --keep-open         Leave the game running afterwards.
  --via-steam         Launch through the Steam client (steam://run/<id>) rather
                      than running disco.exe. Steam does work before the process
                      exists that a direct launch skips, so which settings the
                      game honours may differ between the two.
  --app-id <id>       Steam app id for --via-steam (default: 632470).
  --close-holders     When the profile cannot be moved, clear what is holding
                      it: Explorer windows showing it are closed, and editors are
                      asked to close. Off by default - a test run is no reason to
                      close somebody's windows. Without it a blocked move just
                      reports what is holding the folder.
  --askable a,b       Process names unlock may ask to close (default: Code).
                      Explorer is not on this list; its windows are closed
                      directly rather than the process being asked anything.
  --close-deadline    Seconds to wait for an asked process before leaving it
                      running (default: 30, long enough to answer a save
                      prompt). It is never killed.
  --timeline          Capture every frame of startup instead of waiting for the
                      screen to settle. Startup runs through several animated
                      screens, so stopped-changing never becomes true - and a
                      looping animation can sample identically twice and fake it.
  --timeline-interval How often to capture in timeline mode, in ms (default 1000).
  --timeline-seconds  How long to record in timeline mode (default 60). Separate
                      from --timeout, which is the wait for the window to appear.
  --verbose           Report every sample the waits take.
  --no-log            Do not keep a log of this run. Every verb otherwise writes
                      its whole output to testing/logs under a name carrying the
                      date, the commit it ran against, and the verb. Setting
                      DISCO_ELYSIUM_GCT_NO_RUN_LOG does the same thing.");
        }

        /// <summary>
        /// Reads a recorded startup back and works out which frames identify which
        /// screens, and what threshold separates them.
        /// </summary>
        /// <remarks>
        /// Runs on the saved PNGs, so it needs no game and can be re-run freely. That is
        /// the point: a threshold chosen because it looked about right is how a check ends
        /// up passing for the wrong reason, and this startup contains both a screen that
        /// animates and a screen that holds perfectly still for twenty-five seconds.
        /// </remarks>
        private static int AnalyseTimeline(Options options)
        {
            string artifacts = options.Artifacts ?? Path.Combine(RepoRoot(), ".build", "automation");
            string directory = Path.Combine(artifacts, "timeline");
            if (!Directory.Exists(directory))
            {
                throw new DirectoryNotFoundException(
                    $"No timeline at {directory}. Run capture-reference --timeline first.");
            }

            string[] files = Directory.GetFiles(directory, "timeline-*.png");
            Array.Sort(files, StringComparer.Ordinal);
            if (files.Length == 0)
            {
                throw new InvalidOperationException($"No timeline frames in {directory}.");
            }

            Console.WriteLine($"reading {files.Length} frames from {directory}");
            var fingerprints = new List<double[]>();
            foreach (string file in files)
            {
                fingerprints.Add(GameScreen.FingerprintFile(file));
            }

            TimelineStage[] stages = TimelineAnalysis.FindStages(fingerprints);

            Console.WriteLine();
            Console.WriteLine($"{stages.Length} distinct screens:");
            foreach (TimelineStage stage in stages)
            {
                string blank = stage.MeanDetail < GameSession.BlankDetailFloor
                    ? "   <- below the blank floor; a wait starting here reads it as unpainted"
                    : string.Empty;
                Console.WriteLine($"  {stage}{blank}");
            }

            Console.WriteLine();
            Console.WriteLine("best identifying frame per screen:");
            foreach (TimelineStage stage in stages)
            {
                ReferenceQuality quality = TimelineAnalysis.BestReference(fingerprints, stage);
                Console.WriteLine($"  {Path.GetFileName(files[quality.Frame])}  {quality}");
            }

            TimelineStage last = stages[stages.Length - 1];

            // Whole-frame first, then candidate regions. The menu is a static list of
            // options beside a painting that never stops moving, so the whole frame
            // measures mostly the painting: the threshold has to tolerate the animation,
            // and that same tolerance is what lets another screen match. Narrowing to the
            // still part removes the problem instead of budgeting for it.
            Console.WriteLine();
            Console.WriteLine("matching the last screen, whole frame vs regions:");

            var candidates = new List<KeyValuePair<string, Rectangle?>>
            {
                new KeyValuePair<string, Rectangle?>("whole frame", null),
                new KeyValuePair<string, Rectangle?>(
                    "menu text", new Rectangle(160, 100, 250, 320)),
                new KeyValuePair<string, Rectangle?>(
                    "left panel", new Rectangle(100, 0, 320, 720)),
                new KeyValuePair<string, Rectangle?>(
                    "options only", new Rectangle(170, 115, 230, 290)),
            };

            if (options.Region != null)
            {
                candidates.Add(new KeyValuePair<string, Rectangle?>("--region", options.Region));
            }

            ReferenceQuality? best = null;
            Rectangle? bestRegion = null;
            string bestName = "whole frame";

            foreach (KeyValuePair<string, Rectangle?> candidate in candidates)
            {
                List<double[]> prints;
                if (candidate.Value == null)
                {
                    prints = fingerprints;
                }
                else
                {
                    prints = new List<double[]>();
                    foreach (string file in files)
                    {
                        prints.Add(GameScreen.FingerprintFileRegion(file, candidate.Value.Value));
                    }
                }

                ReferenceQuality quality = TimelineAnalysis.BestReference(prints, last);
                string verdict = quality.IsUsable
                    ? $"varies {quality.WorstWithinStage:N4}, nearest other {quality.BestOutsideStage:N4}"
                        + $", margin {quality.Margin:N4}, threshold {quality.SuggestedThreshold:N4}"
                    : "UNUSABLE";
                Console.WriteLine($"  {candidate.Key,-14} {verdict}");

                if (quality.IsUsable && (best == null || quality.Margin > best.Margin))
                {
                    best = quality;
                    bestRegion = candidate.Value;
                    bestName = candidate.Key;
                }
            }

            ReferenceQuality menu = best ?? TimelineAnalysis.BestReference(fingerprints, last);
            string chosen = files[menu.Frame];

            Console.WriteLine();
            Console.WriteLine("The last screen is normally the main menu.");
            Console.WriteLine($"  reference:  {Path.GetFileName(chosen)}");
            Console.WriteLine(
                $"  region:     {bestName}"
                + (bestRegion == null
                    ? string.Empty
                    : $" ({bestRegion.Value.X},{bestRegion.Value.Y} "
                        + $"{bestRegion.Value.Width}x{bestRegion.Value.Height})"));
            if (menu.IsUsable)
            {
                Console.WriteLine($"  threshold:  {menu.SuggestedThreshold:N4}");
                Console.WriteLine();
                Console.WriteLine(
                    $"  Above the {menu.WorstWithinStage:N4} that screen varies by as it animates,");
                Console.WriteLine(
                    $"  and below the {menu.BestOutsideStage:N4} to the nearest other screen.");
                Console.WriteLine("  Both measured here, not chosen.");

                if (options.SaveReference)
                {
                    string referencePath = Path.Combine(artifacts, "main-menu.png");
                    File.Copy(chosen, referencePath, overwrite: true);
                    Console.WriteLine();
                    Console.WriteLine($"Saved it as {referencePath}");
                    Console.WriteLine("Look at it and confirm it is the main menu before relying on it.");
                }
            }
            else
            {
                Console.WriteLine();
                Console.WriteLine(
                    "  NO usable threshold: this screen varies more than it differs from");
                Console.WriteLine(
                    "  another one, so matching it would match that one too. A longer");
                Console.WriteLine("  recording, or a region rather than the whole frame, is needed.");
                return 1;
            }

            return 0;
        }

        /// <summary>
        /// The startup screens, and which recorded frame identifies each.
        /// </summary>
        /// <remarks>
        /// Frames and thresholds come from analyse-timeline over a clean recording. The
        /// thresholds sit half way between how much each screen varies within itself and
        /// how far it is from the nearest other screen, both measured.
        /// </remarks>
        private static readonly (string Name, int Frame, Rectangle? Region)[] PhaseSources =
        {
            ("loading", 4, null),
            ("legal-notice", 14, null),

            // The logo animates in the middle of a still grey field, so it is recognised
            // by the field: everything above the animation, which never changes.
            ("logo", 20, new Rectangle(0, 0, 1280, 400)),

            ("main-menu", 28, new Rectangle(170, 115, 230, 290)),
        };

        /// <summary>
        /// How much of the gap between a screen and the next-nearest one to allow.
        /// </summary>
        /// <remarks>
        /// A quarter, not a half. Halfway is the widest a threshold can be while still
        /// telling the four screens apart, and it is far too wide for the other job these
        /// do: rejecting screens that are none of them. A shutting-down game was being
        /// identified as the legal notice at 0.07, because that was inside a threshold set
        /// by how far away the LOGO is rather than by how much the legal notice varies.
        ///
        /// Screens vary within themselves by 0.0002 to 0.0075, so a quarter of the gap is
        /// still many times looser than they need.
        /// </remarks>
        private const double ThresholdShare = 0.25;

        /// <summary>The smallest threshold to allow, whatever the measurement says.</summary>
        /// <remarks>
        /// Recorded frames are compared with each other; live ones are captured from a
        /// screen and differ a little more. A floor keeps a screen that measured as
        /// perfectly still from being rejected over that.
        /// </remarks>
        private const double MinimumThreshold = 0.01;

        /// <summary>The phase whose arrival ends the wait.</summary>
        private const string MenuPhase = StartupWatcher.MenuPhaseName;

        /// <summary>How many unrecognised screens to save a picture of.</summary>
        /// <remarks>
        /// Enough to see what went wrong without filling the artifacts folder when a run
        /// spends a minute unable to recognise anything.
        /// </remarks>
        private const int MaxUnknownCaptures = 6;

        /// <summary>
        /// Waits for the main menu, saying which startup screen is showing as it goes.
        /// </summary>
        /// <remarks>
        /// <para>The menu itself is what is waited for; the phases are for reporting, and
        /// for the one screen worth acting on. The logo can be skipped with a keypress, so
        /// recognising it saves several seconds a run - and knowing where startup has got
        /// to turns a silent fifty second wait into something that says what it is doing.
        /// </para>
        ///
        /// <para>An unrecognised screen is not an error. Startup has two brief screens
        /// before the loading one that are not worth naming, and a game update could add
        /// more; the wait carries on regardless, because the menu is what it is really
        /// looking for. Without the phase file at all, it just waits.</para>
        /// </remarks>
        private static WaitResult WaitForMenu(
            GameWindow window, string referencePath, string artifacts, Options options)
        {
            int unknowns = 0;
            StartupPhase[] phases = Array.Empty<StartupPhase>();
            string phasePath = Path.Combine(RepoRoot(), "testing", StartupPhases.DefaultFileName);
            try
            {
                phases = StartupPhases.Load(phasePath);
            }
            catch (Exception error)
            {
                Console.WriteLine(
                    $"  (no startup phases, so just waiting: {error.Message})");
            }

            // The menu is one of the phases, so its own measured threshold decides when it
            // has arrived. Comparing a reference image at a separate --threshold as well
            // meant two numbers answering one question: a run identified the menu at
            // 0.0735 and then kept waiting, because the other number was 0.05.
            StartupPhase? menuPhase = Array.Find(phases, p => p.Name == MenuPhase);
            if (menuPhase == null)
            {
                Console.WriteLine(
                    $"  (no '{MenuPhase}' phase, so matching {Path.GetFileName(referencePath)} "
                    + $"at --threshold {options.Threshold:N4} instead)");
            }

            double[] menu = menuPhase?.Fingerprint
                ?? (options.MenuRegion == null
                    ? GameScreen.FingerprintFile(referencePath)
                    : GameScreen.FingerprintFileRegion(referencePath, options.MenuRegion.Value));

            Rectangle? menuRegion = menuPhase?.Region ?? options.MenuRegion;
            double menuThreshold = menuPhase?.Threshold ?? options.Threshold;

            var watcher = new StartupWatcher(
                options.ProcessName,
                phases,
                menu,
                menuRegion,
                menuThreshold,
                progress: message => Console.WriteLine($"  {message}"),
                onUnknownScreen: (screen, elapsed) =>
                {
                    // Kept because by the time a run has finished the screen is long
                    // gone - the last one is captured after the game has closed, which is
                    // how a terminal ended up in one.
                    if (unknowns >= MaxUnknownCaptures)
                    {
                        return;
                    }

                    unknowns++;
                    string file = Path.Combine(
                        artifacts, $"unknown-{elapsed.TotalSeconds:00}s.png");
                    screen.Save(file, System.Drawing.Imaging.ImageFormat.Png);
                    Console.WriteLine($"           saved {Path.GetFileName(file)}");
                });

            return watcher.WaitForMenu(TimeSpan.FromSeconds(options.TimeoutSeconds));
        }

        /// <summary>Builds the startup phase file from a recorded timeline.</summary>
        private static int CapturePhases(Options options)
        {
            string artifacts = options.Artifacts ?? Path.Combine(RepoRoot(), ".build", "automation");
            string directory = Path.Combine(artifacts, "timeline");
            if (!Directory.Exists(directory))
            {
                throw new DirectoryNotFoundException(
                    $"No timeline at {directory}. Run capture-reference --timeline first.");
            }

            string[] all = Directory.GetFiles(directory, "timeline-*.png");
            Array.Sort(all, StringComparer.Ordinal);

            // Which frames are the same screen, so a threshold can be measured against
            // that rather than guessed from where a gap falls in a list of distances.
            var whole = new List<double[]>();
            foreach (string file in all)
            {
                whole.Add(GameScreen.FingerprintFile(file));
            }

            TimelineStage[] stages = TimelineAnalysis.FindStages(whole);

            var phases = new List<StartupPhase>();
            foreach ((string name, int frame, Rectangle? region) in PhaseSources)
            {
                string file = Path.Combine(directory, $"timeline-{frame:D4}.png");
                if (!File.Exists(file))
                {
                    throw new FileNotFoundException($"No frame {frame} at {file}.", file);
                }

                double[] fingerprint = region == null
                    ? GameScreen.FingerprintFile(file)
                    : GameScreen.FingerprintFileRegion(file, region.Value);

                // Which frames are the same screen comes from the stage split, not from
                // guessing where a gap falls in the distances. How far this frame is from
                // them is then measured in its OWN region, which is the number a threshold
                // has to sit above; the nearest frame of any other stage is what it has to
                // stay below.
                int index = Array.IndexOf(all, file);
                TimelineStage stage = Array.Find(stages, s => index >= s.FirstFrame && index <= s.LastFrame)
                    ?? throw new InvalidOperationException(
                        $"Frame {frame} is not inside any stage; the recording may be truncated.");

                double within = 0;
                double nearest = 1.0;
                for (int i = 0; i < all.Length; i++)
                {
                    if (i == index)
                    {
                        continue;
                    }

                    double[] print = region == null
                        ? GameScreen.FingerprintFile(all[i])
                        : GameScreen.FingerprintFileRegion(all[i], region.Value);
                    double distance = GameScreen.Difference(fingerprint, print);

                    if (i >= stage.FirstFrame && i <= stage.LastFrame)
                    {
                        if (distance > within)
                        {
                            within = distance;
                        }
                    }
                    else if (distance < nearest)
                    {
                        nearest = distance;
                    }
                }

                double threshold = Math.Max(
                    MinimumThreshold, within + ((nearest - within) * ThresholdShare));

                phases.Add(new StartupPhase(name, fingerprint, threshold, region));
                Console.WriteLine(
                    $"  {name,-13} timeline-{frame:D4}.png  varies {within:N4}, "
                    + $"nearest other {nearest:N4}, threshold {threshold:N4}");
            }

            string path = Path.Combine(RepoRoot(), "testing", StartupPhases.DefaultFileName);
            StartupPhases.Save(path, phases);

            Console.WriteLine();
            Console.WriteLine($"wrote {path} ({new FileInfo(path).Length:N0} bytes)");

            // Every recorded frame, labelled. A frame that identifies as the wrong screen,
            // or as nothing at all, is the thing to catch here rather than mid-launch.
            Console.WriteLine();
            Console.WriteLine("checking every recorded frame against them:");
            string[] files = Directory.GetFiles(directory, "timeline-*.png");
            Array.Sort(files, StringComparer.Ordinal);

            string last = string.Empty;
            int unknown = 0;
            foreach (string file in files)
            {
                using var bitmap = new System.Drawing.Bitmap(file);
                StartupPhase? phase = StartupPhases.Identify(bitmap, phases, out double difference);
                string label = phase?.Name ?? "unknown";
                if (phase == null)
                {
                    unknown++;
                }

                if (label != last)
                {
                    Console.WriteLine(
                        $"  {Path.GetFileName(file)}  {label}"
                        + (phase == null ? string.Empty : $" ({difference:N4})"));
                    last = label;
                }
            }

            Console.WriteLine();
            Console.WriteLine(
                $"{files.Length - unknown} of {files.Length} frames identified; {unknown} unknown.");
            return 0;
        }

        private static int ListWindows(Options options)
        {
            GameWindow[] windows = GameSession.AllWindows(options.ProcessName);
            if (windows.Length == 0)
            {
                Console.WriteLine($"No visible windows for '{options.ProcessName}'. Is it running?");
                return 1;
            }

            foreach (GameWindow window in windows)
            {
                string note = window.ClassName == GameWindows.UnityWindowClass
                    ? "  <- the game"
                    : window.ClassName == GameWindows.ConsoleWindowClass
                        ? "  <- console, never captured"
                        : string.Empty;
                Console.WriteLine(
                    $"  {window.ClassName,-22} {window.Width,5}x{window.Height,-5} '{window.Title}'{note}");
            }

            return 0;
        }

        /// <summary>
        /// Moves the profile aside, clearing what is holding it if the first try fails.
        /// </summary>
        /// <remarks>
        /// Nothing is touched without --close-holders. Running a test is not a reason to
        /// move somebody's Explorer window or take their editor away, even politely, and
        /// even though moving a window costs nothing to undo. Without the flag a blocked
        /// move reports what is holding the folder and stops.
        ///
        /// With it, and only after a move has ALREADY failed: Explorer windows showing the
        /// folder are closed, and anything still holding it whose name is on the askable
        /// list is asked to close.
        /// </remarks>
        /// <summary>
        /// How long to keep retrying the move after clearing what was holding it.
        /// </summary>
        /// <remarks>
        /// Explorer releases a folder some time after a window leaves it, not at once, and
        /// holds subfolders that window visited earlier. Fifteen seconds covers that
        /// without turning a genuinely stuck folder into a long wait.
        /// </remarks>
        private static readonly TimeSpan ReleaseWait = TimeSpan.FromSeconds(15);

        private static ProfileBackup BackupProfile(string backupPath, Options options)
        {
            try
            {
                return GameProfile.Backup(backupPath);
            }
            catch (Exception error) when (error is IOException || error is UnauthorizedAccessException)
            {
                Console.WriteLine();
                Console.WriteLine("could not move the profile; something is holding it.");

                string profile = GameProfile.ProfilePath;

                if (!options.CloseHolders)
                {
                    Console.WriteLine();
                    Console.WriteLine(FileLocks.Describe(profile));
                    Console.WriteLine();
                    Console.WriteLine(
                        "Close it yourself, or re-run with --close-holders to have this move "
                        + "Explorer windows off the folder and ask editors to close.");
                    throw;
                }

                int moved = ExplorerWindows.CloseShowing(
                    profile, message => Console.WriteLine($"  {message}"));

                int closed = 0;
                LockHolder[]? holders = SysinternalsHandle.WhoIsHolding(
                    profile, executable: null, message => Console.WriteLine($"  {message}"));

                if (holders != null && holders.Length > 0)
                {
                    foreach (CloseAttempt attempt in PoliteClose.AskToClose(
                        holders,
                        options.Askable,
                        TimeSpan.FromSeconds(options.CloseDeadlineSeconds),
                        message => Console.WriteLine($"  {message}")))
                    {
                        Console.WriteLine($"  {attempt}");
                        if (attempt.Closed)
                        {
                            closed++;
                        }
                    }
                }

                if (moved == 0 && closed == 0)
                {
                    // Nothing changed, so retrying would fail for exactly the same reason.
                    Console.WriteLine();
                    Console.WriteLine(FileLocks.Describe(profile));
                    throw;
                }

                // Explorer does not release a folder the instant a window leaves it, and it
                // keeps handles on subfolders that window visited earlier - the run that
                // showed this had moved the window off SaveGames and was still held on
                // Settings. So retry for a while rather than once: an immediate retry
                // reports failure for something that clears itself a second later.
                var deadline = DateTime.UtcNow + ReleaseWait;
                Exception last = error;
                while (DateTime.UtcNow < deadline)
                {
                    Thread.Sleep(1000);
                    try
                    {
                        Console.WriteLine("  trying again...");
                        return GameProfile.Backup(backupPath);
                    }
                    catch (Exception again)
                        when (again is IOException || again is UnauthorizedAccessException)
                    {
                        last = again;
                    }
                }

                Console.WriteLine();
                Console.WriteLine(FileLocks.Describe(profile));
                throw last;
            }
        }

        private static int RunSession(Options options, bool captureReference)
        {
            string game = ResolveGame(options);
            string artifacts = options.Artifacts ?? Path.Combine(RepoRoot(), @".build\automation");
            string testSettings = options.TestSettings ?? Path.Combine(RepoRoot(), @"testing\Settings.json");
            string referencePath = Path.Combine(artifacts, "main-menu.png");
            string logPath = Path.Combine(
                FilePaths.FolderOf(game, nameof(game)), "BepInEx", "LogOutput.log");

            Directory.CreateDirectory(artifacts);

            Console.WriteLine($"game:      {game}");
            Console.WriteLine($"artifacts: {artifacts}");
            Console.WriteLine($"settings:  {testSettings}");
            if (options.DryRun)
            {
                Console.WriteLine("mode:      dry run, no keys will be sent");
            }

            var checks = new Checks();
            string saveSource = options.SaveFile
                ?? Path.Combine(RepoRoot(), "testing", TemplateSave);
            string saveTemplate = PackSave(saveSource, artifacts);

            // The same staging the look-ahead run uses. Sharing it is the point: this
            // one forced a known window size and asserted it while that one staged only
            // the settings file, so the two were measuring the same game at different
            // resolutions with nothing saying so.
            StagedGame staged = StagedGame.Stage(
                options.ProcessName,
                testSettings,
                new[] { saveTemplate },
                globalStateFile: null,
                screenOverride: options.RegistryScreen,
                installScreenPrefs: !options.SkipScreenPrefs,
                backupProfile: path => BackupProfile(path, options),
                progress: message => Console.WriteLine($"staging:   {message}"));
            DisplaySettings requested = staged.Requested;

            Console.WriteLine(
                $"           {Path.GetFileName(saveSource)} as the only save, so "
                + "Continue can only load that one");

            Process? process = null;
            try
            {

                Console.WriteLine();
                Console.WriteLine("launching...");
                if (options.ViaSteam)
                {
                    // Through the Steam client, the way a player starts it. Worth being
                    // able to choose: Steam does work before the process exists - the
                    // cloud download among it - that launching the exe skips entirely, so
                    // the two launch methods are not interchangeable when the question is
                    // which settings the game honours.
                    Console.WriteLine($"  via Steam: steam://run/{options.AppId}");
                    Process.Start(new ProcessStartInfo($"steam://run/{options.AppId}")
                    {
                        UseShellExecute = true,
                    });

                    // Steam returns immediately and spawns the game itself, so there is no
                    // process handle to hold; it gets found by name below.
                    process = null;
                }
                else
                {
                    process = Process.Start(game);
                }

                GameWindow window = GameSession.WaitForWindow(
                    options.ProcessName, TimeSpan.FromSeconds(options.WindowTimeoutSeconds));
                Console.WriteLine($"  window: {window.ClassName} '{window.Title}'");
                checks.Pass("the game window appeared");

                // Steam Auto-Cloud syncs this directory when the application launches,
                // downloading the cloud copy BEFORE the game reads it. So settings staged
                // moments ago can already be gone, and the game runs with the player's
                // real ones - which is indistinguishable from the staging having failed
                // unless the bytes are compared.
                DisplaySettings wanted = GameSettings.ReadDisplay(testSettings);
                bool stagedSurvived = GameSettings.StillMatches(testSettings);
                if (!stagedSurvived)
                {
                    Console.WriteLine();
                    Console.WriteLine(
                        "  The staged settings file was REPLACED between installing it and the");
                    Console.WriteLine(
                        "  game starting. Steam Auto-Cloud syncs this folder on launch and will");
                    Console.WriteLine(
                        "  overwrite it with the cloud copy. Turn Steam Cloud off for this game:");
                    Console.WriteLine(
                        "  Library > right-click Disco Elysium > Properties > General (older");
                    Console.WriteLine(
                        "  clients: Updates) > uncheck the Steam Cloud option.");
                    Console.WriteLine();
                }

                checks.Check("the staged settings survived the launch", stagedSurvived);

                Console.WriteLine(
                    $"  opened at: {window.Width}x{window.Height} "
                    + $"{GameWindows.DescribeStyle(window.Handle)}");

                bool rightSize =
                    window.Width == requested.Width && window.Height == requested.Height;
                checks.Check(
                    $"the window is the requested {requested}",
                    rightSize,
                    $"got {window.Width}x{window.Height}");

                if (!rightSize)
                {
                    throw new InvalidOperationException(
                        $"The game opened at {window.Width}x{window.Height}, not the "
                        + $"{requested.Width}x{requested.Height} asked for. The registry "
                        + "PlayerPrefs did not take effect; see testing/SETTINGS-PRECEDENCE.md. "
                        + "Everything downstream measures screen positions, so stopping here.");
                }

                bool foreground = GameWindows.BringToFront(window.Handle);
                checks.Check(
                    "the window came to the front",
                    foreground || GameWindows.IsForeground(window.Handle),
                    "capture and input both need it; is something stealing focus?");

                if (options.Timeline)
                {
                    Console.WriteLine();
                    Console.WriteLine(
                        $"capturing every {options.TimelineIntervalMs}ms for {options.TimelineSeconds}s...");
                    Console.WriteLine(
                        "Startup is a sequence of distinct screens, not a fade to a still image,");
                    Console.WriteLine(
                        "so this records the whole thing rather than guessing when it ended.");
                    Console.WriteLine();

                    int frames = CaptureTimeline(window, artifacts, options);

                    Console.WriteLine();
                    Console.WriteLine($"Wrote {frames} frames to {artifacts}");
                    Console.WriteLine(
                        "Look through them and pick one frame per stage; those become the stage");
                    Console.WriteLine(
                        "references. The difference column marks where one stage becomes another.");
                    checks.Pass("captured a launch timeline");
                    return checks.Report();
                }


                // No settle step. Startup is a sequence of screens, and the legal
                // notice in the middle of it holds still for 25 seconds at a difference
                // of 0.0002 with detail below the blank floor - so waiting for the screen
                // to stop changing reports success there, less than halfway through, and
                // the menu is another 40 seconds away. Waiting for the menu ITSELF is both
                // simpler and the thing actually wanted.


                GameScreen.SaveCapture(window.Handle, Path.Combine(artifacts, "after-launch.png"));

                if (captureReference)
                {
                    File.Copy(Path.Combine(artifacts, "after-launch.png"), referencePath, overwrite: true);
                    Console.WriteLine();
                    Console.WriteLine($"Saved the main-menu reference to {referencePath}");
                    Console.WriteLine("Look at it and confirm it is the main menu before relying on it.");
                    return checks.Report();
                }

                if (!File.Exists(referencePath))
                {
                    throw new FileNotFoundException(
                        $"No main-menu reference at {referencePath}. Run capture-reference first.",
                        referencePath);
                }

                Console.WriteLine();
                Console.WriteLine("waiting for the main menu, about 50 seconds...");
                WaitResult atMenu = WaitForMenu(window, referencePath, artifacts, options);
                checks.Check("the main menu is on screen", atMenu.Succeeded,
                    // The threshold is the menu phase's own, so quoting --threshold here
                    // named a number the run never used.
                    $"closest difference {atMenu.Difference:N4}");

                if (!atMenu.Succeeded)
                {
                    Console.WriteLine("  (see after-launch.png - if that IS the menu, raise --threshold)");
                    return checks.Report();
                }

                Console.WriteLine();
                Console.WriteLine($"loading a save: {string.Join(" -> ", options.Keys)}");
                if (options.DryRun)
                {
                    Console.WriteLine("  (dry run, not sent)");
                    return checks.Report();
                }

                if (!File.Exists(logPath))
                {
                    throw new FileNotFoundException(
                        $"No BepInEx log at {logPath}; cannot verify that the save loaded.",
                        logPath);
                }

                if (LogContains(logPath, SaveLoadedLogText))
                {
                    throw new InvalidOperationException(
                        "The current BepInEx log already says a save loaded before the load key "
                        + "was sent, so it cannot prove this command loaded the staged save.");
                }

                GameWindows.BringToFront(window.Handle);
                GameSession.SendKeys(options.Keys, options.Verbose ? Log : (Action<string>?)null);

                Console.WriteLine();
                Console.WriteLine("waiting for the screen to leave the menu...");
                WaitResult left = GameSession.WaitUntilStopsMatching(
                    window,
                    referencePath,
                    options.Threshold,
                    TimeSpan.FromSeconds(options.TimeoutSeconds),
                    options.Verbose ? Log : (Action<string>?)null,
                    options.MenuRegion);

                // Leaving the menu is what can be checked. Whether the save then finished
                // loading cannot be, without a reference for the loaded screen: the game
                // world animates, so there is no settling to wait for, and the loading
                // screens animate too.
                checks.Check(
                    "the screen is no longer the main menu",
                    left.Succeeded,
                    $"difference {left.Difference:N4}; too low means the keys did nothing");

                Console.WriteLine();
                Console.WriteLine("waiting for the tracker to receive the loaded save...");
                bool loaded = WaitForLogText(
                    logPath,
                    SaveLoadedLogText,
                    TimeSpan.FromSeconds(options.TimeoutSeconds));
                string loadDetail = loaded
                    ? $"saw '{SaveLoadedLogText}' in {logPath}"
                    : $"'{SaveLoadedLogText}' did not appear in {logPath}";
                checks.Check(
                    "the staged save finished loading",
                    loaded,
                    loadDetail);

                // Whether the plugin could reach the Rust look-ahead from inside the game,
                // which is the one thing no test outside the game can answer - see
                // de-i5xj.5. Reported rather than checked while the look-ahead still runs
                // on the managed engine: a contributor who has not run `cargo build` has a
                // mod that works, and failing their run would be telling them off for
                // something that is not yet a requirement.
                NativeEngineReport native = NativeEngineReport.FromLog(logPath);
                Console.WriteLine($"  NOTE  {native}");

                GameScreen.SaveCapture(window.Handle, Path.Combine(artifacts, "after-load.png"));

                Console.WriteLine();
                Console.WriteLine($"screenshots are in {artifacts}");
                return checks.Report();
            }
            finally
            {
                // Before restoring, always: the game rewrites both the settings file and
                // the PlayerPrefs key as it exits, straight over anything put back first.
                if (!options.KeepOpen)
                {
                    Console.WriteLine();
                    Console.WriteLine("closing the game...");
                    if (process != null)
                    {
                        TryKill(process);
                    }
                    else
                    {
                        // Launched through Steam, which spawns the game itself and hands
                        // back no handle. It still has to be closed before the settings go
                        // back, or the game writes its own over them on the way out.
                        foreach (Process running in Process.GetProcessesByName(options.ProcessName))
                        {
                            TryKill(running);
                        }
                    }
                }

                if (options.KeepOpen)
                {
                    staged.Abandon();
                    Console.Error.WriteLine();
                    Console.Error.WriteLine(
                        "Left the game running, so NOTHING was restored. The player's profile is");
                    Console.Error.WriteLine(
                        $"  at {staged.ProfileMovedTo ?? "(there was none)"}");
                    Console.Error.WriteLine(
                        $"  and their PlayerPrefs at {staged.RegistryBackupPath}.");
                    Console.Error.WriteLine(
                        "Close the game and put both back before playing.");
                }
                else
                {
                    staged.Restore();
                }
            }
        }

        private static bool WaitForLogText(string path, string text, TimeSpan timeout)
        {
            DateTime deadline = DateTime.UtcNow + timeout;
            while (DateTime.UtcNow < deadline)
            {
                try
                {
                    if (LogContains(path, text))
                    {
                        return true;
                    }
                }
                catch (IOException)
                {
                    // BepInEx may be writing the file while it is sampled.
                }

                Thread.Sleep(500);
            }

            return false;
        }

        private static bool LogContains(string path, string text)
        {
            using var stream = new FileStream(
                path,
                FileMode.Open,
                FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete);
            using var reader = new StreamReader(stream);
            return reader.ReadToEnd().Contains(text);
        }

        /// <summary>
        /// Captures the whole of startup, frame by frame, rather than waiting for it.
        /// </summary>
        /// <remarks>
        /// Waiting for the screen to stop changing cannot find the main menu here. Startup
        /// runs through several distinct screens - a loading animation, a legal notice, an
        /// animated logo - and the menu itself is animated too, so "still" is never true.
        /// Worse, a LOOPING animation samples identically whenever two captures land at
        /// the same phase, so a settle can be declared in the middle of one. That is what
        /// produced a "settled" reference showing the loading screen.
        ///
        /// So this gathers evidence rather than judging it: every frame at full
        /// resolution, with the difference from the previous frame, which is large at a
        /// stage boundary and small within a stage.
        /// </remarks>
        private static int CaptureTimeline(GameWindow window, string artifacts, Options options)
        {
            string directory = Path.Combine(artifacts, "timeline");
            Directory.CreateDirectory(directory);

            var clock = Stopwatch.StartNew();
            TimeSpan deadline = TimeSpan.FromSeconds(options.TimelineSeconds);
            double[]? previous = null;
            int frame = 0;
            IntPtr lastHandle = window.Handle;
            int skipped = 0;

            while (clock.Elapsed < deadline)
            {
                // Re-find the window every frame rather than holding the handle. Unity
                // DESTROYS and recreates its window when the display mode changes, so a
                // handle captured at launch dies partway through startup - and until it
                // does, it reports the old window's size, which is how a switch to
                // windowed 1280x720 looked like a game stuck at 3840x1200.
                GameWindow? current = GameSession.FindGameWindow(options.ProcessName);
                if (current == null)
                {
                    // Expected briefly: between the old window going and the new arriving.
                    Console.WriteLine(
                        $"  {clock.Elapsed.TotalSeconds,6:N1}s  (no window - being recreated?)");
                    Thread.Sleep(options.TimelineIntervalMs);
                    continue;
                }

                if (current.Handle != lastHandle && lastHandle != IntPtr.Zero)
                {
                    Console.WriteLine(
                        $"  {clock.Elapsed.TotalSeconds,6:N1}s  *** the window was recreated "
                        + $"({current.Width}x{current.Height}) ***");
                    GameWindows.BringToFront(current.Handle);

                    // A new window is a new screen, not a continuation of the old one.
                    previous = null;
                }

                lastHandle = current.Handle;

                // CopyFromScreen reads SCREEN pixels, so anything covering the game is
                // captured instead of it. A frame taken while the game is not in front is
                // not a picture of the game, and saving it silently would put another
                // window's contents into the stage references.
                if (!GameWindows.IsForeground(current.Handle))
                {
                    GameWindows.BringToFront(current.Handle);
                    Thread.Sleep(200);
                }

                if (!GameWindows.IsForeground(current.Handle))
                {
                    skipped++;
                    Console.WriteLine(
                        $"  {clock.Elapsed.TotalSeconds,6:N1}s  SKIPPED - the game is not in front; "
                        + "this frame would show whatever is");
                    previous = null;
                    Thread.Sleep(options.TimelineIntervalMs);
                    continue;
                }

                using (Bitmap bitmap = GameScreen.Capture(current.Handle))
                {
                    frame++;
                    string name = $"timeline-{frame:D4}.png";
                    bitmap.Save(Path.Combine(directory, name), ImageFormat.Png);

                    double[] fingerprint = GameScreen.FingerprintOf(bitmap);
                    double detail = GameScreen.Detail(fingerprint);
                    string change = previous == null
                        ? "     -"
                        : GameScreen.Difference(previous, fingerprint).ToString("N4");

                    Console.WriteLine(
                        $"  {clock.Elapsed.TotalSeconds,6:N1}s  {name}  {bitmap.Width}x{bitmap.Height}"
                        + $"  {GameWindows.DescribeStyle(current.Handle),-10}"
                        + $"  difference {change}  detail {detail:N3}");

                    previous = fingerprint;
                }

                Thread.Sleep(options.TimelineIntervalMs);
            }

            if (skipped > 0)
            {
                Console.WriteLine();
                Console.WriteLine(
                    $"  {skipped} frame(s) skipped because the game was not in front. Leave the");
                Console.WriteLine(
                    "  game focused for the whole recording - a capture is of the screen, not");
                Console.WriteLine("  of the window, so clicking elsewhere photographs that instead.");
            }

            return frame;
        }

        private static void TryKill(Process process)
        {
            try
            {
                if (!process.HasExited)
                {
                    process.Kill();
                    process.WaitForExit(15000);
                }
            }
            catch (Exception error)
            {
                Console.Error.WriteLine($"WARNING: could not close the game: {error.Message}");
            }
        }

        private static void Log(string message)
        {
            Console.WriteLine($"    {message}");
        }

        private static string ResolveGame(Options options)
        {
            return GameInstall.FindGame(options.GamePath);
        }

        /// <summary>Turns an expanded sparse save into the archive the game reads.</summary>
        internal static string PackSave(string source, string artifacts)
        {
            if (!Directory.Exists(source))
            {
                return source;
            }

            string configuration =
                new DirectoryInfo(AppDomain.CurrentDomain.BaseDirectory).Parent?.Name ?? "Debug";
            string decoder = Path.Combine(
                RepoRoot(),
                ".build",
                "bin",
                "NtwtfDecode",
                configuration,
                "net10.0",
                "NtwtfDecode.dll");
            if (!File.Exists(decoder))
            {
                throw new FileNotFoundException(
                    "NtwtfDecode was not built with GameHarness.", decoder);
            }

            string packed = Path.Combine(artifacts, Path.GetFileName(source) + ".zip");
            Console.WriteLine($"packing:   {Path.GetFileName(source)}");
            using Process process = Process.Start(new ProcessStartInfo
            {
                FileName = "dotnet",
                Arguments = $"\"{decoder}\" --pack \"{source}\" -o \"{packed}\"",
                RedirectStandardOutput = true,
                UseShellExecute = false,
            }) ?? throw new InvalidOperationException("Could not start NtwtfDecode.");
            string actualPacked = process.StandardOutput.ReadToEnd().Trim();
            process.WaitForExit();
            if (process.ExitCode != 0)
            {
                throw new InvalidOperationException(
                    $"NtwtfDecode could not pack '{source}' (exit {process.ExitCode}).");
            }
            if (!File.Exists(actualPacked))
            {
                throw new FileNotFoundException(
                    "NtwtfDecode did not report a packed save it created.", actualPacked);
            }
            return actualPacked;
        }

        /// <summary>The repository root, which is where the test data lives.</summary>
        private static string RepoRoot()
        {
            return GameInstall.RepoRoot();
        }

        /// <summary>Counts what passed and what did not, and reports at the end.</summary>
        private sealed class Checks
        {
            private readonly List<string> _failures = new List<string>();

            public void Pass(string label)
            {
                Console.WriteLine($"  PASS  {label}");
            }

            public void Check(string label, bool condition, string detail = "")
            {
                string suffix = string.IsNullOrEmpty(detail) ? string.Empty : $" ({detail})";
                if (condition)
                {
                    Console.WriteLine($"  PASS  {label}{suffix}");
                }
                else
                {
                    Console.WriteLine($"  FAIL  {label}{suffix}");
                    _failures.Add(label);
                }
            }

            public int Report()
            {
                Console.WriteLine();
                if (_failures.Count == 0)
                {
                    Console.WriteLine("ALL PASS");
                    return 0;
                }

                Console.WriteLine($"FAILURES: {string.Join(", ", _failures)}");
                return 1;
            }
        }

        /// <summary>The parsed command line.</summary>
        private sealed class Options
        {
            public string Verb { get; private set; } = string.Empty;

            public string? GamePath { get; private set; }

            public string ProcessName { get; private set; } = DefaultProcessName;

            public string? Artifacts { get; private set; }

            public string? TestSettings { get; private set; }

            public string[] Keys { get; private set; } = { "Enter" };

            public double Threshold { get; private set; } = 0.05;

            /// <summary>Seconds to wait for loading, which is the slow part.</summary>
            public int TimeoutSeconds { get; private set; } = 300;

            /// <summary>Seconds to wait for the game window to appear at all.</summary>
            /// <remarks>
            /// Much shorter than the loading timeout, because it answers a different
            /// question. A window either appears in the first few seconds or something is
            /// wrong - the wrong executable, a Steam prompt, a crash on startup - and
            /// waiting five minutes to be told so just makes the failure slow.
            /// </remarks>
            public int WindowTimeoutSeconds { get; private set; } = 30;

            public bool DryRun { get; private set; }

            public bool KeepOpen { get; private set; }

            public bool Verbose { get; private set; }

            /// <summary>Do not keep a log of this run under testing/logs.</summary>
            public bool NoLog { get; private set; }

            /// <summary>Capture every frame of startup instead of waiting for a settle.</summary>
            public bool Timeline { get; private set; }

            /// <summary>Launch through the Steam client rather than the executable.</summary>
            public bool ViaSteam { get; private set; }

            /// <summary>Leave Unity's registry screen prefs untouched.</summary>
            public bool SkipScreenPrefs { get; private set; }

            /// <summary>
            /// Process names that unlock may ask to close, matched as a prefix.
            /// </summary>
            /// <remarks>
            /// Explorer is not here and must not be: its window is moved off the folder
            /// instead, which costs nothing. This list is for things that hold a folder
            /// with no way to let go short of closing - an editor with it open.
            /// </remarks>
            public string[] Askable { get; private set; } = { "Code" };

            /// <summary>Seconds to wait for an asked process to go before leaving it.</summary>
            /// <remarks>
            /// It is asked, never killed. A process with unsaved work answers WM_CLOSE with
            /// a save prompt and stays open, which is correct; when the deadline passes it
            /// is left running and reported.
            ///
            /// 30 seconds because the prompt is for a PERSON: long enough to notice an
            /// editor asking about unsaved work and answer it, rather than only long
            /// enough for a process with nothing to save.
            /// </remarks>
            public int CloseDeadlineSeconds { get; private set; } = 30;

            /// <summary>
            /// Ask editors holding the profile to close, when a move fails.
            /// </summary>
            /// <remarks>
            /// Off by default because it can lose unsaved work. Moving an Explorer window
            /// off the folder is free and always happens; closing somebody's editor is
            /// not, and has to be asked for.
            /// </remarks>
            public bool CloseHolders { get; private set; }

            /// <summary>Write the chosen frame out as the main-menu reference.</summary>
            public bool SaveReference { get; private set; }

            /// <summary>An extra region to try, as x,y,width,height.</summary>
            public Rectangle? Region { get; private set; }

            /// <summary>A single save to stage, so Continue has only one thing to load.</summary>
            public string? SaveFile { get; private set; }

            private readonly List<string> _suiteNames = new List<string>();

            /// <summary>Which look-ahead suites to run, or none for every suite.</summary>
            public IReadOnlyList<string> SuiteNames => _suiteNames;

            private readonly List<string> _scenarioNames = new List<string>();

            /// <summary>Which scenarios to keep, or none to keep every one.</summary>
            public IReadOnlyList<string> ScenarioNames => _scenarioNames;

            /// <summary>
            /// A memory budget in megabytes to run every suite at, overriding what they
            /// ask for; null to leave them alone.
            /// </summary>
            /// <remarks>
            /// For asking how the cost behaves as the limit moves, which is a question
            /// about the conversations rather than about any suite. Editing the suite to
            /// ask it makes the answer unrepeatable and the tree dirty.
            ///
            /// Was --state-budget until de-7z0f, which took the state budget away: memory
            /// and time are the two a player can set, so they are the two worth sweeping.
            /// </remarks>
            public int? MemoryBudgetMb { get; private set; }

            /// <summary>A time budget to run every suite at, or null to leave them.</summary>
            public int? TimeBudgetMs { get; private set; }

            /// <summary>
            /// The part of the menu to match on: the option list, which does not animate.
            /// </summary>
            /// <remarks>
            /// The menu is a static list of options beside a painting that never stops
            /// moving. Measured over a recorded startup, the whole frame varies by 0.0503
            /// as it animates against 0.1507 to the nearest other screen - three times
            /// separation - while this region varies by 0.0030 against 0.1161, nearly
            /// forty times.
            ///
            /// The left-hand panel as a whole scored better still, at sixty-five times,
            /// and is deliberately not used: most of it is a storefront advert, which
            /// scores well today and changes when the promo does. This region is only the
            /// six menu options.
            /// </remarks>
            public Rectangle? MenuRegion { get; private set; } = new Rectangle(170, 115, 230, 290);

            /// <summary>Screen prefs to write, when deliberately disagreeing with the file.</summary>
            public DisplaySettings? RegistryScreen { get; private set; }

            /// <summary>The Steam app id, for launching through Steam.</summary>
            public string AppId { get; private set; } = "632470";

            /// <summary>How often to capture in timeline mode, in milliseconds.</summary>
            public int TimelineIntervalMs { get; private set; } = 1000;

            /// <summary>How long to record in timeline mode, in seconds.</summary>
            /// <remarks>
            /// Separate from --timeout, which is how long to wait for the window to appear
            /// at all. Recording is bounded by how long startup takes, not by how patient
            /// the launch wait is, and at one frame a second 60s is already 60 full-size
            /// PNGs to look through.
            /// </remarks>
            public int TimelineSeconds { get; private set; } = 60;

            public static Options? Parse(string[] args)
            {
                if (args.Length == 0)
                {
                    return null;
                }

                var options = new Options { Verb = args[0] };
                for (int i = 1; i < args.Length; i++)
                {
                    string flag = args[i];
                    string? Next() => i + 1 < args.Length ? args[++i] : null;

                    switch (flag)
                    {
                        case "--game": options.GamePath = Next(); break;
                        case "--process": options.ProcessName = Next() ?? DefaultProcessName; break;
                        case "--artifacts": options.Artifacts = Next(); break;
                        case "--settings": options.TestSettings = Next(); break;
                        case "--keys":
                            options.Keys = (Next() ?? string.Empty)
                                .Split(new[] { ',' }, StringSplitOptions.RemoveEmptyEntries)
                                .Select(key => key.Trim())
                                .ToArray();
                            break;
                        case "--threshold":
                            options.Threshold = double.Parse(
                                Next() ?? "0.05", System.Globalization.CultureInfo.InvariantCulture);
                            break;
                        case "--timeout":
                            options.TimeoutSeconds = int.Parse(
                                Next() ?? "300", System.Globalization.CultureInfo.InvariantCulture);
                            break;
                        case "--window-timeout":
                            options.WindowTimeoutSeconds = int.Parse(
                                Next() ?? "30", System.Globalization.CultureInfo.InvariantCulture);
                            break;
                        case "--dry-run": options.DryRun = true; break;
                        case "--no-log": options.NoLog = true; break;
                        case "--keep-open": options.KeepOpen = true; break;
                        case "--via-steam": options.ViaSteam = true; break;
                        case "--no-screen-prefs": options.SkipScreenPrefs = true; break;
                        case "--close-holders": options.CloseHolders = true; break;
                        case "--askable":
                            options.Askable = (Next() ?? "Code").Split(',');
                            break;
                        case "--close-deadline":
                            options.CloseDeadlineSeconds = int.Parse(
                                Next() ?? "10", CultureInfo.InvariantCulture);
                            break;
                        case "--save-reference": options.SaveReference = true; break;
                        case "--whole-frame": options.MenuRegion = null; break;
                        case "--save": options.SaveFile = Next(); break;
                        case "--suite":
                        {
                            string suite = Next() ?? throw new ArgumentException(
                                "--suite needs at least one suite name.");
                            string[] names = suite.Split(
                                new[] { ',' }, StringSplitOptions.RemoveEmptyEntries);
                            if (names.Length == 0)
                            {
                                throw new ArgumentException(
                                    "--suite needs at least one suite name.");
                            }

                            options._suiteNames.AddRange(names.Select(name => name.Trim()));
                            break;
                        }
                        case "--scenario":
                        {
                            string scenario = Next() ?? throw new ArgumentException(
                                "--scenario needs at least one scenario name.");
                            string[] names = scenario.Split(
                                new[] { ',' }, StringSplitOptions.RemoveEmptyEntries);
                            if (names.Length == 0)
                            {
                                throw new ArgumentException(
                                    "--scenario needs at least one scenario name.");
                            }

                            options._scenarioNames.AddRange(names.Select(name => name.Trim()));
                            break;
                        }
                        case "--memory-budget-mb":
                            options.MemoryBudgetMb = int.Parse(
                                Next() ?? throw new ArgumentException(
                                    "--memory-budget-mb needs a number."),
                                CultureInfo.InvariantCulture);
                            break;
                        case "--time-budget-ms":
                            options.TimeBudgetMs = int.Parse(
                                Next() ?? throw new ArgumentException(
                                    "--time-budget-ms needs a number."),
                                CultureInfo.InvariantCulture);
                            break;
                        case "--region":
                        {
                            string[] parts = (Next() ?? string.Empty).Split(',');
                            if (parts.Length != 4)
                            {
                                throw new ArgumentException("--region wants x,y,width,height.");
                            }

                            options.Region = new Rectangle(
                                int.Parse(parts[0], CultureInfo.InvariantCulture),
                                int.Parse(parts[1], CultureInfo.InvariantCulture),
                                int.Parse(parts[2], CultureInfo.InvariantCulture),
                                int.Parse(parts[3], CultureInfo.InvariantCulture));
                            break;
                        }
                        case "--registry-screen":
                        {
                            string spec = Next() ?? string.Empty;
                            string[] halves = spec.Split(':');
                            string[] size = halves[0].Split('x');
                            if (halves.Length != 2 || size.Length != 2)
                            {
                                throw new ArgumentException(
                                    $"--registry-screen wants WIDTHxHEIGHT:UNITYMODE, got '{spec}'. "
                                    + "Unity modes: 0 exclusive, 1 borderless, 2 maximised, 3 windowed.");
                            }

                            options.RegistryScreen = new DisplaySettings(
                                int.Parse(size[0], CultureInfo.InvariantCulture),
                                int.Parse(size[1], CultureInfo.InvariantCulture),
                                int.Parse(halves[1], CultureInfo.InvariantCulture) == UnityPlayerPrefs.Windowed
                                    ? DisplaySettings.WindowedMode
                                    : 0);
                            break;
                        }

                        case "--app-id": options.AppId = Next() ?? "632470"; break;
                        case "--timeline": options.Timeline = true; break;
                        case "--timeline-seconds":
                            options.TimelineSeconds = int.Parse(
                                Next() ?? "60", CultureInfo.InvariantCulture);
                            break;
                        case "--timeline-interval":
                            options.TimelineIntervalMs = int.Parse(
                                Next() ?? "1000", CultureInfo.InvariantCulture);
                            break;
                        case "--verbose": options.Verbose = true; break;
                        default:
                            Console.Error.WriteLine($"Unknown option '{flag}'.");
                            return null;
                    }
                }

                return options;
            }
        }
    }
}
