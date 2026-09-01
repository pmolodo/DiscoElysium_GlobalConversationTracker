// SPDX-License-Identifier: MIT
using System.Globalization;
using System.Text;

namespace NtwtfDecode;

/// <summary>Compact text for the runs of numbers the sparse form carries.</summary>
/// <remarks>
/// A grouped table names its entries by value rather than by key, so it has to say
/// separately which keys it stands for; that is a key range. The other user is the
/// dialogue order inside a SimX string. Both are strings rather than arrays so that
/// an indented writer cannot spread a few hundred numbers over a few hundred lines.
/// </remarks>
public static class SparseOrder
{
    private const char GroupSeparator = ',';
    private const char RangeSeparator = '-';
    private const char MoveSeparator = ':';

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

    /// <summary>
    /// Replays a recorded list of lift-and-reinsert moves. Each is (from, to): the
    /// item at index <c>from</c> of the canonical order belongs at index <c>to</c>.
    /// Only <see cref="SimXOrders"/> uses this now, for the dialogue order inside a
    /// SimX string; the moves themselves are computed by generate_simx_order.py.
    /// </summary>
    public static List<T> ApplyMoves<T>(
        IReadOnlyList<T> canonical,
        IReadOnlyList<(int From, int To)> moves
    )
    {
        var lifted = new List<(int From, int To)>(moves);
        // Lifting from the back first keeps the earlier indices meaning what they
        // meant in the canonical order.
        var byFrom = new List<(int From, int To)>(lifted);
        byFrom.Sort((a, b) => b.From.CompareTo(a.From));
        var rest = new List<T>(canonical);
        var items = new Dictionary<int, T>(byFrom.Count);
        foreach ((int from, int _) in byFrom)
        {
            if (from < 0 || from >= rest.Count)
            {
                throw new InvalidDataException($"Reorder source {from} is out of range");
            }
            items[from] = rest[from];
            rest.RemoveAt(from);
        }

        lifted.Sort((a, b) => a.To.CompareTo(b.To));
        foreach ((int from, int to) in lifted)
        {
            if (to < 0 || to > rest.Count)
            {
                throw new InvalidDataException($"Reorder target {to} is out of range");
            }
            rest.Insert(to, items[from]);
        }
        return rest;
    }

    /// <summary>Reads "from:to" pairs, as generate_simx_order.py writes them.</summary>
    public static List<(int From, int To)> UnpackMoves(object? node, string context)
    {
        var moves = new List<(int, int)>();
        if (node is null)
        {
            return moves;
        }
        if (node is not string text)
        {
            throw new InvalidDataException($"{context} reorder must be a string");
        }
        if (text.Length == 0)
        {
            return moves;
        }
        foreach (string part in text.Split(GroupSeparator))
        {
            int colon = part.IndexOf(MoveSeparator);
            if (colon < 0)
            {
                throw new InvalidDataException($"{context} reorder '{part}' is not 'from:to'");
            }
            moves.Add((
                ParseIndex(part[..colon], part, context),
                ParseIndex(part[(colon + 1)..], part, context)
            ));
        }
        return moves;
    }

    private static string Number(long value) =>
        value.ToString(CultureInfo.InvariantCulture);

    private static long ParseBound(string text, string part, string context) =>
        long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out long value)
            ? value
            : throw new InvalidDataException($"{context} has a malformed key range '{part}'");

    private static int ParseIndex(string text, string part, string context) =>
        int.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out int value)
        && value >= 0
            ? value
            : throw new InvalidDataException($"{context} reorder '{part}' has a bad index");
}
