// SPDX-License-Identifier: MIT
using System;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// A test that needs Disco Elysium installed and a desktop to drive. Skipped
    /// unless explicitly opted into.
    /// </summary>
    /// <remarks>
    /// <para>These are fragile in a way the rest of the suite is not, and fragile for
    /// reasons that have nothing to do with the code: they need a display, the game
    /// installed, the window unobstructed for the whole run, and nothing else stealing
    /// focus. A run that fails because someone alt-tabbed teaches nobody anything, and a
    /// suite that cries wolf gets ignored.</para>
    ///
    /// <para>So they are off by default and turned on deliberately:</para>
    /// <code>
    /// set DEGCT_INGAME_TESTS=1
    /// dotnet test tools/GameAutomation.Tests
    /// </code>
    /// </remarks>
    public sealed class InGameFactAttribute : FactAttribute
    {
        /// <summary>
        /// The bare name of the variable that opts in: <c>DEGCT_INGAME_TESTS</c>.
        /// </summary>
        public const string OptInVariable = "INGAME_TESTS";

        /// <summary>Creates the attribute, skipping unless opted in.</summary>
        public InGameFactAttribute()
        {
            if (!IsOptedIn)
            {
                // QUALIFIED WHERE IT IS SAID, so the message cannot name a variable that has
                // moved: the bare name is the only spelling anything here holds.
                Skip = "Needs a running game and a desktop. Set "
                    + $"{DegctEnv.Qualified(OptInVariable)}=1 to run.";
            }
        }

        /// <summary>Whether in-game tests have been opted into.</summary>
        public static bool IsOptedIn
        {
            get
            {
                string? value = DegctEnv.Get(OptInVariable);
                return !string.IsNullOrWhiteSpace(value)
                    && !string.Equals(value, "0", StringComparison.Ordinal)
                    && !string.Equals(value, "false", StringComparison.OrdinalIgnoreCase);
            }
        }
    }
}
