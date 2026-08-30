// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>Raised when a guard uses something outside the recognised grammar.</summary>
    /// <remarks>
    /// Thrown rather than swallowed. A guard we cannot parse is a guard we cannot
    /// reason about, and the caller has to decide what that means - the engine's answer
    /// is to treat the entry as reachable, but that is a decision worth making at the
    /// call site rather than hiding behind a silent null.
    /// </remarks>
    public sealed class GuardParseException : Exception
    {
        /// <summary>Creates the exception.</summary>
        /// <param name="message">What went wrong.</param>
        /// <param name="source">The guard text that failed.</param>
        public GuardParseException(string message, string source)
            : base(message + " in: " + source)
        {
            GuardText = source;
        }

        /// <summary>The guard text that failed to parse.</summary>
        public string GuardText { get; }
    }

    /// <summary>
    /// Parses the Lua subset the game's <c>conditionsString</c> fields are written in.
    /// </summary>
    /// <remarks>
    /// <para>Recursive descent over a deliberately small grammar. Validated against the
    /// shipped Final Cut database: all 26,210 non-empty guards parse, spanning 986
    /// distinct shapes and 49 distinct world-query functions.</para>
    ///
    /// <para>Lua block comments are stripped first. They are not decoration - the
    /// exporter emits markers like <c>IsTaskActive("x")--[[ Variable[ ]]</c>, and the
    /// unbalanced <c>Variable[</c> inside would otherwise derail the tokenizer.</para>
    /// </remarks>
    public static class GuardParser
    {
        /// <summary>
        /// Parses a guard. An empty or whitespace-only condition means "no condition",
        /// which the game treats as true.
        /// </summary>
        /// <param name="text">The <c>conditionsString</c> to parse.</param>
        /// <exception cref="GuardParseException">The text is not in the grammar.</exception>
        public static GuardExpression Parse(string? text)
        {
            if (string.IsNullOrWhiteSpace(text))
            {
                return GuardExpression.AlwaysTrue;
            }

            string stripped = StripComments(text!);
            if (string.IsNullOrWhiteSpace(stripped))
            {
                return GuardExpression.AlwaysTrue;
            }

            List<Token> tokens = Tokenize(stripped, text!);
            var parser = new Cursor(tokens, text!);
            GuardExpression expression = parser.ParseExpression();
            parser.ExpectEnd();
            return expression;
        }

        /// <summary>
        /// Parses a guard, returning false rather than throwing when it will not parse.
        /// </summary>
        /// <param name="text">The <c>conditionsString</c> to parse.</param>
        /// <param name="expression">The parsed guard, when this returns true.</param>
        public static bool TryParse(string? text, out GuardExpression expression)
        {
            try
            {
                expression = Parse(text);
                return true;
            }
            catch (GuardParseException)
            {
                expression = GuardExpression.AlwaysTrue;
                return false;
            }
        }

        /// <summary>Removes Lua block and line comments.</summary>
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

                if (index + 1 < text.Length && text[index] == '-' && text[index + 1] == '-')
                {
                    while (index < text.Length && text[index] != '\n')
                    {
                        index++;
                    }

                    continue;
                }

                builder.Append(text[index]);
                index++;
            }

            return builder.ToString();
        }

        private enum TokenKind
        {
            Variable,
            Number,
            Text,
            Operator,
            Name,
            And,
            Or,
            Not,
            True,
            False,
            Nil,
            OpenParen,
            CloseParen,
            Comma,
        }

        private readonly struct Token
        {
            public Token(TokenKind kind, string value)
            {
                Kind = kind;
                Value = value;
            }

            public TokenKind Kind { get; }

            public string Value { get; }
        }

        private static List<Token> Tokenize(string text, string original)
        {
            var tokens = new List<Token>();
            int index = 0;
            while (index < text.Length)
            {
                char current = text[index];
                if (char.IsWhiteSpace(current))
                {
                    index++;
                    continue;
                }

                if (current == '(')
                {
                    tokens.Add(new Token(TokenKind.OpenParen, "("));
                    index++;
                    continue;
                }

                if (current == ')')
                {
                    tokens.Add(new Token(TokenKind.CloseParen, ")"));
                    index++;
                    continue;
                }

                if (current == ',')
                {
                    tokens.Add(new Token(TokenKind.Comma, ","));
                    index++;
                    continue;
                }

                if (current == '"')
                {
                    int close = text.IndexOf('"', index + 1);
                    if (close < 0)
                    {
                        throw new GuardParseException("unterminated string", original);
                    }

                    tokens.Add(new Token(TokenKind.Text, text.Substring(index + 1, close - index - 1)));
                    index = close + 1;
                    continue;
                }

                if (char.IsDigit(current))
                {
                    int start = index;
                    while (index < text.Length && (char.IsDigit(text[index]) || text[index] == '.'))
                    {
                        index++;
                    }

                    tokens.Add(new Token(TokenKind.Number, text.Substring(start, index - start)));
                    continue;
                }

                if (TryReadOperator(text, index, out string op))
                {
                    tokens.Add(new Token(TokenKind.Operator, op));
                    index += op.Length;
                    continue;
                }

                if (char.IsLetter(current) || current == '_')
                {
                    int start = index;
                    while (index < text.Length
                        && (char.IsLetterOrDigit(text[index]) || text[index] == '_' || text[index] == '.'))
                    {
                        index++;
                    }

                    string word = text.Substring(start, index - start);
                    if (word == "Variable" && TryReadVariableIndex(text, ref index, out string name))
                    {
                        tokens.Add(new Token(TokenKind.Variable, name));
                        continue;
                    }

                    tokens.Add(new Token(KeywordKind(word), word));
                    continue;
                }

                throw new GuardParseException($"unexpected character '{current}'", original);
            }

            return tokens;
        }

        private static bool TryReadOperator(string text, int index, out string op)
        {
            if (index + 1 < text.Length)
            {
                string two = text.Substring(index, 2);
                if (two == "==" || two == "~=" || two == ">=" || two == "<=")
                {
                    op = two;
                    return true;
                }
            }

            char current = text[index];
            if (current == '>' || current == '<')
            {
                op = current.ToString();
                return true;
            }

            op = string.Empty;
            return false;
        }

        /// <summary>Reads the <c>["name"]</c> that follows the word <c>Variable</c>.</summary>
        private static bool TryReadVariableIndex(string text, ref int index, out string name)
        {
            int scan = index;
            while (scan < text.Length && char.IsWhiteSpace(text[scan]))
            {
                scan++;
            }

            if (scan >= text.Length || text[scan] != '[')
            {
                name = string.Empty;
                return false;
            }

            scan++;
            while (scan < text.Length && char.IsWhiteSpace(text[scan]))
            {
                scan++;
            }

            if (scan >= text.Length || text[scan] != '"')
            {
                name = string.Empty;
                return false;
            }

            int close = text.IndexOf('"', scan + 1);
            if (close < 0)
            {
                name = string.Empty;
                return false;
            }

            name = text.Substring(scan + 1, close - scan - 1);
            scan = close + 1;
            while (scan < text.Length && char.IsWhiteSpace(text[scan]))
            {
                scan++;
            }

            if (scan >= text.Length || text[scan] != ']')
            {
                return false;
            }

            index = scan + 1;
            return true;
        }

        private static TokenKind KeywordKind(string word)
        {
            switch (word)
            {
                case "and":
                    return TokenKind.And;
                case "or":
                    return TokenKind.Or;
                case "not":
                    return TokenKind.Not;
                case "true":
                    return TokenKind.True;
                case "false":
                    return TokenKind.False;
                case "nil":
                    return TokenKind.Nil;
                default:
                    return TokenKind.Name;
            }
        }

        private sealed class Cursor
        {
            private readonly List<Token> _tokens;
            private readonly string _source;
            private int _position;

            public Cursor(List<Token> tokens, string source)
            {
                _tokens = tokens;
                _source = source;
            }

            public void ExpectEnd()
            {
                if (_position != _tokens.Count)
                {
                    throw new GuardParseException("trailing tokens", _source);
                }
            }

            public GuardExpression ParseExpression()
            {
                GuardExpression node = ParseConjunction();
                while (Peek() == TokenKind.Or)
                {
                    Take();
                    node = new GuardExpression.Connective(false, node, ParseConjunction());
                }

                return node;
            }

            private GuardExpression ParseConjunction()
            {
                GuardExpression node = ParseComparison();
                while (Peek() == TokenKind.And)
                {
                    Take();
                    node = new GuardExpression.Connective(true, node, ParseComparison());
                }

                return node;
            }

            private GuardExpression ParseComparison()
            {
                GuardExpression left = ParseUnary();
                if (Peek() == TokenKind.Operator)
                {
                    string op = Take().Value;
                    return new GuardExpression.Comparison(op, left, ParseUnary());
                }

                return left;
            }

            private GuardExpression ParseUnary()
            {
                if (Peek() == TokenKind.Not)
                {
                    Take();
                    return new GuardExpression.Not(ParseUnary());
                }

                return ParsePrimary();
            }

            private GuardExpression ParsePrimary()
            {
                TokenKind kind = Peek();
                switch (kind)
                {
                    case TokenKind.OpenParen:
                    {
                        Take();
                        GuardExpression inner = ParseExpression();
                        Expect(TokenKind.CloseParen);
                        return inner;
                    }

                    case TokenKind.Variable:
                        return new GuardExpression.Variable(Take().Value);

                    case TokenKind.True:
                        Take();
                        return new GuardExpression.Literal(GuardValue.FromBoolean(true));

                    case TokenKind.False:
                        Take();
                        return new GuardExpression.Literal(GuardValue.FromBoolean(false));

                    case TokenKind.Nil:
                        Take();
                        return new GuardExpression.Literal(GuardValue.Unknown);

                    case TokenKind.Number:
                    {
                        string raw = Take().Value;
                        if (!double.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture,
                                out double number))
                        {
                            throw new GuardParseException($"bad number '{raw}'", _source);
                        }

                        return new GuardExpression.Literal(GuardValue.FromNumber(number));
                    }

                    case TokenKind.Text:
                        return new GuardExpression.Literal(GuardValue.FromText(Take().Value));

                    case TokenKind.Name:
                    {
                        string name = Take().Value;
                        var arguments = new List<GuardExpression>();
                        if (Peek() == TokenKind.OpenParen)
                        {
                            Take();
                            while (Peek() != TokenKind.CloseParen)
                            {
                                arguments.Add(ParseExpression());
                                if (Peek() == TokenKind.Comma)
                                {
                                    Take();
                                }
                                else if (Peek() != TokenKind.CloseParen)
                                {
                                    throw new GuardParseException("bad argument list", _source);
                                }
                            }

                            Expect(TokenKind.CloseParen);
                        }

                        return new GuardExpression.Call(name, arguments);
                    }

                    default:
                        throw new GuardParseException("unexpected token", _source);
                }
            }

            private TokenKind Peek()
            {
                return _position < _tokens.Count ? _tokens[_position].Kind : (TokenKind)(-1);
            }

            private Token Take()
            {
                if (_position >= _tokens.Count)
                {
                    throw new GuardParseException("unexpected end of guard", _source);
                }

                return _tokens[_position++];
            }

            private void Expect(TokenKind kind)
            {
                if (Peek() != kind)
                {
                    throw new GuardParseException($"expected {kind}", _source);
                }

                _position++;
            }
        }
    }
}
