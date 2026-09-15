// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;

namespace GlobalConversationTracker.Harness
{
    /// <summary>
    /// One thing a scenario presses on its way from a conversation's start to its menu.
    /// </summary>
    /// <remarks>
    /// <para>THE SAME INPUTS <c>src/walkthrough.rs</c> follows offline, spelled the same way,
    /// so one row is walked the same way by both executors. "enter" advances a waiting line
    /// or takes the only option of a menu of one; a number chooses that option, counting from
    /// 1 in the order the game draws them, off a menu of several.</para>
    ///
    /// <para>A line with a menu behind it does not wait - the menu comes up beside it - and a
    /// chosen option never does, so "enter" is only ever pressed at a line that is still
    /// asking once the interface has settled.</para>
    /// </remarks>
    public readonly struct ScenarioInput : IEquatable<ScenarioInput>
    {
        /// <summary>How "enter" is spelled in a scenario.</summary>
        public const string EnterText = "enter";

        private ScenarioInput(int number)
        {
            Number = number;
        }

        /// <summary>Advances a waiting line, or takes the only option of a menu of one.</summary>
        public static ScenarioInput Enter => new ScenarioInput(0);

        /// <summary>Whether this is "enter" rather than an option's number.</summary>
        public bool IsEnter => Number == 0;

        /// <summary>The option chosen, counting from 1 in drawn order; 0 for "enter".</summary>
        public int Number { get; }

        /// <summary>Chooses the N-th option of a menu of several.</summary>
        /// <param name="number">The option, counting from 1 in drawn order.</param>
        /// <returns>The input.</returns>
        /// <exception cref="ArgumentOutOfRangeException">The number is below 1.</exception>
        public static ScenarioInput Choose(int number) =>
            number >= 1
                ? new ScenarioInput(number)
                : throw new ArgumentOutOfRangeException(
                    nameof(number), number, "Options are counted from 1.");

        /// <summary>Reads one input as a scenario spells it.</summary>
        /// <param name="text">"enter", or an option's number counting from 1.</param>
        /// <returns>The input.</returns>
        /// <exception cref="FormatException">It is neither.</exception>
        public static ScenarioInput Parse(string text)
        {
            if (text == EnterText)
            {
                return Enter;
            }

            if (int.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out int number)
                && number >= 1)
            {
                return Choose(number);
            }

            throw new FormatException(
                $"\"{text}\" is not an input: an input is \"{EnterText}\" or an option's number, "
                + "counting from 1.");
        }

        /// <summary>Reads a scenario's inputs, or null where it names none.</summary>
        /// <param name="texts">The inputs as spelled, or null.</param>
        /// <returns>The inputs, or null.</returns>
        /// <exception cref="FormatException">One is not an input.</exception>
        public static IReadOnlyList<ScenarioInput>? ParseAll(IEnumerable<string>? texts) =>
            texts?.Select(Parse).ToArray();

        /// <inheritdoc/>
        public bool Equals(ScenarioInput other) => Number == other.Number;

        /// <inheritdoc/>
        public override bool Equals(object? obj) => obj is ScenarioInput other && Equals(other);

        /// <inheritdoc/>
        public override int GetHashCode() => Number;

        /// <summary>The input as a scenario spells it.</summary>
        public override string ToString() =>
            IsEnter ? EnterText : Number.ToString(CultureInfo.InvariantCulture);
    }
}
