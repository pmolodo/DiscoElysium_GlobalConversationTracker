// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
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
    /// - <see cref="GameWorld"/>, which answers the managed engine directly, and
    /// <see cref="LookAheadRequestBuilder"/>, which writes the same facts down for the native
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

        /// <summary>Which undefined variables have already been named in the log.</summary>
        private static readonly HashSet<string> _undefinedNamed = new HashSet<string>();

        /// <summary>
        /// What a check's failure slot is called: its flag's name with this after it.
        /// </summary>
        /// <remarks>
        /// The engine's <c>index::FAILED_FLAG_SUFFIX</c> written again on this side, because
        /// the names arrive over the wire already built and nothing sends the suffix itself.
        /// The two must agree; if they drift, the only symptom is this log filling with
        /// failure slots reported as content bugs.
        /// </remarks>
        private const string FailedFlagSuffix = "_failed";

        /// <summary>
        /// A dialogue variable's value, or null for one the database never declared.
        /// </summary>
        /// <remarks>
        /// <para>AN UNDEFINED VARIABLE IS FALSE, because that is what the game makes of
        /// one. <c>FlagSet</c> is a call to <c>LuaHelper.GetVariable</c>, which returns
        /// <c>bool</c> - there is no third answer for it to give - and a name Lua has never
        /// heard of reads nil, which is false in the condition the guard puts it in. So a
        /// guard on an undeclared variable hides its entry, every time.</para>
        ///
        /// <para>Null rather than a value, so the caller supplies the false in its own
        /// type. Returning it from here would make this method know about both engines'
        /// value types in order to say one thing.</para>
        ///
        /// <para>WORTH A WARNING EVEN THOUGH IT IS HANDLED. A guard reading a name nothing
        /// declares is a content bug - an entry that can never appear, and whose author
        /// meant it to - so the log names it once per variable. The shipped database has
        /// exactly one, <c>undefined.pinball_asked_about_the_goats</c> on 1467:104, which
        /// gates "And Gurdi?" on a flag that no script anywhere sets; the flag it was
        /// plainly meant to read, <c>whirling.pinball_asked_about_the_goats</c>, differs
        /// only in its namespace. A second one appearing is worth knowing about.</para>
        /// </remarks>
        /// <param name="name">The variable to read.</param>
        internal static Lua.Result? ReadVariable(string name)
        {
            Lua.Result result = DialogueLua.GetVariable(name);
            if (result.isBool || result.isNumber || result.isString)
            {
                return result;
            }

            // A CHECK'S FAILURE SLOT IS OURS AND IS MEANT TO BE MISSING. The engine gives
            // every check flag a `_failed` companion whether or not the database declares
            // one, so Lua has never heard of most of them. False is right for such a slot -
            // it means "already failed", and a check nothing has failed yet is open - but it
            // is not a content bug, and naming one here would fill the log with the engine
            // reporting its own modelling as the game's mistake.
            if (!name.EndsWith(FailedFlagSuffix, StringComparison.Ordinal)
                && _undefinedNamed.Add(name))
            {
                Log?.Warning(
                    $"Look-ahead: no variable named '{name}' exists, though a guard reads "
                    + "it. Reading it as false, which is what the game does with it. The "
                    + "entry behind that guard can never be shown.");
            }

            return null;
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
