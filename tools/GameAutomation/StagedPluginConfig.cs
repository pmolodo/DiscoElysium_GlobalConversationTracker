// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Changes settings in the mod's own BepInEx config for the length of a run, and
    /// puts the player's file back.
    /// </summary>
    /// <remarks>
    /// <para>Some of what the look-ahead does is only reachable through its
    /// configuration: the state budget has to be made small before a crawl will exhaust
    /// it, the diagnostics files are not written unless asked for, and the feature's own
    /// off switch has to be flipped to check that turning it off leaves tracking
    /// alone.</para>
    ///
    /// <para>Edited in place rather than replaced wholesale. The file carries the
    /// player's own choices for everything a run does not care about - colours, HUD
    /// offsets - and a run should change the one setting it is about and hand the rest
    /// back untouched. BepInEx rewrites the file on load with every key present, so a
    /// setting a run names is always already there to edit.</para>
    /// </remarks>
    public sealed class StagedPluginConfig : IDisposable
    {
        /// <summary>The mod's config file, named for its plugin id.</summary>
        public const string ConfigFileName =
            "com.molodowitch.globalconversationtracker.cfg";

        private readonly string _path;
        private readonly string _original;
        private readonly Action<string>? _progress;
        private bool _restored;

        private StagedPluginConfig(string path, string original, Action<string>? progress)
        {
            _path = path;
            _original = original;
            _progress = progress;
        }

        /// <summary>Where the mod's config lives for a game install.</summary>
        /// <param name="gameExecutable">Path to disco.exe.</param>
        public static string PathFor(string gameExecutable)
        {
            return Path.Combine(
                FilePaths.FolderOf(gameExecutable, nameof(gameExecutable)),
                "BepInEx",
                "config",
                ConfigFileName);
        }

        /// <summary>Applies settings, and puts the file back when the scope closes.</summary>
        /// <param name="gameExecutable">Path to disco.exe.</param>
        /// <param name="settings">Setting name to value; the section is found by name.</param>
        /// <param name="progress">Called with each change, for verbose output.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="FileNotFoundException">The mod has never been run.</exception>
        /// <exception cref="InvalidDataException">A named setting is not in the file.</exception>
        public static StagedPluginConfig Apply(
            string gameExecutable,
            IReadOnlyDictionary<string, string> settings,
            Action<string>? progress = null)
        {
            if (settings == null)
            {
                throw new ArgumentNullException(nameof(settings));
            }

            string path = PathFor(gameExecutable);
            if (!File.Exists(path))
            {
                throw new FileNotFoundException(
                    $"No mod config at {path}. BepInEx writes it the first time the mod "
                    + "loads, so run the game once before a test that changes a setting.",
                    path);
            }

            string original = File.ReadAllText(path);
            var staged = new StagedPluginConfig(path, original, progress);

            try
            {
                string updated = original;
                foreach (KeyValuePair<string, string> setting in settings)
                {
                    updated = Set(updated, setting.Key, setting.Value, path);
                    progress?.Invoke($"{setting.Key} = {setting.Value}");
                }

                if (updated != original)
                {
                    File.WriteAllText(path, updated, new UTF8Encoding(false));
                }
            }
            catch (Exception)
            {
                staged.Restore();
                throw;
            }

            return staged;
        }

        /// <summary>
        /// Replaces one setting's value, leaving everything around it alone.
        /// </summary>
        /// <remarks>
        /// A setting the file does not already carry is refused rather than appended.
        /// BepInEx writes every key it knows about, so a name it does not have is a
        /// misspelling or a setting that no longer exists - and appending it would be
        /// silently ignored by the game while the test went on believing it had taken.
        /// </remarks>
        internal static string Set(string config, string name, string value, string context)
        {
            string[] lines = config.Replace("\r\n", "\n").Split('\n');
            for (int i = 0; i < lines.Length; i++)
            {
                string trimmed = lines[i].TrimStart();
                if (trimmed.StartsWith("#", StringComparison.Ordinal)
                    || trimmed.StartsWith("[", StringComparison.Ordinal))
                {
                    continue;
                }

                int equals = trimmed.IndexOf('=');
                if (equals < 0)
                {
                    continue;
                }

                if (!string.Equals(
                    trimmed.Substring(0, equals).TrimEnd(), name, StringComparison.Ordinal))
                {
                    continue;
                }

                lines[i] = $"{name} = {value}";
                return string.Join(Environment.NewLine, lines);
            }

            throw new InvalidDataException(
                $"'{context}' has no setting called '{name}'. BepInEx writes every setting the "
                + "mod declares, so this is a misspelling or a setting that no longer exists.");
        }

        /// <summary>Reads one setting's value, or null if the file has no such name.</summary>
        /// <param name="config">The config file's text.</param>
        /// <param name="name">The setting name.</param>
        public static string? Get(string config, string name)
        {
            foreach (string line in config.Replace("\r\n", "\n").Split('\n'))
            {
                string trimmed = line.TrimStart();
                if (trimmed.StartsWith("#", StringComparison.Ordinal)
                    || trimmed.StartsWith("[", StringComparison.Ordinal))
                {
                    continue;
                }

                int equals = trimmed.IndexOf('=');
                if (equals >= 0
                    && string.Equals(
                        trimmed.Substring(0, equals).TrimEnd(), name, StringComparison.Ordinal))
                {
                    return trimmed.Substring(equals + 1).Trim();
                }
            }

            return null;
        }

        /// <summary>Puts the player's config back. Safe to call twice.</summary>
        public void Restore()
        {
            if (_restored)
            {
                return;
            }

            _restored = true;
            File.WriteAllText(_path, _original, new UTF8Encoding(false));
            _progress?.Invoke("the mod's config is back as it was");
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            Restore();
        }
    }
}
