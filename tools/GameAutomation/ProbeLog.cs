// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Automation
{
    /// <summary>One dialogue option as the game finished composing it.</summary>
    public sealed class ProbeOption
    {
        /// <summary>Creates an option.</summary>
        /// <param name="conversationId">Its destination entry's conversation.</param>
        /// <param name="entryId">Its destination entry.</param>
        /// <param name="text">Its final text, markup and all.</param>
        /// <param name="check">Which kind of roll it is, or null for none.</param>
        public ProbeOption(int? conversationId, int? entryId, string? text, string? check = null)
        {
            ConversationId = conversationId;
            EntryId = entryId;
            Text = text;
            Check = check;
        }

        /// <summary>Its destination entry's conversation.</summary>
        public int? ConversationId { get; }

        /// <summary>Its destination entry.</summary>
        public int? EntryId { get; }

        /// <summary>Its final text, with every tag the game and the mod put on it.</summary>
        public string? Text { get; }

        /// <summary>
        /// <c>white</c>, <c>red</c>, or null where the option rolls nothing.
        /// </summary>
        /// <remarks>
        /// The game's own answer, read off the entry by the probe rather than inferred
        /// from what the mod drew. It is what lets a suite say "a rolled check gets a
        /// Pass / Fail line and nothing else does" without naming a single entry - which
        /// matters, because which options a conversation offers is not stable from one
        /// run to the next.
        /// </remarks>
        public string? Check { get; }

        /// <summary>Whether the option rolls a white or red check.</summary>
        public bool IsRolledCheck => Check != null;

        /// <summary>Whether it carries a look-ahead marker in a given colour.</summary>
        /// <remarks>
        /// THE OPTION'S OWN LINE, NOT THE LINE BELOW IT. A check's Pass / Fail line carries
        /// markers of its own, in the same colours and the same glyphs, and asking the
        /// whole text would read one of those as the option's - so an option drawing no
        /// marker at all would still answer yes on the strength of its Pass half.
        /// </remarks>
        /// <param name="colourHtml">The colour, as the mod's config spells it.</param>
        public bool HasMarker(string colourHtml)
        {
            return ProbeLog.HasMarker(OwnLine(), colourHtml);
        }

        /// <summary>Whether the option carries a marker of this colour and glyph.</summary>
        /// <param name="colourHtml">The colour.</param>
        /// <param name="glyph">The marker itself.</param>
        public bool HasMarker(string colourHtml, string glyph)
        {
            return ProbeLog.HasMarker(OwnLine(), colourHtml, glyph);
        }

        /// <summary>
        /// The Pass / Fail line under the option, or null where it has none.
        /// </summary>
        /// <exception cref="FormatException">It has one and it is malformed.</exception>
        public ProbeBranchLine? Branches()
        {
            return ProbeLog.BranchLine(Text);
        }

        /// <summary>The option's own line, without the Pass / Fail line under it.</summary>
        public string? OwnLine()
        {
            return ProbeLog.WithoutBranchLine(Text);
        }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{ConversationId}:{EntryId} {Text}";
        }
    }

    /// <summary>One half of the Pass / Fail line drawn under a check option.</summary>
    /// <remarks>
    /// The word carries two facts, in the two ways an option itself carries them: its
    /// colour is where that outcome LANDS, and its marker is what lies BEYOND. They are
    /// read separately here because a scenario asserts them separately.
    /// </remarks>
    public sealed class ProbeBranch
    {
        /// <summary>Creates a half.</summary>
        /// <param name="word">The word, <c>Pass</c> or <c>Fail</c>.</param>
        /// <param name="colourHtml">The colour the word is drawn in.</param>
        /// <param name="markerColourHtml">The marker's colour, or null for no marker.</param>
        /// <param name="marker">The marker itself, or null for no marker.</param>
        public ProbeBranch(
            string word, string colourHtml, string? markerColourHtml, string? marker)
        {
            Word = word;
            ColourHtml = colourHtml;
            MarkerColourHtml = markerColourHtml;
            Marker = marker;
        }

        /// <summary>The word, <c>Pass</c> or <c>Fail</c>.</summary>
        public string Word { get; }

        /// <summary>The colour the word is drawn in.</summary>
        public string ColourHtml { get; }

        /// <summary>The marker's colour, or null when it carries none.</summary>
        public string? MarkerColourHtml { get; }

        /// <summary>The marker, or null when it carries none.</summary>
        public string? Marker { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return Marker == null
                ? $"{Word}[{ColourHtml}]"
                : $"{Word}[{ColourHtml}]{Marker}[{MarkerColourHtml}]";
        }
    }

    /// <summary>The Pass / Fail line an option carries, both halves of it.</summary>
    public sealed class ProbeBranchLine
    {
        /// <summary>Creates a line.</summary>
        /// <param name="pass">The outcome where the check succeeds.</param>
        /// <param name="fail">The outcome where it fails.</param>
        public ProbeBranchLine(ProbeBranch pass, ProbeBranch fail)
        {
            Pass = pass;
            Fail = fail;
        }

        /// <summary>The outcome where the check succeeds.</summary>
        public ProbeBranch Pass { get; }

        /// <summary>The outcome where it fails.</summary>
        public ProbeBranch Fail { get; }

        /// <inheritdoc/>
        public override string ToString() => $"{Pass}  {Fail}";
    }

    /// <summary>One event the probe wrote.</summary>
    public sealed class ProbeEvent
    {
        private readonly JsonElement _body;

        /// <summary>Creates an event from its JSON body.</summary>
        /// <param name="body">The parsed object.</param>
        public ProbeEvent(JsonElement body)
        {
            _body = body;
            Name = Text("event") ?? string.Empty;
        }

        /// <summary>The event name, such as "menu" or "world-ready".</summary>
        public string Name { get; }

        /// <summary>One string member, or null if the event does not carry it.</summary>
        /// <param name="key">The member name.</param>
        public string? Text(string key)
        {
            return _body.ValueKind == JsonValueKind.Object
                && _body.TryGetProperty(key, out JsonElement value)
                && value.ValueKind == JsonValueKind.String
                ? value.GetString()
                : null;
        }

        /// <summary>One numeric member, or null if it is missing or not a number.</summary>
        /// <param name="key">The member name.</param>
        public int? Number(string key)
        {
            return _body.ValueKind == JsonValueKind.Object
                && _body.TryGetProperty(key, out JsonElement value)
                && value.ValueKind == JsonValueKind.Number
                && value.TryGetInt32(out int number)
                ? number
                : (int?)null;
        }

        /// <summary>One Boolean member, or null if it is missing or not Boolean.</summary>
        /// <param name="key">The member name.</param>
        public bool? Boolean(string key)
        {
            return _body.ValueKind == JsonValueKind.Object
                && _body.TryGetProperty(key, out JsonElement value)
                && (value.ValueKind == JsonValueKind.True
                    || value.ValueKind == JsonValueKind.False)
                ? value.GetBoolean()
                : (bool?)null;
        }

        /// <summary>
        /// The options a "menu" event carried, or an empty array for any other event.
        /// </summary>
        public ProbeOption[] Options()
        {
            if (_body.ValueKind != JsonValueKind.Object
                || !_body.TryGetProperty("options", out JsonElement options)
                || options.ValueKind != JsonValueKind.Array)
            {
                return Array.Empty<ProbeOption>();
            }

            var found = new List<ProbeOption>();
            foreach (JsonElement option in options.EnumerateArray())
            {
                var wrapped = new ProbeEvent(option);
                found.Add(new ProbeOption(
                    wrapped.Number("conversation"),
                    wrapped.Number("entry"),
                    wrapped.Text("text"),
                    wrapped.Text("check")));
            }

            return found.ToArray();
        }

        /// <inheritdoc/>
        public override string ToString()
        {
            return _body.ToString();
        }
    }

    /// <summary>
    /// Reads what the in-game test probe wrote into the BepInEx log.
    /// </summary>
    /// <remarks>
    /// <para>The probe writes each event as a JSON object on its own line, bracketed by
    /// a <see cref="Begin"/> line and an <see cref="End"/> line. The brackets are what
    /// make it findable: BepInEx prefixes everything it writes with a level and a
    /// source, the file is shared with the mod and the game, and a dialogue option's
    /// text - the field these tests turn on - is arbitrary prose full of quotes,
    /// asterisks and angle brackets.</para>
    ///
    /// <para>Anything outside the brackets is skipped rather than refused, and a
    /// bracketed block that will not parse is skipped too: a log truncated mid-write by
    /// a killed game is normal, and it should cost the last event rather than the
    /// run.</para>
    /// </remarks>
    public static class ProbeLog
    {
        /// <summary>The line that opens an event.</summary>
        public const string Begin = "GCTPROBE-BEGIN";

        /// <summary>The line that closes one.</summary>
        public const string End = "GCTPROBE-END";

        /// <summary>The glyph the look-ahead appends to an option it has marked.</summary>
        public const string MarkerGlyph = "*";

        /// <summary>
        /// The marker for an option whose look-ahead ran out of budget.
        /// </summary>
        /// <remarks>
        /// A different GLYPH as well as a different colour, and both halves have to be
        /// matched. Looking only for the colour would be looser than the mod is, and
        /// looking only for the asterisk read this as an ordinary marker - which is how a
        /// suite reported a plain option while the game had drawn a grey '*?' on it.
        /// </remarks>
        public const string UncertainMarkerGlyph = "*?";

        /// <summary>The word naming the outcome where a check succeeds.</summary>
        /// <remarks>
        /// RESTATED HERE rather than shared with <c>BranchLine</c> in the Engine assembly,
        /// like the glyphs above and for the same reason: this reads the game's output as
        /// a black box. A mod that renamed the word should FAIL these suites, which is
        /// what it would do here and is not what it would do if both sides read one
        /// constant.
        /// </remarks>
        public const string BranchPassWord = "Pass";

        /// <summary>The word naming the outcome where it fails.</summary>
        public const string BranchFailWord = "Fail";

        /// <summary>One run of text in one colour, as the game's markup spells it.</summary>
        private static readonly Regex ColouredSpan = new Regex(
            "<color=(?<colour>[^>]+)>(?<text>[^<]*)</color>", RegexOptions.Compiled);

        /// <summary>Every probe event in some text, in order.</summary>
        /// <param name="text">Log text.</param>
        /// <exception cref="ArgumentNullException"><paramref name="text"/> is null.</exception>
        public static ProbeEvent[] Read(string text)
        {
            if (text == null)
            {
                throw new ArgumentNullException(nameof(text));
            }

            var events = new List<ProbeEvent>();
            var payload = new StringBuilder();
            bool inside = false;

            foreach (string raw in text.Split('\n'))
            {
                string line = raw.TrimEnd('\r');

                if (line.IndexOf(Begin, StringComparison.Ordinal) >= 0)
                {
                    // A second Begin without an End means the first block was cut off.
                    inside = true;
                    payload.Clear();
                    continue;
                }

                if (!inside)
                {
                    continue;
                }

                if (line.IndexOf(End, StringComparison.Ordinal) >= 0)
                {
                    inside = false;
                    ProbeEvent? parsed = Parse(payload.ToString());
                    if (parsed != null)
                    {
                        events.Add(parsed);
                    }

                    payload.Clear();
                    continue;
                }

                payload.Append(StripLogPrefix(line));
            }

            return events.ToArray();
        }

        /// <summary>
        /// Whether an option's text carries a look-ahead marker in a given colour.
        /// </summary>
        /// <remarks>
        /// The mod appends the marker as a coloured span, and the colour is the answer:
        /// one colour means the option leads somewhere no save has reached, the other
        /// somewhere this save has not. Matching the whole span rather than hunting for
        /// the glyph is what keeps an asterisk that is simply part of the writing from
        /// being read as a marker - and this game's prose is full of them.
        /// </remarks>
        /// <param name="optionText">An option's final text.</param>
        /// <param name="colourHtml">The colour, as the mod's config spells it.</param>
        /// <exception cref="ArgumentNullException"><paramref name="colourHtml"/> is null.</exception>
        public static bool HasMarker(string? optionText, string colourHtml)
        {
            return HasMarker(optionText, colourHtml, MarkerGlyph);
        }

        /// <summary>
        /// Whether an option's text carries a marker of a given colour AND glyph.
        /// </summary>
        /// <param name="optionText">An option's final text.</param>
        /// <param name="colourHtml">The colour, as the mod's config spells it.</param>
        /// <param name="glyph">The marker itself.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static bool HasMarker(string? optionText, string colourHtml, string glyph)
        {
            if (colourHtml == null)
            {
                throw new ArgumentNullException(nameof(colourHtml));
            }

            if (glyph == null)
            {
                throw new ArgumentNullException(nameof(glyph));
            }

            return optionText != null
                && optionText.IndexOf(
                    "<color=" + colourHtml + ">" + glyph + "</color>",
                    StringComparison.OrdinalIgnoreCase) >= 0;
        }

        private static ProbeEvent? Parse(string json)
        {
            if (json.Length == 0)
            {
                return null;
            }

            try
            {
                using var document = JsonDocument.Parse(json);
                // Cloned because the element does not outlive its document.
                return new ProbeEvent(document.RootElement.Clone());
            }
            catch (JsonException)
            {
                // A log cut off mid-write by a killed game, most likely. Losing the
                // last event is the right price; refusing the whole log is not.
                return null;
            }
        }

        /// <summary>
        /// The Pass / Fail line under an option, or null where it has none.
        /// </summary>
        /// <remarks>
        /// <para>The mod appends the line to the option's own text, beginning with a
        /// newline, so the last line of an option is where it is if it is anywhere. A
        /// SECOND line is what it is: the line is drawn under a check, whose own text sits
        /// on a coloured background where the mod's three colours stop being legible.</para>
        ///
        /// <para>An option with no <see cref="BranchPassWord"/> in its last line has no
        /// line, and that is an answer rather than a failure - most options are not rolled
        /// checks. Having one that will not parse IS a failure, and a loud one: it means
        /// the mod drew something this cannot read, which is exactly what a suite asserting
        /// on it must not paper over.</para>
        /// </remarks>
        /// <param name="optionText">An option's final text.</param>
        /// <exception cref="FormatException">There is a line and it is malformed.</exception>
        public static ProbeBranchLine? BranchLine(string? optionText)
        {
            string? tail = LastLine(optionText);
            if (tail == null || tail.IndexOf(BranchPassWord, StringComparison.Ordinal) < 0)
            {
                return null;
            }

            var spans = new List<(string Colour, string Text)>();
            foreach (Match match in ColouredSpan.Matches(tail))
            {
                spans.Add((match.Groups["colour"].Value, match.Groups["text"].Value));
            }

            int at = 0;
            ProbeBranch pass = ReadHalf(BranchPassWord, spans, ref at, tail);
            ProbeBranch fail = ReadHalf(BranchFailWord, spans, ref at, tail);
            if (at != spans.Count)
            {
                throw new FormatException(
                    $"'{tail}' carries {spans.Count - at} coloured span(s) after its "
                    + $"{BranchFailWord} half.");
            }

            return new ProbeBranchLine(pass, fail);
        }

        /// <summary>An option's text with any Pass / Fail line taken off it.</summary>
        /// <param name="optionText">An option's final text.</param>
        public static string? WithoutBranchLine(string? optionText)
        {
            if (optionText == null || BranchLine(optionText) == null)
            {
                return optionText;
            }

            int lastLine = optionText.LastIndexOf('\n');
            return lastLine < 0 ? optionText : optionText.Substring(0, lastLine);
        }

        /// <summary>One half of the line: its word, its colour, and its marker if any.</summary>
        private static ProbeBranch ReadHalf(
            string word, List<(string Colour, string Text)> spans, ref int at, string line)
        {
            if (at >= spans.Count || spans[at].Text != word)
            {
                throw new FormatException(
                    $"'{line}' does not carry '{word}' where one is expected"
                    + (at < spans.Count ? $"; found '{spans[at].Text}'." : "."));
            }

            string colour = spans[at].Colour;
            at++;

            if (at >= spans.Count
                || (spans[at].Text != MarkerGlyph && spans[at].Text != UncertainMarkerGlyph))
            {
                return new ProbeBranch(word, colour, null, null);
            }

            var marker = spans[at];
            at++;
            return new ProbeBranch(word, colour, marker.Colour, marker.Text);
        }

        /// <summary>The last line of a text, or null when there is only one.</summary>
        private static string? LastLine(string? text)
        {
            int lastLine = text?.LastIndexOf('\n') ?? -1;
            return lastLine < 0 ? null : text!.Substring(lastLine + 1);
        }

        /// <summary>
        /// Drops BepInEx's own "[Level:Source] " prefix from a payload line.
        /// </summary>
        /// <remarks>
        /// The probe writes its JSON as a single log call, so BepInEx puts a prefix in
        /// front of it. Cutting at the first "] " would be wrong for any line whose JSON
        /// legitimately contains one, so the prefix is only removed when the line starts
        /// with it and the remainder starts a JSON object.
        /// </remarks>
        private static string StripLogPrefix(string line)
        {
            string trimmed = line.TrimStart();
            if (trimmed.Length == 0 || trimmed[0] != '[')
            {
                return trimmed;
            }

            int close = trimmed.IndexOf("] ", StringComparison.Ordinal);
            if (close < 0)
            {
                return trimmed;
            }

            string rest = trimmed.Substring(close + 2).TrimStart();
            return rest.Length > 0 && (rest[0] == '{' || rest[0] == '[') ? rest : trimmed;
        }
    }
}
