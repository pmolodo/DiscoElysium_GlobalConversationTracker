using System.Globalization;
using System.Numerics;
using System.Text;

namespace NtwtfDecode;

/// <summary>
/// Writes decoded Lua data as JSON, byte-for-byte compatible with Python's
/// json.dumps(..., indent=N, ensure_ascii=False).
///
/// Matching Python exactly matters because the reference decode of the example
/// save was produced by the Python tool this one replaces; identical output is
/// what lets the two be diffed directly.
/// </summary>
public static class PythonJson
{
    /// <summary>Writes <paramref name="value"/> as JSON, with no trailing newline.</summary>
    public static void Write(TextWriter writer, object? value, int? indent)
    {
        WriteValue(writer, value, indent, 0);
    }

    private static void WriteValue(TextWriter writer, object? value, int? indent, int depth)
    {
        switch (value)
        {
            case null:
                writer.Write("null");
                break;
            case LuaTable table:
                WriteTable(writer, table, indent, depth);
                break;
            case string s:
                WriteString(writer, s);
                break;
            case bool b:
                writer.Write(b ? "true" : "false");
                break;
            case long l:
                writer.Write(l.ToString(CultureInfo.InvariantCulture));
                break;
            case BigInteger bi:
                writer.Write(bi.ToString(CultureInfo.InvariantCulture));
                break;
            case double d:
                writer.Write(FormatDouble(d));
                break;
            default:
                throw new InvalidDataException($"Unsupported value type {value.GetType().Name}");
        }
    }

    private static void WriteTable(TextWriter writer, LuaTable table, int? indent, int depth)
    {
        if (table.Count == 0)
        {
            writer.Write("{}");
            return;
        }

        string? childIndent = indent is null ? null : new string(' ', indent.Value * (depth + 1));
        string? closeIndent = indent is null ? null : new string(' ', indent.Value * depth);

        writer.Write('{');
        bool first = true;
        foreach (var entry in table.Entries)
        {
            if (!first)
            {
                writer.Write(',');
            }
            if (childIndent is not null)
            {
                writer.Write('\n');
                writer.Write(childIndent);
            }
            else if (!first)
            {
                // Python's default separator when no indent is given is ", ".
                writer.Write(' ');
            }
            first = false;
            WriteString(writer, LuaKey.ToKeyString(entry.Key));
            writer.Write(": ");
            WriteValue(writer, entry.Value, indent, depth + 1);
        }
        if (closeIndent is not null)
        {
            writer.Write('\n');
            writer.Write(closeIndent);
        }
        writer.Write('}');
    }

    /// <summary>
    /// Escapes exactly what Python's json escapes with ensure_ascii=False:
    /// the quote, the backslash, and control characters below 0x20.
    /// </summary>
    private static void WriteString(TextWriter writer, string value)
    {
        writer.Write('"');
        foreach (char c in value)
        {
            switch (c)
            {
                case '"':
                    writer.Write("\\\"");
                    break;
                case '\\':
                    writer.Write("\\\\");
                    break;
                case '\b':
                    writer.Write("\\b");
                    break;
                case '\f':
                    writer.Write("\\f");
                    break;
                case '\n':
                    writer.Write("\\n");
                    break;
                case '\r':
                    writer.Write("\\r");
                    break;
                case '\t':
                    writer.Write("\\t");
                    break;
                default:
                    if (c < 0x20)
                    {
                        writer.Write("\\u");
                        writer.Write(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                    }
                    else
                    {
                        writer.Write(c);
                    }
                    break;
            }
        }
        writer.Write('"');
    }

    /// <summary>
    /// Formats a double the way Python's repr() does, which is what json.dumps
    /// uses for floats.
    ///
    /// Both .NET and CPython produce the shortest round-tripping digit string,
    /// but they disagree on presentation: .NET switches to exponent notation at
    /// different magnitudes and uses 'E'. CPython uses fixed notation while the
    /// decimal point position is in (-4, 16], always keeps at least one
    /// fractional digit, and writes the exponent as 'e+NN' / 'e-NN'.
    /// </summary>
    public static string FormatDouble(double value)
    {
        if (double.IsNaN(value))
        {
            return "NaN";
        }
        if (double.IsPositiveInfinity(value))
        {
            return "Infinity";
        }
        if (double.IsNegativeInfinity(value))
        {
            return "-Infinity";
        }

        // .NET's "R" is the shortest round-trippable form on .NET Core 3.0+.
        string repr = value.ToString("R", CultureInfo.InvariantCulture);

        bool negative = false;
        int i = 0;
        if (repr[0] == '-')
        {
            negative = true;
            i = 1;
        }

        // Split into digits and a decimal-point position, so that the value is
        // 0.<digits> * 10^decpt.
        var digits = new StringBuilder();
        int decpt = 0;
        bool seenPoint = false;
        for (; i < repr.Length; i++)
        {
            char c = repr[i];
            if (c == '.')
            {
                seenPoint = true;
                continue;
            }
            if (c is 'E' or 'e')
            {
                decpt += int.Parse(repr.AsSpan(i + 1), CultureInfo.InvariantCulture);
                break;
            }
            digits.Append(c);
            if (!seenPoint)
            {
                decpt++;
            }
        }

        // Drop leading zeros, which shift the decimal point.
        int lead = 0;
        while (lead < digits.Length - 1 && digits[lead] == '0')
        {
            lead++;
            decpt--;
        }
        digits.Remove(0, lead);
        // Drop trailing zeros; they carry no information in the digit string.
        int end = digits.Length;
        while (end > 1 && digits[end - 1] == '0')
        {
            end--;
        }
        digits.Length = end;

        string ds = digits.ToString();
        if (ds == "0")
        {
            return negative ? "-0.0" : "0.0";
        }

        var sb = new StringBuilder();
        if (negative)
        {
            sb.Append('-');
        }

        // CPython's repr uses exponent notation when decpt <= -4 or decpt > 16.
        if (decpt <= -4 || decpt > 16)
        {
            sb.Append(ds[0]);
            if (ds.Length > 1)
            {
                sb.Append('.').Append(ds, 1, ds.Length - 1);
            }
            int exponent = decpt - 1;
            sb.Append('e').Append(exponent < 0 ? '-' : '+');
            sb.Append(Math.Abs(exponent).ToString("D2", CultureInfo.InvariantCulture));
        }
        else if (decpt <= 0)
        {
            sb.Append("0.").Append('0', -decpt).Append(ds);
        }
        else if (decpt >= ds.Length)
        {
            sb.Append(ds).Append('0', decpt - ds.Length).Append(".0");
        }
        else
        {
            sb.Append(ds, 0, decpt).Append('.').Append(ds, decpt, ds.Length - decpt);
        }
        return sb.ToString();
    }
}
