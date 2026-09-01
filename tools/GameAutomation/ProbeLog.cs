// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using System.Text.Json;

namespace GlobalConversationTracker.Automation
{
    /// <summary>One dialogue option as the game finished composing it.</summary>
    public sealed class ProbeOption
    {
        /// <summary>Creates an option.</summary>
        /// <param name="conversationId">Its destination entry's conversation.</param>
        /// <param name="entryId">Its destination entry.</param>
        /// <param name="text">Its final text, markup and all.</param>
        public ProbeOption(int? conversationId, int? entryId, string? text)
        {
            ConversationId = conversationId;
            EntryId = entryId;
            Text = text;
        }

        /// <summary>Its destination entry's conversation.</summary>
        public int? ConversationId { get; }

        /// <summary>Its destination entry.</summary>
        public int? EntryId { get; }

        /// <summary>Its final text, with every tag the game and the mod put on it.</summary>
        public string? Text { get; }

        /// <summary>Whether it carries a look-ahead marker in a given colour.</summary>
        /// <param name="colourHtml">The colour, as the mod's config spells it.</param>
        public bool HasMarker(string colourHtml)
        {
            return ProbeLog.HasMarker(Text, colourHtml);
        }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{ConversationId}:{EntryId} {Text}";
        }
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
                    wrapped.Text("text")));
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
        /// Every probe event in a log file, in order, or none if it is not there yet.
        /// </summary>
        /// <remarks>
        /// A missing file is empty rather than an error because a run deletes the log
        /// before launching - which is what stops it reading the PREVIOUS run's events
        /// and acting on them - so there is a window, until BepInEx creates its own,
        /// where the right answer is "nothing has happened yet".
        /// </remarks>
        /// <param name="logPath">The BepInEx log.</param>
        /// <exception cref="ArgumentNullException"><paramref name="logPath"/> is null.</exception>
        public static ProbeEvent[] ReadFile(string logPath)
        {
            if (logPath == null)
            {
                throw new ArgumentNullException(nameof(logPath));
            }

            if (!File.Exists(logPath))
            {
                return Array.Empty<ProbeEvent>();
            }

            // Shared: BepInEx holds the log open for writing for the whole run.
            return Read(FilePaths.ReadShared(logPath));
        }

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
            if (colourHtml == null)
            {
                throw new ArgumentNullException(nameof(colourHtml));
            }

            return optionText != null
                && optionText.IndexOf(
                    "<color=" + colourHtml + ">" + MarkerGlyph + "</color>",
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
