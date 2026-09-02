// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.IO;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>
    /// Turns the streamed conversations section into one <see cref="ConversationRecord"/>
    /// per conversation.
    /// </summary>
    /// <remarks>
    /// The index is what makes it possible to choose an in-game test's conversation on the
    /// shape of its graph - how many options a menu offers, how far a forward scan can
    /// travel from one - rather than on whatever happens to stand near the player in a
    /// save. The guard and script carry conditionsString and userScript verbatim, because
    /// what a forward scan can reach turns on them: a purchase is a guard that tests money
    /// and a script that spends it, and choosing a test scenario means reading both.
    /// </remarks>
    public static class ConversationIndexExtractor
    {
        private const string TitleField = "Title";
        private const string ActorField = "Actor";
        private const string ConversantField = "Conversant";
        private const string GroupValue = "1";

        /// <summary>Extracts every conversation from the database at <paramref name="path"/>.</summary>
        public static IEnumerable<ConversationRecord> Extract(string path)
        {
            return Extract(DialogueAssetScanner.Scan(path));
        }

        /// <summary>Extracts every conversation from an already-open database.</summary>
        public static IEnumerable<ConversationRecord> Extract(TextReader reader)
        {
            return Extract(DialogueAssetScanner.Scan(reader));
        }

        private static IEnumerable<ConversationRecord> Extract(IEnumerable<DialogueAssetEvent> scan)
        {
            ConversationRecord? conversation = null;
            EntryRecord? entry = null;

            foreach (DialogueAssetEvent item in scan)
            {
                switch (item.Kind)
                {
                    case DialogueAssetEventKind.ConversationStart:
                        if (conversation != null)
                        {
                            Close(conversation, entry);
                            yield return conversation;
                        }

                        entry = null;
                        conversation = new ConversationRecord { Id = DialogueScalar.ParseId(item.Text) };
                        break;

                    case DialogueAssetEventKind.ConversationField:
                        // Three of the conversation's fields are wanted; the rest, and the
                        // whole list once the entries have started, are not.
                        switch (DialogueScalar.Decode(item.Name))
                        {
                            case TitleField:
                                conversation!.Title = DialogueScalar.Decode(item.Text);
                                break;
                            case ActorField:
                                conversation!.Actor = DialogueScalar.AsInt(DialogueScalar.Decode(item.Text));
                                break;
                            case ConversantField:
                                conversation!.Conversant = DialogueScalar.AsInt(DialogueScalar.Decode(item.Text));
                                break;
                        }

                        break;

                    case DialogueAssetEventKind.EntryStart:
                        if (entry != null)
                        {
                            conversation!.Entries.Add(entry);
                        }

                        entry = new EntryRecord { Id = DialogueScalar.ParseId(item.Text) };
                        break;

                    case DialogueAssetEventKind.EntryField:
                        {
                            string name = DialogueScalar.Decode(item.Name);
                            if (name.Length == 0)
                            {
                                // A field with no name is no field: the index has always
                                // dropped these rather than keying an entry on "".
                                break;
                            }

                            string value = DialogueScalar.Decode(item.Text);
                            entry!.Fields[name] = value;
                            if (name == TitleField)
                            {
                                entry.Title = value;
                            }
                        }

                        break;

                    case DialogueAssetEventKind.EntryProperty:
                        switch (item.Name)
                        {
                            case DialogueAssetScanner.IsGroupProperty:
                                entry!.Group = item.Text.Trim() == GroupValue;
                                break;
                            case DialogueAssetScanner.ConditionsProperty:
                                entry!.Guard = DialogueScalar.Decode(item.Text);
                                break;
                            case DialogueAssetScanner.ScriptProperty:
                                entry!.Script = DialogueScalar.Decode(item.Text);
                                break;
                        }

                        break;

                    case DialogueAssetEventKind.EntryLink:
                        if (item.Name == DialogueAssetScanner.DestinationEntryLink)
                        {
                            entry!.To.Add(DialogueScalar.ParseId(item.Text));
                        }
                        else
                        {
                            (entry!.ToConversation ??= new List<int>()).Add(DialogueScalar.ParseId(item.Text));
                        }

                        break;
                }
            }

            if (conversation != null)
            {
                Close(conversation, entry);
                yield return conversation;
            }
        }

        private static void Close(ConversationRecord conversation, EntryRecord? entry)
        {
            if (entry != null)
            {
                conversation.Entries.Add(entry);
            }
        }
    }
}
