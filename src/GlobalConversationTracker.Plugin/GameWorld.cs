// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Globalization;
using GlobalConversationTracker.Engine;
using PixelCrushers.DialogueSystem;
using Voidforge;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The player's situation, read out of the running game for one look-ahead crawl.
    /// </summary>
    /// <remarks>
    /// <para>Built fresh per response menu and thrown away, because everything it holds is
    /// read at one moment: the balance, the variables, what Kim is doing. Caching one across
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
    internal sealed class GameWorld : ILookAheadWorld
    {
        private readonly Dictionary<string, GuardValue> _queries =
            new Dictionary<string, GuardValue>();
        private readonly Dictionary<string, GuardValue> _variables =
            new Dictionary<string, GuardValue>();
        private readonly Dictionary<DialogueNodeId, Ternary> _checks =
            new Dictionary<DialogueNodeId, Ternary>();

        /// <summary>Reads the running game.</summary>
        internal GameWorld()
        {
            Money = GameFacts.ReadMoney();

            GameFacts.GameClock? time = GameFacts.ReadClock();
            DayMinutes = time?.DayMinutes ?? 0;
            DayCounter = time?.DayCounter ?? 1;
            // LOCKED WHATEVER THE READING SAYS, because nothing the game exposes to Lua
            // says whether its clock is locked, and a crawl that invents movement can mark
            // an option for content behind a wait the player cannot make happen. So time
            // stands still during a crawl, which is what it did when the clock could not be
            // read at all - what changes is that the HOUR is now the hour. See de-3jec.
            IsClockLocked = true;
        }

        /// <inheritdoc/>
        public int Money { get; }

        /// <inheritdoc/>
        public int DayMinutes { get; }

        /// <inheritdoc/>
        public int DayCounter { get; }

        /// <summary>
        /// Whether the clock is locked. True when it could not be read at all, so a
        /// crawl that cannot see the clock leaves it where it is rather than inventing
        /// movement.
        /// </summary>
        public bool IsClockLocked { get; }

        /// <inheritdoc/>
        public GuardValue GetVariable(string name)
        {
            if (_variables.TryGetValue(name, out GuardValue cached))
            {
                return cached;
            }

            // NULL IS AN UNDECLARED NAME, and the game reads one as false rather than as a
            // question - see `GameFacts.ReadVariable`, which says so once in the log.
            Lua.Result? read = GameFacts.ReadVariable(name);
            GuardValue value = read == null
                ? GuardValue.FromBoolean(false)
                : Convert(read);
            _variables[name] = value;
            return value;
        }

        /// <inheritdoc/>
        public bool HasItem(string name)
        {
            return Evaluate("CheckItem(\"" + name + "\")").AsCondition() == Ternary.True;
        }

        /// <inheritdoc/>
        public GuardValue Query(string name, IReadOnlyList<GuardValue> arguments)
        {
            string? call = Render(name, arguments);
            return call == null ? GuardValue.Unknown : Evaluate(call);
        }

        /// <summary>
        /// Whether a passive check on this entry fires, per <see cref="PassiveCheckRule"/>.
        /// </summary>
        /// <remarks>
        /// Cached for the crawl. The same entry is asked about repeatedly as the search
        /// fans out, and the answer cannot change while a single response menu is drawn.
        /// </remarks>
        public Ternary CheckPasses(DialogueNodeId node)
        {
            if (_checks.TryGetValue(node, out Ternary cached))
            {
                return cached;
            }

            Ternary outcome;
            try
            {
                DialogueDatabase database = DialogueManager.masterDatabase;
                DialogueEntry? entry = database == null
                    ? null
                    : database.GetDialogueEntry(node.ConversationId, node.EntryId);
                outcome = PassiveCheckRule.Evaluate(entry);
            }
            catch (System.Exception)
            {
                // Reaching into the character sheet from a UI callback: if anything is
                // half-built, Unknown keeps the crawl correct rather than guessing.
                outcome = Ternary.Unknown;
            }

            _checks[node] = outcome;
            return outcome;
        }

        /// <inheritdoc/>
        public bool IsSeen(DialogueNodeId node)
        {
            return GameFacts.IsSeen(node.ConversationId, node.EntryId);
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

            // A query this build of the game does not define, or one that threw, is
            // Unknown - the honest answer and the safe one.
            Lua.Result? result = GameFacts.Run(expression);
            GuardValue value = result == null ? GuardValue.Unknown : Convert(result);

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
    }
}
