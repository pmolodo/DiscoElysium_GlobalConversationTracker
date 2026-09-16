// SPDX-License-Identifier: MIT
using System.Collections.Generic;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// Every entry a conversation has stepped through since it started, oldest first.
    /// </summary>
    /// <remarks>
    /// <para>A RECORDER AND NOTHING ELSE. The engine decides what a walk means - which hubs the
    /// player is inside, and what lies behind them - so the game and an offline run hand it the
    /// same kind of walk and get their answer from the same code.</para>
    ///
    /// <para>ONE ENTRY PER STEP. The game asks an entry's links once per condition priority, so
    /// the same entry arriving twice in a row is one step taken once. An entry arriving again
    /// LATER is a second visit and is kept: arriving back at a hub is what gives its topics
    /// back.</para>
    ///
    /// <para>BOUNDED, keeping the newest. A conversation long enough to overflow this is walked
    /// from part way in, and the engine takes the first hub it meets there as the outermost; that
    /// can only cut less than a full walk would, never more.</para>
    /// </remarks>
    public sealed class ConversationWalk
    {
        /// <summary>The most entries kept.</summary>
        public const int Capacity = 1024;

        private readonly List<NodeRef> _shown = new List<NodeRef>();

        /// <summary>What has been stepped through, oldest first.</summary>
        public IReadOnlyList<NodeRef> Shown => _shown;

        /// <summary>Forgets the walk, for a conversation starting or ending.</summary>
        public void Clear() => _shown.Clear();

        /// <summary>Records one entry as stepped through.</summary>
        /// <param name="entry">The entry the game walked, displayed or not.</param>
        public void Record(NodeRef entry)
        {
            if (_shown.Count > 0 && _shown[_shown.Count - 1] == entry)
            {
                return;
            }

            _shown.Add(entry);
            if (_shown.Count > Capacity)
            {
                _shown.RemoveAt(0);
            }
        }
    }
}
