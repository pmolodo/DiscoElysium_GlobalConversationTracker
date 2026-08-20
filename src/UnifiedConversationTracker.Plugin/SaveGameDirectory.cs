using System;
using System.IO;
using BepInEx.Logging;
using UnityEngine;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// Resolves the directory the game keeps its saves in, which is where the
    /// unified state file lives.
    /// </summary>
    /// <remarks>
    /// Both calls were verified callable from a BepInEx 6 IL2CPP plugin against
    /// this game build's own interop assemblies.
    /// <c>SunshinePersistenceFileManager.GetSaveGameDirectoryPath()</c> is literally
    /// what the game runs (<c>Application.persistentDataPath + "/SaveGames/"</c>),
    /// so it is preferred; it is also coupled to the Assembly-CSharp interop shape,
    /// which is regenerated per game build and would break on a rename, hence the
    /// fallback. Nothing here is Windows-only: Unity maps persistentDataPath to the
    /// platform's own location.
    /// </remarks>
    internal static class SaveGameDirectory
    {
        /// <summary>The subdirectory name the game appends to its data path.</summary>
        private const string SaveGameFolderName = "SaveGames";

        /// <summary>
        /// Resolves the SaveGames directory. Must be called from plugin
        /// <c>Load()</c> or later, once the IL2CPP domain is attached.
        /// </summary>
        public static string Resolve(ManualLogSource log)
        {
            if (log == null)
            {
                throw new ArgumentNullException(nameof(log));
            }

            try
            {
                string gameDirectory = SunshinePersistenceFileManager.GetSaveGameDirectoryPath();
                if (!string.IsNullOrWhiteSpace(gameDirectory))
                {
                    return gameDirectory;
                }

                log.LogWarning(
                    "SunshinePersistenceFileManager.GetSaveGameDirectoryPath() returned nothing; "
                    + "falling back to Application.persistentDataPath.");
            }
            catch (Exception ex)
            {
                log.LogWarning(
                    $"SunshinePersistenceFileManager.GetSaveGameDirectoryPath() failed ({ex.Message}); "
                    + "falling back to Application.persistentDataPath.");
            }

            return Path.Combine(Application.persistentDataPath, SaveGameFolderName);
        }
    }
}
