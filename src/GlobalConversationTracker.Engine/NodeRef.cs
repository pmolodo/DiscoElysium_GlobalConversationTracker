// SPDX-License-Identifier: MIT
using System;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// One dialogue entry, as it crosses the bridge.
    /// </summary>
    /// <remarks>
    /// The same pair as <c>DialogueNodeId</c> in the look-ahead engine and as
    /// <c>NodeRef</c> in the Rust one. Kept separate from both rather than reusing either,
    /// because this is the WIRE type: it exists to be written and read, and giving it a
    /// dependency on the managed engine would tie the bridge to the assembly it is
    /// replacing.
    /// </remarks>
    public readonly struct NodeRef : IEquatable<NodeRef>
    {
        /// <summary>Creates a reference.</summary>
        /// <param name="conversation">The conversation id.</param>
        /// <param name="entry">The entry id within it.</param>
        public NodeRef(int conversation, int entry)
        {
            Conversation = conversation;
            Entry = entry;
        }

        /// <summary>The conversation this entry belongs to.</summary>
        public int Conversation { get; }

        /// <summary>The entry's id within its conversation.</summary>
        public int Entry { get; }

        /// <inheritdoc/>
        public bool Equals(NodeRef other)
        {
            return Conversation == other.Conversation && Entry == other.Entry;
        }

        /// <inheritdoc/>
        public override bool Equals(object? obj)
        {
            return obj is NodeRef other && Equals(other);
        }

        /// <inheritdoc/>
        public override int GetHashCode()
        {
            unchecked
            {
                return (Conversation * 397) ^ Entry;
            }
        }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{Conversation}:{Entry}";
        }

        /// <summary>Whether two references name the same entry.</summary>
        /// <param name="left">One reference.</param>
        /// <param name="right">The other.</param>
        public static bool operator ==(NodeRef left, NodeRef right) => left.Equals(right);

        /// <summary>Whether two references name different entries.</summary>
        /// <param name="left">One reference.</param>
        /// <param name="right">The other.</param>
        public static bool operator !=(NodeRef left, NodeRef right) => !left.Equals(right);
    }
}
