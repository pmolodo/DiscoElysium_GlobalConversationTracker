using System;
using System.Collections.Generic;
using Il2CppInterop.Runtime;
using PixelCrushers.DialogueSystem;
using UnifiedConversationTracker.Session;
using Language.Lua;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// Reads the running game's SimStatus values out of the Dialogue System, for
    /// the one-time seed of a unified state that has nothing on disk to load.
    /// </summary>
    /// <remarks>
    /// <para><b>Why the database and not the Lua table.</b> The authoritative values
    /// live in the Lua <c>Conversation</c> table, but its integer keys land in
    /// either the array part or the hash part of a <c>LuaTable</c> depending on the
    /// order they were inserted, so walking it directly means reimplementing Lua
    /// table semantics through IL2CPP interop. Walking the dialogue database for
    /// the conversation and entry IDs and asking
    /// <c>DialogueLua.GetSimStatus(conversationID, entryID)</c> for each one is the
    /// game's own accessor and handles both parts. It reports Untouched for anything
    /// absent, which is the correct answer and costs nothing to merge.</para>
    ///
    /// <para><b>Timing.</b> <c>PersistentDataManager</c> rebuilds the whole SimStatus
    /// table when a savegame loads, without going through
    /// <c>MarkDialogueEntry</c> (de-0s5). Reading before that has happened would
    /// return an all-Untouched table. <see cref="IsReady"/> only reports that the
    /// dialogue system is up, which is necessary but not sufficient; the sufficient
    /// part is the caller's trigger point, which is the first
    /// <c>MarkDialogueEntry</c> and therefore inside a running conversation, which
    /// can only happen after a game is in play.</para>
    /// </remarks>
    internal sealed class DialogueLuaSimStatusSource : ISimStatusSource
    {
        /// <summary>The Lua global holding the per-conversation tables.</summary>
        private const string ConversationTableName = "Conversation";

        private DialogueDatabase? _database;

        /// <inheritdoc />
        public string Description => "Dialogue System master database";

        /// <inheritdoc />
        public bool IsReady => ResolveDatabase() != null && HasConversationTable();

        /// <inheritdoc />
        public IEnumerable<SimStatusRow> EnumerateSimStatuses()
        {
            DialogueDatabase database = ResolveDatabase()
                ?? throw new InvalidOperationException(
                    "The Dialogue System master database is not available; the seed should have been deferred.");

            Il2CppSystem.Collections.Generic.List<Conversation> conversations = database.conversations;
            if (conversations == null)
            {
                yield break;
            }

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

                    yield return new SimStatusRow(
                        entry.conversationID,
                        entry.id,
                        DialogueLua.GetSimStatus(entry.conversationID, entry.id));
                }
            }
        }

        /// <summary>
        /// The master database, or null while the dialogue system is not up.
        /// </summary>
        /// <remarks>
        /// Cached once found, because <c>DialogueManager.masterDatabase</c> falls
        /// back to <c>FindObjectOfType</c> whenever its own static is null, and this
        /// is polled on every access until it succeeds.
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
        /// True once the Lua environment holds a <c>Conversation</c> table, which is
        /// what <c>DialogueLua.GetSimStatus</c> reads through. Without this, the
        /// first read would throw and the seed would be written off for the session.
        /// </summary>
        private static bool HasConversationTable()
        {
            LuaTable environment = Lua.Environment;
            if (environment == null)
            {
                return false;
            }

            LuaValue conversations = environment.GetValue(ConversationTableName);
            return conversations != null && conversations.TryCast<LuaTable>() != null;
        }
    }
}
