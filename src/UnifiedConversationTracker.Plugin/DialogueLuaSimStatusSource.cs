using System;
using System.Collections.Generic;
using Il2CppInterop.Runtime;
using PixelCrushers.DialogueSystem;
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
    /// <c>SimStatus</c> - which is five or six IL2CPP interop crossings per row.
    /// Over the ~113,000 rows of a real playthrough that measured 649 ms. Everything
    /// above the entry is invariant, so it is fetched once for the whole walk and
    /// once per conversation instead, leaving two lookups per row.</para>
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

        /// <inheritdoc />
        public string Description => "Dialogue System master database";

        /// <inheritdoc />
        public bool IsReady => ResolveDatabase() != null && ResolveConversationTable() != null;

        /// <inheritdoc />
        public IEnumerable<SimStatusRow> EnumerateSimStatuses()
        {
            DialogueDatabase database = ResolveDatabase()
                ?? throw new InvalidOperationException(
                    "The Dialogue System master database is not available; the walk should have been skipped.");

            LuaTable conversationTable = ResolveConversationTable()
                ?? throw new InvalidOperationException(
                    $"The Lua '{ConversationTableName}' table is not available; "
                    + "the walk should have been skipped.");

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

                for (int j = 0; j < entries.Count; j++)
                {
                    DialogueEntry entry = entries[j];
                    if (entry == null)
                    {
                        continue;
                    }

                    int conversationId = entry.conversationID;
                    if (!haveDialogTable || conversationId != dialogTableConversationId)
                    {
                        dialogTable = ResolveDialogTable(conversationTable, conversationId);
                        dialogTableConversationId = conversationId;
                        haveDialogTable = true;
                    }

                    yield return new SimStatusRow(
                        conversationId,
                        entry.id,
                        simStatusIsLive
                            ? ReadSimStatus(dialogTable, entry.id)
                            : SimStatusNames.Untouched);
                }
            }
        }

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

            LuaValue? status = entryFields.GetValue(SimStatusFieldName);
            LuaString? text = status?.TryCast<LuaString>();

            // Pass the game's string through as-is, including one that is not one of
            // the three known names: the session counts and reports those, and
            // silently turning one into Untouched here would hide it.
            return text == null ? SimStatusNames.Untouched : text.Text;
        }

        /// <summary>
        /// A Lua value as a table, or null if it is nil or something else. Folds the
        /// nil-versus-null distinction away: <c>GetValue</c> returns
        /// <c>LuaNil.Nil</c> for an absent key rather than a null reference.
        /// </summary>
        private static LuaTable? AsTable(LuaValue? value) => value?.TryCast<LuaTable>();

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
