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

        /// <summary>Entries the player has never seen in any save.</summary>
        public NodeSet UnseenAnyGame { get; } = new NodeSet();

        /// <summary>Entries unseen this save but seen in a previous one.</summary>
        public NodeSet UnseenThisGame { get; } = new NodeSet();

        /// <summary>
        /// The most search states one option may cost, or 0 for the engine's own default.
        /// </summary>
        /// <remarks>
        /// The player's <c>LookAheadStateBudget</c>, and it has to cross. Once the marker
        /// comes from the engine on the other side, a budget configured here and ignored
        /// there would be a dial connected to nothing.
        /// </remarks>
        public int StateBudget { get; set; }

        /// <summary>
        /// The longest one option may run for, in milliseconds; 0 for no limit.
        /// </summary>
        /// <remarks>
        /// Zero means NO LIMIT rather than a default, which is what
        /// <c>LookAheadTimeBudgetMs</c> documents it as. The state budget's zero means the
        /// opposite - use the default - because that setting has no "off".
        /// </remarks>
        public int TimeBudgetMs { get; set; }

        /// <summary>The request as the JSON the library reads.</summary>
        public string ToJson()
        {
            var buffer = new System.IO.MemoryStream();
            using (var writer = new Utf8JsonWriter(buffer))
            {
                writer.WriteStartObject();
                writer.WriteNumber("conversation", Conversation);

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

                writer.WritePropertyName("world");
                World.Write(writer);

                writer.WriteEndObject();
            }

            return Encoding.UTF8.GetString(buffer.ToArray());
        }
    }
}
