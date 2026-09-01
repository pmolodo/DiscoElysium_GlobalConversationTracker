// SPDX-License-Identifier: MIT
using System.Globalization;
using System.Text;

namespace NtwtfDecode;

/// <summary>
/// The bookkeeping that lets a regrouped table be put back in its original order.
/// </summary>
/// <remarks>
/// A grouped table names its entries by value rather than by key, which loses both
/// which keys were present and what order they came in. Those two facts are stored
/// back as a key range and a short list of moves. Neither is pleasant to read, but
/// they exist only to make the round trip exact - the readable part of a grouped
/// table is the grouping itself. Both are strings rather than arrays so that an
/// indented writer cannot spread a few hundred numbers over a few hundred lines.
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
    /// The fewest lift-and-reinsert moves that turn <paramref name="canonical"/> into
    /// <paramref name="observed"/>. Each is (from, to): the item at index
    /// <c>from</c> of the canonical order belongs at index <c>to</c> of the observed
    /// one.
    /// </summary>
    /// <remarks>
    /// Everything left alone has to stay in its canonical relative order, so the
    /// items to leave alone are a longest increasing subsequence of where the
    /// observed items sit in the canonical order, and the moves are the rest. Doing
    /// it any more simply - walking left to right and swapping whatever is out of
    /// place - turns one displaced entry into a move for every entry after it,
    /// which is what a real dialogue map looks like: ascending, with key 0 sitting
    /// somewhere in the middle.
    /// </remarks>
    public static List<(int From, int To)> Moves<T>(
        IReadOnlyList<T> canonical,
        IReadOnlyList<T> observed
    )
        where T : notnull
    {
        var canonicalIndex = new Dictionary<T, int>(canonical.Count);
        for (int i = 0; i < canonical.Count; i++)
        {
            canonicalIndex[canonical[i]] = i;
        }

        var where = new int[observed.Count];
        for (int i = 0; i < observed.Count; i++)
        {
            if (!canonicalIndex.TryGetValue(observed[i], out where[i]))
            {
                throw new InvalidDataException(
                    "The observed order is not a rearrangement of the canonical one"
                );
            }
        }

        bool[] keep = LongestIncreasingRun(where);
        var moves = new List<(int, int)>();
        for (int to = 0; to < where.Length; to++)
        {
            if (!keep[to])
            {
                moves.Add((where[to], to));
            }
        }
        return moves;
    }

    /// <summary>Replays what <see cref="Moves{T}"/> recorded.</summary>
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

    /// <summary>Renders moves as "from:to" pairs, e.g. "30:1187,44:900".</summary>
    public static string PackMoves(IReadOnlyList<(int From, int To)> moves)
    {
        var text = new StringBuilder();
        foreach ((int from, int to) in moves)
        {
            if (text.Length > 0)
            {
                text.Append(GroupSeparator);
            }
            text.Append(Number(from)).Append(MoveSeparator).Append(Number(to));
        }
        return text.ToString();
    }

    /// <summary>Reads back what <see cref="PackMoves"/> wrote.</summary>
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

    /// <summary>
    /// Which positions belong to a longest run whose canonical indices only ever
    /// increase - the entries that can stay where the canonical order put them.
    /// </summary>
    private static bool[] LongestIncreasingRun(int[] where)
    {
        // Patience sorting: tails[k] is the position ending the best run of k+1.
        var tails = new List<int>();
        var previous = new int[where.Length];
        for (int i = 0; i < where.Length; i++)
        {
            previous[i] = -1;
            int low = 0;
            int high = tails.Count;
            while (low < high)
            {
                int mid = (low + high) / 2;
                if (where[tails[mid]] < where[i])
                {
                    low = mid + 1;
                }
                else
                {
                    high = mid;
                }
            }
            if (low > 0)
            {
                previous[i] = tails[low - 1];
            }
            if (low == tails.Count)
            {
                tails.Add(i);
            }
            else
            {
                tails[low] = i;
            }
        }

        var keep = new bool[where.Length];
        for (int i = tails.Count > 0 ? tails[^1] : -1; i >= 0; i = previous[i])
        {
            keep[i] = true;
        }
        return keep;
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
