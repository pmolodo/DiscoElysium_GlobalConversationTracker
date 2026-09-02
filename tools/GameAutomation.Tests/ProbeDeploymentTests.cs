// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Installing the test probe into a game folder, and taking it out.</summary>
    public class ProbeDeploymentTests : IDisposable
    {
        private readonly string _root;
        private readonly string _game;
        private readonly string _plugins;
        private readonly string _probe;

        /// <summary>Builds a game folder shaped like a BepInEx install.</summary>
        public ProbeDeploymentTests()
        {
            _root = Path.Combine(Path.GetTempPath(), "gct-probe-" + Guid.NewGuid().ToString("N"));
            string install = Path.Combine(_root, "Disco Elysium");
            _plugins = Path.Combine(install, "BepInEx", "plugins");
            Directory.CreateDirectory(_plugins);

            _game = Path.Combine(install, "disco.exe");
            File.WriteAllText(_game, "not really a game");

            _probe = Path.Combine(_root, ProbeDeployment.ProbeFileName);
            File.WriteAllText(_probe, "not really a plugin");
        }

        /// <summary>Removes the temporary install.</summary>
        public void Dispose()
        {
            if (Directory.Exists(_root))
            {
                Directory.Delete(_root, recursive: true);
            }
        }

        private string Installed => Path.Combine(_plugins, ProbeDeployment.ProbeFileName);

        [Fact]
        public void TheProbeIsInstalledForTheLengthOfTheScope()
        {
            using (ProbeDeployment deployment = ProbeDeployment.Deploy(_game, _probe))
            {
                Assert.True(File.Exists(Installed));
                Assert.Equal(Installed, deployment.DeployedPath);
                Assert.Equal("not really a plugin", File.ReadAllText(Installed));
            }

            Assert.False(File.Exists(Installed));
        }

        [Fact]
        public void ItIsRemovedEvenWhenTheRunThrows()
        {
            // Cast because a block lambda that only ever throws also matches xunit's
            // async overload, which is obsolete and would not run this at all.
            Assert.Throws<InvalidOperationException>((Action)(() =>
            {
                using (ProbeDeployment.Deploy(_game, _probe))
                {
                    throw new InvalidOperationException("the run failed");
                }
            }));

            Assert.False(File.Exists(Installed));
        }

        [Fact]
        public void RemovingTwiceIsHarmless()
        {
            ProbeDeployment deployment = ProbeDeployment.Deploy(_game, _probe);

            deployment.Remove();
            deployment.Remove();
            deployment.Dispose();

            Assert.False(File.Exists(Installed));
        }

        [Fact]
        public void SomethingElseDeletingItFirstIsNotAnError()
        {
            using (ProbeDeployment.Deploy(_game, _probe))
            {
                File.Delete(Installed);
            }
        }

        [Fact]
        public void AProbeLeftByAKilledRunIsReplacedWhenTheGameIsNotRunning()
        {
            File.WriteAllText(Installed, "a probe from some older build");

            using (ProbeDeployment deployment = ProbeDeployment.Deploy(
                _game, _probe, isGameRunning: () => false))
            {
                Assert.Equal(Installed, deployment.DeployedPath);
                Assert.Equal("not really a plugin", File.ReadAllText(Installed));
            }

            Assert.False(File.Exists(Installed));
        }

        [Fact]
        public void AProbeIsRefusedWhileTheGameIsRunning()
        {
            File.WriteAllText(Installed, "a probe currently loaded by the game");

            InvalidOperationException error = Assert.Throws<InvalidOperationException>(
                () => ProbeDeployment.Deploy(_game, _probe, isGameRunning: () => true));

            Assert.Contains("already installed", error.Message);
            Assert.Equal("a probe currently loaded by the game", File.ReadAllText(Installed));
        }

        [Fact]
        public void AnUnbuiltProbeIsRefused()
        {
            Assert.Throws<FileNotFoundException>(
                () => ProbeDeployment.Deploy(_game, Path.Combine(_root, "not-built.dll")));
        }

        [Fact]
        public void AGameWithoutBepInExIsRefused()
        {
            Directory.Delete(_plugins, recursive: true);

            Assert.Throws<DirectoryNotFoundException>(
                () => ProbeDeployment.Deploy(_game, _probe));
        }

        [Fact]
        public void NoProbePathIsRefused()
        {
            Assert.Throws<ArgumentNullException>(() => ProbeDeployment.Deploy(_game, null!));
        }

        [Fact]
        public void ThePluginsFolderSitsUnderTheGameFolder()
        {
            Assert.Equal(_plugins, ProbeDeployment.PluginsFolder(_game));
        }
    }
}
