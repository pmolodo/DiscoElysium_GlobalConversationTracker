// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Text;

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

        /// <summary>Entries SOME save has shown, which is what the global state records.</summary>
        /// <remarks>
        /// One of the two facts that decide an entry's seen state; the other is
        /// <see cref="WorldSnapshot.Seen"/>, what THIS save has shown. Neither is a claim about
        /// the other, and the engine takes the three states from the pair in one place. So
        /// nothing sends "unseen this save": an entry this world has not seen and this set holds
        /// IS that state.
        /// <para>
        /// EMPTY MEANS NOTHING HAS BEEN SEEN ANYWHERE. That is the opposite of what an empty set
        /// meant while the two were sent inverted, which is why the wire tag changed with it.
        /// </para>
        /// </remarks>
        public NodeSet SeenAnyGame { get; } = new NodeSet();

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

        /// <summary>
        /// What this conversation has shown the player since it started, oldest first: every
        /// line displayed and every option chosen. It must begin at the conversation's start,
        /// since the hubs are followed from there.
        /// </summary>
        /// <remarks>
        /// WHERE THE PLAYER HAS BEEN, which the options alone cannot say. The engine follows
        /// the hubs the player has passed through, and treats a route back through anything
        /// passed since them as looping back rather than leading onward. Empty says nothing
        /// about where the player is.
        /// </remarks>
        public IList<NodeRef> Encountered { get; } = new List<NodeRef>();
    }
}
