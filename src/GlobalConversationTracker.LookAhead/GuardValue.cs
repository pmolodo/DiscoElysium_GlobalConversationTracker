// SPDX-License-Identifier: MIT
using System.Globalization;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>What a guard sub-expression evaluated to, or that it could not be.</summary>
    public enum GuardValueKind
    {
        /// <summary>Not determinable from the state we hold.</summary>
        Unknown = 0,

        /// <summary>A boolean.</summary>
        Boolean = 1,

        /// <summary>A number.</summary>
        Number = 2,

        /// <summary>A string literal.</summary>
        Text = 3,
    }

    /// <summary>
    /// The result of evaluating part of a guard: a boolean, a number, a string, or
    /// the admission that we do not know.
    /// </summary>
    /// <remarks>
    /// <para><see cref="GuardValueKind.Unknown"/> is the load-bearing case. A world
    /// query the host cannot answer, or a variable absent from the state, produces it,
    /// and it propagates through comparisons and connectives so the engine can apply
    /// the soundness rule: an undetermined guard must never be treated as blocking.
    /// Under-reporting hides content the player has not seen, which defeats the
    /// feature; over-reporting only costs a wasted click.</para>
    ///
    /// <para>Truthiness follows Lua, because these expressions are Lua: everything
    /// except <c>false</c> and <c>nil</c> is true. A number of 0 is therefore
    /// <em>true</em>, which is why <see cref="AsCondition"/> does not simply test for
    /// non-zero.</para>
    /// </remarks>
    public readonly struct GuardValue
    {
        private readonly double _number;
        private readonly bool _boolean;
        private readonly string? _text;

        private GuardValue(GuardValueKind kind, bool boolean, double number, string? text)
        {
            Kind = kind;
            _boolean = boolean;
            _number = number;
            _text = text;
        }

        /// <summary>What sort of value this is.</summary>
        public GuardValueKind Kind { get; }

        /// <summary>The undetermined value.</summary>
        public static GuardValue Unknown => new GuardValue(GuardValueKind.Unknown, false, 0, null);

        /// <summary>A boolean value.</summary>
        /// <param name="value">The boolean.</param>
        public static GuardValue FromBoolean(bool value)
        {
            return new GuardValue(GuardValueKind.Boolean, value, 0, null);
        }

        /// <summary>A numeric value.</summary>
        /// <param name="value">The number.</param>
        public static GuardValue FromNumber(double value)
        {
            return new GuardValue(GuardValueKind.Number, false, value, null);
        }

        /// <summary>A string value.</summary>
        /// <param name="value">The string.</param>
        public static GuardValue FromText(string value)
        {
            return new GuardValue(GuardValueKind.Text, false, 0, value);
        }

        /// <summary>The boolean, when <see cref="Kind"/> is Boolean.</summary>
        public bool Boolean => _boolean;

        /// <summary>The number, when <see cref="Kind"/> is Number.</summary>
        public double Number => _number;

        /// <summary>The string, when <see cref="Kind"/> is Text.</summary>
        public string Text => _text ?? string.Empty;

        /// <summary>
        /// This value read as a condition, Lua style: anything but <c>false</c> is true.
        /// </summary>
        public Ternary AsCondition()
        {
            switch (Kind)
            {
                case GuardValueKind.Boolean:
                    return _boolean ? Ternary.True : Ternary.False;
                case GuardValueKind.Number:
                case GuardValueKind.Text:
                    return Ternary.True;
                default:
                    return Ternary.Unknown;
            }
        }

        /// <summary>
        /// This value as a number, for ordering comparisons. Booleans do not convert:
        /// Lua raises on <c>true &lt; 1</c>, and treating them as 0/1 would invent an
        /// ordering the game does not have.
        /// </summary>
        /// <param name="value">The number, when this returns true.</param>
        public bool TryAsNumber(out double value)
        {
            value = _number;
            return Kind == GuardValueKind.Number;
        }

        /// <inheritdoc/>
        public override string ToString()
        {
            switch (Kind)
            {
                case GuardValueKind.Boolean:
                    return _boolean ? "true" : "false";
                case GuardValueKind.Number:
                    return _number.ToString(CultureInfo.InvariantCulture);
                case GuardValueKind.Text:
                    return "\"" + Text + "\"";
                default:
                    return "unknown";
            }
        }
    }
}
