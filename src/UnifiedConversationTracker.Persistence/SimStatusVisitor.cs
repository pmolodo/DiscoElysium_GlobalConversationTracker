using System;
using System.Collections.Generic;
using System.IO;
using UnifiedConversationTracker.Core;

namespace UnifiedConversationTracker.Persistence;

/// <summary>
/// Pulls the SimStatus of every dialogue entry out of the blob, and nothing else.
///
/// The rows live at Conversation[convID].Dialog[dialogueID].SimStatus. That is one
/// string per entry out of a table that is most of the file, so this visitor only
/// ever decodes a conversation ID, a dialogue entry ID and a status string; every
/// other key is compared as raw UTF-8 bytes and every other value is stepped over
/// without being decoded, boxed or stored. The first four top-level tables are not
/// looked at at all - they still have to be walked, since a value's length is only
/// knowable from its own encoding, but nothing in them is read.
/// </summary>
public sealed class SimStatusVisitor : IRawDataVisitor
{
    private static ReadOnlySpan<byte> DialogKey => "Dialog"u8;
    private static ReadOnlySpan<byte> SimStatusKey => "SimStatus"u8;

    /// <summary>Where in the nesting a table sits, as far as this visitor cares.</summary>
    private enum Scope
    {
        /// <summary>Anything not on the path to a SimStatus. Walked, never read.</summary>
        Ignored,

        /// <summary>The Conversation table: conversation ID to conversation.</summary>
        Conversations,

        /// <summary>One conversation's fields, one of which must be Dialog.</summary>
        Conversation,

        /// <summary>One conversation's Dialog table: entry ID to entry.</summary>
        Dialogs,

        /// <summary>One dialogue entry's fields, one of which must be SimStatus.</summary>
        DialogEntry,
    }

    private struct Frame
    {
        public Scope Scope;

        /// <summary>
        /// Whether the key this scope is required to carry - Dialog for a
        /// Conversation, SimStatus for a DialogEntry - has turned up yet.
        /// </summary>
        public bool SawRequiredKey;
    }

    private readonly List<SimStatusRow> _rows = new();

    // Used as a stack; a List so the top frame can be updated in place, which
    // Stack<T> of a struct cannot do.
    private readonly List<Frame> _frames = new();

    private bool _inConversationTopLevel;
    private bool _expectingKey;

    // The key most recently read, in the only two forms this visitor needs it: as an
    // ID, and as "did it match the name this scope was looking for". Consumed by the
    // value that follows, including a table, which takes it as it opens.
    private int _pendingIntKey;
    private bool _pendingKeyMatches;

    private int _conversationId;
    private int _dialogueEntryId;

    // How much was walked to find the rows. One increment each on paths that already
    // run for every value and every table, so that a caller timing the parse can say
    // what the time was spent on rather than only how long it was.
    private long _conversationCount;
    private long _tableCount;
    private long _valueCount;

    /// <summary>Every SimStatus found, in file order.</summary>
    public List<SimStatusRow> Rows => _rows;

    /// <summary>Conversations found in the Conversation table.</summary>
    public long ConversationCount => _conversationCount;

    /// <summary>Tables opened, at every depth, across all five top-level tables.</summary>
    public long TableCount => _tableCount;

    /// <summary>
    /// Values stepped over. Table keys are values too and are counted here, since
    /// stepping over one costs the same as stepping over anything else.
    /// </summary>
    public long ValueCount => _valueCount;

    private Scope CurrentScope =>
        _frames.Count > 0 ? _frames[_frames.Count - 1].Scope : Scope.Ignored;

    /// <inheritdoc />
    public void BeginTopLevelValue(ref RawDataParser parser, int index, string name)
    {
        // Conversation is the last of the five; the other four are walked past.
        _inConversationTopLevel = index == RawDataParser.TableNames.Length - 1;
    }

    /// <inheritdoc />
    public void VisitDictKey(ref RawDataParser parser) => _expectingKey = true;

    /// <inheritdoc />
    public void VisitDictValue(ref RawDataParser parser) => _expectingKey = false;

    /// <inheritdoc />
    public void VisitListIndex(ref RawDataParser parser, int index)
    {
        _expectingKey = false;
        _pendingIntKey = index;
        _pendingKeyMatches = false;
    }

    /// <inheritdoc />
    public void VisitValue(ref RawDataParser parser, RawDataParser.DataType type)
    {
        _valueCount++;
        Scope scope = CurrentScope;
        if (_expectingKey)
        {
            // The ID scopes are keyed by number; VisitNumber does the rest.
            if (type != RawDataParser.DataType.Number)
            {
                if (scope == Scope.Conversations)
                {
                    throw new InvalidDataException(
                        $"Non-int conversation ID of type '{type}' at offset {parser.Position}"
                    );
                }
                if (scope == Scope.Dialogs)
                {
                    throw new InvalidDataException(
                        $"Non-int dialogue ID of type '{type}' for conversation "
                            + $"{_conversationId} at offset {parser.Position}"
                    );
                }
            }
            return;
        }

        switch (scope)
        {
            case Scope.Conversations:
                if (type != RawDataParser.DataType.Table)
                {
                    throw new InvalidDataException(
                        $"Non-table value for conversation ID {_pendingIntKey}: '{type}'"
                    );
                }
                break;
            case Scope.Conversation:
                if (_pendingKeyMatches && type != RawDataParser.DataType.Table)
                {
                    throw new InvalidDataException(
                        type == RawDataParser.DataType.Nil
                            ? $"Conversation ID {_conversationId} had a null 'Dialog' entry"
                            : $"Non-table value for conversation ID {_conversationId} "
                                + $"'Dialog' entry: '{type}'"
                    );
                }
                break;
            case Scope.Dialogs:
                if (type != RawDataParser.DataType.Table)
                {
                    throw new InvalidDataException(
                        $"Non-table value for conversation {_conversationId}, "
                            + $"dialogue {_pendingIntKey}: '{type}'"
                    );
                }
                break;
            case Scope.DialogEntry:
                if (_pendingKeyMatches && type != RawDataParser.DataType.String)
                {
                    throw new InvalidDataException(
                        type == RawDataParser.DataType.Nil
                            ? $"Conversation {_conversationId}, dialogue {_dialogueEntryId} "
                                + "had a null 'SimStatus' entry"
                            : $"Non-string value for conversation {_conversationId}, "
                                + $"dialogue {_dialogueEntryId} 'SimStatus' entry: '{type}'"
                    );
                }
                break;
        }
    }

    /// <inheritdoc />
    public void VisitString(ref RawDataParser parser, int length)
    {
        Scope scope = CurrentScope;
        if (_expectingKey)
        {
            // Compared as bytes: decoding every key in the Conversation table just to
            // find two names would cost more than the whole walk.
            _pendingKeyMatches = scope switch
            {
                Scope.Conversation => parser.PeekBytes(length).SequenceEqual(DialogKey),
                Scope.DialogEntry => parser.PeekBytes(length).SequenceEqual(SimStatusKey),
                _ => false,
            };
            return;
        }

        if (scope == Scope.DialogEntry && _pendingKeyMatches)
        {
            _rows.Add(
                new SimStatusRow(_conversationId, _dialogueEntryId, parser.PeekString(length))
            );
            MarkRequiredKeySeen();
        }
    }

    /// <inheritdoc />
    public void VisitNumber(ref RawDataParser parser)
    {
        if (!_expectingKey)
        {
            return;
        }
        _pendingKeyMatches = false;

        Scope scope = CurrentScope;
        if (scope != Scope.Conversations && scope != Scope.Dialogs)
        {
            // Nothing keyed by number anywhere else on the path matters.
            return;
        }

        double value = parser.PeekDouble();
        if (!RawDataParser.TryNumberToInt32(value, out _pendingIntKey))
        {
            throw new InvalidDataException(
                scope == Scope.Conversations
                    ? $"Non-int conversation ID '{value}' at offset {parser.Position}"
                    : $"Non-int dialogue ID '{value}' for conversation {_conversationId} "
                        + $"at offset {parser.Position}"
            );
        }
    }

    /// <inheritdoc />
    public void VisitBoolean(ref RawDataParser parser)
    {
        if (_expectingKey)
        {
            _pendingKeyMatches = false;
        }
    }

    /// <inheritdoc />
    public void OpenTable(ref RawDataParser parser)
    {
        _tableCount++;
        Scope child;
        if (_frames.Count == 0)
        {
            child = _inConversationTopLevel ? Scope.Conversations : Scope.Ignored;
        }
        else
        {
            switch (CurrentScope)
            {
                case Scope.Conversations:
                    _conversationId = _pendingIntKey;
                    _conversationCount++;
                    child = Scope.Conversation;
                    break;
                case Scope.Conversation:
                    if (_pendingKeyMatches)
                    {
                        MarkRequiredKeySeen();
                        child = Scope.Dialogs;
                    }
                    else
                    {
                        child = Scope.Ignored;
                    }
                    break;
                case Scope.Dialogs:
                    _dialogueEntryId = _pendingIntKey;
                    child = Scope.DialogEntry;
                    break;
                default:
                    child = Scope.Ignored;
                    break;
            }
        }

        _frames.Add(new Frame { Scope = child });
        // Whatever this table's own keys turn out to be, the parent's is spent.
        _expectingKey = false;
        _pendingKeyMatches = false;
    }

    /// <inheritdoc />
    public void CloseTable(ref RawDataParser parser)
    {
        Frame frame = _frames[_frames.Count - 1];
        _frames.RemoveAt(_frames.Count - 1);
        if (frame.SawRequiredKey)
        {
            return;
        }
        if (frame.Scope == Scope.Conversation)
        {
            throw new InvalidDataException(
                $"Conversation ID {_conversationId} did not have a 'Dialog' entry"
            );
        }
        if (frame.Scope == Scope.DialogEntry)
        {
            throw new InvalidDataException(
                $"Conversation {_conversationId}, dialogue {_dialogueEntryId} "
                    + "did not have a 'SimStatus' entry"
            );
        }
    }

    private void MarkRequiredKeySeen()
    {
        Frame frame = _frames[_frames.Count - 1];
        frame.SawRequiredKey = true;
        _frames[_frames.Count - 1] = frame;
    }
}
