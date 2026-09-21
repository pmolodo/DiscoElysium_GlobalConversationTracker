// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using GlobalConversationTracker.Core;
using GlobalConversationTracker.Engine;
using GlobalConversationTracker.Session;
using PixelCrushers.DialogueSystem;
using Sunshine.Metric;
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
    /// <para>This is a RE-SHAPING of what <see cref="GameWorld"/> already does
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
    internal static class LookAheadRequestBuilder
    {

        /// <summary>
        /// Builds a request for one conversation group, with the world filled in and every
        /// entry's seen state decided. The caller adds the options to score.
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
            var world = new WorldRawData();
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
                // THROUGH THE SAME READ THE MANAGED WORLD USES, so the two cannot disagree
                // about a variable nothing declares: both send false, which is what the
                // game makes of one. See `GameFacts.ReadVariable`.
                Lua.Result? read = GameFacts.ReadVariable(name);
                world.VariableValues.Add(
                    read == null ? WireValue.FromBoolean(false) : Convert(read));
            }

            foreach (string key in questions.Queries)
            {
                world.QueryValues.Add(Evaluate(key));
            }

            FillMembers(questions.Items, "CheckItem", world.Items);
            FillMembers(questions.Thoughts, "IsTHCPresent", world.Thoughts);

            foreach (DataRequest ask in questions.Data)
            {
                world.DataValues.Add(Serviced(ask, questions));
            }

            FillChecks(questions.Checks, world);
            FillFailedWhiteChecks(world);
            FillRedChecksFail(world);
            FillSeenState(questions.Entries, session, world, request);

            return request;
        }

        /// <summary>
        /// The queries the engine names are the CALLS themselves - <c>IsKimHere()</c>,
        /// <c>IsCunoInParty()</c> - so a key runs as Lua as it stands.
        /// </summary>
        /// <remarks>
        /// A coupling worth naming: the engine renders these keys and this runs them,
        /// which works because the rendering is a call with literal arguments and is
        /// exactly what <see cref="GameWorld"/> rebuilds by hand today. A key that
        /// stopped being valid Lua would not crash anything - the run fails and the answer
        /// is Unknown - but every query in the group would quietly go permissive, so the
        /// in-game comparison counts how many answered.
        /// </remarks>
        private static WireValue Evaluate(string call)
        {
            Lua.Result? result = GameFacts.Run(call);
            return result == null ? WireValue.Unknown : Convert(result);
        }

        /// <summary>What the game says for one data request, READ rather than evaluated.</summary>
        /// <remarks>
        /// <para>The difference from <see cref="Evaluate"/> is the whole point of this
        /// channel: nothing here runs a dialogue function, so no request can move the game.
        /// A kind this build does not know is reported unserviced rather than guessed at.</para>
        ///
        /// <para>UNSERVICED IS NOT EMPTY. Every path that cannot read returns
        /// <see cref="DataAnswer.Unreadable"/>, because an empty set would tell the engine
        /// the subject is definitely not in it - which closes routes the game opens. That
        /// matters more than usual here: naming a thought needs
        /// <c>SingletonComponent&lt;ThoughtCabinetProjectList&gt;</c>, a static on a GENERIC
        /// BASE CLASS, and those answer null through the IL2CPP interop layer however alive
        /// the object is. That is de-3jec, which sent every crawl to midnight on day one for
        /// exactly this reason, so the failure is expected rather than hypothetical.</para>
        /// </remarks>
        private static DataAnswer Serviced(DataRequest wanted, LookAheadQuestions questions)
        {
            try
            {
                switch (wanted.Kind)
                {
                    case DataKind.ThoughtsCooking:
                        return ThoughtsWhere(questions.Thoughts, "IsTHCCooking");
                    case DataKind.ThoughtsFixed:
                        return ThoughtsWhere(questions.Thoughts, "IsTHCFixed");
                    case DataKind.EquippedInSlot:
                        return EquippedIn(wanted.Subject);
                    case DataKind.TabHoldsItems:
                        return TabHoldsItems(wanted.Subject);
                    case DataKind.ItemsInGroup:
                        return ItemsInGroup(wanted.Subject);
                    case DataKind.HeldItemsInGroup:
                        return HeldItemsInGroup(wanted.Subject);
                    case DataKind.SceneIsOutside:
                        return SceneIsOutside();
                    case DataKind.SkillDamage:
                        return SkillDamage(wanted.Subject);
                    case DataKind.GameMode:
                        return GameMode();
                    case DataKind.PartyFlag:
                        return PartyFlag(wanted.Subject);
                    case DataKind.HardcorePlaythroughCompleted:
                        return DataAnswer.Of(
                            WireValue.FromBoolean(GameStatsManager.HardcorePlaythroughCompleted));
                    default:
                        return DataAnswer.Unreadable();
                }
            }
            catch (System.Exception)
            {
                // Reaching into the game from a UI callback: if anything is half-built,
                // unserviced keeps the crawl correct rather than guessing.
                return DataAnswer.Unreadable();
            }
        }

        /// <summary>The item in one equipment slot, named by its slot type.</summary>
        /// <remarks>
        /// <para>A READ OF THE GAME'S OWN TABLE, not a dialogue function:
        /// <c>InventoryViewData.GetEquipped</c> looks the slot up in the equipment dictionary
        /// and nothing else. An empty slot answers empty text, which is a definite "nothing
        /// here", while a slot name this build does not know is unserviced.</para>
        /// </remarks>
        /// <param name="slot">An <c>EquipmentSlotType</c> name, such as <c>HAT</c>.</param>
        private static DataAnswer EquippedIn(string slot)
        {
            InventoryViewData? inventory = InventoryViewData.Singleton;
            if (inventory == null
                || !System.Enum.TryParse(slot, ignoreCase: false, out EquipmentSlotType type))
            {
                return DataAnswer.Unreadable();
            }

            return DataAnswer.Of(WireValue.FromText(inventory.GetEquipped(type) ?? string.Empty));
        }

        /// <summary>Whether one inventory tab holds anything, named by its tab group.</summary>
        /// <remarks>
        /// A read of the inventory's own tab table through
        /// <c>InventoryViewData.IsTabEmpty</c>. A tab name this build does not know is
        /// unserviced.
        /// </remarks>
        /// <param name="tab">An <c>ItemTabGroup</c> name, such as <c>PAWNABLES</c>.</param>
        private static DataAnswer TabHoldsItems(string tab)
        {
            InventoryViewData? inventory = InventoryViewData.Singleton;
            if (inventory == null
                || !System.Enum.TryParse(tab, ignoreCase: false, out ItemTabGroup group))
            {
                return DataAnswer.Unreadable();
            }

            return DataAnswer.Of(WireValue.FromBoolean(!inventory.IsTabEmpty(group)));
        }

        /// <summary>Whether the current scene is outdoors.</summary>
        /// <remarks>
        /// <para>What <c>MapLuaFunctions.IsExterior</c> returns, read directly:
        /// <c>ApplicationManager.CurrentSceneProperties.IsOutside</c>.</para>
        ///
        /// <para>FOUND BY TYPE rather than through <c>SingletonScriptable&lt;ApplicationManager&gt;
        /// .Singleton</c>, because a static on a generic base answers null through the IL2CPP
        /// interop layer however alive the object is - de-3jec. No scene properties yet is
        /// unserviced: asked through Lua at that moment, the call threw.</para>
        /// </remarks>
        private static DataAnswer SceneIsOutside()
        {
            foreach (FortressOccident.ApplicationManager manager in
                UnityEngine.Resources.FindObjectsOfTypeAll<FortressOccident.ApplicationManager>())
            {
                SceneProperties? scene = manager?.CurrentSceneProperties;
                if (scene != null)
                {
                    return DataAnswer.Of(WireValue.FromBoolean(scene.IsOutside));
                }
            }

            return DataAnswer.Unreadable();
        }

        /// <summary>One party flag, read off the party member it belongs to.</summary>
        /// <remarks>
        /// What <c>PartyManager.IsKimHere</c> and its siblings read - see <c>core::party</c> in
        /// the engine. The members are found by type rather than through
        /// <c>SingletonComponent&lt;T&gt;.Singleton</c>, a static on a generic base that
        /// answers null through the interop layer.
        /// </remarks>
        /// <param name="flag">A <c>partyState</c> field name.</param>
        private static DataAnswer PartyFlag(string flag)
        {
            bool? value = flag switch
            {
                "isKimInParty" => UnityEngine.Object.FindObjectOfType<Sunshine.Hack.KimKitsuragi>()?.IsInParty,
                "isKimLeftOutside" => UnityEngine.Object.FindObjectOfType<Sunshine.Hack.KimKitsuragi>()?.IsLeftOutside,
                "isCunoInParty" => UnityEngine.Object.FindObjectOfType<Cuno>()?.IsInParty,
                _ => null,
            };

            return value == null
                ? DataAnswer.Unreadable()
                : DataAnswer.Of(WireValue.FromBoolean(value.Value));
        }

        /// <summary>The game mode, by its enum name.</summary>
        /// <remarks>
        /// What <c>IsHardcoreModeActive</c> answers from, recovered from the binary and measured
        /// - see <c>core::game_mode</c> in the engine. No controller yet is unserviced.
        /// </remarks>
        private static DataAnswer GameMode()
        {
            GameModeController? controller = GameModeController.Singleton;
            return controller == null
                ? DataAnswer.Unreadable()
                : DataAnswer.Of(WireValue.FromText(controller.currentMode.ToString()));
        }

        /// <summary>One skill's damage value, named by its skill type.</summary>
        /// <remarks>
        /// What <c>CharacterLuaFunctions.HasVolitionDamage</c> and its sibling compare against
        /// zero, read directly. The world is found by type for the same reason the scene is -
        /// its generic singleton answers null through the interop layer.
        /// </remarks>
        /// <param name="skill"><c>VOLITION</c> or <c>ENDURANCE</c>.</param>
        private static DataAnswer SkillDamage(string skill)
        {
            World? world = UnityEngine.Object.FindObjectOfType<World>();
            Sunshine.Metric.CharacterSheet? you = world?.you;
            Sunshine.Metric.Skill? read = skill switch
            {
                "VOLITION" => you?.volition,
                "ENDURANCE" => you?.endurance,
                _ => null,
            };

            return read == null
                ? DataAnswer.Unreadable()
                : DataAnswer.Of(WireValue.FromNumber(read.damageValue));
        }

        /// <summary>The database field an item's group index is stored in.</summary>
        private const string ItemGroupField = "itemGroup";

        /// <summary>Every item the dialogue database files under one group.</summary>
        /// <remarks>
        /// The database stores the group as an index into the game's own
        /// <c>ItemUtil.itemGroup</c> table, which is what <c>Inventory.CheckItemGroup</c>
        /// compares by name - so the name is looked up there rather than restated.
        /// </remarks>
        /// <param name="group">An item group's name, such as <c>alcohol</c>.</param>
        private static DataAnswer ItemsInGroup(string group)
        {
            List<string>? members = MembersOf(group);
            return members == null ? DataAnswer.Unreadable() : DataAnswer.OfNames(members);
        }

        /// <summary>The items of one group the player holds.</summary>
        /// <remarks>
        /// Held is <c>CheckItem</c>, asked per member the way <see cref="FillMembers"/> asks
        /// it: a pure read. One member the game will not answer unmakes the whole set,
        /// since a set missing a held item would say the group is not held.
        /// </remarks>
        /// <param name="group">An item group's name, such as <c>alcohol</c>.</param>
        private static DataAnswer HeldItemsInGroup(string group)
        {
            List<string>? members = MembersOf(group);
            if (members == null)
            {
                return DataAnswer.Unreadable();
            }

            var held = new List<string>();
            foreach (string item in members)
            {
                Lua.Result? result = GameFacts.Run("CheckItem(\"" + item + "\")");
                if (result == null)
                {
                    return DataAnswer.Unreadable();
                }

                if (IsTrue(result))
                {
                    held.Add(item);
                }
            }

            return DataAnswer.OfNames(held);
        }

        /// <summary>The names of the database's items in one group, or null if unreadable.</summary>
        private static List<string>? MembersOf(string group)
        {
            DialogueDatabase? database = DialogueManager.masterDatabase;
            string[]? names = ItemUtil.itemGroup;
            if (database == null || database.items == null || names == null)
            {
                return null;
            }

            var members = new List<string>();
            foreach (Item item in database.items)
            {
                int index = Field.LookupInt(item.fields, ItemGroupField);
                if (index >= 0 && index < names.Length && names[index] == group)
                {
                    members.Add(item.Name);
                }
            }

            return members;
        }

        /// <summary>Which of the thoughts the group asks about are in one state.</summary>
        /// <remarks>
        /// <para>ASKED PER NAME, THOUGH THE ANSWER IS A SET, and the two are worth keeping
        /// apart. The engine asks for the set because the game HOLDS one - the cabinet's
        /// cooking and fixed thoughts are two collections, and a save records them as two
        /// lists - so a set is the shape both sides should speak. How the plugin fills it is
        /// its own business.</para>
        ///
        /// <para>Walking those collections directly is what this wanted to do, the way
        /// <c>CharacterSheetPersister</c> does when writing a save. It is not available here:
        /// Il2CppInterop generates <c>CharacterThoughts</c> without <c>cookingEffects</c> or
        /// <c>fixedEffects</c>, which are <c>Dictionary&lt;ThoughtCabinetProject,
        /// CharacterEffect[]&gt;</c> and do not project. So the set is built by asking, over
        /// the thoughts the group named - the same list <see cref="FillMembers"/> walks.</para>
        ///
        /// <para>That leaves this family still evaluating Lua, which the data channel exists
        /// to stop. It is a PURE READ, which is the distinction that matters: the hazard the
        /// channel was built for is a guard calling <c>FinishTask</c> and closing a journal
        /// task, and nothing of that kind is here. What is still gained is that both sides
        /// now answer the question from one set rather than each deciding
        /// cooking-or-fixed for themselves.</para>
        /// </remarks>
        /// <param name="named">The thoughts the group asks about.</param>
        /// <param name="query">The Lua question that decides the state.</param>
        private static DataAnswer ThoughtsWhere(IReadOnlyList<string> named, string query)
        {
            var found = new List<string>();
            foreach (string thought in named)
            {
                Lua.Result? result = GameFacts.Run(query + "(\"" + thought + "\")");
                if (result == null)
                {
                    // THE WHOLE SET IS UNREADABLE, not this one thought. A set missing a
                    // member says the thought is definitely not in that state, and the
                    // engine would read it that way - so a question the game would not
                    // answer has to unmake the answer rather than shrink it.
                    return DataAnswer.Unreadable();
                }

                if (IsTrue(result))
                {
                    found.Add(thought);
                }
            }

            return DataAnswer.OfNames(found);
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
        /// therefore what <see cref="GameWorld"/> answers these three with: only
        /// false is false, and a number or a string is true. Written out here because
        /// requiring a boolean instead would have quietly answered "not held" for a query
        /// that returns a count.
        /// </remarks>
        private static bool IsTrue(Lua.Result result)
        {
            return result.isBool ? result.asBool : result.isNumber || result.isString;
        }

        /// <summary>Whether each entry's passive check fires, per <see cref="PassiveCheckRule"/>.</summary>
        private static void FillChecks(IReadOnlyList<NodeRef> checks, WorldRawData world)
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
                    CheckMargin? margin = PassiveCheckRule.MarginOf(entry, node);
                    if (margin != null)
                    {
                        world.CheckMargins.Add(margin);
                    }
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
        /// The flags of every white check the game holds as failed, from its own table.
        /// </summary>
        /// <remarks>
        /// <para>Not Lua: the game keeps a failed white check in
        /// <c>FailedWhiteChecks.ChecksBySkill</c> and refuses it while it stays there, and no
        /// dialogue variable says so. Without this a locked check reads as untried, and every
        /// option can route through it to whatever it would have opened.</para>
        ///
        /// <para>ALL OF THEM, not the group's: a few dozen names, and the engine only reads the
        /// ones its checks carry. A table that cannot be read leaves the set empty, which reads
        /// as nothing locked - the permissive direction, as everywhere else here.</para>
        /// </remarks>
        private static void FillFailedWhiteChecks(WorldRawData world)
        {
            try
            {
                var bySkill = FailedWhiteChecks.ChecksBySkill;
                if (bySkill == null)
                {
                    return;
                }

                foreach (var flags in bySkill.Values)
                {
                    if (flags == null)
                    {
                        continue;
                    }

                    foreach (string flag in flags)
                    {
                        world.FailedWhiteChecks.Add(flag);
                    }
                }
            }
            catch (System.Exception)
            {
                world.FailedWhiteChecks.Clear();
            }
        }

        /// <summary>
        /// Whether a thought the player holds forces every red check to fail.
        /// </summary>
        /// <remarks>
        /// <para>The game's own flag, <c>ThoughtAlterant.RedChecksFail</c>, set while a thought
        /// with that effect is applied. No dialogue variable says so, and without it a red
        /// check's success reads as open.</para>
        ///
        /// <para>A flag that cannot be read sends false, which leaves every red check free to
        /// succeed - the permissive direction, as everywhere else here.</para>
        /// </remarks>
        private static void FillRedChecksFail(WorldRawData world)
        {
            try
            {
                world.RedChecksFail = ThoughtAlterant.RedChecksFail;
            }
            catch (System.Exception)
            {
                world.RedChecksFail = false;
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
        private static void FillSeenState(
            IReadOnlyList<NodeRef> entries,
            GlobalStateSession session,
            WorldRawData world,
            LookAheadRequest request)
        {
            GlobalConversationState state = session.EnsureInitialized();
            foreach (NodeRef node in entries)
            {
                // TWO SETS, NOT THREE RUNGS. What THIS save has shown goes to the world, and
                // what SOME save has shown to the request - and the first implies the second, so
                // an entry the game calls seen joins both. The engine takes the three rungs from
                // the pair; nothing here decides one.
                if (GameFacts.IsSeen(node.Conversation, node.Entry))
                {
                    world.Seen.Add(node);
                    request.SeenAnyGame.Add(node);
                    continue;
                }

                if (state.GetStatus(node.Conversation, node.Entry) == SimStatus.WasDisplayed)
                {
                    request.SeenAnyGame.Add(node);
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
