using System;
using System.Collections.Generic;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// Turns a savegame's compressed SimStatus blobs - one Lua string per
    /// conversation - into the rows the unified state merges, reproducing what the
    /// game's own expansion would have written.
    /// </summary>
    /// <remarks>
    /// <para><b>The format</b> (de-0m0.19, confirmed against real Final Cut save data
    /// and against the decompiled writer). One string per conversation, held at
    /// <c>Variable["Conversation_SimX_&lt;articyId&gt;"]</c>, containing
    /// <c>entryArticyId;statusChar;entryArticyId;statusChar;...</c> with no trailing
    /// separator and one pair per dialogue entry. The status characters are
    /// <c>u</c>, <c>d</c> and <c>o</c> for Untouched, WasDisplayed and WasOffered
    /// (<c>PersistentDataManager.CharToSimStatus</c>,
    /// pre-final-cut-assetripper-export/.../PersistentDataManager.cs:850-859).</para>
    ///
    /// <para><b>Absence means Untouched, definitively.</b> The blob is complete rather
    /// than a delta: the game writes a pair for every dialogue entry, Untouched ones
    /// included, and any entry the blob does not mention is filled in as Untouched by
    /// the expansion itself (PersistentDataManager.cs:827-836). Measured on a real
    /// save, 1494 blobs encoded exactly 112,940 pairs - byte for byte the row count a
    /// walk of the master database visits - of which only 1,473 were not Untouched.
    /// So the unified state's monotonic merge needs no change in meaning, and this
    /// emits only the non-Untouched rows: merging an Untouched row stores nothing, and
    /// not emitting it is where the speedup lives.</para>
    ///
    /// <para><b>Last-wins, on purpose</b> (de-0m0.19). Articy ids are not unique
    /// within a conversation, and a blob therefore repeats one once per entry that
    /// carries it. The game resolves each occurrence through a
    /// <c>Dictionary&lt;string, int&gt;</c> that itself kept only the last entry with
    /// that id, and then writes the status with <c>LuaTable.AddRaw(int, ...)</c>,
    /// which overwrites rather than appends - so the LAST occurrence in the blob is
    /// the status that survives, and the entries shadowed by it fall through to
    /// Untouched. This replicates that exactly. It means the game itself drops
    /// SimStatus for duplicate-shadowed entries across a save/load, and it means a
    /// walk of the master database after a load already sees only that lossy result:
    /// reproducing it is what makes interception agree with the walk, so it is
    /// deliberately not "fixed" here.</para>
    ///
    /// <para><b>Counting is the sanity check.</b> Everything this cannot make sense of
    /// - a malformed pair, an articy id the map does not know, an id the map places in
    /// another conversation - is counted rather than guessed at, so the caller can
    /// refuse the whole result and fall back to walking the master database. Nothing
    /// here throws on bad data: a blob that stops making sense half way through still
    /// reports what it managed and what it could not.</para>
    ///
    /// <para><b>What this costs, MEASURED - but measured offline, not in the game.</b>
    /// Run over a real Final Cut savegame's 1,494 blobs (112,940 pairs, 2.37 MB of
    /// characters) on a desktop CPU, outside Unity and outside IL2CPP, this class takes
    /// 35-53 ms: 53 ms on the first pass and 35 ms once warm. That is the managed half
    /// only - no interop crossings, no Lua - and it is the half that does not change
    /// when the game is doing the calling. It is also higher than de-0m0.19's reasoned
    /// 5-15 ms for this step, which is worth knowing before reading its 20-50 ms total
    /// as a prediction. Per pair it is roughly 300 ns, and it goes on one substring per
    /// articy id and one lookup apiece in a 93,000-entry string dictionary and a
    /// per-conversation integer one. The same run reproduced the save's own expanded
    /// SimStatus EXACTLY: 1,473 non-Untouched rows, zero disagreements in either
    /// direction, zero malformed, unresolved or foreign pairs.</para>
    /// </remarks>
    public sealed class CompressedSimStatusDecoder
    {
        /// <summary>The separator between every token of a blob.</summary>
        private const char PairSeparator = ';';

        /// <summary>The game's character for <see cref="SimStatus.Untouched"/>.</summary>
        private const char UntouchedChar = 'u';

        /// <summary>The game's character for <see cref="SimStatus.WasDisplayed"/>.</summary>
        private const char WasDisplayedChar = 'd';

        /// <summary>The game's character for <see cref="SimStatus.WasOffered"/>.</summary>
        private const char WasOfferedChar = 'o';

        private readonly ArticyIdMap _map;

        /// <summary>
        /// One conversation's resolved statuses, keyed by dialogue entry id so that a
        /// later pair overwrites an earlier one exactly as the game's
        /// <c>LuaTable.AddRaw</c> does. Reused across blobs rather than reallocated:
        /// there are ~1,500 blobs and only one is ever in flight.
        /// </summary>
        private readonly Dictionary<int, SimStatus> _resolved = new Dictionary<int, SimStatus>();

        private readonly List<SimStatusRow> _rows = new List<SimStatusRow>();

        /// <summary>Creates a decoder over one articy id map.</summary>
        /// <param name="map">The map that translates the blobs' articy ids.</param>
        /// <exception cref="ArgumentNullException"><paramref name="map"/> is null.</exception>
        public CompressedSimStatusDecoder(ArticyIdMap map)
        {
            _map = map ?? throw new ArgumentNullException(nameof(map));
        }

        /// <summary>Every non-Untouched row decoded since the last <see cref="Reset"/>.</summary>
        public IReadOnlyList<SimStatusRow> Rows => _rows;

        /// <summary>How many blobs have been decoded.</summary>
        public long BlobCount { get; private set; }

        /// <summary>
        /// How many id/status pairs the blobs held, Untouched ones included. This is
        /// the figure directly comparable to a walk's row count.
        /// </summary>
        public long PairCount { get; private set; }

        /// <summary>
        /// How many pairs were overwritten by a later pair addressing the same
        /// dialogue entry, which is what a duplicated articy id does.
        /// </summary>
        public long ShadowedPairCount { get; private set; }

        /// <summary>
        /// How many pairs could not be read at all: an odd trailing token, an empty
        /// status token, or a status character the game does not define.
        /// </summary>
        public long MalformedPairCount { get; private set; }

        /// <summary>How many pairs used an articy id the map does not know.</summary>
        public long UnresolvedPairCount { get; private set; }

        /// <summary>
        /// How many pairs used an articy id the map places in a different
        /// conversation from the blob that held it. The game would drop these, because
        /// it resolves ids against only the blob's own conversation's entries.
        /// </summary>
        public long ForeignConversationPairCount { get; private set; }

        /// <summary>Whether every pair of every blob so far was read and resolved.</summary>
        public bool IsClean =>
            MalformedPairCount == 0 && UnresolvedPairCount == 0 && ForeignConversationPairCount == 0;

        /// <summary>Forgets everything decoded so far, ready for another load.</summary>
        public void Reset()
        {
            _rows.Clear();
            BlobCount = 0;
            PairCount = 0;
            ShadowedPairCount = 0;
            MalformedPairCount = 0;
            UnresolvedPairCount = 0;
            ForeignConversationPairCount = 0;
        }

        /// <summary>
        /// Decodes one conversation's blob, appending its non-Untouched rows to
        /// <see cref="Rows"/>.
        /// </summary>
        /// <param name="conversationId">The conversation the blob belongs to.</param>
        /// <param name="blob">The compressed string, as the savegame stored it.</param>
        /// <exception cref="ArgumentNullException"><paramref name="blob"/> is null.</exception>
        public void Decode(int conversationId, string blob)
        {
            if (blob == null)
            {
                throw new ArgumentNullException(nameof(blob));
            }

            BlobCount++;
            _resolved.Clear();

            int position = 0;
            while (position < blob.Length)
            {
                int idEnd = blob.IndexOf(PairSeparator, position);
                if (idEnd < 0)
                {
                    // A trailing token with no status after it. The game divides the
                    // token count by two and drops the odd one; so does this, but it
                    // says so rather than passing silently.
                    MalformedPairCount++;
                    break;
                }

                int statusEnd = blob.IndexOf(PairSeparator, idEnd + 1);
                if (statusEnd < 0)
                {
                    statusEnd = blob.Length;
                }

                ReadPair(
                    conversationId,
                    blob.Substring(position, idEnd - position),
                    idEnd + 1 == statusEnd ? default(char?) : blob[idEnd + 1]);

                position = statusEnd + 1;
            }

            EmitResolved(conversationId);
        }

        private void ReadPair(int conversationId, string articyId, char? statusChar)
        {
            PairCount++;

            if (statusChar == null || !TryParseStatus(statusChar.Value, out SimStatus status))
            {
                MalformedPairCount++;
                return;
            }

            if (!_map.TryResolveEntry(articyId, out int mappedConversationId, out int entryId))
            {
                UnresolvedPairCount++;
                return;
            }

            if (mappedConversationId != conversationId)
            {
                // The game builds its articy-id lookup from the blob's own
                // conversation's entries, so an id belonging elsewhere simply would
                // not be found and the pair would be dropped.
                ForeignConversationPairCount++;
                return;
            }

            if (_resolved.ContainsKey(entryId))
            {
                ShadowedPairCount++;
            }

            _resolved[entryId] = status;
        }

        private void EmitResolved(int conversationId)
        {
            foreach (KeyValuePair<int, SimStatus> entry in _resolved)
            {
                if (entry.Value == SimStatus.Untouched)
                {
                    // The unified state stores nothing for Untouched, and absence means
                    // Untouched to every reader of it, so this row would be a no-op.
                    continue;
                }

                _rows.Add(new SimStatusRow(
                    conversationId, entry.Key, SimStatusNames.ToGameString(entry.Value)));
            }
        }

        private static bool TryParseStatus(char statusChar, out SimStatus status)
        {
            switch (statusChar)
            {
                case UntouchedChar:
                    status = SimStatus.Untouched;
                    return true;
                case WasDisplayedChar:
                    status = SimStatus.WasDisplayed;
                    return true;
                case WasOfferedChar:
                    status = SimStatus.WasOffered;
                    return true;
                default:
                    status = SimStatus.Untouched;
                    return false;
            }
        }

        /// <inheritdoc />
        public override string ToString() =>
            $"CompressedSimStatusDecoder({BlobCount} blobs, {PairCount} pairs, {_rows.Count} rows, "
            + $"clean={IsClean})";
    }
}
