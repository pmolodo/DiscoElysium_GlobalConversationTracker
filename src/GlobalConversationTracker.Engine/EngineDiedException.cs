// SPDX-License-Identifier: MIT
using System;

namespace GlobalConversationTracker.Engine
{
    /// <summary>Why the look-ahead engine is no longer there.</summary>
    /// <remarks>
    /// The distinction is drawn because A PLAYER CAN ACT ON ONE OF THEM. Running out of
    /// memory is a reason to look at <c>LookAheadMemoryBudgetMb</c>; a crash is not, and
    /// telling somebody to lower a setting that had nothing to do with it is worse than
    /// telling them nothing.
    /// </remarks>
    public enum EngineDeath
    {
        /// <summary>
        /// It stopped, and nothing it said explains why.
        /// </summary>
        /// <remarks>
        /// The honest default. Guessing between the reasons on thin evidence would produce
        /// a confident message that is sometimes wrong, which is the one outcome worse than
        /// an unspecific one.
        /// </remarks>
        Crashed,

        /// <summary>
        /// It could not get the memory it asked for.
        /// </summary>
        /// <remarks>
        /// Claimed only on the engine's own words: Rust prints "memory allocation of N
        /// bytes failed" before aborting, and that line is on the stderr this captures.
        /// NOTE that an ordinary search running out of its BUDGET is not this - the engine
        /// reports that as an answer, with a verdict saying so, and stays running. This is
        /// the machine refusing, which no budget can prevent.
        /// </remarks>
        OutOfMemory,

        /// <summary>
        /// It is still there but stopped answering, and was stopped.
        /// </summary>
        /// <remarks>
        /// A read passed its deadline. Kept apart from <see cref="Crashed"/> because it
        /// describes a different fault - a process that is alive and wedged, rather than
        /// one that fell over - and because the engine was killed by us rather than by
        /// anything that went wrong inside it.
        /// </remarks>
        Unresponsive,
    }

    /// <summary>
    /// The look-ahead engine has gone, and is not coming back without a restart.
    /// </summary>
    /// <remarks>
    /// <para>DISTINCT FROM AN ORDINARY FAILURE, which is the whole reason it exists. A call
    /// that could not be served comes back as a response carrying an error, and a call that
    /// was refused comes back as a status; both leave the engine standing and the next menu
    /// answerable. This means the engine itself is gone, so the feature is over for this
    /// session - see de-bnjy.1.2, where the mod stops trying rather than restarting it.</para>
    ///
    /// <para>It carries what the engine said on its way out, because that is the only
    /// evidence there is: a log line quoting it is what turns a bug report about a missing
    /// asterisk into one about an allocation.</para>
    /// </remarks>
    public sealed class EngineDiedException : Exception
    {
        /// <summary>Creates one.</summary>
        /// <param name="death">What kind of ending it was.</param>
        /// <param name="message">What to say about it.</param>
        /// <param name="lastWords">The tail of the engine's stderr, or empty.</param>
        /// <param name="cause">The failure that revealed it, if there was one.</param>
        public EngineDiedException(
            EngineDeath death, string message, string lastWords = "", Exception? cause = null)
            : base(message, cause)
        {
            Death = death;
            LastWords = lastWords ?? string.Empty;
        }

        /// <summary>What kind of ending it was.</summary>
        public EngineDeath Death { get; }

        /// <summary>
        /// The last thing the engine wrote to its stderr, or empty if it said nothing.
        /// </summary>
        /// <remarks>
        /// Bounded to the tail rather than the whole stream: a process that dies says what
        /// matters last, and keeping everything would mean holding whatever a wedged engine
        /// chose to print for as long as it ran.
        /// </remarks>
        public string LastWords { get; }
    }
}
