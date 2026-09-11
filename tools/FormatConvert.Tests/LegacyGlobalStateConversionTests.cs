// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Text;
using GlobalConversationTracker.Persistence;
using Xunit;

namespace FormatConvert.Tests
{
    /// <summary>Strict migration from every shape this repository has ever written.</summary>
    /// <remarks>
    /// HERE RATHER THAN BESIDE THE READER, because this tool is the only thing that knows
    /// what an older shape looks like. The mod refuses one and names the converter; what the
    /// converter then does with it is what these check.
    /// </remarks>
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
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{\"2\":\"9\"},"
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
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{\"3\":\"18\"},"
                + "\"WasDisplayed\":{\"3\":\"17\"}},\"orbs\":[\"Church\",\"Whirling-In-Rags\"]}",
                converted);
        }

        [Fact]
        public void ConvertLegacy_Version3_RunEncodesTheArraysItWroteAsArrays()
        {
            // THE VERSION THAT HAD NO READER AT ALL until de-bnjy.7. Version 3 grouped by
            // status and conversation, as 4 does, and wrote a plain ARRAY of entry IDs
            // where 4 writes a run-encoded string - so the runtime refused it and pointed
            // at the converter, and the converter refused it as an unknown legacy version.
            const string legacy =
                "{\"version\":3,\"conversations\":{\"WasDisplayed\":{\"10\":[5,6,7,9]},"
                + "\"WasOffered\":{\"2\":[9]}},\"orbs\":[\"Church\"]}";

            string converted = Convert(legacy);

            // The consecutive run collapses and the gap survives, which is the whole of
            // what version 4 changed.
            Assert.Equal(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{\"2\":\"9\"},"
                + "\"WasDisplayed\":{\"10\":\"5-7,9\"}},\"orbs\":[\"Church\"]}",
                converted);
        }

        [Fact]
        public void ConvertLegacy_Version3_BadElementCostsItsOwnRowRatherThanTheConversation()
        {
            // An array IS a list with elements in it, so one bad element costs that entry
            // and the rest of the array still says what it says - unlike a malformed run,
            // where nothing can be trusted. Either way the converter refuses to write a
            // partial file, which is what this actually checks.
            const string legacy =
                "{\"version\":3,\"conversations\":{\"WasDisplayed\":{\"10\":[5,\"six\",7]}},"
                + "\"orbs\":[]}";

            InvalidDataException error = Assert.Throws<InvalidDataException>(() => Convert(legacy));

            // REFUSED WHOLE rather than converted with the row dropped: a converted file
            // missing history nobody asked it to lose is worse than no file.
            Assert.Contains("Could not convert", error.Message, StringComparison.Ordinal);
        }

        [Fact]
        public void ConvertLegacy_Version4Array_IsRefusedRatherThanTreatedAsVersion3()
        {
            // The shape is decided by the VERSION, not by looking at the value. A version 4
            // file carrying an array is a damaged version 4 file, and sniffing the value
            // would quietly accept it as an old one and convert whatever it found.
            const string wrong =
                "{\"version\":4,\"conversations\":{\"WasDisplayed\":{\"10\":[5,6]}},\"orbs\":[]}";

            InvalidDataException error = Assert.Throws<InvalidDataException>(() => Convert(wrong));

            Assert.Contains("run-encoded string", error.Message, StringComparison.Ordinal);
        }

        [Fact]
        public void ConvertLegacy_Version4_IsBroughtForwardToTheHeaderedShape()
        {
            // The one version 5 added is the header itself: the file says which format it is
            // in as well as which version of it, and what follows is untouched.
            const string four =
                "{\"version\":4,\"conversations\":{\"WasDisplayed\":{\"10\":\"5-6\"}},\"orbs\":[]}";

            Assert.Equal(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,"
                + "\"conversations\":{\"WasDisplayed\":{\"10\":\"5-6\"}},\"orbs\":[]}",
                Convert(four));
        }

        [Fact]
        public void ConvertLegacy_InvalidJson_Throws()
        {
            InvalidDataException error = Assert.Throws<InvalidDataException>(() => Convert("{"));

            Assert.Contains("not valid JSON", error.Message, StringComparison.Ordinal);
        }

        [Fact]
        public void ConvertLegacy_UnreadableRow_ThrowsInsteadOfProducingPartialOutput()
        {
            const string legacy =
                "{\"version\":2,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\","
                + "\"18\":\"Nonsense\"}},\"orbs\":[]}";

            InvalidDataException error = Assert.Throws<InvalidDataException>(() => Convert(legacy));

            // REFUSED WHOLE rather than converted with the row dropped: a converted file
            // missing history nobody asked it to lose is worse than no file.
            Assert.Contains("Could not convert", error.Message, StringComparison.Ordinal);
            Assert.Contains("Nonsense", error.Message, StringComparison.Ordinal);
        }

        [Theory]
        [InlineData(0)]
        [InlineData(5)]
        [InlineData(99)]
        public void ConvertLegacy_UnsupportedVersion_Throws(int version)
        {
            string legacy = $"{{\"version\":{version},\"conversations\":{{}},\"orbs\":[]}}";

            InvalidDataException error = Assert.Throws<InvalidDataException>(() => Convert(legacy));

            Assert.Contains($"version {version}", error.Message, StringComparison.Ordinal);
        }

        private static string Convert(string legacy) =>
            Encoding.UTF8.GetString(
                LegacyGlobalState.ConvertToUtf8Bytes(
                    Encoding.UTF8.GetBytes(legacy), TestSource));
    }
}
