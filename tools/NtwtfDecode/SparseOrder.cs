// SPDX-License-Identifier: MIT
using System.Globalization;
using System.Text;

namespace NtwtfDecode;

/// <summary>Compact text for the runs of numbers the sparse form carries.</summary>
/// <remarks>
/// A grouped table names its entries by value rather than by key, so it has to say
/// separately which keys it stands for, and which keys carry each value. Those are
/// key ranges. They are strings rather than arrays so that an indented writer cannot
/// spread a few hundred numbers over a few hundred lines.
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
            // A leading '-' is a negative bound, not a separator, so look past it.
            int dash = part.IndexOf(RangeSeparator, 1);
            long first = ParseBound(dash < 0 ? part : part[..dash], part, context);
            long last = ParseBound(dash < 0 ? part : part[(dash + 1)..], part, context);
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
