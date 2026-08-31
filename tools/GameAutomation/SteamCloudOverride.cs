// SPDX-License-Identifier: MIT
using System;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Turns Steam Cloud off for one game for the life of a test, and puts it back.
    /// </summary>
    /// <remarks>
    /// <para>The sequence is forced by how Steam behaves. It keeps localconfig.vdf in
    /// memory and rewrites it on exit, so the file can only be edited while Steam is
    /// stopped: shut Steam down, edit, start it again. Restoring repeats all three.</para>
    ///
    /// <para>Restore puts back the state that was FOUND, not a fixed value. A player who
    /// already had cloud off for this game keeps it off.</para>
    ///
    /// <para>Restore is also surgical - it removes the one key this added - rather than
    /// copying a whole-file backup over the top. Steam will have rewritten the file on
    /// exit with legitimate changes of its own (playtime, last-played, sync state), and a
    /// blanket restore would silently discard them. The full backup is still taken, but as
    /// a disaster copy rather than as the restore mechanism.</para>
    /// </remarks>
    public sealed class SteamCloudOverride : IDisposable
    {
        private readonly string _localConfigPath;
        private readonly string _steamExecutable;
        private readonly string _appId;
        private readonly bool _wasEnabled;
        private readonly TimeSpan _timeout;
        private readonly Action<string>? _progress;
        private bool _restored;

        private SteamCloudOverride(
            string localConfigPath,
            string steamExecutable,
            string appId,
            bool wasEnabled,
            string backupPath,
            TimeSpan timeout,
            Action<string>? progress)
        {
            _localConfigPath = localConfigPath;
            _steamExecutable = steamExecutable;
            _appId = appId;
            _wasEnabled = wasEnabled;
            _timeout = timeout;
            _progress = progress;
            BackupPath = backupPath;
        }

        /// <summary>The untouched copy of localconfig.vdf, kept in case of disaster.</summary>
        public string BackupPath { get; }

        /// <summary>Whether cloud sync was on before this changed anything.</summary>
        public bool WasEnabled => _wasEnabled;

        /// <summary>Turns cloud sync off for an app, restarting Steam around the edit.</summary>
        /// <param name="localConfigPath">The account's localconfig.vdf.</param>
        /// <param name="steamExecutable">Path to steam.exe.</param>
        /// <param name="appId">The Steam app id.</param>
        /// <param name="backupPath">Where to keep the untouched copy.</param>
        /// <param name="timeout">How long to allow Steam to stop and start.</param>
        /// <param name="progress">Called with status, for verbose output.</param>
        public static SteamCloudOverride Disable(
            string localConfigPath,
            string steamExecutable,
            string appId,
            string backupPath,
            TimeSpan timeout,
            Action<string>? progress = null)
        {
            if (!File.Exists(localConfigPath))
            {
                throw new FileNotFoundException(
                    $"No Steam local config at {localConfigPath}.", localConfigPath);
            }

            File.Copy(localConfigPath, backupPath, overwrite: true);
            progress?.Invoke($"backed up localconfig.vdf to {backupPath}");

            bool wasEnabled = SteamCloud.IsEnabled(
                File.ReadAllText(localConfigPath), appId);

            var scope = new SteamCloudOverride(
                localConfigPath, steamExecutable, appId, wasEnabled, backupPath, timeout, progress);

            if (!wasEnabled)
            {
                progress?.Invoke($"Steam Cloud is already off for app {appId}; leaving Steam alone");
                return scope;
            }

            scope.Rewrite(enabled: false);
            return scope;
        }

        /// <summary>Puts the cloud setting back the way it was found.</summary>
        public void Restore()
        {
            if (_restored)
            {
                return;
            }

            _restored = true;

            if (!_wasEnabled)
            {
                // It was already off, so nothing was changed and nothing needs undoing.
                return;
            }

            Rewrite(enabled: true);
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            Restore();
        }

        private void Rewrite(bool enabled)
        {
            string what = enabled ? "re-enabling" : "disabling";
            _progress?.Invoke($"{what} Steam Cloud for app {_appId}");

            SteamClient.Shutdown(_steamExecutable, _timeout, _progress);

            string vdf = File.ReadAllText(_localConfigPath);
            string updated = SteamCloud.SetEnabled(vdf, _appId, enabled);
            if (!ReferenceEquals(vdf, updated) && vdf != updated)
            {
                File.WriteAllText(_localConfigPath, updated);
                _progress?.Invoke($"localconfig.vdf updated ({what})");
            }
            else
            {
                _progress?.Invoke("localconfig.vdf already in the wanted state");
            }

            SteamClient.Start(_steamExecutable, _timeout, silent: true, progress: _progress);
        }
    }
}
