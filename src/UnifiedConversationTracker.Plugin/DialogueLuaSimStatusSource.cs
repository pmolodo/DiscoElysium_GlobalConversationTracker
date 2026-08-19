using System;
using System.Collections.Generic;
using System.Diagnostics;
using Il2CppInterop.Runtime;
using PixelCrushers.DialogueSystem;
using UnifiedConversationTracker.Core;
using UnifiedConversationTracker.Session;
using Language.Lua;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// Reads the running game's SimStatus values out of the Dialogue System, for the
    /// resync that follows every savegame load.
    /// </summary>
    /// <remarks>
    /// <para><b>Which IDs to visit, and where to read them from.</b> The dialogue
    /// database supplies the population - every conversation and every entry the
    /// game knows about - and the Lua <c>Conversation</c> table supplies the values.
    /// The database is used for the IDs rather than iterating the Lua table because
    /// a <c>LuaTable</c>'s integer keys land in either its array part or its hash
    /// part depending on insertion order (entry 0 goes to the hash part, 1..N to the
    /// array part), so iterating one means handling both halves; asking for a known
    /// key with <c>LuaTable.GetValue(int)</c> handles that split already.</para>
    ///
    /// <para><b>Why not DialogueLua.GetSimStatus</b> (de-omm.23). It is the game's
    /// own accessor and it is correct, but it restarts the whole lookup chain from
    /// the Lua global root on every single row - <c>Environment["Conversation"]</c>,
    /// then the conversation, then <c>Dialog</c>, then the entry, then
    /// <c>SimStatus</c> - which is five or six lookups per row. Over the ~113,000
    /// rows of a real playthrough that measured 649 ms. Everything above the entry
    /// is invariant, so it is fetched once for the whole walk and once per
    /// conversation instead, leaving two lookups per row. de-omm.23 took those five
    /// or six to be IL2CPP interop crossings, and expected the hoist to be cheaper
    /// on that basis; the count is now disputed and has never been measured either
    /// way, so read it as what was believed at the time rather than as a result.</para>
    ///
    /// <para><b>Whether the hoist actually helped is an open question</b> (de-p1h.1).
    /// The first in-game measurement of this walk was 1744 ms over the same 112,940
    /// rows - 15.4 us a row against the 5.7 us the old path's 649 ms works out to.
    /// de-p1h.1 established the two figures are comparable: same method, same rows,
    /// same thread, same real game, both excluding the file write. It could not
    /// establish why they differ. Its reading of the source - analysis only, nothing
    /// was run - is that the five or six are Lua table lookups rather than interop
    /// crossings, and that the two are not the same cost. On that reading
    /// <c>GetSimStatus</c> would be <em>one</em> managed-to-IL2CPP call whose lookups
    /// then run inside the game's own compiled code, while this walk pays roughly
    /// three <c>il2cpp_runtime_invoke</c> calls, about four interop wrapper
    /// allocations, and a fresh <c>Il2CppString</c> for the <c>"SimStatus"</c> key on
    /// every row - which would make the hoist a pessimization rather than an
    /// improvement. It is not the only candidate: the 649 ms walk ran mid-conversation
    /// on an otherwise idle main thread, and the 1744 ms one on the first savegame
    /// load of a cold process, competing with the load and paying first-call JIT and
    /// interop warmup. Both are n=1. Nothing rules either out.</para>
    ///
    /// <para><b>What would settle it.</b> <see cref="DescribeLastWalk"/> splits the
    /// walk into the database scan, the per-conversation Dialog resolves and the
    /// per-row status read, so an in-game load says where the time actually went. If
    /// de-p1h.1's reading holds, the per-row read dominates and the resolves are
    /// negligible; if the scan dominates instead, the cost is in the database
    /// traversal and neither lookup chain is the subject. What the line cannot show
    /// is that the hoist made things worse, because it only times the path that is
    /// here - an A/B needs a second walk calling <c>GetSimStatus</c>, timed the same
    /// way, and there is not one. Until
    /// then this is not reverted, because the reason it replaced <c>GetSimStatus</c>
    /// was never really the speed (see the next paragraph), and because the walk now
    /// runs inside the savegame loading screen rather than at the moment a
    /// conversation opens. The cache-miss count in that line also settles the
    /// assumption this walk rests on - that entries arrive grouped by conversation,
    /// so the Dialog table is resolved once per conversation and not once per row.
    /// The shipped database agrees: 112,962 entries across 1,501 conversations, every
    /// one of them carrying its parent conversation's id, so the expected miss
    /// count is 1,501.</para>
    ///
    /// <para><b>It also stops mutating the game.</b> <c>GetSimStatus</c> goes through
    /// <c>GetSimStatusTable</c>, which does not report a miss - it CREATES the
    /// missing table, writes <c>SimStatus = "Untouched"</c> into it and adds it to
    /// the parent, and will rebuild a whole missing conversation's Dialog table.
    /// Reading the table directly reports the same value for a miss (Untouched, the
    /// game's own default) and writes nothing, so the walk is a genuine read of
    /// shared Lua state rather than a write to it.</para>
    ///
    /// <para><b>Timing.</b> <c>PersistentDataManager</c> rebuilds the whole SimStatus
    /// table when a savegame loads, without going through <c>MarkDialogueEntry</c>
    /// (de-0s5). Reading before that has happened returns the pre-load table.
    /// <see cref="IsReady"/> only reports that the dialogue system is up and the
    /// <c>Conversation</c> table exists, which is necessary but not sufficient; the
    /// sufficient part is the caller's trigger point, a postfix on
    /// <c>PersistentDataManager.ExpandCompressedSimStatusData</c>, which is the very
    /// method that does the rewriting.</para>
    /// </remarks>
    internal sealed class DialogueLuaSimStatusSource : ISimStatusSource
    {
        /// <summary>The Lua global holding the per-conversation tables.</summary>
        private const string ConversationTableName = "Conversation";

        /// <summary>The field of a conversation table holding its entries.</summary>
        private const string DialogTableName = "Dialog";

        /// <summary>The field of an entry table holding its status string.</summary>
        private const string SimStatusFieldName = "SimStatus";

        private DialogueDatabase? _database;

        /// <summary>
        /// Where the last walk's time went. Written only by the walk, read only by
        /// <see cref="DescribeLastWalk"/> after it, both on the main thread.
        /// </summary>
        private SimStatusWalkMeasurement _lastWalk;

        /// <inheritdoc />
        public string Description => "Dialogue System master database";

        /// <inheritdoc />
        public bool IsReady => ResolveDatabase() != null && ResolveConversationTable() != null;

        /// <inheritdoc />
        /// <remarks>
        /// <para><b>The timestamps</b> (de-p1h). Three <see cref="Stopwatch.GetTimestamp"/>
        /// reads a row, four on the rows where the Dialog table has to be resolved:
        /// one splitting the database scan from the Lua status read, one ending the
        /// read, one after the <c>yield return</c> so the caller's merge is charged
        /// to neither section, and the extra one closing a resolve. On Windows that
        /// is a <c>QueryPerformanceCounter</c> apiece, tens of nanoseconds; over
        /// 113,000 rows it is single-digit milliseconds against a walk measured in
        /// seconds, which is small enough not to move the number it is measuring -
        /// and the count of reads is reported next to the times, so that stays a
        /// judgement a reader can make rather than one they have to accept.</para>
        /// </remarks>
        public IEnumerable<SimStatusRow> EnumerateSimStatuses()
        {
            DialogueDatabase database = ResolveDatabase()
                ?? throw new InvalidOperationException(
                    "The Dialogue System master database is not available; the walk should have been skipped.");

            LuaTable conversationTable = ResolveConversationTable()
                ?? throw new InvalidOperationException(
                    $"The Lua '{ConversationTableName}' table is not available; "
                    + "the walk should have been skipped.");

            // Reset before anything is counted, so a walk that throws or is abandoned
            // still describes itself rather than repeating the previous walk's line.
            _lastWalk = SimStatusWalkMeasurement.Starting();

            // Hoisted out of the row loop: this is the first two of the five lookups
            // GetSimStatus would redo per row.
            Il2CppSystem.Collections.Generic.List<Conversation> conversations = database.conversations;
            if (conversations == null)
            {
                yield break;
            }

            // The game's own master switch. When it is off, GetSimStatus reports
            // Untouched for everything regardless of what the table holds, and
            // MarkDialogueEntry writes nothing; reading the table anyway would invent
            // history the game does not consider real.
            bool simStatusIsLive = DialogueLua.includeSimStatus;

            // The Dialog table of the conversation the previous row belonged to.
            // Entries within one conversation share a conversationID, so this is one
            // lookup per conversation rather than per row - but it is keyed on the
            // entry's own conversationID rather than the enclosing conversation's id,
            // so an entry that disagrees with its parent still reads the table
            // GetSimStatus would have read.
            LuaTable? dialogTable = null;
            int dialogTableConversationId = 0;
            bool haveDialogTable = false;

            long rowCount = 0;
            long conversationCount = 0;
            long resolveCount = 0;
            long scanTicks = 0;
            long resolveTicks = 0;
            long readTicks = 0;
            long sectionStart = Stopwatch.GetTimestamp();

            try
            {
                for (int i = 0; i < conversations.Count; i++)
                {
                    Conversation conversation = conversations[i];
                    if (conversation == null)
                    {
                        continue;
                    }

                    Il2CppSystem.Collections.Generic.List<DialogueEntry> entries = conversation.dialogueEntries;
                    if (entries == null)
                    {
                        continue;
                    }

                    conversationCount++;

                    for (int j = 0; j < entries.Count; j++)
                    {
                        DialogueEntry entry = entries[j];
                        if (entry == null)
                        {
                            continue;
                        }

                        int conversationId = entry.conversationID;
                        int entryId = entry.id;

                        long scanned = Stopwatch.GetTimestamp();
                        scanTicks += scanned - sectionStart;

                        if (!haveDialogTable || conversationId != dialogTableConversationId)
                        {
                            dialogTable = ResolveDialogTable(conversationTable, conversationId);
                            dialogTableConversationId = conversationId;
                            haveDialogTable = true;

                            resolveCount++;
                            long resolved = Stopwatch.GetTimestamp();
                            resolveTicks += resolved - scanned;
                            scanned = resolved;
                        }

                        string statusName = simStatusIsLive
                            ? ReadSimStatus(dialogTable, entryId)
                            : SimStatusNames.Untouched;

                        readTicks += Stopwatch.GetTimestamp() - scanned;
                        rowCount++;

                        yield return new SimStatusRow(conversationId, entryId, statusName);

                        // After the yield, so the caller's merge is charged to neither
                        // the read it follows nor the scan it precedes.
                        sectionStart = Stopwatch.GetTimestamp();
                    }
                }
            }
            finally
            {
                _lastWalk = new SimStatusWalkMeasurement(
                    rowCount,
                    conversationCount,
                    resolveCount,
                    scanTicks,
                    resolveTicks,
                    readTicks);
            }
        }

        /// <inheritdoc />
        public string? DescribeLastWalk() => _lastWalk.Describe();

        /// <summary>
        /// The <c>Dialog</c> table of one conversation, or null if the Lua table has
        /// no such conversation or no Dialog field for it.
        /// </summary>
        /// <remarks>
        /// Deliberately does NOT create what is missing, which is where this parts
        /// company with <c>DialogueLua.GetSimStatusTable</c>. A conversation the Lua
        /// table has never heard of has no statuses to report, and reporting that as
        /// all-Untouched is both the right answer and what GetSimStatus would end up
        /// returning once it had finished building the table.
        /// </remarks>
        private static LuaTable? ResolveDialogTable(LuaTable conversationTable, int conversationId)
        {
            LuaTable? conversation = AsTable(conversationTable.GetValue(conversationId));
            return conversation == null ? null : AsTable(conversation.GetValue(DialogTableName));
        }

        /// <summary>
        /// One entry's status string. Untouched - the game's own default for an entry
        /// with no record - whenever the entry, its table or its SimStatus field is
        /// absent, which is exactly what <c>DialogueLua.GetSimStatus</c> reports.
        /// </summary>
        private static string ReadSimStatus(LuaTable? dialogTable, int entryId)
        {
            if (dialogTable == null)
            {
                return SimStatusNames.Untouched;
            }

            LuaTable? entryFields = AsTable(dialogTable.GetValue(entryId));
            if (entryFields == null)
            {
                return SimStatusNames.Untouched;
            }

            // Pass the game's string through as-is, including one that is not one of
            // the three known names: the session counts and reports those, and
            // silently turning one into Untouched here would hide it.
            return LuaValues.AsText(entryFields.GetValue(SimStatusFieldName))
                ?? SimStatusNames.Untouched;
        }

        /// <summary>Shorthand for the shared nil-folding cast.</summary>
        private static LuaTable? AsTable(LuaValue? value) => LuaValues.AsTable(value);

        /// <summary>
        /// The master database, or null while the dialogue system is not up.
        /// </summary>
        /// <remarks>
        /// Cached once found, because <c>DialogueManager.masterDatabase</c> falls
        /// back to <c>FindObjectOfType</c> whenever its own static is null, and this
        /// is polled before every walk.
        /// </remarks>
        private DialogueDatabase? ResolveDatabase()
        {
            if (_database != null)
            {
                return _database;
            }

            DialogueDatabase database = DialogueManager.masterDatabase;
            if (database == null)
            {
                return null;
            }

            Il2CppSystem.Collections.Generic.List<Conversation> conversations = database.conversations;
            if (conversations == null || conversations.Count == 0)
            {
                // Present but not populated yet; do not cache it.
                return null;
            }

            _database = database;
            return _database;
        }

        /// <summary>
        /// The Lua <c>Conversation</c> table, or null while the Lua environment does
        /// not hold one. Never cached: the environment is rebuilt across loads, and
        /// holding a stale table would read a table the game has stopped using.
        /// </summary>
        private static LuaTable? ResolveConversationTable()
        {
            LuaTable environment = Lua.Environment;
            return environment == null ? null : AsTable(environment.GetValue(ConversationTableName));
        }
    }
}
