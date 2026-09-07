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
    /// <para>Small, and worth having anyway: what this class exists to prevent is a NEWER
    /// file being half-read by an OLDER reader, and a check that quietly passed everything
    /// would look exactly like a check that works right up until the day it matters.</para>
    ///
    /// <para>SINCE de-bnjy.3 IT REFUSES BOTH DIRECTIONS, and they are different failures.
    /// A file from the future is one this build cannot fully understand and must not touch;
    /// a file from the past is one the converter can bring forward. Only the second has a
    /// remedy, which is why only the second names a command.</para>
    /// </remarks>
    public sealed class FormatStampTests
    {
        [Fact]
        public void AFileFromThisBuildIsReadable()
        {
            FormatStamp.EnsureReadable("sparse", 3, 3);
        }

        [Fact]
        public void AnOlderFileIsRefusedAndTheMessageNamesTheConverter()
        {
            // THIS USED TO BE THE CASE THAT MUST NOT THROW, on the grounds that the point
            // of versioning a format is that old files keep working. de-bnjy.3 is the
            // decision to reverse that: old files keep working THROUGH A CONVERTER, and
            // not through a branch inside the live reader. A player's history should not
            // depend on a code path nothing else exercises - a converter is where an old
            // shape is written down and tested, and a legacy branch is where one rots.
            InvalidDataException refused = Assert.Throws<InvalidDataException>(
                () => FormatStamp.EnsureReadable("sparse", 1, 3));

            // A STRICT READER WITHOUT A SIGNPOSTED CONVERTER IS A WALL. "Your file is
            // version 1" is not an instruction; the command is.
            Assert.Contains("sparse", refused.Message);
            Assert.Contains(FormatStamp.Converter, refused.Message);
            Assert.Contains("not damaged", refused.Message);
        }

        [Fact]
        public void AnUnstampedFileIsReadableWhereTheCurrentVersionIsStillOne()
        {
            // Every format was stamped without changing its shape, so a file written
            // before the stamp existed is version 1 in fact rather than by convention -
            // and every Lua-side format is still at version 1, so every committed fixture
            // in this repository reads.
            FormatStamp.EnsureReadable("sparse", FormatStamp.Unstamped, 1);
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
