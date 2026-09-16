// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using GlobalConversationTracker.Engine;
using PixelCrushers.DialogueSystem;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The dialogue database the game actually loaded, read the way the index describes it.
    /// </summary>
    /// <remarks>
    /// <para>The mod ships a pre-computed index of this database. That index is a CACHE,
    /// valid exactly while it still describes what is in memory - and a game patch, a
    /// localisation or another mod can end that. This is the live half of the comparison:
    /// the same reduction the extractor applied to 170 MB of YAML, applied instead to the
    /// PixelCrushers objects in front of us.</para>
    ///
    /// <para>It is deliberately NOT a second implementation of that reduction.
    /// <see cref="ConversationHasher"/> is the reduction; this only feeds it, which is what
    /// keeps the extractor and the plugin from drifting apart on what a conversation is.
    /// </para>
    ///
    /// <para>Reading a live IL2CPP object graph from a UI callback can throw at any point.
    /// Everything here reports "could not read" rather than a value, because the caller's
    /// only two answers are "the cache matches" and "rebuild it", and a wrong hash would
    /// pick the second one for no reason.</para>
    /// </remarks>
    internal static class LiveDialogueDatabase
    {
        /// <summary>
        /// What one conversation in the loaded database reduces to, or null if it is not
        /// there or could not be read.
        /// </summary>
        /// <param name="conversationId">The conversation id.</param>
        internal static string? HashOf(int conversationId)
        {
            try
            {
                DialogueDatabase database = DialogueManager.masterDatabase;
                Conversation? conversation = database == null
                    ? null
                    : database.GetConversation(conversationId);
                if (conversation == null)
                {
                    // A conversation the shipped index knows about and the loaded database
                    // does not is exactly the mismatch this exists to notice, so it is a
                    // "no" rather than an error - and the caller reads a null the same way.
                    return null;
                }

                return Read(conversation).Hash();
            }
            catch (Exception)
            {
                return null;
            }
        }

        /// <summary>
        /// How many entries the loaded database holds for a conversation, or -1 if it holds
        /// no such conversation.
        /// </summary>
        /// <remarks>
        /// For explaining a cache miss. Comparing this against the index's own count is the
        /// cheapest question worth asking when the two disagree, and it separates "a
        /// different conversation" from "the same conversation written differently" - which
        /// need quite different investigations.
        /// </remarks>
        /// <param name="conversationId">The conversation id.</param>
        internal static int EntryCountOf(int conversationId)
        {
            try
            {
                DialogueDatabase database = DialogueManager.masterDatabase;
                Conversation? conversation = database == null
                    ? null
                    : database.GetConversation(conversationId);
                if (conversation == null)
                {
                    return -1;
                }

                int count = 0;
                foreach (DialogueEntry _ in Entries(conversation))
                {
                    count++;
                }

                return count;
            }
            catch (Exception)
            {
                return -1;
            }
        }

        /// <summary>
        /// One live conversation, as an index carries it.
        /// </summary>
        /// <remarks>
        /// The same reading a hash is taken over, so a rebuilt index and the check that
        /// asked for it cannot disagree about what was in the database - they are one walk
        /// written once.
        /// </remarks>
        /// <param name="conversation">The conversation, out of the loaded database.</param>
        internal static IndexConversation Read(Conversation conversation)
        {
            var built = new IndexConversation(conversation.id);
            foreach (KeyValuePair<string, string> field in FieldsOf(conversation.fields))
            {
                built.Fields.Add(field);
            }

            foreach (DialogueEntry entry in Entries(conversation))
            {
                var line = new IndexEntry
                {
                    Id = entry.id,
                    Group = entry.isGroup,
                    Guard = entry.conditionsString ?? string.Empty,
                    Script = entry.userScript ?? string.Empty,
                };

                foreach (KeyValuePair<int, int> link in LinksOf(entry))
                {
                    line.Links.Add(link);
                }

                foreach (KeyValuePair<string, string> field in FieldsOf(entry))
                {
                    line.Fields.Add(field);
                }

                built.Entries.Add(line);
            }

            return built;
        }

        /// <summary>Its dialogue entries, skipping any the database left null.</summary>
        private static IEnumerable<DialogueEntry> Entries(Conversation conversation)
        {
            Il2CppSystem.Collections.Generic.List<DialogueEntry> entries =
                conversation.dialogueEntries;
            if (entries == null)
            {
                yield break;
            }

            for (int index = 0; index < entries.Count; index++)
            {
                DialogueEntry entry = entries[index];
                if (entry != null)
                {
                    yield return entry;
                }
            }
        }

        /// <summary>
        /// Where an entry's links go, as (conversation, entry) pairs.
        /// </summary>
        /// <remarks>
        /// Every live link names its destination conversation outright. The index writes
        /// that key only when a link leaves its own conversation, and the extractor's side
        /// fills the absent ones back in - so both arrive here having resolved the same
        /// thing, which is the only way the two hashes can agree.
        /// </remarks>
        private static IEnumerable<KeyValuePair<int, int>> LinksOf(DialogueEntry entry)
        {
            Il2CppSystem.Collections.Generic.List<Link> links = entry.outgoingLinks;
            if (links == null)
            {
                yield break;
            }

            for (int index = 0; index < links.Count; index++)
            {
                Link link = links[index];
                if (link != null)
                {
                    yield return new KeyValuePair<int, int>(
                        link.destinationConversationID, link.destinationDialogueID);
                }
            }
        }

        /// <summary>Its fields, by title. The hasher keeps only the ones it reads.</summary>
        private static IEnumerable<KeyValuePair<string, string>> FieldsOf(DialogueEntry entry)
        {
            return FieldsOf(entry.fields);
        }

        /// <summary>A field list's titled fields, as name and value.</summary>
        private static IEnumerable<KeyValuePair<string, string>> FieldsOf(
            Il2CppSystem.Collections.Generic.List<Field>? fields)
        {
            if (fields == null)
            {
                yield break;
            }

            for (int index = 0; index < fields.Count; index++)
            {
                Field field = fields[index];
                if (field != null && field.title != null)
                {
                    yield return new KeyValuePair<string, string>(
                        field.title, field.value ?? string.Empty);
                }
            }
        }
    }
}
