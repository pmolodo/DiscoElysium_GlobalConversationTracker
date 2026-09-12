// SPDX-License-Identifier: MIT
using System.Globalization;

using Wire = GlobalConversationTracker.Engine.Wire;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// One answer to a question the engine asked, in the shape it crosses in.
    /// </summary>
    /// <remarks>
    /// <para>Tagged with a <c>kind</c> string rather than by the shape of the value, so
    /// there is one thing to write and one thing to read whichever variant it is:</para>
    /// <code>
    /// {"kind":"bool","value":true}
    /// {"kind":"number","value":3}
    /// {"kind":"text","value":"blue"}
    /// {"kind":"unknown"}
    /// </code>
    ///
    /// <para><see cref="Unknown"/> is not a failure to answer, it is an answer: the engine
    /// treats it as passable, so it widens the reachable set rather than narrowing it -
    /// the direction that costs a wasted click instead of hiding content the player has
    /// never seen. It is also the answer for a question left out of the snapshot
    /// altogether, so sending it explicitly and omitting it mean the same thing - except
    /// for a dialogue variable, where both fall back to what the database declares: see
    /// <see cref="WorldSnapshot.VariableValues"/>.</para>
    /// </remarks>
    public readonly struct WireValue
    {
        private readonly string _kind;
        private readonly bool _boolean;
        private readonly double _number;
        private readonly string? _text;

        private WireValue(string kind, bool boolean, double number, string? text)
        {
            _kind = kind;
            _boolean = boolean;
            _number = number;
            _text = text;
        }

        /// <summary>Not knowable, and the permissive answer.</summary>
        public static WireValue Unknown { get; } = new WireValue("unknown", false, 0, null);

        /// <summary>A boolean answer.</summary>
        /// <param name="value">What it is.</param>
        public static WireValue FromBoolean(bool value)
        {
            return new WireValue("bool", value, 0, null);
        }

        /// <summary>A numeric answer.</summary>
        /// <param name="value">What it is.</param>
        public static WireValue FromNumber(double value)
        {
            return new WireValue("number", false, value, null);
        }

        /// <summary>A text answer.</summary>
        /// <param name="value">What it is.</param>
        public static WireValue FromText(string value)
        {
            return new WireValue("text", false, 0, value);
        }

        /// <summary>Whether this is <see cref="Unknown"/>.</summary>
        public bool IsUnknown => _kind == "unknown";

        /// <summary>The value as it crosses.</summary>
        /// <remarks>
        /// UNKNOWN IS AN UNSET ONEOF, which is to say a message with nothing in it. That is
        /// what silence means on the far side, it encodes to no bytes at all, and it is
        /// what a default-constructed one already is - so there is no way to build a value
        /// that means neither this nor an answer.
        /// </remarks>
        public Wire.WireValue Written()
        {
            switch (_kind)
            {
                case "bool":
                    return new Wire.WireValue { Boolean = _boolean };
                case "number":
                    return new Wire.WireValue { Number = _number };
                case "text":
                    return new Wire.WireValue { Text = _text };
                default:
                    return new Wire.WireValue();
            }
        }

        /// <inheritdoc/>
        public override string ToString()
        {
            switch (_kind)
            {
                case "bool":
                    return _boolean ? "true" : "false";
                case "number":
                    return _number.ToString(CultureInfo.InvariantCulture);
                case "text":
                    return "\"" + _text + "\"";
                default:
                    return "unknown";
            }
        }
    }
}
