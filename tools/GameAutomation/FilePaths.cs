// SPDX-License-Identifier: MIT
using System;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>Path questions the harness asks more than once.</summary>
    public static class FilePaths
    {
        /// <summary>
        /// The folder an executable lives in, which is where the things beside it are
        /// looked for - Steam's userdata, the game's BepInEx folder.
        /// </summary>
        /// <remarks>
        /// <see cref="Path.GetDirectoryName(string)"/> answers null for a bare filename
        /// and for a drive root, and neither is an install. Letting that null become an
        /// empty path would turn "you gave me the wrong path" into "no Steam account has
        /// this game" or "there is no log here", which sends the reader looking in
        /// entirely the wrong place.
        /// </remarks>
        /// <param name="executablePath">The executable.</param>
        /// <param name="parameterName">What to name in the exception.</param>
        /// <exception cref="ArgumentException">The path has no folder.</exception>
        public static string FolderOf(string executablePath, string parameterName)
        {
            string? folder = Path.GetDirectoryName(executablePath);
            if (string.IsNullOrEmpty(folder))
            {
                throw new ArgumentException(
                    $"'{executablePath}' has no folder to look beside; an absolute path to "
                    + "the executable is needed.",
                    parameterName);
            }

            return folder!;
        }

        /// <summary>
        /// Reads a file something else is still writing to.
        /// </summary>
        /// <remarks>
        /// BepInEx holds its log open for the whole run, and the mod rewrites its
        /// diagnostics as it goes, so a plain read of either fails with a sharing
        /// violation while the game is up - which is exactly when a harness wants to look.
        /// </remarks>
        /// <param name="path">The file.</param>
        /// <exception cref="ArgumentNullException"><paramref name="path"/> is null.</exception>
        public static string ReadShared(string path)
        {
            if (path == null)
            {
                throw new ArgumentNullException(nameof(path));
            }

            using var stream = new FileStream(
                path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
            using var reader = new StreamReader(stream);
            return reader.ReadToEnd();
        }
    }
}
