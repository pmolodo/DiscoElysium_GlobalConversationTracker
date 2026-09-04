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

    /// <summary>
    /// Checks a file's version against the build's, and refuses one from the future.
    /// </summary>
    /// <param name="format">What the format is called, for the message.</param>
    /// <param name="found">The version the file records, or <see cref="Unstamped"/>.</param>
    /// <param name="current">The version this build writes.</param>
    /// <exception cref="InvalidDataException">
    /// The file is newer than the build. NOT treated as corruption: the file is fine and
    /// this program is old, and the two want opposite responses - one says overwrite it,
    /// the other says do not touch it.
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
    }
}
