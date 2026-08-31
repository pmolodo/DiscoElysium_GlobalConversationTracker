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
  windows             List every window the game's process owns, with its class.
                      What to run when the wrong window is being captured.
  keys                List the key names the harness accepts.

Options:
  --game <path>       disco.exe. Found in the usual Steam libraries if omitted.
  --process <name>    Process name without .exe (default: disco).
  --artifacts <dir>   Where screenshots go (default: .build/automation).
  --settings <file>   Test settings to install (default: testing/Settings.json).
  --keys a,b,c        The key sequence for load-save (default: Down,Enter,Enter).
  --threshold <n>     How close the menu match must be (default: 0.05).
  --timeout <n>       Seconds to wait for loading, the slow part (default: 300).
  --window-timeout    Seconds to wait for the game window to appear (default: 30).
                      A window either shows up quickly or something is wrong.
  --dry-run           Do everything except press keys.
  --keep-open         Leave the game running afterwards.
  --timeline          Capture every frame of startup instead of waiting for the
                      screen to settle. Startup runs through several animated
                      screens, so stopped-changing never becomes true - and a
                      looping animation can sample identically twice and fake it.
  --timeline-interval How often to capture in timeline mode, in ms (default 1000).
  --timeline-seconds  How long to record in timeline mode (default 60). Separate
                      from --timeout, which is the wait for the window to appear.
  --verbose           Report every sample the waits take.");
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
            string backupPath = Path.Combine(
                Path.GetTempPath(), $"disco-settings-{DateTime.Now:yyyyMMdd-HHmmss}.json");

            SettingsBackup backup = GameSettings.Backup(backupPath);
            Console.WriteLine($"backup:    {backup.SettingsPath}");

            Process? process = null;
            try
            {
                GameSettings.Install(testSettings);

                Console.WriteLine();
                Console.WriteLine("launching...");
                process = Process.Start(game);

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

                // The window OPENS at whatever Unity's own PlayerPrefs say - for this game
                // a native-resolution borderless window - and only switches to what the
                // settings file asks for once the game's startup code runs, during the
                // legal notice. So this is a note, not a verdict.
                Console.WriteLine(
                    $"  opened at: {window.Width}x{window.Height} (the game applies its own "
                    + "resolution during startup)");

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

                // Only meaningful when the window did NOT open at the wanted size. If it
                // did, waiting returns on the first poll and proves nothing about whether
                // the game applied its settings - the two look identical from out here.
                bool canObserveSwitch = GameSession.CanObserveResolutionSwitch(window, wanted);

                Console.WriteLine();
                if (canObserveSwitch)
                {
                    Console.WriteLine($"waiting for the game to switch to {wanted}...");
                }
                else
                {
                    Console.WriteLine(
                        $"the window already opened at {wanted}, so there is no switch to see.");
                    Console.WriteLine(
                        "  This run can only confirm the size is right, NOT that the settings");
                    Console.WriteLine(
                        "  file was applied - a game ignoring it entirely would look the same.");
                }

                GameWindow? resized = GameSession.WaitForResolution(
                    options.ProcessName,
                    wanted.Width,
                    wanted.Height,
                    TimeSpan.FromSeconds(options.TimeoutSeconds),
                    options.Verbose ? Log : (Action<string>?)null);

                checks.Check(
                    canObserveSwitch
                        ? $"the game switched to the requested {wanted}"
                        : $"the game is running at the requested {wanted} (it opened there; "
                            + "no switch was observable)",
                    resized != null,
                    $"still {window.Width}x{window.Height} at the deadline");

                if (resized == null)
                {
                    throw new InvalidOperationException(
                        $"The game never switched to {wanted.Width}x{wanted.Height}. It opens at "
                        + "the size Unity's registry PlayerPrefs describe and then applies its own "
                        + "settings file, so this means the settings file was not applied - not "
                        + "merely that the window started large. Capturing now would save a "
                        + "reference at the wrong resolution, so stopping instead.");
                }

                window = resized;

                Console.WriteLine();
                Console.WriteLine("waiting for the screen to render and settle...");
                WaitResult settled = GameSession.WaitUntilStill(
                    window,
                    TimeSpan.FromSeconds(options.TimeoutSeconds),
                    progress: options.Verbose ? Log : (Action<string>?)null);
                checks.Check("the screen rendered and stopped changing", settled.Succeeded, settled.ToString());

                if (!settled.SawMotion)
                {
                    Console.WriteLine("  (never saw it move - it may still have been loading)");
                }

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
                Console.WriteLine("checking we are at the main menu...");
                WaitResult atMenu = GameSession.WaitUntilMatches(
                    window, referencePath, options.Threshold, TimeSpan.FromSeconds(30),
                    options.Verbose ? Log : (Action<string>?)null);
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
                Console.WriteLine("waiting for the load to finish...");
                WaitResult loaded = GameSession.WaitUntilStill(
                    window,
                    TimeSpan.FromSeconds(options.TimeoutSeconds),
                    stableSamples: 6,
                    progress: options.Verbose ? Log : (Action<string>?)null);
                checks.Check("the screen settled again after loading", loaded.Succeeded, loaded.ToString());

                GameScreen.SaveCapture(window.Handle, Path.Combine(artifacts, "after-load.png"));

                // Without this, a run where the keys did nothing looks exactly like a
                // successful one: a menu sitting still is also "settled".
                WaitResult stillMenu = GameSession.WaitUntilMatches(
                    window, referencePath, options.Threshold, TimeSpan.FromSeconds(2));
                checks.Check(
                    "the screen is no longer the main menu",
                    !stillMenu.Succeeded,
                    $"difference from the menu {stillMenu.Difference:N4}; too low means the keys did nothing");

                Console.WriteLine();
                Console.WriteLine($"screenshots are in {artifacts}");
                return checks.Report();
            }
            finally
            {
                // Before restoring, always: the game rewrites both the settings file and
                // the PlayerPrefs key as it exits, straight over anything put back first.
                if (process != null && !options.KeepOpen)
                {
                    Console.WriteLine();
                    Console.WriteLine("closing the game...");
                    TryKill(process);
                }

                try
                {
                    if (options.KeepOpen)
                    {
                        Console.WriteLine(
                            $"Left the game running; settings NOT restored. Backup: {backup.SettingsPath}");
                    }
                    else
                    {
                        GameSettings.Restore(backup);
                        File.Delete(backup.SettingsPath);
                        if (backup.RegistryPath != null)
                        {
                            File.Delete(backup.RegistryPath);
                        }
                    }
                }
                catch (Exception error)
                {
                    Console.Error.WriteLine($"WARNING: could not restore settings: {error.Message}");
                    Console.Error.WriteLine($"WARNING: the backup is at {backup.SettingsPath}");
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
                        + $"  difference {change}  detail {detail:N3}");

                    previous = fingerprint;
                }

                Thread.Sleep(options.TimelineIntervalMs);
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

            public string[] Keys { get; private set; } = { "Down", "Enter", "Enter" };

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
