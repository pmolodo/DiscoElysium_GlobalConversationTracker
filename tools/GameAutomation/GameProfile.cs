// SPDX-License-Identifier: MIT
using System;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>Where the player's profile went while a test ran.</summary>
    public sealed class ProfileBackup
    {
        /// <summary>Creates a record.</summary>
        /// <param name="movedTo">Where the real profile was moved, or null if there was none.</param>
        /// <param name="entryCount">Files and folders it held, for checking the restore.</param>
        public ProfileBackup(string? movedTo, int entryCount)
        {
            MovedTo = movedTo;
            EntryCount = entryCount;
        }

        /// <summary>Where the real profile was moved, or null if there was none.</summary>
        public string? MovedTo { get; }

        /// <summary>How many entries it held when it was moved.</summary>
        public int EntryCount { get; }
    }

    /// <summary>
    /// Swapping the whole game profile for a built one, and putting it back.
    /// </summary>
    /// <remarks>
    /// <para>The profile is one folder holding everything the game remembers about a
    /// player: settings, saves, collages, logs. Staging it as a unit rather than
    /// file-by-file means one move out and one move back, so there is no order to get
    /// wrong and no half-restored state to reason about - either the player's folder is
    /// in place or it is sitting whole at a path this reports.</para>
    ///
    /// <para>A move, not a copy. The folder runs to gigabytes, and a move on one volume is
    /// atomic, so there is never a moment when both or neither exists. Nothing here
    /// deletes the player's folder; the only thing removed is the staged one this built,
    /// and only once the original is safely back.</para>
    ///
    /// <para>The profile is Steam-Cloud-synced, which is what makes leaving a staged one
    /// behind serious rather than untidy: a later launch can push it upward. That is why
    /// restore is checked and why failing to restore is loud.</para>
    /// </remarks>
    public static class GameProfile
    {
        /// <summary>The folder holding everything the game remembers.</summary>
        /// <remarks>
        /// <c>DISCO_ELYSIUM_GCT_PROFILE_DIR</c> redirects it, which is how the tests run
        /// against a scratch folder instead of a real installation.
        /// </remarks>
        public static string ProfilePath
        {
            get
            {
                string? redirect = Environment.GetEnvironmentVariable(
                    "DISCO_ELYSIUM_GCT_PROFILE_DIR");
                if (!string.IsNullOrWhiteSpace(redirect))
                {
                    return redirect!;
                }

                return Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                    @"AppData\LocalLow\ZAUM Studio\Disco Elysium");
            }
        }

        /// <summary>The settings file inside the profile.</summary>
        public static string SettingsFile => Path.Combine(ProfilePath, "Settings", "Settings.json");

        /// <summary>The saves folder inside the profile.</summary>
        public static string SavesFolder => Path.Combine(ProfilePath, "SaveGames");

        /// <summary>Where to move the profile, beside where it already is.</summary>
        /// <remarks>
        /// A sibling, not a temp folder, because Directory.Move cannot cross volumes and
        /// the temp directory is not guaranteed to be on the same one - AppData redirected
        /// to another drive, or a TEMP set elsewhere, and the move fails. Beside it is the
        /// same volume by construction, and it is also where somebody would look for it.
        ///
        /// Outside the profile rather than inside: Steam Auto-Cloud is configured per
        /// folder, and a backup nested under a synced one could be uploaded.
        /// </remarks>
        /// <param name="stamp">Something to make the name unique, usually a timestamp.</param>
        public static string DefaultBackupPath(string stamp)
        {
            string profile = ProfilePath;
            string? parent = Path.GetDirectoryName(profile.TrimEnd(
                Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
            if (string.IsNullOrEmpty(parent))
            {
                throw new InvalidOperationException(
                    $"Cannot work out where to put a backup beside {profile}.");
            }

            return Path.Combine(parent!, $"{Path.GetFileName(profile)}.backup-{stamp}");
        }

        /// <summary>Moves the player's profile aside.</summary>
        /// <param name="movedTo">Where to move it. Must not already exist.</param>
        /// <exception cref="InvalidOperationException">The destination is in the way.</exception>
        public static ProfileBackup Backup(string movedTo)
        {
            if (Directory.Exists(movedTo))
            {
                throw new InvalidOperationException(
                    $"Refusing to move the profile onto an existing folder at {movedTo}.");
            }

            string profile = ProfilePath;
            if (!Directory.Exists(profile))
            {
                return new ProfileBackup(null, 0);
            }

            int entries = Directory.GetFileSystemEntries(profile).Length;

            string? parent = Path.GetDirectoryName(movedTo);
            if (!string.IsNullOrEmpty(parent))
            {
                Directory.CreateDirectory(parent!);
            }

            try
            {
                Directory.Move(profile, movedTo);
            }
            catch (IOException error)
            {
                // Two causes, and Windows names neither: the paths being on different
                // volumes, where Move does not work at all, and something holding the
                // folder open, where the message is a bare "used by another process" with
                // no hint which one. Both are worth answering here rather than leaving to
                // whoever reads the failure.
                throw new IOException(
                    $"Could not move {profile} to {movedTo}: {error.Message}"
                    + Environment.NewLine
                    + FileLocks.Describe(profile)
                    + Environment.NewLine
                    + "A backup also has to be on the same volume as the profile; see "
                    + "DefaultBackupPath.",
                    error);
            }
            catch (UnauthorizedAccessException error)
            {
                throw new UnauthorizedAccessException(
                    $"Not allowed to move {profile} to {movedTo}: {error.Message}"
                    + Environment.NewLine
                    + FileLocks.Describe(profile),
                    error);
            }

            return new ProfileBackup(movedTo, entries);
        }

        /// <summary>Builds a fresh profile containing exactly what a test needs.</summary>
        /// <remarks>
        /// Only what the game reads is created. Everything else - logs, collages, Unity's
        /// own scratch folders - the game makes for itself, and leaving them out is what
        /// makes each run start from the same place regardless of what the last one did.
        /// </remarks>
        /// <param name="settingsFile">The settings file to install, or null to omit it.</param>
        /// <param name="saveFile">A single save to install, or null for none.</param>
        /// <exception cref="FileNotFoundException">A named file does not exist.</exception>
        public static void Stage(string? settingsFile, string? saveFile)
        {
            Directory.CreateDirectory(ProfilePath);
            Directory.CreateDirectory(SavesFolder);

            if (settingsFile != null)
            {
                if (!File.Exists(settingsFile))
                {
                    throw new FileNotFoundException(
                        $"No settings file at {settingsFile}.", settingsFile);
                }

                Directory.CreateDirectory(Path.GetDirectoryName(SettingsFile)!);
                File.Copy(settingsFile, SettingsFile, overwrite: true);
            }

            if (saveFile != null)
            {
                GameSaves.CopyInto(saveFile, SavesFolder);
            }
        }

        /// <summary>Puts the player's profile back, removing the staged one.</summary>
        /// <param name="backup">What Backup returned.</param>
        /// <exception cref="ArgumentNullException"><paramref name="backup"/> is null.</exception>
        /// <exception cref="InvalidOperationException">It did not come back intact.</exception>
        public static void Restore(ProfileBackup backup)
        {
            if (backup == null)
            {
                throw new ArgumentNullException(nameof(backup));
            }

            if (backup.MovedTo == null)
            {
                // There was nothing to move aside, so anything staged is this tool's.
                if (Directory.Exists(ProfilePath))
                {
                    Directory.Delete(ProfilePath, recursive: true);
                }

                return;
            }

            if (!Directory.Exists(backup.MovedTo))
            {
                throw new InvalidOperationException(
                    $"The profile backup at {backup.MovedTo} is gone. A staged profile is still at "
                    + $"{ProfilePath}; do not launch the game until this is sorted out by hand.");
            }

            if (Directory.Exists(ProfilePath))
            {
                Directory.Delete(ProfilePath, recursive: true);
            }

            Directory.Move(backup.MovedTo, ProfilePath);

            int restored = Directory.GetFileSystemEntries(ProfilePath).Length;
            if (restored != backup.EntryCount)
            {
                throw new InvalidOperationException(
                    $"Restored {restored} entries but {backup.EntryCount} were moved aside. The "
                    + $"profile is at {ProfilePath}; check it before playing.");
            }
        }
    }
}
