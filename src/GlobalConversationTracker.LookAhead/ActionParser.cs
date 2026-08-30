// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>
    /// Turns a dialogue entry's <c>userScript</c> into the state changes it makes.
    /// </summary>
    /// <remarks>
    /// <para>Scripts are semicolon-separated Lua calls, sometimes several per entry, and
    /// carry the same <c>--[[ Variable[ ]]</c> exporter markers the guards do. Nine
    /// call shapes are modelled, covering every mutation a guard in this database can
    /// observe:</para>
    ///
    /// <list type="bullet">
    ///   <item><description><c>SetVariableValue(name, value)</c> - assignment, or an
    ///   increment when the value reads the same variable back.</description></item>
    ///   <item><description><c>GainItem</c> / <c>LoseItem</c> - inventory, which
    ///   <c>CheckItem</c> and <c>CheckEquipped</c> read.</description></item>
    ///   <item><description><c>GainTask</c> / <c>FinishTask</c> / <c>CancelTask</c> -
    ///   tasks, which <c>IsTaskActive</c> reads.</description></item>
    ///   <item><description><c>GainMoneyOnce</c> / <c>GainMoneyAlways</c> /
    ///   <c>LoseMoneyOnce</c> / <c>LoseMoneyAlways</c> - the balance, which
    ///   <c>MoneyAmount</c> reads and every cost option spends.</description></item>
    ///   <item><description><c>PassTime</c> - the clock, which the hour predicates
    ///   read.</description></item>
    /// </list>
    ///
    /// <para>Anything else is recorded as unmodelled rather than dropped.</para>
    /// </remarks>
    public static class ActionParser
    {
        /// <summary>The variable value idiom meaning "add n, but only the first time".</summary>
        private const string OnceFunction = "once";

        /// <summary>Parses a script into actions.</summary>
        /// <param name="script">The entry's <c>userScript</c>.</param>
        /// <param name="symbols">The table slots are interned into.</param>
        /// <returns>The actions, in source order. Empty for an empty script.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="symbols"/> is null.</exception>
        public static IReadOnlyList<DialogueAction> Parse(string? script, StateSymbols symbols)
        {
            if (symbols == null)
            {
                throw new ArgumentNullException(nameof(symbols));
            }

            var actions = new List<DialogueAction>();
            if (string.IsNullOrWhiteSpace(script))
            {
                return actions;
            }

            foreach (Invocation call in Invocations(StripComments(script!)))
            {
                Translate(call, symbols, actions);
            }

            return actions;
        }

        private static void Translate(
            Invocation call, StateSymbols symbols, List<DialogueAction> actions)
        {
            switch (call.Name)
            {
                case "SetVariableValue":
                {
                    if (call.Arguments.Count < 2)
                    {
                        actions.Add(DialogueAction.Unmodelled(call.Name));
                        return;
                    }

                    string name = Unquote(call.Arguments[0]);
                    int slot = symbols.Variable(name);
                    string value = call.Arguments[1].Trim();
                    if (TryReadIncrement(value, name, out int amount, out bool once))
                    {
                        actions.Add(DialogueAction.Increment(slot, amount, once, call.Name));
                        return;
                    }

                    actions.Add(DialogueAction.Assign(slot, ReadAssignedValue(value), call.Name));
                    return;
                }

                case "GainItem":
                    actions.Add(DialogueAction.Assign(
                        symbols.Item(Unquote(call.FirstArgument)), 1, call.Name));
                    return;

                case "LoseItem":
                    actions.Add(DialogueAction.Assign(
                        symbols.Item(Unquote(call.FirstArgument)), 0, call.Name));
                    return;

                case "GainTask":
                    actions.Add(DialogueAction.Assign(
                        symbols.Task(Unquote(call.FirstArgument)), 1, call.Name));
                    return;

                case "FinishTask":
                case "CancelTask":
                    // Both end the task, and IsTaskActive is the only guard that reads
                    // one, so both clear the slot. A model that distinguished finished
                    // from cancelled would need a guard that can tell them apart, and
                    // none exists in this database.
                    actions.Add(DialogueAction.Assign(
                        symbols.Task(Unquote(call.FirstArgument)), 0, call.Name));
                    return;

                case "GainMoneyOnce":
                case "GainMoneyAlways":
                case "LoseMoneyOnce":
                case "LoseMoneyAlways":
                {
                    bool gain = call.Name.StartsWith("Gain", StringComparison.Ordinal);
                    bool once = call.Name.EndsWith("Once", StringComparison.Ordinal);
                    if (!TryReadInt(call.FirstArgument, out int amount))
                    {
                        actions.Add(DialogueAction.Unmodelled(call.Name));
                        return;
                    }

                    actions.Add(DialogueAction.Money(gain, amount, once, call.Name));
                    return;
                }

                case "PassTime":
                    actions.Add(DialogueAction.PassTime(call.Name));
                    return;

                default:
                    actions.Add(DialogueAction.Unmodelled(call.Name));
                    return;
            }
        }

        /// <summary>
        /// Recognises <c>Variable["x"] + once(2)</c> and <c>Variable["x"] + 2</c>, the
        /// two forms a counter increment is written in.
        /// </summary>
        private static bool TryReadIncrement(
            string value, string variable, out int amount, out bool once)
        {
            amount = 0;
            once = false;

            string self = "Variable[\"" + variable + "\"]";
            int at = value.IndexOf(self, StringComparison.Ordinal);
            if (at < 0)
            {
                return false;
            }

            string rest = value.Substring(at + self.Length).Trim();
            if (!rest.StartsWith("+", StringComparison.Ordinal))
            {
                return false;
            }

            rest = rest.Substring(1).Trim();
            if (rest.StartsWith(OnceFunction, StringComparison.Ordinal))
            {
                int open = rest.IndexOf('(');
                int close = rest.LastIndexOf(')');
                if (open < 0 || close < open)
                {
                    return false;
                }

                once = true;
                rest = rest.Substring(open + 1, close - open - 1);
            }

            return TryReadInt(rest, out amount);
        }

        private static int ReadAssignedValue(string value)
        {
            string text = value.Trim();
            if (string.Equals(text, "true", StringComparison.Ordinal))
            {
                return 1;
            }

            if (string.Equals(text, "false", StringComparison.Ordinal))
            {
                return 0;
            }

            return TryReadInt(text, out int number) ? number : 1;
        }

        private static bool TryReadInt(string text, out int value)
        {
            return int.TryParse(
                text.Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out value);
        }

        private static string Unquote(string text)
        {
            string trimmed = text.Trim();
            if (trimmed.Length >= 2 && trimmed[0] == '"' && trimmed[trimmed.Length - 1] == '"')
            {
                return trimmed.Substring(1, trimmed.Length - 2);
            }

            return trimmed;
        }

        private static string StripComments(string text)
        {
            var builder = new StringBuilder(text.Length);
            int index = 0;
            while (index < text.Length)
            {
                if (index + 3 < text.Length && text[index] == '-' && text[index + 1] == '-'
                    && text[index + 2] == '[' && text[index + 3] == '[')
                {
                    int close = text.IndexOf("]]", index + 4, StringComparison.Ordinal);
                    index = close < 0 ? text.Length : close + 2;
                    builder.Append(' ');
                    continue;
                }

                builder.Append(text[index]);
                index++;
            }

            return builder.ToString();
        }

        private readonly struct Invocation
        {
            public Invocation(string name, List<string> arguments)
            {
                Name = name;
                Arguments = arguments;
            }

            public string Name { get; }

            public List<string> Arguments { get; }

            public string FirstArgument => Arguments.Count > 0 ? Arguments[0] : string.Empty;
        }

        /// <summary>
        /// Splits a script into calls. Hand-rolled rather than regex because arguments
        /// nest - <c>SetVariableValue("x", Variable["x"] + once(1))</c> - and a regex
        /// that balances parentheses is worse to read than a scanner that counts them.
        /// </summary>
        private static IEnumerable<Invocation> Invocations(string script)
        {
            int index = 0;
            while (index < script.Length)
            {
                while (index < script.Length && !IsNameStart(script[index]))
                {
                    index++;
                }

                if (index >= script.Length)
                {
                    yield break;
                }

                int nameStart = index;
                while (index < script.Length && IsNamePart(script[index]))
                {
                    index++;
                }

                string name = script.Substring(nameStart, index - nameStart);

                while (index < script.Length && char.IsWhiteSpace(script[index]))
                {
                    index++;
                }

                if (index >= script.Length || script[index] != '(')
                {
                    continue;
                }

                var arguments = new List<string>();
                var current = new StringBuilder();
                int depth = 0;
                bool inString = false;
                for (; index < script.Length; index++)
                {
                    char c = script[index];
                    if (inString)
                    {
                        current.Append(c);
                        if (c == '"')
                        {
                            inString = false;
                        }

                        continue;
                    }

                    if (c == '"')
                    {
                        inString = true;
                        current.Append(c);
                        continue;
                    }

                    if (c == '(')
                    {
                        depth++;
                        if (depth == 1)
                        {
                            continue;
                        }
                    }
                    else if (c == ')')
                    {
                        depth--;
                        if (depth == 0)
                        {
                            index++;
                            break;
                        }
                    }
                    else if (c == ',' && depth == 1)
                    {
                        arguments.Add(current.ToString());
                        current.Clear();
                        continue;
                    }

                    current.Append(c);
                }

                if (current.Length > 0)
                {
                    arguments.Add(current.ToString());
                }

                yield return new Invocation(name, arguments);
            }
        }

        private static bool IsNameStart(char c)
        {
            return char.IsLetter(c) || c == '_';
        }

        private static bool IsNamePart(char c)
        {
            return char.IsLetterOrDigit(c) || c == '_';
        }
    }
}
