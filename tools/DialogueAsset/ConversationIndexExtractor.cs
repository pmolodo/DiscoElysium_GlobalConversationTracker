// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.IO;
using GlobalConversationTracker.Engine;

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

        /// <summary>What schedules an entry's presentation, and the one field read back as text.</summary>
        private const string SequenceField = "Sequence";

        /// <summary>
        /// Turns the escaped line breaks a multi-line sequence is exported with back into real
        /// ones, so the index spells it the way the loaded database does.
        /// </summary>
        /// <remarks>
        /// <para>NARROW ON PURPOSE, and the narrowness is the point. <see cref="DialogueScalar.Decode"/>
        /// says it unescapes only \" and \\ and that a reader needing more wants its own
        /// decoder, because widening the shared one would silently rewrite every field of the
        /// index. This is that own decoder, applied to the one field that needs it.</para>
        ///
        /// <para>WHY IT NEEDS IT: 41 sequences in the database run to several lines, and the
        /// export writes those breaks as the two characters \ and n where the game holds a real
        /// newline. Left alone, the index carries text the game does not have - and the
        /// conversation hash says so, which is how this was found. See de-oaaq.</para>
        /// </remarks>
        private static string UnescapeLines(string value)
        {
            return value.Contains("\\n") ? value.Replace("\\n", "\n") : value;
        }
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
                        {
                            string name = DialogueScalar.Decode(item.Name);
                            if (name.Length == 0)
                            {
                                // As for an entry: a field with no name is no field.
                                break;
                            }

                            // Every field is kept, as for an entry, so that a reader after
                            // one the index line never named - the Articy Id, above all -
                            // does not need a scan of its own. Three of them are also read
                            // out by name, because the line carries those.
                            string value = DialogueScalar.Decode(item.Text);
                            conversation!.Fields[name] = value;
                            if (System.Array.IndexOf(IndexFields.ConversationRead, name) >= 0)
                            {
                                (conversation.IndexedFields ??= new OrderedDictionary<string, string>())[name] = value;
                            }

                            switch (name)
                            {
                                case TitleField:
                                    conversation.Title = value;
                                    break;
                                case ActorField:
                                    conversation.Actor = DialogueScalar.AsInt(value);
                                    break;
                                case ConversantField:
                                    conversation.Conversant = DialogueScalar.AsInt(value);
                                    break;
                            }
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
                            if (name == SequenceField)
                            {
                                value = UnescapeLines(value);
                            }

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
                            case DialogueAssetScanner.ConversationIdProperty:
                                entry!.ConversationId = DialogueScalar.AsInt(item.Text);
                                break;
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
