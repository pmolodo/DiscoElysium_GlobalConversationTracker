using System.Collections.Generic;

namespace GlobalConversationTracker
{
    /// <summary>
    /// How many dialogue entries are above Untouched <em>in the save currently being
    /// played</em>, kept correct as the game marks and unmarks them.
    /// </summary>
    /// <remarks>
    /// <para><b>Why this is not <see cref="GlobalConversationState"/>.</b> That type
    /// answers "has this line ever been reached, in any save", and to do that it
    /// enforces a merge rule where a status can only ever go up. This one answers
    /// "does this line count for the save on screen right now", where a status going
    /// down is a real event that has to be honoured:
    /// <c>DialogueLua.MarkDialogueEntryUntouched</c> exists and the game uses it. The
    /// two are deliberately separate objects with opposite rules, rather than one
    /// object with a mode.</para>
    ///
    /// <para><b>Why a set and not a counter.</b> The game re-marks entries constantly -
    /// the same line is offered again and again - so an increment per mark would count
    /// the same entry many times, and a decrement per Untouched would run the total
    /// below zero. Membership is the only thing that answers "did this mark actually
    /// change the total", and <see cref="Count"/> is then the set's own size rather
    /// than a number that has to be kept in step with it.</para>
    ///
    /// <para><b>What is stored.</b> One <c>long</c> per entry, packing the
    /// conversation ID into the high half and the entry ID into the low half. Nothing
    /// else: which of the two non-Untouched statuses an entry holds does not affect
    /// the count, so it is not kept. Entries at Untouched are absent rather than
    /// stored, exactly as in the global state.</para>
    ///
    /// <para>Not thread safe, and it does not synchronize itself; the session that
    /// owns it holds the lock, the same as for the global state.</para>
    /// </remarks>
    public sealed class CurrentSaveTally
    {
        private readonly HashSet<long> _aboveUntouched = new HashSet<long>();

        /// <summary>How many entries in the current save are above Untouched.</summary>
        public int Count => _aboveUntouched.Count;

        /// <summary>True when the current save has no entry above Untouched.</summary>
        public bool IsEmpty => _aboveUntouched.Count == 0;

        /// <summary>
        /// Records one entry's status in the current save, replacing whatever was
        /// there.
        /// </summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="dialogueEntryId">The dialogue entry's integer ID.</param>
        /// <param name="status">
        /// The status the game just wrote. <see cref="SimStatus.Untouched"/> removes
        /// the entry, which is the downgrade case this type exists for.
        /// </param>
        /// <returns>
        /// <c>true</c> if <see cref="Count"/> changed. Re-marking an entry that is
        /// already counted, or clearing one that was not, returns <c>false</c>.
        /// </returns>
        public bool Set(int conversationId, int dialogueEntryId, SimStatus status)
        {
            long key = Key(conversationId, dialogueEntryId);
            return status == SimStatus.Untouched
                ? _aboveUntouched.Remove(key)
                : _aboveUntouched.Add(key);
        }

        /// <summary>
        /// Records one entry's status supplied as one of the game's status strings,
        /// without throwing on an unrecognized value.
        /// </summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="dialogueEntryId">The dialogue entry's integer ID.</param>
        /// <param name="statusName">The incoming status string from the game.</param>
        /// <param name="changed">
        /// Set to <c>true</c> if <see cref="Count"/> changed. Always <c>false</c> when
        /// the return value is <c>false</c>.
        /// </param>
        /// <returns>
        /// <c>true</c> if <paramref name="statusName"/> was recognized. An
        /// unrecognized one leaves the tally untouched, so a status the mod does not
        /// understand cannot silently drop an entry out of the count.
        /// </returns>
        public bool TrySet(int conversationId, int dialogueEntryId, string? statusName, out bool changed)
        {
            if (!SimStatusNames.TryParse(statusName, out SimStatus status))
            {
                changed = false;
                return false;
            }

            changed = Set(conversationId, dialogueEntryId, status);
            return true;
        }

        /// <summary>
        /// Whether this entry counts towards <see cref="Count"/> in the current save.
        /// </summary>
        public bool Contains(int conversationId, int dialogueEntryId) =>
            _aboveUntouched.Contains(Key(conversationId, dialogueEntryId));

        /// <summary>
        /// Empties the tally, for a savegame load about to refill it or a new game
        /// that has thrown the old save's statuses away.
        /// </summary>
        /// <returns>How many entries were dropped, so the caller can say so.</returns>
        public int Clear()
        {
            int dropped = _aboveUntouched.Count;
            _aboveUntouched.Clear();
            return dropped;
        }

        /// <inheritdoc />
        public override string ToString() => $"CurrentSaveTally({Count} entries above Untouched)";

        /// <summary>
        /// Packs the two IDs into one key: conversation in the high 32 bits, entry in
        /// the low 32. Both are the game's own <c>int</c> IDs, so the pair is unique
        /// and the packing is lossless.
        /// </summary>
        private static long Key(int conversationId, int dialogueEntryId) =>
            ((long)conversationId << 32) | (uint)dialogueEntryId;
    }
}
