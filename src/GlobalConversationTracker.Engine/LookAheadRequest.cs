// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Text;
using System.Text.Json;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// One look-ahead question: which options to score, against what world.
    /// </summary>
    /// <remarks>
    /// <para>The whole menu in one request. <see cref="Starts"/> is a list because the
    /// world is the same for every option drawn at once, and it is the world that is
    /// expensive to send - the 306 variable names of conversation 631's group cost more
    /// than everything else in the request together.</para>
    ///
    /// <para>The graph does not cross. The engine builds it from the conversation index
    /// shipped with the mod, so this names a conversation rather than marshalling four
    /// thousand entries per menu.</para>
    /// </remarks>
    public sealed class LookAheadRequest
    {
        /// <summary>Creates a request against one conversation group.</summary>
        /// <param name="conversation">
        /// Any conversation in the group; the engine loads the whole group from it.
        /// </param>
        /// <param name="world">The player's situation.</param>
        /// <exception cref="ArgumentNullException">The world is null.</exception>
        public LookAheadRequest(int conversation, WorldSnapshot world)
        {
            Conversation = conversation;
            World = world ?? throw new ArgumentNullException(nameof(world));
        }

        /// <summary>Any conversation in the group to crawl.</summary>
        public int Conversation { get; }

        /// <summary>The player's situation. One answer comes back per start.</summary>
        public WorldSnapshot World { get; }

        /// <summary>The option entries to score.</summary>
        public IList<NodeRef> Starts { get; } = new List<NodeRef>();

        /// <summary>Score Starts together as the options of one response menu.</summary>
        public bool Menu { get; set; }

        /// <summary>Entries the player has never seen in any save.</summary>
        public NodeSet UnseenAnyGame { get; } = new NodeSet();

        /// <summary>Entries unseen this save but seen in a previous one.</summary>
        public NodeSet UnseenThisGame { get; } = new NodeSet();

        /// <summary>
        /// The most search states one option may hold, or 0 for no such limit.
        /// </summary>
        /// <remarks>
        /// A TEST-ONLY KNOB, and the only budget here that is not a player's. No
        /// configuration setting writes it: <c>LookAheadStateBudget</c> was removed in
        /// de-7z0f because a count of search states is not a quantity anybody outside this
        /// repository can reason about. What a player sets is memory and time.
        ///
        /// It survives because it is the only limit that gives an exactly reproducible
        /// give-up, which the in-game suites that starve a crawl need. The memory budget
        /// cannot do that job at the small end: it crosses in megabytes, and a megabyte is
        /// more than a small group's whole crawl costs.
        /// </remarks>
        public int StateBudget { get; set; }

        /// <summary>
        /// The longest one option may run for, in milliseconds; 0 for no limit.
        /// </summary>
        /// <remarks>
        /// Zero means NO LIMIT rather than a default, which is what
        /// <c>LookAheadTimeBudgetMs</c> documents it as. <see cref="MemoryBudgetMb"/>'s
        /// zero means the opposite - use the engine's default - because that setting has
        /// no "off": a crawl always has some ceiling on what it may hold.
        /// </remarks>
        public int TimeBudgetMs { get; set; }

        /// <summary>
        /// The longest the WHOLE MENU may run for, in milliseconds; 0 for no limit.
        /// </summary>
        /// <remarks>
        /// <para>ONE OPTION'S BUDGET DOES NOT BOUND A MENU. <see cref="TimeBudgetMs"/> bounds
        /// one option and this request is a whole menu, so what the player waits for is the
        /// sum - and a rolled check is two searches rather than one, so a wide menu's worst
        /// case is the per-option dial times twice its option count. Until de-dt75.3 nothing
        /// bounded that sum, and the only thing further out was the host's read deadline,
        /// which is not a budget: crossing it kills the engine mid-conversation.</para>
        ///
        /// <para>WHAT AN OPTION THE WALL STOPS COMES BACK AS. An ordinary gave-up answer, so
        /// the mod marks it uncertain rather than leaving it out - the same marker an option
        /// whose own budget ran out gets, and the same meaning: a search ran and did not
        /// finish. The options that lose are the ones at the BOTTOM of the menu, since the
        /// engine answers the starts in the order they are sent.</para>
        ///
        /// <para>Zero means no limit, like <see cref="TimeBudgetMs"/>'s zero and unlike
        /// <see cref="MemoryBudgetMb"/>'s.</para>
        /// </remarks>
        public int MenuTimeBudgetMs { get; set; }

        /// <summary>
        /// The most memory one option's search may hold, in MEGABYTES; 0 for the default.
        /// </summary>
        /// <remarks>
        /// <para>THE LIMIT THAT GOVERNS. A search state carries one slot per variable its
        /// group tracks, so a budget counted in STATES buys a different amount of memory in
        /// every conversation - between 136 and 455 megabytes across the six heaviest,
        /// measured. A number that elastic protects nothing in particular, which is why the
        /// budget is now stated in the unit it is actually spent in. See de-e23q.</para>
        ///
        /// <para>MEGABYTES HERE, BYTES IN THE ENGINE. This is a figure a player types into
        /// a configuration file, and 300 is a number a person can hold in their head where
        /// 314572800 is not.</para>
        ///
        /// <para>ZERO MEANS THE DEFAULT, not "no limit" - the opposite of what zero means
        /// for the time budget, and deliberately so. A crawl with no clock still finishes;
        /// a crawl with no memory limit is the thing this budget exists to prevent, so an
        /// unset value must not switch the protection off.</para>
        /// </remarks>
        public int MemoryBudgetMb { get; set; }


        /// <summary>The request as the JSON the library reads.</summary>
        public string ToJson()
        {
            var buffer = new System.IO.MemoryStream();
            using (var writer = new Utf8JsonWriter(buffer))
            {
                writer.WriteStartObject();
                writer.WriteNumber("conversation", Conversation);
                writer.WriteBoolean("menu", Menu);

                writer.WritePropertyName("starts");
                writer.WriteStartArray();
                foreach (NodeRef start in Starts)
                {
                    writer.WriteStartObject();
                    writer.WriteNumber("conversation", start.Conversation);
                    writer.WriteNumber("entry", start.Entry);
                    writer.WriteEndObject();
                }

                writer.WriteEndArray();

                writer.WritePropertyName("unseen_any_game");
                UnseenAnyGame.Write(writer);
                writer.WritePropertyName("unseen_this_game");
                UnseenThisGame.Write(writer);

                writer.WriteNumber("state_budget", Math.Max(0, StateBudget));
                writer.WriteNumber("time_budget_ms", Math.Max(0, TimeBudgetMs));
                writer.WriteNumber("menu_time_budget_ms", Math.Max(0, MenuTimeBudgetMs));
                writer.WriteNumber("memory_budget_mb", Math.Max(0, MemoryBudgetMb));

                writer.WritePropertyName("world");
                World.Write(writer);

                writer.WriteEndObject();
            }

            return Encoding.UTF8.GetString(buffer.ToArray());
        }
    }
}
