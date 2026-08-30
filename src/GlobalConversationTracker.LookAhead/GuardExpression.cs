// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>
    /// Supplies the facts a guard asks about: variable values, and the results of the
    /// game's read-only world queries.
    /// </summary>
    /// <remarks>
    /// Split out so the engine never touches game types. The plugin implements this
    /// over the live Lua state and the player character; tests implement it over a
    /// dictionary.
    /// </remarks>
    public interface IGuardContext
    {
        /// <summary>The value of a <c>Variable["name"]</c> reference.</summary>
        /// <param name="name">The variable's name, without the <c>Variable[]</c> wrapper.</param>
        /// <returns>Its value, or <see cref="GuardValue.Unknown"/> if we do not hold it.</returns>
        GuardValue GetVariable(string name);

        /// <summary>
        /// The result of a world query such as <c>IsKimHere()</c> or
        /// <c>CheckItem("shoes_faln")</c>.
        /// </summary>
        /// <param name="name">The function's name.</param>
        /// <param name="arguments">Its evaluated arguments, in order.</param>
        /// <returns>
        /// Its result, or <see cref="GuardValue.Unknown"/> for a query this context
        /// cannot answer - which the engine treats as passable, never as blocking.
        /// </returns>
        GuardValue Query(string name, IReadOnlyList<GuardValue> arguments);
    }

    /// <summary>One node of a parsed guard.</summary>
    /// <remarks>
    /// <para>The guard language is small: 26,210 guards across the shipped database
    /// reduce to 986 distinct shapes built from <c>Variable[...]</c> references,
    /// read-only calls, the six comparison operators, <c>and</c>/<c>or</c>/<c>not</c>,
    /// and parentheses. All 26,210 parse with this grammar.</para>
    ///
    /// <para>The articy exporter writes negation as <c>(expr) == false</c> rather than
    /// <c>not expr</c>, which is why comparison against a boolean literal has to be
    /// handled properly rather than treated as an oddity - it is the single most
    /// common negative shape in the database.</para>
    /// </remarks>
    public abstract class GuardExpression
    {
        /// <summary>Evaluates this expression against the supplied facts.</summary>
        /// <param name="context">Where variable and query results come from.</param>
        public abstract GuardValue Evaluate(IGuardContext context);

        /// <summary>Evaluates this expression as a condition.</summary>
        /// <param name="context">Where variable and query results come from.</param>
        public Ternary Test(IGuardContext context)
        {
            return Evaluate(context).AsCondition();
        }

        /// <summary>A literal.</summary>
        public sealed class Literal : GuardExpression
        {
            /// <summary>Creates a literal.</summary>
            /// <param name="value">The value.</param>
            public Literal(GuardValue value)
            {
                Value = value;
            }

            /// <summary>The literal's value.</summary>
            public GuardValue Value { get; }

            /// <inheritdoc/>
            public override GuardValue Evaluate(IGuardContext context)
            {
                return Value;
            }

            /// <inheritdoc/>
            public override string ToString()
            {
                return Value.ToString();
            }
        }

        /// <summary>A <c>Variable["name"]</c> reference.</summary>
        public sealed class Variable : GuardExpression
        {
            /// <summary>Creates a reference.</summary>
            /// <param name="name">The variable's name.</param>
            public Variable(string name)
            {
                Name = name ?? throw new ArgumentNullException(nameof(name));
            }

            /// <summary>The variable's name.</summary>
            public string Name { get; }

            /// <inheritdoc/>
            public override GuardValue Evaluate(IGuardContext context)
            {
                return context.GetVariable(Name);
            }

            /// <inheritdoc/>
            public override string ToString()
            {
                return "Variable[\"" + Name + "\"]";
            }
        }

        /// <summary>A call to a read-only world query.</summary>
        public sealed class Call : GuardExpression
        {
            /// <summary>Creates a call.</summary>
            /// <param name="name">The function's name.</param>
            /// <param name="arguments">Its argument expressions.</param>
            public Call(string name, IReadOnlyList<GuardExpression> arguments)
            {
                Name = name ?? throw new ArgumentNullException(nameof(name));
                Arguments = arguments ?? throw new ArgumentNullException(nameof(arguments));
            }

            /// <summary>The function's name.</summary>
            public string Name { get; }

            /// <summary>Its argument expressions.</summary>
            public IReadOnlyList<GuardExpression> Arguments { get; }

            /// <inheritdoc/>
            public override GuardValue Evaluate(IGuardContext context)
            {
                var values = new GuardValue[Arguments.Count];
                for (int i = 0; i < Arguments.Count; i++)
                {
                    values[i] = Arguments[i].Evaluate(context);
                }

                return context.Query(Name, values);
            }

            /// <inheritdoc/>
            public override string ToString()
            {
                return Name + "(" + string.Join(", ", Arguments) + ")";
            }
        }

        /// <summary><c>not</c>.</summary>
        public sealed class Not : GuardExpression
        {
            /// <summary>Creates a negation.</summary>
            /// <param name="operand">What to negate.</param>
            public Not(GuardExpression operand)
            {
                Operand = operand ?? throw new ArgumentNullException(nameof(operand));
            }

            /// <summary>The negated expression.</summary>
            public GuardExpression Operand { get; }

            /// <inheritdoc/>
            public override GuardValue Evaluate(IGuardContext context)
            {
                Ternary inner = Operand.Test(context);
                Ternary negated = TernaryLogic.Not(inner);
                return negated == Ternary.Unknown
                    ? GuardValue.Unknown
                    : GuardValue.FromBoolean(negated == Ternary.True);
            }

            /// <inheritdoc/>
            public override string ToString()
            {
                return "not " + Operand;
            }
        }

        /// <summary><c>and</c> / <c>or</c>.</summary>
        public sealed class Connective : GuardExpression
        {
            /// <summary>Creates a connective.</summary>
            /// <param name="isAnd">True for <c>and</c>, false for <c>or</c>.</param>
            /// <param name="left">The left operand.</param>
            /// <param name="right">The right operand.</param>
            public Connective(bool isAnd, GuardExpression left, GuardExpression right)
            {
                IsAnd = isAnd;
                Left = left ?? throw new ArgumentNullException(nameof(left));
                Right = right ?? throw new ArgumentNullException(nameof(right));
            }

            /// <summary>True for <c>and</c>, false for <c>or</c>.</summary>
            public bool IsAnd { get; }

            /// <summary>The left operand.</summary>
            public GuardExpression Left { get; }

            /// <summary>The right operand.</summary>
            public GuardExpression Right { get; }

            /// <inheritdoc/>
            public override GuardValue Evaluate(IGuardContext context)
            {
                Ternary left = Left.Test(context);
                Ternary right = Right.Test(context);
                Ternary result = IsAnd
                    ? TernaryLogic.And(left, right)
                    : TernaryLogic.Or(left, right);
                return result == Ternary.Unknown
                    ? GuardValue.Unknown
                    : GuardValue.FromBoolean(result == Ternary.True);
            }

            /// <inheritdoc/>
            public override string ToString()
            {
                return "(" + Left + (IsAnd ? " and " : " or ") + Right + ")";
            }
        }

        /// <summary>A comparison.</summary>
        public sealed class Comparison : GuardExpression
        {
            /// <summary>Creates a comparison.</summary>
            /// <param name="op">The operator, as written.</param>
            /// <param name="left">The left operand.</param>
            /// <param name="right">The right operand.</param>
            public Comparison(string op, GuardExpression left, GuardExpression right)
            {
                Operator = op ?? throw new ArgumentNullException(nameof(op));
                Left = left ?? throw new ArgumentNullException(nameof(left));
                Right = right ?? throw new ArgumentNullException(nameof(right));
            }

            /// <summary>The operator: one of <c>== ~= &gt;= &lt;= &gt; &lt;</c>.</summary>
            public string Operator { get; }

            /// <summary>The left operand.</summary>
            public GuardExpression Left { get; }

            /// <summary>The right operand.</summary>
            public GuardExpression Right { get; }

            /// <inheritdoc/>
            public override GuardValue Evaluate(IGuardContext context)
            {
                GuardValue left = Left.Evaluate(context);
                GuardValue right = Right.Evaluate(context);
                if (left.Kind == GuardValueKind.Unknown || right.Kind == GuardValueKind.Unknown)
                {
                    return GuardValue.Unknown;
                }

                if (Operator == "==" || Operator == "~=")
                {
                    bool equal = AreEqual(left, right);
                    return GuardValue.FromBoolean(Operator == "==" ? equal : !equal);
                }

                if (!left.TryAsNumber(out double a) || !right.TryAsNumber(out double b))
                {
                    // Lua raises when ordering non-numbers. We do not have a player to
                    // raise at, and guessing an order would invent behaviour, so this
                    // is exactly the case Unknown exists for.
                    return GuardValue.Unknown;
                }

                switch (Operator)
                {
                    case ">=":
                        return GuardValue.FromBoolean(a >= b);
                    case "<=":
                        return GuardValue.FromBoolean(a <= b);
                    case ">":
                        return GuardValue.FromBoolean(a > b);
                    case "<":
                        return GuardValue.FromBoolean(a < b);
                    default:
                        return GuardValue.Unknown;
                }
            }

            private static bool AreEqual(GuardValue left, GuardValue right)
            {
                if (left.Kind != right.Kind)
                {
                    // Lua's == is false across types rather than coercing, so
                    // `Variable["x"] == true` is false when x holds a number.
                    return false;
                }

                switch (left.Kind)
                {
                    case GuardValueKind.Boolean:
                        return left.Boolean == right.Boolean;
                    case GuardValueKind.Number:
                        return left.Number.Equals(right.Number);
                    case GuardValueKind.Text:
                        return string.Equals(left.Text, right.Text, StringComparison.Ordinal);
                    default:
                        return false;
                }
            }

            /// <inheritdoc/>
            public override string ToString()
            {
                return "(" + Left + " " + Operator + " " + Right + ")";
            }
        }

        /// <summary>The always-true guard, for entries with no condition at all.</summary>
        public static GuardExpression AlwaysTrue { get; } =
            new Literal(GuardValue.FromBoolean(true));

        /// <summary>Formats a number the way the parser accepts it back.</summary>
        /// <param name="value">The number.</param>
        internal static string FormatNumber(double value)
        {
            return value.ToString(CultureInfo.InvariantCulture);
        }
    }
}
