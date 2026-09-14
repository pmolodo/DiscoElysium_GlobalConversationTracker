// SPDX-License-Identifier: MIT
using System.Collections.Generic;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// What a conversation has shown the player since it started, oldest first.
    /// </summary>
    /// <remarks>
    /// <para>A RECORDER AND NOTHING ELSE. The engine decides what a walk means - which hubs the
    /// player is inside, and what lies behind them - so the game and an offline run hand it the
    /// same kind of walk and get their answer from the same code.</para>
    ///
    /// <para>ONE ENTRY PER LINE, however many interfaces report it. The game has two dialogue
    /// interfaces of the same shape and the mod listens to both, so the same entry arriving twice
    /// in a row is one line shown once.</para>
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

        /// <summary>What has been shown, oldest first.</summary>
        public IReadOnlyList<NodeRef> Shown => _shown;

        /// <summary>Forgets the walk, for a conversation starting or ending.</summary>
        public void Clear() => _shown.Clear();

        /// <summary>Records one entry as shown.</summary>
        /// <param name="entry">The entry the game displayed.</param>
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
