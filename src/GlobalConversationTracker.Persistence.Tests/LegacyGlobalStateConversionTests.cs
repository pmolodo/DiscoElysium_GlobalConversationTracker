// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Text;
using Xunit;

namespace GlobalConversationTracker.Persistence.Tests
{
    /// <summary>Strict migration from the two legacy per-entry formats.</summary>
    public class LegacyGlobalStateConversionTests
    {
        private const string TestSource = "test";

        [Fact]
        public void ConvertLegacy_Version1_PreservesEntriesAndAddsEmptyOrbs()
        {
            const string legacy =
                "{\"version\":1,\"conversations\":{\"10\":{\"5\":\"WasDisplayed\"},"
                + "\"2\":{\"9\":\"WasOffered\"}}}";

            string converted = Convert(legacy);

            Assert.Equal(
                "{\"version\":4,\"conversations\":{\"WasOffered\":{\"2\":\"9\"},"
                + "\"WasDisplayed\":{\"10\":\"5\"}},\"orbs\":[]}",
                converted);
        }

        [Fact]
        public void ConvertLegacy_Version2_PreservesEntriesAndOrbs()
        {
            const string legacy =
                "{\"version\":2,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\","
                + "\"18\":\"WasOffered\"}},\"orbs\":[\"Whirling-In-Rags\",\"Church\"]}";

            string converted = Convert(legacy);

            Assert.Equal(
                "{\"version\":4,\"conversations\":{\"WasOffered\":{\"3\":\"18\"},"
                + "\"WasDisplayed\":{\"3\":\"17\"}},\"orbs\":[\"Church\",\"Whirling-In-Rags\"]}",
                converted);
        }

        [Fact]
        public void ConvertLegacy_InvalidJson_Throws()
        {
            InvalidDataException error = Assert.Throws<InvalidDataException>(() => Convert("{"));

            Assert.Contains("Not valid JSON", error.Message, StringComparison.Ordinal);
        }

        [Fact]
        public void ConvertLegacy_UnreadableRow_ThrowsInsteadOfProducingPartialOutput()
        {
            const string legacy =
                "{\"version\":2,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\","
                + "\"18\":\"Nonsense\"}},\"orbs\":[]}";

            InvalidDataException error = Assert.Throws<InvalidDataException>(() => Convert(legacy));

            Assert.Contains("1 unreadable row(s) would be lost", error.Message, StringComparison.Ordinal);
            Assert.Contains("Nonsense", error.Message, StringComparison.Ordinal);
        }

        [Theory]
        [InlineData(0)]
        [InlineData(3)]
        [InlineData(99)]
        public void ConvertLegacy_UnsupportedVersion_Throws(int version)
        {
            string legacy = $"{{\"version\":{version},\"conversations\":{{}},\"orbs\":[]}}";

            InvalidDataException error = Assert.Throws<InvalidDataException>(() => Convert(legacy));

            Assert.Contains($"format version {version}", error.Message, StringComparison.Ordinal);
        }

        private static string Convert(string legacy) =>
            Encoding.UTF8.GetString(
                GlobalStateJson.ConvertLegacyToUtf8Bytes(
                    Encoding.UTF8.GetBytes(legacy), TestSource));
    }
}
