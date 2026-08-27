// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Persistence
{
    /// <summary>
    /// Reads the conversation titles out of a savegame's <c>{save}.states.lua</c>.
    /// </summary>
    /// <remarks>
    /// <para><b>Why a text parser and not the game's own table.</b> Unlike dialogue
    /// SimStatus, which arrives as bytes for <c>PersistentDataManager.ApplyRawData</c>
    /// and can be intercepted there, orbs are saved as a Lua <em>script</em> the game
    /// simply executes. The text is what the mod can get hold of safely: a plain
    /// <c>string</c> crossing the IL2CPP boundary, parsed here in ordinary C# that can
    /// be tested against real save files, rather than an interop enumeration of a live
    /// Lua table that could only ever be tested by running the game. It also removes an
    /// ordering hazard: the text is the whole answer whether or not the game has
    /// executed it yet, so there is no "was the table populated by now" question to get
    /// wrong.</para>
    ///
    /// <para><b>The format is generated, not authored.</b> Every row is written by
    /// <c>SunshinePersistenceLua.AppendPropertiesToLuaTable</c> as
    /// <c>{table}[{key}]={fields};\n</c>, with the key run through
    /// <c>LuaHelper.FormatLuaValue</c>, which wraps a string in plain double quotes and
    /// escapes nothing. So a row looks exactly like:</para>
    /// <code>ShownOrbs["WHIRLING F2 ORB / locked door"]={OrbSeen=1};</code>
    /// <para>No conversation title used by an orb contains a double quote - which is
    /// just as well, since the game's own writer would produce broken Lua if one did -
    /// so the closing <c>"]=</c> is an unambiguous terminator.</para>
    ///
    /// <para><b>Only <c>OrbSeen=1</c> counts.</b> Nothing in the game writes any other
    /// value - <c>SenseOrb.SetShown</c> writes the literal 1 and there is no unsetter -
    /// so requiring it costs nothing today and means a row that ever did say otherwise
    /// would be ignored rather than counted.</para>
    ///
    /// <para>Rows of other tables in the same file - <c>AreaState</c> is the other one -
    /// are ignored, and so is anything that does not match.</para>
    /// </remarks>
    public static class ShownOrbsParser
    {
        /// <summary>The game's orb table, as <c>SenseOrb.ORB_LUA_TABLE</c>.</summary>
        public const string TableName = "ShownOrbs";

        /// <summary>The field inside a row, as <c>SenseOrb.luaFieldName</c>.</summary>
        public const string SeenFieldName = "OrbSeen";

        /// <summary>
        /// The suffix of the save file this parses, as
        /// <c>SunshinePersistenceFileManager.LUA_STATES_FILE_SUFFIX</c>.
        /// </summary>
        public const string StatesFileSuffix = ".states.lua";

        /// <summary>
        /// One row. Whitespace is tolerated where the writer happens not to put any
        /// today, so a cosmetic change to the writer does not silently stop this
        /// matching; the structure is what is pinned down.
        /// </summary>
        private static readonly Regex RowPattern = new Regex(
            @"^\s*" + TableName + @"\s*\[\s*""(?<title>.*)""\s*\]\s*=\s*\{(?<fields>[^}]*)\}\s*;?\s*$",
            RegexOptions.Compiled | RegexOptions.CultureInvariant);

        /// <summary>Matches the seen field set to 1 inside a row's field list.</summary>
        private static readonly Regex SeenPattern = new Regex(
            @"(^|[,\s])" + SeenFieldName + @"\s*=\s*1(\.0*)?\s*($|[,\s])",
            RegexOptions.Compiled | RegexOptions.CultureInvariant);

        /// <summary>
        /// Whether a save file name or suffix is the states file this parses.
        /// </summary>
        public static bool IsStatesFile(string? nameOrSuffix) =>
            nameOrSuffix != null
            && nameOrSuffix.EndsWith(StatesFileSuffix, StringComparison.OrdinalIgnoreCase);

        /// <summary>
        /// The conversation titles of every orb recorded as seen in this file.
        /// </summary>
        /// <param name="statesLua">The contents of a save's <c>.states.lua</c>.</param>
        /// <returns>
        /// The titles, in the order the file lists them, duplicates included - the
        /// caller holds them in a set, so de-duplicating here would only hide what the
        /// file actually said.
        /// </returns>
        /// <exception cref="ArgumentNullException"><paramref name="statesLua"/> is null.</exception>
        public static List<string> GetSeenOrbTitles(string statesLua)
        {
            if (statesLua == null)
            {
                throw new ArgumentNullException(nameof(statesLua));
            }

            var titles = new List<string>();
            foreach (string line in statesLua.Split('\n'))
            {
                Match row = RowPattern.Match(line);
                if (!row.Success)
                {
                    continue;
                }

                string title = row.Groups["title"].Value;
                if (title.Length == 0)
                {
                    continue;
                }

                if (!SeenPattern.IsMatch(row.Groups["fields"].Value))
                {
                    continue;
                }

                titles.Add(title);
            }

            return titles;
        }
    }
}
