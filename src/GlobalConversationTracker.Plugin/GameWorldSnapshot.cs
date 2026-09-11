// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using GlobalConversationTracker.Core;
using GlobalConversationTracker.Engine;
using GlobalConversationTracker.Session;
using PixelCrushers.DialogueSystem;
using Voidforge;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Writes down what the running game says, as answers to the questions the native
    /// engine asked.
    /// </summary>
    /// <remarks>
    /// <para>The half of the bridge that fills a world. The engine names its own questions
    /// - see <see cref="LookAheadQuestions"/> - so this is a walk over the lists it
    /// returned, asking the game each one and putting the answer in the matching slot. It
    /// invents nothing and decides nothing.</para>
    ///
    /// <para>This is a RE-SHAPING of what <see cref="GameLookAheadWorld"/> already does
    /// rather than new knowledge: the same Lua, the same defaults, the same direction of
    /// caution. The two share their game reads through <see cref="GameFacts"/> so they
    /// cannot drift on the parts that are simply reads, and what is left to check between
    /// them - which questions get asked, and whether an answer lands under the right key -
    /// is what the in-game comparison is for.</para>
    ///
    /// <para>ANSWER NOTHING YOU CANNOT ANSWER. A question the game will not settle is left
    /// Unknown, which is permissive: it widens the reachable set rather than narrowing it,
    /// costing a wasted click instead of hiding content the player has never read.</para>
    /// </remarks>
    internal static class GameWorldSnapshot
    {
        /// <summary>
        /// Builds a request for one conversation group, with the world filled in and every
        /// entry's novelty decided. The caller adds the options to score.
        /// </summary>
        /// <param name="conversation">Any conversation in the group.</param>
        /// <param name="questions">
        /// What the engine asked, as cached for this conversation. The answers go back
        /// BY POSITION, so this must be the list the engine will resolve against.
        /// </param>
        /// <param name="session">The global state, for what other saves have shown.</param>
        internal static LookAheadRequest Build(
            int conversation, LookAheadQuestions questions, GlobalStateSession session)
        {
            var world = new WorldSnapshot();
            var request = new LookAheadRequest(conversation, world);

            world.Money = GameFacts.ReadMoney();

            GameFacts.GameClock? time = GameFacts.ReadClock();
            world.DayMinutes = time?.DayMinutes ?? 0;
            world.DayCounter = time?.DayCounter ?? 1;
            // LOCKED WHATEVER THE READING SAYS, because nothing the game exposes to Lua
            // says whether its clock is locked, and a crawl that invents movement can mark
            // an option for content behind a wait the player cannot make happen. So time
            // stands still during a crawl, which is what it did when the clock could not be
            // read at all - what changes is that the HOUR is now the hour. See de-3jec.
            world.ClockLocked = true;

            foreach (string name in questions.Variables)
            {
                world.VariableValues.Add(Convert(DialogueLua.GetVariable(name)));
            }

            foreach (string key in questions.Queries)
            {
                world.QueryValues.Add(Evaluate(key));
            }

            FillMembers(questions.Items, "CheckItem", world.Items);
            FillMembers(questions.Tasks, "IsTaskActive", world.Tasks);
            FillMembers(questions.Thoughts, "IsTHCPresent", world.Thoughts);

            FillChecks(questions.Checks, world);
            FillNovelty(questions.Entries, session, world, request);

            return request;
        }

        /// <summary>
        /// The queries the engine names are the CALLS themselves - <c>IsKimHere()</c>,
        /// <c>CheckEquipped("neck_tie")</c> - so a key runs as Lua as it stands.
        /// </summary>
        /// <remarks>
        /// A coupling worth naming: the engine renders these keys and this runs them,
        /// which works because the rendering is a call with literal arguments and is
        /// exactly what <see cref="GameLookAheadWorld"/> rebuilds by hand today. A key that
        /// stopped being valid Lua would not crash anything - the run fails and the answer
        /// is Unknown - but every query in the group would quietly go permissive, so the
        /// in-game comparison counts how many answered.
        /// </remarks>
        private static WireValue Evaluate(string call)
        {
            Lua.Result? result = GameFacts.Run(call);
            return result == null ? WireValue.Unknown : Convert(result);
        }

        /// <summary>Asks a yes/no query per name, keeping the names that said yes.</summary>
        /// <remarks>
        /// Only a definite yes puts a name in the set. An unanswerable query leaves it out,
        /// which reads as "not held" - and that is the permissive direction for these
        /// three, because a guard demanding an item the player may or may not have is more
        /// usefully treated as blocked than as open when the crawl can also GAIN the item
        /// along the way.
        /// </remarks>
        private static void FillMembers(
            IReadOnlyList<string> names, string query, ISet<string> holding)
        {
            foreach (string name in names)
            {
                Lua.Result? result = GameFacts.Run(query + "(\"" + name + "\")");
                if (result != null && IsTrue(result))
                {
                    holding.Add(name);
                }
            }
        }

        /// <summary>Whether a Lua answer counts as yes.</summary>
        /// <remarks>
        /// Lua's own truthiness, which is what <c>GuardValue.AsCondition</c> applies and
        /// therefore what <see cref="GameLookAheadWorld"/> answers these three with: only
        /// false is false, and a number or a string is true. Written out here because
        /// requiring a boolean instead would have quietly answered "not held" for a query
        /// that returns a count.
        /// </remarks>
        private static bool IsTrue(Lua.Result result)
        {
            return result.isBool ? result.asBool : result.isNumber || result.isString;
        }

        /// <summary>Whether each entry's passive check fires, per <see cref="PassiveCheckRule"/>.</summary>
        private static void FillChecks(IReadOnlyList<NodeRef> checks, WorldSnapshot world)
        {
            DialogueDatabase database = DialogueManager.masterDatabase;
            foreach (NodeRef node in checks)
            {
                Ternary outcome;
                try
                {
                    DialogueEntry? entry = database == null
                        ? null
                        : database.GetDialogueEntry(node.Conversation, node.Entry);
                    outcome = PassiveCheckRule.Evaluate(entry);
                }
                catch (System.Exception)
                {
                    // Reaching into the character sheet from a UI callback: if anything is
                    // half-built, Unknown keeps the crawl correct rather than guessing.
                    outcome = Ternary.Unknown;
                }

                if (outcome == Ternary.True)
                {
                    world.ChecksPass.Add(node);
                }
                else if (outcome == Ternary.False)
                {
                    world.ChecksFail.Add(node);
                }
            }
        }

        /// <summary>
        /// Sorts every entry in the group into seen, unseen-this-save, or unseen-anywhere.
        /// </summary>
        /// <remarks>
        /// One walk for all four sets, because they come from the same two facts: the
        /// game's own per-save SimStatus, and the global state's record of every other
        /// save. Read once and held constant for the crawl, which is what
        /// <see cref="ILookAheadWorld.IsSeen"/> promises.
        /// </remarks>
        private static void FillNovelty(
            IReadOnlyList<NodeRef> entries,
            GlobalStateSession session,
            WorldSnapshot world,
            LookAheadRequest request)
        {
            GlobalConversationState state = session.EnsureInitialized();
            foreach (NodeRef node in entries)
            {
                if (GameFacts.IsSeen(node.Conversation, node.Entry))
                {
                    world.Seen.Add(node);
                    continue;
                }

                if (state.GetStatus(node.Conversation, node.Entry) == SimStatus.WasDisplayed)
                {
                    request.UnseenThisGame.Add(node);
                }
                else
                {
                    request.UnseenAnyGame.Add(node);
                }
            }
        }

        private static WireValue Convert(Lua.Result result)
        {
            if (result.isBool)
            {
                return WireValue.FromBoolean(result.asBool);
            }

            if (result.isNumber)
            {
                return WireValue.FromNumber(result.asFloat);
            }

            if (result.isString)
            {
                return WireValue.FromText(result.asString);
            }

            return WireValue.Unknown;
        }
    }
}
