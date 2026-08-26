using System;
using System.IO;

namespace GlobalConversationTracker.Persistence.Tests
{
    /// <summary>
    /// A scratch directory that stands in for the game's SaveGames directory,
    /// deleted when the test finishes.
    /// </summary>
    internal sealed class TempDirectory : IDisposable
    {
        public TempDirectory()
        {
            Path = System.IO.Path.Combine(
                System.IO.Path.GetTempPath(),
                "uct-persistence-tests",
                Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(Path);
        }

        /// <summary>The directory, with no trailing separator.</summary>
        public string Path { get; }

        /// <summary>
        /// The directory with a trailing separator, the way
        /// <c>SunshinePersistenceFileManager.GetSaveGameDirectoryPath()</c> returns it.
        /// </summary>
        public string PathWithTrailingSeparator => Path + System.IO.Path.DirectorySeparatorChar;

        public GlobalStateStore CreateStore() => new GlobalStateStore(Path);

        public void Dispose()
        {
            try
            {
                Directory.Delete(Path, recursive: true);
            }
            catch (DirectoryNotFoundException)
            {
                // Nothing to clean up.
            }
        }
    }
}
