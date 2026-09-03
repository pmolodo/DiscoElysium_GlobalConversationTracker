// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>What one scanned line of the conversations section carries.</summary>
    public enum DialogueAssetEventKind
    {
        /// <summary>A new conversation begins; <see cref="DialogueAssetEvent.Text"/> is its id.</summary>
        ConversationStart,

        /// <summary>
        /// A conversation-level field, from the two-line "title:" then "value:" pair Unity
        /// writes fields in. Only reported before the conversation's first dialogue entry,
        /// because that is the only place they appear.
        /// </summary>
        ConversationField,

        /// <summary>A new dialogue entry begins; <see cref="DialogueAssetEvent.Text"/> is its id.</summary>
        EntryStart,

        /// <summary>An entry-level field, from the same two-line pair.</summary>
        EntryField,

        /// <summary>
        /// An entry property written directly rather than as a field: conversationID,
        /// isGroup, conditionsString or userScript.
        /// </summary>
        EntryProperty,

        /// <summary>
        /// One destination of an outgoing link: destinationDialogueID or
        /// destinationConversationID.
        /// </summary>
        EntryLink,
    }

    /// <summary>One thing the scanner found, as the line said it.</summary>
    /// <remarks>
    /// <see cref="Text"/> is the raw remainder of the line, undecoded and untrimmed, and
    /// so is <see cref="Name"/> for the two field kinds. Decoding is left to the caller on
    /// purpose: the extractors that read this asset do not all unescape a double-quoted
    /// scalar the same way, and a scanner that decoded for them would silently impose one
    /// of those readings on the others.
    /// </remarks>
    public readonly struct DialogueAssetEvent
    {
        internal DialogueAssetEvent(DialogueAssetEventKind kind, string name, string text)
        {
            Kind = kind;
            Name = name;
            Text = text;
        }

        /// <summary>What was found.</summary>
        public DialogueAssetEventKind Kind { get; }

        /// <summary>
        /// The field, property or link name, still as the line wrote it. Empty for the two
        /// start kinds.
        /// </summary>
        public string Name { get; }

        /// <summary>The rest of the line after the prefix that identified it.</summary>
        public string Text { get; }
    }

    /// <summary>
    /// Streams the conversations section of a Dialogue System database .asset.
    /// </summary>
    /// <remarks>
    /// The .asset is a Unity-serialized YAML document of ~170 MB, so it is read line by
    /// line at fixed indentation rather than parsed as YAML. Every prefix below carries
    /// its trailing space, which is load-bearing: Unity writes an empty scalar as
    /// "conditionsString:" with nothing after the colon, and that line deliberately does
    /// not match.
    /// </remarks>
    public static class DialogueAssetScanner
    {
        /// <summary>The line the conversations begin after.</summary>
        public const string ConversationsSection = "  conversations:";

        /// <summary>The line the conversations end at.</summary>
        public const string NextSection = "  syncInfo:";

        /// <summary>
        /// The conversationID property name: the id of the conversation an entry says it
        /// belongs to, which is not always read but is worth checking against the
        /// conversation it was actually written inside.
        /// </summary>
        public const string ConversationIdProperty = "conversationID";

        /// <summary>The isGroup property name, as <see cref="DialogueAssetEvent.Name"/> reports it.</summary>
        public const string IsGroupProperty = "isGroup";

        /// <summary>The conditionsString property name.</summary>
        public const string ConditionsProperty = "conditionsString";

        /// <summary>The userScript property name.</summary>
        public const string ScriptProperty = "userScript";

        /// <summary>The link name carrying a destination entry id.</summary>
        public const string DestinationEntryLink = "destinationDialogueID";

        /// <summary>The link name carrying a destination conversation id.</summary>
        public const string DestinationConversationLink = "destinationConversationID";

        private const string ConversationStartPrefix = "  - id: ";
        private const string ConversationFieldPrefix = "    - title: ";
        private const string ConversationValuePrefix = "      value: ";

        private const string EntryStartPrefix = "    - id: ";
        private const string EntryFieldPrefix = "      - title: ";
        private const string EntryValuePrefix = "        value: ";
        // Six spaces, so the eight-space originConversationID inside an outgoing link does
        // not match it.
        private const string EntryConversationIdPrefix = "      conversationID: ";
        private const string EntryIsGroupPrefix = "      isGroup: ";
        private const string EntryConditionsPrefix = "      conditionsString: ";
        private const string EntryScriptPrefix = "      userScript: ";
        private const string LinkDestinationPrefix = "        destinationDialogueID: ";
        private const string LinkDestinationConversationPrefix = "        destinationConversationID: ";

        // Big enough that reading 170 MB is not a syscall per few lines.
        private const int ReadBufferBytes = 1 << 16;

        /// <summary>Scans the database at <paramref name="path"/>.</summary>
        public static IEnumerable<DialogueAssetEvent> Scan(string path)
        {
            // Replacement, not an exception, on malformed bytes: the Python this was
            // ported from read the file with errors="replace", and a database that is
            // 170 MB of exported game text is not worth failing over one bad byte.
            using var reader = new StreamReader(path, Encoding.UTF8, detectEncodingFromByteOrderMarks: true,
                ReadBufferBytes);
            foreach (DialogueAssetEvent item in Scan(reader))
            {
                yield return item;
            }
        }

        /// <summary>Scans an already-open database.</summary>
        public static IEnumerable<DialogueAssetEvent> Scan(TextReader reader)
        {
            bool inside = false;
            bool conversationOpen = false;
            bool entryOpen = false;
            // Which field's value line is expected next, for the two-line "title: X" then
            // "value: Y" shape. Neither is cleared at a conversation or entry boundary,
            // only when its value arrives.
            string? pendingConversationField = null;
            string? pendingEntryField = null;

            string? line;
            while ((line = reader.ReadLine()) != null)
            {
                if (!inside)
                {
                    inside = line == ConversationsSection;
                    continue;
                }

                if (line == NextSection)
                {
                    break;
                }

                if (line.StartsWith(ConversationStartPrefix, StringComparison.Ordinal))
                {
                    conversationOpen = true;
                    entryOpen = false;
                    yield return Event(DialogueAssetEventKind.ConversationStart, line, ConversationStartPrefix);
                    continue;
                }

                if (!conversationOpen)
                {
                    continue;
                }

                if (line.StartsWith(EntryStartPrefix, StringComparison.Ordinal))
                {
                    entryOpen = true;
                    yield return Event(DialogueAssetEventKind.EntryStart, line, EntryStartPrefix);
                    continue;
                }

                if (!entryOpen)
                {
                    if (line.StartsWith(ConversationFieldPrefix, StringComparison.Ordinal))
                    {
                        pendingConversationField = line.Substring(ConversationFieldPrefix.Length);
                    }
                    else if (pendingConversationField != null
                        && line.StartsWith(ConversationValuePrefix, StringComparison.Ordinal))
                    {
                        yield return new DialogueAssetEvent(DialogueAssetEventKind.ConversationField,
                            pendingConversationField, line.Substring(ConversationValuePrefix.Length));
                        pendingConversationField = null;
                    }

                    continue;
                }

                if (line.StartsWith(EntryFieldPrefix, StringComparison.Ordinal))
                {
                    pendingEntryField = line.Substring(EntryFieldPrefix.Length);
                }
                else if (pendingEntryField != null
                    && line.StartsWith(EntryValuePrefix, StringComparison.Ordinal))
                {
                    yield return new DialogueAssetEvent(DialogueAssetEventKind.EntryField,
                        pendingEntryField, line.Substring(EntryValuePrefix.Length));
                    pendingEntryField = null;
                }
                else if (line.StartsWith(EntryConversationIdPrefix, StringComparison.Ordinal))
                {
                    yield return Property(ConversationIdProperty, line, EntryConversationIdPrefix);
                }
                else if (line.StartsWith(EntryIsGroupPrefix, StringComparison.Ordinal))
                {
                    yield return Property(IsGroupProperty, line, EntryIsGroupPrefix);
                }
                else if (line.StartsWith(EntryConditionsPrefix, StringComparison.Ordinal))
                {
                    yield return Property(ConditionsProperty, line, EntryConditionsPrefix);
                }
                else if (line.StartsWith(EntryScriptPrefix, StringComparison.Ordinal))
                {
                    yield return Property(ScriptProperty, line, EntryScriptPrefix);
                }
                else if (line.StartsWith(LinkDestinationPrefix, StringComparison.Ordinal))
                {
                    yield return Link(DestinationEntryLink, line, LinkDestinationPrefix);
                }
                else if (line.StartsWith(LinkDestinationConversationPrefix, StringComparison.Ordinal))
                {
                    yield return Link(DestinationConversationLink, line, LinkDestinationConversationPrefix);
                }
            }
        }

        private static DialogueAssetEvent Event(DialogueAssetEventKind kind, string line, string prefix)
        {
            return new DialogueAssetEvent(kind, string.Empty, line.Substring(prefix.Length));
        }

        private static DialogueAssetEvent Property(string name, string line, string prefix)
        {
            return new DialogueAssetEvent(DialogueAssetEventKind.EntryProperty, name,
                line.Substring(prefix.Length));
        }

        private static DialogueAssetEvent Link(string name, string line, string prefix)
        {
            return new DialogueAssetEvent(DialogueAssetEventKind.EntryLink, name, line.Substring(prefix.Length));
        }
    }
}
