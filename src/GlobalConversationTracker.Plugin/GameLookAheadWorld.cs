// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Globalization;
using GlobalConversationTracker.LookAhead;
using PixelCrushers.DialogueSystem;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The player's situation, read out of the running game for one look-ahead crawl.
    /// </summary>
    /// <remarks>
    /// <para>Built fresh per response menu and thrown away, because everything it holds
    /// is a snapshot: the balance, the variables, what Kim is doing. Caching one across
    /// menus would answer this menu's question with the last one's facts.</para>
    ///
    /// <para>Whatever cannot be answered comes back
    /// <see cref="GuardValue.Unknown"/> or <see cref="Ternary.Unknown"/>, never a guess.
    /// The engine treats Unknown as passable, so an unanswered query widens the reported
    /// reachable set rather than narrowing it - which is the direction that costs a
    /// wasted click instead of hiding content the player has never seen.</para>
    ///
    /// <para>Every read goes through Lua, which is where the game keeps this state, and
    /// each distinct query is run once and cached for the life of the crawl. A menu asks
    /// the same handful of questions hundreds of times as the search fans out.</para>
    /// </remarks>
    internal sealed class GameLookAheadWorld : ILookAheadWorld
    {
        /// <summary>
        /// What to assume when the balance cannot be read. Everything is affordable, so
        /// cost options stay walkable - the over-reporting direction.
        /// </summary>
        private const int UnknownMoney = int.MaxValue;

        private readonly Dictionary<string, GuardValue> _queries =
            new Dictionary<string, GuardValue>();
        private readonly Dictionary<string, GuardValue> _variables =
            new Dictionary<string, GuardValue>();

        /// <summary>Creates a snapshot.</summary>
        internal GameLookAheadWorld()
        {
            Money = ReadMoney();
        }

        /// <inheritdoc/>
        public int Money { get; }

        /// <inheritdoc/>
        public GuardValue GetVariable(string name)
        {
            if (_variables.TryGetValue(name, out GuardValue cached))
            {
                return cached;
            }

            GuardValue value = Convert(DialogueLua.GetVariable(name));
            _variables[name] = value;
            return value;
        }

        /// <inheritdoc/>
        public bool HasItem(string name)
        {
            return Evaluate("CheckItem(\"" + name + "\")").AsCondition() == Ternary.True;
        }

        /// <inheritdoc/>
        public bool IsTaskActive(string name)
        {
            return Evaluate("IsTaskActive(\"" + name + "\")").AsCondition() == Ternary.True;
        }

        /// <inheritdoc/>
        public GuardValue Query(string name, IReadOnlyList<GuardValue> arguments)
        {
            string? call = Render(name, arguments);
            return call == null ? GuardValue.Unknown : Evaluate(call);
        }

        /// <summary>
        /// Whether a passive check on this entry fires.
        /// </summary>
        /// <remarks>
        /// Not yet implemented, so every check comes back Unknown and the engine explores
        /// both outcomes. That is correct and conservative, and it costs branching at the
        /// 10,500 entries in the database that carry a <c>DifficultyPass</c>. The rule
        /// itself is simple - <c>PassiveNode.CheckSuccess</c> is skill plus six against
        /// the difficulty threshold, no dice - and wiring it up is what turns this from
        /// "never wrong" into "usually exact".
        /// </remarks>
        public Ternary CheckPasses(DialogueNodeId node)
        {
            return Ternary.Unknown;
        }

        /// <inheritdoc/>
        public bool IsSeen(DialogueNodeId node)
        {
            return DialogueLua.GetSimStatus(node.ConversationId, node.EntryId)
                == "WasDisplayed";
        }

        /// <summary>
        /// Rebuilds a call as Lua source. Returns null for anything whose arguments
        /// cannot be written back out, which becomes an Unknown rather than a guess.
        /// </summary>
        private static string? Render(string name, IReadOnlyList<GuardValue> arguments)
        {
            var text = new System.Text.StringBuilder(name).Append('(');
            for (int i = 0; i < arguments.Count; i++)
            {
                if (i > 0)
                {
                    text.Append(", ");
                }

                GuardValue argument = arguments[i];
                switch (argument.Kind)
                {
                    case GuardValueKind.Text:
                        if (argument.Text.IndexOf('"') >= 0)
                        {
                            return null;
                        }

                        text.Append('"').Append(argument.Text).Append('"');
                        break;
                    case GuardValueKind.Number:
                        text.Append(argument.Number.ToString(CultureInfo.InvariantCulture));
                        break;
                    case GuardValueKind.Boolean:
                        text.Append(argument.Boolean ? "true" : "false");
                        break;
                    default:
                        return null;
                }
            }

            return text.Append(')').ToString();
        }

        /// <summary>Runs a Lua expression once, caching whatever it says.</summary>
        private GuardValue Evaluate(string expression)
        {
            if (_queries.TryGetValue(expression, out GuardValue cached))
            {
                return cached;
            }

            GuardValue value;
            try
            {
                value = Convert(Lua.Run("return " + expression));
            }
            catch (System.Exception)
            {
                // A query this build of the game does not define, or one that threw.
                // Unknown is the honest answer and the safe one.
                value = GuardValue.Unknown;
            }

            _queries[expression] = value;
            return value;
        }

        private static GuardValue Convert(Lua.Result result)
        {
            if (result.isBool)
            {
                return GuardValue.FromBoolean(result.asBool);
            }

            if (result.isNumber)
            {
                return GuardValue.FromNumber(result.asFloat);
            }

            if (result.isString)
            {
                return GuardValue.FromText(result.asString);
            }

            return GuardValue.Unknown;
        }

        private static int ReadMoney()
        {
            try
            {
                Lua.Result result = Lua.Run("return MoneyAmount()");
                if (result.isNumber)
                {
                    float amount = result.asFloat;
                    return amount <= 0 ? 0 : (int)amount;
                }
            }
            catch (System.Exception)
            {
                // Fall through to the permissive default.
            }

            return UnknownMoney;
        }
    }
}
