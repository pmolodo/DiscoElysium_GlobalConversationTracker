// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Automation
{
    /// <summary>The resolution and display mode a settings file asks for.</summary>
    public sealed class DisplaySettings
    {
        /// <summary>The display-mode index meaning windowed.</summary>
        /// <remarks>
        /// Measured from the game's own writeback, NOT from Unity's FullScreenMode enum,
        /// which numbers its modes differently (there 1 is borderless fullscreen, which
        /// would ignore the requested resolution entirely). Setting the game to windowed
        /// 1280x720 through its own options menu produced DISPLAY MODE 1.
        /// </remarks>
        public const int WindowedMode = 1;

        /// <summary>Creates a value.</summary>
        /// <param name="width">Requested width.</param>
        /// <param name="height">Requested height.</param>
        /// <param name="displayMode">The game's display-mode index.</param>
        public DisplaySettings(int width, int height, int displayMode)
        {
            Width = width;
            Height = height;
            DisplayMode = displayMode;
        }

        /// <summary>Requested width.</summary>
        public int Width { get; }

        /// <summary>Requested height.</summary>
        public int Height { get; }

        /// <summary>The game's display-mode index.</summary>
        public int DisplayMode { get; }

        /// <summary>Whether this asks for a window rather than a fullscreen surface.</summary>
        public bool IsWindowed => DisplayMode == WindowedMode;

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{Width}x{Height} {(IsWindowed ? "windowed" : $"display mode {DisplayMode}")}";
        }
    }

    /// <summary>What a settings swap saved, so it can be put back.</summary>
    public sealed class SettingsBackup
    {
        /// <summary>Creates a backup record.</summary>
        /// <param name="settingsPath">Where the settings file was copied to.</param>
        /// <param name="registryPath">Where the PlayerPrefs export went, or null.</param>
        public SettingsBackup(string settingsPath, string? registryPath)
        {
            SettingsPath = settingsPath;
            RegistryPath = registryPath;
        }

        /// <summary>The copy of the player's settings file.</summary>
        public string SettingsPath { get; }

        /// <summary>The PlayerPrefs export, or null if the registry was skipped.</summary>
        public string? RegistryPath { get; }
    }

    /// <summary>
    /// Swaps the game's settings for a fixed test set, and puts them back.
    /// </summary>
    /// <remarks>
    /// <para>Disco Elysium keeps its settings in its own file, not in Unity's
    /// PlayerPrefs: <c>persistentDataPath/Settings/Settings.json</c>, read and written by
    /// SettingsPersister through JsonUtil. That file is what decides the game's
    /// behaviour.</para>
    ///
    /// <para>The PlayerPrefs registry key is a downstream cache. Unity opens the window
    /// at the registry's resolution before any game code runs; then ResolutionSwitcher
    /// reads the saved resolution out of the JSON, applies it, and Unity writes the
    /// result back. Observed directly: with the two disagreeing before launch, the game
    /// used the JSON and the registry afterwards matched it. It is backed up anyway,
    /// because a run changes it as a side effect and leaving it at 1280x720 would open
    /// the next launch small before the game corrected itself.</para>
    ///
    /// <para>A whole file is installed rather than individual settings edited, so every
    /// run starts from exactly the same state whatever the player last chose.</para>
    /// </remarks>
    public static class GameSettings
    {
        /// <summary>The registry key holding Unity's PlayerPrefs for this game.</summary>
        public const string RegistryKey = @"HKCU\Software\ZAUM Studio\Disco Elysium";

        /// <summary>The settings file the game reads and writes.</summary>
        /// <remarks>
        /// <c>DISCO_ELYSIUM_GCT_SETTINGS_FILE</c> redirects it, which is how the tests run
        /// against a scratch copy instead of a real installation.
        /// </remarks>
        public static string SettingsPath
        {
            get
            {
                string? redirect = Environment.GetEnvironmentVariable(
                    "DISCO_ELYSIUM_GCT_SETTINGS_FILE");
                if (!string.IsNullOrWhiteSpace(redirect))
                {
                    return redirect!;
                }

                return Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                    @"AppData\LocalLow\ZAUM Studio\Disco Elysium\Settings\Settings.json");
            }
        }

        /// <summary>Whether the game has written its settings at least once.</summary>
        public static bool Exists => File.Exists(SettingsPath);

        /// <summary>
        /// Copies the settings file aside, and exports the PlayerPrefs key beside it.
        /// </summary>
        /// <param name="backupPath">Where to copy the settings file.</param>
        /// <param name="includeRegistry">Whether to export PlayerPrefs as well.</param>
        /// <exception cref="InvalidOperationException">There is nothing to back up.</exception>
        public static SettingsBackup Backup(string backupPath, bool includeRegistry = true)
        {
            if (!Exists)
            {
                throw new InvalidOperationException(
                    $"Nothing to back up: no settings file at {SettingsPath}.");
            }

            string? directory = Path.GetDirectoryName(backupPath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory!);
            }

            File.Copy(SettingsPath, backupPath, overwrite: true);

            string? registryPath = null;
            if (includeRegistry)
            {
                registryPath = backupPath + ".reg";
                RunReg("export", RegistryKey, registryPath, "/y");
            }

            return new SettingsBackup(backupPath, registryPath);
        }

        /// <summary>Installs a settings file, replacing the player's.</summary>
        /// <param name="testSettingsPath">The file to install.</param>
        /// <exception cref="FileNotFoundException">There is no such file.</exception>
        public static void Install(string testSettingsPath)
        {
            if (!File.Exists(testSettingsPath))
            {
                throw new FileNotFoundException(
                    $"No test settings file at {testSettingsPath}.", testSettingsPath);
            }

            string? directory = Path.GetDirectoryName(SettingsPath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory!);
            }

            File.Copy(testSettingsPath, SettingsPath, overwrite: true);
        }

        /// <summary>The display settings a settings file asks for.</summary>
        /// <param name="path">The settings file to read.</param>
        /// <exception cref="InvalidDataException">A value is missing or not unique.</exception>
        public static DisplaySettings ReadDisplay(string path)
        {
            string json = File.ReadAllText(path);
            return new DisplaySettings(
                ReadInt(json, "resolutionWidth", path),
                ReadInt(json, "resolutionHeight", path),
                ReadInt(json, "DISPLAY MODE", path));
        }

        /// <summary>
        /// Whether the live settings file is still byte-for-byte the one installed.
        /// </summary>
        /// <remarks>
        /// Steam Auto-Cloud syncs this directory when the application launches and again
        /// when it exits, and it downloads the cloud copy BEFORE the game starts. So a
        /// file staged moments before launch can be replaced by the player's real
        /// settings in the gap, and the game reads those instead. That looks exactly like
        /// the staging having silently failed; comparing the bytes afterwards is what
        /// tells the two apart.
        /// </remarks>
        /// <param name="installedPath">The file that was installed.</param>
        public static bool StillMatches(string installedPath)
        {
            if (!File.Exists(SettingsPath) || !File.Exists(installedPath))
            {
                return false;
            }

            byte[] live = File.ReadAllBytes(SettingsPath);
            byte[] staged = File.ReadAllBytes(installedPath);
            if (live.Length != staged.Length)
            {
                return false;
            }

            for (int i = 0; i < live.Length; i++)
            {
                if (live[i] != staged[i])
                {
                    return false;
                }
            }

            return true;
        }

        private static int ReadInt(string json, string key, string path)
        {
            // Deliberately strict. The shape is "key": { "intValue": N, ... }, and
            // anything else means the file is not what this was written against. Guessing
            // would yield a plausible wrong number, which is the failure being hunted.
            var pattern = new Regex(
                "\"" + Regex.Escape(key) + @"""\s*:\s*\{\s*""intValue""\s*:\s*(-?\d+)");
            MatchCollection matches = pattern.Matches(json);
            if (matches.Count != 1)
            {
                throw new InvalidDataException(
                    $"Expected exactly one '{key}' in {path}, found {matches.Count}.");
            }

            return int.Parse(matches[0].Groups[1].Value, CultureInfo.InvariantCulture);
        }

        /// <summary>Puts the settings file, and the PlayerPrefs cache, back.</summary>
        /// <remarks>
        /// The registry is deleted before importing, because an import alone merges: a
        /// value the run added would survive and the restore would be a lie. That leaves
        /// a window in which the key is gone, which is why a failed import throws loudly
        /// rather than being swallowed.
        /// </remarks>
        /// <param name="backup">What <see cref="Backup"/> returned.</param>
        /// <exception cref="ArgumentNullException"><paramref name="backup"/> is null.</exception>
        /// <exception cref="FileNotFoundException">The backup is gone.</exception>
        public static void Restore(SettingsBackup backup)
        {
            if (backup == null)
            {
                throw new ArgumentNullException(nameof(backup));
            }

            if (!File.Exists(backup.SettingsPath))
            {
                throw new FileNotFoundException(
                    $"No settings backup at {backup.SettingsPath}.", backup.SettingsPath);
            }

            File.Copy(backup.SettingsPath, SettingsPath, overwrite: true);

            if (backup.RegistryPath != null && File.Exists(backup.RegistryPath))
            {
                RunReg("delete", RegistryKey, "/f");
                RunReg("import", backup.RegistryPath);
            }
        }

        /// <summary>
        /// Runs reg.exe, believing its exit code rather than its output.
        /// </summary>
        /// <remarks>
        /// reg.exe writes "The operation completed successfully." to STDERR even when it
        /// succeeds. A PowerShell version of this deleted the PlayerPrefs key and then
        /// aborted before importing it back, because redirecting that success message
        /// turned it into a terminating error. The exit code is the only thing worth
        /// reading - and for delete, code 1 also means "it was not there", which is not a
        /// failure worth stopping for.
        /// </remarks>
        private static void RunReg(params string[] arguments)
        {
            var start = new ProcessStartInfo("reg.exe")
            {
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                CreateNoWindow = true,
            };

            foreach (string argument in arguments)
            {
                start.Arguments += Quote(argument) + " ";
            }

            using (Process? process = Process.Start(start))
            {
                if (process == null)
                {
                    throw new InvalidOperationException("Could not start reg.exe.");
                }

                string error = process.StandardError.ReadToEnd();
                process.StandardOutput.ReadToEnd();
                process.WaitForExit();

                bool deleting = arguments.Length > 0 && arguments[0] == "delete";
                if (process.ExitCode != 0 && !(deleting && process.ExitCode == 1))
                {
                    throw new InvalidOperationException(
                        $"reg {string.Join(" ", arguments)} failed ({process.ExitCode}): {error.Trim()}");
                }
            }
        }

        private static string Quote(string argument)
        {
            return argument.IndexOf(' ') >= 0 ? "\"" + argument + "\"" : argument;
        }
    }
}
