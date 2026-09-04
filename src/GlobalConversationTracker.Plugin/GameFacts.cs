// SPDX-License-Identifier: MIT
using System;
using PixelCrushers.DialogueSystem;
using Voidforge;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The handful of things a look-ahead reads out of the running game.
    /// </summary>
    /// <remarks>
    /// <para>Shared by the two worlds that exist while the look-ahead moves from C# to Rust
    /// - <see cref="GameLookAheadWorld"/>, which answers the managed engine directly, and
    /// <see cref="GameWorldSnapshot"/>, which writes the same facts down for the native
    /// one. They must agree, and the way to make two things agree is for them to be one
    /// thing.</para>
    ///
    /// <para>Every read here is defensive in the same direction: a failure is reported as
    /// "could not read" and never as a value. These run from a UI callback, against a
    /// modded game, and a guess that reaches a player is worse than a marker that does
    /// not.</para>
    /// </remarks>
    internal static class GameFacts
    {
        /// <summary>
        /// What to assume when the balance cannot be read. Everything is affordable, so
        /// cost options stay walkable - the over-reporting direction.
        /// </summary>
        internal const int UnknownMoney = int.MaxValue;

        /// <summary>
        /// Runs a Lua expression, or null for one this build of the game does not define,
        /// or that threw.
        /// </summary>
        /// <param name="expression">The expression, without a leading <c>return</c>.</param>
        internal static Lua.Result? Run(string expression)
        {
            try
            {
                return Lua.Run("return " + expression);
            }
            catch (Exception)
            {
                return null;
            }
        }

        /// <summary>The player's balance in centimes, or <see cref="UnknownMoney"/>.</summary>
        internal static int ReadMoney()
        {
            Lua.Result? result = Run("MoneyAmount()");
            if (result != null && result.isNumber)
            {
                float amount = result.asFloat;
                return amount <= 0 ? 0 : (int)amount;
            }

            return UnknownMoney;
        }

        /// <summary>The game's clock, or null before it exists.</summary>
        internal static SunshineClockTime? ReadClock()
        {
            try
            {
                SunshineClock clock = SingletonClass<SunshineClock>.Singleton;
                return clock == null ? null : clock.Time;
            }
            catch (Exception)
            {
                return null;
            }
        }

        /// <summary>
        /// Whether this entry has been displayed in the current save.
        /// </summary>
        /// <param name="conversationId">The conversation.</param>
        /// <param name="entryId">The entry within it.</param>
        internal static bool IsSeen(int conversationId, int entryId)
        {
            return DialogueLua.GetSimStatus(conversationId, entryId) == "WasDisplayed";
        }
    }
}
