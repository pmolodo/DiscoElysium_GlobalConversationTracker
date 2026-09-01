// SPDX-License-Identifier: MIT
using System;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>Temporarily disables Steam Cloud for Disco Elysium.</summary>
    public sealed class SteamCloudOverride : IDisposable
    {
        /// <summary>Disco Elysium's Steam app id.</summary>
        public const string DiscoElysiumAppId = "632470";

        private readonly string _configPath;
        private readonly string _steamExecutable;
        private readonly bool _steamWasRunning;
        private readonly bool _wasEnabled;
        private readonly TimeSpan _timeout;
        private readonly Action<string>? _progress;
        private bool _restored;

        private SteamCloudOverride(
            string configPath,
            string steamExecutable,
            bool steamWasRunning,
            bool wasEnabled,
            string backupPath,
            TimeSpan timeout,
            Action<string>? progress)
        {
            _configPath = configPath;
            _steamExecutable = steamExecutable;
            _steamWasRunning = steamWasRunning;
            _wasEnabled = wasEnabled;
            _timeout = timeout;
            _progress = progress;
            BackupPath = backupPath;
        }

        /// <summary>The untouched VDF copied after Steam cleanly exited.</summary>
        public string BackupPath { get; }

        /// <summary>Checks the setting and disables it only when necessary.</summary>
        public static SteamCloudOverride Disable(
            string steamExecutable,
            string backupPath,
            TimeSpan timeout,
            Action<string>? progress = null)
        {
            string steamRoot = Path.GetDirectoryName(steamExecutable);
            string[] configs = SteamCloud.FindAppConfigs(steamRoot, DiscoElysiumAppId);
            if (configs.Length != 1)
            {
                throw new InvalidDataException(
                    "Expected exactly one Steam account able to hold Disco Elysium's cloud "
                    + $"setting, found {configs.Length}.");
            }

            bool steamWasRunning = SteamClient.IsRunning;
            SteamClient.Shutdown(steamExecutable, timeout, progress);

            string configPath = configs[0];
            File.Copy(configPath, backupPath, overwrite: true);
            progress?.Invoke($"backed up sharedconfig.vdf to {backupPath}");

            // Steam owns the authoritative copy in memory while running and writes it
            // during shutdown. Read the setting only after that write has completed.
            bool wasEnabled = SteamCloud.IsEnabled(
                File.ReadAllText(configPath), DiscoElysiumAppId);
            var scope = new SteamCloudOverride(
                configPath,
                steamExecutable,
                steamWasRunning,
                wasEnabled,
                backupPath,
                timeout,
                progress);

            if (!wasEnabled)
            {
                progress?.Invoke("Steam Cloud is already disabled for Disco Elysium");
                if (steamWasRunning)
                {
                    SteamClient.Start(steamExecutable, timeout, progress);
                }
                return scope;
            }

            try
            {
                WriteSetting(configPath, enabled: false);
                progress?.Invoke("disabled Steam Cloud for Disco Elysium");

                if (steamWasRunning)
                {
                    SteamClient.Start(steamExecutable, timeout, progress);
                }

                return scope;
            }
            catch (Exception)
            {
                scope.Restore();
                throw;
            }
        }

        /// <summary>Restores only Disco Elysium's cloud key to its original state.</summary>
        public void Restore()
        {
            if (_restored)
            {
                return;
            }

            _restored = true;
            if (!_wasEnabled)
            {
                return;
            }

            SteamClient.Shutdown(_steamExecutable, _timeout, _progress);
            WriteSetting(_configPath, enabled: true);
            _progress?.Invoke("restored Steam Cloud for Disco Elysium");

            if (_steamWasRunning)
            {
                SteamClient.Start(_steamExecutable, _timeout, _progress);
            }
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            Restore();
        }

        private static void WriteSetting(string path, bool enabled)
        {
            string before = File.ReadAllText(path);
            string after = SteamCloud.SetEnabled(before, DiscoElysiumAppId, enabled);
            if (after != before)
            {
                File.WriteAllText(path, after);
            }
        }
    }
}
