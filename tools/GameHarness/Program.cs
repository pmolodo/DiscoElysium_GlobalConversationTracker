// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
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
  --timeout <n>       Seconds to wait for launch and for loading (default: 300).
  --dry-run           Do everything except press keys.
  --keep-open         Leave the game running afterwards.
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
                    options.ProcessName, TimeSpan.FromSeconds(options.TimeoutSeconds));
                Console.WriteLine($"  window: {window.ClassName} '{window.Title}'");
                checks.Pass("the game window appeared");

                bool foreground = GameWindows.BringToFront(window.Handle);
                checks.Check(
                    "the window came to the front",
                    foreground || GameWindows.IsForeground(window.Handle),
                    "capture and input both need it; is something stealing focus?");

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

                // Only now. ResolutionSwitcher applies the saved resolution during
                // startup, so measuring earlier reads whatever size the window opened at.
                var rect = GameSession.FindGameWindow(options.ProcessName);
                if (rect != null)
                {
                    Console.WriteLine($"  client area: {rect.Width}x{rect.Height}");
                    checks.Check(
                        "the test settings resolution took effect",
                        rect.Width == 1280 && rect.Height == 720,
                        $"got {rect.Width}x{rect.Height}; a display without 1280x720 snaps to its maximum");
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

            public int TimeoutSeconds { get; private set; } = 300;

            public bool DryRun { get; private set; }

            public bool KeepOpen { get; private set; }

            public bool Verbose { get; private set; }

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
                        case "--dry-run": options.DryRun = true; break;
                        case "--keep-open": options.KeepOpen = true; break;
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
