// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.LookAhead
{
    /// <summary>A truth value that admits "we cannot tell".</summary>
    /// <remarks>
    /// Guards mention things the look-ahead may not be able to resolve - a world query
    /// the host cannot answer, a skill check we decline to predict. Collapsing those to
    /// false would silently prune reachable dialogue; collapsing them to true would be
    /// right by luck rather than by design. Naming the third case keeps the choice
    /// explicit at the one place it matters, <see cref="TernaryLogic.CanPass"/>.
    /// </remarks>
    public enum Ternary
    {
        /// <summary>Definitely false.</summary>
        False = 0,

        /// <summary>Definitely true.</summary>
        True = 1,

        /// <summary>Not determinable from the state we hold.</summary>
        Unknown = 2,
    }

    /// <summary>Three-valued connectives, and the engine's rule for acting on them.</summary>
    public static class TernaryLogic
    {
        /// <summary>Three-valued negation.</summary>
        /// <param name="value">The value to negate.</param>
        public static Ternary Not(Ternary value)
        {
            switch (value)
            {
                case Ternary.True:
                    return Ternary.False;
                case Ternary.False:
                    return Ternary.True;
                default:
                    return Ternary.Unknown;
            }
        }

        /// <summary>
        /// Three-valued conjunction. False wins over Unknown, because a definitely
        /// false operand settles the result whatever the other one is.
        /// </summary>
        /// <param name="left">The left operand.</param>
        /// <param name="right">The right operand.</param>
        public static Ternary And(Ternary left, Ternary right)
        {
            if (left == Ternary.False || right == Ternary.False)
            {
                return Ternary.False;
            }

            if (left == Ternary.True && right == Ternary.True)
            {
                return Ternary.True;
            }

            return Ternary.Unknown;
        }

        /// <summary>Three-valued disjunction. True wins over Unknown, symmetrically.</summary>
        /// <param name="left">The left operand.</param>
        /// <param name="right">The right operand.</param>
        public static Ternary Or(Ternary left, Ternary right)
        {
            if (left == Ternary.True || right == Ternary.True)
            {
                return Ternary.True;
            }

            if (left == Ternary.False && right == Ternary.False)
            {
                return Ternary.False;
            }

            return Ternary.Unknown;
        }

        /// <summary>
        /// Whether a guard with this result should let the crawl through.
        /// </summary>
        /// <remarks>
        /// Only a definite <see cref="Ternary.False"/> blocks. This is the soundness
        /// rule stated once, in code: the two error directions are not symmetric. A
        /// spurious marker costs the player a wasted click; a missing one tells them a
        /// branch is exhausted when it is not, which is the whole thing the feature
        /// exists to prevent.
        /// </remarks>
        /// <param name="value">The guard's evaluation.</param>
        public static bool CanPass(Ternary value)
        {
            return value != Ternary.False;
        }
    }
}
