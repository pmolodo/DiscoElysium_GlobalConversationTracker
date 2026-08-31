// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>Where the player's saves went while a test ran.</summary>
    public sealed class SavesBackup
    {
        /// <summary>Creates a record.</summary>
        /// <param name="movedTo">Where the real folder was moved, or null if there was none.</param>
        /// <param name="fileCount">How many files it held, for checking the restore.</param>
        public SavesBackup(string? movedTo, int fileCount)
        {
            MovedTo = movedTo;
            FileCount = fileCount;
        }

        /// <summary>Where the real folder was moved, or null if there was none.</summary>
        public string? MovedTo { get; }

        /// <summary>How many files it held when it was moved.</summary>
        public int FileCount { get; }
    }

    /// <summary>
    /// Swapping the player's saves for a single known one, and putting them back.
    /// </summary>
    /// <remarks>
    /// <para>A test that loads a save has to know WHICH save. Navigating a load menu by
    /// arrow keys does not scale - this game's folder holds 143 of them - and picking the
    /// wrong one still looks like a pass. Staging a folder containing exactly one save
    /// removes the navigation entirely: Continue loads it, and there is nothing else it
    /// could load.</para>
    ///
    /// <para>The real folder is MOVED aside rather than copied, because it runs to
    /// gigabytes, and moved back afterwards. Nothing here ever deletes it. The staged
    /// folder is the only thing removed, and only after the original is safely back.</para>
    ///
    /// <para>This folder is Steam-Cloud-synced, which makes it the most dangerous thing
    /// the harness touches: a game exiting while a nearly-empty folder is staged can sync
    /// that state upward. The restore therefore runs before anything else in the cleanup
    /// path, and reports loudly if the file count does not come back.</para>
    /// </remarks>
    public static class GameSaves
    {
        /// <summary>The save file extension. Each save also has a .jpg thumbnail.</summary>
        public const string SaveExtension = ".ntwtf.zip";

        /// <summary>The folder the game reads saves from.</summary>
        /// <remarks>
        /// <c>DISCO_ELYSIUM_GCT_SAVES_DIR</c> redirects it, which is how the tests run
        /// against a scratch folder instead of a real installation.
        /// </remarks>
        public static string SavesPath
        {
            get
            {
                string? redirect = Environment.GetEnvironmentVariable(
                    "DISCO_ELYSIUM_GCT_SAVES_DIR");
                if (!string.IsNullOrWhiteSpace(redirect))
                {
                    return redirect!;
                }

                return Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                    @"AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames");
            }
        }

        /// <summary>Every save in a folder, without their thumbnails.</summary>
        /// <param name="folder">The folder to list.</param>
        public static string[] ListSaves(string folder)
        {
            if (!Directory.Exists(folder))
            {
                return Array.Empty<string>();
            }

            var saves = new List<string>();
            foreach (string file in Directory.GetFiles(folder))
            {
                if (file.EndsWith(SaveExtension, StringComparison.OrdinalIgnoreCase))
                {
                    saves.Add(file);
                }
            }

            saves.Sort(StringComparer.OrdinalIgnoreCase);
            return saves.ToArray();
        }

        /// <summary>Moves the player's saves aside, leaving an empty folder.</summary>
        /// <param name="movedTo">Where to move them. Must not already exist.</param>
        /// <exception cref="InvalidOperationException">The destination is in the way.</exception>
        public static SavesBackup Backup(string movedTo)
        {
            if (Directory.Exists(movedTo))
            {
                throw new InvalidOperationException(
                    $"Refusing to move saves onto an existing folder at {movedTo}.");
            }

            string saves = SavesPath;
            if (!Directory.Exists(saves))
            {
                return new SavesBackup(null, 0);
            }

            int count = Directory.GetFiles(saves).Length;

            string? parent = Path.GetDirectoryName(movedTo);
            if (!string.IsNullOrEmpty(parent))
            {
                Directory.CreateDirectory(parent!);
            }

            // A move, not a copy: the real folder is gigabytes, and a move is atomic on
            // the same volume so there is no window where both or neither exists.
            Directory.Move(saves, movedTo);
            Directory.CreateDirectory(saves);

            return new SavesBackup(movedTo, count);
        }

        /// <summary>Puts a single save into the staged folder.</summary>
        /// <remarks>
        /// The thumbnail beside it is copied too when present. Without it the menu shows
        /// the slot with a blank image, which still loads but makes a screenshot
        /// comparison depend on whichever picture happened to be there.
        /// </remarks>
        /// <param name="savePath">The .ntwtf.zip to stage.</param>
        /// <exception cref="FileNotFoundException">There is no such save.</exception>
        public static void Install(string savePath)
        {
            if (!File.Exists(savePath))
            {
                throw new FileNotFoundException($"No save file at {savePath}.", savePath);
            }

            if (!savePath.EndsWith(SaveExtension, StringComparison.OrdinalIgnoreCase))
            {
                throw new ArgumentException(
                    $"A save is a '{SaveExtension}' file; got {Path.GetFileName(savePath)}.",
                    nameof(savePath));
            }

            Directory.CreateDirectory(SavesPath);
            File.Copy(
                savePath,
                Path.Combine(SavesPath, Path.GetFileName(savePath)),
                overwrite: true);

            string thumbnail = savePath.Substring(0, savePath.Length - SaveExtension.Length) + ".jpg";
            if (File.Exists(thumbnail))
            {
                File.Copy(
                    thumbnail,
                    Path.Combine(SavesPath, Path.GetFileName(thumbnail)),
                    overwrite: true);
            }
        }

        /// <summary>Puts the player's saves back, and removes the staged ones.</summary>
        /// <param name="backup">What Backup returned.</param>
        /// <exception cref="ArgumentNullException"><paramref name="backup"/> is null.</exception>
        /// <exception cref="InvalidOperationException">The saves did not come back intact.</exception>
        public static void Restore(SavesBackup backup)
        {
            if (backup == null)
            {
                throw new ArgumentNullException(nameof(backup));
            }

            if (backup.MovedTo == null)
            {
                return;
            }

            if (!Directory.Exists(backup.MovedTo))
            {
                throw new InvalidOperationException(
                    $"The saves backup at {backup.MovedTo} is gone. The staged folder is still at "
                    + $"{SavesPath}; do not launch the game until this is sorted out by hand.");
            }

            // Remove the staged folder first, then move the real one back. Deleting only
            // ever touches the folder this created, never the player's.
            if (Directory.Exists(SavesPath))
            {
                Directory.Delete(SavesPath, recursive: true);
            }

            Directory.Move(backup.MovedTo, SavesPath);

            int restored = Directory.GetFiles(SavesPath).Length;
            if (restored != backup.FileCount)
            {
                throw new InvalidOperationException(
                    $"Restored {restored} files but {backup.FileCount} were moved aside. The saves "
                    + $"are at {SavesPath}; check them before playing.");
            }
        }
    }
}
