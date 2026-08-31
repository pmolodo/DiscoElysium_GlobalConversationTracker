// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Editing Steam's per-game cloud toggle inside localconfig.vdf.
    /// </summary>
    /// <remarks>
    /// The fixture mirrors the real file's shape, decoys included: the same app id appears
    /// as a plain key/value in the licenses and language sections, and only one occurrence
    /// is the settings block. Getting that wrong would corrupt a licence cache.
    /// </remarks>
    public class SteamCloudTests
    {
        private const string AppId = "632470";

        /// <summary>A cut-down localconfig.vdf with the same traps as the real one.</summary>
        private static string Fixture(string appBody)
        {
            return string.Join("\r\n", new[]
            {
                "\"UserLocalConfigStore\"",
                "{",
                "\t\"Licenses\"",
                "\t{",
                "\t\t\"632470\"\t\t\"3200000004000000bc245201\"",
                "\t\t\"1491340\"\t\t\"3200000004000000bc245202\"",
                "\t}",
                "\t\"Software\"",
                "\t{",
                "\t\t\"Valve\"",
                "\t\t{",
                "\t\t\t\"Steam\"",
                "\t\t\t{",
                "\t\t\t\t\"apps\"",
                "\t\t\t\t{",
                "\t\t\t\t\t\"632470\"",
                "\t\t\t\t\t{",
                appBody,
                "\t\t\t\t\t}",
                "\t\t\t\t\t\"1491340\"",
                "\t\t\t\t\t{",
                "\t\t\t\t\t\t\"LastPlayed\"\t\t\"1788000000\"",
                "\t\t\t\t\t}",
                "\t\t\t\t}",
                "\t\t\t}",
                "\t\t}",
                "\t}",
                "\t\"Languages\"",
                "\t{",
                "\t\t\"632470\"\t\t\"00696e7465726e616c00\"",
                "\t}",
                "}",
                string.Empty,
            });
        }

        private const string Played =
            "\t\t\t\t\t\t\"LastPlayed\"\t\t\"1788157123\"\r\n"
            + "\t\t\t\t\t\t\"cloud\"\r\n"
            + "\t\t\t\t\t\t{\r\n"
            + "\t\t\t\t\t\t\t\"last_sync_state\"\t\t\"synchronized\"\r\n"
            + "\t\t\t\t\t\t}";

        [Fact]
        public void AnAbsentKeyMeansCloudIsEnabled()
        {
            // Steam only writes the key when the box is UNCHECKED, so absent means on.
            Assert.True(SteamCloud.IsEnabled(Fixture(Played), AppId));
            Assert.Null(SteamCloud.ReadKey(Fixture(Played), AppId, SteamCloud.CloudEnabledKey));
        }

        [Fact]
        public void DisablingWritesTheKeyIntoTheAppBlock()
        {
            string before = Fixture(Played);

            string after = SteamCloud.SetEnabled(before, AppId, enabled: false);

            Assert.False(SteamCloud.IsEnabled(after, AppId));
            Assert.Equal("0", SteamCloud.ReadKey(after, AppId, SteamCloud.CloudEnabledKey));
        }

        /// <summary>
        /// The decoys are the point: the id also appears in Licenses and Languages, and
        /// editing either of those would corrupt something unrelated.
        /// </summary>
        [Fact]
        public void DisablingLeavesEveryOtherOccurrenceAlone()
        {
            string before = Fixture(Played);

            string after = SteamCloud.SetEnabled(before, AppId, enabled: false);

            Assert.Contains("\"632470\"\t\t\"3200000004000000bc245201\"", after);
            Assert.Contains("\"632470\"\t\t\"00696e7465726e616c00\"", after);
            Assert.Contains("\"1491340\"\t\t\"3200000004000000bc245202\"", after);

            // The neighbouring app is untouched, key and block alike.
            Assert.Contains("\"LastPlayed\"\t\t\"1788000000\"", after);
        }

        [Fact]
        public void TheAppsOwnContentSurvives()
        {
            string after = SteamCloud.SetEnabled(Fixture(Played), AppId, enabled: false);

            Assert.Contains("\"LastPlayed\"\t\t\"1788157123\"", after);
            Assert.Contains("\"last_sync_state\"\t\t\"synchronized\"", after);
        }

        /// <summary>
        /// Restoring removes the key rather than writing "1", because that is what Steam
        /// itself does. A leftover "cloudenabled" "1" would be a key Steam never writes.
        /// </summary>
        [Fact]
        public void RestoringRemovesTheKeyEntirely()
        {
            string original = Fixture(Played);
            string disabled = SteamCloud.SetEnabled(original, AppId, enabled: false);

            string restored = SteamCloud.SetEnabled(disabled, AppId, enabled: true);

            Assert.DoesNotContain(SteamCloud.CloudEnabledKey, restored);
            Assert.True(SteamCloud.IsEnabled(restored, AppId));
        }

        /// <summary>The whole point of a modify/restore pair: it must round-trip exactly.</summary>
        [Fact]
        public void DisableThenRestoreIsByteForByteTheOriginal()
        {
            string original = Fixture(Played);

            string restored = SteamCloud.SetEnabled(
                SteamCloud.SetEnabled(original, AppId, enabled: false), AppId, enabled: true);

            Assert.Equal(original, restored);
        }

        [Fact]
        public void DisablingTwiceChangesNothingTheSecondTime()
        {
            string once = SteamCloud.SetEnabled(Fixture(Played), AppId, enabled: false);

            Assert.Equal(once, SteamCloud.SetEnabled(once, AppId, enabled: false));
        }

        [Fact]
        public void RestoringWhenAlreadyEnabledChangesNothing()
        {
            string original = Fixture(Played);

            Assert.Equal(original, SteamCloud.SetEnabled(original, AppId, enabled: true));
        }

        [Fact]
        public void APreExistingDisabledSettingIsPreserved()
        {
            // If the player already had cloud off, restoring must leave it off - putting it
            // back the way it was, not the way this tool would have set it.
            string body = "\t\t\t\t\t\t\"cloudenabled\"\t\t\"0\"\r\n" + Played;
            string original = Fixture(body);

            Assert.False(SteamCloud.IsEnabled(original, AppId));
            Assert.Equal(original, SteamCloud.SetEnabled(original, AppId, enabled: false));
        }

        [Fact]
        public void AnUnknownAppIsRefusedRatherThanGuessed()
        {
            Assert.Throws<InvalidDataException>(
                () => SteamCloud.SetEnabled(Fixture(Played), "999999", enabled: false));
        }

        [Fact]
        public void FindingNoLocalConfigsIsNotAnError()
        {
            Assert.Empty(SteamCloud.FindLocalConfigs(Path.GetTempPath()));
        }
    }
}
