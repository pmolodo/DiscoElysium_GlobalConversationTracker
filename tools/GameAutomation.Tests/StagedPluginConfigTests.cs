// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Changing one setting in the mod's config and putting the file back.</summary>
    public class StagedPluginConfigTests : IDisposable
    {
        /// <summary>A config shaped the way BepInEx writes one.</summary>
        private const string Original =
            "## Settings file was created by plugin GlobalConversationTracker v0.1.0\n"
            + "## Plugin GUID: com.molodowitch.globalconversationtracker\n"
            + "\n"
            + "[Diagnostics]\n"
            + "\n"
            + "## Append a report to look-ahead-budget-overflows.log ...\n"
            + "# Setting type: Boolean\n"
            + "# Default value: false\n"
            + "LogLookAheadBudgetExceeded = false\n"
            + "\n"
            + "[Display]\n"
            + "\n"
            + "## Colour for dialogue options ...\n"
            + "# Setting type: String\n"
            + "# Default value: #FF8C42\n"
            + "NovelOptionColor = #FF8C42\n"
            + "\n"
            + "## The most search states one option's look-ahead may explore ...\n"
            + "# Setting type: Int32\n"
            + "# Default value: 200000\n"
            + "LookAheadMemoryBudgetMb = 256\n";

        private readonly string _root;
        private readonly string _game;
        private readonly string _config;

        /// <summary>Builds a game folder with a mod config in it.</summary>
        public StagedPluginConfigTests()
        {
            _root = Path.Combine(Path.GetTempPath(), "gct-cfg-" + Guid.NewGuid().ToString("N"));
            string install = Path.Combine(_root, "Disco Elysium");
            Directory.CreateDirectory(Path.Combine(install, "BepInEx", "config"));

            _game = Path.Combine(install, "disco.exe");
            File.WriteAllText(_game, "not really a game");

            _config = StagedPluginConfig.PathFor(_game);
            File.WriteAllText(_config, Original);
        }

        /// <summary>Removes the temporary install.</summary>
        public void Dispose()
        {
            if (Directory.Exists(_root))
            {
                Directory.Delete(_root, recursive: true);
            }
        }

        private static Dictionary<string, string> Settings(string name, string value) =>
            new Dictionary<string, string> { [name] = value };

        [Fact]
        public void ASettingIsChangedForTheLengthOfTheScope()
        {
            using (StagedPluginConfig.Apply(_game, Settings("LookAheadMemoryBudgetMb", "2")))
            {
                Assert.Equal(
                    "2", StagedPluginConfig.Get(File.ReadAllText(_config), "LookAheadMemoryBudgetMb"));
            }

            Assert.Equal(Original, File.ReadAllText(_config));
        }

        [Fact]
        public void EverythingElseInTheFileIsLeftAlone()
        {
            using (StagedPluginConfig.Apply(_game, Settings("LookAheadMemoryBudgetMb", "2")))
            {
                string updated = File.ReadAllText(_config);

                // The player's own choices, and the comments that explain them.
                Assert.Equal("#FF8C42", StagedPluginConfig.Get(updated, "NovelOptionColor"));
                Assert.Equal(
                    "false", StagedPluginConfig.Get(updated, "LogLookAheadBudgetExceeded"));
                Assert.Contains("## Plugin GUID:", updated);
                Assert.Contains("# Default value: 200000", updated);
                Assert.Contains("[Diagnostics]", updated);
            }
        }

        [Fact]
        public void SeveralSettingsChangeAtOnce()
        {
            var settings = new Dictionary<string, string>
            {
                ["LookAheadMemoryBudgetMb"] = "1",
                ["LogLookAheadBudgetExceeded"] = "true",
            };

            using (StagedPluginConfig.Apply(_game, settings))
            {
                string updated = File.ReadAllText(_config);
                Assert.Equal("1", StagedPluginConfig.Get(updated, "LookAheadMemoryBudgetMb"));
                Assert.Equal(
                    "true", StagedPluginConfig.Get(updated, "LogLookAheadBudgetExceeded"));
            }

            Assert.Equal(Original, File.ReadAllText(_config));
        }

        [Fact]
        public void ItIsPutBackEvenWhenTheRunThrows()
        {
            Assert.Throws<InvalidOperationException>((Action)(() =>
            {
                using (StagedPluginConfig.Apply(_game, Settings("LookAheadMemoryBudgetMb", "2")))
                {
                    throw new InvalidOperationException("the run failed");
                }
            }));

            Assert.Equal(Original, File.ReadAllText(_config));
        }

        [Fact]
        public void ASettingTheFileDoesNotHaveIsRefused()
        {
            // Appending it would be ignored by the game while the test believed it had
            // taken, so a name the file does not carry is a misspelling worth saying so.
            InvalidDataException error = Assert.Throws<InvalidDataException>(
                () => StagedPluginConfig.Apply(_game, Settings("LookAheadMemoryBudgetMbb", "2")));

            Assert.Contains("LookAheadMemoryBudgetMbb", error.Message);
            Assert.Equal(Original, File.ReadAllText(_config));
        }

        [Fact]
        public void ACommentedOutLineIsNotMistakenForASetting()
        {
            // Every setting in the file is preceded by "# Default value: ...", which
            // contains an equals sign and would match a careless parser.
            using (StagedPluginConfig.Apply(_game, Settings("LookAheadMemoryBudgetMb", "7")))
            {
                string updated = File.ReadAllText(_config);
                Assert.Contains("# Default value: 200000", updated);
                Assert.Equal("7", StagedPluginConfig.Get(updated, "LookAheadMemoryBudgetMb"));
            }
        }

        [Fact]
        public void RestoringTwiceIsHarmless()
        {
            StagedPluginConfig staged = StagedPluginConfig.Apply(
                _game, Settings("LookAheadMemoryBudgetMb", "2"));

            staged.Restore();
            staged.Restore();
            staged.Dispose();

            Assert.Equal(Original, File.ReadAllText(_config));
        }

        [Fact]
        public void AGameThatHasNeverRunTheModIsRefused()
        {
            File.Delete(_config);

            Assert.Throws<FileNotFoundException>(
                () => StagedPluginConfig.Apply(_game, Settings("LookAheadMemoryBudgetMb", "2")));
        }

        [Fact]
        public void NoSettingsIsRefused()
        {
            Assert.Throws<ArgumentNullException>(() => StagedPluginConfig.Apply(_game, null!));
        }
    }
}
