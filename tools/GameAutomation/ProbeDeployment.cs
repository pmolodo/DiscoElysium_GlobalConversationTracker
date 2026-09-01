// SPDX-License-Identifier: MIT
using System;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Puts the test probe plugin into a game install for the length of a run, and
    /// takes it out again.
    /// </summary>
    /// <remarks>
    /// <para>A scope rather than a pair of calls, so the removal cannot be forgotten:
    /// the probe writes into the player's own BepInEx folder, and one left behind would
    /// go on narrating every ordinary session afterwards.</para>
    ///
    /// <para>It must be opened INSIDE a <see cref="SteamCloudOverride"/>. The install
    /// folder is not cloud-synced, but the profile is, and a run that writes a probe
    /// before cloud sync is off can have its staged profile replaced underneath it
    /// mid-test. Ordering the scopes is how that is avoided; nothing here can check it,
    /// so it is stated where it is done.</para>
    ///
    /// <para>A probe already sitting in the folder is refused rather than reused. It
    /// means an earlier run was killed before it could clean up, and the DLL there is
    /// of unknown age - quietly running against a stale build is how a test starts
    /// reporting on code that is no longer written.</para>
    /// </remarks>
    public sealed class ProbeDeployment : IDisposable
    {
        /// <summary>The probe assembly's file name.</summary>
        public const string ProbeFileName = "GlobalConversationTrackerTestProbe.dll";

        /// <summary>Where BepInEx looks for plugins, under the game folder.</summary>
        public const string PluginsFolderName = "plugins";

        private readonly Action<string>? _progress;
        private bool _removed;

        private ProbeDeployment(string deployedPath, Action<string>? progress)
        {
            DeployedPath = deployedPath;
            _progress = progress;
        }

        /// <summary>Where the probe was copied to.</summary>
        public string DeployedPath { get; }

        /// <summary>The plugins folder of a game install.</summary>
        /// <param name="gameExecutable">Path to disco.exe.</param>
        /// <exception cref="ArgumentException">The path has no folder.</exception>
        public static string PluginsFolder(string gameExecutable)
        {
            return Path.Combine(
                FilePaths.FolderOf(gameExecutable, nameof(gameExecutable)),
                "BepInEx",
                PluginsFolderName);
        }

        /// <summary>Copies the probe in, and removes it when the scope is disposed.</summary>
        /// <param name="gameExecutable">Path to disco.exe.</param>
        /// <param name="probePath">The built probe assembly to install.</param>
        /// <param name="progress">Called with each step, for verbose output.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="FileNotFoundException">The probe assembly is not there.</exception>
        /// <exception cref="DirectoryNotFoundException">The game has no plugins folder.</exception>
        /// <exception cref="InvalidOperationException">A probe is already installed.</exception>
        public static ProbeDeployment Deploy(
            string gameExecutable, string probePath, Action<string>? progress = null)
        {
            if (probePath == null)
            {
                throw new ArgumentNullException(nameof(probePath));
            }

            if (!File.Exists(probePath))
            {
                throw new FileNotFoundException(
                    $"No test probe at {probePath}. Build "
                    + "src/GlobalConversationTracker.TestProbe first.",
                    probePath);
            }

            string plugins = PluginsFolder(gameExecutable);
            if (!Directory.Exists(plugins))
            {
                throw new DirectoryNotFoundException(
                    $"No BepInEx plugins folder at {plugins}. The in-game tests need a "
                    + "BepInEx-modded install, since the probe is a BepInEx plugin.");
            }

            string destination = Path.Combine(plugins, ProbeFileName);
            if (File.Exists(destination))
            {
                throw new InvalidOperationException(
                    $"A test probe is already installed at {destination}. An earlier run was "
                    + "killed before it could remove it; delete it and try again, rather than "
                    + "letting this run report on whatever build that is.");
            }

            File.Copy(probePath, destination);
            progress?.Invoke($"installed the test probe at {destination}");
            return new ProbeDeployment(destination, progress);
        }

        /// <summary>Removes the probe. Safe to call more than once.</summary>
        /// <exception cref="IOException">
        /// The probe could not be removed, which matters enough to report: it would
        /// otherwise narrate the player's own sessions from then on.
        /// </exception>
        public void Remove()
        {
            if (_removed)
            {
                return;
            }

            _removed = true;
            if (!File.Exists(DeployedPath))
            {
                return;
            }

            File.Delete(DeployedPath);
            _progress?.Invoke($"removed the test probe from {DeployedPath}");
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            Remove();
        }
    }
}
