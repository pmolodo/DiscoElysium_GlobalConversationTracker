// SPDX-License-Identifier: MIT
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Surgical editing of Disco Elysium's Steam Cloud setting.</summary>
    public class SteamCloudTests
    {
        private const string AppId = SteamCloudOverride.DiscoElysiumAppId;

        /// <summary>A sharedconfig.vdf whose apps block says nothing about the game.</summary>
        private const string Untouched =
            "\"UserRoamingConfigStore\"\r\n"
            + "{\r\n"
            + "\t\"Software\"\r\n\t{\r\n\t\t\"Valve\"\r\n\t\t{\r\n"
            + "\t\t\t\"Steam\"\r\n\t\t\t{\r\n"
            + "\t\t\t\t\"StartMenuShortcutCheck\"\t\t\"1\"\r\n"
            + "\t\t\t\t\"apps\"\r\n\t\t\t\t{\r\n"
            + "\t\t\t\t\t\"1491340\"\r\n\t\t\t\t\t{\r\n"
            + "\t\t\t\t\t\t\"cloudenabled\"\t\t\"0\"\r\n"
            + "\t\t\t\t\t}\r\n"
            + "\t\t\t\t}\r\n"
            + "\t\t\t}\r\n\t\t}\r\n\t}\r\n"
            + "\t\"JSClientStorage\"\r\n\t{\r\n\t\t\"spotlight.review.632470\"\r\n"
            + "\t\t{\r\n\t\t\t\"rgPromptDismissals\"\t\t\"3\"\r\n\t\t}\r\n\t}\r\n"
            + "}\r\n";

        /// <summary>The same file once the game has a settings block of its own.</summary>
        private const string WithAppBlock =
            "\"UserRoamingConfigStore\"\r\n"
            + "{\r\n"
            + "\t\"Software\"\r\n\t{\r\n\t\t\"Valve\"\r\n\t\t{\r\n"
            + "\t\t\t\"Steam\"\r\n\t\t\t{\r\n"
            + "\t\t\t\t\"apps\"\r\n\t\t\t\t{\r\n"
            + "\t\t\t\t\t\"632470\"\r\n\t\t\t\t\t{\r\n"
            + "\t\t\t\t\t\t\"BadgeData\"\t\t\"02000000080c\"\r\n"
            + "\t\t\t\t\t}\r\n"
            + "\t\t\t\t}\r\n"
            + "\t\t\t}\r\n\t\t}\r\n\t}\r\n"
            + "}\r\n";

        [Fact]
        public void AMissingKeyMeansCloudSyncIsOn()
        {
            Assert.True(SteamCloud.IsEnabled(Untouched, AppId));
            Assert.True(SteamCloud.IsEnabled(WithAppBlock, AppId));
        }

        [Fact]
        public void DisablingAddsTheAppBlockAndRestoringRemovesIt()
        {
            string disabled = SteamCloud.SetEnabled(Untouched, AppId, enabled: false);

            Assert.False(SteamCloud.IsEnabled(disabled, AppId));
            Assert.Contains("\t\t\t\t\t\"632470\"\r\n", disabled);
            Assert.Equal(Untouched, SteamCloud.SetEnabled(disabled, AppId, enabled: true));
        }

        [Fact]
        public void DisablingChangesOnlyTheCloudKeyInTheAppsBlock()
        {
            string disabled = SteamCloud.SetEnabled(WithAppBlock, AppId, enabled: false);

            Assert.False(SteamCloud.IsEnabled(disabled, AppId));
            Assert.Contains("\"BadgeData\"\t\t\"02000000080c\"", disabled);
            Assert.Equal(WithAppBlock, SteamCloud.SetEnabled(disabled, AppId, enabled: true));
        }

        [Fact]
        public void AnotherAppsSettingsAreLeftAlone()
        {
            string disabled = SteamCloud.SetEnabled(Untouched, AppId, enabled: false);

            Assert.Contains("\"1491340\"", disabled);
            Assert.False(SteamCloud.IsEnabled(disabled, "1491340"));
            Assert.Contains("\"spotlight.review.632470\"", disabled);
        }

        [Fact]
        public void AlreadyDisabledIsUnchanged()
        {
            string disabled = SteamCloud.SetEnabled(WithAppBlock, AppId, enabled: false);

            Assert.Equal(disabled, SteamCloud.SetEnabled(disabled, AppId, enabled: false));
        }

        [Fact]
        public void AlreadyEnabledIsUnchanged()
        {
            Assert.Equal(Untouched, SteamCloud.SetEnabled(Untouched, AppId, enabled: true));
        }

        [Fact]
        public void InsertedLinesMatchTheFilesOwnLineEndings()
        {
            // Steam writes sharedconfig.vdf with plain LF, unlike localconfig.vdf.
            string source = Untouched.Replace("\r\n", "\n");

            string disabled = SteamCloud.SetEnabled(source, AppId, enabled: false);

            Assert.DoesNotContain("\r", disabled);
            Assert.False(SteamCloud.IsEnabled(disabled, AppId));
            Assert.Equal(source, SteamCloud.SetEnabled(disabled, AppId, enabled: true));
        }

        [Fact]
        public void AFileWithNoAppsBlockIsRefusedRatherThanEdited()
        {
            Assert.Throws<System.IO.InvalidDataException>(
                () => SteamCloud.SetEnabled("\"UserRoamingConfigStore\"\r\n{\r\n}\r\n", AppId, false));
        }
    }
}
