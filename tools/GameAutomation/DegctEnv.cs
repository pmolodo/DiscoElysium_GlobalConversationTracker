// SPDX-License-Identifier: MIT
using System;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Reads the environment variables this project defines, which are all prefixed
    /// <c>DEGCT_</c>.
    /// </summary>
    /// <remarks>
    /// <para>THE PREFIX IS APPLIED HERE RATHER THAN TYPED, so that a new variable is named
    /// right because there is no other way to ask for one. Every call takes the BARE name:
    /// <c>DegctEnv.IsSet("CHECK_DEPLOY")</c> asks about <c>DEGCT_CHECK_DEPLOY</c>. The rule
    /// and the reasoning are in CLAUDE.md, and docs/environment.md lists what exists; the
    /// same doors exist for Rust, Python, bash and PowerShell.</para>
    ///
    /// <para>IDEMPOTENT ABOUT AN ALREADY-QUALIFIED NAME, because callers build names from
    /// both halves - a bare one they were given and a full one read back out of a message -
    /// and <c>DEGCT_DEGCT_CHECK_DEPLOY</c> would be unset, silently, and read as a default.
    /// </para>
    ///
    /// <para>A variable somebody ELSE owns keeps its own spelling and is read through
    /// <see cref="Foreign"/>, so a call site says which of the two it means.</para>
    /// </remarks>
    public static class DegctEnv
    {
        /// <summary>The prefix on every variable this project defines.</summary>
        public const string Prefix = "DEGCT_";

        /// <summary>The full name of one of ours, from its bare one.</summary>
        /// <param name="name">The bare name, or an already-qualified one.</param>
        /// <returns>The name with the prefix on it, exactly once.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="name"/> is null.</exception>
        public static string Qualified(string name)
        {
            if (name == null)
            {
                throw new ArgumentNullException(nameof(name));
            }

            return name.StartsWith(Prefix, StringComparison.Ordinal) ? name : Prefix + name;
        }

        /// <summary>One of ours, by its BARE name.</summary>
        /// <param name="name">The bare name.</param>
        /// <returns>What it is set to, or null where it is not set.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="name"/> is null.</exception>
        public static string? Get(string name) =>
            Environment.GetEnvironmentVariable(Qualified(name));

        /// <summary>Whether one of ours is set to anything at all.</summary>
        /// <remarks>
        /// SET TO ANYTHING, empty included, because a switch is answered by its presence and
        /// asking a person to remember which values count is how a guard gets turned off by
        /// accident.
        /// </remarks>
        /// <param name="name">The bare name.</param>
        /// <returns>Whether it is set.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="name"/> is null.</exception>
        public static bool IsSet(string name) => Get(name) != null;

        /// <summary>A variable somebody else owns, under its own spelling.</summary>
        /// <param name="name">Its own name, whatever that is.</param>
        /// <returns>What it is set to, or null where it is not set.</returns>
        public static string? Foreign(string name) => Environment.GetEnvironmentVariable(name);
    }
}
