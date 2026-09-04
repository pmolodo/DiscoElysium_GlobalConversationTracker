// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.Core;

/// <summary>Compact text for the runs of numbers this repository's files carry.</summary>
/// <remarks>
/// <para>A grouped table names its entries by value rather than by key, so it has to say
/// separately which keys it stands for, and which keys carry each value. Those are
/// key ranges. They are strings rather than arrays so that an indented writer cannot
/// spread a few hundred numbers over a few hundred lines.</para>
///
/// <para>IN CORE, AND NOT IN THE TOOL THAT FIRST NEEDED IT. Every file this repository
/// writes that carries a dense run of integers spells it the same way, and it can only
/// stay that way if there is one implementation to be the same as. It began in
/// <c>tools/NtwtfDecode</c>, for the sparse saves; the global state file now writes its
/// entry sets with it too (format 4), and that file is written by the shipped plugin,
/// which cannot reference a tool. Moving it here costs nothing - NtwtfDecode already
/// references Persistence, which references this - and it is the difference between one
/// spelling and two that agree until they do not.</para>
///
/// <para>THE HYPHEN, AND WHY IT IS NOT THE WIRE'S <c>..</c>. The engine's own
/// <c>NodeSet</c> spells a run <c>0..40</c>, on the stated grounds that a hyphen becomes
/// ambiguous the first time a negative id appears. That reasoning does not survive this
/// implementation, which has always handled negative bounds by looking for the separator
/// past the first character - a leading <c>-</c> is a sign, not a separator. So the
/// argument for two spellings is gone, and what is left is that the hyphen is what every
/// file already uses and what was asked for. The wire has not moved yet: doing so breaks
/// the contract with the plugin, and there is a second wire change queued (de-8hh2.6)
/// that should break it in the same act rather than twice.</para>
///
/// <para>A RANGE MAY COUNT DOWN, which the wire's cannot. The saves write their dialogue
/// variables newest first, so a backwards run is as common here as a forwards one and
/// costs the same to say.</para>
/// </remarks>
public static class SparseOrder
{
    private const char GroupSeparator = ',';
    private const char RangeSeparator = '-';

    /// <summary>
    /// Renders a run of numbers as comma-separated ranges, e.g. "0-11,14-26". A
    /// range counts down when its end is below its start, which is what makes a
    /// backwards run - the save writes its dialogue variables newest first - as
    /// short as a forwards one.
    /// </summary>
    public static string PackRange(IReadOnlyList<long> numbers)
    {
        var text = new StringBuilder();
        for (int i = 0; i < numbers.Count; )
        {
            int last = i;
            long step = 0;
            if (last + 1 < numbers.Count)
            {
                long delta = numbers[last + 1] - numbers[last];
                if (delta == 1 || delta == -1)
                {
                    step = delta;
                    while (last + 1 < numbers.Count && numbers[last + 1] - numbers[last] == step)
                    {
                        last++;
                    }
                }
            }
            if (text.Length > 0)
            {
                text.Append(GroupSeparator);
            }
            text.Append(Number(numbers[i]));
            if (last > i)
            {
                text.Append(RangeSeparator).Append(Number(numbers[last]));
            }
            i = last + 1;
        }
        return text.ToString();
    }

    /// <summary>Expands what <see cref="PackRange"/> wrote.</summary>
    public static List<long> UnpackRange(string text, string context)
    {
        var keys = new List<long>();
        if (text.Length == 0)
        {
            return keys;
        }
        foreach (string part in text.Split(GroupSeparator))
        {
            // A leading '-' is a negative bound, not a separator, so look past it. This is
            // the whole of what the wire's `..` was chosen to avoid, and it is four
            // characters - see the remarks on this class.
            //
            // Substring rather than a range expression: this compiles into the shipped
            // plugin, whose target framework has no System.Range.
            int dash = part.IndexOf(RangeSeparator, 1);
            long first = ParseBound(dash < 0 ? part : part.Substring(0, dash), part, context);
            long last = ParseBound(dash < 0 ? part : part.Substring(dash + 1), part, context);
            long step = last < first ? -1 : 1;
            for (long key = first; ; key += step)
            {
                keys.Add(key);
                if (key == last)
                {
                    break;
                }
            }
        }
        return keys;
    }

    private static string Number(long value) =>
        value.ToString(CultureInfo.InvariantCulture);

    private static long ParseBound(string text, string part, string context) =>
        long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out long value)
            ? value
            : throw new InvalidDataException($"{context} has a malformed key range '{part}'");

}
