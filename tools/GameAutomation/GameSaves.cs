// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>Knowing what a save file is, and putting one somewhere.</summary>
    /// <remarks>
    /// <para>A test that loads a save has to know WHICH save. Navigating a load menu by
    /// arrow keys does not scale - this game's folder holds 143 of them - and picking the
    /// wrong one still looks like a pass. Staging a folder containing exactly one save
    /// removes the navigation entirely: Continue loads it, and there is nothing else it
    /// could load.</para>
    ///
    /// <para>Moving the folder aside is <see cref="GameProfile"/>'s job; this only knows
    /// what a save looks like.</para>
    /// </remarks>
    public static class GameSaves
    {
        /// <summary>The save file extension. Each save also has a .jpg thumbnail.</summary>
        public const string SaveExtension = ".ntwtf.zip";

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

        /// <summary>Puts a single save into the staged folder.</summary>
        /// <remarks>
        /// The thumbnail beside it is copied too when present. Without it the menu shows
        /// the slot with a blank image, which still loads but makes a screenshot
        /// comparison depend on whichever picture happened to be there.
        /// </remarks>
        /// <param name="savePath">The .ntwtf.zip to copy.</param>
        /// <param name="destination">The folder to copy it into.</param>
        /// <exception cref="FileNotFoundException">There is no such save.</exception>
        public static void CopyInto(string savePath, string destination)
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

            Directory.CreateDirectory(destination);
            File.Copy(
                savePath,
                Path.Combine(destination, Path.GetFileName(savePath)),
                overwrite: true);

            string thumbnail = savePath.Substring(0, savePath.Length - SaveExtension.Length) + ".jpg";
            if (File.Exists(thumbnail))
            {
                File.Copy(
                    thumbnail,
                    Path.Combine(destination, Path.GetFileName(thumbnail)),
                    overwrite: true);
            }
        }

    }
}
