// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Which dialogue entries are above Untouched <em>in the save currently being
    /// played</em>, split by how far above.
    /// </summary>
    /// <remarks>
    /// <para>The counterpart to <see cref="GlobalConversationState"/>, with the opposite
    /// merge rule. That type answers "has this line ever been reached, in any save", so a
    /// status there can only go up. This one answers "does this line count for the save
    /// on screen", where a downgrade is a real event -
    /// <c>DialogueLua.MarkDialogueEntryUntouched</c> exists and the game uses it. Two
    /// separate objects rather than one with a mode.</para>
    ///
    /// <para>Sets, not counters. The game re-marks the same entry constantly, so an
    /// increment per mark would count it many times over and a decrement per Untouched
    /// would run the total below zero. Membership is what answers "did this mark change
    /// the total". Two sets because the statuses are worth different amounts (see
    /// <see cref="DialogueScore"/>); an entry is in at most one, since reaching
    /// <see cref="SimStatus.WasDisplayed"/> takes it out of the offered set.</para>
    ///
    /// <para>Untouched is the only downgrade honoured. Re-marking a displayed line
    /// <see cref="SimStatus.WasOffered"/> is the game re-offering a response the player
    /// has already seen, not the player unseeing it, so the entry stays displayed.</para>
    ///
    /// <para>Orbs are a third set. An orb has no entry ID and no ordering - the game
    /// records a bare <c>ShownOrbs[title].OrbSeen=1</c> and never unsets it - so it is
    /// keyed by title, arrives from a different file on load, and would have to
    /// masquerade as a dialogue entry at some invented ID to be folded in. Each is worth
    /// <see cref="DialogueScore.Orb"/> towards <see cref="Score"/>.</para>
    ///
    /// <para>Not thread safe; the owning session holds the lock, as for the global
    /// state.</para>
    /// </remarks>
    public sealed class CurrentSaveTally
    {
        private readonly HashSet<long> _offered = new HashSet<long>();
        private readonly HashSet<long> _displayed = new HashSet<long>();

        /// <summary>
        /// Conversation titles whose orb has been opened in this save - the keys of the
        /// game's own <c>ShownOrbs</c> table. See <see cref="SetOrb"/> for why the key
        /// is a title rather than an ID.
        /// </summary>
        private readonly HashSet<string> _orbs = new HashSet<string>(StringComparer.Ordinal);

        /// <summary>
        /// How many entries in the current save were offered but never displayed.
        /// </summary>
        public int OfferedCount => _offered.Count;

        /// <summary>How many entries in the current save were actually displayed.</summary>
        public int DisplayedCount => _displayed.Count;

        /// <summary>How many entries in the current save are above Untouched.</summary>
        /// <remarks>Dialogue entries only; orbs are <see cref="OrbCount"/>.</remarks>
        public int Count => _offered.Count + _displayed.Count;

        /// <summary>How many distinct orbs have been opened in the current save.</summary>
        public int OrbCount => _orbs.Count;

        /// <summary>
        /// What the current save is worth: offered entries count half, displayed ones
        /// and orbs whole. This is the number the player is shown.
        /// </summary>
        public double Score => DialogueScore.Total(_offered.Count, _displayed.Count, _orbs.Count);

        /// <summary>
        /// True when the current save has neither an entry above Untouched nor an orb.
        /// </summary>
        public bool IsEmpty => Count == 0 && _orbs.Count == 0;

        /// <summary>Records one entry's status in the current save.</summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="dialogueEntryId">The dialogue entry's integer ID.</param>
        /// <param name="status">
        /// The status the game just wrote. <see cref="SimStatus.Untouched"/> removes the
        /// entry; <see cref="SimStatus.WasOffered"/> over an already displayed entry is
        /// left alone.
        /// </param>
        /// <returns>
        /// <c>true</c> if <see cref="Score"/> changed. Re-marking an entry at a status it
        /// already holds or has passed, or clearing one that was not counted, returns
        /// <c>false</c>.
        /// </returns>
        /// <exception cref="ArgumentOutOfRangeException">
        /// <paramref name="status"/> is not a defined <see cref="SimStatus"/> value.
        /// </exception>
        public bool Set(int conversationId, int dialogueEntryId, SimStatus status)
        {
            long key = Key(conversationId, dialogueEntryId);
            switch (status)
            {
                case SimStatus.Untouched:
                    // Non-short-circuiting on purpose: an entry is only ever in one of
                    // the two sets, but which one is not worth a lookup to find out.
                    return _offered.Remove(key) | _displayed.Remove(key);
                case SimStatus.WasOffered:
                    return !_displayed.Contains(key) && _offered.Add(key);
                case SimStatus.WasDisplayed:
                    _offered.Remove(key);
                    return _displayed.Add(key);
                default:
                    throw new ArgumentOutOfRangeException(
                        nameof(status),
                        status,
                        "Not a defined SimStatus value.");
            }
        }

        /// <summary>
        /// Records one entry's status supplied as one of the game's status strings,
        /// without throwing on an unrecognized value.
        /// </summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="dialogueEntryId">The dialogue entry's integer ID.</param>
        /// <param name="statusName">The incoming status string from the game.</param>
        /// <param name="changed">
        /// Set to <c>true</c> if <see cref="Score"/> changed. Always <c>false</c> when
        /// the return value is <c>false</c>.
        /// </param>
        /// <returns>
        /// <c>true</c> if <paramref name="statusName"/> was recognized. An unrecognized
        /// one leaves the tally untouched, so a status the mod does not understand
        /// cannot silently drop an entry out of the count.
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

        /// <summary>Records that an orb has been opened in the current save.</summary>
        /// <param name="conversationTitle">
        /// The <c>conversation</c> string on the <c>SenseOrb</c>, which is the key the
        /// game writes into <c>ShownOrbs</c>. A title rather than a conversation ID
        /// because it is what both orb sources hand over without a database lookup: the
        /// component at click time, and the save's <c>states.lua</c> at load time.
        /// </param>
        /// <returns>
        /// <c>true</c> if <see cref="Score"/> changed - that is, if this orb was not
        /// already counted.
        /// </returns>
        /// <exception cref="ArgumentException">
        /// <paramref name="conversationTitle"/> is null or empty. The game never writes
        /// an empty key: <c>SenseOrb.SetShown</c> guards on
        /// <c>conversation.Length != 0</c>, which is why a thought orb - instantiated
        /// from the template with no conversation - never reaches <c>ShownOrbs</c>. An
        /// empty title means the caller has lost the key.
        /// </exception>
        public bool SetOrb(string conversationTitle)
        {
            if (string.IsNullOrEmpty(conversationTitle))
            {
                throw new ArgumentException(
                    "An orb's conversation title must not be null or empty.",
                    nameof(conversationTitle));
            }

            return _orbs.Add(conversationTitle);
        }

        /// <summary>Whether this orb counts towards <see cref="Score"/> in the current save.</summary>
        public bool ContainsOrb(string conversationTitle) =>
            conversationTitle != null && _orbs.Contains(conversationTitle);

        /// <summary>
        /// Whether this entry counts towards <see cref="Score"/> in the current save,
        /// at either status.
        /// </summary>
        public bool Contains(int conversationId, int dialogueEntryId)
        {
            long key = Key(conversationId, dialogueEntryId);
            return _displayed.Contains(key) || _offered.Contains(key);
        }

        /// <summary>
        /// The status this entry holds in the current save, or
        /// <see cref="SimStatus.Untouched"/> if it is not counted.
        /// </summary>
        public SimStatus GetStatus(int conversationId, int dialogueEntryId)
        {
            long key = Key(conversationId, dialogueEntryId);
            if (_displayed.Contains(key))
            {
                return SimStatus.WasDisplayed;
            }

            return _offered.Contains(key) ? SimStatus.WasOffered : SimStatus.Untouched;
        }

        /// <summary>Empties entries and orbs both, for a new game.</summary>
        /// <returns>How many entries and orbs together were dropped.</returns>
        public int Clear() => ClearEntries() + ClearOrbs();

        /// <summary>Empties the dialogue entries, leaving the orbs alone.</summary>
        /// <remarks>
        /// The savegame-load case. A load hands the mod the save's whole SimStatus table
        /// at once, so entries are replaced wholesale; orbs live in the save's
        /// <c>states.lua</c> instead and are not in those bytes, so clearing them here
        /// would drop them with nothing to refill them.
        /// </remarks>
        /// <returns>How many entries were dropped.</returns>
        public int ClearEntries()
        {
            int dropped = Count;
            _offered.Clear();
            _displayed.Clear();
            return dropped;
        }

        /// <summary>
        /// Empties the orbs, leaving the dialogue entries alone, for an orb resync about
        /// to refill them.
        /// </summary>
        /// <returns>How many orbs were dropped.</returns>
        public int ClearOrbs()
        {
            int dropped = _orbs.Count;
            _orbs.Clear();
            return dropped;
        }

        /// <inheritdoc />
        public override string ToString() =>
            $"CurrentSaveTally({DialogueScore.Format(Score)} from {DisplayedCount} displayed, "
            + $"{OfferedCount} offered and {OrbCount} orbs)";

        /// <summary>
        /// Packs the two IDs into one key: conversation in the high 32 bits, entry in
        /// the low 32.
        /// </summary>
        private static long Key(int conversationId, int dialogueEntryId) =>
            ((long)conversationId << 32) | (uint)dialogueEntryId;
    }
}
