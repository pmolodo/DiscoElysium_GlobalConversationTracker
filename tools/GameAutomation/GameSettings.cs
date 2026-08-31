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
    /// <para>The settings FILE is staged with the rest of the profile, which moves as one
    /// directory; see <see cref="GameProfile"/>. What is left here is the registry, which
    /// no folder move can reach, and reading the display values a test asks for.</para>
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

        /// <summary>
        /// Writes Unity's own screen PlayerPrefs, which are what size the window.
        /// </summary>
        /// <remarks>
        /// Installing the settings FILE is not enough, and this is the part that was
        /// missing. Unity creates its window from these registry values before any game
        /// code runs, and while "use native" is set it takes the display's resolution and
        /// ignores the stored size - so a staged file asking for 1280x720 produced a
        /// 3840x1200 window that never changed. All four values have to move together:
        /// turning off native, choosing a real window, and giving it a size.
        ///
        /// Backup() already exports this whole key and Restore() re-imports it, so these
        /// writes are undone with everything else.
        /// </remarks>
        /// <param name="display">The resolution and mode to ask Unity for.</param>
        public static void InstallScreenPrefs(DisplaySettings display)
        {
            if (display == null)
            {
                throw new ArgumentNullException(nameof(display));
            }

            SetInt(UnityPlayerPrefs.UseNativeResolution, 0);
            SetInt(
                UnityPlayerPrefs.FullScreenMode,
                display.IsWindowed ? UnityPlayerPrefs.Windowed : UnityPlayerPrefs.FullScreenWindow);
            SetInt(UnityPlayerPrefs.ResolutionWidth, display.Width);
            SetInt(UnityPlayerPrefs.ResolutionHeight, display.Height);
        }

        /// <summary>Exports Unity's PlayerPrefs key.</summary>
        /// <remarks>
        /// Kept separate from staging the profile folder, because this does not live in
        /// it. The profile is one directory that can be moved aside whole; PlayerPrefs are
        /// in the registry, so a run that changes them has to put them back by itself.
        /// </remarks>
        /// <param name="exportPath">Where to write the .reg export.</param>
        public static void BackupRegistry(string exportPath)
        {
            string? directory = Path.GetDirectoryName(exportPath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory!);
            }

            RunReg("export", RegistryKey, exportPath, "/y");
        }

        /// <summary>Puts Unity's PlayerPrefs key back from an export.</summary>
        /// <remarks>
        /// Deletes before importing, so values this run ADDED are gone rather than left
        /// behind. An import alone merges, which would leave a "use native resolution" of
        /// 0 in place for a player whose key never had one.
        /// </remarks>
        /// <param name="exportPath">The .reg export to restore.</param>
        /// <exception cref="FileNotFoundException">There is no such export.</exception>
        public static void RestoreRegistry(string exportPath)
        {
            if (!File.Exists(exportPath))
            {
                throw new FileNotFoundException(
                    $"No PlayerPrefs backup at {exportPath}.", exportPath);
            }

            RunReg("delete", RegistryKey, "/f");
            RunReg("import", exportPath);
        }

        private static void SetInt(string key, int value)
        {
            RunReg(
                "add",
                RegistryKey,
                "/v",
                UnityPlayerPrefs.ValueName(key),
                "/t",
                "REG_DWORD",
                "/d",
                value.ToString(CultureInfo.InvariantCulture),
                "/f");
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
        /// The profile is Steam-Cloud-synced, and a sync can replace a staged file
        /// between staging it and the game reading it - which looks exactly like the
        /// staging having silently failed. Comparing the bytes afterwards tells the two
        /// apart. Measured not to be happening on a direct launch with cloud off, but it
        /// costs nothing to keep checking, and it is how that was established.
        /// </remarks>
        /// <param name="installedPath">The file that was installed.</param>
        public static bool StillMatches(string installedPath)
        {
            if (!File.Exists(GameProfile.SettingsFile) || !File.Exists(installedPath))
            {
                return false;
            }

            byte[] live = File.ReadAllBytes(GameProfile.SettingsFile);
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
