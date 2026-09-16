// SPDX-License-Identifier: MIT
using System;
using PixelCrushers.DialogueSystem;
using Voidforge;
using GlobalConversationTracker.Session;

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

        /// <summary>
        /// What a Lua expression answers, as a plain value, or null where it could not be
        /// read.
        /// </summary>
        /// <remarks>
        /// <para>A HARNESS INSTRUMENT, and this is why it exists: most of what Final Cut
        /// ships has no readable body anywhere, because the export stubs every method it
        /// could not reconstruct. What such a function COMPUTES can then only be learned by
        /// asking a running game - load a save that varies what it reads, evaluate it, and
        /// write the answer down. That is the shipped build answering, which is the only
        /// authority there is once the source is gone.</para>
        ///
        /// <para>The kinds are the ones a dialogue guard can answer in, discriminated the
        /// way the world snapshot discriminates them. NULL IS "COULD NOT READ" AND NEVER A
        /// VALUE - an expression this build does not define, one that threw, or one
        /// answering something that is none of these - so a caller can tell silence from
        /// false. Reading them the same way matters more than it looks: a truth table built
        /// here is compared against what the snapshot reports, and two discriminations would
        /// make a disagreement about the reader look like one about the game.</para>
        /// </remarks>
        /// <param name="expression">The expression, without a leading <c>return</c>.</param>
        internal static object? Evaluate(string expression)
        {
            Lua.Result? result = Run(expression);
            if (result == null)
            {
                return null;
            }

            if (result.isBool)
            {
                return result.asBool;
            }

            if (result.isNumber)
            {
                return result.asFloat;
            }

            return result.isString ? result.asString : null;
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

        /// <summary>Where a clock that cannot be read is reported. Set once, at load.</summary>
        internal static IGlobalStateLog? Log { get; set; }

        /// <summary>Minutes in an hour, for a clock read to the hour.</summary>
        private const int MinutesPerHour = 60;

        /// <summary>Whether the complaint below has been made this session.</summary>
        private static bool _clockComplained;

        /// <summary>What the game's clock says, to the hour.</summary>
        /// <remarks>
        /// TO THE HOUR, WHICH IS EVERY RESOLUTION THE GAME ASKS FOR. The database's guards
        /// compare the clock with <c>HourCount</c>, <c>TotalHourCount</c>,
        /// <c>IsHourBetween</c>, <c>IsDayFrom</c> and <c>DayCount</c>, and not one of them
        /// asks after a minute - so an hour read exactly is worth more than a minute read
        /// not at all, which is what the alternative turned out to be.
        /// </remarks>
        internal readonly struct GameClock
        {
            /// <summary>Creates a reading.</summary>
            /// <param name="dayMinutes">Minutes past midnight.</param>
            /// <param name="dayCounter">Which day it is.</param>
            internal GameClock(int dayMinutes, int dayCounter)
            {
                DayMinutes = dayMinutes;
                DayCounter = dayCounter;
            }

            /// <summary>Minutes past midnight.</summary>
            internal int DayMinutes { get; }

            /// <summary>Which day it is.</summary>
            internal int DayCounter { get; }
        }

        /// <summary>The game's clock, or null where it cannot be read.</summary>
        /// <remarks>
        /// <para>ASKED OF LUA, THE WAY THE BALANCE IS, and that is the whole of de-3jec.
        /// This used to read <c>SingletonClass&lt;SunshineClock&gt;.Singleton</c> - a static
        /// on a GENERIC base class, which answers null through the IL2CPP interop layer
        /// however alive the object is - and then reported "no clock", which sends a crawl
        /// to midnight on day one. Every marker the mod has ever drawn was computed there.
        /// Captured 2026-09-11: the game sent day_minutes 0 and day_counter 1 for
        /// conversation 29 while the HUD in the same frame drew 10:35 on day three.</para>
        ///
        /// <para>THE SCENE WAS TRIED FIRST AND IS NOT THE ANSWER: the singleton is a plain
        /// object rather than a component, so <c>FindObjectOfType</c> never sees it. Lua is
        /// where the game already answers this, to the game's own dialogue, and it is the
        /// path <see cref="ReadMoney"/> has always used.</para>
        ///
        /// <para>AND IT SAYS SO WHEN IT CANNOT, once per session. What this replaces was
        /// silent for as long as the feature has existed.</para>
        /// </remarks>
        internal static GameClock? ReadClock()
        {
            Lua.Result? hour = Run("HourCount()");
            Lua.Result? day = Run("DayCount()");

            if (hour == null || !hour.isNumber || day == null || !day.isNumber)
            {
                Complain("the game's clock did not answer");
                return null;
            }

            return new GameClock((int)hour.asFloat * MinutesPerHour, (int)day.asFloat);
        }

        /// <summary>Says something once, and never again this session.</summary>
        /// <param name="what">What could not be read.</param>
        private static void Complain(string what)
        {
            if (_clockComplained)
            {
                return;
            }

            _clockComplained = true;
            Log?.Warning(
                $"Look-ahead: {what}, so every crawl this session runs at midnight on day "
                + "one with time locked. Markers will be drawn for a world the player is "
                + "not in.");
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
