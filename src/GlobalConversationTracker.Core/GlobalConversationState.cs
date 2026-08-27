// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Linq;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The in-memory global ("across all saves") conversation state: conversation ID
    /// to dialogue entry ID to <see cref="SimStatus"/>.
    /// </summary>
    /// <remarks>
    /// <para>
    /// This mirrors the shape of the <c>Conversation</c> table in the game's
    /// <c>{save}.ntwtf.lua</c> data, flattened to drop everything except SimStatus.
    /// </para>
    /// <para>
    /// <b>The merge rule is the only way this object is ever mutated.</b> There is no
    /// setter, no indexer, and no exposed mutable collection anywhere on this type;
    /// every mutating method routes through
    /// <see cref="Merge(int, int, SimStatus)"/>. A status may only increase, ordered
    /// <c>Untouched &lt; WasOffered &lt; WasDisplayed</c>. An attempt to lower a
    /// status is a silent no-op, not an error. This matters because the game itself
    /// downgrades entries: <c>DialogueLua.MarkDialogueEntryUntouched</c> writes
    /// "Untouched" over an existing status, and a newly started save resets every
    /// entry. The global state must absorb those calls without losing history.
    /// </para>
    /// <para>
    /// <see cref="SimStatus.Untouched"/> is the bottom of the ordering and is also the
    /// value reported for anything not present, so it is never stored: merging
    /// <see cref="SimStatus.Untouched"/> into an absent entry records nothing and
    /// reports no change. Only entries that have advanced past Untouched occupy space
    /// in memory or on disk.
    /// </para>
    /// <para>
    /// Not thread safe, and it does not synchronize itself. Marks arrive from the
    /// Unity main thread, and a background writer reads the state too - but only long
    /// enough to take a <see cref="Snapshot"/> and then work on that, so the window
    /// the two have to be kept apart for is a copy rather than a whole serialize.
    /// <c>GlobalStateSession</c> owns the lock that keeps them apart, and is the only
    /// thing that should be reaching this object once a session exists.
    /// </para>
    /// </remarks>
    public sealed class GlobalConversationState
    {
        private readonly Dictionary<int, Dictionary<int, SimStatus>> _conversations =
            new Dictionary<int, Dictionary<int, SimStatus>>();

        /// <summary>
        /// Conversation titles whose orb has been opened in any save. The merge rule
        /// degenerates to set insertion here: an orb has one state, the game never
        /// unsets <c>OrbSeen</c>, and so "keep the higher value" and "add to the set"
        /// are the same operation.
        /// </summary>
        private readonly HashSet<string> _orbs = new HashSet<string>(StringComparer.Ordinal);

        /// <summary>
        /// How many recorded entries sit at each of the two statuses that are stored.
        /// Kept as the merge goes rather than counted on demand, because the display
        /// asks for the total on every mark and the state runs to six figures.
        /// </summary>
        private int _offeredCount;

        private int _displayedCount;

        /// <summary>Creates an empty state.</summary>
        public GlobalConversationState()
        {
        }

        /// <summary>
        /// Adopts an already-built store of entries, for <see cref="Snapshot"/>. Private
        /// because it is the one way into this type that does not go through the merge
        /// rule, and it is only safe because the caller is this class handing over
        /// dictionaries it has just copied out of itself.
        /// </summary>
        private GlobalConversationState(
            Dictionary<int, Dictionary<int, SimStatus>> conversations,
            int offeredCount,
            int displayedCount,
            HashSet<string> orbs)
        {
            _conversations = conversations;
            _offeredCount = offeredCount;
            _displayedCount = displayedCount;
            _orbs = orbs;
        }

        /// <summary>Number of conversations that have at least one recorded entry.</summary>
        public int ConversationCount => _conversations.Count;

        /// <summary>Total number of recorded dialogue entries across all conversations.</summary>
        public int EntryCount => _offeredCount + _displayedCount;

        /// <summary>
        /// How many recorded entries were offered somewhere but never displayed
        /// anywhere.
        /// </summary>
        public int OfferedCount => _offeredCount;

        /// <summary>How many recorded entries were displayed in some save.</summary>
        public int DisplayedCount => _displayedCount;

        /// <summary>How many distinct orbs have been opened across all saves.</summary>
        public int OrbCount => _orbs.Count;

        /// <summary>
        /// What everything recorded is worth: offered entries count half, displayed
        /// ones and orbs whole. This is the number the player is shown.
        /// </summary>
        public double Score => DialogueScore.Total(_offeredCount, _displayedCount, _orbs.Count);

        /// <summary>True when nothing has been recorded yet, neither entry nor orb.</summary>
        public bool IsEmpty => EntryCount == 0 && _orbs.Count == 0;

        // -------------------------------------------------------------------
        // Mutation: merge is the only path in.
        // -------------------------------------------------------------------

        /// <summary>
        /// Merges a status for one dialogue entry, keeping the higher of the existing
        /// and incoming values.
        /// </summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="dialogueEntryId">The dialogue entry's integer ID.</param>
        /// <param name="status">The incoming status.</param>
        /// <returns>
        /// <c>true</c> if the stored state actually changed (a new entry was recorded
        /// or an existing one was raised); <c>false</c> if the incoming status was
        /// equal to or lower than what was already recorded. Callers can use this to
        /// decide whether a rewrite to disk is needed.
        /// </returns>
        /// <exception cref="ArgumentOutOfRangeException">
        /// <paramref name="status"/> is not one of the three defined
        /// <see cref="SimStatus"/> values.
        /// </exception>
        public bool Merge(int conversationId, int dialogueEntryId, SimStatus status)
        {
            switch (status)
            {
                case SimStatus.Untouched:
                    // Bottom of the ordering: can never raise anything, and is the
                    // implicit value of every absent entry, so nothing to do.
                    return false;
                case SimStatus.WasOffered:
                case SimStatus.WasDisplayed:
                    break;
                default:
                    throw new ArgumentOutOfRangeException(
                        nameof(status),
                        status,
                        "Not a defined SimStatus value.");
            }

            if (!_conversations.TryGetValue(conversationId, out Dictionary<int, SimStatus>? entries))
            {
                entries = new Dictionary<int, SimStatus>();
                _conversations.Add(conversationId, entries);
            }

            if (entries.TryGetValue(dialogueEntryId, out SimStatus existing))
            {
                if (status <= existing)
                {
                    // Downgrade or no change: silent no-op, by design.
                    return false;
                }

                entries[dialogueEntryId] = status;

                // There are only two storable statuses, so the one raise that reaches
                // here is WasOffered -> WasDisplayed: the entry stops being half a line
                // and becomes a whole one.
                _offeredCount--;
                _displayedCount++;
                return true;
            }

            entries.Add(dialogueEntryId, status);
            if (status == SimStatus.WasOffered)
            {
                _offeredCount++;
            }
            else
            {
                _displayedCount++;
            }

            return true;
        }

        /// <summary>
        /// Merges a status supplied as one of the game's status strings.
        /// </summary>
        /// <returns>
        /// <c>true</c> if the stored state actually changed; see
        /// <see cref="Merge(int, int, SimStatus)"/>.
        /// </returns>
        /// <exception cref="ArgumentException">
        /// <paramref name="statusName"/> is not one of the three strings the game uses.
        /// Use <see cref="TryMerge"/> at call sites (such as the game hook) that must
        /// not throw.
        /// </exception>
        public bool Merge(int conversationId, int dialogueEntryId, string? statusName)
        {
            return Merge(conversationId, dialogueEntryId, SimStatusNames.Parse(statusName));
        }

        /// <summary>
        /// Merges a status supplied as one of the game's status strings, without
        /// throwing on an unrecognized value.
        /// </summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="dialogueEntryId">The dialogue entry's integer ID.</param>
        /// <param name="statusName">The incoming status string from the game.</param>
        /// <param name="changed">
        /// Set to <c>true</c> if the stored state actually changed. Always <c>false</c>
        /// when the return value is <c>false</c>.
        /// </param>
        /// <returns>
        /// <c>true</c> if <paramref name="statusName"/> was recognized (and therefore
        /// merged); <c>false</c> if it was not, in which case the state is left
        /// completely untouched so the caller can log and carry on.
        /// </returns>
        public bool TryMerge(int conversationId, int dialogueEntryId, string? statusName, out bool changed)
        {
            if (!SimStatusNames.TryParse(statusName, out SimStatus status))
            {
                changed = false;
                return false;
            }

            changed = Merge(conversationId, dialogueEntryId, status);
            return true;
        }

        /// <summary>
        /// Merges a batch of entries.
        /// </summary>
        /// <returns>The number of entries that actually changed the stored state.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="entries"/> is null.</exception>
        /// <exception cref="ArgumentOutOfRangeException">
        /// An entry carries a status that is not a defined <see cref="SimStatus"/> value.
        /// </exception>
        public int MergeAll(IEnumerable<GlobalStatusEntry> entries)
        {
            if (entries == null)
            {
                throw new ArgumentNullException(nameof(entries));
            }

            int changedCount = 0;
            foreach (GlobalStatusEntry entry in entries)
            {
                if (Merge(entry.ConversationId, entry.DialogueEntryId, entry.Status))
                {
                    changedCount++;
                }
            }

            return changedCount;
        }

        /// <summary>
        /// Records that an orb has been opened. Idempotent: an orb already recorded is
        /// a no-op, because <c>OrbSeen</c> has no state above 1 to be raised to.
        /// </summary>
        /// <param name="conversationTitle">
        /// The orb's conversation title, the key the game itself uses in
        /// <c>ShownOrbs</c>. See <see cref="CurrentSaveTally.SetOrb"/> for why a title
        /// and not a conversation ID.
        /// </param>
        /// <returns><c>true</c> if this orb was not already recorded.</returns>
        /// <exception cref="ArgumentException">
        /// <paramref name="conversationTitle"/> is null or empty; the game never writes
        /// such a key.
        /// </exception>
        public bool MergeOrb(string conversationTitle)
        {
            if (string.IsNullOrEmpty(conversationTitle))
            {
                throw new ArgumentException(
                    "An orb's conversation title must not be null or empty.",
                    nameof(conversationTitle));
            }

            return _orbs.Add(conversationTitle);
        }

        /// <summary>
        /// Merges many orbs at once, for a file load or a resync.
        /// </summary>
        /// <returns>How many of them were not already recorded.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="conversationTitles"/> is null.</exception>
        /// <exception cref="ArgumentException">Any title is null or empty.</exception>
        public int MergeAllOrbs(IEnumerable<string> conversationTitles)
        {
            if (conversationTitles == null)
            {
                throw new ArgumentNullException(nameof(conversationTitles));
            }

            int changedCount = 0;
            foreach (string title in conversationTitles)
            {
                if (MergeOrb(title))
                {
                    changedCount++;
                }
            }

            return changedCount;
        }

        /// <summary>
        /// Merges every entry and orb of another state into this one.
        /// </summary>
        /// <returns>
        /// The number of entries and orbs together that actually changed the stored
        /// state.
        /// </returns>
        /// <exception cref="ArgumentNullException"><paramref name="other"/> is null.</exception>
        public int MergeAll(GlobalConversationState other)
        {
            if (other == null)
            {
                throw new ArgumentNullException(nameof(other));
            }

            if (ReferenceEquals(other, this))
            {
                return 0;
            }

            return MergeAll(other.EnumerateEntries()) + MergeAllOrbs(other.EnumerateOrbs());
        }

        // -------------------------------------------------------------------
        // Read-only access.
        // -------------------------------------------------------------------

        /// <summary>
        /// Returns the recorded status, or <see cref="SimStatus.Untouched"/> when the
        /// conversation or the entry is unknown.
        /// </summary>
        public SimStatus GetStatus(int conversationId, int dialogueEntryId)
        {
            TryGetStatus(conversationId, dialogueEntryId, out SimStatus status);
            return status;
        }

        /// <summary>
        /// Looks up a recorded status, distinguishing "absent" from "recorded as
        /// Untouched" (the latter never happens; see the type's remarks).
        /// </summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="dialogueEntryId">The dialogue entry's integer ID.</param>
        /// <param name="status">
        /// The recorded status, or <see cref="SimStatus.Untouched"/> if absent.
        /// </param>
        /// <returns><c>true</c> if an entry was recorded for this pair.</returns>
        public bool TryGetStatus(int conversationId, int dialogueEntryId, out SimStatus status)
        {
            if (_conversations.TryGetValue(conversationId, out Dictionary<int, SimStatus>? entries)
                && entries.TryGetValue(dialogueEntryId, out status))
            {
                return true;
            }

            status = SimStatus.Untouched;
            return false;
        }

        /// <summary>True if the conversation has at least one recorded entry.</summary>
        public bool ContainsConversation(int conversationId) => _conversations.ContainsKey(conversationId);

        /// <summary>
        /// The IDs of every conversation with at least one recorded entry, in
        /// unspecified order.
        /// </summary>
        public IEnumerable<int> ConversationIds
        {
            get
            {
                // Iterator method rather than returning the key collection directly:
                // callers get a snapshot-free read-only sequence they cannot cast
                // back to the underlying dictionary.
                foreach (int conversationId in _conversations.Keys)
                {
                    yield return conversationId;
                }
            }
        }

        /// <summary>
        /// The recorded entries of one conversation, in unspecified order. Yields
        /// nothing for an unknown conversation ID.
        /// </summary>
        public IEnumerable<KeyValuePair<int, SimStatus>> GetConversationEntries(int conversationId)
        {
            if (!_conversations.TryGetValue(conversationId, out Dictionary<int, SimStatus>? entries))
            {
                yield break;
            }

            foreach (KeyValuePair<int, SimStatus> pair in entries)
            {
                yield return pair;
            }
        }

        /// <summary>
        /// Every recorded entry, flattened, in unspecified order. This is the cheap
        /// enumeration; use <see cref="EnumerateEntriesInIdOrder"/> when the output
        /// order has to be stable.
        /// </summary>
        public IEnumerable<GlobalStatusEntry> EnumerateEntries()
        {
            foreach (KeyValuePair<int, Dictionary<int, SimStatus>> conversation in _conversations)
            {
                foreach (KeyValuePair<int, SimStatus> entry in conversation.Value)
                {
                    yield return new GlobalStatusEntry(conversation.Key, entry.Key, entry.Value);
                }
            }
        }

        /// <summary>
        /// Every recorded entry, flattened and sorted by conversation ID then dialogue
        /// entry ID. Use this when serializing so the on-disk file is deterministic.
        /// </summary>
        public IEnumerable<GlobalStatusEntry> EnumerateEntriesInIdOrder()
        {
            foreach (int conversationId in _conversations.Keys.OrderBy(id => id))
            {
                Dictionary<int, SimStatus> entries = _conversations[conversationId];
                foreach (int entryId in entries.Keys.OrderBy(id => id))
                {
                    yield return new GlobalStatusEntry(conversationId, entryId, entries[entryId]);
                }
            }
        }

        /// <summary>
        /// Every recorded orb's conversation title, sorted, so the on-disk file is
        /// deterministic. Ordinal order, matching the set's own comparer.
        /// </summary>
        public IEnumerable<string> EnumerateOrbs() => _orbs.OrderBy(t => t, StringComparer.Ordinal);

        /// <summary>Whether this orb has been recorded in any save.</summary>
        public bool ContainsOrb(string conversationTitle) =>
            conversationTitle != null && _orbs.Contains(conversationTitle);

        /// <summary>
        /// A deep copy of this state, sharing nothing with it: neither object can be
        /// changed by anything done to the other.
        /// </summary>
        /// <remarks>
        /// <para>This exists so the background writer never serializes the live state.
        /// Serializing reads every entry, so it has to be kept apart from the merges
        /// arriving on the Unity main thread; copying under the session lock and
        /// serializing the copy holds that lock only for the copy, which the benchmark
        /// measures at 24-25x cheaper than the serialize at every size it sweeps.</para>
        /// <para>The copy is deep in the only sense that matters here: the outer
        /// dictionary and every inner one are new, and <see cref="SimStatus"/> is an enum,
        /// so there is nothing left that could still be shared. The result is an ordinary
        /// state - it can be merged into, and it serializes byte-identically to its
        /// source, since it holds exactly the same entries.</para>
        /// </remarks>
        public GlobalConversationState Snapshot() =>
            new GlobalConversationState(
                ToNestedDictionary(),
                _offeredCount,
                _displayedCount,
                new HashSet<string>(_orbs, StringComparer.Ordinal));

        /// <summary>
        /// A deep copy of the state as plain nested dictionaries. Prefer
        /// <see cref="Snapshot"/> unless the raw dictionaries are what is wanted;
        /// mutating the returned dictionaries does not affect this object.
        /// </summary>
        public Dictionary<int, Dictionary<int, SimStatus>> ToNestedDictionary()
        {
            var copy = new Dictionary<int, Dictionary<int, SimStatus>>(_conversations.Count);
            foreach (KeyValuePair<int, Dictionary<int, SimStatus>> conversation in _conversations)
            {
                copy.Add(conversation.Key, new Dictionary<int, SimStatus>(conversation.Value));
            }

            return copy;
        }

        /// <inheritdoc />
        public override string ToString()
        {
            return $"GlobalConversationState({ConversationCount} conversations, {EntryCount} entries, "
                + $"{OrbCount} orbs)";
        }
    }
}
