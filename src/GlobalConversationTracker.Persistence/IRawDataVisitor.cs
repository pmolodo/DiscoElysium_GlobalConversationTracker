// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Persistence;

/// <summary>
/// Receives the structure of a .ntwtf.lua raw-data blob as
/// <see cref="RawDataParser"/> walks it, and decides how much of it to actually
/// decode.
///
/// The parser owns the cursor: it consumes every marker, count and length prefix
/// itself, because that is the only thing that says where a value ends. A visitor
/// never advances the cursor. It reads what it wants through the parser's Peek
/// methods and ignores the rest, so a visitor that cares about one table pays only
/// the walk for the other four.
///
/// Every method has a do-nothing default, so a visitor implements only the
/// callbacks it needs. That also means calls must be made through this interface
/// rather than through a concrete visitor type, which is how the parser holds it.
/// </summary>
/// <remarks>
/// The parser is passed by reference to every callback. It is a ref struct, so a
/// visitor cannot keep one in a field; taking it as a parameter is what lets the
/// visitor peek at the value it is being told about.
/// </remarks>
public interface IRawDataVisitor
{
    /// <summary>Called once, before the first of the five top-level values.</summary>
    void BeginParse(ref RawDataParser parser) { }

    /// <summary>
    /// Called once the five top-level values have been read. Anything left over is
    /// the blob's "extra data", which the parser does not interpret; the amount is
    /// <see cref="RawDataParser.Remaining"/>.
    /// </summary>
    void EndParse(ref RawDataParser parser) { }

    /// <summary>
    /// Called before each of the five top-level values, with its index and the name
    /// it is known by (<see cref="RawDataParser.TableNames"/>).
    /// </summary>
    void BeginTopLevelValue(ref RawDataParser parser, int index, string name) { }

    /// <summary>
    /// Called for every value, after its type marker has been consumed and before
    /// its payload is read. This is where a visitor can reject an unexpected shape
    /// without having to implement all of the per-type callbacks.
    /// </summary>
    void VisitValue(ref RawDataParser parser, RawDataParser.DataType type) { }

    /// <summary>
    /// A length-prefixed UTF-8 string. Decode it with
    /// <see cref="RawDataParser.PeekString"/>, or compare it without allocating with
    /// <see cref="RawDataParser.PeekBytes"/>.
    /// </summary>
    void VisitString(ref RawDataParser parser, int length) { }

    /// <summary>A number. Read it with <see cref="RawDataParser.PeekDouble"/>.</summary>
    void VisitNumber(ref RawDataParser parser) { }

    /// <summary>A boolean. Read it with <see cref="RawDataParser.PeekBoolean"/>.</summary>
    void VisitBoolean(ref RawDataParser parser) { }

    /// <summary>A nil. It has no payload, so there is nothing to peek at.</summary>
    void VisitNil(ref RawDataParser parser) { }

    /// <summary>
    /// A table is about to be walked. Fires after the table's marker, before its
    /// list count. <see cref="RawDataParser.Depth"/> counts the table this one is
    /// nested in; it does not include this one yet.
    /// </summary>
    void OpenTable(ref RawDataParser parser) { }

    /// <summary>The table opened by the matching <see cref="OpenTable"/> is finished.</summary>
    void CloseTable(ref RawDataParser parser) { }

    /// <summary>The table's array part, with the number of entries that follow.</summary>
    void OpenList(ref RawDataParser parser, int count) { }

    /// <summary>
    /// The next value belongs at this list position. Lua lists are 1-indexed, so
    /// <paramref name="index"/> starts at 1.
    /// </summary>
    void VisitListIndex(ref RawDataParser parser, int index) { }

    /// <summary>The array part is finished.</summary>
    void CloseList(ref RawDataParser parser) { }

    /// <summary>The table's hash part, with the number of pairs that follow.</summary>
    void OpenDict(ref RawDataParser parser, int count) { }

    /// <summary>A key/value pair is about to be walked.</summary>
    void OpenKeyValuePair(ref RawDataParser parser) { }

    /// <summary>The next value is this pair's key.</summary>
    void VisitDictKey(ref RawDataParser parser) { }

    /// <summary>The next value is this pair's value.</summary>
    void VisitDictValue(ref RawDataParser parser) { }

    /// <summary>The pair is finished.</summary>
    void CloseKeyValuePair(ref RawDataParser parser) { }

    /// <summary>The hash part is finished.</summary>
    void CloseDict(ref RawDataParser parser) { }
}
