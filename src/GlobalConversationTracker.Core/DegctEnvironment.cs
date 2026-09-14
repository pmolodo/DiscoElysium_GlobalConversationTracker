// SPDX-License-Identifier: MIT
using System;

namespace GlobalConversationTracker.Core
{
    /// <summary>
    /// Reading this project's environment variables, with the prefix supplied rather than
    /// typed.
    /// </summary>
    /// <remarks>
    /// <para>WHY A HELPER RATHER THAN A CONVENTION. Every environment variable this project
    /// defines is prefixed <c>DEGCT_</c> - see CLAUDE.md for the rule and
    /// <c>docs/environment.md</c> for the list - and a convention a person has to remember is
    /// one that grows exceptions. The rule exists because short generic names collide with
    /// names the shell already owns: <c>GROUPS</c> is a bash built-in array, assigning to it
    /// looks like it works, and every later read hands back a numeric group id. That cost time
    /// three separate times.</para>
    ///
    /// <para>So the prefix is applied here, and a new variable is named right because there is
    /// no other way to ask for one.</para>
    ///
    /// <para>THE ESCAPE HATCH IS <see cref="Foreign"/>, which reads a variable somebody else
    /// owns - PATH, CARGO_TARGET_DIR - under its own name. It is spelled differently on
    /// purpose: a reader can see at a glance which names this project invented and which it
    /// merely consumes.</para>
    /// </remarks>
    public static class DegctEnvironment
    {
        /// <summary>The prefix on every environment variable this project defines.</summary>
        public const string Prefix = "DEGCT_";

        /// <summary>One of ours, by its bare name, or <paramref name="fallback"/>.</summary>
        /// <param name="name">The bare name, without the prefix.</param>
        /// <param name="fallback">What to answer when it is unset or empty.</param>
        /// <returns>Its value, or the fallback.</returns>
        public static string? Get(string name, string? fallback = null)
        {
            string? value = Environment.GetEnvironmentVariable(Qualified(name));
            return string.IsNullOrEmpty(value) ? fallback : value;
        }

        /// <summary>Whether one of ours is set at all, whatever it is set to.</summary>
        /// <remarks>
        /// The shape a flag takes: several measurements switch on PRESENCE rather than value,
        /// so <c>DEGCT_NOLIMIT=1</c> and <c>DEGCT_NOLIMIT=</c> mean the same thing and neither
        /// has to be parsed.
        /// </remarks>
        /// <param name="name">The bare name, without the prefix.</param>
        /// <returns>Whether it is present.</returns>
        public static bool IsSet(string name) =>
            Environment.GetEnvironmentVariable(Qualified(name)) != null;

        /// <summary>One of ours as an integer, or <paramref name="fallback"/>.</summary>
        /// <param name="name">The bare name, without the prefix.</param>
        /// <param name="fallback">What to answer when it is unset or not a number.</param>
        /// <returns>Its value, or the fallback.</returns>
        public static int Number(string name, int fallback)
        {
            string? value = Get(name);
            return int.TryParse(value, out int parsed) ? parsed : fallback;
        }

        /// <summary>Sets one of ours for this process and the children it starts.</summary>
        /// <param name="name">The bare name, without the prefix.</param>
        /// <param name="value">What to set it to.</param>
        public static void Set(string name, string value) =>
            Environment.SetEnvironmentVariable(Qualified(name), value);

        /// <summary>A variable somebody else owns, read under its own name.</summary>
        /// <param name="name">The name exactly as its owner spells it.</param>
        /// <param name="fallback">What to answer when it is unset or empty.</param>
        /// <returns>Its value, or the fallback.</returns>
        public static string? Foreign(string name, string? fallback = null)
        {
            string? value = Environment.GetEnvironmentVariable(name);
            return string.IsNullOrEmpty(value) ? fallback : value;
        }

        /// <summary>The full name of one of ours.</summary>
        /// <remarks>
        /// Idempotent, because callers build names from both halves - a bare one they were
        /// given and a full one read back out of a message - and
        /// <c>DEGCT_DEGCT_CONVERSATION</c> would be unset, silently, and read as a default.
        /// </remarks>
        /// <param name="name">A bare or already-qualified name.</param>
        /// <returns>The qualified name.</returns>
        public static string Qualified(string name) =>
            name.StartsWith(Prefix, StringComparison.Ordinal) ? name : Prefix + name;
    }
}
