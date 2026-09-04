// SPDX-License-Identifier: MIT
using System.IO;

using GlobalConversationTracker.Core;

using Xunit;

namespace GlobalConversationTracker.Core.Tests
{
    /// <summary>
    /// The one rule every format in this repository reads its own version by.
    /// </summary>
    /// <remarks>
    /// Small, and worth having anyway: what this class exists to prevent is a NEWER file
    /// being half-read by an OLDER reader, and a check that quietly passed everything would
    /// look exactly like a check that works right up until the day it matters.
    /// </remarks>
    public sealed class FormatStampTests
    {
        [Fact]
        public void AFileFromThisBuildIsReadable()
        {
            FormatStamp.EnsureReadable("sparse", 3, 3);
        }

        [Fact]
        public void AnOlderFileIsReadable()
        {
            // The whole point of versioning a format is that old files keep working, so
            // this is the case that must NOT throw.
            FormatStamp.EnsureReadable("sparse", 1, 3);
        }

        [Fact]
        public void AnUnstampedFileIsReadableWhateverTheCurrentVersion()
        {
            // Every format was stamped without changing its shape, so a file written
            // before the stamp existed is version 1 in fact rather than by convention.
            FormatStamp.EnsureReadable("sparse", FormatStamp.Unstamped, 4);
        }

        [Fact]
        public void AFileFromANewerBuildIsRefused()
        {
            InvalidDataException refused = Assert.Throws<InvalidDataException>(
                () => FormatStamp.EnsureReadable("sparse-diff", 5, 4));

            // The message has to carry BOTH numbers and say not to overwrite it: whoever
            // reads it is holding a file that is fine and a program that is old, and the
            // damaging response - replace it from something staler - is the tempting one.
            Assert.Contains("sparse-diff", refused.Message);
            Assert.Contains("5", refused.Message);
            Assert.Contains("4", refused.Message);
            Assert.Contains("not damaged", refused.Message);
        }
    }
}
