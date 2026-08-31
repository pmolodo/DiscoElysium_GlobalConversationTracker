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

        private static readonly string[] DefaultGamePaths =
        {
            @"C:\apps (x86)\games\steam\steamapps\common\Disco Elysium\disco.exe",
            @"C:\Program Files (x86)\Steam\steamapps\common\Disco Elysium\disco.exe",
        };

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

                        int moved = ExplorerWindows.NavigateAwayFrom(
                            target, message => Console.WriteLine($"  {message}"));
                        Console.WriteLine($"moved {moved} Explorer window(s) off it");

                        // Explorer first because it is free: the window survives, only
                        // pointed elsewhere. Only what is still holding the folder after
                        // that is worth asking to close.
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

                    case "windows":
                        return ListWindows(options);
                    case "keys":
                        Console.WriteLine(string.Join(", ", GameKeyboard.KeyNames));
                        return 0;
                    case "capture-reference":
                        return RunSession(options, captureReference: true);
                    case "load-save":
                        return RunSession(options, captureReference: false);
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
  analyse-timeline    Read a recorded timeline back and report which frames
                      identify which screens, with a measured threshold. Add
                      --save-reference to write the chosen frame as main-menu.png.
  locks               Report what is holding the game's profile folder open,
                      which is what blocks staging. --artifacts asks about
                      another path instead.
  unlock              Move any Explorer window off the profile folder, then ask
                      whatever still holds it to close. Explorer windows are
                      navigated to the parent, not closed. Nothing is ever
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
  --save <file>       The .ntwtf.zip to stage as the ONLY save. Defaults to
                      testing/save_template.ntwtf.zip, so Continue has exactly
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
  --no-close-holders  When the profile cannot be moved, do NOT ask editors
                      holding it to close. Asking is on by default: it sends the
                      same request the window's X does, so an editor with unsaved
                      changes puts up a save prompt and stays open, and nothing
                      is ever killed. Turn it off for an unattended run, where an
                      unanswered prompt would be left on screen.
  --askable a,b       Process names unlock may ask to close (default: Code).
                      Explorer is never on this list; its windows are moved off
                      the folder instead, which costs nothing.
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
  --verbose           Report every sample the waits take.");
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
        /// The commonest reason staging fails is an Explorer window showing the folder,
        /// which can be moved off it for nothing - the window survives, pointed at the
        /// parent. That is only done when a move has ALREADY failed, so a run that would
        /// have worked never disturbs anybody's windows.
        ///
        /// Asking an editor to close is not done here unless --close-holders says so. It
        /// can lose unsaved work, which is too high a price to pay silently for a test.
        /// </remarks>
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
                int moved = ExplorerWindows.NavigateAwayFrom(
                    profile, message => Console.WriteLine($"  {message}"));

                if (options.CloseHolders)
                {
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
                        }
                    }
                }

                if (moved == 0 && !options.CloseHolders)
                {
                    // Nothing was changed, so retrying would fail the same way. Report what
                    // is holding it - which is the slow part, and worth it here - rather
                    // than failing twice for the same reason.
                    Console.WriteLine();
                    Console.WriteLine(FileLocks.Describe(profile));
                    Console.WriteLine();
                    Console.WriteLine(
                        "Nothing could be moved off it, and --no-close-holders means nothing "
                        + "was asked to close. Close it yourself, or drop that flag.");
                    throw;
                }

                Console.WriteLine("  trying again...");
                return GameProfile.Backup(backupPath);
            }
        }

        private static int RunSession(Options options, bool captureReference)
        {
            string game = ResolveGame(options);
            string artifacts = options.Artifacts ?? Path.Combine(RepoRoot(), @".build\automation");
            string testSettings = options.TestSettings ?? Path.Combine(RepoRoot(), @"testing\Settings.json");
            string referencePath = Path.Combine(artifacts, "main-menu.png");

            Directory.CreateDirectory(artifacts);

            Console.WriteLine($"game:      {game}");
            Console.WriteLine($"artifacts: {artifacts}");
            Console.WriteLine($"settings:  {testSettings}");
            if (options.DryRun)
            {
                Console.WriteLine("mode:      dry run, no keys will be sent");
            }

            if (Process.GetProcessesByName(options.ProcessName).Length > 0)
            {
                throw new InvalidOperationException(
                    $"'{options.ProcessName}' is already running. Close it first: two instances make "
                    + "the capture ambiguous.");
            }

            var checks = new Checks();
            string stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss");
            string profileBackupPath = GameProfile.DefaultBackupPath(stamp);
            string registryBackupPath = Path.Combine(Path.GetTempPath(), $"disco-prefs-{stamp}.reg");

            // The registry is backed up separately because the profile move cannot reach
            // it: PlayerPrefs live under HKCU, not in the folder.
            GameSettings.BackupRegistry(registryBackupPath);
            Console.WriteLine($"prefs:     {registryBackupPath}");

            ProfileBackup profileBackup = BackupProfile(profileBackupPath, options);
            Console.WriteLine(
                profileBackup.MovedTo == null
                    ? "profile:   none found; a fresh one will be built"
                    : $"profile:   {profileBackup.EntryCount} entries moved to {profileBackup.MovedTo}");

            DisplaySettings requested = GameSettings.ReadDisplay(testSettings);

            Process? process = null;
            try
            {
                string saveTemplate = options.SaveFile
                    ?? Path.Combine(RepoRoot(), "testing", "save_template.ntwtf.zip");
                GameProfile.Stage(testSettings, saveTemplate);
                Console.WriteLine($"staged:    {Path.GetFileName(testSettings)}");
                Console.WriteLine(
                    $"           {Path.GetFileName(saveTemplate)} as the only save, so "
                    + "Continue can only load that one");

                // The file does not size the window on its own; Unity does, from the
                // registry, before the game runs. See testing/SETTINGS-PRECEDENCE.md.
                if (options.RegistryScreen != null)
                {
                    GameSettings.InstallScreenPrefs(options.RegistryScreen);
                    Console.WriteLine(
                        $"screen:    asked Unity for {options.RegistryScreen} "
                        + $"(settings file says {requested})");
                }
                else if (options.SkipScreenPrefs)
                {
                    Console.WriteLine(
                        "screen:    left Unity's registry alone; only the settings file was staged");
                }
                else
                {
                    GameSettings.InstallScreenPrefs(requested);
                    Console.WriteLine($"screen:    asked Unity for {requested}");
                }

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
                Console.WriteLine("waiting for the main menu...");
                Console.WriteLine(
                    "  Startup runs through several screens and takes about 50 seconds; the");
                Console.WriteLine(
                    "  difference falls as it goes and drops under the threshold at the menu.");
                WaitResult atMenu = GameSession.WaitUntilMatches(
                    window, referencePath, options.Threshold,
                    TimeSpan.FromSeconds(options.TimeoutSeconds),
                    options.Verbose ? Log : (Action<string>?)null,
                    options.MenuRegion);
                checks.Check("the main menu is on screen", atMenu.Succeeded,
                    $"closest difference {atMenu.Difference:N4}, threshold {options.Threshold:N4}");

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

                GameScreen.SaveCapture(window.Handle, Path.Combine(artifacts, "after-keys.png"));

                Console.WriteLine();
                Console.WriteLine(
                    "Look at after-keys.png. That the screen changed is checked; that the save");
                Console.WriteLine(
                    "LOADED is not, and cannot be until there is a reference for the loaded");
                Console.WriteLine("screen to compare against.");

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
                    Console.Error.WriteLine();
                    Console.Error.WriteLine(
                        "Left the game running, so NOTHING was restored. The player's profile is");
                    Console.Error.WriteLine($"  at {profileBackup.MovedTo ?? "(there was none)"}");
                    Console.Error.WriteLine($"  and their PlayerPrefs at {registryBackupPath}.");
                    Console.Error.WriteLine(
                        "Close the game and put both back before playing.");
                }
                else
                {
                // The profile first: a staged one is the worst thing to leave behind,
                // because it is Steam-Cloud-synced and a later launch can push it upward.
                // Each restore gets its own try, so a failure in one does not skip the
                // other - they are independent, and leaving either is its own problem.
                try
                {
                    GameProfile.Restore(profileBackup);
                    Console.WriteLine($"restored the profile ({profileBackup.EntryCount} entries)");
                }
                catch (Exception error)
                {
                    Console.Error.WriteLine();
                    Console.Error.WriteLine($"PROFILE NOT RESTORED: {error.Message}");
                    Console.Error.WriteLine(
                        "Put it back by hand before launching the game again; a staged profile");
                    Console.Error.WriteLine(
                        "left in place can be synced to Steam Cloud by the next launch.");
                }

                try
                {
                    GameSettings.RestoreRegistry(registryBackupPath);
                    File.Delete(registryBackupPath);
                }
                catch (Exception error)
                {
                    Console.Error.WriteLine($"WARNING: could not restore PlayerPrefs: {error.Message}");
                    Console.Error.WriteLine($"WARNING: the export is at {registryBackupPath}");
                }
                }
            }
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
            if (options.GamePath != null)
            {
                if (!File.Exists(options.GamePath))
                {
                    throw new FileNotFoundException($"No game at {options.GamePath}.", options.GamePath);
                }

                return options.GamePath;
            }

            foreach (string candidate in DefaultGamePaths)
            {
                if (File.Exists(candidate))
                {
                    return candidate;
                }
            }

            throw new FileNotFoundException("Could not find disco.exe. Pass --game.");
        }

        /// <summary>
        /// The repository root, found by walking up from the assembly to the directory
        /// holding the solution.
        /// </summary>
        private static string RepoRoot()
        {
            var directory = new DirectoryInfo(AppDomain.CurrentDomain.BaseDirectory);
            while (directory != null)
            {
                if (File.Exists(Path.Combine(directory.FullName, "GlobalConversationTracker.slnx")))
                {
                    return directory.FullName;
                }

                directory = directory.Parent;
            }

            throw new InvalidOperationException(
                "Could not find the repository root; pass --artifacts and --settings.");
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
            /// On, because the ask is genuinely polite and the failure mode is mild: only
            /// names on the askable list are asked, the request is the one the window's X
            /// sends, an editor with unsaved changes answers with a save prompt and stays
            /// open, and after the deadline it is left running. Nothing is ever killed, so
            /// the worst outcome is a prompt appearing and a test run stopping - which is
            /// what would have happened anyway.
            ///
            /// --no-close-holders turns it off, for an unattended run where an unanswered
            /// save prompt would rather not be left on screen.
            /// </remarks>
            public bool CloseHolders { get; private set; } = true;

            /// <summary>Write the chosen frame out as the main-menu reference.</summary>
            public bool SaveReference { get; private set; }

            /// <summary>An extra region to try, as x,y,width,height.</summary>
            public Rectangle? Region { get; private set; }

            /// <summary>A single save to stage, so Continue has only one thing to load.</summary>
            public string? SaveFile { get; private set; }

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
            /// and is deliberately not used: it contains a storefront advert and the save
            /// slots with their dates, so it scores well today and breaks when the promo
            /// changes or a game is saved. This region is only the six menu options.
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
                        case "--keep-open": options.KeepOpen = true; break;
                        case "--via-steam": options.ViaSteam = true; break;
                        case "--no-screen-prefs": options.SkipScreenPrefs = true; break;
                        case "--close-holders": options.CloseHolders = true; break;
                        case "--no-close-holders": options.CloseHolders = false; break;
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
