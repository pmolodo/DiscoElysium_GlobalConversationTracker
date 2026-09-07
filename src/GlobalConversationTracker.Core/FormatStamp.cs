// SPDX-License-Identifier: MIT
using System.Globalization;
using System.IO;

namespace GlobalConversationTracker.Core;

/// <summary>
/// What a file this repository wrote says about its own format, and the one rule for
/// reading it.
/// </summary>
/// <remarks>
/// <para>THE REFUSAL IS THE POINT, and it is the reason this is shared rather than copied.
/// A version nobody checks is decoration; the failure a check prevents is a NEW writer's
/// file being half-read by an OLD reader, which is exactly the failure that is hard to
/// diagnose from the wreckage - the file parses, some of it means something, and nothing
/// says why the rest is missing. <see cref="EnsureReadable"/> is that check, written once.
/// </para>
///
/// <para>TWO PROPERTY NAMES, DELIBERATELY. The Lua-side formats - the sparse tables, the
/// sparse diffs, the expanded-save manifest - already carry a <c>_format</c> naming which
/// representation they are in, so their version sits beside it as
/// <see cref="PropertyName"/>. The global state file is not a Lua table and has carried a
/// plain <c>version</c> at its root through four versions, which is load-bearing across
/// real player files; renaming that for tidiness would be a format break bought with
/// nothing.</para>
///
/// <para>AND TWO WAYS OF REFUSING, which is the more interesting difference and is also
/// deliberate. This THROWS, which is right for a tool: a command that cannot read its input
/// should stop, loudly, before it writes anything. <c>GlobalStateJson</c> returns an
/// <c>UnsupportedVersion</c> RESULT instead, because its reader runs inside the game and an
/// exception there is a player's session, not a message - and because the caller has a
/// decision to make that a throw would take away: a file from a newer build is full of real
/// history, so it must not be overwritten and must not be replaced from a stale backup
/// either. What the two share is the question and the answer to it; how a caller is told is
/// a property of where the caller runs.</para>
///
/// <para>AN UNSTAMPED FILE IS VERSION 1, everywhere, and that is the answer to the files
/// already sitting on disk. Every format that gains a stamp gains it while its shape is
/// unchanged, so the files written before it are version 1 in fact as well as by
/// convention - a reader that refused them would be refusing files it can read perfectly.
/// A later version that genuinely cannot be read by an old reader is what the stamp is
/// for, and that is the direction it protects.</para>
/// </remarks>
public static class FormatStamp
{
    /// <summary>What a Lua-side file calls its format version, beside its <c>_format</c>.</summary>
    public const string PropertyName = "_formatVersion";

    /// <summary>What a file with no stamp is taken to be.</summary>
    /// <remarks>
    /// See the remarks on this class: every format was stamped without changing its shape,
    /// so this is what those files are rather than a guess about them.
    /// </remarks>
    public const int Unstamped = 1;

    /// <summary>How to bring a file that is refused up to date, named in every message.</summary>
    /// <remarks>
    /// A STRICT READER WITHOUT A SIGNPOSTED CONVERTER IS A WALL, which is de-bnjy.3's own
    /// warning about itself. The refusal below is only useful if the person reading it can
    /// act on it, and "your file is version 2" is not an instruction.
    /// </remarks>
    public const string Converter = "dotnet run --project tools/FormatConvert -- <file>";

    /// <summary>
    /// Checks a file's version against the build's, and refuses anything but the current one.
    /// </summary>
    /// <remarks>
    /// <para>BOTH DIRECTIONS, and they are different failures with different remedies. A
    /// file from the FUTURE is one this build cannot fully understand and must not touch. A
    /// file from the PAST is one the converter can bring forward, and the message says so.
    /// </para>
    ///
    /// <para>REFUSING THE PAST IS THE POINT OF de-bnjy.3, and it is what this method gained
    /// there. It used to accept anything not newer, which meant an old shape was read by
    /// whatever branch happened to still handle it - and a legacy branch inside a live
    /// reader is a place where an old shape ROTS, because nothing else exercises it. A
    /// converter is a place where one is written down and tested.</para>
    ///
    /// <para>IT REFUSES NOTHING TODAY. Every Lua-side format is at version 1 and an
    /// unstamped file is version 1, so no committed fixture and no file this repository has
    /// written is turned away. That is the right moment to make a reader strict: the rule
    /// is in place before there is a second version for it to be wrong about.</para>
    /// </remarks>
    /// <param name="format">What the format is called, for the message.</param>
    /// <param name="found">The version the file records, or <see cref="Unstamped"/>.</param>
    /// <param name="current">The version this build writes.</param>
    /// <exception cref="InvalidDataException">
    /// The file is not the current version. NOT treated as corruption in either direction:
    /// the file is fine and this program is old, or the file is old and convertible, and
    /// neither says overwrite it.
    /// </exception>
    public static void EnsureReadable(string format, int found, int current)
    {
        if (found > current)
        {
            throw new InvalidDataException(
                $"This {format} file is version "
                + found.ToString(CultureInfo.InvariantCulture)
                + ", and this build reads version "
                + current.ToString(CultureInfo.InvariantCulture)
                + " at most. It was written by a newer build; the file is not damaged, so "
                + "do not overwrite it - use a build at least as new as the one that wrote "
                + "it.");
        }

        if (found < current)
        {
            throw new InvalidDataException(
                $"This {format} file is version "
                + found.ToString(CultureInfo.InvariantCulture)
                + ", and this build reads only version "
                + current.ToString(CultureInfo.InvariantCulture)
                + ". It is not damaged and nothing in it is lost - convert it first:\n  "
                + Converter);
        }
    }
}
