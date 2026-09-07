// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Everything an in-game run needs put in place, and taken back out again.
    /// </summary>
    /// <remarks>
    /// <para>One scope for all of it because the pieces are not independent. The profile
    /// carries the saves and the settings file; the display size does NOT come from that
    /// file but from Unity's PlayerPrefs in the registry, which the profile move cannot
    /// reach; and both have to be put back whatever happens.</para>
    ///
    /// <para>It exists because they had drifted. The look-ahead run staged the settings
    /// file and stopped there, so it launched at whatever resolution the machine
    /// happened to be at while the load-save run forced 1280x720 and asserted it - two
    /// in-game runs measuring the same game under different conditions, and nothing
    /// saying so. A run that wants to differ now has to say which argument it is
    /// changing.</para>
    /// </remarks>
    public sealed class StagedGame : IDisposable
    {
        private readonly ProfileBackup _profileBackup;
        private readonly string _registryBackupPath;
        private readonly Action<string>? _progress;
        private readonly Action<string> _onFailure;
        private bool _restored;
        private bool _abandoned;

        private StagedGame(
            ProfileBackup profileBackup,
            string registryBackupPath,
            DisplaySettings requested,
            Action<string>? progress,
            Action<string>? onFailure)
        {
            _profileBackup = profileBackup;
            _registryBackupPath = registryBackupPath;
            _progress = progress;
            _onFailure = onFailure ?? (message => Console.Error.WriteLine(message));
            Requested = requested;
        }

        /// <summary>The display the run asked Unity for.</summary>
        public DisplaySettings Requested { get; }

        /// <summary>Where the player's profile is while the run happens.</summary>
        public string? ProfileMovedTo => _profileBackup.MovedTo;

        /// <summary>Where the player's PlayerPrefs are while the run happens.</summary>
        public string RegistryBackupPath => _registryBackupPath;

        /// <summary>Puts a test profile and display in place.</summary>
        /// <param name="processName">The game's process name; it must not be running.</param>
        /// <param name="settingsFile">The settings file to stage.</param>
        /// <param name="saveFiles">The saves to stage, or null for none.</param>
        /// <param name="globalStateFile">The mod's global state to stage, or null.</param>
        /// <param name="screenOverride">
        /// A display to ask Unity for instead of the settings file's, or null to use the
        /// file's own.
        /// </param>
        /// <param name="installScreenPrefs">
        /// Whether to write Unity's PlayerPrefs at all. False leaves the registry alone,
        /// which is only for finding out what the settings file does on its own.
        /// </param>
        /// <param name="backupProfile">
        /// How to move the player's profile aside, or null for the plain move. The
        /// harness passes its own so it can first close whatever is holding the folder.
        /// </param>
        /// <param name="progress">Called with each step, for verbose output.</param>
        /// <param name="onFailure">
        /// Called when something cannot be put back, or null for standard error. This is
        /// the one thing a caller must not miss.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="InvalidOperationException">
        /// The game is already running, or no attached display could hold the window this
        /// would ask for.
        /// </exception>
        public static StagedGame Stage(
            string processName,
            string settingsFile,
            IEnumerable<string>? saveFiles,
            string? globalStateFile,
            DisplaySettings? screenOverride = null,
            bool installScreenPrefs = true,
            Func<string, ProfileBackup>? backupProfile = null,
            Action<string>? progress = null,
            Action<string>? onFailure = null)
        {
            if (settingsFile == null)
            {
                throw new ArgumentNullException(nameof(settingsFile));
            }

            if (Process.GetProcessesByName(processName).Length > 0)
            {
                throw new InvalidOperationException(
                    $"'{processName}' is already running. Close it first: two instances make "
                    + "the capture ambiguous.");
            }

            DisplaySettings requested = screenOverride ?? GameSettings.ReadDisplay(settingsFile);

            // BEFORE ANYTHING IS MOVED, and before the game is launched. A window the
            // display cannot hold gets clamped, every screen reference this run compares
            // against was captured at the requested size, and the run then fails on the
            // window check having already staged the player's profile aside and spent a
            // launch. The refusal below costs nothing and reads the same diagnosis.
            //
            // Only what is CLEARLY impossible: DisplayBounds returns Unknown for a
            // fullscreen request, for a process that is not measuring real pixels, and for
            // a machine whose displays it could not read. Those are reported and allowed.
            DisplayBounds.FitReport fit = DisplayBounds.CanHold(requested);
            if (fit.Verdict == DisplayBounds.Fit.TooSmall)
            {
                throw new InvalidOperationException(
                    $"The display cannot give the game the {requested} it would be asked "
                    + $"for: {fit.What}. Every screen reference an in-game run compares "
                    + "against was captured at that size, so nothing downstream could "
                    + "pass. Check the display's orientation, resolution and scaling, and "
                    + "run again.");
            }

            progress?.Invoke(fit.What);

            string stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss");
            string registryBackupPath = Path.Combine(
                Path.GetTempPath(), $"disco-prefs-{stamp}.reg");

            // Backed up separately from the profile because the profile move cannot
            // reach it: PlayerPrefs live under HKCU, not in the folder.
            GameSettings.BackupRegistry(registryBackupPath);
            progress?.Invoke($"prefs backed up to {registryBackupPath}");

            ProfileBackup profileBackup;
            try
            {
                profileBackup = (backupProfile ?? GameProfile.Backup)(
                    GameProfile.DefaultBackupPath(stamp));
            }
            catch (Exception)
            {
                GameSettings.RestoreRegistry(registryBackupPath);
                File.Delete(registryBackupPath);
                throw;
            }

            progress?.Invoke(
                profileBackup.MovedTo == null
                    ? "no profile to move aside; a fresh one will be built"
                    : $"{profileBackup.EntryCount} entries moved to {profileBackup.MovedTo}");

            var staged = new StagedGame(
                profileBackup, registryBackupPath, requested, progress, onFailure);

            try
            {
                GameProfile.Stage(settingsFile, saveFiles, globalStateFile);
                progress?.Invoke($"staged {Path.GetFileName(settingsFile)}");

                // The settings file does not size the window on its own; Unity does,
                // from the registry, before the game runs. See
                // testing/SETTINGS-PRECEDENCE.md.
                if (installScreenPrefs)
                {
                    GameSettings.InstallScreenPrefs(requested);
                    progress?.Invoke($"asked Unity for {requested}");
                }
                else
                {
                    progress?.Invoke(
                        "left Unity's registry alone; only the settings file was staged");
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
        /// Leaves everything staged, for a run that deliberately outlives the harness.
        /// </summary>
        /// <remarks>
        /// The caller has to say what it left behind: a staged profile is the player's
        /// profile as far as the game and Steam Cloud are concerned.
        /// </remarks>
        public void Abandon()
        {
            _abandoned = true;
        }

        /// <summary>Whether anything failed to go back where it came from.</summary>
        public bool RestoreFailed { get; private set; }

        /// <summary>Puts the profile and the PlayerPrefs back. Safe to call twice.</summary>
        /// <remarks>
        /// <para>The profile first: a staged one is the worst thing to leave behind,
        /// because it is Steam-Cloud-synced and a later launch can push it upward.</para>
        ///
        /// <para>Each gets its own attempt, so a failure in one does not skip the other -
        /// they are independent, and leaving either is its own problem. Failures are
        /// reported rather than thrown: this runs in a finally, and throwing there would
        /// replace whatever went wrong during the run with a complaint about the
        /// cleanup.</para>
        /// </remarks>
        public void Restore()
        {
            if (_restored || _abandoned)
            {
                return;
            }

            _restored = true;

            try
            {
                GameProfile.Restore(_profileBackup);
                _progress?.Invoke($"profile restored ({_profileBackup.EntryCount} entries)");
            }
            catch (Exception error)
            {
                RestoreFailed = true;
                _onFailure(
                    $"PROFILE NOT RESTORED: {error.Message}"
                    + Environment.NewLine
                    + "Put it back by hand before launching the game again; a staged profile "
                    + "left in place can be synced to Steam Cloud by the next launch.");
            }

            try
            {
                if (File.Exists(_registryBackupPath))
                {
                    GameSettings.RestoreRegistry(_registryBackupPath);
                    File.Delete(_registryBackupPath);
                    _progress?.Invoke("prefs restored");
                }
            }
            catch (Exception error)
            {
                RestoreFailed = true;
                _onFailure(
                    $"PLAYERPREFS NOT RESTORED: {error.Message}"
                    + Environment.NewLine
                    + $"The export is at {_registryBackupPath}; the game is left at whatever "
                    + "resolution the run asked for.");
            }
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            Restore();
        }
    }
}
