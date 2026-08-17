using System;
using System.Diagnostics;
using System.Globalization;
using PixelCrushers.DialogueSystem;
using UnifiedConversationTracker.Session;
using Language.Lua;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// Reads the SimStatus a savegame is carrying out of the compressed
    /// <c>Conversation_SimX_</c> strings it arrived in, at the one moment they exist:
    /// the prefix of the method that expands and then destroys them.
    /// </summary>
    /// <remarks>
    /// <para><b>What it reads, and why nothing else can.</b> Disco Elysium's savegames
    /// carry SimStatus as one Lua string per conversation, at
    /// <c>Variable["Conversation_SimX_&lt;articyId&gt;"]</c>, holding
    /// <c>entryArticyId;statusChar;...</c> for every dialogue entry.
    /// <c>PersistentDataManager.ExpandCompressedSimStatusData</c> expands those into
    /// the per-entry Lua tables the walk reads - and, in the same method, appends
    /// <c>Variable["Conversation_SimX_..."]=nil;</c> for every conversation and runs
    /// the whole batch through <c>Lua.Run</c> BEFORE returning (de-0m0.19, confirmed
    /// against the Final Cut ISIL at instructions 211-222 and 456). So a postfix sees
    /// nothing, and no intermediate bulk form survives either. A prefix on the same
    /// method sees all of it.</para>
    ///
    /// <para><b>Keyed lookups, not a scan</b>, and the reason is not only the obvious
    /// one. The articy id map supplies every conversation's variable name, so this
    /// asks the Lua <c>Variable</c> table ~1,500 specific questions instead of
    /// enumerating its ~12,000 entries across the interop boundary, which de-0m0.19
    /// estimated would add 100-200 ms. The subtler reason is
    /// <c>Language.Lua.LuaTable.GetKey(string)</c>: a HIT is a dictionary lookup in
    /// the table's own string-key cache, but a MISS falls through to a linear scan of
    /// every key in the table calling <c>ToString()</c> on each. One miss is cheap
    /// and a few thousand would not be, which is what
    /// <see cref="MaxMissingBlobs"/> bounds. (Read from the decompiled LuaTable.cs;
    /// no part of this has been timed in a running game.)</para>
    ///
    /// <para><b>It executes no game code.</b> Two Lua table reads and then ~1,500 more,
    /// all of them plain value reads of state the save has already populated. Nothing
    /// is called that the game does not call itself, nothing is written, and the
    /// expansion that follows is completely unmodified - which is what makes running
    /// this inside the player's load safe in a way that regenerating the same data
    /// through the Dialogue System's own serializer would not be.</para>
    ///
    /// <para><b>Its cost in a running game is unmeasured.</b> No prefix had ever
    /// executed in one when this was written, and the Lua reads below have therefore
    /// never happened at all. What IS measured, and only offline, is the decode that
    /// follows them: 35-53 ms over a real save's blobs on a desktop CPU, where it also
    /// reproduced that save's own expanded SimStatus exactly (see
    /// <see cref="CompressedSimStatusDecoder"/>). de-0m0.19 reasoned order 20-50 ms for
    /// read and decode together, against the walk's measured 1,892 ms; the decode
    /// figure alone already exceeds the lower end of that. Treat the total as unknown
    /// until a load has been timed. That uncertainty is exactly why
    /// <see cref="Intercept"/> refuses rather than guesses.</para>
    /// </remarks>
    internal sealed class CompressedSimStatusInterceptor : ISimStatusInterceptor
    {
        /// <summary>The Lua global holding the save's variables.</summary>
        private const string VariableTableName = "Variable";

        /// <summary>
        /// What the Dialogue System treats as an absent blob as well as an empty one.
        /// Its own reader compares the string it gets back against this
        /// (PersistentDataManager.cs:785), because it fetches the value through
        /// <c>Lua.Run(...).AsString</c>, where a nil arrives as its own name.
        /// </summary>
        private const string LuaNilText = "nil";

        /// <summary>
        /// How many conversations may have no blob before the whole interception is
        /// refused.
        /// </summary>
        /// <remarks>
        /// A conversation with no dialogue entries gets no blob, and a real Final Cut
        /// save had exactly 7 of those out of 1,501 conversations (de-0m0.19) - so a
        /// handful of misses is the normal, correct case and must not be treated as an
        /// error. Many misses mean something else: a map built from a different
        /// database, or a save written on the Dialogue System's other, integer-keyed
        /// branch, where none of these variables exist at all. This cap is what stops
        /// that case both merging nonsense and paying LuaTable's linear-scan miss
        /// path ~1,500 times over. Nine times the only observed value, because the
        /// only cost of being generous is a slower fallback.
        /// </remarks>
        private const int MaxMissingBlobs = 64;

        /// <summary>
        /// How much of the dialogue database the decoded pairs have to account for
        /// before the result is trusted.
        /// </summary>
        /// <remarks>
        /// The blob is complete, not a delta, so a healthy interception decodes very
        /// nearly one pair per dialogue entry in the database: a real save decoded
        /// 112,940 pairs against the map's 112,962 entries, or 99.98%. This is the
        /// "does the row count look right" guard the design asks for, set far enough
        /// below that to survive a save that legitimately covers slightly fewer
        /// conversations, and far enough above zero to catch a read that quietly
        /// returned almost nothing.
        /// </remarks>
        private const double MinimumPairFraction = 0.90;

        private readonly ArticyIdMap _map;
        private readonly CompressedSimStatusDecoder _decoder;

        /// <summary>Creates an interceptor over one articy id map.</summary>
        /// <param name="map">The map, already loaded, off the load path.</param>
        /// <exception cref="ArgumentNullException"><paramref name="map"/> is null.</exception>
        internal CompressedSimStatusInterceptor(ArticyIdMap map)
        {
            _map = map ?? throw new ArgumentNullException(nameof(map));
            _decoder = new CompressedSimStatusDecoder(map);
        }

        /// <inheritdoc />
        public string Description => "savegame compressed SimStatus blobs";

        /// <inheritdoc />
        public SimStatusInterception Intercept()
        {
            string? wrongBranch = DescribeWrongBranch();
            if (wrongBranch != null)
            {
                return SimStatusInterception.Unavailable(wrongBranch);
            }

            LuaTable? variables = ResolveVariableTable();
            if (variables == null)
            {
                return SimStatusInterception.Unavailable(
                    $"the Lua '{VariableTableName}' table is not available, so there is nothing "
                    + "to read the compressed blobs out of.");
            }

            _decoder.Reset();

            long missingBlobs = 0;
            long readTicks = 0;
            long decodeTicks = 0;
            long sectionStart = Stopwatch.GetTimestamp();

            foreach (ArticyConversation conversation in _map.Conversations)
            {
                string? blob = ReadBlob(variables, conversation.VariableName);

                long read = Stopwatch.GetTimestamp();
                readTicks += read - sectionStart;

                if (blob == null)
                {
                    missingBlobs++;
                    if (missingBlobs > MaxMissingBlobs)
                    {
                        return SimStatusInterception.Unavailable(
                            $"more than {MaxMissingBlobs} of the map's {_map.Conversations.Count} "
                            + "conversations had no compressed blob, which means this save was not "
                            + "written the way the map describes rather than that a few "
                            + "conversations are empty.");
                    }
                }
                else
                {
                    _decoder.Decode(conversation.ConversationId, blob);
                }

                sectionStart = Stopwatch.GetTimestamp();
                decodeTicks += sectionStart - read;
            }

            string? refusal = DescribeUntrustworthyResult();
            if (refusal != null)
            {
                return SimStatusInterception.Unavailable(refusal);
            }

            return SimStatusInterception.Succeeded(
                _decoder.Rows,
                new SimStatusInterceptionMeasurement(
                    conversationCount: _map.Conversations.Count,
                    blobCount: _decoder.BlobCount,
                    pairCount: _decoder.PairCount,
                    shadowedPairCount: _decoder.ShadowedPairCount,
                    rowCount: _decoder.Rows.Count,
                    readTicks: readTicks,
                    decodeTicks: decodeTicks));
        }

        /// <summary>
        /// Says why this save cannot be on the articy-keyed branch, or null if it can.
        /// </summary>
        /// <remarks>
        /// <para>The Dialogue System has two encodings for the compressed form, chosen
        /// by <c>useConversationID</c> and <c>useEntryID</c>, which are derived from
        /// these two fields being empty (PersistentDataManager.cs:707-708). Only the
        /// non-empty case produces the articy-keyed
        /// <c>Variable["Conversation_SimX_..."]</c> strings this reads; the other
        /// writes <c>Conversation[id].SimX</c> with plain integer entry ids, which this
        /// map cannot translate.</para>
        ///
        /// <para>The two fields cannot be read from the postfix's side of the method,
        /// because <c>useConversationID</c> and <c>useEntryID</c> are assigned INSIDE
        /// it and hold the previous call's values at prefix time. These are the
        /// underlying statics they are derived from, so reading them recomputes the
        /// answer rather than trusting a stale one.</para>
        ///
        /// <para>Real Final Cut save data shows both non-empty, so this gate is
        /// expected to pass; it exists so that a build where it does not falls back
        /// with an explanation instead of resolving nothing.</para>
        ///
        /// <para>de-0m0.19 left it open whether these two are reachable BY NAME from
        /// the BepInEx interop assembly, since answering it needed a compile it was
        /// scoped out of. They are: this compiles against
        /// <c>BepInEx\interop\DialogueSystem.dll</c>, so the hardcoded-field-name
        /// fallback it proposed is not needed.</para>
        /// </remarks>
        private static string? DescribeWrongBranch()
        {
            if (string.IsNullOrEmpty(PersistentDataManager.saveConversationSimStatusWithField)
                || string.IsNullOrEmpty(PersistentDataManager.saveDialogueEntrySimStatusWithField))
            {
                return "the Dialogue System is configured to save SimStatus by integer id "
                    + "(saveConversationSimStatusWithField or saveDialogueEntrySimStatusWithField is "
                    + "empty), so this save holds no articy-keyed compressed blobs to intercept.";
            }

            return null;
        }

        /// <summary>
        /// Says why the decoded result must not be merged, or null if it may be.
        /// </summary>
        /// <remarks>
        /// Deliberately all-or-nothing and deliberately strict. Every one of these
        /// means the map and the save disagree about what the ids mean, and a merge
        /// that only ever raises cannot be undone - so a wrong row would be permanent,
        /// while a refusal costs one walk of the master database.
        /// </remarks>
        private string? DescribeUntrustworthyResult()
        {
            if (_decoder.BlobCount == 0)
            {
                return "not one of the map's conversations had a compressed blob.";
            }

            if (_decoder.MalformedPairCount > 0)
            {
                return $"{_decoder.MalformedPairCount} of the {_decoder.PairCount} decoded pairs were "
                    + "malformed, so this is not the format the decoder expects.";
            }

            if (_decoder.UnresolvedPairCount > 0)
            {
                return $"{_decoder.UnresolvedPairCount} of the {_decoder.PairCount} decoded pairs used "
                    + "an articy id the map does not know, so the map does not describe this save's "
                    + "dialogue database.";
            }

            if (_decoder.ForeignConversationPairCount > 0)
            {
                return $"{_decoder.ForeignConversationPairCount} of the {_decoder.PairCount} decoded "
                    + "pairs used an articy id the map places in a different conversation, so the map "
                    + "does not describe this save's dialogue database.";
            }

            long required = (long)(_map.DialogueEntryCount * MinimumPairFraction);
            if (_decoder.PairCount < required)
            {
                return $"only {_decoder.PairCount} pairs were decoded for a database of "
                    + $"{_map.DialogueEntryCount} dialogue entries, below the {required} that a "
                    + "complete blob set would have to reach.";
            }

            return null;
        }

        /// <summary>
        /// One conversation's compressed blob, or null when there is none to read.
        /// </summary>
        /// <remarks>
        /// Reads the value straight out of the <c>Variable</c> table rather than
        /// through <c>Lua.Run("return Variable[...]")</c> the way the game does: that
        /// call compiles and executes a Lua chunk per conversation, and the value it
        /// would return is the one already sitting in the table.
        /// </remarks>
        private static string? ReadBlob(LuaTable variables, string variableName)
        {
            string? blob = LuaValues.AsText(variables.GetValue(variableName));
            return string.IsNullOrEmpty(blob)
                || string.Equals(blob, LuaNilText, StringComparison.Ordinal)
                    ? null
                    : blob;
        }

        /// <summary>
        /// The Lua <c>Variable</c> table, or null while the Lua environment does not
        /// hold one. Never cached, for the same reason the walk never caches the
        /// <c>Conversation</c> table: the environment is rebuilt across loads.
        /// </summary>
        private static LuaTable? ResolveVariableTable()
        {
            LuaTable environment = Lua.Environment;
            return environment == null ? null : LuaValues.AsTable(environment.GetValue(VariableTableName));
        }

        /// <inheritdoc />
        public override string ToString() =>
            string.Format(
                CultureInfo.InvariantCulture,
                "CompressedSimStatusInterceptor({0})",
                _map);
    }
}
