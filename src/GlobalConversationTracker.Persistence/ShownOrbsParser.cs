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
    /// <para>A text parser, not the game's own table. Orbs are saved as a Lua
    /// <em>script</em> the game executes, so unlike dialogue SimStatus there are no
    /// bytes to intercept. The text crosses the IL2CPP boundary as a plain
    /// <c>string</c> and is parsed here in ordinary C# testable against real save
    /// files. It also removes an ordering hazard: the text is the whole answer whether
    /// or not the game has executed it yet.</para>
    ///
    /// <para>The format is generated, not authored.
    /// <c>SunshinePersistenceLua.AppendPropertiesToLuaTable</c> writes every row as
    /// <c>{table}[{key}]={fields};\n</c>, with the key run through
    /// <c>LuaHelper.FormatLuaValue</c>, which wraps a string in plain double quotes and
    /// escapes nothing:</para>
    /// <code>ShownOrbs["WHIRLING F2 ORB / locked door"]={OrbSeen=1};</code>
    /// <para>No orb's conversation title contains a double quote - the game's own writer
    /// would produce broken Lua if one did - so the closing <c>"]=</c> is an unambiguous
    /// terminator.</para>
    ///
    /// <para>Only <c>OrbSeen=1</c> counts: <c>SenseOrb.SetShown</c> writes the literal 1
    /// and there is no unsetter, so requiring it costs nothing and means a row saying
    /// anything else would be ignored rather than counted.</para>
    ///
    /// <para>Rows of other tables in the same file (<c>AreaState</c>) are ignored, as is
    /// anything that does not match.</para>
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
